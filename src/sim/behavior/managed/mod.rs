//! Managed execution: a small phase machine per object, stepped by the control
//! layer.
//!
//! **These routines are not native behaviour and do not claim parity.** They
//! exist so a policy-driven house can pull a hurt unit out of a fight, walk a
//! spy around a patrol or spread an aircraft flight's fire — micro a player
//! performs by hand and neither `gamemd`'s computer AI nor VERA's assist brain
//! issues. They are enabled only for a house the assist brain drives, never for
//! a scenario's own AI house, and no golden or native comparison covers them.
//!
//! A routine is a **pure function of `(me, world, phase)`**. It owns no state:
//! the phase lives in the control layer's table
//! ([`crate::sim::control::micro`]), so replaying the same inputs against the
//! same phase produces the same step, and nothing here reads a clock, an `f32`
//! or an unordered container. That invariant is what keeps managed mode out of
//! the simulation's authoritative state and out of its checksum: a routine's
//! only output is a [`Goal`] or a wait, and the goal travels the ordinary
//! command stream.
//!
//! Adding a routine is one file, one [`MicroKind`] variant and one arm in
//! [`MicroKind::routine`]; no other module changes.

pub mod air_split;
pub mod kite;
pub mod retreat;
pub mod spy_approach;

use crate::sim::behavior::goal::Goal;
use crate::sim::behavior::view::{ObjectView, ThreatView};

/// Which managed routine drives an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MicroKind {
    /// Pull back while the weapon reloads, then re-engage.
    Kite,
    /// Wait out a patrol, then take the building.
    SpyApproach,
    /// Rotate a flight's preferred target so its fire splits.
    AirSplit,
    /// Break off a hurt unit and send it to the rally post.
    Retreat,
}

impl MicroKind {
    /// The routine for this kind. Routines are zero-sized and stateless, so one
    /// shared instance each is enough.
    pub fn routine(self) -> &'static dyn MicroRoutine {
        match self {
            Self::Kite => &kite::Kite,
            Self::SpyApproach => &spy_approach::SpyApproach,
            Self::AirSplit => &air_split::AirSplit,
            Self::Retreat => &retreat::Retreat,
        }
    }

    /// The kind's name, for diagnostics.
    pub fn name(self) -> &'static str {
        match self {
            Self::Kite => "kite",
            Self::SpyApproach => "spy-approach",
            Self::AirSplit => "air-split",
            Self::Retreat => "retreat",
        }
    }
}

/// The knobs a routine reads. Held by the control layer with the phase, not by
/// the routine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicroParams {
    /// Health fraction, in permille, at or below which the routine breaks off.
    pub health_floor: u16,
    /// Cells kept between the object and a threat it is backing away from.
    pub standoff_cells: u16,
    /// Cells within which a threat is considered at all.
    pub radius_cells: u16,
    /// Frames to wait when there is nothing to do.
    pub hold_frames: u32,
}

impl Default for MicroParams {
    fn default() -> Self {
        Self {
            health_floor: 250,
            standoff_cells: 3,
            radius_cells: 8,
            hold_frames: 15,
        }
    }
}

/// Where a routine is in its cycle.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Stage {
    /// Looking for work, or pressing an attack.
    #[default]
    Engage,
    /// Backing off.
    Withdraw,
    /// Waiting out a reload.
    Reload,
    /// Reforming at the post.
    Regroup,
    /// Standing off while a patrol blocks the way.
    Avoid,
}

/// A routine's whole memory, held by the control layer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Phase {
    pub stage: Stage,
    /// Frames left before the routine is asked again.
    pub wait: u32,
    /// The object the routine is working on.
    pub target: Option<u64>,
    /// A cell the routine works from or towards, when the caller set one.
    pub post: Option<(u16, u16)>,
    /// Steps taken in the cycle, for routines that rotate a preference.
    pub wave: u32,
}

/// One routine step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicroStep {
    /// Issue this goal for the object.
    Goal(Goal),
    /// Nothing to do for a while.
    Wait { frames: u32 },
    /// The routine has finished; the object returns to the brain.
    Done,
    /// Hand the object back to the brain, with a reason for the log.
    Fallback(FallbackReason),
}

/// Why a routine stopped managing an object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// Health below the floor and nothing to withdraw from.
    HealthBelowFloor,
    /// The routine needs a target or post the caller never set.
    NoTarget,
    /// No threat to work on.
    NothingToDo,
    /// The object stayed mid-mission for the whole give-up window, so no order
    /// of the routine's ever took (`Ready_To_Commence` answered false).
    RepeatedNotReady,
    /// The gate kept dropping the routine's orders.
    Refused,
    /// The object no longer exists.
    Gone,
}

impl FallbackReason {
    pub fn describe(self) -> &'static str {
        match self {
            Self::HealthBelowFloor => "health below the floor",
            Self::NoTarget => "no target or post",
            Self::NothingToDo => "nothing to do",
            Self::RepeatedNotReady => "mid-mission for the whole give-up window",
            Self::Refused => "orders kept being refused",
            Self::Gone => "object gone",
        }
    }
}

/// A stateless execution routine.
///
/// `step` may not touch anything but `phase`: no clock, no global, no RNG. A
/// routine that needs a random draw derives it from the caller's seeded stream
/// before the call, never from inside.
pub trait MicroRoutine: Sync {
    fn kind(&self) -> MicroKind;

    fn step(
        &self,
        me: &ObjectView,
        world: &ThreatView<'_>,
        phase: &mut Phase,
        params: &MicroParams,
    ) -> MicroStep;
}

/// One cell from `from` directly away from `threat`, `cells` cells out.
pub(crate) fn step_away(from: (u16, u16), threat: (u16, u16), cells: i32) -> (u16, u16) {
    let dx = i32::from(from.0) - i32::from(threat.0);
    let dy = i32::from(from.1) - i32::from(threat.1);
    if dx == 0 && dy == 0 {
        // Standing on the threat: pick a fixed direction so the answer stays
        // deterministic rather than depending on anything unordered.
        return (clamp_cell(i32::from(from.0) + cells.max(1)), from.1);
    }
    let span = dx.abs().max(dy.abs());
    let step = cells.clamp(1, span);
    let x = i32::from(from.0) + dx * step / span;
    let y = i32::from(from.1) + dy * step / span;
    (clamp_cell(x), clamp_cell(y))
}

fn clamp_cell(value: i32) -> u16 {
    u16::try_from(value.clamp(0, i32::from(u16::MAX))).unwrap_or(0)
}
