//! Decidable events (RFC 0001 section 6f, open question 10): the numbers of
//! the events this engine asks modules to decide, the `DecideRecord`
//! `on_decide` reads and writes in place, and the decision budget.
//!
//! The first three decidable events, chosen for clean commit points:
//! - `TakeDamage` (appendix 4.3): the final damage, just before Object's
//!   ReceiveDamage writes Strength;
//! - `ProductionQueued` (4.5): 1 allow, 0 refuse, when a PRODUCE event or
//!   the AI's factory adds a type;
//! - `Defeat` (4.5): 1 defeat, 0 veto, when the house defeat gate is about
//!   to defeat a house.
//!
//! Provisional numbering (open question 5): new events continue from 1000
//! in appendix order, 4.3 (1000..1025), then 4.4 (1026..1030), then 4.5
//! (1031..1036).

use crate::triggers::{EVENT_TAKE_DAMAGE, EventRecord};

pub const EVENT_PRODUCTION_QUEUED: i32 = 1032;
pub const EVENT_DEFEAT: i32 = 1033;

/// Events with a decision form in this engine.
pub const DECIDABLE_EVENTS: [i32; 3] = [EVENT_TAKE_DAMAGE, EVENT_PRODUCTION_QUEUED, EVENT_DEFEAT];
/// Of those, the decision-only ones (appendix 4.5): they need a decision
/// handler and are never delivered.
pub const DECISION_ONLY_EVENTS: [i32; 2] = [EVENT_PRODUCTION_QUEUED, EVENT_DEFEAT];

/// Operations one `on_decide` call may use (open question 7): a tenth of
/// the default frame budget, since decisions run inside the engine's own
/// loops. A call that runs out stops the module.
pub const DECIDE_BUDGET: u64 = 20_000;

/// `DecideRecord` (appendix "Fixed layouts"), 152 bytes little endian. The
/// kinds decided here have no result area (`area`, `area_cap` and
/// `previous_len` are 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecideRecord {
    pub event: EventRecord,
    /// The engine's own result.
    pub native: i64,
    /// The previous link's result, `native` for the first link.
    pub previous: i64,
}

impl DecideRecord {
    pub const SIZE: usize = 152;

    /// The record as `on_decide` receives it: `result` starts as
    /// `previous` and `handled` as 0.
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut out = [0u8; Self::SIZE];
        out[..EventRecord::SIZE].copy_from_slice(&self.event.to_bytes());
        out[112..120].copy_from_slice(&self.native.to_le_bytes());
        out[120..128].copy_from_slice(&self.previous.to_le_bytes());
        out[128..136].copy_from_slice(&self.previous.to_le_bytes());
        out
    }

    /// The handler's answer in a record it returned: `result` when it set
    /// `handled`, else `None` (the previous result stands).
    pub fn answer(bytes: &[u8; Self::SIZE]) -> Option<i64> {
        let handled = crate::le_i32(bytes, 136);
        (handled != 0).then(|| crate::le_i64(bytes, 128))
    }
}
