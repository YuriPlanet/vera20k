//! `AircraftClass::AI`'s removal of an aircraft past the map's edge
//! (`0x00414F47..0x00414FDE`) and the predicate it asks, vtable `+0x4DC`
//! (`0x0041B890`).
//!
//! Evidence: `tools/superweapon_oracle.py` section `aircraft_leave_map` runs
//! the block and the predicate natively; `leave_map_tests.rs` replays it.
//!
//! RESIDUAL: the block after it (`0x00414FDF..0x00415046`), which drops a
//! NavCom (`+0x5A4`) or Target (`+0x2B4`) that is the map's dummy cell
//! (`0x00ABDC50`) through vt+0x480(NULL, 1), vt+0x3C8(NULL) and
//! vt+0x484(NULL, 1). VERA holds a cell as its coordinates, with no dummy
//! identity, and keeps it. Trigger: a destination or target cell the map
//! lacks, such as an edge pick (`PickCellOnEdge @ 0x004AA440` accepts the
//! first cell outside the playfield) past the map's `Size=` on a map whose
//! playfield reaches its edge. Effect: native drops it and the Spy Plane's
//! Overfly picks again (one more Scenario draw); VERA flies on toward it
//! until this removal. Frequency: only where the playfield reaches the
//! map's `Size=` edge.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{Simulation, UninitContext};

impl Simulation {
    /// The removal block, after the class AI's sinking and trailer steps.
    /// The aircraft's cell (vt+0x1B8 = `0x0041BEA0`, its Location over 256
    /// toward zero) is tested against the playfield
    /// (`MapClass::IsCellInPlayfield @ 0x00578460`, mode 1) unless its type
    /// is `FlyBy=` or `FlyBack=` (AircraftType `+0xE0B`, `+0xE0C`), then
    /// against the map's `Size=` diamond (`MapClass::In_Bounds @ 0x00568300`).
    /// Outside either, an aircraft [`Self::aircraft_may_leave_map`] admits is
    /// UnInit (vt+0xF8, `0x00414F93`, `0x00414FD1`): silently, with no
    /// Death_Announcement, and its AI ends there. Native asks the predicate
    /// after each failed test; it reads only state, so asking once is the
    /// same. Returns whether it was removed.
    pub(crate) fn remove_aircraft_off_map(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.category != EntityCategory::Aircraft {
            return false;
        }
        let Some(object) = rules.object(self.interner.resolve(entity.type_ref())) else {
            return false;
        };
        // No MapClass authority (headless fixtures): no map to leave.
        if self.map_size_diamond().is_none() {
            return false;
        }
        let terrain = self.resolved_terrain.as_ref();
        let location = crate::sim::movement::ground_pose::object_location(entity, terrain);
        let cell = ((location.x / 256) as i16, (location.y / 256) as i16);
        let passes_playfield = object.fly_by
            || object.fly_back
            || crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                (i32::from(cell.0), i32::from(cell.1)),
                self.playfield_bounds,
                terrain,
            );
        if (passes_playfield && self.map_cell_in_bounds(cell)) || !self.aircraft_may_leave_map(id) {
            return false;
        }
        self.uninit_with_context(id, UninitContext::new(Some(rules), registry));
        true
    }

    /// `AircraftClass @ 0x0041B890` (vtable `+0x4DC`): whether an aircraft
    /// outside the map may go. Its Target (`+0x2B4`) must be NULL or its
    /// current mission (`+0xAC`) Spyplane Overfly; it must have been in the
    /// playfield (`+0x3D5`); its mission (`Get_Mission`, vt+0x184) must not
    /// be either paradrop mission; then a team member (`+0x5D4`) asks its
    /// team (`0x006EC300`), any other the mission-only latch (`+0x3D4`).
    ///
    /// `0x006EC300` answers false at each exit before its waypoint read
    /// (`TeamScriptVm::member_step_reads_waypoint`).
    ///
    /// RESIDUAL: that read. A member whose team has formed and whose script
    /// cursor names action 3 is removed natively when the action's waypoint
    /// lies outside the playfield; VERA has no waypoint table in the
    /// simulation, so it is kept. Trigger: a team aircraft past the map's
    /// edge while its script moves to a waypoint off the playfield, mostly in
    /// campaigns. Effect: it stays where native removes it.
    pub(crate) fn aircraft_may_leave_map(&self, id: u64) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let target = super::attack_mission::aircraft_target_present(
            entity.attack_target.as_ref(),
            &self.substrate.entities,
        );
        if target && entity.mission.current() != MissionId::from_known(MissionType::SpyplaneOverfly)
        {
            return false;
        }
        let mission = entity.mission.effective();
        if !entity.in_playfield
            || mission == MissionId::from_known(MissionType::ParadropApproach)
            || mission == MissionId::from_known(MissionType::ParadropOverfly)
        {
            return false;
        }
        match self.team_script_vm.team_for_member(id) {
            Some(_) => false,
            None => entity.is_mission_only(),
        }
    }
}
