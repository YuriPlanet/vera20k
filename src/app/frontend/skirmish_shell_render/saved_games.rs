//! Active saved-game Load/Save/Delete browsers. The native row/editor/prompt
//! mechanism is shared with saved seeds; only template geometry, visible40C,
//! mode titles and the active-game parent/button art differ here.

use super::in_game_shell::{self, InGameShellFrame};
use super::modals::{
    BackdropInteriorPaint, push_saved_browser_modal_instances, push_saved_browser_prompt_instances,
};
use super::pause_menu::{button_frame, button_text_rect, button_text_rgb};
use super::text::{
    localized_label, push_saved_browser_contents_text, push_saved_browser_prompt_text,
    push_text_draw, rect_to_text_rect,
};
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::pause_menu::PauseMenuButtonState;
use crate::ui::shell::saved_games::{saved_game_layout, saved_game_title_label};
use crate::ui::skirmish_shell::SavedSeedControl;

pub(crate) fn render_saved_game_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let width = state.renderer.gpu.config.width as i32;
    let height = state.renderer.gpu.config.height as i32;
    let (shell, button_size) = in_game_shell::current_in_game_shell_layout(state)
        .expect("active saved-game shell geometry");
    let browser = state
        .match_state
        .match_presentation
        .saved_game_browser
        .as_ref()
        .expect("active saved-game browser");
    let layout = saved_game_layout(browser.mode, width, height, shell, button_size);
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active side art");
    let theme = crate::app::presentation::sidebar_render::current_sidebar_theme(state);
    let mut art = in_game_shell::background_instances(atlas, shell, width, height);
    let mut texts = Vec::new();
    let (title_key, title_fallback) = saved_game_title_label(browser.mode);
    let (prompt_key, prompt_fallback) = browser.mode.prompt_label();
    // 558F8A's hide40C condition is false in the active-game route. Both
    // resource statics use lowstylebits1: ownerdraw615A81 selects top/H_CENTER.
    for (key, fallback, rect) in [
        (title_key, title_fallback, layout.title),
        (prompt_key, prompt_fallback, layout.prompt),
    ] {
        push_text_draw(
            &mut texts,
            state,
            &localized_label(state, key, fallback),
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::H_CENTER,
            SHELL_CONTROL_TEXT_DEPTH,
        );
    }
    push_saved_browser_contents_text(&mut texts, state, &layout, browser);
    let (action_key, action_fallback) = browser.mode.action_label();
    for (id, rect, key, fallback, enabled) in [
        (
            SavedSeedControl::Action,
            layout.action,
            action_key,
            action_fallback,
            browser.action_enabled(),
        ),
        (
            SavedSeedControl::Back0x686,
            layout.back,
            "GUI:Back",
            "Back",
            true,
        ),
    ] {
        // A confirmation owns pressed_control while open; its buttons are
        // painted by the common prompt renderer, not the underlying column.
        let button = PauseMenuButtonState {
            pressed: browser.prompt.is_none() && browser.pressed_control == Some(id),
            highlighted: false,
            enabled,
        };
        if let Some(entry) =
            atlas.in_game_shell.buttons[button_frame(button)].or(atlas.in_game_shell.buttons[0])
        {
            in_game_shell::push_art(&mut art, entry, rect, RectPx::new(0, 0, width, height));
        }
        push_text_draw(
            &mut texts,
            state,
            &localized_label(state, key, fallback),
            rect_to_text_rect(button_text_rect(rect, button.pressed)),
            button_text_rgb(theme, enabled),
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_CONTROL_TEXT_DEPTH,
        );
    }
    // Shared confirmation clipping excludes all underlying text, including the
    // active-game title and column labels, before drawing the prompt text.
    push_saved_browser_prompt_text(&mut texts, state, &layout, browser);
    let mut controls = Vec::new();
    let control_atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("active saved-game controls");
    push_saved_browser_modal_instances(
        &mut controls,
        control_atlas,
        &state.renderer.bit_font,
        &layout,
        browser,
        BackdropInteriorPaint::PreserveBacking,
        false,
    );
    in_game_shell::render_in_game_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "Active Saved Game Browser",
        },
    )
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
