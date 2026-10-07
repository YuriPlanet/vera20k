//! Original Terrain AI/Spread/Place execution replay. Goldens and binary
//! identity: tools/spatial_oracle/tibtre.{json,md,meta.json}.
use super::harvest_field_oracle_tests::{registry, row_scene_with};
use super::refinery_dock_oracle_tests::{Scene, cell};
use crate::map::overlay::TerrainObject;
use crate::rules::ini_parser::IniFile;
use crate::sim::terrain_spawn::{
    TerrainAnimationState, construct_terrain_objects, seed_terrain_spawner_animation,
};
use crate::sim::tiberium::{
    NewTiberiumAdmission, PlaceTiberiumContext, TiberiumPlacementObjectContext, spread_tiberium,
};
use crate::util::native_x87::NativeF32Bits;
use serde_json::{Value, json};

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/tibtre.json",
    ))
    .unwrap()
}

fn fixture(input: &Value, stock: &Value) -> (Scene, Vec<(u16, u16)>) {
    let typ = input.get("type").unwrap_or(stock);
    let mut scene_input = input.clone();
    scene_input["frame"] = json!(input["constructor_frame"].as_u64().unwrap_or(0));
    scene_input["mission"] = json!("guard");
    scene_input["miner_cell"] = json!([8, 8]);
    scene_input["ore"] = input.get("ore").cloned().unwrap_or(json!([]));
    let mut s = row_scene_with(&scene_input, |text, art| {
        *text = text.replacen("2=GAOREP\n", "2=GAOREP\n3=BLOCK\n", 1);
        art.merge(&IniFile::from_str("[BLOCK]\nFoundation=1x1\n"));
        text.push_str("\n[TerrainTypes]\n0=TIBTRE01\n1=TREE01\n[TIBTRE01]\n");
        for (key, value) in typ["raw_keys"].as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
        // Queue/land rows are explicit native fixture inputs, not retail claims.
        text.push_str(&format!(
            "[BLOCK]\nStrength=100\nInvisible={}\nInvisibleInGame={}\n",
            if input["building_invisible"] == true {
                "yes"
            } else {
                "no"
            },
            if input["building_invisible_in_game"] == true {
                "yes"
            } else {
                "no"
            }
        ));
        *text = text.replacen(
            "[Riparius]\nImage=1\nValue=25\n",
            "[Riparius]\nImage=1\nValue=25\nGrowth=2200\nGrowthPercentage=.1\n",
            1,
        );
        text.push_str("[Riparius]\nGrowthPercentage=.1\nSpreadPercentage=.1\n[Cruentus]\nGrowthPercentage=.1\nSpreadPercentage=.1\n[Clear]\nBuildable=yes\n[Road]\nBuildable=no\n[Tiberium]\nBuildable=yes\n");
    });
    for ry in 0..33 {
        for rx in 0..33 {
            s.sim
                .resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut(rx, ry)
                .unwrap()
                .allows_tiberium = input["tile_allows_tiberium"] != false;
        }
    }
    s.rules.set_terrain_spawner_frame_count_for_test(
        "TIBTRE01",
        input["shape_frames"].as_u64().unwrap_or(22) as u16,
    );
    let coords: Vec<_> = input
        .get("tree_cells")
        .and_then(Value::as_array)
        .map(|cs| cs.iter().map(cell).collect())
        .unwrap_or(vec![(16, 16)]);
    let mut trees: Vec<_> = coords
        .iter()
        .map(|&(rx, ry)| TerrainObject {
            rx,
            ry,
            name: "TIBTRE01".into(),
        })
        .collect();
    for blocked in input["blocked"].as_array().into_iter().flatten() {
        if blocked[2] == "ordinary_tree" {
            let at = cell(blocked);
            trees.push(TerrainObject {
                rx: at.0,
                ry: at.1,
                name: "TREE01".into(),
            });
        }
    }
    construct_terrain_objects(&mut s.sim, &trees, &s.rules, false);
    seed_terrain_spawner_animation(&mut s.sim, &s.rules);
    let type_ref = s.sim.interner.get("TIBTRE01").unwrap();
    for &at in &coords {
        let mut animation = TerrainAnimationState::new(
            type_ref,
            NativeF32Bits::from_bits(typ["probability_bits"].as_u64().unwrap() as u32),
            typ["rate"].as_i64().unwrap() as i32,
            22,
            s.sim.session.binary_frame,
        );
        if let Some(stage) = input.get("stage") {
            let mut retained = serde_json::to_value(&animation).unwrap();
            retained["current_frame"] = stage[0].clone();
            retained["timer"] = json!({"start_frame": stage[1], "duration": stage[3]});
            retained["rate"] = stage[4].clone();
            animation = serde_json::from_value(retained).unwrap();
        }
        // Retained controls deliberately include a non-animated active Stage.
        s.sim.production.terrain_animations.insert(at, animation);
    }
    for blocked in input["blocked"].as_array().into_iter().flatten() {
        let at = cell(blocked);
        match blocked[2].as_str().unwrap() {
            "overlay" => s
                .sim
                .overlay_grid
                .as_mut()
                .unwrap()
                .place_overlay(at.0, at.1, 7, 0),
            "land" => {
                let c = s
                    .sim
                    .resolved_terrain
                    .as_mut()
                    .unwrap()
                    .cell_mut(at.0, at.1)
                    .unwrap();
                c.land_type = 1;
                c.base_terrain_class = crate::rules::terrain_rules::TerrainClass::Road;
                c.allows_tiberium = false;
            }
            "slope" => {
                s.sim
                    .resolved_terrain
                    .as_mut()
                    .unwrap()
                    .cell_mut(at.0, at.1)
                    .unwrap()
                    .slope_type = 1
            }
            "bridge" => {
                s.sim
                    .resolved_terrain
                    .as_mut()
                    .unwrap()
                    .cell_mut(at.0, at.1)
                    .unwrap()
                    .bridge_facts
                    .raw_flags = 0x100
            }
            "tree" => {
                s.sim.production.tiberium_spawning_terrain_cells.insert(at);
            }
            "ordinary_tree" => {}
            "building" => {
                let id = s
                    .sim
                    .spawn_object("BLOCK", "Americans", at.0, at.1, 0, &s.rules)
                    .unwrap();
                s.sim.substrate.entities.get_mut(id).unwrap().health.current =
                    input["building_health"].as_i64().unwrap_or(100) as i32;
            }
            other => panic!("unhandled native fixture blocker {other}"),
        }
    }
    // These retained source-overlay controls start after Terrain construction.
    for ore in input["ore"].as_array().into_iter().flatten() {
        let at = cell(ore);
        let prefix = if ore[2] == 1 { "GEM" } else { "TIB" };
        let name = format!("{prefix}{:02}", ore[3].as_u64().unwrap() + 1);
        s.sim.overlay_grid.as_mut().unwrap().place_overlay(
            at.0,
            at.1,
            registry().id_for_name(&name).unwrap(),
            ore[4].as_u64().unwrap() as u8,
        );
    }
    // Native controls supply already-constructed adjacent objects and seed
    // Scenario at the measured boundary; exclude Rust fixture constructors.
    s.sim.scenario_rng = crate::sim::rng::SimRng::new(input["seed"].as_u64().unwrap_or(1));
    (s, coords)
}

fn compare(s: &Scene, coords: &[(u16, u16)], native: &Value, context: &str) {
    for (at, expected) in coords.iter().zip(native["actors"].as_array().unwrap()) {
        let state = serde_json::to_value(&s.sim.production.terrain_animations[at]).unwrap();
        assert_eq!(
            state["current_frame"], expected["stage"],
            "{context}: Stage"
        );
        assert_eq!(state["rate"], expected["rate"], "{context}: rate");
        assert_eq!(
            state["timer"]["start_frame"], expected["timer"][0],
            "{context}: timer anchor"
        );
        assert_eq!(
            state["timer"]["duration"], expected["timer"][2],
            "{context}: timer duration"
        );
    }
    for expected in native["cells"].as_array().unwrap() {
        let at = cell(&expected["cell"]);
        let c = s.sim.overlay_grid.as_ref().unwrap().cell(at.0, at.1);
        assert_eq!(
            json!(c.overlay_id.map_or(-1, i64::from)),
            expected["overlay"],
            "{context}: overlay {at:?}"
        );
        assert_eq!(
            json!(c.overlay_data),
            expected["data"],
            "{context}: density {at:?}"
        );
    }
    let classes = &s
        .sim
        .production
        .ore_growth_state
        .native_tiberium_state()
        .classes;
    for queue in native["growth"].as_array().unwrap() {
        let growth = &classes[queue["kind"].as_u64().unwrap() as usize].growth;
        let actual: Vec<_> = (0..growth.len())
            .map(|i| {
                let entry = growth.heap_entry(i).unwrap();
                json!({"cell": [entry.rx, entry.ry], "priority_bits": entry.priority_bits})
            })
            .collect();
        assert_eq!(json!(actual), queue["entries"], "{context}: growth queue");
    }
    let rng = s.sim.scenario_rng.logical_view();
    assert_eq!(
        json!([rng.index_a, rng.index_b]),
        native["rng_indices"],
        "{context}: RNG cursors"
    );
    let bytes = s.sim.scenario_rng.native_state_hex();
    let bytes: Vec<_> = bytes
        .as_bytes()
        .chunks_exact(2)
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect();
    assert_eq!(
        crate::util::sha256::sha256_hex(&bytes),
        native["rng_sha256"].as_str().unwrap(),
        "{context}: complete RNG"
    );
}

#[test]
fn terrain_ai_timelines_match_native_stage_timers_ore_queues_and_rng() {
    let corpus = corpus();
    let stock = &corpus["retail_inputs"]["rows"][0];
    for case in corpus["timelines"].as_array().unwrap() {
        let (mut s, coords) = fixture(&case["input"], stock);
        for row in case["rows"].as_array().unwrap() {
            s.sim.session.binary_frame = row["frame"].as_u64().unwrap() as u32;
            let at = coords[row["actor"].as_u64().unwrap() as usize];
            let id = s.sim.production.terrain_object_cells[&at];
            crate::sim::terrain_spawn::tick_terrain_object_ai(
                &mut s.sim,
                id,
                Some(&s.rules),
                Some(registry()),
                None,
            );
            compare(
                &s,
                &coords,
                &row["state"],
                &format!(
                    "{} frame{} actor{}",
                    case["input"]["name"], row["frame"], row["actor"]
                ),
            );
        }
        let next: Vec<_> = (0..4).map(|_| s.sim.scenario_rng.next_u32()).collect();
        assert_eq!(
            json!(next),
            case["next_random"],
            "{} subsequent stream",
            case["input"]["name"]
        );
    }
}

#[test]
fn forced_spread_matches_original_admission_placement_queues_and_rng() {
    let corpus = corpus();
    let stock = &corpus["retail_inputs"]["rows"][0];
    for case in corpus["spread_cases"].as_array().unwrap() {
        let (mut s, coords) = fixture(&case["input"], stock);
        let objects = TiberiumPlacementObjectContext::new(
            &s.sim.substrate.entities,
            &s.sim.substrate.occupancy,
            &s.rules,
            &s.sim.interner,
            &s.sim.production.terrain_object_cells,
        );
        let terrain = s.sim.resolved_terrain.as_ref().unwrap();
        let result = spread_tiberium(
            &mut PlaceTiberiumContext {
                overlay_grid: s.sim.overlay_grid.as_mut().unwrap(),
                ore_growth_state: &mut s.sim.production.ore_growth_state,
                overlay_registry: registry(),
                tiberium_types: &s.rules.tiberium_types,
                resolved_terrain: Some(terrain),
                source_object_cells: &s.sim.production.tiberium_spawning_terrain_cells,
                new_cell_admission: Some(NewTiberiumAdmission::runtime(terrain, objects)),
                live_objects: Some(objects.object_view()),
                rng: &mut s.sim.scenario_rng,
                binary_frame: case["input"]["frame"].as_u64().unwrap_or(200) as u32,
                growth_enabled: true,
                spread_enabled: case["input"]["spreads"] != false,
                radar_dirty_cells: None,
                radar_dirty_generation: None,
                tactical_dirty_cells: None,
            },
            coords[0],
            true,
        );
        let succeeded = result.is_some();
        s.sim.publish_overlay_constructions(
            result.and_then(crate::sim::tiberium::TiberiumPlacement::into_overlay_construction),
        );
        assert_eq!(
            json!(u8::from(succeeded)),
            case["result"],
            "{} result",
            case["input"]["name"]
        );
        compare(
            &s,
            &coords,
            &case["state"],
            case["input"]["name"].as_str().unwrap(),
        );
        let next: Vec<_> = (0..4).map(|_| s.sim.scenario_rng.next_u32()).collect();
        assert_eq!(
            json!(next),
            case["next_random"],
            "{} subsequent RNG",
            case["input"]["name"]
        );
    }
}

#[test]
fn terrain_probability_and_rate_reader_match_native_physical_inputs() {
    let corpus = corpus();
    for row in corpus["retail_inputs"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .chain(corpus["custom_type_inputs"].as_array().unwrap())
    {
        let mut text = "[TerrainTypes]\n0=TIBTRE01\n[TIBTRE01]\n".to_owned();
        for (key, value) in row["raw_keys"].as_object().unwrap() {
            text.push_str(&format!("{key}={}\n", value.as_str().unwrap()));
        }
        let ini = IniFile::from_str(&text);
        let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
        let t = rules
            .terrain_object_type_case_insensitive("TIBTRE01")
            .unwrap();
        assert_eq!(
            json!(t.animation_probability.bits()),
            row["probability_bits"]
        );
        assert_eq!(json!(t.animation_rate), row["rate"]);
        assert_eq!(json!(t.is_animated), row["animated"]);
        assert_eq!(json!(t.spawns_tiberium), row["spawns"]);
    }
}

/// A single Terrain AI slot must publish movement costs before returning to
/// Logic's next object. Native midpoint timing is covered by the AI replay.
#[test]
fn same_frame_ore_terrain_ai_publishes_path_costs_before_return() {
    let native = corpus();
    let typ = &native["retail_inputs"]["rows"][0];
    // Native animated midpoint11 emits through force1. Isolate that object
    // turn without any frame-tail cache publication.
    let (mut s, _) = fixture(&json!({"seed":4,"stage":[10,0,0,0,1]}), typ);
    let mut ini = IniFile::from_str(&crate::sim::tiberium::test_support::tiberium_rules_text());
    // Stock AnyTown mode1 rows from the production asset reader; the fixture
    // otherwise uses equal land costs, which would hide the stale cache.
    ini.merge(&IniFile::from_str(
        "[Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nBuildable=yes\n\
         [Tiberium]\nFoot=90%\nTrack=70%\nWheel=50%\nBuildable=yes\n",
    ));
    let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let tree = s
        .sim
        .production
        .terrain_objects
        .iter()
        .find(|(_, object)| object.cell() == (16, 16))
        .map(|(&id, _)| id)
        .unwrap();
    crate::sim::terrain_spawn::tick_terrain_object_ai(
        &mut s.sim,
        tree,
        Some(&s.rules),
        Some(&registry),
        None,
    );
    let (x, y, _) = s
        .sim
        .overlay_grid
        .as_ref()
        .unwrap()
        .iter_occupied()
        .next()
        .expect("force1 Terrain turn creates actual ore");
    let cell = s.sim.resolved_terrain.as_ref().unwrap().cell(x, y).unwrap();
    assert_eq!(cell.land_type, 5, "native Mark published ore attributes");
    for (&speed_type, costs) in &s.sim.terrain_costs {
        assert_eq!(
            costs.ground_cost_at(x, y),
            cell.speed_costs
                .cost_for_speed_type(speed_type)
                .unwrap_or(0),
            "later Foot path requests must see the emitted ore's current costs"
        );
    }
    assert!(
        s.sim
            .path_grid
            .as_ref()
            .unwrap()
            .resolved_cell_is_current(cell, false)
    );
    assert!(
        !s.sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .pending_dirty_cells()
            .is_empty(),
        "synchronous navigation publication retains render updates"
    );
}

/// Composition witness through the normal frame and real player commands.
/// Native AI/queue timing is established separately by the executable replay.
#[test]
fn tibtre_spawned_ore_grows_is_harvested_and_reaches_refinery_credits() {
    use crate::sim::command::Command;
    use crate::sim::ore_growth::OreGrowthConfig;
    let corpus = corpus();
    let stock = &corpus["retail_inputs"]["rows"][0];
    let (mut s, _) = fixture(&json!({"seed":4}), stock);
    s.sim.production.ore_growth_config = OreGrowthConfig::disabled();
    let frame = |s: &mut Scene| {
        let path = s.sim.path_grid_snapshot();
        assert!(
            s.sim
                .advance_tick(&[], Some(&s.rules), path.as_deref(), Some(registry()), 67)
                .frame_committed
        );
    };
    assert_eq!(
        s.sim.overlay_grid.as_ref().unwrap().iter_occupied().count(),
        0
    );
    let mut at = None;
    for _ in 0..2000 {
        frame(&mut s);
        if let Some((rx, ry, overlay)) = s.sim.overlay_grid.as_ref().unwrap().iter_occupied().next()
        {
            assert_eq!(overlay.overlay_data, 3);
            at = Some((rx, ry));
            break;
        }
    }
    let at = at.expect("stock tree emits through its live Logic turn");
    assert_eq!(
        s.sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .unwrap()
            .land_type,
        5
    );
    assert!(
        s.sim
            .production
            .ore_growth_state
            .native_tiberium_state()
            .classes[0]
            .growth_bitmap
            .contains(&at)
    );
    // A queued cell reaches the existing global growth driver; the stock
    // Growth=2200 timer must expire without seeding replacement ore.
    s.sim.production.ore_growth_config = OreGrowthConfig {
        grows: true,
        spreads: false,
        tiberium_grows_flag: true,
    };
    for _ in 0..1600 {
        frame(&mut s);
        if s.sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .overlay_data
            > 3
        {
            break;
        }
    }
    assert!(
        s.sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .overlay_data
            > 3,
        "spawned ore grows from its actual retained queue"
    );
    s.sim.production.ore_growth_config = OreGrowthConfig::disabled();
    let before = s
        .sim
        .overlay_grid
        .as_ref()
        .unwrap()
        .cell(at.0, at.1)
        .overlay_data;
    assert!(s.sim.apply_command_with_overlays(
        "Americans",
        &Command::HarvestCell {
            entity_id: s.miner,
            target_rx: at.0,
            target_ry: at.1
        },
        Some(&s.rules),
        Some(registry())
    ));
    for _ in 0..1600 {
        frame(&mut s);
        if !s
            .sim
            .substrate
            .entities
            .get(s.miner)
            .unwrap()
            .miner
            .as_ref()
            .unwrap()
            .cargo
            .is_empty()
        {
            break;
        }
    }
    assert!(
        !s.sim
            .substrate
            .entities
            .get(s.miner)
            .unwrap()
            .miner
            .as_ref()
            .unwrap()
            .cargo
            .is_empty(),
        "miner cuts the tree's ore through Mission_Harvest"
    );
    assert!(
        s.sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .overlay_data
            < before
    );
    let owner = s.sim.interner.get("Americans").unwrap();
    let balance = s.sim.houses[&owner].economy.credits;
    assert!(s.sim.apply_command_with_overlays(
        "Americans",
        &Command::MinerReturn {
            entity_id: s.miner,
            target_refinery_id: Some(s.refinery)
        },
        Some(&s.rules),
        Some(registry())
    ));
    for _ in 0..1800 {
        frame(&mut s);
        if s.sim.houses[&owner].economy.credits > balance {
            break;
        }
    }
    assert!(
        s.sim.houses[&owner].economy.credits > balance,
        "real refinery unload credits the harvested resource"
    );
}

#[test]
fn tibtre_active_stage_survives_snapshot_and_removal_clears_its_owner() {
    use crate::sim::ore_growth::OreGrowthConfig;
    use crate::sim::rng::SimRng;
    use crate::sim::snapshot::GameSnapshot;
    let corpus = corpus();
    let (mut s, coords) = fixture(&json!({"seed":4}), &corpus["retail_inputs"]["rows"][0]);
    s.sim.production.ore_growth_config = OreGrowthConfig::disabled();
    let at = coords[0];
    for _ in 0..2000 {
        let path = s.sim.path_grid_snapshot();
        s.sim
            .advance_tick(&[], Some(&s.rules), path.as_deref(), Some(registry()), 67);
        let a = &s.sim.production.terrain_animations[&at];
        if a.is_active() && a.current_frame() >= 3 {
            break;
        }
    }
    assert!(s.sim.production.terrain_animations[&at].is_active());
    // Save loading intentionally reseeds Scenario RNG; compare both
    // continuations under that existing owner policy.
    s.sim.scenario_rng = SimRng::new(0);
    let base_terrain = s.sim.resolved_terrain.as_ref().unwrap().clone();
    let bytes = GameSnapshot::save(&s.sim, 0, 0, "TIBTRE continuation", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        base_terrain,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &s.rules,
    );
    for _ in 0..40 {
        let path = s.sim.path_grid_snapshot();
        s.sim
            .advance_tick(&[], Some(&s.rules), path.as_deref(), Some(registry()), 67);
        let path = restored.path_grid_snapshot();
        restored.advance_tick(&[], Some(&s.rules), path.as_deref(), Some(registry()), 67);
        assert_eq!(
            s.sim.production.terrain_animations,
            restored.production.terrain_animations
        );
        assert_eq!(
            s.sim.scenario_rng.logical_state(),
            restored.scenario_rng.logical_state()
        );
        let ore = |sim: &crate::sim::world::Simulation| {
            sim.overlay_grid
                .as_ref()
                .unwrap()
                .iter_occupied()
                .map(|(x, y, c)| (x, y, c.overlay_id, c.overlay_data))
                .collect::<Vec<_>>()
        };
        assert_eq!(ore(&s.sim), ore(&restored));
    }
    let id = s.sim.production.terrain_object_cells[&at];
    assert!(crate::sim::terrain_object::limbo_terrain_object_at_cell(
        &mut s.sim.production,
        at,
        &mut s.sim.substrate.raw_cell_occupation,
        s.sim.resolved_terrain.as_mut()
    ));
    s.sim.unregister_non_entity_object(id);
    assert!(!s.sim.production.terrain_animations.contains_key(&at));
    assert!(
        !s.sim
            .production
            .tiberium_spawning_terrain_cells
            .contains(&at)
    );
}

#[test]
fn forced_terrain_enqueue_rebuild_uses_live_scenario_flags_through_advance_tick() {
    use crate::sim::ore_growth::OreGrowthConfig;
    let corpus = corpus();
    // Retained active stage reaches the stock midpoint on its next due tick.
    let (mut s, _) = fixture(
        &json!({"seed":1, "stage":[10,0,0,0,1]}),
        &corpus["retail_inputs"]["rows"][0],
    );
    s.sim.production.ore_growth_config = OreGrowthConfig::disabled();
    let grid = s.sim.overlay_grid.as_mut().unwrap();
    grid.place_overlay(10, 10, registry().id_for_name("TIB01").unwrap(), 2);
    // Prepared drained-heap prestate models a valid long-running append array.
    // Native admission/rebuild ordering and growth-off controls: ore_queue.json.
    let capacity = crate::sim::ore_growth::native_tiberium_queue_capacity((16, 16));
    let mut saved = serde_json::to_value(&s.sim.production.ore_growth_state).unwrap();
    saved["native_tiberium"]["classes"][0]["growth"]["entries"] = json!(vec![
        json!({"rx":10,"ry":10,"priority_bits":0});
        (capacity - 9)
            as usize
    ]);
    saved["native_tiberium"]["classes"][0]["growth"]["heap"] = json!([0]);
    saved["native_tiberium"]["classes"][0]["growth_bitmap"] = json!([[10, 10]]);
    s.sim.production.ore_growth_state = serde_json::from_value(saved).unwrap();
    let path = s.sim.path_grid_snapshot();
    s.sim
        .advance_tick(&[], Some(&s.rules), path.as_deref(), Some(registry()), 67);
    let saved = serde_json::to_value(&s.sim.production.ore_growth_state).unwrap();
    let class = &saved["native_tiberium"]["classes"][0];
    let entries = class["growth"]["entries"].as_array().unwrap();
    assert_eq!(
        entries.len(),
        1,
        "growth-off rebuild seeds nothing; forced new placement still appends"
    );
    let at = (
        entries[0]["rx"].as_u64().unwrap() as u16,
        entries[0]["ry"].as_u64().unwrap() as u16,
    );
    assert_ne!(at, (10, 10));
    assert_eq!(
        s.sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(at.0, at.1)
            .overlay_data,
        3
    );
    assert_eq!(class["growth_bitmap"], json!([[at.0, at.1]]));
    assert_eq!(class["spread"]["entries"], json!([]));
    assert_eq!(
        s.sim
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(10, 10)
            .overlay_data,
        2
    );
}
