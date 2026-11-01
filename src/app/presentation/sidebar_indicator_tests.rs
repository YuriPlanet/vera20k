//! Phobos's power-delta branch of the sidebar indicator hook
//! (`CreditsClass_GraphicLogic_HarvesterCounter @ 0x4A25E0`), driven through
//! the projection owner that resolves the indicators the sidebar text lane
//! draws.
use super::power_delta_indicator;
use crate::rules::ini_parser::IniFile;
use crate::rules::phobos_sidebar_indicators::{SidebarIndicatorsUi, SidebarTextAlign};
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;
use crate::sim::intern::InternedId;
use crate::sim::power_system::PowerState;
use crate::sim::world::Simulation;

fn rules(side_body: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[Countries]\n0=Alpha\n[Sides]\nNorth=Alpha\n[North]\n{side_body}"
    )))
    .expect("rules read")
}

fn ui(sidebar_body: &str) -> SidebarIndicatorsUi {
    SidebarIndicatorsUi::from_ini(
        &IniFile::from_str(&format!("[Sidebar]\n{sidebar_body}")),
        &|_| None,
    )
}

/// One house that owns `buildings` tracked buildings.
fn house(buildings: i32) -> (Simulation, InternedId) {
    let mut sim = Simulation::new();
    let owner = sim.intern("me");
    sim.houses_mut_for_test()
        .register(HouseState::new(owner, 0, None, true, 0, 0));
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("house")
        .tracking_mut_for_test()
        .set_buildings_for_test(buildings);
    (sim, owner)
}

/// `Hooks.UI.cpp:137`: `PowerDelta.Show`, `[Phobos] ShowPowerDelta` and a
/// non-empty House+68 each have to hold.
#[test]
fn the_indicator_needs_both_switches_and_one_owned_building() {
    let (sim, owner) = house(2);
    let rules = rules("");
    let on = ui("PowerDelta.Show=yes\n");

    assert!(
        power_delta_indicator(&sim, &rules, 0, owner, &ui(""), true, 100, 40).is_none(),
        "the uimd switch defaults to no"
    );
    assert!(
        power_delta_indicator(&sim, &rules, 0, owner, &on, false, 100, 40).is_none(),
        "[Phobos] ShowPowerDelta=no closes the branch"
    );
    let homeless = house(0);
    assert!(
        power_delta_indicator(&homeless.0, &rules, 0, homeless.1, &on, true, 100, 40).is_none(),
        "a house with no building draws nothing"
    );
    assert!(
        power_delta_indicator(&sim, &rules, 0, owner, &on, true, 100, 40).is_some(),
        "all three conjuncts open the branch"
    );
}

/// The text is `%ls%+d` over `PowerOutput - PowerDrain`, while the colour comes
/// from the drain-to-output ratio: strict `< ConditionYellow` for green and
/// Phobos' `(x - y) <= 0.001` for yellow.
#[test]
fn the_net_power_text_and_ratio_colour_follow_the_hook() {
    let (sim, owner) = house(2);
    let rules = rules("");
    let on = ui("PowerDelta.Show=yes\n");
    let resolved = |produced: i32, drained: i32| {
        let indicator = power_delta_indicator(&sim, &rules, 0, owner, &on, true, produced, drained)
            .expect("drawn");
        (indicator.text.clone(), indicator.rgb)
    };

    assert_eq!(resolved(100, 40), ("⚡+60".to_string(), [0, 255, 0]));
    // 0.8 is above the 0.75 yellow threshold and within the red one.
    assert_eq!(resolved(100, 80), ("⚡+20".to_string(), [255, 255, 0]));
    assert_eq!(resolved(100, 200), ("⚡-100".to_string(), [255, 0, 0]));
    // Exactly the yellow threshold is not below it, so it turns yellow.
    assert_eq!(resolved(100, 75), ("⚡+25".to_string(), [255, 255, 0]));
    // No output at all: any drain doubles the red threshold, and the empty base
    // keeps the yellow fallback rather than reading green.
    assert_eq!(resolved(0, 50), ("⚡-50".to_string(), [255, 0, 0]));
    assert_eq!(resolved(0, 0), ("⚡+0".to_string(), [255, 255, 0]));
}

/// A spy or force-shield outage replaces the whole string with
/// `PowerBlackoutLabel` in `ColorGrey` — no number is drawn.
#[test]
fn a_blackout_drops_the_number_and_greys_the_label() {
    let (mut sim, owner) = house(2);
    sim.power_states_mut_for_test()
        .insert(owner, PowerState::default());
    sim.power_states_mut_for_test()
        .get_mut(&owner)
        .expect("power state")
        .start_blackout_for_test(0, 10);
    let rules = rules("Sidebar.PowerDelta.ColorGrey=9,9,9\n");
    let indicator = power_delta_indicator(
        &sim,
        &rules,
        0,
        owner,
        &ui("PowerDelta.Show=yes\n"),
        true,
        100,
        40,
    )
    .expect("drawn");
    assert_eq!(indicator.text, "⚡❌", "the blackout label alone");
    assert_eq!(indicator.rgb, [9, 9, 9], "ColorGrey, not the ratio colour");
}

/// The side's `Offset` is added to the hook's own `-70, 2` anchor, and `Align`
/// reaches the draw flags.
#[test]
fn the_side_offset_and_alignment_move_the_anchor() {
    let (sim, owner) = house(2);
    let rules = rules("Sidebar.PowerDelta.Offset=-5,3\nSidebar.PowerDelta.Align=right\n");
    let indicator = power_delta_indicator(
        &sim,
        &rules,
        0,
        owner,
        &ui("PowerDelta.Show=yes\n"),
        true,
        60,
        60,
    )
    .expect("drawn");
    assert_eq!(indicator.anchor_x, -75);
    assert_eq!(indicator.anchor_y, 5);
    assert_eq!(indicator.align, SidebarTextAlign::Right);
    // A side outside `[Sides]` keeps the constructor defaults.
    let unknown = power_delta_indicator(
        &sim,
        &rules,
        9,
        owner,
        &ui("PowerDelta.Show=yes\n"),
        true,
        60,
        60,
    )
    .expect("drawn");
    assert_eq!((unknown.anchor_x, unknown.anchor_y), (-70, 2));
    assert_eq!(unknown.align, SidebarTextAlign::Left);
}

/// `0x4A25E0` runs for the player house only after the defeat check at
/// `Hooks.UI.cpp:104`, which closes all three indicators.
#[test]
fn a_defeated_player_draws_no_indicator() {
    let (mut sim, owner) = house(2);
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("house")
        .set_is_defeated_for_test(true);
    let rules = rules("");
    assert!(
        power_delta_indicator(
            &sim,
            &rules,
            0,
            owner,
            &ui("PowerDelta.Show=yes\n"),
            true,
            100,
            40
        )
        .is_none()
    );
}
