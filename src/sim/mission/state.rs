//! Lossless mission selector values used by Mission authority.
//!
//! Native Mission selectors are signed dwords. This wrapper preserves every
//! raw bit while allowing checked access to the known `MissionType` vocabulary.

use super::{MISSION_COUNT, MissionDispatchTimer, MissionType};

/// A native-width Mission selector.
///
/// `-1` is the native no-mission sentinel. Values outside `0..=31` are retained
/// verbatim because native state and save data can contain unknown dwords.
#[repr(transparent)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct MissionId(i32);

impl MissionId {
    /// The native signed no-mission sentinel.
    pub const NONE: Self = Self(-1);

    /// Preserve a raw native selector without validation or normalization.
    #[inline]
    pub const fn from_raw(raw: i32) -> Self {
        Self(raw)
    }

    /// Return the raw signed dword.
    #[inline]
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Convert a known mission while translating the Rust vocabulary's idle
    /// discriminant to the native signed sentinel.
    #[inline]
    pub const fn from_known(known: MissionType) -> Self {
        if matches!(known, MissionType::None) {
            Self::NONE
        } else {
            Self(known as i32)
        }
    }

    /// Return the known mission represented by this selector.
    ///
    /// Unknown values and the signed no-mission sentinel remain distinguishable
    /// through [`MissionId::raw`] and are never normalized to `MissionType::None`.
    pub fn known(self) -> Option<MissionType> {
        let raw = self.0;
        if !(0..MISSION_COUNT as i32).contains(&raw) {
            return None;
        }
        MissionType::from_id(raw as u8)
    }

    /// Return the dispatch-table index only for known dispatched missions.
    #[cfg(test)]
    pub fn dispatch_index(self) -> Option<usize> {
        let raw = self.0;
        (0..MISSION_COUNT as i32)
            .contains(&raw)
            .then_some(raw as usize)
    }
}

/// Exact native-width Mission state owned by one game object.
///
/// Field order mirrors the deterministic snapshot/hash contract.  The fields
/// stay private so all gameplay writes pass through a named compatibility or
/// exact-authority transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MissionCom {
    current: MissionId,
    suspended: MissionId,
    queued: MissionId,
    movement_bypass_latch: u8,
    handler_state: u32,
    mission_start_frame: u32,
    ai_counter: u32,
    dispatch_timer: MissionDispatchTimer,
}

impl MissionCom {
    /// Construct native Mission state at the entity's construction frame.
    pub(crate) const fn at_frame(frame: u32) -> Self {
        Self {
            current: MissionId::NONE,
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(frame),
        }
    }

    pub const fn current(&self) -> MissionId {
        self.current
    }

    pub const fn suspended(&self) -> MissionId {
        self.suspended
    }

    pub const fn queued(&self) -> MissionId {
        self.queued
    }

    pub const fn movement_bypass_latch(&self) -> u8 {
        self.movement_bypass_latch
    }

    pub const fn handler_state(&self) -> u32 {
        self.handler_state
    }

    pub const fn mission_start_frame(&self) -> u32 {
        self.mission_start_frame
    }

    pub const fn ai_counter(&self) -> u32 {
        self.ai_counter
    }

    pub const fn dispatch_timer(&self) -> MissionDispatchTimer {
        self.dispatch_timer
    }

    /// Native effective selector: current when present, otherwise queued.
    pub fn effective(&self) -> MissionId {
        if self.current == MissionId::NONE {
            self.queued
        } else {
            self.current
        }
    }

    pub(super) fn assign_transition(&mut self, requested: MissionId, now: u32) {
        self.current = requested;
        self.queued = MissionId::NONE;
        self.movement_bypass_latch = 0;
        self.handler_state = 0;
        self.mission_start_frame = now;
        self.ai_counter = 0;
        self.dispatch_timer = MissionDispatchTimer::at_frame(now);
    }

    pub(super) fn write_queue_and_clear_b8(&mut self, requested: MissionId) {
        self.queued = requested;
        self.movement_bypass_latch = 0;
    }

    pub(super) fn promote_queue(&mut self, now: u32) {
        self.current = self.queued;
        self.queued = MissionId::NONE;
        self.movement_bypass_latch = 0;
        self.handler_state = 0;
        self.mission_start_frame = now;
        self.ai_counter = 0;
        self.dispatch_timer = MissionDispatchTimer::at_frame(now);
    }

    pub(super) fn override_transition(&mut self, requested: MissionId) {
        self.suspended = if self.queued == MissionId::NONE {
            self.current
        } else {
            self.queued
        };
        self.current = requested;
        self.movement_bypass_latch = 0;
    }

    pub(super) fn restore_transition(&mut self) {
        self.current = self.suspended;
        self.suspended = MissionId::NONE;
        self.movement_bypass_latch = 0;
    }

    pub(crate) fn increment_ai_counter(&mut self) {
        self.ai_counter = self.ai_counter.wrapping_add(1);
    }

    pub(crate) fn write_dispatch_epilogue(&mut self, start_frame: i32, delay: i32) {
        self.dispatch_timer = MissionDispatchTimer::from_raw(start_frame, delay);
    }

    /// Handler-owned FSM cursor write at the dispatch commit. Native mission
    /// handlers write the handler-state dword directly while executing; this is
    /// that write, performed by the host when it commits a handler step.
    pub(crate) fn set_handler_state(&mut self, value: u32) {
        self.handler_state = value;
    }

    /// Handler-owned `+0xB8 = 1` write. `UnitClass::Mission_Unload` state 4
    /// (`0x0073DCC7`) sets it immediately after its `Queue_Mission(Guard)`
    /// cleared it, so the queued Guard promotes past the moving-defer gate.
    pub(crate) fn set_movement_bypass_latch(&mut self) {
        self.movement_bypass_latch = 1;
    }

    #[cfg(test)]
    pub(super) fn set_movement_bypass_after_verified_queue(&mut self) {
        self.set_movement_bypass_latch();
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub(crate) struct MissionTestFixture {
    pub current: MissionId,
    pub suspended: MissionId,
    pub queued: MissionId,
    pub movement_bypass_latch: u8,
    pub handler_state: u32,
    pub mission_start_frame: u32,
    pub ai_counter: u32,
    pub dispatch_timer: MissionDispatchTimer,
}

#[cfg(test)]
impl MissionCom {
    pub(crate) fn apply_test_fixture(&mut self, fixture: MissionTestFixture) {
        self.current = fixture.current;
        self.suspended = fixture.suspended;
        self.queued = fixture.queued;
        self.movement_bypass_latch = fixture.movement_bypass_latch;
        self.handler_state = fixture.handler_state;
        self.mission_start_frame = fixture.mission_start_frame;
        self.ai_counter = fixture.ai_counter;
        self.dispatch_timer = fixture.dispatch_timer;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_known_mission_ids_round_trip_without_changing_width() {
        for raw in 0..MISSION_COUNT as i32 {
            let known = MissionType::from_id(raw as u8).expect("known mission id");
            let id = MissionId::from_raw(raw);

            assert_eq!(id.raw(), raw);
            assert_eq!(id.known(), Some(known));
            assert_eq!(id.dispatch_index(), Some(raw as usize));
            assert_eq!(MissionId::from_known(known), id);
        }
    }

    #[test]
    fn unknown_mission_id_round_trips_without_normalization() {
        let id = MissionId::from_raw(0x1234_5678);

        assert_eq!(id.raw(), 0x1234_5678);
        assert_eq!(id.known(), None);
        assert_eq!(id.dispatch_index(), None);
    }

    #[test]
    fn none_is_signed_minus_one_not_enum_ff() {
        assert_eq!(MissionId::NONE.raw(), -1);
        assert_eq!(MissionId::from_known(MissionType::None), MissionId::NONE);
        assert_eq!(MissionId::NONE.known(), None);
        assert_eq!(MissionId::NONE.dispatch_index(), None);
        assert_ne!(MissionId::NONE.raw(), MissionType::None as i32);
    }

    #[test]
    fn high_bit_and_enum_idle_raw_values_remain_unknown() {
        for raw in [i32::MIN, -2, MissionType::None as i32, i32::MAX] {
            let id = MissionId::from_raw(raw);

            assert_eq!(id.raw(), raw);
            assert_eq!(id.known(), None);
            assert_eq!(id.dispatch_index(), None);
        }
    }

    #[test]
    fn mission_state_constructor_uses_exact_native_initial_values() {
        let state = MissionCom::at_frame(37);

        assert_eq!(state.current(), MissionId::NONE);
        assert_eq!(state.suspended(), MissionId::NONE);
        assert_eq!(state.queued(), MissionId::NONE);
        assert_eq!(state.movement_bypass_latch(), 0);
        assert_eq!(state.handler_state(), 0);
        assert_eq!(state.mission_start_frame(), 0);
        assert_eq!(state.ai_counter(), 0);
        assert_eq!(state.dispatch_timer(), MissionDispatchTimer::at_frame(37));
    }

    #[test]
    fn mission_state_serde_preserves_unknown_raw_selectors() {
        let mut state = MissionCom::at_frame(37);
        state.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(i32::MIN),
            suspended: MissionId::from_raw(0x1234_5678),
            queued: MissionId::from_raw(i32::MAX),
            movement_bypass_latch: 0xa5,
            handler_state: 0x1122_3344,
            mission_start_frame: 0x5566_7788,
            ai_counter: 0x99aa_bbcc,
            dispatch_timer: MissionDispatchTimer::from_raw(-17, -29),
        });

        let bytes = bincode::serialize(&state).expect("serialize Mission state");
        let restored: MissionCom = bincode::deserialize(&bytes).expect("deserialize Mission state");

        assert_eq!(restored, state);
    }

    #[test]
    fn mission_verified_host_write_counter_wraps_and_epilogue_only_rewrites_timer() {
        let mut state = MissionCom::at_frame(0);
        state.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(1),
            suspended: MissionId::from_raw(2),
            queued: MissionId::from_raw(3),
            movement_bypass_latch: 4,
            handler_state: 5,
            mission_start_frame: 6,
            ai_counter: u32::MAX,
            dispatch_timer: MissionDispatchTimer::from_raw(7, 8),
        });

        state.increment_ai_counter();
        assert_eq!(state.ai_counter(), 0);
        let before = state;

        state.write_dispatch_epilogue(-17, -29);

        assert_eq!(
            state.dispatch_timer(),
            MissionDispatchTimer::from_raw(-17, -29)
        );
        assert_eq!(state.current(), before.current());
        assert_eq!(state.suspended(), before.suspended());
        assert_eq!(state.queued(), before.queued());
        assert_eq!(
            state.movement_bypass_latch(),
            before.movement_bypass_latch()
        );
        assert_eq!(state.handler_state(), before.handler_state());
        assert_eq!(state.mission_start_frame(), before.mission_start_frame());
        assert_eq!(state.ai_counter(), before.ai_counter());
    }
}
