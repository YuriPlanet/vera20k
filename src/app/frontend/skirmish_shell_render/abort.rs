//! Full-screen B6, campaign mode0 (Restart712 shown) and Skirmish mode5
//! (712 hidden). Original4F1840/4F18B0 and 60C540→621FB1→72F540; no modal
//! card and no title694.

use super::in_game_shell::{self, InGameShellFrame};
use super::pause_menu::{button_frame, button_text_rect};
use super::text::{localized_label, push_text_draw, rect_to_text_rect};
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::pause_menu::PauseMenuButtonState;

pub(crate) fn render_abort_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let in_game_shell::InGameShellGeometry {
        width,
        height,
        layout: shell,
        ..
    } = in_game_shell::current_in_game_shell_layout(state).expect("active B6 geometry");
    let layout = crate::app::input::abort::current_abort_layout(state).expect("active B6 geometry");
    let interaction = state.match_state.match_presentation.abort_buttons;
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active B6 art");
    let mut art = in_game_shell::background_instances(atlas, shell, width, height);
    let mut texts = Vec::new();
    for (button, rect) in layout.buttons() {
        let pressed = interaction.is_pressed(button);
        let frame = button_frame(PauseMenuButtonState {
            pressed,
            ..Default::default()
        });
        if let Some(entry) = atlas.in_game_shell.buttons[frame].or(atlas.in_game_shell.buttons[0]) {
            in_game_shell::push_art(&mut art, entry, rect, RectPx::new(0, 0, width, height));
        }
        let (key, fallback) = button.label();
        push_text_draw(
            &mut texts,
            state,
            &localized_label(state, key, fallback),
            rect_to_text_rect(button_text_rect(rect, pressed)),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            SHELL_CONTROL_TEXT_DEPTH,
            1.0,
        );
    }
    // Plain static615A81..615AE8 uses low style bits1 for horizontal center;
    // it does not apply SS_CENTERIMAGE's vertical centering flag.
    push_text_draw(
        &mut texts,
        state,
        &localized_label(state, "GUI:AskAbortMission", "What would you like to do?"),
        rect_to_text_rect(layout.question),
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::H_CENTER,
        SHELL_CONTROL_TEXT_DEPTH,
        1.0,
    );
    if let Some(button) = interaction.hovered {
        let (key, fallback) = button.help();
        push_text_draw(
            &mut texts,
            state,
            &localized_label(state, key, fallback),
            rect_to_text_rect(layout.footer),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::NONE,
            SHELL_CONTROL_TEXT_DEPTH,
            1.0,
        );
    }
    in_game_shell::render_in_game_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls: Vec::new(),
            texts,
            label: "Active Abort Shell B6",
        },
    )
}
