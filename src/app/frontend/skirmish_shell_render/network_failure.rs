//! ExecuteDoList64CD0C -> ShellMessageBox5D3490: CE over the empty
//! shell backdrop, not over the battlefield or the previous dialog.

use super::in_game_shell::{
    InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay, render_shell_frame,
};
use super::text::localized_label;
use crate::app::AppState;
use crate::app::match_runtime::network_failure::{BODY, DIALOG, STOP};
use crate::ui::shell::geom::ShellScale;

pub(crate) fn render_network_failure(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let (width, height) = (state.render_width(), state.render_height());
    let labels = crate::ui::skirmish_shell::SkirmishValidationModalState::new(
        localized_label(state, BODY.0, BODY.1),
        localized_label(state, STOP.0, STOP.1),
    );
    let modal = crate::ui::shell::modal::body_ok_layout(width as i32, height as i32);
    let pressed = state.frontend.shell_controller.top_id() == Some(DIALOG)
        && state.frontend.shell_controller.pressed() == Some(crate::ui::shell::modal::control::OK);
    let mut art = Vec::new();
    let mut controls = Vec::new();
    let mut texts = Vec::new();
    let frame_art = if state.frontend.skirmish_shell_chrome.is_some() {
        ShellFrameArt::Launcher
    } else {
        // A failed optional art load must still expose the blocking message
        // and its working button through the shared text compositor.
        ShellFrameArt::Clear
    };
    if let Some(atlas) = state.frontend.skirmish_shell_chrome.as_ref() {
        // Original5D3514 -> DrawEmptyBackdrop52FEC0 ->72E820. Reuse the
        // existing generic shell/right-panel owner with every slot closed.
        super::modals::push_message_box_backdrop_instances(
            &mut art,
            atlas,
            &crate::ui::skirmish_shell::compute_layout_on(ShellScale::RETAIL, width, height),
        );
        super::modals::push_validation_modal_instances(&mut controls, atlas, &modal, pressed);
    }
    super::text::push_validation_modal_text_draws(&mut texts, state, &labels, &modal, pressed);
    render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "Checksum Reconnection Error CE",
        },
        frame_art,
        ShellFrameOverlay::default(),
        ShellFrameCursor::Visible,
    )
}
