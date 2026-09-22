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
//! `locomotor_ready` is destined for `sim::substrate::locomotion`, whose
//! dependency floor is rules/util only. Producing the inputs requires reading
//! `GameEntity`, so it stays here in `sim::movement`.
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
//! Drive/Ship share their coordinate query with cell-entry and tube consumers.
//! Walk reads its retained byte/head and the shared Foot speed fraction.
//! Original query comparisons are in locomotor_moving.json; other family
//! adapters and the cached Drive/Ship owner speed still have recorded limits.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/ movement and entity state only.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

use super::locomotor::LocomotorState;
use super::locomotor_ready::LocomotorReadyState;
use super::teleport_movement::TeleportPhase;
use super::track_process::TrackFamily;

/// Readiness inputs for one entity, or `None` when this family has no faithful
/// producer yet (the gate then keeps its conservative "not moving" answer).
///
/// Called straight from the Mission readiness gate, once per gate evaluation.
pub(crate) fn ready_state_for(
    entity: &GameEntity,
    binary_frame: u32,
) -> Option<LocomotorReadyState> {
    let locomotor = entity.locomotor.as_ref()?;
    match locomotor.active_kind() {
        LocomotorKind::Drive => Some(drive_family(entity, binary_frame, TrackFamily::Drive)),
        LocomotorKind::Ship => Some(drive_family(entity, binary_frame, TrackFamily::Ship)),
        LocomotorKind::Teleport => Some(teleport(entity)),
        LocomotorKind::Jumpjet => Some(jumpjet(locomotor)),
        LocomotorKind::Walk => Some(walk(entity, locomotor)),
        LocomotorKind::Hover => Some(hover(entity, locomotor)),
        // Catches six kinds: Fly, Rocket, Parachute, Tunnel, DropPod and Mech.
        // None needs a producer, because nothing consumes one for them: our two
        // consumers of `is_moving_now` are the Unit and Infantry readiness
        // branches in `sim::mission::readiness`, aircraft readiness decides from
        // its mission plus two flags and never reads the locomotor, and
        // Rocket-locomotor objects are aircraft too, not vehicles or infantry.
        //
        // Three things worth knowing before anyone "completes" this arm:
        //
        // - It is unreachable for the *readiness gate*, but the native slot
        //   itself is not dead. gamemd reads it every tick on every foot object
        //   for the sight/occupancy refresh and the move-sound state, and one
        //   aircraft weapon predicate is literally its negation. So the slot has
        //   consumers; the readiness answer just is not one of them.
        // - These kinds do not agree on what the slot even is. Fly, Rocket and
        //   Mech each override it with a real body — Mech's is Drive-shaped.
        //   DropPod inherits the base thunk, which for an unspecialised
        //   locomotor resolves to a constant false. Tunnel and Parachute have no
        //   such slot at all; Parachute has no native locomotor class whatsoever.
        // - Mech and DropPod are dormant TS in stock YR, and Tunnel is not the
        //   low-bridge tube movement that *is* live.
        _ => None,
    }
}

/// Fresh post-Process moving-now answer for FootClass side effects such as
/// MoveSound. Native dispatches this locomotor slot at each consumer.
pub(crate) fn is_moving_now_for(entity: &GameEntity, binary_frame: u32) -> bool {
    ready_state_for(entity, binary_frame).is_some_and(LocomotorReadyState::is_moving_now)
}

/// UnitClass draw-time `ILocomotion::Is_Moving` answer for the two active-stock
/// SHP vehicle families. This is deliberately separate from
/// [`is_moving_now_for`]: drawing does not fold in hull rotation or applied
/// owner speed.
pub(crate) fn is_moving_for_unit_shp_draw(entity: &GameEntity) -> bool {
    let Some(locomotor) = entity.locomotor.as_ref() else {
        return false;
    };
    match locomotor.active_kind() {
        LocomotorKind::Drive => super::track_head::motion_state(entity, TrackFamily::Drive).0,
        LocomotorKind::Ship => super::track_head::motion_state(entity, TrackFamily::Ship).0,
        _ => false,
    }
}

/// Positive sign projection used by Walk's strict >0 query. The native
/// fraction can take other values; only its sign affects this predicate.
const F64_BITS_ONE: u64 = 0x3FF0_0000_0000_0000;
/// The hover throttle request's third reachable value.
const F64_BITS_HALF: u64 = 0x3FE0_0000_0000_0000;

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
    binary_frame: u32,
    family: TrackFamily,
) -> LocomotorReadyState {
    let turning_active = entity
        .body_facing
        .as_ref()
        .is_some_and(|facing| facing.is_rotating(binary_frame));

    let (slot_moving, head_to_nonnull) = super::track_head::motion_state(entity, family);

    // Existing adapter caches the signed speed after two truncations, retaining
    // the low-fraction DLPH/SQD frame where a positive fraction yields zero.
    // OPEN host integration: native4AFC71 invokes the live getter here; a
    // callback's speed/modifier changes must be observed without a stale cache.
    let owner_speed = entity.foot_speed.cached_current_speed;

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

/// Teleport's readiness input is a private one-shot flag, not the warp phase
/// counter.
///
/// It is true only for the relocation tick itself. It is NOT true during the
/// post-warp chrono delay — treating the whole teleport state as "moving" would
/// defer a warped unit's missions for the entire delay.
fn teleport(entity: &GameEntity) -> LocomotorReadyState {
    LocomotorReadyState::Teleport {
        state: u8::from(matches!(
            entity.teleport_state.as_ref().map(|state| state.phase),
            Some(TeleportPhase::Relocate)
        )),
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
/// mirror that folded 2 and 3 together (told apart by whether a movement target
/// existed) and had no 5 or 6.
fn jumpjet(locomotor: &LocomotorState) -> LocomotorReadyState {
    LocomotorReadyState::Jumpjet {
        state: locomotor
            .jumpjet_runtime()
            .map_or(0, |runtime| runtime.phase),
    }
}

/// Walk75AB40 calls IsMoving75AB30, reads Foot+578 >0, then tests the
/// retained step XYZ. The Walk owner now retains all three inputs; NavCom,
/// MovementTarget and GroundMovePhase are not authorities for this query.
/// Project the fixed fraction's sign to 1/0 without a float conversion.
fn walk(entity: &GameEntity, locomotor: &LocomotorState) -> LocomotorReadyState {
    LocomotorReadyState::Walk {
        moving_byte: u8::from(locomotor.walk_is_moving().unwrap_or(false)),
        applied_speed_bits: if entity.foot_speed.applied_fraction > SIM_ZERO {
            F64_BITS_ONE
        } else {
            0
        },
        destination_nonnull: locomotor
            .step_head()
            .is_some_and(|c| c.x != 0 || c.y != 0 || c.z != 0),
    }
}

/// Hover's readiness inputs.
///
/// Native predicate: `slot_moving && speed != 0`, where the speed is a double on
/// the locomotor itself and the test really is `!= 0` — a *negative* speed counts
/// as moving. That makes the speed term weaker than a `> 0` test would be, so it
/// does **not** compensate for an over-inclusive `slot_moving`; an earlier
/// revision of this comment claimed it did.
///
/// Our `slot_moving` is a loose analogue of native's two-coord test, built from
/// the two nearest carriers we have. It is UNCHECKED, and over-inclusive.
///
/// The speed input is the **unramped request**, not `hover_throttle`. The ramp
/// lags the request by up to roughly 27 ticks on the brake side, so reading it
/// would keep reporting "moving" well after a hover unit stopped — the stall
/// direction.
fn hover(entity: &GameEntity, locomotor: &LocomotorState) -> LocomotorReadyState {
    LocomotorReadyState::Hover {
        slot_moving: entity.movement_target.is_some() || entity.navigation.nav_com.is_some(),
        // Forced to zero without a live movement target: the request is only
        // written inside the hover movement branch, so a stopped unit would
        // otherwise keep its last non-zero value indefinitely.
        speed_bits: if entity.movement_target.is_some() {
            hover_request_bits(locomotor.hover_speed_request)
        } else {
            0
        },
    }
}

/// The hover throttle request has exactly three reachable values, so it maps to
/// the native double's bits by table — no float arithmetic in `sim/`.
fn hover_request_bits(request: SimFixed) -> u64 {
    if request == SIM_ZERO {
        0
    } else if request == SimFixed::lit("0.5") {
        F64_BITS_HALF
    } else {
        F64_BITS_ONE
    }
}

#[cfg(test)]
#[path = "ready_producer_tests.rs"]
mod ready_producer_tests;
