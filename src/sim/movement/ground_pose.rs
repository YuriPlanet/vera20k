//! Grounded ObjectClass coordinate writes shared by movement and placement.
//!
//! Native FootClass::Set_Height_On_Bridge @ 0x005F5FA0 samples the committed
//! world XY through GetGroundHeight @ 0x00578080, then adds the explicit
//! OnBridge offset. Callers own cadence: Drive/Ship residual movement does
//! not call this setter and retains the last raw coordinate Z.

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
        if let Some(index) = terrain.native_fixed_cell_index(rx, ry) {
            let cell = &terrain.cells()[index];
            (cell.level, cell.slope_type)
        } else {
            // GetGroundHeight @ 0x578080 evaluates the shared dummy's live
            // fields too. Its default zero height is not an invariant.
            let shared = terrain.shared_cell_dummy();
            shared.stamp_coord(i32::from(rx), i32::from(ry));
            let dummy = shared.snapshot();
            (dummy.level as u8, dummy.slope_type)
        }
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
