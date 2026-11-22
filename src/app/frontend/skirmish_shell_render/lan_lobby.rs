//! BB LAN lobby composition: native5DDBD0 with ordinary shell60CF00,
//! NewEdit614B30, list619230 and scrollbar61C690. Discovery rows are UI views.

use super::chrome::*;
use super::in_game_shell::{
    self, InGameShellFrame, ShellFrameArt, ShellFrameCursor, ShellFrameOverlay,
};
use super::text::rect_to_text_rect;
use super::{SHELL_CONTROL_DEPTH, SHELL_CONTROL_TEXT_DEPTH, SHELL_LABEL_TEXT_RGB};
use crate::app::AppState;
use crate::app::frontend::shell_transition::ShellSlideKind;
use crate::render::shell_paint;
use crate::render::shell_text::{self, ShellTextDraw};
use crate::render::shell_text_reveal::PathAReveal;
use crate::ui::lan_shell::{CHAT_LOG, GAMES, JOIN, PAGE, PLAYERS};
use crate::ui::shell::geom::{RectPx, ShellAlign};
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
    let layout = crate::app::shell_lan::create_box_layout(state);
    overlay.controls.extend(super::wol_lobby::online_box(
        atlas,
        layout.dialog,
        layout.scale,
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
    let style = super::gui_paint::GuiStyle::popup();
    let widgets =
        super::gui_paint::paint_gui_ids(state, controller.gui(), root, ids, layout.scale, style);
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
            .map(|key| state.csf_label(key, ""))
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
    if !leaving {
        for (id, key) in crate::ui::lan_shell::LABELS {
            texts.push(text(
                &state.csf_label(key, key),
                layout.rect(id).unwrap_or_default(),
                ShellAlign::NONE,
                SHELL_LABEL_TEXT_RGB,
            ));
        }
        let observer = state.frontend.multiplayer_shell_runtime.lan_observer();
        // The column's buttons (left to the slide while it runs), the lists
        // with their rows as their widgets hold them and the observer box.
        let rows_of = |id: i32| -> Option<super::gui_paint::HostRows> {
            let yellow = |text: String| (text, u32::MAX);
            Some(match u16::try_from(id).ok()? {
                GAMES => (0..lobby.game_count())
                    .map(|index| match index.checked_sub(1) {
                        None => yellow(state.csf_label("TXT_LOBBY", "Lobby")),
                        Some(game) => {
                            let game = &lobby.games[game];
                            let format = state.csf_label(game.label_key(), game.label_key());
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
        // The column's captions; Join reads Observe for an observer.
        let text_of = |id: i32| {
            let id = u16::try_from(id).ok()?;
            let key = match id {
                JOIN if observer => "GUI:Observe",
                id => PAGE.button(id)?.csf_key,
            };
            Some(state.csf_label(key, key))
        };
        let enabled_of = |_| Some(lobby.enabled);
        // An edit's caret blinks while it has BB's focus, under the message
        // box too.
        let controller = &state.frontend.shell_controller;
        let caret_of = |node| lobby.enabled && controller.caret_visible_in(PAGE.dialog, node);
        let style = super::gui_paint::GuiStyle {
            caret_of: Some(&caret_of),
            buttons: match sliding {
                true => super::gui_paint::ButtonArt::Host,
                false => super::gui_paint::ButtonArt::Panel,
            },
            text_of: Some(&text_of),
            enabled_of: Some(&enabled_of),
            rows_of: Some(&rows_of),
            control_lists: true,
            ..Default::default()
        };
        if let Some(root) = root {
            // The observer image paints as its art below, not as a box.
            let skip = [crate::ui::lan_shell::OBSERVER_IMAGE];
            let ids = super::gui_paint::child_ids_except(gui, root, &skip);
            let paint = super::gui_paint::paint_gui_ids(state, gui, root, ids, scale, style);
            controls.extend(paint.controls);
            texts.extend(paint.texts);
        }
        if let Some(entry) = atlas.lan_observer_toggle(observer) {
            push_flag_entry_clipped_centered(
                &mut controls,
                entry,
                layout
                    .rect(crate::ui::lan_shell::OBSERVER_IMAGE)
                    .unwrap_or_default(),
                scale,
                SHELL_CONTROL_DEPTH,
            );
        }
        if let Some(reveal) = title {
            texts.push(shell_text::draw_in_rect_path_a_scaled(
                font,
                &state.csf_label(PAGE.title_key, "Lobby"),
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
        texts.extend(shell_paint::paint_labels_scaled(
            font,
            &status.into_iter().collect::<Vec<_>>(),
            scale.factor(),
        ));
        if let Some(key) = runtime.modal {
            use crate::app::shell_lan::{MODAL, OK};
            let body = runtime
                .modal_body
                .clone()
                .unwrap_or_else(|| state.csf_label(key, key));
            let ok = state.csf_label("TXT_OK", "OK");
            let notice = crate::ui::shell::modal::ShellBox::new(MODAL, body).button(OK, ok);
            let pressed = box_pressed(state, OK.into());
            let look = super::BoxLook::skirmish(atlas.mode_two_dialog_background(None));
            let (sprites, texts) = (
                Some(&mut overlay.controls),
                Some((&mut overlay.texts, false)),
            );
            let size = (width as i32, height as i32);
            let pressed = |id| pressed && id == OK;
            super::shell_box::push_skirmish_box(
                state,
                Some(atlas),
                size,
                &notice,
                pressed,
                look,
                sprites,
                texts,
            );
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
