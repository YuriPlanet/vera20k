//! Committed Drive/Ship head coordinates.
//!
//! Fresh Drive4B32AF/4B40B0 and Ship6A28FF/6A36E0 add path directions to
//! current Foot XYZ. Chain4B1BC4/6A120A instead adds to the previous head.
//! Original executable cases: tools/spatial_oracle/locomotor_head_coordinates.json.

use super::drive_track::{self, DriveTrackPlan};
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{DriveCoord, DriveLocomotionRuntime, Position, ShipLocomotionRuntime};
use crate::sim::game_entity::GameEntity;
use crate::util::direction_tables::lepton::LEPTON_DELTAS;

/// One original direction-table addition; no terrain or cell-center sampling.
pub(super) fn offset_head(base: DriveCoord, direction: u8) -> DriveCoord {
    let (dx, dy) = LEPTON_DELTAS[usize::from(direction & 7)];
    DriveCoord {
        x: base.x.wrapping_add(dx),
        y: base.y.wrapping_add(dy),
        z: base.z,
    }
}

/// Publish the selected descriptor and cursor on the active locomotor,
/// preserving its residual. The immutable tables project its coordinates.
pub(super) fn accept_fresh_progress(
    kind: LocomotorKind,
    drive: &mut Option<DriveLocomotionRuntime>,
    ship: &mut Option<ShipLocomotionRuntime>,
    turn_index: usize,
) {
    use super::track_process::TrackFamily;
    let (progress, family) = match kind {
        LocomotorKind::Drive => {
            let state = drive.get_or_insert_with(Default::default);
            (&mut state.track, TrackFamily::Drive)
        }
        LocomotorKind::Ship => {
            let state = ship.get_or_insert_with(Default::default);
            (&mut state.track, TrackFamily::Ship)
        }
        _ => return,
    };
    assert!(progress.select_fresh(family, (turn_index / 8) as u8, (turn_index % 8) as u8));
    progress.accept_fresh();
    // Drive ProcessMovement4B46C5 publishes +63 before the accepted head
    // and Apply1, even when this invocation cannot pay a point.
    match kind {
        LocomotorKind::Drive => drive.as_mut().unwrap().track_valid = true,
        LocomotorKind::Ship => ship.as_mut().unwrap().track_valid = true,
        _ => unreachable!(),
    }
}

/// Outer Process admission (Drive4B055A..576, mirrored Ship): the class
/// valid byte and selector are authoritative, independent of head coordinates.
/// This is distinct from querying a non-null committed coordinate below.
pub(super) fn active_track_family(
    entity: &GameEntity,
) -> Option<super::track_process::TrackFamily> {
    use super::track_process::TrackFamily;
    let (family, valid, selector) = match entity.locomotor.as_ref()?.kind {
        LocomotorKind::Drive => {
            let state = entity.drive_locomotion.as_ref()?;
            (
                TrackFamily::Drive,
                state.track_valid,
                state.track.turn_index,
            )
        }
        LocomotorKind::Ship => {
            let state = entity.ship_locomotion.as_ref()?;
            (TrackFamily::Ship, state.track_valid, state.track.turn_index)
        }
        _ => return None,
    };
    (valid && selector != -1).then_some(family)
}

/// The active Drive/Ship selector and retained head identify a committed segment.
/// Native callbacks may clear the head while leaving a selector installed.
pub(crate) fn committed_track_head(entity: &GameEntity) -> Option<DriveCoord> {
    let (head, track) = match entity.locomotor.as_ref()?.kind {
        LocomotorKind::Drive => {
            let state = entity.drive_locomotion.as_ref()?;
            (state.head_to?, state.track)
        }
        LocomotorKind::Ship => {
            let state = entity.ship_locomotion.as_ref()?;
            (state.head_to?, state.track)
        }
        _ => return None,
    };
    (track.turn_index >= 0).then_some(head)
}

/// Validate the immutable raw table and form the accepted head from exact Foot
/// XYZ, including noncentered subcells. Fresh progress is published separately.
pub(super) fn begin_fresh(plan: &DriveTrackPlan, position: &Position) -> Option<DriveCoord> {
    if drive_track::raw_track_points(plan.selection.raw_track_index).is_empty() {
        return None;
    }
    let current = super::ground_pose::position_world_coord(position);
    let from = (plan.selection.turn_track_index / 8) as u8;
    let mut head = offset_head(current, from);
    if plan.nodes == 2 {
        head = offset_head(head, (plan.selection.turn_track_index % 8) as u8);
    }
    Some(head)
}

#[cfg(test)]
#[path = "track_head_tests.rs"]
mod tests;
