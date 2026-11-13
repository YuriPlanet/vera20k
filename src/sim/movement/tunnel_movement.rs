//! Tunnel locomotor: `TunnelLocomotionClass`, the subterranean mover.
//!
//! CLSID `{4A582743-9839-11D1-B709-00A024DDAFD1}` (`0x007E9A50`), constructor
//! `0x00728A00`, ILocomotion vtable `0x007F5A24`. No retail type selects it;
//! a mod does through `Locomotor=`. The class has no IPiggyback: a Drive or a
//! Chrono Warp's Teleport can suspend it, it suspends nothing.
//!
//! The object keeps State (`+0x18`), Destination (`+0x1C`), DigTimer
//! (`+0x28`) and the timer's rate (`+0x34`). Move_To (`0x00728AF0`) arms
//! State 1, or State 4 for an owner already below ground. Process
//! (`0x00728E30`) then runs, by State:
//! 1. PreDigIn (`0x007291F0`): turn the owner toward Destination, then start
//!    the dig-in timer, play the dig effects and enter State 2;
//! 2. DiggingIn (inline at `0x00728E90`): when the timer runs out, full speed,
//!    State 3, the dig effects and the owner's Detach_All;
//! 3. DugIn (`0x00728F86`): sink the owner to Z -256, then Mark(UP) and State 4;
//! 4. Digging (`0x00729580`): travel 19 leptons a frame straight toward
//!    Destination, below every cell; near it, Mark(DOWN) and State 5;
//! 5. PreDigOut (`0x007298F0`): rise to the ground, then See, start the
//!    dig-out timer, stop and enter State 6;
//! 6. DiggingOut (`0x0072904F`): when the timer runs out, PerCellProcess(2)
//!    and the end of the move;
//! 7. DugOut (`0x007290B7`): a dig-in that Stop_Moving broke off ends when its
//!    timer runs out.
//!
//! This module owns that state and the class's arithmetic;
//! `world::tunnel_process` runs Process, Move_To and Stop_Moving with the
//! owner calls in their native order.
//!
//! Not kept: the byte at `+0x38`. ProcessPreDigIn and ProcessDigging clear
//! it and the draw query Z_Adjust (`0x00729F74..0x00729F8C`) sets it, and
//! only that same Z_Adjust test reads it.
//!
//! RESIDUAL (persistence): native Load (`0x0072A150`) restarts DigTimer at
//! the load frame with a zero duration and rate, so a timed phase (State 2, 6
//! or 7) running at a save ends at the first Process after the load. VERA's
//! snapshot keeps the timer, so after a load that phase runs to its end.
//! Trigger: a save while a Tunnel unit digs. Effect: the dig ends up to
//! `64 / ROT` frames later than native. Mod-only.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on util/ and sim/components, sim/timer.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::sim::components::DriveCoord;
use crate::sim::timer::CdTimer;
use crate::util::fixed_math::SimFixed;

/// Native's null coordinate (`0x00B0F910`).
pub(crate) const NULL_COORD: DriveCoord = DriveCoord { x: 0, y: 0, z: 0 };
/// ProcessDigging's step, `[0x007F5B20]` = 19.0 leptons a frame
/// (`0x00729893`, `0x007298B1`).
pub(crate) const TUNNEL_MOVEMENT_SPEED: i32 = 19;
/// The owner's Z underground: DugIn sinks to it (`0x00728FB0..0x00728FCB`)
/// and ProcessDigging aims at it (`0x007295E6`).
pub(crate) const TUNNEL_DEPTH_Z: i32 = -0x100;
/// ProcessDigging arrives within this many leptons of Destination
/// (`Point2D::Length @ 0x004C1B50` at `0x007295C0..0x007295CA`).
pub(crate) const TUNNEL_ARRIVAL_RADIUS: i32 = 0x14;
/// Height above which ProcessDigging plays the dig-out sound and
/// ProcessPreDigOut its dig effects (`0x007297BC`, `0x00729A0C`).
pub(crate) const TUNNEL_SURFACE_EFFECT_HEIGHT: i32 = -0x32;

/// State (`+0x18`). The names follow Process's dispatch (`0x007291AC`).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum TunnelState {
    #[default]
    Idle,
    PreDigIn,
    DiggingIn,
    DugIn,
    Digging,
    PreDigOut,
    DiggingOut,
    DugOut,
}

impl TunnelState {
    /// The native State value.
    pub(crate) const fn raw(self) -> u8 {
        self as u8
    }
}

/// Which dig effects a Process step plays at the owner: the `[AudioVisual]
/// DigSound=` and, with `anim`, the `Dig=` animation. Ares 3.0 replaces each
/// pair with the type's `DigInSound`/`DigIn` or `DigOutSound`/`DigOut`
/// (`PlayDigEffects`, Ares.dll `0x100443E0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DigEffects {
    pub(crate) dig_in: bool,
    pub(crate) anim: bool,
}

impl DigEffects {
    /// ProcessPreDigIn's start (`0x007292DD..0x00729365`) and DiggingIn's end
    /// (`0x00728EF0..0x00728F74`).
    pub(crate) const DIG_IN: Self = Self {
        dig_in: true,
        anim: true,
    };
    /// ProcessDigging's arrival, a sound only (`0x007297C4..0x007297EE`).
    pub(crate) const SURFACING: Self = Self {
        dig_in: false,
        anim: false,
    };
    /// ProcessPreDigOut's rise past the effect height (`0x007299A9..0x00729A34`).
    pub(crate) const DIG_OUT: Self = Self {
        dig_in: false,
        anim: true,
    };
}

/// One `TunnelLocomotionClass` object's state.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TunnelRuntime {
    /// `+0x18`.
    state: TunnelState,
    /// `+0x1C`. The zero coordinate is native's null (`0x00B0F910`).
    destination: DriveCoord,
    /// `+0x28`: start frame and duration (its `+0x2C` dword is a stale stack
    /// copy nothing reads).
    dig_timer: CdTimer,
    /// `+0x34`: the timer's full length. Process, Draw_Matrix and Z_Adjust
    /// take the phase's progress as `(rate - left) / rate`; a zero rate is
    /// done.
    dig_timer_rate: i32,
}

impl TunnelRuntime {
    /// The constructor (`0x00728A00`): Idle, the null Destination, and
    /// DigTimer started at the frame with no duration and no rate.
    pub(crate) fn constructed(binary_frame: u32) -> Self {
        Self {
            state: TunnelState::Idle,
            destination: NULL_COORD,
            dig_timer: CdTimer::started(binary_frame as i32, 0),
            dig_timer_rate: 0,
        }
    }

    pub fn state(&self) -> TunnelState {
        self.state
    }

    pub(crate) fn set_state(&mut self, state: TunnelState) {
        self.state = state;
    }

    /// `Is_Moving` (`0x00728A50`): State is not Idle.
    pub(crate) fn is_moving(&self) -> bool {
        self.state != TunnelState::Idle
    }

    /// `Destination` (`0x00728A90`): the stored coordinate while moving, else
    /// the owner's `location`.
    pub(crate) fn destination_or(&self, location: DriveCoord) -> DriveCoord {
        if self.is_moving() {
            self.destination
        } else {
            location
        }
    }

    /// `+0x1C` as stored.
    pub(crate) fn raw_destination(&self) -> DriveCoord {
        self.destination
    }

    pub(crate) fn set_destination(&mut self, destination: DriveCoord) {
        self.destination = destination;
    }

    /// Start DigTimer for `frames` and keep them as its rate
    /// (ProcessPreDigIn `0x007292A9..0x007292DD`, ProcessPreDigOut
    /// `0x00729A5B..0x00729A81`).
    pub(crate) fn start_dig_timer(&mut self, binary_frame: u32, frames: i32) {
        self.dig_timer = CdTimer::started(binary_frame as i32, frames);
        self.dig_timer_rate = frames;
    }

    /// Process's timed-phase test (`0x00728E9B..0x00728ECA`, and States 6
    /// and 7): `(rate - left) / rate == 1.0`, or a zero rate. For a nonzero
    /// rate the quotient is exactly 1.0 only when nothing is left.
    pub(crate) fn dig_timer_done(&self, binary_frame: u32) -> bool {
        self.dig_timer_rate == 0 || self.dig_timer.remaining(binary_frame as i32) == 0
    }

    /// The timed phase's progress, `(rate - left) / rate`, with a zero rate
    /// as 1.0: what Draw_Matrix (`0x00729B40`) and Z_Adjust (`0x00729E50`)
    /// tilt and lower the owner by.
    pub fn dig_progress(&self, binary_frame: u32) -> f32 {
        if self.dig_timer_rate == 0 {
            return 1.0;
        }
        let left = self.dig_timer.remaining(binary_frame as i32);
        (self.dig_timer_rate - left) as f32 / self.dig_timer_rate as f32
    }

    /// `In_Which_Layer` (`0x0072A1A0`): underground in State 4, else ground.
    pub fn underground(&self) -> bool {
        self.state == TunnelState::Digging
    }

    /// `Can_Fire` (`0x0072A1C0`): the base answer, or 7 (FireError) while
    /// State is not Idle.
    pub(crate) fn blocks_fire(&self) -> bool {
        self.is_moving()
    }

    /// `Is_Surfacing` (`0x0072A1E0`): PreDigOut or DiggingOut.
    pub(crate) fn is_surfacing(&self) -> bool {
        matches!(self.state, TunnelState::PreDigOut | TunnelState::DiggingOut)
    }

    /// `Is_To_Have_Shadow` (`0x0072A060`): Idle, PreDigIn, DiggingIn,
    /// DiggingOut and DugOut draw a shadow.
    pub fn has_shadow(&self) -> bool {
        matches!(
            self.state,
            TunnelState::Idle
                | TunnelState::PreDigIn
                | TunnelState::DiggingIn
                | TunnelState::DiggingOut
                | TunnelState::DugOut
        )
    }

    /// `Draw_Matrix` (`0x00729B40`)'s tilt about the Y axis while State is
    /// set, in radians: DiggingIn tips forward by progress x pi/2, DugIn by
    /// pi/2, PreDigOut by -pi/2, DiggingOut by (1 - progress) x -pi/2, DugOut
    /// by (1 - progress) x pi/2, the rest not at all. `None` while Idle, when
    /// the ground slope's matrix applies instead.
    pub fn draw_tilt(&self, binary_frame: u32) -> Option<f32> {
        use std::f32::consts::FRAC_PI_2;
        let progress = || self.dig_progress(binary_frame);
        Some(match self.state {
            TunnelState::Idle => return None,
            TunnelState::DiggingIn => progress() * FRAC_PI_2,
            TunnelState::DugIn => FRAC_PI_2,
            TunnelState::PreDigOut => -FRAC_PI_2,
            TunnelState::DiggingOut => (1.0 - progress()) * -FRAC_PI_2,
            TunnelState::DugOut => (1.0 - progress()) * FRAC_PI_2,
            TunnelState::PreDigIn | TunnelState::Digging => 0.0,
        })
    }

    /// Stop_Moving's State table (`0x00728C17`, table `0x00728E08`) apart
    /// from State 4's search: DiggingIn becomes DugOut and DugIn PreDigOut;
    /// PreDigIn, PreDigOut, DiggingOut and DugOut (and those two after
    /// their change) null Destination. Answers whether the State 4 arm
    /// applies.
    pub(crate) fn stop_moving(&mut self) -> bool {
        match self.state {
            TunnelState::Idle => return false,
            TunnelState::Digging => return true,
            TunnelState::DiggingIn => self.state = TunnelState::DugOut,
            TunnelState::DugIn => self.state = TunnelState::PreDigOut,
            TunnelState::PreDigIn
            | TunnelState::PreDigOut
            | TunnelState::DiggingOut
            | TunnelState::DugOut => {}
        }
        self.destination = NULL_COORD;
        false
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn for_test(state: TunnelState, destination: DriveCoord) -> Self {
        Self {
            state,
            destination,
            ..Self::constructed(0)
        }
    }
}

/// `64.0 / ROT / TunnelSpeed`, cut by ftol: ProcessPreDigIn's dig-in frames
/// (`0x0072929F..0x007292B5`; `[0x007F5B18]` = 64.0, TechnoType `+0x71C`,
/// Rules `+0x20`). A zero divisor gives ftol's integer indefinite.
pub(crate) fn dig_in_frames(rot: i32, tunnel_speed: SimFixed) -> i32 {
    if rot == 0 || tunnel_speed == SimFixed::ZERO {
        return i32::MIN;
    }
    let frames = SimFixed::from_num(64) / SimFixed::from_num(rot) / tunnel_speed;
    frames.to_num::<i32>()
}

/// `64.0 / ROT`, cut by ftol: ProcessPreDigOut's dig-out frames
/// (`0x00729A4E..0x00729A5B`).
pub(crate) fn dig_out_frames(rot: i32) -> i32 {
    if rot == 0 {
        return i32::MIN;
    }
    (SimFixed::from_num(64) / SimFixed::from_num(rot)).to_num::<i32>()
}

/// DugIn's and PreDigOut's vertical step: `ftol(GetCurrentSpeed *
/// TunnelSpeed)`, and 5 for anything below 6 (`0x00728FD0..0x00728FEC`,
/// `0x00729948..0x0072996B`).
pub(crate) fn vertical_step(current_speed: i32, tunnel_speed: SimFixed) -> i32 {
    let step = (SimFixed::saturating_from_num(current_speed) * tunnel_speed).to_num::<i32>();
    if step < 6 { 5 } else { step }
}

/// The facing word from `from` toward `to`: `atan2(-(dy), dx)` through
/// `(angle - pi/2) * -10430.06` and ftol (ProcessPreDigIn
/// `0x00729229..0x0072926B`, ProcessDigging `0x0072980F..0x00729850`).
pub(crate) fn facing_toward(from: DriveCoord, to: DriveCoord) -> u16 {
    crate::util::direction_tables::facing16_from_delta(
        to.x.wrapping_sub(from.x),
        to.y.wrapping_sub(from.y),
    )
}

/// ProcessDigging's travel (`0x0072980F..0x007298CF`): 19 leptons along
/// [`facing_toward`] through the sine and cosine tables, Z unchanged.
pub(crate) fn underground_step(from: DriveCoord, to: DriveCoord) -> DriveCoord {
    let [x, y] = crate::util::native_trig::facing_step_world_xy(
        [from.x, from.y],
        facing_toward(from, to),
        TUNNEL_MOVEMENT_SPEED,
    );
    DriveCoord { x, y, z: from.z }
}

/// ProcessDigging's arrival test: `Point2D::Length` of `to - from` below
/// [`TUNNEL_ARRIVAL_RADIUS`].
pub(crate) fn within_arrival_radius(from: DriveCoord, to: DriveCoord) -> bool {
    crate::util::native_x87::sqrt_approx_length([
        to.x.wrapping_sub(from.x),
        to.y.wrapping_sub(from.y),
    ]) < TUNNEL_ARRIVAL_RADIUS
}

/// Stop_Moving's State 4 test (`0x00728C2D..0x00728C7E`): the owner is
/// farther from Destination than the cell diagonal (`[0x00B0F8F8]`,
/// Sqrt_Approx of `2 * 256²`, static initializer `0x007287F0`).
pub(crate) fn beyond_cell_diagonal(from: DriveCoord, to: DriveCoord) -> bool {
    use crate::util::native_x87::{X87Chop53, X87Ordering, sqrt_approx_f32, sqrt_approx_length};
    let distance = sqrt_approx_length([from.y.wrapping_sub(to.y), from.x.wrapping_sub(to.x)]);
    let diagonal = sqrt_approx_f32(X87Chop53::load_i32(2 * 256 * 256))
        .and_then(X87Chop53::load_f32)
        .expect("Sqrt_Approx of a finite square is finite");
    X87Chop53::compare(diagonal, X87Chop53::load_i32(distance)) == X87Ordering::Less
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn coord(x: i32, y: i32, z: i32) -> DriveCoord {
        DriveCoord { x, y, z }
    }

    /// Stop_Moving breaks a dig-in off into DugOut and a sink into PreDigOut;
    /// only the underground State keeps its Destination for the search.
    #[test]
    fn stop_moving_follows_the_state_table() {
        let target = coord(0x1080, 0x2080, 0);
        for (from, to, searches, cleared) in [
            (TunnelState::DiggingIn, TunnelState::DugOut, false, true),
            (TunnelState::DugIn, TunnelState::PreDigOut, false, true),
            (TunnelState::PreDigIn, TunnelState::PreDigIn, false, true),
            (TunnelState::Digging, TunnelState::Digging, true, false),
            (TunnelState::Idle, TunnelState::Idle, false, false),
        ] {
            let mut runtime = TunnelRuntime::for_test(from, target);
            assert_eq!(runtime.stop_moving(), searches, "{from:?}");
            assert_eq!(runtime.state(), to, "{from:?}");
            assert_eq!(runtime.raw_destination() == NULL_COORD, cleared, "{from:?}");
        }
    }

    /// The vertical step floors at 5 below 6, and the dig frames truncate.
    #[test]
    fn dig_arithmetic_truncates_and_floors() {
        assert_eq!(vertical_step(0, SimFixed::ONE), 5);
        assert_eq!(vertical_step(6, SimFixed::ONE), 6);
        assert_eq!(vertical_step(9, SimFixed::from_num(1.5)), 13);
        assert_eq!(dig_in_frames(5, SimFixed::ONE), 12);
        assert_eq!(dig_in_frames(5, SimFixed::from_num(2)), 6);
        assert_eq!(dig_out_frames(7), 9);
    }

    /// Underground travel moves 19 leptons along the bearing, and arrival
    /// is the first point within 0x14 leptons.
    #[test]
    fn underground_step_heads_for_the_destination() {
        let from = coord(0x1000, 0x1000, TUNNEL_DEPTH_Z);
        let east = underground_step(from, coord(0x1800, 0x1000, 0));
        assert_eq!((east.x - from.x, east.y - from.y, east.z), (19, 0, -0x100));
        assert!(within_arrival_radius(from, coord(0x1010, 0x1000, 0)));
        assert!(!within_arrival_radius(from, coord(0x1020, 0x1000, 0)));
    }
}
