//! Ordinary allied-House MTNK object actions through the shared production
//! query, real hover picker and existing clicked-Move owner. Native:
//! DetermineAction692610 -> Unit73FD50 -> Foot4DDED0 -> Techno6FFEC0;
//! ally rejection700391..70040E, identity tail70058D..70059B and
//! Unit7403EB..740419. Display maps NoMove2 to row19 and dispatches it.
//! The supplied flat arena is not a native destination or movement golden.

use super::*;
use crate::app::input::entity_pick::HoverTargetKindWithId;
use crate::app::types::CursorId;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

fn object_pair(rules: &RuleSet, actor_kind: &str, relation: &str) -> (Simulation, u64, u64) {
    let mut sim = Simulation::with_seed(0xC9_0001);
    crate::sim::arena_fixture::flat_ground(&mut sim, rules);
    *sim.fog_mut_for_test() = crate::sim::vision::FogState::new(32, 32);
    for (index, (name, human)) in [("Americans", true), ("Russians", false)]
        .into_iter()
        .enumerate()
    {
        let owner = sim.intern(name);
        sim.houses_mut_for_test().register(HouseState::new(
            owner,
            index as u8,
            None,
            human,
            10_000,
            10,
        ));
    }
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    if relation == "ally" || relation == "reverse-ally" {
        let (asker, other) = if relation == "ally" {
            ("AMERICANS", "RUSSIANS")
        } else {
            ("RUSSIANS", "AMERICANS")
        };
        // An admitted directional alliance is an input to this component
        // scene; production House diplomacy owns installing/mutating it.
        sim.house_alliances_mut_for_test()
            .entry(asker.into())
            .or_default()
            .insert(other.into());
    }
    let actor = sim
        .spawn_object(actor_kind, "Americans", 10, 10, 0, rules)
        .unwrap();
    let target = sim
        .spawn_object(
            "MTNK",
            if relation == "own" {
                "Americans"
            } else {
                "Russians"
            },
            14,
            10,
            0,
            rules,
        )
        .unwrap();
    sim.fog_mut_for_test().reveal_all_for_owner(owner);
    sim.entities_mut_for_test()
        .get_mut(actor)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    (sim, actor, target)
}

fn picked_target(sim: &Simulation, target: u64) -> HoverTargetKindWithId {
    let (x, y) = crate::app::presentation::instances::interpolated_screen_position_entity(
        sim.entities().get(target).unwrap(),
    );
    let hover =
        crate::app::input::entity_pick::hover_target_at_point(sim, x, y, "Americans", false)
            .expect("revealed ordinary tank is picked");
    assert_eq!(hover.stable_id, target);
    hover
}

#[test]
fn ares_no_manual_fire_object_click_keeps_native_movement_fallback() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/no_manual_fire.json");
    let rows = corpus["rows"].as_array().unwrap();
    for setting in [false, true] {
        let patch = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[MTNK]\nNoManualFire={}\n",
            if setting { "yes" } else { "no" },
        ));
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "XMP03T4.MAP",
            &patch,
        ) else {
            return;
        };
        let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
            &battle.processed_rules,
            Some(&battle.fixed_art),
        );
        for row in rows.iter().filter(|row| {
            let case = &row["input"];
            case["route"] == "object"
                && case["shift"] == false
                && case["alt"] == false
                && row["no_manual_fire_native_reader_value"] == u64::from(setting)
        }) {
            let expected = if crate::rules::ARES_ENABLED {
                row
            } else {
                rows.iter()
                    .find(|other| {
                        other["input"] == row["input"]
                            && other["no_manual_fire_native_reader_value"] == 0
                    })
                    .unwrap()
            };
            let (sim, actor, target) = object_pair(&battle.rules, "MTNK", "enemy");
            let owner = sim.entities().get(actor).unwrap().owner();
            let ctrl = row["input"]["ctrl"].as_bool().unwrap();
            assert_eq!(
                object_click_release(
                    &sim,
                    &battle.rules,
                    &[actor],
                    &[actor],
                    target,
                    &overlays,
                    ctrl,
                ),
                LeftRelease::Dispatch
            );
            let command = object_attack_click_command(
                &sim,
                &battle.rules,
                &overlays,
                owner,
                OrderMode::Move,
                resolve_order_modifiers(ctrl, false, false),
                false,
                actor,
                target,
            );
            match expected["action"].as_u64().unwrap() {
                5 => assert!(
                    matches!(command, Some(Command::Attack { attacker_id, target_id }) if attacker_id == actor && target_id == target)
                ),
                // Native UnitClicked reaches Foot's navigation boundary.
                // The Rust flat arena tests integration, not exact native
                // destination coordinates or later movement scheduling.
                2 => assert!(
                    matches!(command, Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. }) if entity_id == actor)
                ),
                other => panic!("unexpected native object action {other}"),
            }
        }
        let (sim, actor, _) = object_pair(&battle.rules, "MTNK", "enemy");
        let effective = setting && crate::rules::ARES_ENABLED;
        let native_cell = rows
            .iter()
            .find(|row| {
                let case = &row["input"];
                case["route"] == "cell"
                    && case["ctrl"] == true
                    && case["shift"] == false
                    && case["alt"] == false
                    && row["no_manual_fire_native_reader_value"] == u64::from(effective)
            })
            .unwrap();
        let dispatch = attack_branch_orders(
            &sim,
            &battle.rules,
            &[actor],
            TargetKind::Cell(12, 10),
            true,
            &overlays,
        );
        if native_cell["action"] == 5 {
            assert!(
                matches!(dispatch.commands.as_slice(), [Command::ForceAttackCell { attacker_id, .. }] if *attacker_id == actor)
            );
        } else {
            assert!(dispatch.commands.is_empty());
            assert!(
                !dispatch.receivers.contains(&actor),
                "rejected attack keeps the ordinary Cell-move receiver"
            );
            let owner = sim.entities().get(actor).unwrap().owner();
            assert_eq!(
                crate::app::input::commands::ordinary_cell_move_goal(
                    &sim,
                    &battle.rules,
                    Some(&overlays),
                    owner,
                    actor,
                    (12, 10),
                    false,
                ),
                Some((12, 10)),
                "the production clicked-Cell resolver supplies the ordinary Move destination"
            );
        }
    }
}

fn ivan_grant_scene() -> Option<(
    crate::rules::retail_ini_fixture::RetailBattleRules,
    Simulation,
    u64,
    u64,
)> {
    let patch = crate::rules::ini_parser::IniFile::from_str(
        "[IVAN]\nAttackFriendlies=yes\nAttackCursorOnFriendlies=no\n",
    );
    let battle = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
        "XMP03T4.MAP",
        &patch,
    )?;
    let (sim, actor, target) = object_pair(&battle.rules, "IVAN", "ally");
    assert!(battle.rules.object("IVAN").unwrap().attacks_friendlies());
    assert!(
        !battle
            .rules
            .object("IVAN")
            .unwrap()
            .attack_cursor_on_friendlies
    );
    assert_eq!(battle.rules.object("MTNK").unwrap().passengers, 0);
    assert_eq!(
        attack_action(
            &sim,
            &battle.rules,
            actor,
            &TargetKind::Entity(target),
            false,
            None
        ),
        AttackAction::Attack,
        "canonical Techno grant must already admit the action"
    );
    Some((battle, sim, actor, target))
}

fn native_ivan_bomb_feedback(name: &str) -> crate::app::types::CursorFeedbackKind {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/allied_object_action.json");
    let row = corpus["infantry_bomb_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == name)
        .unwrap();
    assert_eq!(row["output"]["boundary"], "returned");
    match row["output"]["action"].as_u64().unwrap() {
        53 => crate::app::types::CursorFeedbackKind::IvanBomb,
        54 => crate::app::types::CursorFeedbackKind::Invalid,
        other => panic!("unexpected native Ivan consumer action {other}"),
    }
}

/// Infantry51EB24..51EB7E consumes the admitted base action; it does not
/// require AttackCursorOnFriendlies again. This ordinary non-transport target
/// avoids asserting anything about the separate boarding/transport wrapper.
#[test]
fn allied_object_ivan_cursor_consumes_the_canonical_grant() {
    let Some((battle, sim, actor, target)) = ivan_grant_scene() else {
        return;
    };
    let hover = picked_target(&sim, target);
    assert_eq!(hover.kind, HoverTargetKind::FriendlyUnit);
    assert_eq!(
        crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[actor],
            Some(actor),
            &hover,
            Some(&battle.rules),
            None,
            crate::app::input::context_order::OrderModifier::Normal,
            false,
            false,
        ),
        native_ivan_bomb_feedback("attack_friendlies_only_ivan"),
    );
}

#[test]
fn allied_object_ivan_click_consumes_the_canonical_grant() {
    let Some((battle, sim, actor, target)) = ivan_grant_scene() else {
        return;
    };
    assert_eq!(
        friendly_bomb_ivans(&sim, &battle.rules, &[actor], 1, target, None).contains(&actor),
        native_ivan_bomb_feedback("attack_friendlies_only_ivan")
            == crate::app::types::CursorFeedbackKind::IvanBomb,
    );
}

#[test]
fn allied_object_ivan_bomb_refusal_matches_native_consumer() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/allied_object_action.json");
    let mut compared = 0;
    for row in corpus["infantry_bomb_controls"].as_array().unwrap() {
        if row["output"]["boundary"] != "returned" {
            continue;
        }
        let input = &row["input"];
        let patch = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[IVAN]\nAttackFriendlies=yes\nAttackCursorOnFriendlies=no\n[MTNK]\nBombable={}\n",
            if input["bombable"] == true {
                "yes"
            } else {
                "no"
            },
        ));
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "XMP03T4.MAP",
            &patch,
        ) else {
            return;
        };
        let (mut sim, actor, target) = object_pair(&battle.rules, "IVAN", "ally");
        if input["has_bomb"] == true {
            sim.bomb_attach_for_test(actor, Some(target), &battle.rules);
        }
        let entity = sim.entities().get(target).unwrap();
        // Compare this shared refusal leaf only: a previously attached bomb
        // can make GetFireError leave Attack before the native53/54 consumer.
        assert_eq!(
            ivan_bomb_refused(
                entity,
                sim.interner().object_type(entity.type_ref(), &battle.rules)
            ),
            row["output"]["action"] == 54,
            "{}",
            row["name"],
        );
        compared += 1;
    }
    assert_eq!(compared, 3);
}

#[test]
fn allied_object_action_uses_the_asking_house_and_real_owner_identity() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &battle.processed_rules,
        Some(&battle.fixed_art),
    );
    let kind = battle.rules.object("MTNK").unwrap();
    assert!(!kind.attacks_friendlies());
    assert!(!kind.attack_cursor_on_friendlies);
    let corpus = crate::test_fixture::json("tools/spatial_oracle/allied_object_action.json");
    let names = [
        "own_unselected",
        "other_ally",
        "actor_only_ally",
        "target_only_ally",
        "enemy",
        "ctrl_ally",
        "ctrl_own",
        "ctrl_enemy",
    ];
    let mut compared = 0;
    for row in corpus["action_controls"].as_array().unwrap() {
        let input = &row["input"];
        if !names.contains(&input["name"].as_str().unwrap()) {
            continue;
        }
        let relation = if input["relation"] == "own" {
            "own"
        } else if input["actor_alliance_mask"].as_u64().unwrap() & 2 != 0 {
            "ally"
        } else if input["target_alliance_mask"].as_u64().unwrap() & 1 != 0 {
            "reverse-ally"
        } else {
            "enemy"
        };
        let expected = match row["action"].as_u64().unwrap() {
            2 => AttackAction::NoMove,
            5 => AttackAction::Attack,
            7 => AttackAction::Select,
            other => panic!("unexpected original action {other}"),
        };
        let (sim, actor, target) = object_pair(&battle.rules, "MTNK", relation);
        let before = sim.state_hash();
        assert_eq!(
            attack_action(
                &sim,
                &battle.rules,
                actor,
                &TargetKind::Entity(target),
                input["ctrl"].as_bool().unwrap(),
                Some(&overlays),
            ),
            expected,
            "{relation}"
        );
        assert_eq!(sim.state_hash(), before, "query must not mutate the world");
        compared += 1;
    }
    assert_eq!(compared, names.len());
}

#[test]
fn allied_object_shared_grants_match_original_without_forking_weapon_selection() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/allied_object_action.json");
    let ordinary_grants = [
        "attack_friendlies",
        "attack_cursor_friendlies",
        "ivan_empty_keeps_cursor_grant",
        "sonic_infested_ally",
        "sonic_uninfested_ally",
        "ordinary_infested_ally",
    ];
    let rows = corpus["action_controls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| ordinary_grants.contains(&row["input"]["name"].as_str().unwrap()))
        .chain(corpus["grant_controls"].as_array().unwrap());
    let mut compared = 0;
    for row in rows {
        let input = &row["input"];
        let yes_no = |key: &str| if input[key] == true { "yes" } else { "no" };
        // Explicit scenario inputs recreate native consumer priors through the
        // actual layered readers. They do not claim retail MTNK authors these
        // grants, or that an infestation/transport lifecycle has run.
        let patch = crate::rules::ini_parser::IniFile::from_str(&format!(
            "[MTNK]\nAttackFriendlies={}\nAttackCursorOnFriendlies={}\nPassengers={}\n\
             [AP]\nIvanBomb={}\nSonic={}\n",
            yes_no("attack_friendlies"),
            yes_no("attack_cursor_on_friendlies"),
            input["target_passengers"].as_u64().unwrap(),
            yes_no("ivan_bomb"),
            yes_no("sonic"),
        ));
        let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map_with_patch(
            "XMP03T4.MAP",
            &patch,
        ) else {
            return;
        };
        let (mut sim, actor, target) = object_pair(&battle.rules, "MTNK", "ally");
        if input["infested"] == true {
            crate::sim::arena_fixture::install_action_infestation(
                sim.entities_mut_for_test().get_mut(target).unwrap(),
                actor,
            );
        }
        let before = sim.state_hash();
        assert_eq!(
            friendly_attack_is_admitted(&sim, &battle.rules, actor, target),
            row["action"] == 5,
            "{}: Techno admission boundary only",
            input["name"]
        );
        assert_eq!(sim.state_hash(), before);
        compared += 1;
    }
    assert_eq!(compared, ordinary_grants.len() + 2);
}

#[test]
fn allied_object_cursor_does_not_offer_selection_for_another_allied_house() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &battle.processed_rules,
        Some(&battle.fixed_art),
    );
    let (sim, actor, target) = object_pair(&battle.rules, "MTNK", "ally");
    let hover = picked_target(&sim, target);
    assert_eq!(hover.kind, HoverTargetKind::FriendlyUnit);
    let feedback = crate::app::input::cursor::capability_cursor_for_hover(
        &sim,
        &[actor],
        Some(actor),
        &hover,
        Some(&battle.rules),
        Some(&overlays),
        crate::app::input::context_order::OrderModifier::Normal,
        false,
        false,
    );
    let corpus = crate::test_fixture::json("tools/spatial_oracle/allied_object_action.json");
    let native = corpus["action_controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "other_ally")
        .unwrap();
    let expected = match native["cursor"]["output"]["cursor_row"].as_u64().unwrap() {
        19 => CursorId::NoMove,
        other => panic!("unexpected original cursor row {other}"),
    };
    assert_eq!(
        crate::app::input::cursor::cursor_id_for_feedback(feedback),
        Some(expected)
    );
}

#[test]
fn allied_object_click_dispatches_the_existing_move_receiver() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &battle.processed_rules,
        Some(&battle.fixed_art),
    );
    let (sim, actor, target) = object_pair(&battle.rules, "MTNK", "ally");
    assert_eq!(
        object_click_release(
            &sim,
            &battle.rules,
            &[actor],
            &[actor],
            target,
            &overlays,
            false
        ),
        LeftRelease::Dispatch
    );
    let owner = sim.entities().get(actor).unwrap().owner();
    let command = object_attack_click_command(
        &sim,
        &battle.rules,
        &overlays,
        owner,
        OrderMode::Move,
        OrderModifier::Normal,
        false,
        actor,
        target,
    );
    assert!(
        matches!(command, Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. }) if entity_id == actor),
        "NoMove2 still dispatches a Move through the existing destination owner: {command:?}"
    );
}

/// Infantry and Building cursor owners take their own What_Action over
/// another allied House's tank too. Techno's tail answers 0 for an armed
/// human player's object (`0x0070056C..0x0070059B`): a GI's Infantry tail
/// turns it into NoMove (`0x0051F05E..0x0051F08B`), which dispatches a move
/// near the tank, and a Pillbox's Building What_Action keeps it
/// (`0x0044750D`): the arrow, whose release still selects the tank. Retail
/// arms the Engineer (`Primary=DefuseKit`), so it answers NoMove too; an
/// unarmed civilian keeps Techno's Select. Over the player's own tank all
/// answer Select.
#[test]
fn allied_object_infantry_and_building_owners_answer_their_own_tails() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &battle.processed_rules,
        Some(&battle.fixed_art),
    );
    let rows = [
        ("E1", "ally", CursorId::NoMove, LeftRelease::Dispatch),
        ("ENGINEER", "ally", CursorId::NoMove, LeftRelease::Dispatch),
        ("CIV1", "ally", CursorId::Select, LeftRelease::Select),
        ("GAPILL", "ally", CursorId::Default, LeftRelease::Select),
        ("E1", "own", CursorId::Select, LeftRelease::Select),
        ("CIV1", "own", CursorId::Select, LeftRelease::Select),
        ("GAPILL", "own", CursorId::Select, LeftRelease::Select),
    ];
    for (kind, relation, cursor, release) in rows {
        let (sim, actor, target) = object_pair(&battle.rules, kind, relation);
        let hover = picked_target(&sim, target);
        assert_eq!(hover.kind, HoverTargetKind::FriendlyUnit);
        assert!(object_action_answers_friendly(
            &sim,
            &[actor],
            actor,
            target
        ));
        let feedback = crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[actor],
            Some(actor),
            &hover,
            Some(&battle.rules),
            Some(&overlays),
            OrderModifier::Normal,
            false,
            false,
        );
        assert_eq!(
            crate::app::input::cursor::cursor_id_for_feedback(feedback),
            Some(cursor),
            "{kind} over {relation}"
        );
        assert_eq!(
            object_click_release(
                &sim,
                &battle.rules,
                &[actor],
                &[actor],
                target,
                &overlays,
                false
            ),
            release,
            "{kind} over {relation}"
        );
    }
    let (sim, gi, tank) = object_pair(&battle.rules, "E1", "ally");
    let owner = sim.entities().get(gi).unwrap().owner();
    let command = object_attack_click_command(
        &sim,
        &battle.rules,
        &overlays,
        owner,
        OrderMode::Move,
        OrderModifier::Normal,
        false,
        gi,
        tank,
    );
    assert!(
        matches!(command, Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. }) if entity_id == gi),
        "the GI's NoMove moves him near the tank: {command:?}"
    );
}

/// A selected building over another selected own building answers 0
/// (Techno `0x007005BF`, kept by Building `0x0044750D`), so the arrow shows
/// and the click neither selects nor orders.
#[test]
fn a_building_over_another_selected_building_does_nothing() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &battle.processed_rules,
        Some(&battle.fixed_art),
    );
    let (mut sim, pillbox, _) = object_pair(&battle.rules, "GAPILL", "own");
    let other = sim
        .spawn_object("GAPILL", "Americans", 10, 14, 0, &battle.rules)
        .unwrap();
    sim.entities_mut_for_test()
        .get_mut(other)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    let selected = [pillbox, other];
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyStructure,
        stable_id: other,
    };
    let owner = sim.entities().get(pillbox).unwrap().owner();
    let best = cursor_owner(&sim, &battle.rules, &selected, other).expect("a cursor owner");
    let feedback = crate::app::input::cursor::capability_cursor_for_hover(
        &sim,
        &selected,
        Some(best),
        &hover,
        Some(&battle.rules),
        Some(&overlays),
        OrderModifier::Normal,
        false,
        false,
    );
    assert_eq!(
        crate::app::input::cursor::cursor_id_for_feedback(feedback),
        Some(CursorId::Default)
    );
    assert_eq!(
        building_selection_click(
            &sim,
            &battle.rules,
            &overlays,
            &selected,
            &selected,
            owner,
            Some(&hover),
            (10, 14),
            OrderModifier::Normal,
            false,
        ),
        BuildingClick::Nothing
    );
}

/// Techno What_Action's self rung (`0x007000B9..0x007000DF`) answers before
/// its attack rung for the lone selected object over itself, so a Crazy
/// Ivan's friendly-bomb grant never applies there: his self arm leaves 0
/// (`0x0051ECFC`), his tail makes it NoMove, and the click moves him near
/// himself. A tank's NoMove over itself orders nothing (`0x007388D5`).
#[test]
fn a_lone_selected_ivan_clicking_himself_bombs_nothing() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let (sim, ivan, _) = object_pair(&battle.rules, "IVAN", "own");
    let owner = sim.entities().get(ivan).unwrap().owner();
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyUnit,
        stable_id: ivan,
    };
    assert!(
        self_click_command(
            &sim,
            &battle.rules,
            None,
            owner,
            &[ivan],
            Some(&hover),
            false
        )
        .is_none()
    );
    assert_eq!(
        attack_action(
            &sim,
            &battle.rules,
            ivan,
            &TargetKind::Entity(ivan),
            false,
            None
        ),
        AttackAction::Attack,
        "the attack rung alone would admit the self-bomb"
    );
    let release = lone_self_foot_release(
        &sim,
        &battle.rules,
        owner,
        &[ivan],
        ivan,
        OrderMode::Move,
        false,
    );
    assert!(
        matches!(
            release,
            Some((
                LeftRelease::Dispatch,
                Some(Command::Move { entity_id, .. } | Command::MoveToObject { entity_id, .. })
            )) if entity_id == ivan
        ),
        "{release:?}"
    );
    assert_eq!(
        lone_self_foot_release(
            &sim,
            &battle.rules,
            owner,
            &[ivan, 99],
            ivan,
            OrderMode::Move,
            false
        ),
        None,
        "a group member is no lone self"
    );
    let (sim, tank, _) = object_pair(&battle.rules, "MTNK", "own");
    assert_eq!(
        lone_self_foot_release(
            &sim,
            &battle.rules,
            owner,
            &[tank],
            tank,
            OrderMode::Move,
            false
        ),
        Some((LeftRelease::Dispatch, None))
    );
}

/// The Unit and Infantry tails turn a NoMove over an object worn as Terrain
/// with no worn house into Move (Unit `0x00740440..0x00740476`, Infantry
/// `0x0051F114..0x0051F14F`), as the Aircraft's does: over an ally's tank
/// wearing a tree, a GI and a tank show Move and move to it.
#[test]
fn a_ground_owner_moves_onto_an_ally_worn_as_a_tree() {
    let Some(battle) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
    else {
        return;
    };
    let overlays = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &battle.processed_rules,
        Some(&battle.fixed_art),
    );
    for kind in ["E1", "MTNK"] {
        let (mut sim, actor, target) = object_pair(&battle.rules, kind, "ally");
        let hover = picked_target(&sim, target);
        let tree = sim.intern("TREE01");
        let mut disguise = crate::sim::cloak_disguise::DisguiseRuntime::new(0);
        disguise.acquire(0, Some(tree), None);
        sim.entities_mut_for_test()
            .get_mut(target)
            .unwrap()
            .disguise = Some(disguise);
        assert_eq!(
            attack_action(
                &sim,
                &battle.rules,
                actor,
                &TargetKind::Entity(target),
                false,
                Some(&overlays)
            ),
            AttackAction::Move,
            "{kind}"
        );
        let feedback = crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[actor],
            Some(actor),
            &hover,
            Some(&battle.rules),
            Some(&overlays),
            OrderModifier::Normal,
            false,
            false,
        );
        assert_eq!(
            crate::app::input::cursor::cursor_id_for_feedback(feedback),
            Some(CursorId::Move),
            "{kind}"
        );
    }
}
