//! Authoritative regular-wall autofill scanning.
//!
//! Native provenance: `HouseClass::Place_Production @ 0x004FB0E0` admits and
//! consumes the ready product and `FUN_00588750 @ 0x00588750` constructs
//! ordinary fillers. Each filler's Overlay reaches the one
//! `OverlayClass::Mark @ 0x005FC570` port ([`crate::map::overlay_mark`])
//! through Building Unlimbo, which the shared world lifecycle owner keeps.

use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::intern::InternedId;
use crate::sim::world::{FrameEffects, Simulation};

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
            None,
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
