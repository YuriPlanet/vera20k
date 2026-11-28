//! Mission restate briefing 0x10C over the active-game shell, as Diplomacy
//! 0x73: title, Return and status line in the right-panel places, and the
//! briefing as a plain left/top static wrapped and clipped to its rect
//! (`OwnerDraw_Static` 0x006153E0).
//!
//! Return and the briefing paint from 0x10C's `vera20k-gui` nodes
//! ([`super::gui_paint`]), made current first every frame.

use super::gui_paint::GuiStyle;
use crate::app::frontend::state::FrontendState;
use crate::app::input::restate::DIALOG;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::ui::shell::restate::RestateControl;

pub(crate) fn render_restate_shell(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    crate::app::input::restate::sync(platform, renderer, frontend, match_state, persistence)
        .expect("active 0x10C geometry");
    let frame = super::in_game_shell::paint_dialog(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
        &DIALOG,
        |paint, ui| {
            let (ime, bit_font) = (&platform.ime, &renderer.bit_font);
            let content = RestateControl::Content.id();
            // The briefing 0x6E7 is SS_LEFT, top aligned (its node's `Align`),
            // its text the node's own (`input::restate::sync`), borrowed.
            paint.nodes(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                ui,
                &[content],
                GuiStyle::default(),
            );
        },
    );
    super::in_game_shell::render_in_game_shell_frame(
        platform,
        renderer,
        frontend,
        match_state,
        persistence,
        encoder,
        destination,
        frame,
    )
}
