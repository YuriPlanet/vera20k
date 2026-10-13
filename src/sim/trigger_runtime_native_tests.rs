//! Executed original histories, not a second trigger interpreter. Inputs set
//! up the supplied native storage; every operation calls the production owner.

use super::*;
use crate::sim::world::{DeferredDeleteQueue, DeferredDeleteTicket};
use serde_json::{Value, json};
use std::collections::HashMap;

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn native_rng_hash(hex: &str) -> String {
    let bytes: Vec<_> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    crate::util::sha256::sha256_hex(&bytes)
}

#[test]
fn native_damage_at_waypoint_dispatch_matches_original_controls() {
    use crate::map::waypoints::Waypoint;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_damage_at_waypoint.json",
    ))
    .unwrap();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n[Super]\nCellSpread=0\n",
    ))
    .unwrap();
    let controls = corpus["dispatch_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 6);
    for row in controls {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let x = integer(&input["cell"][0]) as u16;
        let y = integer(&input["cell"][1]) as u16;
        let level = input["level"].as_i64().unwrap_or(0) as u8;
        let slope = input["slope"].as_i64().unwrap_or(0) as u8;
        let flags = input["flags"].as_u64().unwrap_or(0) as u32;
        let mut terrain = crate::map::resolved_terrain::test_grid(32, 32, |rx, ry| {
            let mut cell = crate::map::resolved_terrain::test_clear_cell(rx, ry);
            if (rx, ry) == (x, y) {
                cell.level = level;
                cell.slope_type = slope;
                cell.bridge_facts.raw_flags = flags;
            }
            cell
        });
        let identity = terrain.native_cell_identity((x as i16, y as i16));
        terrain.write_native_cell_flags(identity, flags);
        let mut sim = Simulation::with_seed(31);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        sim.install_resolved_terrain_for_new_map(terrain);
        let index = integer(&row["read"]["value"]) as u32;
        sim.session.install_waypoints(&HashMap::from([
            (
                index,
                Waypoint {
                    index,
                    rx: x,
                    ry: y,
                },
            ),
            (
                0,
                Waypoint {
                    index: 0,
                    rx: 20,
                    ry: 20,
                },
            ),
        ]));
        let mut definitions = RuntimeDefinitions::default();
        let typ = definitions.trigger_type("DAMAGE").unwrap();
        let ini = IniFile::from_str(&format!(
            "[Actions]\nDAMAGE=1,{}\n",
            row["read"]["raw"].as_str().unwrap()
        ));
        let entry = crate::map::actions::parse_actions(&ini)
            .remove("DAMAGE")
            .unwrap()
            .entries
            .remove(0);
        let action = definitions.resolve_action(entry, None, None);
        assert_eq!(json!(action.value), row["read"]["value"], "{name}: +90");
        sim.trigger_runtime.definitions = definitions;
        let before = (
            sim.main_rng.native_state_hex(),
            sim.scenario_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        let (returned, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
            sim.apply_live_trigger_action(
                &action,
                typ,
                &mut Vec::new(),
                Some(&rules),
                None,
                FrameEffects::default(),
            )
        });
        assert_eq!(
            u8::from(returned),
            row["returned"].as_u64().unwrap() as u8,
            "{name}"
        );
        let native: Vec<_> = row["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["kind"] == "area")
            .collect();
        assert_eq!(visits.len(), native.len(), "{name}");
        for (actual, expected) in visits.iter().zip(native) {
            assert_eq!(actual["coord"], expected["coord"], "{name}");
            assert_eq!(actual["damage"], expected["damage"], "{name}");
            assert_eq!(actual["warhead"], "Super", "{name}");
            assert_eq!(actual["no_source"], true, "{name}");
            assert_eq!(actual["no_source_house"], true, "{name}");
        }
        // Native downstream damage/bridge answers are explicit seams in this
        // dispatch family. Real NANRCT receivers are compared in the next test.
        assert_eq!(row["rng_before"], row["rng_after"], "{name}");
        assert_eq!(
            before,
            (
                sim.main_rng.native_state_hex(),
                sim.scenario_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ),
            "{name}: empty world RNG"
        );
    }
}

#[test]
fn native_all01_reactor_damage_uses_retail_rules_and_shared_power_owner() {
    use crate::map::entities::EntityCategory;
    use crate::map::waypoints::Waypoint;
    use crate::rules::{
        ini_parser::IniFile,
        native_processing::{RulesLayerKind, RulesLayerStack},
        ruleset::RuleSet,
    };
    use crate::sim::{
        building_construction::BuildingBodyMode, components::Health, game_entity::GameEntity,
        house_state::HouseState, native_identity::NativeUniqueIdCursor, rng::SimRng,
    };
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_damage_at_waypoint.json",
    ))
    .unwrap();
    let Some(raw_rules) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini") else {
        return;
    };
    let Some(raw_art) = crate::rules::retail_ini_fixture::retail_ini_bytes("artmd.ini") else {
        return;
    };
    let Some(raw_map) = crate::rules::retail_ini_fixture::retail_archive_bytes("ALL01UMD.MAP")
    else {
        return;
    };
    let map = crate::map::map_file::MapFile::from_bytes(&raw_map).unwrap();
    for (name, raw) in [("RULESMD.INI", &raw_rules), ("ALL01UMD.MAP", &raw_map)] {
        let native = corpus["retail_inputs"]["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["file"] == name)
            .unwrap();
        assert_eq!(
            crate::util::sha256::sha256_hex(raw),
            native["sha256"],
            "{name}"
        );
    }
    assert_eq!(
        crate::util::sha256::sha256_hex(&raw_art),
        corpus["retail_inputs"]["artmd"]["sha256"]
    );
    let mut layers = RulesLayerStack::new(IniFile::from_bytes(&raw_rules).unwrap());
    let lang = std::path::PathBuf::from(std::env::var("RA2_DIR").unwrap()).join("LANGRULE.INI");
    if lang.is_file() {
        layers.push(
            RulesLayerKind::LangRule,
            IniFile::from_bytes(&std::fs::read(lang).unwrap()).unwrap(),
        );
    } else {
        assert!(
            corpus["retail_inputs"]["layers"]
                .as_array()
                .unwrap()
                .iter()
                .any(|row| row["file"] == "LANGRULE.INI" && row["absent"] == true)
        );
    }
    layers.push(RulesLayerKind::Scenario, map.ini.clone());
    let processed = layers
        .process_with_fixed_art(&IniFile::from_bytes(&raw_art).unwrap())
        .unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let object = rules.object("NANRCT").unwrap();
    let dimensions = crate::rules::foundation::foundation_dimensions(&object.foundation);
    assert_eq!(
        json!([dimensions.0, dimensions.1]),
        corpus["retail_inputs"]["foundation_dimensions"]
    );
    let authored = map
        .entities
        .iter()
        .find(|entity| entity.type_id == "NANRCT" && entity.owner == "Confederation")
        .unwrap();
    let native = corpus["receiver_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["route"] == "area")
        .unwrap();
    assert_eq!(
        json!(authored.health),
        native["authored_health_native"]["input"]["authored"]
    );
    assert_eq!(
        json!(object.strength),
        native["authored_health_native"]["input"]["strength"]
    );
    let initial = integer(&native["authored_health_native"]["output"]["actual"]);

    // Match the native control's translated, already-admitted single reactor
    // at10,10, null Tag and empty animation slots. This is a receiver/power
    // comparison, not full campaign admission, difficulty or rendered parity.
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
        32,
        32,
        crate::map::resolved_terrain::test_clear_cell,
    ));
    sim.session.binary_frame = integer(&native["frame"]) as u32;
    let owner = sim.interner.intern(&authored.owner);
    sim.houses
        .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
    let type_id = sim.interner.intern(&authored.type_id);
    assert_eq!(sim.allocate_stable_id(), 1);
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        10,
        0,
        0,
        owner,
        Health { current: initial },
        type_id,
        EntityCategory::Structure,
        0,
        0,
        false,
    );
    entity.native_unique_id = integer(&native["reactor_native_unique_id"]);
    entity.foundation = object.foundation.clone();
    entity.begin_building_body(BuildingBodyMode::Idle, 0);
    sim.substrate.entities.insert(entity);
    sim.add_entity_occupancy(1);
    sim.sample_building_health_for_house_update(1);
    crate::sim::power_system::assess_house_power(
        sim.power_states.get_mut(&owner).unwrap(),
        &sim.substrate.entities,
        &rules,
        owner,
        &sim.interner,
        sim.session.binary_frame,
    );
    assert_eq!(
        json!(sim.power_states[&owner].total_output),
        native["power_consumer"]["initial_power"]
    );
    sim.session.install_waypoints(&HashMap::from([(
        364,
        Waypoint {
            index: 364,
            rx: 10,
            ry: 10,
        },
    )]));
    let mut definitions = RuntimeDefinitions::default();
    let typ = definitions.trigger_type("DAMAGE").unwrap();
    let entry = map.actions["087857CC"]
        .entries
        .iter()
        .find(|entry| entry.kind == ACTION_DAMAGE_AT_WAYPOINT)
        .unwrap()
        .clone();
    let action = definitions.resolve_action(entry, None, None);
    sim.trigger_runtime.definitions = definitions;
    // Import the original post-constructor boundary. Damage crossing yellow
    // must retain the shared smoke owner's one Scenario selection draw.
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(integer(
        &native["reactor_native_unique_id"],
    ) as u32));
    let rng_before = &native["rng_state_before"];
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(rng_before["main"].as_str().unwrap()).into();
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(rng_before["scenario"].as_str().unwrap());
    sim.mapgen_rng = SimRng::from_native_state_hex_for_test(rng_before["mapgen"].as_str().unwrap());
    let (returned, visits) = crate::sim::combat::receiver_fixture::trace_area_visits(|| {
        sim.apply_live_trigger_action(
            &action,
            typ,
            &mut Vec::new(),
            Some(&rules),
            None,
            FrameEffects::default(),
        )
    });
    assert!(returned);
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(visits.len(), rows.len());
    for (index, visit) in visits.iter().enumerate() {
        let prior = if index == 0 {
            initial
        } else {
            integer(&rows[index - 1]["health"])
        };
        assert_eq!(
            visit["health_before"],
            json!([[1, prior]]),
            "native receiver visit{}",
            index + 1
        );
    }
    let last = rows.last().unwrap();
    let reactor = sim.substrate.entities.get(1).unwrap();
    assert_eq!(json!(reactor.health.current), last["health"]);
    assert_eq!(json!(reactor.estimated_health.get()), last["estimated"]);
    assert_eq!(
        u8::from(reactor.building_damage_state_active),
        last["damaged_mode"].as_u64().unwrap() as u8
    );
    assert_eq!(
        json!(reactor.building_power_health_sample()),
        native["power_consumer"]["before_sample"]["sampled_health"]
    );
    assert_eq!(
        json!(sim.power_states[&owner].total_output),
        native["power_consumer"]["before_sample"]["house_power"]
    );
    sim.sample_building_health_for_house_update(1);
    assert_eq!(
        json!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .building_power_health_sample()
        ),
        native["power_consumer"]["after_sample"]["sampled_health"]
    );
    assert!(sim.power_states[&owner].power_recheck_for_test());
    crate::sim::power_system::assess_house_power(
        sim.power_states.get_mut(&owner).unwrap(),
        &sim.substrate.entities,
        &rules,
        owner,
        &sim.interner,
        sim.session.binary_frame,
    );
    assert_eq!(
        json!(sim.power_states[&owner].total_output),
        native["power_consumer"]["after_power"]["house_power"]
    );
    assert!(!sim.power_states[&owner].power_recheck_for_test());
    let smoke = &last["damage_smoke"];
    assert_eq!(
        json!(sim.particle_systems().len()),
        smoke["registered_count"]
    );
    let (id, system) = sim.particle_systems().iter().next().unwrap();
    assert_eq!(
        system.type_id,
        rules
            .ps_type_id_by_name(smoke["type_name"].as_str().unwrap())
            .unwrap()
    );
    assert_eq!(json!(system.coords.to_array()), smoke["coordinate"]);
    assert_eq!(json!(system.native_unique_id()), smoke["native_unique_id"]);
    assert_eq!(system.owner_entity, Some(1));
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .damage_smoke_system_id,
        Some(*id)
    );
    assert!(system.particles.is_empty(), "no particle AI visit yet");
    for (stream, actual) in [
        ("main", sim.main_rng.native_state_hex()),
        ("scenario", sim.scenario_rng.native_state_hex()),
        ("mapgen", sim.mapgen_rng.native_state_hex()),
    ] {
        assert_eq!(
            actual,
            native["rng_state_after"][stream].as_str().unwrap(),
            "{stream}"
        );
    }
}

/// Original Execute17→6E0FE0→5678E0→653830→4A9CA0 with the native
/// constructor's empty-resident, FogOfWar-clear cell priors. The production
/// action uses the retained+90 waypoint, current PlayerPtr and Fog+240 owner;
/// geometry, both counter passes, dummy writes and all three RNG states are
/// compared with original executable observations. Resident discovery and
/// GPU output are separately integrated/validated, not certified by this row.
#[test]
fn native_trigger_reveal_action_matches_original_controls() {
    use crate::map::cell_index::NativeCellIdentity;
    use crate::map::resolved_terrain::{NativeCellQuery, test_clear_cell, test_grid};
    use crate::map::waypoints::Waypoint;
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::rng::SimRng;
    use crate::sim::vision::{FogState, OwnerVisibility};
    use std::collections::BTreeSet;

    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    let native = corpus
        .iter()
        .find(|row| row["case_kind"] == "trigger_reveal")
        .unwrap();
    let controls = native["controls"].as_array().unwrap();
    assert_eq!(controls.len(), 16);
    for row in controls {
        let name = row["name"].as_str().unwrap();
        let input = &row["input"];
        let source = (
            integer(&input["cell"][0]) as i16,
            integer(&input["cell"][1]) as i16,
        );
        let level = input["level"].as_i64().unwrap_or(0) as i8;
        let radius = input["radius"].as_i64().unwrap_or(9) as i32;
        let rules = RuleSet::from_ini(&IniFile::from_str(&format!(
            "[General]\nRevealTriggerRadius={radius}\nRevealByHeight=yes\n"
        )))
        .unwrap();
        let mut terrain = test_grid(128, 128, test_clear_cell);
        let identity = if input["missing"] == true {
            NativeCellIdentity::Dummy
        } else {
            terrain.native_cell_identity(source)
        };
        terrain.write_native_cell_level(identity, level as u8);
        terrain.write_native_cell_flags(identity, input["flags"].as_u64().unwrap_or(0) as u32);
        let mut sim = Simulation::with_seed(31);
        let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&terrain);
        sim.install_resolved_terrain_for_new_map(terrain);
        sim.install_fixture_path_grid(Some(&path));
        sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width: integer(&row["size"][0]) as u32,
            height: integer(&row["size"][1]) as u32,
            local_left: 0,
            local_top: 0,
            local_width: integer(&row["size"][0]) as u32,
            local_height: integer(&row["size"][1]) as u32,
        });
        let player = sim.interner.intern("Player");
        let other = sim.interner.intern("Other");
        sim.session.current_house = Some(player);
        sim.fog.width = 128;
        sim.fog.height = 128;
        for (owner, latch) in [(player, &input["latch"]), (other, &input["other_latch"])] {
            if latch.as_u64().unwrap_or(0) != 0 {
                sim.fog.reveal_all_for_owner(owner);
            }
            // Native fixture supplies House240 independently of the Cell
            // constructor's untouched planes. Seed them through their owner.
            sim.fog
                .by_owner
                .insert(owner, OwnerVisibility::new(128, 128));
        }
        let mut definitions = RuntimeDefinitions::default();
        let typ = definitions.trigger_type("REVEAL").unwrap();
        definitions.trigger_types[typ].owner = Some(
            if input["trigger_house"] == "player" {
                "Player"
            } else {
                "Other"
            }
            .into(),
        );
        let ini = IniFile::from_str(&format!(
            "[Actions]\nREVEAL={}\n",
            row["read"]["read"].as_str().unwrap()
        ));
        let entry = crate::map::actions::parse_actions(&ini)
            .remove("REVEAL")
            .unwrap()
            .entries
            .remove(0);
        let action = definitions.resolve_action(entry, None, None);
        assert_eq!(
            json!(action.value),
            row["read"]["value"],
            "{name}: retained+90"
        );
        assert_eq!(
            json!(action.entry.waypoint_index),
            row["read"]["waypoint"],
            "{name}: independent+44"
        );
        let mut waypoints = HashMap::new();
        let literal = integer(&row["read"]["value"]) as u32;
        if let Some(tail) = action.entry.waypoint_index {
            waypoints.insert(
                tail,
                Waypoint {
                    index: tail,
                    rx: 48,
                    ry: 76,
                },
            );
        }
        waypoints.insert(
            literal,
            Waypoint {
                index: literal,
                rx: source.0 as u16,
                ry: source.1 as u16,
            },
        );
        sim.session.install_waypoints(&waypoints);
        sim.trigger_runtime.definitions = definitions;
        sim.scenario_rng = SimRng::new(31);
        sim.main_rng = SimRng::new(31).into();
        sim.mapgen_rng = SimRng::new(31);
        let hashes = |sim: &Simulation| {
            json!({
                "scenario": native_rng_hash(&sim.scenario_rng.native_state_hex()),
                "main": native_rng_hash(&sim.main_rng.native_state_hex()),
                "mapgen": native_rng_hash(&sim.mapgen_rng.native_state_hex()),
            })
        };
        assert_eq!(
            hashes(&sim),
            row["rng_before"],
            "{name}: original seeded priors"
        );
        let frame = sim.session.binary_frame;
        let mut effects = Vec::new();
        let (returned, draws) = crate::sim::rng::trace_draws(|| {
            sim.apply_live_trigger_action(
                &action,
                typ,
                &mut effects,
                Some(&rules),
                None,
                FrameEffects::default(),
            )
        });
        assert_eq!(
            i32::from(returned),
            integer(&row["returned_al"]),
            "{name}: native AL"
        );
        assert!(
            draws.is_empty() && effects.is_empty(),
            "{name}: no RNG/presentation/timer events"
        );
        assert_eq!(
            hashes(&sim),
            row["rng_after"],
            "{name}: all retained RNG words"
        );
        assert_eq!(
            sim.session.binary_frame, frame,
            "{name}: no frame advancement"
        );
        assert_eq!(
            sim.fog.whole_map_revealed_for_owner(player),
            row["latch_after"].as_u64().unwrap() != 0
        );
        let expected: BTreeSet<_> = row["final_cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| {
                (
                    integer(&cell["cell"][0]) as u16,
                    integer(&cell["cell"][1]) as u16,
                )
            })
            .collect();
        let revealed = |fog: &FogState| {
            (0..128u16)
                .flat_map(|y| (0..128u16).map(move |x| (x, y)))
                .filter(|&(x, y)| fog.is_ground_unshrouded(player, x, y))
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(
            revealed(&sim.fog),
            expected,
            "{name}: native whole footprint, no extra cells"
        );
        assert!(
            (0..128u16).all(|y| (0..128u16).all(|x| !sim.fog.is_ground_unshrouded(other, x, y))),
            "{name}: trigger House is not PlayerPtr"
        );
        let compare = |fog: &FogState, cells: &Value| {
            for cell in cells.as_array().unwrap() {
                let x = integer(&cell["cell"][0]) as u16;
                let y = integer(&cell["cell"][1]) as u16;
                let vis = &fog.by_owner[&player];
                let state = vis.shroud_knowledge_raw()[usize::from(y) * 128 + usize::from(x)];
                assert_eq!(
                    state.counter,
                    integer(&cell["counter"]),
                    "{name}: ({x},{y}) counter"
                );
                assert_eq!(
                    state.gap_counter,
                    integer(&cell["gap"]),
                    "{name}: ({x},{y}) gap"
                );
                assert_eq!(
                    state.pending,
                    cell["flags"].as_u64().unwrap() & 0x20 != 0,
                    "{name}: pending"
                );
                assert_eq!(
                    fog.is_ground_unshrouded(player, x, y),
                    cell["alt"].as_u64().unwrap() & 8 != 0
                );
                assert_eq!(
                    fog.is_ground_open(player, x, y),
                    cell["alt"].as_u64().unwrap() & 16 != 0
                );
                assert_eq!(
                    state.local_sources + state.allied_sources,
                    0,
                    "{name}: no permanent source"
                );
            }
        };
        compare(&sim.fog, &row["final_cells"]);
        let config = sim.sight_reveal_config(Some(&rules));
        let heights = sim.path_grid().unwrap().ground_height_grid();
        let mut probe = Simulation::with_seed(31);
        probe.fog = FogState::new(128, 128);
        probe.session.current_house = Some(player);
        probe.install_resolved_terrain_for_new_map(sim.resolved_terrain.as_ref().unwrap().clone());
        probe
            .fog
            .by_owner
            .insert(player, OwnerVisibility::new(128, 128));
        for pass in row["passes"].as_array().unwrap() {
            let coord = &pass["coord"];
            assert_eq!(pass["house"], "player");
            assert_eq!(pass["by_height"], 1);
            assert_eq!(
                [&pass["outline"], &pass["unreveal"], &pass["arg6"]],
                [&json!(0), &json!(0), &json!(0)]
            );
            probe.reveal_area(
                &[player],
                (
                    (integer(&coord[0]) / 256) as u16,
                    (integer(&coord[1]) / 256) as u16,
                ),
                integer(&coord[2]),
                radius.clamp(0, i32::from(u16::MAX)) as u16,
                Some(&heights),
                integer(&pass["final"]) != 0,
                &config,
                crate::sim::world::UninitContext::with_rules(&rules),
            );
            compare(&probe.fog, &pass["states"]);
        }
        if input["missing"] == true {
            if let Some(last) = row["calls"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                .find(|call| call["call"] == "packed_lookup")
            {
                let dummy = sim.effective_shared_cell_dummy().snapshot();
                assert_eq!(
                    json!([dummy.coord.0, dummy.coord.1]),
                    last["cell"],
                    "{name}: shared Dummy ordering"
                );
                assert_eq!(dummy.level, level);
            }
        }
        for vis in sim.fog.by_owner.values_mut() {
            vis.clear_all_visible();
        }
        assert_eq!(
            revealed(&sim.fog),
            expected,
            "{name}: knowledge survives sight cleanup"
        );
        let restored: FogState =
            bincode::deserialize(&bincode::serialize(&sim.fog).unwrap()).unwrap();
        assert_eq!(
            revealed(&restored),
            expected,
            "{name}: knowledge survives save/load"
        );
        compare(&restored, &row["final_cells"]);
        let query = NativeCellQuery::isolated(sim.resolved_terrain.as_ref().unwrap());
        for cell in row["final_cells"].as_array().unwrap() {
            assert_eq!(
                crate::sim::vision::cell_is_shrouded(
                    &restored,
                    &query,
                    player,
                    (
                        integer(&cell["cell"][0]) as u16,
                        integer(&cell["cell"][1]) as u16
                    )
                ),
                cell["shrouded_query_al"] == true,
                "{name}: original IsShrouded consumer"
            );
        }
    }
}

fn rows() -> Vec<Value> {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/tag_lifecycle.json",
    ))
    .unwrap()
}

#[test]
fn native_trigger_reveal_retail_definitions_use_retained_radius_and_literal_waypoints() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let Some(raw_rules) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini") else {
        return;
    };
    let Some(raw_art) = crate::rules::retail_ini_fixture::retail_ini_bytes("artmd.ini") else {
        return;
    };
    let Some(raw_map) = crate::rules::retail_ini_fixture::retail_archive_bytes("SOV01UMD.MAP")
    else {
        return;
    };
    let map = crate::map::map_file::MapFile::from_bytes(&raw_map).unwrap();
    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    let native = corpus
        .iter()
        .find(|row| row["case_kind"] == "trigger_reveal")
        .unwrap();
    let definitions = &native["retail_definitions"];
    for (name, raw) in [("rulesmd.ini", &raw_rules), ("sov01umd.map", &raw_map)] {
        assert_eq!(
            crate::util::sha256::sha256_hex(raw),
            definitions["sources"][name]["sha256"]
        );
        assert_eq!(json!(raw.len()), definitions["sources"][name]["bytes"]);
    }
    let mut layers = RulesLayerStack::new(IniFile::from_bytes(&raw_rules).unwrap());
    let art = IniFile::from_bytes(&raw_art).unwrap();
    let rule_radius = |layers: &RulesLayerStack| {
        crate::rules::ruleset::RuleSet::from_processed_rules(
            &layers.process_with_fixed_art(&art).unwrap(),
        )
        .unwrap()
        .reveal_trigger_radius()
    };
    assert_eq!(
        json!(rule_radius(&layers)),
        native["radius_reader"]["rows"][0]["value"]
    );
    layers.push(RulesLayerKind::Scenario, map.ini.clone());
    assert_eq!(
        json!(rule_radius(&layers)),
        native["radius_reader"]["rows"][1]["value"]
    );
    let triggers = definitions["triggers"].as_array().unwrap();
    assert_eq!(triggers.len(), 4);
    let mut count = 0;
    for trigger in triggers {
        let id = trigger["id"].as_str().unwrap();
        let authored = &map.actions[id];
        assert_eq!(
            authored.entries.len(),
            trigger["actions"].as_array().unwrap().len(),
            "{id}"
        );
        for (entry, expected) in authored
            .entries
            .iter()
            .zip(trigger["actions"].as_array().unwrap())
        {
            assert_eq!(json!(entry.kind), expected["kind"], "{id}");
            if entry.kind == 17 {
                assert_eq!(json!(entry.literal_value()), expected["value"], "{id}: +90");
                assert_eq!(
                    json!(entry.waypoint_index),
                    expected["waypoint_44"],
                    "{id}: +44"
                );
                count += 1;
            }
        }
    }
    assert_eq!(json!(count), definitions["action17_count"]);
    assert_eq!(
        json!(
            map.actions
                .values()
                .flat_map(|actions| &actions.entries)
                .filter(|entry| entry.kind == 16)
                .count()
        ),
        definitions["action16_count_all_definitions"]
    );
    for (index, expected) in definitions["waypoints"].as_object().unwrap() {
        let waypoint = &map.waypoints[&index.parse::<u32>().unwrap()];
        assert_eq!(
            json!([waypoint.rx, waypoint.ry]),
            *expected,
            "native waypoint{index}"
        );
    }
}

#[test]
fn action_11_inline_labels_and_ordered_effects_match_original_controls() {
    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    let message = corpus
        .iter()
        .find(|row| row["case_kind"] == "localized_message")
        .unwrap();
    let mut compared = 0;
    for control in message["label_controls"].as_array().unwrap() {
        let entry = if let Some(raw) = control["read"]["read"].as_str() {
            let ini =
                crate::rules::ini_parser::IniFile::from_str(&format!("[Actions]\nLABEL={raw}\n"));
            crate::map::actions::parse_actions(&ini)
                .remove("LABEL")
                .unwrap()
                .entries
                .remove(0)
        } else {
            // Original constructor-only control retains an empty inline label.
            ActionEntry {
                kind: 11,
                params: Vec::new(),
                waypoint_index: Some(0),
            }
        };
        assert_eq!(entry.csf_label(), control["label"].as_str().unwrap());
        let mut sim = Simulation::new();
        let mut definitions = RuntimeDefinitions::default();
        let typ = definitions.trigger_type("LABEL").unwrap();
        let action = definitions.resolve_action(entry, None, None);
        sim.trigger_runtime.definitions = definitions;
        let preceding = TriggerEffect::InputLockChanged {
            locked: true,
            was_locked: false,
        };
        let mut effects = vec![preceding.clone()];
        let result = sim.apply_live_trigger_action(
            &action,
            typ,
            &mut effects,
            None,
            None,
            FrameEffects::default(),
        );
        assert_eq!(json!(i32::from(result)), control["returned"]);
        let mut expected = vec![preceding];
        let label = control["label"].as_str().unwrap();
        if !label.is_empty() {
            expected.push(TriggerEffect::LocalizedMessage {
                csf_label: label.to_string(),
            });
        }
        assert_eq!(
            effects, expected,
            "{:?}: native posting admission and action order",
            control["input"]
        );
        compared += 1;
    }
    assert_eq!(compared, 10);
}

#[test]
fn restored_live_owner_rejects_dangling_definitions_instances_and_attachments() {
    let corpus = rows();
    let input = &corpus
        .iter()
        .find(|row| row["input"]["name"] == "reverse_chain")
        .unwrap()["input"];
    let mut sim = Simulation::with_seed(31);
    sim.trigger_runtime.definitions = supplied_definitions(input);
    sim.trigger_runtime.definitions_prepared = true;
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
        64,
        64,
        crate::map::resolved_terrain::test_clear_cell,
    ));
    let house = supplied_house(&mut sim, input);
    let tag = sim.shared_trigger_tag(0);
    let object = sim.allocate_stable_id();
    let type_id = sim.interner.intern("E1");
    let mut entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        object,
        0,
        0,
        0,
        0,
        house,
        crate::sim::components::Health { current: 100 },
        type_id,
        crate::map::entities::EntityCategory::Unit,
        0,
        5,
        false,
    );
    entity.lifecycle.in_limbo = true;
    sim.substrate.entities.insert(entity);
    sim.replace_trigger_tag_attachment(TagAttachment::Object(object), Some(tag));
    let cell = sim.trigger_cell_identity((50, 60));
    sim.replace_cell_trigger_tag(cell, Some(tag));
    sim.trigger_runtime.logic_tags.push(tag);
    sim.validate_restored_trigger_runtime().unwrap();
    let owner = sim.trigger_runtime.clone();
    let corruptions: [(&str, fn(&mut Simulation)); 10] = [
        ("definition link", |sim| {
            sim.trigger_runtime.definitions.trigger_types[0].linked = Some(usize::MAX)
        }),
        ("team reference count", |sim| {
            sim.trigger_runtime.definitions.team_tags.push(None)
        }),
        ("team retained tag", |sim| {
            let index = sim.trigger_runtime.definitions.team_type("TT").unwrap();
            sim.trigger_runtime.definitions.team_tags[index] = Some(usize::MAX);
        }),
        ("live type", |sim| {
            sim.trigger_runtime.tags[0].as_mut().unwrap().type_index = usize::MAX
        }),
        ("live link", |sim| {
            sim.trigger_runtime.tags[0].as_mut().unwrap().triggers[0] = usize::MAX
        }),
        ("registry duplicate", |sim| {
            sim.trigger_runtime.tag_order.push(0)
        }),
        ("object attachment", |sim| {
            sim.trigger_runtime
                .attachments
                .insert(TagAttachment::Object(u64::MAX), 0);
        }),
        ("team attachment", |sim| {
            sim.trigger_runtime
                .attachments
                .insert(TagAttachment::Team(u64::MAX), 0);
        }),
        ("cell attachment", |sim| {
            sim.trigger_runtime
                .cell_tags
                .insert(NativeCellIdentity::Real(usize::MAX), 0);
        }),
        ("reference count", |sim| {
            sim.trigger_runtime.tags[0].as_mut().unwrap().refs = 999
        }),
    ];
    for (name, corrupt) in corruptions {
        sim.trigger_runtime = owner.clone();
        corrupt(&mut sim);
        assert!(sim.validate_restored_trigger_runtime().is_err(), "{name}");
    }
    sim.trigger_runtime = owner;
    sim.mark_live_tag_for_delete(tag);
    sim.validate_restored_trigger_runtime().unwrap();
    // The Tag deleting destructor leaves its Triggers in the common queue.
    // This valid saved intermediate state must not reconstruct or reject them.
    sim.substrate
        .pending_delete
        .remove_ticket_for_test(crate::sim::world::DeferredDeleteTicket::Tag(tag));
    sim.delete_live_tag(tag, None, 0, false);
    sim.validate_restored_trigger_runtime().unwrap();
}

#[test]
fn physical_team_tag_references_match_original_retention_and_long_id_controls() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;

    let corpus = rows();
    let reference = corpus
        .iter()
        .find(|row| row["input"]["reference_controls"] == true)
        .unwrap();
    let controls = reference["team_tag_controls"].as_array().unwrap();
    assert_eq!(controls.len(), 12);
    let team_source = |value: &Value| {
        format!(
            // The native field controls supply a constructed TT directly.
            // Keep its section admitted by the full physical reader even
            // when Tag is absent/empty; entryless sections are discarded by
            // LoadFromStraw525A60 and are outside that isolated field prior.
            "[TeamTypes]\n0=TT\n[TT]\nName=TT\n{}",
            value
                .as_str()
                .map_or(String::new(), |value| format!("Tag={value}\n"))
        )
    };
    const MAP_PREFIX: &str = "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\n\
        LocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n";
    for control in controls {
        let fixed = IniFile::from_str(&team_source(&control["values"][0]));
        for (step, expected) in control["history"].as_array().unwrap().iter().enumerate() {
            let scenario = if step == 0 {
                MAP_PREFIX.to_string()
            } else {
                format!("{MAP_PREFIX}{}", team_source(&control["values"][1]))
            };
            let map = crate::map::map_file::MapFile::from_bytes(scenario.as_bytes()).unwrap();
            let teams = TeamAiIniRegistry::from_sources(&fixed, &map.ini, false);
            let definitions = RuntimeDefinitions::read_map(&map, &teams, None, None);
            assert_eq!(definitions.team_types, ["TT"]);
            assert_eq!(
                json!(
                    definitions
                        .tag_types
                        .iter()
                        .map(|typ| typ.id.as_str())
                        .collect::<Vec<_>>()
                ),
                expected["after"],
                "{} read {step}: factory order",
                control["name"]
            );
            let retained = definitions.team_tags[0];
            let actual = retained.map_or(
                Value::Null,
                |index| json!({"index":index,"id":definitions.tag_types[index].id}),
            );
            assert_eq!(
                actual, expected["result"],
                "{} read {step}",
                control["name"]
            );

            // Native Team6E8D76 consumes its stored Type+D0 pointer. The
            // constructor must allocate a fresh live Tag from that ordinal,
            // without resolving the raw name again or consuming an RNG draw.
            let mut sim = Simulation::new();
            sim.trigger_runtime.definitions = definitions;
            let typ = sim.interner.intern("TT");
            let before = sim.trigger_runtime.definitions.clone();
            let (_, draws) = crate::sim::rng::trace_draws(|| sim.attach_team_trigger_tag(7, typ));
            assert!(draws.is_empty());
            assert_eq!(sim.trigger_runtime.definitions, before);
            let attachment = sim.trigger_runtime.attachments.get(&TagAttachment::Team(7));
            assert_eq!(attachment.is_some(), retained.is_some());
            if let Some(&tag) = attachment {
                assert_eq!(
                    sim.trigger_runtime.tag(tag).unwrap().type_index,
                    retained.unwrap()
                );
                assert_eq!(sim.trigger_runtime.tag(tag).unwrap().refs, 0);
            }
            let bytes = bincode::serialize(&sim.trigger_runtime).unwrap();
            sim.trigger_runtime = bincode::deserialize(&bytes).unwrap();
            sim.attach_team_trigger_tag(8, typ);
            assert_eq!(sim.trigger_runtime.definitions, before);
            if let Some(retained) = retained {
                let first = sim.trigger_runtime.attachments[&TagAttachment::Team(7)];
                let second = sim.trigger_runtime.attachments[&TagAttachment::Team(8)];
                assert_ne!(first, second);
                assert_eq!(
                    sim.trigger_runtime.tag(second).unwrap().type_index,
                    retained
                );
            }
        }
        assert_eq!(control["rng_before"], control["rng_after"]);
    }
}

#[test]
fn team_member_tag_admission_and_removal_match_original_controls() {
    use crate::rules::art_data::ArtRegistry;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::ruleset::RuleSet;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::house_state::HouseState;
    use crate::sim::rng::SimRng;
    use crate::sim::team_script_vm::TeamScriptVm;

    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/team_creation.json",
    ))
    .unwrap();
    let controls: Vec<_> = corpus
        .iter()
        .filter(|row| row["case_kind"] == "member_tag")
        .collect();
    assert_eq!(controls.len(), 8);
    let Some((retail, art)) = crate::rules::retail_ini_fixture::retail_rules_and_art() else {
        return;
    };
    let mut actual = Vec::new();
    let mut expected = Vec::new();
    for control in controls {
        let input = &control["input"];
        let mut layers = RulesLayerStack::new(retail.clone());
        // The original matrix explicitly supplies Type+5E0=0/1 on a genuine
        // E1. This is an admission control, not a retail CARRIER cargo proof.
        layers.push(
            RulesLayerKind::Scenario,
            IniFile::from_str(&format!(
                "[E1]\nPassengers={}\n",
                integer(&input["passengers"])
            )),
        );
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
        rules.install_art_data(ArtRegistry::from_ini(&art));
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        sim.houses
            .insert(owner, HouseState::new(owner, 0, Some(owner), false, 0, 10));
        let definitions = IniFile::from_str(&format!(
            "[TeamTypes]\n0=TT\n[TT]\nHouse=Americans\nScript=S\nTaskForce=F\nOnTransOnly={}\n\
             [ScriptTypes]\n0=S\n[S]\n0=5,0\n[TaskForces]\n0=F\n[F]\n0=1,E1\n",
            if input["on_trans_only"].as_bool().unwrap() {
                "yes"
            } else {
                "no"
            }
        ));
        let registry = TeamAiIniRegistry::from_sources(&definitions, &IniFile::from_str(""), false);
        sim.install_team_ai_registry(&registry, &rules).unwrap();
        let author_type = sim.trigger_runtime.definitions.tag_type("author").unwrap();
        let team_type = sim.trigger_runtime.definitions.tag_type("team").unwrap();
        sim.trigger_runtime.definitions.team_types = vec!["TT".into()];
        sim.trigger_runtime.definitions.team_tags = vec![
            input["nonnull_team_tag"]
                .as_bool()
                .unwrap()
                .then_some(team_type),
        ];
        let typ = sim.interner.get("TT").unwrap();
        let team = sim.create_team(typ, Some(owner), &rules).unwrap();
        let member = sim
            .with_object_placement_scope(|sim| {
                sim.construct_object_limbo_at_height("E1", "Americans", 0, 0, 0, 0, &rules)
            })
            .unwrap();
        let author_tag = sim.construct_trigger_tag(author_type);
        // Supplied anchor attachments retain refs2/1 throughout the original
        // control. Zero-ref destruction and deferred drain are excluded here.
        sim.replace_trigger_tag_attachment(TagAttachment::Object(u64::MAX), Some(author_tag));
        sim.replace_trigger_tag_attachment(TagAttachment::Object(member), Some(author_tag));
        let team_tag = sim
            .trigger_runtime
            .attachments
            .get(&TagAttachment::Team(team))
            .copied();
        if let Some(tag) = team_tag {
            sim.replace_trigger_tag_attachment(TagAttachment::Object(u64::MAX - 1), Some(tag));
        }
        let state = |sim: &Simulation| {
            let attached = sim
                .trigger_runtime
                .attachments
                .get(&TagAttachment::Object(member))
                .copied();
            json!({
                "object_tag": attached.map(|tag| if tag == author_tag { "author" } else if Some(tag) == team_tag { "team" } else { "other" }),
                "author_refs": sim.trigger_runtime.tag(author_tag).unwrap().refs,
                "team_refs": team_tag.map(|tag| sim.trigger_runtime.tag(tag).unwrap().refs),
                "has_team": sim.team_script_vm.team_for_member(member).is_some(),
                "team_count": sim.team_script_vm.team(team).unwrap().taskforce_counts_for_test()[0],
            })
        };
        let native_state = |value: &Value| {
            json!({
                "object_tag": value["object_tag"], "author_refs": value["author_refs"],
                "team_refs": value["team_refs"], "has_team": value["foot_team"] != "0x0",
                "team_count": value["team_count"],
            })
        };
        let prior = state(&sim);
        // Import the native mutation boundary after genuine constructors;
        // concrete constructor RNG is separately covered by reinforcement rows.
        let rng_before = &control["member_rng_before"];
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(rng_before["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(rng_before["main"].as_str().unwrap()).into();
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(rng_before["mapgen"].as_str().unwrap());
        let (added, add_draws) = crate::sim::rng::trace_draws(|| {
            sim.with_object_placement_scope(|sim| {
                sim.team_add_member(team, member, false, &rules, None, FrameEffects::default())
            })
        });
        assert_eq!(i32::from(added), integer(&control["added_al"]), "{input}");
        let after_add = state(&sim);
        sim.trigger_runtime =
            bincode::deserialize(&bincode::serialize(&sim.trigger_runtime).unwrap()).unwrap();
        sim.team_script_vm =
            bincode::deserialize::<TeamScriptVm>(&bincode::serialize(&sim.team_script_vm).unwrap())
                .unwrap();
        assert_eq!(
            state(&sim),
            after_add,
            "{input}: retained attachments/membership through save"
        );
        let (_, remove_draws) = crate::sim::rng::trace_draws(|| {
            sim.team_remove_member(team, member, false, Some(&rules), FrameEffects::default());
        });
        assert!(add_draws.is_empty() && remove_draws.is_empty(), "{input}");
        let rng_after = &control["member_rng_after"];
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            rng_after["scenario"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            rng_after["main"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            rng_after["mapgen"].as_str().unwrap(),
            "{input}"
        );
        actual.push(json!({"name":input["name"], "prior":prior, "after_add":after_add, "after_remove":state(&sim)}));
        expected.push(json!({"name":input["name"], "prior":native_state(&control["prior"]), "after_add":native_state(&control["after_add"]), "after_remove":native_state(&control["after_remove"])}));
        assert!(
            sim.substrate.pending_delete.is_empty(),
            "{input}: anchors prevent destructor admission"
        );
    }
    assert_eq!(
        actual, expected,
        "original AddMember6EA500 / RemoveMember6EA870 Tag histories"
    );
}

#[test]
fn definition_factories_match_all_original_action_reference_controls() {
    let corpus = rows();
    let controls = corpus
        .iter()
        .find(|row| row["input"]["reference_controls"] == true)
        .unwrap();
    // The original controls are a continuous reader history: parameter types
    // 1 and5 share the TeamType array, including two distinct identities whose
    // stored24-byte IDs are equal. Replaying each truncated `before` name
    // through FindOrAllocate would collapse that supplied native prior.
    let mut definitions = RuntimeDefinitions::default();
    for name in controls["initial_ids"].as_array().unwrap() {
        let name = name.as_str().unwrap();
        definitions.team_type(name);
        definitions.trigger_type(name);
        definitions.tag_type(name);
    }
    let mut compared = 0;
    for row in controls["rows"].as_array().unwrap() {
        let parameter_type = integer(&row["parameter_type"]);
        let before: Vec<_> = match parameter_type {
            1 | 5 => definitions.team_types.iter().map(String::as_str).collect(),
            2 => definitions
                .trigger_types
                .iter()
                .map(|typ| typ.id.as_str())
                .collect(),
            3 => definitions
                .tag_types
                .iter()
                .map(|typ| typ.id.as_str())
                .collect(),
            _ => unreachable!(),
        };
        assert_eq!(
            json!(before),
            row["before"],
            "{}: original retained prior",
            row["raw"]
        );
        let raw = row["raw"].as_str().unwrap();
        let ini =
            crate::rules::ini_parser::IniFile::from_str(&format!("[Actions]\nREFERENCE={raw}\n"));
        let record = crate::map::actions::parse_actions(&ini)
            .remove("REFERENCE")
            .unwrap();
        let resolved = definitions.resolve_action(record.entries[0].clone(), None, None);
        let names: Vec<_> = match parameter_type {
            1 | 5 => definitions.team_types.iter().map(String::as_str).collect(),
            2 => definitions
                .trigger_types
                .iter()
                .map(|typ| typ.id.as_str())
                .collect(),
            3 => definitions
                .tag_types
                .iter()
                .map(|typ| typ.id.as_str())
                .collect(),
            _ => unreachable!(),
        };
        assert_eq!(json!(names), row["after"], "{raw}: retained type order");
        let result = match resolved.reference {
            None => Value::Null,
            Some(DefinitionReference::Team(index)) => json!({
                "index": index, "id": definitions.team_types[index],
                "name": definitions.team_types[index],
            }),
            Some(DefinitionReference::Trigger(index)) => {
                let typ = &definitions.trigger_types[index];
                json!({ "index": index, "id": typ.id, "name": typ.id,
                    "flags": [i32::from(typ.difficulty[0]), i32::from(typ.difficulty[1]),
                        i32::from(typ.difficulty[2]), i32::from(typ.enabled), 0],
                    "linked": typ.linked, "event_count": typ.events.len(),
                    "action_count": typ.actions.len() })
            }
            Some(DefinitionReference::Tag(index)) => {
                let typ = &definitions.tag_types[index];
                json!({ "index": index, "id": typ.id, "name": typ.name,
                    "repeat": typ.repeat, "trigger": typ.trigger })
            }
        };
        let mut expected = row["result"].clone();
        if let Some(object) = expected.as_object_mut() {
            object.remove("id_hex");
        }
        assert_eq!(result, expected, "{raw}: original factory return/defaults");
        compared += 1;
    }
    assert_eq!(compared, 44);
}

fn stock_state(sim: &Simulation, draws: usize) -> Value {
    let runtime = &sim.trigger_runtime;
    let tag_name = |id: usize| {
        runtime.definitions.tag_types[runtime.tag(id).unwrap().type_index]
            .id
            .clone()
    };
    let live_tags: Vec<_> = runtime
        .tag_order
        .iter()
        .map(|&id| {
            let tag = runtime.tag(id).unwrap();
            let triggers: Vec<_> = tag
                .triggers
                .iter()
                .map(|&id| {
                    let live = runtime.trigger(id).unwrap();
                    json!({ "type": runtime.definitions.trigger_types[live.type_index].id,
                "enabled": i32::from(live.enabled), "pending": i32::from(live.pending),
                "start": live.timer.start_frame(), "duration": live.timer.duration(),
                "completed": live.completed })
                })
                .collect();
            json!({ "type": tag_name(id), "refs": tag.refs, "pending": i32::from(tag.pending),
            "guard": i32::from(tag.dispatch_guard), "triggers": triggers })
        })
        .collect();
    let houses: serde_json::Map<_, _> = sim
        .session
        .house_order
        .iter()
        .map(|house| {
            let tags: Vec<_> = runtime
                .house_tags
                .get(house)
                .into_iter()
                .flatten()
                .map(|&tag| tag_name(tag))
                .collect();
            (sim.interner.resolve(*house).to_string(), json!(tags))
        })
        .collect();
    json!({ "frame": sim.session.binary_frame, "live_tags": live_tags,
        "logic_tags": runtime.logic_tags.iter().map(|&tag| tag_name(tag)).collect::<Vec<_>>(),
        "map_tags": runtime.map_tags.iter().map(|&tag| tag_name(tag)).collect::<Vec<_>>(),
        "house_tags": houses, "rng_draws": draws, "logic_cursor": runtime.logic_cursor,
        "dispatches": runtime.dispatch_trace.iter().map(|&(tag, kind)| (tag_name(tag), kind)).collect::<Vec<_>>() })
}

fn compare_stock_state(sim: &Simulation, draws: usize, expected: &Value) {
    let actual = stock_state(sim, draws);
    for (key, value) in actual.as_object().unwrap() {
        assert_eq!(value, &expected[key], "{}: {key}", expected["phase"]);
    }
}

#[test]
fn physical_retail_sov01_registration_and_enable_timers_match_original_at_all_difficulties() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    let Some(raw_ai) = crate::rules::retail_ini_fixture::retail_ini_bytes("aimd.ini") else {
        return;
    };
    let Some(raw_eva) = crate::rules::retail_ini_fixture::retail_ini_bytes("evamd.ini") else {
        return;
    };
    let Some(raw_rules) = crate::rules::retail_ini_fixture::retail_ini_bytes("rulesmd.ini") else {
        return;
    };
    let Some(raw_art) = crate::rules::retail_ini_fixture::retail_ini_bytes("artmd.ini") else {
        return;
    };
    let Some(raw_map) = crate::rules::retail_ini_fixture::retail_archive_bytes("SOV01UMD.MAP")
    else {
        return;
    };
    let map = crate::map::map_file::MapFile::from_bytes(&raw_map).unwrap();
    let teams = crate::rules::team_ai_ini::TeamAiIniRegistry::from_sources(
        &IniFile::from_bytes(&raw_ai).unwrap(),
        &map.ini,
        false,
    );
    let eva =
        crate::rules::sound_ini::EvaRegistry::from_ini(&IniFile::from_bytes(&raw_eva).unwrap());
    let mut layers = RulesLayerStack::new(IniFile::from_bytes(&raw_rules).unwrap());
    layers.push(RulesLayerKind::Scenario, map.ini.clone());
    let processed = layers
        .process_with_fixed_art(&IniFile::from_bytes(&raw_art).unwrap())
        .unwrap();
    let rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
    let roster = crate::map::houses::parse_house_roster(&map.ini, &[], Some(&rules));
    let mut compared = 0;
    for row in rows()
        .into_iter()
        .filter(|row| row["input"]["stock_prefix"] == true)
    {
        for (name, raw) in [
            ("sov01umd.map", &raw_map),
            ("aimd.ini", &raw_ai),
            ("evamd.ini", &raw_eva),
            ("rulesmd.ini", &raw_rules),
        ] {
            assert_eq!(json!(raw.len()), row["sources"][name]["bytes"]);
            assert_eq!(
                crate::util::sha256::sha256_hex(raw),
                row["sources"][name]["sha256"]
            );
        }
        let mut sim = Simulation::with_seed(31);
        sim.scenario_rng = crate::sim::rng::SimRng::from_native_state_hex_for_test(
            row["rng_before"]["scenario"].as_str().unwrap(),
        );
        sim.session.binary_frame = 0;
        // The original comparison supplies sparse initialized Cells and
        // minimal House/Country bindings. Full terrain and House construction
        // RNG are explicitly outside this native prefix's coverage.
        sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
            160,
            128,
            crate::map::resolved_terrain::test_clear_cell,
        ));
        for authored in &roster.houses {
            let house = sim.interner.intern(&authored.name);
            let country = authored
                .country
                .as_deref()
                .map(|name| sim.interner.intern(name));
            sim.houses.insert(
                house,
                crate::sim::house_state::HouseState::new(house, 0, country, false, 0, 10),
            );
            sim.session.house_order.push(house);
        }
        sim.initialize_trigger_definitions(&map, &teams, Some(&eva), None);
        sim.trigger_runtime.fixture_difficulty = Some(integer(&row["input"]["difficulty"]));
        let runtime = &sim.trigger_runtime;
        let final_order = row["type_orders"].as_array().unwrap().last().unwrap();
        assert_eq!(
            json!(runtime.definitions.team_types),
            final_order["registries"]["team_type"]
        );
        assert_eq!(
            json!(
                runtime
                    .definitions
                    .trigger_types
                    .iter()
                    .map(|typ| &typ.id)
                    .collect::<Vec<_>>()
            ),
            final_order["registries"]["trigger_type"]
        );
        assert_eq!(
            json!(
                runtime
                    .definitions
                    .tag_types
                    .iter()
                    .map(|typ| &typ.id)
                    .collect::<Vec<_>>()
            ),
            final_order["registries"]["tag_type"]
        );
        let tag_definitions: Vec<_> = runtime
            .definitions
            .tag_types
            .iter()
            .enumerate()
            .map(|(index, typ)| {
                json!({ "id": typ.id, "repeat": typ.repeat,
                "trigger": typ.trigger.map(|id| &runtime.definitions.trigger_types[id].id),
                "flags": runtime.definitions.classification(index) })
            })
            .collect();
        assert_eq!(json!(tag_definitions), row["definitions"]);
        for selected in row["selected_triggers"].as_array().unwrap() {
            let index = runtime
                .definitions
                .trigger_types
                .iter()
                .position(|typ| typ.id == selected["id"].as_str().unwrap())
                .unwrap();
            let typ = &runtime.definitions.trigger_types[index];
            assert_eq!(json!(index), selected["index"]);
            assert_eq!(json!(typ.owner), selected["owner"]);
            assert_eq!(
                json!(
                    typ.events
                        .iter()
                        .map(|event| json!({"kind": event.kind, "value":event.value}))
                        .collect::<Vec<_>>()
                ),
                selected["events"]
            );
            let actions: Vec<_> = typ.actions.iter().map(|action| {
                let (mut team, mut trigger, mut tag) = (Value::Null, Value::Null, Value::Null);
                match action.reference {
                    Some(DefinitionReference::Team(index)) => team = json!(runtime.definitions.team_types[index]),
                    Some(DefinitionReference::Trigger(index)) => trigger = json!(runtime.definitions.trigger_types[index].id),
                    Some(DefinitionReference::Tag(index)) => tag = json!(runtime.definitions.tag_types[index].id),
                    None => {},
                }
                json!({ "kind": action.entry.kind, "value": action.value,
                    "parameters": action.entry.params.iter().skip(2).take(4).map(|value| crt_atoi(value)).collect::<Vec<_>>(),
                    "team": team, "trigger": trigger, "tag": tag })
            }).collect();
            assert_eq!(json!(actions), selected["actions"]);
        }
        let ((), draws) =
            crate::sim::rng::trace_draws(|| sim.install_authored_cell_trigger_tags(&map));
        let mut draw_count = draws.len();
        compare_stock_state(&sim, draw_count, &row["history"][0]);
        let ((), draws) = crate::sim::rng::trace_draws(|| sim.classify_map_triggers(Some(&rules)));
        draw_count += draws.len();
        compare_stock_state(&sim, draw_count, &row["history"][1]);
        sim.trigger_runtime.stop_before_action = Some(integer(&row["first_poll"]["action_kind"]));
        let boundary = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            sim.advance_triggers(
                TriggerInputs {
                    events: &map.events,
                    actions: &map.actions,
                    rules: Some(&rules),
                },
                None,
                FrameEffects::default(),
            )
        }))
        .expect_err("same original first gameplay-action boundary must stop the prefix");
        assert_eq!(*boundary.downcast::<i32>().unwrap(), 46);
        sim.trigger_runtime.stop_before_action = None;
        compare_stock_state(&sim, draw_count, &row["history"][2]);
        let intro_type = sim
            .trigger_runtime
            .definitions
            .trigger_types
            .iter()
            .position(|typ| typ.id == "0DD8E18C")
            .unwrap();
        let intro = *sim
            .trigger_runtime
            .trigger_order
            .iter()
            .find(|&&id| sim.trigger_runtime.trigger(id).unwrap().type_index == intro_type)
            .unwrap();
        let setup_type = sim
            .trigger_runtime
            .definitions
            .trigger_types
            .iter()
            .position(|typ| typ.id == "0B76DF3C")
            .unwrap();
        let enable = sim.trigger_runtime.definitions.trigger_types[setup_type]
            .actions
            .iter()
            .find(|action| action.entry.kind == 53)
            .unwrap()
            .clone();
        for (step, observation) in row["observations"].as_array().unwrap().iter().enumerate() {
            sim.session.binary_frame = observation["frame"].as_u64().unwrap() as u32;
            let (returned, draws) = crate::sim::rng::trace_draws(|| {
                if observation["phase"] == "resolved53_enable" {
                    sim.apply_live_trigger_action(
                        &enable,
                        setup_type,
                        &mut Vec::new(),
                        Some(&rules),
                        None,
                        FrameEffects::default(),
                    )
                } else {
                    sim.register_live_trigger_event(intro, 13, false, false)
                }
            });
            draw_count += draws.len();
            assert_eq!(json!(i32::from(returned)), observation["returned"]);
            compare_stock_state(&sim, draw_count, &row["history"][step + 3]);
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"]["scenario"]
        );
        compared += 1;
    }
    assert_eq!(compared, 3);
}

fn supplied_definitions(input: &Value) -> RuntimeDefinitions {
    let default = json!([{ "events": [[47,0]], "actions": [[28,7]] }]);
    let triggers = input
        .get("triggers")
        .unwrap_or(&default)
        .as_array()
        .unwrap();
    let mut definitions = RuntimeDefinitions::default();
    for (index, trigger) in triggers.iter().enumerate() {
        let flags = trigger.get("flags").cloned().unwrap_or(json!([1, 1, 1, 1]));
        let events = trigger["events"]
            .as_array()
            .map_or_else(Vec::new, |events| {
                events
                    .iter()
                    .map(|event| EventCondition {
                        kind: integer(&event[0]),
                        value: integer(&event[1]),
                        ..EventCondition::default()
                    })
                    .collect()
            });
        let actions = trigger["actions"]
            .as_array()
            .map_or_else(Vec::new, |actions| {
                actions
                    .iter()
                    .map(|action| RuntimeAction {
                        entry: ActionEntry {
                            kind: integer(&action[0]),
                            params: Vec::new(),
                            waypoint_index: Some(0),
                        },
                        value: Some(integer(&action[1])),
                        reference: action
                            .get(2)
                            .map(|target| DefinitionReference::Trigger(integer(target) as usize)),
                        sound: None,
                    })
                    .collect()
            });
        definitions.trigger_types.push(TriggerType {
            id: format!("TRIGGER{index}"),
            owner: Some("Americans".into()),
            linked: (index + 1 < triggers.len()).then_some(index + 1),
            enabled: integer(&flags[3]) != 0,
            difficulty: [
                integer(&flags[0]) != 0,
                integer(&flags[1]) != 0,
                integer(&flags[2]) != 0,
            ],
            events,
            actions,
        });
    }
    for index in 0..8 {
        definitions.tag_types.push(TagType {
            id: format!("TAG{index}"),
            name: format!("TAG{index}"),
            repeat: input.get("repeat").map_or(2, integer),
            trigger: (!triggers.is_empty()).then_some(0),
        });
    }
    definitions
}

fn supplied_house(sim: &mut Simulation, input: &Value) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, Some(owner), false, 0, 10);
    if input["active_cleanup"].as_bool().unwrap_or(false) {
        house.supply_allow_win_tag_state(
            input.get("allow_win_count").map_or(3, integer),
            CdTimer::from_raw(-1, 99),
        );
    }
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    owner
}

fn supplied_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[Countries]\n0=Americans\n[Americans]\nName=United States\n",
    ))
    .unwrap()
}

fn history_state(
    sim: &Simulation,
    input: &Value,
    identities: &[usize],
    objects: &[u64],
    returned: Option<i32>,
    draws: usize,
    house: crate::sim::intern::InternedId,
) -> Value {
    let runtime = &sim.trigger_runtime;
    let mixed = input["mixed_deferred_queue"].as_bool().unwrap_or(false);
    let ticket_value = |ticket: DeferredDeleteTicket| match ticket {
        DeferredDeleteTicket::Object(id) => json!([
            "object",
            objects.iter().position(|&object| object == id).unwrap()
        ]),
        DeferredDeleteTicket::Tag(id) => {
            json!(["tag", identities.iter().position(|&tag| tag == id).unwrap()])
        }
        DeferredDeleteTicket::Trigger(id) => json!(["trigger", id]),
        DeferredDeleteTicket::Overlay(_) => {
            panic!("mixed Object/Tag/Trigger controls contain no Overlay")
        }
    };
    let destructs: Vec<_> = sim
        .lifecycle_test_events_for_test()
        .iter()
        .filter_map(|event| match event {
            crate::sim::world::LifecycleTestEvent::DeferredDeleteSelected { ticket } => {
                Some(*ticket)
            }
            _ => None,
        })
        .collect();
    let tags: Vec<_> = identities
        .iter()
        .map(|&index| {
            let Some(tag) = runtime.tag(index) else {
                return Value::Null;
            };
            let triggers: Vec<_> = tag
                .triggers
                .iter()
                .map(|&index| {
                    let trigger = runtime.trigger(index).unwrap();
                    json!({ "type": trigger.type_index, "enabled": i32::from(trigger.enabled),
                "pending": i32::from(trigger.pending), "start": trigger.timer.start_frame(),
                "duration": trigger.timer.duration(), "completed": trigger.completed })
                })
                .collect();
            json!({ "refs": tag.refs, "pending": i32::from(tag.pending), "triggers": triggers })
        })
        .collect();
    let pending_tags: Vec<_> = sim
        .substrate
        .pending_delete
        .retained_tickets()
        .filter_map(|pending| match pending {
            DeferredDeleteTicket::Tag(index) => {
                Some(identities.iter().position(|id| *id == index).unwrap())
            }
            _ => None,
        })
        .collect();
    let mut state = json!({ "returned": returned, "tags": tags, "actions": runtime.action_trace,
        "rng_draws": draws, "pending_count": sim.substrate.pending_delete.len(), "pending_tags": pending_tags,
        "globals": runtime.globals_set, "locals": runtime.locals_set, "changed": i32::from(runtime.dirty[0]) });
    if input["lifecycle"].as_bool().unwrap_or(false) {
        let tags = runtime.tag_order.len();
        let triggers = runtime.trigger_order.len();
        state["frees"] = if mixed {
            json!(
                destructs
                    .iter()
                    .map(|ticket| match ticket {
                        DeferredDeleteTicket::Object(_) => "object",
                        DeferredDeleteTicket::Tag(_) => "tag",
                        DeferredDeleteTicket::Trigger(_) => "trigger",
                        DeferredDeleteTicket::Overlay(_) => unreachable!(),
                    })
                    .collect::<Vec<_>>()
            )
        } else {
            json!(runtime.freed_trace)
        };
        // The corpus supplies initialized empty Objects to exercise attachment
        // teardown. These counts describe those supplied identities only.
        let retained_objects = if mixed {
            objects
                .iter()
                .filter(|&&id| sim.classify_object(id).is_some())
                .count()
        } else {
            objects.len()
        };
        state["registry_counts"] = json!({ "abstract": tags + triggers + retained_objects,
            "tag_trigger_base": tags + triggers, "tag": tags, "trigger": triggers,
            "pending": sim.substrate.pending_delete.len(), "cell_tags": runtime.cell_tags.len(),
            "logic_tags": runtime.logic_tags.len(), "map_tags": runtime.map_tags.len(),
            "object": retained_objects, "object_base": retained_objects, "object_base_b": retained_objects });
        state["object_tags"] = json!(
            objects
                .iter()
                .map(|id| runtime
                    .attachments
                    .get(&TagAttachment::Object(*id))
                    .map(|tag| identities
                        .iter()
                        .position(|identity| identity == tag)
                        .unwrap()))
                .collect::<Vec<_>>()
        );
    }
    if mixed {
        state["pending_tickets"] = json!(
            sim.substrate
                .pending_delete
                .retained_tickets()
                .map(ticket_value)
                .collect::<Vec<_>>()
        );
        state["destruct_order"] =
            json!(destructs.into_iter().map(ticket_value).collect::<Vec<_>>());
        state["object_alive"] = json!(
            objects
                .iter()
                .map(|&id| sim
                    .substrate
                    .entities
                    .get(id)
                    .map(|entity| i32::from(entity.lifecycle.object_alive)))
                .collect::<Vec<_>>()
        );
    }
    if input["polling"].as_bool().unwrap_or(false) {
        state["dispatches"] = json!(
            runtime
                .dispatch_trace
                .iter()
                .map(|(tag, kind)| (
                    identities
                        .iter()
                        .position(|identity| identity == tag)
                        .unwrap(),
                    *kind
                ))
                .collect::<Vec<_>>()
        );
        state["logic_tags"] = json!(
            runtime
                .logic_tags
                .iter()
                .map(|tag| identities
                    .iter()
                    .position(|identity| identity == tag)
                    .unwrap())
                .collect::<Vec<_>>()
        );
        state["scenario_flags"] = json!(runtime.dirty.map(i32::from));
    }
    if input["active_cleanup"].as_bool().unwrap_or(false) {
        let (count, timer) = sim.houses[&house].allow_win_tag_state();
        state["logic_cursor"] = json!(runtime.logic_cursor);
        state["map_cursor"] = json!(runtime.map_cursor);
        state["allow_win_count"] = json!(count);
        state["allow_win_timer"] = json!([timer.start_frame(), timer.duration()]);
    }
    state
}

#[test]
fn live_instances_match_all_native_supplied_histories_and_serialization() {
    let mut compared = 0;
    for row in rows().into_iter().filter(|row| {
        !row["input"]["stock_prefix"].as_bool().unwrap_or(false)
            && !row["input"]["reference_controls"]
                .as_bool()
                .unwrap_or(false)
    }) {
        let input = &row["input"];
        let name = input["name"].as_str().unwrap();
        let mut sim = Simulation::with_seed(input["seed"].as_u64().unwrap_or(31));
        // The original small-world fixture supplies initialized CellClass
        // allocations at(50,60)/(51,60). This is a storage prerequisite,
        // not a claim about the production retail terrain-construction pass.
        sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
            64,
            64,
            crate::map::resolved_terrain::test_clear_cell,
        ));
        sim.session.binary_frame = input["frame"].as_u64().unwrap_or(123) as u32;
        sim.trigger_runtime.definitions = supplied_definitions(input);
        sim.trigger_runtime.fixture_difficulty = Some(input.get("difficulty").map_or(1, integer));
        sim.trigger_runtime.logic_cursor = input.get("logic_cursor").map_or(0, integer);
        sim.trigger_runtime.map_cursor = input.get("map_cursor").map_or(0, integer);
        let house = supplied_house(&mut sim, input);
        let rules = supplied_rules();
        let flags = input.get("live_flags").cloned().unwrap_or(json!([0, 0]));
        let default_operations = json!([["shared"], ["dispatch", 0, 13], ["dispatch", 0, 13]]);
        let operations = input
            .get("operations")
            .unwrap_or(&default_operations)
            .as_array()
            .unwrap();
        let mut identities = Vec::new();
        let mut objects = Vec::new();
        let mut draws = 0;
        for (step, operation) in operations.iter().enumerate() {
            let args = operation.as_array().unwrap();
            let ((returned, effects), trace) = crate::sim::rng::trace_draws(|| {
                let mut effects = Vec::new();
                let mut returned = None;
                match args[0].as_str().unwrap() {
                    "shared" | "fresh" => {
                        let typ = args.get(1).map_or(0, integer) as usize;
                        let index = if args[0] == "shared" {
                            sim.shared_trigger_tag(typ)
                        } else {
                            sim.construct_trigger_tag(typ)
                        };
                        if !identities.contains(&index) {
                            identities.push(index);
                        }
                        returned =
                            Some(identities.iter().position(|id| *id == index).unwrap() as i32);
                    }
                    "frame" => sim.session.binary_frame = args[1].as_u64().unwrap() as u32,
                    "attach_cell" => {
                        let cell = sim.trigger_cell_identity((
                            integer(&args[2]) as i16,
                            integer(&args[3]) as i16,
                        ));
                        sim.replace_cell_trigger_tag(
                            cell,
                            Some(identities[integer(&args[1]) as usize]),
                        );
                    }
                    "object" => {
                        if input["mixed_deferred_queue"] == true {
                            // This control supplies a plain, unplaced Object
                            // lifetime. The shared drain's family admission,
                            // ordering and physical removal are compared; no
                            // concrete Unit destructor parity is claimed.
                            let id = sim.allocate_stable_id();
                            let type_id = sim.interner.intern("E1");
                            let mut object =
                                crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
                                    id,
                                    0,
                                    0,
                                    0,
                                    0,
                                    house,
                                    crate::sim::components::Health { current: 100 },
                                    type_id,
                                    crate::map::entities::EntityCategory::Unit,
                                    0,
                                    5,
                                    false,
                                );
                            object.lifecycle.in_limbo = true;
                            object.in_logic_vector = false;
                            sim.substrate.entities.insert(object);
                            objects.push(id);
                        } else {
                            objects.push(objects.len() as u64);
                        }
                    }
                    "object_alive" => {
                        sim.substrate
                            .entities
                            .get_mut(objects[integer(&args[1]) as usize])
                            .unwrap()
                            .lifecycle
                            .object_alive = integer(&args[2]) != 0;
                    }
                    "supply_pending" => {
                        sim.substrate.pending_delete.clear_for_test();
                        for ticket in args[1].as_array().unwrap() {
                            let index = integer(&ticket[1]) as usize;
                            match ticket[0].as_str().unwrap() {
                                "object" => {
                                    sim.substrate.pending_delete.queue_object(objects[index])
                                }
                                "tag" => sim.substrate.pending_delete.queue_tag(identities[index]),
                                "trigger" => sim.substrate.pending_delete.queue_trigger(index),
                                family => panic!("unexpected supplied family {family}"),
                            }
                        }
                    }
                    "attach_object" => {
                        let tag = (integer(&args[2]) >= 0)
                            .then(|| identities[integer(&args[2]) as usize]);
                        returned = Some(i32::from(sim.replace_trigger_tag_attachment(
                            TagAttachment::Object(objects[integer(&args[1]) as usize]),
                            tag,
                        )));
                    }
                    "dispatch" => {
                        let cell = args.get(3).map_or((0, 0), |cell| {
                            (integer(&cell[0]) as i16, integer(&cell[1]) as i16)
                        });
                        let force = args.get(4).is_some_and(|value| integer(value) != 0);
                        let object = args.get(5).map(|index| objects[integer(index) as usize]);
                        returned = Some(i32::from(sim.dispatch_live_tag_event(
                            identities[integer(&args[1]) as usize],
                            integer(&args[2]),
                            object,
                            cell,
                            force,
                            &mut effects,
                            Some(&rules),
                            None,
                            FrameEffects::default(),
                        )));
                    }
                    "global" | "local" => {
                        returned = Some(i32::from(sim.set_trigger_variable(
                            args[0] == "global",
                            integer(&args[1]),
                            integer(&args[2]) != 0,
                        )))
                    }
                    "action" => {
                        let action = RuntimeAction {
                            entry: ActionEntry {
                                kind: integer(&args[1]),
                                params: Vec::new(),
                                waypoint_index: Some(0),
                            },
                            reference: Some(DefinitionReference::Trigger(
                                integer(&args[2]) as usize
                            )),
                            value: Some(0),
                            sound: None,
                        };
                        returned = Some(i32::from(sim.apply_live_trigger_action(
                            &action,
                            0,
                            &mut effects,
                            Some(&rules),
                            None,
                            FrameEffects::default(),
                        )));
                    }
                    "mark_tag" => {
                        sim.mark_live_tag_for_delete(identities[integer(&args[1]) as usize])
                    }
                    "mark_trigger" => {
                        let trigger = sim
                            .trigger_runtime
                            .tag(identities[integer(&args[1]) as usize])
                            .unwrap()
                            .triggers[integer(&args[2]) as usize];
                        sim.mark_live_trigger_for_delete(trigger);
                    }
                    "drain" => sim.process_pending_delete_with(
                        Some(&rules),
                        None,
                        FrameEffects::default(),
                        integer(&flags[0]),
                        integer(&flags[1]) != 0,
                    ),
                    "register_logic" => {
                        sim.trigger_runtime.logic_tags = args[1..]
                            .iter()
                            .map(|i| identities[integer(i) as usize])
                            .collect()
                    }
                    "register_map" => {
                        sim.trigger_runtime.map_tags = args[1..]
                            .iter()
                            .map(|i| identities[integer(i) as usize])
                            .collect()
                    }
                    "scenario_flags" => {
                        sim.trigger_runtime.dirty = [
                            integer(&args[1]) != 0,
                            integer(&args[2]) != 0,
                            integer(&args[3]) != 0,
                            integer(&args[4]) != 0,
                        ]
                    }
                    "poll" => {
                        effects = sim.advance_triggers(
                            TriggerInputs {
                                events: &HashMap::new(),
                                actions: &HashMap::new(),
                                rules: Some(&rules),
                            },
                            None,
                            FrameEffects::default(),
                        );
                    }
                    kind => panic!("unhandled native operation {name}/{step}/{kind}"),
                }
                (returned, effects)
            });
            assert!(
                effects.is_empty(),
                "supplied native histories do not contain presentation actions"
            );
            draws += trace.len();
            sim.substrate.pending_delete.assert_consistent_for_test();
            assert_eq!(
                history_state(&sim, input, &identities, &objects, returned, draws, house),
                row["history"][step],
                "{name}/{step}: {operation}"
            );
            let bytes =
                bincode::serialize(&(&sim.trigger_runtime, &sim.substrate.pending_delete)).unwrap();
            let (mut restored, restored_queue): (TriggerRuntime, DeferredDeleteQueue) =
                bincode::deserialize(&bytes).unwrap();
            assert_eq!(restored, sim.trigger_runtime, "{name}/{step} saved owner");
            assert_eq!(
                restored_queue, sim.substrate.pending_delete,
                "{name}/{step} saved queue"
            );
            restored_queue.assert_consistent_for_test();
            let mut original_hash = std::collections::hash_map::DefaultHasher::new();
            let mut restored_hash = std::collections::hash_map::DefaultHasher::new();
            sim.trigger_runtime.hash_state(&mut original_hash);
            restored.hash_state(&mut restored_hash);
            sim.substrate.pending_delete.hash_state(&mut original_hash);
            restored_queue.hash_state(&mut restored_hash);
            assert_eq!(
                original_hash.finish(),
                restored_hash.finish(),
                "{name}/{step} saved hash"
            );
            // Continue through the restored authority at every boundary. Only
            // cumulative test observers are transferred outside serialization.
            restored.action_trace = std::mem::take(&mut sim.trigger_runtime.action_trace);
            restored.dispatch_trace = std::mem::take(&mut sim.trigger_runtime.dispatch_trace);
            restored.freed_trace = std::mem::take(&mut sim.trigger_runtime.freed_trace);
            sim.trigger_runtime = restored;
            sim.substrate.pending_delete = restored_queue;
        }
        assert_eq!(
            sim.scenario_rng.next_u32(),
            row["next_random"].as_u64().unwrap() as u32,
            "{name} final RNG continuation"
        );
        compared += 1;
    }
    assert_eq!(compared, 62);
}

#[test]
fn native_trigger_reveal_initial_event48_tags_receive_neutral_discovery_dispatch() {
    use crate::sim::game_entity::GameEntity;
    use crate::sim::house_state::HouseState;
    use crate::sim::world::UninitContext;
    let Some(raw_map) = crate::rules::retail_ini_fixture::retail_archive_bytes("SOV01UMD.MAP")
    else {
        return;
    };
    let map = crate::map::map_file::MapFile::from_bytes(&raw_map).unwrap();
    let corpus = crate::sim::vision::discovery_corpus();
    let footprint = &corpus["full_startup_footprint"];
    assert_eq!(
        crate::util::sha256::sha256_hex(&raw_map),
        footprint["sources"]["sov01umd.map"]["sha256"]
    );
    assert!(
        footprint["event4_lead_intersection"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut compared = 0;
    for lead in footprint["admitted_structure_anchor_leads"]
        .as_array()
        .unwrap()
    {
        if lead["type"] != "GTGCAN" {
            continue;
        }
        let mut sim = Simulation::with_seed(31);
        let player = sim.interner.intern("Player");
        let owner = sim.interner.intern(lead["owner"].as_str().unwrap());
        sim.session.current_house = Some(player);
        sim.session.game_mode_nonzero = false;
        sim.houses
            .insert(player, HouseState::new(player, 0, None, true, 0, 10));
        sim.houses
            .insert(owner, HouseState::new(owner, 0, None, false, 0, 10));
        sim.initialize_trigger_definitions(
            &map,
            &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
            None,
            None,
        );
        let typ = sim.interner.intern("GTGCAN");
        let id = sim.allocate_stable_id();
        let mut entity = GameEntity::new_at_frame_for_test(
            id,
            integer(&lead["cell"][0]) as u16,
            integer(&lead["cell"][1]) as u16,
            4,
            0,
            owner,
            crate::sim::components::Health { current: 100 },
            typ,
            crate::map::entities::EntityCategory::Structure,
            0,
            0,
            false,
            0,
        );
        entity.building_actually_placed = true;
        sim.substrate.entities.insert(entity);
        sim.attach_object_trigger_tag(id, Some(lead["tag"].as_str().unwrap()));
        let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(id)];
        let chain = sim.trigger_runtime.tag(tag).unwrap().triggers.clone();
        assert_eq!(chain.len(), 1);
        let trigger = sim.trigger_runtime.trigger(chain[0]).unwrap();
        let events = &sim.trigger_runtime.definitions.trigger_types[trigger.type_index].events;
        assert_eq!(events.len(), 1);
        assert_eq!(
            json!(events[0].kind),
            lead["native_trigger"]["events"][0]["kind"]
        );
        assert_eq!(events[0].kind, 48);
        let before = sim.trigger_runtime.clone();
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            sim.with_object_placement_scope(|sim| {
                assert!(sim.discover_techno(id, owner, UninitContext::default()));
            });
            assert!(sim.trigger_runtime.dispatch_trace.is_empty());
            assert!(sim.discover_techno(id, player, UninitContext::default()));
        });
        assert!(draws.is_empty());
        assert_eq!(sim.trigger_runtime.dispatch_trace, [(tag, 4)]);
        assert_eq!(sim.trigger_runtime, before);
        assert!(sim.drain_trigger_effects().is_empty());
        assert!(!sim.discover_techno(id, player, UninitContext::default()));
        assert_eq!(sim.trigger_runtime.dispatch_trace, [(tag, 4)]);
        compared += 1;
    }
    assert_eq!(compared, 2);
}
