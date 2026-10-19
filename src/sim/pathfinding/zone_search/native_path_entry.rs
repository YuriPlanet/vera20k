// Active42C927..42CB22 entry order and583180/5835D0 projection.
// Evidence: PHASE3_TUBE_HIERARCHY_20260910.md; bounded native prefix oracle.
use std::collections::BTreeSet;

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::MovementZone;
use crate::sim::cell_rect::{
    CellRef, PlayfieldBounds, cell_is_in_playfield_height_aware, get_cellclass_fallback,
};
use crate::sim::pathfinding::zone_build;
use crate::sim::pathfinding::zone_map::ZoneGrid;

pub(super) struct NativePathEntry {
    pub(super) raw_equal: Option<bool>,
    pub(super) goal_bridge: bool,
    pub(super) hierarchy_start: (u16, u16),
    pub(super) hierarchy_goal: (u16, u16),
    pub(super) endpoints_in_playfield: bool,
}

// Projection also reads real Cell.Flags800, outside the narrower1180
// shared stamp facade. The retained dummy authority now includes400/800 from
// fresh-load gap restamping and the associated SetBridgeDirection writers.
fn live_cell_flags(cell: &CellRef<'_>) -> u32 {
    match cell {
        CellRef::Real(cell) => cell.bridge_facts.raw_flags,
        CellRef::Dummy { cell } => cell.retained_bridge_flags(),
    }
}

fn live_cell_coord(cell: &CellRef<'_>) -> (u16, u16) {
    match cell {
        CellRef::Real(cell) => (cell.rx, cell.ry),
        CellRef::Dummy { cell } => {
            let c = cell.snapshot().coord;
            (c.0 as u16, c.1 as u16)
        }
    }
}

fn live_cell_step<'a>(
    terrain: &'a ResolvedTerrainGrid,
    cell: &CellRef<'_>,
    direction: usize,
) -> CellRef<'a> {
    let c = live_cell_coord(cell);
    let d = crate::util::direction::DIRECTION_DELTAS[direction];
    get_cellclass_fallback(
        Some(terrain),
        i32::from((c.0 as i16).wrapping_add(d.0 as i16)),
        i32::from((c.1 as i16).wrapping_add(d.1 as i16)),
    )
}

fn live_cell_state(cell: &CellRef<'_>) -> ((u16, u16), bool) {
    (live_cell_coord(cell), matches!(cell, CellRef::Dummy { .. }))
}

fn live_high_nonrock(terrain: &ResolvedTerrainGrid, cell: &CellRef<'_>) -> bool {
    match cell {
        CellRef::Real(cell) => {
            terrain.high_bridge_tile_offset(cell).is_some()
                && cell.land_type() != crate::rules::terrain_rules::LandType::Rock.as_index()
        }
        // Constructor65535 is outside all hash-bound retail high sets.
        CellRef::Dummy { .. } => false,
    }
}

/// `MapClass::Is_Cell_In_Playfield(c, 1)` (`0x00578460`) for one path
/// search. A fixture without bounds admits every cell.
pub(super) fn live_mode_one(
    terrain: &ResolvedTerrainGrid,
    bounds: Option<PlayfieldBounds>,
    c: (u16, u16),
) -> bool {
    bounds.is_none_or(|b| {
        cell_is_in_playfield_height_aware(
            (i32::from(c.0 as i16), i32::from(c.1 as i16)),
            Some(b),
            Some(terrain),
        )
    })
}

fn live_no_record_projection(
    terrain: &ResolvedTerrainGrid,
    original: &CellRef<'_>,
    bounds: Option<PlayfieldBounds>,
) -> Result<(u16, u16), &'static str> {
    let c = live_cell_coord(original);
    let mut positive =
        get_cellclass_fallback(Some(terrain), i32::from(c.0 as i16), i32::from(c.1 as i16));
    if live_cell_flags(&positive) & 0x100 == 0 {
        return Ok(live_cell_coord(&positive));
    }
    let mut negative = positive.clone();
    let (pd, nd) = if live_cell_flags(&positive) & 0x800 != 0 {
        (2, 6)
    } else {
        (4, 0)
    };
    let mut pc = (0, 0);
    let mut nc = (0, 0);
    let mut visited = BTreeSet::new();
    loop {
        if !visited.insert((live_cell_state(&positive), live_cell_state(&negative))) {
            return Err("cyclic5835D0 walk");
        }
        if live_cell_flags(&positive) & 0x100 != 0 {
            positive = live_cell_step(terrain, &positive, pd);
            if live_cell_flags(&positive) & 0x100 == 0 && live_high_nonrock(terrain, &positive) {
                pc = live_cell_coord(&positive);
            }
        }
        if live_cell_flags(&negative) & 0x100 != 0 {
            negative = live_cell_step(terrain, &negative, nd);
            if live_cell_flags(&negative) & 0x100 == 0 && live_high_nonrock(terrain, &negative) {
                nc = live_cell_coord(&negative);
            }
        }
        if live_cell_flags(&positive) & 0x100 == 0 && live_cell_flags(&negative) & 0x100 == 0 {
            break;
        }
    }
    // Both probes run even for sentinel(0,0). Their dummy writes can change
    // the original retained reference before distance calculation.
    let positive_inside = live_mode_one(terrain, bounds, pc);
    let negative_inside = live_mode_one(terrain, bounds, nc);
    let mut selected = if positive_inside { pc } else { (0, 0) };
    if negative_inside
        && (selected == (0, 0)
            || zone_build::native_packed_cell_distance(live_cell_coord(original), pc)
                > zone_build::native_packed_cell_distance(live_cell_coord(original), nc))
    {
        selected = nc;
    }
    if selected == (0, 0) {
        Err("583180 record[-1] after sentinel fallback")
    } else {
        Ok(selected)
    }
}

pub(crate) fn live_hierarchy_projection(
    terrain: &ResolvedTerrainGrid,
    records: &[crate::sim::bridge_state::BridgeEndpointRecord],
    original: &CellRef<'_>,
    enabled: bool,
    bounds: Option<PlayfieldBounds>,
) -> Result<(u16, u16), &'static str> {
    let c = live_cell_coord(original);
    let flags = live_cell_flags(original);
    if !enabled || flags & 0x100 == 0 {
        return Ok(c);
    }
    let Some(record) = zone_build::find_high_bridge_record(records, 0, c, 2) else {
        return live_no_record_projection(terrain, original, bounds);
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
    let direction = if record.endpoint_a.0 == record.endpoint_b.0 {
        4
    } else {
        2
    };
    let mut current = original.clone();
    let mut visited = BTreeSet::new();
    while live_cell_flags(&current) & 0x100 != 0 {
        if !visited.insert(live_cell_state(&current)) {
            return Err("cyclic583180 inactive walk");
        }
        current = live_cell_step(terrain, &current, direction);
    }
    Ok(project(if live_high_nonrock(terrain, &current) {
        record.endpoint_b
    } else {
        record.endpoint_a
    }))
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
    let lookup = |c: (u16, u16)| {
        get_cellclass_fallback(Some(terrain), i32::from(c.0 as i16), i32::from(c.1 as i16))
    };
    let source_cell = lookup(start); //42C938 retained identity
    let goal_cell = lookup(goal); //42C94B retained identity
    let source_zone =
        zones.and_then(|z| z.get_zone_id_native(terrain, start, movement, start_bridge));
    let query_goal_bridge = live_cell_flags(&lookup(goal)) & 0x100 != 0; //42C9B7
    let goal_zone =
        zones.and_then(|z| z.get_zone_id_native(terrain, goal, movement, query_goal_bridge));
    let records = zones.map_or(&[][..], ZoneGrid::bridge_records);
    let projection = |cell: &CellRef<'_>, enabled| {
        live_hierarchy_projection(terrain, records, cell, enabled, bounds).unwrap_or_else(
            |reason| {
                // Explicit unsupported process-memory/termination domain: keep
                // the current retained coordinate, not a claimed native result.
                log::warn!("unresolved native path entry projection: {reason}");
                live_cell_coord(cell)
            },
        )
    };
    let hierarchy_start = projection(&source_cell, start_bridge);
    let goal_bridge = live_cell_flags(&goal_cell) & 0x100 != 0; //42CA05 after source projection
    let hierarchy_goal = projection(&goal_cell, goal_bridge);
    let endpoints_in_playfield = allow_hierarchy
        && live_mode_one(terrain, bounds, hierarchy_start)
        && live_mode_one(terrain, bounds, hierarchy_goal);
    NativePathEntry {
        raw_equal: source_zone.zip(goal_zone).map(|(a, b)| a == b),
        goal_bridge,
        hierarchy_start,
        hierarchy_goal,
        endpoints_in_playfield,
    }
}
