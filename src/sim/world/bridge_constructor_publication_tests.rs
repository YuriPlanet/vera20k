use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::native_identity::NativeUniqueIdCursor;
use crate::sim::overlay_grid::OverlayGrid;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn fixture() -> (
    Simulation,
    RuleSet,
    crate::map::overlay_types::OverlayTypeRegistry,
) {
    let mut text = String::from("[Clear]\nWheel=100%\n[Road]\nWheel=100%\n[OverlayTypes]\n");
    for id in 0..=238 {
        text.push_str(&format!("{id}=O{id}\n"));
    }
    for id in 0..=238 {
        text.push_str(&format!("[O{id}]\nNoUseTileLandType=no\n"));
        if id == 7 {
            text.push_str("Overrides=yes\n");
        }
    }
    let ini = IniFile::from_str(&text);
    let rules = RuleSet::from_ini(&ini).unwrap();
    let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let terrain = crate::map::resolved_terrain::bridge_constructor_terrain();
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.overlay_grid = Some(OverlayGrid::new(33, 33));
    sim.native_unique_ids = Some(NativeUniqueIdCursor::test_at_current_value(1000));
    (sim, rules, registry)
}

fn cells(sim: &Simulation) -> Value {
    let grid = sim.resolved_terrain.as_ref().unwrap();
    Value::Array((12..=20).flat_map(|y| (12..=20).filter_map(move |x| {
        let cell = grid.cell(x, y).unwrap();
        let flags = cell.bridge_facts.raw_flags;
        let overlay = cell.bridge_facts.overlay_id.map_or(-1, i32::from);
        let state = cell.bridge_facts.state_byte;
        let anchor = cell.bridge_facts.native_anchor.map(|id| grid.native_cell_coord(id));
        (flags != 0 || overlay != -1 || state != 0 || anchor.is_some()).then(||
            json!({"coord":[x,y], "flags":flags, "overlay":overlay, "state":state, "anchor":anchor}))
    })).collect())
}

fn dummy(sim: &Simulation) -> Value {
    let grid = sim.resolved_terrain.as_ref().unwrap();
    let fallback = grid.shared_cell_dummy();
    let (overlay, state) = fallback.overlay_identity_state();
    json!({"coord":grid.native_cell_coord(Cell::Dummy), "flags":fallback.raw_flags(),
        "overlay":overlay,"state":state,"anchor_is_self":fallback.native_anchor()==Some(Cell::Dummy)})
}

#[test]
fn live_bridge_recalc_publishes_retained_attributes_before_connectivity() {
    use crate::sim::pathfinding::{PathGrid, zone_map::ZoneGrid};
    let (mut sim, rules, registry) = fixture();
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let path = PathGrid::from_resolved_terrain(terrain);
    let mut prior_terrain = terrain.clone();
    prior_terrain.cell_mut(16, 16).unwrap().overlay_blocks = true;
    prior_terrain.cell_mut(17, 16).unwrap().overlay_blocks = true;
    sim.terrain_costs =
        crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(&prior_terrain);
    let prior_path = std::sync::Arc::new(PathGrid::from_resolved_terrain(&prior_terrain));
    sim.path_grid = Some(prior_path.clone());
    sim.zone_grid = Some(ZoneGrid::build_with_terrain(
        &path,
        &BTreeMap::new(),
        Some(terrain),
        &[],
        33,
        33,
    ));
    let base = sim.zone_grid.as_mut().unwrap().base_topology_mut().unwrap();
    base.levels.fill(200);
    base.movement_classes.fill(6);
    let ids_before = base.zone_ids.clone();
    let rows_before = base.raw_zone_ids_by_row.clone();
    let mut host = LivePublication {
        sim: &mut sim,
        rules: &rules,
        registry: Some(&registry),
        collapsed: false,
    };
    let cell = host.lookup((16, 16));
    host.recalc_cell(cell, 4).unwrap();
    let terrain_cell = sim.resolved_terrain.as_ref().unwrap().cell(16, 16).unwrap();
    assert_eq!(terrain_cell.level, 4);
    let mut expected_classes = vec![6; 33 * 33];
    let mut expected_levels = vec![200; 33 * 33];
    expected_classes[16 * 33 + 16] = terrain_cell.zone_type;
    expected_levels[16 * 33 + 16] = terrain_cell.level;
    let base = sim.zone_grid.as_mut().unwrap().base_topology_mut().unwrap();
    assert_eq!(base.movement_classes, expected_classes);
    assert_eq!(base.levels, expected_levels);
    assert_eq!(base.zone_ids, ids_before);
    assert_eq!(base.raw_zone_ids_by_row, rows_before);
    let current = sim.path_grid().unwrap();
    assert_eq!(current.cell(16, 16).unwrap().ground_level, 4);
    assert!(current.is_walkable(16, 16));
    assert_eq!(current.cell(17, 16), prior_path.cell(17, 16));
    assert_eq!(prior_path.cell(16, 16).unwrap().ground_level, 6);
    for speed in crate::rules::locomotor_type::SpeedType::ALL_WITH_COSTS {
        let costs = &sim.terrain_costs[speed];
        assert_eq!(costs.cost_at(17, 16), 0, "unrelated cached cost changed");
        assert_eq!(
            costs.cost_at(16, 16),
            crate::sim::pathfinding::terrain_cost::TerrainCostGrid::from_resolved_terrain(
                sim.resolved_terrain.as_ref().unwrap(),
                *speed
            )
            .cost_at(16, 16)
        );
    }
}

#[test]
fn live_bridge_constructor_matches_original_and_drains_at_admitted_tick() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bridge_constructor.json"
    ))
    .unwrap();
    let original_cases = corpus["cases"].as_array().unwrap();
    assert_eq!(original_cases.len(), 36);
    for original in original_cases {
        let (mut sim, rules, registry) = fixture();
        match original["kind"].as_str().unwrap() {
            "terrain" => {
                sim.production.terrain_object_cells.insert((16, 16), 919);
            }
            "slope" => {
                sim.resolved_terrain
                    .as_mut()
                    .unwrap()
                    .cell_mut(16, 16)
                    .unwrap()
                    .slope_type = 5;
            }
            "overrides" => {
                sim.overlay_grid
                    .as_mut()
                    .unwrap()
                    .place_overlay(16, 16, 7, 0);
                sim.resolved_terrain.as_mut().unwrap().cells[16 * 33 + 16]
                    .bridge_facts
                    .overlay_id = Some(7);
            }
            "success" => (),
            other => panic!("unexpected native case {other}"),
        }
        let requested = (
            original["requested"][0].as_i64().unwrap() as i16,
            original["requested"][1].as_i64().unwrap() as i16,
        );
        let mut host = LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: Some(&registry),
            collapsed: false,
        };
        let handle = host
            .construct_bridge_overlay(
                requested,
                original["overlay_id"].as_u64().unwrap() as u8,
                -1,
            )
            .unwrap();
        let snapshot = sim.load_objects.snapshot(handle).unwrap();
        assert_eq!(
            json!(snapshot.world),
            original["object_world"],
            "{original}"
        );
        for (key, value) in [
            ("alive", snapshot.alive),
            ("limbo", snapshot.limbo),
            ("on_map", snapshot.on_map),
            ("redraw", snapshot.redraw),
        ] {
            assert_eq!(
                u64::from(value),
                original[key].as_u64().unwrap(),
                "{key}: {original}"
            );
        }
        assert_eq!(
            json!(sim.load_objects.registry_counts()),
            original["registry_counts"]
        );
        assert_eq!(
            json!(sim.load_objects.queue_count()),
            original["queue_count"]
        );
        assert_eq!(json!(snapshot.native_id.unwrap()), original["native_id"]);
        assert_eq!(cells(&sim), original["cells"], "{original}");
        assert_eq!(dummy(&sim), original["dummy"], "{original}");
        assert_eq!(sim.native_unique_ids.as_ref().unwrap().current_raw(), 1001);
        // This is the production frame admission/drain, not a direct test-only
        // destruction call. Original corpus separately executes725C70's body.
        sim.advance_tick(&[], None, &BTreeMap::new(), None, None, 67);
        assert_eq!(
            json!(sim.load_objects.registry_counts()),
            original["after_drain"]["registry_counts"]
        );
        assert_eq!(
            json!(sim.load_objects.queue_count()),
            original["after_drain"]["queue_count"]
        );
        assert_eq!(cells(&sim), original["after_drain"]["cells"]);
        assert_eq!(dummy(&sim), original["after_drain"]["dummy"]);
        assert_eq!(sim.native_unique_ids.as_ref().unwrap().current_raw(), 1001);
    }
}

#[test]
fn live_bridge_constructor_queue_respects_terminal_admission_and_other_objects() {
    let (mut sim, rules, registry) = fixture();
    for (index, requested) in [(16, 16), (17, 16)].into_iter().enumerate() {
        let mut host = LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: Some(&registry),
            collapsed: false,
        };
        host.construct_bridge_overlay(requested, 24, -1).unwrap();
        let id = 100 + index as u64;
        let mut object =
            crate::sim::game_entity::GameEntity::test_default(id, "E1", "Americans", 16, 16);
        object.type_ref = sim.interner.intern("E1");
        object.owner = sim.interner.intern("Americans");
        sim.substrate.entities.insert(object);
        sim.reveal(id);
        if index == 0 {
            sim.uninit(id);
        } else {
            sim.substrate.pending_delete.push(id);
        }
    }
    let retained_cells = cells(&sim);
    let retained_dummy = dummy(&sim);
    sim.quit_requested = true;
    sim.advance_tick(&[], None, &BTreeMap::new(), None, None, 67);
    assert_eq!(sim.load_objects.queue_count(), 2);
    assert_eq!(sim.load_objects.registry_counts(), [2; 5]);
    assert!(sim.substrate.entities.get(100).is_some());
    assert!(sim.substrate.pending_delete.contains(&100));
    assert!(sim.substrate.pending_delete.contains(&101));
    sim.quit_requested = false;
    sim.advance_tick(&[], None, &BTreeMap::new(), None, None, 67);
    assert_eq!(sim.load_objects.queue_count(), 0);
    assert_eq!(sim.load_objects.registry_counts(), [0; 5]);
    assert!(sim.substrate.entities.get(100).is_none());
    assert!(sim.substrate.entities.get(101).is_some());
    assert_eq!(sim.substrate.pending_delete, vec![101]);
    assert_eq!(cells(&sim), retained_cells);
    assert_eq!(dummy(&sim), retained_dummy);
    assert_eq!(sim.native_unique_ids.as_ref().unwrap().current_raw(), 1002);
}

#[test]
fn live_bridge_constructor_restamp_updates_existing_runtime_axis() {
    let (mut sim, rules, registry) = fixture();
    {
        let mut host = LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: Some(&registry),
            collapsed: false,
        };
        host.construct_bridge_overlay((16, 16), 25, -1).unwrap();
    }
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain(
        sim.resolved_terrain.as_ref().unwrap(),
        true,
        1,
    ));
    assert_eq!(
        sim.bridge_state
            .as_ref()
            .unwrap()
            .cell(16, 16)
            .unwrap()
            .axis,
        Some(Axis::EW)
    );
    let mut host = LivePublication {
        sim: &mut sim,
        rules: &rules,
        registry: Some(&registry),
        collapsed: false,
    };
    host.construct_bridge_overlay((16, 16), 24, -1).unwrap();
    let selected = host.lookup((16, 16));
    assert_eq!(host.state(selected), 0);
    let runtime = host
        .sim
        .bridge_state
        .as_ref()
        .unwrap()
        .cell(16, 16)
        .unwrap();
    assert_eq!(runtime.axis, Some(Axis::NS));
    assert_eq!(runtime.overlay_byte, 24);
}
