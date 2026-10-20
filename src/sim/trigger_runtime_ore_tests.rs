//! Steam gamemd.exe3e81a61775d2745d1dabe397325ef663cd994ffc194da4e998e3bf5d2d308600:
//! seven original59/57B790 executions observed109 ordered480A80(centre,1)
//! sinks (real/Dummy, signed extremes, primary versus tail waypoint).
//! Thirteen original86 controls pin the raw low-byte store. They do not
//! certify the reducer's full effects; those retain their existing oracle.
use super::*;
use crate::map::map_file::MapFile;
use crate::map::resolved_terrain::{test_flat_cell, test_grid};
use crate::map::waypoints::Waypoint;
use crate::rules::{ini_parser::IniFile, overlay_types::OverlayTypeRegistry, ruleset::RuleSet};
use crate::sim::ore_growth::{OreGrowthConfig, OreGrowthState};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::TickLane;
use std::collections::HashMap;

fn action(kind: i32, raw: &str, tail: &str) -> RuntimeAction {
    let ini = IniFile::from_str(&format!(
        "[Actions]\nCASE=1,{kind},0,{raw},0,0,0,0,{tail}\n"
    ));
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("CASE")
        .unwrap()
        .entries()[0]
        .clone();
    RuntimeDefinitions::default().resolve_action(entry, None, None)
}

fn run(
    sim: &mut Simulation,
    action: &RuntimeAction,
    rules: Option<&RuleSet>,
    registry: Option<&OverlayTypeRegistry>,
) -> bool {
    sim.apply_live_trigger_action(
        action,
        TriggerActionContext {
            live_index: None,
            incoming_house: None,
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        rules,
        registry,
        FrameEffects::default(),
    )
}

fn rng(sim: &Simulation) -> [String; 3] {
    [
        sim.main_rng.native_state_hex(),
        sim.scenario_rng.native_state_hex(),
        sim.mapgen_rng.native_state_hex(),
    ]
}

fn scene(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(0x59_86);
    sim.session.set_map_width_for_test(16);
    sim.session.set_map_height_for_test(16);
    sim.session.set_tiberium_grows_flag_for_test(true);
    sim.session.set_tiberium_spreads_flag_for_test(false);
    sim.install_resolved_terrain_for_new_map(test_grid(16, 16, test_flat_cell));
    sim.overlay_grid = Some(OverlayGrid::new(16, 16));
    sim.ore_growth_config = OreGrowthConfig::for_test(true, false, true);
    sim.ore_growth_state = OreGrowthState::new(16, 16);
    sim.ore_growth_state.reset_native_tiberium_classes_for_rect(
        (8, 8),
        rules.tiberium_types.len(),
        0,
    );
    waypoints(&mut sim, &[(0, (8, 8)), (1, (8, 9))]);
    sim
}

fn waypoints(sim: &mut Simulation, points: &[(u32, (i16, i16))]) {
    sim.session.install_waypoints(
        &points
            .iter()
            .map(|&(index, cell)| {
                (
                    index,
                    Waypoint {
                        index,
                        rx: cell.0 as u16,
                        ry: cell.1 as u16,
                    },
                )
            })
            .collect::<HashMap<_, _>>(),
    );
}

fn fixture() -> (Simulation, RuleSet, &'static OverlayTypeRegistry) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        &crate::sim::tiberium::test_support::tiberium_rules_text(),
    ))
    .unwrap();
    let sim = scene(&rules);
    (
        sim,
        rules,
        crate::sim::tiberium::test_support::overlay_registry(),
    )
}

fn ore(sim: &mut Simulation, registry: &OverlayTypeRegistry, cell: (u16, u16), data: u8) {
    let id = registry.id_for_name("TIB01").unwrap();
    let grid = sim.overlay_grid.as_mut().unwrap();
    grid.place_overlay(cell.0, cell.1, id, data);
    grid.recalculate_runtime_cell(sim.resolved_terrain.as_mut().unwrap(), registry, cell);
}

fn seed_growth(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    cell: (u16, u16),
) {
    let type_id = rules.tiberium_types.id_by_name("Riparius").unwrap();
    assert!(
        sim.ore_growth_state
            .add_native_growth_queue_cell(
                sim.overlay_grid.as_ref().unwrap(),
                registry,
                &rules.tiberium_types,
                sim.resolved_terrain.as_ref(),
                true,
                type_id,
                cell.0,
                cell.1,
                sim.session.binary_frame(),
                &mut sim.scenario_rng,
            )
            .is_some()
    );
}

#[test]
fn thirteen_original_growth_byte_controls_preserve_queues_timers_rng_and_other_flags() {
    for (raw, parsed, expected) in [
        ("0", 0, false),
        ("1", 1, true),
        ("2", 2, true),
        ("255", 255, true),
        ("256", 256, false),
        ("257", 257, true),
        ("-1", -1, true),
        ("-256", -256, false),
        ("-257", -257, true),
        ("-2147483648", i32::MIN, false),
        ("2147483647", i32::MAX, true),
        (".2", 0, false),
        ("257suffix", 257, true),
    ] {
        let (mut sim, rules, registry) = fixture();
        sim.session.set_binary_frame_for_test(73);
        ore(&mut sim, registry, (8, 8), 5);
        seed_growth(&mut sim, &rules, registry, (8, 8));
        let queues = serde_json::to_value(&sim.ore_growth_state).unwrap();
        let previous_rng = rng(&sim);
        let hash = sim.state_hash();
        let entry = action(86, raw, "A");
        assert_eq!(entry.value, Some(parsed));
        // Native reads no Rules, actor, House or waypoint for86.
        assert!(run(&mut sim, &entry, None, None));
        assert_eq!(sim.ore_growth_config.grows(), expected, "{raw}");
        assert!(!sim.ore_growth_config.spreads());
        assert!(sim.ore_growth_config.tiberium_grows_flag());
        assert!(sim.session.tiberium_grows_flag() && !sim.session.tiberium_spreads_flag());
        assert_eq!(serde_json::to_value(&sim.ore_growth_state).unwrap(), queues);
        assert_eq!(rng(&sim), previous_rng);
        assert_eq!(sim.session.binary_frame(), 73);
        assert_eq!(
            sim.state_hash() == hash,
            expected,
            "the flag hashes before a driver runs"
        );
    }
}

#[test]
fn primary_waypoint_and_109_bites_reach_one_centre_without_radius_mutation() {
    let (mut sim, rules, registry) = fixture();
    ore(&mut sim, registry, (8, 8), 255);
    ore(&mut sim, registry, (8, 9), 200);
    waypoints(&mut sim, &[(0, (8, 9)), (1, (8, 8))]);
    let queues = serde_json::to_value(&sim.ore_growth_state).unwrap();
    let previous_rng = rng(&sim);
    // Raw density255 distinguishes109 unit reductions from a single bite or
    // a bounded "clear" approximation.146 is the instruction-derived byte
    // subtraction regression; the independently executed golden is count109.
    assert!(run(
        &mut sim,
        &action(59, "1", "A"),
        Some(&rules),
        Some(registry)
    ));
    let grid = sim.overlay_grid.as_ref().unwrap();
    assert_eq!(grid.cell(8, 8).overlay_data, 146);
    assert!(grid.cell(8, 8).overlay_id.is_some());
    assert_eq!(grid.cell(8, 9).overlay_data, 200);
    assert_eq!(serde_json::to_value(&sim.ore_growth_state).unwrap(), queues);
    assert_eq!(rng(&sim), previous_rng);
}

#[test]
fn retail_layered_centre_removal_keeps_neighbors_and_uses_existing_spread_radar_cleanup() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    // Production RULESMD/LANGRULE when present/Battle/Hills readers; geometry
    // below is a declared flat crop, not a retail Hills playback.
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let rules = retail.rules;
    let mut sim = scene(&rules);
    sim.ore_growth_config.set_grows_for_test(false);
    sim.ore_growth_config.set_spreads_for_test(true);
    sim.session.set_tiberium_spreads_flag_for_test(true);
    let center = (8, 8);
    ore(&mut sim, &registry, center, 11);
    let neighbors: Vec<_> = crate::sim::tiberium::ADJACENT_OFFSETS
        .iter()
        .map(|&(dx, dy)| ((8 + dx) as u16, (8 + dy) as u16))
        .collect();
    for &cell in &neighbors {
        ore(&mut sim, &registry, cell, 11);
    }
    let type_id = rules.tiberium_types.id_by_name("Riparius").unwrap();
    sim.ore_growth_state
        .mark_native_spread_bitmap_for_tests(type_id.0 as usize, center);
    sim.dirty_cells.clear_radar_for_test();
    sim.dirty_cells.clear_tactical_for_test();
    let generation = sim.dirty_cells.radar_generation();
    // Basic growth is disabled, but reduction and its SpecialFlags spread
    // registration retain their own native paths.
    assert!(run(
        &mut sim,
        &action(59, "0", "B"),
        Some(&rules),
        Some(&registry)
    ));
    let grid = sim.overlay_grid.as_ref().unwrap();
    assert!(grid.cell(8, 8).overlay_id.is_none());
    for &cell in &neighbors {
        assert_eq!(grid.cell(cell.0, cell.1).overlay_data, 11);
        assert!(grid.cell(cell.0, cell.1).overlay_id.is_some());
    }
    let state = &sim.ore_growth_state.native_tiberium_state().classes[type_id.0 as usize];
    assert!(!state.spread_bitmap.contains(&center));
    assert_eq!(state.spread_bitmap.len(), 8);
    assert!(
        neighbors
            .iter()
            .all(|cell| state.spread_bitmap.contains(cell))
    );
    assert!(sim.dirty_cells.radar().contains(&center));
    assert_eq!(sim.dirty_cells.radar_generation(), generation + 1);
    assert!(sim.dirty_cells.tactical_for_test().contains(&center));
}

#[test]
fn reducer_uses_native_alias_identity_and_does_not_consume_an_unallocated_grid_record() {
    let (mut sim, rules, registry) = fixture();
    ore(&mut sim, registry, (0, 1), 255);
    ore(&mut sim, registry, (4, 4), 200);
    sim.resolved_terrain
        .as_mut()
        .unwrap()
        .test_set_native_allocated_cells(&[(0, 1)]);
    // Native y*512+x aliases this packed request to the real(0,1) CellClass.
    waypoints(&mut sim, &[(0, (512, 0))]);
    assert!(run(
        &mut sim,
        &action(59, "0", "A"),
        Some(&rules),
        Some(registry)
    ));
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(0, 1).overlay_data,
        146
    );
    waypoints(&mut sim, &[(0, (4, 4))]);
    assert!(run(
        &mut sim,
        &action(59, "0", "A"),
        Some(&rules),
        Some(registry)
    ));
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(4, 4).overlay_data,
        200
    );
    assert_eq!(sim.effective_shared_cell_dummy().snapshot().coord, (4, 4));
    // A mineral-bearing Dummy remains a declared response gap; do not pin
    // its incorrect no-op as an expected native reduction result.
}

#[test]
fn counted_tag_pauses_drivers_preserves_saved_gate_and_reenables_due_timers() {
    let map = MapFile::from_bytes(b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n[IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n[Basic]\nNewINIFormat=4\n[Triggers]\nORE=Neutral,<none>,ORE,0,1,1,1,0\n[Events]\nORE=1,13,0,0\n[Actions]\nORE=2,86,0,256,0,0,0,0,A,59,0,1,0,0,0,0,A\n[Tags]\nORETAG=0,ORE,ORE\n").unwrap();
    let (mut sim, rules, registry) = fixture();
    ore(&mut sim, registry, (8, 8), 11);
    ore(&mut sim, registry, (8, 9), 5);
    seed_growth(&mut sim, &rules, registry, (8, 9));
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.classify_map_triggers(None);
    waypoints(&mut sim, &[(0, (8, 9)), (1, (8, 8))]);
    sim.session.publish_started();
    sim.advance_triggers(Some(&rules), Some(registry), FrameEffects::default());
    assert!(!sim.ore_growth_config.grows());
    assert!(sim.trigger_runtime.logic_tags.is_empty());
    assert!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .cell(8, 8)
            .overlay_id
            .is_none()
    );
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(8, 9).overlay_data,
        5
    );
    let queues = serde_json::to_value(&sim.ore_growth_state).unwrap();
    let frame = sim
        .advance_app_frame(
            &[],
            Some(&rules),
            Some(registry),
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(frame.tick.frame_committed);
    assert_eq!(serde_json::to_value(&sim.ore_growth_state).unwrap(), queues);
    let saved = GameSnapshot::save(&sim, 0, 0, "Paused ore", 0);
    let mut restored = GameSnapshot::load(&saved).unwrap().sim;
    restored.bind_shared_cell_dummy(crate::map::resolved_terrain::SharedCellDummy::fresh());
    restored.reconstruct_cellclass_dummy_for_map_resize();
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(
        test_grid(16, 16, test_flat_cell),
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    restored
        .restore_map_authority_after_snapshot_load(&rules, registry, None)
        .unwrap();
    restored.resolve_type_handles(&rules);
    assert!(!restored.ore_growth_config.grows());
    assert!(!restored.ore_growth_config.spreads());
    assert!(restored.ore_growth_config.tiberium_grows_flag());
    restored.session.publish_started();
    assert!(run(&mut restored, &action(86, "257", "A"), None, None));
    let current_frame = restored.session.binary_frame() as i32;
    let frame = restored
        .advance_app_frame(
            &[],
            Some(&rules),
            Some(registry),
            134,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert!(frame.tick.frame_committed);
    let states = &restored.ore_growth_state.native_tiberium_state().classes;
    assert!(!states.is_empty());
    for state in states {
        assert_eq!(state.growth_timer.start_frame(), current_frame);
        assert_eq!(state.spread_timer.start_frame(), current_frame);
        assert!(state.growth_timer.duration() > 0 && state.spread_timer.duration() > 0);
    }
}
