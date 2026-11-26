//! Decidable events (RFC 0001 section 6f, open question 10): the numbers of
//! the events this engine asks modules to decide, the `DecideRecord`
//! `on_decide` reads and writes in place, and the decision budget.
//!
//! The decidable events, each at a clean commit point:
//! - `TakeDamage` (appendix 4.3): the final damage, just before Object's
//!   ReceiveDamage writes Strength;
//! - `ProductionAllowed` (4.5): 1 allow, 0 disallow, once per frame for each
//!   house and bound type; the engine's buildability answer reads it;
//! - `ProductionQueued` (4.5): 1 allow, 0 refuse, when a PRODUCE event or
//!   the AI's factory adds a type;
//! - `Defeat` (4.5): 1 defeat, 0 veto, when the house defeat gate is about
//!   to defeat a house;
//! - `CommandAllowed` (4.5): 1 allow, 0 refuse, when the simulation
//!   executes a player command (a module order included) on a bound object.
//!
//! `Launch` (1035) and `ProjectileFrame` (1036) are tier "later" in the RFC's
//! phase table and have no decision point here.
//!
//! Provisional numbering (open question 5): new events continue from 1000
//! in appendix order, 4.3 (1000..1025), then 4.4 (1026..1030), then 4.5
//! (1031..1036).

use crate::triggers::{EVENT_TAKE_DAMAGE, EventRecord};

pub const EVENT_PRODUCTION_ALLOWED: i32 = 1031;
pub const EVENT_PRODUCTION_QUEUED: i32 = 1032;
pub const EVENT_DEFEAT: i32 = 1033;
pub const EVENT_COMMAND_ALLOWED: i32 = 1034;

/// Events with a decision form in this engine.
pub const DECIDABLE_EVENTS: [i32; 5] = [
    EVENT_TAKE_DAMAGE,
    EVENT_PRODUCTION_ALLOWED,
    EVENT_PRODUCTION_QUEUED,
    EVENT_DEFEAT,
    EVENT_COMMAND_ALLOWED,
];
/// Of those, the decision-only ones (appendix 4.5): they need a decision
/// handler and are never delivered.
pub const DECISION_ONLY_EVENTS: [i32; 4] = [
    EVENT_PRODUCTION_ALLOWED,
    EVENT_PRODUCTION_QUEUED,
    EVENT_DEFEAT,
    EVENT_COMMAND_ALLOWED,
];

/// Operations one `on_decide` call may use (open question 7): a tenth of
/// the default frame budget, since decisions run inside the engine's own
/// loops. A call that runs out stops the module.
pub const DECIDE_BUDGET: u64 = 20_000;

/// `DecideRecord` (appendix "Fixed layouts"), 160 bytes little endian
/// (RFC 0001 0.0.21 grew `EventRecord` to 120).
///
/// Result areas (`area`, `area_cap`, `previous_len`) are always 0: of the
/// kinds with a result area in appendix 4.3 and 4.5 (`Fire`'s shot plan,
/// `Launch`'s `PathSpec`, `ProjectileFrame`'s `ProjectileStep`), none has a
/// decision point here. Every kind decided here answers in `result`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecideRecord {
    pub event: EventRecord,
    /// The engine's own result.
    pub native: i64,
    /// The previous link's result, `native` for the first link.
    pub previous: i64,
}

impl DecideRecord {
    pub const SIZE: usize = 160;

    /// The record as `on_decide` receives it: `result` starts as
    /// `previous` and `handled` as 0.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[..EventRecord::SIZE].copy_from_slice(&self.event.to_bytes());
        out[120..128].copy_from_slice(&self.native.to_le_bytes());
        out[128..136].copy_from_slice(&self.previous.to_le_bytes());
        out[136..144].copy_from_slice(&self.previous.to_le_bytes());
        out
    }

    /// The handler's answer in a record it returned: `result` when it set
    /// `handled`, else `None` (the previous result stands).
    pub fn answer(bytes: &[u8; Self::SIZE]) -> Option<i64> {
        let handled = crate::le_i32(bytes, 144);
        (handled != 0).then(|| crate::le_i64(bytes, 136))
    }
}
