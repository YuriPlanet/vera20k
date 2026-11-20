//! Paint Single Player's Load Saved Game `0xB7`. Outside a suspended game it
//! paints as a family page (`0x00621FB1..0x00621FFE`): the default parent
//! background (`0x0060D20B`, MNSCRNL), the right panel, heading `0x694`,
//! status line `0x695`, Load and Back, and the list `0x525`. The template has
//! no monitor and no movie static, and the prompt `0x40C` is hidden.

use crate::ui::shell::list::ListMetrics;
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
use crate::ui::shell::list::{ListScrollPart, ShellListGeometry};
use crate::ui::shell::saved_games::{LOAD_BUTTON, LOAD_SAVED_GAME_PAGE};
use crate::ui::skirmish_shell::SavedSeedControl;

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
        |state| crate::app::App::load_saved_game_status_key(state).map(str::to_owned),
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
    // The browser's own input owns the buttons; a message box owns the press
    // while it is up, so the page buttons paint released under it.
    let pressed = browser
        .prompt
        .is_none()
        .then_some(browser.pressed_control)
        .flatten();
    let buttons = layout
        .page
        .buttons
        .iter()
        .map(|button| {
            let (control, enabled) = if button.id == LOAD_BUTTON {
                (SavedSeedControl::Action, browser.action_enabled())
            } else {
                (SavedSeedControl::Back0x686, true)
            };
            PageButton {
                id: button.id,
                paint: PaintButton {
                    rect: button.rect,
                    pressed: enabled && pressed == Some(control),
                    hovered: false,
                    enabled,
                },
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
        let geometry = ShellListGeometry::new(
            layout.browser.list,
            browser.entries.len(),
            browser.top_index,
            ListMetrics::scaled(layout.browser.scale),
        );
        let pressed = match browser.pressed_control {
            Some(SavedSeedControl::ScrollUp) => Some(ListScrollPart::Up),
            Some(SavedSeedControl::ScrollDown) => Some(ListScrollPart::Down),
            Some(SavedSeedControl::ScrollThumb) => Some(ListScrollPart::Thumb),
            Some(SavedSeedControl::ScrollTrack) => Some(ListScrollPart::Track),
            _ => None,
        };
        let view = vera20k_gui::ListView {
            rows: browser.entries.len(),
            top: browser.top_index,
            selected: browser.selected,
            pressed,
        };
        push_family_list(
            &mut frame.over_panel,
            chrome,
            &backdrop,
            layout.list_window,
            Some(&(geometry, view)),
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
