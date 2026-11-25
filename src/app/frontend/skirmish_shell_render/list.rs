//! The owner-draw list `0x00618D40` (paint `0x00619230`) and its scrollbar
//! child (`0x0061C690`) on the Skirmish-family atlas.
use super::*;
use crate::render::skirmish_shell_chrome::SkirmishShellChromeEntry;
use crate::ui::shell::geom::ShellScale;
use vera20k_gui::shell::list::{
    ListFrameTone, ListScrollPart, ShellListGeometry, list_frame_lines, scrollbar_edge_lines,
    scrollbar_interior,
};

fn tone_rgb(tone: ListFrameTone) -> [f32; 3] {
    tone.rgb().map(|channel| f32::from(channel) / 255.0)
}

/// The list's two frame rings around `window`, for a list and for a box
/// that sits against one.
pub(crate) fn paint_list_frame(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    window: RectPx,
) {
    paint_list_frame_px(out, atlas.white_pixel, window);
}

/// [`paint_list_frame`] with the atlas's white pixel.
pub(crate) fn paint_list_frame_px(
    out: &mut Vec<SpriteInstance>,
    white_pixel: Option<SkirmishShellChromeEntry>,
    window: RectPx,
) {
    for (line, tone) in list_frame_lines(window) {
        push_solid_rect_px(
            out,
            white_pixel,
            line,
            tone_rgb(tone),
            SHELL_DROPDOWN_DEPTH - 0.00012,
        );
    }
}

/// A list over its dialog's background: the selected row's fill, the two
/// frame rings around the window (the paint surface less its extra column
/// and row) and, when the rows overflow, the scrollbar child. The native
/// list also darkens the background under its rows by one RGB565 unit; that
/// is not drawn here. `solid_fill` backs the rows and the scrollbar's track
/// with the panel color, for a list that no art backs.
pub(crate) fn paint_list(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    geometry: ShellListGeometry,
    top: usize,
    selected: Option<usize>,
    pressed_part: Option<ListScrollPart>,
    solid_fill: bool,
    scale: ShellScale,
) {
    let depth = SHELL_DROPDOWN_DEPTH - 0.00010;
    if solid_fill {
        push_solid_rect(out, atlas, geometry.content, SHELL_MODAL_PANEL_RGB, depth);
        if let Some(bar) = geometry.scrollbar {
            let track = scrollbar_interior(bar);
            push_solid_rect(out, atlas, track, SHELL_MODAL_PANEL_RGB, depth);
        }
    }
    if let Some(selected) = selected.filter(|i| *i >= top && *i < top + geometry.visible_rows) {
        push_solid_rect(
            out,
            atlas,
            geometry.row(selected - top),
            OWNERDRAW_SELECTED_RGB_FROM_DAT_00AC4604_PACKED_000000FF,
            depth - 0.00001,
        );
    }
    let outer = geometry.outer;
    let window = RectPx::new(outer.x, outer.y, outer.w - 1, outer.h - 1);
    paint_list_frame(out, atlas, window);
    let (Some(bar), Some(thumb)) = (geometry.scrollbar, geometry.thumb) else {
        return;
    };
    for (line, tone) in scrollbar_edge_lines(bar) {
        // The corner pixels go over the lines they cross.
        let line_depth = if tone == ListFrameTone::Corner {
            depth - 0.000025
        } else {
            depth - 0.00002
        };
        push_solid_rect(out, atlas, line, tone_rgb(tone), line_depth);
    }
    crate::render::shell_paint::push_list_scroll_art(
        out,
        &atlas.control_chrome().list_scroll(),
        (bar, thumb, geometry.metrics.arrow_h),
        pressed_part,
        scale,
        (depth - 0.00003, depth - 0.00004),
    );
}
