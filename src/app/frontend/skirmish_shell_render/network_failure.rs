//! ExecuteDoList64CD0C -> ShellMessageBox5D3490: CE over the empty
//! shell backdrop, not over the battlefield or the previous dialog.

use super::in_game_shell::{
    InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay, render_shell_frame,
};
use crate::app::frontend::state::FrontendState;
use crate::app::match_runtime::network_failure::{labels, vera_body};
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;

pub(crate) fn render_network_failure(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &mut FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let (width, height) = (
        renderer.render_width(&frontend.screen),
        renderer.render_height(&frontend.screen),
    );
    let (body, button) = labels(&match_state.sim_runtime);
    let shell_box = crate::ui::shell::modal::ShellBox::notice(
        vera_body(frontend, match_state, process_assets)
            .unwrap_or_else(|| process_assets.csf_label(body.0, body.1)),
        process_assets.csf_label(button.0, button.1),
    )
    .on_screen();
    let parts = super::shell_box::skirmish_parts(&shell_box, (width as i32, height as i32));
    crate::app::match_runtime::network_failure::sync_box(
        &mut frontend.shell_controller,
        &shell_box,
        &parts,
    );
    let pressed =
        crate::app::match_runtime::network_failure::ok_pressed(&frontend.shell_controller);
    let pressed = |id| pressed && id == crate::ui::shell::modal::control::OK;
    let mut art = Vec::new();
    let mut controls = Vec::new();
    let mut texts = Vec::new();
    let frame_art = if frontend.skirmish_shell_chrome.is_some() {
        ShellFrameArt::Launcher
    } else {
        // A failed optional art load must still expose the blocking message
        // and its working button through the shared text compositor.
        ShellFrameArt::Clear
    };
    let atlas = frontend.skirmish_shell_chrome.as_ref();
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
    let tree = super::controller_tree(&frontend.shell_controller, shell_box.dialog);
    super::shell_box::push_skirmish_box(
        &platform.ime,
        &renderer.bit_font,
        frontend,
        match_state,
        process_assets,
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
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
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
