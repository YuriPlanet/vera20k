//! Native StageClass clock operations, shared by Infantry actions, harvesting and
//! building controls. Original identities: constructor6F2B5E..6F2B81;
//! Techno6FABC4..6FAC31; Building4509DE..450A38. Native Building steps before
//! its Techno AI and skips the latter block; other classes step after Mission
//! dispatch. Construction and Selling read that same early Building clock.
//!
//! Native evidence: tools/spatial_oracle/anytown_damage/foot_missions.json
//! ground_firing_receipt and stage_clock_receipt (36 original Techno/Building
//! controls, 58 frame visits and admitted/refused original DoAction controls),
//! tools/spatial_oracle/harvest_field.json and building_construction.json.
//! +104 is copied stack residue and does not participate in the timer decision.

use crate::sim::timer::CdTimer;

/// The retained +F8/+FC/+100/+108/+10C/+110 Techno clock; Building repair
/// owns a distinct instance at+620 (constructor43B7F5, Repair44BBF2).
/// Fields remain private; family receivers use the owner's operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StageClass {
    value: i32,
    changed: u8,
    timer: CdTimer,
    rate: i32,
    increment: i32,
}

impl StageClass {
    pub(crate) const fn constructed(now: i32) -> Self {
        Self {
            value: 0,
            changed: 0,
            timer: CdTimer::started(now, 0),
            rate: 0,
            increment: 1,
        }
    }

    /// Original6FABC4 and4509DE: advance at most once, even after a long gap.
    /// The changed byte records passing the gate, including increment zero.
    pub(crate) fn advance(&mut self, now: i32) -> bool {
        let stepped = self.timer.expired(now) && self.rate != 0;
        self.changed = u8::from(stepped);
        if stepped {
            self.value = self.value.wrapping_add(self.increment);
            self.timer.start(now, self.rate);
        }
        stepped
    }

    /// DoAction51D9D2..51DA96, Unit Unlimbo737BF5..737C75, and the
    /// existing harvesting/building receivers
    /// restart value/rate/timer while retaining the independent FC/110 fields.
    pub(crate) fn restart(&mut self, value: i32, now: i32, rate: i32) {
        self.value = value;
        self.timer.start(now, rate);
        self.rate = rate;
    }

    /// Unload73E3xx resets the dump counter without restarting its clock.
    pub(crate) fn set_value(&mut self, value: i32) {
        self.value = value;
    }

    pub const fn value(&self) -> i32 {
        self.value
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn timer(&self) -> CdTimer {
        self.timer
    }

    /// Repair44BCD6 changes only the rate, retaining the existing timer.
    pub(crate) fn set_rate(&mut self, rate: i32) {
        self.rate = rate;
    }

    pub const fn rate(&self) -> i32 {
        self.rate
    }

    /// Raw prior state supplied by a native corpus, never a production writer.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn from_native_fixture(
        value: i32,
        changed: u8,
        timer: CdTimer,
        rate: i32,
        increment: i32,
    ) -> Self {
        Self {
            value,
            changed,
            timer,
            rate,
            increment,
        }
    }
}
