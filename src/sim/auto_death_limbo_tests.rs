//! Phobos `AutoDeath`'s limbo arm (`Ext/Techno/Body.Update.cpp:721-766`) and the
//! scenario-level `AutoDeathObjects` collection (`Ext/Scenario/Body.cpp:101-134`),
//! driven through the production owners.
//!
//! The collection's only admission is the Ares initial payload
//! (`Misc/Hooks.Ares.cpp:31-57`), which `FootClass::Put`/`BuildingClass::Place`
//! run from the ordinary spawn, so the fixtures here spawn real objects and let
//! the frame or [`Simulation::update_auto_death_objects_in_limbo`] do the work.
use crate::rules::ruleset::RuleSet;
use crate::sim::world::{FrameEffects, Simulation, UninitContext};

const RULES: &str = "\
[InfantryTypes]\n0=E1\n1=E1AD\n\
[VehicleTypes]\n0=TLIMBO\n1=TLIMBONO\n2=TRNS\n3=TRNSP\n4=TIFV1\n5=TIFV2\n\
[BuildingTypes]\n0=ABSORB\n\
[WeaponTypes]\n0=W0\n\
[E1]\nStrength=125\nSight=0\nSpeed=4\n\
[E1AD]\nStrength=125\nSight=0\nSpeed=4\n\
AutoDeath.Behavior=kill\nAutoDeath.AfterDelay=1\n\
[TLIMBO]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.AfterDelay=1\n\
[TLIMBONO]\nStrength=100\nSight=0\nSpeed=5\n\
AutoDeath.Behavior=kill\nAutoDeath.AfterDelay=1\nAutoDeath.AllowLimboed=no\n\
[TRNS]\nStrength=300\nSight=0\nSpeed=6\nPassengers=3\nWeaponCount=4\nGunner=yes\n\
InitialPayload.Types=TLIMBO,TIFV1,TIFV2\nInitialPayload.Nums=1\n\
[TRNSP]\nStrength=300\nSight=0\nSpeed=6\nPassengers=2\n\
[TIFV1]\nStrength=100\nSight=0\nSpeed=5\nIFVMode=1\n\
[TIFV2]\nStrength=100\nSight=0\nSpeed=5\nIFVMode=2\n\
[ABSORB]\nStrength=500\nSight=0\nInfantryAbsorb=yes\nPassengers=1\n\
InitialPayload.Types=E1AD\nInitialPayload.Nums=1\n\
[W0]\nDamage=1\nROF=10\nRange=5\nProjectile=SHOT\nWarhead=C4WH\n\
[Animations]\n[Warheads]\n0=C4WH\n\
[C4WH]\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n\
[CombatDamage]\nC4Warhead=C4WH\n\
[Projectiles]\n0=SHOT\n[SHOT]\nInviso=yes\n";

fn arena() -> (Simulation, RuleSet) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(RULES)).unwrap();
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::flat_ground(&mut sim, &rules);
    for name in ["Americans", "Russians"] {
        let id = sim.intern(name);
        sim.houses_mut_for_test()
            .register(crate::sim::house_state::HouseState::new(
                id, 0, None, true, 0, 0,
            ));
    }
    (sim, rules)
}

fn alive(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .is_some_and(|entity| entity.lifecycle.object_alive())
}

fn tick(sim: &mut Simulation, rules: &RuleSet, count: usize) {
    for _ in 0..count {
        sim.advance_tick(&[], Some(rules), None, None, 22);
    }
}

/// Spawn and register on the map the way the scenario loader does, so a later
/// limbo transition has a house count to leave.
fn place(sim: &mut Simulation, rules: &RuleSet, kind: &str, rx: u16, ry: u16) -> u64 {
    let id = sim
        .spawn_object(kind, "Americans", rx, ry, 0, rules)
        .expect("the fixture type spawns");
    sim.house_added_to_game(id, rules);
    id
}

fn cargo(sim: &Simulation, transport: u64) -> Vec<u64> {
    sim.substrate
        .entities
        .get(transport)
        .and_then(|entity| entity.passenger_role.cargo())
        .map(|cargo| cargo.passengers().to_vec())
        .unwrap_or_default()
}

fn limbo(sim: &mut Simulation, rules: &RuleSet, id: u64) {
    let _ = sim.techno_limbo_with_context(id, UninitContext::new(rules, None));
}

/// `Ext/Techno/Body.Update.cpp:142-153`: the type key is read before the limbo
/// gate, and a limboed object dies only when `AutoDeath.AllowLimboed` (per type,
/// else the `[CombatDamage]` global whose constructor default is `yes`) lets the
/// visit through. The sweep itself is `Ext/Scenario/Body.cpp:101-110`.
#[test]
fn a_limboed_auto_death_object_dies_only_when_allow_limboed_is_set() {
    let (mut sim, rules) = arena();
    let allowed = place(&mut sim, &rules, "TLIMBO", 10, 10);
    let refused = place(&mut sim, &rules, "TLIMBONO", 12, 10);
    for id in [allowed, refused] {
        limbo(&mut sim, &rules, id);
        sim.register_auto_death(id, &rules);
    }
    assert_eq!(sim.auto_death_objects, [allowed, refused]);
    // The first sweep starts the countdown; the second completes it.
    tick(&mut sim, &rules, 2);
    assert!(
        !alive(&sim, allowed),
        "the per-type default (the global yes) admits the limboed object"
    );
    assert!(
        alive(&sim, refused),
        "AllowLimboed=no refuses every limboed visit, so the clock never starts"
    );
    assert!(
        !sim.auto_death_objects.contains(&allowed),
        "the sweep drops an erased object from the collection"
    );
}

/// `:723-766` after an Ares initial payload put two limboed passengers aboard a
/// `Gunner=` transport: the arm registers the kill under the object's own house,
/// uninits it, then empties the gunner slot and hands it to the **last**
/// remaining passenger. The same arm's `Absorbed` step dirties the absorbing
/// building's house power (House `+0x5778`).
#[cfg(feature = "ares")]
#[test]
fn the_limbo_arm_settles_the_transporter_gunner_and_the_power_recheck() {
    let (mut sim, rules) = arena();
    let transport = place(&mut sim, &rules, "TRNS", 10, 10);
    let payload = cargo(&sim, transport);
    assert_eq!(payload.len(), 3, "InitialPayload fills the hold");
    // `passengers()` is list head first and each Ares payload passenger joins
    // at the head, so the first boarded `TLIMBO` is the list's last entry and
    // the two IFVs sit in front of it in reverse board order.
    let dead = payload[2];
    let (ifv1, ifv2) = (payload[1], payload[0]);
    assert!(
        sim.auto_death_objects.contains(&dead),
        "the Ares payload admission registers a keyed type only"
    );
    assert_eq!(
        sim.entities()
            .get(transport)
            .unwrap()
            .current_weapon_number(),
        0,
        "the first boarded passenger's IFVMode armed the slot at admission"
    );
    // The first sweep starts the countdown; the second completes it.
    sim.update_auto_death_objects_in_limbo(&rules, None, FrameEffects::default());
    sim.session.set_binary_frame_for_test(3);
    sim.update_auto_death_objects_in_limbo(&rules, None, FrameEffects::default());
    assert!(!alive(&sim, dead), "the limbo arm uninits the object");
    assert_eq!(
        cargo(&sim, transport),
        [ifv2, ifv1],
        "UnInit leaves the surviving cargo behind"
    );
    assert_eq!(
        sim.entities()
            .get(transport)
            .unwrap()
            .current_weapon_number(),
        1,
        "ReceiveGunner takes the cargo list's last remaining passenger (TIFV1), whose IFVMode is 1"
    );

    // `:739-743`: an absorbed passenger's extra power leaves with it. The
    // fixture's absorber has no `ExtraPower=`, so only the death dirties power.
    let (mut sim, rules) = arena();
    let absorber = place(&mut sim, &rules, "ABSORB", 16, 16);
    let absorbed = cargo(&sim, absorber);
    assert_eq!(absorbed.len(), 1, "the garrison holds the payload");
    let absorbed = absorbed[0];
    assert!(
        sim.entities()
            .get(absorbed)
            .expect("the absorbed passenger stays live")
            .house_tracking_latches()
            .is_absorbed(),
        "InfantryAbsorb=yes marks the payload Absorbed (+0x439)"
    );
    // One frame starts the countdown and lets the House update consume the
    // House's initial power dirty bit, so the death's own write is observable.
    tick(&mut sim, &rules, 1);
    let owner = sim.entities().get(absorbed).unwrap().owner();
    let recheck = |sim: &mut Simulation| {
        sim.power_states_mut_for_test()
            .get(&owner)
            .is_some_and(|state| state.power_recheck_for_test())
    };
    assert!(
        !recheck(&mut sim),
        "nothing has asked this house to reassess power yet"
    );
    sim.session.set_binary_frame_for_test(5);
    sim.update_auto_death_objects_in_limbo(&rules, None, FrameEffects::default());
    assert!(!alive(&sim, absorbed), "the absorbed passenger is killed");
    assert!(
        recheck(&mut sim),
        "Absorbed on a transporter dirties the transporter owner's power"
    );
}

/// `Ext/Scenario/Body.cpp:123-134`: only the Ares initial-payload entries
/// register. An ordinary transport boarding and a plain limbo transition — the
/// Chrono, sell and repair holds — leave the collection untouched, so those
/// objects get no limbo `AutoDeath` visit.
#[test]
fn an_ordinary_transported_passenger_is_never_registered() {
    let (mut sim, rules) = arena();
    let transport = place(&mut sim, &rules, "TRNSP", 10, 10);
    let passenger = place(&mut sim, &rules, "TLIMBO", 12, 10);
    assert!(
        crate::sim::passenger::board_passenger(
            &mut sim,
            &rules,
            passenger,
            transport,
            crate::sim::passenger::BoardingArm::UnitTransport,
            None,
            FrameEffects::default(),
        ),
        "the passenger boards the ordinary hold"
    );
    assert!(
        sim.auto_death_objects.is_empty(),
        "ordinary boarding never registers an AutoDeath object"
    );
    limbo(&mut sim, &rules, passenger);
    assert!(
        sim.auto_death_objects.is_empty(),
        "a limbo transition never registers either"
    );
    tick(&mut sim, &rules, 2);
    assert!(
        alive(&sim, passenger),
        "an unregistered limboed object gets no sweep and no countdown"
    );
}
