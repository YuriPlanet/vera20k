//! Chrome and primitive sprite helpers for the skirmish shell renderer.
//!
//! These helpers build shell chrome, owner-draw button art, bevels, and
//! low-level sprite rectangles without changing render behavior.

use crate::app::frontend::shell_transition::{ColumnDraw, PanelArt};
use crate::render::batch::SpriteInstance;
use crate::render::shell_paint::{push_column_frames, push_panel_art, push_right_panel_column};
pub(super) use crate::render::shell_paint::{
    push_entry, push_entry_fitted, push_entry_grown, push_entry_native, push_entry_sized,
    push_entry_sized_alpha, push_entry_top_clipped, push_entry_wrapped_centered,
    push_flag_entry_clipped_centered, push_tinted_entry,
};
use crate::render::skirmish_shell_chrome::{SkirmishShellChromeAtlas, SkirmishShellChromeEntry};
use crate::ui::shell::geom::{SDBTNANM_CELL_H, SDBTNANM_CELL_W_NARROW};
use crate::ui::shell::slide::panel_art_origin;
use crate::ui::skirmish_shell::{RectPx, SkirmishShellLayout, SkirmishShellState};

use super::draw_order::{LowerStripRole, ShellDialogChromeProfile, lower_strip_role};
use super::{
    BUTTON_DISABLED_ALPHA, OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
    OWNERDRAW_BEVEL_LIGHT_RGB_FROM_PACKED_00C5BEA7, PRESSED_BUTTON_CONTENT_OFFSET_Y,
    SHELL_LOWER_STRIP_DEPTH,
};

const RIGHT_PANEL_TOP_DEPTH: f32 = 0.00080;
const RIGHT_PANEL_TILE_DEPTH: f32 = 0.00079;
const RIGHT_PANEL_OVERLAY_DEPTH: f32 = 0.000785;
const RIGHT_PANEL_BOTTOM_DEPTH: f32 = 0.00078;

/// Native612B70 type3: MNBTTN0 released,1 held,2 timer highlight.
/// Disabled ordinary controls retain released art; text has a separate gate.
pub(crate) fn type3_button_frames(
    atlas: &SkirmishShellChromeAtlas,
) -> crate::render::shell_paint::ModalButtonFrames {
    let frames = [
        atlas.modal_button_mnbttn_frame0,
        atlas.modal_button_mnbttn_frame1,
        atlas.modal_button_mnbttn_frame2,
    ];
    let index = vera20k_gui::shell::button::owner_button_frame;
    crate::render::shell_paint::ModalButtonFrames {
        up: frames[index(false, false)],
        disabled: frames[index(false, false)],
        pressed: frames[index(true, false)],
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ButtonPiece {
    Left,
    Middle,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ButtonSegment {
    pub(super) piece: ButtonPiece,
    pub(super) x: f32,
    pub(super) width: f32,
    pub(super) uv_x_ratio: f32,
    pub(super) uv_width_ratio: f32,
}
#[cfg(test)]
pub(super) fn button_piece_asset_names(
    pressed: bool,
) -> (&'static str, &'static str, &'static str) {
    if pressed {
        ("bde_li30.pcx", "bde_mi30.pcx", "bde_ri30.pcx")
    } else {
        ("bue_li30.pcx", "bue_mi30.pcx", "bue_ri30.pcx")
    }
}

pub(super) fn build_button_segments(
    rect: RectPx,
    left_w: f32,
    mid_w: f32,
    right_w: f32,
) -> Vec<ButtonSegment> {
    if rect.w <= 0 {
        return Vec::new();
    }
    let rect_w = rect.w as f32;
    let left_w = left_w.round().max(1.0).min(rect_w);
    let right_w = right_w.round().max(1.0).min(rect_w);
    let mid_w = mid_w.round().max(1.0);
    let mut segments = vec![ButtonSegment {
        piece: ButtonPiece::Left,
        x: rect.x as f32,
        width: left_w,
        uv_x_ratio: 0.0,
        uv_width_ratio: 1.0,
    }];

    let middle_start = rect.x as f32 + left_w;
    let middle_dest_w = (rect_w - right_w).max(0.0);
    let middle_phase = ((mid_w - middle_dest_w) / 2.0).max(0.0);
    let mut covered = 0.0;
    while covered < middle_dest_w - f32::EPSILON {
        let source_x = (middle_phase + covered) % mid_w;
        let width = (middle_dest_w - covered).min(mid_w - source_x);
        segments.push(ButtonSegment {
            piece: ButtonPiece::Middle,
            x: middle_start + covered,
            width,
            uv_x_ratio: source_x / mid_w,
            uv_width_ratio: width / mid_w,
        });
        covered += width;
    }

    segments.push(ButtonSegment {
        piece: ButtonPiece::Right,
        x: rect.x as f32 + rect_w - right_w,
        width: right_w,
        uv_x_ratio: 0.0,
        uv_width_ratio: 1.0,
    });
    segments
}

pub(super) fn button_segment_sprite_size(
    entry: SkirmishShellChromeEntry,
    segment: ButtonSegment,
) -> [f32; 2] {
    [segment.width, entry.pixel_size[1]]
}

pub(super) fn button_art_y(rect: RectPx, art_h: f32, pressed: bool, disabled: bool) -> f32 {
    let art_h = art_h.round() as i32;
    let pressed_offset = if pressed && !disabled {
        PRESSED_BUTTON_CONTENT_OFFSET_Y
    } else {
        0
    };
    (rect.y + (rect.h - art_h) / 2 + pressed_offset) as f32
}

pub(super) fn button_entries(
    atlas: &SkirmishShellChromeAtlas,
    pressed: bool,
) -> Option<(
    SkirmishShellChromeEntry,
    SkirmishShellChromeEntry,
    SkirmishShellChromeEntry,
)> {
    if pressed {
        Some((
            atlas.button_down_left_30?,
            atlas.button_down_mid_30?,
            atlas.button_down_right_30?,
        ))
    } else {
        Some((
            atlas.button_up_left_30?,
            atlas.button_up_mid_30?,
            atlas.button_up_right_30?,
        ))
    }
}

pub(super) fn push_button_30(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    rect: RectPx,
    pressed: bool,
    disabled: bool,
    depth: f32,
) {
    let Some((left, mid, right)) = button_entries(atlas, pressed && !disabled) else {
        return;
    };
    let alpha = if disabled { BUTTON_DISABLED_ALPHA } else { 1.0 };
    for segment in build_button_segments(
        rect,
        left.pixel_size[0],
        mid.pixel_size[0],
        right.pixel_size[0],
    ) {
        let mut entry = match segment.piece {
            ButtonPiece::Left => left,
            ButtonPiece::Middle => mid,
            ButtonPiece::Right => right,
        };
        if segment.piece == ButtonPiece::Middle
            && (segment.uv_x_ratio > 0.0 || segment.uv_width_ratio < 1.0)
        {
            entry.uv_origin[0] += entry.uv_size[0] * segment.uv_x_ratio;
            entry.uv_size[0] *= segment.uv_width_ratio;
            entry.pixel_size[0] = segment.width;
        }
        push_entry_sized_alpha(
            out,
            entry,
            segment.x,
            button_art_y(rect, entry.pixel_size[1], pressed, disabled),
            button_segment_sprite_size(entry, segment),
            depth,
            alpha,
        );
    }
}

/// Returns whether the selected frame uses the standard SDBTNANM canvas.
pub(super) fn push_right_panel_button_shp(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    rect: RectPx,
    visual: impl Into<vera20k_gui::shell::button::ButtonVisual>,
    disabled: bool,
    depth: f32,
) -> bool {
    let visual = visual.into();
    let pressed = visual.pressed;
    // Skirmish shell setup classifies Start/Choose/Back as owner-draw type 1.
    // Type 0 PCX pieces are only a missing-asset fallback for this path.
    let entry = match right_panel_button_sdbtnanm_frame_index(visual, disabled) {
        4 => atlas.right_panel_button_sdbtnanm_frame4,
        3 => atlas.right_panel_button_sdbtnanm_frame3,
        _ => atlas.right_panel_button_sdbtnanm_frame2,
    };
    if let Some(entry) = entry {
        // A disabled column button keeps its art: the disabled black blend
        // runs only for owner-draw type 0 (`0x006135F3`, record `+0xB0`).
        push_entry_sized_alpha(
            out,
            entry,
            rect.x as f32,
            rect.y as f32,
            [rect.w as f32, rect.h as f32],
            depth,
            1.0,
        );
        entry.pixel_size == [SDBTNANM_CELL_W_NARROW as f32, SDBTNANM_CELL_H as f32]
    } else {
        push_button_30(out, atlas, rect, pressed, disabled, depth);
        false
    }
}

pub(super) fn right_panel_button_sdbtnanm_frame_index(
    visual: impl Into<vera20k_gui::shell::button::ButtonVisual>,
    disabled: bool,
) -> usize {
    let visual = visual.into();
    if visual.pressed && !disabled {
        4
    } else if visual.highlighted {
        3
    } else {
        2
    }
}

pub(super) fn push_solid_rect_px(
    out: &mut Vec<SpriteInstance>,
    white_pixel: Option<SkirmishShellChromeEntry>,
    rect: RectPx,
    tint: [f32; 3],
    depth: f32,
) {
    let Some(pixel) = white_pixel else {
        return;
    };
    push_tinted_entry(out, pixel, rect, tint, depth);
}

pub(super) fn push_solid_rect(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    rect: RectPx,
    tint: [f32; 3],
    depth: f32,
) {
    push_solid_rect_px(out, atlas.white_pixel, rect, tint, depth);
}

pub(super) fn push_rect_outline(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    rect: RectPx,
    tint: [f32; 3],
    depth: f32,
) {
    push_rect_outline_px(out, atlas.white_pixel, rect, tint, depth);
}

/// [`push_rect_outline`] from the white pixel alone: top, bottom, left,
/// right.
pub(super) fn push_rect_outline_px(
    out: &mut Vec<SpriteInstance>,
    white_pixel: Option<SkirmishShellChromeEntry>,
    rect: RectPx,
    tint: [f32; 3],
    depth: f32,
) {
    for edge in vera20k_gui::shell::bar::outline_edges(rect).into_iter().flatten() {
        push_solid_rect_px(out, white_pixel, edge, tint, depth);
    }
}

pub(super) fn push_bevel_ring_px(
    out: &mut Vec<SpriteInstance>,
    white_pixel: Option<SkirmishShellChromeEntry>,
    rect: RectPx,
    top_left_tint: [f32; 3],
    bottom_right_tint: [f32; 3],
    depth: f32,
) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    push_solid_rect_px(
        out,
        white_pixel,
        RectPx::new(rect.x, rect.y, rect.w, 1),
        top_left_tint,
        depth,
    );
    if rect.h > 1 {
        push_solid_rect_px(
            out,
            white_pixel,
            RectPx::new(rect.x, rect.y + 1, 1, rect.h - 1),
            top_left_tint,
            depth,
        );
    }
    push_solid_rect_px(
        out,
        white_pixel,
        RectPx::new(rect.x, rect.y + rect.h - 1, rect.w, 1),
        bottom_right_tint,
        depth,
    );
    if rect.w > 1 && rect.h > 2 {
        push_solid_rect_px(
            out,
            white_pixel,
            RectPx::new(rect.x + rect.w - 1, rect.y + 1, 1, rect.h - 2),
            bottom_right_tint,
            depth,
        );
    }
}

pub(super) fn push_ownerdraw_two_pixel_bevel_frame_px(
    out: &mut Vec<SpriteInstance>,
    white_pixel: Option<SkirmishShellChromeEntry>,
    rect: RectPx,
    depth: f32,
) {
    push_bevel_ring_px(
        out,
        white_pixel,
        rect,
        OWNERDRAW_BEVEL_LIGHT_RGB_FROM_PACKED_00C5BEA7,
        OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
        depth,
    );
    if rect.w > 2 && rect.h > 2 {
        push_bevel_ring_px(
            out,
            white_pixel,
            RectPx::new(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2),
            OWNERDRAW_BEVEL_DARK_RGB_FROM_PACKED_00807A68,
            OWNERDRAW_BEVEL_LIGHT_RGB_FROM_PACKED_00C5BEA7,
            depth - 0.00001,
        );
    }
}

pub(super) fn push_ownerdraw_two_pixel_bevel_frame(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    rect: RectPx,
    depth: f32,
) {
    push_ownerdraw_two_pixel_bevel_frame_px(out, atlas.white_pixel, rect, depth);
}

pub(super) fn lower_strip_entry(
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) -> Option<SkirmishShellChromeEntry> {
    match lower_strip_role(layout) {
        LowerStripRole::Lwscrns640 => atlas.lower_side_640_lwscrns,
        LowerStripRole::LwscrnlLarge => atlas.lower_side_large_lwscrnl,
    }
}

pub(super) fn common_shell_origin(layout: &SkirmishShellLayout) -> (i32, i32) {
    layout.shell_origin()
}

pub(super) fn lower_strip_rect(
    layout: &SkirmishShellLayout,
    entry: SkirmishShellChromeEntry,
) -> RectPx {
    let [w, h] = layout
        .scale
        .art_size(entry.pixel_size[0], entry.pixel_size[1]);
    let (w, h) = (w.round() as i32, h.round() as i32);
    let (origin_x, origin_y) = layout.shell_origin();
    RectPx::new(origin_x, origin_y + layout.shell_height() - h, w, h)
}

pub(super) fn sdmpbtn_rect(
    layout: &SkirmishShellLayout,
    entry: SkirmishShellChromeEntry,
) -> RectPx {
    let (scale_x, scale_y) = layout.right_panel.stretch();
    let w = (entry.pixel_size[0] * scale_x).round() as i32;
    let h = (entry.pixel_size[1] * scale_y).round().max(1.0) as i32;
    let (x, y) = panel_art_origin(layout.right_panel, PanelArt::MapButton(0), w, h);
    RectPx::new(x, y, w, h)
}

pub(super) fn push_right_panel_base_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
    overlay_frame10_active: bool,
) {
    let column = [
        atlas.right_panel_top_sdtp,
        atlas.right_panel_tile_sdbtnbkgd,
        atlas
            .right_panel_overlay_sdbtnanm_frame10
            .filter(|_| overlay_frame10_active),
        atlas.right_panel_bottom_sdbtm,
    ];
    let depths = [
        RIGHT_PANEL_TOP_DEPTH,
        RIGHT_PANEL_TILE_DEPTH,
        RIGHT_PANEL_OVERLAY_DEPTH,
        RIGHT_PANEL_BOTTOM_DEPTH,
    ];
    push_right_panel_column(out, layout.right_panel, column, depths);
}

pub(super) fn push_lower_strip_instance(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
) {
    if let Some(lower_strip) = lower_strip_entry(atlas, layout) {
        push_entry(
            out,
            lower_strip,
            lower_strip_rect(layout, lower_strip),
            SHELL_LOWER_STRIP_DEPTH,
        );
    }
}

pub(super) fn push_steady_optional_chrome_instances(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
    profile: ShellDialogChromeProfile,
) {
    if profile.draws_top_highlight()
        && let Some(top_highlight) = atlas.right_panel_top_highlight_sdtp_frame1
    {
        push_entry(
            out,
            top_highlight,
            layout.right_panel.top,
            SHELL_LOWER_STRIP_DEPTH - 0.00001,
        );
    }
    if profile.draws_map_button()
        && let Some(sdmpbtn) = atlas.sd_map_button
    {
        push_entry(
            out,
            sdmpbtn,
            sdmpbtn_rect(layout, sdmpbtn),
            SHELL_LOWER_STRIP_DEPTH - 0.00002,
        );
    }
}

/// The slide engine's top-panel art of one tick (`0x006071E0`) in its draw
/// order (later on top), in place of the steady top highlight and map button:
/// SDTP and the warning display at the panel top, the map button where it
/// steadily sits.
pub(super) fn push_slide_panel_art(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
    art: &[PanelArt],
) {
    let entry = |art| match art {
        PanelArt::Sdtp(0) => atlas.right_panel_top_sdtp,
        PanelArt::Sdtp(_) => atlas.right_panel_top_highlight_sdtp_frame1,
        PanelArt::Warning(frame) => atlas.sd_warning_frames.get(frame).copied().flatten(),
        PanelArt::MapButton(frame) => atlas.sd_map_button_frames.get(frame).copied().flatten(),
        // Campaign score art is composited by its own renderer.
        PanelArt::Scrt(_) | PanelArt::ScrBackdrop => None,
    };
    let depth = SHELL_LOWER_STRIP_DEPTH - 0.00001;
    push_panel_art(out, layout.right_panel, art, entry, depth);
}

/// The slide engine's SDBTNANM draws of one tick: each frame right-aligned
/// over its tile row, in draw order (later on top), in place of the buttons.
pub(super) fn push_slide_column(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    layout: &SkirmishShellLayout,
    draws: &[ColumnDraw],
    depth: f32,
) {
    let frames = &atlas.right_panel_button_sdbtnanm_frames;
    push_column_frames(out, layout.right_panel, frames, draws, depth);
}

pub(super) fn right_panel_frame10_overlay_active(_shell: &SkirmishShellState) -> bool {
    // Verified standard offline Skirmish first paint leaves the dialog gate at
    // zero, and that gate makes RightPanel__Draw skip the frame-10 overlay.
    false
}
