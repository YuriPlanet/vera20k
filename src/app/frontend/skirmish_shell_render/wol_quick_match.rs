//! `0x117`'s left-side children on the shared control owners: the "Match
//! Status" caption and the status transcript `0x53F` (an owner-draw list,
//! `0x50014151`). The WOL page compositor draws them over its own art and
//! chrome.

use super::controls::list_view_controls;
use super::text::rect_to_text_rect;
use super::wol_lobby::{WolLobbyPaint, row_rgb};
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::shell_text;
use crate::ui::shell::geom::{RectPx, ShellAlign};
use crate::ui::wol_quick_match::{
    CAPTION, DIALOG, MATCH_STATUS_KEY, TRANSCRIPT, WolQuickMatchLayout, WolQuickMatchPage,
};
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

pub(crate) fn paint_wol_quick_match_children(
    state: &AppState,
    ui: &WolQuickMatchPage,
    layout: &WolQuickMatchLayout,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let font = &state.renderer.bit_font;
    let scale = layout.page.scale;
    let text = |value: &str, rect: RectPx, rgb: [f32; 3]| {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            rgb,
            ShellAlign::V_CENTER,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            scale.factor(),
        )
    };
    paint.texts.push(text(
        &state.csf_text(MATCH_STATUS_KEY),
        layout.rect_of(CAPTION).unwrap_or_default(),
        SHELL_LABEL_TEXT_RGB,
    ));
    // The list widget's top and pushed arrow.
    let controller = &state.frontend.shell_controller;
    let gui = controller.gui();
    let view = controller
        .root_in(DIALOG)
        .and_then(|root| gui.find_id(root, TRANSCRIPT.into()))
        .and_then(|list| gui.list_view(list));
    let (top, pressed) = view.map_or((0, None), |view| (view.top, view.pressed));
    let geometry = ShellListGeometry::new(
        layout.rect_of(TRANSCRIPT).unwrap_or_default(),
        ui.lines.len(),
        top,
        ListMetrics::scaled(scale),
    );
    list_view_controls(
        &mut paint.controls,
        atlas,
        geometry,
        (None, top, pressed),
        scale,
    );
    for (visible, line) in ui
        .lines
        .iter()
        .skip(top)
        .take(geometry.visible_rows)
        .enumerate()
    {
        let row = scale.derive_rect(geometry.row(visible), |row| {
            RectPx::new(row.x + 2, row.y, row.w - 4, row.h)
        });
        paint.texts.push(text(
            &String::from_utf16_lossy(&line.text),
            row,
            row_rgb(line.color),
        ));
    }
    paint
}
