//! Front-end shell dialogs (Framework B: Win32-native dialog shells).
//!
//! The render-agnostic substrate (geometry, descriptors, layout pass, button
//! capture, trackbar, paging, static reveal, warning monitor) lives in the
//! `vera20k-gui` crate and is re-exported here; this module keeps the
//! dialog-specific state.

pub mod abort;
pub mod connection;
pub mod controller;
pub mod diplomacy;
pub mod game_speed;
pub mod in_game_options_state;
pub mod in_game_shell;
pub mod menu_page;
pub mod modal;
pub mod pause_menu;
pub mod restate;
pub mod saved_file_input;
pub mod saved_games;
pub mod slide;
pub mod sound;
pub mod list;
pub mod keyboard;
pub(crate) mod combobox;

// The render-agnostic substrate lives in the GUI crate.
pub use vera20k_gui::shell::{
    button, descriptor, geom, layout, paging, static_reveal, trackbar, warning_monitor,
};
