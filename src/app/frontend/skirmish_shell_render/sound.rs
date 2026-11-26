//! Full-screen B8, original6B6300 and shared native shell/control owners.
//!
//! Its trackbars, boxes, list, Play/Stop (MNBTTN, type-2 captions) and
//! slider captions paint from its `vera20k-gui` nodes ([`super::gui_paint`]),
//! made current first every frame.

use super::gui_paint::{ButtonArt, GuiStyle};
use crate::app::frontend::state::FrontendState;
use crate::app::input::sound::DIALOG;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::ui::shell::sound;

pub(crate) fn render_sound_shell(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    crate::app::input::sound::sync(platform, renderer, frontend, match_state, persistence)
        .expect("active B8 geometry");
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
            let text_of = |id: i32| -> Option<String> {
                let (key, fallback) = sound::caption(u16::try_from(id).ok()?)?;
                Some(process_assets.csf_label(key, fallback))
            };
            // 609FC7..609FE2/60A330 selects type3. MNBTTN uses native canvas
            // centered on the resource control, not the side column's SIDEBTTN.
            let style = GuiStyle {
                buttons: ButtonArt::ModalType2,
                text_of: Some(&text_of),
                ..Default::default()
            };
            paint.nodes(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                ui,
                &sound::PAINT_ORDER,
                style,
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
