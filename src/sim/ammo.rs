//! The shared Techno ammo (`+0x2FC`) owner, including an aircraft's pending
//! release (`+0x6C8`) and dock (`+0x6CC`). Unit735592..7355A8,
//! Infantry517D69..517D7A and Aircraft414033..41404B initialize the same
//! signed count from InitialAmmo, falling back to Ammo only for exactly -1.
//! Chrono reinforcement65ED83..65EDC8 reads/decrements that count even on
//! an unlimited-ammo ground unit; it is not a separate formation counter.
//!
//! The airfield loop itself is native: the aircraft's idle mode, Mission_Guard
//! and Mission_Enter (`sim::aircraft`) find a dock through
//! [`Simulation::aircraft_find_docking_bay`] and fly to the pad its radio
//! contact slot names; the dock's own Mission_Repair reloads it
//! (`docking::airfield_reload`).
//!
//! [`Simulation::aircraft_find_docking_bay`]: crate::sim::world::Simulation::aircraft_find_docking_bay
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

/// Per-Techno ammo and the associated aircraft release/dock state.
///
/// Present on every constructed Foot, including negative native counts. The
/// pending release survives Mission changes; it is not an Attack sub-state.
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize)]
pub struct TechnoAmmo {
    /// `TechnoClass+0x2FC`, Ammo: a signed count that wraps. 0 is depleted,
    /// which sends the aircraft home.
    current: i32,
    /// The type's `Ammo=`, copied at construction.
    max: i32,
    /// Aircraft+6C8, initialized by413D40 and set before the release loop41840E.
    pending_release: bool,
    /// Aircraft `+0x6CC`, the dock it last found or was built on: written by
    /// Find_Docking_Bay (`0x0041BC1C`), Mission_Enter (`0x00419CE5`,
    /// `0x00419D57`) and the factory exit (`ExitObject_Main`), cleared by
    /// PointerExpired (`0x0041B67F`). Only an AirportBound aircraft reads it
    /// back (`0x0041BBE3`).
    dock: Option<u64>,
}

impl TechnoAmmo {
    /// Concrete Foot constructors select InitialAmmo unless it is exactly -1
    /// (module addresses). No clamping to zero or the type's maximum occurs.
    pub(crate) fn from_type(obj: &crate::rules::object_type::ObjectType) -> Self {
        let mut ammo = Self::new(obj.ammo);
        if obj.initial_ammo != -1 {
            ammo.current = obj.initial_ammo;
        }
        ammo
    }

    /// Create a new ammo tracker with full ammo.
    pub fn new(max_ammo: i32) -> Self {
        Self {
            current: max_ammo,
            max: max_ammo,
            pending_release: false,
            dock: None,
        }
    }

    /// Ammo (`+0x2FC`).
    pub const fn current(&self) -> i32 {
        self.current
    }

    /// The type's `Ammo=`; only the rearm reads it in production.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn max(&self) -> i32 {
        self.max
    }

    /// `AircraftClass::Drop_Payload @ 0x00415C60`'s Ammo writes: one round
    /// off for each passenger it drops and one back for a refused drop.
    /// Chrono reinforcement65EDC8 also decrements every non-Aircraft member,
    /// including negative/unlimited counts; its DEC wraps at the i32 boundary.
    /// Returns the new count.
    pub(crate) fn add(&mut self, delta: i32) -> i32 {
        self.current = self.current.wrapping_add(delta);
        self.current
    }

    /// RELOAD, radio `0x1F` (`0x006F4C9C`): one round more. The receiver has
    /// already refused a full aircraft.
    pub(crate) fn reload_round(&mut self) {
        self.current = self.current.wrapping_add(1);
    }

    /// The SpawnManager's case-6 rearm (`0x006B788F..0x006B78AA`): Ammo from
    /// the type's `Ammo=`.
    pub(crate) fn refill(&mut self) {
        self.current = self.max;
    }

    /// The kamikaze tracker's Ammo = 1: Push (`0x0054E478..0x0054E483`) and
    /// Update (`0x0054E524`).
    pub(crate) fn load_single_round(&mut self) {
        self.current = 1;
    }

    /// `Assign_Target`'s aircraft arm (`0x006FCDEB..0x006FCE46`): a spawned
    /// aircraft handed another target mid-run drops its ammo.
    pub(crate) fn empty(&mut self) {
        self.current = 0;
    }

    /// Install a fixture's Ammo.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_current_for_test(&mut self, current: i32) {
        self.current = current;
    }

    /// Aircraft `+0x6CC`.
    pub(crate) const fn dock(&self) -> Option<u64> {
        self.dock
    }

    /// The `+0x6CC` writers and PointerExpired's clear.
    pub(crate) fn set_dock(&mut self, dock: Option<u64>) {
        self.dock = dock;
    }

    /// The admitted Mission_Attack release sets this before its Burst loop,
    /// even for Burst<=0 or a FireAt call that returns no Bullet. Its production
    /// writer must be connected with the mission/emission migration; a legacy
    /// fire request alone is not admission.
    pub(crate) fn begin_release(&mut self) {
        self.pending_release = true;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn release_pending(&self) -> bool {
        self.pending_release
    }

    /// Mission_Attack state1/3 and AI after leaving Attack always use DEC;
    /// state10 alone checks Ammo>0. Clear the pending byte even without a debit.
    /// Native418031/4180A1/418BEC,41505E; aircraft_attack_release.json witnesses.
    pub(crate) fn consume_release(&mut self, positive_only: bool) {
        if std::mem::take(&mut self.pending_release) && (!positive_only || self.current > 0) {
            self.current = self.current.wrapping_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chrono_reinforcement_signed_decrement_matches_original_dec() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/trigger_chrono_reinforcement.json",
        ))
        .unwrap();
        let rows = corpus["ammo_decrement"]["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        for row in rows {
            let before = i32::try_from(row["before"].as_i64().unwrap()).unwrap();
            let after = i32::try_from(row["after"].as_i64().unwrap()).unwrap();
            let mut ammo = TechnoAmmo::new(before);
            assert_eq!(ammo.add(-1), after, "original65EDC8 DEC from {before}");
        }
    }
}
