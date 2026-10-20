//! The entry click chain on retail rules: CAN_LOAD (Grinder `0x0043C4F8`,
//! Bunker `0x0043C506`, UnitRepair `0x0043C544`, refinery, Unit transport
//! `0x0073758A`), the Unit building arm (`0x00740122..0x007401BC`), the
//! Infantry grinder row (`0x0051F0AF..0x0051F111`), both passenger rows
//! (`0x0051ED10..0x0051EE36`, `0x0074029F..0x00740378`), the Enter/NoEnter
//! cursor rows and Foot ClickedAction's missions Enter (case 3) and Eaten
//! (`0x004D774F`), then the production receivers.

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
        false,
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
    let orders = entry_orders(
        &sim,
        rules,
        &selected.clone(),
        &mut selected,
        &hover(grinder),
        OrderModifier::Normal,
        false,
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
            entry_action(
                &sim,
                rules,
                actor,
                grinder,
                OrderModifier::Normal,
                false,
                Some(&registry)
            ),
            None
        );
    }
    let mut selected = vec![tank, gi];
    assert_eq!(
        entry_orders(
            &sim,
            rules,
            &selected.clone(),
            &mut selected,
            &hover(grinder),
            OrderModifier::Normal,
            false,
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
        entry_orders(
            &sim,
            rules,
            &selected.clone(),
            &mut selected,
            &hover(grinder),
            OrderModifier::Normal,
            false,
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
    let orders = entry_orders(
        &sim,
        rules,
        &selected.clone(),
        &mut selected,
        &hover(refinery),
        OrderModifier::Normal,
        false,
        Some(&registry),
    )
    .expect("the miner enters");
    assert_eq!(orders, [enter(miner, refinery)]);
    assert!(selected.is_empty());

    assert_eq!(
        entry_action(
            &sim,
            rules,
            tank,
            refinery,
            OrderModifier::Normal,
            false,
            Some(&registry)
        ),
        None
    );
    let mut selected = vec![tank, miner];
    assert_eq!(
        entry_orders(
            &sim,
            rules,
            &selected.clone(),
            &mut selected,
            &hover(refinery),
            OrderModifier::Normal,
            false,
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
        entry_action(
            &sim,
            rules,
            gi,
            depot,
            OrderModifier::Normal,
            false,
            Some(&registry)
        ),
        None
    );
    let mut selected = vec![tank, gi];
    assert_eq!(
        entry_orders(
            &sim,
            rules,
            &selected.clone(),
            &mut selected,
            &hover(depot),
            OrderModifier::Normal,
            false,
            Some(&registry),
        ),
        None
    );
    let mut selected = vec![tank];
    let orders = entry_orders(
        &sim,
        rules,
        &selected.clone(),
        &mut selected,
        &hover(depot),
        OrderModifier::Normal,
        false,
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
    let orders = entry_orders(
        &sim,
        rules,
        &selected.clone(),
        &mut selected,
        &hover(depot),
        OrderModifier::Normal,
        false,
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
    let orders = entry_orders(
        &sim,
        rules,
        &selected.clone(),
        &mut selected,
        &hover(bunker),
        OrderModifier::Normal,
        false,
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
        entry_action(
            &sim,
            rules,
            second,
            bunker,
            OrderModifier::Normal,
            false,
            Some(&registry)
        ),
        None
    );
    assert_ne!(
        cursor(&sim, rules, &registry, second, bunker),
        Some(CursorId::Enter)
    );
}

/// The player's `carrier` beside the Grizzly and GI of [`scene_with`].
fn carrier_scene(rules: &RuleSet, carrier: &str) -> (Simulation, u64, u64, u64) {
    let (mut sim, _, tank, gi) = scene_with(rules, "GAPOWR", "Americans");
    let carrier = sim
        .spawn_object(carrier, "Americans", 16, 18, 0, rules)
        .unwrap_or_else(|| panic!("{carrier}"));
    (sim, carrier, tank, gi)
}

fn unit_hover(carrier: u64) -> HoverTargetKindWithId {
    HoverTargetKindWithId {
        stable_id: carrier,
        kind: HoverTargetKind::FriendlyUnit,
    }
}

fn carrier_cursor(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    actor: u64,
    carrier: u64,
    modifier: OrderModifier,
) -> Option<CursorId> {
    cursor_id_for_feedback(capability_cursor_for_hover(
        sim,
        &[actor],
        Some(actor),
        &unit_hover(carrier),
        Some(rules),
        Some(registry),
        modifier,
        modifier == OrderModifier::ForceMove,
        false,
    ))
}

fn carrier_orders(
    sim: &Simulation,
    rules: &RuleSet,
    registry: &crate::rules::overlay_types::OverlayTypeRegistry,
    selected: &[u64],
    carrier: u64,
    modifier: OrderModifier,
) -> Option<Vec<Command>> {
    let mut receivers = selected.to_vec();
    let orders = entry_orders(
        sim,
        rules,
        selected,
        &mut receivers,
        &unit_hover(carrier),
        modifier,
        modifier == OrderModifier::ForceMove,
        Some(registry),
    );
    if orders.is_some() {
        assert!(receivers.is_empty(), "every receiver answers an entry");
    }
    orders
}

/// Advances until `passenger` is inside `carrier`, at most `frames` frames.
fn board(sim: &mut Simulation, rules: &RuleSet, passenger: u64, carrier: u64, frames: u32) {
    for _ in 0..frames {
        if sim
            .entities()
            .get(passenger)
            .is_some_and(|entity| entity.passenger_role.is_inside_transport())
        {
            break;
        }
        sim.advance_tick(&[], Some(rules), None, None, 67);
    }
    assert_eq!(
        sim.entities()
            .get(carrier)
            .unwrap()
            .passenger_role
            .passenger_count(),
        1
    );
}

/// A GI over the player's IFV gets Enter and boards it. A full IFV answers
/// CAN_LOAD NEGATORY (`0x0073758A..0x007376AA`), so the next GI gets NoEnter,
/// and his click orders nothing and selects nothing.
#[test]
fn a_gi_boards_the_players_ifv_until_it_is_full() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, ifv, _, gi) = carrier_scene(rules, "FV");
    assert_eq!(
        carrier_cursor(&sim, rules, &registry, gi, ifv, OrderModifier::Normal),
        Some(CursorId::Enter)
    );
    let orders = carrier_orders(&sim, rules, &registry, &[gi], ifv, OrderModifier::Normal)
        .expect("the GI enters");
    assert_eq!(orders, [enter(gi, ifv)]);
    assert!(sim.apply_command("Americans", &orders[0], Some(rules)));
    board(&mut sim, rules, gi, ifv, 200);

    let second = sim
        .spawn_object("E1", "Americans", 13, 20, 0, rules)
        .expect("second gi");
    assert_eq!(
        carrier_cursor(&sim, rules, &registry, second, ifv, OrderModifier::Normal),
        Some(CursorId::NoEnter)
    );
    assert_eq!(
        carrier_orders(
            &sim,
            rules,
            &registry,
            &[second],
            ifv,
            OrderModifier::Normal
        ),
        Some(Vec::new())
    );
}

/// A Grizzly over the player's IFV: its rung asks CAN_LOAD, which a
/// `SizeLimit=1` IFV answers NEGATORY, so the cursor is NoEnter and the click
/// orders nothing (it no longer selects the IFV). Beside a GI, whichever owns
/// the cursor answers an entry, so the click dispatches and only the GI is
/// sent in. A Rocketeer (`BalloonHover=yes`) is turned away
/// (`0x0051EDAE..0x0051EDC3`).
#[test]
fn a_tank_or_a_rocketeer_over_the_players_ifv_gets_no_enter() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, ifv, tank, gi) = carrier_scene(rules, "FV");
    assert_eq!(
        carrier_cursor(&sim, rules, &registry, tank, ifv, OrderModifier::Normal),
        Some(CursorId::NoEnter)
    );
    assert_eq!(
        carrier_orders(&sim, rules, &registry, &[tank], ifv, OrderModifier::Normal),
        Some(Vec::new())
    );
    assert_eq!(
        carrier_orders(
            &sim,
            rules,
            &registry,
            &[tank, gi],
            ifv,
            OrderModifier::Normal
        ),
        Some(vec![enter(gi, ifv)])
    );
    let rocketeer = sim
        .spawn_object("JUMPJET", "Americans", 13, 21, 0, rules)
        .expect("rocketeer");
    assert_eq!(
        carrier_cursor(
            &sim,
            rules,
            &registry,
            rocketeer,
            ifv,
            OrderModifier::Normal
        ),
        Some(CursorId::NoEnter)
    );
}

/// A Grizzly over the player's Amphibious Transport (`SizeLimit=6`) gets
/// Enter and boards it.
#[test]
fn a_tank_boards_the_players_amphibious_transport() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, transport, tank, _) = carrier_scene(rules, "LCRF");
    assert_eq!(
        carrier_cursor(
            &sim,
            rules,
            &registry,
            tank,
            transport,
            OrderModifier::Normal
        ),
        Some(CursorId::Enter)
    );
    let orders = carrier_orders(
        &sim,
        rules,
        &registry,
        &[tank],
        transport,
        OrderModifier::Normal,
    )
    .expect("the tank enters");
    assert_eq!(orders, [enter(tank, transport)]);
    assert!(sim.apply_command("Americans", &orders[0], Some(rules)));
    board(&mut sim, rules, tank, transport, 300);
}

/// The Bio Reactor (`Passengers=5`, `InfantryAbsorb=yes`) takes a GI. A
/// Grizzly's building arm finds its CAN_LOAD refusing and leaves Select, then
/// its passenger rung asks again and answers NoEnter.
#[test]
fn the_bio_reactor_takes_a_gi_and_turns_a_tank_away() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (sim, reactor, tank, gi) = scene_with(rules, "YAPOWR", "Americans");
    assert_eq!(
        cursor(&sim, rules, &registry, gi, reactor),
        Some(CursorId::Enter)
    );
    assert_eq!(
        cursor(&sim, rules, &registry, tank, reactor),
        Some(CursorId::NoEnter)
    );
    let mut selected = vec![gi, tank];
    assert_eq!(
        entry_orders(
            &sim,
            rules,
            &selected.clone(),
            &mut selected,
            &hover(reactor),
            OrderModifier::Normal,
            false,
            Some(&registry),
        ),
        Some(vec![enter(gi, reactor)])
    );
}

/// Alt answers Move (`0x007001E9`), which the passenger rows still take: a
/// GI over the IFV gets Enter and is sent in, a Grizzly gets NoEnter.
#[test]
fn alt_still_boards_a_transport() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (sim, ifv, tank, gi) = carrier_scene(rules, "FV");
    assert_eq!(
        carrier_cursor(&sim, rules, &registry, gi, ifv, OrderModifier::ForceMove),
        Some(CursorId::Enter)
    );
    assert_eq!(
        carrier_cursor(&sim, rules, &registry, tank, ifv, OrderModifier::ForceMove),
        Some(CursorId::NoEnter)
    );
    assert_eq!(
        carrier_orders(&sim, rules, &registry, &[gi], ifv, OrderModifier::ForceMove),
        Some(vec![enter(gi, ifv)])
    );
}

/// A GI selected with the player's IFV, clicking the IFV. The IFV owns the
/// cursor (distance 0); its rung skips itself (`0x00740312`) and its 0 over
/// the selected IFV becomes NoMove (`0x007403EB`), an order, so the release
/// dispatches and the GI's own row sends him in.
#[test]
fn a_gi_selected_with_the_ifv_boards_it_when_the_ifv_is_clicked() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, ifv, _, gi) = carrier_scene(rules, "FV");
    for id in [gi, ifv] {
        sim.entities_mut_for_test()
            .get_mut(id)
            .unwrap()
            .selection
            .set_selected_for_test(true);
    }
    assert_eq!(cursor_owner(&sim, rules, &[gi, ifv], ifv), Some(ifv));
    assert_eq!(
        entry_action(
            &sim,
            rules,
            ifv,
            ifv,
            OrderModifier::Normal,
            false,
            Some(&registry)
        ),
        None
    );
    let mut selected = vec![gi, ifv];
    assert_eq!(
        entry_orders(
            &sim,
            rules,
            &[gi, ifv],
            &mut selected,
            &unit_hover(ifv),
            OrderModifier::Normal,
            false,
            Some(&registry),
        ),
        Some(vec![enter(gi, ifv)])
    );
    assert_eq!(selected, [ifv]);
}

/// Ctrl leaves the attack rung for an actor neither armed nor `Infiltrate=`
/// (`0x00700444..0x00700474`), whose tail answers Select, so an unarmed MCV's
/// Ctrl over the player's Amphibious Transport still asks the passenger rung
/// and enters. An armed GI's Ctrl answers Attack and skips it (retail
/// Engineers are armed too: `Primary=DefuseKit`).
#[test]
fn an_unarmed_mcvs_ctrl_over_a_transport_still_enters() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, transport, _, gi) = carrier_scene(rules, "LCRF");
    let mcv = sim
        .spawn_object("AMCV", "Americans", 10, 16, 0, rules)
        .expect("mcv");
    assert_eq!(
        carrier_cursor(
            &sim,
            rules,
            &registry,
            mcv,
            transport,
            OrderModifier::ForceFire
        ),
        Some(CursorId::Enter)
    );
    assert_eq!(
        carrier_orders(
            &sim,
            rules,
            &registry,
            &[mcv],
            transport,
            OrderModifier::ForceFire
        ),
        Some(vec![enter(mcv, transport)])
    );
    assert_eq!(
        entry_action(
            &sim,
            rules,
            gi,
            transport,
            OrderModifier::ForceFire,
            false,
            Some(&registry)
        ),
        None
    );
}

/// A player's deployed GI (Doing 27..30) answers NoMove for Alt's Move before
/// the passenger row (`0x0051EB81..0x0051EBD7`); a plain click still asks it.
#[test]
fn a_deployed_gis_alt_over_the_ifv_skips_the_row() {
    let Some((retail, registry)) = retail() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, ifv, _, gi) = carrier_scene(rules, "FV");
    assert!(sim.apply_command(
        "Americans",
        &Command::ToggleInfantryDeploy { entity_id: gi },
        Some(rules),
    ));
    for _ in 0..60 {
        if sim.entities().get(gi).unwrap().infantry_deploy_doing() {
            break;
        }
        sim.advance_tick(&[], Some(rules), None, None, 67);
    }
    assert!(sim.entities().get(gi).unwrap().infantry_deploy_doing());
    let action = |modifier| {
        entry_action(
            &sim,
            rules,
            gi,
            ifv,
            modifier,
            modifier == OrderModifier::ForceMove,
            Some(&registry),
        )
    };
    assert_eq!(action(OrderModifier::ForceMove), None);
    assert_eq!(
        action(OrderModifier::Normal),
        Some(crate::sim::world::EntryClickAction::Enter)
    );
}
