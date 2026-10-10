//! Tests for the sprite animation system.
//!
//! Separated from animation.rs to stay within the 400-line file limit.

use crate::rules::art_data::ArtRegistry;
use crate::rules::infantry_sequence::parse_infantry_sequence_registry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::*;
use crate::sim::combat::AttackTarget;
use crate::sim::components::{DriveCoord, MovementTarget};
use crate::sim::game_entity::GameEntity;
use crate::sim::game_options::GameOptions;
use crate::sim::intern::StringInterner;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::sim::movement::{FacingClass, SpeedRules};
use crate::sim::type_handle_table::TypeHandleTable;
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

fn gsi_13_06_active_shp_unit(type_name: &str, kind: LocomotorKind) -> GameEntity {
    let mut entity = GameEntity::test_default(13_006, type_name, "Soviet", 5, 5);
    entity.is_voxel = false;
    entity.lifecycle.in_limbo = false;
    entity.locomotor = Some(LocomotorState::for_test_kind(kind));
    let head = DriveCoord::cell(6, 5, 0);
    match kind {
        LocomotorKind::Drive => {
            entity.foot_speed.set_speed_fraction(SimFixed::from_num(1));
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
        }
        LocomotorKind::Ship => {
            entity.foot_speed.set_speed_fraction(SimFixed::from_num(1));
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
        }
        _ => unreachable!("stock SHP vehicle fixture uses Drive or Ship"),
    }
    entity.movement_target = Some(make_movement_target());
    entity
}

/// The inputs the live GetCurrentSpeed reads for the SHP fixtures: each type's
/// `Speed=`, resolved by name through an unbuilt handle table.
struct Gsi1306Speed {
    rules: RuleSet,
    interner: StringInterner,
    types: TypeHandleTable,
    houses: std::collections::BTreeMap<
        crate::sim::intern::InternedId,
        crate::sim::house_state::HouseState,
    >,
}

impl Gsi1306Speed {
    fn new() -> Self {
        for name in ["DLPH", "SQD", "DRON"] {
            crate::sim::intern::test_intern(name);
        }
        Self {
            rules: RuleSet::from_ini(&IniFile::from_str(
                "[VehicleTypes]\n0=DLPH\n1=SQD\n2=DRON\n\
                 [DLPH]\nSpeed=8\n[SQD]\nSpeed=8\n[DRON]\nSpeed=10\n",
            ))
            .expect("SHP fixture rules"),
            interner: crate::sim::intern::test_interner(),
            types: TypeHandleTable::default(),
            houses: std::collections::BTreeMap::new(),
        }
    }

    fn rules(&self) -> Option<SpeedRules<'_>> {
        Some(SpeedRules::new(
            &self.rules,
            &self.interner,
            &self.types,
            &self.houses,
        ))
    }
}

#[test]
fn gsi_13_06_body_counter_uses_absolute_precommit_binary_frame_phase() {
    let cadence = ShpVehicleCadence {
        walk_rate: 4,
        idle_rate: 8,
    };

    let speed = Gsi1306Speed::new();
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut entity = gsi_13_06_active_shp_unit("DRON", kind);
        tick_unit_body_frame_counter(&mut entity, speed.rules(), cadence, false, false, 3);
        assert_eq!(entity.body_frame_counter, 0);
        tick_unit_body_frame_counter(&mut entity, speed.rules(), cadence, false, false, 4);
        assert_eq!(entity.body_frame_counter, 1);
        tick_unit_body_frame_counter(&mut entity, speed.rules(), cadence, false, false, 5);
        assert_eq!(entity.body_frame_counter, 1);
        tick_unit_body_frame_counter(&mut entity, speed.rules(), cadence, false, false, 8);
        assert_eq!(entity.body_frame_counter, 2);
    }
}

#[test]
fn gsi_13_06_body_counter_wraps_and_survives_moving_idle_transitions() {
    let speed = Gsi1306Speed::new();
    let mut entity = gsi_13_06_active_shp_unit("DRON", LocomotorKind::Drive);
    entity.body_frame_counter = u32::MAX;
    tick_unit_body_frame_counter(
        &mut entity,
        speed.rules(),
        ShpVehicleCadence {
            walk_rate: 1,
            idle_rate: 8,
        },
        false,
        false,
        4,
    );
    assert_eq!(entity.body_frame_counter, 0, "native dword wraps");

    entity.body_frame_counter = 9;
    if let Some(drive) = entity
        .locomotor
        .as_mut()
        .filter(|l| l.has_track_state(crate::sim::movement::track_process::TrackFamily::Drive))
    {
        assert!(drive.store_track_destination(
            crate::sim::movement::track_process::TrackFamily::Drive,
            None
        ));
        assert!(drive.store_track_head(
            crate::sim::movement::track_process::TrackFamily::Drive,
            None
        ));
        entity.foot_speed.set_speed_fraction(SIM_ZERO);
    }
    tick_unit_body_frame_counter(
        &mut entity,
        speed.rules(),
        ShpVehicleCadence {
            walk_rate: 4,
            idle_rate: 8,
        },
        false,
        false,
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
    let speed = Gsi1306Speed::new();
    let base = gsi_13_06_active_shp_unit("DRON", LocomotorKind::Drive);
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
    falling.set_falling_down_for_test(true);
    variants.push(("falling", falling));
    let mut warp_out = base.clone();
    // BeingWarpedOut (`+0x270`): a Temporal chain's head.
    warp_out.temporal = crate::sim::temporal::TemporalState::warped_by_for_test(99);
    variants.push(("warp out", warp_out));
    let mut warp_in = base.clone();
    warp_in.install_teleport_state_for_test(Some(TeleportState::for_test(
        TeleportPhase::ChronoDelay,
        8,
        8,
        1,
    )));
    variants.push(("warp in", warp_in));
    let mut swapped = base.clone();
    swapped.foot_locomotor_swap_active = true;
    variants.push(("locomotor swap", swapped));

    for (name, mut entity) in variants {
        entity.body_frame_counter = 17;
        tick_unit_body_frame_counter(&mut entity, speed.rules(), cadence, false, false, 1);
        assert_eq!(entity.body_frame_counter, 17, "{name}");
    }
}

#[test]
fn gsi_13_06_draw_and_cadence_use_distinct_movement_predicates() {
    let speed = Gsi1306Speed::new();
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut entity = gsi_13_06_active_shp_unit("DRON", kind);
        // No applied speed: GetCurrentSpeed reads 0 with the destination set.
        entity.foot_speed.set_speed_fraction(SIM_ZERO);

        assert!(
            crate::sim::movement::motion_query::is_moving(&entity) == Some(true),
            "slot-4 Is_Moving sees the class-owned destination"
        );
        assert!(
            !crate::sim::movement::motion_query::is_moving_now(&entity, speed.rules(), 4),
            "slot-32 Is_Moving_Now also requires positive applied speed"
        );
        tick_unit_body_frame_counter(
            &mut entity,
            speed.rules(),
            ShpVehicleCadence {
                walk_rate: 4,
                idle_rate: 0,
            },
            false,
            false,
            4,
        );
        assert_eq!(entity.body_frame_counter, 0, "IdleRate=0 holds the counter");
    }
}

#[test]
fn gsi_13_06_positive_fraction_below_get_current_speed_threshold_is_idle() {
    let speed = Gsi1306Speed::new();
    for (name, kind) in [
        ("DLPH", LocomotorKind::Ship),
        ("SQD", LocomotorKind::Ship),
        ("DRON", LocomotorKind::Drive),
    ] {
        let mut entity = gsi_13_06_active_shp_unit(name, kind);
        entity.foot_speed.set_speed_fraction(SimFixed::lit("0.03"));

        assert!(
            crate::sim::movement::motion_query::is_moving(&entity) == Some(true),
            "{name} slot +0x10 still sees its locomotor destination"
        );
        assert!(
            !crate::sim::movement::motion_query::is_moving_now(&entity, speed.rules(), 1),
            "{name} slot +0x80 requires truncated GetCurrentSpeed > 0"
        );
        tick_unit_body_frame_counter(
            &mut entity,
            speed.rules(),
            ShpVehicleCadence {
                walk_rate: 1,
                idle_rate: 0,
            },
            false,
            false,
            1,
        );
        assert_eq!(entity.body_frame_counter, 0, "{name} remains idle");

        entity.foot_speed.set_speed_fraction(SimFixed::from_num(1));
        assert!(
            crate::sim::movement::motion_query::is_moving_now(&entity, speed.rules(), 1),
            "{name} moves at the full fraction"
        );
    }
}

#[test]
fn gsi_13_06_shp_movement_predicates_ignore_path_execution_surrogates() {
    let speed = Gsi1306Speed::new();
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut entity = gsi_13_06_active_shp_unit("DRON", kind);
        let owner = DriveCoord::cell(5, 5, 0);

        // A live MovementTarget is not either native class coordinate. With a
        // null destination and an equal-X/Y head, both slots answer false even
        // though the execution adapter still reports a positive speed.
        match kind {
            LocomotorKind::Drive => {
                entity.foot_speed.set_speed_fraction(SimFixed::from_num(1));
                assert!(
                    entity
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .install_drive_state_for_test(Some(
                            DriveLocomotionRuntime::default().with_head_to_for_test(Some(owner))
                        ))
                );
            }
            LocomotorKind::Ship => {
                entity.foot_speed.set_speed_fraction(SimFixed::from_num(1));
                assert!(
                    entity
                        .locomotor
                        .as_mut()
                        .unwrap()
                        .install_ship_state_for_test(Some(
                            ShipLocomotionRuntime::default().with_head_to_for_test(Some(owner))
                        ))
                );
            }
            _ => unreachable!(),
        }
        assert_eq!(
            crate::sim::movement::motion_query::is_moving(&entity),
            Some(false)
        );
        assert!(!crate::sim::movement::motion_query::is_moving_now(
            &entity,
            speed.rules(),
            4
        ));

        // Conversely, locomotor-owned state alone is sufficient; no
        // MovementTarget is needed by either draw or cadence. The live
        // GetCurrentSpeed reads DRON's type speed.
        entity.movement_target = None;
        let head = DriveCoord::cell(6, 5, 0);
        match kind {
            LocomotorKind::Drive => {
                entity.foot_speed.set_speed_fraction(SimFixed::from_num(1));
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
            }
            LocomotorKind::Ship => {
                entity.foot_speed.set_speed_fraction(SimFixed::from_num(1));
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
            }
            _ => unreachable!(),
        }
        assert!(crate::sim::movement::motion_query::is_moving(&entity) == Some(true));
        assert!(crate::sim::movement::motion_query::is_moving_now(
            &entity,
            speed.rules(),
            4
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
        resolve_shp_vehicle_body_frame(&dlph, 0, 7, false, false),
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
        resolve_shp_vehicle_body_frame(&sqd, 0, 3, false, false),
        Some(23),
        "idle Squid uses walk/swim slot 1 and counter 3"
    );

    let dron = gsi_13_06_shp_set(6, 48, ShpVehicleCadence::native_defaults());
    assert_eq!(
        resolve_shp_vehicle_body_frame(&dron, 0, 99, false, false),
        Some(49),
        "idle Terror Drone with IdleRate=0 uses standing slot 1"
    );
    assert_eq!(
        resolve_shp_vehicle_body_frame(&dron, 0, 7, true, false),
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

// --- Infantry class action and presentation integration tests ---

/// Production rules and ART readers establish the action records; the class
/// owners use them directly. GI layouts supply the counts; the completed idle
/// hint and a different Ready hint are authored completion controls below.
fn infantry_action_fixture(
    idle_hint: Option<&str>,
) -> (crate::sim::world::Simulation, RuleSet, u64) {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\
         [E1]\nStrength=125\nSpeed=4\nImage=GI\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Infantry\n",
    );
    let hint = idle_hint.map_or(String::new(), |hint| format!(",{hint}"));
    let art = IniFile::from_str(&format!(
        "[GI]\nCrawls=yes\nSequence=GISequence\n\
         [GISequence]\nReady=0,1,1,E\nGuard=0,1,1\nProne=86,1,6\nWalk=8,6,6\n\
         FireUp=164,6,6\nDown=260,2,2\nCrawl=86,6,6\nUp=276,2,2\nFireProne=212,6,6\n\
         Idle1=56,15,0{hint}\nIdle2=71,15,0,E\nDie1=134,15,0\nDie2=149,15,0\n",
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(&parse_infantry_sequence_registry(&art));
    assert_eq!(
        rules
            .animation_sequence("E1")
            .unwrap()
            .infantry_action(11)
            .unwrap()
            .frames_per_facing,
        15,
        "the declared Die1 record must reach the gameplay owner"
    );
    let mut sim = crate::sim::world::Simulation::with_seed(0);
    let house = sim.interner.intern("Americans");
    sim.houses.insert(
        house,
        crate::sim::house_state::HouseState::new(house, 0, None, true, 0, 10),
    );
    let id = sim
        .construct_object_limbo_at_height("E1", "Americans", 10, 10, 0, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .lifecycle
        .in_limbo = false;
    assert!(sim.infantry_do_action(id, 0, false, &rules).unwrap());
    (sim, rules, id)
}

fn make_movement_target() -> MovementTarget {
    MovementTarget {
        speed: SimFixed::from_num(512),
        ..Default::default()
    }
}

/// Install the retained Walk +0x36 byte that `Is_Really_Moving_Now` answers
/// (`0x0075CB20`): a moving man's head went through Process `0x0075BD25`; a
/// stopped man was Stopped with no head (`0x0075ADEC`). A MovementTarget
/// alone cannot select the Infantry locomotion action.
fn set_infantry_walk_motion(sim: &mut crate::sim::world::Simulation, id: u64, moving: bool) {
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    let locomotor = actor.locomotor.as_mut().unwrap();
    let coord = moving.then(|| DriveCoord::cell(11, 10, 0));
    locomotor.set_step_head(coord);
    locomotor.set_walk_destination(coord);
    if moving {
        locomotor.begin_walk_motion();
    } else {
        locomotor.stop_walk();
    }
    actor
        .foot_speed
        .set_speed_fraction(if moving { SimFixed::ONE } else { SIM_ZERO });
}

fn assert_infantry_pose(
    sim: &crate::sim::world::Simulation,
    id: u64,
    doing: i32,
    kind: SequenceKind,
    stage: i32,
) {
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(actor.mission_leaf.as_infantry().unwrap().doing(), doing);
    assert_eq!(actor.native_stage().value(), stage);
    assert_eq!(actor.infantry_sprite_pose(), Some((doing, stage)));
    assert_eq!(
        crate::rules::infantry_sequence::action_kind(doing),
        Some(kind)
    );
    assert!(
        actor.animation.is_none(),
        "Infantry has no writable presentation clock"
    );
}

#[test]
fn infantry_walk_action_uses_retained_locomotor_motion() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    sim.substrate.entities.get_mut(id).unwrap().movement_target = Some(make_movement_target());
    sim.infantry_movement_actions(id, &rules, None);
    assert_infantry_pose(&sim, id, 0, SequenceKind::Stand, 0);

    set_infantry_walk_motion(&mut sim, id, true);
    sim.infantry_movement_actions(id, &rules, None);
    assert_infantry_pose(&sim, id, 3, SequenceKind::Walk, 0);
}

/// `InfantryClass::Limbo @ 0x0051DF10` stops the locomotor's movement
/// animation (`0x0051DF30`, Walk `0x0075CBC0` clears +0x36) and stores the
/// water state's constructor sentinel (`0x0051DF38`) beside its prone and
/// Doing stores, so the next locomotion action after Unlimbo starts from Ready.
#[test]
fn infantry_limbo_stops_walk_animation_and_resets_water_state() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    set_infantry_walk_motion(&mut sim, id, true);
    sim.infantry_movement_actions(id, &rules, None);
    assert_infantry_pose(&sim, id, 3, SequenceKind::Walk, 0);
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.mission_leaf.install_infantry_water_state_fixture(1);
    actor.infantry.as_mut().unwrap().is_prone = true;
    let _ = sim.techno_limbo(id);
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(
        actor.locomotor.as_ref().unwrap().walk_animation_moving(),
        Some(false)
    );
    let leaf = actor.mission_leaf.as_infantry().unwrap();
    assert_eq!(leaf.water_state(), 2);
    assert_eq!(leaf.doing(), 0);
    assert!(!actor.infantry.as_ref().unwrap().is_prone);
}

#[test]
fn infantry_stopped_walk_returns_to_ready_through_class_action() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    set_infantry_walk_motion(&mut sim, id, true);
    sim.infantry_movement_actions(id, &rules, None);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(3);
    set_infantry_walk_motion(&mut sim, id, false);
    sim.infantry_movement_actions(id, &rules, None);
    assert_infantry_pose(&sim, id, 0, SequenceKind::Stand, 0);
}

#[test]
fn infantry_walk_stage_uses_absolute_frames_and_presentation_leaves_it_alone() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    set_infantry_walk_motion(&mut sim, id, true);
    sim.infantry_movement_actions(id, &rules, None);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .native_stage()
            .rate(),
        3
    );
    for now in [0, 1, 2, 2] {
        assert!(
            !sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .tick_native_stage(now)
        );
    }
    assert!(
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .tick_native_stage(3)
    );
    assert!(
        !sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .tick_native_stage(3)
    );
    assert_infantry_pose(&sim, id, 3, SequenceKind::Walk, 1);
    for now in [3, 3, 100] {
        let dead = tick_animations(
            &mut sim.substrate.entities,
            rules.animation_sequences(),
            &sim.session.game_options,
            &sim.interner,
            now,
        );
        assert!(dead.is_empty());
        assert_infantry_pose(&sim, id, 3, SequenceKind::Walk, 1);
    }
}

#[test]
fn presentation_preserves_infantry_fire_action_latch_and_signed_stage() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    assert!(sim.infantry_do_action(id, 4, false, &rules).unwrap());
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.attack_target = Some(AttackTarget::new(999));
    actor.mission_leaf.set_foot_firing_sequence(1);
    actor.set_native_stage_value(65_538);
    let facing = actor.body_facing;
    let dead = tick_animations(
        &mut sim.substrate.entities,
        rules.animation_sequences(),
        &sim.session.game_options,
        &sim.interner,
        100,
    );
    assert!(dead.is_empty());
    assert_infantry_pose(&sim, id, 4, SequenceKind::Attack, 65_538);
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(actor.mission_leaf.foot_firing_sequence_latch(), 1);
    assert_eq!(
        actor.attack_target.as_ref().unwrap().target,
        crate::sim::combat::TargetKind::Entity(999)
    );
    assert_eq!(actor.body_facing, facing);
}

#[test]
fn gsi_05_07_idle_completion_snaps_current_hint_before_ready_dispatch() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    sim.session.binary_frame = 77;
    sim.substrate.entities.get_mut(id).unwrap().body_facing = FacingClass::new(0, 4);
    assert!(sim.infantry_do_action(id, 9, false, &rules).unwrap());
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(15);
    assert!(!sim.infantry_sequencer(id, &rules));
    assert_infantry_pose(&sim, id, 0, SequenceKind::Stand, 0);
    let body = sim.substrate.entities.get(id).unwrap().body_facing;
    assert_eq!(body.destination(), 0x8000);
    assert_eq!(body.current(77), 0x8000);
    assert!(!body.is_rotating(77));
    assert_eq!(body.timer_start_frame(), Some(77));
}

#[test]
fn gsi_05_07_unhinted_completion_preserves_body_facing() {
    let (mut sim, rules, id) = infantry_action_fixture(None);
    sim.session.binary_frame = 91;
    sim.substrate.entities.get_mut(id).unwrap().body_facing = FacingClass::new(0x2000, 4);
    assert!(sim.infantry_do_action(id, 9, false, &rules).unwrap());
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(15);
    assert!(!sim.infantry_sequencer(id, &rules));
    assert_infantry_pose(&sim, id, 0, SequenceKind::Stand, 0);
    let body = sim.substrate.entities.get(id).unwrap().body_facing;
    assert_eq!(body.destination(), 0x2000);
    assert_eq!(body.current(91), 0x2000);
}

#[test]
fn infantry_prone_state_drives_prone_crawl_and_fireprone_actions() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .infantry
        .as_mut()
        .unwrap()
        .is_prone = true;
    assert!(sim.infantry_do_action(id, 2, false, &rules).unwrap());
    assert_infantry_pose(&sim, id, 2, SequenceKind::Prone, 0);
    set_infantry_walk_motion(&mut sim, id, true);
    sim.infantry_movement_actions(id, &rules, None);
    assert_infantry_pose(&sim, id, 6, SequenceKind::Crawl, 0);
    set_infantry_walk_motion(&mut sim, id, false);
    sim.infantry_movement_actions(id, &rules, None);
    assert_infantry_pose(&sim, id, 2, SequenceKind::Prone, 0);
    assert!(sim.infantry_do_action(id, 8, false, &rules).unwrap());
    assert_infantry_pose(&sim, id, 8, SequenceKind::FireProne, 0);
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .infantry
            .as_ref()
            .unwrap()
            .is_prone
    );
}

#[test]
fn infantry_down_and_up_remain_uninterruptible_until_sequence_completion() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    assert!(sim.infantry_do_action(id, 5, false, &rules).unwrap());
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .infantry
            .as_ref()
            .unwrap()
            .is_prone
    );
    assert!(!sim.infantry_do_action(id, 3, false, &rules).unwrap());
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(1);
    assert!(!sim.infantry_sequencer(id, &rules));
    assert_infantry_pose(&sim, id, 5, SequenceKind::Down, 1);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(2);
    assert!(!sim.infantry_sequencer(id, &rules));
    assert_infantry_pose(&sim, id, 2, SequenceKind::Prone, 0);

    assert!(sim.infantry_do_action(id, 7, false, &rules).unwrap());
    assert!(
        !sim.substrate
            .entities
            .get(id)
            .unwrap()
            .infantry
            .as_ref()
            .unwrap()
            .is_prone
    );
    assert!(!sim.infantry_do_action(id, 3, false, &rules).unwrap());
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(1);
    assert!(!sim.infantry_sequencer(id, &rules));
    assert_infantry_pose(&sim, id, 7, SequenceKind::Up, 1);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(2);
    assert!(!sim.infantry_sequencer(id, &rules));
    assert_infantry_pose(&sim, id, 0, SequenceKind::Stand, 0);
}

#[test]
fn presentation_skips_dying_infantry_without_changing_its_stage_or_action() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    assert!(sim.infantry_do_action(id, 11, true, &rules).unwrap());
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.dying = true;
    actor.health.current = 0;
    actor.movement_target = Some(make_movement_target());
    actor.set_native_stage_value(14);
    let dead = tick_animations(
        &mut sim.substrate.entities,
        rules.animation_sequences(),
        &sim.session.game_options,
        &sim.interner,
        100,
    );
    assert!(dead.is_empty());
    assert_infantry_pose(&sim, id, 11, SequenceKind::Die1, 14);
}

#[test]
fn completed_infantry_death_is_removed_by_the_class_sequencer() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    assert!(sim.infantry_do_action(id, 11, true, &rules).unwrap());
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.dying = true;
    actor.health.current = 0;
    actor.set_native_stage_value(15);
    assert!(sim.infantry_sequencer(id, &rules));
    let retired = sim.substrate.entities.get(id).unwrap();
    assert!(!retired.lifecycle.object_alive);
    assert!(retired.lifecycle.in_limbo);
    assert!(sim.substrate.pending_delete.contains(&id));
    sim.process_pending_delete_with(Some(&rules), None);
    assert!(sim.substrate.entities.get(id).is_none());
}

#[test]
fn infantry_death_finishes_at_its_sequence_count_and_not_one_frame_early() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    assert!(sim.infantry_do_action(id, 11, true, &rules).unwrap());
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.dying = true;
    actor.health.current = 0;
    actor.set_native_stage_value(14);
    assert_eq!(
        (
            actor.mission_leaf.as_infantry().unwrap().doing(),
            actor.native_stage().value(),
            sim.interner.resolve(actor.type_ref()),
            rules
                .animation_sequence(sim.interner.resolve(actor.type_ref()))
                .unwrap()
                .infantry_action(11)
                .unwrap()
                .frames_per_facing,
        ),
        (11, 14, "E1", 15),
        "the supplied death pose and the bound action record must agree",
    );
    assert!(!sim.infantry_sequencer(id, &rules));
    assert_infantry_pose(&sim, id, 11, SequenceKind::Die1, 14);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(15);
    assert!(sim.infantry_sequencer(id, &rules));
    let retired = sim.substrate.entities.get(id).unwrap();
    assert!(!retired.lifecycle.object_alive);
    assert!(retired.lifecycle.in_limbo);
    assert!(sim.substrate.pending_delete.contains(&id));
    sim.process_pending_delete_with(Some(&rules), None);
    assert!(sim.substrate.entities.get(id).is_none());
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
    let rules_ini = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\
         [VehicleTypes]\n0=DRON\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n0=GAPOWR\n\
         [E1]\nImage=GI\nStrength=100\n\
         [DRON]\nStrength=100\nWalkRate=3\nIdleRate=2\n\
         [GAPOWR]\nStrength=100\n",
    );
    let art_ini = IniFile::from_str(&format!(
        "[GI]\nSequence=GISequence\n\
         [GISequence]\nReady={ready_start},1,8\nWalk=8,6,8\nDie1=56,2,1\n\
         [DRON]\nVoxel=no\nWalkFrames=6\nFiringFrames=4\n",
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini)
        .expect("animation catalog rules with fixed ART supplied to UnitType reads");
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
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
fn mirage_voxel_unit_shp_layout_does_not_create_a_second_stand_walk_clock() {
    let rules_ini = IniFile::from_str(
        "[VehicleTypes]\n0=ACTOR\n[ACTOR]\nImage=IMAGE\nStrength=100\nWalkRate=3\nIdleRate=4\n",
    );
    let art = IniFile::from_str("[IMAGE]\nVoxel=yes\nFacings=8\nWalkFrames=6\nStandingFrames=1\n");
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(&parse_infantry_sequence_registry(&art));
    assert!(
        rules.animation_sequence("ACTOR").is_some(),
        "a Unit keeps its actual immutable SHP layout even when its normal body is voxel"
    );
    let mut sim = crate::sim::world::Simulation::new();
    let id = sim
        .spawn_object_limbo_at_height("ACTOR", "Americans", 3, 3, 0, 0, &rules)
        .unwrap();
    for (sequence, moving) in [(SequenceKind::Stand, false), (SequenceKind::Walk, true)] {
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        assert!(actor.is_voxel);
        actor.body_frame_counter = 17;
        actor.animation = Some(Animation {
            sequence,
            frame_index: 3,
            elapsed_frames: 2,
            finished: false,
        });
        actor.movement_target = moving.then(make_movement_target);
        for now in [0, 4, 100] {
            assert!(
                tick_animations(
                    &mut sim.substrate.entities,
                    rules.animation_sequences(),
                    &sim.session.game_options,
                    &sim.interner,
                    now,
                )
                .is_empty()
            );
            let actor = sim.substrate.entities.get(id).unwrap();
            let animation = actor.animation.as_ref().unwrap();
            assert_eq!(
                (
                    animation.sequence,
                    animation.frame_index,
                    animation.elapsed_frames
                ),
                (sequence, 3, 2)
            );
            assert_eq!(
                actor.body_frame_counter, 17,
                "only the reached Foot AI owns this counter"
            );
        }
    }
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

#[test]
fn raw_infantry_frames_match_whole_original_selector_rows() {
    // Whole518D80 outputs, not a Rust or hand-calculated frame golden. The
    // ordinary physical GI bank is independently bound through production
    // rules/ART readers. Disguise selection, Jumpjet target-facing, rotating
    // FacingClass and downstream invalid-index shape access are excluded.
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(&parse_infantry_sequence_registry(&art));
    let set = rules.animation_sequence("E1").unwrap();
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anytown_damage/foot_missions.json",
    ))
    .unwrap();
    let receipt = &corpus["infantry_frame_selection_receipt"];
    assert_eq!(
        receipt["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    assert_eq!(
        receipt["native_span"]["sha256"],
        "60ee1cdafdd13ab060a3fc6fc80794476f42f943d933090b63d41ced74714608"
    );
    assert_eq!(
        receipt["preserved_payload"]["canonical_sha256"],
        "54a2a1f2b554a404ed0e6530b4dc2bdea0f666584bf03f1ed8f873bb45a28e91"
    );
    let records = receipt["physical_gi_sequence"]["records"]
        .as_array()
        .unwrap();
    assert_eq!(records.len(), 42);
    for row in records {
        let doing = row["doing"].as_i64().unwrap() as i32;
        let record = set.infantry_action(doing).unwrap();
        let words = &row["words_i32"];
        assert_eq!(
            record.start_frame,
            words[0].as_i64().unwrap() as i32,
            "Doing{doing}"
        );
        assert_eq!(
            record.frames_per_facing,
            words[1].as_i64().unwrap() as i32,
            "Doing{doing}"
        );
        assert_eq!(
            record.facings,
            words[2].as_i64().unwrap() as i32,
            "Doing{doing}"
        );
    }
    let mut compared = 0;
    for (group, count) in [
        ("physical_rows", 336),
        ("facing_rows", 40),
        ("scalar_rows", 136),
        ("default_rows", 4),
    ] {
        let rows = receipt[group].as_array().unwrap();
        assert_eq!(rows.len(), count, "{group}");
        for row in rows {
            let input = &row["input"];
            let doing = input["doing"].as_i64().unwrap() as i32;
            // Whole original default-action selection is a recorded premise
            // here; production drawing reads the live map through its Cell
            // query, while this comparison isolates the shared arithmetic.
            let selected = if doing == -1 {
                if row["before"]["cell_land_type"] == 2 && input["on_bridge"] == 0 {
                    16
                } else {
                    0
                }
            } else {
                doing
            };
            let mut record = *set.infantry_action(selected).unwrap();
            if let Some(words) = input["record_override"].as_array() {
                record.start_frame = words[0].as_i64().unwrap() as i32;
                record.frames_per_facing = words[1].as_i64().unwrap() as i32;
                record.facings = words[2].as_i64().unwrap() as i32;
            }
            let observed = row["native_observations"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["kind"] == "selected_record")
                .unwrap();
            assert_eq!(
                record.start_frame,
                observed["start"].as_i64().unwrap() as i32
            );
            assert_eq!(
                record.frames_per_facing,
                observed["count"].as_i64().unwrap() as i32
            );
            assert_eq!(record.facings, observed["stride"].as_i64().unwrap() as i32);
            let facing = (input["facing_bam_u32"].as_u64().unwrap() >> 8) as u8;
            assert_eq!(
                resolve_shp_frame(&record, facing, input["stage"].as_i64().unwrap() as i32),
                row["output"]["frame_i32"].as_i64().unwrap() as i32,
                "{}",
                row["name"]
            );
            assert_eq!(row["rng_before"], row["rng_after"]);
            assert_eq!(row["rng_unchanged"], true);
            assert!(row["callback_events"].as_array().unwrap().is_empty());
            compared += 1;
        }
    }
    assert_eq!(compared, 516);
}

#[test]
fn unit_body_counter_matches_original_foot_cadence() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_simple_deploy.json",
    ))
    .unwrap();
    let speed = Gsi1306Speed::new();
    for row in corpus["body_cadence"].as_array().unwrap() {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap_or(false);
        let integer =
            |key: &str, fallback: i32| input[key].as_i64().map_or(fallback, |value| value as i32);
        for voxel in [false, true] {
            let mut entity = gsi_13_06_active_shp_unit("DRON", LocomotorKind::Drive);
            entity.is_voxel = voxel;
            entity.body_frame_counter = input["counter"].as_u64().unwrap_or(0) as u32;
            entity
                .foot_speed
                .set_speed_fraction(SimFixed::from_num(i32::from(flag("moving"))));
            let loco = entity.locomotor.as_mut().unwrap();
            loco.altitude = SimFixed::from_num(integer("height", 0));
            loco.layer = crate::sim::movement::locomotor::MovementLayer::Air;
            if let Some(flags) = input["flags"].as_array() {
                entity.set_unit_simple_deploy_for_test(
                    flags[0].as_u64().unwrap() != 0,
                    flags[1].as_u64().unwrap() != 0,
                    flags[2].as_u64().unwrap() != 0,
                );
            }
            if flag("target") {
                entity.attack_target = Some(AttackTarget::new(42));
            }
            entity.foot_locomotor_swap_active = flag("locomotor_swap");
            // BeingWarpedOut (`+0x270`) through a Temporal chain's head,
            // WarpingIn (`+0x271`) through the teleport's warp-in.
            if flag("warp_out") {
                entity.temporal = crate::sim::temporal::TemporalState::warped_by_for_test(99);
            }
            if flag("warp_in") {
                entity.install_teleport_state_for_test(Some(TeleportState::for_test(
                    TeleportPhase::ChronoDelay,
                    8,
                    8,
                    1,
                )));
            }
            tick_unit_body_frame_counter(
                &mut entity,
                speed.rules(),
                ShpVehicleCadence {
                    walk_rate: integer("walk_rate", 1),
                    idle_rate: integer("idle_rate", 0),
                },
                flag("hover_attack"),
                flag("deploy_to_land"),
                integer("frame", 0) as u32,
            );
            assert_eq!(
                u64::from(entity.body_frame_counter),
                row["counter"].as_u64().unwrap(),
                "{input}, voxel={voxel}"
            );
        }
    }
}
