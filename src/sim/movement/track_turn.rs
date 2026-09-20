//! Drive/Ship Process's retained turn observation (complete class+62).
//! Native: Drive4B0775..4B08C9, Ship69FE22..69FF90; executable entry
//! evidence in tools/spatial_oracle/track_process_entry.{py,json}.
//! Live Facing readiness is deliberately independent of this retained state.

use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PerCellReason {
    TurnComplete,
    Arrival,
}

pub(super) fn sample(latched: &mut bool, rotating: bool) -> bool {
    let completed = *latched && !rotating;
    *latched = rotating;
    completed
}

/// TrackProcess4B0F26..63 /6A05FC..633 returns before its scalar prefix,
/// clearing +4C, unless the descriptor (or Foot queue8) and turn gates admit.
/// The world TrackProcess entry calls this once before the scalar prefix.
pub(super) fn admit_track_entry(entity: &mut GameEntity, has_turret: bool) -> bool {
    let queue_eight = entity.navigation.path_replay.remaining_directions().first() == Some(&8);
    let (valid, latched, track) = match entity.locomotor.as_ref().map(|l| l.kind) {
        Some(LocomotorKind::Drive) => {
            let Some(state) = entity.drive_locomotion.as_mut() else {
                return false;
            };
            (state.track_valid, state.turn_latched, &mut state.track)
        }
        Some(LocomotorKind::Ship) => {
            let Some(state) = entity.ship_locomotion.as_mut() else {
                return false;
            };
            (state.track_valid, state.turn_latched, &mut state.track)
        }
        _ => return false,
    };
    let admitted = ((valid && track.turn_index != -1) || queue_eight) && (!latched || has_turret);
    if !admitted {
        track.residual = 0;
    }
    admitted
}

impl Simulation {
    /// Called by the actual ground Process owner, including synchronous calls.
    /// Active-track dispatch bypasses this sampler even if that call finishes
    /// the track; its native continuation goes to the later movement/tail arm.
    pub(crate) fn process_track_turn(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        grid: Option<&PathGrid>,
    ) -> bool {
        let frame = self.session.binary_frame;
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if entity.dying || !entity.lifecycle.object_alive {
            return false;
        }
        if entity.low_bridge_tube_state.is_some()
            || super::track_head::active_track_family(entity).is_some()
        {
            return true;
        }
        // Drive4B066C..6D2 / Ship69FD7C: same-cell NavCom's stop/waypoint
        // continuation returns before the turn observation.
        if matches!(entity.navigation.nav_com, Some(NavTargetRef::Cell { rx, ry })
            if (rx, ry) == (entity.position.rx, entity.position.ry))
        {
            return true;
        }
        let current = super::ground_pose::position_world_coord(&entity.position);
        let (latched, valid, destination) = match entity.locomotor.as_ref().map(|l| l.kind) {
            Some(LocomotorKind::Drive) => {
                let state = entity.drive_locomotion.get_or_insert_with(Default::default);
                (
                    &mut state.turn_latched,
                    state.track_valid,
                    state.destination,
                )
            }
            Some(LocomotorKind::Ship) => {
                let state = entity.ship_locomotion.get_or_insert_with(Default::default);
                (
                    &mut state.turn_latched,
                    state.track_valid,
                    state.destination,
                )
            }
            _ => return true,
        };
        // Drive4B06D5..772: Guard (native mission5) at its non-null exact destination also
        // reaches Stop/waypoint completion before Facing sampling.
        if entity.mission.current().known() == Some(MissionType::Guard)
            && !valid
            && destination.is_some_and(|dest| dest == current)
        {
            return true;
        }
        let rotating = entity
            .body_facing
            .as_ref()
            .is_some_and(|f| f.is_rotating(frame));
        let completed = sample(latched, rotating);
        if let Some(body) = entity.body_facing.as_ref() {
            entity.facing = (body.current(frame) >> 8) as u8;
        }
        // Drive4B0788 ->4B078C..893 / Ship69FE35: live rotation
        // returns through the Process tail, never fresh ProcessMovement.
        // A display-facing target is not required for the native timer to own
        // this visit. The common resting-speed tail remains to be migrated.
        if rotating {
            return false;
        }
        if !completed {
            return true;
        }
        self.unit_track_per_cell(id, PerCellReason::TurnComplete, rules, grid);
        // Native reloads these three bytes after the synchronous callback.
        self.substrate.entities.get(id).is_some_and(|e| {
            e.lifecycle.object_alive && !e.lifecycle.in_limbo && e.object_is_falling_down == 0
        })
    }
}

#[cfg(test)]
#[path = "track_turn_tests.rs"]
mod tests;
