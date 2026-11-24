//! Phobos `TechnoClass_CanAutoTarget_AttackNoThreatBuildings` (`0x6F85CF`,
//! `Ext/Techno/Hooks.cpp:1848-1858`): a human scanner's weapon
//! `AttackNoThreatBuildings=`, else `[General] AutoTarget.NoThreatBuildings=`,
//! admits an enemy building that is unarmed or poses no threat.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::world::Simulation;

/// A human scanner with `Primary=GUN` and an unarmed enemy building that
/// authors a non-zero `ThreatPosed=` (the shape the retail Chronosphere and
/// Iron Curtain have): the building scores, but the stock gate refuses it for
/// want of a weapon.
fn scene(weapon_key: &str, general_key: &str) -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n0=SCAN\n\
         [BuildingTypes]\n0=BUNKER\n\
         [SCAN]\nStrength=300\nArmor=heavy\nPrimary=GUN\nSpeed=6\nSight=8\n\
         [BUNKER]\nStrength=500\nArmor=heavy\nThreatPosed=30\n\
         [GUN]\nDamage=10\nROF=20\nRange=8\nWarhead=WH\n{weapon_key}\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [General]\n{general_key}"
    )))
    .expect("no-threat building rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6F85_CF00);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let scan = sim
        .spawn_object("SCAN", "Americans", 10, 10, 0, &rules)
        .expect("scanner");
    let bunker = sim
        .spawn_object("BUNKER", "Russians", 12, 10, 0, &rules)
        .expect("unarmed candidate");
    crate::sim::arena_fixture::discover(&mut sim, bunker);
    (sim, rules, scan, bunker)
}

#[test]
fn a_human_scanner_needs_the_override_to_acquire_an_unarmed_building() {
    let pick = |sim: &mut Simulation, rules: &RuleSet, scan: u64| {
        sim.greatest_threat_represented(rules, None, scan, ScanMission::Guard, None)
            .and_then(crate::sim::combat::TargetKind::object_id)
    };
    // Stock: `[General] AutoTarget.NoThreatBuildings=no` keeps the reject.
    let (mut sim, rules, scan, _) = scene("", "");
    assert_eq!(pick(&mut sim, &rules, scan), None);
    // The weapon's own key beats the default global no.
    let (mut sim, rules, scan, bunker) = scene("AttackNoThreatBuildings=yes\n", "");
    assert_eq!(pick(&mut sim, &rules, scan), Some(bunker));
    // The `[General]` key admits it with no weapon key at all.
    let (mut sim, rules, scan, bunker) = scene("", "AutoTarget.NoThreatBuildings=yes\n");
    assert_eq!(pick(&mut sim, &rules, scan), Some(bunker));
    // An explicit `no` on the weapon beats a global yes, as the Get() order
    // at `:1854` requires.
    let (mut sim, rules, scan, _) = scene(
        "AttackNoThreatBuildings=no\n",
        "AutoTarget.NoThreatBuildings=yes\n",
    );
    assert_eq!(pick(&mut sim, &rules, scan), None);
}
