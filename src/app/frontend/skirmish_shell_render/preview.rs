//! Preview texture and start-marker sprite helpers for the skirmish shell.
//!
//! Keeps preview decoding, random-map preview cache handling, and live
//! marker sprite projection separate from the shell render orchestration.

use std::path::Path;

use crate::app::AppState;
use crate::app::loading::init::MapMenuEntry;
use crate::assets::asset_manager::AssetManager;
use crate::map::preview::{DecodedPreview, PreviewSourceBounds};
use crate::render::batch::{BatchTexture, SpriteInstance};
use crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas;
use crate::rules::ini_parser::IniFile;
use crate::ui::skirmish_shell::RectPx;

use super::chrome::push_entry_native;
use super::{
    RANDMAP_PREVIEW_FILE_NAME, RANDMAP_SENTINEL_FILE_NAME, SHELL_PREVIEW_SURFACE_DEPTH,
    START_MARKER_OFFSET_X, START_MARKER_OFFSET_Y,
};

pub(crate) struct SkirmishPreviewTexture {
    pub selected_map_idx: usize,
    pub texture: BatchTexture,
    pub width: u32,
    pub height: u32,
    /// Set when the texture holds a random-map setup preview rather than a
    /// chooser map's thumbnail, carrying the generation it was built from.
    pub setup_preview_generation: Option<u32>,
}

pub(super) fn push_start_marker_sprites(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    projected_positions: &[(i32, i32)],
    depth: f32,
) {
    let Some(marker) = atlas.start_marker else {
        return;
    };
    for &(x, y) in projected_positions {
        let (marker_x, marker_y) = start_marker_top_left(x, y);
        push_entry_native(out, marker, marker_x, marker_y, depth);
    }
}

pub(super) fn start_marker_top_left(anchor_x: i32, anchor_y: i32) -> (i32, i32) {
    (
        anchor_x + START_MARKER_OFFSET_X,
        anchor_y + START_MARKER_OFFSET_Y,
    )
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

    bounds
        .start_points
        .iter()
        .take(8)
        .map(|point| {
            let x_per_mille = ((point.x - bounds.origin_x) as i64 * 1000) / bounds.width as i64;
            let y_per_mille = ((point.y - bounds.origin_y) as i64 * 1000) / bounds.height as i64;
            let x = fitted_preview_rect.x
                + ((x_per_mille * fitted_preview_rect.w as i64) / 1000) as i32;
            let y = fitted_preview_rect.y
                + ((y_per_mille * fitted_preview_rect.h as i64) / 1000) as i32;
            (x, y)
        })
        .collect()
}

pub(super) fn build_start_marker_instances(
    atlas: &SkirmishShellChromeAtlas,
    projected_positions: &[(i32, i32)],
) -> Vec<SpriteInstance> {
    let mut instances = Vec::new();
    push_start_marker_sprites(&mut instances, atlas, projected_positions, 0.00056);
    instances
}

pub(super) fn selected_preview_texture_is_current(
    state: &AppState,
    selected_map_idx: usize,
) -> bool {
    state
        .frontend.skirmish_preview_texture
        .as_ref()
        .is_some_and(|cached| {
            // A setup-dialog preview happens to carry the selected index too, so it
            // has to be rejected explicitly or closing the dialog would leave the
            // generated image standing in for the chooser's own thumbnail.
            cached.setup_preview_generation.is_none() && cached.selected_map_idx == selected_map_idx
        })
}

pub(super) fn is_random_map_sentinel_file_name(file_name: &str) -> bool {
    // Map names can carry the game's own `\` separators, so split on both `\`
    // and `/` on every host (`Path::file_name` only knows the host's).
    let name = file_name.rsplit(['\\', '/']).next().unwrap_or(file_name);
    name.eq_ignore_ascii_case(RANDMAP_SENTINEL_FILE_NAME)
}

pub(super) fn is_random_map_sentinel_entry(entry: &MapMenuEntry) -> bool {
    is_random_map_sentinel_file_name(&entry.file_name)
}

pub(super) fn decode_randmap_preview_from_runtime_file()
-> Option<crate::map::preview::DecodedPreview> {
    let config = crate::util::config::GameConfig::load().ok()?;
    let preview_path = config.paths.ra2_dir.join(RANDMAP_PREVIEW_FILE_NAME);
    let bytes = match std::fs::read(&preview_path) {
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
        let ini_path = ra2_dir.join(&entry.file_name);
        if let Some(ini) = crate::app::frontend::list_maps::read_map_ini_for_metadata(&ini_path) {
            if let Some(preview) = decode_preview_from_ini(&ini, &entry.file_name) {
                return Some(preview);
            }
        }
    }

    if let Some(assets) = assets {
        for candidate in crate::app::frontend::list_maps::asset_map_candidates(&entry.file_name) {
            if let Some(bytes) = assets.get_ref(&candidate) {
                if let Some(preview) = decode_preview_from_map_bytes(bytes, &candidate) {
                    return Some(preview);
                }
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
    let Some(modal) = state.frontend.skirmish_shell_state.random_map_setup_modal.as_ref() else {
        return false;
    };
    let Some(preview) = modal.generated_preview.as_ref() else {
        state.frontend.skirmish_preview_texture = None;
        return true;
    };
    let generation = modal.preview_generation;
    let already_current = state
        .frontend.skirmish_preview_texture
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
    state.frontend.skirmish_preview_texture = Some(SkirmishPreviewTexture {
        selected_map_idx: state.frontend.skirmish_shell_state.selected_map_idx,
        texture,
        width: preview.width,
        height: preview.height,
        setup_preview_generation: Some(generation),
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

pub(super) fn ensure_selected_preview_texture(state: &mut AppState) {
    if ensure_random_map_setup_preview_texture(state) {
        return;
    }
    let selected_map_idx = preview_map_idx(state);
    let selected_entry = state.frontend.scenario_catalog.shell_maps().get(selected_map_idx).cloned();
    let selected_is_random_sentinel = selected_entry
        .as_ref()
        .is_some_and(is_random_map_sentinel_entry);
    if !selected_is_random_sentinel && selected_preview_texture_is_current(state, selected_map_idx)
    {
        return;
    }

    let ra2_dir = state
        .platform.game_config
        .as_ref()
        .map(|config| config.paths.ra2_dir.as_path())
        .or_else(|| state.process_assets.manager().map(|assets| assets.ra2_dir()));
    let decoded = selected_entry.as_ref().and_then(|entry| {
        decode_preview_for_map_entry(entry, ra2_dir, state.process_assets.manager())
    });

    let Some(decoded) = decoded.as_ref() else {
        state.frontend.skirmish_preview_texture = None;
        return;
    };

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
    });
}

pub(super) fn should_draw_start_marker_overlays(
    fitted_preview_rect: Option<RectPx>,
    projected_start_positions: &[(i32, i32)],
    preview_has_baked_start_markers: bool,
) -> bool {
    fitted_preview_rect.is_some()
        && !projected_start_positions.is_empty()
        && !preview_has_baked_start_markers
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
