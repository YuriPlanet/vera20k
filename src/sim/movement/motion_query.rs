//! Active ILocomotion+10 queries over existing retained state.
//!
//! This is distinct from +80 IsMovingNow. Unsupported payloads return None;
//! callers must keep their remaining adapter limits explicit. No order/path
//! presence, movement phase or speed is substituted for a native query here.
use super::track_process::TrackFamily;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;

/// Drive4AFB80, Ship69F290, Walk75AB30, Fly4CCA90 and Jumpjet54AE50.
/// Evidence: locomotor_moving and air_locomotor_moving native corpora.
pub(crate) fn is_moving(entity: &GameEntity) -> Option<bool> {
    let locomotor = entity.locomotor.as_ref()?;
    match locomotor.active_kind() {
        LocomotorKind::Drive => Some(super::track_head::motion_state(entity, TrackFamily::Drive).0),
        LocomotorKind::Ship => Some(super::track_head::motion_state(entity, TrackFamily::Ship).0),
        LocomotorKind::Walk => locomotor.walk_is_moving(),
        LocomotorKind::Fly => locomotor
            .fly_runtime()
            .map(|state| state.moving() || entity.flight_attitude.blocks_landing()),
        LocomotorKind::Jumpjet => locomotor.jumpjet_runtime().map(|state| state.moving),
        // Teleport Is_Moving 0x718080 reads the +0x30 request byte: Move_To
        // raises it and the warp's Stop_Moving (0x00719725) clears it, so it
        // is up only for an armed warp not yet processed.
        LocomotorKind::Teleport => {
            Some(entity.teleport_state.as_ref().is_some_and(|state| {
                state.phase == super::teleport_movement::TeleportPhase::Relocate
            }))
        }
        // Hover/Rocket destination storage still requires its native
        // producer/lifecycle migration.
        _ => None,
    }
}

#[cfg(test)]
#[path = "motion_query_tests.rs"]
pub(crate) mod tests;
