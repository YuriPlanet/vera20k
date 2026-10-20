//! A building's sale as `BuildingClass::DrawIt @ 0x0043D290` draws it, read
//! through a retail GAWEAP's production sale.
use super::*;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::world::Simulation;

/// A sale drops `ZShapePointMove=` for the whole Selling mission
/// (`0x0043D6DB..0x0043D712`) and hides the slot anims once the build-up runs
/// backwards in BState 0 (`0x0043D2C1..0x0043D306`), not before.
#[test]
fn a_sale_hides_slot_anims_in_its_build_down_and_drops_the_zshape_move() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    // Asset binding gives GAWEAP its Buildup control; only a type with one
    // sells.
    let mut rules = retail.rules.clone();
    rules.set_buildup_control_for_test("GAWEAP", [0, 25, 2]);
    let rules = &rules;
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 60);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let factory = sim
        .spawn_object("GAWEAP", "Americans", 10, 10, 0, rules)
        .expect("factory");
    let object = rules.object("GAWEAP").unwrap();
    let art = rules.art().resolve_metadata_entry("GAWEAP", &object.image);
    let sample = |sim: &Simulation| {
        sim.entities().get(factory).map(|building| {
            (
                building.building_down(),
                building.in_construction_bstate(),
                slot_anims_hidden_by_sale(building),
                building_zshape_point_move(building, art),
            )
        })
    };
    assert_eq!(sample(&sim), Some((false, false, false, (30, 15))));
    let order = CommandEnvelope::new(
        sim.entities().get(factory).unwrap().owner(),
        sim.session().tick() + 1,
        Command::SellBuilding { entity_id: factory },
    );
    sim.advance_tick(&[order], Some(rules), None, None, 67);
    let mut seen = Vec::new();
    while let Some(state) = sample(&sim)
        && seen.len() < 8
        && sim.session().tick() < 2000
    {
        if seen.last() != Some(&state) {
            seen.push(state);
        }
        sim.advance_tick(&[], Some(rules), None, None, 67);
    }
    assert_eq!(
        seen,
        [(true, false, false, (0, 0)), (true, true, true, (0, 0))]
    );
}
