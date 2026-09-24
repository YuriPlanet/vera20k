//! Shared front-end shell substrate (Framework B: Win32-native dialog shells).
//!
//! Holds the geometry primitives the three pixel-parity shells (main menu 0xE2,
//! single player 0x100, skirmish 0x102) used to each re-implement. Render-agnostic:
//! depends on nothing above this layer (no sim/render/assets), so it honors the
//! ui/ layering rule. `geom` (Slice 0), `descriptor`/`layout` (Slice 1), and
//! `controller` (Slice 2 input authority) are shared today; the modal/slide
//! substrate is roadmap (see docs/plans/2026-05-31-shell-substrate-design.md §5).

pub mod controller;
pub mod descriptor;
pub mod geom;
pub mod in_game_options;
pub mod in_game_options_state;
pub mod in_game_shell;
pub mod pause_menu;
pub mod saved_games;
pub mod saved_file_input;
pub mod layout;
pub mod menu_page;
pub mod modal;
pub mod slide;
pub mod static_reveal;
pub mod warning_monitor;
pub mod abort;
pub mod button;

pub mod sound;

pub mod list;

pub mod keyboard;
