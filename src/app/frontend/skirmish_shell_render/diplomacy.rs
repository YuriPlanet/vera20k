//! In-battle Diplomacy 0x73 over the active-game shell (621FB1 -> 72F540):
//! title, Return and status line in the right-panel places, the rows and
//! headers as ordinary children.

use super::SHELL_CONTROL_DEPTH;
use super::chrome::push_flag_entry_clipped_centered;
use super::controls::{ControlPaint, paint_control};
use crate::app::AppState;
use crate::app::input::dialog_page::pressed;
use crate::app::input::diplomacy::DIALOG;
use crate::ui::shell::checkbox::CheckboxFace;
use crate::ui::shell::diplomacy::{DiplomacyControl, DiplomacyHeader, diplomacy_layout};
use crate::ui::shell::geom::ShellAlign;

fn face(checked: bool) -> CheckboxFace {
    if checked {
        CheckboxFace::Checked
    } else {
        CheckboxFace::Unchecked
    }
}

pub(crate) fn render_diplomacy_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    super::in_game_shell::paint_dialog(state, encoder, destination, &DIALOG, |paint, ui| {
        let state = paint.state;
        let layout = paint.shell.place(diplomacy_layout);
        let view = crate::app::input::diplomacy::view(state);
        if view.page_count > 1 {
            let held = [DiplomacyControl::PreviousPage, DiplomacyControl::NextPage]
                .map(|c| pressed(ui, c.id()));
            paint.paging(layout.paging, held, view.page_index + 1, view.page_count);
        }
        // Title 0x694 is SS_CENTER; every other static is left-aligned.
        for header in DiplomacyHeader::ALL {
            let shown = match header {
                DiplomacyHeader::Allies => view.allies_header,
                DiplomacyHeader::Chat => view.chat_header,
                DiplomacyHeader::Player | DiplomacyHeader::Kills => true,
            };
            if shown {
                let caption = paint.label(header.label());
                paint.text(&caption, layout.headers[header as usize], ShellAlign::NONE);
            }
        }
        let chrome = state.frontend.skirmish_shell_chrome.as_ref();
        let control_chrome = chrome
            .map(|atlas| atlas.control_chrome())
            .unwrap_or_default();
        let checkbox = |controls: &mut Vec<_>, face, enabled: bool, rect| {
            let control = ControlPaint::StatefulCheckbox {
                face,
                disabled: !enabled,
                rect,
            };
            paint_control(
                controls,
                &control_chrome,
                control,
                crate::ui::shell::geom::RETAIL_SCALE,
            );
        };
        for (row, shown) in view.rows.iter().enumerate() {
            let Some(shown) = shown else {
                continue;
            };
            let insignia = shown.insignia.as_deref();
            if let Some(entry) = insignia.and_then(|name| chrome?.lan_player_side(name)) {
                let rect = layout.insignia[row];
                push_flag_entry_clipped_centered(
                    &mut paint.controls,
                    entry,
                    rect,
                    crate::ui::shell::geom::RETAIL_SCALE,
                    SHELL_CONTROL_DEPTH,
                );
            }
            let rgb = shown.rgb.map(|unit| f32::from(unit) / 255.0);
            paint.text_rgb(&shown.name, layout.names[row], rgb, ShellAlign::NONE);
            if let Some(kills) = shown.kills {
                paint.text_rgb(&kills.to_string(), layout.kills[row], rgb, ShellAlign::NONE);
            }
            if let Some(allies) = shown.allies {
                checkbox(
                    &mut paint.controls,
                    allies.face(),
                    allies.enabled,
                    layout.allies[row],
                );
            }
            if let Some(chat) = shown.chat {
                checkbox(
                    &mut paint.controls,
                    face(chat.checked),
                    chat.enabled,
                    layout.chat[row],
                );
            }
        }
        if let Some(checked) = view.taunts {
            checkbox(&mut paint.controls, face(checked), true, layout.taunts);
            // The checkbox proc draws its caption 26 px right of the box,
            // vertically centred (0x0061663A..0x00616674).
            let caption = paint.label(("GUI:TauntsOn", "Taunts On"));
            let at = crate::ui::skirmish_shell::checkbox_text_rect(
                layout.taunts,
                crate::ui::shell::geom::RETAIL_SCALE,
            );
            paint.text(&caption, at, ShellAlign::V_CENTER);
        }
        if let Some(mode_name) = &view.mode_name {
            paint.text(mode_name, layout.mode_name, ShellAlign::NONE);
        }
        paint.text(&view.map_name, layout.map_name, ShellAlign::NONE);
    })
}
