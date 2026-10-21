//! A warping building's translucency as `TechnoClass_DrawSHP @ 0x00705E00`
//! selects it, read through retail ART.
use super::observer_draw_context;
use crate::render::draw_state::DrawState;
use crate::sim::world::Simulation;

/// The warp arm (`0x00705E5B..0x00705EAE`) ORs 6 (75%) for a building whose
/// type's ART sets `DoubleThick=` (`BuildingTypeClass +0x16B1`), else 4
/// (50%). Retail ARTMD sets it for one BuildingType, NACLON.
#[test]
fn an_erased_double_thick_building_draws_at_seventy_five_percent() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let double_thick: Vec<&str> = rules
        .building_ids
        .iter()
        .filter(|id| {
            rules.object(id).is_some_and(|object| {
                rules
                    .art()
                    .resolve_metadata_entry(id, &object.image)
                    .is_some_and(|art| art.building_double_thick)
            })
        })
        .map(String::as_str)
        .collect();
    assert_eq!(double_thick, ["NACLON"]);

    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 60);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let vats = sim
        .spawn_object("NACLON", "Russians", 10, 10, 0, rules)
        .expect("cloning vats");
    let reactor = sim
        .spawn_object("NAPOWR", "Russians", 20, 10, 0, rules)
        .expect("tesla reactor");
    let viewer = sim.interner().get("Americans");
    let selector = |sim: &Simulation, id: u64| {
        let building = sim.entities().get(id).unwrap();
        let observer = observer_draw_context(sim, building, Some("Americans"), viewer, Some(rules));
        let decision = DrawState::for_entity(building, sim.session().binary_frame(), 0, observer);
        assert!(decision.visible);
        decision.state.native_selector_bits()
    };
    assert_eq!((selector(&sim, vats), selector(&sim, reactor)), (0, 0));
    for id in [vats, reactor] {
        sim.entities_mut_for_test()
            .get_mut(id)
            .unwrap()
            .set_temporal_for_test(crate::sim::temporal::TemporalState::warped_by_for_test(id));
    }
    assert_eq!((selector(&sim, vats), selector(&sim, reactor)), (6, 4));
}
