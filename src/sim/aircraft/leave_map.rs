//! `AircraftClass::AI`'s removal of an aircraft past the map's edge
//! (`0x00414F47..0x00414FDE`) and the predicate it asks, vtable `+0x4DC`
//! (`0x0041B890`).
//!
//! Evidence: `tools/superweapon_oracle.py` section `aircraft_leave_map` runs
//! the block and the predicate natively; `leave_map_tests.rs` replays it.
//!
//! The surviving aircraft then drops a NavCom or Target that names the
//! shared dummy cell (`0x00414FDF..0x00415046`), before Ready/Commence.
//! A Spy Plane's next Overfly visit can then choose another exit cell.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

impl Simulation {
    /// Aircraft AI414FDF..415046: compare NavCom, then the live Target, with
    /// DummyCell00ABDC50. Each match calls Assign_Destination(NULL, true),
    /// Assign_Target(NULL), Enter_Idle_Mode(false, true), in that order.
    ///
    /// These are pointer comparisons, not map lookups: reuse the retained
    /// cell identity owner without stamping Dummy.coord. Read Target after
    /// the first Idle, which can restore a suspended mission and Target.
    /// The block itself draws no RNG or writes timers; the existing class
    /// setters and Idle own those effects and any mission/dock changes.
    pub(crate) fn cleanup_aircraft_dummy_cells(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        if !self
            .substrate
            .entities
            .get(id)
            .is_some_and(|entity| entity.category() == EntityCategory::Aircraft)
        {
            return;
        }
        for nav_com in [true, false] {
            let pointer = self.substrate.entities.get(id).and_then(|entity| {
                if nav_com {
                    entity
                        .navigation
                        .nav_com()
                        .map(crate::sim::combat::TargetKind::from)
                } else {
                    entity.attack_target.target()
                }
            });
            let dummy = pointer
                .zip(self.resolved_terrain.as_ref())
                .and_then(|(pointer, terrain)| pointer.cell_identity(terrain))
                == Some(crate::map::cell_index::NativeCellIdentity::Dummy);
            if dummy {
                self.assign_aircraft_destination(id, None, rules, effects);
                self.assign_target_represented(id, None, rules, registry, effects)
                    .expect("live aircraft clears its dummy-cell target");
                super::enter_idle_mode_for(self, id, rules, registry, effects);
            }
        }
    }

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
        effects: FrameEffects<'_>,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        if entity.category() != EntityCategory::Aircraft {
            return false;
        }
        let Some(object) = self.interner.object_type(entity.type_ref(), rules) else {
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
                self.playfield_bounds(),
                terrain,
            );
        if (passes_playfield && self.map_cell_in_bounds(cell)) || !self.aircraft_may_leave_map(id) {
            return false;
        }
        self.uninit_with_context(
            id,
            UninitContext::new(rules, registry).with_effects(effects),
        );
        true
    }

    /// `AircraftClass @ 0x0041B890` (vtable `+0x4DC`): whether an aircraft
    /// outside the map may go. Its Target (`+0x2B4`) must be NULL or its
    /// current mission (`+0xAC`) Spyplane Overfly; it must have been in the
    /// playfield (`+0x3D5`); its mission (`Get_Mission`, vt+0x184) must not
    /// be either paradrop mission; then a team member (`+0x5D4`) asks its
    /// team (`0x006EC300`), any other the mission-only latch (`+0x3D4`).
    ///
    /// The shared Team6EC300 owner reads the Scenario waypoint and its
    /// mode1 height-aware playfield gate, including the early cursor exits.
    pub(crate) fn aircraft_may_leave_map(&self, id: u64) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let target = crate::sim::combat::target_present(entity.attack_target.target(), self);
        if target && entity.mission.current() != MissionId::from_known(MissionType::SpyplaneOverfly)
        {
            return false;
        }
        let mission = entity.mission.effective();
        if !entity.in_playfield()
            || mission == MissionId::from_known(MissionType::ParadropApproach)
            || mission == MissionId::from_known(MissionType::ParadropOverfly)
        {
            return false;
        }
        match self.team_script_vm.team_for_member(id) {
            Some(_) => self.member_team_is_moving_outside_playfield(id),
            None => entity.is_mission_only(),
        }
    }
}
