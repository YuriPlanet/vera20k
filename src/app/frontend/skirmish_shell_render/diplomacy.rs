//! In-battle Diplomacy 0x73 over the active-game shell (621FB1 -> 72F540):
//! title, Return and status line in the right-panel places, the rows and
//! headers as ordinary children.
//!
//! Return, the paging buttons, the headers, the rows (insignia, names and
//! kills in the House colour, the dual-state Allies and the Chat boxes,
//! dimmed while disabled), Taunts and the mode and map statics paint from
//! the dialog's `vera20k-gui` nodes ([`super::gui_paint`]), made current
//! first every frame; VERA's "page n of m" line stays hand-painted.

use super::gui_paint::{ButtonArt, GuiStyle};
use crate::app::AppState;
use crate::app::input::diplomacy::DIALOG;
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::diplomacy::{
    DIPLOMACY_ROWS, DiplomacyControl, DiplomacyHeader, TAUNTS_LABEL,
};

pub(crate) fn render_diplomacy_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let (layout, view) = crate::app::input::diplomacy::sync(state).expect("active 0x73 geometry");
    super::in_game_shell::paint_dialog_nodes(state, encoder, destination, &DIALOG, |paint, ui| {
        let state = paint.state;
        let text_of = |id: i32| -> Option<String> {
            let id = u16::try_from(id).ok()?;
            if id == DiplomacyControl::PreviousPage.id() {
                return Some(crate::i18n::tr("VERA:Previous"));
            }
            if id == DiplomacyControl::NextPage.id() {
                return Some(crate::i18n::tr("VERA:Next"));
            }
            let (key, fallback) = if id == DiplomacyControl::Taunts.id() {
                TAUNTS_LABEL
            } else {
                DiplomacyHeader::ALL
                    .into_iter()
                    .find(|header| DiplomacyControl::Header(*header).id() == id)?
                    .label()
            };
            Some(state.csf_label(key, fallback))
        };
        let style = GuiStyle {
            buttons: ButtonArt::Side,
            side: Some(paint.side_buttons()),
            text_of: Some(&text_of),
            dim_disabled_checkboxes: true,
            ..Default::default()
        };
        // The paging buttons show only where there is more than one page.
        let paging = [DiplomacyControl::PreviousPage, DiplomacyControl::NextPage];
        let paging = paging.map(DiplomacyControl::id);
        paint.nodes(ui, &paging, style);
        if view.page_count > 1 {
            let first = layout.paging[0];
            let page = view.page_index + 1;
            let pages = view.page_count;
            let shown = crate::i18n::tr_fill("VERA:PageOf", &[("page", &page), ("pages", &pages)]);
            paint.text(
                &shown,
                crate::ui::shell::geom::RectPx::new(first.x, first.y - 24, first.w, 20),
                ShellAlign::H_CENTER,
            );
        }
        // The Allies and Chat headers hide as their columns do.
        let headers = DiplomacyHeader::ALL.map(|header| DiplomacyControl::Header(header).id());
        paint.nodes(ui, &headers, style);
        for row in 0..DIPLOMACY_ROWS {
            let ids = [
                DiplomacyControl::Insignia(row),
                DiplomacyControl::Name(row),
                DiplomacyControl::Kills(row),
                DiplomacyControl::Allies(row),
                DiplomacyControl::Chat(row),
            ]
            .map(|c| c.id());
            paint.nodes(ui, &ids, style);
        }
        // Taunts with its caption 26 px right of the box, vertically centred
        // (0x0061663A..0x00616674), then the mode and map statics.
        let tail = [
            DiplomacyControl::Taunts,
            DiplomacyControl::ModeName,
            DiplomacyControl::MapName,
        ]
        .map(|c| c.id());
        paint.nodes(ui, &tail, style);
    })
}
