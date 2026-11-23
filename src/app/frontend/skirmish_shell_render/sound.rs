//! Full-screen B8, original6B6300 and shared native shell/control owners.
//!
//! Its trackbars, boxes, list, Play/Stop (MNBTTN, type-2 captions) and
//! slider captions paint from its `vera20k-gui` nodes ([`super::gui_paint`]),
//! made current first every frame.

use super::gui_paint::{ButtonArt, GuiStyle};
use crate::app::AppState;
use crate::app::input::sound::DIALOG;
use crate::ui::shell::sound;

pub(crate) fn render_sound_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    crate::app::input::sound::sync(state).expect("active B8 geometry");
    super::in_game_shell::paint_dialog(state, encoder, destination, &DIALOG, |paint, ui| {
        let state = paint.state;
        let text_of = |id: i32| -> Option<String> {
            let (key, fallback) = sound::caption(u16::try_from(id).ok()?)?;
            Some(state.csf_label(key, fallback))
        };
        // 609FC7..609FE2/60A330 selects type3. MNBTTN uses native canvas
        // centered on the resource control, not the side column's SIDEBTTN.
        let style = GuiStyle {
            buttons: ButtonArt::ModalType2,
            text_of: Some(&text_of),
            ..Default::default()
        };
        paint.nodes(ui, &sound::PAINT_ORDER, style);
    })
}
