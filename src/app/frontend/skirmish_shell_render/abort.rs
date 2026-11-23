//! Full-screen B6: campaign mode0 (Restart712 shown), Skirmish mode5
//! (712 hidden) and LAN/WOL (Observe712 or hidden). Original4F1840/4F18B0
//! and 60C540→621FB1→72F540; no modal
//! card and no title694.
//!
//! Its buttons and question paint from its `vera20k-gui` nodes
//! ([`super::gui_paint`]), made current first every frame.

use super::gui_paint::{ButtonArt, GuiStyle};
use crate::app::AppState;
use crate::app::input::abort::DIALOG;
use crate::ui::shell::abort::{AbortButton, QUESTION, QUESTION_LABEL};

pub(crate) fn render_abort_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let layout = crate::app::input::abort::sync(state).expect("active B6 geometry");
    let secondary = layout.secondary_kind;
    super::in_game_shell::paint_dialog(state, encoder, destination, &DIALOG, |paint, ui| {
        let state = paint.state;
        let text_of = |id: i32| -> Option<String> {
            let id = u16::try_from(id).ok()?;
            let (key, fallback) = match AbortButton::from_id(id) {
                Some(button) => button.label(secondary),
                None if id == QUESTION => QUESTION_LABEL,
                None => return None,
            };
            Some(state.csf_label(key, fallback))
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
        paint.nodes(ui, &ids, style);
    })
}
