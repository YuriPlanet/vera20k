//! The Drive/Ship outer Process after Process_Track(0) returns false for an
//! active track (Drive 0x4B0583..0x4B0667, Ship 0x69FC93..0x69FD0E; twins
//! except the Drive-only re-aim). In the SAME Process call the host
//! (`world::object_turn`) runs:
//! 1. [`Simulation::begin_track_end_continuation`]: the gates 0x4B0583..
//!    0x4B05CA (live owner; selector retired; Is_Moving or a Foot+5E0 head;
//!    no Unit+6D1 unload latch), then the Drive Unit->Infantry NavCom re-aim
//!    0x4B05D0..0x4B063B. A class setter Rust deferred is finished before
//!    the gates read +34 (natively it ran before this Process);
//! 2. Process_Movement(&out, 1, 0) (0x4B0647 / 0x69FCEE) through the pending
//!    pass (`MoverReentry::AfterTrackEnd`);
//! 3. Process_Track(1) (0x4B0AAA / 0x6A0173): its budget is the retained
//!    residual alone (`TrackInvocation::retry`, 0x4B127A / 0x6A0942).
//!
//! A true Process_Track return (the terminal abort, `TrackPass::aborted`)
//! returns the whole Process first; the out byte or a dead owner skip the
//! Process_Track(1).
//!
//! Evidence: tools/spatial_oracle/track_outer_continuation (after-active
//! rows: gates, argument 1, the Drive re-aim and its Ship contrast; the
//! Process_Movement, Process_Track, Is_Moving and NavCom callees supplied)
//! and track_speed_native (argument-1 budget rows).

use super::track_process::TrackFamily;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

impl Simulation {
    /// 0x4B0583..0x4B063B / 0x69FC93..0x69FCDA after Process_Track(0)
    /// returned false. True: Process_Movement(&out, 1, 0) follows in this
    /// Process.
    pub(crate) fn begin_track_end_continuation(
        &mut self,
        id: u64,
        family: TrackFamily,
        rules: Option<&RuleSet>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        // 4B0583..4B058C: a dead owner returns the Process.
        if !entity.lifecycle.object_alive() {
            return Ok(false);
        }
        // 4B0592: a selector still set keeps its track (common tail 4B078C).
        let selector = entity
            .locomotor
            .as_ref()
            .and_then(|loco| loco.track_progress(family))
            .map(|p| p.turn_index());
        if selector.is_some_and(|selector| selector != -1) {
            return Ok(false);
        }
        // Rust bookkeeping: a class setter Rust deferred during this Process
        // (a mission restore's `Assign_Destination(saved, 1)` from a
        // callback) ran natively before these gates, so +34 and the path
        // head already follow NavCom when they read them.
        self.complete_pending_order(id, rules, None, frame_effects);
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(false);
        };
        // 4B059B..4B05AE: Is_Moving (4AFB80 / 69F290) or a Foot+5E0 head.
        if !super::motion_query::is_moving(entity).unwrap_or(false)
            && entity
                .navigation
                .path_replay
                .remaining_directions()
                .is_empty()
        {
            return Ok(false);
        }
        // 4B05B4..4B05CA: a Unit's +6D1 unload-active latch.
        if entity.category() == EntityCategory::Unit
            && entity
                .miner
                .as_ref()
                .is_some_and(|miner| miner.unload_active())
        {
            return Ok(false);
        }
        if family == TrackFamily::Drive {
            self.reaim_drive_infantry_navcom(id, rules)?;
        }
        Ok(true)
    }

    /// Drive 0x4B05D0..0x4B063B (no Ship twin): a Unit whose NavCom is an
    /// Infantry object (What_Am_I 0xF at 0x4B05F3)
    /// asks it for its coordinate (vt+0x4C with the Foot as requester) and
    /// calls the locomotor Move_To only when that differs from +34.
    fn reaim_drive_infantry_navcom(
        &mut self,
        id: u64,
        rules: Option<&RuleSet>,
    ) -> Result<(), String> {
        let entities = &self.substrate.entities;
        let Some(entity) = entities.get(id) else {
            return Ok(());
        };
        let target = entity.navigation.nav_com().filter(|target| {
            entity.category() == EntityCategory::Unit
                && match *target {
                    NavTargetRef::Cell { .. } => false,
                    NavTargetRef::Object { id } => entities
                        .get(id)
                        .is_some_and(|object| object.category() == EntityCategory::Infantry),
                }
        });
        let Some(target) = target else {
            return Ok(());
        };
        let coord = super::navcom::nav_target_coordinate(
            target,
            Some(id),
            self,
            self.resolved_terrain.as_ref(),
            rules.map(|rules| (rules, &self.interner)),
        )?;
        let terrain = self.resolved_terrain.as_ref();
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            super::navcom::refresh_drive_destination_coord(
                entity,
                coord,
                terrain,
                self.session.binary_frame(),
            );
        }
        Ok(())
    }
}
