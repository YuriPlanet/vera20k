//! VERA's HD art: 2x upscaled building frames drawn over the
//! wheel-zoomed world.
//!
//! VERA-only presentation extension (HD art (`[VERA] HdArt=`, `render::hd_switch`), off by default). The
//! world is still composed at native pixels, exactly as before; when the
//! wheel zoom magnifies it, this pass replaces the pixels of the zoomed copy
//! that the native world still shows as a building frame's own colour with
//! that frame's HD art (`hd_art.wgsl`). Nothing reaches `sim/`, a save, a
//! replay or the network.
//!
//! The art lives in a pack folder (`hd_art/pack` beside the executable, or
//! `VERA20K_HD_ART_PACK`): `<hash>.png` per frame, 2x the frame's stored
//! rectangle, RGB in the pack's `reference.pal` and alpha for coverage. The
//! hash ([`frame_hash`]) is the FNV-1a of the frame's size and palette
//! indices, so a frame finds its art whichever type, theater or mod draws it.
//! `VERA20K_HD_DUMP=<folder>` writes `<hash>.idx` (u16 width, u16 height,
//! indices) for every building frame the pack lacks, for the offline upscaler
//! (`hd_art/tools`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bytemuck::{Pod, Zeroable};

use super::atlas_growth::ShelfCursor;

use super::sprite_atlas::{ShpSpriteKey, SpriteAtlas};

/// Native pixels per side of the index page; the HD page is twice that.
const PAGE: u32 = 2048;

/// One visible atlas piece: where its frame is in the sprite atlas, and where
/// the SHP blitter put it in the native composition.
pub(crate) struct HdArtPiece {
    pub page: usize,
    pub uv_origin: [f32; 2],
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub source_palette: u32,
}

/// FNV-1a 64 of a frame: width and height (u16 little-endian), then its
/// palette indices row by row.
pub(crate) fn frame_hash(width: u32, height: u32, indices: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |byte: u8| {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for byte in (width as u16)
        .to_le_bytes()
        .into_iter()
        .chain((height as u16).to_le_bytes())
    {
        eat(byte);
    }
    for &byte in indices {
        eat(byte);
    }
    hash
}

/// The pack folder: `VERA20K_HD_ART_PACK`, else the selected mod's
/// `hd_art/pack` (`Mods/<mod>/hd_art/pack`, docs/hd-art.md): an HD pack
/// installs like any mod. None without either.
pub(crate) fn pack_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("VERA20K_HD_ART_PACK") {
        return Some(PathBuf::from(dir));
    }
    crate::map::source::mod_dir()
        .map(|mod_dir| mod_dir.join("hd_art").join("pack"))
        .filter(|dir| dir.is_dir())
}

static PACK_AVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Look for the pack again: once a start has chosen its mod
/// (`app::initialize`), and in tests.
pub(crate) fn refresh_pack_available() {
    let found = pack_dir().is_some_and(|dir| dir.is_dir());
    PACK_AVAILABLE.store(found, std::sync::atomic::Ordering::Relaxed);
}

/// Whether this start has an HD art pack ([`refresh_pack_available`]):
/// HD art can be on only with one.
pub(crate) fn pack_available() -> bool {
    PACK_AVAILABLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// A folder of the pack (`terrain`, `sidebar`).
pub(crate) fn pack_subdir(name: &str) -> Option<PathBuf> {
    Some(pack_dir()?.join(name))
}

static ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// HD art's state (`render::hd_switch`), set at the start of each frame.
pub(crate) fn set_active(on: bool) {
    ACTIVE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether HD art is on this frame: the sidebar then draws its HD pictures.
pub(crate) fn sidebar_active() -> bool {
    ACTIVE.load(std::sync::atomic::Ordering::Relaxed)
}

fn load_reference_palette(dir: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(dir.join("reference.pal")).ok()?;
    if bytes.len() < 768 {
        return None;
    }
    // 6-bit VGA entries, as the game's PAL files.
    let mut rgba = Vec::with_capacity(1024);
    for entry in bytes[..768].chunks_exact(3) {
        for &c in entry {
            rgba.push(((u32::from(c) * 255) / 63).min(255) as u8);
        }
        rgba.push(255);
    }
    Some(rgba)
}

/// One HD draw: a building frame instance placed in the native composition.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, PartialEq)]
pub(crate) struct HdArtDraw {
    origin: [f32; 2],
    size: [f32; 2],
    page_origin: [u32; 2],
    row: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    frame: [f32; 4],
}

struct Gpu {
    indices: wgpu::Texture,
    art: wgpu::Texture,
    reference: wgpu::Texture,
    rows: Option<(wgpu::Texture, usize)>,
    layout: wgpu::BindGroupLayout,
    pipeline: Option<(wgpu::TextureFormat, wgpu::RenderPipeline)>,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    sampler: wgpu::Sampler,
    uniform: wgpu::Buffer,
    instances: wgpu::Buffer,
    instance_capacity: usize,
}

/// Pictures asked of the readers and not yet back: a screen of new frames
/// waits here instead of queueing every frame of every building.
const MAX_IN_FLIGHT: usize = 256;

/// RGBA bytes of finished pictures uploaded in one frame, so a burst of
/// arrivals spreads over a few frames.
const UPLOAD_BYTES_PER_FRAME: usize = 32 << 20;

/// Frames after which a picture with no room is asked for again.
const NO_ROOM_RETRY_FRAMES: u64 = 120;

/// A frame's HD art.
enum Art {
    /// The readers have it.
    Pending,
    /// The pack has no picture for it (or it is larger than a page).
    Missing,
    /// On the pages: its native rectangle, and the last frame that showed it.
    Ready { rect: [u32; 4], used: u64 },
    /// Read, but even the frames on screen left no room; asked again later.
    NoRoom { at: u64 },
}

/// A frame waiting for its picture: size and native palette indices, which
/// go up with it.
struct Waiting {
    width: u32,
    height: u32,
    indices: Vec<u8>,
}

/// The pack's frames resident on the GPU, keyed by the atlas entries that
/// show them.
pub(crate) struct HdArt {
    dir: PathBuf,
    dump: Option<PathBuf>,
    reference: Vec<u8>,
    /// The atlas the entries below describe ([`SpriteAtlas::instance`]).
    atlas: u64,
    generation: (usize, usize, usize),
    /// Atlas entry (page, uv origin bits) of every building frame → its key.
    candidates: HashMap<(u8, u32, u32), ShpSpriteKey>,
    /// Atlas entry → its frame's hash, or `None` when the frame has no
    /// indices to hash.
    entries: HashMap<(u8, u32, u32), Option<u64>>,
    /// Frame hash → its art. Kept across maps: a hash names the same art
    /// whichever atlas shows it.
    frames: HashMap<u64, Art>,
    waiting: HashMap<u64, Waiting>,
    loader: super::hd_loader::HdLoader,
    /// Frames prepared so far, for [`Art::Ready`]'s `used`.
    frame: u64,
    shelf: ShelfCursor,
    /// A full visible set can refuse several pictures in the same pass.
    repacked_this_pass: bool,
    gpu: Option<Gpu>,
}

impl HdArt {
    /// The pack, or `None` when there is no pack folder with a reference
    /// palette: the pass then never runs.
    pub(crate) fn open() -> Option<Self> {
        let dir = pack_dir()?;
        let reference = load_reference_palette(&dir)?;
        log::info!("HD art pack: {}", dir.display());
        Some(Self {
            dir,
            dump: std::env::var("VERA20K_HD_DUMP").ok().map(PathBuf::from),
            reference,
            atlas: 0,
            generation: (0, 0, 0),
            candidates: HashMap::new(),
            entries: HashMap::new(),
            frames: HashMap::new(),
            waiting: HashMap::new(),
            loader: super::hd_loader::HdLoader::new("HD art reader"),
            frame: 0,
            shelf: Self::empty_shelf(),
            repacked_this_pass: false,
            gpu: None,
        })
    }

    fn key(page: u8, uv: [f32; 2]) -> (u8, u32, u32) {
        (page, uv[0].to_bits(), uv[1].to_bits())
    }

    /// Bring the art of `pieces` (this frame's visible pieces) up: each
    /// building frame among them is asked of the readers once, and the
    /// pictures they finished go up at the start of a frame. Only frames on
    /// screen are kept, since the atlas holds every animation of every
    /// building type, far more than one page; a full page keeps the frames
    /// shown in the last frames and lets the rest go. A capture reads every
    /// picture at once, since it shows one frame.
    pub(crate) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        atlas: &SpriteAtlas,
        pieces: &[HdArtPiece],
    ) {
        self.repacked_this_pass = false;
        self.ensure_gpu(device, queue);
        self.frame += 1;
        if self.atlas != atlas.instance() {
            // A new map's atlas: its positions and palette rows are its own.
            self.atlas = atlas.instance();
            self.entries.clear();
            self.generation = (usize::MAX, 0, 0);
            if let Some(gpu) = self.gpu.as_mut() {
                gpu.rows = None;
            }
        }
        if self.generation != atlas.generation() {
            self.generation = atlas.generation();
            self.candidates = atlas
                .building_frames()
                .map(|(key, entry)| (Self::key(entry.page, entry.uv_origin), key.clone()))
                .collect();
        }
        let mut uploaded = 0;
        while uploaded < UPLOAD_BYTES_PER_FRAME {
            let Some(loaded) = self.loader.take() else {
                break;
            };
            uploaded += self.finish(device, queue, loaded.key, loaded.rgba);
        }
        let capture = super::hd_switch::capture_zoom().is_some();
        for piece in pieces {
            let Ok(page) = u8::try_from(piece.page) else {
                continue;
            };
            let entry_key = Self::key(page, piece.uv_origin);
            let mut fresh = None;
            let hash = match self.entries.get(&entry_key) {
                Some(None) => continue,
                Some(Some(hash)) => *hash,
                None => match self.frame_of(atlas, entry_key) {
                    None => continue,
                    Some(None) => {
                        self.entries.insert(entry_key, None);
                        continue;
                    }
                    Some(Some(waiting)) => {
                        let hash = frame_hash(waiting.width, waiting.height, &waiting.indices);
                        self.entries.insert(entry_key, Some(hash));
                        fresh = Some(waiting);
                        hash
                    }
                },
            };
            let frame = self.frame;
            let ask = match self.frames.get_mut(&hash) {
                Some(Art::Ready { used, .. }) => {
                    *used = frame;
                    false
                }
                Some(Art::Pending | Art::Missing) => false,
                Some(Art::NoRoom { at }) => frame > *at + NO_ROOM_RETRY_FRAMES,
                None => true,
            };
            if !ask || (!capture && self.loader.in_flight() >= MAX_IN_FLIGHT) {
                continue;
            }
            let waiting = match fresh {
                Some(waiting) => waiting,
                None => match self.frame_of(atlas, entry_key) {
                    Some(Some(waiting)) => waiting,
                    _ => continue,
                },
            };
            self.request(device, queue, hash, waiting, capture);
        }
        self.upload_rows(device, queue, atlas);
    }

    /// The size and indices of the building frame at `entry_key`: `None`
    /// when no building frame is there, `Some(None)` when its indices are
    /// not to be had.
    fn frame_of(&self, atlas: &SpriteAtlas, entry_key: (u8, u32, u32)) -> Option<Option<Waiting>> {
        let key = self.candidates.get(&entry_key)?;
        let entry = atlas.get(key)?;
        let (width, height) = (entry.pixel_size[0] as u32, entry.pixel_size[1] as u32);
        Some(
            atlas
                .frame_indices(key)
                .filter(|indices| {
                    width > 0 && height > 0 && indices.len() == (width * height) as usize
                })
                .map(|indices| Waiting {
                    width,
                    height,
                    indices,
                }),
        )
    }

    /// Ask for `hash`'s picture: of the readers, or at once in a capture.
    fn request(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hash: u64,
        waiting: Waiting,
        capture: bool,
    ) {
        if waiting.width >= PAGE || waiting.height >= PAGE {
            self.frames.insert(hash, Art::Missing);
            return;
        }
        let path = self.dir.join(format!("{hash:016x}.png"));
        let size = [waiting.width * 2, waiting.height * 2];
        self.waiting.insert(hash, waiting);
        if capture {
            let rgba = super::hd_loader::read_picture(&path, size);
            self.finish(device, queue, hash, rgba);
        } else {
            self.frames.insert(hash, Art::Pending);
            self.loader.request(hash, path, size);
        }
    }

    /// Place and upload a finished picture (or note the pack lacks it, and
    /// dump the frame then). Returns the RGBA bytes uploaded.
    fn finish(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hash: u64,
        rgba: Option<Vec<u8>>,
    ) -> usize {
        let Some(waiting) = self.waiting.remove(&hash) else {
            return 0;
        };
        let Some(art) = rgba else {
            if let Some(dump) = &self.dump {
                let mut bytes = Vec::with_capacity(4 + waiting.indices.len());
                bytes.extend_from_slice(&(waiting.width as u16).to_le_bytes());
                bytes.extend_from_slice(&(waiting.height as u16).to_le_bytes());
                bytes.extend_from_slice(&waiting.indices);
                let _ = std::fs::create_dir_all(dump);
                let _ = std::fs::write(dump.join(format!("{hash:016x}.idx")), bytes);
            }
            self.frames.insert(hash, Art::Missing);
            return 0;
        };
        let (width, height) = (waiting.width, waiting.height);
        let mut rect = self.place(width, height);
        if rect.is_none() && !self.repacked_this_pass {
            self.repacked_this_pass = true;
            self.repack(device, queue);
            rect = self.place(width, height);
        }
        let Some(rect) = rect else {
            self.frames.insert(hash, Art::NoRoom { at: self.frame });
            return 0;
        };
        let gpu = self.gpu.as_ref().expect("HD art GPU resources");
        super::atlas_growth::write_texels(
            queue,
            &gpu.indices,
            [rect[0], rect[1]],
            [width, height],
            1,
            &waiting.indices,
        );
        super::atlas_growth::write_texels(
            queue,
            &gpu.art,
            [rect[0] * 2, rect[1] * 2],
            [width * 2, height * 2],
            4,
            &art,
        );
        self.frames.insert(
            hash,
            Art::Ready {
                rect,
                used: self.frame,
            },
        );
        art.len()
    }

    /// The pages are full: copy the frames shown in the last two frames to
    /// fresh pages, tallest first, and let the rest go (they are read again
    /// when seen). Nothing is read from disk again, and nothing a draw
    /// already recorded reads is overwritten: the old pages stay as they are
    /// for it.
    fn repack(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        let keep_from = self.frame.saturating_sub(2);
        let mut keep: Vec<(u64, [u32; 4], u64)> = self
            .frames
            .iter()
            .filter_map(|(hash, art)| match art {
                Art::Ready { rect, used } if *used >= keep_from => Some((*hash, *rect, *used)),
                _ => None,
            })
            .collect();
        keep.sort_by_key(|(hash, rect, _)| (std::cmp::Reverse(rect[3]), *hash));
        self.frames
            .retain(|_, art| !matches!(art, Art::Ready { used, .. } if *used < keep_from));
        let (indices, art) = page_textures(device);
        self.shelf = Self::empty_shelf();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("HD art repack"),
        });
        let mut kept = 0;
        for (hash, old, used) in keep {
            let Some(rect) = self.place(old[2], old[3]) else {
                self.frames.remove(&hash);
                continue;
            };
            let gpu = self.gpu.as_ref().expect("HD art GPU resources");
            copy_rect(&mut encoder, &gpu.indices, &indices, old, rect, 1);
            copy_rect(&mut encoder, &gpu.art, &art, old, rect, 2);
            self.frames.insert(hash, Art::Ready { rect, used });
            kept += 1;
        }
        queue.submit([encoder.finish()]);
        let gpu = self.gpu.as_mut().expect("HD art GPU resources");
        gpu.indices = indices;
        gpu.art = art;
        log::info!("HD art page full; kept the {kept} frames shown last");
    }

    fn empty_shelf() -> ShelfCursor {
        let page = PAGE / super::hd_switch::page_shrink();
        ShelfCursor::new(page, page)
    }

    fn place(&mut self, width: u32, height: u32) -> Option<[u32; 4]> {
        let [x, y] = self.shelf.place(width, height)?;
        Some([x, y, width, height])
    }

    fn upload_rows(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, atlas: &SpriteAtlas) {
        let rows = atlas.palette_rows();
        let gpu = self.gpu.as_mut().expect("HD art GPU resources");
        if rows.is_empty()
            || gpu
                .rows
                .as_ref()
                .is_some_and(|(_, count)| *count == rows.len())
        {
            return;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HD art palette rows"),
            size: wgpu::Extent3d {
                width: 256,
                height: rows.len() as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        for (row, colors) in rows.iter().enumerate() {
            super::atlas_growth::write_texels(
                queue,
                &texture,
                [0, row as u32],
                [256, 1],
                4,
                colors,
            );
        }
        gpu.rows = Some((texture, rows.len()));
    }

    fn ensure_gpu(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) {
        if self.gpu.is_some() {
            return;
        }
        let (indices, art) = page_textures(device);
        let reference = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("HD art reference palette"),
            size: wgpu::Extent3d {
                width: 256,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        super::atlas_growth::write_texels(queue, &reference, [0, 0], [256, 1], 4, &self.reference);
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::VERTEX,
            ty,
            count: None,
        };
        let tex = |sample_type| wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        };
        let unfiltered = wgpu::TextureSampleType::Float { filterable: false };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("HD art"),
            entries: &[
                entry(0, tex(unfiltered)),
                entry(1, tex(wgpu::TextureSampleType::Uint)),
                entry(2, tex(wgpu::TextureSampleType::Float { filterable: true })),
                entry(
                    3,
                    wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                ),
                entry(4, tex(unfiltered)),
                entry(5, tex(unfiltered)),
                entry(
                    6,
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                ),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("HD art"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("HD art"),
            source: wgpu::ShaderSource::Wgsl(shader_source().into()),
        });
        self.gpu = Some(Gpu {
            indices,
            art,
            reference,
            rows: None,
            layout,
            pipeline: None,
            pipeline_layout,
            shader,
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("HD art"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
            uniform: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("HD art"),
                size: std::mem::size_of::<Params>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            instances: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("HD art draws"),
                size: 256 * std::mem::size_of::<HdArtDraw>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            instance_capacity: 256,
        });
    }

    /// The HD draw for `piece`, or `None` when its frame has no resident art.
    pub(crate) fn placement(&self, piece: &HdArtPiece) -> Option<HdArtDraw> {
        let key = Self::key(u8::try_from(piece.page).ok()?, piece.uv_origin);
        let hash = (*self.entries.get(&key)?)?;
        let Art::Ready { rect, .. } = self.frames.get(&hash)? else {
            return None;
        };
        let rect = *rect;
        if piece.source_palette == 0 || piece.size != [rect[2] as f32, rect[3] as f32] {
            return None;
        }
        Some(HdArtDraw {
            origin: piece.origin,
            size: piece.size,
            page_origin: [rect[0], rect[1]],
            row: piece.source_palette - 1,
            _pad: 0,
        })
    }

    /// Draw `draws` into `target` (the zoomed frame, `scale` frame pixels
    /// per native pixel) over `scissor`, reading the native `composition`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn encode(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        composition: &wgpu::Texture,
        scale: f32,
        scissor: [u32; 4],
        draws: &[HdArtDraw],
        debug: bool,
    ) {
        let Some(gpu) = self.gpu.as_mut() else {
            return;
        };
        let Some((rows, _)) = gpu.rows.as_ref() else {
            return;
        };
        if draws.is_empty() || scissor[2] == 0 || scissor[3] == 0 {
            return;
        }
        let format = target.format();
        if gpu
            .pipeline
            .as_ref()
            .is_none_or(|(cached, _)| *cached != format)
        {
            gpu.pipeline = Some((
                format,
                create_pipeline(device, &gpu.pipeline_layout, &gpu.shader, format),
            ));
        }
        if draws.len() > gpu.instance_capacity {
            gpu.instance_capacity = draws.len().next_power_of_two();
            gpu.instances = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("HD art draws"),
                size: (gpu.instance_capacity * std::mem::size_of::<HdArtDraw>()) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        queue.write_buffer(&gpu.instances, 0, bytemuck::cast_slice(draws));
        let size = target.size();
        let params = Params {
            frame: [
                size.width as f32,
                size.height as f32,
                scale,
                if debug { 1.0 } else { 0.0 },
            ],
        };
        queue.write_buffer(&gpu.uniform, 0, bytemuck::bytes_of(&params));
        let view = |texture: &wgpu::Texture| texture.create_view(&Default::default());
        let views = [
            view(composition),
            view(&gpu.indices),
            view(&gpu.art),
            view(rows),
            view(&gpu.reference),
        ];
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("HD art"),
            layout: &gpu.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&gpu.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&views[3]),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&views[4]),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: gpu.uniform.as_entire_binding(),
                },
            ],
        });
        let target_view = target.create_view(&Default::default());
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("HD art"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target_view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let right = (scissor[0] + scissor[2]).min(size.width);
        let bottom = (scissor[1] + scissor[3]).min(size.height);
        if right <= scissor[0] || bottom <= scissor[1] {
            return;
        }
        pass.set_scissor_rect(
            scissor[0],
            scissor[1],
            right - scissor[0],
            bottom - scissor[1],
        );
        pass.set_pipeline(&gpu.pipeline.as_ref().expect("pipeline").1);
        pass.set_bind_group(0, &group, &[]);
        pass.set_vertex_buffer(0, gpu.instances.slice(..));
        pass.draw(0..4, 0..draws.len() as u32);
    }
}

/// The native index page and its 2x art page, empty.
fn page_textures(device: &wgpu::Device) -> (wgpu::Texture, wgpu::Texture) {
    let texture = |label, size: u32, format| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        })
    };
    (
        texture("HD art native indices", PAGE, wgpu::TextureFormat::R8Uint),
        texture("HD art", PAGE * 2, wgpu::TextureFormat::Rgba8UnormSrgb),
    )
}

/// Copy native rectangle `from` of `source` to `to` of `target`, both
/// scaled by `scale` (1 for the index page, 2 for the art page).
fn copy_rect(
    encoder: &mut wgpu::CommandEncoder,
    source: &wgpu::Texture,
    target: &wgpu::Texture,
    from: [u32; 4],
    to: [u32; 4],
    scale: u32,
) {
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: source,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: from[0] * scale,
                y: from[1] * scale,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: target,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: to[0] * scale,
                y: to[1] * scale,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: from[2] * scale,
            height: from[3] * scale,
            depth_or_array_layers: 1,
        },
    );
}

fn shader_source() -> String {
    format!(
        "{}\n{}",
        super::webgl::srgb_wgsl(),
        include_str!("hd_art.wgsl")
    )
}

fn create_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
) -> wgpu::RenderPipeline {
    let attributes =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Uint32x2, 3 => Uint32];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("HD art"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[Some(wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<HdArtDraw>() as u64,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &attributes,
            })],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleStrip,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    #[test]
    fn hd_art_shader_parses_and_validates() {
        use wgpu::naga;
        let module = naga::front::wgsl::parse_str(&shader_source()).expect("HD art shader parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("HD art shader validates");
    }

    /// The offline upscaler (`hd_art/tools`) names files by the same hash;
    /// this pins its byte order.
    #[test]
    fn frame_hash_covers_size_and_indices() {
        // The same value the Python tool computes for this frame.
        assert_eq!(frame_hash(2, 1, &[5, 6]), 0xf10c_ed57_af42_a1d5);
        assert_ne!(frame_hash(2, 1, &[5, 6]), frame_hash(1, 2, &[5, 6]));
        assert_ne!(frame_hash(2, 1, &[5, 6]), frame_hash(2, 1, &[6, 5]));
    }

    /// An HD pack installs as a mod: the selected mod's `hd_art/pack` is the
    /// pack, and a mod without one, or no mod, leaves none.
    #[test]
    fn the_selected_mods_pack_is_the_pack() {
        if std::env::var_os("VERA20K_HD_ART_PACK").is_some() {
            return;
        }
        let root = std::env::temp_dir().join(format!("vera-hd-pack-{}", std::process::id()));
        let with_pack = root.join("Mods").join("HD Pack");
        std::fs::create_dir_all(with_pack.join("hd_art").join("pack")).unwrap();
        let without = root.join("Mods").join("Gameplay");
        std::fs::create_dir_all(&without).unwrap();

        crate::map::source::set_mod_dir(Some(with_pack.clone()));
        refresh_pack_available();
        assert!(pack_available());
        assert_eq!(pack_dir(), Some(with_pack.join("hd_art").join("pack")));
        assert_eq!(
            pack_subdir("terrain"),
            Some(with_pack.join("hd_art").join("pack").join("terrain"))
        );
        crate::map::source::set_mod_dir(Some(without));
        refresh_pack_available();
        assert!(!pack_available());
        assert_eq!(pack_dir(), None);
        crate::map::source::set_mod_dir(None);
        refresh_pack_available();
        assert!(!pack_available());
        assert_eq!(pack_dir(), None);
        let _ = std::fs::remove_dir_all(root);
    }
}
