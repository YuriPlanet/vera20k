//! The Grinder's click chain on retail rules: CAN_LOAD (`0x0043C4F8`), the
//! Unit (`0x00740122..0x007401BC`) and Infantry (`0x0051F0AF..0x0051F111`)
//! What_Action rows, the Enter/NoEnter cursor rows and Foot ClickedAction's
//! mission Eaten (`0x004D774F`), then the existing Eaten walk and refund.

use super::*;
use crate::app::input::cursor::{capability_cursor_for_hover, cursor_id_for_feedback};
use crate::app::types::CursorId;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::world::Simulation;

fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::new();
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let owner = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(owner, side, None, true, 0, 10));
    }
    sim.session
        .set_current_house_for_test(sim.interner.get("Americans"));
    sim.session
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim
}

/// The player's Grizzly and GI, and a Grinder of `grinder_house`.
fn scene(rules: &RuleSet, grinder_house: &str) -> (Simulation, u64, u64, u64) {
    let mut sim = arena(rules);
    if grinder_house != "Americans" {
        for (from, to) in [("AMERICANS", "RUSSIANS"), ("RUSSIANS", "AMERICANS")] {
            sim.house_alliances
                .entry(from.into())
                .or_default()
                .insert(to.into());
        }
    }
    // The Russians keep a building, so the skirmish has no winner and every
    // frame reaches its late delete drain.
    sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, rules)
        .expect("russian power plant");
    let grinder = sim
        .spawn_object("YAGRND", grinder_house, 20, 16, 0, rules)
        .expect("grinder");
    let tank = sim
        .spawn_object("MTNK", "Americans", 13, 16, 0, rules)
        .expect("tank");
    let gi = sim
        .spawn_object("E1", "Americans", 13, 19, 0, rules)
        .expect("gi");
    (sim, grinder, tank, gi)
}

fn cursor(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    actor: u64,
    grinder: u64,
) -> Option<CursorId> {
    chord_cursor(
        sim,
        rules,
        registry,
        actor,
        grinder,
        OrderModifier::Normal,
        false,
    )
}

fn chord_cursor(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    actor: u64,
    grinder: u64,
    modifier: OrderModifier,
    force_move_key: bool,
) -> Option<CursorId> {
    cursor_id_for_feedback(capability_cursor_for_hover(
        sim,
        &[actor],
        Some(actor),
        &HoverTargetKindWithId {
            stable_id: grinder,
            kind: HoverTargetKind::FriendlyStructure,
        },
        Some(rules),
        Some(registry),
        modifier,
        force_move_key,
    ))
}

fn hover(grinder: u64) -> HoverTargetKindWithId {
    HoverTargetKindWithId {
        stable_id: grinder,
        kind: HoverTargetKind::FriendlyStructure,
    }
}

fn eaten(entity_id: u64, grinder: u64) -> Command {
    Command::MegaMission {
        entity_id,
        mission: crate::sim::mission::MissionType::Eaten,
        target: None,
        destination: Some(TargetKind::Entity(grinder)),
    }
}

/// A player's vehicle and rifleman over the player's own Grinder show the
/// Enter row, and the click sends each mission Eaten; both walk in and are
/// ground for a refund.
#[test]
fn a_players_vehicle_and_rifleman_grind_at_their_own_grinder() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    assert!(rules.object("YAGRND").unwrap().grinding);
    let (mut sim, grinder, tank, gi) = scene(rules, "Americans");
    assert_eq!(
        cursor(&sim, rules, &registry, tank, grinder),
        Some(CursorId::Enter)
    );
    assert_eq!(
        cursor(&sim, rules, &registry, gi, grinder),
        Some(CursorId::Enter)
    );
    let mut selected = vec![tank, gi];
    let orders = grinder_orders(&sim, rules, &mut selected, &hover(grinder), Some(&registry))
        .expect("both grind");
    assert!(selected.is_empty());
    assert_eq!(orders, [eaten(tank, grinder), eaten(gi, grinder)]);

    let owner = sim.interner.get("Americans").unwrap();
    let credits = sim.houses[&owner].economy().credits();
    for order in &orders {
        assert!(sim.apply_command("Americans", order, Some(rules)));
    }
    for _ in 0..900 {
        sim.advance_tick(&[], Some(rules), None, None, 67);
        if sim.entities().get(tank).is_none() && sim.entities().get(gi).is_none() {
            break;
        }
    }
    assert!(sim.entities().get(tank).is_none(), "the Grizzly is ground");
    assert!(sim.entities().get(gi).is_none(), "the GI is ground");
    assert!(sim.houses[&owner].economy().credits() > credits, "refunded");
}

/// Over an ally's Grinder an armed player's object has base action 0, not
/// Select (`0x0070056C`), so neither grinder row runs and the click sends no
/// one to be ground; the Unit tail (`0x007403EB`) makes the vehicle's NoMove
/// (2). The rifleman's cursor keeps the existing friendly-object fallback.
#[test]
fn armed_objects_over_an_allied_grinder_are_not_ground() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let (sim, grinder, tank, gi) = scene(rules, "Russians");
    assert_eq!(
        cursor(&sim, rules, &registry, tank, grinder),
        Some(CursorId::NoMove)
    );
    for actor in [tank, gi] {
        assert_eq!(
            grinder_action(&sim, rules, actor, grinder, Some(&registry)),
            None
        );
    }
    let mut selected = vec![tank, gi];
    assert_eq!(
        grinder_orders(&sim, rules, &mut selected, &hover(grinder), Some(&registry)),
        None
    );
    assert_eq!(selected, [tank, gi]);
}

/// A Crazy Ivan (`AttackCursorOnFriendlies=yes`) over the player's own
/// Grinder (`Bombable=` by default) has base action Attack, so the Ivan row
/// (`0x0051EB24`) answers IvanBomb before the grinder row is reached.
#[test]
fn a_crazy_ivan_bombs_the_players_grinder_rather_than_entering() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let (mut sim, grinder, _, _) = scene(rules, "Americans");
    let ivan = sim
        .spawn_object("IVAN", "Americans", 15, 19, 0, rules)
        .expect("ivan");
    assert_eq!(
        cursor(&sim, rules, &registry, ivan, grinder),
        Some(CursorId::IvanBomb)
    );
    let mut selected = vec![ivan];
    assert_eq!(
        grinder_orders(&sim, rules, &mut selected, &hover(grinder), Some(&registry)),
        None
    );
}

/// Alt answers Move before any object row (`0x007001E9`), in the
/// Ctrl+Shift+Alt chord too.
#[test]
fn the_alt_chord_moves_rather_than_grinds() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    let (sim, grinder, tank, _) = scene(rules, "Americans");
    assert_eq!(
        chord_cursor(
            &sim,
            rules,
            &registry,
            tank,
            grinder,
            OrderModifier::AttackMove,
            true,
        ),
        Some(CursorId::Move)
    );
}
