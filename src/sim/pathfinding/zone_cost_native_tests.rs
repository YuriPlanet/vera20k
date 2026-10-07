//! Original42D170/42C290 comparisons. Graphs, paths, costs and pop bits come
//! from native execution; no expected path is calculated by another search.
use super::*;
use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
use crate::sim::pathfinding::{PathGrid, zone_map::ZoneGrid};
use crate::sim::world::Simulation;
use serde_json::Value;

const WIDTH: u16 = 160;

fn threat_packet() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/zone_threat.json",
    ))
    .unwrap()
}

fn grid_value(grid: &Value, index: i32) -> i32 {
    grid["overrides"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row[0].as_i64().unwrap() == i64::from(index))
        .map_or(grid["default"].as_i64().unwrap() as i32, |row| {
            row[1].as_i64().unwrap() as i32
        })
}

#[test]
fn original_zone_threat_sampling_matches_all_directions_and_signed_average() {
    let packet = threat_packet();
    assert_eq!(
        packet["corners"],
        serde_json::json!([0, 0, 4, 0, 0, 4, 4, 4])
    );
    for row in packet["helpers"].as_array().unwrap() {
        let reads = std::cell::RefCell::new(Vec::new());
        let lookup = |index| {
            reads.borrow_mut().push(index);
            Ok(grid_value(&row["grid"], index))
        };
        let mut graph = ZoneLevelGraph::new(2);
        for (zone, key) in [(1, "source_index"), (2, "target_index")] {
            let mut record = ZoneRecord::new(zone, 0, 0);
            record.native_threat_index = Some(row[key].as_i64().unwrap() as i32);
            graph.set_record(record);
        }
        let answer = estimate_zone_threat(
            &graph,
            row["level"].as_u64().unwrap() as usize,
            1,
            2,
            &lookup,
        )
        .unwrap();
        assert_eq!(
            i64::from(answer),
            row["returned"].as_i64().unwrap(),
            "{}",
            row["name"]
        );
        let expected: Vec<i32> = row["reads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["index"].as_i64().unwrap() as i32)
            .collect();
        assert_eq!(*reads.borrow(), expected, "{} read order", row["name"]);
    }
}

#[test]
fn original_positive_zone_prechecks_match_routes_float_costs_and_threat_reads() {
    let packet = threat_packet();
    for row in packet["prechecks"].as_array().unwrap() {
        let graph = hierarchy(&row["graphs"], None);
        let reads = std::cell::RefCell::new(Vec::new());
        let lookup = |index| {
            reads.borrow_mut().push(index);
            Ok(grid_value(&row["grid"], index))
        };
        let coefficient = NativeF64Bits::from_bits(
            u64::from_str_radix(row["coefficient_bits"].as_str().unwrap(), 16).unwrap(),
        );
        let outcome = zone_precheck_flat(
            &graph,
            1,
            4,
            MovementZone::Normal,
            &ZonePrecheckExclusions::default(),
            Some(ZonePrecheckThreat::new(coefficient, &lookup)),
        )
        .unwrap();
        let ZonePrecheckOutcome::Passed(result) = outcome else {
            panic!("{}", row["name"]);
        };
        assert_eq!(
            result.paths,
            expected_paths(&row["flow"][0]),
            "{} paths",
            row["name"]
        );
        assert_eq!(
            result.pops,
            expected_pops(&row["hierarchy"]),
            "{} cost/order",
            row["name"]
        );
        let expected: Vec<i32> = row["reads"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["index"].as_i64().unwrap() as i32)
            .collect();
        assert_eq!(*reads.borrow(), expected, "{} threat reads", row["name"]);
    }
}

#[test]
fn original_full_and_local_hierarchy_producers_retain_seed_threat_index() {
    let packet = threat_packet();
    let writes = packet["producer_writes"].as_array().unwrap();
    assert!(writes.iter().any(|row| row["pc"] == "005822ab"));
    assert!(writes.iter().any(|row| row["pc"] == "00584a78"));
    for row in writes {
        let seed = cell(&row["seed"]);
        let record = ZoneRecord::from_seed(row["zone"].as_u64().unwrap() as u16, 0, 0, seed);
        assert_eq!(
            record.native_threat_index().unwrap() as u32,
            row["index"].as_u64().unwrap() as u32,
            "{} level{} zone{} seed{seed:?}",
            row["pc"],
            row["level"],
            row["zone"]
        );
    }
}

fn cell(value: &Value) -> (i16, i16) {
    (
        value[0].as_i64().unwrap() as i16,
        value[1].as_i64().unwrap() as i16,
    )
}

fn hierarchy(
    graphs: &Value,
    endpoints: Option<((u16, u16), (u16, u16), [u16; 3], [u16; 3])>,
) -> ZoneHierarchy {
    let levels: [ZoneLevelGraph; 3] = graphs
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(level, graph)| {
            let rows = graph["records"]
                .as_array()
                .unwrap_or_else(|| graph.as_array().unwrap());
            let mut ids = graph.get("ids").map_or_else(
                || vec![0; usize::from(WIDTH) * usize::from(WIDTH)],
                |ids| {
                    ids.as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_u64().unwrap() as u16)
                        .collect()
                },
            );
            if let Some((start, goal, start_id, goal_id)) = endpoints {
                ids[usize::from(start.1) * usize::from(WIDTH) + usize::from(start.0)] =
                    start_id[level];
                ids[usize::from(goal.1) * usize::from(WIDTH) + usize::from(goal.0)] =
                    goal_id[level];
            }
            let mut result =
                ZoneLevelGraph::new((rows.len() - 1) as u16).with_cell_zone_ids(ids, WIDTH, WIDTH);
            for (id, record) in rows.iter().enumerate() {
                let mut native_record = ZoneRecord::new(
                    id as u16,
                    record["parent"].as_u64().unwrap() as u16,
                    record["zone_type"].as_u64().unwrap() as u8,
                );
                native_record.native_threat_index =
                    record["threat_index"].as_i64().map(|v| v as i32);
                result.set_record(native_record);
                for edge in record["edges"].as_array().unwrap() {
                    result.push_edge(
                        id as u16,
                        ZoneEdgeRecord::new(
                            edge[0].as_u64().unwrap() as u16,
                            edge[1].as_u64().unwrap() as u8,
                        ),
                    );
                }
            }
            result
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let [fine, middle, coarse] = levels;
    ZoneHierarchy::new(fine, middle, coarse)
}

/// Supplied ordinary actor/flat Cell inputs isolate this dependency. The
/// physical FV readers, Cell geometry and complete mission are covered by the
/// production FV fixture; these tests deliberately supply the native graphs.
fn simulation() -> (Simulation, RuleSet, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=FV\n[FV]\nStrength=200\nSpeed=10\nSpeedType=Wheel\nMovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    )).unwrap();
    let terrain = test_flat_ground_grid(WIDTH);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_terrain(&grid, &terrain, &[], WIDTH, WIDTH);
    let mut sim = Simulation::new();
    sim.session.map_width = WIDTH;
    sim.session.map_height = WIDTH;
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.zone_grid = Some(zones);
    sim.install_resolved_terrain_for_new_map(terrain);
    let id = sim.allocate_stable_id();
    let mut actor =
        crate::sim::game_entity::GameEntity::test_default(id, "FV", "Americans", 87, 48);
    actor.type_ref = sim.interner.intern("FV");
    sim.substrate.entities.insert(actor);
    (sim, rules, id)
}

fn expected_paths(native: &Value) -> [Vec<ZoneId>; 3] {
    native["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|path| {
            path.as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u16)
                .collect()
        })
        .collect::<Vec<_>>()
        .try_into()
        .unwrap()
}

fn expected_pops(events: &Value) -> Vec<(usize, ZoneId, u32, u32)> {
    events
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "heap_pop")
        .map(|event| {
            let bytes = event["cost_bits"].as_str().unwrap();
            let bits = u32::from_le_bytes(std::array::from_fn(|i| {
                u8::from_str_radix(&bytes[i * 2..i * 2 + 2], 16).unwrap()
            }));
            (
                event["level"].as_u64().unwrap() as usize,
                event["zone_id"].as_u64().unwrap() as u16,
                bits,
                event["depth"].as_u64().unwrap() as u32,
            )
        })
        .collect()
}

#[test]
fn native_controls_cover_heap_ties_head_admission_and_binary32_costs() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/zone_cost.json",
    ))
    .unwrap();
    let (mut sim, rules, id) = simulation();
    for row in native["controls"].as_array().unwrap() {
        let start = cell(&row["source"]);
        let goal = cell(&row["target"]);
        let graph = hierarchy(
            &row["graphs"],
            Some((
                (start.0 as u16, start.1 as u16),
                (goal.0 as u16, goal.1 as u16),
                [row["source_id"].as_u64().unwrap() as u16, 1, 1],
                [row["target_id"].as_u64().unwrap() as u16, 1, 1],
            )),
        );
        let precheck = zone_precheck_flat(
            &graph,
            row["source_id"].as_u64().unwrap() as u16,
            row["target_id"].as_u64().unwrap() as u16,
            MovementZone::Normal,
            &ZonePrecheckExclusions::default(),
            None,
        )
        .unwrap();
        let original = row["flow"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["kind"] == "hierarchy_preflight")
            .unwrap();
        if original["returned_al"] == 0 {
            assert_eq!(precheck, ZonePrecheckOutcome::Failed, "{}", row["name"]);
        } else {
            let ZonePrecheckOutcome::Passed(result) = precheck else {
                panic!("{}", row["name"]);
            };
            assert_eq!(result.paths, expected_paths(original), "{}", row["name"]);
            assert_eq!(
                result.pops,
                expected_pops(&row["hierarchy"]),
                "{}",
                row["name"]
            );
        }
        sim.zone_grid.as_mut().unwrap().set_hierarchy(graph);
        let result = sim
            .estimate_zone_cost(
                id,
                start,
                goal,
                row["source_bridge"].as_bool().unwrap(),
                row["target_bridge"].as_bool().unwrap(),
                &rules,
            )
            .unwrap();
        assert_eq!(
            i64::from(result),
            row["returned"].as_i64().unwrap(),
            "{}",
            row["name"]
        );
    }
}

#[test]
fn physical_fv_candidate_and_fallback_costs_match_four_native_bridge_states() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/zone_cost_vectors.json",
    ))
    .unwrap();
    let (mut sim, rules, id) = simulation();
    for row in native["cases"].as_array().unwrap() {
        let start = cell(&row["source"]);
        let goal = cell(&row["target"]);
        let ids = |key: &str| std::array::from_fn(|i| row[key][i].as_u64().unwrap() as u16);
        let start_ids: [u16; 3] = ids("source_ids");
        let goal_ids: [u16; 3] = ids("target_ids");
        let graph = hierarchy(
            &native["graphs"][row["graph"].as_u64().unwrap() as usize],
            Some((
                (start.0 as u16, start.1 as u16),
                (goal.0 as u16, goal.1 as u16),
                start_ids,
                goal_ids,
            )),
        );
        let precheck = zone_precheck_flat(
            &graph,
            start_ids[0],
            goal_ids[0],
            MovementZone::Normal,
            &ZonePrecheckExclusions::default(),
            None,
        )
        .unwrap();
        let preflight = &row["preflight"];
        if preflight["returned_al"] == 0 {
            assert_eq!(precheck, ZonePrecheckOutcome::Failed, "{}", row["name"]);
        } else {
            let ZonePrecheckOutcome::Passed(result) = precheck else {
                panic!("{}", row["name"]);
            };
            assert_eq!(result.paths, expected_paths(preflight), "{}", row["name"]);
            assert_eq!(
                result.pops,
                expected_pops(&row["hierarchy"]),
                "{}",
                row["name"]
            );
        }
        sim.zone_grid.as_mut().unwrap().set_hierarchy(graph);
        let actual = sim
            .estimate_zone_cost(
                id,
                start,
                goal,
                row["source_bridge"].as_bool().unwrap(),
                row["target_bridge"].as_bool().unwrap(),
                &rules,
            )
            .unwrap();
        assert_eq!(
            i64::from(actual),
            row["cost"].as_i64().unwrap(),
            "{}",
            row["name"]
        );
    }
    assert_eq!(native["cases"].as_array().unwrap().len(), 6);
}

#[test]
fn structural_bridge_cost_exits_match_original_six_probe_order() {
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/fv_cell_attack/zone_exits.json",
    ))
    .unwrap();
    for row in native["controls"].as_array().unwrap() {
        let mut terrain = test_flat_ground_grid(WIDTH);
        terrain.test_set_high_bridge_set_starts(Some(10000), None);
        for facts in row["cells"].as_array().unwrap() {
            let xy = cell(&facts["coord"]);
            let entry = terrain.cell_mut(xy.0 as u16, xy.1 as u16).unwrap();
            entry.final_tile_index = facts["tile"].as_i64().unwrap() as i32;
            entry.yr_cell_land_type = facts["land"].as_u64().unwrap() as u8;
            entry.bridge_facts.raw_flags = facts["flags"].as_u64().unwrap() as u32;
        }
        let grid = PathGrid::from_resolved_terrain(&terrain);
        let mut zones = ZoneGrid::build_with_terrain(&grid, &terrain, &[], WIDTH, WIDTH);
        let mut fine = ZoneLevelGraph::new(901).with_cell_zone_ids(
            vec![0; usize::from(WIDTH) * usize::from(WIDTH)],
            WIDTH,
            WIDTH,
        );
        for (i, xy) in row["choices"].as_array().unwrap().iter().enumerate() {
            let xy = cell(xy);
            fine.set_zone_at(
                i32::from(xy.0),
                i32::from(xy.1),
                row["zone_ids"][i].as_u64().unwrap() as u16,
            );
        }
        zones.set_hierarchy(ZoneHierarchy::new(
            fine,
            ZoneLevelGraph::new(0),
            ZoneLevelGraph::new(0),
        ));
        let normalized = &row["bounds"]["normalized"];
        let bounds = crate::map::playfield::PlayfieldBounds {
            base: row["bounds"]["size"][0].as_i64().unwrap() as i32,
            off_fc: normalized[0].as_i64().unwrap() as i32,
            off_100: normalized[1].as_i64().unwrap() as i32,
            off_104: normalized[2].as_i64().unwrap() as i32,
            off_108: normalized[3].as_i64().unwrap() as i32,
        };
        let source = cell(&row["source"]);
        let actual = zones
            .bridge_cell_for_hierarchy_zone(
                &terrain,
                (source.0 as u16, source.1 as u16),
                0,
                900,
                Some(bounds),
            )
            .unwrap();
        let expected = cell(&row["result"]);
        assert_eq!(
            actual,
            (expected.0 as u16, expected.1 as u16),
            "{}",
            row["name"]
        );
    }
}
