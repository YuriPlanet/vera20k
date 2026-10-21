use super::*;
use std::collections::HashMap;

use crate::map::entities::EntityCategory;
use crate::map::waypoints::Waypoint;
use crate::sim::game_entity::GameEntity;
use crate::sim::projectile::{
    ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload, ProjectileSpawn,
    ProjectileTarget, TargetExpiryPolicy,
};
use crate::sim::replay::{ReplayHeader, ReplayLog, ReplayRunner};
use crate::sim::snapshot::GameSnapshot;
use crate::sim::world::{FrameEffects, MasterFrameTestRung, Simulation, TickLane};

#[path = "trigger_runtime_mind_control_callback_tests.rs"]
mod mind_control_callbacks;
#[path = "trigger_runtime_mind_control_tests.rs"]
mod mind_control_campaign;
#[path = "trigger_runtime_mind_control_native_tests.rs"]
mod mind_control_native;

#[test]
fn loss_threshold_events_poll_recorded_losses_and_create_real_reinforcements() {
    use crate::rules::{ini_parser::IniFile, team_ai_ini::TeamAiIniRegistry};
    use crate::sim::combat::{KillCallback, KillSource};
    use crate::sim::world::UninitContext;

    for (kind, victim_type) in [(15, "CABHUT"), (16, "TAGUNIT")] {
        let (mut sim, rules, overlay, houses) = event_ingress_fixture();
        sim.session.set_game_mode(
            crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false),
        );
        for house in sim.houses.values_mut() {
            house.set_is_human_for_test(true);
        }
        let victims: Vec<_> = [(15, 15), (16, 15)]
            .into_iter()
            .map(|cell| {
                sim.spawn_object_at_height(victim_type, "House0", cell.0, cell.1, 0, 0, &rules)
                    .unwrap()
            })
            .collect();
        let killer = spawn_event_ingress_unit(&mut sim, &rules, houses[1], (17, 16));
        let map = install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nLOSS=Country0,<none>,Loss,0,1,1,1,0\n\
                 [Events]\nLOSS=1,{kind},0,2\n\
                 [Actions]\nLOSS=2,7,1,TT,0,0,0,0,A,28,0,7,0,0,0,0,A\n\
                 [Tags]\nLOSS_TAG=0,Loss,LOSS\n\
                 [TeamTypes]\n0=TT\n[TT]\nHouse=Country0\nScript=S\nTaskForce=F\nWaypoint=A\nMax=1\n\
                 [ScriptTypes]\n0=S\n[S]\n0=5,0\n\
                 [TaskForces]\n0=F\n[F]\n0=1,TAGUNIT\n\
                 [Waypoints]\n0=16014\n"
            ),
        );
        let teams = TeamAiIniRegistry::from_sources(&IniFile::from_str(""), &map.ini, false);
        sim.install_team_ai_registry(&teams, &rules).unwrap();
        // The shared simple-map fixture initially has no Team registry.
        // Re-enter the real definition reader with the authored Team inputs.
        sim.initialize_trigger_definitions(&map, &teams, None, None);
        sim.session.install_waypoints(&map.waypoints);
        sim.classify_map_triggers(&rules);
        let team_type = sim.interner.get("TT").unwrap();
        let loss_count = |sim: &Simulation| {
            let stats = sim.houses[&houses[0]].stats();
            if kind == 15 {
                stats.buildings_lost()
            } else {
                stats.units_lost()
            }
        };
        let frame = |sim: &mut Simulation| {
            ordinary_master_frame(sim, Some(&rules), Some(&overlay), true);
        };
        assert_eq!(loss_count(&sim), 0);
        for (index, victim) in victims.into_iter().enumerate() {
            // Enter at the existing real kill callback, before UnInit. The
            // callback's native arithmetic/gates have their own RecordKill
            // evidence; this regression connects its authoritative loss write
            // to House polling, Team construction, placement and Tag cleanup.
            sim.record_the_kill(
                victim,
                KillSource::Techno(Some(killer)),
                KillCallback::Terminal,
                UninitContext::new(&rules, Some(&overlay)),
            );
            assert_eq!(loss_count(&sim), index as u32 + 1);
            assert!(
                sim.substrate
                    .entities
                    .get(victim)
                    .unwrap()
                    .lifecycle
                    .object_alive()
            );
            assert!(!sim.trigger_runtime.globals_set.contains(&7));
            sim.uninit_with_rules(victim, &rules);
            sim.session
                .set_binary_frame_for_test(if index == 0 { 8 } else { 16 });
            frame(&mut sim);
            assert_eq!(sim.trigger_runtime.globals_set.contains(&7), index == 1);
        }
        let members: Vec<_> = sim
            .team_script_vm
            .teams_in_order()
            .filter(|team| team.team_type_id() == Some(team_type))
            .flat_map(|team| team.members())
            .collect();
        assert_eq!(members.len(), 1, "event{kind}: real Team member");
        let reinforcement = sim.substrate.entities.get(members[0]).unwrap();
        assert_eq!(reinforcement.owner(), houses[0]);
        assert_eq!(sim.interner.resolve(reinforcement.type_ref()), "TAGUNIT");
        assert!(!reinforcement.lifecycle.in_limbo());
        assert!(reinforcement.lifecycle.object_alive());
        assert!(sim.trigger_runtime.tag_order.is_empty());
        sim.session.set_binary_frame_for_test(24);
        frame(&mut sim);
        assert_eq!(
            sim.trigger_runtime
                .action_trace
                .iter()
                .map(|row| row.0)
                .collect::<Vec<_>>(),
            [7, 28],
            "event{kind}: one-shot reinforcement must not repeat"
        );
    }
}

#[test]
fn funds_event_runs_from_house_rung_after_a_spend() {
    let (mut sim, rules, overlay, houses) = event_ingress_fixture();
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    for house in sim.houses.values_mut() {
        house.set_is_human_for_test(true);
    }
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nFUNDS=Country0,<none>,Funds,0,1,1,1,0\n\
         [Events]\nFUNDS=1,52,0,10000\n\
         [Actions]\nFUNDS=1,28,0,7,0,0,0,0,A\n\
         [Tags]\nFUNDS_TAG=0,Funds,FUNDS\n",
    );
    crate::sim::credit_income::add_credits(&mut sim.houses, houses[0], 70000);
    sim.session.set_binary_frame_for_test(0);
    for frame in 0..=8 {
        assert_eq!(sim.session.binary_frame(), frame);
        if frame == 7 {
            crate::sim::credit_income::spend_money(&mut sim.houses, houses[0], 60000);
        }
        ordinary_master_frame(&mut sim, Some(&rules), Some(&overlay), false);
        // Native House4F8F87 -> Tag/RegisterEvent -> HasOccurred52:
        // equality after spending admits the next eighth-frame poll.
        assert_eq!(
            sim.trigger_runtime.globals_set.contains(&7),
            frame == 8,
            "frame {frame}"
        );
    }
    assert_eq!(
        crate::sim::credit_income::available_money(&sim.houses, houses[0]),
        10000
    );
    assert!(sim.trigger_runtime.tag_order.is_empty());
}

#[test]
fn building_loss_event_waits_for_the_real_destructor_and_house_poll() {
    let (mut sim, rules, overlay, houses) = event_ingress_fixture();
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    for house in sim.houses.values_mut() {
        house.set_is_human_for_test(true);
    }
    let building = sim
        .spawn_object_at_height("CABHUT", "House0", 15, 15, 0, 0, &rules)
        .expect("the shared fixture admits ordinary Building construction");
    assert_eq!(sim.houses[&houses[0]].tracking().buildings(), 1);
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nLOSS=Country7,<none>,Buildings gone,0,1,1,1,0\nFOLLOW=Country7,<none>,Follow,0,1,1,1,0\n\
         [Events]\nLOSS=1,10,0,0\nFOLLOW=1,36,0,10\n\
         [Actions]\nLOSS=1,56,0,10,0,0,0,0,A\nFOLLOW=1,28,0,7,0,0,0,0,A\n\
         [Tags]\nLOSS_TAG=0,Loss,LOSS\nFOLLOW_TAG=0,Follow,FOLLOW\n",
    );
    let frame = |sim: &mut Simulation| {
        ordinary_master_frame(sim, Some(&rules), Some(&overlay), true);
    };
    frame(&mut sim);
    assert!(!sim.trigger_runtime.locals_set.contains(&10));
    sim.session.set_binary_frame_for_test(8);
    sim.uninit_with_rules(building, &rules);
    assert!(
        sim.substrate
            .entities
            .get(building)
            .unwrap()
            .lifecycle
            .in_limbo()
    );
    assert_eq!(sim.houses[&houses[0]].tracking().buildings(), 1);
    frame(&mut sim);
    // House polls before the shared pending-delete drain. Limbo alone must
    // not report all buildings destroyed, even with no live map building.
    assert!(!sim.trigger_runtime.locals_set.contains(&10));
    assert!(sim.substrate.entities.get(building).is_none());
    assert_eq!(sim.houses[&houses[0]].tracking().buildings(), 0);
    sim.session.set_binary_frame_for_test(16);
    frame(&mut sim);
    assert!(sim.trigger_runtime.locals_set.contains(&10));
    assert!(!sim.trigger_runtime.globals_set.contains(&7));
    frame(&mut sim);
    assert!(sim.trigger_runtime.globals_set.contains(&7));
    assert_eq!(sim.trigger_runtime.action_trace, [(56, 10), (28, 7)]);
    assert!(sim.trigger_runtime.tag_order.is_empty());
}

#[test]

fn naval_land_events_poll_final_tracking_removal_from_the_master_house_rung() {
    let (mut sim, rules, overlay) = crate::sim::world::entry_test_fixture::fixture_with_rules(
        "[Countries]\n0=Country0\n[Country0]\n\
         [VehicleTypes]\n0=NAVAL\n1=LAND\n\
         [NAVAL]\nStrength=100\nNaval=yes\n[LAND]\nStrength=100\n",
    );
    let country = sim.interner.intern("Country0");
    let owner = sim.interner.intern("House0");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner,
            0,
            Some(country),
            true,
            0,
            10,
        ));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    let navy = sim
        .spawn_object_limbo_at_height("NAVAL", "House0", 15, 15, 0, 0, &rules)
        .unwrap();
    let land = sim
        .spawn_object_limbo_at_height("LAND", "House0", 17, 15, 0, 0, &rules)
        .unwrap();
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nNAVY=Country0,<none>,Navy,0,1,1,1,0\nLAND=Country0,<none>,Land,0,1,1,1,0\n\
         [Events]\nNAVY=1,55,0,0\nLAND=1,56,0,0\n\
         [Actions]\nNAVY=1,28,0,7,0,0,0,0,A\nLAND=1,28,0,8,0,0,0,0,A\n\
         [Tags]\nNAVY_TAG=0,Navy,NAVY\nLAND_TAG=0,Land,LAND\n",
    );
    for frame in 0..=24 {
        assert_eq!(sim.session.binary_frame(), frame);
        if frame == 7 {
            sim.uninit_with_rules(navy, &rules);
            assert_eq!(sim.houses[&owner].tracking().navy(), 1);
        } else if frame == 15 {
            sim.uninit_with_rules(land, &rules);
            assert_eq!(sim.houses[&owner].tracking().vehicles(), 1);
        }
        ordinary_master_frame(&mut sim, Some(&rules), Some(&overlay), true);
        assert_eq!(
            sim.trigger_runtime.globals_set.contains(&7),
            frame >= 8,
            "navy at frame{frame}"
        );
        assert_eq!(
            sim.trigger_runtime.globals_set.contains(&8),
            frame >= 16,
            "land at frame{frame}"
        );
    }
    assert_eq!(sim.trigger_runtime.action_trace, [(28, 7), (28, 8)]);
    assert!(
        sim.trigger_runtime.tag_order.is_empty(),
        "both one-shot tags detach and drain"
    );
}

#[test]
fn music_action_reaches_the_ordered_presentation_effects() {
    let (mut sim, rules, overlay, _) = event_ingress_fixture();
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nMUSIC=Country0,<none>,Music,0,1,1,1,0\n\
         [Events]\nMUSIC=1,8,0,0\n\
         [Actions]\nMUSIC=3,20,8,DROK,0,0,0,0,A,20,0,-3,0,0,0,0,A,11,4,AFTER_MUSIC,0,0,0,0,A\n",
    );
    let source = sim
        .trigger_runtime
        .definitions
        .trigger_type("MUSIC")
        .unwrap();
    let actions = sim.trigger_runtime.definitions.trigger_types[source]
        .actions
        .clone();
    let mut effects = Vec::new();
    for action in &actions {
        assert!(sim.apply_live_trigger_action(
            action,
            sim.trigger_action_context(source, None, TagEventInput::default(), &rules),
            &mut effects,
            &rules,
            Some(&overlay),
            FrameEffects::default(),
        ));
    }
    assert_eq!(
        effects,
        [
            TriggerEffect::MusicTheme {
                request: ThemeRequest::Name("DROK".into()),
            },
            TriggerEffect::MusicTheme {
                request: ThemeRequest::Index(-3),
            },
            TriggerEffect::LocalizedMessage {
                csf_label: "AFTER_MUSIC".into(),
            },
        ]
    );
}

#[test]
fn retail_sov01_closeout_queues_matching_triggers_without_detaching_them() {
    let Some((mut sim, rules, overlay, _)) = retail_sov01_script_context() else {
        return;
    };
    for tag in 0..sim.trigger_runtime.definitions.tag_types.len() {
        sim.construct_trigger_tag(tag);
    }
    let source = sim
        .trigger_runtime
        .definitions
        .trigger_type("07FA53DC")
        .unwrap();
    let actions = sim.trigger_runtime.definitions.trigger_types[source]
        .actions
        .clone();
    let before = sim.trigger_runtime.trigger_order.clone();
    for action in actions.iter().filter(|action| action.entry.kind == 12) {
        assert!(sim.apply_live_trigger_action(
            action,
            sim.trigger_action_context(source, None, TagEventInput::default(), &rules),
            &mut Vec::new(),
            &rules,
            Some(&overlay),
            FrameEffects::default(),
        ));
    }
    // Original Execute6DF051 calls726720, never the deleting destructor.
    assert_eq!(sim.trigger_runtime.trigger_order, before);
    for name in [
        "0B603E5C", "0B60744C", "06A25F3C", "09FF490C", "082C467C", "082C44AC", "0DD0203C",
        "082C3CEC", "082C79BC", "082C77EC",
    ] {
        let target = sim.trigger_runtime.definitions.trigger_type(name).unwrap();
        let matching: Vec<_> = sim
            .trigger_runtime
            .triggers
            .iter()
            .flatten()
            .filter(|trigger| trigger.type_index == target)
            .collect();
        assert!(!matching.is_empty(), "retail target {name}");
        assert!(matching.iter().all(|trigger| trigger.pending), "{name}");
    }
    assert!(
        sim.trigger_runtime
            .triggers
            .iter()
            .flatten()
            .filter(|trigger| trigger.type_index == source)
            .all(|trigger| !trigger.pending)
    );
    sim.process_pending_delete_with(&rules, Some(&overlay), FrameEffects::default(), 1, true);
    assert!(
        sim.trigger_runtime
            .triggers
            .iter()
            .flatten()
            .all(|trigger| !trigger.pending)
    );
    sim.validate_restored_trigger_runtime().unwrap();
}

#[test]
fn delete_tag_returns_safely_through_its_active_dispatcher() {
    // Rust safety regression, not a native self-deletion parity claim: the
    // original Action70 destructor frees the caller's retained raw pointer.
    for repeat in [0, 1, 2] {
        for nested in [false, true] {
            let (mut sim, rules, overlays, _) = event_ingress_fixture();
            let killer = if nested {
                "22,2,NESTED,0,0,0,0,A"
            } else {
                "70,3,ACTIVE,0,0,0,0,A"
            };
            install_event_ingress_map(
                &mut sim,
                &rules,
                &format!(
                    "[Triggers]\nTAIL=Country0,KILLER,Tail,0,1,1,1,0\n\
                     KILLER=Country0,<none>,Killer,0,1,1,1,0\n\
                     NESTED=Country0,<none>,Nested,0,1,1,1,0\n\
                     [Tags]\nACTIVE={repeat},Active,TAIL\nOTHER=2,Other,NESTED\n\
                     [Events]\nTAIL=1,8,0,0\nKILLER=1,8,0,0\nNESTED=1,8,0,0\n\
                     [Actions]\nTAIL=1,28,0,7,0,0,0,0,A\nKILLER=1,{killer}\n\
                     NESTED=1,70,3,ACTIVE,0,0,0,0,A\n"
                ),
            );
            let active_type = sim.trigger_runtime.definitions.tag_type("ACTIVE").unwrap();
            let active = sim.shared_trigger_tag(active_type);
            let other_type = sim.trigger_runtime.definitions.tag_type("OTHER").unwrap();
            let other = sim.shared_trigger_tag(other_type);
            let cell = Simulation::trigger_cell_identity(&sim.resolved_terrain, (15, 15));
            sim.trigger_runtime
                .replace_cell_trigger_tag(cell, Some(active));
            assert_eq!(sim.trigger_runtime.tag(active).unwrap().refs, 1);
            assert!(sim.dispatch_live_tag_event(
                active,
                TagEventInput {
                    force: true,
                    cell: (15, 15),
                    ..TagEventInput::default()
                },
                &mut Vec::new(),
                &rules,
                Some(&overlays),
                FrameEffects::default(),
            ));
            assert!(sim.trigger_runtime.tag(active).is_none());
            assert!(!sim.trigger_runtime.cell_tags.contains_key(&cell));
            assert!(!sim.trigger_runtime.locals_set.contains(&7));
            assert!(!sim.trigger_runtime.tag(other).unwrap().dispatch_guard);
            sim.process_pending_delete_with(
                &rules,
                Some(&overlays),
                FrameEffects::default(),
                1,
                true,
            );
            sim.validate_restored_trigger_runtime().unwrap();
        }
    }
}

#[test]
fn retail_sov01_boris_tag_delete_is_immediate_and_releases_attachments() {
    let Some((mut sim, rules, overlay, _)) = retail_sov01_script_context() else {
        return;
    };
    let source = sim
        .trigger_runtime
        .definitions
        .trigger_type("097651DC")
        .unwrap();
    let action = sim.trigger_runtime.definitions.trigger_types[source]
        .actions
        .iter()
        .find(|action| action.entry.kind == 70)
        .unwrap()
        .clone();
    let Some(DefinitionReference::Tag(target)) = action.reference else {
        panic!("retail Tag reference")
    };
    let first = sim.construct_trigger_tag(target);
    let second = sim.construct_trigger_tag(target);
    let unrelated_type = sim
        .trigger_runtime
        .definitions
        .tag_type("0C47FB1C")
        .unwrap();
    let unrelated = sim.construct_trigger_tag(unrelated_type);
    let cell = Simulation::trigger_cell_identity(&sim.resolved_terrain, (50, 60));
    sim.trigger_runtime
        .replace_cell_trigger_tag(cell, Some(first));
    let linked = sim.trigger_runtime.tag(first).unwrap().triggers.clone();
    assert!(sim.apply_live_trigger_action(
        &action,
        sim.trigger_action_context(source, None, TagEventInput::default(), &rules),
        &mut Vec::new(),
        &rules,
        Some(&overlay),
        FrameEffects::default(),
    ));
    //6DF0CA's virtual20 is6E58B0 ->6E4F60; only its linked Triggers defer.
    assert!(sim.trigger_runtime.tag(first).is_none());
    assert!(sim.trigger_runtime.tag(second).is_none());
    assert!(sim.trigger_runtime.tag(unrelated).is_some());
    assert!(!sim.trigger_runtime.cell_tags.contains_key(&cell));
    assert!(
        linked
            .iter()
            .all(|&id| sim.trigger_runtime.trigger(id).is_some())
    );
    sim.process_pending_delete_with(&rules, Some(&overlay), FrameEffects::default(), 1, true);
    assert!(
        linked
            .iter()
            .all(|&id| sim.trigger_runtime.trigger(id).is_none())
    );
    sim.validate_restored_trigger_runtime().unwrap();
}

#[test]
fn retail_sov01_make_enemy_reaches_the_shared_diplomacy_owner() {
    let Some((mut sim, rules, overlay, _)) = retail_sov01_script_context() else {
        return;
    };
    let russians = sim.session.current_house().unwrap();
    let arabs = sim.interner.intern("Arabs");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            arabs,
            1,
            Some(arabs),
            false,
            0,
            10,
        ));
    let graph = [
        (
            "RUSSIANS".into(),
            ["RUSSIANS".into(), "ARABS".into()].into_iter().collect(),
        ),
        (
            "ARABS".into(),
            ["ARABS".into(), "RUSSIANS".into()].into_iter().collect(),
        ),
    ]
    .into_iter()
    .collect();
    sim.install_house_alliances(
        crate::sim::house_diplomacy::HouseAllianceAdmission::Admitted(graph),
        &rules,
    );
    let source = sim
        .trigger_runtime
        .definitions
        .trigger_type("0C331D8C")
        .unwrap();
    let tag = sim
        .trigger_runtime
        .definitions
        .tag_types
        .iter()
        .position(|tag| tag.trigger == Some(source))
        .unwrap();
    let tag = sim.construct_trigger_tag(tag);
    let live = sim
        .trigger_runtime
        .tag(tag)
        .unwrap()
        .triggers
        .iter()
        .copied()
        .find(|&id| sim.trigger_runtime.trigger(id).unwrap().type_index == source)
        .unwrap();
    let action = sim.trigger_runtime.definitions.trigger_types[source]
        .actions
        .iter()
        .find(|action| action.entry.kind == 38)
        .unwrap()
        .clone();
    assert!(sim.apply_live_trigger_action(
        &action,
        sim.trigger_action_context(source, Some(live), TagEventInput::default(), &rules),
        &mut Vec::new(),
        &rules,
        Some(&overlay),
        FrameEffects::default(),
    ));
    assert!(!sim.house_alliances.as_map()["RUSSIANS"].contains("ARABS"));
    assert!(!sim.house_alliances.as_map()["ARABS"].contains("RUSSIANS"));
    assert_eq!(sim.session.current_house(), Some(russians));
}

#[test]
fn retail_sov01_transport_hint_emits_the_named_cameo_on_the_native_clock() {
    let Some((mut sim, rules, overlay, _)) = retail_sov01_script_context() else {
        return;
    };
    let source = sim
        .trigger_runtime
        .definitions
        .trigger_type("097DB5BC")
        .unwrap();
    let actions = sim.trigger_runtime.definitions.trigger_types[source]
        .actions
        .clone();
    sim.session.set_binary_frame_for_test(0x7FFF_FFF0);
    let mut effects = Vec::new();
    for action in actions
        .iter()
        .filter(|action| matches!(action.entry.kind, 114 | 115))
    {
        assert!(sim.apply_live_trigger_action(
            action,
            sim.trigger_action_context(source, None, TagEventInput::default(), &rules),
            &mut effects,
            &rules,
            Some(&overlay),
            FrameEffects::default(),
        ));
    }
    assert_eq!(
        effects,
        [
            TriggerEffect::SelectSidebarTab { index: 3 },
            TriggerEffect::FlashSidebarCameo {
                type_id: sim.interner.get("SAPC").unwrap(),
                duration: 100,
                frame: 0x7FFF_FFF0,
            }
        ]
    );
}

#[test]
fn scenario_timer_retail_sov01_actions_reach_the_expiry_predicate() {
    let Some((mut sim, rules, overlay, _map)) = retail_sov01_script_context() else {
        return;
    };
    let source = trigger_type_index(&sim, "0952F87C");
    let actions = sim.trigger_runtime.definitions.trigger_types[source]
        .actions
        .clone();
    sim.session.set_binary_frame_for_test(123);
    for action in actions
        .iter()
        .filter(|action| matches!(action.entry.kind, 23..=27 | 103))
    {
        assert!(sim.apply_live_trigger_action(
            action,
            sim.trigger_action_context(source, None, TagEventInput::default(), &rules),
            &mut Vec::new(),
            &rules,
            Some(&overlay),
            FrameEffects::default(),
        ));
    }
    assert_eq!(
        sim.session.scenario_timer_view(),
        Some((1800, "MISSION:TmeTimer"))
    );
    sim.classify_map_triggers(&rules);
    let live = live_trigger_of_type(&sim, trigger_type_index(&sim, "0BDE333C"));
    for (frame, elapsed) in [(1922, false), (1923, true)] {
        sim.session.set_binary_frame_for_test(frame);
        assert_eq!(
            sim.has_occurred(0, TagEventInput::default(), None, &mut false, &rules, live,),
            elapsed
        );
    }
}

#[test]
fn all_change_house_dispatch_matches_original_selector_controls() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_all_change_house.json",
    ))
    .unwrap();
    for row in corpus["rows"].as_array().unwrap() {
        let input = &row["input"];
        let actors = input["actors"].as_array().unwrap();
        // These execute real ChangeOwner. Reentrant supplied native sinks and
        // powered-class dispatch are covered separately, not simulated here.
        if actors.len() != 1
            || actors[0]["class"] != 1
            || actors[0].get("callback_effect").is_some()
        {
            continue;
        }
        let (mut sim, rules, overlays, houses) = event_ingress_fixture();
        let order = input
            .get("house_order")
            .cloned()
            .unwrap_or_else(|| serde_json::json!([1, 2, 0]));
        sim.houses.reorder_for_test(
            &order
                .as_array()
                .unwrap()
                .iter()
                .map(|index| houses[index.as_u64().unwrap() as usize])
                .collect::<Vec<_>>(),
        );
        for (slot, house_index) in [(0, 1), (1, 0)] {
            sim.session
                .start_slot_houses_mut_for_test()
                .insert(slot, sim.houses.order()[house_index]);
        }
        let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nSHIFT=Country0,<none>,Shift,0,1,1,1,0\n\
             [Tags]\nTAG=2,Shift,SHIFT\n[Events]\nSHIFT=1,8,0,0\n\
             [Actions]\nSHIFT=1,{}\n",
                row["read"]["raw"].as_str().unwrap(),
            ),
        );
        let tag_type = sim.trigger_runtime.definitions.tag_type("TAG").unwrap();
        let tag = sim.shared_trigger_tag(tag_type);
        let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
        sim.trigger_runtime.triggers[live]
            .as_mut()
            .unwrap()
            .retained_house = input.get("trigger_owner").map_or(Some(houses[2]), |index| {
            index.as_u64().map(|index| houses[index as usize])
        });
        let typ = sim.trigger_runtime.trigger(live).unwrap().type_index;
        let action = sim.trigger_runtime.definitions.trigger_types[typ].actions[0].clone();
        let result = sim.apply_live_trigger_action(
            &action,
            TriggerActionContext {
                live_index: (input["null_trigger"] != true).then_some(live),
                incoming_house: Some(houses[input["source_owner"].as_u64().unwrap_or(0) as usize]),
                input: TagEventInput::default(),
            },
            &mut Vec::new(),
            &rules,
            Some(&overlays),
            FrameEffects::default(),
        );
        assert_eq!(result, row["result"] == 1, "{}", input["name"]);
        assert_eq!(
            sim.substrate.entities.get(actor).unwrap().owner(),
            houses[row["owners"][0].as_u64().unwrap() as usize],
            "{}",
            input["name"]
        );
    }
}

#[test]
fn all_change_house_retail_sov01_handoff_transfers_units_and_powered_buildings() {
    use crate::sim::house_state::HouseState;
    let Some((_, rules, _, map)) = retail_sov01_script_context() else {
        return;
    };
    let (rules, mut sim, owner) =
        crate::sim::superweapon::chronosphere_tests::world_with(rules, 128, &[]);
    // Match the map's Country/House identity, using the production layered
    // rules and action reader; the flat arena is not retail terrain evidence.
    for name in ["Americans", "Russians"] {
        let id = sim.interner.intern(name);
        sim.houses
            .get_mut(&id)
            .unwrap()
            .set_country_for_test(Some(id));
    }
    let target = sim.interner.intern("PowerHouse");
    let country = sim.interner.intern("PowerHouse Country");
    sim.houses
        .register(HouseState::new(target, 2, Some(country), false, 0, 10));
    assert_eq!(sim.house_by_trigger_selector(14, &rules), Some(target));
    let actors: Vec<_> = [("GAGAP", 20, 20), ("MTNK", 25, 25), ("GAPOWR", 30, 30)]
        .into_iter()
        .map(|(name, x, y)| {
            sim.spawn_object_at_height(name, "Americans", x, y, 0, 0, &rules)
                .unwrap()
        })
        .collect();
    sim.initialize_trigger_definitions(
        &map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
    sim.classify_map_triggers(&rules);
    let typ = trigger_type_index(&sim, "07F2978C");
    let live = live_trigger_of_type(&sim, typ);
    let action = sim.trigger_runtime.definitions.trigger_types[typ].actions[0].clone();
    assert_eq!((action.entry.kind, action.value), (36, Some(14)));
    let context = sim.trigger_action_context(typ, Some(live), TagEventInput::default(), &rules);
    assert_eq!(context.incoming_house, Some(owner));
    assert!(sim.apply_live_trigger_action(
        &action,
        context,
        &mut Vec::new(),
        &rules,
        None,
        FrameEffects::default()
    ));
    for actor in actors {
        assert_eq!(sim.substrate.entities.get(actor).unwrap().owner(), target);
    }
}

#[test]
fn action128_retail_foots_use_reverse_shared_sweep_and_finish_the_warp() {
    use crate::sim::superweapon::chronosphere_tests::{retail_world, step, world_with};
    use crate::sim::superweapon::invulnerability::{InvulnKind, apply_invulnerability};
    let Some((rules, _, _)) = retail_world() else {
        return;
    };
    let (rules, mut sim, owner) = world_with(rules, 128, &[]);
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_chrono_house.json",
    ))
    .unwrap();
    let offsets: Vec<(i16, i16)> =
        serde_json::from_value(native["native_offset_pairs"].clone()).unwrap();
    assert_eq!(&crate::sim::combat::cell_spread::sweep(11)[..=350], offsets);
    let mixed = native["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "reverse_mixed_classes")
        .unwrap();
    // Declared flat production arena: map reader/dispatch and real movement
    // consumers are tested here, not SOV01 terrain or whole-scene parity.
    let first = sim
        .spawn_object_at_height("MTNK", "Americans", 20, 20, 0, 0, &rules)
        .unwrap();
    let infantry = sim
        .spawn_object_at_height("E1", "Americans", 22, 20, 0, 0, &rules)
        .unwrap();
    let last = sim
        .spawn_object_at_height("MTNK", "Americans", 24, 20, 0, 0, &rules)
        .unwrap();
    let immune = sim
        .spawn_object_at_height("MTNK", "Americans", 26, 20, 0, 0, &rules)
        .unwrap();
    let other = sim
        .spawn_object_at_height("HTNK", "Russians", 10, 10, 0, 0, &rules)
        .unwrap();
    apply_invulnerability(
        sim.substrate.entities.get_mut(immune).unwrap(),
        0,
        900,
        InvulnKind::IronCurtain,
    );
    let map = install_event_ingress_map(
        &mut sim,
        &rules,
        "[Waypoints]\n15=50075\n[Triggers]\nSHIFT=Americans,<none>,Shift,0,1,1,1,0\n\
         [Events]\nSHIFT=1,8,0,0\n[Actions]\nSHIFT=1,128,0,478,0,0,0,0,P\n",
    );
    sim.session.install_waypoints(&map.waypoints);
    let source = trigger_type_index(&sim, "SHIFT");
    let action = sim.trigger_runtime.definitions.trigger_types[source].actions[0].clone();
    assert_eq!(action.entry.waypoint_index, Some(15));
    let before_other = crate::sim::movement::ground_pose::object_get_coords(
        sim.substrate.entities.get(other).unwrap(),
        sim.resolved_terrain.as_ref(),
    );
    assert!(sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            input: TagEventInput::default(),
            incoming_house: Some(owner)
        },
        &mut Vec::new(),
        &rules,
        None,
        FrameEffects::default(),
    ));
    for id in [immune, other] {
        assert!(!sim.substrate.entities.get(id).unwrap().chrono_warp_latch());
    }
    let destination = |id| {
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .chrono_warp()
            .unwrap()
            .destination()
    };
    for (id, row) in [(last, &mixed["warp"][0]), (first, &mixed["warp"][2])] {
        let coord = destination(id);
        assert_eq!(serde_json::json!([coord.x, coord.y, coord.z]), row["coord"]);
    }
    assert!(
        sim.substrate
            .entities
            .get(infantry)
            .unwrap()
            .chrono_warp_latch(),
        "Action128 warps ordinary infantry; Super Organic-kill admission does not apply"
    );
    for _ in 0..180 {
        step(&mut sim, &rules);
    }
    for id in [first, infantry, last] {
        let entity = sim
            .substrate
            .entities
            .get(id)
            .expect("survives ordinary ground warp");
        assert!(
            !entity.chrono_warp_latch() && !entity.is_warped_out() && !entity.is_warping_in(),
            "warp finishes for {id}"
        );
        assert!(entity.position.rx().abs_diff(75) <= 2 && entity.position.ry().abs_diff(50) <= 2);
    }
    assert_eq!(
        crate::sim::movement::ground_pose::object_get_coords(
            sim.substrate.entities.get(other).unwrap(),
            sim.resolved_terrain.as_ref(),
        ),
        before_other
    );
}

#[test]
fn script40_clears_local_through_trigger_owner_and_advances_on_rejected_indices() {
    use crate::sim::team_script_vm::{TeamScriptAction, TeamScriptDefinition};
    let rules =
        crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=125\nSpeed=4\n",
        ))
        .unwrap();
    // Original Script40 @ 6E9BBE calls SetLocal689910 even without `first`
    // and finishes after its unsigned index rejection. These are behavior
    // regressions; the shared variable owner's native controls remain in
    // parsed_variable_actions_match_native_reader_dispatch_and_restore.
    for (index, initially_set, changed) in [
        (0, true, true),
        (99, false, false),
        (-1, false, false),
        (100, false, false),
    ] {
        let mut sim = Simulation::new();
        let owner = sim.interner.intern("Russians");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                owner, 1, None, false, 0, 10,
            ));
        let member = sim
            .spawn_object("E1", "Russians", 10, 10, 0, &rules)
            .unwrap();
        let script = sim.interner.intern("Clear local then wait");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: crate::rules::team_ai_ini::TeamAiDefinitionSource::Scenario,
            actions: vec![
                TeamScriptAction {
                    action_id: 40,
                    argument: index,
                },
                TeamScriptAction {
                    action_id: 2,
                    argument: 0,
                },
            ],
        });
        let team = sim
            .team_script_vm
            .create_team(owner, script, vec![member], 0);
        if initially_set {
            Simulation::set_trigger_variable(
                &sim.session,
                &mut sim.scenario_rng,
                &mut sim.trigger_runtime,
                false,
                index,
                true,
            );
        }
        Simulation::set_trigger_variable(
            &sim.session,
            &mut sim.scenario_rng,
            &mut sim.trigger_runtime,
            true,
            3,
            true,
        );
        sim.trigger_runtime.dirty[0] = false;
        let rng = sim.rng_state();
        sim.run_team_ai_pass(&rules, None, FrameEffects::default());
        assert!(sim.trigger_runtime.locals_set.is_empty(), "index {index}");
        assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([3]));
        assert_eq!(sim.trigger_runtime.dirty[0], changed, "index {index}");
        assert!(
            sim.team_script_vm.team(team).unwrap().advance_pending(),
            "index {index}"
        );
        assert_eq!(sim.rng_state(), rng, "clearing a local has no RNG draw");
        sim.run_team_ai_pass(&rules, None, FrameEffects::default());
        assert_eq!(sim.team_script_vm.team(team).unwrap().cursor(), 1);
    }
}

/// Shared retail readers and a declared flat-water script arena. Terrain and
/// complete mission playback remain with the production game/manual checks.
fn retail_sov01_script_context() -> Option<(
    Simulation,
    crate::rules::ruleset::RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    crate::map::map_file::MapFile,
)> {
    use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
    use crate::rules::retail_ini_fixture::{
        retail_archive_bytes, retail_ini, retail_rules_and_art,
    };
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    let raw_map = retail_archive_bytes("SOV01UMD.MAP")?;
    let ai = retail_ini("aimd.ini")?;
    let map = crate::map::map_file::MapFile::from_bytes(&raw_map).unwrap();
    assert_eq!(
        crate::util::sha256::sha256_hex(&raw_map),
        "498b989a4b6f579dee0904a77f1fabf982c14786db21a46c41900fc8344ec72b"
    );
    let registry = TeamAiIniRegistry::from_sources(&ai, &map.ini, false);
    // The map's Rules over the retail root depend on those files alone, so
    // they are processed once per test process; each caller gets copies.
    let (rules, water, overlay) = crate::test_fixture::once_per_process("SOV01 rules", || {
        let (retail, art) = retail_rules_and_art()?;
        let mut layers = RulesLayerStack::new(retail);
        layers.push(RulesLayerKind::Scenario, map.ini.clone());
        let processed = layers.process_with_fixed_art(&art).unwrap();
        let mut rules = crate::rules::ruleset::RuleSet::from_processed_rules(&processed).unwrap();
        rules.install_art_data(crate::rules::retail_ini_fixture::art_registry(&art));
        let terrain_rules = crate::rules::terrain_rules::TerrainRules::from_ini(processed.ini());
        let water = *terrain_rules.semantics_for_land_type(2).unwrap();
        let overlay =
            crate::rules::overlay_types::OverlayTypeRegistry::from_ini(processed.ini(), Some(&art));
        Some((rules, water, overlay))
    })?;
    let rules = rules.with_fresh_serial();
    let mut sim = Simulation::new();
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    // A declared flat-water test scene isolates script admission. Retail map
    // definitions/waypoints use production readers; this is not a native
    // terrain or whole-opening replay. The release capture covers real terrain.
    sim.install_resolved_terrain_for_new_map(crate::map::resolved_terrain::test_grid(
        198,
        198,
        |x, y| {
            let mut cell = crate::map::resolved_terrain::test_flat_cell(x, y);
            cell.land_type = 2;
            cell.base_land_type = 2;
            cell.speed_costs = water.speed_costs;
            cell.base_speed_costs = water.speed_costs;
            cell.zone_type = crate::map::resolved_terrain::zone_class::WATER;
            cell
        },
    ));
    sim.session.set_map_width_for_test(198);
    sim.session.set_map_height_for_test(198);
    sim.set_playfield_bounds_for_test(Some(
        crate::map::playfield::PlayfieldBounds::from_map_header(&map.header),
    ));
    sim.set_playfield_size_height_for_test(Some(map.header.height as i32));
    sim.session.install_waypoints(&map.waypoints);
    let owner = sim.interner.intern("Russians");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner,
            1,
            Some(owner),
            true,
            0,
            10,
        ));
    sim.session.set_current_house_for_test(Some(owner));
    sim.install_team_ai_registry(&registry, &rules).unwrap();
    sim.initialize_trigger_definitions(&map, &registry, None, None);
    Some((sim, rules, overlay, map))
}

/// The registry index of TriggerType `id`, without registering a missing one.
fn trigger_type_index(sim: &Simulation, id: &str) -> usize {
    sim.trigger_runtime
        .definitions
        .trigger_types
        .iter()
        .position(|typ| typ.id == id)
        .unwrap()
}

/// The first live Trigger instance of TriggerType `type_index`.
fn live_trigger_of_type(sim: &Simulation, type_index: usize) -> usize {
    sim.trigger_runtime
        .triggers
        .iter()
        .position(|trigger| {
            trigger
                .as_ref()
                .is_some_and(|trigger| trigger.type_index == type_index)
        })
        .unwrap()
}

#[test]
fn script40_releases_all_retail_sov01_opening_naval_teams_to_their_move_step() {
    let Some((mut sim, rules, overlay, _map)) = retail_sov01_script_context() else {
        return;
    };
    let naval = [
        "0C0A612C", "0C0A790C", "0C0A756C", "0C0A71CC", "0C0A8B7C", "0C0A824C", "0C0A9C8C",
        "0C0A98EC", "0C04FEFC",
    ];
    for name in naval {
        let team_type = sim.interner.get(name).unwrap();
        assert!(
            sim.spawn_team_reinforcements(
                team_type,
                -1,
                &rules,
                Some(&overlay),
                FrameEffects::default()
            )
            .0,
            "{name}"
        );
    }
    assert_eq!(sim.team_script_vm.teams_in_order().count(), 9);
    assert!(
        sim.team_script_vm
            .teams_in_order()
            .all(|team| team.members().count() == 1),
        "each authored naval TaskForce has one member; spawn-manager children are not team members"
    );
    sim.run_team_ai_pass(&rules, Some(&overlay), FrameEffects::default());
    assert!(
        sim.team_script_vm
            .teams_in_order()
            .all(|team| team.advance_pending())
    );
    sim.run_team_ai_pass(&rules, Some(&overlay), FrameEffects::default());
    for team in sim.team_script_vm.teams_in_order() {
        assert_eq!(
            team.cursor(),
            1,
            "{}",
            sim.interner.resolve(team.script_id())
        );
        assert_eq!(team.refusal(), None);
    }
}

#[test]
fn script20_uses_country_order_and_survives_members_unlinked_by_owner_callbacks() {
    use crate::sim::team_script_vm::{TeamScriptAction, TeamScriptDefinition};
    let (mut sim, rules, overlay, houses) = event_ingress_fixture();
    sim.houses
        .get_mut(&houses[2])
        .unwrap()
        .set_is_human_for_test(true);
    sim.session.set_current_house_for_test(Some(houses[2]));
    sim.houses
        .reorder_for_test(&[houses[1], houses[2], houses[0]]);
    // Houses 0 and 2 share Country0. Registration selects House2, while
    // lexical order would select House0 and raw House index0 names House1.
    let members: Vec<_> = [(3, 4), (4, 4), (5, 4)]
        .into_iter()
        .map(|cell| spawn_event_ingress_unit(&mut sim, &rules, houses[1], cell))
        .collect();
    let script = sim.interner.intern("Script20 transfer callback control");
    sim.team_script_vm.register_script(TeamScriptDefinition {
        id: script,
        source: crate::rules::team_ai_ini::TeamAiDefinitionSource::Scenario,
        actions: vec![TeamScriptAction {
            action_id: 20,
            argument: 0,
        }],
    });
    let team = sim
        .team_script_vm
        .create_team(houses[1], script, members.clone(), 0);
    sim.run_team_ai_pass(&rules, Some(&overlay), FrameEffects::default());
    for member in members {
        assert_eq!(
            sim.substrate.entities.get(member).unwrap().owner(),
            houses[2]
        );
        assert!(sim.team_script_vm.team_for_member(member).is_none());
    }
    let team = sim.team_script_vm.team(team).unwrap();
    assert_eq!(
        team.owner(),
        houses[1],
        "the script does not change Team.House"
    );
    assert_eq!(
        team.member_count(),
        0,
        "the human-owner callback unlinks every Foot"
    );
    assert!(team.advance_pending());
}

#[test]
fn script20_retail_sov01_transport_tails_set_the_authored_landing_flags() {
    use crate::rules::ruleset::CountryIdx;
    use crate::rules::team_ai_ini::TeamAiIniRegistry;
    use crate::sim::team_script_vm::{TeamScriptAction, TeamScriptDefinition};
    // Only the admitted, already-unloaded tail is cropped here. Movement,
    // Full cargo construction and actual landing are played through the game.
    for (stock_id, expected_flag) in [("08816C8C", 45), ("0D2F490C", 9)] {
        let Some((mut sim, rules, overlay, map)) = retail_sov01_script_context() else {
            return;
        };
        let registry = TeamAiIniRegistry::from_sources(
            &crate::rules::ini_parser::IniFile::from_str(""),
            &map.ini,
            false,
        );
        let stock = registry
            .scripts
            .iter()
            .find(|script| script.id == stock_id)
            .unwrap();
        let actions: Vec<_> = stock
            .actions
            .iter()
            .skip_while(|action| action.action_id != 20)
            .take(2)
            .map(|action| TeamScriptAction {
                action_id: action.action_id,
                argument: action.argument,
            })
            .collect();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].argument, 15);
        assert_eq!(
            actions[1],
            TeamScriptAction {
                action_id: 39,
                argument: expected_flag
            }
        );
        let target_name = rules.country_name(CountryIdx(15)).unwrap().to_owned();
        assert_eq!(
            target_name, "Soviet2 Country",
            "the map appends this Country after retail rules"
        );
        let target = sim.interner.intern(&target_name);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                target,
                2,
                Some(target),
                false,
                0,
                10,
            ));
        let member = sim
            .spawn_object("SAPC", "Russians", 100, 100, 0, &rules)
            .unwrap();
        let original_owner = sim.substrate.entities.get(member).unwrap().owner();
        let script = sim.interner.intern("Admitted retail unload tail");
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: stock.source,
            actions,
        });
        let team = sim
            .team_script_vm
            .create_team(original_owner, script, vec![member], 0);
        sim.run_team_ai_pass(&rules, Some(&overlay), FrameEffects::default());
        assert_eq!(sim.substrate.entities.get(member).unwrap().owner(), target);
        assert_eq!(
            sim.team_script_vm.team(team).unwrap().owner(),
            original_owner
        );
        assert!(sim.team_script_vm.team(team).unwrap().advance_pending());
        assert!(
            !sim.trigger_runtime
                .locals_set
                .contains(&(expected_flag as u32))
        );
        sim.run_team_ai_pass(&rules, Some(&overlay), FrameEffects::default());
        assert!(
            sim.trigger_runtime
                .locals_set
                .contains(&(expected_flag as u32)),
            "{stock_id}"
        );
        assert_eq!(sim.team_script_vm.team(team).unwrap().refusal(), None);
    }
}

/// Shared native-shaped arena for synchronous Object and cell-entry consumers.
/// Rules, object construction and owner tracking use production ingress; the
/// three Houses deliberately include two different instances of Country0.
pub(super) fn event_ingress_fixture() -> (
    Simulation,
    crate::rules::ruleset::RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    [crate::sim::intern::InternedId; 3],
) {
    event_ingress_fixture_with_rules("")
}

pub(super) fn event_ingress_fixture_with_rules(
    extra_rules: &str,
) -> (
    Simulation,
    crate::rules::ruleset::RuleSet,
    crate::rules::overlay_types::OverlayTypeRegistry,
    [crate::sim::intern::InternedId; 3],
) {
    let mut countries = String::from("[Countries]\n");
    for index in 0..8 {
        countries.push_str(&format!("{index}=Country{index}\n"));
    }
    for index in 0..8 {
        countries.push_str(&format!("[Country{index}]\n"));
    }
    countries.push_str(
        "[VehicleTypes]\n0=TAGUNIT\n[TAGUNIT]\nStrength=100\nSpeed=5\nSpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    );
    countries.push_str(extra_rules);
    let (mut sim, rules, overlays) =
        crate::sim::world::entry_test_fixture::fixture_with_rules(&countries);
    let houses = std::array::from_fn(|index| {
        let house = sim.interner.intern(&format!("House{index}"));
        let country = sim
            .interner
            .intern(if index == 1 { "Country7" } else { "Country0" });
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                house,
                index as u8,
                Some(country),
                false,
                0,
                10,
            ));
        house
    });
    (sim, rules, overlays, houses)
}

pub(super) fn install_event_ingress_map(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    body: &str,
) -> crate::map::map_file::MapFile {
    let map = trigger_fixture_map(body);
    install_map_triggers(sim, &map);
    sim.install_authored_cell_trigger_tags(&map);
    sim.classify_map_triggers(rules);
    map
}

pub(super) fn spawn_event_ingress_unit(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    house: crate::sim::intern::InternedId,
    cell: (u16, u16),
) -> u64 {
    let owner = sim.interner.resolve(house).to_owned();
    sim.spawn_object_at_height("TAGUNIT", &owner, cell.0, cell.1, 0, 0, rules)
        .expect("fixture cell admits the normal production Unit constructor")
}

/// Native line controls supply a sparse 4x4 cell table and an actor at 3,4.
/// Construct a real active Unit first, then set its declared coordinate through
/// the shared Foot owner before restricting the native cell-array rectangle.
/// Retain the loaded terrain and its matching overlay/path/bridge caches; the
/// compared marker and scan slices visit only the declared 4x4 cells.
/// A bare test constructor
/// remains in limbo, so Unit PerCell correctly refuses it before the Foot tail.
pub(super) fn sparse_line_ingress_fixture() -> (
    Simulation,
    crate::rules::ruleset::RuleSet,
    [crate::sim::intern::InternedId; 3],
    u64,
) {
    let (mut sim, rules, _, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    sim.foot_set_location_marked(
        actor,
        crate::sim::components::DriveCoord {
            x: 3 * 256,
            y: 4 * 256,
            z: 0,
        },
        &rules,
        None,
        FrameEffects::default(),
    );
    sim.session.set_map_width_for_test(5);
    sim.session.set_map_height_for_test(5);
    (sim, rules, houses, actor)
}

pub(super) fn event_ingress_damage(
    sim: &mut Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    overlays: &crate::rules::overlay_types::OverlayTypeRegistry,
    victim: u64,
    source: u64,
    damage: i32,
    effects: FrameEffects<'_>,
) {
    let warhead = sim.interner.intern("SA");
    sim.commit_direct_damage_receiver(
        rules,
        Some(overlays),
        crate::sim::combat::EntityDamageEvent::direct_receiver(
            victim,
            damage,
            0,
            source,
            None,
            warhead,
            crate::sim::combat::ReceiverCallFlags {
                ignore_defenses: true,
                arg6: false,
            },
        ),
        effects,
    );
}

#[test]
fn unit_record_kill_dispatches_its_virtual_prefix_before_the_base_events() {
    use crate::sim::combat::{KillCallback, KillSource};
    // Original Unit744720 -> Techno702D40; E4 stays703230 (vtable
    //7F5C70+E0/E4). Repeating Tag keeps each callback observable.
    for (techno, present, expected) in [
        (true, true, vec![7, 48, 29, 6, 4]),
        (true, false, vec![48, 29]),
        (false, true, vec![6, 4]),
        (false, false, vec![]),
    ] {
        let (mut sim, rules, overlays, houses) = event_ingress_fixture();
        let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
        let killer = spawn_event_ingress_unit(&mut sim, &rules, houses[1], (17, 15));
        install_event_ingress_map(
            &mut sim,
            &rules,
            "[Triggers]\nDEATH=Country0,<none>,Death,0,1,1,1,0\n\
             [Tags]\nTAG=2,Death,DEATH\n[Events]\nDEATH=1,48,0,0\n",
        );
        sim.attach_object_trigger_tag(victim, Some("TAG"));
        sim.trigger_runtime.dispatch_trace.clear();
        let source = if techno {
            KillSource::Techno(present.then_some(killer))
        } else {
            KillSource::House(present.then_some(houses[1]))
        };
        sim.record_the_kill(
            victim,
            source,
            KillCallback::Terminal,
            crate::sim::world::UninitContext::new(&rules, Some(&overlays)),
        );
        assert_eq!(
            sim.trigger_runtime
                .dispatch_trace
                .iter()
                .map(|call| call.1)
                .collect::<Vec<_>>(),
            expected,
            "E0={techno}, incoming pointer present={present}"
        );
    }
}

#[test]
fn unit_record_kill_releases_retail_sov01_landing_when_the_last_target_is_a_ship() {
    use crate::sim::combat::{KillCallback, KillSource};
    let Some((mut sim, rules, overlay, map)) = retail_sov01_script_context() else {
        return;
    };
    for (name, country) in [("Americans", "Americans"), ("Alliance", "Alliance")] {
        let house = sim.interner.intern(name);
        let country = sim.interner.intern(country);
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                house,
                0,
                Some(country),
                false,
                0,
                10,
            ));
    }
    // Real map readers select the seven authored targets and their shared
    // repeat=1 Tag. Flat-water placement is the declared arena above, not
    // a whole mission replay. Destroy buildings first: a remaining ship must
    // spring48 itself rather than rely on a later building's base callback.
    let mut targets: Vec<_> = map
        .entities
        .iter()
        .filter(|entity| entity.trigger_tag.as_deref() == Some("0B75455C"))
        .cloned()
        .collect();
    assert_eq!(targets.len(), 7);
    targets.sort_by_key(|entity| entity.category != EntityCategory::Structure);
    assert_eq!(sim.spawn_from_map(&targets, Some(&rules)), 7);
    sim.classify_map_triggers(&rules);
    let tag_type = sim
        .trigger_runtime
        .definitions
        .tag_type("0B75455C")
        .unwrap();
    let tag = sim.shared_trigger_tag(tag_type);
    assert_eq!(sim.trigger_runtime.tag(tag).unwrap().refs, 7);
    let landing = live_trigger_of_type(&sim, trigger_type_index(&sim, "0B92C6AC"));
    assert!(!sim.trigger_runtime.trigger(landing).unwrap().enabled);
    let mut victims: Vec<_> = sim
        .substrate
        .entities
        .values()
        .filter(|entity| {
            sim.trigger_runtime
                .attachments
                .get(&TagAttachment::Object(entity.stable_id()))
                == Some(&tag)
        })
        .map(|entity| {
            (
                entity.category() != EntityCategory::Structure,
                entity.stable_id(),
            )
        })
        .collect();
    victims.sort_unstable();
    for (index, (_, victim)) in victims.into_iter().enumerate() {
        sim.record_the_kill(
            victim,
            KillSource::Techno(None),
            KillCallback::Terminal,
            crate::sim::world::UninitContext::new(&rules, Some(&overlay)),
        );
        assert_eq!(
            sim.trigger_runtime.trigger(landing).unwrap().enabled,
            index == 6,
            "landing must enable exactly on the seventh target"
        );
        sim.uninit_with_context(
            victim,
            crate::sim::world::UninitContext::new(&rules, Some(&overlay)),
        );
    }
}

const MIND_CONTROL_TAG_RULES: &str = "\
[InfantryTypes]
2=TAGYURI
[TAGYURI]
Strength=100
Speed=4
SpeedType=Foot
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
Primary=TagMindControl
ImmuneToPsionics=yes
[TagMindControl]
Damage=1
ROF=200
Range=7
Speed=100
Projectile=TagPsychicControl
Warhead=TagController
[TagPsychicControl]
Inviso=yes
[Warheads]
2=TagController
[TagController]
MindControl=yes
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
";

const MIND_CONTROL_TAG_MAP: &str = "\
[Triggers]
ATTACKED=Country0,SOURCE,Attacked,0,1,1,1,0
SOURCE=Country0,<none>,Source,0,1,1,1,0
[Tags]
TAG=2,Attacked,ATTACKED
[Events]
ATTACKED=1,6,0,0
SOURCE=1,44,0,0
[Actions]
ATTACKED=1,28,0,6,0,0,0,0,A
SOURCE=1,28,0,44,0,0,0,0,A
";

fn mind_control_tag_projectile(
    sim: &mut Simulation,
    source: u64,
    target: u64,
) -> crate::sim::projectile::ProjectileDetonation {
    let projectile_id = sim.substrate.allocate_stable_id();
    let payload = ProjectilePayload::new(
        1,
        sim.interner.intern("TagController"),
        Some(sim.interner.intern("TagPsychicControl")),
        false,
    );
    let impact = ProjectileCoord::new(15 * 256 + 128, 15 * 256 + 128, 0);
    sim.admit_projectile(
        projectile_id,
        ProjectileSpawn {
            native_unique_id: 0,
            line_trail: None,
            flat: false,
            source_id: source,
            origin: ProjectileCoord::new(17 * 256 + 128, 15 * 256 + 128, 0),
            target: ProjectileTarget::Entity(target),
            initial_target_position: impact,
            payload,
            speed_leptons_per_frame: 100,
            velocity: crate::sim::projectile::ProjectileVelocity::new(0, 0, 0),
            trajectory: crate::sim::projectile::ProjectileTrajectory::Straight,
            guidance: None,
            visual: crate::sim::projectile::ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        },
    );
    crate::sim::projectile::ProjectileDetonation {
        projectile_id,
        source_id: source,
        target: ProjectileTarget::Entity(target),
        impact,
        payload,
        reason: crate::sim::projectile::ProjectileDetonationReason::ReachedTarget,
    }
}

#[test]
fn mind_control_tags_precede_capture_admission_and_use_object_alive() {
    for (manager, alive, health, immune, expected) in [
        (true, true, 100, false, true),
        (true, true, 100, true, true),
        (true, true, 0, true, true),
        (true, false, 100, true, false),
        (false, true, 100, false, false),
    ] {
        let (mut sim, rules, overlay, _) = event_ingress_fixture_with_rules(MIND_CONTROL_TAG_RULES);
        let victim_type = if immune { "TAGYURI" } else { "TAGUNIT" };
        assert_eq!(
            rules.object(victim_type).unwrap().immune_to_psionics,
            immune
        );
        let source = sim
            .spawn_object_at_height(
                if manager { "TAGYURI" } else { "TAGUNIT" },
                "House0",
                17,
                15,
                0,
                0,
                &rules,
            )
            .unwrap();
        let victim = sim
            .spawn_object_at_height(victim_type, "House2", 15, 15, 0, 0, &rules)
            .unwrap();
        install_event_ingress_map(&mut sim, &rules, MIND_CONTROL_TAG_MAP);
        sim.attach_object_trigger_tag(victim, Some("TAG"));
        let entity = sim.substrate.entities.get_mut(victim).unwrap();
        entity.health.set_for_test(health);
        entity.lifecycle.set_object_alive_for_test(alive);
        let detonation = mind_control_tag_projectile(&mut sim, source, victim);
        sim.mind_control_detonation(detonation, &rules, Some(&overlay), FrameEffects::default());
        let context =
            format!("manager={manager}, ObjectAlive={alive}, HP={health}, immune={immune}");
        assert_eq!(
            sim.trigger_runtime.globals_set.contains(&6),
            expected,
            "{context}"
        );
        assert_eq!(
            sim.trigger_runtime.globals_set.contains(&44),
            expected,
            "{context}"
        );
        assert_eq!(
            sim.substrate.entities.get(victim).unwrap().health.current(),
            health,
            "{context}: no damage"
        );
        assert_eq!(
            sim.substrate
                .entities
                .get(victim)
                .unwrap()
                .mind_control
                .controller(),
            (manager && !immune).then_some(source),
            "{context}: existing capture admission"
        );
    }
}

#[test]
fn mind_control_event6_owner_change_updates_source_house_before_event44_and_capture() {
    let (mut sim, rules, overlay, houses) =
        event_ingress_fixture_with_rules(MIND_CONTROL_TAG_RULES);
    let source = sim
        .spawn_object_at_height("TAGYURI", "House0", 17, 15, 0, 0, &rules)
        .unwrap();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[2], (15, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nATTACKED=Country0,SOURCE,Attacked,0,1,1,1,0\nSOURCE=Country0,<none>,Source,0,1,1,1,0\n\
         [Tags]\nTAG=2,Attacked,ATTACKED\n[Events]\nATTACKED=1,6,0,0\nSOURCE=1,44,0,1\n\
         [Actions]\nATTACKED=2,14,0,7,0,0,0,0,A,28,0,6,0,0,0,0,A\nSOURCE=1,28,0,44,0,0,0,0,A\n",
    );
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    sim.attach_object_trigger_tag(source, Some("TAG"));
    let detonation = mind_control_tag_projectile(&mut sim, source, victim);
    sim.mind_control_detonation(detonation, &rules, Some(&overlay), FrameEffects::default());
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([6, 44]));
    let live = sim.projectiles.current_detonation(detonation);
    assert_eq!(live.source_id, source);
    assert_eq!(live.target, ProjectileTarget::Entity(victim));
    assert_eq!(
        sim.substrate.entities.get(source).unwrap().owner(),
        houses[1]
    );
    let victim = sim.substrate.entities.get(victim).unwrap();
    assert_eq!(victim.owner(), houses[1]);
    assert_eq!(victim.health.current(), 100);
    assert_eq!(
        victim.mind_control.controller(),
        None,
        "event6 transfers both actors to the same house before the capture attempt"
    );
}

#[test]
fn mind_control_attack_order_fires_attacked_tags_without_hp_damage() {
    use crate::sim::command::{Command, CommandEnvelope};
    let (mut sim, rules, overlay, houses) =
        event_ingress_fixture_with_rules(MIND_CONTROL_TAG_RULES);
    let source = sim
        .spawn_object_at_height("TAGYURI", "House0", 17, 15, 0, 0, &rules)
        .unwrap();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[2], (15, 15));
    install_event_ingress_map(&mut sim, &rules, MIND_CONTROL_TAG_MAP);
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    assert!(sim.trigger_runtime.globals_set.is_empty());
    sim.queue_command(CommandEnvelope::new(
        houses[0],
        sim.session.tick() + 1,
        Command::Attack {
            attacker_id: source,
            target_id: victim,
        },
    ));
    for _ in 0..90 {
        let commands = sim.take_due_commands();
        sim.advance_tick(&commands, Some(&rules), None, Some(&overlay), 33);
        if sim
            .substrate
            .entities
            .get(victim)
            .unwrap()
            .mind_control
            .controller()
            .is_some()
        {
            break;
        }
    }
    let victim = sim.substrate.entities.get(victim).unwrap();
    assert_eq!(victim.health.current(), 100);
    assert_eq!(victim.mind_control.controller(), Some(source));
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([6, 44]));
}

#[test]
fn object_damage_tags_spring_before_techno_and_reread_the_source_house_index() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let source = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (17, 15));
    sim.substrate
        .entities
        .get_mut(victim)
        .unwrap()
        .health
        .set_for_test(80);
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nDAMAGE=Country0,SOURCE,Damage,0,1,1,1,0\nSOURCE=Country0,<none>,Source,0,1,1,1,0\n\
         [Tags]\nTAG=2,Damage,DAMAGE\n\
         [Events]\nDAMAGE=1,6,0,0\nSOURCE=1,44,0,1\n\
         [Actions]\nDAMAGE=2,14,0,7,0,0,0,0,A,28,0,6,0,0,0,0,A\nSOURCE=1,28,0,44,0,0,0,0,A\n",
    );
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    sim.attach_object_trigger_tag(source, Some("TAG"));
    let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(victim)];
    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        victim,
        source,
        1,
        FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate.entities.get(victim).unwrap().health.current(),
        79
    );
    assert_eq!(
        sim.substrate.entities.get(victim).unwrap().owner(),
        houses[1]
    );
    assert_eq!(
        sim.substrate.entities.get(source).unwrap().owner(),
        houses[1]
    );
    // Event6's Action14 changes both owners. Each Unit E0(null-killer)
    // reaches original744720's48/29 prefix before the later damage44.
    assert_eq!(
        sim.trigger_runtime.dispatch_trace,
        [
            (tag, 6),
            (tag, 48),
            (tag, 29),
            (tag, 48),
            (tag, 29),
            (tag, 44)
        ]
    );
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([6, 44]));
    assert!(
        !sim.substrate
            .entities
            .get(victim)
            .unwrap()
            .ai_sell_repair
            .was_attacked_by_enemy(),
        "the Techno continuation must consume the changed, now-allied owners"
    );
}

#[test]
fn object_damage_tag_owner_change_precedes_the_hostile_hit_latch() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let source = spawn_event_ingress_unit(&mut sim, &rules, houses[1], (17, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nDAMAGE=Country0,<none>,Damage,0,1,1,1,0\n\
         [Tags]\nTAG=2,Damage,DAMAGE\n[Events]\nDAMAGE=1,6,0,0\n\
         [Actions]\nDAMAGE=1,14,0,7,0,0,0,0,A\n",
    );
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        victim,
        source,
        1,
        FrameEffects::default(),
    );
    let victim = sim.substrate.entities.get(victim).unwrap();
    assert_eq!(victim.owner(), houses[1]);
    assert!(
        !victim.ai_sell_repair.was_attacked_by_enemy(),
        "capturing pre-Tag hostility would latch an enemy hit after ownership changed"
    );
}

#[test]
fn object_damage_event44_uses_source_house_index_instead_of_country_or_victim() {
    for (victim_house, source_house, expected) in [(0, 1, true), (1, 0, false)] {
        let (mut sim, rules, overlays, houses) = event_ingress_fixture();
        let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[victim_house], (15, 15));
        let source = spawn_event_ingress_unit(&mut sim, &rules, houses[source_house], (17, 15));
        install_event_ingress_map(
            &mut sim,
            &rules,
            "[Triggers]\nSOURCE=Country0,<none>,Source,0,1,1,1,0\n\
             [Tags]\nTAG=2,Source,SOURCE\n[Events]\nSOURCE=1,44,0,1\n\
             [Actions]\nSOURCE=1,28,0,44,0,0,0,0,A\n",
        );
        sim.attach_object_trigger_tag(victim, Some("TAG"));
        event_ingress_damage(
            &mut sim,
            &rules,
            &overlays,
            victim,
            source,
            1,
            FrameEffects::default(),
        );
        assert_eq!(
            sim.trigger_runtime.globals_set.contains(&44),
            expected,
            "Event44 has its own source object, independent of actor and authored incoming Country"
        );
    }
}

#[test]
fn fatal_receiver_does_not_raise_surviving_damage_event44() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let victim = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    let source = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (17, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nDAMAGE=Country0,SOURCE,Damage,0,1,1,1,0\nSOURCE=Country0,<none>,Source,0,1,1,1,0\n\
         [Tags]\nTAG=2,Damage,DAMAGE\n[Events]\nDAMAGE=1,6,0,0\nSOURCE=1,44,0,0\n\
         [Actions]\nDAMAGE=1,28,0,6,0,0,0,0,A\nSOURCE=1,28,0,44,0,0,0,0,A\n",
    );
    sim.attach_object_trigger_tag(victim, Some("TAG"));
    event_ingress_damage(
        &mut sim,
        &rules,
        &overlays,
        victim,
        source,
        100,
        FrameEffects::default(),
    );
    // RecordKill703000 legitimately raises victim6 with sourceNULL. It must
    // not admit the separate surviving Object6/44 pair after a fatal result.
    assert!(sim.trigger_runtime.globals_set.contains(&6));
    assert!(!sim.trigger_runtime.globals_set.contains(&44));
    assert!(
        sim.trigger_runtime
            .dispatch_trace
            .iter()
            .all(|call| call.1 != 44)
    );
    assert!(
        sim.substrate
            .entities
            .get(victim)
            .is_none_or(|entity| entity.health.current() <= 0)
    );
}

#[test]
fn entered_cell_retains_the_actual_duplicate_country_house_for_8997_actions() {
    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[2], (15, 15));
    let other = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (17, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nENTER=Country0,<none>,Enter,0,1,1,1,0\n\
         [Tags]\nTAG=2,Enter,ENTER\n[Events]\nENTER=1,1,0,-1\n\
         [Actions]\nENTER=2,14,0,8997,0,0,0,0,A,3,0,8997,0,0,0,0,A\n\
         [CellTags]\n15015=TAG\n",
    );
    sim.attach_object_trigger_tag(actor, Some("TAG"));
    sim.attach_object_trigger_tag(other, Some("TAG"));
    sim.per_cell_process(
        actor,
        crate::sim::movement::PerCellReason::Arrival,
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(
        sim.substrate.entities.get(other).unwrap().owner(),
        houses[2]
    );
    assert!(sim.houses[&houses[2]].ai_activation().production);
    assert!(!sim.houses[&houses[0]].ai_activation().production);
    let tag = sim.trigger_runtime.attachments[&TagAttachment::Object(actor)];
    let live = sim.trigger_runtime.tag(tag).unwrap().triggers[0];
    assert_eq!(
        sim.trigger_runtime.trigger(live).unwrap().retained_house,
        Some(houses[2])
    );
    assert_eq!(sim.live_action_house(live, 8997, &rules), Some(houses[2]));
    assert_eq!(sim.live_action_house(live, 9001, &rules), None);
    // ChangeOwner's null-killer Unit E0 callback744720 raises48/29 before
    // the retained PerCell tail59; see unit_record_kill_ native-prefix checks.
    // Techno6F5115 then raises34 on the still-attached object Tag.
    assert_eq!(
        sim.trigger_runtime.dispatch_trace,
        [(tag, 1), (tag, 48), (tag, 29), (tag, 59), (tag, 34)]
    );
}

#[test]
fn line_arrival_rereads_later_cell_attachments_after_real_tag_cleanup() {
    let (mut sim, rules, _, actor) = sparse_line_ingress_fixture();
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nLINE=Country0,<none>,Line,0,1,1,1,0\n\
         [Tags]\nTAG=0,Line,LINE\n[Events]\nLINE=1,26,0,0\n\
         [Actions]\nLINE=1,28,0,26,0,0,0,0,A\n\
         [CellTags]\n2003=TAG\n3003=TAG\n",
    );
    let mut terrain = sim.resolved_terrain.take().unwrap();
    let bounds = crate::sim::scenario_bootstrap::NativeStartBounds::from_session(&sim, &terrain);
    sim.trigger_runtime.initialize_cell_trigger_lines(
        bounds,
        &mut terrain,
        (1..5).flat_map(|y| (1..5).map(move |x| (x, y))),
    );
    sim.resolved_terrain = Some(terrain);
    let first = Simulation::trigger_cell_identity(&sim.resolved_terrain, (3, 2));
    let tag = sim.trigger_runtime.cell_tags[&first];
    sim.per_cell_process(
        actor,
        crate::sim::movement::PerCellReason::Arrival,
        &rules,
        None,
        FrameEffects::default(),
    )
    .unwrap();
    assert_eq!(sim.trigger_runtime.dispatch_trace, [(tag, 26)]);
    assert_eq!(sim.trigger_runtime.globals_set, BTreeSet::from([26]));
    assert!(
        sim.trigger_runtime.cell_tags.is_empty(),
        "repeat0 removes every attachment of the shared Tag while the row scan is active"
    );
    let later = Simulation::trigger_cell_identity(&sim.resolved_terrain, (3, 3));
    assert_ne!(
        sim.resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_flags(later)
            & 0x200000,
        0,
        "line bits remain authored map attributes after Tag detach"
    );
    sim.trigger_runtime.dispatch_trace.clear();
    sim.per_cell_process(
        actor,
        crate::sim::movement::PerCellReason::Arrival,
        &rules,
        None,
        FrameEffects::default(),
    )
    .unwrap();
    assert!(sim.trigger_runtime.dispatch_trace.is_empty());
}

/// Drive the production dispatch owner at a chosen native frame without
/// advancing unrelated simulation phases in focused action tests.
fn advance_trigger_frame(
    sim: &mut Simulation,
    current_frame: u32,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    waypoints: &HashMap<u32, Waypoint>,
) -> Vec<TriggerEffect> {
    sim.session.set_binary_frame_for_test(current_frame);
    sim.session.install_waypoints(waypoints);
    if sim.resolved_terrain.is_none()
        && sim
            .trigger_runtime
            .definitions
            .trigger_types
            .iter()
            .any(|typ| {
                typ.actions
                    .iter()
                    .any(|action| matches!(action.entry.kind, 48 | 112))
            })
    {
        // Supply native Cell storage for camera actions. Unset valid waypoints
        // retain the original(0,0) cell; no diagnostic waypoint shadow is used.
        sim.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
    }
    sim.advance_triggers(
        rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
        None,
        FrameEffects::default(),
    )
}

fn flat_trigger_playfield_terrain(
    width: u16,
    height: u16,
) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
    crate::map::resolved_terrain::test_grid(
        width,
        height,
        crate::map::resolved_terrain::test_clear_cell,
    )
}

/// One fixture INI body behind a minimal map header, read by the production
/// map reader.
fn trigger_fixture_map(body: &str) -> crate::map::map_file::MapFile {
    let text = format!(
        "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Basic]\nNewINIFormat=4\n{body}"
    );
    crate::map::map_file::MapFile::from_bytes(text.as_bytes()).unwrap()
}

/// Install the map's trigger definitions through the production scenario-init
/// reader, without Team, EVA or sound tables. Fill (`classify_map_triggers`)
/// stays the caller's next step.
fn install_map_triggers(sim: &mut Simulation, map: &crate::map::map_file::MapFile) {
    sim.initialize_trigger_definitions(
        map,
        &crate::rules::team_ai_ini::TeamAiIniRegistry::default(),
        None,
        None,
    );
}

/// One ordinary production master frame: no commands, the 67 ms tick and
/// default frame effects. With `poll_triggers` the frame polls the installed
/// triggers with the same `rules`.
fn ordinary_master_frame(
    sim: &mut Simulation,
    rules: Option<&crate::rules::ruleset::RuleSet>,
    overlays: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    poll_triggers: bool,
) -> crate::sim::world::TickResult {
    sim.advance_master_frame(
        &[],
        rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()),
        overlays,
        67,
        TickLane::Ordinary,
        poll_triggers,
        FrameEffects::default(),
    )
    .expect("the ordinary fixture frame completes")
    .into_tick()
}

/// A waypoint table holding only `index` at `(rx, ry)`.
fn waypoint_map(index: u32, (rx, ry): (u16, u16)) -> HashMap<u32, Waypoint> {
    HashMap::from([(index, Waypoint { index, rx, ry })])
}

fn spawn_type(sim: &mut Simulation, type_id: &str) -> u64 {
    let sid = sim.substrate.allocate_stable_id();
    let owner_id = sim.interner.intern("Neutral");
    let type_id_interned = sim.interner.intern(type_id);
    let ge = GameEntity::new_at_frame_zero_for_test(
        sid,
        0,
        0,
        0,
        0,
        owner_id,
        crate::sim::combat::Health::new(100),
        type_id_interned,
        EntityCategory::Unit,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(ge);
    sid
}

#[test]
fn create_building_type10_reaches_placement_and_house_publication() {
    use crate::rules::ini_parser::IniFile;
    use crate::sim::house_state::HouseState;

    // ALL01's cannon restoration uses type10, not type9. Original
    // Read6DD5B0/Execute125/6E4200 binds the inline+54 and alphabetic+44
    // independently. This is a Rust production-chain regression; the native
    // reader outputs are pinned separately in trigger_inline_type.json.
    // Reuse the complete shared placement fixture (native tile catalog,
    // passability, overlays and usable area), rather than a terrain-only
    // camera fixture. Type/art data still enter the production readers.
    let (mut sim, rules, overlays) =
        crate::sim::world::entry_test_fixture::fixture_with_rules_and_fixed_art(
            "[BuildingTypes]\n1=GTGCAN\n[GTGCAN]\nStrength=500\n",
            &IniFile::from_str("[GTGCAN]\nFoundation=1x1\n"),
        );
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, Some(owner), false, 0, 10));
    sim.session.install_waypoints(&waypoint_map(139, (10, 10)));
    assert!(crate::sim::build_site::can_place_building_at(
        &sim,
        &rules,
        Some(&overlays),
        rules.object("GTGCAN").unwrap(),
        (10, 10),
        Some(owner),
        None,
        None,
    ));
    assert!(sim.trigger_waypoint_coord(139).is_some());
    let ini = IniFile::from_str("[Actions]\nRESTORE=1,125,10,GTGCAN,0,0,0,0,EJ\n");
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("RESTORE")
        .unwrap()
        .entries()[0]
        .clone();
    let action = sim
        .trigger_runtime
        .definitions
        .resolve_action(entry, None, None);
    assert!(sim.apply_live_trigger_action(
        &action,
        TriggerActionContext {
            live_index: None,
            incoming_house: Some(owner),
            input: TagEventInput::default(),
        },
        &mut Vec::new(),
        &rules,
        Some(&overlays),
        FrameEffects::default(),
    ));
    let building = sim
        .substrate
        .entities
        .values()
        .find(|entity| entity.category() == EntityCategory::Structure && entity.owner() == owner)
        .expect("CreateBuilding must publish the building, not silently reject an empty type");
    assert_eq!(sim.resolve(building.type_ref()), "GTGCAN");
    assert_eq!((building.position.rx(), building.position.ry()), (10, 10));
    assert!(building.lifecycle.object_alive() && building.lifecycle.cell_marked());
    assert!(!building.lifecycle.in_limbo());
    assert!(
        sim.houses[&owner]
            .base_projection()
            .buildings()
            .contains(&building.stable_id())
    );
}

#[test]
fn destroyed_house_predicates_match_original_independent_signed_counters() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::house_state::HouseState;

    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_destroyed_house_events.json",
    ))
    .unwrap();
    let mut country_ini = String::from("[Countries]\n");
    for index in 0..8 {
        country_ini.push_str(&format!("{index}=Country{index}\n"));
    }
    for index in 0..8 {
        country_ini.push_str(&format!("[Country{index}]\n"));
    }
    let rules = RuleSet::from_ini(&IniFile::from_str(&country_ini)).unwrap();
    for case in corpus["predicate_controls"].as_array().unwrap() {
        let mut sim = Simulation::new();
        let houses: Vec<_> = (0..3)
            .map(|index| {
                let owner = sim.interner.intern(&format!("House{index}"));
                let country = sim
                    .interner
                    .intern(if index == 1 { "Country7" } else { "Country0" });
                sim.houses.register(HouseState::new(
                    owner,
                    index as i32,
                    Some(country),
                    false,
                    0,
                    10,
                ));
                let totals = &case["inputs"]["destruction_counts"][index as usize];
                sim.houses
                    .get_mut(&owner)
                    .unwrap()
                    .tracking_mut()
                    .set_for_test(
                        totals[0].as_i64().unwrap() as i32,
                        &[],
                        (
                            totals[1].as_i64().unwrap() as i32,
                            totals[2].as_i64().unwrap() as i32,
                            totals[3].as_i64().unwrap() as i32,
                        ),
                        &[],
                    );
                owner
            })
            .collect();
        sim.houses.reorder_for_test(
            &case["inputs"]["house_order"]
                .as_array()
                .unwrap()
                .iter()
                .map(|index| houses[index.as_u64().unwrap() as usize])
                .collect::<Vec<_>>(),
        );
        let map = trigger_fixture_map(&format!(
            "[Triggers]\nDESTRUCTION=Neutral,<none>,Destruction,0,1,1,1,0\n\
             [Events]\nDESTRUCTION={}\n\
             [Tags]\nDESTRUCTION_TAG=0,Destruction,DESTRUCTION\n",
            case["raw"].as_str().unwrap()
        ));
        install_map_triggers(&mut sim, &map);
        sim.classify_map_triggers(&rules);
        let live = sim.trigger_runtime.trigger_order[0];
        let type_index = sim.trigger_runtime.trigger(live).unwrap().type_index;
        sim.trigger_runtime.definitions.trigger_types[type_index].events[0].retained_house =
            Some(houses[1]);
        let timer_before = sim.trigger_runtime.trigger(live).unwrap().timer;
        let mut persistence = false;
        sim.session.set_binary_frame_for_test(123);
        let mut evaluate = |sim: &mut Simulation| {
            sim.has_occurred(
                0,
                TagEventInput {
                    kind: case["inputs"]["raised"].as_i64().unwrap() as i32,
                    actor: None,
                    source: None,
                    cell: (0, 0),
                    force: false,
                },
                case["inputs"]["incoming_house"]
                    .as_u64()
                    .map(|index| houses[index as usize]),
                &mut persistence,
                &rules,
                live,
            )
        };
        let result = if case["inputs"]["scenario_init"].as_bool().unwrap() {
            sim.with_buildup_bypass(evaluate)
        } else {
            evaluate(&mut sim)
        };
        assert_eq!(result, case["result"].as_u64().unwrap() != 0, "{case}");
        assert_eq!(
            persistence,
            case["persistence"].as_u64().unwrap() != 0,
            "{case}"
        );
        assert_eq!(
            sim.trigger_runtime.trigger(live).unwrap().timer,
            timer_before
        );
        assert_eq!(
            sim.trigger_runtime.definitions.trigger_types[type_index].events[0].retained_house,
            Some(houses[1]),
            "9/11 do not replace Event+54: {case}"
        );
    }
}

#[test]
fn damage_at_waypoint_reaches_the_shared_building_receiver() {
    use crate::rules::{ini_parser::IniFile, ruleset::RuleSet};
    use crate::sim::house_state::HouseState;

    // ALL01's Damage_Reactor uses +90=364. The trailing waypoint token is
    // deliberately different so it cannot accidentally supply the impact.
    // Native Execute63 -> 6E0490 -> AreaDamage489280 establishes that this
    // action damages a present building; numeric parity is tested separately.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[BuildingTypes]\n0=REACTOR\n[REACTOR]\nStrength=1000\nArmor=concrete\n\
         [CombatDamage]\nC4Warhead=Super\n[Warheads]\n0=Super\n\
         [Super]\nCellSpread=0\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
    ))
    .unwrap();
    let mut sim = Simulation::with_seed(31);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
    let owner = sim.interner.intern("Neutral");
    sim.houses
        .register(HouseState::new(owner, 0, None, false, 0, 10));
    let type_id = sim.interner.intern("REACTOR");
    let building = GameEntity::new_at_frame_zero_for_test(
        1,
        10,
        10,
        0,
        0,
        owner,
        crate::sim::combat::Health::new(769),
        type_id,
        EntityCategory::Structure,
        0,
        5,
        false,
    );
    sim.substrate.entities.insert(building);
    sim.add_entity_occupancy(1);
    sim.session.install_waypoints(&waypoint_map(364, (10, 10)));
    let mut definitions = RuntimeDefinitions::default();
    let typ = definitions.trigger_type("DAMAGE").unwrap();
    let ini = IniFile::from_str("[Actions]\nDAMAGE=1,63,0,364,0,0,0,0,A\n");
    let entry = crate::map::actions::parse_actions(&ini)
        .remove("DAMAGE")
        .unwrap()
        .entries()[0]
        .clone();
    let action = definitions.resolve_action(entry, None, None);
    sim.trigger_runtime.definitions = definitions;
    assert!(sim.apply_live_trigger_action(
        &action,
        sim.trigger_action_context(typ, None, TagEventInput::default(), &rules),
        &mut Vec::new(),
        &rules,
        None,
        FrameEffects::default(),
    ));
    assert!(sim.substrate.entities.get(1).unwrap().health.current() < 769);
}

#[test]
fn parsed_event_records_match_native_list_and_production_predicates() {
    #[derive(serde::Deserialize)]
    struct NativeCondition {
        kind: i32,
        value: i32,
        name: String,
        results: Vec<u8>,
    }
    #[derive(serde::Deserialize)]
    struct NativeRow {
        raw: String,
        conditions: Vec<NativeCondition>,
        frames: Vec<u32>,
    }
    let rows: Vec<NativeRow> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_event_records.json",
    ))
    .unwrap();
    for row in rows {
        let map = trigger_fixture_map(&format!(
            "[Triggers]\nEVENT=Neutral,<none>,Event,0,1,1,1,0\n\
             [Events]\nEVENT={}\n[Actions]\nEVENT=1,112,0,0,0,0,0,0,A\n\
             [Tags]\nEVENT_TAG=0,Event,EVENT\n",
            row.raw
        ));
        let conditions = map.events["EVENT"].conditions();
        assert_eq!(conditions.len(), row.conditions.len(), "{}", row.raw);
        for (rust, native) in conditions.iter().zip(&row.conditions) {
            assert_eq!(
                (
                    rust.kind,
                    rust.value,
                    rust.type_name.as_deref().unwrap_or("")
                ),
                (native.kind, native.value, native.name.as_str()),
                "{}",
                row.raw
            );
        }
        // Timer events need the live Trigger instance port. Type-count reader
        // coverage here does not certify the existing registry/count query.
        if conditions.is_empty()
            || !row.conditions.iter().all(|c| {
                matches!(c.kind, 27 | 28 | 36 | 37 | 47) && c.results.len() == row.frames.len()
            })
        {
            continue;
        }
        for (n, frame) in row.frames.iter().enumerate() {
            let mut sim = Simulation::new();
            sim.session.set_binary_frame_for_test(*frame);
            install_map_triggers(&mut sim, &map);
            sim.classify_map_triggers(crate::rules::ruleset::RuleSet::empty_for_test());
            sim.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
            sim.trigger_runtime.globals_set.insert(7);
            sim.trigger_runtime.locals_set.insert(9);
            let result = sim
                .advance_app_frame(
                    &[],
                    crate::rules::ruleset::RuleSet::empty_for_test(),
                    None,
                    67,
                    TickLane::Ordinary,
                    crate::sim::world::FrameEffects::default(),
                )
                .unwrap();
            assert!(result.tick.frame_committed);
            assert_eq!(
                !result.trigger_effects.is_empty(),
                row.conditions.iter().all(|c| c.results[n] != 0),
                "{} at frame {frame}",
                row.raw
            );
        }
    }
}

#[test]
fn parsed_variable_actions_match_native_reader_dispatch_and_restore() {
    #[derive(serde::Deserialize)]
    struct NativeRow {
        raw: String,
        kind: i32,
        value: i32,
        waypoint: i32,
        initial_globals: BTreeSet<u32>,
        initial_locals: BTreeSet<u32>,
        globals: BTreeSet<u32>,
        locals: BTreeSet<u32>,
    }
    let corpus: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/trigger_action_values.json",
    ))
    .unwrap();
    // The same native reader producer also owns reference/EVA/input controls.
    // Select the legacy numeric family before deserializing its required fields.
    let rows: Vec<NativeRow> = corpus
        .into_iter()
        .filter(|row| row.get("case_kind").is_none())
        .map(|row| serde_json::from_value(row).unwrap())
        .collect();
    assert_eq!(rows.len(), 61);
    for row in rows {
        let map = trigger_fixture_map(&format!(
            "[Triggers]\nVARIABLE=Neutral,<none>,Variable,0,1,1,1,0\n\
             [Events]\nVARIABLE=1,47,0,0\n[Actions]\nVARIABLE=1,{}\n\
             [Tags]\nVARIABLE_TAG=0,Variable,VARIABLE\n",
            row.raw
        ));
        let entry = &map.actions["VARIABLE"].entries()[0];
        assert_eq!(entry.kind, row.kind, "{}", row.raw);
        assert_eq!(entry.literal_value(), Some(row.value), "{}", row.raw);
        assert_eq!(
            entry.waypoint_index.map_or(-1, |value| value as i32),
            row.waypoint,
            "{}",
            row.raw
        );
        let mut original = Simulation::new();
        install_map_triggers(&mut original, &map);
        original.classify_map_triggers(crate::rules::ruleset::RuleSet::empty_for_test());
        original.trigger_runtime.globals_set = row.initial_globals;
        original.trigger_runtime.locals_set = row.initial_locals;
        let bytes = GameSnapshot::save(&original, 0, 0, "trigger_values", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        restored.restore_after_snapshot_load().unwrap();
        for sim in [&mut original, &mut restored] {
            let before_rng = sim.rng_state();
            let frame = ordinary_master_frame(sim, None, None, true);
            assert!(frame.frame_committed);
            assert_eq!(sim.trigger_runtime.globals_set, row.globals, "{}", row.raw);
            assert_eq!(sim.trigger_runtime.locals_set, row.locals, "{}", row.raw);
            assert_eq!(
                sim.rng_state(),
                before_rng,
                "the poll and the variable action draw no RNG"
            );
        }
        assert_eq!(
            original.trigger_runtime, restored.trigger_runtime,
            "{}",
            row.raw
        );
    }
}

#[test]
fn parsed_map_trigger_flags_gate_production_frames_and_survive_restore() {
    // Use actual map text, not hand-built MapTrigger booleans. The enabled
    // and difficulty expectations are established by trigger_type_flags.json.
    let map = trigger_fixture_map(
        "[Triggers]\n\
         ACTIVE=Neutral,<none>,Active,0,1,1,1,0\n\
         DISABLED=Neutral,<none>,Disabled,1,1,1,1,0\n\
         NO_MEDIUM=Neutral,<none>,Other difficulty,0,1,0,1,0\n\
         MISSING=Neutral,<none>,Missing flags\n\
         SHIFTED=Neutral,<none>,Empty token,,0,0,-2,0,0\n\
         [Events]\n\
         ACTIVE=1,47,0,0\nDISABLED=1,47,0,0\nNO_MEDIUM=1,47,0,0\n\
         MISSING=1,47,0,0\nSHIFTED=1,47,0,0\n\
         [Actions]\n\
         ACTIVE=1,112,0,0,0,0,0,0,A\n\
         DISABLED=1,112,0,0,0,0,0,0,B\n\
         NO_MEDIUM=1,112,0,0,0,0,0,0,C\n\
         MISSING=1,112,0,0,0,0,0,0,D\n\
         SHIFTED=1,112,0,0,0,0,0,0,E\n\
         [Tags]\n\
         ACTIVE_TAG=0,Active,ACTIVE\nDISABLED_TAG=0,Disabled,DISABLED\n\
         NO_MEDIUM_TAG=0,Other difficulty,NO_MEDIUM\n\
         MISSING_TAG=0,Missing flags,MISSING\nSHIFTED_TAG=0,Empty token,SHIFTED\n",
    );
    let mut original = Simulation::new();
    install_map_triggers(&mut original, &map);
    original.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
    // Classify before saving.
    original.classify_map_triggers(crate::rules::ruleset::RuleSet::empty_for_test());
    // TriggerTypes are allocated in [Triggers] key order.
    assert!(
        original
            .trigger_runtime
            .definitions
            .trigger_types
            .iter()
            .filter(|t| !t.enabled || !t.admits_difficulty(1))
            .map(|t| t.id.as_str())
            .eq(["DISABLED", "NO_MEDIUM", "MISSING"])
    );
    let bytes = GameSnapshot::save(&original, 0, 0, "trigger_flags", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    for sim in [&mut original, &mut restored] {
        // Terrain is a resource-derived authority restored separately from
        // GameSnapshot. Supply the same canonical fixture to both candidates.
        sim.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
        let frame = sim
            .advance_app_frame(
                &[],
                crate::rules::ruleset::RuleSet::empty_for_test(),
                None,
                67,
                TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("production frame completes");
        assert!(frame.tick.frame_committed);
        assert_eq!(
            frame.trigger_effects,
            vec![
                TriggerEffect::CameraAtCoord {
                    coord: [128, 128, 0],
                    velocity: None
                },
                TriggerEffect::CameraAtCoord {
                    coord: [128, 128, 0],
                    velocity: None
                },
            ]
        );
    }
    assert_eq!(original.trigger_runtime, restored.trigger_runtime);
}

#[test]
fn change_visible_map_area_uses_native_parameter_indices_and_atoi() {
    let fields = vec![
        "991".to_string(),
        "-774".to_string(),
        "2cells".to_string(),
        "41".to_string(),
        "junk".to_string(),
        "+38tail".to_string(),
        "A".to_string(),
    ];
    assert_eq!(parse_visible_map_area(&fields), Some([2, 41, 0, 38]));
    assert_eq!(parse_visible_map_area(&fields[..5]), None);
}

#[test]
fn trigger_action_40_normalizes_and_refreshes_authority_same_frame() {
    use crate::map::map_file::MapHeader;
    use crate::map::playfield::PlayfieldBounds;
    use crate::map::resolved_terrain::zone_class;
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::pathfinding::PathGrid;
    use crate::sim::pathfinding::zone_map::ZONE_INVALID;

    // Stock all06umd-shaped payload: 40,0,0,2,41,56,38,A. With params equal
    // to chunk[1..], ParamType/Param3 are the first two zeroes and LocalSize
    // is exactly params[2..=5].
    let map = trigger_fixture_map(
        "[Triggers]\nAREA=Neutral,<none>,Change visible map area,0,1,1,1,0\n\
         [Events]\nAREA=1,47,0,0\n[Actions]\nAREA=1,40,0,0,2,41,56,38,A\n\
         [Tags]\nAREA_TAG=0,Change visible map area,AREA\n",
    );
    let header = MapHeader {
        theater: "TEMPERATE".to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width: 80,
        height: 58,
        local_left: 2,
        local_top: 4,
        local_width: 76,
        local_height: 48,
    };
    let mut sim = Simulation::new();
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    sim.install_playfield_from_map_header(&header);
    let initial_bounds = sim.playfield_bounds().expect("initial playfield");
    let mut terrain = flat_trigger_playfield_terrain(100, 100);
    terrain.recalc_playfield_attributes(initial_bounds);
    let path_grid = PathGrid::from_resolved_terrain(&terrain);
    sim.resolved_terrain = Some(terrain);
    sim.rebuild_zone_grid(&path_grid);
    install_map_triggers(&mut sim, &map);
    sim.classify_map_triggers(crate::rules::ruleset::RuleSet::empty_for_test());

    let before_zone = sim
        .zone_grid
        .as_ref()
        .and_then(|zones| zones.map_for(MovementZone::Normal))
        .expect("normal zone")
        .zone_at(50, 50, MovementLayer::Ground);
    assert_ne!(before_zone, ZONE_INVALID);
    assert!(
        !sim.resolved_terrain
            .as_ref()
            .unwrap()
            .cell(50, 50)
            .unwrap()
            .outside_playfield
    );

    let tick = ordinary_master_frame(&mut sim, None, None, true);

    assert_eq!(
        sim.playfield_bounds(),
        Some(PlayfieldBounds {
            base: 80,
            off_fc: 2,
            off_100: 41,
            off_104: 56,
            // Raw 38 clips to Size bottom 17, then the six-cell margin caps 11.
            off_108: 11,
        })
    );
    let terrain = sim.resolved_terrain.as_ref().unwrap();
    assert!(terrain.cell(50, 50).unwrap().outside_playfield);
    assert_eq!(terrain.cell(50, 50).unwrap().zone_type, zone_class::OUTSIDE);
    assert!(!terrain.cell(85, 85).unwrap().outside_playfield);
    assert_eq!(terrain.cell(85, 85).unwrap().zone_type, zone_class::GROUND);
    let zones = sim.zone_grid.as_ref().expect("zones rebuilt");
    let normal = zones.map_for(MovementZone::Normal).expect("normal zone");
    assert_eq!(normal.zone_at(50, 50, MovementLayer::Ground), ZONE_INVALID);
    assert_ne!(normal.zone_at(85, 85, MovementLayer::Ground), ZONE_INVALID);
    assert!(sim.dirty_cells.radar().is_empty());
    assert_eq!(sim.dirty_cells.radar_generation(), 0);
    assert_eq!(sim.playfield_revision(), 1);
    assert_eq!(tick.state_hash, Some(sim.state_hash()));

    // FUN_006E21E0 rebuilds radar surfaces for every firing. A second writer
    // must not disappear behind the persistent per-cell dirty-list dedup gate.
    assert!(sim.change_visible_map_area(
        [4, 40, 54, 12],
        crate::rules::ruleset::RuleSet::empty_for_test()
    ));
    assert_eq!(sim.playfield_revision(), 2);
    assert!(sim.change_visible_map_area(
        [4, 40, 54, 12],
        crate::rules::ruleset::RuleSet::empty_for_test()
    ));
    assert_eq!(sim.playfield_revision(), 3);
    assert!(sim.dirty_cells.radar().is_empty());

    let bytes = GameSnapshot::save(&sim, 0, 0, "all06umd.map", 0);
    let restored = GameSnapshot::load(&bytes)
        .expect("action-40 snapshot loads")
        .sim;
    assert_eq!(restored.playfield_bounds(), sim.playfield_bounds());
    assert_eq!(
        restored.playfield_size_height(),
        sim.playfield_size_height()
    );
    assert_eq!(restored.playfield_revision(), sim.playfield_revision());
}

#[test]
fn techno_playfield_action_40_exact_recompute_and_mobile_reveal_callback() {
    use crate::map::map_file::MapHeader;
    use crate::map::playfield::PlayfieldBounds;
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::combat::Health;
    use crate::sim::house_state::HouseState;

    let header = MapHeader {
        theater: "TEMPERATE".to_string(),
        fill: "Clear".to_string(),
        level: 0,
        width: 80,
        height: 58,
        local_left: 30,
        local_top: 20,
        local_width: 8,
        local_height: 6,
    };
    let mut sim = Simulation::new();
    sim.install_playfield_from_map_header(&header);
    let initial = sim.playfield_bounds().unwrap();
    let expanded = PlayfieldBounds::from_raw_local_size(80, 58, [2, 2, 76, 48]);
    let candidates: Vec<(u16, u16)> = (0u16..100)
        .flat_map(|ry| (0u16..100).map(move |rx| (rx, ry)))
        .filter(|&(rx, ry)| {
            expanded.contains_height_aware_packed(rx.into(), ry.into(), 0, 0)
                && !initial.contains_height_aware_packed(rx.into(), ry.into(), 0, 0)
        })
        .collect();
    let mobile_cell = candidates
        .get(candidates.len() / 2)
        .copied()
        .expect("expansion adds cells");
    let building_cell = candidates
        .iter()
        .copied()
        .max_by_key(|&(rx, ry)| {
            i32::from(rx).abs_diff(i32::from(mobile_cell.0))
                + i32::from(ry).abs_diff(i32::from(mobile_cell.1))
        })
        .expect("distant expansion cell");
    assert!(mobile_cell.0.abs_diff(building_cell.0) + mobile_cell.1.abs_diff(building_cell.1) > 10);
    let veteran_probe = [(3i32, 0i32), (-3, 0), (0, 3), (0, -3)]
        .into_iter()
        .find_map(|(dx, dy)| {
            let rx = i32::from(mobile_cell.0) + dx;
            let ry = i32::from(mobile_cell.1) + dy;
            (rx >= 0
                && ry >= 0
                && rx < 100
                && ry < 100
                && expanded.contains_height_aware_packed(rx, ry, 0, 0))
            .then_some((rx as u16, ry as u16))
        })
        .expect("effective veteran sight probe");
    // `UpdateReveal @ 0x0070AF50`: the veteran bonus is `ftol(Sight *
    // VeteranSight)` for a `SIGHT`-ability type, so the probe at distance 3
    // needs `1 * 3` and the ability — not the old additive `1 + 3`.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[General]\nVeteranSight=3\nRevealByHeight=no\n\
         [VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nVeteranAbilities=SIGHT\n",
    ))
    .expect("effective-sight action fixture");

    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, Some(owner), true, 0, 10));
    sim.fog.set_width_for_test(100);
    sim.fog.set_height_for_test(100);
    let mobile_type = sim.interner.intern("MTNK");
    let building_type = sim.interner.intern("GAPOWR");
    let mut mobile = GameEntity::new_at_frame_zero_for_test(
        1,
        mobile_cell.0,
        mobile_cell.1,
        0,
        0,
        owner,
        Health::new(100),
        mobile_type,
        EntityCategory::Unit,
        100,
        1,
        true,
    );
    mobile.lifecycle.set_object_alive_for_test(true);
    mobile.lifecycle.set_in_limbo_for_test(false);
    let mut building = GameEntity::new_at_frame_zero_for_test(
        2,
        building_cell.0,
        building_cell.1,
        0,
        0,
        owner,
        Health::new(100),
        building_type,
        EntityCategory::Structure,
        0,
        1,
        false,
    );
    building.lifecycle.set_object_alive_for_test(true);
    building.lifecycle.set_in_limbo_for_test(false);
    sim.substrate.entities.insert(mobile);
    sim.substrate.entities.insert(building);
    sim.resolved_terrain = Some(flat_trigger_playfield_terrain(100, 100));

    assert!(sim.change_visible_map_area([2, 2, 76, 48], &rules));
    assert!(sim.substrate.entities.get(1).unwrap().in_playfield());
    assert!(sim.substrate.entities.get(2).unwrap().in_playfield());
    assert!(
        sim.fog
            .is_cell_revealed(owner, mobile_cell.0, mobile_cell.1)
    );
    assert!(
        sim.fog
            .is_cell_revealed(owner, veteran_probe.0, veteran_probe.1),
        "0x0070ADC0 callback must use the shared veteran-adjusted sight, not raw VisionRange"
    );
    assert!(
        !sim.fog
            .is_cell_revealed(owner, building_cell.0, building_cell.1),
        "FUN_006E21E0 excludes BuildingClass from the false-to-true callback"
    );

    assert!(sim.change_visible_map_area([30, 20, 8, 6], &rules));
    assert!(!sim.substrate.entities.get(1).unwrap().in_playfield());
    assert!(!sim.substrate.entities.get(2).unwrap().in_playfield());
    assert!(sim.change_visible_map_area([2, 2, 76, 48], &rules));
    assert!(sim.substrate.entities.get(1).unwrap().in_playfield());
    assert!(sim.substrate.entities.get(2).unwrap().in_playfield());
}

#[test]
fn master_frame_polls_triggers_before_logic_houses_commit_and_delete() {
    let map = trigger_fixture_map(
        "[Triggers]\nTRIG_A=Neutral,<none>,Frame Zero,0,1,1,1,0\n\
         [Events]\nTRIG_A=1,47,0,0\n[Actions]\nTRIG_A=1,112,0,0,0,0,0,0,J\n\
         [Tags]\nTRIG_A_TAG=0,Frame Zero,TRIG_A\n",
    );
    let mut sim = Simulation::new();
    sim.resolved_terrain = Some(flat_trigger_playfield_terrain(32, 32));
    install_map_triggers(&mut sim, &map);
    sim.classify_map_triggers(crate::rules::ruleset::RuleSet::empty_for_test());

    let tick = ordinary_master_frame(&mut sim, None, None, true);

    assert!(tick.frame_committed);
    assert_eq!(
        sim.take_master_frame_test_trace(),
        vec![
            MasterFrameTestRung::SessionCommands,
            MasterFrameTestRung::Triggers,
            MasterFrameTestRung::TeamScript,
            MasterFrameTestRung::LogicVector,
            MasterFrameTestRung::Houses,
            MasterFrameTestRung::FrameCommit,
            MasterFrameTestRung::PendingDelete,
        ]
    );
    assert_eq!(
        sim.drain_trigger_effects(),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
}

#[test]
fn master_frame_save_load_continues_trigger_projectile_and_delete_state() {
    let map = trigger_fixture_map(
        "[Triggers]\nTRIG_A=Neutral,<none>,Set Global,0,1,1,1,0\n\
         [Events]\nTRIG_A=1,47,0,0\n[Actions]\nTRIG_A=1,28,0,13,0,0,0,0,A\n\
         [Tags]\nTRIG_A_TAG=0,Set Global,TRIG_A\n",
    );
    let mut original = Simulation::new();
    original.hash_state_every(std::num::NonZeroU64::MIN);
    install_map_triggers(&mut original, &map);
    original.classify_map_triggers(crate::rules::ruleset::RuleSet::empty_for_test());
    ordinary_master_frame(&mut original, None, None, true);
    assert!(original.trigger_runtime.globals_set.contains(&13));

    let projectile_id = original.substrate.allocate_stable_id();
    original.admit_projectile(
        projectile_id,
        ProjectileSpawn {
            native_unique_id: 0,
            line_trail: None,
            flat: false,
            source_id: crate::sim::combat::RAD_NO_ATTACKER,
            origin: ProjectileCoord::new(0, 0, 0),
            target: ProjectileTarget::Cell { rx: 4, ry: 0 },
            initial_target_position: ProjectileCoord::new(1024, 0, 0),
            payload: ProjectilePayload::new(
                40,
                crate::sim::intern::InternedId::from_index(0),
                None,
                false,
            ),
            speed_leptons_per_frame: 64,
            velocity: crate::sim::projectile::ProjectileVelocity::new(64, 0, 0),
            trajectory: crate::sim::projectile::ProjectileTrajectory::Straight,
            guidance: None,
            visual: crate::sim::projectile::ProjectileVisualState::new(0, 0, 0),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        },
    );
    let deleted_id = spawn_type(&mut original, "GAPOWR");
    original.uninit(deleted_id);
    assert_eq!(original.projectiles.len(), 1);
    assert!(
        original
            .substrate
            .pending_delete
            .contains_object(&deleted_id)
    );

    // Native in-scenario load restarts Scenario RNG from Seed0. Continue both
    // branches from that cursor while testing unrelated trigger/lifecycle state.
    original.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash_before_save = original.state_hash();
    let bytes = GameSnapshot::save(&original, 0, 0, "test_map", 0);
    let mut restored = GameSnapshot::load(&bytes).expect("snapshot loads").sim;
    restored
        .restore_after_snapshot_load()
        .expect("snapshot references and Logic membership restore");
    assert_eq!(restored.state_hash(), hash_before_save);
    assert!(restored.trigger_runtime.globals_set.contains(&13));
    assert_eq!(restored.projectiles.len(), 1);
    assert!(
        restored
            .substrate
            .pending_delete
            .contains_object(&deleted_id)
    );

    let original_tick = ordinary_master_frame(&mut original, None, None, true);
    let mut replay_log = ReplayLog::new(ReplayHeader {
        pixel_conversion_bounds: Default::default(),
        version: 1,
        tick_hz: 15,
        seed: original.session.seed(),
        map_name: original.session.map_name().to_owned(),
        rules_hash: 0,
    });
    replay_log.record_tick(original_tick.tick, Vec::new(), original_tick.state_hash);
    let restored_tick = ordinary_master_frame(&mut restored, None, None, true);

    assert!(original_tick.frame_committed);
    assert!(restored_tick.frame_committed);
    assert_eq!(original.state_hash(), restored.state_hash());
    assert_eq!(original.projectiles.len(), 1);
    assert!(original.substrate.pending_delete.is_empty());

    let mut replayed = GameSnapshot::load(&bytes).expect("snapshot reloads").sim;
    replayed
        .restore_after_snapshot_load()
        .expect("replay snapshot references and Logic membership restore");
    assert_eq!(
        ReplayRunner::run_fixture_master_frame(
            &mut replayed,
            &replay_log,
            None,
            None,
            None,
            67,
            true,
        ),
        vec![original_tick.state_hash],
    );
}

#[test]
fn trigger_runtime_latches_participate_in_state_hash() {
    let mut sim = Simulation::new();
    let before = sim.state_hash();

    sim.trigger_runtime.globals_set.insert(13);

    assert_ne!(sim.state_hash(), before);
}

#[test]
fn elapsed_time_uses_signed_current_frame_divided_by_fifteen() {
    let sim = Simulation::new();
    let one_second = EventCondition {
        kind: 47,
        value: 1,
        ..Default::default()
    };
    let zero_seconds = EventCondition {
        kind: 47,
        value: 0,
        ..Default::default()
    };

    assert!(sim.trigger_runtime.evaluate_event(&zero_seconds, 0, &sim));
    assert!(!sim.trigger_runtime.evaluate_event(&one_second, 14, &sim));
    assert!(sim.trigger_runtime.evaluate_event(&one_second, 15, &sim));
    assert!(
        !sim.trigger_runtime
            .evaluate_event(&one_second, 0x8000_0000, &sim),
        "the native frame counter is divided as a signed 32-bit value"
    );
}

/// Exercise the existing action dispatcher with the production TAction reader.
/// The original Read6DD5B0 puts a type2 token3 reference at +50; token2 is
/// metadata, even when a TriggerType named "2" happens to exist.
fn dispatch_typed_trigger_reference(kind: i32, target_enabled: bool) -> Simulation {
    let disabled = u8::from(!target_enabled);
    let map = trigger_fixture_map(&format!(
        "[Triggers]\nSOURCE=Neutral,<none>,Source,0,1,1,1,0\n\
         0DD8E18C=Neutral,<none>,Intro,{disabled},1,1,1,0\n\
         2=Neutral,<none>,Metadata decoy,{disabled},1,1,1,0\n\
         [Actions]\nSOURCE=1,{kind},2,0DD8E18C,0,0,0,0,A\n\
         0DD8E18C=1,28,0,7,0,0,0,0,A\n2=1,28,0,8,0,0,0,0,A\n\
         [Tags]\nSOURCE_TAG=0,Source,SOURCE\n0DD8E18C_TAG=0,Intro,0DD8E18C\n\
         2_TAG=0,Metadata decoy,2\n"
    ));
    let mut sim = Simulation::new();
    install_map_triggers(&mut sim, &map);
    for tag in 0..sim.trigger_runtime.definitions.tag_types.len() {
        sim.construct_trigger_tag(tag);
    }
    let source = trigger_type_index(&sim, "SOURCE");
    let action = sim.trigger_runtime.definitions.trigger_types[source].actions[0].clone();
    sim.apply_live_trigger_action(
        &action,
        sim.trigger_action_context(
            source,
            None,
            TagEventInput::default(),
            crate::rules::ruleset::RuleSet::empty_for_test(),
        ),
        &mut Vec::new(),
        crate::rules::ruleset::RuleSet::empty_for_test(),
        None,
        FrameEffects::default(),
    );
    sim
}

#[test]
fn typed_trigger_reference_force_uses_operand_instead_of_metadata() {
    let sim = dispatch_typed_trigger_reference(22, true);
    assert_eq!(sim.trigger_runtime.globals_set, [7].into_iter().collect());
}

#[test]
fn typed_trigger_reference_enable_and_disable_use_operand_instead_of_metadata() {
    // Enable53 starts from disabled Triggers, Disable54 from enabled ones;
    // only the operand Trigger flips, never the "2" metadata decoy.
    for (kind, initially_enabled) in [(53, false), (54, true)] {
        let sim = dispatch_typed_trigger_reference(kind, initially_enabled);
        let enabled = |id: &str| {
            let live = live_trigger_of_type(&sim, trigger_type_index(&sim, id));
            sim.trigger_runtime.trigger(live).unwrap().enabled
        };
        assert_eq!(enabled("0DD8E18C"), !initially_enabled, "kind {kind}");
        assert_eq!(enabled("2"), initially_enabled, "kind {kind}");
    }
}

#[test]
fn mission_announce_then_end_uses_the_player_house_and_shared_timer() {
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::sim::house_state::HouseState;

    // Native67/69 change the PlayerPtr's result and existing+298 timer;
    // they do not manufacture a message or directly select a result screen.
    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[Countries]\n0=PlayerCountry\n1=OtherCountry\n[PlayerCountry]\n[OtherCountry]\n[AudioVisual]\nSavourDelay=.1\n",
    )).unwrap();
    let mut sim = Simulation::new();
    let player = sim.interner.intern("PlayerHouse");
    let other = sim.interner.intern("OtherHouse");
    let player_country = sim.interner.intern("PlayerCountry");
    let other_country = sim.interner.intern("OtherCountry");
    sim.houses.register(HouseState::new(
        player,
        0,
        Some(player_country),
        true,
        0,
        10,
    ));
    sim.houses
        .register(HouseState::new(other, 0, Some(other_country), false, 0, 10));
    sim.session.set_current_house_for_test(Some(player));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(false));
    let map = trigger_fixture_map(
        "[Triggers]\nRESULT=OtherCountry,<none>,End Mission,0,1,1,1,0\n\
         [Events]\nRESULT=1,47,0,1\n[Actions]\nRESULT=2,67,0,0,0,0,0,0,A,69,0,0,0,0,0,0,A\n\
         [Tags]\nRESULT_TAG=0,End Mission,RESULT\n",
    );
    let before = sim.houses[&player].outcome_timer();
    install_map_triggers(&mut sim, &map);
    sim.classify_map_triggers(&rules);
    let effects = advance_trigger_frame(&mut sim, 15, Some(&rules), &HashMap::new());
    assert!(
        effects.is_empty(),
        "results use the existing House/audio exit path"
    );
    assert!(sim.houses[&player].has_won());
    assert!(!sim.houses[&player].has_lost());
    assert!(!sim.houses[&other].has_won());
    assert!(!sim.houses[&other].has_lost());
    assert_eq!(
        sim.houses[&player].outcome_timer().remaining(15),
        before.remaining(15),
        "67 preserves duration;69 resumes it instead of starting SavourDelay"
    );
    assert!(sim.ready_outcome_for_owner(player).is_none());
    assert!(
        sim.houses
            .get_mut(&player)
            .unwrap()
            .advance_outcome_savour(15, false)
    );
    assert!(sim.ready_outcome_for_owner(player).is_some());
}

#[test]
fn techtype_exists_and_not_exists_query_simulation_world() {
    let map = trigger_fixture_map(
        "[Triggers]\nTRIG_A=Neutral,<none>,Need Two Power Plants,0,1,1,1,0\n\
         TRIG_B=Neutral,<none>,No Radar,0,1,1,1,0\n\
         [Events]\nTRIG_A=1,60,2,2,GAPOWR\nTRIG_B=1,61,2,0,GAAIRC\n\
         [Actions]\nTRIG_A=1,112,0,0,0,0,0,0,L\nTRIG_B=1,112,0,0,0,0,0,0,M\n\
         [Tags]\nTRIG_A_TAG=0,Need Two Power Plants,TRIG_A\nTRIG_B_TAG=0,No Radar,TRIG_B\n",
    );
    let mut sim = Simulation::new();
    install_map_triggers(&mut sim, &map);
    spawn_type(&mut sim, "GAPOWR");
    spawn_type(&mut sim, "GAPOWR");
    sim.classify_map_triggers(crate::rules::ruleset::RuleSet::empty_for_test());

    // Native55AFB0 revisits the live list length after each Tag. Removing
    // TRIG_A shifts TRIG_B into the current slot, which the increment skips
    // until the next Logic visit; both queries must still fire exactly once.
    assert_eq!(
        advance_trigger_frame(&mut sim, 0, None, &HashMap::new()),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
    assert_eq!(
        advance_trigger_frame(&mut sim, 1, None, &HashMap::new()),
        vec![TriggerEffect::CameraAtCoord {
            coord: [128, 128, 0],
            velocity: None
        }]
    );
    assert!(advance_trigger_frame(&mut sim, 2, None, &HashMap::new()).is_empty());
}

fn run_waypoint_action(
    kind: i32,
    token: Option<&str>,
    trigger_country: &str,
    sim: &mut Simulation,
    waypoints: &HashMap<u32, Waypoint>,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> Vec<TriggerEffect> {
    // The comma after the token keeps a whitespace-only token from being
    // trimmed with the line; strtok skips the empty field it leaves.
    let token = token.map_or(String::new(), |token| format!(",{token},"));
    let map = trigger_fixture_map(&format!(
        "[Triggers]\nWAYPOINT_ACTION={trigger_country},<none>,Waypoint action,0,1,1,1,0\n\
         [Events]\nWAYPOINT_ACTION=1,47,0,0\n\
         [Actions]\nWAYPOINT_ACTION=1,{kind},0,0,0,0,0,0{token}\n\
         [Tags]\nWAYPOINT_ACTION_TAG=0,Waypoint action,WAYPOINT_ACTION\n"
    ));
    install_map_triggers(sim, &map);
    sim.classify_map_triggers(rules.unwrap_or(crate::rules::ruleset::RuleSet::empty_for_test()));
    advance_trigger_frame(sim, 0, rules, waypoints)
}

fn waypoint_action_rules() -> crate::rules::ruleset::RuleSet {
    crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[Countries]\n0=Americans\n1=Russians\n\
         [Americans]\nName=United States\n\
         [Russians]\nName=Russian Federation\n",
    ))
    .expect("waypoint-action country registry parses")
}

fn register_waypoint_action_house(
    sim: &mut Simulation,
    name: &str,
    country: Option<&str>,
    alternate_base_center: (u16, u16),
) -> crate::sim::intern::InternedId {
    let owner = sim.interner.intern(name);
    let country = country.map(|country| sim.interner.intern(country));
    let mut house = crate::sim::house_state::HouseState::new(owner, 0, country, false, 0, 10);
    house.set_base_center_for_test(Some((40, 50)));
    house.set_alternate_base_center_for_test(alternate_base_center);
    sim.houses.register(house);
    owner
}

#[test]
fn camera_actions_use_alpha_tokens_and_ctor_zero_not_decimal_indices() {
    for (kind, token, waypoint, immediate) in [(48, "NZ", 389, false), (112, "AA", 26, true)] {
        let mut sim = Simulation::new();
        let waypoints = waypoint_map(waypoint, (9, 10));
        assert_eq!(
            run_waypoint_action(kind, Some(token), "Neutral", &mut sim, &waypoints, None),
            vec![TriggerEffect::CameraAtCoord {
                coord: [2432, 2688, 0],
                velocity: if immediate { None } else { Some(0) }
            }]
        );
    }

    for token in [None, Some("")] {
        let mut sim = Simulation::new();
        assert_eq!(
            run_waypoint_action(112, token, "Neutral", &mut sim, &HashMap::new(), None),
            vec![TriggerEffect::CameraAtCoord {
                coord: [128, 128, 0],
                velocity: None
            }]
        );
    }
    for token in [Some("15"), Some("   ")] {
        let mut sim = Simulation::new();
        assert!(
            run_waypoint_action(112, token, "Neutral", &mut sim, &HashMap::new(), None).is_empty()
        );
    }
}

#[test]
fn action_137_replays_all_three_retail_waypoint_fixtures() {
    for (token, index, cell) in [
        ("P", 15, (93, 106)),
        ("NZ", 389, (122, 135)),
        ("AA", 26, (105, 194)),
    ] {
        let mut sim = Simulation::new();
        let rules = waypoint_action_rules();
        let house = register_waypoint_action_house(&mut sim, "HouseA", Some("Americans"), (0, 0));
        let waypoints = waypoint_map(index, cell);

        assert!(
            run_waypoint_action(
                137,
                Some(token),
                "americans",
                &mut sim,
                &waypoints,
                Some(&rules),
            )
            .is_empty()
        );
        assert_eq!(sim.houses[&house].alternate_base_center(), cell);
        assert_eq!(sim.houses[&house].base_center(), Some((40, 50)));
    }
}

#[test]
fn action_137_uses_first_registration_order_country_match_only() {
    let mut sim = Simulation::new();
    let rules = waypoint_action_rules();
    let first = register_waypoint_action_house(&mut sim, "First", Some("Americans"), (1, 2));
    let second = register_waypoint_action_house(&mut sim, "Second", Some("AMERICANS"), (3, 4));
    let waypoints = waypoint_map(15, (93, 106));

    run_waypoint_action(
        137,
        Some("P"),
        "americans",
        &mut sim,
        &waypoints,
        Some(&rules),
    );

    assert_eq!(sim.houses[&first].alternate_base_center(), (93, 106));
    assert_eq!(sim.houses[&second].alternate_base_center(), (3, 4));
    assert_eq!(sim.houses[&first].base_center(), Some((40, 50)));
    assert_eq!(sim.houses[&second].base_center(), Some((40, 50)));
}

#[test]
fn action_137_canonicalizes_house_type_alias_and_none_owner() {
    let rules = waypoint_action_rules();
    let waypoints = waypoint_map(15, (93, 106));

    let mut alias_sim = Simulation::new();
    let alias_house =
        register_waypoint_action_house(&mut alias_sim, "AliasHouse", Some("Americans"), (0, 0));
    run_waypoint_action(
        137,
        Some("P"),
        "united states",
        &mut alias_sim,
        &waypoints,
        Some(&rules),
    );
    assert_eq!(
        alias_sim.houses[&alias_house].alternate_base_center(),
        (93, 106)
    );

    let mut none_sim = Simulation::new();
    let russian =
        register_waypoint_action_house(&mut none_sim, "RussianHouse", Some("Russians"), (7, 8));
    let first_type =
        register_waypoint_action_house(&mut none_sim, "AmericanHouse", Some("Americans"), (0, 0));
    run_waypoint_action(
        137,
        Some("P"),
        "<none>",
        &mut none_sim,
        &waypoints,
        Some(&rules),
    );
    assert_eq!(none_sim.houses[&russian].alternate_base_center(), (7, 8));
    assert_eq!(
        none_sim.houses[&first_type].alternate_base_center(),
        (93, 106)
    );
}

#[test]
fn action_137_writes_signed_waypoint_halves_as_raw_nonzero_cell() {
    let rules = waypoint_action_rules();
    let mut sim = Simulation::new();
    let house =
        register_waypoint_action_house(&mut sim, "SignedWaypointHouse", Some("Americans"), (9, 9));
    let waypoints = crate::map::waypoints::parse_waypoints(
        &crate::rules::ini_parser::IniFile::from_str("[Waypoints]\n15=-1001\n"),
    );

    run_waypoint_action(
        137,
        Some("P"),
        "Americans",
        &mut sim,
        &waypoints,
        Some(&rules),
    );

    assert_eq!(
        sim.houses[&house].alternate_base_center(),
        (u16::MAX, u16::MAX),
        "signed -1 quotient/remainder halves are nonzero and must be written"
    );
    assert_eq!(sim.houses[&house].base_center(), Some((40, 50)));
}

#[test]
fn action_137_invalid_resolution_and_waypoints_do_not_mutate_either_base_cell() {
    fn assert_no_write(
        trigger_country: &str,
        house_country: Option<&str>,
        token: Option<&str>,
        waypoints: HashMap<u32, Waypoint>,
        register: bool,
        with_rules: bool,
    ) {
        let mut sim = Simulation::new();
        let rules = waypoint_action_rules();
        let house = register
            .then(|| register_waypoint_action_house(&mut sim, "Americans", house_country, (7, 8)));
        run_waypoint_action(
            137,
            token,
            trigger_country,
            &mut sim,
            &waypoints,
            with_rules.then_some(&rules),
        );
        if let Some(house) = house {
            assert_eq!(sim.houses[&house].alternate_base_center(), (7, 8));
            assert_eq!(sim.houses[&house].base_center(), Some((40, 50)));
        }
    }

    let (usa, p) = (Some("Americans"), Some("P"));
    let valid = || waypoint_map(15, (93, 106));
    // (trigger country, House country, token, waypoints, registered, with rules)
    for (trigger, house, token, waypoints, registered, ruled) in [
        // `Neutral` names no country here, so the owner resolves to no House.
        ("Neutral", usa, p, valid(), true, true),
        ("Americans", usa, p, valid(), true, false),
        ("Americans", None, p, valid(), true, true),
        ("Americans", usa, p, valid(), false, true),
        ("Americans", usa, Some("15"), valid(), true, true),
        ("Americans", usa, p, HashMap::new(), true, true),
        ("Americans", usa, p, waypoint_map(15, (0, 0)), true, true),
        // Matching the House name is insufficient: only HouseState.country owns
        // the native Spring/Find_By_Country_Index resolution.
        ("Americans", Some("Russians"), p, valid(), true, true),
    ] {
        assert_no_write(trigger, house, token, waypoints, registered, ruled);
    }
}

#[test]
fn action_138_clears_only_first_country_match_without_token_or_rng_use() {
    let mut sim = Simulation::new();
    let rules = waypoint_action_rules();
    let first = register_waypoint_action_house(&mut sim, "First", Some("Americans"), (93, 106));
    let second = register_waypoint_action_house(&mut sim, "Second", Some("Americans"), (122, 135));
    let rng_before = sim.scenario_rng.state();

    run_waypoint_action(
        138,
        Some("present but ignored"),
        "AMERICANS",
        &mut sim,
        &HashMap::new(),
        Some(&rules),
    );

    assert_eq!(sim.houses[&first].alternate_base_center(), (0, 0));
    assert_eq!(sim.houses[&second].alternate_base_center(), (122, 135));
    assert_eq!(sim.houses[&first].base_center(), Some((40, 50)));
    assert_eq!(sim.houses[&second].base_center(), Some((40, 50)));
    assert_eq!(sim.scenario_rng.state(), rng_before);

    let mut no_match = Simulation::new();
    let house =
        register_waypoint_action_house(&mut no_match, "OnlyHouse", Some("Russians"), (105, 194));
    run_waypoint_action(
        138,
        None,
        "Americans",
        &mut no_match,
        &HashMap::new(),
        Some(&rules),
    );
    assert_eq!(no_match.houses[&house].alternate_base_center(), (105, 194));
}

/// HouseClass::Update's Tag raise against the original block executed in
/// `tools/spatial_oracle/house_tag_raise.json`: every eighth signed frame,
/// event 8 with no object, the zero Cell, no force and no source, last Tag
/// first.
#[test]
fn house_tag_raise_matches_original_cadence_and_order() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/house_tag_raise.json",
    ))
    .unwrap();
    assert_eq!(cases.len(), 19);
    for case in cases {
        let frame = case["input"]["frame"].as_i64().unwrap() as i32;
        let count = case["input"]["tags"].as_i64().unwrap().max(0) as usize;
        let mut sim = Simulation::new();
        let house = sim.interner.intern("Americans");
        sim.trigger_runtime
            .house_tags
            .insert(house, (0..count).collect());
        sim.session.set_binary_frame_for_test(frame as u32);
        sim.raise_house_tags(
            house,
            crate::rules::ruleset::RuleSet::empty_for_test(),
            None,
            FrameEffects::default(),
        );
        let expected: Vec<(usize, i32)> = case["raises"]
            .as_array()
            .unwrap()
            .iter()
            .map(|raise| {
                assert_eq!(
                    (
                        &raise["object"],
                        &raise["cell"],
                        &raise["force"],
                        &raise["source"]
                    ),
                    (
                        &serde_json::json!(0),
                        &serde_json::json!([0, 0]),
                        &serde_json::json!(0),
                        &serde_json::json!(0)
                    ),
                    "{case}"
                );
                (
                    raise["tag"].as_u64().unwrap() as usize,
                    raise["event"].as_i64().unwrap() as i32,
                )
            })
            .collect();
        assert_eq!(sim.trigger_runtime.dispatch_trace, expected, "{case}");
    }
}

const READER_MAP_PREFIX: &str = "[Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
    [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n";

/// Full_Init reads `[Triggers]` (`0x006879C4` -> LoadFromINIList
/// `0x007275D0`) before `[Tags]` (`0x006879CB` -> `0x006E5ED0`). Each
/// physical entry finds or allocates the Type of its name, ignoring case;
/// ReadINI (`0x00727240`, `0x006E6080`) then reads `[Triggers]`, `[Events]`,
/// `[Actions]` and `[Tags]` by that Type's own ID, exact case. A case variant
/// revisits the first Type and reads the first spelling's records again: the
/// header and Tag fields are rewritten, the events are prepended once more and
/// the actions are rebuilt. Tag repeat is `atoi` of the whole ReadString copy
/// (`0x006E60D5`), so a leading comma reads 0.
#[test]
fn physical_readers_take_each_record_by_the_type_id() {
    let map = crate::map::map_file::MapFile::from_bytes(
        format!(
            "{READER_MAP_PREFIX}\
             [Triggers]\nt1=House0,<none>,First,0,1,1,1,0\nT1=House1,<none>,Second,1,0,0,0,1\n\
             [Events]\nt1=1,13,0,7\nT1=1,13,0,9\n\
             [Actions]\nt1=1,28,0,3,0,0,0,0,A\nT1=1,28,0,4,0,0,0,0,A\n\
             [Tags]\ng1=,2,Lead,t1\nG1=1,Other,T1\n"
        )
        .as_bytes(),
    )
    .unwrap();
    let definitions = RuntimeDefinitions::read_map(&map, &Default::default(), None, None);
    let [trigger] = definitions.trigger_types.as_slice() else {
        panic!("one TriggerType: {:?}", definitions.trigger_types);
    };
    assert_eq!(trigger.id, "t1");
    assert_eq!(trigger.owner.as_deref(), Some("House0"));
    assert!(trigger.enabled);
    assert_eq!(trigger.difficulty, [true; 3]);
    assert!(!trigger.transfer_on_capture);
    assert_eq!(
        trigger
            .events
            .iter()
            .map(|event| event.value)
            .collect::<Vec<_>>(),
        [7, 7]
    );
    assert_eq!(
        trigger
            .actions
            .iter()
            .map(|action| action.value)
            .collect::<Vec<_>>(),
        [Some(3)]
    );
    let [tag] = definitions.tag_types.as_slice() else {
        panic!("one TagType: {:?}", definitions.tag_types);
    };
    assert_eq!(
        (tag.id.as_str(), tag.repeat, tag.name.as_str(), tag.trigger),
        ("g1", 0, "Lead", Some(0))
    );
}

/// Read_Map_Section `0x004AD1B3..0x004AD332` walks `[CellTags]` in physical
/// order. Each entry allocates its TagType (`0x004AD20C`) before admission;
/// the cell is `atoi` of the name, split by 128 under NewINIFormat below 4
/// and by 1000 from 4 on, and a cell whose Tag is already attached keeps it.
#[test]
fn cell_tags_split_by_ini_format_and_keep_the_first_tag_per_cell() {
    for (format, stride) in [(3, 128), (4, 1000)] {
        let cell = 6 * stride + 5;
        let map = crate::map::map_file::MapFile::from_bytes(
            format!(
                "{READER_MAP_PREFIX}[Basic]\nNewINIFormat={format}\n\
                 [Triggers]\nTR=House0,<none>,Tr,0,1,1,1,0\n\
                 [Tags]\nA=0,A,TR\n\
                 [CellTags]\n{cell}=A\n0{cell}=B\n"
            )
            .as_bytes(),
        )
        .unwrap();
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(
            16,
            16,
            crate::map::resolved_terrain::test_clear_cell,
        ));
        sim.initialize_trigger_definitions(&map, &Default::default(), None, None);
        sim.install_authored_cell_trigger_tags(&map);
        let identity = Simulation::trigger_cell_identity(&sim.resolved_terrain, (5, 6));
        let tag = sim.trigger_runtime.cell_tags[&identity];
        let type_index = sim.trigger_runtime.tag(tag).unwrap().type_index;
        let definitions = &sim.trigger_runtime.definitions;
        assert_eq!(definitions.tag_types[type_index].id, "A", "format {format}");
        assert_eq!(sim.trigger_runtime.cell_tags.len(), 1, "format {format}");
        assert!(
            definitions.tag_types.iter().any(|typ| typ.id == "B"),
            "format {format}: the losing entry still allocates its TagType"
        );
    }
}
