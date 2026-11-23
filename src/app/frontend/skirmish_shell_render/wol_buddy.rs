//! `0x116`'s children on the shared control owners: the column headers,
//! the owner-draw buddy list `0x689` with its cells (`0x4A8` at the
//! `0x0084A1C4` stops), Refresh's picture, the observer checkbox and image,
//! and the selected buddy's heading and statistics. The WOL page
//! compositor draws them, and the group box, over its own backdrop and
//! chrome.

use super::SHELL_CONTROL_DEPTH;
use super::chrome::push_flag_entry_clipped_centered;
use super::gui_paint::{CellRow, GuiStyle, child_ids_except, paint_gui_ids};
use super::wol_lobby::{WolLobbyPaint, row_rgb};
use crate::app::AppState;
use crate::render::main_menu_shell_chrome::WolPageArt;
use crate::render::shell_paint;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::wol_buddy::*;

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
    let scale = layout.page.scale;
    // The captions, heading, statistics and observer box are widgets.
    let controller = &state.frontend.shell_controller;
    let gui = controller.gui();
    let root = controller.root_in(DIALOG);
    if let Some(root) = root {
        // The observer image paints as its art below, not as a box, and the
        // list after it.
        let ids = child_ids_except(gui, root, &[OBSERVER_IMAGE, LIST]);
        let widgets = paint_gui_ids(state, gui, root, ids, scale, GuiStyle::default());
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

    // The list over its widget, each row's cells the page's: only the rows
    // the list shows are filled.
    if let Some(root) = root {
        let list = node(LIST);
        let top = list.and_then(|list| gui.list_view(list)).map_or(0, |view| view.top);
        let shown = list
            .and_then(|list| gui.list_geometry(list))
            .map_or(0, |geometry| geometry.visible_rows);
        let cells_of = |id: i32| {
            (id == i32::from(LIST)).then(|| {
                let rows = ui.rows.iter().enumerate();
                rows.map(|(index, row)| match (top..top + shown).contains(&index) {
                    true => buddy_cells(row),
                    false => CellRow::default(),
                })
                .collect()
            })
        };
        let style = GuiStyle {
            cells_of: Some(&cells_of),
            art,
            control_lists: true,
            ..Default::default()
        };
        let list = paint_gui_ids(state, gui, root, [LIST], scale, style);
        paint.controls.extend(list.controls);
        paint.texts.extend(list.texts);
        paint.icons.extend(list.icons);
    }
    paint
}

/// A buddy row's cells (`0x4A8` at the `0x0084A1C4` stops): name and
/// status, and for one in a game its ping and its type, key and limit
/// pictures (on the page art, when it is loaded).
fn buddy_cells(row: &WolBuddyRow) -> CellRow {
    let mut cells = CellRow::default();
    let rgb = row_rgb(row.color);
    cells.text(NAME_X, NAME_W, &row.name, ShellAlign::V_CENTER, rgb);
    cells.text(STATUS_X, STATUS_W, &row.status, ShellAlign::V_CENTER, rgb);
    let Some(game) = &row.game else {
        return cells;
    };
    cells.text(PING_X, 0, &game.ping.to_string(), ShellAlign::V_CENTER, rgb);
    if let Some(file) = game.kind.file() {
        cells.icons.push((TYPE_X, file));
    }
    if game.keyed {
        cells.icons.push((PASSWORD_X, "wolpriv.pcx"));
    }
    if let Some(file) = game.limit_icon {
        cells.icons.push((LIMIT_X, file));
    }
    cells
}
