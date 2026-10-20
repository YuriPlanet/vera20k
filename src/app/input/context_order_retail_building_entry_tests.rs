//! The building-entry click chain on retail rules: CAN_LOAD (Grinder
//! `0x0043C4F8`, Bunker `0x0043C506`, UnitRepair `0x0043C544`, refinery), the
//! Unit (`0x00740122..0x007401BC`) and Infantry (`0x0051F0AF..0x0051F111`)
//! What_Action rows, the Enter/NoEnter cursor rows and Foot ClickedAction's
//! missions Enter (case 3) and Eaten (`0x004D774F`), then the production
//! receivers.

use super::*;
use crate::app::input::cursor::{capability_cursor_for_hover, cursor_id_for_feedback};
use crate::app::types::CursorId;
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
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim
}

/// The player's Grizzly and GI, and a Grinder of `grinder_house`.
fn scene(rules: &RuleSet, grinder_house: &str) -> (Simulation, u64, u64, u64) {
    scene_with(rules, "YAGRND", grinder_house)
}

/// The player's Grizzly and GI, and a `kind` building of `house`.
fn scene_with(rules: &RuleSet, kind: &str, house: &str) -> (Simulation, u64, u64, u64) {
    let mut sim = arena(rules);
    if house != "Americans" {
        for (from, to) in [("AMERICANS", "RUSSIANS"), ("RUSSIANS", "AMERICANS")] {
            sim.house_alliances_mut_for_test()
                .entry(from.into())
                .or_default()
                .insert(to.into());
        }
    }
    // The Russians keep a building, so the skirmish has no winner and every
    // frame reaches its late delete drain.
    sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, rules)
        .expect("russian power plant");
    let building = sim
        .spawn_object(kind, house, 20, 16, 0, rules)
        .unwrap_or_else(|| panic!("{kind}"));
    let tank = sim
        .spawn_object("MTNK", "Americans", 13, 16, 0, rules)
        .expect("tank");
    let gi = sim
        .spawn_object("E1", "Americans", 13, 19, 0, rules)
        .expect("gi");
    (sim, building, tank, gi)
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

fn enter(entity_id: u64, building: u64) -> Command {
    Command::MegaMission {
        entity_id,
        mission: crate::sim::mission::MissionType::Enter,
        target: None,
        destination: Some(TargetKind::Entity(building)),
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
    let orders = building_entry_orders(
        &sim,
        rules,
        &selected.clone(),
        &mut selected,
        &hover(grinder),
        Some(&registry),
    )
    .expect("both grind");
    assert!(selected.is_empty());
    assert_eq!(orders, [eaten(tank, grinder), eaten(gi, grinder)]);

    let owner = sim.interner().get("Americans").unwrap();
    let credits = sim.houses()[&owner].economy().credits();
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
    assert!(
        sim.houses()[&owner].economy().credits() > credits,
        "refunded"
    );
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
            building_entry_action(&sim, rules, actor, grinder, Some(&registry)),
            None
        );
    }
    let mut selected = vec![tank, gi];
    assert_eq!(
        building_entry_orders(
            &sim,
            rules,
            &selected.clone(),
            &mut selected,
            &hover(grinder),
            Some(&registry)
        ),
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
        building_entry_orders(
            &sim,
            rules,
            &selected.clone(),
            &mut selected,
            &hover(grinder),
            Some(&registry)
        ),
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

fn retail() -> Option<(
    crate::rules::retail_ini_fixture::RetailBattleRules,
    crate::rules::overlay_types::OverlayTypeRegistry,
)> {
    let retail = crate::rules::retail_ini_fixture::retail_battle_rules()?;
    let registry = crate::rules::overlay_types::OverlayTypeRegistry::from_ini(
        &retail.processed_rules,
        Some(&retail.fixed_art),
    );
    Some((retail, registry))
}

/// A Chrono Miner over the player's refinery answers Enter through the
/// refinery's CAN_LOAD and is sent mission Enter, like any Unit the building
/// admits. Beside an armed Grizzly, which wins the cursor (DetermineAction
/// scores an armed object above an unarmed one) and answers Select, the
/// release selects the refinery and sends no one (`0x004ABE18`).
#[test]
fn a_miner_enters_the_players_refinery_unless_a_tank_owns_the_cursor() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, refinery, tank, _) = scene_with(rules, "GAREFN", "Americans");
    let miner = sim
        .spawn_object("CMIN", "Americans", 13, 13, 0, rules)
        .expect("miner");
    assert_eq!(
        cursor(&sim, rules, &registry, miner, refinery),
        Some(CursorId::Enter)
    );
    let mut selected = vec![miner];
    let orders = building_entry_orders(
        &sim,
        rules,
        &selected.clone(),
        &mut selected,
        &hover(refinery),
        Some(&registry),
    )
    .expect("the miner enters");
    assert_eq!(orders, [enter(miner, refinery)]);
    assert!(selected.is_empty());

    assert_eq!(
        building_entry_action(&sim, rules, tank, refinery, Some(&registry)),
        None
    );
    let mut selected = vec![tank, miner];
    assert_eq!(
        building_entry_orders(
            &sim,
            rules,
            &selected.clone(),
            &mut selected,
            &hover(refinery),
            Some(&registry)
        ),
        None
    );
    assert_eq!(selected, [tank, miner]);
}

/// The Service Depot's CAN_LOAD (`0x0043C544..0x0043C58C`) has no health
/// test, so a full-health Grizzly over the player's depot shows Enter and is
/// sent in. Its first Mission_Enter dispatch
/// (`0x004D9290`) sends DOCKING, which the depot refuses for a full-health
/// contact (IS_REPAIRING, `0x0043C814..0x0043C849`), so it breaks off to
/// Guard where it stands.
#[test]
fn a_full_health_tank_is_sent_to_the_depot_and_turned_away() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, depot, tank, gi) = scene_with(rules, "GADEPT", "Americans");
    let strength = rules.object("MTNK").unwrap().strength;
    assert_eq!(sim.entities().get(tank).unwrap().health.current(), strength);
    assert_eq!(
        cursor(&sim, rules, &registry, tank, depot),
        Some(CursorId::Enter)
    );
    // The GI has no depot row. Beside the tank it stands nearer the depot,
    // so its Select owns the cursor and the release selects the depot.
    assert_eq!(
        building_entry_action(&sim, rules, gi, depot, Some(&registry)),
        None
    );
    let mut selected = vec![tank, gi];
    assert_eq!(
        building_entry_orders(
            &sim,
            rules,
            &selected.clone(),
            &mut selected,
            &hover(depot),
            Some(&registry),
        ),
        None
    );
    let mut selected = vec![tank];
    let orders = building_entry_orders(
        &sim,
        rules,
        &selected.clone(),
        &mut selected,
        &hover(depot),
        Some(&registry),
    )
    .expect("the tank enters");
    assert_eq!(orders, [enter(tank, depot)]);
    assert!(sim.apply_command("Americans", &orders[0], Some(rules)));
    let cell = |sim: &Simulation| {
        let position = &sim.entities().get(tank).unwrap().position;
        (position.rx(), position.ry())
    };
    let start = cell(&sim);
    for _ in 0..90 {
        sim.advance_tick(&[], Some(rules), None, None, 67);
    }
    let entity = sim.entities().get(tank).unwrap();
    assert_eq!(
        entity.mission.effective().known(),
        Some(crate::sim::mission::MissionType::Guard)
    );
    assert_eq!(entity.health.current(), strength);
    assert_eq!(cell(&sim), start);
}

/// A damaged Grizzly's click over the player's depot sends the same Enter,
/// and the depot's production docking and Repair mission mend it.
#[test]
fn a_damaged_tank_sent_to_the_depot_is_repaired() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, depot, tank, _) = scene_with(rules, "GADEPT", "Americans");
    let strength = rules.object("MTNK").unwrap().strength;
    sim.entities_mut_for_test()
        .get_mut(tank)
        .unwrap()
        .health
        .set_for_test(strength / 2);
    // Each repair step is paid for.
    let owner = sim.interner().get("Americans").unwrap();
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .economy_mut_for_test()
        .add_credits(5000);
    assert_eq!(
        cursor(&sim, rules, &registry, tank, depot),
        Some(CursorId::Enter)
    );
    let mut selected = vec![tank];
    let orders = building_entry_orders(
        &sim,
        rules,
        &selected.clone(),
        &mut selected,
        &hover(depot),
        Some(&registry),
    )
    .expect("the tank enters");
    assert_eq!(orders, [enter(tank, depot)]);
    assert!(sim.apply_command("Americans", &orders[0], Some(rules)));
    for _ in 0..1800 {
        sim.advance_tick(&[], Some(rules), None, None, 67);
        if sim.entities().get(tank).unwrap().health.current() == strength {
            break;
        }
    }
    assert_eq!(sim.entities().get(tank).unwrap().health.current(), strength);
}

/// Each Grizzly over an empty Tank Bunker answers Enter on its own
/// What_Action, so both are sent in, as `Selection__DispatchMultiUnitOrder`
/// asks every receiver. Once one is installed, the bunker's CAN_LOAD
/// (`0x0043C506..0x0043C541`) refuses the other: no Enter row, no order.
#[test]
fn every_tank_is_sent_into_an_empty_bunker_until_one_is_installed() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, bunker, tank, _) = scene_with(rules, "NATBNK", "Americans");
    let second = sim
        .spawn_object("MTNK", "Americans", 13, 12, 0, rules)
        .expect("second tank");
    for actor in [tank, second] {
        assert_eq!(
            cursor(&sim, rules, &registry, actor, bunker),
            Some(CursorId::Enter)
        );
    }
    let mut selected = vec![tank, second];
    let orders = building_entry_orders(
        &sim,
        rules,
        &selected.clone(),
        &mut selected,
        &hover(bunker),
        Some(&registry),
    )
    .expect("both enter");
    assert_eq!(orders, [enter(tank, bunker), enter(second, bunker)]);
    assert!(sim.apply_command("Americans", &orders[0], Some(rules)));
    for _ in 0..1500 {
        sim.advance_tick(&[], Some(rules), None, None, 67);
        if sim.entities().get(bunker).unwrap().bunker.occupant() == Some(tank) {
            break;
        }
    }
    assert_eq!(
        sim.entities().get(bunker).unwrap().bunker.occupant(),
        Some(tank)
    );
    assert_eq!(
        building_entry_action(&sim, rules, second, bunker, Some(&registry)),
        None
    );
    assert_ne!(
        cursor(&sim, rules, &registry, second, bunker),
        Some(CursorId::Enter)
    );
}
