//! Active Techno sensor-count deposit lifecycle.
//!
//! FootClass add/remove/move/owner writers are at 0x004D7318, 0x004DB376,
//! 0x004D8611/0x004D8621, and 0x004DBEFB/0x004DBF81. Building sensor arrays
//! use 0x00455820/0x004556D0 and deliberately add with `SensorsSight=` but
//! remove with the signed-byte `CloakRadiusInCells=` value.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::Simulation;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SensorDeposit {
    /// The house the circle was stamped FOR — native reads
    /// `this->Owner->ArrayIndex` live at add time and has no such cache. It is
    /// a record of the add, not the removal's authority: every native remove
    /// re-reads the live owner (`0x00455980`, `0x004556D0`, `0x004DE940`), so
    /// `remove_cached_sensor_deposit` must not consult this field.
    owner: InternedId,
    center: (u16, u16),
    add_radius: u16,
    remove_radius: i16,
    building_array: bool,
    /// `DetectDisguiseRange=` (`TechnoTypeClass+0x5F4`) circle stamped by
    /// `BuildingClass::AddDetectDisguiseAt @ 0x00455A80` alongside the sensor
    /// array. Unlike the sensor array, its paired
    /// `RemoveDetectDisguiseAt @ 0x00455980` reads the SAME field back
    /// (`0x00455991` loads `+0x5F4`), so add and remove are symmetric and one
    /// radius suffices.
    #[serde(default)]
    detect_disguise_radius: u16,
}

impl SensorDeposit {
    /// The house the circle was stamped for.
    pub(crate) fn owner(&self) -> InternedId {
        self.owner
    }

    /// The latest add's cell. Native removals use their caller's cell, which
    /// can differ after flight or owner change; this is not removal authority.
    pub(crate) fn center(&self) -> (u16, u16) {
        self.center
    }

    /// The add radius.
    pub(crate) fn add_radius(&self) -> u16 {
        self.add_radius
    }

    /// The remove radius.
    pub(crate) fn remove_radius(&self) -> i16 {
        self.remove_radius
    }

    /// A building's sensor array, not a unit's.
    pub(crate) fn building_array(&self) -> bool {
        self.building_array
    }

    /// The `DetectDisguiseRange=` circle's radius.
    pub(crate) fn detect_disguise_radius(&self) -> u16 {
        self.detect_disguise_radius
    }

    /// Install a fixture's stamped cell.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_center_for_test(&mut self, center: (u16, u16)) {
        self.center = center;
    }

    /// A unit sensor array's deposit: one radius for the add and the remove.
    pub(crate) fn unit(owner: InternedId, center: (u16, u16), radius: u16) -> Self {
        Self {
            owner,
            center,
            add_radius: radius,
            remove_radius: radius as i16,
            building_array: false,
            detect_disguise_radius: 0,
        }
    }

    fn building(
        owner: InternedId,
        center: (u16, u16),
        add: u16,
        remove: i8,
        detect_disguise_radius: u16,
    ) -> Self {
        Self {
            owner,
            center,
            add_radius: add,
            remove_radius: i16::from(remove),
            building_array: true,
            detect_disguise_radius,
        }
    }
}

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
        let object = self.interner.object_type(entity.type_ref(), rules)?;
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
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.sensor_deposit = None;
        }
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
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.sensor_deposit = Some(SensorDeposit::unit(owner, center, radius));
        }
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

    /// Every native remove takes its house index from `this->Owner->ArrayIndex`
    /// at REMOVAL time, never from anything recorded when the circle was
    /// stamped. `BuildingClass::RemoveDetectDisguiseAt @ 0x00455980` opens with
    /// `0045598A MOV EDX,[ECX+0x21C]` / `004559A1 MOV EAX,[EDX+0x30]`;
    /// `BuildingClass::RemoveSensorArrayAt @ 0x004556D0` and
    /// `FootClass::RemoveSensorsAt @ 0x004DE940` open with the identical
    /// `*(int *)(param_1[0x87] + 0x30)` read, and so do both adds
    /// (`0x00455820`, `0x00455A80`). `TechnoClass::ChangeOwner @ 0x007014A0`
    /// overwrites that pointer (`param_1[0x87] = param_2`), so the live owner
    /// is whoever holds the object now.
    ///
    /// This cached footprint belongs to the Building path. Foot callers use
    /// their own explicit/current cell and live radius instead. For BUILDINGS
    /// the recorded and live owner can diverge permanently:
    /// construction credits the builder, `ChangeOwner` dispatches neither add
    /// nor remove, and Limbo debits the CAPTOR. Its old owner keeps phantom
    /// coverage and its captor can acquire negative array/disguise counts.
    fn remove_cached_sensor_deposit(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Option<SensorDeposit> {
        let (deposit, remove_owner) =
            self.substrate
                .entities
                .get_mut(stable_id)
                .and_then(|entity| {
                    let deposit = entity.sensor_deposit.take()?;
                    Some((deposit, entity.owner()))
                })?;
        if deposit.detect_disguise_radius > 0 {
            // `BuildingClass::RemoveDetectDisguiseAt @ 0x00455980`, reached
            // ONLY from `BuildingClass::Limbo` — `0x00445A58` reads
            // `TechnoTypeClass+0xD31` (`DetectDisguise=`) and `0x00445A6C`
            // dispatches vtable `+0x500`. It walks the same circle it stamped
            // and decrements `CellClass+0xAC[house]`; no resident callback is
            // issued. `search_instructions "call [..+0x500]"` returns that one
            // BuildingClass site program-wide, which is why the owner-change
            // path below must not come through here.
            self.fog.disguise_detect_remove_at(
                remove_owner,
                deposit.center,
                deposit.detect_disguise_radius,
            );
        }
        if deposit.building_array {
            // A building that stamped only a disguise-detect circle never added
            // sensor counts, so it must not run the asymmetric
            // `CloakRadiusInCells=` decrement and manufacture negative fringe
            // counts. VERA-internal guard; gamemd dispatches the two removals
            // from separate vtable slots and the pairing is UNCHECKED, but no
            // stock building reaches this case (NAPSIS carries both keys).
            if deposit.add_radius > 0 {
                self.apply_building_sensor_remove(
                    remove_owner,
                    deposit.center,
                    deposit.remove_radius.max(0) as u16,
                    rules,
                    registry,
                );
            }
        } else {
            self.apply_unit_sensor_remove(
                remove_owner,
                deposit.center,
                deposit.remove_radius.max(0) as u16,
                rules,
                registry,
            );
        }
        Some(deposit)
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

    /// `BuildingClass::AddSensorArrayAt @ 0x00455820` and its sibling
    /// `BuildingClass::AddDetectDisguiseAt @ 0x00455A80`, used only after map
    /// initialization or construction completion and only while powered — both
    /// open with the same `vt+0x350` powered test, and
    /// `BuildingClass::OnConstructionComplete` dispatches them from adjacent
    /// slots: `+0x4F4` at `0x004467A1` (sensor array) and `+0x4FC` at
    /// `0x004467C3` (disguise circle), the latter reached only after
    /// `0x004467AD` reads `TechnoTypeClass+0xD31` (`DetectDisguise=`). The
    /// BuildingClass vtable block at `0x007E43B0` is
    /// `[+0x4F4]=0x00455820` add, `[+0x4F8]=0x004556D0` remove,
    /// `[+0x4FC]=0x00455A80` disguise add, `[+0x500]=0x00455980` disguise
    /// remove — `+0x4F8` is the sensor REMOVE, dispatched from
    /// `BuildingClass::Limbo @ 0x00445A4C`.
    ///
    /// Stock authors: NAPSIS is the only building that stamps a disguise-detect
    /// circle, because `DetectDisguiseRange=` defaults to zero
    /// (`TechnoTypeClass::Constructor @ 0x0071100E`) and YAPSYT / NAPSYB carry
    /// `DetectDisguise=yes` without a range.
    pub(crate) fn add_building_sensor_array_if_powered(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some((owner, center, sight, remove_radius, detect_radius, powered, in_limbo)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                let object = self.interner.object_type(entity.type_ref(), rules)?;
                if entity.category() != EntityCategory::Structure
                    || object.category != ObjectCategory::Building
                {
                    return None;
                }
                let sight = if object.sensor_array {
                    u16::from(object.sensors_sight)
                } else {
                    0
                };
                let detect_radius = if object.detect_disguise {
                    u16::from(object.detect_disguise_range)
                } else {
                    0
                };
                if sight == 0 && detect_radius == 0 {
                    return None;
                }
                Some((
                    entity.owner(),
                    (entity.position.rx(), entity.position.ry()),
                    sight,
                    object.cloak_radius_in_cells,
                    detect_radius,
                    crate::sim::power_system::is_building_powered(
                        self.power_states.as_map(),
                        rules,
                        entity,
                        &self.interner,
                    ),
                    entity.lifecycle.in_limbo(),
                ))
            })
        else {
            return;
        };
        if !powered || in_limbo {
            return;
        }
        let _ = self.remove_cached_sensor_deposit(stable_id, rules, registry);
        if sight > 0 {
            self.apply_sensor_add(owner, center, sight, rules, registry);
        }
        if detect_radius > 0 {
            // The disguise-detect walk issues no resident `+0x420` callback —
            // it only increments `CellClass+0xAC[house]`.
            self.fog
                .disguise_detect_add_at(owner, center, detect_radius);
        }
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.sensor_deposit = Some(SensorDeposit::building(
                owner,
                center,
                sight,
                remove_radius,
                detect_radius,
            ));
        }
    }

    /// Building Limbo retains its separate sensor-array/disguise footprints.
    /// BuildingChangeOwner448260/Techno7014A0 dispatch no Building sensor
    /// slots: the builder keeps both circles, and Limbo debits the captor's
    /// live owner. Preserve that native counter asymmetry.
    pub(crate) fn remove_building_sensor_before_limbo(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        if self
            .substrate
            .entities
            .get(stable_id)
            .is_some_and(|entity| entity.category() == EntityCategory::Structure)
        {
            let _ = self.remove_cached_sensor_deposit(stable_id, rules, registry);
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
        RuleSet::from_ini(&IniFile::from_str(
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
        ))
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

    #[test]
    fn napsis_construction_adds_radius_fifteen_but_limbo_removes_default_twenty() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        sim.spawn_object_at_height("NAPOWR", "Soviet", 30, 40, 0, 0, &rules)
            .unwrap();
        let id = sim
            .spawn_object_limbo_at_height("NAPSIS", "Soviet", 40, 40, 0, 0, &rules)
            .unwrap();
        let owner = sim.substrate.entities.get(id).unwrap().owner();
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .sensor_deposit
                .is_none()
        );
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
        assert!(!sim.substrate.entities.get(id).unwrap().building_up());
        assert!(sim.fog.has_sensor_for_house(owner, 54, 40));
        assert!(!sim.fog.has_sensor_for_house(owner, 55, 40));
        sim.techno_limbo_with_rules(id, &rules, None);
        let index = 40 * usize::from(sim.fog.width()) + 55;
        assert_eq!(sim.fog.sensors_by_house()[&owner][index], -1);
    }

    /// Engineer capture of a Psychic Sensor moves NEITHER stamped circle.
    ///
    /// `BuildingClass::ChangeOwner @ 0x00448260` and the
    /// `TechnoClass::ChangeOwner @ 0x007014A0` it calls at `0x00448BE8`
    /// dispatch none of the four BuildingClass sensor slots; program-wide,
    /// `+0x4F4`/`+0x4FC` are dispatched only by `OnConstructionComplete`
    /// (`0x004467A1` / `0x004467C3`) and `+0x4F8`/`+0x500` only by `Limbo`
    /// (`0x00445A4C` / `0x00445A6C`). So the counters stay attached to the
    /// CONSTRUCTING house until the building is limboed — while the removals
    /// read the LIVE owner (`0x00455980` / `0x004556D0`), which is what the
    /// limbo tail below and
    /// `selling_a_captured_psychic_sensor_debits_the_captor_and_leaves_the_builder_detecting`
    /// pin.
    #[test]
    fn capturing_a_psychic_sensor_leaves_both_circles_with_the_constructing_house() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        sim.spawn_object_at_height("NAPOWR", "Soviet", 30, 40, 0, 0, &rules)
            .unwrap();
        let id = sim
            .spawn_object_at_height("NAPSIS", "Soviet", 40, 40, 0, 0, &rules)
            .unwrap();
        let soviet = sim.substrate.entities.get(id).unwrap().owner();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .install_building_up(BuildingUp::completing_in_ticks(1, 0), 0);
        sim.advance_tick(&[], Some(&rules), None, None, 67);
        assert!(sim.fog.has_sensor_for_house(soviet, 54, 40));
        assert!(sim.fog.detects_disguise_for_house(soviet, 54, 40));

        let americans = sim.interner.intern("Americans");
        sim.change_owner(id, americans);

        assert!(
            sim.fog.detects_disguise_for_house(soviet, 54, 40),
            "ChangeOwner dispatches no +0x500, so the builder keeps detection"
        );
        assert!(
            !sim.fog.detects_disguise_for_house(americans, 54, 40),
            "ChangeOwner dispatches no +0x4FC, so the captor is granted none"
        );
        assert!(
            sim.fog.has_sensor_for_house(soviet, 54, 40),
            "the sensor array is on the same two slots and moves no more"
        );
        assert!(!sim.fog.has_sensor_for_house(americans, 54, 40));
        let deposit = sim
            .substrate
            .entities
            .get(id)
            .unwrap()
            .sensor_deposit
            .expect("the construction deposit survives the capture");
        assert_eq!(deposit.owner, soviet);
        assert_eq!(deposit.detect_disguise_radius, 15);

        // Selling under the NEW owner unwinds the NEW owner: both removals read
        // `this->Owner->ArrayIndex` at removal time, and `ChangeOwner` has
        // already overwritten that pointer (`0x007014A0 param_1[0x87] =
        // param_2`). The builder's credit is therefore never returned.
        sim.techno_limbo_with_rules(id, &rules, None);
        let index = 40 * usize::from(sim.fog.width()) + 54;
        assert_eq!(
            sim.fog.disguise_detect_by_house()[&soviet][index],
            1,
            "the builder's increment is permanent: no +0x500 ever names it again"
        );
        assert_eq!(
            sim.fog.disguise_detect_by_house()[&americans][index],
            -1,
            "0x00455980 debits the LIVE owner and 0x00487180 decrements unconditionally"
        );
    }

    /// Capture, then sell, end to end — the permanent counter skew gamemd
    /// leaves behind, on BOTH planes.
    ///
    /// Adds credit `*(int *)(param_1[0x87] + 0x30)` at add time
    /// (`BuildingClass::AddSensorArrayAt @ 0x00455820`,
    /// `AddDetectDisguiseAt @ 0x00455A80`); `BuildingClass::ChangeOwner @
    /// 0x00448260` dispatches neither an add nor a remove but does overwrite
    /// `+0x21C` through `TechnoClass::ChangeOwner @ 0x007014A0`; and Limbo's
    /// removes (`0x004556D0`, `0x00455980`) re-read `+0x21C` live. Net: the
    /// builder keeps `SensorsSight=15` and `DetectDisguiseRange=15`, the captor
    /// takes `-1` over the `CloakRadiusInCells=20` sensor circle and over the
    /// 15-cell disguise circle.
    #[test]
    fn selling_a_captured_psychic_sensor_debits_the_captor_and_leaves_the_builder_detecting() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        sim.spawn_object_at_height("NAPOWR", "Soviet", 30, 40, 0, 0, &rules)
            .unwrap();
        let id = sim
            .spawn_object_at_height("NAPSIS", "Soviet", 40, 40, 0, 0, &rules)
            .unwrap();
        let soviet = sim.substrate.entities.get(id).unwrap().owner();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .install_building_up(BuildingUp::completing_in_ticks(1, 0), 0);
        sim.advance_tick(&[], Some(&rules), None, None, 67);

        let americans = sim.interner.intern("Americans");
        sim.change_owner(id, americans);
        sim.techno_limbo_with_rules(id, &rules, None);

        let row = 40 * usize::from(sim.fog.width());
        let inside = row + 54; // inside both the 15-cell add and the 20-cell remove
        let fringe = row + 55; // outside the add, inside the sensor remove only

        assert_eq!(
            sim.fog.sensors_by_house()[&soviet][inside],
            1,
            "the builder's SensorsSight credit is never returned"
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
            sim.fog.has_sensor_for_house(soviet, 54, 40),
            "the builder keeps submarine detection over ground it no longer owns"
        );
        assert!(!sim.fog.has_sensor_for_house(americans, 54, 40));

        assert_eq!(sim.fog.disguise_detect_by_house()[&soviet][inside], 1);
        assert_eq!(
            sim.fog.disguise_detect_by_house()[&americans][inside],
            -1,
            "the disguise circle is symmetric in radius but not in house"
        );
        assert!(sim.fog.detects_disguise_for_house(soviet, 54, 40));
        assert!(
            !sim.fog.detects_disguise_for_house(americans, 54, 40),
            "a later NAPSIS the captor builds here needs two increments to see again"
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
