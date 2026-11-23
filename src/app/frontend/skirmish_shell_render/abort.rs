//! Full-screen B6: campaign mode0 (Restart712 shown), Skirmish mode5
//! (712 hidden) and LAN/WOL (Observe712 or hidden). Original4F1840/4F18B0
//! and 60C540→621FB1→72F540; no modal
//! card and no title694.

use crate::app::AppState;
use crate::app::input::abort::{DIALOG, current_abort_layout};
use crate::ui::shell::abort::AbortButton;
use crate::ui::shell::geom::ShellAlign;

pub(crate) fn render_abort_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    super::in_game_shell::paint_dialog(state, encoder, destination, &DIALOG, |paint, ui| {
        let layout = current_abort_layout(paint.state).expect("active B6 geometry");
        // Resume 686 is the lower button.
        for (button, rect) in layout.buttons().filter(|(b, _)| *b != AbortButton::Resume) {
            let caption = paint.label(button.label(layout.secondary_kind));
            paint.button(
                rect,
                &caption,
                crate::app::input::dialog_page::pressed(ui, button.id()),
            );
        }
        // Plain static615A81..615AE8 uses low style bits1 for horizontal center;
        // it does not apply SS_CENTERIMAGE's vertical centering flag.
        let question = paint.label(("GUI:AskAbortMission", "What would you like to do?"));
        paint.text(&question, layout.question, ShellAlign::H_CENTER);
    })
}
