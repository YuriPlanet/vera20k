//! Physical Shrapnel low bridge: authored Water gap -> ordinary Engineer repair.
//! Ordinary-command integration with bounded native first Ship head, receiver,
//! sinking AI and cleanup comparisons. Scene timing is a Rust production
//! regression; these assertions do not claim a native whole-scene comparison.
use crate::headless_scenario::{HeadlessScenario, SIM_TICK_MS};
use crate::rules::terrain_rules::LandType;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::bridge_test_evidence::{load_shrapnel as loaded_shrapnel, save_scene};
use serde_json::{Value, json};

fn export_scene(scene: &HeadlessScenario, phase: &str) {
    let sim = scene.sim();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let cells: Vec<Value> = (51..=67)
        .flat_map(|y| (111..=120).map(move |x| (x, y)))
        .filter_map(|(x, y)| {
            let cell = terrain.cell(x, y)?;
            Some(json!({
                "coord":[x,y], "tile":cell.final_tile_index,
                "subtile":cell.final_sub_tile, "level":cell.level,
                "slope":cell.slope_type, "land":cell.yr_cell_land_type,
                "base_land":cell.base_yr_cell_land_type,
                "zone_type":cell.zone_type,
                "bridge":cell.bridge_facts,
                "ground_walk_blocked":cell.ground_walk_blocked,
                "speed_costs":format!("{:?}",cell.speed_costs),
                "full_cell":format!("{cell:?}"),
            }))
        })
        .collect();
    let entities: Vec<Value> = sim
        .entities()
        .values()
        .filter(|e| (110..=121).contains(&e.position.rx) && (50..=68).contains(&e.position.ry))
        .map(|e| json!({"type":sim.resolve(e.type_ref()), "entity":e}))
        .collect();
    let result = json!({
        "phase":phase, "frame":sim.session.binary_frame,
        "map_hash":format!("{:016x}",scene.map.ini.content_hash()),
        "rules_hash":format!("{:016x}",scene.runtime.resources.rules.simulation_config_hash()),
        "theater":scene.map.header.theater,
        "native_size":[scene.map.header.width,scene.map.header.height],
        "local_size":[scene.map.header.local_left,scene.map.header.local_top,scene.map.header.local_width,scene.map.header.local_height],
        "navigation":super::bridge_test_evidence::navigation_snapshot(
            sim, phase, (51..=67).flat_map(|y| (111..=120).map(move |x| (x,y))),
        ),
        "cells":cells,"entities":entities,
        "rng":{"main":sim.main_rng,"scenario":sim.scenario_rng,"mapgen":sim.mapgen_rng},
    });
    if let Some(root) = std::env::var_os("VERA20K_LOW_BRIDGE_EXPORT") {
        let path = std::path::PathBuf::from(root).with_extension(format!("{phase}.json"));
        serde_json::to_writer_pretty(std::fs::File::create(path).unwrap(), &result).unwrap();
    }
    eprintln!(
        "SHRAPNEL_BOUNDARY {phase} frame{}",
        sim.session.binary_frame
    );
}

#[test]
#[ignore = "requires the physical Shrapnel map and retail SNOW theater assets"]
fn retail_shrapnel_engineer_repairs_authored_water_gap() {
    let mut scene = loaded_shrapnel();
    let pristine = scene.sim().resolved_terrain.as_ref().unwrap().clone();
    export_scene(&scene, "loaded");
    for x in 114..=116 {
        let cell = scene
            .sim()
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(x, 59)
            .unwrap();
        assert_eq!(cell.bridge_facts.overlay_id, Some(101));
        assert_eq!(cell.yr_cell_land_type, LandType::Water.as_index());
        assert!(!cell.bridge_facts.has_structural_bridge());
    }
    let runtime = &mut scene.runtime;
    let hut = runtime
        .simulation
        .entities()
        .values()
        .find_map(|entity| {
            (runtime.simulation.resolve(entity.type_ref()) == "CABHUT"
                && (entity.position.rx, entity.position.ry) == (117, 56))
                .then_some(entity.stable_id())
        })
        .unwrap();
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let engineer = runtime
        .simulation
        .spawn_object(
            "ENGINEER",
            &owner_name,
            117,
            55,
            0,
            &runtime.resources.rules,
        )
        .expect("ordinary Engineer placement on the hut's bank");
    let command = CommandEnvelope::new(
        owner,
        runtime.simulation.session.tick + 1,
        Command::CaptureBuilding {
            engineer_id: engineer,
            target_building_id: hut,
        },
    );
    export_scene(&scene, "before_command");
    let mut consumed = false;
    let mut approach_snapshot = None;
    for frame in 0..1200 {
        scene
            .runtime
            .advance_frame_for_tooling(
                if frame == 0 {
                    std::slice::from_ref(&command)
                } else {
                    &[]
                },
                SIM_TICK_MS,
            )
            .unwrap();
        if frame == 9 {
            approach_snapshot = Some(save_scene(&scene, "Shrapnel Engineer approaching hut"));
        }
        let actor = scene.sim().entities().get(engineer);
        if actor.is_none_or(|entity| !entity.lifecycle.object_alive) {
            consumed = true;
            eprintln!("SHRAPNEL_ENGINEER consumed after {} frames", frame + 1);
            break;
        }
    }
    export_scene(&scene, "after_approach");
    assert!(
        consumed,
        "ordinary Engineer must physically enter and be consumed"
    );
    for x in 114..=116 {
        let cell = scene
            .sim()
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(x, 59)
            .unwrap();
        assert!(
            cell.bridge_facts
                .overlay_id
                .is_some_and(|overlay| (83..=86).contains(&overlay))
        );
        assert_eq!(cell.yr_cell_land_type, LandType::Road.as_index());
        assert!(!cell.bridge_facts.has_structural_bridge());
    }
    let repaired_snapshot = save_scene(&scene, "Shrapnel low bridge repaired");
    cross_repaired_road(&mut scene);
    export_scene(&scene, "crossed");

    // Native load reseeds Scenario RNG; compare two independently restored
    // futures, retaining the saved navigation and MapGen state in both.
    for (name, bytes) in [
        ("approach", approach_snapshot.unwrap()),
        ("repaired", repaired_snapshot),
    ] {
        let first = super::bridge_test_evidence::restored_retail(&scene, &pristine, &bytes);
        let mut second = super::bridge_test_evidence::restored_retail(&scene, &pristine, &bytes);
        scene.runtime.simulation = first;
        for frame in 0..120 {
            for _ in 0..2 {
                scene
                    .runtime
                    .advance_frame_for_tooling(&[], SIM_TICK_MS)
                    .unwrap();
                std::mem::swap(&mut scene.runtime.simulation, &mut second);
            }
            assert_eq!(
                scene.sim().state_hash(),
                second.state_hash(),
                "{name} restored frame{frame}"
            );
        }
        assert!(scene.sim().entities().get(engineer).is_none());
        for x in 114..=116 {
            let cell = scene
                .sim()
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(x, 59)
                .unwrap();
            assert_eq!(cell.yr_cell_land_type, LandType::Road.as_index());
            assert!(!cell.bridge_facts.has_structural_bridge());
        }
        cross_repaired_road(&mut scene);
    }
}

fn cross_repaired_road(scene: &mut HeadlessScenario) {
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let fv = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "FV",
            &owner_name,
            115,
            55,
            0,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .expect("ordinary FV on the north road");
    let command = CommandEnvelope::new(
        owner,
        runtime.simulation.session.tick + 1,
        Command::Move {
            entity_id: fv,
            target_rx: 115,
            target_ry: 63,
            queue: false,
        },
    );
    let mut crossed_gap = false;
    for frame in 0..600 {
        runtime
            .advance_frame_for_tooling(
                if frame == 0 {
                    std::slice::from_ref(&command)
                } else {
                    &[]
                },
                SIM_TICK_MS,
            )
            .unwrap();
        let actor = runtime.simulation.entities().get(fv).unwrap();
        assert!(
            !actor.on_bridge,
            "low road must not create raised-deck occupation"
        );
        assert!(actor.low_bridge_tube_state.is_none());
        crossed_gap |= (114..=116).contains(&actor.position.rx) && actor.position.ry == 59;
        if (actor.position.rx, actor.position.ry) == (115, 63) && actor.movement_target.is_none() {
            assert!(crossed_gap, "the FV must physically cross the repaired row");
            assert_eq!(actor.position.exact_z_leptons, Some(208));
            assert!(
                runtime
                    .simulation
                    .cell_objects((115, 63), MovementLayer::Ground)
                    .any(|object| object == crate::sim::occupancy::CellObjectMember::Entity(fv))
            );
            eprintln!("SHRAPNEL_FV crossed after {} frames", frame + 1);
            return;
        }
    }
    panic!("FV did not finish ordinary movement across the repaired low road");
}

#[test]
#[ignore = "physical Shrapnel naval head meeting ordinary Engineer repair"]
fn retail_shrapnel_repair_reaches_moving_water_neighbor() {
    use crate::sim::world::{LifecycleOutput, LifecycleTestEvent, TickLane};

    let head_native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_head_producer.json",
    ))
    .unwrap();
    let head_input = &head_native["input"];
    let head_process = &head_native["result"]["stages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stage| stage["stage"] == "process")
        .unwrap()["state"];
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_lifetime_cleanup.json",
    ))
    .unwrap();
    let native = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["input"]["name"] == "west_water_head_road_dz0"
                && row["input"]["moving_input"] == true
        })
        .unwrap();
    let sink: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_sink_tick.json",
    ))
    .unwrap();
    let suffix = |name| {
        &sink["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == name)
            .unwrap()["output"]
    };
    let ground_z = native["after_damage"]["coord"][2].as_i64().unwrap() as i32;
    let fall_per_visit =
        suffix("surface_frame_1001")["coord"][2].as_i64().unwrap() as i32 - ground_z;
    let last_live_z = suffix("threshold_z-395_frame1001")["coord"][2]
        .as_i64()
        .unwrap() as i32;

    let mut scene = loaded_shrapnel();
    let pristine = scene.sim().resolved_terrain.as_ref().unwrap().clone();
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let hut = runtime
        .simulation
        .entities()
        .values()
        .find_map(|entity| {
            (runtime.simulation.resolve(entity.type_ref()) == "CABHUT"
                && (entity.position.rx, entity.position.ry) == (117, 56))
                .then_some(entity.stable_id())
        })
        .unwrap();
    let engineer = runtime
        .simulation
        .spawn_object(
            "ENGINEER",
            &owner_name,
            117,
            55,
            0,
            &runtime.resources.rules,
        )
        .unwrap();
    let ship = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "AEGIS",
            &owner_name,
            113,
            59,
            64,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .expect("ordinary AEGIS placement in the untouched water lane");
    let ship_type = runtime.simulation.entities().get(ship).unwrap().type_ref();
    let actor = runtime.simulation.entities().get(ship).unwrap();
    let object = runtime.resources.rules.object("AEGIS").unwrap();
    let readers = head_native["movement_readers"]
        .as_array()
        .unwrap()
        .last()
        .unwrap();
    // Speed's existing production projection stores leptons/second, while
    // native71464A stores the 15 Hz integer. Compare through that owner.
    let speed = crate::sim::combat::veterancy::entity_mover_speed_leptons_per_second(
        actor,
        Some(object),
        object.speed,
        runtime.resources.rules.general.veteran_speed,
        crate::sim::movement::owner_speed_bonus(&runtime.simulation.houses, actor, Some(object)),
    );
    assert_eq!(
        i64::from(crate::sim::movement::owner_current_speed_from_fraction(
            speed,
            crate::util::fixed_math::SIM_ONE,
        )),
        readers["speed"]
    );
    assert_eq!(i64::from(object.turret_rot), readers["rot"]);
    assert_eq!(
        i64::from(runtime.resources.rules.general.close_enough),
        readers["close_enough"]
    );
    let path_delay_bytes: String = runtime
        .resources
        .rules
        .general
        .path_delay
        .to_le_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(
        path_delay_bytes,
        readers["path_delay_bits"].as_str().unwrap()
    );
    let ships = |sim: &crate::sim::world::Simulation| {
        sim.houses[&owner]
            .tracking
            .active_count(crate::map::entities::EntityCategory::Unit, ship_type)
    };
    let other_ships = ships(&runtime.simulation) - 1;
    let initial_losses = runtime.simulation.houses[&owner].stats.units_lost();
    let ship_delay: usize = std::env::var("VERA20K_LOW_NAVAL_DELAY")
        .ok()
        .map(|value| value.parse().unwrap())
        .unwrap_or_else(|| usize::try_from(head_input["order_frame"].as_u64().unwrap()).unwrap());
    let mut trace = Vec::new();
    let mut repair_frame = None;
    let mut terminal_frame = None;
    let mut active_snapshot = None;
    let mut retained_track = None;
    let mut saw_last_live_height = false;
    for frame in 0..150 {
        let visit_frame = scene.sim().session.binary_frame;
        if frame == ship_delay {
            // The native interval begins at Unit741970 after command
            // dispatch. Match its explicit inputs at our ordinary order edge;
            // preceding scene frames and RNG seeds are separate witnesses.
            let actor = scene.sim().entities().get(ship).unwrap();
            let coord = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
            assert_eq!(u64::from(visit_frame), head_input["order_frame"]);
            assert_eq!(
                json!([actor.position.rx, actor.position.ry]),
                head_input["source_cell"]
            );
            assert_eq!(json!([coord.x, coord.y, coord.z]), head_input["source_xyz"]);
            assert_eq!(
                u64::from(actor.body_facing_current(visit_frame)),
                head_input["facing_raw"]
            );
            assert_eq!(actor.lifecycle.cell_marked, head_input["initial_marked"]);
            assert!(
                actor
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_ship_runtime())
                    .and_then(|r| r.retained())
                    .unwrap()
                    .head_to()
                    .is_none()
            );
            assert_eq!(head_input["initial_head"], json!([0, 0, 0]));
        }
        let command = if frame == 0 {
            Some(Command::CaptureBuilding {
                engineer_id: engineer,
                target_building_id: hut,
            })
        } else if frame == ship_delay {
            Some(Command::Move {
                entity_id: ship,
                target_rx: u16::try_from(head_input["destination_cell"][0].as_u64().unwrap())
                    .unwrap(),
                target_ry: u16::try_from(head_input["destination_cell"][1].as_u64().unwrap())
                    .unwrap(),
                queue: false,
            })
        } else {
            None
        };
        let envelope = command
            .map(|command| CommandEnvelope::new(owner, scene.sim().session.tick + 1, command));
        let before = scene.sim().entities().get(ship).map(|e| {
            json!({
                "position":e.position,"ship":e.locomotor.as_ref().and_then(|l| l.selected_ship_runtime()).and_then(|r| r.retained()).cloned(),"lifecycle":e.lifecycle,
                "health":e.health,"entity":e,
            })
        });
        let before_coord = scene
            .sim()
            .entities()
            .get(ship)
            .map(|e| crate::sim::movement::ground_pose::position_world_coord(&e.position));
        let lifetime_start = scene.sim().lifecycle_test_events_for_test().len();
        let output = scene
            .runtime
            .advance_frame(envelope.as_slice(), SIM_TICK_MS, TickLane::Ordinary)
            .unwrap();
        let lifetime = &scene.sim().lifecycle_test_events_for_test()[lifetime_start..];
        let actor = scene.sim().entities().get(ship);
        let engineer_present = scene.sim().entities().get(engineer).is_some();
        trace.push(json!({
            "frame":frame,"binary_frame":scene.sim().session.binary_frame,
            "before":before,"after":actor,"engineer_present":engineer_present,
            "owner_losses":scene.sim().houses[&owner].stats.units_lost(),
            "lifecycle":format!("{lifetime:?}"),
            "fire_sources":output.fire_events.iter().map(|event|event.attacker_id).collect::<Vec<_>>(),
        }));
        assert!(
            output
                .fire_events
                .iter()
                .all(|event| event.attacker_id != ship)
        );
        if let Some(actor) = actor {
            if frame == ship_delay + 1 {
                // Full native Ship69FC10 -> Foot4D3920 -> AStar429A90
                // produces this first head without supplied path answers.
                // Compare the live logical queue, not its unused raw suffix.
                let loco = actor
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_ship_runtime())
                    .and_then(|r| r.retained())
                    .unwrap();
                let head = loco.head_to().unwrap();
                let destination = loco.destination().unwrap();
                let coord =
                    crate::sim::movement::ground_pose::position_world_coord(&actor.position);
                let route: Vec<u8> = head_process["path"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .take_while(|direction| direction.as_i64() != Some(-1))
                    .map(|direction| u8::try_from(direction.as_u64().unwrap()).unwrap())
                    .collect();
                assert_eq!(u64::from(visit_frame), head_input["process_frame"]);
                assert_eq!(json!([head.x, head.y, head.z]), head_process["head"]);
                assert_eq!(json!([coord.x, coord.y, coord.z]), head_process["xyz"]);
                assert_eq!(
                    json!([destination.x, destination.y, destination.z]),
                    head_process["destination"]
                );
                assert_eq!(actor.navigation.path_replay.remaining_directions(), route);
                assert_eq!(
                    i64::from(loco.track().turn_index),
                    head_process["track_selector"]
                );
                assert_eq!(i64::from(loco.track().cursor), head_process["track_cursor"]);
                assert_eq!(u64::from(loco.track_valid()), head_process["head_valid"]);
                assert_eq!(
                    loco.target_speed_fraction().to_num::<f64>(),
                    head_process["fraction"].as_f64().unwrap()
                );
                let timer = actor.navigation.path_runtime.movement_timer;
                assert_eq!(
                    i64::from(timer.start_frame()),
                    head_process["movement_timer"][0]
                );
                assert_eq!(
                    i64::from(timer.duration()),
                    head_process["movement_timer"][2]
                );
                assert_eq!(
                    u64::from(actor.navigation.path_runtime.retries_left),
                    head_process["retries"]
                );
                assert_eq!(
                    u64::from(actor.lifecycle.cell_marked),
                    head_process["marked"]
                );
                assert_eq!(
                    actor.health.current,
                    native["initial"]["health"].as_i64().unwrap() as i32
                );
            }
            if actor.sinking.is_active() {
                assert!(!engineer_present);
                assert_retained_naval_ship(
                    scene.sim(),
                    ship,
                    &native["after_damage"],
                    initial_losses,
                );
                let loco = actor
                    .locomotor
                    .as_ref()
                    .and_then(|l| l.selected_ship_runtime())
                    .and_then(|r| r.retained())
                    .unwrap();
                let coord =
                    crate::sim::movement::ground_pose::position_world_coord(&actor.position);
                if repair_frame.is_none() {
                    repair_frame = Some(frame);
                    retained_track = Some(loco.track());
                    assert_eq!(
                        json!([coord.x, coord.y, coord.z]),
                        native["after_damage"]["coord"]
                    );
                    let bytes = save_scene(&scene, "Shrapnel repaired bridge: AEGIS sinking");
                    if let Some(root) = std::env::var_os("VERA20K_LOW_BRIDGE_EXPORT") {
                        std::fs::write(
                            std::path::PathBuf::from(root).with_extension("naval-sinking.bin"),
                            &bytes,
                        )
                        .unwrap();
                    }
                    active_snapshot = Some(bytes);
                    export_scene(&scene, "naval_sinking");
                } else {
                    let before = before_coord.unwrap();
                    assert_eq!((coord.x, coord.y), (before.x, before.y));
                    assert_eq!(coord.z - before.z, fall_per_visit, "frame{frame}");
                }
                assert_eq!(
                    Some(loco.track()),
                    retained_track,
                    "sinking bypasses Process"
                );
                saw_last_live_height |= coord.z == last_live_z;
                assert_eq!(
                    ships(scene.sim()) - other_ships,
                    native["after_damage"]["owner_type_count"].as_i64().unwrap() as i32
                );
            }
        } else if terminal_frame.is_none() {
            assert!(
                repair_frame.is_some(),
                "repair must retain the live hull first"
            );
            terminal_frame = Some(frame);
            assert_eq!(before_coord.unwrap().z, last_live_z);
            let terminal = &native["terminal"]["state"];
            assert_eq!(
                lifetime
                    .iter()
                    .filter(|event| **event
                        == LifecycleTestEvent::UninitAliveCleared { stable_id: ship })
                    .count(),
                usize::from(terminal["alive"] == 0)
            );
            assert_eq!(
                lifetime
                    .iter()
                    .filter(|event| **event
                        == LifecycleTestEvent::PendingDeleteQueued { stable_id: ship })
                    .count(),
                terminal["deferred_count"].as_u64().unwrap() as usize
            );
            assert!(lifetime.contains(&LifecycleTestEvent::ConcealLimboSet));
            assert_eq!(terminal["limbo"], 1);
            assert!(
                output
                    .lifecycle_outputs
                    .contains(&LifecycleOutput::DisplayRemove { stable_id: ship })
            );
            // Native cleanup stops after UnInit; the same ordinary Rust frame
            // additionally runs its production pending-delete drain.
            assert!(lifetime.contains(&LifecycleTestEvent::FinalizedCommon { stable_id: ship }));
        }
        if terminal_frame.is_some() {
            assert_naval_cleanup(
                scene.sim(),
                ship,
                owner,
                &native["terminal"]["state"],
                initial_losses,
            );
            assert_eq!(
                ships(scene.sim()) - other_ships,
                native["terminal"]["state"]["owner_type_count"]
                    .as_i64()
                    .unwrap() as i32
            );
        }
    }
    if let Some(root) = std::env::var_os("VERA20K_LOW_BRIDGE_EXPORT") {
        let path =
            std::path::PathBuf::from(root).with_extension(format!("naval-delay{ship_delay}.json"));
        serde_json::to_writer(std::fs::File::create(path).unwrap(), &trace).unwrap();
    }
    assert!(scene.sim().entities().get(engineer).is_none());
    assert!(
        saw_last_live_height,
        "native threshold remains live at relative -400"
    );
    assert_eq!(
        repair_frame,
        Some(29),
        "ordinary-command Rust frame regression"
    );
    assert_eq!(
        terminal_frame,
        Some(110),
        "ordinary-command Rust frame regression"
    );

    // Native load restarts Scenario at zero. Compare independently restored
    // futures, retaining the active hull, counters and all non-reset authority.
    let bytes = active_snapshot.unwrap();
    let first = super::bridge_test_evidence::restored_retail(&scene, &pristine, &bytes);
    let mut second = super::bridge_test_evidence::restored_retail(&scene, &pristine, &bytes);
    scene.runtime.simulation = first;
    assert_retained_naval_ship(scene.sim(), ship, &native["after_damage"], initial_losses);
    assert_retained_naval_ship(&second, ship, &native["after_damage"], initial_losses);
    assert_eq!(
        scene.sim().scenario_rng.logical_state(),
        crate::sim::rng::SimRng::new(0).logical_state()
    );
    for frame in 0..120 {
        for _ in 0..2 {
            let output = scene
                .runtime
                .advance_frame(&[], SIM_TICK_MS, TickLane::Ordinary)
                .unwrap();
            assert!(
                output
                    .fire_events
                    .iter()
                    .all(|event| event.attacker_id != ship)
            );
            std::mem::swap(&mut scene.runtime.simulation, &mut second);
        }
        assert_eq!(
            scene.sim().state_hash(),
            second.state_hash(),
            "restored frame{frame}"
        );
        assert_eq!(
            scene.sim().rng_state(),
            second.rng_state(),
            "restored frame{frame}"
        );
        assert_eq!(
            scene.sim().houses[&owner].stats,
            second.houses[&owner].stats
        );
        if scene.sim().entities().contains(ship) {
            assert_retained_naval_ship(scene.sim(), ship, &native["after_damage"], initial_losses);
        } else {
            assert_naval_cleanup(
                scene.sim(),
                ship,
                owner,
                &native["terminal"]["state"],
                initial_losses,
            );
        }
    }
    assert!(!scene.sim().entities().contains(ship));
    assert!(!second.entities().contains(ship));
}

fn assert_retained_naval_ship(
    sim: &crate::sim::world::Simulation,
    id: u64,
    native: &Value,
    initial_losses: u32,
) {
    use crate::sim::world::display_layers::DisplayLayer;
    let actor = sim.entities().get(id).unwrap();
    assert_eq!(
        actor.health.current,
        native["health"].as_i64().unwrap() as i32
    );
    assert_eq!(actor.lifecycle.object_alive, native["alive"] == 1);
    assert_eq!(actor.lifecycle.in_limbo, native["limbo"] == 1);
    assert_eq!(actor.lifecycle.cell_marked, native["marked"] == 1);
    assert_eq!(actor.sinking.is_active(), native["sinking"] == 1);
    assert_eq!(actor.in_logic_vector, native["logic_registered"] == 1);
    assert_eq!(
        sim.logic_order()
            .iter()
            .filter(|&&member| member == id)
            .count(),
        native["logic_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        sim.display_layers().layer_of(id),
        DisplayLayer::from_index(native["layer"].as_u64().unwrap() as u8)
    );
    assert_eq!(
        sim.display_layers()
            .ordered_ids()
            .filter(|&&member| member == id)
            .count(),
        native["display_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        sim.substrate.pending_delete.contains(&id),
        native["deferred_is_self"].as_bool().unwrap()
    );
    let coord = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
    assert_eq!(
        json!([coord.x, coord.y]),
        json!([native["coord"][0], native["coord"][1]])
    );
    let loco = actor
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_ship_runtime())
        .and_then(|r| r.retained())
        .unwrap();
    let head = loco.head_to().unwrap();
    assert_eq!(json!([head.x, head.y, head.z]), native["head"]);
    assert!(loco.destination().is_none());
    assert_eq!(native["destination"], json!([0, 0, 0]));
    assert_eq!(
        sim.houses[&actor.owner()].stats.units_lost() - initial_losses,
        native["owner_losses"].as_u64().unwrap() as u32
    );
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(113, 59)
            .unwrap()
            .yr_cell_land_type,
        native["land"].as_u64().unwrap() as u8
    );
    assert_no_naval_cell_member(sim, id);
}

fn assert_no_naval_cell_member(sim: &crate::sim::world::Simulation, id: u64) {
    // Mark(UP) removes CellClass list membership; the retained Ship head-to
    // reservation is independent and must not be cleared by this assertion.
    for x in 113..=117 {
        assert!(
            !sim.cell_objects((x, 59), MovementLayer::Ground)
                .any(|member| { member == crate::sim::occupancy::CellObjectMember::Entity(id) })
        );
    }
}

fn assert_naval_cleanup(
    sim: &crate::sim::world::Simulation,
    id: u64,
    owner: crate::sim::intern::InternedId,
    native: &Value,
    initial_losses: u32,
) {
    assert_eq!(
        sim.logic_order()
            .iter()
            .filter(|&&member| member == id)
            .count(),
        native["logic_count"].as_u64().unwrap() as usize
    );
    assert_eq!(
        sim.display_layers()
            .ordered_ids()
            .filter(|&&member| member == id)
            .count(),
        native["display_count"].as_u64().unwrap() as usize
    );
    assert!(sim.display_layers().layer_of(id).is_none());
    assert_eq!(native["layer"], -1);
    assert_eq!(
        sim.houses[&owner].stats.units_lost() - initial_losses,
        native["owner_losses"].as_u64().unwrap() as u32
    );
    assert!(!sim.entities().contains(id));
    assert!(!sim.substrate.pending_delete.contains(&id));
    assert_no_naval_cell_member(sim, id);
}
