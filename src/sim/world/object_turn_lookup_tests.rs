//! What an object's turn looks up in the entity store (VERA-only costs; the
//! turn's results are covered by the tests of each step).

use super::*;
use crate::sim::arena_fixture::{clear_ground_ini, rival_arena};

/// A tank and a power plant of one house on the flat arena.
fn scene() -> (Simulation, RuleSet, u64, u64) {
    let rules = RuleSet::from_ini(&clear_ground_ini(
        "[VehicleTypes]\n0=MTNK\n[BuildingTypes]\n0=GAPOWR\n\
         [MTNK]\nStrength=300\nSpeed=6\n[GAPOWR]\nStrength=750\n",
    ))
    .unwrap();
    let mut sim = rival_arena(&rules, 0x0B1);
    let tank = sim
        .spawn_object("MTNK", "Americans", 10, 10, 0, &rules)
        .expect("tank");
    let plant = sim
        .spawn_object("GAPOWR", "Americans", 16, 16, 0, &rules)
        .expect("power plant");
    sim.resolve_type_handles(&rules);
    (sim, rules, tank, plant)
}

/// The store's lookups by id during `turn`, and those its id index answered.
fn lookups_during(sim: &mut Simulation, turn: impl FnOnce(&mut Simulation)) -> (u64, u64) {
    let before = sim.substrate.entities.lookups();
    turn(sim);
    let after = sim.substrate.entities.lookups();
    (after.0 - before.0, after.1 - before.1)
}

/// A turn looks its own object up by id through the store's focused slot:
/// the id index answers next to none of an idle tank's lookups of itself.
#[test]
fn a_turn_finds_its_own_object_without_the_id_index() {
    let (mut sim, rules, tank, _) = scene();
    let (lookups, hashed) = lookups_during(&mut sim, |sim| {
        sim.advance_live_object_turn(tank, &rules, techno_ai::ObjectAiCtx::default())
            .unwrap();
    });
    assert!(
        hashed <= 8,
        "the id index answered {hashed} of the turn's {lookups} lookups"
    );
}
