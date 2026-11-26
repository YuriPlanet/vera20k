//! Full-screen B6: campaign mode0 (Restart712 shown), Skirmish mode5
//! (712 hidden) and LAN/WOL (Observe712 or hidden). Original4F1840/4F18B0
//! and 60C540→621FB1→72F540; no modal
//! card and no title694.
//!
//! Its buttons and question paint from its `vera20k-gui` nodes
//! ([`super::gui_paint`]), made current first every frame.

use super::gui_paint::{ButtonArt, GuiStyle};
use crate::app::frontend::state::FrontendState;
use crate::app::input::abort::DIALOG;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::ui::shell::abort::{AbortButton, QUESTION, QUESTION_LABEL};

pub(crate) fn render_abort_shell(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let layout =
        crate::app::input::abort::sync(platform, renderer, frontend, match_state, persistence)
            .expect("active B6 geometry");
    let secondary = layout.secondary_kind;
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
                let id = u16::try_from(id).ok()?;
                let (key, fallback) = match AbortButton::from_id(id) {
                    Some(button) => button.label(secondary),
                    None if id == QUESTION => QUESTION_LABEL,
                    None => return None,
                };
                Some(process_assets.csf_label(key, fallback))
            };
            let style = GuiStyle {
                buttons: ButtonArt::Side,
                side: Some(paint.side_buttons()),
                text_of: Some(&text_of),
                ..Default::default()
            };
            // Resume 686 is the lower button; 712 shows only where the mode
            // gives it a rect.
            let ids = [
                AbortButton::Leave.id(),
                AbortButton::Secondary.id(),
                QUESTION,
            ];
            paint.nodes(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                ui,
                &ids,
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
