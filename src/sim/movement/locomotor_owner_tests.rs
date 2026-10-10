//! Rust production-route regressions for the instance lifetime documented in
//! `locomotor_owner`. These supplied states do not certify native command or
//! warp behavior beyond the constructor/transfer/reuse boundaries cited there.

use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::Command;
use crate::sim::components::{DriveCoord, Health};
use crate::sim::movement;
use crate::sim::movement::locomotion::LocomotorRuntimePayload;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::track_process::TrackFamily;
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::sim::pathfinding::PathGrid;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

fn fixture() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=CMIN\n[CMIN]\nStrength=400\nSpeed=4\n\
         Harvester=yes\nTeleporter=yes\nMovementZone=Normal\n\
         Locomotor={4A582747-9839-11d1-B709-00A024DDAFD1}\n\
         [BuildingTypes]\n0=GAREFN\n[GAREFN]\nStrength=900\nDockUnload=yes\n",
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
    entity.foot_speed.set_speed_fraction(SimFixed::lit("0.5"));
    assert!(
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .install_drive_state_for_test(Some(
                DriveLocomotionRuntime::default()
                    // Is_Moving compares exact XY only. Retained Z deliberately differs
                    // from the owner's height, so retirement cannot depend on full XYZ.
                    .with_head_to_for_test(Some(DriveCoord::cell(8, 8, 731)))
                    .with_track_for_test(crate::sim::components::TrackProgress {
                        turn_index: 1,
                        cursor: 3,
                        residual: 971,
                        ..Default::default()
                    })
            ))
    );
}

fn activate_drive(entity: &mut GameEntity) {
    assert!(begin_drive_for_teleporter(entity, 19));
    supply_drive_state(entity);
}

fn owned_state(entity: &GameEntity) -> serde_json::Value {
    serde_json::to_value((
        &entity.locomotor,
        &entity.navigation.path_replay,
        &entity.foot_speed,
    ))
    .expect("serialized complete locomotor and Foot state")
}

fn assert_retired(entity: &GameEntity) {
    let locomotor = entity.locomotor.as_ref().expect("restored locomotor");
    assert_eq!(locomotor.active_kind(), LocomotorKind::Teleport);
    assert!(locomotor.piggyback.is_none());
    assert!(!locomotor.has_track_state(TrackFamily::Drive));
}

/// The Unit setter (`0x741970`) to cell (12, 8). With `dock_contact` a
/// `DockUnload=` refinery holds radio slot 0, so the Teleporter arm keeps the
/// Teleport primary; without it the arm installs a Drive.
fn destination(sim: &mut Simulation, rules: &RuleSet, dock_contact: bool) -> bool {
    if dock_contact {
        let owner = sim.interner.intern("Americans");
        let type_id = sim.interner.intern("GAREFN");
        sim.substrate
            .entities
            .insert(GameEntity::new_at_frame_zero_for_test(
                2,
                3,
                3,
                0,
                0,
                owner,
                Health { current: 900 },
                type_id,
                EntityCategory::Structure,
                0,
                5,
                false,
            ));
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .radio_contacts
            .set_slot(0, 2);
    }
    sim.set_unit_destination(
        1,
        crate::sim::components::NavTargetRef::cell(12, 8),
        rules,
        true,
    )
}

#[test]
fn dock_pad_destination_retires_drive_before_teleport_move_to() {
    let (mut sim, rules) = fixture();
    activate_drive(sim.substrate.entities.get_mut(1).unwrap());

    assert!(destination(&mut sim, &rules, true));

    let entity = sim.substrate.entities.get(1).unwrap();
    assert_retired(entity);
    assert!(entity.teleport_state().is_some());
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

/// The arm's can't-end branch (`0x0074258C..0x007425C6`): a Drive still on
/// its head is stopped, not ended; the Foot tail keeps NavCom without a
/// Move_To and Techno+0x1F8 forces the next setter call.
#[test]
fn refused_restore_keeps_live_head_and_forced_segment() {
    for forced in [false, true] {
        let (mut sim, rules) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        activate_drive(entity);
        // Both ordinary and forced tracks use the active class's raw head.
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .store_track_head(TrackFamily::Drive, Some(DriveCoord::cell(9, 8, 731)));
        if forced {
            assert!(sim.force_track(1, 0x47, DriveCoord::cell(9, 8, 731), None, None));
        }

        assert!(destination(&mut sim, &rules, true));
        assert!(!movement::tick_locomotor_piggyback_restore_one(
            &mut sim.substrate.entities,
            1
        ));
        let entity = sim.substrate.entities.get(1).unwrap();
        let locomotor = entity.locomotor.as_ref().unwrap();
        assert_eq!(locomotor.active_kind(), LocomotorKind::Drive);
        assert!(locomotor.piggyback.is_some());
        let loco = entity.locomotor.as_ref().unwrap();
        assert_eq!(
            loco.track_head(TrackFamily::Drive),
            Some(DriveCoord::cell(9, 8, 731))
        );
        assert_eq!(
            loco.track_destination(TrackFamily::Drive),
            None,
            "Stop_Moving, no Move_To"
        );
        assert_eq!(
            entity.navigation.nav_com,
            Some(crate::sim::components::NavTargetRef::cell(12, 8))
        );
        assert!(entity.setter_force_reassign);
        assert_eq!(
            entity.mission.current(),
            crate::sim::mission::MissionId::NONE
        );
        assert_eq!(
            entity.mission.queued(),
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Enter)
        );
        assert!(entity.teleport_state().is_none());
    }
}

#[test]
fn out_of_contact_destination_installs_fresh_drive_without_previous_instance_state() {
    let (mut sim, rules) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    // Retire a coherent old Drive, then take the next original Unit setter.
    // There is no entity-side slot in which that retired instance can survive.
    activate_drive(entity);
    let loco = entity.locomotor.as_mut().unwrap();
    let mut track = loco.track_progress(TrackFamily::Drive).unwrap();
    track.turn_index = 0x47;
    loco.store_track_progress(TrackFamily::Drive, track);
    assert!(try_end_piggyback(entity));

    assert!(destination(&mut sim, &rules, false));

    let entity = sim.substrate.entities.get(1).unwrap();
    let locomotor = entity.locomotor.as_ref().unwrap();
    assert_eq!(locomotor.active_kind(), LocomotorKind::Drive);
    assert_eq!(locomotor.effective_kind(), LocomotorKind::Teleport);
    assert!(entity.movement_target.is_some());
    assert_eq!(
        locomotor
            .track_progress(TrackFamily::Drive)
            .unwrap()
            .residual,
        0
    );
    assert_eq!(entity.foot_speed.applied_fraction(), SimFixed::lit("0.5"));
}

#[test]
fn reusing_active_drive_keeps_complete_instance_including_forced_track() {
    let (mut sim, _) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    activate_drive(entity);
    assert!(sim.force_track(1, 0x47, DriveCoord::cell(9, 8, 731), None, None));
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    let before = owned_state(entity);

    assert!(begin_drive_for_teleporter(entity, 900));
    assert_eq!(owned_state(entity), before);
}

/// The active object's kinds, down the piggyback chain.
fn chain(entity: &GameEntity) -> Vec<LocomotorKind> {
    let mut kinds = Vec::new();
    let mut at = entity.locomotor.as_ref();
    while let Some(locomotor) = at {
        kinds.push(locomotor.active_kind());
        at = locomotor.piggyback.as_deref();
    }
    kinds
}

/// Over an active piggyback the Teleporter arm ends it first when it may
/// end (`0x007425FE..0x00742681`), then begins the fresh Drive over
/// whatever is active without asking the class again
/// (`0x00742684..0x0074277E`). A Chrono Warp's Teleport over a driving
/// miner ends once its warp is over, so the fresh Drive goes over the Drive
/// it hands back; mid-warp it may not end, so the Drive goes over it.
#[test]
fn teleporter_drive_ends_what_may_end_and_begins_over_the_rest() {
    use LocomotorKind::{Drive, Teleport};
    for mid_warp in [false, true] {
        let (mut sim, _) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        activate_drive(entity);
        let driving = entity.locomotor.clone();
        let locomotor = entity.locomotor.as_mut().unwrap();
        assert!(locomotor.begin_piggyback(Teleport, 20));
        if mid_warp {
            let warp = movement::teleport_movement::ChronoWarp::new(
                DriveCoord::cell(12, 8, 0),
                entity.owner(),
                20,
            );
            let locomotor = entity.locomotor.as_mut().unwrap();
            locomotor.teleport_runtime_mut().unwrap().arm_chrono(warp);
        }
        let warp_teleport = entity.locomotor.clone();

        assert!(begin_drive_for_teleporter(entity, 21));
        let locomotor = entity.locomotor.as_ref().unwrap();
        if mid_warp {
            assert_eq!(chain(entity), [Drive, Teleport, Drive, Teleport]);
            assert_eq!(locomotor.piggyback.as_deref(), warp_teleport.as_ref());
        } else {
            assert_eq!(chain(entity), [Drive, Drive, Teleport]);
            assert_eq!(locomotor.piggyback.as_deref(), driving.as_ref());
        }
    }
}

#[test]
fn refused_installation_and_absent_stash_preserve_instance_and_foot_state() {
    for state in [
        None,
        Some(LocomotorState::for_test_kind(LocomotorKind::Drive)),
    ] {
        let (mut sim, _) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.locomotor = state;
        if entity.locomotor.is_some() {
            supply_drive_state(entity);
        } else {
            entity.navigation.path_replay = replay_fixture();
            entity.foot_speed.set_speed_fraction(SimFixed::lit("0.5"));
        }
        let before = owned_state(entity);

        assert!(!begin_drive_for_teleporter(entity, 37));
        assert!(!try_end_piggyback(entity));
        assert!(!end_admitted_piggyback(entity));
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
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .store_track_head(TrackFamily::Drive, Some(DriveCoord::cell(9, 8, 731)));
        }
        assert!(sim.apply_command("Americans", &Command::Stop { entity_id: 1 }, Some(&rules),));

        let entity = sim.substrate.entities.get(1).unwrap();
        if head_ahead {
            assert_eq!(
                entity.locomotor.as_ref().unwrap().active_kind(),
                LocomotorKind::Drive
            );
            assert_eq!(
                entity
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .track_head(TrackFamily::Drive),
                Some(DriveCoord::cell(9, 8, 731))
            );
            assert_eq!(
                entity
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .track_progress(TrackFamily::Drive)
                    .unwrap()
                    .residual,
                971
            );
        } else {
            assert_retired(entity);
        }
    }
}

/// Unit741970's class refusals return before its Teleporter swap (0x7423CD),
/// so a refused Chrono Miner order never installs Drive: the complete payload
/// and Foot state stay untouched. An accepted order has no rollback (the
/// Drive setter cannot refuse; see outbound_drive_tests).
#[test]
fn refused_miner_order_leaves_teleport_payload_untouched() {
    for retired_drive in [false, true] {
        let (mut sim, rules) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        if retired_drive {
            activate_drive(entity);
            assert!(try_end_piggyback(entity));
        }
        let mut path_runtime = crate::sim::components::FootPathRuntime::at_frame(0);
        path_runtime.movement_timer = crate::sim::timer::CdTimer::from_raw(-1, -7);
        path_runtime.blocked_timer = crate::sim::timer::CdTimer::from_raw(i32::MAX - 2, 31);
        path_runtime.path_blocked = true;
        path_runtime.retries_left = u32::MAX;
        entity.navigation.path_runtime = path_runtime;
        entity.dying = true;
        let before = owned_state(entity);
        let mut grid = PathGrid::test_all_blocked(16, 16);
        grid.set_blocked(8, 8, false);
        grid.set_blocked(12, 8, false);
        sim.path_grid = Some(std::sync::Arc::new(grid));

        assert!(
            !crate::sim::miner::miner_system::issue_stock_miner_drive_move(
                &mut sim,
                &rules,
                1,
                (12, 8),
                None,
            )
        );

        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(owned_state(entity), before);
        assert_eq!(entity.navigation.path_runtime, path_runtime);
        assert!(matches!(
            entity.locomotor.as_ref().unwrap().runtime_payload,
            LocomotorRuntimePayload::Teleport(_)
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
    assert!(try_end_piggyback(entity));
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
    expected.set_speed_fraction(SimFixed::lit("0.625"));
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
    assert!(
        !entity
            .locomotor
            .as_ref()
            .unwrap()
            .has_track_state(TrackFamily::Drive)
    );
    assert!(
        !entity
            .locomotor
            .as_ref()
            .unwrap()
            .has_track_state(TrackFamily::Ship)
    );
    assert_eq!(entity.foot_speed, expected);
    assert_eq!(loaded.state_hash(), sim.state_hash());
    let original = loaded.state_hash();
    for field in 0..2 {
        let speed = &mut loaded.substrate.entities.get_mut(1).unwrap().foot_speed;
        *speed = expected.clone();
        if field == 0 {
            speed.set_speed_fraction(speed.applied_fraction() + SimFixed::lit("0.125"));
        } else {
            *speed = Default::default();
            speed.set_speed_fraction(expected.applied_fraction());
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
    use crate::sim::components::FootSpeedState;
    let cases: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_speed_owner.json",
    ))
    .unwrap();
    // The setter-only rows are the owner's (`components.rs`).
    let cases: Vec<_> = cases
        .into_iter()
        .filter(|case| case["input"]["family"] != "setter")
        .collect();
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
            let drive =
                DriveLocomotionRuntime::default().with_target_speed_fraction_for_test(requested);
            let step = super::super::drive_locomotion::track_speed_prefix(
                &non_accelerating_prefix(),
                || unreachable!("Accelerates=false measures no distance"),
                drive.target_speed_fraction(),
                owner_speed.applied_fraction(),
            );
            owner_speed.set_speed_fraction(step.set_fraction.unwrap());
            assert_eq!(owner_speed.applied_fraction(), expected);
            entity.foot_speed = owner_speed.clone();
            assert!(begin_drive_for_teleporter(entity, 3));
            super::super::navcom::set_destination_internal_cell(entity, (12, 8), None, 0);
            assert_eq!(entity.foot_speed, owner_speed);
            assert_eq!(
                entity
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .track_target_fraction(TrackFamily::Drive),
                Some(SimFixed::from_num(
                    case["output"]["constructor_target"].as_f64().unwrap()
                ))
            );
            assert!(end_admitted_piggyback(entity));
            assert_retired(entity);
            assert_eq!(entity.foot_speed, owner_speed);
            assert_eq!(case["output"]["end_preserves_owner"], true);
        } else {
            let ship =
                ShipLocomotionRuntime::default().with_target_speed_fraction_for_test(requested);
            let step = super::super::drive_locomotion::track_speed_prefix(
                &non_accelerating_prefix(),
                || unreachable!("Accelerates=false measures no distance"),
                ship.target_speed_fraction(),
                owner_speed.applied_fraction(),
            );
            owner_speed.set_speed_fraction(step.set_fraction.unwrap());
            assert_eq!(owner_speed.applied_fraction(), expected);
            entity.foot_speed = owner_speed.clone();
            entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
            super::super::navcom::set_destination_internal_cell(entity, (12, 8), None, 0);
            assert_eq!(entity.foot_speed, owner_speed);
            assert_eq!(
                entity
                    .locomotor
                    .as_ref()
                    .unwrap()
                    .track_target_fraction(TrackFamily::Ship),
                Some(SimFixed::from_num(
                    case["output"]["constructor_target"].as_f64().unwrap()
                ))
            );
        }
        assert_eq!(case["output"]["constructor_preserves_owner"], true);
    }
}

#[test]
fn foot_stop_preserves_queue() {
    let (mut sim, _) = fixture();
    let entity = sim.substrate.entities.get_mut(1).unwrap();
    activate_drive(entity);
    entity.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(12, 8));
    super::super::navcom::foot_stop_moving(entity);
    assert_eq!(entity.navigation.path_replay, replay_fixture());
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
    assert!(
        !entity
            .locomotor
            .as_ref()
            .unwrap()
            .has_track_state(TrackFamily::Drive)
    );
    assert!(
        !entity
            .locomotor
            .as_ref()
            .unwrap()
            .has_track_state(TrackFamily::Ship)
    );
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
    let cases: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_path_queue.json",
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
            assert!(try_end_piggyback(entity));
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
fn drive_end_uses_native_gates_and_preserves_owner_state() {
    for denied in [0, 1, 2, 3] {
        let (mut sim, _) = fixture();
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        activate_drive(entity);
        match denied {
            1 => {
                entity
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .store_drive_end_permission(false);
            }
            2 => entity.foot_locomotor_swap_active = true,
            3 => {
                entity.locomotor.as_mut().unwrap().store_track_destination(
                    TrackFamily::Drive,
                    Some(DriveCoord { x: 0, y: 0, z: 1 }),
                );
            }
            _ => {}
        }
        let before = owned_state(entity);
        let speed = entity.foot_speed.clone();
        assert_eq!(try_end_piggyback(entity), denied == 0);
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
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .store_drive_end_permission(permission);
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
        entity
            .locomotor
            .as_mut()
            .unwrap()
            .store_drive_end_permission(true);
        entity.foot_locomotor_swap_active = false;
        assert_ne!(loaded.state_hash(), before);
        assert!(movement::tick_locomotor_piggyback_restore_one(
            &mut loaded.substrate.entities,
            1
        ));
    }
}

fn non_accelerating_prefix() -> super::super::drive_locomotion::TrackSpeedPrefix {
    super::super::drive_locomotion::TrackSpeedPrefix {
        accelerates: false,
        unit_passive: false,
        selector: 0,
        raw_type_speed: 1,
        accel: SIM_ZERO,
        decel: SIM_ZERO,
        slowdown_distance: 0,
        sinking: false,
        crush_slowdown: false,
    }
}
