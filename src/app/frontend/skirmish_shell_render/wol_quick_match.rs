//! `0x117`'s left-side children from their widgets
//! ([`crate::ui::wol_quick_match::spec`]): the "Match Status" caption and
//! the status transcript `0x53F` (an owner-draw list, `0x50014151`), its
//! rows the page's. The WOL page compositor draws them over its own art and
//! chrome.

use super::gui_paint::{GuiStyle, paint_gui_ids};
use super::wol_lobby::WolLobbyPaint;
use crate::app::frontend::state::FrontendState;
use crate::app::input::ime::ImeState;
use crate::app::match_runtime::state::MatchState;
use crate::app::process_assets::ProcessAssets;
use crate::render::bit_font::BitFont;
use crate::ui::wol_quick_match::{
    CAPTION, DIALOG, TRANSCRIPT, WolQuickMatchLayout, WolQuickMatchPage,
};

pub(crate) fn paint_wol_quick_match_children(
    ime: &ImeState,
    bit_font: &BitFont,
    frontend: &FrontendState,
    match_state: &MatchState,
    process_assets: &ProcessAssets,
    ui: &WolQuickMatchPage,
    layout: &WolQuickMatchLayout,
) -> WolLobbyPaint {
    let controller = &frontend.shell_controller;
    let gui = controller.gui();
    let Some(root) = controller.root_in(DIALOG) else {
        return WolLobbyPaint::default();
    };
    // Only the rows the list shows are named.
    let list = gui.find_id(root, TRANSCRIPT.into());
    let top = list
        .and_then(|list| gui.list_view(list))
        .map_or(0, |view| view.top);
    let shown = list
        .and_then(|list| gui.list_geometry(list))
        .map_or(0, |geometry| geometry.visible_rows);
    let rows_of = |id: i32| {
        (id == i32::from(TRANSCRIPT)).then(|| {
            let rows = ui.lines.iter().enumerate();
            rows.map(|(row, line)| match (top..top + shown).contains(&row) {
                true => (String::from_utf16_lossy(&line.text), line.color),
                false => (String::new(), line.color),
            })
            .collect()
        })
    };
    let style = GuiStyle {
        rows_of: Some(&rows_of),
        control_lists: true,
        ..Default::default()
    };
    let mut paint = paint_gui_ids(
        ime,
        bit_font,
        frontend,
        match_state,
        process_assets,
        gui,
        root,
        [CAPTION, TRANSCRIPT],
        layout.page.scale,
        style,
    );
    WolLobbyPaint {
        controls: paint.take_lane(crate::render::hud_paint::Material::Controls),
        icons: Vec::new(),
        texts: paint.texts,
    }
}
