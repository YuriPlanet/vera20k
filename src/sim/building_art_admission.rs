//! Building slot producers formerly approximated by presentation predicates.
use super::*;

impl Simulation {
    /// Building452480 / ChangeOwner4484C6: this differs from Type.Powered
    /// constructor policy. Toggle existing slot Powered flags only.
    fn set_building_stuff_enabled(&mut self, id: u64, enabled: bool, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        entity.building_stuff_enabled = enabled;
        let slots = entity.building_anim_slots;
        for (slot, anim_id) in slots.into_iter().enumerate() {
            if self
                .building_anim_power_flags(id, slot as u8, rules)
                .powered
            {
                if let Some(anim) = anim_id.and_then(|id| self.substrate.anims.get_mut(id)) {
                    anim.runtime.paused = !enabled;
                }
            }
        }
    }

    /// Authored ReadINI44FD35 runs after successful Unlimbo and slot creation.
    pub(crate) fn finish_authored_building_enable(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category == crate::map::entities::EntityCategory::Structure
            && !entity.building_has_engineer
            && rules
                .object(self.interner.resolve(entity.type_ref()))
                .is_some_and(|o| o.needs_engineer)
        {
            self.set_building_stuff_enabled(id, false, rules);
        }
    }

    /// Every changed-owner Building path writes HasEngineer (4484AF), including
    /// non-engineer transfers. Same-owner calls return before this entry.
    pub(crate) fn enable_building_after_owner_change(&mut self, id: u64, rules: Option<&RuleSet>) {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        if entity.category != crate::map::entities::EntityCategory::Structure {
            return;
        }
        entity.building_has_engineer = true;
        if let Some(rules) = rules {
            if rules
                .object(self.interner.resolve(entity.type_ref()))
                .is_some_and(|o| o.needs_engineer)
            {
                self.set_building_stuff_enabled(id, true, rules);
            }
        }
    }

    /// Original450B34..450CB7: absorbed cargo selects Active/ActiveTwo before
    /// the Silo update. No desired-slot recreation or extra timer ownership.
    pub(crate) fn update_building_absorb_anim(&mut self, id: u64, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = rules.object(self.interner.resolve(entity.type_ref())) else {
            return;
        };
        if entity.category != crate::map::entities::EntityCategory::Structure
            || !object.infantry_absorb
            || object.extra_power <= 0
            || !entity.building_actually_placed
            || entity.building_up.is_some()
        {
            return;
        }
        // The represented construction owner is the body-state==0 admission
        // gate. General body/turret state migration is not implemented here.
        let occupied = entity
            .passenger_role
            .cargo()
            .is_some_and(|c| !c.passengers.is_empty());
        let damaged = requested_damage_state(
            entity.health,
            object.strength,
            rules.general.condition_yellow,
        );
        let (remove, keep) = if occupied { (3, 4) } else { (4, 3) };
        self.clear_building_anim_slot(id, remove);
        if self.substrate.entities.get(id).unwrap().building_anim_slots[usize::from(keep)].is_none()
        {
            // Absorbed passengers (+114) are not the distinct +694 garrison
            // vector. Stock YAPOWR has only the former. Custom types combining
            // CanBeOccupied and InfantryAbsorb remain outside this policy's
            // native parity claim: the pre-existing shared cargo representation
            // cannot independently express both native containers.
            self.set_building_anim_slot(id, keep, damaged, false, 0, rules);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::entities::{EntityCategory, MapEntity};
    use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
    use std::collections::BTreeMap;

    fn fixture(extra: &str) -> (Simulation, RuleSet, u64) {
        let art = IniFile::from_str(
            "[B]\nActiveAnim=N\nActiveAnimDamaged=D\nActiveAnimTwo=M\nActiveAnimTwoDamaged=MD\nActiveAnimPowered=yes\nActiveAnimTwoPowered=yes\n[N]\nLoopCount=-1\nRate=900\n[D]\nLoopCount=-1\nRate=900\n[M]\nLoopCount=-1\nRate=900\n[MD]\nLoopCount=-1\nRate=900\n",
        );
        let ini = IniFile::from_str(&format!(
            "[BuildingTypes]\n0=B\n[InfantryTypes]\n0=E1\n[B]\nStrength=100\n{extra}\n[E1]\nStrength=100\nSpeed=4\n[Animations]\n0=N\n1=D\n2=M\n3=MD\n"
        ));
        // Native ART bodies can be read after their type was registered by the
        // prior pass. Exercise looping animations, not the first-pass defaults.
        let mut layers = crate::rules::native_processing::RulesLayerStack::new(ini);
        layers.push(
            crate::rules::native_processing::RulesLayerKind::Scenario,
            IniFile::from_str(""),
        );
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        let mut registry = ArtRegistry::from_ini(&art);
        for name in ["N", "D", "M", "MD"] {
            registry.bind_anim_frame_count_for_test(name, 40);
        }
        rules.merge_art_data(&registry);
        let mut sim = Simulation::new();
        assert_eq!(
            sim.spawn_from_map(
                &[MapEntity {
                    owner: "Neutral".into(),
                    type_id: "B".into(),
                    health: 256,
                    cell_x: 2,
                    cell_y: 2,
                    facing: 0,
                    category: EntityCategory::Structure,
                    sub_cell: 0,
                    veterancy: 0,
                    high: false,
                    mission: None,
                    recruitable_a: true,
                    recruitable_b: true,
                    structure_upgrades: [None, None, None],
                    structure_ai_sellable: false,
                }],
                Some(&rules),
                &BTreeMap::new()
            ),
            1
        );
        let id = sim.entities().keys_sorted()[0];
        (sim, rules, id)
    }

    fn tick(sim: &mut Simulation, rules: &RuleSet) {
        sim.advance_tick(&[], Some(rules), &BTreeMap::new(), None, None, 33);
    }

    #[test]
    fn authored_tech_slots_pause_then_changed_owner_resumes_and_restore_retains_latch() {
        let (mut sim, rules, id) = fixture("NeedsEngineer=yes\nCapturable=yes\nPowered=no");
        let anim = sim.entities().get(id).unwrap().building_anim_slots[3].unwrap();
        // Rate=900 produces one frame per native timer tick; Rate=0 freezes.
        assert_eq!(sim.anim(anim).unwrap().runtime.rate_reload, 1);
        assert!(sim.anim(anim).unwrap().effective_end > 3);
        let frame = sim.anim(anim).unwrap().runtime.current_frame;
        assert!(!sim.entities().get(id).unwrap().building_stuff_enabled);
        assert_eq!(sim.building_operational_state(id, &rules), Some(false));
        for _ in 0..6 {
            tick(&mut sim, &rules);
        }
        assert!(sim.anim(anim).unwrap().runtime.paused);
        assert_eq!(sim.anim(anim).unwrap().runtime.current_frame, frame);
        let old_owner = sim.entities().get(id).unwrap().owner();
        sim.change_owner_with_rules(id, old_owner, &rules);
        assert!(!sim.entities().get(id).unwrap().building_has_engineer);
        let owner = sim.interner.intern("Americans");
        sim.change_owner_with_rules(id, owner, &rules);
        assert!(sim.entities().get(id).unwrap().building_has_engineer);
        assert!(sim.entities().get(id).unwrap().building_stuff_enabled);
        assert!(!sim.anim(anim).unwrap().runtime.paused);
        for _ in 0..3 {
            tick(&mut sim, &rules);
        }
        assert_ne!(sim.anim(anim).unwrap().runtime.current_frame, frame);
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.restore_after_snapshot_load().unwrap();
        assert!(restored.entities().get(id).unwrap().building_has_engineer);
        assert_eq!(restored.state_hash(), sim.state_hash());
    }

    #[test]
    fn offline_damage_replacement_uses_constructor_power_gate() {
        let (mut sim, rules, id) = fixture("NeedsEngineer=yes\nCapturable=yes\nPowered=no");
        let old = sim.entities().get(id).unwrap().building_anim_slots[3].unwrap();
        assert!(sim.anim(old).unwrap().runtime.paused);
        sim.entities_mut().get_mut(id).unwrap().health.current = 25;
        sim.refresh_building_damage_state(id, &rules);
        let replaced = sim.entities().get(id).unwrap().building_anim_slots[3].unwrap();
        assert!(
            !sim.anim(replaced).unwrap().runtime.paused,
            "replacement does not copy pause; TypePowered is false"
        );
        assert!(!sim.entities().get(id).unwrap().building_stuff_enabled);
    }

    #[test]
    fn absorb_slots_follow_cargo_through_logic_damage_and_restore() {
        let (mut sim, rules, id) = fixture("InfantryAbsorb=yes\nExtraPower=100\nPassengers=5");
        tick(&mut sim, &rules);
        let empty = sim.entities().get(id).unwrap().building_anim_slots[3].unwrap();
        assert!(sim.entities().get(id).unwrap().building_anim_slots[4].is_none());
        tick(&mut sim, &rules);
        assert_eq!(
            sim.entities().get(id).unwrap().building_anim_slots[3],
            Some(empty)
        );
        let passenger = sim
            .spawn_object("E1", "Neutral", 6, 6, 0, &rules, &BTreeMap::new())
            .unwrap();
        sim.conceal(passenger);
        sim.entities_mut()
            .get_mut(passenger)
            .unwrap()
            .passenger_role = crate::sim::passenger::PassengerRole::Inside { transport_id: id };
        assert!(
            sim.entities_mut()
                .get_mut(id)
                .unwrap()
                .passenger_role
                .cargo_mut()
                .unwrap()
                .board(passenger, 1)
        );
        tick(&mut sim, &rules);
        let occupied = sim.entities().get(id).unwrap().building_anim_slots[4].unwrap();
        assert!(sim.anim(empty).is_none());
        assert_eq!(
            sim.interner.resolve(sim.anim(occupied).unwrap().type_id),
            "M"
        );
        sim.entities_mut().get_mut(id).unwrap().health.current = 25;
        sim.refresh_building_damage_state(id, &rules);
        let damaged = sim.entities().get(id).unwrap().building_anim_slots[4].unwrap();
        assert_eq!(
            sim.interner.resolve(sim.anim(damaged).unwrap().type_id),
            "MD"
        );
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .unwrap()
            .sim;
        restored.retain_in_scenario_process_state_from(&sim);
        restored.restore_after_snapshot_load().unwrap();
        assert_eq!(
            restored.entities().get(id).unwrap().building_anim_slots[4],
            Some(damaged)
        );
        // Exercise the empty transition after restoring its actual slot identity.
        let cargo = restored
            .entities_mut()
            .get_mut(id)
            .unwrap()
            .passenger_role
            .cargo_mut()
            .unwrap();
        cargo.passengers.clear();
        cargo.passenger_sizes.clear();
        cargo.total_size = 0;
        tick(&mut restored, &rules);
        let empty = restored.entities().get(id).unwrap().building_anim_slots[3].unwrap();
        assert!(restored.entities().get(id).unwrap().building_anim_slots[4].is_none());
        assert_eq!(
            restored
                .interner
                .resolve(restored.anim(empty).unwrap().type_id),
            "D"
        );
    }
}
