//! The production click owner must consume the current real CellClass flags,
//! including immediately after the existing setter and PreparedLoad commit.
//! Expected cells are original6D6590 outputs; this does not execute a UI/GPU.

use super::*;
use crate::app::persistence::{LoadPreparationView, PreparedLoad, SaveRepository};
use crate::map::bridge_facts::{BridgeFlagStamp, BridgeStampFamily};
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::runtime::SimRuntime;
use crate::sim::snapshot::GameSnapshot;

fn vectors() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/bridge_click_state_oracle/vectors.json",
    ))
    .unwrap()
}

fn runtime(direction: u8) -> SimRuntime {
    let mut sim = Simulation::new();
    sim.session.map_name = "BRIDGE-CLICK.MAP".into();
    sim.session.map_width = 32;
    sim.session.map_height = 32;
    let cells = (0..32)
        .flat_map(|y| {
            (0..32).map(move |x| {
                let mut cell = ResolvedTerrainCell::clear_for_test(x, y);
                cell.level = 2;
                cell
            })
        })
        .collect();
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(32, 32, cells));
    sim.overlay_grid = Some(OverlayGrid::new(32, 32));
    sim.apply_runtime_bridge_mark_stamp(
        BridgeFlagStamp::new((16, 16), direction, true),
        if direction == 0 {
            BridgeStampFamily::Nesw
        } else {
            BridgeStampFamily::Nwse
        },
    );
    let template = sim.resolved_terrain.clone().unwrap();
    let mut runtime = SimRuntime::from_simulation(sim);
    runtime.resources.terrain_template = Some(template);
    runtime
}

fn assert_clicks(runtime: &SimRuntime, row: &serde_json::Value) {
    for case in row["cases"].as_array().unwrap() {
        let point = (
            case["world_input"][0].as_f64().unwrap() as f32,
            case["world_input"][1].as_f64().unwrap() as f32,
        );
        let expected = (
            case["expected_cell"][0].as_u64().unwrap() as u16,
            case["expected_cell"][1].as_u64().unwrap() as u16,
        );
        assert_eq!(
            world_point_to_cell(
                point.0,
                point.1,
                &runtime
                    .resources
                    .terrain_template
                    .as_ref()
                    .unwrap()
                    .build_height_map(),
                crate::app::match_runtime::sim_tick::tactical_bridge_cells(&runtime.simulation)
            ),
            expected,
            "phase={} direction={} point={point:?}",
            row["phase"],
            row["direction"]
        );
    }
}

#[test]
fn collapse_and_repair_clicks_read_current_flags() {
    let vectors = vectors();
    for direction in [0, 6] {
        let mut runtime = runtime(direction);
        for row in vectors["rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["direction"] == direction)
        {
            runtime
                .simulation
                .apply_runtime_bridge_flag_stamp(BridgeFlagStamp::new(
                    (16, 16),
                    direction,
                    row["phase"] != "collapsed",
                ));
            assert_clicks(&runtime, row);
        }
    }
}

#[test]
fn prepared_load_clicks_read_saved_flags_against_intact_template() {
    let vectors = vectors();
    for direction in [0, 6] {
        let mut runtime = runtime(direction);
        runtime
            .simulation
            .apply_runtime_bridge_flag_stamp(BridgeFlagStamp::new((16, 16), direction, false));
        let directory = std::env::temp_dir().join(format!(
            "vera-bridge-click-restore-{}-{direction}",
            std::process::id()
        ));
        let repository = SaveRepository::at(&directory);
        let path = repository
            .write_named(
                "collapsed.bin",
                &GameSnapshot::save_validated(
                    &runtime.simulation,
                    91,
                    runtime.resources.rules.simulation_config_hash(),
                    "collapsed",
                    1,
                ),
            )
            .unwrap();
        runtime
            .simulation
            .apply_runtime_bridge_flag_stamp(BridgeFlagStamp::new((16, 16), direction, true));
        let prepared = PreparedLoad::from_repository(
            LoadPreparationView::from_runtime(&repository, Some(&runtime), Some(91)),
            &path,
        )
        .unwrap();
        prepared.commit_into(&mut runtime);
        std::fs::remove_dir_all(directory).unwrap();
        let row = vectors["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["direction"] == direction && row["phase"] == "collapsed")
            .unwrap();
        assert!(
            !runtime
                .view()
                .resolved_terrain()
                .unwrap()
                .cell(16, 16)
                .unwrap()
                .bridge_facts
                .has_structural_bridge()
        );
        assert_clicks(&runtime, row);
    }
}
