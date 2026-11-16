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
        (indicator.text.clone(), indicator.tint)
    };

    assert_eq!(resolved(100, 40), ("⚡+60".to_string(), [0.0, 1.0, 0.0]));
    // 0.8 is above the 0.75 yellow threshold and within the red one.
    assert_eq!(resolved(100, 80), ("⚡+20".to_string(), [1.0, 1.0, 0.0]));
    assert_eq!(resolved(100, 200), ("⚡-100".to_string(), [1.0, 0.0, 0.0]));
    // Exactly the yellow threshold is not below it, so it turns yellow.
    assert_eq!(resolved(100, 75), ("⚡+25".to_string(), [1.0, 1.0, 0.0]));
    // No output at all: any drain doubles the red threshold, and the empty base
    // keeps the yellow fallback rather than reading green.
    assert_eq!(resolved(0, 50), ("⚡-50".to_string(), [1.0, 0.0, 0.0]));
    assert_eq!(resolved(0, 0), ("⚡+0".to_string(), [1.0, 1.0, 0.0]));
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
    assert_eq!(
        indicator.tint,
        [9.0 / 255.0; 3],
        "ColorGrey, not the ratio colour"
    );
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

/// The harvester-counter branch of the same hook (`Hooks.UI.cpp:111-135`):
/// both switches and an undefeated player, `[label][working]/[total]`, the
/// colour from a strict *greater-than* on the working ratio (the reverse of the
/// power delta), and its own `+50` centre-anchored column.
#[test]
fn the_harvester_counter_opens_on_both_switches_and_formats_both_counts() {
    use super::harvester_counter_indicator;
    let (sim, owner) = house(2);
    let rules = rules("");
    let on = ui("HarvesterCounter.Show=yes\n");
    let tooltip = [1.0, 1.0, 1.0];
    let resolved = |active: i32, total: i32| {
        harvester_counter_indicator(&sim, &rules, 0, owner, &on, true, tooltip, active, total)
            .expect("drawn")
    };

    // The uimd switch defaults to no, and the client switch closes the branch.
    assert!(
        super::harvester_counter_indicator(&sim, &rules, 0, owner, &ui(""), true, tooltip, 1, 1)
            .is_none(),
        "HarvesterCounter.Show defaults to no"
    );
    assert!(
        super::harvester_counter_indicator(&sim, &rules, 0, owner, &on, false, tooltip, 1, 1)
            .is_none(),
        "[Phobos] ShowHarvesterCounter=no closes the branch"
    );

    let all_working = resolved(2, 2);
    assert_eq!(all_working.text, "⛏2/2");
    // 2/2 is 1.0, above the 0.99 default, and `ColorGreen` is a Nullable left
    // unset, so the source's `Drawing::TooltipColor` fallback carries it.
    assert_eq!(all_working.tint, tooltip);
    assert_eq!((all_working.anchor_x, all_working.anchor_y), (50, 2));
    assert_eq!(
        all_working.align,
        SidebarTextAlign::Center,
        "the harvester branch hardcodes the centre bit"
    );

    // Exactly the yellow threshold is not above it, so it turns yellow, and
    // exactly the red one is not above it either, so it turns red.
    assert_eq!(resolved(99, 100).tint, [1.0, 1.0, 0.0]);
    assert_eq!(resolved(60, 100).tint, [1.0, 1.0, 0.0]);
    assert_eq!(resolved(1, 2).tint, [1.0, 0.0, 0.0]);
    assert_eq!(resolved(2, 5).tint, [1.0, 0.0, 0.0]);

    // A house with no counted harvester at all answers 1.0, so it stays green.
    let none = resolved(0, 0);
    assert_eq!(none.text, "⛏0/0");
    assert_eq!(none.tint, tooltip);
}

/// The side's own count-format switches, offset and explicit green.
#[test]
fn the_harvester_counter_side_settings_shape_the_string_and_anchor() {
    use super::harvester_counter_indicator;
    let (sim, owner) = house(2);
    let on = ui("HarvesterCounter.Show=yes\n");
    let resolved = |side_body: &str, active: i32, total: i32| {
        harvester_counter_indicator(
            &sim,
            &rules(side_body),
            0,
            owner,
            &on,
            true,
            [0.0, 0.0, 0.0],
            active,
            total,
        )
        .expect("drawn")
    };

    // HideMaxValue wins over OnlyMaxValue, the way the format branch reads.
    assert_eq!(
        resolved(
            "Sidebar.HarvesterCounter.HideMaxValue=yes\nSidebar.HarvesterCounter.OnlyMaxValue=yes\n",
            1,
            3
        )
        .text,
        "⛏1"
    );
    assert_eq!(
        resolved("Sidebar.HarvesterCounter.OnlyMaxValue=yes\n", 1, 3).text,
        "⛏3"
    );

    let moved = resolved(
        "Sidebar.HarvesterCounter.Offset=20,-6\nSidebar.HarvesterCounter.ColorGreen=1,2,3\n",
        2,
        2,
    );
    assert_eq!((moved.anchor_x, moved.anchor_y), (70, -4));
    assert_eq!(moved.tint, [1.0 / 255.0, 2.0 / 255.0, 3.0 / 255.0]);
}

/// The hook returns before any indicator for a defeated player
/// (`Hooks.UI.cpp:104-105`).
#[test]
fn a_defeated_player_draws_no_harvester_counter() {
    use super::harvester_counter_indicator;
    let (mut sim, owner) = house(2);
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("house")
        .set_is_defeated_for_test(true);
    assert!(
        harvester_counter_indicator(
            &sim,
            &rules(""),
            0,
            owner,
            &ui("HarvesterCounter.Show=yes\n"),
            true,
            [0.0, 0.0, 0.0],
            2,
            2
        )
        .is_none()
    );
}

/// The weeds branch (`Hooks.UI.cpp:175-190`): two switches and no building
/// gate, the bare total with no label, one nullable colour falling back to the
/// tooltip tint, and the hardcoded centre bit on the shared `+50` column.
#[test]
fn the_weeds_counter_prints_the_bare_total_without_a_building_gate() {
    use super::weeds_counter_indicator;
    let (mut sim, owner) = house(0);
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("house")
        .economy_mut_for_test()
        .give_weed_for_test(7, 0, 100);
    let rules = rules("");
    let on = ui("WeedsCounter.Show=yes\n");
    let tooltip = [1.0, 1.0, 1.0];

    assert!(
        weeds_counter_indicator(&sim, &rules, 0, owner, &ui(""), true, tooltip).is_none(),
        "WeedsCounter.Show defaults to no"
    );
    assert!(
        weeds_counter_indicator(&sim, &rules, 0, owner, &on, false, tooltip).is_none(),
        "[Phobos] ShowWeedsCounter=no closes the branch"
    );

    let drawn = weeds_counter_indicator(&sim, &rules, 0, owner, &on, true, tooltip)
        .expect("a house with no tracked building still draws the weeds counter");
    assert_eq!(drawn.text, "7", "the branch prints L\"%d\" with no label");
    assert_eq!(
        drawn.tint, tooltip,
        "an unset Nullable colour falls back to Drawing::TooltipColor"
    );
    assert_eq!((drawn.anchor_x, drawn.anchor_y), (50, 2));
    assert_eq!(drawn.align, SidebarTextAlign::Center);
}

/// `Sidebar.WeedsCounter.Offset` and the single `Color`
/// (`Ext/Side/Body.cpp:39-40`, defaults `Body.h:66-67`).
#[test]
fn the_weeds_counter_side_settings_move_the_anchor_and_set_the_only_colour() {
    use super::weeds_counter_indicator;
    let (mut sim, owner) = house(0);
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("house")
        .economy_mut_for_test()
        .give_weed_for_test(3, 0, 100);
    let drawn = weeds_counter_indicator(
        &sim,
        &rules("Sidebar.WeedsCounter.Offset=10,-4\nSidebar.WeedsCounter.Color=255,128,0\n"),
        0,
        owner,
        &ui("WeedsCounter.Show=yes\n"),
        true,
        [1.0, 1.0, 1.0],
    )
    .expect("drawn");
    assert_eq!(drawn.text, "3");
    assert_eq!((drawn.anchor_x, drawn.anchor_y), (60, -2));
    assert_eq!(drawn.tint, [1.0, 128.0 / 255.0, 0.0]);
    assert_eq!(
        drawn.align,
        SidebarTextAlign::Center,
        "the branch has no alignment key, unlike the power delta"
    );
}

/// `Hooks.UI.cpp:104` returns before every branch, so a defeated player draws
/// no weeds counter either.
#[test]
fn a_defeated_player_draws_no_weeds_counter() {
    use super::weeds_counter_indicator;
    let (mut sim, owner) = house(0);
    sim.houses_mut_for_test()
        .get_mut(&owner)
        .expect("house")
        .set_is_defeated_for_test(true);
    assert!(
        weeds_counter_indicator(
            &sim,
            &rules(""),
            0,
            owner,
            &ui("WeedsCounter.Show=yes\n"),
            true,
            [1.0, 1.0, 1.0]
        )
        .is_none()
    );
}

/// `SidebarClass_DrawIt_ProducingProgress @ 0x6A6EB1`: the four strips start at
/// `(Sidebar_GDIPositions ? 26 : 20) + offset.X`, `197 + offset.Y`, one
/// `XOffset` (`? 29 : 32`) apart, and only an existing factory gets a strip.
/// VERA prints the completion as a percentage (no `tab0xpp.shp` in retail).
#[test]
fn the_producing_progress_strips_use_the_native_columns_and_switch() {
    use super::producing_progress_indicators;
    let on = ui("ProducingProgress.Show=yes\n");
    let settings = rules("Sidebar.ProducingProgress.Offset=12,-7\n");
    let side_settings = settings.side_producing_progress(0);
    let tint = [0.5, 0.25, 0.125];

    // 168-wide surface, so the lane's midpoint anchor is 84.
    let drawn = producing_progress_indicators(
        &on,
        &side_settings,
        [Some(27), None, Some(54), None],
        true,
        168,
        tint,
    );
    assert_eq!(drawn.len(), 2, "only existing factories draw a strip");
    assert_eq!(drawn[0].text, "50%");
    assert_eq!(drawn[0].anchor_x, 26 + 12 - 84);
    assert_eq!(drawn[0].anchor_y, 190);
    assert_eq!(drawn[0].tint, tint);
    assert_eq!(drawn[1].text, "100%");
    assert_eq!(drawn[1].anchor_x, 26 + 2 * 29 + 12 - 84);
    assert_eq!(drawn[1].anchor_y, 190);

    // Non-GDI positions use the wider 32 px pitch from the base of 20.
    let non_gdi = producing_progress_indicators(
        &on,
        &side_settings,
        [Some(0), Some(0), Some(0), Some(0)],
        false,
        168,
        tint,
    );
    assert_eq!(non_gdi.len(), 4);
    assert_eq!(non_gdi[3].anchor_x, 20 + 3 * 32 + 12 - 84);

    assert!(
        producing_progress_indicators(
            &ui(""),
            &side_settings,
            [Some(27), Some(27), Some(27), Some(27)],
            true,
            168,
            tint
        )
        .is_empty(),
        "ProducingProgress.Show defaults to no"
    );
}

/// The four columns follow `0x6A6EB1`'s factory chain through the production
/// registry: `i=0` accepts a combat-only house (BuildCat::DontCare), and `i=3`
/// is land vehicle, then naval, then aircraft.
#[test]
fn the_producing_progress_columns_map_the_native_factory_chain() {
    use super::producing_progress_columns;
    use crate::sim::production::ProductionCategory;

    // A fresh sim with one house factory in each listed category.
    let columns = |categories: &[ProductionCategory], set_progress: Option<u16>| {
        let mut sim = Simulation::new();
        let owner = sim.intern("me");
        let type_id = sim.intern("TEST");
        for (order, &category) in categories.iter().enumerate() {
            assert!(
                sim.production_mut_for_test()
                    .factories_mut_for_test()
                    .test_enqueue_kernel(owner, category, type_id, order as u64, 100,)
            );
        }
        if let Some(value) = set_progress {
            sim.production_mut_for_test()
                .factories_mut_for_test()
                .test_first_mut()
                .expect("one factory")
                .set_progress_for_test(value);
        }
        producing_progress_columns(&sim, owner)
    };

    let infantry = columns(&[ProductionCategory::Infantry], Some(27));
    assert_eq!(infantry[2], Some(27), "the infantry strip keeps its stage");
    assert_eq!(infantry[0], None);
    assert_eq!(infantry[3], None);

    let combat_only = columns(&[ProductionCategory::Defense], None);
    assert_eq!(combat_only[0], Some(0), "DontCare admits the combat slot");
    assert_eq!(combat_only[1], Some(0), "the combat column too");

    assert_eq!(
        columns(&[ProductionCategory::Ship], None)[3],
        Some(0),
        "no land-vehicle factory falls through to naval"
    );
    assert_eq!(
        columns(&[ProductionCategory::Aircraft], None)[3],
        Some(0),
        "and then to aircraft"
    );
}
