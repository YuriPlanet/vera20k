//! Paint Single Player's Load Saved Game `0xB7`. Outside a suspended game it
//! paints as a family page (`0x00621FB1..0x00621FFE`): the default parent
//! background (`0x0060D20B`, MNSCRNL), the right panel, heading `0x694`,
//! status line `0x695`, Load and Back, and the list `0x525`. The template has
//! no monitor and no movie static, and the prompt `0x40C` is hidden.

use anyhow::Result;

use crate::app::AppState;
use crate::app::frontend::main_menu_shell_render::shell_reveal_path_a;
use crate::app::frontend::movies_credits_render::{
    FamilyListRows, family_backdrop, push_family_backdrop, push_family_list,
};
use crate::app::frontend::shell_pass::{
    ShellComposition, TexturedDraw, encode_shell_pass, owner_draw_button_label_rect, resolve_csf,
    software_cursor,
};
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::shell_paint::{
    self, CURSOR_DEPTH, PaintButton, PaintLabel, SHELL_TEXT_RGB_DISABLED, SHELL_TEXT_RGB_ENABLED,
};
use crate::render::shell_text::ShellAlign;
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
    // The teardown slide starts with a full dialog repaint (`0x00622C4F`)
    // and pumps no messages until it ends: every child stays blank.
    let exit_wave = crate::app::frontend::shell_transition::shell_exit_wave(
        state,
        ShellSlideKind::LoadSavedGame,
    )
    .cloned();
    let leaving = exit_wave.is_some();
    let wave = exit_wave.or_else(|| state.frontend.shell_first_paint_slide.clone());
    let (title_window, status_label) = if leaving {
        (None, None)
    } else {
        let title_window = state
            .frontend
            .shell_page_title
            .paint(std::time::Instant::now());
        let status_text = crate::app::App::load_saved_game_status_key(state)
            .map(|key| resolve_csf(state, key).into_owned())
            .unwrap_or_default();
        let status_label = crate::app::frontend::menu_page_render::paint_shell_status_line(
            state,
            status_text,
            layout.page.status_help,
        );
        (title_window, status_label)
    };

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
    let backdrop = family_backdrop(chrome, layout.page.screen.w, layout.page.screen.h);
    let mut sprites = Vec::new();
    push_family_backdrop(&mut sprites, &backdrop);
    sprites.extend(shell_paint::paint_chrome(
        chrome,
        layout.page.right_panel,
        Some(layout.page.lower_strip),
        layout.page.screen.w,
    ));
    let geometry = ShellListGeometry::new(
        layout.browser.list,
        browser.entries.len(),
        browser.top_index,
    );
    if !leaving {
        let pressed = match browser.pressed_control {
            Some(SavedSeedControl::ScrollUp) => Some(ListScrollPart::Up),
            Some(SavedSeedControl::ScrollDown) => Some(ListScrollPart::Down),
            Some(SavedSeedControl::ScrollThumb) => Some(ListScrollPart::Thumb),
            Some(SavedSeedControl::ScrollTrack) => Some(ListScrollPart::Track),
            _ => None,
        };
        push_family_list(
            &mut sprites,
            chrome,
            &backdrop,
            layout.list_window,
            Some(FamilyListRows {
                geometry: &geometry,
                top: browser.top_index,
                selected: browser.selected,
                pressed,
            }),
        );
    }

    // A message box owns the press while it is up; the page buttons paint
    // released under it.
    let pressed = browser
        .prompt
        .is_none()
        .then_some(browser.pressed_control)
        .flatten();
    let load_enabled = browser.action_enabled();
    let button_state = |id: u16| {
        let control = if id == LOAD_BUTTON {
            SavedSeedControl::Action
        } else {
            SavedSeedControl::Back0x686
        };
        let enabled = id != LOAD_BUTTON || load_enabled;
        (enabled && pressed == Some(control), enabled)
    };
    // While a slide runs the engine draws the whole tile column in place of
    // the buttons (`0x006071E0`).
    let buttons: Vec<PaintButton> = match wave.as_ref() {
        Some(wave) => {
            sprites.extend(shell_paint::paint_slide_column(
                chrome,
                layout.page.right_panel,
                &wave.button_draws(),
            ));
            Vec::new()
        }
        None => layout
            .page
            .buttons
            .iter()
            .map(|button| {
                let (pressed, enabled) = button_state(button.id);
                PaintButton {
                    rect: button.rect,
                    pressed,
                    hovered: false,
                    enabled,
                }
            })
            .collect(),
    };
    let button_sprites = shell_paint::paint_buttons(
        chrome,
        &buttons,
        crate::app::frontend::menu_page_render::MENU_PAGE_BUTTON_POLICY,
        std::time::Instant::now(),
        None,
    );
    let mut labels = Vec::new();
    for button in layout.page.buttons.iter().filter(|_| wave.is_none()) {
        let Some(spec) = LOAD_SAVED_GAME_PAGE.button(button.id) else {
            continue;
        };
        let (pressed, enabled) = button_state(button.id);
        labels.push(PaintLabel {
            text: resolve_csf(state, spec.csf_key),
            rect: owner_draw_button_label_rect(button.rect, pressed),
            align: ShellAlign(ShellAlign::H_CENTER.0 | ShellAlign::V_CENTER.0),
            rgb: if enabled {
                SHELL_TEXT_RGB_ENABLED
            } else {
                SHELL_TEXT_RGB_DISABLED
            },
            path_a_reveal: None,
        });
    }
    if let Some(window) = title_window {
        labels.push(PaintLabel {
            text: resolve_csf(state, LOAD_SAVED_GAME_PAGE.title_key),
            rect: layout.page.title,
            align: ShellAlign::H_CENTER,
            rgb: SHELL_TEXT_RGB_ENABLED,
            path_a_reveal: Some(shell_reveal_path_a(window)),
        });
    }
    labels.extend(status_label);
    let mut text = shell_paint::paint_labels(&state.renderer.bit_font, &labels);
    let prompt_sprites = crate::app::frontend::skirmish_shell_render::family_saved_game_overlays(
        state,
        &layout.browser,
        browser,
        !leaving,
        &mut text,
    );
    let prompt_texture = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .map(|atlas| &atlas.texture);
    let mut draws = vec![
        TexturedDraw {
            texture: &chrome.texture,
            instances: sprites,
        },
        TexturedDraw {
            texture: &chrome.texture,
            instances: button_sprites,
        },
    ];
    if let Some(texture) = prompt_texture {
        draws.push(TexturedDraw {
            texture,
            instances: prompt_sprites,
        });
    }
    encode_shell_pass(
        state,
        encoder,
        destination,
        "Load Saved Game Shell",
        ShellComposition {
            draws: &draws,
            text: &text,
            cursor: software_cursor(state, CURSOR_DEPTH),
            effects: None,
        },
    );
    Ok(true)
}
