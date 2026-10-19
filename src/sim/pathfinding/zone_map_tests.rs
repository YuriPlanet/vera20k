//! Tests for the zone map: native base topology, row projection and repair.

use super::zone_build::hierarchy::{
    LocalHierarchyPatchResult, incremental_rebuild_zone_hierarchy_around_cell,
};
use super::zone_hierarchy::{ZoneEdgeRecord, ZoneHierarchy, ZoneLevelGraph, ZoneRecord};
use super::zone_incremental::{
    PackedZoneCoord, ZoneRepairKind, ZoneRepairOutcome, repair_zone_cell,
};
use super::zone_map::*;
use crate::map::resolved_terrain::{
    BridgeDirection, BridgeLayer, ResolvedTerrainCell, ResolvedTerrainGrid, zone_class,
};
use crate::map::tube_facts::{TubeFact, TubeId, TubeSource};
use crate::rules::locomotor_type::MovementZone;
use crate::rules::terrain_rules::TerrainClass;
use crate::sim::bridge_state::{BridgeEndpointRecord, BridgeRecordKind, BridgeRuntimeState};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::PathGrid;

fn tiny_hierarchy() -> ZoneHierarchy {
    let mut level2 = ZoneLevelGraph::new(1);
    level2.set_record(ZoneRecord::new(1, 0, 0));
    let mut level1 = ZoneLevelGraph::new(1);
    level1.set_record(ZoneRecord::new(1, 1, 0));
    let mut level0 = ZoneLevelGraph::new(1);
    level0.set_record(ZoneRecord::new(1, 1, 0));
    level0.push_edge(1, ZoneEdgeRecord::new(1, 0));
    ZoneHierarchy::new(level0, level1, level2)
}

fn water_row_terrain(width: u16) -> ResolvedTerrainGrid {
    let cells = (0..width)
        .map(|rx| ResolvedTerrainCell {
            land_type: crate::sim::pathfinding::passability::LandType::Water.as_index(),
            terrain_class: TerrainClass::Water,
            is_water: true,
            zone_type: 4,
            ..crate::map::resolved_terrain::test_flat_cell(rx, 0)
        })
        .collect();
    ResolvedTerrainGrid::from_cells(width, 1, cells)
}

fn clear_beach_water_row_terrain() -> ResolvedTerrainGrid {
    let land_types = [
        crate::sim::pathfinding::passability::LandType::Clear.as_index(),
        crate::sim::pathfinding::passability::LandType::Beach.as_index(),
        crate::sim::pathfinding::passability::LandType::Water.as_index(),
    ];
    let cells = land_types
        .into_iter()
        .enumerate()
        .map(|(rx, land_type)| ResolvedTerrainCell {
            land_type,
            terrain_class: match land_type {
                x if x == crate::sim::pathfinding::passability::LandType::Water.as_index() => {
                    TerrainClass::Water
                }
                x if x == crate::sim::pathfinding::passability::LandType::Beach.as_index() => {
                    TerrainClass::Beach
                }
                _ => TerrainClass::Clear,
            },
            is_water: land_type == crate::sim::pathfinding::passability::LandType::Water.as_index(),
            zone_type: match land_type {
                x if x == crate::sim::pathfinding::passability::LandType::Water.as_index() => 4,
                x if x == crate::sim::pathfinding::passability::LandType::Beach.as_index() => 3,
                _ => 0,
            },
            ..crate::map::resolved_terrain::test_flat_cell(rx as u16, 0)
        })
        .collect();
    ResolvedTerrainGrid::from_cells(3, 1, cells)
}

fn automatic_tube_shell_ground_terrain() -> ResolvedTerrainGrid {
    let mut cells = Vec::new();
    let mut tubes = Vec::new();
    for rx in 0..5u16 {
        let is_low_bridge = (1..=3).contains(&rx);
        let tube_index = if is_low_bridge {
            let tube_id = TubeId(tubes.len() as u16);
            tubes.push(TubeFact::auto_low_bridge((rx, 0), 2));
            Some(tube_id)
        } else {
            None
        };
        cells.push(ResolvedTerrainCell {
            land_type: if is_low_bridge {
                crate::sim::pathfinding::passability::LandType::Tunnel.as_index()
            } else {
                crate::sim::pathfinding::passability::LandType::Clear.as_index()
            },
            terrain_class: if is_low_bridge {
                TerrainClass::Tunnel
            } else {
                TerrainClass::Clear
            },
            zone_type: zone_class::GROUND,
            has_bridge_deck: is_low_bridge,
            bridge_layer: is_low_bridge.then(|| BridgeLayer {
                overlay_id: 0x4a,
                overlay_name: "LOBRDG01".to_string(),
                deck_level: 0,
                direction: BridgeDirection::Low,
            }),
            tube_index,
            ..crate::map::resolved_terrain::test_flat_cell(rx, 0)
        });
    }
    ResolvedTerrainGrid::from_cells_with_tubes(5, 1, cells, tubes)
}

pub(super) fn terrain_from_zone_classes(
    width: u16,
    height: u16,
    classes: &[u8],
    levels: &[u8],
) -> ResolvedTerrainGrid {
    assert_eq!(classes.len(), width as usize * height as usize);
    assert_eq!(levels.len(), classes.len());
    let prototype = water_row_terrain(1).cells.into_iter().next().unwrap();
    let cells = classes
        .iter()
        .zip(levels)
        .enumerate()
        .map(|(index, (&zone_type, &level))| {
            let mut cell = prototype.clone();
            cell.rx = (index % width as usize) as u16;
            cell.ry = (index / width as usize) as u16;
            cell.level = level;
            cell.zone_type = zone_type;
            cell.outside_playfield = zone_type == zone_class::OUTSIDE;
            cell.is_water = zone_type == zone_class::WATER;
            cell.land_type = if cell.is_water {
                crate::sim::pathfinding::passability::LandType::Water.as_index()
            } else if zone_type == zone_class::BEACH {
                crate::sim::pathfinding::passability::LandType::Beach.as_index()
            } else {
                crate::sim::pathfinding::passability::LandType::Clear.as_index()
            };
            cell.terrain_class = if cell.is_water {
                TerrainClass::Water
            } else if zone_type == zone_class::BEACH {
                TerrainClass::Beach
            } else {
                TerrainClass::Clear
            };
            cell.ground_walk_blocked = false;
            cell.terrain_object_blocks = false;
            cell.overlay_blocks = false;
            cell
        })
        .collect();
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

fn native_nonbridge_zone_fixture(width: u16, height: u16) -> ZoneGrid {
    let cell_count = usize::from(width) * usize::from(height);
    let terrain = terrain_from_zone_classes(
        width,
        height,
        &vec![zone_class::GROUND; cell_count],
        &vec![0; cell_count],
    );
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], width, height)
}

#[test]
fn gsi_04_01_nonbridge_getzoneid_uses_padded_square_clamps_and_raw_rows() {
    let mut zones = native_nonbridge_zone_fixture(2, 2);
    {
        let base = zones.base_topology_mut();
        base.movement_classes = vec![0; 4];
        base.zone_ids = vec![2, 3, 4, 5];
        for row in &mut base.raw_zone_ids_by_row {
            row.resize(6, 0);
        }
        base.raw_zone_ids_by_row[MovementZone::Normal.matrix_row().unwrap()][0] = 41;
        base.raw_zone_ids_by_row[MovementZone::Normal.matrix_row().unwrap()][2] = 42;
        base.raw_zone_ids_by_row[MovementZone::Crusher.matrix_row().unwrap()][2] = 1;
        base.raw_zone_ids_by_row[MovementZone::Destroyer.matrix_row().unwrap()][2] = u16::MAX;
        base.raw_zone_ids_by_row[MovementZone::Amphibious.matrix_row().unwrap()][2] = 73;
    }

    assert_eq!(
        zones.get_zone_id_nonbridge_native((2, 0), MovementZone::Normal),
        Some(41),
        "the native W+1 padded final column owns base cluster 0"
    );
    assert_eq!(
        zones.get_zone_id_nonbridge_native((0, 2), MovementZone::Normal),
        Some(41),
        "the native W+1 padded final row owns base cluster 0"
    );
    assert_eq!(
        zones.get_zone_id_nonbridge_native((-1, 0), MovementZone::Normal),
        Some(42),
        "a negative linear index clamps to the first real base entry"
    );
    assert_eq!(
        zones.get_zone_id_nonbridge_native((u16::MAX.into(), 0), MovementZone::Normal),
        Some(42),
        "coordinate components truncate to their packed signed-i16 words"
    );
    assert_eq!(
        zones
            .get_zone_id_nonbridge_native((i16::MAX.into(), i16::MAX.into()), MovementZone::Normal),
        Some(41),
        "an oversized positive linear index clamps to the final padded entry"
    );

    assert_eq!(
        zones.get_zone_id_nonbridge_native((0, 0), MovementZone::Normal),
        Some(42)
    );
    assert_eq!(
        zones.get_zone_id_nonbridge_native((0, 0), MovementZone::Crusher),
        Some(1),
        "raw reserved label 1 is not flattened"
    );
    assert_eq!(
        zones.get_zone_id_nonbridge_native((0, 0), MovementZone::Destroyer),
        Some(u16::MAX),
        "raw 0xffff is not flattened"
    );
    assert_eq!(
        zones.get_zone_id_nonbridge_native((0, 0), MovementZone::Amphibious),
        Some(73),
        "the same base cluster projects through the selected movement row"
    );
}

#[test]
fn gsi_04_01_nonbridge_getzoneid_rejects_non_native_topology_metadata() {
    let nonsquare = native_nonbridge_zone_fixture(2, 1);
    assert_eq!(
        nonsquare.get_zone_id_nonbridge_native((0, 0), MovementZone::Normal),
        None
    );

    let mut inconsistent = native_nonbridge_zone_fixture(2, 2);
    inconsistent.base_topology_mut().zone_ids.pop();
    assert_eq!(
        inconsistent.get_zone_id_nonbridge_native((0, 0), MovementZone::Normal),
        None
    );

    let mut missing_raw_cluster = native_nonbridge_zone_fixture(2, 2);
    {
        let base = missing_raw_cluster.base_topology_mut();
        base.zone_ids[0] = 500;
        base.raw_zone_ids_by_row[MovementZone::Normal.matrix_row().unwrap()].truncate(2);
    }
    assert_eq!(
        missing_raw_cluster.get_zone_id_nonbridge_native((0, 0), MovementZone::Normal),
        None
    );
    assert_eq!(
        missing_raw_cluster.get_zone_id_nonbridge_native((0, 0), MovementZone::Invalid),
        None,
        "native's unchecked invalid row is an explicit safe failure in Rust"
    );
}

#[test]
fn gsi_04_06_scanline_storage_fringe_merges_isometric_cardinal_cells() {
    let terrain = terrain_from_zone_classes(
        2,
        2,
        &[zone_class::GROUND, 7, 7, zone_class::GROUND],
        &[0; 4],
    );
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], 2, 2);
    let normal = zones.map_for(MovementZone::Normal).unwrap();

    assert_eq!(normal.zone_at(0, 0, MovementLayer::Ground), 2);
    assert_eq!(normal.zone_at(1, 1, MovementLayer::Ground), 2);
}

#[test]
fn gsi_04_06_scanline_fringe_edge_merges_amphibious_class_transition() {
    let terrain = terrain_from_zone_classes(
        2,
        2,
        &[zone_class::GROUND, 7, 7, zone_class::BEACH],
        &[0; 4],
    );
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], 2, 2);

    let normal = zones.map_for(MovementZone::Normal).unwrap();
    assert_eq!(normal.zone_at(0, 0, MovementLayer::Ground), 2);
    assert_eq!(normal.zone_at(1, 1, MovementLayer::Ground), ZONE_INVALID);

    let amphibious = zones.map_for(MovementZone::Amphibious).unwrap();
    assert_eq!(amphibious.zone_at(0, 0, MovementLayer::Ground), 2);
    assert_eq!(amphibious.zone_at(1, 1, MovementLayer::Ground), 2);
}

#[test]
fn gsi_04_06_base_fill_allows_right_delta_three_but_not_vertical() {
    let east = terrain_from_zone_classes(2, 1, &[0, 0], &[0, 3]);
    let east_path = PathGrid::from_resolved_terrain(&east);
    let east_zones = ZoneGrid::build_with_terrain(&east_path, &east, &[], 2, 1);
    let east_normal = east_zones.map_for(MovementZone::Normal).unwrap();
    assert_eq!(
        east_normal.zone_at(0, 0, MovementLayer::Ground),
        east_normal.zone_at(1, 0, MovementLayer::Ground)
    );

    let vertical = terrain_from_zone_classes(1, 2, &[0, 0], &[0, 3]);
    let vertical_path = PathGrid::from_resolved_terrain(&vertical);
    let vertical_zones = ZoneGrid::build_with_terrain(&vertical_path, &vertical, &[], 1, 2);
    let vertical_normal = vertical_zones.map_for(MovementZone::Normal).unwrap();
    assert_eq!(vertical_normal.zone_at(0, 0, MovementLayer::Ground), 2);
    assert_eq!(vertical_normal.zone_at(0, 1, MovementLayer::Ground), 3);
}

#[test]
fn gsi_04_06_class_transition_merges_for_amphibious_not_normal() {
    let terrain =
        terrain_from_zone_classes(2, 1, &[zone_class::GROUND, zone_class::BEACH], &[0; 2]);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], 2, 1);

    let amphibious = zones.map_for(MovementZone::Amphibious).unwrap();
    assert_eq!(amphibious.zone_at(0, 0, MovementLayer::Ground), 2);
    assert_eq!(amphibious.zone_at(1, 0, MovementLayer::Ground), 2);

    let normal = zones.map_for(MovementZone::Normal).unwrap();
    assert_eq!(normal.zone_at(0, 0, MovementLayer::Ground), 2);
    assert_eq!(normal.zone_at(1, 0, MovementLayer::Ground), ZONE_INVALID);
}

#[test]
fn gsi_04_06_class_six_boundary_bypasses_height_for_subterranean_row() {
    let terrain =
        terrain_from_zone_classes(2, 1, &[zone_class::GROUND, zone_class::IMPASSABLE], &[0, 7]);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], 2, 1);
    let subterranean = zones.map_for(MovementZone::Subterranean).unwrap();

    assert_eq!(subterranean.zone_at(0, 0, MovementLayer::Ground), 2);
    assert_eq!(subterranean.zone_at(1, 0, MovementLayer::Ground), 2);
}

#[test]
fn gsi_04_06_active_bridge_edge_merges_base_zones_before_projection() {
    let terrain = terrain_from_zone_classes(5, 1, &[0, 7, 7, 7, 0], &[0; 5]);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let without_bridge = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], 5, 1);
    let normal = without_bridge.map_for(MovementZone::Normal).unwrap();
    assert_eq!(normal.zone_at(0, 0, MovementLayer::Ground), 2);
    assert_eq!(normal.zone_at(4, 0, MovementLayer::Ground), 3);

    let records = [BridgeEndpointRecord {
        endpoint_a: (0, 0),
        endpoint_b: (4, 0),
        active: true,
        bridge_kind: BridgeRecordKind::High,
    }];
    let with_bridge = ZoneGrid::build_with_terrain(&path_grid, &terrain, &records, 5, 1);
    let normal = with_bridge.map_for(MovementZone::Normal).unwrap();
    assert_eq!(normal.zone_at(0, 0, MovementLayer::Ground), 2);
    assert_eq!(normal.zone_at(4, 0, MovementLayer::Ground), 2);
}

#[test]
fn gsi_04_06_all_thirteen_rows_preserve_native_derived_labels() {
    let classes = [0, 7, 1, 7, 2, 7, 3, 7, 4, 7, 5, 7, 6];
    let terrain = terrain_from_zone_classes(13, 1, &classes, &[0; 13]);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], 13, 1);

    assert_eq!(MovementZone::all_ground().len(), 13);
    for &movement_zone in MovementZone::all_ground() {
        let row = crate::sim::pathfinding::passability::MOVEMENT_ZONE_PASSABILITY
            [movement_zone.matrix_row().unwrap()];
        let map = zones.map_for(movement_zone).unwrap();
        let mut next_label = 2;
        for class in 0..=6u8 {
            let x = u16::from(class) * 2;
            let expected = if row[class as usize] == 1 {
                let label = next_label;
                next_label += 1;
                label
            } else {
                ZONE_INVALID
            };
            assert_eq!(
                map.zone_at(x, 0, MovementLayer::Ground),
                expected,
                "{movement_zone:?} class {class}"
            );
        }
        assert_eq!(map.zone_count(), next_label - 1, "{movement_zone:?}");
        assert_eq!(map.zone_at(1, 0, MovementLayer::Ground), ZONE_INVALID);
    }
}

#[test]
fn gsi_04_06_simulation_rebuild_initializes_zone_grid() {
    let terrain = terrain_from_zone_classes(1, 1, &[zone_class::GROUND], &[0]);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolved_terrain = Some(terrain);

    sim.rebuild_zone_grid(&path_grid);

    let normal = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(MovementZone::Normal))
        .unwrap();
    assert_eq!(normal.zone_at(0, 0, MovementLayer::Ground), 2);
}

#[test]
fn gsi_04_06_pathgrid_blocking_does_not_rewrite_cell_owned_reduced_class() {
    let terrain = terrain_from_zone_classes(
        3,
        1,
        &[zone_class::GROUND, zone_class::CRUSHABLE, zone_class::WALL],
        &[0; 3],
    );
    let mut path_grid = PathGrid::from_resolved_terrain(&terrain);
    for x in 0..3 {
        path_grid.set_blocked(x, 0, true);
    }
    let zones = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], 3, 1);

    assert_ne!(
        zones
            .map_for(MovementZone::Normal)
            .unwrap()
            .zone_at(0, 0, MovementLayer::Ground),
        ZONE_INVALID,
        "a PathGrid bit cannot turn Ground into Building"
    );
    assert_ne!(
        zones
            .map_for(MovementZone::Crusher)
            .unwrap()
            .zone_at(1, 0, MovementLayer::Ground),
        ZONE_INVALID,
        "Crusher must retain the Crushable column"
    );
    assert_eq!(
        zones
            .map_for(MovementZone::Infantry)
            .unwrap()
            .zone_at(2, 0, MovementLayer::Ground),
        ZONE_INVALID,
        "Wall must not be coerced to Infantry-passable Building"
    );
}

#[test]
fn gsi_04_06_simulation_detects_class_only_change_with_identical_pathgrid() {
    let terrain = terrain_from_zone_classes(1, 1, &[zone_class::GROUND], &[0]);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let mut sim = crate::sim::world::Simulation::new();
    sim.resolved_terrain = Some(terrain);
    sim.rebuild_zone_grid(&path_grid);
    assert_ne!(
        sim.zone_grid
            .as_ref()
            .unwrap()
            .map_for(MovementZone::Normal)
            .unwrap()
            .zone_at(0, 0, MovementLayer::Ground),
        ZONE_INVALID
    );

    sim.resolved_terrain.as_mut().unwrap().cells[0].zone_type = zone_class::CRUSHABLE;
    sim.rebuild_zone_grid(&path_grid);

    let zones = sim.zone_grid.as_ref().unwrap();
    assert_eq!(
        zones
            .map_for(MovementZone::Normal)
            .unwrap()
            .zone_at(0, 0, MovementLayer::Ground),
        ZONE_INVALID
    );
    assert_ne!(
        zones
            .map_for(MovementZone::Crusher)
            .unwrap()
            .zone_at(0, 0, MovementLayer::Ground),
        ZONE_INVALID
    );
}

// Deliberately broad final MapClass fields for the tiny rectangular fixtures.
// These are supplied branch-test bounds, not normalized retail-map dimensions.
fn repair_test_bounds() -> crate::map::playfield::PlayfieldBounds {
    crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -1_000,
        off_100: -1_000,
        off_104: 2_000,
        off_108: 2_000,
    }
}

fn base_repair_fixture(
    classes: [u8; 9],
    clusters: [ZoneId; 9],
) -> (ResolvedTerrainGrid, PathGrid, ZoneGrid) {
    let terrain = terrain_from_zone_classes(3, 3, &classes, &[0; 9]);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let mut zones = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], 3, 3);
    let base = zones.base_topology_mut();
    base.movement_classes = classes.to_vec();
    base.zone_ids = clusters.to_vec();
    let zone_count = clusters.iter().copied().max().unwrap_or(0);
    for row in &mut base.raw_zone_ids_by_row {
        row.resize(zone_count as usize + 1, 1);
        row[0] = u16::MAX;
        for cluster in 1..=zone_count {
            row[cluster as usize] = cluster + 1;
        }
    }
    let flat_ids: Vec<ZoneId> = clusters
        .iter()
        .map(|&cluster| {
            if cluster != ZONE_INVALID {
                cluster + 1
            } else {
                ZONE_INVALID
            }
        })
        .collect();
    for &movement_zone in MovementZone::all_ground() {
        *zones.map_mut(movement_zone).unwrap().zone_ids_mut() = flat_ids.clone();
    }
    (terrain, path_grid, zones)
}

#[test]
fn gsi_04_06_base_repair_uses_transition_count_first_candidate_and_preserves_tables() {
    // Neighbor order from center: N=A, NE=B, E=A, then sentinels. This is
    // exactly three row-0 mapping transitions, so the first candidate wins.
    let classes = [7, 0, 2, 7, 0, 0, 7, 7, 7];
    let clusters = [0, 1, 2, 0, 3, 3, 0, 0, 0];
    let (terrain, path_grid, mut zones) = base_repair_fixture(classes, clusters);
    for &movement_zone in MovementZone::all_ground() {
        let map = zones.map_for(movement_zone).unwrap();
        assert_eq!(map.zone_at(1, 1, MovementLayer::Ground), 4);
    }
    let (before_raw, before_clusters) = {
        let base = zones.base_topology_mut();
        (base.raw_zone_ids_by_row.clone(), base.zone_ids.clone())
    };
    let before_maps: Vec<Vec<ZoneId>> = MovementZone::all_ground()
        .iter()
        .map(|&mz| zones.map_for(mz).unwrap().zone_ids_slice().to_vec())
        .collect();

    let outcome = repair_zone_cell(
        &mut zones,
        PackedZoneCoord::new(1, 1),
        ZoneRepairKind::AssignOrphaned,
        &path_grid,
        None,
        &terrain,
        &[],
    );
    assert_eq!(outcome, ZoneRepairOutcome::Adopted { cluster: 1 });

    {
        let base = zones.base_topology_mut();
        assert_eq!(base.raw_zone_ids_by_row, before_raw);
        assert_eq!(base.zone_ids[4], 1);
        for (index, (&before, &after)) in before_clusters.iter().zip(&base.zone_ids).enumerate() {
            if index != 4 {
                assert_eq!(after, before, "unrelated base cell {index}");
            }
        }
    }
    for (row, &movement_zone) in MovementZone::all_ground().iter().enumerate() {
        let after = zones.map_for(movement_zone).unwrap().zone_ids_slice();
        for index in 0..after.len() {
            if index != 4 {
                assert_eq!(
                    after[index], before_maps[row][index],
                    "row {row} cell {index}"
                );
            }
        }
        assert_eq!(before_maps[row][4], 4);
        assert_eq!(after[4], 2, "target inherits candidate cluster mapping");
    }
}

#[test]
fn gsi_04_06_base_repair_fallbacks_and_explicit_merge_provenance() {
    let alternating_classes = [7, 0, 2, 7, 0, 0, 7, 7, 2];
    let alternating_clusters = [0, 1, 2, 0, 3, 1, 0, 0, 2];
    let (terrain, path_grid, mut zones) =
        base_repair_fixture(alternating_classes, alternating_clusters);
    assert_eq!(
        repair_zone_cell(
            &mut zones,
            PackedZoneCoord::new(1, 1),
            ZoneRepairKind::AssignOrphaned,
            &path_grid,
            None,
            &terrain,
            &[],
        ),
        ZoneRepairOutcome::FullRebuild,
        "A/B/A/B reaches four transitions and must rebuild"
    );

    let non_ground_assign_classes = [7, 0, 2, 7, 2, 7, 7, 7, 7];
    let non_ground_assign_clusters = [0, 1, 2, 0, 2, 0, 0, 0, 0];
    let (terrain, path_grid, mut zones) =
        base_repair_fixture(non_ground_assign_classes, non_ground_assign_clusters);
    assert_eq!(
        repair_zone_cell(
            &mut zones,
            PackedZoneCoord::new(1, 1),
            ZoneRepairKind::AssignOrphaned,
            &path_grid,
            None,
            &terrain,
            &[],
        ),
        ZoneRepairOutcome::FullRebuild,
        "AssignOrphaned cannot adopt when the target is non-ground"
    );

    let merge_classes = [7, 2, 0, 7, 2, 2, 7, 7, 7];
    let merge_clusters = [0, 2, 1, 0, 3, 2, 0, 0, 0];
    let (terrain, path_grid, mut zones) = base_repair_fixture(merge_classes, merge_clusters);
    assert_eq!(
        repair_zone_cell(
            &mut zones,
            PackedZoneCoord::new(1, 1),
            ZoneRepairKind::MergeAdjacent,
            &path_grid,
            None,
            &terrain,
            &[],
        ),
        ZoneRepairOutcome::Adopted { cluster: 2 },
        "Merge adopts the first same-type non-ground neighbor"
    );

    let mut sentinel = terrain_from_zone_classes(1, 1, &[zone_class::OUTSIDE], &[0]);
    sentinel.cells[0].outside_playfield = false;
    let sentinel_path = PathGrid::from_resolved_terrain(&sentinel);
    let mut sentinel_zones = ZoneGrid::build_with_terrain(&sentinel_path, &sentinel, &[], 1, 1);
    assert_eq!(
        repair_zone_cell(
            &mut sentinel_zones,
            PackedZoneCoord::new(0, 0),
            ZoneRepairKind::MergeAdjacent,
            &sentinel_path,
            None,
            &sentinel,
            &[],
        ),
        ZoneRepairOutcome::SentinelNoOp
    );
    assert_eq!(
        repair_zone_cell(
            &mut sentinel_zones,
            PackedZoneCoord::new(-1, 0),
            ZoneRepairKind::MergeAdjacent,
            &sentinel_path,
            None,
            &sentinel,
            &[],
        ),
        ZoneRepairOutcome::OutsideNoOp
    );
}

#[derive(Debug, PartialEq, Eq)]
struct HierarchyRegionSnapshot {
    cell_ids: Vec<ZoneId>,
    records: Vec<(ZoneId, Option<ZoneRecord>, Vec<ZoneEdgeRecord>)>,
}

fn hierarchy_region_snapshot(
    hierarchy: &ZoneHierarchy,
    width: u16,
    height: u16,
    x_min: u16,
) -> Vec<HierarchyRegionSnapshot> {
    (0..3)
        .map(|level| {
            let graph = hierarchy.level(level).unwrap();
            let mut cell_ids = Vec::new();
            let mut record_ids = Vec::new();
            for y in 0..height {
                for x in x_min..width {
                    let zone = graph.zone_at(x, y);
                    cell_ids.push(zone);
                    if zone != ZONE_INVALID && !record_ids.contains(&zone) {
                        record_ids.push(zone);
                    }
                }
            }
            let records = record_ids
                .into_iter()
                .map(|zone| (zone, graph.record(zone), graph.edges(zone).to_vec()))
                .collect();
            HierarchyRegionSnapshot { cell_ids, records }
        })
        .collect()
}

#[test]
fn gsi_04_06_fallback_rebuilds_base_without_resetting_hierarchy_high_water() {
    let width = 16;
    let height = 8;
    let classes: Vec<u8> = (0..height)
        .flat_map(|_| {
            (0..width).map(|x| {
                if x == 7 {
                    zone_class::OUTSIDE
                } else if x >= 12 {
                    zone_class::WALL
                } else {
                    zone_class::GROUND
                }
            })
        })
        .collect();
    let terrain = terrain_from_zone_classes(width, height, &classes, &vec![0; classes.len()]);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let mut zones = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], width, height);
    let expected = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], width, height);

    let (initial_slots, initial_right_ids) = {
        let (_, hierarchy) = zones.base_and_hierarchy_mut();
        (
            std::array::from_fn::<_, 3, _>(|level| {
                hierarchy.level(level).unwrap().record_slot_count()
            }),
            std::array::from_fn::<_, 3, _>(|level| hierarchy.level(level).unwrap().zone_at(10, 2)),
        )
    };
    {
        let (base, hierarchy) = zones.base_and_hierarchy_mut();
        assert_eq!(
            incremental_rebuild_zone_hierarchy_around_cell(
                hierarchy,
                base,
                &terrain,
                &[],
                (10, 2),
                width,
                height,
            ),
            LocalHierarchyPatchResult::Patched
        );
    }

    let prior_high_water = {
        let (_, hierarchy) = zones.base_and_hierarchy_mut();
        std::array::from_fn::<_, 3, _>(|level| {
            let graph = hierarchy.level(level).unwrap();
            assert!(graph.record_slot_count() > initial_slots[level]);
            assert!(graph.record(initial_right_ids[level]).is_some());
            assert!(graph.edges(initial_right_ids[level]).is_empty());
            graph.record_slot_count()
        })
    };
    let right_before = {
        let (_, hierarchy) = zones.base_and_hierarchy_mut();
        hierarchy_region_snapshot(hierarchy, width, height, 8)
    };
    assert!(
        right_before
            .iter()
            .any(|level| { level.records.iter().any(|(_, _, edges)| !edges.is_empty()) })
    );

    let index = |x: usize, y: usize| y * width as usize + x;
    {
        let base = zones.base_topology_mut();
        for &(x, y, zone_type, cluster) in &[
            (2, 1, zone_class::GROUND, 1),
            (3, 1, zone_class::WALL, 2),
            (3, 2, zone_class::GROUND, 1),
            (3, 3, zone_class::WALL, 2),
            (2, 3, zone_class::OUTSIDE, 0),
            (1, 3, zone_class::OUTSIDE, 0),
            (1, 2, zone_class::OUTSIDE, 0),
            (1, 1, zone_class::OUTSIDE, 0),
        ] {
            base.movement_classes[index(x, y)] = zone_type;
            base.zone_ids[index(x, y)] = cluster;
        }
        base.movement_classes[index(2, 2)] = zone_class::GROUND;
        base.zone_ids[index(2, 2)] = 1;
        base.raw_zone_ids_by_row[0][0] = u16::MAX;
        base.raw_zone_ids_by_row[0][1] = 2;
        base.raw_zone_ids_by_row[0][2] = 3;

        // A distant, deliberately stale base ID/projection proves the
        // connectivity refresh is global while cached attributes are retained.
        base.movement_classes[index(14, 2)] = zone_class::GROUND;
        base.zone_ids[index(14, 2)] = 1;
    }
    zones.project_adopted_base_cell(index(14, 2));
    assert_ne!(
        zones
            .map_for(MovementZone::Normal)
            .unwrap()
            .zone_at(14, 2, MovementLayer::Ground),
        expected
            .map_for(MovementZone::Normal)
            .unwrap()
            .zone_at(14, 2, MovementLayer::Ground)
    );

    // Original56C510 rebuilds IDs globally from retained classes/heights,
    // not from current Cell attributes. No Recalc refreshes the distant cell.
    let retained = zones.base_topology_mut();
    let cached_terrain =
        terrain_from_zone_classes(width, height, &retained.movement_classes, &retained.levels);
    let mut expected =
        ZoneGrid::build_with_terrain(&path_grid, &cached_terrain, &[], width, height);
    let expected_base = expected.base_topology_mut().clone();
    let expected_rows: Vec<(MovementZone, Vec<ZoneId>)> = MovementZone::all_ground()
        .iter()
        .map(|&movement_zone| {
            (
                movement_zone,
                expected
                    .map_for(movement_zone)
                    .unwrap()
                    .zone_ids_slice()
                    .to_vec(),
            )
        })
        .collect();

    assert_eq!(
        repair_zone_cell(
            &mut zones,
            PackedZoneCoord::new(2, 2),
            ZoneRepairKind::AssignOrphaned,
            &path_grid,
            Some(repair_test_bounds()),
            &terrain,
            &[],
        ),
        ZoneRepairOutcome::FullRebuild,
        "the ordered A/B/A/B neighborhood has exactly four transitions"
    );

    {
        let actual = zones.base_topology_mut();
        assert_eq!(actual.movement_classes, expected_base.movement_classes);
        assert_eq!(actual.zone_ids, expected_base.zone_ids);
        assert_eq!(
            actual.raw_zone_ids_by_row,
            expected_base.raw_zone_ids_by_row
        );
    }
    for (movement_zone, expected_ids) in expected_rows {
        assert_eq!(
            zones.map_for(movement_zone).unwrap().zone_ids_slice(),
            expected_ids
        );
    }

    let (_, hierarchy) = zones.base_and_hierarchy_mut();
    assert_eq!(
        hierarchy_region_snapshot(hierarchy, width, height, 8),
        right_before,
        "the isolated hierarchy block retains IDs, metadata, and edge order"
    );
    for (level, &high_water) in prior_high_water.iter().enumerate() {
        let graph = hierarchy.level(level).unwrap();
        assert_eq!(graph.zone_at(2, 2), high_water as ZoneId);
        assert!(graph.record_slot_count() > high_water);
    }
}

#[test]
fn gsi_04_06_local_hierarchy_patch_keeps_stale_holes_and_appends_edges_stably() {
    let terrain = terrain_from_zone_classes(6, 1, &[zone_class::GROUND; 6], &[0; 6]);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    let mut zones = ZoneGrid::build_with_terrain(&path_grid, &terrain, &[], 6, 1);
    let (old, middle, right, old_slots) = {
        let (_, hierarchy) = zones.base_and_hierarchy_mut();
        let level0 = hierarchy.level(0).unwrap();
        (
            level0.zone_at(0, 0),
            level0.zone_at(2, 0),
            level0.zone_at(5, 0),
            level0.record_slot_count(),
        )
    };

    assert_eq!(
        repair_zone_cell(
            &mut zones,
            PackedZoneCoord::new(-1, 0),
            ZoneRepairKind::MergeAdjacent,
            &path_grid,
            None,
            &terrain,
            &[],
        ),
        ZoneRepairOutcome::OutsideNoOp
    );
    {
        let (_, hierarchy) = zones.base_and_hierarchy_mut();
        let level0 = hierarchy.level(0).unwrap();
        assert_eq!(level0.zone_at(0, 0), old);
        assert_eq!(level0.zone_at(2, 0), middle);
        assert_eq!(level0.zone_at(5, 0), right);
        assert_eq!(level0.record_slot_count(), old_slots);
    }

    assert_eq!(
        repair_zone_cell(
            &mut zones,
            PackedZoneCoord::new(0, 0),
            ZoneRepairKind::MergeAdjacent,
            &path_grid,
            Some(repair_test_bounds()),
            &terrain,
            &[],
        ),
        ZoneRepairOutcome::Adopted { cluster: 1 }
    );

    let (_, hierarchy) = zones.base_and_hierarchy_mut();
    let level0 = hierarchy.level(0).unwrap();
    let replacement = level0.zone_at(0, 0);
    assert_ne!(replacement, old);
    assert_eq!(level0.zone_at(2, 0), middle, "2x2 alignment boundary");
    assert_eq!(level0.zone_at(5, 0), right, "unrelated level-0 block");
    assert!(level0.record(old).is_some(), "old slot remains allocated");
    assert!(level0.edges(old).is_empty(), "stale slot edges are cleared");
    assert!(level0.record_slot_count() > old_slots);
    let middle_edges: Vec<ZoneId> = level0
        .edges(middle)
        .iter()
        .map(|edge| edge.neighbor)
        .collect();
    assert!(!middle_edges.contains(&old));
    assert_eq!(middle_edges.first().copied(), Some(right));
    assert_eq!(middle_edges.last().copied(), Some(replacement));
    assert_eq!(
        level0.record(replacement).unwrap().parent,
        hierarchy.level(1).unwrap().zone_at(0, 0),
        "8x8 parent refresh links the replacement to the rebuilt coarse level"
    );
}

#[test]
fn zone_grid_hierarchy_is_shared_by_every_row() {
    let terrain = terrain_from_zone_classes(1, 1, &[zone_class::GROUND], &[0]);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let mut zg = ZoneGrid::build_with_terrain(&grid, &terrain, &[], 1, 1);

    let normal = zg.hierarchy_for(MovementZone::Normal).unwrap();
    let water = zg.hierarchy_for(MovementZone::Water).unwrap();
    assert!(std::ptr::eq(normal, water));
    assert!(zg.hierarchy_for(MovementZone::Invalid).is_none());

    zg.set_hierarchy(tiny_hierarchy());
    let replaced = zg.hierarchy_for(MovementZone::Normal).unwrap();
    assert!(std::ptr::eq(
        replaced,
        zg.hierarchy_for(MovementZone::Water).unwrap()
    ));
    assert_eq!(replaced.level(0).unwrap().zone_count(), 1);

    let rebuilt_terrain = terrain_from_zone_classes(3, 1, &[zone_class::GROUND; 3], &[0; 3]);
    let rebuilt_grid = PathGrid::from_resolved_terrain(&rebuilt_terrain);
    let rebuilt = ZoneGrid::build_with_terrain(&rebuilt_grid, &rebuilt_terrain, &[], 3, 1);
    let rebuilt_normal = rebuilt.hierarchy_for(MovementZone::Normal).unwrap();
    let rebuilt_water = rebuilt.hierarchy_for(MovementZone::Water).unwrap();
    assert!(std::ptr::eq(rebuilt_normal, rebuilt_water));
    let rebuilt_level0 = rebuilt_normal.level(0).unwrap();
    assert_eq!(rebuilt_level0.zone_count(), 2);
    assert_eq!(rebuilt_level0.zone_at(0, 0), 1);
    assert_eq!(rebuilt_level0.zone_at(2, 0), 2);
}

#[test]
fn can_reach_compares_the_two_zone_ids() {
    // Ground | rock | ground: the rock splits the ground rows, not Fly.
    let classes = [
        zone_class::GROUND,
        zone_class::IMPASSABLE,
        zone_class::GROUND,
    ];
    let terrain = terrain_from_zone_classes(3, 1, &classes, &[0; 3]);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let zg = ZoneGrid::build_with_terrain(&grid, &terrain, &[], 3, 1);
    let ground = MovementLayer::Ground;
    assert!(zg.can_reach(MovementZone::Normal, (0, 0), ground, (0, 0), ground));
    assert!(!zg.can_reach(MovementZone::Normal, (0, 0), ground, (2, 0), ground));
    assert!(
        !zg.can_reach(MovementZone::Normal, (0, 0), ground, (1, 0), ground),
        "an invalid cell reaches nothing"
    );
    assert!(zg.can_reach(MovementZone::Fly, (0, 0), ground, (2, 0), ground));
    assert!(
        zg.can_reach(MovementZone::Invalid, (0, 0), ground, (2, 0), ground),
        "no zone row: reachable"
    );
}

#[test]
fn water_zone_grid_uses_resolved_land_type_directly() {
    let terrain = water_row_terrain(5);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let zg = ZoneGrid::build_with_terrain(&grid, &terrain, &[], 5, 1);
    assert!(zg.can_reach(
        MovementZone::Water,
        (0, 0),
        MovementLayer::Ground,
        (4, 0),
        MovementLayer::Ground,
    ));
}

#[test]
fn waterbeach_zone_grid_connects_beach_to_water_with_resolved_terrain() {
    let terrain = clear_beach_water_row_terrain();
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let zg = ZoneGrid::build_with_terrain(&grid, &terrain, &[], 3, 1);
    assert!(zg.can_reach(
        MovementZone::WaterBeach,
        (1, 0),
        MovementLayer::Ground,
        (2, 0),
        MovementLayer::Ground,
    ));
    assert!(zg.can_reach(
        MovementZone::Amphibious,
        (0, 0),
        MovementLayer::Ground,
        (2, 0),
        MovementLayer::Ground,
    ));
}

#[test]
fn automatic_tube_shells_keep_ground_connectivity_without_bridge_records() {
    let terrain = automatic_tube_shell_ground_terrain();
    assert!(
        terrain
            .tube_facts()
            .iter()
            .all(|tube| tube.source == TubeSource::AutoLowBridge && tube.path_len() == 0)
    );

    let bridge_state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 300);
    let records = bridge_state.endpoint_records();
    // ComputeBridgeZones56D6E0 requires current ordinal < Tube exit ordinal.
    // Native bridge_records.json automatic_shells proves these same-cell
    // shells emit nothing. This synthetic row remains class GROUND throughout;
    // ordinary class connectivity must not depend on an invented Tube span.
    assert!(records.is_empty());

    let grid = PathGrid::from_resolved_terrain(&terrain);
    let zg = ZoneGrid::build_with_terrain(&grid, &terrain, records, 5, 1);
    assert!(zg.can_reach(
        MovementZone::Normal,
        (0, 0),
        MovementLayer::Ground,
        (4, 0),
        MovementLayer::Ground,
    ));
    assert!(zg.can_reach(
        MovementZone::Infantry,
        (0, 0),
        MovementLayer::Ground,
        (4, 0),
        MovementLayer::Ground,
    ));

    let normal_map = zg.map_for(MovementZone::Normal).expect("normal zone map");
    assert_eq!(
        normal_map.zone_at(2, 0, MovementLayer::Bridge),
        ZONE_INVALID,
        "automatic Tube shells do not create high-bridge redirect records"
    );
}

// ---------------------------------------------------------------------------
// Height continuity tests
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Incremental zone update tests
// ---------------------------------------------------------------------------

#[test]
fn per_movement_zone_grids_are_separate() {
    // Verify that the ZoneCategory collapse is truly gone —
    // each MovementZone variant gets its own independent zone grid.
    let terrain = terrain_from_zone_classes(5, 2, &[zone_class::GROUND; 10], &[0; 10]);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    let zg = ZoneGrid::build_with_terrain(&grid, &terrain, &[], 5, 2);
    // Normal and Crusher should each have their own zone map.
    assert!(
        zg.map_for(MovementZone::Normal).is_some(),
        "Normal should have a zone map"
    );
    assert!(
        zg.map_for(MovementZone::Crusher).is_some(),
        "Crusher should have a zone map"
    );
    assert!(
        zg.map_for(MovementZone::Infantry).is_some(),
        "Infantry should have a zone map"
    );
    assert!(
        zg.map_for(MovementZone::Water).is_some(),
        "Water should have a zone map"
    );
    // The binary rebuild loop covers all 13 matrix rows, including Fly.
    assert!(
        zg.map_for(MovementZone::Fly).is_some(),
        "Fly should have a zone map"
    );
    // All matrix-backed movement zones should have maps.
    for &mz in MovementZone::all_ground() {
        assert!(
            zg.map_for(mz).is_some(),
            "MovementZone {:?} should have a zone map",
            mz
        );
    }
}
