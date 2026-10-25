//! Engineer WhatAction51E5E8..51E656 -> Foot clicked Enter7/Eaten9.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::mission::MissionType;
use crate::sim::world::Simulation;

fn scene(hospital: bool) -> (Simulation, RuleSet, u64, HoverTargetKindWithId) {
    scene_with_repair_step(hospital, 20)
}

fn scene_with_repair_step(
    hospital: bool,
    repair_step: i32,
) -> (Simulation, RuleSet, u64, HoverTargetKindWithId) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[General]\nIRepairStep={repair_step}\nIRepairRate=.001\n\
         [Countries]\n0=Americans\n[InfantryTypes]\n0=ENGINEER\n[BuildingTypes]\n0=FACILITY\n\
         [ENGINEER]\nImage=GI\nStrength=75\nEngineer=yes\nSpeed=4\nCost=500\nSoylent=500\n\
         Locomotor={{4A582744-9839-11D1-B709-00A024DDAFD1}}\n\
         [FACILITY]\nStrength=1000\nRepairable=yes\nHospital={}\nGrinding={}\n",
        if hospital { "yes" } else { "no" },
        if hospital { "no" } else { "yes" },
    ));
    let art = IniFile::from_str(&format!(
        "{}\n[FACILITY]\nFoundation=1x1\n",
        crate::rules::retail_ini_fixture::GI_ART_EXCERPT
    ));
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap();
    let mut sim = Simulation::with_seed(2022);
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.intern("Americans");
    sim.houses_mut_for_test()
        .register(HouseState::new(owner, 0, None, true, 10_000, 10));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.set_input_delay_ticks(0);
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .short_game = false;
    let actor = sim
        .spawn_object("ENGINEER", "Americans", 10, 12, 0, &rules)
        .unwrap();
    if hospital {
        sim.entities_mut_for_test()
            .get_mut(actor)
            .unwrap()
            .health
            .set_for_test(30);
    }
    let target = sim
        .spawn_object("FACILITY", "Americans", 16, 12, 0, &rules)
        .unwrap();
    (
        sim,
        rules,
        actor,
        HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyStructure,
            stable_id: target,
        },
    )
}

fn step(sim: &mut Simulation, rules: &RuleSet) -> crate::sim::world::SimFrameOutput {
    let commands = sim.take_due_commands();
    sim.advance_app_frame(
        &commands,
        rules,
        None,
        33,
        crate::sim::world::TickLane::Ordinary,
        Default::default(),
    )
    .unwrap()
}

fn click(
    sim: &mut Simulation,
    rules: &RuleSet,
    actor: u64,
    hover: &HoverTargetKindWithId,
    planning: bool,
) {
    let owner = sim.entities().get(actor).unwrap().owner();
    let mut selected = vec![actor];
    let orders = engineer_capture_orders(sim, rules, &mut selected, hover).unwrap();
    assert_eq!(orders.len(), 1);
    let record = if planning {
        sim.encode_planning_order_record(owner, &orders[0], None)
    } else {
        sim.encode_megamission_record(owner, &orders[0])
    }
    .unwrap();
    let command = sim
        .decode_native_command_record(&record, sim.session().tick())
        .unwrap();
    assert!(sim.apply_command("Americans", &command.payload, Some(rules)));
    if planning {
        let record = sim.encode_plan_commit_record(owner).unwrap();
        let command = sim
            .decode_native_command_record(&record, sim.session().tick())
            .unwrap();
        assert!(sim.apply_command("Americans", &command.payload, Some(rules)));
    }
}

#[test]
fn ordinary_and_planned_grinder_clicks_consume_engineers_and_refund_once() {
    for planning in [false, true] {
        let (mut sim, rules, actor, hover) = scene(false);
        let owner = sim.entities().get(actor).unwrap().owner();
        let before = sim.houses()[&owner].economy().credits();
        click(&mut sim, &rules, actor, &hover, planning);
        for _ in 0..600 {
            step(&mut sim, &rules);
            if sim.entities().get(actor).is_none() {
                break;
            }
        }
        assert!(sim.entities().get(actor).is_none(), "planning={planning}");
        assert_eq!(sim.houses()[&owner].economy().credits(), before + 500);
        assert!(sim.planning_paths(owner).is_empty());
        for _ in 0..10 {
            step(&mut sim, &rules);
        }
        assert_eq!(sim.houses()[&owner].economy().credits(), before + 500);
    }
}

#[test]
fn ordinary_and_planned_hospital_clicks_enter_heal_and_exit_after_save_load() {
    for planning in [false, true] {
        let (mut sim, rules, actor, hover) = scene(true);
        let owner = sim.entities().get(actor).unwrap().owner();
        let before = sim.houses()[&owner].economy().credits();
        assert_eq!(rules.object("FACILITY").unwrap().passengers, 0);
        click(&mut sim, &rules, actor, &hover, planning);
        let mut entered = false;
        let mut left = false;
        for _ in 0..1000 {
            step(&mut sim, &rules);
            let patient = sim
                .entities()
                .get(actor)
                .expect("treatment must not consume an engineer");
            if !entered && patient.passenger_role.inside_transport_id() == Some(hover.stable_id) {
                entered = true;
                assert!(patient.in_limbo());
                assert!(patient.has_live_contact_with(hover.stable_id));
                let terrain = sim.resolved_terrain().unwrap().clone();
                let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "hospital", 0);
                sim = crate::sim::snapshot::GameSnapshot::load(&bytes)
                    .unwrap()
                    .sim;
                sim.restore_after_snapshot_load().unwrap();
                sim.resolve_type_handles(&rules);
                // Restore the same external32x32 map, not supply_native_map's
                // default64x64 map (the snapshot's overlay plane is32x32).
                *sim.resolved_terrain_mut_for_test() = Some(terrain);
                crate::sim::arena_fixture::supply_native_map(&mut sim);
                assert!(sim.rebuild_dynamic_navigation(&rules));
            } else if entered
                && !patient.in_limbo()
                && patient.passenger_role.inside_transport_id().is_none()
            {
                assert_eq!(patient.health.current(), 75);
                left = true;
                break;
            }
        }
        assert!(
            entered && left,
            "planning={planning}: entered={entered}, left={left}"
        );
        assert_eq!(
            sim.houses()[&owner].economy().credits(),
            before,
            "infantry repair is free"
        );
        assert_eq!(
            sim.entities()
                .get(hover.stable_id)
                .unwrap()
                .passenger_role
                .passenger_count(),
            0
        );
        assert!(sim.planning_paths(owner).is_empty());
    }
}

#[test]
fn damaged_grinders_keep_the_repair_action_and_healthy_patients_do_not_enter() {
    let (mut sim, rules, actor, hover) = scene(false);
    sim.entities_mut_for_test()
        .get_mut(hover.stable_id)
        .unwrap()
        .health
        .set_for_test(500);
    let orders = engineer_capture_orders(&sim, &rules, &mut vec![actor], &hover).unwrap();
    assert_eq!(
        orders,
        vec![Command::CaptureBuilding {
            engineer_id: actor,
            target_building_id: hover.stable_id,
        }]
    );
    let (mut sim, rules, actor, hover) = scene(true);
    sim.entities_mut_for_test()
        .get_mut(actor)
        .unwrap()
        .health
        .set_for_test(75);
    assert_ne!(
        sim.engineer_building_action(actor, hover.stable_id, &rules),
        Some(crate::sim::world::EngineerBuildingAction::EnterHospital)
    );
}

#[test]
fn hospital_can_load_preserves_native_ammo_admission() {
    use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};
    //Original43C607..C61D: exactly0 refuses; positive and unlimited counts
    //admit. This query is separate from the clicked Enter's CAN_DOCK0E.
    for ammo in [0, 1, -1] {
        let (mut sim, rules, actor, hover) = scene(true);
        sim.entities_mut_for_test()
            .get_mut(hover.stable_id)
            .unwrap()
            .install_ammo_for_test(Some(crate::sim::ammo::TechnoAmmo::new(ammo)));
        assert_eq!(
            transmit(
                &mut sim,
                actor,
                hover.stable_id,
                RadioMessage::CanEnter,
                RadioPayload::default(),
                &rules,
                Default::default()
            ),
            if ammo == 0 {
                RadioResponse::Negatory
            } else {
                RadioResponse::Roger
            }
        );
    }
}

#[test]
fn hospital_completion_uses_the_native_player_house_voice_gate() {
    for local in [false, true] {
        let (mut sim, rules, actor, hover) = scene(true);
        click(&mut sim, &rules, actor, &hover, false);
        if !local {
            let other = sim.intern("OtherHuman");
            sim.houses_mut_for_test()
                .register(HouseState::new(other, 0, None, true, 10_000, 10));
            // A House holding nothing is defeated on its first House update.
            sim.spawn_object("ENGINEER", "OtherHuman", 2, 2, 0, &rules)
                .unwrap();
            sim.session_mut_for_test()
                .set_current_house_for_test(Some(other));
        }
        let mut heard_completion = false;
        for _ in 0..500 {
            let output = step(&mut sim, &rules);
            heard_completion |= output.sound_events.iter().any(|event| {
                matches!(event, crate::sim::world::SimSoundEvent::UnitRepaired { .. })
            });
        }
        assert_eq!(sim.entities().get(actor).unwrap().health.current(), 75);
        assert_eq!(heard_completion, local, "44B93F calls IsHumanPlayer50B6F0");
    }
}

#[test]
fn stopping_before_entry_does_not_grind_or_hide_the_engineer() {
    for hospital in [false, true] {
        for planning in [false, true] {
            let (mut sim, rules, actor, hover) = scene(hospital);
            click(&mut sim, &rules, actor, &hover, planning);
            for _ in 0..2 {
                step(&mut sim, &rules);
            }
            assert!(sim.apply_command(
                "Americans",
                &Command::Stop { entity_id: actor },
                Some(&rules)
            ));
            for _ in 0..300 {
                step(&mut sim, &rules);
            }
            let engineer = sim.entities().get(actor).unwrap();
            assert!(
                !engineer.in_limbo(),
                "hospital={hospital}, planning={planning}"
            );
            assert!(engineer.passenger_role.inside_transport_id().is_none());
        }
    }
}

#[test]
fn destroying_a_hospital_cleans_up_its_hidden_patient() {
    let (mut sim, rules, actor, hover) = scene(true);
    click(&mut sim, &rules, actor, &hover, false);
    for _ in 0..600 {
        step(&mut sim, &rules);
        if sim.entities().get(actor).unwrap().in_limbo() {
            sim.uninit_with_context(
                hover.stable_id,
                crate::sim::world::UninitContext::with_rules(&rules),
            );
            //UnInit makes the objects inactive; the frame's existing
            //retirement phase removes their retained storage.
            assert!(
                !sim.entities()
                    .get(hover.stable_id)
                    .unwrap()
                    .is_object_alive()
            );
            for _ in 0..2 {
                step(&mut sim, &rules);
            }
            assert!(sim.entities().get(hover.stable_id).is_none());
            assert!(sim.entities().get(actor).is_none());
            return;
        }
    }
    panic!("patient never entered the hospital");
}

#[test]
fn infantry_repair_tick_clamps_nonpositive_steps_to_one() {
    use crate::sim::radio::{RadioMessage, RadioPayload, RadioResponse, transmit};

    // Techno6F4D1C clamps the virtual InfantryType524790 getter result to1.
    for repair_step in [0, -3] {
        let (mut sim, rules, actor, hover) = scene_with_repair_step(true, repair_step);
        let owner = sim.entities().get(actor).unwrap().owner();
        let before = sim.houses()[&owner].economy().credits();
        assert_eq!(rules.general.infantry_repair_step, repair_step);
        assert_eq!(
            transmit(
                &mut sim,
                hover.stable_id,
                actor,
                RadioMessage::RepairTick,
                RadioPayload::default(),
                &rules,
                Default::default(),
            ),
            RadioResponse::Roger
        );
        assert_eq!(sim.entities().get(actor).unwrap().health.current(), 31);
        assert_eq!(sim.houses()[&owner].economy().credits(), before);
    }
}

#[test]
fn infantry_repair_keys_have_constructor_defaults_and_unclamped_native_reads() {
    let defaults = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    assert_eq!(defaults.general.infantry_repair_step, 1);
    assert_eq!(
        defaults.general.infantry_repair_rate.to_bits(),
        0.016f64.to_bits()
    );
    let custom = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nIRepairStep=-3\nIRepairRate=.001\n",
    ))
    .unwrap();
    assert_eq!(custom.general.infantry_repair_step, -3);
    assert_eq!(custom.general.infantry_repair_rate, f64::from(0.001f32));
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    assert_eq!(retail.rules.general.infantry_repair_step, 20);
    assert_eq!(
        retail.rules.general.infantry_repair_rate,
        f64::from(0.001f32)
    );
    assert!(!retail.rules.object("CATHOSP").unwrap().hospital);
    assert!(
        retail
            .rules
            .object("CATHOSP")
            .unwrap()
            .infantry_gain_self_heal
            > 0
    );
}

#[test]
fn engineer_hospital_and_grinder_clicks_produce_the_native_entry_missions() {
    for hospital in [false, true] {
        let (sim, rules, actor, hover) = scene(hospital);
        assert!(matches!(
            sim.engineer_building_action(actor, hover.stable_id, &rules),
            Some(
                crate::sim::world::EngineerBuildingAction::EnterHospital
                    | crate::sim::world::EngineerBuildingAction::EnterGrinder
            )
        ));
        let mut selected = vec![actor];
        let orders = engineer_capture_orders(&sim, &rules, &mut selected, &hover).unwrap();
        assert_eq!(
            orders,
            vec![Command::MegaMission {
                entity_id: actor,
                mission: if hospital {
                    MissionType::Enter
                } else {
                    MissionType::Eaten
                },
                target: None,
                destination: Some(TargetKind::Entity(hover.stable_id))
            }]
        );
        assert!(selected.is_empty());
        assert_eq!(command_actor_id(&orders[0]), Some(actor));
        assert_eq!(
            order_voice_key(&orders[0]),
            Some(if hospital {
                "VoiceEnter"
            } else {
                "VoiceSpecialAttack"
            })
        );
    }
}

#[test]
fn retail_engineer_grinder_clicks_reach_yagrnd_in_both_input_modes() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    for planning in [false, true] {
        let mut sim = Simulation::with_seed(2022);
        crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.intern("Americans");
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, 0, None, true, 10_000, 10));
        sim.session_mut_for_test()
            .set_current_house_for_test(Some(owner));
        sim.session_mut_for_test().set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true),
        );
        sim.session_mut_for_test()
            .game_options_mut_for_test()
            .short_game = false;
        sim.set_input_delay_ticks(0);
        let actor = sim
            .spawn_object("ENGINEER", "Americans", 10, 12, 0, &rules)
            .unwrap();
        let target = sim
            .spawn_object("YAGRND", "Americans", 16, 12, 0, &rules)
            .unwrap();
        let hover = HoverTargetKindWithId {
            kind: HoverTargetKind::FriendlyStructure,
            stable_id: target,
        };
        click(&mut sim, &rules, actor, &hover, planning);
        for _ in 0..1000 {
            step(&mut sim, &rules);
            if sim.entities().get(actor).is_none() {
                break;
            }
        }
        assert!(sim.entities().get(actor).is_none(), "planning={planning}");
        assert_eq!(
            sim.houses()[&owner].economy().credits(),
            10_000 + rules.object("ENGINEER").unwrap().soylent
        );
    }
}
