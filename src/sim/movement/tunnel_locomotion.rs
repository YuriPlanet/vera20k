//! `TunnelLocomotionClass` — the stock burrowing locomotor.
//!
//! Class vtable `0x7F5A24`, size `0x3C`; `Process` `0x728E30` dispatches on
//! `[this+0x14]&nbsp;(-&nbsp;object+0x18)` through the jump table `0x7291AC`
//! (`0x728E73 / 0x728E90 / 0x728F86 / 0x729035 / 0x729042 / 0x72904F /
//! 0x7290B7` for states 1..7). The state enum is
//! `Idle=0, PreDigIn=1, DiggingIn=2, DugIn=3, Digging=4, PreDigOut=5,
//! DiggingOut=6, DugOut=7` (`TunnelLocomotionClass.h`).
//!
//! Everything here is the body of that class, not a VERA invention:
//!
//! - Dig time: `ProcessPreDigIn 0x7291F0` sets
//!   `DigTimer.duration = round(64.0 / GetType()->Speed / frame_scale)`; the
//!   binary64 at `0x7F5B18` is `64.0`.
//! - Sink/rise: `ProcessDugIn` (inline at `0x728F86`) and `ProcessPreDigOut
//!   0x7298F0` move Z by `max(5, round(Speed * frame_scale))` per frame.
//!   `DugIn` stops at the exact `0xFFFFFF00` (-256) plane and reads no ground;
//!   `PreDigOut` calls `MapClass::0x578080(&current coord)` every frame and
//!   raises to that cell's ground. The caller supplies the query as
//!   [`GroundQuery`]; this module keeps no ground sample of its own.
//! - Underground travel: `ProcessDigging 0x729580` moves toward the stored
//!   `Coords` at the binary64 `TunnelMovementSpeed` `0x7F5B20` (= 19.0), and
//!   takes the arrival branch once the 2-D distance drops under `0x14` (20
//!   leptons).
//! - `In_Which_Layer 0x72A1A0` answers `Layer::Underground` only in state 4 and
//!   `Layer::Ground` otherwise, so the layer is derived from the state rather
//!   than cached.
//! - `Can_Fire 0x72A1C0` refuses (7) whenever `State != Idle` and the base
//!   `Can_Fire 0x55AD00` said OK.
//! - `Is_To_Have_Shadow 0x72A060` is false in states 3, 4 and 5.
//! - `Mark_All_Occupation_Bits` is the empty base (`0x4B6620`, slot 39), so a
//!   Tunnel unit never claims or releases cell occupation.
//!
//! The dig and dig-out *animations* (`0x1c8`-byte Anim, ctor `0x421EA0`) are
//! assets this package does not create; the timers that gate them are.

use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;

/// Binary64 at `0x007F5B18`, the numerator of the dig timer.
const DIG_FRAMES_NUMERATOR: f64 = 64.0;
/// Binary64 at `0x007F5B20`, `TunnelMovementSpeed` in leptons per frame.
const TUNNEL_SPEED_LEPTONS: i32 = 19;
/// `0xFFFFFF00`: the exact Z `ProcessDugIn` stops sinking at.
pub(crate) const UNDERGROUND_Z: i32 = -256;
/// `ProcessDugIn`/`ProcessPreDigOut`'s per-frame floor (5 leptons).
const MIN_Z_STEP: i32 = 5;
/// `ProcessDigging 0x729580`'s arrival distance, `0x14`.
const ARRIVAL_DISTANCE: i32 = 20;
/// `ProcessPreDigIn 0x7291F0`'s frame-scale term, `[0x8871E0+0x20]`, which a
/// default-rate simulation leaves at 1.0.
const FRAME_SCALE: f64 = 1.0;

/// `TunnelLocomotionClass+0x18`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum TunnelState {
    /// 0: parked; `Is_Moving` is false and `Can_Fire` is not overridden.
    #[default]
    Idle,
    /// 1: turning the hull toward the destination before the dig timer starts.
    PreDigIn,
    /// 2: the dig-in timer (`0x729370`); the hull is already in place.
    DiggingIn,
    /// 3: sinking to [`UNDERGROUND_Z`] (`0x728F86`).
    DugIn,
    /// 4: traveling under the surface (`0x729580`); the only state whose layer
    /// is `Underground`.
    Digging,
    /// 5: rising back to the surface (`0x7298F0`).
    PreDigOut,
    /// 6: the dig-out timer (`0x729AA0`).
    DiggingOut,
    /// 7: settled, waiting for the last timer to expire (`0x729480`).
    DugOut,
}

/// `MapClass::Get_Ground_Height 0x578080(&coord)` in leptons, as
/// `ProcessPreDigOut 0x7298F0` reads it on every frame (`0x72991D`). `None` is
/// what the caller answers when it has no terrain to query, the one case that
/// falls back to [`TunnelRuntime::ground_z`].
pub(crate) type GroundQuery<'a> = &'a dyn Fn(DriveCoord) -> Option<i32>;

/// The class-local state carried in
/// [`LocomotorRuntimePayload::Tunnel`](super::locomotion::piggyback::LocomotorRuntimePayload).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, Default)]
pub struct TunnelRuntime {
    state: TunnelState,
    /// `DigTimer.start` (`+0x28`): the frame the current timer began.
    timer_start: u32,
    /// `DigTimer.duration` (`+0x34`).
    timer_duration: u32,
    /// The height the unit stood at when the dig-in timer completed, in
    /// leptons. Native keeps no such sample — `ProcessDugIn` sinks to the
    /// absolute `0xFFFFFF00` plane and `ProcessPreDigOut` re-reads `0x578080` —
    /// so this is only the terrain-free fallback for a caller without a map,
    /// plus the hash fold's record of that sample.
    #[serde(default)]
    ground_z: i32,
}

impl TunnelRuntime {
    /// The class constructor (`0x728A00`) leaves `State` at zero.
    pub(crate) fn constructed() -> Self {
        Self::default()
    }

    pub(crate) fn state(&self) -> TunnelState {
        self.state
    }

    /// `In_Which_Layer 0x72A1A0`: `state == 4` is the only underground state.
    pub(crate) fn layer(&self) -> crate::sim::movement::locomotor::MovementLayer {
        if self.state == TunnelState::Digging {
            crate::sim::movement::locomotor::MovementLayer::Underground
        } else {
            crate::sim::movement::locomotor::MovementLayer::Ground
        }
    }

    /// `Is_To_Have_Shadow 0x72A060`: no shadow while sunk, traveling or rising.
    pub(crate) fn has_shadow(&self) -> bool {
        !matches!(
            self.state,
            TunnelState::DugIn | TunnelState::Digging | TunnelState::PreDigOut
        )
    }

    /// `Is_Moving 0x728A50`.
    pub(crate) fn is_moving(&self) -> bool {
        self.state != TunnelState::Idle
    }

    /// `Is_Surfacing 0x72A1E0`.
    pub(crate) fn is_surfacing(&self) -> bool {
        matches!(self.state, TunnelState::PreDigOut | TunnelState::DiggingOut)
    }

    /// `Is_Moving_Now 0x728A60`: only `PreDigIn` answers true.
    pub(crate) fn is_moving_now(&self) -> bool {
        self.state == TunnelState::PreDigIn
    }

    /// `Can_Fire 0x72A1C0`: the base answer first, then a non-idle state
    /// refuses with 7.
    pub(crate) fn refuses_fire(&self) -> bool {
        self.state != TunnelState::Idle
    }

    /// `Stop_Moving 0x728C00`'s jump table `0x728E08`, indexed by `State - 1`:
    ///
    /// | state | target | result |
    /// |-------|--------|--------|
    /// | 1 `PreDigIn` | `0x728DE0` | unchanged; the null coordinate written there makes the next `ProcessPreDigIn` leave it at `Idle` |
    /// | 2 `DiggingIn` | `0x728DD9` | `DugOut` (7): the timer never finished, so nothing sank |
    /// | 3 `DugIn` | `0x728DD0` | `PreDigOut` (5): rise from the Z reached so far |
    /// | 4 `Digging` | `0x728C1E` | `PreDigOut`: surface where the unit stands |
    /// | 5/6/7 | `0x728DE0` | unchanged; surfacing continues |
    ///
    /// Every target also stores the null coordinate over `+0x18`; VERA's stored
    /// destination is the NavCom cell, which the same stop clears.
    pub(crate) fn stop_moving(&mut self) {
        self.state = match self.state {
            TunnelState::DiggingIn => TunnelState::DugOut,
            TunnelState::DugIn | TunnelState::Digging => TunnelState::PreDigOut,
            state => state,
        };
    }

    /// `Move_To 0x728AF0`'s tail: a destination arms `PreDigIn`, and an
    /// already-underground unit goes straight to `Digging`.
    pub(crate) fn move_to(&mut self, below_ground: bool) {
        if below_ground
            && !matches!(
                self.state,
                TunnelState::PreDigIn
                    | TunnelState::DiggingIn
                    | TunnelState::DugIn
                    | TunnelState::PreDigOut
                    | TunnelState::DiggingOut
            )
        {
            self.state = TunnelState::Digging;
            return;
        }
        if self.state == TunnelState::Idle {
            self.state = TunnelState::PreDigIn;
        }
    }

    /// Advance one frame. `destination` is the cell-centre `Move_To` stored,
    /// `speed` the type's `Speed=`, `frame` the binary frame counter and
    /// `ground_of` the `0x578080` query `ProcessPreDigOut` raises toward.
    /// Returns whether the object changed place this frame.
    pub(crate) fn process(
        &mut self,
        position: &mut crate::sim::movement::ground_pose::Position,
        facing: &mut crate::sim::movement::FacingClass,
        destination: Option<DriveCoord>,
        speed: i32,
        frame: u32,
        ground_of: GroundQuery<'_>,
    ) -> bool {
        match self.state {
            TunnelState::Idle => false,
            TunnelState::PreDigIn => self.pre_dig_in(position, facing, destination, speed, frame),
            TunnelState::DiggingIn => self.digging_in(position, frame),
            TunnelState::DugIn => self.dug_in(position, speed),
            TunnelState::Digging => self.digging(position, facing, destination, frame),
            TunnelState::PreDigOut => self.pre_dig_out(position, speed, frame, ground_of),
            TunnelState::DiggingOut => self.digging_out(frame),
            TunnelState::DugOut => self.dug_out(frame),
        }
    }

    /// `ProcessPreDigIn 0x7291F0`: wait out the hull turn, then arm the dig
    /// timer and enter `DiggingIn`.
    fn pre_dig_in(
        &mut self,
        position: &mut crate::sim::movement::ground_pose::Position,
        facing: &mut crate::sim::movement::FacingClass,
        destination: Option<DriveCoord>,
        speed: i32,
        frame: u32,
    ) -> bool {
        let Some(destination) = destination else {
            self.state = TunnelState::Idle;
            return false;
        };
        // `FacingClass::GetCurrentFacing 0x4C9480` gates the whole body while
        // the hull is still turning, and `0x4C93D0` compares against the new
        // direction; a mismatch re-issues `Do_Turn` and returns.
        if facing.is_rotating(frame) {
            return false;
        }
        let target = facing_toward(position_world(position), destination);
        if facing.current(frame) != target {
            facing.set(target, frame);
            return false;
        }
        self.start_timer(dig_frames(speed), frame);
        self.state = TunnelState::DiggingIn;
        false
    }

    /// `ProcessDiggingIn 0x729370`: `elapsed / duration == 1.0` (`0x7E1718`)
    /// completes the dig-in. The body has no ground read of its own; the object
    /// Z it leaves with is kept only as the query-failure fallback.
    fn digging_in(
        &mut self,
        position: &mut crate::sim::movement::ground_pose::Position,
        frame: u32,
    ) -> bool {
        if !self.timer_elapsed(frame) {
            return false;
        }
        self.ground_z = position_world(position).z;
        self.state = TunnelState::DugIn;
        false
    }

    /// `ProcessDugIn` (inline at `0x728F86`): sink by
    /// `max(5, round(Speed * frame_scale))` until the Location Z reaches the
    /// absolute `0xFFFFFF00` plane, where the object becomes underground
    /// (`State = 4`).
    ///
    /// This body reads no ground: `0x578080` has no caller anywhere between
    /// the Process entry `0x728E30` and the case tail `0x729030`, the entry
    /// guard compares the Location Z against the constant (`0x728FC4`,
    /// `jle 0x729021`) and the step clamp writes it (`0x729001`). The plane is
    /// the same `-256` `Move_To` stores as the travel destination, whatever
    /// ground the unit dug in from.
    fn dug_in(
        &mut self,
        position: &mut crate::sim::movement::ground_pose::Position,
        speed: i32,
    ) -> bool {
        let coord = position_world(position);
        if coord.z <= UNDERGROUND_Z {
            self.state = TunnelState::Digging;
            return false;
        }
        let step = z_step(speed);
        let z = (coord.z - step).max(UNDERGROUND_Z);
        position.put_location(DriveCoord { z, ..coord });
        if z <= UNDERGROUND_Z {
            self.state = TunnelState::Digging;
        }
        true
    }

    /// `ProcessDigging 0x729580`: straight-line travel at
    /// `TunnelMovementSpeed` toward the stored coordinate. Reaching within
    /// [`ARRIVAL_DISTANCE`] takes the arrival branch (`0x7297F6`), which enters
    /// `PreDigOut` and leaves the `Underground` layer.
    fn digging(
        &mut self,
        position: &mut crate::sim::movement::ground_pose::Position,
        facing: &mut crate::sim::movement::FacingClass,
        destination: Option<DriveCoord>,
        frame: u32,
    ) -> bool {
        let Some(destination) = destination else {
            // Without a stored coordinate the class has nothing to travel
            // toward; `Stop_Moving 0x728C00` owns the aborted-dig cancellation.
            return false;
        };
        let from = position_world(position);
        let dx = destination.x - from.x;
        let dy = destination.y - from.y;
        let distance = dx.abs().max(dy.abs());
        if distance <= ARRIVAL_DISTANCE {
            position.put_location(destination);
            self.state = TunnelState::PreDigOut;
            self.timer_start = 0;
            self.timer_duration = 0;
            return true;
        }
        let step = TUNNEL_SPEED_LEPTONS;
        let target = facing_toward(from, destination);
        if facing.current(frame) != target {
            facing.snap(target, frame);
        }
        let next = DriveCoord {
            x: from.x + (dx.signum() * step),
            y: from.y + (dy.signum() * step),
            z: from.z,
        };
        position.put_location(next);
        true
    }

    /// `ProcessPreDigOut 0x7298F0`: read the object's current cell
    /// (`0x578080(&coord)`), raise Z by `max(5, round(rate * frame_scale))` up
    /// to that ground, then arm the dig-out timer the same `round(64.0 / Speed)`
    /// way (`0x729A54`) and enter `DiggingOut`.
    fn pre_dig_out(
        &mut self,
        position: &mut crate::sim::movement::ground_pose::Position,
        speed: i32,
        frame: u32,
        ground_of: GroundQuery<'_>,
    ) -> bool {
        let coord = position_world(position);
        let ground = ground_of(coord).unwrap_or(self.ground_z);
        let step = z_step(speed);
        if coord.z >= ground {
            position.put_location(DriveCoord { z: ground, ..coord });
            self.start_timer(dig_frames(speed), frame);
            self.state = TunnelState::DiggingOut;
            return true;
        }
        position.put_location(DriveCoord {
            z: (coord.z + step).min(ground),
            ..coord
        });
        true
    }

    /// `ProcessDiggingOut 0x729AA0`: the dig-out timer, then `DugOut`.
    fn digging_out(&mut self, frame: u32) -> bool {
        if !self.timer_elapsed(frame) {
            return false;
        }
        self.state = TunnelState::DugOut;
        false
    }

    /// `ProcessDugOut 0x729480`: the last timer, then back to `Idle`.
    fn dug_out(&mut self, frame: u32) -> bool {
        if !self.timer_elapsed(frame) {
            return false;
        }
        self.state = TunnelState::Idle;
        false
    }

    fn start_timer(&mut self, duration: u32, frame: u32) {
        self.timer_start = frame;
        self.timer_duration = duration;
    }

    /// The defined fields the world hash folds: `State`, `DigTimer.start` and
    /// `DigTimer.duration`.
    pub(crate) fn hash_fields(&self) -> (u8, u32, u32, i32) {
        (
            self.state as u8,
            self.timer_start,
            self.timer_duration,
            self.ground_z,
        )
    }

    /// `RateTimer` completion as `ProcessDiggingIn` reads it: the elapsed
    /// fraction reaches the `1.0` constant at `0x7E1718`.
    fn timer_elapsed(&self, frame: u32) -> bool {
        frame.saturating_sub(self.timer_start) >= self.timer_duration
    }
}

/// `round(64.0 / Speed / frame_scale)`, floored at one frame.
fn dig_frames(speed: i32) -> u32 {
    let speed = f64::from(speed.max(1));
    let frames = (DIG_FRAMES_NUMERATOR / speed / FRAME_SCALE).round();
    (frames.max(1.0)) as u32
}

/// `max(5, round(Speed * frame_scale))`.
fn z_step(speed: i32) -> i32 {
    ((f64::from(speed) * FRAME_SCALE).round() as i32).max(MIN_Z_STEP)
}

fn position_world(position: &crate::sim::movement::ground_pose::Position) -> DriveCoord {
    crate::sim::movement::ground_pose::position_world_coord(position)
}

/// Facing (0 north, clockwise, 65536 units) from `from` toward `to`.
///
/// The native body (`0x7291F0`) feeds `atan2 0x4CAE30`, subtracts `π/2` at
/// `0x7E2820` and multiplies by `-65536/2π` at `0x7E2818`; that is the same
/// angle this computes directly.
fn facing_toward(from: DriveCoord, to: DriveCoord) -> u16 {
    let dx = f64::from(to.x - from.x);
    let north = f64::from(from.y - to.y);
    let turn = dx.atan2(north);
    let units = turn * (65536.0 / std::f64::consts::TAU);
    (units.round() as i64).rem_euclid(65536) as u16
}

/// Run the Tunnel controller for one entity this frame.
///
/// `ground_of` is the caller's `MapClass::0x578080` query, the answer
/// `ProcessPreDigOut` rebuilds its surfacing target from every frame.
pub(crate) fn process(
    entity: &mut GameEntity,
    speed: i32,
    frame: u32,
    ground_of: GroundQuery<'_>,
) -> bool {
    let destination = destination_coord(entity);
    let Some(loco) = entity.locomotor.as_mut() else {
        return false;
    };
    if loco.active_kind() != crate::rules::locomotor_type::LocomotorKind::Tunnel {
        return false;
    }
    let Some(runtime) = loco.tunnel_runtime_mut() else {
        return false;
    };
    runtime.process(
        &mut entity.position,
        &mut entity.body_facing,
        destination,
        speed,
        frame,
        ground_of,
    )
}

/// `Move_To 0x728AF0`'s stored coordinate: the destination cell's centre.
/// `Stop_Moving 0x728C00`: its jump table `0x728E08` maps states 1/5/6/7 to
/// `0x728DE0`, state 2 to `0x728DD9`, state 3 to `0x728DD0` and state 4 to
/// `0x728C1E`, whose body measures the object's location against the stored
/// coordinate — a travelling unit surfaces where it is instead of popping up.
pub(crate) fn stop_moving(entity: &mut GameEntity) {
    // Every `Stop_Moving` target writes the null coordinate over `+0x18`
    // (`0x728DE0`, and the `0x728DD9`/`0x728DD0` arms reach it); the NavCom cell
    // is this class's stored destination.
    entity.navigation.stop_moving();
    if let Some(runtime) = entity
        .locomotor
        .as_mut()
        .and_then(|loco| loco.tunnel_runtime_mut())
    {
        runtime.stop_moving();
    }
}

/// `Move_To 0x728AF0`'s stored coordinate: the destination cell's centre.
pub(crate) fn destination_coord(entity: &GameEntity) -> Option<DriveCoord> {
    match entity.navigation.nav_com() {
        Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) => {
            Some(DriveCoord::cell(rx, ry, UNDERGROUND_Z))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "tunnel_locomotion_tests.rs"]
mod tests;
