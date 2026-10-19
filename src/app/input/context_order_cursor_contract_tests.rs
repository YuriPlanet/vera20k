//! Paired hover/command checks for the tactical cursor repair.
use super::*;
use crate::app::input::cursor::{capability_cursor_for_hover, cursor_id_for_feedback};
use crate::app::types::CursorId;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    let owner = sim.interner.intern("Americans");
    sim.session.current_house = Some(owner);
    sim.session
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
            sim.house_alliances
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
            sim.session.current_house.unwrap(),
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
