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

fn vectors() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/bridge_click_state_oracle/vectors.json")
}

fn runtime(direction: u8) -> SimRuntime {
    let mut sim = Simulation::new();
    sim.session_mut_for_test()
        .set_map_name_for_test("BRIDGE-CLICK.MAP".into());
    sim.session_mut_for_test().set_map_width_for_test(32);
    sim.session_mut_for_test().set_map_height_for_test(32);
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
    *sim.overlay_grid_mut_for_test() = Some(OverlayGrid::new(32, 32));
    sim.apply_runtime_bridge_mark_stamp_for_test(
        BridgeFlagStamp::new((16, 16), direction, true),
        if direction == 0 {
            BridgeStampFamily::Nesw
        } else {
            BridgeStampFamily::Nwse
        },
    );
    let template = sim.resolved_terrain().cloned().unwrap();
    let mut runtime = SimRuntime::from_simulation(sim);
    runtime.resources.terrain_template = Some(template);
    runtime
}

/// Supply a native reader input through the existing test accessors. As in
/// LivePublication::retain_real_write, keep the dynamic-cell owner coherent so
/// the supplied replacement survives a subsequent save/restore.
fn set_fixture_level(runtime: &mut SimRuntime, x: u16, y: u16, level: u8) {
    let cell = runtime
        .simulation
        .resolved_terrain_mut_for_test()
        .as_mut()
        .unwrap()
        .cell_mut_for_test(x, y)
        .unwrap();
    cell.level = level;
    let saved = cell.clone();
    runtime
        .simulation
        .dynamic_terrain_cells_mut_for_test()
        .record(&saved);
}

fn assert_clicks(runtime: &SimRuntime, row: &serde_json::Value) {
    let (orders, owner, movers) = commandable_movers();
    let before = runtime.simulation.state_hash();
    let dummy = runtime
        .simulation
        .resolved_terrain()
        .unwrap()
        .shared_cell_dummy()
        .snapshot();
    for case in row["cases"].as_array().unwrap() {
        let point = (
            case["world_input"][0].as_f64().unwrap() as f32,
            case["world_input"][1].as_f64().unwrap() as f32,
        );
        let expected = (
            case["expected_cell"][0].as_u64().unwrap() as u16,
            case["expected_cell"][1].as_u64().unwrap() as u16,
        );
        let actual = world_point_to_cell(
            point.0,
            point.1,
            crate::app::match_runtime::sim_tick::tactical_cells(&runtime.simulation),
        );
        assert_eq!(
            actual, expected,
            "phase={} direction={} point={point:?}",
            row["phase"], row["direction"]
        );
        // Carry the live result through the same ordinary MegaMission codec
        // used by context_order::finish_order for both Unit and Infantry.
        for mover in movers {
            ordinary_move(&orders, owner, mover, actual);
        }
    }
    assert_eq!(
        runtime.simulation.state_hash(),
        before,
        "picking must be read-only"
    );
    assert_eq!(
        runtime
            .simulation
            .resolved_terrain()
            .unwrap()
            .shared_cell_dummy()
            .snapshot(),
        dummy
    );
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
fn prepared_load_clicks_read_saved_cells_against_intact_template() {
    let vectors = vectors();
    for direction in [0, 6] {
        for (phase, rows) in [("collapsed", "rows"), ("anchor_raised", "level_rows")] {
            let mut runtime = runtime(direction);
            if phase == "collapsed" {
                runtime
                    .simulation
                    .apply_runtime_bridge_flag_stamp(BridgeFlagStamp::new(
                        (16, 16),
                        direction,
                        false,
                    ));
            } else {
                set_fixture_level(&mut runtime, 16, 16, 6);
            }
            let directory = std::env::temp_dir().join(format!(
                "vera-bridge-click-restore-{}-{direction}-{phase}",
                std::process::id()
            ));
            let repository = SaveRepository::at(&directory);
            let path = repository
                .write_named(
                    "changed.bin",
                    &GameSnapshot::save_validated(
                        &runtime.simulation,
                        91,
                        runtime.resources.rules.simulation_config_hash(),
                        phase,
                        1,
                    ),
                )
                .unwrap();
            // Restore into an outgoing world back at the original height and
            // intact flags. Both that world and the load template are stale.
            runtime
                .simulation
                .apply_runtime_bridge_flag_stamp(BridgeFlagStamp::new((16, 16), direction, true));
            set_fixture_level(&mut runtime, 16, 16, 2);
            let prepared = PreparedLoad::from_repository(
                LoadPreparationView::from_runtime(&repository, Some(&runtime), Some(91)),
                &path,
            )
            .unwrap();
            prepared.commit_into(&mut runtime);
            std::fs::remove_dir_all(directory).unwrap();
            let row = vectors[rows]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["direction"] == direction && row["phase"] == phase)
                .unwrap();
            let cell = runtime
                .simulation
                .resolved_terrain()
                .unwrap()
                .cell(16, 16)
                .unwrap();
            assert_eq!(
                cell.bridge_facts.has_structural_bridge(),
                phase != "collapsed"
            );
            assert_eq!(cell.level, if phase == "collapsed" { 2 } else { 6 });
            assert_clicks(&runtime, row);
        }
    }
}

#[test]
fn tactical_clicks_follow_native_live_level_transitions() {
    for direction in [0, 6] {
        let mut runtime = runtime(direction);
        for row in vectors()["level_rows"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["direction"] == direction)
        {
            for write in row["level_writes"].as_array().unwrap() {
                let (x, y, level) = (
                    write[0].as_u64().unwrap() as u16,
                    write[1].as_u64().unwrap() as u16,
                    write[2].as_u64().unwrap() as u8,
                );
                set_fixture_level(&mut runtime, x, y, level);
            }
            // assert_clicks uses the same app click owner as tactical input.
            // The unchanged load template deliberately retains the initial2;
            // it must never supply the live query after a level write.
            assert_clicks(&runtime, row);
        }
    }
}

#[test]
#[ignore = "slow; requires retail maps and ordinary bridge damage/Engineer repair"]
fn retail_bridge_level_changes_reach_tactical_inverse() {
    use crate::sim::world::bridge_test_evidence::{
        RetailHighBridgeScene, visit_anytown_concrete_stages, visit_retail_high_bridge_stages,
    };
    let output = std::env::var_os("VERA20K_LIVE_TERRAIN_OUTPUT").map(std::path::PathBuf::from);
    let mut total_changed = 0;
    for (label, kind) in [
        ("hills", Some(RetailHighBridgeScene::HillsWood)),
        ("pacific", Some(RetailHighBridgeScene::PacificConcrete)),
        ("c3-middle", Some(RetailHighBridgeScene::C3MiddleConcrete)),
        ("anytown", None),
    ] {
        let mut initial: Option<ResolvedTerrainGrid> = None;
        let mut states = Vec::new();
        let mut observe = |phase: &str, scene: &crate::headless_scenario::HeadlessScenario| {
            let live = scene.sim().resolved_terrain().unwrap();
            if phase == "loaded" {
                initial = Some(live.clone());
                if label == "c3-middle" {
                    let (_, assets) = crate::rules::retail_ini_fixture::retail_assets().unwrap();
                    let raw = assets.get_ref("C3Y03MD.MAP").unwrap();
                    assert_eq!(
                        crate::util::sha256::sha256_hex(raw),
                        vectors()["middle_rows"][0]["source_map_sha256"]
                            .as_str()
                            .unwrap()
                    );
                }
            }
            let initial = initial.as_ref().unwrap();
            let before = scene.sim().state_hash();
            let dummy = live.shared_cell_dummy().snapshot();
            if label == "c3-middle" {
                let native = &vectors()["middle_rows"][0];
                let point = &native["world_input"];
                let expected = if phase == "collapsed" {
                    &native["after"]
                } else {
                    &native["before"]
                };
                assert_eq!(
                    world_point_to_cell(
                        point[0].as_f64().unwrap() as f32,
                        point[1].as_f64().unwrap() as f32,
                        Some(live)
                    ),
                    (
                        expected["expected_cell"][0].as_u64().unwrap() as u16,
                        expected["expected_cell"][1].as_u64().unwrap() as u16
                    ),
                    "physical C3 middle/{phase} must match the original inverse witness"
                );
            }
            let changes: Vec<_> = live
                .cells()
                .iter()
                .filter(|cell| {
                    initial
                        .cell(cell.rx, cell.ry)
                        .is_some_and(|old| old.level != cell.level)
                })
                .collect();
            let mut witness = None;
            'search: for cell in &changes {
                let origin = crate::map::terrain::iso_to_screen(cell.rx, cell.ry, cell.level);
                for dy in (-45..=45).step_by(5) {
                    for dx in (-30..=30).step_by(5) {
                        let point = (origin.0 + dx as f32, origin.1 + dy as f32);
                        let current = world_point_to_cell(point.0, point.1, Some(live));
                        let stale = world_point_to_cell(point.0, point.1, Some(initial));
                        if current != stale {
                            witness = Some((point, current, stale));
                            break 'search;
                        }
                    }
                }
            }
            assert_eq!(scene.sim().state_hash(), before);
            assert_eq!(live.shared_cell_dummy().snapshot(), dummy);
            println!(
                "{label}/{phase}: {} changed levels, sample={witness:?}",
                changes.len()
            );
            if let Some((point, current, stale)) = witness {
                total_changed += 1;
                let centre = current;
                let bounds = [centre.0 - 12, centre.1 - 12, centre.0 + 12, centre.1 + 12];
                let cells = |grid: &ResolvedTerrainGrid| {
                    (bounds[1]..=bounds[3])
                        .flat_map(|y| {
                            (bounds[0]..=bounds[2]).map(move |x| {
                                let cell = grid
                                    .cell(x, y)
                                    .expect("retail witness crop is all real cells");
                                [
                                    u32::from(x),
                                    u32::from(y),
                                    u32::from(cell.level),
                                    cell.bridge_facts.raw_flags,
                                ]
                            })
                        })
                        .collect::<Vec<_>>()
                };
                states.push(serde_json::json!({"phase":phase,"bounds":bounds,
                    "world_input":[point.0 as i32,point.1 as i32],
                    "rust_current":[current.0,current.1],"rust_stale":[stale.0,stale.1],
                    "initial_cells":cells(initial),"current_cells":cells(live)}));
                if let Some(directory) = &output {
                    std::fs::create_dir_all(directory).unwrap();
                    std::fs::write(
                        directory.join(format!("{label}-inputs.json")),
                        serde_json::to_vec_pretty(&states).unwrap(),
                    )
                    .unwrap();
                }
            }
        };
        if let Some(kind) = kind {
            visit_retail_high_bridge_stages(kind, &mut observe);
        } else {
            visit_anytown_concrete_stages(&mut observe);
        }
        if let Some(directory) = &output {
            std::fs::create_dir_all(directory).unwrap();
            std::fs::write(
                directory.join(format!("{label}-inputs.json")),
                serde_json::to_vec_pretty(&states).unwrap(),
            )
            .unwrap();
        }
    }
    assert!(
        total_changed > 0,
        "retail bridge publication must exercise a stale-height click"
    );
}
