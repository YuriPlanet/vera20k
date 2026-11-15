//! VERA's Ten Superweapons (`SuperWeaponPerBuilding=`,
//! `docs/ten-superweapons.md`): each such building charges its own shot.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;

const OWNER: &str = "Americans";
const REVEAL: &str = "RevealSpecial";

fn rules() -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
        [BuildingTypes]\n0=SILO\n1=LAB\n\
        [SILO]\nStrength=1000\nSuperWeapon=RevealSpecial\nSuperWeaponPerBuilding=yes\n\
        [LAB]\nStrength=1000\nSuperWeapon=RevealSpecial\n\
        [SuperWeaponTypes]\n0=RevealSpecial\n\
        [RevealSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n",
    );
    let art = IniFile::from_str("[SILO]\nFoundation=1x1\n[LAB]\nFoundation=1x1\n");
    RuleSet::from_ini_with_fixed_art_for_test(&ini, &art).unwrap()
}

fn world(rules: &RuleSet) -> Simulation {
    let mut sim = Simulation::with_seed(23);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let owner = sim.interner.intern(OWNER);
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 2000, 10));
    construct_house_supers(&mut sim, rules, owner, false);
    sim.session.game_options_mut_for_test().super_weapons = true;
    sim.resolved_terrain = Some(crate::map::resolved_terrain::test_grid(32, 32, |x, y| {
        super::cell_receiver_tests::test_terrain_cell(x, y)
    }));
    sim.set_playfield_bounds_for_test(Some(super::cell_receiver_tests::test_playfield_bounds()));
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    sim
}

fn build(sim: &mut Simulation, rules: &RuleSet, name: &str, x: u16) {
    sim.spawn_object_at_height(name, OWNER, x, 10, 0, 0, rules)
        .unwrap();
    let owner = sim.interner.get(OWNER).unwrap();
    sim.recheck_house_tech_tree(owner, rules);
}

fn reveal(sim: &Simulation) -> &SuperWeaponInstance {
    &sim.super_weapons[&sim.interner.get(OWNER).unwrap()][&sim.interner.get(REVEAL).unwrap()]
}

/// One SuperClass::AI frame for the house's Supers at `frame`.
fn tick(sim: &mut Simulation, rules: &RuleSet, frame: u32) {
    sim.session.set_binary_frame_for_test(frame);
    let owner = sim.interner.get(OWNER).unwrap();
    tick_house_superweapon_instances(sim, rules, owner);
}

#[test]
fn a_second_silo_charges_its_own_shot_and_a_lab_without_the_key_does_not() {
    let rules = rules();
    let mut sim = world(&rules);
    build(&mut sim, &rules, "SILO", 10);
    build(&mut sim, &rules, "SILO", 12);
    build(&mut sim, &rules, "LAB", 14);
    let recharge = rules.super_weapon(REVEAL).unwrap().recharge_time_frames;
    assert!(reveal(&sim).is_active());
    assert_eq!(reveal(&sim).building_charges, [recharge]);
    for frame in 1..=recharge as u32 {
        tick(&mut sim, &rules, frame);
    }
    assert!(reveal(&sim).is_ready());
    assert_eq!(reveal(&sim).ready_shots(), 2);

    let owner = sim.interner.get(OWNER).unwrap();
    let id = sim.interner.get(REVEAL).unwrap();
    assert!(sim.fire_super_weapon(
        &rules,
        owner,
        id,
        (15, 15),
        None,
        FrameEffects::default(),
        false
    ));
    // The second silo's shot is next: ready on the following frame, while
    // that silo starts charging again.
    assert_eq!(reveal(&sim).building_charges, [recharge]);
    tick(&mut sim, &rules, recharge as u32 + 1);
    assert!(reveal(&sim).is_ready());
    assert_eq!(reveal(&sim).ready_shots(), 1);
}
