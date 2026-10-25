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
    *sim.scenario_rng_mut_for_test() = crate::sim::rng::SimRng::new(0);
    let hash = sim.state_hash();
    let waterlines = [(7, native_saved_waterline()), (8, i16::MIN)];
    let bytes = GameSnapshot::save_validated_with_presentation(
        &sim,
        1,
        2,
        "waterline",
        3,
        &waterlines,
        &[],
        None,
        None,
        None,
        None,
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
    let mut rules = load_fixture_rules();
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&IniFile::from_str(
        "[SPARK]\nRate=1\nEnd=8\n",
    ));
    art.bind_anim_frame_count_for_test("SPARK", 8);
    rules.replace_art_registry_for_test(art);
    let mut sim = load_fixture_simulation(true);
    let id = sim.substrate_mut_for_test().allocate_stable_id();
    let mut entity =
        crate::sim::game_entity::GameEntity::test_default(id, "TEST", "Americans", 0, 0);
    entity.set_type_ref_for_test(sim.intern("TEST"));
    entity.set_owner_for_test(sim.intern("Americans"));
    sim.entities_mut_for_test().insert(entity);
    let anim_type = sim.intern("SPARK");
    let coord = crate::sim::anim_class::AnimWorldCoord {
        x: 128,
        y: 128,
        z: 0,
    };
    let (rx, ry, sub_x, sub_y, level) = coord.to_cell_sub_z();
    let anim = sim
        .spawn_anim_at_world(
            &rules,
            crate::sim::components::AnimClassSpawnDescriptor::new(
                anim_type, rx, ry, sub_x, sub_y, level,
            ),
            coord,
        )
        .unwrap();
    *sim.scenario_rng_mut_for_test() = crate::sim::rng::SimRng::new(0);
    let waterline = native_saved_waterline();
    let bytes = GameSnapshot::save_validated_with_presentation(
        &sim,
        LOAD_FIXTURE_MAP_HASH,
        rules.simulation_config_hash(),
        "retained draw",
        1,
        &[(id, waterline), (u64::MAX, -7)],
        &[(anim, 1234), (u64::MAX, -7)],
        None,
        None,
        None,
        None,
    );
    let terrain = load_fixture_terrain();
    let registry = OverlayTypeRegistry::empty();
    let mut presentation = SinkingWaterlines::default();
    presentation.restore([(id, 7), (999, 8)]);
    let outgoing = presentation.saved();
    let mut lighting = crate::app::presentation::lighting::MatchLighting::default();
    lighting.restore_anim_intensities(vec![(anim, 555), (999, 8)]);
    let outgoing_lights = lighting.anim_intensities().clone();
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
    assert_eq!(*lighting.anim_intensities(), outgoing_lights);
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
    assert_eq!(prepared.anim_intensities, [(anim, 1234)]);
    assert_eq!(presentation.saved(), outgoing);
    assert_eq!(*lighting.anim_intensities(), outgoing_lights);
    let mut runtime = crate::sim::runtime::SimRuntime::from_simulation(sim);
    let committed = prepared.commit_into(&mut runtime);
    presentation.restore(committed.sinking_waterlines);
    lighting.restore_anim_intensities(committed.anim_intensities);
    assert_eq!(lighting.anim_intensities().get(&anim), Some(&1234));
    assert_eq!(lighting.anim_intensities().len(), 1);
    assert_eq!(presentation.saved(), [(id, waterline)]);
    // A later camera/height sample must use the saved positive waterline.
    assert_eq!(presentation.unit_draw(id, true, 5000), Some(waterline));
    assert_eq!(presentation.saved(), [(id, waterline)]);
    assert!(runtime.simulation.entities().contains(id));
}
