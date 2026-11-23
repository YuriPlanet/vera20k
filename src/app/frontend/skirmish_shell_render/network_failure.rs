//! ExecuteDoList64CD0C -> ShellMessageBox5D3490: CE over the empty
//! shell backdrop, not over the battlefield or the previous dialog.

use super::in_game_shell::{
    InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay, render_shell_frame,
};
use crate::app::AppState;
use crate::app::match_runtime::network_failure::{labels, vera_body};

pub(crate) fn render_network_failure(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let (width, height) = (state.render_width(), state.render_height());
    let (body, button) = labels(state);
    let shell_box = crate::ui::shell::modal::ShellBox::notice(
        vera_body(state).unwrap_or_else(|| state.csf_label(body.0, body.1)),
        state.csf_label(button.0, button.1),
    )
    .on_screen();
    let parts = super::shell_box::skirmish_parts(&shell_box, (width as i32, height as i32));
    crate::app::match_runtime::network_failure::sync_box(state, &shell_box, &parts);
    let pressed = crate::app::match_runtime::network_failure::ok_pressed(state);
    let pressed = |id| pressed && id == crate::ui::shell::modal::control::OK;
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
    let atlas = state.frontend.skirmish_shell_chrome.as_ref();
    if let Some(atlas) = atlas {
        // Original5D3514 -> DrawEmptyBackdrop52FEC0 ->72E820. Reuse the
        // existing generic shell/right-panel owner with every slot closed.
        super::modals::push_message_box_backdrop_instances(
            &mut art,
            atlas,
            &crate::ui::skirmish_shell::compute_layout_on(
                crate::ui::shell::geom::RETAIL_SCALE,
                width,
                height,
            ),
        );
    }
    let look =
        super::BoxLook::skirmish(atlas.and_then(|atlas| atlas.mode_two_dialog_background(None)));
    let (controls_out, texts_out) = (&mut controls, (&mut texts, false));
    let tree = super::controller_tree(&state.frontend.shell_controller, shell_box.dialog);
    super::shell_box::push_skirmish_box(
        state,
        atlas,
        tree,
        &shell_box,
        &parts,
        pressed,
        look,
        controls_out,
        texts_out,
    );
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
