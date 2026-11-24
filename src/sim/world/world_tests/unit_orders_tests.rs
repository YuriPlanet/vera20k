//! Unit order tests: selection admission and select events, Drive and Ship state
//! preserved across move and stop orders, move-event ownership, refused move
//! orders and Construction Yard undeploy into an MCV.

use super::*;

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

/// Declare one human house and one AI house, the ordinary skirmish shape.
fn declare_selection_gate_houses(sim: &mut Simulation) {
    for (name, is_human) in [("Americans", true), ("Soviet", false)] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                id, 0, None, is_human, 0, 10,
            ));
    }
}

#[test]
fn item83_final_select_allows_caller_admitted_nonlocal_entity() {
    let mut sim: Simulation = Simulation::new();
    let rules = selection_gate_test_rules();
    let mine = sim
        .spawn_object("MTNK", "Americans", 20, 22, 0, &rules)
        .expect("spawn own MTNK");
    let theirs = sim
        .spawn_object("MTNK", "Soviet", 24, 22, 0, &rules)
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
    let _ = sim.advance_tick(&[select], Some(&rules), None, None, 33);

    assert!(
        sim.substrate
            .entities
            .get(mine)
            .is_some_and(|e| e.seldom.selection.is_selected())
    );
    assert!(
        sim.substrate
            .entities
            .get(theirs)
            .is_some_and(|e| e.seldom.selection.is_selected())
    );
}

#[test]
fn test_select_command_rejects_limbo_object() {
    let mut sim: Simulation = Simulation::new();
    let rules = selection_gate_test_rules();
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
    let _ = sim.advance_tick(&[select], Some(&rules), None, None, 33);

    assert!(
        !sim.substrate
            .entities
            .get(cargo)
            .is_some_and(|e| e.seldom.selection.is_selected()),
        "an in-limbo object has no map presence to select"
    );
}

#[test]
fn item83_fresh_selection_rejects_warp_out_but_keeps_preexisting_selection() {
    let mut sim = Simulation::new();
    let rules = selection_gate_test_rules();
    let tank = sim
        .spawn_object("MTNK", "Americans", 20, 22, 0, &rules)
        .expect("spawn MTNK");
    let wingman = sim
        .spawn_object("MTNK", "Americans", 21, 22, 0, &rules)
        .expect("spawn second MTNK");
    assert!(sim.try_select_object(tank, UninitContext::new(&rules)));
    // BeingWarpedOut (`+0x270`): a Temporal chain's head.
    sim.substrate
        .entities
        .get_mut(tank)
        .unwrap()
        .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(
            wingman,
        ));

    assert!(
        sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .seldom
            .selection
            .is_selected(),
        "entering warp-out does not retroactively remove an existing selection"
    );
    assert!(sim.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: vec![tank, wingman],
            additive: true,
        },
        Some(&rules),
    ));
    assert!(
        sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .seldom
            .selection
            .is_selected()
    );
    assert!(
        sim.substrate
            .entities
            .get(wingman)
            .unwrap()
            .seldom
            .selection
            .is_selected()
    );

    assert!(sim.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: vec![wingman],
            additive: false,
        },
        Some(&rules),
    ));
    assert!(
        !sim.substrate
            .entities
            .get(tank)
            .unwrap()
            .seldom
            .selection
            .is_selected(),
        "an ordinary replacement still deselects an omitted warp-out member"
    );
    assert!(!sim.try_select_object(tank, UninitContext::new(&rules)));
}

#[test]
fn event33_select_command_raises_tags_once_in_payload_order_after_admission() {
    let mut sim = Simulation::new();
    let rules = selection_gate_test_rules();
    let first = sim
        .spawn_object("MTNK", "Americans", 20, 22, 0, &rules)
        .unwrap();
    let second = sim
        .spawn_object("MTNK", "Americans", 21, 22, 0, &rules)
        .unwrap();
    let refused = sim
        .spawn_object("NOSEL", "Americans", 22, 22, 0, &rules)
        .unwrap();
    // Both admitted objects have already passed the graphical input discovery
    // gate. The command below is the same ordered payload kept in a replay.
    for id in [first, second] {
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .seldom
            .discovery
            .set_discovered_by_current_house_for_test(true);
    }
    let map = crate::map::map_file::MapFile::from_bytes(
        b"[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
          [Basic]\nNewINIFormat=4\n\
          [Triggers]\nFIRST=<none>,<none>,First selected,0,1,1,1,0\n\
          SECOND=<none>,<none>,Second selected,0,1,1,1,0\n\
          [Tags]\nONE=2,First selection,FIRST\nTWO=2,Second selection,SECOND\n\
          [Events]\nFIRST=1,33,0,0\nSECOND=1,33,0,0\n\
          [Actions]\nFIRST=1,11,4,MISSION:First,0,0,0,0,A\n\
          SECOND=1,11,4,MISSION:Second,0,0,0,0,A\n",
    )
    .unwrap();
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
        None,
    );
    sim.classify_map_triggers(&rules);
    sim.attach_object_trigger_tag(first, Some("ONE"));
    sim.attach_object_trigger_tag(second, Some("TWO"));
    sim.attach_object_trigger_tag(refused, Some("ONE"));
    let _ = sim.drain_trigger_effects();
    let command = Command::Select {
        entity_ids: vec![second, refused, second, first],
        additive: false,
    };
    assert!(sim.apply_command("Americans", &command, Some(&rules)));
    assert_eq!(
        sim.drain_trigger_effects(),
        vec![
            crate::sim::trigger_runtime::TriggerEffect::LocalizedMessage {
                csf_label: "MISSION:Second".to_string(),
            },
            crate::sim::trigger_runtime::TriggerEffect::LocalizedMessage {
                csf_label: "MISSION:First".to_string(),
            },
        ],
        "native6FBFCE refuses the event after failed ObjectSelect; duplicates do not select twice",
    );
    assert!(
        !sim.substrate
            .entities
            .get(refused)
            .unwrap()
            .seldom
            .selection
            .is_selected()
    );
    assert!(sim.apply_command("Americans", &command, Some(&rules)));
    assert!(
        sim.drain_trigger_effects().is_empty(),
        "retained members do not run Select again"
    );
    assert!(sim.apply_command(
        "Americans",
        &Command::Select {
            entity_ids: Vec::new(),
            additive: false
        },
        Some(&rules),
    ));
    assert!(sim.apply_command("Americans", &command, Some(&rules)));
    assert_eq!(
        sim.drain_trigger_effects().len(),
        2,
        "a fresh selection raises the repeat2 Tags again"
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
    for accelerates in [false, true] {
        let rules = drive_fraction_writer_rules(accelerates);
        let mut sim = Simulation::new();
        let grid = crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        let entity_id = sim
            .spawn_object("DRIVE", "Americans", 12, 13, 64, &rules)
            .expect("spawn Drive vehicle");
        {
            let entity = sim
                .substrate
                .entities
                .get_mut(entity_id)
                .expect("Drive vehicle remains live");
            assert_eq!(
                entity
                    .locomotor
                    .as_ref()
                    .map(|locomotor| locomotor.active_kind()),
                Some(LocomotorKind::Drive),
            );
            let drive = entity.locomotor.as_mut().unwrap();
            assert!(drive.ensure_installed_track_state());
            assert!(drive.store_track_target_fraction(
                crate::sim::movement::track_process::TrackFamily::Drive,
                SimFixed::lit("0.4")
            ));
            entity.foot_speed.set_speed_fraction(SimFixed::lit("0.25"));
        }

        assert!(sim.apply_command(
            "Americans",
            &move_command(entity_id, (17, 13)),
            Some(&rules),
        ));

        {
            let entity = sim
                .substrate
                .entities
                .get(entity_id)
                .expect("Drive vehicle remains live");
            let drive = retained_drive(entity).expect("drive state");
            assert_eq!(drive.target_speed_fraction(), SimFixed::lit("0.4"));
            assert_eq!(entity.foot_speed.applied_fraction(), SimFixed::lit("0.25"));
        }

        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 100);

        let entity = sim
            .substrate
            .entities
            .get(entity_id)
            .expect("Drive vehicle remains live");
        let drive = retained_drive(entity).expect("drive state");
        let expected_current = if accelerates {
            SimFixed::lit("0.28")
        } else {
            SIM_ONE
        };
        assert_eq!(drive.target_speed_fraction(), SIM_ONE);
        assert_eq!(entity.foot_speed.applied_fraction(), expected_current);
        let raw_stage = crate::util::fixed_math::ra2_speed_to_leptons_per_frame(
            rules.object("DRIVE").unwrap().speed,
        );
        let expected_owner = (SimFixed::from_num(raw_stage) * expected_current).to_num::<i32>();
        assert_eq!(
            sim.current_speed_for_test(entity_id, &rules),
            expected_owner
        );
    }
}

#[test]
fn gsi_04_05_stop_preserves_committed_drive_until_reserved_head_finishes() {
    use crate::sim::occupancy::VEHICLE_OCCUPATION_BIT;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nSpeed=6\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    sim.spawn_from_map(
        &[MapEntity {
            cell_x: 4,
            cell_y: 4,
            ..make_test_entity("MTNK", EntityCategory::Unit)
        }],
        Some(&rules),
    );
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .install_drive_state_for_test(Some(Default::default()))
        );
        entity.body_facing.snap(0x4000, 0);
    }

    install_rectangular_test_map(&mut sim, 16, 16);
    sim.resolved_terrain = Some(gsi_04_10_clear_terrain(16, 16));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let grid = (*sim.path_grid_snapshot().unwrap()).clone();
    assert!(sim.set_unit_destination(
        1,
        crate::sim::components::NavTargetRef::cell(8, 4),
        &rules,
        true,
        &mut crate::sim::world::FrameEffects::default(),
    ));
    // An accepted order only installs NavCom/route. The first production
    // Process owns selection, the retained head, and its occupation claim.
    sim.process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
        .expect("Drive Process must commit the first segment before Stop");
    let committed_track = retained_drive(sim.substrate.entities.get(1).unwrap())
        .unwrap()
        .track();
    assert!(committed_track.turn_index() >= 0);
    let committed_head = retained_drive(sim.substrate.entities.get(1).unwrap())
        .and_then(|drive| drive.head_to())
        .map(|head| ((head.x / 256) as u16, (head.y / 256) as u16))
        .expect("first Drive step has a committed head");
    assert_eq!(committed_head, (5, 4));
    assert_eq!(
        crate::sim::movement::movement_goal_cell(sim.substrate.entities.get(1).unwrap()),
        Some((8, 4))
    );

    assert!(sim.apply_command("Americans", &Command::Stop { entity_id: 1 }, Some(&rules)));
    let stopped = sim.substrate.entities.get(1).unwrap();
    assert_eq!(stopped.navigation.nav_com(), None);
    assert!(crate::sim::movement::motion_query::is_moving(&stopped) == Some(true));
    // The route is cut at the committed head: Stop retires every Foot+5E0
    // word beyond it and the head stays the committed track head.
    assert!(
        stopped
            .navigation
            .path_replay()
            .remaining_directions()
            .is_empty()
    );
    let head = crate::sim::movement::track_head::committed_track_head(stopped)
        .expect("Stop keeps the committed head");
    assert_eq!(
        ((head.x / 256) as u16, (head.y / 256) as u16),
        committed_head
    );
    assert_eq!(
        crate::sim::movement::movement_goal_cell(stopped),
        Some(committed_head)
    );
    let drive = retained_drive(stopped).unwrap();
    assert_eq!(
        drive.track(),
        committed_track,
        "Stop preserves the active track"
    );
    assert!(drive.head_to().is_some());
    assert!(sim.substrate.occupancy.contains_entity(4, 4, 1));
    assert!(
        !sim.substrate
            .occupancy
            .contains_entity(committed_head.0, committed_head.1, 1)
    );

    let initial_point_index = retained_drive(stopped).unwrap().track().cursor();
    let mut cursor_advanced = false;
    for _ in 0..32 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 33);
        cursor_advanced = retained_drive(sim.substrate.entities.get(1).unwrap())
            .is_some_and(|drive| drive.track().cursor() > initial_point_index);
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
            .contains_entity(committed_head.0, committed_head.1, 1)
    );
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(4, 4) & VEHICLE_OCCUPATION_BIT,
        0,
        "the first paid post-Stop point clears current occupation without stranding the track"
    );

    for _ in 0..192 {
        if crate::sim::movement::motion_query::is_moving(sim.substrate.entities.get(1).unwrap())
            != Some(true)
        {
            break;
        }
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 33);
    }

    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!((entity.position.rx(), entity.position.ry()), committed_head);
    assert!(crate::sim::movement::motion_query::is_moving(&entity) != Some(true));
    let drive = retained_drive(entity).unwrap();
    assert_eq!(
        (drive.track().turn_index(), drive.track().cursor()),
        (-1, 0)
    );
    assert_eq!(drive.head_to(), None);
    assert!(
        sim.substrate
            .occupancy
            .contains_entity(committed_head.0, committed_head.1, 1)
    );
    assert_eq!(
        sim.substrate
            .raw_cell_occupation
            .ground_bits(committed_head.0, committed_head.1)
            & VEHICLE_OCCUPATION_BIT,
        VEHICLE_OCCUPATION_BIT
    );
    assert!(!sim.substrate.occupancy.contains_entity(6, 4, 1));
    assert!(!sim.substrate.occupancy.contains_entity(8, 4, 1));
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(6, 4) & VEHICLE_OCCUPATION_BIT,
        0
    );
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(8, 4) & VEHICLE_OCCUPATION_BIT,
        0
    );

    for _ in 0..32 {
        let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 33);
    }
    let parked = sim.substrate.entities.get(1).unwrap();
    assert_eq!(
        (parked.position.rx(), parked.position.ry()),
        committed_head,
        "Stop must remain parked at the committed head after the old route is gone"
    );
    assert!(crate::sim::movement::motion_query::is_moving(&parked) != Some(true));
    let drive = retained_drive(parked).unwrap();
    assert_eq!(drive.head_to(), None);
    assert!(!sim.substrate.occupancy.contains_entity(6, 4, 1));
    assert_eq!(
        sim.substrate.raw_cell_occupation.ground_bits(6, 4) & VEHICLE_OCCUPATION_BIT,
        0
    );
}

#[test]
fn gsi_13_06_stop_preserves_committed_ship_segment_and_speed_state() {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=DLPH\n[DLPH]\nSpeed=6\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    sim.spawn_from_map(
        &[MapEntity {
            cell_x: 4,
            cell_y: 4,
            ..make_test_entity("DLPH", EntityCategory::Unit)
        }],
        Some(&rules),
    );
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Ship));
        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .install_ship_state_for_test(Some(ShipLocomotionRuntime::default()))
        );
        entity.body_facing.snap(0x4000, 0);
    }

    install_rectangular_test_map(&mut sim, 16, 16);
    sim.resolved_terrain = Some(water_terrain(16, 16));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    let grid = (*sim.path_grid_snapshot().unwrap()).clone();
    assert!(sim.set_unit_destination(
        1,
        crate::sim::components::NavTargetRef::cell(8, 4),
        &rules,
        true,
        &mut crate::sim::world::FrameEffects::default(),
    ));
    sim.process_ground_locomotor_for_test(1, Some(&rules), Some(&grid), None)
        .expect("Ship Process must commit the first segment before Stop");
    let (committed_head, committed_track) = {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        let ship = entity.locomotor.as_mut().unwrap();
        assert!(
            ship.track_progress(crate::sim::movement::track_process::TrackFamily::Ship)
                .unwrap()
                .turn_index()
                >= 0
        );
        assert!(ship.store_track_target_fraction(
            crate::sim::movement::track_process::TrackFamily::Ship,
            SIM_ONE
        ));
        entity.foot_speed.set_speed_fraction(SIM_HALF);
        (
            ship.track_head(crate::sim::movement::track_process::TrackFamily::Ship)
                .expect("Ship curve has a committed head"),
            ship.track_progress(crate::sim::movement::track_process::TrackFamily::Ship)
                .unwrap(),
        )
    };
    let committed_cell = (
        u16::try_from(committed_head.x.div_euclid(256)).unwrap(),
        u16::try_from(committed_head.y.div_euclid(256)).unwrap(),
    );

    assert!(sim.apply_command("Americans", &Command::Stop { entity_id: 1 }, Some(&rules)));

    let stopped = sim.substrate.entities.get(1).unwrap();
    assert!(
        crate::sim::movement::motion_query::is_moving(&stopped) == Some(true),
        "committed Ship segment survives Stop"
    );
    // Stop retires every Foot+5E0 word beyond the committed head; the head
    // itself is pinned below.
    assert!(
        stopped
            .navigation
            .path_replay()
            .remaining_directions()
            .is_empty()
    );
    assert_eq!(
        crate::sim::movement::movement_goal_cell(stopped),
        Some(committed_cell)
    );
    let ship = stopped
        .locomotor
        .as_ref()
        .and_then(|l| l.selected_ship_runtime())
        .and_then(|r| r.retained())
        .expect("Ship runtime");
    assert_eq!(
        ship.track(),
        committed_track,
        "Stop preserves the active track"
    );
    assert_eq!(ship.destination(), None);
    assert_eq!(ship.head_to(), Some(committed_head));
    assert_eq!(ship.target_speed_fraction(), SimFixed::lit("0.3"));
    assert_eq!(stopped.foot_speed.applied_fraction(), SIM_HALF);
}

#[test]
fn move_event_issuer_does_not_replace_actor_owner() {
    let rules = driving_combat_test_rules();
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
            veterancy: Some(0),
            high: false,
            mission: None,
            recruitable_a: true,
            recruitable_b: true,
            group: -1,
            follows: None,
            structure_columns: Default::default(),
            structure_ai_sellable: false,
            structure_ai_repairable: false,
            structure_online: true,
            trigger_tag: None,
        }],
        Some(&rules),
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
        },
    );

    let _ = sim.advance_tick(&[cmd], Some(&rules), Some(&grid), None, 33);
    assert!(
        sim.substrate
            .entities
            .get(1)
            .is_some_and(|e| e.navigation.nav_com()
                == Some(crate::sim::components::NavTargetRef::cell(8, 2))),
        "Event4C71CA resolves the actor independently of the issuer"
    );
    assert_eq!(
        sim.interner
            .resolve(sim.substrate.entities.get(1).unwrap().owner()),
        "Americans"
    );
}

fn legionnaire_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=CLEG\n\n[VehicleTypes]\n\n[AircraftTypes]\n\n[BuildingTypes]\n\n\
         [CLEG]\nStrength=125\nSpeed=4\nTeleporter=yes\nSpeedType=Foot\nMovementZone=Infantry\n\
         Locomotor={4A582747-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .expect("legionnaire rules")
}

/// A Chrono Legionnaire's move order runs the Infantry setter
/// (`0x0051AA40`), whose Foot tail writes NavCom and calls
/// `TeleportLocomotionClass::Move_To @ 0x00718100`. Move_To refuses a
/// timer-locked owner (Foot+0x6A0, `vt+0x380`) or one still warping in
/// (`vt+0x1D8`) with a raw NavCom clear (`0x0071820F`); the warp's arrival
/// assigns the NULL destination (`0x0071973C`). The order entry this replaced
/// armed a new warp anyway.
#[test]
fn a_paralyzed_or_warping_in_teleport_infantryman_refuses_a_move_order() {
    let rules = legionnaire_rules();
    let grid = PathGrid::new(32, 32);
    let tick = |sim: &mut Simulation, commands: &[Command]| {
        let envelopes: Vec<_> = commands
            .iter()
            .map(|command| cmd_envelope(sim, "Americans", 1, command.clone()))
            .collect();
        let _ = sim.advance_tick(&envelopes, Some(&rules), Some(&grid), None, 33);
    };
    let order = |id: u64, rx: u16| move_command(id, (rx, 2));
    let spawn = |sim: &mut Simulation| {
        // Infantry718B70/51BF90 require allocated cells, a passable Foot row
        // and a usable playfield; the command fixture supplies clear ground.
        install_rectangular_test_map(sim, 32, 32);
        sim.install_resolved_terrain_for_new_map(gsi_04_10_clear_terrain(32, 32));
        sim.spawn_object("CLEG", "Americans", 2, 2, 64, &rules)
            .expect("spawn legionnaire")
    };
    let cell = |rx: u16| Some(crate::sim::components::NavTargetRef::cell(rx, 2));

    // The order runs in the frame's event tail; Process warps next frame.
    let mut sim: Simulation = Simulation::new();
    let id = spawn(&mut sim);
    tick(&mut sim, &[order(id, 8)]);
    let entity = sim.substrate.entities.get(id).unwrap();
    assert_eq!(entity.navigation.nav_com(), cell(8));
    assert!(entity.teleport_state().is_some());
    tick(&mut sim, &[]);
    let entity = sim.substrate.entities.get(id).unwrap();
    assert_eq!((entity.position.rx(), entity.position.ry()), (8, 2));
    assert!(entity.is_warping_in());
    assert_eq!(entity.navigation.nav_com(), None);

    // Ordered again while warping in: NavCom is written, then cleared.
    tick(&mut sim, &[order(id, 12)]);
    tick(&mut sim, &[]);
    let entity = sim.substrate.entities.get(id).unwrap();
    assert_eq!((entity.position.rx(), entity.position.ry()), (8, 2));
    assert_eq!(entity.navigation.nav_com(), None);
    assert!(entity.is_warping_in());

    // Stop reaches Teleport Stop_Moving (`0x00718230`), which keeps the
    // owner's warp-in.
    tick(&mut sim, &[Command::Stop { entity_id: id }]);
    assert!(sim.substrate.entities.get(id).unwrap().is_warping_in());

    // A paralyzed one never warps.
    let mut sim: Simulation = Simulation::new();
    let id = spawn(&mut sim);
    let frame = sim.session.binary_frame() as i32;
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .parasite
        .set_paralysis_for_test(crate::sim::timer::CdTimer::started(frame, 100));
    tick(&mut sim, &[order(id, 8)]);
    tick(&mut sim, &[]);
    let entity = sim.substrate.entities.get(id).unwrap();
    assert_eq!((entity.position.rx(), entity.position.ry()), (2, 2));
    assert!(entity.teleport_state().is_none());
    assert_eq!(entity.navigation.nav_com(), None);
}

#[test]
fn test_undeploy_conyard_spawns_mcv() {
    let mut sim = Simulation::new();
    let mut rules = combat_test_rules();
    // Retail GACNSTMK: 58 frames with shadows.
    rules.set_buildup_control_for_test("GACNST", [0, 29, 1]);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    // A yard converts back only in a multiplayer game (`Mission_Selling 0x00449D08`).
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    insert_house_with_counts(&mut sim, "Americans", 0, 0);

    // First deploy an MCV to get a ConYard.
    let mcv = sim
        .spawn_object("AMCV", "Americans", 20, 22, 128, &rules)
        .expect("spawn MCV");
    if let Some(e) = sim.substrate.entities.get_mut(mcv) {
        e.seldom.selection.set_selected_for_test(true);
    }
    let deploy_cmd = cmd_envelope(&sim, "Americans", 1, Command::DeployMcv { entity_id: mcv });
    let _ = sim.advance_tick(&[deploy_cmd], Some(&rules), None, None, 33);
    let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);

    // Find the ConYard that was spawned.
    let yard_id: u64 = sim
        .substrate
        .entities
        .values()
        .find(|e| sim.interner.resolve(e.type_ref()) == "GACNST")
        .map(|e| e.stable_id())
        .expect("ConYard should exist after deploy");

    // Clear building_up so we can undeploy (can't undeploy during construction).
    if let Some(e) = sim.substrate.entities.get_mut(yard_id) {
        e.finish_building_construction_for_test();
        e.seldom.selection.set_selected_for_test(true);
    }

    // Undeploy the ConYard: Sell's stage 0 and stage 1 visits, then the
    // build-up played in reverse from stage 1's Begin_Mode(0).
    let undeploy_cmd = cmd_envelope(
        &sim,
        "Americans",
        sim.session.tick() + 1,
        Command::UndeployBuilding { entity_id: yard_id },
    );
    let undeploy_frame = sim.session.binary_frame();
    let _ = sim.advance_tick(&[undeploy_cmd], Some(&rules), None, None, 33);

    // ConYard should still exist but have building_down set.
    assert!(
        sim.substrate.entities.get(yard_id).is_some(),
        "ConYard should still exist during undeploy animation"
    );
    assert!(
        sim.substrate.entities.get(yard_id).unwrap().building_down(),
        "ConYard should have building_down component"
    );

    // The player's order stands for the retail cell click, so the pack-up
    // runs to the last frame (no archive-less stage-0x17 exit): the stage-2
    // visit two frames plus 28 steps after the command frame converts it.
    let mut converted = None;
    for _ in 0..40 {
        let frame = sim.session.binary_frame();
        let _ = sim.advance_tick(&[], Some(&rules), None, None, 33);
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
        .filter(|e| e.type_ref() == amcv_id)
        .map(|e| {
            (
                e.position.rx(),
                e.position.ry(),
                e.seldom.selection.is_selected(),
            )
        })
        .collect();
    assert_eq!(mcvs.len(), 1, "Exactly one MCV should exist after undeploy");
    let (rx, ry, selected) = mcvs[0];
    // Deploy put the origin at (19, 21) from an MCV standing on (20, 22), so
    // undeploy has to hand (20, 22) back.
    assert_eq!(rx, 20, "MCV should return to the cell it deployed from, X");
    assert_eq!(ry, 22, "MCV should return to the cell it deployed from, Y");
    assert!(selected, "MCV should inherit selection from ConYard");
}
