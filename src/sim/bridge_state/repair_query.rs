//! Map587410 Engineer hut admission. Evidence: bridge_repair_query.{py,json}
//! and physical anytown_damage/hut_cursor. Cell queries retain allocation
//! identities; BridgeRuntimeState supplies the sole live record vector.

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::zone_build::find_high_bridge_record;

use super::{BridgeEndpointRecord, ordinary, ramp_repair::Family};

// Original image tables82AA04/82AA24/82AA44, separately pinned in the corpus.
const OFFSET_X: [i16; 16] = [1, 1, 0, 2, 2, 2, 0, 0, 0, 0, 0, 2, 2, 2, 2, 2];
const OFFSET_Y: [i16; 16] = [2, 2, 2, 1, 1, 0, 2, 2, 2, 2, 2, 0, 0, 0, 0, 0];
const DIRECTIONS: [i32; 16] = [2, 2, 6, 4, 4, 0, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1];

#[derive(Clone, Copy)]
enum Selection {
    Tile {
        cell: NativeCellIdentity,
        offset: usize,
    },
    Overlay {
        cell: NativeCellIdentity,
        family: Family,
    },
}

/// CellClass+44 of a cell `query` looked up: OverlayGrid's for a real cell,
/// the query's own fallback for the dummy.
fn overlay_identity(
    query: &NativeCellQuery<'_>,
    overlays: Option<&OverlayGrid>,
    cell: NativeCellIdentity,
) -> i32 {
    match cell {
        NativeCellIdentity::Real(_) => {
            let (x, y) = query.coord(cell);
            overlays
                .and_then(|grid| grid.cell(x as u16, y as u16).overlay_id)
                .map_or(-1, i32::from)
        }
        NativeCellIdentity::Dummy => query.dummy_overlay_identity(),
    }
}

/// Entire587410 Boolean (AL), including Y-major last-match selection and
/// tile-before-overlay precedence. No RNG draws, timers, detach or rebuild.
pub(crate) fn can_repair(
    query: &NativeCellQuery<'_>,
    overlays: Option<&OverlayGrid>,
    records: &[BridgeEndpointRecord],
    center: (i16, i16),
) -> bool {
    let terrain = query.terrain();
    let mut selected = None;
    for dy in -2i16..=2 {
        for dx in -2i16..=2 {
            let coord = (center.0.wrapping_add(dx), center.1.wrapping_add(dy));
            let cell = query.lookup(coord);
            let tile = terrain.native_cell_tile_index(cell);
            //587483 tests wood first; record construction has a DIFFERENT
            //concrete-first predicate. Never reuse that selection here.
            let offset = [
                terrain.wood_bridge_set_base(),
                terrain.concrete_bridge_set_base(),
            ]
            .into_iter()
            .find_map(|base| {
                (tile >= base && tile < base.wrapping_add(16))
                    .then(|| tile.wrapping_sub(base) as usize)
            });
            if let Some(offset) = offset {
                selected = Some(Selection::Tile {
                    cell: query.lookup(coord),
                    offset,
                });
                continue;
            }
            // Original performs these repeated GetCell calls even on misses.
            let cell = query.lookup(coord);
            if ordinary::member(overlay_identity(query, overlays, cell), Family::Low) {
                selected = Some(Selection::Overlay {
                    cell: query.lookup(coord),
                    family: Family::Low,
                });
                continue;
            }
            let cell = query.lookup(coord);
            if ordinary::member(overlay_identity(query, overlays, cell), Family::High) {
                selected = Some(Selection::Overlay {
                    cell: query.lookup(coord),
                    family: Family::High,
                });
            }
        }
    }
    match selected {
        None => false,
        Some(Selection::Tile { cell, offset }) => walk_records(query, records, cell, offset),
        Some(Selection::Overlay { cell, family }) => walk_overlays(query, overlays, cell, family),
    }
}

fn walk_records(
    query: &NativeCellQuery<'_>,
    records: &[BridgeEndpointRecord],
    cell: NativeCellIdentity,
    offset: usize,
) -> bool {
    let terrain = query.terrain();
    let Ok((width, _)) = terrain.current_tile_dimensions(terrain.native_cell_tile_index(cell))
    else {
        // Missing asset metadata cannot establish a native tile origin.
        return false;
    };
    assert_ne!(width, 0, "registered bridge template has zero width");
    let sub = i16::from(terrain.native_cell_sub_tile(cell));
    let mut endpoint = query.coord(cell);
    endpoint.0 = endpoint
        .0
        .wrapping_sub(sub % i16::from(width))
        .wrapping_add(OFFSET_X[offset]);
    endpoint.1 = endpoint
        .1
        .wrapping_sub(sub / i16::from(width))
        .wrapping_add(OFFSET_Y[offset]);
    let direction = DIRECTIONS[offset];
    if direction == -1 {
        return false;
    }
    let mut coord = endpoint;
    while let Some(record) =
        find_high_bridge_record(records, 0, (coord.0 as u16, coord.1 as u16), 3)
    {
        if !record.active {
            return true;
        }
        let current = (endpoint.0 as u16, endpoint.1 as u16);
        endpoint = if current == record.endpoint_a {
            (record.endpoint_b.0 as i16, record.endpoint_b.1 as i16)
        } else if current == record.endpoint_b {
            (record.endpoint_a.0 as i16, record.endpoint_a.1 as i16)
        } else {
            // The tolerance search can select a span whose endpoint differs
            // from the retained endpoint. Original5877BB/5877C1 returns true.
            return true;
        };
        coord = super::rim::step(endpoint, direction as u8);
    }
    false
}

fn walk_overlays(
    query: &NativeCellQuery<'_>,
    overlays: Option<&OverlayGrid>,
    seed: NativeCellIdentity,
    family: Family,
) -> bool {
    let overlay = overlay_identity(query, overlays, seed);
    let along_x = ordinary::axis(overlay, family) == Some(super::Axis::NS);
    let first = if along_x { 6 } else { 4 };
    for direction in [first, (first + 4) & 7] {
        //5878A4/5879A6 etc read the retained seed AFTER the first pass;
        //a fallback seed has moved with all intervening lookups.
        let mut coord = query.coord(seed);
        loop {
            let cell = query.lookup(coord);
            if !ordinary::member(overlay_identity(query, overlays, cell), family) {
                break;
            }
            let cell = query.lookup(coord);
            let overlay = overlay_identity(query, overlays, cell);
            let terminals = match family {
                Family::Low => [100, 101],
                Family::High => [231, 232],
            };
            if terminals.contains(&overlay) {
                return true;
            }
            coord = super::rim::step(coord, direction);
        }
    }
    false
}

#[cfg(test)]
#[path = "repair_query_tests.rs"]
mod tests;
