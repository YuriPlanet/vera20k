//! Foot4DBDF0 navigation coordinates and the active locomotor's retained head.
//! Native witnesses: tools/spatial_oracle/foot_navigation_coordinate.{py,json}.
//! These are read projections; paths, selectors and lifecycle do not own them.

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;

pub(super) const NULL_COORD: DriveCoord = DriveCoord { x: 0, y: 0, z: 0 };

pub(super) fn head_or_current(stored: Option<DriveCoord>, current: DriveCoord) -> DriveCoord {
    stored
        .filter(|coord| *coord != NULL_COORD)
        .unwrap_or(current)
}

/// Physical Object+9C projection: retained XY and the object's world Z,
/// without resampling changed terrain (an object without an exact coordinate
/// uses its stored level plus its altitude source).
pub(super) fn current_coordinate(entity: &GameEntity) -> DriveCoord {
    let [x, y] = super::ground_pose::position_world_xy(&entity.position);
    DriveCoord {
        x,
        y,
        z: super::ground_pose::object_world_z_leptons(entity, None),
    }
}

/// Publish the integer displacement of an existing altitude controller without
/// reinterpreting an exact owner coordinate as its ground baseline. Quantize
/// each endpoint separately: the stored Object coordinate has integer leptons.
pub(super) fn publish_altitude_change(
    position: &mut crate::sim::components::Position,
    previous: crate::util::fixed_math::SimFixed,
    next: crate::util::fixed_math::SimFixed,
) {
    let before = previous.to_num::<i32>();
    let after = next.to_num::<i32>();
    let z = position.exact_z_leptons.map_or_else(
        || {
            super::ground_pose::position_world_coord(position)
                .z
                .wrapping_add(after)
        },
        |z| z.wrapping_sub(before).wrapping_add(after),
    );
    position.exact_z_leptons = Some(z);
}

/// Stored head, before the Head_To null fallback. AtCoord's handoff transform
/// needs these original XY values even when Head_To resolves to current XYZ.
pub(super) fn stored_head(entity: &GameEntity) -> Option<DriveCoord> {
    match entity.locomotor.as_ref()?.active_kind() {
        LocomotorKind::Drive => entity.drive_locomotion.as_ref()?.head_to,
        LocomotorKind::Ship => entity.ship_locomotion.as_ref()?.head_to,
        LocomotorKind::Walk | LocomotorKind::Hover => entity.locomotor.as_ref()?.step_head(),
        _ => None,
    }
}

pub(super) fn navigation_coordinate(
    entity: &GameEntity,
    terrain: Option<&ResolvedTerrainGrid>,
) -> Result<DriveCoord, String> {
    // Foot+684 is tested before dereferencing the locomotor, even on a row
    // awaiting Uninit. Tube+28 is the exit, not the retained transit target.
    if let Some(state) = entity.low_bridge_tube_state {
        let tube = terrain
            .and_then(|grid| grid.tube(state.tube_id))
            .ok_or("Foot coordinate references a missing active TubeClass")?;
        return Ok(DriveCoord {
            x: i32::from(tube.exit.0 as i16) * 256 + 128,
            y: i32::from(tube.exit.1 as i16) * 256 + 128,
            z: 0,
        });
    }
    let loco = entity
        .locomotor
        .as_ref()
        .ok_or("Foot coordinate requires an active locomotor")?;
    let current = current_coordinate(entity);
    match loco.active_kind() {
        LocomotorKind::Drive | LocomotorKind::Ship | LocomotorKind::Walk | LocomotorKind::Hover => {
            if loco.active_kind() == LocomotorKind::Drive && entity.drive_locomotion.is_none()
                || loco.active_kind() == LocomotorKind::Ship && entity.ship_locomotion.is_none()
            {
                return Err("Foot coordinate requires the active track locomotor payload".into());
            }
            Ok(head_or_current(stored_head(entity), current))
        }
        // Fly/Rocket/Teleport share +18/55ACA0: copy linked Object+9C.
        LocomotorKind::Teleport | LocomotorKind::Fly | LocomotorKind::Rocket => Ok(current),
        LocomotorKind::Jumpjet => {
            let state = loco
                .jumpjet_runtime()
                .ok_or("Jumpjet payload does not match active class")?;
            Ok(head_or_current(Some(state.coordinate(current)), current))
        }
    }
}

impl Simulation {
    pub(crate) fn foot_navigation_coordinate(&self, id: u64) -> Result<DriveCoord, String> {
        let entity = self
            .substrate
            .entities
            .get(id)
            .ok_or("Foot coordinate owner disappeared")?;
        navigation_coordinate(entity, self.resolved_terrain.as_ref())
    }
}

#[cfg(test)]
#[path = "foot_coordinate_tests.rs"]
mod tests;
