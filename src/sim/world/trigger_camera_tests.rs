//! Original actions48/112 coordinate production, through the production
//! waypoint, CellClass query and ground-height owners. Focus/SetView and
//! their downstream presentation are outside these recorded controls.

use super::Simulation;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::map::waypoints::Waypoint;
use serde_json::{Value, json};
use std::collections::HashMap;

#[test]
fn waypoint_ground_bridge_and_dummy_coordinates_match_original_actions48_and112() {
    let corpus: Vec<Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    let native = corpus
        .iter()
        .find(|row| row["case_kind"] == "waypoint_coord")
        .expect("original waypoint coordinate controls");
    let rows = native["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 20);
    for row in rows {
        let input = &row["input"];
        let x = input["cell"][0].as_i64().unwrap() as i16;
        let y = input["cell"][1].as_i64().unwrap() as i16;
        let level = input["level"].as_i64().unwrap_or(0) as i8;
        let slope = input["slope"].as_u64().unwrap_or(0) as u8;
        let flags = input["flags"].as_u64().unwrap_or(0) as u32;
        let missing = input["missing"] == true;
        let mut terrain = ResolvedTerrainGrid::from_cells(
            32,
            32,
            (0..32)
                .flat_map(|y| (0..32).map(move |x| test_flat_cell(x, y)))
                .collect(),
        );
        // The native fixture's sparse512-stride table contains only the
        // supplied real cell, or no cells for its Dummy controls.
        if missing {
            terrain.test_set_native_allocated_cells(&[]);
            terrain.shared_cell_dummy().set_level_slope(level, slope);
        } else {
            terrain.test_set_native_allocated_cells(&[(x as u16, y as u16)]);
            terrain
                .cell_mut_for_test(x as u16, y as u16)
                .unwrap()
                .slope_type = slope;
        }
        let identity = if missing {
            NativeCellIdentity::Dummy
        } else {
            terrain.native_cell_identity((x, y))
        };
        terrain.write_native_cell_level(identity, level as u8);
        terrain.write_native_cell_flags(identity, flags);
        let mut sim = Simulation::with_seed(0);
        sim.install_resolved_terrain_for_new_map(terrain);
        let index = row["read"]["waypoint"].as_u64().unwrap() as u32;
        sim.session.install_waypoints(&HashMap::from([(
            index,
            Waypoint {
                index,
                rx: x as u16,
                ry: y as u16,
            },
        )]));
        let before_rng = sim.scenario_rng.logical_state();
        let before_frame = sim.session.binary_frame();
        let calls = row["calls"].as_array().unwrap();
        let sink = calls.last().unwrap();
        let kind = row["read"]["kind"].as_i64().unwrap();
        assert_eq!(sink["call"], if kind == 48 { "focus" } else { "set_view" });
        let actual = sim.trigger_waypoint_coord(index as i32).unwrap();
        assert_eq!(json!(actual), sink["coord"], "{input} action{kind}");
        assert_eq!(sim.scenario_rng.logical_state(), before_rng);
        assert_eq!(sim.session.binary_frame(), before_frame);
        if missing {
            // Ground's world lookup and the later packed lookup stamp the
            // same retained Dummy. Compare the final native observation,
            // including negative words; do not infer equal storage layouts.
            let last_lookup = calls
                .iter()
                .rev()
                .find(|call| call["call"] == "packed_lookup")
                .unwrap();
            let dummy = sim.effective_shared_cell_dummy().snapshot();
            assert_eq!(
                json!([dummy.coord.0, dummy.coord.1]),
                last_lookup["returned_cell"]
            );
            assert_eq!(dummy.level, level);
            assert_eq!(dummy.slope_type, slope);
            assert_eq!(sim.effective_shared_cell_dummy().raw_flags(), flags);
        }
    }
}

/// Original Steam3e81a617..08600 GetWaypointLocation68BD00 + Cell486840 /
/// 47B3A0, with Scenario CRT8148A8..8148CC deriving deck416. Sparse Cell
/// fields are declared inputs; this is not a full map-load comparison.
#[test]
fn eleven_original_waypoint_location_controls_keep_receiver_identity_and_signed_height() {
    for (request, real, level, slope, flags, point) in [
        ((12, 17), Some((12, 17)), 0, 0, 0, [3200, 4480, 0]),
        ((12, 17), Some((12, 17)), 2, 1, 0, [3200, 4480, 260]),
        ((12, 17), Some((12, 17)), 2, 0, 0x100, [3200, 4480, 624]),
        ((12, 17), Some((12, 17)), 2, 0, 0x400, [3200, 4480, 624]),
        ((512, 0), Some((0, 1)), 2, 0, 0, [128, 384, 208]),
        ((512, 0), Some((0, 1)), 2, 0, 0x100, [128, 384, 624]),
        ((-1, -1), None, -3, 0, 0, [-128, -128, -311]),
        ((-1, -1), None, -3, 0, 0x400, [-128, -128, 105]),
        (
            (32767, -32768),
            None,
            127,
            15,
            0,
            [8388480, -8388480, 13312],
        ),
        (
            (-32768, 32767),
            None,
            -128,
            4,
            0x500,
            [-8388480, 8388480, -12843],
        ),
        ((12, 17), Some((12, 17)), -3, 0, 0, [3200, 4480, -311]),
    ] {
        let mut terrain = crate::map::resolved_terrain::test_grid(32, 32, test_flat_cell);
        terrain.test_set_native_allocated_cells(&real.into_iter().collect::<Vec<_>>());
        let identity = if let Some((x, y)) = real {
            terrain.cell_mut_for_test(x, y).unwrap().slope_type = slope;
            terrain.native_cell_identity((x as i16, y as i16))
        } else {
            terrain.shared_cell_dummy().set_level_slope(level, slope);
            NativeCellIdentity::Dummy
        };
        terrain.write_native_cell_level(identity, level as u8);
        terrain.write_native_cell_flags(identity, flags);
        let mut sim = Simulation::with_seed(7);
        sim.install_resolved_terrain_for_new_map(terrain);
        sim.session.install_waypoints(&HashMap::from([(
            7,
            Waypoint {
                index: 7,
                rx: request.0 as u16,
                ry: request.1 as u16,
            },
        )]));
        let rng = sim.scenario_rng.native_state_hex();
        assert_eq!(
            sim.trigger_waypoint_location(7),
            Some(point),
            "{request:?} {real:?}"
        );
        assert_eq!(sim.scenario_rng.native_state_hex(), rng);
        if real.is_none() {
            assert_eq!(sim.effective_shared_cell_dummy().snapshot().coord, request);
        }
    }
}
