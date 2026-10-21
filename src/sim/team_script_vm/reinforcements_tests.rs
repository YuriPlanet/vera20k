//! Original team_creation executions. Supplied prior state is explicit for
//! 6EC300/script50; action7 uses real Rust constructors, TaskForce admission,
//! class Unlimbo and scalar cleanup against the original composed executions.
//! Building-exit regressions additionally exercise instruction-derived rules
//! through production owners; they are not original whole-spawn goldens.

use std::collections::HashMap;

use serde_json::Value;

use super::*;
use crate::map::entities::EntityCategory;
use crate::map::playfield::PlayfieldBounds;
use crate::map::resolved_terrain::{test_flat_cell, test_grid};
use crate::map::waypoints::Waypoint;
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::retail_ini_fixture::retail_rules_and_art;
use crate::rules::ruleset::RuleSet;
use crate::rules::team_ai_ini::TeamAiIniRegistry;
use crate::rules::terrain_rules::TerrainRules;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::movement::ground_pose::position_world_coord;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::rng::SimRng;
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::save_codec;

fn rows(kind: &str, count: usize) -> Vec<Value> {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/team_creation.json",
    ))
    .unwrap();
    let selected: Vec<_> = corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["case_kind"] == kind)
        .cloned()
        .collect();
    assert_eq!(selected.len(), count, "native {kind} controls");
    selected
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

#[test]
fn on_trans_only_retained_reader_matches_original_and_survives_save() {
    let control = rows("on_trans_only_reader", 1).remove(0);
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ))
    .unwrap();
    assert_eq!(
        i32::from(TeamTypeIniMetadata::default().on_trans_only),
        int(&control["constructor_B3"])
    );
    assert_eq!(control["key"], "OnTransOnly");
    assert_eq!(control["reader_rng_before"], control["reader_rng_after"]);
    let visits = control["rows"].as_array().unwrap();
    assert_eq!(visits.len(), 12);
    for visit in visits {
        // The native sequential reader supplies its retained prior byte.
        // Exercise that same byte across the production fixed AIMD/map passes,
        // including a missing, empty, malformed or wrong-case map key.
        let fixed = IniFile::from_str(&format!(
            "[TeamTypes]\n0=TT\n[TT]\nScript=S\nTaskForce=F\nOnTransOnly={}\n\
             [ScriptTypes]\n0=S\n[S]\n0=5,0\n[TaskForces]\n0=F\n[F]\n0=1,E1\n",
            if int(&visit["before"]) != 0 {
                "yes"
            } else {
                "no"
            }
        ));
        let mut map = "[TeamTypes]\n0=TT\n[TT]\nName=retained\n".to_string();
        if let Some(value) = visit["value"].as_str() {
            map.push_str(&format!("{}={value}\n", visit["key"].as_str().unwrap()));
        }
        let registry = TeamAiIniRegistry::from_sources(&fixed, &IniFile::from_str(&map), false);
        let mut interner = crate::sim::intern::StringInterner::new();
        let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let id = interner.get("TT").unwrap();
        assert_eq!(
            i32::from(vm.team_type_ini[&id].on_trans_only),
            int(&visit["after"]),
            "{visit}"
        );
        let restored: TeamScriptVm =
            save_codec::deserialize(&save_codec::serialize(&vm).unwrap()).unwrap();
        assert_eq!(
            restored.team_type_ini[&id], vm.team_type_ini[&id],
            "{visit}"
        );
        if vm.team_type_ini[&id].on_trans_only {
            use std::hash::Hasher;
            let hash = |vm: &TeamScriptVm| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                vm.hash_state(0, &mut hasher);
                // A Rust consistency check, not a native CRC golden.
                hasher.finish()
            };
            let mut live = vm.clone();
            let owner = interner.intern("OWNER");
            live.construct_team(id, owner, false, 0, false, None)
                .unwrap();
            let saved: TeamScriptVm =
                save_codec::deserialize(&save_codec::serialize(&live).unwrap()).unwrap();
            assert_eq!(hash(&saved), hash(&live));
            let mut unrestricted = live.clone();
            unrestricted
                .team_type_ini
                .get_mut(&id)
                .unwrap()
                .on_trans_only = false;
            assert_ne!(
                hash(&unrestricted),
                hash(&live),
                "future admission changes authoritative Team state"
            );
        }
    }

    let Some(map_bytes) = crate::rules::retail_ini_fixture::retail_archive_bytes("SOV01UMD.MAP")
    else {
        return;
    };
    assert_eq!(
        crate::util::sha256::sha256_hex(&map_bytes),
        control["map_sha256"].as_str().unwrap()
    );
    let map = IniFile::from_bytes(&map_bytes).unwrap();
    // The selected retail Carrier Team is a map addition. An empty fixed pass
    // isolates its native B3 field without requiring unrelated AI definitions.
    let registry = TeamAiIniRegistry::from_sources(&IniFile::from_str(""), &map, false);
    let Some((retail, art)) = retail_rules_and_art() else {
        return;
    };
    let mut layers = crate::rules::native_processing::RulesLayerStack::new(retail);
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        map,
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let mut interner = crate::sim::intern::StringInterner::new();
    let (vm, diagnostics) = TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    let stock = &control["stock"];
    let id = interner.get(stock["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        i32::from(vm.team_type_ini[&id].on_trans_only),
        int(&stock["B3"])
    );
}

fn waypoint(sim: &mut Simulation, index: u32, cell: (i16, i16)) {
    sim.session.install_waypoints(&HashMap::from([(
        index,
        Waypoint {
            index,
            rx: cell.0 as u16,
            ry: cell.1 as u16,
        },
    )]));
}

#[test]
fn shared_team_waypoint_predicate_matches_original_exits_and_dummy_writes() {
    for row in rows("waypoint", 14) {
        let input = &row["input"];
        let mut sim = Simulation::with_seed(31);
        let owner = sim.interner.intern("OWNER");
        let script = sim.interner.intern("SCRIPT");
        let index = input["index"].as_u64().unwrap_or(7) as u32;
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::Scenario,
            actions: vec![
                TeamScriptAction {
                    action_id: input["action"].as_i64().unwrap_or(3) as i32,
                    argument: index as i32,
                },
                TeamScriptAction {
                    action_id: 2,
                    argument: 0,
                },
            ],
        });
        // Original execution supplies precisely this prior formed Team and
        // member→Team reference; the predicate never reads the member object.
        let team = sim.team_script_vm.create_team(owner, script, vec![99], 0);
        let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
        state.formed = input["formed"].as_bool().unwrap_or(true);
        state.cursor = input["cursor"].as_i64().unwrap_or(0) as i32;
        let cell = (
            int(&input["waypoint"][0]) as i16,
            int(&input["waypoint"][1]) as i16,
        );
        waypoint(&mut sim, index, cell);
        let level = input["level"].as_i64().unwrap_or(0) as u8;
        let terrain = test_grid(128, 128, |x, y| {
            let mut result = test_flat_cell(x, y);
            if (x as i16, y as i16) == cell {
                result.level = level;
            }
            result
        });
        terrain.stamp_dummy_cell_requested_coord(-91, 77);
        sim.install_resolved_terrain_for_new_map(terrain);
        sim.set_playfield_bounds_for_test(Some(PlayfieldBounds::from_normalized_local_size(
            64, 0, 0, 64, 64,
        )));
        assert_eq!(
            sim.member_team_is_moving_outside_playfield(99),
            row["result"].as_bool().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .dummy_cell_requested_coord(),
            (int(&row["dummy"][0]), int(&row["dummy"][1])),
            "{input}"
        );
    }
}

#[test]
fn flash_team_type_matches_original_member_writes_across_houses() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_flash_team.json",
    ))
    .unwrap();
    let controls = corpus.as_array().unwrap();
    assert_eq!(controls.len(), 3);
    // Null TAction references belong to its dispatcher. This owner replays
    // the original nonnull TeamType calls, including an empty Team registry.
    for row in controls
        .iter()
        .filter(|row| row["input"]["type_present"] == true)
    {
        let input = &row["input"];
        let mut sim = Simulation::new();
        let owners = [sim.interner.intern("OWNER0"), sim.interner.intern("OWNER1")];
        let types = [
            sim.interner.intern("088C1A4C"),
            sim.interner.intern("OtherFlashTeam"),
        ];
        let script = sim.interner.intern("SCRIPT");
        for (member, initial) in input["initial"].as_array().unwrap().iter().enumerate() {
            let mut entity = GameEntity::test_default(member as u64 + 1, "TANY", "OWNER0", 64, 64);
            entity.set_category_for_test(crate::map::entities::EntityCategory::Infantry);
            // The native member-write body reads none of these admission
            // fields; a retained member is still flashed before retirement.
            entity.lifecycle.set_object_alive_for_test(false);
            entity.lifecycle.set_in_limbo_for_test(true);
            entity.health.set_for_test(0);
            entity.flashing.start(int(initial));
            sim.substrate.entities.insert(entity);
        }
        for definition in input["teams"].as_array().unwrap() {
            let members: Vec<_> = definition["members"]
                .as_array()
                .unwrap()
                .iter()
                .map(|member| member.as_u64().unwrap() + 1)
                .collect();
            let owner = owners[definition["owner"].as_u64().unwrap() as usize];
            let team = sim.team_script_vm.create_team(owner, script, members, 0);
            sim.team_script_vm
                .teams
                .get_mut(&team)
                .unwrap()
                .team_type_id = Some(types[usize::from(definition["type"] == "other")]);
        }
        let teams_before = serde_json::to_string(&sim.team_script_vm).unwrap();
        let rng_before = [
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        ];
        Simulation::flash_teams_of_type(
            &mut sim.substrate,
            &sim.team_script_vm,
            types[0],
            int(&row["read"]["value"]),
        );
        let actual: Vec<_> = (0..input["initial"].as_array().unwrap().len())
            .map(|member| {
                sim.entities()
                    .get(member as u64 + 1)
                    .unwrap()
                    .flashing
                    .frames()
            })
            .collect();
        assert_eq!(
            actual,
            row["stored"]
                .as_array()
                .unwrap()
                .iter()
                .map(int)
                .collect::<Vec<_>>(),
            "{row}"
        );
        assert_eq!(
            serde_json::to_string(&sim.team_script_vm).unwrap(),
            teams_before
        );
        assert_eq!(
            [
                sim.scenario_rng.native_state_hex(),
                sim.main_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex(),
            ],
            rng_before
        );
        assert!(row["rng_calls"].as_array().unwrap().is_empty());
    }
}

#[test]
fn script50_uses_the_existing_signed_flash_owner_and_original_countdown() {
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    for row in rows("elite_flash", 21) {
        let input = &row["input"];
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("OWNER");
        let script = sim.interner.intern("SCRIPT");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::Scenario,
            actions: vec![TeamScriptAction {
                action_id: 50,
                argument: int(&input["argument"]),
            }],
        });
        let initial = row["initial"].as_array().unwrap();
        let members: Vec<_> = initial
            .iter()
            .enumerate()
            .map(|(index, count)| {
                let id = index as u64 + 1;
                let mut entity = GameEntity::test_default(id, "MTNK", "OWNER", 64, 64);
                entity.flashing.start(int(count));
                sim.substrate.entities.insert(entity);
                id
            })
            .collect();
        let team = sim
            .team_script_vm
            .create_team(owner, script, members.clone(), 0);
        if input["first"].as_bool().unwrap() {
            let state = sim.team_script_vm.teams.get_mut(&team).unwrap();
            state.cursor = -1;
            state.advance_pending = true;
        }
        sim.team_step_script(team, &rules, None, FrameEffects::default());
        let counters = |sim: &Simulation| {
            members
                .iter()
                .map(|&id| sim.substrate.entities.get(id).unwrap().flashing.frames())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            counters(&sim),
            row["stored"]
                .as_array()
                .unwrap()
                .iter()
                .map(int)
                .collect::<Vec<_>>(),
            "{input}"
        );
        assert_eq!(
            sim.team_script_vm.teams[&team].advance_pending,
            row["advance"].as_bool().unwrap(),
            "{input}"
        );
        for step in row["steps"].as_array().unwrap() {
            let active: Vec<_> = members
                .iter()
                .map(|&id| sim.substrate.entities.get_mut(id).unwrap().flashing.tick())
                .collect();
            assert_eq!(
                active,
                step["active"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|value| value.as_bool().unwrap())
                    .collect::<Vec<_>>(),
                "{input}"
            );
            assert_eq!(
                counters(&sim),
                step["counters"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(int)
                    .collect::<Vec<_>>(),
                "{input}"
            );
        }
    }
}

pub(crate) fn reinforcement_world(
    rules: &RuleSet,
    terrain_rules: &TerrainRules,
    input: &Value,
) -> Simulation {
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    // Native crop's selected ordinary cells are level4/slope0/Road1.
    // Allocation/bounds are declared prior inputs, not a full map-load claim.
    let road = terrain_rules.semantics_for_land_type(1).unwrap();
    let mut terrain = test_grid(128, 128, |x, y| {
        let mut cell = test_flat_cell(x, y);
        cell.level = 4;
        cell.land_type = 1;
        cell.base_land_type = 1;
        cell.speed_costs = road.speed_costs.clone();
        cell.base_speed_costs = road.speed_costs.clone();
        cell
    });
    if input["droppod"].as_bool().unwrap_or(false) {
        // FootMissions inherits OriginalRim's sparse Anytown crop: y47..61,
        // x82..93; every other native table slot is NULL. Geometry corpus
        // anytown_geometry.json.gz payload e86df51f..1bb164 binds physical
        // XMP03T4.MAP 7a390de3..9e17e; this is not a whole-map load.
        // The House-edge origin therefore uses the unchanged Dummy level0,
        // while the transport waypoint's real crop cell retains level4.
        let allocated: Vec<_> = (47_u16..62)
            .flat_map(|y| (82_u16..94).map(move |x| (x, y)))
            .collect();
        terrain.test_set_native_allocated_cells(&allocated);
        assert_eq!(terrain.shared_cell_dummy().snapshot().level, 0);
    }
    let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&terrain);
    sim.install_fixture_path_grid(Some(&path));
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.session.set_map_width_for_test(80);
    sim.session.set_map_height_for_test(85);
    sim.session.set_binary_frame_for_test(1);
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.set_playfield_bounds_for_test(Some(PlayfieldBounds::from_normalized_local_size(
        80, 5, 5, 70, 73,
    )));
    sim.set_playfield_size_height_for_test(Some(85));
    let country = sim.interner.intern("Americans");
    let owner = sim.interner.intern("Americans");
    let enemy = sim.interner.intern("ENEMY");
    let mut house = HouseState::new(owner, 0, Some(country), true, 0, 10);
    house.initialize_scenario_parameters(
        crate::map::houses::ScenarioHouseParameters {
            tech_level: 10,
            credits: 0,
            edge: input["edge"].as_i64().unwrap_or(0) as i32,
            ratio_team_aircraft: 75,
            ratio_team_infantry: 75,
            ratio_team_units: 75,
        },
        None,
        Some(&rules.general),
    );
    sim.houses.register(house);
    sim.houses
        .register(HouseState::new(enemy, 0, Some(country), true, 0, 10));
    sim.session
        .set_current_house_for_test(Some(if input["allied"].as_bool().unwrap_or(true) {
            owner
        } else {
            enemy
        }));
    let definitions_inputs = input["definitions"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_else(|| std::slice::from_ref(input));
    let mut definitions = "[TeamTypes]\n".to_string();
    for (index, definition) in definitions_inputs.iter().enumerate() {
        definitions.push_str(&format!(
            "{index}={}\n",
            definition["team_name"].as_str().unwrap_or("TT")
        ));
    }
    // Native INI keeps physical duplicate sections, rather than merging
    // their bodies. Emit each registry once before its definition sections.
    definitions.push_str("[ScriptTypes]\n");
    for index in 0..definitions_inputs.len() {
        definitions.push_str(&format!(
            "{index}={}\n",
            if definitions_inputs.len() == 1 {
                "S".to_string()
            } else {
                format!("S{index}")
            }
        ));
    }
    definitions.push_str("[TaskForces]\n");
    for index in 0..definitions_inputs.len() {
        definitions.push_str(&format!(
            "{index}={}\n",
            if definitions_inputs.len() == 1 {
                "F".to_string()
            } else {
                format!("F{index}")
            }
        ));
    }
    for (index, definition) in definitions_inputs.iter().enumerate() {
        let team_name = definition["team_name"].as_str().unwrap_or("TT");
        let script = if definitions_inputs.len() == 1 {
            "S".to_string()
        } else {
            format!("S{index}")
        };
        let task_force = if definitions_inputs.len() == 1 {
            "F".to_string()
        } else {
            format!("F{index}")
        };
        definitions.push_str(&format!(
            "[{team_name}]\nHouse=Americans\nScript={script}\nTaskForce={task_force}\nWaypoint=E\nVeteranLevel={}\nMax={}\n",
            definition["veteran"].as_i64().unwrap_or(1),
            definition["max"].as_i64().unwrap_or(-1)
        ));
        for (field, key) in [
            ("full", "Full"),
            ("transports_return_on_unload", "TransportsReturnOnUnload"),
            ("droppod", "Droppod"),
            ("use_transport_origin", "UseTransportOrigin"),
        ] {
            if let Some(value) = definition[field].as_bool() {
                definitions.push_str(&format!("{key}={}\n", if value { "yes" } else { "no" }));
            }
        }
        if let Some(letters) = definition["transport_waypoint_letters"].as_str() {
            definitions.push_str(&format!("TransportWaypoint={letters}\n"));
        }
        definitions.push_str(&format!("[{script}]\n"));
        if let Some(steps) = definition["script_steps"].as_array() {
            for (index, step) in steps.iter().enumerate() {
                definitions.push_str(&format!("{index}={},{}\n", int(&step[0]), int(&step[1])));
            }
        } else {
            definitions.push_str(&format!(
                "0={}\n",
                definition["script"].as_str().unwrap_or("3,4")
            ));
        }
        definitions.push_str(&format!("[{task_force}]\n"));
        for (index, entry) in definition["entries"].as_array().unwrap().iter().enumerate() {
            definitions.push_str(&format!(
                "{index}={},{}\n",
                int(&entry[1]),
                entry[0].as_str().unwrap()
            ));
        }
    }
    let registry = TeamAiIniRegistry::from_sources(
        &IniFile::from_str(&definitions),
        &IniFile::from_str(""),
        true,
    );
    sim.install_team_ai_registry(&registry, rules)
        .expect("resolved native Team inputs install");
    let mut waypoints = HashMap::from([(
        4,
        Waypoint {
            index: 4,
            rx: int(&input["origin"][0]) as u16,
            ry: int(&input["origin"][1]) as u16,
        },
    )]);
    if let Some(origin) = input["transport_origin"].as_array() {
        waypoints.insert(
            5,
            Waypoint {
                index: 5,
                rx: int(&origin[0]) as u16,
                ry: int(&origin[1]) as u16,
            },
        );
    }
    if let Some(origin) = input["override_cell"].as_array() {
        waypoints.insert(
            6,
            Waypoint {
                index: 6,
                rx: int(&origin[0]) as u16,
                ry: int(&origin[1]) as u16,
            },
        );
    }
    // Scenario installation replaces the whole waypoint table. The native
    // Droppod prior retains both requested and transport origins at once.
    sim.session.install_waypoints(&waypoints);
    assert_eq!(
        sim.session.waypoint_cell(4),
        Some((
            int(&input["origin"][0]) as i16,
            int(&input["origin"][1]) as i16
        )),
        "{input}: supplied native reinforcement waypoint"
    );
    sim
}

/// SpawnReinforcements65DAB6 offers every Infantry to the selected building.
/// GAREFN's original refinery arm444EF8 scatters it and returns0, so65DB01
/// deletes it; the building-exit branch never records a review cell.
#[test]
fn all_infantry_reinforcement_refinery_refusal_deletes_members_and_skips_review_cell() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    assert!(rules.object("GAREFN").unwrap().refinery);
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let input = serde_json::json!({"entries": [["E1", 2]], "origin": [85, 50]});
    let mut sim = reinforcement_world(&rules, &terrain_rules, &input);
    let refinery = sim
        .spawn_object_at_height("GAREFN", "Americans", 86, 50, 4, 0, &rules)
        .expect("refinery beside the requested reinforcement origin");
    // The retail refinery's FreeUnit is already part of the world.
    let before = sim.entities().len();
    let team_type = sim.interner.get("TT").unwrap();

    let (returned, review) = sim.spawn_team_reinforcements(
        team_type,
        -1,
        &rules,
        Some(&overlay),
        FrameEffects::default(),
    );

    assert!(!returned, "no member leaves the refinery's refused exit");
    assert_eq!(review, None, "building exits bypass the recent-cell ring");
    assert_eq!(
        sim.entities().len(),
        before,
        "refused members are destroyed"
    );
    assert!(
        sim.entities()
            .values()
            .all(|entity| entity.category() != EntityCategory::Infantry)
    );
    assert!(sim.entities().get(refinery).unwrap().is_alive());
}

/// Original fresh517A50 -> refinery443C60 returned0 after one Scenario
/// RandomRanged(0,4) through51D0D0, while still limbo/unmarked. This checks
/// that bounded exit prefix; caller-owned scalar deletion is checked above.
#[test]
fn refinery_exit_scatters_a_fresh_limbo_infantryman_before_caller_cleanup() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let input = serde_json::json!({"entries": [["E1", 1]], "origin": [85, 50]});
    let mut sim = reinforcement_world(&rules, &terrain_rules, &input);
    let refinery = sim
        .spawn_object_at_height("GAREFN", "Americans", 86, 50, 4, 0, &rules)
        .unwrap();
    let member = sim
        .construct_object_limbo_at_height(
            "E1",
            "Americans",
            0,
            0,
            0,
            0,
            &rules,
            FrameEffects::default(),
        )
        .unwrap();
    let mut expected_rng = sim.scenario_rng.clone();
    expected_rng.next_range_i32_inclusive(0, 4);
    let before_main = sim.main_rng.native_state_hex();
    assert_eq!(
        crate::sim::production::exit_produced_object(
            &mut sim,
            &rules,
            refinery,
            member,
            (85, 50),
            Some(&overlay),
            FrameEffects::default(),
        ),
        crate::sim::ai_base_building::BuildingExit::Failed
    );
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        expected_rng.native_state_hex()
    );
    assert_eq!(sim.main_rng.native_state_hex(), before_main);
    let actor = sim.entities().get(member).unwrap();
    assert!(actor.lifecycle.in_limbo() && !actor.lifecycle.cell_marked());
    assert!(actor.is_alive());
}

#[test]
fn infantry_reinforcement_building_exits_release_contacts_and_skip_the_review_ring() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    // These flags are unset on these retail types. Enable the dormant
    // Hospital/Armory arms explicitly through the custom map reader.
    let mut layers = crate::rules::native_processing::RulesLayerStack::new(ini.clone());
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[CAHOSP]\nHospital=yes\n[CAPARS07]\nArmory=yes\n"),
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    for (kind, owner, clears_choice) in [
        ("GAPILE", "Americans", true),
        ("GAPOWR", "ENEMY", true),
        ("GAWEAP", "Americans", true),
        ("GAYARD", "Americans", true),
        ("CAHOSP", "ENEMY", false),
        ("CAPARS07", "ENEMY", false),
    ] {
        let producer_type = rules.object(kind).expect("retail exit receiver");
        assert_eq!(
            clears_choice,
            !producer_type.hospital && !producer_type.armory(),
            "{kind}: native4440DD bypasses economy/choice for Hospital/Armory"
        );
        let input = serde_json::json!({"entries": [["E1", 1]], "origin": [85, 50]});
        let mut sim = reinforcement_world(&rules, &terrain_rules, &input);
        if producer_type.naval {
            // Admit the actual yard on water, with the origin and its
            // foundation perimeter still on land for the Infantry exit.
            let water = terrain_rules
                .semantics_for_land_type(crate::rules::terrain_rules::LandType::Water.as_index())
                .unwrap();
            let (width, height) =
                crate::rules::foundation::foundation_dimensions(&producer_type.foundation);
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            for x in 86..86 + width as u16 {
                for y in 50..50 + height as u16 {
                    let cell = terrain.cell_mut_for_test(x, y).unwrap();
                    cell.land_type = crate::rules::terrain_rules::LandType::Water.as_index();
                    cell.base_land_type = cell.land_type;
                    cell.speed_costs = water.speed_costs.clone();
                    cell.base_speed_costs = water.speed_costs.clone();
                }
            }
            let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(terrain);
            sim.install_fixture_path_grid(Some(&path));
        }
        let building = sim
            .spawn_object_at_height(kind, owner, 86, 50, 4, 0, &rules)
            .unwrap_or_else(|| panic!("{kind}: building beside the reinforcement origin"));
        let choice = crate::sim::ai_unit_choice::UnitChoiceKind::Infantry;
        for name in ["Americans", "ENEMY"] {
            let house = sim.interner.get(name).unwrap();
            sim.houses
                .get_mut(&house)
                .unwrap()
                .ai_unit_choices_mut()
                .set(choice, 0);
        }
        let team_type = sim.interner.get("TT").unwrap();
        let (returned, review) = sim.spawn_team_reinforcements(
            team_type,
            -1,
            &rules,
            Some(&overlay),
            FrameEffects::default(),
        );
        assert!(returned, "{kind}: one successful ExitObject");
        assert_eq!(review, None, "{kind}: no ordinary placement review");
        assert_eq!(sim.entities().len(), 2, "{kind}: retained member");
        let member = sim
            .entities()
            .values()
            .find(|entity| entity.category() == EntityCategory::Infantry)
            .unwrap();
        assert!(!member.lifecycle.in_limbo(), "{kind}");
        assert!(member.radio_contacts.is_empty(), "{kind}: member BREAK");
        assert!(
            sim.entities()
                .get(building)
                .unwrap()
                .radio_contacts
                .is_empty(),
            "{kind}: producer BREAK"
        );
        let producer_owner = sim.interner.get(owner).unwrap();
        let member_owner = sim.interner.get("Americans").unwrap();
        assert_eq!(
            sim.houses[&producer_owner].ai_unit_choices().get(choice),
            if clears_choice { -1 } else { 0 },
            "{kind}: the producer's pending choice follows its exit prelude"
        );
        if producer_owner != member_owner {
            assert_eq!(
                sim.houses[&member_owner].ai_unit_choices().get(choice),
                0,
                "{kind}: the reinforcement owner's choice is retained"
            );
        }
    }
}

#[test]
fn reinforcement_exit_breaks_slot_zero_instead_of_the_new_members_contact() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    // A custom map can retain two contacts.SpawnReinforcements65D8E0 uses
    // building's Contacts[0], even when the new member entered slot1.
    let mut layers = crate::rules::native_processing::RulesLayerStack::new(ini.clone());
    layers.push(
        crate::rules::native_processing::RulesLayerKind::Scenario,
        IniFile::from_str("[GAPILE]\nNumberOfDocks=2\n"),
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let rules = RuleSet::from_processed_rules(&processed).unwrap();
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let input = serde_json::json!({"entries": [["E1", 2]], "origin": [85, 50]});
    let mut sim = reinforcement_world(&rules, &terrain_rules, &input);
    let building = sim
        .spawn_object_at_height("GAPILE", "Americans", 86, 50, 4, 0, &rules)
        .unwrap();
    let old_contact = sim
        .spawn_object_at_height("E1", "Americans", 90, 55, 4, 0, &rules)
        .unwrap();
    assert_eq!(
        crate::sim::radio::transmit(
            &mut sim,
            building,
            old_contact,
            crate::sim::radio::RadioMessage::Hello,
            crate::sim::radio::RadioPayload::default(),
            &rules,
            FrameEffects::default(),
        ),
        crate::sim::radio::RadioResponse::Roger
    );
    let team_type = sim.interner.get("TT").unwrap();
    assert_eq!(
        sim.spawn_team_reinforcements(
            team_type,
            -1,
            &rules,
            Some(&overlay),
            FrameEffects::default(),
        ),
        (true, None)
    );
    let producer = sim.entities().get(building).unwrap();
    assert_eq!(producer.radio_contacts.slot(0), None);
    let retained = producer
        .radio_contacts
        .slot(1)
        .expect("the first new member retains its slot1 contact");
    assert_ne!(retained, old_contact);
    assert_eq!(
        sim.entities().get(retained).unwrap().radio_contacts.slot(0),
        Some(building)
    );
    assert!(
        sim.entities()
            .get(old_contact)
            .unwrap()
            .radio_contacts
            .is_empty()
    );
    let members: Vec<_> = sim
        .entities()
        .values()
        .filter(|entity| {
            entity.category() == EntityCategory::Infantry && entity.stable_id() != old_contact
        })
        .collect();
    assert_eq!(members.len(), 2);
    assert!(members.iter().all(|member| !member.lifecycle.in_limbo()));
    assert!(
        members
            .iter()
            .filter(|member| member.stable_id() != retained)
            .all(|member| member.radio_contacts.is_empty())
    );
}

#[test]
fn mixed_reinforcements_bypass_the_building_exit_branch() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let input = serde_json::json!({"entries": [["MTNK", 1], ["E1", 1]], "origin": [85, 50]});
    let mut sim = reinforcement_world(&rules, &terrain_rules, &input);
    sim.spawn_object_at_height("GAREFN", "Americans", 86, 50, 4, 0, &rules)
        .unwrap();
    let before = sim.entities().len();
    let team_type = sim.interner.get("TT").unwrap();
    let (returned, review) = sim.spawn_team_reinforcements(
        team_type,
        -1,
        &rules,
        Some(&overlay),
        FrameEffects::default(),
    );
    assert!(returned);
    assert_eq!(review, Some((85, 50)));
    assert_eq!(
        sim.entities().len(),
        before + 2,
        "mixed members use ordinary placement"
    );
}

#[test]
fn reinforcement_receiver_is_the_last_healthy_building_with_a_free_exit() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    for west_healthy in [true, false] {
        let input = serde_json::json!({"entries": [["E1", 1]], "origin": [85, 50]});
        let mut sim = reinforcement_world(&rules, &terrain_rules, &input);
        sim.spawn_object_at_height("GAPOWR", "ENEMY", 86, 50, 4, 0, &rules)
            .unwrap();
        let west = sim
            .spawn_object_at_height("GAPILL", "Americans", 84, 50, 4, 0, &rules)
            .unwrap();
        if !west_healthy {
            sim.substrate.entities.get_mut(west).unwrap().health.set(0);
        }
        let american = sim.interner.get("Americans").unwrap();
        let enemy = sim.interner.get("ENEMY").unwrap();
        let choice = crate::sim::ai_unit_choice::UnitChoiceKind::Infantry;
        for house in [american, enemy] {
            sim.houses
                .get_mut(&house)
                .unwrap()
                .ai_unit_choices_mut()
                .set(choice, 0);
        }
        let team_type = sim.interner.get("TT").unwrap();
        let (returned, review) = sim.spawn_team_reinforcements(
            team_type,
            -1,
            &rules,
            Some(&overlay),
            FrameEffects::default(),
        );
        assert!(returned);
        assert_eq!(review, None);
        //65DA6C replaces the candidate on every match. West is visited
        //after the eastern power plant; a nonpositive Health skips it.
        let selected = if west_healthy { american } else { enemy };
        let skipped = if west_healthy { enemy } else { american };
        assert_eq!(sim.houses[&selected].ai_unit_choices().get(choice), -1);
        assert_eq!(sim.houses[&skipped].ai_unit_choices().get(choice), 0);
    }
}

#[test]
fn reinforcement_busy_exit_deletes_the_member_without_breaking_the_existing_contact() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let input = serde_json::json!({"entries": [["E1", 1]], "origin": [85, 50]});
    let mut sim = reinforcement_world(&rules, &terrain_rules, &input);
    let building = sim
        .spawn_object_at_height("GAPOWR", "Americans", 86, 50, 4, 0, &rules)
        .unwrap();
    let contact = sim
        .spawn_object_at_height("E1", "Americans", 90, 60, 4, 0, &rules)
        .unwrap();
    let _ = crate::sim::radio::transmit(
        &mut sim,
        building,
        contact,
        crate::sim::radio::RadioMessage::Hello,
        crate::sim::radio::RadioPayload::default(),
        &rules,
        FrameEffects::default(),
    );
    let before = sim.entities().len();
    let producer_contacts = sim.entities().get(building).unwrap().radio_contacts.clone();
    let member_contacts = sim.entities().get(contact).unwrap().radio_contacts.clone();
    assert!(producer_contacts.first_free().is_none());
    let team_type = sim.interner.get("TT").unwrap();
    let (returned, review) = sim.spawn_team_reinforcements(
        team_type,
        -1,
        &rules,
        Some(&overlay),
        FrameEffects::default(),
    );
    assert!(
        !returned,
        "ExitObject1 is not a retained factory retry here"
    );
    assert_eq!(review, None);
    assert_eq!(sim.entities().len(), before);
    assert_eq!(
        sim.entities().get(building).unwrap().radio_contacts,
        producer_contacts
    );
    assert_eq!(
        sim.entities().get(contact).unwrap().radio_contacts,
        member_contacts
    );
}

#[test]
fn a_building_without_a_clear_foundation_exit_keeps_ordinary_reinforcement_placement() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let input = serde_json::json!({"entries": [["E1", 1]], "origin": [85, 50]});
    let mut sim = reinforcement_world(&rules, &terrain_rules, &input);
    sim.spawn_object_at_height("GAPILL", "Americans", 86, 50, 4, 0, &rules)
        .unwrap();
    for dx in -1_i32..=1 {
        for dy in -1_i32..=1 {
            if (dx, dy) != (0, 0) {
                sim.spawn_object_at_height(
                    "E1",
                    "Americans",
                    (86 + dx) as u16,
                    (50 + dy) as u16,
                    4,
                    0,
                    &rules,
                )
                .unwrap();
            }
        }
    }
    let before = sim.entities().len();
    let team_type = sim.interner.get("TT").unwrap();
    let (returned, review) = sim.spawn_team_reinforcements(
        team_type,
        -1,
        &rules,
        Some(&overlay),
        FrameEffects::default(),
    );
    assert!(returned);
    assert_eq!(review, Some((85, 50)), "no qualifying building receiver");
    assert_eq!(sim.entities().len(), before + 1);
}

#[test]
fn droppod_reinforcements_bypass_a_nearby_refinery() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/team_paradrop_transport.json",
    ))
    .unwrap();
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    for row in corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["case_kind"] == "droppod_transport")
    {
        let input = &row["input"];
        let mut sim = reinforcement_world(&rules, &terrain_rules, input);
        let origin = (
            int(&input["origin"][0]) as u16,
            int(&input["origin"][1]) as u16,
        );
        sim.spawn_object_at_height("GAREFN", "Americans", origin.0 + 1, origin.1, 4, 0, &rules)
            .unwrap();
        let team_type = sim.interner.get("TT").unwrap();
        assert!(
            sim.spawn_team_reinforcements(
                team_type,
                -1,
                &rules,
                Some(&overlay),
                FrameEffects::default(),
            )
            .0
        );
        assert_eq!(
            sim.entities()
                .values()
                .filter(|entity| entity.category() == EntityCategory::Aircraft)
                .count(),
            1
        );
        let members: Vec<_> = sim
            .entities()
            .values()
            .filter(|entity| entity.category() == EntityCategory::Infantry)
            .collect();
        assert_eq!(members.len(), 2);
        assert!(members.iter().all(|member| member.lifecycle.in_limbo()));
    }
}

#[test]
fn droppod_reinforcement_carrier_matches_original_constructor_cargo_and_orders() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/team_paradrop_transport.json",
    ))
    .unwrap();
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let rows: Vec<_> = corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["case_kind"] == "droppod_transport")
        .collect();
    assert_eq!(
        rows.len(),
        2,
        "selected original waypoint/House-edge controls"
    );
    for row in rows {
        let input = &row["input"];
        let mut sim = reinforcement_world(&rules, &terrain_rules, input);
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(int(
            &row["id_before"],
        )
            as u32));
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                .into();
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
        let team_type = sim.interner.get("TT").unwrap();
        let (returned, review) = sim.spawn_team_reinforcements(
            team_type,
            -1,
            &rules,
            Some(&overlay),
            FrameEffects::default(),
        );
        assert_eq!(i32::from(returned), int(&row["returned"]), "{input}");
        let created: Vec<_> = sim
            .entities()
            .values()
            .map(|entity| entity.native_unique_id())
            .collect();
        let expected = |value: &Value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(int)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            created,
            expected(&row["created_ids"]),
            "{input}: members precede carrier"
        );
        let plane = sim
            .entities()
            .values()
            .find(|entity| entity.native_unique_id() == int(&row["plane"]["native_id"]))
            .unwrap();
        let xyz = crate::sim::movement::ground_pose::object_location(
            plane,
            sim.resolved_terrain.as_ref(),
        );
        assert_eq!(
            vec![xyz.x, xyz.y, xyz.z],
            expected(&row["plane"]["xyz"]),
            "{input}"
        );
        assert_eq!(
            plane.mission.current().raw(),
            int(&row["plane"]["mission"]),
            "{input}"
        );
        assert_eq!(
            plane.mission.queued().raw(),
            int(&row["plane"]["queued"]),
            "{input}"
        );
        assert_eq!(
            plane.lifecycle.in_limbo(),
            row["plane"]["limbo"].as_bool().unwrap(),
            "{input}"
        );
        assert_eq!(
            plane.is_mission_only(),
            row["plane"]["mission_only"].as_bool().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.team_script_vm
                .team_for_member(plane.stable_id())
                .is_some(),
            row["plane"]["team_present"].as_bool().unwrap(),
            "{input}"
        );
        assert_eq!(
            plane.navigation.nav_com().is_some(),
            row["plane"]["nav_present"].as_bool().unwrap(),
            "{input}"
        );
        let native_id = |id| sim.entities().get(id).unwrap().native_unique_id();
        assert_eq!(
            plane.attack_target.target().unwrap(),
            crate::sim::combat::TargetKind::Cell(
                int(&row["plane"]["target_cell"][0]) as u16,
                int(&row["plane"]["target_cell"][1]) as u16
            ),
            "{input}"
        );
        assert_eq!(
            plane
                .passenger_role
                .cargo()
                .unwrap()
                .passengers()
                .iter()
                .copied()
                .map(native_id)
                .collect::<Vec<_>>(),
            expected(&row["plane"]["cargo"]),
            "{input}"
        );
        let team = sim.team_script_vm.teams.values().next().unwrap();
        assert_eq!(
            team.members().map(native_id).collect::<Vec<_>>(),
            expected(&row["members"]),
            "{input}"
        );
        let limbo: Vec<_> = sim
            .entities()
            .values()
            .filter(|entity| entity.category() == EntityCategory::Infantry)
            .map(|entity| entity.lifecycle.in_limbo())
            .collect();
        assert_eq!(
            limbo,
            row["member_limbo"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_bool().unwrap())
                .collect::<Vec<_>>(),
            "{input}"
        );
        assert_eq!(
            review,
            (int(&row["recent_counter"]) != 0).then(|| (
                int(&row["recent_cell"][0]) as i16,
                int(&row["recent_cell"][1]) as i16
            )),
            "{input}"
        );
        assert!(!sim.object_placement_scope_active(), "{input}");
        assert_eq!(
            sim.native_identity_cursor(),
            Some(int(&row["id_after"]) as u32),
            "{input}"
        );
        for stream in ["scenario", "main", "mapgen"] {
            let actual = match stream {
                "scenario" => sim.scenario_rng.native_state_hex(),
                "main" => sim.main_rng.native_state_hex(),
                _ => sim.mapgen_rng.native_state_hex(),
            };
            assert_eq!(
                actual,
                row["rng_after"][stream].as_str().unwrap(),
                "{input}"
            );
        }
    }
}

#[test]
fn transport_origin_fields_match_original_retained_reads_and_survive_save_hash() {
    use std::hash::Hasher;
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/team_paradrop_transport.json",
    ))
    .unwrap();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
    ))
    .unwrap();
    let rows: Vec<_> = corpus
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["case_kind"] == "transport_input")
        .collect();
    assert_eq!(rows.len(), 4, "native retained input controls");
    for row in rows {
        let before = &row["before"];
        let mut fixed = format!(
            "[TeamTypes]\n0=TT\n[TT]\nScript=S\nTaskForce=F\nUseTransportOrigin={}\n",
            if before["use_transport_origin"].as_bool().unwrap() {
                "yes"
            } else {
                "no"
            }
        );
        if int(&before["transport_waypoint"]) != -1 {
            fixed.push_str(&format!(
                "TransportWaypoint={}\n",
                before["letters"].as_str().unwrap()
            ));
        }
        fixed.push_str("[ScriptTypes]\n0=S\n[S]\n0=5,0\n[TaskForces]\n0=F\n[F]\n0=1,E1\n");
        let mut map = "[TeamTypes]\n0=TT\n[TT]\nName=retained\n".to_string();
        for (key, value) in row["fields"].as_object().unwrap() {
            map.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
        let registry = TeamAiIniRegistry::from_sources(
            &IniFile::from_str(&fixed),
            &IniFile::from_str(&map),
            false,
        );
        let mut interner = crate::sim::intern::StringInterner::new();
        let (mut vm, diagnostics) =
            TeamScriptVm::from_ini_registry(&registry, &mut interner, &rules);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let id = interner.get("TT").unwrap();
        let metadata = &vm.team_type_ini[&id];
        assert_eq!(
            metadata.use_transport_origin,
            row["after"]["use_transport_origin"].as_bool().unwrap(),
            "{row}"
        );
        assert_eq!(
            metadata.transport_waypoint,
            int(&row["after"]["transport_waypoint"]),
            "{row}"
        );
        assert_eq!(
            row["rng_before"], row["rng_after"],
            "native input reads draw nothing"
        );
        let owner = interner.intern("OWNER");
        vm.construct_team(id, owner, false, 0, false, None).unwrap();
        let restored: TeamScriptVm =
            save_codec::deserialize(&save_codec::serialize(&vm).unwrap()).unwrap();
        assert_eq!(restored.team_type_ini[&id], vm.team_type_ini[&id]);
        let hash = |vm: &TeamScriptVm| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            vm.hash_state(0, &mut hasher);
            hasher.finish()
        };
        assert_eq!(hash(&restored), hash(&vm));
        let metadata = vm.team_type_ini.get_mut(&id).unwrap();
        metadata.use_transport_origin = !metadata.use_transport_origin;
        assert_ne!(
            hash(&restored),
            hash(&vm),
            "active input gate survives state hashing"
        );
    }
}

#[test]
fn all01_full_transport_inputs_fit_the_selected_retail_cargo_chain() {
    use crate::rules::process_owner::NativeScenarioRulesPrefix;
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let bytes = assets.get_ref("ALL01UMD.MAP").expect("retail ALL01 map");
    assert_eq!(
        crate::util::sha256::sha256_hex(bytes),
        "dee38769f2247a85908705486c175ef14a4cf90437899defe7c8b4c0d1c51fb0",
        "physical selected mission data"
    );
    let map = crate::map::map_file::MapFile::from_bytes(bytes).unwrap();
    let named = assets.resolve_ref("ALL01UMD.INI").map(|_| {
        crate::rules::retail_sources::select_ini(&assets, "ALL01UMD.INI")
            .expect("present campaign prepass parses")
            .ini
    });
    let mut owner = crate::rules::retail_ini_fixture::retail_rules_owner(&assets);
    let (rules, _, _, _) = owner
        .load_scenario(
            NativeScenarioRulesPrefix::Campaign(named.as_ref()),
            &map.ini,
        )
        .unwrap()
        .into_parts();
    let aimd = crate::rules::retail_sources::select_ini(&assets, "AIMD.INI")
        .expect("retail fixed team definitions");
    let registry = TeamAiIniRegistry::from_sources(&aimd.ini, &map.ini, false);
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&registry, &rules).unwrap();
    // Literal identities/counts below come from the bound physical map,
    // not from a replicated native transport-selection implementation.
    for (name, expected) in [
        ("0A2BD90C", vec![("LTNK", 1), ("INIT", 3), ("YHVR", 1)]),
        ("0A80332C", vec![("LTNK", 4), ("YHVR", 1)]),
        ("0A7F16AC", vec![("LTNK", 3), ("INIT", 2), ("YHVR", 1)]),
    ] {
        let team_type = sim.interner.get(name).unwrap();
        let metadata = &sim.team_script_vm.team_type_ini[&team_type];
        assert!(metadata.full && !metadata.droppod && metadata.transports_return_on_unload);
        let definition = &sim.team_script_vm.team_types[&team_type];
        let entries = &sim.team_script_vm.task_forces[&definition.task_force_id].entries;
        assert_eq!(
            entries
                .iter()
                .map(|entry| (sim.interner.resolve(entry.member_type.id), entry.count))
                .collect::<Vec<_>>(),
            expected,
            "{name} resolved physical TaskForce"
        );
        for entry in entries {
            let object = rules
                .object_in_category(
                    entry.member_type.category,
                    sim.interner.resolve(entry.member_type.id),
                )
                .unwrap();
            assert_eq!(
                object.storage, 0,
                "{name}: Full Storage fill is outside this selected chain"
            );
        }
        assert!(
            sim.team_script_vm.scripts[&definition.script_id]
                .actions
                .iter()
                .any(|action| action.action_id == 8 && action.argument == 2)
        );
    }
    let carrier = rules.object("YHVR").unwrap();
    assert!(
        carrier.naval && carrier.passengers > 0,
        "retail hover-carrier data reaches the selected Full branch"
    );
    for (name, expected, transport_waypoint) in [
        ("0A2BC12C", vec![("INIT", 8)], 9),
        ("0D2AF91C", vec![("INIT", 5)], 86),
        ("0908B07C", vec![("ENGINEER", 4)], 143),
    ] {
        let team_type = sim.interner.get(name).unwrap();
        let metadata = &sim.team_script_vm.team_type_ini[&team_type];
        assert!(metadata.droppod && !metadata.full && metadata.use_transport_origin);
        assert_eq!(
            metadata.transport_waypoint, transport_waypoint,
            "{name}: physical TransportWaypoint letters"
        );
        let definition = &sim.team_script_vm.team_types[&team_type];
        assert_eq!(
            sim.team_script_vm.task_forces[&definition.task_force_id]
                .entries
                .iter()
                .map(|entry| (sim.interner.resolve(entry.member_type.id), entry.count))
                .collect::<Vec<_>>(),
            expected,
            "{name}"
        );
    }
    let plane = rules
        .object_in_category(
            crate::rules::object_type::ObjectCategory::Aircraft,
            crate::sim::superweapon::paradrop::PDPLANE,
        )
        .unwrap();
    assert!(
        !plane.selectable,
        "native payload-latch fire/selection residual is dormant on this retail carrier"
    );
}

#[test]
fn full_reinforcement_members_cargo_and_scalar_cleanup_match_original() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/team_full_transport.json",
    ))
    .unwrap();
    let Some((mut ini, art)) = retail_rules_and_art() else {
        return;
    };
    // This is the declared native control prior, not a retail MTNK/YHVR
    // capability claim. Both sides retain the physical MTNK/E1 constructors;
    // only Passengers is supplied to reach the Full cargo branch.
    ini.merge(&IniFile::from_str("[MTNK]\nPassengers=16\n"));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let rows = corpus.as_array().unwrap();
    assert_eq!(rows.len(), 3, "selected original Full controls");
    for row in rows {
        let input = &row["input"];
        let mut sim = reinforcement_world(&rules, &terrain_rules, input);
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(int(
            &row["id_before"],
        )
            as u32));
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                .into();
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
        let team_type = sim.interner.get("TT").unwrap();
        let placement_members = sim
            .create_reinforcement_members(
                team_type,
                &rules,
                Some(&overlay),
                FrameEffects::default(),
            )
            .expect("original Full constructor result");
        let created: Vec<_> = sim
            .entities()
            .values()
            .map(|entity| (entity.stable_id(), entity.native_unique_id()))
            .collect();
        let native_ids = |ids: &[u64]| {
            ids.iter()
                .map(|id| created.iter().find(|(stable, _)| stable == id).unwrap().1)
                .collect::<Vec<_>>()
        };
        let values = |value: &Value| {
            value
                .as_array()
                .unwrap()
                .iter()
                .map(int)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            created
                .iter()
                .map(|(_, native)| *native)
                .collect::<Vec<_>>(),
            values(&row["created_ids"]),
            "{input} construction order"
        );
        assert_eq!(
            placement_members.len(),
            1,
            "{input} places only its carrier chain"
        );
        let head = placement_members[0];
        assert_eq!(native_ids(&[head]), vec![int(&row["head_id"])]);
        let cargo = sim
            .entities()
            .get(head)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap();
        assert_eq!(
            native_ids(cargo.passengers()),
            values(&row["cargo_before"]),
            "{input}"
        );
        let team = *sim.team_script_vm.teams.keys().next().unwrap();
        let team_members: Vec<_> = sim.team_script_vm.teams[&team].members().collect();
        assert_eq!(
            native_ids(&team_members),
            values(&row["members_before"]),
            "{input}"
        );
        for stream in ["scenario", "main", "mapgen"] {
            let actual = match stream {
                "scenario" => sim.scenario_rng.native_state_hex(),
                "main" => sim.main_rng.native_state_hex(),
                _ => sim.mapgen_rng.native_state_hex(),
            };
            assert_eq!(
                actual,
                row["rng_after_creation"][stream].as_str().unwrap(),
                "{input}"
            );
        }
        match input["stage"].as_str().unwrap() {
            "members" => {}
            "null_placement" => {
                let owner = sim.interner.get("Americans").unwrap();
                let placed = sim.place_reinforcement_members(
                    &placement_members,
                    owner,
                    (0, 0),
                    false,
                    &rules,
                    Some(&overlay),
                    FrameEffects::default(),
                );
                assert_eq!(i32::from(placed), int(&row["returned"]), "{input}");
            }
            "scalar_scope1" => {
                sim.with_object_placement_scope(|sim| {
                    assert!(sim.object_placement_scope_active());
                    assert!(sim.discard_constructed_limbo(head, &rules, FrameEffects::default()));
                });
                // Native row supplies the retained1 prior. The Rust caller's
                // existing RAII scope restores its own prior after the call.
            }
            stage => panic!("unknown original Full control {stage}"),
        }
        assert!(
            !sim.object_placement_scope_active(),
            "{input}: no leaked caller scope"
        );
        assert_eq!(
            created
                .iter()
                .map(|(id, _)| sim.entities().contains(*id))
                .collect::<Vec<_>>(),
            row["created_present"]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_bool().unwrap())
                .collect::<Vec<_>>(),
            "{input} scalar ownership"
        );
        let remaining: Vec<_> = sim
            .team_script_vm
            .teams
            .get(&team)
            .map(|state| state.members().collect())
            .unwrap_or_default();
        assert_eq!(
            native_ids(&remaining),
            values(&row["members_after"]),
            "{input}"
        );
        assert_eq!(
            sim.native_identity_cursor(),
            Some(int(&row["id_after"]) as u32),
            "{input}"
        );
        for stream in ["scenario", "main", "mapgen"] {
            let actual = match stream {
                "scenario" => sim.scenario_rng.native_state_hex(),
                "main" => sim.main_rng.native_state_hex(),
                _ => sim.mapgen_rng.native_state_hex(),
            };
            assert_eq!(
                actual,
                row["rng_after"][stream].as_str().unwrap(),
                "{input}"
            );
        }
    }
}

#[test]
fn action7_real_members_match_original_identity_rng_placement_and_cleanup() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    for row in rows("reinforcement", 10) {
        let input = &row["input"];
        let mut sim = reinforcement_world(&rules, &terrain_rules, input);
        sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(int(
            &row["id_before"],
        )
            as u32));
        sim.scenario_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
        sim.main_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                .into();
        sim.mapgen_rng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
        let team_type = sim.interner.get("TT").unwrap();
        let (spawned, review) = sim.spawn_team_reinforcements(
            team_type,
            -1,
            &rules,
            Some(&overlay),
            FrameEffects::default(),
        );
        assert_eq!(spawned, int(&row["returned"]) != 0, "{input}");
        let expected_review = (int(&row["recent_counter"]) != 0).then(|| {
            (
                int(&row["recent_cell"][0]) as i16,
                int(&row["recent_cell"][1]) as i16,
            )
        });
        assert_eq!(review, expected_review, "{input}");
        let team = sim.team_script_vm.teams.values().next().unwrap();
        let expected = row["members"].as_array().unwrap();
        assert_eq!(team.members.len(), expected.len(), "{input}");
        for (member, native) in team.members.iter().zip(expected) {
            let entity = sim.substrate.entities.get(member.id).unwrap();
            assert_eq!(
                entity.native_unique_id(),
                int(&native["native_id"]),
                "{input}"
            );
            assert_eq!(
                sim.interner.resolve(entity.type_ref()),
                native["type"].as_str().unwrap(),
                "{input}"
            );
            assert_eq!(
                entity.lifecycle.in_limbo(),
                int(&native["limbo"]) != 0,
                "{input}"
            );
            assert_eq!(
                entity.lifecycle.object_alive(),
                int(&native["alive"]) != 0,
                "{input}"
            );
            assert_eq!(member.initiated, int(&native["initiated"]) != 0, "{input}");
            let coord = position_world_coord(&entity.position);
            assert_eq!(
                [coord.x, coord.y, coord.z],
                [
                    int(&native["xyz"][0]),
                    int(&native["xyz"][1]),
                    int(&native["xyz"][2])
                ],
                "{input}"
            );
            assert_eq!(
                entity
                    .veterancy
                    .raw()
                    .bits()
                    .to_le_bytes()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>(),
                native["veterancy"].as_str().unwrap(),
                "{input}"
            );
            assert_eq!(
                entity.mission.current().raw(),
                int(&native["mission"]),
                "{input}"
            );
            assert_eq!(
                entity.mission.queued().raw(),
                int(&native["queued"]),
                "{input}"
            );
            let constructed = row["constructed"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["native_id"] == native["native_id"])
                .unwrap();
            assert_eq!(
                format!(
                    "{:02x}{:02x}",
                    entity.techno_ctor_random_word() as u8,
                    (entity.techno_ctor_random_word() >> 8) as u8
                ),
                constructed["phase"].as_str().unwrap(),
                "{input}"
            );
        }
        assert_eq!(
            sim.entities().len(),
            expected.len(),
            "{input}: no fake or retained rejected members"
        );
        assert_eq!(
            sim.native_identity_cursor(),
            Some(int(&row["id_after"]) as u32),
            "{input}"
        );
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"]["scenario"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.main_rng.native_state_hex(),
            row["rng_after"]["main"].as_str().unwrap(),
            "{input}"
        );
        assert_eq!(
            sim.mapgen_rng.native_state_hex(),
            row["rng_after"]["mapgen"].as_str().unwrap(),
            "{input}"
        );
    }
}

#[test]
fn shared_leader_matches_original_script3_signed_ratings_and_live_joined_gates() {
    for row in rows("script_move", 10) {
        let input = &row["input"];
        assert_eq!(row["caller"], "0x006E9140");
        assert_eq!(row["dispatch"]["advance"], false);
        assert_eq!(row["dispatch"]["cursor"], 0);
        assert_eq!(
            row["dispatch"]["first"],
            input["first"].as_bool().unwrap_or(true)
        );
        assert_eq!(
            row["rng_before"], row["rng_after"],
            "{input}: original full TeamAI, Script3 and CoordinateMove RNG"
        );
        if input["first"].as_bool() == Some(false) {
            assert!(row["leader"].is_null());
            assert_eq!(row["calls"], serde_json::json!(["0x6ebad0"]));
            continue;
        }
        let prior = row["prior_members"].as_array().unwrap();
        let rules=RuleSet::from_ini(&IniFile::from_str(&format!(
            "[InfantryTypes]\n0=E1\n[VehicleTypes]\n0=MTNK\n[E1]\nStrength=100\nLeadershipRating={}\n[MTNK]\nStrength=100\nLeadershipRating={}\n",
            int(&prior[0]["rating"]),int(&prior[1]["rating"]),
        ))).unwrap();
        let mut sim = Simulation::new();
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("OWNER");
        let script = sim.interner.intern("SCRIPT");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::Scenario,
            actions: vec![TeamScriptAction {
                action_id: 3,
                argument: 4,
            }],
        });
        // This replay boundary is the declared prior member list used by
        // original6EC7D0; it does not manufacture a campaign reinforcement.
        let members: Vec<_> = prior
            .iter()
            .map(|state| {
                let id = sim
                    .construct_object_limbo_at_height(
                        state["type"].as_str().unwrap(),
                        "OWNER",
                        64,
                        64,
                        0,
                        0,
                        &rules,
                        crate::sim::world::FrameEffects::default(),
                    )
                    .unwrap();
                let entity = sim.substrate.entities.get_mut(id).unwrap();
                entity.set_native_unique_id_for_test(int(&state["native_id"]));
                entity
                    .lifecycle
                    .set_object_alive_for_test(int(&state["alive"]) != 0);
                entity
                    .lifecycle
                    .set_in_limbo_for_test(int(&state["limbo"]) != 0);
                entity.health.set_for_test(int(&state["health"]));
                id
            })
            .collect();
        let team = sim
            .team_script_vm
            .create_team(owner, script, members.clone(), 0);
        for (member, state) in sim
            .team_script_vm
            .teams
            .get_mut(&team)
            .unwrap()
            .members
            .iter_mut()
            .zip(prior)
        {
            member.initiated = int(&state["initiated"]) != 0;
        }
        let leader = if input["depth"].as_i64().unwrap_or(0) != 0 {
            sim.with_object_placement_scope(|sim| sim.team_leader(team, &rules))
        } else {
            sim.team_leader(team, &rules)
        }
        .unwrap();
        assert_eq!(
            sim.entities().get(leader).unwrap().native_unique_id(),
            int(&row["leader"]),
            "{input}"
        );
    }
}

#[test]
fn script3_real_members_match_original_outside_move_and_rng_continuation() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let row = rows("script_move", 10)
        .into_iter()
        .find(|row| row["input"]["name"] == "script3_outside")
        .unwrap();
    let spawn = serde_json::json!({"entries":[["MTNK",1],["E1",1]],"origin":[87,50]});
    let mut sim = reinforcement_world(&rules, &terrain_rules, &spawn);
    let first_id = row["prior_members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| int(&member["native_id"]))
        .min()
        .unwrap();
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(
        first_id.wrapping_sub(1) as u32,
    ));
    let team_type = sim.interner.get("TT").unwrap();
    assert!(
        sim.spawn_team_reinforcements(
            team_type,
            -1,
            &rules,
            Some(&overlay),
            FrameEffects::default()
        )
        .0
    );
    let team = *sim.team_script_vm.teams.keys().next().unwrap();
    for (member, prior) in sim.team_script_vm.teams[&team]
        .members
        .iter()
        .zip(row["prior_members"].as_array().unwrap())
    {
        let entity = sim.entities().get(member.id).unwrap();
        assert_eq!(entity.native_unique_id(), int(&prior["native_id"]));
        assert_eq!(
            sim.object_type(entity.type_ref(), &rules)
                .unwrap()
                .leadership_rating,
            int(&prior["rating"])
        );
    }
    // Original6E9140 enters with the genuine action7 constructor flags and
    // Script cursor, then recalc/form/advance-clear admit Script3. Replay the
    // same shared production TeamAI owner, rather than supplying first=true
    // to a handler whose constructor advance flag is still set.
    assert_eq!(row["constructor_entry"], true);
    assert!(row["preparation"].is_null());
    let native_prior = &row["caller_prior"];
    let prior = &sim.team_script_vm.teams[&team];
    assert_eq!(prior.formed, native_prior["formed"].as_bool().unwrap());
    assert_eq!(prior.altered, native_prior["altered"].as_bool().unwrap());
    assert_eq!(
        prior.advance_pending,
        native_prior["advance"].as_bool().unwrap()
    );
    assert_eq!(prior.cursor, int(&native_prior["cursor"]));
    waypoint(&mut sim, 4, (127, 127));
    sim.scenario_rng =
        SimRng::from_native_state_hex_for_test(row["rng_before"]["scenario"].as_str().unwrap());
    sim.main_rng =
        SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap()).into();
    sim.mapgen_rng =
        SimRng::from_native_state_hex_for_test(row["rng_before"]["mapgen"].as_str().unwrap());
    sim.run_team_ai_pass(&rules, Some(&overlay), FrameEffects::default());
    assert_eq!(
        sim.team_script_vm.teams[&team].formed,
        row["dispatch"]["formed"].as_bool().unwrap()
    );
    assert_eq!(
        sim.team_script_vm.teams[&team].altered,
        row["dispatch"]["altered"].as_bool().unwrap()
    );
    assert_eq!(
        sim.team_script_vm.teams[&team].cursor,
        int(&row["dispatch"]["cursor"])
    );
    assert_eq!(
        sim.team_script_vm.teams[&team].mission_target,
        Some(TeamTarget::Cell {
            x: int(&row["mission_target"][0]) as i16,
            y: int(&row["mission_target"][1]) as i16,
        })
    );
    assert_eq!(
        sim.team_script_vm.teams[&team].advance_pending,
        row["advance"].as_bool().unwrap()
    );
    for (member, native) in sim.team_script_vm.teams[&team]
        .members
        .iter()
        .zip(row["members_after"].as_array().unwrap())
    {
        let entity = sim.entities().get(member.id).unwrap();
        assert_eq!(entity.native_unique_id(), int(&native["native_id"]));
        assert_eq!(entity.mission.current().raw(), int(&native["mission"]));
        assert_eq!(entity.mission.queued().raw(), int(&native["queued"]));
        assert_eq!(
            entity.navigation.nav_com(),
            Some(crate::sim::components::NavTargetRef::Cell {
                rx: int(&native["nav_cell"][0]) as u16,
                ry: int(&native["nav_cell"][1]) as u16,
            })
        );
    }
    assert_eq!(
        sim.scenario_rng.native_state_hex(),
        row["rng_after"]["scenario"].as_str().unwrap()
    );
    assert_eq!(
        sim.main_rng.native_state_hex(),
        row["rng_after"]["main"].as_str().unwrap()
    );
    assert_eq!(
        sim.mapgen_rng.native_state_hex(),
        row["rng_after"]["mapgen"].as_str().unwrap()
    );
}

#[test]
fn action4_recruits_real_released_members_then_script50_and_cleanup_share_owners() {
    let Some((ini, art)) = retail_rules_and_art() else {
        return;
    };
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail rules");
    let overlay = OverlayTypeRegistry::from_ini(&ini, Some(&art));
    let terrain_rules = TerrainRules::from_ini(&ini);
    let native_flash = rows("elite_flash", 21)
        .into_iter()
        .find(|row| {
            row["input"]["argument"] == 100
                && row["input"]["first"] == true
                && row["input"]["count"] == 3
        })
        .unwrap();
    let input = serde_json::json!({"entries":[["MTNK",2]],"origin":[87,50],
        "max":0,"script":format!("50,{}",int(&native_flash["input"]["argument"]))});
    let mut sim = reinforcement_world(&rules, &terrain_rules, &input);
    let team_type = sim.interner.get("TT").unwrap();
    assert!(
        sim.spawn_team_reinforcements(
            team_type,
            -1,
            &rules,
            Some(&overlay),
            FrameEffects::default()
        )
        .0
    );
    let reinforcement = *sim.team_script_vm.teams.keys().next().unwrap();
    let mut original_members: Vec<_> = sim.team_script_vm.teams[&reinforcement].members().collect();
    original_members.sort_unstable();
    sim.destroy_team(reinforcement, &rules, FrameEffects::default());
    let rng_before = [
        sim.scenario_rng.native_state_hex(),
        sim.main_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ];
    assert!(
        sim.create_team(team_type, None, &rules).is_none(),
        "Max=0 rejects ordinary CreateTeam"
    );
    let team = sim
        .trigger_create_team(team_type, &rules, Some(&overlay), FrameEffects::default())
        .expect("original Trigger4 creation scope bypasses Max=0");
    assert!(
        !sim.object_placement_scope_active(),
        "Trigger4 balances its shared scope"
    );
    // This is a production integration regression. Constructor/Max,
    // recruitment and Script50 goldens each have their own native corpora;
    // it does not claim an original execution of this artificial whole chain.
    for _ in 0..8 {
        sim.run_team_ai_pass(&rules, Some(&overlay), FrameEffects::default());
        if sim
            .team_script_vm
            .teams
            .get(&team)
            .is_some_and(|team| team.cursor == 0 && team.advance_pending)
        {
            break;
        }
    }
    let state = sim
        .team_script_vm
        .teams
        .get(&team)
        .expect("the flash action ran before the script end");
    assert!(state.formed && state.cursor == 0 && state.advance_pending);
    let mut recruited: Vec<_> = state.members().collect();
    recruited.sort_unstable();
    assert_eq!(
        recruited, original_members,
        "action4 must recruit existing live objects"
    );
    assert_eq!(
        sim.entities().len(),
        original_members.len(),
        "action4 has no manufactured members"
    );
    for &id in &original_members {
        assert_eq!(
            sim.entities().get(id).unwrap().flashing.frames(),
            int(&native_flash["stored"][0])
        );
    }
    sim.run_team_ai_pass(&rules, Some(&overlay), FrameEffects::default());
    assert!(
        sim.team_script_vm.team(team).is_none(),
        "the ordinary script-end destructor removes the Team"
    );
    for id in original_members {
        assert_eq!(sim.team_script_vm.team_for_member(id), None);
        assert!(sim.entities().get(id).unwrap().lifecycle.object_alive());
    }
    assert_eq!(
        [
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex()
        ],
        rng_before
    );
}
