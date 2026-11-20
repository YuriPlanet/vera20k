//! Selection4AE750's ordered object loop4AE829..4AE85A, independent of capability grouping.
use super::*;

#[test]
fn mixed_engineer_c4_batches_restore_original_selection_order() {
    let owner = InternedId::default();
    let wrap = |payload| CommandEnvelope::new(owner, 42, payload);
    let capture = Command::CaptureBuilding {
        engineer_id: 20,
        target_building_id: 100,
    };
    let c4 = Command::PlantC4 {
        attacker_id: 30,
        target_building_id: 100,
    };
    let attack = Command::Attack {
        attacker_id: 10,
        target_id: 100,
    };
    let mut queued = vec![
        wrap(capture.clone()),
        wrap(c4.clone()),
        wrap(attack.clone()),
    ];
    restore_selection_dispatch_order(&mut queued, &[30, 10, 20]);
    assert_eq!(
        queued.iter().map(|e| &e.payload).collect::<Vec<_>>(),
        vec![&c4, &attack, &capture]
    );
    assert!(queued.iter().all(|e| e.execute_tick == 42));
}

#[test]
fn same_actor_orders_keep_their_original_sequence() {
    let owner = InternedId::default();
    let wrap = |payload| CommandEnvelope::new(owner, 42, payload);
    let mut queued = vec![
        wrap(Command::Stop { entity_id: 20 }),
        wrap(Command::Stop { entity_id: 10 }),
        wrap(Command::CaptureBuilding {
            engineer_id: 20,
            target_building_id: 100,
        }),
    ];
    restore_selection_dispatch_order(&mut queued, &[10, 20]);
    assert_eq!(command_actor_id(&queued[0].payload), Some(10));
    assert!(matches!(queued[1].payload, Command::Stop { entity_id: 20 }));
    assert!(matches!(
        queued[2].payload,
        Command::CaptureBuilding {
            engineer_id: 20,
            ..
        }
    ));
}

#[test]
fn prepared_factory_rallies_and_mobile_orders_keep_native_selection_order() {
    let owner = InternedId::default();
    let wrap = |payload| CommandEnvelope::new(owner, 42, payload);
    let mut queued = vec![
        wrap(Command::SetRally {
            rx: 7,
            ry: 8,
            producer_ids: vec![30],
        }),
        wrap(Command::SetRally {
            rx: 9,
            ry: 10,
            producer_ids: vec![20],
        }),
        wrap(Command::SellBuilding { entity_id: 20 }),
        wrap(Command::Move {
            entity_id: 10,
            target_rx: 7,
            target_ry: 8,
        }),
    ];
    restore_selection_dispatch_order(&mut queued, &[30, 10, 20]);
    assert_eq!(
        queued
            .iter()
            .map(|envelope| command_actor_id(&envelope.payload))
            .collect::<Vec<_>>(),
        vec![Some(30), Some(10), Some(20), Some(20)]
    );
    assert!(matches!(
        &queued[2].payload,
        Command::SetRally { rx: 9, ry: 10, .. }
    ));
    assert!(matches!(
        &queued[3].payload,
        Command::SellBuilding { entity_id: 20 }
    ));
}

#[test]
fn aggregated_rally_batch_keeps_existing_dispatch_order() {
    let owner = InternedId::default();
    let mut queued = vec![
        CommandEnvelope::new(
            owner,
            42,
            Command::SetRally {
                rx: 7,
                ry: 8,
                producer_ids: vec![30, 20],
            },
        ),
        CommandEnvelope::new(
            owner,
            42,
            Command::Move {
                entity_id: 10,
                target_rx: 7,
                target_ry: 8,
            },
        ),
    ];
    let original = queued.clone();
    restore_selection_dispatch_order(&mut queued, &[30, 10, 20]);
    assert_eq!(queued, original);
}

#[test]
fn shared_seats_project_context_clicks_and_queue_their_own_native_records() {
    use crate::sim::command::{
        ArchiveRecord, MegaMissionRecord, MegaMissionTarget, ObjectAction, ObjectActionRecord,
    };
    use crate::sim::control_authority::{ControlGrant, ControlGrantLevel};
    use crate::sim::game_options::GameOptions;
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_session::ScenarioGameMode;
    use crate::sim::world::{CellActionQuery, MoveAction, Simulation};

    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
            "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=REFN\n1=POWR\n\
             [MTNK]\nStrength=1000\nSpeed=4\nSight=0\n\
             Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n\
             [REFN]\nStrength=1000\nUndeploysInto=MTNK\n\
             ResourceGatherer=yes\nResourceDestination=yes\n[POWR]\nStrength=1000\nPower=100\n\
             [Clear]\nFoot=100%\nTrack=100%\nWheel=100%\n",
        ))
        .unwrap();
    let mut scene = crate::sim::arena_fixture::rival_arena(&rules, 7);
    scene.intern_rule_type_ids(&rules);
    scene.resolve_type_handles(&rules);
    let primary = scene.interner().get("Americans").unwrap();
    let beta = scene.interner().get("Russians").unwrap();
    let gamma = scene.intern("Gamma");
    scene.register_house(HouseState::new(gamma, 2, None, false, 0, 10));
    scene
        .spawn_object("MTNK", "Americans", 5, 5, 0, &rules)
        .unwrap();
    let beta_tank = scene
        .spawn_object("MTNK", "Russians", 10, 10, 0, &rules)
        .unwrap();
    let gamma_tank = scene
        .spawn_object("MTNK", "Gamma", 10, 15, 0, &rules)
        .unwrap();
    let refinery = scene
        .spawn_object("REFN", "Russians", 8, 8, 0, &rules)
        .unwrap();
    scene
        .spawn_object("POWR", "Russians", 14, 9, 0, &rules)
        .unwrap();
    *scene.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    scene.fog_mut_for_test().reveal_all_for_owner(primary);
    let session = scene.session_mut_for_test();
    session.set_game_mode(ScenarioGameMode::Lan);
    session.set_current_house_for_test(Some(primary));
    session.set_game_options_for_test(GameOptions {
        short_game: false,
        crates: false,
        ..GameOptions::default()
    });
    scene.install_network_timing(10);

    for source in [0, 1] {
        let mut sim = scene.clone();
        // The arena registers Russians first, then Americans.
        sim.session_mut_for_test()
            .install_shared_house_sources_for_test(vec![1, 1], Some(source));
        sim.network_command_queues().install_timing(
            0,
            std::num::NonZeroU8::new(5).unwrap(),
            [0, 1],
        );
        sim.install_control_grants(vec![
            ControlGrant {
                source: 0,
                house: 0,
                level: ControlGrantLevel::FullControl,
            },
            ControlGrant {
                source: 1,
                house: 0,
                level: ControlGrantLevel::UnitsOnly,
            },
            ControlGrant {
                source: 0,
                house: 2,
                level: ControlGrantLevel::UnitsOnly,
            },
        ])
        .unwrap();
        assert!(!sim.owner_is_human(beta));
        assert!(!sim.houses()[&beta].player_control());
        assert_eq!(sim.houses()[&primary].tracking().buildings(), 0);
        assert_eq!(sim.houses()[&beta].tracking().buildings(), 1);
        let has_building = crate::app::input::dispatch::local_has_manageable_building(&sim);
        let mut sidebar = crate::ui::sidebar::gadget_flash::SidebarGadgetState::default();
        for mode in [
            crate::ui::sidebar::gadget_flash::SelectionMode::Repair,
            crate::ui::sidebar::gadget_flash::SelectionMode::Sell,
        ] {
            assert_eq!(
                sidebar.toggle_selection_mode(mode, false, i32::from(has_building)),
                source == 0,
            );
        }
        sim.publish_scenario_started();

        let clicked = (18, 13);
        let cursor = |sim: &Simulation, actor| {
            sim.cell_move_action(
                primary,
                actor,
                clicked,
                CellActionQuery {
                    mouse_over: true,
                    shift: false,
                },
                &rules,
            )
            .unwrap()
        };
        // Warm the projection at the live three-House width, then grow the
        // roster between actual simulation frames.
        assert_eq!(cursor(&sim, beta_tank), MoveAction::Move);
        let old_frame = sim.session().binary_frame();
        let frame = sim.advance_tick(&[], Some(&rules), None, Some(rules.overlay_registry()), 33);
        assert!(frame.frame_committed);
        assert_ne!(sim.session().binary_frame(), old_frame);
        let delta = sim.intern("Delta");
        sim.register_house(HouseState::new(delta, 3, None, false, 0, 10));
        let delta_tank = sim
            .spawn_object("MTNK", "Delta", 10, 20, 0, &rules)
            .unwrap();
        assert_eq!(cursor(&sim, beta_tank), MoveAction::Move);
        assert_eq!(
            cursor(&sim, gamma_tank),
            if source == 0 {
                MoveAction::Move
            } else {
                MoveAction::None
            },
        );
        assert_eq!(cursor(&sim, delta_tank), MoveAction::None);
        let selected = [delta_tank, beta_tank];
        let receivers = projected_order_selection(&sim, Some(&rules), &selected, None);
        assert_eq!(receivers.as_ref(), &[beta_tank]);

        let tick = sim.session().tick();
        let mut orders = [beta_tank, gamma_tank, delta_tank]
            .into_iter()
            .filter_map(|actor| {
                let goal = crate::app::input::commands::ordinary_cell_move_goal(
                    &sim, &rules, primary, actor, clicked, false,
                )?;
                Some(CommandEnvelope::new(
                    primary,
                    tick,
                    move_click_command(
                        &sim,
                        &rules,
                        OrderMode::Move,
                        actor,
                        TargetKind::Cell(goal.0, goal.1),
                    ),
                ))
            })
            .collect::<Vec<_>>();
        // An ordinary refinery repack click requests an archive and SELL.
        // The two orders from the same actor require different capabilities.
        let (building_orders, _) =
            building_cell_orders(&sim, &rules, &[refinery], primary, clicked, false, false);
        orders.extend(
            building_orders
                .into_iter()
                .map(|payload| CommandEnvelope::new(primary, tick, payload)),
        );
        retain_context_orders(&sim, &rules, &mut orders);
        let native = orders
            .into_iter()
            .map(|envelope| {
                crate::app::input::commands::encode_ordinary_local_command(&sim, envelope).unwrap()
            })
            .collect::<Vec<_>>();
        assert!(sim.issue_local_commands(native));
        assert!(
            crate::app::input::commands::queue_house_cheer_in_sim(
                &mut sim,
                &rules,
                &mut crate::app::input::state::MatchInputState::new(Default::default()),
                "Americans",
            )
            .is_some(),
        );

        let outlist = sim.network_command_queues();
        let records = outlist.local_records().unwrap();
        assert_eq!(records.len(), if source == 0 { 5 } else { 3 });
        assert!(records.iter().all(|record| record.house_id() == source));
        assert_eq!(
            records.last().unwrap().opcode(),
            crate::sim::command::CHEER_OPCODE
        );
        let moves: Vec<_> = records
            .iter()
            .filter_map(|record| MegaMissionRecord::decode(record))
            .map(|event| event.source_id as u64)
            .collect();
        assert_eq!(
            moves,
            if source == 0 {
                vec![beta_tank, gamma_tank]
            } else {
                vec![beta_tank]
            }
        );
        let rally: Vec<_> = records
            .iter()
            .filter_map(|record| ArchiveRecord::decode(record))
            .collect();
        assert_eq!(rally.len(), 1);
        assert_eq!(rally[0].entity_id as u64, refinery);
        assert!(matches!(rally[0].target, MegaMissionTarget::Cell { .. }));
        let sales: Vec<_> = records
            .iter()
            .filter_map(|record| ObjectActionRecord::decode(record))
            .map(|record| (record.action, record.entity_id as u64))
            .collect();
        assert_eq!(
            sales,
            if source == 0 {
                vec![(ObjectAction::Sell, refinery)]
            } else {
                Vec::new()
            },
        );
    }
}
