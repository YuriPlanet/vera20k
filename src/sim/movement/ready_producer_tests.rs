//! Producer tests for the Mission gate's readiness inputs.
//!
//! These assert the mapping direction, not native parity. The predicate they
//! feed is separately proven exhaustively in `locomotor_ready.rs`.

use super::*;
use crate::sim::components::{
    DriveCoord, DriveLocomotionRuntime, MovementTarget, ShipLocomotionRuntime,
};
use crate::sim::movement::locomotor::GroundMovePhase;
use crate::sim::movement::teleport_movement::TeleportState;
use crate::util::fixed_math::{SIM_ONE, SimFixed};

fn entity_with(kind: LocomotorKind) -> GameEntity {
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    entity
}

fn moving_target(speed: i32) -> MovementTarget {
    MovementTarget {
        path: vec![(5, 5), (6, 5)],
        next_index: 1,
        current_speed: SimFixed::from_num(speed),
        ..MovementTarget::default()
    }
}

/// A parked vehicle must report "not moving". This is the direction that
/// matters: a false "moving" makes the mission gate defer every tick, which
/// stalls the unit outright.
#[test]
fn parked_drive_unit_reports_not_moving() {
    let entity = entity_with(LocomotorKind::Drive);
    let state = ready_state_for(&entity, 100).expect("Drive has a producer");
    assert!(
        !state.is_moving_now(),
        "a parked tank must not report moving"
    );
}

/// A vehicle with a class-owned destination/head and positive owner-applied
/// speed reports moving without a path-execution adapter.
#[test]
fn driving_unit_reports_moving() {
    let mut entity = entity_with(LocomotorKind::Drive);
    let head = DriveCoord {
        x: 6 * 256 + 128,
        y: 5 * 256 + 128,
        z: 0,
    };
    entity.foot_speed.applied_fraction = SIM_ONE;
    entity.foot_speed.cached_current_speed = 25;
    entity.drive_locomotion = Some(DriveLocomotionRuntime {
        destination: Some(head),
        head_to: Some(head),
        ..DriveLocomotionRuntime::default()
    });

    let state = ready_state_for(&entity, 100).expect("Drive has a producer");
    assert!(state.is_moving_now(), "a driving tank must report moving");
}

/// Standing exactly on the stale head-to point reads not-moving. This is native
/// behaviour, not a workaround: the native slot compares head-to against the
/// owner's coordinate and ignores Z.
#[test]
fn unit_parked_on_its_head_to_reports_not_moving() {
    let mut entity = entity_with(LocomotorKind::Drive);
    entity.drive_locomotion = Some(DriveLocomotionRuntime {
        head_to: Some(DriveCoord {
            x: 5 * 256 + i32::from(entity.position.sub_x.to_num::<i32>() as i16),
            y: 5 * 256 + i32::from(entity.position.sub_y.to_num::<i32>() as i16),
            z: 0,
        }),
        ..DriveLocomotionRuntime::default()
    });

    let state = ready_state_for(&entity, 100).expect("Drive has a producer");
    assert!(!state.is_moving_now());
}

/// Ship uses the same four inputs through its own native slot, so it must
/// behave identically to Drive on identical state — but keep its own variant.
#[test]
fn ship_mirrors_drive_but_keeps_its_own_variant() {
    let mut entity = entity_with(LocomotorKind::Ship);
    let head = DriveCoord::cell(6, 5, 0);
    entity.foot_speed.applied_fraction = SIM_ONE;
    entity.foot_speed.cached_current_speed = 20;
    entity.ship_locomotion = Some(ShipLocomotionRuntime {
        destination: Some(head),
        head_to: Some(head),
        ..Default::default()
    });
    let state = ready_state_for(&entity, 100).expect("Ship has a producer");
    assert!(matches!(state, LocomotorReadyState::Ship { .. }));
    assert!(state.is_moving_now());
}

/// The trap this producer exists to avoid: a warped unit sitting out its chrono
/// delay is NOT moving. Treating the whole teleport state as "moving" would
/// defer its missions for the entire delay.
#[test]
fn teleport_chrono_delay_reports_not_moving() {
    let mut entity = entity_with(LocomotorKind::Teleport);
    entity.teleport_state = Some(TeleportState {
        phase: TeleportPhase::ChronoDelay,
        target_rx: 20,
        target_ry: 20,
        being_warped_ticks: 10,
    });

    let state = ready_state_for(&entity, 100).expect("Teleport has a producer");
    assert!(
        !state.is_moving_now(),
        "chrono delay must not defer the unit's missions"
    );
}

/// The relocation tick itself is the one moment the native flag is set.
#[test]
fn teleport_relocate_reports_moving() {
    let mut entity = entity_with(LocomotorKind::Teleport);
    entity.teleport_state = Some(TeleportState {
        phase: TeleportPhase::Relocate,
        target_rx: 20,
        target_ry: 20,
        being_warped_ticks: 16,
    });

    let state = ready_state_for(&entity, 100).expect("Teleport has a producer");
    assert!(state.is_moving_now());
}

/// The readiness input is the locomotor's own state field, all seven native
/// values: `state != 0 && state != 2`. It used to be rebuilt from
/// `AirMovePhase`, which had no 5 or 6 and told 2 from 3 by whether a movement
/// target existed.
#[test]
fn jumpjet_readiness_reads_the_native_state_field() {
    let mut entity = entity_with(LocomotorKind::Jumpjet);
    for (state, moving) in [
        (0, false),
        (1, true),
        (2, false),
        (3, true),
        (4, true),
        (5, true),
        (6, true),
    ] {
        entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.jumpjet_runtime_mut())
            .expect("a Jumpjet locomotor carries its runtime")
            .phase = state;
        let ready = ready_state_for(&entity, 100).expect("Jumpjet has a producer");
        assert_eq!(ready.is_moving_now(), moving, "native state {state}");
    }

    // A pending order does not make a holding Jumpjet "moving": state 2 is
    // excluded whatever the Foot destination says.
    entity
        .locomotor
        .as_mut()
        .and_then(|locomotor| locomotor.jumpjet_runtime_mut())
        .unwrap()
        .phase = 2;
    entity.movement_target = Some(moving_target(20));
    assert!(!ready_state_for(&entity, 100).unwrap().is_moving_now());
}

/// A standing infantryman is not moving.
#[test]
fn idle_walker_reports_not_moving() {
    let entity = entity_with(LocomotorKind::Walk);
    let state = ready_state_for(&entity, 100).expect("Walk has a producer");
    assert!(!state.is_moving_now());
}

/// A walker stepping toward the next cell is moving.
#[test]
fn walking_infantry_reports_moving() {
    let mut entity = entity_with(LocomotorKind::Walk);
    let head = DriveCoord::cell(6, 5, 0);
    let loco = entity.locomotor.as_mut().unwrap();
    loco.set_walk_destination(Some(head));
    loco.set_step_head(Some(head));
    entity.foot_speed.applied_fraction = SIM_ONE;
    let state = ready_state_for(&entity, 100).expect("Walk has a producer");
    assert!(state.is_moving_now());
}

/// An outstanding order without an admitted head cannot defer commencement.
#[test]
fn blocked_walker_reports_not_moving() {
    let mut entity = entity_with(LocomotorKind::Walk);
    entity.movement_target = Some(moving_target(10));
    if let Some(locomotor) = entity.locomotor.as_mut() {
        locomotor.phase = GroundMovePhase::Blocked;
    }
    let state = ready_state_for(&entity, 100).expect("Walk has a producer");
    assert!(
        !state.is_moving_now(),
        "a blocked walker must not defer its mission indefinitely"
    );
}

#[test]
fn walk_stop_keeps_paid_head_readiness_until_retirement_and_restore() {
    let mut entity = entity_with(LocomotorKind::Walk);
    let head = DriveCoord::cell(6, 5, 0);
    let loco = entity.locomotor.as_mut().unwrap();
    loco.set_walk_destination(Some(head));
    loco.set_step_head(Some(head));
    loco.set_walk_destination(None);
    loco.phase = GroundMovePhase::Blocked;
    entity.foot_speed.applied_fraction = SIM_ONE;
    assert!(entity.movement_target.is_none());
    assert!(is_moving_now_for(&entity, 100));
    let mut restored: GameEntity =
        serde_json::from_value(serde_json::to_value(&entity).unwrap()).unwrap();
    assert!(is_moving_now_for(&restored, 100));
    restored.locomotor.as_mut().unwrap().set_step_head(None);
    assert!(!is_moving_now_for(&restored, 100));
    assert_eq!(
        restored.locomotor.as_ref().unwrap().walk_is_moving(),
        Some(true)
    );
}

#[test]
fn retained_motion_and_walk_readiness_match_original_queries() {
    use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/locomotor_moving.json"
    ))
    .unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 84);
    let coord = |v: &serde_json::Value| DriveCoord {
        x: v[0].as_i64().unwrap() as i32,
        y: v[1].as_i64().unwrap() as i32,
        z: v[2].as_i64().unwrap() as i32,
    };
    let optional = |c: DriveCoord| (c.x != 0 || c.y != 0 || c.z != 0).then_some(c);
    for row in rows.as_array().unwrap() {
        let input = &row["input"];
        let kind = match input["family"].as_str().unwrap() {
            "drive" => LocomotorKind::Drive,
            "ship" => LocomotorKind::Ship,
            "walk" => LocomotorKind::Walk,
            _ => unreachable!(),
        };
        let mut entity = entity_with(kind);
        let current = coord(&input["current"]);
        entity.position.rx = (current.x / 256) as u16;
        entity.position.ry = (current.y / 256) as u16;
        entity.position.sub_x = SimFixed::from_num(current.x % 256);
        entity.position.sub_y = SimFixed::from_num(current.y % 256);
        let head = optional(coord(&input["head"]));
        match kind {
            LocomotorKind::Drive => {
                entity.drive_locomotion = Some(DriveLocomotionRuntime {
                    destination: optional(coord(&input["destination"])),
                    head_to: head,
                    ..Default::default()
                })
            }
            LocomotorKind::Ship => {
                entity.ship_locomotion = Some(ShipLocomotionRuntime {
                    destination: optional(coord(&input["destination"])),
                    head_to: head,
                    ..Default::default()
                })
            }
            LocomotorKind::Walk => {
                let LocomotorRuntimePayload::Walk(state) =
                    &mut entity.locomotor.as_mut().unwrap().runtime_payload
                else {
                    unreachable!()
                };
                state.head = head;
                state.moving = input["moving"].as_bool().unwrap();
                entity.foot_speed.applied_fraction =
                    SimFixed::from_num(input["speed"].as_f64().unwrap());
            }
            _ => unreachable!(),
        }
        // The compatibility path/order must not affect any represented query.
        for has_order in [false, true] {
            entity.movement_target = has_order.then(|| moving_target(20));
            entity.navigation.nav_com =
                has_order.then(|| crate::sim::components::NavTargetRef::cell(6, 5));
            let moving = if kind == LocomotorKind::Walk {
                assert_eq!(
                    is_moving_now_for(&entity, 100),
                    row["moving_now"].as_bool().unwrap(),
                    "{row}"
                );
                entity.locomotor.as_ref().unwrap().walk_is_moving().unwrap()
            } else {
                is_moving_for_unit_shp_draw(&entity)
            };
            assert_eq!(moving, row["moving"].as_bool().unwrap(), "{row}");
        }
    }
}

/// A hover unit with no movement work is not moving, and a stale speed request
/// cannot resurrect it.
#[test]
fn stopped_hover_reports_not_moving_despite_stale_request() {
    let mut entity = entity_with(LocomotorKind::Hover);
    if let Some(locomotor) = entity.locomotor.as_mut() {
        // Left over from the last leg — the producer must not trust it.
        locomotor.hover_speed_request = SIM_ONE;
    }
    let state = ready_state_for(&entity, 100).expect("Hover has a producer");
    assert!(!state.is_moving_now());
}

/// A hover unit under way with a non-zero throttle request is moving.
#[test]
fn hover_under_way_reports_moving() {
    let mut entity = entity_with(LocomotorKind::Hover);
    entity.movement_target = Some(moving_target(15));
    if let Some(locomotor) = entity.locomotor.as_mut() {
        locomotor.hover_speed_request = SIM_ONE;
    }
    let state = ready_state_for(&entity, 100).expect("Hover has a producer");
    assert!(state.is_moving_now());
}

/// A zero throttle request — the turn-stall case — reads not moving even with a
/// live movement target, because the native predicate's speed term is strict.
#[test]
fn hover_turn_stall_reports_not_moving() {
    let mut entity = entity_with(LocomotorKind::Hover);
    entity.movement_target = Some(moving_target(15));
    if let Some(locomotor) = entity.locomotor.as_mut() {
        locomotor.hover_speed_request = SIM_ZERO;
    }
    let state = ready_state_for(&entity, 100).expect("Hover has a producer");
    assert!(!state.is_moving_now());
}

/// The three reachable throttle requests map onto the native double's bits.
#[test]
fn hover_request_maps_to_native_double_bits() {
    assert_eq!(hover_request_bits(SIM_ZERO), 0);
    assert_eq!(hover_request_bits(SimFixed::lit("0.5")), F64_BITS_HALF);
    assert_eq!(hover_request_bits(SIM_ONE), F64_BITS_ONE);
}

/// Families with no readiness slot this gate consults must yield `None`, which
/// leaves the mission gate on its conservative answer rather than a guess.
#[test]
fn unmapped_families_yield_no_producer() {
    for kind in [LocomotorKind::Fly, LocomotorKind::Rocket] {
        let entity = entity_with(kind);
        assert!(
            ready_state_for(&entity, 100).is_none(),
            "{kind:?} has no faithful producer and must not be guessed"
        );
    }
}

/// An entity with no locomotor at all yields nothing.
#[test]
fn entity_without_locomotor_yields_no_producer() {
    let entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    assert!(ready_state_for(&entity, 100).is_none());
}
