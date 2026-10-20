//! Native tactical action/display regressions, using stock rules and real weapon selection.
use super::*;
use crate::app::input::context_order::OrderModifier;
use crate::app::input::entity_pick::HoverTargetKindWithId;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::new();
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let owner = sim.intern(name);
        sim.houses_mut_for_test()
            .register(HouseState::new(owner, side, None, true, 0, 10));
    }
    let house = sim.interner().get("Americans");
    sim.session_mut_for_test().set_current_house_for_test(house);
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim
}

#[test]
fn retail_weapon_cursor_overrides_survive_distance() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    assert!(retail.rules.weapon("Sapper").unwrap().sabotage_cursor);
    assert!(retail.rules.weapon("Flare").unwrap().mig_attack_cursor);
    assert!(!retail.rules.weapon("105mm").unwrap().sabotage_cursor);
    assert!(!retail.rules.weapon("105mm").unwrap().mig_attack_cursor);
    for (actor_type, target_type, kind, expected) in [
        (
            "TANY",
            "MTNK",
            HoverTargetKind::EnemyUnit,
            CursorId::Demolish,
        ),
        (
            "BORIS",
            "NAPOWR",
            HoverTargetKind::EnemyStructure,
            CursorId::AirStrike,
        ),
        (
            "TANY",
            "NAPOWR",
            HoverTargetKind::EnemyStructure,
            CursorId::Demolish,
        ),
    ] {
        for x in [17, 27] {
            let mut sim = arena(&retail.rules);
            let actor = sim
                .spawn_object(actor_type, "Americans", 16, 16, 64, &retail.rules)
                .unwrap();
            let target = sim
                .spawn_object(target_type, "Russians", x, 16, 64, &retail.rules)
                .unwrap();
            let before = sim.state_hash();
            let feedback = capability_cursor_for_hover(
                &sim,
                &[actor],
                Some(actor),
                &HoverTargetKindWithId {
                    kind,
                    stable_id: target,
                },
                Some(&retail.rules),
                None,
                crate::app::input::context_order::OrderModifier::Normal,
                false,
            );
            assert_eq!(
                cursor_id_for_feedback(feedback),
                Some(expected),
                "{actor_type} -> {target_type} x={x}"
            );
            assert_eq!(
                sim.state_hash(),
                before,
                "hover must not mutate gameplay/RNG"
            );
        }
    }
}

#[test]
fn retail_cell_chords_have_their_native_cursors() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut sim = arena(&retail.rules);
    let actor = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, &retail.rules)
        .unwrap();
    for (modifier, expected) in [
        (OrderModifier::AttackMove, CursorId::AttackMove),
        (OrderModifier::GuardArea, CursorId::GuardArea),
    ] {
        let feedback = empty_cell_cursor(
            &sim,
            Some(&retail.rules),
            None,
            sim.session().current_house(),
            Some(actor),
            (30, 30),
            modifier,
            crate::app::types::OrderMode::Move,
        );
        assert_eq!(
            cursor_id_for_feedback(feedback),
            Some(expected),
            "{modifier:?}"
        );
    }
}

#[test]
fn native_modifier_controls_match_production_cursor_queries() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/mouse_oracle/tactical_actions.json",
    ))
    .unwrap();
    for row in corpus["modifiers"].as_array().unwrap() {
        let case = &row["input"];
        let mut sim = arena(&retail.rules);
        let actor = sim
            .spawn_object(
                if case["unarmed"] == true {
                    "AMCV"
                } else {
                    "MTNK"
                },
                "Americans",
                16,
                16,
                64,
                &retail.rules,
            )
            .unwrap();
        let mut selected = vec![actor];
        if case["mixed_unarmed_selection"] == true {
            selected.push(
                sim.spawn_object("AMCV", "Americans", 15, 16, 64, &retail.rules)
                    .unwrap(),
            );
        }
        let alt = case["alt"].as_bool().unwrap();
        let mut modifier = super::super::context_order::resolve_order_modifiers(
            case["ctrl"].as_bool().unwrap(),
            case["shift"].as_bool().unwrap(),
            alt,
        );
        if modifier == OrderModifier::AttackMove
            && !super::super::context_order::selection_can_attack_move(
                &sim,
                Some(&retail.rules),
                &selected,
            )
        {
            modifier = if alt {
                OrderModifier::ForceMove
            } else {
                OrderModifier::Normal
            };
        }
        let feedback = if case["route"] == "object" {
            let own = case["relation"] == "own";
            let target = if case["self_target"] == true {
                actor
            } else {
                sim.spawn_object(
                    "MTNK",
                    if own { "Americans" } else { "Russians" },
                    17,
                    16,
                    64,
                    &retail.rules,
                )
                .unwrap()
            };
            let feedback = capability_cursor_for_hover(
                &sim,
                &selected,
                Some(actor),
                &HoverTargetKindWithId {
                    stable_id: target,
                    kind: if own {
                        HoverTargetKind::FriendlyUnit
                    } else {
                        HoverTargetKind::EnemyUnit
                    },
                },
                Some(&retail.rules),
                None,
                modifier,
                alt,
            );
            promote_attack_move_feedback(feedback, modifier == OrderModifier::AttackMove)
        } else {
            let forced = (modifier == OrderModifier::ForceFire)
                .then(|| {
                    force_fire_feedback(
                        &sim,
                        actor,
                        &combat::TargetKind::Cell(17, 16),
                        selected.len(),
                        Some(&retail.rules),
                        None,
                    )
                })
                .flatten();
            forced
                .map(|kind| kind.unwrap_or(CursorFeedbackKind::Default))
                .unwrap_or_else(|| {
                    empty_cell_cursor(
                        &sim,
                        Some(&retail.rules),
                        None,
                        sim.session().current_house(),
                        Some(actor),
                        (17, 16),
                        modifier,
                        crate::app::types::OrderMode::Move,
                    )
                })
        };
        let expected = match row["cursor_row"].as_u64().unwrap() {
            17 => CursorId::Select,
            18 => CursorId::Move,
            19 => CursorId::NoMove,
            20 => CursorId::Attack,
            21 => CursorId::AttackOutOfRange,
            22 => CursorId::GuardArea,
            71 => CursorId::AttackMove,
            n => panic!("unexpected row {n}"),
        };
        assert_eq!(cursor_id_for_feedback(feedback), Some(expected), "{case}");
    }
}

#[test]
fn infantry_deploy_and_loaded_transport_self_cursors_share_click_admission() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    for (name, loaded, expected) in [
        ("E1", false, CursorId::Deploy),
        ("GGI", false, CursorId::Deploy),
        ("DESO", false, CursorId::Deploy),
        ("FV", true, CursorId::Deploy),
        ("FV", false, CursorId::NoDeploy),
    ] {
        let mut sim = arena(&retail.rules);
        let actor = sim
            .spawn_object(name, "Americans", 16, 16, 64, &retail.rules)
            .unwrap();
        if loaded {
            let passenger = sim
                .spawn_object("E1", "Americans", 15, 16, 64, &retail.rules)
                .unwrap();
            let mut cargo = crate::sim::passenger::PassengerCargo::new(1, 0.0);
            cargo.board_forced_chain(&[(passenger, 1.0)]);
            sim.entities_mut_for_test()
                .get_mut(actor)
                .unwrap()
                .passenger_role = crate::sim::passenger::PassengerRole::transport(cargo);
        }
        for modifier in [
            OrderModifier::Normal,
            OrderModifier::ForceFire,
            OrderModifier::ForceMove,
            OrderModifier::GuardArea,
            OrderModifier::AttackMove,
        ] {
            let feedback = capability_cursor_for_hover(
                &sim,
                &[actor],
                Some(actor),
                &HoverTargetKindWithId {
                    stable_id: actor,
                    kind: HoverTargetKind::FriendlyUnit,
                },
                Some(&retail.rules),
                None,
                modifier,
                true,
            );
            assert_eq!(
                cursor_id_for_feedback(feedback),
                Some(expected),
                "{name} {modifier:?}"
            );
        }
    }
}

#[test]
fn native_infantry_weapon_suffix_controls_match_shared_action_owner() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/mouse_oracle/tactical_actions.json",
    ))
    .unwrap();
    for row in corpus["weapons"].as_array().unwrap() {
        let spec = &row["input"];
        let flag = |name: &str| if spec[name] == true { "yes" } else { "no" };
        let ini = format!(
            "[InfantryTypes]\n0=ACTOR\n[VehicleTypes]\n0=UNIT\n[BuildingTypes]\n0=BLD\n\
            [ACTOR]\nStrength=100\nPrimary=Gun\nC4={}\n[UNIT]\nStrength=100\n\
            [BLD]\nStrength=100\nCanC4={}\nInvisibleInGame={}\n\
            [Gun]\nDamage=60\nRange=5\nProjectile=Invisible\nWarhead=AP\nSabotageCursor={}\nMigAttackCursor={}\n\
            [Invisible]\nInviso=yes\n[AP]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n",
            flag("c4"),
            if spec["can_c4"] == false { "no" } else { "yes" },
            flag("invisible"),
            flag("sabotage"),
            flag("mig")
        );
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(&ini)).unwrap();
        let mut sim = arena(&rules);
        let actor = sim
            .spawn_object_limbo_at_height("ACTOR", "Americans", 16, 16, 0, 0, &rules)
            .unwrap();
        let building = spec["building"] == true;
        let target = sim
            .spawn_object_limbo_at_height(
                if building { "BLD" } else { "UNIT" },
                "Russians",
                17,
                16,
                0,
                0,
                &rules,
            )
            .unwrap();
        if spec["bunkered"] == true {
            sim.entities_mut_for_test()
                .get_mut(target)
                .unwrap()
                .bunker
                .set_bunker_link_for_test(crate::sim::docking::bunker_link::BunkerLink::Installed(
                    99,
                ));
        }
        let base = if spec["base_action"] == 7 {
            CursorFeedbackKind::Select
        } else {
            CursorFeedbackKind::EnemyUnit
        };
        let feedback = infantry_weapon_feedback(
            &sim,
            Some(&rules),
            actor,
            &HoverTargetKindWithId {
                stable_id: target,
                kind: if building {
                    HoverTargetKind::EnemyStructure
                } else {
                    HoverTargetKind::EnemyUnit
                },
            },
            base,
        )
        .unwrap_or(base);
        let expected = match row["output"]["cursor_row"].as_u64().unwrap() {
            17 => CursorId::Select,
            20 => CursorId::Attack,
            52 => CursorId::Demolish,
            82 => CursorId::AirStrike,
            n => panic!("unexpected row {n}"),
        };
        assert_eq!(cursor_id_for_feedback(feedback), Some(expected), "{spec}");
    }
}

#[test]
fn deployed_infantry_self_hover_refuses_a_second_deploy_click() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut sim = arena(&retail.rules);
    let actor = sim
        .spawn_object("E1", "Americans", 16, 16, 64, &retail.rules)
        .unwrap();
    assert!(sim.apply_command(
        "Americans",
        &crate::sim::command::Command::ToggleInfantryDeploy { entity_id: actor },
        Some(&retail.rules)
    ));
    for _ in 0..32 {
        sim.advance_tick(&[], Some(&retail.rules), None, None, 67);
        if sim.entities().get(actor).unwrap().infantry_deploy_doing() {
            break;
        }
    }
    assert!(sim.entities().get(actor).unwrap().infantry_deploy_doing());
    let hover = HoverTargetKindWithId {
        stable_id: actor,
        kind: HoverTargetKind::FriendlyUnit,
    };
    assert_eq!(
        cursor_id_for_feedback(capability_cursor_for_hover(
            &sim,
            &[actor],
            Some(actor),
            &hover,
            Some(&retail.rules),
            None,
            OrderModifier::Normal,
            false
        )),
        Some(CursorId::NoDeploy)
    );
    assert_eq!(
        super::super::context_order::self_click_command(
            &sim,
            &retail.rules,
            sim.session().current_house().unwrap(),
            &[actor],
            Some(&hover)
        ),
        None
    );
}
