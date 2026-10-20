//! Mission restate briefing 0x10C over the active-game shell, as Diplomacy
//! 0x73: title, Return and status line in the right-panel places, and the
//! briefing as a plain left/top static wrapped and clipped to its rect
//! (`OwnerDraw_Static` 0x006153E0).

use super::in_game_shell::{self, InGameShellFrame};
use super::pause_menu::{button_frame, button_text_rect};
use super::text::{localized_label, push_text_draw, rect_to_text_rect};
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::geom::RectPx;
use crate::ui::shell::pause_menu::PauseMenuButtonState;
use crate::ui::shell::restate::{RETURN_LABEL, RestateButton, TITLE_LABEL, restate_layout};

pub(crate) fn render_restate_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let in_game_shell::InGameShellGeometry {
        width,
        height,
        layout: shell,
        button_size,
    } = in_game_shell::current_in_game_shell_layout(state).expect("active 0x10C geometry");
    let layout = restate_layout(width, height, shell, button_size);
    let interaction = state.match_state.match_presentation.restate;
    let screen = RectPx::new(0, 0, width, height);
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active 0x10C art");
    let mut art = in_game_shell::background_instances(atlas, shell, width, height);
    let mut texts = Vec::new();
    let text = |texts: &mut Vec<_>, label: &str, rect: RectPx, align| {
        push_text_draw(
            texts,
            state,
            label,
            rect_to_text_rect(rect),
            SHELL_LABEL_TEXT_RGB,
            align,
            SHELL_CONTROL_TEXT_DEPTH,
            1.0,
        );
    };

    let pressed = interaction.button.is_pressed(RestateButton::Return);
    let frame = button_frame(PauseMenuButtonState {
        pressed,
        ..Default::default()
    });
    if let Some(entry) = atlas.in_game_shell.buttons[frame].or(atlas.in_game_shell.buttons[0]) {
        in_game_shell::push_art(&mut art, entry, layout.return_button, screen);
    }
    text(
        &mut texts,
        &localized_label(state, RETURN_LABEL.0, RETURN_LABEL.1),
        button_text_rect(layout.return_button, pressed),
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
    );
    // Title 0x694 is SS_CENTER; the briefing 0x6E7 is SS_LEFT, top aligned.
    text(
        &mut texts,
        &localized_label(state, TITLE_LABEL.0, TITLE_LABEL.1),
        layout.title,
        ShellAlign::H_CENTER,
    );
    let briefing = state.match_state.match_presentation.restate_text.clone();
    text(&mut texts, &briefing, layout.content, ShellAlign::NONE);
    if let Some(key) = interaction.hovered.and_then(|control| control.help_key()) {
        text(
            &mut texts,
            &localized_label(state, key, ""),
            layout.status_line,
            ShellAlign::NONE,
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
            label: "Active Restate Shell 10C",
        },
    )
}
