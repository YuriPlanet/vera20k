// Active42C927..42CB22 entry order and583180/5833F0/5835D0 projection.
// Evidence: PHASE3_TUBE_HIERARCHY_20260910.md; bounded native prefix oracle.
use std::collections::BTreeSet;

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::rules::locomotor_type::MovementZone;
use crate::sim::cell_rect::{
    CellRef, PlayfieldBounds, cell_is_in_playfield_height_aware_in_query, native_cell_ref,
};
use crate::sim::pathfinding::zone_build;
use crate::sim::pathfinding::zone_map::{ZoneGrid, ZoneQueryCell};

pub(super) struct NativePathEntry {
    pub(super) raw_equal: Option<bool>,
    pub(super) goal_bridge: bool,
    pub(super) hierarchy_start: (u16, u16),
    pub(super) hierarchy_goal: (u16, u16),
    pub(super) endpoints_in_playfield: bool,
}

fn packed_coord((x, y): (i16, i16)) -> (u16, u16) {
    (x as u16, y as u16)
}

fn live_cell_coord(cells: &NativeCellQuery<'_>, cell: NativeCellIdentity) -> (u16, u16) {
    packed_coord(cells.coord(cell))
}

fn live_cell_state(cells: &NativeCellQuery<'_>, cell: NativeCellIdentity) -> ((u16, u16), bool) {
    (
        live_cell_coord(cells, cell),
        cell == NativeCellIdentity::Dummy,
    )
}

fn live_high_nonrock(cells: &NativeCellQuery<'_>, cell: NativeCellIdentity) -> bool {
    match native_cell_ref(cells, cell) {
        CellRef::Real(cell) => {
            cells.terrain().high_bridge_tile_offset(cell).is_some()
                && cell.land_type != crate::rules::terrain_rules::LandType::Rock.as_index()
        }
        // Constructor65535 is outside all retail high bridge sets.
        CellRef::Dummy { .. } => false,
    }
}

/// `MapClass::Is_Cell_In_Playfield(c, 1)` (`0x00578460`) for one path
/// search. A fixture without bounds admits every cell.
pub(super) fn live_mode_one(
    cells: &NativeCellQuery<'_>,
    bounds: Option<PlayfieldBounds>,
    c: (u16, u16),
) -> bool {
    bounds.is_none_or(|b| {
        cell_is_in_playfield_height_aware_in_query(
            (i32::from(c.0 as i16), i32::from(c.1 as i16)),
            Some(b),
            Some(cells.terrain()),
            Some(cells),
        )
    })
}

/// Original `MapClass::FindClosestBridgeExit @ 0x005835D0`. Its two
/// CellStruct pointers are independent: either can be a copied coordinate or
/// a retained CellClass+24 that follows this query's Dummy writes. A successful
/// `(0,0)` means neither exit was admitted; callers apply their own fallback.
pub(crate) fn closest_bridge_exit(
    cells: &NativeCellQuery<'_>,
    at: ZoneQueryCell,
    reference: ZoneQueryCell,
    bounds: Option<PlayfieldBounds>,
) -> Result<(u16, u16), &'static str> {
    let mut positive = cells.lookup(at.coord(cells));
    if cells.flags(positive) & 0x100 == 0 {
        return Ok(live_cell_coord(cells, positive));
    }
    let mut negative = positive;
    let (pd, nd) = if cells.flags(positive) & 0x800 != 0 {
        (2, 6)
    } else {
        (4, 0)
    };
    let mut pc = (0, 0);
    let mut nc = (0, 0);
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert((
            live_cell_state(cells, positive),
            live_cell_state(cells, negative),
        )) {
            return Err("cyclic5835D0 walk");
        }
        if cells.flags(positive) & 0x100 != 0 {
            positive = cells.neighbor(positive, pd);
            if cells.flags(positive) & 0x100 == 0 && live_high_nonrock(cells, positive) {
                pc = live_cell_coord(cells, positive);
            }
        }
        if cells.flags(negative) & 0x100 != 0 {
            negative = cells.neighbor(negative, nd);
            if cells.flags(negative) & 0x100 == 0 && live_high_nonrock(cells, negative) {
                nc = live_cell_coord(cells, negative);
            }
        }
        if cells.flags(positive) & 0x100 == 0 && cells.flags(negative) & 0x100 == 0 {
            break;
        }
    }
    // Both probes run even for sentinel(0,0). Their dummy writes can change
    // the original retained reference before distance calculation.
    let positive_inside = live_mode_one(cells, bounds, pc);
    let negative_inside = live_mode_one(cells, bounds, nc);
    let mut selected = if positive_inside { pc } else { (0, 0) };
    if negative_inside {
        let negative_is_closer = selected == (0, 0) || {
            //58376C reads the reference only after both membership probes.
            let reference = packed_coord(reference.coord(cells));
            let negative_distance = zone_build::native_packed_cell_distance(nc, reference);
            let positive_distance = zone_build::native_packed_cell_distance(pc, reference);
            negative_distance < positive_distance
        };
        if negative_is_closer {
            selected = nc;
        }
    }
    Ok(selected)
}

// Both inactive-record branches (583180 and5833F0) walk the same east/south
// axis with Cell481810, then select B only for a nonrock high bridge exit.
fn inactive_record_endpoint(
    cells: &NativeCellQuery<'_>,
    record: &crate::sim::bridge_state::BridgeEndpointRecord,
    mut current: NativeCellIdentity,
) -> Option<(u16, u16)> {
    let direction = if record.endpoint_a.0 == record.endpoint_b.0 {
        4
    } else {
        2
    };
    let mut visited = BTreeSet::new();
    while cells.flags(current) & 0x100 != 0 {
        if !visited.insert(live_cell_state(cells, current)) {
            return None;
        }
        current = cells.neighbor(current, direction);
    }
    Some(if live_high_nonrock(cells, current) {
        record.endpoint_b
    } else {
        record.endpoint_a
    })
}

/// Original `MapClass::ResolveBridgeEndpointTowardCell @ 0x005833F0`.
/// Searches at tolerance one without a structural/enabled gate, and measures
/// unshifted endpoints from the independent reference. A tie selects B.
pub(crate) fn resolve_bridge_endpoint(
    cells: &NativeCellQuery<'_>,
    records: &[crate::sim::bridge_state::BridgeEndpointRecord],
    at: ZoneQueryCell,
    reference: ZoneQueryCell,
    bounds: Option<PlayfieldBounds>,
) -> Result<(u16, u16), &'static str> {
    let c = packed_coord(at.coord(cells));
    let Some(record) = zone_build::find_high_bridge_record(records, 0, c, 1) else {
        let exit = closest_bridge_exit(cells, at, reference, bounds)?;
        //5835BC re-reads *at after5835D0; a retained Dummy may have changed.
        return Ok(if exit == (0, 0) {
            packed_coord(at.coord(cells))
        } else {
            exit
        });
    };
    if record.active {
        let reference = packed_coord(reference.coord(cells));
        return Ok(
            if zone_build::native_packed_cell_distance(reference, record.endpoint_a)
                < zone_build::native_packed_cell_distance(reference, record.endpoint_b)
            {
                record.endpoint_a
            } else {
                record.endpoint_b
            },
        );
    }
    let current = cells.lookup(at.coord(cells));
    inactive_record_endpoint(cells, record, current).ok_or("cyclic5833F0 inactive walk")
}

/// Original `MapClass::ResolvePathCoord_BridgeAware @ 0x00583180`.
pub(crate) fn live_hierarchy_projection(
    cells: &NativeCellQuery<'_>,
    records: &[crate::sim::bridge_state::BridgeEndpointRecord],
    original: NativeCellIdentity,
    enabled: bool,
    bounds: Option<PlayfieldBounds>,
) -> Result<(u16, u16), &'static str> {
    let c = live_cell_coord(cells, original);
    let flags = cells.flags(original);
    if !enabled || flags & 0x100 == 0 {
        return Ok(c);
    }
    let Some(record) = zone_build::find_high_bridge_record(records, 0, c, 2) else {
        let at = ZoneQueryCell::Retained(original);
        let exit = closest_bridge_exit(cells, at, at, bounds)?;
        return if exit == (0, 0) {
            //5831F8 would index record[-1], unlike5833F0's input fallback.
            Err("583180 record[-1] after sentinel fallback")
        } else {
            Ok(exit)
        };
    };
    let offset = if flags & 0x800 != 0 {
        (0, c.1.wrapping_sub(record.endpoint_a.1))
    } else {
        (c.0.wrapping_sub(record.endpoint_a.0), 0)
    };
    let project = |e: (u16, u16)| (e.0.wrapping_add(offset.0), e.1.wrapping_add(offset.1));
    if record.active {
        let a = project(record.endpoint_a);
        let b = project(record.endpoint_b);
        return Ok(
            if zone_build::native_packed_cell_distance(c, a)
                < zone_build::native_packed_cell_distance(c, b)
            {
                a
            } else {
                b
            },
        );
    }
    let endpoint =
        inactive_record_endpoint(cells, record, original).ok_or("cyclic583180 inactive walk")?;
    Ok(project(endpoint))
}

pub(super) fn prepare_native_path_entry(
    zones: Option<&ZoneGrid>,
    terrain: Option<&ResolvedTerrainGrid>,
    movement: MovementZone,
    start: (u16, u16),
    start_bridge: bool,
    goal: (u16, u16),
    allow_hierarchy: bool,
    bounds: Option<PlayfieldBounds>,
) -> NativePathEntry {
    let Some(terrain) = terrain else {
        return NativePathEntry {
            raw_equal: None,
            goal_bridge: false,
            hierarchy_start: start,
            hierarchy_goal: goal,
            endpoints_in_playfield: allow_hierarchy && bounds.is_none(),
        };
    };
    let cells = NativeCellQuery::canonical(terrain);
    let lookup = |c: (u16, u16)| cells.lookup((c.0 as i16, c.1 as i16));
    let source_cell = lookup(start); //42C938 retained identity
    let goal_cell = lookup(goal); //42C94B retained identity
    let source_zone =
        zones.and_then(|z| z.get_zone_id_native(terrain, start, movement, start_bridge));
    let query_goal_bridge = cells.flags(lookup(goal)) & 0x100 != 0; //42C9B7
    let goal_zone =
        zones.and_then(|z| z.get_zone_id_native(terrain, goal, movement, query_goal_bridge));
    let records = zones.map_or(&[][..], ZoneGrid::bridge_records);
    let projection = |cell: NativeCellIdentity, enabled| {
        live_hierarchy_projection(&cells, records, cell, enabled, bounds).unwrap_or_else(|reason| {
            // Explicit unsupported process-memory/termination domain: keep
            // the current retained coordinate, not a claimed native result.
            log::warn!("unresolved native path entry projection: {reason}");
            live_cell_coord(&cells, cell)
        })
    };
    let hierarchy_start = projection(source_cell, start_bridge);
    let goal_bridge = cells.flags(goal_cell) & 0x100 != 0; //42CA05 after source projection
    let hierarchy_goal = projection(goal_cell, goal_bridge);
    let endpoints_in_playfield = allow_hierarchy
        && live_mode_one(&cells, bounds, hierarchy_start)
        && live_mode_one(&cells, bounds, hierarchy_goal);
    NativePathEntry {
        raw_equal: source_zone.zip(goal_zone).map(|(a, b)| a == b),
        goal_bridge,
        hierarchy_start,
        hierarchy_goal,
        endpoints_in_playfield,
    }
}
