//! VERA's HD art: 2x upscaled SHP frames, theater tiles and overlays,
//! sampled by the ordinary world blitters while the camera zooms in.
//!
//! VERA-only presentation extension (HD art, `[VERA] HdArt=`,
//! `render::hd_switch`, off by default), experimental
//! (`docs/zoom-one-pass.md`, phase 2). The battlefield draws once, at the
//! window's resolution, through a camera whose zoom may be above 1; while it
//! is and HD art is on ([`bind_hd_twins`]), the atlases bind HD twins of
//! their pages in place of the 1x ones, and the same instances read them:
//! SHP frames from [`ShpHd`]'s pages (`sprite_source.wgsl`
//! `shp_hd_color`), tiles and overlays from their atlases' twins
//! (`render::hd_terrain::RgbaTwin`). The 1x index plane, palette row, light,
//! Z and silhouette stay the 1x pixel's; only the colour comes from the art.
//! Each twin has two levels (2x and 1x), so a zoom between 1 and 2 blends
//! them. Nothing reaches `sim/`, a save, a replay or the network.
//!
//! The art lives in a pack folder (`hd_art/pack` beside the executable, or
//! `VERA20K_HD_ART_PACK`): `<hash>.png` per frame, 2x the frame's stored
//! rectangle, RGB in the pack's `reference.pal` and alpha for coverage. The
//! hash ([`frame_hash`]) is the FNV-1a of the frame's size and palette
//! indices, so a frame finds its art whichever type, theater or mod draws it.
//! `VERA20K_HD_DUMP=<folder>` writes `<hash>.idx` (u16 width, u16 height,
//! indices) for every building frame the pack lacks, for the offline upscaler
//! (`hd_art/tools`).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use super::batch::{BatchRenderer, BatchTexture};
use super::hd_loader::HdLoader;
use super::overlay_atlas::OverlayAtlas;
use super::sprite_atlas::{ShpSpriteEntry, ShpSpriteKey, SpriteAtlas, SpriteAtlasPage};
use super::tile_atlas::TileAtlas;

/// Native pixels per side of an SHP HD page at most; its twin is twice that.
const SHP_PAGE: u32 = 2048;

/// SHP HD pages at most: each holds a 2x twin of two levels (80 MiB at
/// [`SHP_PAGE`]). Frames past them stay 1x.
const MAX_SHP_PAGES: usize = 4;

/// Pictures asked of the pack readers (`render::hd_loader`) and not yet
/// back: a screen of new frames waits here instead of queueing every frame
/// of every building.
const MAX_IN_FLIGHT: usize = 256;

/// RGBA bytes of finished pictures put on the HD pages in one frame, so a
/// burst of arrivals spreads over a few frames.
const UPLOAD_BYTES_PER_FRAME: usize = 32 << 20;

/// Frames asked for that one frame looks at, at most; the rest are asked for
/// again by the next frame's draws.
const KEYS_PER_FRAME: usize = 512;

/// Levels of every HD twin: 2x and 1x. The world draws zoomed in only, so a
/// target pixel never spans more than one 1x texel.
pub(crate) const TWIN_LEVELS: u32 = 2;

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

/// `VERA20K_HD_ART=debug` (`hd_switch::env_override`): HD texels are tinted
/// as they are put on their twins, `colour` at 35%.
pub(crate) fn debug_tint(rgb: [u8; 3], colour: [u8; 3]) -> [u8; 3] {
    if !super::hd_switch::env_override().is_some_and(|(_, debug)| debug) {
        return rgb;
    }
    std::array::from_fn(|c| ((u32::from(rgb[c]) * 65 + u32::from(colour[c]) * 35) / 100) as u8)
}

/// The colour the HD art of an indexed SHP frame tints with in debug.
const SHP_DEBUG: [u8; 3] = [0, 255, 0];

/// Whether this frame's world reads HD twins: the camera zooms in and HD
/// art is on. Not on the WebGL2 fallback.
pub(crate) fn twins_on(device: &wgpu::Device, camera_zoom: f32, hd_art_on: bool) -> bool {
    camera_zoom > 1.0 && hd_art_on && !super::gpu::webgl_fallback(device)
}

/// VERA-only, experimental (`docs/zoom-one-pass.md`): the frame's one HD art
/// hook, called once per frame before the world's instances are built and
/// drawn (and after the unit atlas uploads). While the camera zooms in
/// (`camera_zoom`, the world camera's zoom) with HD art on (`hd_art_on`,
/// `app::state::hd_art_on`), the SHP atlas hands out its HD page entries
/// ([`SpriteAtlas::bind_hd_art`]) and the tile and overlay atlases bind their
/// twins ([`TileAtlas::bind_hd_twin`], [`OverlayAtlas::bind_hd_twin`]); each
/// reads a few more pack files a frame until it has them all. Otherwise
/// every page has its 1x binding back. Voxels need no call: their HD pages
/// are bound beside the 1x ones and read by the camera's zoom
/// (`sprite_voxel_shader.wgsl` `voxel_byte_at`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn bind_hd_twins(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    batch: &BatchRenderer,
    sprites: Option<&mut SpriteAtlas>,
    tiles: Option<&mut TileAtlas>,
    overlays: Option<&mut OverlayAtlas>,
    camera_zoom: f32,
    hd_art_on: bool,
) {
    let on = twins_on(device, camera_zoom, hd_art_on);
    if let Some(sprites) = sprites {
        sprites.bind_hd_art(device, queue, batch, on);
    }
    if let Some(tiles) = tiles {
        tiles.bind_hd_twin(device, queue, batch, on);
    }
    if let Some(overlays) = overlays {
        overlays.bind_hd_twin(device, queue, batch, on);
    }
}

/// Whether pack pictures are read at once on the render thread instead of
/// by the readers: a capture (`hd_switch::capture_zoom`) shows one frame, so
/// it reads all it needs before drawing it.
pub(crate) fn read_at_once() -> bool {
    super::hd_switch::capture_zoom().is_some()
}

/// An HD twin texture of `size` texels (level 0): encoded colour
/// premultiplied by coverage, [`TWIN_LEVELS`] levels, zeroed (no coverage)
/// until written with [`write_twin`].
pub(crate) fn create_twin_texture(
    device: &wgpu::Device,
    label: &str,
    size: [u32; 2],
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: TWIN_LEVELS,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Write `texels` (premultiplied RGBA, `size` texels, both sides even) at
/// `origin` (even) of a twin's level 0, and their 2x2 averages at half that
/// on level 1, so the levels stay one picture.
pub(crate) fn write_twin(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    origin: [u32; 2],
    size: [u32; 2],
    texels: &[u8],
) {
    let [width, height] = size;
    if width == 0 || height == 0 || texels.len() != (width * height * 4) as usize {
        return;
    }
    super::atlas_growth::write_texels(queue, texture, origin, size, 4, texels);
    let half = half_level(texels, size);
    let [w, h] = [width / 2, height / 2];
    if w == 0 || h == 0 {
        return;
    }
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 1,
            origin: wgpu::Origin3d {
                x: origin[0] / 2,
                y: origin[1] / 2,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        &half,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(w * 4),
            rows_per_image: Some(h),
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
}

/// The next level of premultiplied `texels` (`size` texels): each texel the
/// rounded average of the 2x2 it covers.
fn half_level(texels: &[u8], [width, height]: [u32; 2]) -> Vec<u8> {
    let [w, h] = [(width / 2) as usize, (height / 2) as usize];
    let row = width as usize * 4;
    let mut half = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        for x in 0..w {
            let at = 2 * y * row + 2 * x * 4;
            for c in 0..4 {
                let sum = u32::from(texels[at + c])
                    + u32::from(texels[at + 4 + c])
                    + u32::from(texels[at + row + c])
                    + u32::from(texels[at + row + 4 + c]);
                half.push(((sum + 2) / 4) as u8);
            }
        }
    }
    half
}

/// The twin texels of an indexed SHP frame: `indices` (`size`, its 1x
/// palette indices) and `art` (its pack picture, twice `size`, RGB in the
/// pack's `reference` palette, alpha for coverage). Each 2x texel takes its
/// 1x pixel's coverage (a 1x hole stays one, so the silhouette is the 1x
/// one) and holds the art as `sprite_source.wgsl` `shp_hd_color` reads it:
/// for house colours (16..31) the ratio of its brightest channel to the
/// reference entry's, over 2.5 (the old redraw's correction, which keeps the
/// owner's ramp); otherwise its difference from the reference entry, halved
/// around 128 (so the row's theater colours come through). A texel the art
/// leaves uncovered holds no difference: the 1x colour.
pub(crate) fn compose_shp_twin(
    indices: &[u8],
    [width, height]: [u32; 2],
    art: &[u8],
    reference: &[u8],
) -> Vec<u8> {
    let (w2, h2) = (2 * width as usize, 2 * height as usize);
    let mut texels = vec![0u8; w2 * h2 * 4];
    if indices.len() != width as usize * height as usize
        || art.len() != w2 * h2 * 4
        || reference.len() < 1024
    {
        return texels;
    }
    for y in 0..h2 {
        for x in 0..w2 {
            let index = usize::from(indices[(y / 2) * width as usize + x / 2]);
            if index == 0 {
                continue;
            }
            let at = (y * w2 + x) * 4;
            let base = &reference[index * 4..index * 4 + 3];
            let covered = art[at + 3] >= 128;
            let hd = debug_tint([art[at], art[at + 1], art[at + 2]], SHP_DEBUG);
            let stored = if (16..32).contains(&index) {
                let ratio = if covered {
                    let peak = f32::from(hd[0].max(hd[1]).max(hd[2]));
                    let reference_peak = f32::from(base[0].max(base[1]).max(base[2]));
                    (peak / reference_peak.max(5.1)).clamp(0.0, 2.5)
                } else {
                    1.0
                };
                [(ratio / 2.5 * 255.0).round() as u8; 3]
            } else if covered {
                std::array::from_fn(|c| {
                    let difference = i32::from(hd[c]) - i32::from(base[c]);
                    (128 + difference.div_euclid(2) + difference.rem_euclid(2)).clamp(0, 255) as u8
                })
            } else {
                [128; 3]
            };
            texels[at..at + 3].copy_from_slice(&stored);
            texels[at + 3] = 255;
        }
    }
    texels
}

/// The twin texels of an RGBA picture: `native` (`size`, encoded RGBA whose
/// alpha is its coverage) at twice its size, each 2x texel with its 1x
/// pixel's coverage and the colour of `art` (twice `size`, encoded RGBA) where
/// the art covers it, else the 1x colour, tinted with `debug` in debug
/// (`debug_tint`); premultiplied.
pub(crate) fn compose_rgba_twin(
    native: &[u8],
    [width, height]: [u32; 2],
    art: Option<&[u8]>,
    debug: [u8; 3],
) -> Vec<u8> {
    let (w2, h2) = (2 * width as usize, 2 * height as usize);
    let mut texels = vec![0u8; w2 * h2 * 4];
    if native.len() != width as usize * height as usize * 4 {
        return texels;
    }
    let art = art.filter(|art| art.len() == w2 * h2 * 4);
    for y in 0..h2 {
        for x in 0..w2 {
            let from = ((y / 2) * width as usize + x / 2) * 4;
            let alpha = native[from + 3];
            if alpha == 0 {
                continue;
            }
            let at = (y * w2 + x) * 4;
            let rgb = match art {
                Some(art) if art[at + 3] >= 128 => {
                    debug_tint([art[at], art[at + 1], art[at + 2]], debug)
                }
                _ => [native[from], native[from + 1], native[from + 2]],
            };
            for c in 0..3 {
                texels[at + c] = ((u32::from(rgb[c]) * u32::from(alpha) + 127) / 255) as u8;
            }
            texels[at + 3] = alpha;
        }
    }
    texels
}

/// VERA-only, experimental (`docs/zoom-one-pass.md`): a page's 1x bind group
/// while its HD binding stands in for it, to put back.
#[derive(Default)]
pub(crate) struct TwinSwap {
    low: Option<wgpu::BindGroup>,
}

impl TwinSwap {
    /// Put `hd` in `slot`, keeping the 1x binding, when `on` and there is an
    /// HD binding; otherwise the 1x binding back.
    pub(crate) fn set(
        &mut self,
        slot: &mut wgpu::BindGroup,
        hd: Option<&wgpu::BindGroup>,
        on: bool,
    ) {
        match hd.filter(|_| on) {
            Some(hd) => {
                let low = std::mem::replace(slot, hd.clone());
                self.low.get_or_insert(low);
            }
            None => {
                if let Some(low) = self.low.take() {
                    *slot = low;
                }
            }
        }
    }
}

/// One SHP HD page: its 1x index plane and its 2x twin, which a page among
/// the atlas's `pages` binds (`BatchRenderer::create_shp_hd_binding`).
struct ShpHdPage {
    atlas_page: usize,
    indices: wgpu::TextureView,
    twin: wgpu::TextureView,
}

/// VERA-only, experimental (`docs/zoom-one-pass.md`): the pack's art of the
/// SHP frames the zoomed-in world has drawn, on HD pages of the sprite
/// atlas. An HD page is an indexed page like the atlas's own: the frame's
/// 1x indices at the same place, resolved through the same palette rows; its
/// binding adds the 2x twin of [`compose_shp_twin`] texels. A frame's key
/// then has an HD entry, the 1x entry on that page
/// ([`SpriteAtlas::get`]). Frames are read only when asked for, by the pack
/// readers (`render::hd_loader`, at most [`MAX_IN_FLIGHT`] at a time), go up
/// when their picture is back ([`UPLOAD_BYTES_PER_FRAME`] a frame) and stay
/// for the match: an entry handed out never goes stale. A frame draws 1x
/// until then.
pub(crate) struct ShpHd {
    dir: PathBuf,
    dump: Option<PathBuf>,
    reference: Vec<u8>,
    /// 1x pixels per side of an HD page.
    side: u32,
    pages: Vec<ShpHdPage>,
    /// Shelf cursor on the last page: x, y, row height.
    shelf: [u32; 3],
    /// Frame hash → its HD page (index into `pages`) and 1x origin there,
    /// or None when the pack has no art for it or no page has room.
    frames: HashMap<u64, Option<(usize, [u32; 2])>>,
    /// Each key with art: its HD page entry.
    entries: HashMap<ShpSpriteKey, ShpSpriteEntry>,
    /// Keys looked at: with an entry, or none to have.
    checked: HashSet<ShpSpriteKey>,
    /// Keys drawn this frame and not yet looked at ([`Self::entry`]).
    demand: std::sync::Mutex<HashSet<ShpSpriteKey>>,
    /// Frame hash → its 1x size and indices while the readers have its
    /// picture.
    waiting: HashMap<u64, ([u32; 2], Vec<u8>)>,
    /// The pack readers, started when the first picture is asked for.
    loader: Option<HdLoader>,
    active: bool,
    /// No page is left: further frames stay 1x.
    full: bool,
}

impl ShpHd {
    /// The pack, or None without a pack folder with a reference palette.
    pub(crate) fn open(device: &wgpu::Device) -> Option<Self> {
        let dir = pack_dir()?;
        let reference = load_reference_palette(&dir)?;
        log::info!("HD art pack: {}", dir.display());
        Some(Self {
            dir,
            dump: std::env::var("VERA20K_HD_DUMP").ok().map(PathBuf::from),
            reference,
            side: SHP_PAGE.min(device.limits().max_texture_dimension_2d / 2),
            pages: Vec::new(),
            shelf: [0, 0, 0],
            frames: HashMap::new(),
            entries: HashMap::new(),
            checked: HashSet::new(),
            demand: std::sync::Mutex::new(HashSet::new()),
            waiting: HashMap::new(),
            loader: None,
            active: false,
            full: false,
        })
    }

    pub(crate) fn set_active(&mut self, on: bool) {
        self.active = on;
    }

    /// `key`'s HD entry while on; a key not yet looked at is remembered for
    /// the next [`Self::load`].
    pub(crate) fn entry(&self, key: &ShpSpriteKey) -> Option<&ShpSpriteEntry> {
        if !self.active {
            return None;
        }
        if let Some(entry) = self.entries.get(key) {
            return Some(entry);
        }
        if !self.checked.contains(key) {
            self.demand
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(key.clone());
        }
        None
    }

    /// Whether atlas page `page` is an HD page.
    pub(crate) fn is_page(&self, page: usize) -> bool {
        self.pages.iter().any(|hd| hd.atlas_page == page)
    }

    /// Bind each HD page again with the atlas's grown `palette_rows`.
    pub(crate) fn rebind(
        &self,
        device: &wgpu::Device,
        batch: &BatchRenderer,
        pages: &mut [SpriteAtlasPage],
        palette_rows: &wgpu::TextureView,
    ) {
        for hd in &self.pages {
            if let Some(page) = pages.get_mut(hd.atlas_page) {
                page.texture.bind_group =
                    batch.create_shp_hd_binding(device, palette_rows, &hd.indices, &hd.twin);
            }
        }
    }

    /// Put the pictures the readers finished on the HD pages, then look at
    /// the keys asked for since the last call: `lookup` gives a key's 1x
    /// entry and frame indices. A frame the pack has art for is put on an HD
    /// page (`pages`, the atlas's, gains one when the last is full) once its
    /// picture is back, and each of its keys then gets an HD entry; a key
    /// whose picture is still being read is asked for again by a later
    /// frame's draws.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn load(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        pages: &mut Vec<SpriteAtlasPage>,
        palette_rows: &wgpu::TextureView,
        lookup: impl Fn(&ShpSpriteKey) -> Option<(ShpSpriteEntry, Vec<u8>)>,
    ) {
        let mut uploaded = 0;
        while uploaded < UPLOAD_BYTES_PER_FRAME {
            let Some(loaded) = self.loader.as_mut().and_then(HdLoader::take) else {
                break;
            };
            uploaded += self.finish(
                device,
                queue,
                batch,
                pages,
                palette_rows,
                loaded.key,
                loaded.rgba,
            );
        }
        let demand = std::mem::take(
            &mut *self
                .demand
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let at_once = read_at_once();
        let keys = if at_once { usize::MAX } else { KEYS_PER_FRAME };
        for key in demand.into_iter().take(keys) {
            if self.checked.contains(&key) {
                continue;
            }
            let Some((entry, indices)) = lookup(&key) else {
                self.checked.insert(key);
                continue;
            };
            let size = entry.pixel_size.map(|side| side as u32);
            if !super::sprite_atlas::source_palette_indexed(entry.source_palette)
                || size.contains(&0)
                || indices.len() != (size[0] * size[1]) as usize
            {
                self.checked.insert(key);
                continue;
            }
            let hash = frame_hash(size[0], size[1], &indices);
            if !self.frames.contains_key(&hash) {
                if self.waiting.contains_key(&hash) {
                    // Asked for again by a later frame's draws.
                    continue;
                }
                if size[0] >= self.side || size[1] >= self.side {
                    self.frames.insert(hash, None);
                } else {
                    let path = self.dir.join(format!("{hash:016x}.png"));
                    let art_size = size.map(|side| side * 2);
                    if at_once {
                        let art = super::hd_loader::read_picture(&path, art_size);
                        self.waiting.insert(hash, (size, indices));
                        self.finish(device, queue, batch, pages, palette_rows, hash, art);
                    } else {
                        let loader = self
                            .loader
                            .get_or_insert_with(|| HdLoader::new("HD art reader"));
                        if loader.in_flight() < MAX_IN_FLIGHT {
                            loader.request(hash, path, art_size);
                            self.waiting.insert(hash, (size, indices));
                        }
                        continue;
                    }
                }
            }
            if let Some(Some((page, origin))) = self.frames.get(&hash).copied()
                && let Some(hd) = self.pages.get(page)
                && let Ok(page_byte) = u8::try_from(hd.atlas_page)
            {
                let side = self.side as f32;
                self.entries.insert(
                    key.clone(),
                    ShpSpriteEntry {
                        uv_origin: origin.map(|v| v as f32 / side),
                        uv_size: size.map(|v| v as f32 / side),
                        page: page_byte,
                        ..entry
                    },
                );
            }
            self.checked.insert(key);
        }
    }

    /// Put the finished picture `art` of the frame `hash` on an HD page, or
    /// note that it has none: the pack lacks it (the frame is dumped then) or
    /// no page has room. Returns the picture's RGBA bytes put up.
    #[allow(clippy::too_many_arguments)]
    fn finish(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        batch: &BatchRenderer,
        pages: &mut Vec<SpriteAtlasPage>,
        palette_rows: &wgpu::TextureView,
        hash: u64,
        art: Option<Vec<u8>>,
    ) -> usize {
        let Some(([width, height], indices)) = self.waiting.remove(&hash) else {
            return 0;
        };
        let Some(art) = art else {
            if let Some(dump) = &self.dump {
                let mut bytes = Vec::with_capacity(4 + indices.len());
                bytes.extend_from_slice(&(width as u16).to_le_bytes());
                bytes.extend_from_slice(&(height as u16).to_le_bytes());
                bytes.extend_from_slice(&indices);
                let _ = std::fs::create_dir_all(dump);
                let _ = std::fs::write(dump.join(format!("{hash:016x}.idx")), bytes);
            }
            self.frames.insert(hash, None);
            return 0;
        };
        let place = self.place(device, batch, pages, palette_rows, width, height);
        if let Some((page, origin)) = place {
            let hd = &self.pages[page];
            super::atlas_growth::write_texels(
                queue,
                hd.indices.texture(),
                origin,
                [width, height],
                1,
                &indices,
            );
            let texels = compose_shp_twin(&indices, [width, height], &art, &self.reference);
            write_twin(
                queue,
                hd.twin.texture(),
                origin.map(|v| v * 2),
                [width * 2, height * 2],
                &texels,
            );
        }
        self.frames.insert(hash, place);
        art.len()
    }

    /// Shelf placement, one 1x pixel apart, on the last HD page or a new
    /// one: None when no page is left.
    fn place(
        &mut self,
        device: &wgpu::Device,
        batch: &BatchRenderer,
        pages: &mut Vec<SpriteAtlasPage>,
        palette_rows: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Option<(usize, [u32; 2])> {
        if !self.pages.is_empty() && !self.full {
            let [x, y, row] = &mut self.shelf;
            if *x + width > self.side {
                *y += *row + 1;
                *x = 0;
                *row = 0;
            }
            if *y + height <= self.side {
                let origin = [*x, *y];
                *x += width + 1;
                *row = (*row).max(height);
                return Some((self.pages.len() - 1, origin));
            }
        }
        if self.full {
            return None;
        }
        if self.pages.len() >= MAX_SHP_PAGES || u8::try_from(pages.len()).is_err() {
            log::info!("HD art: no HD page left; further frames stay 1x");
            self.full = true;
            return None;
        }
        let indices = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("HD art 1x indices"),
                size: wgpu::Extent3d {
                    width: self.side,
                    height: self.side,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Uint,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let twin = create_twin_texture(device, "HD art twin", [self.side * 2; 2])
            .create_view(&Default::default());
        let atlas_page = pages.len();
        pages.push(SpriteAtlasPage {
            texture: BatchTexture {
                bind_group: batch.create_shp_hd_binding(device, palette_rows, &indices, &twin),
                view: indices.clone(),
                source_indices: indices.clone(),
                width: self.side,
                height: self.side,
            },
        });
        log::info!(
            "HD art page {} (sprite atlas page {atlas_page}, {}x{})",
            self.pages.len(),
            self.side,
            self.side
        );
        self.pages.push(ShpHdPage {
            atlas_page,
            indices,
            twin,
        });
        self.shelf = [width + 1, 0, height];
        Some((self.pages.len() - 1, [0, 0]))
    }
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    /// An SHP twin texel without art, or art the same as the reference,
    /// is the 1x colour exactly (`shp_hd_color`: own + 0, own * 1), and a 1x
    /// hole stays one.
    #[test]
    fn shp_twin_without_art_differences_reads_the_1x_colour() {
        let mut reference = vec![0u8; 1024];
        reference[5 * 4..5 * 4 + 4].copy_from_slice(&[40, 80, 120, 255]);
        reference[20 * 4..20 * 4 + 4].copy_from_slice(&[200, 0, 0, 255]);
        let mut art = vec![0u8; 4 * 2 * 4];
        // Index 5's texels as the reference; index 20's at half its peak.
        for at in [0, 1, 4, 5] {
            art[at * 4..at * 4 + 4].copy_from_slice(&[40, 80, 120, 255]);
        }
        for at in [2, 3, 6, 7] {
            art[at * 4..at * 4 + 4].copy_from_slice(&[100, 0, 0, 255]);
        }
        let twin = compose_shp_twin(&[5, 20], [2, 1], &art, &reference);
        assert_eq!(&twin[..4], &[128, 128, 128, 255]);
        assert_eq!(&twin[2 * 4..3 * 4], &[51, 51, 51, 255]);
        assert_eq!(
            &compose_shp_twin(&[0, 5], [2, 1], &art, &reference)[..4],
            &[0; 4]
        );
        assert_eq!(
            half_level(&twin, [4, 2]),
            vec![128, 128, 128, 255, 51, 51, 51, 255]
        );
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
