//! Active saved-game Load/Save/Delete browsers. The native row/editor/prompt
//! mechanism is shared with saved seeds; only template geometry, visible40C,
//! mode titles and the active-game parent/button art differ here.

use super::in_game_shell::{InGamePaint, render_in_game_shell_frame};
use super::modals::{BackdropInteriorPaint, push_saved_browser_modal_instances};
use super::shell_box::{push_saved_browser_prompt_instances, push_saved_browser_prompt_text};
use super::text::push_saved_browser_contents_text;
use crate::app::AppState;
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::pause_menu::PauseMenuButtonState;
use crate::ui::shell::saved_games::{saved_game_layout, saved_game_title_label};
use crate::ui::skirmish_shell::saved_seed_browser::{ACTION, BACK};

pub(crate) fn render_saved_game_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let frame = {
        let mut paint = InGamePaint::new(state);
        let state = paint.state;
        let presentation = &state.match_state.match_presentation;
        let browser = presentation
            .saved_game_browser
            .as_ref()
            .expect("active saved-game browser");
        let layout = paint
            .shell
            .place(|w, h, shell, size| saved_game_layout(browser.mode, w, h, shell, size));
        // 558F8A's hide40C condition is false in the active-game route. Both
        // resource statics use lowstylebits1: ownerdraw615A81 selects top/H_CENTER.
        for (label, rect, align) in [
            (
                saved_game_title_label(browser.mode),
                layout.title,
                ShellAlign::PANEL_TITLE,
            ),
            (
                browser.mode.prompt_label(),
                layout.prompt,
                ShellAlign::H_CENTER,
            ),
        ] {
            let text = paint.label(label);
            paint.text(&text, rect, align);
        }
        push_saved_browser_contents_text(&mut paint.texts, state, &layout, browser);
        for (id, rect, label, enabled) in [
            (
                ACTION,
                layout.action,
                browser.mode.action_label(),
                browser.action_enabled(),
            ),
            (BACK, layout.back, ("GUI:Back", "Back"), true),
        ] {
            // A confirmation drops the browser's press while open; its buttons
            // are painted by the common prompt renderer, not the underlying column.
            let pressed = browser.prompt.is_none() && browser.pressed(id);
            let caption = paint.label(label);
            paint.owner_button(
                rect,
                &caption,
                PauseMenuButtonState {
                    pressed,
                    highlighted: false,
                    enabled,
                },
            );
        }
        // Shared confirmation clipping excludes all underlying text, including the
        // active-game title and column labels, before drawing the prompt text.
        push_saved_browser_prompt_text(&mut paint.texts, state, &layout, browser);
        let control_atlas = state
            .frontend
            .skirmish_shell_chrome
            .as_ref()
            .expect("active saved-game controls");
        let font = &state.renderer.bit_font;
        let interior = BackdropInteriorPaint::PreserveBacking;
        push_saved_browser_modal_instances(
            &mut paint.controls,
            control_atlas,
            font,
            &layout,
            browser,
            interior,
            false,
        );
        paint.finish("Active Saved Game Browser")
    };
    render_in_game_shell_frame(state, encoder, destination, frame)
}

/// Row text and the message box of Single Player's Load Saved Game `0xB7`,
/// a family page: the row text (description, date, time) goes into `texts`
/// unless the teardown slide blanks the list, then the box's text clips
/// everything under it. Returns the box's art, drawn from the skirmish
/// chrome, when one is up.
pub(crate) fn family_saved_game_overlays(
    state: &AppState,
    layout: &crate::ui::skirmish_shell::SavedSeedLayout,
    browser: &crate::ui::skirmish_shell::SavedSeedBrowserState<std::path::PathBuf>,
    rows: bool,
    texts: &mut Vec<crate::render::shell_text::ShellTextDraw>,
) -> Vec<crate::render::batch::SpriteInstance> {
    if rows {
        push_saved_browser_contents_text(texts, state, layout, browser);
    }
    push_saved_browser_prompt_text(texts, state, layout, browser);
    let mut sprites = Vec::new();
    if let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() {
        push_saved_browser_prompt_instances(&mut sprites, atlas, layout, browser);
    }
    sprites
}
