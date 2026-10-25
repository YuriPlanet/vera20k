//! Techno700444..700474 rejects an unarmed attack even under Ctrl.
//! The ordinary selection/Unit NoMove tail and Foot click owner then apply.
//! Original action, cursor and LeftUp/receiver boundaries are consumed from
//! allied_object_action.json; the flat Rust arena is not a movement golden.

use super::*;
use crate::app::input::cursor::{capability_cursor_for_hover, cursor_id_for_feedback};
use crate::app::types::CursorId;
use crate::rules::retail_ini_fixture::RetailBattleRules;
use crate::rules::{overlay_types::OverlayTypeRegistry, ruleset::RuleSet};
use crate::sim::world::Simulation;

fn scene(rules: &RuleSet, selected_target: bool) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x700474);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session_mut_for_test()
        .game_options_mut_for_test()
        .short_game = false;
    let owner = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.fog_mut_for_test().reveal_all_for_owner(owner);
    let miner = sim
        .spawn_object("CMIN", "Americans", 10, 10, 0, rules)
        .unwrap();
    let enemy = sim
        .spawn_object("MTNK", "Russians", 20, 10, 0, rules)
        .unwrap();
    sim.entities_mut_for_test()
        .get_mut(miner)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    sim.entities_mut_for_test()
        .get_mut(enemy)
        .unwrap()
        .selection
        .set_selected_for_test(selected_target);
    (sim, miner, enemy)
}

fn retail() -> Option<RetailBattleRules> {
    crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
}

fn click(
    sim: &Simulation,
    rules: &RuleSet,
    overlays: &OverlayTypeRegistry,
    miner: u64,
    enemy: u64,
    modifier: OrderModifier,
) -> Option<Command> {
    object_attack_click_command(
        sim,
        rules,
        overlays,
        sim.session().current_house().unwrap(),
        OrderMode::Move,
        modifier,
        false,
        miner,
        enemy,
    )
}

#[test]
fn retail_unarmed_ctrl_click_matches_original_selection_and_navigation() {
    let Some(retail) = retail() else {
        return;
    };
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let corpus = crate::test_fixture::json("tools/spatial_oracle/allied_object_action.json");
    let rows = corpus["unarmed_object_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    for row in rows {
        let input = &row["input"];
        let selected = input["target_selected"].as_bool().unwrap();
        let forced = input["ctrl"].as_bool().unwrap();
        let modifier = if forced {
            OrderModifier::ForceFire
        } else {
            OrderModifier::Normal
        };
        let (sim, miner, enemy) = scene(&retail.rules, selected);
        let actor = sim.entities().get(miner).unwrap();
        assert!(!crate::sim::combat::combat_weapon::is_armed(
            actor,
            retail.rules.object("CMIN").unwrap(),
        ));
        let before = sim.state_hash();
        let rng = sim.rng_state();
        let feedback = capability_cursor_for_hover(
            &sim,
            &[miner],
            Some(miner),
            &HoverTargetKindWithId {
                kind: HoverTargetKind::EnemyUnit,
                stable_id: enemy,
            },
            Some(&retail.rules),
            Some(&overlays),
            modifier,
            false,
            false,
        );
        let expected_cursor = match row["cursor"]["output"]["cursor_row"].as_u64().unwrap() {
            17 => CursorId::Select,
            19 => CursorId::NoMove,
            other => panic!("unexpected original cursor row {other}"),
        };
        assert_eq!(cursor_id_for_feedback(feedback), Some(expected_cursor));
        let release = object_click_release(
            &sim,
            &retail.rules,
            &[miner],
            &[miner],
            enemy,
            &overlays,
            forced,
        );
        let command = click(&sim, &retail.rules, &overlays, miner, enemy, modifier);
        match row["left_up"]["boundary"].as_str().unwrap() {
            "replace_selection" => {
                assert_eq!(release, LeftRelease::Select, "{input}");
                assert_eq!(command, None, "{input}");
            }
            "object_dispatch" => {
                assert_eq!(release, LeftRelease::Dispatch, "{input}");
                assert!(
                    matches!(command, Some(Command::Move { entity_id, .. }) if entity_id == miner),
                    "Foot NoMove dispatches movement, not Attack: {input}: {command:?}",
                );
            }
            other => panic!("unexpected original LeftUp boundary {other}"),
        }
        assert_eq!(sim.state_hash(), before, "input queries do not retask");
        assert_eq!(sim.rng_state(), rng);
    }
}

#[test]
fn retail_unarmed_ctrl_selected_target_moves_through_the_command_and_tick_owners() {
    let Some(retail) = retail() else {
        return;
    };
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let (mut sim, miner, enemy) = scene(&retail.rules, true);
    let command = click(
        &sim,
        &retail.rules,
        &overlays,
        miner,
        enemy,
        OrderModifier::ForceFire,
    )
    .expect("native NoMove is a movement order");
    assert!(matches!(command, Command::Move { entity_id, .. } if entity_id == miner));
    let envelope = crate::app::input::commands::roundtrip_ordinary_local_megamission(
        &sim,
        CommandEnvelope::new(
            sim.session().current_house().unwrap(),
            sim.session().tick(),
            command,
        ),
    )
    .unwrap();
    sim.queue_command(envelope);
    let position = &sim.entities().get(miner).unwrap().position;
    let start = (position.rx(), position.ry());
    let mut moved = false;
    for _ in 0..120 {
        let commands = sim.take_due_commands();
        let grid = sim.path_grid_snapshot();
        sim.advance_tick(
            &commands,
            Some(&retail.rules),
            grid.as_deref(),
            Some(&overlays),
            33,
        );
        let actor = sim.entities().get(miner).unwrap();
        assert_eq!(actor.attack_target.target(), None);
        moved |= (actor.position.rx(), actor.position.ry()) != start;
        if moved {
            break;
        }
    }
    assert!(moved, "the existing movement owner consumes the click");
}

#[test]
fn a_mixed_ctrl_click_keeps_the_armed_order_without_an_unarmed_attack() {
    let Some(retail) = retail() else {
        return;
    };
    let overlays = OverlayTypeRegistry::from_ini(&retail.processed_rules, Some(&retail.fixed_art));
    let (mut sim, miner, enemy) = scene(&retail.rules, false);
    let tank = sim
        .spawn_object("MTNK", "Americans", 10, 12, 0, &retail.rules)
        .unwrap();
    let selected = [miner, tank];
    assert_eq!(
        cursor_owner(&sim, &retail.rules, &selected, enemy),
        Some(tank)
    );
    let armed = attack_branch_orders(
        &sim,
        &retail.rules,
        &selected,
        TargetKind::Entity(enemy),
        true,
        OrderMode::Move,
        &overlays,
    );
    assert_eq!(armed.receivers, [tank].into_iter().collect());
    assert_eq!(
        armed.commands,
        [Command::Attack {
            attacker_id: tank,
            target_id: enemy
        }]
    );
    assert_eq!(
        object_click_release(
            &sim,
            &retail.rules,
            &selected,
            &[miner],
            enemy,
            &overlays,
            true
        ),
        LeftRelease::Dispatch,
    );
    assert_eq!(
        click(
            &sim,
            &retail.rules,
            &overlays,
            miner,
            enemy,
            OrderModifier::ForceFire
        ),
        None,
    );
}
