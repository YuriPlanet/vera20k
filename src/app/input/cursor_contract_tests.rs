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
    // The native corpus rows hover mapped ground; Foot's shroud wrapper
    // (`0x004DDE64..0x004DDEBE`) would rewrite their answers otherwise.
    let rows = sim.session().map_height();
    reveal_rows(&mut sim, house.unwrap(), rows);
    sim
}

/// The player's map shows rows `0..rows` and shrouds the rest.
fn reveal_rows(sim: &mut Simulation, owner: crate::sim::intern::InternedId, rows: u16) {
    let (width, height) = (sim.session().map_width(), sim.session().map_height());
    let fog = sim.fog_mut_for_test();
    *fog = crate::sim::vision::FogState::new(width, height);
    fog.reveal_cells_for_owner(
        owner,
        (0..width).flat_map(|x| (0..rows).map(move |y| (x, y))),
    );
}

#[test]
fn ares_no_manual_fire_native_modifier_cursors_use_the_shared_action() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/no_manual_fire.json");
    let rows = corpus["rows"].as_array().unwrap();
    for setting in [false, true] {
        let patch = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[MTNK]\nNoManualFire={}\n",
            if setting { "yes" } else { "no" },
        ));
        let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "XMP03T4.MAP",
            &patch,
        ) else {
            return;
        };
        for row in rows
            .iter()
            .filter(|row| row["no_manual_fire_native_reader_value"] == u64::from(setting))
        {
            let case = &row["input"];
            // The same authored key is ignored by the default build. Its
            // expected result is the actual DLL control with the flag off.
            let expected = if crate::rules::ARES_ENABLED {
                row
            } else {
                rows.iter()
                    .find(|other| {
                        other["input"] == *case && other["no_manual_fire_native_reader_value"] == 0
                    })
                    .unwrap()
            };
            let mut sim = arena(&retail.rules);
            let actor = sim
                .spawn_object("MTNK", "Americans", 16, 16, 64, &retail.rules)
                .unwrap();
            let target = sim
                .spawn_object("MTNK", "Russians", 17, 16, 64, &retail.rules)
                .unwrap();
            let alt = case["alt"].as_bool().unwrap();
            let modifier = crate::app::input::context_order::resolve_order_modifiers(
                case["ctrl"].as_bool().unwrap(),
                case["shift"].as_bool().unwrap(),
                alt,
            );
            let before = sim.state_hash();
            let feedback = if case["route"] == "object" {
                promote_attack_move_feedback(
                    capability_cursor_for_hover(
                        &sim,
                        &[actor],
                        Some(actor),
                        &HoverTargetKindWithId {
                            stable_id: target,
                            kind: HoverTargetKind::EnemyUnit,
                        },
                        Some(&retail.rules),
                        None,
                        modifier,
                        alt,
                        false,
                    ),
                    modifier == OrderModifier::AttackMove,
                )
            } else {
                let forced = (modifier == OrderModifier::ForceFire)
                    .then(|| {
                        force_fire_feedback(
                            &sim,
                            actor,
                            &combat::TargetKind::Cell(17, 16),
                            1,
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
            let native_cursor = match expected["cursor_row"].as_u64().unwrap() {
                0 => CursorId::Default,
                17 => CursorId::Select,
                18 => CursorId::Move,
                19 => CursorId::NoMove,
                20 => CursorId::Attack,
                21 => CursorId::AttackOutOfRange,
                22 => CursorId::GuardArea,
                71 => CursorId::AttackMove,
                other => panic!("unexpected native cursor {other}"),
            };
            assert_eq!(
                cursor_id_for_feedback(feedback),
                Some(native_cursor),
                "{setting} {case}"
            );
            assert_eq!(sim.state_hash(), before, "hover cannot mutate gameplay/RNG");
        }
    }
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

/// Techno's Ctrl+Alt rung (`0x00700815..0x007008A3`) answers GuardArea on
/// mapped ground. Over shroud a Foot's wrapper (`0x004DDE64..0x004DDEBE`)
/// makes it Move for a MoveToShroud type, and the click asks the same rung.
/// A berserk tank never reaches it.
#[test]
fn retail_ctrl_alt_over_shroud_is_a_move_for_a_foot() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = arena(rules);
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, rules)
        .unwrap();
    let owner = sim.interner().get("Americans").unwrap();
    reveal_rows(&mut sim, owner, 20);
    let cursor = |sim: &Simulation, cell| {
        cursor_id_for_feedback(empty_cell_cursor(
            sim,
            Some(rules),
            None,
            sim.session().current_house(),
            Some(tank),
            cell,
            OrderModifier::GuardArea,
            crate::app::types::OrderMode::Move,
        ))
    };
    let guards = |sim: &Simulation, cell| {
        crate::app::input::context_order::cell_guard_area_action(
            sim,
            rules,
            tank,
            Some(owner),
            cell,
        )
    };
    assert_eq!(cursor(&sim, (10, 10)), Some(CursorId::GuardArea));
    assert_eq!(cursor(&sim, (10, 30)), Some(CursorId::Move));
    assert!(guards(&sim, (10, 10)));
    assert!(!guards(&sim, (10, 30)));
    // A berserk tank answers 0 before the rung (`0x00700609`): the arrow.
    sim.entities_mut_for_test()
        .get_mut(tank)
        .unwrap()
        .berserk
        .set_active_for_test(true);
    assert!(!guards(&sim, (10, 10)));
    assert_eq!(cursor(&sim, (10, 10)), Some(CursorId::Default));
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
                false,
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
                false,
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
        let feedback =
            infantry_weapon_feedback(&sim, Some(&rules), actor, target, base).unwrap_or(base);
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

/// Native Infantry WhatAction reads Type+6C4 (UndeployDelay), not
/// Type+6A8 (DeployFireWeapon), at 51EC39/51EC79. A self action remains
/// available throughout Doing27..31 when the delay is -1. The native
/// 51EB81..51ED10 controls also cover the adjacent Doing26/32 boundaries.
#[test]
fn retail_infantry_self_click_admission_uses_undeploy_delay() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    for (name, delay) in [
        ("E1", -1),
        ("GGI", -1),
        ("DESO", -1),
        ("YURI", 150),
        ("YURIPR", 75),
    ] {
        let rules = &retail.rules;
        let object = rules.object(name).unwrap();
        assert!(object.deployer && object.deploy_fire, "{name}");
        assert_eq!(object.undeploy_delay, delay, "{name}");
        assert_eq!(
            object.deploy_fire_weapon, 1,
            "the unrelated selector: {name}"
        );
        let mut sim = arena(rules);
        let actor = sim
            .spawn_object(name, "Americans", 16, 16, 64, rules)
            .unwrap();
        let owner = sim.session().current_house().unwrap();
        let hover = HoverTargetKindWithId {
            stable_id: actor,
            kind: HoverTargetKind::FriendlyUnit,
        };
        for (doing, delayed_refusal) in [
            (0, false),
            (26, false),
            (27, true),
            (28, true),
            (29, true),
            (30, true),
            (31, true),
            (32, false),
        ] {
            sim.entities_mut_for_test()
                .get_mut(actor)
                .unwrap()
                .mission_leaf
                .set_infantry_doing_verified(doing)
                .unwrap();
            let refused = delay >= 0 && delayed_refusal;
            let expected = if refused {
                CursorId::NoDeploy
            } else {
                CursorId::Deploy
            };
            for modifier in [
                OrderModifier::Normal,
                OrderModifier::ForceFire,
                OrderModifier::ForceMove,
                OrderModifier::GuardArea,
                OrderModifier::AttackMove,
            ] {
                assert_eq!(
                    cursor_id_for_feedback(capability_cursor_for_hover(
                        &sim,
                        &[actor],
                        Some(actor),
                        &hover,
                        Some(rules),
                        None,
                        modifier,
                        false,
                        false,
                    )),
                    Some(expected),
                    "{name} Doing{doing} {modifier:?}"
                );
            }
            assert_eq!(
                super::super::context_order::self_click_command(
                    &sim,
                    rules,
                    None,
                    owner,
                    &[actor],
                    Some(&hover),
                    false,
                ),
                (!refused).then_some(crate::sim::command::Command::ToggleInfantryDeploy {
                    entity_id: actor,
                }),
                "{name} Doing{doing}"
            );
        }
    }
}

/// The second click on a lone selected infantryman takes the shared self
/// action. Drive the actual DEPLOY receiver and animation completion both
/// ways, rather than treating a cursor change as a completed undeployment.
#[test]
fn retail_gi_guardian_and_desolator_self_clicks_complete_both_deployment_directions() {
    fn advance_to(sim: &mut Simulation, rules: &RuleSet, actor: u64, doing: i32) {
        for _ in 0..256 {
            if sim
                .entities()
                .get(actor)
                .unwrap()
                .infantry_sprite_pose()
                .is_some_and(|pose| pose.0 == doing)
            {
                return;
            }
            sim.advance_tick(&[], Some(rules), None, None, 67);
        }
        panic!(
            "deployment did not reach Doing{doing}: {:?}",
            sim.entities().get(actor).unwrap().infantry_sprite_pose()
        );
    }
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    for name in ["E1", "GGI", "DESO"] {
        let mut sim = arena(rules);
        let actor = sim
            .spawn_object(name, "Americans", 16, 16, 64, rules)
            .unwrap();
        let owner = sim.session().current_house().unwrap();
        let hover = HoverTargetKindWithId {
            stable_id: actor,
            kind: HoverTargetKind::FriendlyUnit,
        };
        for destination_doing in [28, 0] {
            assert_eq!(
                cursor_id_for_feedback(capability_cursor_for_hover(
                    &sim,
                    &[actor],
                    Some(actor),
                    &hover,
                    Some(rules),
                    None,
                    OrderModifier::Normal,
                    false,
                    false,
                )),
                Some(CursorId::Deploy),
                "{name}: click toward Doing{destination_doing}"
            );
            let command = super::super::context_order::self_click_command(
                &sim,
                rules,
                None,
                owner,
                &[actor],
                Some(&hover),
                false,
            )
            .unwrap_or_else(|| {
                panic!("{name}: self-click toward Doing{destination_doing} refused")
            });
            assert!(
                sim.apply_command("Americans", &command, Some(rules)),
                "{name}"
            );
            advance_to(&mut sim, rules, actor, destination_doing);
        }
        assert!(
            !sim.entities().get(actor).unwrap().infantry_deploy_doing(),
            "{name}"
        );
    }
}
