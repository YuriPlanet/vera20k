//! Simulation integration tests — exercises the full tick pipeline: entity spawning,
//! movement commands, combat, bridge traversal, ship pathfinding, deploy/undeploy,
//! and multi-system interactions.

use std::collections::BTreeMap;

#[path = "navigation_tests.rs"]
mod navigation_tests;

use super::*;
use crate::map::entities::{EntityCategory, MapEntity};
use crate::map::houses::HouseAllianceMap;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::terrain;
use crate::map::tube_facts::{TubeFact, TubeId};
use crate::rules::art_data::ArtRegistry;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::particle_system_type::ParticleSystemTypeId;
use crate::rules::particle_type::ParticleTypeId;
use crate::rules::ruleset::RuleSet;
use crate::sim::animation::{
    Animation, FacingSlots, LoopMode, SequenceDef, SequenceKind, SequenceSet,
};
use crate::sim::bridge_state::{BridgeDamageEvent, BridgeRuntimeState};
use crate::sim::combat::AttackTarget;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::{
    DriveCoord, DriveLocomotionRuntime, MovementTarget, ShipLocomotionRuntime,
};
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement::FacingClass;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;
use crate::sim::particles::{Particle, ParticleSystem};
use crate::sim::pathfinding::PathGrid;
use crate::util::fixed_math::{SIM_HALF, SIM_ONE, SIM_ZERO, SimFixed};
use glam::IVec3;

fn make_test_entity(type_id: &str, category: EntityCategory) -> MapEntity {
    MapEntity {
        owner: "Americans".to_string(),
        type_id: type_id.to_string(),
        health: 256,
        cell_x: 30,
        cell_y: 40,
        facing: 64,
        category,
        sub_cell: 0,
        veterancy: 0,
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        structure_upgrades: [None, None, None],
    }
}

pub(crate) fn empty_heights() -> BTreeMap<(u16, u16), u8> {
    BTreeMap::new()
}

#[test]
fn gsi_04_05_map_recruitment_bytes_reach_persistent_techno_state() {
    let mut sim = Simulation::new();
    let mut placement = make_test_entity("MTNK", EntityCategory::Unit);
    placement.recruitable_a = false;
    placement.recruitable_b = true;
    assert_eq!(sim.spawn_from_map(&[placement], None, &empty_heights()), 1);

    let response = sim
        .substrate
        .entities
        .get(1)
        .expect("map unit spawned")
        .base_defense_response;
    assert!(!response.recruitable_a);
    assert!(response.recruitable_b);
}

fn game_speed_command_sim() -> (Simulation, crate::sim::intern::InternedId) {
    let mut sim = Simulation::with_seed(0x5EED_0001);
    let owner = sim.interner.intern("Local");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
    );
    sim.session.house_order.push(owner);
    (sim, owner)
}

#[test]
fn game_speed_transition_applies_at_ingress_before_triggers_and_hash() {
    let (mut sim, owner) = game_speed_command_sim();
    let (mut control, _) = game_speed_command_sim();
    let command = CommandEnvelope::new(owner, 1, Command::SetGameSpeed { speed: 4 });

    let result = sim
        .advance_master_frame(
            &[command],
            None,
            &empty_heights(),
            None,
            None,
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    let control_result = control
        .advance_master_frame(
            &[],
            None,
            &empty_heights(),
            None,
            None,
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");

    assert!(result.frame_committed);
    assert_eq!(result.executed_commands, 1);
    assert_eq!(result.tick, control_result.tick);
    assert_eq!(sim.session.binary_frame, control.session.binary_frame);
    assert_eq!(sim.session.game_options.game_speed, 4);
    assert_eq!(control.session.game_options.game_speed, 1);
    assert_ne!(result.state_hash, control_result.state_hash);
    assert_eq!(result.state_hash, sim.state_hash());
    assert!(sim.take_master_frame_test_trace().starts_with(&[
        MasterFrameTestRung::SessionCommands,
        MasterFrameTestRung::Triggers,
        MasterFrameTestRung::TeamScript,
        MasterFrameTestRung::LogicVector,
    ]));
}

#[test]
fn invalid_or_unknown_game_speed_transition_is_consumed_without_state_effect() {
    let (mut invalid, owner) = game_speed_command_sim();
    let (mut invalid_control, _) = game_speed_command_sim();
    let invalid_result = invalid.advance_tick(
        &[CommandEnvelope::new(
            owner,
            1,
            Command::SetGameSpeed { speed: 7 },
        )],
        None,
        &empty_heights(),
        None,
        None,
        67,
    );
    let invalid_control_result =
        invalid_control.advance_tick(&[], None, &empty_heights(), None, None, 67);
    assert_eq!(invalid_result.executed_commands, 1);
    assert_eq!(invalid.session.game_options.game_speed, 1);
    assert_eq!(invalid_result.state_hash, invalid_control_result.state_hash);

    let (mut unknown, _) = game_speed_command_sim();
    let (mut unknown_control, _) = game_speed_command_sim();
    let unknown_owner = unknown.interner.intern("Unknown");
    let unknown_control_owner = unknown_control.interner.intern("Unknown");
    assert_eq!(unknown_owner, unknown_control_owner);
    let unknown_result = unknown.advance_tick(
        &[CommandEnvelope::new(
            unknown_owner,
            1,
            Command::SetGameSpeed { speed: 4 },
        )],
        None,
        &empty_heights(),
        None,
        None,
        67,
    );
    let unknown_control_result =
        unknown_control.advance_tick(&[], None, &empty_heights(), None, None, 67);
    assert_eq!(unknown_result.executed_commands, 1);
    assert_eq!(unknown.session.game_options.game_speed, 1);
    assert_eq!(unknown_result.state_hash, unknown_control_result.state_hash);
}

#[test]
fn game_speed_ingress_uses_house_order_and_survives_same_frame_exit() {
    let (mut sim, local) = game_speed_command_sim();
    let remote = sim.interner.intern("Remote");
    sim.houses.insert(
        remote,
        crate::sim::house_state::HouseState::new(remote, 1, None, false, 0, 10),
    );
    sim.session.house_order.push(remote);
    let commands = [
        CommandEnvelope::new(remote, 1, Command::SetGameSpeed { speed: 2 }),
        CommandEnvelope::new(local, 1, Command::SetGameSpeed { speed: 4 }),
        CommandEnvelope::new(local, 1, Command::ExitMatch),
    ];

    let result = sim.advance_tick(&commands, None, &empty_heights(), None, None, 67);

    assert!(!result.frame_committed);
    assert_eq!(result.executed_commands, 3);
    assert_eq!(sim.session.game_options.game_speed, 2);
    assert!(sim.quit_requested);
    assert_eq!(result.state_hash, sim.state_hash());
}

#[test]
fn network_modal_does_not_execute_game_speed_ingress() {
    let (mut sim, owner) = game_speed_command_sim();
    let command = CommandEnvelope::new(owner, 1, Command::SetGameSpeed { speed: 4 });

    let result = sim
        .advance_master_frame(
            &[command],
            None,
            &empty_heights(),
            None,
            None,
            67,
            TickLane::NetworkModal,
            None,
        )
        .expect("fixture frame must complete");

    assert_eq!(result.executed_commands, 0);
    assert_eq!(sim.session.game_options.game_speed, 1);
}

fn animation_boundary_fixture() -> (Simulation, RuleSet) {
    let mut sim = Simulation::with_seed(0xA11A_7100);
    let owner = sim.interner.intern("Americans");
    let type_ref = sim.interner.intern("E1");
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        1,
        4,
        4,
        0,
        0,
        owner,
        crate::sim::components::Health { current: 100 },
        type_ref,
        EntityCategory::Infantry,
        0,
        0,
        false,
    );
    entity.animation = Some(Animation::new(SequenceKind::Idle1));
    entity.body_facing = Some(FacingClass::new(0, 4));
    sim.substrate.entities.insert(entity);

    let idle = SequenceDef {
        start_frame: 0,
        frame_count: 1,
        facings: 8,
        facing_multiplier: 1,
        frame_delay: 1,
        normalized: false,
        completion_facing: Some(128),
        loop_mode: LoopMode::TransitionTo(SequenceKind::Stand),
        facing_slots: FacingSlots::InfantryTable,
    };
    let stand = SequenceDef {
        start_frame: 1,
        frame_count: 1,
        facings: 8,
        facing_multiplier: 1,
        frame_delay: 1,
        normalized: false,
        completion_facing: None,
        loop_mode: LoopMode::Loop,
        facing_slots: FacingSlots::InfantryTable,
    };
    let mut set = SequenceSet::new();
    set.insert(SequenceKind::Idle1, idle);
    set.insert(SequenceKind::Stand, stand);
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n[E1]\nStrength=100\n",
    ))
    .expect("animation fixture rules");
    rules.replace_animation_sequences_for_test(BTreeMap::from([("E1".to_string(), set)]));
    (sim, rules)
}

fn particle_frame_boundary_fixture(frame_count: u16) -> (Simulation, RuleSet) {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[Particles]\n0=SmokeP\n\
         [SmokeP]\nBehavesLike=Smoke\nImage=SMOKEIMG\nStateAIAdvance=0\n\
         EndStateAI=2\nDeleteOnStateLimit=yes\nMaxEC=100\n\
         [ParticleSystems]\n0=SmokeSys\n\
         [SmokeSys]\nBehavesLike=Smoke\nHoldsWhat=SmokeP\nSpawns=no\n\
         Lifetime=100\nParticleCap=10\n",
    ))
    .expect("particle boundary rules");
    rules.set_effect_frame_count_for_test("SMOKEIMG", frame_count, frame_count);

    let mut sim = Simulation::with_seed(0xEFFE_C705);
    let stable_id = sim.allocate_stable_id();
    let particle = Particle {
        type_id: ParticleTypeId(0),
        coords: IVec3::ZERO,
        previous_coords: IVec3::ZERO,
        origin: IVec3::ZERO,
        direction: [SIM_ZERO; 3],
        velocity: SIM_ZERO,
        lifetime_remaining: 100,
        damage_counter: 0,
        state_ai_advance: 0,
        animation_state: 0,
        translucency: 0,
        hit_ground: false,
        marked_for_deletion: false,
        drift_x: 0,
        drift_y: 0,
        drift_z: 0,
        current_color: [0; 3],
        color_index: 0,
        color_accumulator: SIM_ZERO,
        spark: None,
        prev_delta: [SIM_ZERO; 3],
        state_advance_counter: 0,
    };
    sim.particle_systems_mut().insert(ParticleSystem {
        stable_id,
        in_logic_vector: false,
        type_id: ParticleSystemTypeId(0),
        coords: IVec3::ZERO,
        offset: IVec3::ZERO,
        particles: vec![particle],
        spawn_timer: SIM_ZERO,
        lifetime: 100,
        spark_spawn_frames: 0,
        facing: 0,
        directionless: false,
        attached_entity: None,
        owner_entity: None,
        target_coords: IVec3::ZERO,
        owner_house: None,
        done_spawning: true,
    });
    assert!(sim.reveal_particle_system(stable_id, None));
    (sim, rules)
}

#[test]
fn master_frame_hash_observes_living_animation_completion_facing() {
    let (mut sim, rules) = animation_boundary_fixture();

    let result = sim
        .advance_master_frame(
            &[],
            Some(&rules),
            &empty_heights(),
            None,
            None,
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");

    let entity = sim.substrate.entities.get(1).expect("living infantry");
    assert_eq!(entity.facing, 128);
    assert_eq!(
        entity.animation.as_ref().expect("animation").sequence,
        SequenceKind::Stand
    );
    assert_eq!(result.state_hash, sim.state_hash());
}

#[test]
fn app_and_headless_frames_hash_identically_for_animation_progress() {
    let (mut app_sim, rules) = animation_boundary_fixture();
    let (mut headless_sim, _) = animation_boundary_fixture();

    let app = app_sim
        .advance_app_frame(
            &[],
            Some(&rules),
            &empty_heights(),
            None,
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    let headless = headless_sim.advance_tick(&[], Some(&rules), &empty_heights(), None, None, 67);

    assert!(app.tick.frame_committed && headless.frame_committed);
    assert_eq!(app.tick.state_hash, headless.state_hash);
    assert_eq!(app_sim.state_hash(), headless_sim.state_hash());
    let app_entity = app_sim.substrate.entities.get(1).expect("app infantry");
    assert_eq!(app_entity.facing, 128);
    assert_eq!(
        app_entity
            .animation
            .as_ref()
            .expect("app animation")
            .sequence,
        SequenceKind::Stand,
    );
    assert_eq!(
        app_sim
            .substrate
            .entities
            .get(1)
            .and_then(|entity| entity.animation.as_ref())
            .map(|anim| {
                (
                    anim.sequence,
                    anim.frame_index,
                    anim.elapsed_frames,
                    anim.finished,
                )
            }),
        headless_sim
            .substrate
            .entities
            .get(1)
            .and_then(|entity| entity.animation.as_ref())
            .map(|anim| {
                (
                    anim.sequence,
                    anim.frame_index,
                    anim.elapsed_frames,
                    anim.finished,
                )
            }),
    );
}

#[test]
fn app_and_headless_frames_hash_identically_for_particle_frame_timing() {
    let (mut app_sim, rules) = particle_frame_boundary_fixture(5);
    let (mut headless_sim, _) = particle_frame_boundary_fixture(5);

    for frame in 1..=4 {
        let app = app_sim
            .advance_app_frame(
                &[],
                Some(&rules),
                &empty_heights(),
                None,
                67,
                TickLane::Ordinary,
                None,
            )
            .expect("fixture frame must complete");
        let headless =
            headless_sim.advance_tick(&[], Some(&rules), &empty_heights(), None, None, 67);

        assert!(app.tick.frame_committed && headless.frame_committed);
        assert_eq!(app.tick.state_hash, headless.state_hash, "frame {frame}");
        assert_eq!(
            app_sim.state_hash(),
            headless_sim.state_hash(),
            "frame {frame}"
        );
        let app_particles = app_sim
            .particle_systems()
            .iter()
            .next()
            .map(|(_, system)| system.particles.len())
            .unwrap_or(0);
        let headless_particles = headless_sim
            .particle_systems()
            .iter()
            .next()
            .map(|(_, system)| system.particles.len())
            .unwrap_or(0);
        assert_eq!(app_particles, headless_particles, "frame {frame}");
        assert_eq!(app_particles, usize::from(frame < 4), "frame {frame}");
    }
}

#[test]
fn moving_water_unit_spawns_rules_bound_wake_without_preinterned_effect_name() {
    // A drive-family boat: the native wake gate is the drive locomotor's
    // `Is_Moving_Now` slot, so the fixture names the Drive locomotor.
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]

         [VehicleTypes]
0=BOAT

         [AircraftTypes]

         [BuildingTypes]

         [BOAT]
Strength=300
Armor=heavy
Speed=6
MovementZone=Water
SpeedType=Float
Naval=yes
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("drive boat rules should parse");
    // Stock artmd.ini [WAKE1]: ground layer, sorted under the hull.
    rules.merge_art_data(&ArtRegistry::from_ini(&IniFile::from_str(
        "[WAKE1]
Layer=ground
YSortAdjust=-288
Translucent=yes
Rate=120
",
    )));
    rules
        .art_registry
        .bind_anim_frame_count_for_test("WAKE1", 15);
    let mut sim = Simulation::new();
    let boat_id = sim
        .spawn_object("BOAT", "Americans", 0, 0, 64, &rules, &empty_heights())
        .expect("spawn boat");
    assert!(sim.interner.get("WAKE1").is_none());
    // The native gate reads CellClass+0xEC == 2 (Water); the fixture's water
    // grid carries land type 4 for movement, so stamp the binary's mirror.
    let mut terrain = water_terrain(2, 1);
    for cell in &mut terrain.cells {
        cell.yr_cell_land_type = crate::rules::terrain_rules::LandType::Water.as_index();
    }
    sim.resolved_terrain = Some(terrain);
    {
        let boat = sim
            .substrate
            .entities
            .get_mut(boat_id)
            .expect("boat entity");
        // The native gate is the locomotor's `Is_Moving_Now` slot: a live
        // destination/head-to coordinate and a positive owner speed. BOAT's
        // drive runtime is created lazily by the first movement step, so seed it.
        let ahead = crate::sim::components::DriveCoord {
            x: 256 + 128,
            y: 128,
            z: 0,
        };
        let drive = boat
            .drive_locomotion
            .get_or_insert_with(crate::sim::components::DriveLocomotionRuntime::default);
        drive.destination = Some(ahead);
        drive.head_to = Some(ahead);
        boat.foot_speed.cached_current_speed = 256;
    }

    // The movement step of a full tick would rewrite the drive runtime from
    // the fixture's (non-)motion, so exercise the spawn owner directly at a
    // frame the native cadence accepts.
    assert_eq!(sim.session.binary_frame % 10, 0);
    sim.spawn_wakes_for_frame(&rules);

    let wake_anims: Vec<u64> = sim
        .logic_order()
        .iter()
        .copied()
        .filter(|id| {
            sim.anim(*id)
                .is_some_and(|anim| sim.interner.resolve(anim.type_id) == "WAKE1")
        })
        .collect();
    assert_eq!(wake_anims.len(), 1, "one wake AnimClass per moving boat");
    let wake = sim.anim(wake_anims[0]).expect("wake anim");
    // Exact lepton position, not the cell centre (native `PositionCoord`).
    let boat = sim.substrate.entities.get(boat_id).expect("boat");
    let expected = crate::sim::anim_class::AnimWorldCoord::from_cell_sub_z(
        boat.position.rx,
        boat.position.ry,
        boat.position.sub_x,
        boat.position.sub_y,
        boat.position.z,
    );
    assert_eq!(
        (wake.world_coord.x, wake.world_coord.y),
        (expected.x, expected.y)
    );
    assert_eq!(wake.draw_flags, 0x600);

    // Off-cadence frames and a unit that is not moving now spawn nothing.
    sim.session.binary_frame = 5;
    sim.spawn_wakes_for_frame(&rules);
    sim.session.binary_frame = 10;
    sim.substrate
        .entities
        .get_mut(boat_id)
        .expect("boat")
        .foot_speed
        .cached_current_speed = 0;
    sim.spawn_wakes_for_frame(&rules);
    let count_after = sim
        .logic_order()
        .iter()
        .filter(|id| {
            sim.anim(**id)
                .is_some_and(|a| sim.interner.resolve(a.type_id) == "WAKE1")
        })
        .count();
    assert_eq!(
        count_after, 1,
        "off-cadence or stationary must not add wakes"
    );
}

#[test]
fn advance_tick_finishes_dying_infantry_from_rules_catalog() {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [E1]\nStrength=100\n",
    ))
    .expect("dying infantry rules");
    let art_ini = IniFile::from_str(
        "[E1]\nSequence=TestSequence\n\
         [TestSequence]\nReady=0,1,1\nDie1=8,2,0\n",
    );
    rules.merge_art_data(&ArtRegistry::from_ini(&art_ini));
    rules.bind_animation_sequences(
        &crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art_ini),
    );
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object("E1", "Americans", 4, 4, 0, &rules, &empty_heights())
        .expect("spawn infantry");
    let entity = sim
        .substrate
        .entities
        .get_mut(id)
        .expect("spawned infantry");
    entity.dying = true;
    entity.animation = Some(Animation {
        sequence: SequenceKind::Die1,
        frame_index: 0,
        elapsed_frames: 0,
        finished: false,
    });

    let first = sim.advance_tick(&[], Some(&rules), &empty_heights(), None, None, 67);
    let after_first = sim
        .substrate
        .entities
        .get(id)
        .and_then(|entity| entity.animation.as_ref())
        .expect("two-frame death survives its first visit");
    assert_eq!(after_first.frame_index, 1);
    assert!(!after_first.finished);

    let second = sim.advance_tick(&[], Some(&rules), &empty_heights(), None, None, 67);

    assert!(first.frame_committed && second.frame_committed);
    assert!(
        sim.substrate.entities.get(id).is_none(),
        "the headless adapter must use RuleSet timing and drain the finished death",
    );
    assert_eq!(second.state_hash, sim.state_hash());
}

#[test]
fn terminal_master_frame_does_not_advance_living_animation() {
    let (mut sim, rules) = animation_boundary_fixture();
    let owner = insert_house_with_counts(&mut sim, "Americans", 1, 1);
    let exit = CommandEnvelope::new(owner, 1, Command::ExitMatch);

    let result = sim
        .advance_master_frame(
            &[exit],
            Some(&rules),
            &empty_heights(),
            None,
            None,
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");

    assert!(!result.frame_committed);
    assert_eq!(sim.session.tick, 0);
    assert_eq!(sim.session.binary_frame, 0);
    let entity = sim.substrate.entities.get(1).expect("living infantry");
    let animation = entity.animation.as_ref().expect("animation");
    assert_eq!(animation.sequence, SequenceKind::Idle1);
    assert_eq!(animation.frame_index, 0);
    assert_eq!(animation.elapsed_frames, 0);
    assert!(!animation.finished);
    assert_eq!(entity.facing, 0);
}

#[test]
fn app_frame_output_transfers_pre_tick_sound_exactly_once_without_hash_change() {
    let mut sim = Simulation::new();
    let sound_id = sim.interner.intern("WaterfallLoop");
    sim.sound_events.push(SimSoundEvent::AnimationStarted {
        anim_id: 9,
        sound_id,
        world: crate::sim::anim_class::AnimWorldCoord {
            x: 128,
            y: 128,
            z: 0,
        },
    });

    let first = sim
        .advance_app_frame(
            &[],
            None,
            &empty_heights(),
            None,
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    assert!(matches!(
        first.sound_events.as_slice(),
        [SimSoundEvent::AnimationStarted { anim_id: 9, .. }]
    ));
    assert_eq!(first.tick.state_hash, sim.state_hash());

    let second = sim
        .advance_app_frame(
            &[],
            None,
            &empty_heights(),
            None,
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    assert!(second.sound_events.is_empty());
    assert_eq!(second.tick.state_hash, sim.state_hash());
}

#[test]
fn app_frame_output_finalizes_overlay_navigation_and_delivers_updates_once() {
    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(gsi_04_10_clear_terrain(4, 4));
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(4, 4));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let owner = sim.interner.intern("WallOwner");
    sim.overlay_grid
        .as_mut()
        .expect("overlay grid")
        .place_owned_wall(2, 2, 2, 0x23, owner);

    let deferred = sim
        .advance_app_frame(
            &[],
            Some(&rules),
            &empty_heights(),
            None,
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    assert!(deferred.overlay_updates.is_empty());
    assert!(
        sim.path_grid()
            .expect("pre-finalization navigation")
            .is_walkable(2, 2),
        "a partial-input frame must retain dirty overlay work for later finalization"
    );

    let first = sim
        .advance_app_frame(
            &[],
            Some(&rules),
            &empty_heights(),
            Some(&overlays),
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    assert_eq!(first.overlay_updates.len(), 1);
    let update = &first.overlay_updates[0];
    assert_eq!((update.rx, update.ry), (2, 2));
    assert_eq!((update.overlay_id, update.frame), (2, 0x23));
    assert!(
        !sim.path_grid()
            .expect("finalized navigation")
            .is_walkable(2, 2)
    );
    assert_eq!(first.tick.state_hash, sim.state_hash());

    let second = sim
        .advance_app_frame(
            &[],
            Some(&rules),
            &empty_heights(),
            Some(&overlays),
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    assert!(second.overlay_updates.is_empty());
    assert_eq!(second.tick.state_hash, sim.state_hash());
}

#[test]
fn terminal_app_frame_finalizes_overlay_updates_before_hash() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [OverlayTypes]\n0=ORE\n1=WALL\n\
         [ORE]\nTiberium=yes\n[WALL]\nWall=yes\nStrength=100\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("terminal overlay rules");
    let overlays = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let mut terrain = gsi_04_10_clear_terrain(2, 1);
    terrain.cell_mut(0, 0).expect("terrain cell").slope_type = 5;

    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(terrain);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(2, 1));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    {
        let grid = sim.overlay_grid.as_mut().expect("overlay grid");
        grid.place_overlay(0, 0, 0, 3);
        grid.place_overlay(1, 0, 1, 7);
    }
    let owner = insert_house_with_counts(&mut sim, "Americans", 1, 1);
    let exit = CommandEnvelope::new(owner, 1, Command::ExitMatch);

    let output = sim
        .advance_app_frame(
            &[exit],
            Some(&rules),
            &empty_heights(),
            Some(&overlays),
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    assert!(!output.tick.frame_committed);
    assert_eq!(output.overlay_updates.len(), 1);
    assert_eq!(
        (
            output.overlay_updates[0].rx,
            output.overlay_updates[0].ry,
            output.overlay_updates[0].overlay_id,
            output.overlay_updates[0].frame,
        ),
        (1, 0, 1, 7)
    );
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .expect("overlay grid")
            .cell(0, 0)
            .overlay_id,
        None
    );
    assert!(
        !sim.path_grid()
            .expect("terminal navigation")
            .is_walkable(1, 0)
    );
    assert_eq!(output.tick.state_hash, sim.state_hash());

    let next = sim
        .advance_app_frame(
            &[],
            Some(&rules),
            &empty_heights(),
            Some(&overlays),
            67,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    assert!(!next.tick.frame_committed);
    assert!(next.overlay_updates.is_empty());
}

fn gsi_13_10_art_model_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[General]\nFixtureOnly=1\n\
         [InfantryTypes]\n0=FALLBACKINF\n\
         [VehicleTypes]\n0=DLPH\n1=DRON\n2=SQD\n3=VXLTEST\n4=ALIASED\n5=OMITTED\n6=FALLBACKVEH\n\
         [AircraftTypes]\n0=FALLBACKAIR\n\
         [BuildingTypes]\n0=FALLBACKBLD\n\
         [DLPH]\nStrength=100\n\
         [DRON]\nStrength=100\n\
         [SQD]\nStrength=100\n\
         [VXLTEST]\nStrength=100\n\
         [ALIASED]\nStrength=100\nImage=ALT\n\
         [OMITTED]\nStrength=100\n\
         [FALLBACKVEH]\nStrength=100\n\
         [FALLBACKAIR]\nStrength=100\n\
         [FALLBACKINF]\nStrength=100\n\
         [FALLBACKBLD]\nStrength=100\n",
    );
    let art = ArtRegistry::from_ini(&IniFile::from_str(
        "[DLPH]\nVoxel=no\n\
         [DRON]\nVoxel=no\n\
         [SQD]\nVoxel=no\n\
         [VXLTEST]\nVoxel=yes\n\
         [ALT]\nVoxel=no\n\
         [OMITTED]\nCameo=OMITTEDICON\n",
    ));
    let mut rules = RuleSet::from_ini(&ini).expect("art model rules");
    rules.merge_art_data(&art);
    rules
}

fn assert_gsi_13_10_shp_unit(entity: &GameEntity) {
    assert_eq!(entity.category, EntityCategory::Unit);
    assert!(
        !entity.is_voxel,
        "SHP Unit must enter the non-voxel cadence path"
    );
    assert!(entity.animation.is_some());
    assert!(entity.voxel_animation.is_none());
}

fn assert_gsi_13_10_vxl_unit(entity: &GameEntity) {
    assert_eq!(entity.category, EntityCategory::Unit);
    assert!(entity.is_voxel);
    assert!(entity.animation.is_none());
    assert!(entity.voxel_animation.is_some());
}

fn move_sound_test_rules(configured: bool) -> RuleSet {
    let move_sound = if configured {
        "MoveSound=TestMove\n"
    } else {
        ""
    };
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[InfantryTypes]\n\
         [VehicleTypes]\n0=TESTUNIT\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [TESTUNIT]\nStrength=100\nArmor=light\nSpeed=6\n{move_sound}"
    )))
    .expect("MoveSound rules")
}

fn move_sound_test_sim() -> Simulation {
    let mut sim = Simulation::with_seed(0x1020_3040);
    let mut entity = GameEntity::test_default(1, "TESTUNIT", "Americans", 4, 4);
    entity.type_ref = sim.interner.intern("TESTUNIT");
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    sim.substrate.entities.insert(entity);
    sim
}

fn trigger_move_sound_tail(sim: &mut Simulation, rules: &RuleSet) {
    let mut before = sim.movement_sound_probe(1).expect("test Foot exists");
    before.facing = before.facing.wrapping_add(1);
    sim.tick_move_sound_after_process(1, Some(before), Some(rules));
}

#[test]
fn move_sound_start_consumes_exactly_one_main_draw() {
    let configured_rules = move_sound_test_rules(true);
    let mut sim = move_sound_test_sim();
    let scenario_before = sim.scenario_rng.state();
    let mapgen_before = sim.mapgen_rng.state();
    let mut expected_main = sim.main_rng.clone();

    // The current RuleSet resolves one MoveSound string. Retail still calls
    // Random::Next before modulo-by-one, so a fresh start consumes one raw draw.
    expected_main.next_u32();
    trigger_move_sound_tail(&mut sim, &configured_rules);
    assert_eq!(sim.main_rng.state(), expected_main.state());
    assert_eq!(sim.scenario_rng.state(), scenario_before);
    assert_eq!(sim.mapgen_rng.state(), mapgen_before);
    assert!(sim.substrate.entities.get(1).unwrap().move_sound_active);
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().move_sound_countdown,
        3
    );
    assert!(matches!(
        sim.sound_events.last(),
        Some(SimSoundEvent::AnimationStarted { anim_id: 1, .. })
    ));

    // Qualifying again while the handle is active only reloads the grace
    // counter; it does not choose another sample.
    trigger_move_sound_tail(&mut sim, &configured_rules);
    assert_eq!(sim.main_rng.state(), expected_main.state());

    // A fresh start after stop chooses again and therefore draws once again.
    sim.release_move_sound(1);
    expected_main.next_u32();
    trigger_move_sound_tail(&mut sim, &configured_rules);
    assert_eq!(sim.main_rng.state(), expected_main.state());
    assert_eq!(sim.scenario_rng.state(), scenario_before);
    assert_eq!(sim.mapgen_rng.state(), mapgen_before);

    // No configured MoveSound never enters the native vector-pick branch.
    let no_sound_rules = move_sound_test_rules(false);
    let mut silent = move_sound_test_sim();
    let silent_scenario = silent.scenario_rng.state();
    let silent_main = silent.main_rng.state();
    let silent_mapgen = silent.mapgen_rng.state();
    trigger_move_sound_tail(&mut silent, &no_sound_rules);
    assert_eq!(silent.scenario_rng.state(), silent_scenario);
    assert_eq!(silent.main_rng.state(), silent_main);
    assert_eq!(silent.mapgen_rng.state(), silent_mapgen);
    assert!(!silent.substrate.entities.get(1).unwrap().move_sound_active);
}

pub(crate) fn gsi_04_07_wall_sell_rules(
    first_unsellable: bool,
    with_sound: bool,
) -> (RuleSet, crate::map::overlay_types::OverlayTypeRegistry) {
    let ini = IniFile::from_str(&format!(
        "[General]\nFixtureOnly=1\n[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=FIRSTWALL\n1=SECONDWALL\n\
         [OverlayTypes]\n0=DUMMY0\n1=DUMMY1\n2=GAWALL\n\
         [FIRSTWALL]\nWall=yes\nCost=100\nUnsellable={}\nClickRepairable=no\n\
         [SECONDWALL]\nWall=yes\nCost=200\nUnsellable=no\n\
         [GAWALL]\nWall=yes\nStrength=300\n\
         [AudioVisual]\nSellSound={}\n",
        if first_unsellable { "yes" } else { "no" },
        if with_sound { "SellBuilding" } else { "" },
    ));
    let art_ini = IniFile::from_str(
        "[FIRSTWALL]\nToOverlay=GAWALL\n\
         [SECONDWALL]\nToOverlay=GAWALL\n\
         [GAWALL]\nDamageLevels=3\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("wall-sale rules");
    rules.merge_art_data(&crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
    let overlays = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art_ini));
    (rules, overlays)
}

pub(crate) fn gsi_04_07_wall_sell_seed_houses(
    sim: &mut Simulation,
) -> (
    crate::sim::intern::InternedId,
    crate::sim::intern::InternedId,
) {
    let wall_owner = sim.interner.intern("WallOwner");
    let receiver = sim.interner.intern("Receiver");
    let mut owner_house =
        crate::sim::house_state::HouseState::new(wall_owner, 0, None, false, 0, 10);
    owner_house.player_control = true;
    sim.houses.insert(wall_owner, owner_house);
    sim.houses.insert(
        receiver,
        crate::sim::house_state::HouseState::new(receiver, 1, None, false, 0, 10),
    );
    sim.session.house_order = vec![wall_owner, receiver];
    (wall_owner, receiver)
}

#[test]
fn gsi_04_07_wall_sell_ordered_cleanup_detach_navigation_and_zero_refund_rng() {
    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, true);
    assert!(!rules.object("FIRSTWALL").unwrap().click_repairable);
    let mut sim = Simulation::with_seed(77);
    let (wall_owner, receiver) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    let credits_before = sim.houses.get(&wall_owner).unwrap().economy.credits;
    let rng_before = sim.scenario_rng.state();

    let terrain = gsi_04_10_clear_terrain(8, 8);
    let mut retained_wall_counts = vec![0u8; 64];
    let adjust_source = |counts: &mut [u8], source: (u16, u16), add: bool| {
        const ADJACENT_8: [(i16, i16); 8] = [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ];
        for (dx, dy) in ADJACENT_8 {
            let x = (source.0 as i16).wrapping_add(dx);
            let y = (source.1 as i16).wrapping_add(dy);
            let Some(index) = terrain.native_fixed_cell_index(x, y) else {
                continue;
            };
            counts[index] = if add {
                counts[index].wrapping_add(1)
            } else {
                counts[index].wrapping_sub(1)
            };
        }
    };
    for source in [(4, 4), (4, 3), (5, 4)] {
        adjust_source(&mut retained_wall_counts, source, true);
    }
    let mut expected_after_sale = retained_wall_counts.clone();
    for cleanup_removed_source in [(4, 3), (5, 4)] {
        adjust_source(&mut expected_after_sale, cleanup_removed_source, false);
    }
    let mut grid = crate::sim::overlay_grid::OverlayGrid::from_finalized_map_payload(
        crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
            8,
            8,
            vec![(-1, 0); 64],
            retained_wall_counts,
        ),
    );
    grid.place_owned_wall(4, 4, 2, 0x03, wall_owner);
    // Damaged GAWALLs connected south/west to the sold cell. Sale cleanup
    // removes north first, then east, preserving each stale owner.
    grid.place_owned_wall(4, 3, 2, 0x24, wall_owner);
    grid.place_owned_wall(5, 4, 2, 0x28, wall_owner);
    sim.overlay_grid = Some(grid);
    sim.resolved_terrain = Some(terrain);
    {
        let grid = sim.overlay_grid.as_mut().unwrap();
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        for cell in [(4, 4), (4, 3), (5, 4)] {
            let _ = crate::sim::overlay_grid::recalc_overlay_passability(
                grid, terrain, &overlays, cell.0, cell.1,
            );
        }
        let _ = grid.take_dirty_cells();
    }
    let path = PathGrid::from_resolved_terrain(sim.resolved_terrain.as_ref().unwrap());
    assert!(!path.is_walkable(4, 4));
    assert!(!path.is_walkable(4, 3));
    assert!(!path.is_walkable(5, 4));
    sim.terrain_costs = crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(
        sim.resolved_terrain.as_ref().unwrap(),
    );
    assert_eq!(
        sim.terrain_costs[&crate::rules::locomotor_type::SpeedType::Track].cost_at(4, 4),
        0
    );
    sim.rebuild_zone_grid_full(&path);
    let ground_zone_before = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
        .expect("normal zone map");
    assert_eq!(
        ground_zone_before.zone_at(4, 3, MovementLayer::Ground),
        crate::sim::pathfinding::zone_map::ZONE_INVALID,
        "the north cleanup candidate starts blocked and unassigned"
    );
    assert_eq!(
        ground_zone_before.zone_at(5, 4, MovementLayer::Ground),
        crate::sim::pathfinding::zone_map::ZONE_INVALID,
        "the east cleanup candidate starts blocked and unassigned"
    );
    let expected_ground_zone = ground_zone_before.zone_at(4, 2, MovementLayer::Ground);
    assert_ne!(
        expected_ground_zone,
        crate::sim::pathfinding::zone_map::ZONE_INVALID
    );
    sim.path_grid = Some(std::sync::Arc::new(path.clone()));

    for (id, target) in [(10, (4, 4)), (20, (4, 3))] {
        let mut listener = GameEntity::test_default(id, "E1", "Receiver", 2, 2);
        listener.owner = receiver;
        listener.type_ref = sim.interner.intern("E1");
        listener.attack_target = Some(AttackTarget::for_cell(target.0, target.1));
        sim.substrate.entities.insert(listener);
    }
    let hash_before_sale = sim.state_hash();
    super::world_commands::clear_wall_sell_zone_repair_test_trace();

    assert!(sim.apply_command_with_overlays(
        "Receiver",
        &Command::SellWallAtCell { x: 4, y: 4 },
        Some(&rules),
        Some(&path),
        &empty_heights(),
        Some(&overlays),
    ));

    let sold = sim.overlay_grid.as_ref().unwrap().cell(4, 4);
    assert_eq!(
        (sold.overlay_id, sold.overlay_data, sold.wall_owner),
        (None, 0, None)
    );
    let cleanup = sim.overlay_grid.as_ref().unwrap().cell(4, 3);
    assert_eq!(cleanup.overlay_id, None);
    assert_eq!(cleanup.wall_owner, Some(wall_owner));
    let east_cleanup = sim.overlay_grid.as_ref().unwrap().cell(5, 4);
    assert_eq!(east_cleanup.overlay_id, None);
    assert_eq!(east_cleanup.wall_owner, Some(wall_owner));
    assert_eq!(
        sim.overlay_grid
            .as_ref()
            .unwrap()
            .retained_neighbor_counts(),
        Some(expected_after_sale.as_slice()),
        "native sale leaves the sold wall contribution stale and reverses only cleanup removals"
    );
    assert_eq!(
        expected_after_sale[5 * 8 + 3],
        1,
        "a cell adjacent only to the sold source proves that source was not decremented"
    );
    assert_eq!(
        sim.tactical_dirty_cells,
        vec![(4, 3), (5, 4), (4, 5), (3, 4), (4, 4)]
    );
    assert_eq!(sim.radar_terrain_dirty_cells, sim.tactical_dirty_cells);
    assert_eq!(
        sim.radar_terrain_dirty_generation, 5,
        "sale publishes each first-unique cleanup radar call before recompute; the sold-cell tail is a duplicate"
    );
    assert!(
        sim.substrate
            .entities
            .get(10)
            .unwrap()
            .attack_target
            .is_none()
    );
    assert!(
        sim.substrate
            .entities
            .get(20)
            .unwrap()
            .attack_target
            .is_none(),
        "cleanup removal expires the represented Cell target"
    );
    assert!(sim.path_grid.as_deref().unwrap().is_walkable(4, 4));
    assert!(sim.path_grid.as_deref().unwrap().is_walkable(4, 3));
    assert!(sim.path_grid.as_deref().unwrap().is_walkable(5, 4));
    let projected = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().expect("terrain"),
        sim.bridge_state.as_ref(),
    );
    assert_eq!(
        sim.path_grid.as_deref().unwrap().diff_cells(&projected),
        Some(Vec::new()),
        "wall-sale tail must already publish the final path cells"
    );
    assert!(
        sim.zone_grid
            .as_ref()
            .expect("zone grid")
            .movement_classes_match(sim.resolved_terrain.as_ref().expect("terrain")),
        "ordered wall-sale repair must publish every reduced movement class"
    );
    let (zone_ids_ptr, repaired_zone_ids) = {
        let map = sim
            .zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
            .expect("normal zone map before frame finalization");
        (
            map.zone_ids_slice().as_ptr() as usize,
            map.zone_ids_slice().to_vec(),
        )
    };
    assert!(
        sim.finalize_frame_overlays_and_navigation(Some(&rules), Some(&overlays), false)
            .is_empty(),
        "the sold and cleanup cells are cleared, so no occupied render update remains"
    );
    let ground_zone_after = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
        .expect("normal zone map");
    assert_eq!(
        ground_zone_after.zone_ids_slice().as_ptr() as usize,
        zone_ids_ptr,
        "diff-empty finalization must retain the ordered wall-sale zone repair"
    );
    assert_eq!(ground_zone_after.zone_ids_slice(), repaired_zone_ids);
    assert_eq!(
        ground_zone_after.zone_at(4, 3, MovementLayer::Ground),
        expected_ground_zone,
        "cleanup-created removal receives its own orphan/graph repair"
    );
    assert_eq!(
        ground_zone_after.zone_at(4, 4, MovementLayer::Ground),
        expected_ground_zone,
        "the House sale tail separately repairs the sold cell"
    );
    assert_eq!(
        ground_zone_after.zone_at(5, 4, MovementLayer::Ground),
        expected_ground_zone,
        "the east cleanup removal is repaired after north"
    );
    let repair_trace = super::world_commands::take_wall_sell_zone_repair_test_trace();
    assert_eq!(
        repair_trace
            .iter()
            .map(|step| step.repair_cell)
            .collect::<Vec<_>>(),
        vec![(4, 3), (5, 4), (4, 4)],
        "cleanup repairs N then E; the sold cell remains the House tail"
    );
    assert_eq!(
        repair_trace[0].walkable_cross,
        [true, false, true, true, true],
        "north repair sees sold+north open while east remains blocked"
    );
    assert_eq!(
        repair_trace[0].movement_class_cross,
        [
            crate::map::resolved_terrain::zone_class::GROUND,
            crate::map::resolved_terrain::zone_class::WALL,
            crate::map::resolved_terrain::zone_class::GROUND,
            crate::map::resolved_terrain::zone_class::GROUND,
            crate::map::resolved_terrain::zone_class::GROUND,
        ]
    );
    for step in &repair_trace[1..] {
        assert_eq!(step.walkable_cross, [true; 5]);
        assert_eq!(
            step.movement_class_cross,
            [crate::map::resolved_terrain::zone_class::GROUND; 5]
        );
    }
    assert_eq!(
        sim.terrain_costs[&crate::rules::locomotor_type::SpeedType::Track].cost_at(4, 4),
        100
    );
    assert_eq!(
        sim.terrain_costs[&crate::rules::locomotor_type::SpeedType::Track].cost_at(5, 4),
        100
    );
    assert_eq!(
        sim.houses.get(&wall_owner).unwrap().economy.credits,
        credits_before
    );
    assert_eq!(sim.scenario_rng.state(), rng_before);
    assert_ne!(sim.state_hash(), hash_before_sale);
    assert!(matches!(
        sim.sound_events.as_slice(),
        [SimSoundEvent::WallSold { receiver: event_receiver }] if *event_receiver == receiver
    ));
}

#[test]
fn canonical_path_grid_snapshot_remains_pinned_after_publication() {
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(gsi_04_10_clear_terrain(2, 1));
    let first = PathGrid::new(2, 1);
    sim.rebuild_zone_grid(&first);
    let pinned = sim.path_grid_snapshot().expect("first navigation snapshot");

    let mut second = first.clone();
    second.set_blocked(0, 0, true);
    sim.rebuild_zone_grid(&second);

    assert!(pinned.is_walkable(0, 0));
    assert!(
        !sim.path_grid()
            .expect("published navigation")
            .is_walkable(0, 0)
    );
    assert!(!std::sync::Arc::ptr_eq(
        &pinned,
        &sim.path_grid_snapshot()
            .expect("second navigation snapshot")
    ));
}

#[test]
fn wall_sale_preserves_the_sold_anchor_retained_count_source() {
    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    let (wall_owner, _) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    sim.resolved_terrain = Some(gsi_04_10_clear_terrain(5, 5));
    let mut retained = vec![0u8; 25];
    for index in [6usize, 7, 8, 11, 13, 16, 17, 18] {
        retained[index] = 1;
    }
    let expected = retained.clone();
    let mut grid = crate::sim::overlay_grid::OverlayGrid::from_finalized_map_payload(
        crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
            5,
            5,
            vec![(-1, 0); 25],
            retained,
        ),
    );
    grid.place_owned_wall(2, 2, 2, 0, wall_owner);
    sim.overlay_grid = Some(grid);

    assert!(sim.apply_command_with_overlays(
        "Receiver",
        &Command::SellWallAtCell { x: 2, y: 2 },
        Some(&rules),
        None,
        &empty_heights(),
        Some(&overlays),
    ));
    let grid = sim.overlay_grid.as_ref().expect("overlay authority");
    assert_eq!(grid.cell(2, 2).overlay_id, None);
    assert_eq!(
        grid.retained_neighbor_counts(),
        Some(expected.as_slice()),
        "HouseClass sale has no CellClass+0x122 decrement for the sold anchor"
    );
}

#[test]
fn wall_sale_cleanup_reaches_fixed_stride_alias_and_reverses_that_source_only() {
    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    let (wall_owner, _) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    let mut terrain = gsi_04_10_clear_terrain(512, 2);
    let mut retained = vec![0u8; 1024];
    let adjust_source = |counts: &mut [u8], source: (u16, u16), add: bool| {
        const ADJACENT_8: [(i16, i16); 8] = [
            (0, -1),
            (1, -1),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
        ];
        for (dx, dy) in ADJACENT_8 {
            let Some(index) = terrain.native_fixed_cell_index(
                (source.0 as i16).wrapping_add(dx),
                (source.1 as i16).wrapping_add(dy),
            ) else {
                continue;
            };
            counts[index] = if add {
                counts[index].wrapping_add(1)
            } else {
                counts[index].wrapping_sub(1)
            };
        }
    };
    for source in [(0, 1), (511, 0)] {
        adjust_source(&mut retained, source, true);
    }
    let mut expected = retained.clone();
    adjust_source(&mut expected, (511, 0), false);

    let mut grid = crate::sim::overlay_grid::OverlayGrid::from_finalized_map_payload(
        crate::map::authored_overlay::FinalizedOverlayPayload::from_cells_for_test(
            512,
            2,
            vec![(-1, 0); 1024],
            retained,
        ),
    );
    grid.place_owned_wall(0, 1, 2, 0x08, wall_owner);
    grid.place_owned_wall(511, 0, 2, 0x22, wall_owner);
    for (rx, ry) in [(0, 1), (511, 0)] {
        let _ = crate::sim::overlay_grid::recalc_overlay_passability(
            &mut grid,
            &mut terrain,
            &overlays,
            rx,
            ry,
        );
    }
    sim.overlay_grid = Some(grid);
    sim.resolved_terrain = Some(terrain);

    assert!(sim.apply_command_with_overlays(
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 1 },
        Some(&rules),
        None,
        &empty_heights(),
        Some(&overlays),
    ));
    let grid = sim.overlay_grid.as_ref().expect("overlay authority");
    assert_eq!(grid.cell(0, 1).overlay_id, None);
    assert_eq!(grid.cell(511, 0).overlay_id, None);
    assert_eq!(
        grid.retained_neighbor_counts(),
        Some(expected.as_slice()),
        "sale keeps the sold aliasing source but reverses the cleanup-removed aliased source"
    );
    assert!(sim.tactical_dirty_cells.contains(&(511, 0)));
    assert!(sim.radar_terrain_dirty_cells.contains(&(511, 0)));
}

#[test]
fn dynamic_navigation_publication_composes_structures_bibs_and_bridges() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GAREFN\n\
         [GAREFN]\nStrength=100\nFoundation=4x3\nBib=yes\n",
    ))
    .expect("dynamic navigation rules");
    let mut terrain = gsi_04_10_clear_terrain(16, 16);
    for rx in [1, 3] {
        let cell = terrain.cell_mut(rx, 1).expect("bridgehead cell");
        cell.bridge_walkable = true;
        cell.bridge_transition = true;
        cell.bridge_deck_level = 4;
    }
    {
        let cell = terrain.cell_mut(2, 1).expect("bridge body cell");
        cell.ground_walk_blocked = true;
        cell.build_blocked = true;
        cell.base_build_blocked = true;
        cell.is_water = true;
        cell.bridge_walkable = true;
        cell.has_bridge_deck = true;
        cell.bridge_deck_level = 4;
    }

    let mut sim = Simulation::new();
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain(
        &terrain, true, 10,
    ));
    sim.resolved_terrain = Some(terrain);
    let mut building = make_test_entity("GAREFN", EntityCategory::Structure);
    building.cell_x = 8;
    building.cell_y = 8;
    assert_eq!(
        sim.spawn_from_map(&[building], Some(&rules), &empty_heights()),
        1
    );
    let placed = sim.substrate.entities.values().next().unwrap();
    assert!(placed.lifecycle.cell_marked);
    assert!(
        sim.substrate
            .occupancy
            .contains_entity(8, 9, placed.stable_id())
    );

    assert!(sim.rebuild_dynamic_navigation(&rules));
    let grid = sim.path_grid().expect("published navigation");
    assert!(!grid.is_walkable(8, 9));
    assert!(!grid.is_walkable(10, 9));
    assert!(grid.is_walkable(11, 9), "Bib must relax the east edge");
    assert!(grid.cell(1, 1).expect("west bridgehead").transition);
    assert!(grid.cell(2, 1).expect("bridge body").bridge_walkable);
    assert!(grid.cell(3, 1).expect("east bridgehead").transition);
    assert_eq!(
        sim.terrain_costs.len(),
        crate::rules::locomotor_type::SpeedType::ALL_WITH_COSTS.len(),
        "canonical publication must install every terrain-cost row"
    );
    for speed_type in crate::rules::locomotor_type::SpeedType::ALL_WITH_COSTS {
        assert!(sim.terrain_costs.contains_key(speed_type));
    }
    let normal_zones = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(crate::rules::locomotor_type::MovementZone::Normal))
        .expect("normal movement zones");
    assert_ne!(
        normal_zones.zone_at(0, 0, MovementLayer::Ground),
        crate::sim::pathfinding::zone_map::ZONE_INVALID,
        "canonical publication must assign a reachable ground cell"
    );

    let bridge_state = sim.bridge_state.as_mut().expect("bridge runtime state");
    let _ = bridge_state.write_overlay_byte(2, 1, 0xE8);
    bridge_state
        .cell_mut(2, 1)
        .expect("bridge body runtime cell")
        .damage_state = crate::sim::bridge_state::DamageState::Destroyed;
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let collapsed_grid = sim.path_grid().expect("collapsed navigation publication");
    assert!(
        !collapsed_grid
            .cell(2, 1)
            .expect("collapsed bridge body")
            .bridge_walkable,
        "canonical publication must project the live bridge runtime state"
    );
}

#[test]
fn gsi_04_07_wall_sell_eligibility_gate_matrix_rejects_without_mutation() {
    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    let (wall_owner, _) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(3, 3));

    let sell = |sim: &mut Simulation, rules: &RuleSet| {
        sim.apply_command_with_overlays(
            "Receiver",
            &Command::SellWallAtCell { x: 1, y: 1 },
            Some(rules),
            None,
            &empty_heights(),
            Some(&overlays),
        )
    };

    assert!(!sell(&mut sim, &rules), "absent overlay rejects");
    sim.overlay_grid.as_mut().unwrap().place_overlay(1, 1, 2, 0);
    assert!(!sell(&mut sim, &rules), "absent owner rejects");

    let missing_house = sim.interner.intern("MissingHouse");
    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(1, 1, 2, 0, missing_house);
    assert!(!sell(&mut sim, &rules), "unregistered owner rejects");

    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(1, 1, 0, 0, wall_owner);
    assert!(!sell(&mut sim, &rules), "non-wall overlay rejects");

    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(1, 1, 2, 0, wall_owner);
    let no_art_match = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=FIRSTWALL\n\
         [FIRSTWALL]\nWall=yes\nCost=100\n",
    ))
    .unwrap();
    assert!(
        !sell(&mut sim, &no_art_match),
        "missing ToOverlay match rejects"
    );
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(1, 1).overlay_id,
        Some(2)
    );
}

#[test]
fn gsi_04_07_wall_sell_first_match_and_split_human_gate_are_exact() {
    let (unsellable_rules, overlays) = gsi_04_07_wall_sell_rules(true, true);
    let mut sim = Simulation::new();
    let (wall_owner, _) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(3, 3);
    grid.place_owned_wall(0, 2, 2, 0, wall_owner);
    sim.overlay_grid = Some(grid);
    assert!(!sim.apply_command_with_overlays(
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 2 },
        Some(&unsellable_rules),
        None,
        &empty_heights(),
        Some(&overlays),
    ));
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(0, 2).overlay_id,
        Some(2)
    );

    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    assert!(sim.apply_command_with_overlays(
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 2 },
        Some(&rules),
        None,
        &empty_heights(),
        Some(&overlays),
    ));
    assert!(sim.sound_events.is_empty());

    sim.overlay_grid
        .as_mut()
        .unwrap()
        .place_owned_wall(0, 2, 2, 0, wall_owner);
    sim.session.game_mode_nonzero = true;
    assert!(!sim.apply_command_with_overlays(
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 2 },
        Some(&rules),
        None,
        &empty_heights(),
        Some(&overlays),
    ));
    sim.houses.get_mut(&wall_owner).unwrap().is_human = true;
    assert!(sim.apply_command_with_overlays(
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 2 },
        Some(&rules),
        None,
        &empty_heights(),
        Some(&overlays),
    ));
    assert!(!sim.apply_command_with_overlays(
        "Receiver",
        &Command::SellWallAtCell { x: 0, y: 0 },
        Some(&rules),
        None,
        &empty_heights(),
        Some(&overlays),
    ));
}

#[test]
fn gsi_04_07_damage_fatal_transport_lifecycle_brackets_nested_death_weapon() {
    fn run(carrier_hp: i32) -> (Simulation, crate::sim::combat::CombatTickResult, u64) {
        let ini = IniFile::from_str(
            "[InfantryTypes]\n0=PASSENGER\n\
             [VehicleTypes]\n0=BOOMER\n1=SHOOTER\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n0=LISTENER\n\
             [Warheads]\n0=KillWH\n1=NoDamageWH\n2=WallWH\n\
             [OverlayTypes]\n0=TESTWALL\n\
             [BOOMER]\nStrength=11\nArmor=heavy\nExplodes=yes\nDeathWeapon=DeathBoom\n\
             [SHOOTER]\nStrength=100\nArmor=heavy\nPrimary=Gun\n\
             [PASSENGER]\nStrength=50\nArmor=none\n\
             [LISTENER]\nStrength=300\nArmor=wood\n\
             [DeathBoom]\nDamage=214\nWarhead=WallWH\n\
             [Gun]\nDamage=1\nROF=50\nRange=8\nWarhead=NoDamageWH\n\
             [KillWH]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,0%,100%,100%,100%,100%\n\
             [NoDamageWH]\nCellSpread=0\nVerses=100%,100%,100%,0%,100%,100%,100%,100%,100%,100%,100%\n\
             [WallWH]\nCellSpread=.5\nWall=yes\nVerses=100%,100%,100%,100%,100%,100%,50%,100%,100%,100%,100%\n\
             [TESTWALL]\nWall=yes\nArmor=concrete\nStrength=400\n",
        );
        let art = IniFile::from_str("[TESTWALL]\nDamageLevels=2\n");
        let rules = RuleSet::from_ini(&ini).expect("fatal lifecycle rules");
        let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, Some(&art));
        let mut sim = Simulation::with_seed(1);
        let owner = sim.interner.intern("Americans");
        let enemy = sim.interner.intern("Soviet");

        let mut cargo = crate::sim::passenger::PassengerCargo::new(2, 1);
        assert!(cargo.board(11, 1));
        let mut carrier = GameEntity::test_default(10, "BOOMER", "Soviet", 8, 5);
        carrier.owner = enemy;
        carrier.type_ref = sim.interner.intern("BOOMER");
        carrier.health.current = carrier_hp;
        carrier.passenger_role = crate::sim::passenger::PassengerRole::Transport { cargo };
        sim.substrate.entities.insert(carrier);
        let _ = sim.reveal(10);

        let mut passenger = GameEntity::test_default(11, "PASSENGER", "Soviet", 8, 5);
        passenger.owner = enemy;
        passenger.type_ref = sim.interner.intern("PASSENGER");
        passenger.category = EntityCategory::Infantry;
        passenger.mission_leaf =
            crate::sim::mission::leaf::MissionLeafState::for_entity_category(
                EntityCategory::Infantry,
            );
        passenger.is_voxel = false;
        passenger.passenger_role =
            crate::sim::passenger::PassengerRole::Inside { transport_id: 10 };
        sim.substrate.entities.insert(passenger);

        let mut listener = GameEntity::test_default(30, "LISTENER", "Americans", 8, 5);
        listener.owner = owner;
        listener.type_ref = sim.interner.intern("LISTENER");
        listener.category = EntityCategory::Structure;
        listener.is_voxel = false;
        listener.health.current = 300;
        sim.substrate.entities.insert(listener);
        let _ = sim.reveal(30);

        let mut nested_fatal = GameEntity::test_default(31, "LISTENER", "Americans", 8, 5);
        nested_fatal.owner = owner;
        nested_fatal.type_ref = sim.interner.intern("LISTENER");
        nested_fatal.category = EntityCategory::Structure;
        nested_fatal.is_voxel = false;
        nested_fatal.health.current = 107;
        sim.substrate.entities.insert(nested_fatal);
        let _ = sim.reveal(31);

        let mut attacker = GameEntity::test_default(20, "SHOOTER", "Americans", 6, 5);
        attacker.owner = owner;
        attacker.type_ref = sim.interner.intern("SHOOTER");
        sim.substrate.entities.insert(attacker);
        let _ = sim.reveal(20);
        let attacker = sim.substrate.entities.get_mut(20).unwrap();
        attacker.attack_target = Some(AttackTarget::new(10));
        attacker.radio_contacts.insert(10);
        attacker
            .mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: crate::sim::mission::MissionId::from_known(
                    crate::sim::mission::MissionType::Attack,
                ),
                suspended: crate::sim::mission::MissionId::NONE,
                queued: crate::sim::mission::MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
            });

        let mut overlays = crate::sim::overlay_grid::OverlayGrid::new(16, 16);
        overlays.place_overlay(8, 5, 0, 0);
        sim.overlay_grid = Some(overlays);
        let detonation = crate::sim::projectile::ProjectileDetonation {
            projectile_id: 1,
            source_id: 99,
            target: crate::sim::projectile::ProjectileTarget::Entity(10),
            impact: crate::sim::projectile::ProjectileCoord::new(8 * 256 + 128, 5 * 256 + 128, 0),
            payload: crate::sim::projectile::ProjectilePayload {
                base_damage: 10,
                warhead: sim.interner.intern("KillWH"),
                weapon: sim.interner.intern("Gun"),
            },
            reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
        };
        let result = sim.tick_combat_with_fatal_lifecycle(
            &rules,
            Some(&registry),
            100,
            &[10, 20],
            &BTreeSet::new(),
            &BTreeSet::new(),
            &[detonation],
            &[],
        );
        let rng = sim.scenario_rng.state();
        (sim, result, rng)
    }

    let (fatal, result, fatal_rng) = run(10);
    assert_eq!(fatal.substrate.pending_delete, vec![11, 31, 10]);
    for id in [11, 31, 10] {
        let entity = fatal.substrate.entities.get(id).unwrap();
        assert_eq!(entity.health.current, 0);
        assert!(!entity.lifecycle.object_alive);
        assert!(!entity.in_logic_vector);
    }
    let listener = fatal.substrate.entities.get(30).unwrap();
    assert_eq!(
        listener.health.current, 193,
        "wood Verses scales 214 to 107"
    );
    let nested_fatal = fatal.substrate.entities.get(31).unwrap();
    assert_eq!(
        fatal.interner.resolve(nested_fatal.killed_by.unwrap()),
        "Soviet"
    );
    // Only the surviving listener (30) pings. The nested-fatal building (31)
    // takes `BuildingClass::ReceiveDamage` case 4, whose stock 8-frame timer
    // runs `ObjectClass::UnInit` (`0x005F6625` clears `IsAlive +0x90`) before
    // the `0x00442905` re-test, so `NotifyUnderAttack` never runs for it.
    assert_eq!(result.consequences.effects().under_attack_events.len(), 1);
    assert_eq!(
        (
            result.consequences.effects().under_attack_events[0].rx,
            result.consequences.effects().under_attack_events[0].ry
        ),
        (8, 5)
    );
    assert!(result.consequences.effects().under_attack_events[0].structure);
    assert!(!fatal.substrate.occupancy.contains_entity(8, 5, 10));
    let attacker = fatal.substrate.entities.get(20).unwrap();
    assert!(!attacker.radio_contacts.contains(10));
    assert!(attacker.attack_target.is_none());
    assert!(
        result
            .consequences
            .effects()
            .immediate_uninit_ids
            .is_empty()
    );
    assert_eq!(
        fatal.overlay_grid.as_ref().unwrap().cell(8, 5).overlay_id,
        None
    );
    assert_eq!(
        fatal.radar_terrain_dirty_cells,
        vec![
            (8, 5),
            (8, 3),
            (9, 4),
            (7, 4),
            (8, 4),
            (7, 6),
            (6, 5),
            (7, 5),
            (9, 6),
            (8, 7),
            (8, 6),
            (10, 5),
            (9, 5),
        ],
        "combat-result commit projects the complete DestroyOverlay visit stencil",
    );
    // ObjectClass::ReceiveDamage's exact-zero Destroy broadcast (0x005F57AF)
    // re-arms the attacker's passive scan (4..8) before TechnoClass's death
    // arm fires the DeathWeapon whose wall hit draws 0..400.
    let mut one_draw = SimRng::new(1);
    let _ = one_draw.next_range_u32_inclusive(4, 8);
    let _ = one_draw.next_range_u32_inclusive(0, 400);
    assert_eq!(fatal_rng, one_draw.state());

    let (boundary, boundary_result, boundary_rng) = run(11);
    assert!(boundary.substrate.pending_delete.is_empty());
    assert!(boundary.substrate.occupancy.contains_entity(8, 5, 10));
    for id in [30, 31] {
        let listener = boundary.substrate.entities.get(id).unwrap();
        assert_eq!(listener.health.current, if id == 30 { 300 } else { 107 });
    }
    assert!(
        boundary_result
            .consequences
            .effects()
            .under_attack_events
            .is_empty()
    );
    assert_eq!(
        boundary
            .substrate
            .entities
            .get(10)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .passengers,
        vec![11]
    );
    assert!(boundary.substrate.entities.get(11).unwrap().is_alive());
    assert!(
        boundary
            .substrate
            .entities
            .get(20)
            .unwrap()
            .radio_contacts
            .contains(10)
    );
    assert_eq!(
        boundary
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(8, 5)
            .overlay_id,
        Some(0)
    );
    assert!(boundary.radar_terrain_dirty_cells.is_empty());
    assert_eq!(boundary_rng, SimRng::new(1).state());
}

#[test]
fn gsi_04_11_bullet_ore_reduction_precedes_outer_crater_anim_start() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [Warheads]\n0=OREWH\n\
         [OverlayTypes]\n0=ORE\n\
         [SmudgeTypes]\n0=CR1\n\
         [Tiberiums]\n0=Riparius\n\
         [OREWH]\nCellSpread=0\nAnimList=EXPLOSION\nTiberium=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ORE]\nTiberium=yes\nChainReaction=yes\n\
         [Riparius]\nImage=1\nValue=25\n\
         [CR1]\nCrater=yes\nWidth=1\nHeight=1\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("bullet ore-order rules");
    rules.art_registry = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[EXPLOSION]\nCrater=yes\nScorch=no\n",
    ));
    // One raw frame: no middle frame, so Start runs Middle at construction.
    rules
        .art_registry
        .bind_anim_frame_count_for_test("EXPLOSION", 1);
    let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let ore_id = registry.id_for_name("ORE").expect("ORE overlay id");

    let mut sim = Simulation::with_seed(1);
    let mut terrain = gsi_04_10_clear_terrain(10, 10);
    for cell in &mut terrain.cells {
        cell.filled_clear = true;
        cell.accepts_smudge = true;
        cell.allows_tiberium = true;
    }
    sim.resolved_terrain = Some(terrain);
    sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(10, 10));
    sim.production.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(10, 10);
    let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(10, 10);
    overlay.place_overlay(5, 5, ore_id, 9);
    sim.overlay_grid = Some(overlay);
    let owner = sim.interner.intern("Americans");
    let detonation = crate::sim::projectile::ProjectileDetonation {
        projectile_id: 1,
        source_id: crate::sim::combat::RAD_NO_ATTACKER,
        target: crate::sim::projectile::ProjectileTarget::Cell { rx: 5, ry: 5 },
        impact: crate::sim::projectile::ProjectileCoord::new(5 * 256 + 128, 5 * 256 + 128, 0),
        payload: crate::sim::projectile::ProjectilePayload {
            base_damage: 100,
            warhead: sim.interner.intern("OREWH"),
            weapon: sim.interner.intern("TestWeapon"),
        },
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    };

    let result = sim.tick_combat_with_fatal_lifecycle(
        &rules,
        Some(&registry),
        100,
        &[],
        &BTreeSet::new(),
        &BTreeSet::new(),
        &[detonation],
        &[],
    );

    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
        None,
        "Apply_area_damage must clear ten-density ore before Bullet impact AnimClass::Start"
    );
    assert!(
        result
            .consequences
            .effects()
            .tiberium_reduction_requests
            .is_empty()
    );
    assert!(
        result
            .consequences
            .effects()
            .smudge_spawn_requests
            .is_empty()
    );
    // The receiver transaction's commit constructs the AnimList anim.
    let _ = result
        .consequences
        .commit(&mut sim, &rules, Some(&registry), None);
    assert!(
        sim.smudge_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .type_id
            .is_some(),
        "the outer crater must observe the already-cleared overlay cell"
    );
    let mut expected_rng = crate::sim::rng::SimRng::new(1);
    let _ = expected_rng.next_range_u32(1);
    assert_eq!(sim.scenario_rng.state(), expected_rng.state());
}

#[test]
fn gsi_04_11_missile_outer_anim_precedes_per_cell_ore_reduction() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [Warheads]\n0=MISSILEWH\n[OverlayTypes]\n0=ORE\n\
         [SmudgeTypes]\n0=CR1\n[Tiberiums]\n0=Riparius\n\
         [MISSILEWH]\nCellSpread=0\nAnimList=EXPLOSION\nTiberium=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [ORE]\nTiberium=yes\nChainReaction=yes\n\
         [Riparius]\nImage=1\nValue=25\n\
         [CR1]\nCrater=yes\nWidth=1\nHeight=1\n",
    );
    let mut rules = RuleSet::from_ini(&ini).expect("missile ore-order rules");
    rules.art_registry = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[EXPLOSION]\nCrater=yes\nScorch=no\nFrameWidth=100\nFrameHeight=100\n",
    ));
    let registry = crate::map::overlay_types::OverlayTypeRegistry::from_ini(&ini, None);
    let ore_id = registry.id_for_name("ORE").unwrap();
    let mut sim = Simulation::with_seed(7);
    let mut terrain = gsi_04_10_clear_terrain(10, 10);
    for cell in &mut terrain.cells {
        cell.filled_clear = true;
        cell.accepts_smudge = true;
        cell.allows_tiberium = true;
    }
    sim.resolved_terrain = Some(terrain);
    sim.smudge_grid = Some(crate::sim::smudge_grid::SmudgeGrid::new(10, 10));
    sim.production.ore_growth_state = crate::sim::ore_growth::OreGrowthState::new(10, 10);
    let mut overlay = crate::sim::overlay_grid::OverlayGrid::new(10, 10);
    overlay.place_overlay(5, 5, ore_id, 9);
    sim.overlay_grid = Some(overlay);
    let owner = sim.interner.intern("Americans");
    let missile_warhead = sim.interner.intern("MISSILEWH");
    sim.pending_missile_detonations
        .push(crate::sim::spawn_manager::MissileDetonation {
            rx: 5,
            ry: 5,
            warhead: missile_warhead,
            damage: 100,
            firer_id: crate::sim::combat::RAD_NO_ATTACKER,
            owner,
            impact: None,
        });
    let before_rng = sim.scenario_rng.state();

    let result = sim.tick_combat_with_fatal_lifecycle(
        &rules,
        Some(&registry),
        100,
        &[],
        &BTreeSet::new(),
        &BTreeSet::new(),
        &[],
        &[],
    );

    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(5, 5).overlay_id,
        None
    );
    assert!(
        sim.smudge_grid
            .as_ref()
            .unwrap()
            .cell(5, 5)
            .type_id
            .is_none(),
        "RocketLocomotion starts its crater Anim before the later ore sweep"
    );
    assert_eq!(sim.scenario_rng.state(), before_rng);
    assert!(
        result
            .consequences
            .effects()
            .tiberium_reduction_requests
            .is_empty()
    );
    assert!(
        result
            .consequences
            .effects()
            .smudge_spawn_requests
            .is_empty()
    );
}

#[test]
fn uninit_removes_all_structure_foundation_cells() {
    let mut sim = Simulation::new();
    let mut structure = GameEntity::test_default(10, "GAPOWR", "Americans", 4, 5);
    // Entity ids must come from the Simulation's own interner — test_default interns
    // into the thread-local test interner, which sim code never resolves against.
    structure.owner = sim.interner.intern("Americans");
    structure.type_ref = sim.interner.intern("GAPOWR");
    structure.category = EntityCategory::Structure;
    structure.foundation = "2x2".to_string();
    sim.substrate.entities.insert(structure);
    sim.reveal(10);
    sim.add_entity_occupancy(10);

    for cell in [(4, 5), (4, 6), (5, 5), (5, 6)] {
        assert!(sim.substrate.occupancy.contains_entity(cell.0, cell.1, 10));
    }

    sim.uninit(10);

    // Occupancy is unmarked synchronously in uninit, before the deferred free.
    for cell in [(4, 5), (4, 6), (5, 5), (5, 6)] {
        assert!(
            !sim.substrate.occupancy.contains_entity(cell.0, cell.1, 10),
            "uninit should clear foundation cell {cell:?}"
        );
    }
    // Two-phase: still resolvable-but-Dying until the drain frees the slot.
    assert!(sim.substrate.entities.get(10).is_some_and(|e| e.dying));
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(10).is_none());
    sim.debug_assert_logic_membership_consistent();
}

#[test]
fn unregister_live_object_clears_flag_when_vector_entry_is_missing() {
    let mut sim = Simulation::new();
    let entity = GameEntity::test_default(10, "HTNK", "Americans", 4, 5);
    sim.substrate.entities.insert(entity);
    sim.reveal(10);
    sim.substrate.logic.set_order_for_test(Vec::new());
    assert!(sim.substrate.entities.get(10).unwrap().in_logic_vector);

    sim.unregister_live_object(10);

    sim.debug_assert_logic_membership_consistent();
    assert!(sim.live_object_order_snapshot().is_empty());
    assert!(!sim.substrate.entities.get(10).unwrap().in_logic_vector);
}

/// A house whose tracked buildings and on-map units stand for objects the
/// fixture does not construct, in a non-campaign game, where the defeat gate
/// runs.
fn insert_house_with_counts(
    sim: &mut Simulation,
    name: &str,
    buildings: i32,
    units: i32,
) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern(name);
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10);
    house.tracking.set_buildings_for_test(buildings);
    house.tracking.set_active_units_for_test(units);
    sim.houses.insert(owner, house);
    sim.session.house_order.push(owner);
    sim.session.game_mode_nonzero = true;
    owner
}

/// The house rung's defeat pass, past frame zero as the gate requires.
fn check_defeat_now(sim: &mut Simulation, rules: Option<&RuleSet>) {
    sim.session.binary_frame = sim.session.binary_frame.max(1);
    sim.check_defeat(rules, None);
}

fn insert_test_entity_for_owner(
    sim: &mut Simulation,
    stable_id: u64,
    owner: crate::sim::intern::InternedId,
    type_id: &str,
    category: EntityCategory,
) {
    let owner_name = sim.interner.resolve(owner).to_string();
    let mut entity = GameEntity::test_default(stable_id, type_id, &owner_name, 10, 10);
    entity.owner = owner;
    entity.type_ref = sim.interner.intern(type_id);
    entity.category = category;
    sim.substrate.entities.insert(entity);
    // As its construction would: Add_Tracking.
    sim.update_house_tracking(
        stable_id,
        crate::sim::house_tracking::HouseTracking::add_tracking,
    );
}

#[test]
fn gsi_05_16_change_owner_moves_live_category_counts_once_and_noops() {
    let mut sim = Simulation::new();
    let old_owner = insert_house_with_counts(&mut sim, "Americans", 0, 0);
    let new_owner = insert_house_with_counts(&mut sim, "Russians", 0, 0);
    insert_test_entity_for_owner(&mut sim, 1, old_owner, "GAPOWR", EntityCategory::Structure);
    insert_test_entity_for_owner(&mut sim, 2, old_owner, "MTNK", EntityCategory::Unit);
    let counts = |sim: &Simulation, owner| {
        let tracking = &sim.houses[&owner].tracking;
        (tracking.buildings_for_test(), tracking.units_for_test())
    };
    assert_eq!(counts(&sim, old_owner), (1, 1));

    sim.change_owner(1, new_owner);
    sim.change_owner(2, new_owner);

    assert_eq!(counts(&sim, old_owner), (0, 0));
    assert_eq!(counts(&sim, new_owner), (1, 1));
    assert_eq!(sim.substrate.entities.get(1).unwrap().owner, new_owner);
    assert_eq!(sim.substrate.entities.get(2).unwrap().owner, new_owner);

    sim.change_owner(1, new_owner);
    sim.change_owner(999, old_owner);

    assert_eq!(counts(&sim, old_owner), (0, 0));
    assert_eq!(counts(&sim, new_owner), (1, 1));
}

/// Create a CommandEnvelope with a string owner, interning it via the sim's interner.
fn cmd_envelope(
    sim: &Simulation,
    owner: &str,
    execute_tick: u64,
    payload: Command,
) -> CommandEnvelope {
    let owner_id = sim
        .interner
        .get(owner)
        .unwrap_or_else(|| panic!("owner '{}' not interned", owner));
    CommandEnvelope::new(owner_id, execute_tick, payload)
}

#[test]
fn despawn_entity_clears_live_radio_contacts() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    let htnk = sim.interner.intern("HTNK");
    let mtnk = sim.interner.intern("MTNK");
    let mut despawned = GameEntity::test_default(1, "HTNK", "Americans", 10, 10);
    let mut survivor = GameEntity::test_default(2, "MTNK", "Americans", 11, 10);

    despawned.owner = owner;
    despawned.type_ref = htnk;
    despawned.mark_live_contact_with(2);
    survivor.owner = owner;
    survivor.type_ref = mtnk;
    survivor.mark_live_contact_with(1);
    sim.substrate.entities.insert(despawned);
    sim.substrate.entities.insert(survivor);
    assert!(matches!(sim.reveal(1), RevealOutcome::Revealed { .. }));
    assert!(matches!(sim.reveal(2), RevealOutcome::Revealed { .. }));

    sim.despawn_entity(1);

    // Radio contacts are cleared synchronously in uninit, before the deferred free;
    // the despawned entity stays resolvable-but-Dying until the drain.
    assert!(sim.substrate.entities.get(1).is_some_and(|e| e.dying));
    assert!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .radio_contacts
            .is_empty()
    );
    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(1).is_none());
}

/// Create a water terrain grid (all cells are water, land_type=4) for ship tests.
fn water_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    water_terrain_with_land_type(width, height, 4, false)
}

fn gsi_04_10_clear_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};

    let speed_costs = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    let mut terrain = water_terrain(width, height);
    for cell in &mut terrain.cells {
        cell.land_type = LandType::Clear.as_index();
        cell.yr_cell_land_type = LandType::Clear.as_index();
        cell.terrain_class = TerrainClass::Clear;
        cell.speed_costs = speed_costs;
        cell.is_water = false;
        cell.ground_walk_blocked = false;
        cell.zone_type = zone_class::GROUND;
        cell.base_ground_walk_blocked = false;
        cell.base_build_blocked = false;
        cell.base_land_type = LandType::Clear.as_index();
        cell.base_yr_cell_land_type = LandType::Clear.as_index();
        cell.base_terrain_class = TerrainClass::Clear;
        cell.base_speed_costs = speed_costs;
        cell.build_blocked = false;
    }
    terrain
}

#[test]
fn gsi_04_15_active_tube_leaf_preempts_unit_and_infantry_mission_host() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\n\
         [InfantryTypes]\n0=TESTINF\n\
         [VehicleTypes]\n0=TESTUNIT\n\
         [AircraftTypes]\n[BuildingTypes]\n0=TESTBUILD\n\
         [TESTINF]\nSpeed=4\nPrimary=TESTGUN\n\
         [TESTUNIT]\nSpeed=4\nPrimary=TESTGUN\n\
         [TESTBUILD]\nStrength=100\nArmor=none\nPrimary=TESTGUN\nThreatPosed=30\n\
         [TESTGUN]\nDamage=1\nROF=100\nRange=6\nWarhead=TESTWH\n\
         [TESTWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .expect("TubeMovement host rules");

    for (category, type_name) in [
        (EntityCategory::Unit, "TESTUNIT"),
        (EntityCategory::Infantry, "TESTINF"),
    ] {
        let mut clear = gsi_04_10_clear_terrain(3, 1);
        clear.cells[0].tube_index = Some(TubeId(0));
        let terrain = ResolvedTerrainGrid::from_cells_with_tubes(
            3,
            1,
            clear.cells,
            vec![TubeFact::explicit((0, 0), (1, 0), 2, vec![2])],
        );
        let path = PathGrid::from_resolved_terrain(&terrain);

        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Americans");
        let type_ref = sim.interner.intern(type_name);
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            1,
            0,
            0,
            0,
            owner,
            crate::sim::components::Health { current: 100 },
            type_ref,
            category,
            0,
            5,
            true,
        );
        entity.lifecycle.in_limbo = false;
        entity.lifecycle.cell_marked = false;
        if category == EntityCategory::Unit {
            entity.order_intent = Some(crate::sim::components::OrderIntent::AttackMove {
                goal_rx: 2,
                goal_ry: 0,
            });
        } else {
            entity.attack_target = Some(AttackTarget::for_cell(0, 0));
            entity.passively_acquired_target = true;
            entity.c4_plant = Some(crate::sim::components::C4PlantState {
                target_building_id: 2,
            });
        }
        entity.locomotor = Some(LocomotorState::for_test_kind(match category {
            EntityCategory::Unit => LocomotorKind::Drive,
            EntityCategory::Infantry => LocomotorKind::Walk,
            _ => unreachable!("fixture is Foot only"),
        }));
        if category == EntityCategory::Unit {
            entity.drive_locomotion = Some(DriveLocomotionRuntime::default());
        } else {
            entity.capture_target = Some(2);
        }
        for _ in 0..41 {
            entity.mission.increment_ai_counter();
        }
        entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
            tube_id: TubeId(0),
            cursor: 1,
            target: DriveCoord {
                x: 384,
                y: 128,
                z: 17,
            },
        });
        sim.substrate.entities.insert(entity);
        let original_building_owner = sim.interner.intern("Russians");
        // Keep the later combat/capture target adjacent to the Tube exit.
        // A marked building on the exit itself correctly blocks finalization.
        let mut building = GameEntity::new_at_frame_zero_for_test(
            2,
            2,
            0,
            0,
            0,
            original_building_owner,
            crate::sim::components::Health { current: 100 },
            sim.interner.intern("TESTBUILD"),
            EntityCategory::Structure,
            0,
            5,
            true,
        );
        building.lifecycle.in_limbo = false;
        sim.substrate.entities.insert(building);
        sim.add_entity_occupancy(2);
        sim.fog = crate::sim::vision::FogState {
            width: 3,
            height: 1,
            ..Default::default()
        };
        crate::sim::vision::reveal_radius(&mut sim.fog, owner, 0, 0, 3);
        sim.set_logic_order_for_test(vec![1]);
        sim.mission_queue_exact(
            1,
            MissionId::from_known(MissionType::Move),
            0,
            0,
            &crate::sim::mission::authority::EntityReadyInputProvider,
        )
        .expect("queue remains pending through TubeMovement");
        sim.terrain_costs =
            crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(&terrain);
        sim.resolved_terrain = Some(terrain);

        sim.advance_tick(&[], Some(&rules), &empty_heights(), Some(&path), None, 67);

        {
            let entity = sim.substrate.entities.get(1).expect("Tube mover survives");
            assert_eq!(entity.mission.ai_counter(), 41, "{category:?}");
            assert_eq!(
                entity.mission.queued(),
                MissionId::from_known(MissionType::Move),
                "{category:?} skips both mission-promotion checkpoints"
            );
            assert!(entity.low_bridge_tube_state.is_none(), "{category:?}");
            assert!(entity.lifecycle.cell_marked, "{category:?}");
            assert!(
                sim.substrate
                    .occupancy
                    .contains_entity(1, 0, entity.stable_id),
                "{category:?} final Tube leaf runs exactly once and restores occupancy"
            );
            if category == EntityCategory::Unit {
                assert!(entity.attack_target.is_none());
                assert!(!entity.passively_acquired_target);
                assert!(entity.movement_target.is_none());
                assert!(matches!(
                    entity.order_intent,
                    Some(crate::sim::components::OrderIntent::AttackMove {
                        goal_rx: 2,
                        goal_ry: 0
                    })
                ));
            } else {
                assert!(entity.passively_acquired_target);
                assert!(matches!(
                    entity.attack_target.as_ref().map(|target| target.target),
                    Some(crate::sim::combat::TargetKind::Cell(0, 0))
                ));
                assert_eq!(entity.capture_target, Some(2));
                assert!(entity.c4_plant.is_some());
                assert!(
                    entity.movement_target.is_none(),
                    "Tube final skips C4 Mission_Enter, which would move into the adjacent building"
                );
                assert_eq!(
                    sim.substrate.entities.get(2).map(|building| building.owner),
                    Some(original_building_owner),
                    "Tube final returns before Mission_Capture; ownership changes next visit"
                );
                assert!(
                    sim.substrate
                        .entities
                        .get(2)
                        .is_some_and(|building| building.pending_c4_detonation.is_none()),
                    "Tube final returns before Mission_Enter; no C4 claim is made"
                );
            }
        }
        if category == EntityCategory::Unit {
            // `TESTBUILD` carries `Primary=`/`ThreatPosed=30` so that it stays a
            // legal auto-acquire target: the human-attacker building gate in
            // `TechnoClass::Evaluate_Candidate @ 0x006F85AB` refuses an enemy
            // building with no weapon or no posed threat, which is a targeting
            // fact this Tube-ordering test is not about.
            sim.tick_order_intents_pre_combat(&rules, None, &std::collections::BTreeSet::new());
            assert!(matches!(
                sim.substrate
                    .entities
                    .get(1)
                    .and_then(|entity| entity.attack_target.as_ref())
                    .map(|target| target.target),
                Some(crate::sim::combat::TargetKind::Entity(2))
            ));
        }
    }
}

fn gsi_04_10_terrain_object(
    sim: &mut Simulation,
    stable_id: u64,
    cell: (u16, u16),
    occupation_bits: u8,
) -> crate::sim::terrain_object::TerrainObjectState {
    crate::sim::terrain_object::TerrainObjectState {
        stable_id,
        native_unique_id: None,
        in_logic_vector: false,
        type_ref: sim.interner.intern("TREE01"),
        rx: cell.0,
        ry: cell.1,
        health: 10,
        max_health: 10,
        occupation_bits,
        lifecycle: crate::sim::terrain_object::TerrainObjectLifecycle::Live,
    }
}

#[test]
fn gsi_04_10_in_tick_refresh_updates_tail_path_and_cost_before_consumers() {
    use crate::rules::locomotor_type::SpeedType;
    use crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids;
    use crate::sim::terrain_object::{mark_terrain_occupation, unmark_terrain_occupation};

    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(gsi_04_10_clear_terrain(2, 1));
    let tree = gsi_04_10_terrain_object(&mut sim, 1, (0, 0), 7);
    {
        let (production, terrain) = (&mut sim.production, &mut sim.resolved_terrain);
        mark_terrain_occupation(production, &tree, terrain.as_mut());
    }
    sim.terrain_costs = build_canonical_terrain_cost_grids(
        sim.resolved_terrain.as_ref().expect("resolved terrain"),
    );
    let mut input_path_grid = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().expect("resolved terrain"),
        sim.bridge_state.as_ref(),
    );
    input_path_grid.set_blocked(1, 0, true);
    assert!(!input_path_grid.is_walkable(0, 0));
    assert_eq!(sim.terrain_costs[&SpeedType::Track].cost_at(0, 0), 0);

    {
        let (production, terrain) = (&mut sim.production, &mut sim.resolved_terrain);
        unmark_terrain_occupation(production, &tree, terrain.as_mut());
    }
    let tail_path_grid =
        sim.refresh_navigation_after_terrain_changes(Some(&input_path_grid), &[(0, 0)]);
    let phase_six_consumer_grid = tail_path_grid.as_ref().or(Some(&input_path_grid));
    let phase_six_consumer_grid = phase_six_consumer_grid.expect("tail grid");

    assert!(phase_six_consumer_grid.is_walkable(0, 0));
    assert_eq!(phase_six_consumer_grid.terrain_object_cell_bits_at(0, 0), 0);
    assert!(phase_six_consumer_grid.is_walkable_for_infantry(0, 0));
    assert!(
        !phase_six_consumer_grid.is_walkable(1, 0),
        "unrelated dynamic blockers from the input grid must survive"
    );
    assert_eq!(sim.terrain_costs.len(), SpeedType::ALL_WITH_COSTS.len());
    assert_eq!(
        sim.terrain_costs[&SpeedType::Track].cost_at(0, 0),
        100,
        "Phase 6+ must see the rebuilt cost authority in the lethal-event tick"
    );
}

#[test]
fn gsi_04_10_zero_occupation_removal_forces_ground_zone_with_same_walkability() {
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::locomotor_type::{MovementZone, SpeedType};
    use crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids;
    use crate::sim::pathfinding::zone_map::ZONE_INVALID;
    use crate::sim::terrain_object::{mark_terrain_occupation, unmark_terrain_occupation};

    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(gsi_04_10_clear_terrain(1, 1));
    let tree = gsi_04_10_terrain_object(&mut sim, 1, (0, 0), 0);
    {
        let (production, terrain) = (&mut sim.production, &mut sim.resolved_terrain);
        mark_terrain_occupation(production, &tree, terrain.as_mut());
    }
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(0, 0)
            .unwrap()
            .zone_type,
        zone_class::BUILDING
    );
    sim.terrain_costs = build_canonical_terrain_cost_grids(
        sim.resolved_terrain.as_ref().expect("resolved terrain"),
    );
    let input_path_grid = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().expect("resolved terrain"),
        sim.bridge_state.as_ref(),
    );
    assert!(input_path_grid.is_walkable(0, 0));
    assert_eq!(sim.terrain_costs[&SpeedType::Track].cost_at(0, 0), 100);
    sim.rebuild_zone_grid_full(&input_path_grid);
    assert_eq!(
        sim.zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(MovementZone::Normal))
            .expect("normal zone map")
            .zone_at(0, 0, MovementLayer::Ground),
        ZONE_INVALID,
        "OccupationBits=0 is a reduced Building zone even though PathGrid is walkable"
    );

    {
        let (production, terrain) = (&mut sim.production, &mut sim.resolved_terrain);
        unmark_terrain_occupation(production, &tree, terrain.as_mut());
    }
    let tail_path_grid = sim
        .refresh_navigation_after_terrain_changes(Some(&input_path_grid), &[(0, 0)])
        .expect("tail grid");

    assert_eq!(tail_path_grid, input_path_grid);
    assert_eq!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(0, 0)
            .unwrap()
            .zone_type,
        zone_class::GROUND
    );
    assert_ne!(
        sim.zone_grid
            .as_ref()
            .and_then(|zones| zones.map_for(MovementZone::Normal))
            .expect("normal zone map")
            .zone_at(0, 0, MovementLayer::Ground),
        ZONE_INVALID,
        "forced rebuild must observe the reduced-zone change despite identical PathGrid cells"
    );
}

fn water_terrain_with_land_type(
    width: u16,
    height: u16,
    land_type: u8,
    is_cliff_like: bool,
) -> ResolvedTerrainGrid {
    let speed_costs = crate::rules::terrain_rules::SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    let mut cells = Vec::new();
    for y in 0..height {
        for x in 0..width {
            cells.push(crate::map::resolved_terrain::ResolvedTerrainCell {
                rx: x,
                ry: y,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: false,
                tileset_index: Some(0),
                land_type,
                yr_cell_land_type: land_type,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: crate::rules::terrain_rules::TerrainClass::Clear,
                speed_costs,
                is_water: true,
                is_cliff_like,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 4,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: speed_costs,
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    ResolvedTerrainGrid::from_cells(width, height, cells)
}

fn single_bridge_cell(rx: u16, ry: u16, deck_level: u8) -> ResolvedTerrainGrid {
    let mut cells = Vec::new();
    for y in 0..=ry {
        for x in 0..=rx {
            cells.push(crate::map::resolved_terrain::ResolvedTerrainCell {
                rx: x,
                ry: y,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: false,
                tileset_index: Some(0),
                land_type: 0,
                yr_cell_land_type: 0,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: crate::rules::terrain_rules::TerrainClass::Clear,
                speed_costs: crate::rules::terrain_rules::SpeedCostProfile::default(),
                is_water: false,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: Default::default(),
                build_blocked: false,
                has_bridge_deck: x == rx && y == ry,
                bridge_walkable: x == rx && y == ry,
                bridge_transition: x == rx && y == ry,
                bridge_deck_level: if x == rx && y == ry { deck_level } else { 0 },
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    ResolvedTerrainGrid::from_cells(rx + 1, ry + 1, cells)
}

fn bridge_cell_with_ground_block(
    rx: u16,
    ry: u16,
    deck_level: u8,
    ground_walk_blocked: bool,
    level: u8,
) -> ResolvedTerrainGrid {
    let mut terrain = single_bridge_cell(rx, ry, deck_level);
    let water_speed_costs = crate::rules::terrain_rules::SpeedCostProfile {
        float: Some(100),
        ..Default::default()
    };
    for cell in &mut terrain.cells {
        cell.land_type = 4;
        cell.yr_cell_land_type = 4;
        cell.speed_costs = water_speed_costs;
        cell.is_water = true;
        cell.zone_type = 4;
        cell.base_land_type = 4;
        cell.base_yr_cell_land_type = 4;
        cell.base_speed_costs = water_speed_costs;
    }
    let idx = terrain.index(rx, ry).expect("bridge index");
    let cell = &mut terrain.cells[idx];
    cell.level = level;
    cell.ground_walk_blocked = ground_walk_blocked;
    cell.is_water = ground_walk_blocked;
    cell.base_build_blocked = ground_walk_blocked;
    cell.build_blocked = true;
    terrain
}

/// Build a 3-cell EW bridge strip centered at `(center_rx, ry)`, with the
/// `bridge_state` pre-classified so the orchestrator's HighDirect path
/// fires the walker (overlay 0xDC → final-stage collapse on all 3 cells).
///
/// All 3 bridge cells share the same `level`, `deck_level`, and
/// `ground_walk_blocked` flag. Caller mutates extras like `overlay_blocks`
/// / `terrain_object_blocks` / `is_cliff_like` on the returned terrain
/// before constructing the simulation if a specific fallout shape is
/// being asserted (mutate the center cell at `center_rx, ry`).
///
/// Constraints: `center_rx >= 1` (strip needs the west neighbor in-grid).
fn install_rectangular_test_playfield(sim: &mut Simulation, width: u16, height: u16) {
    let span = i32::from(width.max(height));
    sim.playfield_bounds = Some(crate::map::playfield::PlayfieldBounds {
        base: 0,
        off_fc: -span,
        off_100: -span,
        off_104: span * 2,
        off_108: span * 2,
    });
}

fn ew_high_bridge_strip_for_dispatch(
    center_rx: u16,
    ry: u16,
    deck_level: u8,
    ground_walk_blocked: bool,
    level: u8,
) -> (ResolvedTerrainGrid, BridgeRuntimeState) {
    use crate::map::resolved_terrain::ResolvedTerrainCell;
    use crate::sim::bridge_state::{Axis, BridgeCellRole, BridgeRuntimeCell, DamageState};
    assert!(center_rx >= 1, "EW strip needs west neighbor in-grid");

    let width = center_rx + 2; // 0..=(center_rx + 1)
    let height = ry + 1;
    let west = center_rx - 1;
    let east = center_rx + 1;
    let speed_costs = crate::rules::terrain_rules::SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };

    let mut cells = Vec::with_capacity(width as usize * height as usize);
    for y in 0..height {
        for x in 0..width {
            let on_bridge = y == ry && x >= west && x <= east;
            cells.push(ResolvedTerrainCell {
                rx: x,
                ry: y,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: if on_bridge { level } else { 0 },
                filled_clear: false,
                tileset_index: Some(0),
                land_type: 0,
                yr_cell_land_type: 0,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: crate::rules::terrain_rules::TerrainClass::Clear,
                speed_costs,
                is_water: on_bridge && ground_walk_blocked,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: on_bridge && ground_walk_blocked,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: on_bridge && ground_walk_blocked,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: speed_costs,
                build_blocked: on_bridge,
                has_bridge_deck: on_bridge,
                bridge_walkable: on_bridge,
                bridge_transition: on_bridge,
                bridge_deck_level: if on_bridge { deck_level } else { 0 },
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts {
                    raw_flags: if on_bridge {
                        crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                    } else {
                        0
                    },
                    ..Default::default()
                },
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    let resolved = ResolvedTerrainGrid::from_cells(width, height, cells);

    // Build bridge state, then override the 3 deck cells with overlay 0xDC
    // (HIGH EW final-eligible). The HighDirect dispatcher path matches on
    // overlay alone (no Z-gate, no role check), so a single hit at any of
    // the 3 cells drives the walker to write 0xE8 / Destroyed across the
    // (this, west, east) triple.
    let mut state = BridgeRuntimeState::from_resolved_terrain(&resolved, true, 15);
    for x in west..=east {
        state.test_seed_cell(
            x,
            ry,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level,
                bridge_group_id: Some(1),
                damage_state: DamageState::Healthy { variant: 0 },
                axis: Some(Axis::EW),
                role: BridgeCellRole::Body,
                anchor_span_id: Some(1),
                overlay_byte: 0xDC,
                bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
            },
        );
    }
    (resolved, state)
}

fn alliance_map(pairs: &[(&str, &[&str])]) -> HouseAllianceMap {
    let mut map = HouseAllianceMap::default();
    for &(owner, allies) in pairs {
        let mut set = std::collections::BTreeSet::new();
        for ally in allies {
            set.insert(ally.trim().to_ascii_uppercase());
        }
        map.insert(owner.trim().to_ascii_uppercase(), set);
    }
    map
}

fn combat_test_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n1=AMCV\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GACNST\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\nPrimary=M60\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
         [AMCV]\nStrength=450\nArmor=heavy\nSpeed=5\nPrimary=none\nDeploysInto=GACNST\n\n\
         [GACNST]\nStrength=1000\nArmor=wood\nFoundation=4x3\nConstructionYard=yes\nUndeploysInto=AMCV\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nWarhead=SA\n\n\
         [105mm]\nDamage=65\nROF=50\nRange=6\nWarhead=AP\n\n\
         [SA]\nVerses=100%,100%,100%,90%,70%,25%,100%,25%,25%,0%,0%\n\n\
         [AP]\nVerses=100%,100%,90%,75%,75%,75%,60%,30%,20%,0%,0%\n",
    );
    RuleSet::from_ini(&ini).expect("combat test rules should parse")
}

fn sonic_wave_test_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=DLPH\n1=TARGET\n\n\
         [DLPH]\nStrength=200\nArmor=light\nSpeed=8\nPrimary=SonicZap\nElitePrimary=SonicZapE\n\n\
         [TARGET]\nStrength=100\nArmor=wood\n\n\
         [SonicZap]\nDamage=4\nAmbientDamage=10\nROF=20\nRange=6\nProjectile=Sonic\nSpeed=100\nWarhead=SonicWH\nIsSonic=yes\n\n\
         [SonicZapE]\nDamage=8\nAmbientDamage=15\nROF=20\nRange=6\nProjectile=Sonic\nSpeed=100\nWarhead=SonicWH\nIsSonic=yes\n\n\
         [Sonic]\nLevel=yes\n\n\
         [SonicWH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("Sonic Wave fixture")
}

fn sonic_tail_order_test_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=DLPH\n1=LATER\n2=TARGET\n\n\
         [DLPH]\nStrength=200\nArmor=light\nSpeed=8\nSight=8\nPrimary=SonicZap\n\n\
         [LATER]\nStrength=200\nArmor=light\nSpeed=8\nSight=8\nPrimary=LaterGun\n\n\
         [TARGET]\nStrength=100\nArmor=none\nSpeed=1\n\n\
         [SonicZap]\nDamage=4\nAmbientDamage=10\nROF=20\nRange=6\nProjectile=Sonic\nSpeed=100\nWarhead=SonicWH\nIsSonic=yes\n\n\
         [LaterGun]\nDamage=7\nROF=20\nRange=6\nWarhead=LaterWH\n\n\
         [Sonic]\nLevel=yes\n\n\
         [SonicWH]\nWood=yes\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n\n\
         [LaterWH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,0%,0%\n",
    ))
    .expect("Sonic Logic-tail ordering fixture")
}

fn admit_test_wave(sim: &mut Simulation, rules: &RuleSet, event: &SimFireEvent) {
    let wave = sim
        .prepare_fired_wave(
            rules,
            event,
            &sim.substrate.entities,
            &sim.interner,
            sim.resolved_terrain.as_ref(),
        )
        .expect("wave prepared");
    let terrain = sim.resolved_terrain.take();
    sim.admit_fired_wave(event.attacker_id, wave, terrain.as_ref());
    sim.resolved_terrain = terrain;
}

fn sonic_fire_event(sim: &mut Simulation, attacker_id: u64, target_id: u64) -> SimFireEvent {
    SimFireEvent {
        attacker_id,
        attacker_type_ref: sim.interner.intern("DLPH"),
        weapon_slot: crate::sim::combat::combat_weapon::WeaponSlot::Primary,
        weapon_id: sim.interner.intern("SonicZap"),
        facing: 0,
        veterancy: 0,
        origin_snapshot: FireOriginSnapshot {
            rx: 0,
            ry: 0,
            sub_x: SimFixed::from_num(0),
            sub_y: SimFixed::from_num(0),
            z: 0,
            facing: 0,
        },
        target: crate::sim::combat::TargetKind::Entity(target_id),
        report_sound_id: None,
        fire_coord: crate::sim::projectile::ProjectileCoord::new(0, 0, 0),
        fire_offset_y: 0,
        muzzle_anim: None,
        occupied_building: false,
        firer_category: EntityCategory::Unit,
    }
}

#[test]
fn sonic_constructor_dead_pointer_link_lives_until_deferred_delete_at_239() {
    let rules = sonic_wave_test_rules();
    let mut sim = Simulation::new();
    let firer_id = sim.allocate_stable_id();
    let target_id = sim.allocate_stable_id();
    let owner = sim.interner.intern("Americans");
    let firer_type = sim.interner.intern("DLPH");
    let target_type = sim.interner.intern("TARGET");
    let mut firer = GameEntity::test_default(firer_id, "DLPH", "Americans", 0, 0);
    firer.owner = owner;
    firer.type_ref = firer_type;
    firer.position.sub_x = SimFixed::from_num(0);
    firer.position.sub_y = SimFixed::from_num(0);
    let mut target = GameEntity::test_default(target_id, "TARGET", "Russians", 0, 0);
    target.type_ref = target_type;
    target.position.sub_x = SimFixed::from_num(239);
    target.position.sub_y = SimFixed::from_num(0);
    sim.substrate.entities.insert(firer);
    sim.substrate.entities.insert(target);
    let event = sonic_fire_event(&mut sim, firer_id, target_id);

    admit_test_wave(&mut sim, &rules, &event);

    let wave_id = *sim
        .active_wave_links
        .get(&firer_id)
        .expect("dead pointer stored");
    assert!(sim.waves.get(wave_id).is_some());
    assert!(!sim.waves.get(wave_id).unwrap().in_logic_vector);
    assert_eq!(sim.substrate.pending_delete, vec![wave_id]);

    sim.process_pending_delete();
    assert!(!sim.active_wave_links.contains_key(&firer_id));
    assert!(sim.waves.get(wave_id).is_none());
}

#[test]
fn sonic_constructor_at_240_registers_then_runs_at_same_pass_tail() {
    let rules = sonic_wave_test_rules();
    let mut sim = Simulation::new();
    let firer_id = sim.allocate_stable_id();
    let target_id = sim.allocate_stable_id();
    let owner = sim.interner.intern("Americans");
    let firer_type = sim.interner.intern("DLPH");
    let target_type = sim.interner.intern("TARGET");
    let mut firer = GameEntity::test_default(firer_id, "DLPH", "Americans", 0, 0);
    firer.owner = owner;
    firer.type_ref = firer_type;
    firer.position.sub_x = SimFixed::from_num(0);
    firer.position.sub_y = SimFixed::from_num(0);
    let mut target = GameEntity::test_default(target_id, "TARGET", "Russians", 0, 0);
    target.type_ref = target_type;
    target.position.sub_x = SimFixed::from_num(240);
    target.position.sub_y = SimFixed::from_num(0);
    sim.substrate.entities.insert(firer);
    sim.substrate.entities.insert(target);
    let event = sonic_fire_event(&mut sim, firer_id, target_id);

    admit_test_wave(&mut sim, &rules, &event);

    let wave_id = *sim
        .active_wave_links
        .get(&firer_id)
        .expect("live owner link");
    let wave = sim.waves.get(wave_id).expect("registered Wave");
    assert!(wave.in_logic_vector);
    assert_eq!(wave.lifetime, 100, "FireAt only registers the Logic tail");

    sim.visit_combat_tail(0, &rules, None);

    let wave = sim.waves.get(wave_id).expect("Wave survives first tail AI");
    assert_eq!(wave.lifetime, 99, "first AI belongs to the firing pass");
    assert_eq!(
        wave.target.z, 50,
        "type-0 uses the live target-side Sonic Z adjustment"
    );
    assert!(sim.substrate.pending_delete.is_empty());
}

#[test]
fn sonic_cell_target_uses_persistent_dummy_gettargetcoords_on_create_and_refresh() {
    let rules = sonic_wave_test_rules();
    let mut sim = Simulation::new();
    let terrain = ResolvedTerrainGrid::from_cells(0, 0, Vec::new());
    terrain.test_set_dummy_cell_level_slope(2, 0);
    let process_dummy = terrain.shared_cell_dummy();
    process_dummy.set_bridge_flags_0x1180(crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL);
    sim.install_resolved_terrain_for_new_map(terrain);
    assert!(
        process_dummy.same_identity(&sim.effective_shared_cell_dummy()),
        "the Wave lookup must retain the map's process-global dummy identity",
    );

    let firer_id = sim.allocate_stable_id();
    let owner = sim.interner.intern("Americans");
    let firer_type = sim.interner.intern("DLPH");
    let mut firer = GameEntity::test_default(firer_id, "DLPH", "Americans", 0, 0);
    firer.owner = owner;
    firer.type_ref = firer_type;
    firer.position.sub_x = SimFixed::from_num(0);
    firer.position.sub_y = SimFixed::from_num(0);
    firer.attack_target = Some(AttackTarget::for_cell(u16::MAX, 7));
    sim.substrate.entities.insert(firer);

    let mut event = sonic_fire_event(&mut sim, firer_id, u64::MAX);
    event.target = crate::sim::combat::TargetKind::Cell(u16::MAX, 7);
    admit_test_wave(&mut sim, &rules, &event);

    let wave_id = *sim
        .active_wave_links
        .get(&firer_id)
        .expect("off-map Cell target produces a live Wave");
    let wave = sim.waves.get(wave_id).expect("registered Wave");
    assert_eq!(
        wave.target,
        crate::sim::projectile::ProjectileCoord::new(-128, 1_920, 674),
        "CellClass ground 2*104 plus structural +416 and Sonic +50",
    );
    assert_eq!(process_dummy.snapshot().coord, (-1, 7));

    process_dummy.stamp_coord(-9, -9);
    let context = sim.wave_update_context(wave_id);
    assert_eq!(process_dummy.snapshot().coord, (-1, 7));
    assert_eq!(
        context.target_position,
        Some(crate::sim::projectile::ProjectileCoord::new(
            -128, 1_920, 624,
        )),
        "every live refresh re-enters GetCellClass then GetTargetCoords",
    );
    assert_eq!(process_dummy.snapshot().level, 2);
    assert_eq!(
        process_dummy.snapshot().bridge_flags_0x1180,
        crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL,
        "coordinate restamps preserve the dummy's live non-coordinate fields",
    );

    sim.visit_combat_tail(0, &rules, None);
    let wave = sim.waves.get(wave_id).expect("Wave survives first live AI");
    assert_eq!(wave.lifetime, 99);
    assert_eq!(wave.target.z, 674);
    assert_eq!(
        process_dummy.snapshot().coord,
        (0, 6),
        "UpdateCells keeps the shared identity but leaves the final miss restamp live",
    );
}

/// Chain 1 end to end through the production frame: a GI placed on the map
/// enters Guard on Unlimbo (`0x0051CBA0`), its passive block
/// (`0x006FA65A`) scans the enemy infantryman into range, and it fires Inviso
/// bullets that land in each frame's Logic tail until the target dies, then
/// holds no target.
#[test]
fn a_gi_on_guard_acquires_an_enemy_infantryman_and_fires_until_it_dies() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n[VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
         [E1]\nStrength=125\nArmor=none\nSpeed=4\nSight=5\nPrimary=M60\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\n\
         [M60]\nDamage=25\nROF=20\nRange=5\nProjectile=InvisibleLow\nSpeed=100\nWarhead=SA\n\n\
         [InvisibleLow]\nInviso=yes\nImage=none\n\n\
         [SA]\nVerses=100%,80%,80%,50%,25%,25%,75%,50%,25%,100%,100%\n",
    ))
    .expect("GI fixture");
    let mut sim = Simulation::with_seed(0x5EED_0061);
    sim.input_delay_ticks = 0;
    let terrain = ResolvedTerrainGrid::from_cells(
        12,
        3,
        (0..3)
            .flat_map(|ry| (0..12).map(move |rx| bridgehead_base_cell(rx, ry)))
            .collect(),
    );
    install_rectangular_test_playfield(&mut sim, terrain.width(), terrain.height());
    sim.install_resolved_terrain_for_new_map(terrain);
    let heights = empty_heights();
    let gi = sim
        .spawn_object("E1", "Americans", 2, 1, 64, &rules, &heights)
        .expect("GI");
    let enemy = sim
        .spawn_object("E1", "Russians", 5, 1, 64, &rules, &heights)
        .expect("enemy infantryman");
    sim.path_grid = Some(std::sync::Arc::new(PathGrid::test_all_passable(12, 3)));
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    runtime.resources.rules = rules;
    runtime.resources.height_map = heights;

    let mut gi_shots = 0;
    let mut enemy_health = Vec::new();
    for _ in 0..600 {
        let output = runtime
            .advance_frame(&[], 67, TickLane::Ordinary)
            .expect("fixture frame must complete");
        assert!(output.tick.frame_committed);
        let sim = &runtime.simulation;
        gi_shots += output
            .fire_events
            .iter()
            .filter(|event| event.attacker_id == gi)
            .count();
        match sim.substrate.entities.get(enemy) {
            Some(target) if !target.dying => enemy_health.push(target.health.current),
            _ => break,
        }
    }
    let sim = &runtime.simulation;
    assert!(
        sim.substrate
            .entities
            .get(enemy)
            .is_none_or(|target| target.dying),
        "the enemy infantryman dies"
    );
    enemy_health.dedup();
    assert_eq!(enemy_health.first(), Some(&125));
    assert!(
        enemy_health.windows(2).all(|pair| pair[0] - pair[1] == 25),
        "every hit is one M60 bullet: {enemy_health:?}"
    );
    assert_eq!(gi_shots, 5, "five 25-damage shots kill 125 HP");
    // The enemy fired back, which put the GI on Attack (retaliation). With its
    // target gone, Mission_Attack's next visits return it to Guard, where it
    // idles holding nothing.
    let guard = crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard);
    for _ in 0..60 {
        if runtime
            .simulation
            .substrate
            .entities
            .get(gi)
            .unwrap()
            .mission
            .current()
            == guard
        {
            break;
        }
        runtime
            .advance_frame(&[], 67, TickLane::Ordinary)
            .expect("fixture frame must complete");
    }
    let gi = runtime
        .simulation
        .substrate
        .entities
        .get(gi)
        .expect("the GI survives");
    assert_eq!(gi.mission.current(), guard);
    assert!(gi.attack_target.is_none());
}

#[test]
fn sonic_fire_registers_immediately_but_later_techno_fires_before_wave_tail_ai() {
    let rules = sonic_tail_order_test_rules();
    let mut sim = Simulation::with_seed(0x5EED_760F);
    sim.input_delay_ticks = 0;
    let terrain = ResolvedTerrainGrid::from_cells(
        8,
        3,
        (0..3)
            .flat_map(|ry| (0..8).map(move |rx| bridgehead_base_cell(rx, ry)))
            .collect(),
    );
    install_rectangular_test_playfield(&mut sim, terrain.width(), terrain.height());
    sim.install_resolved_terrain_for_new_map(terrain);
    let heights = empty_heights();
    let dolphin_id = sim
        .spawn_object("DLPH", "Americans", 0, 0, 64, &rules, &heights)
        .expect("Dolphin placed first in Logic order");
    let later_id = sim
        .spawn_object("LATER", "Americans", 0, 2, 64, &rules, &heights)
        .expect("later shooter placed after Dolphin");
    let sonic_endpoint_id = sim
        .spawn_object("TARGET", "Russians", 6, 0, 0, &rules, &heights)
        .expect("Sonic endpoint");
    let wave_receiver_id = sim
        .spawn_object("TARGET", "Russians", 5, 0, 0, &rules, &heights)
        .expect("cell-list Wave receiver");
    let later_target_id = sim
        .spawn_object("TARGET", "Russians", 2, 2, 0, &rules, &heights)
        .expect("later shooter's target");
    assert!(crate::sim::combat::issue_attack_command(
        &mut sim.substrate.entities,
        dolphin_id,
        sonic_endpoint_id,
        Some(&rules),
        &sim.interner,
    ));
    assert!(crate::sim::combat::issue_attack_command(
        &mut sim.substrate.entities,
        later_id,
        later_target_id,
        Some(&rules),
        &sim.interner,
    ));
    sim.clear_lifecycle_test_events_for_test();

    let path = PathGrid::test_all_passable(8, 3);
    sim.path_grid = Some(std::sync::Arc::new(path));
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    runtime.resources.rules = rules;
    runtime.resources.height_map = heights;
    let output = runtime
        .advance_frame(&[], 67, TickLane::Ordinary)
        .expect("fixture frame must complete");
    assert!(output.tick.frame_committed);
    let sim = &runtime.simulation;

    assert_eq!(
        sim.substrate
            .entities
            .get(later_target_id)
            .expect("later target survives")
            .health
            .current,
        93,
        "the later pre-existing Techno completed its FireAt effects",
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(wave_receiver_id)
            .expect("Wave receiver survives")
            .health
            .current,
        90,
        "the appended Wave still damages its first recorded cell this frame",
    );
    let wave_id = *sim
        .active_wave_links
        .get(&dolphin_id)
        .expect("Dolphin retains the live Wave link");
    assert_eq!(
        sim.waves.get(wave_id).expect("Wave remains live").lifetime,
        99,
        "the new Logic tail owns its first AI in the firing pass",
    );

    let events = sim.lifecycle_test_events_for_test();
    let dolphin_boundary = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            LifecycleTestEvent::CombatFireEffectsCommitted {
                attacker_id,
                scenario_rng_state,
            } if *attacker_id == dolphin_id => Some((index, *scenario_rng_state)),
            _ => None,
        })
        .expect("Dolphin FireAt boundary traced");
    let later_boundary = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            LifecycleTestEvent::CombatFireEffectsCommitted {
                attacker_id,
                scenario_rng_state,
            } if *attacker_id == later_id => Some((index, *scenario_rng_state)),
            _ => None,
        })
        .expect("later FireAt boundary traced");
    let wave_receiver = events
        .iter()
        .enumerate()
        .find_map(|(index, event)| match event {
            LifecycleTestEvent::WaveDamageReceiverSelected {
                wave_id: selected_wave,
                target_id,
                scenario_rng_state,
            } if *selected_wave == wave_id && *target_id == wave_receiver_id => {
                Some((index, *scenario_rng_state))
            }
            _ => None,
        })
        .expect("Wave receiver boundary traced");
    assert!(dolphin_boundary.0 < later_boundary.0);
    assert!(
        later_boundary.0 < wave_receiver.0,
        "later FireAt/effects must finish before the appended Wave receiver walk",
    );
    assert_ne!(
        dolphin_boundary.1, later_boundary.1,
        "the later FireAt consumed its own ROF RNG before the Wave",
    );
    assert_eq!(
        later_boundary.1, wave_receiver.1,
        "the Wave receiver begins from the RNG state left by the later Techno",
    );
}

#[test]
fn sonic_cell_fire_wave_damage_selects_level_two_bridge_plane() {
    let rules = sonic_wave_test_rules();
    let mut sim = Simulation::with_seed(0x5EED_6240);
    sim.input_delay_ticks = 0;
    let mut cells = (0..8)
        .map(|rx| bridgehead_base_cell(rx, 0))
        .collect::<Vec<_>>();
    for cell in &mut cells {
        cell.level = 2;
        cell.template_height = 2;
        cell.land_type = 4;
        cell.yr_cell_land_type = 4;
        cell.is_water = true;
        cell.zone_type = 4;
        cell.base_land_type = 4;
        cell.base_yr_cell_land_type = 4;
        cell.has_bridge_deck = true;
        cell.bridge_walkable = true;
        cell.bridge_deck_level = 6;
        cell.bridge_facts.raw_flags |= crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL;
    }
    install_rectangular_test_playfield(&mut sim, 8, 1);
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(8, 1, cells));
    let heights = empty_heights();
    let dolphin_id = sim
        .spawn_object("DLPH", "Americans", 0, 0, 64, &rules, &heights)
        .expect("bridge Dolphin");
    let receiver_id = sim
        .spawn_object("TARGET", "Russians", 5, 0, 0, &rules, &heights)
        .expect("bridge receiver");
    for id in [dolphin_id, receiver_id] {
        sim.remove_entity_occupancy(id);
        sim.substrate
            .entities
            .get_mut(id)
            .expect("live entity")
            .on_bridge = true;
        sim.add_entity_occupancy(id);
    }
    assert!(crate::sim::combat::issue_attack_cell_command(
        &mut sim.substrate.entities,
        dolphin_id,
        6,
        0,
        Some(&rules),
        &sim.interner,
    ));

    let path = PathGrid::test_all_passable(8, 1);
    sim.path_grid = Some(std::sync::Arc::new(path));
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    runtime.resources.rules = rules;
    runtime.resources.height_map = heights;
    let output = runtime
        .advance_frame(&[], 67, TickLane::Ordinary)
        .expect("fixture frame must complete");
    assert!(output.tick.frame_committed);
    let wave_id = *runtime
        .simulation
        .active_wave_links
        .get(&dolphin_id)
        .expect("cell FireAt registered its Wave");
    // The Sonic bullet, Logic-appended just ahead of its Wave, strikes the
    // deck on its first AI in the same tail. Its removal shifts the Wave into
    // the visited slot (`0x0055B608..0x0055B619`), so the Wave's first AI is
    // the next frame's.
    assert_eq!(runtime.simulation.waves.get(wave_id).unwrap().lifetime, 100);
    let output = runtime
        .advance_frame(&[], 67, TickLane::Ordinary)
        .expect("fixture frame must complete");
    assert!(output.tick.frame_committed);
    let sim = &runtime.simulation;
    let wave = sim.waves.get(wave_id).expect("Wave survives its first AI");
    assert_eq!(wave.lifetime, 99);
    assert_eq!(
        wave.target.z, 674,
        "level 2 CellClass target is 2*104 + structural 416 + Sonic 50",
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(receiver_id)
            .expect("bridge receiver survives")
            .health
            .current,
        90,
        "Wave Z 674 meets the level-2 equality threshold 624 and walks AltObject",
    );
}

fn short_game_defeat_test_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[General]\nBaseUnit=AMCV,SMCV,PCV\n\n\
         [InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n1=AMCV\n2=SMCV\n3=PCV\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GACNST\n1=CAGAS01\n\n\
         [CAGAS01]\nStrength=400\nArmor=wood\nInsignificant=yes\nCanBeOccupied=yes\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\n\
         [AMCV]\nStrength=450\nArmor=heavy\nSpeed=5\nDeploysInto=GACNST\n\n\
         [SMCV]\nStrength=450\nArmor=heavy\nSpeed=5\nDeploysInto=GACNST\n\n\
         [PCV]\nStrength=450\nArmor=heavy\nSpeed=5\nDeploysInto=GACNST\n\n\
         [GACNST]\nStrength=1000\nArmor=wood\nFoundation=4x3\nConstructionYard=yes\nUndeploysInto=AMCV\n\n\
         [Warheads]\n0=Super\n\n\
         [Super]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    );
    RuleSet::from_ini(&ini).expect("short game defeat test rules should parse")
}

fn naval_bridge_test_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=BOAT\n1=DRED\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [BOAT]\nStrength=300\nArmor=heavy\nSpeed=6\nMovementZone=Water\nSpeedType=Float\nNaval=yes\n\n\
         [DRED]\nStrength=600\nArmor=heavy\nSpeed=5\nMovementZone=Water\nSpeedType=Float\nNaval=yes\nTooBigToFitUnderBridge=yes\n",
    );
    RuleSet::from_ini(&ini).expect("naval bridge test rules should parse")
}

fn real_ship_test_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=DEST\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [DEST]\nStrength=600\nArmor=heavy\nSpeed=6\nROT=5\nNaval=yes\nLocomotor={2BEA74E1-7CCA-11d3-BE14-00104B62A16C}\nMovementZone=Water\nSpeedType=Float\nTooBigToFitUnderBridge=yes\n",
    );
    RuleSet::from_ini(&ini).expect("real ship rules should parse")
}

fn teleport_command_test_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=CMIN\n1=CHRONO\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GAREFN\n\n\
         [CMIN]\nStrength=400\nArmor=light\nSpeed=4\nHarvester=yes\nTeleporter=yes\nDock=GAREFN\n\n\
         [CHRONO]\nStrength=200\nArmor=light\nSpeed=5\nTeleporter=yes\n\n\
         [GAREFN]\nStrength=900\nArmor=wood\nFoundation=4x3\nRefinery=yes\nDockUnload=yes\n",
    );
    RuleSet::from_ini(&ini).expect("teleport command rules should parse")
}

fn gate_test_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GAGATE_A\n\n\
         [GAGATE_A]\nStrength=500\nArmor=wood\nFoundation=3x1\nGate=yes\nDeployTime=.066\nGateCloseDelay=.2\n",
    );
    RuleSet::from_ini(&ini).expect("gate test rules should parse")
}

#[test]
fn native_frame_committed_late_gate_captures_pre_increment_frame() {
    // The native frame is committed LATE, so a Phase-1 consumer sees frame N
    // during the whole advance. The host duration is deliberately one
    // millisecond: admission, not elapsed time, advances the frame.
    use crate::sim::game_entity::{BuildingGateMissionState, BuildingGatePhase};

    let mut sim = Simulation::new();
    let rules = gate_test_rules();
    let heights = empty_heights();
    let gate_id = sim
        .spawn_object("GAGATE_A", "Americans", 10, 10, 0, &rules, &heights)
        .expect("spawn gate");
    {
        let gate = sim
            .substrate
            .entities
            .get_mut(gate_id)
            .expect("gate entity");
        let rt = gate.building_gate.get_or_insert_with(Default::default);
        rt.mission_18_active = true;
        rt.mission_state = BuildingGateMissionState::Setup;
        rt.phase = BuildingGatePhase::ClosedStable;
    }
    assert_eq!(sim.session.binary_frame, 0, "fresh sim starts at frame 0");

    let _ = sim.advance_tick(&[], Some(&rules), &heights, None, None, 1);

    // Committed late: post-tick frame advanced to 1.
    assert_eq!(
        sim.session.binary_frame, 1,
        "native frame committed late to 1"
    );
    // The consumer captured the PRE-increment frame 0 during the tick.
    let rt = sim
        .substrate
        .entities
        .get(gate_id)
        .expect("gate entity")
        .building_gate
        .as_ref()
        .expect("gate runtime");
    assert_eq!(
        rt.transition_timer.start_frame, 0,
        "gate captured pre-increment frame 0, not post-increment 1"
    );
}

#[test]
fn mission_host_counter_changes_state_hash() {
    // `mission` is folded into world_hash, so the host's per-object AI-counter
    // tick DOES move the lockstep hash — the mission state is live hashed
    // state, not a shadow.
    let mut sim = Simulation::new();
    sim.substrate
        .entities
        .insert(GameEntity::test_default(1, "E1", "Americans", 3, 3));
    sim.set_logic_order_for_test(vec![1]);
    let before = sim.state_hash();
    sim.object_ai_stage(None);
    let after = sim.state_hash();
    assert_ne!(
        before, after,
        "the host counter tick must perturb the state hash (mission is folded)"
    );
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().mission.ai_counter(),
        1,
        "object_ai_stage actually ran (AI counter advanced)"
    );
}

#[test]
fn short_game_defeats_house_with_no_buildings_even_if_ordinary_units_remain() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = true;
    let owner = insert_house_with_counts(&mut sim, "Americans", 0, 1);
    insert_test_entity_for_owner(&mut sim, 1, owner, "MTNK", EntityCategory::Unit);

    check_defeat_now(&mut sim, Some(&rules));

    assert!(sim.houses[&owner].is_defeated);
}

#[test]
fn short_game_keeps_house_alive_when_base_unit_remains() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = true;
    let owner = insert_house_with_counts(&mut sim, "Americans", 0, 1);
    insert_test_entity_for_owner(&mut sim, 1, owner, "AMCV", EntityCategory::Unit);

    check_defeat_now(&mut sim, Some(&rules));

    assert!(!sim.houses[&owner].is_defeated);
}

/// A dying MCV stays tracked until its destructor's Remove_Tracking at the
/// pending-delete drain, so the short game defeats its house only after.
#[test]
fn short_game_counts_a_dying_base_unit_until_it_is_deleted() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = true;
    let owner = insert_house_with_counts(&mut sim, "Americans", 0, 0);
    insert_test_entity_for_owner(&mut sim, 1, owner, "AMCV", EntityCategory::Unit);
    sim.substrate
        .entities
        .get_mut(1)
        .expect("AMCV inserted")
        .dying = true;

    check_defeat_now(&mut sim, Some(&rules));
    assert!(!sim.houses[&owner].is_defeated, "still tracked");

    sim.uninit(1);
    sim.flush_pending_delete();
    check_defeat_now(&mut sim, Some(&rules));
    assert!(sim.houses[&owner].is_defeated);
}

#[test]
fn long_game_keeps_house_alive_when_units_remain() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = false;
    let owner = insert_house_with_counts(&mut sim, "Americans", 0, 1);

    check_defeat_now(&mut sim, Some(&rules));

    assert!(!sim.houses[&owner].is_defeated);
}

#[test]
fn long_game_defeats_when_no_owned_objects_remain() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = false;
    let owner = insert_house_with_counts(&mut sim, "Americans", 0, 0);

    check_defeat_now(&mut sim, Some(&rules));

    assert!(sim.houses[&owner].is_defeated);
}

#[test]
fn short_game_victory_resolution_uses_new_defeat_state() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = true;
    let defeated = insert_house_with_counts(&mut sim, "Americans", 0, 1);
    let survivor = insert_house_with_counts(&mut sim, "Russians", 1, 0);
    insert_test_entity_for_owner(&mut sim, 1, defeated, "MTNK", EntityCategory::Unit);

    check_defeat_now(&mut sim, Some(&rules));

    assert!(sim.houses[&defeated].is_defeated);
    assert!(sim.houses[&survivor].has_won);
}

/// HouseClass::Update's gate calls Blowup_All (`0x004F8F7B`) before
/// MPlayer_Defeated: the straggler dies to its own Health in C4 damage, with
/// no attacker, so no house is credited.
#[test]
fn defeated_house_is_flagged_has_lost_and_its_stragglers_die() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = true;
    let defeated = insert_house_with_counts(&mut sim, "Americans", 0, 1);
    let survivor = insert_house_with_counts(&mut sim, "Russians", 1, 0);
    // A straggler vehicle owned by the losing house.
    insert_test_entity_for_owner(&mut sim, 1, defeated, "MTNK", EntityCategory::Unit);

    check_defeat_now(&mut sim, Some(&rules));

    // The loser is flagged both defeated and has_lost; the winner is not.
    assert!(sim.houses[&defeated].is_defeated);
    assert!(sim.houses[&defeated].has_lost);
    assert!(!sim.houses[&survivor].has_lost);
    assert!(sim.houses[&survivor].has_won);
    let straggler = sim
        .entities()
        .get(1)
        .expect("dead objects wait for the drain");
    assert_eq!(
        straggler.health.current, 0,
        "Blowup_All killed the straggler"
    );
    assert!(straggler.killed_by.is_none(), "no house is credited");
    assert_eq!(sim.houses[&survivor].stats.units_killed, 0);
}

#[test]
fn gsi_01_04_house_rung_owns_savour_deadline_and_emits_one_transition_edge() {
    use crate::sim::house_state::HouseOutcomeKind;
    use crate::sim::world::SimSoundEvent;

    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = false;
    sim.session.tick = 10;
    let winner = insert_house_with_counts(&mut sim, "Americans", 1, 0);
    let loser = insert_house_with_counts(&mut sim, "Russians", 0, 0);

    check_defeat_now(&mut sim, Some(&rules));

    let winner_outcome = sim.houses[&winner].outcome_state.expect("victory accepted");
    assert_eq!(winner_outcome.kind, HouseOutcomeKind::Victory);
    assert_eq!(winner_outcome.savour_until_tick, 38);
    assert!(!winner_outcome.exit_ready);
    assert_eq!(
        sim.sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::MatchOutcome { .. }))
            .count(),
        2,
        "one accepted loss and one accepted victory each emit one EVA edge"
    );

    sim.sound_events.clear();
    sim.session.tick = 36;
    check_defeat_now(&mut sim, Some(&rules));
    assert!(!sim.houses[&winner].outcome_state.unwrap().exit_ready);
    assert!(!sim.termination_frame_requested());
    assert!(sim.sound_events.is_empty(), "accepted edges never replay");

    sim.session.tick = 37;
    check_defeat_now(&mut sim, Some(&rules));
    assert!(sim.houses[&winner].outcome_state.unwrap().exit_ready);
    assert!(sim.houses[&loser].outcome_state.unwrap().exit_ready);
    assert!(sim.termination_frame_requested());
    assert!(sim.sound_events.is_empty(), "expiry does not replay EVA");
}

#[test]
fn short_game_base_unit_survivor_prevents_enemy_victory() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = true;
    let mcv_owner = insert_house_with_counts(&mut sim, "Americans", 0, 1);
    let enemy = insert_house_with_counts(&mut sim, "Russians", 1, 0);
    insert_test_entity_for_owner(&mut sim, 1, mcv_owner, "AMCV", EntityCategory::Unit);

    check_defeat_now(&mut sim, Some(&rules));

    assert!(!sim.houses[&mcv_owner].is_defeated);
    assert!(!sim.houses[&enemy].has_won);
}

/// Retail CAGAS01 is `Insignificant=yes`, and Add_Tracking skips
/// Insignificant types (`0x004FF71B`), including on ChangeOwner
/// (`0x007015E6`): a captured garrison does not keep its captor alive, and
/// Blowup_All destroys it with the rest of that house.
#[test]
fn gsi_05_16_a_captured_insignificant_garrison_does_not_keep_its_house_alive() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = true;
    let civilian = insert_passive_house_with_counts(&mut sim, "Neutral", 0, 0);
    let player = insert_house_with_counts(&mut sim, "Americans", 0, 1);
    let enemy = insert_house_with_counts(&mut sim, "Russians", 1, 0);
    let mut garrison = GameEntity::test_default(1, "CAGAS01", "Neutral", 10, 10);
    garrison.owner = civilian;
    garrison.type_ref = sim.interner.intern("CAGAS01");
    garrison.category = EntityCategory::Structure;
    garrison.tracking_facts.insignificant = true;
    sim.substrate.entities.insert(garrison);
    sim.update_house_tracking(1, crate::sim::house_tracking::HouseTracking::add_tracking);

    // The passenger reconciler uses this chokepoint when the first occupant
    // captures a civilian CanBeOccupied building.
    sim.change_owner(1, player);
    check_defeat_now(&mut sim, Some(&rules));

    assert_eq!(sim.houses[&player].tracking.buildings_for_test(), 0);
    assert!(sim.houses[&player].is_defeated);
    assert!(sim.houses[&enemy].has_won);
    assert_eq!(sim.entities().get(1).unwrap().health.current, 0);
}

/// Insert a `MultiplayPassive=true` house — the stock `Neutral` (Civilian) and
/// `Special` (JP) shape, which every skirmish creates and which owns civilian
/// map objects for the whole match.
fn insert_passive_house_with_counts(
    sim: &mut Simulation,
    name: &str,
    buildings: i32,
    units: i32,
) -> crate::sim::intern::InternedId {
    let owner = insert_house_with_counts(sim, name, buildings, units);
    let house = sim.houses.get_mut(&owner).expect("house just inserted");
    house.multiplay_passive = true;
    // Stock Civilian/JP are never player-controlled.
    house.is_human = false;
    owner
}

/// Build an alliance graph with exactly the edges given — no symmetrization, so
/// a single `(a, b)` pair models a one-way alliance.
fn directed_alliances(edges: &[(&str, &str)]) -> HouseAllianceMap {
    let mut map = HouseAllianceMap::new();
    for (from, to) in edges {
        map.entry(from.to_ascii_uppercase())
            .or_default()
            .insert(to.to_ascii_uppercase());
        map.entry(to.to_ascii_uppercase()).or_default();
    }
    map
}

#[test]
fn passive_house_owning_buildings_does_not_block_last_player_victory() {
    // The player-visible bug: Neutral/Special own civilian structures on most
    // stock maps, so before the passive filter the alive set never reached 1 and
    // the victory screen never appeared after the last opponent died.
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = false;
    let survivor = insert_house_with_counts(&mut sim, "Americans", 3, 4);
    let loser = insert_house_with_counts(&mut sim, "Russians", 0, 0);
    insert_passive_house_with_counts(&mut sim, "Neutral", 7, 0);

    check_defeat_now(&mut sim, Some(&rules));

    assert!(sim.houses[&loser].is_defeated);
    assert!(
        sim.houses[&survivor].has_won,
        "last non-passive house standing must win despite the Civilian house owning buildings"
    );
}

#[test]
fn passive_house_is_never_defeated_even_with_nothing_left() {
    // gamemd skips the whole defeat block for a MultiplayPassive house, so it is
    // never flagged defeated no matter how empty it gets.
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = true;
    let passive = insert_passive_house_with_counts(&mut sim, "Neutral", 0, 0);
    let player = insert_house_with_counts(&mut sim, "Americans", 2, 1);
    let opponent = insert_house_with_counts(&mut sim, "Russians", 0, 0);

    check_defeat_now(&mut sim, Some(&rules));

    assert!(!sim.houses[&passive].is_defeated);
    assert!(!sim.houses[&passive].has_lost);
    assert!(!sim.houses[&passive].has_won);
    assert!(sim.houses[&opponent].is_defeated);
    assert!(sim.houses[&player].has_won);
}

#[test]
fn passive_houses_do_not_make_a_solo_board_look_contested() {
    // The automatic victory creation guard. A one-player dev map whose
    // [Houses] is the player plus Neutral/Special reaches alive.len() == 1 on
    // tick 1, so counting passive houses here would announce instant victory.
    let mut sim = Simulation::new();
    insert_house_with_counts(&mut sim, "Americans", 1, 1);
    insert_passive_house_with_counts(&mut sim, "Neutral", 4, 0);
    insert_passive_house_with_counts(&mut sim, "Special", 2, 0);

    assert_eq!(sim.houses.len(), 3);
    assert_eq!(sim.contending_house_count(), 1);

    insert_house_with_counts(&mut sim, "Russians", 1, 1);
    assert_eq!(sim.contending_house_count(), 2);
}

#[test]
fn solo_developer_board_creates_no_automatic_victory_state_or_eva() {
    let mut sim = Simulation::new();
    let player = insert_house_with_counts(&mut sim, "Americans", 1, 1);
    insert_passive_house_with_counts(&mut sim, "Neutral", 4, 0);
    insert_passive_house_with_counts(&mut sim, "Special", 2, 0);
    for tick in 0..300 {
        sim.session.tick = tick;
        check_defeat_now(&mut sim, None);
        assert!(!sim.houses[&player].has_won);
        assert!(sim.houses[&player].outcome_state.is_none());
        assert!(sim.ready_outcome_for_owner(player).is_none());
        assert!(!sim.termination_frame_requested());
    }
    assert!(
        sim.sound_events
            .iter()
            .all(|event| !matches!(event, SimSoundEvent::MatchOutcome { .. }))
    );
}

#[test]
fn explicit_solo_outcomes_advance_and_reach_the_shared_ready_query() {
    use crate::sim::house_state::HouseOutcomeKind;
    for kind in [HouseOutcomeKind::Victory, HouseOutcomeKind::Defeat] {
        let mut sim = Simulation::new();
        let player = insert_house_with_counts(&mut sim, "Americans", 1, 1);
        insert_passive_house_with_counts(&mut sim, "Neutral", 4, 0);
        insert_passive_house_with_counts(&mut sim, "Special", 2, 0);
        let house = sim.houses.get_mut(&player).unwrap();
        assert!(match kind {
            HouseOutcomeKind::Victory => house.flag_to_win(0, 3),
            HouseOutcomeKind::Defeat => house.flag_to_lose(0, 3),
        });
        // Accepted outcomes carry no automatic/explicit origin discriminator.
        // Their timer and app-visible readiness must not depend on opponents.
        for tick in 0..3 {
            sim.session.tick = tick;
            check_defeat_now(&mut sim, None);
            assert_eq!(sim.termination_frame_requested(), tick == 2);
            assert_eq!(sim.ready_outcome_for_owner(player).is_some(), tick == 2);
        }
        assert_eq!(sim.ready_outcome_for_owner(player).unwrap().kind, kind);
        assert!(
            sim.sound_events.is_empty(),
            "timer advancement must not replay EVA"
        );
    }
}

#[test]
fn one_way_alliance_does_not_end_the_game() {
    // Native alliance is directional; the game-over scan requires both houses of
    // a pair to name the other. A unilateral "I ally you" must not hand out wins.
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    sim.session.game_options.short_game = false;
    let a = insert_house_with_counts(&mut sim, "Americans", 1, 1);
    let b = insert_house_with_counts(&mut sim, "Russians", 1, 1);
    sim.house_alliances = directed_alliances(&[("Americans", "Russians")]);

    check_defeat_now(&mut sim, Some(&rules));

    assert!(!sim.houses[&a].has_won, "one-way alliance must not win");
    assert!(!sim.houses[&b].has_won, "one-way alliance must not win");

    // Control: once the alliance is mutual the same board is a shared victory.
    sim.house_alliances =
        directed_alliances(&[("Americans", "Russians"), ("Russians", "Americans")]);
    check_defeat_now(&mut sim, Some(&rules));
    assert!(sim.houses[&a].has_won);
    assert!(sim.houses[&b].has_won);
}

#[test]
fn test_spawn_vehicle_has_voxel_marker() {
    let mut sim: Simulation = Simulation::new();
    let entities: Vec<MapEntity> = vec![make_test_entity("MTNK", EntityCategory::Unit)];
    let count: u32 = sim.spawn_from_map(&entities, None, &empty_heights());

    assert_eq!(count, 1);
    let voxel_count: usize = sim
        .substrate
        .entities
        .values()
        .filter(|e| e.is_voxel)
        .count();
    assert_eq!(voxel_count, 1, "Vehicle should have VoxelModel marker");
}

#[test]
fn test_spawn_infantry_has_sprite_marker() {
    let mut sim: Simulation = Simulation::new();
    let entities: Vec<MapEntity> = vec![make_test_entity("E1", EntityCategory::Infantry)];
    sim.spawn_from_map(&entities, None, &empty_heights());

    let sprite_count: usize = sim
        .substrate
        .entities
        .values()
        .filter(|e| !e.is_voxel)
        .count();
    assert_eq!(sprite_count, 1, "Infantry should have SpriteModel marker");
}

#[test]
fn gsi_13_10_art_voxel_no_selects_shp_unit_in_all_three_spawn_constructors() {
    let rules = gsi_13_10_art_model_rules();
    let mut sim = Simulation::new();

    assert_eq!(
        sim.spawn_from_map(
            &[make_test_entity("DLPH", EntityCategory::Unit)],
            Some(&rules),
            &empty_heights(),
        ),
        1
    );
    assert_gsi_13_10_shp_unit(sim.substrate.entities.get(1).expect("map DLPH"));

    let dron = sim
        .spawn_object_at_height("DRON", "Americans", 31, 40, 64, 0, &rules)
        .expect("placed DRON");
    assert_gsi_13_10_shp_unit(sim.substrate.entities.get(dron).expect("DRON entity"));

    let squid = sim
        .spawn_object_limbo_at_height("SQD", "Americans", 32, 40, 64, 0, &rules)
        .expect("limbo SQD");
    assert_gsi_13_10_shp_unit(sim.substrate.entities.get(squid).expect("SQD entity"));
}

#[test]
fn gsi_13_10_effective_art_metadata_precedes_complete_category_fallback() {
    let rules = gsi_13_10_art_model_rules();
    let mut sim = Simulation::new();

    let vxl = sim
        .spawn_object_limbo_at_height("VXLTEST", "Americans", 1, 1, 0, 0, &rules)
        .expect("explicit VXL vehicle");
    assert_gsi_13_10_vxl_unit(sim.substrate.entities.get(vxl).expect("VXL entity"));

    let aliased = sim
        .spawn_object_limbo_at_height("ALIASED", "Americans", 2, 1, 0, 0, &rules)
        .expect("Image=ALT vehicle");
    assert_gsi_13_10_shp_unit(sim.substrate.entities.get(aliased).expect("aliased entity"));

    let omitted = sim
        .spawn_object_limbo_at_height("OMITTED", "Americans", 3, 1, 0, 0, &rules)
        .expect("art entry with omitted Voxel");
    assert_gsi_13_10_shp_unit(sim.substrate.entities.get(omitted).expect("omitted entity"));

    for (type_id, expected_voxel) in [
        ("FALLBACKVEH", true),
        ("FALLBACKAIR", true),
        ("FALLBACKINF", false),
        ("FALLBACKBLD", false),
    ] {
        let id = sim
            .spawn_object_limbo_at_height(type_id, "Americans", 4, 1, 0, 0, &rules)
            .unwrap_or_else(|| panic!("missing-metadata fallback spawn {type_id}"));
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .expect("fallback entity")
                .is_voxel,
            expected_voxel,
            "category fallback for {type_id}"
        );
    }
}

#[test]
fn test_spawn_sets_position_and_facing() {
    let mut sim: Simulation = Simulation::new();
    let entities: Vec<MapEntity> = vec![make_test_entity("HTNK", EntityCategory::Unit)];
    sim.spawn_from_map(&entities, None, &empty_heights());

    for e in sim.substrate.entities.values() {
        assert_eq!(e.position.rx, 30);
        assert_eq!(e.position.ry, 40);
        assert_eq!(e.facing, 64);
        assert_eq!(sim.interner.resolve(e.type_ref), "HTNK");
        // The diamond-centre screen projection of this spawn is asserted on
        // the render side (`render::locomotor_visual` boundary tests, F14).
    }
}

#[test]
fn test_spawn_from_map_high_unit_uses_bridge_layer_and_deck_level() {
    let mut sim = Simulation::new();
    let heights = empty_heights();
    let resolved = single_bridge_cell(5, 5, 3);
    let count = sim.spawn_from_map_with_resolved(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 5,
            cell_y: 5,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: true,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        Some(&combat_test_rules()),
        &heights,
        Some(&resolved),
    );

    assert_eq!(count, 1);
    let e = sim.substrate.entities.get(1).expect("spawned entity");
    assert_eq!(e.position.z, 3);
    let bridge = e.bridge_occupancy.as_ref().expect("bridge occupancy");
    assert_eq!(bridge.deck_level, 3);
    assert!(e.on_bridge);
    let loco = e.locomotor.as_ref().expect("loco");
    assert_eq!(loco.layer, MovementLayer::Bridge);
}

#[test]
fn test_spawn_from_map_high_without_bridge_falls_back_to_ground() {
    let mut sim = Simulation::new();
    let heights = BTreeMap::from([((5, 5), 1)]);
    let resolved = ResolvedTerrainGrid::from_cells(
        6,
        6,
        (0..6u16)
            .flat_map(|ry| {
                (0..6u16).map(
                    move |rx| crate::map::resolved_terrain::ResolvedTerrainCell {
                        rx,
                        ry,
                        source_tile_index: 0,
                        source_sub_tile: 0,
                        final_tile_index: 0,
                        final_sub_tile: 0,
                        is_wood_bridge_repair_tile: false,
                        level: 0,
                        filled_clear: false,
                        tileset_index: Some(0),
                        land_type: 0,
                        yr_cell_land_type: 0,
                        slope_type: 0,
                        template_height: 0,
                        render_offset_x: 0,
                        render_offset_y: 0,
                        terrain_class: crate::rules::terrain_rules::TerrainClass::Clear,
                        speed_costs: crate::rules::terrain_rules::SpeedCostProfile::default(),
                        is_water: false,
                        is_cliff_like: false,
                        height_in_pixels: 0,
                        variant: 0,
                        is_rough: false,
                        is_road: false,
                        accepts_smudge: false,
                        allows_tiberium: false,
                        has_ramp: false,
                        canonical_ramp: None,
                        ground_walk_blocked: false,
                        terrain_object_blocks: false,
                        terrain_object_occupation: None,
                        overlay_blocks: false,
                        overlay_zone_type: None,
                        outside_playfield: false,
                        zone_type: 0,
                        base_ground_walk_blocked: false,
                        base_build_blocked: false,
                        base_land_type: 0,
                        base_yr_cell_land_type: 0,
                        base_terrain_class: Default::default(),
                        base_speed_costs: Default::default(),
                        build_blocked: false,
                        has_bridge_deck: false,
                        bridge_walkable: false,
                        bridge_transition: false,
                        bridge_deck_level: 0,
                        bridge_layer: None,
                        bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                        tube_index: None,
                        radar_left: [0, 0, 0],
                        radar_right: [0, 0, 0],
                        has_damaged_data: false,
                        bridgehead_anchor_class_at_load: None,
                    },
                )
            })
            .collect(),
    );
    sim.spawn_from_map_with_resolved(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 5,
            cell_y: 5,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: true,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        Some(&combat_test_rules()),
        &heights,
        Some(&resolved),
    );
    let e = sim.substrate.entities.get(1).expect("spawned entity");
    assert_eq!(e.position.z, 1);
    assert!(e.bridge_occupancy.is_none());
    assert!(!e.on_bridge);
    let loco = e.locomotor.as_ref().expect("loco");
    assert_eq!(loco.layer, MovementLayer::Ground);
}

#[test]
fn test_bridge_damage_rebuilds_path_grid() {
    // 3-cell EW strip at (1..=3, 0), all overlay 0xDC. HighDirect dispatcher
    // path fires the EW walker → all 3 cells transition to 0xE8 + Destroyed
    // → `is_bridge_walkable` returns false → rebuilt path grid says no bridge
    // layer at any of the 3 cells.
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(2, 0, 2, false, 0);
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    // Build PathGrid before damage — all 3 cells walkable on bridge layer.
    let grid_before =
        PathGrid::from_resolved_terrain_with_bridges(&resolved, sim.bridge_state.as_ref());
    for x in 1..=3 {
        assert!(grid_before.is_walkable_on_layer(x, 0, MovementLayer::Bridge));
    }

    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    let _state_changed = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 2,
            ry: 0,
            damage: 20,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );

    // Rebuild PathGrid after damage — none of the 3 cells walkable on bridge.
    let grid_after = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().unwrap(),
        sim.bridge_state.as_ref(),
    );
    for x in 1..=3 {
        assert!(
            !grid_after.is_walkable_on_layer(x, 0, MovementLayer::Bridge),
            "cell ({x}, 0) should not be walkable on bridge layer after collapse"
        );
    }
}

/// End-to-end: a bridge collapse driven through the orchestrator must
/// signal `state_changed = true`, AND the PathGrid that the app would
/// rebuild post-tick (via PathGrid::from_resolved_terrain_with_bridges)
/// must show the collapsed cells as non-walkable on the bridge layer.
///
/// Ledger #1 (one-tick delay), #4 (ground revert), #9 (layer separation).
#[test]
fn test_bridge_collapse_signals_pathgrid_refresh() {
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(2, 0, 2, false, 0);
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);

    let state_changed = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 2,
            ry: 0,
            damage: 20,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );
    assert!(
        state_changed,
        "orchestrator must signal state_changed=true on collapse"
    );

    // The PathGrid the app would build after this tick (via rebuild_
    // dynamic_path_grid → PathGrid::from_resolved_terrain_with_bridges):
    let post_tick_grid = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().unwrap(),
        sim.bridge_state.as_ref(),
    );
    for x in 1..=3 {
        assert!(
            !post_tick_grid.is_walkable_on_layer(x, 0, MovementLayer::Bridge),
            "cell ({x}, 0) must not be walkable on bridge layer after collapse"
        );
    }
}

/// No-collapse tick must NOT signal state_changed. Empty event lists
/// (no bridge damage this tick) leave the path grid untouched — avoids
/// firing unnecessary refresh ticks.
#[test]
fn test_no_collapse_does_not_signal_refresh() {
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(2, 0, 2, false, 0);
    sim.resolved_terrain = Some(resolved);
    sim.bridge_state = Some(bridge_state);

    let rules = combat_test_rules();

    let state_changed =
        crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(&mut sim, &rules, &[]);
    assert!(!state_changed, "empty events must not signal state_changed");
}

/// Regression for ledger #2 / #3: when a bridge body span collapses, every
/// cell that previously had `transition: true` must lose it. Otherwise A*
/// would still permit Ground→Bridge entry into the destroyed span.
///
/// The fixture seeds Body-role cells with `bridge_transition = true`
/// (mimicking bridgeheads from the PathCell projection's perspective).
/// Post-collapse, `from_resolved_terrain_with_bridges` gates `transition`
/// on `is_bridge_walkable`, so all 3 cells must drop the flag.
///
/// Guards against future per-cell-delta optimizations that might only
/// update the directly-destroyed cell and miss adjacent transition cells.
#[test]
fn test_bridge_collapse_clears_transition_flag() {
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(2, 0, 2, false, 0);
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    // Snapshot cells with transition=true before damage.
    let grid_before =
        PathGrid::from_resolved_terrain_with_bridges(&resolved, sim.bridge_state.as_ref());
    let transition_cells_before: Vec<(u16, u16)> = (0..resolved.width())
        .flat_map(|x| (0..resolved.height()).map(move |y| (x, y)))
        .filter_map(|(x, y)| {
            let cell = grid_before.cell(x, y)?;
            cell.transition.then_some((x, y))
        })
        .collect();
    assert!(
        !transition_cells_before.is_empty(),
        "test fixture must have at least one transition cell"
    );

    // Damage event collapses the entire EW strip.
    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    let _ = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 2,
            ry: 0,
            damage: 20,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );

    let grid_after = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().unwrap(),
        sim.bridge_state.as_ref(),
    );
    for (x, y) in &transition_cells_before {
        let cell = grid_after.cell(*x, *y).expect("cell exists");
        assert!(
            !cell.transition,
            "cell ({x}, {y}) must lose transition flag after bridge collapse"
        );
    }
}

#[test]
fn test_destroyed_bridge_snaps_unit_to_ground_when_ground_exists() {
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 3, false, 1);
    install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    sim.spawn_from_map_with_resolved(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 5,
            cell_y: 5,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: true,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        Some(&combat_test_rules()),
        &BTreeMap::from([((5, 5), 1)]),
        Some(&resolved),
    );

    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    let _state_changed = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 5,
            ry: 5,
            damage: 15,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );

    let e = sim
        .substrate
        .entities
        .get(1)
        .expect("surviving bridge unit");
    assert_eq!(e.position.z, 1);
    assert!(e.bridge_occupancy.is_none());
    assert!(!e.on_bridge);
    let loco = e.locomotor.as_ref().expect("locomotor");
    assert_eq!(loco.layer, MovementLayer::Ground);
    assert!(e.movement_target.is_none());
}

/// Per HIGH §12.7 / §12.9: deck units snap to ground level on collapse —
/// no damage, no despawn, even when the ground below is unwalkable (water,
/// `is_water=true` + `ground_walk_blocked=true`). Vanilla has no drown
/// mechanism.
#[test]
fn test_destroyed_bridge_snaps_unit_to_ground_over_water_below() {
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 3, true, 0);
    install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    sim.spawn_from_map_with_resolved(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 5,
            cell_y: 5,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: true,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        Some(&combat_test_rules()),
        &BTreeMap::new(),
        Some(&resolved),
    );

    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    let _state_changed = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 5,
            ry: 5,
            damage: 15,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );

    // DropIn correction: unit ALIVE, snapped to ground level=0, OnBridge
    // cleared, locomotor flipped to Ground/Idle.
    let e = sim
        .substrate
        .entities
        .get(1)
        .expect("deck unit must SURVIVE collapse over water");
    assert_eq!(
        e.health.current, 300,
        "DropIn never harms — authored full health uses MTNK Strength300"
    );
    assert_eq!(e.position.z, 0, "snapped to ground level");
    assert!(!e.on_bridge);
    assert!(e.bridge_occupancy.is_none());
    let loco = e.locomotor.as_ref().expect("locomotor");
    assert_eq!(loco.layer, MovementLayer::Ground);
    assert!(e.movement_target.is_none());
}

/// Same DropIn correction over an overlay-blocked ground cell.
#[test]
fn test_destroyed_bridge_snaps_unit_to_ground_over_overlay_blocked() {
    let mut sim = Simulation::new();
    let (mut resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 3, false, 0);
    let idx = resolved.index(5, 5).expect("bridge index");
    resolved.cells[idx].overlay_blocks = true;
    install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    sim.spawn_from_map_with_resolved(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 5,
            cell_y: 5,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: true,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        Some(&combat_test_rules()),
        &BTreeMap::new(),
        Some(&resolved),
    );

    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    let _state_changed = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 5,
            ry: 5,
            damage: 15,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );

    let e = sim
        .substrate
        .entities
        .get(1)
        .expect("deck unit must SURVIVE over overlay-blocked ground");
    assert_eq!(e.health.current, 300, "DropIn never harms");
    assert_eq!(e.position.z, 0);
    assert!(!e.on_bridge);
    assert!(e.bridge_occupancy.is_none());
}

/// Same DropIn correction over a terrain-object-blocked ground cell.
#[test]
fn test_destroyed_bridge_snaps_unit_to_ground_over_terrain_object_blocked() {
    let mut sim = Simulation::new();
    let (mut resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 3, false, 0);
    let idx = resolved.index(5, 5).expect("bridge index");
    resolved.cells[idx].terrain_object_blocks = true;
    install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    sim.spawn_from_map_with_resolved(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 5,
            cell_y: 5,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: true,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        Some(&combat_test_rules()),
        &BTreeMap::new(),
        Some(&resolved),
    );

    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    let _state_changed = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 5,
            ry: 5,
            damage: 15,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );

    let e = sim
        .substrate
        .entities
        .get(1)
        .expect("deck unit must SURVIVE over terrain-object-blocked ground");
    assert_eq!(e.health.current, 300, "DropIn never harms");
    assert_eq!(e.position.z, 0);
    assert!(!e.on_bridge);
    assert!(e.bridge_occupancy.is_none());
}

/// After collapse, the rebuilt path grid reverts the bridge cell to its
/// underlying ground walkability — a cliff-like cell stays unwalkable
/// (per `from_resolved_terrain_with_bridges`'s `is_cliff_like` branch).
/// Plus the DropIn correction: the deck unit still survives.
#[test]
fn test_destroyed_bridge_fallout_matches_rebuilt_ground_walkability() {
    let mut sim = Simulation::new();
    let (mut resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 3, false, 0);
    let idx = resolved.index(5, 5).expect("bridge index");
    resolved.cells[idx].is_cliff_like = true;
    install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    sim.spawn_from_map_with_resolved(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 5,
            cell_y: 5,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: true,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        Some(&combat_test_rules()),
        &BTreeMap::new(),
        Some(&resolved),
    );

    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    let _state_changed = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 5,
            ry: 5,
            damage: 15,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );

    let rebuilt_grid = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().expect("resolved terrain"),
        sim.bridge_state.as_ref(),
    );
    assert!(
        !rebuilt_grid.is_walkable_on_layer(5, 5, MovementLayer::Bridge),
        "destroyed bridge layer should be unwalkable"
    );
    assert!(
        !rebuilt_grid.is_walkable_on_layer(5, 5, MovementLayer::Ground),
        "destroyed cliff-like cell falls back to unwalkable underlying terrain"
    );
    // DropIn correction: the unit survived stranded at ground level even
    // though the underlying ground is cliff-like (vanilla never despawns).
    let e = sim.substrate.entities.get(1).expect("deck unit survives");
    assert_eq!(e.health.current, 300, "DropIn never harms");
    assert!(!e.on_bridge);
}

/// Full-pipeline cascade: ground-layer entity at a destroyed bridge cell is
/// force-killed (health=0, dying=true) per HIGH §11.4 step 1 — mirrors the
/// binary's `BlowUpBridge` ground-occupant pass with C4Warhead semantics.
/// Bridge-deck entities go through DropIn (Step 2) and survive; this test
/// covers the parallel ground-layer path.
#[test]
fn test_bridge_collapse_kills_ground_unit_under_destroyed_cell() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\
         [Warheads]\n0=Super\n[Super]\nInfDeath=2\nPenetratesBunker=yes\n\
         Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 3, false, 0);
    install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(bridge_state);

    // Spawn at (5, 5), then explicitly model the native lower object-list
    // occupant below the structural bridge deck. Constructor admission rightly
    // selects the deck on this cell; the collapse path itself needs a ground-list
    // witness.
    sim.spawn_from_map_with_resolved(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 5,
            cell_y: 5,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        Some(&rules),
        &BTreeMap::new(),
        Some(&resolved),
    );
    let id = sim
        .substrate
        .entities
        .iter_sorted()
        .next()
        .map(|(id, _)| id)
        .expect("ground unit spawned");
    sim.remove_entity_occupancy(id);
    let unit = sim.substrate.entities.get_mut(id).unwrap();
    unit.on_bridge = false;
    if let Some(locomotor) = unit.locomotor.as_mut() {
        locomotor.layer = MovementLayer::Ground;
    }
    sim.add_entity_occupancy(id);
    assert!(
        !sim.substrate.entities.get(id).unwrap().on_bridge,
        "ground layer"
    );

    sim.resolve_type_handles(&rules);
    let _ = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 5,
            ry: 5,
            damage: 15,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );

    let e = sim
        .substrate
        .entities
        .get(id)
        .expect("ground unit retained until pending deletion drains");
    assert_eq!(e.health.current, 0, "kill_ground_occupants_at zeroed HP");
    assert!(e.dying, "direct receiver completed death before returning");
    assert!(e.attack_target.is_none());
    assert!(e.movement_target.is_none());
}

/// Full-pipeline walker: a single Ion-Cannon hit at the center of a 3-cell
/// EW strip drives the HighDirect dispatcher → walker → all 3 cells of the
/// (this, west, east) triple get overlay 0xE8 + DamageState::Destroyed.
#[test]
fn test_bridge_walker_collapses_full_3_cell_strip_on_single_hit() {
    use crate::sim::bridge_state::DamageState;
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 3, false, 0);
    sim.resolved_terrain = Some(resolved);
    sim.bridge_state = Some(bridge_state);

    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    let _ = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 5,
            ry: 5,
            damage: 15,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );

    let bs = sim.bridge_state.as_ref().unwrap();
    for x in 4..=6 {
        let cell = bs.cell(x, 5).expect("bridge cell present");
        assert_eq!(
            cell.damage_state,
            DamageState::Destroyed,
            "cell ({x}, 5) must be destroyed by walker triple"
        );
        assert_eq!(
            cell.overlay_byte, 0xE8,
            "cell ({x}, 5) must hold the EW final-stage overlay"
        );
    }
}

/// Path mutual exclusion: a cell whose overlay has been TRANSITIONED out
/// of the raw body range (e.g. 0x6) routes to the state-machine path,
/// never to direct-overlay. The reverse (raw body overlay) routes to the
/// direct path. Verifies the dispatcher's overlay invariant prevents
/// double-firing on the same hit.
#[test]
fn test_bridge_dispatcher_state_machine_overlay_routes_to_high_sm_not_direct() {
    use crate::sim::bridge_state::{
        AnchorSpan, Axis, BridgeCellRole, BridgeRuntimeCell, DamageState, Direction, DispatchPath,
    };
    let mut sim = Simulation::new();
    let (resolved, mut bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 4, false, 0);
    // Override center cell to the post-transition state: overlay 0x6 (out
    // of the 0xCD..=0xE6 raw HIGH range), role=Anchor, damage_state=Damaged
    // (so a single hit Damaged→Destroyed). Anchor span carries only the
    // anchor itself so set_bridge_direction emits one BlowUpBridge action.
    bridge_state.test_seed_cell(
        5,
        5,
        BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 4,
            bridge_group_id: Some(1),
            damage_state: DamageState::Damaged,
            axis: Some(Axis::EW),
            role: BridgeCellRole::Anchor,
            anchor_span_id: Some(1),
            overlay_byte: 0x6,
            bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
        },
    );
    bridge_state.test_seed_anchor_span(AnchorSpan {
        id: 1,
        anchor: (5, 5),
        cells: [Some((5, 5)), None, None, None, None, None],
        axis: Axis::EW,
        direction: Direction::S,
        damage_state: DamageState::Damaged,
        bridge_group_id: 1,
    });
    sim.resolved_terrain = Some(resolved);
    sim.bridge_state = Some(bridge_state);

    // Path classifier: HighSM matches, HighDirect does NOT, when the
    // overlay has been transitioned out of the raw body range.
    let bs = sim.bridge_state.as_ref().unwrap();
    let ctx = crate::sim::bridge_state::BridgeDamageContext {
        damage: 15,
        warhead_ref: crate::sim::intern::InternedId::default(),
        is_ion_cannon: true,
        bridge_strength: bs.bridge_strength(),
        impact_z: 0,
    };
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    assert!(
        bs.path_matches_cell(DispatchPath::HighStateMachine, 5, 5, &ctx, terrain),
        "transitioned overlay routes to HighSM"
    );
    assert!(
        !bs.path_matches_cell(DispatchPath::HighDirect, 5, 5, &ctx, terrain),
        "transitioned overlay must NOT also match HighDirect"
    );

    // BR-02: a cell still in the raw body range matches HighDirect AND the
    // High SM block — the SM block's overlay-first driver routes it to the
    // direct walker, so both blocks fire and consume two BridgeStrength draws.
    // Re-seed (4, 5) with overlay 0xDC.
    let bs_mut = sim.bridge_state.as_mut().unwrap();
    bs_mut.test_seed_cell(
        4,
        5,
        BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 4,
            bridge_group_id: Some(1),
            damage_state: DamageState::Healthy { variant: 0 },
            axis: Some(Axis::EW),
            role: BridgeCellRole::Body,
            anchor_span_id: Some(1),
            overlay_byte: 0xDC,
            bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
        },
    );
    let bs = sim.bridge_state.as_ref().unwrap();
    assert!(
        bs.path_matches_cell(DispatchPath::HighDirect, 4, 5, &ctx, terrain),
        "raw body overlay routes to HighDirect"
    );
    assert!(
        bs.path_matches_cell(DispatchPath::HighStateMachine, 4, 5, &ctx, terrain),
        "BR-02: in-band cell also matches the High SM block (overlay-first), consuming a second draw"
    );
}

/// Integration test: full apply_bridge_damage_events pipeline on a
/// state-machine path. Native self anchor with overlay25/state15 →
/// body driver fires Damaged→Destroyed → endpoint deactivation cascade
/// runs via `refresh_bridge_zones_if_dirty`. Independently exercises the
/// HighSM path (Task 15 coverage focused on HighDirect).
#[test]
fn test_bridge_orchestrator_state_machine_path_collapses_anchor_and_deactivates_endpoint() {
    use crate::sim::bridge_state::{
        AnchorSpan, Axis, BridgeCellRole, BridgeRuntimeCell, DamageState, Direction,
    };
    let mut sim = Simulation::new();
    // Use the strip helper so resolved_terrain has a bridge group with
    // ground neighbors → endpoint records exist.
    let (mut resolved, _) = ew_high_bridge_strip_for_dispatch(5, 5, 4, false, 0);
    // Native587180 requires structural self/+2C and canonical overlay18/19.
    // This test's former overlay6/topology-only anchor was not a native body.
    resolved.apply_runtime_bridge_mark_stamp(
        crate::map::bridge_facts::BridgeFlagStamp::new((5, 5), 6, true),
        crate::map::bridge_facts::BridgeStampFamily::Nesw,
    );
    let facts = &mut resolved.cell_mut(5, 5).unwrap().bridge_facts;
    facts.overlay_id = Some(25);
    facts.state_byte = 15;
    let mut bridge_state = BridgeRuntimeState::from_resolved_terrain(&resolved, true, 15);
    // Override (5, 5) to the post-transition Damaged state-machine setup.
    bridge_state.test_seed_cell(
        5,
        5,
        BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 4,
            bridge_group_id: Some(1),
            damage_state: DamageState::Damaged,
            axis: Some(Axis::EW),
            role: BridgeCellRole::Anchor,
            anchor_span_id: Some(1),
            overlay_byte: 25,
            bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
        },
    );
    bridge_state.test_seed_anchor_span(AnchorSpan {
        id: 1,
        anchor: (5, 5),
        cells: [
            Some((5, 5)),
            Some((4, 5)),
            Some((3, 5)),
            Some((2, 5)),
            Some((6, 5)),
            None,
        ],
        axis: Axis::EW,
        direction: Direction::W,
        damage_state: DamageState::Damaged,
        bridge_group_id: 1,
    });
    sim.resolved_terrain = Some(resolved);
    sim.bridge_state = Some(bridge_state);

    let pre_active: Vec<bool> = sim
        .bridge_state
        .as_ref()
        .unwrap()
        .endpoint_records()
        .iter()
        .map(|r| r.active)
        .collect();

    let mut rules = combat_test_rules();
    let mut building = make_test_entity("GACNST", EntityCategory::Structure);
    building.cell_x = 0;
    building.cell_y = 0;
    let mut mover = make_test_entity("E1", EntityCategory::Infantry);
    mover.cell_x = 6;
    mover.cell_y = 0;
    assert_eq!(
        sim.spawn_from_map(&[building, mover], Some(&rules), &empty_heights()),
        2
    );
    sim.resolve_type_handles(&rules);
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let before_path = sim.path_grid_snapshot().unwrap();
    for ry in 0..3 {
        for rx in 0..4 {
            assert!(sim.substrate.occupancy.contains_entity(rx, ry, 1));
            assert!(!before_path.is_walkable(rx, ry));
        }
    }
    let _ = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 5,
            ry: 5,
            damage: 15,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 0, // Z-gate window for level=0 is [-1, 1]
        }],
    );

    // A bridge refresh must publish the complete world projection before the
    // next reader, including unrelated foundations. No end-frame repair here.
    for ry in 0..3 {
        for rx in 0..4 {
            assert!(sim.substrate.occupancy.contains_entity(rx, ry, 1));
            assert!(
                !sim.path_grid().unwrap().is_walkable(rx, ry),
                "bridge collapse lost unrelated foundation blocker at {rx},{ry}"
            );
            assert!(!before_path.is_walkable(rx, ry), "pinned reader changed");
        }
    }
    let bs = sim.bridge_state.as_ref().unwrap();
    assert_eq!(
        bs.cell(5, 5).unwrap().damage_state,
        DamageState::Destroyed,
        "body driver collapsed anchor on Damaged→Destroyed"
    );
    // Endpoint deactivation: at least one record was active pre-collapse
    // and any active record for group 1 is now inactive.
    if pre_active.iter().any(|&a| a) {
        let post_active: Vec<bool> = bs.endpoint_records().iter().map(|r| r.active).collect();
        assert!(
            post_active.iter().all(|&a| !a),
            "all group-1 endpoints must deactivate after collapse \
             (pre={pre_active:?}, post={post_active:?})"
        );
    }
    let collapsed = sim.path_grid_snapshot().unwrap();
    assert_ne!(before_path.cell(5, 5), collapsed.cell(5, 5));
    // The combat frame's fallback predates bridge fallout. Both receipt paths
    // must return and publish the current projection without mutating that Arc.
    for changed_cells in [&[][..], &[(6, 0)][..]] {
        let tail = sim
            .finish_terrain_navigation_changes(Some(&before_path), changed_cells)
            .unwrap();
        assert_eq!(tail.as_ref(), collapsed.as_ref());
        if changed_cells.is_empty() {
            assert!(
                Arc::ptr_eq(&tail, &collapsed),
                "no-change readers reuse the immutable projection"
            );
        }
        assert_eq!(sim.path_grid(), Some(collapsed.as_ref()));
        assert_ne!(before_path.cell(5, 5), tail.cell(5, 5));

        // Real Phase-6 order resumption consumes the returned grid alongside
        // the live zone owner, before any frame-final projection rebuild.
        let unit = sim.substrate.entities.get_mut(2).unwrap();
        unit.movement_target = None;
        unit.order_intent = Some(crate::sim::components::OrderIntent::AttackMove {
            goal_rx: 6,
            goal_ry: 3,
        });
        sim.tick_order_intents_post_combat(Some(&tail), Some(&rules));
        let target = sim
            .substrate
            .entities
            .get(2)
            .unwrap()
            .movement_target
            .as_ref()
            .unwrap();
        assert_eq!(target.path.last(), Some(&(6, 3)));
        assert!(target.path.iter().all(|&(rx, ry)| !(rx < 4 && ry < 3)));
    }

    // The persisted bridge/entity owners must reconstruct the same navigation
    // projection; a save/load must not be what repairs a lost foundation.
    let expected_path = sim.path_grid_snapshot().unwrap();
    let terrain_cache = sim.resolved_terrain.as_ref().unwrap().clone();
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "bridge-nav", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    restored.rebuild_caches_after_load(terrain_cache, Default::default(), Vec::new(), Vec::new());
    assert!(restored.rebuild_dynamic_navigation(&rules));
    assert_eq!(restored.path_grid(), Some(expected_path.as_ref()));
}

/// Determinism: two independent simulations with identical seeds, identical
/// resolved terrain, identical bridge runtime state, and identical damage
/// events MUST produce the same state hash after running
/// `apply_bridge_damage_events`. Lockstep invariant — any divergence in
/// RNG draw order, iteration order, or non-deterministic sets desyncs
/// multiplayer.
#[test]
fn test_bridge_collapse_is_deterministic_under_replay() {
    fn run_one_collapse(seed: u64) -> u64 {
        let mut sim = Simulation::new();
        sim.reseed_scenario_and_main(seed);
        let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 4, false, 0);
        sim.resolved_terrain = Some(resolved);
        sim.bridge_state = Some(bridge_state);

        let mut rules = combat_test_rules();
        sim.resolve_type_handles(&rules);
        let _ = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
            &mut sim,
            &rules,
            &[BridgeDamageEvent {
                rx: 5,
                ry: 5,
                damage: 100,
                warhead_ref: crate::sim::intern::InternedId::default(),
                is_ion_cannon: false, // exercises per-path RNG gate
                impact_z: 4,
            }],
        );
        sim.state_hash()
    }

    let h1 = run_one_collapse(0xCAFE_F00D);
    let h2 = run_one_collapse(0xCAFE_F00D);
    assert_eq!(
        h1, h2,
        "identical seed + inputs must produce identical post-collapse state hash"
    );
}

/// Replay determinism with bridge collapse + rim refresh. The new
/// `update_adjacent_bridges` step in the cascade introduces additional
/// `BridgeRuntimeCell` writes (`damaged_variant`, `damage_state` resets).
/// This test pins that those mutations are deterministic across two
/// identical-seed runs, so the new sim writes can never silently desync
/// lockstep.
#[test]
fn replay_determinism_with_bridge_collapse_and_rim_refresh() {
    fn run_one(seed: u64) -> u64 {
        let mut sim = Simulation::new();
        sim.reseed_scenario_and_main(seed);
        let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 4, false, 0);
        sim.resolved_terrain = Some(resolved);
        sim.bridge_state = Some(bridge_state);

        let mut rules = combat_test_rules();
        sim.resolve_type_handles(&rules);
        let _ = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
            &mut sim,
            &rules,
            &[BridgeDamageEvent {
                rx: 5,
                ry: 5,
                damage: 100,
                warhead_ref: crate::sim::intern::InternedId::default(),
                is_ion_cannon: false,
                impact_z: 4,
            }],
        );
        sim.state_hash()
    }

    let h1 = run_one(0xFEED_BEEF);
    let h2 = run_one(0xFEED_BEEF);
    assert_eq!(
        h1, h2,
        "identical seed + inputs must produce identical state hash across the rim-refresh cascade"
    );
}

/// Snapshot regression: serialize the `BridgeRuntimeState` after a collapse
/// (overlay-byte progression + DamageState::Destroyed cells +
/// endpoint_records active flips), deserialize it, and assert the
/// post-restore state matches the pre-serialize state. Locks down the
/// snapshot contract across the orchestrator switchover.
#[test]
fn test_bridge_snapshot_roundtrip_preserves_state_after_collapse() {
    use crate::sim::bridge_state::DamageState;
    let mut sim = Simulation::new();
    let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 4, false, 0);
    sim.resolved_terrain = Some(resolved);
    sim.bridge_state = Some(bridge_state);

    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    let _ = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 5,
            ry: 5,
            damage: 15,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z: 4,
        }],
    );

    let pre = sim.bridge_state.as_ref().unwrap().clone();
    let json = serde_json::to_string(&pre).expect("serialize bridge_state");
    let restored: crate::sim::bridge_state::BridgeRuntimeState =
        serde_json::from_str(&json).expect("deserialize");

    // Compare every cell in the strip + bridge_strength + endpoint_records.
    for x in 4..=6 {
        let pre_cell = pre.cell(x, 5).expect("pre cell");
        let post_cell = restored.cell(x, 5).expect("restored cell");
        assert_eq!(pre_cell, post_cell, "cell ({x}, 5) round-trip");
        assert_eq!(post_cell.damage_state, DamageState::Destroyed);
        assert_eq!(post_cell.overlay_byte, 0xE8);
    }
    assert_eq!(pre.bridge_strength(), restored.bridge_strength());
    assert_eq!(
        pre.endpoint_records().len(),
        restored.endpoint_records().len()
    );
    for (a, b) in pre
        .endpoint_records()
        .iter()
        .zip(restored.endpoint_records())
    {
        assert_eq!(a.active, b.active, "endpoint record active flag round-trip");
        assert_eq!(
            a.bridge_kind, b.bridge_kind,
            "endpoint record kind round-trip"
        );
    }
}

/// RNG draw-count parity: per-event the dispatcher consumes RNG draws in
/// a fixed sequence. With non-IonCannon damage:
///   1. Per-path BridgeStrength gate fires once before the first matching
///      driver — `next_range_u32_inclusive(1, bridge_strength)`.
///   2. Driver dispatch (walker / state machine) does not draw RNG itself.
///   3. Cascade `spawn_bridge_debris` per destroyed cell consumes its
///      well-known sequence (covered by orchestrator unit tests).
///
/// This integration test pins step 1: with `is_ion_cannon=false`, the
/// orchestrator pulls exactly one BridgeStrength roll before falling
/// through to HighDirect (HighSM raw-overlay rejects, LowSM rejects,
/// LowDirect rejects). A parallel RNG primed with the same seed must
/// yield the same post-event state.
#[test]
fn test_bridge_dispatcher_consumes_one_path_gate_draw_per_non_ion_event() {
    let seed = 0xABCD_1234_u64;
    let mut sim = Simulation::new();
    sim.reseed_scenario_and_main(seed);
    let main_before = sim.main_rng.logical_state();
    let mapgen_before = sim.mapgen_rng.logical_state();
    let (resolved, bridge_state) = ew_high_bridge_strip_for_dispatch(5, 5, 4, false, 0);
    let bridge_strength = bridge_state.bridge_strength();
    sim.resolved_terrain = Some(resolved);
    sim.bridge_state = Some(bridge_state);

    // Predict: HighSM rejected on raw-overlay; LowSM rejected
    // (deck_level=4 vs want_high=false); LowDirect rejected (overlay not
    // in LOW range). HighDirect matches → one BridgeStrength gate roll
    // → walker (consumes no RNG) → cascade spawn_bridge_debris (consumes
    // the well-known per-cell sequence — but with both bridge_explosions
    // and metallic_debris empty in this fixture, the helper short-circuits
    // on the empty-lists check and draws no RNG).
    let mut predicted = crate::sim::rng::SimRng::new(seed);
    let _gate = predicted.next_range_u32_inclusive(1, bridge_strength as u32);

    let mut rules = combat_test_rules();
    sim.resolve_type_handles(&rules);
    // High damage so the gate roll passes deterministically (any roll < 9999
    // succeeds when damage > roll).
    let _ = crate::sim::world::bridge_orchestrator::apply_bridge_damage_events(
        &mut sim,
        &rules,
        &[BridgeDamageEvent {
            rx: 5,
            ry: 5,
            damage: 9999,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: false,
            impact_z: 4,
        }],
    );

    assert_eq!(
        sim.scenario_rng.logical_state(),
        predicted.logical_state(),
        "non-IonCannon hit must consume exactly one BridgeStrength gate roll"
    );
    assert_eq!(
        sim.main_rng.logical_state(),
        main_before,
        "bridge damage dispatch must not consume Main"
    );
    assert_eq!(
        sim.mapgen_rng.logical_state(),
        mapgen_before,
        "bridge damage dispatch must not consume MapGen"
    );
}

#[test]
fn test_water_mover_lookahead_does_not_attach_bridge_occupancy_under_bridge() {
    let rules = naval_bridge_test_rules();
    let mut sim = Simulation::new();
    let resolved = bridge_cell_with_ground_block(1, 0, 3, true, 0);
    install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain(
        &resolved, true, 15,
    ));
    let boat_id = sim
        .spawn_object("BOAT", "Americans", 0, 0, 64, &rules, &BTreeMap::new())
        .expect("spawn boat");
    let boat = sim
        .substrate
        .entities
        .get_mut(boat_id)
        .expect("boat entity");
    boat.movement_target = Some(MovementTarget {
        path: vec![(0, 0), (1, 0)],
        path_layers: vec![MovementLayer::Ground, MovementLayer::Ground],
        next_index: 1,
        speed: SimFixed::from_num(256),
        current_speed: SimFixed::from_num(256),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });

    let path_grid = PathGrid::new(2, 1);
    let _ = sim.advance_tick(
        &[],
        Some(&rules),
        &BTreeMap::new(),
        Some(&path_grid),
        None,
        33,
    );

    let boat = sim
        .substrate
        .entities
        .get(boat_id)
        .expect("boat still exists");
    assert!(
        boat.bridge_occupancy.is_none(),
        "Ship under a bridge should stay on the water layer"
    );
    assert_eq!(boat.position.z, 0);
}

#[test]
fn test_too_big_ship_can_move_under_bridge_route() {
    let rules = naval_bridge_test_rules();
    let mut sim = Simulation::new();
    // Build a 2x1 water terrain where cell (1,0) has a bridge deck.
    // Water movers need land_type=4 (Water) for passability.
    let mut resolved = water_terrain(2, 1);
    let idx = resolved.index(1, 0).expect("bridge cell index");
    resolved.cells[idx].has_bridge_deck = true;
    resolved.cells[idx].bridge_walkable = true;
    resolved.cells[idx].bridge_transition = true;
    resolved.cells[idx].bridge_deck_level = 3;
    resolved.cells[idx].ground_walk_blocked = true;
    resolved.cells[idx].build_blocked = true;
    install_rectangular_test_playfield(&mut sim, resolved.width(), resolved.height());
    sim.resolved_terrain = Some(resolved.clone());
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain(
        &resolved, true, 15,
    ));
    let ship_id = sim
        .spawn_object("DRED", "Americans", 0, 0, 64, &rules, &BTreeMap::new())
        .expect("spawn dreadnought");
    let ship = sim
        .substrate
        .entities
        .get_mut(ship_id)
        .expect("ship entity");
    ship.movement_target = Some(MovementTarget {
        path: vec![(0, 0), (1, 0)],
        path_layers: vec![MovementLayer::Ground, MovementLayer::Ground],
        next_index: 1,
        speed: SimFixed::from_num(256),
        current_speed: SimFixed::from_num(256),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });

    // TooBigToFitUnderBridge is rendering-only in retail. Advance admitted
    // native frames until the one-cell route completes; host milliseconds do
    // not scale locomotor movement.
    let path_grid = PathGrid::new(2, 1);
    for _ in 0..16 {
        let _ = sim.advance_tick(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            Some(&path_grid),
            None,
            1,
        );
        if sim
            .substrate
            .entities
            .get(ship_id)
            .is_some_and(|ship| ship.movement_target.is_none())
        {
            break;
        }
    }

    let ship = sim
        .substrate
        .entities
        .get(ship_id)
        .expect("ship still exists");
    assert!(
        ship.movement_target.is_none(),
        "TooBigToFitUnderBridge must not gate the ship's direct retail route"
    );
    assert_eq!((ship.position.rx, ship.position.ry), (1, 0));
}

#[test]
fn test_ship_turn_path_completes_without_drive_track_stall() {
    let rules = naval_bridge_test_rules();
    let mut sim = Simulation::new();
    // Water movers need resolved_terrain with water cells (land_type=4) for
    // the passability check in is_cell_passable_for_mover.
    install_rectangular_test_playfield(&mut sim, 3, 3);
    sim.resolved_terrain = Some(water_terrain(3, 3));
    let boat_id = sim
        .spawn_object("BOAT", "Americans", 0, 0, 64, &rules, &BTreeMap::new())
        .expect("spawn boat");
    let boat = sim
        .substrate
        .entities
        .get_mut(boat_id)
        .expect("boat entity");
    boat.movement_target = Some(MovementTarget {
        path: vec![(0, 0), (1, 0), (1, 1)],
        path_layers: vec![
            MovementLayer::Ground,
            MovementLayer::Ground,
            MovementLayer::Ground,
        ],
        next_index: 1,
        speed: SimFixed::from_num(1024),
        current_speed: SimFixed::from_num(1024),
        move_dir_x: SimFixed::from_num(256),
        move_dir_y: SIM_ZERO,
        move_dir_len: SimFixed::from_num(256),
        ..Default::default()
    });

    let path_grid = PathGrid::new(3, 3);
    for _ in 0..10 {
        let _ = sim.advance_tick(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            Some(&path_grid),
            None,
            100,
        );
    }

    let boat = sim
        .substrate
        .entities
        .get(boat_id)
        .expect("boat still exists");
    assert_eq!(
        (boat.position.rx, boat.position.ry),
        (1, 1),
        "ship should finish a simple turn path instead of stalling in place"
    );
    assert!(
        boat.movement_target.is_none(),
        "ship movement should complete after reaching the goal"
    );
}

#[test]
fn test_real_ship_locomotor_move_command_crosses_water_cells() {
    let rules = real_ship_test_rules();
    let mut sim = Simulation::new();
    let terrain = water_terrain(4, 4);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    install_rectangular_test_playfield(&mut sim, terrain.width(), terrain.height());
    sim.resolved_terrain = Some(terrain.clone());

    sim.terrain_costs.insert(
        crate::rules::locomotor_type::SpeedType::Float,
        crate::sim::pathfinding::terrain_cost::TerrainCostGrid::from_resolved_terrain(
            &terrain,
            crate::rules::locomotor_type::SpeedType::Float,
        ),
    );

    let ship_id = sim
        .spawn_object("DEST", "Americans", 0, 0, 64, &rules, &BTreeMap::new())
        .expect("spawn destroyer");
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Move {
            entity_id: ship_id,
            target_rx: 3,
            target_ry: 1,
            queue: false,
            group_id: None,
        },
    );

    let _ = sim.advance_tick(
        &[cmd],
        Some(&rules),
        &BTreeMap::new(),
        Some(&path_grid),
        None,
        100,
    );
    // GSI-13.06: Ship Process_Drive_Track (0x6A05F0) spends the integer
    // GetCurrentSpeed budget in strict 7-unit points; DEST's default ramp can
    // reach the 0.3 brake floor while its final raw-track tail is still live.
    for _ in 0..100 {
        let _ = sim.advance_tick(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            Some(&path_grid),
            None,
            100,
        );
        if sim
            .substrate
            .entities
            .get(ship_id)
            .is_some_and(|ship| ship.movement_target.is_none())
        {
            break;
        }
    }

    let ship = sim
        .substrate
        .entities
        .get(ship_id)
        .expect("ship still exists");
    assert_eq!(
        (ship.position.rx, ship.position.ry),
        (3, 1),
        "real Ship locomotor should complete a simple move command over water"
    );
    assert!(
        ship.movement_target.is_none(),
        "real Ship locomotor should finish its move command"
    );
}

#[test]
fn test_real_ship_locomotor_crosses_water_surface_cells_with_non_water_land_type() {
    let rules = real_ship_test_rules();
    let mut sim = Simulation::new();
    // Real maps contain water-surface tiles that keep is_water=true while carrying
    // shoreline/coast land_type values. Ships should still navigate them.
    let terrain = water_terrain_with_land_type(4, 4, 7, false);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    install_rectangular_test_playfield(&mut sim, terrain.width(), terrain.height());
    sim.resolved_terrain = Some(terrain.clone());

    sim.terrain_costs.insert(
        crate::rules::locomotor_type::SpeedType::Float,
        crate::sim::pathfinding::terrain_cost::TerrainCostGrid::from_resolved_terrain(
            &terrain,
            crate::rules::locomotor_type::SpeedType::Float,
        ),
    );

    let ship_id = sim
        .spawn_object("DEST", "Americans", 0, 0, 64, &rules, &BTreeMap::new())
        .expect("spawn destroyer");
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Move {
            entity_id: ship_id,
            target_rx: 3,
            target_ry: 1,
            queue: false,
            group_id: None,
        },
    );

    let _ = sim.advance_tick(
        &[cmd],
        Some(&rules),
        &BTreeMap::new(),
        Some(&path_grid),
        None,
        100,
    );
    for _ in 0..100 {
        let _ = sim.advance_tick(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            Some(&path_grid),
            None,
            100,
        );
        if sim
            .substrate
            .entities
            .get(ship_id)
            .is_some_and(|ship| ship.movement_target.is_none())
        {
            break;
        }
    }

    let ship = sim
        .substrate
        .entities
        .get(ship_id)
        .expect("ship still exists");
    assert_eq!(
        (ship.position.rx, ship.position.ry),
        (3, 1),
        "real Ship locomotor should treat water-surface cells as navigable even when land_type is not the pure water column"
    );
    assert!(
        ship.movement_target.is_none(),
        "real Ship locomotor should finish its move command on water-surface cells"
    );
}

#[test]
fn test_real_ship_move_command_can_path_under_bridge_when_too_big() {
    let rules = real_ship_test_rules();
    let mut sim = Simulation::new();
    let mut terrain = water_terrain(5, 3);
    let bridge_idx = terrain.index(2, 1).expect("bridge cell index");
    terrain.cells[bridge_idx].bridge_deck_level = 1;
    terrain.cells[bridge_idx].bridge_walkable = true;
    terrain.cells[bridge_idx].bridge_transition = true;
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    install_rectangular_test_playfield(&mut sim, terrain.width(), terrain.height());
    sim.resolved_terrain = Some(terrain.clone());

    sim.terrain_costs.insert(
        crate::rules::locomotor_type::SpeedType::Float,
        crate::sim::pathfinding::terrain_cost::TerrainCostGrid::from_resolved_terrain(
            &terrain,
            crate::rules::locomotor_type::SpeedType::Float,
        ),
    );

    let ship_id = sim
        .spawn_object("DEST", "Americans", 0, 1, 64, &rules, &BTreeMap::new())
        .expect("spawn destroyer");
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Move {
            entity_id: ship_id,
            target_rx: 4,
            target_ry: 1,
            queue: false,
            group_id: None,
        },
    );

    // The Move dispatches at this frame's EventClass tail; Ship69F450
    // accepts without a route, and the next frame's first Process requests it.
    for commands in [vec![cmd], Vec::new()] {
        let _ = sim.advance_tick(
            &commands,
            Some(&rules),
            &BTreeMap::new(),
            Some(&path_grid),
            None,
            100,
        );
    }
    let initial_path = sim
        .substrate
        .entities
        .get(ship_id)
        .and_then(|ship| ship.movement_target.as_ref())
        .map(|mt| mt.path.clone())
        .expect("ship should have an initial path");
    for _ in 0..120 {
        let _ = sim.advance_tick(
            &[],
            Some(&rules),
            &BTreeMap::new(),
            Some(&path_grid),
            None,
            100,
        );
    }

    let ship = sim
        .substrate
        .entities
        .get(ship_id)
        .expect("ship still exists");
    assert_eq!(
        (ship.position.rx, ship.position.ry),
        (4, 1),
        "Naval ships should still complete move commands when the straight route passes under a bridge"
    );
    assert!(
        initial_path.contains(&(2, 1)),
        "planned path should be allowed to include under-bridge structural cells for naval movers"
    );
}

#[test]
fn test_spawn_multiple_entities() {
    let mut sim: Simulation = Simulation::new();
    let entities: Vec<MapEntity> = vec![
        make_test_entity("MTNK", EntityCategory::Unit),
        make_test_entity("HTNK", EntityCategory::Unit),
        make_test_entity("E1", EntityCategory::Infantry),
        make_test_entity("GAPOWR", EntityCategory::Structure),
    ];
    let count: u32 = sim.spawn_from_map(&entities, None, &empty_heights());
    assert_eq!(count, 4);

    let total: usize = sim.substrate.entities.values().count();
    assert_eq!(total, 4);
}

#[test]
fn test_empty_entities_spawns_nothing() {
    let mut sim: Simulation = Simulation::new();
    let count: u32 = sim.spawn_from_map(&[], None, &empty_heights());
    assert_eq!(count, 0);
    assert_eq!(sim.substrate.entities.values().count(), 0);
}

#[test]
fn test_stable_ids_are_assigned() {
    let mut sim: Simulation = Simulation::new();
    let entities: Vec<MapEntity> = vec![
        make_test_entity("MTNK", EntityCategory::Unit),
        make_test_entity("E1", EntityCategory::Infantry),
    ];
    sim.spawn_from_map(&entities, None, &empty_heights());

    let mut ids: Vec<u64> = sim
        .substrate
        .entities
        .values()
        .map(|e| e.stable_id)
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, vec![1, 2]);
}

#[test]
fn test_select_command_applies_snapshot_selection() {
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[
            make_test_entity("MTNK", EntityCategory::Unit),
            make_test_entity("E1", EntityCategory::Infantry),
        ],
        None,
        &empty_heights(),
    );

    let select = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Select {
            entity_ids: vec![2],
            additive: false,
        },
    );
    let _ = sim.advance_tick(&[select], None, &empty_heights(), None, None, 33);

    assert!(!sim.substrate.entities.get(1).is_some_and(|e| e.selected));
    assert!(sim.substrate.entities.get(2).is_some_and(|e| e.selected));
}

#[test]
fn test_select_command_replaces_previous_selection() {
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[
            make_test_entity("MTNK", EntityCategory::Unit),
            make_test_entity("E1", EntityCategory::Infantry),
        ],
        None,
        &empty_heights(),
    );

    let cmd1 = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Select {
            entity_ids: vec![1],
            additive: false,
        },
    );
    let _ = sim.advance_tick(&[cmd1], None, &empty_heights(), None, None, 33);

    let cmd2 = cmd_envelope(
        &sim,
        "Americans",
        2,
        Command::Select {
            entity_ids: vec![2],
            additive: true,
        },
    );
    let _ = sim.advance_tick(&[cmd2], None, &empty_heights(), None, None, 33);

    assert!(!sim.substrate.entities.get(1).is_some_and(|e| e.selected));
    assert!(sim.substrate.entities.get(2).is_some_and(|e| e.selected));
}

#[test]
fn test_select_command_deduplicates_without_reordering_payload() {
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[
            make_test_entity("MTNK", EntityCategory::Unit),
            make_test_entity("E1", EntityCategory::Infantry),
        ],
        None,
        &empty_heights(),
    );

    let select = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Select {
            entity_ids: vec![2, 2, 1],
            additive: false,
        },
    );
    let _ = sim.advance_tick(&[select], None, &empty_heights(), None, None, 33);

    assert!(sim.substrate.entities.get(1).is_some_and(|e| e.selected));
    assert!(sim.substrate.entities.get(2).is_some_and(|e| e.selected));
}

/// One ordinary type plus one carrying `Selectable=no` — the flag stock puts on
/// the scripted aircraft (`PDPLANE`, `SPYP`, `BPLN`), walls, and civilian props.
/// The gate is type-driven, so a ground type exercises it without dragging the
/// aircraft spawn path into the fixture.
fn selection_gate_test_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n0=MTNK\n1=NOSEL\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n\n\
         [MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\n\n\
         [NOSEL]\nStrength=150\nArmor=light\nSpeed=6\nSelectable=no\n",
    );
    RuleSet::from_ini(&ini).expect("selection gate rules should parse")
}

#[test]
fn test_select_command_rejects_selectable_no_type() {
    let mut sim: Simulation = Simulation::new();
    let rules = selection_gate_test_rules();
    let heights = empty_heights();
    let tank = sim
        .spawn_object("MTNK", "Americans", 20, 22, 0, &rules, &heights)
        .expect("spawn MTNK");
    let unselectable = sim
        .spawn_object("NOSEL", "Americans", 21, 22, 0, &rules, &heights)
        .expect("spawn NOSEL");

    let select = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Select {
            entity_ids: vec![tank, unselectable],
            additive: false,
        },
    );
    let _ = sim.advance_tick(&[select], Some(&rules), &heights, None, None, 33);

    assert!(sim.substrate.entities.get(tank).is_some_and(|e| e.selected));
    assert!(
        !sim.substrate
            .entities
            .get(unselectable)
            .is_some_and(|e| e.selected),
        "a Selectable=no object must never join the selection"
    );
}

/// Declare one human house and one AI house, the ordinary skirmish shape.
fn declare_selection_gate_houses(sim: &mut Simulation) {
    for (name, is_human) in [("Americans", true), ("Soviet", false)] {
        let id = sim.interner.intern(name);
        sim.houses.insert(
            id,
            crate::sim::house_state::HouseState::new(id, 0, None, is_human, 0, 10),
        );
    }
}

#[test]
fn item83_final_select_allows_caller_admitted_nonlocal_entity() {
    let mut sim: Simulation = Simulation::new();
    let rules = selection_gate_test_rules();
    let heights = empty_heights();
    let mine = sim
        .spawn_object("MTNK", "Americans", 20, 22, 0, &rules, &heights)
        .expect("spawn own MTNK");
    let theirs = sim
        .spawn_object("MTNK", "Soviet", 24, 22, 0, &rules, &heights)
        .expect("spawn AI MTNK");
    declare_selection_gate_houses(&mut sim);

    // The snapshot a band-box swept across a fight would produce.
    let select = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Select {
            entity_ids: vec![mine, theirs],
            additive: false,
        },
    );
    let _ = sim.advance_tick(&[select], Some(&rules), &heights, None, None, 33);

    assert!(sim.substrate.entities.get(mine).is_some_and(|e| e.selected));
    assert!(
        sim.substrate
            .entities
            .get(theirs)
            .is_some_and(|e| e.selected)
    );
}

#[test]
fn test_select_command_rejects_limbo_object() {
    let mut sim: Simulation = Simulation::new();
    let rules = selection_gate_test_rules();
    let heights = empty_heights();
    // Never revealed onto the map — the state a paradrop passenger sits in while
    // it rides inside the plane.
    let cargo = sim
        .spawn_object_limbo_at_height("MTNK", "Americans", 20, 22, 0, 0, &rules)
        .expect("spawn limbo MTNK");

    let select = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Select {
            entity_ids: vec![cargo],
            additive: false,
        },
    );
    let _ = sim.advance_tick(&[select], Some(&rules), &heights, None, None, 33);

    assert!(
        !sim.substrate
            .entities
            .get(cargo)
            .is_some_and(|e| e.selected),
        "an in-limbo object has no map presence to select"
    );
}

#[test]
fn item83_fresh_selection_rejects_warp_out_but_keeps_preexisting_selection() {
    use crate::sim::movement::teleport_movement::{TeleportPhase, TeleportState};

    let mut sim = Simulation::new();
    let rules = selection_gate_test_rules();
    let heights = empty_heights();
    let tank = sim
        .spawn_object("MTNK", "Americans", 20, 22, 0, &rules, &heights)
        .expect("spawn MTNK");
    let wingman = sim
        .spawn_object("MTNK", "Americans", 21, 22, 0, &rules, &heights)
        .expect("spawn second MTNK");
    assert!(sim.try_select_object(tank, Some(&rules)));
    sim.substrate.entities.get_mut(tank).unwrap().teleport_state = Some(TeleportState {
        phase: TeleportPhase::Relocate,
        target_rx: 30,
        target_ry: 30,
        being_warped_ticks: 0,
    });

    assert!(
        sim.substrate.entities.get(tank).unwrap().selected,
        "entering warp-out does not retroactively remove an existing selection"
    );
    assert!(sim.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: vec![tank, wingman],
            additive: true,
        },
        Some(&rules),
        None,
        &heights,
    ));
    assert!(sim.substrate.entities.get(tank).unwrap().selected);
    assert!(sim.substrate.entities.get(wingman).unwrap().selected);

    assert!(sim.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: vec![wingman],
            additive: false,
        },
        Some(&rules),
        None,
        &heights,
    ));
    assert!(
        !sim.substrate.entities.get(tank).unwrap().selected,
        "an ordinary replacement still deselects an omitted warp-out member"
    );
    assert!(!sim.try_select_object(tank, Some(&rules)));
}

#[test]
fn test_try_select_object_rejects_an_already_selected_object() {
    let mut sim: Simulation = Simulation::new();
    let rules = selection_gate_test_rules();
    let heights = empty_heights();
    let tank = sim
        .spawn_object("MTNK", "Americans", 20, 22, 0, &rules, &heights)
        .expect("spawn MTNK");

    assert!(sim.try_select_object(tank, Some(&rules)));
    assert!(
        !sim.try_select_object(tank, Some(&rules)),
        "the selection group holds no duplicates"
    );
    assert!(sim.substrate.entities.get(tank).is_some_and(|e| e.selected));
}

#[test]
fn test_deploy_mcv_replaces_vehicle_with_conyard() {
    let mut sim = Simulation::new();
    let rules = combat_test_rules();
    let heights = empty_heights();
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules, &heights)
        .expect("spawn MCV");
    if let Some(e) = sim.substrate.entities.get_mut(mcv) {
        e.selected = true;
    }

    let cmd = cmd_envelope(&sim, "Americans", 1, Command::DeployMcv { entity_id: mcv });
    let _ = sim.advance_tick(&[cmd], Some(&rules), &heights, None, None, 33);
    assert!(
        sim.substrate.entities.get(mcv).is_some(),
        "command queues the mission"
    );
    let result = sim.advance_tick(&[], Some(&rules), &heights, None, None, 33);
    assert!(
        result.spawned_entities,
        "mission conversion publishes its spawn"
    );

    assert!(
        sim.substrate.entities.get(mcv).is_none(),
        "MCV should be removed"
    );
    let gacnst_id = sim
        .interner
        .get("GACNST")
        .expect("GACNST should be interned");
    assert!(
        sim.substrate
            .entities
            .values()
            .any(|e| e.type_ref == gacnst_id && e.position.rx == 19 && e.position.ry == 21),
        "Construction yard should spawn at gamemd's deploy foundation origin"
    );
}

fn drive_fraction_writer_rules(accelerates: bool) -> RuleSet {
    let ini = IniFile::from_str(&format!(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=DRIVE\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [DRIVE]\n\
         Strength=300\n\
         Speed=6\n\
         Locomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\n\
         MovementZone=Normal\n\
         Accelerates={}\n\
         AccelerationFactor=0.03\n\
         DeaccelerationFactor=0.002\n",
        if accelerates { "yes" } else { "no" }
    ));
    RuleSet::from_ini(&ini).expect("Drive fraction writer rules")
}

#[test]
fn phase14_drive_move_command_preserves_fractions_until_scheduled_visit() {
    let heights = empty_heights();
    let grid = PathGrid::new(16, 16);

    for accelerates in [false, true] {
        let rules = drive_fraction_writer_rules(accelerates);
        let mut sim = Simulation::new();
        let entity_id = sim
            .spawn_object("DRIVE", "Americans", 2, 3, 64, &rules, &heights)
            .expect("spawn Drive vehicle");
        {
            let entity = sim
                .substrate
                .entities
                .get_mut(entity_id)
                .expect("Drive vehicle remains live");
            assert_eq!(
                entity.locomotor.as_ref().map(|locomotor| locomotor.kind),
                Some(LocomotorKind::Drive),
            );
            let drive = entity
                .drive_locomotion
                .get_or_insert_with(DriveLocomotionRuntime::default);
            drive.target_speed_fraction = SimFixed::lit("0.4");
            entity.foot_speed.applied_fraction = SimFixed::lit("0.25");
            entity.foot_speed.cached_current_speed = 7;
        }

        assert!(sim.apply_command(
            "Americans",
            &Command::Move {
                entity_id,
                target_rx: 7,
                target_ry: 3,
                queue: false,
                group_id: None,
            },
            Some(&rules),
            Some(&grid),
            &heights,
        ));

        let movement_speed = {
            let entity = sim
                .substrate
                .entities
                .get(entity_id)
                .expect("Drive vehicle remains live");
            let drive = entity.drive_locomotion.as_ref().expect("drive state");
            assert_eq!(drive.target_speed_fraction, SimFixed::lit("0.4"));
            assert_eq!(entity.foot_speed.applied_fraction, SimFixed::lit("0.25"));
            assert_eq!(entity.foot_speed.cached_current_speed, 7);
            let movement = entity
                .movement_target
                .as_ref()
                .expect("Move command installs movement target");
            assert_eq!(movement.slowdown_distance, SimFixed::from_num(500));
            movement.speed
        };

        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);

        let entity = sim
            .substrate
            .entities
            .get(entity_id)
            .expect("Drive vehicle remains live");
        let drive = entity.drive_locomotion.as_ref().expect("drive state");
        let expected_current = if accelerates {
            SimFixed::lit("0.28")
        } else {
            SIM_ONE
        };
        assert_eq!(drive.target_speed_fraction, SIM_ONE);
        assert_eq!(entity.foot_speed.applied_fraction, expected_current);
        let raw_stage = (movement_speed / SimFixed::from_num(15)).to_num::<i32>();
        let expected_owner = (SimFixed::from_num(raw_stage) * expected_current).to_num::<i32>();
        assert_eq!(entity.foot_speed.cached_current_speed, expected_owner);
    }
}

#[test]
fn test_execute_tick_delay_blocks_early_execution() {
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 2,
            cell_y: 2,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        None,
        &empty_heights(),
    );
    let grid = PathGrid::new(32, 32);
    let delayed = cmd_envelope(
        &sim,
        "Americans",
        3,
        Command::Move {
            entity_id: 1,
            target_rx: 8,
            target_ry: 2,

            queue: false,
            group_id: None,
        },
    );

    let _ = sim.advance_tick(
        &[delayed.clone()],
        None,
        &empty_heights(),
        Some(&grid),
        None,
        33,
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .and_then(|e| e.movement_target.as_ref())
            .is_none()
    );

    let _ = sim.advance_tick(
        &[delayed.clone()],
        None,
        &empty_heights(),
        Some(&grid),
        None,
        33,
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .and_then(|e| e.movement_target.as_ref())
            .is_none()
    );

    let _ = sim.advance_tick(&[delayed], None, &empty_heights(), Some(&grid), None, 33);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .and_then(|e| e.movement_target.as_ref())
            .is_some()
    );
}

#[test]
fn test_move_queue_command_appends_waypoint() {
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 2,
            cell_y: 2,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        None,
        &empty_heights(),
    );
    let grid = PathGrid::new(32, 32);
    let commands = vec![
        cmd_envelope(
            &sim,
            "Americans",
            1,
            Command::Move {
                entity_id: 1,
                target_rx: 8,
                target_ry: 2,
                queue: false,
                group_id: None,
            },
        ),
        cmd_envelope(
            &sim,
            "Americans",
            1,
            Command::Move {
                entity_id: 1,
                target_rx: 12,
                target_ry: 2,
                queue: true,
                group_id: None,
            },
        ),
    ];
    let _ = sim.advance_tick(&commands, None, &empty_heights(), Some(&grid), None, 33);

    let ge = sim
        .substrate
        .entities
        .get(1)
        .expect("entity 1 should exist in EntityStore");
    let movement = ge
        .movement_target
        .as_ref()
        .expect("movement target should be set");
    assert_eq!(movement.path.last().copied(), Some((12, 2)));
}

#[test]
fn test_stop_command_clears_move_and_attack_intent() {
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 4,
            cell_y: 4,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        None,
        &empty_heights(),
    );

    if let Some(e) = sim.substrate.entities.get_mut(1) {
        e.movement_target = Some(MovementTarget {
            path: vec![(4, 4), (5, 4)],
            path_layers: vec![MovementLayer::Ground; 2],
            next_index: 1,
            speed: SimFixed::from_num(1024),
            move_dir_x: SimFixed::from_num(256),
            move_dir_y: SIM_ZERO,
            move_dir_len: SimFixed::from_num(256),
            ..Default::default()
        });
        e.attack_target = Some(AttackTarget::new(1));
    }

    let cmd = cmd_envelope(&sim, "Americans", 1, Command::Stop { entity_id: 1 });
    let _ = sim.advance_tick(&[cmd], None, &empty_heights(), None, None, 33);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .movement_target
            .is_none(),
        "movement target should be cleared by Stop"
    );
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .attack_target
            .is_none(),
        "AttackTarget should be cleared by Stop command"
    );
}

#[test]
fn gsi_04_05_stop_preserves_committed_drive_until_reserved_head_finishes() {
    let mut sim = Simulation::new();
    sim.spawn_from_map(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 4,
            cell_y: 4,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        None,
        &empty_heights(),
    );
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        entity.drive_locomotion = Some(Default::default());
        entity.facing = 64;
    }

    let grid = PathGrid::new(16, 16);
    let issued = {
        let (entities, cell_occupation) = (
            &mut sim.substrate.entities,
            &mut sim.substrate.cell_occupation,
        );
        crate::sim::movement::issue_move_command_with_layered(
            entities,
            &grid,
            1,
            (8, 4),
            SimFixed::from_num(128),
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(cell_occupation),
            crate::sim::movement::DestinationTiming::new(0, 60),
        )
    };
    assert!(issued);
    {
        let movement = sim
            .substrate
            .entities
            .get_mut(1)
            .unwrap()
            .movement_target
            .as_mut()
            .unwrap();
        movement.accel_factor = SimFixed::lit("0.03");
        movement.decel_factor = SimFixed::lit("0.002");
        movement.slowdown_distance = SimFixed::from_num(500);
    }
    // An accepted order only installs NavCom/route. The first production
    // Process owns selection, the retained head, and its occupation claim.
    sim.process_ground_locomotor_for_test(1, None, Some(&grid), None)
        .expect("Drive Process must commit the first segment before Stop");
    let committed_track = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .drive_locomotion
        .as_ref()
        .unwrap()
        .track;
    assert!(committed_track.turn_index >= 0);
    let committed_head = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .drive_locomotion
        .as_ref()
        .and_then(|drive| drive.occupation_head_to)
        .expect("first Drive step has a committed occupation head");
    assert_eq!((committed_head.rx, committed_head.ry), (5, 4));
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .movement_target
            .as_ref()
            .unwrap()
            .final_goal,
        Some((8, 4))
    );

    assert!(sim.apply_command(
        "Americans",
        &Command::Stop { entity_id: 1 },
        None,
        Some(&grid),
        &empty_heights(),
    ));
    let stopped = sim.substrate.entities.get(1).unwrap();
    assert_eq!(stopped.navigation.nav_com, None);
    assert!(stopped.movement_target.is_some());
    let stopped_target = stopped.movement_target.as_ref().unwrap();
    assert_eq!(
        stopped_target.path,
        vec![(4, 4), (committed_head.rx, committed_head.ry)]
    );
    assert_eq!(
        stopped_target.final_goal,
        Some((committed_head.rx, committed_head.ry))
    );
    let drive = stopped.drive_locomotion.as_ref().unwrap();
    assert_eq!(
        drive.track, committed_track,
        "Stop preserves the active track"
    );
    assert!(drive.head_to.is_some());
    assert!(drive.occupation_head_to.is_some());
    assert!(sim.substrate.occupancy.contains_entity(4, 4, 1));
    assert!(
        !sim.substrate
            .occupancy
            .contains_entity(committed_head.rx, committed_head.ry, 1)
    );

    let heights = empty_heights();
    let initial_point_index = stopped.drive_locomotion.as_ref().unwrap().track.cursor;
    let mut cursor_advanced = false;
    for _ in 0..32 {
        let _ = sim.advance_tick(&[], None, &heights, Some(&grid), None, 33);
        cursor_advanced = sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .drive_locomotion
            .as_ref()
            .is_some_and(|drive| drive.track.cursor > initial_point_index);
        if cursor_advanced {
            break;
        }
    }
    assert!(
        cursor_advanced,
        "the committed Drive cursor must keep consuming after Stop clears its owner destination"
    );
    assert!(sim.substrate.occupancy.contains_entity(4, 4, 1));
    assert!(
        !sim.substrate
            .occupancy
            .contains_entity(committed_head.rx, committed_head.ry, 1)
    );
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(4, 4, MovementLayer::Ground),
        0,
        "the first paid post-Stop point clears current occupation without stranding the track"
    );

    for _ in 0..192 {
        if sim
            .substrate
            .entities
            .get(1)
            .unwrap()
            .movement_target
            .is_none()
        {
            break;
        }
        let _ = sim.advance_tick(&[], None, &heights, Some(&grid), None, 33);
    }

    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (entity.position.rx, entity.position.ry),
        (committed_head.rx, committed_head.ry)
    );
    assert!(entity.movement_target.is_none());
    let drive = entity.drive_locomotion.as_ref().unwrap();
    assert_eq!((drive.track.turn_index, drive.track.cursor), (-1, 0));
    assert_eq!(drive.head_to, None);
    assert_eq!(drive.occupation_head_to, None);
    assert!(
        sim.substrate
            .occupancy
            .contains_entity(committed_head.rx, committed_head.ry, 1)
    );
    assert_eq!(
        sim.substrate.cell_occupation.vehicle_bits(
            committed_head.rx,
            committed_head.ry,
            MovementLayer::Ground
        ),
        crate::sim::occupancy::VEHICLE_OCCUPATION_BIT
    );
    assert!(!sim.substrate.occupancy.contains_entity(6, 4, 1));
    assert!(!sim.substrate.occupancy.contains_entity(8, 4, 1));
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(6, 4, MovementLayer::Ground),
        0
    );
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(8, 4, MovementLayer::Ground),
        0
    );

    for _ in 0..32 {
        let _ = sim.advance_tick(&[], None, &heights, Some(&grid), None, 33);
    }
    let parked = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (parked.position.rx, parked.position.ry),
        (committed_head.rx, committed_head.ry),
        "Stop must remain parked at the committed head after the old route is gone"
    );
    assert!(parked.movement_target.is_none());
    let drive = parked.drive_locomotion.as_ref().unwrap();
    assert_eq!(drive.head_to, None);
    assert_eq!(drive.occupation_head_to, None);
    assert!(!sim.substrate.occupancy.contains_entity(6, 4, 1));
    assert_eq!(
        sim.substrate
            .cell_occupation
            .vehicle_bits(6, 4, MovementLayer::Ground),
        0
    );
}

#[test]
fn gsi_13_06_stop_preserves_committed_ship_segment_and_speed_state() {
    let mut sim = Simulation::new();
    sim.spawn_from_map(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "DLPH".to_string(),
            health: 256,
            cell_x: 4,
            cell_y: 4,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        None,
        &empty_heights(),
    );
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
        entity.ship_locomotion = Some(ShipLocomotionRuntime::default());
        entity.facing = 64;
    }

    let grid = PathGrid::new(16, 16);
    let issued = {
        let (entities, cell_occupation) = (
            &mut sim.substrate.entities,
            &mut sim.substrate.cell_occupation,
        );
        crate::sim::movement::issue_move_command_with_layered(
            entities,
            &grid,
            1,
            (8, 4),
            SimFixed::from_num(120),
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(cell_occupation),
            crate::sim::movement::DestinationTiming::new(0, 60),
        )
    };
    assert!(issued);
    sim.process_ground_locomotor_for_test(1, None, Some(&grid), None)
        .expect("Ship Process must commit the first segment before Stop");
    let (committed_head, committed_track) = {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let ship = entity.ship_locomotion.as_mut().expect("Ship runtime");
        assert!(ship.track.turn_index >= 0);
        ship.target_speed_fraction = SIM_ONE;
        entity.foot_speed.applied_fraction = SIM_HALF;
        entity.foot_speed.cached_current_speed = 10;
        (
            ship.head_to.expect("Ship curve has a committed head"),
            ship.track,
        )
    };
    let committed_cell = (
        u16::try_from(committed_head.x.div_euclid(256)).unwrap(),
        u16::try_from(committed_head.y.div_euclid(256)).unwrap(),
    );

    assert!(sim.apply_command(
        "Americans",
        &Command::Stop { entity_id: 1 },
        None,
        Some(&grid),
        &empty_heights(),
    ));

    let stopped = sim.substrate.entities.get(1).unwrap();
    let target = stopped
        .movement_target
        .as_ref()
        .expect("committed Ship segment survives Stop");
    assert_eq!(target.path, vec![(4, 4), committed_cell]);
    assert_eq!(target.final_goal, Some(committed_cell));
    let ship = stopped.ship_locomotion.as_ref().expect("Ship runtime");
    assert_eq!(
        ship.track, committed_track,
        "Stop preserves the active track"
    );
    assert_eq!(ship.destination, None);
    assert_eq!(ship.head_to, Some(committed_head));
    assert_eq!(ship.target_speed_fraction, SimFixed::lit("0.3"));
    assert_eq!(stopped.foot_speed.applied_fraction, SIM_HALF);
    assert_eq!(stopped.foot_speed.cached_current_speed, 10);
}

#[test]
fn gsi_13_06_shp_counter_admission_uses_only_tube_state_at_unit_ai_entry() {
    assert!(shp_vehicle_counter_admitted(false));
    assert!(!shp_vehicle_counter_admitted(true));

    let tube_active_at_entry = false;
    let tube_armed_during_ordinary_foot_visit = true;
    assert!(tube_armed_during_ordinary_foot_visit);
    assert!(
        shp_vehicle_counter_admitted(tube_active_at_entry),
        "post-Process tube state must not retroactively suppress this Foot visit"
    );
}

#[test]
fn test_move_command_rejects_non_owned_entity() {
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 2,
            cell_y: 2,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        None,
        &empty_heights(),
    );
    let grid = PathGrid::new(32, 32);
    sim.interner.intern("Russians"); // Ensure "Russians" is in sim's interner for cmd_envelope lookup.
    let cmd = cmd_envelope(
        &sim,
        "Russians",
        1,
        Command::Move {
            entity_id: 1,
            target_rx: 8,
            target_ry: 2,

            queue: false,
            group_id: None,
        },
    );

    let _ = sim.advance_tick(&[cmd], None, &empty_heights(), Some(&grid), None, 33);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .is_some_and(|e| e.movement_target.is_none())
    );
}

#[test]
fn test_move_command_chrono_miner_uses_ground_path() {
    let rules = teleport_command_test_rules();
    let mut sim: Simulation = Simulation::new();
    let heights = empty_heights();
    let entity = sim
        .spawn_object("CMIN", "Americans", 2, 2, 64, &rules, &heights)
        .expect("spawn chrono miner");
    let grid = PathGrid::new(32, 32);
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Move {
            entity_id: entity,
            target_rx: 8,
            target_ry: 2,
            queue: false,
            group_id: None,
        },
    );

    let _ = sim.advance_tick(&[cmd], Some(&rules), &heights, Some(&grid), None, 33);
    assert!(
        sim.substrate
            .entities
            .get(entity)
            .and_then(|e| e.movement_target.as_ref())
            .is_some(),
        "Chrono Miner should path like a ground unit on normal move orders"
    );
    assert!(
        sim.substrate
            .entities
            .get(entity)
            .and_then(|e| e.teleport_state.as_ref())
            .is_none(),
        "Chrono Miner should not enter teleport movement on a normal move order"
    );
}

#[test]
fn test_move_command_non_harvester_teleporter_uses_teleport() {
    let rules = teleport_command_test_rules();
    let mut sim: Simulation = Simulation::new();
    let heights = empty_heights();
    let entity = sim
        .spawn_object("CHRONO", "Americans", 2, 2, 64, &rules, &heights)
        .expect("spawn teleporter");
    let grid = PathGrid::new(32, 32);
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Move {
            entity_id: entity,
            target_rx: 8,
            target_ry: 2,
            queue: false,
            group_id: None,
        },
    );

    let _ = sim.advance_tick(&[cmd], Some(&rules), &heights, Some(&grid), None, 33);
    assert!(
        sim.substrate
            .entities
            .get(entity)
            .and_then(|e| e.teleport_state.as_ref())
            .is_some(),
        "Non-harvester teleporters should still use teleport movement"
    );
    assert!(
        sim.substrate
            .entities
            .get(entity)
            .is_some_and(|e| e.movement_target.is_none()),
        "Teleport movement should not attach a ground MovementTarget"
    );
}

#[test]
fn test_attack_move_command_chrono_miner_uses_ground_path() {
    let rules = teleport_command_test_rules();
    let mut sim: Simulation = Simulation::new();
    let heights = empty_heights();
    let entity = sim
        .spawn_object("CMIN", "Americans", 2, 2, 64, &rules, &heights)
        .expect("spawn chrono miner");
    let grid = PathGrid::new(32, 32);
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::AttackMove {
            entity_id: entity,
            target_rx: 8,
            target_ry: 2,
            queue: false,
        },
    );

    let _ = sim.advance_tick(&[cmd], Some(&rules), &heights, Some(&grid), None, 33);
    assert!(
        sim.substrate
            .entities
            .get(entity)
            .and_then(|e| e.movement_target.as_ref())
            .is_some(),
        "Chrono Miner should path on attack-move instead of teleporting"
    );
    assert!(
        sim.substrate
            .entities
            .get(entity)
            .is_some_and(|e| e.order_intent.is_some()),
        "Attack-move should still set order intent"
    );
    assert!(
        sim.substrate
            .entities
            .get(entity)
            .and_then(|e| e.teleport_state.as_ref())
            .is_none(),
        "Chrono Miner should not enter teleport movement on attack-move"
    );
}

#[test]
fn test_attack_command_rejects_friendly_target() {
    let mut sim: Simulation = Simulation::new();
    sim.house_alliances = alliance_map(&[
        ("Americans", &["Americans", "British"]),
        ("British", &["Americans", "British"]),
    ]);
    sim.spawn_from_map(
        &[
            MapEntity {
                owner: "Americans".to_string(),
                type_id: "MTNK".to_string(),
                health: 256,
                cell_x: 2,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Unit,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
            },
            MapEntity {
                owner: "British".to_string(),
                type_id: "E1".to_string(),
                health: 256,
                cell_x: 4,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Infantry,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
            },
        ],
        None,
        &empty_heights(),
    );
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Attack {
            attacker_id: 1,
            target_id: 2,
        },
    );

    let _ = sim.advance_tick(&[cmd], None, &empty_heights(), None, None, 33);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .attack_target
            .is_none(),
        "Attack on same-owner target should not issue"
    );
}

#[test]
fn test_attack_move_auto_acquires_enemy() {
    let rules = combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[
            MapEntity {
                owner: "Americans".to_string(),
                type_id: "MTNK".to_string(),
                health: 256,
                cell_x: 2,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Unit,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
            },
            MapEntity {
                owner: "Russians".to_string(),
                type_id: "E1".to_string(),
                health: 256,
                cell_x: 4,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Infantry,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
            },
        ],
        None,
        &empty_heights(),
    );
    let grid = PathGrid::new(32, 32);
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::AttackMove {
            entity_id: 1,
            target_rx: 8,
            target_ry: 2,
            queue: false,
        },
    );

    let _ = sim.advance_tick(
        &[cmd],
        Some(&rules),
        &empty_heights(),
        Some(&grid),
        None,
        100,
    );
    // Native EventClass dispatch is in Main_Tick's tail, after the object-AI
    // walk.  The command arms AttackMove here; acquisition begins next frame.
    let _ = sim.advance_tick(&[], Some(&rules), &empty_heights(), Some(&grid), None, 100);
    let attack = sim
        .substrate
        .entities
        .get(1)
        .unwrap()
        .attack_target
        .as_ref()
        .expect("attack-move should acquire target");
    assert!(matches!(
        attack.target,
        crate::sim::combat::TargetKind::Entity(2)
    ));
    assert!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .order_intent
            .is_some()
    );
}

#[test]
fn test_attack_move_lethal_hit_expires_the_target_at_the_kill() {
    let rules = combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[
            MapEntity {
                owner: "Americans".to_string(),
                type_id: "MTNK".to_string(),
                health: 256,
                cell_x: 2,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Unit,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
            },
            MapEntity {
                owner: "Russians".to_string(),
                type_id: "E1".to_string(),
                health: 256,
                cell_x: 4,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Infantry,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
            },
        ],
        None,
        &empty_heights(),
    );
    if let Some(e) = sim.substrate.entities.get_mut(2) {
        e.health.current = 50;
    }
    let grid = PathGrid::new(32, 32);
    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::AttackMove {
            entity_id: 1,
            target_rx: 8,
            target_ry: 2,
            queue: false,
        },
    );

    let _ = sim.advance_tick(
        &[cmd],
        Some(&rules),
        &empty_heights(),
        Some(&grid),
        None,
        100,
    );
    // The tail-dispatched AttackMove cannot participate in the object-AI walk
    // that preceded it.  Its first acquisition/fire opportunity is frame two.
    let _ = sim.advance_tick(&[], Some(&rules), &empty_heights(), Some(&grid), None, 100);
    let victim = sim
        .substrate
        .entities
        .get(2)
        .expect("animated victim remains stored through its death sequence");
    assert_eq!(victim.health.current, 0);
    assert!(victim.dying);
    assert!(victim.lifecycle.object_alive);

    let attacker = sim.substrate.entities.get(1).expect("attacker exists");
    assert!(
        attacker.attack_target.is_none(),
        "ObjectClass::ReceiveDamage's exact-zero Destroy (0x005F57AF) expires the target at the kill"
    );
    assert_eq!(
        attacker.order_intent,
        Some(crate::sim::components::OrderIntent::AttackMove {
            goal_rx: 8,
            goal_ry: 2
        }),
        "the attack-move order outlives its target"
    );
    assert!(
        attacker.movement_target.is_some(),
        "the post-combat order pass resumes the attack-move on the kill tick"
    );
}

/// `TechnoClass::PointerExpired @ 0x007077C0` clears a listener's Target and,
/// with a mission suspended, Restores it. The archived Target re-enters
/// `Assign_Target @ 0x006FCDB0`, which commits NULL for a Health-0 object
/// (`0x006FCEF8..0x006FCF03`). AI retaliation archives the target a unit
/// already shoots, so a killer whose current and archived targets are both its
/// victim holds no target once the killing hit's Destroy has run, while the
/// infantry corpse is still stored for its die sequence.
#[test]
fn test_lethal_hit_restore_refuses_the_dying_archived_target() {
    use crate::sim::mission::{MissionId, MissionType};

    let rules = combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[
            MapEntity {
                owner: "Americans".to_string(),
                type_id: "MTNK".to_string(),
                health: 256,
                cell_x: 2,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Unit,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
            },
            MapEntity {
                owner: "Russians".to_string(),
                type_id: "E1".to_string(),
                health: 256,
                cell_x: 4,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Infantry,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
            },
        ],
        None,
        &empty_heights(),
    );
    sim.substrate.entities.get_mut(2).unwrap().health.current = 50;
    {
        let tank = sim.substrate.entities.get_mut(1).unwrap();
        tank.attack_target = Some(AttackTarget::new(2));
        tank.suspended_attack_target = Some(crate::sim::combat::TargetKind::Entity(2));
        tank.mission
            .apply_test_fixture(crate::sim::mission::state::MissionTestFixture {
                current: MissionId::from_known(MissionType::Attack),
                suspended: MissionId::from_known(MissionType::Guard),
                queued: MissionId::NONE,
                movement_bypass_latch: 0,
                handler_state: 0,
                mission_start_frame: 0,
                ai_counter: 0,
                dispatch_timer: crate::sim::mission::MissionDispatchTimer::at_frame(0),
            });
    }
    let grid = PathGrid::new(32, 32);
    for _ in 0..8 {
        let _ = sim.advance_tick(&[], Some(&rules), &empty_heights(), Some(&grid), None, 100);
        if sim
            .substrate
            .entities
            .get(2)
            .is_some_and(|victim| victim.health.current == 0)
        {
            break;
        }
    }

    let victim = sim
        .substrate
        .entities
        .get(2)
        .expect("the corpse stays stored for its die sequence");
    assert_eq!(victim.health.current, 0);
    assert!(victim.lifecycle.object_alive);
    let tank = sim.substrate.entities.get(1).expect("the killer survives");
    assert!(
        tank.attack_target.is_none(),
        "Assign_Target refuses the dying archived target the Restore names"
    );
    assert_eq!(tank.mission.current().known(), Some(MissionType::Guard));
    assert_eq!(tank.suspended_attack_target, None);
}

/// The death arm's Stun (`0x00702210`: FootClass::Stun `0x004D5660`, then
/// TechnoClass::Stun `0x006FCD40`) drops the dying object's own Target and
/// destination at the killing hit, while the infantry corpse is still stored
/// for its die sequence.
#[test]
fn test_lethal_hit_stuns_the_dying_infantry() {
    let rules = combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[
            MapEntity {
                owner: "Russians".to_string(),
                type_id: "E1".to_string(),
                health: 256,
                cell_x: 4,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Infantry,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
            },
            MapEntity {
                owner: "Americans".to_string(),
                type_id: "MTNK".to_string(),
                health: 256,
                cell_x: 2,
                cell_y: 2,
                facing: 64,
                category: EntityCategory::Unit,
                sub_cell: 0,
                veterancy: 0,
                high: false,
                mission: None,
                recruitable_a: true,
                recruitable_b: true,
                structure_upgrades: [None, None, None],
            },
        ],
        None,
        &empty_heights(),
    );
    {
        let gi = sim.substrate.entities.get_mut(1).unwrap();
        gi.attack_target = Some(AttackTarget::new(2));
        gi.navigation.nav_com = Some(crate::sim::components::NavTargetRef::Cell { rx: 9, ry: 2 });
    }
    let warhead = sim.interner.intern("AP");
    let hit = crate::sim::combat::EntityDamageEvent::direct_receiver(
        1,
        10_000,
        0,
        crate::sim::combat::RAD_NO_ATTACKER,
        None,
        warhead,
        crate::sim::combat::ReceiverCallFlags {
            ignore_defenses: true,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit]);

    let gi = sim
        .substrate
        .entities
        .get(1)
        .expect("the corpse stays stored for its die sequence");
    assert_eq!(gi.health.current, 0);
    assert!(gi.lifecycle.object_alive);
    assert!(
        gi.attack_target.is_none(),
        "TechnoClass::Stun Assign_Target(NULL)"
    );
    assert!(
        gi.navigation.nav_com.is_none(),
        "FootClass::Stun Assign_Destination(NULL)"
    );
}

#[test]
fn test_guard_returns_to_anchor_when_displaced() {
    let rules = combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[MapEntity {
            owner: "Americans".to_string(),
            type_id: "MTNK".to_string(),
            health: 256,
            cell_x: 2,
            cell_y: 2,
            facing: 64,
            category: EntityCategory::Unit,
            sub_cell: 0,
            veterancy: 0,
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            structure_upgrades: [None, None, None],
        }],
        None,
        &empty_heights(),
    );
    let guard_cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Guard {
            entity_id: 1,
            target_id: None,
        },
    );
    let grid = PathGrid::new(32, 32);
    let _ = sim.advance_tick(
        &[guard_cmd],
        Some(&rules),
        &empty_heights(),
        Some(&grid),
        None,
        100,
    );

    sim.remove_entity_occupancy(1);
    if let Some(e) = sim.substrate.entities.get_mut(1) {
        e.position.rx = 5;
        e.position.ry = 2;
        e.movement_target = None;
        e.attack_target = None;
    }
    sim.add_entity_occupancy(1);

    let _ = sim.advance_tick(&[], Some(&rules), &empty_heights(), Some(&grid), None, 100);
    let ge = sim
        .substrate
        .entities
        .get(1)
        .expect("entity 1 should exist");
    let movement = ge
        .movement_target
        .as_ref()
        .expect("guard should re-path back to its anchor");
    assert_eq!(movement.path.last().copied(), Some((2, 2)));
}

#[test]
fn test_fog_revealed_persists_after_unit_moves_away() {
    let mut sim = Simulation::new();
    let (sx, sy) = terrain::iso_to_screen(1, 1, 0);
    use crate::sim::game_entity::GameEntity;
    let americans_id = sim.interner.intern("Americans");
    let e1_id = sim.interner.intern("E1");
    let mut ge = GameEntity::new_at_frame_zero_for_test(
        1,
        1,
        1,
        0,
        0,
        americans_id,
        crate::sim::components::Health { current: 100 },
        e1_id,
        EntityCategory::Infantry,
        0,
        // Sight=1. This test used to pass 0 and lean on VERA revealing a
        // sight-0 object's own cell; gamemd reveals nothing at all for
        // Sight=0 (36 stock types carry it), so a zero here would test the
        // sight gate rather than fog persistence.
        1,
        false,
    );
    ge.lifecycle.in_limbo = false;
    sim.substrate.entities.insert(ge);

    let grid = PathGrid::new(8, 8);
    let americans = sim.interner.get("Americans").expect("Americans interned");
    let _ = sim.advance_tick(&[], None, &empty_heights(), Some(&grid), None, 33);
    assert!(sim.fog.is_cell_visible(americans, 1, 1));
    assert!(sim.fog.is_cell_revealed(americans, 1, 1));

    let _ = (sx, sy); // suppress unused warning
    // Far enough that (1,1) leaves a Sight=1 reveal disc entirely.
    if let Some(e) = sim.substrate.entities.get_mut(1) {
        e.position.rx = 6;
        e.position.ry = 1;
    }
    let _ = sim.advance_tick(&[], None, &empty_heights(), Some(&grid), None, 33);
    assert!(!sim.fog.is_cell_visible(americans, 1, 1));
    assert!(sim.fog.is_cell_revealed(americans, 1, 1));
    assert!(sim.fog.is_cell_visible(americans, 6, 1));
}

#[test]
fn test_undeploy_conyard_spawns_mcv() {
    let mut sim = Simulation::new();
    let mut rules = combat_test_rules();
    // Retail GACNSTMK: 58 frames with shadows.
    rules.set_buildup_control_for_test("GACNST", [0, 29, 1]);
    let heights = empty_heights();
    insert_house_with_counts(&mut sim, "Americans", 0, 0);

    // First deploy an MCV to get a ConYard.
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules, &heights)
        .expect("spawn MCV");
    if let Some(e) = sim.substrate.entities.get_mut(mcv) {
        e.selected = true;
    }
    let deploy_cmd = cmd_envelope(&sim, "Americans", 1, Command::DeployMcv { entity_id: mcv });
    let _ = sim.advance_tick(&[deploy_cmd], Some(&rules), &heights, None, None, 33);
    let _ = sim.advance_tick(&[], Some(&rules), &heights, None, None, 33);

    // Find the ConYard that was spawned.
    let yard_id: u64 = sim
        .substrate
        .entities
        .values()
        .find(|e| sim.interner.resolve(e.type_ref) == "GACNST")
        .map(|e| e.stable_id)
        .expect("ConYard should exist after deploy");

    // Clear building_up so we can undeploy (can't undeploy during construction).
    if let Some(e) = sim.substrate.entities.get_mut(yard_id) {
        e.building_up = None;
        e.selected = true;
    }

    // Undeploy the ConYard: Sell's stage 0 and stage 1 visits, then the
    // build-up played in reverse from stage 1's Begin_Mode(0).
    let undeploy_cmd = cmd_envelope(
        &sim,
        "Americans",
        sim.session.tick + 1,
        Command::UndeployBuilding { entity_id: yard_id },
    );
    let undeploy_frame = sim.session.binary_frame;
    let _ = sim.advance_tick(&[undeploy_cmd], Some(&rules), &heights, None, None, 33);

    // ConYard should still exist but have building_down set.
    assert!(
        sim.substrate.entities.get(yard_id).is_some(),
        "ConYard should still exist during undeploy animation"
    );
    assert!(
        sim.substrate
            .entities
            .get(yard_id)
            .unwrap()
            .building_down
            .is_some(),
        "ConYard should have building_down component"
    );

    // The player's order stands for the retail cell click, so the pack-up
    // runs to the last frame (no archive-less stage-0x17 exit): the stage-2
    // visit two frames plus 28 steps after the command frame converts it.
    let mut converted = None;
    for _ in 0..40 {
        let frame = sim.session.binary_frame;
        let _ = sim.advance_tick(&[], Some(&rules), &heights, None, None, 33);
        if sim.substrate.entities.get(yard_id).is_none() {
            converted = Some(frame);
            break;
        }
    }
    assert_eq!(converted, Some(undeploy_frame + 2 + 28));

    // The MCV returns to the cell it deployed from — one step south-east of the
    // footprint's north-west cell, mirroring the one step north-west that deploy
    // took. Not the footprint's centre: gamemd never halves the foundation, and
    // an even-sided footprint has no centre cell to land on anyway.
    let amcv_id = sim.interner.get("AMCV").expect("AMCV should be interned");
    let mcvs: Vec<(u16, u16, bool)> = sim
        .substrate
        .entities
        .values()
        .filter(|e| e.type_ref == amcv_id)
        .map(|e| (e.position.rx, e.position.ry, e.selected))
        .collect();
    assert_eq!(mcvs.len(), 1, "Exactly one MCV should exist after undeploy");
    let (rx, ry, selected) = mcvs[0];
    // Deploy put the origin at (19, 21) from an MCV standing on (20, 22), so
    // undeploy has to hand (20, 22) back.
    assert_eq!(rx, 20, "MCV should return to the cell it deployed from, X");
    assert_eq!(ry, 22, "MCV should return to the cell it deployed from, Y");
    assert!(selected, "MCV should inherit selection from ConYard");
}

#[test]
fn level_has_single_source_of_truth_for_vision_height_derivation() {
    use crate::map::resolved_terrain::ResolvedTerrainCell;
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};

    // Build a 3x3 terrain with one elevated cell at (1,1), level=4.
    let width: u16 = 3;
    let height: u16 = 3;
    let mut cells = Vec::with_capacity((width as usize) * (height as usize));
    for y in 0..height {
        for x in 0..width {
            let level: u8 = if x == 1 && y == 1 { 4 } else { 0 };
            cells.push(ResolvedTerrainCell {
                rx: x,
                ry: y,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level,
                filled_clear: false,
                tileset_index: Some(0),
                land_type: 0,
                yr_cell_land_type: 0,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class: TerrainClass::Clear,
                speed_costs: SpeedCostProfile::default(),
                is_water: false,
                is_cliff_like: false,
                height_in_pixels: 0,
                variant: 0,
                is_rough: false,
                is_road: false,
                accepts_smudge: false,
                allows_tiberium: false,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: false,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: 0,
                base_ground_walk_blocked: false,
                base_build_blocked: false,
                base_land_type: 0,
                base_yr_cell_land_type: 0,
                base_terrain_class: Default::default(),
                base_speed_costs: Default::default(),
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0, 0, 0],
                radar_right: [0, 0, 0],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            });
        }
    }
    let terrain = ResolvedTerrainGrid::from_cells(width, height, cells);
    let grid = PathGrid::from_resolved_terrain(&terrain);

    let heights = grid.ground_height_grid();
    assert_eq!(heights.len(), (width as usize) * (height as usize));

    // Index = ry * width + rx; the elevated cell at (1,1) must report level 4.
    let idx = (1usize) * (width as usize) + 1usize;
    assert_eq!(heights[idx], 4, "elevated cell should report level 4");

    // Flat cells stay at 0.
    assert_eq!(heights[0], 0, "(0,0) is flat");
    assert_eq!(
        heights[(width as usize) * (height as usize) - 1],
        0,
        "(2,2) is flat"
    );
}

// The Phase D Task 16 bridge-atlas integration test lives in
// `src/app/presentation/instances/bridges.rs` because it imports render-layer types
// (`BridgeAtlasLookup`, `OverlaySpriteEntry`, `SpriteInstance`) — sim/
// must never depend on render/.

// --- G7 bridgehead registration: cross-rebuild + A* invariants ---

/// 5x1 high-bridge fixture with realistic bridgehead semantics:
/// ground(h=4) → bridgehead → body(water, deck=4) → bridgehead → ground(h=4).
/// Used by the two G7 invariant tests below.
fn make_realistic_bridgehead_terrain() -> ResolvedTerrainGrid {
    use crate::map::resolved_terrain::ResolvedTerrainCell;
    let cells = vec![
        ResolvedTerrainCell {
            level: 4,
            ..bridgehead_base_cell(0, 0)
        },
        ResolvedTerrainCell {
            bridge_walkable: true,
            bridge_transition: true,
            bridge_deck_level: 4,
            has_bridge_deck: true,
            ..bridgehead_base_cell(1, 0)
        },
        ResolvedTerrainCell {
            ground_walk_blocked: true,
            build_blocked: true,
            base_build_blocked: true,
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: Default::default(),
            base_speed_costs: Default::default(),
            bridge_walkable: true,
            bridge_transition: true,
            bridge_deck_level: 4,
            has_bridge_deck: true,
            is_water: true,
            ..bridgehead_base_cell(2, 0)
        },
        ResolvedTerrainCell {
            bridge_walkable: true,
            bridge_transition: true,
            bridge_deck_level: 4,
            has_bridge_deck: true,
            ..bridgehead_base_cell(3, 0)
        },
        ResolvedTerrainCell {
            level: 4,
            ..bridgehead_base_cell(4, 0)
        },
    ];
    ResolvedTerrainGrid::from_cells(5, 1, cells)
}

fn bridgehead_base_cell(rx: u16, ry: u16) -> crate::map::resolved_terrain::ResolvedTerrainCell {
    use crate::map::resolved_terrain::ResolvedTerrainCell;
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    let speed_costs = SpeedCostProfile {
        foot: Some(100),
        track: Some(100),
        wheel: Some(100),
        float: Some(100),
        amphibious: Some(100),
        float_beach: Some(100),
        hover: Some(100),
    };
    ResolvedTerrainCell {
        rx,
        ry,
        source_tile_index: 0,
        source_sub_tile: 0,
        final_tile_index: 0,
        final_sub_tile: 0,
        is_wood_bridge_repair_tile: false,
        level: 0,
        filled_clear: false,
        tileset_index: Some(0),
        land_type: 0,
        yr_cell_land_type: 0,
        slope_type: 0,
        template_height: 0,
        render_offset_x: 0,
        render_offset_y: 0,
        terrain_class: TerrainClass::Clear,
        speed_costs,
        is_water: false,
        is_cliff_like: false,
        height_in_pixels: 0,
        variant: 0,
        is_rough: false,
        is_road: false,
        accepts_smudge: false,
        allows_tiberium: false,
        has_ramp: false,
        canonical_ramp: None,
        ground_walk_blocked: false,
        terrain_object_blocks: false,
        terrain_object_occupation: None,
        overlay_blocks: false,
        overlay_zone_type: None,
        outside_playfield: false,
        zone_type: 0,
        base_ground_walk_blocked: false,
        base_build_blocked: false,
        base_land_type: 0,
        base_yr_cell_land_type: 0,
        base_terrain_class: Default::default(),
        base_speed_costs: speed_costs,
        build_blocked: false,
        has_bridge_deck: false,
        bridge_walkable: false,
        bridge_transition: false,
        bridge_deck_level: 0,
        bridge_layer: None,
        bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
        tube_index: None,
        radar_left: [0, 0, 0],
        radar_right: [0, 0, 0],
        has_damaged_data: false,
        bridgehead_anchor_class_at_load: None,
    }
}

#[test]
fn test_bridgehead_walkability_invariant_across_non_bridge_rebuild_triggers() {
    // Simulation's frame finalizer republishes canonical navigation after
    // structure, bridge, or overlay passability changes. Calling the shared
    // bridge-aware projection N times models those rebuilds; bridgehead
    // walkability must hold across every publication.
    let mut sim = Simulation::new();
    let terrain = make_realistic_bridgehead_terrain();
    sim.resolved_terrain = Some(terrain.clone());
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain(
        &terrain, true, 10,
    ));

    for trigger_idx in 0..3 {
        let grid = PathGrid::from_resolved_terrain_with_bridges(
            sim.resolved_terrain.as_ref().unwrap(),
            sim.bridge_state.as_ref(),
        );
        for rx in [1u16, 3] {
            let pc = grid
                .cell(rx, 0)
                .expect("bridgehead cell exists in path grid");
            assert!(
                pc.bridge_walkable,
                "bridgehead ({rx},0) lost bridge_walkable on rebuild #{trigger_idx}"
            );
            assert!(
                pc.transition,
                "bridgehead ({rx},0) lost transition on rebuild #{trigger_idx}"
            );
        }
    }
}

#[test]
fn test_layered_astar_can_traverse_bridge_after_unrelated_rebuild() {
    // Build a sim with the realistic bridgehead fixture. Find an A* layered
    // path Ground(0,0) → Bridge(1,0)..(3,0) → Ground(4,0). Then rebuild the
    // PathGrid (simulating an unrelated event like a building dying somewhere
    // off-bridge) and re-find the same path. PRE-G7 this would fail on the
    // second find: rebuild flips bridgehead bridge_walkable false → A* can't
    // enter the bridge layer. POST-G7 both finds succeed.
    let mut sim = Simulation::new();
    let terrain = make_realistic_bridgehead_terrain();
    sim.resolved_terrain = Some(terrain.clone());
    sim.bridge_state = Some(BridgeRuntimeState::from_resolved_terrain(
        &terrain, true, 10,
    ));

    let grid_initial = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().unwrap(),
        sim.bridge_state.as_ref(),
    );
    let path_initial = crate::sim::pathfinding::find_layered_path(
        &grid_initial,
        None,
        None,
        (0, 0),
        MovementLayer::Ground,
        (4, 0),
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(
        path_initial.is_some(),
        "intact bridge must allow Ground→Bridge→Ground A* path"
    );

    // Simulate canonical navigation publication after an unrelated structure
    // or overlay-authority change.
    let grid_after_rebuild = PathGrid::from_resolved_terrain_with_bridges(
        sim.resolved_terrain.as_ref().unwrap(),
        sim.bridge_state.as_ref(),
    );
    let path_after_rebuild = crate::sim::pathfinding::find_layered_path(
        &grid_after_rebuild,
        None,
        None,
        (0, 0),
        MovementLayer::Ground,
        (4, 0),
        None,
        None,
        None,
        0,
        false,
        false,
    );
    assert!(
        path_after_rebuild.is_some(),
        "A* path must still exist after an unrelated rebuild (G7: bridgeheads \
         must keep bridge_walkable across PathGrid refresh)"
    );
}

// --- Slice 6: deferred-delete Dying-window behavior ---

/// Insert a revealed, occupancy-marked 2x2 structure owned by `Americans`.
#[cfg(test)]
fn insert_revealed_structure(sim: &mut Simulation, id: u64, rx: u16, ry: u16) {
    let mut s = GameEntity::test_default(id, "GAPOWR", "Americans", rx, ry);
    s.owner = sim.interner.intern("Americans");
    s.type_ref = sim.interner.intern("GAPOWR");
    s.category = EntityCategory::Structure;
    s.foundation = "2x2".to_string();
    sim.substrate.entities.insert(s);
    sim.reveal(id);
}

/// Immediate (structure) path: `uninit` leaves the entity resolvable-but-`Dying`
/// (off logic, off occupancy, enqueued) until the end-of-tick flush frees the slot.
#[test]
fn immediate_structure_death_is_dying_then_flushed() {
    let mut sim = Simulation::new();
    insert_revealed_structure(&mut sim, 7, 4, 5);

    // Alive before death: on the logic order and on every foundation cell.
    assert!(sim.live_object_order_snapshot().contains(&7));
    assert!(sim.substrate.occupancy.contains_entity(4, 5, 7));

    sim.uninit(7);

    // The deferred-delete window: still in the store as Dying, but off logic +
    // off occupancy + enqueued for the end-of-tick drain.
    assert!(sim.substrate.entities.get(7).is_some_and(|e| e.dying));
    assert!(!sim.live_object_order_snapshot().contains(&7));
    for cell in [(4, 5), (4, 6), (5, 5), (5, 6)] {
        assert!(
            !sim.substrate.occupancy.contains_entity(cell.0, cell.1, 7),
            "dying structure must be off occupancy cell {cell:?}"
        );
    }
    assert!(sim.substrate.pending_delete.contains(&7));

    sim.flush_pending_delete();
    assert!(sim.substrate.entities.get(7).is_none());
    assert!(sim.substrate.pending_delete.is_empty());
}

/// Mutual same-tick death: both structures resolve as `Dying` in death order until
/// the flush, and the pre-flush state is replay-deterministic across two runs.
#[test]
fn mutual_same_tick_death_both_dying_then_flushed() {
    fn build() -> Simulation {
        let mut sim = Simulation::new();
        insert_revealed_structure(&mut sim, 1, 4, 5);
        insert_revealed_structure(&mut sim, 2, 8, 5);
        sim
    }

    let mut a = build();
    a.uninit(1);
    a.uninit(2);
    assert!(a.substrate.entities.get(1).is_some_and(|e| e.dying));
    assert!(a.substrate.entities.get(2).is_some_and(|e| e.dying));
    // Drain order = death (enqueue) order, deterministic.
    assert_eq!(a.substrate.pending_delete, vec![1, 2]);

    // Determinism: an identical second run hashes equal at the pre-flush point.
    let mut b = build();
    b.uninit(1);
    b.uninit(2);
    assert_eq!(
        a.state_hash(),
        b.state_hash(),
        "pre-flush mutual-death state must be replay-deterministic",
    );

    a.flush_pending_delete();
    assert!(a.substrate.entities.get(1).is_none());
    assert!(a.substrate.entities.get(2).is_none());
    assert!(a.substrate.pending_delete.is_empty());
}

/// Animated (infantry/SHP) compatibility path: completion requests central UnInit.
/// The corpse remains resolvable until the next ordinary simulation tail commits
/// the frame and performs the sole pending-delete drain.
#[test]
fn animated_death_uninit_waits_for_ordinary_tail_drain() {
    let mut sim = Simulation::new();
    let mut inf = GameEntity::test_default(5, "E1", "Americans", 3, 3);
    inf.owner = sim.interner.intern("Americans");
    inf.type_ref = sim.interner.intern("E1");
    inf.category = EntityCategory::Infantry;
    inf.mission_leaf =
        crate::sim::mission::leaf::MissionLeafState::for_entity_category(EntityCategory::Infantry);
    sim.substrate.entities.insert(inf);
    sim.reveal(5);

    sim.uninit(5);
    assert!(sim.substrate.entities.get(5).is_some_and(|e| e.dying));
    assert!(sim.substrate.pending_delete.contains(&5));

    sim.advance_tick(&[], None, &BTreeMap::new(), None, None, 67);
    assert!(sim.substrate.entities.get(5).is_none());
    assert!(sim.substrate.pending_delete.is_empty());
}

/// Command-applied death (here: selling a power plant) is UnInit'd during command
/// application but remains resolvable until the ordinary tail drain. Earlier
/// systems must gate on lifecycle authority rather than counting the dead-limbo
/// object merely because it is still stored.
#[test]
fn command_death_is_ignored_before_ordinary_tail_drain() {
    use crate::sim::components::Health;

    let ini_str: &str = "\
[VehicleTypes]\n\n\
[BuildingTypes]\n0=GAPOWR\n\n\
[InfantryTypes]\n\n\
[AircraftTypes]\n\n\
[GAPOWR]\nStrength=750\nArmor=wood\nFoundation=2x2\nPower=100\n";
    let ini = IniFile::from_str(ini_str);
    let rules = RuleSet::from_ini(&ini).expect("power rules parse");

    let mut sim = Simulation::new();
    sim.input_delay_ticks = 0;
    let grid = PathGrid::test_all_passable(64, 64);
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    // Two plants: selling one still leaves a structure, so power recomputes this
    // tick. (With a single plant the owner would drop off the recompute list and
    // retain a stale reading, masking whether the sold plant was counted.)
    // Force the strings into the thread-local interner before snapshotting it.
    let _ = (
        crate::sim::intern::test_intern("GAPOWR"),
        crate::sim::intern::test_intern("Americans"),
    );
    sim.interner = crate::sim::intern::test_interner();
    let owner_id = sim.interner.intern("Americans");
    for (id, rx, ry) in [(1u64, 10u16, 10u16), (2u64, 20u16, 20u16)] {
        let mut bld = GameEntity::test_default(id, "GAPOWR", "Americans", rx, ry);
        bld.category = EntityCategory::Structure;
        bld.foundation = "2x2".to_string();
        bld.health = Health { current: 750 };
        sim.substrate.entities.insert(bld);
        sim.reveal(id);
    }

    // Tick 1: power registers both plants.
    sim.advance_tick(&[], Some(&rules), &height_map, Some(&grid), None, 100);
    assert_eq!(
        sim.power_states.get(&owner_id).map(|s| s.total_output),
        Some(200),
        "two power plants should produce 200 before sale",
    );

    // Tick 2: sell plant 1 via command. It remains stored as dead-limbo until the
    // tail, while P4 power counts only the surviving plant 2 through its lifecycle
    // gate.
    let sell = CommandEnvelope::new(
        owner_id,
        sim.session.tick + 1,
        Command::SellBuilding { entity_id: 1 },
    );
    sim.advance_tick(&[sell], Some(&rules), &height_map, Some(&grid), None, 100);

    assert!(
        sim.substrate.entities.get(1).is_none(),
        "sold plant freed this tick"
    );
    assert!(
        sim.substrate.entities.get(2).is_some(),
        "surviving plant still present"
    );
    assert!(
        sim.substrate.pending_delete.is_empty(),
        "command-death queue drained"
    );
    assert_eq!(
        sim.power_states.get(&owner_id).map(|s| s.total_output),
        Some(200),
        "power ran before EventClass sold the plant at the native command tail",
    );

    // The next object/system frame observes the tail-committed deletion.
    sim.advance_tick(&[], Some(&rules), &height_map, Some(&grid), None, 100);
    assert_eq!(
        sim.power_states.get(&owner_id).map(|s| s.total_output),
        Some(100),
        "the surviving plant is the only contributor on the following frame",
    );
}

/// Combat-death counterpart: a structure killed in combat (Phase 5) now lives in the
/// Dying window through Phase 7 and is freed only by the single end-of-tick drain. The
/// Phase-7 auto-repair scan is dying-gated, so a destroyed building on auto-repair is
/// NOT healed (no credits spent) on the death tick, and after the tick it is gone.
#[test]
fn combat_death_not_repaired_then_freed_at_end_of_tick() {
    use crate::sim::components::Health;
    use crate::sim::house_state::HouseState;

    let ini_str: &str = "\
[VehicleTypes]\n0=MTNK\n\n\
[BuildingTypes]\n0=TARGB\n\n\
[InfantryTypes]\n\n\
[AircraftTypes]\n\n\
[MTNK]\nStrength=300\nArmor=heavy\nSpeed=6\nPrimary=105mm\n\n\
[TARGB]\nStrength=750\nArmor=wood\nFoundation=1x1\nCost=1000\n\n\
[105mm]\nDamage=65\nROF=20\nRange=6\nWarhead=AP\n\n\
[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n";
    let ini = IniFile::from_str(ini_str);
    let rules = RuleSet::from_ini(&ini).expect("repair rules parse");

    let mut sim = Simulation::new();
    sim.input_delay_ticks = 0;
    let grid = PathGrid::test_all_passable(64, 64);
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();

    let mut atk = GameEntity::test_default(1, "MTNK", "Americans", 5, 5);
    // The fixture `MTNK` authors no `Turret=`, so the native body gate
    // (`UnitClass::GetFireError @ 0x00740FD0` step 17) compares its HULL
    // `+0x388`. Face it east at the building so the death/drain ordering under
    // test happens on the first tick instead of after a turn-to-fire.
    atk.facing = 64;
    atk.health = Health { current: 300 };
    // Damaged, auto-repairing enemy building MTNK destroys this tick at Phase 5.
    let mut bld = GameEntity::test_default(2, "TARGB", "Russia", 7, 5);
    bld.category = EntityCategory::Structure;
    bld.foundation = "1x1".to_string();
    bld.health = Health { current: 50 };
    bld.repairing = true;
    sim.interner = crate::sim::intern::test_interner();
    let russia = sim.interner.intern("Russia");
    sim.houses
        .insert(russia, HouseState::new(russia, 0, None, false, 1000, 10));
    sim.substrate.entities.insert(atk);
    sim.substrate.entities.insert(bld);
    sim.reveal(1);
    sim.reveal(2);
    sim.add_entity_occupancy(2);
    sim.substrate.entities.get_mut(1).unwrap().attack_target = Some(AttackTarget::new(2));

    sim.advance_tick(&[], Some(&rules), &height_map, Some(&grid), None, 100);

    assert!(
        sim.substrate.entities.get(2).is_none(),
        "building destroyed + freed by the end-of-tick drain"
    );
    assert!(
        sim.substrate.pending_delete.is_empty(),
        "end-of-tick drain emptied the queue"
    );
    assert_eq!(
        sim.houses.get(&russia).map(|h| h.economy.credits),
        Some(1000),
        "destroyed building must not be repaired at Phase 7 (dying-gated, no credits spent)",
    );
}

// ===========================================================================
// GROUP-MOVE STACKING REPRODUCTION
//
// Player report: "tanks when moved in a group stack on top of each other."
// In retail YR ground vehicles are strictly one-per-cell. These tests drive
// the real command path (Command::Move through advance_tick, including the
// staged-megamission group-destination distributor) and measure whether two
// vehicles ever end up on the same cell.
// ===========================================================================

fn stacking_repro_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n\n\
         [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=300\nArmor=heavy\nSpeed=6\n",
    );
    RuleSet::from_ini(&ini).expect("stacking repro rules should parse")
}

/// Same clear-ground world, plus a crushable infantry type and a crusher tank.
fn stacking_crusher_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\n\
         [VehicleTypes]\n0=MTNK\n\n\
         [E1]\nStrength=125\nArmor=flak\nSpeed=4\n\n\
         [MTNK]\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nStrength=300\n\
         Armor=heavy\nSpeed=6\nMovementZone=Crusher\nCrusher=yes\n",
    );
    RuleSet::from_ini(&ini).expect("stacking crusher rules should parse")
}

fn stacking_crusher_world(size: u16) -> (Simulation, RuleSet, PathGrid) {
    use crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids;
    use crate::sim::pathfinding::zone_map::ZoneGrid;

    let rules = stacking_crusher_rules();
    let mut sim = Simulation::new();
    let terrain = gsi_04_10_clear_terrain(size, size);
    sim.terrain_costs = build_canonical_terrain_cost_grids(&terrain);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    sim.zone_grid = Some(ZoneGrid::build(&grid, &sim.terrain_costs, size, size));
    install_rectangular_test_playfield(&mut sim, size, size);
    sim.resolved_terrain = Some(terrain);
    (sim, rules, grid)
}

/// The longest stretch of consecutive ticks on which `id` did not change cell
/// while it still held a movement order. A mover whose selection is refused by
/// a predicate that nothing downstream can resolve produces a bit-identical
/// tick forever; that shows up here as an unbounded run.
fn longest_stationary_run_while_ordered(series: &[(bool, (u16, u16))]) -> usize {
    let mut longest = 0usize;
    let mut open = 0usize;
    let mut previous: Option<(u16, u16)> = None;
    for &(ordered, cell) in series {
        if ordered && previous == Some(cell) {
            open += 1;
            longest = longest.max(open);
        } else {
            open = 0;
        }
        previous = Some(cell);
    }
    longest
}

/// D1 GUARD — a crusher must not be frozen by a body it is entitled to drive
/// over.
///
/// The cell occupation mask holds a bit for vehicles only: `0x20` is written
/// exclusively by `UnitClass__MarkCellOccupationBit20 @ 0x007441B0`, while
/// `InfantryClass__MarkCellOccupancy @ 0x005217C0` writes `1 << GetSubCell` into
/// the sub-cell bits of the same byte. So the selection gate — which models the
/// mask arm — must never see infantry at all, and a tank ordered through them
/// keeps moving. A gate that refused on mere unit presence would stall the tank
/// one refusal per tick, forever, because nothing downstream ever clears the
/// refusal.
#[test]
fn crusher_does_not_freeze_in_front_of_infantry() {
    for enemy_infantry in [false, true] {
        let (mut sim, rules, grid) = stacking_crusher_world(24);
        let heights = empty_heights();

        let infantry_owner = if enemy_infantry {
            "Russians"
        } else {
            "Americans"
        };
        let blocker = sim
            .spawn_object("E1", infantry_owner, 10, 10, 0, &rules, &heights)
            .expect("infantry spawns");
        let tank = sim
            .spawn_object("MTNK", "Americans", 6, 10, 64, &rules, &heights)
            .expect("tank spawns");

        let cmd = cmd_envelope(
            &sim,
            "Americans",
            1,
            Command::Move {
                entity_id: tank,
                target_rx: 16,
                target_ry: 10,
                queue: false,
                group_id: None,
            },
        );
        let _ = sim.advance_tick(&[cmd], Some(&rules), &heights, Some(&grid), None, 100);

        // Make the victim's house human so a stray "Unit lost" would be
        // audible: a crush is `RecordKill` + `UnInit` (`0x007416A0`), never
        // `ReceiveDamage`, so `Death_Announcement` (`+0x3B8`) must stay silent.
        if let Some(house) = sim
            .interner
            .get(infantry_owner)
            .and_then(|id| sim.houses.get_mut(&id))
        {
            house.is_human = true;
        }

        let mut series: Vec<(bool, (u16, u16))> = Vec::new();
        let mut arrived_at: Option<u64> = None;
        let mut entered_blocker_cell: Option<u64> = None;
        let mut refusals = 0u32;
        for tick in 0..600u64 {
            let result = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
            refusals += result.movement.selection_admission_refusals;
            let Some(e) = sim.substrate.entities.get(tank) else {
                break;
            };
            let cell = (e.position.rx, e.position.ry);
            series.push((e.movement_target.is_some(), cell));
            if cell == (10, 10) && entered_blocker_cell.is_none() {
                entered_blocker_cell = Some(tick);
            }
            if cell == (16, 10) && arrived_at.is_none() {
                arrived_at = Some(tick);
            }
        }

        let stall = longest_stationary_run_while_ordered(&series);
        println!(
            "--- crusher_does_not_freeze_in_front_of_infantry (enemy={enemy_infantry}) ---\n    \
             tank: {}\n    blocker: {}\n    longest ordered-but-stationary run = {stall} tick(s); \
             arrived_at = {arrived_at:?}; entered (10,10) at {entered_blocker_cell:?}; \
             selection refusals = {refusals}",
            stacking_motion_state(&sim, tank),
            stacking_motion_state(&sim, blocker),
        );

        // THE COUNTER ASSERTION. Arrival alone does not prove the exclusion was
        // exercised — a tank that routed politely around the man never asked the
        // gate about his cell at all. The gate is the only producer of this
        // counter, so a zero says the infantry occupant never refused a curve on
        // either arm: the object arm skipped him by category, and he holds no
        // vehicle bit for the mask arm to find.
        assert_eq!(
            refusals, 0,
            "the infantry blocker refused a Drive selection {refusals} time(s) \
             (enemy_infantry={enemy_infantry}) — the infantry exclusion is not holding"
        );
        // And the tank genuinely went THROUGH him rather than politely around:
        // measured entry into the blocker's own cell at tick 53 in both the
        // friendly and the enemy case. Without this the zero above is
        // ambiguous — a mover that never approached also refuses nothing.
        assert!(
            entered_blocker_cell.is_some(),
            "the crusher never entered the infantry's cell (10,10) \
             (enemy_infantry={enemy_infantry}), so the exclusion was never exercised: {}",
            stacking_motion_state(&sim, tank)
        );

        // A blocked mover legitimately waits: BlockagePathDelay is 60 frames and
        // the scatter/repath ladder can run several spans. What it never does is
        // sit still for the whole run.
        assert!(
            stall < 300,
            "crusher sat still for {stall} consecutive ticks while still ordered \
             (enemy_infantry={enemy_infantry}) — this is the permanent-refusal freeze"
        );
        assert!(
            arrived_at.is_some(),
            "crusher never reached (16,10) with enemy_infantry={enemy_infantry}: {}",
            stacking_motion_state(&sim, tank)
        );
        // `sim.sound_events` accumulates until the app drains it, so the whole
        // run is visible here.
        let unit_lost_lines = sim
            .sound_events
            .iter()
            .filter(|event| matches!(event, SimSoundEvent::UnitLost { .. }))
            .count();
        assert_eq!(
            unit_lost_lines, 0,
            "a crush kill must not announce \"Unit lost\" (enemy_infantry={enemy_infantry})"
        );
    }
}

/// D2 GUARD — a turning curve whose ENDPOINT is occupied must still make
/// progress.
///
/// gamemd asks `Can_Enter_Cell` once per selection, about one cell
/// (0x004B34C0), and dispatches on the one code it gets back. A gate that asked
/// about a second cell — the curve's two-cells-out endpoint — and then reported
/// the FIRST cell to its dispatch would hand the dispatch a cell that is clear,
/// which resolves to "not blocked", resets the timers and returns without
/// stepping: a bit-identical tick, forever.
#[test]
fn turning_mover_with_an_occupied_endpoint_still_makes_progress() {
    let (mut sim, rules, grid) = stacking_world(24);
    let heights = empty_heights();

    // Mover at (10,10) ordered north-east: the curve's head node is (10,9) and
    // its endpoint two cells out is (11,9). Park a friendly on the endpoint and
    // leave the head node clear.
    let parked = sim
        .spawn_object("MTNK", "Americans", 11, 9, 64, &rules, &heights)
        .expect("parked tank spawns");
    let mover = sim
        .spawn_object("MTNK", "Americans", 10, 10, 64, &rules, &heights)
        .expect("mover spawns");

    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Move {
            entity_id: mover,
            target_rx: 10,
            target_ry: 4,
            queue: false,
            group_id: None,
        },
    );
    let _ = sim.advance_tick(&[cmd], Some(&rules), &heights, Some(&grid), None, 100);

    let mut series: Vec<(bool, (u16, u16))> = Vec::new();
    let mut arrived_at: Option<u64> = None;
    for tick in 0..600u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        let Some(e) = sim.substrate.entities.get(mover) else {
            break;
        };
        let cell = (e.position.rx, e.position.ry);
        series.push((e.movement_target.is_some(), cell));
        if cell == (10, 4) && arrived_at.is_none() {
            arrived_at = Some(tick);
        }
    }

    let stall = longest_stationary_run_while_ordered(&series);
    println!(
        "--- turning_mover_with_an_occupied_endpoint_still_makes_progress ---\n    \
         mover: {}\n    parked: {}\n    longest ordered-but-stationary run = {stall}; \
         arrived_at = {arrived_at:?}",
        stacking_motion_state(&sim, mover),
        stacking_motion_state(&sim, parked),
    );

    assert!(
        stall < 300,
        "mover sat still for {stall} consecutive ticks while still ordered — \
         the endpoint refusal is not resolvable by its own dispatch"
    );
    assert!(
        arrived_at.is_some(),
        "mover never reached (10,4): {}",
        stacking_motion_state(&sim, mover)
    );
    let m = sim.substrate.entities.get(mover).expect("mover alive");
    let p = sim.substrate.entities.get(parked).expect("parked alive");
    assert_ne!(
        (m.position.rx, m.position.ry),
        (p.position.rx, p.position.ry),
        "two ground vehicles must never rest on the same cell"
    );
}

/// B1 GUARD — a parked friendly vehicle standing ON the mover's route must be
/// told to move, not merely repathed around.
///
/// Code 6 is the one `Can_Enter_Cell` answer the Drive selection gate can
/// produce that does NOT share gamemd's entry at 0x004B3607. `CMP EDX,0x6 /
/// JNZ 0x004B3944` at 0x004B36F4 splits it into its own arm at 0x004B36FD, and
/// that arm reaches `CellClass__Scatter_Objects @ 0x00481670` — call site
/// 0x004B393A, via 0x004B38B3 — before falling into the shared entry through
/// `JMP 0x004B3607`. Codes 2 and 5 reach that shared entry directly and no
/// `Scatter_Objects` call sits anywhere between it and its `Find_Path` tail. A
/// gate that sent all three to one dispatch left the parked blocker parked.
///
/// The blocker is parked AFTER the order is issued and directly on the path A*
/// already returned. That is the ordinary case — a group member that finishes
/// its own move while a peer is still routed through the cell it stopped on —
/// and it is the only way to get a stationary vehicle onto the route at all: on
/// open ground A* simply steers around a code-6 cell, and an occupied goal cell
/// makes it return a path that stops one short without ever consulting the gate.
///
/// SEEN TO FAIL before being trusted (2026-08-05). With the code-6 routing
/// removed — every refusal taking the shared-entry dispatch, which is what this
/// change shipped before this fixture existed — the blocker never leaves (12,8).
/// Excluding code 6 from the object arm instead does NOT help: a stationary
/// vehicle always holds its own occupation bit, so the mask arm refuses the same
/// blocker one line later. The dispatch is what has to change.
#[test]
fn parked_friendly_on_the_route_is_scattered_out_of_the_way() {
    let (mut sim, rules, grid) = stacking_world(24);
    let heights = empty_heights();

    const PARKED_AT: (u16, u16) = (12, 8);
    const DESTINATION: (u16, u16) = (18, 8);

    let mover = sim
        .spawn_object("MTNK", "Americans", 6, 8, 64, &rules, &heights)
        .expect("mover spawns");

    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Move {
            entity_id: mover,
            target_rx: DESTINATION.0,
            target_ry: DESTINATION.1,
            queue: false,
            group_id: None,
        },
    );
    let first = sim.advance_tick(&[cmd], Some(&rules), &heights, Some(&grid), None, 100);
    let mut refusals = first.movement.selection_admission_refusals;
    // The Move dispatches at this frame's EventClass tail; the next frame's
    // first Process requests the route (Unit741970 accepts without one).
    let second = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
    refusals += second.movement.selection_admission_refusals;

    // The route A* actually returned, before anything is parked on it. The
    // fixture is only meaningful if the blocker cell is on it.
    let route: Vec<(u16, u16)> = sim
        .substrate
        .entities
        .get(mover)
        .and_then(|m| m.movement_target.as_ref())
        .map(|t| t.path.clone())
        .unwrap_or_default();
    assert!(
        route.contains(&PARKED_AT),
        "fixture precondition: {PARKED_AT:?} must lie on the mover's route {route:?}"
    );

    let parked = sim
        .spawn_object(
            "MTNK",
            "Americans",
            PARKED_AT.0,
            PARKED_AT.1,
            64,
            &rules,
            &heights,
        )
        .expect("parked tank spawns");

    let mut series: Vec<(bool, (u16, u16))> = Vec::new();
    let mut blocker_left_at: Option<u64> = None;
    let mut arrived_at: Option<u64> = None;
    for tick in 0..400u64 {
        let result = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        refusals += result.movement.selection_admission_refusals;
        if let Some(b) = sim.substrate.entities.get(parked)
            && (b.position.rx, b.position.ry) != PARKED_AT
            && blocker_left_at.is_none()
        {
            blocker_left_at = Some(tick);
        }
        let Some(m) = sim.substrate.entities.get(mover) else {
            break;
        };
        let cell = (m.position.rx, m.position.ry);
        series.push((m.movement_target.is_some(), cell));
        if cell == DESTINATION && arrived_at.is_none() {
            arrived_at = Some(tick);
        }
    }

    let stall = longest_stationary_run_while_ordered(&series);
    println!(
        "--- parked_friendly_on_the_route_is_scattered_out_of_the_way ---
             route: {route:?}
    mover: {}
    parked: {}
             selection refusals = {refusals}; blocker left {PARKED_AT:?} at {blocker_left_at:?};          arrived_at = {arrived_at:?}; longest ordered-but-stationary run = {stall}",
        stacking_motion_state(&sim, mover),
        stacking_motion_state(&sim, parked),
    );

    // The lane fired at all. `selection_admission_refusals` has exactly one
    // producer — the gate — so without this the two assertions below could both
    // pass on a build where the gate never ran, which is how a silent upstream
    // change would leave every new fixture in this file green.
    assert!(
        refusals > 0,
        "the Drive selection gate never refused anything, so this fixture proves          nothing about its dispatch"
    );
    assert!(
        blocker_left_at.is_some(),
        "the parked friendly never left {PARKED_AT:?}, so nothing ever reached the          code-6 scatter: {}",
        stacking_motion_state(&sim, parked)
    );
    assert!(
        arrived_at.is_some(),
        "the mover never reached its ordered destination {DESTINATION:?}: {}",
        stacking_motion_state(&sim, mover)
    );
}

/// Clear square map with PathGrid + terrain costs + zone grid, so both the
/// group-destination distributor and the runtime cell-entry checks are live.
fn stacking_world(size: u16) -> (Simulation, RuleSet, PathGrid) {
    use crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids;
    use crate::sim::pathfinding::zone_map::ZoneGrid;

    let rules = stacking_repro_rules();
    let mut sim = Simulation::new();
    // Live maps have normalized playfield authority before Techno unlimbo.
    // This deliberately broad diamond keeps the stacking fixture focused on
    // movement admission while still exercising production membership state.
    sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 0,
        off_fc: -256,
        off_100: -256,
        off_104: 512,
        off_108: 512,
    });
    let terrain = gsi_04_10_clear_terrain(size, size);
    sim.terrain_costs = build_canonical_terrain_cost_grids(&terrain);
    let grid = PathGrid::from_resolved_terrain(&terrain);
    sim.zone_grid = Some(ZoneGrid::build(&grid, &sim.terrain_costs, size, size));
    sim.resolved_terrain = Some(terrain);
    (sim, rules, grid)
}

/// `stacking_world` with production-shaped native zone topology and Map
/// Size, so the Find_Path owner (precheck, target answer, FNPC redirect)
/// serves the first Drive request as it does in a loaded map.
fn stacking_world_native(size: u16) -> (Simulation, RuleSet, PathGrid) {
    use crate::sim::pathfinding::zone_map::ZoneGrid;
    let (mut sim, rules, grid) = stacking_world(size);
    let bounds = crate::sim::cell_rect::PlayfieldBounds {
        base: i32::from(size),
        ..sim.playfield_bounds.unwrap()
    };
    sim.playfield_bounds = Some(bounds);
    sim.playfield_size_height = Some(i32::from(size));
    sim.zone_grid = Some(ZoneGrid::build_with_native_map_context(
        &grid,
        &sim.terrain_costs,
        sim.resolved_terrain.as_ref().unwrap(),
        &[],
        Some((i32::from(size), i32::from(size))),
        Some(bounds),
    ));
    assert!(sim.zone_grid.as_ref().unwrap().has_native_topology());
    (sim, rules, grid)
}

fn stacking_cells(sim: &Simulation, ids: &[u64]) -> Vec<(u64, u16, u16)> {
    ids.iter()
        .filter_map(|&id| {
            sim.substrate
                .entities
                .get(id)
                .map(|e| (id, e.position.rx, e.position.ry))
        })
        .collect()
}

fn stacking_duplicates(cells: &[(u64, u16, u16)]) -> BTreeMap<(u16, u16), Vec<u64>> {
    let mut by_cell: BTreeMap<(u16, u16), Vec<u64>> = BTreeMap::new();
    for &(id, rx, ry) in cells {
        by_cell.entry((rx, ry)).or_default().push(id);
    }
    by_cell.retain(|_, ids| ids.len() > 1);
    by_cell
}

/// (d) raw grid state for one cell: object list, the owner-aware vehicle
/// occupation plane, and the destructive raw CellClass ground byte.
fn stacking_cell_state(sim: &Simulation, rx: u16, ry: u16) -> String {
    let list: Vec<(u64, MovementLayer, Option<u8>, bool)> = sim
        .substrate
        .occupancy
        .get(rx, ry)
        .map(|o| {
            o.occupants
                .iter()
                .map(|c| (c.entity_id, c.layer, c.sub_cell, c.is_building))
                .collect()
        })
        .unwrap_or_default();
    format!(
        "({rx},{ry}) object_list={list:?} vehicle_bits=0x{:02X} raw_ground=0x{:02X}",
        sim.substrate
            .cell_occupation
            .vehicle_bits(rx, ry, MovementLayer::Ground),
        sim.substrate.raw_cell_occupation.ground_bits(rx, ry),
    )
}

/// (e) is this vehicle still trying to move?
fn stacking_motion_state(sim: &Simulation, id: u64) -> String {
    let Some(e) = sim.substrate.entities.get(id) else {
        return "<gone>".to_string();
    };
    let adapter = match e.movement_target.as_ref() {
        None => format!(
            "id={id} at ({},{}) sub=({},{}) movement_target=None",
            e.position.rx, e.position.ry, e.position.sub_x, e.position.sub_y
        ),
        Some(mt) => format!(
            "id={id} at ({},{}) sub=({},{}) path_len={} next_index={} goal={:?}",
            e.position.rx,
            e.position.ry,
            e.position.sub_x,
            e.position.sub_y,
            mt.path.len(),
            mt.next_index,
            mt.path.last().copied()
        ),
    };
    format!(
        "{adapter} nav={:?} drive={:?} foot={:?} mission={:?} marked={} occupation={}",
        e.navigation.nav_com,
        e.drive_locomotion,
        e.foot_speed,
        e.mission.current(),
        e.lifecycle.cell_marked,
        e.foot_occupation_enabled
    )
}

/// MINIMAL CASE: one stopped vehicle sits on a cell; a second vehicle is
/// ordered by the real command path to move onto exactly that cell.
/// Retail: the mover must NOT be admitted onto the occupied cell.
/// World lepton position of a mover. 256 leptons per cell — the verified
/// leptons-per-cell constant the whole coordinate frame is built on.
fn stacking_lepton_pos(sim: &Simulation, id: u64) -> Option<(i64, i64)> {
    sim.substrate.entities.get(id).map(|e| {
        (
            i64::from(e.position.rx) * 256 + i64::from(e.position.sub_x.to_num::<i32>()),
            i64::from(e.position.ry) * 256 + i64::from(e.position.sub_y.to_num::<i32>()),
        )
    })
}

/// Straight-line hull separation between two movers, in leptons. This is what
/// the renderer draws; the cell index is not.
fn stacking_gap(sim: &Simulation, a: u64, b: u64) -> Option<i64> {
    let (ax, ay) = stacking_lepton_pos(sim, a)?;
    let (bx, by) = stacking_lepton_pos(sim, b)?;
    let dx = ax - bx;
    let dy = ay - by;
    Some((((dx * dx + dy * dy) as f64).sqrt()) as i64)
}

/// Closest approach between any two of `ids` on this tick.
fn stacking_min_gap(sim: &Simulation, ids: &[u64]) -> Option<(u64, u64, i64)> {
    let mut worst: Option<(u64, u64, i64)> = None;
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            let Some(gap) = stacking_gap(sim, ids[i], ids[j]) else {
                continue;
            };
            if worst.is_none_or(|(_, _, best)| gap < best) {
                worst = Some((ids[i], ids[j], gap));
            }
        }
    }
    worst
}

/// Closest approach between two movers that are on the SAME cell this tick.
///
/// This is the regime the derived bound actually covers: it bounds the instant
/// gamemd admits a mover into a cell another mover still occupies. Convergence
/// between two movers in different cells is ordinary traffic and is not what the
/// derivation speaks to.
fn stacking_min_gap_within_shared_cell(sim: &Simulation, ids: &[u64]) -> Option<(u64, u64, i64)> {
    let mut worst: Option<(u64, u64, i64)> = None;
    for (_, members) in stacking_duplicates(&stacking_cells(sim, ids)) {
        for i in 0..members.len() {
            for j in (i + 1)..members.len() {
                let Some(gap) = stacking_gap(sim, members[i], members[j]) else {
                    continue;
                };
                if worst.is_none_or(|(_, _, best)| gap < best) {
                    worst = Some((members[i], members[j], gap));
                }
            }
        }
    }
    worst
}

/// Half a cell. Two MTNK hulls this close read on screen as one stacked sprite,
/// which is what the player reported seeing.
const VISIBLE_OVERLAP_LEPTONS: i64 = 128;

/// The player's measured symptoms, used directly as the acceptance bound.
///
/// Verbatim report: "tanks when moved in a group stack on top of each other."
/// Measured: **38 leptons** between a head-on pair, and **29 leptons held for 31
/// consecutive ticks** on a group move. 256 leptons is one cell.
///
/// Both numbers matter and neither substitutes for the other. A minimum taken
/// over a run and then compared to a floor cannot see the second report at all:
/// two hulls parked 130 leptons apart for 31 straight ticks are visually
/// indistinguishable from the complaint and clear every floor above. So the
/// duration of close approach is measured too, and both are asserted.
const REPORTED_HEADON_OVERLAP_LEPTONS: i64 = 38;
const REPORTED_SUSTAINED_OVERLAP_TICKS: usize = 31;

/// The bound the duration assertion actually uses — deliberately NOT the
/// player's 31.
///
/// 31 was the bound until 2026-08-05 and it had never fired, because it sits
/// ABOVE the worst value ever measured against it. The known-bad intermediate
/// build recorded on [`StackingWatch`] produced a **28**-tick run inside half a
/// cell, and 28 < 31, so that build PASSED this assertion; only the sibling
/// 38-lepton floor caught it. A regression that parked two hulls 45 leptons
/// apart for 28 straight ticks would have failed nothing.
///
/// Derived from measurement instead. On the passing build all three fixtures
/// that assert this report `longest run under 128 leptons = 0` — no pair ever
/// comes within half a cell for even one tick, the tightest approach anywhere
/// being 180 leptons on `group_move_short_range` and 181 on
/// `group_move_eight_to_one_cell` and `column_of_four`. The bound is one
/// post-scatter wait span (`bump_crush::POST_SCATTER_WAIT_FRAMES` = 10), the
/// shortest window in which a legitimate pass-by resolves itself. That is 10
/// ticks of headroom over the measured 0 and it refuses 18 ticks before the
/// known-bad 28.
const SUSTAINED_OVERLAP_TICK_BOUND: usize =
    crate::sim::movement::bump_crush::POST_SCATTER_WAIT_FRAMES as usize;

/// Per-tick separation record for a set of movers.
///
/// Deliberately keeps the whole series rather than folding straight to a
/// minimum: the reported defect is a *sustained* overlap, and a fold to one
/// number throws away exactly the axis that distinguishes it from an ordinary
/// pass-by.
///
/// SEEN TO FAIL, twice, before being trusted (2026-08-05):
///
/// * With the cell-admission gate reverted entirely,
///   `repro_two_moving_vehicles_pass_through_each_other` reports "head-on pair
///   closed to 36 leptons (0.14 cells) at tick 68" and
///   `column_of_vehicles_all_arrive_without_stacking` reports two hulls 190
///   leptons apart while sharing a cell, under the 239-lepton derived bound.
/// * With an intermediate build of the gate that dropped a refused mover's own
///   cell claim, `repro_group_move_of_eight_vehicles_to_one_cell` reported
///   "vehicles 3 and 8 closed to 29 leptons (0.11 cells) at tick 341" with a
///   28-consecutive-tick run inside half a cell — the player's reported numbers
///   almost exactly, on a pair in ADJACENT cells. The predecessor of this
///   struct passed that build: its bound sat inside an `if let Some(..)` on a
///   same-cell measure that the change itself drove to `None` everywhere, and
///   its all-pairs helper was never called.
#[derive(Default)]
struct StackingWatch {
    /// Closest approach between ANY two movers: (a, b, leptons, tick).
    closest: Option<(u64, u64, i64, u64)>,
    /// Closest approach between two movers that share a cell on that tick.
    closest_in_cell: Option<(u64, u64, i64, u64)>,
    /// Per-tick all-pairs minimum, kept in full.
    series: Vec<(u64, i64)>,
    /// Longest consecutive run of ticks whose all-pairs minimum was inside
    /// [`VISIBLE_OVERLAP_LEPTONS`], and where that run ended.
    longest_close_run: usize,
    longest_close_run_end: u64,
    open_close_run: usize,
    /// Ticks on which two movers occupied one cell.
    shared_cell_ticks: Vec<u64>,
}

impl StackingWatch {
    fn sample(&mut self, sim: &Simulation, ids: &[u64], tick: u64) {
        if let Some((a, b, gap)) = stacking_min_gap(sim, ids) {
            self.series.push((tick, gap));
            if self.closest.is_none_or(|(_, _, best, _)| gap < best) {
                self.closest = Some((a, b, gap, tick));
            }
            if gap < VISIBLE_OVERLAP_LEPTONS {
                self.open_close_run += 1;
                if self.open_close_run > self.longest_close_run {
                    self.longest_close_run = self.open_close_run;
                    self.longest_close_run_end = tick;
                }
            } else {
                self.open_close_run = 0;
            }
        }
        if let Some((a, b, gap)) = stacking_min_gap_within_shared_cell(sim, ids)
            && self
                .closest_in_cell
                .is_none_or(|(_, _, best, _)| gap < best)
        {
            self.closest_in_cell = Some((a, b, gap, tick));
        }
        if !stacking_duplicates(&stacking_cells(sim, ids)).is_empty() {
            self.shared_cell_ticks.push(tick);
        }
    }

    fn report(&self, label: &str) {
        println!(
            "[{label}] samples={} closest(any pair)={:?} closest(shared cell)={:?} \
             longest run under {VISIBLE_OVERLAP_LEPTONS} leptons = {} tick(s) ending {} \
             shared-cell ticks = {}",
            self.series.len(),
            self.closest,
            self.closest_in_cell,
            self.longest_close_run,
            self.longest_close_run_end,
            self.shared_cell_ticks.len(),
        );
        let tightest: Vec<String> = {
            let mut s = self.series.clone();
            s.sort_by_key(|&(_, gap)| gap);
            s.iter()
                .take(12)
                .map(|(t, g)| format!("t{t}:{g}"))
                .collect()
        };
        println!("[{label}] tightest sampled ticks: {}", tightest.join(" "));
    }

    /// The acceptance surface. Unconditional: a fixture that sampled nothing
    /// fails here rather than passing vacuously.
    fn assert_no_reported_stacking(&self, label: &str) {
        let (a, b, gap, tick) = self
            .closest
            .unwrap_or_else(|| panic!("[{label}] measured no pair separation at all"));
        assert!(
            gap > REPORTED_HEADON_OVERLAP_LEPTONS,
            "[{label}] vehicles {a} and {b} closed to {gap} leptons ({:.2} cells) at tick {tick}; \
             the player's head-on report was {REPORTED_HEADON_OVERLAP_LEPTONS}",
            gap as f64 / 256.0
        );
        assert!(
            self.longest_close_run < SUSTAINED_OVERLAP_TICK_BOUND,
            "[{label}] some pair stayed inside {VISIBLE_OVERLAP_LEPTONS} leptons for {} \
             consecutive ticks (run ends at tick {}); the bound is \
             {SUSTAINED_OVERLAP_TICK_BOUND} and the passing build measures 0. For scale, \
             the player's group-move report was {REPORTED_SUSTAINED_OVERLAP_TICKS} \
             consecutive ticks and the known-bad intermediate build produced 28",
            self.longest_close_run,
            self.longest_close_run_end,
        );
        // The transit bound is derived for ONE instant — a mover admitted into a
        // cell another mover still occupies — so it is asserted only on that
        // regime. It is a refinement of the two bounds above, never a substitute.
        if let Some((ca, cb, cgap, ctick)) = self.closest_in_cell {
            let bound = derived_min_transit_separation_leptons();
            assert!(
                cgap >= bound,
                "[{label}] vehicles {ca} and {cb} shared a cell only {cgap} leptons \
                 ({:.2} cells) apart at tick {ctick}; retail's own admission rule cannot \
                 produce anything below {bound}",
                cgap as f64 / 256.0
            );
        }
    }
}

/// The smallest hull separation retail's own admission rule can produce between
/// two ground vehicles — DERIVED from the shipped curve tables, not chosen.
///
/// gamemd lets a follower into the cell a leader is transiting at the leader's
/// FIRST PAID TRACK POINT: that is where the movement body clears the leader's
/// occupation bit and lowers its cell-occupation-enabled byte. At that instant
/// the leader has advanced from its cell centre by exactly the first inter-point
/// step of whichever curve it selected, and the follower is still a full cell
/// pitch away. So the closest the rule can legitimately put two hulls is one
/// cell minus the largest first step over every curve retail can select.
///
/// Anything tighter than this is not something retail's cell exclusion produces,
/// and is the regime the player reported (the head-on pair measured 38 leptons).
fn derived_min_transit_separation_leptons() -> i64 {
    use crate::sim::movement::drive_track::raw_track_points;
    let mut widest_first_step: i64 = 0;
    for raw_index in 1u8..16 {
        let points = raw_track_points(raw_index);
        if points.len() < 2 {
            continue;
        }
        let dx = i64::from(points[1].x) - i64::from(points[0].x);
        let dy = i64::from(points[1].y) - i64::from(points[0].y);
        let step = (((dx * dx + dy * dy) as f64).sqrt()) as i64;
        widest_first_step = widest_first_step.max(step);
    }
    256 - widest_first_step
}

#[test]
fn derived_transit_separation_bound_is_inside_one_cell() {
    let bound = derived_min_transit_separation_leptons();
    println!("derived minimum in-transit hull separation = {bound} leptons");
    // Must be a real bound: strictly inside one cell pitch (a leader that has
    // paid a point has moved), and far above the reported symptom's 38.
    assert!(
        bound > 38 && bound < 256,
        "derived bound {bound} is not a usable in-transit separation bound"
    );
}

#[test]
fn repro_second_vehicle_ordered_onto_an_occupied_cell() {
    // Unit741970 names the occupied cell unchanged; the first Process's
    // Find_Path answers code 6 there and, beyond CloseEnough, retargets to an
    // FNPC cell (0x4D3A92..0x4D3E0A). That owner needs native topology; the
    // compatibility zone grid keeps the legacy search, which drives into the
    // blocker's cell.
    let (mut sim, rules, grid) = stacking_world_native(24);
    let heights = empty_heights();

    let blocker = sim
        .spawn_object("MTNK", "Americans", 12, 8, 64, &rules, &heights)
        .expect("blocker spawns");
    let mover = sim
        .spawn_object("MTNK", "Americans", 6, 8, 64, &rules, &heights)
        .expect("mover spawns");

    let cmd = cmd_envelope(
        &sim,
        "Americans",
        1,
        Command::Move {
            entity_id: mover,
            target_rx: 12,
            target_ry: 8,
            queue: false,
            group_id: None,
        },
    );
    let _ = sim.advance_tick(&[cmd], Some(&rules), &heights, Some(&grid), None, 100);

    let mut shared_ticks: Vec<u64> = Vec::new();
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        let cells = stacking_cells(&sim, &[blocker, mover]);
        if !stacking_duplicates(&cells).is_empty() {
            shared_ticks.push(tick);
        }
    }

    println!("--- repro_second_vehicle_ordered_onto_an_occupied_cell ---");
    println!("blocker: {}", stacking_motion_state(&sim, blocker));
    println!("mover:   {}", stacking_motion_state(&sim, mover));
    println!("blocker cell: {}", stacking_cell_state(&sim, 12, 8));
    println!(
        "shared-cell ticks: count={} first={:?} last={:?}",
        shared_ticks.len(),
        shared_ticks.first(),
        shared_ticks.last()
    );

    let b = sim.substrate.entities.get(blocker).expect("blocker alive");
    let m = sim.substrate.entities.get(mover).expect("mover alive");
    assert_ne!(
        (b.position.rx, b.position.ry),
        (m.position.rx, m.position.ry),
        "two ground vehicles must never rest on the same cell"
    );
    // RECORDED RESIDUAL: this is STRICTER than retail. gamemd genuinely lets two
    // vehicles share one `CellClass` in transit — the leader clears its
    // occupation bit at its first paid track point but stays linked in that
    // cell's object list until the crossing, and the derived transit-separation
    // bound exists precisely because the follower can be admitted in that
    // window. Asserting emptiness here is a Rust regression ratchet on a fixture
    // that happens to produce none, not a parity claim, and it must not be read
    // as one.
    assert!(
        shared_ticks.is_empty(),
        "two ground vehicles shared a cell during transit on ticks {shared_ticks:?}"
    );
}

/// FAITHFUL CASE: eight vehicles selected as a group, one Move order each to
/// a single destination cell, issued in one batch exactly as a group order is.
#[test]
fn repro_group_move_of_eight_vehicles_to_one_cell() {
    let (mut sim, rules, grid) = stacking_world(48);
    let heights = empty_heights();

    let start_cells = [
        (6u16, 6u16),
        (7, 6),
        (8, 6),
        (9, 6),
        (6, 7),
        (7, 7),
        (8, 7),
        (9, 7),
    ];
    let ids: Vec<u64> = start_cells
        .iter()
        .map(|&(cx, cy)| {
            sim.spawn_object("MTNK", "Americans", cx, cy, 64, &rules, &heights)
                .expect("tank spawns")
        })
        .collect();

    let target = (30u16, 30u16);
    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .map(|&id| {
            cmd_envelope(
                &sim,
                "Americans",
                1,
                Command::Move {
                    entity_id: id,
                    target_rx: target.0,
                    target_ry: target.1,
                    queue: false,
                    group_id: None,
                },
            )
        })
        .collect();

    // (a) What the group-destination distributor rewrites each command to.
    // Same &self read the tick performs, run before any tick mutates state.
    let mut staged = commands.clone();
    sim.adjust_staged_megamission_destinations(&mut staged, Some(&grid));
    let assigned: Vec<(u64, (u16, u16))> = staged
        .iter()
        .filter_map(|c| match &c.payload {
            Command::Move {
                entity_id,
                target_rx,
                target_ry,
                ..
            } => Some((*entity_id, (*target_rx, *target_ry))),
            _ => None,
        })
        .collect();
    let mut assigned_counts: BTreeMap<(u16, u16), Vec<u64>> = BTreeMap::new();
    for &(id, cell) in &assigned {
        assigned_counts.entry(cell).or_default().push(id);
    }

    println!("--- repro_group_move_of_eight_vehicles_to_one_cell ---");
    println!("(a) distributor assignments (entity -> destination):");
    for (id, cell) in &assigned {
        println!("    {id} -> {cell:?}");
    }
    println!(
        "(a) distinct destinations = {} of {}; collisions = {:?}",
        assigned_counts.len(),
        assigned.len(),
        assigned_counts
            .iter()
            .filter(|(_, v)| v.len() > 1)
            .collect::<Vec<_>>()
    );

    // Run the real path.
    let _ = sim.advance_tick(&commands, Some(&rules), &heights, Some(&grid), None, 100);

    // (c) per-tick cell sharing during transit, with (d) sampled AT the
    // sharing tick — the grid must be read while the two movers are still
    // co-located, not after they have moved on.
    let mut shared_by_tick: Vec<(u64, BTreeMap<(u16, u16), Vec<u64>>)> = Vec::new();
    let mut shared_snapshots: Vec<String> = Vec::new();
    let mut watch = StackingWatch::default();
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        watch.sample(&sim, &ids, tick);
        let dups = stacking_duplicates(&stacking_cells(&sim, &ids));
        if !dups.is_empty() {
            for (cell, members) in &dups {
                shared_snapshots.push(format!(
                    "tick {tick} shared {cell:?} by {members:?}\n        grid: {}\n        {}",
                    stacking_cell_state(&sim, cell.0, cell.1),
                    members
                        .iter()
                        .map(|&id| stacking_motion_state(&sim, id))
                        .collect::<Vec<_>>()
                        .join("\n        "),
                ));
            }
            shared_by_tick.push((tick, dups));
        }
    }

    // (b) final resting cells.
    let final_cells = stacking_cells(&sim, &ids);
    println!("(b) final positions / (e) still-moving state:");
    for (id, _, _) in &final_cells {
        println!("    {}", stacking_motion_state(&sim, *id));
    }
    let final_dups = stacking_duplicates(&final_cells);
    println!("(b) cells shared AT REST: {final_dups:?}");

    // (c) transient (one isolated tick) vs persistent (consecutive ticks).
    let mut runs: Vec<(u64, u64)> = Vec::new();
    for &(tick, _) in &shared_by_tick {
        match runs.last_mut() {
            Some(last) if last.1 + 1 == tick => last.1 = tick,
            _ => runs.push((tick, tick)),
        }
    }
    println!(
        "(c) ticks with a shared cell: {} across {} consecutive run(s); \
         longest run = {} tick(s)",
        shared_by_tick.len(),
        runs.len(),
        runs.iter().map(|(a, b)| b - a + 1).max().unwrap_or(0),
    );
    println!("(c) runs (first..last tick): {runs:?}");

    // (d) grid state sampled AT each sharing tick.
    println!("(d) grid + entity state at each sharing tick:");
    for snap in shared_snapshots.iter().take(8) {
        println!("    {snap}");
    }
    if shared_snapshots.len() > 8 {
        println!("    ... ({} more)", shared_snapshots.len() - 8);
    }

    // AT REST: cell identity. Retail-backed and kept — a stopped vehicle holds
    // its cell's occupation bit, so two of them cannot rest on one cell.
    assert!(
        final_dups.is_empty(),
        "ground vehicles must never rest on the same cell; shared: {final_dups:?}"
    );
    // IN TRANSIT: hull separation, not cell identity. gamemd releases a mover's
    // occupation bit at its first paid track point and relinks the cell object
    // list only at the crossing, so two vehicles genuinely share one CellClass
    // while one is leaving it. Cell distinctness is therefore NOT retail's
    // in-transit invariant; overlapping hulls are what it never produces, and
    // that is the player's reported symptom.
    watch.report("group_move_eight_to_one_cell");
    watch.assert_no_reported_stacking("group_move_eight_to_one_cell");
}

/// SHORT-RANGE GROUP CASE: the same eight vehicles, but the destination is
/// close enough that most of them arrive within a few ticks of each other,
/// maximising arrival contention. Also traces the full per-tick cell of every
/// member so co-travel (two tanks moving as one) is visible, not just sampled.
#[test]
fn repro_group_move_short_range_traces_every_tick() {
    let (mut sim, rules, grid) = stacking_world(48);
    let heights = empty_heights();

    let start_cells = [
        (10u16, 10u16),
        (11, 10),
        (12, 10),
        (13, 10),
        (10, 11),
        (11, 11),
        (12, 11),
        (13, 11),
    ];
    let ids: Vec<u64> = start_cells
        .iter()
        .map(|&(cx, cy)| {
            sim.spawn_object("MTNK", "Americans", cx, cy, 64, &rules, &heights)
                .expect("tank spawns")
        })
        .collect();

    let target = (16u16, 16u16);
    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .map(|&id| {
            cmd_envelope(
                &sim,
                "Americans",
                1,
                Command::Move {
                    entity_id: id,
                    target_rx: target.0,
                    target_ry: target.1,
                    queue: false,
                    group_id: None,
                },
            )
        })
        .collect();

    let mut staged = commands.clone();
    sim.adjust_staged_megamission_destinations(&mut staged, Some(&grid));
    println!("--- repro_group_move_short_range_traces_every_tick ---");
    println!("(a) distributor assignments:");
    for c in &staged {
        if let Command::Move {
            entity_id,
            target_rx,
            target_ry,
            ..
        } = &c.payload
        {
            println!("    {entity_id} -> ({target_rx},{target_ry})");
        }
    }

    let _ = sim.advance_tick(&commands, Some(&rules), &heights, Some(&grid), None, 100);

    let mut shared_by_tick: Vec<(u64, BTreeMap<(u16, u16), Vec<u64>>)> = Vec::new();
    let mut trace: Vec<String> = Vec::new();
    let mut watch = StackingWatch::default();
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        watch.sample(&sim, &ids, tick);
        let cells = stacking_cells(&sim, &ids);
        let dups = stacking_duplicates(&cells);
        if tick < 120 {
            trace.push(format!(
                "t{tick:>3} {}{}",
                cells
                    .iter()
                    .map(|(id, rx, ry)| format!("{id}:({rx},{ry})"))
                    .collect::<Vec<_>>()
                    .join(" "),
                if dups.is_empty() {
                    String::new()
                } else {
                    format!("   <<< SHARED {dups:?}")
                }
            ));
        }
        if !dups.is_empty() {
            for (cell, members) in &dups {
                trace.push(format!(
                    "t{tick:>3} SHARED {cell:?} by {members:?}\n        grid: {}\n        {}",
                    stacking_cell_state(&sim, cell.0, cell.1),
                    members
                        .iter()
                        .map(|&id| stacking_reservation_state(&sim, id))
                        .collect::<Vec<_>>()
                        .join("\n        "),
                ));
            }
            shared_by_tick.push((tick, dups));
        }
    }

    for line in &trace {
        println!("{line}");
    }

    let final_cells = stacking_cells(&sim, &ids);
    for (id, _, _) in &final_cells {
        println!("    {}", stacking_motion_state(&sim, *id));
    }
    let final_dups = stacking_duplicates(&final_cells);
    println!("cells shared AT REST: {final_dups:?}");
    println!("ticks with a shared cell: {}", shared_by_tick.len());

    // AT REST: cell identity. Retail-backed and kept — a stopped vehicle holds
    // its cell's occupation bit, so two of them cannot rest on one cell.
    assert!(
        final_dups.is_empty(),
        "ground vehicles must never rest on the same cell; shared: {final_dups:?}"
    );
    // IN TRANSIT: hull separation, not cell identity. gamemd releases a mover's
    // occupation bit at its first paid track point and relinks the cell object
    // list only at the crossing, so two vehicles genuinely share one CellClass
    // while one is leaving it. Cell distinctness is therefore NOT retail's
    // in-transit invariant; overlapping hulls are what it never produces, and
    // that is the player's reported symptom.
    watch.report("group_move_short_range");
    watch.assert_no_reported_stacking("group_move_short_range");
}

/// What the one-per-cell predicate WOULD say for `mover` entering `cell`,
/// evaluated read-only from live sim state. Used to show that the predicate
/// exists and answers correctly while the runtime step never consults it.
fn stacking_cell_entry_verdict(sim: &Simulation, mover: u64, rx: u16, ry: u16) -> String {
    use crate::sim::movement::bump_crush::CrushCapability;
    use crate::sim::pathfinding::cell_entry::{
        CanEnterLayerContext, check_terrain_with_layers, classify_occupied_cell_with_layers,
    };
    let Some(e) = sim.substrate.entities.get(mover) else {
        return "<gone>".to_string();
    };
    let layers = CanEnterLayerContext::single(MovementLayer::Ground);
    let phase1 = check_terrain_with_layers(
        (rx, ry),
        layers,
        e.category,
        None,
        None,
        &sim.substrate.occupancy,
    );
    let owner = sim.interner.resolve(e.owner).to_string();
    let phase2 = classify_occupied_cell_with_layers(
        (rx, ry),
        layers,
        mover,
        CrushCapability::new(false, false),
        &owner,
        e.locomotor
            .as_ref()
            .map(|l| l.kind)
            .unwrap_or(crate::rules::locomotor_type::LocomotorKind::Drive),
        false,
        &sim.substrate.occupancy,
        &sim.substrate.entities,
        &sim.house_alliances,
        &sim.interner,
    );
    format!(
        "phase1={phase1:?} phase2={phase2:?} yr_code={}",
        phase2.yr_code()
    )
}

/// MINIMAL TWO-MOVER CASE â€” no group order at all.
///
/// Two vehicles are given INDEPENDENT Move commands with DIFFERENT targets,
/// so each is a run of one and the group-destination distributor never runs.
/// They are driven head-on through each other. Retail keeps ground vehicles
/// strictly one-per-cell whether they are moving or stopped.
#[test]
fn repro_two_moving_vehicles_pass_through_each_other() {
    let (mut sim, rules, grid) = stacking_world(24);
    let heights = empty_heights();

    let west = sim
        .spawn_object("MTNK", "Americans", 5, 10, 64, &rules, &heights)
        .expect("west tank spawns");
    let east = sim
        .spawn_object("MTNK", "Americans", 15, 10, 192, &rules, &heights)
        .expect("east tank spawns");

    let commands = vec![
        cmd_envelope(
            &sim,
            "Americans",
            1,
            Command::Move {
                entity_id: west,
                target_rx: 16,
                target_ry: 10,
                queue: false,
                group_id: None,
            },
        ),
        cmd_envelope(
            &sim,
            "Americans",
            1,
            Command::Move {
                entity_id: east,
                target_rx: 4,
                target_ry: 10,
                queue: false,
                group_id: None,
            },
        ),
    ];
    // Different targets => different formation keys => runs of length 1 =>
    // the group-destination distributor cannot touch either command.
    let mut staged = commands.clone();
    sim.adjust_staged_megamission_destinations(&mut staged, Some(&grid));
    println!("--- repro_two_moving_vehicles_pass_through_each_other ---");
    for c in &staged {
        if let Command::Move {
            entity_id,
            target_rx,
            target_ry,
            ..
        } = &c.payload
        {
            println!("    post-distributor: {entity_id} -> ({target_rx},{target_ry})");
        }
    }

    let _ = sim.advance_tick(&commands, Some(&rules), &heights, Some(&grid), None, 100);

    let ids = [west, east];
    let mut shared_ticks: Vec<u64> = Vec::new();
    let mut snapshots: Vec<String> = Vec::new();
    let mut closest_approach: Option<(i64, u64)> = None;
    let mut shared_cell_approach: Option<(u64, u64, i64, u64)> = None;
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        if let Some(gap) = stacking_gap(&sim, west, east)
            && closest_approach.is_none_or(|(best, _)| gap < best)
        {
            closest_approach = Some((gap, tick));
        }
        if let Some((a, b, gap)) = stacking_min_gap_within_shared_cell(&sim, &ids)
            && shared_cell_approach.is_none_or(|(_, _, best, _)| gap < best)
        {
            shared_cell_approach = Some((a, b, gap, tick));
        }
        let dups = stacking_duplicates(&stacking_cells(&sim, &ids));
        if !dups.is_empty() {
            shared_ticks.push(tick);
            for (cell, members) in &dups {
                snapshots.push(format!(
                    "tick {tick} cell {cell:?} members {members:?}\n        grid: {}\n        {}\n        \
                     cell-entry predicate for {}: {}",
                    stacking_cell_state(&sim, cell.0, cell.1),
                    members
                        .iter()
                        .map(|&id| stacking_motion_state(&sim, id))
                        .collect::<Vec<_>>()
                        .join("\n        "),
                    members[1],
                    stacking_cell_entry_verdict(&sim, members[1], cell.0, cell.1),
                ));
            }
        }
    }

    println!("west: {}", stacking_motion_state(&sim, west));
    println!("east: {}", stacking_motion_state(&sim, east));
    println!("shared ticks: {} -> {shared_ticks:?}", shared_ticks.len());
    for snap in snapshots.iter().take(10) {
        println!("    {snap}");
    }
    if snapshots.len() > 10 {
        println!("    ... ({} more)", snapshots.len() - 10);
    }

    // THE REPORTED SYMPTOM, MEASURED. This is the only fixture in the repo known
    // to have produced the player's overlap: before the cell-exclusion gate both
    // tanks occupied (10,10) for ten consecutive ticks and closed to 38 leptons
    // — an ~85% hull overlap. Cell distinctness alone places no lower bound on
    // separation, so it cannot detect a regression of that; the separation
    // assertions below can.
    //
    // TWO REGIMES, TWO BOUNDS — and they are not interchangeable.
    //
    // `derived_min_transit_separation_leptons` is derived for one instant: when
    // gamemd admits a mover into a cell another mover is still occupying. It
    // does NOT bound two movers in DIFFERENT cells, and gamemd's own crossing
    // rule refutes any attempt to make it: `Process_Drive_Track` derives the
    // cell from one absolute coordinate and crosses the moment that coordinate
    // crosses, so a hull sits a handful of leptons inside a cell immediately
    // after entering it. A vehicle resting at the centre of the cell it just
    // left is then a little over half a cell away, both legally in their own
    // cells. Applying the transit bound to every pair asserted something retail
    // does not honour, so the transit bound is asserted here on the regime it
    // was derived for, and the all-pair measurement keeps the half-cell
    // "two hulls on one spot" floor used by
    // `group_move_never_draws_two_hulls_on_one_spot` — which is what the
    // player's 38-lepton report was a violation of.
    //
    // FLOOR RE-DERIVED 2026-09-15 with the native ally-occupant arm in place.
    // `Process_Drive_Track` clears the mover's own raw occupation bit and
    // `Foot+0x6B6` on the first processed point while `+0x6B6` is still set
    // (`0x004B15D3 CALL [owner+0xC0]`, `0x004B1611 CALL [owner+0xF4]`,
    // `0x004B161A`; chained curves do not re-arm it), `UnitClass::Can_Enter_Cell` then skips
    // that in-transit ally on the object list unless its locomotor answers
    // `Can_Use_Track` (`0x0073FA30..FA7C`), and the head-on exit fires only for
    // an ally facing the mover inside `0x1FF` leptons and inside the mover's
    // octant (`0x0073F8D4..FA26`). So once the east tank has committed a
    // dodge curve out of its cell, nothing native keeps the west tank out of
    // that cell while the dodging hull is still leaving it. Measured closest
    // approach on this fixture with that contract: 116 leptons at tick 77,
    // in different cells. The earlier 128 floor was VERA's own half-cell
    // heuristic; the player's report was 38 leptons in ONE cell, which the
    // shared-cell transit bound below still refuses. This floor is a ratchet
    // on the measured value, not a native bound.
    //
    // RE-MEASURED 2026-09-23 with the same-call track-end continuation (a
    // track end runs Process_Movement and Process_Track(1) in that Process,
    // Drive 0x4B0583..0x4B0667). The west tank now selects its next head in
    // the Process that ends its track at (10,10), one frame before the former
    // next-visit reselection, and enters (11,10) while the east tank's dodge
    // curve is still leaving through that cell's south-west corner: closest
    // approach 108 leptons at tick 77, and the pair shares (11,10) for tick
    // 78 only, 119 leptons apart, below the derived transit bound. That bound
    // is derived for the admission instant, inside the frame, which this
    // fixture does not sample; whether native admits here too is open while
    // Drive admission runs through `classify_blocker` (ledger row I15).
    // Ratchets on the measured values, not native bounds; the bound is
    // printed.
    const VISIBLE_OVERLAP_LEPTONS: i64 = 108;
    const SHARED_CELL_FLOOR_LEPTONS: i64 = 119;
    const SHARED_CELL_TICKS: usize = 1;
    let bound = derived_min_transit_separation_leptons();
    let (gap, gap_tick) = closest_approach.expect("both movers sampled");
    println!(
        "closest approach (any pair): {gap} leptons ({:.2} cells) at tick {gap_tick}; \
         transit bound {bound}, visible-overlap floor {VISIBLE_OVERLAP_LEPTONS}; \
         closest approach while sharing a cell: {shared_cell_approach:?}",
        gap as f64 / 256.0
    );
    assert!(
        gap >= VISIBLE_OVERLAP_LEPTONS,
        "head-on pair closed to {gap} leptons ({:.2} cells) at tick {gap_tick} — hulls visibly overlap",
        gap as f64 / 256.0
    );
    if let Some((a, b, shared_gap, shared_tick)) = shared_cell_approach {
        assert!(
            shared_gap >= SHARED_CELL_FLOOR_LEPTONS,
            "vehicles {a} and {b} shared a cell only {shared_gap} leptons ({:.2} cells) at tick {shared_tick}",
            shared_gap as f64 / 256.0
        );
    }
    // RECORDED RESIDUAL: stricter than retail, for the reason written out at
    // `repro_second_vehicle_ordered_onto_an_occupied_cell`. Regression ratchet,
    // not a parity claim.
    assert!(
        shared_ticks.len() <= SHARED_CELL_TICKS,
        "two moving ground vehicles shared a cell on ticks {shared_ticks:?}"
    );
}

/// Head-to reservation + occupation-bit state for one vehicle.
fn stacking_reservation_state(sim: &Simulation, id: u64) -> String {
    let Some(e) = sim.substrate.entities.get(id) else {
        return format!("{id}:<gone>");
    };
    let Some(d) = e.drive_locomotion.as_ref() else {
        return format!("{id}:<no drive>");
    };
    format!(
        "{id}@({},{})sub({},{}) head_to={:?} cur_cleared={}",
        e.position.rx,
        e.position.ry,
        e.position.sub_x,
        e.position.sub_y,
        d.occupation_head_to.map(|f| (f.rx, f.ry)),
        !e.foot_occupation_enabled,
    )
}

/// Tick-by-tick trace of the head-on race, showing exactly when each mover
/// installs its head-to reservation on the contested cell and whether the
/// other mover's reservation was visible at that moment.
#[test]
fn repro_two_moving_vehicles_reservation_trace() {
    let (mut sim, rules, grid) = stacking_world(24);
    let heights = empty_heights();

    let west = sim
        .spawn_object("MTNK", "Americans", 5, 10, 64, &rules, &heights)
        .expect("west tank spawns");
    let east = sim
        .spawn_object("MTNK", "Americans", 15, 10, 192, &rules, &heights)
        .expect("east tank spawns");

    let commands = vec![
        cmd_envelope(
            &sim,
            "Americans",
            1,
            Command::Move {
                entity_id: west,
                target_rx: 16,
                target_ry: 10,
                queue: false,
                group_id: None,
            },
        ),
        cmd_envelope(
            &sim,
            "Americans",
            1,
            Command::Move {
                entity_id: east,
                target_rx: 4,
                target_ry: 10,
                queue: false,
                group_id: None,
            },
        ),
    ];
    let _ = sim.advance_tick(&commands, Some(&rules), &heights, Some(&grid), None, 100);

    println!("--- repro_two_moving_vehicles_reservation_trace ---");
    // The head-to mark is the only cell-exclusion mechanism a Drive curve
    // installs. If two movers can hold it on the same cell at the same tick,
    // the reservation is not exclusive and both will commit into that cell.
    let mut double_reservation_ticks: Vec<(u64, (u16, u16))> = Vec::new();
    for tick in 0..80u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        let heads: Vec<Option<(u16, u16)>> = [west, east]
            .iter()
            .map(|&id| {
                sim.substrate
                    .entities
                    .get(id)
                    .and_then(|e| e.drive_locomotion.as_ref())
                    .and_then(|d| d.occupation_head_to)
                    .map(|f| (f.rx, f.ry))
            })
            .collect();
        if let (Some(a), Some(b)) = (heads[0], heads[1])
            && a == b
        {
            double_reservation_ticks.push((tick, a));
        }
        if !(40..=75).contains(&tick) {
            continue;
        }
        let bits: Vec<String> = (9u16..=11)
            .map(|rx| {
                format!(
                    "({rx},10)=0x{:02X}/n{}",
                    sim.substrate
                        .cell_occupation
                        .vehicle_bits(rx, 10, MovementLayer::Ground),
                    sim.substrate
                        .occupancy
                        .count_on_layer(rx, 10, MovementLayer::Ground),
                )
            })
            .collect();
        println!(
            "t{tick:>3} | {} | {} | cells {}",
            stacking_reservation_state(&sim, west),
            stacking_reservation_state(&sim, east),
            bits.join(" "),
        );
    }

    println!("double-reserved ticks: {double_reservation_ticks:?}");
    // The double-reservation check only fires while BOTH movers hold a head-to
    // reservation, and a refusal sets that to None — so a permanently gridlocked
    // pair would satisfy it vacuously. Require real progress first: both movers
    // must have left their start cells, which a gridlocked pair never does.
    let west_moved = sim
        .substrate
        .entities
        .get(west)
        .is_some_and(|e| (e.position.rx, e.position.ry) != (5, 10));
    let east_moved = sim
        .substrate
        .entities
        .get(east)
        .is_some_and(|e| (e.position.rx, e.position.ry) != (15, 10));
    assert!(
        west_moved && east_moved,
        "the reservation check is vacuous unless both movers actually moved;          west_moved={west_moved} east_moved={east_moved} — {} | {}",
        stacking_motion_state(&sim, west),
        stacking_motion_state(&sim, east)
    );
    assert!(
        double_reservation_ticks.is_empty(),
        "two Drive movers held the head-to cell reservation on the SAME cell: \
         {double_reservation_ticks:?} — the mark installed by \
         select_fresh_drive_track_at_current_cell (movement_step.rs) is not \
         gated on CellOccupationGrid::occupied_by_other"
    );
}

/// DEADLOCK GUARD — head-on pair.
///
/// The cell-exclusion gate refuses a curve into a cell another vehicle has
/// already claimed. Two movers that each want the other's cell would freeze
/// under a bare refusal, so the refusal must land in gamemd's per-code
/// dispatch: a temporary claim waits and repaths at escalating urgency, and a
/// blocker that has come to rest gets scattered out of the way. Both tanks must
/// therefore still finish their orders.
#[test]
fn head_on_pair_resolves_without_deadlock() {
    let (mut sim, rules, grid) = stacking_world(24);
    let heights = empty_heights();

    let west = sim
        .spawn_object("MTNK", "Americans", 5, 10, 64, &rules, &heights)
        .expect("west tank spawns");
    let east = sim
        .spawn_object("MTNK", "Americans", 15, 10, 192, &rules, &heights)
        .expect("east tank spawns");

    let commands = vec![
        cmd_envelope(
            &sim,
            "Americans",
            1,
            Command::Move {
                entity_id: west,
                target_rx: 16,
                target_ry: 10,
                queue: false,
                group_id: None,
            },
        ),
        cmd_envelope(
            &sim,
            "Americans",
            1,
            Command::Move {
                entity_id: east,
                target_rx: 4,
                target_ry: 10,
                queue: false,
                group_id: None,
            },
        ),
    ];
    let _ = sim.advance_tick(&commands, Some(&rules), &heights, Some(&grid), None, 100);

    // Generous budget: 11 cells each at MTNK speed, plus whatever the block
    // dispatch costs in waits and repaths.
    let mut west_done_at: Option<u64> = None;
    let mut east_done_at: Option<u64> = None;
    for tick in 0..900u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        let w = sim.substrate.entities.get(west).expect("west alive");
        let e = sim.substrate.entities.get(east).expect("east alive");
        if west_done_at.is_none() && (w.position.rx, w.position.ry) == (16, 10) {
            west_done_at = Some(tick);
        }
        if east_done_at.is_none() && (e.position.rx, e.position.ry) == (4, 10) {
            east_done_at = Some(tick);
        }
        if west_done_at.is_some() && east_done_at.is_some() {
            break;
        }
    }

    println!("--- head_on_pair_resolves_without_deadlock ---");
    println!("west: {}", stacking_motion_state(&sim, west));
    println!("east: {}", stacking_motion_state(&sim, east));
    println!("west reached goal at tick {west_done_at:?}, east at {east_done_at:?}");

    assert!(
        west_done_at.is_some(),
        "west tank never reached (16,10): {}",
        stacking_motion_state(&sim, west)
    );
    assert!(
        east_done_at.is_some(),
        "east tank never reached (4,10): {}",
        stacking_motion_state(&sim, east)
    );
}

/// DEADLOCK GUARD — column.
///
/// Four vehicles queued nose-to-tail on one row, each with its own destination
/// further along that row. The trailing movers repeatedly select a curve into
/// the cell the mover ahead is heading for, so this is the case the gate is
/// asked about most often in ordinary play. Every member must arrive, and no
/// two may ever occupy one cell.
#[test]
fn column_of_vehicles_all_arrive_without_stacking() {
    let (mut sim, rules, grid) = stacking_world(32);
    let heights = empty_heights();

    let starts = [(5u16, 10u16), (6, 10), (7, 10), (8, 10)];
    let goals = [(18u16, 10u16), (19, 10), (20, 10), (21, 10)];
    let ids: Vec<u64> = starts
        .iter()
        .map(|&(cx, cy)| {
            sim.spawn_object("MTNK", "Americans", cx, cy, 64, &rules, &heights)
                .expect("tank spawns")
        })
        .collect();

    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .zip(goals.iter())
        .map(|(&id, &(gx, gy))| {
            cmd_envelope(
                &sim,
                "Americans",
                1,
                Command::Move {
                    entity_id: id,
                    target_rx: gx,
                    target_ry: gy,
                    queue: false,
                    group_id: None,
                },
            )
        })
        .collect();
    let _ = sim.advance_tick(&commands, Some(&rules), &heights, Some(&grid), None, 100);

    let mut watch = StackingWatch::default();
    for tick in 0..900u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        watch.sample(&sim, &ids, tick);
    }

    println!("--- column_of_vehicles_all_arrive_without_stacking ---");
    for &id in &ids {
        println!("    {}", stacking_motion_state(&sim, id));
    }

    let mut stalled: Vec<String> = Vec::new();
    for (&id, &goal) in ids.iter().zip(goals.iter()) {
        let at_goal = sim
            .substrate
            .entities
            .get(id)
            .is_some_and(|e| (e.position.rx, e.position.ry) == goal);
        if !at_goal {
            stalled.push(format!(
                "{} (wanted {goal:?})",
                stacking_motion_state(&sim, id)
            ));
        }
    }

    assert!(
        stalled.is_empty(),
        "column members never reached their destinations: {stalled:?}"
    );
    // Their destinations are distinct cells, so arriving proves the at-rest
    // half. In transit, measure hull separation rather than cell identity for
    // the reason written out at the group fixtures.
    watch.report("column_of_four");
    watch.assert_no_reported_stacking("column_of_four");
}

/// DIAGNOSTIC: per-tick reservation trace of the four-tank column, printing the
/// tick a cell first becomes shared together with the preceding window of
/// head-to reservations and occupation bits for every member.
#[test]
#[ignore = "diagnostic"]
fn diag_column_reservation_trace() {
    let (mut sim, rules, grid) = stacking_world(32);
    let heights = empty_heights();

    let starts = [(5u16, 10u16), (6, 10), (7, 10), (8, 10)];
    let goals = [(18u16, 10u16), (19, 10), (20, 10), (21, 10)];
    let ids: Vec<u64> = starts
        .iter()
        .map(|&(cx, cy)| {
            sim.spawn_object("MTNK", "Americans", cx, cy, 64, &rules, &heights)
                .expect("tank spawns")
        })
        .collect();
    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .zip(goals.iter())
        .map(|(&id, &(gx, gy))| {
            cmd_envelope(
                &sim,
                "Americans",
                1,
                Command::Move {
                    entity_id: id,
                    target_rx: gx,
                    target_ry: gy,
                    queue: false,
                    group_id: None,
                },
            )
        })
        .collect();
    let _ = sim.advance_tick(&commands, Some(&rules), &heights, Some(&grid), None, 100);

    for tick in 0..70u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        let dups = stacking_duplicates(&stacking_cells(&sim, &ids));
        let bits: Vec<String> = (4u16..=14)
            .map(|rx| {
                format!(
                    "{rx}:{:02X}",
                    sim.substrate
                        .cell_occupation
                        .vehicle_bits(rx, 10, MovementLayer::Ground)
                )
            })
            .collect();
        println!(
            "t{tick:>3} {} | row10 {} {}",
            ids.iter()
                .map(|&id| stacking_reservation_state(&sim, id))
                .collect::<Vec<_>>()
                .join("  "),
            bits.join(" "),
            if dups.is_empty() {
                String::new()
            } else {
                format!("<<< SHARED {dups:?}")
            }
        );
    }
}

/// How close two vehicles actually get, in leptons, over a group move.
///
/// The three cell-identity tests above assert that no two vehicles ever share
/// an `(rx, ry)`. That is stricter than the retail invariant: gamemd derives a
/// unit's cell from one absolute coordinate, releases its cell-occupation bit
/// on the first paid track point of a curve, and only relinks the cell object
/// list at the crossing itself — so a follower is admitted into the cell a
/// leader is leaving while the leader's body is still nominally in it. What
/// gamemd never produces is two hulls drawn on the same spot. This test
/// measures that directly: the closest approach between any two movers, in
/// leptons (256 per cell). The reported bug measured 38.
#[test]
fn group_move_never_draws_two_hulls_on_one_spot() {
    let (mut sim, rules, grid) = stacking_world(48);
    let heights = empty_heights();

    let start_cells = [
        (10u16, 10u16),
        (11, 10),
        (12, 10),
        (13, 10),
        (10, 11),
        (11, 11),
        (12, 11),
        (13, 11),
    ];
    let ids: Vec<u64> = start_cells
        .iter()
        .map(|&(cx, cy)| {
            sim.spawn_object("MTNK", "Americans", cx, cy, 64, &rules, &heights)
                .expect("tank spawns")
        })
        .collect();
    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .map(|&id| {
            cmd_envelope(
                &sim,
                "Americans",
                1,
                Command::Move {
                    entity_id: id,
                    target_rx: 16,
                    target_ry: 16,
                    queue: false,
                    group_id: None,
                },
            )
        })
        .collect();
    let _ = sim.advance_tick(&commands, Some(&rules), &heights, Some(&grid), None, 100);

    // Lepton world position of each mover, so the measurement is what the
    // renderer draws rather than the cell index.
    fn lepton_pos(sim: &Simulation, id: u64) -> Option<(i64, i64)> {
        sim.substrate.entities.get(id).map(|e| {
            (
                i64::from(e.position.rx) * 256 + i64::from(e.position.sub_x.to_num::<i32>()),
                i64::from(e.position.ry) * 256 + i64::from(e.position.sub_y.to_num::<i32>()),
            )
        })
    }

    let mut worst: Option<(u64, u64, u64, i64)> = None;
    for tick in 0..400u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        for i in 0..ids.len() {
            for j in (i + 1)..ids.len() {
                let (Some(a), Some(b)) = (lepton_pos(&sim, ids[i]), lepton_pos(&sim, ids[j]))
                else {
                    continue;
                };
                let dx = a.0 - b.0;
                let dy = a.1 - b.1;
                let d2 = dx * dx + dy * dy;
                let d = (d2 as f64).sqrt() as i64;
                if worst.is_none_or(|(_, _, _, best)| d < best) {
                    worst = Some((tick, ids[i], ids[j], d));
                }
            }
        }
    }

    let (tick, a, b, gap) = worst.expect("at least one pair sampled");
    println!(
        "--- group_move_never_draws_two_hulls_on_one_spot ---\n\
         closest approach: {gap} leptons ({:.2} cells) between {a} and {b} at tick {tick}",
        gap as f64 / 256.0
    );

    // Half a cell. Below this the two hulls visibly overlap, which is the
    // player-reported symptom; the reported bug produced 38.
    assert!(
        gap >= 128,
        "two vehicles closed to {gap} leptons ({:.2} cells) at tick {tick} \
         ({a} and {b}) — hulls visibly overlap",
        gap as f64 / 256.0
    );
}

/// DIAGNOSTIC: short-range group move, per-tick head-to reservations, so the
/// curve geometry at each shared tick is visible (two-node turning curve vs
/// one-node straight follow).
#[test]
#[ignore = "diagnostic"]
fn diag_short_range_group_reservation_trace() {
    let (mut sim, rules, grid) = stacking_world(48);
    let heights = empty_heights();
    let start_cells = [
        (10u16, 10u16),
        (11, 10),
        (12, 10),
        (13, 10),
        (10, 11),
        (11, 11),
        (12, 11),
        (13, 11),
    ];
    let ids: Vec<u64> = start_cells
        .iter()
        .map(|&(cx, cy)| {
            sim.spawn_object("MTNK", "Americans", cx, cy, 64, &rules, &heights)
                .expect("tank spawns")
        })
        .collect();
    let commands: Vec<CommandEnvelope> = ids
        .iter()
        .map(|&id| {
            cmd_envelope(
                &sim,
                "Americans",
                1,
                Command::Move {
                    entity_id: id,
                    target_rx: 16,
                    target_ry: 16,
                    queue: false,
                    group_id: None,
                },
            )
        })
        .collect();
    let _ = sim.advance_tick(&commands, Some(&rules), &heights, Some(&grid), None, 100);

    for tick in 0..110u64 {
        let _ = sim.advance_tick(&[], Some(&rules), &heights, Some(&grid), None, 100);
        let dups = stacking_duplicates(&stacking_cells(&sim, &ids));
        if dups.is_empty() && !(60..=70).contains(&tick) && !(93..=99).contains(&tick) {
            continue;
        }
        println!(
            "t{tick:>3} {} {}",
            ids.iter()
                .map(|&id| stacking_reservation_state(&sim, id))
                .collect::<Vec<_>>()
                .join(" | "),
            if dups.is_empty() {
                String::new()
            } else {
                format!("<<< SHARED {dups:?}")
            }
        );
    }
}

#[test]
fn rule_handles_resolve_at_init_and_stay_none_for_unresolved_fixtures() {
    let rules = RuleSet::from_ini(&IniFile::from_str("")).expect("empty rules fixture parses");

    // Init-path resolution pins the canonical warhead names.
    let mut init_sim = Simulation::new();
    init_sim.interner.intern("SOMETYPE");
    init_sim.intern_rule_type_ids(&rules);
    init_sim.resolve_type_handles(&rules);
    let handles = init_sim.rule_handles();
    assert_eq!(init_sim.interner.resolve(handles.crush), "Crush");
    assert!(handles.is_crush(handles.crush));

    // A fixture that skips init resolution keeps None — combat treats every
    // warhead as non-crush and, critically, its interner is never mutated by
    // a tick pass, so historical fixture hashes cannot shift.
    let unresolved = Simulation::new();
    assert!(unresolved.rule_handles.is_none());
}

#[test]
#[should_panic(expected = "resolve_type_handles")]
fn rule_handles_accessor_panics_before_resolution() {
    let sim = Simulation::new();
    let _ = sim.rule_handles();
}

/// F07 characterization: the production app frame seam — drain due commands,
/// advance with the app-bound resources, then post-frame reads — pinned as a
/// contract before the SimRuntime extraction. The app adapter internally pins
/// its own canonical path snapshot (`advance_app_frame` has no path-grid
/// parameter), so callers cannot substitute navigation.
#[test]
fn current_rust_frame_call_order_is_preserved() {
    let rules = RuleSet::from_ini(&IniFile::from_str("")).expect("empty rules parse");
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[make_test_entity("MTNK", EntityCategory::Unit)],
        None,
        &empty_heights(),
    );
    let select = cmd_envelope(
        &sim,
        "Americans",
        sim.session.tick + 2,
        Command::Select {
            entity_ids: vec![1],
            additive: false,
        },
    );
    sim.queue_command(select);

    // Not yet due (dispatch admits execute_tick <= tick + 1): the drain
    // leaves the queue intact, and an empty-command frame carries it forward.
    assert!(sim.take_due_commands().is_empty());
    let _ = sim
        .advance_app_frame(
            &[],
            Some(&rules),
            &std::collections::BTreeMap::new(),
            None,
            16,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");

    // Step to the due tick with the exact app-shaped call: drained commands
    // in, bound resources, Ordinary lane. The command must execute within
    // THIS frame (drain-before-advance), not the next.
    let tick_before = sim.session.tick;
    let due = sim.take_due_commands();
    assert_eq!(due.len(), 1, "the queued command is due exactly once");
    let output = sim
        .advance_app_frame(
            &due,
            Some(&rules),
            &std::collections::BTreeMap::new(),
            None,
            16,
            TickLane::Ordinary,
            None,
        )
        .expect("fixture frame must complete");
    assert!(output.tick.frame_committed);
    assert_eq!(sim.session.tick, tick_before + 1);
    assert!(
        sim.substrate.entities.get(1).is_some_and(|e| e.selected),
        "a due command executes in the frame that drained it"
    );
    // Once-per-pass drain: nothing left for a second consumer.
    assert!(sim.take_due_commands().is_empty());

    // Post-frame reads (fog merge, digest) observe the committed frame.
    let owner = sim.interner.get("Americans").expect("owner interned");
    sim.fog.build_merged_for(owner, &sim.interner);
    let _ = sim.parity_digest();
}

/// F07: the runtime API preserves the characterized seam — drain from the
/// runtime's simulation, advance through `SimRuntime::advance_frame` with the
/// bound resources, and the due command executes within that same frame.
#[test]
fn runtime_frame_call_order_matches_the_app_seam() {
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[make_test_entity("MTNK", EntityCategory::Unit)],
        None,
        &empty_heights(),
    );
    let select = cmd_envelope(
        &sim,
        "Americans",
        sim.session.tick + 2,
        Command::Select {
            entity_ids: vec![1],
            additive: false,
        },
    );
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    runtime.simulation.queue_command(select);

    assert!(runtime.simulation.take_due_commands().is_empty());
    let _ = runtime
        .advance_frame(&[], 16, TickLane::Ordinary)
        .expect("fixture frame must complete");

    let due = runtime.simulation.take_due_commands();
    assert_eq!(due.len(), 1);
    let output = runtime
        .advance_frame(&due, 16, TickLane::Ordinary)
        .expect("fixture frame must complete");
    assert!(output.tick.frame_committed);
    assert!(
        runtime
            .simulation
            .substrate
            .entities
            .get(1)
            .is_some_and(|e| e.selected),
        "a due command executes in the runtime frame that drained it"
    );
    assert!(runtime.simulation.take_due_commands().is_empty());
}

/// F10: the debug-logging toggle is a sim-owned boundary method — enabling
/// allocates logs on every existing entity and stamps the spawn flag so
/// future spawns log too; disabling clears both.
#[test]
fn debug_toggle_updates_existing_and_future_entities() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=GACNST\n[GACNST]\nStrength=400\n",
    ))
    .expect("debug toggle rules");
    let height_map: BTreeMap<(u16, u16), u8> = BTreeMap::new();
    let mut sim = Simulation::new();
    let existing = sim
        .spawn_object("GACNST", "Player", 5, 5, 0, &rules, &height_map)
        .expect("existing spawn");
    assert!(
        sim.entities()
            .get(existing)
            .expect("existing")
            .debug_log
            .is_none(),
        "logging starts disabled"
    );

    sim.set_debug_event_logging(true);
    assert!(
        sim.entities()
            .get(existing)
            .expect("existing")
            .debug_log
            .is_some(),
        "enabling allocates a log on the existing entity"
    );
    let future = sim
        .spawn_object("GACNST", "Player", 9, 9, 0, &rules, &height_map)
        .expect("future spawn");
    assert!(
        sim.entities()
            .get(future)
            .expect("future")
            .debug_log
            .is_some(),
        "an entity spawned after enabling logs from the spawn flag"
    );

    sim.set_debug_event_logging(false);
    assert!(
        sim.entities()
            .get(existing)
            .expect("existing")
            .debug_log
            .is_none()
    );
    assert!(
        sim.entities()
            .get(future)
            .expect("future")
            .debug_log
            .is_none(),
        "disabling clears every entity's log"
    );
}

/// The `VoxelAnimClass` store is a live scheduler citizen, not a side table.
///
/// gamemd-derived: `VoxelAnimClass::Constructor @ 0x007493B0` assigns the
/// shared unique id, appends to the VoxelAnim registry, and `ObjectClass::
/// Unlimbo` reveals the piece into the LogicClass vector; `LogicClass::AI`
/// then visits it in that order every frame until `VoxelAnimClass::AI @
/// 0x00749F30` reaches `vtable+0xF8`. This pins the four wiring facts a
/// desync would ride in on: the id comes from the shared allocator, the piece
/// enters the live order, the state hash folds it, and a snapshot restores
/// both the store and its logic membership.
#[test]
fn gsi_05_14_death_debris_joins_the_live_order_the_hash_and_the_snapshot() {
    let voxel_type = crate::rules::voxel_anim_type::VoxelAnimType::from_ini_section(
        "TIRE",
        IniFile::from_str(
            "[TIRE]\nElasticity=0.8\nMinAngularVelocity=12.0\nMaxAngularVelocity=24.0\n\
             MinZVel=28.0\nMaxZVel=32.0\nMaxXYVel=10.0\nDuration=150\n",
        )
        .section("TIRE")
        .unwrap(),
    );

    let mut sim = Simulation::new();
    let empty_hash = sim.state_hash();
    let mut rng = crate::sim::rng::SimRng::new(17);
    let pieces: Vec<_> = (0..3)
        .map(|_| crate::sim::voxel_anim::VoxelDebrisSpawn {
            type_id: crate::rules::voxel_anim_type::VoxelAnimTypeId(0),
            object: crate::sim::voxel_anim::spawn_debris_piece(
                0,
                crate::rules::voxel_anim_type::VoxelAnimTypeId(0),
                &voxel_type,
                None,
                glam::IVec3::new(1280, 2560, 0),
                &mut rng,
            )
            .expect("in domain"),
        })
        .collect();
    sim.admit_death_debris(pieces);

    assert_eq!(sim.substrate.voxel_anims.len(), 3);
    let ids: Vec<u64> = sim.substrate.voxel_anims.ids();
    assert_eq!(
        sim.substrate
            .display
            .members(super::display_layers::DisplayLayer::AIR),
        ids
    );
    assert_eq!(
        sim.live_object_order_snapshot(),
        ids,
        "each piece is revealed into the live order in spawn order"
    );
    assert_ne!(
        sim.state_hash(),
        empty_hash,
        "the store folds into the state hash"
    );

    // The fold reads the physics body, not just the identity: a single tick of
    // the duration has to move the hash.
    let before_tick = sim.state_hash();
    let first_id = ids[0];
    sim.substrate
        .voxel_anims
        .get_mut(first_id)
        .unwrap()
        .duration -= 1;
    assert_ne!(sim.state_hash(), before_tick);
    sim.substrate
        .voxel_anims
        .get_mut(first_id)
        .unwrap()
        .duration += 1;
    assert_eq!(sim.state_hash(), before_tick);

    let expected_order = sim.live_object_order_snapshot();
    let expected_store = sim.substrate.voxel_anims.clone();
    let bytes = bincode::serialize(&sim).expect("serialize sim with VoxelAnimStore");
    let mut restored: Simulation =
        bincode::deserialize(&bytes).expect("deserialize VoxelAnimStore");
    restored.rebuild_logic_membership();
    assert_eq!(restored.live_object_order_snapshot(), expected_order);
    assert_eq!(
        restored.substrate.voxel_anims, expected_store,
        "every piece and its physics body survive the snapshot"
    );
    restored.debug_assert_logic_membership_consistent();

    // The scheduler slot advances the physics body, and a piece leaves the live
    // order once its AI reaches Delete.
    let first = first_id;
    let before = sim.substrate.voxel_anims.get(first).unwrap().world_coord();
    sim.visit_voxel_anim(first, None);
    let after = sim.substrate.voxel_anims.get(first).unwrap().world_coord();
    assert_ne!(before, after, "one AI visit moves the body");
    for _ in 0..200 {
        if !sim.substrate.voxel_anims.contains_key(first) {
            break;
        }
        sim.visit_voxel_anim(first, None);
    }
    assert!(
        !sim.substrate.voxel_anims.contains_key(first),
        "the piece expires inside its Duration"
    );
    assert!(
        !sim.live_object_order_snapshot().contains(&first),
        "Delete leaves the LogicVector"
    );
    assert_eq!(
        sim.substrate.display.layer_of(first),
        None,
        "Delete leaves Display too"
    );
    sim.debug_assert_logic_membership_consistent();
}

fn capture_eva_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=GAPOWR\n1=CAOILD\n\n\
         [GAPOWR]\nStrength=100\nArmor=wood\nCapturable=yes\n\n\
         [CAOILD]\nStrength=1000\nArmor=wood\nCapturable=yes\nNeedsEngineer=yes\n\
         CaptureEvaEvent=EVA_OilRefineryCaptured\n",
    );
    RuleSet::from_ini(&ini).expect("capture eva rules should parse")
}

fn captured_events(
    sim: &Simulation,
) -> Vec<(InternedId, InternedId, bool, bool, Option<InternedId>)> {
    sim.sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::BuildingCaptured {
                old_owner,
                new_owner,
                tech_building,
                radar,
                capture_eva_event,
            } => Some((
                *old_owner,
                *new_owner,
                *tech_building,
                radar.is_some_and(|request| {
                    request.event_type == crate::sim::radar::RadarEventType::BuildingCaptured
                }),
                *capture_eva_event,
            )),
            _ => None,
        })
        .collect()
}

/// `BuildingClass::ChangeOwner 0x004483FB..0x0044848F`: an ordinary building
/// goes through `CreateRadarEvent(10, cell)` and announces only when the
/// client's radar array accepted the event, so the world publishes the type-10
/// request with every such capture (the fourth tuple field). Row 10 of the
/// `0x007F0998` type table is not unique; `render::radar_events` pins that a
/// second capture next to a live event is accepted too.
#[test]
fn engineer_capture_of_an_ordinary_building_requests_the_radar_event() {
    let rules = capture_eva_rules();
    let mut sim = Simulation::new();
    sim.session.game_mode_nonzero = true;
    let player = insert_house_with_counts(&mut sim, "Americans", 0, 0);
    let enemy = insert_house_with_counts(&mut sim, "Russians", 2, 0);
    sim.houses.get_mut(&enemy).expect("enemy").is_human = false;
    insert_test_entity_for_owner(&mut sim, 1, enemy, "GAPOWR", EntityCategory::Structure);
    insert_test_entity_for_owner(&mut sim, 2, enemy, "GAPOWR", EntityCategory::Structure);
    sim.substrate
        .entities
        .get_mut(2)
        .expect("second plant")
        .position
        .rx = 11;

    sim.announce_engineer_capture(1, player, &rules);
    assert_eq!(
        captured_events(&sim),
        vec![(enemy, player, false, true, None)]
    );

    // A second capture one cell away publishes its own request as well.
    sim.sound_events.clear();
    sim.announce_engineer_capture(2, player, &rules);
    assert_eq!(
        captured_events(&sim),
        vec![(enemy, player, false, true, None)]
    );
}

/// `0x00448401 MOV CL,[Type+0x1552]` (`NeedsEngineer=`): no radar event; the
/// old owner's `EVA_TechBuildingLost` and the new owner's `CaptureEvaEvent=`
/// are the app's to route.
#[test]
fn engineer_capture_of_a_tech_building_carries_its_capture_eva_event() {
    let rules = capture_eva_rules();
    let mut sim = Simulation::new();
    sim.session.game_mode_nonzero = true;
    let player = insert_house_with_counts(&mut sim, "Americans", 0, 0);
    let civilian = insert_passive_house_with_counts(&mut sim, "Neutral", 1, 0);
    insert_test_entity_for_owner(&mut sim, 1, civilian, "CAOILD", EntityCategory::Structure);

    sim.announce_engineer_capture(1, player, &rules);
    let line = sim.interner.get("EVA_OilRefineryCaptured");
    assert!(
        line.is_some(),
        "the CaptureEvaEvent name is interned for the app"
    );
    assert_eq!(
        captured_events(&sim),
        vec![(civilian, player, true, false, line)],
        "NeedsEngineer skips CreateRadarEvent"
    );
}

/// `0x004483C6/0x004483D1`: a human house on one side is required, and
/// `0x004483E1`: a `MultiplayPassive` new owner never announces.
#[test]
fn engineer_capture_is_silent_without_a_human_side_or_into_a_passive_house() {
    let rules = capture_eva_rules();
    let mut sim = Simulation::new();
    // Skirmish: `IsControlledByHuman` is the human flag alone (`0x0050B730`).
    sim.session.game_mode_nonzero = true;
    let ai_a = insert_house_with_counts(&mut sim, "Russians", 1, 0);
    let ai_b = insert_house_with_counts(&mut sim, "Cubans", 0, 0);
    let civilian = insert_passive_house_with_counts(&mut sim, "Neutral", 0, 0);
    let player = insert_house_with_counts(&mut sim, "Americans", 1, 0);
    for ai in [ai_a, ai_b] {
        let house = sim.houses.get_mut(&ai).expect("ai house");
        house.is_human = false;
        house.player_control = false;
    }
    insert_test_entity_for_owner(&mut sim, 1, ai_a, "GAPOWR", EntityCategory::Structure);
    insert_test_entity_for_owner(&mut sim, 2, player, "GAPOWR", EntityCategory::Structure);

    sim.announce_engineer_capture(1, ai_b, &rules);
    assert!(
        captured_events(&sim).is_empty(),
        "AI-vs-AI capture: nobody listens"
    );
    sim.announce_engineer_capture(2, civilian, &rules);
    assert!(
        captured_events(&sim).is_empty(),
        "passive new owner is silent"
    );
}

/// `HouseClass::MPlayer_Defeated 0x004FC30F..0x004FC3BC`: every non-passive
/// defeat is announced; the app splits local from other.
#[test]
fn defeat_of_a_non_passive_house_emits_player_defeated() {
    let rules = short_game_defeat_test_rules();
    let mut sim = Simulation::new();
    let player = insert_house_with_counts(&mut sim, "Americans", 1, 0);
    let enemy = insert_house_with_counts(&mut sim, "Russians", 0, 0);
    let civilian = insert_passive_house_with_counts(&mut sim, "Neutral", 0, 0);

    check_defeat_now(&mut sim, Some(&rules));

    let defeated: Vec<InternedId> = sim
        .sound_events
        .iter()
        .filter_map(|event| match event {
            SimSoundEvent::PlayerDefeated { house } => Some(*house),
            _ => None,
        })
        .collect();
    assert_eq!(defeated, vec![enemy]);
    assert!(!sim.houses[&player].is_defeated);
    assert!(
        !sim.houses[&civilian].is_defeated,
        "passive houses are never evaluated"
    );

    // A house already flagged defeated is not announced again.
    sim.sound_events.clear();
    check_defeat_now(&mut sim, Some(&rules));
    assert!(
        sim.sound_events
            .iter()
            .all(|event| !matches!(event, SimSoundEvent::PlayerDefeated { .. }))
    );
}

/// I9c regression, a first search. Resuming an attack-move with no target and
/// no path issues a fresh move (`tick_order_intents_post_combat`), whose
/// first Process searches. That search once passed "not a crusher", so after
/// I9a a `Crusher=yes` tank resuming its attack-move was refused a sandbag
/// cell the crossing and every repath admit.
#[test]
fn attack_move_resume_lets_a_crusher_tank_through_a_sandbag_cell() {
    use crate::map::resolved_terrain::zone_class;
    let run = |regular_crusher: bool| {
        let mut terrain = gsi_04_10_clear_terrain(20, 1);
        terrain.cells[5].overlay_zone_type = Some(zone_class::CRUSHABLE);
        terrain.cells[5].zone_type = zone_class::CRUSHABLE;
        let path = PathGrid::from_resolved_terrain(&terrain);
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(terrain);
        let mut entity = GameEntity::test_default(1, "MTNK", "Americans", 0, 0);
        entity.category = EntityCategory::Unit;
        entity.regular_crusher = regular_crusher;
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
        entity.order_intent = Some(crate::sim::components::OrderIntent::AttackMove {
            goal_rx: 10,
            goal_ry: 0,
        });
        sim.substrate.entities.insert(entity);
        // The Process reads type names; snapshot after the entity interned its.
        sim.interner = crate::sim::intern::test_interner();
        sim.tick_order_intents_post_combat(Some(&path), None);
        sim.process_ground_locomotor_for_test(1, None, Some(&path), None)
            .expect("the first Process requests the route");
        sim.substrate
            .entities
            .get(1)
            .and_then(|e| e.movement_target.as_ref())
            .map(|t| t.path.clone())
    };
    let crusher = run(true).expect("crusher resumes its attack-move");
    assert!(
        crusher.contains(&(5, 0)),
        "crusher route crosses the sandbag: {crusher:?}"
    );
    assert!(
        run(false).is_none(),
        "non-crusher is refused at the sandbag"
    );
}
