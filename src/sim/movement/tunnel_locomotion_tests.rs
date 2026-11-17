//! `TunnelLocomotionClass` state machine: the dig cycle and the three pure
//! answers (`In_Which_Layer`, `Is_To_Have_Shadow`, `Can_Fire`).

use super::*;
use crate::sim::movement::ground_pose::Position;
use crate::util::fixed_math::SimFixed;

fn test_position() -> Position {
    Position::new(
        5,
        5,
        0,
        Some(0),
        SimFixed::from_num(128),
        SimFixed::from_num(128),
    )
}

/// One whole dig cycle: the layer is `Underground` in `Digging` and nowhere
/// else, no frame pushes Z past the `-256` floor, and the object returns to
/// `Idle`.
#[test]
fn the_dig_cycle_owns_the_underground_layer_and_the_z_floor() {
    let mut runtime = TunnelRuntime::constructed();
    let mut position = test_position();
    let mut facing = crate::sim::movement::FacingClass::new(0, 0);
    let speed = 5;
    // Due north of the object, so `PreDigIn` finds the hull already aligned
    // (`0x4C93D0` against the atan2 direction `0x4CAE30`).
    let from = position_world(&position);
    let destination = Some(DriveCoord {
        x: from.x,
        y: from.y - 300,
        z: UNDERGROUND_Z,
    });

    runtime.move_to(false);
    assert_eq!(runtime.state(), TunnelState::PreDigIn);

    let mut saw_digging = false;
    let mut saw_sunk = false;
    let mut frame = 0u32;
    for _ in 0..2000 {
        frame += 1;
        runtime.process(&mut position, &mut facing, destination, speed, frame);
        let z = position_world(&position).z;
        assert!(
            z >= UNDERGROUND_Z,
            "Z went past the 0xFFFFFF00 floor: {z} at frame {frame}"
        );
        if runtime.state() == TunnelState::Digging {
            saw_digging = true;
            assert_eq!(
                runtime.layer(),
                crate::sim::movement::locomotor::MovementLayer::Underground,
                "Digging is the only underground state (0x72A1A0)"
            );
        } else {
            assert_eq!(
                runtime.layer(),
                crate::sim::movement::locomotor::MovementLayer::Ground,
                "state {:?} answers Ground",
                runtime.state()
            );
        }
        if z == UNDERGROUND_Z {
            saw_sunk = true;
        }
        if runtime.state() == TunnelState::Idle {
            break;
        }
    }

    assert!(saw_digging, "the class never entered Digging");
    assert!(saw_sunk, "the class never reached the -256 floor");
    assert_eq!(
        runtime.state(),
        TunnelState::Idle,
        "the cycle did not return to Idle"
    );
    let end = position_world(&position);
    assert_eq!(end.z, 0, "the object did not rise back to the surface");
    assert_eq!(
        (end.x, end.y),
        (from.x, from.y - 300),
        "the object did not arrive at its stored destination"
    );
}

/// `Can_Fire 0x72A1C0` refuses every non-idle state, and `Is_To_Have_Shadow
/// 0x72A060` is false exactly while sunk, traveling or rising.
#[test]
fn a_digging_tunnel_neither_fires_nor_casts_a_shadow() {
    let mut runtime = TunnelRuntime::constructed();
    let mut position = test_position();
    let mut facing = crate::sim::movement::FacingClass::new(0, 0);
    let from = position_world(&position);
    let destination = Some(DriveCoord {
        x: from.x,
        y: from.y - 300,
        z: UNDERGROUND_Z,
    });

    runtime.move_to(false);
    assert!(runtime.is_moving(), "Move_To 0x728AF0 arms the class");
    assert!(
        runtime.refuses_fire(),
        "any state but Idle refuses 0x72A1C0"
    );

    let mut shadow_off_states = Vec::new();
    let mut frame = 0u32;
    for _ in 0..2000 {
        frame += 1;
        runtime.process(&mut position, &mut facing, destination, 5, frame);
        if runtime.state() != TunnelState::Idle {
            assert!(
                runtime.refuses_fire(),
                "state {:?} must refuse fire",
                runtime.state()
            );
        }
        if !runtime.has_shadow() {
            shadow_off_states.push(runtime.state());
        }
        if runtime.state() == TunnelState::Idle {
            break;
        }
    }

    assert_eq!(runtime.state(), TunnelState::Idle);
    assert!(!runtime.refuses_fire(), "Idle is the base Can_Fire answer");
    assert!(runtime.has_shadow(), "Idle casts a shadow");
    assert!(
        shadow_off_states.iter().all(|state| matches!(
            state,
            TunnelState::DugIn | TunnelState::Digging | TunnelState::PreDigOut
        )),
        "a shadow was dropped outside states 3/4/5: {shadow_off_states:?}"
    );
    assert!(
        shadow_off_states.contains(&TunnelState::Digging),
        "the shadow must be off while underground"
    );
}

/// `Stop_Moving 0x728C00`'s abort: an armed dig returns to Idle without
/// travelling.
#[test]
fn stopping_an_armed_dig_returns_to_idle() {
    let mut runtime = TunnelRuntime::constructed();
    let mut position = test_position();
    let mut facing = crate::sim::movement::FacingClass::new(0, 0);
    let from = position_world(&position);
    let destination = Some(DriveCoord {
        x: from.x,
        y: from.y - 300,
        z: UNDERGROUND_Z,
    });
    runtime.move_to(false);
    // One frame arms the dig timer and enters DiggingIn; the hull is still on
    // the surface, so an abort here must not have moved Z.
    runtime.process(&mut position, &mut facing, destination, 5, 1);
    assert_eq!(runtime.state(), TunnelState::DiggingIn);
    assert_eq!(
        position_world(&position).z,
        0,
        "the dig-in left the Z alone"
    );
    runtime.state = TunnelState::Idle;
    assert_eq!(position_world(&position).z, 0, "the abort left the Z alone");
    assert!(!runtime.refuses_fire());
}
