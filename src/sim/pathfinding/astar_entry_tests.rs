//! Native AStar height fragments. These compare original instruction results,
//! not a second Rust model; they do not establish full-route equivalence.

use super::*;
use serde_json::Value;
use std::cell::RefCell;

fn scalar_cell(level: u8, flags: u32, walkable_hint: bool) -> PathCell {
    PathCell {
        ground_level: level,
        bridge_structural: flags & 0x100 != 0,
        bridge_walkable: walkable_hint,
        // This derived byte is deliberately not the source of native height.
        bridge_deck_level: level.wrapping_add(4),
        ..DEFAULT_WALKABLE_CELL
    }
}

#[test]
fn original_signed_height_producer_and_blocked_goal_tail() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_signed_height.json",
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 11);
    for case in cases {
        let input = &case["input"];
        let start = scalar_cell(
            input["source_level_raw"].as_u64().unwrap() as u8,
            input["source_flags"].as_u64().unwrap() as u32,
            false,
        );
        let goal = scalar_cell(
            input["goal_level_raw"].as_u64().unwrap() as u8,
            input["goal_flags"].as_u64().unwrap() as u32,
            false,
        );
        let layer = if input["source_on_bridge"].as_bool().unwrap() {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let (initial, goal_height) = initial_search_heights(&start, layer, &goal);
        assert_eq!(
            i64::from(initial),
            case["produced_initial_current_height"].as_i64().unwrap(),
            "{input}"
        );
        assert_eq!(
            i64::from(goal_height),
            case["produced_goal_height"].as_i64().unwrap(),
            "{input}"
        );
        // Two controls supply a later node height after original initial-height
        // production. This distinguishes the tail's current node from its start.
        let current = input["current_height_after_hops_supplied"]
            .as_i64()
            .map_or(initial, |height| height as i16);
        assert_eq!(
            blocked_goal_height_matches(current, goal_height),
            case["branch"] == "blocked_goal_abort",
            "{input}"
        );
    }
}

#[test]
fn original_structural_node_height_and_closed_list_selection() {
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_structural_height.json",
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 22);
    for case in cases {
        let input = &case["input"];
        let parent = scalar_cell(
            input["parent_ground_raw"].as_u64().unwrap() as u8,
            input["parent_flags"].as_u64().unwrap() as u32,
            input["parent_rust_bridge_walkable"]
                .as_bool()
                .unwrap_or(false),
        );
        let candidate = scalar_cell(
            input["candidate_ground_raw"].as_u64().unwrap() as u8,
            input["candidate_flags"].as_u64().unwrap() as u32,
            input["candidate_rust_bridge_walkable"]
                .as_bool()
                .unwrap_or(false),
        );
        let current = input["current_height"].as_i64().unwrap() as i16;
        let parent = (!input["initial_node"].as_bool().unwrap_or(false)).then_some(&parent);
        assert_eq!(
            i64::from(compute_node_height(current, parent, &candidate)),
            case["node_height"].as_i64().unwrap(),
            "{input}"
        );
        assert_eq!(
            is_at_bridge_level(current, &candidate),
            case["selected_list"] == "deck",
            "{input}"
        );
    }
}

struct SuppliedEntry {
    answer: Result<u8, String>,
    queries: RefCell<Vec<SearchEntryQuery>>,
}

impl SearchFootEntry for SuppliedEntry {
    fn classify(&self, query: SearchEntryQuery) -> Result<u8, String> {
        self.queries.borrow_mut().push(query);
        self.answer.clone()
    }
}

#[test]
fn canonical_entry_reaches_blocked_goal_before_static_grid_refusal() {
    // Production wiring check: the supplied class comes from concrete native
    // Infantry-entry controls. This two-cell Rust route is not a native route
    // golden; it exposes a second static-grid verdict overriding the live one.
    let native: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_capture_neighbor.json",
    ))
    .unwrap();
    let source = scalar_cell(10, 0, false);
    let goal = PathCell {
        ground_walkable: false,
        ..source
    };
    let grid = PathGrid::from_cells(vec![source, goal], 2, 1);
    for name in ["synthetic_flat_capture", "synthetic_iron_curtain_control"] {
        let case = native["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["input"]["name"] == name)
            .unwrap();
        let entry = SuppliedEntry {
            answer: Ok(case["can_enter_class"].as_u64().unwrap() as u8),
            queries: RefCell::new(Vec::new()),
        };
        let result = astar_search(
            &grid,
            (0, 0),
            MovementLayer::Ground,
            (1, 0),
            &AStarOptions {
                foot_entry: Some(&entry),
                is_infantry: true,
                ..Default::default()
            },
        );
        assert_eq!(result.is_ok(), case["outcome"] == "admit_node_creation");
        let queries = entry.queries.borrow();
        assert_eq!(queries.len(), 1, "{name}");
        let query = queries[0];
        assert_eq!(
            (query.from, query.candidate),
            (Some((0, 0)), SearchEntryCandidate::CopiedCoord((1, 0)))
        );
        assert_eq!((query.direction, query.path_height), (2, 10));
    }
}

#[test]
fn unavailable_live_entry_is_not_an_ordinary_blocked_route() {
    let entry = SuppliedEntry {
        answer: Err("unavailable canonical Cell input".into()),
        queries: RefCell::new(Vec::new()),
    };
    assert!(matches!(
        astar_search(
            &PathGrid::new(2, 1),
            (0, 0),
            MovementLayer::Ground,
            (1, 0),
            &AStarOptions { foot_entry: Some(&entry), ..Default::default() },
        ),
        Err(PathSearchFailure::CellEntryUnavailable(cause))
            if cause == "unavailable canonical Cell input"
    ));
    assert_eq!(entry.queries.borrow().len(), 1);
}

#[test]
fn native_null_candidates_never_reach_entry_or_create_routes() {
    use crate::map::tube_facts::TubeFact;

    struct LiveLookupEntry<'a> {
        terrain: &'a ResolvedTerrainGrid,
        queries: RefCell<Vec<SearchEntryQuery>>,
    }
    impl SearchFootEntry for LiveLookupEntry<'_> {
        fn classify(&self, query: SearchEntryQuery) -> Result<u8, String> {
            self.queries.borrow_mut().push(query);
            // The production adapter performs these same canonical lookups.
            // Class0 is supplied here: a NULL slot must never call it at all.
            if let Some(previous) = query.from {
                self.terrain
                    .native_cell_identity((previous.0 as i16, previous.1 as i16));
            }
            let SearchEntryCandidate::CopiedCoord(candidate) = query.candidate else {
                panic!("A* must supply its expanded coordinate domain");
            };
            self.terrain
                .native_cell_identity((candidate.0 as i16, candidate.1 as i16));
            Ok(0)
        }
    }
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_structural_height.json",
    ))
    .unwrap();
    let controls = corpus["allocation_guards"].as_array().unwrap();
    assert_eq!(controls.len(), 4);
    for control in controls {
        let tube = control["direction"] == 8;
        let allocated = control["allocated"].as_bool().unwrap();
        let width = if tube { 4 } else { 3 };
        let candidate = if tube { (2, 0) } else { (1, 0) };
        let goal = (width - 1, 0);
        let cells = (0..width)
            .map(|x| {
                let mut c = crate::sim::world::common_raw_test_terrain_cell(x, 0, 10, false);
                if tube && x == 0 {
                    c.tube_index = Some(TubeId(0));
                }
                c
            })
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells_with_tubes(
            width,
            1,
            cells,
            if tube {
                vec![TubeFact::explicit((0, 0), candidate, 2, vec![2, 2])]
            } else {
                Vec::new()
            },
        );
        let mut real = vec![(0, 0), goal];
        if allocated {
            real.push(candidate);
        }
        terrain.test_set_native_allocated_cells(&real);
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(71, 77);
        let before = dummy.snapshot();
        let grid = PathGrid::from_resolved_terrain(&terrain);
        let entry = LiveLookupEntry {
            terrain: &terrain,
            queries: RefCell::new(Vec::new()),
        };
        let result = astar_search(
            &grid,
            (0, 0),
            MovementLayer::Ground,
            goal,
            &AStarOptions {
                resolved_terrain: Some(&terrain),
                foot_entry: Some(&entry),
                is_infantry: true,
                ..Default::default()
            },
        );
        assert_eq!(
            result.is_err(),
            control["skipped"].as_bool().unwrap(),
            "{control}"
        );
        assert_eq!(
            dummy.snapshot(),
            before,
            "{control}: search must not stamp Dummy"
        );
        if allocated {
            let route: Vec<_> = result
                .unwrap()
                .iter()
                .map(|step| (step.rx, step.ry))
                .collect();
            assert_eq!(route, vec![(0, 0), candidate, goal], "{control}");
        } else {
            assert!(
                entry.queries.borrow().is_empty(),
                "{control}: NULL candidate reached +1AC"
            );
        }
    }
}

#[test]
fn original_reconstruction_retains_each_parent_descriptor_height() {
    // Original caller 42A3FE..42A423 executes 42AA90 against supplied native
    // parent descriptors. These are reconstruction controls, not native A*
    // route-selection goldens. In particular, signed_parent_heights supplies
    // values independent of physical Cell levels; reconstruction must not
    // derive another height from terrain or the object-list layer.
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_path_finishing.json",
    ))
    .unwrap();
    let cases = corpus["cases"].as_array().unwrap();
    assert!(cases.len() >= 33);
    // The frozen original 33 cases have unique, allocated square-grid Cells.
    // Later Tube/wrap controls deliberately supply mismatched descriptors and
    // fallback identities, outside this A* parent-index representation.
    for case in cases.iter().take(33) {
        let supplied = case["supplied_nodes"].as_array().unwrap();
        let width = 64;
        let mut ground = vec![PathDescriptor::UNVISITED; width * width];
        let bridge = vec![PathDescriptor::UNVISITED; width * width];
        let mut previous = usize::MAX;
        for node in supplied {
            let x = node["cell"][0].as_u64().unwrap() as usize;
            let y = node["cell"][1].as_u64().unwrap() as usize;
            let index = y * width + x;
            assert_eq!(
                ground[index].parent,
                usize::MAX,
                "supplied chain repeats a cell"
            );
            ground[index] = PathDescriptor {
                parent: if previous == usize::MAX {
                    previous
                } else {
                    encode_from(previous, false)
                },
                height: node["height"].as_i64().unwrap() as i16,
            };
            previous = index;
        }
        let first = &supplied[0];
        let start_index = first["cell"][1].as_u64().unwrap() as usize * width
            + first["cell"][0].as_u64().unwrap() as usize;
        let result =
            reconstruct_path_dual(&ground, &bridge, start_index, false, previous, false, width);
        assert_eq!(
            result
                .iter()
                .map(|step| (step.rx, step.ry))
                .collect::<Vec<_>>(),
            supplied
                .iter()
                .map(|node| (
                    node["cell"][0].as_u64().unwrap() as u16,
                    node["cell"][1].as_u64().unwrap() as u16
                ))
                .collect::<Vec<_>>(),
            "{}",
            case["input"]["name"]
        );
        let native = &case["snapshots"][0];
        assert_eq!(native["name"], "reconstructed");
        assert_eq!(
            result[..result.len() - 1]
                .iter()
                .map(|step| i64::from(step.path_height()))
                .collect::<Vec<_>>(),
            native["retained_heights"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap())
                .collect::<Vec<_>>(),
            "{}",
            case["input"]["name"]
        );
        assert_eq!(
            i64::from(result.last().unwrap().path_height()),
            supplied.last().unwrap()["height"].as_i64().unwrap()
        );
    }
}

#[test]
fn accepted_descriptor_height_survives_selected_list_transition() {
    // Production owner regression using the original scalar-height producer
    // corpus: selected_list can remain deck even when the accepted new node's
    // height becomes ground. Reconstruction must keep both facts separately.
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/astar_structural_height.json",
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let input = &case["input"];
        if input["initial_node"].as_bool().unwrap_or(false) {
            continue;
        }
        let parent = scalar_cell(
            input["parent_ground_raw"].as_u64().unwrap() as u8,
            input["parent_flags"].as_u64().unwrap() as u32,
            true,
        );
        let candidate = scalar_cell(
            input["candidate_ground_raw"].as_u64().unwrap() as u8,
            input["candidate_flags"].as_u64().unwrap() as u32,
            true,
        );
        let previous_height = input["current_height"].as_i64().unwrap() as i16;
        let next_height = compute_node_height(previous_height, Some(&parent), &candidate);
        let next_bridge = is_at_bridge_level(previous_height, &candidate);
        let mut ground = vec![PathDescriptor::UNVISITED; 2];
        let mut bridge = vec![PathDescriptor::UNVISITED; 2];
        ground[0] = PathDescriptor {
            parent: usize::MAX,
            height: previous_height,
        };
        let next = PathDescriptor {
            parent: encode_from(0, false),
            height: next_height,
        };
        if next_bridge {
            bridge[1] = next;
        } else {
            ground[1] = next;
        }
        let result = reconstruct_path_dual(&ground, &bridge, 0, false, 1, next_bridge, 2);
        assert_eq!(
            i64::from(result[1].path_height()),
            case["node_height"].as_i64().unwrap(),
            "{input}"
        );
        assert_eq!(
            result[1].layer == MovementLayer::Bridge,
            case["selected_list"] == "deck",
            "{input}"
        );
    }
}
