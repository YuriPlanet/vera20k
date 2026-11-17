//! Committed Drive/Ship head coordinates.
//!
//! Fresh Drive4B32AF/4B40B0 and Ship6A28FF/6A36E0 add path directions to
//! current Foot XYZ. Chain4B1BC4/6A120A instead adds to the previous head.
//! Original executable cases: tools/spatial_oracle/locomotor_head_coordinates.json.

use super::locomotor::LocomotorState;
use super::track_process::TrackFamily;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::util::direction_tables::lepton::LEPTON_DELTAS;

/// Drive4AFB80 / Ship69F290 IsMoving and their readiness head-presence input.
/// A destination is sufficient; without one, a non-null head must differ from
/// physical owner X/Y (Z is ignored). Read the selected instance even when a
/// caller is testing a suspended Drive. Orders, power and track cursors are
/// not inputs. Original comparisons: locomotor_moving.{py,json,meta.json}.
pub(crate) fn motion_state(entity: &GameEntity, family: TrackFamily) -> (bool, bool) {
    let (destination, head) = entity.locomotor.as_ref().map_or((None, None), |loco| {
        (loco.track_destination(family), loco.track_head(family))
    });
    let nonnull = |c: &DriveCoord| c.x != 0 || c.y != 0 || c.z != 0;
    let head = head.filter(nonnull);
    let current = super::ground_pose::position_world_xy(&entity.position);
    (
        destination.is_some_and(|c| nonnull(&c)) || head.is_some_and(|c| [c.x, c.y] != current),
        head.is_some(),
    )
}

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
pub(super) fn accept_fresh_progress(loco: &mut LocomotorState, turn_index: usize) {
    let Some(mut progress) = select_fresh_progress(loco, turn_index) else {
        return;
    };
    progress.accept_fresh();
    let family = TrackFamily::from_kind(loco.active_kind()).unwrap();
    loco.store_track_progress(family, progress);
    // Drive ProcessMovement4B46C5 publishes +63 before the accepted head
    // and Apply1, even when this invocation cannot pay a point.
    loco.store_track_valid(family, true);
}

/// Drive ProcessMovement4B4016..4B4034 / Ship twin: the turn table selector
/// (+58) and the reversed byte (+60 = 0), written before the crate question
/// and the second candidate, so every later arm of the same call sees them.
pub(super) fn select_fresh_progress(
    loco: &mut LocomotorState,
    turn_index: usize,
) -> Option<crate::sim::components::TrackProgress> {
    let family = TrackFamily::from_kind(loco.active_kind())?;
    loco.ensure_installed_track_state();
    let mut progress = loco.track_progress(family)?;
    progress.select_fresh(turn_index);
    loco.store_track_progress(family, progress);
    Some(progress)
}

/// Outer Process admission (Drive4B055A..576, mirrored Ship): the class
/// valid byte and selector are authoritative, independent of head coordinates.
/// This is distinct from querying a non-null committed coordinate below.
pub(super) fn active_track_family(
    entity: &GameEntity,
) -> Option<super::track_process::TrackFamily> {
    let loco = entity.locomotor.as_ref()?;
    let family = TrackFamily::from_kind(loco.active_kind())?;
    let valid = loco.track_valid(family)?;
    let selector = loco.track_progress(family)?.turn_index();
    (valid && selector != -1).then_some(family)
}

/// The active Drive/Ship selector and retained head identify a committed segment.
/// Native callbacks may clear the head while leaving a selector installed.
pub(crate) fn committed_track_head(entity: &GameEntity) -> Option<DriveCoord> {
    let loco = entity.locomotor.as_ref()?;
    let family = TrackFamily::from_kind(loco.active_kind())?;
    let head = loco.track_head(family)?;
    let track = loco.track_progress(family)?;
    (track.turn_index() >= 0).then_some(head)
}

#[cfg(all(test, tests_sim_movement))]
#[path = "track_head_tests.rs"]
mod tests;
