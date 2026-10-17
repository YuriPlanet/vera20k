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
    sim.fog = crate::sim::vision::FogState::new(32, 32);
    for (index, (name, human)) in [("Americans", true), ("Russians", false)]
        .into_iter()
        .enumerate()
    {
        let owner = sim.interner.intern(name);
        sim.houses.insert(
            owner,
            HouseState::new(owner, index as u8, None, human, 10_000, 10),
        );
        sim.session.house_order.push(owner);
    }
    let owner = sim.interner.get("Americans").unwrap();
    sim.session.game_mode_nonzero = true;
    sim.session.current_house = Some(owner);
    if relation == "ally" || relation == "reverse-ally" {
        let (asker, other) = if relation == "ally" {
            ("AMERICANS", "RUSSIANS")
        } else {
            ("RUSSIANS", "AMERICANS")
        };
        // An admitted directional alliance is an input to this component
        // scene; production House diplomacy owns installing/mutating it.
        sim.house_alliances
            .entry(asker.into())
            .or_default()
            .insert(other.into());
        sim.fog.alliances = sim.house_alliances.clone();
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
    sim.fog.reveal_all_for_owner(owner);
    sim.entities_mut()
        .get_mut(actor)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    (sim, actor, target)
}

fn picked_target(sim: &Simulation, rules: &RuleSet, target: u64) -> HoverTargetKindWithId {
    let (x, y) = crate::app::presentation::instances::interpolated_screen_position_entity(
        sim.entities().get(target).unwrap(),
    );
    let hover = crate::app::input::entity_pick::hover_target_at_point(
        sim,
        x,
        y,
        "Americans",
        false,
        Some(rules),
        &Default::default(),
        None,
    )
    .expect("revealed ordinary tank is picked");
    assert_eq!(hover.stable_id, target);
    hover
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
    let hover = picked_target(&sim, &battle.rules, target);
    assert_eq!(hover.kind, HoverTargetKind::FriendlyUnit);
    assert_eq!(
        crate::app::input::cursor::capability_cursor_for_hover(
            &sim,
            &[actor],
            Some(actor),
            &hover,
            Some(&battle.rules),
            None,
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
            sim.bomb_attach(actor, Some(target), &battle.rules);
        }
        let entity = sim.entities().get(target).unwrap();
        // Compare this shared refusal leaf only: a previously attached bomb
        // can make GetFireError leave Attack before the native53/54 consumer.
        assert_eq!(
            ivan_bomb_refused(
                entity,
                sim.interner.object_type(entity.type_ref(), &battle.rules)
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
                sim.entities_mut().get_mut(target).unwrap(),
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
    let hover = picked_target(&sim, &battle.rules, target);
    assert_eq!(hover.kind, HoverTargetKind::FriendlyUnit);
    let feedback = crate::app::input::cursor::capability_cursor_for_hover(
        &sim,
        &[actor],
        Some(actor),
        &hover,
        Some(&battle.rules),
        Some(&overlays),
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
        object_click_release(&sim, &battle.rules, &[actor], &[actor], target, &overlays,),
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
