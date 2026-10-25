use super::*;
use crate::assets::asset_manager::AssetManager;
use crate::map::resolved_terrain::{test_clear_cell, test_grid};
use crate::map::source::{
    load_map_by_name_or_path_with_assets, load_map_ini_by_name_or_path_with_assets,
    test_support::TestDirectory,
};
use crate::sim::house_state::{HouseOutcomeKind, HouseState};
use crate::sim::scenario_session::ScenarioGameMode;
use crate::sim::world::TickLane;

#[test]
fn digest_checked_loose_map_uses_the_enabled_second_trigger_section() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules_for_map("Fight.MAP")
    else {
        return;
    };
    let rules = retail.rules;
    let directory = TestDirectory::new("trigger-digest-history");
    // Original ReadCCFile (0x004741F0) sorts ten section pairs while reading
    // Digest, then Clear (0x005257C0) removes it without invalidating sorting.
    // This exact physical order leaves the second Triggers section selected
    // by LoadFromINIList (0x007275D0) and ReadINI (0x00727240).
    directory.write(
        "DigestHistory.yrm",
        b"[Waypoints]\n0=1001\n1=1002\n\
          [Map]\nTheater=TEMPERATE\nSize=0,0,2,1\nLocalSize=0,0,2,1\n\
          [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
          [Basic]\nNewINIFormat=4\n\
          [Triggers]\nROOT=Russians,<none>,Disabled first section,1,1,1,1,0\n\
          [Events]\nROOT=1,13,0,0\n\
          [Actions]\nROOT=1,67,0,0,0,0,0,0,A\n\
          [Tags]\nROOT_TAG=0,Announce victory,ROOT\n\
          [Triggers]\nROOT=Russians,<none>,Enabled second section,0,1,1,1,0\n\
          [Digest]\n1=Gd0KRJOkiq81XByt5c7SzeTeG/Q=\n",
    );
    let assets = AssetManager::from_loose_root_for_test(directory.path());
    let loaded =
        load_map_by_name_or_path_with_assets(directory.path(), "DigestHistory.yrm", &assets)
            .unwrap();
    let metadata =
        load_map_ini_by_name_or_path_with_assets(directory.path(), "DigestHistory.yrm", &assets)
            .unwrap();
    assert_eq!(
        metadata
            .ini
            .section_or_empty("Digest")
            .read_string("1", "", 64),
        "Gd0KRJOkiq81XByt5c7SzeTeG/Q="
    );
    assert!(loaded.map.ini.section("Digest").is_none());
    // A source-to-frame regression: real file/map readers and live Trigger
    // owners, with flat terrain and no starting-force placement dependency.
    let mut sim = Simulation::new();
    sim.install_session_mode(ScenarioGameMode::Skirmish);
    sim.publish_scenario_started();
    sim.session.set_map_width_for_test(32);
    sim.session.set_map_height_for_test(32);
    sim.install_resolved_terrain_for_new_map(test_grid(32, 32, test_clear_cell));
    let player = sim.interner.intern("Player");
    let country = sim.interner.intern("Russians");
    sim.register_house(HouseState::new(
        player,
        rules.country_side_index_raw("Russians").unwrap(),
        Some(country),
        true,
        0,
        10,
    ));
    sim.session.set_current_house_for_test(Some(player));
    sim.initialize_trigger_definitions(&loaded.map, &Default::default(), None, None);
    sim.install_authored_cell_trigger_tags(&loaded.map);
    sim.classify_map_triggers(&rules);

    let frame = sim
        .advance_app_frame(
            &[],
            &rules,
            None,
            67,
            TickLane::Ordinary,
            FrameEffects::default(),
        )
        .unwrap();
    assert_eq!(
        sim.ready_outcome_for_owner(player)
            .map(|outcome| outcome.kind),
        Some(HouseOutcomeKind::Victory)
    );
    assert_eq!(sim.trigger_runtime.action_trace, vec![(67, 0)]);
    assert!(!frame.tick.frame_committed);
}
