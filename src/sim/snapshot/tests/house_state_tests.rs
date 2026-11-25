//! Snapshot round trips of House and game-outcome state: AI activation, base
//! planning and defense, waves, game end and result flags, lighting,
//! difficulty and extra registered Houses.

use super::*;

#[test]
fn house_ai_activation_all_combinations_roundtrip_v115() {
    use crate::sim::house_state::{HouseAiActivationLatches, HouseState};

    for bits in 0u8..16 {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Computer1");
        let latches = HouseAiActivationLatches {
            production: bits & 1 != 0,
            autocreate_allowed: bits & 2 != 0,
            ai_triggers_active: bits & 4 != 0,
            auto_base_building: bits & 8 != 0,
        };
        let mut house = HouseState::new(owner, 0, None, false, 0, 10);
        house.set_ai_activation_for_test(latches);
        sim.houses.register(house);

        let description = format!("house-ai-activation-{bits}");
        let bytes = GameSnapshot::save(&sim, 0, 0, &description, 0);
        assert_eq!(
            GameSnapshot::read_header(&bytes).unwrap().layout,
            super::snapshot_layout()
        );
        let restored = GameSnapshot::load(&bytes)
            .expect("current v115 snapshot")
            .sim;
        assert_eq!(restored.houses[&owner].ai_activation(), latches);
    }
}

#[test]
fn alternate_base_center_roundtrips_with_primary_center_and_hash() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("AMERICANS");
    let country = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, Some(country), false, 0, 10);
    house.set_base_center_for_test(Some((40, 50)));
    house.set_alternate_base_center_for_test((93, 106));
    house.set_base_plan_center_for_test((19, 21));
    sim.houses.register(house);
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "alternate-base-center", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        super::snapshot_layout()
    );
    let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

    assert_eq!(restored.houses[&owner].base_center(), Some((40, 50)));
    assert_eq!(restored.houses[&owner].alternate_base_center(), (93, 106));
    assert_eq!(restored.houses[&owner].base_plan_center(), (19, 21));
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn naval_build_const_order_and_membership_roundtrip_with_hash() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("AMERICANS");
    let country = sim.interner.intern("Americans");
    let type_ref = sim.interner.intern("GACNST");
    let house = crate::sim::house_state::HouseState::new(owner, 0, Some(country), false, 0, 10);
    sim.houses.register(house);
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        9,
        4,
        5,
        0,
        0,
        owner,
        crate::sim::combat::Health::new(1000),
        type_ref,
        crate::map::entities::EntityCategory::Structure,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(entity);

    let default_hash = sim.state_hash();

    sim.houses
        .get_mut(&owner)
        .unwrap()
        .set_build_const_order_for_test(vec![9, 3]);
    sim.substrate
        .entities
        .get_mut(9)
        .unwrap()
        .seldom
        .type_facts
        .set_build_const_eligible_for_test(true);

    let ordered_hash = sim.state_hash();
    assert_ne!(ordered_hash, default_hash);
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .build_const_order_mut_for_test()
        .swap(0, 1);
    assert_ne!(
        sim.state_hash(),
        ordered_hash,
        "stored vector order is hashed"
    );
    sim.houses
        .get_mut(&owner)
        .unwrap()
        .build_const_order_mut_for_test()
        .swap(0, 1);

    sim.substrate
        .entities
        .get_mut(9)
        .unwrap()
        .seldom
        .type_facts
        .set_build_const_eligible_for_test(false);
    assert_ne!(
        sim.state_hash(),
        ordered_hash,
        "entity membership is hashed"
    );
    sim.substrate
        .entities
        .get_mut(9)
        .unwrap()
        .seldom
        .type_facts
        .set_build_const_eligible_for_test(true);
    assert_eq!(sim.state_hash(), ordered_hash);

    let bytes = GameSnapshot::save(&sim, 0, 0, "naval-build-const", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        super::snapshot_layout()
    );
    let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

    assert_eq!(restored.houses[&owner].build_const_order(), vec![9, 3]);
    assert!(
        restored
            .substrate
            .entities
            .get(9)
            .unwrap()
            .seldom
            .type_facts
            .build_const_eligible()
    );
    assert_eq!(restored.state_hash(), ordered_hash);
}

#[test]
fn gsi_04_05_base_plan_and_building_facts_roundtrip_current_snapshot() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Computer1");
    let type_ref = sim.interner.intern("GAPOWR");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 0, 10);
    house.base_plan_mut().set_percent_built_for_test(-17);
    house.base_plan_mut().set_nodes_for_test(vec![
        crate::sim::base_plan::BasePlanNode {
            type_or_control: 2,
            packed_cell: crate::sim::base_plan::pack_base_plan_cell(-4, 9),
            filled: true,
            retry_count: -8,
        },
        crate::sim::base_plan::BasePlanNode {
            type_or_control: -3,
            packed_cell: 0,
            filled: false,
            retry_count: 6,
        },
    ]);
    sim.houses.register(house);
    let mut entity = crate::sim::game_entity::GameEntity::new_at_frame_zero_for_test(
        1,
        12,
        14,
        0,
        0,
        owner,
        crate::sim::combat::Health::new(750),
        type_ref,
        crate::map::entities::EntityCategory::Structure,
        0,
        5,
        false,
    );
    entity
        .seldom
        .type_facts
        .set_base_plan_type_index_for_test(2);
    entity
        .seldom
        .type_facts
        .set_base_plan_is_defense_for_test(true);
    entity
        .seldom
        .type_facts
        .set_base_plan_has_undeploy_target_for_test(true);
    sim.substrate.entities.insert(entity);
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "base-plan", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        super::snapshot_layout()
    );
    let restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    assert_eq!(restored.houses[&owner].base_plan().percent_built(), -17);
    assert_eq!(restored.houses[&owner].base_plan().nodes().len(), 2);
    assert_eq!(
        restored.houses[&owner].base_plan().nodes()[0].retry_count,
        -8
    );
    let restored_entity = restored.substrate.entities.get(1).unwrap();
    assert_eq!(restored_entity.seldom.type_facts.base_plan_type_index(), 2);
    assert!(restored_entity.seldom.type_facts.base_plan_is_defense());
    assert!(
        restored_entity
            .seldom
            .type_facts
            .base_plan_has_undeploy_target()
    );
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn gsi_04_05_house_and_techno_base_defense_state_roundtrip_with_hash() {
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Computer1");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, false, 10_000, 10);
    house.strategy_emergency_mut().set_state_four();
    house.strategy_emergency_mut().set_all_to_hunt_bias();
    house.strategy_emergency_mut().note_building_attack(-17);
    house.strategy_emergency_mut().note_building_attacker(3);
    house.restart_strategy_timer(12, 106);
    sim.houses.register(house);
    let mut responder =
        crate::sim::game_entity::GameEntity::test_default(1, "E1", "Computer1", 4, 5);
    responder
        .seldom
        .base_defense_response
        .set_recruitable_a_for_test(false);
    responder
        .seldom
        .base_defense_response
        .set_recruitable_b_for_test(true);
    responder.set_archive_target(Some(crate::sim::combat::TargetKind::Entity(9)));
    responder
        .seldom
        .base_defense_response
        .set_cooldown_for_test(crate::sim::timer::CdTimer::started(-11, 225));
    sim.substrate.entities.insert(responder);
    let script_id = sim.interner.intern("BaseDefenseScript");
    let task_force_id = sim.interner.intern("BaseDefenseTaskForce");
    let team_type_id = sim.interner.intern("BaseDefenseTeamType");
    let ai_trigger_id = sim.interner.intern("BaseDefenseAITrigger");
    let member_type = sim.interner.intern("E1");
    let member_identity = crate::sim::team_script_vm::TeamMemberTypeIdentity {
        category: crate::rules::object_type::ObjectCategory::Infantry,
        id: member_type,
    };
    sim.team_script_vm
        .register_script(crate::sim::team_script_vm::TeamScriptDefinition {
            id: script_id,
            source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
            actions: vec![crate::sim::team_script_vm::TeamScriptAction {
                action_id: 2,
                argument: 0,
            }],
        });
    sim.team_script_vm
        .register_task_force(crate::sim::team_script_vm::TeamTaskForceDefinition {
            id: task_force_id,
            source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
            group: 7,
            entries: vec![crate::sim::team_script_vm::TeamTaskForceEntry {
                member_type: member_identity,
                count: 1,
            }],
        });
    sim.team_script_vm
        .register_team_type(crate::sim::team_script_vm::TeamTypeDefinition {
            id: team_type_id,
            script_id,
            task_force_id,
            priority: 0,
            is_base_defense: true,
            suicide: false,
            aggressive: false,
            combined_movement_zone: crate::rules::locomotor_type::MovementZone::Amphibious,
            base_zone_relation_enforced: false,
            transport_crossing_required: true,
        });
    sim.team_script_vm
        .register_ai_trigger(crate::sim::team_script_vm::TeamAiTriggerDefinition {
            id: ai_trigger_id,
            tokens: std::array::from_fn(|index| format!("raw-token-{index}")),
            display_name: "Base defense trigger".to_string(),
            enabled: true,
            primary_team_type: Some(team_type_id),
            owner: Some(crate::sim::team_script_vm::TeamAiTriggerOwner::Country(
                crate::rules::ruleset::CountryIdx(4),
            )),
            threshold: 7,
            condition: 6,
            object_type: Some(member_identity),
            comparison_mask: std::array::from_fn(|index| index as u8),
            weights: [
                crate::util::native_x87::NativeF64Bits::from_bits(1.5_f64.to_bits()),
                crate::util::native_x87::NativeF64Bits::from_bits(2.5_f64.to_bits()),
                crate::util::native_x87::NativeF64Bits::from_bits(3.5_f64.to_bits()),
            ],
            multiplayer: true,
            side: -9,
            storage_flag_d1: false,
            secondary_team_type: None,
            difficulty_enabled: [true, false, true],
            source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
        });
    let team_id = sim.team_script_vm.create_team_from_type(
        owner,
        team_type_id,
        &[crate::sim::team_script_vm::TeamScriptMember {
            entity_id: 1,
            member_type: member_identity,
        }],
        sim.session.binary_frame() as i32,
    );
    let resources = crate::sim::runtime::SimResources::empty();
    sim.suspend_teams_for_base_defense(
        owner,
        1,
        1800,
        &resources.rules,
        &mut crate::sim::world::FrameEffects::default(),
    );
    assert!(sim.team_script_vm.team_for_member(1).is_none());
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_05_strategy", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        super::snapshot_layout()
    );
    let restored = GameSnapshot::load(&bytes).expect("v102 snapshot").sim;
    let emergency = restored.houses[&owner].strategy_emergency();
    assert_eq!(emergency.mode(), 4);
    assert!(emergency.all_to_hunt_bias());
    assert_eq!(emergency.last_building_attack_frame(), -17);
    assert_eq!(emergency.last_attacker_house_index(), 3);
    assert_eq!(
        *restored.houses[&owner].strategy_timer(),
        crate::sim::timer::CdTimer::started(12, 106)
    );
    let restored_responder = restored.substrate.entities.get(1).unwrap();
    let response = restored_responder.seldom.base_defense_response;
    assert!(!response.recruitable_a());
    assert!(response.recruitable_b());
    assert_eq!(
        restored_responder.archive_target(),
        Some(crate::sim::combat::TargetKind::Entity(9))
    );
    assert_eq!(
        response.cooldown(),
        crate::sim::timer::CdTimer::started(-11, 225)
    );
    let team = restored.team_script_vm.team(team_id).unwrap();
    assert_eq!(team.member_count(), 0);
    assert_eq!(restored.team_script_vm.registry_counts(), (1, 1, 1, 1));
    let restored_team_type = restored
        .team_script_vm
        .team_type(team_type_id)
        .expect("persisted TeamType");
    assert_eq!(
        restored_team_type.combined_movement_zone,
        crate::rules::locomotor_type::MovementZone::Amphibious
    );
    assert!(!restored_team_type.base_zone_relation_enforced);
    assert!(restored_team_type.transport_crossing_required);
    assert_eq!(restored.team_script_vm.script_order(), &[script_id]);
    assert_eq!(restored.team_script_vm.task_force_order(), &[task_force_id]);
    assert_eq!(restored.team_script_vm.ai_trigger_order(), &[ai_trigger_id]);
    let restored_script = restored
        .team_script_vm
        .script(script_id)
        .expect("persisted ScriptType");
    assert_eq!(
        restored_script.source,
        crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd
    );
    assert_eq!(restored_script.actions.len(), 1);
    assert_eq!(restored_script.actions[0].action_id, 2);
    assert_eq!(restored_script.actions[0].argument, 0);
    let restored_task_force = restored
        .team_script_vm
        .task_force(task_force_id)
        .expect("persisted TaskForce");
    assert_eq!(
        restored_task_force.source,
        crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd
    );
    assert_eq!(restored_task_force.entries.len(), 1);
    assert_eq!(restored_task_force.entries[0].member_type, member_identity);
    assert_eq!(restored_task_force.entries[0].count, 1);
    assert_eq!(restored_task_force.group, 7);
    let restored_trigger = restored
        .team_script_vm
        .ai_trigger(ai_trigger_id)
        .expect("persisted typed AITriggerType");
    assert_eq!(restored_trigger.tokens[11], "raw-token-11");
    assert_eq!(restored_trigger.display_name, "Base defense trigger");
    assert!(restored_trigger.enabled);
    assert_eq!(restored_trigger.primary_team_type, Some(team_type_id));
    assert_eq!(
        restored_trigger.owner,
        Some(crate::sim::team_script_vm::TeamAiTriggerOwner::Country(
            crate::rules::ruleset::CountryIdx(4)
        ))
    );
    assert_eq!(restored_trigger.threshold, 7);
    assert_eq!(restored_trigger.condition, 6);
    assert_eq!(restored_trigger.object_type, Some(member_identity));
    assert_eq!(restored_trigger.comparison_mask[31], 31);
    assert_eq!(
        restored_trigger.weights,
        [
            crate::util::native_x87::NativeF64Bits::from_bits(1.5_f64.to_bits()),
            crate::util::native_x87::NativeF64Bits::from_bits(2.5_f64.to_bits()),
            crate::util::native_x87::NativeF64Bits::from_bits(3.5_f64.to_bits()),
        ]
    );
    assert!(restored_trigger.multiplayer);
    assert_eq!(restored_trigger.side, -9);
    assert!(!restored_trigger.storage_flag_d1);
    assert_eq!(restored_trigger.secondary_team_type, None);
    assert_eq!(restored_trigger.difficulty_enabled, [true, false, true]);
    assert_eq!(
        restored_trigger.source,
        crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd
    );
    assert_eq!(
        team.response_suspension_state(),
        (true, true, true, 0, 1800)
    );
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn wave_active_inactive_dummy_links_roundtrip_with_identical_hash() {
    use crate::map::entities::EntityCategory;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::projectile::ProjectileCoord;
    use crate::sim::wave::{Wave, WaveRecordedCell, WaveUpdateContext};
    use crate::util::native_x87::NativeF64Bits;

    let mut sim = Simulation::new();
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let owner = sim.interner.intern("Americans");
    let type_ref = sim.interner.intern("DLPH");
    let mut owner_ids = Vec::new();
    for ordinal in 1..=3 {
        let stable_id = sim.substrate.allocate_stable_id();
        owner_ids.push(stable_id);
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            stable_id,
            ordinal,
            2,
            0,
            0,
            owner,
            Health::new(200),
            type_ref,
            EntityCategory::Unit,
            0,
            5,
            false,
        );
        entity.seldom.in_logic_vector.set_for_test(true);
        sim.substrate.entities.insert(entity);
        sim.substrate
            .logic
            .try_push(stable_id, crate::sim::world::ObjectKind::Entity)
            .expect("owner Logic registration");
    }

    let active_id = sim.substrate.allocate_stable_id();
    let active_target = crate::sim::combat::TargetKind::Cell(8, 2);
    let active_source = ProjectileCoord::new(256, 512, 0);
    let active_endpoint = ProjectileCoord::new(2048, 512, 0);
    let mut active = Wave::new_owned(
        0,
        owner_ids[0],
        active_target,
        active_source,
        active_endpoint,
    );
    assert!(!active.initialize(
        WaveUpdateContext {
            owner_position: Some(active_source),
            owner_fire_coordinate: Some(active_source),
            owner_current_target: Some(active_target),
            target_position: Some(active_endpoint),
            ares: None,
            target_is_unit: false,
        },
        None,
    ));
    let constructor_direction = active.direction_octant();
    let moved_endpoint = ProjectileCoord::new(0, 512, 0);
    let _ = active.advance(
        WaveUpdateContext {
            owner_position: Some(active_source),
            owner_fire_coordinate: Some(active_source),
            owner_current_target: Some(active_target),
            target_position: Some(moved_endpoint),
            ares: None,
            target_is_unit: false,
        },
        None,
    );
    assert_eq!(
        active.direction_octant(),
        constructor_direction,
        "live geometry refresh retains constructor-only +0x1CC"
    );
    let moved_edges = active.edge_geometry();
    sim.admit_wave(active_id, active);
    sim.link_active_wave_for_test(owner_ids[0], active_id);

    let inactive_id = sim.substrate.allocate_stable_id();
    let mut inactive = Wave::new_owned(
        0,
        owner_ids[1],
        crate::sim::combat::TargetKind::Cell(9, 2),
        ProjectileCoord::new(512, 512, 0),
        ProjectileCoord::new(2304, 512, 50),
    );
    inactive.set_active_geometry_for_test(false);
    inactive.set_decaying_for_test(true);
    inactive.set_fade_in_for_test(NativeF64Bits::from_bits(0x3fe0_0000_0000_0000));
    inactive.set_fade_out_for_test(NativeF64Bits::from_bits(0x3fc9_9999_a000_0000));
    inactive.set_direction_octant_for_test(6);
    inactive.replace_recorded_cells(vec![WaveRecordedCell::real(7, 2)]);
    sim.admit_wave(inactive_id, inactive);
    sim.link_active_wave_for_test(owner_ids[1], inactive_id);

    let dummy_id = sim.substrate.allocate_stable_id();
    let mut dummy = Wave::new_owned(
        0,
        owner_ids[2],
        crate::sim::combat::TargetKind::Cell(10, 2),
        ProjectileCoord::new(768, 512, 0),
        ProjectileCoord::new(2560, 512, 50),
    );
    dummy.set_active_geometry_for_test(false);
    dummy.set_decaying_for_test(true);
    dummy.set_direction_octant_for_test(2);
    dummy.replace_recorded_cells(vec![WaveRecordedCell::shared_dummy()]);
    sim.admit_wave(dummy_id, dummy);
    sim.link_active_wave_for_test(owner_ids[2], dummy_id);

    let expected_hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "wave-state.map", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("Wave snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("Wave identities restore structurally");

    assert_eq!(restored.state_hash(), expected_hash);
    assert_eq!(
        restored.active_wave_links().collect::<Vec<_>>(),
        sim.active_wave_links().collect::<Vec<_>>()
    );
    assert_eq!(restored.waves.get(active_id), sim.waves.get(active_id));
    assert_eq!(
        restored.waves.get(active_id).unwrap().direction_octant(),
        constructor_direction
    );
    assert_eq!(
        restored.waves.get(active_id).unwrap().edge_geometry(),
        moved_edges
    );
    assert_eq!(restored.waves.get(inactive_id), sim.waves.get(inactive_id));
    assert_eq!(restored.waves.get(dummy_id), sim.waves.get(dummy_id));
    assert_eq!(
        restored.waves.get(dummy_id).unwrap().recorded_cells(),
        vec![WaveRecordedCell::shared_dummy()],
    );
}

#[test]
fn gsi_01_04_pending_exit_command_roundtrips_without_terminal_edge() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    let hash_without_pending_input = sim.state_hash();
    sim.queue_command(CommandEnvelope::new(owner, 17, Command::ExitMatch));
    assert_eq!(
        sim.state_hash(),
        hash_without_pending_input,
        "pending EXIT follows the existing external-command hash convention"
    );
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 1, 2, "abort.map", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("v75 EXIT snapshot").sim;

    assert_eq!(
        restored.pending_command_snapshot(),
        sim.pending_command_snapshot()
    );
    assert!(!restored.quit_requested());
    assert_eq!(restored.take_executed_exit_owner(), None);
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn pending_game_speed_transition_roundtrips_in_current_version_and_executes_once() {
    use crate::sim::command::{Command, CommandEnvelope};
    use crate::sim::house_state::HouseState;

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    let hash_without_pending_input = sim.state_hash();
    sim.queue_command(CommandEnvelope::new(
        owner,
        1,
        Command::SetGameSpeed { speed: 4 },
    ));
    assert_eq!(sim.state_hash(), hash_without_pending_input);

    let bytes = GameSnapshot::save(&sim, 1, 2, "speed.map", 0);
    let header = GameSnapshot::read_header(&bytes).expect("current GameSpeed header");
    assert_eq!(header.layout, snapshot_layout());
    let mut restored = GameSnapshot::load(&bytes)
        .expect("current GameSpeed snapshot")
        .sim;
    assert_eq!(
        restored.pending_command_snapshot(),
        sim.pending_command_snapshot()
    );
    assert_eq!(restored.session.game_options().game_speed, 1);
    assert_eq!(restored.projected_in_game_options_speed(), Some(4));

    let due = restored.take_due_commands();
    let result = restored.advance_tick(&due, None, None, None, 67);
    assert_eq!(result.executed_commands, 1);
    assert_eq!(restored.session.game_options().game_speed, 4);
    assert!(restored.pending_command_snapshot().is_empty());
    assert_eq!(result.state_hash, Some(restored.state_hash()));

    let second = restored.advance_tick(&[], None, None, None, 67);
    assert_eq!(second.executed_commands, 0);
    assert_eq!(restored.session.game_options().game_speed, 4);
}

#[test]
fn pending_flag_to_die_snapshot_preserves_latch_shared_timer_and_hash() {
    use crate::sim::house_state::HouseState;
    let mut original = Simulation::with_seed(0);
    let owner = original.interner.intern("Americans");
    original
        .houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    let before = original.state_hash();
    assert!(original.flag_house_to_die(owner));
    assert_ne!(original.state_hash(), before);
    let saved_hash = original.state_hash();
    let bytes = GameSnapshot::save(&original, 11, 22, "", 33);
    let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    restored
        .restore_after_snapshot_load()
        .expect("pending destruction is valid");
    assert_eq!(restored.state_hash(), saved_hash);
    let house = restored.houses.get_mut(&owner).unwrap();
    assert!(house.is_to_die());
    assert_eq!(
        house.outcome_timer(),
        crate::sim::timer::CdTimer::started(0, 0)
    );
    assert!(house.outcome().is_none());
    assert!(house.take_due_to_die(0));
    assert!(!house.take_due_to_die(0));
    assert_ne!(
        restored.state_hash(),
        saved_hash,
        "latch itself is hash-relevant"
    );
}

#[test]
fn gsi_01_04_mid_savour_snapshot_preserves_remaining_frames_without_replaying_eva() {
    use crate::sim::house_state::{HouseOutcomeKind, HouseState};
    use crate::sim::world::SimSoundEvent;

    let mut original = Simulation::with_seed(0x104);
    original.session.set_tick_for_test(145);
    original.session.set_binary_frame_for_test(145);
    let owner = original.interner.intern("Americans");
    original
        .houses
        .register(HouseState::new(owner, 0, None, true, 10_000, 10));
    let before_outcome = original.state_hash();
    assert!(
        original
            .houses
            .get_mut(&owner)
            .expect("house")
            .accept_win_result(100, 90, false)
    );
    assert_ne!(
        original.state_hash(),
        before_outcome,
        "accepted outcome and Savour target are hash-relevant"
    );
    let accepted_hash = original.state_hash();
    original
        .houses
        .get_mut(&owner)
        .expect("house")
        .supply_allow_win_tag_state(0, crate::sim::timer::CdTimer::started(100, 91));
    assert_ne!(
        original.state_hash(),
        accepted_hash,
        "the shared result timer itself is hash-relevant"
    );
    original
        .houses
        .get_mut(&owner)
        .expect("house")
        .supply_allow_win_tag_state(0, crate::sim::timer::CdTimer::started(100, 90));
    original.sound_events.emit(SimSoundEvent::MatchOutcome {
        owner,
        kind: HouseOutcomeKind::Victory,
    });
    // Full snapshot load resets Scenario RNG to Seed0; compare hashes on
    // that canonical post-load cursor.
    *original.scenario_rng = crate::sim::rng::SimRng::new(0);
    let saved_hash = original.state_hash();
    let bytes = GameSnapshot::save(&original, 11, 22, "", 33);
    let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;

    assert!(
        restored.sound_events.is_empty(),
        "the already-issued outcome EVA edge must not survive save/load"
    );
    restored
        .restore_after_snapshot_load()
        .expect("mid-Savour outcome state is structurally valid");
    assert_eq!(restored.state_hash(), saved_hash);

    let restored_house = restored.houses.get(&owner).expect("restored house");
    let outcome = restored_house.outcome().expect("accepted outcome");
    assert_eq!(outcome.kind, HouseOutcomeKind::Victory);
    assert!(restored.session().outcome_request().is_none());
    assert_eq!(
        restored_house
            .outcome_timer()
            .remaining(restored.session.binary_frame() as i32),
        45
    );

    let outcome_policy = restored.session().rule_policy();
    assert_eq!(restored_house.pending_outcome(189, outcome_policy), None);
    assert_eq!(
        restored_house.pending_outcome(190, outcome_policy),
        Some(HouseOutcomeKind::Victory)
    );
}

#[test]
fn gsi_01_04_conflicting_native_house_result_flags_are_rejected() {
    use crate::sim::house_state::HouseState;

    for (won, lost, to_die) in [
        (true, true, false),
        (true, false, true),
        (false, true, true),
    ] {
        let mut malformed = Simulation::new();
        let owner = malformed.interner.intern("Americans");
        let mut house = HouseState::new(owner, 0, None, true, 10_000, 10);
        if to_die {
            assert!(house.flag_to_die(0));
        }
        house.supply_result_flags_for_test(won, lost, crate::sim::timer::CdTimer::default());
        malformed.houses.register(house);

        let bytes = GameSnapshot::save(&malformed, 11, 22, "", 33);
        let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
        assert_eq!(
            restored.restore_after_snapshot_load(),
            Err(SnapshotRestoreError::InvalidHouseOutcomeState {
                owner,
                reason: if to_die {
                    "Flag_To_Die conflicts with a terminal result"
                } else {
                    "Won and Lost flags conflict"
                },
            }),
            "FlagWin/FlagLose cannot produce conflicting House flags"
        );
    }
}

#[test]
fn defeated_house_without_a_result_restores() {
    use crate::sim::house_state::HouseState;

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Americans");
    let mut house = HouseState::new(owner, 0, None, false, 10_000, 10);
    house.set_is_defeated_for_test(true);
    sim.houses.register(house);
    let bytes = GameSnapshot::save(&sim, 11, 22, "", 33);
    let mut restored = GameSnapshot::load(&bytes).expect("current snapshot").sim;
    assert_eq!(restored.restore_after_snapshot_load(), Ok(()));
    assert!(restored.houses[&owner].is_defeated());
    assert!(restored.houses[&owner].outcome().is_none());
}

#[test]
fn gsi_04_20_mid_fade_lighting_roundtrip_preserves_hash_and_next_rung() {
    let mut original = Simulation::with_seed(0x420);
    original.session.set_binary_frame_for_test(20);
    original
        .session
        .lighting_mut()
        .normal_mut_for_test()
        .red_percent = 93;
    original
        .session
        .lighting_mut()
        .ion_mut_for_test()
        .blue_percent = 71;
    original
        .session
        .lighting_mut()
        .set_current_ambient_for_test(95);
    original
        .session
        .lighting_mut()
        .set_target_ambient_for_test(87);
    original
        .session
        .lighting_mut()
        .set_selected_profile_for_test(crate::sim::scenario_session::ScenarioLightingProfile::Ion);
    original
        .session
        .lighting_mut()
        .set_transition_timer_for_test(crate::sim::timer::CdTimer::from_raw(17, 3));

    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // lighting persistence on that same post-load cursor.
    *original.scenario_rng = crate::sim::rng::SimRng::new(0);
    let saved_hash = original.state_hash();
    let bytes = GameSnapshot::save(&original, 11, 22, "", 33);
    let mut restored = GameSnapshot::load(&bytes)
        .expect("v65 lighting snapshot")
        .sim;

    assert_eq!(restored.session.lighting(), original.session.lighting());
    assert_eq!(restored.state_hash(), saved_hash);

    let original_due = original
        .session
        .lighting_mut()
        .advance_transition_if_due(20, true, 180, 20);
    let restored_due = restored
        .session
        .lighting_mut()
        .advance_transition_if_due(20, true, 180, 20);
    assert_eq!(restored_due, original_due);
    assert_eq!(restored.session.lighting(), original.session.lighting());
    assert_eq!(restored.state_hash(), original.state_hash());
}

/// `HouseClass+0x242` rides the raw House block in the native save; a
/// v132 snapshot carries it and the restored hash still matches.
#[test]
fn house_harvester_no_ore_latch_roundtrips_at_v132() {
    let mut sim = Simulation::with_seed(0x242);
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 5_000, 10);
    house.set_harvester_no_ore_for_test(true);
    sim.houses.register(house);
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "house_no_ore_latch", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        super::snapshot_layout()
    );
    let restored = GameSnapshot::load(&bytes)
        .expect("v132 house latch snapshot")
        .sim;
    assert!(restored.houses[&owner].harvester_no_ore());
    assert_eq!(restored.state_hash(), expected_hash);
}

/// `HouseClass+0x57D4` and the low-power guard ride the House block in
/// the native save; a v133 snapshot carries them and the restored hash
/// still matches.
#[test]
fn house_eva_advice_state_roundtrips_at_v133() {
    let mut sim = Simulation::with_seed(0x57D4);
    let owner = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, None, true, 5_000, 10);
    house.set_eva_funds_timer_for_test(crate::sim::timer::CdTimer::started(1_234, 2_880));
    house.set_eva_low_power_guard_for_test(true);
    sim.houses.register(house);
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "house_eva_advice", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        super::snapshot_layout()
    );
    let restored = GameSnapshot::load(&bytes)
        .expect("v133 house EVA snapshot")
        .sim;
    assert_eq!(
        *restored.houses[&owner].eva_funds_timer(),
        crate::sim::timer::CdTimer::started(1_234, 2_880)
    );
    assert!(restored.houses[&owner].eva_low_power_guard());
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn gsi_04_18_spy_sat_latch_and_erased_map_knowledge_roundtrip_at_v65() {
    let mut sim = Simulation::with_seed(0x418);
    sim.fog.set_width_for_test(12);
    sim.fog.set_height_for_test(12);
    let owner = sim.interner.intern("Soviet");
    let gapper = sim.interner.intern("Americans");
    let mut house = crate::sim::house_state::HouseState::new(owner, 1, None, true, 10_000, 10);
    house.set_spy_sat_active_for_test(true);
    house.set_map_is_clear_for_test(true);
    sim.houses.register(house);
    sim.fog.reveal_all_for_owner(owner);
    crate::sim::vision::apply_gap_generators(
        &mut sim.fog,
        &sim.house_alliances,
        &[(gapper, 6, 6, 2)],
        &sim.interner,
    );
    assert!(sim.fog.is_cell_revealed(owner, 0, 0));
    assert!(!sim.fog.is_cell_revealed(owner, 6, 6));
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // SpySat/map-knowledge persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "gsi_04_18_shroud", 0);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().layout,
        super::snapshot_layout()
    );
    let restored = GameSnapshot::load(&bytes).expect("v65 SpySat snapshot").sim;

    assert!(restored.houses[&owner].spy_sat_active());
    assert!(restored.houses[&owner].map_is_clear());
    assert!(restored.fog.is_cell_revealed(owner, 0, 0));
    assert!(!restored.fog.is_cell_revealed(owner, 6, 6));
    assert_eq!(restored.state_hash(), expected_hash);
}

#[test]
fn house_difficulty_round_trips_through_snapshot() {
    use crate::sim::house_state::{HouseDifficulty, HouseState};

    let mut sim = Simulation::new();
    let owner = sim.interner.intern("Computer1");
    let mut house = HouseState::new(owner, 0, None, false, 0, 10);
    house.set_difficulty_for_test(HouseDifficulty::Easy);
    sim.houses.register(house);
    // Native in-scenario load restarts Scenario RNG from Seed0; isolate
    // house-difficulty persistence on that same post-load cursor.
    *sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let expected_hash = sim.state_hash();

    let bytes = GameSnapshot::save(&sim, 0, 0, "test_map", 0);
    let loaded = GameSnapshot::load(&bytes).expect("load should succeed");

    assert_eq!(
        loaded
            .sim
            .houses
            .get(&owner)
            .map(|house| house.difficulty()),
        Some(HouseDifficulty::Easy),
    );
    assert_eq!(loaded.sim.state_hash(), expected_hash);
}

/// Issue #1949. Retail witnesses register an extra hostile House beside
/// the admitted scenario Houses before saving (Dustbowl defences, the
/// Hills bridge layers). The native House array holds every entry with
/// the bounded name `FromName50C170` reads, so such a registration must
/// install that name and its array membership, and snapshot401 must
/// restore the mixed scenario. A registration that skips the name stays
/// rejectable: the admitted-scenario array invariant is native, not a
/// test convenience.
#[test]
fn additionally_registered_scenario_house_round_trips_through_snapshot() {
    use crate::sim::house_state::HouseState;

    let mut sim = Simulation::with_seed(0x1949);
    let launch = sim.interner.intern("<human player>");
    let mut launch_house = HouseState::new(launch, 0, None, true, 10_000, 10);
    launch_house.initialize_scenario_name("<human player>");
    sim.houses.register(launch_house);
    sim.session.set_current_house_for_test(Some(launch));
    let hostile = sim.interner.intern("Russians");
    let mut hostile_house = HouseState::new(hostile, 1, None, true, 0, 10);
    hostile_house.initialize_scenario_name("Russians");
    sim.houses.register(hostile_house);

    let bytes = GameSnapshot::save(&sim, 0, 0, "mixed-registered-houses", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("load should succeed").sim;
    restored
        .restore_after_snapshot_load()
        .expect("mixed admitted and additionally registered Houses restore");
    assert_eq!(
        Simulation::scenario_house_named(&restored.houses, "Russians"),
        Some(hostile)
    );
    assert_eq!(
        restored.houses[&hostile].native_scenario_name(),
        Some("Russians")
    );

    // The pre-fix witness shape — the same extra House without its
    // native name — is the state the validation must keep rejecting.
    let mut nameless = Simulation::with_seed(0x1949);
    let launch = nameless.interner.intern("<human player>");
    let mut launch_house = HouseState::new(launch, 0, None, true, 10_000, 10);
    launch_house.initialize_scenario_name("<human player>");
    nameless.houses.register(launch_house);
    let hostile = nameless.interner.intern("Russians");
    nameless
        .houses
        .register(HouseState::new(hostile, 1, None, true, 0, 10));
    let bytes = GameSnapshot::save(&nameless, 0, 0, "nameless-registered-house", 0);
    let mut rejected = GameSnapshot::load(&bytes).expect("load should succeed").sim;
    assert!(matches!(
        rejected.restore_after_snapshot_load(),
        Err(SnapshotRestoreError::InvalidHouseScenarioName { owner, .. }) if owner == hostile
    ));
}

/// A team counts its members in six TaskForce slots by entry position, so a
/// saved TaskForce with a seventh entry is refused as a damaged save before
/// recruitment can index past the slots.
#[test]
fn a_saved_task_force_past_the_team_slots_is_refused() {
    use crate::sim::team_script_vm::{
        TASK_FORCE_SLOTS, TeamMemberTypeIdentity, TeamTaskForceDefinition, TeamTaskForceEntry,
    };
    let mut sim = Simulation::new();
    let entry = TeamTaskForceEntry {
        member_type: TeamMemberTypeIdentity {
            category: crate::rules::object_type::ObjectCategory::Infantry,
            id: sim.interner.intern("E1"),
        },
        count: 1,
    };
    sim.team_script_vm
        .register_task_force(TeamTaskForceDefinition {
            id: sim.interner.intern("WIDE"),
            source: crate::rules::team_ai_ini::TeamAiDefinitionSource::FixedAimd,
            group: -1,
            entries: vec![entry; TASK_FORCE_SLOTS + 1],
        });
    let refusal = sim.restore_after_snapshot_load().unwrap_err();
    assert!(matches!(
        refusal,
        SnapshotRestoreError::InvalidTeamState { .. }
    ));
    assert!(refusal.to_string().starts_with("this save is damaged"));
}
