//! The `LandTargeting=2` candidate gate (`0x006F7D55..0x006F7D82`), through
//! the production Guard scan against retail rules: a Boomer (`[BSUB]
//! LandTargeting=2`) on its own picks only targets on Water.

use super::*;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::world::Simulation;

/// A Russian Boomer on the western sea of a 48-cell rival arena (columns
/// below 24 are Water), with the player's Grizzly on the land 14 cells east:
/// past the Cruise Launcher's `MinimumRange=8`, inside its `Range=20`.
fn coast_scene(rules: &RuleSet) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 1, 48);
    crate::sim::arena_fixture::flood(
        &mut sim,
        rules,
        48,
        (0..48).flat_map(|y| (0..24).map(move |x| (x, y))),
    );
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let boomer = sim
        .spawn_object("BSUB", "Russians", 18, 24, 64, rules)
        .expect("boomer on water");
    let tank = sim
        .spawn_object("MTNK", "Americans", 32, 24, 64, rules)
        .expect("tank on land");
    (sim, boomer, tank)
}

/// A guarding Boomer refuses the Grizzly on land and takes a Destroyer on
/// the water.
#[test]
fn a_guarding_boomer_takes_ships_but_not_land_targets() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    assert_eq!(rules.object("BSUB").unwrap().land_targeting, 2);
    let (mut sim, boomer, _) = coast_scene(rules);
    let pick = |sim: &mut Simulation| {
        sim.greatest_threat_represented(rules, None, boomer, ScanMission::Guard, None)
    };
    assert_eq!(pick(&mut sim), None, "the Grizzly stands on land");
    let destroyer = sim
        .spawn_object("DEST", "Americans", 12, 24, 64, rules)
        .expect("destroyer on water");
    assert_eq!(pick(&mut sim), Some(destroyer));
}
