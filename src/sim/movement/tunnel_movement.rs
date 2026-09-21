//! Tunnel locomotor Process adapter.
//!
//! `TunnelLocomotionClass::Process @ 0x00728e30` owns seven non-idle states.
//! The live YR rules do not construct a tunnel locomotor, so this module keeps
//! the recovered state machine independent from `GameEntity` until the root
//! movement rung supplies its owner, occupation, and abort-motion adapters.

use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::teleport_movement::SpecialMovementOutcome;

/// Native TunnelLocomotionClass state byte at locomotor `+20`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum TunnelPhase {
    Idle = 0,
    PreDigIn = 1,
    Burrow = 2,
    UndergroundTravel = 3,
    Digging = 4,
    PreDigOut = 5,
    SurfaceMark = 6,
    AbortMotion = 7,
}

impl Default for TunnelPhase {
    fn default() -> Self {
        Self::Idle
    }
}

/// Locomotor-local tunnel state. Destination path ownership remains on Foot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct TunnelState {
    pub phase: TunnelPhase,
}

/// Owner and map mutations supplied by the world integration layer.
///
/// The adapter deliberately separates surface and underground occupancy so a
/// caller cannot accidentally leave the unit on both layers during a phase
/// transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TunnelProcessContext {
    pub destination_reached: bool,
    pub surface_cell_available: bool,
    pub layer: MovementLayer,
    pub z: i32,
    pub surface_occupied: bool,
    pub underground_occupied: bool,
    pub abort_motion_called: bool,
}

impl Default for TunnelProcessContext {
    fn default() -> Self {
        Self {
            destination_reached: false,
            surface_cell_available: true,
            layer: MovementLayer::Ground,
            z: 0,
            surface_occupied: true,
            underground_occupied: false,
            abort_motion_called: false,
        }
    }
}

/// Advance one native tunnel Process phase.
///
/// State 2 is the only entry to the underground layer and writes Z=-256 before
/// travel. State 6 restores the surface occupation before state 7 invokes the
/// Foot abort-motion hook. An unavailable exit retries from PreDigOut; a caller
/// may explicitly call [`abort_tunnel_motion`] to choose the native cleanup path.
pub fn process_tunnel(
    state: &mut TunnelState,
    context: &mut TunnelProcessContext,
) -> SpecialMovementOutcome {
    match state.phase {
        TunnelPhase::Idle => SpecialMovementOutcome::Complete,
        TunnelPhase::PreDigIn => {
            state.phase = TunnelPhase::Burrow;
            SpecialMovementOutcome::Continue
        }
        TunnelPhase::Burrow => {
            context.surface_occupied = false;
            context.layer = MovementLayer::Underground;
            context.z = -256;
            context.underground_occupied = true;
            state.phase = TunnelPhase::UndergroundTravel;
            SpecialMovementOutcome::Continue
        }
        TunnelPhase::UndergroundTravel => {
            if context.destination_reached {
                state.phase = TunnelPhase::Digging;
            }
            SpecialMovementOutcome::Continue
        }
        TunnelPhase::Digging => {
            state.phase = TunnelPhase::PreDigOut;
            SpecialMovementOutcome::Continue
        }
        TunnelPhase::PreDigOut => {
            if context.surface_cell_available {
                state.phase = TunnelPhase::SurfaceMark;
            }
            SpecialMovementOutcome::Continue
        }
        TunnelPhase::SurfaceMark => {
            context.underground_occupied = false;
            context.layer = MovementLayer::Ground;
            context.z = 0;
            context.surface_occupied = true;
            state.phase = TunnelPhase::AbortMotion;
            SpecialMovementOutcome::Continue
        }
        TunnelPhase::AbortMotion => {
            context.abort_motion_called = true;
            state.phase = TunnelPhase::Idle;
            SpecialMovementOutcome::Complete
        }
    }
}

/// Route an interrupted tunnel move through its proven state-7 cleanup.
#[cfg(test)]
pub fn abort_tunnel_motion(state: &mut TunnelState) -> SpecialMovementOutcome {
    if state.phase == TunnelPhase::Idle {
        return SpecialMovementOutcome::Abort;
    }
    state.phase = TunnelPhase::AbortMotion;
    SpecialMovementOutcome::Continue
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_process_preserves_the_native_layer_and_occupation_order() {
        let mut state = TunnelState {
            phase: TunnelPhase::PreDigIn,
        };
        let mut context = TunnelProcessContext::default();

        assert_eq!(
            process_tunnel(&mut state, &mut context),
            SpecialMovementOutcome::Continue
        );
        assert_eq!(state.phase, TunnelPhase::Burrow);

        assert_eq!(
            process_tunnel(&mut state, &mut context),
            SpecialMovementOutcome::Continue
        );
        assert_eq!(state.phase, TunnelPhase::UndergroundTravel);
        assert_eq!(context.layer, MovementLayer::Underground);
        assert_eq!(context.z, -256);
        assert!(!context.surface_occupied);
        assert!(context.underground_occupied);

        context.destination_reached = true;
        assert_eq!(
            process_tunnel(&mut state, &mut context),
            SpecialMovementOutcome::Continue
        );
        assert_eq!(state.phase, TunnelPhase::Digging);
        assert_eq!(
            process_tunnel(&mut state, &mut context),
            SpecialMovementOutcome::Continue
        );
        assert_eq!(state.phase, TunnelPhase::PreDigOut);
        assert_eq!(
            process_tunnel(&mut state, &mut context),
            SpecialMovementOutcome::Continue
        );
        assert_eq!(state.phase, TunnelPhase::SurfaceMark);

        assert_eq!(
            process_tunnel(&mut state, &mut context),
            SpecialMovementOutcome::Continue
        );
        assert_eq!(state.phase, TunnelPhase::AbortMotion);
        assert_eq!(context.layer, MovementLayer::Ground);
        assert_eq!(context.z, 0);
        assert!(context.surface_occupied);
        assert!(!context.underground_occupied);

        assert_eq!(
            process_tunnel(&mut state, &mut context),
            SpecialMovementOutcome::Complete
        );
        assert_eq!(state.phase, TunnelPhase::Idle);
        assert!(context.abort_motion_called);
    }

    #[test]
    fn unavailable_surface_cell_holds_the_predigout_phase() {
        let mut state = TunnelState {
            phase: TunnelPhase::PreDigOut,
        };
        let mut context = TunnelProcessContext {
            surface_cell_available: false,
            layer: MovementLayer::Underground,
            z: -256,
            surface_occupied: false,
            underground_occupied: true,
            ..TunnelProcessContext::default()
        };

        assert_eq!(
            process_tunnel(&mut state, &mut context),
            SpecialMovementOutcome::Continue
        );
        assert_eq!(state.phase, TunnelPhase::PreDigOut);
        assert_eq!(context.layer, MovementLayer::Underground);
        assert!(context.underground_occupied);
    }

    #[test]
    fn interrupted_tunnel_uses_state_seven_cleanup() {
        let mut state = TunnelState {
            phase: TunnelPhase::UndergroundTravel,
        };
        let mut context = TunnelProcessContext {
            layer: MovementLayer::Underground,
            z: -256,
            surface_occupied: false,
            underground_occupied: true,
            ..TunnelProcessContext::default()
        };

        assert_eq!(
            abort_tunnel_motion(&mut state),
            SpecialMovementOutcome::Continue
        );
        assert_eq!(state.phase, TunnelPhase::AbortMotion);
        assert_eq!(
            process_tunnel(&mut state, &mut context),
            SpecialMovementOutcome::Complete
        );
        assert!(context.abort_motion_called);
    }
}
