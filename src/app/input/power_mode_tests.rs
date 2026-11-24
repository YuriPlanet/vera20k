//! Power mode's target test: DetermineAction's power arm (`0x00692815`) and
//! Ares 3.0's replacement (`1005C790`, hook `0x0069281E`).

use super::admits;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

fn scene(ares: bool) -> (Simulation, RuleSet) {
    let _way = crate::rules::ares_switch::read_ares_way(ares);
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[General]\nTogglePowerAllowed=yes\n\
         [BuildingTypes]\n0=DRAIN\n1=POWERED\n2=PLANT\n3=FIXED\n\
         [DRAIN]\nStrength=750\nPower=-50\n\
         [POWERED]\nStrength=750\nPowered=yes\n\
         [PLANT]\nStrength=750\nPower=100\n\
         [FIXED]\nStrength=750\nPower=-50\nTogglePower=no\n",
    ))
    .unwrap();
    assert_eq!(rules.ares(), ares);
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 7);
    let player = sim.interner().get("Americans").unwrap();
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(player));
    (sim, rules)
}

/// `CanTogglePower` (`0x0069286F..0x00692893`) and the owner test: a drain
/// or `Powered=`, with `TogglePower=`, on the player's own building.
#[test]
fn power_mode_takes_the_players_consumers_and_powered_buildings() {
    let (mut sim, rules) = scene(true);
    let mut admitted = |type_id: &str, owner: &str, x: u16| {
        let id = sim.spawn_object(type_id, owner, x, 8, 0, &rules).unwrap();
        admits(&sim, &rules, id)
    };
    assert!(admitted("DRAIN", "Americans", 4));
    assert!(admitted("POWERED", "Americans", 8));
    assert!(!admitted("PLANT", "Americans", 12), "no drain, not Powered");
    assert!(!admitted("FIXED", "Americans", 16), "TogglePower=no");
    assert!(!admitted("DRAIN", "Russians", 20), "another House's");
}

/// Ares' arm alone refuses a building under EMP (vt+0x37C).
#[test]
fn only_ares_refuses_a_building_under_emp() {
    for ares in [false, true] {
        let (mut sim, rules) = scene(ares);
        let id = sim
            .spawn_object("DRAIN", "Americans", 8, 8, 0, &rules)
            .unwrap();
        sim.entity_mut_for_test(id)
            .unwrap()
            .set_emp_remaining_for_test(30);
        assert_eq!(admits(&sim, &rules, id), !ares, "ares {ares}");
    }
}
