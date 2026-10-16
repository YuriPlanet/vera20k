//! Drive/Ship outer Process prefix before fresh Process_Movement: the
//! same-cell/Guard stops, the retained turn observation (complete class+62)
//! and the mission/NavCom/zone gates.
//! Native: Drive4B066C..4B0A69, Ship69FD13..6A0131 (twins, R1); executable
//! entry evidence for the turn part in tools/spatial_oracle/track_process_entry.
//! Live Facing readiness is deliberately independent of this retained state.

use super::track_process::TrackFamily;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::sim::world::{FrameEffects, Simulation};

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
    let Some(loco) = entity.locomotor.as_mut() else {
        return false;
    };
    let Some(family) = TrackFamily::from_kind(loco.active_kind()) else {
        return false;
    };
    let Some(mut track) = loco.track_progress(family) else {
        return false;
    };
    let valid = loco.track_valid(family).unwrap();
    let latched = loco.track_turn_latched(family).unwrap();
    let admitted = ((valid && track.turn_index() != -1) || queue_eight) && (!latched || has_turret);
    if !admitted {
        track.clear_residual();
        loco.store_track_progress(family, track);
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
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let frame = self.session.binary_frame;
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        if entity.death.is_dying() || !entity.lifecycle.object_alive() {
            return false;
        }
        if super::track_head::active_track_family(entity).is_some() {
            return true;
        }
        if !entity
            .locomotor
            .as_ref()
            .is_some_and(|l| matches!(l.active_kind(), LocomotorKind::Drive | LocomotorKind::Ship))
        {
            return true;
        }
        // Drive4B066C..4B06D2 / Ship69FD13..69FD79: a Cell NavCom naming the
        // current cell stops (or takes the next waypoint) and Process returns
        // before the turn observation, Process_Movement and Process_Track.
        if matches!(entity.navigation.nav_com(), Some(NavTargetRef::Cell { rx, ry })
            if (rx, ry) == (entity.position.rx(), entity.position.ry()))
        {
            self.track_navcom_stop(id, rules, effects);
            return false;
        }
        let current = super::ground_pose::position_world_coord(&entity.position);
        let loco = entity.locomotor.as_mut().unwrap();
        let family = TrackFamily::from_kind(loco.active_kind()).unwrap();
        loco.ensure_installed_track_state();
        let mut latched = loco.track_turn_latched(family).unwrap();
        let valid = loco.track_valid(family).unwrap();
        let destination = loco.track_destination(family);
        // Drive4B06D5..4B0772 / Ship69FD7F..69FE1C: Guard (current +AC = 5)
        // with no valid track at its non-null exact destination takes the
        // same stop/waypoint pair, before Facing sampling.
        if entity.mission.current().known() == Some(MissionType::Guard)
            && !valid
            && destination.is_some_and(|dest| dest == current)
        {
            self.track_navcom_stop(id, rules, effects);
            return false;
        }
        let rotating = entity.body_facing.is_rotating(frame);
        let completed = sample(&mut latched, rotating);
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .store_track_turn_latched(family, latched);
        // Drive4B0788 ->4B078C..893 / Ship69FE35: live rotation
        // returns through the Process tail, never fresh ProcessMovement.
        // The common resting-speed tail remains to be migrated.
        if rotating {
            return false;
        }
        if completed {
            self.unit_per_cell_process(
                id,
                super::per_cell::PerCellReason::TurnComplete,
                rules,
                registry,
                effects,
            );
            // Native reloads these three bytes after the synchronous callback.
            if !self.substrate.entities.get(id).is_some_and(|e| {
                e.lifecycle.object_alive() && !e.lifecycle.in_limbo() && !e.is_falling_down()
            }) {
                return false;
            }
        }
        self.track_movement_gates(id, rules, effects)
    }

    /// Drive4B08D1..4B0A69 / Ship69FF98..6A0131: the outer Process gates
    /// between the turn observation and fresh Process_Movement. True admits
    /// Process_Movement (4B0A79 / 6A0142); false is the Process tail, which
    /// skips both Process_Movement and Process_Track.
    fn track_movement_gates(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
        effects: FrameEffects<'_>,
    ) -> bool {
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
            if let Some(target) = entity.navigation.nav_com()
                && let Ok(coord) = super::navcom::nav_target_coordinate(
                    target,
                    Some(id),
                    &self.substrate.entities,
                    self.resolved_terrain.as_ref(),
                    rules.map(|rules| (rules, &self.interner)),
                )
            {
                let terrain = self.resolved_terrain.as_ref();
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    super::navcom::track_move_to(entity, coord, terrain, self.session.binary_frame);
                }
            }
            return false;
        }
        // 4B09AB..4B0A68: an in-playfield (+3D5), non-Enter mover whose
        // destination fails the zone precheck clears its head, then stops or
        // takes the next waypoint; either way the tail follows.
        if entity.in_playfield() && mission != Some(MissionType::Enter) && moving {
            let destination = super::track_path::track_destination_or_null(entity);
            let Some(rules) = rules else {
                return true;
            };
            // Unrepresented precheck inputs (no zone topology or bounds in a
            // component fixture) admit Process_Movement unchanged.
            if let Ok(false) = self.foot_path_zone_precheck(id, destination, rules) {
                self.track_zone_drop(id, rules, effects);
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
#[path = "track_turn_tests.rs"]
mod tests;
