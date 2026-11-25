//! Does the house's fog ever report an enemy in a running match?
//!
//! Every reading of a real match shows `seen=0`, on all seventeen of them, even in
//! the one scout run where units walked into enemy fire and died (`lost 0:0 ->
//! 5:0`). The accessor itself is tested in a fixture and works there, so the
//! question this test asks is narrower: with two houses in one arena and an enemy
//! standing inside a friendly unit's sight, after the simulation has run long
//! enough for the fog to refresh, does the projection report it?
//!
//! This is the fact the whole army side rests on: every targeting decision reads
//! `seen`, and if it is always zero in a match, none of that work can be judged.

use vera20k_core::rules::ruleset::RuleSet;
use vera20k_core::sim::control::observation::Observation;
use vera20k_core::sim::intern::InternedId;
use vera20k_core::sim::world::Simulation;

fn arena() -> Option<(Simulation, RuleSet)> {
    let retail = vera20k_core::rules::retail_ini_fixture::retail_battle_rules()?;
    let rules = retail.rules;
    let mut sim = vera20k_core::sim::arena_fixture::rival_arena(&rules, 0xA17);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    Some((sim, rules))
}

#[test]
fn the_fog_reports_an_enemy_standing_in_line_of_sight() {
    let Some((mut sim, rules)) = arena() else {
        return;
    };
    let americans: InternedId = sim.intern("Americans");
    // Ours and theirs, two cells apart: well inside a rifleman's five-cell sight.
    let ours = sim
        .spawn_object("E1", "Americans", 20, 20, 0, &rules)
        .expect("our rifleman");
    let theirs = sim
        .spawn_object("E1", "Russians", 22, 20, 0, &rules)
        .expect("their rifleman");
    assert!(ours > 0 && theirs > 0);

    // Give the fog time to refresh: sight is rebuilt as units are placed and as
    // the frame advances, and one frame is not enough for either.
    let mut visible_at = None;
    let mut cell_seen_at = None;
    for frame in 0..120 {
        let _ = sim.take_due_commands();
        sim.advance_tick(&[], Some(&rules), None, None, 33);
        let obs = Observation::new(&sim, americans);
        if cell_seen_at.is_none() && obs.is_cell_visible(22, 20) {
            cell_seen_at = Some(frame);
        }
        if obs
            .visible_enemy_entities()
            .any(|enemy| enemy.entity_id == theirs)
        {
            visible_at = Some(frame);
            break;
        }
    }
    assert!(
        cell_seen_at.is_some(),
        "the cell the enemy stands on is never visible to the house: the fog is \
         not refreshed, or the per-house grid never gets this owner's sight"
    );
    assert!(
        visible_at.is_some(),
        "the cell is visible but the enemy on it is not reported: the enemy list \
         filters on something other than sight"
    );
}
