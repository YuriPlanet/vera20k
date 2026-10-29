//! Input owner (F12): tactical/menu input dispatch, hotkeys, context orders,
//! command scheduling, camera, cursor, entity picking, sidebar gadget input,
//! in-game options input, tooltips, and messages.

pub(crate) mod camera;
pub(crate) mod chat;
pub(crate) mod commands;
pub(crate) mod connection;
pub(crate) mod context_order;
pub(crate) mod coop_score;
pub(crate) mod cursor;
pub(crate) mod diplomacy;
pub(crate) mod dispatch;
pub(crate) mod entity_pick;
pub(crate) mod gadget_input;
pub(crate) mod hotkeys;
pub(crate) mod ime;
pub(crate) mod in_game_options;
pub(crate) mod messages;
pub(crate) mod page_user;
pub(crate) mod pause_menu;
pub(crate) mod planning_mode;
pub(crate) mod radar;
pub(crate) mod restate;
pub(crate) mod sidebar_eva;
pub(crate) mod state;
pub(crate) mod superweapon_cameo;
pub(crate) mod taunt;
pub(crate) mod beacon;
pub(crate) mod tooltips;
pub(crate) mod transport_orders;

pub(crate) mod abort;
#[cfg(test)]
mod bridge_click_tests;
pub(crate) mod surrender;

pub(crate) mod sound;

pub(crate) mod campaign_input;
pub(crate) mod keyboard;
pub(crate) mod multiplayer_setup;
pub(crate) mod network_options;
