//! Front-end shell dialogs (Framework B: Win32-native dialog shells).
//!
//! The widgets' render-agnostic behaviour (button faces, combo drop-down,
//! list, trackbar, edit) and the generic pixel geometry live in the
//! `vera20k-gui` crate. This module keeps the retail shell's own layout
//! knowledge (dialog resource ids and anchor rules, right-panel geometry, the
//! layout pass, paging, the kind-1 static reveal and the `0x71C` monitor), the
//! 800x600 base and the retail scaling rule, the checkbox art faces, the
//! native fixtures the widgets' goldens replay, and the dialog-specific state.

pub mod abort;
pub mod checkbox;
pub mod connection;
pub mod controller;
pub mod descriptor;
pub mod diplomacy;
pub mod game_speed;
pub mod geom;
pub mod in_game_options_state;
pub mod in_game_shell;
pub mod layout;
pub mod layout_data;
pub mod menu_page;
pub mod modal;
#[cfg(all(test, tests_ui))]
pub mod native_fixtures;
pub mod page;
pub mod paging;
pub mod pause_menu;
pub mod restate;
pub mod saved_file_input;
pub mod saved_games;
pub mod slide;
pub mod sound;
pub mod static_reveal;
pub mod warning_monitor;
pub mod keyboard;

/// The GUI crate's copy of `Math::ftol` for binary64 agrees with the engine's
/// x87 model on the edges a list thumb can reach.
#[cfg(all(test, tests_ui))]
mod ftol_tests {
    #[test]
    fn list_ftol_matches_the_x87_model() {
        for v in [0.0, -0.0, 0.99, -0.99, 13.999999, 2147483647.9, 2147483648.0, -2147483649.5,
            9.3e18, -9.3e18, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300]
        {
            assert_eq!(
                vera20k_gui::shell::list::ftol_f64(v),
                crate::util::native_x87::X87Chop53::ftol_f64_low_masked(v),
                "{v}"
            );
        }
    }
}
