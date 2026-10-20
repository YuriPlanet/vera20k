//! D5 -> D7 -> D5 lifecycle and input (55FC80 / 560480).
//! The retained options profile owns settings; this adapter applies the
//! dialog's ordered effects without creating another session or transport.

use crate::app::frontend::{shell_layout, shell_transition};
use crate::app::{App, AppState};
use crate::ui::main_menu_dialogs::options::LauncherCue;
use crate::ui::network_options::*;
use crate::ui::shell::descriptor::DialogId;
use std::time::Instant;
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{KeyCode, PhysicalKey};

const MESSAGE_DIALOG: DialogId = DialogId(0xCE);

pub(crate) fn layout(state: &AppState) -> NetworkOptionsLayout {
    let (width, height) = shell_layout::shell_screen_size(state);
    if shell_layout::fills_window(state) {
        NetworkOptionsLayout::responsive(width as i32, height as i32)
    } else {
        NetworkOptionsLayout::new(width as i32, height as i32)
    }
}

/// Card addresses are a service input. With no IPX service the native combo
/// contains TXT_NO_BOUND_NET_CARD. Controlled GUI runs supply card responses.
pub(crate) fn open(state: &mut AppState, cards: &[String]) {
    let profile = &state.persistence.options_profile;
    state.frontend.network_options = Some(NetworkOptionsState::new(
        profile.network_socket,
        &profile.network_destination,
        profile.network_card,
        profile.network_send_delay,
        profile.network_port,
        cards,
    ));
    state.frontend.shell_controller.reset_to(PAGE.dialog, false);
    App::ensure_skirmish_shell_chrome(state);
    state.platform.window.request_redraw();
}

pub(crate) fn commit_close(state: &mut AppState) {
    state.frontend.network_options = None;
    state
        .frontend
        .shell_controller
        .reset_to(DialogId(0xD5), false);
    App::open_launcher_options_dialog(state);
}

pub(crate) fn update_edit_scroll(state: &mut AppState) {
    let layout = layout(state);
    let Some(dialog) = state.frontend.network_options.as_mut() else {
        return;
    };
    for (edit, rect) in [
        (&mut dialog.socket, layout.socket),
        (&mut dialog.destination, layout.destination),
        (&mut dialog.port, layout.port),
    ] {
        edit.scroll_to_caret(
            (layout.page.scale.base_rect(rect).w - 3).max(0) as u32,
            |text| state.renderer.bit_font.text_width(text),
        );
    }
}

fn apply_effects(state: &mut AppState, effects: Vec<NetworkOptionsEffect>) {
    for effect in effects {
        match effect {
            NetworkOptionsEffect::Lan {
                socket,
                destination,
                card,
            } => {
                let profile = &mut state.persistence.options_profile;
                profile.network_socket = socket;
                profile.network_destination = destination;
                profile.network_card = card;
            }
            NetworkOptionsEffect::SendDelay(value) => {
                state.persistence.options_profile.network_send_delay = value;
            }
            NetworkOptionsEffect::Message(_) => {
                state.frontend.shell_controller.push(MESSAGE_DIALOG, true);
            }
            NetworkOptionsEffect::PortAndClose(port) => {
                state.persistence.options_profile.network_port = port;
                // Native also resets firewall detection (5D8FC0). That service
                // has no live transport owner here; the shell commits the
                // settings in memory before attempting its file write.
                if let Some(config) = state.platform.game_config.as_ref()
                    && let Err(error) = state
                        .persistence
                        .options_profile
                        .commit_network_options(config.paths.settings_dir())
                {
                    log::warn!("Could not save network options: {error}");
                }
                shell_close(state);
            }
            NetworkOptionsEffect::Close => shell_close(state),
        }
    }
    state.platform.window.request_redraw();
}

fn shell_close(state: &mut AppState) {
    App::leave_shell_dialog(state, shell_transition::ShellExitThen::NetworkOptionsClose);
}

fn activate(state: &mut AppState, control: u16) {
    let dialog = state
        .frontend
        .network_options
        .as_mut()
        .expect("D7 input owner");
    let effects = if dialog.pending_message().is_some() {
        if control != OK {
            return;
        }
        state.frontend.shell_controller.pop();
        dialog.acknowledge()
    } else {
        match control {
            OK => dialog.accept(),
            CANCEL => dialog.cancel(),
            _ => Vec::new(),
        }
    };
    apply_effects(state, effects);
}

pub(crate) fn key(state: &mut AppState, event: &KeyEvent) {
    if !event.state.is_pressed() || shell_transition::blocks_shell_input(state) {
        return;
    }
    let code = match event.physical_key {
        PhysicalKey::Code(code) => Some(code),
        _ => None,
    };
    key_press(state, code, event.text.as_deref());
}

/// Already-admitted window keys and controlled GUI captures use this same path.
pub(crate) fn key_press(state: &mut AppState, code: Option<KeyCode>, text: Option<&str>) {
    let dialog = state
        .frontend
        .network_options
        .as_mut()
        .expect("D7 input owner");
    if dialog.pending_message().is_some() {
        if matches!(
            code,
            Some(KeyCode::Enter | KeyCode::NumpadEnter | KeyCode::Escape)
        ) {
            activate(state, OK);
        }
        return;
    }
    match code {
        Some(KeyCode::Tab) => dialog.cycle_focus(state.platform.live_modifiers.shift_key()),
        Some(KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter)
            if dialog.focused_edit_mut().is_some() => {}
        Some(KeyCode::Escape) => activate(state, CANCEL),
        Some(KeyCode::Enter | KeyCode::NumpadEnter) => activate(state, OK),
        _ => {
            if let Some(edit) = dialog.focused_edit_mut() {
                match code {
                    Some(KeyCode::Backspace) => edit.backspace(),
                    Some(KeyCode::Delete) => edit.delete(),
                    Some(KeyCode::ArrowLeft) => edit.left(),
                    Some(KeyCode::ArrowRight) => edit.right(),
                    Some(KeyCode::Home) => edit.home(),
                    Some(KeyCode::End) => edit.end(),
                    _ => {
                        if let Some(text) = text {
                            dialog.insert_text(text);
                        }
                    }
                }
            }
        }
    }
    state.platform.window.request_redraw();
}

fn pointer_control(state: &AppState) -> Option<u16> {
    let (x, y) = state.match_state.input.cursor_px();
    if state
        .frontend
        .network_options
        .as_ref()?
        .pending_message()
        .is_some()
    {
        return crate::ui::shell::modal::body_ok_layout(
            state.render_width() as i32,
            state.render_height() as i32,
        )
        .ok
        .contains(x, y)
        .then_some(OK);
    }
    layout(state).control_at(x, y)
}

pub(crate) fn cursor_moved(state: &mut AppState) {
    let over = pointer_control(state);
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    if let Some(dialog) = state.frontend.network_options.as_mut() {
        dialog.buttons.hovered = over;
        if let Some(popup) = dialog.card_popup(&layout) {
            // ComboDropWin60E262..60E2AC keeps its last hot row when
            // the pointer leaves rows, including over the scrollbar.
            if let Some(row) = popup.row_at(x, y) {
                dialog.card_hot_row = Some(row);
            }
            dialog.card_scroll.pointer_moved(
                popup.scroll_geometry(),
                &mut dialog.card_top_index,
                x,
                y,
            );
        }
    }
    state.platform.window.request_redraw();
}

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left || shell_transition::blocks_shell_input(state) {
        return;
    }
    let over = pointer_control(state);
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let dialog = state
        .frontend
        .network_options
        .as_mut()
        .expect("D7 input owner");
    if pressed {
        if dialog.pending_message().is_none() {
            if let Some(popup) = dialog.card_popup(&layout) {
                if let Some(part) = popup.scroll_part_at(x, y) {
                    dialog.card_scroll.press(
                        part,
                        popup.scroll_geometry(),
                        &mut dialog.card_top_index,
                        y,
                        Instant::now(),
                    );
                    state.platform.window.request_redraw();
                    return;
                }
                if let Some(row) = popup.row_at(x, y) {
                    dialog.select_card(row);
                } else {
                    dialog.combo_open = false;
                }
                App::play_launcher_options_cue(state, LauncherCue::ComboClose);
                state.platform.window.request_redraw();
                return;
            }
            if over == Some(CARD) {
                if crate::ui::skirmish_shell::combo_arrow_hit_rect(layout.card, layout.page.scale)
                    .contains(x, y)
                {
                    dialog.combo_open = true;
                    dialog.card_top_index = 0;
                    dialog.card_hot_row = Some(dialog.selected_card);
                }
                App::play_launcher_options_cue(state, LauncherCue::ComboOpen);
                state.platform.window.request_redraw();
                return;
            }
            if over == Some(SEND_DELAY) {
                // OwnerDraw_CheckBox 6163A0 only admits its 18x18 icon,
                // toggles on button-down, and leaves NewEdit focus alone.
                if crate::ui::skirmish_shell::checkbox_icon_rect(
                    layout.send_delay,
                    layout.page.scale,
                )
                .contains(x, y)
                {
                    dialog.send_delay = !dialog.send_delay;
                    App::play_launcher_options_cue(state, LauncherCue::Checkbox);
                }
                state.platform.window.request_redraw();
                return;
            }
            if let Some(control) = over {
                dialog.focus(control);
            }
        }
        dialog
            .buttons
            .press(over.filter(|id| matches!(*id, OK | CANCEL)));
        if dialog.buttons.pressed.is_some() {
            App::play_main_menu_button_sound(state);
        }
    } else {
        dialog.card_scroll.cancel();
        if let Some(control) = dialog.buttons.release(over) {
            activate(state, control);
        }
    }
    state.platform.window.request_redraw();
}

pub(crate) fn poll_scroll_repeat(state: &mut AppState) -> Option<Instant> {
    let layout = layout(state);
    let (x, y) = state.match_state.input.cursor_px();
    let dialog = state.frontend.network_options.as_mut()?;
    let popup = dialog.card_popup(&layout)?;
    if dialog.card_scroll.poll(
        popup.scroll_geometry(),
        &mut dialog.card_top_index,
        x,
        y,
        Instant::now(),
    ) {
        state.platform.window.request_redraw();
    }
    dialog.card_scroll.repeat_at()
}
