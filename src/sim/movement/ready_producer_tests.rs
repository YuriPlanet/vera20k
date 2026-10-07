//! Producer tests for the Mission gate's readiness inputs.
//!
//! These assert the mapping direction, not native parity. The predicate they
//! feed is separately proven exhaustively in `locomotor_ready.rs`.

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::{DriveCoord, MovementTarget};
use crate::sim::movement::SpeedRules;
use crate::sim::movement::motion_query::is_moving_now;
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::sim::type_handle_table::TypeHandleTable;
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed};
use crate::util::native_x87::NativeF64Bits;

fn entity_with(kind: LocomotorKind) -> GameEntity {
    let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    entity
}

fn mtnk_rules() -> RuleSet {
    RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nSpeed=6\n",
    ))
    .expect("minimal MTNK rules")
}

/// An order toward (6,5). Its route is the Foot+5E0 queue
/// (`order_route`); the adapter keeps only the goal.
fn moving_target() -> MovementTarget {
    MovementTarget {
        final_goal: Some((6, 5)),
        ..MovementTarget::default()
    }
}

/// The Foot+5E0 words Find_Path installs for that order from (5,5).
fn order_route() -> crate::sim::components::FootPathQueue {
    crate::sim::movement::fixture_path_replay(&[(5, 5), (6, 5)])
}

fn driving_mtnk() -> GameEntity {
    let mut entity = entity_with(LocomotorKind::Drive);
    let head = DriveCoord {
        x: 6 * 256 + 128,
        y: 5 * 256 + 128,
        z: 0,
    };
    entity.foot_speed.set_speed_fraction(SIM_ONE);
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                DriveLocomotionRuntime::default()
                    .with_destination_for_test(Some(head))
                    .with_head_to_for_test(Some(head))
            ))
    );
    entity
}

/// A parked vehicle must report "not moving". This is the direction that
/// matters: a false "moving" makes the mission gate defer every tick, which
/// stalls the unit outright.
#[test]
fn parked_drive_unit_reports_not_moving() {
    let entity = entity_with(LocomotorKind::Drive);
    let state = ready_state_for(&entity, None, 100).expect("Drive has a producer");
    assert!(
        !state.is_moving_now(),
        "a parked tank must not report moving"
    );
}

/// A vehicle with a class-owned destination/head and positive owner-applied
/// speed reports moving without a path-execution adapter.
#[test]
fn driving_unit_reports_moving() {
    let mut entity = driving_mtnk();
    let rules = mtnk_rules();
    let interner = crate::sim::intern::test_interner();
    let types = TypeHandleTable::default();
    let houses = std::collections::BTreeMap::new();
    let speed = Some(SpeedRules::new(&rules, &interner, &types, &houses));

    let state = ready_state_for(&entity, speed, 100).expect("Drive has a producer");
    assert!(state.is_moving_now(), "a driving tank must report moving");

    // `0x004AFC71` reads the live getter: a callback that zeroes Foot+578
    // (a stop) takes effect at the next query, with no speed cache left to
    // refresh.
    entity.foot_speed.set_speed_fraction(SIM_ZERO);
    assert!(!is_moving_now(&entity, speed, 100));
}

/// A speed crate multiplies the type speed inside GetCurrentSpeed
/// (`0x004DB1A0`), so the pickup reaches the next moving query without a
/// cache refresh. At three quarters of a lepton per frame the getter truncates
/// to 0; the doubled type speed reads one and a half.
#[test]
fn speed_crate_reaches_the_next_moving_query() {
    let mut entity = driving_mtnk();
    let rules = mtnk_rules();
    let interner = crate::sim::intern::test_interner();
    let types = TypeHandleTable::default();
    let houses = std::collections::BTreeMap::new();
    let speed = SpeedRules::new(&rules, &interner, &types, &houses);

    let full = speed.owner_current_speed(&entity);
    assert!(full > 1, "MTNK covers several leptons per frame");
    entity
        .foot_speed
        .set_speed_fraction(SimFixed::lit("0.75") / SimFixed::from_num(full));
    assert!(!is_moving_now(&entity, Some(speed), 100));

    assert!(
        entity
            .foot_speed
            .accept_speed_crate(NativeF64Bits::from_bits(2.0f64.to_bits()))
    );
    assert!(is_moving_now(&entity, Some(speed), 100));
}

/// Standing exactly on the stale head-to point reads not-moving. This is native
/// behaviour, not a workaround: the native slot compares head-to against the
/// owner's coordinate and ignores Z.
#[test]
fn unit_parked_on_its_head_to_reports_not_moving() {
    let mut entity = entity_with(LocomotorKind::Drive);
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                DriveLocomotionRuntime::default().with_head_to_for_test(Some(DriveCoord {
                    x: 5 * 256 + i32::from(entity.position.sub_x.to_num::<i32>() as i16),
                    y: 5 * 256 + i32::from(entity.position.sub_y.to_num::<i32>() as i16),
                    z: 0,
                }))
            ))
    );

    let state = ready_state_for(&entity, None, 100).expect("Drive has a producer");
    assert!(!state.is_moving_now());
}

/// Ship uses the same four inputs through its own native slot, so it must
/// behave identically to Drive on identical state — but keep its own variant.
#[test]
fn ship_mirrors_drive_but_keeps_its_own_variant() {
    let mut entity = entity_with(LocomotorKind::Ship);
    let head = DriveCoord::cell(6, 5, 0);
    entity.foot_speed.set_speed_fraction(SIM_ONE);
    // Without rules the getter reads the order's stamped speed.
    entity.movement_target = Some(MovementTarget {
        speed: SimFixed::from_num(300),
        ..moving_target()
    });
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_ship_state_for_test(Some(
                ShipLocomotionRuntime::default()
                    .with_destination_for_test(Some(head))
                    .with_head_to_for_test(Some(head))
            ))
    );
    let state = ready_state_for(&entity, None, 100).expect("Ship has a producer");
    assert!(matches!(state, LocomotorReadyState::Ship { .. }));
    assert!(state.is_moving_now());
}

/// Existing Process-adapter regression; native Cell setter/Stop request-byte
/// controls live in teleport_cell_destination_tests, rather than this fixture.
#[test]
fn teleport_chrono_delay_reports_not_moving() {
    let mut entity = entity_with(LocomotorKind::Teleport);
    entity.install_teleport_state_for_test(Some(TeleportState::for_test(
        TeleportPhase::ChronoDelay,
        20,
        20,
        10,
    )));

    let state = ready_state_for(&entity, None, 100).expect("Teleport has a producer");
    assert!(
        !state.is_moving_now(),
        "chrono delay must not defer the unit's missions"
    );
}

/// Supplied-state query regression; native request publication is compared
/// separately through the actual Cell setter in teleport_cell_destination_tests.
#[test]
fn teleport_relocate_reports_moving() {
    let mut entity = entity_with(LocomotorKind::Teleport);
    entity.install_teleport_state_for_test(Some(TeleportState::for_test(
        TeleportPhase::Relocate,
        20,
        20,
        16,
    )));

    let state = ready_state_for(&entity, None, 100).expect("Teleport has a producer");
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
        let runtime = entity
            .locomotor
            .as_mut()
            .and_then(|locomotor| locomotor.jumpjet_runtime_mut())
            .expect("a Jumpjet locomotor carries its runtime");
        *runtime = runtime.clone().with_phase_for_test(state);
        let ready = ready_state_for(&entity, None, 100).expect("Jumpjet has a producer");
        assert_eq!(ready.is_moving_now(), moving, "native state {state}");
    }

    // A pending order does not make a holding Jumpjet "moving": state 2 is
    // excluded whatever the Foot destination says.
    let runtime = entity
        .locomotor
        .as_mut()
        .and_then(|locomotor| locomotor.jumpjet_runtime_mut())
        .unwrap();
    *runtime = runtime.clone().with_phase_for_test(2);
    entity.movement_target = Some(moving_target());
    entity.navigation.path_replay = order_route();
    assert!(!ready_state_for(&entity, None, 100).unwrap().is_moving_now());
}

/// A standing infantryman is not moving.
#[test]
fn idle_walker_reports_not_moving() {
    let entity = entity_with(LocomotorKind::Walk);
    let state = ready_state_for(&entity, None, 100).expect("Walk has a producer");
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
    entity.foot_speed.set_speed_fraction(SIM_ONE);
    let state = ready_state_for(&entity, None, 100).expect("Walk has a producer");
    assert!(state.is_moving_now());
}

/// An outstanding order without an admitted head cannot defer commencement.
#[test]
fn blocked_walker_reports_not_moving() {
    let mut entity = entity_with(LocomotorKind::Walk);
    entity.movement_target = Some(moving_target());
    entity.navigation.path_replay = order_route();
    let state = ready_state_for(&entity, None, 100).expect("Walk has a producer");
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
    entity.foot_speed.set_speed_fraction(SIM_ONE);
    assert!(entity.movement_target.is_none());
    assert!(is_moving_now(&entity, None, 100));
    let mut restored: GameEntity =
        serde_json::from_value(serde_json::to_value(&entity).unwrap()).unwrap();
    assert!(is_moving_now(&restored, None, 100));
    restored.locomotor.as_mut().unwrap().set_step_head(None);
    assert!(!is_moving_now(&restored, None, 100));
    assert_eq!(
        restored.locomotor.as_ref().unwrap().walk_is_moving(),
        Some(true)
    );
}

#[test]
fn retained_motion_and_walk_readiness_match_original_queries() {
    use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/locomotor_moving.json",
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
                assert!(
                    entity
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .install_drive_state_for_test(Some(
                            DriveLocomotionRuntime::default()
                                .with_destination_for_test(optional(coord(&input["destination"])))
                                .with_head_to_for_test(head)
                        ))
                )
            }
            LocomotorKind::Ship => {
                assert!(
                    entity
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .install_ship_state_for_test(Some(
                            ShipLocomotionRuntime::default()
                                .with_destination_for_test(optional(coord(&input["destination"])))
                                .with_head_to_for_test(head)
                        ))
                )
            }
            LocomotorKind::Walk => {
                let LocomotorRuntimePayload::Walk(state) =
                    &mut entity.locomotor.as_mut().unwrap().runtime_payload
                else {
                    unreachable!()
                };
                state.head = head;
                state.moving = input["moving"].as_bool().unwrap();
                // The harness pokes Foot+0x578 directly, including -1, which
                // SetSpeedFraction (its only writer) never stores: the owner
                // stores +0 for it. IsMovingNow's `<= 0` test (0x0075AB52..63)
                // answers -1 and 0 alike, so those rows still check the reader.
                entity
                    .foot_speed
                    .set_speed_fraction(SimFixed::from_num(input["speed"].as_f64().unwrap()));
            }
            _ => unreachable!(),
        }
        // The compatibility path/order must not affect any represented query.
        for has_order in [false, true] {
            entity.movement_target = has_order.then(moving_target);
            entity.navigation.path_replay = if has_order {
                order_route()
            } else {
                Default::default()
            };
            entity.navigation.nav_com =
                has_order.then(|| crate::sim::components::NavTargetRef::cell(6, 5));
            if kind == LocomotorKind::Walk {
                assert_eq!(
                    is_moving_now(&entity, None, 100),
                    row["moving_now"].as_bool().unwrap(),
                    "{row}"
                );
            }
            assert_eq!(
                crate::sim::movement::motion_query::is_moving(&entity),
                Some(row["moving"].as_bool().unwrap()),
                "{row}"
            );
        }
    }
}

fn hover_runtime(entity: &mut GameEntity, runtime: super::super::hover::HoverRuntime) {
    if let Some(locomotor) = entity.locomotor.as_mut() {
        locomotor.runtime_payload =
            super::super::locomotion::piggyback::LocomotorRuntimePayload::Hover(runtime);
    }
}

/// Is_Moving_Now 0x00514C80: with neither a destination nor a head the Foot is
/// not moving, whatever request is left.
#[test]
fn stopped_hover_reports_not_moving_despite_stale_request() {
    let mut entity = entity_with(LocomotorKind::Hover);
    hover_runtime(
        &mut entity,
        super::super::hover::HoverRuntime::moving_for_test(None, NativeF64Bits::ONE),
    );
    let state = ready_state_for(&entity, None, 100).expect("Hover has a producer");
    assert!(!state.is_moving_now());
}

/// A destination and a nonzero request read moving.
#[test]
fn hover_under_way_reports_moving() {
    let mut entity = entity_with(LocomotorKind::Hover);
    hover_runtime(
        &mut entity,
        super::super::hover::HoverRuntime::moving_for_test(
            Some(DriveCoord::cell(4, 4, 0)),
            NativeF64Bits::HALF,
        ),
    );
    let state = ready_state_for(&entity, None, 100).expect("Hover has a producer");
    assert!(state.is_moving_now());
}

/// A zero request, the hard-turn case, reads not moving under way.
#[test]
fn hover_turn_stall_reports_not_moving() {
    let mut entity = entity_with(LocomotorKind::Hover);
    hover_runtime(
        &mut entity,
        super::super::hover::HoverRuntime::moving_for_test(
            Some(DriveCoord::cell(4, 4, 0)),
            NativeF64Bits::POSITIVE_ZERO,
        ),
    );
    let state = ready_state_for(&entity, None, 100).expect("Hover has a producer");
    assert!(!state.is_moving_now());
}

/// Families with no readiness slot this gate consults must yield `None`, which
/// leaves the mission gate on its conservative answer rather than a guess.
#[test]
fn unmapped_families_yield_no_producer() {
    for kind in [LocomotorKind::Fly, LocomotorKind::Rocket] {
        let entity = entity_with(kind);
        assert!(
            ready_state_for(&entity, None, 100).is_none(),
            "{kind:?} has no faithful producer and must not be guessed"
        );
    }
}

/// An entity with no locomotor at all yields nothing.
#[test]
fn entity_without_locomotor_yields_no_producer() {
    let entity = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    assert!(ready_state_for(&entity, None, 100).is_none());
}
