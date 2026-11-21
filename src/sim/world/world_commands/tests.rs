use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::Health;
use crate::sim::game_entity::GameEntity;

use crate::sim::mission::MissionId;
use crate::sim::movement::locomotor::LocomotorState;

pub(super) fn amcv_move_rules() -> RuleSet {
    let ini = IniFile::from_str(
        "[InfantryTypes]\n\
         [VehicleTypes]\n\
         0=AMCV\n\
         [AircraftTypes]\n\
         [BuildingTypes]\n\
         [AMCV]\n\
         Strength=1000\n\
         Speed=4\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n\
         MovementZone=Normal\n\
         Crusher=yes\n\
         DeploysInto=GACNST\n",
    );
    RuleSet::from_ini(&ini).expect("amcv rules")
}

pub(super) fn spawn_rule_backed_unit(
    sim: &mut Simulation,
    sid: u64,
    type_id: &str,
    rules: &RuleSet,
) {
    let owner = sim.interner.intern("Americans");
    let type_ref = sim.interner.intern(type_id);
    let obj = rules.object(type_id).expect("object type");
    let health = obj.strength;
    let mut entity = GameEntity::new_at_frame_zero_for_test(
        sid,
        20,
        20,
        0,
        0,
        owner,
        Health::new(health),
        type_ref,
        EntityCategory::Unit,
        0,
        obj.sight.max(0) as u16,
        true,
    );
    entity.locomotor = Some(LocomotorState::from_object_type(
        obj,
        sim.session.binary_frame(),
    ));
    entity.mover_facts.set_regular_crusher_for_test(obj.crusher);
    entity
        .mover_facts
        .set_drive_accelerates_for_test(obj.accelerates);
    entity
        .mover_facts
        .set_omni_crusher_for_test(obj.omni_crusher);
    // A directly-inserted GameEntity keeps the constructed `in_limbo`
    // byte; production spawns clear it through Reveal. Order admission
    // reads that byte, so the fixture must model a revealed object.
    entity.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(entity);
}

#[test]
fn delegated_mission_only_unit_cannot_be_selected() {
    use crate::sim::control_authority::{ControlGrant, ControlGrantLevel};
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_session::ScenarioGameMode;

    let rules = amcv_move_rules();
    let mut sim = Simulation::new();
    let local = sim.interner.intern("Local");
    let delegated = sim.interner.intern("Americans");
    sim.register_house(HouseState::new(local, 0, None, true, 0, 1));
    sim.register_house(HouseState::new(delegated, 1, None, false, 0, 1));
    sim.install_session_mode(ScenarioGameMode::Lan);
    sim.session.set_current_house_for_test(Some(local));
    sim.install_control_grants(vec![ControlGrant {
        source: 0,
        house: 1,
        level: ControlGrantLevel::UnitsOnly,
    }])
    .unwrap();
    spawn_rule_backed_unit(&mut sim, 101, "AMCV", &rules);
    assert!(sim.can_select_object(101, &rules));
    sim.substrate
        .entities
        .get_mut(101)
        .unwrap()
        .seldom
        .mission_only
        .mark();
    assert!(!sim.can_select_object(101, &rules));
}

/// Event IDLE's terrain exit (`0x004C752A..0x004C7577`): a harvesting
/// miner on a flat cell drawn under a higher one ignores Stop and keeps
/// Harvest. With that cell flat too, Stop puts it on Guard.
#[test]
fn stop_leaves_a_miner_on_a_hidden_flat_cell_harvesting() {
    use crate::sim::mission::MissionType;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=HARV\n[HARV]\nStrength=600\nSpeed=4\nHarvester=yes\n\
         MovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .expect("HARV rules");
    let harvest = MissionId::from_known(MissionType::Harvest);
    for hidden in [true, false] {
        let mut sim = Simulation::new();
        // The projection probe from (20, 20) meets (21, 21) four levels
        // up, which covers (20, 20) on screen.
        sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(32, 32, |rx, ry| {
            let mut cell = crate::map::resolved_terrain::test_flat_cell(rx, ry);
            if hidden && (rx, ry) == (21, 21) {
                cell.level = 4;
            }
            cell
        }));
        spawn_rule_backed_unit(&mut sim, 42, "HARV", &rules);
        sim.substrate.entities.get_mut(42).unwrap().miner =
            Some(Box::new(crate::sim::miner::Miner::new(
                crate::sim::miner::MinerKind::War,
                &crate::sim::miner::MinerConfig::default(),
                40,
            )));
        sim.substrate
            .mission_assign_exact(42, harvest, 0)
            .expect("miner");
        assert!(sim.apply_command("Americans", &Command::Stop { entity_id: 42 }, Some(&rules)));
        let mission = sim.substrate.entities.get(42).unwrap().mission.current();
        let expected = if hidden {
            harvest
        } else {
            MissionId::from_known(MissionType::Guard)
        };
        assert_eq!(mission, expected, "hidden={hidden}");
    }
}

#[test]
fn lockstep_subject_gate_refuses_an_unrecognized_native_opcode() {
    use crate::sim::scenario_session::ScenarioGameMode;

    let mut sim = Simulation::new();
    sim.session.set_game_mode(ScenarioGameMode::Lan);
    let rules = RuleSet::empty_for_test();
    let command = Command::SynchronizedRecord {
        record: CommandRecord::encode(0xff, 0, 0, &[]).unwrap(),
    };
    assert!(sim.network_command_actor_foreign("Americans", None, &command, rules, None));
}

/// Event IDLE admits its actor on `+0x90` and `+0x81` alone
/// (`0x004C74E8..0x004C74FE`), with no strength test: a live miner at 0
/// strength still stops and goes on Guard. VERA's dying mark, native's
/// UnInit, refuses it.
#[test]
fn stop_admits_a_live_actor_at_zero_strength() {
    use crate::sim::mission::MissionType;
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=HARV\n[HARV]\nStrength=600\nSpeed=4\nHarvester=yes\n\
         MovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .expect("HARV rules");
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
        32,
        32,
        crate::map::resolved_terrain::test_flat_cell,
    ));
    spawn_rule_backed_unit(&mut sim, 42, "HARV", &rules);
    let miner = sim.substrate.entities.get_mut(42).unwrap();
    miner.miner = Some(Box::new(crate::sim::miner::Miner::new(
        crate::sim::miner::MinerKind::War,
        &crate::sim::miner::MinerConfig::default(),
        40,
    )));
    miner.health = crate::sim::combat::Health::new(0);
    sim.substrate
        .mission_assign_exact(42, MissionId::from_known(MissionType::Harvest), 0)
        .expect("miner");
    assert!(sim.apply_command("Americans", &Command::Stop { entity_id: 42 }, Some(&rules)));
    let mission = sim.substrate.entities.get(42).unwrap().mission.current();
    assert_eq!(mission, MissionId::from_known(MissionType::Guard));

    sim.substrate
        .entities
        .get_mut(42)
        .unwrap()
        .death
        .set_dying_for_test(true);
    sim.substrate
        .mission_assign_exact(42, MissionId::from_known(MissionType::Harvest), 0)
        .expect("miner");
    assert!(!sim.apply_command("Americans", &Command::Stop { entity_id: 42 }, Some(&rules)));
    let mission = sim.substrate.entities.get(42).unwrap().mission.current();
    assert_eq!(mission, MissionId::from_known(MissionType::Harvest));
}

#[test]
fn network_match_refuses_orders_for_another_houses_object() {
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_session::ScenarioGameMode;
    let rules = amcv_move_rules();
    let world = |mode| {
        let mut sim = Simulation::new();
        for (index, name) in [(0, "Americans"), (1, "British")] {
            let owner = sim.intern(name);
            sim.houses
                .register(HouseState::new(owner, index, None, true, 0, 10));
        }
        sim.session.set_game_mode(mode);
        spawn_rule_backed_unit(&mut sim, 42, "AMCV", &rules);
        sim
    };
    let move_order = Command::Move {
        entity_id: 42,
        target_rx: 25,
        target_ry: 20,
    };
    let stop = Command::Stop { entity_id: 42 };

    let mut sim = world(ScenarioGameMode::Wol);
    assert!(!sim.apply_command("British", &move_order, Some(&rules)));
    assert!(!sim.apply_command("British", &stop, Some(&rules)));
    let actor = sim.substrate.entities.get(42).unwrap();
    assert_eq!(actor.mission.queued(), MissionId::NONE);
    assert!(actor.navigation.nav_com().is_none());
    assert!(sim.apply_command("Americans", &move_order, Some(&rules)));
    assert!(sim.apply_command("Americans", &stop, Some(&rules)));

    // Offline receivers stay native: MEGAMISSION has no sender gate.
    let mut sim = world(ScenarioGameMode::Skirmish);
    assert!(sim.apply_command("British", &move_order, Some(&rules)));
}

/// DETONATE `0x004C7823` clears its carrier's planning tokens
/// (`0x004C7831`) before it looks for a bomb. Online, a carrier with no
/// bomb answers to its owner: another House's DETONATE leaves its plan,
/// the owner's own still clears it.
#[test]
fn network_detonate_on_a_bombless_carrier_answers_to_its_owner() {
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_session::ScenarioGameMode;
    let rules = amcv_move_rules();
    let mut sim = Simulation::new();
    for (index, name) in [(0, "Americans"), (1, "British")] {
        let owner = sim.intern(name);
        sim.houses
            .register(HouseState::new(owner, index, None, true, 0, 10));
    }
    sim.session.set_game_mode(ScenarioGameMode::Wol);
    spawn_rule_backed_unit(&mut sim, 42, "AMCV", &rules);
    sim.start_empty_planning_path_for_test(42);
    let mut record = CommandRecord::decode_exact(&[0; COMMAND_RECORD_LEN]).unwrap();
    crate::sim::command::DetonateRecord {
        house_id: 1,
        frame: 0,
        entity_id: 42,
    }
    .write_into(&mut record);
    let detonate = Command::SynchronizedRecord { record };

    assert!(!sim.apply_command("British", &detonate, Some(&rules)));
    assert!(sim.has_planning_path_for_test(42));
    assert!(!sim.apply_command("Americans", &detonate, Some(&rules)));
    assert!(!sim.has_planning_path_for_test(42));
}

/// `TechnoClass::Select @ 0x006FBFA0` raises event 33 (`0x006FBFE4`) for
/// a client-local selection. A network game raises none, so the clients
/// keep one trigger state; offline Selects still spring the trigger.
#[test]
fn network_match_selection_raises_no_selected_tag_event() {
    use crate::sim::house_state::HouseState;
    use crate::sim::scenario_session::ScenarioGameMode;
    let rules = amcv_move_rules();
    let map = crate::map::map_file::MapFile::from_bytes(
        b"[Map]\nSize=0,0,32,32\nLocalSize=0,0,32,32\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
          [Triggers]\nSEL=Americans,<none>,Sel,0,1,1,1,0\n\
          [Events]\nSEL=1,33,0,0\n\
          [Actions]\nSEL=1,28,0,9,0,0,0,0,A\n\
          [Tags]\nSEL_TAG=0,Sel,SEL\n",
    )
    .unwrap();
    let global_after_select = |mode| {
        let mut sim = Simulation::new();
        let owner = sim.intern("Americans");
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
        sim.session.set_game_mode(mode);
        spawn_rule_backed_unit(&mut sim, 42, "AMCV", &rules);
        sim.initialize_trigger_definitions(
            &map,
            &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
            None,
            None,
            None,
        );
        sim.classify_map_triggers(&rules);
        sim.attach_object_trigger_tag(42, Some("SEL_TAG"));
        let select = Command::Select {
            entity_ids: vec![42],
            additive: false,
        };
        assert!(sim.apply_command("Americans", &select, Some(&rules)));
        assert!(
            sim.substrate
                .entities
                .get(42)
                .unwrap()
                .seldom
                .selection
                .is_selected()
        );
        sim.global_variables()[9]
    };
    assert!(!global_after_select(ScenarioGameMode::Lan));
    assert!(global_after_select(ScenarioGameMode::Skirmish));
}

#[test]
fn network_match_refuses_selling_another_houses_wall() {
    use crate::sim::house_state::HouseState;
    let (rules, overlays) = crate::sim::world::tests::gsi_04_07_wall_sell_rules(false, false);
    let mut sim = Simulation::new();
    let owner = sim.intern("Americans");
    for (index, house) in [(0, owner), (1, sim.intern("British"))] {
        sim.houses
            .register(HouseState::new(house, index, None, true, 0, 10));
    }
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Wol);
    let mut grid = crate::sim::overlay_grid::OverlayGrid::new(3, 3);
    grid.place_owned_wall(1, 1, 2, 0, owner);
    sim.overlay_grid = Some(grid);
    let sale = Command::SellWallAtCell { x: 1, y: 1 };
    let sell = |sim: &mut Simulation, house: &str| {
        sim.apply_command_with_overlays(
            None,
            house,
            &sale,
            &rules,
            Some(&overlays),
            &mut FrameEffects::default(),
        )
    };
    let wall = |sim: &Simulation| sim.overlay_grid.as_ref().unwrap().cell(1, 1).overlay_id;

    assert!(!sell(&mut sim, "British"));
    assert!(wall(&sim).is_some());
    assert!(sell(&mut sim, "Americans"));
    assert_eq!(wall(&sim), None);
}

#[test]
fn accepted_archive_keeps_its_tokens_after_capture_and_resolves_missing_target_to_null() {
    use crate::sim::house_state::HouseState;
    let rules = amcv_move_rules();
    let mut sim = Simulation::new();
    let old_owner = sim.intern("Americans");
    let new_owner = sim.intern("British");
    for owner in [old_owner, new_owner] {
        sim.houses
            .register(HouseState::new(owner, 0, None, true, 0, 10));
    }
    spawn_rule_backed_unit(&mut sim, 42, "AMCV", &rules);
    spawn_rule_backed_unit(&mut sim, 43, "AMCV", &rules);
    sim.substrate
        .entities
        .get_mut(42)
        .unwrap()
        .set_archive_target(Some(combat::TargetKind::Cell(5, 6)));
    assert!(sim.issue_local_command(CommandEnvelope::new(
        old_owner,
        0,
        Command::ArchiveObject {
            entity_id: 42,
            target_id: 43,
        }
    )));
    let record = sim.network_command_queues().local_records().unwrap()[0].clone();
    assert_eq!(
        ArchiveRecord::decode(&record).unwrap().target,
        MegaMissionTarget::Object { id: 43 }
    );
    sim.change_owner_with_rules(42, new_owner, &rules, None, &mut FrameEffects::default());
    sim.substrate.entities.remove(43);
    let queued = sim.take_frame_commands_for_test();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].owner, old_owner);
    assert!(sim.apply_command("Americans", &queued[0].payload, Some(&rules)));
    assert_eq!(
        sim.substrate.entities.get(42).unwrap().archive_target(),
        None
    );

    // Native6E6F0F resolves an unknown kind to NULL too. It must still
    // execute the archive setter, rather than silently dropping the event.
    sim.substrate
        .entities
        .get_mut(42)
        .unwrap()
        .set_archive_target(Some(combat::TargetKind::Cell(7, 8)));
    let mut null = record;
    null.payload_mut()[9] = 0xff;
    let command = sim.decode_native_command_record(&null, 0).unwrap();
    assert!(sim.apply_command("Americans", &command.payload, Some(&rules)));
    assert_eq!(
        sim.substrate.entities.get(42).unwrap().archive_target(),
        None
    );
}

#[test]
fn queued_sale_retains_issuer_across_capture_and_does_not_readmit_can_sell() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let Some((_, mut assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let mut rules = retail.rules;
    crate::map::theater::load_theater(&mut assets, "URBAN");
    rules.bind_building_buildup_assets(&assets, "URBAN");
    assert!(rules.has_buildup("GAPILE"));
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4C6F12);
    let old_owner = sim.interner.get("Americans").unwrap();
    let new_owner = sim.interner.get("Russians").unwrap();
    sim.houses
        .get_mut(&new_owner)
        .unwrap()
        .set_is_human_for_test(true);
    let building = sim
        .spawn_object("GAPILE", "Americans", 8, 8, 0, &rules)
        .unwrap();
    let record = sim
        .encode_object_action_record(
            old_owner,
            &Command::SellBuilding {
                entity_id: building,
            },
        )
        .unwrap();
    assert!(sim.admit_native_command_record(record.clone()));
    sim.change_owner_with_rules(
        building,
        new_owner,
        &rules,
        None,
        &mut FrameEffects::default(),
    );
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "sale", 0);
    let mut sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    sim.restore_after_snapshot_load().unwrap();
    let admitted = sim.take_frame_commands_for_test();
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].owner, old_owner);
    assert!(!sim.apply_command("Americans", &admitted[0].payload, Some(&rules)));
    assert!(
        !sim.substrate
            .entities
            .get(building)
            .unwrap()
            .building_down()
    );

    // A double click can admit two events before the first starts Selling.
    // The second still reaches SellBack(-1), even though CanSell now refuses.
    let current = sim
        .encode_object_action_record(
            new_owner,
            &Command::SellBuilding {
                entity_id: building,
            },
        )
        .unwrap();
    let envelope = sim.decode_native_command_record(&current, 0).unwrap();
    let credits = sim.houses[&new_owner].economy().credits();
    let (accepted, draws) = crate::sim::rng::trace_draws(|| {
        sim.apply_command("Russians", &envelope.payload, Some(&rules))
    });
    assert!(accepted);
    assert!(draws.is_empty(), "crew RNG belongs to later Selling visits");
    let actor = sim.substrate.entities.get(building).unwrap();
    assert!(actor.building_down());
    let progress = (actor.mission.handler_state(), *actor.native_stage());
    assert!(!production::can_sell_building(&sim, &rules, building));
    assert!(sim.apply_command("Russians", &envelope.payload, Some(&rules)));
    let actor = sim.substrate.entities.get(building).unwrap();
    assert_eq!(
        (actor.mission.handler_state(), *actor.native_stage()),
        progress
    );
    assert_eq!(sim.houses[&new_owner].economy().credits(), credits);
    // Event4C6F2D..4C6F35 refuses a retained but no-longer-alive target.
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .lifecycle
        .set_object_alive_for_test(false);
    sim.sound_events.clear_for_test();
    assert!(!sim.apply_command("Russians", &envelope.payload, Some(&rules)));
    assert!(sim.sound_events.iter().next().is_none());
    sim.substrate.entities.remove(building);
    let stale = sim.decode_native_command_record(&record, 0).unwrap();
    assert!(!sim.apply_command("Americans", &stale.payload, Some(&rules)));
}

#[test]
fn accepted_repair_resolves_the_captured_building_and_stale_target_at_dispatch() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x4C6ED2);
    let old_owner = sim.interner.get("Americans").unwrap();
    let new_owner = sim.interner.get("Russians").unwrap();
    let building = sim
        .spawn_object("GAPILE", "Americans", 8, 8, 0, &rules)
        .unwrap();
    sim.substrate
        .entities
        .get_mut(building)
        .unwrap()
        .health
        .set(300);
    let record = sim.encode_repair_record(old_owner, building).unwrap();
    assert!(sim.admit_native_command_record(record.clone()));
    sim.change_owner_with_rules(
        building,
        new_owner,
        &rules,
        None,
        &mut FrameEffects::default(),
    );
    assert!(
        !sim.substrate
            .entities
            .get(building)
            .unwrap()
            .building
            .repairing
            .is_on()
    );
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "repair", 0);
    let mut sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    sim.restore_after_snapshot_load().unwrap();
    let admitted = sim.take_frame_commands_for_test();
    assert_eq!(admitted.len(), 1);
    assert_eq!(admitted[0].owner, old_owner);
    let (accepted, draws) = crate::sim::rng::trace_draws(|| {
        sim.apply_command("Americans", &admitted[0].payload, Some(&rules))
    });
    assert!(accepted, "the old issuer is not receiver admission");
    assert!(draws.is_empty());
    assert!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .building
            .repairing
            .is_on()
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .health
            .current(),
        300
    );
    sim.substrate.entities.remove(building);
    let stale = sim.decode_native_command_record(&record, 0).unwrap();
    assert!(!sim.apply_command("Americans", &stale.payload, Some(&rules)));
}

#[test]
fn idle_record_preserves_unresolved_token_and_decodes_at_execution() {
    use crate::sim::house_state::HouseState;

    let rules = amcv_move_rules();
    let mut sim = Simulation::new();
    sim.interner.intern("Interner gap");
    let actor_owner = sim.interner.intern("Americans");
    let issuer = sim.interner.intern("British");
    sim.houses
        .register(HouseState::new(actor_owner, 0, None, true, 0, 10));
    sim.houses
        .register(HouseState::new(issuer, 1, None, true, 0, 10));
    sim.session.set_binary_frame_for_test(u32::MAX - 3);

    // Encoding must not resolve a stale identifier. The native token is
    // retained until4C74CE, independently of the issuer's House.
    let record = sim.encode_stop_record(issuer, 42).unwrap();
    assert_eq!(record.house_id(), 1);
    assert_eq!(record.frame_stamp(), -4);
    assert_eq!(StopRecord::decode(&record).unwrap().entity_id, 42);
    assert!(sim.decode_native_command_record(&record, 123).is_none());
    assert!(sim.encode_scatter_record(issuer, 42).is_none());

    spawn_rule_backed_unit(&mut sim, 42, "AMCV", &rules);
    let actor = sim.substrate.entities.get_mut(42).unwrap();
    actor.navigation.set_nav_list_for_test(
        vec![crate::sim::components::NavTargetRef::cell(12, 13)],
        false,
    );
    let command = sim.decode_native_command_record(&record, 123).unwrap();
    assert_eq!(command.owner, issuer);
    assert_eq!(command.execute_tick, 123);
    assert_eq!(command.payload, Command::Stop { entity_id: 42 });
    assert!(sim.apply_command("British", &command.payload, Some(&rules)));
    assert!(
        sim.substrate
            .entities
            .get(42)
            .unwrap()
            .navigation
            .nav_list()
            .is_empty()
    );
}

#[test]
fn idle_record_rejects_unmapped_issuer_and_out_of_range_identifier() {
    use crate::sim::house_state::HouseState;

    let mut sim = Simulation::new();
    let issuer = sim.interner.intern("British");
    assert!(sim.encode_stop_record(issuer, 1).is_none());
    sim.houses
        .register(HouseState::new(issuer, 0, None, true, 0, 10));
    assert!(sim.encode_stop_record(issuer, i32::MAX as u64).is_some());
    assert!(
        sim.encode_stop_record(issuer, i32::MAX as u64 + 1)
            .is_none()
    );
}

// ===== Order admission (GSI-07.01): liveness, strength and limbo =====

/// Original Event4C73C7/4C73D7 clear the suspended references while
/// preserving the suspended mission selector. AreaGuard4C7409..4C7430
/// installs the first token as destination/post, never as combat Target.
/// Executed Cell/object/null controls: input_oracle/area_guard.json.
#[test]
fn area_guard_clears_suspended_references_and_keeps_the_ordered_post() {
    use crate::sim::components::NavTargetRef;
    use crate::sim::mission::MissionDispatchTimer;
    use crate::sim::mission::state::MissionTestFixture;

    for post in [
        None,
        Some(combat::TargetKind::Cell(20, 20)),
        Some(combat::TargetKind::Entity(2)),
        Some(combat::TargetKind::Entity(999)),
    ] {
        let rules = amcv_move_rules();
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::supply_native_map(&mut sim);
        spawn_rule_backed_unit(&mut sim, 1, "AMCV", &rules);
        spawn_rule_backed_unit(&mut sim, 2, "AMCV", &rules);
        let before_timer = MissionDispatchTimer::from_raw(4, 45);
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        actor.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Guard),
            suspended: MissionId::from_known(MissionType::Attack),
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: before_timer,
        });
        actor
            .seldom
            .suspended_attack_target
            .set_for_test(Some(combat::TargetKind::Entity(2)));
        actor
            .navigation
            .set_suspended_nav_com_for_test(Some(NavTargetRef::cell(12, 12)));
        actor.set_archive_target(Some(combat::TargetKind::Cell(5, 5)));
        actor.seldom.mega_mission = crate::sim::mission::mega_mission::MegaMission::attack_move(
            Some(crate::sim::combat::TargetKind::Cell(12, 12)),
        );

        assert!(sim.apply_command(
            "Americans",
            &Command::Guard {
                entity_id: 1,
                target: post
            },
            Some(&rules),
        ));
        let actor = sim.substrate.entities.get(1).unwrap();
        let post = post.filter(|post| *post != combat::TargetKind::Entity(999));
        assert_eq!(
            actor.mission.current(),
            MissionId::from_known(MissionType::Guard)
        );
        assert_eq!(
            actor.mission.queued(),
            MissionId::from_known(MissionType::AreaGuard)
        );
        assert_eq!(
            actor.mission.suspended(),
            MissionId::from_known(MissionType::Attack)
        );
        assert_eq!(actor.mission.dispatch_timer(), before_timer);
        assert!(actor.seldom.suspended_attack_target.target().is_none());
        assert!(actor.navigation.suspended_nav_com().is_none());
        assert!(actor.attack_target.target().is_none());
        assert!(
            !actor.seldom.mega_mission.is_attack_move(),
            "Assign_Target_Command answers Area Guard with Clear_All_TarCom"
        );
        assert_eq!(actor.archive_target(), post);
        assert_eq!(actor.navigation.nav_com(), post.map(Into::into));
    }
}

// ===== Duplicate Enter on a building is a no-op (GSI-07.01 C1) =====

fn bunker_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=TANK\n1=NOGUN\n\n[InfantryTypes]\n\n[AircraftTypes]\n\n\
         [BuildingTypes]\n0=NATBNK\n\n\
         [TANK]\nStrength=400\nArmor=heavy\nSpeed=6\nTurret=yes\nPrimary=120mm\n\n\
         [NOGUN]\nStrength=400\nArmor=heavy\nSpeed=6\nTurret=yes\n\n\
         [NATBNK]\nStrength=1000\nArmor=heavy\nBunker=yes\n",
    ))
    .expect("bunker rules")
}

fn spawn_bunker_struct(sim: &mut Simulation, sid: u64, owner: &str, rx: u16, ry: u16) {
    let owner_id = sim.interner.intern(owner);
    let type_id = sim.interner.intern("NATBNK");
    let mut ge = GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health::new(1000),
        type_id,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    ge.building.bunker_runtime = Some(crate::sim::docking::bunker_install::BunkerRuntime::idle());
    // Revealed object: order admission reads the limbo byte.
    ge.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(ge);
}

fn spawn_bunkerable(
    sim: &mut Simulation,
    sid: u64,
    owner: &str,
    type_name: &str,
    rx: u16,
    ry: u16,
) {
    let owner_id = sim.interner.intern(owner);
    let type_id = sim.interner.intern(type_name);
    let mut ge = GameEntity::new_at_frame_zero_for_test(
        sid,
        rx,
        ry,
        0,
        0,
        owner_id,
        Health::new(400),
        type_id,
        EntityCategory::Unit,
        0,
        5,
        true,
    );
    // The native Unit setter dispatches through an installed locomotor.
    ge.locomotor = Some(
        crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Drive,
        ),
    );
    ge.locomotor
        .as_mut()
        .unwrap()
        .ensure_installed_track_state();
    // Revealed object: order admission reads the limbo byte. Its Unlimbo
    // would have published its threat (`0x006F6ED2`); TANK poses none.
    ge.lifecycle.set_in_limbo_for_test(false);
    ge.cached_spatial_threat.publish(0);
    sim.substrate.entities.insert(ge);
}

/// The MEGAMISSION arm tests neither friendship nor the actor's weapon,
/// and an object token no longer present resolves to NULL
/// (`0x004C71F6..0x004C749D`): an Attack onto an own unit, an Attack onto
/// a removed object and an unarmed unit's Cell attack each queue Attack
/// with the target the setter receives. What_Action refuses them at input.
#[test]
fn attack_event_runs_whatever_the_relation_weapon_or_token() {
    let rules = bunker_rules();
    for (command, attacker, target) in [
        (
            Command::Attack {
                attacker_id: 1,
                target_id: 2,
            },
            1,
            Some(combat::TargetKind::Entity(2)),
        ),
        (
            Command::Attack {
                attacker_id: 1,
                target_id: 999,
            },
            1,
            None,
        ),
        (
            Command::ForceAttackCell {
                attacker_id: 3,
                target_rx: 20,
                target_ry: 20,
            },
            3,
            Some(combat::TargetKind::Cell(20, 20)),
        ),
    ] {
        let mut sim = Simulation::new();
        spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 14, 14);
        spawn_bunkerable(&mut sim, 2, "Americans", "TANK", 16, 14);
        spawn_bunkerable(&mut sim, 3, "Americans", "NOGUN", 18, 14);

        assert!(
            sim.apply_command("Americans", &command, Some(&rules)),
            "{command:?}"
        );
        let actor = sim.substrate.entities.get(attacker).unwrap();
        assert_eq!(
            actor.mission.queued(),
            MissionId::from_known(MissionType::Attack),
            "{command:?}"
        );
        assert_eq!(actor.attack_target.target(), target, "{command:?}");
    }
}

/// What_Action's CAN_LOAD query refuses an unarmed unit (no Primary,
/// so CanAutoDeployHere rejects it) and an enemy bunker. This is input
/// admission; the MEGAMISSION event does not repeat the query.
#[test]
fn bunker_click_query_rejects_an_unarmed_unit_or_an_enemy_bunker() {
    use crate::sim::docking::bunker_install::BunkerState;
    use crate::sim::docking::bunker_link::BunkerLink;
    let rules = bunker_rules();
    for (bunker_owner, unit_type) in [("Americans", "NOGUN"), ("Soviets", "TANK")] {
        let mut sim = Simulation::new();
        spawn_bunker_struct(&mut sim, 2, bunker_owner, 10, 10);
        spawn_bunkerable(&mut sim, 1, "Americans", unit_type, 14, 14);

        assert_ne!(
            crate::sim::radio::receive::can_load_query(&sim, 2, 1, &rules),
            crate::sim::radio::RadioResponse::Roger,
            "{bunker_owner} bunker, {unit_type}"
        );
        assert_eq!(
            sim.substrate.entities.get(1).unwrap().bunker.link(),
            BunkerLink::None
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(2)
                .unwrap()
                .building
                .bunker_runtime
                .unwrap()
                .state(),
            BunkerState::Idle,
            "rejected admission leaves the machine idle"
        );
    }
}

/// CAN_LOAD refuses a bunker under Construction or being sold
/// (`0x0043C326..0x0043C346`) without mutating the tank's current seek
/// or the target's ready latch. This compares the query only; no
/// replacement event or UI click is executed.
#[test]
fn bunker_click_query_preserves_seek_while_target_builds_up_or_is_sold() {
    use crate::sim::docking::bunker_install::BunkerState;
    let rules = bunker_rules();
    for mission in [MissionType::Construction, MissionType::Selling] {
        let mut sim = Simulation::new();
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        spawn_bunker_struct(&mut sim, 2, "Americans", 10, 10);
        spawn_bunker_struct(&mut sim, 3, "Americans", 16, 10);
        spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 13, 14);
        assert!(sim.apply_command(
            "Americans",
            &Command::clicked_mission(1, MissionType::Enter, combat::TargetKind::Entity(3)),
            Some(&rules),
        ));
        sim.substrate
            .entities
            .get_mut(2)
            .unwrap()
            .mission
            .set_current_for_test(MissionId::from_known(mission), 1);

        assert_eq!(
            crate::sim::radio::receive::can_load_query(&sim, 2, 1, &rules),
            crate::sim::radio::RadioResponse::Negatory,
            "{mission:?}"
        );
        let building = sim.substrate.entities.get(2).unwrap();
        assert_eq!(building.building_ready_latch(), 0, "{mission:?}");
        assert_eq!(building.mission.queued(), MissionId::NONE, "{mission:?}");
        assert_eq!(
            building.building.bunker_runtime.unwrap().state(),
            BunkerState::Idle,
            "{mission:?}"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .seldom
                .radio_contacts
                .slot(0),
            Some(3),
            "{mission:?} keeps the current seek"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(3)
                .unwrap()
                .seldom
                .radio_contacts
                .slot(0),
            Some(1),
            "{mission:?}"
        );
    }
}

/// A compact installed bunker: the VERA-only eject record names the
/// building, so both the occupant's link and its radio contact must be
/// real production state.
fn installed_bunker_fixture() -> (Simulation, RuleSet) {
    let rules = bunker_rules();
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    spawn_bunker_struct(&mut sim, 2, "Americans", 10, 10);
    spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 10, 10);
    crate::sim::docking::bunker_link::install_bunker_link(&mut sim, 2, 1, &rules);
    let unit = sim.substrate.entities.get_mut(1).unwrap();
    unit.seldom.radio_contacts.set_slot(0, 2);
    unit.navigation
        .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::object(2)));
    (sim, rules)
}

/// `UnitClass::Mission_Unload @ 0x0073D630` is the only caller of
/// `BuildingClass::EmptyBunker @ 0x004595C0` (`0x0073D66D`). Opcode 0x31
/// must therefore publish Event9's DEPLOY order, not release the tank
/// while the command executes (#3675).
#[test]
fn eject_bunker_record_queues_unload_instead_of_releasing_at_command_time() {
    let (mut sim, rules) = installed_bunker_fixture();
    assert!(sim.apply_command(
        "Americans",
        &Command::EjectBunker { bunker_id: 2 },
        Some(&rules),
    ));
    assert_eq!(
        sim.substrate.entities.get(2).unwrap().bunker.occupant(),
        Some(1),
        "EmptyBunker belongs to the Unload mission's Logic update"
    );
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().bunker.link(),
        crate::sim::docking::bunker_link::BunkerLink::Installed(2)
    );
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!(unit.mission.queued().known(), Some(MissionType::Unload));
    // Event9 `0x004C77E4` breaks contact 0 before the null destination
    // (`0x004C77F8`) and null target (`0x004C7804`) setters.
    assert_eq!(unit.seldom.radio_contacts.slot(0), None);
    assert!(unit.navigation.nav_com().is_none());
    assert!(unit.attack_target.target().is_none());
}

/// Event9 detaches the deployed object's planning (`0x004C76CA`), so the
/// eject record detaches the occupant's path, not the bunker's.
#[test]
fn eject_bunker_record_detaches_the_occupants_planning() {
    let (mut sim, rules) = installed_bunker_fixture();
    sim.start_empty_planning_path_for_test(1);
    assert!(sim.apply_command(
        "Americans",
        &Command::EjectBunker { bunker_id: 2 },
        Some(&rules),
    ));
    assert!(!sim.has_planning_path_for_test(1));
}

/// The command-time release only checked ownership and an occupant.
/// Event9's admission (`0x004C76BC`) refuses a limboed actor, so a dead
/// or limboed tank keeps its bunker and gets no Unload mission.
#[test]
fn eject_bunker_record_refuses_a_limboed_occupant() {
    let (mut sim, rules) = installed_bunker_fixture();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .lifecycle
        .set_in_limbo_for_test(true);
    assert!(!sim.apply_command(
        "Americans",
        &Command::EjectBunker { bunker_id: 2 },
        Some(&rules),
    ));
    assert_eq!(
        sim.substrate.entities.get(2).unwrap().bunker.occupant(),
        Some(1)
    );
    assert_ne!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .mission
            .queued()
            .known(),
        Some(MissionType::Unload)
    );
}

/// Drive the record through real frames: `Mission_Unload @ 0x0073D630`
/// promotes, its head calls `EmptyBunker @ 0x004595C0`, and only then do
/// the links clear. Without the fix the very first command execution
/// would already have released the tank.
#[test]
fn eject_bunker_record_releases_on_the_unload_logic_visit() {
    let (mut sim, rules) = installed_bunker_fixture();
    // `unit_unload_empties_bunker` looks the building up in the Unit's
    // cell (`Look_up_building_in_cell @ 0x0047C520`).
    for (rx, ry) in
        crate::sim::occupancy::entity_occupancy_cells(sim.substrate.entities.get(2).unwrap())
    {
        sim.substrate.occupancy.add(
            rx,
            ry,
            2,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
    }
    assert!(sim.register_live_object(1));
    assert!(sim.apply_command(
        "Americans",
        &Command::EjectBunker { bunker_id: 2 },
        Some(&rules),
    ));
    assert_eq!(
        sim.substrate.entities.get(2).unwrap().bunker.occupant(),
        Some(1)
    );
    for _ in 0..16 {
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        if sim
            .substrate
            .entities
            .get(2)
            .unwrap()
            .bunker
            .occupant()
            .is_none()
        {
            break;
        }
    }
    assert_eq!(
        sim.substrate.entities.get(2).unwrap().bunker.occupant(),
        None,
        "the Unload Logic visit must empty the bunker"
    );
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().bunker.link(),
        crate::sim::docking::bunker_link::BunkerLink::None
    );
}

#[test]
fn bunker_full_lifecycle_enter_install_then_eject() {
    use crate::sim::docking::bunker_install::{BunkerState, update_bunker};
    use crate::sim::docking::bunker_link::BunkerLink;
    let rules = bunker_rules();
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    spawn_bunker_struct(&mut sim, 2, "Americans", 10, 10);
    // Keep this component fixture's declared TANK rules, but use the
    // production ART reader's retail 2x2 foundation and LTNK turn rate.
    // This exercises the same-cell case that must slide away from the
    // building's raw display-registration coordinate before installation.
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .seldom
        .type_facts
        .set_foundation_for_test(
            retail
                .rules
                .object("NATBNK")
                .unwrap()
                .foundation
                .clone()
                .into(),
        );
    // The install's arrival test reads its cell's building list
    // (`0x0047C520`), so the bunker stands on its retail foundation.
    for (rx, ry) in
        crate::sim::occupancy::entity_occupancy_cells(sim.substrate.entities.get(2).unwrap())
    {
        sim.substrate.occupancy.add(
            rx,
            ry,
            2,
            crate::sim::movement::locomotor::MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::AppendBuilding,
        );
    }
    let center = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(2).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    // The tank stands in the bunker's cell, where it stops after its
    // approach.
    spawn_bunkerable(&mut sim, 1, "Americans", "TANK", 10, 10);
    let unit = sim.substrate.entities.get_mut(1).unwrap();
    unit.body_facing
        .set_rot(retail.rules.object("LTNK").unwrap().turret_rot);
    unit.locomotor = Some(
        crate::sim::movement::locomotor::LocomotorState::for_test_kind(
            crate::rules::locomotor_type::LocomotorKind::Drive,
        ),
    );
    unit.locomotor
        .as_mut()
        .unwrap()
        .ensure_installed_track_state();
    // Exercise real cell/Logic membership, not the admission-only helper's
    // already-clear Limbo byte (which makes Reveal a no-op).
    unit.lifecycle.set_in_limbo_for_test(true);
    assert!(matches!(
        sim.reveal(1),
        crate::sim::world::RevealOutcome::Revealed { .. }
    ));

    // The supplied arrival retains Enter, object NavCom and contact
    // zero, as in the original bunker_entry history. Run its real
    // PerCell receiver to queue Repair and power off the Drive.
    let unit = sim.substrate.entities.get_mut(1).unwrap();
    unit.mission
        .set_current_for_test(MissionId::from_known(MissionType::Enter), 0);
    unit.navigation
        .set_nav_com_for_test(Some(crate::sim::components::NavTargetRef::object(2)));
    unit.seldom.radio_contacts.set_slot(0, 2);
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .seldom
        .radio_contacts
        .set_slot(0, 1);
    assert!(sim.unit_dock_now(1, &rules, None, &mut FrameEffects::default()));
    // This fixture starts at a completed, stopped arrival. Give the
    // building its first visit before a new Drive Process: a fresh
    // Process with Enter/NavCom but no retained destination re-aims the
    // Drive (4B0971..4B09A6), so it is no longer this stopped boundary.
    update_bunker(&mut sim, &rules, None, 2, &mut FrameEffects::default());
    assert_eq!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .building
            .bunker_runtime
            .unwrap()
            .state(),
        BunkerState::ClearWait,
    );
    // Prepared same-cell arrival, then the real production locomotor
    // corridor and installer, in their production order, one frame at a
    // time. No head/position writes or Facing::Snap complete the motion.
    // Object/Techno AI and a whole native match remain outside this test.
    let mut saw_track_step = false;
    let mut moved = 0;
    for _ in 0..600 {
        let movement = sim
            .process_ground_locomotor_stats_for_test(1, Some(&rules), None)
            .expect("flat arena supplies every production movement receiver");
        moved += movement.moved_steps;
        update_bunker(
            &mut sim,
            &rules,
            None,
            2,
            &mut crate::sim::world::FrameEffects::default(),
        );
        let state = sim
            .substrate
            .entities
            .get(2)
            .unwrap()
            .building
            .bunker_runtime
            .unwrap()
            .state();
        saw_track_step |= state == BunkerState::TrackStep;
        sim.session
            .set_binary_frame_for_test(sim.session.binary_frame() + 1);
        if state == BunkerState::Occupied {
            break;
        }
    }
    assert!(
        saw_track_step,
        "same-cell installation must not skip Force_Track"
    );
    assert!(
        moved > 0,
        "the production track host must actually place the unit"
    );
    let rt = sim
        .substrate
        .entities
        .get(2)
        .unwrap()
        .building
        .bunker_runtime
        .unwrap();
    assert_eq!(rt.state(), BunkerState::Occupied);
    assert_eq!(
        sim.substrate.entities.get(2).unwrap().bunker.occupant(),
        Some(1)
    );
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!(unit.bunker.link(), BunkerLink::Installed(2));
    assert!(
        unit.seldom.in_logic_vector.is_member(),
        "install preserves LogicVector membership"
    );
    assert!(!unit.lifecycle.in_limbo());
    let parked =
        crate::sim::movement::ground_pose::object_get_coords(unit, sim.resolved_terrain.as_ref());
    assert_eq!(
        parked, center,
        "real Drive Process parks at the foundation centre"
    );
    assert_ne!(
        parked,
        crate::sim::movement::ground_pose::object_location(
            sim.substrate.entities.get(2).unwrap(),
            sim.resolved_terrain.as_ref(),
        )
    );
    assert!(crate::sim::movement::track_head::committed_track_head(unit).is_none());
    assert_eq!(
        sim.bunker_wall_events.iter().filter(|e| e.up).count(),
        1,
        "one walls-up event on install"
    );

    // 3) Eject: opcode 0x31 publishes the Unload mission. Its Logic
    // visit owns `EmptyBunker @ 0x004595C0` (`0x0073D66D`), so the
    // release, Force, links and walls happen there, not at command time.
    sim.bunker_wall_events.clear();
    assert!(sim.apply_command(
        "Americans",
        &Command::EjectBunker { bunker_id: 2 },
        Some(&rules),
    ));
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .mission
            .queued()
            .known(),
        Some(MissionType::Unload)
    );
    assert_eq!(
        sim.substrate.entities.get(2).unwrap().bunker.occupant(),
        Some(1),
        "no command-time release"
    );
    assert!(
        crate::sim::docking::bunker_link::unit_unload_empties_bunker(
            &mut sim,
            1,
            &rules,
            &mut FrameEffects::default(),
        )
    );
    assert_eq!(
        sim.substrate.entities.get(2).unwrap().bunker.occupant(),
        None
    );
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!(unit.bunker.link(), BunkerLink::None);
    assert!(
        unit.seldom.in_logic_vector.is_member(),
        "occupant stays active on eject"
    );
    assert!(!unit.lifecycle.in_limbo());
    assert_eq!(
        crate::sim::movement::ground_pose::object_get_coords(unit, sim.resolved_terrain.as_ref(),),
        parked,
        "Eject starts Force_Track without teleporting"
    );
    assert_eq!(
        unit.locomotor
            .as_ref()
            .and_then(|loco| loco.selected_drive_runtime())
            .and_then(|runtime| runtime.retained())
            .unwrap()
            .track()
            .turn_index(),
        0x47
    );
    assert_eq!(
        unit.mission.queued(),
        MissionId::from_known(MissionType::Move)
    );
    assert_eq!(
        sim.bunker_wall_events.iter().filter(|e| !e.up).count(),
        1,
        "one walls-down event on eject"
    );
    let exit = match unit.navigation.nav_com() {
        Some(crate::sim::components::NavTargetRef::Cell { rx, ry }) => (rx, ry),
        other => panic!("normal bunker release must issue its exit cell: {other:?}"),
    };
    for _ in 0..600 {
        sim.process_ground_locomotor_stats_for_test(1, Some(&rules), None)
            .expect("normal release reaches the production movement corridor");
        sim.session
            .set_binary_frame_for_test(sim.session.binary_frame() + 1);
        let unit = sim.substrate.entities.get(1).unwrap();
        if (unit.position.rx(), unit.position.ry()) == exit
            && crate::sim::movement::track_head::committed_track_head(unit).is_none()
            && crate::sim::movement::motion_query::is_moving(&unit) != Some(true)
        {
            break;
        }
    }
    let unit = sim.substrate.entities.get(1).unwrap();
    assert_eq!((unit.position.rx(), unit.position.ry()), exit);
    assert!(crate::sim::movement::track_head::committed_track_head(unit).is_none());
    assert!(crate::sim::movement::motion_query::is_moving(&unit) != Some(true));
    assert_eq!(unit.bunker.link(), BunkerLink::None);
    assert_eq!(
        sim.substrate
            .entities
            .get(2)
            .unwrap()
            .building
            .bunker_runtime
            .unwrap()
            .state(),
        BunkerState::Idle
    );
}
