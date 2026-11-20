//! BB LAN lobby composition: native5DDBD0 with ordinary shell60CF00,
//! NewEdit614B30, list619230 and scrollbar61C690. Discovery rows are UI views.

use super::chrome::*;
use super::controls::push_new_edit;
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::text::{button_text_rect, localized_label, rect_to_text_rect};
use super::{
    SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_DROPDOWN_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB,
};
use crate::app::AppState;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::shell_paint;
use crate::render::shell_text::{self, ShellAlign, ShellTextDraw};
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::lan_shell::{CHAT_LOG, GAMES, JOIN, PAGE, PLAYERS};
use crate::ui::shell::geom::{RectPx, ShellScale};
use crate::util::time::Instant;

/// Whether box button `id` shows pushed (612B70).
fn box_pressed(state: &AppState, id: i32) -> bool {
    let gui = state.frontend.shell_controller.gui();
    gui.dialogs()
        .last()
        .and_then(|d| gui.find_id(gui.dialog_root(d)?, id))
        .is_some_and(|node| gui.shows_pressed(node))
}

/// VERA's Create Game box (`LanShellRuntime::creating`): the online box's
/// art, then its buttons and its room's limits as its widgets hold them.
fn paint_create_box(
    state: &AppState,
    atlas: &crate::render::skirmish_shell_chrome::SkirmishShellChromeAtlas,
    overlay: &mut ShellFrameOverlay,
) {
    use crate::app::shell_lan::{
        CANCEL, MODAL, OK, ROOM_PLAYERS, ROOM_PLAYERS_CAPTION, ROOM_SPECTATORS,
    };
    let (width, height) = (state.render_width() as i32, state.render_height() as i32);
    let dialog = crate::ui::shell::modal::lan_create_game_layout(width, height).dialog;
    overlay.controls.extend(super::wol_lobby::online_box(
        atlas,
        dialog,
        ShellScale::RETAIL,
    ));
    let controller = &state.frontend.shell_controller;
    let Some(root) = controller.root_in(MODAL) else {
        return;
    };
    let ids = [
        OK,
        CANCEL,
        ROOM_PLAYERS,
        ROOM_PLAYERS_CAPTION,
        ROOM_SPECTATORS,
    ];
    let style = super::gui_paint::GuiStyle::popup(atlas);
    let widgets = super::gui_paint::paint_gui_ids(
        state,
        controller.gui(),
        root,
        ids,
        ShellScale::RETAIL,
        style,
    );
    overlay.controls.extend(widgets.controls);
    overlay.texts.extend(widgets.texts);
}

pub(crate) fn render_lan_lobby(
    state: &mut AppState,
    encoder: &mut wgpu::CommandEncoder,
    destination: &wgpu::Texture,
) -> anyhow::Result<()> {
    crate::app::shell_lan::sync_gui(state);
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
        let gui = state.frontend.shell_controller.gui();
        let help = crate::ui::shell::page::hovered_key(gui)
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
    // VERA's Create Game box shows over the bare backdrop, as the online
    // lobby's does.
    let gui = state.frontend.shell_controller.gui();
    let root = state.frontend.shell_controller.root_in(PAGE.dialog);
    if !leaving && !runtime.creating {
        for (key, rect) in layout.labels {
            texts.push(text(
                &localized_label(state, key, key),
                rect,
                ShellAlign::NONE,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
        let chat_composition = state
            .platform
            .ime
            .composition(crate::app::input::ime::ImeTarget::LanLobbyChat);
        for (rect, edit, composition) in [
            (layout.name, &lobby.name, None),
            (layout.chat, &lobby.chat.edit, chat_composition),
        ] {
            push_new_edit(
                &mut controls,
                &mut texts,
                atlas,
                font,
                edit,
                composition,
                rect,
                lobby.enabled && lobby.caret_visible(now),
                SHELL_LABEL_TEXT_RGB,
                SHELL_CONTROL_TEXT_DEPTH,
                scale,
            );
        }
        let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
        if let Some(root) = root {
            controls.extend(super::gui_paint::paint_gui_children(state, gui, root, scale).controls);
        }
        if let Some(entry) = atlas.lan_observer_toggle(observer) {
            push_flag_entry_clipped_centered(
                &mut controls,
                entry,
                layout.observer_image,
                scale,
                SHELL_CONTROL_DEPTH,
            );
        }
        // The lists with their rows, as their widgets hold them.
        let rows_of = |id: i32| -> Option<super::gui_paint::HostRows> {
            let yellow = |text: String| (text, u32::MAX);
            Some(match u16::try_from(id).ok()? {
                GAMES => (0..lobby.game_count())
                    .map(|index| match index.checked_sub(1) {
                        None => yellow(localized_label(state, "TXT_LOBBY", "Lobby")),
                        Some(game) => {
                            let game = &lobby.games[game];
                            let format = localized_label(state, game.label_key(), game.label_key());
                            yellow(format.replacen("%s", &game.name, 1))
                        }
                    })
                    .collect(),
                // Active BB has no custom columns: native registry record+F8
                // is null (selected-game GUI probe).5DAFE0's4A8 color/side/status
                // writes therefore return-1 in618D40; only the name is visible.
                PLAYERS => lobby
                    .players
                    .iter()
                    .map(|player| yellow(player.name.clone()))
                    .collect(),
                CHAT_LOG => {
                    let line = |line: &crate::ui::lan_shell::LanChatLine| {
                        (String::from_utf16_lossy(&line.text), line.color)
                    };
                    lobby.chat.lines.iter().map(line).collect()
                }
                _ => return None,
            })
        };
        let style = super::gui_paint::GuiStyle {
            rows_of: Some(&rows_of),
            control_lists: true,
            ..Default::default()
        };
        for id in [GAMES, PLAYERS, CHAT_LOG] {
            if let Some(list) = root.and_then(|root| gui.find_id(root, id.into())) {
                let paint = super::gui_paint::paint_gui_styled(state, gui, list, scale, &[], style);
                controls.extend(paint.controls);
                texts.extend(paint.texts);
            }
        }
        if let Some(reveal) = title {
            texts.push(shell_text::draw_in_rect_path_a_scaled(
                font,
                &localized_label(state, PAGE.title_key, "Lobby"),
                rect_to_text_rect(layout.page.title),
                ShellAlign::PANEL_TITLE,
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
            let looks = crate::ui::shell::page::menu_button_looks(gui, root, &layout.page.buttons);
            for (button, pressed, _) in &looks {
                let pressed = *pressed;
                push_right_panel_button_shp(
                    &mut art,
                    atlas,
                    button.rect,
                    pressed,
                    false,
                    SHELL_CONTROL_DEPTH,
                );
                if let Some(spec) = PAGE.button(button.id) {
                    let key = if button.id == JOIN && observer {
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
                            shell_paint::SHELL_TEXT_RGB_DISABLED
                        },
                        ShellAlign::BUTTON_CAPTION,
                        [0.0; 2],
                        SHELL_CONTROL_TEXT_DEPTH,
                        scale.factor(),
                    ));
                }
            }
        }
        texts.extend(shell_paint::paint_labels_scaled(
            font,
            &status.into_iter().collect::<Vec<_>>(),
            scale.factor(),
        ));
        if let Some(key) = runtime.modal {
            let modal = crate::ui::shell::modal::body_ok_layout(width as i32, height as i32);
            let pressed = box_pressed(state, 1);
            super::modals::push_validation_modal_instances(
                &mut overlay.controls,
                atlas,
                &modal,
                pressed,
            );
            overlay.texts.push(shell_text::draw_in_rect(
                font,
                &runtime.modal_body.clone().unwrap_or_else(|| {
                    crate::i18n::label(state.process_assets.csf.as_ref(), key, key)
                }),
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
    if runtime.creating {
        paint_create_box(state, atlas, &mut overlay);
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
