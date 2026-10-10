//! G730D60 -> shared ordinary MegaMission / sound-request comparisons.
//! The corpus executes original instructions; these fixtures use the production
//! Rules, spawn, navigation-cell and record owners. Actor construction and the
//! surrounding flat test terrain are supplied inputs, not whole-map parity.
//! Event execution, mission AI and device audio have separate comparisons.

use super::{queue_area_guard_orders, selected_stable_ids_in_order};
use crate::audio::events::GameSoundEvent;
use crate::audio::voice_queue::VoiceQueue;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::area_guard_tests as native;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::retail_ini_fixture::RetailBattleRules;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::command::{Command, CommandEnvelope, CommandRecord, MegaMissionRecord};
use crate::sim::components::DriveCoord;
use crate::sim::house_state::HouseState;
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use serde_json::{Value, json};
use std::collections::BTreeMap;

struct Fixture {
    sim: Simulation,
    rules: RuleSet,
    ids: BTreeMap<String, u64>,
    native_ids: BTreeMap<u64, u64>,
}

fn rng_hex<'a>(row: &'a Value, state: &Value, stream: &str) -> &'a str {
    let reference = state["rng"][stream].as_str().unwrap();
    row["complete_rng_states"][reference]["bytes"]
        .as_str()
        .unwrap()
}

fn assert_rng(sim: &Simulation, row: &Value, state: &Value) {
    for (name, stream) in [
        ("main", &sim.main_rng),
        ("scenario", &sim.scenario_rng),
        ("mapgen", &sim.mapgen_rng),
    ] {
        assert_eq!(
            stream.native_state_hex(),
            rng_hex(row, state, name),
            "{}: {name}",
            row["name"]
        );
    }
}

fn fixture(corpus: &Value, row: &Value, retail: RetailBattleRules) -> Fixture {
    let before = &row["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|boundary| {
            boundary["label"]
                .as_str()
                .unwrap()
                .starts_with("original_Guard_execute")
        })
        .unwrap()["before"];
    let mut rules = retail.rules;
    let mut ini = retail.processed_rules.clone();
    ini.merge(&native::voice_overrides(&row["input"]));
    rules.bind_type_sound_references(&ini, &native::catalog(corpus));
    let registry = OverlayTypeRegistry::from_ini(&ini, Some(&retail.fixed_art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let profile = terrain_rules
        .semantics_by_name("Clear")
        .unwrap()
        .speed_costs;
    let stride = corpus["histories"]["world"]["zone_storage"]["stride"]
        .as_u64()
        .unwrap() as u16;
    let level = corpus["histories"]["world"]["source_recalc"]["after"]["level"]
        .as_u64()
        .unwrap() as u8;
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.map_width = stride;
    sim.session.map_height = stride;
    // Navigation Cell5F6A10 reads the current admitted coordinate here. A
    // flat surrounding arena is sufficient for this ingress-only comparison;
    // no path search, Process, terrain asset loading or held movement runs.
    let cells = (0..stride)
        .flat_map(|y| {
            (0..stride).map(move |x| {
                let mut cell = crate::sim::world::common_raw_test_terrain_cell(x, y, level, false);
                cell.speed_costs = profile;
                cell.base_speed_costs = profile;
                cell
            })
        })
        .collect();
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
        stride, stride, cells,
    ));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    sim.fog.width = stride;
    sim.fog.height = stride;
    let current_house = before["current_house"].as_u64().unwrap();
    let mut owners = BTreeMap::new();
    let mut owner_rows: Vec<_> = before["actors"].as_object().unwrap().values().collect();
    owner_rows.sort_by_key(|actor| {
        (
            actor["owner"].as_u64().unwrap() != current_house,
            actor["owner"].as_u64().unwrap(),
        )
    });
    for actor in owner_rows {
        let pointer = actor["owner"].as_u64().unwrap();
        if owners.contains_key(&pointer) {
            continue;
        }
        let name = format!("NativeHouse{pointer}");
        let owner = sim.interner.intern(&name);
        let mut house = HouseState::new(
            owner,
            u8::try_from(owners.len()).unwrap(),
            None,
            actor["human"].as_u64().unwrap() != 0,
            10_000,
            10,
        );
        house.player_control = actor["player_control"].as_u64().unwrap() != 0;
        sim.houses.insert(owner, house);
        sim.session.house_order.push(owner);
        if pointer == current_house {
            sim.session.current_house = Some(owner);
        }
        owners.insert(pointer, name);
    }
    sim.session.game_mode_nonzero = before["game_mode"].as_u64().unwrap() != 0;
    sim.session.binary_frame = before["frame"].as_u64().unwrap() as u32;
    sim.session.tick = before["frame"].as_u64().unwrap();
    let mut ids = BTreeMap::new();
    let mut native_ids = BTreeMap::new();
    let mut actors: Vec<_> = before["actors"].as_object().unwrap().iter().collect();
    actors.sort_by_key(|(_, actor)| actor["id"].as_u64().unwrap());
    for (name, actor) in actors {
        assert_eq!(
            actor["offline"], 0,
            "unrepresented RobotOffline fixture is excluded explicitly"
        );
        assert_eq!(actor["limbo"], 0);
        assert_eq!(actor["alive"], 1);
        assert_eq!(actor["loco_head"], json!([0, 0, 0]), "standing Foot input");
        let xyz = &actor["position"];
        let coord = DriveCoord {
            x: xyz[0].as_i64().unwrap() as i32,
            y: xyz[1].as_i64().unwrap() as i32,
            z: xyz[2].as_i64().unwrap() as i32,
        };
        let type_name = actor["type_name"].as_str().unwrap();
        assert_eq!(
            json!(rules.object(type_name).unwrap().voice_special_attack),
            actor["voice"]["names"]
        );
        let owner = &owners[&actor["owner"].as_u64().unwrap()];
        let id = sim
            .spawn_object_at_height_with_overlay_registry(
                type_name,
                owner,
                (coord.x / 256) as u16,
                (coord.y / 256) as u16,
                64,
                level,
                &rules,
                &registry,
            )
            .expect("ordinary retail receiver enters supplied clear ground");
        sim.foot_set_location_marked(id, coord, Some(&rules), Some(&registry));
        ids.insert(name.clone(), id);
        native_ids.insert(actor["id"].as_u64().unwrap(), id);
    }
    // Construction is outside the comparison. Continue all three actual
    // original process streams from the observed input boundary, without
    // reproducing their seed/draw arithmetic in this test.
    sim.main_rng = SimRng::from_native_state_hex_for_test(rng_hex(row, before, "main"));
    sim.scenario_rng = SimRng::from_native_state_hex_for_test(rng_hex(row, before, "scenario"));
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(rng_hex(row, before, "mapgen"));
    Fixture {
        sim,
        rules,
        ids,
        native_ids,
    }
}

fn native_record(hex: &str) -> MegaMissionRecord {
    let bytes: Vec<_> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    MegaMissionRecord::decode(&CommandRecord::decode_exact(&bytes).unwrap()).unwrap()
}

fn boundary_receipts<'a>(row: &'a Value, boundary: &Value) -> impl Iterator<Item = &'a Value> {
    let first = boundary["first_sequence"].as_u64().unwrap();
    let last = boundary["last_sequence"].as_u64().unwrap();
    row["receipts"]
        .as_array()
        .unwrap()
        .iter()
        .filter(move |receipt| {
            let sequence = receipt["sequence"].as_u64().unwrap();
            (first..=last).contains(&sequence)
        })
}

fn sound_name<'a>(corpus: &'a Value, id: &Value) -> Option<&'a str> {
    corpus["histories"]["sound_registry"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sound| sound["fixture_index"] == *id)
        .map(|sound| sound["name"].as_str().unwrap())
}

#[test]
fn area_guard_ingress_matches_native_order_current_cells_voices_and_rng() {
    let corpus = native::native();
    let mut compared = 0;
    let mut visits = 0;
    let mut offline_excluded = 0;
    for row in corpus["histories"]["rows"].as_array().unwrap() {
        if !row["input"]["direct"].is_null() {
            continue;
        }
        if row["input"]["offline"] == true {
            assert_eq!(row["name"], "offline_selection");
            offline_excluded += 1;
            continue;
        }
        // Every independent native history begins with a fresh process-owned
        // Rules result; an authored voice override cannot leak into the next.
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let mut f = fixture(&corpus, row, retail);
        let selected: Vec<_> = row["input"]["selection"]
            .as_array()
            .unwrap()
            .iter()
            .map(|name| {
                // NULL and CellClass slots are supplied non-Techno selection
                // holes. Neither has a live Techno stable ID in the Rust store.
                if name.is_null() {
                    0
                } else {
                    f.ids
                        .get(name.as_str().unwrap())
                        .copied()
                        .unwrap_or(u64::MAX)
                }
            })
            .collect();
        let mut voices = VoiceQueue::new();
        for boundary in row["boundaries"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|boundary| {
                boundary["label"]
                    .as_str()
                    .unwrap()
                    .starts_with("original_Guard_execute")
            })
        {
            assert_rng(&f.sim, row, &boundary["before"]);
            let prior_count = f.sim.pending_command_snapshot().len();
            let events = queue_area_guard_orders(
                &mut f.sim,
                &f.rules,
                &selected,
                boundary["before"]["voice_enabled"] != 0,
            );
            let actual_sounds: Vec<_> = events
                .iter()
                .map(|event| match event {
                    GameSoundEvent::UnitMoveOrder {
                        speaker_id,
                        sound_id,
                    } => {
                        voices.queue(*speaker_id, sound_id);
                        let name = f.ids.iter().find(|(_, id)| **id == *speaker_id).unwrap().0;
                        json!({"kind":"QueueVoice", "actor":name, "sound":sound_id})
                    }
                    GameSoundEvent::UiSound { sound_id } => {
                        json!({"kind":"GuardSound", "actor":null, "sound":sound_id})
                    }
                    other => panic!("unexpected Guard event {other:?}"),
                })
                .collect();
            let expected_sounds: Vec<_> = boundary_receipts(row, boundary)
                .filter(|receipt| matches!(receipt["kind"].as_str(), Some("QueueVoice" | "GuardSound")) && !receipt["sound_name"].is_null())
                .map(|receipt| json!({"kind":receipt["kind"], "actor":receipt["actor"], "sound":receipt["sound_name"]})).collect();
            assert_eq!(
                actual_sounds, expected_sounds,
                "{}: request order",
                row["name"]
            );
            assert_rng(&f.sim, row, &boundary["after"]);
            for (name, id) in &f.ids {
                assert_eq!(
                    voices.pending_for(*id),
                    sound_name(&corpus, &boundary["after"]["actors"][name]["queued_voice"]),
                    "{}: {name} pending voice",
                    row["name"]
                );
            }
            let pending = f.sim.pending_command_snapshot();
            let actual: Vec<_> = pending[prior_count..]
                .iter()
                .map(|envelope| {
                    assert_eq!(envelope.owner, f.sim.session.current_house.unwrap());
                    assert_eq!(envelope.execute_tick, f.sim.session.tick);
                    MegaMissionRecord::decode(
                        &f.sim
                            .encode_megamission_record(envelope.owner, &envelope.payload)
                            .unwrap(),
                    )
                    .unwrap()
                })
                .collect();
            let expected: Vec<_> = boundary_receipts(row, boundary)
                .filter_map(|receipt| receipt["event_bytes"].as_str())
                .map(|bytes| {
                    let mut record = native_record(bytes);
                    record.source_id = f.native_ids[&(record.source_id as u64)] as i32;
                    record
                })
                .collect();
            // The full-OutList contrast compares original constructor calls,
            // requests and RNG only. VERA's app ingress has no bounded native
            // OutList admission; this test does not certify its overflow path.
            assert_eq!(actual, expected, "{}: ordered event contents", row["name"]);
            visits += 1;
        }
        compared += 1;
    }
    assert_eq!((compared, visits), (12, 15));
    assert_eq!(
        offline_excluded, 1,
        "RobotOffline lifecycle remains separate"
    );
}

#[test]
fn area_guard_uses_pending_selection_array_before_simulation_membership_commits() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let corpus = native::native();
    let row = corpus["histories"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "stock_ordered_selection")
        .unwrap();
    let mut f = fixture(&corpus, row, retail);
    let pending_order: Vec<_> = row["input"]["selection"]
        .as_array()
        .unwrap()
        .iter()
        .map(|name| f.ids[name.as_str().unwrap()])
        .collect();
    let mut sorted = pending_order.clone();
    sorted.sort_unstable();
    assert_ne!(
        pending_order, sorted,
        "the native witness distinguishes selection and storage order"
    );
    assert!(f.sim.entities().values().all(|actor| !actor.selected));
    let selected = selected_stable_ids_in_order(Some(&f.sim), Some(&f.rules), &pending_order, true);
    assert_eq!(selected, pending_order);
    let owner = f.sim.session.current_house.unwrap();
    f.sim.queue_command(CommandEnvelope::new(
        owner,
        f.sim.session.tick,
        Command::Select {
            entity_ids: selected.clone(),
            additive: false,
        },
    ));
    queue_area_guard_orders(&mut f.sim, &f.rules, &selected, true);
    let pending = f.sim.pending_command_snapshot();
    assert!(matches!(pending[0].payload, Command::Select { .. }));
    let guard_ids: Vec<_> = pending[1..]
        .iter()
        .map(|envelope| match envelope.payload {
            Command::Guard { entity_id, .. } => entity_id,
            ref other => panic!("unexpected payload {other:?}"),
        })
        .collect();
    let native_ids: Vec<_> = row["receipts"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|receipt| receipt["event_bytes"].as_str())
        .map(|bytes| f.native_ids[&(native_record(bytes).source_id as u64)])
        .collect();
    assert_eq!(guard_ids, native_ids);
    assert!(
        f.sim.entities().values().all(|actor| !actor.selected),
        "issuing Guard never commits selection"
    );
}
