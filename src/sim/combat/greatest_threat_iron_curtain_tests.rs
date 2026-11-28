//! Phobos `TechnoClass_CanAutoTargetObject_IronCurtain` (`0x6F7CE2`,
//! `Ext/Techno/Hooks.Targeting.cpp:57-76`): a human scanner's weapon
//! override, else `[CombatDamage] AutoTarget.IronCurtained=`, decides whether
//! an iron-curtained candidate enters the scan.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::threat_range::ScanMission;
use crate::sim::world::Simulation;

fn scene(weapon_key: &str, global_no: bool) -> (Simulation, RuleSet, u64, u64) {
    use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
    use crate::sim::timer::CdTimer;
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[InfantryTypes]\n[BuildingTypes]\n[AircraftTypes]\n\
         [VehicleTypes]\n0=SCAN\n1=DECOY\n\
         [SCAN]\nStrength=300\nArmor=heavy\nPrimary=GUN\nSpeed=6\nSight=8\n\
         [DECOY]\nStrength=300\nArmor=heavy\n\
         [GUN]\nDamage=10\nROF=20\nRange=8\nWarhead=WH\n{weapon_key}\
         [WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
         [CombatDamage]\nAutoTarget.IronCurtained={}\n",
        if global_no { "no" } else { "yes" }
    )))
    .expect("curtain targeting rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x6F7C_E200);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    let scan = sim
        .spawn_object("SCAN", "Americans", 10, 10, 0, &rules)
        .expect("scanner");
    let decoy = sim
        .spawn_object("DECOY", "Russians", 12, 10, 0, &rules)
        .expect("curtained candidate");
    crate::sim::arena_fixture::discover(&mut sim, decoy);
    sim.substrate
        .entities
        .get_mut(decoy)
        .unwrap()
        .invulnerability = Some(InvulnerabilityState::new(
        CdTimer::started(0, 100),
        InvulnKind::IronCurtain,
    ));
    (sim, rules, scan, decoy)
}

#[test]
fn a_human_scanner_skips_a_curtained_candidate_per_weapon_and_global() {
    let pick = |sim: &mut Simulation, rules: &RuleSet, scan: u64| {
        sim.greatest_threat_represented(rules, None, scan, ScanMission::Guard, None)
            .and_then(crate::sim::combat::TargetKind::object_id)
    };
    // The default global yes keeps the stock acquisition.
    let (mut sim, rules, scan, decoy) = scene("", false);
    assert_eq!(pick(&mut sim, &rules, scan), Some(decoy));
    // The global no refuses the curtained candidate.
    let (mut sim, rules, scan, _) = scene("", true);
    assert_eq!(pick(&mut sim, &rules, scan), None);
    // A weapon yes beats the global no.
    let (mut sim, rules, scan, decoy) = scene("AutoTarget.IronCurtained=yes\n", true);
    assert_eq!(pick(&mut sim, &rules, scan), Some(decoy));
    // A weapon no beats the default global yes.
    let (mut sim, rules, scan, _) = scene("AutoTarget.IronCurtained=no\n", false);
    assert_eq!(pick(&mut sim, &rules, scan), None);
}
