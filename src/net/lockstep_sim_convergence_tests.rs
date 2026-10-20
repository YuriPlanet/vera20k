//! Boundary tests (F14): raw lockstep command records converging with
//! semantic simulation execution.
//!
//! These tests exercise `net::lockstep` admission/dispatch against the full
//! `Simulation`, so they live on the net side of the sim/net boundary —
//! `sim/` itself never names `crate::net`. Moved verbatim from the
//! `sim::world` test module (whose fixtures they still share).

use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::Simulation;
use crate::sim::world::tests::{gsi_04_07_wall_sell_rules, gsi_04_07_wall_sell_seed_houses};

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
        sim.overlay_grid = Some(grid);
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

    let mut queue = crate::net::lockstep::SynchronizedCommandQueue::new();
    assert!(
        queue.admit(crate::net::lockstep::SynchronizedCommand::opaque(
            replay_record
        ))
    );
    let dispatch_houses = raw
        .houses
        .order()
        .iter()
        .copied()
        .enumerate()
        .map(|(index, owner)| {
            crate::net::lockstep::CommandDispatchHouse::new(
                owner,
                index as i8,
                raw.houses[&owner].event_dispatch_eligible(),
            )
        })
        .collect::<Vec<_>>();
    let mut due = Vec::new();
    let early = queue.dispatch_due_offline(
        -1,
        &dispatch_houses,
        |_, _| {},
        |_, command, _| {
            if let Some(envelope) = command.decode_for_simulation(&raw, raw.session.tick()) {
                due.push(envelope);
            }
        },
    );
    assert_eq!(early.executed, 0, "frame -1 cannot execute frame-0 bytes");
    assert!(due.is_empty());
    let due_summary = queue.dispatch_due_offline(
        0,
        &dispatch_houses,
        |_, _| {},
        |_, command, _| {
            if let Some(envelope) = command.decode_for_simulation(&raw, raw.session.tick()) {
                due.push(envelope);
            }
        },
    );
    assert_eq!(due_summary.executed, 1);
    assert_eq!(due.len(), 1);
    raw.advance_tick(&due, Some(&raw_rules), None, Some(&raw_overlays), 67);
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
        crate::net::lockstep::SynchronizedCommand::opaque(invalid_house)
            .decode_for_simulation(&raw, raw.session.tick())
            .is_none()
    );
    let invalid_opcode =
        crate::sim::command::CommandRecord::encode(0x16, 1, 0, &[1, 0, 1, 0]).unwrap();
    assert!(
        crate::net::lockstep::SynchronizedCommand::opaque(invalid_opcode)
            .decode_for_simulation(&raw, raw.session.tick())
            .is_none()
    );
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
        let envelope = crate::net::lockstep::SynchronizedCommand::opaque(record)
            .decode_for_simulation(sim, sim.session.tick())
            .expect("registered receiver decodes");
        let receiver = sim.interner.resolve(envelope.owner).to_string();
        sim.apply_command_with_overlays(
            &receiver,
            &envelope.payload,
            Some(rules),
            Some(overlays),
            crate::sim::world::FrameEffects::default(),
        )
    }

    let (rules, overlays) = gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    let (wall_owner, _) = gsi_04_07_wall_sell_seed_houses(&mut sim);
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(512, 2);
    for cell in [(1, 1), (511, 0), (0, 0), (2, 1)] {
        grid.place_owned_wall(cell.0, cell.1, 2, 0, wall_owner);
    }
    sim.overlay_grid = Some(grid);

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

/// Stage 0 determinism guard: two multiplayer-route instances of one combat
/// scene, one fed the live semantic commands and the other the same orders as
/// native records through the lockstep queue, store identical retail sync
/// checksums every frame; one extra Scenario RNG draw is then caught.
#[test]
fn two_instance_multiplayer_checksums_match_every_frame_and_catch_a_desync() {
    use crate::net::lockstep::{
        CommandDispatchHouse, LockstepScheduler, MultiplayerChecksumHistory, SynchronizedCommand,
        SynchronizedCommandQueue,
    };
    use crate::rules::ini_parser::IniFile;
    use crate::sim::combat::TargetKind;
    use crate::sim::world::{FrameEffects, TickLane};

    const FRAMES: u32 = 150;

    fn scene() -> (Simulation, RuleSet, u64, u64, u64) {
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

    fn frame(sim: &mut Simulation, rules: &RuleSet, commands: &[CommandEnvelope]) -> u32 {
        let sync = sim
            .advance_app_frame(
                commands,
                Some(rules),
                None,
                67,
                TickLane::Ordinary,
                FrameEffects::default(),
            )
            .expect("scene frame completes")
            .multiplayer_checksum
            .expect("multiplayer route checksums every frame");
        sync.checksum.expect("scene registries are complete").value
    }

    let (mut live, rules, actor, guard, enemy) = scene();
    let (mut raw, _, _, _, _) = scene();
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

    let mut queue = SynchronizedCommandQueue::new();
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
            let issued = LockstepScheduler::new()
                .issue(
                    native_frame,
                    i32::from(record.house_id()),
                    record.opcode(),
                    record.payload(),
                )
                .unwrap();
            assert_eq!(issued.record(), &record);
            assert!(queue.issue(issued));
            live_commands.push(CommandEnvelope::new(local, execute_tick, order));
        }
        queue.transfer_single_player();
        let houses = raw
            .houses
            .order()
            .iter()
            .enumerate()
            .map(|(index, owner)| {
                CommandDispatchHouse::new(
                    *owner,
                    index as i8,
                    raw.houses[owner].event_dispatch_eligible(),
                )
            })
            .collect::<Vec<_>>();
        let mut raw_commands = Vec::new();
        queue.dispatch_due_offline(
            native_frame as i32,
            &houses,
            |_, _| {},
            |_, command: &SynchronizedCommand, _| {
                raw_commands.push(
                    command
                        .decode_for_simulation(&raw, execute_tick)
                        .expect("dispatched record decodes"),
                )
            },
        );
        assert_eq!(raw_commands, live_commands, "frame {native_frame}");

        let live_checksum = frame(&mut live, &rules, &live_commands);
        let raw_checksum = frame(&mut raw, &rules, &raw_commands);
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
    live_history.record(native_frame, frame(&mut live, &rules, &[]));
    raw_history.record(native_frame, frame(&mut raw, &rules, &[]));
    assert_ne!(
        live_history.get(native_frame),
        raw_history.get(native_frame)
    );
}
