//! Original MapCell/cache controls. Fixture inputs are native supplied storage;
//! expected values are executed bytes, never a second edge-frame algorithm.
use super::*;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_clear_cell};
use crate::sim::vision::MapTransition;
use serde_json::{Value, json};

pub(crate) fn discovery_corpus() -> Value {
    let rows: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    rows.into_iter()
        .find(|row| row["case_kind"] == "trigger_reveal_discovery")
        .unwrap()
}

pub(crate) fn pair(value: &Value) -> (u16, u16) {
    (
        value[0].as_i64().unwrap() as u16,
        value[1].as_i64().unwrap() as u16,
    )
}

/// Build precisely the supplied native allocated slots, including fixed512-row
/// aliases. Slot location and the Cell's stored+24 coordinate are distinct.
/// Fog indexes the latter; NativeCellQuery owns the former and the Dummy.
pub(crate) fn native_map_cell_fixture(
    topology: &Value,
    viewer: InternedId,
) -> (FogState, ResolvedTerrainGrid) {
    let inputs: Vec<Value> = topology["allocated_cells"]
        .as_array()
        .unwrap()
        .iter()
        .map(|input| {
            if input.is_array() {
                let mut row = topology["cell_constructor_prior"].clone();
                row["cell"] = json!([input[0], input[1]]);
                row["level"] = input[2].clone();
                row
            } else {
                input.clone()
            }
        })
        .collect();
    let slots: Vec<_> = inputs
        .iter()
        .map(|cell| {
            let (x, y) = pair(&cell["cell"]);
            let linear =
                crate::map::cell_index::cell_linear_index(i32::from(x as i16), i32::from(y as i16))
                    .unwrap();
            ((linear % 512) as u16, (linear / 512) as u16)
        })
        .collect();
    let width = slots.iter().map(|p| p.0).max().unwrap() + 1;
    let height = slots.iter().map(|p| p.1).max().unwrap() + 1;
    let mut cells = (0..height)
        .flat_map(|y| (0..width).map(move |x| test_clear_cell(x, y)))
        .collect::<Vec<_>>();
    for (input, &(x, y)) in inputs.iter().zip(&slots) {
        let (rx, ry) = pair(&input["cell"]);
        let mut cell = test_clear_cell(rx, ry);
        cell.level = input["level"].as_i64().unwrap() as u8;
        cell.bridge_facts.raw_flags = input["flags"].as_u64().unwrap() as u32 & !0x63;
        cells[usize::from(y) * usize::from(width) + usize::from(x)] = cell;
    }
    let mut terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
    terrain.test_set_native_allocated_cells(&slots);
    let dummy = &topology["dummy_prior"];
    terrain.test_set_dummy_cell_level_slope(dummy["level"].as_i64().unwrap() as i8, 0);
    let (x, y) = pair(&dummy["cell"]);
    terrain.stamp_dummy_cell_requested_coord(i32::from(x as i16), i32::from(y as i16));
    let width = inputs
        .iter()
        .map(|cell| pair(&cell["cell"]).0)
        .max()
        .unwrap()
        + 1;
    let height = inputs
        .iter()
        .map(|cell| pair(&cell["cell"]).1)
        .max()
        .unwrap()
        + 1;
    let mut fog = FogState::new(width, height);
    let mut view = OwnerVisibility::new(width, height);
    for input in &inputs {
        let (x, y) = pair(&input["cell"]);
        let key = VisibilityCell::Real(view.index(x, y).unwrap());
        seed_cell(&mut view, key, input);
    }
    seed_cell(&mut view, VisibilityCell::Dummy, dummy);
    // A supplied native state bypasses the writers' lists.
    view.edge_cells = EdgeCells::default();
    fog.by_owner.insert(viewer, view);
    (fog, terrain)
}

fn seed_cell(view: &mut OwnerVisibility, key: VisibilityCell, input: &Value) {
    let (knowledge, ground, cache) = view.map_cell_parts_mut(key);
    let flags = input["flags"].as_u64().unwrap() as u32;
    *ground = input["alt"].as_u64().unwrap() as u8;
    knowledge.counter = input["counter"].as_i64().unwrap() as i32;
    knowledge.gap_counter = input["gap"].as_i64().unwrap() as i32;
    knowledge.open = *ground & 0x18 == 0x18;
    knowledge.pending = flags & 0x20 != 0;
    *cache = MapCellEdgeCache {
        initialized: flags & 2 != 0,
        fog_open: flags & 1 != 0,
        frames: [
            input["cache"][0].as_i64().unwrap() as i8,
            input["cache"][1].as_i64().unwrap() as i8,
        ],
    };
    if let VisibilityCell::Real(index) = key {
        view.publish_knowledge(index);
    }
}

#[test]
fn native_forced_map_frontier_cache_and_notifications_preserve_counters() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_forced_map.json",
    ))
    .unwrap();
    let viewer = crate::sim::intern::test_intern("Player");
    let (mut fog, terrain) = native_map_cell_fixture(&corpus["topology"], viewer);
    for row in corpus["controls"].as_array().unwrap() {
        let query = NativeCellQuery::canonical(&terrain);
        let notification = fog.reveal_forced_map_cell(viewer, pair(&row["cell"]), Some(&query));
        assert_eq!(
            notification.is_some(),
            row["returned_al"] == 1,
            "{}",
            row["name"]
        );
        let expected = row["notifications"].as_array().unwrap().first();
        assert_eq!(
            notification
                .as_ref()
                .map(|event| json!([event.cell.0, event.cell.1, event.fresh])),
            expected.map(|event| json!([event["cell"][0], event["cell"][1], event["fresh"] == 1])),
            "{}",
            row["name"]
        );
        for cell in row["cells"].as_array().unwrap() {
            assert_native_cell_history(&fog, viewer, cell, &terrain, false);
        }
        //5865F0's later empty-cell discovery stamps the Dummy after this leaf
        //returns its notification. Compare the retained coordinate only in
        //the unchanged case, which executes no notification receiver.
        if notification.is_none() {
            assert_native_cell_history(&fog, viewer, &row["dummy"], &terrain, true);
        }
        assert_eq!(row["rng_before"], row["rng_after"]);
        assert!(row["rng_calls"].as_array().unwrap().is_empty());
        let restored: FogState = bincode::deserialize(&bincode::serialize(&fog).unwrap()).unwrap();
        for cell in row["cells"].as_array().unwrap() {
            assert_native_cell_history(&restored, viewer, cell, &terrain, false);
        }
    }
}

#[test]
fn bulk_map_store_shares_forced_reveal_admission_without_changing_frame_bytes() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_forced_map.json",
    ))
    .unwrap();
    let viewer = crate::sim::intern::test_intern("Player");
    let (mut fog, terrain) = native_map_cell_fixture(&corpus["topology"], viewer);
    let query = NativeCellQuery::canonical(&terrain);
    fog.reveal_forced_map_cell(viewer, (9, 10), Some(&query));
    let frames = fog.by_owner[&viewer]
        .map_cell_edge_cache_for_test((9, 10))
        .0;
    for cell in corpus["topology"]["allocated_cells"].as_array().unwrap() {
        fog.by_owner
            .get_mut(&viewer)
            .unwrap()
            .store_bulk_map_cell(pair(&cell["cell"]), false);
    }
    //577ED1/577EE0 writes bits18/3, leaving120/121 and pending20.
    let view = &fog.by_owner[&viewer];
    assert_eq!(view.map_cell_edge_cache_for_test((9, 10)), (frames, 3));
    assert!(view.shroud_knowledge[view.index(9, 10).unwrap()].pending);
    let notification = fog
        .reveal_forced_map_cell(viewer, (9, 10), Some(&query))
        .unwrap();
    assert!(!notification.fresh);
    let native_center = corpus["controls"][2]["cells"]
        .as_array()
        .unwrap()
        .iter()
        .find(|cell| pair(&cell["cell"]) == (9, 10))
        .unwrap();
    assert_eq!(
        fog.by_owner[&viewer]
            .map_cell_edge_cache_for_test((9, 10))
            .0,
        [
            native_center["cache"][0].as_i64().unwrap() as i8,
            native_center["cache"][1].as_i64().unwrap() as i8
        ]
    );
    fog.by_owner
        .get_mut(&viewer)
        .unwrap()
        .store_bulk_map_cell((9, 10), true);
    assert_eq!(
        fog.by_owner[&viewer]
            .map_cell_edge_cache_for_test((9, 10))
            .1,
        0
    );
    assert!(
        fog.reveal_forced_map_cell(viewer, (9, 10), Some(&query))
            .unwrap()
            .fresh
    );
}

pub(crate) fn assert_native_cell_history(
    fog: &FogState,
    viewer: InternedId,
    row: &Value,
    terrain: &ResolvedTerrainGrid,
    dummy: bool,
) {
    let view = &fog.by_owner[&viewer];
    let cell = pair(&row["cell"]);
    let key = if dummy {
        VisibilityCell::Dummy
    } else {
        VisibilityCell::Real(view.index(cell.0, cell.1).unwrap())
    };
    let (ground, cache) = view.map_cell_parts(key);
    let state = match key {
        VisibilityCell::Real(index) => view.shroud_knowledge[index],
        VisibilityCell::Dummy => view.dummy_visibility.knowledge,
    };
    assert_eq!(
        state.counter,
        row["counter"].as_i64().unwrap() as i32,
        "counter at {cell:?}"
    );
    assert_eq!(
        state.gap_counter,
        row["gap"].as_i64().unwrap() as i32,
        "gap at {cell:?}"
    );
    assert_eq!(
        u64::from(ground & 0x18),
        row["alt"].as_u64().unwrap() & 0x18,
        "ground at {cell:?}"
    );
    assert_eq!(
        state.pending,
        row["flags"].as_u64().unwrap() & 0x20 != 0,
        "pending at {cell:?}"
    );
    assert_eq!(json!(cache.frames), row["cache"], "edge bytes at {cell:?}");
    let flags = u64::from(cache.initialized) * 2 | u64::from(cache.fog_open);
    assert_eq!(
        flags,
        row["flags"].as_u64().unwrap() & 3,
        "map flags at {cell:?}"
    );
    if dummy {
        let actual = terrain.shared_cell_dummy().snapshot();
        assert_eq!(
            json!([actual.coord.0, actual.coord.1]),
            row["cell"],
            "canonical Dummy ordering"
        );
        assert_eq!(i64::from(actual.level), row["level"].as_i64().unwrap());
    }
}

#[test]
fn native_reshroud_leave_and_signed_clear_gate_preserve_gap_and_edge_bytes() {
    // --reshroud executes487690 and the actual577D26..577D4D mask branch.
    // Expected values are produced by that selected original run, including
    // signed overflow; this test does not carry a second counter algorithm.
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_reshroud.json",
    ))
    .unwrap();
    let viewer = crate::sim::intern::test_intern("Player");
    for row in corpus["scalar_controls"].as_array().unwrap() {
        let topology = json!({
            "allocated_cells": [row["input"]],
            "dummy_prior": corpus["controls"][0]["topology"]["dummy_prior"],
        });
        let (mut fog, terrain) = native_map_cell_fixture(&topology, viewer);
        let cell = pair(&row["input"]["cell"]);
        let view = fog.by_owner.get_mut(&viewer).unwrap();
        let index = view.index(cell.0, cell.1).unwrap();
        view.shroud_knowledge[index].leave();
        assert_native_cell_history(&fog, viewer, &row["after_leave"], &terrain, false);

        // Start from the same supplied native prior and call the production
        // combined receiver. Its negative-counter branch must retain bits18/3.
        let (mut fog, _) = native_map_cell_fixture(&topology, viewer);
        fog.by_owner
            .get_mut(&viewer)
            .unwrap()
            .store_reshrouded_map_cell(cell);
        assert_native_cell_history(&fog, viewer, &row["stored"], &terrain, false);
        let loaded: FogState = bincode::deserialize(&bincode::serialize(&fog).unwrap()).unwrap();
        assert_native_cell_history(&loaded, viewer, &row["stored"], &terrain, false);
        assert_eq!(row["rng_before"], row["rng_after"], "{}", row["name"]);
        assert!(row["rng_calls"].as_array().unwrap().is_empty());
    }
}

#[test]
fn native_reshroud_allocated_cursor_stops_on_null_and_calls_shared_receiver() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_reshroud.json",
    ))
    .unwrap();
    let viewer = crate::sim::intern::test_intern("Player");
    for name in ["current_house", "null_house", "allocated_hole"] {
        let row = corpus["controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == name)
            .unwrap();
        let (mut fog, terrain) = native_map_cell_fixture(&row["topology"], viewer);
        let width = row["size_width"].as_i64().unwrap() as i32;
        let cells: Vec<_> = terrain
            .native_cell_iterator(width)
            .map(|cell| (cell.rx, cell.ry))
            .collect();
        assert_eq!(json!(cells), row["visits"], "{name}");
        assert!(fog.admit_whole_map_reveal(viewer));
        let mut world = crate::sim::world::Simulation::default();
        world.fog = fog;
        world.resolved_terrain = Some(terrain.clone());
        world.transition_whole_map_with_gap_reentry(
            viewer,
            cells,
            MapTransition::Reshroud,
            false,
            &std::collections::BTreeMap::new(),
            Some(&[]),
            &std::collections::BTreeMap::new(),
            &crate::sim::vision::VisionConfig::default(),
            crate::sim::world::UninitContext::default(),
        );
        let fog = world.fog;
        assert_eq!(
            fog.whole_map_revealed_for_owner(viewer),
            row["houses"]["player"]["revealed"] == 1,
            "{name}"
        );
        for cell in row["cells"].as_array().unwrap() {
            assert_native_cell_history(&fog, viewer, cell, &terrain, false);
        }
        // Null allocated slots do not query or restamp the shared Dummy.
        assert_native_cell_history(&fog, viewer, &row["dummy"], &terrain, true);
        assert_eq!(row["rng_before"], row["rng_after"], "{name}");
        assert!(row["rng_calls"].as_array().unwrap().is_empty());
    }
}

#[test]
fn native_trigger_reveal_edge_channels_match_all_original_masks_and_center_gates() {
    let corpus = discovery_corpus();
    let controls = &corpus["cache_controls"];
    let viewer = crate::sim::intern::test_intern("Player");
    let (mut fog, terrain) = native_map_cell_fixture(&controls["topology"], viewer);
    let cells = NativeCellQuery::canonical(&terrain);
    let center = (9, 10);
    for channel in 0..2usize {
        for (mask, expected) in controls["channel_mask_outputs"][channel]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let view = fog.by_owner.get_mut(&viewer).unwrap();
            let index = view.index(center.0, center.1).unwrap();
            if channel == 0 {
                view.ground_flags[index] = 8;
            } else {
                view.map_cell_edge_cache[index].initialized = true;
            }
            for input in controls["neighbor_masks"].as_array().unwrap() {
                let x = (9i16).wrapping_add(input[0].as_i64().unwrap() as i16) as u16;
                let y = (10i16).wrapping_add(input[1].as_i64().unwrap() as i16) as u16;
                let index = view.index(x, y).unwrap();
                let set = mask & input[2].as_u64().unwrap() as usize == 0;
                if channel == 0 {
                    view.ground_flags[index] = if set { 8 } else { 0 };
                } else {
                    view.map_cell_edge_cache[index].initialized = set;
                }
            }
            assert_eq!(
                i64::from(fog.map_cell_edge_frame(viewer, center, channel != 0, Some(&cells))),
                expected.as_i64().unwrap(),
                "channel{channel} mask{mask}"
            );
        }
    }
    for input in controls["center_gates"].as_array().unwrap() {
        let channel = input["channel"].as_u64().unwrap();
        let value = input["center"].as_u64().unwrap() as u8;
        let view = fog.by_owner.get_mut(&viewer).unwrap();
        let index = view.index(9, 10).unwrap();
        if channel == 0 {
            view.ground_flags[index] = value;
        } else {
            view.map_cell_edge_cache[index].initialized = value & 2 != 0;
            view.map_cell_edge_cache[index].fog_open = value & 1 != 0;
        }
        assert_eq!(
            i64::from(fog.map_cell_edge_frame(viewer, center, channel != 0, Some(&cells))),
            input["returned"].as_i64().unwrap()
        );
    }
}

#[test]
fn native_trigger_reveal_retained_cache_admission_matches_original_histories() {
    let corpus = discovery_corpus();
    let controls = &corpus["cache_controls"];
    let viewer = crate::sim::intern::test_intern("Player");
    let (mut fog, terrain) = native_map_cell_fixture(&controls["topology"], viewer);
    for history in controls["histories"].as_array().unwrap() {
        let cells = NativeCellQuery::canonical(&terrain);
        let notify = fog.reveal_map_cell(
            viewer,
            pair(&history["cell"]),
            history["final"] == 1,
            Some(&cells),
            crate::sim::vision::MapRevealProjection::Transient,
        );
        let expected = history["trace"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["pc"] == "005865F0");
        assert_eq!(
            notify
                .as_ref()
                .map(|event| json!([event.cell.0, event.cell.1, event.fresh])),
            expected.map(|event| json!([event["cell"][0], event["cell"][1], event["fresh"] == 1])),
            "{}",
            history["stage"]
        );
        for row in history["states"].as_array().unwrap() {
            assert_native_cell_history(&fog, viewer, row, &terrain, false);
        }
        let restored: FogState = bincode::deserialize(&bincode::serialize(&fog).unwrap()).unwrap();
        for row in history["states"].as_array().unwrap() {
            assert_native_cell_history(&restored, viewer, row, &terrain, false);
        }
        assert_eq!(
            restored.by_owner[&viewer].map_cell_edge_cache_for_test(pair(&history["cell"])),
            fog.by_owner[&viewer].map_cell_edge_cache_for_test(pair(&history["cell"]))
        );
    }
}

/// RevealCell4AA050 over every cell of a bare 5x4 plane in row order. With no
/// terrain, off-plane neighbors read the never-explored Dummy. Each cell is
/// explored and notifies once, fresh; no counter or current sight moves; only
/// cells whose eight neighbors are all on the plane open (`0x10`).
#[test]
fn reveal_cell_explores_every_cell_and_opens_only_surrounded_ones() {
    let viewer = crate::sim::intern::test_intern("Player");
    let mut fog = FogState::new(5, 4);
    let mut notified = Vec::new();
    for y in 0..4 {
        for x in 0..5 {
            let notify = fog
                .reveal_cell(viewer, (x, y), None)
                .expect("an unexplored cell changes");
            assert!(notify.fresh);
            notified.push(notify.cell);
        }
    }
    let rows: Vec<_> = (0..4).flat_map(|y| (0..5).map(move |x| (x, y))).collect();
    assert_eq!(notified, rows);
    let view = &fog.by_owner[&viewer];
    for (x, y) in rows {
        let index = view.index(x, y).unwrap();
        let surrounded = (1..4).contains(&x) && (1..3).contains(&y);
        let expected = if surrounded { 0x18 } else { 0x08 };
        assert_eq!(view.ground_flags[index], expected, "ground at {x},{y}");
        assert_eq!(
            view.shroud_knowledge[index].counter, 1,
            "counter at {x},{y}"
        );
        assert!(fog.is_cell_revealed(viewer, x, y));
        assert!(!fog.is_cell_visible(viewer, x, y));
        assert!(fog.is_ground_unshrouded(viewer, x, y));
    }
    assert_eq!(view.dummy_visibility, DummyVisibility::default());
}

/// The neighbor branch that opens a cell (`0x004AA17C..0x004AA204`) stores its
/// eight neighbors' frames but not its own. On a 3x3 plane only the middle
/// cell can be surrounded: opened as the last corner's neighbor, it keeps the
/// frame its last other neighbor gave it. Revealed last itself, its own -1
/// frame opens it.
#[test]
fn reveal_cell_opens_a_surrounded_neighbor_without_storing_its_frame() {
    let viewer = crate::sim::intern::test_intern("Player");
    let middle = (1, 1);
    let plane = |last: (u16, u16)| {
        let mut fog = FogState::new(3, 3);
        for y in 0..3 {
            for x in 0..3 {
                if (x, y) != last {
                    fog.reveal_cell(viewer, (x, y), None);
                }
            }
        }
        fog
    };
    let open = |fog: &FogState, cell: (u16, u16)| {
        let view = &fog.by_owner[&viewer];
        view.ground_flags[view.index(cell.0, cell.1).unwrap()] & 0x10 != 0
    };

    let corner = (2, 2);
    let mut fog = plane(corner);
    let (before, _) = fog.by_owner[&viewer].map_cell_edge_cache_for_test(middle);
    assert!(before[0] >= 0, "the corner is still unexplored: {before:?}");
    assert_eq!(
        fog.map_cell_edge_frame(viewer, middle, false, None),
        before[0]
    );
    assert!(fog.reveal_cell(viewer, corner, None).unwrap().fresh);
    assert!(open(&fog, middle));
    assert!(!open(&fog, corner), "a corner borders the unexplored Dummy");
    assert_eq!(
        fog.by_owner[&viewer].map_cell_edge_cache_for_test(middle).0,
        before
    );
    assert_eq!(fog.map_cell_edge_frame(viewer, middle, false, None), -1);
    // Its own visit stores the -1 and notifies the change, not fresh.
    assert!(!fog.reveal_cell(viewer, middle, None).unwrap().fresh);
    assert_eq!(
        fog.by_owner[&viewer].map_cell_edge_cache_for_test(middle).0[0],
        -1
    );

    let mut fog = plane(middle);
    assert!(!open(&fog, middle));
    assert!(fog.reveal_cell(viewer, middle, None).unwrap().fresh);
    assert!(open(&fog, middle));
    assert_eq!(
        fog.by_owner[&viewer].map_cell_edge_cache_for_test(middle).0[0],
        -1
    );
}

/// `reveal_cell` reads `frames` and `map_cell_edge_frame` reads `fog_open` of
/// cells whose cache is not initialized, so the state hash covers them too.
#[test]
fn uninitialized_edge_cache_fields_move_the_state_hash() {
    use crate::sim::state_hasher::StateHasher;
    use std::hash::Hasher;
    let hash = |visibility: &OwnerVisibility| {
        let mut hasher = StateHasher::new();
        visibility.hash_cells(&mut hasher);
        hasher.finish()
    };
    let base = OwnerVisibility::new(4, 4);
    assert!(!base.map_cell_edge_cache[5].initialized);
    let mut open = base.clone();
    open.map_cell_edge_cache[5].fog_open = true;
    let mut framed = base.clone();
    framed.map_cell_edge_cache[5].frames[0] = 3;
    assert_ne!(hash(&open), hash(&base));
    assert_ne!(hash(&framed), hash(&base));
}
