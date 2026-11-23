//! History-sensitive navigation belongs to the live hash. Native ordinary
//! load deliberately clears/rebuilds hierarchy at67E8CD ->581F50; it is not a
//! byte-for-byte rollback. These flat fixtures bound the Rust restore helper.

use super::Simulation;
use crate::map::resolved_terrain::{ResolvedTerrainGrid, test_flat_cell};
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_hierarchy::ZoneRecord;
use crate::sim::pathfinding::zone_map::ZoneGrid;

// Supplied branch-test bounds, as in zone_map_tests::repair_test_bounds. They
// admit the retained flat cells through the native mode1 query; None rejects.
fn bounds() -> crate::map::playfield::PlayfieldBounds {
    crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -1_000,
        off_100: -1_000,
        off_104: 2_000,
        off_108: 2_000,
    }
}

fn terrain() -> ResolvedTerrainGrid {
    ResolvedTerrainGrid::from_cells(
        16,
        16,
        (0..16)
            .flat_map(|y| (0..16).map(move |x| test_flat_cell(x, y)))
            .collect(),
    )
}

fn world(terrain: ResolvedTerrainGrid) -> Simulation {
    let path = PathGrid::from_resolved_terrain(&terrain);
    let zones = ZoneGrid::build_with_native_map_context(&path, &terrain, &[], None, Some(bounds()));
    let mut sim = Simulation::new();
    sim.playfield_bounds = Some(bounds());
    sim.install_resolved_terrain_for_new_map(terrain);
    sim.zone_grid = Some(zones.into());
    sim
}

#[test]
fn identical_source_and_houses_with_different_retained_threat_seed_hash_differently() {
    let mut a = world(terrain());
    let mut b = world(terrain());
    assert_eq!(a.state_hash(), b.state_hash());
    let zones = b.zone_grid.as_mut().unwrap();
    let mut hierarchy = zones
        .hierarchy_for(crate::rules::locomotor_type::MovementZone::Normal)
        .unwrap()
        .clone();
    let graph = &mut hierarchy.levels_mut()[1];
    let zone = graph.zone_at(4, 4);
    let record = graph.record(zone).unwrap();
    let seed = if record.native_threat_index() == Some(131) {
        (4, 0)
    } else {
        (0, 0)
    };
    graph.set_record(ZoneRecord::from_seed(
        zone,
        record.parent,
        record.zone_type,
        seed,
        crate::map::cell_index::CellGrid::Retail,
    ));
    zones.replace_hierarchy(hierarchy);
    assert_ne!(
        a.state_hash(),
        b.state_hash(),
        "585F40 now reads a different House grid cell"
    );
    // Removing just this owner proves every other retained source/House field
    // in the world hash stayed identical; no second authority was introduced.
    a.zone_grid = None;
    b.zone_grid = None;
    assert_eq!(a.state_hash(), b.state_hash());
}
