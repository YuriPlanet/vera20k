//! VERA-only, experimental (`docs/zoom-one-pass.md`, "picture streaming"):
//! the sprite atlas's pictures at their own resolution, streamed in while
//! the camera zooms in, as a 3D game streams its textures.
//!
//! The atlas keeps every picture frame at game pixels for good
//! (`picture_atlas::GamePictureFrames::page_sprite`), which is all a camera
//! at zoom 1 or below samples. Above zoom 1 that page would be magnified, so
//! [`PictureStream::entry`] (through `SpriteAtlas::get`) records each frame
//! drawn from art of a higher resolution as wanted, and hands out its HD
//! entry once the frame is on an HD page: the same entry with only its page
//! and UV changed. Once a frame, [`PictureStream::frame`] (the frame's
//! `SpriteAtlas::stream_pictures` hook) groups the wanted frames by file and
//! hands each file to a decode worker, which reads the file a frame at a
//! time (`picture_atlas::PictureFrameStream`) up to the last frame wanted
//! and builds each wanted frame's HD tile (`picture_atlas::hd_page_sprite`
//! and its mips); the hook then uploads a bounded number of bytes of
//! finished tiles a frame. HD pages are a pool of fixed-size pages within a
//! byte budget; when it is full the least recently drawn page is emptied and
//! reused, its frames back on their game-pixel entries.
//!
//! The browser build has no threads to decode on, so it draws the
//! game-pixel pages at every zoom.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};

use crate::render::batch::BatchRenderer;
use crate::render::picture_atlas::{
    HdSource, HouseColor, PAGE_GAP, PageSprite, PictureFile, PictureFrameId, PictureFrameStream,
    PicturePage, ZOOM_OUT_LEVELS, hd_page_sprite, page_levels, page_scale,
};
use crate::render::picture_zoom::PictureHdPage;
use crate::render::sprite_atlas::{ShpSpriteEntry, ShpSpriteKey, SpriteAtlasPage};

/// A frame of art above 1x, as [`Slot::state`] holds it.
const IDLE: u8 = 0;
/// Drawn while zoomed in, on the demand list for the next hook.
const WANTED: u8 = 1;
/// In a decode job, or decoded and waiting for its upload.
const PENDING: u8 = 2;
/// On an HD page: its keys have HD entries.
const RESIDENT: u8 = 3;
/// No HD copy: too large for a page, or its file could not be read again.
const FAILED: u8 = 4;
/// Decoded when no page had room; wanted again after the next eviction or
/// summary.
const NO_ROOM: u8 = 5;

/// The HD pages' byte budget without `VERA_PICTURE_HD_BUDGET_MB`.
const DEFAULT_BUDGET_MB: u64 = 1024;
/// An HD page's side in texels (or the device's limit): about 36 MiB with
/// its mips, index plane and house layer, so a full budget is a few dozen
/// pages and an eviction frees a small part of it. 512 game pixels for 4x
/// art, 256 for 8x; a larger frame keeps its game-pixel entry.
const PAGE_SIDE: u32 = 2048;
/// Texel bytes of finished tiles uploaded a frame (level 0, mips, index
/// plane and house layer together).
const UPLOAD_BYTES_PER_FRAME: u64 = 16 << 20;
/// Decode workers, and the most file jobs sent and not finished.
const WORKERS: usize = 2;
const JOBS_IN_FLIGHT: usize = 2 * WORKERS;
/// Finished tiles waiting for the hook: a worker waits while this many are,
/// so decoded art is never held unbounded.
const READY_TILES: usize = 8;
/// Frames between two cache summaries in the log.
const SUMMARY_FRAMES: u64 = 600;
/// Whether this build can decode on threads of its own: not the browser's.
const THREADS: bool = !cfg!(all(
    feature = "wasm",
    target_arch = "wasm32",
    target_os = "unknown"
));

/// The streamed HD copies of the sprite atlas's picture frames: what each
/// frame and key has, the demand the draws record, the decode workers and
/// the HD page pool. Owned by the sprite atlas.
pub(crate) struct PictureStream {
    files: Vec<StreamFile>,
    file_index: HashMap<PictureFile, usize>,
    slots: Vec<Slot>,
    slot_index: HashMap<PictureFrameId, u32>,
    /// Every key drawn from a slot's frame: its slot, and its HD entry and
    /// pool page while the slot is resident.
    keys: HashMap<ShpSpriteKey, KeyStream>,
    /// Slots [`Self::entry`] found idle since the last hook.
    demand: Mutex<Vec<u32>>,
    /// Wanted slots not yet in a job (their file's job is still running, or
    /// too many jobs are).
    wanted: Vec<u32>,
    /// Slots that found no room, wanted again after an eviction.
    no_room: Vec<u32>,
    /// Each running job's slots, by file.
    jobs: HashMap<usize, Vec<u32>>,
    /// The camera zooms in: HD entries are handed out and demand recorded.
    active: bool,
    /// Hooks so far: the clock of the pages' last use.
    frame: u64,
    pool: PagePool,
    /// The GPU side of each of `pool`'s pages.
    gpu: Vec<PictureHdPage>,
    worker: Option<Worker>,
    /// The workers could not be started: no HD.
    no_worker: bool,
}

/// A file whose frames stream at their own resolution.
struct StreamFile {
    name: String,
    source: Arc<HdSource>,
    /// A job for it is running.
    busy: bool,
    /// Its frames too large for a page were logged.
    told_too_large: bool,
}

/// One frame of a file above 1x art as the atlas draws it.
struct Slot {
    file: usize,
    frame: usize,
    deferred: bool,
    /// Its drawn rectangle in game pixels, the game-pixel entry's.
    origin: [u32; 2],
    size: [u32; 2],
    keys: Vec<ShpSpriteKey>,
    state: AtomicU8,
    /// While resident: its pool page and its tile's corner there, in game
    /// pixels.
    place: Option<(usize, [u32; 2])>,
}

struct KeyStream {
    slot: u32,
    hd: Option<(ShpSpriteEntry, usize)>,
}

impl Default for PictureStream {
    fn default() -> Self {
        let budget = std::env::var("VERA_PICTURE_HD_BUDGET_MB")
            .ok()
            .and_then(|value| value.trim().parse::<u64>().ok())
            .unwrap_or(DEFAULT_BUDGET_MB);
        Self {
            files: Vec::new(),
            file_index: HashMap::new(),
            slots: Vec::new(),
            slot_index: HashMap::new(),
            keys: HashMap::new(),
            demand: Mutex::new(Vec::new()),
            wanted: Vec::new(),
            no_room: Vec::new(),
            jobs: HashMap::new(),
            active: false,
            frame: 0,
            pool: PagePool::new(budget.saturating_mul(1 << 20), PAGE_SIDE),
            gpu: Vec::new(),
            worker: None,
            no_worker: false,
        }
    }
}

impl PictureStream {
    /// Take `file`'s frames as streamable from `source` (once per file), on
    /// a device whose textures are at most `max_side` texels a side.
    pub(crate) fn add_file(&mut self, file: PictureFile, source: HdSource, max_side: u32) {
        self.pool.max_side = max_side;
        if self.file_index.contains_key(&file) {
            return;
        }
        self.file_index.insert(file.clone(), self.files.len());
        self.files.push(StreamFile {
            name: file.0,
            source: Arc::new(source),
            busy: false,
            told_too_large: false,
        });
    }

    /// Take frame `id`, drawn in `origin`, `size` (game pixels) on the
    /// atlas's pages, as streamable when its file is ([`Self::add_file`]).
    pub(crate) fn add_frame(&mut self, id: &PictureFrameId, origin: [u32; 2], size: [u32; 2]) {
        if self.slot_index.contains_key(id) {
            return;
        }
        let Some(&file) = self.file_index.get(&(id.file.clone(), id.size)) else {
            return;
        };
        let scale = page_scale(self.files[file].source.scale);
        let fits = self.pool.fits(scale, tile_extent(size));
        if !fits && !self.files[file].told_too_large {
            self.files[file].told_too_large = true;
            log::info!(
                "Picture HD: {}'s frames over {} game pixels stay at game pixels zoomed in",
                id.file,
                (self.pool.side(scale) / scale).saturating_sub(2 * PAGE_GAP)
            );
        }
        let Ok(slot) = u32::try_from(self.slots.len()) else {
            return;
        };
        self.slot_index.insert(id.clone(), slot);
        self.slots.push(Slot {
            file,
            frame: id.frame,
            deferred: id.deferred,
            origin,
            size,
            keys: Vec::new(),
            state: AtomicU8::new(if fits { IDLE } else { FAILED }),
            place: None,
        });
    }

    /// Take atlas key `key`, drawn from `frame` with the game-pixel entry
    /// `entry`, when the frame streams ([`Self::add_frame`]).
    pub(crate) fn add_key(
        &mut self,
        key: &ShpSpriteKey,
        frame: &PictureFrameId,
        entry: &ShpSpriteEntry,
    ) {
        let Some(&slot) = self.slot_index.get(frame) else {
            return;
        };
        if self.keys.contains_key(key) {
            return;
        }
        self.slots[slot as usize].keys.push(key.clone());
        let hd = self.slots[slot as usize]
            .place
            .map(|(page, at)| (self.hd_entry(slot, page, at, entry), page));
        self.keys.insert(key.clone(), KeyStream { slot, hd });
    }

    /// `key`'s HD entry while the camera zooms in and its frame is on an HD
    /// page; a frame not there yet is recorded as wanted for the next
    /// [`Self::frame`]. None otherwise: the game-pixel entry is drawn.
    pub(crate) fn entry(&self, key: &ShpSpriteKey) -> Option<&ShpSpriteEntry> {
        if !self.active {
            return None;
        }
        let stream = self.keys.get(key)?;
        if let Some((entry, page)) = &stream.hd {
            self.pool.pages[*page]
                .used
                .store(self.frame, Ordering::Relaxed);
            return Some(entry);
        }
        if self.slots[stream.slot as usize]
            .state
            .compare_exchange(IDLE, WANTED, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            self.demand
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(stream.slot);
        }
        None
    }

    /// The frame's hook, before the world's instances are built: with the
    /// camera zoomed in (`camera_zoom` above 1), start decode jobs for the
    /// frames drawn since the last call, upload finished tiles within the
    /// frame's budget onto HD pages (`pages`, the atlas's, gains a page when
    /// the pool does; `picture_pages` marks it) and hand out their entries,
    /// made from the game-pixel `entries`. At zoom 1 or below nothing is
    /// handed out, wanted frames are forgotten and running jobs wait.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        pages: &mut Vec<SpriteAtlasPage>,
        picture_pages: &mut HashSet<usize>,
        entries: &HashMap<ShpSpriteKey, ShpSpriteEntry>,
        camera_zoom: f32,
    ) {
        self.frame += 1;
        self.active = THREADS && !self.no_worker && camera_zoom > 1.0 && !self.slots.is_empty();
        let demand =
            std::mem::take(&mut *self.demand.lock().unwrap_or_else(PoisonError::into_inner));
        if !self.active {
            for slot in demand.into_iter().chain(self.wanted.drain(..)) {
                let _ = self.slots[slot as usize].state.compare_exchange(
                    WANTED,
                    IDLE,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
            }
            self.summary();
            return;
        }
        self.wanted.extend(demand);
        self.dispatch();
        // A capture (`hd_switch::capture_zoom`) shows few frames: it waits
        // for everything its frame drew.
        let capture = super::hd_switch::capture_zoom().is_some();
        let budget = if capture {
            u64::MAX
        } else {
            UPLOAD_BYTES_PER_FRAME
        };
        let mut uploaded = 0;
        while uploaded < budget {
            let Some(worker) = self.worker.as_mut() else {
                break;
            };
            let results = worker
                .results
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner);
            let message = if capture && !(self.jobs.is_empty() && self.wanted.is_empty()) {
                results.recv().ok()
            } else {
                results.try_recv().ok()
            };
            let Some(message) = message else {
                break;
            };
            uploaded += self.handle(message, device, queue, batch, pages, picture_pages, entries);
            if capture {
                self.dispatch();
            }
        }
        self.summary();
    }

    /// Hand the wanted slots of every file with no running job to the
    /// workers, one job a file, at most [`JOBS_IN_FLIGHT`] at once.
    fn dispatch(&mut self) {
        if self.wanted.is_empty() {
            return;
        }
        if self.worker.is_none() {
            self.worker = Worker::spawn();
            if self.worker.is_none() {
                log::warn!(
                    "Picture HD: no decode thread could start; pictures stay at game pixels"
                );
                self.no_worker = true;
                self.active = false;
                return;
            }
        }
        let mut by_file: std::collections::BTreeMap<usize, Vec<u32>> = Default::default();
        for slot in std::mem::take(&mut self.wanted) {
            if self.slots[slot as usize].state.load(Ordering::Relaxed) == WANTED {
                by_file
                    .entry(self.slots[slot as usize].file)
                    .or_default()
                    .push(slot);
            }
        }
        for (file, mut slots) in by_file {
            if self.files[file].busy || self.jobs.len() >= JOBS_IN_FLIGHT {
                self.wanted.extend(slots);
                continue;
            }
            slots.sort_by_key(|&slot| self.slots[slot as usize].frame);
            let frames = slots
                .iter()
                .map(|&slot| {
                    let it = &self.slots[slot as usize];
                    it.state.store(PENDING, Ordering::Relaxed);
                    JobFrame {
                        slot,
                        frame: it.frame,
                        deferred: it.deferred,
                        origin: it.origin,
                        size: it.size,
                    }
                })
                .collect();
            let job = Job {
                file,
                source: self.files[file].source.clone(),
                frames,
            };
            let Some(worker) = &self.worker else {
                return;
            };
            if worker.jobs.send(job).is_err() {
                // The workers are gone: nothing streams any more.
                for &slot in &slots {
                    self.slots[slot as usize]
                        .state
                        .store(FAILED, Ordering::Relaxed);
                }
                self.no_worker = true;
                return;
            }
            self.files[file].busy = true;
            self.jobs.insert(file, slots);
        }
    }

    /// Take one worker message: upload a tile (its bytes returned), or
    /// finish a job.
    #[allow(clippy::too_many_arguments)]
    fn handle(
        &mut self,
        message: Message,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        pages: &mut Vec<SpriteAtlasPage>,
        picture_pages: &mut HashSet<usize>,
        entries: &HashMap<ShpSpriteKey, ShpSpriteEntry>,
    ) -> u64 {
        match message {
            Message::Tile { slot, tile } => {
                let bytes = tile.bytes();
                self.upload(
                    device,
                    queue,
                    batch,
                    pages,
                    picture_pages,
                    entries,
                    slot,
                    tile,
                );
                bytes
            }
            Message::Done {
                file,
                decoded,
                millis,
                error,
            } => {
                self.files[file].busy = false;
                // A slot the job did not reach (past the file's frames, or
                // an error) keeps its game-pixel entry.
                for slot in self.jobs.remove(&file).unwrap_or_default() {
                    let _ = self.slots[slot as usize].state.compare_exchange(
                        PENDING,
                        FAILED,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    );
                }
                let name = &self.files[file].name;
                match error {
                    None => {
                        log::info!("Picture HD: {name} decoded to frame {decoded} in {millis} ms")
                    }
                    Some(error) => log::warn!("Picture HD: {name} cannot be read again: {error}"),
                }
                0
            }
        }
    }

    /// Put `slot`'s finished `tile` on an HD page and hand out its keys' HD
    /// entries; with no room, the slot waits for an eviction.
    #[allow(clippy::too_many_arguments)]
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        pages: &mut Vec<SpriteAtlasPage>,
        picture_pages: &mut HashSet<usize>,
        entries: &HashMap<ShpSpriteKey, ShpSpriteEntry>,
        slot: u32,
        tile: HdTile,
    ) {
        if self.slots[slot as usize].state.load(Ordering::Relaxed) != PENDING {
            return;
        }
        let can_add = u8::try_from(pages.len()).is_ok();
        let Some((page, at, fresh)) = self
            .pool
            .place(tile.scale, tile.extent, self.frame, can_add)
        else {
            self.slots[slot as usize]
                .state
                .store(NO_ROOM, Ordering::Relaxed);
            self.no_room.push(slot);
            return;
        };
        let (side, scale) = (self.pool.pages[page].side, self.pool.pages[page].scale);
        match fresh {
            Fresh::Kept => {}
            Fresh::Created => {
                let hd = PictureHdPage::empty(device, side, page_levels(scale));
                self.pool.pages[page].atlas_page = pages.len();
                picture_pages.insert(pages.len());
                pages.push(SpriteAtlasPage {
                    texture: hd.texture(device, batch),
                });
                self.gpu.push(hd);
            }
            Fresh::Cleared(slots) => {
                self.revert(&slots);
            }
            Fresh::Replaced(slots) => {
                self.revert(&slots);
                let hd = PictureHdPage::empty(device, side, page_levels(scale));
                pages[self.pool.pages[page].atlas_page].texture = hd.texture(device, batch);
                self.gpu[page] = hd;
            }
        }
        self.gpu[page].write(
            queue,
            at.map(|side| side * scale),
            tile.extent.map(|side| side * scale),
            tile.levels,
            &tile.color,
            &tile.house,
            &tile.indices,
        );
        self.set_resident(slot, page, at, entries);
    }

    /// `slot` is on pool page `page` with its tile at `at`: its keys get HD
    /// entries.
    fn set_resident(
        &mut self,
        slot: u32,
        page: usize,
        at: [u32; 2],
        entries: &HashMap<ShpSpriteKey, ShpSpriteEntry>,
    ) {
        self.pool.pages[page].slots.push(slot);
        self.slots[slot as usize].place = Some((page, at));
        self.slots[slot as usize]
            .state
            .store(RESIDENT, Ordering::Relaxed);
        for index in 0..self.slots[slot as usize].keys.len() {
            let key = &self.slots[slot as usize].keys[index];
            let Some(entry) = entries.get(key) else {
                continue;
            };
            let hd = self.hd_entry(slot, page, at, entry);
            if let Some(stream) = self.keys.get_mut(key) {
                stream.hd = Some((hd, page));
            }
        }
    }

    /// The evicted `slots` keep their game-pixel entries again; the slots
    /// that found no room are wanted again.
    fn revert(&mut self, slots: &[u32]) {
        for &slot in slots {
            let it = &mut self.slots[slot as usize];
            it.place = None;
            it.state.store(IDLE, Ordering::Relaxed);
            for key in &it.keys {
                if let Some(stream) = self.keys.get_mut(key) {
                    stream.hd = None;
                }
            }
        }
        for slot in std::mem::take(&mut self.no_room) {
            let _ = self.slots[slot as usize].state.compare_exchange(
                NO_ROOM,
                IDLE,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
    }

    /// `entry` (a key of `slot`) on pool page `page` with the slot's tile at
    /// `at`: everything but its page and UV as the game-pixel entry.
    fn hd_entry(
        &self,
        slot: u32,
        page: usize,
        at: [u32; 2],
        entry: &ShpSpriteEntry,
    ) -> ShpSpriteEntry {
        let pool_page = &self.pool.pages[page];
        let extent = (pool_page.side / pool_page.scale) as f32;
        let size = self.slots[slot as usize].size;
        ShpSpriteEntry {
            uv_origin: at.map(|side| (side + PAGE_GAP) as f32 / extent),
            uv_size: size.map(|side| side as f32 / extent),
            page: u8::try_from(pool_page.atlas_page).unwrap_or(u8::MAX),
            ..*entry
        }
    }

    /// Every [`SUMMARY_FRAMES`] hooks, the cache in one log line while it
    /// holds or waits for anything; the slots that found no room are wanted
    /// again.
    fn summary(&mut self) {
        if !self.frame.is_multiple_of(SUMMARY_FRAMES) {
            return;
        }
        for slot in std::mem::take(&mut self.no_room) {
            let _ = self.slots[slot as usize].state.compare_exchange(
                NO_ROOM,
                IDLE,
                Ordering::Relaxed,
                Ordering::Relaxed,
            );
        }
        if self.pool.pages.is_empty() && self.jobs.is_empty() && self.wanted.is_empty() {
            return;
        }
        let resident = self
            .slots
            .iter()
            .filter(|slot| slot.state.load(Ordering::Relaxed) == RESIDENT)
            .count();
        let pending = self
            .slots
            .iter()
            .filter(|slot| slot.state.load(Ordering::Relaxed) == PENDING)
            .count();
        log::info!(
            "Picture HD cache: {} pages, {} of {} MiB, {resident} frames HD, {pending} decoding, {} wanted",
            self.pool.pages.len(),
            self.pool.bytes() >> 20,
            self.pool.budget >> 20,
            self.wanted.len()
        );
    }
}

/// The game pixels a frame of `size` takes on an HD page: its rectangle,
/// [`PAGE_GAP`] around it, rounded up to whole texels of the last mip level
/// so every level's rectangle is whole.
fn tile_extent(size: [u32; 2]) -> [u32; 2] {
    size.map(|side| (side + 2 * PAGE_GAP).next_multiple_of(1 << ZOOM_OUT_LEVELS))
}

/// One frame's HD tile as a worker builds it: [`tile_extent`] game pixels
/// at `scale` texels each with the frame at `PAGE_GAP` in, its colour and
/// house layer with their mip chains (`PicturePage::mip_chain`, level 0
/// first) and its level-0 index plane. Written whole, gap included, so a
/// reused page keeps nothing of the frames it held.
struct HdTile {
    scale: u32,
    extent: [u32; 2],
    levels: u32,
    color: Vec<u8>,
    house: Vec<u8>,
    indices: Vec<u8>,
}

impl HdTile {
    fn new(sprite: &PageSprite) -> Self {
        let extent = tile_extent(sprite.size);
        let page = PicturePage::compose(sprite.scale, extent, u32::MAX, &[(sprite, [PAGE_GAP; 2])]);
        let (color, levels) = page.mip_chain();
        // Two bytes a texel to the colour's four, level for level.
        let house = page
            .house_mip_chain()
            .unwrap_or_else(|| vec![0; color.len() / 2]);
        Self {
            scale: sprite.scale,
            extent,
            levels,
            color,
            house,
            indices: page.indices,
        }
    }

    fn bytes(&self) -> u64 {
        (self.color.len() + self.house.len() + self.indices.len()) as u64
    }
}

/// A decode job: the wanted frames of one file, in frame order.
struct Job {
    file: usize,
    source: Arc<HdSource>,
    frames: Vec<JobFrame>,
}

struct JobFrame {
    slot: u32,
    frame: usize,
    deferred: bool,
    origin: [u32; 2],
    size: [u32; 2],
}

enum Message {
    Tile {
        slot: u32,
        tile: HdTile,
    },
    /// A job's end: the frames decoded, how long it took, and why it
    /// stopped early.
    Done {
        file: usize,
        decoded: usize,
        millis: u128,
        error: Option<String>,
    },
}

/// The decode threads: jobs in, tiles and job ends out (at most
/// [`READY_TILES`] waiting).
struct Worker {
    jobs: mpsc::Sender<Job>,
    results: Mutex<mpsc::Receiver<Message>>,
}

impl Worker {
    /// [`WORKERS`] threads, None when none starts. They end when the stream
    /// (the atlas) is dropped.
    fn spawn() -> Option<Self> {
        if !THREADS {
            return None;
        }
        let (jobs, job_queue) = mpsc::channel::<Job>();
        let (sender, results) = mpsc::sync_channel::<Message>(READY_TILES);
        let job_queue = Arc::new(Mutex::new(job_queue));
        let mut started = false;
        for index in 0..WORKERS {
            let (job_queue, sender) = (job_queue.clone(), sender.clone());
            let spawned = std::thread::Builder::new()
                .name(format!("picture-hd-{index}"))
                .spawn(move || {
                    loop {
                        let job = job_queue
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .recv();
                        let Ok(job) = job else {
                            return;
                        };
                        if !run_job(job, &sender) {
                            return;
                        }
                    }
                });
            started |= spawned.is_ok();
        }
        started.then(|| Self {
            jobs,
            results: Mutex::new(results),
        })
    }
}

/// Run `job`, sending its tiles and its end to `results`; false once the
/// stream is gone.
fn run_job(job: Job, results: &mpsc::SyncSender<Message>) -> bool {
    let started = std::time::Instant::now();
    let mut open = true;
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        stream_job(&job, results, &mut open)
    }));
    let (decoded, error) = match outcome {
        Ok(Ok(decoded)) => (decoded, None),
        Ok(Err((decoded, error))) => (decoded, Some(error.to_string())),
        Err(_) => (0, Some("its decoder stopped".to_owned())),
    };
    open && results
        .send(Message::Done {
            file: job.file,
            decoded,
            millis: started.elapsed().as_millis(),
            error,
        })
        .is_ok()
}

/// Read `job`'s file a frame at a time, mask in lockstep, up to its last
/// wanted frame, sending each wanted frame's tile as it comes; the frames
/// read, or how far it got and why it stopped. `open` turns false when the
/// stream is gone.
fn stream_job(
    job: &Job,
    results: &mpsc::SyncSender<Message>,
    open: &mut bool,
) -> Result<usize, (usize, crate::assets::picture_file::PictureError)> {
    let source = &job.source;
    let mut stream = PictureFrameStream::new(&source.bytes).map_err(|error| (0, error))?;
    let (mut decoded, mut next) = (0, 0);
    while next < job.frames.len() {
        let Some(frame) = stream.next_frame() else {
            break;
        };
        let (rgba, mask) = frame.map_err(|error| (decoded, error))?;
        let index = decoded;
        decoded += 1;
        while let Some(wanted) = job.frames.get(next)
            && wanted.frame == index
        {
            let house = if wanted.deferred {
                HouseColor::Deferred
            } else {
                HouseColor::Own
            };
            let sprite = hd_page_sprite(
                rgba,
                mask,
                source.canvas,
                source.scale,
                wanted.origin,
                wanted.size,
                house,
            );
            let tile = HdTile::new(&sprite);
            drop(sprite);
            if results
                .send(Message::Tile {
                    slot: wanted.slot,
                    tile,
                })
                .is_err()
            {
                *open = false;
                return Ok(decoded);
            }
            next += 1;
        }
    }
    Ok(decoded)
}

/// What [`PagePool::place`] did to the page it placed on.
#[derive(Debug, PartialEq, Eq)]
enum Fresh {
    /// It had room.
    Kept,
    /// A new page: its textures are to be made.
    Created,
    /// The least recently drawn page of this scale, emptied: these slots
    /// were on it.
    Cleared(Vec<u32>),
    /// The least recently drawn page, of another scale, emptied and made
    /// again at this one.
    Replaced(Vec<u32>),
}

/// The HD pages' bookkeeping, apart from their textures: each page's scale,
/// shelf, frames and last use, within a byte budget.
struct PagePool {
    pages: Vec<PoolPage>,
    budget: u64,
    /// Texels a page side, before the device's limit.
    page_side: u32,
    max_side: u32,
}

struct PoolPage {
    /// Texels per game pixel.
    scale: u32,
    /// Texels a side.
    side: u32,
    shelf: Shelf,
    slots: Vec<u32>,
    /// The hook clock when a frame on it was last drawn
    /// ([`PictureStream::entry`]) or placed.
    used: AtomicU64,
    bytes: u64,
    /// Its index among the sprite atlas's pages.
    atlas_page: usize,
}

impl PoolPage {
    fn new(scale: u32, side: u32, frame: u64) -> Self {
        Self {
            scale,
            side,
            shelf: Shelf::new(side / scale),
            slots: Vec::new(),
            used: AtomicU64::new(frame),
            bytes: page_bytes(side, scale),
            atlas_page: usize::MAX,
        }
    }

    fn used(&self) -> u64 {
        self.used.load(Ordering::Relaxed)
    }
}

/// An HD page's texture bytes: colour (4) and house layer (2) with their
/// mip levels, index plane (1) at level 0.
fn page_bytes(side: u32, scale: u32) -> u64 {
    let side = u64::from(side);
    let levels: u64 = (0..page_levels(scale))
        .map(|level| (side >> level) * (side >> level))
        .sum();
    levels * 6 + side * side
}

impl PagePool {
    fn new(budget: u64, page_side: u32) -> Self {
        Self {
            pages: Vec::new(),
            budget,
            page_side,
            max_side: page_side,
        }
    }

    /// A page's side in texels.
    fn side(&self, _scale: u32) -> u32 {
        self.page_side.min(self.max_side)
    }

    /// Whether a tile of `extent` game pixels at `scale` fits on a page.
    fn fits(&self, scale: u32, extent: [u32; 2]) -> bool {
        let side = self.side(scale) / scale;
        extent.iter().all(|&extent| extent <= side)
    }

    fn bytes(&self) -> u64 {
        self.pages.iter().map(|page| page.bytes).sum()
    }

    /// A place for a tile of `extent` game pixels at `scale` at hook
    /// `frame`: a page of that scale with room, else a new page while the
    /// budget allows and `can_add`, else the least recently drawn page not
    /// drawn in this or the last frame, emptied (and made again at `scale`
    /// when it had another). None when none fits.
    fn place(
        &mut self,
        scale: u32,
        extent: [u32; 2],
        frame: u64,
        can_add: bool,
    ) -> Option<(usize, [u32; 2], Fresh)> {
        if !self.fits(scale, extent) {
            return None;
        }
        for (index, page) in self.pages.iter_mut().enumerate() {
            if page.scale == scale
                && let Some(at) = page.shelf.alloc(extent)
            {
                page.used.store(frame, Ordering::Relaxed);
                return Some((index, at, Fresh::Kept));
            }
        }
        let side = self.side(scale);
        if can_add && self.bytes() + page_bytes(side, scale) <= self.budget {
            let mut page = PoolPage::new(scale, side, frame);
            let at = page.shelf.alloc(extent)?;
            self.pages.push(page);
            return Some((self.pages.len() - 1, at, Fresh::Created));
        }
        let victim = (0..self.pages.len())
            .filter(|&index| self.pages[index].used() + 2 <= frame)
            .min_by_key(|&index| self.pages[index].used())?;
        let page = &mut self.pages[victim];
        let slots = std::mem::take(&mut page.slots);
        let fresh = if page.scale == scale {
            page.shelf = Shelf::new(side / scale);
            Fresh::Cleared(slots)
        } else {
            let atlas_page = page.atlas_page;
            *page = PoolPage::new(scale, side, frame);
            page.atlas_page = atlas_page;
            Fresh::Replaced(slots)
        };
        page.used.store(frame, Ordering::Relaxed);
        // It fits an empty page (checked above).
        let at = page.shelf.alloc(extent).unwrap_or([0, 0]);
        Some((victim, at, fresh))
    }
}

/// Shelf placement in game pixels on a square page `side` wide: rows left
/// to right, a new row under the tallest of the last.
struct Shelf {
    side: u32,
    x: u32,
    y: u32,
    row: u32,
}

impl Shelf {
    fn new(side: u32) -> Self {
        Self {
            side,
            x: 0,
            y: 0,
            row: 0,
        }
    }

    fn alloc(&mut self, [width, height]: [u32; 2]) -> Option<[u32; 2]> {
        if width > self.side || height > self.side {
            return None;
        }
        if self.x + width > self.side {
            (self.x, self.y, self.row) = (0, self.y + self.row, 0);
        }
        if self.y + height > self.side {
            return None;
        }
        let at = [self.x, self.y];
        self.x += width;
        self.row = self.row.max(height);
        Some(at)
    }
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    /// A full budget evicts the page drawn longest ago, never one drawn this
    /// or the last frame; past the budget no page is added.
    #[test]
    fn a_full_pool_evicts_its_least_recently_drawn_page() {
        // Pages 64 texels square at 4x: 16 game pixels, one 12x12 tile each
        // (8 game pixels drawn, the gap around).
        let one = page_bytes(64, 4);
        let mut pool = PagePool::new(2 * one, 64);
        let extent = tile_extent([8, 8]);
        assert_eq!(extent, [12, 12]);
        let first = pool.place(4, extent, 1, true).unwrap();
        assert_eq!(first, (0, [0, 0], Fresh::Created));
        pool.pages[0].slots.push(7);
        assert_eq!(pool.place(4, extent, 2, true).unwrap().2, Fresh::Created);
        pool.pages[1].slots.push(8);
        assert_eq!(pool.bytes(), 2 * one);
        // Page 1 was drawn at frame 2, page 0 at frame 1: at frame 3 page 0
        // is the one to empty.
        let (page, at, fresh) = pool.place(4, extent, 3, true).unwrap();
        assert_eq!((page, at, fresh), (0, [0, 0], Fresh::Cleared(vec![7])));
        assert_eq!(pool.pages.len(), 2, "no page past the budget");
        pool.pages[0].slots.push(9);
        // Both drawn at frame 3: at frame 4 nothing can go.
        pool.pages[1].used.store(3, Ordering::Relaxed);
        assert!(pool.place(4, extent, 4, true).is_none());
        // Another scale replaces the oldest page in place.
        let (page, _, fresh) = pool.place(2, extent, 6, true).unwrap();
        assert_eq!((page, fresh), (0, Fresh::Replaced(vec![9])));
        assert_eq!(pool.pages[0].scale, 2);
        // Too large for a page at all.
        assert!(pool.place(4, tile_extent([16, 4]), 9, true).is_none());
    }

    fn key(frame: u16) -> ShpSpriteKey {
        ShpSpriteKey {
            palette_context: crate::render::sprite_atlas::ShpPaletteContext::GlobalAnim,
            type_id: "BOOM".into(),
            facing: 0,
            frame,
            house_color: crate::rules::house_colors::HouseColorIndex(0),
        }
    }

    /// While zoomed in, a frame drawn is wanted once; once on an HD page its
    /// key's entry is the game-pixel one with only its page and UV changed,
    /// and an eviction gives the game-pixel entry back.
    #[test]
    fn a_drawn_frame_is_wanted_then_redirected_with_its_own_offsets() {
        let mut stream = PictureStream::default();
        let file: PictureFile = ("BOOM.png".into(), Some([8, 8]));
        let bytes =
            crate::assets::mix_archive::SharedBytes::from(std::sync::Arc::<[u8]>::from(&[0u8][..]));
        stream.add_file(
            file.clone(),
            HdSource {
                bytes: crate::render::picture_atlas::PictureBytes {
                    picture: bytes,
                    mask: None,
                },
                canvas: [8, 8],
                scale: 4,
            },
            16384,
        );
        let id = PictureFrameId {
            file: file.0.clone(),
            size: file.1,
            frame: 0,
            deferred: false,
        };
        stream.add_frame(&id, [1, 2], [3, 4]);
        let entry = ShpSpriteEntry {
            uv_origin: [0.5, 0.5],
            uv_size: [0.1, 0.1],
            pixel_size: [3.0, 4.0],
            offset_x: -3.0,
            offset_y: -2.0,
            canvas_rect: [-4.0, -4.0, 8.0, 8.0],
            extended: true,
            page: 3,
            source_palette: 0,
        };
        stream.add_key(&key(0), &id, &entry);
        assert!(stream.entry(&key(0)).is_none(), "off at zoom 1");
        assert!(stream.demand.lock().unwrap().is_empty());
        stream.active = true;
        assert!(stream.entry(&key(0)).is_none());
        assert!(stream.entry(&key(0)).is_none());
        assert_eq!(*stream.demand.lock().unwrap(), [0], "wanted once");
        assert!(stream.entry(&key(1)).is_none(), "not a streamed key");

        let (page, at, _) = stream.pool.place(4, tile_extent([3, 4]), 1, true).unwrap();
        stream.pool.pages[page].atlas_page = 9;
        let entries = HashMap::from([(key(0), entry)]);
        stream.set_resident(0, page, at, &entries);
        let hd = *stream.entry(&key(0)).unwrap();
        assert_eq!(hd.page, 9);
        // A 2048-texel page at 4x is 512 game pixels; the frame is
        // `PAGE_GAP` into its tile.
        assert_eq!(hd.uv_origin, [2.0 / 512.0, 2.0 / 512.0]);
        assert_eq!(hd.uv_size, [3.0 / 512.0, 4.0 / 512.0]);
        assert_eq!(
            (hd.pixel_size, hd.offset_x, hd.offset_y, hd.canvas_rect),
            (
                entry.pixel_size,
                entry.offset_x,
                entry.offset_y,
                entry.canvas_rect
            )
        );
        assert_eq!((hd.extended, hd.source_palette), (true, 0));

        stream.revert(&[0]);
        assert!(stream.entry(&key(0)).is_none(), "back to game pixels");
    }
}
