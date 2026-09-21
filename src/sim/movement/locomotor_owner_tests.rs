//! Rust production-route regressions for the instance lifetime documented in
//! `locomotor_owner`. These supplied states do not certify native command or
//! warp behavior beyond the constructor/transfer/reuse boundaries cited there.

use std::collections::BTreeMap;

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::components::{DriveCoord, Health};
use crate::sim::movement::locomotion::{LocomotorRuntimePayload, LocomotorSlot};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::movement::{self, teleport_movement};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

fn fixture() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=CMIN\n[CMIN]\nStrength=400\nSpeed=4\n\
         Harvester=yes\nTeleporter=yes\nMovementZone=Normal\n\
         Locomotor={4A582747-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .expect("CMIN fixture rules");
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    let type_id = sim.interner.intern("CMIN");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        8,
        8,
        0,
        0,
        owner,
        Health { current: 400 },
        type_id,
        EntityCategory::Unit,
        0,
        5,
        true,
    );
    entity.locomotor = Some(LocomotorState::from_object_type(
        rules.object("CMIN").expect("CMIN type"),
        rules.general.flight_level,
        0,
    ));
    entity.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(entity);
    (sim, rules)
}

fn replay_fixture() -> crate::sim::components::FootPathQueue {
    crate::sim::components::FootPathQueue {
        directions: vec![2, 8, 7],
        cursor: 1,
        reference_cell: Some((-17, 301)),
    }
}

fn supply_drive_state(entity: &mut GameEntity) {
    entity.navigation.path_replay = replay_fixture();
    entity.foot_speed.applied_fraction = SimFixed::lit("0.5");
    entity.foot_speed.cached_current_speed = 11;
    entity.drive_locomotion = Some(DriveLocomotionRuntime {
        // Is_Moving compares exact XY only. Retained Z deliberately differs
        // from the owner's height, so retirement cannot depend on full XYZ.
        head_to: Some(DriveCoord::cell(8, 8, 731)),
        track: crate::sim::components::TrackProgress {
            turn_index: 1,
            cursor: 3,
            residual: 971,
            ..Default::default()
        },
        ..Default::default()
    });
}

fn activate_drive(entity: &mut GameEntity) {
    assert!(begin_drive_for_teleporter(entity, 19));
    supply_drive_state(entity);
}

fn owned_state(entity: &GameEntity) -> serde_json::Value {
    serde_json::to_value((
        &entity.locomotor,
        &entity.drive_locomotion,
        &entity.navigation.path_replay,
        &entity.foot_speed,
    ))
    .expect("serialized locomotor and external instance state")
}

fn assert_retired(entity: &GameEntity) {
    let locomotor = entity.locomotor.as_ref().expect("restored locomotor");
    assert_eq!(locomotor.active_kind(), LocomotorKind::Teleport);
    assert!(locomotor.piggyback.is_none());
    assert!(entity.drive_locomotion.is_none());
}

fn destination(sim: &mut Simulation, rules: &RuleSet, building: bool) -> bool {
    let grid = PathGrid::test_all_passable(16, 16);
    movement::set_destination_for_teleporter_entity(
        &mut sim.substrate.entities,
        Some(&grid),
        1,
        (12, 8),
        SimFixed::from_num(6),
        false,
        None,
        None,
        None,
        None,
        None,
        &rules.general,
        true,
        true,
        building,
        None,
        37,
    )
}

#[test]
fn empty_destination_retires_drive_before_starting_primary_teleport() {
    let (mut sim, rules) = fixture();
    activate_drive(sim.substrate.entities.get_mut(1).unwrap());

    assert!(destination(&mut sim, &rules, false));

    let entity = sim.substrate.entities.get(1).unwrap();
    assert_retired(entity);
    assert!(entity.teleport_state.is_some());
}

#[test]
fn foot_ai_restore_retires_same_drive_fields_as_direct_destination() {
    let (mut sim, _) = fixture();
    activate_drive(sim.substrate.entities.get_mut(1).unwrap());

    assert!(movement::tick_locomotor_piggyback_restore_one(
        &mut sim.substrate.entities,
        1
    ));
    assert_retired(sim.substrate.entities.get(1).unwrap());
    assert!(!movement::tick_locomotor_piggyback_restore_one(
        &mut sim.substrate.entities,
        1
    ));
}

#[test]
fn refused_restore_keeps_live_head_and_forced_segment() {
    for forced in [false, true] {
        let (mut sim, rules) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        activate_drive(entity);
        // Both ordinary and forced tracks use the active class's raw head.
        entity.drive_locomotion.as_mut().unwrap().head_to = Some(DriveCoord::cell(9, 8, 731));
        if forced {
            assert!(sim.force_drive_track(1, 0x47, DriveCoord::cell(9, 8, 731)));
        }
        let before = owned_state(sim.substrate.entities.get(1).unwrap());

        assert!(!destination(&mut sim, &rules, false));
        assert!(!movement::tick_locomotor_piggyback_restore_one(
            &mut sim.substrate.entities,
            1
        ));
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(owned_state(entity), before);
        assert!(entity.teleport_state.is_none());
    }
}

#[test]
fn building_destination_installs_fresh_drive_without_previous_instance_state() {
    let (mut sim, rules) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    // A saved state produced by the old direct-END path could leave these
    // fields attached to primary Teleport. They must not become a new Drive.
    supply_drive_state(entity);
    entity.drive_locomotion.as_mut().unwrap().track.turn_index = 0x47;

    assert!(destination(&mut sim, &rules, true));

    let entity = sim.substrate.entities.get(1).unwrap();
    let locomotor = entity.locomotor.as_ref().unwrap();
    assert_eq!(locomotor.active_kind(), LocomotorKind::Drive);
    assert_eq!(locomotor.effective_kind(), LocomotorKind::Teleport);
    assert!(entity.movement_target.is_some());
    let drive = entity.drive_locomotion.as_ref().unwrap();
    assert_eq!(drive.track.residual, 0);
    assert_eq!(entity.foot_speed.applied_fraction, SimFixed::lit("0.5"));
    assert_eq!(entity.foot_speed.cached_current_speed, 11);
}

#[test]
fn reusing_active_drive_keeps_complete_instance_including_forced_track() {
    let (mut sim, _) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    activate_drive(entity);
    assert!(sim.force_drive_track(1, 0x47, DriveCoord::cell(9, 8, 731)));
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    let before = owned_state(entity);

    assert!(begin_drive_for_teleporter(entity, 900));
    assert_eq!(owned_state(entity), before);
}

#[test]
fn refused_installation_and_absent_stash_do_not_retire_external_state() {
    for state in [
        None,
        Some(LocomotorState::for_test_kind(LocomotorKind::Drive)),
        {
            let mut incoherent = LocomotorState::for_test_kind(LocomotorKind::Drive);
            incoherent.slot = LocomotorSlot::from_kind(LocomotorKind::Teleport);
            Some(incoherent)
        },
    ] {
        let (mut sim, _) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.locomotor = state;
        supply_drive_state(entity);
        let before = owned_state(entity);

        assert!(!begin_drive_for_teleporter(entity, 37));
        assert!(!try_restore_primary(entity));
        assert!(!restore_admitted_primary(entity));
        assert_eq!(owned_state(entity), before);
    }
}

#[test]
fn stop_command_retires_only_the_drive_admitted_by_its_existing_gate() {
    // Stop's existing swap policy is VERA-specific. This checks the changed
    // transfer lifetime and preserves its prior admission timing.
    for head_ahead in [false, true] {
        let (mut sim, rules) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        activate_drive(entity);
        if head_ahead {
            entity.drive_locomotion.as_mut().unwrap().head_to = Some(DriveCoord::cell(9, 8, 731));
        }
        assert!(sim.apply_command(
            "Americans",
            &Command::Stop { entity_id: 1 },
            Some(&rules),
            None,
            &BTreeMap::new(),
        ));

        let entity = sim.substrate.entities.get(1).unwrap();
        if head_ahead {
            assert_eq!(
                entity.locomotor.as_ref().unwrap().active_kind(),
                LocomotorKind::Drive
            );
            assert_eq!(
                entity.drive_locomotion.as_ref().unwrap().head_to,
                Some(DriveCoord::cell(9, 8, 731))
            );
            assert_eq!(
                entity.drive_locomotion.as_ref().unwrap().track.residual,
                971
            );
        } else {
            assert_retired(entity);
        }
    }
}

#[test]
fn finished_teleport_restores_suspended_drive_without_retiring_its_state() {
    let (mut sim, rules) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    supply_drive_state(entity);
    let before = owned_state(entity);

    assert!(teleport_movement::issue_teleport_command(
        &mut sim.substrate.entities,
        1,
        (12, 8),
        &rules.general,
        true,
        37,
    ));
    teleport_movement::tick_teleport_movement(
        &mut sim.substrate.entities,
        &mut sim.substrate.occupancy,
        &[1],
        1,
        None,
        None,
    );

    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.teleport_state.is_none());
    assert_eq!(
        entity.locomotor.as_ref().unwrap().layer,
        MovementLayer::Ground
    );
    assert_eq!(owned_state(entity), before);
}

#[test]
fn failed_miner_path_restores_full_payload_and_external_instance_state() {
    for stale_fields in [false, true] {
        let (mut sim, rules) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        if stale_fields {
            supply_drive_state(entity);
        }
        let path_runtime = crate::sim::components::FootPathRuntime {
            movement_timer: crate::sim::timer::CdTimer::from_raw(-1, -7),
            blocked_timer: crate::sim::timer::CdTimer::from_raw(i32::MAX - 2, 31),
            path_blocked: true,
            retries_left: u32::MAX,
        };
        entity.navigation.path_runtime = path_runtime;
        let before = owned_state(entity);
        let mut grid = PathGrid::test_all_blocked(16, 16);
        grid.set_blocked(8, 8, false);
        grid.set_blocked(12, 8, false);

        assert!(
            !crate::sim::miner::miner_system::issue_stock_miner_drive_move(
                &mut sim,
                &rules,
                &grid,
                1,
                (12, 8),
            )
        );

        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(owned_state(entity), before);
        assert_eq!(entity.navigation.path_runtime, path_runtime);
        assert!(matches!(
            entity.locomotor.as_ref().unwrap().runtime_payload,
            LocomotorRuntimePayload::Teleport(None)
        ));
        assert!(entity.movement_target.is_none());
    }
}

#[test]
fn foot_queue_survives_drive_retirement_construction_and_reuse() {
    let (mut sim, _) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    activate_drive(entity);
    assert!(entity.foot_speed.accept_speed_crate(
        crate::util::native_x87::NativeF64Bits::from_bits(1.2_f64.to_bits())
    ));
    let queue = entity.navigation.path_replay.clone();
    let speed = entity.foot_speed.clone();
    assert!(try_restore_primary(entity));
    assert_retired(entity);
    assert_eq!(entity.navigation.path_replay, queue);
    assert_eq!(entity.foot_speed, speed);
    assert!(begin_drive_for_teleporter(entity, 51));
    assert_eq!(entity.navigation.path_replay, queue);
    assert_eq!(entity.foot_speed, speed);
    assert!(begin_drive_for_teleporter(entity, 52));
    assert_eq!(entity.navigation.path_replay, queue);
    assert_eq!(entity.foot_speed, speed);
}

#[test]
fn foot_speed_without_class_payload_roundtrips_and_hashes_each_field() {
    let (mut sim, _) = fixture();
    // Scenario RNG is deliberately reset by the production deserializer.
    // Normalize this fixture to that load state before comparing whole hashes.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let mut expected = crate::sim::components::FootSpeedState::default();
    expected.applied_fraction = SimFixed::lit("0.625");
    expected.cached_current_speed = 13;
    assert!(
        expected.accept_speed_crate(crate::util::native_x87::NativeF64Bits::from_bits(
            1.2_f64.to_bits()
        ))
    );
    sim.substrate.entities.get_mut(1).unwrap().foot_speed = expected.clone();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "foot_speed", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    let entity = loaded.substrate.entities.get(1).unwrap();
    assert!(entity.drive_locomotion.is_none());
    assert!(entity.ship_locomotion.is_none());
    assert_eq!(entity.foot_speed, expected);
    assert_eq!(loaded.state_hash(), sim.state_hash());
    let original = loaded.state_hash();
    for field in 0..3 {
        let speed = &mut loaded.substrate.entities.get_mut(1).unwrap().foot_speed;
        *speed = expected.clone();
        if field == 0 {
            speed.applied_fraction += SimFixed::lit("0.125");
        } else if field == 1 {
            speed.cached_current_speed += 1;
        } else {
            *speed = Default::default();
            speed.applied_fraction = expected.applied_fraction;
            speed.cached_current_speed = expected.cached_current_speed;
            assert!(
                speed.accept_speed_crate(crate::util::native_x87::NativeF64Bits::from_bits(
                    1.3_f64.to_bits()
                ))
            );
        }
        assert_ne!(loaded.state_hash(), original, "Foot speed field {field}");
    }
}

#[test]
fn foot_speed_ownership_matches_original_helper_witnesses() {
    use crate::sim::components::{FootSpeedState, ShipLocomotionRuntime};
    use crate::util::fixed_math::SIM_ONE;
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/foot_speed_owner.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 12);
    for case in cases {
        let requested = SimFixed::from_num(case["input"]["requested"].as_f64().unwrap());
        let expected = SimFixed::from_num(case["output"]["applied"].as_f64().unwrap());
        let (mut sim, _) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let mut owner_speed = FootSpeedState::default();
        // The non-accelerating production branch reaches the same finite
        // SetSpeedFraction clamp. Native witnesses execute the complete setter.
        if case["input"]["family"] == "drive" {
            let mut drive = DriveLocomotionRuntime::default();
            drive.target_speed_fraction = requested;
            super::super::drive_locomotion::update_drive_speed_fraction(
                &drive,
                &mut owner_speed,
                false,
                false,
                SIM_ONE,
                SIM_ZERO,
                SIM_ZERO,
                SIM_ZERO,
                SIM_ONE,
            );
            assert_eq!(owner_speed.applied_fraction, expected);
            entity.foot_speed = owner_speed.clone();
            assert!(begin_drive_for_teleporter(entity, 3));
            super::super::navcom::set_destination_internal_cell(entity, (12, 8), None);
            assert_eq!(entity.foot_speed, owner_speed);
            assert_eq!(
                entity
                    .drive_locomotion
                    .as_ref()
                    .unwrap()
                    .target_speed_fraction,
                SimFixed::from_num(case["output"]["constructor_target"].as_f64().unwrap())
            );
            assert!(restore_admitted_primary(entity));
            assert_retired(entity);
            assert_eq!(entity.foot_speed, owner_speed);
            assert_eq!(case["output"]["end_preserves_owner"], true);
        } else {
            let mut ship = ShipLocomotionRuntime::default();
            ship.target_speed_fraction = requested;
            super::super::drive_locomotion::update_ship_speed_fraction(
                &ship,
                &mut owner_speed,
                false,
                false,
                SIM_ONE,
                SIM_ZERO,
                SIM_ZERO,
                SIM_ZERO,
                SIM_ONE,
            );
            assert_eq!(owner_speed.applied_fraction, expected);
            entity.foot_speed = owner_speed.clone();
            entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
            super::super::navcom::set_destination_internal_cell(entity, (12, 8), None);
            assert_eq!(entity.foot_speed, owner_speed);
            assert_eq!(
                entity
                    .ship_locomotion
                    .as_ref()
                    .unwrap()
                    .target_speed_fraction,
                SimFixed::from_num(case["output"]["constructor_target"].as_f64().unwrap())
            );
        }
        assert_eq!(case["output"]["constructor_preserves_owner"], true);
    }
}

#[test]
fn foot_stop_preserves_queue_while_explicit_abandonment_exhausts_it() {
    let (mut sim, _) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    activate_drive(entity);
    entity.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(12, 8));
    super::super::navcom::foot_stop_moving(entity);
    assert_eq!(entity.navigation.path_replay, replay_fixture());
    super::super::movement_commands::stop_navigation_at_committed_head(entity);
    assert!(
        entity
            .navigation
            .path_replay
            .remaining_directions()
            .is_empty()
    );
    assert_eq!(
        entity.navigation.path_replay.reference_cell,
        Some((-17, 301))
    );
}

#[test]
fn foot_queue_without_class_payload_roundtrips_and_hashes_each_field() {
    let (mut sim, _) = fixture();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .navigation
        .path_replay = replay_fixture();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "foot_queue", 0);
    let mut loaded = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    let entity = loaded.substrate.entities.get(1).unwrap();
    assert!(entity.drive_locomotion.is_none());
    assert!(entity.ship_locomotion.is_none());
    assert_eq!(entity.navigation.path_replay, replay_fixture());
    let original_hash = loaded.state_hash();
    for field in 0..3 {
        let queue = &mut loaded
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .navigation
            .path_replay;
        *queue = replay_fixture();
        match field {
            0 => queue.directions[1] = 6,
            1 => queue.cursor = 2,
            _ => queue.reference_cell = Some((-18, 301)),
        }
        assert_ne!(
            loaded.state_hash(),
            original_hash,
            "Foot queue field {field}"
        );
    }
}

#[test]
fn foot_queue_operations_match_original_memory_witnesses() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/foot_path_queue.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 28);
    for case in cases {
        let input = &case["input"];
        let operation = input["operation"].as_str().unwrap();
        let directions: Vec<u8> = serde_json::from_value(input["directions"].clone()).unwrap();
        let reference: (i16, i16) = serde_json::from_value(input["reference"].clone()).unwrap();
        let endpoint: (i32, i32) = serde_json::from_value(input["endpoint"].clone()).unwrap();
        let (mut sim, _) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        activate_drive(entity);
        // Native shifts the queue; Rust can retain an already-consumed prefix.
        entity.navigation.path_replay = crate::sim::components::FootPathQueue {
            directions: [vec![6], directions].concat(),
            cursor: 1,
            reference_cell: Some(reference),
        };
        if operation.contains("fresh") {
            super::super::path_markers::accept_path_replay(
                &mut entity.navigation.path_replay,
                ((endpoint.0 / 256) as i16, (endpoint.1 / 256) as i16),
                if operation.ends_with("two") { 2 } else { 1 },
            );
        } else if operation == "foot_stop" {
            super::super::navcom::foot_stop_moving(entity);
        } else if operation == "drive_end" {
            assert!(try_restore_primary(entity));
        } else {
            super::super::path_markers::consume_path_replay(&mut entity.navigation.path_replay, 1);
        }
        let expected: Vec<u8> =
            serde_json::from_value(case["output"]["directions"].clone()).unwrap();
        let expected_reference: (i16, i16) =
            serde_json::from_value(case["output"]["reference"].clone()).unwrap();
        assert_eq!(
            entity.navigation.path_replay.remaining_directions(),
            expected,
            "{operation}"
        );
        assert_eq!(
            entity.navigation.path_replay.reference_cell,
            Some(expected_reference),
            "{operation}"
        );
    }
}

#[test]
fn foot_idle_drive_end_uses_native_gates_and_preserves_owner_state() {
    for denied in [0, 1, 2, 3] {
        let (mut sim, _) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        activate_drive(entity);
        entity.locomotor.as_mut().unwrap().phase =
            super::super::locomotor::GroundMovePhase::Cruising;
        match denied {
            1 => entity.drive_locomotion.as_mut().unwrap().end_permitted = false,
            2 => entity.foot_locomotor_swap_active = true,
            3 => {
                entity.drive_locomotion.as_mut().unwrap().destination =
                    Some(DriveCoord { x: 0, y: 0, z: 1 })
            }
            _ => {}
        }
        let before = owned_state(entity);
        let speed = entity.foot_speed.clone();
        assert_eq!(try_end_drive_at_foot_idle(entity), denied == 0);
        assert_eq!(entity.foot_speed, speed);
        if denied != 0 {
            assert_eq!(owned_state(entity), before);
        } else {
            assert_retired(entity);
        }
    }
}

#[test]
fn drive_end_denial_flags_survive_save_and_block_generic_restore() {
    use crate::sim::snapshot::GameSnapshot;
    for permission in [false, true] {
        let (mut sim, _) = fixture();
        // Production load resets Scenario RNG; normalize the fixture before
        // comparing whole hashes, as in the Foot speed roundtrip above.
        sim.scenario_rng = crate::sim::rng::SimRng::new(0);
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        activate_drive(entity);
        entity.drive_locomotion.as_mut().unwrap().end_permitted = permission;
        entity.foot_locomotor_swap_active = permission;
        let bytes = GameSnapshot::save(&sim, 0, 0, "drive_end_gate", 0);
        let mut loaded = GameSnapshot::load(&bytes).unwrap().sim;
        assert_eq!(loaded.state_hash(), sim.state_hash());
        assert!(!movement::tick_locomotor_piggyback_restore_one(
            &mut loaded.substrate.entities,
            1
        ));
        let before = loaded.state_hash();
        let entity = loaded.substrate.entities.get_mut(1).unwrap();
        entity.drive_locomotion.as_mut().unwrap().end_permitted = true;
        entity.foot_locomotor_swap_active = false;
        assert_ne!(loaded.state_hash(), before);
        assert!(movement::tick_locomotor_piggyback_restore_one(
            &mut loaded.substrate.entities,
            1
        ));
    }
}
