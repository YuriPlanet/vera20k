//! TechnoType::ReadINI71551E..715720 stores threat weights during each
//! admitted type read. Later General defaults do not rewrite a read type.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::combat::threat_range::ScanMission;

fn guard_pick(rules: &RuleSet) -> (Option<u64>, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 1);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let gi = sim
        .spawn_object("E1", "Americans", 10, 10, 64, rules)
        .expect("GI");
    let tank = sim
        .spawn_object("MTNK", "Russians", 12, 10, 64, rules)
        .expect("wounded Grizzly");
    let target = sim.substrate.entities.get_mut(tank).unwrap();
    target.health.set_for_test(target.health.current() / 2);
    sim.spawn_object("E1", "Russians", 12, 12, 64, rules)
        .expect("healthy enemy GI in the same scan ring");
    (
        sim.greatest_threat_represented(rules, None, gi, ScanMission::Guard, None),
        tank,
    )
}

#[test]
fn map_general_default_does_not_retarget_a_previously_read_gi() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let (pick, tank) = guard_pick(&retail.rules);
    assert_eq!(pick, Some(tank), "the GI prefers the wounded tank");
    let Some(patched) = crate::rules::retail_ini_fixture::retail_battle_rules_with_map_patch(
        &IniFile::from_str("[General]\nMyEffectivenessCoefficientDefault=600\n"),
    ) else {
        return;
    };
    assert_eq!(
        patched
            .rules
            .general
            .threat_coefficient_defaults()
            .my_effectiveness(),
        600.0,
    );
    let (pick, tank) = guard_pick(&patched.rules);
    assert_eq!(
        pick,
        Some(tank),
        "ReadINI kept E1's nonzero root coefficient despite the new General default",
    );
}
