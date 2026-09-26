//! Active Techno sensor-count deposit lifecycle.
//!
//! FootClass add/remove/move/owner writers are at 0x004D7318, 0x004DB376,
//! 0x004D8611/0x004D8621, and 0x004DBEFB/0x004DBF81. Building sensor arrays
//! use 0x00455820/0x004556D0 and deliberately add with `SensorsSight=` but
//! remove with the signed-byte `CloakRadiusInCells=` value.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::{ObjectCategory, ObjectType};
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
    pub owner: InternedId,
    pub center: (u16, u16),
    pub add_radius: u16,
    pub remove_radius: i16,
    pub building_array: bool,
    /// `DetectDisguiseRange=` (`TechnoTypeClass+0x5F4`) circle stamped by
    /// `BuildingClass::AddDetectDisguiseAt @ 0x00455A80` alongside the sensor
    /// array. Unlike the sensor array, its paired
    /// `RemoveDetectDisguiseAt @ 0x00455980` reads the SAME field back
    /// (`0x00455991` loads `+0x5F4`), so add and remove are symmetric and one
    /// radius suffices.
    #[serde(default)]
    pub detect_disguise_radius: u16,
}

impl SensorDeposit {
    fn unit(owner: InternedId, center: (u16, u16), radius: u16) -> Self {
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
                                    entity.category,
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
        rules: Option<&RuleSet>,
    ) -> Vec<u64> {
        let Some(rules) = rules else {
            return Vec::new();
        };
        // Add/RemoveSensorsAt @ 0x004DE7B0/0x004DE940 and the BuildingClass
        // variants walk CellClass::FirstObject head-to-tail after mutating this
        // exact cell. OccupancyGrid's ground list is that authoritative order.
        let residents = self.sensor_residents_in_native_order(cell);
        for &stable_id in &residents {
            crate::sim::world::techno_ai_cloak::sensor_reevaluate_stock_cloak(
                self, stable_id, rules,
            );
        }
        residents
    }

    fn apply_sensor_add(
        &mut self,
        owner: InternedId,
        center: (u16, u16),
        radius: u16,
        rules: Option<&RuleSet>,
    ) -> Vec<u64> {
        let mut callbacks = Vec::new();
        for cell in self.fog.sensor_circle_cells(center, radius) {
            self.fog.increment_sensor_at(owner, cell.0, cell.1);
            callbacks.extend(self.sensor_reevaluate_residents(cell, rules));
        }
        callbacks
    }

    fn apply_unit_sensor_remove(
        &mut self,
        owner: InternedId,
        center: (u16, u16),
        radius: u16,
        rules: Option<&RuleSet>,
    ) -> Vec<u64> {
        let mut callbacks = Vec::new();
        for cell in self.fog.sensor_circle_cells(center, radius) {
            // RemoveSensorsAt @ 0x004DE940 skips both mutation and callbacks
            // when the signed pre-count is not positive.
            if self
                .fog
                .decrement_sensor_at_if_positive(owner, cell.0, cell.1)
            {
                callbacks.extend(self.sensor_reevaluate_residents(cell, rules));
            }
        }
        callbacks
    }

    fn apply_building_sensor_remove(
        &mut self,
        owner: InternedId,
        center: (u16, u16),
        radius: u16,
        rules: Option<&RuleSet>,
    ) -> Vec<u64> {
        let mut callbacks = Vec::new();
        for cell in self.fog.sensor_circle_cells(center, radius) {
            self.fog
                .decrement_sensor_at_unconditional(owner, cell.0, cell.1);
            callbacks.extend(self.sensor_reevaluate_residents(cell, rules));
        }
        callbacks
    }

    /// Every native remove takes its house index from `this->Owner->ArrayIndex`
    /// at REMOVAL time, never from anything recorded when the circle was
    /// stamped. `BuildingClass::RemoveDetectDisguiseAt @ 0x00455980` opens with
    /// `0045598A MOV EDX,[ECX+0x21C]` / `004559A1 MOV EAX,[EDX+0x30]`;
    /// `BuildingClass::RemoveSensorArrayAt @ 0x004556D0` and
    /// `TechnoClass::RemoveSensorsAt @ 0x004DE940` open with the identical
    /// `*(int *)(param_1[0x87] + 0x30)` read, and so do both adds
    /// (`0x00455820`, `0x00455A80`). `TechnoClass::ChangeOwner @ 0x007014A0`
    /// overwrites that pointer (`param_1[0x87] = param_2`), so the live owner
    /// is whoever holds the object now.
    ///
    /// For units the two agree: `FootClass::ChangeOwner @ 0x004DBED0` dispatches
    /// the remove (`vt+0x4EC`) BEFORE the pointer flip and the add
    /// (`vt+0x4E8`) after, and VERA calls
    /// `transfer_sensor_before_owner_change` from the same slot. For BUILDINGS
    /// they diverge permanently, and that asymmetry is native map state:
    /// construction credits the builder, `ChangeOwner` dispatches neither add
    /// nor remove, and Limbo debits the CAPTOR. See the residual on
    /// `transfer_sensor_before_owner_change_inner`.
    fn remove_cached_sensor_deposit(
        &mut self,
        stable_id: u64,
        rules: Option<&RuleSet>,
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
                );
            }
        } else {
            self.apply_unit_sensor_remove(
                remove_owner,
                deposit.center,
                deposit.remove_radius.max(0) as u16,
                rules,
            );
        }
        Some(deposit)
    }

    /// `FootClass::Unlimbo` sensor writer. The deposit is absent for a
    /// rules-less/headless fixture and for non-Foot objects.
    pub(crate) fn add_unit_sensor_after_unlimbo(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some((owner, center, radius, in_limbo)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                let object = rules.object(self.interner.resolve(entity.type_ref()))?;
                Some((
                    entity.owner(),
                    (entity.position.rx, entity.position.ry),
                    unit_sensor_radius(entity.category, object)?,
                    entity.lifecycle.in_limbo,
                ))
            })
        else {
            return;
        };
        if in_limbo {
            return;
        }
        let _ = self.remove_cached_sensor_deposit(stable_id, Some(rules));
        self.apply_sensor_add(owner, center, radius, Some(rules));
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.sensor_deposit = Some(SensorDeposit::unit(owner, center, radius));
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
    pub(crate) fn add_building_sensor_array_if_powered(&mut self, stable_id: u64, rules: &RuleSet) {
        let Some((owner, center, sight, remove_radius, detect_radius, powered, in_limbo)) =
            self.substrate.entities.get(stable_id).and_then(|entity| {
                let object = rules.object(self.interner.resolve(entity.type_ref()))?;
                if entity.category != EntityCategory::Structure
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
                    (entity.position.rx, entity.position.ry),
                    sight,
                    object.cloak_radius_in_cells,
                    detect_radius,
                    crate::sim::power_system::is_building_powered(
                        &self.power_states,
                        rules,
                        entity,
                        &self.interner,
                    ),
                    entity.lifecycle.in_limbo,
                ))
            })
        else {
            return;
        };
        if !powered || in_limbo {
            return;
        }
        let _ = self.remove_cached_sensor_deposit(stable_id, Some(rules));
        if sight > 0 {
            self.apply_sensor_add(owner, center, sight, Some(rules));
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

    /// Exact cached remove used by Foot/Building Limbo.
    pub(crate) fn remove_sensor_before_limbo(&mut self, stable_id: u64) {
        let _ = self.remove_cached_sensor_deposit(stable_id, None);
    }

    pub(crate) fn remove_sensor_before_limbo_with_rules(
        &mut self,
        stable_id: u64,
        rules: &RuleSet,
    ) {
        let _ = self.remove_cached_sensor_deposit(stable_id, Some(rules));
    }

    /// FootClass::PerCellProcess old-remove/new-add pair. TubeMovement owns an
    /// early-return turn and is intentionally not routed here until its native
    /// completion writer lands.
    pub(crate) fn move_unit_sensor_after_cell_change(
        &mut self,
        stable_id: u64,
        old_cell: Option<(u16, u16)>,
        new_cell: Option<(u16, u16)>,
        rules: &RuleSet,
    ) {
        if old_cell == new_cell {
            return;
        }
        self.refresh_unit_sensor_at_per_cell(stable_id, rules);
    }

    /// Foot4D8611/4D8621 executes both receivers for PerCellProcess(2),
    /// including a terminal or chain callback in the same cell.
    pub(crate) fn refresh_unit_sensor_at_per_cell(&mut self, stable_id: u64, rules: &RuleSet) {
        let had_unit_deposit = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| entity.sensor_deposit)
            .is_some_and(|deposit| !deposit.building_array);
        if had_unit_deposit {
            let _ = self.remove_cached_sensor_deposit(stable_id, Some(rules));
        }
        self.add_unit_sensor_after_unlimbo(stable_id, rules);
    }

    /// `FootClass::ChangeOwner @ 0x004DBED0` old-remove/new-add pair — both
    /// arms gated on `TechnoTypeClass+0x5F0` (`SensorsSight=`), removing for
    /// the old house through vtable `+0x4EC` at `0x004DBEFB` and adding for the
    /// new one through `+0x4E8` at `0x004DBF81`.
    ///
    /// A BUILDING deposit is deliberately left untouched. Neither
    /// `BuildingClass::ChangeOwner @ 0x00448260` nor the
    /// `TechnoClass::ChangeOwner @ 0x007014A0` it calls at `0x00448BE8`
    /// dispatches any of the four BuildingClass sensor slots, and the
    /// program-wide `search_instructions` for `call [..+0x4f4/0x4f8/0x4fc/
    /// 0x500]` finds no BuildingClass dispatcher outside
    /// `OnConstructionComplete` and `Limbo`. So gamemd leaves BOTH the sensor
    /// array and the disguise-detect circle stamped for the CONSTRUCTING house
    /// until the building is limboed: capturing a Psychic Sensor transfers
    /// neither.
    ///
    /// DRIFT-BY-DESIGN — this reproduces a permanent native counter skew, it
    /// does not avoid one. Limbo's removes read the LIVE owner
    /// (`RemoveDetectDisguiseAt @ 0x00455980` opens `0045598A MOV
    /// EDX,[ECX+0x21C]` / `004559A1 MOV EAX,[EDX+0x30]`; `RemoveSensorArrayAt
    /// @ 0x004556D0` repeats the read) and
    /// `CellClass::DecrementDisguiseDetectCount @ 0x00487180` decrements
    /// unconditionally. So selling or losing a captured Psychic Sensor debits
    /// the CAPTOR for the circles the BUILDER was credited with.
    /// *Trigger:* engineer capture of a building carrying `SensorArray=` or
    /// `DetectDisguise=` (stock: NAPSIS only), then its limbo. *Player effect:*
    /// the constructing house keeps a phantom detection circle over that ground
    /// for the rest of the match, and the capturing house's counters go negative
    /// there, silently cancelling the detection of the next such building it
    /// puts on the same cells. *Frequency:* uncommon per match, but the state is
    /// permanent once it happens. *Downstream risk:* none new — the asymmetric
    /// `CloakRadiusInCells=` remove radius already produces negative sensor
    /// fringe counts without a capture, so no reader gains an unseen case.
    fn transfer_sensor_before_owner_change_inner(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
        rules: Option<&RuleSet>,
    ) {
        let building_deposit = self
            .substrate
            .entities
            .get(stable_id)
            .and_then(|entity| entity.sensor_deposit)
            .is_some_and(|deposit| deposit.building_array);
        if building_deposit {
            return;
        }
        let Some(mut deposit) = self.remove_cached_sensor_deposit(stable_id, rules) else {
            return;
        };
        deposit.owner = new_owner;
        self.apply_sensor_add(new_owner, deposit.center, deposit.add_radius, rules);
        if let Some(entity) = self.substrate.entities.get_mut(stable_id) {
            entity.sensor_deposit = Some(deposit);
        }
    }

    pub(crate) fn transfer_sensor_before_owner_change(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
    ) {
        self.transfer_sensor_before_owner_change_inner(stable_id, new_owner, None);
    }

    pub(crate) fn transfer_sensor_before_owner_change_with_rules(
        &mut self,
        stable_id: u64,
        new_owner: InternedId,
        rules: &RuleSet,
    ) {
        self.transfer_sensor_before_owner_change_inner(stable_id, new_owner, Some(rules));
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
        sim.fog.width = 64;
        sim.fog.height = 64;
        sim.playfield_bounds = Some(PlayfieldBounds::from_normalized_local_size(
            64, 2, 2, 56, 52,
        ));
        sim
    }

    #[test]
    fn unit_sensor_unlimbo_move_owner_limbo_and_overlap_use_cached_deposits() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        let first = sim
            .spawn_object_at_height("DEST", "Americans", 40, 30, 0, 0, &rules)
            .unwrap();
        let second = sim
            .spawn_object_at_height("DEST", "Americans", 40, 30, 0, 0, &rules)
            .unwrap();
        let americans = sim.substrate.entities.get(first).unwrap().owner;
        let soviet = sim.interner.intern("Soviet");
        assert!(sim.fog.has_sensor_for_house(americans, 33, 30));
        assert!(!sim.fog.has_sensor_for_house(americans, 48, 30));

        sim.techno_limbo_with_rules(first, &rules);
        assert!(
            sim.fog.has_sensor_for_house(americans, 33, 30),
            "the overlapping second deposit remains positive"
        );

        let old = Some((40, 30));
        sim.substrate.entities.get_mut(second).unwrap().position.rx = 50;
        sim.move_unit_sensor_after_cell_change(second, old, Some((50, 30)), &rules);
        assert!(!sim.fog.has_sensor_for_house(americans, 33, 30));
        assert!(sim.fog.has_sensor_for_house(americans, 57, 30));

        sim.change_owner(second, soviet);
        assert!(!sim.fog.has_sensor_for_house(americans, 57, 30));
        assert!(sim.fog.has_sensor_for_house(soviet, 57, 30));

        // Drift every live fact after the deposit. The cached center still
        // decides WHICH cells the removal walks, but the HOUSE is re-read live:
        // `TechnoClass::RemoveSensorsAt @ 0x004DE940` opens with
        // `*(int *)(param_1[0x87] + 0x30)`, the same read as the BuildingClass
        // pair. gamemd cannot reach this state for a Foot — only
        // `FootClass::ChangeOwner @ 0x004DBED0` moves `+0x21C`, and it dispatches
        // `vt+0x4EC` first — so this pins the read, not a reachable scenario.
        let neutral = sim.interner.intern("Neutral");
        {
            let entity = sim.substrate.entities.get_mut(second).unwrap();
            entity.owner = neutral;
            entity.position.rx = 5;
            entity.position.ry = 5;
        }
        sim.techno_limbo_with_rules(second, &rules);
        assert!(
            sim.fog.has_sensor_for_house(soviet, 57, 30),
            "the removal debits the live owner, not the deposit's recorded house"
        );
        let index = 30 * usize::from(sim.fog.width) + 57;
        assert_eq!(
            sim.fog.sensors_by_house[&neutral][index], 0,
            "and the unit remove's positive pre-count gate leaves no negative residue"
        );
    }

    #[test]
    fn napsis_construction_adds_radius_fifteen_but_limbo_removes_default_twenty() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        sim.spawn_object_at_height("NAPOWR", "Soviet", 30, 40, 0, 0, &rules)
            .unwrap();
        let id = sim
            .spawn_object_at_height("NAPSIS", "Soviet", 40, 40, 0, 0, &rules)
            .unwrap();
        let owner = sim.substrate.entities.get(id).unwrap().owner;
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .sensor_deposit
                .is_none()
        );
        sim.substrate.entities.get_mut(id).unwrap().building_up =
            Some(BuildingUp::completing_in_ticks(1, 0));
        sim.advance_tick(
            &[],
            Some(&rules),
            &std::collections::BTreeMap::new(),
            None,
            None,
            67,
        );
        assert!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .building_up
                .is_none()
        );
        assert!(sim.fog.has_sensor_for_house(owner, 54, 40));
        assert!(!sim.fog.has_sensor_for_house(owner, 55, 40));
        sim.techno_limbo_with_rules(id, &rules);
        let index = 40 * usize::from(sim.fog.width) + 55;
        assert_eq!(sim.fog.sensors_by_house[&owner][index], -1);
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
        let soviet = sim.substrate.entities.get(id).unwrap().owner;
        sim.substrate.entities.get_mut(id).unwrap().building_up =
            Some(BuildingUp::completing_in_ticks(1, 0));
        sim.advance_tick(
            &[],
            Some(&rules),
            &std::collections::BTreeMap::new(),
            None,
            None,
            67,
        );
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
        sim.techno_limbo_with_rules(id, &rules);
        let index = 40 * usize::from(sim.fog.width) + 54;
        assert_eq!(
            sim.fog.disguise_detect_by_house[&soviet][index], 1,
            "the builder's increment is permanent: no +0x500 ever names it again"
        );
        assert_eq!(
            sim.fog.disguise_detect_by_house[&americans][index], -1,
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
        let soviet = sim.substrate.entities.get(id).unwrap().owner;
        sim.substrate.entities.get_mut(id).unwrap().building_up =
            Some(BuildingUp::completing_in_ticks(1, 0));
        sim.advance_tick(
            &[],
            Some(&rules),
            &std::collections::BTreeMap::new(),
            None,
            None,
            67,
        );

        let americans = sim.interner.intern("Americans");
        sim.change_owner(id, americans);
        sim.techno_limbo_with_rules(id, &rules);

        let row = 40 * usize::from(sim.fog.width);
        let inside = row + 54; // inside both the 15-cell add and the 20-cell remove
        let fringe = row + 55; // outside the add, inside the sensor remove only

        assert_eq!(
            sim.fog.sensors_by_house[&soviet][inside], 1,
            "the builder's SensorsSight credit is never returned"
        );
        assert_eq!(
            sim.fog.sensors_by_house[&americans][inside], -1,
            "0x004556D0 debits the live owner over CloakRadiusInCells"
        );
        assert_eq!(sim.fog.sensors_by_house[&soviet][fringe], 0);
        assert_eq!(
            sim.fog.sensors_by_house[&americans][fringe], -1,
            "the asymmetric remove radius reaches past the add circle too"
        );
        assert!(
            sim.fog.has_sensor_for_house(soviet, 54, 40),
            "the builder keeps submarine detection over ground it no longer owns"
        );
        assert!(!sim.fog.has_sensor_for_house(americans, 54, 40));

        assert_eq!(sim.fog.disguise_detect_by_house[&soviet][inside], 1);
        assert_eq!(
            sim.fog.disguise_detect_by_house[&americans][inside], -1,
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
        let soviet = sim.substrate.entities.get(older).unwrap().owner;
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
            cloak.state = 0;
            cloak.visual_phase = None;
        }

        let added = sim.apply_sensor_add(detector, (40, 30), 1, Some(&rules));
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
                .state,
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
            cloak.state = 0;
            cloak.visual_phase = None;
        }
        let removed = sim.apply_unit_sensor_remove(detector, (40, 30), 1, Some(&rules));
        assert_eq!(removed, vec![newer, older]);
        assert!(sim.sound_events.is_empty());
        assert!(
            sim.apply_unit_sensor_remove(detector, (40, 30), 1, Some(&rules))
                .is_empty(),
            "unit RemoveSensorsAt skips decrement and callbacks at nonpositive pre-count"
        );
        assert!(sim.sound_events.is_empty());

        let building_removed =
            sim.apply_building_sensor_remove(detector, (40, 30), 1, Some(&rules));
        assert_eq!(building_removed, vec![newer, older]);
        let index = 30 * usize::from(sim.fog.width) + 40;
        assert_eq!(
            sim.fog.sensors_by_house[&detector][index], -1,
            "BuildingClass removal is unconditional and signed"
        );
    }

    #[test]
    fn map_and_production_unlimbo_reject_outside_playfield_units() {
        let rules = rules();
        let mut sim = sim_with_map_authority();
        let bounds = sim.playfield_bounds.unwrap();
        let inside = (0u16..64)
            .flat_map(|ry| (0u16..64).map(move |rx| (rx, ry)))
            .find(|&(rx, ry)| bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
            .expect("mode-one inside cell");
        let outside = (0u16..64)
            .flat_map(|ry| (0u16..64).map(move |rx| (rx, ry)))
            .find(|&(rx, ry)| !bounds.contains_height_aware_packed(rx.into(), ry.into(), 0, 0))
            .expect("mode-one outside cell");
        let height = std::collections::BTreeMap::new();
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
                    veterancy: 0,
                    high: false,
                    mission: None,
                    recruitable_a: true,
                    recruitable_b: true,
                    structure_upgrades: [None, None, None],
                    structure_ai_sellable: false,
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
                    veterancy: 0,
                    high: false,
                    mission: None,
                    recruitable_a: true,
                    recruitable_b: true,
                    structure_upgrades: [None, None, None],
                    structure_ai_sellable: false,
                },
            ],
            Some(&rules),
            &height,
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .cloak
                .as_ref()
                .unwrap()
                .state,
            0
        );
        assert!(
            sim.substrate.entities.get(2).is_none(),
            "authored outside-playfield Unit must fail Unlimbo and be discarded"
        );

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
                .state,
            0
        );
        assert!(
            outside.is_none(),
            "runtime outside-playfield Unit must fail Unlimbo"
        );
    }
}
