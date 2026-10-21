//! Render-agnostic retail UI models and dialogs.
//!
//! Shells and the in-game sidebar keep layout and state here; app/render
//! layers draw them with the shared retail artwork and bitmap font.
//!
//! ## Dependency rules
//! - ui/ depends on: sim/ (reads game state, produces commands)
//! - ui/ does NOT depend on: assets/, render/, audio/, net/

pub mod campaign_score_shell;
pub(crate) mod coop_score_shell;
pub mod campaign_shell;
pub mod gadget;
pub mod game_screen;
pub(crate) mod lan_shell;
pub mod main_menu_dialogs;
pub mod main_menu_shell;
pub mod messages;
pub mod movies_credits_shell;
pub(crate) mod multiplayer_setup;
pub mod network_options;
pub mod pause_menu;
pub mod score_shell;
pub mod shell;
pub mod sidebar;
pub mod single_player_shell;
pub mod skirmish_shell;
pub mod tooltips;
pub(crate) mod wol_buddy;
pub(crate) mod wol_chat;
pub(crate) mod wol_lobby;
pub mod wol_login;
pub(crate) mod wol_my_info;
pub(crate) mod wol_options;
pub(crate) mod wol_page;
pub(crate) mod wol_persona;
pub(crate) mod wol_quick_match;
pub mod wol_shell;
pub(crate) mod wol_verify;
// pub mod skirmish;
// pub mod dialog;
// pub mod settings;
