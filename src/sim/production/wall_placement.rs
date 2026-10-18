//! Authoritative regular-wall autofill scanning and Overlay Mark publication.
//!
//! Native provenance: `HouseClass::Place_Production @ 0x004FB0E0` admits and
//! consumes the ready product, `FUN_00588750 @ 0x00588750` constructs ordinary
//! fillers, and `OverlayClass::Mark @ 0x005FC570` stamps each overlay. Building
//! Unlimbo and retirement remain with the shared world lifecycle owner.

use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::overlay_grid::{
    WallDamageTransactionHost, refresh_wall_connectivity_after_placement_with_host,
};
use crate::sim::pathfinding::zone_incremental::ZoneRepairKind;
use crate::sim::world::{FrameEffects, Simulation, SimulationWallRuntimeHost};

/// Native regular-wall visit order: north, east, south, west.
const CARDINAL_DIRECTIONS: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// Resolve a BuildingType through its merged ART `ToOverlay=` identity.
pub(crate) fn linked_overlay_id(
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

/// `MapClass588750`: after successful origin Unlimbo, construct every filler
/// nearest-to-click in cardinal order and call its ordinary Building Unlimbo.
/// Both HousePlace4FB29A and BuildingExit445408 reach this same scan.
/// Source: original588750..588952; building_death_anims wall-autofill controls.
pub(crate) fn autofill_walls(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    object_type: &ObjectType,
    origin: (u16, u16),
    owner: InternedId,
    effects: FrameEffects<'_>,
) {
    let Some(overlay_id) = linked_overlay_id(object_type, registry) else {
        return;
    };
    let owner_name = sim.interner.resolve(owner).to_owned();
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
            //5888CE constructs through BuildingType45E880/Building43B740;
            //the shared constructor owns Scenario RNG, IDs and tracking.
            let Some(filler) = sim.construct_object_limbo_at_height(
                &object_type.id,
                &owner_name,
                rx,
                ry,
                0,
                0,
                rules,
                effects,
            ) else {
                continue;
            };
            //58890C ignores failure and keeps that alive-limbo constructor.
            //Requested Z is literal0, independent of the cell's terrain level.
            let _ = sim.reveal_constructed_object_at_coord_with_overlay_context(
                filler,
                crate::sim::components::DriveCoord {
                    x: i32::from(rx).wrapping_mul(256).wrapping_add(128),
                    y: i32::from(ry).wrapping_mul(256).wrapping_add(128),
                    z: 0,
                },
                0,
                crate::sim::world::PlacementEvidence::EvaluateMark,
                rules,
                Some(registry),
                effects,
            );
        }
    }
}

/// Overlay5FC6F4..5FC777's wall arm, after base Mark and slope admission.
/// The caller owns its transient Overlay lifecycle. Building4407FC stamps
/// the placing House only after Overlay Unlimbo succeeds; crate constructors
/// supply discriminator-1, preserving the prior owner here.
/// Source: original5FC570/5FD270; shared wall-autofill/trigger-crate evidence.
pub(crate) fn mark_wall(
    sim: &mut Simulation,
    rules: &RuleSet,
    registry: &OverlayTypeRegistry,
    rx: u16,
    ry: u16,
    overlay_id: u8,
) -> bool {
    let Some(grid) = sim.overlay_grid.as_ref() else {
        return false;
    };
    if rx >= grid.width() || ry >= grid.height() {
        return false;
    }
    // Overlay 0x5FC6F4..0x5FC705 asks the existing Cell 0x47C620 owner with
    // SpeedType 1 (Track), BuildingType NULL and House NULL before any write.
    // A refused old-wall sale must not become an unconditional overwrite.
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return false;
    };
    let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
    if !crate::sim::build_site::is_clear_to_build(
        sim,
        rules,
        Some(registry),
        cells.lookup((rx as i16, ry as i16)),
        Some(crate::rules::locomotor_type::SpeedType::Track),
        None,
        None,
        Some(&cells),
    ) {
        return false;
    }

    //5FC726..742 skips only the explicit anchor navigation pair in scope.
    //PostDestructionWallCleanup retains its own hosted effects below.
    let merge_anchor = !sim.object_placement_scope_active();
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
    mark_wall_transaction(
        grid,
        registry,
        sim.resolved_terrain.as_mut(),
        rx,
        ry,
        overlay_id,
        merge_anchor,
        &mut host,
    )
}

fn mark_wall_transaction(
    grid: &mut crate::sim::overlay_grid::OverlayGrid,
    registry: &OverlayTypeRegistry,
    mut terrain: Option<&mut crate::map::resolved_terrain::ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
    overlay_id: u8,
    merge_anchor: bool,
    host: &mut dyn WallDamageTransactionHost,
) -> bool {
    //5FC717/71E writes only data/type: retained owner and rubble survive.
    let Some(resolved) = terrain.as_deref_mut() else {
        return false;
    };
    if !grid.write_crate_mark_fields(resolved, registry, rx, ry, overlay_id, 0) {
        return false;
    }
    refresh_wall_connectivity_after_placement_with_host(
        grid,
        registry,
        terrain.as_deref_mut(),
        rx,
        ry,
        Some(&mut *host),
    );
    if merge_anchor && let Some(terrain) = terrain.as_deref() {
        host.navigation_step(terrain, (rx, ry), false, ZoneRepairKind::MergeAdjacent);
    }
    grid.add_retained_wall_neighbor_source(terrain.as_deref(), rx, ry);
    if let Some(terrain) = terrain {
        grid.recalculate_runtime_cell(terrain, registry, (rx, ry));
    }
    true
}
