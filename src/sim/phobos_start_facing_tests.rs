//! Phobos `StartFacing` / `StartFacing.Random` through the production
//! Unlimbo (`0x6F6D9E`) and construction-finish (`0x449AFE`) facing writes.

use crate::map::entities::{EntityCategory, MapEntity};
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

fn scene(key: &str, general: &str) -> (Simulation, RuleSet, u64) {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(&format!(
        "[BuildingTypes]\n0=SB\n\
         [SB]\nName=StartFacing Box\nStrength=500\nArmor=concrete\nFoundation=1x1\n{key}\
         [General]\n{general}"
    )))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x0D_FA_CE);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(0);
    let building = sim
        .spawn_object("SB", "Americans", 5, 5, 0, &rules)
        .expect("building");
    (sim, rules, building)
}

fn facing(sim: &Simulation, id: u64) -> u16 {
    sim.substrate
        .entities
        .get(id)
        .expect("building")
        .body_facing_current(sim.session.binary_frame())
}

#[test]
fn the_type_start_facing_replaces_the_unlimbo_byte() {
    let (sim, _rules, building) = scene("StartFacing=64\n", "");
    assert_eq!(facing(&sim, building), 0x40 << 8, "east on a 256 circle");
}

#[test]
fn an_unset_type_takes_the_general_fallback() {
    let (sim, _rules, building) = scene("", "BuildingStartFacing=128\n");
    assert_eq!(facing(&sim, building), 0x80 << 8);

    // A type key overrides the general line.
    let (sim, _rules, building) = scene("StartFacing=32\n", "BuildingStartFacing=128\n");
    assert_eq!(facing(&sim, building), 0x20 << 8);
}

#[test]
fn random_mode_draws_once_and_keeps_the_value() {
    let (mut sim, rules, _) = scene("StartFacing.Random=yes\n", "");
    // The runtime constructor draws its own random word first, then the
    // start facing draws RandomRanged(0, 255).
    let mut expected = sim.scenario_rng.clone();
    expected.next_u32();
    let drawn = expected.next_range_i32_inclusive(0, 255);
    let building = sim
        .spawn_object("SB", "Americans", 6, 5, 0, &rules)
        .expect("building");
    assert_eq!(facing(&sim, building), u16::from(drawn as u8) << 8);

    // A later read keeps the cached value and draws nothing.
    let before = sim.scenario_rng.logical_state();
    assert_eq!(
        sim.phobos_building_start_facing(building, &rules),
        Some(drawn)
    );
    assert_eq!(
        sim.scenario_rng.logical_state(),
        before,
        "the per-building value is cached"
    );
}

#[test]
fn a_type_no_overrides_general_random_without_a_draw() {
    let (mut sim, rules, _) = scene(
        "StartFacing=48\nStartFacing.Random=no\n",
        "BuildingStartFacing.Random=yes\n",
    );
    let building = sim
        .spawn_object("SB", "Americans", 7, 5, 0, &rules)
        .expect("building");
    assert_eq!(facing(&sim, building), 0x30 << 8);
    let before = sim.scenario_rng.logical_state();
    assert_eq!(sim.phobos_building_start_facing(building, &rules), Some(48));
    assert_eq!(sim.scenario_rng.logical_state(), before, "no random draw");
}

#[test]
fn a_map_created_building_keeps_its_authored_facing() {
    let rules = RuleSet::from_ini(&crate::sim::arena_fixture::clear_ground_ini(
        "[BuildingTypes]\n0=SB\n\
         [SB]\nName=StartFacing Box\nStrength=500\nArmor=concrete\nFoundation=1x1\n\
         StartFacing=64\n",
    ))
    .expect("rules");
    let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x0D_FA_CF);
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.session.set_binary_frame_for_test(0);
    let entity = MapEntity {
        owner: "Americans".to_string(),
        type_id: "SB".to_string(),
        health: 256,
        cell_x: 5,
        cell_y: 5,
        facing: 0,
        category: EntityCategory::Structure,
        sub_cell: 0,
        veterancy: None,
        high: false,
        mission: None,
        recruitable_a: true,
        recruitable_b: true,
        group: -1,
        follows: None,
        structure_columns: Default::default(),
        structure_ai_sellable: false,
        structure_ai_repairable: false,
        structure_online: true,
        trigger_tag: None,
    };
    assert_eq!(sim.spawn_from_map(&[entity], Some(&rules)), 1);
    let building = sim
        .substrate
        .entities
        .keys_sorted()
        .first()
        .copied()
        .expect("map building");
    assert_eq!(
        facing(&sim, building),
        0,
        "map-created buildings skip the Unlimbo start facing"
    );
}
