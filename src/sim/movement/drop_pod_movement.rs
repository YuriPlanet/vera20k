//! DropPod locomotor — independent descent and landing controller.
//!
//! `DropPodLocomotionClass::Process` (`0x004b5b70`) is not Parachute: while
//! airborne it emits a six-frame smoke cadence and a three-frame ground-damage
//! cadence, then atomically either Unlimbos or crushes and destroys at landing.
//! This module reports those effects; the world owner applies animation, damage,
//! occupancy and lifecycle changes in the same master-frame rung.

#[cfg(test)]
use crate::util::fixed_math::SIM_ONE;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

use super::rocket_movement::SpecialMovementOutcome;

const SMOKE_TRAIL_INTERVAL: u32 = 6;
const GROUND_DAMAGE_INTERVAL: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DropPodPhase {
    Descending,
    Landed,
    Destroyed,
}

/// Serializable active DropPod runtime. No ground occupation is owned while
/// `phase == Descending`; a successful landing atomically hands that ownership
/// to the caller's Unlimbo path.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DropPodState {
    pub phase: DropPodPhase,
    pub target_rx: u16,
    pub target_ry: u16,
    pub altitude: SimFixed,
    /// Native-derived fall amount: `max(speed / 10 + 2, minimum_speed)`.
    pub descent_speed: SimFixed,
    pub elapsed_frames: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPodLanding {
    /// `Unlimbo`/`CanEnterCell` accepted the target. Caller must mark exactly
    /// once, open the pod and then complete the special locomotor.
    UnlimboSucceeded,
    /// The target did not admit the unit. Caller must apply crush damage and
    /// destroy the pod in the same transaction; it must never become occupied.
    Blocked,
}

/// Effects requested by one `DropPodLocomotionClass::Process` frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DropPodFrameEffects {
    pub spawn_smoke_trail: bool,
    pub apply_ground_damage: bool,
    pub spawn_ground_debris: bool,
    pub open_pod: bool,
    pub apply_blocked_landing_crush: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropPodProcessResult {
    pub outcome: SpecialMovementOutcome,
    pub effects: DropPodFrameEffects,
}

/// Resolve the terminal DropPod decision through the owner's virtual Unlimbo
/// admission, never through a direct Cell or occupancy predicate.
///
/// The callback corresponds to `ObjectClass::Unlimbo(coords, 0)` and is invoked
/// exactly once only on the frame whose integration reaches the ground.
// Native: DropPodLocomotionClass::Process @ 0x004B5B70, owner vslot +0xD8.
pub fn landing_from_virtual_unlimbo(
    state: &DropPodState,
    mut virtual_unlimbo: impl FnMut((u16, u16), u8) -> bool,
) -> Option<DropPodLanding> {
    if state.phase != DropPodPhase::Descending || state.altitude > state.descent_speed {
        return None;
    }
    Some(if virtual_unlimbo((state.target_rx, state.target_ry), 0) {
        DropPodLanding::UnlimboSucceeded
    } else {
        DropPodLanding::Blocked
    })
}

/// Initialize a distinct DropPod process. It deliberately does not alter an
/// entity's base locomotor or use parachute state.
#[cfg(test)]
pub fn begin_drop_pod_state(
    target: (u16, u16),
    altitude: SimFixed,
    locomotor_speed: SimFixed,
    minimum_speed: SimFixed,
) -> DropPodState {
    let derived_speed = locomotor_speed / SimFixed::from_num(10) + SimFixed::from_num(2);
    DropPodState {
        phase: DropPodPhase::Descending,
        target_rx: target.0,
        target_ry: target.1,
        altitude: altitude.max(SIM_ZERO),
        descent_speed: derived_speed.max(minimum_speed).max(SIM_ONE),
        elapsed_frames: 0,
    }
}

/// Process one DropPod frame.
///
/// `landing` is read only when altitude reaches zero. Passing `None` then is an
/// intentional abort: callers must supply one atomic placement decision rather
/// than split admission from destruction across frames.
pub fn process_drop_pod_state(
    state: &mut DropPodState,
    landing: Option<DropPodLanding>,
) -> DropPodProcessResult {
    match state.phase {
        DropPodPhase::Descending => {
            state.elapsed_frames = state.elapsed_frames.saturating_add(1);
            let effects = DropPodFrameEffects {
                spawn_smoke_trail: state.elapsed_frames.is_multiple_of(SMOKE_TRAIL_INTERVAL),
                apply_ground_damage: state.elapsed_frames.is_multiple_of(GROUND_DAMAGE_INTERVAL),
                spawn_ground_debris: state.elapsed_frames.is_multiple_of(GROUND_DAMAGE_INTERVAL),
                ..DropPodFrameEffects::default()
            };
            state.altitude = (state.altitude - state.descent_speed).max(SIM_ZERO);
            if state.altitude > SIM_ZERO {
                return DropPodProcessResult {
                    outcome: SpecialMovementOutcome::Continue,
                    effects,
                };
            }

            match landing {
                Some(DropPodLanding::UnlimboSucceeded) => {
                    state.phase = DropPodPhase::Landed;
                    DropPodProcessResult {
                        outcome: SpecialMovementOutcome::Complete,
                        effects: DropPodFrameEffects {
                            open_pod: true,
                            ..effects
                        },
                    }
                }
                Some(DropPodLanding::Blocked) => {
                    state.phase = DropPodPhase::Destroyed;
                    DropPodProcessResult {
                        outcome: SpecialMovementOutcome::Abort,
                        effects: DropPodFrameEffects {
                            apply_blocked_landing_crush: true,
                            ..effects
                        },
                    }
                }
                None => {
                    state.phase = DropPodPhase::Destroyed;
                    DropPodProcessResult {
                        outcome: SpecialMovementOutcome::Abort,
                        effects,
                    }
                }
            }
        }
        DropPodPhase::Landed => DropPodProcessResult {
            outcome: SpecialMovementOutcome::Complete,
            effects: DropPodFrameEffects::default(),
        },
        DropPodPhase::Destroyed => DropPodProcessResult {
            outcome: SpecialMovementOutcome::Abort,
            effects: DropPodFrameEffects::default(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_pod_is_not_parachute_and_keeps_airborne_effect_cadences() {
        let mut state = begin_drop_pod_state(
            (10, 12),
            SimFixed::from_num(20),
            SimFixed::from_num(10),
            SimFixed::from_num(1),
        );
        assert_eq!(state.descent_speed, SimFixed::from_num(3));

        let mut smoke = 0;
        let mut damage = 0;
        for _ in 0..6 {
            let result = process_drop_pod_state(&mut state, Some(DropPodLanding::UnlimboSucceeded));
            smoke += u32::from(result.effects.spawn_smoke_trail);
            damage += u32::from(result.effects.apply_ground_damage);
            assert_eq!(result.outcome, SpecialMovementOutcome::Continue);
        }
        assert_eq!(smoke, 1);
        assert_eq!(damage, 2);
    }

    #[test]
    fn landing_unlimbo_and_blocked_crush_are_atomic_and_exclusive() {
        let mut landed = begin_drop_pod_state(
            (1, 1),
            SimFixed::from_num(1),
            SimFixed::from_num(1),
            SimFixed::from_num(1),
        );
        let success = process_drop_pod_state(&mut landed, Some(DropPodLanding::UnlimboSucceeded));
        assert_eq!(success.outcome, SpecialMovementOutcome::Complete);
        assert!(success.effects.open_pod);
        assert!(!success.effects.apply_blocked_landing_crush);

        let mut blocked = begin_drop_pod_state(
            (1, 1),
            SimFixed::from_num(1),
            SimFixed::from_num(1),
            SimFixed::from_num(1),
        );
        let failure = process_drop_pod_state(&mut blocked, Some(DropPodLanding::Blocked));
        assert_eq!(failure.outcome, SpecialMovementOutcome::Abort);
        assert!(failure.effects.apply_blocked_landing_crush);
        assert!(!failure.effects.open_pod);
        assert_eq!(blocked.phase, DropPodPhase::Destroyed);
    }

    #[test]
    fn missing_atomic_placement_decision_aborts_instead_of_occupying() {
        let mut state = begin_drop_pod_state(
            (1, 1),
            SimFixed::from_num(1),
            SimFixed::from_num(1),
            SimFixed::from_num(1),
        );
        assert_eq!(
            process_drop_pod_state(&mut state, None).outcome,
            SpecialMovementOutcome::Abort
        );
        assert_eq!(state.phase, DropPodPhase::Destroyed);
    }

    #[test]
    fn virtual_unlimbo_is_the_only_terminal_admission_call() {
        let mut airborne = begin_drop_pod_state(
            (7, 9),
            SimFixed::from_num(4),
            SimFixed::from_num(10),
            SimFixed::from_num(1),
        );
        let mut calls = Vec::new();
        assert_eq!(
            landing_from_virtual_unlimbo(&airborne, |coords, facing| {
                calls.push((coords, facing));
                true
            }),
            None
        );
        assert!(calls.is_empty());

        airborne.altitude = airborne.descent_speed;
        assert_eq!(
            landing_from_virtual_unlimbo(&airborne, |coords, facing| {
                calls.push((coords, facing));
                false
            }),
            Some(DropPodLanding::Blocked)
        );
        assert_eq!(calls, vec![((7, 9), 0)]);
    }
}
