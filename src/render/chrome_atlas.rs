//! Shell art decoding and the shelf packer shared by the loading-screen,
//! main-menu and skirmish shell chrome atlases.
//!
//! [`ChromeArt`] decodes SHP frames and PCX images into pieces in the order an
//! atlas lists them. Pieces keep that order on rows of a page at least 1024 px
//! wide, two pixels apart; the page height rounds up to a power of two. The map
//! atlases use a different policy (area-sorted, one-pixel padding, page
//! limits) and `atlas_growth::ShelfCursor` wraps without the trailing gap, so
//! neither shares this placement.

use std::collections::HashMap;
use std::ops::Range;

use anyhow::{Context, Result};

use crate::assets::asset_manager::AssetManager;
use crate::assets::pal_file::Palette;
use crate::assets::pcx_file::PcxFile;
use crate::assets::shp_file::ShpFile;
use crate::render::batch::{BatchRenderer, BatchTexture};
use crate::render::gpu::GpuContext;
use crate::render::native_surface_format::ACTIVE_RETAIL_RGB565_PRESENTATION;
use crate::render::shell_ui_style_assets::{decode_ui_pcx, decode_ui_shp, ui_art_candidates};

const ATLAS_PADDING: u32 = 2;
const MIN_ATLAS_WIDTH: u32 = 1024;

/// Magenta is transparent in owner-draw flags, list images and kind-2 image
/// statics, compared after the PCX cache (`0x006B9D00`, `0x006BA140`) packs
/// the embedded palette to the display's RGB565 (`OwnerDraw_Static_006153E0`).
pub(crate) const PACKED_TRANSPARENT_RGB: [u8; 3] = [255, 0, 255];

/// Shell palettes come from ConvertClass__BuildFromPaletteFile `0x0072ADE0`
/// (SHELL.PAL is read at `0x0072ABDC`), the shared palette conversion.
pub(crate) fn read_palette(assets: &AssetManager, name: &str) -> Result<Palette> {
    decode_ui_art(assets, name, |bytes| {
        Palette::from_bytes(bytes).with_context(|| format!("Could not parse palette {name}"))
    })
}

/// The first copy of UI art `name` that `decode` accepts: the selected mod's
/// copies first, then the game's (`ui_art_candidates`), so a mod's broken
/// copy falls through to the next. The first rejection is the error.
pub(crate) fn decode_ui_art<T>(
    assets: &AssetManager,
    name: &str,
    decode: impl Fn(&[u8]) -> Result<T>,
) -> Result<T> {
    let mut first_error = None;
    for candidate in ui_art_candidates(assets, name) {
        match decode(candidate.bytes) {
            Ok(art) => {
                log::debug!("UI art {name} from {}", candidate.source);
                return Ok(art);
            }
            Err(error) => {
                log::debug!(
                    "UI art {name} from {} was rejected: {error:#}",
                    candidate.source
                );
                first_error.get_or_insert(error);
            }
        }
    }
    Err(first_error
        .unwrap_or_else(|| anyhow::anyhow!("UI artwork {name} is missing or unreadable")))
}

/// PCX `file`, the selected mod's copy first, bounded as UI art when a mod
/// is selected.
pub(crate) fn read_ui_pcx(assets: &AssetManager, file: &str) -> Result<PcxFile> {
    decode_ui_art(assets, file, |bytes| {
        if assets.mod_selected() {
            decode_ui_pcx(bytes)
        } else {
            PcxFile::from_bytes(bytes)
        }
        .map_err(Into::into)
    })
}

/// SHP `name` handed to `read`. With a mod selected its copies are tried
/// first and a copy `read` rejects falls through to the next; otherwise the
/// game's one copy is read.
pub(crate) fn read_ui_shp<T>(
    assets: &AssetManager,
    name: &str,
    read: impl Fn(ShpFile) -> Result<T>,
) -> Result<T> {
    let decode = |bytes: &[u8]| {
        let shp = if assets.mod_selected() {
            decode_ui_shp(bytes)
        } else {
            ShpFile::from_bytes(bytes)
        }
        .with_context(|| format!("Could not parse {name}"))?;
        read(shp)
    };
    if assets.mod_selected() {
        decode_ui_art(assets, name, decode)
    } else {
        let bytes = assets
            .get_ref(name)
            .with_context(|| format!("Required game-menu artwork {name} is missing or unreadable"))?;
        decode(bytes)
    }
}

/// [`read_palette`], logging why the palette is unavailable.
pub(crate) fn load_palette(assets: &AssetManager, name: &str) -> Option<Palette> {
    read_palette(assets, name)
        .map_err(|err| log::warn!("{err:#}"))
        .ok()
}

/// SHP `file` parsed; `None` (logged when unparsable) when unavailable.
pub(crate) fn load_shp(assets: &AssetManager, file: &str) -> Option<ShpFile> {
    if !assets.mod_selected() {
        return ShpFile::from_bytes(assets.get_ref(file)?)
            .map_err(|err| log::warn!("Could not parse {file}: {err:#}"))
            .ok();
    }
    read_ui_shp(assets, file, Ok)
        .map_err(|err| log::debug!("Could not load {file}: {err:#}"))
        .ok()
}

/// Frame `frame` of `shp` through `palette` (index 0 transparent), placed on
/// the SHP's full canvas.
pub(crate) fn shp_piece(
    shp: &ShpFile,
    label: &str,
    palette: &Palette,
    frame: usize,
) -> Result<ChromePiece> {
    let frame_rgba = shp
        .frame_to_rgba(frame, palette)
        .with_context(|| format!("Could not render {label} frame {frame}"))?;
    let (canvas_w, canvas_h) = (shp.width as u32, shp.height as u32);
    let shp_frame = &shp.frames[frame];
    let frame_w = shp_frame.frame_width as u32;
    let frame_h = shp_frame.frame_height as u32;
    let frame_x = shp_frame.frame_x as u32;
    let frame_y = shp_frame.frame_y as u32;
    let rgba = if frame_w == canvas_w && frame_h == canvas_h && frame_x == 0 && frame_y == 0 {
        frame_rgba
    } else {
        let mut canvas = vec![0u8; (canvas_w * canvas_h * 4) as usize];
        for row in 0..frame_h {
            let src = (row * frame_w * 4) as usize;
            let dst = (((frame_y + row) * canvas_w + frame_x) * 4) as usize;
            let len = (frame_w * 4) as usize;
            if src + len <= frame_rgba.len() && dst + len <= canvas.len() {
                canvas[dst..dst + len].copy_from_slice(&frame_rgba[src..src + len]);
            }
        }
        canvas
    };
    Ok(ChromePiece {
        label: label.to_ascii_lowercase(),
        width: canvas_w,
        height: canvas_h,
        rgba,
    })
}

/// Frame `frame` of SHP `file` through `palette`, labelled `label`.
pub(crate) fn shp_file_piece(
    assets: &AssetManager,
    file: &str,
    label: &str,
    palette: &Palette,
    frame: usize,
) -> Option<ChromePiece> {
    shp_piece(&load_shp(assets, file)?, label, palette, frame).ok()
}

/// A PCX through its embedded palette, full canvas, labelled by its
/// lower-case name. `keyed` applies [`PACKED_TRANSPARENT_RGB`].
pub(crate) fn pcx_piece(assets: &AssetManager, file: &str, keyed: bool) -> Option<ChromePiece> {
    let pcx = read_ui_pcx(assets, file)
        .map_err(|err| log::debug!("Could not load {file}: {err:#}"))
        .ok()?;
    let mut rgba = pcx.to_rgba(None);
    if keyed {
        ACTIVE_RETAIL_RGB565_PRESENTATION
            .apply_packed_color_key_rgba8(&mut rgba, PACKED_TRANSPARENT_RGB);
    }
    Some(ChromePiece {
        label: file.to_ascii_lowercase(),
        width: pcx.width as u32,
        height: pcx.height as u32,
        rgba,
    })
}

/// A `width` x `height` piece of one colour.
pub(crate) fn solid_piece(label: &str, width: u32, height: u32, color: [u8; 4]) -> ChromePiece {
    ChromePiece {
        label: label.into(),
        width,
        height,
        rgba: color.repeat((width * height) as usize),
    }
}

/// What a missing piece means: the atlas cannot load, a logged gap, or an
/// expected absence.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Need {
    Required,
    Optional,
    Silent,
}

/// One chrome atlas's pieces in packing order, decoded from the shell art.
pub(crate) struct ChromeArt<'a> {
    assets: &'a AssetManager,
    /// Names the atlas in missing-art warnings.
    owner: &'static str,
    pub(crate) pieces: Vec<ChromePiece>,
}

impl<'a> ChromeArt<'a> {
    pub(crate) fn new(assets: &'a AssetManager, owner: &'static str) -> Self {
        Self {
            assets,
            owner,
            pieces: Vec::new(),
        }
    }

    fn missing(&self, need: Need, what: std::fmt::Arguments) {
        match need {
            Need::Required => log::warn!("Missing mandatory {} asset {what}", self.owner),
            Need::Optional => log::warn!("Missing optional {} asset {what}", self.owner),
            Need::Silent => {}
        }
    }

    pub(crate) fn push(&mut self, piece: ChromePiece) {
        self.pieces.push(piece);
    }

    /// [`Self::push`] when present, else a missing `name` of `need`.
    pub(crate) fn push_or(&mut self, piece: Option<ChromePiece>, name: &str, need: Need) {
        match piece {
            Some(piece) => self.pieces.push(piece),
            None => self.missing(need, format_args!("{name}")),
        }
    }

    /// `frames` of SHP `file` through `palette`, each labelled `label#frame`
    /// and loaded on its own; `None` when any is missing.
    pub(crate) fn shp(
        &mut self,
        file: &str,
        palette: &Palette,
        frames: Range<usize>,
        label: &str,
        need: Need,
    ) -> Option<()> {
        let shp = load_shp(self.assets, file);
        let mut complete = true;
        for frame in frames {
            match shp
                .as_ref()
                .map(|shp| shp_piece(shp, &format!("{label}#{frame}"), palette, frame))
            {
                Some(Ok(piece)) => self.pieces.push(piece),
                _ => {
                    complete = false;
                    self.missing(need, format_args!("{file} frame {frame}"));
                }
            }
        }
        complete.then_some(())
    }

    /// SHP `file`'s frames from 0 up to the first that does not decode (at
    /// most `limit`), labelled `label#frame`; how many loaded.
    pub(crate) fn shp_run(
        &mut self,
        file: &str,
        palette: &Palette,
        label: &str,
        limit: usize,
    ) -> usize {
        let Some(shp) = load_shp(self.assets, file) else {
            return 0;
        };
        let start = self.pieces.len();
        self.pieces.extend(
            (0..limit.min(shp.frames.len())).map_while(|frame| {
                shp_piece(&shp, &format!("{label}#{frame}"), palette, frame).ok()
            }),
        );
        self.pieces.len() - start
    }

    /// Each of `files` as a PCX ([`pcx_piece`]).
    pub(crate) fn pcx<'n>(
        &mut self,
        files: impl IntoIterator<Item = &'n str>,
        keyed: bool,
        need: Need,
    ) {
        for file in files {
            match pcx_piece(self.assets, file, keyed) {
                Some(piece) => self.pieces.push(piece),
                None => self.missing(need, format_args!("{file}")),
            }
        }
    }

    /// Pack the pieces; their entries by label.
    pub(crate) fn pack(
        &self,
        gpu: &GpuContext,
        batch: &BatchRenderer,
        atlas_name: &str,
    ) -> Option<(BatchTexture, ChromeLabels)> {
        let (texture, entries) = pack_chrome_atlas(gpu, batch, atlas_name, &self.pieces)?;
        let labels = self
            .pieces
            .iter()
            .map(|piece| piece.label.clone())
            .zip(entries)
            .collect();
        Some((texture, ChromeLabels(labels)))
    }
}

/// A packed atlas's entries by piece label (a later duplicate wins).
pub(crate) struct ChromeLabels(HashMap<String, ChromeAtlasEntry>);

impl ChromeLabels {
    pub(crate) fn get(&self, label: &str) -> Option<ChromeAtlasEntry> {
        self.0.get(label).copied()
    }

    /// Frame `frame` of the SHP piece labelled `label`.
    pub(crate) fn frame(&self, label: &str, frame: usize) -> Option<ChromeAtlasEntry> {
        self.get(&format!("{label}#{frame}"))
    }

    /// Frames `0..N` of `label`, `None` where missing.
    pub(crate) fn frames<const N: usize>(&self, label: &str) -> [Option<ChromeAtlasEntry>; N] {
        std::array::from_fn(|frame| self.frame(label, frame))
    }

    /// The frames of `label` from 0 up to the first missing one.
    pub(crate) fn run(&self, label: &str) -> Vec<ChromeAtlasEntry> {
        (0..).map_while(|frame| self.frame(label, frame)).collect()
    }
}

/// One CPU-rendered RGBA chrome piece awaiting placement.
pub(crate) struct ChromePiece {
    pub(crate) label: String,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) rgba: Vec<u8>,
}

/// A packed piece's normalized rectangle and native pixel size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromeAtlasEntry {
    pub uv_origin: [f32; 2],
    pub uv_size: [f32; 2],
    pub pixel_size: [f32; 2],
}

impl ChromeAtlasEntry {
    /// The image's native width and height in pixels.
    pub fn native_size(self) -> (i32, i32) {
        (
            self.pixel_size[0].round() as i32,
            self.pixel_size[1].round() as i32,
        )
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ChromeAtlasLayout {
    width: u32,
    height: u32,
    origins: Vec<[u32; 2]>,
}

fn layout(sizes: &[(u32, u32)]) -> ChromeAtlasLayout {
    let width = sizes
        .iter()
        .map(|&(width, _)| width)
        .max()
        .unwrap_or(1)
        .max(MIN_ATLAS_WIDTH);
    let (mut x, mut y, mut row_height) = (0u32, 0u32, 0u32);
    let mut origins = Vec::with_capacity(sizes.len());
    for &(piece_width, piece_height) in sizes {
        // A row's first piece stays even when it is wider than the page
        // minus the trailing gap.
        if x > 0 && x + piece_width + ATLAS_PADDING > width {
            x = 0;
            y += row_height + ATLAS_PADDING;
            row_height = 0;
        }
        origins.push([x, y]);
        x += piece_width + ATLAS_PADDING;
        row_height = row_height.max(piece_height);
    }
    ChromeAtlasLayout {
        width,
        height: (y + row_height).next_power_of_two().max(1),
        origins,
    }
}

/// Pack `pieces` in order into one uploaded texture; `None` when empty.
/// Entries come back in the same order as `pieces`.
pub(crate) fn pack_chrome_atlas(
    gpu: &GpuContext,
    batch: &BatchRenderer,
    atlas_name: &str,
    pieces: &[ChromePiece],
) -> Option<(BatchTexture, Vec<ChromeAtlasEntry>)> {
    if pieces.is_empty() {
        return None;
    }
    let sizes: Vec<_> = pieces
        .iter()
        .map(|piece| (piece.width, piece.height))
        .collect();
    // Mod art replaces every piece; a page taller than the device allows
    // fails validation instead of drawing, and the callers already turn a
    // missing atlas into the startup message naming the art.
    let layout = layout(&sizes);
    if !crate::render::gpu::texture_fits(&gpu.device, [layout.width, layout.height]) {
        log::error!(
            "{atlas_name} atlas: {}x{} px does not fit this GPU's {} px textures",
            layout.width,
            layout.height,
            gpu.device.limits().max_texture_dimension_2d
        );
        return None;
    }
    let ChromeAtlasLayout {
        width,
        height,
        origins,
    } = layout;

    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for (piece, &[px, py]) in pieces.iter().zip(&origins) {
        let len = (piece.width * 4) as usize;
        for row in 0..piece.height {
            let src = (row * piece.width * 4) as usize;
            let dst = (((py + row) * width + px) * 4) as usize;
            rgba[dst..dst + len].copy_from_slice(&piece.rgba[src..src + len]);
        }
    }

    log::info!(
        "{atlas_name} atlas: {width}x{height} px, {} pieces",
        pieces.len()
    );
    for piece in pieces {
        log::debug!("  {}: {}x{}", piece.label, piece.width, piece.height);
    }

    let texture = batch.create_texture(gpu, &rgba, width, height);
    let entries = pieces
        .iter()
        .zip(origins)
        .map(|(piece, [px, py])| ChromeAtlasEntry {
            uv_origin: [px as f32 / width as f32, py as f32 / height as f32],
            uv_size: [
                piece.width as f32 / width as f32,
                piece.height as f32 / height as f32,
            ],
            pixel_size: [piece.width as f32, piece.height as f32],
        })
        .collect();
    Some((texture, entries))
}

#[cfg(all(test, tests_render_a_r))]
mod tests {
    use super::*;

    /// Expected rectangles come from running the three former per-screen
    /// loops on this input: an over-wide first piece, an exact-width fit and
    /// two wraps.
    #[test]
    fn chrome_layout_keeps_former_shelf_rectangles() {
        let sizes = [
            (1030, 20),
            (800, 600),
            (226, 30),
            (2, 1),
            (640, 480),
            (300, 16),
        ];
        assert_eq!(
            layout(&sizes),
            ChromeAtlasLayout {
                width: 1030,
                height: 2048,
                origins: vec![[0, 0], [0, 22], [802, 22], [0, 624], [4, 624], [646, 624]],
            }
        );
    }
}
