//! Paired hover/command checks for the tactical cursor repair.
use super::*;
use crate::app::input::cursor::{capability_cursor_for_hover, cursor_id_for_feedback};
use crate::app::types::CursorId;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    let owner = sim.intern("Americans");
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim
}

#[test]
fn retail_guard_cursor_and_click_share_alliance_admission() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    for relation in ["own", "ally", "enemy"] {
        let mut sim = arena(&retail.rules);
        if relation == "ally" {
            sim.house_alliances_mut_for_test()
                .entry("AMERICANS".into())
                .or_default()
                .insert("RUSSIANS".into());
        }
        let actor = sim
            .spawn_object("MTNK", "Americans", 16, 16, 64, &retail.rules)
            .unwrap();
        let target = sim
            .spawn_object(
                "MTNK",
                if relation == "own" {
                    "Americans"
                } else {
                    "Russians"
                },
                18,
                16,
                64,
                &retail.rules,
            )
            .unwrap();
        let hover = HoverTargetKindWithId {
            stable_id: target,
            kind: if relation == "enemy" {
                HoverTargetKind::EnemyUnit
            } else {
                HoverTargetKind::FriendlyUnit
            },
        };
        let before = sim.state_hash();
        let feedback = capability_cursor_for_hover(
            &sim,
            &[actor],
            Some(actor),
            &hover,
            Some(&retail.rules),
            Some(&registry),
            OrderModifier::GuardArea,
            true,
        );
        let command = object_attack_click_command(
            &sim,
            &retail.rules,
            &registry,
            sim.session().current_house().unwrap(),
            OrderMode::Move,
            OrderModifier::GuardArea,
            true,
            actor,
            target,
        );
        if relation == "enemy" {
            assert_eq!(cursor_id_for_feedback(feedback), Some(CursorId::Move));
            assert!(matches!(
                command,
                Some(Command::Move { .. } | Command::MoveToObject { .. })
            ));
        } else {
            assert_eq!(cursor_id_for_feedback(feedback), Some(CursorId::GuardArea));
            assert_eq!(
                command,
                Some(Command::Guard {
                    entity_id: actor,
                    target: Some(TargetKind::Entity(target))
                })
            );
        }
        assert_eq!(sim.state_hash(), before);
    }
}

#[test]
fn retail_weapon_cursors_keep_native_attack_and_sabotage_commands() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/mouse_oracle/tactical_actions.json",
    ))
    .unwrap();
    for (name, actor_type, target_type, expected) in [
        ("sapper", "TANY", "MTNK", CursorId::Demolish),
        ("flare", "BORIS", "NAPOWR", CursorId::AirStrike),
        ("c4_building", "TANY", "NAPOWR", CursorId::Demolish),
    ] {
        let mut sim = arena(&retail.rules);
        let actor = sim
            .spawn_object(actor_type, "Americans", 16, 16, 64, &retail.rules)
            .unwrap();
        let target = sim
            .spawn_object(target_type, "Russians", 18, 16, 64, &retail.rules)
            .unwrap();
        let hover = HoverTargetKindWithId {
            stable_id: target,
            kind: if target_type == "MTNK" {
                HoverTargetKind::EnemyUnit
            } else {
                HoverTargetKind::EnemyStructure
            },
        };
        let feedback = capability_cursor_for_hover(
            &sim,
            &[actor],
            Some(actor),
            &hover,
            Some(&retail.rules),
            Some(&registry),
            OrderModifier::ForceFire,
            false,
        );
        assert_eq!(cursor_id_for_feedback(feedback), Some(expected), "{name}");
        let dispatch = attack_branch_orders(
            &sim,
            &retail.rules,
            &[actor],
            TargetKind::Entity(target),
            true,
            &registry,
        );
        let mission = &native["weapons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["input"]["name"] == name)
            .unwrap()["output"]["clicked_mission"];
        let expected_command = if mission["mission"] == 17 {
            assert_eq!(mission["destination_is_object"], true);
            Command::PlantC4 {
                attacker_id: actor,
                target_building_id: target,
            }
        } else {
            assert_eq!(mission["mission"], 1);
            assert_eq!(mission["target_is_object"], true);
            Command::Attack {
                attacker_id: actor,
                target_id: target,
            }
        };
        assert_eq!(dispatch.commands, [expected_command], "{name}");
    }
}

#[test]
fn retail_engineer_forced_attack_keeps_native_no_move_override() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let mut sim = arena(&retail.rules);
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let actor = sim
        .spawn_object("ENGINEER", "Americans", 16, 16, 64, &retail.rules)
        .unwrap();
    let target = sim
        .spawn_object("MTNK", "Russians", 18, 16, 64, &retail.rules)
        .unwrap();
    let hover = HoverTargetKindWithId {
        stable_id: target,
        kind: HoverTargetKind::EnemyUnit,
    };
    let feedback = capability_cursor_for_hover(
        &sim,
        &[actor],
        Some(actor),
        &hover,
        Some(&retail.rules),
        Some(&registry),
        OrderModifier::ForceFire,
        false,
    );
    assert_eq!(cursor_id_for_feedback(feedback), Some(CursorId::NoMove));
    let dispatch = attack_branch_orders(
        &sim,
        &retail.rules,
        &[actor],
        TargetKind::Entity(target),
        true,
        &registry,
    );
    assert_eq!(dispatch.commands.len(), 1);
    assert!(matches!(
        dispatch.commands[0],
        Command::Move { .. } | Command::MoveToObject { .. }
    ));
}

/// Infantry What_Action's deployed arm (object `0x0051EB81..0x0051EC11`,
/// Cell `0x0051F8BB..0x0051F94E`): a deployed GI keeps Attack only where his
/// SelectWeapon(NULL) weapon (Para, range 5; standing M60 reaches 4) reaches,
/// so beyond it the cursor shows NoMove. Ctrl on an object then walks him
/// toward it (Infantry ClickedAction_Object `0x0051F190` hands 2 to Foot
/// `0x004D74E0`); Ctrl on a Cell sends nothing (`0x0051F250`). A GI on his
/// feet still attacks: he walks into range.
#[test]
fn a_deployed_gi_answers_nomove_beyond_his_deployed_range() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let mut sim = arena(rules);
    let deployed = sim
        .spawn_object("E1", "Americans", 16, 16, 64, rules)
        .unwrap();
    let standing = sim
        .spawn_object("E1", "Americans", 16, 22, 64, rules)
        .unwrap();
    assert!(sim.apply_command(
        "Americans",
        &Command::ToggleInfantryDeploy {
            entity_id: deployed
        },
        Some(rules)
    ));
    for _ in 0..32 {
        sim.advance_tick(&[], Some(rules), None, None, 67);
        if sim
            .entities()
            .get(deployed)
            .unwrap()
            .infantry_deploy_doing()
        {
            break;
        }
    }
    assert!(
        sim.entities()
            .get(deployed)
            .unwrap()
            .infantry_deploy_doing()
    );
    // About 4.5 Cells away: Para reaches it, M60 would not.
    let near = sim
        .spawn_object("MTNK", "Russians", 20, 18, 64, rules)
        .unwrap();
    let far = sim
        .spawn_object("MTNK", "Russians", 28, 16, 64, rules)
        .unwrap();
    let action =
        |actor, target, forced| attack_action(&sim, rules, actor, &target, forced, Some(&registry));
    assert_eq!(
        action(deployed, TargetKind::Entity(near), false),
        AttackAction::Attack
    );
    assert_eq!(
        action(deployed, TargetKind::Entity(far), false),
        AttackAction::NoMove
    );
    assert_eq!(
        action(deployed, TargetKind::Cell(20, 19), true),
        AttackAction::Attack
    );
    assert_eq!(
        action(deployed, TargetKind::Cell(28, 17), true),
        AttackAction::NoMove
    );
    assert_eq!(
        action(standing, TargetKind::Entity(far), false),
        AttackAction::Attack
    );
    assert_eq!(
        action(standing, TargetKind::Cell(28, 17), true),
        AttackAction::Attack
    );

    let cursor = |target| {
        cursor_id_for_feedback(capability_cursor_for_hover(
            &sim,
            &[deployed],
            Some(deployed),
            &HoverTargetKindWithId {
                stable_id: target,
                kind: HoverTargetKind::EnemyUnit,
            },
            Some(rules),
            Some(&registry),
            OrderModifier::Normal,
            false,
        ))
    };
    assert_eq!(cursor(near), Some(CursorId::Attack));
    assert_eq!(cursor(far), Some(CursorId::NoMove));

    let ctrl = |target| attack_branch_orders(&sim, rules, &[deployed], target, true, &registry);
    assert_eq!(
        ctrl(TargetKind::Entity(near)).commands,
        [Command::Attack {
            attacker_id: deployed,
            target_id: near
        }]
    );
    let walk = ctrl(TargetKind::Entity(far));
    assert!(walk.consumed);
    assert!(
        matches!(
            walk.commands.as_slice(),
            [Command::Move { .. } | Command::MoveToObject { .. }]
        ),
        "{:?}",
        walk.commands
    );
    let cell = ctrl(TargetKind::Cell(28, 17));
    assert!(cell.consumed && cell.commands.is_empty());
}
