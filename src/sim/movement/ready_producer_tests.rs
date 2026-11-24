//! Producer tests for the Mission gate's readiness inputs.
//!
//! These assert the mapping direction, not native parity. The predicate they
//! feed is separately proven exhaustively in `locomotor_ready.rs`.

use super::*;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::movement::SpeedRules;
use crate::sim::movement::motion_query::is_moving_now;
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::util::fixed_math::{SIM_ONE, SimFixed};
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

/// A speed crate multiplies the type speed inside GetCurrentSpeed
/// (`0x004DB1A0`), so the pickup reaches the next moving query without a
/// cache refresh. At three quarters of a lepton per frame the getter truncates
/// to 0; the doubled type speed reads one and a half.
#[test]
fn speed_crate_reaches_the_next_moving_query() {
    let mut entity = driving_mtnk();
    let rules = mtnk_rules();
    let interner = crate::sim::intern::test_interner();
    let houses = crate::sim::houses::Houses::default();
    let speed = SpeedRules::new(&rules, &interner, &houses);

    let full = speed.owner_current_speed(&entity);
    assert!(full > 1, "MTNK covers several leptons per frame");
    entity
        .foot_speed
        .set_speed_fraction(SimFixed::lit("0.75") / SimFixed::from_num(full));
    assert!(!is_moving_now(&entity, speed, 100));

    assert!(
        entity
            .foot_speed
            .accept_speed_crate(NativeF64Bits::from_bits(2.0f64.to_bits()))
    );
    assert!(is_moving_now(&entity, speed, 100));
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
                    x: 5 * 256 + i32::from(entity.position.sub_x().to_num::<i32>() as i16),
                    y: 5 * 256 + i32::from(entity.position.sub_y().to_num::<i32>() as i16),
                    z: 0,
                }))
            ))
    );

    let state = ready_state_for(
        &entity,
        crate::sim::movement::SpeedRules::new(
            crate::rules::ruleset::RuleSet::empty_for_test(),
            &crate::sim::intern::test_interner(),
            &crate::sim::houses::Houses::default(),
        ),
        100,
    )
    .expect("Drive has a producer");
    assert!(!state.is_moving_now());
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
        entity.position.set_rx_for_test((current.x / 256) as u16);
        entity.position.set_ry_for_test((current.y / 256) as u16);
        entity
            .position
            .set_sub_x_for_test(SimFixed::from_num(current.x % 256));
        entity
            .position
            .set_sub_y_for_test(SimFixed::from_num(current.y % 256));
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
                    entity.locomotor.as_mut().unwrap().runtime_payload_mut()
                else {
                    unreachable!()
                };
                state.set_head_for_test(head);
                state.set_moving_for_test(input["moving"].as_bool().unwrap());
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
        // A Foot+5E0 route must not affect any represented query.
        for has_order in [false, true] {
            entity.navigation.set_path_replay_for_test(if has_order {
                order_route()
            } else {
                Default::default()
            });
            entity.navigation.set_nav_com_for_test(
                has_order.then(|| crate::sim::components::NavTargetRef::cell(6, 5)),
            );
            if kind == LocomotorKind::Walk {
                assert_eq!(
                    is_moving_now(
                        &entity,
                        crate::sim::movement::SpeedRules::new(
                            crate::rules::ruleset::RuleSet::empty_for_test(),
                            &crate::sim::intern::test_interner(),
                            &crate::sim::houses::Houses::default()
                        ),
                        100
                    ),
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
