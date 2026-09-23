//! Drive/Ship outer Process prefix before fresh Process_Movement: the
//! same-cell/Guard stops, the retained turn observation (complete class+62)
//! and the mission/NavCom/zone gates.
//! Native: Drive4B066C..4B0A69, Ship69FD13..6A0131 (twins, R1); executable
//! entry evidence for the turn part in tools/spatial_oracle/track_process_entry.
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
        if !entity
            .locomotor
            .as_ref()
            .is_some_and(|l| matches!(l.kind, LocomotorKind::Drive | LocomotorKind::Ship))
        {
            return true;
        }
        // A Rust route adapter (direct move, component fixture) keeps its
        // pre-existing lane: none of the gates below reads its route.
        let adapter = super::movement_tick::adapter_route_pending(entity);
        // Drive4B066C..4B06D2 / Ship69FD13..69FD79: a Cell NavCom naming the
        // current cell stops (or takes the next waypoint) and Process returns
        // before the turn observation, Process_Movement and Process_Track.
        if matches!(entity.navigation.nav_com, Some(NavTargetRef::Cell { rx, ry })
            if (rx, ry) == (entity.position.rx, entity.position.ry))
        {
            if adapter {
                return true;
            }
            self.track_navcom_stop(id, rules);
            return false;
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
        // Drive4B06D5..4B0772 / Ship69FD7F..69FE1C: Guard (current +AC = 5)
        // with no valid track at its non-null exact destination takes the
        // same stop/waypoint pair, before Facing sampling.
        if entity.mission.current().known() == Some(MissionType::Guard)
            && !valid
            && destination.is_some_and(|dest| dest == current)
        {
            if adapter {
                return true;
            }
            self.track_navcom_stop(id, rules);
            return false;
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
        if completed {
            self.unit_track_per_cell(id, PerCellReason::TurnComplete, rules, grid);
            // Native reloads these three bytes after the synchronous callback.
            if !self.substrate.entities.get(id).is_some_and(|e| {
                e.lifecycle.object_alive && !e.lifecycle.in_limbo && e.object_is_falling_down == 0
            }) {
                return false;
            }
        }
        adapter || self.track_movement_gates(id, rules)
    }

    /// Drive4B08D1..4B0A69 / Ship69FF98..6A0131: the outer Process gates
    /// between the turn observation and fresh Process_Movement. True admits
    /// Process_Movement (4B0A79 / 6A0142); false is the Process tail, which
    /// skips both Process_Movement and Process_Track.
    fn track_movement_gates(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        let moving = super::motion_query::is_moving(entity).unwrap_or(false);
        let mission = entity.mission.effective().known();
        // 4B08D1..4B08FD: Guard while not moving, or Unload, take the tail.
        if (mission == Some(MissionType::Guard) && !moving) || mission == Some(MissionType::Unload)
        {
            return false;
        }
        if !moving
            && entity
                .navigation
                .path_replay
                .remaining_directions()
                .is_empty()
        {
            // 4B0921..4B096C: the +3CD (sinking/crashing) arm has no Rust
            // producer. 4B0971..4B09A6: a NavCom re-enters the locomotor's
            // own Move_To (+0x44) with its live coordinate; either way the
            // tail follows and Process_Movement waits for the next visit.
            if let Some(target) = entity.navigation.nav_com
                && let Ok(coord) = super::navcom::nav_target_coordinate(
                    target,
                    Some(id),
                    &self.substrate.entities,
                    self.resolved_terrain.as_ref(),
                    rules.map(|rules| (rules, &self.interner)),
                )
            {
                let speed = self.resolve_move_info(id, rules).map(|info| info.speed);
                let terrain = self.resolved_terrain.as_ref();
                if let Some(entity) = self.substrate.entities.get_mut(id)
                    && super::navcom::track_move_to(entity, coord, terrain)
                    && entity.movement_target.is_none()
                    && let Some(speed) = speed
                {
                    // Schedule the next visit's Process_Movement; the adapter
                    // carries no route.
                    super::movement_commands::schedule_track_process(
                        entity,
                        ((coord.x / 256) as u16, (coord.y / 256) as u16),
                        speed,
                    );
                }
            }
            return false;
        }
        // 4B09AB..4B0A68: an in-playfield (+3D5), non-Enter mover whose
        // destination fails the zone precheck clears its head, then stops or
        // takes the next waypoint; either way the tail follows.
        if entity.in_playfield && mission != Some(MissionType::Enter) && moving {
            let destination = match entity.locomotor.as_ref().map(|l| l.kind) {
                Some(LocomotorKind::Drive) => {
                    entity.drive_locomotion.as_ref().and_then(|d| d.destination)
                }
                Some(LocomotorKind::Ship) => {
                    entity.ship_locomotion.as_ref().and_then(|s| s.destination)
                }
                _ => None,
            }
            .unwrap_or(crate::sim::components::DriveCoord { x: 0, y: 0, z: 0 });
            let Some(rules) = rules else {
                return true;
            };
            // Unrepresented precheck inputs (no zone topology or bounds in a
            // component fixture) admit Process_Movement unchanged.
            if let Ok(false) = self.foot_path_zone_precheck(id, destination, rules) {
                self.track_zone_drop(id, rules);
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
#[path = "track_turn_tests.rs"]
mod tests;
