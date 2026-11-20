//! Paint Single Player's Load Saved Game `0xB7`. Outside a suspended game it
//! paints as a family page (`0x00621FB1..0x00621FFE`): the default parent
//! background (`0x0060D20B`, MNSCRNL), the right panel, heading `0x694`,
//! status line `0x695`, Load and Back, and the list `0x525`. The template has
//! no monitor and no movie static, and the prompt `0x40C` is hidden.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::menu_page_render::{
    FamilyFrame, PageButton, encode_family_page, page_panel_state,
};
use crate::app::frontend::movies_credits_render::{
    family_backdrop, push_family_backdrop, push_family_list,
};
use crate::app::frontend::shell_pass::TexturedDraw;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::shell_paint::PaintButton;
use crate::ui::shell::list::{ListMetrics, ShellListGeometry};
use crate::ui::shell::saved_games::{LOAD_BUTTON, LOAD_SAVED_GAME_PAGE};
use crate::ui::skirmish_shell::saved_seed_browser::{ACTION, BACK, LIST};

/// Paint `0xB7`. Returns `false` when the shell chrome is missing.
pub(crate) fn render_load_saved_game_page(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> Result<bool> {
    if state.frontend.main_menu_shell_chrome.is_none() || state.frontend.load_saved_game.is_none() {
        return Ok(false);
    }
    let layout = crate::app::App::load_saved_game_layout(state);
    let panel = page_panel_state(
        state,
        ShellSlideKind::LoadSavedGame,
        false,
        crate::app::App::load_saved_game_status_key,
        layout.page.status_help,
    );
    let leaving = panel.leaving;
    let chrome = state
        .frontend
        .main_menu_shell_chrome
        .as_ref()
        .expect("checked before render");
    let browser = state
        .frontend
        .load_saved_game
        .as_ref()
        .expect("checked before render");
    let backdrop = family_backdrop(
        chrome,
        layout.page.screen.w,
        layout.page.screen.h,
        layout.page.scale,
    );
    // The browser's own widgets hold the buttons' press; a message box drops
    // it, so the page buttons paint released under it.
    let buttons = layout
        .page
        .buttons
        .iter()
        .map(|button| {
            let (control, enabled) = match button.id {
                LOAD_BUTTON => (ACTION, browser.action_enabled()),
                _ => (BACK, true),
            };
            let pressed = enabled && browser.pressed(control);
            let paint = PaintButton {
                rect: button.rect,
                pressed,
                hovered: false,
                enabled,
            };
            PageButton {
                id: button.id,
                paint,
            }
        })
        .collect();
    let mut frame = FamilyFrame::new(
        "Load Saved Game Shell",
        &LOAD_SAVED_GAME_PAGE,
        &layout.page,
        panel,
        buttons,
    );
    let mut background = Vec::new();
    push_family_backdrop(&mut background, &backdrop);
    frame.under.push(TexturedDraw {
        texture: &chrome.texture,
        instances: background,
    });
    if !leaving {
        let view = browser
            .node(LIST)
            .and_then(|list| browser.ui.gui().list_view(list));
        let metrics = ListMetrics::scaled(layout.browser.scale);
        let lists = view.map(|view| {
            (
                ShellListGeometry::new(layout.browser.list, view.rows, view.top, metrics),
                view,
            )
        });
        push_family_list(
            &mut frame.over_panel,
            chrome,
            &backdrop,
            layout.list_window,
            lists.as_ref(),
        );
    }
    let prompt = crate::app::frontend::skirmish_shell_render::family_saved_game_overlays(
        state,
        &layout.browser,
        browser,
        !leaving,
        &mut frame.over_text,
    );
    frame.over = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .map(|atlas| TexturedDraw {
            texture: &atlas.texture,
            instances: prompt,
        });
    encode_family_page(state, encoder, destination, frame);
    Ok(true)
}
