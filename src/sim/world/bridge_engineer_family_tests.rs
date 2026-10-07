use super::*;
use crate::map::resolved_terrain::test_flat_cell;
use crate::rules::ini_parser::IniFile;
use crate::sim::{components::DriveCoord, movement::ground_pose};
use serde_json::{Value, json};

#[test]
fn engineer_family_selector_matches_original_boundary_and_dummy() {
    let cases: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/engineer_family_selector.json",
    ))
    .unwrap();
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=ENGINEER\n[ENGINEER]\nEngineer=yes\nStrength=75\nSpeed=4\n",
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let input = &case["input"];
        let output = &case["output"];
        let mut terrain = ResolvedTerrainGrid::from_cells(
            16,
            16,
            (0..16)
                .flat_map(|y| {
                    (0..16).map(move |x| {
                        let mut c = test_flat_cell(x, y);
                        c.final_tile_index = 65535;
                        c
                    })
                })
                .collect(),
        );
        terrain.test_set_high_bridge_set_starts(
            None,
            Some(input["wood_base"].as_u64().unwrap() as u16),
        );
        for row in input["cells"].as_array().unwrap() {
            let x = row[0].as_u64().unwrap() as usize;
            let y = row[1].as_u64().unwrap() as usize;
            let c = &mut terrain.cells[y * 16 + x];
            c.final_tile_index = row[2].as_i64().unwrap() as i32;
            c.bridge_facts.overlay_id = u8::try_from(row[3].as_i64().unwrap()).ok();
        }
        let allocated: Vec<_> = (8..=12)
            .flat_map(|y| (8..=12).map(move |x| (x, y)))
            .filter(|&(x, y)| json!([x, y]) != input["missing"])
            .collect();
        terrain.test_set_native_allocated_cells(&allocated);
        let mut sim = Simulation::with_seed(31);
        sim.resolve_type_handles(&rules);
        sim.install_resolved_terrain_for_new_map(terrain);
        let engineer = sim
            .construct_object_limbo_at_height("ENGINEER", "Americans", 10, 10, 0, 10, &rules)
            .unwrap();
        {
            let actor = sim.substrate.entities.get_mut(engineer).unwrap();
            // Original engineer_family_selector.py supplies this complete
            // Location before519C07; constructor/Unlimbo are not its boundary.
            ground_pose::put_location(
                &mut actor.position,
                DriveCoord {
                    x: 2688,
                    y: 2688,
                    z: 1040,
                },
            );
            actor.position.z = 10;
        }
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((1234, -2345));
        let rng = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        let mut live = LivePublication {
            sim: &mut sim,
            rules: &rules,
            registry: None,
            collapsed: false,
        };
        let (argument, family) = engineer_repair_family(&mut live, engineer).unwrap();
        assert_eq!(json!(argument), output["repair_argument"], "{input}");
        assert_eq!(
            if family == Family::Low { "low" } else { "high" },
            output["family"].as_str().unwrap(),
            "{input}"
        );
        let dummy = live.terrain().shared_cell_dummy().snapshot().coord;
        assert_eq!(
            json!([dummy.0 as i16, dummy.1 as i16]),
            output["dummy_cell"],
            "{input}"
        );
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            rng
        );
    }
    assert_eq!(cases.as_array().unwrap().len(), 14);
}
