//! Robot Tanks and their Robot Control Center: while a house has an
//! operational `PowersUnit=` building, its vehicles of that type work; when
//! it has none, its `PoweredUnit=` vehicles go offline (Techno `+0x1C8`,
//! [`GameEntity::deactivated`](crate::sim::game_entity::GameEntity)).
//!
//! gamemd-derived (instruction reading; YR 1.001, SHA-256 `1cdd1180…4298c`):
//! - the house's count of powered centers (House `+0x2D8`) and each
//!   building's share in it (`+0x662`): the `PowersUnit=` arms of
//!   BuildingClass's operational-edge dispatcher (`0x004549B0`: on at
//!   `0x004549D1..0x004549F3`, off at `0x00454B16..0x00454B38`, both ahead of
//!   its other work), and Removed_From_Game's copy of LostPoweredCenter
//!   (`0x0050263B..0x00502776`). Added_To_Game never counts, so a center
//!   taken over while working counts for its new house only after its next
//!   off and on edge;
//! - `HouseClass::RobotTanksBackOnline @ 0x0050E010` and `LostPoweredCenter
//!   @ 0x0050E0E0`: the count's first and last center walk the TechnoClass
//!   array from its last entry and Reactivate or Deactivate each object the
//!   house owns whose type is the building's `PowersUnit=` and that is not in
//!   limbo. Any match speaks `EVA_RobotTanksBackOnline` /
//!   `EVA_RobotTanksOffline` for the player;
//! - `TechnoClass::Deactivate @ 0x0070FC90` and `Reactivate @ 0x0070FBE0`;
//! - the Unit's cell arrival (`UnitClass::Per_Cell_Process`
//!   `0x00739F21..0x00739F59`): a `PoweredUnit=` vehicle whose house has no
//!   powered center (`HouseClass @ 0x0050E1B0`) goes offline when no building
//!   stands in its cell. Hover runs it at each step before it leaves the
//!   cell (`TryAdvancePathStep`, `0x00515A1C`) and then claims the next
//!   cell; Stop_Moving (`0x00516320`) keeps the head, and its height keeps
//!   it powered (`0x00516C70`) as it settles, so it drifts toward that head
//!   until it touches down (from a standing start, inside its own cell).
//!   Arrival never brings one back;
//! - the sparks an offline Unit throws (`UnitClass::AI`
//!   `0x007360C9..0x007361A4`).
//!
//! Readers of the latch: control (`TechnoClass::IsControllable`,
//! `0x00700CBC`), the band, type and N/M selection prefix
//! (`CanBeSelectedNow`, `0x006FC041`; a click still selects it), the fire
//! error (`0x006FC117`, ILLEGAL), the select voice (`0x00708EFD`) and the
//! voxel draw (`0x0073C143`, half intensity). Deactivate's Power_Off spins a
//! Hover down (`movement::hover`), and a warp ends the hover of a
//! `PoweredUnit=` type whose house has no powered center (`0x00718864`,
//! `movement::teleport_chrono`).
//!
//! Ledger: Deactivate draws twice from the Scenario stream through Hover's
//! Power_Off when its Is_Powered holds (`0x00516C70`: the powered byte,
//! which a working tank keeps, or any height) and it is neither sleeping
//! nor entering; an offline Unit draws twice on each of its spark frames. Timers: the
//! mission and destination timers ResetOrdersToGuard writes. Detaches: none.
//! `tools/robot_offline_oracle.py` executes Hover's Power_Off and the spark
//! arm (`powered_unit_tests`).
//!
//! RESIDUALS:
//! - TechnoClass::Array order stands on stable-id order (construction order),
//!   as in `world::house_defeat`.
//! - The EVA gate's campaign arm (GameMode 0: `IsHumanPlayer` or
//!   `PlayerControl`, `0x0050E098..0x0050E0AC`) is the app's local-player
//!   test, which is the multiplayer arm (`0x0050E0B0`). Trigger: a campaign
//!   house the player controls loses its center. Effect: no line.
//! - The three EVA sites also require the game to be active (`0x00A8E9A0`)
//!   and no scenario initialization under way (`0x00A8E7AC`,
//!   `0x0050E088..0x0050E0C4`); the walks push the line without them.
//!   Inaudible: retail `evamd.ini` has no entries for either line.
//! - A rules-less lifecycle (fixture paths) cannot read `PowersUnit=`:
//!   Removed_From_Game still uncounts the building but walks nothing.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::movement::ground_pose::object_location;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

/// String `0x00824E4C`.
pub(crate) const EVA_ROBOT_TANKS_OFFLINE: &str = "EVA_RobotTanksOffline";
/// String `0x00824FE4`.
pub(crate) const EVA_ROBOT_TANKS_BACK_ONLINE: &str = "EVA_RobotTanksBackOnline";

impl Simulation {
    /// The `PowersUnit=` arms of the operational-edge dispatcher
    /// (`0x004549B0`), which run before the rest of it: coming online, an
    /// uncounted building counts and calls RobotTanksBackOnline
    /// (`0x004549D1..0x004549F3`); going offline, a counted one stops
    /// counting and calls LostPoweredCenter (`0x00454B16..0x00454B38`).
    pub(crate) fn powered_center_edge(
        &mut self,
        id: u64,
        operational: bool,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(building) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(powers_unit) = self
            .object_type(building.type_ref(), rules)
            .and_then(|object| object.powers_unit.as_deref())
        else {
            return;
        };
        if building.building_powered_center == operational {
            return;
        }
        let owner = building.owner();
        self.substrate
            .entities
            .get_mut(id)
            .expect("same building")
            .building_powered_center = operational;
        if operational {
            self.house_gained_powered_center(owner, powers_unit, rules, effects);
        } else {
            self.house_lost_powered_center(owner, Some(powers_unit), Some(rules), effects);
        }
    }

    /// Removed_From_Game's arm (`0x0050263B..0x00502776`): a counted
    /// `PowersUnit=` Building stops counting (`0x0050269F`), then
    /// LostPoweredCenter's body runs inline.
    pub(super) fn powered_center_removed(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) {
        let Some(building) = self.substrate.entities.get(id) else {
            return;
        };
        if building.category != EntityCategory::Structure || !building.building_powered_center {
            return;
        }
        let owner = building.owner();
        let unit_type = rules
            .and_then(|rules| self.object_type(building.type_ref(), rules))
            .and_then(|object| object.powers_unit.as_deref());
        self.substrate
            .entities
            .get_mut(id)
            .expect("same building")
            .building_powered_center = false;
        self.house_lost_powered_center(owner, unit_type, rules, effects);
    }

    /// `HouseClass::RobotTanksBackOnline @ 0x0050E010`: the first powered
    /// center Reactivates the house's `unit_type` objects.
    fn house_gained_powered_center(
        &mut self,
        owner: InternedId,
        unit_type: &str,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(house) = self.houses.get_mut(&owner) else {
            return;
        };
        if house.gain_powered_center() != 1 {
            return;
        }
        if self.for_each_powered_unit(owner, unit_type, |sim, id| {
            sim.techno_reactivate(id, rules, effects);
        }) {
            self.sound_events.push(SimSoundEvent::HouseEva {
                owner,
                event: EVA_ROBOT_TANKS_BACK_ONLINE,
            });
        }
    }

    /// `HouseClass::LostPoweredCenter @ 0x0050E0E0`: losing the last
    /// powered center Deactivates the house's `unit_type` objects.
    fn house_lost_powered_center(
        &mut self,
        owner: InternedId,
        unit_type: Option<&str>,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) {
        let Some(house) = self.houses.get_mut(&owner) else {
            return;
        };
        if house.lose_powered_center() != 0 {
            return;
        }
        let (Some(unit_type), Some(rules)) = (unit_type, rules) else {
            return;
        };
        if self.for_each_powered_unit(owner, unit_type, |sim, id| {
            sim.techno_deactivate(id, rules, effects);
        }) {
            self.sound_events.push(SimSoundEvent::HouseEva {
                owner,
                event: EVA_ROBOT_TANKS_OFFLINE,
            });
        }
    }

    /// The walk both edges share (`0x0050E039..0x0050E081`,
    /// `0x0050E104..0x0050E14C`): the TechnoClass array from its last entry,
    /// its count read once, visiting each object `owner` owns whose type is
    /// the UnitType `unit_type` names (`FindOrAllocate` matches names without
    /// case) and that is not in limbo. Returns whether any matched; the line
    /// plays only then.
    fn for_each_powered_unit(
        &mut self,
        owner: InternedId,
        unit_type: &str,
        mut visit: impl FnMut(&mut Self, u64),
    ) -> bool {
        let mut matched = false;
        for id in self.substrate.entities.keys_sorted().into_iter().rev() {
            let Some(entity) = self.substrate.entities.get(id) else {
                continue;
            };
            if entity.owner() != owner
                || entity.category != EntityCategory::Unit
                || !self
                    .interner
                    .resolve(entity.type_ref())
                    .eq_ignore_ascii_case(unit_type)
                || entity.lifecycle.in_limbo
            {
                continue;
            }
            visit(self, id);
            matched = true;
        }
        matched
    }

    /// `HouseClass @ 0x0050E1B0`: the house counts a powered center.
    pub(crate) fn has_powered_center(&self, owner: InternedId) -> bool {
        self.houses
            .get(&owner)
            .is_some_and(|house| house.has_powered_center())
    }

    /// `TechnoClass::Deactivate @ 0x0070FC90`. A tethered object (`+0x418`)
    /// whose radio contact is the building in its cell stays as it is
    /// (`0x0070FC97..0x0070FCCA`). Otherwise: ResetOrdersToGuard (vt+0x3D0),
    /// Deselect (vt+0x150), a Foot's ILocomotion Power_Off, its type's
    /// `DeactivateSound=` where it stands unless it was already offline
    /// (`0x0070FD0E..0x0070FD5D`), and the latch (`0x0070FD62`).
    pub(crate) fn techno_deactivate(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.dock_entered_with.is_some()
            && let Some(contact) = entity.radio_contacts.slot(0)
            && self.substrate.occupancy.first_building_on_layer(
                entity.position.rx,
                entity.position.ry,
                MovementLayer::Ground,
            ) == Some(contact)
        {
            return;
        }
        let foot = entity.category.is_foot();
        self.reset_orders_to_guard(id, rules, effects);
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.selected = false;
        }
        if foot {
            self.locomotor_power_off(id);
        }
        self.latch_deactivated(id, true, rules);
    }

    /// `TechnoClass::Reactivate @ 0x0070FBE0`: ResetOrdersToGuard
    /// (vt+0x3D0), a Foot's ILocomotion Power_On (the base `0x0055A8F0` in
    /// every family), its type's `ActivateSound=` where it stands if it was
    /// offline (`0x0070FC18..0x0070FC68`), and the latch cleared
    /// (`0x0070FC6D`, `0x0070FC7A`).
    pub(crate) fn techno_reactivate(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(foot) = self
            .substrate
            .entities
            .get(id)
            .map(|entity| entity.category.is_foot())
        else {
            return;
        };
        self.reset_orders_to_guard(id, rules, effects);
        if foot
            && let Some(locomotor) = self
                .substrate
                .entities
                .get_mut(id)
                .and_then(|entity| entity.locomotor.as_mut())
        {
            locomotor.power_on();
        }
        self.latch_deactivated(id, false, rules);
    }

    /// The two bodies' tails: the type's sound for a change of state, at the
    /// object's Location, then the latch.
    fn latch_deactivated(&mut self, id: u64, deactivated: bool, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.deactivated != deactivated
            && let Some(object) = self.object_type(entity.type_ref(), rules)
            && let Some(sound) = if deactivated {
                object.deactivate_sound.clone()
            } else {
                object.activate_sound.clone()
            }
        {
            self.sound_events
                .push(SimSoundEvent::voc_at(sound, &entity.position));
        }
        self.substrate
            .entities
            .get_mut(id)
            .expect("same object")
            .deactivated = deactivated;
    }

    /// `UnitClass::Per_Cell_Process`'s arm for a cell arrival
    /// (`0x00739F21..0x00739F59`): a `PoweredUnit=` type whose house has no
    /// powered center Deactivates when no building stands in its cell
    /// (`Look_up_building_in_cell @ 0x0047C520` on its cell's ground list).
    pub(crate) fn powered_unit_arrival(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if !self
            .object_type(entity.type_ref(), rules)
            .is_some_and(|object| object.powered_unit)
            || self.has_powered_center(entity.owner())
            || self
                .substrate
                .occupancy
                .first_building_on_layer(
                    entity.position.rx,
                    entity.position.ry,
                    MovementLayer::Ground,
                )
                .is_some()
        {
            return;
        }
        self.techno_deactivate(id, rules, effects);
    }

    /// `UnitClass::AI @ 0x007360C9..0x007361A4`, its first block: an offline
    /// Unit throws a `DefaultSparkSystem=` system on the frames where
    /// `x / 10 + (y / 10 << 16) + Frame + its ID` (Location, signed
    /// divisions, wrapping sums; `Fetch_ID` through `+4`, vt+0x10) is a
    /// multiple of 30. Two Scenario draws in `[-100, 100]` offset it, the
    /// first in Y and the second in X, at the Location's Z.
    pub(crate) fn deactivated_unit_sparks(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if !entity.deactivated {
            return;
        }
        let location = object_location(entity, self.resolved_terrain.as_ref());
        let phase = (location.x / 10)
            .wrapping_add((location.y / 10) << 16)
            .wrapping_add(self.session.binary_frame as i32)
            .wrapping_add(entity.native_unique_id);
        if phase % 30 != 0 {
            return;
        }
        let dy = self.scenario_rng.next_range_i32_inclusive(-100, 100);
        let dx = self.scenario_rng.next_range_i32_inclusive(-100, 100);
        let Some(system) = rules
            .combat_damage
            .default_spark_system
            .as_deref()
            .and_then(|name| rules.ps_type_id_by_name(name))
        else {
            return;
        };
        self.spawn_particle_system(
            system,
            glam::IVec3::new(
                location.x.wrapping_add(dx),
                location.y.wrapping_add(dy),
                location.z,
            ),
            None,
            None,
            glam::IVec3::ZERO,
            None,
            rules,
        );
    }
}

#[cfg(test)]
#[path = "powered_unit_tests.rs"]
mod tests;
