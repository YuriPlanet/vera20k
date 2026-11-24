//! `Ammo.Shared=` / `Ammo.Shared.Group=` through the production entry of
//! `TechnoClass::StartReloading` (`0x006FB080`, Phobos hook `0x006FB086`).
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::world::Simulation;
use crate::sim::{ammo::TechnoAmmo, game_entity::GameEntity};

const TRUCK: &str = "Ammo=10\nOpenTopped=yes\nAmmo.Shared=yes";
const TANK: &str = "Ammo.Shared=yes";

/// `truck` and `tank` replace each type's extension lines; `TANKA` is shared
/// with group 1, `TANKB` shared with group 2.
fn rules(truck: &str, tank: &str) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=TRUCK\n1=TANK\n2=TANKA\n3=TANKB\n\n\
[TRUCK]\nStrength=500\nSpeed=6\nPassengers=4\nReload=50\n{truck}\n\n\
[TANK]\nStrength=100\nSpeed=5\nAmmo=5\n{tank}\n\n\
[TANKA]\nStrength=100\nSpeed=5\nAmmo=5\nAmmo.Shared=yes\nAmmo.Shared.Group=1\n\n\
[TANKB]\nStrength=100\nSpeed=5\nAmmo=5\nAmmo.Shared=yes\nAmmo.Shared.Group=2\n"
    )))
    .expect("shared-ammo rules")
}

fn entity(sim: &mut Simulation, id: u64, name: &str, rules: &RuleSet, current: i32) -> GameEntity {
    let type_ref = sim.intern(name);
    let mut entity = GameEntity::test_default(id, name, "Americans", 5, 5);
    entity.set_type_ref_for_test(type_ref);
    let mut ammo = TechnoAmmo::from_type(rules.object(name).unwrap());
    ammo.set_current_for_test(current);
    entity.install_ammo_for_test(Some(ammo));
    entity
}

/// A transport with the given boarded passengers.
fn arena(rules: &RuleSet, truck_current: i32, passengers: &[(u64, &str, i32)]) -> Simulation {
    let mut sim = Simulation::new();
    let mut truck = entity(&mut sim, 1, "TRUCK", rules, truck_current);
    truck.passenger_role = PassengerRole::transport(PassengerCargo::new(4, 0.0));
    sim.substrate.entities.insert(truck);
    let mut ids = Vec::new();
    for &(id, name, current) in passengers {
        let passenger = entity(&mut sim, id, name, rules, current);
        sim.substrate.entities.insert(passenger);
        ids.push(id);
    }
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .passenger_role
        .cargo_mut()
        .unwrap()
        .set_passengers_for_test(ids);
    sim
}

fn start(sim: &mut Simulation, rules: &RuleSet, now: i32) {
    crate::sim::shared_ammo::start_reloading(
        &mut sim.substrate.entities,
        &sim.interner,
        rules,
        1,
        now,
    );
}

fn current(sim: &Simulation, id: u64) -> i32 {
    sim.entities().get(id).unwrap().ammo().unwrap().current()
}

/// Phobos runs the hand-out at `StartReloading`'s entry, so a transport that
/// is full the moment it gives a round away still starts its reload clock.
#[test]
fn a_full_open_topped_transport_hands_out_before_it_starts_its_clock() {
    let rules = rules(TRUCK, TANK);
    let mut sim = arena(&rules, 10, &[(2, "TANK", 0)]);
    start(&mut sim, &rules, 100);
    assert_eq!(current(&sim, 1), 9, "the transport pays one round");
    assert_eq!(current(&sim, 2), 1, "the passenger receives it");
    let timer = sim.entities().get(1).unwrap().ammo_reload_timer();
    assert_eq!(timer.start_frame(), 100, "the clock body saw the reduced 9");
    assert_eq!(timer.remaining(100), 50);
}

/// The transport group selects its receivers; a full passenger is skipped and
/// the list head order bounds a low transport count.
#[test]
fn the_shared_group_and_each_passengers_cap_select_receivers() {
    let grouped = rules(
        "Ammo=10\nOpenTopped=yes\nAmmo.Shared=yes\nAmmo.Shared.Group=2",
        TANK,
    );
    let mut sim = arena(
        &grouped,
        3,
        &[(2, "TANKA", 0), (3, "TANKB", 0), (4, "TANK", 0)],
    );
    start(&mut sim, &grouped, 0);
    assert_eq!(
        (
            current(&sim, 1),
            current(&sim, 2),
            current(&sim, 3),
            current(&sim, 4)
        ),
        (2, 0, 1, 0),
        "only the group-2 passenger is fed"
    );

    // A transport without a group feeds every shared passenger in list order;
    // the third is already full.
    let all = rules(TRUCK, TANK);
    let mut sim = arena(&all, 3, &[(2, "TANK", 0), (3, "TANKA", 0), (4, "TANKB", 5)]);
    start(&mut sim, &all, 0);
    assert_eq!(
        (
            current(&sim, 1),
            current(&sim, 2),
            current(&sim, 3),
            current(&sim, 4)
        ),
        (1, 1, 1, 5)
    );

    // One round left: the head gets it, the next passenger does not.
    let mut sim = arena(&all, 1, &[(2, "TANK", 0), (3, "TANKA", 0)]);
    start(&mut sim, &all, 0);
    assert_eq!(
        (current(&sim, 1), current(&sim, 2), current(&sim, 3)),
        (0, 1, 0)
    );
}

/// Phobos requires the transport type to set `Ammo.Shared=yes`, to be
/// `OpenTopped` and to have a finite positive `Ammo=`.
#[test]
fn an_unshared_closed_or_unlimited_transport_hands_nothing_out() {
    for (truck, tank) in [
        ("Ammo=5\nOpenTopped=yes\nAmmo.Shared=no", TANK),
        ("Ammo=5\nOpenTopped=no\nAmmo.Shared=yes", TANK),
        ("Ammo=-1\nOpenTopped=yes\nAmmo.Shared=yes", TANK),
        (TRUCK, "Ammo.Shared=no"),
    ] {
        let rules = rules(truck, tank);
        let mut sim = arena(&rules, 5, &[(2, "TANK", 0)]);
        start(&mut sim, &rules, 0);
        assert_eq!(
            (current(&sim, 1), current(&sim, 2)),
            (5, 0),
            "no hand-out for [{truck}] [{tank}]"
        );
    }
}
