//! Original107 ->65EC30 -> concrete placement/Teleport/cleanup controls.
//! The native corpus supplies cropped map/Team priors explicitly. These
//! comparisons cover the connected action and shared state, not ALL07 scenes.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::{retail_assets, retail_rules_and_art};
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::components::DriveCoord;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::rng::SimRng;
use crate::sim::team_script_vm::reinforcements_tests::reinforcement_world;
use crate::sim::world::ObjectAiCtx;
use serde_json::Value;

fn number(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn corpus() -> Value {
    let result: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_chrono_reinforcement.json",
    ))
    .unwrap();
    assert_eq!(
        result["native_sha256"],
        "3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600"
    );
    result
}

fn assert_streams(sim: &Simulation, corpus: &Value, expected: &Value) {
    for (name, actual) in [
        ("scenario", sim.scenario_rng.native_state_hex()),
        ("main", sim.main_rng.native_state_hex()),
        ("mapgen", sim.mapgen_rng.native_state_hex()),
    ] {
        let state = &corpus["rng_states"][expected[name].as_str().unwrap()];
        assert_eq!(actual, state.as_str().unwrap(), "{name} native stream");
    }
}

fn assert_member(sim: &Simulation, id: u64, native: &Value) {
    let actor = sim.entities().get(id).unwrap();
    let coord = position_world_coord(&actor.position);
    assert_eq!(
        [coord.x, coord.y, coord.z],
        [
            number(&native["xyz"][0]),
            number(&native["xyz"][1]),
            number(&native["xyz"][2])
        ]
    );
    assert_eq!(actor.ammo().unwrap().current(), number(&native["ammo"]));
    assert_eq!(
        actor.is_warping_in(),
        native["being_warped"].as_bool().unwrap()
    );
    assert_eq!(actor.chrono_warp_delay(), number(&native["delay"]));
    assert_eq!(actor.mission.current().raw(), number(&native["mission"]));
    assert_eq!(actor.mission.queued().raw(), number(&native["queued"]));
    if native.get("target_id").is_some() {
        let expected = native["target_id"].as_i64().unwrap_or(0);
        let target = actor
            .attack_target
            .target()
            .and_then(|target| match target {
                crate::sim::combat::TargetKind::Entity(id) => {
                    sim.entities().get(id).map(|actor| actor.native_unique_id())
                }
                _ => None,
            });
        assert_eq!(target.unwrap_or(0), expected as i32);
    }
    assert_eq!(
        actor.lifecycle.in_limbo(),
        native["limbo"].as_bool().unwrap()
    );
    if let Some(warp) = actor.chrono_warp() {
        assert_eq!(i32::from(warp.state()), number(&native["locomotor_state"]));
        assert_eq!(i32::from(warp.pending_state()), number(&native["pending"]));
        assert_eq!(
            [
                warp.destination().x,
                warp.destination().y,
                warp.destination().z
            ],
            [
                number(&native["warp_destination"][0]),
                number(&native["warp_destination"][1]),
                number(&native["warp_destination"][2])
            ]
        );
        assert_eq!(
            [warp.timer().start_frame(), warp.timer().duration()],
            [
                number(&native["locomotor_timer"][0]),
                number(&native["locomotor_timer"][1])
            ]
        );
    } else {
        // Native retains stale destination/timer bytes at idle state0. They
        // have no consumer until a new warp replaces the complete payload.
        assert_eq!(number(&native["locomotor_state"]), 0);
        assert_eq!(number(&native["pending"]), 0);
        assert!(!native["being_warped"].as_bool().unwrap());
    }
    let loco = actor.locomotor.as_ref().unwrap();
    assert_eq!(
        loco.active_kind(),
        crate::rules::locomotor_type::LocomotorKind::Teleport
    );
    assert_eq!(
        loco.piggyback().is_some(),
        native["locomotor_piggyback"].as_bool().unwrap()
    );
    let marked = loco
        .teleport_runtime()
        .unwrap()
        .resolved_destination()
        .unwrap();
    assert_eq!(
        [marked.x, marked.y, marked.z],
        [
            number(&native["locomotor_marked"][0]),
            number(&native["locomotor_marked"][1]),
            number(&native["locomotor_marked"][2])
        ]
    );
    assert_eq!(
        loco.teleport_runtime().unwrap().is_moving(),
        native["locomotor_moving"].as_bool().unwrap()
    );
}

#[test]
fn chrono_reinforcement_reader_dispatch_placement_and_process_match_original() {
    let corpus = corpus();
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let Some((_, assets)) = retail_assets() else {
        return;
    };
    let map_bytes = assets
        .get_ref(corpus["retail"]["map"].as_str().unwrap())
        .unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(map_bytes),
        corpus["retail"]["map_sha256"].as_str().unwrap()
    );
    let mut layers = crate::rules::native_processing::RulesLayerStack::new(ini.clone());
    if let Some(language) = assets.get_ref("langrule.ini") {
        layers.push(
            crate::rules::native_processing::RulesLayerKind::LangRule,
            IniFile::from_bytes(language).unwrap(),
        );
    }
    let map = IniFile::from_bytes(map_bytes).unwrap();
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        map,
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let mut rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain = TerrainRules::from_ini(&ini);
    let rows = corpus["cases"].as_array().unwrap();
    assert_eq!(rows.len(), 12);
    let definitions: Vec<_> = rows
        .iter()
        .map(|row| {
            let mut input = row["input"].clone();
            input["team_name"] = input["name"].clone();
            input
        })
        .collect();
    let mut initial = definitions[0].clone();
    initial["definitions"] = Value::Array(definitions.clone());
    let mut sim = reinforcement_world(&rules, &terrain, &initial);
    // Preserve the original sparse allocator, including projection lookups
    // that stamp the shared Dummy outside this supplied crop.
    let allocated: Vec<_> = corpus["inherited_crop"]["supplied_cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| {
            (
                number(&cell["coord"][0]) as u16,
                number(&cell["coord"][1]) as u16,
            )
        })
        .collect();
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .test_set_native_allocated_cells(&allocated);
    assert_eq!(
        rules.general.chrono_delay,
        number(&corpus["inherited_crop"]["chrono_delay"])
    );
    assert_eq!(
        rules.general.chrono_reinf_delay,
        number(&corpus["inherited_crop"]["chrono_reinf_delay"])
    );
    // These four already-live actors are supplied original state, not golden
    // constructor output. Preserve real occupancy and the hostile scan target
    // before installing the action's original RNG/identity cursors.
    for prior in rows[0]["prior"].as_array().unwrap() {
        let owner = sim.session.house_order[number(&prior["owner_house_index"]) as usize];
        let owner = sim.interner.resolve(owner).to_string();
        let coord = DriveCoord {
            x: number(&prior["xyz"][0]),
            y: number(&prior["xyz"][1]),
            z: number(&prior["xyz"][2]),
        };
        let id = sim
            .construct_object_limbo_at_height(
                prior["type"].as_str().unwrap(),
                &owner,
                (coord.x / 256) as u16,
                (coord.y / 256) as u16,
                0,
                4,
                &rules,
                FrameEffects::default(),
            )
            .unwrap();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .set_native_unique_id_for_test(number(&prior["native_id"]));
        assert!(
            sim.with_object_placement_scope(|sim| {
                sim.reveal_constructed_object_at_coord_with_overlay_context(
                    id,
                    coord,
                    0,
                    crate::sim::world::PlacementEvidence::UnitEntryAdmitted,
                    &rules,
                    Some(&overlays),
                    FrameEffects::default(),
                )
            })
            .is_some()
        );
    }
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(number(
        &rows[0]["id_before"],
    ) as u32));
    let first_stream = |name: &str| {
        corpus["rng_states"][rows[0]["rng_before"][name].as_str().unwrap()]
            .as_str()
            .unwrap()
    };
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(first_stream("scenario"));
    sim.main_rng = SimRng::from_native_state_hex_for_test(first_stream("main")).into();
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(first_stream("mapgen"));
    for input in &definitions {
        sim.trigger_runtime
            .definitions
            .team_type(input["name"].as_str().unwrap());
    }
    for row in rows {
        let input = &row["input"];
        for prior in row["prior"].as_array().unwrap() {
            let actor = sim
                .entities()
                .values()
                .find(|actor| actor.native_unique_id() == number(&prior["native_id"]))
                .unwrap();
            let coord = position_world_coord(&actor.position);
            assert_eq!(
                [coord.x, coord.y, coord.z],
                [
                    number(&prior["xyz"][0]),
                    number(&prior["xyz"][1]),
                    number(&prior["xyz"][2])
                ]
            );
            assert_eq!(
                sim.interner.resolve(actor.type_ref()),
                prior["type"].as_str().unwrap()
            );
            assert_eq!(
                actor.owner(),
                sim.session.house_order[number(&prior["owner_house_index"]) as usize]
            );
            assert_eq!(
                actor.in_playfield(),
                prior["in_playfield"].as_bool().unwrap()
            );
            assert_eq!(
                actor.lifecycle.in_limbo(),
                prior["limbo"].as_bool().unwrap()
            );
            assert_eq!(
                actor.lifecycle.object_alive(),
                prior["alive"].as_bool().unwrap()
            );
            assert_eq!(
                actor.is_warping_in(),
                prior["being_warped"].as_bool().unwrap()
            );
        }
        assert_streams(&sim, &corpus, &row["rng_before"]);
        assert_eq!(
            sim.native_identity_cursor(),
            Some(number(&row["id_before"]) as u32)
        );
        sim.session.binary_frame = input["frame"].as_u64().unwrap() as u32;
        let mut waypoints = std::collections::HashMap::new();
        for (index, key) in [(4, "origin"), (6, "override_cell")] {
            let point = &input[key];
            waypoints.insert(
                index,
                crate::map::waypoints::Waypoint {
                    index,
                    rx: number(&point[0]) as u16,
                    ry: number(&point[1]) as u16,
                },
            );
        }
        sim.session.install_waypoints(&waypoints);
        let ini = IniFile::from_str(&format!(
            "[Actions]\nCONTROL={}\n",
            row["read"]["read"].as_str().unwrap()
        ));
        let entry = crate::map::actions::parse_actions(&ini)["CONTROL"].entries[0].clone();
        assert_eq!(entry.kind, number(&row["read"]["kind"]));
        assert_eq!(
            entry.waypoint_index.map(|value| value as i32).unwrap_or(-1),
            number(&row["read"]["waypoint"])
        );
        let mut action = sim
            .trigger_runtime
            .definitions
            .resolve_action(entry, None, None);
        if let Some(waypoint) = input["waypoint_override"].as_i64() {
            action.entry.waypoint_index = Some(waypoint as u32);
        }
        let owner = sim.session.current_house();
        let mut effects = Vec::new();
        let returned = sim.apply_live_trigger_action(
            &action,
            TriggerActionContext {
                live_index: None,
                input: TagEventInput::default(),
                incoming_house: owner,
            },
            &mut effects,
            Some(&rules),
            Some(&overlays),
            FrameEffects::default(),
        );
        assert_eq!(returned, number(&row["returned"]) != 0, "{}", input["name"]);
        let expected_ids: Vec<_> = row["created_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(number)
            .collect();
        let ids: Vec<_> = sim
            .entities()
            .values()
            .filter(|actor| expected_ids.contains(&actor.native_unique_id()))
            .map(|actor| actor.stable_id())
            .collect();
        assert_eq!(
            ids.iter()
                .map(|&id| sim.entities().get(id).unwrap().native_unique_id())
                .collect::<Vec<_>>(),
            expected_ids
        );
        assert_eq!(
            sim.native_identity_cursor(),
            Some(number(&row["id_after"]) as u32)
        );
        for (&id, native) in ids.iter().zip(row["members"].as_array().unwrap()) {
            assert_member(&sim, id, native);
        }
        assert_streams(&sim, &corpus, &row["rng_after"]);
        if returned {
            assert_eq!(
                effects,
                vec![TriggerEffect::RecentEventCell {
                    cell: (
                        number(&input["override_cell"][0]) as i16,
                        number(&input["override_cell"][1]) as i16
                    )
                }]
            );
            let team_type = sim.interner.get(input["name"].as_str().unwrap()).unwrap();
            let team = sim
                .team_script_vm
                .teams_in_order()
                .find(|team| team.team_type_id() == Some(team_type))
                .unwrap();
            assert_eq!(
                team.members()
                    .map(|id| sim.entities().get(id).unwrap().native_unique_id())
                    .collect::<Vec<_>>(),
                row["team_members"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(number)
                    .collect::<Vec<_>>()
            );
        } else {
            assert!(effects.is_empty());
        }
        if let Some(target) = input["process_target_id"].as_i64() {
            let target = sim
                .entities()
                .values()
                .find(|actor| i64::from(actor.native_unique_id()) == target)
                .unwrap()
                .stable_id();
            sim.assign_target_represented(
                ids[0],
                Some(crate::sim::combat::TargetKind::Entity(target)),
                Some(&rules),
                Some(&overlays),
                FrameEffects::default(),
            )
            .unwrap();
        }
        for step in row["process"].as_array().into_iter().flatten() {
            let id = ids
                .iter()
                .copied()
                .find(|&id| {
                    sim.entities().get(id).unwrap().native_unique_id()
                        == number(&step["before"]["native_id"])
                })
                .unwrap();
            if let Some(frame) = step["frame"].as_u64() {
                sim.session.binary_frame = frame as u32;
            }
            assert_member(&sim, id, &step["before"]);
            sim.process_chrono_warp(
                id,
                &rules,
                ObjectAiCtx {
                    overlay_registry: Some(&overlays),
                    ..ObjectAiCtx::default()
                },
            )
            .unwrap();
            assert_member(&sim, id, &step["after"]);
            assert_streams(&sim, &corpus, &step["rng"]);
        }
        // Persistence and hash sensitivity are Rust properties, not native CRC.
        for &id in &ids {
            let actor = sim.entities().get(id).unwrap();
            let restored: crate::sim::game_entity::GameEntity =
                bincode::deserialize(&bincode::serialize(actor).unwrap()).unwrap();
            assert_eq!(
                restored.ammo().unwrap().current(),
                actor.ammo().unwrap().current()
            );
            assert_eq!(restored.chrono_warp(), actor.chrono_warp());
        }
        if let Some(&id) = ids.first() {
            let hash = sim.state_hash();
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .ammo_mut()
                .unwrap()
                .add(1);
            assert_ne!(sim.state_hash(), hash);
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .ammo_mut()
                .unwrap()
                .add(-1);
            if sim.entities().get(id).unwrap().chrono_warp().is_some() {
                let warp = sim
                    .substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .warp_teleport_mut()
                    .unwrap()
                    .chrono_mut()
                    .unwrap();
                let pending = warp.pending_state();
                warp.set_pending_state(pending.wrapping_add(1));
                assert_ne!(sim.state_hash(), hash);
                sim.substrate
                    .entities
                    .get_mut(id)
                    .unwrap()
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .warp_teleport_mut()
                    .unwrap()
                    .chrono_mut()
                    .unwrap()
                    .set_pending_state(pending);
            }
        }
    }
}
