//! The underground (`0x006F7E11..0x006F7E1E`) and HarvesterImmune
//! (`0x006F8009..0x006F8047`) candidate gates, through the production scan
//! owner against retail rules and the retail `[General] HarvesterUnit=` list.

use super::*;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::world::Simulation;

/// A guarding MTNK on the rival arena with one enemy candidate of
/// `target_type`, through the production spawn and scan owners.
fn guard_scene(rules: &RuleSet, target_type: &str) -> (Simulation, u64, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let tank = sim
        .spawn_object("MTNK", "Americans", 16, 16, 64, rules)
        .expect("scanner");
    let target = sim
        .spawn_object(target_type, "Russians", 18, 16, 64, rules)
        .expect("candidate");
    (sim, tank, target)
}

/// Scenario bit `0x800` (`0x006F8009..0x006F8047`): while set, a guarding
/// tank refuses an enemy harvester — retail `[General] HarvesterUnit=HARV`
/// is the list the production reader resolves — and still takes an enemy
/// tank; with the bit clear the harvester is an ordinary candidate again.
#[test]
fn retail_guard_scan_skips_a_harvester_while_scenario_bit_800_is_set() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    assert!(
        rules
            .harvester_unit_types
            .iter()
            .any(|type_id| type_id.eq_ignore_ascii_case("HARV")),
        "retail [General] HarvesterUnit= lists HARV"
    );

    let (mut sim, tank, harv) = guard_scene(rules, "HARV");
    let pick = |sim: &mut Simulation| {
        sim.greatest_threat_represented(rules, None, tank, ScanMission::Guard, None)
    };
    assert_eq!(
        pick(&mut sim),
        Some(harv),
        "bit clear: the harvester is an ordinary candidate"
    );
    sim.session.set_harvester_immune_for_test(true);
    assert_eq!(
        pick(&mut sim),
        None,
        "bit set: the listed HarvesterUnit type is refused"
    );

    // A candidate outside the list stays admissible while the bit is set.
    let (mut sim, tank, rhino) = guard_scene(rules, "HTNK");
    sim.session.set_harvester_immune_for_test(true);
    assert_eq!(
        sim.greatest_threat_represented(rules, None, tank, ScanMission::Guard, None),
        Some(rhino),
        "bit set: a non-harvester tank is not refused"
    );
}

/// The underground gate (`0x006F7E11..0x006F7E1E`): a candidate whose
/// GetHeight (vt+0x1C8, `0x005F5F40`) is strictly below -20 leptons — the
/// exact Z set through the owning Position field, flat arena ground — is
/// refused; one at exactly -20 is not. VERA writes no such height today (no
/// Tunnel locomotor), so the boundary is what the port preserves.
#[test]
fn guard_scan_refuses_a_candidate_strictly_below_minus_twenty_leptons() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let (mut sim, tank, target) = guard_scene(rules, "HTNK");
    let pick = |sim: &mut Simulation| {
        sim.greatest_threat_represented(rules, None, tank, ScanMission::Guard, None)
    };
    let set_height = |sim: &mut Simulation, z: i32| {
        sim.substrate
            .entities
            .get_mut(target)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(z));
    };
    set_height(&mut sim, -20);
    assert_eq!(
        pick(&mut sim),
        Some(target),
        "GetHeight -20 is not below the -20 cutoff"
    );
    set_height(&mut sim, -21);
    assert_eq!(
        pick(&mut sim),
        None,
        "GetHeight -21 is below the -20 cutoff: the candidate is underground"
    );
    set_height(&mut sim, -20);
    assert_eq!(
        pick(&mut sim),
        Some(target),
        "the gate reads the current height each scan"
    );
}
