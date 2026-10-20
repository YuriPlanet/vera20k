//! Tests for zone-aware pathfinding wrappers.

use super::super::zone_hierarchy::{ZoneEdgeRecord, ZoneHierarchy, ZoneLevelGraph, ZoneRecord};
use super::super::zone_map::{ZoneGrid, ZoneId};
use super::*;
use crate::map::bridge_facts::{
    BRIDGE_FLAG_DIRECTION_ZERO, BRIDGE_FLAG_STRUCTURAL, BRIDGE_FLAG_TRANSITION,
};
use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::ruleset::RuleSet;
use crate::rules::terrain_rules::{LandType, SpeedCostProfile};
use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind};
use crate::sim::cell_rect::PlayfieldBounds;
use crate::sim::components::NavTargetRef;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

fn gsi_04_12_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    let speed_costs = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    let cells = (0..height)
        .flat_map(|ry| {
            (0..width).map(move |rx| ResolvedTerrainCell {
                tileset_index: None,
                land_type: LandType::Clear.as_index(),
                speed_costs,
                zone_type: zone_class::GROUND,
                base_land_type: LandType::Clear.as_index(),
                base_speed_costs: speed_costs,
                ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
            })
        })
        .collect();
    let mut terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
    terrain.test_set_high_bridge_set_starts(Some(100), None);
    terrain
}

fn rectangular_spawn_bounds(span: i32) -> PlayfieldBounds {
    PlayfieldBounds {
        base: 0,
        off_fc: -span,
        off_100: -span,
        off_104: span * 2,
        off_108: span * 2,
    }
}

/// Whether `route` crosses one of the high-bridge deck cells (2..=4, 2) of
/// `caller_count_bridge_detour`. The fixture pins that the ordinary shortcut
/// stays on row 0 and the marked detour crosses these cells on the deck.
fn crosses_detour_bridge_deck(route: &[(u16, u16)]) -> bool {
    route.iter().any(|&(x, y)| y == 2 && (2..=4).contains(&x))
}

fn linear_level0_hierarchy(zones: Vec<ZoneId>, edges: &[(ZoneId, ZoneId)]) -> ZoneHierarchy {
    let width = zones.len() as u16;
    level0_hierarchy(zones, width, 1, edges)
}

fn level0_hierarchy(
    zones: Vec<ZoneId>,
    width: u16,
    height: u16,
    edges: &[(ZoneId, ZoneId)],
) -> ZoneHierarchy {
    debug_assert_eq!(zones.len(), width as usize * height as usize);
    let zone_count = zones.iter().copied().max().unwrap_or(0);
    let mut level2 = ZoneLevelGraph::new(1);
    level2.set_record(ZoneRecord::new(1, 0, 0));

    let mut level1 = ZoneLevelGraph::new(1);
    level1.set_record(ZoneRecord::new(1, 1, 0));

    let mut level0 = ZoneLevelGraph::new(zone_count).with_cell_zone_ids(zones, width, height);
    for zone in 1..=zone_count {
        level0.set_record(ZoneRecord::new(zone, 1, 0));
    }
    for &(a, b) in edges {
        level0.push_edge(a, ZoneEdgeRecord::new(b, 0));
        level0.push_edge(b, ZoneEdgeRecord::new(a, 0));
    }

    ZoneHierarchy::new(level0, level1, level2)
}

#[test]
fn wrapper_rejection_precedes_the_marker_transaction() {
    struct MarkerBoundary;
    impl super::super::SearchFootEntry for MarkerBoundary {
        fn classify(&self, _: super::super::SearchEntryQuery) -> Result<u8, String> {
            panic!("the marker boundary must precede cell expansion")
        }
        fn finishing_threat_coefficient(
            &self,
        ) -> Result<crate::util::native_x87::NativeF64Bits, String> {
            Ok(crate::util::native_x87::NativeF64Bits::POSITIVE_ZERO)
        }
        fn prepare_search_markers(
            &self,
        ) -> Result<Option<(u8, std::cell::Ref<'_, SearchMarkerOverlay>)>, String> {
            Err("reached429C1A".into())
        }
    }
    //42CB22 rejects different raw base labels before429C1A. A connected
    //coarse graph must not prepare markers (which can query the shared Dummy).
    //The admitted control proves this receiver is wired to the live boundary.
    let grid = PathGrid::new(3, 1);
    let mut zones = ZoneGrid::following_path_grid(&grid);
    zones.set_hierarchy(linear_level0_hierarchy(vec![1, 2, 3], &[(1, 2), (2, 3)]));
    let counts = BlockerNeighborCounts::new(3, 1);
    for equal in [false, true] {
        let entry = NativePathEntry {
            raw_equal: Some(equal),
            goal_bridge: false,
            hierarchy_start: (0, 0),
            hierarchy_goal: (2, 0),
            endpoints_in_playfield: true,
        };
        let result = find_layered_path_after_entry(
            &grid,
            (0, 0),
            MovementLayer::Ground,
            (2, 0),
            &entry,
            Some(&zones),
            MovementZone::Normal,
            Some(MovementZone::Normal),
            None,
            None,
            MoverSearchFacts {
                foot_entry: &MarkerBoundary,
                urgency: 2,
            },
            Some(&counts),
            None,
        );
        assert_eq!(
            result.unwrap_err(),
            if equal {
                PathSearchFailure::CellEntryUnavailable("reached429C1A".into())
            } else {
                PathSearchFailure::NativeEntryRejected
            }
        );
    }
}

// Shared caller witness: the ordinary ground shortcut differs from the marked
// bridge detour. Its unmarked ground approach needs Cell+122; bridge-deck
// height exemption alone cannot admit that approach.
fn caller_count_bridge_detour(
    mz: MovementZone,
    reverse: bool,
) -> (PathGrid, ZoneGrid, ResolvedTerrainGrid) {
    let mut path = PathGrid::new(6, 4);
    let mut terrain = gsi_04_12_terrain(6, 4);
    for y in 0..4 {
        for x in 0..6 {
            let open = (y == 0 && x >= 1) || (y == 1 && (x == 1 || x == 5)) || (y == 2 && x >= 1);
            path.set_blocked(x, y, !open);
            if open {
                path.set_cell_for_test(x, y, 4, false, false);
                terrain.cell_mut_for_test(x, y).unwrap().level = 4;
            }
        }
    }
    for x in 2..=4 {
        path.set_cell_for_test(x, 2, 0, true, true);
        let cell = terrain.cell_mut_for_test(x, 2).unwrap();
        cell.level = 0;
        // Keep the native Cell flags consistent with PathCell.transition.
        // Foot4D9C60 requires raw200 for both the height4 -> deck entry and
        // subsequent deck candidates. Original unit_entry_traversal rows
        // 58/59 pin clear0 with raw300 versus refusal7 with raw100 alone.
        cell.bridge_facts.raw_flags =
            BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DIRECTION_ZERO | BRIDGE_FLAG_TRANSITION;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_transition = true;
        cell.bridge_deck_level = 4;
    }
    // Foot4D3810 -> Map56D100 needs the native base rows before the route
    // search. Build those from this fixture's real levels and Map Size=(6,4),
    // including the bridge's ground endpoints. The deliberately restricted
    // hierarchy below remains the separate Cell+122 caller-count witness.
    let mut zones = ZoneGrid::build_with_native_bridge_geometry(
        &path,
        &terrain,
        &[BridgeEndpointRecord {
            endpoint_a: (1, 2),
            endpoint_b: (5, 2),
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }],
        6,
        4,
        Some((6, 4)),
    );
    let mut labels = vec![3; 24];
    for (x, y) in [(1, 0), (5, 0), (5, 1), (5, 2), (1, 2)] {
        labels[y * 6 + x] = 1;
    }
    zones.set_hierarchy(level0_hierarchy(labels, 6, 4, &[]));
    let (start, goal) = if reverse {
        ((5, 0), (1, 0))
    } else {
        ((1, 0), (5, 0))
    };
    let native_start = zones
        .get_zone_id_native(&terrain, start, mz, false)
        .expect("caller fixture retains native topology");
    assert!(
        (2..u32::from(u16::MAX)).contains(&native_start),
        "the native precheck must use a valid ground row, not equal invalid labels"
    );
    assert_eq!(
        zones.get_zone_id_native(&terrain, goal, mz, false),
        Some(native_start),
        "the top-row shortcut connects the native source and destination"
    );
    assert_eq!(
        zones.get_zone_id_native(&terrain, (3, 2), mz, true),
        Some(native_start),
        "the bridge deck resolves through the same reachable ground component"
    );
    let route = |counts: Option<&BlockerNeighborCounts>| {
        find_layered_path_zoned_marker(
            &path,
            start,
            MovementLayer::Ground,
            goal,
            Some(&zones),
            mz,
            Some(mz),
            Some(&terrain),
            None,
            counts,
            true,
            None,
        )
    };
    let ordinary = route(None).expect("no-count route must take the available shortcut");
    assert!(
        ordinary
            .iter()
            .all(|step| step.ry == 0 && step.layer == MovementLayer::Ground)
    );
    // Zero counts keep the corridor search off the unmarked approach: the
    // precheck marks only zone 1, so the search fails rather than take the
    // shortcut or the detour.
    let zero_counts = BlockerNeighborCounts::new(6, 4);
    let hierarchy = zones.hierarchy_for(mz).expect("the fixture hierarchy");
    let level0 = hierarchy.level(0).expect("its level 0");
    let ZonePrecheckOutcome::Passed(precheck) = zone_precheck_flat(
        hierarchy,
        level0.zone_at(start.0, start.1),
        level0.zone_at(goal.0, goal.1),
        mz,
        &ZonePrecheckExclusions::default(),
        None,
    )
    .expect("the precheck reads the fixture hierarchy") else {
        panic!("both endpoints are zone 1");
    };
    let corridor = find_layered_path_marker(
        &path,
        start,
        MovementLayer::Ground,
        goal,
        Some(HierarchyGate {
            level0_zones: level0,
            marked_level0: &precheck.marked[0],
            blocker_neighbor_counts: &zero_counts,
            level0_route: &precheck.paths[0],
            progress: &Cell::new(RouteProgress {
                index: 0,
                cell: start,
            }),
        }),
        Some(&terrain),
        None,
        MoverSearchFacts {
            urgency: 0,
            foot_entry: &super::super::WalkableEntry::new(&path),
        },
    );
    assert_eq!(
        corridor,
        Err(PathSearchFailure::CellSearchExhausted),
        "zero counts cannot enter the unmarked ground approach"
    );
    // The whole search retries. This fixture's levels 1 and 2 map no cells,
    // so every cell reads zone 0 there: the retry's flood finds that zone
    // split on a one-zone route, which clears the hierarchy (`0x0042CF9A`),
    // and the plain search takes the shortcut.
    assert_eq!(route(Some(&zero_counts)), Some(ordinary));
    let mut live_counts = BlockerNeighborCounts::new(6, 4);
    live_counts.add_single_cell_neighbor_source(0, 2);
    let marked = route(Some(&live_counts)).expect("neighbor escape admits marked bridge detour");
    assert!(
        marked
            .iter()
            .any(|step| step.layer == MovementLayer::Bridge)
    );
    // Foot+5E0 words carry no layer: production callers observe the bridge
    // detour as a route through the deck cells, which it crosses on the deck.
    let marked_cells: Vec<_> = marked.iter().map(|step| (step.rx, step.ry)).collect();
    assert!(crosses_detour_bridge_deck(&marked_cells));
    assert!(
        marked
            .iter()
            .filter(|step| crosses_detour_bridge_deck(&[(step.rx, step.ry)]))
            .all(|step| step.layer == MovementLayer::Bridge)
    );
    (path, zones, terrain)
}

#[test]
fn gsi_04_12_interaction_order_entry_threads_exact_blocker_counts() {
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n0=ENGINEER\n\n\
         [VehicleTypes]\n0=BLOCK\n\n\
         [BuildingTypes]\n0=TARGET\n\n\
         [ENGINEER]\nStrength=75\nSpeed=4\nLocomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Infantry\nEngineer=yes\n\n\
         [BLOCK]\nStrength=300\nSpeed=4\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n\n\
         [TARGET]\nStrength=500\nCapturable=yes\n",
    ), &IniFile::from_str("[TARGET]\nFoundation=1x1\n"))
    .expect("interaction-order rules should parse");

    let (path_grid, zone_grid, terrain) = caller_count_bridge_detour(MovementZone::Infantry, false);

    let mut sim = Simulation::new();
    // Explicit fixture Map Size=(6,4), separate from the generous LocalSize
    // bounds. Foot's production precheck consumes both header dimensions.
    sim.playfield_bounds = Some(PlayfieldBounds {
        base: 6,
        ..rectangular_spawn_bounds(6)
    });
    sim.playfield_size_height = Some(4);
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.zone_grid = Some(zone_grid);
    let engineer_id = sim
        .spawn_object("ENGINEER", "Americans", 1, 0, 0, &rules)
        .expect("engineer should spawn");
    let target_id = sim
        .spawn_object("TARGET", "Russians", 5, 0, 0, &rules)
        .expect("capture target should spawn");
    sim.spawn_object("BLOCK", "Russians", 0, 2, 0, &rules)
        .expect("dynamic blocker should spawn");

    sim.install_fixture_path_grid(Some(&path_grid));
    assert!(sim.apply_command(
        "Americans",
        &crate::sim::command::Command::CaptureBuilding {
            engineer_id,
            target_building_id: target_id,
        },
        Some(&rules),
    ));

    let engineer = sim.substrate.entities.get(engineer_id).unwrap();
    assert_eq!(
        engineer.navigation.nav_com(),
        Some(NavTargetRef::object(target_id))
    );
    assert!(
        crate::sim::movement::motion_query::is_moving(&engineer) == Some(true),
        "accepted Capture request"
    );
    assert_eq!(
        engineer.locomotor.as_ref().unwrap().walk_destination_cell(),
        Some((5, 0))
    );
    assert!(
        engineer
            .navigation
            .path_replay()
            .remaining_directions()
            .is_empty(),
        "Walk searches during Process, after order admission"
    );

    assert_eq!(
        engineer.mission.queued().known(),
        Some(crate::sim::mission::MissionType::Capture)
    );

    // The direct Process seam omits Object/Techno AI. Run the production
    // Ready -> Commence checkpoint first: Infantry51C300 reads CURRENT
    // mission when admitting the Building NavCom, not its queued Capture.
    sim.mission_host_promote(engineer_id, sim.session.binary_frame(), &rules);
    assert_eq!(
        sim.substrate
            .entities
            .get(engineer_id)
            .unwrap()
            .mission
            .current()
            .known(),
        Some(crate::sim::mission::MissionType::Capture)
    );
    // Publish the same navigation used by order admission for shared world
    // receivers, which take their grid from Simulation rather than the
    // locomotor test seam's optional fallback.
    sim.path_grid = Some(std::sync::Arc::new(path_grid));
    let target_cell = sim
        .resolved_terrain
        .as_ref()
        .unwrap()
        .native_cell_identity((5, 0));
    assert_eq!(
        sim.infantry_can_enter(
            engineer_id,
            target_cell,
            crate::sim::movement::infantry_entry::InfantryEntryArgs::REPAIR,
            &rules,
            None,
        )
        .expect("Capture target admission uses the live Building NavCom"),
        crate::sim::movement::infantry_entry::InfantryEntryClass::Clear
    );

    let destination = sim
        .substrate
        .entities
        .get(engineer_id)
        .unwrap()
        .locomotor
        .as_ref()
        .unwrap()
        .walk_destination()
        .unwrap();
    assert!(
        sim.foot_path_zone_precheck(engineer_id, destination, &rules)
            .expect("native source and destination row query"),
        "the live Foot precheck admits the supplied map/zone inputs"
    );
    // REPAIR above deliberately bypasses height/direction checks. Exercise
    // AStar's explicit previous-Cell inputs as well: the marked bridge detour
    // must be physically traversable through the same live +1AC owner.
    for (previous, candidate, direction) in [
        ((1, 0), (1, 1), 4),
        ((1, 2), (2, 2), 2),
        ((2, 2), (3, 2), 2),
        ((3, 2), (4, 2), 2),
        ((4, 2), (5, 2), 2),
        ((5, 1), (5, 0), 0),
    ] {
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        assert_eq!(
            sim.foot_can_enter(
                engineer_id,
                terrain.native_cell_identity(candidate),
                crate::sim::movement::infantry_entry::InfantryEntryArgs {
                    direction,
                    height: 4,
                    previous_cell: Some(terrain.native_cell_identity(previous)),
                },
                &rules,
                None,
            )
            .expect("live Capture corridor admission"),
            0,
            "physical corridor {previous:?} -> {candidate:?}"
        );
    }

    // Execute the real no-head Process with live blockers and canonical grids.
    sim.process_ground_locomotor_for_test(engineer_id, Some(&rules), None, None)
        .expect("Capture Process retains the fixture's native map inputs");

    let engineer = sim.substrate.entities.get(engineer_id).unwrap();
    assert_eq!(
        engineer.navigation.nav_com(),
        Some(NavTargetRef::object(target_id))
    );
    // Native Infantry 51C300/51C37D/51C71B admits the matching Building
    // NavCom. Foot 4D3A92..4D3E0A passes its original Cell to the core;
    // tools/spatial_oracle/capture_core_goal.py covers that admission/goal
    // boundary. This Rust route check does not claim full native AStar parity.
    // The installed Foot+5E0 route runs from the start Cell to the building
    // Cell; its words carry no layer.
    let route = engineer.navigation.path_replay();
    assert_eq!(route.reference_cell(), Some((1, 0)));
    assert_eq!(
        route
            .remaining_cells_from((engineer.position.rx() as i16, engineer.position.ry() as i16))
            .last()
            .copied(),
        Some((5, 0)),
        "Capture must retain the admitted building Cell as its path goal"
    );
}

#[test]
fn tube_hierarchy_gate_uses_raw_invalid_labels_and_goal_bridge_flag() {
    let mut grid = PathGrid::new(2, 2);
    grid.set_cell_for_test(0, 0, 4, false, false);
    grid.set_cell_for_test(1, 0, 0, true, true);
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/tube_hierarchy.json",
    ))
    .unwrap();
    let expected_admission = |a, b| {
        corpus["path_gate"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["a"] == a && r["b"] == b && r["allow"] == 1)
            .unwrap()["action"]
            == "precheck"
    };
    let mut terrain = gsi_04_12_terrain(2, 2);
    let mut zones = ZoneGrid::build_with_native_bridge_geometry(
        &grid,
        &terrain,
        &[BridgeEndpointRecord {
            endpoint_a: (0, 0),
            endpoint_b: (1, 0),
            active: true,
            bridge_kind: BridgeRecordKind::High,
        }],
        2,
        2,
        Some((1, 1)),
    );
    let row = MovementZone::Normal.matrix_row().unwrap();
    {
        let base = zones.base_topology_mut();
        base.zone_ids = vec![2, 3, 2, 3];
        base.raw_zone_ids_by_row[row] = vec![0, 0, 1, u16::MAX];
    }
    {
        let map = zones.map_mut(MovementZone::Normal).unwrap();
        for index in 0..4 {
            map.set_ground_zone_at_index(index, 0);
        }
    }
    zones.set_hierarchy(level0_hierarchy(vec![1; 4], 2, 2, &[]));
    let counts = BlockerNeighborCounts::new(2, 2);
    let route = |zones: &ZoneGrid, terrain: &ResolvedTerrainGrid| {
        find_layered_path_zoned_marker(
            &grid,
            (0, 0),
            MovementLayer::Ground,
            (1, 0),
            Some(zones),
            MovementZone::Normal,
            Some(MovementZone::Normal),
            Some(terrain),
            None,
            Some(&counts),
            true,
            None,
        )
    };
    assert_eq!(
        zones.get_zone_id_native(&terrain, (0, 0), MovementZone::Normal, false),
        Some(1)
    );
    assert_eq!(
        zones.get_zone_id_native(&terrain, (1, 0), MovementZone::Normal, false),
        Some(u32::from(u16::MAX))
    );
    assert_eq!(
        zones
            .map_for(MovementZone::Normal)
            .unwrap()
            .zone_at(0, 0, MovementLayer::Ground),
        0
    );
    assert_eq!(
        zones
            .map_for(MovementZone::Normal)
            .unwrap()
            .zone_at(1, 0, MovementLayer::Ground),
        0
    );
    assert_eq!(
        route(&zones, &terrain).is_some(),
        expected_admission(1, 65535),
        "distinct native invalid labels must reject before a connected graph"
    );
    {
        let base = zones.base_topology_mut();
        base.raw_zone_ids_by_row[row][2] = 2;
        base.raw_zone_ids_by_row[row][3] = 3;
    }
    zones
        .map_mut(MovementZone::Normal)
        .unwrap()
        .set_bridge_redirect(Some(vec![None, Some((0, 0)), None, None]));
    zones.set_hierarchy(level0_hierarchy(vec![1; 4], 2, 2, &[]));
    terrain.cell_mut_for_test(0, 0).unwrap().level = 4;
    {
        let cell = terrain.cell_mut_for_test(1, 0).unwrap();
        cell.bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_transition = true;
        cell.bridge_deck_level = 4;
    }
    assert_eq!(
        zones.get_zone_id_native(&terrain, (1, 0), MovementZone::Normal, true),
        Some(2)
    );
    assert_eq!(
        route(&zones, &terrain).is_some(),
        expected_admission(2, 2),
        "goal Flags0x100 must select the redirected raw zone"
    );
    terrain
        .cell_mut_for_test(1, 0)
        .unwrap()
        .bridge_facts
        .raw_flags = 0;
    assert_eq!(
        route(&zones, &terrain).is_some(),
        expected_admission(2, 3),
        "ground goal reads its different underlying raw label"
    );
}

#[test]
fn tube_hierarchy_dword_zone_query_matches_original_executable() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/tube_hierarchy.json",
    ))
    .unwrap();
    for case in corpus["get_zone"].as_array().unwrap() {
        let coord =
            |v: &serde_json::Value| (v[0].as_i64().unwrap() as u16, v[1].as_i64().unwrap() as u16);
        let query = coord(&case["query"]);
        let linear = i32::from(query.1 as i16) * 512 + i32::from(query.0 as i16);
        let real = case["real"].as_bool().unwrap();
        let canonical = ((linear.rem_euclid(512)) as u16, (linear / 512) as u16);
        let extras = case["extra"].as_array().cloned().unwrap_or_default();
        let mut allocated = if real { vec![canonical] } else { vec![] };
        allocated.extend(extras.iter().map(|entry| coord(&entry[0])));
        let width = allocated.iter().map(|c| c.0).max().unwrap_or(0).max(15) + 1;
        let height = allocated.iter().map(|c| c.1).max().unwrap_or(0).max(15) + 1;
        let mut terrain = gsi_04_12_terrain(width, height);
        terrain.test_set_native_allocated_cells(&allocated);
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        for entry in &extras {
            let c = coord(&entry[0]);
            let cell = terrain.cell_mut_for_test(c.0, c.1).unwrap();
            cell.final_tile_index = entry[1].as_i64().unwrap() as i32;
            cell.land_type = entry[2].as_u64().unwrap() as u8;
            cell.bridge_facts.raw_flags = entry[3].as_u64().unwrap() as u32;
        }
        let flags = case["flags"].as_u64().unwrap() as u32;
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(1234, -2345);
        if real {
            terrain
                .cell_mut_for_test(canonical.0, canonical.1)
                .unwrap()
                .bridge_facts
                .raw_flags = flags;
        } else if flags & 0x100 != 0 {
            dummy.apply_bridge_flag_slot(crate::map::bridge_facts::BridgeStampSlot::Anchor, true);
        }
        let records = case["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| BridgeEndpointRecord {
                endpoint_a: coord(&r[0]),
                endpoint_b: coord(&r[1]),
                active: r[2].as_bool().unwrap(),
                bridge_kind: if r[3] == 0 {
                    BridgeRecordKind::High
                } else {
                    BridgeRecordKind::Low
                },
            })
            .collect::<Vec<_>>();
        let mut zones = ZoneGrid::build_with_native_bridge_geometry(
            &PathGrid::new(width, height),
            &terrain,
            &records,
            width,
            height,
            Some((8, 8)),
        );
        {
            let base = zones.base_topology_mut();
            base.zone_ids.fill(1);
            for record in &records {
                if !record.active {
                    let index = (i32::from(record.endpoint_b.1 as i16) * 17
                        + i32::from(record.endpoint_b.0 as i16))
                    .clamp(0, 288) as usize;
                    let (x, y) = (index % 17, index / 17);
                    assert!(
                        x < 16 && y < 16,
                        "fixture endpoint must project into real node storage"
                    );
                    base.zone_ids[y * usize::from(width) + x] = 2;
                }
            }
            base.raw_zone_ids_by_row[MovementZone::Normal.matrix_row().unwrap()] =
                vec![0, case["raw"].as_u64().unwrap() as u16, 3];
        }
        dummy.stamp_coord(1234, -2345);
        assert_eq!(
            zones.get_zone_id_native(&terrain, query, MovementZone::Normal, case["check"] == 1),
            Some(case["result"].as_u64().unwrap() as u32),
            "{}",
            case["name"]
        );
        assert_eq!(
            dummy.snapshot().coord,
            (
                case["dummy_coord"][0].as_i64().unwrap() as i32,
                case["dummy_coord"][1].as_i64().unwrap() as i32
            ),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn tube_hierarchy_native_entry_prefix_matches_original_executable() {
    let cases: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/path_entry.json",
    ))
    .unwrap();
    assert_eq!(cases.as_array().unwrap().len(), 19);
    for case in cases.as_array().unwrap() {
        let coord =
            |v: &serde_json::Value| (v[0].as_i64().unwrap() as u16, v[1].as_i64().unwrap() as u16);
        let rows = case["cells"].as_array().unwrap();
        let allocated: Vec<_> = rows.iter().map(|r| coord(&r[0])).collect();
        let width = allocated.iter().map(|c| c.0).max().unwrap().max(15) + 1;
        let height = allocated.iter().map(|c| c.1).max().unwrap().max(15) + 1;
        let grid = PathGrid::new(width, height);
        let mut terrain = gsi_04_12_terrain(width, height);
        terrain.test_set_native_allocated_cells(&allocated);
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        for row in rows {
            let c = coord(&row[0]);
            let cell = terrain.cell_mut_for_test(c.0, c.1).unwrap();
            cell.bridge_facts.raw_flags = row[1].as_u64().unwrap() as u32;
            cell.final_tile_index = row[2].as_i64().unwrap() as i32;
            cell.land_type = row[3].as_u64().unwrap() as u8;
        }
        let records: Vec<_> = case["records"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| BridgeEndpointRecord {
                endpoint_a: coord(&r[0]),
                endpoint_b: coord(&r[1]),
                active: r[2].as_bool().unwrap(),
                bridge_kind: BridgeRecordKind::High,
            })
            .collect();
        let mut zones = ZoneGrid::build_with_native_bridge_geometry(
            &grid,
            &terrain,
            &records,
            width,
            height,
            Some((8, 8)),
        );
        let base = zones.base_topology_mut();
        base.zone_ids.fill(1);
        base.raw_zone_ids_by_row[MovementZone::Normal.matrix_row().unwrap()] = vec![0, 2];
        let b = &case["bounds"];
        let bounds = PlayfieldBounds::from_normalized_local_size(
            b[0].as_i64().unwrap() as i32,
            b[1].as_i64().unwrap() as i32,
            b[2].as_i64().unwrap() as i32,
            b[3].as_i64().unwrap() as i32,
            b[4].as_i64().unwrap() as i32,
        );
        assert_eq!(
            case["dummy_flags"], 0,
            "fixture currently supplies constructor bridge flags"
        );
        let dummy = terrain.shared_cell_dummy();
        dummy.reconstruct_for_map_resize();
        dummy.stamp_coord(1234, -2345);
        let start = coord(&case["start"]);
        let goal = coord(&case["goal"]);
        let entry = prepare_native_path_entry(
            Some(&zones),
            Some(&terrain),
            MovementZone::Normal,
            start,
            case["start_bridge"].as_bool().unwrap(),
            goal,
            case["allow"].as_bool().unwrap(),
            Some(bounds),
        );
        assert_eq!(
            entry.raw_equal,
            Some(case["labels"][0] == case["labels"][1]),
            "{}",
            case["name"]
        );
        assert_eq!(
            entry.hierarchy_start,
            coord(&case["hierarchy_start"]),
            "{}",
            case["name"]
        );
        assert_eq!(
            entry.hierarchy_goal,
            coord(&case["hierarchy_goal"]),
            "{}",
            case["name"]
        );
        assert_eq!(
            entry.endpoints_in_playfield,
            case["endpoints_inside"].as_bool().unwrap(),
            "{}",
            case["name"]
        );
        let terminal = coord(&case["dummy_coord"]);
        assert_eq!(
            dummy.snapshot().coord,
            (i32::from(terminal.0 as i16), i32::from(terminal.1 as i16)),
            "{}",
            case["name"]
        );
        if case["name"] == "initial_source_miss_False" {
            // Physical backing admits this same-cell request, while the native
            // allocation table has a hole. With no playfield configured and
            // hierarchy disabled, only the live entry lookup publishes it;
            // AStar then has no route for a goal in its start cell
            // (0x00429BF3..0x00429C0A).
            let hole = (4, 4);
            assert!(terrain.cell(hole.0, hole.1).is_none());
            dummy.stamp_coord(1234, -2345);
            assert!(
                find_layered_path_zoned_marker(
                    &grid,
                    hole,
                    MovementLayer::Ground,
                    hole,
                    Some(&zones),
                    MovementZone::Normal,
                    Some(MovementZone::Normal),
                    Some(&terrain),
                    None,
                    None,
                    false,
                    None
                )
                .is_none()
            );
            assert_eq!(dummy.snapshot().coord, (4, 4));
        }
    }
}

/// `AStar_pathfind_search` (`0x0042C900`) takes the row from the type's
/// `MovementZone=` (`+0x5B4`, `0x0042C963..0x0042C96D`) and compares the raw
/// labels at `0x0042CB22` for every row: nothing there exempts Water or
/// WaterBeach. Two water cells split by land carry different labels in both
/// rows, so the search returns NULL before the cell A*; the Fly row, which
/// joins them, searches.
#[test]
fn water_rows_reject_split_labels_before_the_cell_astar() {
    let terrain = super::super::zone_map_tests::terrain_from_zone_classes(
        3,
        1,
        &[zone_class::WATER, zone_class::GROUND, zone_class::WATER],
        &[0; 3],
    );
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let zones =
        ZoneGrid::build_with_native_bridge_geometry(&grid, &terrain, &[], 3, 1, Some((3, 1)));
    let counts = BlockerNeighborCounts::new(3, 1);
    let search = |mz| {
        find_layered_path_zoned_marker_detailed(
            &grid,
            (0, 0),
            MovementLayer::Ground,
            (2, 0),
            Some(&zones),
            mz,
            Some(mz),
            Some(&terrain),
            None,
            Some(&counts),
            MoverSearchFacts {
                urgency: 0,
                foot_entry: &super::super::WalkableEntry::new(&grid),
            },
            true,
            None,
        )
        .map(|path| path.len())
    };
    for mz in [MovementZone::Water, MovementZone::WaterBeach] {
        let label = |cell| {
            zones
                .get_zone_id_native(&terrain, cell, mz, false)
                .expect("raw label")
        };
        assert_ne!(
            label((0, 0)),
            label((2, 0)),
            "{mz:?}: land splits the water"
        );
        assert_eq!(
            search(mz),
            Err(PathSearchFailure::NativeEntryRejected),
            "{mz:?}: 0x0042CB22 refuses before the cell A*"
        );
    }
    assert_eq!(
        search(MovementZone::Fly),
        Ok(3),
        "the Fly row joins the water and searches"
    );
}
