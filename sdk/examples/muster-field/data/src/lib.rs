//! What "Muster Field"'s `rules.wasm` and `ui.wasm` both know: the tables
//! of what can be hired, won and met, the camp's prices, and the protocol
//! between the two (the variables the simulation publishes and the messages
//! the interface sends). No engine calls: both modules link it.
#![no_std]

pub mod camp;
pub mod content;
pub mod line;
pub mod perks;
pub mod proto;
pub mod waves;

pub use line::Line;

pub const MAX_OFFERS: usize = 5;
pub const MAX_COMPANY: usize = 10;
pub const MAX_RAIDERS: usize = 24;

/// The sides the raiders come from, as `battle` names them (its `SIDES`
/// order).
pub const FRONT_NAMES: [&str; 8] = ["西南", "东南", "东北", "西北", "南面", "东面", "西面", "北面"];
