//! Phobos `[Phobos] PrioritySelectionFiltering=` with `LowSelectionPriority=`
//! (`Misc/Selection.cpp:58-73`, `:92`, `Phobos.INI.cpp:93`): once the band
//! rectangle holds a selectable object that is *not* low priority, the low
//! priority candidates drop out of the selection.

use super::filter_low_selection_priority;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// Two unit types that differ only in `LowSelectionPriority=`.
fn rules() -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n[VehicleTypes]\n0=LOWP\n1=HIGHP\n[AircraftTypes]\n[BuildingTypes]\n\
         [LOWP]\nStrength=200\nArmor=none\nSpeed=4\nLowSelectionPriority=yes\n\
         [HIGHP]\nStrength=200\nArmor=none\nSpeed=4\n",
    ))
    .expect("rules")
}

fn scene(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x52);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let low = sim
        .spawn_object("LOWP", "Americans", 10, 10, 0, rules)
        .expect("low");
    let high = sim
        .spawn_object("HIGHP", "Americans", 12, 10, 0, rules)
        .expect("high");
    (sim, low, high)
}

/// With the switch on, the low-priority unit is skipped as soon as a
/// higher-priority unit shares the rectangle; with it off, both stay, and a
/// rectangle holding only low-priority units keeps them.
#[test]
fn priority_selection_filtering_drops_low_priority_units() {
    let rules = rules();
    let (sim, low, high) = scene(&rules);

    let both = filter_low_selection_priority(
        vec![low, high],
        sim.entities(),
        Some(&rules),
        Some(sim.interner()),
        true,
    );
    assert_eq!(both, [high], "the low-priority unit drops out");

    let unfiltered = filter_low_selection_priority(
        vec![low, high],
        sim.entities(),
        Some(&rules),
        Some(sim.interner()),
        false,
    );
    assert_eq!(unfiltered, [low, high], "the switch off keeps both");

    let only_low = filter_low_selection_priority(
        vec![low],
        sim.entities(),
        Some(&rules),
        Some(sim.interner()),
        true,
    );
    assert_eq!(only_low, [low], "a lone low-priority unit still selects");
}

/// A type the interner cannot resolve does not participate, the way
/// `TechnoTypeExt::TryFetch` leaves a non-techno object unfiltered.
#[test]
fn an_unresolved_type_is_never_treated_as_low_priority() {
    let rules = rules();
    let (sim, low, _) = scene(&rules);
    let unknown = 9999;
    let kept = filter_low_selection_priority(
        vec![unknown, low],
        sim.entities(),
        Some(&rules),
        Some(sim.interner()),
        true,
    );
    assert_eq!(kept, [unknown, low]);
}
