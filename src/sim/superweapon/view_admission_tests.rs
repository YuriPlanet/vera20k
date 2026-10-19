//! Sidebar view fire admission (`SuperWeaponView::can_fire`) against the
//! real grant/charge/hold owners. Native identity: `SuperClass::CanFire @
//! 0x006CC360` refuses a suspended Super (`+0x70`) before its charge
//! (`+0x6F`) is read, and `SelectClass::Action @ 0x006AAD00` reads exactly
//! that at `0x006AAEEA` before arming the sidebar's target mode.

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::house_state::HouseState;

/// NAMISL provides NukeSpecial (`IsPowered=yes`): the issue #1313 fixture.
fn nuke_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
         [BuildingTypes]\n0=NAMISL\n\
         [NAMISL]\nStrength=1000\nSuperWeapon=NukeSpecial\n\
         [SuperWeaponTypes]\n0=NukeSpecial\n\
         [NukeSpecial]\nType=MultiMissile\nRechargeTime=10\nIsPowered=yes\nShowTimer=yes\n",
    ))
    .unwrap()
}

fn nuke_scene() -> (Simulation, RuleSet, InternedId, InternedId) {
    let rules = nuke_rules();
    let mut sim = Simulation::with_seed(0);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    let type_id = sim.interner.intern("NukeSpecial");
    sim.super_weapons
        .entry(owner)
        .or_default()
        .insert(type_id, SuperWeaponInstance::new(type_id, owner, 0));
    sim.spawn_object("NAMISL", "Americans", 10, 10, 0, &rules)
        .unwrap();
    sim.power_states.entry(owner).or_default();
    (sim, rules, owner, type_id)
}

fn nuke_view(sim: &Simulation, rules: &RuleSet, owner: InternedId) -> SuperWeaponView {
    superweapon_views_for_owner(sim, rules, &owner)
        .into_iter()
        .find(|view| view.display_name == "NukeSpecial")
        .expect("granted NukeSpecial view")
}

#[test]
fn charged_super_held_by_low_power_stays_charged_but_cannot_fire() {
    let (mut sim, rules, owner, type_id) = nuke_scene();

    // House50B1D0 grant while powered starts the recharge.
    grant_provided_supers(&mut sim, &rules, owner);
    let view = nuke_view(&sim, &rules, owner);
    assert!(!view.is_ready, "the grant starts the recharge");
    assert!(view.is_online);
    assert!(!view.can_fire, "a charging Super cannot fire");

    // The House Super AI visit completes the charge (RechargeTime=10 is
    // minutes -> 9000 native frames).
    sim.session.set_binary_frame_for_test(9000);
    tick_house_superweapon_instances(&mut sim, &rules, owner);
    let view = nuke_view(&sim, &rules, owner);
    assert!(view.is_ready);
    assert!(view.is_online);
    assert!(view.can_fire, "charged and online admits firing");

    // The power edge holds the charged Super: the view keeps the charge
    // (and the timer/flash readers), drops online, and drops the fire
    // admission — 0x006CC360 reads the hold before the charge.
    sim.power_states.get_mut(&owner).unwrap().is_low_power = true;
    update_owned_supers(&mut sim, &rules, owner);
    assert!(sim.super_weapons[&owner][&type_id].is_suspended);
    let view = nuke_view(&sim, &rules, owner);
    assert!(view.is_ready, "the hold keeps the charge");
    assert!(!view.is_online);
    assert!(
        !view.can_fire,
        "a suspended Super is refused before its charge"
    );

    // Restoring power releases the hold; the admission returns.
    sim.power_states.get_mut(&owner).unwrap().is_low_power = false;
    update_owned_supers(&mut sim, &rules, owner);
    assert!(!sim.super_weapons[&owner][&type_id].is_suspended);
    assert!(nuke_view(&sim, &rules, owner).can_fire);
}
