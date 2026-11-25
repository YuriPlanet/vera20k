//! `0x116`'s children on the shared control owners: the column headers,
//! the owner-draw buddy list `0x689` with its cells (`0x4A8` at the
//! `0x0084A1C4` stops), Refresh's picture, the observer checkbox and image,
//! and the selected buddy's heading and statistics. The WOL page
//! compositor draws them, and the group box, over its own backdrop and
//! chrome.

use super::gui_paint::{ButtonArt, CellRow, GuiStyle, Picture, paint_gui_styled};
use super::wol_lobby::{WolLobbyPaint, row_rgb};
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::render::bit_font::BitFont;
use crate::render::hud_paint::Material;
use crate::render::main_menu_shell_chrome::WolPageArt;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::wol_buddy::*;

pub(crate) fn paint_wol_buddy_children(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    ui: &WolBuddyPage,
    layout: &WolBuddyLayout,
    art: Option<&WolPageArt>,
) -> WolLobbyPaint {
    let mut paint = WolLobbyPaint::default();
    if frontend.skirmish_shell_chrome.is_none() {
        return paint;
    }
    let scale = layout.page.scale;
    // The captions, heading, statistics and observer box are widgets; so
    // are Refresh's picture (released, pressed, or disabled: the released
    // face; the owner-draw button greys nothing of a picture) and the
    // observer image's eye.
    let controller = &frontend.shell_controller;
    let gui = controller.gui();
    // The page's Refresh and list, found when its tree was built.
    let Some(nodes) = controller
        .root_in(DIALOG)
        .and_then(|root| ui.nodes(gui, root))
    else {
        return paint;
    };
    let refresh_pressed = nodes
        .refresh
        .is_some_and(|refresh| gui.shows_pressed(refresh));
    let art_of = |id: i32| (id == i32::from(REFRESH)).then_some(ButtonArt::Image);
    let pressed_of = |id: i32| (id == i32::from(REFRESH)).then_some(refresh_pressed);
    let picture_of = |id: i32| match u16::try_from(id).ok()? {
        REFRESH => Some(Picture::Icon {
            released: REFRESH_FACES.0,
            pressed: REFRESH_FACES.1,
        }),
        OBSERVER_IMAGE => Some(Picture::Observer { on: ui.observer }),
        _ => None,
    };
    let style = GuiStyle {
        art,
        art_of: Some(&art_of),
        pressed_of: Some(&pressed_of),
        picture_of: Some(&picture_of),
        ..GuiStyle::default()
    };
    // The root's children in tree order, the list after the rest.
    let children = gui.tree().children(nodes.root).unwrap_or_default();
    for child in children.iter().filter(|child| **child != nodes.list) {
        let widgets = paint_gui_styled(
            ime,
            bit_font,
            frontend,
            match_state,
            process_assets,
            gui,
            *child,
            scale,
            &[],
            style,
        );
        paint
            .controls
            .extend(widgets.lane(Material::Controls).copied());
        paint.icons.extend(widgets.lane(Material::Icons).copied());
        paint.texts.extend(widgets.texts);
    }

    // The list over its widget, each row's cells the page's: only the rows
    // the list shows are filled.
    let list = nodes.list;
    let top = gui.list_view(list).map_or(0, |view| view.top);
    let shown = gui
        .list_geometry(list)
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
    let list = paint_gui_styled(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        gui,
        list,
        scale,
        &[],
        style,
    );
    paint
        .controls
        .extend(list.lane(Material::Controls).copied());
    paint.icons.extend(list.lane(Material::Icons).copied());
    paint.texts.extend(list.texts);
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
