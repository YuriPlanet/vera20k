//! BC/BD/C2/C9 window input adapter. The dialog produces intents;
//! MatchSession and the setup lifecycle adapter decide admission and effects.

use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::lan_shell::LanEditKey;
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::multiplayer_setup::*;
use crate::ui::shell::trackbar::TrackbarPress;
use crate::util::time::Instant;
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{KeyCode, PhysicalKey};

pub(crate) fn layout(state: &AppState) -> MultiplayerSetupLayout {
    let (w, h) = shell_layout::shell_screen_size(state);
    let mut layout = MultiplayerSetupLayout::new(
        crate::app::shell_network_setup::dialog(state),
        w as i32,
        h as i32,
        shell_layout::fills_window(state),
    );
    if state
        .match_state
        .session
        .network_setup_capacity()
        .is_some_and(|rows| rows > 8)
    {
        layout.arrange_extended_rows();
    }
    layout
}

pub(crate) fn update_geometry(state: &mut AppState) {
    let layout = layout(state);
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return;
    };
    ui.chat
        .list
        .reconcile_geometry(layout.chat_log, ui.chat.lines.len(), layout.page.scale);
    ui.chat.edit.scroll_to_caret(
        (layout.page.scale.base_rect(layout.chat).w - 3).max(0) as u32,
        |text| state.renderer.bit_font.text_width(text),
    );
}

fn notice_active(state: &AppState) -> bool {
    state
        .frontend
        .multiplayer_setup
        .as_ref()
        .is_some_and(|ui| ui.notice.is_some())
}

fn transfer_active(state: &AppState) -> bool {
    state
        .frontend
        .network_setup_protocol
        .as_ref()
        .is_some_and(|protocol| {
            protocol.blocking_map_transfer() && protocol.transfer_progress().is_some()
        })
}

fn notice_feed(state: &AppState) -> [crate::ui::shell::layout::LaidOutControl; 1] {
    let layout = crate::ui::shell::modal::body_ok_layout(
        state.render_width() as i32,
        state.render_height() as i32,
    );
    [crate::ui::shell::layout::LaidOutControl {
        id: crate::ui::shell::modal::control::OK,
        rect: layout.ok,
    }]
}

fn activate_notice(state: &mut AppState) {
    // The native 5D3490 message box owns all input while its CE runs.
    state
        .frontend
        .shell_controller
        .ensure_active(crate::ui::shell::descriptor::DialogId(0xCE), true);
}

pub(crate) fn key(state: &mut AppState, event: &KeyEvent) {
    if !event.state.is_pressed() || shell_transition::blocks_shell_input(state) {
        return;
    }
    let code = if let PhysicalKey::Code(code) = event.physical_key {
        Some(code)
    } else {
        None
    };
    key_press(state, code, event.text.as_deref());
}

/// The chat NewEdit while it takes keys, for the input method
/// (`app::input::ime`): [`key_press`] reaches the focused chat with no
/// transfer, verify box, page dialog, notice or ChooseMap over the dialog.
pub(crate) fn ime_chat_rect(state: &AppState) -> Option<crate::ui::shell::geom::RectPx> {
    let routed = state.frontend.screen == crate::ui::game_screen::GameScreen::MainMenu
        && state.frontend.shell_route.network_setup()
        && state
            .frontend
            .skirmish_shell_state
            .choose_map_modal
            .is_none()
        && !shell_transition::blocks_shell_input(state)
        && !transfer_active(state)
        && crate::app::shell_network_setup::verify(state).is_none()
        && !crate::app::shell_wol::page::open(state)
        && !notice_active(state)
        && crate::app::shell_network_setup::view(state).is_some();
    state
        .frontend
        .multiplayer_setup
        .as_ref()
        .filter(|ui| routed && ui.focused_control == CHAT)
        .map(|_| layout(state).chat)
}

pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    if transfer_active(state) {
        return;
    }
    // WOL Start Game Now's 0x130 has no key handling; the locked C2/C9
    // under it takes none either.
    if crate::app::shell_network_setup::verify(state).is_some() {
        return;
    }
    // The page dialog 0xCF runs its own modal loop over C2/C9.
    if crate::app::shell_wol::page::open(state) {
        crate::app::shell_wol::page::key_press(state, code, text);
        return;
    }
    if notice_active(state) {
        activate_notice(state);
        if let Some(key) = code.and_then(App::shell_key_for_code)
            && state.frontend.shell_controller.on_key(key)
        {
            crate::app::shell_network_setup::acknowledge_notice(state);
        }
        return;
    }
    let Some(view) = crate::app::shell_network_setup::view(state) else {
        return;
    };
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return;
    };
    // NewEdit consumes all keyboard input while chat has focus. Escape is
    // inert; Enter appends CRLF before shared5EDD00/5DE360 submission.
    let action = if ui.focused_control == CHAT {
        let key = match code {
            Some(KeyCode::Backspace) => Some(LanEditKey::Backspace),
            Some(KeyCode::Delete) => Some(LanEditKey::Delete),
            Some(KeyCode::ArrowLeft) => Some(LanEditKey::Left),
            Some(KeyCode::ArrowRight) => Some(LanEditKey::Right),
            Some(KeyCode::Home) => Some(LanEditKey::Home),
            Some(KeyCode::End) => Some(LanEditKey::End),
            Some(KeyCode::Enter | KeyCode::NumpadEnter) => Some(LanEditKey::Enter),
            _ => None,
        };
        if let Some(key) = key {
            ui.chat.key(key).map(SetupAction::Chat)
        } else {
            if !matches!(code, Some(KeyCode::Escape | KeyCode::Tab))
                && let Some(text) = text
            {
                ui.chat.insert_text(text);
            }
            None
        }
    } else {
        match code {
            Some(KeyCode::Escape) if !view.locked => Some(SetupAction::Back),
            _ => None,
        }
    };
    if let Some(action) = action {
        crate::app::shell_network_setup::apply_action(state, action);
    }
    state.platform.window.request_redraw();
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    if transfer_active(state) {
        return;
    }
    if crate::app::shell_network_setup::verify(state).is_some() {
        crate::app::shell_network_setup::verify_cursor_moved(state);
        return;
    }
    if crate::app::shell_wol::page::open(state) {
        crate::app::shell_wol::page::cursor_moved(state);
        return;
    }
    if notice_active(state) {
        activate_notice(state);
        let feed = notice_feed(state);
        let (x, y) = state.match_state.input.cursor_px();
        state.frontend.shell_controller.on_pointer_move(x, y, &feed);
        state.platform.window.request_redraw();
        return;
    }
    let Some(view) = crate::app::shell_network_setup::view(state) else {
        return;
    };
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let help = layout
        .control_at(&view, x, y)
        .and_then(|id| tooltip_key(view.dialog, id))
        .and_then(|key| {
            state
                .process_assets
                .csf
                .as_ref()
                .and_then(|csf| csf.get(&key))
                .map(str::to_owned)
        })
        .unwrap_or_default();
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return;
    };
    let mut action = None;
    if let Some(hold) = ui.trackbar_hold.filter(|_| !view.locked) {
        if hold.dragging
            && let Some(bar) = view.trackbars.iter().find(|bar| bar.id == hold.id)
            && let Some(rect) = layout.control_rect(hold.id)
        {
            action = Some(SetupAction::Trackbar {
                control: hold.id,
                value: bar.pointer_value(x, rect, layout.page.scale),
            });
        }
    } else {
        ui.buttons.hovered = layout.control_at(&view, x, y);
        ui.statics.hover(&help, Instant::now());
        if let Some(popup) = ui.popup_geometry(&view, &layout) {
            ui.popup.pointer_moved(popup, x, y);
        }
        let geometry =
            ui.chat
                .list
                .geometry(layout.chat_log, ui.chat.lines.len(), layout.page.scale);
        ui.chat
            .list
            .scroll
            .pointer_moved(geometry, &mut ui.chat.list.top_index, x, y);
    }
    if let Some(action) = action {
        crate::app::shell_network_setup::apply_action(state, action);
    }
    state.platform.window.request_redraw();
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if transfer_active(state) {
        return;
    }
    if button != MouseButton::Left || shell_transition::blocks_shell_input(state) {
        return;
    }
    if crate::app::shell_network_setup::verify(state).is_some() {
        crate::app::shell_network_setup::verify_mouse(state, pressed);
        return;
    }
    if crate::app::shell_wol::page::open(state) {
        crate::app::shell_wol::page::mouse(state, pressed);
        return;
    }
    if notice_active(state) {
        activate_notice(state);
        let feed = notice_feed(state);
        let (x, y) = state.match_state.input.cursor_px();
        if pressed {
            state.frontend.shell_controller.on_pointer_down(x, y, &feed);
            if state.frontend.shell_controller.pressed().is_some() {
                App::play_launcher_options_cue(state, LauncherCue::MainButton);
            }
        } else if state.frontend.shell_controller.on_pointer_up(x, y, &feed)
            == Some(crate::ui::shell::modal::control::OK)
        {
            crate::app::shell_network_setup::acknowledge_notice(state);
        }
        state.platform.window.request_redraw();
        return;
    }
    let Some(view) = crate::app::shell_network_setup::view(state) else {
        return;
    };
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let over = layout
        .control_at(&view, x, y)
        .filter(|id| view.enabled(*id));
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return;
    };
    let mut action = None;
    let mut cue = None;
    if pressed {
        if let Some(popup) = ui.popup_geometry(&view, &layout) {
            if let Some(part) = popup.scroll_part_at(x, y) {
                ui.popup.scroll.press(
                    part,
                    popup.scroll_geometry(),
                    &mut ui.popup.top_index,
                    y,
                    Instant::now(),
                );
            } else {
                if let Some(index) = popup.row_at(x, y)
                    && let Some(control) = ui.popup.control
                    && let Some(item) = view
                        .combo(control)
                        .and_then(|combo| combo.choices.get(index))
                {
                    action = Some(SetupAction::Select {
                        control,
                        value: item.value,
                    });
                }
                ui.popup.close();
                cue = Some(LauncherCue::ComboClose);
            }
        } else if let Some(control) = over {
            if let Some(combo) = view.combo(control) {
                if crate::ui::skirmish_shell::combo_arrow_hit_rect(
                    layout.control_rect(control).expect("combo rect"),
                    layout.page.scale,
                )
                .contains(x, y)
                {
                    ui.popup.open(control, combo.selected);
                }
                cue = Some(LauncherCue::ComboOpen);
            } else if CHECKBOXES.contains(&control) {
                if crate::ui::skirmish_shell::checkbox_icon_rect(
                    layout.control_rect(control).expect("checkbox rect"),
                    layout.page.scale,
                )
                .contains(x, y)
                {
                    action = Some(SetupAction::Toggle { control });
                    cue = Some(LauncherCue::Checkbox);
                }
            } else if let Some(bar) = view.trackbars.iter().find(|bar| bar.id == control) {
                let press = bar.press(
                    x,
                    y,
                    layout.control_rect(control).expect("trackbar rect"),
                    layout.page.scale,
                );
                ui.trackbar_hold = press.hold(control);
                if let TrackbarPress::Jump(value) = press {
                    action = Some(SetupAction::Trackbar { control, value });
                }
            } else if control == CHAT {
                ui.focus_chat();
            } else if control == CHAT_LOG {
                let geometry =
                    ui.chat
                        .list
                        .geometry(layout.chat_log, ui.chat.lines.len(), layout.page.scale);
                if let Some(part) = geometry.scroll_part_at(x, y) {
                    ui.chat.list.scroll.press(
                        part,
                        geometry,
                        &mut ui.chat.list.top_index,
                        y,
                        Instant::now(),
                    );
                }
            } else {
                ui.buttons.press(Some(control));
                cue = Some(LauncherCue::MainButton);
            }
        }
    } else {
        ui.trackbar_hold = None;
        ui.popup.cancel_pointer();
        ui.chat.list.scroll.cancel();
        if let Some(control) = ui.buttons.release(over) {
            action = match control {
                START => Some(SetupAction::Start),
                ACCEPT => Some(SetupAction::Accept),
                BACK => Some(SetupAction::Back),
                CHOOSE_MAP => Some(SetupAction::ChooseMap),
                EMOTE => Some(SetupAction::Emote),
                PAGE => Some(SetupAction::Page),
                PREVIOUS_ROWS => Some(SetupAction::RosterPage { forward: false }),
                NEXT_ROWS => Some(SetupAction::RosterPage { forward: true }),
                _ => None,
            };
        }
    }
    if let Some(cue) = cue {
        App::play_launcher_options_cue(state, cue);
    }
    if let Some(action) = action {
        crate::app::shell_network_setup::apply_action(state, action);
    }
    state.platform.window.request_redraw();
}

/// Original scrollbar timers: one callback, no catch-up burst; no wheel or
/// arrow-key path exists in the native ComboDropWin/list subclasses.
pub(crate) fn poll(state: &mut AppState) -> Option<Instant> {
    if crate::app::shell_network_setup::verify(state).is_some() {
        return crate::app::shell_network_setup::verify_poll_scroll(state);
    }
    if transfer_active(state) {
        return None;
    }
    if notice_active(state) {
        return None;
    }
    let Some(view) = crate::app::shell_network_setup::view(state) else {
        return None;
    };
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let Some(ui) = state.frontend.multiplayer_setup.as_mut() else {
        return None;
    };
    let mut deadline = None;
    if let Some(popup) = ui.popup_geometry(&view, &layout) {
        if ui.popup.scroll.poll(
            popup.scroll_geometry(),
            &mut ui.popup.top_index,
            x,
            y,
            Instant::now(),
        ) {
            state.platform.window.request_redraw();
        }
        deadline = ui.popup.scroll.repeat_at();
    }
    let geometry = ui
        .chat
        .list
        .geometry(layout.chat_log, ui.chat.lines.len(), layout.page.scale);
    if ui
        .chat
        .list
        .scroll
        .poll(geometry, &mut ui.chat.list.top_index, x, y, Instant::now())
    {
        state.platform.window.request_redraw();
    }
    deadline
        .into_iter()
        .chain(ui.chat.list.scroll.repeat_at())
        .min()
}
