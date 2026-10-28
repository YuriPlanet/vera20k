//! Active Techno sensor-count deposit lifecycle.
//!
//! FootClass add/remove/move/owner writers are at 0x004D7318, 0x004DB376,
//! 0x004D8611/0x004D8621, and 0x004DBEFB/0x004DBF81. Building sensor arrays
//! use 0x00455820/0x004556D0 and deliberately add with `SensorsSight=` but
//! remove with the signed-byte `CloakRadiusInCells=` value. The building add
//! is gated on Is_Operational and the remove is not, so a building whose
//! opening stamped nothing still debits its owner when it leaves the map.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::Simulation;

fn unit_sensor_radius(entity_category: EntityCategory, object: &ObjectType) -> Option<u16> {
    if object.sensors_sight == 0
        || !matches!(
            entity_category,
            EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
        )
    {
        return None;
    }
    Some(u16::from(object.sensors_sight))
}

impl Simulation {
    fn unit_sensor_parameters(
        &self,
        id: u64,
        rules: &RuleSet,
    ) -> Option<(InternedId, (u16, u16), u16)> {
        let entity = self.substrate.entities.get(id)?;
        let object = self.entity_object_type(entity, rules)?;
        Some((
            entity.owner(),
            (entity.position.rx(), entity.position.ry()),
            unit_sensor_radius(entity.category(), object)?,
        ))
    }

    /// FootClass::RemoveSensorsAt @ 0x004DE940 reads the live type and
    /// owner. NullCell resolves current GetCoords; PerCell supplies +55C.
    /// No deposit-presence check may suppress the native counter operation.
    fn remove_unit_sensors_at(
        &mut self,
        id: u64,
        cell: Option<(i16, i16)>,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some((owner, current, radius)) = self.unit_sensor_parameters(id, rules) else {
            return;
        };
        let center = cell.map_or(current, |(x, y)| (x as u16, y as u16));
        self.apply_unit_sensor_remove(owner, center, radius, rules, registry);
    }

    /// FootClass::AddSensorsAt @ 0x004DE7B0 adds at the freshly sampled
    /// current cell. It neither removes an earlier add nor changes +55C.
    pub(crate) fn add_unit_sensors_at_current(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some((owner, center, radius)) = self.unit_sensor_parameters(id, rules) else {
            return;
        };
        self.apply_sensor_add(owner, center, radius, rules, registry);
    }

    /// Foot Limbo4DB376 and ChangeOwner4DBEFB remove at current coordinates,
    /// even when an airborne sensor circle was last added elsewhere.
    pub(crate) fn remove_unit_sensors_at_current(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        self.remove_unit_sensors_at(id, None, rules, registry);
    }
    fn sensor_residents_in_native_order(&self, cell: (u16, u16)) -> Vec<u64> {
        self.substrate
            .occupancy
            .get(cell.0, cell.1)
            .map(|occupancy| {
                occupancy
                    .iter_layer(MovementLayer::Ground)
                    .filter_map(|occupant| {
                        self.substrate
                            .entities
                            .get(occupant.entity_id)
                            .is_some_and(|entity| {
                                matches!(
                                    entity.category(),
                                    EntityCategory::Unit
                                        | EntityCategory::Infantry
                                        | EntityCategory::Aircraft
                                )
                            })
                            .then_some(occupant.entity_id)
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn sensor_reevaluate_residents(
        &mut self,
        cell: (u16, u16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Vec<u64> {
        // Add/RemoveSensorsAt @ 0x004DE7B0/0x004DE940 and the BuildingClass
        // variants walk CellClass::FirstObject head-to-tail after mutating this
        // exact cell. OccupancyGrid's ground list is that authoritative order.
        let residents = self.sensor_residents_in_native_order(cell);
        for &stable_id in &residents {
            crate::sim::world::techno_ai_cloak::sensor_reevaluate_stock_cloak(
                self, stable_id, rules, registry,
            );
        }
        residents
    }

    fn apply_sensor_add(
        &mut self,
        owner: InternedId,
        center: (u16, u16),
        radius: u16,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Vec<u64> {
        let mut callbacks = Vec::new();
        for cell in self.fog.sensor_circle_cells(center, radius) {
            self.fog.increment_sensor_at(owner, cell.0, cell.1);
            callbacks.extend(self.sensor_reevaluate_residents(cell, rules, registry));
        }
        callbacks
    }

    fn apply_unit_sensor_remove(
        &mut self,
        owner: InternedId,
        center: (u16, u16),
        radius: u16,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Vec<u64> {
        let mut callbacks = Vec::new();
        for cell in self.fog.sensor_circle_cells(center, radius) {
            // RemoveSensorsAt @ 0x004DE940 skips both mutation and callbacks
            // when the signed pre-count is not positive.
            if self
                .fog
                .decrement_sensor_at_if_positive(owner, cell.0, cell.1)
            {
                callbacks.extend(self.sensor_reevaluate_residents(cell, rules, registry));
            }
        }
        callbacks
    }

    fn apply_building_sensor_remove(
        &mut self,
        owner: InternedId,
        center: (u16, u16),
        radius: u16,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Vec<u64> {
        let mut callbacks = Vec::new();
        for cell in self.fog.sensor_circle_cells(center, radius) {
            self.fog
                .decrement_sensor_at_unconditional(owner, cell.0, cell.1);
            callbacks.extend(self.sensor_reevaluate_residents(cell, rules, registry));
        }
        callbacks
    }

    /// `FootClass::Unlimbo` sensor writer. A non-Foot object or a type without
    /// a sensor radius makes no deposit.
    /// `FootClass::Unlimbo @ 0x004D7170`'s sensor add (vt+0x4E8 at `0x004D7318`), at
    /// the cell of the object it took out of limbo.
    pub(crate) fn add_unit_sensor_after_unlimbo(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| !entity.lifecycle.in_limbo())
        {
            self.add_unit_sensors_at_current(stable_id, rules, registry);
        }
    }

    /// `BuildingClass::Place @ 0x00445F80`'s sensor arm, inside its first-opening
    /// block (`0x00445FD0` skips it for a placed building, so a capture's
    /// `Place(1)` stamps nothing): `SensorArray=` (`Type+0x16C8`, `0x0044678B`)
    /// dispatches vt+0x4F4 `AddSensorArrayAt @ 0x00455820` at `0x004467A1`, then
    /// `DetectDisguise=` (`+0xD31`, `0x004467AD`) vt+0x4FC
    /// `AddDetectDisguiseAt @ 0x00455A80` at `0x004467C3`. These are the only
    /// dispatches of either slot. Place passes the empty cell (`0x0089C818`),
    /// so both centre on vt+0x48 `BuildingClass::GetCoords @ 0x00447AC0`, the
    /// foundation centre (`0x0045587F`, `0x00455ADF`).
    ///
    /// Both adds open with vt+0x350 `Is_Operational_For_Output @ 0x004555D0`
    /// (`0x0045582A`, `0x00455A8A`) and stamp nothing when it refuses, so any
    /// first opening that passes Is_Operational stamps. It refuses an effective
    /// Construction mission, which is current when Mission_Construction opens a
    /// built building (`0x00449AD4`), so a built one never stamps. Unlimbo's
    /// CellPUT `DiscoveredBy(PlayerPtr)` runs before Added_To_Game
    /// (`0x006F6D8F`) and Enter_Idle_Mode (`0x006F6E2A`), so a human-owned
    /// SpawnBuilding or a human map building read before any other draining
    /// building of its house can pass too. The resident `+0x420` callbacks of
    /// the sensor add cannot change the building's own operational facts, so
    /// one sample serves both adds.
    ///
    /// Stock authors: NAPSIS is the only building that stamps a disguise-detect
    /// circle, because `DetectDisguiseRange=` defaults to zero
    /// (`TechnoTypeClass::Constructor @ 0x0071100E`) and YAPSYT / NAPSYB carry
    /// `DetectDisguise=yes` without a range.
    pub(crate) fn add_building_sensor_circles_at_opening(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some((owner, center, sight, detect_radius)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                let object = self.interner.object_type(entity.type_ref(), rules)?;
                if entity.category() != EntityCategory::Structure
                    || object.category != ObjectCategory::Building
                {
                    return None;
                }
                Some((
                    entity.owner(),
                    crate::sim::world::object_get_coords_cell(entity)?,
                    object
                        .sensor_array
                        .then_some(u16::from(object.sensors_sight)),
                    object
                        .detect_disguise
                        .then_some(u16::from(object.detect_disguise_range)),
                ))
            })
        else {
            return;
        };
        if (sight.is_none() && detect_radius.is_none())
            || self.building_operational_state(stable_id, rules) != Some(true)
        {
            return;
        }
        if let Some(sight) = sight {
            self.apply_sensor_add(owner, center, sight, rules, registry);
        }
        if let Some(radius) = detect_radius {
            // The disguise-detect walk issues no resident `+0x420` callback —
            // it only increments `CellClass+0xAC[house]`.
            self.fog.disguise_detect_add_at(owner, center, radius);
        }
    }

    /// `BuildingClass::Limbo @ 0x00445880`'s sensor removals, reached only for a
    /// building not already in limbo (`0x004458C6` reads `+0x81`) and outside
    /// the placement scope (`0x00445A2A` tests `0xA8E7AC`): `SensorArray=`
    /// dispatches vt+0x4F8 `RemoveSensorArrayAt @ 0x004556D0` at `0x00445A4C`,
    /// then `DetectDisguise=` vt+0x500 `RemoveDetectDisguiseAt @ 0x00455980` at
    /// `0x00445A6C`, both centred on GetCoords like the adds (`0x0045571C`,
    /// `0x004559CB`). Neither has the add's Is_Operational gate or any record of
    /// a stamp, and both decrement with a bare signed-word `DEC` (`0x00487160`,
    /// `0x00487180`) — no clamp. A building that stamped nothing (any built
    /// one) therefore leaves its owner at -1 over `CloakRadiusInCells=` (signed
    /// byte `Type+0x1707`, read live) and over `DetectDisguiseRange=`.
    ///
    /// Every remove takes its house from `this->Owner->ArrayIndex` at removal
    /// time (`0x004556DA`, `0x0045598A`). BuildingChangeOwner448260 and
    /// Techno7014A0 dispatch no sensor slot, so after a capture the first owner
    /// keeps its circles and Limbo debits the captor. Preserve that native
    /// counter asymmetry.
    pub(crate) fn remove_building_sensor_before_limbo(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        if self.object_placement_scope_active() {
            return;
        }
        let Some((owner, center, cloak_radius, detect_radius)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                if entity.category() != EntityCategory::Structure || entity.lifecycle.in_limbo() {
                    return None;
                }
                let object = self.interner.object_type(entity.type_ref(), rules)?;
                Some((
                    entity.owner(),
                    crate::sim::world::object_get_coords_cell(entity)?,
                    object.sensor_array.then_some(object.cloak_radius_in_cells),
                    object
                        .detect_disguise
                        .then_some(u16::from(object.detect_disguise_range)),
                ))
            })
        else {
            return;
        };
        if let Some(radius) = cloak_radius {
            self.apply_building_sensor_remove(owner, center, radius.max(0) as u16, rules, registry);
        }
        if let Some(radius) = detect_radius {
            // No resident callback, as for the add.
            self.fog.disguise_detect_remove_at(owner, center, radius);
        }
    }

    /// Foot4D8611/4D8621 executes both receivers for PerCellProcess(2),
    /// including a terminal or chain callback in the same cell.
    pub fn refresh_unit_sensor_at_per_cell(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let old = self
            .substrate
            .entities
            .get(stable_id)
            .map(|entity| entity.navigation.neighbor_state().cell());
        if let Some(old) = old {
            self.remove_unit_sensors_at(stable_id, Some(old), rules, registry);
            self.add_unit_sensors_at_current(stable_id, rules, registry);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::{EntityCategory, MapEntity};
    use crate::map::playfield::PlayfieldBounds;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::BuildingUp;

    fn rules() -> RuleSet {
        RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(
                "[General]\nCloakingStages=9\nCloakDelay=.02\n\
             [AudioVisual]\nCloakSound=NavalUnitEmerge\n\
             [VehicleTypes]\n0=DEST\n1=SUB\n2=SQD\n3=TGT\n\
             [BuildingTypes]\n0=NAPSIS\n1=NAPOWR\n\
             [DEST]\nStrength=600\nSpeed=6\nSensorsSight=8\n\
             [SUB]\nStrength=600\nSpeed=4\nCloakable=yes\nCloakingSpeed=1\nSensorsSight=7\n\
             [SQD]\nStrength=600\nSpeed=4\nCloakable=yes\nCloakingSpeed=5\nSensorsSight=8\n\
             [TGT]\nStrength=600\nSpeed=4\nCloakable=yes\nCloakingSpeed=1\n\
             [NAPSIS]\nStrength=750\nSensorArray=yes\nSensorsSight=15\nPower=-100\nPowered=yes\n\
             DetectDisguise=yes\nDetectDisguiseRange=15\nCapturable=true\n\
             [NAPOWR]\nStrength=750\nPower=200\n",
            ),
            // Retail ARTMD.INI's NAPSIS foundation.
            &IniFile::from_str("[NAPSIS]\nFoundation=2x2\n"),
        )
        .expect("cloak/sensor rules")
    }

    fn sim_with_map_authority() -> Simulation {
        let mut sim = Simulation::with_seed(0x5E45_0A00);
        sim.fog.set_width_for_test(64);
        sim.fog.set_height_for_test(64);
        sim.set_playfield_bounds_for_test(Some(PlayfieldBounds::from_normalized_local_size(
            64, 2, 2, 56, 52,
        )));
        sim
    }

    #[test]
    fn unit_sensor_unlimbo_move_owner_limbo_and_overlap_use_native_callbacks() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        let first = sim
            .spawn_object_at_height("DEST", "Americans", 40, 30, 0, 0, &rules)
            .unwrap();
        let second = sim
            .spawn_object_at_height("DEST", "Americans", 40, 30, 0, 0, &rules)
            .unwrap();
        let americans = sim.substrate.entities.get(first).unwrap().owner();
        let soviet = sim.interner.intern("Soviet");
        assert!(sim.fog.has_sensor_for_house(americans, 33, 30));
        assert!(!sim.fog.has_sensor_for_house(americans, 48, 30));

        sim.techno_limbo_with_rules(first, &rules, None);
        assert!(
            sim.fog.has_sensor_for_house(americans, 33, 30),
            "the overlapping second deposit remains positive"
        );
        let counters = sim.fog.sensors_by_house().clone();
        sim.techno_limbo_with_rules(first, &rules, None);
        assert_eq!(
            sim.fog.sensors_by_house(),
            &counters,
            "repeated Limbo must not debit the second unit's positive coverage"
        );

        sim.substrate
            .entities
            .get_mut(second)
            .unwrap()
            .position
            .set_rx_for_test(50);
        sim.refresh_unit_sensor_at_per_cell(second, &rules, None);
        assert!(!sim.fog.has_sensor_for_house(americans, 33, 30));
        assert!(sim.fog.has_sensor_for_house(americans, 57, 30));

        sim.change_owner_with_rules(
            second,
            soviet,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert!(!sim.fog.has_sensor_for_house(americans, 57, 30));
        assert!(sim.fog.has_sensor_for_house(soviet, 57, 30));

        // FootLimbo4DB376 removes the live owner's current-coordinate circle.
        sim.techno_limbo_with_rules(second, &rules, None);
        assert!(!sim.fog.has_sensor_for_house(soviet, 57, 30));
    }

    /// A computer house's 2x2 NAPSIS read from a map's `[Structures]` at
    /// (40, 40), so its GetCoords cell is (41, 41), with its house's power
    /// already assessed full. Its Unlimbo under the placement scope queues
    /// Guard (`0x0044D6DC..0x0044D6F1`), so the owner's discovery opening
    /// (`0x00440E76` -> `0x0044D68A`) passes Is_Operational. A retail scenario
    /// read has no such assessment: the building's own drain joins its house at
    /// `0x00502B4A` while PowerOutput stays 0 until House Update, so a Powered
    /// sensor building stamps only if its first opening passes Is_Operational
    /// anyway, as a human one discovered by CellPUT before its own
    /// Added_To_Game does while its house has no other drain.
    fn map_placed_napsis(sim: &mut Simulation, rules: &RuleSet) -> u64 {
        for name in ["Soviet", "Americans"] {
            let house = sim.interner.intern(name);
            sim.register_house(crate::sim::house_state::HouseState::new(
                house, 0, None, false, 0, 10,
            ));
        }
        sim.spawn_object_at_height("NAPOWR", "Soviet", 30, 40, 0, 0, rules)
            .unwrap();
        sim.assess_power_for_test(rules);
        let napsis = MapEntity {
            owner: "Soviet".into(),
            type_id: "NAPSIS".into(),
            health: 256,
            cell_x: 40,
            cell_y: 40,
            facing: 0,
            category: EntityCategory::Structure,
            sub_cell: 0,
            veterancy: None,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            group: -1,
            structure_upgrades: [None, None, None],
            structure_ai_sellable: false,
            structure_ai_repairable: false,
            structure_online: true,
            trigger_tag: None,
        };
        assert_eq!(sim.spawn_from_map(&[napsis], Some(rules)), 1);
        sim.entities()
            .values()
            .find(|entity| sim.interner.resolve(entity.type_ref()) == "NAPSIS")
            .unwrap()
            .stable_id()
    }

    /// `AddSensorArrayAt @ 0x00455820` and `AddDetectDisguiseAt @ 0x00455A80`
    /// refuse while Is_Operational (`0x004555D0`) sees Construction, which is
    /// the current mission when Mission_Construction opens the building
    /// (`0x00449AD4`). Limbo's removals (`0x004556D0`, `0x00455980`) read the
    /// type, not the add, and decrement without a clamp around the foundation
    /// centre (41, 41).
    #[test]
    fn napsis_construction_stamps_nothing_yet_limbo_still_debits_both_circles() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        sim.spawn_object_at_height("NAPOWR", "Soviet", 30, 40, 0, 0, &rules)
            .unwrap();
        let id = sim
            .spawn_object_limbo_at_height("NAPSIS", "Soviet", 40, 40, 0, 0, &rules)
            .unwrap();
        let owner = sim.substrate.entities.get(id).unwrap().owner();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .install_building_up(BuildingUp::completing_in_ticks(1, 0), 0);
        assert!(
            sim.reveal_constructed_object_at_height(
                id,
                40,
                40,
                0,
                0,
                crate::sim::world::PlacementEvidence::EvaluateMark,
                &rules,
                crate::sim::world::FrameEffects::default(),
            )
            .is_some()
        );
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .building_art
                .actually_placed()
        );
        assert!(!sim.fog.has_sensor_for_house(owner, 41, 41));
        assert!(!sim.fog.detects_disguise_for_house(owner, 41, 41));

        sim.techno_limbo_with_rules(id, &rules, None);
        let row = 41 * usize::from(sim.fog.width());
        // `CloakRadiusInCells=` defaults to 20; `DetectDisguiseRange=15`.
        assert_eq!(sim.fog.sensors_by_house()[&owner][row + 60], -1);
        assert_eq!(sim.fog.sensors_by_house()[&owner][row + 61], 0);
        assert_eq!(sim.fog.disguise_detect_by_house()[&owner][row + 55], -1);
        assert_eq!(sim.fog.disguise_detect_by_house()[&owner][row + 56], 0);
    }

    /// ReadScenarioBuildings (`0x0044F820`) adds no circle of its own; the map
    /// building's one stamp is its discovery opening's, centred on GetCoords
    /// (`0x00447AC0`): the 2x2 foundation's centre cell (41, 41), not its
    /// north-west cell.
    #[test]
    fn map_placed_napsis_stamps_both_circles_once_around_its_foundation_centre() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        let id = map_placed_napsis(&mut sim, &rules);
        let owner = sim.substrate.entities.get(id).unwrap().owner();
        let width = usize::from(sim.fog.width());
        let row = 41 * width;
        let sensors = &sim.fog.sensors_by_house()[&owner];
        // `SensorsSight=15` spans x 27..=55 on the centre row.
        assert_eq!(sensors[row + 26], 0);
        assert_eq!(sensors[row + 27], 1);
        assert_eq!(sensors[row + 55], 1);
        assert_eq!(sensors[row + 56], 0);
        assert_eq!(sensors[55 * width + 41], 1);
        assert_eq!(sensors[56 * width + 41], 0);
        let disguise = &sim.fog.disguise_detect_by_house()[&owner];
        assert_eq!(disguise[row + 27], 1);
        assert_eq!(disguise[row + 55], 1);
        assert_eq!(disguise[row + 56], 0);
    }

    /// Capture, then sell, end to end — the permanent counter skew gamemd
    /// leaves behind, on BOTH planes.
    ///
    /// Adds credit `*(int *)(param_1[0x87] + 0x30)` at add time
    /// (`BuildingClass::AddSensorArrayAt @ 0x00455820`,
    /// `AddDetectDisguiseAt @ 0x00455A80`); `BuildingClass::ChangeOwner @
    /// 0x00448260` and the `TechnoClass::ChangeOwner @ 0x007014A0` it calls
    /// dispatch no sensor slot, and its `Place(captured)` at `0x00448CEF`
    /// skips the sensor arm (`0x00445FD0`), but `0x007014A0` does overwrite
    /// `+0x21C`; and Limbo's removes (`0x004556D0`, `0x00455980`) re-read
    /// `+0x21C` live. Net: the first owner keeps `SensorsSight=15` and
    /// `DetectDisguiseRange=15`, the captor takes `-1` over the
    /// `CloakRadiusInCells=20` sensor circle and over the 15-cell disguise
    /// circle.
    #[test]
    fn selling_a_captured_psychic_sensor_debits_the_captor_and_leaves_the_first_owner_detecting() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        let id = map_placed_napsis(&mut sim, &rules);
        let soviet = sim.substrate.entities.get(id).unwrap().owner();

        let americans = sim.interner.intern("Americans");
        sim.change_owner_with_rules(
            id,
            americans,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );
        assert!(
            sim.fog.has_sensor_for_house(soviet, 55, 41)
                && sim.fog.detects_disguise_for_house(soviet, 55, 41),
            "the capture moves neither circle off the first owner"
        );
        assert!(
            !sim.fog.has_sensor_for_house(americans, 55, 41)
                && !sim.fog.detects_disguise_for_house(americans, 55, 41),
            "nor grants the captor one"
        );
        sim.techno_limbo_with_rules(id, &rules, None);

        let row = 41 * usize::from(sim.fog.width());
        let inside = row + 55; // inside both the 15-cell add and the 20-cell remove
        let fringe = row + 56; // outside the add, inside the sensor remove only

        assert_eq!(
            sim.fog.sensors_by_house()[&soviet][inside],
            1,
            "the first owner's SensorsSight credit is never returned"
        );
        assert_eq!(
            sim.fog.sensors_by_house()[&americans][inside],
            -1,
            "0x004556D0 debits the live owner over CloakRadiusInCells"
        );
        assert_eq!(sim.fog.sensors_by_house()[&soviet][fringe], 0);
        assert_eq!(
            sim.fog.sensors_by_house()[&americans][fringe],
            -1,
            "the asymmetric remove radius reaches past the add circle too"
        );
        assert!(
            sim.fog.has_sensor_for_house(soviet, 55, 41),
            "the first owner keeps submarine detection over ground it no longer owns"
        );
        assert!(!sim.fog.has_sensor_for_house(americans, 55, 41));

        assert_eq!(sim.fog.disguise_detect_by_house()[&soviet][inside], 1);
        assert_eq!(
            sim.fog.disguise_detect_by_house()[&americans][inside],
            -1,
            "the disguise circle is symmetric in radius but not in house"
        );
        assert!(sim.fog.detects_disguise_for_house(soviet, 55, 41));
        assert!(
            !sim.fog.detects_disguise_for_house(americans, 55, 41),
            "a later NAPSIS the captor owns here needs two increments to see again"
        );
    }

    #[test]
    fn sensor_callbacks_use_firstobject_order_and_unit_vs_building_remove_gates() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        let older = sim
            .spawn_object_at_height("TGT", "Soviet", 40, 30, 0, 0, &rules)
            .unwrap();
        let newer = sim
            .spawn_object_at_height("TGT", "Soviet", 40, 30, 0, 0, &rules)
            .unwrap();
        let soviet = sim.substrate.entities.get(older).unwrap().owner();
        let detector = sim.interner.intern("Americans");
        // CORRECTION: this used to reveal the residents' own cell to their own
        // house, on the reading that `CellClass::IsVisibleToHouse @ 0x004870B0`
        // meant cell visibility. It is the `CloakedByHouses` bit, whose only
        // writers sit in `BuildingClass::UpdateGapGenerator_Tick` behind
        // `CloakGenerator=`, so the vt+0x420 cloak arm is dormant in stock YR.
        // The FirstObject dispatch ORDER is what this test owns, and it is
        // observable from the returned resident list alone.
        sim.fog.mark_visible_for_owner(soviet, 40, 30);
        for id in [older, newer] {
            let cloak = sim
                .substrate
                .entities
                .get_mut(id)
                .unwrap()
                .cloak
                .as_mut()
                .unwrap();
            cloak.set_state_for_test(0);
        }

        let added = sim.apply_sensor_add(detector, (40, 30), 1, &rules, None);
        assert_eq!(
            added,
            vec![newer, older],
            "non-Buildings prepend to FirstObject"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(newer)
                .unwrap()
                .cloak
                .as_ref()
                .unwrap()
                .state(),
            0,
            "no CloakGenerator field means the vt+0x420 cloak arm never fires"
        );
        assert!(
            sim.sound_events.is_empty(),
            "a dormant cloak arm emits no CloakSound"
        );

        for id in [older, newer] {
            let cloak = sim
                .substrate
                .entities
                .get_mut(id)
                .unwrap()
                .cloak
                .as_mut()
                .unwrap();
            cloak.set_state_for_test(0);
        }
        let removed = sim.apply_unit_sensor_remove(detector, (40, 30), 1, &rules, None);
        assert_eq!(removed, vec![newer, older]);
        assert!(sim.sound_events.is_empty());
        assert!(
            sim.apply_unit_sensor_remove(detector, (40, 30), 1, &rules, None)
                .is_empty(),
            "unit RemoveSensorsAt skips decrement and callbacks at nonpositive pre-count"
        );
        assert!(sim.sound_events.is_empty());

        let building_removed =
            sim.apply_building_sensor_remove(detector, (40, 30), 1, &rules, None);
        assert_eq!(building_removed, vec![newer, older]);
        let index = 30 * usize::from(sim.fog.width()) + 40;
        assert_eq!(
            sim.fog.sensors_by_house()[&detector][index],
            -1,
            "BuildingClass removal is unconditional and signed"
        );
    }

    #[test]
    fn authored_scope_and_runtime_unlimbo_playfield_gate_are_distinct() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        let bounds = sim.playfield_bounds().unwrap();
        let inside = (0u16..64)
            .flat_map(|ry| (0u16..64).map(move |rx| (rx, ry)))
            .find(|&(rx, ry)| bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
            .expect("mode-one inside cell");
        let outside = (0u16..64)
            .flat_map(|ry| (0u16..64).map(move |rx| (rx, ry)))
            .find(|&(rx, ry)| !bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
            .expect("mode-one outside cell");
        sim.spawn_from_map(
            &[
                MapEntity {
                    owner: "Soviet".into(),
                    type_id: "SUB".into(),
                    health: 256,
                    cell_x: inside.0,
                    cell_y: inside.1,
                    facing: 0,
                    category: EntityCategory::Unit,
                    sub_cell: 0,
                    veterancy: Some(0),
                    high: false,
                    mission: None,
                    recruitable_a: true,
                    recruitable_b: true,
                    group: -1,
                    structure_upgrades: [None, None, None],
                    structure_ai_sellable: false,
                    structure_ai_repairable: false,
                    structure_online: true,
                    trigger_tag: None,
                },
                MapEntity {
                    owner: "Soviet".into(),
                    type_id: "SUB".into(),
                    health: 256,
                    cell_x: outside.0,
                    cell_y: outside.1,
                    facing: 0,
                    category: EntityCategory::Unit,
                    sub_cell: 0,
                    veterancy: Some(0),
                    high: false,
                    mission: None,
                    recruitable_a: true,
                    recruitable_b: true,
                    group: -1,
                    structure_upgrades: [None, None, None],
                    structure_ai_sellable: false,
                    structure_ai_repairable: false,
                    structure_online: true,
                    trigger_tag: None,
                },
            ],
            Some(&rules),
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .cloak
                .as_ref()
                .unwrap()
                .state(),
            0
        );
        // ReadScenario/FullInit keep A8E7AC nonzero through authored
        // readers, so Object5F4F1B skips the ordinary class gate. The
        // successful Techno6F6CFE tail still establishes mode-one membership.
        let authored_outside = sim.substrate.entities.get(2).unwrap();
        assert!(!authored_outside.in_playfield());
        assert!(!authored_outside.lifecycle.in_limbo());
        assert!(authored_outside.lifecycle.cell_marked());

        let inside = sim
            .spawn_object_at_height("SUB", "Soviet", inside.0, inside.1, 0, 0, &rules)
            .unwrap();
        let outside =
            sim.spawn_object_at_height("SUB", "Soviet", outside.0, outside.1, 0, 0, &rules);
        assert_eq!(
            sim.substrate
                .entities
                .get(inside)
                .unwrap()
                .cloak
                .as_ref()
                .unwrap()
                .state(),
            0
        );
        assert!(
            outside.is_none(),
            "runtime outside-playfield Unit must fail Unlimbo"
        );
    }
}
