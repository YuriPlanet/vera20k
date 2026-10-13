//! Connected, GPU-free resource preparation through the production saved-load
//! owner. The small authored worlds use retail Rules/ART/Battle/theater assets;
//! their setup is a persistence fixture, not a second fresh-scenario loader or
//! a native whole-world startup comparison.

use super::*;
use crate::map::source::{LoadedMap, test_support::TestDirectory};
use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::{retail_assets, retail_rules_owner};
use crate::sim::rng::SimRng;
use crate::sim::snapshot::saved_local_view::{SavedLocalView, SavedLocalViewError};
use crate::sim::snapshot::scenario_resources::SavedScenarioResourcesRef;

const LOAD_CLOCK_MS: u64 = 16_000;
const SAVED_ELAPSED_BUCKETS: u32 = 180;
const MISSING_FOLLOW_ID: u64 = 0xBAD_F011;

fn map_text(theater: &str, strength: i32, size: u32) -> String {
    // This Pack5 record is the existing map/source.rs one-cell fixture. The
    // map owner supplies the remaining native Size diamond from retail Clear.
    format!(
        "[Map]\nTheater={theater}\nSize=0,0,{size},{size}\n\
         LocalSize=0,0,{size},{size}\n\
         [IsoMapPack5]\n1=DwALABwBAAIA/////wAAABEAAA==\n\
         [Basic]\nName=Saved {theater} resource fixture\nNewINIFormat=4\n\
         [MTNK]\nStrength={strength}\n\
         [Waypoints]\n7=5005\n\
         [Houses]\n0=Americans\n[Americans]\nColor=Gold\nCountry=Americans\n\
         [Lighting]\nAmbient=0.7\nRed=0.6\nGreen=0.8\nBlue=0.9\nGround=0.125\n"
    )
}

fn bound_saved_world(
    loaded: LoadedMap,
    name: &str,
    assets: &mut AssetManager,
    owner: &mut NativeRulesProcessOwner,
    mode: &IniFile,
    cache: &mut TileVariantSelectorCache,
) -> (SimRuntime, SavedMapMetadata, String) {
    let map = loaded.map;
    let theater =
        theater::load_theater(assets, &map.header.theater).expect("retail saved-world theater");
    let (mut rules, processed, fixed_art, _) = owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(Some(mode)),
            &map.ini,
        )
        .expect("retail Rules/Battle plus authored map")
        .into_parts();
    init_helpers::bind_rules_art_metadata(
        &mut rules,
        &fixed_art,
        assets,
        theater.extension,
        &map.header.theater,
    );
    let overlay_registry =
        crate::rules::overlay_types::OverlayTypeRegistry::from_ini(&processed, Some(&fixed_art));
    let mut simulation = super::super::tests::load_fixture_simulation(false);
    let terrain = {
        let mut main_draw = || simulation.main_rng.next_u32();
        let mut selector = cache.begin_load(&mut main_draw);
        let mut fill_draw = |min, max| simulation.scenario_rng.next_range_u32_inclusive(min, max);
        ResolvedTerrainGrid::build_with_variant_selector(
            &map,
            Some(&theater),
            Some(assets),
            Some(&rules.terrain_rules),
            Some(&overlay_registry),
            Some(&rules.terrain_object_types),
            true,
            rules.general.cliff_back_impassability,
            &mut fill_draw,
            &mut selector,
        )
    };
    // Match the existing fresh-loader publication after its Fill pass. The
    // saved-load candidate must later replace these retained theater values.
    cache.complete_theater_registry_load(theater.rmg_tiles.clear_tile, theater.rmg_tiles.water_set);
    let roots =
        init_helpers::scheduler_anim_roots(&rules, &overlay_registry, terrain.tile_animations());
    init_helpers::bind_rules_asset_closure(
        &mut rules,
        &processed,
        &fixed_art,
        &roots,
        assets,
        theater.extension,
        &map.header.theater,
    )
    .expect("retail simulation asset closure");
    let roster =
        crate::map::houses::parse_house_roster(&map.ini, &rules.color_schemes, Some(&rules));
    let lighting = crate::map::lighting::parse_lighting(&map.ini);
    let metadata = SavedMapMetadata::capture(
        map.ini.content_hash(),
        &loaded.source,
        &map.basic,
        &map.terrain_objects,
        &roster,
        &roster.color_map(),
        &lighting,
        SAVED_ELAPSED_BUCKETS,
        Some("Saved player"),
        [6, -2],
    );
    simulation.session.map_name = name.into();
    simulation.session.install_waypoints(&map.waypoints);
    simulation.session.theater = map.header.theater.clone();
    simulation.session.map_width = terrain.width();
    simulation.session.map_height = terrain.height();
    simulation.session.tick = 91;
    simulation.session.binary_frame = 92;
    simulation.session.lighting = crate::sim::scenario_session::ScenarioLightingState::from_map(
        &crate::map::lighting::parse_lighting_profiles(&map.ini),
    );
    simulation.install_playfield_from_map_header(&map.header);
    simulation.overlay_grid = Some(crate::sim::overlay_grid::OverlayGrid::new(
        terrain.width(),
        terrain.height(),
    ));
    simulation.install_resolved_terrain_for_new_map(terrain.clone());
    let clear = theater.rmg_tiles.clear_tile.expect("retail ClearTile");
    assert!(
        theater.lookup.variant_count(clear) > 0,
        "fixture reaches the native variant selector"
    );
    let clear_file = theater
        .lookup
        .filename(i32::from(clear))
        .unwrap()
        .to_owned();
    (
        SimRuntime {
            simulation,
            resources: SimResources {
                rules,
                terrain_template: Some(terrain),
                overlay_registry,
                trigger_graph: map.trigger_graph,
                triggers: map.triggers,
                events: map.events,
                actions: map.actions,
            },
        },
        metadata,
        clear_file,
    )
}

fn snapshot_bytes(
    runtime: &SimRuntime,
    owner: &NativeRulesProcessOwner,
    map: &SavedMapMetadata,
    follow: Option<u64>,
    rules_hash: u64,
) -> Vec<u8> {
    let terrain = runtime
        .resources
        .terrain_template
        .as_ref()
        .unwrap()
        .capture_saved_template()
        .expect("fixture terrain authority");
    let resources = SavedScenarioResourcesRef::capture(
        &runtime.resources,
        owner.saved_scenario_rules().unwrap(),
        owner.fixed_art().content_hash(),
        terrain,
        map.clone(),
    );
    let view = SavedLocalView::new(
        [[4, 5], [5, 4], [0, 0], [u16::MAX, 0]],
        Vec::new(),
        false,
        std::array::from_fn(|_| Vec::new()),
        follow,
        Default::default(),
    )
    .unwrap();
    GameSnapshot::save_validated_with_presentation(
        &runtime.simulation,
        map.map_hash(),
        rules_hash,
        "retail resources",
        123,
        &[],
        None,
        None,
        Some(&view),
        Some(&resources),
    )
}

fn lookup(assets: &AssetManager, name: &str) -> Option<(String, i32, Vec<u8>)> {
    assets.resolve_ref(name).map(|source| {
        (
            source.source_archive.to_owned(),
            source.entry_id,
            source.bytes.to_vec(),
        )
    })
}

fn assert_incoming_resources(
    prepared: &PreparedSavedResources,
    incoming: &SimRuntime,
    map: &SavedMapMetadata,
) {
    let sim = &prepared.world.simulation;
    assert_eq!(sim.session.map_name, incoming.simulation.session.map_name);
    assert_eq!(sim.session.theater, "TEMPERATE");
    assert_eq!(sim.session.tick, incoming.simulation.session.tick);
    assert_eq!(
        sim.session.binary_frame,
        incoming.simulation.session.binary_frame
    );
    assert_eq!(
        sim.map_size_diamond(),
        incoming.simulation.map_size_diamond()
    );
    assert_eq!(
        prepared.resources.rules.object("MTNK").unwrap().strength,
        321
    );
    assert_eq!(
        prepared.resources.rules.simulation_config_hash(),
        incoming.resources.rules.simulation_config_hash()
    );
    assert!(
        !prepared
            .resources
            .rules
            .art()
            .scheduler_anim_types()
            .is_empty()
    );
    assert_eq!(
        sim.session.waypoint_cell(7),
        incoming.simulation.session.waypoint_cell(7)
    );
    assert_ne!(sim.session.waypoint_cell(7), Some((0, 0)));
    assert_eq!(
        serde_json::to_value(&prepared.map).unwrap(),
        serde_json::to_value(map).unwrap()
    );
    assert_eq!(
        bincode::serialize(
            &prepared
                .resources
                .terrain_template
                .as_ref()
                .unwrap()
                .capture_saved_template()
                .unwrap()
        )
        .unwrap(),
        bincode::serialize(
            &incoming
                .resources
                .terrain_template
                .as_ref()
                .unwrap()
                .capture_saved_template()
                .unwrap()
        )
        .unwrap(),
        "asset rebinding preserves baseline terrain values",
    );
    let live = sim.resolved_terrain.as_ref().unwrap();
    assert!(
        live.native_tmp_file_count(i32::from(prepared.theater.rmg_tiles.clear_tile.unwrap()))
            .is_some(),
        "saved live terrain has actual retail TMP catalogs"
    );
    assert_eq!(prepared.elapsed_clock.elapsed_seconds(LOAD_CLOCK_MS), 3);
    assert_eq!(
        prepared.elapsed_clock.elapsed_seconds(LOAD_CLOCK_MS + 960),
        4
    );
}

/// A later authored map's pre-pack Fill reads the process cache, even though
/// loading the save itself does not run Fill. Exercise the same adoption and
/// Fill owners with a zero range result to isolate the retail registry base.
fn assert_next_fill_uses_saved_theater_registry(
    prepared: &PreparedSavedResources,
    process_cache: &TileVariantSelectorCache,
) {
    let mut adopted = process_cache.detached();
    adopted
        .adopt_prepared(prepared.tile_cache.detached())
        .unwrap();
    let mut main = || panic!("Fill must not draw Main");
    let mut next_map = adopted.begin_load(&mut main);
    let mut first_variant = |_, _| 0;
    assert_eq!(
        next_map.draw_fill_tile_index(false, &mut first_variant),
        i32::from(prepared.theater.rmg_tiles.clear_tile.unwrap()),
        "next map Clear Fill must use the loaded theater registry",
    );
    assert_eq!(
        next_map.draw_fill_tile_index(true, &mut first_variant),
        i32::from(prepared.theater.rmg_tiles.water_set.unwrap()),
        "next map Water Fill must use the loaded theater registry",
    );
}

#[test]
fn retail_saved_resources_prepare_cold_cross_map_and_roll_back_late_failure() {
    let Some((root, mut assets)) = retail_assets() else {
        return;
    };
    let mut owner = retail_rules_owner(&assets);
    assets
        .register_neutral_archives()
        .expect("retail shell archives");
    let modes = crate::skirmish_modes::skirmish_modes_from_assets(&assets).expect("retail modes");
    let mode = crate::skirmish_modes::mode_by_id(&modes, 1).expect("retail Battle mode");
    let mode = crate::rules::retail_sources::select_ini(&assets, &mode.override_file)
        .expect("retail Battle INI");
    let directory = TestDirectory::new("saved-retail-resources");
    directory.write("incoming.map", map_text("TEMPERATE", 321, 8).as_bytes());
    directory.write("outgoing.map", map_text("SNOW", 654, 6).as_bytes());
    let source_path = directory.path().join("incoming.map");
    let source_name = source_path.to_str().unwrap();
    let loaded =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, source_name, &assets)
            .unwrap();
    let mut warm_cache = TileVariantSelectorCache::default();
    let (incoming, map, incoming_file) = bound_saved_world(
        loaded,
        source_name,
        &mut assets,
        &mut owner,
        &mode.ini,
        &mut warm_cache,
    );
    let rules_hash = incoming.resources.rules.simulation_config_hash();
    let bytes = snapshot_bytes(&incoming, &owner, &map, None, rules_hash);
    let bad_follow = snapshot_bytes(&incoming, &owner, &map, Some(MISSING_FOLLOW_ID), rules_hash);
    let bad_rules = snapshot_bytes(&incoming, &owner, &map, None, rules_hash ^ 1);
    let incoming_lookup = lookup(&assets, &incoming_file).expect("incoming retail TMP is resolved");
    std::fs::remove_file(&source_path).unwrap();
    assert!(
        !source_path.exists(),
        "loading must use the saved map owners"
    );
    let repository = SaveRepository::at(directory.path().join("saves"));
    let clock = || LOAD_CLOCK_MS;

    // Native546DA0 cold table initialization is the narrow RNG comparison:
    // real loaded Clear cells enter the same selector, without Fill or IDs.
    let native: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/bridge_recalc_presentation.json",
    ))
    .unwrap();
    let native = native["load_selector_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["input"]["name"] == "cold_fixed_count2_seed0")
        .unwrap();
    let frontend: crate::sim::rng::MainRng =
        SimRng::from_native_state_hex_for_test(native["before"]["main"].as_str().unwrap()).into();
    let frontend_before = frontend.logical_state();
    let cold_owner = retail_rules_owner(&assets);
    assert!(cold_owner.saved_scenario_rules().is_none());
    let cold_cache = TileVariantSelectorCache::default();
    let view = SavedGamePreparationView {
        repository: &repository,
        current_simulation: None,
        frontend_main: &frontend,
        rules_owner: &cold_owner,
        tile_cache: &cold_cache,
        load_clock: &clock,
    };
    let cold = assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap();
    assert_incoming_resources(&cold, &incoming, &map);
    assert_next_fill_uses_saved_theater_registry(&cold, &cold_cache);
    assert_eq!(
        cold.world.simulation.main_rng.logical_state(),
        SimRng::from_native_state_hex_for_test(native["after"]["main"].as_str().unwrap())
            .logical_state()
    );
    let expected_table: Vec<u8> = native["table_after"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| u8::try_from(value.as_u64().unwrap()).unwrap())
        .collect();
    assert_eq!(
        cold.tile_cache
            .selector()
            .initialized_table()
            .unwrap()
            .as_slice(),
        expected_table
    );
    assert_eq!(
        cold.world.simulation.rng_state().scenario,
        SimRng::new(0).logical_state()
    );
    assert_eq!(
        cold.world.simulation.rng_state().mapgen,
        SimRng::new(0).logical_state()
    );
    assert_eq!(frontend.logical_state(), frontend_before);
    assert!(
        !cold_cache.is_initialized(),
        "uncommitted candidate cannot warm the process cache"
    );
    assert!(cold_owner.saved_scenario_rules().is_none());
    drop(cold);

    let outgoing_path = directory.path().join("outgoing.map");
    let outgoing_name = outgoing_path.to_str().unwrap();
    let loaded =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, outgoing_name, &assets)
            .unwrap();
    let (mut outgoing, _, outgoing_file) = bound_saved_world(
        loaded,
        outgoing_name,
        &mut assets,
        &mut owner,
        &mode.ini,
        &mut warm_cache,
    );
    outgoing
        .simulation
        .initialize_random_number_system(0xA123_4567);
    for _ in 0..7 {
        outgoing.simulation.main_rng.next_u32();
    }
    for _ in 0..11 {
        outgoing.simulation.mapgen_rng.next_u32();
    }
    for _ in 0..13 {
        outgoing.simulation.scenario_rng.next_u32();
    }
    assert_eq!(
        outgoing.resources.rules.object("MTNK").unwrap().strength,
        654
    );
    assert_ne!(
        outgoing.resources.rules.simulation_config_hash(),
        rules_hash
    );
    let outgoing_hash = outgoing.simulation.state_hash();
    let outgoing_rng = outgoing.simulation.rng_state();
    let outgoing_dummy = outgoing.simulation.effective_shared_cell_dummy().snapshot();
    let outgoing_rules = owner.saved_scenario_rules().unwrap().ini().content_hash();
    let outgoing_archives = assets.registered_archive_names();
    let outgoing_lookup = lookup(&assets, &outgoing_file).expect("outgoing retail TMP is resolved");
    let before_incoming_lookup = lookup(&assets, &incoming_file);
    let retained_table = warm_cache
        .selector()
        .initialized_table()
        .expect("fixture warmed process selector");
    let retained_fill_bases = (
        warm_cache.cached_clear_tile_base(),
        warm_cache.cached_water_set_base(),
    );
    let view = SavedGamePreparationView {
        repository: &repository,
        current_simulation: Some(&outgoing.simulation),
        frontend_main: &frontend,
        rules_owner: &owner,
        tile_cache: &warm_cache,
        load_clock: &clock,
    };

    // A wrong digest must still fail after rebuilding the incoming asset-bound
    // rules. It must not be accepted from saved or outgoing header values alone.
    let rejected = assets.prepare_saved_load(|assets| {
        PreparedSavedGame::prepare_resources(&bad_rules, &view, assets)
    });
    assert!(
        matches!(rejected, Err(PreparedLoadError::Snapshot(SnapshotError::RulesMismatch { expected, found })) if expected == rules_hash && found == (rules_hash ^ 1))
    );
    assert_eq!(assets.registered_archive_names(), outgoing_archives);
    assert_eq!(
        lookup(&assets, &outgoing_file),
        Some(outgoing_lookup.clone())
    );

    // Missing Follow is admitted structurally, then rejected only after the
    // world and live terrain have been prepared. Inspect lookup inside the
    // guard to prove the theater switch happened before rollback. A detached
    // cold selector exercises candidate-only initialization on this failure.
    let failed_view = SavedGamePreparationView {
        tile_cache: &cold_cache,
        ..view
    };
    let rejected = assets.prepare_saved_load(|assets| {
        let result = PreparedSavedGame::prepare_resources(&bad_follow, &failed_view, assets);
        assert_eq!(
            lookup(assets, &incoming_file),
            Some(incoming_lookup.clone())
        );
        assert_ne!(assets.registered_archive_names(), outgoing_archives);
        result
    });
    assert!(matches!(
        rejected,
        Err(PreparedLoadError::LocalView(
            SavedLocalViewError::MissingEntityReference {
                field: "Follow",
                object_id: MISSING_FOLLOW_ID
            }
        ))
    ));
    assert_eq!(assets.registered_archive_names(), outgoing_archives);
    assert_eq!(lookup(&assets, &outgoing_file), Some(outgoing_lookup));
    assert_eq!(lookup(&assets, &incoming_file), before_incoming_lookup);
    assert!(
        !cold_cache.is_initialized(),
        "failed candidate cannot publish its table"
    );
    assert_eq!(
        warm_cache.selector().initialized_table(),
        Some(retained_table)
    );
    assert_eq!(cold_cache.cached_clear_tile_base(), 0);
    assert_eq!(cold_cache.cached_water_set_base(), 0);
    assert_eq!(
        (
            warm_cache.cached_clear_tile_base(),
            warm_cache.cached_water_set_base(),
        ),
        retained_fill_bases,
        "rejected preparation cannot publish the incoming registry",
    );
    assert_eq!(outgoing.simulation.state_hash(), outgoing_hash);
    assert_eq!(outgoing.simulation.rng_state(), outgoing_rng);
    assert_eq!(
        outgoing.simulation.effective_shared_cell_dummy().snapshot(),
        outgoing_dummy
    );
    assert_eq!(
        owner.saved_scenario_rules().unwrap().ini().content_hash(),
        outgoing_rules
    );

    let prepared = assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap();
    assert_incoming_resources(&prepared, &incoming, &map);
    assert_next_fill_uses_saved_theater_registry(&prepared, &warm_cache);
    assert_eq!(
        prepared.world.simulation.rng_state().main,
        outgoing_rng.main
    );
    assert_eq!(
        prepared.world.simulation.rng_state().mapgen,
        outgoing_rng.mapgen
    );
    assert_eq!(
        prepared.world.simulation.rng_state().scenario,
        SimRng::new(0).logical_state()
    );
    assert_eq!(
        prepared.world.simulation.session.seed,
        outgoing.simulation.session.seed
    );
    assert_eq!(
        prepared.tile_cache.selector().initialized_table(),
        Some(retained_table)
    );
    assert_eq!(outgoing.simulation.state_hash(), outgoing_hash);
    assert_eq!(outgoing.simulation.rng_state(), outgoing_rng);
    assert_eq!(
        outgoing.simulation.effective_shared_cell_dummy().snapshot(),
        outgoing_dummy
    );
    assert_eq!(
        owner.saved_scenario_rules().unwrap().ini().content_hash(),
        outgoing_rules
    );
    assert!(!source_path.exists());
}
