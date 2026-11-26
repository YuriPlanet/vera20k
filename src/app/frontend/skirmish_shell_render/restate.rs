//! Mission restate briefing 0x10C over the active-game shell, as Diplomacy
//! 0x73: title, Return and status line in the right-panel places, and the
//! briefing as a plain left/top static wrapped and clipped to its rect
//! (`OwnerDraw_Static` 0x006153E0).
//!
//! Return and the briefing paint from 0x10C's `vera20k-gui` nodes
//! ([`super::gui_paint`]), made current first every frame.

use super::gui_paint::GuiStyle;
use crate::app::AppState;
use crate::app::input::restate::DIALOG;
use crate::ui::shell::restate::RestateControl;

pub(crate) fn render_restate_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    crate::app::input::restate::sync(state).expect("active 0x10C geometry");
    super::in_game_shell::paint_dialog(state, encoder, destination, &DIALOG, |paint, ui| {
        let content = RestateControl::Content.id();
        // The briefing 0x6E7 is SS_LEFT, top aligned (its node's `Align`),
        // its text the node's own (`input::restate::sync`), borrowed.
        paint.nodes(ui, &[content], GuiStyle::default());
    })
}
