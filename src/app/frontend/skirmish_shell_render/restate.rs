//! Mission restate briefing 0x10C over the active-game shell, as Diplomacy
//! 0x73: title, Return and status line in the right-panel places, and the
//! briefing as a plain left/top static wrapped and clipped to its rect
//! (`OwnerDraw_Static` 0x006153E0).

use crate::app::AppState;
use crate::app::input::restate::DIALOG;
use crate::ui::shell::geom::ShellAlign;
use crate::ui::shell::restate::restate_layout;

pub(crate) fn render_restate_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    super::in_game_shell::paint_dialog(state, encoder, destination, &DIALOG, |paint, _| {
        let layout = paint.shell.place(restate_layout);
        // The briefing 0x6E7 is SS_LEFT, top aligned.
        let state = paint.state;
        let briefing = &state.match_state.match_presentation.restate_text;
        paint.text(briefing, layout.content, ShellAlign::NONE);
    })
}
