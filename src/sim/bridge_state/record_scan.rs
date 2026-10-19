//! Ordered bridge-record production: gamemd 0x0056D6E0 and CellIterator
//! 0x00578350/0x00578290. Evidence: PHASE3_CELL_ITERATION_BRIDGE_RECORDS_20260910.md.

use super::*;
use crate::map::authored_overlay::NativeOverlayMapShape;
use crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
use crate::map::resolved_terrain::ResolvedTerrainCell;
use crate::sim::cell_rect::{CellRef, get_cellclass_fallback};

pub(super) fn compute_bridge_endpoints(
    terrain: &ResolvedTerrainGrid,
    size: Option<(i32, i32)>,
) -> Vec<BridgeEndpointRecord> {
    let mut records = Vec::new();
    let starts: Vec<_> = if let Some((width, _)) = size {
        terrain.native_cell_iterator(width).collect()
    } else {
        // Explicit synthetic test seam: rectangular fixtures have no Map Size.
        let mut cells: Vec<_> = terrain.iter().collect();
        cells.sort_by_key(|cell| (u32::from(cell.rx) + u32::from(cell.ry), cell.rx));
        cells
    };
    let map_width = size.map_or(i32::from(terrain.width()), |s| s.0);
    for start in starts {
        if let Some(offset) = terrain.high_bridge_tile_offset(start) {
            // The high-tile branch has priority, even when its subtile rejects.
            if HIGH_BRIDGE_START_SUBTILE[offset] != i32::from(start.final_sub_tile) {
                continue;
            }
            let Ok(direction) = u8::try_from(HIGH_BRIDGE_WALK_DIRECTION[offset]) else {
                continue;
            };
            if let Some(record) = high_record(terrain, size, start, direction) {
                records.push(record);
            }
        } else if is_tube(terrain, start) {
            // Exact short-circuit order matters to the shared dummy coordinate.
            let opposite = step_is_tube(terrain, start, 2) && step_is_tube(terrain, start, 6)
                || step_is_tube(terrain, start, 4) && step_is_tube(terrain, start, 0);
            if !opposite {
                continue;
            }
            let tube = terrain
                .tube_at_cell(start.rx, start.ry)
                .expect("validated tube");
            let a = (start.rx, start.ry);
            if cell_ordinal(a, map_width) < cell_ordinal(tube.exit, map_width) {
                records.push(BridgeEndpointRecord {
                    endpoint_a: a,
                    endpoint_b: tube.exit,
                    // Tube records are always active and independent of a
                    // structural bridge damage group, native kind +0xC = 1.
                    active: true,
                    bridge_kind: BridgeRecordKind::Low,
                });
            }
        }
    }
    records
}

fn is_tube(terrain: &ResolvedTerrainGrid, cell: &ResolvedTerrainCell) -> bool {
    cell.land_type() == crate::rules::terrain_rules::LandType::Tunnel.as_index()
        && terrain.tube_at_cell(cell.rx, cell.ry).is_some()
}

fn step<'a>(terrain: &'a ResolvedTerrainGrid, coord: (i32, i32), direction: u8) -> CellRef<'a> {
    let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[usize::from(direction & 7)];
    get_cellclass_fallback(
        Some(terrain),
        coord.0.wrapping_add(dx),
        coord.1.wrapping_add(dy),
    )
}

fn coord(cell: &CellRef<'_>) -> (i32, i32) {
    match cell {
        CellRef::Real(cell) => (i32::from(cell.rx as i16), i32::from(cell.ry as i16)),
        CellRef::Dummy { cell } => cell.snapshot().coord,
    }
}

fn step_is_tube(terrain: &ResolvedTerrainGrid, cell: &ResolvedTerrainCell, direction: u8) -> bool {
    match step(
        terrain,
        (i32::from(cell.rx as i16), i32::from(cell.ry as i16)),
        direction,
    ) {
        CellRef::Real(cell) => is_tube(terrain, cell),
        CellRef::Dummy { .. } => false,
    }
}

/// `FUN_0042B1C0`: signed packed words and wrapping dword arithmetic.
fn cell_ordinal((x, y): (u16, u16), width: i32) -> i32 {
    let (x, y) = (i32::from(x as i16), i32::from(y as i16));
    x.wrapping_sub(width)
        .wrapping_sub(1)
        .wrapping_add(y)
        .wrapping_mul(width)
        .wrapping_add(x.wrapping_sub(y).wrapping_sub(1).wrapping_add(width) >> 1)
}

fn high_record(
    terrain: &ResolvedTerrainGrid,
    size: Option<(i32, i32)>,
    start: &ResolvedTerrainCell,
    direction: u8,
) -> Option<BridgeEndpointRecord> {
    let a = (start.rx, start.ry);
    let mut cursor = (i32::from(start.rx as i16), i32::from(start.ry as i16));
    let mut far_match = false;
    let mut intact = true;
    loop {
        let next = step(terrain, cursor, direction);
        cursor = coord(&next);
        let inside = size.map_or_else(
            || matches!(next, CellRef::Real(_)),
            |(w, h)| NativeOverlayMapShape::new(w, h).admits(cursor.0 as i16, cursor.1 as i16),
        );
        if far_match {
            // Native commits even if the next probe left Size, then uses the
            // real backward helper (which can overwrite the shared dummy).
            let previous = step(terrain, cursor, direction.wrapping_sub(4) & 7);
            let b = coord(&previous);
            return Some(BridgeEndpointRecord {
                endpoint_a: a,
                endpoint_b: (b.0 as u16, b.1 as u16),
                active: intact,
                bridge_kind: BridgeRecordKind::High,
            });
        }
        if !inside {
            return None;
        }
        match next {
            CellRef::Real(cell) => {
                if let Some(offset) = terrain.high_bridge_tile_offset(cell) {
                    far_match = HIGH_BRIDGE_END_SUBTILE[offset] == i32::from(cell.final_sub_tile);
                } else if !cell.bridge_facts.has_structural_bridge() {
                    intact = false;
                }
            }
            CellRef::Dummy { cell } => {
                // Dummy tile identity is the native no-tile sentinel. Its
                // independently retained structural bit is a real reader.
                if cell.snapshot().bridge_flags_0x1180 & BRIDGE_FLAG_STRUCTURAL == 0 {
                    intact = false;
                }
            }
        }
    }
}
