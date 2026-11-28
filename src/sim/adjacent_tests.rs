//! Regressions for the Phobos `Adjacent.*` placement filters.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

fn scene(placer_lines: &str) -> (Simulation, RuleSet) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[Countries]\n0=Americans\n1=Russians\n\
         [BuildingTypes]\n0=PLACER\n1=POWER\n2=REFINERY\n\
         [PLACER]\nStrength=1000\nFoundation=1x1\nAdjacent=1\n{placer_lines}\
         [POWER]\nStrength=1000\nFoundation=1x1\nBaseNormal=yes\nAdjacent=1\n\
         [REFINERY]\nStrength=1000\nFoundation=1x1\nBaseNormal=yes\nAdjacent=1\n"
    ));
    let art = IniFile::from_str(
        "[PLACER]\nFoundation=1x1\n[POWER]\nFoundation=1x1\n[REFINERY]\nFoundation=1x1\n",
    );
    let rules = RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).expect("adjacent rules");
    (crate::sim::arena_fixture::rival_arena(&rules, 9), rules)
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, x: u16, y: u16) -> u64 {
    sim.spawn_object_at_height(kind, "Americans", x, y, 0, 0, rules)
        .expect("spawn")
}

fn place_ok(sim: &Simulation, rules: &RuleSet, x: u16, y: u16) -> bool {
    crate::sim::production::is_within_build_area(
        sim,
        rules,
        "Americans",
        rules.object("PLACER").unwrap(),
        x,
        y,
    )
}

/// The allow list keeps only the named provider types.
#[test]
fn adjacent_allowed_list_filters_providers() {
    let (mut sim, rules) = scene("Adjacent.Allowed=POWER\n");
    spawn(&mut sim, &rules, "REFINERY", 12, 10);
    assert!(!place_ok(&sim, &rules, 11, 10), "REFINERY is not allowed");
    let power = spawn(&mut sim, &rules, "POWER", 13, 10);
    assert!(
        place_ok(&sim, &rules, 11, 10),
        "POWER is an allowed provider"
    );
    sim.uninit_with_context(power, crate::sim::world::UninitContext::new(&rules));
    assert!(!place_ok(&sim, &rules, 11, 10));
}

/// The disallow list drops same-house providers instead of failing, unless
/// `Prohibit` makes one fatal.
#[test]
fn adjacent_disallowed_skips_or_prohibits_same_house_providers() {
    let (mut sim, rules) = scene("Adjacent.Disallowed=POWER\n");
    let power = spawn(&mut sim, &rules, "POWER", 12, 10);
    assert!(
        !place_ok(&sim, &rules, 11, 10),
        "the only provider, POWER, is skipped"
    );
    spawn(&mut sim, &rules, "REFINERY", 13, 10);
    assert!(place_ok(&sim, &rules, 11, 10), "REFINERY still provides");
    let (mut sim, rules) = scene("Adjacent.Disallowed=POWER\nAdjacent.Disallowed.Prohibit=yes\n");
    spawn(&mut sim, &rules, "POWER", 12, 10);
    spawn(&mut sim, &rules, "REFINERY", 13, 10);
    assert!(
        !place_ok(&sim, &rules, 11, 10),
        "a same-house POWER blocks the placement"
    );
    let (mut sim, rules) = scene("Adjacent.Disallowed=POWER\nAdjacent.Disallowed.Prohibit=yes\n");
    spawn(&mut sim, &rules, "REFINERY", 12, 10);
    assert!(place_ok(&sim, &rules, 11, 10));
}

/// `ProhibitDistance` scans a larger ring before the normal one and fails on
/// a same-house disallowed building there, even when an allowed provider is
/// nearer.
#[test]
fn adjacent_prohibit_distance_scans_the_larger_ring() {
    let (mut sim, rules) = scene(
        "Adjacent.Disallowed=POWER\nAdjacent.Disallowed.Prohibit=yes\n\
         Adjacent.Disallowed.ProhibitDistance=3\n",
    );
    let power = spawn(&mut sim, &rules, "POWER", 13, 10);
    spawn(&mut sim, &rules, "REFINERY", 12, 10);
    assert!(
        !place_ok(&sim, &rules, 11, 10),
        "POWER three cells away is inside the prohibit ring"
    );
    sim.uninit_with_context(power, crate::sim::world::UninitContext::new(&rules));
    assert!(
        place_ok(&sim, &rules, 11, 10),
        "the nearer REFINERY admits once POWER is gone"
    );
}

/// The prohibit check is ring-local: a disallowed building outside the
/// scanned ring never blocks the placement.
#[test]
fn adjacent_prohibit_ignores_a_disallowed_building_outside_the_ring() {
    let (mut sim, rules) = scene(
        "Adjacent.Disallowed=POWER
Adjacent.Disallowed.Prohibit=yes
         Adjacent.Disallowed.ProhibitDistance=3
",
    );
    spawn(&mut sim, &rules, "POWER", 20, 10);
    spawn(&mut sim, &rules, "REFINERY", 12, 10);
    assert!(place_ok(&sim, &rules, 11, 10));
}
