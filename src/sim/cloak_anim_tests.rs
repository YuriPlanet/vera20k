//! Phobos `CloakAnims` / `DecloakAnims` (`0x7037F7` / `0x703736`): the
//! transition edge and the type/global list fallback.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::cloak_disguise::CloakRuntime;
use crate::sim::world::Simulation;

fn rules_with(unit_keys: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n0=TRK\n[AircraftTypes]\n[BuildingTypes]\n\
         [TRK]\nStrength=200\nArmor=none\nSpeed=4\n{unit_keys}"
    )))
    .expect("rules")
}

fn scene(rules: &RuleSet) -> (Simulation, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x41);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session.set_binary_frame_for_test(0);
    let unit = sim
        .spawn_object("TRK", "Americans", 10, 10, 0, rules)
        .expect("unit");
    (sim, unit)
}

#[test]
fn cloak_transitions_record_the_phobos_anim_edge() {
    let rules = rules_with("CloakAnims=CA\nDecloakAnims=CD\n");
    let (mut sim, unit) = scene(&rules);
    let cloak = sim
        .substrate
        .entities
        .get_mut(unit)
        .unwrap()
        .cloak
        .get_or_insert_with(|| Box::new(CloakRuntime::new(0)));
    let start = cloak.start_cloaking(0, 1, false);
    assert!(start.transitioned);
    assert!(cloak.take_pending_anim().is_some());
    let stop = cloak.start_uncloaking(0, 1, 1, false);
    assert!(stop.transitioned);
    assert!(cloak.take_pending_anim().is_some());
    assert_eq!(
        rules.object("TRK").unwrap().phobos_cloak_anims,
        vec!["CA".to_string()]
    );
}

#[test]
fn cloak_anim_list_falls_back_to_the_audio_visual_global() {
    let mut ini = IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n0=TRK\n[AircraftTypes]\n[BuildingTypes]\n\
         [TRK]\nStrength=200\nArmor=none\nSpeed=4\n\
         [AudioVisual]\nCloakAnims=GA\nDecloakAnims=GD\n",
    );
    ini.merge(&IniFile::from_str("[Clear]\nBuildable=yes\n"));
    let rules = RuleSet::from_ini(&ini).expect("rules");
    assert_eq!(rules.phobos_globals.cloak_anims, vec!["GA".to_string()]);
    assert_eq!(rules.phobos_globals.decloak_anims, vec!["GD".to_string()]);
    assert!(rules.object("TRK").unwrap().phobos_cloak_anims.is_empty());
}
