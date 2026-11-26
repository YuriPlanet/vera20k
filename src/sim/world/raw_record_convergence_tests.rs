//! Native command records converge with semantic execution through the
//! simulation's own decoder, command queues and execution host.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::tests::{gsi_04_07_wall_sell_rules, gsi_04_07_wall_sell_seed_houses};
use crate::sim::world::{FrameEffects, Simulation, TickLane};

#[test]
fn gsi_04_07_wall_sell_raw_lockstep_replay_converges_with_semantic_execution() {
    fn seeded_world() -> (
        Simulation,
        RuleSet,
        crate::rules::overlay_types::OverlayTypeRegistry,
        crate::sim::intern::InternedId,
    ) {
        let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
        let mut sim = Simulation::new();
        let (wall_owner, receiver) = gsi_04_07_wall_sell_seed_houses(&mut sim);
        sim.houses
            .get_mut(&receiver)
            .unwrap()
            .set_player_control_for_test(true);
        let mut grid = crate::sim::overlay_grid::OverlayGrid::new(3, 3);
        grid.place_owned_wall(1, 1, 2, 0, wall_owner);
        *sim.overlay_grid = Some(grid);
        (sim, rules, overlays, receiver)
    }

    let (mut semantic, semantic_rules, semantic_overlays, semantic_receiver) = seeded_world();
    semantic.advance_tick(
        &[CommandEnvelope::new(
            semantic_receiver,
            0,
            Command::SellWallAtCell { x: 1, y: 1 },
        )],
        Some(&semantic_rules),
        None,
        Some(&semantic_overlays),
        67,
    );
    assert_eq!(
        semantic
            .overlay_grid
            .as_ref()
            .unwrap()
            .cell(1, 1)
            .overlay_id,
        None
    );

    let (mut raw, raw_rules, raw_overlays, raw_receiver) = seeded_world();
    let issued = raw
        .encode_sell_wall_at_cell_record(raw_receiver, 1, 1)
        .expect("registered receiver encodes");
    let replay = crate::sim::replay::NativeReplay {
        header: crate::sim::replay::NativeReplayHeader::new(1, "wall.map"),
        frames: vec![crate::sim::replay::NativeReplayFrame::record(
            crate::sim::replay::NativeReplayPresentation::new([0; 8], Vec::new(), [0; 2]),
            0,
            true,
            [&issued],
        )],
    };
    let replay_bytes = replay.encode().expect("native replay bytes");
    let decoded =
        crate::sim::replay::NativeReplay::decode_with_command_schedule(&replay_bytes, |_, _| true)
            .expect("native replay decode");
    let replay_record = decoded.frames[0].commands.as_ref().unwrap()[0].clone();

    raw.advance_tick(
        &[CommandEnvelope::synchronized(
            replay_record,
            raw.session.tick(),
        )],
        Some(&raw_rules),
        None,
        Some(&raw_overlays),
        67,
    );
    assert_eq!(
        raw.overlay_grid.as_ref().unwrap().cell(1, 1).overlay_id,
        None
    );
    assert_eq!(raw.state_hash(), semantic.state_hash());

    let invalid_house = crate::sim::command::SellWallAtCellRecord {
        house_id: 9,
        frame: 0,
        x: 1,
        y: 1,
    }
    .encode()
    .unwrap();
    assert!(
        raw.decode_native_command_record(&invalid_house, raw.session.tick())
            .is_none()
    );
    let invalid_opcode =
        crate::sim::command::CommandRecord::encode(0x16, 1, 0, &[1, 0, 1, 0]).unwrap();
    assert!(
        raw.decode_native_command_record(&invalid_opcode, raw.session.tick())
            .is_none()
    );
}

#[test]
fn wall_sale_respects_the_admitted_player_control_rule_in_skirmish() {
    use crate::sim::scenario_session::ScenarioGameMode;

    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    let (wall_owner, receiver) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(3, 3);
    grid.place_owned_wall(1, 1, 2, 0, wall_owner);
    *sim.overlay_grid = Some(grid);

    let mode = ScenarioGameMode::Skirmish;
    let stock = mode.stock_rule_policy(None);
    sim.install_session_admission(mode, None, stock);
    let sell = |sim: &mut Simulation| {
        sim.sell_wall_at_cell(
            receiver.into(),
            None,
            1,
            1,
            false,
            &rules,
            &overlays,
            &mut crate::sim::world::FrameEffects::empty(),
        )
    };
    assert!(!sell(&mut sim));
    sim.install_session_admission(mode, None, stock.with_counts_player_control_as_human(true));
    assert!(sell(&mut sim));
}

#[test]
fn gsi_04_07_wall_sell_raw_signed_linear_coordinates_use_canonical_cell() {
    fn apply_raw(
        sim: &mut Simulation,
        rules: &RuleSet,
        overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
        x: i16,
        y: i16,
    ) -> bool {
        let record = crate::sim::command::SellWallAtCellRecord {
            house_id: 1,
            frame: sim.session.binary_frame(),
            x,
            y,
        }
        .encode()
        .expect("wall-sale record");
        let envelope = sim
            .decode_native_command_record(&record, sim.session.tick())
            .expect("registered receiver decodes");
        let receiver = sim.interner.resolve(envelope.owner).to_string();
        (sim.apply_command_with_overlays(
            crate::sim::command::CommandSender::Player(None),
            &receiver,
            &envelope.payload,
            rules,
            Some(overlays),
            &mut crate::sim::world::FrameEffects::default(),
        )).is_ok()
    }

    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    let (wall_owner, _) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(512, 2);
    for cell in [(1, 1), (511, 0), (0, 0), (2, 1)] {
        grid.place_owned_wall(cell.0, cell.1, 2, 0, wall_owner);
    }
    *sim.overlay_grid = Some(grid);

    assert!(apply_raw(&mut sim, &rules, &overlays, 513, 0));
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(1, 1).overlay_id,
        None,
        "linear index 513 canonicalizes to cell (1,1)"
    );

    assert!(apply_raw(&mut sim, &rules, &overlays, -1, 1));
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(511, 0).overlay_id,
        None,
        "a negative component remains valid when its signed linear index is 511"
    );

    assert!(apply_raw(&mut sim, &rules, &overlays, 512, -1));
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(0, 0).overlay_id,
        None,
        "only original packed (0,0), not an alias to it, is the null sentinel"
    );

    assert!(!apply_raw(&mut sim, &rules, &overlays, -1, 0));
    assert!(!apply_raw(&mut sim, &rules, &overlays, 0, 512));
    assert_eq!(
        sim.overlay_grid.as_ref().unwrap().cell(2, 1).overlay_id,
        Some(2),
        "negative and above-0x3ffff linear indices leave the allocated grid untouched"
    );
}

/// Two LAN-route infantry of `Local` and one of `Enemy` on a 64x64 clear map:
/// the actor, a guard and the enemy, in that order.
fn combat_scene() -> (Simulation, RuleSet, u64, u64, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n\
         [E1]\nStrength=125\nArmor=none\nSpeed=4\nSight=8\nPrimary=Rifle\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n\
         [Rifle]\nDamage=15\nROF=20\nRange=4\nProjectile=Bullet\nWarhead=SA\n\
         [Bullet]\nAG=yes\nAA=no\n\
         [SA]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [Clear]\nFoot=100%\nTrack=100%\nWheel=100%\nFloat=0%\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(0x0064_dab0);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    for (index, (name, human)) in [("Local", true), ("Enemy", false)].into_iter().enumerate() {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner,
                index as u8,
                None,
                human,
                0,
                10,
            ));
    }
    sim.session
        .set_current_house_for_test(sim.interner.get("Local"));
    let costs = rules
        .terrain_rules
        .semantics_for_land_type(crate::rules::terrain_rules::LandType::Clear.as_index())
        .unwrap()
        .speed_costs;
    let cells = (0..64)
        .flat_map(|y| {
            (0..64).map(move |x| {
                let mut cell = crate::map::resolved_terrain::ResolvedTerrainCell {
                    filled_clear: true,
                    accepts_smudge: true,
                    ..crate::map::resolved_terrain::test_flat_cell(x, y)
                };
                cell.speed_costs = costs;
                cell.base_speed_costs = costs;
                cell
            })
        })
        .collect();
    sim.install_resolved_terrain_for_new_map(
        crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(64, 64, cells),
    );
    assert!(sim.rebuild_dynamic_navigation(&rules));
    sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 16,
        off_fc: -64,
        off_100: -64,
        off_104: 128,
        off_108: 128,
    }));
    sim.set_playfield_size_height_for_test(Some(64));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let actor = sim.spawn_object("E1", "Local", 10, 10, 0, &rules).unwrap();
    let guard = sim.spawn_object("E1", "Local", 10, 12, 0, &rules).unwrap();
    let enemy = sim.spawn_object("E1", "Enemy", 18, 10, 0, &rules).unwrap();
    // A LAN session takes the multiplayer Queue_AI route. As a whole
    // network session it has admitted timing, and ShortGame is off so these
    // building-less houses are not blown up before the first order.
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Lan);
    sim.session.install_network_timing(10);
    sim.session.game_options_mut_for_test().short_game = false;
    (sim, rules, actor, guard, enemy)
}

fn checksum_frame(sim: &mut Simulation, rules: &RuleSet, commands: &[CommandEnvelope]) -> u32 {
    let sync = sim
        .advance_app_frame(
            commands,
            rules,
            67,
            TickLane::Ordinary,
            &mut FrameEffects::default(),
        )
        .expect("scene frame completes")
        .multiplayer_checksum
        .expect("multiplayer route checksums every frame");
    sync.checksum.expect("scene registries are complete").value
}

/// Stage 0 determinism guard: two multiplayer-route instances of one combat
/// scene, one fed the live semantic commands and the other the same orders as
/// locally issued native records, store identical retail sync checksums every
/// frame; one extra Scenario RNG draw is then caught.
#[test]
fn two_instance_multiplayer_checksums_match_every_frame_and_catch_a_desync() {
    use crate::sim::combat::TargetKind;
    use crate::sim::command_queue::MultiplayerChecksumHistory;

    const FRAMES: u32 = 150;

    let (mut live, rules, actor, guard, enemy) = combat_scene();
    let (mut raw, _, _, _, _) = combat_scene();
    assert_eq!(live.state_hash(), raw.state_hash());
    let local = live.interner.get("Local").unwrap();
    let orders = |frame: u32| match frame {
        2 => Some(Command::Move {
            entity_id: actor,
            target_rx: 15,
            target_ry: 10,
        }),
        20 => Some(Command::Guard {
            entity_id: guard,
            target: Some(TargetKind::Cell(10, 12)),
        }),
        40 => Some(Command::Scatter { entity_id: guard }),
        _ => None,
    };

    let mut live_history = MultiplayerChecksumHistory::new();
    let mut raw_history = MultiplayerChecksumHistory::new();
    for _ in 0..FRAMES {
        let native_frame = live.session.binary_frame();
        assert_eq!(raw.session.binary_frame(), native_frame);
        let execute_tick = live.session.tick() + 1;
        let mut live_commands = Vec::new();
        if let Some(order) = orders(native_frame) {
            let record = match order {
                Command::Scatter { entity_id } => raw.encode_scatter_record(local, entity_id),
                ref megamission => raw.encode_megamission_record(local, megamission),
            }
            .expect("the scene's orders have native records");
            let semantic = CommandEnvelope::new(local, execute_tick, order);
            assert_eq!(
                raw.decode_native_command_record(&record, execute_tick)
                    .as_ref(),
                Some(&semantic),
                "frame {native_frame}"
            );
            assert!(
                raw.issue_local_command(CommandEnvelope::synchronized(record, raw.session.tick()))
            );
            live_commands.push(semantic);
        }

        let live_checksum = checksum_frame(&mut live, &rules, &live_commands);
        let raw_checksum = checksum_frame(&mut raw, &rules, &[]);
        live_history.record(native_frame, live_checksum);
        raw_history.record(native_frame, raw_checksum);
        assert_eq!(
            live_history.get(native_frame),
            raw_history.get(native_frame),
            "instances diverged at native frame {native_frame}"
        );
    }
    assert_eq!(live.state_hash(), raw.state_hash());
    let moved = &live.substrate.entities.get(actor).unwrap().position;
    assert_ne!(
        (moved.rx(), moved.ry()),
        (10, 10),
        "the Move order executed"
    );
    assert!(
        live.substrate
            .entities
            .get(enemy)
            .is_none_or(|enemy| enemy.health.current() < 125),
        "the scene exchanged fire"
    );

    // One extra Scenario RNG draw on one instance is a desync the next frame's
    // stored checksum exposes.
    let _ = raw.scenario_rng.next_u32();
    let native_frame = live.session.binary_frame();
    live_history.record(native_frame, checksum_frame(&mut live, &rules, &[]));
    raw_history.record(native_frame, checksum_frame(&mut raw, &rules, &[]));
    assert_ne!(
        live_history.get(native_frame),
        raw_history.get(native_frame)
    );
}

/// A peer's order may name any signed cell word (a MegaMission token is
/// `x + y * 1000`, so x stays within 999). Each Cell token (Move,
/// Ctrl-attack, attack-move, Area Guard, rally) decodes bit for bit and
/// resolves an off-map cell to the shared fallback cell, as
/// `MapClass::Get_CellClass` does, and the frames that follow run.
#[test]
fn a_peer_order_onto_a_cell_off_the_map_is_executed_and_play_goes_on() {
    use crate::sim::combat::TargetKind;
    use crate::sim::command::{MegaMissionOrder, MegaMissionRecord, MegaMissionTarget};

    let (mut sim, rules, actor, guard, enemy) = combat_scene();
    let local = sim.interner.get("Local").unwrap();
    let decoded = |sim: &Simulation, record| {
        sim.decode_native_command_record(&record, sim.session.tick() + 1)
            .map(|envelope| envelope.payload)
    };
    let mut move_record = sim
        .encode_megamission_record(
            local,
            &Command::Guard {
                entity_id: actor,
                target: Some(TargetKind::Cell(1, 1)),
            },
        )
        .unwrap();
    let mut typed = MegaMissionRecord::decode(&move_record).unwrap();
    typed.order = MegaMissionOrder::Move {
        target_x: -1,
        target_y: i16::MIN,
    };
    typed.write_into(&mut move_record).unwrap();
    assert_eq!(
        decoded(&sim, move_record.clone()),
        Some(Command::Move {
            entity_id: actor,
            target_rx: 0xFFFF,
            target_ry: 0x8000,
        })
    );
    let orders = [
        move_record,
        sim.encode_megamission_record(
            local,
            &Command::ForceAttackCell {
                attacker_id: guard,
                target_rx: 999,
                target_ry: 0x7FFF,
            },
        )
        .unwrap(),
        sim.encode_megamission_record(
            local,
            &Command::AttackMove {
                entity_id: enemy,
                target: None,
                destination: Some(TargetKind::Cell(0xFFFF, 0)),
            },
        )
        .unwrap(),
        sim.encode_archive_record(guard, MegaMissionTarget::Cell { x: -1, y: -1 })
            .unwrap(),
    ];
    for record in orders {
        assert!(decoded(&sim, record.clone()).is_some());
        assert!(sim.issue_local_command(CommandEnvelope::synchronized(record, sim.session.tick())));
        for _ in 0..20 {
            checksum_frame(&mut sim, &rules, &[]);
        }
    }
    let guard_post = Command::Guard {
        entity_id: guard,
        target: Some(TargetKind::Cell(-999i16 as u16, 0x8000)),
    };
    let record = sim.encode_megamission_record(local, &guard_post).unwrap();
    assert!(sim.issue_local_command(CommandEnvelope::synchronized(record, sim.session.tick())));
    for _ in 0..60 {
        checksum_frame(&mut sim, &rules, &[]);
    }
    assert_eq!(
        sim.substrate.entities.get(guard).unwrap().archive_target(),
        Some(TargetKind::Cell(-999i16 as u16, 0x8000)),
        "the Area Guard post off the map is kept"
    );
}

/// A decoded peer Move onto a high cell word re-encodes bit for bit, as the
/// Cell tokens do, so the encoder no longer drops it.
#[test]
fn a_move_onto_a_high_cell_word_round_trips() {
    let (sim, _rules, actor, _guard, _enemy) = combat_scene();
    let local = sim.interner.get("Local").unwrap();
    let order = Command::Move {
        entity_id: actor,
        target_rx: 0xFFFF,
        target_ry: 0x8000,
    };
    let record = sim.encode_megamission_record(local, &order).unwrap();
    assert_eq!(
        sim.decode_native_command_record(&record, sim.session.tick() + 1)
            .map(|envelope| envelope.payload),
        Some(order)
    );
}
