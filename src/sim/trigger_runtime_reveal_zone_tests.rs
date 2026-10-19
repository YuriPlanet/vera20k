//! Replays the selected original18 controls through the existing action,
//! map-query, raw-zone and Fog owners. The supplied topology is a premise,
//! not a second connectivity producer or retail-map parity claim.
use super::*;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::{
    NativeCellQuery, NativeLocalCellCursor, ResolvedTerrainGrid, test_clear_cell, test_grid,
};
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::vision::{FogState, OwnerVisibility};
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_reveal_zone.json")
}

fn number(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn rng_hashes(sim: &Simulation) -> Value {
    let hash = |hex: String| {
        let bytes: Vec<_> = hex
            .as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        crate::util::sha256::sha256_hex(&bytes)
    };
    json!({"scenario":hash(sim.scenario_rng.native_state_hex()),
        "main":hash(sim.main_rng.native_state_hex()),"mapgen":hash(sim.mapgen_rng.native_state_hex())})
}

fn supplied_cells(row: &Value) -> (ResolvedTerrainGrid, Vec<(u16, u16)>) {
    let mut terrain = test_grid(128, 128, test_clear_cell);
    let size = (number(&row["size"][0]), number(&row["size"][1]));
    // The oracle constructs this declared49x49 window, admitting the native
    // Size diamond. These are input allocations, not expected reveal cells.
    let allocated: Vec<_> = (67..=115u16)
        .flat_map(|y| (44..=92u16).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            crate::map::playfield::size_diamond_contains(size.0, size.1, (x as i16, y as i16))
        })
        .collect();
    assert_eq!(
        allocated.len(),
        row["allocated_cells"].as_u64().unwrap() as usize
    );
    let source = &row["input"]["cell"];
    let identity =
        terrain.native_cell_identity((number(&source[0]) as i16, number(&source[1]) as i16));
    terrain.write_native_cell_level(identity, number(&row["input"]["level"]) as u8);
    let mut live = allocated.clone();
    if let Some(removed) = row["removed_cell"].as_array() {
        live.retain(|&(x, y)| {
            (i32::from(x), i32::from(y)) != (number(&removed[0]), number(&removed[1]))
        });
    }
    terrain.test_set_native_allocated_cells(&live);
    (terrain, allocated)
}

fn bounds(row: &Value) -> crate::map::playfield::PlayfieldBounds {
    let local = &row["local_size"];
    crate::map::playfield::PlayfieldBounds::from_normalized_local_size(
        number(&row["size"][0]),
        number(&local[0]),
        number(&local[1]),
        number(&local[2]),
        number(&local[3]),
    )
}

#[test]
fn native_local_iterator_preserves_height_order_and_continues_after_null_member() {
    let corpus = corpus();
    for row in corpus["controls"].as_array().unwrap() {
        if row["input"]["latch"] == 1 {
            continue;
        }
        let (terrain, _) = supplied_cells(row);
        let query = NativeCellQuery::canonical(&terrain);
        let mut cursor = NativeLocalCellCursor::reset(bounds(row));
        let mut actual = Vec::new();
        loop {
            let identity = cursor.next_in_query(bounds(row), &query);
            actual.push(json!({
                "kind": "next",
                "cell": identity.map(|cell| { let coord=query.coord(cell); [coord.0, coord.1] }),
                "dummy": identity == Some(NativeCellIdentity::Dummy),
            }));
            if identity.is_none() {
                break;
            }
        }
        assert_eq!(json!(actual), row["next"], "{}", row["name"]);
    }
}

#[test]
fn native_reveal_zone_dispatch_uses_literal_waypoint_raw_zone_and_radius_two() {
    let corpus = corpus();
    for row in corpus["controls"].as_array().unwrap() {
        let name = row["name"].as_str().unwrap();
        let (terrain, allocated) = supplied_cells(row);
        let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain(&terrain);
        let mut clusters = vec![0u16; 128 * 128];
        for &(x, y) in &allocated {
            clusters[usize::from(y) * 128 + usize::from(x)] =
                if i32::from(x) <= number(&row["cluster_split_x"]) {
                    2
                } else {
                    3
                };
        }
        // The saved-plane reader is this owner's existing authoritative
        // fixture boundary. It installs the explicitly supplied raw plane
        // and13 rows, not zones inferred by Rust's flood fill.
        let zones = serde_json::from_value(json!({
            "width":128,"height":128,
            "base_topology":{
                "native_bridge_source_size":[number(&row["size"][0]),number(&row["size"][1])],
                "movement_classes":vec![0u8;128*128],"levels":vec![0u8;128*128],
                "zone_ids":clusters,
                "raw_zone_ids_by_row":vec![row["raw_row"].clone();13],
            }
        }))
        .unwrap();
        let mut sim = Simulation::with_seed(31);
        sim.zone_grid = Some(zones);
        sim.install_resolved_terrain_for_new_map(terrain);
        sim.install_fixture_path_grid(Some(&path));
        sim.playfield_bounds = Some(bounds(row));
        sim.playfield_size_height = Some(number(&row["size"][1]));
        let player = sim.interner.intern("Player");
        let other = sim.interner.intern("Other");
        sim.session.current_house = Some(player);
        sim.fog = FogState::new(128, 128);
        if row["input"]["latch"] == 1 {
            sim.fog.reveal_all_for_owner(player);
        }
        for viewer in [player, other] {
            sim.fog
                .by_owner
                .insert(viewer, OwnerVisibility::new(128, 128));
        }
        let mut definitions = RuntimeDefinitions::default();
        let typ = definitions.trigger_type("REVEAL").unwrap();
        definitions.trigger_types[typ].owner = Some("Other".into());
        let actions = IniFile::from_str(&format!(
            "[Actions]\nREVEAL={}\n",
            row["read"]["read"].as_str().unwrap()
        ));
        let entry = crate::map::actions::parse_actions(&actions)
            .remove("REVEAL")
            .unwrap()
            .entries()[0]
            .clone();
        let action = definitions.resolve_action(entry, None, None);
        assert_eq!(json!(action.value), row["read"]["value"]);
        assert_eq!(json!(action.entry.waypoint_index), row["read"]["waypoint"]);
        let waypoint = number(&row["read"]["value"]) as u32;
        let source = &row["input"]["cell"];
        let mut waypoints = std::collections::HashMap::new();
        if let Some(tail) = action.entry.waypoint_index {
            waypoints.insert(
                tail,
                crate::map::waypoints::Waypoint {
                    index: tail,
                    rx: 48,
                    ry: 76,
                },
            );
        }
        waypoints.insert(
            waypoint,
            crate::map::waypoints::Waypoint {
                index: waypoint,
                rx: number(&source[0]) as u16,
                ry: number(&source[1]) as u16,
            },
        );
        sim.session.install_waypoints(&waypoints);
        sim.trigger_runtime.definitions = definitions;
        // Radius0 deliberately differs from18's native literal2; only17
        // consumes this key. Positive source heights exercise both shifts.
        let rules = RuleSet::from_ini(&IniFile::from_str(
            "[General]\nRevealTriggerRadius=0\nRevealByHeight=yes\n",
        ))
        .unwrap();
        sim.mapgen_rng = crate::sim::rng::SimRng::new(31);
        assert_eq!(
            rng_hashes(&sim),
            row["rng_before"],
            "{name}: supplied original seed states"
        );
        let states = (
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        );
        let mut effects = Vec::new();
        let (returned, draws) = crate::sim::rng::trace_draws(|| {
            sim.apply_live_trigger_action(
                &action,
                sim.trigger_action_context(typ, None, TagEventInput::default(), Some(&rules)),
                &mut effects,
                Some(&rules),
                None,
                FrameEffects::default(),
            )
        });
        assert_eq!(i32::from(returned), number(&row["returned_al"]), "{name}");
        assert!(draws.is_empty() && effects.is_empty(), "{name}");
        assert_eq!(
            (
                sim.scenario_rng.native_state_hex(),
                sim.main_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ),
            states,
            "{name}"
        );
        assert_eq!(
            rng_hashes(&sim),
            row["rng_after"],
            "{name}: retained original stream words"
        );
        assert_eq!(
            json!(sim.session.binary_frame),
            row["frame_after"],
            "{name}"
        );
        assert_eq!(
            sim.fog.whole_map_revealed_for_owner(player),
            row["latch_after"] == 1,
            "{name}"
        );
        let query = NativeCellQuery::canonical(sim.resolved_terrain.as_ref().unwrap());
        let expected: BTreeSet<_> = row["final_cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| {
                (
                    number(&cell["cell"][0]) as u16,
                    number(&cell["cell"][1]) as u16,
                )
            })
            .collect();
        let opened: BTreeSet<_> = allocated
            .iter()
            .copied()
            .filter(|&(x, y)| {
                !crate::sim::vision::point_is_shrouded(
                    &sim.fog,
                    &query,
                    player,
                    crate::sim::components::DriveCoord {
                        x: i32::from(x) * 256 + 128,
                        y: i32::from(y) * 256 + 128,
                        z: 0,
                    },
                )
            })
            .collect();
        assert_eq!(
            opened, expected,
            "{name}: native ground knowledge including missing-member Dummy"
        );
        for cell in row["final_cells"].as_array().unwrap() {
            let (x, y) = (
                number(&cell["cell"][0]) as u16,
                number(&cell["cell"][1]) as u16,
            );
            if query.lookup((x as i16, y as i16)) != NativeCellIdentity::Dummy {
                let state = sim.fog.by_owner[&player].shroud_knowledge_raw()
                    [usize::from(y) * 128 + usize::from(x)];
                assert_eq!(
                    state.counter,
                    number(&cell["counter"]),
                    "{name}: counter {x},{y}"
                );
                assert_eq!(
                    state.gap_counter,
                    number(&cell["gap"]),
                    "{name}: gap {x},{y}"
                );
                assert_eq!(
                    state.local_sources + state.allied_sources,
                    0,
                    "{name}: no sustained source"
                );
            }
        }
        assert!(
            allocated
                .iter()
                .all(|&(x, y)| !sim.fog.is_ground_unshrouded(other, x, y)),
            "{name}: PlayerPtr is distinct from trigger House"
        );
    }
}
