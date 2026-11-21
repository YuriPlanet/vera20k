//! Frame pipeline tests: game-speed and building on/off-line ingress, state-hash
//! interval selection, animation and particle frame boundaries, dying-infantry
//! completion, app frame output delivery and the runtime frame call order.

use super::*;

fn game_speed_command_sim() -> (Simulation, crate::sim::intern::InternedId) {
    let mut sim = Simulation::with_seed(0x5EED_0001);
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    let owner = sim.interner.intern("Local");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    (sim, owner)
}

#[test]
fn combatant_selection_ingress_precedes_destroy_and_is_not_replayed_at_tail() {
    // Original MainTick55D8AB/55D8B4 -> P5367F0/732280 commits Select
    // before Logic55DC9E. ObjectReceiveDamage5F57AF -> Foot4D9720 ->
    // ObjectDetach5F5280 then Deselect5F44A0 wins in that same frame.
    // Execute those owners at their phase boundaries; this supplies the
    // exact-zero input without claiming a full native fatal-hit execution.
    let (mut sim, owner) = game_speed_command_sim();
    let mut infantry = GameEntity::test_default(1, "E1", "Local", 30, 40);
    infantry.set_owner_for_test(owner);
    infantry.set_type_ref_for_test(sim.interner.intern("E1"));
    infantry.set_category_for_test(EntityCategory::Infantry);
    infantry.lifecycle.set_object_alive_for_test(true);
    infantry.lifecycle.set_in_limbo_for_test(false);
    infantry.seldom.selection.set_selected_for_test(false);
    sim.substrate.entities.insert(infantry);
    let commands = [CommandEnvelope::new(
        owner,
        1,
        Command::Select {
            entity_ids: vec![1],
            additive: false,
        },
    )];

    let ingress_count = sim.apply_due_frame_ingress_commands(
        &commands,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        1,
        &mut crate::sim::world::FrameEffects::default(),
    );
    let selected_before_logic = sim
        .entities()
        .get(1)
        .unwrap()
        .seldom
        .selection
        .is_selected();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .health
        .set_for_test(0);
    sim.detach_all(1, true, UninitContext::default());
    assert!(
        !sim.entities()
            .get(1)
            .unwrap()
            .seldom
            .selection
            .is_selected()
    );
    let (tail_count, spawned) = sim.apply_due_commands(
        &commands,
        crate::rules::ruleset::RuleSet::empty_for_test(),
        1,
        &mut crate::sim::world::FrameEffects::default(),
    );

    assert!(
        !sim.entities()
            .get(1)
            .unwrap()
            .seldom
            .selection
            .is_selected(),
        "an earlier selection must not undo same-frame Destroy/Deselect"
    );
    assert!(selected_before_logic, "local Select commits before Logic");
    assert_eq!((ingress_count, tail_count, spawned), (1, 0, false));
    assert!(sim.entities().get(1).unwrap().is_object_alive());
    assert!(!sim.entities().get(1).unwrap().lifecycle.in_limbo());
    // A distinct later real P remains admitted, as the executed native
    // infantry_health_zero_alive_true control pins. Do not add a health gate.
    assert!(sim.try_select_object(
        1,
        UninitContext::new(crate::rules::ruleset::RuleSet::empty_for_test())
    ));
    assert!(
        sim.entities()
            .get(1)
            .unwrap()
            .seldom
            .selection
            .is_selected()
    );
}

#[test]
fn game_speed_transition_applies_at_ingress_before_triggers_and_hash() {
    let (mut sim, owner) = game_speed_command_sim();
    let (mut control, _) = game_speed_command_sim();
    let command = CommandEnvelope::new(owner, 1, Command::SetGameSpeed { speed: 4 });

    let result = sim
        .advance_master_frame(
            &[command],
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            67,
            TickLane::Ordinary,
            false,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();
    let control_result = control
        .advance_master_frame(
            &[],
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            67,
            TickLane::Ordinary,
            false,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();

    assert!(result.frame_committed);
    assert_eq!(result.executed_commands, 1);
    assert_eq!(result.tick, control_result.tick);
    assert_eq!(sim.session.binary_frame(), control.session.binary_frame());
    assert_eq!(sim.session.game_options().game_speed, 4);
    assert_eq!(control.session.game_options().game_speed, 1);
    assert_ne!(result.state_hash, control_result.state_hash);
    assert_eq!(result.state_hash, Some(sim.state_hash()));
    let order = [
        FrameStep::SessionCommands,
        FrameStep::Triggers,
        FrameStep::TeamScript,
        FrameStep::DiskLasers,
        FrameStep::LightningStorm,
        FrameStep::LogicVector,
    ];
    let trace = sim.take_frame_step_trace();
    let visited: Vec<_> = trace
        .into_iter()
        .filter(|step| order.contains(step))
        .collect();
    assert!(visited.starts_with(&order));
}

#[test]
fn live_game_speed_event_decodes_and_applies_within_its_frame() {
    // Native opcode 0x0D (EventClass::Execute type 13 @ 0x004C794B): the payload
    // dword at Event+7 is the new GameSpeed setting (0x00A8EB60). Unlike the
    // offline SetGameSpeed it applies at the EventClass tail of its frame.
    let (mut sim, owner) = game_speed_command_sim();
    let (mut control, _) = game_speed_command_sim();

    // A native 0x0D record for header house 0 (== owner) carrying speed 5.
    let record = crate::sim::command::GameSpeedRecord {
        house_id: 0,
        frame: 1,
        speed: 5,
    }
    .encode()
    .expect("encode 0x0D notice");
    let decoded = sim
        .decode_native_command_record(&record, 1)
        .expect("a 0x0D record decodes");
    assert_eq!(decoded.owner, owner);
    assert_eq!(decoded.payload, Command::LiveGameSpeed { speed: 5 });

    let command = CommandEnvelope::new(owner, 1, Command::LiveGameSpeed { speed: 5 });
    let result = sim
        .advance_master_frame(
            &[command],
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            67,
            TickLane::Ordinary,
            false,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();
    let control_result = control
        .advance_master_frame(
            &[],
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            67,
            TickLane::Ordinary,
            false,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();

    assert!(result.frame_committed);
    assert_eq!(result.executed_commands, 1);
    assert_eq!(sim.session.game_options().game_speed, 5);
    assert_eq!(control.session.game_options().game_speed, 1);
    assert_ne!(result.state_hash, control_result.state_hash);
}

#[test]
fn building_on_off_line_opcodes_decode_direction_and_target() {
    // Native opcodes 0x01/0x02 (EventClass::Execute branches 0x004C6CF0 /
    // 0x004C6D4D) drive the Building+660 latch via GoOnline 0x00452260 /
    // GoOffline 0x00452360; the direction is the opcode, the target the
    // Abstract token at payload[0..4] (kind 0x34).
    let (mut sim, owner) = game_speed_command_sim();
    let mut building = GameEntity::test_default(7, "GAPOWR", "Local", 30, 40);
    building.set_owner_for_test(owner);
    sim.substrate.entities.insert(building);

    let on = crate::sim::command::BuildingOnLineRecord {
        house_id: 0,
        frame: 1,
        entity_id: 7,
        on: true,
    }
    .encode()
    .expect("encode 0x01 On Line");
    let off = crate::sim::command::BuildingOnLineRecord {
        house_id: 0,
        frame: 1,
        entity_id: 7,
        on: false,
    }
    .encode()
    .expect("encode 0x02 Off Line");

    let decoded_on = sim
        .decode_native_command_record(&on, 1)
        .expect("0x01 decodes");
    let decoded_off = sim
        .decode_native_command_record(&off, 1)
        .expect("0x02 decodes");
    assert_eq!(decoded_on.owner, owner);
    assert_eq!(
        decoded_on.payload,
        Command::SetBuildingOnLine {
            entity_id: 7,
            on: true
        }
    );
    assert_eq!(
        decoded_off.payload,
        Command::SetBuildingOnLine {
            entity_id: 7,
            on: false
        }
    );

    // 0x004C6CF0/0x004C6D4D drop a dead or limboed target before GoOffline.
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[BuildingTypes]\n1=GAPOWR\n[GAPOWR]\nPower=-20\n",
        ))
        .unwrap();
    {
        let building = sim.substrate.entities.get_mut(7).unwrap();
        building.set_type_ref_for_test(sim.interner.intern("GAPOWR"));
        building.set_category_for_test(EntityCategory::Structure);
        building.lifecycle.set_object_alive_for_test(true);
        building.lifecycle.set_in_limbo_for_test(true);
    }
    let off = Command::SetBuildingOnLine {
        entity_id: 7,
        on: false,
    };
    assert!(!sim.apply_command("Local", &off, Some(&rules)));
    assert!(sim.substrate.entities.get(7).unwrap().building_online());
    sim.substrate
        .entities
        .get_mut(7)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(false);
    assert!(sim.apply_command("Local", &off, Some(&rules)));
    assert!(!sim.substrate.entities.get(7).unwrap().building_online());
}

/// Native's last 0x01/0x02 gate (`0x004C6D2D`/`0x004C6D8A`) reads
/// `[target+0x520]+0x16C0`, and `+0x520` is `BuildingClass::pType`
/// (docs/ghidra-workflow.md): the target TYPE's `FirestormWall=` byte
/// (`ObjectType.firestorm_wall`). A Firestorm Wall building is driven by the
/// firestorm logic, so the record is dropped before GoOnline/GoOffline; stock
/// rules leave the key unset and the gate is open.
#[test]
fn building_on_off_line_dropped_for_a_firestorm_wall_type() {
    let (mut sim, owner) = game_speed_command_sim();
    let mut building = GameEntity::test_default(7, "GAPOWR", "Local", 30, 40);
    building.set_owner_for_test(owner);
    building.set_type_ref_for_test(sim.interner.intern("GAPOWR"));
    building.set_category_for_test(EntityCategory::Structure);
    building.lifecycle.set_object_alive_for_test(true);
    building.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(building);

    let off = Command::SetBuildingOnLine {
        entity_id: 7,
        on: false,
    };

    // A `FirestormWall=yes` type is dropped; the online latch stays set.
    let firestorm = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n1=GAPOWR\n[GAPOWR]\nPower=-20\nFirestormWall=yes\n",
    ))
    .unwrap();
    assert!(!sim.apply_command("Local", &off, Some(&firestorm)));
    assert!(sim.substrate.entities.get(7).unwrap().building_online());

    // The stock (keyless) type passes the gate and the latch clears.
    let stock = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n1=GAPOWR\n[GAPOWR]\nPower=-20\n",
    ))
    .unwrap();
    assert!(sim.apply_command("Local", &off, Some(&stock)));
    assert!(!sim.substrate.entities.get(7).unwrap().building_online());
}

/// A launch hashes every frame and a test world none until the caller asks.
/// Then only a frame whose tick is a multiple of the interval hashes, and its
/// hash is the state's.
#[test]
fn a_state_hash_interval_selects_frames_by_tick() {
    let mut launched =
        Simulation::from_descriptor(&crate::sim::scenario_session::ScenarioDescriptor::default());
    let mut test_world = Simulation::with_seed(0x5EED_0001);
    for sim in [&mut launched, &mut test_world] {
        let owner = sim.interner.intern("Local");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 0, None, true, 0, 10,
            ));
    }
    let launched_first = launched.advance_tick(&[], None, None, None, 67);
    assert!(launched_first.frame_committed);
    assert_eq!(launched_first.state_hash, Some(launched.state_hash()));
    let mut sim = test_world;
    let first = sim.advance_tick(&[], None, None, None, 67);
    assert!(first.frame_committed);
    assert_eq!(first.state_hash, None);
    sim.hash_state_every(std::num::NonZeroU64::new(3).unwrap());
    for step in 1..=6 {
        let foretold = sim.next_frame_hashes_state();
        let result = sim.advance_tick(&[], None, None, None, 67);
        assert_eq!(
            result.tick,
            first.tick + step,
            "each frame advances the tick"
        );
        assert_eq!(
            foretold,
            result.state_hash.is_some(),
            "tick {} was foretold",
            result.tick
        );
        assert_eq!(
            result.state_hash,
            result.tick.is_multiple_of(3).then(|| sim.state_hash()),
            "tick {}",
            result.tick
        );
    }
}

/// A frame that ends the match hashes between intervals, so the replay log's
/// last frame carries the final state: here the frame an EXIT executes on.
#[test]
fn a_match_ending_frame_hashes_between_intervals() {
    let (mut sim, owner) = game_speed_command_sim();
    let interval = crate::sim::replay::PLAY_HASH_INTERVAL;
    sim.hash_state_every(interval);
    let committed = sim.advance_tick(&[], None, None, None, 67);
    assert!(committed.frame_committed);
    assert_eq!(committed.state_hash, None, "tick {}", committed.tick);

    let exit = CommandEnvelope::new(owner, committed.tick + 1, Command::ExitMatch);
    let ended = sim.advance_tick(&[exit], None, None, None, 67);
    assert_eq!(ended.executed_commands, 1);
    assert!(!ended.frame_committed);
    // It keeps the last committed tick, which is off the interval.
    assert_eq!(ended.tick, committed.tick);
    assert_eq!(ended.state_hash, Some(sim.state_hash()));
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
        None,
        None,
        67,
    );
    let invalid_control_result = invalid_control.advance_tick(&[], None, None, None, 67);
    assert_eq!(invalid_result.executed_commands, 1);
    assert_eq!(invalid.session.game_options().game_speed, 1);
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
        None,
        None,
        67,
    );
    let unknown_control_result = unknown_control.advance_tick(&[], None, None, None, 67);
    assert_eq!(unknown_result.executed_commands, 1);
    assert_eq!(unknown.session.game_options().game_speed, 1);
    assert_eq!(unknown_result.state_hash, unknown_control_result.state_hash);
}

#[test]
fn game_speed_ingress_uses_house_order_and_survives_same_frame_exit() {
    let (mut sim, local) = game_speed_command_sim();
    let remote = sim.interner.intern("Remote");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            remote, 1, None, false, 0, 10,
        ));
    let commands = [
        CommandEnvelope::new(remote, 1, Command::SetGameSpeed { speed: 2 }),
        CommandEnvelope::new(local, 1, Command::SetGameSpeed { speed: 4 }),
        CommandEnvelope::new(local, 1, Command::ExitMatch),
    ];

    let result = sim.advance_tick(&commands, None, None, None, 67);

    assert!(!result.frame_committed);
    assert_eq!(result.executed_commands, 3);
    assert_eq!(sim.session.game_options().game_speed, 2);
    assert!(*sim.quit_requested);
    assert_eq!(result.state_hash, Some(sim.state_hash()));
}

/// A WOL tournament player's EXIT runs DestroyConnection(house, 0) on every
/// remaining peer (`0x0064C9B1`), whose tail kills the leaver's House
/// (`0x005DA8F7..0x005DA90F`). A replay runs only the shared EXIT.
#[test]
fn a_tournament_players_exit_flags_their_house_on_the_remaining_peers() {
    let (mut sim, local) = game_speed_command_sim();
    let leaver = sim.interner.intern("Leaver");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            leaver, 1, None, true, 0, 10,
        ));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Wol);
    sim.install_wol_start(1, Default::default());
    sim.session.set_current_house_for_test(Some(local));

    let exit = CommandEnvelope::new(leaver, 1, Command::ExitMatch);
    let result = sim.advance_tick(&[exit], None, None, None, 67);

    assert_eq!(result.executed_commands, 1);
    let house = sim.houses.get(&leaver).unwrap();
    assert!(house.is_to_die() || house.has_lost());
    assert_eq!(
        sim.take_departed_senders(),
        [sim.houses.index_of(leaver).unwrap() as i8]
    );
}

#[test]
fn network_modal_does_not_execute_game_speed_ingress() {
    let (mut sim, owner) = game_speed_command_sim();
    let command = CommandEnvelope::new(owner, 1, Command::SetGameSpeed { speed: 4 });

    let result = sim
        .advance_master_frame(
            &[command],
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            67,
            TickLane::NetworkModal,
            false,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();

    assert_eq!(result.executed_commands, 0);
    assert_eq!(sim.session.game_options().game_speed, 1);
}

fn animation_boundary_fixture() -> (Simulation, RuleSet) {
    let mut sim = Simulation::with_seed(0xA11A_7100);
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    let ini = IniFile::from_str("[InfantryTypes]\n0=E1\n\n[E1]\nStrength=100\n");
    let art = IniFile::from_str(
        "[E1]\nSequence=TestSequence\n\
         [TestSequence]\nReady=1,1,1\nIdle1=0,1,0,S\n",
    );
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("animation fixture rules");
    rules.install_art_data(ArtRegistry::from_ini(&art));
    let id = sim
        .spawn_object("E1", "Americans", 4, 4, 0, &rules)
        .expect("living infantry");
    assert_eq!(id, 1);
    sim.substrate.entities.get_mut(id).unwrap().body_facing = FacingClass::new(0, 4);
    assert!(
        sim.infantry_do_action(
            id,
            crate::sim::movement::infantry_action::DO_IDLE1,
            true,
            &rules,
            &mut crate::sim::world::FrameEffects::default()
        )
        .unwrap()
    );
    // Original520AE0 completes Idle1 at its record's signed count, then
    // applies the facing hint and requests Ready. Supply that boundary;
    // the ordinary object visit, not presentation animation, consumes it.
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .set_native_stage_value(1);
    (sim, rules)
}

fn particle_frame_boundary_fixture(frame_count: u16) -> (Simulation, RuleSet) {
    let mut rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nWindDirection=1\n[Particles]\n0=SmokeP\n\
         [SmokeP]\nBehavesLike=Smoke\nImage=SMOKEIMG\nStateAIAdvance=1\n\
         EndStateAI=2\nDeleteOnStateLimit=yes\nMaxEC=100\n\
         [ParticleSystems]\n0=SmokeSys\n\
         [SmokeSys]\nBehavesLike=Smoke\nHoldsWhat=SmokeP\nSpawns=no\n\
         Lifetime=100\nParticleCap=10\n",
    ))
    .expect("particle boundary rules");
    rules.set_effect_frame_count_for_test("SMOKEIMG", frame_count, frame_count);

    let mut sim = Simulation::with_seed(0xEFFE_C705);
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    let stable_id = sim.substrate.allocate_stable_id();
    let particle = Particle::new(
        ParticleTypeId(0),
        IVec3::ZERO,
        IVec3::ZERO,
        &rules,
        &mut sim,
    );
    sim.substrate.particle_systems.insert({
        let mut system =
            ParticleSystem::test_fixture(stable_id, ParticleSystemTypeId(0), IVec3::ZERO);
        system.set_particles_for_test(vec![particle]);
        system.set_lifetime_for_test(100);
        system.set_facing_for_test(0);
        system.set_done_spawning_for_test(true);
        system
    });
    assert!(sim.reveal_particle_system(stable_id));
    (sim, rules)
}

#[test]
fn app_and_headless_frames_hash_identically_for_particle_frame_timing() {
    let (mut app_sim, rules) = particle_frame_boundary_fixture(5);
    let (mut headless_sim, _) = particle_frame_boundary_fixture(5);

    for frame in 1..=4 {
        let app = app_sim
            .advance_app_frame(
                &[],
                &rules,
                67,
                TickLane::Ordinary,
                &mut crate::sim::world::FrameEffects::default(),
            )
            .expect("fixture frame must complete");
        let headless = headless_sim.advance_tick(&[], Some(&rules), None, None, 67);

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
            .map(|(_, system)| system.particles().len())
            .unwrap_or(0);
        let headless_particles = headless_sim
            .particle_systems()
            .iter()
            .next()
            .map(|(_, system)| system.particles().len())
            .unwrap_or(0);
        assert_eq!(app_particles, headless_particles, "frame {frame}");
        if frame == 4 {
            assert_eq!(app_particles, 0, "native UID cadence retires by frame4");
        }
    }
}

#[test]
fn advance_tick_finishes_dying_infantry_from_rules_catalog() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n0=E1\n\
         [VehicleTypes]\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [E1]\nStrength=100\n",
    );
    let art_ini = IniFile::from_str(
        "[E1]\nSequence=TestSequence\n\
         [TestSequence]\nReady=0,1,1\nDie1=8,2,0\n",
    );
    let mut rules =
        RuleSet::from_ini_with_fixed_art_for_test(&ini, &art_ini).expect("dying infantry rules");
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    let mut sim = Simulation::new();
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    let id = sim
        .spawn_object("E1", "Americans", 4, 4, 0, &rules)
        .expect("spawn infantry");
    sim.substrate
        .entities
        .get_mut(id)
        .expect("spawned infantry")
        .health
        .set_for_test(0);
    sim.begin_infantry_death_sequence(
        id,
        crate::sim::world::InfantryDeathSequence::Die1,
        &rules,
        &mut crate::sim::world::FrameEffects::default(),
    );
    let corpse = sim.substrate.entities.get(id).expect("Die1 receiver");
    assert_eq!(corpse.mission_leaf.as_infantry().unwrap().doing(), 11);
    assert_eq!(corpse.infantry_sprite_pose(), Some((11, 0)));
    assert_eq!(
        [
            corpse.native_stage().timer().start_frame(),
            corpse.native_stage().timer().duration(),
            corpse.native_stage().rate(),
        ],
        [0, 1, 1],
    );

    // The visit at the DoAction restart frame cannot advance its absolute
    // clock. Original stage_clock_receipt pins this repeated-frame gate.
    let same_frame = sim.advance_tick(&[], Some(&rules), None, None, 67);
    assert_eq!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .infantry_sprite_pose(),
        Some((11, 0)),
    );

    let first = sim.advance_tick(&[], Some(&rules), None, None, 67);
    let after_first = sim
        .substrate
        .entities
        .get(id)
        .expect("two-frame death survives its first visit");
    assert_eq!(after_first.infantry_sprite_pose(), Some((11, 1)),);
    assert_eq!(after_first.mission_leaf.as_infantry().unwrap().doing(), 11);
    assert_eq!(
        [
            after_first.native_stage().timer().start_frame(),
            after_first.native_stage().timer().duration(),
            after_first.native_stage().rate(),
        ],
        [1, 1, 1],
    );

    let second = sim.advance_tick(&[], Some(&rules), None, None, 67);

    assert!(same_frame.frame_committed && first.frame_committed && second.frame_committed);
    assert!(
        sim.substrate.entities.get(id).is_none(),
        "the headless adapter must use RuleSet timing and drain the finished death",
    );
    assert_eq!(second.state_hash, Some(sim.state_hash()));
}

#[test]
fn terminal_master_frame_visits_class_sequence_before_exit_without_frame_commit() {
    let (mut sim, rules) = animation_boundary_fixture();
    let owner = insert_house_with_counts(&mut sim, "Americans", 1, 1);
    let exit = CommandEnvelope::new(owner, 1, Command::ExitMatch);

    let result = sim
        .advance_master_frame(
            &[exit],
            &rules,
            None,
            67,
            TickLane::Ordinary,
            false,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete")
        .into_tick();

    // Main_Tick's live Logic walk precedes EventClass's EXIT tail. Original
    // Infantry520AE0 therefore completes this supplied Idle1 boundary before
    // termination skips frame commit and the deferred-delete drain. The retired
    // late presentation clock incorrectly made this class update conditional
    // on successful frame commit.
    assert!(!result.frame_committed);
    assert_eq!(result.executed_commands, 1);
    assert_eq!(sim.session.tick(), 0);
    assert_eq!(sim.session.binary_frame(), 0);
    let entity = sim.substrate.entities.get(1).expect("living infantry");
    assert_eq!(entity.mission_leaf.as_infantry().unwrap().doing(), 0);
    assert_eq!(entity.infantry_sprite_pose(), Some((0, 0)));
    assert_eq!(entity.native_stage().rate(), 0);
    assert_eq!(entity.native_stage().timer().start_frame(), 0);
    assert_eq!(entity.body_facing.destination(), 0x8000);
    assert_eq!(result.state_hash, Some(sim.state_hash()));
}

#[test]
fn app_frame_output_transfers_pre_tick_sound_exactly_once_without_hash_change() {
    let mut sim = Simulation::new();
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    sim.sound_events.emit(SimSoundEvent::AnimationStarted {
        anim_id: 9,
        sound_id: crate::rules::ruleset::SoundName::Unlisted("WaterfallLoop".into()),
        world: crate::sim::anim_class::AnimWorldCoord {
            x: 128,
            y: 128,
            z: 0,
        },
    });

    let first = sim
        .advance_app_frame(
            &[],
            crate::rules::ruleset::RuleSet::empty_for_test(),
            67,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");
    assert!(matches!(
        first.sound_events.as_slice(),
        [SimSoundEvent::AnimationStarted { anim_id: 9, .. }]
    ));
    assert_eq!(first.tick.state_hash, Some(sim.state_hash()));

    let second = sim
        .advance_app_frame(
            &[],
            crate::rules::ruleset::RuleSet::empty_for_test(),
            67,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");
    assert!(second.sound_events.is_empty());
    assert_eq!(second.tick.state_hash, Some(sim.state_hash()));
}

#[test]
fn app_frame_output_finalizes_overlay_navigation_and_delivers_updates_once() {
    let (rules, _) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    *sim.resolved_terrain = Some(gsi_04_10_clear_terrain(4, 4));
    *sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(4, 4));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let owner = sim.interner.intern("WallOwner");
    sim.overlay_grid
        .as_mut()
        .expect("overlay grid")
        .place_owned_wall(2, 2, 2, 0x23, owner);

    let deferred = sim
        .advance_app_frame_without_overlay_types_for_test(
            &[],
            &rules,
            67,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
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
            &rules,
            67,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
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
    assert_eq!(first.tick.state_hash, Some(sim.state_hash()));

    let second = sim
        .advance_app_frame(
            &[],
            &rules,
            67,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");
    assert!(second.overlay_updates.is_empty());
    assert_eq!(second.tick.state_hash, Some(sim.state_hash()));
}

#[test]
fn terminal_app_frame_finalizes_overlay_updates_before_hash() {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [OverlayTypes]\n0=ORE\n1=WALL\n\
         [ORE]\nTiberium=yes\n[WALL]\nWall=yes\nStrength=100\n",
    );
    let rules = RuleSet::from_ini(&ini).expect("terminal overlay rules");
    let mut terrain = gsi_04_10_clear_terrain(2, 1);
    terrain
        .cell_mut_for_test(0, 0)
        .expect("terrain cell")
        .slope_type = 5;

    let mut sim = Simulation::new();
    *sim.resolved_terrain = Some(terrain);
    *sim.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(2, 1));
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
            &rules,
            67,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
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
    assert_eq!(output.tick.state_hash, Some(sim.state_hash()));

    let next = sim
        .advance_app_frame(
            &[],
            &rules,
            67,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");
    assert!(!next.tick.frame_committed);
    assert!(next.overlay_updates.is_empty());
}

fn gate_test_rules() -> RuleSet {
    let ini: IniFile = IniFile::from_str(
        "[InfantryTypes]\n\n\
         [VehicleTypes]\n\n\
         [AircraftTypes]\n\n\
         [BuildingTypes]\n0=GAGATE_A\n\n\
         [GAGATE_A]\nStrength=500\nArmor=wood\nGate=yes\nDeployTime=.066\nGateCloseDelay=.2\n",
    );
    RuleSet::from_ini_with_fixed_art_for_test(
        &ini,
        &IniFile::from_str("[GAGATE_A]\nFoundation=3x1\n"),
    )
    .expect("gate test rules should parse")
}

#[test]
fn native_frame_committed_late_gate_captures_pre_increment_frame() {
    // The native frame is committed LATE, so every step of the frame sees
    // frame N during the whole advance. The host duration is deliberately one
    // millisecond: admission, not elapsed time, advances the frame.

    let mut sim = Simulation::new();
    let rules = gate_test_rules();
    let gate_id = sim
        .spawn_object("GAGATE_A", "Americans", 10, 10, 0, &rules)
        .expect("spawn gate");
    sim.substrate
        .mission_assign_exact(gate_id, crate::sim::mission::MissionId::NONE, 0)
        .unwrap();
    sim.mission_queue_exact(
        gate_id,
        crate::sim::mission::MissionId::from_known(MissionType::Open),
        0,
        0,
        &crate::sim::mission::authority::LiveReadyInputProvider { rules: &rules },
    )
    .unwrap();
    sim.substrate.mission_commence_exact(gate_id, 0).unwrap();
    assert_eq!(sim.session.binary_frame(), 0, "fresh sim starts at frame 0");

    let _ = sim.advance_tick(&[], Some(&rules), None, None, 1);

    // Committed late: post-tick frame advanced to 1.
    assert_eq!(
        sim.session.binary_frame(),
        1,
        "native frame committed late to 1"
    );
    // The consumer captured the PRE-increment frame 0 during the tick.
    let gate = sim.substrate.entities.get(gate_id).expect("gate entity");
    assert_eq!(gate.door_phase(), crate::sim::door::DoorPhase::Opening);
    assert_eq!(gate.mission.handler_state(), 1);
    assert_eq!(
        gate.door_timer_fields().0,
        0,
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
    *sim.interner = crate::sim::intern::test_interner();
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

/// F07: the runtime API preserves the characterized seam — drain from the
/// runtime's simulation, advance through `SimRuntime::advance_frame` with the
/// bound resources, and the due command executes within that same frame.
#[test]
fn runtime_frame_call_order_matches_the_app_seam() {
    let rules = driving_combat_test_rules();
    let mut sim: Simulation = Simulation::new();
    sim.spawn_from_map(
        &[make_test_entity("MTNK", EntityCategory::Unit)],
        Some(&rules),
    );
    let select = cmd_envelope(
        &sim,
        "Americans",
        sim.session.tick() + 2,
        Command::Select {
            entity_ids: vec![1],
            additive: false,
        },
    );
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    runtime.resources_mut().rules = rules;
    runtime.simulation.queue_command(select);

    assert!(runtime.simulation.take_due_commands().is_empty());
    let _ = runtime
        .advance_frame(
            &[],
            16,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");

    let due = runtime.simulation.take_due_commands();
    assert_eq!(due.len(), 1);
    let output = runtime
        .advance_frame(
            &due,
            16,
            TickLane::Ordinary,
            &mut crate::sim::world::FrameEffects::default(),
        )
        .expect("fixture frame must complete");
    assert!(output.tick.frame_committed);
    assert!(
        runtime
            .simulation
            .substrate
            .entities
            .get(1)
            .is_some_and(|e| e.seldom.selection.is_selected()),
        "a due command executes in the runtime frame that drained it"
    );
    assert!(runtime.simulation.take_due_commands().is_empty());
}

/// A frame is one native logic frame however fast the host paced it, so the
/// session's simulated clock must not follow the step its caller declared
/// (issue #5107: the client declares 22 ms, headless 66 ms).
#[test]
fn the_session_clock_counts_native_frames_not_the_host_step() {
    let mut client = Simulation::new();
    let mut headless = Simulation::new();
    for _ in 0..3 {
        client.advance_tick(&[], None, None, None, 22);
        headless.advance_tick(&[], None, None, None, 66);
    }
    assert_eq!(
        client.session.total_sim_ms(),
        headless.session.total_sim_ms(),
        "22 ms and 66 ms steps report different simulated time for 3 frames"
    );
    assert_eq!(
        client.session.total_sim_ms(),
        3 * 1000 / u64::from(crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND),
        "3 native frames are 200 ms, not 3 x the host's step"
    );
}
