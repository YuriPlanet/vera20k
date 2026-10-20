//! Native Infantry51F660 wrapper and inherited Foot4D4200 controls.

use serde_json::Value;

use super::dispatch_foot_mission;
use crate::sim::deploy_tests::{assert_native_deploy_state, native_deploy_fixture};
use crate::sim::mission::MissionDispatchTimer;

#[test]
fn deployed_move_matches_native_action_navigation_timer_and_rng() {
    let data: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_mission_move.json",
    ))
    .unwrap();
    let rows = data.as_array().unwrap();
    assert_eq!(rows.len(), 16);
    for row in rows {
        let (mut sim, rules, id) = native_deploy_fixture(row);
        let now = sim.session.binary_frame();
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .mission
            .write_dispatch_epilogue(now as i32, 0);
        dispatch_foot_mission(&mut sim, id, &rules, Default::default());
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .mission
                .dispatch_timer(),
            MissionDispatchTimer::from_raw(
                now as i32,
                row["return_signed"].as_i64().unwrap() as i32
            ),
            "{}: original class caller return through the single timer epilogue",
            row["input"]["name"]
        );
        assert_native_deploy_state(&sim, id, row);
        assert_eq!(row["original_code_and_vtable_unchanged"], true);
    }
}
