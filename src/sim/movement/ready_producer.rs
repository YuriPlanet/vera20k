//! Readiness inputs for the Mission gate, derived on demand from entity state.
//!
//! The readiness *predicate* lives in [`super::locomotor_ready`] and is
//! exhaustively tested against the native comparison. This module supplies its
//! *inputs* from live entity state, so the Mission readiness gate stops
//! substituting a constant "not moving".
//!
//! ## Why on demand and not once per tick
//! Native's gate is a virtual on the object's own vtable that performs a fresh
//! locomotor call every time it runs. No cached per-frame "is moving" byte
//! exists anywhere on that path, and the gate is consulted from roughly two
//! dozen sites — not just the per-object AI loop but radio receipt, per-cell
//! process, unlimbo, set-destination and the deploy sequence, all of which fire
//! mid-tick in response to events that themselves change locomotor state. The
//! same object's readiness is also evaluated on both sides of its own movement
//! step within one tick, and the two answers can differ.
//!
//! A once-per-tick cache would therefore answer nearly every one of those calls
//! with stale state. The highest-risk shape is a same-tick stop followed by a
//! mid-tick queue-and-commence — the dock, unlink, unload and deploy handoffs —
//! where a stale "moving" defers the mission.
//!
//! Verified by decompiling both readiness overrides (Infantry and Unit), the
//! queue-then-commence caller, and the live locomotor call inside the gate body.
//! The gate reads `Is_Moving_Now`, never the separate `Is_Moving` predicate —
//! those two are genuinely different in every live family except Teleport, whose
//! `Is_Moving_Now` is the inherited thunk that re-dispatches to `Is_Moving`.
//!
//! ## Why this is a separate module
//! `locomotor_ready` answers from plain inputs; producing those inputs requires
//! reading `GameEntity`, which this module does.
//!
//! ## Error direction is the safety property
//! Before this module existed the gate always answered "not moving", so the
//! moving-defer branch never fired. A producer that wrongly answers **moving**
//! makes missions defer and can stall a unit permanently; a producer that
//! wrongly answers **not moving** is no worse than the previous behaviour.
//! Every mapping below is therefore written to fail toward "not moving" when
//! its state is absent, and families without a faithful mapping return `None`
//! rather than a guess.
//!
//! Drive/Ship share their coordinate query with cell-entry and tube consumers
//! and read the owner's live `GetCurrentSpeed`. Walk reads its retained
//! byte/head and the shared Foot speed fraction. Original query comparisons
//! are in locomotor_moving.json; other family adapters still have recorded
//! limits.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/ movement and entity state only.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;
use crate::util::fixed_math::SIM_ZERO;

use super::locomotor::LocomotorState;
use super::locomotor_ready::LocomotorReadyState;
use super::track_process::TrackFamily;

/// Readiness inputs for one entity, or `None` when this family has no faithful
/// producer yet (the gate then keeps its conservative "not moving" answer).
///
/// Called straight from the Mission readiness gate, once per gate evaluation.
/// `rules` gives Drive/Ship the owner's type and `VeteranSpeed`; without it
/// the speed getter falls back to the order's stamped speed.
pub(crate) fn ready_state_for(
    entity: &GameEntity,
    rules: Option<super::SpeedRules<'_>>,
    binary_frame: u32,
) -> Option<LocomotorReadyState> {
    let locomotor = entity.locomotor.as_ref()?;
    match locomotor.active_kind() {
        LocomotorKind::Drive => Some(drive_family(
            entity,
            rules,
            binary_frame,
            TrackFamily::Drive,
        )),
        LocomotorKind::Ship => Some(drive_family(entity, rules, binary_frame, TrackFamily::Ship)),
        LocomotorKind::Teleport => Some(teleport(entity)),
        LocomotorKind::Jumpjet => Some(jumpjet(locomotor)),
        LocomotorKind::Walk => Some(walk(entity, locomotor)),
        LocomotorKind::Hover => Some(hover(locomotor)),
        // Fly and Rocket have no readiness inputs: the Unit and Infantry
        // readiness branches in `sim::mission::readiness` never see them, and
        // aircraft readiness decides from its mission plus two flags. Their
        // `Is_Moving_Now` bodies answer through `motion_query::is_moving_now`.
        LocomotorKind::Fly | LocomotorKind::Rocket => None,
    }
}

/// Positive sign projection used by Walk's strict >0 query. The native
/// fraction can take other values; only its sign affects this predicate.
const F64_BITS_ONE: u64 = 0x3FF0_0000_0000_0000;

/// Drive and Ship read the same four inputs through separate native slots.
///
/// Native predicate: `timer_remaining || (slot_moving && head_to_nonnull && owner_speed > 0)`,
/// verified term for term including the short-circuit order and the signed
/// `> 0`. Ship's body is byte-identical to Drive's apart from its own null-coord
/// constants.
///
/// The first term is **the owner's body-facing turn timer** — now identified, so
/// `turning_active` is the right name and `body_facing.is_rotating` the right
/// input. The owner field it reads is a facing interpolator holding current and
/// previous facing plus an embedded timer and a turn rate; its setter computes
/// the timer's duration as `|new - previous| / rate` and stamps the start frame,
/// and the predicate's helper answers "does this timer still have frames left".
///
/// What ties it to *turning* specifically rather than to some other countdown:
/// the locomotor's own `Do_Turn` slot writes exactly that field, through exactly
/// that setter. So the term means "the hull is still rotating", which is what we
/// model.
fn drive_family(
    entity: &GameEntity,
    rules: Option<super::SpeedRules<'_>>,
    binary_frame: u32,
    family: TrackFamily,
) -> LocomotorReadyState {
    let turning_active = entity.body_facing.is_rotating(binary_frame);

    let (slot_moving, head_to_nonnull) = super::track_head::motion_state(entity, family);

    // `0x004AFC71` calls the owner's live GetCurrentSpeed (`0x004DB1A0`): the
    // signed speed after two truncations, so a low positive fraction can
    // still read zero. Native reaches the call only after the turn timer,
    // Is_Moving and the head test (`0x004AFC35..0x004AFC6A`); elsewhere the
    // predicate is already decided and the getter is not run.
    let owner_speed = if !turning_active && slot_moving && head_to_nonnull {
        match rules {
            Some(rules) => rules.owner_current_speed(entity),
            None => super::foot_speed::owner_current_speed(
                entity,
                None,
                1.0,
                &crate::sim::houses::Houses::default(),
            ),
        }
    } else {
        0
    };

    match family {
        TrackFamily::Drive => LocomotorReadyState::Drive {
            turning_active,
            slot_moving,
            head_to_nonnull,
            owner_speed,
        },
        TrackFamily::Ship => LocomotorReadyState::Ship {
            turning_active,
            slot_moving,
            head_to_nonnull,
            owner_speed,
        },
    }
}

/// Teleport's readiness input: its `Is_Moving_Now` is the base thunk over
/// `Is_Moving` ([`super::motion_query::is_moving`]). Process `0x00719B0D`
/// can also reach Stop through the owner destination setter.
fn teleport(entity: &GameEntity) -> LocomotorReadyState {
    LocomotorReadyState::Teleport {
        state: u8::from(super::motion_query::is_moving(entity).unwrap_or(false)),
    }
}

/// Jumpjet's readiness input is its flight-state enum; the predicate is
/// `state != 0 && state != 2`.
///
/// The state machine is now decoded from the locomotor's per-frame `Process`
/// switch, which dispatches one handler per state and is the only writer of the
/// field. Native values, with the transitions that identify them:
///
/// | value | meaning | leaves to |
/// |-------|------------------------------------|-----------|
/// | 0 | on the ground, idle | 1 |
/// | 1 | ascending / taking off | 2 or 3 |
/// | 2 | holding station at altitude | 3 or 4 |
/// | 3 | translating at altitude | 2 or 4 |
/// | 4 | descending: target altitude is 0 | 0 |
/// | 5 | touchdown, resolving the target cell | 4 or 6 |
/// | 6 | post-landing finalise | — |
///
/// Two independent confirmations that 4 is the descent and 0 the settled ground
/// state: state 4's handler sets the target altitude to zero and returns to 0
/// only once measured height reaches zero, and the shared altitude integrator
/// takes its target from the ground for states 4 and 0 while applying the hover
/// wobble only for 2 and 3.
///
/// So **a descending jumpjet reports moving** (4 is not excluded). An earlier
/// revision of this function mapped `Descending` to a not-moving value on the
/// reasoning that it was the safe direction; that was a deviation from native,
/// introduced before the enum was decoded, and is reverted here.
///
/// The input is `JumpjetRuntime::phase`, the locomotor's own state field
/// (`+0x50`), which `world::jumpjet_cruise` advances through the native
/// `Process` kernel. It used to be reconstructed from `AirMovePhase`, a lossy
/// mirror that folded 2 and 3 together and had no 5 or 6.
fn jumpjet(locomotor: &LocomotorState) -> LocomotorReadyState {
    LocomotorReadyState::Jumpjet {
        state: locomotor
            .jumpjet_runtime()
            .map_or(0, |runtime| runtime.phase()),
    }
}

/// Walk75AB40 calls IsMoving75AB30, reads Foot+578 >0, then tests the
/// retained step XYZ. The Walk owner retains all three inputs; NavCom is not
/// an authority for this query. The fixed fraction's sign projects to 1/0
/// without a float conversion.
fn walk(entity: &GameEntity, locomotor: &LocomotorState) -> LocomotorReadyState {
    LocomotorReadyState::Walk {
        moving_byte: u8::from(locomotor.walk_is_moving().unwrap_or(false)),
        applied_speed_bits: if entity.foot_speed.applied_fraction() > SIM_ZERO {
            F64_BITS_ONE
        } else {
            0
        },
        destination_nonnull: locomotor
            .step_head()
            .is_some_and(|c| c.x != 0 || c.y != 0 || c.z != 0),
    }
}

/// Hover's readiness inputs ([`super::hover::HoverRuntime::ready_state`]).
fn hover(locomotor: &LocomotorState) -> LocomotorReadyState {
    locomotor.hover_runtime().map_or(
        LocomotorReadyState::Hover {
            slot_moving: false,
            speed_bits: 0,
        },
        super::hover::HoverRuntime::ready_state,
    )
}

#[cfg(test)]
#[path = "ready_producer_tests.rs"]
mod ready_producer_tests;
