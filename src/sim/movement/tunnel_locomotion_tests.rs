//! `TunnelLocomotionClass` state machine: the dig cycle and the three pure
//! answers (`In_Which_Layer`, `Is_To_Have_Shadow`, `Can_Fire`).

use super::*;
use crate::sim::movement::ground_pose::Position;
use crate::util::fixed_math::SimFixed;

/// The flat cell every test but the two raised-cell ones stands on: the
/// `0x578080` query answers zero leptons everywhere.
fn flat_ground(_: DriveCoord) -> Option<i32> {
    Some(0)
}

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
        runtime.process(
            &mut position,
            &mut facing,
            destination,
            speed,
            frame,
            &flat_ground,
        );
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
        runtime.process(
            &mut position,
            &mut facing,
            destination,
            5,
            frame,
            &flat_ground,
        );
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

/// A unit that starts one level up sinks to the same `0xFFFFFF00` plane every
/// tunnel uses (`ProcessDugIn` reads no ground) and comes back to its own
/// cell's ground, which `ProcessPreDigOut 0x7298F0` re-reads every frame.
#[test]
fn a_raised_cell_sinks_to_the_underground_plane_and_surfaces_at_its_own_height() {
    let mut runtime = TunnelRuntime::constructed();
    let mut position = Position::new(
        5,
        5,
        1,
        Some(256),
        SimFixed::from_num(128),
        SimFixed::from_num(128),
    );
    let mut facing = crate::sim::movement::FacingClass::new(0, 0);
    let from = position_world(&position);
    // The whole run stays on this raised cell, so every frame's ground query
    // answers the level the position holds: the premise is unchanged, only the
    // input is explicit now that the runtime no longer samples it once.
    let ground_of = |_: DriveCoord| Some(256);
    let destination = Some(DriveCoord {
        x: from.x,
        y: from.y - 300,
        z: from.z + UNDERGROUND_Z,
    });

    runtime.move_to(false);
    let mut lowest = from.z;
    let mut frame = 0u32;
    for _ in 0..2000 {
        frame += 1;
        runtime.process(
            &mut position,
            &mut facing,
            destination,
            5,
            frame,
            &ground_of,
        );
        let z = position_world(&position).z;
        lowest = lowest.min(z);
        if runtime.state() == TunnelState::Idle {
            break;
        }
    }

    assert_eq!(runtime.state(), TunnelState::Idle);
    assert_eq!(
        lowest, UNDERGROUND_Z,
        "the sink stopped off the shared 0xFFFFFF00 plane"
    );
    assert_eq!(
        position_world(&position).z,
        256,
        "the unit did not return to its own ground height"
    );
}

/// `ProcessPreDigOut 0x7298F0` calls `MapClass::0x578080(&coord)` on every
/// frame (`0x72991D`), so the surfacing target is the cell the object finished
/// its travel over: a tunnel that goes under a ramp and comes up a level higher
/// ends on that cell's ground, not on the height it dug in from.
#[test]
fn surfacing_reads_the_destination_cells_ground() {
    let mut runtime = TunnelRuntime::constructed();
    let mut position = test_position();
    let mut facing = crate::sim::movement::FacingClass::new(0, 0);
    let from = position_world(&position);
    // Cell 5 (the entry) is flat; cell 4, where the destination lies, is one
    // level up.
    let ground_of =
        |coord: DriveCoord| -> Option<i32> { Some(if coord.y >= 5 * 256 { 0 } else { 256 }) };
    let destination = Some(DriveCoord {
        x: from.x,
        y: from.y - 300,
        z: UNDERGROUND_Z,
    });

    runtime.move_to(false);
    let mut lowest = from.z;
    let mut frame = 0u32;
    for _ in 0..2000 {
        frame += 1;
        runtime.process(
            &mut position,
            &mut facing,
            destination,
            5,
            frame,
            &ground_of,
        );
        lowest = lowest.min(position_world(&position).z);
        if runtime.state() == TunnelState::Idle {
            break;
        }
    }

    assert_eq!(runtime.state(), TunnelState::Idle);
    assert_eq!(lowest, UNDERGROUND_Z, "the shared sink plane");
    assert_eq!(
        position_world(&position).z,
        256,
        "the unit surfaced at the entry height instead of its destination cell's"
    );
}

/// The mirror case: a tunnel dug in on a raised cell and surfacing over flat
/// ground ends at zero. The sink runs to the shared `0xFFFFFF00` plane; only the
/// surfacing target follows the destination cell.
#[test]
fn surfacing_over_a_flat_cell_ends_at_ground_level() {
    let mut runtime = TunnelRuntime::constructed();
    let mut position = Position::new(
        5,
        5,
        1,
        Some(256),
        SimFixed::from_num(128),
        SimFixed::from_num(128),
    );
    let mut facing = crate::sim::movement::FacingClass::new(0, 0);
    let from = position_world(&position);
    // Cell 5 (the entry) is raised; cell 4, where the destination lies, is flat.
    let ground_of =
        |coord: DriveCoord| -> Option<i32> { Some(if coord.y >= 5 * 256 { 256 } else { 0 }) };
    let destination = Some(DriveCoord {
        x: from.x,
        y: from.y - 300,
        z: UNDERGROUND_Z,
    });

    runtime.move_to(false);
    let mut frame = 0u32;
    for _ in 0..2000 {
        frame += 1;
        runtime.process(
            &mut position,
            &mut facing,
            destination,
            5,
            frame,
            &ground_of,
        );
        if runtime.state() == TunnelState::Idle {
            break;
        }
    }

    assert_eq!(runtime.state(), TunnelState::Idle);
    assert_eq!(
        position_world(&position).z,
        0,
        "the unit kept the entry cell's height instead of surfacing at ground level"
    );
}

/// `Is_Moving_Now 0x728A60` is `State == 1` only: a sunk or travelling tunnel
/// is not "moving now", so the readiness gate cannot hold a tunnel order on it.
#[test]
fn only_pre_dig_in_answers_is_moving_now() {
    let mut runtime = TunnelRuntime::constructed();
    assert!(!runtime.is_moving_now(), "Idle");
    runtime.move_to(false);
    assert_eq!(runtime.state(), TunnelState::PreDigIn);
    assert!(runtime.is_moving_now(), "PreDigIn is the one true state");
    for state in [
        TunnelState::DiggingIn,
        TunnelState::DugIn,
        TunnelState::Digging,
        TunnelState::PreDigOut,
        TunnelState::DiggingOut,
        TunnelState::DugOut,
    ] {
        runtime.state = state;
        assert!(!runtime.is_moving_now(), "{state:?}");
    }
}

/// `Stop_Moving 0x728C00` state 4 -> `0x728C1E`: a travelling unit surfaces
/// where it stands instead of staying underground.
#[test]
fn stopping_while_underground_surfaces_in_place() {
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
    let mut frame = 0u32;
    while runtime.state() != TunnelState::Digging && frame < 500 {
        frame += 1;
        runtime.process(
            &mut position,
            &mut facing,
            destination,
            5,
            frame,
            &flat_ground,
        );
    }
    assert_eq!(runtime.state(), TunnelState::Digging);
    let sunk = position_world(&position);
    assert_eq!(sunk.z, UNDERGROUND_Z);
    // The first `Digging` frame is the one that entered the state; travel
    // starts on the frame after it.
    for _ in 0..2 {
        frame += 1;
        runtime.process(
            &mut position,
            &mut facing,
            destination,
            5,
            frame,
            &flat_ground,
        );
    }
    let underground = position_world(&position);
    let travelled = underground.y - sunk.y;
    assert!(
        travelled < 0,
        "the unit did not travel toward its destination"
    );

    // state 4 -> 0x728C1E, then `PreDigOut` raises it where it stands.
    runtime.state = TunnelState::PreDigOut;
    for _ in 0..500 {
        frame += 1;
        runtime.process(
            &mut position,
            &mut facing,
            destination,
            5,
            frame,
            &flat_ground,
        );
        if runtime.state() == TunnelState::Idle {
            break;
        }
    }
    let end = position_world(&position);
    assert_eq!(runtime.state(), TunnelState::Idle);
    assert_eq!(end.z, 0, "the abort did not reach the surface");
    assert_eq!(
        (end.x, end.y),
        (underground.x, underground.y),
        "the surfacing moved the object away from where it stopped"
    );
}
#[test]
fn stopping_the_dig_in_timer_settles_as_dug_out() {
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
    // the surface, so a stop here must not have moved Z.
    runtime.process(&mut position, &mut facing, destination, 5, 1, &flat_ground);
    assert_eq!(runtime.state(), TunnelState::DiggingIn);
    assert_eq!(
        position_world(&position).z,
        0,
        "the dig-in left the Z alone"
    );

    // State 2 -> `0x728DD9` sets `DugOut`: nothing ever sank, so there is
    // nothing to raise.
    runtime.stop_moving();
    assert_eq!(runtime.state(), TunnelState::DugOut);
    assert_eq!(position_world(&position).z, 0, "the stop left the Z alone");
}

/// State 3 (`DugIn`) -> `0x728DD0` sets `PreDigOut`, so a unit stopped part
/// way through the sink rises from the Z it reached instead of staying buried.
#[test]
fn stopping_half_sunk_raises_the_unit_from_where_it_is() {
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
    let mut frame = 0u32;
    while runtime.state() != TunnelState::DugIn && frame < 500 {
        frame += 1;
        runtime.process(
            &mut position,
            &mut facing,
            destination,
            5,
            frame,
            &flat_ground,
        );
    }
    assert_eq!(runtime.state(), TunnelState::DugIn);
    // One sink frame leaves the hull between the ground and the plane.
    frame += 1;
    runtime.process(
        &mut position,
        &mut facing,
        destination,
        5,
        frame,
        &flat_ground,
    );
    let sunk = position_world(&position).z;
    assert!(sunk < 0 && sunk > UNDERGROUND_Z, "not half sunk: {sunk}");

    runtime.stop_moving();
    assert_eq!(runtime.state(), TunnelState::PreDigOut);

    for _ in 0..500 {
        frame += 1;
        runtime.process(
            &mut position,
            &mut facing,
            destination,
            5,
            frame,
            &flat_ground,
        );
        if runtime.state() == TunnelState::Idle {
            break;
        }
    }
    assert_eq!(position_world(&position).z, 0, "the stop did not surface");
}
