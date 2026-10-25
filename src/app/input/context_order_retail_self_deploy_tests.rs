//! A lone selected Unit or Infantryman clicking itself on retail rules: Unit
//! What_Action's self-arm site tests (`0x0073FE95..0x00740115`) and Planning
//! Mode's refusal (`Planning__AllowsAction 0x00637DB0`) through the
//! production cursor and click helpers.

use super::*;
use crate::app::input::cursor::capability_cursor_for_hover;
use crate::app::types::CursorFeedbackKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// A skirmish arena where the player is the Americans.
fn arena(rules: &RuleSet) -> Simulation {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x24);
    let player = sim.interner().get("Americans");
    sim.session_mut_for_test()
        .set_current_house_for_test(player);
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::from_legacy_nonzero(true));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim
}

/// The cursor and the click order over `id` itself, `id` alone selected.
fn self_click(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    kind: HoverTargetKind,
    planning: bool,
) -> (CursorFeedbackKind, Option<Command>) {
    let hover = HoverTargetKindWithId {
        stable_id: id,
        kind,
    };
    let owner = sim.interner().get("Americans").unwrap();
    (
        capability_cursor_for_hover(
            sim,
            &[id],
            Some(id),
            &hover,
            Some(rules),
            None,
            OrderModifier::Normal,
            false,
            planning,
        ),
        self_click_command(sim, rules, None, owner, &[id], Some(&hover), planning),
    )
}

/// The player's MCV offers Deploy where its Construction Yard fits with the
/// MCV lifted, and NoDeploy with no order once a tank stands on the yard's
/// foundation (`0x0073FF9C..0x0073FFA6`).
#[test]
fn an_mcv_offers_no_deploy_where_its_yard_cannot_stand() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = arena(rules);
    let mcv = sim
        .spawn_object("AMCV", "Americans", 12, 12, 0, rules)
        .expect("mcv");
    assert_eq!(
        self_click(&sim, rules, mcv, HoverTargetKind::FriendlyUnit, false),
        (
            CursorFeedbackKind::Deploy,
            Some(Command::DeployMcv { entity_id: mcv })
        )
    );
    sim.spawn_object("HTNK", "Russians", 14, 14, 0, rules)
        .expect("tank on the foundation");
    assert_eq!(
        self_click(&sim, rules, mcv, HoverTargetKind::FriendlyUnit, false),
        (CursorFeedbackKind::NoDeploy, None)
    );
}

/// In Planning Mode a Unit's or Infantryman's every self action answers
/// NoDeploy and orders nothing, even a tank's, which otherwise has none; a
/// building's self action is unaffected.
#[test]
fn planning_mode_refuses_unit_and_infantry_self_actions() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = arena(rules);
    let mcv = sim
        .spawn_object("AMCV", "Americans", 12, 12, 0, rules)
        .expect("mcv");
    let gi = sim
        .spawn_object("E1", "Americans", 20, 12, 0, rules)
        .expect("gi");
    let tank = sim
        .spawn_object("MTNK", "Americans", 20, 20, 0, rules)
        .expect("tank");
    let barracks = sim
        .spawn_object("GAPILE", "Americans", 4, 20, 0, rules)
        .expect("barracks");
    for id in [mcv, gi, tank] {
        assert_ne!(
            self_click(&sim, rules, id, HoverTargetKind::FriendlyUnit, false).0,
            CursorFeedbackKind::NoDeploy
        );
        assert_eq!(
            self_click(&sim, rules, id, HoverTargetKind::FriendlyUnit, true),
            (CursorFeedbackKind::NoDeploy, None),
            "{id}"
        );
    }
    assert_eq!(
        self_click(
            &sim,
            rules,
            barracks,
            HoverTargetKind::FriendlyStructure,
            true
        ),
        self_click(
            &sim,
            rules,
            barracks,
            HoverTargetKind::FriendlyStructure,
            false
        )
    );
}

/// A unit in a tank bunker offers Deploy over itself (`0x00740023`,
/// `0x00740107`, then CanDeploySlashUnload), and the click sends the DEPLOY
/// that brings it out. Outside a bunker a tank has no self action, and an
/// empty transport answers NoDeploy (`0x0074003F`).
#[test]
fn a_bunkered_unit_offers_deploy_over_itself() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let fv = rules.object("FV").expect("[FV]");
    assert!(
        fv.bunkerable && fv.passengers > 0,
        "the IFV is a bunkerable transport"
    );
    let mut sim = arena(rules);
    let bunker = sim
        .spawn_object("NATBNK", "Americans", 6, 6, 0, rules)
        .expect("bunker");
    let tank = sim
        .spawn_object("MTNK", "Americans", 20, 20, 0, rules)
        .expect("tank");
    let ifv = sim
        .spawn_object("FV", "Americans", 24, 20, 0, rules)
        .expect("ifv");
    assert_eq!(
        self_click(&sim, rules, tank, HoverTargetKind::FriendlyUnit, false).1,
        None
    );
    assert_eq!(
        self_click(&sim, rules, ifv, HoverTargetKind::FriendlyUnit, false),
        (CursorFeedbackKind::NoDeploy, None)
    );
    for id in [tank, ifv] {
        sim.entities_mut_for_test()
            .get_mut(id)
            .unwrap()
            .bunker
            .set_bunker_link_for_test(crate::sim::docking::bunker_link::BunkerLink::Installed(
                bunker,
            ));
        assert_eq!(
            self_click(&sim, rules, id, HoverTargetKind::FriendlyUnit, false),
            (
                CursorFeedbackKind::Deploy,
                Some(Command::DeployMcv { entity_id: id })
            ),
            "{id}"
        );
    }
}

/// The player's lone selected Gap Generator offers Deploy over itself only
/// while its House's power surplus exceeds the type's extra drain
/// (`ExtraPower=-9000`, Building What_Action `0x004472B4..0x004472E1`);
/// the click sends the Unload its ClickedAction queues (`0x00443514`).
#[test]
fn a_gap_generator_offers_its_supercharge_only_above_the_extra_drain() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = arena(rules);
    let gap = sim
        .spawn_object("GAGAP", "Americans", 16, 26, 0, rules)
        .expect("gap generator");
    for x in [1, 6, 11, 16, 21] {
        sim.spawn_object("NANRCT", "Americans", x, 2, 0, rules)
            .expect("reactor");
    }
    let settle = |sim: &mut Simulation| {
        for _ in 0..4 {
            sim.advance_tick(&[], Some(rules), None, None, 33);
        }
    };
    settle(&mut sim);
    assert_eq!(
        self_click(&sim, rules, gap, HoverTargetKind::FriendlyStructure, false),
        (
            CursorFeedbackKind::Deploy,
            Some(Command::UnloadPassengers { transport_id: gap })
        )
    );
    // The retained totals (House+53A4/+53A8) the rung reads: 9100 - 100.
    let owner = sim.interner().get("Americans").unwrap();
    sim.power_states_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .set_total_output_for_test(9100);
    let (cursor, command) = self_click(&sim, rules, gap, HoverTargetKind::FriendlyStructure, false);
    assert_ne!(cursor, CursorFeedbackKind::Deploy);
    assert_eq!(command, None, "a 9000 surplus is not above the extra drain");
}
