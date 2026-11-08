use crate::map::resolved_terrain::DynamicTerrainCellState;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::retail_ini_fixture::{art_registry, retail_assets, retail_rules_and_art};
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::world::FrameEffects;

/// Unit741970 ->7447B0 (!S,U,!DB,!T) inserts a burrowable intermediate;
/// Foot EnterIdle4D8395 must later forward the queued Abstract object pointer.
#[test]
fn below_ground_drive_routes_through_burrowable_cell_then_resumes_object_target() {
    let Some((root, art)) = retail_rules_and_art() else {
        return;
    };
    let mut layers = RulesLayerStack::new(root);
    layers.push(
        RulesLayerKind::Scenario,
        IniFile::from_str("[HTNK]\nMovementZone=Subterannean\n"),
    );
    let processed = layers.process_with_fixed_art(&art).unwrap();
    let mut rules = RuleSet::from_processed_rules(&processed).unwrap();
    rules.install_art_data(art_registry(&art));
    let (_, mut assets) = retail_assets().expect("retail clear TMP");
    let mut theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    // Both physical types use the same passable retail Clear TMP. Only the
    // second type's native AllowBurrowing permission differs.
    theater.lookup = crate::map::theater::parse_tileset_ini(
        b"[TileSet0000]\nTilesInSet=1\nFileName=clear\n\
          [TileSet0001]\nTilesInSet=1\nFileName=clear\nAllowBurrowing=no\n",
        "tem",
    )
    .unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 17);
    let mut terrain = sim.resolved_terrain().unwrap().clone();
    terrain
        .bind_saved_template_assets(
            &theater,
            &assets,
            &rules.terrain_rules,
            rules.overlay_registry(),
            false,
            0,
            Default::default(),
        )
        .unwrap();
    let mut replacement = DynamicTerrainCellState::capture(terrain.cell(20, 20).unwrap());
    replacement.final_tile_index = 1;
    assert!(terrain.apply_dynamic_cell_state(20, 20, &replacement));
    assert!(!terrain.tile_allows_burrowing(1));
    assert!(!terrain.cell(20, 20).unwrap().ground_walk_blocked);
    sim.install_resolved_terrain_for_new_map(terrain);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let mover = sim
        .spawn_object_with_overlay_registry(
            "HTNK",
            "Americans",
            10,
            10,
            0,
            &rules,
            rules.overlay_registry(),
        )
        .unwrap();
    let target = sim
        .spawn_object_with_overlay_registry(
            "MTNK",
            "Americans",
            20,
            20,
            0,
            &rules,
            rules.overlay_registry(),
        )
        .unwrap();
    sim.set_object_height(
        mover,
        -1,
        &rules,
        Some(rules.overlay_registry()),
        FrameEffects::default(),
    );
    assert_eq!(
        super::air_movement::current_fly_height(
            sim.substrate.entities.get(mover).unwrap(),
            sim.resolved_terrain(),
        ),
        -1
    );
    let requested = NavTargetRef::object(target);
    let rng_before = sim.scenario_rng.native_state_hex();
    assert!(sim.set_unit_destination(mover, requested, &rules, true, FrameEffects::default()));
    let navigation = &sim.substrate.entities.get(mover).unwrap().navigation;
    let Some(NavTargetRef::Cell { rx, ry }) = navigation.nav_com() else {
        panic!("the burrowing planner must choose an intermediate Cell before the object");
    };
    assert_ne!((rx, ry), (20, 20));
    let terrain = sim.resolved_terrain().unwrap();
    assert!(terrain.tile_allows_burrowing(terrain.cell(rx, ry).unwrap().final_tile_index));
    assert_eq!(navigation.nav_queue(), &[requested]);

    // EnterIdle owns the continuation after reaching an intermediate. Its
    // queued setter uses clear_queue=false and preserves the Object target.
    // Stop the active Drive through its own arrival/Stop_Moving owner; a
    // still-moving Drive instead queues this request on NavList741E8E.
    assert!(super::navcom::track_stop_moving(
        sim.substrate.entities.get_mut(mover).unwrap()
    ));
    // Stage FootAI4DA54E's between-entry reset; its generic object-turn
    // lifecycle is covered separately by track_object_turn_tests.
    sim.substrate
        .entities
        .get_mut(mover)
        .unwrap()
        .mission_leaf
        .set_foot_idle_entry_latch(0);
    assert!(sim.foot_enter_idle_base(
        mover,
        &rules,
        Some(rules.overlay_registry()),
        FrameEffects::default(),
    ));
    let navigation = &sim.substrate.entities.get(mover).unwrap().navigation;
    assert_eq!(navigation.nav_com(), Some(requested));
    assert!(navigation.nav_queue().is_empty());
    assert_eq!(sim.scenario_rng.native_state_hex(), rng_before);
}
