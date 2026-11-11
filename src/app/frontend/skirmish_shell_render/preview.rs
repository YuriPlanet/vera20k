//! Preview texture and start-marker sprite helpers for the skirmish shell.
//!
//! Keeps preview decoding, random-map preview cache handling, and live
//! marker sprite projection separate from the shell render orchestration.

use std::cell::OnceCell;
use std::path::Path;

use crate::app::AppState;
use crate::app::loading::init::MapMenuEntry;
use crate::assets::asset_manager::AssetManager;
use crate::map::preview::{DecodedPreview, PreviewSourceBounds};
use crate::render::batch::{BatchTexture, SpriteInstance};
use crate::render::bit_font::BitFont;
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::rules::ini_parser::IniFile;
use crate::ui::shell::geom::ShellScale;
use crate::ui::skirmish_shell::RectPx;

use super::chrome::push_entry_grown;
use super::{
    RANDMAP_PREVIEW_FILE_NAME, RANDMAP_SENTINEL_FILE_NAME, SHELL_PREVIEW_SURFACE_DEPTH,
    START_MARKER_OFFSET_X, START_MARKER_OFFSET_Y,
};

pub(crate) struct SkirmishPreviewTexture {
    pub selected_map_idx: usize,
    pub texture: BatchTexture,
    /// The size the preview box fits: the texture's own, or for VERA's
    /// picture of the map the size of the map's own preview whose place it
    /// shows, so both fill the same rectangle.
    pub width: u32,
    pub height: u32,
    /// Set when the texture holds a random-map setup preview rather than a
    /// chooser map's thumbnail, carrying the generation it was built from.
    pub setup_preview_generation: Option<u32>,
    /// Set when the texture holds a LAN guest's received `Preview.bin`,
    /// carrying its arrival number.
    pub received_preview: Option<u32>,
    /// Whether Options' HD Map Previews was on when a chooser map's texture
    /// was made; a change remakes it.
    pub hd_setting: bool,
    /// When the texture holds VERA's own picture of the map
    /// (`app::map_picture`) rather than the map's `[PreviewPack]`, the size
    /// of the preview box it was shrunk for.
    pub hd_picture: Option<(i32, i32)>,
    /// The picture the texture was made from, at its own size, for the
    /// enlarged preview (`frontend::preview_zoom`).
    pub source: DecodedPreview,
    /// The enlarged preview's texture of `source`, and the size it was made
    /// for ([`ensure_zoom_texture`]).
    pub zoom: Option<(BatchTexture, (u32, u32))>,
    /// The map's briefing over the enlarged preview
    /// ([`crate::app::map_library::briefing`]), read when that first shows;
    /// none for a received or generated picture.
    pub briefing: OnceCell<Option<String>>,
}

/// The enlarged preview's margin from the window's edges, in retail pixels.
const ZOOM_MARGIN: f32 = 12.0;
/// The opacity of the black over the screen behind the enlarged preview. The
/// shell target blends in linear light, so the screen keeps 7% of its light,
/// which shows at about 30% of its brightness.
const ZOOM_DIM: f32 = 0.93;
/// The enlarged picture's frame.
const ZOOM_FRAME_RGB: [f32; 3] = [0.55, 0.55, 0.55];
/// Above every shell draw but the cursor; shell draws keep their order.
const ZOOM_DEPTH: f32 = 0.00020;

fn preview_fits_texture(width: u32, height: u32, rgba_len: usize, maximum_side: u32) -> bool {
    width != 0
        && height != 0
        && width <= maximum_side
        && height <= maximum_side
        && u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4))
            == Some(rgba_len as u64)
}

/// A LAN guest whose map did not verify shows the host's `Preview.bin`, or
/// nothing until it arrives (`0x005E1D3B` skips a null preview).
pub(super) fn ensure_received_preview_texture(state: &mut AppState) {
    let Some((arrival, preview)) = state
        .frontend
        .network_setup_protocol
        .as_ref()
        .and_then(|protocol| protocol.received_preview())
    else {
        state.frontend.skirmish_preview_texture = None;
        return;
    };
    // Network admission bounds the format; the live device can request a
    // smaller edge limit. Refuse an unavailable surface before wgpu validation.
    if !preview_fits_texture(
        preview.width,
        preview.height,
        preview.rgba.len(),
        state.renderer.gpu.device.limits().max_texture_dimension_2d,
    ) {
        state.frontend.skirmish_preview_texture = None;
        return;
    }
    if state
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .is_some_and(|texture| texture.received_preview == Some(arrival))
    {
        return;
    }
    let texture = state.renderer.batch_renderer.create_texture(
        &state.renderer.gpu,
        &preview.rgba,
        preview.width,
        preview.height,
    );
    let (width, height) = (preview.width, preview.height);
    let source = preview.clone();
    state.frontend.skirmish_preview_texture = Some(SkirmishPreviewTexture {
        selected_map_idx: usize::MAX,
        texture,
        width,
        height,
        setup_preview_generation: None,
        received_preview: Some(arrival),
        hd_setting: false,
        hd_picture: None,
        source,
        zoom: None,
        briefing: OnceCell::from(None),
    });
}

pub(super) fn push_start_marker_sprites(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    projected_positions: &[(i32, i32)],
    scale: ShellScale,
    depth: f32,
) {
    let Some(marker) = atlas.start_marker else {
        return;
    };
    for &(x, y) in projected_positions {
        let (marker_x, marker_y) = start_marker_top_left(x, y, scale);
        push_entry_grown(out, marker, marker_x, marker_y, scale, depth);
    }
}

/// The marker is authored to sit on its projected anchor; a stretched preview
/// grows the art and the offset together, so it stays centred on the same point.
pub(super) fn start_marker_top_left(anchor_x: i32, anchor_y: i32, scale: ShellScale) -> (i32, i32) {
    (
        anchor_x + (START_MARKER_OFFSET_X as f32 * scale.factor()).round() as i32,
        anchor_y + (START_MARKER_OFFSET_Y as f32 * scale.factor()).round() as i32,
    )
}

/// One start marker as `DrawStartPositions 0x00640710` draws it at retail
/// scale, for pictures of a map drawn outside the shell: RGBA, with the anchor
/// at `origin`.
pub(crate) struct StartMarkerPicture {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) origin: (i32, i32),
    pub(crate) rgba: Vec<u8>,
}

/// `art` (STARTBUT.SHP) at the marker offset, then `label` in `font` at the
/// number's offset and colour, its glyphs advanced as `BitFont::build_text`
/// advances them. None when `font` lacks one of the label's glyphs.
pub(crate) fn start_marker_picture(
    art: &crate::render::chrome_atlas::ChromePiece,
    font: &crate::assets::fnt_file::FntFile,
    label: &str,
) -> Option<StartMarkerPicture> {
    let glyphs = label
        .chars()
        .map(|ch| {
            u16::try_from(u32::from(ch))
                .ok()
                .and_then(|cp| font.glyph(cp))
        })
        .collect::<Option<Vec<_>>>()?;
    let spacing = crate::render::bit_font::CHAR_SPACING as i32;
    let text_w = font.text_width(label) as i32;
    let sprite = start_marker_top_left(0, 0, ShellScale::RETAIL);
    let text = super::text::start_marker_label_origin(0, 0, ShellScale::RETAIL);
    let (left, top) = (sprite.0.min(text.0), sprite.1.min(text.1));
    let right = (sprite.0 + art.width as i32).max(text.0 + text_w);
    let bottom = (sprite.1 + art.height as i32).max(text.1 + font.bitmap_rows as i32);
    let (width, height) = ((right - left) as u32, (bottom - top) as u32);
    let mut rgba = vec![0; width as usize * height as usize * 4];
    // Both the art and the glyphs are wholly opaque or clear.
    let mut put = |source: &[u8], source_w: u32, x: i32, y: i32, tint: Option<[u8; 3]>| {
        for (index, pixel) in source.chunks_exact(4).enumerate() {
            if pixel[3] == 0 {
                continue;
            }
            let (px, py) = (
                x + (index as u32 % source_w) as i32,
                y + (index as u32 / source_w) as i32,
            );
            let at = (py as usize * width as usize + px as usize) * 4;
            rgba[at..at + 3].copy_from_slice(&tint.unwrap_or([pixel[0], pixel[1], pixel[2]]));
            rgba[at + 3] = 255;
        }
    };
    put(&art.rgba, art.width, sprite.0 - left, sprite.1 - top, None);
    let tint =
        super::text::start_marker_label_color().map(|channel| (channel * 255.0).round() as u8);
    let mut x = text.0 - left;
    for glyph in glyphs {
        put(&glyph.rgba, glyph.width, x, text.1 - top, Some(tint));
        x += glyph.width as i32 + spacing;
    }
    Some(StartMarkerPicture {
        width,
        height,
        origin: (-left, -top),
        rgba,
    })
}

pub(super) fn project_preview_start_positions(
    bounds: &PreviewSourceBounds,
    fitted_preview_rect: RectPx,
) -> Vec<(i32, i32)> {
    if fitted_preview_rect.w <= 0
        || fitted_preview_rect.h <= 0
        || bounds.width == 0
        || bounds.height == 0
    {
        return Vec::new();
    }

    // VERA authored maps can expose thirty admitted starts. The projection
    // remains the same for native maps. Reject a malformed header as a whole
    // rather than dropping one point and renumbering every later start label.
    bounds
        .start_points
        .iter()
        .take(crate::skirmish_launch::NETWORK_PLAYER_SLOT_COUNT)
        .map(|point| {
            let x_per_mille = ((i64::from(point.x) - i64::from(bounds.origin_x)) * 1000)
                / i64::from(bounds.width);
            let y_per_mille = ((i64::from(point.y) - i64::from(bounds.origin_y)) * 1000)
                / i64::from(bounds.height);
            let x = i64::from(fitted_preview_rect.x)
                .checked_add(x_per_mille.checked_mul(i64::from(fitted_preview_rect.w))? / 1000)?;
            let y = i64::from(fitted_preview_rect.y)
                .checked_add(y_per_mille.checked_mul(i64::from(fitted_preview_rect.h))? / 1000)?;
            Some((i32::try_from(x).ok()?, i32::try_from(y).ok()?))
        })
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

pub(super) fn build_start_marker_instances(
    atlas: &SkirmishShellChromeAtlas,
    projected_positions: &[(i32, i32)],
    scale: ShellScale,
) -> Vec<SpriteInstance> {
    let mut instances = Vec::new();
    push_start_marker_sprites(&mut instances, atlas, projected_positions, scale, 0.00056);
    instances
}

pub(super) fn selected_preview_texture_is_current(
    state: &AppState,
    selected_map_idx: usize,
    preview_box: RectPx,
) -> bool {
    state
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .is_some_and(|cached| {
            // A setup-dialog preview happens to carry the selected index too, so it
            // has to be rejected explicitly or closing the dialog would leave the
            // generated image standing in for the chooser's own thumbnail.
            cached.setup_preview_generation.is_none()
                && cached.received_preview.is_none()
                && cached.selected_map_idx == selected_map_idx
                && cached.hd_setting == state.hd_map_previews()
                && cached
                    .hd_picture
                    .is_none_or(|size| size == (preview_box.w, preview_box.h))
        })
}

pub(super) fn is_random_map_sentinel_file_name(file_name: &str) -> bool {
    // Map names can carry the game's own `\` separators, so split on both `\`
    // and `/` on every host (`Path::file_name` only knows the host's).
    let name = file_name.rsplit(['\\', '/']).next().unwrap_or(file_name);
    name.eq_ignore_ascii_case(RANDMAP_SENTINEL_FILE_NAME)
}

pub(crate) fn is_random_map_sentinel_entry(entry: &MapMenuEntry) -> bool {
    is_random_map_sentinel_file_name(&entry.file_name)
}

pub(super) fn decode_randmap_preview_from_runtime_file()
-> Option<crate::map::preview::DecodedPreview> {
    let config = crate::util::config::GameConfig::load().ok()?;
    let preview_path =
        crate::map::source::random_map_dir(&config.paths.ra2_dir).join(RANDMAP_PREVIEW_FILE_NAME);
    let bytes = match crate::util::file_system::read(&preview_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::warn!(
                "Failed to read runtime random map preview {}: {err}",
                preview_path.display()
            );
            return None;
        }
    };
    let pcx = match crate::assets::pcx_file::PcxFile::from_bytes(&bytes) {
        Ok(pcx) => pcx,
        Err(err) => {
            log::warn!(
                "Failed to decode runtime random map preview {}: {err}",
                preview_path.display()
            );
            return None;
        }
    };

    Some(crate::map::preview::DecodedPreview {
        width: u32::from(pcx.width),
        height: u32::from(pcx.height),
        rgba: pcx.to_rgba(None),
    })
}

pub(super) fn decode_preview_for_map_entry(
    entry: &MapMenuEntry,
    ra2_dir: Option<&Path>,
    assets: Option<&AssetManager>,
) -> Option<DecodedPreview> {
    if is_random_map_sentinel_entry(entry) {
        return decode_randmap_preview_from_runtime_file();
    }

    if let Some(decoded) = entry.preview.decoded.as_ref() {
        return Some(decoded.clone());
    }

    if let Some(ra2_dir) = ra2_dir {
        // The game folder, then VERA's Maps folder, as every map lookup.
        let ini_path = crate::map::source::local_map_path(ra2_dir, &entry.file_name)
            .unwrap_or_else(|_| ra2_dir.join(&entry.file_name));
        if let Some(ini) = crate::map::source::read_map_ini_for_metadata(&ini_path)
            && let Some(preview) = decode_preview_from_ini(&ini, &entry.file_name)
        {
            return Some(preview);
        }
    }

    if let Some(assets) = assets {
        for candidate in crate::map::source::asset_map_candidates(&entry.file_name) {
            if let Some(bytes) = assets.get_ref(&candidate)
                && let Some(preview) = decode_preview_from_map_bytes(bytes, &candidate)
            {
                return Some(preview);
            }
        }
    }

    None
}

pub(super) fn decode_preview_from_map_bytes(bytes: &[u8], source: &str) -> Option<DecodedPreview> {
    let ini = match IniFile::from_bytes(bytes) {
        Ok(ini) => ini,
        Err(err) => {
            log::warn!("Failed to parse map INI for preview from {source}: {err}");
            return None;
        }
    };
    decode_preview_from_ini(&ini, source)
}

pub(super) fn decode_preview_from_ini(ini: &IniFile, source: &str) -> Option<DecodedPreview> {
    match crate::map::preview::decode_preview_image_from_ini(ini) {
        Ok(preview) => preview,
        Err(err) => {
            log::warn!("Failed to lazily decode map PreviewPack for {source}: {err}");
            None
        }
    }
}

/// Keep the preview texture in step with the setup dialog's generated image.
///
/// Returns true when the setup dialog owns the preview, so the caller skips the
/// chooser's own texture handling. While the dialog is up the box shows the
/// generated map and nothing else — before the first generate it stays empty
/// rather than borrowing whatever the chooser underneath had selected.
pub(super) fn ensure_random_map_setup_preview_texture(state: &mut AppState) -> bool {
    let Some(modal) = state
        .frontend
        .skirmish_shell_state
        .random_map_setup_modal
        .as_ref()
    else {
        return false;
    };
    let Some(preview) = modal.generated_preview.as_ref() else {
        state.frontend.skirmish_preview_texture = None;
        return true;
    };
    if !preview_fits_texture(
        preview.width,
        preview.height,
        preview.rgba.len(),
        state.renderer.gpu.device.limits().max_texture_dimension_2d,
    ) {
        state.frontend.skirmish_preview_texture = None;
        return true;
    }
    let generation = modal.preview_generation;
    let already_current = state
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .is_some_and(|texture| texture.setup_preview_generation == Some(generation));
    if already_current {
        return true;
    }

    let texture = state.renderer.batch_renderer.create_texture(
        &state.renderer.gpu,
        &preview.rgba,
        preview.width,
        preview.height,
    );
    let source = DecodedPreview {
        width: preview.width,
        height: preview.height,
        rgba: preview.rgba.clone(),
    };
    state.frontend.skirmish_preview_texture = Some(SkirmishPreviewTexture {
        selected_map_idx: state.frontend.skirmish_shell_state.selected_map_idx,
        texture,
        width: source.width,
        height: source.height,
        setup_preview_generation: Some(generation),
        received_preview: None,
        hd_setting: false,
        hd_picture: None,
        source,
        zoom: None,
        briefing: OnceCell::from(None),
    });
    true
}

/// The map the preview shows: while Choose Map shows, its highlighted map
/// (the loop callback `0x005E66F0` reloads the preview for every new
/// highlight and restores the session's map afterwards); otherwise the
/// committed one.
pub(super) fn preview_map_idx(state: &AppState) -> usize {
    let shell = &state.frontend.skirmish_shell_state;
    shell
        .choose_map_modal
        .as_ref()
        .filter(|_| shell.random_map_setup_modal.is_none() && shell.saved_seed_browser.is_none())
        .and_then(|modal| modal.selected_record_index())
        .and_then(|record| state.frontend.scenario_catalog.records().get(record))
        .and_then(|record| {
            state
                .frontend
                .scenario_catalog
                .shell_maps()
                .iter()
                .position(|map| map.file_name.eq_ignore_ascii_case(&record.file_name))
        })
        .unwrap_or(shell.selected_map_idx)
}

pub(super) fn ensure_selected_preview_texture(state: &mut AppState, preview_box: RectPx) {
    if ensure_random_map_setup_preview_texture(state) {
        return;
    }
    let selected_map_idx = preview_map_idx(state);
    ensure_map_preview_texture(state, selected_map_idx, preview_box);
}

/// Shared map preview cache used by offline and network setup/chooser pages,
/// for a preview drawn in `preview_box`.
pub(super) fn ensure_map_preview_texture(
    state: &mut AppState,
    selected_map_idx: usize,
    preview_box: RectPx,
) {
    let selected_entry = state
        .frontend
        .scenario_catalog
        .shell_maps()
        .get(selected_map_idx)
        .cloned();
    // PreviewClass689D30 applies Header reads even when no preview bitmap is
    // available. The retained Scenario header belongs to the process runtime.
    if let Some(entry) = selected_entry.as_ref() {
        state
            .frontend
            .multiplayer_shell_runtime
            .observe_map_preview(entry);
    }
    let selected_is_random_sentinel = selected_entry
        .as_ref()
        .is_some_and(is_random_map_sentinel_entry);
    if !selected_is_random_sentinel
        && selected_preview_texture_is_current(state, selected_map_idx, preview_box)
    {
        return;
    }
    let hd_setting = state.hd_map_previews();
    let ra2_dir = state.ra2_dir();
    let decoded = selected_entry
        .as_ref()
        .and_then(|entry| {
            decode_preview_for_map_entry(entry, ra2_dir, state.process_assets.manager())
        })
        // An online library row has no file: its catalog header holds it.
        .or_else(|| crate::app::map_library::online_preview(state, selected_map_idx));
    // VERA's own picture of the map, once one is kept, shrunk here by area to
    // the size the box shows it at: it is up to eight times that size, and the
    // GPU's linear filter, which reads four texels, would alias the shrink.
    if hd_setting
        && selected_entry.is_some()
        && !selected_is_random_sentinel
        && let Some(picture) = crate::app::map_picture::picture_for_map(state, selected_map_idx)
        && preview_fits_texture(
            picture.width,
            picture.height,
            picture.rgba.len(),
            state.renderer.gpu.device.limits().max_texture_dimension_2d,
        )
    {
        // The picture shows the place of the map's own preview in that
        // preview's shape (`map_picture::placement`), so fitted by the
        // preview's size it fills the very rectangle the preview did and the
        // map stays put as one replaces the other. A picture of another
        // shape found no such place.
        let own = decoded
            .as_ref()
            .map(|own| (own.width, own.height))
            .filter(|&own| same_shape(own, (picture.width, picture.height)));
        let (fit_w, fit_h) = own.unwrap_or((picture.width, picture.height));
        let fitted = aspect_fit_rect(preview_box, fit_w, fit_h);
        let texture = picture_texture(state, &picture, (fitted.w, fitted.h));
        state.frontend.skirmish_preview_texture = Some(SkirmishPreviewTexture {
            selected_map_idx,
            texture,
            width: fit_w,
            height: fit_h,
            setup_preview_generation: None,
            received_preview: None,
            hd_setting,
            hd_picture: Some((preview_box.w, preview_box.h)),
            source: picture,
            zoom: None,
            briefing: OnceCell::new(),
        });
        return;
    }

    let Some(decoded) = decoded else {
        state.frontend.skirmish_preview_texture = None;
        return;
    };
    if !preview_fits_texture(
        decoded.width,
        decoded.height,
        decoded.rgba.len(),
        state.renderer.gpu.device.limits().max_texture_dimension_2d,
    ) {
        state.frontend.skirmish_preview_texture = None;
        return;
    }

    let texture = state.renderer.batch_renderer.create_texture(
        &state.renderer.gpu,
        &decoded.rgba,
        decoded.width,
        decoded.height,
    );
    state.frontend.skirmish_preview_texture = Some(SkirmishPreviewTexture {
        selected_map_idx,
        texture,
        width: decoded.width,
        height: decoded.height,
        setup_preview_generation: None,
        received_preview: None,
        hd_setting,
        hd_picture: None,
        source: decoded,
        zoom: None,
        briefing: OnceCell::new(),
    });
}

/// A texture of `picture` for drawing it `size` large: shrunk here by area
/// when that is smaller, else at its own size for the GPU's linear filter to
/// enlarge.
fn picture_texture(state: &AppState, picture: &DecodedPreview, size: (i32, i32)) -> BatchTexture {
    let shrunk = (size.0 > 0 && size.1 > 0 && (size.0 as u32) < picture.width).then(|| {
        let (width, height) = (size.0 as u32, size.1 as u32);
        let rgba = crate::render::area_scale::scale(
            &picture.rgba,
            picture.width as usize,
            picture.height as usize,
            width as usize,
            height as usize,
        );
        (width, height, rgba)
    });
    let (width, height, rgba) = match &shrunk {
        Some((width, height, rgba)) => (*width, *height, rgba.as_slice()),
        None => (picture.width, picture.height, picture.rgba.as_slice()),
    };
    let (_, texture) = state.renderer.batch_renderer.create_updatable_texture(
        &state.renderer.gpu.device,
        &state.renderer.gpu.queue,
        rgba,
        width,
        height,
        wgpu::FilterMode::Linear,
    );
    texture
}

/// The briefing box's least distance from the enlarged picture's edges, and
/// its margin around its text, in retail pixels.
const BRIEFING_INSET: f32 = 12.0;
const BRIEFING_PADDING: f32 = 6.0;
/// The briefing box's most width, in retail pixels.
const BRIEFING_MAX_WIDTH: f32 = 560.0;
/// The last line of a briefing cut to fit.
const BRIEFING_CUT: &str = "...";

/// Where the enlarged preview's parts go, in window pixels.
pub(super) struct ZoomLayout {
    /// The rectangle the picture fills.
    picture: RectPx,
    /// The map's briefing over the picture, while it shows.
    briefing: Option<BriefingBox>,
}

/// The map's briefing in a box over the middle of the enlarged picture.
struct BriefingBox {
    /// The box, darkened and framed as the screen behind the picture is.
    frame: RectPx,
    /// Its lines, wrapped to the box.
    lines: Vec<String>,
}

/// The enlarged `picture` (its width and height) fitted in `area`, with the
/// map's `briefing` over it ([`briefing_box`]).
fn zoom_layout(
    area: RectPx,
    picture: (u32, u32),
    briefing: Option<&str>,
    font: &BitFont,
    scale: f32,
) -> ZoomLayout {
    let fitted = aspect_fit_rect(area, picture.0, picture.1);
    ZoomLayout {
        picture: fitted,
        briefing: briefing.and_then(|text| briefing_box(fitted, text, font, scale)),
    }
}

/// `text` in a box centred over `picture`, as lobby programs show a map's
/// briefing over its preview: as wide as its widest line, at most
/// `BRIEFING_MAX_WIDTH`, and no taller than the picture, its last line "..."
/// when not every line fits. `None` when the picture has no room for a line.
fn briefing_box(picture: RectPx, text: &str, font: &BitFont, scale: f32) -> Option<BriefingBox> {
    let px = |retail: f32| (retail * scale).round() as i32;
    let (inset, padding) = (px(BRIEFING_INSET), px(BRIEFING_PADDING));
    let line_height = font.cell_height() * scale;
    let most_width = (picture.w - 2 * inset).min(px(BRIEFING_MAX_WIDTH));
    let most_lines = ((picture.h - 2 * inset - 2 * padding) as f32 / line_height) as usize;
    if most_width <= 2 * padding || most_lines == 0 {
        return None;
    }
    // Wrapped in the font's own pixels; the glyphs draw `scale` times their
    // size.
    let wrapped = font.wrap_layout(text, ((most_width - 2 * padding) as f32 / scale) as u32);
    let mut lines: Vec<String> = wrapped
        .lines
        .iter()
        .map(|line| text[line.start_byte..line.end_byte].to_owned())
        .collect();
    if lines.len() > most_lines {
        lines.truncate(most_lines - 1);
        lines.push(BRIEFING_CUT.to_owned());
    }
    let width = ((wrapped.width as f32 * scale).ceil() as i32 + 2 * padding).min(most_width);
    let height = (lines.len() as f32 * line_height).ceil() as i32 + 2 * padding;
    Some(BriefingBox {
        frame: RectPx::new(
            picture.x + (picture.w - width) / 2,
            picture.y + (picture.h - height) / 2,
            width,
            height,
        ),
        lines,
    })
}

/// Keep the enlarged preview's texture made for the window while it shows,
/// and close it once the box it enlarges is no longer `shown` or holds no
/// picture. Where its parts go, while it shows.
pub(super) fn ensure_zoom_texture(
    state: &mut AppState,
    scale: ShellScale,
    shown: bool,
) -> Option<ZoomLayout> {
    if !crate::app::frontend::preview_zoom::is_open(state) {
        if let Some(preview) = state.frontend.skirmish_preview_texture.as_mut() {
            preview.zoom = None;
        }
        return None;
    }
    let margin = (ZOOM_MARGIN * scale.factor()).round() as i32;
    let area = RectPx::new(
        margin,
        margin,
        state.render_width() as i32 - 2 * margin,
        state.render_height() as i32 - 2 * margin,
    );
    let shared: &AppState = state;
    let layout = shared
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .filter(|_| shown)
        .map(|preview| {
            let briefing = preview.briefing.get_or_init(|| {
                crate::app::map_library::briefing(shared, preview.selected_map_idx)
            });
            zoom_layout(
                area,
                (preview.width, preview.height),
                briefing.as_deref(),
                &shared.renderer.bit_font,
                scale.factor(),
            )
        })
        .filter(|layout| layout.picture.w > 0 && layout.picture.h > 0);
    let Some(mut layout) = layout else {
        crate::app::frontend::preview_zoom::close(state);
        return None;
    };
    if let Some(briefing) = &layout.briefing
        && !crate::app::frontend::preview_zoom::shows_briefing(state, briefing.frame)
    {
        layout.briefing = None;
    }
    let fitted = layout.picture;
    let preview = state
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .expect("a fitted picture");
    let size = (fitted.w as u32, fitted.h as u32);
    if preview
        .zoom
        .as_ref()
        .is_none_or(|(_, made_for)| *made_for != size)
    {
        let texture = picture_texture(state, &preview.source, (fitted.w, fitted.h));
        state
            .frontend
            .skirmish_preview_texture
            .as_mut()
            .expect("a fitted picture")
            .zoom = Some((texture, size));
    }
    Some(layout)
}

/// The enlarged preview's draws, made before the pass that draws them
/// ([`draw_zoom`]): the screen darkened behind a framed picture, with the
/// box's start markers placed over it as the box places them, and over them
/// the map's briefing in a darkened, framed box.
pub(super) struct ZoomDraws {
    /// The darkening and the picture's frame, from the shell atlas.
    backdrop: Vec<SpriteInstance>,
    picture: SpriteInstance,
    /// The markers from the shell atlas, their numbers in the bit font.
    markers: Vec<SpriteInstance>,
    numbers: Vec<SpriteInstance>,
    /// The briefing's box from the shell atlas, its lines in the bit font.
    briefing: Vec<SpriteInstance>,
    briefing_text: Vec<SpriteInstance>,
}

pub(super) struct ZoomBuffers {
    backdrop: Option<(wgpu::Buffer, u32)>,
    picture: Option<(wgpu::Buffer, u32)>,
    markers: Option<(wgpu::Buffer, u32)>,
    numbers: Option<(wgpu::Buffer, u32)>,
    briefing: Option<(wgpu::Buffer, u32)>,
    briefing_text: Option<(wgpu::Buffer, u32)>,
}

/// The enlarged preview as `layout` places it, with the start markers of
/// `bounds` when the box draws them.
pub(super) fn zoom_draws(
    state: &AppState,
    atlas: &SkirmishShellChromeAtlas,
    layout: &ZoomLayout,
    bounds: Option<&PreviewSourceBounds>,
    scale: ShellScale,
) -> ZoomDraws {
    let fitted = layout.picture;
    let font = &state.renderer.bit_font;
    // The screen and the briefing's box are darkened alike, and framed as
    // the picture is.
    let darken = |out: &mut Vec<SpriteInstance>, rect: RectPx| {
        out.extend(atlas.white_pixel.map(|pixel| SpriteInstance {
            position: [rect.x as f32, rect.y as f32],
            size: [rect.w as f32, rect.h as f32],
            uv_origin: pixel.uv_origin,
            uv_size: pixel.uv_size,
            depth: ZOOM_DEPTH,
            tint: [0.0; 3],
            alpha: ZOOM_DIM,
            ..Default::default()
        }));
    };
    let frame = |out: &mut Vec<SpriteInstance>, rect: RectPx| {
        super::chrome::push_rect_outline(
            out,
            atlas,
            RectPx::new(rect.x - 1, rect.y - 1, rect.w + 2, rect.h + 2),
            ZOOM_FRAME_RGB,
            ZOOM_DEPTH,
        );
    };
    let mut backdrop = Vec::new();
    darken(
        &mut backdrop,
        RectPx::new(
            0,
            0,
            state.render_width() as i32,
            state.render_height() as i32,
        ),
    );
    frame(&mut backdrop, fitted);
    let positions = bounds
        .map(|bounds| project_preview_start_positions(bounds, fitted))
        .unwrap_or_default();
    let mut markers = Vec::new();
    push_start_marker_sprites(&mut markers, atlas, &positions, scale, ZOOM_DEPTH);
    let mut numbers = Vec::new();
    super::text::push_start_marker_labels(&mut numbers, state, &positions, scale, ZOOM_DEPTH);
    let mut briefing = Vec::new();
    let mut briefing_text = Vec::new();
    if let Some(shown) = &layout.briefing {
        darken(&mut briefing, shown.frame);
        frame(&mut briefing, shown.frame);
        let padding = (BRIEFING_PADDING * scale.factor()).round() as i32;
        let line_height = font.cell_height() * scale.factor();
        for (index, line) in shown.lines.iter().enumerate() {
            briefing_text.extend(font.build_text(
                line,
                (shown.frame.x + padding) as f32,
                (shown.frame.y + padding) as f32 + index as f32 * line_height,
                scale.factor(),
                ZOOM_DEPTH,
                super::SHELL_LABEL_TEXT_RGB,
                [0.0, 0.0],
            ));
        }
    }
    ZoomDraws {
        backdrop,
        picture: SpriteInstance {
            position: [fitted.x as f32, fitted.y as f32],
            size: [fitted.w as f32, fitted.h as f32],
            uv_origin: [0.0, 0.0],
            uv_size: [1.0, 1.0],
            depth: ZOOM_DEPTH,
            tint: [1.0, 1.0, 1.0],
            alpha: 1.0,
            ..Default::default()
        },
        markers,
        numbers,
        briefing,
        briefing_text,
    }
}

impl ZoomDraws {
    pub(super) fn buffers(&self, state: &AppState) -> ZoomBuffers {
        let batch = &state.renderer.batch_renderer;
        let gpu = &state.renderer.gpu;
        ZoomBuffers {
            backdrop: batch.create_instance_buffer(gpu, &self.backdrop),
            picture: batch.create_instance_buffer(gpu, &[self.picture]),
            markers: batch.create_instance_buffer(gpu, &self.markers),
            numbers: batch.create_instance_buffer(gpu, &self.numbers),
            briefing: batch.create_instance_buffer(gpu, &self.briefing),
            briefing_text: batch.create_instance_buffer(gpu, &self.briefing_text),
        }
    }
}

/// Draw the enlarged preview over everything but the cursor; the caller's
/// scissor must cover the window.
pub(super) fn draw_zoom<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    state: &'a AppState,
    atlas: &'a SkirmishShellChromeAtlas,
    buffers: &'a ZoomBuffers,
) {
    let batch = &state.renderer.batch_renderer;
    let Some((zoom, _)) = state
        .frontend
        .skirmish_preview_texture
        .as_ref()
        .and_then(|preview| preview.zoom.as_ref())
    else {
        return;
    };
    let font = state.renderer.bit_font.atlas();
    for (buffer, texture) in [
        (&buffers.backdrop, &atlas.texture),
        (&buffers.picture, zoom),
        (&buffers.markers, &atlas.texture),
        (&buffers.numbers, font),
        (&buffers.briefing, &atlas.texture),
        (&buffers.briefing_text, font),
    ] {
        if let Some((buffer, count)) = buffer {
            batch.draw_with_buffer_passthrough(pass, texture, buffer, *count);
        }
    }
}

pub(super) fn should_draw_start_marker_overlays(
    fitted_preview_rect: Option<RectPx>,
    projected_start_positions: &[(i32, i32)],
) -> bool {
    fitted_preview_rect.is_some() && !projected_start_positions.is_empty()
}

/// The header `DrawStartPositions 0x00640710` numbers the box's starts from:
/// a random map's preview takes the last Generate's (`generated`, the
/// `0x0058B820` globals), drawn only when `1 <= count <= 8`; any other map
/// its own.
pub(in crate::app::frontend) fn start_marker_bounds(
    generated: Option<crate::map::preview::ScenarioStartStaging>,
    own: Option<&PreviewSourceBounds>,
) -> Option<PreviewSourceBounds> {
    match generated {
        Some(header) => header.preview_source_bounds(),
        None => own.cloned(),
    }
}

/// Whether `picture` has `own`'s proportions, each side rounded to within a
/// pixel.
fn same_shape(own: (u32, u32), picture: (u32, u32)) -> bool {
    let cross = |a: u32, b: u32| u64::from(a) * u64::from(b);
    cross(own.0, picture.1).abs_diff(cross(own.1, picture.0)) <= u64::from(own.0) + u64::from(own.1)
}

pub(super) fn aspect_fit_rect(dst: RectPx, src_w: u32, src_h: u32) -> RectPx {
    if dst.w <= 0 || dst.h <= 0 || src_w == 0 || src_h == 0 {
        return RectPx::new(dst.x, dst.y, 0, 0);
    }

    let src_w = src_w as i32;
    let src_h = src_h as i32;
    let scale_w = dst.w * 1000 / src_w;
    let scale_h = dst.h * 1000 / src_h;
    let scale = scale_w.min(scale_h);
    let fitted_w = src_w * scale / 1000;
    let fitted_h = src_h * scale / 1000;
    RectPx::new(
        dst.x + dst.w / 2 - (src_w * scale) / 2000,
        dst.y + dst.h / 2 - (src_h * scale) / 2000,
        fitted_w,
        fitted_h,
    )
}

pub(super) fn build_preview_surface_instance(
    dst: RectPx,
    preview_width: u32,
    preview_height: u32,
) -> Option<SpriteInstance> {
    let fitted = aspect_fit_rect(dst, preview_width, preview_height);
    if fitted.w <= 0 || fitted.h <= 0 {
        return None;
    }

    Some(SpriteInstance {
        position: [fitted.x as f32, fitted.y as f32],
        size: [fitted.w as f32, fitted.h as f32],
        uv_origin: [0.0, 0.0],
        uv_size: [1.0, 1.0],
        depth: SHELL_PREVIEW_SURFACE_DEPTH,
        tint: [1.0, 1.0, 1.0],
        alpha: 1.0,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_briefing_box_hugs_its_lines_in_the_pictures_middle_and_cuts_what_does_not_fit() {
        let font = crate::render::bit_font::tests::make_test_font(&[(u16::from(b'x'), 6)], 3);
        let area = RectPx::new(12, 12, 776, 576);
        let (inset, padding) = (BRIEFING_INSET as i32, BRIEFING_PADDING as i32);

        // The picture fills the area as without a briefing; the box over its
        // middle is as wide as the widest line.
        let short = zoom_layout(area, (400, 100), Some("x\nxx\nx"), &font, 1.0);
        assert_eq!(short.picture, aspect_fit_rect(area, 400, 100));
        let briefing = short.briefing.expect("a briefing");
        let (picture, frame) = (short.picture, briefing.frame);
        assert_eq!(briefing.lines, ["x", "xx", "x"]);
        assert_eq!(frame.w, font.text_width("xx") as i32 + 2 * padding);
        assert_eq!(
            (frame.x, frame.y),
            (
                picture.x + (picture.w - frame.w) / 2,
                picture.y + (picture.h - frame.h) / 2
            )
        );

        // Forty lines over a low picture: cut to it, the last line shown "...".
        let long = zoom_layout(area, (400, 100), Some(&["x"; 40].join("\n")), &font, 1.0);
        let briefing = long.briefing.expect("a briefing");
        assert!(briefing.lines.len() < 40);
        assert_eq!(
            briefing.lines.last().map(String::as_str),
            Some(BRIEFING_CUT)
        );
        assert!(briefing.frame.y >= long.picture.y + inset);
        assert!(briefing.frame.y + briefing.frame.h <= long.picture.y + long.picture.h - inset);
    }

    #[test]
    fn received_preview_upload_respects_device_edges_and_exact_rgba_shape() {
        assert!(preview_fits_texture(1024, 1, 4096, 1024));
        assert!(!preview_fits_texture(1024, 1, 4096, 1023));
        assert!(!preview_fits_texture(1, 1024, 4096, 1023));
        assert!(!preview_fits_texture(1024, 1, 4095, 1024));
        assert!(!preview_fits_texture(0, 1, 0, 1024));
    }

    #[test]
    fn hd_picture_of_the_preview_place_fits_the_preview_rectangle() {
        // A 160 x 81 preview's place drawn at most 576 wide is 576 x 292,
        // which fitted by its own size ends a pixel lower than the preview.
        let preview_box = RectPx::new(0, 0, 144, 112);
        assert!(same_shape((160, 81), (576, 292)));
        assert_ne!(
            aspect_fit_rect(preview_box, 576, 292),
            aspect_fit_rect(preview_box, 160, 81)
        );
        // A picture of [Header]'s square rectangle found no place.
        assert!(!same_shape((160, 81), (448, 448)));
    }

    #[test]
    fn verified_thirty_start_header_projects_every_marker_without_integer_wrap() {
        let mut text =
            "[Header]\nStartX=0\nStartY=0\nWidth=100\nHeight=100\nNumberStartingPoints=30\n"
                .to_owned();
        for index in 1..=30 {
            text.push_str(&format!("Waypoint{index}={index},{index}\n"));
        }
        let ini = IniFile::from_str(&text);
        let mut bounds =
            crate::map::scenario_menu::preview_source_bounds_from_verified_source(&ini).unwrap();
        let positions = project_preview_start_positions(&bounds, RectPx::new(10, 20, 200, 100));
        assert_eq!(
            positions.len(),
            30,
            "VERA's admitted start slots were truncated to the native eight-marker prefix"
        );
        assert!(positions.last().unwrap().0 > positions[7].0);
        bounds.origin_x = i32::MIN;
        bounds.width = 1;
        bounds.start_points.truncate(1);
        bounds.start_points[0].x = i32::MAX;
        assert!(
            project_preview_start_positions(&bounds, RectPx::new(10, 20, 200, 100)).is_empty(),
            "unrepresentable untrusted Header coordinates must not wrap into the preview"
        );
    }

    #[test]
    fn start_marker_picture_puts_art_and_number_at_the_shell_offsets() {
        let mut rgba = [10, 20, 30, 255].repeat(18 * 18);
        rgba[3] = 0;
        let art = crate::render::chrome_atlas::ChromePiece {
            label: "startbut".to_owned(),
            width: 18,
            height: 18,
            rgba,
        };
        let font = crate::assets::fnt_file::FntFile::solid_for_test(2);
        let picture = start_marker_picture(&art, &font, "10").unwrap();
        assert_eq!(
            (picture.width, picture.height, picture.origin),
            (18, 18, (9, 6))
        );
        let pixel = |x: usize, y: usize| &picture.rgba[(y * 18 + x) * 4..][..4];
        assert_eq!(pixel(0, 0), [0, 0, 0, 0], "clear art pixels stay clear");
        assert_eq!(pixel(0, 1), [10, 20, 30, 255]);
        // The number starts at anchor (-2, -6); its second glyph follows one
        // spacing pixel after the first.
        assert_eq!(pixel(7, 0), [255, 255, 0, 255]);
        assert_eq!(pixel(9, 0), [10, 20, 30, 255]);
        assert_eq!(pixel(10, 0), [255, 255, 0, 255]);
        assert_eq!(pixel(7, 1), [10, 20, 30, 255]);
    }
}
