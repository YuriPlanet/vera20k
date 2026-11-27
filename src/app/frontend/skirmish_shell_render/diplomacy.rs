//! In-battle Diplomacy 0x73 over the active-game shell (621FB1 -> 72F540):
//! title, Return and status line in the right-panel places, the rows and
//! headers as ordinary children.
//!
//! Return, the paging buttons and VERA's "page n of m" line, the headers,
//! the rows (insignia, names and kills in the House colour, the dual-state
//! Allies and the Chat boxes, dimmed while disabled), Taunts and the mode
//! and map statics paint from the dialog's `vera20k-gui` nodes
//! ([`super::gui_paint`]), made current first every frame.

use super::gui_paint::{ButtonArt, GuiStyle};
use crate::app::frontend::state::FrontendState;
use crate::app::input::diplomacy::DIALOG;
use crate::app::match_runtime::state::MatchState;
use crate::app::persistence::PersistenceState;
use crate::app::process_assets::ProcessAssets;
use crate::app::renderer_state::RendererState;
use crate::app::state::PlatformState;
use crate::ui::shell::diplomacy::{
    DIPLOMACY_ROWS, DiplomacyControl, DiplomacyHeader, TAUNTS_LABEL,
};
use crate::ui::shell::paging;

pub(crate) fn render_diplomacy_shell(
    platform: &mut PlatformState,
    renderer: &mut RendererState,
    frontend: &FrontendState,
    match_state: &mut MatchState,
    process_assets: &ProcessAssets,
    persistence: &PersistenceState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let (_, view) = crate::app::input::diplomacy::sync(
        platform,
        renderer,
        frontend,
        match_state,
        process_assets,
        persistence,
    )
    .expect("active 0x73 geometry");
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
                if id == paging::COUNTER {
                    return Some(paging::counter_text(view.page_index, view.page_count));
                }
                let (key, fallback) = if id == DiplomacyControl::Taunts.id() {
                    TAUNTS_LABEL
                } else {
                    DiplomacyHeader::ALL
                        .into_iter()
                        .find(|header| DiplomacyControl::Header(*header).id() == id)?
                        .label()
                };
                Some(process_assets.csf_label(key, fallback))
            };
            let style = GuiStyle {
                buttons: ButtonArt::Side,
                side: Some(paint.side_buttons()),
                text_of: Some(&text_of),
                dim_disabled_checkboxes: true,
                ..Default::default()
            };
            // The paging buttons and their counter show only where there is
            // more than one page.
            paint.nodes(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                ui,
                &[paging::PREVIOUS, paging::NEXT, paging::COUNTER],
                style,
            );
            // The Allies and Chat headers hide as their columns do.
            let headers = DiplomacyHeader::ALL.map(|header| DiplomacyControl::Header(header).id());
            paint.nodes(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                ui,
                &headers,
                style,
            );
            for row in 0..DIPLOMACY_ROWS {
                let ids = [
                    DiplomacyControl::Insignia(row),
                    DiplomacyControl::Name(row),
                    DiplomacyControl::Kills(row),
                    DiplomacyControl::Allies(row),
                    DiplomacyControl::Chat(row),
                ]
                .map(|c| c.id());
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
            }
            // Taunts with its caption 26 px right of the box, vertically centred
            // (0x0061663A..0x00616674), then the mode and map statics.
            let tail = [
                DiplomacyControl::Taunts,
                DiplomacyControl::ModeName,
                DiplomacyControl::MapName,
            ]
            .map(|c| c.id());
            paint.nodes(
                ime,
                bit_font,
                frontend,
                match_state,
                process_assets,
                ui,
                &tail,
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
