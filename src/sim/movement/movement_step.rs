//! Movement step helpers — cell transition mechanics, vehicle rotation, lepton advancement,
//! and cell boundary crossing detection.
//!
//! Contains the inner-loop logic extracted from `tick_movement_with_grids`: how a mover
//! rotates in place, advances sub-cell position, detects cell boundary crossings, and
//! performs the actual cell transition with occupancy/terrain checks.

use std::collections::BTreeSet;

use super::cell_arrival::CellArrival;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{
    DriveCoord, DriveLocomotionRuntime, DriveOccupationFootprint, MovementTarget, Position,
    ShipLocomotionRuntime,
};
use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::movement::bump_crush;
use crate::sim::movement::drive_track;
use crate::sim::movement::locomotor::{GroundMovePhase, LocomotorState, MovementLayer};
use crate::sim::movement::movement_blocked::handle_blocked_tick;
use crate::sim::movement::movement_bridge::resolve_cell_transition_bridge_state;
use crate::sim::movement::movement_occupancy::{
    BuildingEntrySkipLookup, DeferredCellCheck, detect_deferred_cell_check,
    evaluate_runtime_can_enter_cell_with_transition, naval_terrain_diag,
    runtime_can_enter_cell_args,
};
use crate::sim::occupancy::{CellOccupationGrid, OccupancyGrid};
use crate::sim::pathfinding::LayeredEntityBlockMap;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::rng::SimRng;
use crate::sim::world::EnterOrderCounter;
use crate::util::fixed_math::{
    SIM_HALF, SIM_ONE, SIM_ZERO, SimFixed, facing_from_delta_int as facing_from_delta,
    fixed_distance,
};
use crate::util::lepton::CELL_CENTER_LEPTON;

use super::{
    CLIFF_HEIGHT_THRESHOLD, MovementConfig, MovementTickStats, MoverSnapshot, PATH_STUCK_INIT,
    PathfindingContext,
};

fn shared_track_kind(locomotor: &Option<LocomotorState>) -> Option<LocomotorKind> {
    locomotor
        .as_ref()
        .map(|locomotor| locomotor.kind)
        .filter(|kind| matches!(kind, LocomotorKind::Drive | LocomotorKind::Ship))
}

/// Cell delta of the path step *after* the head node — gamemd's `path[1]`
/// direction, the second index term of the turn table.
///
/// `None` at the last step of a path, which the selector normalises to the head
/// node's own direction (gamemd's `-1` queue terminator does the same).
fn path_window_to_delta(target: &MovementTarget) -> Option<(i32, i32)> {
    let head = target.path.get(target.next_index)?;
    let after = target.path.get(target.next_index + 1)?;
    Some((
        i32::from(after.0) - i32::from(head.0),
        i32::from(after.1) - i32::from(head.1),
    ))
}

#[allow(clippy::too_many_arguments)]
fn accept_shared_track(
    path_replay: &mut crate::sim::components::FootPathQueue,
    kind: LocomotorKind,
    drive_locomotion: &mut Option<DriveLocomotionRuntime>,
    ship_locomotion: &mut Option<ShipLocomotionRuntime>,
    endpoint: (i16, i16),
    endpoint_coord: DriveCoord,
    consumed_directions: usize,
    turn_index: usize,
) {
    super::track_head::accept_fresh_progress(kind, drive_locomotion, ship_locomotion, turn_index);
    match kind {
        LocomotorKind::Drive => {
            if let Some(drive) = drive_locomotion.as_mut() {
                drive.head_to = Some(endpoint_coord);
                super::path_markers::accept_path_replay(path_replay, endpoint, consumed_directions);
            }
        }
        LocomotorKind::Ship => {
            if let Some(ship) = ship_locomotion.as_mut() {
                super::path_markers::accept_path_replay(path_replay, endpoint, consumed_directions);
                ship.head_to = Some(endpoint_coord);
            }
        }
        _ => {}
    }
}

pub(super) fn apply_cell_transition_remainder(
    path_runtime: &mut crate::sim::components::FootPathRuntime,
    position: &mut Position,
    dx_cell: i32,
    dy_cell: i32,
    nx: u16,
    ny: u16,
    is_infantry: bool,
    native_frame: u32,
    walk: bool,
) {
    // Infantry: clear blocking state on each cell arrival (fresh grace period).
    // Vehicles: keep both flags — once blocked, urgency escalates permanently.
    if is_infantry {
        // Walk75BE11/75BFD1 clears the latch but retains the grace timer.
        if !walk {
            path_runtime.start_blocked(native_frame, 0);
        }
        path_runtime.path_blocked = false;
    }
    if dx_cell > 0 {
        position.sub_x -= crate::util::lepton::LEPTONS_PER_CELL;
    } else if dx_cell < 0 {
        position.sub_x += crate::util::lepton::LEPTONS_PER_CELL;
    }
    if dy_cell > 0 {
        position.sub_y -= crate::util::lepton::LEPTONS_PER_CELL;
    } else if dy_cell < 0 {
        position.sub_y += crate::util::lepton::LEPTONS_PER_CELL;
    }
    position.rx = nx;
    position.ry = ny;
}

pub(super) fn configure_motion_after_transition(
    target: &mut MovementTarget,
    locomotor: &Option<LocomotorState>,
    facing: &mut u8,
    facing_target: &mut Option<u8>,
    category: EntityCategory,
    mover_rot: i32,
    position: &Position,
) {
    let current_cell = (position.rx, position.ry);
    let current_sub = (position.sub_x, position.sub_y);
    target.next_index += 1;
    if target.next_index < target.path.len() {
        let next = target.path[target.next_index];
        let ndx = next.0 as i32 - current_cell.0 as i32;
        let ndy = next.1 as i32 - current_cell.1 as i32;

        let new_face = facing_from_delta(ndx, ndy);
        if category == EntityCategory::Infantry || mover_rot <= 0 {
            *facing = new_face;
        } else {
            *facing_target = Some(new_face);
        }

        if category == EntityCategory::Infantry {
            // Infantry: direction from current sub-cell toward next cell's subcell position.
            // Use the allocated subcell offset to maintain visual spread during movement,
            // matching the WalkLocomotionClass which walks to FindSubCellDest result.
            let (sc_x, sc_y) = locomotor
                .as_ref()
                .and_then(|l| l.subcell_dest)
                .unwrap_or((CELL_CENTER_LEPTON, CELL_CENTER_LEPTON));
            let dest_x = SimFixed::from_num(ndx * 256) + sc_x;
            let dest_y = SimFixed::from_num(ndy * 256) + sc_y;
            let dx = dest_x - current_sub.0;
            let dy = dest_y - current_sub.1;
            target.move_dir_x = dx;
            target.move_dir_y = dy;
            target.move_dir_len = fixed_distance(dx, dy);
        } else {
            let (d_x, d_y, d_len) = crate::util::lepton::cell_delta_to_lepton_dir(ndx, ndy);
            target.move_dir_x = d_x;
            target.move_dir_y = d_y;
            target.move_dir_len = d_len;
        }
    } else if let Some(loco) = locomotor {
        if let Some((dest_x, dest_y)) = loco.subcell_dest {
            let dx = dest_x - current_sub.0;
            let dy = dest_y - current_sub.1;
            target.move_dir_x = dx;
            target.move_dir_y = dy;
            let len: SimFixed = fixed_distance(dx, dy);
            target.move_dir_len = if len > SIM_HALF { len } else { SIM_ONE };
        }
    }
}

/// Per-tick hover steering: turn the body facing toward the current one-cell
/// waypoint through the native-frame `FacingClass` at the unit's rules ROT, and
/// point `move_dir` along the resulting hull heading (unit vector, len = 1) so
/// the shared lepton advancement produces facing-lagged curved motion.
///
/// Returns `true` while the required turn exceeds 45° (the turn-stall): the
/// caller brakes the throttle (request 0) and holds position for the tick.
/// Holding position during the hard-turn phase is a disclosed approximation —
/// the original translates along the stale heading while braking, but the
/// path-directed crossing loop cannot absorb a sideways cell exit; the drift
/// this suppresses is bounded by the brake-decay tail (see the P2b plan doc).
///
/// Hover never uses the stop-rotate-go path (`handle_vehicle_rotation`); any
/// `facing_target` left by shared path plumbing is cleared here.
pub(super) fn hover_steer(
    facing: &mut u8,
    facing_target: &mut Option<u8>,
    body_facing: &mut Option<super::facing_class::FacingClass>,
    position: &Position,
    target: &mut MovementTarget,
    rot: i32,
    native_frame: u32,
) -> bool {
    use crate::util::lepton::CELL_CENTER_LEPTON;

    *facing_target = None;
    let (wx, wy): (u16, u16) = if target.next_index < target.path.len() {
        target.path[target.next_index]
    } else {
        target.final_goal.unwrap_or((position.rx, position.ry))
    };
    let dxl: SimFixed = SimFixed::from_num((wx as i32 - position.rx as i32) * 256)
        + (CELL_CENTER_LEPTON - position.sub_x);
    let dyl: SimFixed = SimFixed::from_num((wy as i32 - position.ry as i32) * 256)
        + (CELL_CENTER_LEPTON - position.sub_y);
    if dxl == SIM_ZERO && dyl == SIM_ZERO {
        // Already exactly on the waypoint — nothing to steer toward.
        *body_facing = None;
        return false;
    }

    let desired16: u16 = super::hover::hover_desired_facing16(dxl, dyl);
    let rot_byte: u8 = rot.clamp(0, 0x7F) as u8;
    let bf = body_facing.get_or_insert_with(|| {
        super::facing_class::FacingClass::new((*facing as u16) << 8, rot_byte)
    });
    bf.set(desired16, native_frame);
    let current16: u16 = bf.current(native_frame);
    *facing = (current16 >> 8) as u8;

    let (mx, my) = super::hover::hover_move_dir(current16);
    target.move_dir_x = mx;
    target.move_dir_y = my;
    target.move_dir_len = SIM_ONE;

    super::hover::hover_turning_hard(current16, desired16)
}

/// Result of vehicle rotation — tells the caller whether to skip this tick.
pub(super) enum RotationResult {
    /// Still rotating in place — caller should `continue` (skip lepton advancement).
    StillRotating {
        debug_events: Vec<(u32, DebugEventKind)>,
    },
    /// Rotation complete or not needed — proceed with movement.
    ReadyToMove,
}

/// Handle vehicle in-place rotation before movement begins.
///
/// Vehicles rotate toward `facing_target` before advancing. When `ROT > 0` the
/// hull turns through a native-frame `FacingClass` at the unit's rules ROT —
/// gamemd's `DriveLocomotionClass::Do_Turn` on the body PrimaryFacing, whose
/// turn duration is `abs(delta_8bit) / ROT` native frames (frame-count based,
/// NOT millisecond based). `ROT = 0` means instant snap. Infantry are excluded
/// by the caller (they always turn instantly without this function).
///
/// `facing` mirrors the interpolator's top byte. A retained hull from combat
/// can have no movement-facing target: preserve its full sample for fresh
/// admission. Completing a movement target still retires this adapter at the
/// exact byte target; persistent Facing lifecycle remains a separate migration.
///
/// Takes individual fields to avoid borrow conflicts with `entity.movement_target`.
pub(super) fn handle_vehicle_rotation(
    facing: &mut u8,
    facing_target: &mut Option<u8>,
    body_facing: &mut Option<super::facing_class::FacingClass>,
    _position: &mut Position,
    locomotor: &mut Option<LocomotorState>,
    rot: i32,
    native_frame: u32,
    sim_tick: u64,
) -> RotationResult {
    let Some(target_facing) = *facing_target else {
        // Unit Fire_At_Target/Face_Update can retain an arbitrary16-bit hull
        // without a movement-facing target. Drive4B3408 samples that same
        // PrimaryFacing: discarding it here incorrectly admits one-bit misses.
        if let Some(body) = body_facing.as_ref() {
            *facing = (body.current(native_frame) >> 8) as u8;
        }
        return RotationResult::ReadyToMove;
    };
    if rot <= 0 {
        // ROT=0 — instant turn, no gradual rotation.
        *facing = target_facing;
        *facing_target = None;
        *body_facing = None;
        return RotationResult::ReadyToMove;
    }

    // Rules ROT drives the hull FacingClass. `set` is a no-op once already aimed
    // at the target, so calling it each tick is safe and yields smooth retargets
    // (it snapshots the live animated value into the rotation origin).
    let rot_byte = rot.min(0x7F) as u8;
    let bf = body_facing.get_or_insert_with(|| {
        super::facing_class::FacingClass::new((*facing as u16) << 8, rot_byte)
    });
    bf.set((target_facing as u16) << 8, native_frame);
    *facing = (bf.current(native_frame) >> 8) as u8;

    if bf.is_rotating(native_frame) {
        // Still rotating in place — advance facing but don't move.
        let mut debug_events = Vec::new();
        if let Some(loco) = locomotor {
            let old_phase = loco.phase;
            loco.phase = GroundMovePhase::Accelerating;
            if old_phase != GroundMovePhase::Accelerating {
                debug_events.push((
                    sim_tick as u32,
                    DebugEventKind::PhaseChange {
                        from: format!("{:?}", old_phase),
                        to: "Accelerating".into(),
                        reason: "movement started".into(),
                    },
                ));
            }
        }
        RotationResult::StillRotating { debug_events }
    } else {
        // Rotation complete — snap to the exact target and start moving.
        *facing = target_facing;
        *facing_target = None;
        *body_facing = None;
        RotationResult::ReadyToMove
    }
}

/// Result of lepton position advancement.
pub(super) enum AdvanceResult {
    /// Drive track is active — caller should `continue` (skip cell crossings).
    DriveTrackActive,
    /// A fresh curve was refused by the cell occupation mask. No curve was
    /// installed, no head reservation was stamped, and the mover has not moved.
    /// The refusal carries its own answer — see [`DriveSelectionRefusal`] — so
    /// the caller's dispatch consumes it instead of re-deriving one.
    DriveTrackFreshBlocked(DriveSelectionRefusal),
    /// Normal advancement done — caller should proceed to cell crossings.
    ReadyForCrossings,
}

#[cfg(test)]
#[path = "movement_step_tests.rs"]
mod tests;

/// Install the pair of marks a Drive curve claims: the forward RawTrack handoff
/// cell it passes through and the head cell it comes to rest on.
/// `Apply_Track_Occupation_Mode` writes both on modes 1 and 3 (handoff first,
/// head second) and clears both on mode 0.
fn install_drive_head_to_occupation(
    foot_occupation_enabled: &mut bool,
    drive_locomotion: &mut Option<DriveLocomotionRuntime>,
    cell_occupation: &mut Option<&mut CellOccupationGrid>,
    entity_id: u64,
    current_cell: (u16, u16),
    current_layer: MovementLayer,
    next: Option<DriveOccupationFootprint>,
    handoff: Option<DriveOccupationFootprint>,
) {
    let Some(drive) = drive_locomotion.as_mut() else {
        return;
    };
    let Some(occupation) = cell_occupation.as_deref_mut() else {
        drive.occupation_head_to = next;
        drive.occupation_handoff = handoff;
        return;
    };
    match next {
        Some(next) => crate::sim::occupancy::replace_drive_head_to_occupation(
            foot_occupation_enabled,
            drive,
            occupation,
            entity_id,
            current_cell,
            current_layer,
            next,
        ),
        None => crate::sim::occupancy::clear_drive_head_to_occupation_for_replacement(
            foot_occupation_enabled,
            drive,
            occupation,
            entity_id,
            current_cell,
            current_layer,
        ),
    }
    crate::sim::occupancy::replace_drive_handoff_occupation(
        foot_occupation_enabled,
        drive,
        occupation,
        entity_id,
        current_cell,
        current_layer,
        handoff,
    );
}

/// The cell a freshly installed curve will pass through before reaching its
/// head, if the curve has a handoff point at all. Straight runs have none.
///
/// `Apply_Track_Occupation_Mode` applies one mode to the handoff coordinate and
/// then to the head coordinate, and the mark helper picks its plane from the
/// coordinate's own height and the cell's bridge flag. VERA has no per-cell
/// plane resolution here, so the pair is kept on ONE plane — the head mark's —
/// rather than pinning the handoff to Ground while the head follows the path.
/// A curve whose head resolves to the deck claims neither cell; the deck
/// equivalent of both marks is UNCHECKED.
fn drive_track_handoff_footprint(
    kind: LocomotorKind,
    current: DriveCoord,
    head: DriveCoord,
    track: crate::sim::components::TrackProgress,
    layer: MovementLayer,
) -> Option<DriveOccupationFootprint> {
    if layer != MovementLayer::Ground {
        return None;
    }
    let (handoff, _) = super::at_coord::AtCoordQuery::from_state(
        kind,
        current,
        Some(head),
        super::at_coord::AtCoordTrack {
            turn_index: track.turn_index,
            cursor: track.cursor,
            reversed: track.reversed,
        },
    )?
    .cells();
    let (hx, hy) = handoff?;
    Some(DriveOccupationFootprint {
        rx: u16::try_from(hx).ok()?,
        ry: u16::try_from(hy).ok()?,
        layer,
    })
}

/// Outcome of a fresh selection made while the mover stands on its current cell
/// — the position `Process_Movement` runs from.
enum FreshTrackOutcome {
    /// A curve was installed and the head cell reserved.
    Installed,
    /// The body is not on the head node's octant. gamemd commands the turn and
    /// returns without consuming a node or taking a step.
    TurnFirst(u8),
    /// The cell the curve would step into is already claimed by another
    /// vehicle. Nothing was installed and nothing was reserved.
    BlockedByOccupation(DriveSelectionRefusal),
    /// No usable descriptor; the native movement invocation remains idle.
    None,
}

/// A fresh Drive curve refused by the cell occupation mask, carrying its own
/// answer so the dispatch never has to re-derive one.
///
/// `UnitClass::Can_Enter_Cell` answers in two stages. It walks the cell's object
/// list FIRST and accumulates a code from the bodies it finds there; only if
/// that walk produced nothing (`TEST EBP,EBP; JNZ` at 0x0073FC24) does it fall
/// through to the cell occupation mask. So the mask is a last-resort arm, and
/// when the mask's vehicle bit is the thing that refuses, the answer is exactly
/// one value — `MOV EBP,0x2` at 0x0073FD32, reached from
/// `TEST [ESP+0x14],0x3f / MOV AL,[ESP+0x15] / TEST AL,AL / JNZ` at
/// 0x0073FC38-0x0073FC49. There is nothing left to classify: a mask refusal IS
/// code 2.
///
/// This gate therefore models the mask arm and only the mask arm. VERA's
/// `CellOccupationGrid` is that mask — it holds a bit for vehicles only
/// (`EntityCategory::Unit`), never infantry, exactly as the constant `0x20`
/// written by `UnitClass__MarkCellOccupationBit20 @ 0x007441B0` contrasts with
/// the variable `1 << GetSubCell` written by
/// `InfantryClass__MarkCellOccupancy @ 0x005217C0`. Object-list answers —
/// a parked friendly (6), an enemy (5), a crushable body — are produced
/// downstream by the crossing and chain lanes, which classify once and dispatch
/// on what they classified.
///
/// Recorded DRIFT: gamemd asks the whole predicate before it selects a curve, so
/// an object-list refusal reaches it one dispatch earlier than it reaches VERA,
/// which meets that blocker at the crossing on the following tick instead. That
/// is pre-existing behaviour, unchanged here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DriveSelectionRefusal {
    /// The cell that actually tripped the test — never a different one.
    pub cell: (u16, u16),
    /// The occupation plane the claim was found on.
    pub layer: MovementLayer,
    /// Which arm produced the refusal. Recorded for the trace.
    pub arm: DriveRefusalArm,
    /// The `Can_Enter_Cell` code the object-list walk produced, or `None` for a
    /// bare mask refusal — which is code 2 by construction (`MOV EBP,0x2` at
    /// 0x0073FD32 is the mask arm's only outcome).
    ///
    /// Carried because the codes do NOT share one dispatch. `0x004B36F4
    /// CMP EDX,0x6 / JNZ 0x004B3944` gives code 6 its own arm, and that arm
    /// reaches `CellClass__Scatter_Objects @ 0x00481670` (call at 0x004B393A)
    /// before falling into the shared entry at 0x004B3607. Codes 2, 4 and 5 all
    /// arrive at that shared entry — 4 and 5 via `0x004B3944 CMP EDX,0x1 /
    /// JNZ 0x004B3607` — and no `Scatter_Objects` call sits anywhere between
    /// the entry and its `Find_Path` tail.
    ///
    /// They do not all *stay* there, and an earlier revision of this comment
    /// implied they did (corrected 2026-09-16 from the binary). `0x004B364D
    /// CMP EAX,0x2 / JNZ 0x004B3A97` keeps only code 2 in the shared arm and
    /// sends 4 and 5 on to `0x004B3A97`, which splits them into the wall /
    /// blocking-object Override arm (`Find_Blocking_Object 0x0047C5A0`,
    /// `Is_Ally_ByObject 0x004F9A90`, `+0x1F4(1, object)`, and the wall-cell
    /// path opening at `0x004B3B94`; both arms converge on the single call
    /// instruction `0x004B3BE9 CALL [ESI+0x1F4]`). The `Scatter_Objects` statement above
    /// still holds; only the "codes 2 and 5 reach that entry directly" reading
    /// was wrong, and porting that arm is ledger row I9b's work.
    pub cost_code: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DriveRefusalArm {
    /// A body physically in the cell, from this owner's blocker snapshot.
    ObjectList,
    /// A claim in the vehicle occupation mask, with or without a body.
    OccupationMask,
}

/// The object-list arm of the predicate, in a form the mover can consult while
/// it still holds its own mutable borrow.
///
/// `units` is the per-owner blocker snapshot the tick already builds for
/// pathfinding. Its `cost_code` is the same 2/5/6 the native walk emits.
///
/// **Infantry are skipped.** They never hold the mask's vehicle bit, a crusher
/// is entitled to drive over them, and a gate that refused on their account
/// would stall every squish and every column with a friendly GI in it. gamemd
/// reaches the same place by a different route: an occupant whose RTTI
/// (`vtable+0x2C`) is `0x0F` takes the locomotor `+0xA4` question at 0x0073FA46
/// instead of jumping straight to the raise (RTTI-to-class binding UNCHECKED),
/// and the crush latch at 0x0073FCF6 resolves the rest.
///
/// That locomotor question is NOT reserved for `0x0F`. `0x0073FA30-0x0073FA38`
/// reads the occupant's `Foot+0x6B6` first, and a **zero** takes the same
/// 0x0073FA46 branch whatever the occupant's class is; only `+0x6B6` nonzero
/// AND class != `0x0F` jumps to the raise at 0x0073FA6D. The set state is not a
/// standing property of a vehicle:
/// `DriveLocomotionClass__Process_Drive_Track @ 0x004B0F20` writes 0 at
/// 0x004B161A and 1 at 0x004B1FEF, so an occupant in transit carries 0 and can
/// be skipped by the locomotor answer like any other. The shared runtime
/// classifier in `cell_entry` owns that live occupant predicate.
///
/// Building footprints are NOT consulted here either. Terrain and building
/// admission are answered by the crossing lane, which knows about
/// `bypass_grid` — a miner on its dock approach drives through cells this
/// snapshot marks as blocked, and refusing it here would stall the harvest loop.
/// Recorded DRIFT: gamemd's pre-selection `Can_Enter_Cell` sees buildings, VERA
/// meets them one dispatch later at the crossing. Pre-existing, unchanged.
#[derive(Clone, Copy, Default)]
pub(super) struct DriveCellAdmission<'a> {
    pub units: Option<&'a LayeredEntityBlockMap>,
    /// The mover as the head-on exit sees it; `None` for a non-Unit mover,
    /// whose `+0x1AC` has no head-on arm.
    pub mover: Option<MoverHeadOnContext>,
}

/// The mover's own inputs to the head-on exit of `UnitClass::Can_Enter_Cell`
/// (`0x0073F8D4..FA26`), captured before the mover takes its mutable borrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MoverHeadOnContext {
    pub facing: u8,
    pub world: [i32; 3],
}

impl MoverHeadOnContext {
    pub(super) fn from_entity(entity: &crate::sim::game_entity::GameEntity) -> Option<Self> {
        (entity.category == crate::map::entities::EntityCategory::Unit).then(|| Self {
            facing: entity.facing,
            world: crate::sim::pathfinding::cell_entry::entity_world_leptons(entity),
        })
    }
}

impl DriveCellAdmission<'_> {
    /// The code the object-list walk would raise for `cell`, or `None` when it
    /// finds nothing that refuses. The dispatch needs the code, not a boolean:
    /// code 6 has its own arm in the movement body and codes 2/5 do not.
    ///
    /// The head-on exit comes first, as in the native walk: a moving ally that
    /// faces the mover, within `0x1FF` leptons and inside the mover's facing
    /// octant, answers 7 whatever its class. It is answered from the owner
    /// snapshot's moving-ally record, which carries every moving ally
    /// including those the `+0x6B6`/`+0xA4` rule skipped from the code map.
    fn refusal_code(
        &self,
        cell: (u16, u16),
        layer: MovementLayer,
        self_cell: (u16, u16),
    ) -> Option<u8> {
        if cell == self_cell {
            // The native walk skips the mover itself
            // (`if (param_1 == piVar15)` at 0x0073FC10).
            return None;
        }
        let units = self.units?;
        if let (Some(mover), Some(ally)) = (self.mover, units.moving_ally(layer, &cell))
            && crate::sim::pathfinding::cell_entry::head_on_exit(
                mover.facing,
                mover.world,
                ally.facing,
                ally.world,
            )
        {
            return Some(7);
        }
        units
            .get(layer, &cell)
            .filter(|entry| !entry.blocker_is_infantry)
            .map(|entry| entry.cost_code)
    }
}

/// Run the fresh Drive/Ship curve selection for a mover standing on its own
/// cell: index the turn table by the two leading path directions, install the
/// curve at cursor 0, and reserve its head cell (two cells ahead for a turning
/// curve).
#[allow(clippy::too_many_arguments)]
fn select_fresh_drive_track_at_current_cell(
    foot_occupation_enabled: &mut bool,
    path_replay: &mut crate::sim::components::FootPathQueue,
    target: &mut MovementTarget,
    position: &Position,
    facing: u16,
    facing_target: &mut Option<u8>,
    drive_locomotion: &mut Option<DriveLocomotionRuntime>,
    ship_locomotion: &mut Option<ShipLocomotionRuntime>,
    cell_occupation: &mut Option<&mut CellOccupationGrid>,
    admission: DriveCellAdmission<'_>,
    entity_id: u64,
    current_occupation_layer: MovementLayer,
    shared_kind: LocomotorKind,
) -> FreshTrackOutcome {
    let Some(next) = target.path.get(target.next_index).copied() else {
        return FreshTrackOutcome::None;
    };
    let ndx = i32::from(next.0) - i32::from(position.rx);
    let ndy = i32::from(next.1) - i32::from(position.ry);
    let is_ship = shared_kind == LocomotorKind::Ship;
    let plan = match drive_track::plan_drive_track_from_path(
        facing,
        (ndx, ndy),
        path_window_to_delta(target),
        is_ship,
    ) {
        drive_track::DriveTrackDecision::TurnFirst { desired_facing } => {
            return FreshTrackOutcome::TurnFirst(desired_facing);
        }
        drive_track::DriveTrackDecision::Select(plan) => plan,
        drive_track::DriveTrackDecision::Unavailable => return FreshTrackOutcome::None,
    };

    // Cell exclusion — the occupation-mask arm of gamemd's cell-entry predicate.
    //
    // Asked about every cell this curve is about to CLAIM, because the mask is a
    // claim register, not a presence record: a curve stamps `0x20` into its
    // head-to cell while its body is still in the previous one
    // (`Apply_Track_Occupation_Mode` mark at 0x004B0C2E, reached from the tail
    // mark site at 0x004B4705). For a straight run that is the cell it steps
    // into; for a turning curve it is the cell two out, where the curve comes to
    // rest. Letting a second mover stamp a cell a first has already stamped
    // would make the register non-exclusive, which is the one property the whole
    // mechanism exists to provide.
    //
    // gamemd's own selection asks `Can_Enter_Cell` about one cell (0x004B34C0)
    // and stamps its head-to without a separate test, catching a doubly-claimed
    // endpoint one dispatch later instead. Recorded difference: VERA refuses at
    // stamp time rather than at the following crossing. Measured, on the
    // four-vehicle column fixture: without the endpoint test two members close
    // to 175 leptons inside one cell, below the separation retail's admission
    // rule can produce.
    //
    // The refusal also NULLS the locomotor head-to coordinate before dispatching
    // — 0x004B3607-0x004B3646 copies the null-coordinate triple at 0x008A0790
    // into the Drive head-to slot, on the shared entry the code-2 arm reaches.
    //
    // Nothing here depends on the plan being made first: `plan_drive_track_from_path`
    // only reads the turn table and writes nothing, so the order of the two is
    // unobservable. It runs first solely so the facing precondition — a body off
    // the head node's octant turns in place and never reaches the cell test — is
    // answered before the cell question.
    //
    // Ships keep their previous behaviour: they carry no Drive runtime and stamp
    // no occupation mark, so there is nothing here for them to contend over.
    // The ShipLocomotion equivalent is UNCHECKED. The forward RawTrack handoff
    // cell is marked but NOT tested here; whether it can be doubly claimed is
    // UNCHECKED.
    if drive_locomotion.is_some() {
        let plan_head_index = target.next_index + plan.nodes - 1;
        let endpoint_cell = (
            i32::from(position.rx) + plan.head_dx,
            i32::from(position.ry) + plan.head_dy,
        );
        let candidates = [
            (Some(next), target.layer_at(target.next_index)),
            (
                u16::try_from(endpoint_cell.0)
                    .ok()
                    .zip(u16::try_from(endpoint_cell.1).ok()),
                target.layer_at(plan_head_index),
            ),
        ];
        let self_cell = (position.rx, position.ry);
        for (cell, layer) in candidates {
            let Some(cell) = cell else {
                continue;
            };
            // Object list FIRST, mask LAST — the order the native predicate
            // uses. `TEST EBP,EBP; JNZ 0x0073FD37` at 0x0073FC24 skips the mask
            // arm entirely whenever the walk already produced a code.
            let (arm, cost_code) =
                if let Some(code) = admission.refusal_code(cell, layer, self_cell) {
                    (DriveRefusalArm::ObjectList, Some(code))
                } else if cell_occupation.as_deref().is_some_and(|occupation| {
                    occupation.occupied_by_other(cell.0, cell.1, layer, entity_id)
                }) {
                    (DriveRefusalArm::OccupationMask, None)
                } else {
                    continue;
                };
            // Refused: null the head-to coordinate (and release the cell it
            // still holds, so a refused step cannot leave one poisoned), keep
            // the mover's claim on the cell its body is standing in, then hand
            // the refusal — the cell that ACTUALLY tripped, never a different
            // one — to the caller's dispatch.
            if let Some(drive) = drive_locomotion.as_mut() {
                drive.head_to = None;
            }
            install_drive_head_to_occupation(
                foot_occupation_enabled,
                drive_locomotion,
                cell_occupation,
                entity_id,
                (position.rx, position.ry),
                current_occupation_layer,
                None,
                None,
            );
            if let (Some(drive), Some(occupation)) =
                (drive_locomotion.as_mut(), cell_occupation.as_deref_mut())
            {
                crate::sim::occupancy::restore_current_drive_occupation_after_refusal(
                    foot_occupation_enabled,
                    drive,
                    occupation,
                    entity_id,
                    (position.rx, position.ry),
                    current_occupation_layer,
                );
            }
            return FreshTrackOutcome::BlockedByOccupation(DriveSelectionRefusal {
                cell,
                layer,
                arm,
                cost_code,
            });
        }
    }

    let Some(head) = super::track_head::begin_fresh(&plan, position) else {
        return FreshTrackOutcome::None;
    };

    let (d_x, d_y, d_len) = crate::util::lepton::cell_delta_to_lepton_dir(ndx, ndy);
    target.move_dir_x = d_x;
    target.move_dir_y = d_y;
    target.move_dir_len = d_len;
    *facing_target = None;

    // The reserved head is the curve's endpoint: the head node for a straight
    // run, the node after it for a turning curve.
    let head_index = target.next_index + plan.nodes - 1;
    let endpoint = (
        (i32::from(position.rx) + plan.head_dx) as i16,
        (i32::from(position.ry) + plan.head_dy) as i16,
    );
    let endpoint_layer = target.layer_at(head_index);
    accept_shared_track(
        path_replay,
        shared_kind,
        drive_locomotion,
        ship_locomotion,
        endpoint,
        head,
        plan.nodes,
        plan.selection.turn_track_index,
    );
    let next_occupation = (endpoint_layer == MovementLayer::Ground)
        .then(|| {
            Some(DriveOccupationFootprint {
                rx: u16::try_from(endpoint.0).ok()?,
                ry: u16::try_from(endpoint.1).ok()?,
                layer: MovementLayer::Ground,
            })
        })
        .flatten();
    let progress = match shared_kind {
        LocomotorKind::Drive => drive_locomotion.as_ref().map(|state| state.track),
        LocomotorKind::Ship => ship_locomotion.as_ref().map(|state| state.track),
        _ => None,
    };
    let handoff_occupation = progress.and_then(|track| {
        drive_track_handoff_footprint(
            shared_kind,
            super::ground_pose::position_world_coord(position),
            head,
            track,
            endpoint_layer,
        )
    });
    install_drive_head_to_occupation(
        foot_occupation_enabled,
        drive_locomotion,
        cell_occupation,
        entity_id,
        (position.rx, position.ry),
        current_occupation_layer,
        next_occupation,
        handoff_occupation,
    );
    FreshTrackOutcome::Installed
}

pub(super) enum NativeTrackPreparation {
    Invoke(super::track_process::TrackInvocation),
    TurnFirst(super::track_process::TrackInvocation),
    Idle(super::track_process::TrackInvocation),
    Blocked(super::track_process::TrackInvocation, DriveSelectionRefusal),
}

/// Ordinary world execution hands off below the speed prefix. Admission and
/// execution use the locomotor TrackProgress and retained raw head.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_native_track(
    foot_occupation_enabled: &mut bool,
    replay: &mut crate::sim::components::FootPathQueue,
    target: &mut MovementTarget,
    position: &Position,
    facing: u16,
    facing_target: &mut Option<u8>,
    drive: &mut Option<DriveLocomotionRuntime>,
    ship: &mut Option<ShipLocomotionRuntime>,
    locomotor: &Option<LocomotorState>,
    category: EntityCategory,
    entity_id: u64,
    occupation: &mut CellOccupationGrid,
    admission: DriveCellAdmission<'_>,
    layer: MovementLayer,
) -> Option<NativeTrackPreparation> {
    use super::track_process::{TrackFamily, TrackInvocation};
    let kind = shared_track_kind(locomotor)?;
    if category == EntityCategory::Infantry {
        return None;
    }
    // Ordinary Process4B0A75..4B0AAA /6A013E..6A0173 calls TrackProcess
    // after fresh selection even when AL is false. Its entry, not fresh
    // selection, clears residual when no valid descriptor/queue8 remains.
    let mut invocation = TrackInvocation {
        entity_id,
        family: if kind == LocomotorKind::Ship {
            TrackFamily::Ship
        } else {
            TrackFamily::Drive
        },
        apply_fresh_occupation: false,
    };
    let active = match kind {
        LocomotorKind::Drive => drive
            .as_ref()
            .is_some_and(|state| state.track_valid && state.track.turn_index != -1),
        LocomotorKind::Ship => ship
            .as_ref()
            .is_some_and(|state| state.track_valid && state.track.turn_index != -1),
        _ => false,
    };
    if !active {
        match select_fresh_drive_track_at_current_cell(
            foot_occupation_enabled,
            replay,
            target,
            position,
            facing,
            facing_target,
            drive,
            ship,
            &mut Some(occupation),
            admission,
            entity_id,
            layer,
            kind,
        ) {
            FreshTrackOutcome::Installed => {}
            FreshTrackOutcome::TurnFirst(desired) => {
                *facing_target = Some(desired);
                return Some(NativeTrackPreparation::TurnFirst(invocation));
            }
            FreshTrackOutcome::BlockedByOccupation(refusal) => {
                return Some(NativeTrackPreparation::Blocked(invocation, refusal));
            }
            FreshTrackOutcome::None => return Some(NativeTrackPreparation::Idle(invocation)),
        }
    }
    invocation.apply_fresh_occupation = !active;
    Some(NativeTrackPreparation::Invoke(invocation))
}

/// Walk75BD70 completes a retained subcell head within 17 world leptons.
pub(super) fn completed_walk_head(
    position: &Position,
    locomotor: &Option<LocomotorState>,
) -> Option<crate::sim::components::DriveCoord> {
    let loco = locomotor
        .as_ref()
        .filter(|l| l.kind == LocomotorKind::Walk)?;
    let head = loco.step_head()?;
    let [x, y] = super::ground_pose::position_world_xy(position);
    (fixed_distance(
        SimFixed::from_num(head.x.wrapping_sub(x)),
        SimFixed::from_num(head.y.wrapping_sub(y)),
    ) < SimFixed::from_num(17))
    .then_some(head)
}

pub(super) fn advance_lepton_position(
    target: &mut MovementTarget,
    position: &mut Position,
    locomotor: &mut Option<LocomotorState>,
    category: EntityCategory,
    effective_speed: SimFixed,
    dt: SimFixed,
    entity_id: u64,
) -> AdvanceResult {
    // Track and Walk coordinate execution have their own production owners.
    if shared_track_kind(locomotor).is_some() {
        return AdvanceResult::DriveTrackActive;
    }
    assert!(
        !locomotor
            .as_ref()
            .is_some_and(|l| l.kind == LocomotorKind::Walk),
        "Walk must execute its admitted paid step"
    );
    if target.move_dir_len > SIM_ZERO {
        let frac = effective_speed * dt / target.move_dir_len;
        let final_subcell = locomotor
            .as_ref()
            .and_then(|l| l.subcell_dest)
            .filter(|_| target.next_index >= target.path.len());
        if frac >= SIM_ONE
            && let Some((x, y)) = final_subcell
        {
            position.sub_x = x;
            position.sub_y = y;
        } else {
            position.sub_x += target.move_dir_x * frac;
            position.sub_y += target.move_dir_y * frac;
        }
    }
    advance_infantry_wobble(locomotor, category, entity_id, dt);
    AdvanceResult::ReadyForCrossings
}

/// Preserve the existing presentation phase without coupling it to a numeric
/// movement adapter. Walk and non-Walk infantry publish it once per paid step.
pub(super) fn advance_infantry_wobble(
    locomotor: &mut Option<LocomotorState>,
    category: EntityCategory,
    entity_id: u64,
    dt: SimFixed,
) {
    if category == EntityCategory::Infantry
        && let Some(loco) = locomotor
    {
        if loco.infantry_wobble_phase == 0.0 {
            loco.infantry_wobble_phase = (entity_id.wrapping_mul(2654435761) & 0xFFFF) as f32
                / 0xFFFF as f32
                * std::f32::consts::TAU;
        }
        loco.infantry_wobble_phase += super::INFANTRY_WOBBLE_RATE * dt.to_num::<f32>();
    }
}

/// Output from the cell boundary crossing loop.
pub(super) struct CrossingOutput {
    /// A production Walk crossing must release the entity borrow before
    /// Mark(REMOVE), SetCoords, SetHeight and Mark(PUT). No PerCell here.
    pub walk_boundary: Option<crate::sim::components::DriveCoord>,
    pub walk_head_admitted: bool,
    /// If set, the caller must handle deferred occupancy outside the entity borrow.
    pub deferred_cell_check: Option<DeferredCellCheck>,
    /// If set, the mover was refused a second time at this cell by a wall it can
    /// shoot, and the caller must run the wall-attack Override there.
    ///
    /// Deferred rather than fired inline: this function holds decomposed `&mut`
    /// fields and has neither `&mut EntityStore` nor `&mut GameEntity`, and the
    /// Override needs to write `mission`, `attack_target` and `navigation`.
    pub deferred_wall_override: Option<(u16, u16)>,
    /// Bridge render state to apply after the loop. Predicate-driven; see movement_bridge.rs.
    pub pending_bridge_update: super::movement_bridge::BridgeStateUpdate,
    /// The resolved movement layer after all crossings.
    pub active_layer: MovementLayer,
    /// Debug events accumulated during crossing checks.
    pub debug_events: Vec<(u32, DebugEventKind)>,
    /// Whether the entity was marked as stuck and should abort.
    pub aborted_for_stuck: bool,
    pub runtime_bridge_transition: super::movement_bridge::RuntimeBridgeTransitionState,
}

/// Process cell boundary crossings — the inner loop that checks whether
/// sub_x/sub_y have crossed cell boundaries, validates terrain walkability,
/// cliff height, and occupancy, then performs cell transitions with lepton
/// remainder carry-over.
///
/// Takes individual entity fields to avoid borrow conflicts with
/// `entity.movement_target` (which the caller holds as `ref mut target`).
#[allow(clippy::too_many_arguments)]
pub(super) fn process_cell_crossings(
    foot_occupation_enabled: &mut bool,
    path_replay: &mut crate::sim::components::FootPathQueue,
    target: &mut MovementTarget,
    path_runtime: &mut crate::sim::components::FootPathRuntime,
    position: &mut Position,
    facing: &mut u8,
    facing_target: &mut Option<u8>,
    body_facing: Option<super::FacingClass>,
    locomotor: &mut Option<LocomotorState>,
    drive_locomotion: &mut Option<DriveLocomotionRuntime>,
    ship_locomotion: &mut Option<ShipLocomotionRuntime>,
    sub_cell: &mut Option<u8>,
    category: EntityCategory,
    entity_id: u64,
    mut active_layer: MovementLayer,
    snap: &MoverSnapshot,
    path_grid: Option<&PathGrid>,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    entity_cost_grid: Option<&TerrainCostGrid>,
    mover_entity_blocks: Option<&BTreeSet<(u16, u16)>>,
    mover_entity_block_map: Option<&crate::sim::pathfinding::LayeredEntityBlockMap>,
    live_building_entry_skips: &impl BuildingEntrySkipLookup,
    occupancy: &mut OccupancyGrid,
    cell_occupation: &mut CellOccupationGrid,
    occupancy_enter_order: &mut u64,
    next_occupancy_enter_order: &mut EnterOrderCounter,
    stats: &mut MovementTickStats,
    finished_entities: &mut Vec<u64>,
    rng: &mut SimRng,
    // Borrowed for this call only. The wall arm needs it to resolve house names
    // for the ally test, and a per-call borrow is what keeps it off
    // `PathfindingContext`, which outlives the pass and would collide with the
    // `&mut StringInterner` the pass still needs.
    interner: &crate::sim::intern::StringInterner,
    ctx: PathfindingContext<'_>,
    mcfg: MovementConfig,
    sim_tick: u64,
    marker_context: Option<super::path_markers::BridgeMarkerContext<'_>>,
    suspend_walk_boundary: bool,
    walk_head_admission: bool,
) -> CrossingOutput {
    let walk = locomotor
        .as_ref()
        .is_some_and(|l| l.kind == LocomotorKind::Walk);
    let mut walk_head_admitted = false;
    let mut walk_boundary = None;
    let mut debug_events: Vec<(u32, DebugEventKind)> = Vec::new();
    let mut deferred_cell_check: Option<DeferredCellCheck> = None;
    let mut deferred_wall_override: Option<(u16, u16)> = None;
    let mut runtime_bridge_transition = snap.runtime_bridge_transition;
    let mut pending_bridge_update: super::movement_bridge::BridgeStateUpdate =
        super::movement_bridge::BridgeStateUpdate::Unchanged;
    let mut projected_on_bridge_state = snap.on_bridge;
    let mut aborted_for_stuck: bool = false;

    loop {
        if target.next_index >= target.path.len() {
            break;
        }
        let old_rx = position.rx;
        let old_ry = position.ry;
        let committed_walk = locomotor
            .as_ref()
            .is_some_and(|l| l.kind == LocomotorKind::Walk && l.step_head().is_some());
        let (nx, ny): (u16, u16) = if committed_walk {
            let [x, y] = super::ground_pose::position_world_xy(position);
            ((x / 256) as u16, (y / 256) as u16)
        } else {
            target.path[target.next_index]
        };
        let dx_cell: i32 = nx as i32 - position.rx as i32;
        let dy_cell: i32 = ny as i32 - position.ry as i32;
        if dx_cell == 0
            && dy_cell == 0
            && locomotor
                .as_ref()
                .is_some_and(|l| l.kind == LocomotorKind::Walk)
        {
            break;
        }

        // Check if sub_x/sub_y have crossed cell boundaries on each axis.
        let crossed_x: bool = match dx_cell.signum() {
            1 => position.sub_x >= crate::util::lepton::LEPTONS_PER_CELL,
            -1 => position.sub_x <= SIM_ZERO,
            _ => true, // No X movement needed for this step.
        };
        let crossed_y: bool = match dy_cell.signum() {
            1 => position.sub_y >= crate::util::lepton::LEPTONS_PER_CELL,
            -1 => position.sub_y <= SIM_ZERO,
            _ => true,
        };
        let crossing = if walk_head_admission {
            true
        } else if committed_walk {
            //75C0F3..75C117 compares both actual cell coordinates. A
            //diagonal step can enter a side cell before its other axis crosses.
            let [x, y] = super::ground_pose::position_world_xy(position);
            ((x / 256) as u16, (y / 256) as u16) != (old_rx, old_ry)
        } else {
            crossed_x && crossed_y
        };
        if !crossing {
            break;
        }

        // The wall memo names the cell where a wall refused this mover on its
        // previous attempt. The moment it attempts a different cell - because a
        // repath found a detour - that refusal is stale, and a memo that outlived
        // its cell would let the Override fire on a FIRST refusal later, skipping
        // the repath native requires before its second evaluation.
        if target
            .wall_refusal_cell
            .is_some_and(|cell| cell != (nx, ny))
        {
            target.wall_refusal_cell = None;
        }

        let next_layer = target.layer_at(target.next_index);
        //75AECD..75AEF8 jumps past pathfind/admission when a head exists.
        //Only the no-head branch reaches CanEnter75B690 before75BC1A.
        if !committed_walk {
            let runtime_entry = evaluate_runtime_can_enter_cell_with_transition(
                path_grid,
                next_layer,
                &mut runtime_bridge_transition,
                projected_on_bridge_state,
                runtime_can_enter_cell_args(
                    path_grid,
                    (position.rx, position.ry),
                    (nx, ny),
                    projected_on_bridge_state,
                    position.z,
                ),
            );
            let layer_context = runtime_entry.layers;
            let mut layer_grid_ok: Option<bool> = None;
            let mut layer_terrain_ok: Option<bool> = None;

            if !runtime_entry.bridge_traversal_allowed {
                position.sub_x = crate::util::lepton::CELL_CENTER_LEPTON;
                position.sub_y = crate::util::lepton::CELL_CENTER_LEPTON;
                // VERA centre recovery is not a native locomotor step; keep its
                // committed old-cell pose coherent without sampling the rejected XY.
                if locomotor.as_ref().is_some_and(|loco| {
                    matches!(
                        loco.kind,
                        LocomotorKind::Drive | LocomotorKind::Ship | LocomotorKind::Walk
                    )
                }) {
                    super::ground_pose::commit_ground_height(
                        position,
                        projected_on_bridge_state,
                        resolved_terrain,
                        path_grid,
                    );
                }
                path_runtime.start_movement(mcfg.binary_frame, 0);
                let evts = handle_blocked_tick(
                    path_replay,
                    target,
                    path_runtime,
                    facing,
                    body_facing,
                    &snap.locomotor,
                    drive_locomotion,
                    ship_locomotion,
                    entity_id,
                    (position.rx, position.ry),
                    active_layer,
                    snap.on_bridge,
                    stats,
                    finished_entities,
                    &mut aborted_for_stuck,
                    ctx,
                    entity_cost_grid,
                    mover_entity_blocks,
                    mover_entity_block_map,
                    snap.too_big_to_fit_under_bridge,
                    mcfg,
                    rng,
                    sim_tick,
                    PATH_STUCK_INIT,
                    super::MoverPathFacts::from_snapshot(snap, 0),
                    snap.allow_zone_hierarchy,
                    true,
                    true,
                    marker_context,
                    occupancy,
                );
                debug_events.extend(evts);
                break;
            }

            // --- Terrain walkability check (static map data) ---
            // Set when the Ground arm refuses because of a wall this mover could
            // shoot (`Can_Enter_Cell` 4 or 5 rather than 7). Recorded here and
            // consumed by the refusal block; ledger row I9b.
            let mut ground_wall_class: Option<u8> = None;
            let layer_walkable = match layer_context.terrain_layer {
                MovementLayer::Ground => {
                    // Water movers (ships) bypass PathGrid — water cells are
                    // marked non-walkable for land units but ships need them.
                    // Use passability matrix directly, same as the pathfinder.
                    let cost_grid = if target.ignore_terrain_cost {
                        None
                    } else {
                        entity_cost_grid
                    };
                    // Same predicate the search ran. The original reaches its cell
                    // gate through a single per-class slot, so an infantryman's
                    // sub-cell view of terrain objects has to hold here too —
                    // otherwise A* plans through a tree cell the step-in refuses
                    // and the mover block/repath-loops onto the identical route.
                    // Result-preserving: the bool form flattens the wall
                    // arm's 4 and 5 into the same refusal as a hard 7, and the
                    // Override needs that distinction. With no wall tables the
                    // context is `None` and this is the previous predicate
                    // exactly - see `evaluate_cell_entry_for_category_on_layer`.
                    let entry = match path_grid {
                        Some(grid) => {
                            crate::sim::pathfinding::evaluate_cell_entry_for_category_on_layer(
                                grid,
                                nx,
                                ny,
                                MovementLayer::Ground,
                                Some(snap.movement_zone),
                                snap.speed_type,
                                resolved_terrain,
                                cost_grid,
                                target.bypass_grid,
                                crate::sim::pathfinding::cell_entry::TerrainEntryMode::RuntimeTransition,
                                category == EntityCategory::Infantry,
                                snap.crush_capability().wall_arm_crusher(),
                                ctx.wall_tables.map(|tables| {
                                    crate::sim::pathfinding::cell_entry::WallArmContext {
                                        overlay_grid: tables.overlay_grid,
                                        overlay_registry: tables.overlay_registry,
                                        alliances: tables.alliances,
                                        interner: Some(interner),
                                        mover_owner: Some(snap.owner),
                                        is_armed: snap.is_armed,
                                        warhead_wall: snap.warhead_wall,
                                        warhead_wood: snap.warhead_wood,
                                    }
                                }),
                            )
                        }
                        None => crate::sim::pathfinding::cell_entry::CanEnterCellResult::Clear,
                    };
                    if let crate::sim::pathfinding::cell_entry::CanEnterCellResult::WallBlocked {
                        cost_class,
                    } = entry
                    {
                        ground_wall_class = Some(cost_class);
                    }
                    let grid_ok: bool = entry.is_clear();
                    let terrain_ok: bool = true;
                    layer_grid_ok = Some(grid_ok);
                    layer_terrain_ok = Some(terrain_ok);
                    grid_ok && terrain_ok
                }
                MovementLayer::Bridge => path_grid.is_some_and(|grid| {
                    crate::sim::pathfinding::is_cell_passable_for_mover_on_layer_with_speed(
                        grid,
                        nx,
                        ny,
                        MovementLayer::Bridge,
                        Some(snap.movement_zone),
                        snap.speed_type,
                        resolved_terrain,
                        entity_cost_grid,
                        target.bypass_grid,
                        crate::sim::pathfinding::cell_entry::TerrainEntryMode::RuntimeTransition,
                    )
                }),
                MovementLayer::Air | MovementLayer::Underground => false,
            };
            if !layer_walkable {
                if snap.movement_zone.is_water_mover() {
                    log::info!(
                        "NAVAL transition blocked: entity={} cur=({},{}) next=({},{}) layer={:?} grid_ok={:?} terrain_ok={:?} blocked_delay={} path_blocked={} {}",
                        entity_id,
                        position.rx,
                        position.ry,
                        nx,
                        ny,
                        next_layer,
                        layer_grid_ok,
                        layer_terrain_ok,
                        path_runtime
                            .blocked_timer
                            .remaining(mcfg.binary_frame as i32),
                        path_runtime.path_blocked,
                        naval_terrain_diag(resolved_terrain, (nx, ny)),
                    );
                }
                // Undo lepton advancement — entity stays at cell center.
                position.sub_x = crate::util::lepton::CELL_CENTER_LEPTON;
                position.sub_y = crate::util::lepton::CELL_CENTER_LEPTON;
                // VERA centre recovery is not a native locomotor step; keep its
                // committed old-cell pose coherent without sampling the rejected XY.
                if locomotor.as_ref().is_some_and(|loco| {
                    matches!(
                        loco.kind,
                        LocomotorKind::Drive | LocomotorKind::Ship | LocomotorKind::Walk
                    )
                }) {
                    super::ground_pose::commit_ground_height(
                        position,
                        projected_on_bridge_state,
                        resolved_terrain,
                        path_grid,
                    );
                }

                // Wall arm (ledger I9b). `Can_Enter_Cell` answered 4 or 5: a wall
                // this mover is armed against and whose warhead admits it. Native
                // does not Override on the first refusal - it drops the path and
                // retries within the same call (`0x004B3ADB` -> `0x004B4552`,
                // `arg2 = 0`), and the Override at `0x004B3BE9` fires only when
                // the repathed first step is refused 4/5 again. So the first
                // refusal repaths exactly as any other block does, and only a
                // repeat at the same cell attacks.
                match ground_wall_class {
                    Some(_) if target.wall_refusal_cell == Some((nx, ny)) => {
                        // Second refusal at the same cell: attack it. No
                        // `handle_blocked_tick` - that would scatter, re-arm the
                        // blockage timer and start another repath, which is the
                        // loop this arm exists to break.
                        deferred_wall_override = Some((nx, ny));
                        target.wall_refusal_cell = None;
                        break;
                    }
                    Some(_) => {
                        target.wall_refusal_cell = Some((nx, ny));
                    }
                    None => {
                        // An ordinary block clears the memo, so two unrelated
                        // refusals can never be read as a repeat.
                        target.wall_refusal_cell = None;
                    }
                }

                // Terrain-blocked (building/cliff) — the path is stale.
                // Force immediate repath by clearing movement_delay.
                path_runtime.start_movement(mcfg.binary_frame, 0);
                let evts = handle_blocked_tick(
                    path_replay,
                    target,
                    path_runtime,
                    facing,
                    body_facing,
                    &snap.locomotor,
                    drive_locomotion,
                    ship_locomotion,
                    entity_id,
                    (position.rx, position.ry),
                    active_layer,
                    snap.on_bridge,
                    stats,
                    finished_entities,
                    &mut aborted_for_stuck,
                    ctx,
                    entity_cost_grid,
                    mover_entity_blocks,
                    mover_entity_block_map,
                    snap.too_big_to_fit_under_bridge,
                    mcfg,
                    rng,
                    sim_tick,
                    PATH_STUCK_INIT,
                    super::MoverPathFacts::from_snapshot(snap, 0),
                    snap.allow_zone_hierarchy,
                    true, // terrain block: skip code-2 grace period
                    true,
                    marker_context,
                    occupancy,
                );
                debug_events.extend(evts);
                break;
            }

            // --- Cliff detection ---
            // Original engine: if height difference >= 3 levels and not a
            // bridge ramp, treat as cliff. Catches stale paths after terrain
            // changes, bump/scatter toward cliff edges, etc.
            if let Some(pg) = path_grid {
                if let Some(next_cell) = pg.cell(nx, ny) {
                    let next_level = next_cell.effective_cell_z_for_layer(next_layer);
                    let diff = (position.z as i16 - next_level as i16).unsigned_abs();
                    // `is_elevated_bridge_cell` keys on the walkable permission bit, so a
                    // structural deck cell whose permission is clear — a damaged span, or
                    // one carrying a terrain object — reads as ordinary terrain here. A
                    // mover on the deck now carries `ground + 4` (the native height model,
                    // `FootClass::Set_Height_On_Bridge` 0x005F5FA0), so against that cell's
                    // terrain level the difference is exactly the deck delta and the mover
                    // is stopped mid-span as if it had walked off a cliff. Reading the
                    // structural flag as well keeps the deck a deck regardless of the
                    // permission bit.
                    //
                    // VERA-internal, gamemd equivalent UNCHECKED: `CLIFF_HEIGHT_THRESHOLD`
                    // itself has no identified native owner (`sim::movement::mod.rs`), so
                    // this widens a VERA-only gate rather than porting a native one.
                    let is_bridge_ramp = next_cell.is_bridge_transition_cell()
                        || next_cell.is_elevated_bridge_cell()
                        || next_cell.has_structural_bridge();
                    if diff >= CLIFF_HEIGHT_THRESHOLD && !is_bridge_ramp {
                        position.sub_x = crate::util::lepton::CELL_CENTER_LEPTON;
                        position.sub_y = crate::util::lepton::CELL_CENTER_LEPTON;
                        // VERA centre recovery is not a native locomotor step; keep its
                        // committed old-cell pose coherent without sampling the rejected XY.
                        if locomotor.as_ref().is_some_and(|loco| {
                            matches!(
                                loco.kind,
                                LocomotorKind::Drive | LocomotorKind::Ship | LocomotorKind::Walk
                            )
                        }) {
                            super::ground_pose::commit_ground_height(
                                position,
                                projected_on_bridge_state,
                                resolved_terrain,
                                path_grid,
                            );
                        }
                        path_runtime.start_movement(mcfg.binary_frame, 0);
                        let evts = handle_blocked_tick(
                            path_replay,
                            target,
                            path_runtime,
                            facing,
                            body_facing,
                            &snap.locomotor,
                            drive_locomotion,
                            ship_locomotion,
                            entity_id,
                            (position.rx, position.ry),
                            active_layer,
                            snap.on_bridge,
                            stats,
                            finished_entities,
                            &mut aborted_for_stuck,
                            ctx,
                            entity_cost_grid,
                            mover_entity_blocks,
                            mover_entity_block_map,
                            snap.too_big_to_fit_under_bridge,
                            mcfg,
                            rng,
                            sim_tick,
                            PATH_STUCK_INIT,
                            super::MoverPathFacts::from_snapshot(snap, 0),
                            snap.allow_zone_hierarchy,
                            true, // cliff block: skip code-2 grace period
                            true,
                            marker_context,
                            occupancy,
                        );
                        debug_events.extend(evts);
                        break;
                    }
                }
            }

            // --- Occupancy check (entity-aware: sub-cell, crush, bump) ---
            // Occupancy check: vehicles defer to crush/bump/attack handler,
            // infantry defer to sub-cell/attack handler. Both break out of the
            // loop to release the mutable entity borrow for blocker lookups.
            let current_object_list_layer = if projected_on_bridge_state {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            };
            if let Some(check) = detect_deferred_cell_check(
                snap.category,
                entity_id,
                target.bypass_grid,
                layer_context,
                (nx, ny),
                (position.rx, position.ry),
                current_object_list_layer,
                occupancy,
                cell_occupation,
                live_building_entry_skips,
            ) {
                deferred_cell_check = Some(check);
                break;
            }
        }
        if walk_head_admission {
            walk_head_admitted = true;
            break;
        }

        if suspend_walk_boundary
            && locomotor
                .as_ref()
                .is_some_and(|l| l.kind == LocomotorKind::Walk)
        {
            //75C117: the provisional polar coordinate is selected, but old
            //XYZ must remain visible to RemoveContent and its Recalc callback.
            walk_boundary = Some(super::ground_pose::position_world_coord(position));
            break;
        }

        // --- Cell transition: carry over lepton remainder ---
        // Only adjust the axes that actually crossed a boundary.
        // Do NOT snap the perpendicular axis to center — that causes
        // a visible position jump when transitioning from diagonal
        // to cardinal movement (e.g., sub_x=51 → 128 = ~9px snap).
        apply_cell_transition_remainder(
            path_runtime,
            position,
            dx_cell,
            dy_cell,
            nx,
            ny,
            category == EntityCategory::Infantry,
            mcfg.binary_frame,
            walk,
        );
        // GATE A2 verified order: the object-list layer is selected by the
        // occupant's OnBridge byte sampled at each call site. Capture the OLD
        // (pre-transition) layer BEFORE evaluating the bridge transition, so the
        // old-cell removal walks the old layer and the new-cell insertion the new
        // layer (the two halves may differ when stepping on/off the deck).
        let old_occupancy_layer = if projected_on_bridge_state {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        let bridge_update = resolve_cell_transition_bridge_state(
            position,
            path_grid,
            (old_rx, old_ry),
            (nx, ny),
            projected_on_bridge_state,
        );
        projected_on_bridge_state =
            super::movement_bridge::projected_on_bridge(projected_on_bridge_state, bridge_update);
        if locomotor
            .as_ref()
            .is_some_and(|loco| loco.kind == LocomotorKind::Walk)
        {
            super::ground_pose::commit_ground_height(
                position,
                projected_on_bridge_state,
                resolved_terrain,
                path_grid,
            );
        }
        if !matches!(
            bridge_update,
            super::movement_bridge::BridgeStateUpdate::Unchanged
        ) {
            pending_bridge_update = bridge_update;
        }
        let new_occupancy_layer = if projected_on_bridge_state {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        if committed_walk {
            //Headless adapter: current-coordinate list projection only.
            //Production uses the world runner above, including raw/Recalc.
            *sub_cell = Some(crate::sim::cell_kernel::infantry_preferred_spot(
                crate::sim::cell_kernel::CellQueryPoint {
                    x: position.sub_x.to_num::<i32>(),
                    y: position.sub_y.to_num::<i32>(),
                },
            ));
            *occupancy_enter_order = next_occupancy_enter_order.next();
            occupancy.move_entity_layered(
                old_rx,
                old_ry,
                nx,
                ny,
                entity_id,
                old_occupancy_layer,
                new_occupancy_layer,
                *sub_cell,
                crate::sim::occupancy::CellListInsertion::from_category(category),
            );
            stats.moved_steps = stats.moved_steps.saturating_add(1);
        } else {
            CellArrival {
                entity_id,
                category,
                from: (old_rx, old_ry),
                to: (nx, ny),
                old_list_layer: old_occupancy_layer,
                new_list_layer: new_occupancy_layer,
                position,
                locomotor,
                drive_locomotion,
                foot_occupation_enabled,
                sub_cell,
                occupancy_enter_order,
                next_occupancy_enter_order,
                occupancy,
                cell_occupation,
                stats,
                priority: snap.sub_cell_priority_mission && snap.nav_com_cell == Some((nx, ny)),
            }
            .ordinary(next_layer);
        }
        active_layer = next_layer;

        if locomotor
            .as_ref()
            .is_some_and(|l| l.kind == LocomotorKind::Walk)
        {
            // Walk75C12E relinks a boundary crossing but retains Head_To and
            // the current path entry until its <17 completion corridor.
            break;
        }
        if let Some(loco) = locomotor.as_mut() {
            loco.set_step_head(None);
        }

        configure_motion_after_transition(
            target,
            locomotor,
            facing,
            facing_target,
            category,
            snap.rot,
            position,
        );

        // Pre-allocate subcell in the NEXT path cell for infantry direction targeting.
        // FindSubCellDest reserves a subcell in the destination cell before walking,
        // so each infantry targets its own subcell position based on the destination
        // cell's occupancy rather than carrying the current cell's.
        if category == EntityCategory::Infantry && target.next_index < target.path.len() {
            let next_cell = target.path[target.next_index];
            // Missions Enter / Capture / Eaten / Area Guard / Patrol whose
            // NavCom sits in the cell being reserved place unconditionally,
            // skipping the occupancy, blocker and garrison gates and taking no
            // random draw — matching the original engine's priority branch.
            let pre_priority =
                snap.sub_cell_priority_mission && snap.nav_com_cell == Some(next_cell);
            let pre_slot = if pre_priority {
                Some(bump_crush::priority_sub_cell(
                    position.sub_x,
                    position.sub_y,
                ))
            } else {
                bump_crush::allocate_sub_cell_with_preference(
                    occupancy.get(next_cell.0, next_cell.1),
                    active_layer,
                    None,
                    position.sub_x,
                    position.sub_y,
                    rng,
                )
            };
            if let Some(pre_sub) = pre_slot {
                let (sc_x, sc_y) = crate::util::lepton::subcell_lepton_offset(Some(pre_sub));
                if let Some(loco) = locomotor {
                    loco.subcell_dest = Some((sc_x, sc_y));
                }
                // Recompute direction toward the destination cell's subcell.
                let ndx = next_cell.0 as i32 - nx as i32;
                let ndy = next_cell.1 as i32 - ny as i32;
                let dest_x = SimFixed::from_num(ndx * 256) + sc_x;
                let dest_y = SimFixed::from_num(ndy * 256) + sc_y;
                let dx = dest_x - position.sub_x;
                let dy = dest_y - position.sub_y;
                target.move_dir_x = dx;
                target.move_dir_y = dy;
                target.move_dir_len = fixed_distance(dx, dy);
            }
        }
    }

    CrossingOutput {
        deferred_wall_override,
        walk_boundary,
        walk_head_admitted,
        deferred_cell_check,
        pending_bridge_update,
        active_layer,
        debug_events,
        aborted_for_stuck,
        runtime_bridge_transition,
    }
}
