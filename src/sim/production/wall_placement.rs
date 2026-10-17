//! Authoritative regular-wall autofill scanning and stamping.
//!
//! Native provenance: `HouseClass::Place_Production @ 0x004FB0E0` admits and
//! consumes the ready product, `FUN_00588750 @ 0x00588750` scans and commits
//! ordinary fillers, and `OverlayClass::Mark @ 0x005FC570` stamps each overlay.

use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::overlay_grid::{
    WallDamageTransactionHost, refresh_wall_connectivity_after_placement_with_host,
};
use crate::sim::pathfinding::zone_incremental::ZoneRepairKind;
use crate::sim::world::{Simulation, SimulationWallRuntimeHost};

/// Native regular-wall visit order: north, east, south, west.
const CARDINAL_DIRECTIONS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// Resolve a BuildingType through its merged ART `ToOverlay=` identity.
pub(super) fn linked_overlay_id(
    object_type: &ObjectType,
    registry: &OverlayTypeRegistry,
) -> Option<u8> {
    let overlay_id = registry.id_for_name(object_type.to_overlay.as_deref()?)?;
    registry
        .flags(overlay_id)
        .is_some_and(|flags| flags.wall)
        .then_some(overlay_id)
}

/// Scan one regular-wall direction.
///
/// `FUN_00588750 @ 0x00588750` checks a same-ToOverlay, same-owner endpoint
/// before asking whether the visited cell can accept a filler
/// (`CellClass::Is_Clear_To_Build` for the wall type and owner, `0x0058886E`).
/// Any blocker discards the whole direction; a found endpoint returns the gap
/// in nearest-to-click order. Rust stores GuardRange in I16F16 cells, so
/// integer conversion is the equivalent of native's signed fixed-point shift
/// by 8.
fn scan_autofill_direction(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    object_type: &ObjectType,
    origin: (u16, u16),
    owner: InternedId,
    overlay_id: u8,
    direction: (i32, i32),
    native_cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> Vec<(u16, u16)> {
    let limit = object_type
        .guard_range
        .map(|range| range.to_num::<i32>())
        .unwrap_or(0)
        .max(0) as usize;
    if limit == 0 {
        return Vec::new();
    }
    let (Some(grid), Some(terrain)) = (sim.overlay_grid.as_ref(), sim.resolved_terrain.as_ref())
    else {
        return Vec::new();
    };
    let canonical = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    let query = native_cells.unwrap_or(&canonical);
    let (width, height) = (i32::from(grid.width()), i32::from(grid.height()));
    let (mut cx, mut cy) = (
        i32::from(origin.0) + direction.0,
        i32::from(origin.1) + direction.1,
    );
    let mut gap = Vec::with_capacity(limit.saturating_sub(1));

    while gap.len() < limit {
        if cx < 0 || cy < 0 || cx >= width || cy >= height {
            return Vec::new();
        }
        let cell_coord = (cx as u16, cy as u16);
        let cell = *grid.cell(cell_coord.0, cell_coord.1);
        if cell.overlay_id == Some(overlay_id) && cell.wall_owner == Some(owner) {
            return gap;
        }
        if !crate::sim::build_site::is_clear_to_build(
            sim,
            rules,
            Some(registry),
            query.lookup((cx as i16, cy as i16)),
            crate::sim::build_site::building_speed_type(object_type),
            Some(object_type),
            Some(owner),
            Some(query),
        ) {
            return Vec::new();
        }
        gap.push(cell_coord);
        cx += direction.0;
        cy += direction.1;
    }
    Vec::new()
}

pub(super) fn autofill_cells(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    object_type: &ObjectType,
    origin: (u16, u16),
    owner: InternedId,
    overlay_id: u8,
    native_cells: Option<&crate::map::resolved_terrain::NativeCellQuery<'_>>,
) -> Vec<(u16, u16)> {
    let mut cells = Vec::new();
    for direction in CARDINAL_DIRECTIONS {
        cells.extend(scan_autofill_direction(
            sim,
            rules,
            registry,
            object_type,
            origin,
            owner,
            overlay_id,
            direction,
            native_cells,
        ));
    }
    cells
}

/// A wall BuildingType placed at `origin` for `owner`: `BuildingClass::Unlimbo`
/// stamps its `ToOverlay=` wall (`0x00440774..0x00440865`), then the placer
/// fills towards the owner's walls in reach (`0x00588750`), for the PLACE
/// event (`HouseClass::Place_Production`) and the computer's building exit
/// (`0x00445408`) alike. The caller has admitted the site and deletes the
/// constructed object after.
pub(crate) fn stamp_wall_with_autofill(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    object_type: &ObjectType,
    origin: (u16, u16),
    owner: InternedId,
) -> bool {
    let Some(overlay_id) = linked_overlay_id(object_type, registry) else {
        return false;
    };
    if !stamp_wall(sim, registry, origin.0, origin.1, overlay_id, owner) {
        return false;
    }
    for direction in CARDINAL_DIRECTIONS {
        let gap = scan_autofill_direction(
            sim,
            rules,
            registry,
            object_type,
            origin,
            owner,
            overlay_id,
            direction,
            None,
        );
        for (rx, ry) in gap {
            let stamped = stamp_wall(sim, registry, rx, ry, overlay_id, owner);
            debug_assert!(stamped, "scanned wall filler must remain stampable");
        }
    }
    true
}

/// Stamp one wall cell and synchronously publish its passability projection.
pub(super) fn stamp_wall(
    sim: &mut Simulation,
    registry: &OverlayTypeRegistry,
    rx: u16,
    ry: u16,
    overlay_id: u8,
    owner: InternedId,
) -> bool {
    let Some(grid) = sim.overlay_grid.as_ref() else {
        return false;
    };
    if rx >= grid.width() || ry >= grid.height() {
        return false;
    }

    #[cfg(any(test, feature = "test-support"))]
    let mut detach_trace = Vec::new();
    let mut host = SimulationWallRuntimeHost {
        entities: &mut sim.substrate.entities,
        target_terrain_objects: &sim.production.terrain_objects,
        #[cfg(any(test, feature = "test-support"))]
        detach_trace: &mut detach_trace,
        radar_dirty_cells: &mut sim.radar_terrain_dirty_cells,
        radar_dirty_generation: &mut sim.radar_terrain_dirty_generation,
        tactical_dirty_cells: &mut sim.tactical_dirty_cells,
        zone_grid: &mut sim.zone_grid,
        path_grid: &mut sim.path_grid,
        bridge_state: sim.bridge_state.as_ref(),
        playfield_bounds: sim.playfield_bounds,
    };
    let grid = sim.overlay_grid.as_mut().expect("validated wall grid");
    stamp_wall_transaction(
        grid,
        registry,
        sim.resolved_terrain.as_mut(),
        rx,
        ry,
        overlay_id,
        owner,
        &mut host,
    );
    true
}

fn stamp_wall_transaction(
    grid: &mut crate::sim::overlay_grid::OverlayGrid,
    registry: &OverlayTypeRegistry,
    mut terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
    overlay_id: u8,
    owner: InternedId,
    host: &mut dyn WallDamageTransactionHost,
) {
    // OverlayClass::Mark @ 0x005FC570 makes the pending owner visible only
    // after hosted cleanup and its explicit runtime Merge/graph step.
    grid.stamp_wall_identity(rx, ry, overlay_id, 0);
    refresh_wall_connectivity_after_placement_with_host(
        grid,
        registry,
        terrain.as_deref_mut(),
        rx,
        ry,
        Some(&mut *host),
    );
    if let Some(terrain) = terrain.as_deref() {
        host.navigation_step(terrain, (rx, ry), false, ZoneRepairKind::MergeAdjacent);
    }
    grid.set_wall_owner(rx, ry, owner);
    grid.add_retained_wall_neighbor_source(terrain.as_deref(), rx, ry);
    if let Some(terrain) = terrain {
        grid.recalculate_runtime_cell(terrain, registry, (rx, ry));
    }
}
