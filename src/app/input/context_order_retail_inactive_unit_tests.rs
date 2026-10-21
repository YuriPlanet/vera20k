//! Objects that cannot act over an object or a Cell, on retail rules: a Unit
//! that is not IsActive (`UnitClass::What_Action_OnObject @ 0x0073FD50`,
//! `0x0073FE48`), and one being warped out (Techno `0x006FFF03`,
//! `0x00700731`), through the attack rung, the cursor and the click.

use super::retail_entry_tests::{arena, retail};
use super::*;
use crate::app::types::CursorId;
use crate::sim::world::Simulation;

/// The cursor of the lone selected `actor` over `target`.
fn hover_cursor(
    sim: &Simulation,
    rules: &crate::rules::ruleset::RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    actor: u64,
    target: u64,
    kind: crate::app::types::HoverTargetKind,
    modifier: OrderModifier,
) -> Option<CursorId> {
    crate::app::input::cursor::cursor_id_for_feedback(
        crate::app::input::cursor::capability_cursor_for_hover(
            sim,
            &[actor],
            Some(actor),
            &crate::app::input::entity_pick::HoverTargetKindWithId {
                kind,
                stable_id: target,
            },
            Some(rules),
            Some(registry),
            modifier,
            false,
            false,
        ),
    )
}

/// A Robot Tank whose control is offline (not IsActive, `0x007010D0`)
/// answers Move over an enemy GI it may crush and Select over an enemy tank
/// with Ctrl, and shows those cursors; an active one attacks both. Without
/// Ctrl it answers 0, the arrow: its GetFireError (vt+0x3C0) is ILLEGAL,
/// which Techno's attack rung refuses without force
/// (`0x00700542..0x0070055B`). Its Ctrl click on the GI sends
/// no order: `FootClass::ClickedAction_Object` (`0x004D74E0`) refuses Move
/// for an object that is not controllable (vt+0xA0). The one on the tank
/// selects.
#[test]
fn retail_inactive_robot_tank_crushes_or_selects_instead_of_attacking() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = arena(rules);
    // The Russians keep a building, so the skirmish has no winner.
    sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, rules)
        .expect("russian power plant");
    let robot = sim
        .spawn_object("ROBO", "Americans", 16, 16, 0, rules)
        .expect("robot tank");
    let gi = sim
        .spawn_object("E1", "Russians", 17, 16, 0, rules)
        .expect("gi");
    let tank = sim
        .spawn_object("MTNK", "Russians", 16, 17, 0, rules)
        .expect("tank");
    let action = |sim: &Simulation, target, forced| {
        attack_action(
            sim,
            rules,
            robot,
            &TargetKind::Entity(target),
            forced,
            Some(&registry),
        )
    };
    let enemy_cursor = |sim: &Simulation, target, modifier| {
        hover_cursor(
            sim,
            rules,
            &registry,
            robot,
            target,
            crate::app::types::HoverTargetKind::EnemyUnit,
            modifier,
        )
    };
    for forced in [false, true] {
        assert_eq!(action(&sim, gi, forced), AttackAction::Attack);
        assert_eq!(action(&sim, tank, forced), AttackAction::Attack);
    }
    // The active robot's Ctrl click on the GI fires.
    let fire = attack_branch_orders(
        &sim,
        rules,
        &[robot],
        TargetKind::Entity(gi),
        true,
        &registry,
    );
    assert!(!fire.commands.is_empty());
    sim.entities_mut_for_test()
        .get_mut(robot)
        .unwrap()
        .deactivated
        .set_for_test(true);
    assert_eq!(action(&sim, gi, false), AttackAction::None);
    assert_eq!(action(&sim, tank, false), AttackAction::None);
    assert_eq!(action(&sim, gi, true), AttackAction::Move);
    assert_eq!(action(&sim, tank, true), AttackAction::Select);
    assert_eq!(
        enemy_cursor(&sim, gi, OrderModifier::Normal),
        Some(CursorId::Default)
    );
    assert_eq!(
        enemy_cursor(&sim, gi, OrderModifier::ForceFire),
        Some(CursorId::Move)
    );
    assert_eq!(
        enemy_cursor(&sim, tank, OrderModifier::ForceFire),
        Some(CursorId::Select)
    );
    let crush = attack_branch_orders(
        &sim,
        rules,
        &[robot],
        TargetKind::Entity(gi),
        true,
        &registry,
    );
    assert!(crush.consumed && !crush.select_fallback);
    assert!(crush.commands.is_empty(), "{:?}", crush.commands);
    let select = attack_branch_orders(
        &sim,
        rules,
        &[robot],
        TargetKind::Entity(tank),
        true,
        &registry,
    );
    assert!(select.select_fallback && select.commands.is_empty());
}

/// A tank being warped out (Temporal, vt+0x1D4) answers 0 over an enemy
/// (`0x006FFF03..0x006FFF8A`: armed, and another House's object) and on the ground with
/// Ctrl (`0x00700731`), and Select over an own unselected tank: no attack and
/// no force-fire.
#[test]
fn retail_tank_being_warped_out_neither_attacks_nor_fires() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = arena(rules);
    sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, rules)
        .expect("russian power plant");
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 0, rules)
        .expect("tank");
    let enemy = sim
        .spawn_object("MTNK", "Russians", 17, 16, 0, rules)
        .expect("enemy");
    let own = sim
        .spawn_object("MTNK", "Americans", 16, 17, 0, rules)
        .expect("own tank");
    let action = |sim: &Simulation, target: TargetKind| {
        attack_action(sim, rules, tank, &target, true, Some(&registry))
    };
    assert_eq!(
        action(&sim, TargetKind::Entity(enemy)),
        AttackAction::Attack
    );
    assert_eq!(action(&sim, TargetKind::Cell(18, 18)), AttackAction::Attack);
    sim.entities_mut_for_test()
        .get_mut(tank)
        .unwrap()
        .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(99));
    assert!(sim.entities().get(tank).unwrap().is_warped_out());
    assert_eq!(action(&sim, TargetKind::Entity(enemy)), AttackAction::None);
    assert_eq!(action(&sim, TargetKind::Cell(18, 18)), AttackAction::None);
    assert_eq!(action(&sim, TargetKind::Entity(own)), AttackAction::Select);
    let fire = attack_branch_orders(
        &sim,
        rules,
        &[tank],
        TargetKind::Cell(18, 18),
        true,
        &registry,
    );
    assert!(fire.commands.is_empty(), "{:?}", fire.commands);
    // The warp arm precedes Shift's Select and the self rung: Select over
    // the own tank, which is not selected, and the arrow over itself, which
    // is.
    sim.entities_mut_for_test()
        .get_mut(tank)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    let own_cursor = |sim: &Simulation, target, modifier| {
        hover_cursor(
            sim,
            rules,
            &registry,
            tank,
            target,
            crate::app::types::HoverTargetKind::FriendlyUnit,
            modifier,
        )
    };
    assert_eq!(
        own_cursor(&sim, own, OrderModifier::Shift),
        Some(CursorId::Select)
    );
    assert_eq!(
        own_cursor(&sim, tank, OrderModifier::Normal),
        Some(CursorId::Default)
    );
    assert_eq!(
        own_cursor(&sim, tank, OrderModifier::Shift),
        Some(CursorId::Default)
    );
}

/// An MCV being warped out (the Chronosphere's warp-out leaves it selected;
/// the Temporal one stands in) answers 0 over itself, as the warp arm
/// (`0x006FFF03..0x006FFF8A`) precedes the self rung (`0x007000B9`): no
/// Deploy, no NoDeploy and the arrow, where a calm one deploys.
#[test]
fn retail_mcv_being_warped_out_does_not_deploy() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = arena(rules);
    sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, rules)
        .expect("russian power plant");
    let mcv = sim
        .spawn_object("AMCV", "Americans", 16, 16, 0, rules)
        .expect("mcv");
    sim.entities_mut_for_test()
        .get_mut(mcv)
        .unwrap()
        .selection
        .set_selected_for_test(true);
    let owner = sim.entities().get(mcv).unwrap().owner();
    let hover = HoverTargetKindWithId {
        kind: HoverTargetKind::FriendlyUnit,
        stable_id: mcv,
    };
    let deploys = |sim: &Simulation| {
        self_click_command(
            sim,
            rules,
            Some(&registry),
            owner,
            &[mcv],
            Some(&hover),
            false,
        )
    };
    let self_cursor = |sim: &Simulation| {
        hover_cursor(
            sim,
            rules,
            &registry,
            mcv,
            mcv,
            HoverTargetKind::FriendlyUnit,
            OrderModifier::Normal,
        )
    };
    assert!(deploys(&sim).is_some());
    assert_eq!(self_cursor(&sim), Some(CursorId::Deploy));
    sim.entities_mut_for_test()
        .get_mut(mcv)
        .unwrap()
        .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(99));
    assert_eq!(deploys(&sim), None);
    assert!(!self_answers_no_deploy(
        &sim,
        rules,
        Some(&registry),
        owner,
        &[mcv],
        &hover,
        false
    ));
    assert_eq!(self_cursor(&sim), Some(CursorId::Default));
}
