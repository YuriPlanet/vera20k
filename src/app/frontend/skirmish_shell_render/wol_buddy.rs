//! `0x116`'s children on the shared control owners: the column headers,
//! the owner-draw buddy list `0x689` with its cells (`0x4A8` at the
//! `0x0084A1C4` stops), Refresh's picture, the observer checkbox and image,
//! and the selected buddy's heading and statistics. The WOL page
//! compositor draws them, and the group box, over its own backdrop and
//! chrome.

use super::chrome::push_flag_entry_clipped_centered;
use super::controls::list_view_controls;
use super::text::rect_to_text_rect;
use super::wol_lobby::{WolLobbyPaint, push_icon, row_rgb};
use super::{SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH};
use crate::app::AppState;
use crate::render::main_menu_shell_chrome::WolPageArt;
use crate::render::shell_paint;
use crate::render::shell_text;
use crate::ui::shell::geom::{RectPx, ShellAlign};
use crate::ui::wol_buddy::*;
use vera20k_gui::shell::list::{ListMetrics, ShellListGeometry};

pub(crate) fn paint_wol_buddy_children(
    state: &AppState,
    ui: &WolBuddyPage,
    layout: &WolBuddyLayout,
    art: Option<&WolPageArt>,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() else {
        return paint;
    };
    let font = &state.renderer.bit_font;
    let scale = layout.page.scale;
    let text = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3]| {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            rgb,
            align,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            scale.factor(),
        )
    };
    // A cell `x` base columns into a row, `w` wide (to the row's end when 0).
    let cell = |row: RectPx, x: i32, w: i32| {
        scale.derive_rect(row, |row| {
            let w = if w > 0 { w } else { row.w - x - 2 };
            RectPx::new(row.x + x, row.y, w, row.h)
        })
    };
    // The captions, heading, statistics and observer box are widgets.
    let controller = &state.frontend.shell_controller;
    let gui = controller.gui();
    let root = controller.root_in(DIALOG);
    if let Some(root) = root {
        // The observer image paints as its art below, not as a box.
        let ids = super::gui_paint::child_ids_except(gui, root, &[OBSERVER_IMAGE]);
        let style = super::gui_paint::GuiStyle::default();
        let widgets = super::gui_paint::paint_gui_ids(state, gui, root, ids, scale, style);
        paint.controls.extend(widgets.controls);
        paint.texts.extend(widgets.texts);
    }
    let node = |id: u16| root.and_then(|root| gui.find_id(root, id.into()));

    // Refresh's picture: released, pressed, or disabled (the released
    // face; the owner-draw button greys nothing of a picture).
    if let Some(art) = art {
        let face = if node(REFRESH).is_some_and(|refresh| gui.shows_pressed(refresh)) {
            REFRESH_FACES.1
        } else {
            REFRESH_FACES.0
        };
        if let Some((_, entry)) = art.icons.iter().find(|(name, _)| name == face) {
            let (w, h) = entry.native_size();
            let base = scale.base_rect(layout.rect_of(REFRESH).unwrap_or_default());
            let [x, y] = scale.scale_point([base.x + (base.w - w) / 2, base.y + (base.h - h) / 2]);
            shell_paint::push_entry_grown(
                &mut paint.icons,
                *entry,
                x,
                y,
                scale,
                SHELL_CONTROL_DEPTH - 0.00003,
            );
        }
    }
    if let Some(entry) = atlas.lan_observer_toggle(ui.observer) {
        push_flag_entry_clipped_centered(
            &mut paint.controls,
            entry,
            layout.rect_of(OBSERVER_IMAGE).unwrap_or_default(),
            scale,
            SHELL_CONTROL_DEPTH,
        );
    }

    // The list widget's top and pushed arrow.
    let view = node(LIST).and_then(|list| gui.list_view(list));
    let (top, pressed) = view.map_or((0, None), |view| (view.top, view.pressed));
    let geometry = ShellListGeometry::new(
        layout.rect_of(LIST).unwrap_or_default(),
        ui.rows.len(),
        top,
        ListMetrics::scaled(scale),
    );
    list_view_controls(
        &mut paint.controls,
        atlas,
        geometry,
        (ui.selected, top, pressed),
        scale,
    );
    for (visible, row) in ui
        .rows
        .iter()
        .skip(top)
        .take(geometry.visible_rows)
        .enumerate()
    {
        let rect = geometry.row(visible);
        let rgb = row_rgb(row.color);
        paint.texts.push(text(
            &row.name,
            cell(rect, NAME_X, NAME_W),
            ShellAlign::V_CENTER,
            rgb,
        ));
        paint.texts.push(text(
            &row.status,
            cell(rect, STATUS_X, STATUS_W),
            ShellAlign::V_CENTER,
            rgb,
        ));
        let Some(game) = &row.game else {
            continue;
        };
        paint.texts.push(text(
            &game.ping.to_string(),
            cell(rect, PING_X, 0),
            ShellAlign::V_CENTER,
            rgb,
        ));
        if let Some(art) = art {
            if let Some(file) = game.kind.file() {
                push_icon(&mut paint.icons, art, file, rect, TYPE_X, scale);
            }
            if game.keyed {
                push_icon(
                    &mut paint.icons,
                    art,
                    "wolpriv.pcx",
                    rect,
                    PASSWORD_X,
                    scale,
                );
            }
            if let Some(file) = game.limit_icon {
                push_icon(&mut paint.icons, art, file, rect, LIMIT_X, scale);
            }
        }
    }
    paint
}
