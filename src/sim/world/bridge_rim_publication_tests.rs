//! Actual damage-event caller against original body+perpendicular+rim output.
//! Stock scalar initialization is shared with Unicorn, not a loader comparison.

use super::*;
use crate::map::bridge_facts::BridgeCellFacts;
use crate::map::bridge_rim_tiles::HighBridgeRimTiles;
use serde_json::{Value, json};

fn stock_world(rules: &RuleSet, input: &Value) -> Simulation {
    let rows = input["cells"].as_array().unwrap();
    let height = rows
        .iter()
        .map(|r| r[1].as_u64().unwrap() as u16)
        .max()
        .unwrap()
        + 1;
    let cells = (0..height)
        .flat_map(|y| {
            (0..512).map(move |x| {
                crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
            })
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(512, height, cells);
    let mut allocated = Vec::new();
    let mut grid = OverlayGrid::new_with_retained_wall_plane(512, height);
    for row in rows {
        let (x, y, tile, subtile, flags, overlay, state, anchor, level, land): (
            u16,
            u16,
            i32,
            u8,
            u32,
            Option<u8>,
            u8,
            Option<(u16, u16)>,
            u8,
            u8,
        ) = serde_json::from_value(row.clone()).unwrap();
        let index = usize::from(y) * 512 + usize::from(x);
        allocated.push((x, y));
        let cell = &mut terrain.cells[index];
        cell.final_tile_index = tile;
        cell.final_sub_tile = subtile;
        cell.level = level;
        cell.yr_cell_land_type = land;
        cell.bridge_facts = BridgeCellFacts {
            state_byte: state,
            overlay_id: overlay,
            family: if flags & 0x1f80 != 0 {
                BridgeStampFamily::Nesw
            } else {
                BridgeStampFamily::None
            },
            direction: (flags & 0x1f80 != 0).then_some(6),
            native_anchor: anchor
                .filter(|_| flags & 0x80 == 0)
                .map(|(x, y)| Cell::Real(usize::from(y) * 512 + usize::from(x))),
            ..Default::default()
        };
        terrain.write_native_cell_flags(Cell::Real(index), flags);
        if let Some(overlay) = overlay {
            grid.place_overlay(x, y, overlay, state);
        }
        grid.write_literal_bridge_state(&mut terrain, x, y, state);
    }
    terrain.test_set_native_allocated_cells(&allocated);
    let mut ini = String::from("[General]\n");
    for (key, value) in input["rim_keys"].as_object().unwrap() {
        ini.push_str(&format!("{key}={value}\n"));
    }
    terrain.test_set_high_bridge_rim_tiles(HighBridgeRimTiles::from_ini(
        input["bridge_base"].as_i64().unwrap() as i32,
        ini.as_bytes(),
    ));
    let size = serde_json::from_value(input["size"].clone()).unwrap();
    let state = BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 1, size);
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.bridge_state = Some(state);
    sim.overlay_grid = Some(grid);
    sim
}

fn snapshot(sim: &Simulation, coord: (u16, u16)) -> Value {
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    let cell = terrain.cell(coord.0, coord.1).unwrap();
    json!({"coord": coord, "flags": cell.bridge_facts.raw_flags,
        "state": cell.bridge_facts.state_byte,
        "overlay": cell.bridge_facts.overlay_id.map_or(-1, i32::from),
        "anchor": cell.bridge_facts.native_anchor.map(|a| terrain.native_cell_coord(a))})
}

#[test]
fn bridge_rim_stock_damage_events_match_original_body_perpendicular_and_cleanup() {
    let input: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bridge_rim_stock_inputs.json"
    ))
    .unwrap();
    let original: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bridge_rim_body.json"
    ))
    .unwrap();
    let coords: Vec<(u16, u16)> = input["cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| (r[0].as_u64().unwrap() as u16, r[1].as_u64().unwrap() as u16))
        .collect();
    let rules = rules();
    for case in original["cases"].as_array().unwrap() {
        let mut sim = stock_world(&rules, &input);
        let mut expected: std::collections::BTreeMap<_, _> =
            coords.iter().map(|&c| (c, snapshot(&sim, c))).collect();
        for hit in case["hits"].as_array().unwrap() {
            let coord = serde_json::from_value(hit["input"].clone()).unwrap();
            let mut damage = event(&mut sim, coord);
            damage.impact_z = i32::from(
                sim.resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(coord.0, coord.1)
                    .unwrap()
                    .level,
            );
            sim.radar_terrain_dirty_cells.clear();
            let collapsed =
                apply_bridge_damage_events_with_overlay_registry(&mut sim, &rules, &[damage], None);
            let calls = hit["calls"].as_array().unwrap();
            assert_eq!(
                collapsed,
                calls.iter().any(|c| c["kind"] == "fallout_sink"),
                "{} at{coord:?}",
                case["name"]
            );
            for change in hit["changes"].as_array().unwrap() {
                let coord = serde_json::from_value(change["before"]["coord"].clone()).unwrap();
                assert_eq!(expected[&coord], change["before"]);
                expected.insert(coord, change["after"].clone());
            }
            for (&coord, wanted) in &expected {
                assert_eq!(
                    &snapshot(&sim, coord),
                    wanted,
                    "{} hit{} at{coord:?}",
                    case["name"],
                    hit["input"]
                );
            }
            let mut radar: Vec<(u16, u16)> = Vec::new();
            for call in calls.iter().filter(|c| c["kind"] == "radar_sink") {
                let coord = serde_json::from_value(call["coord"].clone()).unwrap();
                if !radar.contains(&coord) {
                    radar.push(coord);
                }
            }
            assert_eq!(
                sim.radar_terrain_dirty_cells, radar,
                "{} hit{} radar",
                case["name"], hit["input"]
            );
            for change in hit["changes"].as_array().unwrap() {
                let coord: (u16, u16) =
                    serde_json::from_value(change["after"]["coord"].clone()).unwrap();
                let terrain = sim
                    .resolved_terrain
                    .as_ref()
                    .unwrap()
                    .cell(coord.0, coord.1)
                    .unwrap();
                assert_eq!(
                    sim.dynamic_terrain_cells[&coord],
                    DynamicTerrainCellState::capture(terrain)
                );
                if change["before"]["flags"].as_u64().unwrap() & 0x100 != 0
                    && change["after"]["flags"].as_u64().unwrap() & 0x100 == 0
                {
                    assert!(!terrain.has_bridge_deck && !terrain.bridge_walkable);
                    assert!(
                        !sim.path_grid()
                            .unwrap()
                            .cell(coord.0, coord.1)
                            .unwrap()
                            .bridge_walkable
                    );
                    assert!(
                        !sim.bridge_state
                            .as_ref()
                            .unwrap()
                            .cell(coord.0, coord.1)
                            .unwrap()
                            .deck_present
                    );
                }
            }
        }
    }
}

#[test]
fn bridge_rim_middle_section_fallout_and_restored_navigation() {
    let input: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/bridge_rim_stock_inputs.json"
    ))
    .unwrap();
    let rules = rules();
    let mut sim = stock_world(&rules, &input);
    let pristine = sim.resolved_terrain.as_ref().unwrap().clone();
    let victim = sim
        .construct_object_limbo_at_height("MTNK", "Americans", 111, 142, 0, 0, &rules)
        .unwrap();
    sim.reveal(victim);
    for (index, coord) in [(112, 140), (112, 140), (112, 144), (112, 144)]
        .into_iter()
        .enumerate()
    {
        let mut damage = event(&mut sim, coord);
        damage.impact_z = i32::from(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(coord.0, coord.1)
                .unwrap()
                .level,
        );
        apply_bridge_damage_events_with_overlay_registry(&mut sim, &rules, &[damage], None);
        let alive = sim
            .substrate
            .entities
            .get(victim)
            .is_some_and(|unit| unit.health.current > 0);
        assert_eq!(
            alive,
            index < 3,
            "only the final rim group clear reaches this ground occupant"
        );
    }
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "stock-rim", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(pristine, Default::default(), Vec::new(), Vec::new());
    restored
        .restore_map_authority_after_snapshot_load(
            &rules,
            &crate::map::overlay_types::OverlayTypeRegistry::empty(),
        )
        .unwrap();
    for y in 140..=144 {
        for x in 110..=113 {
            assert_eq!(snapshot(&sim, (x, y)), snapshot(&restored, (x, y)));
            assert!(
                !restored
                    .path_grid()
                    .unwrap()
                    .cell(x, y)
                    .unwrap()
                    .bridge_walkable
            );
            assert!(
                !restored
                    .bridge_state
                    .as_ref()
                    .unwrap()
                    .cell(x, y)
                    .unwrap()
                    .deck_present
            );
        }
    }
    assert_eq!(sim.dynamic_terrain_cells, restored.dynamic_terrain_cells);
    assert!(
        restored
            .resolved_terrain
            .as_ref()
            .unwrap()
            .high_bridge_rim_tiles()
            .is_some()
    );
}
