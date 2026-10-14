//! Physical Hills Engineer -> CABHUT entry and retained zone evidence.
use super::bridge_target_layer_tests::retail_hills_collapsed_scene;
use super::bridge_test_evidence::restored_retail;
use crate::sim::command::Command;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::Simulation;
use serde_json::{Value, json};

impl Simulation {
    /// Optional evidence export at the real Foot Mark0/AStar boundary. Reads
    /// retained fields directly: no extra Map queries or altered actor state.
    pub(crate) fn export_bridge_engineer_entry_inputs(
        &self,
        id: u64,
        rules: &crate::rules::ruleset::RuleSet,
        boundary: &str,
    ) {
        use crate::sim::movement::locomotor::MovementLayer::{Bridge, Ground};
        let Some(path) = std::env::var_os("VERA20K_BRIDGE_ZONE_EXPORT") else {
            return;
        };
        let Some(actor) = self.entities().get(id) else {
            return;
        };
        if self.interner.resolve(actor.type_ref()) != "ENGINEER"
            || ![(66, 76), (67, 75)].contains(&(actor.position.rx, actor.position.ry))
        {
            return;
        }
        let terrain = self.resolved_terrain.as_ref().unwrap();
        let entities: Vec<_> = self
            .entities()
            .values()
            .filter(|e| {
                e.stable_id() == id
                    || ((64..=70).contains(&e.position.rx) && (72..=78).contains(&e.position.ry))
            })
            .map(|e| {
                json!({"id":e.stable_id(),"type_name":self.interner.resolve(e.type_ref()),
                    "owner_name":self.interner.resolve(e.owner()),"entity":e,
                    "team_present":self.team_script_vm.team_for_member(e.stable_id()).is_some(),
                    "type":format!("{:?}",rules.object(self.interner.resolve(e.type_ref()))),
                })
            })
            .collect();
        let mut cells = Vec::new();
        for y in 72..=78 {
            for x in 64..=70 {
                let c = terrain.cell(x, y).unwrap();
                let ground: Vec<_> = self
                    .substrate
                    .occupancy
                    .cell_objects(
                        x,
                        y,
                        Ground,
                        self.production.terrain_object_cells.get(&(x, y)).copied(),
                    )
                    .map(|o| format!("{o:?}"))
                    .collect();
                let deck: Vec<_> = self
                    .substrate
                    .occupancy
                    .cell_objects(x, y, Bridge, None)
                    .map(|o| format!("{o:?}"))
                    .collect();
                cells.push(json!({"coord":[x,y],"level":c.level,"slope":c.slope_type,
                    "land":c.yr_cell_land_type,"tile":c.final_tile_index,"subtile":c.final_sub_tile,
                    "zone_type":c.zone_type,"bridge_facts":c.bridge_facts,
                    "ground_list":ground,"deck_list":deck,
                    "ground_bits":self.substrate.raw_cell_occupation.ground_bits(x,y),
                    "deck_bits":self.substrate.raw_cell_occupation.deck_bits(x,y),
                    "ground_owner":self.substrate.raw_cell_occupation.infantry_owner(x,y,Ground),
                    "deck_owner":self.substrate.raw_cell_occupation.infantry_owner(x,y,Bridge),
                    "full_cell_debug":format!("{c:?}"),
                }));
            }
        }
        let bounds = self.playfield_bounds.unwrap();
        let output = json!({"boundary":boundary, "actor":id,
            "tick":self.session.tick,"binary_frame":self.session.binary_frame,
            "native_size":[bounds.base,self.playfield_size_height.unwrap()],
            "local_size":[bounds.off_fc,bounds.off_100,bounds.off_104,bounds.off_108],
            "entities":entities,"cells":cells,"dummy":format!("{:?}",terrain.shared_cell_dummy().snapshot()),
        });
        serde_json::to_writer(
            std::fs::File::create(
                std::path::PathBuf::from(path)
                    .with_extension(format!("{boundary}-{}.json", self.session.binary_frame)),
            )
            .unwrap(),
            &output,
        )
        .unwrap();
    }
}

fn zone_snapshot(sim: &Simulation, name: &str) -> Value {
    super::bridge_test_evidence::navigation_snapshot(
        sim,
        name,
        [(68, 74), (69, 74), (64, 69), (66, 69)],
    )
}

fn capture_probe(
    sim: &mut Simulation,
    resources: &crate::sim::runtime::SimResources,
    engineer: u64,
    hut: u64,
    name: &str,
) -> Value {
    let actor = sim.entities().get(engineer).unwrap();
    let owner = sim.interner.resolve(actor.owner()).to_owned();
    assert!(sim.apply_command_with_overlays(
        &owner,
        &Command::CaptureBuilding {
            engineer_id: engineer,
            target_building_id: hut
        },
        Some(&resources.rules),
        Some(&resources.overlay_registry),
        crate::sim::world::FrameEffects::default(),
    ));
    let mut snapshot = zone_snapshot(sim, name);
    let actor = sim.entities().get(engineer).unwrap();
    let object = resources
        .rules
        .object(sim.interner.resolve(actor.type_ref()))
        .unwrap();
    let destination = actor
        .locomotor
        .as_ref()
        .unwrap()
        .walk_destination()
        .unwrap();
    let navigation_coord = sim.foot_navigation_coordinate(engineer).unwrap();
    let query = json!({
        "actor": engineer, "hut":hut, "type":sim.interner.resolve(actor.type_ref()),
        "movement_zone": object.movement_zone.matrix_row(), "speed_type":format!("{:?}",object.speed_type),
        "position":actor.position, "on_bridge":actor.on_bridge, "in_playfield":actor.in_playfield,
        "mission_only":actor.is_mission_only(), "lifecycle":actor.lifecycle,
        "tube":actor.low_bridge_tube_state, "locomotor":actor.locomotor,
        "mission":actor.mission, "navigation":actor.navigation,
        "team_present":sim.team_script_vm.team_for_member(engineer).is_some(),
        "navigation_coord":navigation_coord, "destination_coord":destination,
        "current_coord":crate::sim::movement::ground_pose::position_world_coord(&actor.position),
        "allow_destination_fringe":sim.foot_allows_outside_playfield(engineer).unwrap(),
    });
    eprintln!("BRIDGE_ENGINEER_PROBE {name}: {query}");
    let before = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .shared_cell_dummy()
        .snapshot();
    let connected = sim
        .foot_path_zone_precheck(engineer, destination, &resources.rules)
        .unwrap();
    snapshot["queries"] = json!([{ "input":query, "connected":connected,
        "dummy_before":format!("{before:?}"),
        "dummy_after":format!("{:?}",sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy().snapshot()),
    }]);
    snapshot
}

#[test]
#[ignore = "physical Hills production zone/Engineer evidence; requires retail assets"]
fn retail_hills_engineer_cliff_start_is_rejected() {
    let mut snapshots = Vec::new();
    let (mut scenario, pristine) =
        retail_hills_collapsed_scene(|sim, name| snapshots.push(zone_snapshot(sim, name)));
    let runtime = &mut scenario.runtime;
    let hut = runtime
        .simulation
        .entities()
        .values()
        .find_map(|e| {
            (runtime.simulation.interner.resolve(e.type_ref()) == "CABHUT"
                && (e.position.rx, e.position.ry) == (68, 74))
                .then_some(e.stable_id())
        })
        .unwrap();
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.interner.resolve(owner).to_owned();
    // This zone/restore negative control supplies an already-resident actor
    // on Rock, whose native Foot speed is zero. Object5F4F1B..5F4F4A
    // bypasses ordinary +1AC admission under the authored A8E7AC scope;
    // Infantry51E027 also uses it for priority floor placement. End that
    // context before snapshot/Capture: ordinary cliff Unlimbo is not legal.
    let engineer = runtime.simulation.with_object_placement_scope(|sim| {
        sim.spawn_object_with_overlay_registry(
            "ENGINEER",
            &owner_name,
            69,
            74,
            0,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .unwrap()
    });
    assert!(!runtime.simulation.object_placement_scope_active());
    let bytes = GameSnapshot::save_validated(
        &runtime.simulation,
        scenario.map.ini.content_hash(),
        runtime.resources.rules.simulation_config_hash(),
        "Engineer zone entry evidence",
        0,
    );
    let mut restored = restored_retail(&scenario, &pristine, &bytes);
    snapshots.push(capture_probe(
        &mut scenario.runtime.simulation,
        &scenario.runtime.resources,
        engineer,
        hut,
        "live_post_collapse_capture",
    ));
    snapshots.push(capture_probe(
        &mut restored,
        &scenario.runtime.resources,
        engineer,
        hut,
        "reconstructed_post_collapse_capture",
    ));
    let header = &scenario.map.header;
    let output = json!({
        "schema_version":1,"native_size":[header.width,header.height],
        "width":scenario.sim().zone_grid.as_ref().unwrap().width,
        "height":scenario.sim().zone_grid.as_ref().unwrap().height,
        "local_size":[header.local_left,header.local_top,header.local_width,header.local_height],
        "map_hash":format!("{:016x}",scenario.map.ini.content_hash()),
        "rules_hash":format!("{:016x}",scenario.runtime.resources.rules.simulation_config_hash()),
        "snapshots":snapshots,
    });
    if let Some(path) = std::env::var_os("VERA20K_BRIDGE_ZONE_EXPORT") {
        let file = std::fs::File::create(path).unwrap();
        serde_json::to_writer(file, &output).unwrap();
    }
    for snapshot in output["snapshots"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s.get("queries").is_some())
    {
        assert_eq!(
            snapshot["queries"][0]["connected"],
            json!(false),
            "{}",
            snapshot["name"]
        );
    }
}

#[test]
#[ignore = "physical Hills ordinary Engineer approach, entry and repair"]
fn retail_hills_engineer_enters_hut_and_repairs() {
    use crate::sim::command::CommandEnvelope;
    use crate::sim::world::TickLane;

    let (mut scenario, pristine) = retail_hills_collapsed_scene(|_, _| {});
    let retail = std::path::PathBuf::from(std::env::var_os("RA2_DIR").unwrap());
    let mut assets = crate::assets::asset_manager::AssetManager::new(
        &retail,
        crate::assets::asset_manager::MediaArchiveMode::STOCK_DIGITAL,
    )
    .unwrap();
    let theater =
        crate::map::theater::load_theater(&mut assets, &scenario.map.header.theater).unwrap();
    eprintln!(
        "HILLS_SOURCE_BINDING {}",
        json!({
            "theater": scenario.map.header.theater,
            "tile":69,"subtile":4,"filename":theater.lookup.filename(69),
            "tileset":theater.lookup.tileset_index(69),
        })
    );
    let runtime = &mut scenario.runtime;
    assert_eq!(
        runtime
            .resources
            .rules
            .object("ENGINEER")
            .unwrap()
            .speed_type,
        crate::rules::locomotor_type::SpeedType::Foot,
        "original5236A0 constructor and layered7121D1 SpeedType reads"
    );
    let hut = runtime
        .simulation
        .entities()
        .values()
        .find_map(|e| {
            (runtime.simulation.interner.resolve(e.type_ref()) == "CABHUT"
                && (e.position.rx, e.position.ry) == (68, 74))
                .then_some(e.stable_id())
        })
        .unwrap();
    let start = (66, 76);
    let cell = runtime
        .simulation
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(start.0, start.1)
        .unwrap();
    assert_eq!((cell.zone_type, cell.level), (0, 10));
    assert!(!cell.ground_walk_blocked);
    assert_eq!(
        runtime
            .simulation
            .substrate
            .occupancy
            .first_building_on_layer(
                start.0,
                start.1,
                crate::sim::movement::locomotor::MovementLayer::Ground
            ),
        None
    );
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.interner.resolve(owner).to_owned();
    let engineer = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "ENGINEER",
            &owner_name,
            start.0,
            start.1,
            0,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .unwrap();
    let command = CommandEnvelope::new(
        owner,
        runtime.simulation.session.tick + 1,
        Command::CaptureBuilding {
            engineer_id: engineer,
            target_building_id: hut,
        },
    );
    let mut trace = Vec::new();
    let mut previous = Value::Null;
    let mut repaired = false;
    let mut approach_snapshot = None;
    for frame in 0..1200 {
        runtime
            .advance_frame(
                if frame == 0 {
                    std::slice::from_ref(&command)
                } else {
                    &[]
                },
                crate::headless_scenario::SIM_TICK_MS,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .unwrap();
        let actor = runtime.simulation.entities().get(engineer);
        if approach_snapshot.is_none()
            && actor.is_some_and(|e| (e.position.rx, e.position.ry) == (67, 75))
        {
            approach_snapshot = Some((
                GameSnapshot::save_validated(
                    &runtime.simulation,
                    scenario.map.ini.content_hash(),
                    runtime.resources.rules.simulation_config_hash(),
                    "Engineer approaching bridge hut",
                    0,
                ),
                engineer_navigation_state(actor.unwrap()),
            ));
        }
        let mut state = actor.map_or(Value::Null, |e| {
            json!({
                "position":e.position,"lifecycle":e.lifecycle,"mission":e.mission,
                "navigation":e.navigation,"locomotor":e.locomotor,
            })
        });
        if let Some(mission) = state.get_mut("mission").and_then(Value::as_object_mut) {
            mission.remove("ai_counter");
        }
        if state != previous {
            eprintln!("HILLS_ENGINEER_FRAME {frame}: {state}");
            trace.push(json!({"frame":frame,"state":state}));
            previous = state;
        }
        repaired = runtime
            .simulation
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(64, 69)
            .unwrap()
            .bridge_facts
            .has_structural_bridge();
        if repaired && actor.is_none_or(|e| !e.lifecycle.object_alive()) {
            break;
        }
    }
    if let Some(path) = std::env::var_os("VERA20K_BRIDGE_ZONE_EXPORT") {
        serde_json::to_writer(
            std::fs::File::create(
                std::path::PathBuf::from(path).with_extension("entry-trace.json"),
            )
            .unwrap(),
            &trace,
        )
        .unwrap();
    }
    assert!(repaired, "ordinary Engineer must rebuild the actual bridge");
    assert!(
        runtime
            .simulation
            .entities()
            .get(engineer)
            .is_none_or(|e| !e.lifecycle.object_alive()),
        "repair consumes the Engineer"
    );
    assert_repaired_hills_authorities(&runtime.simulation, engineer, hut);

    let (bytes, saved_actor) = approach_snapshot.expect("Engineer crossed the legal approach");
    let first = restored_retail(&scenario, &pristine, &bytes);
    let mut second = restored_retail(&scenario, &pristine, &bytes);
    for restored in [&first, &second] {
        assert_eq!(
            engineer_navigation_state(restored.entities().get(engineer).unwrap()),
            saved_actor,
            "save/load retains the in-flight Engineer order, path and Walk head"
        );
    }
    // Native load reseeds Scenario RNG. Compare two independently rebuilt
    // continuations, not a restored future against the uninterrupted one.
    scenario.runtime.simulation = first;
    let mut restored_repaired = false;
    for frame in 0..1200 {
        let runtime = &mut scenario.runtime;
        for _ in 0..2 {
            runtime
                .advance_frame(
                    &[],
                    crate::headless_scenario::SIM_TICK_MS,
                    TickLane::Ordinary,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
            std::mem::swap(&mut runtime.simulation, &mut second);
        }
        assert_eq!(
            runtime.simulation.state_hash(),
            second.state_hash(),
            "restored Engineer continuation frame{frame}"
        );
        if runtime.simulation.entities().get(engineer).is_none() {
            assert_repaired_hills_authorities(&runtime.simulation, engineer, hut);
            assert_repaired_hills_authorities(&second, engineer, hut);
            restored_repaired = true;
            break;
        }
    }
    assert!(restored_repaired, "restored Engineer must enter and repair");
}

fn engineer_navigation_state(actor: &crate::sim::game_entity::GameEntity) -> Value {
    json!({
        "position":actor.position, "mission":actor.mission,
        "navigation":actor.navigation, "locomotor":actor.locomotor,
    })
}

fn assert_repaired_hills_authorities(sim: &Simulation, engineer: u64, hut: u64) {
    let bridges = sim.bridge_state.as_ref().unwrap();
    assert_eq!(
        sim.zone_grid.as_ref().unwrap().bridge_records(),
        bridges.endpoint_records()
    );
    let cell = sim.resolved_terrain.as_ref().unwrap().cell(64, 69).unwrap();
    assert!(cell.bridge_facts.has_structural_bridge());
    assert!(
        sim.path_grid()
            .unwrap()
            .cell(64, 69)
            .unwrap()
            .has_structural_bridge()
    );
    assert!(
        sim.path_grid()
            .unwrap()
            .cell(64, 69)
            .unwrap()
            .bridge_walkable,
        "repaired deck terrain{:?}, path{:?}",
        cell.bridge_facts,
        sim.path_grid().unwrap().cell(64, 69)
    );
    // Original constructor25 writes raw100/state9 on non-anchor side cells
    // while leaving their own overlay at -1; only the center carries25.
    assert_eq!(cell.bridge_facts.overlay_id, None);
    assert!(
        sim.entities().get(engineer).is_none(),
        "Engineer retired after repair"
    );
    assert!(sim.entities().get(hut).unwrap().lifecycle.object_alive());
    for at in [(66, 76), (67, 75), (68, 74)] {
        assert!(
            sim.cell_objects(at, crate::sim::movement::locomotor::MovementLayer::Ground)
                .all(|member| member != crate::sim::occupancy::CellObjectMember::Entity(engineer)),
            "consumed Engineer may not retain a ground-list link at{at:?}"
        );
    }
}
