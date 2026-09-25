//! Ordered building animation producers. Slot identity and animation runtime
//! belong to building_art and AnimStore; this finalizer owns no frame timer.

use super::Simulation;
use crate::rules::art_data::{ArtRegistry, BuildingAnimKind};
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::production;

pub(crate) fn finalize(
    sim: &mut Simulation,
    placed_building_owners: &[InternedId],
    _frame_committed: bool,
    rules: Option<&RuleSet>,
) {
    let Some(rules) = rules else {
        return;
    };
    for &owner in placed_building_owners {
        let owner = sim.interner.resolve(owner).to_owned();
        trigger_crane_anim(sim, rules, &rules.art_registry, &owner);
    }
    // These are observations of synchronous Unit/Building producer calls.
    // Replaying them here would delay constructor IDs, Logic visits and RNG.
    sim.bale_events.clear();
    sim.bunker_wall_events.clear();
}

fn trigger_crane_anim(sim: &mut Simulation, rules: &RuleSet, _art: &ArtRegistry, owner: &str) {
    let Some(producer) = production::active_producer_for_owner_category(
        sim,
        rules,
        owner,
        production::ProductionCategory::Building,
    ) else {
        return;
    };
    let id = producer.stable_id;
    let Some(entity) = sim.entities().get(id) else {
        return;
    };
    let name = sim.interner.resolve(entity.type_ref());
    let Some(object) = rules.object(name) else {
        return;
    };
    let Some(art) = rules
        .art_registry
        .resolve_metadata_entry(name, &object.image)
    else {
        return;
    };
    let slots: Vec<_> = art
        .building_anims
        .iter()
        .filter(|config| {
            matches!(
                config.kind,
                BuildingAnimKind::Active | BuildingAnimKind::Production
            ) && config.loop_count >= 0
        })
        .map(|config| config.native_slot)
        .collect();
    let damaged = crate::sim::building_art::requested_damage_state(
        entity.health,
        object.strength,
        rules.general.condition_yellow,
    );
    for slot in slots {
        sim.set_building_anim_slot(id, slot, damaged, false, 0, rules);
    }
}

fn at_or_below_condition_yellow(current: i32, strength: i32, yellow: f64) -> bool {
    crate::sim::building_art::requested_damage_state(
        crate::sim::components::Health { current },
        strength,
        yellow,
    )
}

/// Mission_Unload's first pass (`0x0073E013..0x0073E08E`): the unload
/// building's PreProductionAnim (slot 7) with the live damaged argument. No
/// stock refinery art defines it, so this plays nothing in retail.
pub(crate) fn start_refinery_unload(sim: &mut Simulation, rules: &RuleSet, building_id: u64) {
    let Some(building) = sim.entities().get(building_id) else {
        return;
    };
    let Some(object) = rules.object(sim.interner.resolve(building.type_ref())) else {
        return;
    };
    let damaged = at_or_below_condition_yellow(
        building.health.current,
        object.strength,
        rules.general.condition_yellow,
    );
    sim.set_building_anim_slot(building_id, 7, damaged, false, 0, rules);
}

/// Unit73E37A: emit smoke on every due gate, then if Special slot10 is null
/// call451750 with the live damaged argument. The empty gate clears it after
/// that call, so its constructor/identity/RNG effects are retained even empty.
pub(crate) fn begin_refinery_unload_gate(sim: &mut Simulation, rules: &RuleSet, building_id: u64) {
    let Some(building) = sim.entities().get(building_id) else {
        return;
    };
    let Some(object) = rules.object(sim.interner.resolve(building.type_ref())) else {
        return;
    };
    let damaged = at_or_below_condition_yellow(
        building.health.current,
        object.strength,
        rules.general.condition_yellow,
    );
    let raw = crate::sim::movement::ground_pose::position_world_coord(&building.position);
    let origin = glam::IVec3::new(raw.x, raw.y, raw.z);
    let lifetime = object.refinery_smoke_frames;
    let particle_type = object
        .refinery_smoke_particle_system
        .as_deref()
        .and_then(|name| rules.ps_type_id_by_name(name));
    let offsets = object.refinery_smoke_offsets;
    if let Some(particle_type) = particle_type {
        for coord in refinery_smoke_coords(origin, offsets) {
            if let Some(system) = sim.spawn_particle_system(
                particle_type,
                coord,
                None,
                Some(building_id),
                glam::IVec3::ZERO,
                None,
                rules,
            ) {
                // Original4599CD calls6301F0 after each constructor.
                sim.particle_systems_mut().get_mut(system).unwrap().lifetime = lifetime;
            }
        }
    }
    if sim
        .entities()
        .get(building_id)
        .is_some_and(|entity| entity.building_anim_slots[10].is_none())
    {
        sim.set_building_anim_slot(building_id, 10, damaged, false, 0, rules);
    }
}

/// Original459900's four ordered calls. The global empty-coordinate sentinel
/// is initialized to128,128,0 by43B110, separately from the zero offset.
fn refinery_smoke_coords(
    origin: glam::IVec3,
    offsets: [glam::IVec3; 4],
) -> impl Iterator<Item = glam::IVec3> {
    offsets
        .into_iter()
        .filter(|offset| *offset != glam::IVec3::ZERO && *offset != glam::IVec3::new(128, 128, 0))
        .map(move |offset| {
            glam::IVec3::new(
                origin.x.wrapping_add(offset.x),
                origin.y.wrapping_add(offset.y),
                origin.z.wrapping_add(offset.z),
            )
        })
}

/// Unit73E4DC..73E534 starts Production8 before clearing Special10.
pub(crate) fn end_refinery_unload_empty(sim: &mut Simulation, rules: &RuleSet, building_id: u64) {
    let Some(building) = sim.entities().get(building_id) else {
        return;
    };
    let Some(object) = rules.object(sim.interner.resolve(building.type_ref())) else {
        return;
    };
    let damaged = at_or_below_condition_yellow(
        building.health.current,
        object.strength,
        rules.general.condition_yellow,
    );
    if object.refinery {
        sim.set_building_anim_slot(building_id, 8, damaged, false, 0, rules);
    }
    sim.clear_building_anim_slot(building_id, 10);
}

/// Building459254..459665 executes these constructor/deletion calls inline.
pub(crate) fn set_bunker_wall_slots(
    sim: &mut Simulation,
    rules: &RuleSet,
    building_id: u64,
    up: bool,
    damaged: bool,
) {
    let slots: &[u8] = if up { &[10, 11] } else { &[12, 13] };
    if !up {
        sim.clear_building_anim_slot(building_id, 10);
        sim.clear_building_anim_slot(building_id, 11);
    }
    for &slot in slots {
        sim.set_building_anim_slot(building_id, slot, damaged, false, 0, rules);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;
    use crate::sim::components::BaleDepositEvent;
    use crate::sim::game_entity::GameEntity;
    #[test]
    fn original_210_refinery_smoke_rows_and_140_represented_producer_calls() {
        let corpus: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tools/spatial_oracle/refinery_smoke.json"
        ))
        .unwrap();
        let vector = |value: &serde_json::Value| {
            glam::IVec3::new(
                value[0].as_i64().unwrap() as i32,
                value[1].as_i64().unwrap() as i32,
                value[2].as_i64().unwrap() as i32,
            )
        };
        let rows = corpus["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 210);
        let mut represented = 0;
        for row in rows {
            let input = &row["input"];
            let origin = vector(&input["origin"]);
            let offsets = std::array::from_fn(|n| vector(&input["offsets"][n]));
            let expected = row["output"].as_array().unwrap();
            let coords: Vec<_> = if input["has_type"] == true {
                refinery_smoke_coords(origin, offsets).collect()
            } else {
                Vec::new()
            };
            assert_eq!(
                coords,
                expected
                    .iter()
                    .map(|call| vector(&call["coords"]))
                    .collect::<Vec<_>>(),
                "{row}"
            );
            // Position's retained cell/subcell XY cannot represent the final
            // signed-MAX/MIN origin; the same production wrapping kernel above
            // covers those70 rows without claiming an unavailable adapter.
            if !(0..=0x00ff_ffff).contains(&origin.x) || !(0..=0x00ff_ffff).contains(&origin.y) {
                continue;
            }
            let mut text = format!(
                "[BuildingTypes]\n0=GAREFN\n[GAREFN]\nStrength=100\nRefinery=yes\nDockUnload=yes\nRefinerySmokeFrames={}\n",
                input["frames"]
            );
            if input["has_type"] == true {
                text.push_str("RefinerySmokeParticleSystem=Sys\n");
            }
            for (suffix, offset) in ["One", "Two", "Three", "Four"].into_iter().zip(offsets) {
                text.push_str(&format!(
                    "RefinerySmokeOffset{suffix}={},{},{}\n",
                    offset.x, offset.y, offset.z
                ));
            }
            text.push_str("[Particles]\n0=Smk\n[ParticleSystems]\n0=Sys\n[Smk]\nBehavesLike=Smoke\nMaxEC=10\n[Sys]\nBehavesLike=Smoke\nHoldsWhat=Smk\nLifetime=200\n");
            let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
            let mut sim = Simulation::new();
            let id = sim.allocate_stable_id();
            let mut entity = GameEntity::test_default(
                id,
                "GAREFN",
                "A",
                (origin.x >> 8) as u16,
                (origin.y >> 8) as u16,
            );
            entity.type_ref = sim.interner.intern("GAREFN");
            entity.owner = sim.interner.intern("A");
            entity.position.sub_x = crate::util::fixed_math::SimFixed::from_num(origin.x & 255);
            entity.position.sub_y = crate::util::fixed_math::SimFixed::from_num(origin.y & 255);
            entity.position.exact_z_leptons = Some(origin.z);
            sim.substrate.entities.insert(entity);
            begin_refinery_unload_gate(&mut sim, &rules, id);
            let actual: Vec<_> = sim
                .particle_systems()
                .iter()
                .map(|(_, system)| system)
                .collect();
            assert_eq!(actual.len(), expected.len(), "{row}");
            for (system, call) in actual.into_iter().zip(expected) {
                assert_eq!(system.coords, vector(&call["coords"]), "{row}");
                assert_eq!(system.target_coords, vector(&call["target"]));
                assert_eq!(system.lifetime, call["lifetime"].as_i64().unwrap() as i32);
                assert_eq!(system.owner_entity, Some(id));
                assert_eq!(system.attached_entity, None);
                assert_eq!(system.owner_house, None);
            }
            represented += 1;
        }
        assert_eq!(represented, 140);
    }
    fn insert_building(sim: &mut Simulation, id: u64, name: &str, rx: u16, ry: u16) {
        let mut entity = GameEntity::test_default(id, name, "Americans", rx, ry);
        entity.type_ref = sim.interner.intern(name);
        entity.owner = sim.interner.intern("Americans");
        entity.category = crate::map::entities::EntityCategory::Structure;
        entity.health.current = 100;
        sim.entities_mut().insert(entity);
    }
    fn refinery_sim_with_bale() -> Simulation {
        let mut sim = Simulation::new();
        insert_building(&mut sim, 41, "GAREFN", 7, 9);
        sim.bale_events.push(BaleDepositEvent {
            building_id: 41,
            tick: 12,
            drained: true,
            empty: false,
        });
        sim
    }
    fn refinery_rules_and_art() -> RuleSet {
        let rules_ini = IniFile::from_str(
            "[BuildingTypes]\n\
             0=GAREFN\n\
             1=GAWALL\n\
             [Particles]\n\
             0=RefSmokeParticle\n\
             [ParticleSystems]\n\
             0=RefSmokeSystem\n\
             [Animations]\n\
             0=GAREFN_B\n\
             [GAREFN]\n\
             Refinery=yes\nDockUnload=yes\n\
             Strength=100\n\
             Image=GAREFN\n\
             RefinerySmokeParticleSystem=RefSmokeSystem\n\
             RefinerySmokeOffsetOne=10,-20,30\n\
             [GAWALL]\n\
             Wall=yes\n\
             [RefSmokeParticle]\n\
             BehavesLike=Smoke\n\
             MaxEC=10\n\
             MaxDC=4\n\
             StartStateAI=0\n\
             EndStateAI=10\n\
             StateAIAdvance=4\n\
             [RefSmokeSystem]\n\
             BehavesLike=Smoke\n\
             HoldsWhat=RefSmokeParticle\n\
             Spawns=yes\n\
             ParticleCap=10\n\
             SpawnFrames=1\n\
             Lifetime=200\n",
        );
        let art_ini = IniFile::from_str(
            "[GAREFN]\n\
             SpecialAnim=GAREFN_B\n\
             [GAREFN_B]\n\
             Start=2\n\
             LoopStart=1\n\
             LoopEnd=5\n\
             Rate=300\n",
        );
        let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini)
            .expect("refinery animation rules");
        let mut art = ArtRegistry::from_ini(&art_ini);
        art.bind_anim_frame_count_for_test("GAREFN_B", 12);
        rules.merge_art_data(&art);
        rules
    }
    #[test]
    fn placement_owner_fact_requires_success_and_skips_walls() {
        use crate::sim::command::{Command, CommandEnvelope};

        let rules = refinery_rules_and_art();
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let refinery = sim.interner.intern("GAREFN");
        let wall = sim.interner.intern("GAWALL");
        let refinery_command = CommandEnvelope::new(
            owner,
            1,
            Command::PlaceReadyBuilding {
                owner,
                type_id: refinery,
                rx: 4,
                ry: 5,
            },
        );
        let wall_command = CommandEnvelope::new(
            owner,
            2,
            Command::PlaceReadyBuilding {
                owner,
                type_id: wall,
                rx: 6,
                ry: 5,
            },
        );

        assert_eq!(
            sim.successful_non_wall_placement_owner(&refinery_command, false, Some(&rules)),
            None,
            "a rejected placement cannot borrow an unrelated spawn signal"
        );
        assert_eq!(
            sim.successful_non_wall_placement_owner(&wall_command, true, Some(&rules)),
            None,
            "wall overlays do not arm a producer crane"
        );
        assert_eq!(
            sim.successful_non_wall_placement_owner(&refinery_command, true, Some(&rules)),
            Some(owner)
        );
    }

    #[test]
    fn headless_and_app_frames_share_synchronous_bale_authority() {
        let rules = refinery_rules_and_art();
        let height_map = std::collections::BTreeMap::new();
        let mut app_sim = refinery_sim_with_bale();
        let mut headless_sim = refinery_sim_with_bale();
        begin_refinery_unload_gate(&mut app_sim, &rules, 41);
        begin_refinery_unload_gate(&mut headless_sim, &rules, 41);

        let app_output = app_sim
            .advance_app_frame(
                &[],
                Some(&rules),
                &height_map,
                None,
                67,
                crate::sim::world::TickLane::Ordinary,
                None,
            )
            .expect("fixture frame must complete");
        let headless_tick =
            headless_sim.advance_tick(&[], Some(&rules), &height_map, None, None, 67);

        assert_eq!(app_output.tick.state_hash, headless_tick.state_hash);
        assert_eq!(app_output.tick.state_hash, app_sim.state_hash());
        assert_eq!(headless_tick.state_hash, headless_sim.state_hash());
        assert_eq!(headless_sim.particle_systems().len(), 1);
        assert!(headless_sim.bale_events.is_empty());
        assert!(app_sim.bale_events.is_empty());
    }

    #[test]
    fn app_frame_hash_includes_synchronous_bale_slot_and_particle_state() {
        let rules = refinery_rules_and_art();
        let mut sim = Simulation::new();
        insert_building(&mut sim, 41, "GAREFN", 7, 9);
        sim.bale_events.push(BaleDepositEvent {
            building_id: 41,
            tick: 12,
            drained: true,
            empty: false,
        });

        begin_refinery_unload_gate(&mut sim, &rules, 41);
        let output = sim
            .advance_app_frame(
                &[],
                Some(&rules),
                &std::collections::BTreeMap::new(),
                None,
                67,
                crate::sim::world::TickLane::Ordinary,
                None,
            )
            .expect("fixture frame must complete");

        assert!(output.tick.frame_committed);
        assert!(sim.bale_events.is_empty());
        assert!(
            sim.entities()
                .get(41)
                .expect("refinery")
                .building_anim_slots[10]
                .is_some()
        );
        assert_eq!(sim.particle_systems().len(), 1);
        assert_eq!(output.tick.state_hash, sim.state_hash());
    }
    #[test]
    fn original_health_ratio_corpus_selects_real_refinery_slot_variant() {
        for row in crate::sim::health_ratio_fixture::rows() {
            let art_ini = IniFile::from_str(
                "[GAREFN]\nSpecialAnim=NORMAL\nSpecialAnimDamaged=DAMAGED\n[NORMAL]\nLoopEnd=5\n[DAMAGED]\nLoopEnd=5\n",
            );
            let mut rules=RuleSet::from_ini_with_fixed_art_for_test(&IniFile::from_str(&format!(
                "[BuildingTypes]\n0=GAREFN\n[GAREFN]\nStrength={}\n[Animations]\n0=NORMAL\n1=DAMAGED\n",
                row.input.strength)), &art_ini).unwrap();
            rules.general.condition_yellow = row.input.yellow();
            let mut art = ArtRegistry::from_ini(&art_ini);
            for name in ["NORMAL", "DAMAGED"] {
                art.bind_anim_frame_count_for_test(name, 10);
            }
            rules.merge_art_data(&art);
            let mut sim = refinery_sim_with_bale();
            sim.entities_mut().get_mut(41).unwrap().health.current = row.input.current;
            begin_refinery_unload_gate(&mut sim, &rules, 41);
            finalize(&mut sim, &[], true, Some(&rules));
            let id = sim.entities().get(41).unwrap().building_anim_slots[10].unwrap();
            assert_eq!(
                sim.interner.resolve(sim.anim(id).unwrap().type_id),
                if row.output.refinery_special_damaged {
                    "DAMAGED"
                } else {
                    "NORMAL"
                },
                "{row:?}"
            );
            assert_eq!(
                at_or_below_condition_yellow(
                    row.input.current,
                    row.input.strength,
                    row.input.yellow()
                ),
                row.output.refinery_active_damaged,
                "{row:?}"
            );
        }
    }
    #[test]
    fn missing_damaged_name_smokes_without_constructing_or_changing_flag() {
        let rules = refinery_rules_and_art();
        let mut sim = refinery_sim_with_bale();
        sim.entities_mut().get_mut(41).unwrap().health.current = 50;
        begin_refinery_unload_gate(&mut sim, &rules, 41);
        finalize(&mut sim, &[], true, Some(&rules));
        assert!(sim.bale_events.is_empty());
        assert_eq!(sim.particle_systems().len(), 1);
        let building = sim.entities().get(41).unwrap();
        assert_eq!(building.building_anim_slots[10], None);
        assert!(!building.building_damage_state_active);
    }
    #[test]
    fn empty_bale_constructs_then_scalar_deletes_and_drain_only_emits_once() {
        let rules = refinery_rules_and_art();
        let mut sim = refinery_sim_with_bale();
        sim.bale_events[0].empty = true;
        begin_refinery_unload_gate(&mut sim, &rules, 41);
        end_refinery_unload_empty(&mut sim, &rules, 41);
        finalize(&mut sim, &[], true, Some(&rules));
        assert!(sim.bale_events.is_empty());
        assert_eq!(sim.particle_systems().len(), 1);
        assert_eq!(
            sim.entities().get(41).unwrap().building_anim_slots[10],
            None
        );
        assert_eq!(sim.anims().count(), 0);
        let hash = sim.state_hash();
        finalize(&mut sim, &[], true, Some(&rules));
        assert_eq!(hash, sim.state_hash());
    }
}
