//! Authentic Hills input comparison and ordinary production Move controls.
//!
//! Native-established: Cell/TMP/Overlay setup and concrete MTNK73F0A0
//! admissions in tools/spatial_oracle/astar_hills_bridge_inputs.{py,json}.
//! Original A*/reconstruction/finishing controls are saved separately in
//! astar_hills_route.json; the first two ordinary Move routes compare against
//! those native outputs. Opposing movement ticks are Rust lifecycle validation.
//! Native command dispatch, full placement and paid Drive ticks remain excluded.
//! No supplied CanEnter answers or synthetic bridge flags.
use super::*;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::movement::locomotor::MovementLayer;

fn receipt() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_hills_bridge_inputs.json",
    ))
    .expect("saved original executable Hills receipt")
}

fn assert_native_route(case_name: &str, actual: &[(u16, u16)]) {
    let packet: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_hills_route.json",
    ))
    .expect("saved original full Hills search receipt");
    assert_eq!(packet["code_unchanged"], true);
    let case = packet["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == case_name)
        .expect("named original route");
    assert_original_route(case, actual);
}

fn assert_original_route(case: &serde_json::Value, actual: &[(u16, u16)]) {
    assert_eq!(case["returned_null"], false);
    assert_eq!(case["prestate"]["mission"], 2);
    assert_eq!(case["rng_before"], case["rng_after"]);
    let mut cell = (
        case["source"][0].as_i64().unwrap() as i32,
        case["source"][1].as_i64().unwrap() as i32,
    );
    let mut expected = vec![(cell.0 as u16, cell.1 as u16)];
    for direction in case["path"]["directions"].as_array().unwrap() {
        let direction = direction.as_i64().unwrap();
        if direction == -1 {
            break;
        }
        let delta = crate::util::direction::DIRECTION_DELTAS[direction as usize];
        cell.0 += delta.0;
        cell.1 += delta.1;
        expected.push((cell.0 as u16, cell.1 as u16));
    }
    assert_eq!(
        actual, expected,
        "original full search and both finishing passes: {}",
        case["name"]
    );
    let snapshots = case["pass_snapshots"].as_array().unwrap();
    assert_eq!(snapshots.len(), 3);
    let height = case["height"].as_i64().unwrap();
    for snapshot in snapshots {
        for retained in snapshot["retained_heights"].as_array().unwrap() {
            assert_eq!(retained.as_i64().unwrap(), height);
        }
    }
}

fn load_hills() -> crate::headless_scenario::HeadlessScenario {
    use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
    use crate::map::source::{LoadedMapSource, load_map_by_name_or_path_with_assets};
    let retail = retail_dir().expect("this ignored native-input test requires retail assets");
    let native = receipt();
    assert_eq!(
        native["native_sha256"].as_str().unwrap(),
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    assert_eq!(native["code_unchanged"], true);
    let assets = AssetManager::new(&retail, MediaArchiveMode::STOCK_DIGITAL)
        .expect("ordinary retail archive stack");
    let loaded = load_map_by_name_or_path_with_assets(&retail, "Hills.mmx", &assets)
        .expect("the same physical Hills.mmx used by the native receipt");
    let (hash, path) = match &loaded.source {
        LoadedMapSource::Loose {
            source_sha256,
            path,
            ..
        } => (source_sha256, path),
        source => panic!("expected the physical Hills.mmx wrapper: {source:?}"),
    };
    assert_eq!(
        hash,
        "96de259a741f1fc4d0d4bc4de1a1582755e90a77c2404978814149113fc42f22"
    );
    let wrapper = crate::assets::mix_archive::MixArchive::from_bytes(
        std::fs::read(path).expect("physical Hills.mmx wrapper"),
    )
    .expect("retail MIX wrapper");
    let inner = wrapper
        .get_by_id(0xD5FE80AC_u32 as i32)
        .expect("native Hills map entry");
    assert_eq!(
        crate::util::sha256::sha256_hex(inner),
        native["physical"]["sha256"].as_str().unwrap()
    );
    let scenario = headless_scenario::load(&retail, "Hills.mmx", SEED)
        .expect("ordinary staged authored-scenario loader");
    let terrain = scenario.sim().resolved_terrain.as_ref().unwrap();
    // These cells are empty in the native input fixture. Comparing terrain
    // fields does not claim native equivalence of map object placement.
    for row in native["after_overlay"].as_array().unwrap() {
        let x = row["coord"][0].as_u64().unwrap() as u16;
        let y = row["coord"][1].as_u64().unwrap() as u16;
        if !(y == 75 && (74..=80).contains(&x) || x == 78 && (73..=77).contains(&y)) {
            continue;
        }
        let cell = terrain.cell(x, y).expect("native-selected Hills cell");
        let identity = terrain.native_cell_identity((x as i16, y as i16));
        let query = NativeCellQuery::canonical(terrain);
        assert_eq!(
            i64::from(cell.final_tile_index),
            row["tile"].as_i64().unwrap(),
            "tile {x},{y}"
        );
        assert_eq!(
            u64::from(cell.final_sub_tile),
            row["subtile"].as_u64().unwrap(),
            "subtile {x},{y}"
        );
        assert_eq!(
            u64::from(cell.level),
            row["level"].as_u64().unwrap(),
            "level {x},{y}"
        );
        assert_eq!(
            u64::from(cell.slope_type),
            row["slope"].as_u64().unwrap(),
            "slope {x},{y}"
        );
        assert_eq!(
            u64::from(cell.yr_cell_land_type),
            row["land"].as_u64().unwrap(),
            "land {x},{y}"
        );
        assert_eq!(
            u64::from(cell.zone_type),
            row["zone_type"].as_u64().unwrap(),
            "zone type {x},{y}"
        );
        assert_eq!(
            u64::from(terrain.native_cell_flags(identity)),
            row["flags"].as_u64().unwrap(),
            "flags {x},{y}"
        );
        assert_eq!(
            i64::from(query.overlay_identity(identity)),
            row["overlay"].as_i64().unwrap(),
            "overlay {x},{y}"
        );
        assert_eq!(
            u64::from(terrain.native_cell_state(identity)),
            row["state"].as_u64().unwrap(),
            "frame {x},{y}"
        );
        let anchor = terrain.native_cell_anchor(identity).map(|anchor| {
            let (x, y) = terrain.native_cell_coord(anchor);
            serde_json::json!([x, y])
        });
        assert_eq!(
            anchor.unwrap_or(serde_json::Value::Null),
            row["anchor"],
            "anchor {x},{y}"
        );
    }
    scenario
}

fn spawn_mtnk(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    owner: &str,
    at: (u16, u16),
) -> u64 {
    let SimRuntime {
        simulation,
        resources,
    } = &mut scenario.runtime;
    let id = simulation
        .spawn_object("MTNK", owner, at.0, at.1, 0, &resources.rules)
        .expect("retail MTNK placed through the ordinary spawn owner");
    simulation.resolve_type_handles(&resources.rules);
    id
}

/// Establish the original Hills control's supplied ground actor prestate.
/// Ordinary height=-1 Unlimbo would select the structural bridge deck here;
/// use the existing concrete entry and admitted Reveal owners at native Z.
fn spawn_supplied_ground_mtnk(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    owner: &str,
    cell: (u16, u16),
    height: u8,
) -> u64 {
    let SimRuntime {
        simulation: sim,
        resources,
    } = &mut scenario.runtime;
    let id = sim
        .construct_object_limbo_at_height(
            "MTNK",
            owner,
            cell.0,
            cell.1,
            0,
            height,
            &resources.rules,
        )
        .unwrap();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    assert_eq!(
        sim.foot_can_enter(
            id,
            terrain.native_cell_identity((cell.0 as i16, cell.1 as i16)),
            InfantryEntryArgs {
                direction: 0,
                height: i32::from(height),
                previous_cell: None,
            },
            &resources.rules,
            Some(&resources.overlay_registry),
        )
        .unwrap(),
        0
    );
    sim.reveal_constructed_object_at_height(
        id,
        cell.0,
        cell.1,
        0,
        height,
        crate::sim::world::PlacementEvidence::UnitEntryAdmitted,
        &resources.rules,
    )
    .unwrap()
}

/// The route the ordinary Move's Find_Path installed from `start`: every
/// Foot+5E0 word stepped from that cell (a Drive head acceptance consumes
/// words but keeps them in the backing queue).
fn installed_route(
    scenario: &crate::headless_scenario::HeadlessScenario,
    id: u64,
    start: (u16, u16),
) -> Vec<(u16, u16)> {
    let queue = &scenario
        .sim()
        .entities()
        .get(id)
        .unwrap()
        .navigation
        .path_replay;
    let route = queue.installed_cells(start);
    let reference = queue.reference_cell.expect("installed route reference");
    assert!(
        route.contains(&(reference.0 as u16, reference.1 as u16)),
        "Foot+558 {reference:?} is not on the route installed from {start:?}: {route:?}"
    );
    route
}

/// Order-time MovementTarget is an empty scheduling adapter and the order
/// installs no route. The normal Drive Process owns Find_Path and installs
/// the Foot+5E0 queue on a subsequent tick
/// (movement_commands::prepare_destination_execution / movement_tick).
fn await_ordinary_route(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    id: u64,
) -> Vec<(u16, u16)> {
    let ordered = scenario
        .sim()
        .entities()
        .get(id)
        .unwrap()
        .navigation
        .path_replay
        .clone();
    for _ in 0..MAX_TICKS {
        let sim = scenario.sim();
        let actor = sim.entities().get(id).expect("live ordinary Move receiver");
        assert!(
            actor.movement_target.is_some(),
            "Move remains scheduled before Find_Path"
        );
        let start = (actor.position.rx, actor.position.ry);
        let facts = sim.path_grid().unwrap().cell(start.0, start.1).unwrap();
        assert_eq!(
            actor.position.z,
            facts.ground_level + if actor.on_bridge { 4 } else { 0 }
        );
        scenario.tick();
        let queue = &scenario
            .sim()
            .entities()
            .get(id)
            .unwrap()
            .navigation
            .path_replay;
        if !queue.directions.is_empty() && *queue != ordered {
            return installed_route(scenario, id, start);
        }
    }
    panic!("ordinary Move never published its first route within {MAX_TICKS} ticks");
}

/// Stop may retain a paid curve in the Rust scheduling adapter. The native
/// occupied Unit arm reads mission/NavCom/Facing/Drive, not adapter presence.
fn await_stopped_deck_pose(
    scenario: &mut crate::headless_scenario::HeadlessScenario,
    id: u64,
    cell: (u16, u16),
) {
    for _ in 0..MAX_TICKS {
        let sim = scenario.sim();
        let actor = sim.entities().get(id).expect("live stopped deck receiver");
        assert_eq!(
            (actor.position.rx, actor.position.ry),
            cell,
            "paid Stop head changed the native fixture cell"
        );
        assert!(actor.on_bridge);
        assert_eq!(actor.position.z, 6);
        assert!(
            actor.navigation.nav_com.is_none(),
            "accepted Stop clears NavCom"
        );
        if actor.passive_acquire_mission() == crate::sim::mission::MissionType::Guard
            && !actor.body_facing.is_rotating(sim.session.binary_frame)
            && crate::sim::movement::motion_query::is_moving(actor) == Some(false)
        {
            return;
        }
        scenario.tick();
    }
    panic!("Stop never reached canonical stationary Guard deck state within {MAX_TICKS} ticks");
}

#[test]
#[ignore = "requires the physical retail Hills.mmx and original native receipt"]
fn hills_native_fields_and_mtnk_admission_feed_an_ordinary_ramp_move() {
    let mut scenario = load_hills();
    let owner = prepare_commanding_house(&mut scenario);
    let id = spawn_mtnk(&mut scenario, &owner, (75, 75));
    let native = receipt();
    let rng = scenario.sim().rng_state();
    let sim = scenario.sim();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let actor = sim.entities().get(id).unwrap();
    assert_eq!(actor.position.z, 6);
    assert!(!actor.on_bridge);
    for row in native["movement_admissions"].as_array().unwrap() {
        let coord = (
            row["coord"][0].as_i64().unwrap() as i16,
            row["coord"][1].as_i64().unwrap() as i16,
        );
        assert!(row["previous_cell"].is_null());
        let result = sim
            .foot_can_enter(
                id,
                terrain.native_cell_identity(coord),
                InfantryEntryArgs {
                    direction: row["direction"].as_i64().unwrap() as i32,
                    height: row["height"].as_i64().unwrap() as i32,
                    previous_cell: None,
                },
                &scenario.runtime.resources.rules,
                Some(&scenario.runtime.resources.overlay_registry),
            )
            .expect("live Unit entry receiver");
        assert_eq!(
            u64::from(result),
            row["result"].as_u64().unwrap(),
            "live MTNK admission {row}"
        );
    }
    assert_eq!(native["movement_admissions"].as_array().unwrap().len(), 18);
    assert_eq!(
        scenario.sim().rng_state(),
        rng,
        "entry queries are RNG-neutral"
    );
    assert!(issue_ordinary_move(&mut scenario, &owner, id, (80, 75)));
    let path = await_ordinary_route(&mut scenario, id);
    assert_native_route("ramp_to_deck", &path);
    assert_eq!(path.first().copied(), Some((75, 75)));
    assert_eq!(path.last().copied(), Some((80, 75)));
    assert!(
        path.contains(&(76, 75)),
        "the route must use the native first deck cell: {path:?}"
    );
    let rows = record_until(&mut scenario, id, (80, 75));
    assert_eq!(
        rows.last().unwrap().cell,
        (80, 75),
        "ordinary ramp Move did not arrive"
    );
    for row in &rows {
        assert!(row.holds_invariant(), "native GetHeight invariant: {row:?}");
        if row.structural {
            assert!(row.on_bridge, "ramp traversal fell underneath: {row:?}");
            assert_eq!(row.loco_layer, MovementLayer::Bridge);
            assert_eq!(row.z, 6);
        }
    }
}

#[test]
#[ignore = "requires the physical retail Hills.mmx and original native receipt"]
fn hills_ground_underpass_keeps_the_ground_route_and_pose() {
    let mut scenario = load_hills();
    let owner = prepare_commanding_house(&mut scenario);
    let id = spawn_mtnk(&mut scenario, &owner, (78, 77));
    assert!(issue_ordinary_move(&mut scenario, &owner, id, (78, 72)));
    let path = await_ordinary_route(&mut scenario, id);
    assert_native_route("ground_underpass", &path);
    assert!(
        path.contains(&(78, 75)),
        "the control avoided the sampled underpass: {path:?}"
    );
    let rows = record_until(&mut scenario, id, (78, 72));
    assert_eq!(
        rows.last().unwrap().cell,
        (78, 72),
        "ordinary underpass Move did not arrive"
    );
    let under: Vec<_> = rows.iter().filter(|row| row.structural).collect();
    assert!(
        !under.is_empty(),
        "underpass control never entered the stamped band"
    );
    assert_under_span_invariant(&under);
    for row in under {
        assert_eq!(row.loco_layer, MovementLayer::Ground);
        assert_eq!(row.z, 2);
    }
}

#[test]
#[ignore = "requires the physical retail Hills.mmx and original native receipt"]
fn hills_stationary_friendly_deck_blocker_matches_original_repath() {
    let mut scenario = load_hills();
    let owner = prepare_commanding_house(&mut scenario);
    let blocker = spawn_mtnk(&mut scenario, &owner, (75, 75));
    assert!(issue_ordinary_move(
        &mut scenario,
        &owner,
        blocker,
        (78, 75)
    ));
    assert_eq!(
        record_until(&mut scenario, blocker, (78, 75))
            .last()
            .unwrap()
            .cell,
        (78, 75)
    );
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        assert!(simulation.apply_command(
            &owner,
            &Command::Stop { entity_id: blocker },
            Some(&resources.rules)
        ));
    }
    await_stopped_deck_pose(&mut scenario, blocker, (78, 75));
    let mover = spawn_mtnk(&mut scenario, &owner, (75, 75));
    assert!(issue_ordinary_move(&mut scenario, &owner, mover, (77, 75)));
    assert_eq!(
        record_until(&mut scenario, mover, (77, 75))
            .last()
            .unwrap()
            .cell,
        (77, 75)
    );
    {
        let SimRuntime {
            simulation,
            resources,
        } = &mut scenario.runtime;
        assert!(simulation.apply_command(
            &owner,
            &Command::Stop { entity_id: mover },
            Some(&resources.rules)
        ));
    }
    await_stopped_deck_pose(&mut scenario, mover, (77, 75));
    let sim = scenario.sim();
    let parked = sim.entities().get(blocker).unwrap();
    let actor = sim.entities().get(mover).unwrap();
    assert_eq!(
        parked.passive_acquire_mission(),
        crate::sim::mission::MissionType::Guard
    );
    for entity in [parked, actor] {
        assert!(entity.on_bridge);
        assert_eq!(entity.position.z, 6);
        assert!(entity.navigation.nav_com.is_none());
    }
    assert!(!parked.body_facing.is_rotating(sim.session.binary_frame));
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(parked),
        Some(false)
    );
    assert!(issue_ordinary_move(&mut scenario, &owner, mover, (81, 75)));
    let path = await_ordinary_route(&mut scenario, mover);
    assert_eq!(
        scenario
            .sim()
            .entities()
            .get(mover)
            .unwrap()
            .passive_acquire_mission(),
        crate::sim::mission::MissionType::Move
    );
    let parked = scenario.sim().entities().get(blocker).unwrap();
    assert_eq!(
        parked.passive_acquire_mission(),
        crate::sim::mission::MissionType::Guard
    );
    assert!(parked.navigation.nav_com.is_none());
    assert_eq!(
        crate::sim::movement::motion_query::is_moving(parked),
        Some(false)
    );
    assert_native_route("occupied_deck_repath", &path);
    assert!(
        !path.contains(&(78, 75)),
        "stationary same-owner deck blocker admitted"
    );
}

#[test]
#[ignore = "requires the physical retail Hills.mmx and original native receipt"]
fn hills_opposing_ordinary_moves_repath_on_the_deck() {
    let mut scenario = load_hills();
    let owner = prepare_commanding_house(&mut scenario);
    // Opposing orders are issued before either mover enters the deck. Their
    // future meeting is absent from the initial static occupation lists.
    let forward = spawn_mtnk(&mut scenario, &owner, (75, 75));
    assert!(issue_ordinary_move(
        &mut scenario,
        &owner,
        forward,
        (98, 75)
    ));
    let reverse = spawn_mtnk(&mut scenario, &owner, (98, 75));
    assert!(issue_ordinary_move(
        &mut scenario,
        &owner,
        reverse,
        (74, 75)
    ));
    let mut previous = scenario
        .sim()
        .entities()
        .get(forward)
        .unwrap()
        .navigation
        .path_replay
        .clone();
    let mut deck_rebuilds = 0;
    let mut met_on_deck = false;
    for _ in 0..MAX_TICKS {
        scenario.tick();
        let sim = scenario.sim();
        let actor = sim.entities().get(forward).unwrap();
        let other = sim.entities().get(reverse).unwrap();
        let cell = (actor.position.rx, actor.position.ry);
        let facts = sim.path_grid().unwrap().cell(cell.0, cell.1).unwrap();
        if facts.bridge_structural {
            assert!(
                actor.on_bridge,
                "blocked mover fell beneath its deck at {cell:?}"
            );
            assert_eq!(actor.movement_layer_or_ground(), MovementLayer::Bridge);
            assert_eq!(actor.position.z, facts.ground_level + 4);
            met_on_deck |= other.on_bridge
                && actor.position.rx.abs_diff(other.position.rx) <= 2
                && actor.position.ry.abs_diff(other.position.ry) <= 1;
        }
        if let Some(target) = actor.movement_target.as_ref() {
            let queue = &actor.navigation.path_replay;
            if route_reinstalled(&previous, queue) && facts.bridge_structural {
                deck_rebuilds += 1;
            }
            previous = queue.clone();
            assert_eq!(target.final_goal, None);
            assert_eq!(
                crate::sim::movement::movement_goal_cell(actor),
                Some((98, 75))
            );
        }
        if cell == (98, 75) {
            break;
        }
    }
    assert!(
        met_on_deck,
        "opposing movers never formed the live deck occupancy control"
    );
    assert!(
        deck_rebuilds > 0,
        "no route rebuild was observed on the occupied deck"
    );
    eprintln!("Hills opposing Move control: {deck_rebuilds} route rebuilds on deck");
}

#[test]
#[ignore = "requires physical retail Hills.mmx and original same-type marker receipt"]
fn hills_same_type_marker_downgrade_reaches_live_foot_search_and_cleanup() {
    use crate::sim::components::MovementTarget;
    use crate::sim::mission::{MissionId, MissionType};
    use crate::sim::movement::FindPathResult;
    use crate::sim::movement::movement_tick::FootPathRequest;
    use crate::sim::movement::path_markers::{
        DeferredBridgeMarker, bridge_marker_peer, install_path_replay,
    };
    use crate::sim::pathfinding::SearchMarkerOverlay;
    use crate::util::fixed_math::{SIM_ONE, SIM_ZERO};

    let packet: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_hills_markers.json",
    ))
    .expect("original42ACF0/full Hills search and429830 controls");
    assert_eq!(packet["code_unchanged"], true);
    assert_eq!(packet["marker_cleanup"], true);
    assert_eq!(packet["original_mark_cleanup"], true);
    assert_eq!(packet["rng_before_unmark"], packet["rng_after_unmark"]);
    let peer_native = &packet["blocker_prestate"];
    assert_eq!(peer_native["same_type_pointer"], true);
    assert_eq!(peer_native["same_house_pointer"], true);
    assert_eq!(
        peer_native["rng_before_mark"],
        peer_native["rng_after_mark"]
    );
    assert_eq!(peer_native["navigation_target"], "0x0");
    assert_eq!(peer_native["on_bridge"], false);
    let peer_cell = (
        peer_native["coord"][0].as_u64().unwrap() as u16,
        peer_native["coord"][1].as_u64().unwrap() as u16,
    );
    let mut peer_path = vec![peer_cell];
    for direction in peer_native["supplied_queue"].as_array().unwrap() {
        let direction = direction.as_i64().unwrap();
        if direction == -1 {
            break;
        }
        let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction as usize];
        let from = *peer_path.last().unwrap();
        peer_path.push((
            (i32::from(from.0) + dx) as u16,
            (i32::from(from.1) + dy) as u16,
        ));
    }

    for case in packet["routes"].as_array().unwrap() {
        let mut scenario = load_hills();
        let owner = prepare_commanding_house(&mut scenario);
        // Ordinary Unlimbo's height=-1 admission prefers a structural deck
        // at both native source cells. This receipt instead supplies ground
        // actors, admitted by their concrete height-aware entry and Reveal.
        let height = case["height"].as_u64().unwrap() as u8;
        let peer = spawn_supplied_ground_mtnk(&mut scenario, &owner, peer_cell, height);
        let source = (
            case["source"][0].as_u64().unwrap() as u16,
            case["source"][1].as_u64().unwrap() as u16,
        );
        let mover = spawn_supplied_ground_mtnk(&mut scenario, &owner, source, height);
        let goal = (
            case["goal"][0].as_u64().unwrap() as u16,
            case["goal"][1].as_u64().unwrap() as u16,
        );
        let urgency = case["urgency"].as_u64().unwrap() as u8;
        let SimRuntime {
            simulation: sim,
            resources,
        } = &mut scenario.runtime;
        let frame = sim.session.binary_frame;
        for id in [peer, mover] {
            sim.mission_assign_exact(id, MissionId::from_known(MissionType::Move), frame)
                .unwrap();
            let actor = sim.substrate.entities.get(id).unwrap();
            assert!(!actor.on_bridge);
            assert_eq!(actor.position.z, case["height"].as_u64().unwrap() as u8);
            assert!(actor.navigation.nav_com.is_none());
            assert!(actor.foot_occupation_enabled);
            assert_eq!(
                crate::sim::movement::motion_query::is_moving(actor),
                Some(false)
            );
        }
        // Explicit native prestate: a live same-type peer has Move, NavNULL,
        // the original returned queue and supplied Drive+34/+40 coordinates.
        // Decode those retained coordinates from the original byte receipt;
        // this does not claim player command, paid Drive or scheduler execution.
        let drive_bytes = peer_native["drive_bytes"].as_str().unwrap();
        let drive_coord = |offset: usize| {
            let word = |at: usize| {
                let mut bytes = [0_u8; 4];
                for (index, byte) in bytes.iter_mut().enumerate() {
                    let at = (at + index) * 2;
                    *byte = u8::from_str_radix(&drive_bytes[at..at + 2], 16).unwrap();
                }
                i32::from_le_bytes(bytes)
            };
            crate::sim::components::DriveCoord {
                x: word(offset),
                y: word(offset + 4),
                z: word(offset + 8),
            }
        };
        let actor = sim.substrate.entities.get_mut(peer).unwrap();
        // Drive constructor storage is lazy in VERA. Materialize it through
        // the existing active-instance Stop owner, whose constructor-rest
        // inputs leave head/NavCom/applied speed unchanged. The native packet
        // supplies the following coordinates; it does not claim a Stop call.
        assert!(crate::sim::movement::navcom::track_stop_moving(actor));
        assert!(actor.navigation.nav_com.is_none());
        assert_eq!(actor.foot_speed.applied_fraction(), SIM_ZERO);
        let drive = actor.locomotor.as_mut().unwrap();
        assert!(
            drive
                .track_destination(crate::sim::movement::track_process::TrackFamily::Drive)
                .is_none()
        );
        assert!(
            drive
                .track_head(crate::sim::movement::track_process::TrackFamily::Drive)
                .is_none()
        );
        assert!(drive.store_track_destination(
            crate::sim::movement::track_process::TrackFamily::Drive,
            Some(drive_coord(0x34))
        ));
        assert!(drive.store_track_head(
            crate::sim::movement::track_process::TrackFamily::Drive,
            Some(drive_coord(0x40))
        ));
        assert_eq!(
            drive.track_destination(crate::sim::movement::track_process::TrackFamily::Drive),
            drive.track_head(crate::sim::movement::track_process::TrackFamily::Drive)
        );
        assert_eq!(
            crate::sim::movement::motion_query::is_moving(actor),
            Some(true)
        );
        actor.movement_target = Some(MovementTarget::default());
        install_path_replay(&mut actor.navigation.path_replay, peer_cell, &peer_path, 1);
        assert_eq!(
            actor.navigation.path_replay.reference_cell,
            Some((
                peer_native["supplied_reference"][0].as_i64().unwrap() as i16,
                peer_native["supplied_reference"][1].as_i64().unwrap() as i16
            ))
        );
        assert_eq!(actor.navigation.path_replay.remaining_directions(), &[0, 0]);
        let peer_queue = actor.navigation.path_replay.clone();
        sim.substrate
            .entities
            .get_mut(mover)
            .unwrap()
            .movement_target = Some(MovementTarget {
            final_goal: Some(goal),
            ..Default::default()
        });
        assert_eq!(
            sim.substrate.entities.get(peer).unwrap().type_ref(),
            sim.substrate.entities.get(mover).unwrap().type_ref()
        );
        let rng = sim.rng_state();
        // Concrete Unit73F0A0 produces code2 for both supplied Foot+578
        // speed controls; no class answer is substituted in the Rust search.
        for control in packet["costs"].as_array().unwrap() {
            let speed = match control["supplied_speed_f64_bits"].as_str().unwrap() {
                "0000000000000000" => SIM_ZERO,
                "3ff0000000000000" => SIM_ONE,
                value => panic!("unrepresented native speed control {value}"),
            };
            sim.substrate
                .entities
                .get_mut(peer)
                .unwrap()
                .foot_speed
                .set_speed_fraction(speed);
            let terrain = sim.resolved_terrain.as_ref().unwrap();
            let entry = sim
                .foot_can_enter(
                    mover,
                    terrain.native_cell_identity((peer_cell.0 as i16, peer_cell.1 as i16)),
                    InfantryEntryArgs {
                        direction: 0,
                        height: case["height"].as_i64().unwrap() as i32,
                        previous_cell: Some(
                            terrain.native_cell_identity((source.0 as i16, source.1 as i16)),
                        ),
                    },
                    &resources.rules,
                    Some(&resources.overlay_registry),
                )
                .unwrap();
            assert_eq!(
                u64::from(entry),
                control["concrete_class"].as_u64().unwrap()
            );
            assert_eq!(control["rng_before"], control["rng_after"]);
        }
        sim.substrate
            .entities
            .get_mut(peer)
            .unwrap()
            .foot_speed
            .set_speed_fraction(SIM_ZERO);
        assert_eq!(
            u64::from(
                sim.substrate
                    .raw_cell_occupation
                    .ground_bits(peer_cell.0, peer_cell.1)
            ),
            peer_native["ground_occupation"].as_u64().unwrap()
        );
        assert_eq!(
            u64::from(
                sim.substrate
                    .raw_cell_occupation
                    .deck_bits(peer_cell.0, peer_cell.1)
            ),
            peer_native["deck_occupation"].as_u64().unwrap()
        );

        // Read the one marker owner between the same Mark0/Mark1 boundary
        // used by Find_Path. The subsequent live search must consume the
        // downgrade; with requested1 retained it would choose a detour.
        assert!(sim.foot_mark_remove(
            mover,
            Some(&resources.rules),
            Some(&resources.overlay_registry)
        ));
        {
            let grid = sim.path_grid_snapshot().unwrap();
            let peer = bridge_marker_peer(
                &sim.substrate.entities,
                mover,
                Some(&resources.rules),
                &sim.interner,
            );
            let context = DeferredBridgeMarker {
                mover_id: mover,
                mover: peer.as_ref(),
                rules: Some(&resources.rules),
                interner: &sim.interner,
                grid: &grid,
                terrain: sim.resolved_terrain.as_ref(),
                playfield_bounds: sim.playfield_bounds,
            }
            .reading(&sim.substrate.entities, &sim.substrate.raw_cell_occupation);
            let search = context.build(&sim.substrate.occupancy, mover, source, 0, false, urgency);
            let transactions = case["marker_transactions"].as_array().unwrap();
            let expected_urgency = transactions.first().map_or(urgency, |transaction| {
                transaction["effective_urgency"].as_u64().unwrap() as u8
            });
            assert_eq!(search.effective_urgency, expected_urgency);
            let mut expected = SearchMarkerOverlay::new();
            if let Some(transaction) = transactions.first() {
                for cell in transaction["marked_cells_after"].as_array().unwrap() {
                    expected.toggle((
                        cell[0].as_u64().unwrap() as u16,
                        cell[1].as_u64().unwrap() as u16,
                    ));
                }
            }
            assert_eq!(search.overlay, expected);
        }
        assert!(sim.foot_mark_put(
            mover,
            Some(&resources.rules),
            Some(&resources.overlay_registry)
        ));
        let marker_flags: Vec<_> = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .iter()
            .map(|cell| (cell.rx, cell.ry, cell.bridge_facts.raw_flags & 0x40000))
            .collect();
        let request = FootPathRequest::track(
            &sim.substrate.entities,
            mover,
            crate::sim::components::DriveCoord {
                x: i32::from(goal.0) * 256 + 128,
                y: i32::from(goal.1) * 256 + 128,
                z: case["height"].as_i64().unwrap() as i32 * 104,
            },
            urgency,
            sim.playfield_bounds,
            None,
            Some(&resources.rules),
        )
        .unwrap();
        assert_eq!(
            sim.foot_find_path(
                &request,
                None,
                &resources.rules,
                Some(&resources.overlay_registry)
            )
            .unwrap(),
            FindPathResult::Route
        );
        let actor = sim.substrate.entities.get(mover).unwrap();
        assert!(actor.movement_target.is_some());
        let queue = &actor.navigation.path_replay;
        assert_eq!(queue.cursor, 0, "Find_Path installs an unconsumed queue");
        assert_original_route(
            case,
            &queue.installed_cells((actor.position.rx, actor.position.ry)),
        );
        assert_eq!(case["rng_before"], case["rng_after"]);
        assert_eq!(
            sim.rng_state(),
            rng,
            "search/entry/markers/cleanup are RNG-neutral"
        );
        for id in [mover, peer] {
            let actor = sim.substrate.entities.get(id).unwrap();
            assert!(actor.lifecycle.cell_marked);
            assert!(sim.substrate.occupancy.contains_entity(
                actor.position.rx,
                actor.position.ry,
                id
            ));
        }
        assert_eq!(
            sim.substrate
                .entities
                .get(peer)
                .unwrap()
                .navigation
                .path_replay,
            peer_queue
        );
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        for (x, y, flags) in marker_flags {
            assert_eq!(
                terrain.cell(x, y).unwrap().bridge_facts.raw_flags & 0x40000,
                flags,
                "search marker leaked into retained Cell {x},{y}"
            );
        }
    }
}
