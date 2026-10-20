//! Native cache→Notify→virtual discovery consumers on supplied original cells.
use super::*;
use crate::map::entities::EntityCategory;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellListInsertion;
use crate::sim::vision::{assert_native_cell_history, discovery_corpus, native_map_cell_fixture};
use crate::sim::world::TechnoDiscoveryHistory;
use crate::util::save_codec;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn pair(v: &Value) -> (u16, u16) {
    (v[0].as_i64().unwrap() as u16, v[1].as_i64().unwrap() as u16)
}

fn fixture(topology: &Value) -> (Simulation, InternedId, InternedId) {
    let mut sim = Simulation::with_seed(31);
    // The native fixture explicitly seeds all three streams with31;
    // production Simulation::with_seed leaves MapGen at its own default.
    sim.mapgen_rng = crate::sim::rng::SimRng::new(31);
    let player = sim.interner.intern("Player");
    let other = sim.interner.intern("Other");
    sim.session.set_current_house_for_test(Some(player));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    for (owner, human) in [(player, true), (other, false)] {
        sim.houses
            .register(HouseState::new(owner, 0, None, human, 0, 10));
        sim.power_states.map_mut_for_test().insert(
            owner,
            crate::sim::power_system::PowerState::settled_for_test(),
        );
    }
    let (fog, terrain) = native_map_cell_fixture(topology, player);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.fog = fog;
    (sim, player, other)
}

fn actor(
    sim: &mut Simulation,
    owner: InternedId,
    cell: (u16, u16),
    offset: (i32, i32),
    category: EntityCategory,
) -> u64 {
    let typ = sim
        .interner
        .intern(if category == EntityCategory::Structure {
            "GTGCAN"
        } else {
            "E1"
        });
    let mut entity = GameEntity::new_at_frame_for_test(
        sim.substrate.allocate_stable_id(),
        cell.0,
        cell.1,
        0,
        0,
        owner,
        Health::new(100),
        typ,
        category,
        0,
        0,
        false,
        0,
    );
    entity
        .position
        .set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(offset.0));
    entity
        .position
        .set_sub_y_for_test(crate::util::fixed_math::SimFixed::from_num(offset.1));
    // The corpus executes original6F322F before the callback corridor.
    // This is its declared constructor prestate, not an alternative producer.
    entity
        .discovery
        .set_owned_by_current_house_for_test(Some(owner) == sim.session.current_house());
    sim.substrate.entities.insert(entity)
}

fn put(sim: &mut Simulation, id: u64, cell: (u16, u16), upper: bool) {
    sim.substrate.occupancy.add(
        cell.0,
        cell.1,
        id,
        if upper {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        },
        None,
        CellListInsertion::from_category(sim.substrate.entities.get(id).unwrap().category()),
    );
    sim.discover_cell_put_object(id, cell, UninitContext::default());
}

fn discovery(sim: &Simulation, id: u64) -> Value {
    let h = sim.substrate.entities.get(id).unwrap().discovery;
    json!([
        u8::from(h.owned_by_current_house()),
        u8::from(h.discovered_by_current_house()),
        u8::from(h.discovered_by_other_house())
    ])
}

fn rng_hashes(sim: &Simulation) -> Value {
    let hash = |hex: String| {
        let bytes: Vec<_> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        crate::util::sha256::sha256_hex(&bytes)
    };
    json!({"scenario":hash(sim.scenario_rng.native_state_hex()),"main":hash(sim.main_rng.native_state_hex()),"mapgen":hash(sim.mapgen_rng.native_state_hex())})
}

fn reveal_cell(sim: &mut Simulation, viewer: InternedId, cell: (u16, u16), final_pass: bool) {
    let notification = {
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(
            sim.resolved_terrain.as_ref().unwrap(),
        );
        sim.fog.reveal_map_cell(
            viewer,
            cell,
            final_pass,
            Some(&cells),
            crate::sim::vision::MapRevealProjection::Transient,
        )
    };
    if let Some(notification) = notification {
        sim.notify_cell_change(
            notification.cell,
            viewer,
            notification.fresh,
            UninitContext::default(),
        );
    }
}

fn conceal_topology(corpus: &Value, input: &Value) -> Value {
    let fixture = &corpus["fixture"];
    let mut allocated = Vec::new();
    for cell in fixture["allocated_cells"].as_array().unwrap() {
        let mut prior = fixture["default_prior"].clone();
        prior["cell"] = cell.clone();
        if let Some(overrides) = input["overrides"].as_array() {
            for change in overrides.iter().filter(|change| change["cell"] == *cell) {
                for (field, value) in change.as_object().unwrap() {
                    prior[field] = value.clone();
                }
            }
        }
        if *cell == json!([4, 4]) {
            if !input["level"].is_null() {
                prior["level"] = input["level"].clone();
            }
            if !input["flags"].is_null() {
                prior["flags"] = input["flags"].clone();
            }
        }
        allocated.push(prior);
    }
    json!({"allocated_cells": allocated,
        "dummy_prior": {"cell": [0,0], "level":0, "counter":1,
                        "gap":0, "alt":0, "flags":0, "cache":[-2,-2]}})
}

fn conceal_expected_after(topology: &Value, row: &Value) -> BTreeMap<(u16, u16), Value> {
    let mut expected: BTreeMap<_, _> = topology["allocated_cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|cell| (pair(&cell["cell"]), cell.clone()))
        .collect();
    for phase in ["after_conceal_delta", "after_update_delta"] {
        for delta in row[phase].as_array().unwrap() {
            let cell = (
                delta[0].as_i64().unwrap() as u16,
                delta[1].as_i64().unwrap() as u16,
            );
            expected.insert(
                cell,
                json!({"cell":[cell.0,cell.1], "level":delta[2],
                "counter":delta[3], "gap":delta[4], "alt":delta[5], "flags":delta[6],
                "cache":[delta[7],delta[8]]}),
            );
        }
    }
    expected
}

#[test]
fn native_trigger_conceal_circle_and_immediate_two_pass_sweep_match_original() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_conceal_area.json",
    ))
    .unwrap();
    assert_eq!(corpus["controls"].as_array().unwrap().len(), 13);
    for row in corpus["controls"].as_array().unwrap() {
        let input = &row["input"];
        let topology = conceal_topology(&corpus, input);
        let (mut sim, player, _) = fixture(&topology);
        sim.session.set_binary_frame_for_test(119); // Action101 must not wait for the120 rung.
        sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width: 4,
            height: 4,
            local_left: 0,
            local_top: 0,
            local_width: 4,
            local_height: 4,
        });
        sim.session.install_waypoints(
            &[(
                7,
                crate::map::waypoints::Waypoint {
                    index: 7,
                    rx: 4,
                    ry: 4,
                },
            )]
            .into(),
        );
        let radius = row["radius_reads"].as_array().unwrap().last().unwrap()["value"]
            .as_i64()
            .unwrap();
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&format!(
            "[General]\nRevealTriggerRadius={radius}\n"
        )))
        .unwrap();
        if input["latch"] == 1 {
            sim.fog.admit_whole_map_reveal(player);
        }
        assert_eq!(rng_hashes(&sim), row["rng_before"], "{}", input["name"]);
        let ((returned, trace), draws) = crate::sim::rng::trace_draws(|| {
            trace_reveal_calls(|| {
                sim.trigger_conceal_at_waypoint(7, UninitContext::with_rules(&rules))
            })
        });
        assert_eq!(returned, row["result"] == 1, "{}", input["name"]);
        let expected_walk: Vec<_> = row["conceal_probes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|probe| {
                (
                    probe[0].as_i64().unwrap() as u16,
                    probe[1].as_i64().unwrap() as u16,
                )
            })
            .collect();
        if row["map_calls"].as_array().unwrap().is_empty() {
            assert!(trace.walks.is_empty());
        } else {
            assert_eq!(trace.walks, vec![expected_walk], "{}", input["name"]);
        }
        let notifications: Vec<_> = row["notifications"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event[0] == "005865F0")
            .map(|event| {
                (
                    (
                        event[1].as_i64().unwrap() as u16,
                        event[2].as_i64().unwrap() as u16,
                    ),
                    true,
                )
            })
            .collect();
        assert_eq!(trace.notifications, notifications, "{}", input["name"]);
        let expected = conceal_expected_after(&topology, row);
        for state in expected.values() {
            assert_native_cell_history(
                &sim.fog,
                player,
                state,
                sim.resolved_terrain.as_ref().unwrap(),
                false,
            );
        }
        assert!(draws.is_empty(), "{}", input["name"]);
        assert_eq!(rng_hashes(&sim), row["rng_after"], "{}", input["name"]);
        let restored: vision::FogState =
            save_codec::deserialize(&save_codec::serialize(&sim.fog).unwrap()).unwrap();
        for state in expected.values() {
            assert_native_cell_history(
                &restored,
                player,
                state,
                sim.resolved_terrain.as_ref().unwrap(),
                false,
            );
        }
    }
}

#[test]
fn periodic_conceal_shares_the_full_sweep_and_preserves_viewer_planes() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_conceal_area.json",
    ))
    .unwrap();
    let row = corpus["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "global_pending_outside_local_circle")
        .unwrap();
    let topology = conceal_topology(&corpus, &row["input"]);
    let (mut sim, player, other) = fixture(&topology);
    sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
        theater: "TEMPERATE".into(),
        fill: "Clear".into(),
        level: 0,
        width: 4,
        height: 4,
        local_left: 0,
        local_top: 0,
        local_width: 4,
        local_height: 4,
    });
    let clear = conceal_topology(&corpus, &json!({}));
    let (other_fog, _) = native_map_cell_fixture(&clear, other);
    sim.fog
        .by_owner_mut_for_test()
        .insert(other, other_fog.by_owner()[&other].clone());
    sim.session.set_binary_frame_for_test(119);
    sim.tick_pending_shroud_conceal(UninitContext::default());
    assert!(sim.fog.is_ground_unshrouded(player, 1, 4));
    sim.session.set_binary_frame_for_test(120);
    let (_, calls) =
        trace_reveal_calls(|| sim.tick_pending_shroud_conceal(UninitContext::default()));
    assert!(!sim.fog.is_ground_unshrouded(player, 1, 4));
    assert!(sim.fog.is_ground_unshrouded(other, 1, 4));
    assert_eq!(calls.notifications, vec![((1, 4), true)]);
    assert_eq!(
        sim.fog.by_owner()[&player]
            .map_cell_edge_cache_for_test((1, 4))
            .1,
        0
    );
    // Discovery callbacks are synchronous: the represented House registry,
    // rather than randomized visibility-map storage, orders their effects.
    let mut alternate = row["input"].clone();
    alternate["overrides"][0]["cell"] = json!([7, 5]);
    let (other_fog, _) = native_map_cell_fixture(&conceal_topology(&corpus, &alternate), other);
    let (player_fog, _) = native_map_cell_fixture(&topology, player);
    sim.fog
        .by_owner_mut_for_test()
        .insert(other, other_fog.by_owner()[&other].clone());
    sim.fog
        .by_owner_mut_for_test()
        .insert(player, player_fog.by_owner()[&player].clone());
    sim.houses.reorder_for_test(&[other, player]);
    let (_, ordered) =
        trace_reveal_calls(|| sim.tick_pending_shroud_conceal(UninitContext::default()));
    assert_eq!(ordered.notifications, vec![((7, 5), true), ((1, 4), true)]);
}

#[test]
fn native_trigger_reveal_cell_put_then_discovery_matches_original_owner_histories() {
    let corpus = discovery_corpus();
    for row in corpus["cell_put_controls"].as_array().unwrap() {
        let input = &row["input"];
        let (mut sim, player, other) = fixture(&input["topology"]);
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(input["mode"] != 0),
        );
        let owner = if input["foreign"] == true {
            other
        } else {
            player
        };
        let id = actor(
            &mut sim,
            owner,
            (9, 10),
            (192, 64),
            EntityCategory::Infantry,
        );
        sim.discover_techno(id, owner, UninitContext::default());
        let compare = |sim: &Simulation, expected: &Value| {
            assert_eq!(discovery(sim, id), expected["object"], "{}", input["name"]);
            for (index, owner) in [player, other].into_iter().enumerate() {
                let power = serde_json::to_value(&sim.power_states[&owner]).unwrap();
                assert_eq!(
                    power["power_dirty"],
                    expected["houses"][index]["power_dirty"]
                );
                assert_eq!(
                    power["radar_dirty"],
                    expected["houses"][index]["radar_dirty"]
                );
                assert_eq!(
                    json!(sim.houses[&owner].discovered_by_current_house()),
                    expected["houses"][index]["byte_1f4"]
                );
            }
        };
        compare(&sim, &row["after_entry"]);
        if input["open_before_put"] == true {
            reveal_cell(&mut sim, player, (9, 10), false);
        }
        assert_eq!(rng_hashes(&sim), row["rng_before"]);
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            put(&mut sim, id, (9, 10), false);
            compare(&sim, &row["after_put"]);
            reveal_cell(&mut sim, player, (9, 10), false);
            compare(&sim, &row["after_reveal"]);
            reveal_cell(&mut sim, player, (9, 10), false);
        });
        assert!(draws.is_empty());
        assert_eq!(rng_hashes(&sim), row["rng_after"]);
        assert_native_cell_history(
            &sim.fog,
            player,
            &row["cell"],
            sim.resolved_terrain.as_ref().unwrap(),
            false,
        );
        let restored: TechnoDiscoveryHistory = save_codec::deserialize(
            &save_codec::serialize(&sim.substrate.entities.get(id).unwrap().discovery).unwrap(),
        )
        .unwrap();
        assert_eq!(restored, sim.substrate.entities.get(id).unwrap().discovery);
    }
}

#[test]
fn native_trigger_reveal_nearest_resident_and_diagonal_height_consumers_match_original() {
    let corpus = discovery_corpus();
    for row in corpus["nearest_controls"].as_array().unwrap() {
        let input = &row["input"];
        let (mut sim, player, other) = fixture(&input["topology"]);
        let mut actors = BTreeMap::new();
        for input in input["actors"].as_array().unwrap() {
            let offset = &input["offset"];
            let id = actor(
                &mut sim,
                other,
                (9, 10),
                (
                    offset[0].as_i64().unwrap() as i32,
                    offset[1].as_i64().unwrap() as i32,
                ),
                EntityCategory::Infantry,
            );
            put(&mut sim, id, (9, 10), input["upper"] == true);
            actors.insert(input["name"].as_str().unwrap(), id);
        }
        sim.notify_cell_change((9, 10), player, true, UninitContext::default());
        for expected in row["actors"].as_array().unwrap() {
            assert_eq!(
                discovery(&sim, actors[expected["name"].as_str().unwrap()]),
                expected["discovery"],
                "{}",
                input["name"]
            );
        }
        assert_eq!(rng_hashes(&sim), row["rng_after"]);
    }
    for row in corpus["diagonal_controls"].as_array().unwrap() {
        let input = &row["input"];
        let (mut sim, player, other) = fixture(&input["topology"]);
        let source = if input["source"].is_null() {
            (9, 10)
        } else {
            pair(&input["source"])
        };
        let mut actors = BTreeMap::new();
        for expected in row["actors"].as_array().unwrap() {
            let name = expected["name"].as_str().unwrap();
            let rung = name.strip_prefix("object").unwrap().parse::<i16>().unwrap();
            let cell = (
                (source.0 as i16).wrapping_add(rung) as u16,
                (source.1 as i16).wrapping_add(rung) as u16,
            );
            let id = actor(&mut sim, other, cell, (128, 128), EntityCategory::Infantry);
            put(&mut sim, id, cell, false);
            actors.insert(name, id);
        }
        sim.notify_cell_change(
            source,
            player,
            input["fresh"] != 0,
            UninitContext::default(),
        );
        for expected in row["actors"].as_array().unwrap() {
            assert_eq!(
                discovery(&sim, actors[expected["name"].as_str().unwrap()]),
                expected["discovery"],
                "{}",
                input["name"]
            );
        }
        assert_eq!(rng_hashes(&sim), row["rng_after"]);
        if input["fresh"] == 0 {
            assert!(sim.dirty_cells.radar().is_empty());
        }
    }
}

#[test]
fn native_trigger_reveal_canonical_dummy_and_passed_viewer_match_original() {
    let corpus = discovery_corpus();
    let row = &corpus["dummy_controls"];
    let (mut sim, player, _) = fixture(&row["input"]["topology"]);
    assert_eq!(rng_hashes(&sim), row["rng_before"]);
    for history in row["histories"].as_array().unwrap() {
        reveal_cell(
            &mut sim,
            player,
            pair(&history["cell"]),
            history["final"] == 1,
        );
        assert_native_cell_history(
            &sim.fog,
            player,
            &history["dummy"],
            sim.resolved_terrain.as_ref().unwrap(),
            true,
        );
        for state in history["states"].as_array().unwrap() {
            assert_native_cell_history(
                &sim.fog,
                player,
                state,
                sim.resolved_terrain.as_ref().unwrap(),
                false,
            );
        }
        let restored: crate::sim::vision::FogState =
            save_codec::deserialize(&save_codec::serialize(&sim.fog).unwrap()).unwrap();
        assert_native_cell_history(
            &restored,
            player,
            &history["dummy"],
            sim.resolved_terrain.as_ref().unwrap(),
            true,
        );
    }
    assert_eq!(rng_hashes(&sim), row["rng_after"]);
    for row in corpus["viewer_controls"].as_array().unwrap() {
        let (mut sim, player, other) = fixture(&row["input"]["topology"]);
        let id = actor(
            &mut sim,
            other,
            (9, 10),
            (128, 128),
            EntityCategory::Infantry,
        );
        put(&mut sim, id, (9, 10), false);
        let viewer = if row["viewer_current"] == true {
            player
        } else {
            other
        };
        // The direct native MapCell contrast uses one physical map plane.
        // Seed that supplied plane under its passed viewer for this API.
        if viewer != player {
            let plane = sim.fog.by_owner()[&player].clone();
            sim.fog.by_owner_mut_for_test().insert(viewer, plane);
        }
        reveal_cell(&mut sim, viewer, (9, 10), false);
        assert_eq!(discovery(&sim, id), row["discovery"]);
        assert_eq!(rng_hashes(&sim), row["rng_after"]);
    }
}

#[test]
fn native_trigger_reveal_whole_area_histories_match_original_cache_and_residents() {
    let corpus = discovery_corpus();
    for row in corpus["area_histories"].as_array().unwrap() {
        let input = &row["input"];
        let (mut sim, player, other) = fixture(&input["topology"]);
        let mut actors = BTreeMap::new();
        for (name, cell, offset) in [
            ("near", (68, 91), (16, 16)),
            ("far", (68, 91), (192, 192)),
            ("overlap", (69, 91), (64, 64)),
            ("diagonal", (70, 93), (128, 128)),
        ] {
            let id = actor(&mut sim, other, cell, offset, EntityCategory::Infantry);
            put(&mut sim, id, cell, false);
            actors.insert(name, id);
        }
        let config = vision::VisionConfig::new(
            None,
            Some((
                input["size"][0].as_i64().unwrap() as i32,
                input["size"][1].as_i64().unwrap() as i32,
            )),
            false,
            Vec::new(),
        );
        for (pass, history) in row["passes"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["histories"].as_array().unwrap())
        {
            let _ = sim.reveal_area(
                &[player],
                pair(&pass["center"]),
                0,
                pass["radius"].as_u64().unwrap() as u16,
                None,
                pass["final"] == 1,
                &config,
                UninitContext::default(),
                crate::sim::vision::MapRevealProjection::Transient,
                sim.session.current_house(),
            );
            for state in history["states"].as_array().unwrap() {
                assert_native_cell_history(
                    &sim.fog,
                    player,
                    state,
                    sim.resolved_terrain.as_ref().unwrap(),
                    false,
                );
            }
        }
        for expected in row["actors"].as_array().unwrap() {
            assert_eq!(
                discovery(&sim, actors[expected["name"].as_str().unwrap()]),
                expected["discovery"]
            );
        }
        assert_eq!(rng_hashes(&sim), row["rng_after"]);
        sim.fog
            .by_owner_mut_for_test()
            .get_mut(&player)
            .unwrap()
            .clear_all_visible();
        for state in row["histories"].as_array().unwrap().last().unwrap()["states"]
            .as_array()
            .unwrap()
        {
            assert_native_cell_history(
                &sim.fog,
                player,
                state,
                sim.resolved_terrain.as_ref().unwrap(),
                false,
            );
        }
    }
}

fn discovery_building(sim: &mut Simulation, owner: InternedId, input: &Value) -> u64 {
    let id = actor(sim, owner, (9, 10), (128, 128), EntityCategory::Structure);
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    entity
        .building_art
        .set_actually_placed_for_test(input["actually_placed"] == 1);
    let mission = &entity.mission;
    let fixture = crate::sim::mission::state::MissionTestFixture {
        current: crate::sim::mission::MissionId::from_raw(
            input["mission"].as_i64().unwrap_or(0) as i32
        ),
        queued: crate::sim::mission::MissionId::from_raw(
            input["queued"].as_i64().unwrap_or(-1) as i32
        ),
        suspended: mission.suspended(),
        movement_bypass_latch: mission.movement_bypass_latch(),
        handler_state: mission.handler_state(),
        mission_start_frame: mission.mission_start_frame(),
        ai_counter: mission.ai_counter(),
        dispatch_timer: mission.dispatch_timer(),
    };
    entity.mission.apply_test_fixture(fixture);
    if let Some(contacts) = input["contacts"].as_array() {
        entity.radio_contacts.set_capacity(contacts.len());
        for (slot, contact) in contacts.iter().enumerate() {
            if contact != &json!(0) {
                entity.radio_contacts.set_slot(slot, 99);
            }
        }
    }
    id
}

fn expected_grand_calls(row: &Value, id: u64) -> Vec<(u64, bool)> {
    row["trace"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["pc"] == "00445F80")
        .map(|_| (id, false))
        .collect()
}

#[test]
fn native_trigger_reveal_building_owner_entry_and_virtual_override_match_original() {
    let corpus = discovery_corpus();
    // Already-opened6E4 selects the original Grand_Opening early return,
    // so this comparison bounds the shared virtual admission and discovery
    // rather than supplying a second first-opening implementation.
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[BuildingTypes]\n0=GTGCAN\n[GTGCAN]\nStrength=100\n",
        ))
        .unwrap();
    let context = UninitContext::with_rules(&rules);
    for row in corpus["building_placement_controls"].as_array().unwrap() {
        let input = &row["input"];
        let (mut sim, _, owner) = fixture(&input["topology"]);
        if input["player_null"] == true {
            sim.session.set_current_house_for_test(None);
        }
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(
                input["mode"].as_i64().unwrap_or(0) != 0,
            ),
        );
        let house = sim.houses.get_mut(&owner).unwrap();
        house.set_is_human_for_test(input["human"].as_i64().unwrap_or(0) != 0);
        house.set_player_control_for_test(input["player_control"].as_i64().unwrap_or(0) != 0);
        let id = discovery_building(&mut sim, owner, input);
        assert_eq!(rng_hashes(&sim), row["rng_before"]);
        let ((_, calls), draws) = crate::sim::rng::trace_draws(|| {
            crate::sim::building_art::trace_grand_opening_calls(|| {
                sim.with_object_placement_scope(|sim| {
                    sim.with_object_placement_scope(|sim| {
                        sim.discover_building_owner_after_unlimbo(id, context);
                    })
                });
            })
        });
        assert_eq!(discovery(&sim, id), row["discovery"], "{}", input["name"]);
        assert_eq!(calls, expected_grand_calls(row, id), "{}", input["name"]);
        assert!(draws.is_empty());
        assert_eq!(rng_hashes(&sim), row["rng_after"]);
    }
    let row = &corpus["building_override"];
    let (mut sim, player, owner) = fixture(&row["input"]["topology"]);
    let id = discovery_building(&mut sim, owner, &row["input"]);
    assert_eq!(rng_hashes(&sim), row["rng_before"]);
    for history in row["histories"].as_array().unwrap() {
        let ((returned, calls), draws) = crate::sim::rng::trace_draws(|| {
            crate::sim::building_art::trace_grand_opening_calls(|| {
                if history["stage"] == "ai_unlimbo_owner" {
                    sim.with_object_placement_scope(|sim| {
                        sim.with_object_placement_scope(|sim| {
                            sim.discover_building_owner_after_unlimbo(id, context);
                        })
                    });
                    None
                } else {
                    Some(sim.discover_techno(id, player, context))
                }
            })
        });
        if let Some(returned) = returned {
            assert_eq!(returned, history["returned"] == 1);
        }
        assert_eq!(discovery(&sim, id), history["discovery"]);
        assert_eq!(
            calls,
            expected_grand_calls(history, id),
            "{}",
            history["stage"]
        );
        assert_eq!(
            json!(u8::from(
                sim.substrate
                    .entities
                    .get(id)
                    .unwrap()
                    .building_art
                    .actually_placed()
            )),
            history["actually_placed"]
        );
        assert!(draws.is_empty());
    }
    assert_eq!(rng_hashes(&sim), row["rng_after"]);
}

#[test]
fn native_trigger_reveal_all_fifty_five_startup_actions_match_original_empty_resident_footprint() {
    let corpus = discovery_corpus();
    let row = &corpus["full_startup_footprint"];
    let (mut sim, player, _) = fixture(&row["input"]["topology"]);
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[General]\nRevealTriggerRadius=9\nRevealByHeight=yes\n",
        ))
        .unwrap();
    assert_eq!(json!(rules.reveal_trigger_radius()), row["input"]["radius"]);
    let size = &row["input"]["size"];
    sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
        theater: "TEMPERATE".into(),
        fill: "Clear".into(),
        level: 0,
        width: size[0].as_u64().unwrap() as u32,
        height: size[1].as_u64().unwrap() as u32,
        local_left: 0,
        local_top: 0,
        local_width: size[0].as_u64().unwrap() as u32,
        local_height: size[1].as_u64().unwrap() as u32,
    });
    let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(
        sim.resolved_terrain.as_ref().unwrap(),
    );
    sim.install_fixture_path_grid(Some(&path));
    let controls = row["controls"].as_array().unwrap();
    assert_eq!(controls.len(), 55);
    let waypoints = controls
        .iter()
        .map(|control| {
            let index = control["value"].as_u64().unwrap() as u32;
            let cell = pair(&control["cell"]);
            (
                index,
                crate::map::waypoints::Waypoint {
                    index,
                    rx: cell.0,
                    ry: cell.1,
                },
            )
        })
        .collect();
    sim.session.install_waypoints(&waypoints);
    assert_eq!(rng_hashes(&sim), row["rng_before"]);
    let ((_, calls), draws) = crate::sim::rng::trace_draws(|| {
        trace_reveal_calls(|| {
            for control in controls {
                let returned = sim.trigger_reveal_at_waypoint(
                    control["value"].as_i64().unwrap() as i32,
                    UninitContext::with_rules(&rules),
                );
                assert_eq!(returned, control["returned"] == 1);
            }
        })
    });
    let passes = row["passes"].as_array().unwrap();
    assert_eq!(calls.walks.len(), passes.len());
    for (actual, expected) in calls.walks.iter().zip(passes) {
        let native: Vec<_> = expected["walk"]
            .as_str()
            .unwrap()
            .split_whitespace()
            .map(|cell| {
                let (x, y) = cell.split_once(',').unwrap();
                (
                    x.parse::<i16>().unwrap() as u16,
                    y.parse::<i16>().unwrap() as u16,
                )
            })
            .collect();
        assert_eq!(
            *actual, native,
            "source{} final{}",
            expected["source"], expected["final"]
        );
    }
    let native: std::collections::BTreeSet<_> = row["admitted_cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(pair)
        .collect();
    assert_eq!(calls.discovery_queries, native);
    assert!(draws.is_empty());
    assert_eq!(rng_hashes(&sim), row["rng_after"]);
    // Source registry order and empty lists are declared fixture inputs.
    // This establishes their geometry/callback reach, not whole native Fill
    // or the later live Tag poll order/resident membership.
    assert!(
        sim.fog.by_owner()[&player]
            .ground_flags_raw()
            .iter()
            .any(|flags| flags & 8 != 0)
    );
}

/// `MapClass::RevealEntireMap @ 0x00577F30` on a map Size: the CellIterator
/// walk (`0x00578350`/`0x00578290`) explores exactly the Size diamond, and
/// each cell's CellChangeNotify runs before the next cell: radar terrain
/// marks in walk order and the discovery of a resident on unseen ground.
#[test]
fn reveal_entire_map_walks_the_size_diamond_and_notifies_each_cell() {
    let mut sim = Simulation::with_seed(31);
    let player = sim.interner.intern("Player");
    let other = sim.interner.intern("Other");
    sim.session.set_current_house_for_test(Some(player));
    // A campaign CellPUT discovers only on unshrouded ground, so the resident
    // stays undiscovered until the walk reaches its cell.
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    for (owner, human) in [(player, true), (other, false)] {
        sim.houses
            .register(HouseState::new(owner, 0, None, human, 0, 10));
    }
    // A Size 6x4 diamond spans rx and ry 1..=9; 10x10 storage ends the walk
    // at the first cell past it, as an allocated map does.
    let (width, height) = (6u32, 4u32);
    let side = (width + height) as u16;
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
        side,
        side,
        crate::map::resolved_terrain::test_flat_cell,
    ));
    sim.fog = crate::sim::vision::FogState::new(side, side);
    sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
        theater: "TEMPERATE".to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width,
        height,
        local_left: 0,
        local_top: 0,
        local_width: width,
        local_height: height,
    });
    let walk: Vec<(u16, u16)> = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .native_cell_iterator(width as i32)
        .map(|cell| (cell.rx, cell.ry))
        .collect();
    assert_eq!(walk.len(), 4 * (2 * 6 - 1));
    let resident = actor(
        &mut sim,
        other,
        walk[20],
        (128, 128),
        EntityCategory::Infantry,
    );
    put(&mut sim, resident, walk[20], false);
    assert_eq!(discovery(&sim, resident), json!([0, 0, 0]));
    assert!(sim.dirty_cells.radar().is_empty());

    let ((), calls) =
        trace_reveal_calls(|| sim.reveal_entire_map(Some(player), UninitContext::default()));

    for &(rx, ry) in &walk {
        assert!(sim.fog.is_cell_revealed(player, rx, ry), "{rx},{ry}");
    }
    for (rx, ry) in [(0, 0), (9, 0), (0, 9), (9, 9)] {
        assert!(!sim.fog.is_cell_revealed(player, rx, ry), "{rx},{ry}");
    }
    // On flat ground only CellChangeNotify's first diagonal rung, the cell
    // itself, passes the height test.
    assert_eq!(
        calls.discovery_queries,
        walk.iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
    );
    assert_eq!(sim.dirty_cells.radar(), walk);
    assert_eq!(discovery(&sim, resident), json!([0, 1, 0]));
}

/// What an update leaves that a listed walk could get wrong: every map
/// plane, the Dummy, the shared cursor, and the update's notifications and
/// their consumers.
fn edge_update_record(sim: &Simulation, calls: &RevealCalls, residents: &[u64]) -> Value {
    use std::hash::Hasher;
    let planes: BTreeMap<_, _> = sim
        .fog
        .by_owner()
        .iter()
        .map(|(viewer, plane)| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            plane.hash_cells(&mut hasher);
            (sim.interner.resolve(*viewer).to_string(), hasher.finish())
        })
        .collect();
    json!({
        "planes": planes,
        "dummy": format!("{:?}", sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy().snapshot()),
        "cursor": format!("{:?}", sim.native_allocated_cell_cursor),
        "notifications": calls.notifications,
        "queries": calls.discovery_queries,
        "radar": sim.dirty_cells.radar(),
        "discovery": residents.iter().map(|&id| discovery(sim, id)).collect::<Vec<_>>(),
    })
}

/// How many receiver calls an update made.
fn edge_visits(record: &Value) -> u64 {
    record["visits"].as_u64().unwrap()
}

/// Three Houses' maps on a Size 12x8 diamond allocated as a materialized map
/// allocates it, with residents to discover, and the script that writes them
/// between updates.
struct EdgeScript {
    sim: Simulation,
    viewers: Vec<InternedId>,
    /// The walk's cells, in walk order.
    walk: Vec<(u16, u16)>,
    /// The walk's cells with an unallocated neighbour.
    edge: Vec<(u16, u16)>,
    residents: Vec<u64>,
    seed: u32,
}

impl EdgeScript {
    const SIZE: (u32, u32) = (12, 8);

    fn new() -> Self {
        let mut sim = Simulation::with_seed(31);
        let viewers: Vec<_> = ["Player", "Other", "Third"]
            .into_iter()
            .map(|name| sim.interner.intern(name))
            .collect();
        sim.session.set_current_house_for_test(Some(viewers[0]));
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
        );
        for (index, &owner) in viewers.iter().enumerate() {
            sim.houses
                .register(HouseState::new(owner, 0, None, index == 0, 0, 10));
        }
        let (width, height) = Self::SIZE;
        let side = (width + height) as u16;
        let mut terrain = crate::map::resolved_terrain::test_grid(side, side, |rx, ry| {
            let mut cell = crate::map::resolved_terrain::test_flat_cell(rx, ry);
            cell.level = ((rx * 7 + ry * 3) % 4) as u8;
            cell
        });
        let walk: Vec<(u16, u16)> = terrain
            .native_cell_iterator(width as i32)
            .map(|cell| (cell.rx, cell.ry))
            .collect();
        terrain.test_set_native_allocated_cells(&walk);
        let edge = walk
            .iter()
            .copied()
            .filter(|&(x, y)| {
                crate::map::resolved_terrain::ROW_NEIGHBORS
                    .iter()
                    .any(|&(dx, dy)| {
                        terrain
                            .native_fixed_cell_index(
                                (x as i16).wrapping_add(dx),
                                (y as i16).wrapping_add(dy),
                            )
                            .is_none()
                    })
            })
            .collect();
        sim.install_resolved_terrain_for_new_map(terrain);
        sim.fog = crate::sim::vision::FogState::new(side, side);
        sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
            theater: "TEMPERATE".to_string(),
            fill: "Clear".to_string(),
            level: 0,
            width,
            height,
            local_left: 0,
            local_top: 0,
            local_width: width,
            local_height: height,
        });
        let residents = [walk[17], walk[60], walk[101], walk[140]]
            .into_iter()
            .enumerate()
            .map(|(index, cell)| {
                let id = actor(
                    &mut sim,
                    viewers[index % 3],
                    cell,
                    (128, 128),
                    EntityCategory::Infantry,
                );
                put(&mut sim, id, cell, false);
                id
            })
            .collect();
        Self {
            sim,
            viewers,
            walk,
            edge,
            residents,
            seed: 0x9E37_79B9,
        }
    }

    fn next(&mut self, n: usize) -> usize {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 17;
        self.seed ^= self.seed << 5;
        self.seed as usize % n
    }

    /// A cell of `cells` (the walk or its edge) picked by the script.
    fn pick(&mut self, edge: bool) -> (u16, u16) {
        let cells = if edge { &self.edge } else { &self.walk };
        let index = self.next(cells.len());
        if edge {
            self.edge[index]
        } else {
            self.walk[index]
        }
    }

    /// One round's writes on every map: reveals and leaves, conceals, a fire
    /// reveal (pending cells), a reveal on the map's edge (Dummy stamps), and
    /// on odd rounds a reveal off the map (the Dummy's ground).
    fn write(&mut self, round: u32) {
        for viewer in self.viewers.clone() {
            for _ in 0..3 {
                let (x, y) = self.pick(false);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (-1, 0), (0, -1)] {
                    let cell = ((x as i16 + dx) as u16, (y as i16 + dy) as u16);
                    reveal_cell(&mut self.sim, viewer, cell, false);
                    if self.next(2) == 0 {
                        reveal_cell(&mut self.sim, viewer, cell, true);
                    }
                }
            }
            for _ in 0..4 {
                let cell = self.pick(false);
                conceal(&mut self.sim, viewer, cell);
            }
            let cell = self.pick(false);
            fire_reveal(&mut self.sim, viewer, cell);
            let cell = self.pick(true);
            reveal_cell(&mut self.sim, viewer, cell, false);
            if round % 2 == 1 {
                reveal_cell(&mut self.sim, viewer, (0, 0), round == 3);
            }
        }
    }
}

fn conceal(sim: &mut Simulation, viewer: InternedId, cell: (u16, u16)) {
    let query = crate::map::resolved_terrain::NativeCellQuery::canonical(
        sim.resolved_terrain.as_ref().unwrap(),
    );
    sim.fog.conceal_map_cell(viewer, cell, Some(&query));
}

fn fire_reveal(sim: &mut Simulation, viewer: InternedId, cell: (u16, u16)) {
    crate::sim::vision::reveal_shroud_on_fire(
        &mut sim.fog,
        viewer,
        crate::sim::components::DriveCoord {
            x: i32::from(cell.0) * 256 + 128,
            y: i32::from(cell.1) * 256 + 128,
            z: 0,
        },
        &crate::sim::vision::VisionConfig::default(),
        None,
    );
}

/// The scripted rounds' 120-frame updates; halfway a save and load drops the
/// lists. Returns each update's record.
fn scripted_edge_updates() -> Vec<Value> {
    let mut script = EdgeScript::new();
    let mut records = Vec::new();
    for round in 0..6u32 {
        if round == 3 {
            script.sim.fog =
                save_codec::deserialize(&save_codec::serialize(&script.sim.fog).unwrap()).unwrap();
        }
        script.write(round);
        script
            .sim
            .session
            .set_binary_frame_for_test(120 * (round + 1));
        let ((), calls) = trace_reveal_calls(|| {
            script
                .sim
                .tick_pending_shroud_conceal(UninitContext::default())
        });
        let mut record = edge_update_record(&script.sim, &calls, &script.residents);
        record["visits"] = json!(calls.edge_visits);
        records.push(record);
    }
    assert!(
        script
            .sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .allocated_cell_order(EdgeScript::SIZE.0 as i32)
            .is_some()
    );
    records
}

/// Rust regression test: updates that visit the listed cells leave what the
/// complete walk leaves.
#[test]
fn listed_shroud_edge_updates_match_the_complete_walk() {
    crate::sim::vision::VISIT_EVERY_CELL.set(true);
    let complete = scripted_edge_updates();
    crate::sim::vision::VISIT_EVERY_CELL.set(false);
    let listed = scripted_edge_updates();
    let notified = complete
        .iter()
        .filter(|record| !record["notifications"].as_array().unwrap().is_empty())
        .count();
    assert!(notified >= 3, "the script makes the updates notify");
    for (round, (listed, complete)) in listed.iter().zip(&complete).enumerate() {
        // Six passes over 184 cells for three Houses visit 1,104 cells; the
        // listed updates visit fewer, except the one after the load.
        assert_eq!(edge_visits(complete), 3 * 2 * 184, "update {round}");
        if round != 3 {
            assert!(
                edge_visits(listed) < edge_visits(complete) / 2,
                "update {round}"
            );
        }
        let mut listed = listed.clone();
        listed["visits"] = complete["visits"].clone();
        assert_eq!(&listed, complete, "update {round}");
    }
}

/// Updates whose notifications change the maps under the walk: every third
/// reveals, conceals and fire-reveals cells ahead of and behind it, every
/// fourth runs a whole nested update of the same map and every fifth one of
/// another, either of which moves the shared cursor. Each notification logs
/// the cursor and the Dummy it sees.
fn edge_updates_with_callbacks() -> Vec<Value> {
    let mut script = EdgeScript::new();
    let mut records = Vec::new();
    let walk = script.walk.clone();
    for round in 0..4u32 {
        script.write(round);
        let mut log = Vec::new();
        let mut count = 0usize;
        let viewers = script.viewers.clone();
        for (index, &viewer) in viewers.iter().enumerate() {
            let other = viewers[(index + 1) % viewers.len()];
            let mut callback = |sim: &mut Simulation, change: vision::MapCellNotification| {
                count += 1;
                let seen = |sim: &Simulation, nested: bool| {
                    json!([
                        change.cell,
                        change.fresh,
                        nested,
                        format!("{:?}", sim.native_allocated_cell_cursor),
                        format!(
                            "{:?}",
                            sim.resolved_terrain
                                .as_ref()
                                .unwrap()
                                .shared_cell_dummy()
                                .snapshot()
                        )
                    ])
                };
                log.push(seen(sim, false));
                sim.notify_cell_change(change.cell, viewer, change.fresh, UninitContext::default());
                if count % 3 == 0 {
                    for step in 0..3 {
                        let cell = walk[(count * 37 + step * 61) % walk.len()];
                        reveal_cell(sim, viewer, cell, step == 1);
                        conceal(sim, viewer, walk[(count * 53 + step * 17) % walk.len()]);
                    }
                    fire_reveal(sim, viewer, walk[(count * 29) % walk.len()]);
                }
                for (every, nested) in [(4, viewer), (5, other)] {
                    if count % every == 0 {
                        sim.update_shroud_edges_notifying(
                            nested,
                            UninitContext::default(),
                            &mut |sim, change| {
                                log.push(json!([change.cell, change.fresh, true]));
                                sim.notify_cell_change(
                                    change.cell,
                                    nested,
                                    change.fresh,
                                    UninitContext::default(),
                                );
                            },
                        );
                    }
                }
            };
            let ((), calls) = trace_reveal_calls(|| {
                script.sim.update_shroud_edges_notifying(
                    viewer,
                    UninitContext::default(),
                    &mut callback,
                )
            });
            let mut record = edge_update_record(&script.sim, &calls, &script.residents);
            record["visits"] = json!(calls.edge_visits);
            records.push(record);
        }
        records.push(json!(log));
    }
    records
}

/// Rust regression test: notifications that change the maps or move the
/// cursor see, and leave, what they do under the complete walk.
#[test]
fn listed_shroud_edge_updates_survive_their_callbacks() {
    crate::sim::vision::VISIT_EVERY_CELL.set(true);
    let complete = edge_updates_with_callbacks();
    crate::sim::vision::VISIT_EVERY_CELL.set(false);
    let listed = edge_updates_with_callbacks();
    let nested = complete
        .iter()
        .filter_map(|record| record.as_array())
        .flatten()
        .filter(|entry| entry[2] == true)
        .count();
    assert!(nested > 0, "the callbacks nest updates");
    for (index, (listed, complete)) in listed.iter().zip(&complete).enumerate() {
        let mut listed = listed.clone();
        if listed.get("visits").is_some() {
            listed["visits"] = complete["visits"].clone();
        }
        assert_eq!(&listed, complete, "record {index}");
    }
}

/// Horizontal_Step's tail (`0x004CF55D..0x004CF5EC`): a selected enemy
/// aircraft drops out of the player's selection once it flies over the
/// player's shroud (`MapClass::IsShrouded @ 0x00586360` at its Location);
/// over mapped ground it stays selected, and the player's own aircraft is
/// never deselected.
#[test]
fn a_selected_enemy_aircraft_is_deselected_over_the_players_shroud() {
    let (mut sim, rules, registry, player, enemy_house) = shroud_selection_fixture(true);
    let own = sim
        .spawn_object_with_overlay_registry("HORNET", "Americans", 6, 12, 0, &rules, &registry)
        .unwrap();
    let enemy = sim
        .spawn_object_with_overlay_registry("HORNET", "Russians", 8, 15, 0, &rules, &registry)
        .unwrap();
    let flights = [(own, player, 12), (enemy, enemy_house, 15)];
    let tracks = move_east_selected(&mut sim, &rules, &registry, &flights, 600);
    for (id, track) in [own, enemy].into_iter().zip(tracks) {
        assert!(
            track.selected_over_mapped_ground,
            "{id}: over mapped ground it stays selected"
        );
        assert!(track.reached_shroud, "{id}: it flies into the shroud");
    }
    assert!(
        !selected(&sim, enemy),
        "over the player's shroud it leaves the selection"
    );
    assert!(
        selected(&sim, own),
        "the player's own aircraft stays selected"
    );
}

/// Foot PerCellProcess's step (`0x004D8DA7..0x004D8DF5`): a selected enemy
/// infantryman walking into the player's shroud leaves the player's
/// selection at the first cell he arrives at there; over mapped ground he
/// stays selected. (A walker's own sight keeps the player's own Location
/// mapped, so the owner test shows only on the fliers above.)
#[test]
fn a_selected_enemy_infantryman_is_deselected_walking_into_the_players_shroud() {
    let (mut sim, rules, registry, _, enemy_house) = shroud_selection_fixture(true);
    // The player keeps an object, so his house stays in the game.
    sim.spawn_object_with_overlay_registry("ENGINEER", "Americans", 6, 12, 0, &rules, &registry)
        .unwrap();
    let enemy = sim
        .spawn_object_with_overlay_registry("ENGINEER", "Russians", 8, 15, 0, &rules, &registry)
        .unwrap();
    let [track] = move_east_selected(
        &mut sim,
        &rules,
        &registry,
        &[(enemy, enemy_house, 15)],
        4000,
    )
    .try_into()
    .ok()
    .unwrap();
    assert!(
        track.selected_over_mapped_ground,
        "over mapped ground he stays selected"
    );
    assert!(track.reached_shroud, "he walks into the shroud");
    assert!(
        !selected(&sim, enemy),
        "in the player's shroud he leaves the selection"
    );
}

/// Jumpjet Process's deselect (`0x0054B07D..0x0054B10C`): a selected enemy
/// jumpjet drops out of the player's selection once it flies over the
/// player's shroud; over mapped ground it stays selected, and the player's
/// own jumpjet is never deselected.
#[test]
fn a_selected_enemy_jumpjet_is_deselected_over_the_players_shroud() {
    let (mut sim, rules, registry, player, enemy_house) = shroud_selection_fixture(true);
    let own = sim
        .spawn_object_with_overlay_registry("JUMPJET", "Americans", 6, 12, 0, &rules, &registry)
        .unwrap();
    let enemy = sim
        .spawn_object_with_overlay_registry("JUMPJET", "Russians", 8, 15, 0, &rules, &registry)
        .unwrap();
    let flights = [(own, player, 12), (enemy, enemy_house, 15)];
    let tracks = move_east_selected(&mut sim, &rules, &registry, &flights, 600);
    for (id, track) in [own, enemy].into_iter().zip(tracks) {
        assert!(
            track.selected_over_mapped_ground,
            "{id}: over mapped ground it stays selected"
        );
        assert!(track.reached_shroud, "{id}: it flies into the shroud");
    }
    assert!(
        !selected(&sim, enemy),
        "over the player's shroud it leaves the selection"
    );
    assert!(
        selected(&sim, own),
        "the player's own jumpjet stays selected"
    );
}

/// Jumpjet Process's discovery (`0x0054B112..0x0054B166`), in a campaign,
/// where a Cell put discovers an object only off the player's shroud
/// (`0x0047E953..0x0047E9DC`): an enemy jumpjet put down in the shroud stays
/// undiscovered until it flies over ground the player has mapped, where the
/// player discovers it while it is still airborne.
#[test]
fn a_campaign_enemy_jumpjet_is_discovered_flying_over_mapped_ground() {
    use crate::sim::command::{Command, CommandEnvelope};
    let (mut sim, rules, registry, _, enemy_house) = shroud_selection_fixture(false);
    let enemy = sim
        .spawn_object_with_overlay_registry("JUMPJET", "Russians", 30, 15, 0, &rules, &registry)
        .unwrap();
    let discovered = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(enemy)
            .unwrap()
            .discovery
            .discovered_by_current_house()
    };
    assert!(
        !discovered(&sim),
        "put down in the shroud, it is undiscovered"
    );
    let grid = sim.path_grid.clone();
    let order = CommandEnvelope::new(
        enemy_house,
        sim.session.tick(),
        Command::Move {
            entity_id: enemy,
            target_rx: 8,
            target_ry: 15,
        },
    );
    sim.advance_tick(&[order], Some(&rules), grid.as_deref(), Some(&registry), 67);
    // MapClass::IsShrouded at the jumpjet's Location, height-projected.
    let over_shroud = |sim: &Simulation| {
        let entity = sim.substrate.entities.get(enemy).unwrap();
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        crate::sim::vision::point_is_shrouded(
            &sim.fog,
            &crate::map::resolved_terrain::NativeCellQuery::canonical(terrain),
            sim.session.current_house().unwrap(),
            crate::sim::movement::ground_pose::object_location(entity, Some(terrain)),
        )
    };
    let mut flew_over_shroud = false;
    let mut discovered_in_flight = false;
    for _ in 0..600 {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), grid.as_deref(), Some(&registry), 67);
        if !discovered(&sim) {
            assert!(
                over_shroud(&sim),
                "over mapped ground it is discovered the same frame"
            );
            flew_over_shroud = true;
            continue;
        }
        let entity = sim.substrate.entities.get(enemy).unwrap();
        discovered_in_flight = crate::sim::movement::air_movement::current_fly_height(
            entity,
            sim.resolved_terrain.as_ref(),
        ) > 0
            && !over_shroud(&sim);
        break;
    }
    assert!(flew_over_shroud, "it flew undiscovered over the shroud");
    assert!(
        discovered_in_flight,
        "the player discovers it in flight over mapped ground"
    );
}

/// A skirmish (or campaign) scene whose player (Americans) has mapped the
/// west of the map only: the 12-cell radius around (6, 12).
fn shroud_selection_fixture(
    skirmish: bool,
) -> (
    Simulation,
    crate::rules::ruleset::RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    crate::sim::intern::InternedId,
    crate::sim::intern::InternedId,
) {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules("");
    sim.session.set_game_mode(
        crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(skirmish),
    );
    sim.session.game_options_mut_for_test().short_game = false;
    for (side, name) in ["Americans", "Russians"].into_iter().enumerate() {
        let house = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                house, side as u8, None, true, 0, 10,
            ));
    }
    let player = sim.interner.get("Americans").unwrap();
    let enemy = sim.interner.get("Russians").unwrap();
    sim.session.set_current_house_for_test(Some(player));
    sim.fog = FogState::new(33, 33);
    sim.fog.ensure_viewer(player);
    // MapClass::RevealArea's final0/final1 pair maps the cells with no
    // sustained sight source.
    let config = sim.sight_reveal_config(Some(&rules));
    for final_pass in [false, true] {
        let _ = sim.reveal_area(
            &[player],
            (6, 12),
            0,
            12,
            None,
            final_pass,
            &config,
            UninitContext::default(),
            crate::sim::vision::MapRevealProjection::Transient,
            Some(player),
        );
    }
    (sim, rules, registry, player, enemy)
}

fn selected(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .unwrap()
        .selection
        .is_selected()
}

/// What one mover did on its way east.
struct EastTrack {
    /// It crossed the player's mapped ground (cells 10..=14), and was
    /// selected on every frame there.
    selected_over_mapped_ground: bool,
    /// It reached cell 24, inside the shroud.
    reached_shroud: bool,
}

/// Select each `(object, owner, row)` mover, order it east to (30, row), and
/// run the production tick until every one reaches cell 24 (`frames` at
/// most).
fn move_east_selected(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    flights: &[(u64, crate::sim::intern::InternedId, u16)],
    frames: u32,
) -> Vec<EastTrack> {
    use crate::sim::command::{Command, CommandEnvelope};
    for &(id, ..) in flights {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .selection
            .set_selected_for_test(true);
    }
    let orders: Vec<_> = flights
        .iter()
        .map(|&(id, owner, row)| {
            CommandEnvelope::new(
                owner,
                sim.session.tick(),
                Command::Move {
                    entity_id: id,
                    target_rx: 30,
                    target_ry: row,
                },
            )
        })
        .collect();
    let grid = sim.path_grid.clone();
    sim.advance_tick(&orders, Some(rules), grid.as_deref(), Some(registry), 67);
    let mut tracks: Vec<_> = flights
        .iter()
        .map(|_| EastTrack {
            selected_over_mapped_ground: true,
            reached_shroud: false,
        })
        .collect();
    let mut over_mapped = vec![false; flights.len()];
    for _ in 0..frames {
        if tracks.iter().all(|track| track.reached_shroud) {
            break;
        }
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(rules), grid.as_deref(), Some(registry), 67);
        for (index, &(id, ..)) in flights.iter().enumerate() {
            let rx = sim.substrate.entities.get(id).unwrap().position.rx();
            if (10..=14).contains(&rx) {
                over_mapped[index] = true;
                tracks[index].selected_over_mapped_ground &= selected(sim, id);
            }
            tracks[index].reached_shroud |= rx >= 24;
        }
    }
    for (track, seen) in tracks.iter_mut().zip(over_mapped) {
        track.selected_over_mapped_ground &= seen;
    }
    tracks
}
