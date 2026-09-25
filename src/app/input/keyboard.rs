//! Shared A3 lifecycle/input. Original5FBEF0/5FB320 and command assignment5FBB38.
use crate::app::input::hotkeys::{self, BindingAssignmentError, catalog::registered_commands};
use crate::app::{App, AppState};
use crate::ui::shell::keyboard::*;
use crate::ui::shell::list::ShellListGeometry;
use std::time::Instant;
use winit::event::{KeyEvent, MouseButton};
use winit::keyboard::{Key, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

pub(crate) fn layout(state: &AppState) -> Option<KeyboardLayout> {
    let dialog = state.frontend.keyboard_dialog.as_ref()?;
    let game = dialog.parent == KeyboardParent::GameControls;
    let size = if game {
        Some(crate::app::frontend::skirmish_shell_render::current_in_game_shell_layout(state)?)
    } else {
        None
    };
    let (w, h) = if game {
        (
            state.renderer.gpu.config.width,
            state.renderer.gpu.config.height,
        )
    } else {
        (state.render_width(), state.render_height())
    };
    Some(KeyboardLayout::new(w as i32, h as i32, size))
}
fn localized(state: &AppState, key: &str, parameter: Option<u8>) -> String {
    let text = state
        .process_assets
        .csf
        .as_ref()
        .and_then(|csf| csf.get(key))
        .unwrap_or(key);
    match parameter {
        Some(value) => text
            .replace("%2d", &format!("{value:2}"))
            .replace("%d", &value.to_string()),
        None => text.to_owned(),
    }
}
pub(crate) fn open(state: &mut AppState, parent: KeyboardParent) {
    if parent == KeyboardParent::GameControls {
        // BBB52C sets state4/result1;4E1D9A applies+writes before dispatcher48CA03.
        crate::app::persistence::options::accept_in_game_options(state);
        state.match_state.match_presentation.in_game_menu =
            crate::ui::pause_menu::InGameMenuState::Keyboard;
        state.match_state.paused = true;
    }
    let commands = registered_commands()
        .iter()
        .enumerate()
        .map(|(index, metadata)| KeyboardCommandRow {
            command_index: index,
            name: localized(state, metadata.name_key, metadata.parameter),
            category: localized(state, metadata.category_key, None),
            description: localized(state, metadata.description_key, metadata.parameter),
        })
        .collect();
    state.frontend.keyboard_dialog = Some(KeyboardState::new(parent, commands));
    state.platform.window.request_redraw();
}
fn reload(state: &mut AppState, reset: bool) {
    let Some(config) = state.platform.game_config.as_ref() else {
        return;
    };
    let archive = state
        .process_assets
        .manager()
        .and_then(|a| a.get_archive_ref("KEYBOARDMD.INI"));
    state.match_state.input.hotkey_bindings = if reset {
        crate::app::persistence::keyboard::reset(&config.paths.ra2_dir, archive)
    } else {
        crate::app::persistence::keyboard::reload(&config.paths.ra2_dir, archive)
    };
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeyboardExit {
    Back,
    Cancel,
    PumpTerminated,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingExit {
    Save,
    Reload,
    Retain,
}
impl KeyboardExit {
    fn binding_action(self) -> BindingExit {
        match self {
            Self::Back => BindingExit::Save,
            Self::Cancel => BindingExit::Reload,
            Self::PumpTerminated => BindingExit::Retain,
        }
    }
    fn recreate_parent(self, parent: KeyboardParent) -> bool {
        self != Self::PumpTerminated || parent == KeyboardParent::Launcher
    }
}
/// Preserve the app's existing OS-close-to-terminal convention for a suspended
/// D5 parent. Original5FBF37's pump terminal destroys A3 without binding IO;
/// result2 does not imply that IDCANCEL5FB6D4 executed. Native WM_CLOSE
/// equivalence is not asserted by this adapter.
pub(crate) fn prepare_terminal_exit(state: &mut AppState) {
    close(state, KeyboardExit::PumpTerminated);
}
/// Back or Cancel. The front-end page slides out first (`0x005FBEF0`
/// tears A3 down through `0x00622720`); the in-game page closes at once.
fn close(state: &mut AppState, exit: KeyboardExit) {
    let launcher = state
        .frontend
        .keyboard_dialog
        .as_ref()
        .is_some_and(|dialog| dialog.parent == KeyboardParent::Launcher);
    if launcher && exit != KeyboardExit::PumpTerminated {
        App::leave_shell_dialog(
            state,
            crate::app::frontend::shell_transition::ShellExitThen::KeyboardClose(exit),
        );
    } else {
        commit_close(state, exit);
    }
}

/// A3 is gone: save or reload the bindings and rebuild the parent.
pub(crate) fn commit_close(state: &mut AppState, exit: KeyboardExit) {
    let Some(dialog) = state.frontend.keyboard_dialog.take() else {
        return;
    };
    if exit.binding_action() == BindingExit::Save {
        if let Some(config) = state.platform.game_config.as_ref() {
            if let Err(error) = crate::app::persistence::keyboard::save(
                &config.paths.ra2_dir,
                &state.match_state.input.hotkey_bindings,
            ) {
                log::warn!("Could not save KeyboardMD.ini: {error}");
            }
        }
    } else if exit.binding_action() == BindingExit::Reload {
        // IDCANCEL5FB6D4 reloads CURRENT disk, including a preceding Reset All.
        reload(state, false);
    }
    if !exit.recreate_parent(dialog.parent) {
        return;
    }
    match dialog.parent {
        KeyboardParent::Launcher => App::open_launcher_options_dialog(state),
        KeyboardParent::GameControls => {
            let mut options = crate::app::persistence::options::in_game_options_from_profile(
                &state.persistence.options_profile,
            );
            options.sound_enabled = state.audio.launcher_audio_available;
            options.on_open();
            state.match_state.match_presentation.in_game_options = options;
            state.match_state.match_presentation.in_game_menu =
                crate::ui::pause_menu::InGameMenuState::Options;
        }
    }
    state.platform.window.request_redraw();
}
fn activate(state: &mut AppState, id: KeyboardButton) {
    match id {
        KeyboardButton::Back => close(state, KeyboardExit::Back),
        KeyboardButton::ResetAll => {
            reload(state, true);
            if let Some(dialog) = state.frontend.keyboard_dialog.as_mut() {
                dialog.reset_categories();
            }
        }
        KeyboardButton::Assign => {
            let Some(dialog) = state.frontend.keyboard_dialog.as_mut() else {
                return;
            };
            let Some(command) = dialog
                .selected_command()
                .map(|r| registered_commands()[r.command_index].command)
            else {
                return;
            };
            let result = state
                .match_state
                .input
                .hotkey_bindings
                .assign(command, dialog.captured);
            dialog.error = result.err().map(|error| match error {
                BindingAssignmentError::CannotMap => "Error:CannotMap",
                BindingAssignmentError::CannotRemap => "Error:CannotRemap",
            });
            // Parent466→465 refreshes current shortcut and clears the capture.
            dialog.captured = 0;
            dialog.current_owner_visible = false;
        }
    }
}
fn cue(state: &mut AppState, open: bool) {
    let id = state.rules().and_then(|r| {
        if open {
            r.general.gui_combo_open_sound.clone()
        } else {
            r.general.gui_combo_close_sound.clone()
        }
    });
    App::play_shell_ui_sound_by_id(state, id.as_deref());
}

/// Poll the captured list gesture and return its next event-loop wake deadline.
pub(crate) fn poll_scroll_repeat(state: &mut AppState) -> Option<Instant> {
    let layout = layout(state)?;
    let (x, y) = state.window_cursor_position();
    let dialog = state.frontend.keyboard_dialog.as_mut()?;
    let geometry = ShellListGeometry::new(layout.commands, dialog.rows.len(), dialog.top);
    if dialog.scroll.poll(
        geometry,
        &mut dialog.top,
        x.round() as i32,
        y.round() as i32,
        Instant::now(),
    ) {
        state.platform.window.request_redraw();
    }
    dialog.scroll.repeat_at()
}
pub(crate) fn cursor_moved(state: &mut AppState) {
    let Some(layout) = layout(state) else {
        return;
    };
    let (x, y) = state.window_cursor_position();
    let (x, y) = (x.round() as i32, y.round() as i32);
    let dialog = state.frontend.keyboard_dialog.as_mut().unwrap();
    dialog.hovered = layout.control_at(x, y);
    dialog.buttons.hovered = match dialog.hovered {
        Some(KeyboardControl::Button(id)) => Some(id),
        _ => None,
    };
    if dialog.category_open {
        dialog.category_hovered = layout.category_row_at(dialog.categories.len(), x, y);
    }
    let geometry = ShellListGeometry::new(layout.commands, dialog.rows.len(), dialog.top);
    dialog.scroll.pointer_moved(geometry, &mut dialog.top, x, y);
    state.platform.window.request_redraw();
}
pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) {
    if button != MouseButton::Left {
        return;
    }
    let Some(layout) = layout(state) else {
        return;
    };
    let (x, y) = state.window_cursor_position();
    let (x, y) = (x.round() as i32, y.round() as i32);
    let dialog = state.frontend.keyboard_dialog.as_mut().unwrap();
    if dialog.category_open {
        if pressed {
            let row = layout.category_row_at(dialog.categories.len(), x, y);
            if let Some(row) = row {
                dialog.select_category(row);
            }
            dialog.category_open = false;
            cue(state, false);
        }
        return;
    }
    let hit = layout.control_at(x, y);
    let over = match hit {
        Some(KeyboardControl::Button(id)) => Some(id),
        _ => None,
    };
    if !pressed {
        dialog.scroll.cancel();
        state.platform.window.request_redraw();
        let action = dialog.buttons.release(over);
        if let Some(action) = action {
            activate(state, action);
        }
        return;
    }
    dialog.scroll.cancel();
    dialog.buttons.press(over);
    match hit {
        Some(KeyboardControl::Button(_)) => App::play_skirmish_shell_generic_click_sound(state),
        Some(KeyboardControl::Category) => {
            dialog.capture_focused = false;
            dialog.category_open = true;
            dialog.category_hovered = Some(dialog.category);
            cue(state, true);
        }
        Some(KeyboardControl::Commands) => {
            let geometry = ShellListGeometry::new(layout.commands, dialog.rows.len(), dialog.top);
            if let Some(part) = geometry.scroll_part_at(x, y) {
                dialog
                    .scroll
                    .press(part, geometry, &mut dialog.top, y, Instant::now());
            } else if let Some(row) = geometry.row_at(dialog.rows.len(), dialog.top, x, y) {
                dialog.select_command(row);
                App::play_skirmish_shell_generic_click_sound(state);
            }
        }
        Some(KeyboardControl::Capture) => dialog.capture_focused = true,
        _ => {}
    }
    state.platform.window.request_redraw();
}
pub(crate) fn wheel(state: &mut AppState, lines: f32) {
    let Some(layout) = layout(state) else {
        return;
    };
    let dialog = state.frontend.keyboard_dialog.as_mut().unwrap();
    if dialog.category_open {
        let next = (dialog.category as i32 - lines.signum() as i32)
            .clamp(0, dialog.categories.len().saturating_sub(1) as i32) as usize;
        dialog.select_category(next);
    } else if dialog.hovered == Some(KeyboardControl::Commands) {
        let geometry = ShellListGeometry::new(layout.commands, dialog.rows.len(), dialog.top);
        dialog.top =
            (dialog.top as i32 - (lines * 3.0) as i32).clamp(0, geometry.max_top as i32) as usize;
    }
}
pub(crate) fn key(state: &mut AppState, event: &KeyEvent) {
    let Some(dialog) = state.frontend.keyboard_dialog.as_mut() else {
        return;
    };
    if event.state.is_pressed() && event.logical_key == Key::Named(NamedKey::Escape) {
        if dialog.category_open {
            dialog.category_open = false;
            cue(state, false);
        } else {
            close(state, KeyboardExit::Cancel);
        }
        return;
    }
    if dialog.category_open {
        if !event.state.is_pressed() {
            return;
        }
        let step = match event.logical_key {
            Key::Named(NamedKey::ArrowUp) => -1,
            Key::Named(NamedKey::ArrowDown) => 1,
            _ => 0,
        };
        if step != 0 {
            dialog.category_hovered = Some(
                (dialog.category_hovered.unwrap_or(dialog.category) as i32 + step)
                    .clamp(0, dialog.categories.len().saturating_sub(1) as i32)
                    as usize,
            );
        } else if event.logical_key == Key::Named(NamedKey::Enter) {
            let row = dialog.category_hovered.unwrap_or(dialog.category);
            dialog.select_category(row);
            cue(state, false);
        }
        return;
    }
    if !dialog.capture_focused {
        return;
    }
    let unmodified = event.key_without_modifiers();
    let logical = hotkeys::binding_logical_key(&event.logical_key, &unmodified, event.location);
    let Some(vk) = hotkeys::logical_virtual_key(logical, event.location) else {
        return;
    };
    // Parent5FB73A rejects HOTKEYF_EXT, including arrows/Insert/Home/etc.
    let extended = event.location != winit::keyboard::KeyLocation::Numpad
        && matches!(
            event.logical_key,
            Key::Named(
                NamedKey::ArrowLeft
                    | NamedKey::ArrowRight
                    | NamedKey::ArrowUp
                    | NamedKey::ArrowDown
                    | NamedKey::Home
                    | NamedKey::End
                    | NamedKey::PageUp
                    | NamedKey::PageDown
                    | NamedKey::Insert
            )
        );
    if let Some(captured) = capture_key(
        dialog.captured,
        vk,
        hotkeys::modifier_bits(state.platform.live_modifiers),
        event.state.is_pressed(),
        extended,
    ) {
        dialog.captured = captured;
        dialog.current_owner_visible = true;
        dialog.error = None;
        state.platform.window.request_redraw();
    }
}

/// Stock hotkey control state plus A3's HOTKEYF_EXT rejection. The saved
/// current-Windows class probe is distinct from the original formatter oracle.
fn capture_key(before: u16, vk: u16, modifiers: u16, pressed: bool, extended: bool) -> Option<u16> {
    let modifier = match vk {
        16 => Some(0x100),
        17 => Some(0x200),
        18 => Some(0x400),
        _ => None,
    };
    if let Some(bit) = modifier {
        return if pressed {
            Some(modifiers | bit)
        } else if before & 0xff == 0 {
            Some(modifiers & !bit)
        } else {
            None
        };
    }
    if !pressed {
        return None;
    }
    // Clear occurs before DefWindowProc handles these reserved keys.
    if matches!(vk, 8 | 9 | 13 | 27 | 32 | 46) || extended {
        Some(0)
    } else {
        Some(vk | modifiers)
    }
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    fn GetKeyNameTextW(parameter: i32, buffer: *mut u16, size: i32) -> i32;
}
fn virtual_key_name(vk: u16) -> String {
    #[cfg(windows)]
    {
        let mut buffer = [0u16; 32];
        // SAFETY: read-only platform lookup, valid fixed UTF16 buffer and size.
        let length = unsafe {
            let scan = MapVirtualKeyW(u32::from(vk), 0);
            GetKeyNameTextW(((scan << 16) | 0x02000001) as i32, buffer.as_mut_ptr(), 32)
        };
        String::from_utf16_lossy(&buffer[..length.max(0) as usize])
    }
    #[cfg(not(windows))]
    {
        match vk {
            0 => String::new(),
            16 => "Shift".into(),
            17 => "Ctrl".into(),
            18 => "Alt".into(),
            0x30..=0x5a => char::from_u32(u32::from(vk)).unwrap().to_string(),
            0x70..=0x87 => format!("F{}", vk - 0x6f),
            _ => format!("{vk}"),
        }
    }
}
/// Original61EF70: Alt,Ctrl,Shift names joined by literal '+', then low-byteVK.
/// No CSF 'None' placeholder; an empty native key name produces an empty caption.
pub(crate) fn key_name(encoded: u16) -> String {
    let mut output = String::new();
    for (mask, vk) in [(0x400, 18), (0x200, 17), (0x100, 16)] {
        if encoded & mask != 0 {
            output.push_str(&virtual_key_name(vk));
            output.push('+');
        }
    }
    output.push_str(&virtual_key_name(encoded & 0xff));
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_child_exit_preserves_binding_io_distinction_and_parent_final_write() {
        // 5FB900 save,5FB6D4 reload,5FBF37 retain.55FD1E->55FCA0 resumesD5;
        // BBB was accepted before state4 and needs no second terminal write.
        assert_eq!(KeyboardExit::Back.binding_action(), BindingExit::Save);
        assert_eq!(KeyboardExit::Cancel.binding_action(), BindingExit::Reload);
        assert_eq!(
            KeyboardExit::PumpTerminated.binding_action(),
            BindingExit::Retain
        );
        for parent in [KeyboardParent::Launcher, KeyboardParent::GameControls] {
            assert!(KeyboardExit::Back.recreate_parent(parent));
            assert!(KeyboardExit::Cancel.recreate_parent(parent));
        }
        assert!(KeyboardExit::PumpTerminated.recreate_parent(KeyboardParent::Launcher));
        assert!(!KeyboardExit::PumpTerminated.recreate_parent(KeyboardParent::GameControls));
    }
    #[test]
    fn field_state_matches_scoped_windows_control_probe_and_native_ext_rejection() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/storage_oracle/keyboard_key_names.json"
        ))
        .unwrap();
        let cases = fixture["capture"].as_array().unwrap();
        assert_eq!(cases.len(), 13);
        for case in cases {
            let before = case["before"].as_u64().unwrap() as u16;
            let vk = case["vk"].as_u64().unwrap() as u16;
            let extended = case["extended"].as_bool().unwrap();
            let native_to_game = |raw: u16| if raw & 0x800 != 0 { 0 } else { raw };
            let down = capture_key(before, vk, 0, true, extended).unwrap_or(before);
            let up = capture_key(down, vk, 0, false, extended).unwrap_or(down);
            assert_eq!(
                down,
                native_to_game(case["down"].as_u64().unwrap() as u16),
                "down VK{vk}"
            );
            assert_eq!(
                up,
                native_to_game(case["up"].as_u64().unwrap() as u16),
                "up VK{vk}"
            );
        }
    }
}
