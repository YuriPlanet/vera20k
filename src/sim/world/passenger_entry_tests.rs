//! The passenger rows of `What_Action_OnObject` on retail rules
//! ([`Simulation::entry_click_action`]): the Foot target's team and
//! locomotor gates (`0x0051EDC5..0x0051EE18`, `0x00740324..0x00740357`).

use super::*;
use crate::sim::world::{EntryBaseAction, EntryClickAction};

/// The player's IFV, GI and Grizzly on a flat skirmish arena.
fn scene(rules: &RuleSet) -> (Simulation, u64, u64, u64) {
    let mut sim = Simulation::new();
    for (name, side) in [("Americans", 0), ("Russians", 1)] {
        let owner = sim.intern(name);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                owner, side, None, true, 0, 10,
            ));
    }
    let house = sim.interner().get("Americans");
    sim.session_mut_for_test().set_current_house_for_test(house);
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    crate::sim::arena_fixture::flat_arena(&mut sim, rules);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    // Each house keeps a building, so the skirmish has no loser.
    sim.spawn_object("NAPOWR", "Russians", 3, 3, 0, rules)
        .expect("russian power plant");
    sim.spawn_object("GAPOWR", "Americans", 22, 22, 0, rules)
        .expect("american power plant");
    let ifv = sim
        .spawn_object("FV", "Americans", 17, 16, 0, rules)
        .expect("ifv");
    let gi = sim
        .spawn_object("E1", "Americans", 13, 19, 0, rules)
        .expect("gi");
    let tank = sim
        .spawn_object("MTNK", "Americans", 13, 16, 0, rules)
        .expect("tank");
    (sim, ifv, gi, tank)
}

/// A carrier in a team whose TeamType is not `Loadable=` (the constructor's
/// 0, which no retail TeamType changes) turns the player's infantry away
/// (`0x0051EDD8..0x0051EDED`). A Unit's rung has no team test.
#[test]
fn a_carrier_in_a_team_turns_infantry_away() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    let (mut sim, ifv, gi, tank) = scene(&rules);
    let action = |sim: &Simulation, actor| {
        sim.entry_click_action(actor, ifv, EntryBaseAction::Select, &rules)
    };
    assert_eq!(action(&sim, gi), Some(EntryClickAction::Enter));
    assert_eq!(action(&sim, tank), Some(EntryClickAction::NoEnter));
    crate::sim::team_script_vm::join_team_for_test(&mut sim, &[ifv], false, false);
    assert_eq!(action(&sim, gi), Some(EntryClickAction::NoEnter));
    assert_eq!(action(&sim, tank), Some(EntryClickAction::NoEnter));
}

/// A moving carrier (its locomotor's Is_Moving) turns every passenger away,
/// before CAN_LOAD is asked (`0x0051EDEF..0x0051EE11`,
/// `0x0074032E..0x00740350`); stopped, it takes the GI again.
#[test]
fn a_moving_carrier_turns_passengers_away() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_rulesmd_rules() else {
        return;
    };
    let (mut sim, ifv, gi, _) = scene(&rules);
    assert!(sim.apply_command(
        "Americans",
        &crate::sim::command::Command::Move {
            entity_id: ifv,
            target_rx: 27,
            target_ry: 16,
        },
        Some(&rules),
    ));
    let moving = |sim: &Simulation| {
        crate::sim::movement::motion_query::is_moving(sim.entities().get(ifv).unwrap())
            == Some(true)
    };
    for _ in 0..30 {
        if moving(&sim) {
            break;
        }
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert!(moving(&sim));
    assert_eq!(
        sim.entry_click_action(gi, ifv, EntryBaseAction::Select, &rules),
        Some(EntryClickAction::NoEnter)
    );
    for _ in 0..300 {
        if !moving(&sim) {
            break;
        }
        sim.advance_tick(&[], Some(&rules), None, None, 67);
    }
    assert_eq!(
        sim.entry_click_action(gi, ifv, EntryBaseAction::Select, &rules),
        Some(EntryClickAction::Enter)
    );
}
