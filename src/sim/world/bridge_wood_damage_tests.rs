//! Physical Shrapnel ordinary wooden bridge: Engineer repair, damage, collapse,
//! occupants, navigation and rebuilding. Native scalar/navigation/occupant
//! packets live in tools/spatial_oracle/shrapnel_damage. Ordinary world timing
//! below is a Rust integration witness, not a native whole-world comparison.
use crate::headless_scenario::{HeadlessScenario, SIM_TICK_MS};
use crate::rules::terrain_rules::LandType;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::bridge_test_evidence::{
    assert_navigation_fields_equal, damage_shrapnel_wood, load_shrapnel, navigation_authority,
    navigation_snapshot, rebuilt_graphs, repair_shrapnel_wood, restored_retail, save_scene,
};
use serde_json::{Value, json};

fn points() -> impl Iterator<Item = (u16, u16)> {
    (51..=67).flat_map(|y| (111..=120).map(move |x| (x, y)))
}

fn export(scene: &HeadlessScenario, phase: &str) {
    let Some(root) = std::env::var_os("VERA20K_WOOD_EXPORT") else {
        return;
    };
    let sim = scene.sim();
    let result = json!({
        "phase":phase, "frame":sim.session.binary_frame,
        "navigation":navigation_snapshot(sim, phase, points()),
        "entities":sim.entities().values().filter(|e|
            (111..=120).contains(&e.position.rx) && (51..=67).contains(&e.position.ry)
        ).map(|e| json!({"type":sim.resolve(e.type_ref()),"entity":e})).collect::<Vec<_>>(),
        "rng":{"main":sim.main_rng,"scenario":sim.scenario_rng,"mapgen":sim.mapgen_rng},
    });
    let path = std::path::PathBuf::from(root).with_extension(format!("{phase}.json"));
    serde_json::to_writer_pretty(std::fs::File::create(path).unwrap(), &result).unwrap();
}

fn assert_ground_surface(scene: &HeadlessScenario, collapsed: bool) {
    let sim = scene.sim();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    for y in 58..=60 {
        for x in 114..=116 {
            let cell = terrain.cell(x, y).unwrap();
            let expected_land = if y == 59 && collapsed {
                LandType::Water
            } else {
                LandType::Road
            };
            assert_eq!(cell.yr_cell_land_type, expected_land.as_index(), "{x},{y}");
            assert_eq!(cell.level, 2);
            assert_eq!(cell.bridge_facts.raw_flags, 0);
            assert!(!cell.bridge_facts.has_structural_bridge());
            assert_eq!(
                sim.overlay_grid.as_ref().unwrap().cell(x, y).overlay_id,
                cell.bridge_facts.overlay_id,
                "OverlayGrid must mirror the raw Cell identity"
            );
            assert!(
                !sim.path_grid.as_ref().unwrap().is_walkable_on_layer(
                    x,
                    y,
                    crate::sim::movement::locomotor::MovementLayer::Bridge
                ),
                "ordinary ground wood must never acquire a raised movement layer"
            );
        }
    }
    for hut in [(117, 56), (113, 62)] {
        assert_eq!(
            crate::sim::world::bridge_orchestrator::bridge_hut_can_repair(sim, hut),
            collapsed,
        );
    }
}

#[test]
#[ignore = "requires physical Shrapnel and retail SNOW assets"]
fn retail_wood_damage_repair_and_restore_publish_navigation() {
    let mut scene = load_shrapnel();
    let pristine = scene.sim().resolved_terrain.as_ref().unwrap().clone();
    export(&scene, "loaded");
    repair_shrapnel_wood(&mut scene);
    assert_ground_surface(&scene, false);
    export(&scene, "healthy");
    let mut saved = vec![(
        "healthy",
        save_scene(&scene, "healthy"),
        navigation_authority(scene.sim(), points()),
    )];
    let scalar: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/shrapnel_damage/scalar_test_vectors.json",
    ))
    .unwrap();
    let physical = &scalar["physical_sequences"][1];
    for (step, phase, collapsed) in [(0, "damaged", false), (1, "collapsed", true)] {
        assert_eq!(damage_shrapnel_wood(&mut scene), collapsed);
        assert_ground_surface(&scene, collapsed);
        for row in physical["steps"][step]["final"].as_array().unwrap() {
            let (x, y) = (
                row[0].as_u64().unwrap() as u16,
                row[1].as_u64().unwrap() as u16,
            );
            let actual = scene
                .sim()
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(x, y)
                .unwrap()
                .bridge_facts
                .overlay_id
                .map_or(-1, i32::from);
            assert_eq!(actual, row[2].as_i64().unwrap() as i32, "{phase} {x},{y}");
        }
        for row in physical["input"]["cells"].as_array().unwrap() {
            let (x, y) = (
                row[0].as_u64().unwrap() as u16,
                row[1].as_u64().unwrap() as u16,
            );
            assert_eq!(
                scene
                    .sim()
                    .overlay_grid
                    .as_ref()
                    .unwrap()
                    .cell(x, y)
                    .overlay_data,
                row[6].as_u64().unwrap() as u8,
                "native damage preserves frame {x},{y}"
            );
        }
        export(&scene, phase);
        saved.push((
            phase,
            save_scene(&scene, phase),
            navigation_authority(scene.sim(), points()),
        ));
    }
    let before_rejected = scene.sim().state_hash();
    assert!(!damage_shrapnel_wood(&mut scene));
    assert_eq!(
        scene.sim().state_hash(),
        before_rejected,
        "collapsed direct band is no longer admitted"
    );
    repair_shrapnel_wood(&mut scene);
    assert_ground_surface(&scene, false);
    export(&scene, "repaired");
    saved.push((
        "repaired",
        save_scene(&scene, "repaired"),
        navigation_authority(scene.sim(), points()),
    ));
    for (phase, bytes, expected_navigation) in saved {
        let first = restored_retail(&scene, &pristine, &bytes);
        let mut second = restored_retail(&scene, &pristine, &bytes);
        assert_navigation_fields_equal(
            &navigation_authority(&first, points()),
            &expected_navigation,
            phase,
        );
        assert_eq!(rebuilt_graphs(&first), rebuilt_graphs(&second));
        scene.runtime.simulation = first;
        export(&scene, &format!("restored_{phase}"));
        for frame in 0..40 {
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
                "{phase} restored frame{frame}"
            );
        }
        assert_eq!(rebuilt_graphs(scene.sim()), rebuilt_graphs(&second));
    }
}

#[test]
#[ignore = "requires physical Shrapnel and retail SNOW assets; ordinary firing can take several minutes"]
fn retail_wood_force_fire_collapses_then_engineer_rebuilds() {
    let mut scene = load_shrapnel();
    repair_shrapnel_wood(&mut scene);
    export(&scene, "attack_healthy");
    let runtime = &mut scene.runtime;
    let owner = runtime.simulation.session.current_house.unwrap();
    let owner_name = runtime.simulation.resolve(owner).to_owned();
    let tank = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "MTNK",
            &owner_name,
            115,
            55,
            128,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .expect("ordinary Road firing placement");
    export(&scene, "attack_before_command");
    let command = CommandEnvelope::new(
        owner,
        scene.sim().session.tick + 1,
        Command::ForceAttackCell {
            attacker_id: tank,
            target_rx: 115,
            target_ry: 59,
        },
    );
    scene
        .runtime
        .advance_frame(
            std::slice::from_ref(&command),
            SIM_TICK_MS,
            crate::sim::world::TickLane::Ordinary,
        )
        .unwrap();
    export(&scene, "attack_command_applied");
    let mut prior = scene
        .sim()
        .resolved_terrain
        .as_ref()
        .unwrap()
        .cell(115, 59)
        .unwrap()
        .bridge_facts
        .overlay_id;
    let mut shots = Vec::new();
    let mut transitions = Vec::new();
    let mut collapsed = false;
    for _ in 0..18_000 {
        let logic_frame = scene.sim().session.binary_frame;
        let output = scene
            .runtime
            .advance_frame(&[], SIM_TICK_MS, crate::sim::world::TickLane::Ordinary)
            .unwrap();
        for shot in output
            .fire_events
            .iter()
            .filter(|shot| shot.attacker_id == tank)
        {
            shots.push(json!({"completed_logic_frame":logic_frame,"shot":format!("{shot:?}"),
                "actor":scene.sim().entities().get(tank),
                "rng":{"main":scene.sim().main_rng,"scenario":scene.sim().scenario_rng,"mapgen":scene.sim().mapgen_rng}}));
        }
        let overlay = scene
            .sim()
            .resolved_terrain
            .as_ref()
            .unwrap()
            .cell(115, 59)
            .unwrap()
            .bridge_facts
            .overlay_id;
        if overlay != prior {
            transitions.push(json!({"completed_logic_frame":logic_frame,"overlay":overlay}));
            eprintln!("SHRAPNEL_ATTACK transition frame{logic_frame} {prior:?}->{overlay:?}");
            prior = overlay;
            export(
                &scene,
                if overlay == Some(101) {
                    "attack_collapsed"
                } else {
                    "attack_damaged"
                },
            );
        }
        if overlay == Some(101) {
            collapsed = true;
            break;
        }
    }
    if let Some(root) = std::env::var_os("VERA20K_WOOD_EXPORT") {
        let path = std::path::PathBuf::from(root).with_extension("attack_trace.json");
        serde_json::to_writer_pretty(
            std::fs::File::create(path).unwrap(),
            &json!({"shots":shots,"transitions":transitions}),
        )
        .unwrap();
    }
    assert!(
        collapsed,
        "ordinary retail MTNK firing must reach both admitted low damage stages"
    );
    assert_eq!(transitions.len(), 2);
    assert_ground_surface(&scene, true);
    let source = scene.sim().entities().get(tank).unwrap();
    assert!(source.is_alive());
    assert!(
        source.attack_target.is_none(),
        "70D4A0 must detach the Cell before the bullet continuation returns"
    );
    let mut guarding = false;
    for _ in 0..32 {
        let output = scene
            .runtime
            .advance_frame(&[], SIM_TICK_MS, crate::sim::world::TickLane::Ordinary)
            .unwrap();
        assert!(
            !output
                .fire_events
                .iter()
                .any(|event| event.attacker_id == tank)
        );
        if scene
            .sim()
            .entities()
            .get(tank)
            .unwrap()
            .mission
            .current()
            .known()
            == Some(crate::sim::mission::MissionType::Guard)
        {
            guarding = true;
            break;
        }
    }
    assert!(guarding);
    export(&scene, "attack_retired");
    repair_shrapnel_wood(&mut scene);
    assert_ground_surface(&scene, false);
    export(&scene, "attack_repaired");
}
