//! Mission scheduler runtime state, authority, and handler substrate.
//!
//! Rules-owned selector vocabulary and immutable mission-control data live in
//! `rules::mission_data`. This module keeps the lossless runtime selectors,
//! private common fields, timers, exact verb surface, and handler state
//! machines. Compatibility re-exports preserve the established
//! `sim::mission` paths without creating a second data owner.
//!
//! Depends on `rules/` for static mission data and otherwise remains in
//! `sim/` — never render/ui/audio/net.

pub(crate) mod authority;
pub(crate) mod capture;
pub(crate) mod concrete_effects;
pub(crate) mod enter;
pub mod control;
pub(crate) mod leaf;
pub(crate) mod mega_mission;
pub(crate) mod readiness;
pub mod retask;
pub mod state;
pub mod timer;
pub mod verb;

pub use crate::rules::mission_data::{MISSION_COUNT, MissionType};
pub use control::{MissionControl, MissionControlEntry};
pub(crate) use leaf::MissionLeafState;
pub use state::{MissionCom, MissionId};
pub use timer::{MissionDispatchTimer, MissionTimer};
