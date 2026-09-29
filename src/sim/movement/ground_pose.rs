//! Grounded ObjectClass coordinate writes shared by movement and placement.
//!
//! Native FootClass::Set_Height_On_Bridge @ 0x005F5FA0 samples the committed
//! world XY through GetGroundHeight @ 0x00578080, then adds the explicit
//! OnBridge offset. Callers own cadence: Drive/Ship residual movement does
//! not call this setter and retains the last raw coordinate Z.

use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::{NativeCellQuery, ResolvedTerrainGrid};
use crate::sim::components::{DriveCoord, Position};
use crate::sim::pathfinding::PathGrid;
use crate::util::lepton::{
    BRIDGE_HEIGHT_DELTA_LEPTONS, GROUND_LEVEL_HEIGHT_LEPTONS, ground_height_leptons,
};

/// Map578080 through the caller's query identity. Input queries isolate Dummy;
/// simulation callbacks use the canonical retained Dummy and its lookup order.
pub(crate) fn query_ground_height(
    cells: &NativeCellQuery<'_>,
    point: DriveCoord,
) -> Result<i32, String> {
    let cell = cells.lookup_world(point.x, point.y);
    let (level, slope) = cells.ground_fields(cell);
    ground_height_leptons(level, slope, point.x, point.y)
        .map_err(|error| format!("native ground query: {error:?}"))
}

/// Object+1BC receiver5F6960 performs two Map565730 lookups from physical
/// Object+9C. The first lookup is observable when it stamps the shared Dummy;
/// the second returns the identity retained by the caller.
pub(crate) fn query_object_cell(
    cells: &NativeCellQuery<'_>,
    physical: DriveCoord,
) -> NativeCellIdentity {
    let _ = cells.lookup_world(physical.x, physical.y);
    cells.lookup_world(physical.x, physical.y)
}

/// Object5F5F00: signed current-cell level plus four for OnBridge, through
/// the same Object+1BC query used by firing and movement.
pub(crate) fn query_object_cell_height(
    cells: &NativeCellQuery<'_>,
    physical: DriveCoord,
    on_bridge: bool,
) -> i32 {
    let cell = query_object_cell(cells, physical);
    i32::from(cells.ground_fields(cell).0 as i8) + if on_bridge { 4 } else { 0 }
}

/// Foot+BC4DDC40(false) -> Object5F6A70. The navigation coordinate can be a
/// paid head; source bridge selection is independent of the cached path layer.
/// Both ground samples precede the conditional structural-cell lookup.
pub(crate) fn navigation_should_be_on_bridge(
    cells: &NativeCellQuery<'_>,
    navigation: DriveCoord,
    current: DriveCoord,
    on_bridge: bool,
    in_tube: bool,
) -> Result<bool, String> {
    if in_tube {
        return Ok(false);
    }
    let head_ground = query_ground_height(cells, navigation)?;
    let current_ground = query_ground_height(cells, current)?;
    if !on_bridge && current_ground.wrapping_sub(head_ground) > 3 * GROUND_LEVEL_HEIGHT_LEPTONS {
        return Ok(cells.flags(cells.lookup_world(navigation.x, navigation.y)) & 0x100 != 0);
    }
    if on_bridge && head_ground.wrapping_sub(current_ground) > 3 * GROUND_LEVEL_HEIGHT_LEPTONS {
        return Ok(false);
    }
    Ok(on_bridge)
}

pub(crate) fn position_world_xy(position: &Position) -> [i32; 2] {
    [
        i32::from(position.rx)
            .wrapping_mul(256)
            .wrapping_add(position.sub_x.to_num::<i32>()),
        i32::from(position.ry)
            .wrapping_mul(256)
            .wrapping_add(position.sub_y.to_num::<i32>()),
    ]
}

/// Read retained ObjectClass coordinates without resampling changed terrain.
/// Legacy positions without an exact Z use their stored signed level until a
/// real coordinate writer supplies raw leptons.
pub(crate) fn position_world_coord(position: &Position) -> DriveCoord {
    let [x, y] = position_world_xy(position);
    DriveCoord {
        x,
        y,
        z: position.exact_z_leptons.unwrap_or_else(|| {
            i32::from(position.z as i8) * crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS
        }),
    }
}

/// ObjectClass GetCoords Z (virtual +0x48, Object+0xA4) of any object: the
/// one answer to "how high is this object". An exact coordinate a native
/// writer retained is already total world Z. Otherwise VERA still keeps the
/// object's height in parts: the live sloped ground at its XY (Map578080
/// through `ground_height_leptons`), the OnBridge deck, and its one active
/// altitude source ([`object_altitude_leptons`]). Without terrain the stored
/// signed level stands in for the ground. InRange's low-flying snap is its
/// caller's rule, not part of this read.
pub(crate) fn object_world_z_leptons(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
) -> i32 {
    entity.position.exact_z_leptons.unwrap_or_else(|| {
        object_ground_z_leptons(entity, terrain).wrapping_add(object_altitude_leptons(entity))
    })
}

/// The ground an object without an exact coordinate stands on: the live
/// sloped ground at its XY plus the OnBridge deck, or without terrain its
/// stored signed level (which already includes a deck).
pub(crate) fn object_ground_z_leptons(
    entity: &crate::sim::game_entity::GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
) -> i32 {
    let [x, y] = position_world_xy(&entity.position);
    terrain
        .and_then(|terrain| terrain.cell(entity.position.rx, entity.position.ry))
        .and_then(|cell| ground_height_leptons(cell.level, cell.slope_type, x, y).ok())
        .map(|ground| {
            ground.wrapping_add(if entity.on_bridge {
                BRIDGE_HEIGHT_DELTA_LEPTONS as i32
            } else {
                0
            })
        })
        .unwrap_or_else(|| i32::from(entity.position.z as i8) * GROUND_LEVEL_HEIGHT_LEPTONS)
}

/// Height above the ground of an object without an exact coordinate: a
/// parachute's descent height, else a rocket's own flight state (its
/// locomotor keeps only a lagging piggyback copy), else the altitude of an
/// Air-layer locomotor or of an active Hover (which floats on the Ground
/// layer). Any other Ground-layer locomotor never lifts, which keeps a landed
/// or docked aircraft on the floor whatever its stale altitude.
pub(crate) fn object_altitude_leptons(entity: &crate::sim::game_entity::GameEntity) -> i32 {
    if let Some(state) = entity.parachute_state.as_ref() {
        return state.altitude.to_num::<i32>();
    }
    if let Some(state) = entity.rocket_state.as_ref() {
        return state.altitude.to_num::<i32>();
    }
    entity
        .locomotor
        .as_ref()
        .filter(|locomotor| {
            use crate::rules::locomotor_type::LocomotorKind;
            (locomotor.layer == crate::sim::movement::locomotor::MovementLayer::Air
                && locomotor.kind != LocomotorKind::Rocket)
                || locomotor.active_kind() == LocomotorKind::Hover
        })
        .map_or(0, |locomotor| locomotor.altitude.to_num::<i32>())
}

/// Building render-coordinate459EF0 and GetYSort449410's type adjustment.
/// Shared by display registration and presentation; neither uses center coords.
pub(crate) fn building_render_order_parts(
    mut location: DriveCoord,
    turret_anim_is_voxel: bool,
    gate: bool,
) -> (DriveCoord, i32) {
    location.x = location.x.wrapping_sub(128);
    location.y = location.y.wrapping_sub(128);
    (
        location,
        i32::from(turret_anim_is_voxel) * 32 - i32::from(gate) * 16,
    )
}

/// Object virtual+48: Unit/Infantry/Aircraft5F65A0 copy retained XYZ;
/// Building447AC0 adds the foundation-center XY offset and keeps raw Z.
/// This is not Building+4C's optional dock/bunker approach-coordinate owner.
pub(crate) fn object_center_coord(
    entity: &crate::sim::game_entity::GameEntity,
    object_type: &crate::rules::object_type::ObjectType,
) -> DriveCoord {
    object_center_coord_with_foundation(entity, &object_type.foundation)
}

/// The same447AC0 owner for lifecycle callers retaining the immutable
/// foundation key on the entity when a RuleSet is not present.
pub(crate) fn object_center_coord_with_foundation(
    entity: &crate::sim::game_entity::GameEntity,
    foundation: &str,
) -> DriveCoord {
    let mut coord = position_world_coord(&entity.position);
    if entity.category == crate::map::entities::EntityCategory::Structure {
        let (width, height) = crate::rules::foundation::foundation_dimensions(foundation);
        coord.x = coord
            .x
            .wrapping_add(i32::from(width).wrapping_mul(128).wrapping_sub(128));
        coord.y = coord
            .y
            .wrapping_add(i32::from(height).wrapping_mul(128).wrapping_sub(128));
    }
    coord
}

/// Sample the live surface at full world XY. A PathGrid supplies the same
/// level/ramp fields only for callers without resolved terrain. Missing
/// headless terrain leaves the caller's existing coordinate authoritative.
pub(crate) fn ground_surface_z_at(
    world_xy: [i32; 2],
    on_bridge: bool,
    terrain: Option<&ResolvedTerrainGrid>,
    path_grid: Option<&PathGrid>,
) -> Option<i32> {
    let rx = (world_xy[0] / 256) as i16;
    let ry = (world_xy[1] / 256) as i16;
    let (level, slope) = if let Some(terrain) = terrain {
        // Map578080 ->565730 forms the wrapping fixed-stride index before
        // narrowing fallback coordinates. Keep the common lookup owner.
        let cells = NativeCellQuery::canonical(terrain);
        cells.ground_fields(cells.lookup_world(world_xy[0], world_xy[1]))
    } else {
        let cell = path_grid?.cell(rx as u16, ry as u16)?;
        (cell.ground_level, cell.slope_type)
    };
    let ground = match ground_height_leptons(level, slope, world_xy[0], world_xy[1]) {
        Ok(ground) => ground,
        Err(_) => {
            log::warn!(
                "ground pose at {world_xy:?} has unsupported slope {slope}; retaining raw Z"
            );
            return None;
        }
    };
    Some(ground.wrapping_add(if on_bridge {
        BRIDGE_HEIGHT_DELTA_LEPTONS as i32
    } else {
        0
    }))
}

pub(crate) fn commit_ground_height(
    position: &mut Position,
    on_bridge: bool,
    terrain: Option<&ResolvedTerrainGrid>,
    path_grid: Option<&PathGrid>,
) -> bool {
    let Some(z) = ground_surface_z_at(position_world_xy(position), on_bridge, terrain, path_grid)
    else {
        return false;
    };
    position.exact_z_leptons = Some(z);
    true
}
