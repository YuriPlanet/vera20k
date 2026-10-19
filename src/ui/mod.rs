//! Render-agnostic retail UI models and dialogs.
//!
//! Shells and the in-game sidebar keep layout and state here; app/render
//! layers draw them with the shared retail artwork and bitmap font.
//!
//! ## Dependency rules
//! - ui/ depends on: sim/ (reads game state, produces commands)
//! - ui/ does NOT depend on: assets/, render/, audio/, net/

pub mod campaign_shell;
pub mod campaign_score_shell;
pub mod gadget;
pub mod game_screen;
pub mod main_menu_dialogs;
pub mod main_menu_shell;
pub mod messages;
pub mod movies_credits_shell;
pub mod pause_menu;
pub mod score_shell;
pub mod shell;
pub mod sidebar;
pub mod single_player_shell;
pub mod skirmish_shell;
pub mod tooltips;
pub mod wol_shell;
pub mod network_options;
pub(crate) mod lan_shell;
pub(crate) mod multiplayer_setup;
// pub mod skirmish;
// pub mod dialog;
// pub mod settings;
