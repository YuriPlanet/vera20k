//! The owner-draw list `0x00618D40` (paint `0x00619230`) and its scrollbar
//! child (`0x0061C690`) on the Skirmish-family atlas.
use super::*;
use crate::ui::shell::list::{
    ListFrameTone, ListScrollPart, ShellListGeometry, grip_tiles, list_frame_lines,
    scrollbar_arrow_origins, scrollbar_edge_lines, scrollbar_interior,
};

fn tone_rgb(tone: ListFrameTone) -> [f32; 3] {
    tone.rgb().map(|channel| f32::from(channel) / 255.0)
}

/// A list over its dialog's background: the selected row's fill, the two
/// frame rings around the window (the paint surface less its extra column
/// and row) and, when the rows overflow, the scrollbar child. The native
/// list also darkens the background under its rows by one RGB565 unit; that
/// is not drawn here.
pub(super) fn paint_list(
    out: &mut Vec<SpriteInstance>,
    atlas: &SkirmishShellChromeAtlas,
    geometry: ShellListGeometry,
    top: usize,
    selected: Option<usize>,
    pressed_part: Option<ListScrollPart>,
    solid_fill: bool,
) {
    let depth = SHELL_DROPDOWN_DEPTH - 0.00010;
    if solid_fill {
        push_solid_rect(out, atlas, geometry.content, SHELL_MODAL_PANEL_RGB, depth);
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
    for (line, tone) in list_frame_lines(window) {
        push_solid_rect(out, atlas, line, tone_rgb(tone), depth - 0.00002);
    }
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
    let chrome = atlas.control_chrome();
    let x = scrollbar_interior(bar).x;
    let arrow_h = chrome
        .scrollbar_arrow_down_released
        .map_or(22, |entry| entry.pixel_size[1].round() as i32);
    for ((released, pressed, part), (x, y)) in [
        (
            chrome.scrollbar_arrow_up_released,
            chrome.scrollbar_arrow_up_pressed,
            ListScrollPart::Up,
        ),
        (
            chrome.scrollbar_arrow_down_released,
            chrome.scrollbar_arrow_down_pressed,
            ListScrollPart::Down,
        ),
    ]
    .into_iter()
    .zip(scrollbar_arrow_origins(bar, arrow_h))
    {
        if let Some(entry) =
            super::controls::scrollbar_arrow_entry(released, pressed, pressed_part == Some(part))
        {
            push_entry_native(out, entry, x, y, depth - 0.00003);
        }
    }
    if let Some(mid) = chrome.scrollbar_thumb_mid {
        let tile_h = mid.pixel_size[1].round() as i32;
        for (y, h) in grip_tiles(thumb, tile_h) {
            out.push(SpriteInstance {
                position: [x as f32, y as f32],
                size: [mid.pixel_size[0], h as f32],
                uv_origin: mid.uv_origin,
                uv_size: [mid.uv_size[0], mid.uv_size[1] * h as f32 / tile_h as f32],
                depth: depth - 0.00003,
                tint: [1.0, 1.0, 1.0],
                alpha: 1.0,
                ..Default::default()
            });
        }
    }
    if let Some(cap) = chrome.scrollbar_thumb_top {
        push_entry_native(out, cap, x, thumb.y, depth - 0.00004);
    }
    if let Some(cap) = chrome.scrollbar_thumb_bottom {
        let h = cap.pixel_size[1].round() as i32;
        push_entry_native(out, cap, x, thumb.y + thumb.h - h, depth - 0.00004);
    }
}
