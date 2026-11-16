//! Phobos `AffectsUnderground` (`0x4899DA`): the direct damage pass over
//! Underground-layer technos.

use crate::rules::ruleset::RuleSet;
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::world::Simulation;

fn rules_with(warhead_keys: &str) -> RuleSet {
    RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[InfantryTypes]\n[VehicleTypes]\n0=TRK\n[AircraftTypes]\n[BuildingTypes]\n\
         [Warheads]\n0=UWH\n\
         [TRK]\nStrength=200\nArmor=none\nSpeed=4\n\
         [UWH]\nCellSpread=2\nVerses=100%,100%,100%,100%,100%,100%,100%,100%,100%,100%,100%\n{warhead_keys}"
    )))
    .expect("rules")
}

fn scene(rules: &RuleSet) -> (Simulation, u64) {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x41);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session.set_binary_frame_for_test(0);
    let unit = sim
        .spawn_object("TRK", "Americans", 10, 10, 0, rules)
        .expect("unit");
    (sim, unit)
}

fn make_underground(sim: &mut Simulation, id: u64) {
    let entity = sim.substrate.entities.get_mut(id).expect("unit");
    let mut locomotor =
        LocomotorState::for_test_kind(crate::rules::locomotor_type::LocomotorKind::Drive);
    locomotor.set_layer_for_test(MovementLayer::Underground);
    entity.locomotor = Some(locomotor);
    entity.set_in_playfield(false);
    // An underground techno is not on the map: leave the surface occupancy
    // list (native `IsOnMap` false).
    sim.remove_entity_occupancy(id);
}

fn area(sim: &mut Simulation, rules: &RuleSet, unit: u64) {
    let warhead = rules.warhead("UWH").expect("warhead").clone();
    let warhead_ref = sim.interner.intern("UWH");
    let coords = {
        let target = sim.substrate.entities.get(unit).expect("unit");
        crate::sim::movement::ground_pose::object_get_coords(target, sim.resolved_terrain.as_ref())
    };
    let mut run = crate::sim::combat::world_receiver::ReceiverRun::default();
    let _ = crate::sim::combat::world_receiver::apply_area_damage_in_run_with_target(
        sim,
        &mut run,
        rules,
        None,
        crate::sim::projectile::ProjectileCoord::new(coords.x, coords.y, coords.z),
        40,
        &warhead,
        false,
        (crate::sim::combat::RAD_NO_ATTACKER, None, warhead_ref),
        None,
    );
}

#[test]
fn affects_underground_damages_an_underground_techno() {
    let rules = rules_with("AffectsUnderground=yes\n");
    let (mut sim, unit) = scene(&rules);
    make_underground(&mut sim, unit);
    area(&mut sim, &rules, unit);
    assert!(
        sim.substrate
            .entities
            .get(unit)
            .is_some_and(|entity| entity.health.current() < 200),
        "the underground techno takes the direct underground hit"
    );

    let rules = rules_with("");
    let (mut sim, unit) = scene(&rules);
    make_underground(&mut sim, unit);
    area(&mut sim, &rules, unit);
    assert_eq!(
        sim.substrate.entities.get(unit).unwrap().health.current(),
        200,
        "without the key the underground techno is untouched"
    );
}
