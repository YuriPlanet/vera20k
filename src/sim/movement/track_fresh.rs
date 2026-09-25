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
//! - Unit door (`vt+0x29C` = `0x744180`, Unit+350 DoorClass), asked on every
//!   pass (`0x4B3398..0x4B33A5`): VERA has no Unit door, so the gate always
//!   passes. Trigger: a transport whose door is opening, open or closing
//!   (unload). Effect: a new track starts while native waits for the door to
//!   close. Frequency: the frames after an unload. Risk: none beyond that
//!   timing.
//! - `vt+0x37C` (Unit `0x746C90`: the EMP lock Techno+504 or the Unit
//!   death-frame counter +6D8): neither has a Rust producer (VERA has no EMP
//!   mechanism; see `techno_ai_cloak`), so the prologue gate reads false.
//!   Trigger: an EMP'd or dying Unit. Effect: it could start a track native
//!   refuses. Frequency: no stock EMP source is proven reachable. Risk: this
//!   gate is a consumer of the EMP system when it lands.
//! - Crates (`CellClass::PickupCrate 0x481A00` at `0x4B4062` and
//!   `0x4B46E6`): answered as a cell without a crate (the native early exit
//!   `0x481A39` returns true). Trigger: a head committed onto a crate cell.
//!   Effect: no crate is picked up by a Drive/Ship head. Frequency: crate
//!   games. Risk: crate effects (a separate mechanism) never apply.
//! - Terrain blockers in the code-4/5 arm (`Find_Blocking_Object 0x47C5A0`
//!   returning a TerrainClass): VERA targets only objects and cells, so no
//!   Override is issued for a tree. Unreachable today: VERA's A* never routes
//!   through a terrain object (native prices a Wood route at 20).
//! - The redraw calls (`0x483480`): presentation only; not represented.
//! - Drive+64 (`0x4B400C`, the straight byte): Process_Drive_Track reads it
//!   at `0x4B19AA`/`0x4B1A04` for the wall-crush tilt (+334 = -0.05) and the
//!   CrusherAll Unit flag (+6B5, the speed ramp's crush clamp). Trigger: a
//!   crusher's head forced straight over a crushable overlay, a CrusherAll
//!   wall or a Unit. Effect: neither write happens; VERA's crush arm
//!   (`apply_wall_crush_on_driveover`) runs without the byte and records both
//!   writes as its own residuals. Frequency: crushers over walls and fences,
//!   the Battle Fortress over vehicles. Risk: owned by the crush chain (A7).
//! - A candidate at a negative cell coordinate publishes no target speed
//!   (`publish_fresh_speed`); native reads the shared dummy Cell's land row.
//!   Trigger: a Foot on map row or column 0 heading off the map. Frequency:
//!   none in play (the playfield keeps movers inside). Risk: none.
//! - A train (`IsTrain=`, Type+C94, set by no retail type) extends its last
//!   path word with Find_Path's append form (`0x4B3F07`), which keeps the
//!   queue and appends at most 24 - prefix words (`0x4D3E82`). VERA's route
//!   install replaces the queue, so a train takes the ordinary request.
//! - Units on a Rust route adapter (`issue_direct_move`: refinery and repair
//!   pads, building entry, sell, passengers, grid-less scatter; and component
//!   fixtures without native zone topology) keep the pass lane's own head
//!   selection (`movement_step::select_fresh_drive_track_at_current_cell`
//!   with `DriveCellAdmission`) and target publish
//!   (`track_speed::publish_fresh_target`), a second implementation of this
//!   arm. Trigger: every direct pad approach and direct scatter. Effect:
//!   those moves skip the native gates, responses and speed products.
//!   Frequency: every harvester dock and repair visit. Risk: the dock chain
//!   (native Enter/Unload missions) removes the direct moves.

use super::foot_path::{FindPathResult, FootPathOutcome, coord_cell};
use super::ground_pose;
use super::infantry_entry::InfantryEntryArgs;
use super::movement_tick::FootPathRequest;
use super::track_fresh_dispatch::{
    FreshDispatch, FreshStage, coerce_entry_code, dispatch_entry, normalize_second_direction,
};
use super::track_path::{clear_track_head, track_destination};
use super::track_process::TrackFamily;
use crate::map::entities::EntityCategory;
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone};
use crate::rules::mission_data::MissionType;
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_kernel::native_xyz_distance;
use crate::sim::combat::TargetKind;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::PathGrid;
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
    fallback: Option<&'a PathGrid>,
    registry: Option<&'a OverlayTypeRegistry>,
}

fn path_word(entity: &GameEntity, index: usize) -> Option<u8> {
    entity
        .navigation
        .path_replay
        .remaining_directions()
        .get(index)
        .copied()
}

/// The FacingClass Current of the body (Foot+388), the value 0x4C93D0
/// returns; an entity without a retained FacingClass rests at its facing.
fn body_facing_current(entity: &GameEntity, frame: u32) -> u16 {
    entity
        .body_facing
        .as_ref()
        .map_or(u16::from(entity.facing) << 8, |facing| {
            facing.current(frame)
        })
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
        fallback: Option<&PathGrid>,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Result<bool, String> {
        let call = FreshCall {
            id,
            family,
            args,
            rules,
            fallback,
            registry,
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
                return Ok(self.track_enter_idle_mode(id, Some(call.rules)));
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
        //parasite arms), return with the out byte clear.
        if actor.is_paralyzed(self.session.binary_frame) {
            return Ok(false);
        }
        let head = if head.is_some() {
            self.trim_path_for_object_navcom(id);
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

    /// 0x4B2770..0x4B2813: while NavCom is a Unit or an Infantry, the path is
    /// cut at the cell count of |Foot - destination| (Distance3D >> 8) when
    /// that is below 24, so a chase re-plans as it closes in.
    fn trim_path_for_object_navcom(&mut self, id: u64) {
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let object = match actor.navigation.nav_com {
            Some(
                NavTargetRef::Entity { id: object }
                | NavTargetRef::Object { id: object }
                | NavTargetRef::Building { id: object },
            ) => object,
            _ => return,
        };
        if !self.substrate.entities.get(object).is_some_and(|nav| {
            matches!(
                nav.category,
                EntityCategory::Unit | EntityCategory::Infantry
            )
        }) {
            return;
        }
        let Some(destination) = track_destination(actor) else {
            return;
        };
        let location = ground_pose::position_world_coord(&actor.position);
        let distance = crate::sim::cell_kernel::native_coord_distance(
            location.x.wrapping_sub(destination.x),
            location.y.wrapping_sub(destination.y),
            location.z.wrapping_sub(destination.z),
        );
        let cells = distance.wrapping_add((distance >> 31) & 0xFF) >> 8;
        if cells >= 24 {
            return;
        }
        let queue = &mut self
            .substrate
            .entities
            .get_mut(id)
            .expect("same NavCom trim owner")
            .navigation
            .path_replay;
        let slot = usize::from(queue.cursor) + cells as usize;
        if let Some(word) = queue.directions.get_mut(slot) {
            *word = u8::MAX;
        }
    }

    /// 0x4B281C..0x4B2845 then track_path's request and continuations. A found
    /// route resumes head selection in the same call (0x4B3282..0x4B3298).
    fn track_no_queue_arm(&mut self, call: &FreshCall<'_>) -> Result<bool, String> {
        let id = call.id;
        let frame = self.session.binary_frame as i32;
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship no-queue owner")?;
        //4B281C..4B2845: the Foot+640 exact-zero wait.
        if !actor.navigation.path_runtime.movement_timer.expired(frame) {
            return Ok(false);
        }
        let destination = track_destination(actor).ok_or("no-queue arm without destination")?;
        let request = self.track_path_request(call, destination, 0)?;
        match self.run_track_path_request(
            &request,
            None,
            Some(call.rules),
            call.fallback,
            call.registry,
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
            self.playfield_bounds,
            Some(&self.type_handles),
            Some(call.rules),
        )
        .ok_or_else(|| "retired Drive/Ship Find_Path requester".into())
    }

    /// The fresh arm, from 0x4B3298 with the live path head `direction`.
    fn track_fresh_arm(&mut self, call: &FreshCall<'_>, direction: u8) -> Result<bool, String> {
        let id = call.id;
        let rules = call.rules;
        //4B3298: a tube word returns; the tube receiver owns it.
        if direction == TUBE_STEP_DIRECTION {
            return Ok(false);
        }
        let frame = self.session.binary_frame;
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
        let height = ground_pose::query_object_cell_height(&cells, location, actor.on_bridge);
        //4B3376..4B3391: the candidate Cell's bridge bit against OnBridge.
        let bridge_mismatch = (cells.flags(cells.lookup(cell)) & 0x100 != 0) != actor.on_bridge;
        let owner = self.interner.resolve(actor.owner()).to_owned();
        if bridge_mismatch {
            self.latch_foot_68b(id);
        }
        //4B3398..4B33A5: Unit+29C (door closed) is always true; see the
        //residual.
        //4B33B3..4B33FA: an allied gate that is not open yet holds the Foot.
        if !crate::sim::gate_runtime::request_gate_open_for_cell(
            &mut self.substrate.entities,
            &self.substrate.occupancy,
            (cell.0 as u16, cell.1 as u16),
            id,
            &owner,
            rules,
            &self.house_alliances,
            &self.interner,
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
        let desired = u16::from(direction) << 13;
        if body_facing_current(actor, frame) != desired {
            self.track_do_turn(id, desired);
            return Ok(false);
        }
        //4B345B..4B34D1: Mark0, Unit+1AC(cell, dir, height, 0, 1), Mark1.
        self.foot_mark_remove(id, Some(rules), call.fallback, call.registry);
        let code = self.track_can_enter(call, cell, direction, height)?;
        self.foot_mark_put(id, Some(rules), call.fallback, call.registry);
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
            FreshDispatch::Redraw { retry } => {
                //4B394D..4B3984: the redraw, then the retry recursion.
                if retry.is_some() {
                    self.clear_path_head(id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B398E..4B39CC then 4B31FC: clear the head, stop or take
                //the next waypoint.
                self.stop_or_take_next_waypoint(id, rules);
                Ok(false)
            }
            FreshDispatch::BlockedDelay => self.track_blocked_delay(call),
            FreshDispatch::Gate { .. } => {
                //4B35EC..4B3602: the gate question, answer discarded.
                let _ = crate::sim::gate_runtime::request_gate_open_for_cell(
                    &mut self.substrate.entities,
                    &self.substrate.occupancy,
                    (cell.0 as u16, cell.1 as u16),
                    id,
                    &owner,
                    rules,
                    &self.house_alliances,
                    &self.interner,
                );
                self.track_first_rejected_tail(id);
                Ok(false)
            }
            FreshDispatch::WallOrObject { retry } => {
                self.clear_track_head_of(id);
                if retry.is_some() {
                    //4B3AD3..4B3AFE then 4B4541: drop the head, expire the
                    //movement timer and recurse.
                    self.clear_path_head(id);
                    self.expire_movement_timer(id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B3B03..4B3BE9: attack the non-allied blocking object, or
                //the wall cell when the cell holds none.
                self.track_override_blocker(call, cell);
                //4B3C67..4B3C81: code != 7 retires the selector.
                self.track_retire_selector(id);
                Ok(false)
            }
            FreshDispatch::ScatterOrStop { retry, .. } => {
                if retry.is_some() {
                    //4B3721..4B373D then 4B4541.
                    self.clear_path_head(id);
                    self.expire_movement_timer(id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B3742..4B38A1: close enough to the destination, level and
                //off a Tunnel: stop or take the next waypoint.
                if self.track_close_enough_stop(id, rules)? {
                    if self.stop_or_take_next_waypoint(id, rules) {
                        //4B38A7: an entered idle mode returns at once.
                        return Ok(false);
                    }
                } else {
                    //4B38B3..4B393A: Scatter_Objects(Null, 1, 1, deck).
                    self.scatter_blocked_track_cell(id, cell, rules, call.fallback);
                }
                self.track_first_rejected_tail(id);
                Ok(false)
            }
            FreshDispatch::FirstOtherBlocked { retry } => {
                //4B3607 then 4B3AA1..4B3ACE: the ScoldSound latch Foot+68A
                //has no writer in the program, so no voice is represented.
                self.clear_track_head_of(id);
                if retry.is_some() {
                    //4B3BF6..4B3C21 then 4B4541.
                    self.clear_path_head(id);
                    self.expire_movement_timer(id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B3C26..4B3C62 then 4B31FC.
                self.stop_or_take_next_waypoint(id, rules);
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

    /// 0x4B3607 then 0x4B3AA1..0x4B3C81 for a code other than 2/4/5/7: clear
    /// the head; the ScoldSound latch Foot+68A has no writer, so no voice;
    /// Foot+68A = 0 and the selector retires.
    fn track_first_rejected_tail(&mut self, id: u64) {
        self.clear_track_head_of(id);
        self.track_retire_selector(id);
    }

    /// 0x4B3607..0x4B3A94: the first candidate's code-2 ladder.
    fn track_blocked_delay(&mut self, call: &FreshCall<'_>) -> Result<bool, String> {
        let id = call.id;
        let rules = call.rules;
        let frame = self.session.binary_frame;
        self.clear_track_head_of(id);
        let actor = self
            .substrate
            .entities
            .get_mut(id)
            .ok_or("retired Drive/Ship code-2 owner")?;
        let runtime = &mut actor.navigation.path_runtime;
        //4B3656..4B368D: the latch arms the blockage timer once.
        if !runtime.path_blocked {
            runtime.path_blocked = true;
            runtime.start_blocked(frame, rules.general.blockage_path_delay_ticks);
        }
        //4B3690..4B36B6: while the movement timer runs, the rejected tail.
        if !runtime.movement_timer.expired(frame as i32) {
            self.track_first_rejected_tail(id);
            return Ok(false);
        }
        //4B36BC..4B36EF / 4B39D1..4B3A0E: urgency 2 once the latched
        //blockage timer expired, else 1; Find_Path(cell(destination), 0, u).
        let urgency = if runtime.path_blocked && runtime.blocked_timer.expired(frame as i32) {
            2
        } else {
            1
        };
        let destination =
            track_destination(actor).ok_or("Drive/Ship code-2 ladder without destination")?;
        let request = self.track_path_request(call, destination, urgency)?;
        let found = self.foot_find_path(&request, None, rules, call.fallback, call.registry)?;
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
                    .path_runtime
                    .start_movement(frame, rules.general.path_delay_ticks());
            }
            return Ok(false);
        }
        //4B3A3E..4B3A56: SetDestination(NULL, 1).
        self.set_unit_null_destination(id, Some(rules));
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
        self.track_crusher_pre_scatter(call, cell);
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired Drive/Ship accept owner")?;
        let mut second = path_word(actor, 1).map_or(-1, i32::from);
        //4B3E6B..4B3F74: the last path word with the destination more than
        //two cells out asks Find_Path for the next words.
        if second == -1 {
            let destination =
                track_destination(actor).ok_or("Drive/Ship accept arm without destination")?;
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
                    self.foot_find_path(&request, None, rules, call.fallback, call.registry)?;
                if found == FindPathResult::Failed {
                    //4B3F40..4B3F55: a vanished Foot sets the out byte.
                    if self.substrate.entities.get(id).is_none() {
                        return Ok(true);
                    }
                    //4B3F58..4B3F6E: an unreachable live destination clears.
                    if !self.foot_path_zone_precheck(id, self.live_track_destination(id), rules)? {
                        self.set_unit_null_destination(id, Some(rules));
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
                                    && self.cell_first_unit(next_cell).is_some())))
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
        let turn_index = super::drive_track::fresh_turn_index(
            direction,
            second as u8,
            call.family == TrackFamily::Ship,
        );
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            let kind = actor
                .locomotor
                .as_ref()
                .map_or(LocomotorKind::Drive, |l| l.kind);
            super::track_head::select_fresh_progress(
                kind,
                &mut actor.drive_locomotion,
                &mut actor.ship_locomotion,
                turn_index,
            );
        }
        let two_node = super::drive_track::turn_track_at(turn_index)
            .is_some_and(|turn| turn.flags & super::drive_track::TURN_TRACK_TURNS_FLAG != 0);
        if !two_node {
            //4B45F6..4B4609: shift one word, then the finalize tail.
            return self.track_fresh_finalize(call, Some(candidate), 1, turn_index);
        }
        //4B404B..4B4092: the crate question on the first cell (see residual).
        if !self.track_owner_alive(id) {
            return Ok(false);
        }
        //4B40A3..4B4126: the second candidate, without the Mark bracket.
        let second_candidate = super::track_head::offset_head(candidate, second as u8);
        let second_cell = coord_cell(second_candidate);
        let code = self.track_can_enter(call, second_cell, second as u8, height2)?;
        let overlay = self.track_overlay(second_cell);
        let code = coerce_entry_code(
            code,
            object.is_train,
            object.crusher,
            overlay.map_or(-1, i32::from),
        )
        .ok_or("Unit+1AC answered outside 0..7")?;
        //4B4179..4B4184: the second query's life check.
        let alive = self.track_owner_alive(id);
        let dispatch = dispatch_entry(FreshStage::Second, code, call.args.allow_retry, alive)
            .ok_or("fresh dispatch outside 0..7")?;
        match dispatch {
            FreshDispatch::OwnerNotAlive => Ok(false),
            FreshDispatch::Accept => {
                //4B45CB..4B45F4: shift two words, Foot+68B = 1, then the
                //finalize tail.
                self.latch_foot_68b(id);
                self.track_fresh_finalize(call, Some(second_candidate), 2, turn_index)
            }
            FreshDispatch::Gate { .. } => {
                //4B419B..4B41AE: the gate question, answer discarded.
                let owner = self.track_owner_name(id);
                let _ = crate::sim::gate_runtime::request_gate_open_for_cell(
                    &mut self.substrate.entities,
                    &self.substrate.occupancy,
                    (second_cell.0 as u16, second_cell.1 as u16),
                    id,
                    &owner,
                    rules,
                    &self.house_alliances,
                    &self.interner,
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
                self.clear_path_head(id);
                self.track_retire_selector(id);
                self.track_process_movement(&call.with_args(ProcessMovementArgs {
                    allow_retry: retry.allow_retry,
                    force_single: retry.force_single_direction,
                }))
            }
            FreshDispatch::ScatterOrStop { retry, .. } => {
                if retry.is_some() {
                    //4B424E..4B426E then 4B4541.
                    self.clear_path_head(id);
                    self.expire_movement_timer(id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B4273..4B43BE: the CloseEnough stop.
                if self.track_close_enough_stop(id, rules)? {
                    if self.stop_or_take_next_waypoint(id, rules) {
                        return Ok(false);
                    }
                } else {
                    //4B43D0..4B4437: Scatter_Objects on the second cell.
                    self.scatter_blocked_track_cell(id, second_cell, rules, call.fallback);
                }
                self.track_second_refused(call)
            }
            FreshDispatch::Redraw { retry } => {
                //4B444A..4B4485: the redraw, then the retry recursion.
                if retry.is_some() {
                    self.clear_path_head(id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B448F..4B450D.
                self.stop_or_take_next_waypoint(id, rules);
                Ok(false)
            }
            FreshDispatch::SecondRetryOrStop { retry } => {
                if retry.is_some() {
                    //4B4521..4B453D then 4B4541.
                    self.clear_path_head(id);
                    self.expire_movement_timer(id);
                    return self
                        .track_process_movement(&call.with_args(ProcessMovementArgs::RETRY));
                }
                //4B4561..4B45C8.
                self.stop_or_take_next_waypoint(id, rules);
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
        self.clear_path_head(call.id);
        self.track_retire_selector(call.id);
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
        clear_track_head(actor);
        let Some(candidate) = candidate else {
            actor.navigation.path_replay.reference_cell = Some(reference);
            return self.track_finalize_stop(id);
        };
        super::path_markers::accept_path_replay(
            &mut actor.navigation.path_replay,
            reference,
            consumed,
        );
        //4B46C3..4B46D7: install the selector and head (+63 = 1).
        super::track_head::accept_fresh_progress(
            actor
                .locomotor
                .as_ref()
                .map_or(LocomotorKind::Drive, |l| l.kind),
            &mut actor.drive_locomotion,
            &mut actor.ship_locomotion,
            turn_index,
        );
        match call.family {
            TrackFamily::Drive => {
                if let Some(drive) = actor.drive_locomotion.as_mut() {
                    drive.head_to = Some(candidate);
                }
            }
            TrackFamily::Ship => {
                if let Some(ship) = actor.ship_locomotion.as_mut() {
                    ship.head_to = Some(candidate);
                }
            }
        }
        //4B46DF..4B46FA: the crate question (see residual), then limbo.
        if !actor.lifecycle.in_limbo {
            //4B46FC..4B4705: Apply_Track_Occupation_Mode(head, 1).
            self.track_apply_occupation(id, call.family, true, call.fallback);
            return Ok(false);
        }
        //4B4716..4B473C: a live owner drops the head again.
        if actor.lifecycle.object_alive {
            clear_track_head(actor);
        }
        self.track_finalize_stop(id)
    }

    /// 0x4B4740..0x4B4764: selector -1, path word -1, SetSpeedFraction(0).
    fn track_finalize_stop(&mut self, id: u64) -> Result<bool, String> {
        self.track_retire_selector(id);
        self.clear_path_head(id);
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.foot_speed.applied_fraction = crate::util::fixed_math::SIM_ZERO;
        }
        Ok(false)
    }

    /// The live Unit+1AC answer for one candidate, arguments (cell, dir, h, 0, 1).
    fn track_can_enter(
        &mut self,
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
        rules
            .object(self.interner.resolve(actor.type_ref()))
            .ok_or_else(|| "Drive/Ship owner without ObjectType".into())
    }

    fn track_owner_name(&self, id: u64) -> String {
        self.substrate
            .entities
            .get(id)
            .map_or_else(String::new, |actor| {
                self.interner.resolve(actor.owner()).to_owned()
            })
    }

    /// Cell+44, the overlay index of a Cell (the shared dummy has its own).
    fn track_overlay(&self, cell: (i16, i16)) -> Option<u8> {
        let terrain = self.resolved_terrain.as_ref()?;
        match terrain.native_cell_identity(cell) {
            crate::map::cell_index::NativeCellIdentity::Real(index) => {
                terrain.cells()[index].bridge_facts.overlay_id
            }
            crate::map::cell_index::NativeCellIdentity::Dummy => {
                u8::try_from(terrain.shared_cell_dummy().overlay_identity_state().0).ok()
            }
        }
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
            .and_then(|actor| rules.object(self.interner.resolve(actor.type_ref())))
            .map(|object| object.strength)
        else {
            return;
        };
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        super::track_speed::publish_fresh_track_target(
            actor,
            strength,
            rules,
            terrain,
            &self.terrain_speed_config,
            (cell.0 as u16, cell.1 as u16),
            road,
        );
    }

    /// CellClass::FindFirstUnit(0) 0x47EBA0: the first Unit of the ground list.
    fn cell_first_unit(&self, cell: (i16, i16)) -> Option<u64> {
        if cell.0 < 0 || cell.1 < 0 {
            return None;
        }
        self.substrate
            .occupancy
            .get(cell.0 as u16, cell.1 as u16)?
            .iter_layer(MovementLayer::Ground)
            .map(|entry| entry.entity_id)
            .find(|&id| {
                self.substrate
                    .entities
                    .get(id)
                    .is_some_and(|entity| entity.category == EntityCategory::Unit)
            })
    }

    fn clear_track_head_of(&mut self, id: u64) {
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            clear_track_head(actor);
        }
    }

    /// Foot+68B = 1 (0x4B3391, 0x4B45ED): write-once in the program (the
    /// Foot constructor zeroes it; nothing clears it), read only by
    /// `FootClass::ComputeChecksum` (0x4DBD0C). Its persisted analogue is
    /// the hashed `RuntimeBridgeTransitionState::pending_mismatch`.
    fn latch_foot_68b(&mut self, id: u64) {
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.runtime_bridge_transition.pending_mismatch = true;
        }
    }

    /// The live Destination (+34) that 0x4B39D6 / 0x4B3E8F address and hand
    /// to +2CC after Find_Path: Find_Path's core-failure receiver (+500 ->
    /// locomotor Stop) nulls it, and a null reads as Cell (0,0).
    fn live_track_destination(&self, id: u64) -> DriveCoord {
        self.substrate
            .entities
            .get(id)
            .and_then(track_destination)
            .unwrap_or(DriveCoord { x: 0, y: 0, z: 0 })
    }

    /// Foot+5E0 = -1 on the live path word. The scheduling adapter's route is
    /// a cache of the same words, so it empties with them; otherwise the next
    /// visit would read the Foot as a Rust route adapter.
    fn clear_path_head(&mut self, id: u64) {
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.navigation.path_replay.clear_live_head();
            if let Some(target) = actor.movement_target.as_mut() {
                target.path.clear();
                target.path_layers.clear();
                target.next_index = 0;
            }
        }
    }

    /// Foot+640 = (Frame, 0) before a retry recursion (0x4B4541..0x4B4548).
    fn expire_movement_timer(&mut self, id: u64) {
        let frame = self.session.binary_frame;
        if let Some(actor) = self.substrate.entities.get_mut(id) {
            actor.navigation.path_runtime.start_movement(frame, 0);
        }
    }

    /// Class selector +58 = -1 (Foot+68A, cleared beside it, has no
    /// nonzero writer in the program).
    fn track_retire_selector(&mut self, id: u64) {
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        match actor.locomotor.as_ref().map(|loco| loco.kind) {
            Some(LocomotorKind::Drive) => {
                if let Some(drive) = actor.drive_locomotion.as_mut() {
                    drive.track.turn_index = -1;
                }
            }
            Some(LocomotorKind::Ship) => {
                if let Some(ship) = actor.ship_locomotion.as_mut() {
                    ship.track.turn_index = -1;
                }
            }
            _ => {}
        }
    }

    /// Do_Turn 0x4B0EF0: FacingClass::Set 0x4C9220 on the body facing.
    fn track_do_turn(&mut self, id: u64, desired: u16) {
        let frame = self.session.binary_frame;
        let Some(actor) = self.substrate.entities.get_mut(id) else {
            return;
        };
        super::drive_do_turn(actor, desired, frame);
        if let Some(body) = actor.body_facing.as_ref() {
            actor.facing = (body.current(frame) >> 8) as u8;
        }
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
        let destination = track_destination(actor).unwrap_or(DriveCoord { x: 0, y: 0, z: 0 });
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
        let land = match cells.lookup_world(location.x, location.y) {
            crate::map::cell_index::NativeCellIdentity::Real(index) => {
                terrain.cells()[index].yr_cell_land_type
            }
            crate::map::cell_index::NativeCellIdentity::Dummy => 0,
        };
        land != LAND_TUNNEL
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
        fallback: Option<&PathGrid>,
    ) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let Some(actor) = self.substrate.entities.get(id) else {
            return;
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let native = cells.lookup(cell);
        let level = i32::from(cells.ground_fields(native).0 as i8);
        let location = ground_pose::position_world_coord(&actor.position);
        let deck = cells.flags(native) & 0x100 != 0
            && (location.z / GROUND_LEVEL_HEIGHT_LEPTONS - level).abs() > 2;
        self.scatter_track_cell(cell, deck, true, rules, fallback);
    }

    fn scatter_track_cell(
        &mut self,
        cell: (i16, i16),
        deck: bool,
        forced: bool,
        rules: &RuleSet,
        fallback: Option<&PathGrid>,
    ) {
        #[cfg(test)]
        if super::fresh_oracle_seam::substitute(
            super::fresh_oracle_seam::FreshCallRecord::Scatter { cell, forced, deck },
        ) {
            return;
        }
        if cell.0 < 0 || cell.1 < 0 {
            return;
        }
        let grid = self.path_grid_snapshot();
        super::bump_crush::scatter_cell_objects(
            &mut self.substrate.entities,
            &self.substrate.occupancy,
            (cell.0 as u16, cell.1 as u16),
            if deck {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            },
            forced,
            grid.as_deref().or(fallback),
            self.resolved_terrain.as_ref(),
            &mut self.scenario_rng,
            Some(rules),
            &self.interner,
            &self.houses,
            super::DestinationTiming::new(
                self.session.binary_frame,
                rules.general.blockage_path_delay_ticks,
            ),
        );
    }

    /// Unit+534(cell, 1) = 0x7416A0 with its second argument set: a Crusher
    /// (Type+D28 or ability 0x11) scatters a Cell holding infantry (raw
    /// +124/+128 & 0x1F) without force. The deck list applies on a structural
    /// Cell the Foot rides or reaches from four levels above.
    fn track_crusher_pre_scatter(&mut self, call: &FreshCall<'_>, cell: (i16, i16)) {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let Some(actor) = self.substrate.entities.get(call.id) else {
            return;
        };
        let Some(object) = call.rules.object(self.interner.resolve(actor.type_ref())) else {
            return;
        };
        //Type+D28, or HasWeaponAbility(0x11) by rank (0x70D0D0).
        let crusher = object.crusher
            || (actor.veterancy >= 100 && object.veteran_crusher)
            || (actor.veterancy >= 200 && object.elite_crusher);
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let native = cells.lookup(cell);
        let deck = cells.flags(native) & 0x100 != 0 && {
            let level = i32::from(cells.ground_fields(native).0 as i8);
            let location = ground_pose::position_world_coord(&actor.position);
            let here = cells.lookup_world(location.x, location.y);
            actor.on_bridge || i32::from(cells.ground_fields(here).0 as i8) == level + 4
        };
        if !crusher {
            return;
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
            self.scatter_track_cell(cell, deck, false, call.rules, call.fallback);
        }
    }

    /// Code 4/5 without a retry (0x4B3B03..0x4B3BE9): `Find_Blocking_Object`
    /// then Override(Attack) on a non-allied object, else on a wall cell.
    fn track_override_blocker(&mut self, call: &FreshCall<'_>, cell: (i16, i16)) {
        if cell.0 < 0 || cell.1 < 0 {
            return;
        }
        let key = (cell.0 as u16, cell.1 as u16);
        match self.find_blocking_object(key, call.rules) {
            Some(BlockingObject::Entity(blocker)) => {
                let (Some(actor), Some(target)) = (
                    self.substrate.entities.get(call.id),
                    self.substrate.entities.get(blocker),
                ) else {
                    return;
                };
                //4B3B52..4B3B62: the mover's House asks about the object.
                if crate::map::houses::is_allied_with(
                    &self.house_alliances,
                    self.interner.resolve(actor.owner()),
                    self.interner.resolve(target.owner()),
                ) {
                    return;
                }
                self.track_override(call, TargetKind::Entity(blocker));
            }
            Some(BlockingObject::Terrain) => {
                //See the terrain-blocker residual: no terrain target exists.
            }
            None => {
                //4B3B94..4B3BE3: a wall overlay makes the Cell the target.
                let wall = self
                    .track_overlay(cell)
                    .and_then(|overlay| self.overlay_flags(call.registry, overlay))
                    .is_some_and(|(_, wall)| wall);
                if wall {
                    self.track_override(call, TargetKind::Cell(key.0, key.1));
                }
            }
        }
    }

    /// Foot::Override_Mission(Attack, target, NULL) at 0x4B3BE9.
    fn track_override(&mut self, call: &FreshCall<'_>, target: TargetKind) {
        #[cfg(test)]
        if super::fresh_oracle_seam::substitute(
            super::fresh_oracle_seam::FreshCallRecord::Override { target },
        ) {
            return;
        }
        self.mission_override_track_blocker(call.id, target, call.rules);
    }

    /// `CellClass::Find_Blocking_Object 0x47C5A0` with the zero point: the
    /// first Aircraft of the ground list, else `Find_Nearest_Object`
    /// (0x47C3D0), else the first terrain object.
    fn find_blocking_object(&self, cell: (u16, u16), rules: &RuleSet) -> Option<BlockingObject> {
        let list = self.substrate.occupancy.get(cell.0, cell.1);
        if let Some(aircraft) = list
            .into_iter()
            .flat_map(|list| list.iter_layer(MovementLayer::Ground))
            .find(|entry| {
                self.substrate
                    .entities
                    .get(entry.entity_id)
                    .is_some_and(|entity| entity.category == EntityCategory::Aircraft)
            })
        {
            return Some(BlockingObject::Entity(aircraft.entity_id));
        }
        let nearest = self.nearest_cell_object(cell, MovementLayer::Ground, rules);
        if let Some(object) = nearest {
            return Some(BlockingObject::Entity(object));
        }
        self.production
            .terrain_object_cells
            .contains_key(&cell)
            .then_some(BlockingObject::Terrain)
    }
}

/// What `Find_Blocking_Object` returned.
enum BlockingObject {
    Entity(u64),
    Terrain,
}

/// LandType Tunnel (10), Cell+EC.
const LAND_TUNNEL: u8 = 10;

impl FreshCall<'_> {
    fn with_args(&self, args: ProcessMovementArgs) -> Self {
        Self { args, ..*self }
    }
}
