//! Sight moves at reveal events only: a mover keeps the footprint its last
//! `UpdateReveal @ 0x0070AF50` stored until its next cell arrival (Unit
//! `0x0073AC74`, Infantry `0x0051A8EB`), not as the frame pass sees it
//! cross into a new cell.

use super::*;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::pathfinding::PathGrid;

/// Retail rules on the flat arena, with a human Americans House.
fn retail_arena() -> Option<(Simulation, RuleSet, PathGrid)> {
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()?;
    let mut sim = Simulation::new();
    let grid = crate::sim::arena_fixture::flat_arena(&mut sim, &rules);
    let owner = sim.intern("Americans");
    sim.houses.insert(
        owner,
        crate::sim::house_state::HouseState::new(owner, 0, None, true, 0, 10),
    );
    Some((sim, rules, grid))
}

/// The cell source `id`'s own sight was last revealed around.
fn sight_origin(sim: &Simulation, id: u64) -> (u16, u16) {
    let owner = sim.substrate.entities.get(id).unwrap().owner();
    let admission = &sim.fog.sight_admissions[&(id, owner)];
    (admission.origin.0, admission.origin.1)
}

/// A tank and an infantryman sent three cells east keep their sight where
/// it was while they cross into each next cell, and take it there when they
/// arrive: once per cell, in path order.
#[test]
fn a_movers_sight_moves_at_each_cell_arrival() {
    for kind in ["MTNK", "E1"] {
        let Some((mut sim, rules, grid)) = retail_arena() else {
            return;
        };
        let id = sim
            .spawn_object(kind, "Americans", 10, 10, 0, &rules)
            .unwrap();
        let owner = sim.substrate.entities.get(id).unwrap().owner();
        let order = CommandEnvelope::new(
            owner,
            sim.session.tick + 1,
            Command::Move {
                entity_id: id,
                target_rx: 13,
                target_ry: 10,
            },
        );
        let _ = sim.advance_tick(&[order], Some(&rules), Some(&grid), None, 67);
        let mut origins = vec![sight_origin(&sim, id)];
        let mut trailed = false;
        for _ in 0..400 {
            let _ = sim.advance_tick(&[], Some(&rules), Some(&grid), None, 67);
            let entity = sim.substrate.entities.get(id).unwrap();
            let origin = sight_origin(&sim, id);
            trailed |= origin != (entity.position.rx(), entity.position.ry());
            if origins.last() != Some(&origin) {
                origins.push(origin);
            }
        }
        let entity = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            (entity.position.rx(), entity.position.ry()),
            (13, 10),
            "{kind}: precondition, it arrived"
        );
        assert!(trailed, "{kind}: its sight trails it into each next cell");
        assert_eq!(
            origins,
            [(10, 10), (11, 10), (12, 10), (13, 10)],
            "{kind}: one reveal per arrival"
        );
    }
}
