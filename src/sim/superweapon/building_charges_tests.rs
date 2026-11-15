//! VERA's Superweapon Limit (`SuperWeaponPerBuilding=`,
//! `docs/superweapon-limit.md`): each such building charges its own shot.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::house_state::HouseState;

const OWNER: &str = "Americans";
const REVEAL: &str = "RevealSpecial";

fn rules() -> RuleSet {
    let ini = crate::sim::arena_fixture::clear_ground_ini(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
        [BuildingTypes]\n0=SILO\n1=LAB\n2=NUKES\n\
        [SILO]\nStrength=1000\nSuperWeapon=RevealSpecial\nSuperWeaponPerBuilding=yes\n\
        [LAB]\nStrength=1000\nSuperWeapon=RevealSpecial\n\
        [NUKES]\nStrength=1000\nNukeSilo=yes\nSuperWeapon=NukeSpecial\nSuperWeaponPerBuilding=yes\n\
        [SuperWeaponTypes]\n0=RevealSpecial\n1=NukeSpecial\n\
        [RevealSpecial]\nType=PsychicReveal\nRechargeTime=1\nIsPowered=no\n\
        [NukeSpecial]\nType=MultiMissile\nRechargeTime=1\nIsPowered=no\n",
    );
    let art = IniFile::from_str(
        "[SILO]\nFoundation=1x1\n[LAB]\nFoundation=1x1\n[NUKES]\nFoundation=1x1\n",
    );
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

fn super_of<'a>(sim: &'a Simulation, name: &str) -> &'a SuperWeaponInstance {
    &sim.super_weapons[&sim.interner.get(OWNER).unwrap()][&sim.interner.get(name).unwrap()]
}

fn reveal(sim: &Simulation) -> &SuperWeaponInstance {
    super_of(sim, REVEAL)
}

fn fire(sim: &mut Simulation, rules: &RuleSet, name: &str, cell: (u16, u16)) -> bool {
    let owner = sim.interner.get(OWNER).unwrap();
    let id = sim.interner.get(name).unwrap();
    sim.fire_super_weapon(rules, owner, id, cell, None, FrameEffects::default(), false)
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

    assert!(fire(&mut sim, &rules, REVEAL, (15, 15)));
    // The second silo's shot is next: ready on the following frame, while
    // that silo starts charging again.
    assert_eq!(reveal(&sim).building_charges, [recharge]);
    tick(&mut sim, &rules, recharge as u32 + 1);
    assert!(reveal(&sim).is_ready());
    assert_eq!(reveal(&sim).ready_shots(), 1);
}

#[test]
fn a_reset_such_as_a_spy_costs_one_charged_shot_not_all() {
    let rules = rules();
    let mut sim = world(&rules);
    build(&mut sim, &rules, "SILO", 10);
    build(&mut sim, &rules, "SILO", 12);
    let recharge = rules.super_weapon(REVEAL).unwrap().recharge_time_frames;
    for frame in 1..=recharge as u32 {
        tick(&mut sim, &rules, frame);
    }
    let owner = sim.interner.get(OWNER).unwrap();
    reset_super(
        &sim.interner,
        &sim.session,
        &mut sim.substrate,
        &mut sim.sound_events,
        &mut sim.super_weapons,
        &rules,
        owner,
        REVEAL,
    );
    tick(&mut sim, &rules, recharge as u32 + 1);
    assert!(reveal(&sim).is_ready());
    assert_eq!(reveal(&sim).ready_shots(), 1);
}

#[test]
fn two_quick_nukes_leave_from_two_silos_each_with_its_own_target() {
    let rules = rules();
    let mut sim = world(&rules);
    build(&mut sim, &rules, "NUKES", 10);
    build(&mut sim, &rules, "NUKES", 12);
    let recharge = rules
        .super_weapon("NukeSpecial")
        .unwrap()
        .recharge_time_frames;
    for frame in 1..=recharge as u32 {
        tick(&mut sim, &rules, frame);
    }
    assert!(fire(&mut sim, &rules, "NukeSpecial", (15, 15)));
    tick(&mut sim, &rules, recharge as u32 + 1);
    assert!(super_of(&sim, "NukeSpecial").is_ready());
    assert!(fire(&mut sim, &rules, "NukeSpecial", (20, 20)));
    let owner = sim.interner.get(OWNER).unwrap();
    let mut targets: Vec<_> = sim
        .houses
        .get(&owner)
        .unwrap()
        .base_projection()
        .buildings()
        .iter()
        .filter_map(|&id| {
            sim.substrate
                .entities
                .get(id)?
                .mission_leaf
                .as_building()?
                .own_nuke_target()
        })
        .collect();
    targets.sort();
    assert_eq!(targets, [(15, 15), (20, 20)]);
}
