//! Ares radar jammers: JammerClass::Update10068500's cadence, eligibility and
//! range, UnjamAll on deactivation, and the jammed provider the radar scan
//! (hook508EBC) skips.

use super::*;
use crate::sim::power_system::{PowerState, assess_house_radar_projection};

fn scene() -> (Simulation, RuleSet, u64, u64, u64) {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[BuildingTypes]\n0=DOME\n[VehicleTypes]\n0=JAMMER\n\
         [DOME]\nStrength=500\nRadar=yes\nSpySat=no\n\
         [JAMMER]\nStrength=100\nSpeed=4\nRadarJamRadius=5\n\
         Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n",
    );
    let rules = RuleSet::from_ini(&ini).unwrap();
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 41);
    sim.intern_rule_type_ids(&rules);
    let near = sim
        .spawn_object("DOME", "Americans", 4, 4, 0, &rules)
        .unwrap();
    let far = sim
        .spawn_object("DOME", "Americans", 24, 24, 0, &rules)
        .unwrap();
    let jammer = sim
        .spawn_object("JAMMER", "Russians", 6, 6, 0, &rules)
        .unwrap();
    sim.resolve_type_handles(&rules);
    (sim, rules, near, far, jammer)
}

fn registered(sim: &Simulation, building: u64) -> Vec<u64> {
    sim.substrate
        .entities
        .get(building)
        .unwrap()
        .ares
        .radar_jam
        .registered_jammers()
        .to_vec()
}

fn americans_radar(sim: &Simulation, rules: &RuleSet, only: u64) -> bool {
    let owner = sim.interner.get("Americans").unwrap();
    let mut state = PowerState::default();
    assess_house_radar_projection(
        &mut state,
        &sim.substrate.entities,
        &[only],
        rules,
        owner,
        &sim.interner,
        false,
        sim.session.binary_frame(),
        false,
    );
    crate::sim::power_system::has_active_radar(&[(owner, state)].into_iter().collect(), owner)
}

#[test]
fn ares_jammer_registers_with_enemy_radar_in_range_and_unjams_when_deactivated() {
    let _ares = crate::rules::ares_switch::read_ares_way(true);
    let (mut sim, rules, near, far, jammer) = scene();
    assert!(americans_radar(&sim, &rules, near));
    let americans = sim.interner.get("Americans").unwrap();
    sim.power_states
        .map_mut_for_test()
        .insert(americans, PowerState::settled_for_test());
    let scan = |sim: &mut Simulation, frame: u32| {
        sim.session.set_binary_frame_for_test(frame);
        sim.update_ares_radar_jammer(jammer, &rules);
    };
    // LastScan starts at 0: no scan before frame 30.
    scan(&mut sim, 29);
    assert!(registered(&sim, near).is_empty());
    scan(&mut sim, 30);
    assert_eq!(registered(&sim, near), [jammer]);
    assert!(registered(&sim, far).is_empty(), "outside 5 * 256 leptons");
    assert!(
        sim.power_states
            .get(&americans)
            .unwrap()
            .radar_recheck_for_test(),
        "the first registration rechecks the owner's radar"
    );
    assert!(!americans_radar(&sim, &rules, near));

    sim.substrate
        .entities
        .get_mut(jammer)
        .unwrap()
        .deactivated
        .set_for_test(true);
    scan(&mut sim, 31);
    assert!(registered(&sim, near).is_empty());
    assert!(americans_radar(&sim, &rules, near));
}
