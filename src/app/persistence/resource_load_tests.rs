//! Connected, GPU-free resource preparation through the production saved-load
//! owner. The small authored worlds use retail Rules/ART/Battle/theater assets;
//! their setup is a persistence fixture, not a second fresh-scenario loader or
//! a native whole-world startup comparison.

use super::*;
use crate::map::source::{LoadedMap, test_support::TestDirectory};
use crate::rules::ini_parser::IniFile;
use crate::rules::retail_ini_fixture::{retail_assets, retail_rules_owner};
use crate::sim::rng::SimRng;
use crate::sim::snapshot::saved_local_view::{SavedLocalView, SavedLocalViewError, SavedPowerBar};
use crate::sim::snapshot::scenario_resources::SavedScenarioResourcesRef;
use crate::util::save_codec;

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
        let main_rng = simulation.main_rng().draws();
        let mut main_draw = || main_rng.next_u32();
        let mut selector = cache.begin_load(&mut main_draw);
        let mut fill_draw = |min, max| {
            simulation
                .scenario_rng_mut_for_test()
                .next_range_u32_inclusive(min, max)
        };
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
    let roots = crate::map::resolved_terrain::scheduler_anim_roots(
        &rules,
        &overlay_registry,
        terrain.tile_animations(),
    );
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
    // A nonempty retained title makes both warm and cold resource restoration
    // prove that saved names survive without consulting the current CSF.
    let mut campaign_names = crate::rules::campaign_loading::CampaignScenarioNames::default();
    campaign_names.apply_ini(
        &IniFile::from_str("[saved.map]\nUIName=NAME:SAVED\n"),
        "saved.map",
        &crate::assets::csf_file::test_csf(&[
            ("NAME:SAVED", "Retained mission name"),
            ("NAME:SAVEDSav", "Retained save title"),
        ]),
    );
    let metadata = SavedMapMetadata::capture(
        map.ini.content_hash(),
        &loaded.source,
        &map.basic,
        &map.ranking,
        &campaign_names,
        &map.terrain_objects,
        &roster,
        &roster.color_map(),
        &lighting,
        SAVED_ELAPSED_BUCKETS,
        Some("Saved player"),
        [6, -2],
        true,
    );
    // One video-owned and one ordinary trigger-owned untimed hold.
    simulation.pause_scenario_for_test(0);
    simulation.pause_scenario_for_test(0);
    simulation
        .session_mut_for_test()
        .set_map_name_for_test(name.into());
    simulation
        .session_mut_for_test()
        .install_waypoints(&map.waypoints);
    simulation
        .session_mut_for_test()
        .set_theater_for_test(map.header.theater.clone());
    simulation
        .session_mut_for_test()
        .set_map_width_for_test(terrain.width());
    simulation
        .session_mut_for_test()
        .set_map_height_for_test(terrain.height());
    simulation.session_mut_for_test().set_tick_for_test(91);
    simulation
        .session_mut_for_test()
        .set_binary_frame_for_test(92);
    simulation.session_mut_for_test().set_lighting_for_test(
        crate::sim::scenario_session::ScenarioLightingState::from_map(
            &crate::map::lighting::parse_lighting_profiles(&map.ini),
        ),
    );
    simulation.install_playfield_from_map_header(&map.header);
    *simulation.overlay_grid_mut_for_test() = Some(crate::sim::overlay_grid::OverlayGrid::new(
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
            },
        },
        metadata,
        clear_file,
    )
}

fn native_saved_power() -> SavedPowerBar {
    let packet = crate::test_fixture::json("tools/sidebar_oracle/power_state.json");
    let prior = &packet["loaded_histories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "retained-active")
        .unwrap()["prior"];
    SavedPowerBar {
        counts_surplus_output_drain: serde_json::from_value(prior["counts"].clone()).unwrap(),
        cached_output: prior["cached"][1].as_i64().unwrap() as i32,
        cached_drain: prior["cached"][0].as_i64().unwrap() as i32,
        flashes_remaining: prior["flash"].as_i64().unwrap() as i32,
        flash_timer: serde_json::from_value(prior["flash_timer"].clone()).unwrap(),
        animating: prior["animating"].as_bool().unwrap(),
    }
}

fn snapshot_bytes(
    runtime: &SimRuntime,
    owner: &NativeRulesProcessOwner,
    map: &SavedMapMetadata,
    follow: Option<u64>,
    rules_hash: u64,
    lighting: Option<&crate::map::lighting::SavedCellLighting>,
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
    let mut view = SavedLocalView::new(
        [[4, 5], [5, 4], [0, 0], [u16::MAX, 0]],
        Vec::new(),
        false,
        std::array::from_fn(|_| Vec::new()),
        follow,
        Default::default(),
    )
    .unwrap();
    view.set_power_bar(native_saved_power());
    let mut lights = crate::app::presentation::lighting::MatchLighting::default();
    lights.install(
        crate::map::lighting::CellLightGrid::new(),
        map.lighting().clone(),
        2,
        Some((
            runtime.simulation.resolved_terrain().unwrap(),
            &runtime.simulation,
        )),
    );
    view.set_lighting(
        lighting
            .cloned()
            .unwrap_or_else(|| lights.saved_at(&runtime.simulation)),
    );
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
    assert_eq!(
        prepared.power_bar_anim.saved(),
        native_saved_power(),
        "incoming retained Power wins on warm and cold preparation"
    );
    let sim = &prepared.world.simulation;
    assert_eq!(
        sim.session().map_name(),
        incoming.simulation.session().map_name()
    );
    assert_eq!(sim.session().theater(), "TEMPERATE");
    assert_eq!(sim.session().tick(), incoming.simulation.session().tick());
    assert_eq!(
        sim.session().binary_frame(),
        incoming.simulation.session().binary_frame()
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
        sim.session().waypoint_cell(7),
        incoming.simulation.session().waypoint_cell(7)
    );
    assert_ne!(sim.session().waypoint_cell(7), Some((0, 0)));
    assert_eq!(
        serde_json::to_value(&prepared.map).unwrap(),
        serde_json::to_value(map).unwrap()
    );
    assert_eq!(
        prepared.map.campaign_names().save_title(),
        "Retained save title"
    );
    assert_eq!(
        save_codec::serialize(
            &prepared
                .resources
                .terrain_template
                .as_ref()
                .unwrap()
                .capture_saved_template()
                .unwrap()
        )
        .unwrap(),
        save_codec::serialize(
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
    let live = sim.resolved_terrain().unwrap();
    assert!(
        live.native_tmp_file_count(i32::from(prepared.theater.rmg_tiles.clear_tile.unwrap()))
            .is_some(),
        "saved live terrain has actual retail TMP catalogs"
    );
    assert!(prepared.map.radar_pause_credit());
    let mut restored_session = Simulation::with_seed(0);
    *restored_session.session_mut_for_test() = sim.session().clone();
    let release = crate::app::radar_movie::release_saved_scenario_hold(
        &mut restored_session,
        prepared.map.radar_pause_credit(),
    );
    assert!(release.is_empty());
    assert!(
        restored_session.session().scenario_paused(),
        "unrelated trigger hold survives missing video retirement"
    );
    assert!(
        restored_session
            .resume_scenario()
            .contains(&crate::sim::trigger_runtime::TriggerEffect::ScenarioResume)
    );
    assert!(!restored_session.session().scenario_paused());
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

/// The incoming saved world: the retail archives with the shell archives
/// registered, the warm rules owner and process tile cache that bound it, its
/// save three ways (valid, with a missing Follow, with a wrong rules digest),
/// and the map directory with the incoming map removed, so a load must use the
/// saved map owners.
struct IncomingSave {
    root: std::path::PathBuf,
    assets: AssetManager,
    owner: NativeRulesProcessOwner,
    mode: IniFile,
    directory: TestDirectory,
    source_path: std::path::PathBuf,
    warm_cache: TileVariantSelectorCache,
    incoming: SimRuntime,
    map: SavedMapMetadata,
    incoming_file: String,
    rules_hash: u64,
    bytes: Vec<u8>,
    bad_follow: Vec<u8>,
    bad_rules: Vec<u8>,
    incoming_lookup: (String, i32, Vec<u8>),
    repository: SaveRepository,
}

fn incoming_save() -> Option<IncomingSave> {
    let (root, mut assets) = retail_assets()?;
    let mut owner = retail_rules_owner(&assets);
    assets
        .register_neutral_archives()
        .expect("retail shell archives");
    let modes = crate::skirmish_modes::skirmish_modes_from_assets(&assets).expect("retail modes");
    let mode = crate::skirmish_modes::mode_by_id(&modes, 1).expect("retail Battle mode");
    let mode = crate::rules::retail_sources::select_ini(&assets, &mode.override_file)
        .expect("retail Battle INI")
        .ini;
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
        &mode,
        &mut warm_cache,
    );
    let rules_hash = incoming.resources.rules.simulation_config_hash();
    let bytes = snapshot_bytes(&incoming, &owner, &map, None, rules_hash, None);
    let bad_follow = snapshot_bytes(
        &incoming,
        &owner,
        &map,
        Some(MISSING_FOLLOW_ID),
        rules_hash,
        None,
    );
    let bad_rules = snapshot_bytes(&incoming, &owner, &map, None, rules_hash ^ 1, None);
    let incoming_lookup = lookup(&assets, &incoming_file).expect("incoming retail TMP is resolved");
    std::fs::remove_file(&source_path).unwrap();
    assert!(
        !source_path.exists(),
        "loading must use the saved map owners"
    );
    let repository = SaveRepository::at(directory.path().join("saves"));
    Some(IncomingSave {
        root,
        assets,
        owner,
        mode,
        directory,
        source_path,
        warm_cache,
        incoming,
        map,
        incoming_file,
        rules_hash,
        bytes,
        bad_follow,
        bad_rules,
        incoming_lookup,
        repository,
    })
}

/// Native546DA0 cold table initialization is the narrow RNG comparison:
/// real loaded Clear cells enter the same selector, without Fill or IDs.
fn cold_selector_case() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/spatial_oracle/bridge_recalc_presentation.json")
        ["load_selector_cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["input"]["name"] == "cold_fixed_count2_seed0")
        .unwrap()
}

/// The frontend Main stream the native cold case starts from.
fn frontend_main() -> crate::sim::rng::MainRng {
    let native = cold_selector_case();
    SimRng::from_native_state_hex_for_test(native["before"]["main"].as_str().unwrap()).into()
}

#[test]
fn saved_local_grapple_light_uses_write_time_events_and_rejects_foreign_ids() {
    use crate::app::presentation::lighting::{MatchLighting, rebuild_lighting_grid_from_sim};
    use crate::map::lighting::CellLightGrid;
    use crate::sim::components::AnimClassSpawnDescriptor;
    use crate::sim::light_sources::{LightingEvent, PaletteUpdate};
    let Some(mut save) = incoming_save() else {
        return;
    };
    let sim = &mut save.incoming.simulation;
    let kind = sim.intern("SQDG");
    let anim = sim
        .spawn_anim_object(
            &save.incoming.resources.rules,
            AnimClassSpawnDescriptor::new(
                kind,
                8,
                8,
                crate::util::fixed_math::SimFixed::ZERO,
                crate::util::fixed_math::SimFixed::ZERO,
                0,
            ),
        )
        .expect("bound retail SQDG constructor");
    let house = sim.intern("Americans");
    sim.anims_mut_for_test().set_anim_house_remap(anim, house);
    let mut lights = MatchLighting::default();
    lights.install(
        CellLightGrid::new(),
        save.map.lighting().clone(),
        2,
        Some((sim.resolved_terrain().unwrap(), sim)),
    );
    // PreparedSave holds the earlier local owner. The actual writer must
    // consume the pre-Logic publications, not reconstruct FC from final light.
    // Zero ambient is a supplied boundary input; this is not a stock map claim.
    let frame = sim.session().binary_frame() as i32;
    sim.start_nuke_flash_for_test(frame);
    sim.session_mut_for_test()
        .lighting_mut()
        .set_current_ambient_for_test(0);
    sim.publish_global_lighting(PaletteUpdate::from_scenario(sim.session().lighting()));
    sim.lighting_sources_mut_for_test()
        .publish_for_test(LightingEvent::RetainAnimTop { anim, cell: (8, 8) });
    sim.session_mut_for_test()
        .lighting_mut()
        .set_current_ambient_for_test(120);
    sim.publish_global_lighting(PaletteUpdate::Preserve);
    let before = (sim.state_hash(), sim.rng_state());
    let mut saved = lights.saved_at(sim);
    assert_eq!(
        saved.anims,
        [(anim, 0)],
        "an explicit zero is retained, not absent/default"
    );
    assert_eq!(
        saved.cells.iter().find(|row| row.0 == (8, 8)).unwrap().3,
        1100
    );
    assert_eq!(
        lights.grid().retained_anim_top(anim),
        None,
        "save projection is isolated"
    );
    assert_eq!((sim.state_hash(), sim.rng_state()), before);

    // A supplied retained104/108/top history distinguishes the post-load
    //484680 refresh from both exact top restoration and a fresh gather.
    let retained = saved.cells.iter_mut().find(|row| row.0 == (8, 8)).unwrap();
    *retained = ((8, 8), 32768, 25, 333);
    let terrain = sim.resolved_terrain().unwrap();
    assert_eq!(terrain.cell(8, 8).unwrap().level, 0);
    assert!(
        crate::map::lighting::native_lighting_cells(terrain, sim.map_size_diamond().unwrap().0)
            .any(|cell| cell == (8, 8))
    );
    let retint = LightingEvent::Global {
        state: *sim.session().lighting(),
        cell_profile: sim.lighting_cell_profile(),
        palette: PaletteUpdate::Normal([1200, 800, 600]),
        constructor_rgb: sim.palette_constructor_rgb(),
    };
    sim.lighting_sources_mut_for_test().publish_for_test(retint);
    // Loaded Scenario normal RGB differs from the surviving process cache.
    let normal = sim
        .session_mut_for_test()
        .lighting_mut()
        .normal_mut_for_test();
    normal.red_percent = 140;
    normal.green_percent = 130;
    normal.blue_percent = 120;
    // Post-load53C280 reselects from effects, not this supplied old selector.
    sim.session_mut_for_test()
        .lighting_mut()
        .set_selected_profile_for_test(
            crate::sim::scenario_session::ScenarioLightingProfile::Normal,
        );
    let before = (sim.state_hash(), sim.rng_state());

    let bytes = snapshot_bytes(
        &save.incoming,
        &save.owner,
        &save.map,
        None,
        save.rules_hash,
        Some(&saved),
    );
    let frontend = frontend_main();
    let cache = TileVariantSelectorCache::default();
    let clock = || LOAD_CLOCK_MS;
    let view = SavedGamePreparationView {
        repository: &save.repository,
        current_simulation: Some(&save.incoming.simulation),
        frontend_main: &frontend,
        rules_owner: &save.owner,
        tile_cache: &cache,
        lighting: &lights,
        load_clock: &clock,
    };
    let prepared = save
        .assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap();
    assert_eq!(prepared.lighting, saved);
    assert_eq!(prepared.scheme_normal_rgb, [1200, 800, 600]);
    let incoming = &prepared.world.simulation;
    assert_eq!(
        incoming.session().lighting().selected_profile(),
        crate::sim::scenario_session::ScenarioLightingProfile::Nuke
    );
    assert_eq!(incoming.session().lighting().target_ambient(), 200);
    let terrain = incoming.resolved_terrain().unwrap();
    let mut staged =
        rebuild_lighting_grid_from_sim(terrain, save.map.lighting(), Some(incoming), 2);
    staged.stage_saved_local_lighting(&prepared.lighting, prepared.scheme_normal_rgb);
    let mut restored = MatchLighting::default();
    restored.install(
        staged,
        save.map.lighting().clone(),
        2,
        Some((terrain, incoming)),
    );
    assert_eq!(restored.grid().retained_anim_top(anim), Some(0));
    assert_eq!(
        restored.grid().cell_light_at((8, 8)).unwrap().top_scalar,
        1125
    );
    assert_eq!(
        restored.grid().cell_light_at((8, 8)).unwrap().common_scalar,
        562
    );
    assert_eq!(restored.grid().color_scheme_rgb(), [1750, 1500, 1250]);
    assert_eq!(restored.grid().color_scheme_normal_rgb(), [1200, 800, 600]);

    // The complete normal post-load arm restores that cache, rather than
    // passing the saved Scenario RGB1400/1300/1200 to53AD00.
    let mut normal_prepared = save
        .assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap();
    let normal_world = &mut normal_prepared.world.simulation;
    normal_world
        .session_mut_for_test()
        .lighting_mut()
        .set_nuke_flash_for_test(0, -1, -1);
    normal_world.reinitialize_lighting_after_user_load();
    let normal_terrain = normal_world.resolved_terrain().unwrap();
    let mut normal_grid =
        rebuild_lighting_grid_from_sim(normal_terrain, save.map.lighting(), Some(normal_world), 2);
    normal_grid.stage_saved_local_lighting(&saved, prepared.scheme_normal_rgb);
    let mut normal_lights = MatchLighting::default();
    normal_lights.install(
        normal_grid,
        save.map.lighting().clone(),
        2,
        Some((normal_terrain, normal_world)),
    );
    assert_eq!(normal_lights.grid().color_scheme_rgb(), [1200, 800, 600]);
    assert_eq!(normal_lights.grid().retained_anim_top(anim), Some(0));
    assert_eq!(
        normal_lights
            .grid()
            .cell_light_at((8, 8))
            .unwrap()
            .top_scalar,
        1100
    );

    let mut invalid = saved.clone();
    invalid.anims[0].0 = u64::MAX;
    let bad = snapshot_bytes(
        &save.incoming,
        &save.owner,
        &save.map,
        None,
        save.rules_hash,
        Some(&invalid),
    );
    let archives = save.assets.registered_archive_names();
    let rejected = save
        .assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bad, &view, assets));
    assert!(matches!(
        rejected,
        Err(PreparedLoadError::LocalView(
            SavedLocalViewError::InvalidLighting {
                field: "Anim identity/scalar"
            }
        ))
    ));
    assert_eq!(save.assets.registered_archive_names(), archives);
    assert!(
        !cache.is_initialized(),
        "candidate cannot publish process caches"
    );
    assert_eq!(
        (
            save.incoming.simulation.state_hash(),
            save.incoming.simulation.rng_state()
        ),
        before
    );
    assert_eq!(
        restored.grid().retained_anim_top(anim),
        Some(0),
        "failed candidate leaves local owner intact"
    );
    restored.refresh(terrain, incoming, 2);
    assert_eq!(restored.grid().retained_anim_top(anim), Some(0));
    assert_eq!(
        lights.grid().color_scheme_rgb(),
        [1000; 3],
        "pending retint is projected only on a clone during preparation"
    );
}

#[test]
fn retail_saved_resources_prepare_cold() {
    let Some(save) = incoming_save() else {
        return;
    };
    let IncomingSave {
        mut assets,
        incoming,
        map,
        bytes,
        repository,
        ..
    } = save;
    let clock = || LOAD_CLOCK_MS;
    let native = cold_selector_case();
    let frontend = frontend_main();
    let frontend_before = frontend.logical_state();
    let cold_owner = retail_rules_owner(&assets);
    assert!(cold_owner.saved_scenario_rules().is_none());
    let cold_cache = TileVariantSelectorCache::default();
    let cold_lighting = crate::app::presentation::lighting::MatchLighting::default();
    let view = SavedGamePreparationView {
        repository: &repository,
        current_simulation: None,
        frontend_main: &frontend,
        rules_owner: &cold_owner,
        tile_cache: &cold_cache,
        lighting: &cold_lighting,
        load_clock: &clock,
    };
    let cold = assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap();
    assert_incoming_resources(&cold, &incoming, &map);
    assert_next_fill_uses_saved_theater_registry(&cold, &cold_cache);
    assert_eq!(
        cold.world.simulation.main_rng().logical_state(),
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
}

#[test]
fn retail_saved_resources_prepare_cross_map_and_roll_back_late_failure() {
    let Some(save) = incoming_save() else {
        return;
    };
    let IncomingSave {
        root,
        mut assets,
        mut owner,
        mode,
        directory,
        source_path,
        mut warm_cache,
        incoming,
        map,
        incoming_file,
        rules_hash,
        bytes,
        bad_follow,
        bad_rules,
        incoming_lookup,
        repository,
    } = save;
    let clock = || LOAD_CLOCK_MS;
    let frontend = frontend_main();
    let cold_cache = TileVariantSelectorCache::default();

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
        &mode,
        &mut warm_cache,
    );
    outgoing
        .simulation
        .initialize_random_number_system(0xA123_4567);
    for _ in 0..7 {
        outgoing.simulation.main_rng().next_u32();
    }
    for _ in 0..11 {
        outgoing.simulation.mapgen_rng_mut_for_test().next_u32();
    }
    for _ in 0..13 {
        outgoing.simulation.scenario_rng_mut_for_test().next_u32();
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
    let outgoing_lighting = crate::app::presentation::lighting::MatchLighting::default();
    let view = SavedGamePreparationView {
        repository: &repository,
        current_simulation: Some(&outgoing.simulation),
        frontend_main: &frontend,
        rules_owner: &owner,
        tile_cache: &warm_cache,
        lighting: &outgoing_lighting,
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
        prepared.world.simulation.session().seed(),
        outgoing.simulation.session().seed()
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
