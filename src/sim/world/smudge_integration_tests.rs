//! End-to-end determinism + snapshot tests for the smudge system.
//!
//! These tests verify the determinism contract for SmudgeGrid: identical seeds
//! plus identical SmudgeGrid mutations must produce identical state hashes,
//! state hashes must be stable across `advance_tick` calls, and a SmudgeGrid
//! survives a snapshot round-trip with state-hash equality preserved.
//!
//! Approach: seed SmudgeGrid directly via the `#[cfg(test)]` `test_force_set`
//! helper rather than driving the full combat -> anim -> smudge_dispatch
//! pipeline. The combat path requires a complete RuleSet, terrain grid,
//! occupancy, overlay registry, etc.; the contract being verified here is
//! purely about determinism + serialization, so the simpler seeding is enough.

use super::Simulation;
use crate::sim::smudge_grid::{SmudgeCell, SmudgeGrid};
use crate::sim::snapshot::GameSnapshot;

/// Build a Simulation with a populated SmudgeGrid seeded with a fixed pattern.
///
/// Same seed -> same starting state. The pattern uses small but nontrivial
/// data so the hash is sensitive to all SmudgeCell fields (type_id,
/// footprint_origin, frame_offset).
fn build_test_sim_with_seed(seed: u64) -> Simulation {
    let mut sim = Simulation::with_seed(seed);
    let mut grid = SmudgeGrid::new(16, 16);

    // A 1x1 crater + a 2x2 footprint laid out across 4 cells.
    grid.test_force_set(
        2,
        3,
        SmudgeCell {
            type_id: Some(0),
            footprint_origin: Some((2, 3)),
            frame_offset: 0,
        },
    );
    // 2x2 footprint at origin (5,5).
    let w: u8 = 2;
    for dy in 0..2u16 {
        for dx in 0..2u16 {
            grid.test_force_set(
                5 + dx,
                5 + dy,
                SmudgeCell {
                    type_id: Some(1),
                    footprint_origin: Some((5, 5)),
                    frame_offset: (dx as u8) + (dy as u8) * w,
                },
            );
        }
    }

    sim.smudge_grid = Some(grid);
    sim
}

/// Run `advance_tick` n times with empty inputs.
fn advance_n(sim: &mut Simulation, n: u32) {
    for _ in 0..n {
        sim.advance_tick(&[], None, None, None, 33);
    }
}

#[test]
fn smudge_state_hash_stable_across_advance_tick() {
    // SmudgeGrid is mutated only by combat events (none fired here). Across
    // 100 empty ticks, the smudge contribution to the hash must stay constant
    // and both sims must agree at every tick. This verifies determinism of
    // the smudge-hashing path under the normal advance loop.
    let mut sim_a = build_test_sim_with_seed(42);
    let mut sim_b = build_test_sim_with_seed(42);

    for tick in 0..100 {
        advance_n(&mut sim_a, 1);
        advance_n(&mut sim_b, 1);
        assert_eq!(
            sim_a.state_hash(),
            sim_b.state_hash(),
            "state_hash diverged at tick {} (sim_a.tick={}, sim_b.tick={})",
            tick,
            sim_a.session.tick(),
            sim_b.session.tick(),
        );
    }

    // Sanity: every seeded cell is still present (advance_tick didn't clobber
    // SmudgeGrid in either sim).
    let occ_a = sim_a
        .smudge_grid
        .as_ref()
        .map(|g| g.iter_occupied().count())
        .unwrap_or(0);
    let occ_b = sim_b
        .smudge_grid
        .as_ref()
        .map(|g| g.iter_occupied().count())
        .unwrap_or(0);
    assert_eq!(occ_a, occ_b);
    assert_eq!(occ_a, 5, "expected 1 (1x1) + 4 (2x2) = 5 occupied cells");
}

#[test]
fn smudge_grid_survives_snapshot_roundtrip() {
    // Build, advance, save, load, advance further on both — final hashes
    // must match.
    let mut sim_orig = build_test_sim_with_seed(7);
    advance_n(&mut sim_orig, 25);

    // Native in-scenario load resets Scenario RNG; isolate smudge persistence
    // by comparing against that same post-load baseline.
    sim_orig.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash_at_save = sim_orig.state_hash();
    let bytes = GameSnapshot::save(&sim_orig, 0, 0, "smudge_roundtrip_test", 0);
    let snap = GameSnapshot::load(&bytes).expect("snapshot load must succeed");
    let mut sim_restored = snap.sim;

    assert_eq!(
        sim_restored.state_hash(),
        hash_at_save,
        "restored sim must hash identically to the source at save time",
    );

    // Verify the SmudgeGrid is preserved cell-for-cell.
    let orig_cells: Vec<_> = sim_orig
        .smudge_grid
        .as_ref()
        .unwrap()
        .iter_occupied()
        .map(|(rx, ry, c)| (rx, ry, c.type_id, c.footprint_origin, c.frame_offset))
        .collect();
    let restored_cells: Vec<_> = sim_restored
        .smudge_grid
        .as_ref()
        .unwrap()
        .iter_occupied()
        .map(|(rx, ry, c)| (rx, ry, c.type_id, c.footprint_origin, c.frame_offset))
        .collect();
    assert_eq!(
        orig_cells, restored_cells,
        "SmudgeGrid contents must round-trip through bincode unchanged",
    );

    // Continue both sims for another 25 ticks — both must reach the same
    // final hash, proving the snapshot didn't corrupt any state needed by
    // future ticks.
    advance_n(&mut sim_orig, 25);
    advance_n(&mut sim_restored, 25);
    assert_eq!(
        sim_orig.state_hash(),
        sim_restored.state_hash(),
        "original and restored sim must reach identical state after 25 more ticks",
    );
}

#[test]
fn gsi_04_11_smudge_dirty_flush_publishes_tactical_then_radar_per_cell() {
    let mut sim = Simulation::with_seed(3);
    let mut grid = SmudgeGrid::new(8, 8);
    for (rx, ry) in [(3, 4), (4, 4), (3, 5), (4, 5)] {
        grid.test_force_set(
            rx,
            ry,
            SmudgeCell {
                type_id: Some(1),
                footprint_origin: Some((3, 4)),
                frame_offset: ((rx - 3) + (ry - 4) * 2) as u8,
            },
        );
    }
    sim.smudge_grid = Some(grid);
    Simulation::flush_smudge_dirty(&mut sim.smudge_grid, &mut sim.dirty_cells);

    let expected = vec![(3, 4), (4, 4), (3, 5), (4, 5)];
    assert_eq!(sim.dirty_cells.tactical_for_test(), expected);
    assert_eq!(sim.dirty_cells.radar(), expected);
    assert_eq!(sim.dirty_cells.radar_generation(), 4);
    assert!(sim.smudge_grid.as_mut().unwrap().drain_dirty().is_empty());
}

/// Whole native Execute118/588AC0 controls pin footprint decoding, first-null
/// traversal and identity-only writes. This replays the conditional supplied
/// map/type fields, not complete retail mission or rendered-output parity.
#[test]
fn action118_matches_original_smudge_fields_dummy_and_first_null_controls() {
    use crate::map::resolved_terrain::{test_flat_cell, test_grid};
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use serde_json::Value;
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/smudge_clear_all.json",
    ))
    .unwrap();
    assert_eq!(corpus["kind"], "bounded-original-clear-all-smudges");
    let mut ini = "[SmudgeTypes]\n".to_string();
    for (index, row) in corpus["supplied_types"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        ini.push_str(&format!("{index}={}\n", row["name"].as_str().unwrap()));
    }
    for row in corpus["supplied_types"].as_array().unwrap() {
        ini.push_str(&format!(
            "[{}]\nWidth={}\nHeight={}\n",
            row["name"].as_str().unwrap(),
            row["width"],
            row["height"],
        ));
    }
    let rules = RuleSet::from_ini(&IniFile::from_str(&ini)).unwrap();
    let pair = |value: &Value| {
        (
            value[0].as_i64().unwrap() as u16,
            value[1].as_i64().unwrap() as u16,
        )
    };
    for row in corpus["rows"].as_array().unwrap() {
        let mut sim = Simulation::with_seed(19);
        let mut terrain = test_grid(10, 10, test_flat_cell);
        let allocated: Vec<_> = row["cells_after"]
            .as_array()
            .unwrap()
            .iter()
            .map(|cell| pair(&cell["cell"]))
            .collect();
        // These are the actual original-constructed cell pointer slots. Clear
        // never changes allocation; the partial row stops at its first null.
        terrain.test_set_native_allocated_cells(&allocated);
        let before = &corpus["dummy_coord_before"];
        terrain.shared_cell_dummy().stamp_coord(
            before[0].as_i64().unwrap() as i32,
            before[1].as_i64().unwrap() as i32,
        );
        if let Some(identity) = row["input"]["dummy"]["smudge"].as_i64() {
            terrain.shared_cell_dummy().write_smudge_identity_data(
                identity as i32,
                row["input"]["dummy"]["smudge_data"].as_u64().unwrap() as u8,
            );
        }
        let mut grid = SmudgeGrid::new(10, 10);
        for input in row["input"]["cells"].as_array().unwrap() {
            grid.test_force_set(
                input["x"].as_u64().unwrap() as u16,
                input["y"].as_u64().unwrap() as u16,
                SmudgeCell {
                    type_id: Some(input["smudge"].as_u64().unwrap() as u16),
                    // This cache is not native authority. Intentionally stale
                    // metadata must not replace the retained data-byte decode.
                    footprint_origin: Some((9, 9)),
                    frame_offset: input["smudge_data"].as_u64().unwrap() as u8,
                },
            );
        }
        let _ = grid.drain_dirty();
        sim.smudge_grid = Some(grid);
        sim.install_resolved_terrain_for_new_map(terrain);
        sim.install_playfield_from_map_header(&crate::map::map_file::MapHeader {
            theater: "TEMPERATE".into(),
            fill: "Clear".into(),
            level: 0,
            width: 6,
            height: 4,
            local_left: 0,
            local_top: 0,
            local_width: 6,
            local_height: 4,
        });
        let rng_before = sim.scenario_rng.native_state_hex();
        assert!(sim.trigger_clear_all_smudges(&rules), "{}", row["name"]);
        assert_eq!(sim.scenario_rng.native_state_hex(), rng_before);
        assert_eq!(row["returned"], 1);
        assert_eq!(row["rng_unchanged"], true);
        let grid = sim.smudge_grid.as_ref().unwrap();
        for expected in row["cells_after"].as_array().unwrap() {
            let at = pair(&expected["cell"]);
            let cell = grid.cell(at.0, at.1);
            assert_eq!(
                cell.type_id.map_or(-1, i32::from),
                expected["identity"].as_i64().unwrap() as i32,
                "{} {at:?}",
                row["name"],
            );
            assert_eq!(
                cell.frame_offset,
                expected["data"].as_u64().unwrap() as u8,
                "{} {at:?} retained data",
                row["name"],
            );
        }
        let dummy = sim.resolved_terrain.as_ref().unwrap().shared_cell_dummy();
        let native_dummy = &row["dummy_after"];
        let native_at = pair(&native_dummy["coord"]);
        assert_eq!(
            dummy.snapshot().coord,
            (i32::from(native_at.0 as i16), i32::from(native_at.1 as i16))
        );
        assert_eq!(
            dummy.smudge_identity_data(),
            (
                native_dummy["smudge"].as_i64().unwrap() as i32,
                native_dummy["smudge_data"].as_u64().unwrap() as u8,
            ),
        );
        let changed: Vec<_> = row["cell_writes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|write| write["dummy"] == false && write["previous"] != -1)
            .map(|write| pair(&write["cell"]))
            .collect();
        assert_eq!(sim.dirty_cells.tactical_for_test(), changed);
        assert_eq!(sim.dirty_cells.radar(), changed);
        assert!(sim.smudge_grid.as_mut().unwrap().drain_dirty().is_empty());
    }
}
