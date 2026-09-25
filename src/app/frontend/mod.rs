//! Front-end owner (F12): launch boundary, map listing, skirmish
//! session/settings, every shell render surface, quit cascade, startup
//! options/splash, and shell slide transitions.

pub(crate) mod campaign_shell_render;
pub(crate) mod load_saved_game_render;
pub(crate) mod credits_roll;
pub(crate) mod fullscreen_movie;
pub mod launch;
pub(crate) mod list_maps;
pub(crate) mod main_menu_shell_render;
pub(crate) mod menu_page_render;
pub(crate) mod movies_credits_render;
pub(crate) mod quit_cascade;
pub(crate) mod score_shell_render;
pub(crate) mod shell_pass;
pub(crate) mod shell_transition;
pub(crate) mod skirmish;
pub(crate) mod skirmish_session;
pub(crate) mod state;
pub(crate) mod skirmish_shell_render;
pub mod startup_options;
pub(super) mod startup_splash;
