//! Original4545D0/4547C0/4549B0: retained Building slots and common Anim pause.
//! Native comparisons: tools/spatial_oracle/building_slot_power*.json.
use crate::rules::{art_data::BuildingAnimPowerFlags, ruleset::RuleSet};
use crate::sim::{building_art::requested_damage_state, world::Simulation};

impl Simulation {
    pub(crate) fn building_anim_power_flags(
        &self,
        id: u64,
        slot: u8,
        rules: &RuleSet,
    ) -> BuildingAnimPowerFlags {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Default::default();
        };
        let name = self.interner.resolve(entity.type_ref());
        rules
            .object(name)
            .and_then(|object| {
                rules
                    .art_registry
                    .resolve_metadata_entry(name, &object.image)
            })
            .map_or_else(Default::default, |entry| {
                entry.building_anim_power[usize::from(slot)]
            })
    }

    fn building_anim_delayed_fire(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let name = self.interner.resolve(entity.type_ref());
        rules
            .object(name)
            .and_then(|object| {
                rules
                    .art_registry
                    .resolve_metadata_entry(name, &object.image)
            })
            .is_some_and(|entry| entry.is_anim_delayed_fire)
    }

    fn create_power_slot(&mut self, id: u64, slot: u8, rules: &RuleSet) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = rules.object(self.interner.resolve(entity.type_ref())) else {
            return;
        };
        let damaged = requested_damage_state(
            entity.health,
            object.strength,
            rules.general.condition_yellow,
        );
        self.set_building_anim_slot(id, slot, damaged, false, 0, rules);
    }

    /// Restore4545D0 clears SuperLowPower20 before ascending visits. Loss4547C0
    /// uses Powered/Light/Effect precedence and marks replay before scalar delete.
    pub(crate) fn apply_building_anim_power(&mut self, id: u64, restoring: bool, rules: &RuleSet) {
        if restoring {
            self.clear_building_anim_slot(id, 20);
        }
        for slot in 0..21u8 {
            let flags = self.building_anim_power_flags(id, slot, rules);
            let Some(entity) = self.substrate.entities.get(id) else {
                return;
            };
            let anim = entity.building_anim_slots[usize::from(slot)];
            let placed = entity.building_actually_placed;
            let replay = entity.building_anim_effect_replay[usize::from(slot)];
            if flags.powered {
                if let Some(anim) = anim.and_then(|anim| self.substrate.anims.get_mut(anim)) {
                    anim.runtime.paused = !restoring;
                }
            } else if flags.powered_light {
                if restoring {
                    if anim.is_none()
                        && placed
                        && !(slot == 10 && self.building_anim_delayed_fire(id, rules))
                    {
                        self.create_power_slot(id, slot, rules);
                    }
                } else if anim.is_some() {
                    self.clear_building_anim_slot(id, slot);
                    if slot == 10
                        && self.building_anim_delayed_fire(id, rules)
                        && self.building_anim_power_flags(id, 3, rules).powered
                    {
                        self.create_power_slot(id, 3, rules);
                    }
                }
            } else if flags.powered_effect {
                if restoring && replay {
                    self.substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .building_anim_effect_replay[usize::from(slot)] = false;
                    self.create_power_slot(id, slot, rules);
                } else if !restoring && anim.is_some() {
                    self.substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .building_anim_effect_replay[usize::from(slot)] = true;
                    self.clear_building_anim_slot(id, slot);
                    if slot == 16 {
                        self.create_power_slot(id, 20, rules);
                    }
                }
            }
        }
    }

    ///4549B0 slot tail, after the gap publication. Ordinary low power and the
    ///PoweredSpecial House outage are distinct conditions in the original.
    pub(crate) fn update_building_anim_power(
        &mut self,
        id: u64,
        operational: bool,
        rules: &RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = rules.object(self.interner.resolve(entity.type_ref())) else {
            return;
        };
        if object.powered && crate::sim::power_system::native_building_power_drain(object.power) > 0
        {
            self.apply_building_anim_power(id, operational, rules);
        }
        if !object.powered_special {
            return;
        }
        if operational {
            self.clear_building_anim_slot(id, 19);
            for slot in 0..21u8 {
                if self
                    .building_anim_power_flags(id, slot, rules)
                    .powered_special
                {
                    self.create_power_slot(id, slot, rules);
                }
            }
        } else if self.building_power_outage(id) {
            self.create_power_slot(id, 19, rules);
            for slot in 0..21u8 {
                if self
                    .building_anim_power_flags(id, slot, rules)
                    .powered_special
                {
                    self.clear_building_anim_slot(id, slot);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::{art_data::ArtRegistry, ini_parser::IniFile};
    use crate::sim::{game_entity::GameEntity, timer::CdTimer};

    fn rules(strength: i32, extra: &str) -> RuleSet {
        let mut text = String::from("[B]\n");
        let keys = [
            "PowerUp1Anim",
            "PowerUp2Anim",
            "PowerUp3Anim",
            "ActiveAnim",
            "ActiveAnimTwo",
            "ActiveAnimThree",
            "ActiveAnimFour",
            "PreProductionAnim",
            "ProductionAnim",
            "TurretAnim",
            "SpecialAnim",
            "SpecialAnimTwo",
            "SpecialAnimThree",
            "SpecialAnimFour",
            "SuperAnim",
            "SuperAnimTwo",
            "SuperAnimThree",
            "SuperAnimFour",
            "IdleAnim",
            "LowPower",
            "SuperLowPower",
        ];
        for (slot, key) in keys.iter().enumerate() {
            let damaged = if slot < 3 {
                format!("PowerUp{}DamagedAnim", slot + 1)
            } else {
                format!("{key}Damaged")
            };
            text.push_str(&format!("{key}=N\n{damaged}=D\n"));
        }
        text.push_str("[N]\nLoopCount=-1\nRate=300\n[D]\nLoopCount=-1\nRate=300\n");
        let ini = IniFile::from_str(&text);
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(
            &IniFile::from_str(&format!(
                "[BuildingTypes]\n0=B\n[B]\nStrength={strength}\n{extra}\n[Animations]\n0=N\n1=D\n"
            )),
            &ini,
        )
        .unwrap();
        let mut art = ArtRegistry::from_ini(&ini);
        for name in ["N", "D"] {
            art.bind_anim_frame_count_for_test(name, 100);
        }
        rules.merge_art_data(&art);
        rules
    }

    fn building(current: i32) -> (Simulation, u64) {
        let mut sim = Simulation::new();
        let id = sim.allocate_stable_id();
        let mut entity = GameEntity::test_default(id, "B", "A", 2, 2);
        entity.type_ref = sim.interner.intern("B");
        entity.owner = sim.interner.intern("A");
        entity.category = crate::map::entities::EntityCategory::Structure;
        entity.health.current = current;
        entity.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(entity);
        (sim, id)
    }

    #[test]
    fn original_1728_power_rows_use_real_slots_pause_and_scalar_deletion() {
        #[derive(serde::Deserialize)]
        struct Input {
            restoring: bool,
            slot: u8,
            flags: u8,
            occupied: bool,
            replay: bool,
            placed: bool,
            delayed: bool,
            current: i32,
            strength: i32,
            active_powered: bool,
        }
        #[derive(serde::Deserialize)]
        struct Replacement {
            slot: u8,
            name: String,
            damaged: u8,
            garrisoned: u8,
            extra: u32,
        }
        #[derive(serde::Deserialize)]
        struct Output {
            occupied: Vec<bool>,
            replay: Vec<u8>,
            paused: u8,
            deleted: Vec<u8>,
            replacements: Vec<Replacement>,
        }
        #[derive(serde::Deserialize)]
        struct Row {
            input: Input,
            output: Output,
        }
        let rows: Vec<Row> = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/building_slot_power.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 1728);
        let mut normal = rules(100, "");
        let mut zero = rules(0, "");
        for (index, row) in rows.into_iter().enumerate() {
            let i = row.input;
            let rules = if i.strength == 0 {
                &mut zero
            } else {
                &mut normal
            };
            let entry = rules.art_registry.get_mut("B").unwrap();
            entry.building_anim_power = [BuildingAnimPowerFlags {
                powered: false,
                ..Default::default()
            }; 21];
            entry.building_anim_power[3].powered = i.active_powered;
            entry.building_anim_power[usize::from(i.slot)] = BuildingAnimPowerFlags {
                powered: i.flags & 1 != 0,
                powered_light: i.flags & 2 != 0,
                powered_effect: i.flags & 4 != 0,
                powered_special: false,
            };
            entry.is_anim_delayed_fire = i.delayed;
            let (mut sim, id) = building(i.current);
            let damaged = requested_damage_state(
                sim.entities().get(id).unwrap().health,
                i.strength,
                rules.general.condition_yellow,
            );
            // Native corpus hooks451890 at the allocation boundary. Match its
            // no nested flag-transition domain while using real constructors.
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .building_damage_state_active = damaged;
            let old = i.occupied.then(|| {
                sim.set_building_anim_slot(id, i.slot, damaged, false, 0, rules)
                    .unwrap()
            });
            if let Some(old) = old {
                sim.substrate.anims.get_mut(old).unwrap().runtime.paused = i.restoring;
            }
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.building_actually_placed = i.placed;
            entity.building_anim_effect_replay[usize::from(i.slot)] = i.replay;
            sim.apply_building_anim_power(id, i.restoring, rules);
            let entity = sim.entities().get(id).unwrap();
            assert_eq!(
                entity.building_anim_slots.map(|id| id.is_some()).as_slice(),
                row.output.occupied,
                "occupied row {index}"
            );
            assert_eq!(
                entity.building_anim_effect_replay.map(u8::from).as_slice(),
                row.output.replay,
                "replay row {index}"
            );
            if let Some(old) = old {
                // The policy corpus hooks451890 before its old-slot destructor.
                // Actual construction additionally deletes a replaced slot at
                //451A1B..451A2F, covered by the replacement corpus.
                assert_eq!(
                    sim.anim(old).is_none(),
                    row.output.deleted.contains(&i.slot)
                        || row.output.replacements.iter().any(|r| r.slot == i.slot),
                    "policy deletion or constructor replacement row {index}"
                );
                if let Some(anim) = sim.anim(old) {
                    assert_eq!(
                        u8::from(anim.runtime.paused),
                        row.output.paused,
                        "pause row {index}"
                    );
                }
            }
            let created: Vec<_> = sim
                .substrate
                .anims
                .iter()
                .filter(|(anim, _)| Some(**anim) != old)
                .map(|(_, anim)| {
                    (
                        anim.building_slot.unwrap().1,
                        sim.interner.resolve(anim.type_id).to_string(),
                    )
                })
                .collect();
            let expected: Vec<_> = row
                .output
                .replacements
                .into_iter()
                .map(|r| {
                    assert_eq!((r.garrisoned, r.extra), (0, 0));
                    assert_eq!(r.damaged != 0, r.name.starts_with('D'));
                    (r.slot, r.name[..1].to_string())
                })
                .collect();
            assert_eq!(created, expected, "constructors row {index}");
        }
    }

    #[test]
    fn operational_edge_pauses_real_animation_without_stopping_its_timer() {
        let rules = rules(100, "Powered=yes\nPower=-10");
        let (mut sim, id) = building(100);
        let owner = sim.entities().get(id).unwrap().owner();
        sim.initialize_completed_building_anims(id, &rules);
        let anim = sim.entities().get(id).unwrap().building_anim_slots[3].unwrap();
        assert!(
            sim.anim(anim).unwrap().runtime.paused,
            "4467D0 initial pause"
        );
        sim.visit_building_operational(id, &rules);
        assert!(!sim.anim(anim).unwrap().runtime.paused);
        sim.substrate
            .anims
            .get_mut(anim)
            .unwrap()
            .runtime
            .first_ai_guard = false;
        sim.substrate
            .anims
            .get_mut(anim)
            .unwrap()
            .runtime
            .frame_timer = CdTimer::started(0, 3);
        sim.power_states.entry(owner).or_default().total_drain = 100;
        sim.session.binary_frame = 20;
        sim.visit_building_operational(id, &rules);
        sim.visit_anim(anim, &rules, None);
        assert_eq!(sim.anim(anim).unwrap().runtime.current_frame, 0);
        assert_eq!(sim.anim(anim).unwrap().runtime.frame_timer.start_frame(), 0);
        sim.power_states.get_mut(&owner).unwrap().total_output = 100;
        sim.visit_building_operational(id, &rules);
        sim.visit_anim(anim, &rules, None);
        assert_eq!(sim.anim(anim).unwrap().runtime.current_frame, 1);
        assert_eq!(
            sim.anim(anim).unwrap().runtime.frame_timer.start_frame(),
            20
        );
    }

    #[test]
    fn original_192_building_edges_keep_normal_and_special_outages_distinct() {
        let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/building_power_dispatch.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 192);
        for (index, row) in rows.into_iter().enumerate() {
            let i = &row["input"];
            let o = &row["output"];
            let current = i["current"].as_i64().unwrap() as i32;
            let mut rules = rules(
                100,
                &format!(
                    "Powered={}\nPoweredSpecial={}\nPower={}",
                    i["powered"],
                    i["special"],
                    (i["drain"].as_i64().unwrap() as i32).wrapping_neg()
                ),
            );
            let entry = rules.art_registry.get_mut("B").unwrap();
            entry.building_anim_power = [BuildingAnimPowerFlags {
                powered: false,
                ..Default::default()
            }; 21];
            entry.building_anim_power[3].powered = true;
            entry.building_anim_power[10].powered_special = true;
            let (mut sim, id) = building(current);
            let damaged = requested_damage_state(
                sim.entities().get(id).unwrap().health,
                100,
                rules.general.condition_yellow,
            );
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .building_damage_state_active = damaged;
            let old3 = sim
                .set_building_anim_slot(id, 3, damaged, false, 0, &rules)
                .unwrap();
            let old10 = sim
                .set_building_anim_slot(id, 10, damaged, false, 0, &rules)
                .unwrap();
            sim.substrate.anims.get_mut(old3).unwrap().runtime.paused = false;
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.building_actually_placed = true;
            entity.building_last_operational = i["old"].as_bool().unwrap();
            let owner = entity.owner();
            let state = sim.power_states.entry(owner).or_default();
            state.total_output = i["output"].as_i64().unwrap() as i32;
            state.total_drain = 100;
            state.is_low_power = state.total_output < 100;
            state.power_blackout_remaining = i["outage"].as_u64().unwrap() as u32;
            sim.visit_building_operational(id, &rules);
            let entity = sim.entities().get(id).unwrap();
            assert_eq!(
                entity.building_last_operational,
                o["operational"].as_bool().unwrap(),
                "sample row {index}"
            );
            assert_eq!(
                sim.anim(old3).unwrap().runtime.paused,
                o["paused"].as_bool().unwrap(),
                "pause row {index}"
            );
            let occupied: Vec<bool> = serde_json::from_value(o["occupied"].clone()).unwrap();
            assert_eq!(
                entity
                    .building_anim_slots
                    .map(|slot| slot.is_some())
                    .as_slice(),
                occupied,
                "slots row {index}"
            );
            let replacements = o["replacements"].as_array().unwrap();
            let actual: Vec<_> = sim
                .substrate
                .anims
                .iter()
                .filter(|(anim, _)| **anim != old3 && **anim != old10)
                .map(|(_, anim)| {
                    (
                        anim.building_slot.unwrap().1,
                        sim.interner.resolve(anim.type_id).to_string(),
                    )
                })
                .collect();
            let expected: Vec<_> = replacements
                .iter()
                .map(|r| {
                    (
                        r["slot"].as_u64().unwrap() as u8,
                        r["name"].as_str().unwrap()[..1].to_string(),
                    )
                })
                .collect();
            assert_eq!(actual, expected, "constructors row {index}");
        }
    }

    #[test]
    fn slot_power_metadata_changes_configuration_identity() {
        let mut rules = rules(100, "");
        let base = rules.simulation_config_hash();
        rules.art_registry.get_mut("B").unwrap().building_anim_power[10].powered_effect = true;
        assert_ne!(base, rules.simulation_config_hash());
        rules.art_registry.get_mut("B").unwrap().building_anim_power[10].powered_effect = false;
        rules
            .art_registry
            .get_mut("B")
            .unwrap()
            .is_anim_delayed_fire = true;
        assert_ne!(base, rules.simulation_config_hash());
    }
}
