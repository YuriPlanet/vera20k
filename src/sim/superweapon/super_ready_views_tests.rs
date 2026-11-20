//! `super_ready_views` is the sole data source of the grouped timer panel.
//! Only Supers with a timer place (`SW.ShowTimer=yes` and not a multiplayer-
//! passive house) may reach it, matching the native timer list's gate.

use super::*;
use crate::sim::house_state::HouseState;

fn scene() -> (Simulation, InternedId) {
    let mut sim = Simulation::with_seed(0);
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 0, 10));
    (sim, owner)
}

fn grant(sim: &mut Simulation, owner: InternedId, name: &str, timer_place: Option<u32>) {
    let type_id = sim.interner.intern(name);
    let mut instance = SuperWeaponInstance::new(type_id, owner, 0);
    instance.set_is_active_for_test(true);
    instance.set_timer_place_for_test(timer_place);
    sim.super_weapons
        .map_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(type_id, instance);
}

#[test]
fn only_timers_with_a_show_timer_place_are_admitted() {
    let (mut sim, owner) = scene();
    grant(&mut sim, owner, "Nuke", Some(1));
    grant(&mut sim, owner, "SpySat", None); // SW.ShowTimer=no
    grant(&mut sim, owner, "ParaDrop", None);
    grant(&mut sim, owner, "Chrono", Some(2));

    let views = super_ready_views(&sim);
    let admitted: Vec<_> = views
        .iter()
        .map(|view| sim.interner().resolve(view.sw_type()))
        .collect();
    assert_eq!(admitted, vec!["Nuke", "Chrono"]);
}

#[test]
fn inactive_supers_are_never_admitted_even_with_a_timer_place() {
    let (mut sim, owner) = scene();
    grant(&mut sim, owner, "Nuke", Some(1));
    let nuke = sim.interner.intern("Nuke");
    sim.super_weapons
        .map_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&nuke)
        .unwrap()
        .set_is_active_for_test(false);

    assert!(super_ready_views(&sim).is_empty());
}

#[test]
fn empty_super_store_yields_no_views() {
    let (sim, _) = scene();
    assert!(super_ready_views(&sim).is_empty());
}
