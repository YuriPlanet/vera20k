//! Tests for the sprite animation system.
//!
//! Separated from animation.rs to stay within the 400-line file limit.

use std::collections::BTreeMap;

use crate::map::entities::EntityCategory;
use crate::rules::art_data::ArtRegistry;
use crate::rules::infantry_sequence::parse_infantry_sequence_registry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::*;
use crate::sim::combat::{AttackTarget, PendingInfantryFire};
use crate::sim::components::{
    DriveCoord, DriveLocomotionRuntime, Health, MovementTarget, ShipLocomotionRuntime,
};
use crate::sim::entity_store::EntityStore;
use crate::sim::game_entity::GameEntity;
use crate::sim::game_options::GameOptions;
use crate::sim::intern::StringInterner;
use crate::sim::movement::FacingClass;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// Helper: create a SequenceDef for tests.
fn test_def(
    start_frame: u16,
    frame_count: u16,
    facings: u8,
    frame_delay: u16,
    loop_mode: LoopMode,
) -> SequenceDef {
    SequenceDef {
        start_frame,
        frame_count,
        facings,
        facing_multiplier: frame_count,
        frame_delay,
        normalized: false,
        completion_facing: None,
        loop_mode,
        facing_slots: FacingSlots::InfantryTable,
    }
}

// --- resolve_shp_frame tests ---

#[test]
fn test_resolve_stand_facing_north() {
    let def = test_def(0, 1, 8, 200, LoopMode::Loop);
    // Facing 0 is cell-N, which is infantry slot 7 → frame 7.
    assert_eq!(resolve_shp_frame(&def, 0, 0), 7);
}

#[test]
fn test_resolve_stand_facing_south() {
    let def = test_def(0, 1, 8, 200, LoopMode::Loop);
    // Facing 128 is cell-S, which is infantry slot 3 → frame 3.
    assert_eq!(resolve_shp_frame(&def, 128, 0), 3);
}

#[test]
fn test_resolve_walk_facing_east_frame_3() {
    let def = test_def(8, 6, 8, 100, LoopMode::Loop);
    // Facing 64 is cell-E → slot 5 → frame = 8 + 5*6 + 3 = 41.
    assert_eq!(resolve_shp_frame(&def, 64, 3), 41);
}

#[test]
fn test_resolve_non_directional() {
    let def = test_def(56, 15, 1, 120, LoopMode::Loop);
    // Non-directional: facing is ignored. Frame 7 → 56 + 7 = 63
    assert_eq!(resolve_shp_frame(&def, 128, 7), 63);
}

#[test]
fn test_resolve_frame_index_wraps() {
    let def = test_def(8, 6, 8, 100, LoopMode::Loop);
    // Frame 7 wraps: 7 % 6 = 1. Facing 0 is cell-N → slot 7 → 8 + 7*6 + 1 = 51.
    assert_eq!(resolve_shp_frame(&def, 0, 7), 51);
}

#[test]
fn test_resolve_facing_multiplier_differs_from_frame_count() {
    // Simulates a sequence with facing_multiplier=8 but frame_count=4
    let def = SequenceDef {
        start_frame: 0,
        frame_count: 4,
        facings: 8,
        facing_multiplier: 8,
        frame_delay: 1,
        normalized: false,
        completion_facing: None,
        loop_mode: LoopMode::Loop,
        facing_slots: FacingSlots::InfantryTable,
    };
    // Facing 64 is cell-E → slot 5 → frame = 0 + 5*8 + 3 = 43.
    assert_eq!(resolve_shp_frame(&def, 64, 3), 43);
}

#[test]
fn test_resolve_all_8_facings() {
    let def = test_def(0, 1, 8, 200, LoopMode::Loop);
    // DirStruct (clockwise, cell-relative) → infantry SHP frame slot.
    // SHP frame 0 is the screen-north pose, which is cell NW; slots then run
    // counter-clockwise: 0=NW, 1=W, 2=SW, 3=S, 4=SE, 5=E, 6=NE, 7=N.
    assert_eq!(resolve_shp_frame(&def, 0, 0), 7); // cell-N  → SHP 7
    assert_eq!(resolve_shp_frame(&def, 32, 0), 6); // cell-NE → SHP 6
    assert_eq!(resolve_shp_frame(&def, 64, 0), 5); // cell-E  → SHP 5
    assert_eq!(resolve_shp_frame(&def, 96, 0), 4); // cell-SE → SHP 4
    assert_eq!(resolve_shp_frame(&def, 128, 0), 3); // cell-S  → SHP 3
    assert_eq!(resolve_shp_frame(&def, 160, 0), 2); // cell-SW → SHP 2
    assert_eq!(resolve_shp_frame(&def, 192, 0), 1); // cell-W  → SHP 1
    assert_eq!(resolve_shp_frame(&def, 224, 0), 0); // cell-NW → SHP 0
}

#[test]
fn test_infantry_slot_boundaries_round_not_truncate() {
    // The eight octant centres above coincide under both a truncating and a
    // rounding quantiser, so they cannot tell the two apart. These values can.
    //
    // Native slot boundaries sit at facing ≡ 12 (mod 32), not at ≡ 0: the arc
    // for slot 7 runs 236..=255 plus 0..=11. A quantiser that truncates
    // `facing / 32` holds the previous slot across each of these pairs.
    let def = test_def(0, 1, 8, 200, LoopMode::Loop);

    assert_eq!(resolve_shp_frame(&def, 11, 0), 7);
    assert_eq!(
        resolve_shp_frame(&def, 12, 0),
        6,
        "slot flips at 12, not 32"
    );

    assert_eq!(resolve_shp_frame(&def, 31, 0), 6);
    assert_eq!(
        resolve_shp_frame(&def, 32, 0),
        6,
        "32 is mid-arc, not a boundary"
    );

    assert_eq!(resolve_shp_frame(&def, 43, 0), 6);
    assert_eq!(resolve_shp_frame(&def, 44, 0), 5);

    // The wrap back onto slot 7 happens 20/256 of a turn before cell-north.
    assert_eq!(resolve_shp_frame(&def, 235, 0), 0);
    assert_eq!(resolve_shp_frame(&def, 236, 0), 7);
    assert_eq!(resolve_shp_frame(&def, 255, 0), 7);
}

#[test]
fn test_infantry_facing_slot_covers_every_byte() {
    // Every facing byte must land on a real frame block; an index outside 0..=7
    // would read past the end of a standing block.
    for facing in 0..=u8::MAX {
        assert!(
            infantry_facing_slot(facing) < 8,
            "facing {facing} produced an out-of-range slot"
        );
    }
}

// --- SHP vehicle frame blocks ---

/// Terror Drone walk block: `WalkFrames=6`, `FiringFrames=4`, 8 facings, no
/// `StandingFrames`. Walk occupies frame 0 and strides 6 frames per slot.
fn dron_walk_def() -> SequenceDef {
    SequenceDef {
        start_frame: 0,
        frame_count: 6,
        facings: 8,
        facing_multiplier: 6,
        frame_delay: 3,
        normalized: false,
        completion_facing: None,
        loop_mode: LoopMode::Loop,
        facing_slots: FacingSlots::VehicleOctant,
    }
}

#[test]
fn test_terror_drone_walk_facings() {
    let def = dron_walk_def();
    // Vehicle slots run clockwise from screen-north (cell NW), and the block
    // index is the octant advanced by one — so frame 0 is NW, not N.
    assert_eq!(resolve_shp_frame(&def, 224, 0), 0); // cell-NW → slot 0
    assert_eq!(resolve_shp_frame(&def, 0, 0), 6); // cell-N  → slot 1
    assert_eq!(resolve_shp_frame(&def, 32, 0), 12); // cell-NE → slot 2
    assert_eq!(resolve_shp_frame(&def, 64, 0), 18); // cell-E  → slot 3
    assert_eq!(resolve_shp_frame(&def, 96, 0), 24); // cell-SE → slot 4
    assert_eq!(resolve_shp_frame(&def, 128, 0), 30); // cell-S  → slot 5
    assert_eq!(resolve_shp_frame(&def, 160, 0), 36); // cell-SW → slot 6
    assert_eq!(resolve_shp_frame(&def, 192, 0), 42); // cell-W  → slot 7
}

#[test]
fn test_terror_drone_walk_slot_rounds_to_nearest_octant() {
    let def = dron_walk_def();
    // Vehicle boundaries sit at facing ≡ 16 (mod 32) — halfway between octant
    // centres. Truncating `facing / 32` would hold slot 0's frames across both.
    assert_eq!(resolve_shp_frame(&def, 15, 0), 6, "still nearest cell-N");
    assert_eq!(resolve_shp_frame(&def, 16, 0), 12, "rounds up to cell-NE");
    // Above 240 the octant rounds forward onto cell-N and wraps to slot 1.
    assert_eq!(resolve_shp_frame(&def, 239, 0), 0);
    assert_eq!(resolve_shp_frame(&def, 240, 0), 6);
}

#[test]
fn test_terror_drone_walk_advances_within_slot() {
    let def = dron_walk_def();
    // Facing cell-W is slot 7 → frames 42..=47 as the walk cycle advances.
    assert_eq!(resolve_shp_frame(&def, 192, 3), 45);
    assert_eq!(resolve_shp_frame(&def, 192, 8), 44, "8 % 6 = 2");
}

#[test]
fn test_shp_vehicle_non_eight_facings_draws_slot_zero() {
    // The vehicle draw path only computes a facing slot when the body declares
    // exactly 8 blocks; any other count draws block 0 for every facing.
    let def = SequenceDef {
        facings: 6,
        ..dron_walk_def()
    };
    assert_eq!(resolve_shp_frame(&def, 0, 0), 0);
    assert_eq!(resolve_shp_frame(&def, 128, 0), 0);
    assert_eq!(resolve_shp_frame(&def, 128, 2), 2);
}

fn gsi_13_06_active_shp_unit(kind: LocomotorKind) -> GameEntity {
    let mut entity = GameEntity::test_default(13_006, "DRON", "Soviet", 5, 5);
    entity.is_voxel = false;
    entity.lifecycle.in_limbo = false;
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    let head = DriveCoord::cell(6, 5, 0);
    match kind {
        LocomotorKind::Drive => {
            entity.foot_speed.applied_fraction = SimFixed::from_num(1);
            entity.foot_speed.cached_current_speed = 1;
            entity.drive_locomotion = Some(DriveLocomotionRuntime {
                destination: Some(head),
                head_to: Some(head),
                ..Default::default()
            });
        }
        LocomotorKind::Ship => {
            entity.foot_speed.applied_fraction = SimFixed::from_num(1);
            entity.foot_speed.cached_current_speed = 1;
            entity.ship_locomotion = Some(ShipLocomotionRuntime {
                destination: Some(head),
                head_to: Some(head),
                ..Default::default()
            });
        }
        _ => unreachable!("stock SHP vehicle fixture uses Drive or Ship"),
    }
    let mut target = make_movement_target();
    target.current_speed = SimFixed::from_num(1);
    entity.movement_target = Some(target);
    entity
}

#[test]
fn gsi_13_06_body_counter_uses_absolute_precommit_binary_frame_phase() {
    let cadence = ShpVehicleCadence {
        walk_rate: 4,
        idle_rate: 8,
    };

    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut entity = gsi_13_06_active_shp_unit(kind);
        tick_shp_vehicle_body_frame_counter(&mut entity, cadence, 3);
        assert_eq!(entity.body_frame_counter, 0);
        tick_shp_vehicle_body_frame_counter(&mut entity, cadence, 4);
        assert_eq!(entity.body_frame_counter, 1);
        tick_shp_vehicle_body_frame_counter(&mut entity, cadence, 5);
        assert_eq!(entity.body_frame_counter, 1);
        tick_shp_vehicle_body_frame_counter(&mut entity, cadence, 8);
        assert_eq!(entity.body_frame_counter, 2);
    }
}

#[test]
fn gsi_13_06_body_counter_wraps_and_survives_moving_idle_transitions() {
    let mut entity = gsi_13_06_active_shp_unit(LocomotorKind::Drive);
    entity.body_frame_counter = u32::MAX;
    tick_shp_vehicle_body_frame_counter(
        &mut entity,
        ShpVehicleCadence {
            walk_rate: 1,
            idle_rate: 8,
        },
        4,
    );
    assert_eq!(entity.body_frame_counter, 0, "native dword wraps");

    entity.body_frame_counter = 9;
    if let Some(drive) = entity.drive_locomotion.as_mut() {
        drive.destination = None;
        drive.head_to = None;
        entity.foot_speed.applied_fraction = SIM_ZERO;
        entity.foot_speed.cached_current_speed = 0;
    }
    tick_shp_vehicle_body_frame_counter(
        &mut entity,
        ShpVehicleCadence {
            walk_rate: 4,
            idle_rate: 8,
        },
        8,
    );
    assert_eq!(
        entity.body_frame_counter, 10,
        "switching to idle changes only the absolute divisor"
    );
}

#[test]
fn gsi_13_06_counter_suppressions_hold_the_persistent_value() {
    let cadence = ShpVehicleCadence {
        walk_rate: 1,
        idle_rate: 1,
    };
    let base = gsi_13_06_active_shp_unit(LocomotorKind::Drive);
    let mut variants = Vec::new();

    let mut in_limbo = base.clone();
    in_limbo.lifecycle.in_limbo = true;
    variants.push(("in limbo", in_limbo));
    let mut dead = base.clone();
    dead.lifecycle.object_alive = false;
    variants.push(("not native-alive", dead));
    let mut dying = base.clone();
    dying.dying = true;
    variants.push(("dying", dying));
    let mut falling = base.clone();
    falling.set_object_is_falling_down_for_test(1);
    variants.push(("falling", falling));
    let mut deployed = base.clone();
    deployed.deploy_state = Some(crate::sim::deploy::DeployPhase::Deployed);
    variants.push(("deploy state", deployed));
    let mut warp_out = base.clone();
    warp_out.teleport_state = Some(TeleportState {
        phase: TeleportPhase::Relocate,
        target_rx: 8,
        target_ry: 8,
        being_warped_ticks: 0,
    });
    variants.push(("warp out", warp_out));
    let mut warp_in = base.clone();
    warp_in.teleport_state = Some(TeleportState {
        phase: TeleportPhase::ChronoDelay,
        target_rx: 8,
        target_ry: 8,
        being_warped_ticks: 1,
    });
    variants.push(("warp in", warp_in));
    let mut piggyback = base.clone();
    assert!(
        piggyback
            .locomotor
            .as_mut()
            .expect("locomotor")
            .begin_piggyback(LocomotorKind::Teleport, MovementLayer::Ground, 0)
    );
    variants.push(("piggyback", piggyback));

    for (name, mut entity) in variants {
        entity.body_frame_counter = 17;
        tick_shp_vehicle_body_frame_counter(&mut entity, cadence, 1);
        assert_eq!(entity.body_frame_counter, 17, "{name}");
    }
}

#[test]
fn gsi_13_06_draw_and_cadence_use_distinct_movement_predicates() {
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut entity = gsi_13_06_active_shp_unit(kind);
        match kind {
            LocomotorKind::Drive => {
                entity.foot_speed.cached_current_speed = 0;
            }
            LocomotorKind::Ship => {
                entity.foot_speed.cached_current_speed = 0;
            }
            _ => unreachable!(),
        }

        assert!(
            crate::sim::movement::ready_producer::is_moving_for_unit_shp_draw(&entity),
            "slot-4 Is_Moving sees the class-owned destination"
        );
        assert!(
            !crate::sim::movement::ready_producer::is_moving_now_for(&entity, 4),
            "slot-32 Is_Moving_Now also requires positive applied speed"
        );
        tick_shp_vehicle_body_frame_counter(
            &mut entity,
            ShpVehicleCadence {
                walk_rate: 4,
                idle_rate: 0,
            },
            4,
        );
        assert_eq!(entity.body_frame_counter, 0, "IdleRate=0 holds the counter");
    }
}

#[test]
fn gsi_13_06_positive_fraction_below_get_current_speed_threshold_is_idle() {
    use crate::util::fixed_math::ra2_speed_to_leptons_per_second;

    for (name, kind, raw_speed) in [
        ("DLPH", LocomotorKind::Ship, 8),
        ("SQD", LocomotorKind::Ship, 8),
        ("DRON", LocomotorKind::Drive, 10),
    ] {
        let mut entity = gsi_13_06_active_shp_unit(kind);
        let fraction = SimFixed::lit("0.03");
        let owner_speed = crate::sim::movement::owner_current_speed_from_fraction(
            ra2_speed_to_leptons_per_second(raw_speed),
            fraction,
        );
        assert_eq!(owner_speed, 0, "{name} +0x538 truncation");
        match kind {
            LocomotorKind::Drive => {
                assert!(entity.drive_locomotion.is_some());
                entity.foot_speed.applied_fraction = fraction;
                entity.foot_speed.cached_current_speed = owner_speed;
            }
            LocomotorKind::Ship => {
                assert!(entity.ship_locomotion.is_some());
                entity.foot_speed.applied_fraction = fraction;
                entity.foot_speed.cached_current_speed = owner_speed;
            }
            _ => unreachable!(),
        }

        assert!(
            crate::sim::movement::ready_producer::is_moving_for_unit_shp_draw(&entity),
            "{name} slot +0x10 still sees its locomotor destination"
        );
        assert!(
            !crate::sim::movement::ready_producer::is_moving_now_for(&entity, 1),
            "{name} slot +0x80 requires truncated GetCurrentSpeed > 0"
        );
        tick_shp_vehicle_body_frame_counter(
            &mut entity,
            ShpVehicleCadence {
                walk_rate: 1,
                idle_rate: 0,
            },
            1,
        );
        assert_eq!(entity.body_frame_counter, 0, "{name} remains idle");
    }
}

#[test]
fn gsi_13_06_shp_movement_predicates_ignore_path_execution_surrogates() {
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut entity = gsi_13_06_active_shp_unit(kind);
        let owner = DriveCoord::cell(5, 5, 0);

        // A live MovementTarget is not either native class coordinate. With a
        // null destination and an equal-X/Y head, both slots answer false even
        // though the execution adapter still reports a positive speed.
        match kind {
            LocomotorKind::Drive => {
                entity.foot_speed.applied_fraction = SimFixed::from_num(1);
                entity.foot_speed.cached_current_speed = 1;
                entity.drive_locomotion = Some(DriveLocomotionRuntime {
                    head_to: Some(owner),
                    ..Default::default()
                });
            }
            LocomotorKind::Ship => {
                entity.foot_speed.applied_fraction = SimFixed::from_num(1);
                entity.foot_speed.cached_current_speed = 1;
                entity.ship_locomotion = Some(ShipLocomotionRuntime {
                    head_to: Some(owner),
                    ..Default::default()
                });
            }
            _ => unreachable!(),
        }
        assert!(!crate::sim::movement::ready_producer::is_moving_for_unit_shp_draw(&entity));
        assert!(!crate::sim::movement::ready_producer::is_moving_now_for(
            &entity, 4
        ));

        // Conversely, locomotor-owned state alone is sufficient; no
        // MovementTarget is needed by either draw or cadence.
        entity.movement_target = None;
        let head = DriveCoord::cell(6, 5, 0);
        match kind {
            LocomotorKind::Drive => {
                entity.foot_speed.applied_fraction = SimFixed::from_num(1);
                entity.foot_speed.cached_current_speed = 1;
                entity.drive_locomotion = Some(DriveLocomotionRuntime {
                    destination: Some(head),
                    head_to: Some(head),
                    ..Default::default()
                });
            }
            LocomotorKind::Ship => {
                entity.foot_speed.applied_fraction = SimFixed::from_num(1);
                entity.foot_speed.cached_current_speed = 1;
                entity.ship_locomotion = Some(ShipLocomotionRuntime {
                    destination: Some(head),
                    head_to: Some(head),
                    ..Default::default()
                });
            }
            _ => unreachable!(),
        }
        assert!(crate::sim::movement::ready_producer::is_moving_for_unit_shp_draw(&entity));
        assert!(crate::sim::movement::ready_producer::is_moving_now_for(
            &entity, 4
        ));
    }
}

fn gsi_13_06_shp_set(
    walk_frames: u16,
    stand_start: u16,
    cadence: ShpVehicleCadence,
) -> SequenceSet {
    let mut set = SequenceSet::new();
    set.set_shp_vehicle_cadence(cadence);
    set.insert(
        SequenceKind::Walk,
        SequenceDef {
            start_frame: 0,
            frame_count: walk_frames,
            facings: 8,
            facing_multiplier: walk_frames,
            frame_delay: 1,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::Loop,
            facing_slots: FacingSlots::VehicleOctant,
        },
    );
    set.insert(
        SequenceKind::Stand,
        SequenceDef {
            start_frame: stand_start,
            frame_count: 1,
            facings: 8,
            facing_multiplier: 1,
            frame_delay: 1,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::Loop,
            facing_slots: FacingSlots::VehicleOctant,
        },
    );
    set
}

#[test]
fn gsi_13_06_dlph_sqd_and_dron_draw_from_native_counter_blocks() {
    let dlph = gsi_13_06_shp_set(
        6,
        48,
        ShpVehicleCadence {
            walk_rate: 4,
            idle_rate: 8,
        },
    );
    assert_eq!(
        resolve_shp_vehicle_body_frame(&dlph, 0, 7, false),
        Some(7),
        "idle Dolphin uses walk/swim slot 1 and counter 7 % 6"
    );

    let sqd = gsi_13_06_shp_set(
        20,
        160,
        ShpVehicleCadence {
            walk_rate: 2,
            idle_rate: 4,
        },
    );
    assert_eq!(
        resolve_shp_vehicle_body_frame(&sqd, 0, 3, false),
        Some(23),
        "idle Squid uses walk/swim slot 1 and counter 3"
    );

    let dron = gsi_13_06_shp_set(6, 48, ShpVehicleCadence::native_defaults());
    assert_eq!(
        resolve_shp_vehicle_body_frame(&dron, 0, 99, false),
        Some(49),
        "idle Terror Drone with IdleRate=0 uses standing slot 1"
    );
    assert_eq!(
        resolve_shp_vehicle_body_frame(&dron, 0, 7, true),
        Some(7),
        "moving Terror Drone uses counter 7 % 6"
    );
}

// --- advance_animation tests ---

#[test]
fn test_advance_one_frame() {
    let def = test_def(0, 6, 8, 1, LoopMode::Loop);
    let mut anim: Animation = Animation::new(SequenceKind::Walk);
    let result = advance_animation(&mut anim, &def, &GameOptions::default());
    assert!(result.is_none());
    assert_eq!(anim.frame_index, 1);
}

#[test]
fn test_advance_loop_wraps_to_zero() {
    let def = test_def(0, 3, 1, 1, LoopMode::Loop);
    let mut anim: Animation = Animation::new(SequenceKind::Walk);
    anim.frame_index = 2; // Last frame
    advance_animation(&mut anim, &def, &GameOptions::default());
    assert_eq!(anim.frame_index, 0, "Should wrap to frame 0");
}

#[test]
fn test_advance_hold_last_frame() {
    let def = test_def(86, 15, 1, 1, LoopMode::HoldLast);
    let mut anim: Animation = Animation::new(SequenceKind::Die1);
    anim.frame_index = 14; // Last frame
    advance_animation(&mut anim, &def, &GameOptions::default());
    assert_eq!(anim.frame_index, 14, "Should hold last frame");
    assert!(anim.finished);
}

#[test]
fn test_advance_transition_to() {
    let def = test_def(56, 3, 1, 1, LoopMode::TransitionTo(SequenceKind::Stand));
    let mut anim: Animation = Animation::new(SequenceKind::Idle1);
    anim.frame_index = 2; // Last frame
    let result = advance_animation(&mut anim, &def, &GameOptions::default());
    assert_eq!(result, Some(SequenceKind::Stand));
}

#[test]
fn test_advance_accumulates_native_frames() {
    let def = test_def(0, 6, 1, 2, LoopMode::Loop);
    let mut anim: Animation = Animation::new(SequenceKind::Walk);
    for _ in 0..7 {
        advance_animation(&mut anim, &def, &GameOptions::default());
    }
    assert_eq!(anim.frame_index, 3);
    assert_eq!(anim.elapsed_frames, 1);
}

#[test]
fn normalized_action_delay_uses_session_game_speed() {
    let mut def = test_def(0, 6, 1, 3, LoopMode::Loop);
    def.normalized = true;

    let mut slow = GameOptions::default();
    slow.game_speed = 0;
    let mut slow_anim = Animation::new(SequenceKind::Idle1);
    for _ in 0..4 {
        advance_animation(&mut slow_anim, &def, &slow);
    }
    assert_eq!(slow_anim.frame_index, 0);
    advance_animation(&mut slow_anim, &def, &slow);
    assert_eq!(slow_anim.frame_index, 1);

    let mut fast = GameOptions::default();
    fast.game_speed = 7;
    let mut fast_anim = Animation::new(SequenceKind::Idle1);
    advance_animation(&mut fast_anim, &def, &fast);
    assert_eq!(fast_anim.frame_index, 1);
}

#[test]
fn test_advance_finished_does_nothing() {
    let def = test_def(86, 15, 1, 1, LoopMode::HoldLast);
    let mut anim: Animation = Animation::new(SequenceKind::Die1);
    anim.finished = true;
    anim.frame_index = 14;
    advance_animation(&mut anim, &def, &GameOptions::default());
    assert_eq!(anim.frame_index, 14);
}

// --- Animation component tests ---

#[test]
fn test_switch_resets_state() {
    let mut anim: Animation = Animation::new(SequenceKind::Walk);
    anim.frame_index = 3;
    anim.elapsed_frames = 1;
    anim.switch_to(SequenceKind::Stand);
    assert_eq!(anim.sequence, SequenceKind::Stand);
    assert_eq!(anim.frame_index, 0);
    assert_eq!(anim.elapsed_frames, 0);
    assert!(!anim.finished);
}

#[test]
fn test_switch_noop_same_sequence() {
    let mut anim: Animation = Animation::new(SequenceKind::Walk);
    anim.frame_index = 3;
    anim.elapsed_frames = 1;
    anim.switch_to(SequenceKind::Walk);
    assert_eq!(anim.frame_index, 3, "Same sequence should not reset");
    assert_eq!(anim.elapsed_frames, 1);
}

#[test]
fn test_animation_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Animation>();
}

// --- Default sequence set tests ---

#[test]
fn test_default_infantry_has_all_sequences() {
    let set: SequenceSet = default_infantry_sequences();
    assert!(set.get(&SequenceKind::Stand).is_some());
    assert!(set.get(&SequenceKind::Walk).is_some());
    assert!(set.get(&SequenceKind::Die1).is_some());
    assert!(set.get(&SequenceKind::Die2).is_some());
    assert!(set.get(&SequenceKind::Idle1).is_some());
    assert!(set.get(&SequenceKind::Idle2).is_some());
    assert_eq!(set.len(), 6);

    let walk: &SequenceDef = set.get(&SequenceKind::Walk).expect("Walk exists");
    assert_eq!(walk.start_frame, 8);
    assert_eq!(walk.frame_count, 6);
    assert_eq!(walk.facings, 8);
    assert_eq!(walk.facing_multiplier, 6);
}

#[test]
fn test_default_building_has_stand_only() {
    let set: SequenceSet = default_building_sequences();
    assert!(set.get(&SequenceKind::Stand).is_some());
    assert_eq!(set.len(), 1);
}

// --- death_sequence_for_inf_death tests ---

#[test]
fn gsi_08_13_death_sequence_and_anim_arms_are_exclusive() {
    use crate::sim::animation::inf_death_spawns_anim;
    // The jump table at 0x00518D58 dispatches on `InfDeath - 1` over 0..9.
    // 0 and > 10 fall off it entirely.
    assert_eq!(death_sequence_for_inf_death(0), None);
    assert!(!inf_death_spawns_anim(0));
    assert_eq!(death_sequence_for_inf_death(11), None);
    assert!(!inf_death_spawns_anim(11));
    // 1 and 2 are the only sequence arms, and they spawn no animation.
    assert_eq!(death_sequence_for_inf_death(1), Some(SequenceKind::Die1));
    assert_eq!(death_sequence_for_inf_death(2), Some(SequenceKind::Die2));
    assert!(!inf_death_spawns_anim(1));
    assert!(!inf_death_spawns_anim(2));
    // 3..=10 are animation arms, and they select no sequence — Die3, Die4 and
    // Die5 are unreachable from the death handler.
    for inf_death in 3..=10u8 {
        assert_eq!(death_sequence_for_inf_death(inf_death), None);
        assert!(inf_death_spawns_anim(inf_death));
    }
}

// --- tick_animations integration tests ---

fn make_test_interner() -> StringInterner {
    let mut interner = StringInterner::new();
    interner.intern("Americans");
    interner.intern("E1");
    interner
}

fn make_infantry_entity(id: u64, facing: u8, interner: &mut StringInterner) -> GameEntity {
    let mut e = GameEntity::new_at_frame_zero_for_test(
        id,
        0,
        0,
        0,
        facing,
        interner.intern("Americans"),
        Health { current: 100 },
        interner.intern("E1"),
        EntityCategory::Infantry,
        0,
        0,
        false,
    );
    e.animation = Some(Animation::new(SequenceKind::Stand));
    e
}

fn make_movement_target() -> MovementTarget {
    MovementTarget {
        path: vec![(0, 0), (1, 0)],
        path_layers: vec![MovementLayer::Ground; 2],
        next_index: 1,
        speed: SimFixed::from_num(512),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    }
}

#[test]
fn test_tick_switches_to_walk_with_movement() {
    let mut interner = make_test_interner();
    let mut store = EntityStore::new();
    let mut e = make_infantry_entity(1, 0, &mut interner);
    e.movement_target = Some(make_movement_target());
    store.insert(e);

    let mut sequences: BTreeMap<String, SequenceSet> = BTreeMap::new();
    sequences.insert("E1".to_string(), default_infantry_sequences());

    tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        0,
    );

    let anim = store.get(1).unwrap().animation.as_ref().unwrap();
    assert_eq!(anim.sequence, SequenceKind::Walk);
}

#[test]
fn test_tick_switches_to_stand_without_movement() {
    let mut interner = make_test_interner();
    let mut store = EntityStore::new();
    let mut e = make_infantry_entity(1, 0, &mut interner);
    e.animation = Some(Animation {
        sequence: SequenceKind::Walk,
        frame_index: 3,
        elapsed_frames: 0,
        finished: false,
    });
    store.insert(e);

    let mut sequences: BTreeMap<String, SequenceSet> = BTreeMap::new();
    sequences.insert("E1".to_string(), default_infantry_sequences());

    tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        0,
    );

    let anim = store.get(1).unwrap().animation.as_ref().unwrap();
    assert_eq!(anim.sequence, SequenceKind::Stand);
}

#[test]
fn test_tick_advances_walk_frame() {
    let mut interner = make_test_interner();
    let mut store = EntityStore::new();
    let mut e = make_infantry_entity(1, 64, &mut interner);
    e.animation = Some(Animation::new(SequenceKind::Walk));
    e.movement_target = Some(make_movement_target());
    store.insert(e);

    let mut sequences: BTreeMap<String, SequenceSet> = BTreeMap::new();
    sequences.insert("E1".to_string(), default_infantry_sequences());

    // Default walk delay is three reached native frames.
    for _ in 0..3 {
        tick_animations(
            &mut store,
            &sequences,
            &GameOptions::default(),
            &interner,
            0,
        );
    }

    let anim = store.get(1).unwrap().animation.as_ref().unwrap();
    assert_eq!(anim.sequence, SequenceKind::Walk);
    assert_eq!(anim.frame_index, 1);
}

#[test]
fn test_tick_attack_triggers_fire_animation() {
    let mut interner = make_test_interner();
    let mut store = EntityStore::new();
    let mut e = make_infantry_entity(1, 0, &mut interner);
    e.attack_target = Some(AttackTarget::new(999));
    e.attack_target.as_mut().unwrap().pending_infantry_fire = Some(PendingInfantryFire {
        sequence: SequenceKind::Attack,
        fire_frame: 2,
    });
    store.insert(e);

    // Build sequences that include Attack.
    let mut set = default_infantry_sequences();
    set.insert(
        SequenceKind::Attack,
        SequenceDef {
            start_frame: 164,
            frame_count: 6,
            facings: 8,
            facing_multiplier: 6,
            frame_delay: 1,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::TransitionTo(SequenceKind::Stand),
            facing_slots: FacingSlots::InfantryTable,
        },
    );
    let mut sequences: BTreeMap<String, SequenceSet> = BTreeMap::new();
    sequences.insert("E1".to_string(), set);

    tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        0,
    );

    let anim = store.get(1).unwrap().animation.as_ref().unwrap();
    assert_eq!(anim.sequence, SequenceKind::Attack);
}

#[test]
fn gsi_05_07_idle_completion_snaps_current_hint_before_stand_dispatch() {
    let mut interner = make_test_interner();
    let mut store = EntityStore::new();
    let mut entity = make_infantry_entity(1, 0, &mut interner);
    entity.animation = Some(Animation::new(SequenceKind::Idle1));
    entity.body_facing = Some(FacingClass::new(0, 4));
    store.insert(entity);

    let mut idle = test_def(56, 1, 1, 1, LoopMode::TransitionTo(SequenceKind::Stand));
    idle.completion_facing = Some(128);
    let mut stand = test_def(0, 1, 8, 1, LoopMode::Loop);
    // Proves completion reads the definition that just finished, not `next`.
    stand.completion_facing = Some(64);
    let mut set = SequenceSet::new();
    set.insert(SequenceKind::Idle1, idle);
    set.insert(SequenceKind::Stand, stand);
    let mut sequences = BTreeMap::new();
    sequences.insert("E1".to_string(), set);

    tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        77,
    );

    let entity = store.get(1).expect("entity");
    assert_eq!(
        entity.animation.as_ref().expect("animation").sequence,
        SequenceKind::Stand
    );
    assert_eq!(entity.facing, 128);
    let body = entity.body_facing.as_ref().expect("body facing");
    assert_eq!(body.destination(), 0x8000);
    assert_eq!(body.current(77), 0x8000);
    assert!(!body.is_rotating(77));
    assert_eq!(body.timer_start_frame(), Some(77));
}

#[test]
fn gsi_05_07_unhinted_completion_preserves_entity_and_body_facing() {
    let mut interner = make_test_interner();
    let mut store = EntityStore::new();
    let mut entity = make_infantry_entity(1, 32, &mut interner);
    entity.animation = Some(Animation::new(SequenceKind::Idle1));
    entity.body_facing = Some(FacingClass::new(0x2000, 4));
    store.insert(entity);

    let mut set = SequenceSet::new();
    set.insert(
        SequenceKind::Idle1,
        test_def(56, 1, 1, 1, LoopMode::TransitionTo(SequenceKind::Stand)),
    );
    set.insert(SequenceKind::Stand, test_def(0, 1, 8, 1, LoopMode::Loop));
    let mut sequences = BTreeMap::new();
    sequences.insert("E1".to_string(), set);

    tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        91,
    );

    let entity = store.get(1).expect("entity");
    assert_eq!(
        entity.animation.as_ref().expect("animation").sequence,
        SequenceKind::Stand
    );
    assert_eq!(entity.facing, 32);
    let body = entity.body_facing.as_ref().expect("body facing");
    assert_eq!(body.destination(), 0x2000);
    assert_eq!(body.current(91), 0x2000);
}

fn add_prone_sequences(set: &mut SequenceSet) {
    // Active retail [E1Sequence] layout. In particular, FireProne has six
    // frames and can reach E1's FireUp=2 discharge frame; a synthetic one-frame
    // transition would complete on this same reached native frame.
    for (kind, start_frame, frame_count, facing_multiplier, next) in [
        (SequenceKind::Prone, 86, 1, 6, None),
        (SequenceKind::Crawl, 86, 6, 6, None),
        (
            SequenceKind::FireProne,
            212,
            6,
            6,
            Some(SequenceKind::Prone),
        ),
        (SequenceKind::Down, 260, 2, 2, Some(SequenceKind::Prone)),
        (SequenceKind::Up, 276, 2, 2, Some(SequenceKind::Stand)),
    ] {
        set.insert(
            kind,
            SequenceDef {
                start_frame,
                frame_count,
                facings: 8,
                facing_multiplier,
                frame_delay: 1,
                normalized: false,
                completion_facing: None,
                loop_mode: next.map_or(LoopMode::Loop, LoopMode::TransitionTo),
                facing_slots: FacingSlots::InfantryTable,
            },
        );
    }
}

#[test]
fn test_runtime_prone_drives_prone_crawl_and_fireprone() {
    let mut interner = make_test_interner();
    let mut sequences = BTreeMap::new();
    let mut set = default_infantry_sequences();
    add_prone_sequences(&mut set);
    sequences.insert("E1".to_string(), set);

    let mut store = EntityStore::new();
    let mut idle = make_infantry_entity(1, 0, &mut interner);
    idle.infantry.as_mut().unwrap().is_prone = true;
    store.insert(idle);
    tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        0,
    );
    assert_eq!(
        store.get(1).unwrap().animation.as_ref().unwrap().sequence,
        SequenceKind::Prone
    );

    let mut moving = make_infantry_entity(2, 0, &mut interner);
    moving.infantry.as_mut().unwrap().is_prone = true;
    moving.movement_target = Some(make_movement_target());
    store.insert(moving);
    tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        0,
    );
    assert_eq!(
        store.get(2).unwrap().animation.as_ref().unwrap().sequence,
        SequenceKind::Crawl
    );

    let mut firing = make_infantry_entity(3, 0, &mut interner);
    firing.infantry.as_mut().unwrap().is_prone = true;
    firing.attack_target = Some(AttackTarget::new(999));
    firing.attack_target.as_mut().unwrap().pending_infantry_fire = Some(PendingInfantryFire {
        sequence: SequenceKind::FireProne,
        fire_frame: 2,
    });
    store.insert(firing);
    tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        0,
    );
    assert_eq!(
        store.get(3).unwrap().animation.as_ref().unwrap().sequence,
        SequenceKind::FireProne
    );
}

#[test]
fn test_down_and_up_transitions_are_preserved_until_complete() {
    let mut interner = make_test_interner();
    let mut store = EntityStore::new();
    let mut e = make_infantry_entity(1, 0, &mut interner);
    e.infantry.as_mut().unwrap().is_prone = true;
    e.animation = Some(Animation::new(SequenceKind::Down));
    e.movement_target = Some(make_movement_target());
    store.insert(e);

    let mut set = default_infantry_sequences();
    set.insert(
        SequenceKind::Down,
        SequenceDef {
            start_frame: 200,
            frame_count: 3,
            facings: 8,
            facing_multiplier: 3,
            frame_delay: 1,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::TransitionTo(SequenceKind::Prone),
            facing_slots: FacingSlots::InfantryTable,
        },
    );
    set.insert(
        SequenceKind::Prone,
        SequenceDef {
            start_frame: 210,
            frame_count: 1,
            facings: 8,
            facing_multiplier: 1,
            frame_delay: 1,
            normalized: false,
            completion_facing: None,
            loop_mode: LoopMode::Loop,
            facing_slots: FacingSlots::InfantryTable,
        },
    );
    let mut sequences = BTreeMap::new();
    sequences.insert("E1".to_string(), set);

    tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        0,
    );
    assert_eq!(
        store.get(1).unwrap().animation.as_ref().unwrap().sequence,
        SequenceKind::Down
    );
    for _ in 0..3 {
        tick_animations(
            &mut store,
            &sequences,
            &GameOptions::default(),
            &interner,
            0,
        );
    }
    assert_eq!(
        store.get(1).unwrap().animation.as_ref().unwrap().sequence,
        SequenceKind::Prone
    );
}

#[test]
fn test_tick_dying_entity_skips_transitions() {
    let mut interner = make_test_interner();
    let mut store = EntityStore::new();
    let mut e = make_infantry_entity(1, 0, &mut interner);
    e.dying = true;
    e.animation = Some(Animation::new(SequenceKind::Die1));
    e.movement_target = Some(make_movement_target());
    store.insert(e);

    let mut sequences: BTreeMap<String, SequenceSet> = BTreeMap::new();
    sequences.insert("E1".to_string(), default_infantry_sequences());

    let dead = tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        0,
    );

    // Dying entity should NOT switch to Walk despite having movement_target.
    let anim = store.get(1).unwrap().animation.as_ref().unwrap();
    assert_eq!(anim.sequence, SequenceKind::Die1);
    // One reached native frame is not enough to finish the death sequence.
    assert!(dead.is_empty());
}

#[test]
fn test_tick_dying_entity_returns_finished_id() {
    let mut interner = make_test_interner();
    let mut store = EntityStore::new();
    let mut e = make_infantry_entity(1, 0, &mut interner);
    e.dying = true;
    e.animation = Some(Animation {
        sequence: SequenceKind::Die1,
        frame_index: 14,
        elapsed_frames: 0,
        finished: true,
    });
    store.insert(e);

    let mut sequences: BTreeMap<String, SequenceSet> = BTreeMap::new();
    sequences.insert("E1".to_string(), default_infantry_sequences());

    let dead = tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        0,
    );
    assert_eq!(dead, vec![1]);
}

#[test]
fn test_tick_dying_entity_returns_id_on_finishing_visit() {
    let mut interner = make_test_interner();
    let mut store = EntityStore::new();
    let mut e = make_infantry_entity(1, 0, &mut interner);
    e.dying = true;
    e.animation = Some(Animation {
        sequence: SequenceKind::Die1,
        frame_index: 14,
        elapsed_frames: 0,
        finished: false,
    });
    store.insert(e);

    let mut sequences: BTreeMap<String, SequenceSet> = BTreeMap::new();
    sequences.insert("E1".to_string(), default_infantry_sequences());

    let dead = tick_animations(
        &mut store,
        &sequences,
        &GameOptions::default(),
        &interner,
        0,
    );
    assert_eq!(dead, vec![1]);
    assert!(store.get(1).unwrap().animation.as_ref().unwrap().finished);
}

#[test]
fn test_sequence_is_prone_helper() {
    assert!(sequence_is_prone(SequenceKind::Prone));
    assert!(sequence_is_prone(SequenceKind::Crawl));
    assert!(sequence_is_prone(SequenceKind::FireProne));
    assert!(sequence_is_prone(SequenceKind::Down));
    assert!(!sequence_is_prone(SequenceKind::Stand));
    assert!(!sequence_is_prone(SequenceKind::Up));
}

fn animation_catalog_rules(ready_start: u16) -> RuleSet {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\
         [VehicleTypes]\n0=DRON\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAPOWR\n\
         [E1]\nImage=GI\nStrength=100\n\
         [DRON]\nStrength=100\nWalkRate=3\nIdleRate=2\n\
         [GAPOWR]\nStrength=100\n",
    ))
    .expect("animation catalog rules");
    let art_ini = IniFile::from_str(&format!(
        "[GI]\nSequence=GISequence\n\
         [GISequence]\nReady={ready_start},1,8\nWalk=8,6,8\nDie1=56,2,1\n\
         [DRON]\nVoxel=no\nWalkFrames=6\nFiringFrames=4\n",
    ));
    rules.merge_art_data(&ArtRegistry::from_ini(&art_ini));
    rules.bind_animation_sequences(&parse_infantry_sequence_registry(&art_ini));
    rules
}

#[test]
fn animation_catalog_covers_unspawned_registered_shp_types() {
    let rules = animation_catalog_rules(3);

    let infantry = rules.animation_sequence("e1").expect("registered infantry");
    assert_eq!(
        infantry
            .get(&SequenceKind::Stand)
            .expect("infantry stand")
            .start_frame,
        3,
    );
    assert_eq!(
        infantry
            .get(&SequenceKind::Die1)
            .expect("infantry death")
            .frame_count,
        2,
    );

    let vehicle = rules
        .animation_sequence("dron")
        .expect("registered SHP vehicle");
    assert_eq!(
        vehicle
            .get(&SequenceKind::Walk)
            .expect("vehicle walk")
            .frame_count,
        6,
    );
    assert_eq!(
        vehicle
            .get(&SequenceKind::Attack)
            .expect("vehicle attack")
            .frame_count,
        4,
    );
    assert_eq!(
        vehicle.shp_vehicle_cadence(),
        Some(ShpVehicleCadence {
            walk_rate: 3,
            idle_rate: 2,
        }),
    );

    assert!(
        rules
            .animation_sequence("gapowr")
            .and_then(|set| set.get(&SequenceKind::Stand))
            .is_some(),
        "registered buildings receive their authoritative default set before any spawn",
    );
}

#[test]
fn simulation_config_hash_changes_with_resolved_sequence_layout() {
    let first = animation_catalog_rules(3);
    let second = animation_catalog_rules(11);

    assert_eq!(first.source_ini_hash(), second.source_ini_hash());
    assert_ne!(
        first.simulation_config_hash(),
        second.simulation_config_hash()
    );
}
