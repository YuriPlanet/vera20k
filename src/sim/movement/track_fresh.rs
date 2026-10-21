//! Drive/Ship `Process_Movement` (Drive `0x004B2630`, Ship `0x006A1C80`):
//! the prologue gates, the NavCom path trim and the fresh arm from the path
//! head (`0x4B3298`) through the finalize tail (`0x4B460C`), with the
//! recursive calls its retry and refusal arms make. The two bodies are
//! instruction twins in these arms; each Drive address below has a Ship twin.
//! The no-queue path request and its two continuations stay in `track_path`;
//! a found route re-enters the fresh arm in the same call (`0x4B3282`).
//!
//! Callers: the outer Process (`0x4B0A79`) and the track-end continuation
//! (`0x4B0647`) call Process_Movement(&out, 1, 0) and ignore its AL; only the
//! out byte (Foot gone, or an idle Move's Enter_Idle_Mode answer) ends the
//! Process. So this port returns the out byte and reproduces effects only.
//!
//! The first candidate is asked through Mark0 / Unit+1AC / Mark1
//! (`0x4B34AE..0x4B34D1`); the second candidate of a turning track is asked
//! without the Mark bracket (`0x4B4120`). Both use the native Unit
//! `Can_Enter_Cell` port ([`Simulation::foot_can_enter`]); the dispatch on
//! the coerced code is [`super::track_fresh_dispatch`]
//! (`tools/spatial_oracle/track_fresh_admission`), the code-2 timers are
//! `tools/spatial_oracle/track_blocked_timers`.
//!
//! Residuals:
//! - `vt+0x37C` (Unit `0x746C90`: the EMP lock Techno+504 or the Unit
//!   death-frame counter +6D8): neither has a Rust producer (VERA has no EMP
//!   mechanism; see `techno_ai_cloak`), so the prologue gate reads false.
//!   Trigger: an EMP'd or dying Unit. Effect: it could start a track native
//!   refuses. Frequency: no stock EMP source is proven reachable. Risk: this
//!   gate is a consumer of the EMP system when it lands.
//! - Crates (`CellClass::PickupCrate 0x481A00` at `0x4B405D` and
//!   `0x4B46E6`) run through [`Simulation::pickup_crate_at`]; a host called
//!   without the OverlayType table answers as a cell without a crate (the
//!   native early exit `0x481A39`). Production always binds it.
//! - Terrain blockers retain the actual Object target through the shared
//!   code-4/5 Override arm (4B3B8D..4B3BE9). The ordinary route must first
//!   admit that cell; a blocked zone can reject a destination before this arm.
//! - Ship's distinct +64 constructor/fresh writers and reader6A103A remain
//!   unported. Drive's +64 wall/CrusherAll pre-entry paths live in `track_host`.
//! - A candidate at a negative cell coordinate publishes no target speed
//!   (`publish_fresh_speed`); native reads the shared dummy Cell's land row.
//!   Trigger: a Foot on map row or column 0 heading off the map. Frequency:
//!   none in play (the playfield keeps movers inside). Risk: none.
//! - A train (`IsTrain=`, Type+C94, set by no retail type) extends its last
//!   path word with Find_Path's append form (`0x4B3F07`), which keeps the
//!   queue and appends at most 24 - prefix words (`0x4D3E82`). VERA's route
//!   install replaces the queue, so a train takes the ordinary request.
//!
//! A tube word at the path head returns here (`0x4B3298`, and `0x4B2F45`
//! after a found route); the Process_Track after it enters the tube
//! ([`Simulation::track_tube_head`]).

use super::foot_path::{FindPathResult, FootPathOutcome, coord_cell};
use super::ground_pose;
use super::infantry_entry::InfantryEntryArgs;
use super::movement_tick::FootPathRequest;
use super::track_fresh_dispatch::{
    FreshDispatch, FreshStage, coerce_entry_code, dispatch_entry, normalize_second_direction,
};
use super::track_path::{clear_track_head, track_destination, track_destination_or_null};
use super::track_process::TrackFamily;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::MovementZone;
use crate::rules::mission_data::MissionType;
use crate::rules::object_type::Ability;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::native_xyz_distance;
use crate::sim::combat::TargetKind;
use crate::sim::combat::veterancy::has_weapon_ability;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::scenario_session::ScenarioSession;
use crate::sim::world::FrameEffects;
use crate::sim::world::ObjectSubstrate;
use crate::sim::world::Simulation;
use crate::util::direction::TUBE_STEP_DIRECTION;
use crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;

/// Arguments 2 and 3 of `Process_Movement(&out, arg2, arg3)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProcessMovementArgs {
    /// Arg2: a refused first candidate may drop the head and recurse once.
    pub allow_retry: bool,
    /// Arg3: select the straight track even when the path turns.
    pub force_single: bool,
}

impl ProcessMovementArgs {
    /// The outer Process and its track-end continuation (0x4B0A79, 0x4B0647).
    pub(crate) const OUTER: Self = Self {
        allow_retry: true,
        force_single: false,
    };
    /// Every retry recursion passes (0, 0) (0x4B397F, 0x4B4480, 0x4B4552).
    const RETRY: Self = Self {
        allow_retry: false,
        force_single: false,
    };
}

/// Mark0/Mark1 and the per-call services the arm reaches.
#[derive(Clone, Copy)]
struct FreshCall<'a> {
    id: u64,
    family: TrackFamily,
    args: ProcessMovementArgs,
    rules: &'a RuleSet,
    registry: Option<&'a OverlayTypeRegistry>,
    frame_effects: FrameEffects<'a>,
}

fn path_word(entity: &GameEntity, index: usize) -> Option<u8> {
    entity
        .navigation
        .path_replay()
        .remaining_directions()
        .get(index)
        .copied()
}

impl Simulation {
    /// `Process_Movement(&out, args)` for a Drive/Ship Unit. Returns the out
    /// byte: true when the Foot is gone (0x4B3A21, 0x4B3F4E and the no-queue
    /// continuation 0x4B28BE) or an idle Move's Enter_Idle_Mode answered true
    /// (0x4B26C5).
    pub(crate) fn run_track_process_movement(
        &mut self,
        id: u64,
        family: TrackFamily,
        args: ProcessMovementArgs,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, String> {
        let call = FreshCall {
            id,
            family,
            args,
            rules,
            registry,
            frame_effects,
        };
        self.track_process_movement(&call)
    }

    fn track_process_movement(&mut self, call: &FreshCall<'_>) -> Result<bool, String> {
        let id = call.id;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship Process_Movement owner")?;
        let head = path_word(actor, 0);
        let (moving, _) = super::track_head::motion_state(actor, call.family);
        //4B264D..4B26CD: not moving and no path word: clear the head; an
        //idle Move enters idle mode and its AL becomes the out byte.
        if !moving && head.is_none() {
            let mission = actor.mission.effective().known();
            let actor = self
                .substrate
                .entities
                .get_mut(id)
                .expect("same Process_Movement owner");
            clear_track_head(actor);
            if mission == Some(MissionType::Move) {
                return Ok(self.unit_enter_idle_mode(id, call.rules, false, call.frame_effects));
            }
            return Ok(false);
        }
        //4B26D0..4B26F3: a null destination returns.
        if track_destination(actor).is_none() {
            return Ok(false);
        }
        //4B26F9..4B2719: the warp bytes (+270/+271 via vt+1D4/+1D8).
        if super::locomotor_owner::owner_is_warping(actor) {
            return Ok(false);
        }
        //4B271F..4B273E: launched missiles out (+2D0, 0x6B7D80).
        if actor.spawn_manager.as_ref().is_some_and(|manager| {
            manager.count_launched_missiles(&self.substrate.entities, call.rules, &self.interner)
                > 0
        }) {
            return Ok(false);
        }
        //4B2741..4B2761: vt+37C (Unit 0x746C90: the EMP lock +504 or the
        //death-frame counter +6D8, neither represented; see the residual) and
        //vt+380, FootClass::IsParalyzed 0x4DE770 (the Foot+6A0 timer a
        //parasite arms), return with the out byte clear. Ship's vt+380 gate
        //is6A1DA9. The consumed track_fresh_response controls cover both
        //families with active and expired timers through this shared owner.
        if actor.parasite.is_paralyzed(self.session.binary_frame()) {
            return Ok(false);
        }
        let head = if head.is_some() {
            if let Some(destination) = track_destination(actor) {
                self.substrate.trim_path_for_object_navcom(id, destination);
            }
            self.substrate
                .entities
                .get(id)
                .and_then(|actor| path_word(actor, 0))
        } else {
            None
        };
        match head {
            //4B281C: the no-queue arm and its continuations (track_path).
            None => self.track_no_queue_arm(call),
            Some(direction) => self.track_fresh_arm(call, direction),
        }
    }

    /// 0x4B281C..0x4B2845 then track_path's request and continuations. A found
    /// route resumes head selection in the same call (0x4B3282..0x4B3298).
    fn track_no_queue_arm(&mut self, call: &FreshCall<'_>) -> Result<bool, String> {
        let id = call.id;
        let frame = self.session.binary_frame() as i32;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship no-queue owner")?;
        //4B281C..4B2845: the Foot+640 exact-zero wait.
        if !actor
            .navigation
            .path_runtime()
            .movement_timer()
            .expired(frame)
        {
            return Ok(false);
        }
        let destination = track_destination(actor).ok_or("no-queue arm without destination")?;
        // A batched request waits for the next frame's batch as if the timer
        // still ran (`path_batch`, a proposed deviation). So does a
        // recursion's (the retries at 0x4B397F, 0x4B4480 and the 0x4B4541
        // tail, the second stage's re-entries at 0x4B41B3 and 0x4B420B),
        // unless it is the Foot's first request in a frame the batch answered.
        if Self::defer_path_search(
            &mut self.path_batch,
            &self.resolved_terrain,
            &self.overlay_grid,
            id,
            super::path_batch::WaitingRequest::NoQueue,
        ) {
            return Ok(false);
        }
        let request = self.track_path_request(call, destination, 0)?;
        match self.run_track_path_request(
            &request,
            call.rules,
            call.registry,
            call.frame_effects,
        )? {
            FootPathOutcome::Deleted => Ok(true),
            FootPathOutcome::Returned => Ok(false),
            FootPathOutcome::Resume => {
                let head = self
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|actor| path_word(actor, 0));
                match head {
                    Some(direction) => self.track_fresh_arm(call, direction),
                    None => Ok(false),
                }
            }
        }
    }

    fn track_path_request(
        &self,
        call: &FreshCall<'_>,
        destination: DriveCoord,
        urgency: u8,
    ) -> Result<FootPathRequest, String> {
        FootPathRequest::track(
            &self.substrate.entities,
            call.id,
            destination,
            urgency,
            self.playfield_bounds(),
        )
        .ok_or_else(|| "retired Drive/Ship Find_Path requester".into())
    }

    /// The fresh arm, from 0x4B3298 with the live path head `direction`.
    fn track_fresh_arm(&mut self, call: &FreshCall<'_>, direction: u8) -> Result<bool, String> {
        let id = call.id;
        let rules = call.rules;
        //4B3298: a tube word returns; Process_Track's tube arm enters the tube.
        if direction == TUBE_STEP_DIRECTION {
            return Ok(false);
        }
        let frame = self.session.binary_frame();
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Drive/Ship fresh arm requires map cells")?;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship fresh owner")?;
        //4B32A1..4B32D5: the candidate is Foot XY plus the direction delta,
        //Z unchanged.
        let location = ground_pose::position_world_coord(&actor.position);
        let candidate = super::track_head::offset_head(location, direction);
        let cell = coord_cell(candidate);
        //4B32F0..4B3332: the current Cell's level, +4 on a bridge.
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let height = ground_pose::query_object_cell_height(&cells, location, actor.on_bridge());
        //4B3376..4B3391: the candidate Cell's bridge bit against OnBridge.
        let bridge_mismatch = (cells.flags(cells.lookup(cell)) & 0x100 != 0) != actor.on_bridge();
        let door_closed = actor.door_phase() == crate::sim::door::DoorPhase::ClosedStable;
        if bridge_mismatch {
            self.substrate.latch_foot_68b(id);
        }
        //4B3398..4B33A5: Unit744180 admits only the shared Techno door's
        //closed-stable predicate. Unit Move closes that same door.
        if !door_closed {
            return Ok(false);
        }
        //4B33B3..4B33FA: an allied gate that is not open yet holds the Foot.
        if !crate::sim::gate_runtime::request_gate_open_for_cell(
            self,
            (cell.0 as u16, cell.1 as u16),
            id,
            rules,
        ) {
            return Ok(false);
        }
        //4B3408..4B3458: the exact body facing gate; a mismatch turns
        //(Do_Turn 0x4B0EF0 -> Facing Set 0x4C9220) and returns.
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship fresh owner")?;
        if !fresh_heading_ready(actor.body_facing_current(frame), direction) {
            Self::track_do_turn(
                &self.session,
                &mut self.substrate,
                id,
                u16::from(direction) << 13,
            );
            return Ok(false);
        }
        //4B345B..4B34D1: Mark0, Unit+1AC(cell, dir, height, 0, 1), Mark1.
        self.foot_mark_remove(id, rules, call.registry, call.frame_effects);
        let code = self.track_can_enter(call, cell, direction, height)?;
        self.foot_mark_put(id, rules, call.registry, call.frame_effects);
        let object = self.track_object(id, rules)?;
        let overlay = self.track_overlay(cell);
        //4B34D7..4B351D: the train and crusher coercions.
        let code = coerce_entry_code(
            code,
            object.is_train,
            object.crusher,
            overlay.map_or(-1, i32::from),
        )
        .ok_or("Unit+1AC answered outside 0..7")?;
        //4B3525..4B357A: a crushable overlay (or a wall for CrusherAll) under
        //an admitted candidate forces the straight track later.
        let crush_overlay = code == 0
            && overlay.is_some_and(|overlay| {
                self.overlay_flags(call.registry, overlay)
                    .is_some_and(|(crushable, wall)| {
                        crushable || (wall && object.movement_zone == MovementZone::CrusherAll)
                    })
            });
        let dispatch = dispatch_entry(FreshStage::First, code, call.args.allow_retry, true)
            .ok_or("fresh dispatch outside 0..7")?;
        match dispatch {
            FreshDispatch::Accept => {
                self.track_fresh_accept(call, direction, candidate, cell, height, crush_overlay)
            }
            FreshDispatch::UncloakContacts { retry } => {
                //4B394D..4B3984: ground contact callbacks, then recursion.
                self.uncloak_contacts_at_cell(cell, rules)?;
                if retry.is_some() {
                    self.substrate.clear_path_head(id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B398E..4B39CC then 4B31FC: clear the head, stop or take
                //the next waypoint.
                self.stop_or_take_next_waypoint(id, rules, call.frame_effects);
                Ok(false)
            }
            FreshDispatch::BlockedDelay => self.track_blocked_delay(call),
            FreshDispatch::Gate { .. } => {
                //4B35EC..4B3602: the gate question, answer discarded.
                let _ = crate::sim::gate_runtime::request_gate_open_for_cell(
                    self,
                    (cell.0 as u16, cell.1 as u16),
                    id,
                    rules,
                );
                self.substrate.track_first_rejected_tail(id);
                Ok(false)
            }
            FreshDispatch::WallOrObject { retry } => {
                self.substrate.clear_track_head_of(id);
                if retry.is_some() {
                    //4B3AD3..4B3AFE then 4B4541: drop the head, expire the
                    //movement timer and recurse.
                    self.substrate.clear_path_head(id);
                    Self::expire_movement_timer(&self.session, &mut self.substrate, id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B3B03..4B3BE9: attack the non-allied blocking object, or
                //the wall cell when the cell holds none.
                self.override_movement_blocker_at(
                    call.id,
                    cell,
                    call.rules,
                    call.registry,
                    call.frame_effects,
                );
                //4B3C67..4B3C81: code != 7 retires the selector.
                if let Some(actor) = self.substrate.entities.get_mut(id) {
                    actor.navigation.path_runtime_mut().clear_scold_latch();
                }
                self.substrate.track_retire_selector(id);
                Ok(false)
            }
            FreshDispatch::ScatterOrStop { retry, .. } => {
                if retry.is_some() {
                    //4B3721..4B373D then 4B4541.
                    self.substrate.clear_path_head(id);
                    Self::expire_movement_timer(&self.session, &mut self.substrate, id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B3742..4B38A1: close enough to the destination, level and
                //off a Tunnel: stop or take the next waypoint.
                if self.track_close_enough_stop(id, rules)? {
                    if self.stop_or_take_next_waypoint(id, rules, call.frame_effects) {
                        //4B38A7: an entered idle mode returns at once.
                        return Ok(false);
                    }
                } else {
                    //4B38B3..4B393A: Scatter_Objects(Null, 1, 1, deck).
                    self.scatter_blocked_track_cell(
                        id,
                        cell,
                        rules,
                        call.registry,
                        call.frame_effects,
                    )?;
                }
                self.substrate.track_first_rejected_tail(id);
                Ok(false)
            }
            FreshDispatch::FirstOtherBlocked { retry } => {
                //4B3607 then4B3AA1..4B3ACE / 6A30F3..6A311D. The sound
                //guard is NOT cleared before the possible code7 recursion.
                self.substrate.clear_track_head_of(id);
                Self::play_foot_path_scold(&self.substrate, &mut self.sound_events, id, rules);
                if retry.is_some() {
                    //4B3BF6..4B3C21 then 4B4541.
                    self.substrate.clear_path_head(id);
                    Self::expire_movement_timer(&self.session, &mut self.substrate, id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B3C26..4B3C62 then 4B31FC.
                self.stop_or_take_next_waypoint(id, rules, call.frame_effects);
                Ok(false)
            }
            FreshDispatch::OwnerNotAlive
            | FreshDispatch::Retry(_)
            | FreshDispatch::ClearSecondThenRetry(_)
            | FreshDispatch::SecondRetryOrStop { .. } => {
                Err("second-stage dispatch at the first candidate".into())
            }
        }
    }

    /// 0x4B3607..0x4B3A94: the first candidate's code-2 ladder.
    fn track_blocked_delay(&mut self, call: &FreshCall<'_>) -> Result<bool, String> {
        let id = call.id;
        let rules = call.rules;
        let frame = self.session.binary_frame();
        self.substrate.clear_track_head_of(id);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Drive/Ship code-2 owner")?;
        let runtime = actor.navigation.path_runtime_mut();
        //4B3656..4B368D: the latch arms the blockage timer once.
        runtime.latch_blockage(frame, rules.general.blockage_path_delay_ticks);
        //4B3690..4B36B6: while the movement timer runs, the rejected tail.
        if !runtime.movement_timer().expired(frame as i32) {
            self.substrate.track_first_rejected_tail(id);
            return Ok(false);
        }
        //4B36BC..4B36EF / 4B39D1..4B3A0E: urgency 2 once the latched
        //blockage timer expired, else 1; Find_Path(cell(destination), 0, u).
        let urgency = runtime.code2_urgency(frame);
        let destination = super::path_batch::code2_ladder_destination(actor)
            .ok_or("Drive/Ship code-2 ladder without a track locomotor")?;
        // A batched re-path waits for the next frame's batch as if the timer
        // still ran (`path_batch`, a proposed deviation).
        if Self::defer_path_search(
            &mut self.path_batch,
            &self.resolved_terrain,
            &self.overlay_grid,
            id,
            super::path_batch::WaitingRequest::Code2Ladder { urgency },
        ) {
            self.substrate.track_first_rejected_tail(id);
            return Ok(false);
        }
        // An answered re-path searches at the urgency it registered at.
        let urgency = self.batched_code2_urgency(id, urgency);
        let request = self.track_path_request(call, destination, urgency)?;
        let found = self.foot_find_path(&request, rules, call.registry, call.frame_effects)?;
        //4B3A13..4B3A2A: a vanished Foot sets the out byte.
        if self.substrate.entities.get(id).is_none() {
            return Ok(true);
        }
        //4B3A2D..4B3A3C: found, or the destination is still reachable (+2CC
        //on the live +34, which a core failure's Stop has nulled).
        if found != FindPathResult::Failed
            || self.foot_path_zone_precheck(id, self.live_track_destination(id), rules)?
        {
            //4B3A59..4B3A8C: +640 = (Frame, PathDelay).
            if let Some(actor) = self.substrate.entities.get_mut(id) {
                actor
                    .navigation
                    .path_runtime_mut()
                    .start_movement(frame, rules.general.path_delay_ticks());
            }
            return Ok(false);
        }
        //4B3A3E..4B3A56: SetDestination(NULL, 1).
        self.set_unit_null_destination(id, rules, None, call.frame_effects);
        Ok(false)
    }

    /// The accepted first candidate, 0x4B357F..0x4B45F6.
    fn track_fresh_accept(
        &mut self,
        call: &FreshCall<'_>,
        direction: u8,
        candidate: DriveCoord,
        cell: (i16, i16),
        height: i32,
        crush_overlay: bool,
    ) -> Result<bool, String> {
        let id = call.id;
        let rules = call.rules;
        //4B357F..4B35D6 / 4B3C84: within two levels the candidate's own level
        //and land speed row; otherwise the current height and Road (1).
        let level = self.track_cell_level(cell)?;
        let road = (height - level).abs() >= 2;
        let height2 = if road { height } else { level };
        //4B3C8D..4B3E21: publish the target speed fraction.
        self.publish_fresh_speed(id, cell, road, rules);
        //4B3E27..4B3E65: Unit+534(cell, 1), the crusher's pre-entry scatter.
        self.track_crusher_pre_scatter(call, cell)?;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship accept owner")?;
        let mut second = path_word(actor, 1).map_or(-1, i32::from);
        //4B3E6B..4B3F74: the last path word with the destination more than
        //two cells out asks Find_Path for the next words. The +34 read
        //(4B3E89, Ship 6A34D8) has no null test: a destination stopped
        //earlier in this call (the Teleporter arm a Find_Path redirect
        //reaches, 0x0074258C, #1672) is the null coordinate, so Find_Path
        //asks for cell (0,0) and its precheck refuses.
        if second == -1 {
            let destination = self.live_track_destination(id);
            let location = ground_pose::position_world_coord(&actor.position);
            let distance = native_xyz_distance(
                location.x.wrapping_sub(destination.x),
                location.y.wrapping_sub(destination.y),
                location.z.wrapping_sub(destination.z),
            );
            if distance > 0x200 {
                //Find_Path(cell(destination), IsTrain, 0); see the train residual.
                let request = self.track_path_request(call, destination, 0)?;
                let found =
                    self.foot_find_path(&request, rules, call.registry, call.frame_effects)?;
                if found == FindPathResult::Failed {
                    //4B3F40..4B3F55: a vanished Foot sets the out byte.
                    if self.substrate.entities.get(id).is_none() {
                        return Ok(true);
                    }
                    //4B3F58..4B3F6E: an unreachable live destination clears.
                    if !self.foot_path_zone_precheck(id, self.live_track_destination(id), rules)? {
                        self.set_unit_null_destination(id, rules, None, call.frame_effects);
                    }
                }
                second = self
                    .substrate
                    .entities
                    .get(id)
                    .and_then(|actor| path_word(actor, 1))
                    .map_or(-1, i32::from);
            } else {
                second = i32::from(direction);
            }
        }
        //4B3F7D..4B3F93: tube, missing or forced-single words go straight.
        let mut second =
            normalize_second_direction(i32::from(direction), second, call.args.force_single);
        //4B3F93..4B4012: a crushable overlay (or a CrusherAll wall or Unit
        //under an overlay) in the next cell, or under this one, goes straight.
        let object = self.track_object(id, rules)?;
        let next_cell = {
            let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[(second & 7) as usize];
            (cell.0 + dx as i16, cell.1 + dy as i16)
        };
        let straight = crush_overlay || {
            match self.track_overlay(next_cell) {
                Some(overlay) => {
                    let (crushable, wall) = self
                        .overlay_flags(call.registry, overlay)
                        .unwrap_or((false, false));
                    //4B3FE1..4B4000: the Unit arm is Drive-only; Ship
                    //6A35E7..6A362C tests the crushable and wall bytes alone.
                    crushable
                        || (object.movement_zone == MovementZone::CrusherAll
                            && (wall
                                || (call.family == TrackFamily::Drive
                                    && next_cell.0 >= 0
                                    && next_cell.1 >= 0
                                    && self
                                        .substrate
                                        .occupancy
                                        .first_category_on_layer(
                                            next_cell.0 as u16,
                                            next_cell.1 as u16,
                                            MovementLayer::Ground,
                                            EntityCategory::Unit,
                                            &self.substrate.entities,
                                        )
                                        .is_some())))
                }
                None => false,
            }
        };
        if straight {
            second = i32::from(direction);
        }
        //4B4016..4B4034: the turn table, with the from*9 fallback; +58 and
        //+60 = 0 are written now, before the crate question and the second
        //query, so a second-stage stop or retry keeps the new selector.
        let turn_index = super::drive_track::fresh_turn_index(direction, second as u8);
        if let Some(loco) = self
            .substrate
            .entities
            .get_mut(id)
            .and_then(|actor| actor.locomotor.as_mut())
        {
            if call.family == TrackFamily::Drive {
                //4B400C/4B4012: retain the actual crush-straight byte,
                //including false. A straight-looking selector is not this
                //decision; forced-single normalization alone does not set it.
                loco.ensure_installed_track_state();
                loco.store_drive_straight(straight);
            }
            super::track_head::select_fresh_progress(loco, turn_index);
        }
        let two_node = super::drive_track::turn_track_at(turn_index)
            .is_some_and(|turn| turn.flags & super::drive_track::TURN_TRACK_TURNS_FLAG != 0);
        if !two_node {
            //4B45F6..4B4609: shift one word, then the finalize tail.
            return self.track_fresh_finalize(call, Some(candidate), 1, turn_index);
        }
        //4B404B..4B4062: the crate question on the first cell.
        let picked = self.pickup_crate_at(
            id,
            coord_cell(candidate),
            call.rules,
            call.registry,
            call.frame_effects,
        );
        //4B4066..4B4089: a false answer (a placed free vehicle): an owner in
        //limbo rejoins the ordinary path, a dead one ends the Process, and a
        //live one skips the second query and takes code 7 at 4B4179.
        let mut forced_code = None;
        if !picked {
            let (in_limbo, alive) = self
                .substrate
                .entities
                .get(id)
                .map_or((false, false), |actor| {
                    (actor.lifecycle.in_limbo(), actor.lifecycle.object_alive())
                });
            if !in_limbo {
                if !alive {
                    return Ok(false);
                }
                forced_code = Some(7);
            }
        }
        //4B4092..4B409D: the owner's life check.
        if !self.track_owner_alive(id) {
            return Ok(false);
        }
        //4B40A3..4B4126: the second candidate, without the Mark bracket.
        let second_candidate = super::track_head::offset_head(candidate, second as u8);
        let second_cell = coord_cell(second_candidate);
        let code = match forced_code {
            Some(code) => code,
            None => {
                let code = self.track_can_enter(call, second_cell, second as u8, height2)?;
                let overlay = self.track_overlay(second_cell);
                coerce_entry_code(
                    code,
                    object.is_train,
                    object.crusher,
                    overlay.map_or(-1, i32::from),
                )
                .ok_or("Unit+1AC answered outside 0..7")?
            }
        };
        //4B4179..4B4184: the second query's life check.
        let alive = self.track_owner_alive(id);
        let dispatch = dispatch_entry(FreshStage::Second, code, call.args.allow_retry, alive)
            .ok_or("fresh dispatch outside 0..7")?;
        match dispatch {
            FreshDispatch::OwnerNotAlive => Ok(false),
            FreshDispatch::Accept => {
                //4B45CB..4B45F4: shift two words, Foot+68B = 1, then the
                //finalize tail.
                self.substrate.latch_foot_68b(id);
                self.track_fresh_finalize(call, Some(second_candidate), 2, turn_index)
            }
            FreshDispatch::Gate { .. } => {
                //4B419B..4B41AE: the gate question, answer discarded.
                let _ = crate::sim::gate_runtime::request_gate_open_for_cell(
                    self,
                    (second_cell.0 as u16, second_cell.1 as u16),
                    id,
                    rules,
                );
                self.track_second_refused(call)
            }
            FreshDispatch::Retry(retry) => {
                //4B420B..4B4219: recurse with arg3 = 1, nothing cleared.
                self.track_process_movement(&call.with_args(ProcessMovementArgs {
                    allow_retry: retry.allow_retry,
                    force_single: retry.force_single_direction,
                }))
            }
            FreshDispatch::ClearSecondThenRetry(retry) => {
                //4B41B3..4B41F7: clear, then recurse with arg3 = 1.
                self.substrate.clear_path_head(id);
                self.substrate.track_retire_selector(id);
                self.track_process_movement(&call.with_args(ProcessMovementArgs {
                    allow_retry: retry.allow_retry,
                    force_single: retry.force_single_direction,
                }))
            }
            FreshDispatch::ScatterOrStop { retry, .. } => {
                if retry.is_some() {
                    //4B424E..4B426E then 4B4541.
                    self.substrate.clear_path_head(id);
                    Self::expire_movement_timer(&self.session, &mut self.substrate, id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B4273..4B43BE: the CloseEnough stop.
                if self.track_close_enough_stop(id, rules)? {
                    if self.stop_or_take_next_waypoint(id, rules, call.frame_effects) {
                        return Ok(false);
                    }
                } else {
                    //4B43D0..4B4437: Scatter_Objects on the second cell.
                    self.scatter_blocked_track_cell(
                        id,
                        second_cell,
                        rules,
                        call.registry,
                        call.frame_effects,
                    )?;
                }
                self.track_second_refused(call)
            }
            FreshDispatch::UncloakContacts { retry } => {
                //4B444A..4B4485: ground contact callbacks, then recursion.
                self.uncloak_contacts_at_cell(second_cell, rules)?;
                if retry.is_some() {
                    self.substrate.clear_path_head(id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B448F..4B450D.
                self.stop_or_take_next_waypoint(id, rules, call.frame_effects);
                Ok(false)
            }
            FreshDispatch::SecondRetryOrStop { retry } => {
                if retry.is_some() {
                    //4B4521..4B453D then 4B4541.
                    self.substrate.clear_path_head(id);
                    Self::expire_movement_timer(&self.session, &mut self.substrate, id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B4561..4B45C8.
                self.stop_or_take_next_waypoint(id, rules, call.frame_effects);
                Ok(false)
            }
            FreshDispatch::BlockedDelay
            | FreshDispatch::WallOrObject { .. }
            | FreshDispatch::FirstOtherBlocked { .. } => {
                Err("first-stage dispatch at the second candidate".into())
            }
        }
    }

    /// 0x4B41B3..0x4B41E7 for a refused second candidate that does not
    /// recurse, then the finalize tail with a null candidate.
    fn track_second_refused(&mut self, call: &FreshCall<'_>) -> Result<bool, String> {
        self.substrate.clear_path_head(call.id);
        self.substrate.track_retire_selector(call.id);
        self.track_fresh_finalize(call, None, 0, 0)
    }

    /// 0x4B460C..0x4B4764. `consumed` path words (1 or 2) have been shifted;
    /// a null candidate records Cell (0,0) as the reference and stops.
    fn track_fresh_finalize(
        &mut self,
        call: &FreshCall<'_>,
        candidate: Option<DriveCoord>,
        consumed: usize,
        turn_index: usize,
    ) -> Result<bool, String> {
        let id = call.id;
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Drive/Ship finalize owner")?;
        //4B460C..4B4659: +63C = -1 (the shift's terminator), +558 = the
        //candidate's cell, Foot+68A = 0, class +5C = 0, then the head clears.
        let reference = candidate.map_or((0, 0), coord_cell);
        actor.navigation.path_runtime_mut().clear_scold_latch();
        clear_track_head(actor);
        let Some(candidate) = candidate else {
            actor.navigation.path_replay_mut().rebase(reference);
            return self.substrate.track_finalize_stop(id);
        };
        actor
            .navigation
            .path_replay_mut()
            .accept(reference, consumed);
        //4B46C3..4B46D7: install the selector and head (+63 = 1).
        if let Some(loco) = actor.locomotor.as_mut() {
            super::track_head::accept_fresh_progress(loco, turn_index);
            loco.store_track_head(call.family, Some(candidate));
        }
        //4B46DF..4B46FA: the crate question on the head, then limbo.
        let picked =
            self.pickup_crate_at(id, reference, call.rules, call.registry, call.frame_effects);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Drive/Ship finalize owner")?;
        if picked && !actor.lifecycle.in_limbo() {
            //4B46FC..4B4705: Apply_Track_Occupation_Mode(head, 1).
            Self::track_apply_occupation(
                &mut self.substrate,
                &self.path_grid,
                &self.resolved_terrain,
                id,
                call.family,
                true,
            );
            return Ok(false);
        }
        //4B4716..4B473C: a false answer or a limbo owner: a live owner drops
        //the head again.
        if actor.lifecycle.object_alive() {
            clear_track_head(actor);
        }
        self.substrate.track_finalize_stop(id)
    }

    /// The live Unit+1AC answer for one candidate, arguments (cell, dir, h, 0, 1).
    fn track_can_enter(
        &self,
        call: &FreshCall<'_>,
        cell: (i16, i16),
        direction: u8,
        height: i32,
    ) -> Result<u8, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Drive/Ship candidate query requires map cells")?;
        let native_cell = terrain.native_cell_identity(cell);
        self.foot_can_enter(
            call.id,
            native_cell,
            InfantryEntryArgs {
                direction: i32::from(direction),
                height,
                previous_cell: None,
            },
            call.rules,
            call.registry,
        )
    }

    fn track_object<'r>(
        &self,
        id: u64,
        rules: &'r RuleSet,
    ) -> Result<&'r crate::rules::object_type::ObjectType, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship owner")?;
        self.interner
            .object_type(actor.type_ref(), rules)
            .ok_or_else(|| "Drive/Ship owner without ObjectType".into())
    }

    /// Cell+44, the overlay index of a Cell (the shared dummy has its own).
    fn track_overlay(&self, cell: (i16, i16)) -> Option<u8> {
        let terrain = self.resolved_terrain.as_ref()?;
        self.cell_overlay(terrain.native_cell_identity(cell))
            .overlay_id
    }

    /// OverlayType +22D (Crushable) and +2A8 (Wall).
    fn overlay_flags(
        &self,
        registry: Option<&OverlayTypeRegistry>,
        overlay: u8,
    ) -> Option<(bool, bool)> {
        registry
            .and_then(|registry| registry.flags(overlay))
            .map(|flags| (flags.crushable, flags.wall))
    }

    /// Cell+11B of the candidate.
    fn track_cell_level(&self, cell: (i16, i16)) -> Result<i32, String> {
        let terrain = self
            .resolved_terrain
            .as_ref()
            .ok_or("Drive/Ship accept arm requires map cells")?;
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        Ok(i32::from(cells.ground_fields(cells.lookup(cell)).0 as i8))
    }

    /// 0x4B3C8D..0x4B3E21: publish the admitted candidate's target speed.
    fn publish_fresh_speed(&mut self, id: u64, cell: (i16, i16), road: bool, rules: &RuleSet) {
        if cell.0 < 0 || cell.1 < 0 {
            return;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let Some(strength) = self
            .substrate
            .entities
            .get(id)
            .and_then(|actor| self.interner.object_type(actor.type_ref(), rules))
            .map(|object| object.strength)
        else {
            return;
        };
        let terrain_speed = self.terrain_speed_config().clone();
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        super::track_speed::publish_fresh_track_target(
            actor,
            strength,
            rules,
            terrain,
            &terrain_speed,
            (cell.0 as u16, cell.1 as u16),
            road,
        );
    }

    /// The live Destination (+34) that 0x4B3E89 reads and 0x4B39D6 / 0x4B3E8F
    /// address and hand to +2CC after Find_Path: Find_Path's core-failure
    /// receiver (+500 -> locomotor Stop) and a redirect's Teleporter arm null
    /// it, and a null reads as Cell (0,0).
    fn live_track_destination(&self, id: u64) -> DriveCoord {
        self.substrate.entities.get(id).map_or(
            super::foot_coordinate::NULL_COORD,
            track_destination_or_null,
        )
    }

    /// Foot+640 = (Frame, 0) before a retry recursion (0x4B4541..0x4B4548).
    fn expire_movement_timer(session: &ScenarioSession, substrate: &mut ObjectSubstrate, id: u64) {
        let frame = session.binary_frame();
        if let Some(actor) = substrate.entities.get_mut(id) {
            actor.navigation.path_runtime_mut().start_movement(frame, 0);
        }
    }

    /// Do_Turn 0x4B0EF0: FacingClass::Set 0x4C9220 on the body facing.
    fn track_do_turn(
        session: &ScenarioSession,
        substrate: &mut ObjectSubstrate,
        id: u64,
        desired: u16,
    ) {
        let frame = session.binary_frame();
        let Some(actor) = substrate.entities.get_mut(id) else {
            return;
        };
        super::drive_do_turn(actor, desired, frame);
    }

    /// The CloseEnough stop tests of the fresh arm's code-6 responses
    /// (0x4B3742..0x4B3829, 0x4B4273..0x4B4346): Sqrt_Approx of |Foot -
    /// destination| below CloseEnough, then the stop band. The first site sums
    /// dz*dz + dy*dy + dx*dx, the second (dx*dx + dz*dz) + dy*dy; integer
    /// squares at map scale add exactly in a double, so one order serves both.
    fn track_close_enough_stop(&self, id: u64, rules: &RuleSet) -> Result<bool, String> {
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship code-6 owner")?;
        let destination = track_destination_or_null(actor);
        let location = ground_pose::position_world_coord(&actor.position);
        let distance = native_xyz_distance(
            location.x.wrapping_sub(destination.x),
            location.y.wrapping_sub(destination.y),
            location.z.wrapping_sub(destination.z),
        );
        Ok(distance < rules.general.close_enough && self.track_stop_band(location, destination))
    }

    /// The stop band both code-6 arms test after CloseEnough (fresh
    /// 0x4B37CA..0x4B3829, continuation ally arm 0x4B2C9C..0x4B2CD7): the
    /// destination within two levels of the Foot, and the Foot's Cell
    /// (Unit+9C through 0x565730) not a Tunnel.
    pub(super) fn track_stop_band(&self, location: DriveCoord, destination: DriveCoord) -> bool {
        if destination.z.wrapping_sub(location.z).wrapping_abs() >= 2 * GROUND_LEVEL_HEIGHT_LEPTONS
        {
            return false;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        //Walk75B7D3..75B7DF consumes this same Cell+EC, including retained
        //Dummy state. The response corpus pins missing-current Clear/Tunnel.
        let current = cells.lookup_world(location.x, location.y);
        cells.land_type(current) != i32::from(LAND_TUNNEL)
    }

    /// Scatter_Objects(Null, 1, 1, deck) on a refused `cell` (fresh
    /// 0x4B38B3..0x4B393A and 0x4B43D0..0x4B4437, the continuation's ally
    /// arm 0x4B2D68..0x4B2DC0): the deck list when the Cell is structural and
    /// the Foot (Unit+9C) is more than two levels from the Cell's level.
    pub(super) fn scatter_blocked_track_cell(
        &mut self,
        id: u64,
        cell: (i16, i16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Ok(());
        };
        let Some(actor) = self.substrate.entities.get(id) else {
            return Ok(());
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let native = cells.lookup(cell);
        let level = i32::from(cells.ground_fields(native).0 as i8);
        let location = ground_pose::position_world_coord(&actor.position);
        let deck = cells.flags(native) & 0x100 != 0
            && (location.z / GROUND_LEVEL_HEIGHT_LEPTONS - level).abs() > 2;
        self.scatter_cell_contacts(cell, deck, true, rules, registry, frame_effects)
    }

    /// `CellClass::Scatter_Objects(null, 1, no_kidding, deck)` on `cell`
    /// ([`Self::scatter_cell_objects`]); also the computer's site clearing
    /// (`sim::build_site::flush_for_placement`). The receivers' bridge-state
    /// answer is dropped: only an Engineer arriving at a BridgeRepairHut cell
    /// sets it, and a receiver's one Process walks toward a cell FNPC or the
    /// neighbour search found passable, which a hut cell is not.
    pub(crate) fn scatter_cell_contacts(
        &mut self,
        cell: (i16, i16),
        deck: bool,
        no_kidding: bool,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<(), String> {
        #[cfg(test)]
        if super::fresh_oracle_seam::substitute(
            super::fresh_oracle_seam::FreshCallRecord::Scatter {
                cell,
                no_kidding,
                deck,
            },
        ) {
            return Ok(());
        }
        if cell.0 < 0 || cell.1 < 0 {
            return Ok(());
        }
        self.scatter_cell_objects(
            (cell.0 as u16, cell.1 as u16),
            if deck {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            },
            None,
            super::ScatterFlags::new(true, no_kidding),
            rules,
            registry,
            frame_effects,
        )?;
        Ok(())
    }

    /// Unit+534(cell, 1) = 0x7416A0 with its second argument set: a Crusher
    /// (Type+D28 or ability 0x11) scatters a Cell holding infantry (raw
    /// +124/+128 & 0x1F) without force. The deck list applies on a structural
    /// Cell the Foot rides or reaches from four levels above.
    fn track_crusher_pre_scatter(
        &mut self,
        call: &FreshCall<'_>,
        cell: (i16, i16),
    ) -> Result<(), String> {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return Ok(());
        };
        let Some(actor) = self.substrate.entities.get(call.id) else {
            return Ok(());
        };
        let Some(object) = self.interner.object_type(actor.type_ref(), call.rules) else {
            return Ok(());
        };
        //Type+D28, or HasWeaponAbility(0x11) by rank (0x70D0D0).
        let crusher = object.crusher
            || has_weapon_ability(actor.veterancy.rank_kind(), object, Ability::Crusher);
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let native = cells.lookup(cell);
        let deck = cells.flags(native) & 0x100 != 0 && {
            let level = i32::from(cells.ground_fields(native).0 as i8);
            let location = ground_pose::position_world_coord(&actor.position);
            let here = cells.lookup_world(location.x, location.y);
            actor.on_bridge()
                || i32::from(cells.ground_fields(here).0 as i8)
                    == level + crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS
        };
        if !crusher {
            return Ok(());
        }
        let layer = if deck {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let infantry = self.substrate.raw_cell_occupation.bits_at(
            crate::sim::occupancy::RawCellKey::from_native(terrain, native),
            layer,
        ) & 0x1F
            != 0;
        if infantry {
            self.scatter_cell_contacts(
                cell,
                deck,
                false,
                call.rules,
                call.registry,
                call.frame_effects,
            )?;
        }
        Ok(())
    }

    /// Code 4/5 without a retry (Drive 0x4B3B03..0x4B3BE9, Hover
    /// 0x00515B97..0x00515C9C): `Find_Blocking_Object` then Override(Attack)
    /// on a non-allied object, else on a wall cell.
    pub(super) fn override_movement_blocker_at(
        &mut self,
        id: u64,
        cell: (i16, i16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) {
        if cell.0 < 0 || cell.1 < 0 {
            return;
        }
        let key = (cell.0 as u16, cell.1 as u16);
        match self.find_blocking_object(key, MovementLayer::Ground) {
            Some(BlockingObject::Entity(blocker)) => {
                let (Some(actor), Some(target)) = (
                    self.substrate.entities.get(id),
                    self.substrate.entities.get(blocker),
                ) else {
                    return;
                };
                //4B3B52..4B3B62: the mover's House asks about the object.
                if crate::map::houses::is_allied_with(
                    self.house_alliances.as_map(),
                    self.interner.resolve(actor.owner()),
                    self.interner.resolve(target.owner()),
                ) {
                    return;
                }
                self.track_override(id, TargetKind::Entity(blocker), rules, frame_effects);
            }
            Some(BlockingObject::Terrain(blocker)) => {
                // Terrain7F522C+3C ->4104A0 returns NULL, so the native
                // House4F9A90 test is false. Keep the Object pointer through
                // the same Foot4D8F40 target/destination transaction.
                self.track_override(id, TargetKind::Entity(blocker), rules, frame_effects);
            }
            None => {
                //4B3B94..4B3BE3: a wall overlay makes the Cell the target.
                let wall = self
                    .track_overlay(cell)
                    .and_then(|overlay| self.overlay_flags(registry, overlay))
                    .is_some_and(|(_, wall)| wall);
                if wall {
                    self.track_override(id, TargetKind::Cell(key.0, key.1), rules, frame_effects);
                }
            }
        }
    }

    /// Foot::Override_Mission(Attack, target, NULL) at 0x4B3BE9.
    fn track_override(
        &mut self,
        id: u64,
        target: TargetKind,
        rules: &RuleSet,
        frame_effects: FrameEffects<'_>,
    ) {
        #[cfg(test)]
        if super::fresh_oracle_seam::substitute(
            super::fresh_oracle_seam::FreshCallRecord::Override { target },
        ) {
            return;
        }
        self.mission_override_movement_blocker(id, target, rules, frame_effects);
    }

    /// `CellClass::Find_Blocking_Object 0x47C5A0` with the zero point: the
    /// first Aircraft of the ground list, else `Find_Nearest_Object`
    /// (0x47C3D0), else the first terrain object. Track admission4B3BE9 and
    /// CoordinateAttack6EB56F use ground; Script1 at6EC9F7 selects the
    /// bridge list from Cell140 bit100, as Script59 at6ECAC7 also does.
    /// This is their shared native owner.
    pub(crate) fn find_blocking_object(
        &self,
        cell: (u16, u16),
        layer: MovementLayer,
    ) -> Option<BlockingObject> {
        let list = self.substrate.occupancy.get(cell.0, cell.1);
        if let Some(aircraft) = list
            .into_iter()
            .flat_map(|list| list.iter_layer(MovementLayer::Ground))
            .find(|entry| {
                self.substrate
                    .entities
                    .get(entry.entity_id)
                    .is_some_and(|entity| entity.category() == EntityCategory::Aircraft)
            })
        {
            return Some(BlockingObject::Entity(aircraft.entity_id));
        }
        let nearest = self.nearest_cell_object(cell, layer, None);
        if let Some(object) = nearest {
            return Some(BlockingObject::Entity(object));
        }
        self.terrain_objects
            .cell_index()
            .get(&cell)
            .copied()
            .map(BlockingObject::Terrain)
    }
}

impl crate::sim::world::ObjectSubstrate {
    /// 0x4B2770..0x4B2813 (Hover 0x005163EC..0x00516485): while NavCom is a
    /// Unit or an Infantry, the path is cut at the cell count of
    /// |Foot - destination| (Distance3D >> 8) when that is below 24, so a
    /// chase re-plans as it closes in. Returns whether it cut.
    pub(super) fn trim_path_for_object_navcom(&mut self, id: u64, destination: DriveCoord) -> bool {
        let Some(actor) = self.entities.get(id) else {
            return false;
        };
        let object = match actor.navigation.nav_com() {
            Some(NavTargetRef::Object { id: object }) => object,
            _ => return false,
        };
        if !self.entities.get(object).is_some_and(|nav| {
            matches!(
                nav.category(),
                EntityCategory::Unit | EntityCategory::Infantry
            )
        }) {
            return false;
        }
        let location = ground_pose::position_world_coord(&actor.position);
        let distance = crate::util::native_x87::distance_3d_leptons(
            [location.x, location.y, location.z],
            [destination.x, destination.y, destination.z],
        );
        let cells = crate::util::lepton::lepton_to_cell(distance);
        if cells >= 24 {
            return false;
        }
        let queue = &mut self
            .entities
            .get_mut(id)
            .expect("same NavCom trim owner")
            .navigation
            .path_replay_mut();
        let slot = usize::from(queue.cursor()) + cells as usize;
        queue.terminate_at(slot);
        true
    }
    /// 0x4B3607 then 0x4B3AA1..0x4B3C81 for a code other than 2/4/5/7: clear
    /// the head; Foot+68A = 0 and the selector retires. This is a silent
    /// exit, distinct from the code7 sound/retry corridor.
    fn track_first_rejected_tail(&mut self, id: u64) {
        self.clear_track_head_of(id);
        if let Some(actor) = self.entities.get_mut(id) {
            actor.navigation.path_runtime_mut().clear_scold_latch();
        }
        self.track_retire_selector(id);
    }
    /// 0x4B4740..0x4B4764: selector -1, path word -1, SetSpeedFraction(0).
    fn track_finalize_stop(&mut self, id: u64) -> Result<bool, String> {
        self.track_retire_selector(id);
        self.clear_path_head(id);
        if let Some(actor) = self.entities.get_mut(id) {
            actor
                .foot_speed
                .set_speed_fraction(crate::util::fixed_math::SIM_ZERO);
        }
        Ok(false)
    }
    fn clear_track_head_of(&mut self, id: u64) {
        if let Some(actor) = self.entities.get_mut(id) {
            clear_track_head(actor);
        }
    }
    /// Foot+68B = 1 (0x4B3391, 0x4B45ED): write-once in the program (the
    /// Foot constructor zeroes it; nothing clears it), read only by
    /// `FootClass::ComputeChecksum` (0x4DBD0C). Its persisted analogue is
    /// the hashed `RuntimeBridgeTransitionState::pending_mismatch`.
    pub(super) fn latch_foot_68b(&mut self, id: u64) {
        if let Some(actor) = self.entities.get_mut(id) {
            actor.runtime_bridge_transition.latch();
        }
    }
    /// Foot+5E0 = -1 on the live path word.
    fn clear_path_head(&mut self, id: u64) {
        if let Some(actor) = self.entities.get_mut(id) {
            actor.clear_live_path_head();
        }
    }
    /// Class selector +58 = -1. Only the first-rejection/fresh-finalize
    /// callers also clear Foot+68A; second-candidate retries retain it.
    fn track_retire_selector(&mut self, id: u64) {
        let Some(actor) = self.entities.get_mut(id) else {
            return;
        };
        if let Some(loco) = actor.locomotor.as_mut()
            && let Some(family) = TrackFamily::from_kind(loco.active_kind())
            && let Some(mut progress) = loco.track_progress(family)
        {
            progress.retire_selector();
            loco.store_track_progress(family, progress);
        }
    }
}

/// What `Find_Blocking_Object` returned.
pub(crate) enum BlockingObject {
    Entity(u64),
    Terrain(u64),
}

/// LandType Tunnel (10), Cell+EC.
const LAND_TUNNEL: u8 = 10;

impl FreshCall<'_> {
    fn with_args(&self, args: ProcessMovementArgs) -> Self {
        Self { args, ..*self }
    }
}

/// Drive 0x4B3408..0x4B3458 / Ship 0x6A2A57 twin: the fresh head needs the
/// body facing exactly on the head direction's octant (`direction << 13`),
/// with no tolerance; otherwise Process turns and returns. Original rows:
/// tools/spatial_oracle/drive_fresh_turn.json.
pub(super) fn fresh_heading_ready(body_facing: u16, direction: u8) -> bool {
    body_facing == u16::from(direction) << 13
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::mission::MissionId;

    #[test]
    fn terrain_blocker_response_preserves_object_target_and_move_archive() {
        // Component boundary: original 4B3B24/4C/8D returns the Terrain
        // pointer, whose null House (7F522C+3C -> 4104A0) permits Attack at
        // 4B3BE9. This supplies that response's cell, not a path or an ordinary
        // map-route claim. Unit73FBAC and the real stock AP reader establish
        // the preceding code5; no CanEnter/FindPath/Override seam is installed.
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        else {
            return;
        };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 1);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let tank = sim
            .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
            .expect("stock tank admitted on clear ground");
        assert_eq!(
            crate::sim::terrain_spawn::construct_terrain_objects(
                &mut sim,
                &[crate::map::overlay::TerrainObject {
                    rx: 11,
                    ry: 10,
                    name: "TREE01".into(),
                }],
                &rules,
                false,
            ),
            1
        );
        let tree = sim.terrain_objects.cell_index()[&(11, 10)];
        assert!(!sim.substrate.entities.contains(tree));
        let cell = sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_identity((11, 10));
        assert_eq!(
            sim.foot_can_enter(
                tank,
                cell,
                InfantryEntryArgs {
                    direction: 2,
                    height: 0,
                    previous_cell: None,
                },
                &rules,
                None,
            )
            .expect("real Unit entry receiver"),
            5,
            "stock AP Wood admits the live Terrain as a destructible blocker"
        );
        let destination = NavTargetRef::cell(14, 10);
        assert!(
            sim.set_unit_destination(tank, destination, &rules, true, FrameEffects::default(),)
        );
        sim.substrate
            .mission_assign_exact(tank, MissionId::from_known(MissionType::Move), 0)
            .unwrap();
        assert_eq!(
            sim.substrate
                .entities
                .get(tank)
                .unwrap()
                .mission
                .effective()
                .known(),
            Some(MissionType::Move)
        );

        sim.override_movement_blocker_at(tank, (11, 10), &rules, None, FrameEffects::default());

        let actor = sim.substrate.entities.get(tank).unwrap();
        assert_eq!(actor.mission.effective().known(), Some(MissionType::Attack));
        assert_eq!(actor.attack_target.target(), Some(TargetKind::Entity(tree)));
        assert_eq!(actor.mission.suspended().known(), Some(MissionType::Move));
        assert_eq!(actor.navigation.suspended_nav_com(), Some(destination));
        assert_eq!(actor.navigation.nav_com(), None);

        // This is a component save/restore, with the same external terrain and
        // rules rebound through the existing owners. It tests the retained
        // Object reference and interrupt stack, not app resource admission or
        // native save timing. Native load resets Scenario RNG, so these two
        // continuations assert the loop outcome, not matching clocks/hashes.
        let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
        let speed_config = sim.terrain_speed_config().clone();
        let bytes =
            crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "Terrain blocker interrupt", 0);
        let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
            .expect("represented Terrain target has the existing wire shape")
            .sim;
        restored
            .restore_after_snapshot_load()
            .expect("real Terrain target resolves in its authoritative store");
        restored.resolve_type_handles(&rules);
        restored.rebuild_caches_after_load(terrain, speed_config, &rules);
        assert!(restored.rebuild_dynamic_navigation(&rules));
        let actor = restored.substrate.entities.get(tank).unwrap();
        assert_eq!(actor.attack_target.target(), Some(TargetKind::Entity(tree)));
        assert_eq!(actor.mission.suspended().known(), Some(MissionType::Move));
        assert_eq!(actor.navigation.suspended_nav_com(), Some(destination));

        for sim in [&mut sim, &mut restored] {
            // Continue through ordinary AI/FireAt/Bullet/Terrain damage and the
            // existing clear-before-Restore expiry owner, without a firing seam.
            // Both controls begin at the bounded response, not an ordinary path.
            sim.clear_lifecycle_test_events_for_test();
            let mut fired_at_tree = false;
            for _ in 0..1200 {
                sim.advance_tick(&[], Some(&rules), None, None, 22);
                fired_at_tree |= sim.fire_events.iter().any(|event| {
                    event.attacker_id == tank && event.target == TargetKind::Entity(tree)
                });
                if !sim.terrain_objects.cell_index().contains_key(&(11, 10)) {
                    break;
                }
            }
            assert!(fired_at_tree, "the retained Object target reaches FireAt");
            assert!(
                !sim.terrain_objects.cell_index().contains_key(&(11, 10)),
                "the existing Terrain receiver takes the cannon's Wood damage"
            );
            let actor = sim.substrate.entities.get(tank).unwrap();
            assert_eq!(actor.attack_target.target(), None);
            assert_eq!(actor.mission.suspended(), MissionId::NONE);
            // Foot Restore4D8F80..4D8FA6 reads +5A8 into the destination
            // setter without clearing the retained archive.
            assert_eq!(actor.navigation.suspended_nav_com(), Some(destination));
            assert_eq!(actor.navigation.nav_com(), Some(destination));
            assert_eq!(actor.mission.effective().known(), Some(MissionType::Move));
            assert_eq!(
                sim.lifecycle_test_events_for_test()
                    .iter()
                    .filter(|event| matches!(
                        event, crate::sim::world::LifecycleTestEvent::TerrainExpiryBoundary {
                            stable_id, indexed: true, in_logic: true, live: true, ..
                        } if *stable_id == tree
                    ))
                    .count(),
                4,
                "all four lethal callbacks precede Terrain unlink"
            );
        }
    }
}
