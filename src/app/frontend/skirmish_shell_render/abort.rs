//! Full-screen B6: campaign mode0 (Restart712 shown), Skirmish mode5
//! (712 hidden) and LAN/WOL (Observe712 or hidden). Original4F1840/4F18B0
//! and 60C540→621FB1→72F540; no modal
//! card and no title694.

use super::in_game_shell::{self, InGameShellFrame};
use super::text::{localized_label, push_text_draw, rect_to_text_rect};
use super::{SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::geom::RectPx;

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
    let ui = &state.match_state.match_presentation.dialog_ui;
    let hovered = crate::app::input::dialog_page::hovered(ui)
        .and_then(crate::ui::shell::abort::AbortButton::from_id);
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active B6 art");
    let mut art = in_game_shell::background_instances(atlas, shell, width, height);
    let mut texts = Vec::new();
    for (button, rect) in layout.buttons() {
        let (key, fallback) = button.label(layout.secondary_kind);
        let pressed = crate::app::input::dialog_page::pressed(ui, button.id());
        let caption = localized_label(state, key, fallback);
        let screen = RectPx::new(0, 0, width, height);
        in_game_shell::push_panel_button(
            &mut art,
            &mut texts,
            state,
            atlas,
            (rect, &caption, pressed),
            screen,
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
    if let Some(button) = hovered {
        let (key, fallback) = button.help(layout.secondary_kind);
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
