//! Stock retail absorber entry followed by the existing window-independent
//! selection/D-command producer. Arena/session/visibility are supplied inputs;
//! this does not execute a window message, physical key or native whole tick.

use super::*;
use crate::rules::{ini_parser::IniFile, overlay_types::OverlayTypeRegistry};
use crate::sim::runtime::{SimResources, SimRuntime};
use crate::sim::world::{FrameEffects, TickLane};

fn retail_occupant_runtime(seed: u64) -> Option<SimRuntime> {
    let mut retail = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "Hills.mmx",
        &IniFile::from_str(""),
    )?;
    let (_, mut assets) = crate::rules::retail_ini_fixture::retail_assets()?;
    let theater = crate::map::theater::load_theater(&mut assets, "TEMPERATE").unwrap();
    let roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&retail.rules);
    retail
        .rules
        .bind_anim_class_assets(&roots, &assets, theater.extension, "TEMPERATE");
    retail
        .rules
        .bind_effect_assets(&assets, theater.extension, "TEMPERATE");
    let registry = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let rules = retail.rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, seed);
    sim.install_overlay_grid_for_load(crate::sim::overlay_grid::OverlayGrid::new(32, 32));
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .short_game = false;
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    for name in ["Americans", "Russians"] {
        let house = sim.interner().get(name).unwrap();
        sim.houses_mut_for_test()
            .get_mut(&house)
            .unwrap()
            .set_current_iq_for_test(0);
        sim.fog_mut_for_test().reveal_all_for_owner(house);
    }
    let terrain_template = sim.resolved_terrain().cloned();
    Some(SimRuntime {
        simulation: sim,
        resources: SimResources {
            rules,
            overlay_registry: registry,
            terrain_template,
        }
        .into(),
    })
}

#[test]
fn deploy_key_on_a_selected_loaded_bio_reactor_does_not_unload_absorber_cargo() {
    let Some(mut runtime) = retail_occupant_runtime(0x730af0) else {
        return;
    };
    let reactor_type = runtime.resources.rules.object("YAPOWR").unwrap();
    assert!(reactor_type.infantry_absorb);
    assert!(!reactor_type.can_be_occupied);
    let owner = runtime.simulation.interner().get("Americans").unwrap();
    let reactor = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "YAPOWR",
            "Americans",
            16,
            14,
            0,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .unwrap();
    runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "NAPOWR",
            "Russians",
            28,
            28,
            0,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .unwrap();
    let initiate = runtime
        .simulation
        .spawn_object_with_overlay_registry(
            "INIT",
            "Americans",
            12,
            14,
            0,
            &runtime.resources.rules,
            &runtime.resources.overlay_registry,
        )
        .unwrap();
    assert!(runtime.apply_command_for_test(
        "Americans",
        &Command::clicked_mission(
            initiate,
            crate::sim::mission::MissionType::Enter,
            crate::sim::combat::TargetKind::Entity(reactor),
        )
    ));
    for _ in 0..600 {
        let due = runtime.simulation.take_due_commands();
        let output = runtime
            .advance_frame(&due, 33, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(output.tick.frame_committed);
        if runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .passenger_role
            .inside_transport_id()
            == Some(reactor)
        {
            break;
        }
    }
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .passenger_role
            .inside_transport_id(),
        Some(reactor)
    );
    assert!(
        runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert!(runtime.apply_command_for_test(
        "Americans",
        &Command::Select {
            entity_ids: vec![reactor],
            additive: false
        }
    ));
    assert!(
        runtime
            .simulation
            .entities()
            .get(reactor)
            .unwrap()
            .selection
            .is_selected()
    );
    // Physical DeployObject=D reaches Selection730AF0. Its special Building
    // arm queries the garrison vector, then ordinary Building465D30 refuses.
    // Cargo-only Bio Reactor self-click Unload is a separate legal producer.
    let commands = deploy_key_commands(
        &runtime.simulation,
        Some(&runtime.resources.rules),
        &[reactor],
        Some(owner),
    );
    assert!(
        commands.is_empty(),
        "D must not turn absorber Cargo into garrison occupants"
    );
    let due = runtime.simulation.take_due_commands();
    let output = runtime
        .advance_frame(&due, 33, TickLane::Ordinary, FrameEffects::default())
        .unwrap();
    assert!(output.tick.frame_committed);
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(reactor)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .passengers(),
        [initiate]
    );
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .passenger_role
            .inside_transport_id(),
        Some(reactor)
    );
    assert!(
        runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    // The same physical loaded object still admits its independent self-click
    // Unload through Building447330/44353F and actually releases its Cargo.
    let hover = crate::app::input::entity_pick::HoverTargetKindWithId {
        kind: crate::app::types::HoverTargetKind::FriendlyStructure,
        stable_id: reactor,
    };
    let clicked = crate::app::input::context_order::self_click_command(
        &runtime.simulation,
        &runtime.resources.rules,
        Some(&runtime.resources.overlay_registry),
        owner,
        &[reactor],
        Some(&hover),
        false,
    )
    .expect("loaded stock Bio Reactor self-click remains legal");
    assert_eq!(
        clicked,
        Command::UnloadPassengers {
            transport_id: reactor
        }
    );
    assert!(runtime.apply_command_for_test("Americans", &clicked));
    for _ in 0..180 {
        let due = runtime.simulation.take_due_commands();
        let output = runtime
            .advance_frame(&due, 33, TickLane::Ordinary, FrameEffects::default())
            .unwrap();
        assert!(output.tick.frame_committed);
        if runtime
            .simulation
            .entities()
            .get(reactor)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
        {
            break;
        }
    }
    assert!(
        runtime
            .simulation
            .entities()
            .get(reactor)
            .unwrap()
            .passenger_role
            .cargo()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .passenger_role
            .inside_transport_id(),
        None
    );
    assert!(
        !runtime
            .simulation
            .entities()
            .get(initiate)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
}

/// Native Selection730B5C emits Event9 for its first occupied Building.
/// Event4C77F8/4C7804 clears destination/target before Queue Unload. Check
/// the actual admitted D command's authoritative result, not just its opcode.
#[test]
fn retail_garrison_deploy_key_clears_target_before_releasing_real_occupants() {
    use crate::sim::command::CommandEnvelope;
    use crate::sim::house_state::{HouseState, resolve_multiplay_passive};

    fn issue(runtime: &mut SimRuntime, payload: Command) {
        let sim = &mut runtime.simulation;
        let owner = sim.interner().get("Americans").unwrap();
        let envelope = super::super::commands::encode_ordinary_local_command(
            sim,
            CommandEnvelope::new(owner, sim.session().tick() + 1, payload),
        )
        .expect("the ordinary command has a native record");
        assert!(
            sim.issue_local_command(envelope),
            "the actual native queue admits the order"
        );
    }
    fn step(runtime: &mut SimRuntime) -> crate::sim::world::TickResult {
        let due = runtime.simulation.take_due_commands();
        let output = runtime
            .advance_frame(&due, 33, TickLane::Ordinary, FrameEffects::default())
            .expect("ordinary retail garrison frame");
        assert!(output.tick.frame_committed);
        output.tick
    }

    // Original components observe Event9 itself, not the whole app frame.
    // Component clock values are not app-frame expectations.
    let native = crate::test_fixture::json("tools/spatial_oracle/building_garrison_deploy.json");
    for fired_before_d in [true, false] {
        let control_name = if fired_before_d {
            "existing_cell_target"
        } else {
            "null_target"
        };
        let expected = native["receiver"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["name"] == control_name)
            .unwrap();
        let Some(mut runtime) = retail_occupant_runtime(0x457de0) else {
            return;
        };
        let owner = runtime.simulation.interner().get("Americans").unwrap();
        let neutral = runtime.simulation.intern("Neutral");
        let mut neutral_house = HouseState::new(neutral, 2, Some(neutral), false, 0, 10);
        neutral_house.set_multiplay_passive_for_test(resolve_multiplay_passive(
            &runtime.resources.rules,
            Some("Neutral"),
        ));
        assert!(
            neutral_house.multiplay_passive(),
            "retail Neutral country input"
        );
        runtime
            .simulation
            .houses_mut_for_test()
            .register(neutral_house);
        let house = runtime
            .simulation
            .spawn_object_with_overlay_registry(
                "CAHSE01",
                "Neutral",
                16,
                14,
                0,
                &runtime.resources.rules,
                &runtime.resources.overlay_registry,
            )
            .expect("ordinary stock civilian house");
        let gi = runtime
            .simulation
            .spawn_object_with_overlay_registry(
                "E1",
                "Americans",
                12,
                14,
                0,
                &runtime.resources.rules,
                &runtime.resources.overlay_registry,
            )
            .expect("real GI with its Walk locomotor");
        runtime
            .simulation
            .spawn_object_with_overlay_registry(
                "NAPOWR",
                "Russians",
                28,
                28,
                0,
                &runtime.resources.rules,
                &runtime.resources.overlay_registry,
            )
            .unwrap();
        assert!(
            runtime
                .simulation
                .entities()
                .get(gi)
                .unwrap()
                .locomotor
                .is_some()
        );
        assert!(crate::sim::passenger::can_enter_transport(
            &runtime.simulation,
            &runtime.resources.rules,
            gi,
            house,
        ));
        // Infantry WhatAction9 -> FootClicked4D7716 emits Capture8.
        // Enter7 is the carrier path and does not occupy a civilian house.
        issue(
            &mut runtime,
            Command::CaptureBuilding {
                engineer_id: gi,
                target_building_id: house,
            },
        );
        for _ in 0..600 {
            step(&mut runtime);
            let actor = runtime.simulation.entities().get(gi).unwrap();
            if actor.passenger_role.inside_transport_id() == Some(house)
                && runtime.simulation.entities().get(house).unwrap().owner() == owner
            {
                break;
            }
        }
        let actor = runtime.simulation.entities().get(gi).unwrap();
        let building = runtime.simulation.entities().get(house).unwrap();
        assert_eq!(
            actor.passenger_role.inside_transport_id(),
            Some(house),
            "production garrison Capture reaches the actual passenger owner, fired={fired_before_d}"
        );
        assert_eq!(
            building.owner(),
            owner,
            "real entry captures the civilian house"
        );
        assert_eq!(building.passenger_role.occupants().ids(), [gi]);
        assert!(
            runtime
                .simulation
                .entities()
                .get(gi)
                .unwrap()
                .lifecycle
                .in_limbo()
        );

        if fired_before_d {
            let enemy = runtime
                .simulation
                .spawn_object_with_overlay_registry(
                    "AMCV",
                    "Russians",
                    20,
                    15,
                    0,
                    &runtime.resources.rules,
                    &runtime.resources.overlay_registry,
                )
                .expect("stock unarmed enemy within the garrison's range");
            let health = runtime
                .simulation
                .entities()
                .get(enemy)
                .unwrap()
                .health
                .current();
            issue(
                &mut runtime,
                Command::Attack {
                    attacker_id: house,
                    target_id: enemy,
                },
            );
            for _ in 0..600 {
                step(&mut runtime);
                if runtime
                    .simulation
                    .entities()
                    .get(enemy)
                    .unwrap()
                    .health
                    .current()
                    < health
                {
                    break;
                }
            }
            assert!(
                runtime
                    .simulation
                    .entities()
                    .get(enemy)
                    .unwrap()
                    .health
                    .current()
                    < health,
                "a legal garrison shot precedes D, rather than a raw target/timer write"
            );
            assert_eq!(
                runtime
                    .simulation
                    .entities()
                    .get(house)
                    .unwrap()
                    .passenger_role
                    .occupants()
                    .ids(),
                [gi]
            );
        }
        assert!(runtime.apply_command_for_test(
            "Americans",
            &Command::Select {
                entity_ids: vec![house],
                additive: false,
            }
        ));
        let before = runtime.simulation.entities().get(house).unwrap();
        let before_mission = before.mission.current();
        assert_eq!(
            before.attack_target.target().is_some(),
            fired_before_d,
            "actual NULL and legal fired-target controls"
        );
        let commands = deploy_key_commands(
            &runtime.simulation,
            Some(&runtime.resources.rules),
            &[house],
            Some(owner),
        );
        assert_eq!(
            commands.len(),
            1,
            "a real occupied first Building takes the D route"
        );
        let mut actual_opcodes = Vec::new();
        for command in commands {
            // The D producer's dedicated deploy DTO uses its Event9 encoder.
            let Command::DeployMcv { entity_id } = command else {
                panic!("the D route issues only deploy commands: {command:?}");
            };
            let record = runtime
                .simulation
                .encode_deploy_record(owner, entity_id)
                .expect("the actual D handler's dedicated record");
            actual_opcodes.push(u64::from(record.opcode()));
            issue(&mut runtime, Command::SynchronizedRecord { record });
        }
        let mut received = false;
        for _ in 0..240 {
            let tick = step(&mut runtime);
            if tick.executed_commands != 0 {
                received = true;
                let building = runtime.simulation.entities().get(house).unwrap();
                assert_eq!(
                    building.attack_target.target().is_some(),
                    expected["target_is_set"].as_bool().unwrap(),
                    "native Event9 target after real receipt, fired={fired_before_d}"
                );
                assert_eq!(
                    building
                        .mission
                        .queued()
                        .known()
                        .map(|mission| mission as i32),
                    Some(expected["queued_mission"].as_i64().unwrap() as i32)
                );
                assert_eq!(
                    building.passenger_role.occupants().count(),
                    expected["occupants"].as_i64().unwrap() as i32
                );
                assert_eq!(
                    building.mission.current(),
                    before_mission,
                    "Event9 queues Unload without forcing Commence"
                );
                break;
            }
        }
        assert!(
            received,
            "the admitted D copy actually executes, fired={fired_before_d}"
        );
        assert_eq!(
            actual_opcodes,
            [native["producer"]["event_opcode"].as_u64().unwrap()],
            "the state-checked D route must retain the native dedicated event"
        );
        for _ in 0..240 {
            if !runtime
                .simulation
                .entities()
                .get(gi)
                .unwrap()
                .passenger_role
                .is_inside_transport()
            {
                break;
            }
            step(&mut runtime);
        }
        let actor = runtime.simulation.entities().get(gi).unwrap();
        assert!(
            !actor.passenger_role.is_inside_transport(),
            "actual Unload releases GI, fired={fired_before_d}"
        );
        assert!(!actor.lifecycle.in_limbo());
        assert!(actor.lifecycle.object_alive());
        assert!(
            runtime
                .simulation
                .entities()
                .get(house)
                .unwrap()
                .passenger_role
                .occupants()
                .is_empty()
        );
    }
}
