//! Manual primary Event17 / Building448160 and its retained-state consumers.
//! Original evidence: tools/spatial_oracle/building_manual_primary.md.

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;
use crate::util::save_codec;

fn retail_factories() -> Option<(RuleSet, Simulation, u64, u64)> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")?;
    let rules = retail.rules;
    let mut sim = Simulation::with_seed(0x0044_8160);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.game_mode_nonzero = true;
    let owner = sim.intern("Americans");
    let side = rules.country_side_index("Americans").unwrap().0;
    let mut house = HouseState::new(owner, side, Some(owner), true, 50_000, 10);
    house.project_country_mults(&rules, &sim.interner);
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    sim.session.current_house = Some(owner);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    let first = sim.spawn_object("GAWEAP", "Americans", 6, 6, 0, &rules)?;
    let second = sim.spawn_object("GAWEAP", "Americans", 20, 6, 0, &rules)?;
    assert!(sim.production.is_primary_factory(first));
    assert!(!sim.production.is_primary_factory(second));
    Some((rules, sim, first, second))
}

#[test]
fn primary_flag_survives_limbo_until_physical_disposal() {
    let Some((_rules, mut sim, first, second)) = retail_factories() else {
        return;
    };
    sim.uninit(first);
    assert!(
        sim.substrate
            .entities
            .get(first)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert!(
        sim.production.is_primary_factory(first),
        "Limbo preserves +3D3"
    );
    sim.process_pending_delete();
    assert!(!sim.substrate.entities.contains(first));
    assert!(
        !sim.production.is_primary_factory(first),
        "the primary flag must not outlive the physically disposed Building"
    );
    assert!(
        !sim.production.is_primary_factory(second),
        "disposal does not elect a replacement"
    );
}

#[test]
fn stale_primary_events_preserve_then_toggle_each_limbo_peer_flag() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;

    let Some((rules, mut sim, first, second)) = retail_factories() else {
        return;
    };
    // This is the original Event17 receiver's accepted retained-state domain.
    // A common retail producer of an already-primary limbo peer is not claimed.
    sim.techno_limbo_with_rules(first, &rules, None);
    let toggle = |sim: &mut Simulation, entity_id| {
        assert!(sim.apply_command(
            "Americans",
            &Command::TogglePrimaryFactory { entity_id },
            Some(&rules)
        ));
    };
    toggle(&mut sim, second);
    assert!(sim.production.is_primary_factory(first));
    assert!(sim.production.is_primary_factory(second));
    // Full snapshot load reseeds the unsaved Scenario stream to zero.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    // Compare this owner across full graph registration. Derived navigation
    // and the process RNG streams have their own native load policies.
    let primary_hash = |sim: &Simulation| {
        let mut hasher = DefaultHasher::new();
        sim.production.hash_primary_factories(&mut hasher);
        hasher.finish()
    };
    let retained_hash = primary_hash(&sim);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    assert!(restored.production.is_primary_factory(first));
    assert!(restored.production.is_primary_factory(second));
    restored
        .restore_after_snapshot_load()
        .expect("canonical retained Building graph restores");
    assert!(restored.production.is_primary_factory(first));
    assert!(restored.production.is_primary_factory(second));
    assert_eq!(primary_hash(&restored), retained_hash);
    toggle(&mut restored, first);
    assert!(!restored.production.is_primary_factory(first));
    assert!(restored.production.is_primary_factory(second));
    toggle(&mut restored, first);
    assert!(restored.production.is_primary_factory(first));
    assert!(!restored.production.is_primary_factory(second));
}

fn manual_primary_corpus() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/spatial_oracle/building_manual_primary.json")
}

fn assert_native_primary_type(rules: &RuleSet, name: &str, expected: &serde_json::Value) {
    use crate::rules::object_type::BuildCategory;
    use serde_json::json;

    let object = rules.object(name).unwrap();
    let build_category = match object.build_cat {
        None => 0,
        Some(BuildCategory::Tech) => 1,
        Some(BuildCategory::Resource) => 2,
        Some(BuildCategory::Power) => 3,
        Some(BuildCategory::Infrastructure) => 4,
        Some(BuildCategory::Combat) => 5,
    };
    assert_eq!(
        json!({
            "factory": object.factory_rtti(),
            "build_category": build_category,
            "construction_yard": u8::from(object.construction_yard),
            "foundation": crate::rules::foundation::foundation_id(&object.foundation),
            "gap_generator": u8::from(object.gap_generator),
            "infantry_absorb": u8::from(object.infantry_absorb),
            "invisible_in_game": u8::from(object.invisible_in_game),
            "naval": u8::from(object.naval),
            "ownable": crate::sim::ai_buildable::house_token_mask(&object.owner, rules),
            "passengers": object.passengers,
            "strength": object.strength,
            "unit_absorb": u8::from(object.unit_absorb),
        }),
        *expected,
        "{name}: original retained native type-reader inputs"
    );
}

fn manual_primary_case_rules(
    retail: &crate::rules::retail_ini_fixture::RetailBattleRules,
    case: &serde_json::Value,
) -> (RuleSet, Vec<String>) {
    use crate::rules::ini_parser::IniFile;

    let mut ini = (*retail.processed_rules).clone();
    let mut names = Vec::new();
    let mut changed = false;
    for (index, actor) in case["input"]["actors"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let name = actor["type"].as_str().unwrap();
        let Some(overrides) = actor.get("type_overrides") else {
            names.push(name.to_owned());
            continue;
        };
        // Native clones only this actor's Type for its supplied control.
        // Parse a distinct copied section; never change every GAWEAP actor.
        let control = format!("PRIMARY_NATIVE_ACTOR_{index}");
        let mut section = ini.section(name).unwrap().clone();
        section.set("Image", name);
        for (key, value) in overrides.as_object().unwrap() {
            let key = match key.as_str() {
                "naval" => "Naval",
                "invisible_in_game" => "InvisibleInGame",
                other => panic!("unsupported native per-actor type control {other}"),
            };
            section.set(
                key,
                if value.as_u64().unwrap() == 0 {
                    "no"
                } else {
                    "yes"
                },
            );
        }
        let mut patch = format!("[BuildingTypes]\n{control}={control}\n[{control}]\n");
        for (key, value) in section.raw_entries() {
            patch.push_str(&format!("{key}={value}\n"));
        }
        ini.merge(&IniFile::from_str(&patch));
        names.push(control);
        changed = true;
    }
    let rules = if changed {
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &retail.fixed_art).unwrap()
    } else {
        retail.rules.clone()
    };
    for (name, expected) in manual_primary_corpus()["native_type_inputs"]
        .as_object()
        .unwrap()
    {
        assert_native_primary_type(&rules, name, expected);
    }
    for (actor, name) in case["input"]["actors"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&names)
    {
        let mut expected =
            manual_primary_corpus()["native_type_inputs"][actor["type"].as_str().unwrap()].clone();
        if let Some(overrides) = actor.get("type_overrides") {
            for (key, value) in overrides.as_object().unwrap() {
                expected[key] = value.clone();
            }
        }
        assert_native_primary_type(&rules, name, &expected);
    }
    let query = rules.object("MTNK").unwrap();
    assert_eq!(
        query.category,
        crate::rules::object_type::ObjectCategory::Vehicle
    );
    assert_eq!(
        u8::from(query.naval),
        manual_primary_corpus()["query_unit"]["naval"]
    );
    assert_eq!(
        crate::sim::ai_buildable::house_token_mask(&query.owner, &rules),
        manual_primary_corpus()["query_unit"]["ownable"]
            .as_u64()
            .unwrap() as u32
    );
    (rules, names)
}

fn supply_primary_actor_flags(sim: &mut Simulation, id: u64, flags: &serde_json::Value) {
    use crate::sim::mission::state::MissionTestFixture;
    use crate::sim::mission::{MissionDispatchTimer, MissionId};

    let entity = sim.substrate.entities.get_mut(id).unwrap();
    if let Some(alive) = flags["alive"].as_u64() {
        entity.lifecycle.set_object_alive_for_test(alive != 0);
    }
    if let Some(limbo) = flags["limbo"].as_u64() {
        entity.lifecycle.set_in_limbo_for_test(limbo != 0);
    }
    if let Some(health) = flags["health"].as_i64() {
        entity.health.set_for_test(health as i32);
    }
    if let Some(online) = flags["online"].as_u64() {
        // Supplied online-latch control, using its existing Rust owner.
        // This does not claim a real CLEG/lifecycle producer of this prior.
        entity.set_temporal_for_test(if online == 0 {
            crate::sim::temporal::TemporalState::warped_by_for_test(id)
        } else {
            Default::default()
        });
    }
    entity.mission.apply_test_fixture(MissionTestFixture {
        current: MissionId::from_raw(
            flags["mission"]
                .as_i64()
                .map_or(entity.mission.current().raw(), |raw| raw as i32),
        ),
        suspended: MissionId::NONE,
        queued: MissionId::from_raw(
            flags["queued_mission"]
                .as_i64()
                .map_or(entity.mission.queued().raw(), |raw| raw as i32),
        ),
        movement_bypass_latch: 0,
        handler_state: 0,
        mission_start_frame: 0,
        ai_counter: 0,
        dispatch_timer: MissionDispatchTimer::from_raw(0, 0),
    });
}

fn assert_primary_native_state(
    sim: &Simulation,
    rules: &RuleSet,
    ids: &[u64],
    owners: &[crate::sim::intern::InternedId; 2],
    expected: &serde_json::Value,
    context: &str,
) {
    use serde_json::json;

    let primary: Vec<_> = ids
        .iter()
        .map(|&id| u8::from(sim.production.is_primary_factory(id)))
        .collect();
    assert_eq!(
        json!(primary),
        expected["primary"],
        "{context}: every retained +3D3 byte"
    );
    let found = super::can_build::find_factory(
        sim,
        rules,
        owners[0],
        rules.object("MTNK").unwrap(),
        false,
        false,
        false,
    );
    assert_eq!(
        found.map(|id| ids.iter().position(|&actor| actor == id).unwrap() + 1),
        expected["find_factory"].as_u64().map(|id| id as usize),
        "{context}: independent live MTNK FindFactory identity"
    );
    for (&id, native) in ids.iter().zip(expected["actors"].as_array().unwrap()) {
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            json!({
                "alive": u8::from(entity.lifecycle.object_alive()),
                "limbo": u8::from(entity.lifecycle.in_limbo()),
                "health": entity.health.current(),
                "online": u8::from(entity.building_online()),
                "mission": entity.mission.current().raw(),
                "queued_mission": entity.mission.queued().raw(),
            }),
            native["flags"],
            "{context}: actor{id} flags"
        );
        assert_eq!(u8::from(entity.selection.is_selected()), native["selected"]);
        assert_eq!(u8::from(entity.lifecycle.cell_marked()), native["marked"]);
        let timer = entity.mission.dispatch_timer();
        assert_eq!(
            [timer.start_frame(), timer.delay()],
            [
                native["mission_timer"][0].as_i64().unwrap() as i32,
                native["mission_timer"][2].as_i64().unwrap() as i32
            ],
            "{context}: actor{id} mission clock (unused padding excluded)"
        );
        let stage = entity.native_stage();
        assert_eq!(
            [
                stage.timer().start_frame(),
                stage.timer().duration(),
                stage.rate()
            ],
            [
                native["stage_timer"][0].as_i64().unwrap() as i32,
                native["stage_timer"][2].as_i64().unwrap() as i32,
                native["stage_timer"][3].as_i64().unwrap() as i32
            ],
            "{context}: actor{id} shared Stage clock"
        );
    }
    for (&owner, native) in owners.iter().zip(expected["houses"].as_array().unwrap()) {
        assert_eq!(
            u8::from(sim.houses[&owner].tech_recheck_pending()),
            native["recheck"]
        );
        let categories = [
            ProductionCategory::Aircraft,
            ProductionCategory::Infantry,
            ProductionCategory::Vehicle,
            ProductionCategory::Ship,
            ProductionCategory::Building,
            ProductionCategory::Defense,
        ];
        for (category, slot) in categories
            .into_iter()
            .zip(native["factory_slots"].as_array().unwrap())
        {
            assert_eq!(
                slot.as_u64(),
                Some(0),
                "{context}: native supplied empty queue slot"
            );
            assert!(
                sim.production.factories.view(owner, category).is_none(),
                "{context}: queue unchanged"
            );
        }
    }
}

#[test]
fn manual_primary_native_histories_preserve_retained_flags_consumers_and_streams() {
    use crate::sim::rng::{SimRng, trace_draws};
    use crate::sim::world::SimSoundEvent;

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let corpus = manual_primary_corpus();
    assert_eq!(
        corpus["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    assert_eq!(corpus["cases"].as_array().unwrap().len(), 6);
    for case in corpus["cases"].as_array().unwrap() {
        let name = case["input"]["name"].as_str().unwrap();
        let (rules, types) = manual_primary_case_rules(&retail, case);
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.session.game_mode_nonzero = true;
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let owners = [sim.intern("Americans"), sim.intern("Alliance")];
        for owner in owners {
            let side = rules
                .country_side_index(sim.interner.resolve(owner))
                .unwrap()
                .0;
            let mut house = HouseState::new(owner, side, Some(owner), true, 50_000, 10);
            house.project_country_mults(&rules, &sim.interner);
            house.take_tech_recheck();
            sim.houses.insert(owner, house);
            sim.session.house_order.push(owner);
        }
        sim.session.current_house = Some(owners[0]);
        let mut ids = Vec::new();
        for (index, (actor, kind)) in case["input"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&types)
            .enumerate()
        {
            let owner = owners[actor["owner"].as_u64().unwrap_or(0) as usize];
            let owner_name = sim.interner.resolve(owner).to_owned();
            let id = sim
                .spawn_object_limbo_at_height(
                    kind,
                    &owner_name,
                    6 + index as u16 * 3,
                    9,
                    0,
                    0,
                    &rules,
                )
                .unwrap();
            assert_eq!(
                u8::from(sim.production.is_primary_factory(id)),
                case["constructor_primary"][index]
            );
            // Both profiles supply scene admission rather than executing
            // full Unlimbo. Native overlapping 5x3 footprints are not a
            // claim about ordinary placement, occupancy or scene loading.
            supply_primary_actor_flags(&mut sim, id, &case["initial"]["actors"][index]["flags"]);
            let entity = sim.substrate.entities.get_mut(id).unwrap();
            entity.lifecycle.set_cell_marked(true);
            entity.selection.set_selected_for_test(false);
            sim.append_house_base_building_for_test(id);
            if actor["primary"].as_u64() == Some(1) {
                let category = super::production_tech::factory_category_for_building(
                    rules.object(kind).unwrap(),
                )
                .unwrap();
                sim.production.set_primary_factory(owner, category, id);
            }
            ids.push(id);
        }
        sim.sound_events.clear();
        let seed = corpus["seed"].as_u64().unwrap();
        sim.main_rng = SimRng::new(seed).into();
        sim.scenario_rng = SimRng::new(seed);
        sim.mapgen_rng = SimRng::new(seed);
        let assert_rng = |sim: &Simulation, expected: &serde_json::Value| {
            assert_eq!(
                sim.main_rng.native_state_hex(),
                expected["main"].as_str().unwrap(),
                "{name}: Main"
            );
            assert_eq!(
                sim.scenario_rng.native_state_hex(),
                expected["scenario"].as_str().unwrap(),
                "{name}: Scenario"
            );
            assert_eq!(
                sim.mapgen_rng.native_state_hex(),
                expected["mapgen"].as_str().unwrap(),
                "{name}: MapGen"
            );
        };
        assert_rng(&sim, &case["rng_before"]);
        assert_primary_native_state(&sim, &rules, &ids, &owners, &case["initial"], name);
        let factories = sim.production.factories.clone();
        let mut issued = Vec::new();
        let (_, draws) = trace_draws(|| {
            for phase in case["phases"].as_array().unwrap() {
                let input = &phase["input"];
                sim.session.binary_frame = phase["frame"].as_u64().unwrap() as u32;
                let actor_index = input["actor"].as_u64().map(|id| id as usize - 1);
                let id = actor_index.map_or(0x1234_5678, |index| ids[index]);
                if let Some(changes) = input.get("changes") {
                    supply_primary_actor_flags(&mut sim, id, changes);
                }
                let clocks: Vec<_> = ids
                    .iter()
                    .map(|&id| *sim.substrate.entities.get(id).unwrap().native_stage())
                    .collect();
                let mut houses_before = sim.houses.clone();
                let edges_before: Vec<_> = owners
                    .iter()
                    .map(|owner| sim.houses[owner].waypoint_edge)
                    .collect();
                sim.sound_events.clear();
                let mut event = None;
                match input["op"].as_str().unwrap() {
                    "automatic" => initialize_factory_primary(&mut sim, id, &rules),
                    "click" => {
                        for &selected in &ids {
                            sim.substrate
                                .entities
                                .get_mut(selected)
                                .unwrap()
                                .selection
                                .set_selected_for_test(selected == id);
                        }
                        let admitted = can_select_factory_primary(&sim, &rules, id);
                        assert_eq!(
                            u8::from(admitted) * 4,
                            phase["action"],
                            "{name}: native self-action admission"
                        );
                        assert_eq!(
                            u8::from(admitted),
                            phase["issued"],
                            "{name}: native click event count"
                        );
                        if admitted {
                            let owner = sim.substrate.entities.get(id).unwrap().owner();
                            issued.push((owner, id));
                            event = Some((owner, id));
                        }
                    }
                    "event" => {
                        let owner = actor_index.map_or(owners[0], |index| {
                            sim.substrate.entities.get(ids[index]).unwrap().owner()
                        });
                        issued.push((owner, id));
                        event = Some((owner, id));
                    }
                    "repeat_event" => {
                        event = Some(issued[input["event"].as_u64().unwrap() as usize])
                    }
                    other => panic!("unknown native operation {other}"),
                }
                if let Some((owner, entity_id)) = event {
                    let owner_name = sim.interner.resolve(owner).to_owned();
                    // Actual Event17 receiver owns resolution, planning detach,
                    // +90 and Factory gates; false is a legitimate control.
                    sim.apply_command(
                        &owner_name,
                        &Command::TogglePrimaryFactory { entity_id },
                        Some(&rules),
                    );
                }
                let context = format!("{name}/{}", phase["frame"]);
                assert_primary_native_state(&sim, &rules, &ids, &owners, &phase["state"], &context);
                assert_rng(&sim, &phase["rng"]);
                assert_eq!(
                    sim.production.factories, factories,
                    "{context}: no allocation/relink/queue mutation"
                );
                assert!(
                    sim.pending_command_snapshot().is_empty(),
                    "{context}: no invented follow-up command"
                );
                for (&id, clock) in ids.iter().zip(clocks) {
                    assert_eq!(
                        *sim.substrate.entities.get(id).unwrap().native_stage(),
                        clock,
                        "{context}: no Stage writes"
                    );
                }
                // Yard setting calls the existing edge owner. Its broader
                // geometry is an explicit separate bound; all other phases,
                // including already-primary yard clear, preserve every House
                // field and timer. Compare cloned state, not a mirror owner.
                let has_edge = phase["effects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|effect| effect["event"] == "determine_edge");
                for owner in owners {
                    if has_edge {
                        houses_before.get_mut(&owner).unwrap().waypoint_edge =
                            sim.houses[&owner].waypoint_edge;
                    }
                }
                assert_eq!(
                    save_codec::serialize(&sim.houses).unwrap(),
                    save_codec::serialize(&houses_before).unwrap(),
                    "{context}: House timers/invariants"
                );
                if !has_edge {
                    assert_eq!(
                        owners
                            .iter()
                            .map(|owner| sim.houses[owner].waypoint_edge)
                            .collect::<Vec<_>>(),
                        edges_before,
                        "{context}: no edge write"
                    );
                }
                let actual_eva: Vec<_> = sim
                    .sound_events
                    .iter()
                    .map(|event| match event {
                        SimSoundEvent::HouseEva { owner, event } => {
                            assert_eq!(*owner, owners[0], "{context}: local-house cue admission");
                            *event
                        }
                        other => panic!("{context}: unexpected primary side effect {other:?}"),
                    })
                    .collect();
                let native_eva: Vec<_> = phase["effects"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|effect| effect["event"] == "eva")
                    .map(|effect| effect["key"].as_str().unwrap())
                    .collect();
                assert_eq!(
                    actual_eva, native_eva,
                    "{context}: original local/foreign EVA"
                );
                for (before, after) in case["initial"]["houses"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .zip(phase["state"]["houses"].as_array().unwrap())
                {
                    assert_eq!(
                        before["timer"], after["timer"],
                        "{context}: observed native House+2A4 timer invariant; no Rust field identity inferred"
                    );
                }
            }
        });
        assert!(draws.is_empty(), "{name}: no source RNG requests");
        assert!(case["rng_requests"].as_array().unwrap().is_empty());
        assert!(case["rng_advances"].as_array().unwrap().is_empty());
        assert_eq!(
            issued.len(),
            case["events"].as_array().unwrap().len(),
            "{name}: actual event history, duplicates excluded"
        );
        for ((owner, id), native) in issued.iter().zip(case["events"].as_array().unwrap()) {
            assert_eq!(
                owners
                    .iter()
                    .position(|candidate| candidate == owner)
                    .unwrap() as u64,
                native["issuer"].as_u64().unwrap(),
                "{name}: original event issuer"
            );
            let relative_uid = ids
                .iter()
                .position(|candidate| candidate == id)
                .map_or(*id, |index| index as u64 + 1);
            assert_eq!(
                relative_uid,
                native["uid"].as_u64().unwrap(),
                "{name}: relative target identity"
            );
        }
    }
}

#[test]
fn primary_event_failed_building_resolution_preserves_a_live_units_plan() {
    let Some((rules, mut sim, _first, _second)) = retail_factories() else {
        return;
    };
    let owner = sim.interner.get("Americans").unwrap();
    let unit = sim
        .spawn_object("MTNK", "Americans", 6, 20, 0, &rules)
        .unwrap();
    let record = sim
        .encode_planning_order_record(
            owner,
            &Command::Move {
                entity_id: unit,
                target_rx: 12,
                target_ry: 20,
            },
        )
        .unwrap();
    let planned = sim.decode_native_command_record(&record, 0).unwrap();
    assert!(sim.apply_command("Americans", &planned.payload, Some(&rules)));
    let paths = sim.planning_paths(owner);
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].actor_id, unit);
    assert_eq!(paths[0].cells, vec![(12, 20)]);
    let before = sim.state_hash();
    // Event4C718F resolves Building first; failed RTTI yields NULL to6386E0,
    // whose6386E8..63882A null arm must not detach this live Foot's plan.
    assert!(!sim.apply_command(
        "Americans",
        &Command::TogglePrimaryFactory { entity_id: unit },
        Some(&rules)
    ));
    assert_eq!(sim.state_hash(), before);
    let retained = sim.planning_paths(owner);
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0].actor_id, unit);
    assert_eq!(retained[0].cells, paths[0].cells);
    assert_eq!(retained[0].loop_start, paths[0].loop_start);
    assert_eq!(retained[0].editing, paths[0].editing);
}
