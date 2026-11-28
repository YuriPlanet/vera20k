//! Regression tests for the Phobos `Passengers.SyncOwner` holds: the
//! AddPassenger board-time record, the ChangeHouse cargo sync, the
//! RemoveFirstPassenger exit revert and the dying-transport cargo revert.
//! Each drives a production path; the stock control keeps both keys unset.

use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{EntityDamageEvent, ReceiverCallFlags};
use crate::sim::house_state::HouseState;
use crate::sim::passenger::{BoardingArm, DepartureRoute, board_passenger, depart_cargo_head};
use crate::sim::world::{FrameEffects, Simulation};

const RULES: &str = "\
[Warheads]
0=KILLWH
[KILLWH]
Verses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%
[InfantryTypes]
0=E1
[VehicleTypes]
0=APCX
1=APCN
2=PLN
[Countries]
0=Americans
1=Russians
[E1]
Strength=125
Speed=4
Size=1
Locomotor={4A582744-9839-11D1-B709-00A024DDAFD1}
[APCX]
Strength=200
Passengers=3
SizeLimit=3
Passengers.SyncOwner=yes
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[APCN]
Strength=200
Passengers=3
SizeLimit=3
Passengers.SyncOwner=yes
Passengers.SyncOwner.RevertOnExit=no
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
[PLN]
Strength=200
Passengers=3
SizeLimit=3
Locomotor={4A582741-9839-11d1-B709-00A024DDAFD1}
";

fn sim_with_houses() -> Simulation {
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &placeholder_rules());
    for (name, side, human) in [
        ("Americans", 0, true),
        ("Russians", 1, false),
        ("Soviets", 1, false),
    ] {
        let id = sim.interner.intern(name);
        sim.houses
            .register(HouseState::new(id, side, None, human, 0, 10));
    }
    sim
}

/// A throwaway parse so [`sim_with_houses`] can lay its ground before the
/// real rules arrive; only the terrain shape is read.
fn placeholder_rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(
        "[VehicleTypes]\n0=APCX\n[APCX]\nStrength=200\n",
    ))
    .unwrap()
}

fn rules() -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(RULES)).unwrap()
}

fn spawn(sim: &mut Simulation, rules: &RuleSet, kind: &str, owner: &str, rx: u16, ry: u16) -> u64 {
    sim.spawn_object_at_height(kind, owner, rx, ry, 0, 0, rules)
        .unwrap_or_else(|| panic!("{kind} spawns"))
}

fn owner<'a>(sim: &'a Simulation, id: u64) -> &'a str {
    sim.interner
        .resolve(sim.entities().get(id).unwrap().owner())
}

fn board(sim: &mut Simulation, rules: &RuleSet, pax: u64, carrier: u64) {
    assert!(
        board_passenger(
            sim,
            rules,
            pax,
            carrier,
            BoardingArm::UnitTransport,
            None,
            &mut FrameEffects::default(),
        ),
        "the passenger boards"
    );
}

fn capture(sim: &mut Simulation, rules: &RuleSet, transport: u64) {
    let soviets = sim.intern("Soviets");
    sim.change_owner_with_rules(transport, soviets, rules, None, &mut FrameEffects::default());
}

/// Phobos `TechnoClass_EnterTransport @ 0x71067B` records the board-time
/// house (native boarding changes none), the
/// `ChangeHouse_Passenger_SyncOwner @ 0x701881` hook re-houses the cargo on
/// the transport's capture, and `FootClass_LeaveTransport @ 0x4DE722`
/// restores the record on exit; `.RevertOnExit=no` keeps the transport's
/// house instead (`Hooks.Transport.cpp:66-122`).
#[test]
fn synced_cargo_reverts_on_exit_and_no_revert_keeps_the_transport_house() {
    let rules = rules();
    // RevertOnExit=yes (the type key and the true global default): out as
    // boarded.
    let mut sim = sim_with_houses();
    let apc = spawn(&mut sim, &rules, "APCX", "Americans", 10, 10);
    let pax = spawn(&mut sim, &rules, "E1", "Russians", 14, 14);
    board(&mut sim, &rules, pax, apc);
    assert_eq!(
        owner(&sim, pax),
        "Russians",
        "native boarding records the house; it changes none"
    );
    capture(&mut sim, &rules, apc);
    assert_eq!(owner(&sim, pax), "Soviets", "the capture syncs the cargo");
    depart_cargo_head(
        &mut sim,
        &rules,
        apc,
        DepartureRoute::Vehicle,
        |_, _, _| Ok(()),
        &mut FrameEffects::default(),
    )
    .expect("the departure removes the head");
    assert_eq!(
        owner(&sim, pax),
        "Russians",
        "the exit restores the board-time house"
    );

    // RevertOnExit=no: the passenger keeps the transport's house.
    let mut sim = sim_with_houses();
    let apc = spawn(&mut sim, &rules, "APCN", "Americans", 10, 10);
    let pax = spawn(&mut sim, &rules, "E1", "Russians", 14, 14);
    board(&mut sim, &rules, pax, apc);
    capture(&mut sim, &rules, apc);
    assert_eq!(owner(&sim, pax), "Soviets", "the capture still syncs");
    depart_cargo_head(
        &mut sim,
        &rules,
        apc,
        DepartureRoute::Vehicle,
        |_, _, _| Ok(()),
        &mut FrameEffects::default(),
    )
    .expect("the departure removes the head");
    assert_eq!(
        owner(&sim, pax),
        "Soviets",
        "RevertOnExit=no keeps the transport's house"
    );
}

/// `TechnoClass_ChangeHouse_Passenger_SyncOwner @ 0x701881`: every cargo
/// member follows the transport's new owner, and a type without the key
/// leaves its cargo untouched (`Hooks.Transport.cpp:44-64`).
#[test]
fn a_house_change_syncs_every_passenger_and_stock_keeps_its_cargo() {
    let rules = rules();
    let mut sim = sim_with_houses();
    let apc = spawn(&mut sim, &rules, "APCX", "Americans", 10, 10);
    let first = spawn(&mut sim, &rules, "E1", "Russians", 14, 14);
    let second = spawn(&mut sim, &rules, "E1", "Russians", 15, 14);
    board(&mut sim, &rules, first, apc);
    board(&mut sim, &rules, second, apc);
    capture(&mut sim, &rules, apc);
    assert_eq!(owner(&sim, first), "Soviets", "the first member syncs");
    assert_eq!(owner(&sim, second), "Soviets", "the second member syncs");

    // The stock control: no key, no sync.
    let mut sim = sim_with_houses();
    let plain = spawn(&mut sim, &rules, "PLN", "Americans", 10, 10);
    let pax = spawn(&mut sim, &rules, "E1", "Russians", 14, 14);
    board(&mut sim, &rules, pax, plain);
    capture(&mut sim, &rules, plain);
    assert_eq!(
        owner(&sim, pax),
        "Russians",
        "without SyncOwner the capture leaves the cargo alone"
    );
}

/// `UnitClass_ReceiveDamage_Cargo_SyncOwner @ 0x737F80`: a killing hit's
/// passenger block runs the cargo revert at its head, before the stock
/// escape — the stepped-out passengers are alive on the transport's cell
/// under their board-time houses (`Hooks.Transport.cpp:125-149`). The Ares
/// survivor hook replaces the stock tail, so the fate assert is a
/// no-ares one; the revert itself runs before it either way.
#[test]
fn a_dying_transport_restores_the_recorded_house_before_the_escape() {
    let rules = rules();
    let mut sim = sim_with_houses();
    let apc = spawn(&mut sim, &rules, "APCX", "Americans", 10, 10);
    let attacker = spawn(&mut sim, &rules, "PLN", "Soviets", 14, 10);
    let pax = spawn(&mut sim, &rules, "E1", "Russians", 14, 14);
    board(&mut sim, &rules, pax, apc);
    capture(&mut sim, &rules, apc);
    assert_eq!(owner(&sim, pax), "Soviets");
    let warhead = sim.interner.intern("KILLWH");
    let house = sim.entities().get(attacker).map(|entity| entity.owner());
    let hit = EntityDamageEvent::direct_receiver(
        apc,
        100_000,
        0,
        attacker,
        house,
        warhead,
        ReceiverCallFlags {
            ignore_defenses: false,
            arg6: false,
        },
    );
    sim.commit_noncombat_aoe_hits(&rules, None, &[hit], &mut FrameEffects::default());
    assert!(sim.substrate.pending_delete.contains_object(&apc), "killed");
    let escaped = sim.entities().get(pax).expect("the passenger escapes");
    assert!(
        escaped.lifecycle.object_alive() && !escaped.lifecycle.in_limbo(),
        "the escape unlimboed the passenger"
    );
    assert_eq!(
        owner(&sim, pax),
        "Russians",
        "the escape runs under the restored board-time house"
    );
}
