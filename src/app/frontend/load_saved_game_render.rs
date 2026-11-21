//! Paint Single Player's Load Saved Game `0xB7`. Outside a suspended game it
//! paints as a family page (`0x00621FB1..0x00621FFE`): the default parent
//! background (`0x0060D20B`, MNSCRNL), the right panel, heading `0x694`,
//! status line `0x695`, Load and Back, and the list `0x525`. The template has
//! no monitor and no movie static, and the prompt `0x40C` is hidden.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::menu_page_render::{
    FamilyFrame, encode_family_page, family_art_ready, page_panel_state,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::app::frontend::skirmish_shell_render::gui_paint::GuiStyle;
use crate::ui::shell::list::{ListMetrics, ShellListGeometry};
use crate::ui::shell::saved_games::LOAD_SAVED_GAME_PAGE;
use crate::ui::skirmish_shell::saved_seed_browser::{ACTION, BROWSER_DIALOG, LIST};

/// Paint `0xB7`. Returns `false` when the shell chrome is missing.
pub(crate) fn render_load_saved_game_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if state.frontend.load_saved_game.is_none() || !family_art_ready(state) {
        return Ok(false);
    }
    let layout = crate::app::frontend::shell_layout::load_saved_game_layout(state);
    let panel = page_panel_state(
        state,
        ShellSlideKind::LoadSavedGame,
        false,
        crate::app::App::load_saved_game_status_key,
        layout.page.status_help,
    );
    let leaving = panel.leaving;
    let (Some(browser), Some(atlas)) = (
        state.frontend.load_saved_game.as_ref(),
        state.frontend.skirmish_shell_chrome.as_ref(),
    ) else {
        return Ok(false);
    };
    // The browser's own Load and Back are the column's buttons, with the
    // page's captions; a message box drops their press, so they paint
    // released under it.
    let caption = |id: i32| {
        let button = LOAD_SAVED_GAME_PAGE.button(u16::try_from(id).ok()?)?;
        Some(state.csf_text(button.csf_key))
    };
    let enabled = |id: i32| (id == i32::from(ACTION)).then(|| browser.action_enabled());
    let title = LOAD_SAVED_GAME_PAGE.title_key;
    let mut frame = FamilyFrame::new("Load Saved Game Shell", &layout.page, panel, title);
    let ui = &browser.ui;
    frame.buttons = ui.root_in(BROWSER_DIALOG).map(|root| (ui.gui(), root));
    frame.style = GuiStyle {
        text_of: Some(&caption),
        enabled_of: Some(&enabled),
        ..GuiStyle::default()
    };
    frame.backdrop = true;
    let view = browser.node(LIST).and_then(|list| ui.gui().list_view(list));
    if let Some(view) = view.filter(|_| !leaving) {
        let scale = layout.browser.scale;
        let metrics = ListMetrics::scaled(scale);
        let geometry = ShellListGeometry::new(layout.browser.list, view.rows, view.top, metrics);
        crate::app::frontend::skirmish_shell_render::paint_list(
            &mut frame.over.controls,
            atlas,
            geometry,
            view.top,
            view.selected,
            view.pressed,
            false,
            scale,
        );
    }
    let prompt = crate::app::frontend::skirmish_shell_render::family_saved_game_overlays(
        state,
        &layout.browser,
        browser,
        !leaving,
        &mut frame.over.texts,
    );
    frame.over.controls.extend(prompt);
    encode_family_page(state, encoder, destination, frame);
    Ok(true)
}
