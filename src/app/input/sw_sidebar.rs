//! Phobos SuperWeaponSidebar pointer input.
//!
//! `SWButtonClass::Action` fires on left/right press; `ToggleSWButtonClass`
//! switches on release. VERA consumes the press and keeps the same
//! `apply_sidebar_action` launch/selection path as the vanilla cameo.

use winit::event::MouseButton;

use crate::app::AppState;
use crate::ui::sidebar::SidebarAction;

pub(crate) fn mouse(state: &mut AppState, button: MouseButton, pressed: bool) -> bool {
    if !matches!(button, MouseButton::Left | MouseButton::Right) {
        return false;
    }
    let (cursor_x, cursor_y) = state.match_state.input.cursor_px();
    let hit =
        crate::app::presentation::sw_sidebar::hit_test(state, cursor_x as f32, cursor_y as f32);
    // A left release ends the toggle press even when it happens off the
    // button, so the next press starts clean.
    let toggle_release = (button == MouseButton::Left && !pressed)
        && std::mem::take(
            &mut state
                .match_state
                .match_presentation
                .sw_sidebar
                .toggle_pressed,
        );
    let Some(hit) = hit else {
        return false;
    };
    if let crate::app::presentation::sw_sidebar::SwSidebarHit::Toggle = hit {
        if crate::app::presentation::sw_sidebar::toggle_art(state).is_none() {
            return false;
        }
        // `ToggleSWButtonClass` listens to left press/release only and switches
        // on release; a right click does nothing.
        if button != MouseButton::Left {
            return true;
        }
        if pressed {
            state
                .match_state
                .match_presentation
                .sw_sidebar
                .toggle_pressed = true;
            return true;
        }
        if toggle_release {
            crate::app::presentation::sw_sidebar::toggle(state);
            play_main_button_sound(state);
            crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
        }
        return true;
    }
    if !pressed {
        return true;
    }
    if let crate::app::presentation::sw_sidebar::SwSidebarHit::SuperWeapon { type_id, .. } = hit {
        crate::app::input::dispatch::apply_sidebar_action(
            state,
            SidebarAction::SuperWeaponPress {
                section: type_id,
                right: button == MouseButton::Right,
            },
        );
    }
    crate::app::presentation::sidebar_render::refresh_sidebar_projection(state);
    true
}

pub(crate) fn play_main_button_sound(state: &mut AppState) {
    let sound = state
        .rules()
        .and_then(|rules| rules.general.gui_main_button_sound.clone());
    crate::app::App::play_shell_ui_sound_by_id(state, sound.as_deref());
}
