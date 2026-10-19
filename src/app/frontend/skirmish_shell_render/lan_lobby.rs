//! BB LAN lobby composition: native5DDBD0 with ordinary shell60CF00,
//! NewEdit614B30, list619230 and scrollbar61C690. Discovery rows are UI views.

use super::chrome::*;
use super::controls::{
    ControlPaint, NewEditPaint, list_controls, paint_control, push_new_edit_instances,
};
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::text::{button_text_rect, localized_label, rect_to_text_rect};
use super::{
    SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH,
    SHELL_EDIT_CARET_DEPTH, SHELL_EDIT_FRAME_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::lan_shell::{JOIN, PAGE};
use crate::ui::shell::geom::{RectPx, ShellScale};
use std::time::Instant;

/// LB row colors carry packed00BBGGRR; -1 means the ordinary yellow text.
fn row_rgb(color: Option<u32>) -> [f32; 3] {
    match color {
        None | Some(u32::MAX) => SHELL_LABEL_TEXT_RGB,
        Some(color) => [color as u8, (color >> 8) as u8, (color >> 16) as u8]
            .map(|channel| f32::from(channel) / 255.0),
    }
}

pub(crate) fn render_lan_lobby(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    crate::app::shell_lan::update_geometry(state);
    let now = Instant::now();
    let layout = crate::app::shell_lan::layout(state);
    let scale = layout.page.scale;
    let width = state.render_width();
    let height = state.render_height();
    let exit =
        crate::app::frontend::shell_transition::shell_exit_wave(state, ShellSlideKind::LanLobby)
            .cloned();
    let leaving = exit.is_some();
    let wave = exit.or_else(|| state.frontend.shell_first_paint_slide.clone());
    let sliding = wave.is_some();
    let title = (!sliding)
        .then(|| state.frontend.shell_page_title.paint(now))
        .flatten();
    let frames = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .map_or(0, |atlas| atlas.launcher_warning_frames.len());
    let monitor = if leaving {
        None
    } else {
        state.frontend.shell_monitor.paint(now, frames, !sliding)
    };
    let status = if sliding {
        None
    } else {
        let help = state
            .frontend
            .lan
            .as_ref()
            .and_then(|runtime| runtime.lobby.buttons.hovered)
            .and_then(crate::ui::lan_shell::tooltip_key)
            .map(|key| localized_label(state, key, ""))
            .unwrap_or_default();
        crate::app::frontend::menu_page_render::paint_shell_status_line(
            state,
            help,
            layout.page.status_help,
        )
    };
    let runtime = state.frontend.lan.as_ref().expect("active BB");
    let lobby = &runtime.lobby;
    let atlas = state
        .frontend
        .skirmish_shell_chrome
        .as_ref()
        .expect("BB control art");
    let font = &state.renderer.bit_font;
    // BB's native background is precisely the ordinary MNSCRN shell family.
    let mut art =
        super::launcher_options::launcher_background_instances(atlas, width, height, scale);
    if let Some(frame) = monitor.and_then(|frame| atlas.launcher_warning_frames.get(frame)) {
        push_flag_entry_clipped_centered(
            &mut art,
            *frame,
            layout.page.warning_monitor,
            scale,
            SHELL_CONTROL_DEPTH,
        );
    }
    if let Some(wave) = &wave {
        push_slide_column(
            &mut art,
            atlas,
            &crate::ui::skirmish_shell::compute_layout_on(scale, width, height),
            &wave.button_draws(),
            SHELL_CONTROL_DEPTH,
        );
    }
    let mut controls = Vec::new();
    let mut texts = Vec::new();
    let mut overlay = ShellFrameOverlay::default();
    let text = |value: &str, rect: RectPx, align: ShellAlign, rgb: [f32; 3]| -> ShellTextDraw {
        shell_text::draw_in_rect_scaled(
            font,
            value,
            rect_to_text_rect(rect),
            rgb,
            align,
            [0.0; 2],
            SHELL_CONTROL_TEXT_DEPTH,
            scale.factor(),
        )
    };
    if !leaving {
        for (key, rect) in layout.labels {
            texts.push(text(
                &localized_label(state, key, key),
                rect,
                ShellAlign::NONE,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
        for (rect, edit) in [(layout.name, &lobby.name), (layout.chat, &lobby.chat.edit)] {
            let field = scale.derive_rect(rect, |rect| {
                RectPx::new(rect.x + 2, rect.y, rect.w - 1, rect.h + 1)
            });
            push_new_edit_instances(
                &mut controls,
                atlas,
                font,
                edit,
                NewEditPaint {
                    frame: scale.derive_rect(rect, |rect| {
                        RectPx::new(rect.x - 2, rect.y - 2, rect.w + 5, rect.h + 5)
                    }),
                    text: field,
                    background: None,
                    frame_depth: SHELL_EDIT_FRAME_DEPTH,
                    caret_depth: SHELL_EDIT_CARET_DEPTH,
                    caret_visible: lobby.enabled && lobby.caret_visible(now),
                },
                scale,
            );
            texts.push(text(
                &String::from_utf16_lossy(
                    &edit.units[edit.first_visible_unit.min(edit.units.len())..],
                ),
                field,
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
        paint_control(
            &mut controls,
            &atlas.control_chrome(),
            ControlPaint::Checkbox {
                rect: layout.observer_toggle,
                checked: lobby.observer,
            },
            scale,
        );
        if let Some(entry) = atlas.lan_observer_toggle(lobby.observer) {
            push_flag_entry_clipped_centered(
                &mut controls,
                entry,
                layout.observer_image,
                scale,
                SHELL_CONTROL_DEPTH,
            );
        }
        let games = lobby
            .game_list
            .geometry(layout.games, lobby.game_count(), scale);
        let players = lobby
            .player_list
            .geometry(layout.players, lobby.players.len(), scale);
        let chat = lobby
            .chat
            .list
            .geometry(layout.chat_log, lobby.chat.lines.len(), scale);
        list_controls(&mut controls, atlas, games, &lobby.game_list, true, scale);
        list_controls(
            &mut controls,
            atlas,
            players,
            &lobby.player_list,
            false,
            scale,
        );
        list_controls(&mut controls, atlas, chat, &lobby.chat.list, false, scale);
        for (visible, index) in (lobby.game_list.top_index..lobby.game_count())
            .take(games.visible_rows)
            .enumerate()
        {
            let label = if index == 0 {
                localized_label(state, "TXT_LOBBY", "Lobby")
            } else {
                let game = &lobby.games[index - 1];
                localized_label(state, game.label_key(), game.label_key())
                    .replacen("%s", &game.name, 1)
            };
            texts.push(text(
                &label,
                scale.derive_rect(games.row(visible), |rect| {
                    RectPx::new(rect.x + 2, rect.y, rect.w - 4, rect.h)
                }),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
        for (visible, player) in lobby
            .players
            .iter()
            .skip(lobby.player_list.top_index)
            .take(players.visible_rows)
            .enumerate()
        {
            let row = players.row(visible);
            // Active BB has no custom columns: native registry record+F8
            // is null (selected-game GUI probe).5DAFE0's4A8 color/side/status
            // writes therefore return-1 in618D40; only the name is visible.
            texts.push(text(
                &player.name,
                scale.derive_rect(row, |rect| {
                    RectPx::new(rect.x + 2, rect.y, rect.w - 4, rect.h)
                }),
                ShellAlign::V_CENTER,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
        for (visible, line) in lobby
            .chat
            .lines
            .iter()
            .skip(lobby.chat.list.top_index)
            .take(chat.visible_rows)
            .enumerate()
        {
            texts.push(text(
                &String::from_utf16_lossy(&line.text),
                scale.derive_rect(chat.row(visible), |rect| {
                    RectPx::new(rect.x + 2, rect.y, rect.w - 4, rect.h)
                }),
                ShellAlign::V_CENTER,
                row_rgb(Some(line.color)),
            ));
        }
        if let Some(reveal) = title {
            texts.push(shell_text::draw_in_rect_path_a_scaled(
                font,
                &localized_label(state, PAGE.title_key, "Lobby"),
                rect_to_text_rect(layout.page.title),
                ShellAlign::H_CENTER,
                [0.0; 2],
                SHELL_CONTROL_TEXT_DEPTH,
                PathAReveal {
                    count: reveal.count,
                    range: reveal.range,
                    base_rgb: [255, 255, 0],
                    highlight_rgb: [255; 3],
                },
                scale.factor(),
            ));
        }
        if !sliding {
            for button in &layout.page.buttons {
                let pressed = lobby.buttons.is_pressed(button.id);
                push_right_panel_button_shp(
                    &mut art,
                    atlas,
                    button.rect,
                    pressed,
                    false,
                    SHELL_CONTROL_DEPTH,
                );
                if let Some(spec) = PAGE.button(button.id) {
                    let key = if button.id == JOIN && lobby.observer {
                        "GUI:Observe"
                    } else {
                        spec.csf_key
                    };
                    texts.push(shell_text::draw_in_rect_scaled(
                        font,
                        &localized_label(state, key, key),
                        button_text_rect(button.rect, pressed, scale),
                        if lobby.enabled {
                            SHELL_LABEL_TEXT_RGB
                        } else {
                            crate::render::shell_paint::SHELL_TEXT_RGB_DISABLED
                        },
                        ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                        [0.0; 2],
                        SHELL_CONTROL_TEXT_DEPTH,
                        scale.factor(),
                    ));
                }
            }
        }
        texts.extend(crate::render::shell_paint::paint_labels_scaled(
            font,
            &status.into_iter().collect::<Vec<_>>(),
            scale.factor(),
        ));
        if let Some(key) = runtime.modal {
            let modal = crate::ui::shell::modal::body_ok_layout(width as i32, height as i32);
            let pressed = lobby.buttons.is_pressed(1);
            super::modals::push_validation_modal_instances(
                &mut overlay.controls,
                atlas,
                &modal,
                pressed,
            );
            overlay.texts.push(shell_text::draw_in_rect(
                font,
                &localized_label(state, key, key),
                rect_to_text_rect(modal.body),
                SHELL_LABEL_TEXT_RGB,
                super::text::validation_modal_body_text_align(),
                [0.0; 2],
                SHELL_DROPDOWN_TEXT_DEPTH - 0.00012,
            ));
            overlay.texts.push(shell_text::draw_in_rect(
                font,
                &localized_label(state, "TXT_OK", "OK"),
                button_text_rect(modal.ok, pressed, ShellScale::RETAIL),
                SHELL_LABEL_TEXT_RGB,
                ShellAlign::H_CENTER | ShellAlign::V_CENTER,
                [0.0; 2],
                SHELL_DROPDOWN_TEXT_DEPTH - 0.00013,
            ));
        }
    }
    in_game_shell::render_shell_frame(
        state,
        encoder,
        destination,
        InGameShellFrame {
            art,
            controls,
            texts,
            label: "LAN Lobby BB",
        },
        ShellFrameArt::Launcher,
        overlay,
        ShellFrameCursor::Visible,
    )?;
    state.platform.window.request_redraw();
    Ok(())
}
