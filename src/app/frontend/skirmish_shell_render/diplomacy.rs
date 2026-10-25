//! In-battle Diplomacy 0x73 over the active-game shell (621FB1 -> 72F540):
//! title, Return and status line in the right-panel places, the rows and
//! headers as ordinary children.

use super::chrome::push_flag_entry_clipped_centered;
use super::controls::{ControlPaint, paint_control};
use super::in_game_shell::{self, InGameShellFrame};
use super::pause_menu::{button_frame, button_text_rect};
use super::text::{localized_label, push_text_draw, rect_to_text_rect};
use super::{SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::render::shell_text::ShellAlign;
use crate::ui::shell::button::CheckboxFace;
use crate::ui::shell::diplomacy::{
    DiplomacyButton, DiplomacyHeader, RETURN_LABEL, TITLE_LABEL, diplomacy_layout,
};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::ui::shell::pause_menu::PauseMenuButtonState;

pub(crate) fn render_diplomacy_shell(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    let in_game_shell::InGameShellGeometry {
        width,
        height,
        layout: shell,
        button_size,
    } = in_game_shell::current_in_game_shell_layout(state).expect("active 0x73 geometry");
    let layout = diplomacy_layout(width, height, shell, button_size);
    let view = crate::app::input::diplomacy::view(state);
    let interaction = state.match_state.match_presentation.diplomacy;
    let screen = RectPx::new(0, 0, width, height);
    let atlas = crate::app::presentation::sidebar_render::current_sidebar_chrome(state)
        .expect("active 0x73 art");
    let mut art = in_game_shell::background_instances(atlas, shell, width, height);
    let mut controls = Vec::new();
    let mut texts = Vec::new();
    let text = |texts: &mut Vec<_>, label: &str, rect: RectPx, rgb: [f32; 3], align| {
        push_text_draw(
            texts,
            state,
            label,
            rect_to_text_rect(rect),
            rgb,
            align,
            SHELL_CONTROL_TEXT_DEPTH,
            1.0,
        );
    };

    let pressed = interaction.button.is_pressed(DiplomacyButton::Return);
    let frame = button_frame(PauseMenuButtonState {
        pressed,
        ..Default::default()
    });
    if let Some(entry) = atlas.in_game_shell.buttons[frame].or(atlas.in_game_shell.buttons[0]) {
        in_game_shell::push_art(&mut art, entry, layout.return_button, screen);
    }
    text(
        &mut texts,
        &localized_label(state, RETURN_LABEL.0, RETURN_LABEL.1),
        button_text_rect(layout.return_button, pressed),
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
    );
    if view.page_count > 1 {
        for (index, button, label) in [
            (0, DiplomacyButton::PreviousPage, "Previous"),
            (1, DiplomacyButton::NextPage, "Next"),
        ] {
            let pressed = interaction.button.is_pressed(button);
            let frame = button_frame(PauseMenuButtonState {
                pressed,
                ..Default::default()
            });
            if let Some(entry) =
                atlas.in_game_shell.buttons[frame].or(atlas.in_game_shell.buttons[0])
            {
                in_game_shell::push_art(&mut art, entry, layout.paging[index], screen);
            }
            text(
                &mut texts,
                label,
                button_text_rect(layout.paging[index], pressed),
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
            );
        }
        let first = layout.paging[0];
        text(
            &mut texts,
            &format!("Page {}/{}", view.page_index + 1, view.page_count),
            RectPx::new(first.x, first.y - 24, first.w, 20),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::H_CENTER,
        );
    }
    // Title 0x694 is SS_CENTER; every other static is left-aligned.
    text(
        &mut texts,
        &localized_label(state, TITLE_LABEL.0, TITLE_LABEL.1),
        layout.title,
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::H_CENTER,
    );
    for header in DiplomacyHeader::ALL {
        let shown = match header {
            DiplomacyHeader::Allies => view.allies_header,
            DiplomacyHeader::Chat => view.chat_header,
            DiplomacyHeader::Player | DiplomacyHeader::Kills => true,
        };
        if shown {
            let (key, fallback) = header.label();
            text(
                &mut texts,
                &localized_label(state, key, fallback),
                layout.headers[header as usize],
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::NONE,
            );
        }
    }

    let chrome = state.frontend.skirmish_shell_chrome.as_ref();
    let control_chrome = chrome
        .map(|atlas| atlas.control_chrome())
        .unwrap_or_default();
    for (row, shown) in view.rows.iter().enumerate() {
        let Some(shown) = shown else {
            continue;
        };
        if let Some(entry) = shown
            .insignia
            .and_then(|name| chrome.and_then(|atlas| atlas.lan_player_side(name)))
        {
            push_flag_entry_clipped_centered(
                &mut controls,
                entry,
                layout.insignia[row],
                ShellScale::RETAIL,
                SHELL_CONTROL_DEPTH,
            );
        }
        let rgb = shown.rgb.map(|unit| f32::from(unit) / 255.0);
        text(
            &mut texts,
            &shown.name,
            layout.names[row],
            rgb,
            ShellAlign::NONE,
        );
        if let Some(kills) = shown.kills {
            text(
                &mut texts,
                &kills.to_string(),
                layout.kills[row],
                rgb,
                ShellAlign::NONE,
            );
        }
        let boxes = [
            shown
                .allies
                .map(|allies| (allies.face(), allies.enabled, layout.allies[row])),
            shown.chat.map(|chat| {
                let face = if chat.checked {
                    CheckboxFace::Checked
                } else {
                    CheckboxFace::Unchecked
                };
                (face, chat.enabled, layout.chat[row])
            }),
        ];
        for (face, enabled, rect) in boxes.into_iter().flatten() {
            paint_control(
                &mut controls,
                &control_chrome,
                ControlPaint::StatefulCheckbox {
                    face,
                    disabled: !enabled,
                    rect,
                },
                ShellScale::RETAIL,
            );
        }
    }
    if let Some(checked) = view.taunts {
        paint_control(
            &mut controls,
            &control_chrome,
            ControlPaint::StatefulCheckbox {
                face: if checked {
                    CheckboxFace::Checked
                } else {
                    CheckboxFace::Unchecked
                },
                disabled: false,
                rect: layout.taunts,
            },
            ShellScale::RETAIL,
        );
        // The checkbox proc draws its caption 26 px right of the box,
        // vertically centred (0x0061663A..0x00616674).
        text(
            &mut texts,
            &localized_label(state, "GUI:TauntsOn", "Taunts On"),
            crate::ui::skirmish_shell::checkbox_text_rect(layout.taunts, ShellScale::RETAIL),
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::V_CENTER,
        );
    }
    if let Some(mode_name) = &view.mode_name {
        text(
            &mut texts,
            mode_name,
            layout.mode_name,
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::NONE,
        );
    }
    text(
        &mut texts,
        &view.map_name,
        layout.map_name,
        SHELL_LABEL_TEXT_RGB,
        ShellAlign::NONE,
    );
    if let Some(key) = interaction.hovered.and_then(|control| control.help_key()) {
        text(
            &mut texts,
            &localized_label(state, key, ""),
            layout.status_line,
            SHELL_LABEL_TEXT_RGB,
            ShellAlign::NONE,
        );
    }
    in_game_shell::render_in_game_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "Active Diplomacy Shell 73",
        },
    )
}
