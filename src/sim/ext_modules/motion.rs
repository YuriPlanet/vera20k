//! Module movement (RFC 0001 appendix 4.5 `MoveFrame`, 5.3 `Push` and
//! `SetFacing`): the per-frame step a module may give a ground mover, the
//! forced motion a `Push` keeps, and the one step both take.
//!
//! Which objects: Units and Infantry whose active locomotor is Drive, Ship,
//! Walk, Mech or Hover, alive, out of limbo, not inside a transport and not
//! a train car or a train with cars ([`module_mover`]). Fly, Jumpjet,
//! Rocket, Tunnel and Teleport (and a Chrono warp, which puts a Teleport in
//! front) are not covered: their Process integrates its own velocity,
//! height or phase, which a displacement from outside would contradict.
//!
//! Where: FootClass::AI's one call of the active locomotor's Process
//! (`Simulation::process_active_locomotor`, `0x004DA877`), after its
//! admission (not sinking, falling or in limbo) and Ares' DropPod, before
//! the locomotor's own Process, and only while the locomotor has no step in
//! flight: no committed Drive/Ship HeadToCoord (`+0x40`; a track, turn
//! included, runs to its end first) and no Walk, Mech or Hover head
//! (`+0x24`). That is the point where a native Process starts its next step
//! (or stands), so a module step never cuts a track or a sub-cell step in
//! two: the native one leaves the Foot at a track or cell point.
//!
//! In that order:
//! 1. An active `Push` takes the frame: its next share of the displacement.
//!    `MoveFrame` is not asked that frame.
//! 2. `MoveFrame` (decision-only, object or type binding): `subject` the
//!    mover, `x`, `y`, `z` its position, `value` 1 while its locomotor is
//!    moving (`Is_Moving`), else 0, `other` and `cell` its NavCom (object or
//!    cell, 0 for none). The result area holds one `MoveStep`, all zero for
//!    the first link; `result` is unused. `handled` 0 leaves the frame to
//!    the native Process. A `MoveStep` with bit 2 (stop) gives the mover a
//!    NULL destination first (the class setter, as a Stop order does), with
//!    bit 1 (facing) turns it toward `facing` through its locomotor's
//!    Do_Turn ([`crate::sim::movement::locomotor_do_turn`]: Drive, Ship,
//!    Hover and Mech turn at their ROT, Walk snaps), and with bit 0 (move)
//!    takes the frame: the native Process does not run and the mover is
//!    displaced by `displacement`. Without bit 0 the native Process runs
//!    after the stop and the turn.
//!
//! The step ([`Simulation::module_ground_step`]): each axis of the
//! displacement is clamped to [`MAX_STEP`] leptons (as `ProjectileFrame`'s);
//! a zero displacement holds the mover where it is. A step into another cell
//! checks that cell only (not the cells crossed, as RFC 9's projectile
//! rule): it must be on the map, the mover must not be on a bridge deck,
//! and its own Can_Enter_Cell (`Simulation::mover_can_enter`, the Unit
//! `0x0073F0A0` / Infantry `0x0051BF90` receiver with the locomotor check)
//! must answer 0 (free). Otherwise the mover stays (and a push ends). An
//! accepted step lifts the mover's mark (Mark(UP)), writes the Location
//! (`FootClass::SetLocation`, riders and an Ares parasite follow), keeps its
//! height above the ground (SetHeight), marks it again (Mark(DOWN), which
//! discovers the new cell for the owner), and, when the cell changed, ends
//! the path queue (`Path[0] = -1`: the next native Process asks Find_Path
//! from the new cell) and runs `Per_Cell_Process(2)` (crates, tiberium,
//! mines and cell tags, the arrival arms; the Infantry form's bridge answer
//! is passed on).
//!
//! `Push` (100035, code 1035): `cell` the direction (from the object toward
//! that cell's center), `value` the distance in leptons (negative pushes
//! away from the cell; at most [`MAX_PUSH`]; 0 ends a push), `value2` the
//! frames it takes (1..255; 0 or less: one per 64 leptons). The distance is
//! spread over the frames in integers (frame `k` of `n` moves
//! `total * k / n - total * (k - 1) / n`), each frame a step as above. The
//! push runs in addition to the object's own movement: its destination and
//! mission stay, and its locomotor carries on from where the push left it.
//! A new `Push` replaces the object's push; a refused step, a `Teleport`,
//! a `Convert` or the object leaving ends it. A push is kept per object in
//! [`ModuleMotions`] (saved and hashed while one runs) and waits while the
//! object is not a mover here (in a transport, warping, a step in flight).
//!
//! `SetFacing` (100045, code 1045): `value` the body facing 0..255; the
//! locomotor's Do_Turn as above, for any object with a locomotor
//! (Fly snaps its secondary facing, Rocket ignores it).
//!
//! Random numbers (checklist 4.4): a frame a module takes skips the native
//! Process, so the draws that Process would have made at a step's start
//! (Walk's sub-cell choice) do not happen; running the Process for its
//! draws alone would move the object. The step itself draws nothing;
//! `Per_Cell_Process` draws as it does natively.
//!
//! Standing infantry do not sleep (`world::idle_sleep`) while a push runs
//! or a `MoveFrame` decider exists, and a `Push` wakes its object.
//!
//! Residuals:
//! - An infantryman already asleep when the first `MoveFrame` decider is
//!   made is asked from its next full turn (at most 64 frames later).
//! - Infantry Scatter's immediate Walk Process (`0x0051D478`) is a
//!   locomotor Process too: a scattered infantryman may be asked twice in
//!   a frame.
//! - Can_Enter_Cell's crush codes (4, 5) and the moving-friendly code (2)
//!   stop a step like any other refusal: a module step does not crush or
//!   wait for traffic; a crushable occupant blocks it.
//! - A step onto a bridge cell stays at the ground layer; a mover on a deck
//!   does not step off it (MOV-X35's transition is not run).
//! - An infantryman keeps its sub-cell spot index in the new cell, even
//!   when another infantryman holds that spot.
//! - The facing does not follow the step; a module turns the mover itself.
//! - No `ROT` modifier: a FacingClass keeps the rate it was built with.

use std::collections::BTreeMap;

use vera20k_wasm_ext::Status;
use vera20k_wasm_ext::decide::{EVENT_MOVE_FRAME, MoveStep};

use super::ModuleEvent;
use super::projectiles::MAX_STEP;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
use crate::sim::movement::track_process::TrackFamily;
use crate::sim::world::{FrameAdvanceError, FrameEffects, Simulation};

/// The longest `Push`, in leptons (eight cells).
pub(crate) const MAX_PUSH: i64 = 2048;
/// The most frames one `Push` takes.
const MAX_PUSH_FRAMES: i64 = 255;
/// A `Push` without frames moves this many leptons a frame.
const PUSH_LEPTONS_PER_FRAME: u64 = 64;

/// The pushes in progress (module docs), by object creation serial.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct ModuleMotions {
    pushes: BTreeMap<u64, Push>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct Push {
    /// The whole displacement, x and y in leptons.
    total: [i32; 2],
    /// Frames the push takes, 1..=255.
    frames: u16,
    /// Frames taken.
    done: u16,
}

impl ModuleMotions {
    pub(crate) fn is_empty(&self) -> bool {
        self.pushes.is_empty()
    }

    pub(crate) fn end_push(&mut self, id: u64) {
        if !self.pushes.is_empty() {
            self.pushes.remove(&id);
        }
    }

    /// The next frame's share of `id`'s push; the push ends with its last.
    fn next_push_step(&mut self, id: u64) -> Option<[i32; 2]> {
        let push = self.pushes.get_mut(&id)?;
        push.done = push.done.saturating_add(1);
        let (frames, done) = (i64::from(push.frames), i64::from(push.done));
        let step = push.total.map(|total| {
            let total = i64::from(total);
            (total * done / frames - total * (done - 1) / frames) as i32
        });
        if push.done >= push.frames {
            self.pushes.remove(&id);
        }
        Some(step)
    }

    /// The table as checksum bytes, in serial order.
    pub(crate) fn state_bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.pushes.iter().flat_map(|(id, push)| {
            id.to_le_bytes()
                .into_iter()
                .chain(push.total.iter().flat_map(|v| v.to_le_bytes()))
                .chain(push.frames.to_le_bytes())
                .chain(push.done.to_le_bytes())
        })
    }
}

/// Whether `entity` is one of the movers module movement covers (module
/// docs).
fn module_mover(entity: &GameEntity) -> bool {
    matches!(
        entity.category(),
        EntityCategory::Unit | EntityCategory::Infantry
    ) && entity.lifecycle.object_alive()
        && !entity.lifecycle.in_limbo()
        && entity.passenger_role().transporter().is_none()
        && entity.unit_follower().is_none()
        && !entity.is_unit_follower()
        && entity.locomotor.as_ref().is_some_and(|locomotor| {
            matches!(
                locomotor.active_kind(),
                LocomotorKind::Drive
                    | LocomotorKind::Ship
                    | LocomotorKind::Walk
                    | LocomotorKind::Mech
                    | LocomotorKind::Hover
            )
        })
}

/// Whether `entity`'s locomotor is inside a step (module docs): a committed
/// Drive/Ship HeadToCoord, or a Walk, Mech or Hover head.
fn step_in_flight(entity: &GameEntity) -> bool {
    let Some(locomotor) = entity.locomotor.as_ref() else {
        return false;
    };
    match TrackFamily::from_kind(locomotor.active_kind()) {
        Some(family) => locomotor
            .track_head(family)
            .is_some_and(|head| head.x != 0 || head.y != 0 || head.z != 0),
        None => locomotor.step_head().is_some(),
    }
}

/// What a frame a module took did, for the locomotor's caller.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ModuleFrame {
    /// The step entered a new cell and ran `Per_Cell_Process(2)`.
    pub(crate) per_cell_ran: bool,
    /// That Per_Cell_Process changed a bridge.
    pub(crate) bridge_state_changed: bool,
}

/// What [`Simulation::module_ground_step`] did.
struct Stepped {
    /// The mover moved (or held still on a zero step).
    moved: bool,
    frame: ModuleFrame,
}

impl Simulation {
    /// Whether [`Self::module_locomotor_frame`] can take any frame: a push
    /// runs or a `MoveFrame` decider exists. One test per Process.
    pub(crate) fn module_motion_active(&self) -> bool {
        !self.ext_modules.motions.is_empty()
            || self.trigger_runtime.module_decides(EVENT_MOVE_FRAME)
    }

    /// A push's step or `MoveFrame`'s answer for mover `id` at its
    /// locomotor's Process (module docs); `None` leaves the frame to the
    /// native Process.
    pub(crate) fn module_locomotor_frame(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> Result<Option<ModuleFrame>, FrameAdvanceError> {
        let pushing = self.ext_modules.motions.pushes.contains_key(&id);
        if !pushing && !self.trigger_runtime.module_decides(EVENT_MOVE_FRAME) {
            return Ok(None);
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(None);
        };
        if !module_mover(entity) || step_in_flight(entity) {
            return Ok(None);
        }
        let moving = crate::sim::movement::motion_query::is_moving(entity) == Some(true);
        let (other, cell) = match entity.navigation.nav_com() {
            Some(NavTargetRef::Object { id: target }) => (target, None),
            Some(NavTargetRef::Cell { rx, ry }) => (0, Some((rx, ry))),
            None => (0, None),
        };
        // Every Process entry completes a deferred destination setter first.
        self.complete_pending_order(id, rules, effects);
        if pushing {
            let Some(delta) = self.ext_modules.motions.next_push_step(id) else {
                return Ok(None);
            };
            let stepped = self.module_ground_step(id, delta, rules, registry, effects)?;
            if !stepped.moved {
                self.ext_modules.motions.end_push(id);
            }
            return Ok(Some(stepped.frame));
        }
        let event = ModuleEvent {
            other,
            cell,
            value: i64::from(moving),
            at_subject: true,
            ..ModuleEvent::new(EVENT_MOVE_FRAME, id)
        };
        let mut area = MoveStep::default().to_bytes().to_vec();
        if self.decide_module_area(rules, &event, 0, &mut area).is_none() {
            return Ok(None);
        }
        let Some(step) = MoveStep::from_bytes(&area) else {
            return Ok(None);
        };
        if step.flags & MoveStep::STOP != 0 {
            self.assign_null_destination(id, rules, registry, effects);
        }
        if step.flags & MoveStep::FACING != 0
            && let Ok(facing) = u8::try_from(step.facing)
        {
            self.module_turn(id, facing);
        }
        if step.flags & MoveStep::MOVE == 0 {
            return Ok(None);
        }
        let stepped = self.module_ground_step(id, step.displacement, rules, registry, effects)?;
        Ok(Some(stepped.frame))
    }

    /// The locomotor's Do_Turn toward body facing `facing` (0..255);
    /// false without a locomotor.
    fn module_turn(&mut self, id: u64, facing: u8) -> bool {
        let frame = self.session.binary_frame();
        self.substrate.entities.get_mut(id).is_some_and(|entity| {
            crate::sim::movement::locomotor_do_turn(entity, u16::from(facing) << 8, frame)
        })
    }

    /// One module step of mover `id` by `delta` (module docs).
    fn module_ground_step(
        &mut self,
        id: u64,
        delta: [i32; 2],
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: &mut FrameEffects<'_>,
    ) -> Result<Stepped, FrameAdvanceError> {
        let stay = Stepped {
            moved: false,
            frame: ModuleFrame::default(),
        };
        let [dx, dy] = delta.map(|d| d.clamp(-MAX_STEP, MAX_STEP));
        let Ok(from) = self.foot_current_coordinate(id) else {
            return Ok(stay);
        };
        let Some(entity) = self.substrate.entities.get(id) else {
            return Ok(stay);
        };
        if dx == 0 && dy == 0 {
            return Ok(Stepped {
                moved: true,
                frame: ModuleFrame::default(),
            });
        }
        let height = crate::sim::movement::air_movement::current_fly_height(
            entity,
            self.resolved_terrain(),
        );
        let (on_bridge, marked) = (entity.on_bridge(), entity.lifecycle.cell_marked());
        let to = DriveCoord {
            x: from.x.saturating_add(dx),
            y: from.y.saturating_add(dy),
            z: from.z,
        };
        let to_cell = crate::sim::movement::coord_cell(to);
        let crossing = crate::sim::movement::coord_cell(from) != to_cell;
        if crossing {
            let on_map = u16::try_from(to_cell.0)
                .ok()
                .zip(u16::try_from(to_cell.1).ok())
                .is_some_and(|(x, y)| {
                    self.resolved_terrain()
                        .is_some_and(|terrain| terrain.cell(x, y).is_some())
                });
            if !on_map || on_bridge {
                return Ok(stay);
            }
            let entry = self.mover_can_enter(
                id,
                to_cell,
                InfantryEntryArgs::REPAIR,
                EntryQueryMode::CheckLocomotor,
                rules,
            );
            if entry != Ok(0) {
                return Ok(stay);
            }
        }
        if marked {
            self.foot_mark_remove(id, rules, registry, effects);
        }
        crate::sim::movement::ground_pose::foot_set_location(
            &mut self.substrate.entities,
            id,
            to,
            rules,
            &self.interner,
        );
        Self::set_object_height_unmarked(&mut self.substrate, &self.resolved_terrain, id, height);
        if marked {
            self.foot_mark_put(id, rules, registry, effects);
        }
        let mut frame = ModuleFrame::default();
        if crossing {
            if let Some(entity) = self.substrate.entities.get_mut(id) {
                entity.navigation.path_replay_mut().exhaust();
            }
            frame.bridge_state_changed = self.per_cell_process(
                id,
                crate::sim::movement::PerCellReason::Arrival,
                rules,
                registry,
                effects,
            )?;
            frame.per_cell_ran = true;
        }
        Ok(Stepped { moved: true, frame })
    }

    /// `Push` on object `id` toward `cell` (module docs).
    pub(crate) fn start_module_push(
        &mut self,
        id: u64,
        cell: (i16, i16),
        force: i64,
        frames: i64,
    ) -> Status {
        let Some(entity) = self.substrate.entities.get(id) else {
            return Status::Gone;
        };
        if !module_mover(entity) || entity.active_chrono_warp().is_some() {
            return Status::Refused;
        }
        if force == 0 {
            self.ext_modules.motions.end_push(id);
            return Status::Ok;
        }
        let Ok(from) = self.foot_current_coordinate(id) else {
            return Status::Refused;
        };
        let dx = i64::from(cell.0) * 256 + 128 - i64::from(from.x);
        let dy = i64::from(cell.1) * 256 + 128 - i64::from(from.y);
        let length = isqrt((dx * dx + dy * dy) as u64) as i64;
        if length == 0 {
            return Status::Refused;
        }
        let distance = force.unsigned_abs().min(MAX_PUSH as u64) as i64 * force.signum();
        let frames = match frames {
            frames if frames > 0 => frames.min(MAX_PUSH_FRAMES),
            _ => distance.unsigned_abs().div_ceil(PUSH_LEPTONS_PER_FRAME) as i64,
        }
        .clamp(1, MAX_PUSH_FRAMES);
        let push = Push {
            total: [dx, dy].map(|d| (d * distance / length) as i32),
            frames: frames as u16,
            done: 0,
        };
        self.ext_modules.motions.pushes.insert(id, push);
        // A sleeping infantryman (`world::idle_sleep`) takes its turns again.
        self.substrate.entities.wake(id);
        Status::Ok
    }

    /// `SetFacing` on object `id` (module docs).
    pub(crate) fn module_set_facing(&mut self, id: u64, facing: u8) -> Status {
        let live = match self.substrate.entities.get(id) {
            None => return Status::Gone,
            Some(entity) => entity.lifecycle.object_alive() && !entity.lifecycle.in_limbo(),
        };
        if live && self.module_turn(id, facing) {
            Status::Ok
        } else {
            Status::Refused
        }
    }
}

/// The integer square root, rounded down.
fn isqrt(n: u64) -> u64 {
    if n == 0 {
        return 0;
    }
    let mut x = 1u64 << (64 - n.leading_zeros()).div_ceil(2);
    loop {
        let y = (x + n / x) / 2;
        if y >= x {
            return x;
        }
        x = y;
    }
}

/// Push arithmetic the movement tests (`ext_modules_movement_tests`) check.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;

    /// The per-frame shares of a push of `total` over `frames`.
    pub(crate) fn push_steps(total: [i32; 2], frames: u16) -> Vec<[i32; 2]> {
        let mut motions = ModuleMotions::default();
        motions.pushes.insert(
            1,
            Push {
                total,
                frames,
                done: 0,
            },
        );
        std::iter::from_fn(|| motions.next_push_step(1)).collect()
    }

    pub(crate) fn check_isqrt() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(15), 3);
        assert_eq!(isqrt(16), 4);
        assert_eq!(isqrt(u64::MAX), u64::from(u32::MAX));
    }
}
