//! Tests for the sprite animation system.
//!
//! Separated from animation.rs to stay within the 400-line file limit.

use crate::rules::art_data::ArtRegistry;
use crate::rules::infantry_sequence::parse_infantry_sequence_registry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::*;
use crate::sim::combat::TargetKind;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::game_options::GameOptions;
use crate::sim::intern::StringInterner;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::sim::movement::{FacingClass, SpeedRules};
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
fn test_infantry_slot_boundaries_round_not_truncate() {
    // The eight octant centres coincide under both a truncating and a
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
    entity.set_is_voxel_for_test(false);
    entity.lifecycle.set_in_limbo_for_test(false);
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
    entity
}

/// The inputs the live GetCurrentSpeed reads for the SHP fixtures: each type's
/// `Speed=`, resolved by name through an interner with no bound handles.
struct Gsi1306Speed {
    rules: RuleSet,
    interner: StringInterner,
    houses: crate::sim::houses::Houses,
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
            houses: crate::sim::houses::Houses::default(),
        }
    }

    fn rules(&self) -> SpeedRules<'_> {
        SpeedRules::new(&self.rules, &self.interner, &self.houses)
    }
}

#[test]
fn gsi_13_06_counter_suppressions_hold_the_persistent_value() {
    let cadence = FootBodyCadence {
        walk_rate: 1,
        idle_rate: 1,
    };
    let speed = Gsi1306Speed::new();
    let base = gsi_13_06_active_shp_unit("DRON", LocomotorKind::Drive);
    let mut variants = Vec::new();

    let mut in_limbo = base.clone();
    in_limbo.lifecycle.set_in_limbo_for_test(true);
    variants.push(("in limbo", in_limbo));
    let mut dead = base.clone();
    dead.lifecycle.set_object_alive_for_test(false);
    variants.push(("not native-alive", dead));
    let mut dying = base.clone();
    dying.death.set_dying_for_test(true);
    variants.push(("dying", dying));
    let mut falling = base.clone();
    falling.set_falling_down_for_test(true);
    variants.push(("falling", falling));
    let mut warp_out = base.clone();
    // BeingWarpedOut (`+0x270`): a Temporal chain's head.
    warp_out.set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(99));
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
    swapped.lift.set_swap_active_for_test(true);
    variants.push(("locomotor swap", swapped));

    for (name, mut entity) in variants {
        entity.body_frame_counter.set_for_test(17);
        tick_foot_body_frame_counter(&mut entity, speed.rules(), cadence, false, false, None, 1);
        assert_eq!(entity.body_frame_counter.get(), 17, "{name}");
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
        tick_foot_body_frame_counter(
            &mut entity,
            speed.rules(),
            FootBodyCadence {
                walk_rate: 1,
                idle_rate: 0,
            },
            false,
            false,
            None,
            1,
        );
        assert_eq!(entity.body_frame_counter.get(), 0, "{name} remains idle");

        entity.foot_speed.set_speed_fraction(SimFixed::from_num(1));
        assert!(
            crate::sim::movement::motion_query::is_moving_now(&entity, speed.rules(), 1),
            "{name} moves at the full fraction"
        );
    }
}

#[test]
fn gsi_13_06_shp_movement_predicates_read_the_locomotor() {
    let speed = Gsi1306Speed::new();
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let mut entity = gsi_13_06_active_shp_unit("DRON", kind);
        let owner = DriveCoord::cell(5, 5, 0);

        // With a null destination and an equal-X/Y head, both slots answer
        // false even with a positive speed fraction.
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

        // Conversely, locomotor-owned state alone is sufficient for both
        // draw and cadence. The live GetCurrentSpeed reads DRON's type speed.
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

fn gsi_13_06_shp_set(walk_frames: u16, stand_start: u16, cadence: FootBodyCadence) -> SequenceSet {
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
        FootBodyCadence {
            walk_rate: 4,
            idle_rate: 8,
        },
    );
    assert_eq!(
        resolve_shp_vehicle_body_frame(&dlph, 0, 7, -1, false, false),
        Some(7),
        "idle Dolphin uses walk/swim slot 1 and counter 7 % 6"
    );

    let sqd = gsi_13_06_shp_set(
        20,
        160,
        FootBodyCadence {
            walk_rate: 2,
            idle_rate: 4,
        },
    );
    assert_eq!(
        resolve_shp_vehicle_body_frame(&sqd, 0, 3, -1, false, false),
        Some(23),
        "idle Squid uses walk/swim slot 1 and counter 3"
    );

    let dron = gsi_13_06_shp_set(6, 48, FootBodyCadence::native_defaults());
    assert_eq!(
        resolve_shp_vehicle_body_frame(&dron, 0, 99, -1, false, false),
        Some(49),
        "idle Terror Drone with IdleRate=0 uses standing slot 1"
    );
    assert_eq!(
        resolve_shp_vehicle_body_frame(&dron, 0, 7, -1, true, false),
        Some(7),
        "moving Terror Drone uses counter 7 % 6"
    );
}

#[test]
fn retail_drone_firing_counter_and_frames_match_original() {
    let native = crate::test_fixture::json("tools/spatial_oracle/unit_shp_fire_frames.json");
    let mut entity = gsi_13_06_active_shp_unit("DRON", LocomotorKind::Drive);
    for row in native["counter_steps"].as_array().unwrap() {
        entity
            .unit_firing_counter
            .set_for_test(i32::try_from(row["before"].as_i64().unwrap()).unwrap());
        tick_unit_firing_counter(&mut entity);
        assert_eq!(
            i64::from(entity.unit_firing_counter.get()),
            row["after"].as_i64().unwrap(),
            "native counter input {}",
            row["before"]
        );
    }
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_fixture(ArtRegistry::from_ini(&art));
    let set = rules.animation_sequence("DRON").unwrap();
    let attack = set.get(&SequenceKind::Attack).unwrap();
    assert_eq!(
        u64::from(attack.start_frame),
        native["input"]["start_firing_frame"].as_u64().unwrap()
    );
    assert_eq!(
        u64::from(attack.frame_count),
        native["input"]["firing_frames"].as_u64().unwrap()
    );
    assert_eq!(
        u64::from(attack.facings),
        native["input"]["facings"].as_u64().unwrap()
    );
    for (facing, expected) in native["first_by_facing"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        assert_eq!(
            resolve_shp_vehicle_body_frame(set, facing as u8, 0, 7, false, false),
            Some(u16::try_from(expected.as_u64().unwrap()).unwrap()),
            "native DRON first firing image, facing {facing}"
        );
    }
    for row in native["countdown"].as_array().unwrap() {
        let facing = u8::try_from(row["facing"].as_u64().unwrap()).unwrap();
        for (step, expected) in row["frames"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                resolve_shp_vehicle_body_frame(set, facing, 99, 7 - step as i32, true, false),
                Some(u16::try_from(expected.as_u64().unwrap()).unwrap()),
                "firing precedes moving body: facing {facing}, step {step}"
            );
        }
    }
}

// --- advance_animation tests ---

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

    let slow = GameOptions {
        game_speed: 0,
        ..Default::default()
    };
    let mut slow_anim = Animation::new(SequenceKind::Idle1);
    for _ in 0..4 {
        advance_animation(&mut slow_anim, &def, &slow);
    }
    assert_eq!(slow_anim.frame_index, 0);
    advance_animation(&mut slow_anim, &def, &slow);
    assert_eq!(slow_anim.frame_index, 1);

    let fast = GameOptions {
        game_speed: 7,
        ..Default::default()
    };
    let mut fast_anim = Animation::new(SequenceKind::Idle1);
    advance_animation(&mut fast_anim, &def, &fast);
    assert_eq!(fast_anim.frame_index, 1);
}

// --- Animation component tests ---

// --- Default sequence set tests ---

// --- Infantry class action and presentation integration tests ---

/// Production rules and ART readers establish the action records; the class
/// owners use them directly. GI layouts supply the counts; the completed idle
/// hint and a different Ready hint are authored completion controls below.
fn infantry_action_fixture(
    idle_hint: Option<&str>,
) -> (crate::sim::world::Simulation, RuleSet, u64) {
    infantry_action_fixture_with(idle_hint, "", &[])
}

/// [`infantry_action_fixture`] with extra `GISequence` lines and the type
/// sounds bound against a SOUNDMD `[SoundList]` of `sound_names`.
fn infantry_action_fixture_with(
    idle_hint: Option<&str>,
    sequence_lines: &str,
    sound_names: &[&str],
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
         Idle1=56,15,0{hint}\nIdle2=71,15,0,E\nDie1=134,15,0\nDie2=149,15,0\n\
         {sequence_lines}",
    ));
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art));
    rules.bind_animation_sequences(&parse_infantry_sequence_registry(&art));
    if !sound_names.is_empty() {
        let mut catalog = String::from("[SoundList]\n");
        for (index, name) in sound_names.iter().enumerate() {
            catalog.push_str(&format!("{index}={name}\n"));
        }
        rules.bind_type_sound_references(
            &ini,
            &crate::rules::sound_ini::SoundRegistry::from_ini(&IniFile::from_str(&catalog)),
        );
    }
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
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            house, 0, None, true, 0, 10,
        ));
    let id = sim
        .construct_object_limbo_at_height(
            "E1",
            "Americans",
            10,
            10,
            0,
            0,
            &rules,
            crate::sim::world::FrameEffects::default(),
        )
        .unwrap();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(false);
    assert!(
        sim.infantry_do_action(
            id,
            0,
            false,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    (sim, rules, id)
}

/// Install the retained Walk +0x36 byte that `Is_Really_Moving_Now` answers
/// (`0x0075CB20`): a moving man's head went through Process `0x0075BD25`; a
/// stopped man was Stopped with no head (`0x0075ADEC`).
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

/// `InfantryClass::Limbo @ 0x0051DF10` stops the locomotor's movement
/// animation (`0x0051DF30`, Walk `0x0075CBC0` clears +0x36) and stores the
/// water state's constructor sentinel (`0x0051DF38`) beside its prone and
/// Doing stores, so the next locomotion action after Unlimbo starts from Ready.
#[test]
fn infantry_limbo_stops_walk_animation_and_resets_water_state() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    set_infantry_walk_motion(&mut sim, id, true);
    sim.infantry_movement_actions(id, &rules, None, crate::sim::world::FrameEffects::default());
    assert_infantry_pose(&sim, id, 3, SequenceKind::Walk, 0);
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.mission_leaf.install_infantry_water_state_fixture(1);
    actor.infantry.as_mut().unwrap().set_is_prone_for_test(true);
    let _ = sim.techno_limbo(id);
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(
        actor.locomotor.as_ref().unwrap().walk_animation_moving(),
        Some(false)
    );
    let leaf = actor.mission_leaf.as_infantry().unwrap();
    assert_eq!(leaf.water_state(), 2);
    assert_eq!(leaf.doing(), 0);
    assert!(!actor.infantry.as_ref().unwrap().is_prone());
}

#[test]
fn infantry_stopped_walk_returns_to_ready_through_class_action() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    set_infantry_walk_motion(&mut sim, id, true);
    sim.infantry_movement_actions(id, &rules, None, crate::sim::world::FrameEffects::default());
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(3);
    set_infantry_walk_motion(&mut sim, id, false);
    sim.infantry_movement_actions(id, &rules, None, crate::sim::world::FrameEffects::default());
    assert_infantry_pose(&sim, id, 0, SequenceKind::Stand, 0);
}

#[test]
fn infantry_walk_stage_uses_absolute_frames_and_presentation_leaves_it_alone() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    set_infantry_walk_motion(&mut sim, id, true);
    sim.infantry_movement_actions(id, &rules, None, crate::sim::world::FrameEffects::default());
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
            &sim.session.game_options(),
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
    assert!(
        sim.infantry_do_action(
            id,
            4,
            false,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor
        .attack_target
        .set_for_test(Some(TargetKind::Entity(999)));
    actor.mission_leaf.set_foot_firing_sequence(1);
    actor.set_native_stage_value(65_538);
    let facing = actor.body_facing;
    let dead = tick_animations(
        &mut sim.substrate.entities,
        rules.animation_sequences(),
        &sim.session.game_options(),
        &sim.interner,
        100,
    );
    assert!(dead.is_empty());
    assert_infantry_pose(&sim, id, 4, SequenceKind::Attack, 65_538);
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(actor.mission_leaf.foot_firing_sequence_latch(), 1);
    assert_eq!(
        actor.attack_target.target().unwrap(),
        crate::sim::combat::TargetKind::Entity(999)
    );
    assert_eq!(actor.body_facing, facing);
}

#[test]
fn gsi_05_07_idle_completion_snaps_current_hint_before_ready_dispatch() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    sim.session.set_binary_frame_for_test(77);
    sim.substrate.entities.get_mut(id).unwrap().body_facing = FacingClass::new(0, 4);
    assert!(
        sim.infantry_do_action(
            id,
            9,
            false,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(15);
    assert!(!sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
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
    sim.session.set_binary_frame_for_test(91);
    sim.substrate.entities.get_mut(id).unwrap().body_facing = FacingClass::new(0x2000, 4);
    assert!(
        sim.infantry_do_action(
            id,
            9,
            false,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(15);
    assert!(!sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
    assert_infantry_pose(&sim, id, 0, SequenceKind::Stand, 0);
    let body = sim.substrate.entities.get(id).unwrap().body_facing;
    assert_eq!(body.destination(), 0x2000);
    assert_eq!(body.current(91), 0x2000);
}

/// DoType_Sequencer's sound tail (`0x00520E75..0x00520EF2`) for the
/// current Doing's record: a FireUp sound on frame 2 of a 6-frame record
/// plays only on a frame whose stage step passed (`+0xFC`) and whose stage
/// modulo the count is 2. A completion that starts Ready reads Ready's record.
#[test]
fn sequence_sound_plays_on_its_stage_frame_after_a_step() {
    use crate::sim::stage::StageClass;
    use crate::sim::timer::CdTimer;
    let (mut sim, rules, id) = infantry_action_fixture_with(
        None,
        "FireUpSounds= 2 SlaveAttack,4 Missing\n",
        &["SlaveAttack"],
    );
    let run = |sim: &mut crate::sim::world::Simulation, value: i32, changed: u8| {
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        actor.mission_leaf.set_infantry_doing_verified(4).unwrap();
        actor.install_native_stage_fixture(StageClass::from_native_fixture(
            value,
            changed,
            CdTimer::from_raw(0, 2),
            2,
            1,
        ));
        sim.sound_events.clear_for_test();
        assert!(!sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
        sim.sound_events.as_slice().to_vec()
    };
    let played = run(&mut sim, 2, 1);
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(played.len(), 1);
    let crate::sim::world::SimSoundEvent::VocAt {
        sound_id,
        audible_to,
        rx,
        ry,
        sub_x,
        sub_y,
        ..
    } = &played[0]
    else {
        panic!("PlayAt is a located Voc request: {played:?}");
    };
    assert_eq!(sound_id, "SlaveAttack");
    assert_eq!(*audible_to, None);
    assert_eq!(
        (*rx, *ry, *sub_x, *sub_y),
        (
            actor.position.rx(),
            actor.position.ry(),
            actor.position.sub_x(),
            actor.position.sub_y()
        )
    );
    // No step this frame, another frame, and the unresolved name's frame.
    assert!(run(&mut sim, 2, 0).is_empty());
    assert!(run(&mut sim, 3, 1).is_empty());
    assert!(run(&mut sim, 4, 1).is_empty());
    // Stage 6 completes FireUp; the tail then reads Ready's record.
    assert!(run(&mut sim, 6, 1).is_empty());
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(actor.mission_leaf.as_infantry().unwrap().doing(), 0);
}

/// A walking infantryman's Walk completion re-requests Walk, which leaves
/// the stage growing past the count, so a Walk sound plays once per cycle of
/// real stage steps (Techno `0x006FABC4`, then the sequencer).
#[test]
fn walk_sequence_sound_plays_once_per_cycle_of_stage_steps() {
    use crate::sim::stage::StageClass;
    use crate::sim::timer::CdTimer;
    let (mut sim, rules, id) =
        infantry_action_fixture_with(None, "WalkSounds= 2 TrexFoot\n", &["TrexFoot"]);
    set_infantry_walk_motion(&mut sim, id, true);
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.mission_leaf.set_infantry_doing_verified(3).unwrap();
    actor.install_native_stage_fixture(StageClass::from_native_fixture(
        0,
        0,
        CdTimer::from_raw(0, 1),
        1,
        1,
    ));
    sim.sound_events.clear_for_test();
    let mut played_at = Vec::new();
    for now in 1..=14 {
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        assert!(actor.tick_native_stage(now));
        assert!(!sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
        let actor = sim.substrate.entities.get(id).unwrap();
        assert_eq!(actor.mission_leaf.as_infantry().unwrap().doing(), 3);
        for _ in sim.sound_events.take_for_test() {
            played_at.push(actor.native_stage().value());
        }
    }
    assert_eq!(played_at, [2, 8, 14]);
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
        .set_is_prone_for_test(true);
    assert!(
        sim.infantry_do_action(
            id,
            2,
            false,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    assert_infantry_pose(&sim, id, 2, SequenceKind::Prone, 0);
    set_infantry_walk_motion(&mut sim, id, true);
    sim.infantry_movement_actions(id, &rules, None, crate::sim::world::FrameEffects::default());
    assert_infantry_pose(&sim, id, 6, SequenceKind::Crawl, 0);
    set_infantry_walk_motion(&mut sim, id, false);
    sim.infantry_movement_actions(id, &rules, None, crate::sim::world::FrameEffects::default());
    assert_infantry_pose(&sim, id, 2, SequenceKind::Prone, 0);
    assert!(
        sim.infantry_do_action(
            id,
            8,
            false,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    assert_infantry_pose(&sim, id, 8, SequenceKind::FireProne, 0);
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .infantry
            .as_ref()
            .unwrap()
            .is_prone()
    );
}

#[test]
fn infantry_down_and_up_remain_uninterruptible_until_sequence_completion() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    assert!(
        sim.infantry_do_action(
            id,
            5,
            false,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .infantry
            .as_ref()
            .unwrap()
            .is_prone()
    );
    assert!(
        !sim.infantry_do_action(
            id,
            3,
            false,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(1);
    assert!(!sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
    assert_infantry_pose(&sim, id, 5, SequenceKind::Down, 1);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(2);
    assert!(!sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
    assert_infantry_pose(&sim, id, 2, SequenceKind::Prone, 0);

    assert!(
        sim.infantry_do_action(
            id,
            7,
            false,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    assert!(
        !sim.substrate
            .entities
            .get(id)
            .unwrap()
            .infantry
            .as_ref()
            .unwrap()
            .is_prone()
    );
    assert!(
        !sim.infantry_do_action(
            id,
            3,
            false,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(1);
    assert!(!sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
    assert_infantry_pose(&sim, id, 7, SequenceKind::Up, 1);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(2);
    assert!(!sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
    assert_infantry_pose(&sim, id, 0, SequenceKind::Stand, 0);
}

#[test]
fn infantry_death_finishes_at_its_sequence_count_and_not_one_frame_early() {
    let (mut sim, rules, id) = infantry_action_fixture(Some("S"));
    assert!(
        sim.infantry_do_action(
            id,
            11,
            true,
            &rules,
            crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.death.set_dying_for_test(true);
    actor.health.set_for_test(0);
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
    assert!(!sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
    assert_infantry_pose(&sim, id, 11, SequenceKind::Die1, 14);
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(15);
    assert!(sim.infantry_sequencer(id, &rules, crate::sim::world::FrameEffects::default()));
    let retired = sim.substrate.entities.get(id).unwrap();
    assert!(!retired.lifecycle.object_alive());
    assert!(retired.lifecycle.in_limbo());
    assert!(sim.substrate.pending_delete.contains_object(&id));
    sim.process_pending_delete_with(
        &rules,
        None,
        crate::sim::world::FrameEffects::default(),
        1,
        false,
    );
    assert!(sim.substrate.entities.get(id).is_none());
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
        Some(FootBodyCadence {
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
    // Nothing switches the generic sequence: the Unit SHP draw asks
    // Is_Moving itself.
    for sequence in [SequenceKind::Stand, SequenceKind::Walk] {
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        assert!(actor.is_voxel());
        actor.body_frame_counter.set_for_test(17);
        actor.animation = Some(Animation {
            sequence,
            frame_index: 3,
            elapsed_frames: 2,
            finished: false,
        });
        for now in [0, 4, 100] {
            assert!(
                tick_animations(
                    &mut sim.substrate.entities,
                    rules.animation_sequences(),
                    &sim.session.game_options(),
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
                actor.body_frame_counter.get(),
                17,
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
    let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let Some(mut rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
    else {
        return;
    };
    rules.bind_animation_sequences(&parse_infantry_sequence_registry(&art));
    let set = rules.animation_sequence("E1").unwrap();
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/anytown_damage/foot_missions.json",
    ))
    .unwrap();
    let receipt = &corpus["infantry_frame_selection_receipt"];
    assert_eq!(receipt["native_span"]["address"], "0x00518D80");
    assert_eq!(receipt["native_span"]["stop"], "0x00518F89");
    assert_eq!(receipt["native_span"]["length"], 0x209);
    assert_eq!(
        receipt["preserved_payload"]["canonical_sha256"],
        "94db9dcd2cb08ecbc23867f5e11f09f5cb1b2d1e3db7f29eebf491dbb59a8a09"
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
    use crate::sim::movement::locomotor::MovementLayer;

    let speed = Gsi1306Speed::new();
    for row in corpus["body_cadence"].as_array().unwrap() {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap_or(false);
        let integer =
            |key: &str, fallback: i32| input[key].as_i64().map_or(fallback, |value| value as i32);
        // The DeployToLand gate asks GetHeight (`0x004DA9CD`), which reads no
        // layer, so each row holds on the Ground layer too.
        for (voxel, layer) in [false, true].into_iter().flat_map(|voxel| {
            [MovementLayer::Air, MovementLayer::Ground].map(|layer| (voxel, layer))
        }) {
            let mut entity = gsi_13_06_active_shp_unit("DRON", LocomotorKind::Drive);
            entity.set_is_voxel_for_test(voxel);
            entity
                .body_frame_counter
                .set_for_test(input["counter"].as_u64().unwrap_or(0) as u32);
            entity
                .foot_speed
                .set_speed_fraction(SimFixed::from_num(i32::from(flag("moving"))));
            entity
                .position
                .set_exact_z_leptons_for_test(Some(integer("height", 0)));
            entity.locomotor.as_mut().unwrap().set_layer_for_test(layer);
            if let Some(flags) = input["flags"].as_array() {
                entity.set_unit_simple_deploy_for_test(
                    flags[0].as_u64().unwrap() != 0,
                    flags[1].as_u64().unwrap() != 0,
                    flags[2].as_u64().unwrap() != 0,
                );
            }
            if flag("target") {
                entity
                    .attack_target
                    .set_for_test(Some(TargetKind::Entity(42)));
            }
            entity.lift.set_swap_active_for_test(flag("locomotor_swap"));
            // BeingWarpedOut (`+0x270`) through a Temporal chain's head,
            // WarpingIn (`+0x271`) through the teleport's warp-in.
            if flag("warp_out") {
                entity.set_temporal_for_test(
                    crate::sim::temporal::TemporalState::warped_by_for_test(99),
                );
            }
            if flag("warp_in") {
                entity.install_teleport_state_for_test(Some(TeleportState::for_test(
                    TeleportPhase::ChronoDelay,
                    8,
                    8,
                    1,
                )));
            }
            tick_foot_body_frame_counter(
                &mut entity,
                speed.rules(),
                FootBodyCadence {
                    walk_rate: integer("walk_rate", 1),
                    idle_rate: integer("idle_rate", 0),
                },
                flag("hover_attack"),
                flag("deploy_to_land"),
                None,
                integer("frame", 0) as u32,
            );
            assert_eq!(
                u64::from(entity.body_frame_counter.get()),
                row["counter"].as_u64().unwrap(),
                "{input}, voxel={voxel}, {layer:?}"
            );
        }
    }
}

#[test]
fn foot_body_counter_matches_original_concrete_classes() {
    use crate::map::entities::EntityCategory;
    use crate::sim::movement::locomotion::piggyback::{LocomotorRuntimePayload, WalkRuntime};

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/unit_simple_deploy.json",
    ))
    .unwrap();
    let rows = corpus["foot_class_cadence"].as_array().unwrap();
    assert_eq!(rows.len(), 22);
    let speed = Gsi1306Speed::new();
    for row in rows {
        let input = &row["input"];
        let flag = |key: &str| input[key].as_bool().unwrap_or(false);
        let integer =
            |key: &str, fallback: i32| input[key].as_i64().map_or(fallback, |value| value as i32);
        let mut entity = match input["category"].as_str().unwrap() {
            "unit" => {
                let mut entity = gsi_13_06_active_shp_unit("DRON", LocomotorKind::Drive);
                entity.foot_speed.set_speed_fraction(SIM_ZERO);
                entity.set_unit_simple_deploy_for_test(flag("deployed"), false, false);
                entity
            }
            category => {
                let (category, type_name, kind) = match category {
                    "aircraft" => (EntityCategory::Aircraft, "SPYP", LocomotorKind::Fly),
                    "infantry" => (EntityCategory::Infantry, "E1", LocomotorKind::Walk),
                    _ => unreachable!("original concrete Foot control"),
                };
                let mut entity = GameEntity::test_default_of_category(
                    23_001, type_name, "Soviet", 5, 5, category,
                );
                entity.lifecycle.set_in_limbo_for_test(false);
                let mut loco = LocomotorState::for_test_kind(kind);
                match kind {
                    LocomotorKind::Fly => {
                        loco.fly_runtime_mut().unwrap().set_current_speed_for_test(
                            SimFixed::from_bits(integer("current_speed_q16", 0)),
                        )
                    }
                    LocomotorKind::Walk => {
                        let v = input["head"].as_array().unwrap();
                        let head = DriveCoord {
                            x: v[0].as_i64().unwrap() as i32,
                            y: v[1].as_i64().unwrap() as i32,
                            z: v[2].as_i64().unwrap() as i32,
                        };
                        loco.set_runtime_payload_for_test(LocomotorRuntimePayload::Walk(
                            WalkRuntime::for_test(
                                (head != DriveCoord { x: 0, y: 0, z: 0 }).then_some(head),
                                None,
                                flag("moving"),
                                false,
                            ),
                        ));
                        entity
                            .foot_speed
                            .set_speed_fraction(SimFixed::from_bits(integer(
                                "speed_fraction_q16",
                                0,
                            )));
                    }
                    _ => unreachable!(),
                }
                entity.locomotor = Some(loco);
                entity
            }
        };
        entity
            .body_frame_counter
            .set_for_test(integer("counter", 0) as u32);
        if flag("target") {
            entity
                .attack_target
                .set_for_test(Some(TargetKind::Entity(42)));
        }
        entity.lift.set_swap_active_for_test(flag("locomotor_swap"));
        if flag("warp_out") {
            entity
                .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(99));
        }
        if flag("warp_in") {
            entity.install_teleport_state_for_test(Some(TeleportState::for_test(
                TeleportPhase::ChronoDelay,
                8,
                8,
                1,
            )));
        }
        tick_foot_body_frame_counter(
            &mut entity,
            speed.rules(),
            FootBodyCadence {
                walk_rate: integer("walk_rate", 1),
                idle_rate: integer("idle_rate", 0),
            },
            flag("hover_attack"),
            flag("deploy_to_land"),
            None,
            integer("frame", 0) as u32,
        );
        assert_eq!(
            u64::from(entity.body_frame_counter.get()),
            row["counter"].as_u64().unwrap(),
            "{input}"
        );
        for stream in ["main", "scenario", "mapgen"] {
            assert_eq!(row["invariants"]["rng_unchanged"][stream], true, "{input}");
        }
        if entity.category() != EntityCategory::Unit {
            assert_eq!(row["observed"]["unit_deployed_calls"], 0, "{input}");
        }
    }
}
