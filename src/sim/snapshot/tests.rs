//! Snapshot tests: shared fixtures and helpers, with the tests grouped by
//! subject in the child modules below.

use super::*;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::MovementZone;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::sim::world::{RevealOutcome, Simulation};
use std::collections::BTreeSet;

/// Helper: advance a sim by one tick with empty inputs.
fn tick(sim: &mut Simulation) {
    sim.advance_tick(&[], None, None, None, 67);
}

fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    crate::map::resolved_terrain::test_grid(
        width,
        height,
        crate::map::resolved_terrain::test_clear_cell,
    )
}

mod gsi_17_04_tests;

fn assert_zone_grids_equivalent(a: &ZoneGrid, b: &ZoneGrid) {
    assert_eq!(a.width(), b.width());
    assert_eq!(a.height(), b.height());
    for &mz in MovementZone::all_ground() {
        let map_a = a.map_for(mz).expect("zone map exists for movement zone");
        let map_b = b.map_for(mz).expect("zone map exists for movement zone");
        let hierarchy_a = a.hierarchy_for(mz).expect("native hierarchy");
        let hierarchy_b = b.hierarchy_for(mz).expect("restored native hierarchy");
        for level in 0..3 {
            let graph_a = hierarchy_a.level(level).unwrap();
            let graph_b = hierarchy_b.level(level).unwrap();
            assert_eq!(graph_a.record_slot_count(), graph_b.record_slot_count());
            for slot in 0..graph_a.record_slot_count() {
                let zone = crate::sim::pathfinding::zone_map::ZoneId::try_from(slot).unwrap();
                // Record equality includes the retained native+20 threat
                // seed; equal connectivity alone does not prove its input.
                assert_eq!(graph_a.record(zone), graph_b.record(zone));
                assert_eq!(graph_a.edges(zone), graph_b.edges(zone));
            }
        }
        assert_eq!(map_a.zone_count(), map_b.zone_count());
        for y in 0..a.height() {
            for x in 0..a.width() {
                assert_eq!(
                    map_a.zone_at(x, y, MovementLayer::Ground),
                    map_b.zone_at(x, y, MovementLayer::Ground),
                    "ground zone mismatch for {mz:?} at ({x},{y})"
                );
                assert_eq!(
                    map_a.zone_at(x, y, MovementLayer::Bridge),
                    map_b.zone_at(x, y, MovementLayer::Bridge),
                    "bridge zone mismatch for {mz:?} at ({x},{y})"
                );
            }
        }
    }
}

mod header_and_session_tests;
mod house_state_tests;
mod lifecycle_and_occupancy_tests;
mod map_restore_tests;
mod object_state_tests;
mod references_tests;
