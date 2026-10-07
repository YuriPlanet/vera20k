//! Native +3CA survives raw Unit load, while its Rust owner stays outside sim.

use super::*;
use crate::render::sinking::SinkingWaterlines;

fn native_saved_waterline() -> i16 {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/naval_lifetime_controls.json",
    ))
    .unwrap();
    let loads = corpus["load"].as_array().unwrap();
    assert_eq!(loads.len(), 2);
    let row = loads[0]["waterline_after_noinit"].as_i64().unwrap();
    assert_eq!(loads[1]["waterline_after_noinit"].as_i64(), Some(row));
    i16::try_from(row).unwrap()
}

#[test]
fn sinking_waterline_envelope_preserves_native_short_without_changing_simulation() {
    let mut sim = load_fixture_simulation(true);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let hash = sim.state_hash();
    let waterlines = [(7, native_saved_waterline()), (8, i16::MIN)];
    let bytes = GameSnapshot::save_validated_with_sinking_waterlines(
        &sim,
        1,
        2,
        "waterline",
        3,
        &waterlines,
    );
    let loaded = GameSnapshot::load_validated(&bytes, 1, 2, LOAD_FIXTURE_MAP_NAME).unwrap();
    assert_eq!(loaded.sinking_waterlines, waterlines);
    assert_eq!(loaded.sim.state_hash(), hash);
    assert_eq!(
        GameSnapshot::read_header(&bytes).unwrap().description,
        "waterline"
    );

    let headless = GameSnapshot::save_validated(&sim, 1, 2, "headless", 3);
    let loaded = GameSnapshot::load_validated(&headless, 1, 2, LOAD_FIXTURE_MAP_NAME).unwrap();
    assert!(loaded.sinking_waterlines.is_empty());
    assert_eq!(loaded.sim.state_hash(), hash);
}

#[test]
fn sinking_waterline_load_replaces_presentation_only_after_commit() {
    let rules = load_fixture_rules();
    let mut sim = load_fixture_simulation(true);
    let id = sim.allocate_stable_id();
    let mut entity =
        crate::sim::game_entity::GameEntity::test_default(id, "TEST", "Americans", 0, 0);
    entity.type_ref = sim.interner.intern("TEST");
    entity.owner = sim.interner.intern("Americans");
    sim.substrate.entities.insert(entity);
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let waterline = native_saved_waterline();
    let bytes = GameSnapshot::save_validated_with_sinking_waterlines(
        &sim,
        LOAD_FIXTURE_MAP_HASH,
        rules.simulation_config_hash(),
        "retained draw",
        1,
        &[(id, waterline), (u64::MAX, -7)],
    );
    let terrain = load_fixture_terrain();
    let registry = OverlayTypeRegistry::empty();
    let mut presentation = SinkingWaterlines::default();
    presentation.restore([(id, 7), (999, 8)]);
    let outgoing = presentation.saved();
    let hash = sim.state_hash();
    let rng = sim.rng_state();
    // Fail after deserialization and cache rebuild. No presentation can be
    // released from this unsuccessful candidate, and the running world stays.
    assert!(matches!(
        PreparedLoad::prepare_candidate(
            &bytes,
            Some(&sim),
            Some(LOAD_FIXTURE_MAP_HASH),
            Some(&rules),
            Some(&terrain),
            None,
        ),
        Err(PreparedLoadError::MissingOverlayRegistry)
    ));
    assert_eq!(presentation.saved(), outgoing);
    assert_eq!(sim.state_hash(), hash);
    assert_eq!(sim.rng_state(), rng);

    let prepared = PreparedLoad::prepare_candidate(
        &bytes,
        Some(&sim),
        Some(LOAD_FIXTURE_MAP_HASH),
        Some(&rules),
        Some(&terrain),
        Some(&registry),
    )
    .unwrap();
    assert_eq!(prepared.sinking_waterlines, [(id, waterline)]);
    assert_eq!(presentation.saved(), outgoing);
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    let committed = prepared.commit_into(&mut runtime);
    presentation.restore(committed.sinking_waterlines);
    assert_eq!(presentation.saved(), [(id, waterline)]);
    // A later camera/height sample must use the saved positive waterline.
    assert_eq!(presentation.unit_draw(id, true, 5000), Some(waterline));
    assert_eq!(presentation.saved(), [(id, waterline)]);
    assert!(runtime.simulation.entities().contains(id));
}
