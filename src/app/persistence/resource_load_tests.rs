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
    let (mut rules, _, fixed_art, _) = owner
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
    let overlay_registry = rules.overlay_registry().clone();
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
        &fixed_art,
        &overlay_registry,
        &roots,
        assets,
        theater.extension,
        &map.header.theater,
        crate::rules::terrain_asset_catalog::TerrainAssetLoadPhase::Saved,
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

fn saved_radar_for_fixture() -> crate::sim::snapshot::saved_local_view::SavedRadarEvents {
    let mut saved = crate::render::radar::saved_radar_fixture();
    // Keep this resource fixture's distinct logical/native clocks (91/92).
    // Shift the live alert fixture's timeline with the restored world.
    let offset = saved.last_advanced_frame.unwrap() - 91;
    saved.last_advanced_frame = Some(91);
    for event in &mut saved.events {
        event.created_frame -= offset;
        event.phase_started_frame = event.phase_started_frame.map(|frame| frame - offset);
    }
    saved
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
    let mut resources = SavedScenarioResourcesRef::capture(
        &runtime.resources,
        owner.saved_scenario_rules().unwrap(),
        owner.fixed_art().content_hash(),
        terrain,
        map.clone(),
    );
    // The same late capture used by PreparedSave::write, after immutable
    // resource preparation and immediately before simulation serialization.
    resources
        .capture_live_cell_land(runtime.simulation.resolved_terrain().unwrap())
        .unwrap();
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
    // Authored signed display input: raw save/load must not clamp or seed it.
    view.set_credits_counter(Some(-7));
    view.set_radar_events(saved_radar_for_fixture());
    GameSnapshot::save_validated_with_presentation(
        &runtime.simulation,
        map.map_hash(),
        rules_hash,
        "retail resources",
        123,
        &[],
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
    assert_eq!(prepared.credits_counter, Some(-7));
    assert_eq!(
        prepared.power_bar_anim.saved(),
        native_saved_power(),
        "incoming retained Power wins on warm and cold preparation"
    );
    assert_eq!(
        prepared.radar_events.saved(),
        saved_radar_for_fixture(),
        "both warm and cold loads transport incoming live alerts"
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
    // TypeLoad can replace image-load attempts after the saved digest has
    // passed. Compare the retained fields/assets below, not a pre-load digest
    // with the post-load projection. The wrong-digest control still rejects.
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
                .capture_user_saved_template(prepared.resources.terrain_template.as_ref().unwrap())
                .unwrap()
        )
        .unwrap(),
        save_codec::serialize(
            &incoming
                .resources
                .terrain_template
                .as_ref()
                .unwrap()
                .capture_user_saved_template(incoming.simulation.resolved_terrain().unwrap())
                .unwrap()
        )
        .unwrap(),
        "asset rebinding preserves baseline metadata plus saved effective Land",
    );
    let live = sim.resolved_terrain().unwrap();
    // CellLoad4839F0 clears literal crates and+4C, keeping saved Land and the
    // separate Map+68 plane. XMP25MW's WCRATE/Rough combination motivates this
    // small materialized fixture; no native/full retail map load is simulated.
    let cell = live.cell(8, 8).unwrap();
    assert_eq!(
        (cell.land_type, cell.base_land_type, cell.zone_type),
        (2, 7, 0)
    );
    assert!(!cell.overlay_blocks);
    assert_eq!(cell.speed_costs.track, Some(0));
    assert_eq!(cell.speed_costs.wheel, Some(0));
    assert_eq!(cell.speed_costs.float, Some(100));
    assert!(sim.path_grid().unwrap().is_walkable(8, 8));
    assert_eq!(sim.overlay_grid().unwrap().cell(8, 8).overlay_id, None);
    assert_eq!(sim.overlay_grid().unwrap().cell(7, 8).overlay_id, None);
    assert_eq!(sim.overlay_grid().unwrap().cell(8, 8).overlay_data, 0);
    assert!(
        !prepared
            .world
            .map_restore
            .occupied_overlays
            .iter()
            .any(|cell| (cell.rx, cell.ry) == (8, 8) || (cell.rx, cell.ry) == (7, 8))
    );
    assert_eq!(
        serde_json::to_value(sim.zone_grid()).unwrap(),
        serde_json::to_value(incoming.simulation.zone_grid()).unwrap()
    );
    for (loaded, saved) in sim
        .crate_authority()
        .slots()
        .iter()
        .zip(incoming.simulation.crate_authority().slots())
    {
        assert_eq!((loaded.cell_x, loaded.cell_y), (0, 0));
        assert_eq!(
            (loaded.start_frame, loaded.aux, loaded.duration),
            (saved.start_frame, saved.aux, saved.duration)
        );
    }
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

fn incoming_save(theater: &str, extra_rules: &str) -> Option<IncomingSave> {
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
    directory.write(
        "incoming.map",
        format!("{}{extra_rules}", map_text(theater, 321, 8)).as_bytes(),
    );
    directory.write("outgoing.map", map_text("SNOW", 654, 6).as_bytes());
    let source_path = directory.path().join("incoming.map");
    let source_name = source_path.to_str().unwrap();
    let loaded =
        crate::map::source::load_map_by_name_or_path_with_assets(&root, source_name, &assets)
            .unwrap();
    let mut warm_cache = TileVariantSelectorCache::default();
    let (mut incoming, map, incoming_file) = bound_saved_world(
        loaded,
        source_name,
        &mut assets,
        &mut owner,
        &mode,
        &mut warm_cache,
    );
    let rough = *incoming
        .resources
        .rules
        .terrain_rules
        .row(crate::rules::terrain_rules::LandType::Rough);
    let template = incoming.resources.terrain_template.as_mut().unwrap();
    let cell = template.cell_mut_for_test(8, 8).unwrap();
    cell.land_type = 7;
    cell.base_land_type = 7;
    cell.speed_costs = rough.speed_costs;
    cell.base_speed_costs = rough.speed_costs;
    incoming
        .simulation
        .install_resolved_terrain_for_new_map(template.clone());
    assert_eq!(
        incoming.resources.overlay_registry.id_for_name("CRATE"),
        Some(179)
    );
    assert_eq!(
        incoming.resources.overlay_registry.id_for_name("WCRATE"),
        Some(242)
    );
    let overlays = incoming
        .simulation
        .overlay_grid_mut_for_test()
        .as_mut()
        .unwrap();
    overlays.place_overlay(8, 8, 242, 10);
    overlays.place_overlay(7, 8, 179, 255);
    let terrain = incoming
        .simulation
        .resolved_terrain_mut_for_test()
        .as_mut()
        .unwrap();
    // Split the test-only borrows through a short-lived owned grid.
    let mut terrain = terrain.clone();
    let overlays = incoming
        .simulation
        .overlay_grid_mut_for_test()
        .as_mut()
        .unwrap();
    crate::sim::overlay_grid::recalc_overlay_passability(
        overlays,
        &mut terrain,
        &incoming.resources.overlay_registry,
        8,
        8,
    );
    *incoming.simulation.resolved_terrain_mut_for_test() = Some(terrain);
    *incoming
        .simulation
        .crate_authority_mut_for_test()
        .slot_mut_for_test(0) = crate::sim::crates::CrateSlot {
        start_frame: 73,
        aux: 0x1234,
        duration: 125,
        cell_x: 8,
        cell_y: 8,
    };
    *incoming
        .simulation
        .crate_authority_mut_for_test()
        .slot_mut_for_test(255) = crate::sim::crates::CrateSlot {
        start_frame: -1,
        aux: u32::MAX,
        duration: 321,
        cell_x: -13,
        cell_y: 0,
    };
    assert!(
        incoming
            .simulation
            .rebuild_dynamic_navigation(&incoming.resources.rules)
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
fn retail_saved_resources_prepare_cold() {
    let Some(save) = incoming_save("TEMPERATE", "") else {
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
fn unread_cell_anim_loads_its_own_retail_shape_without_replaying_construction() {
    use crate::sim::components::AnimClassSpawnDescriptor;
    use crate::sim::native_identity::NativeUniqueIdCursor;
    use crate::util::fixed_math::SIM_ZERO;

    let Some(mut save) = incoming_save("TEMPERATE", "[GEM01]\nCellAnim=SEAL\n") else {
        return;
    };
    // A same-named Infantry type/ART section does not perform an Anim read.
    let rules = &save.incoming.resources.rules;
    let overlay = rules.overlay_registry();
    assert_eq!(
        overlay
            .flags(overlay.id_for_name("GEM01").unwrap())
            .unwrap()
            .cell_anim
            .as_deref(),
        Some("SEAL")
    );
    let config = rules.art().anim_runtime_config("SEAL").unwrap();
    assert!(!config.art_body_read);
    assert_eq!(config.raw_shp_frame_count, None);
    let shape = save
        .assets
        .get_ref("SEAL.SHP")
        .expect("real retail own-name SHP");
    let count = i32::from(i16::from_le_bytes([shape[6], shape[7]]));
    assert!(count > 0);
    // This persistence fixture supplies an already born Anim through the
    // common constructor. Ordinary Overlay Mark is covered by crates tests.
    let sim = &mut save.incoming.simulation;
    *sim.native_unique_ids_mut_for_test() = Some(NativeUniqueIdCursor::for_synthetic_simulation());
    let kind = sim.intern("SEAL");
    let mut descriptor = AnimClassSpawnDescriptor::new(kind, 5, 5, SIM_ZERO, SIM_ZERO, 0);
    descriptor.loop_count = 1;
    descriptor.draw_flags = 0x600;
    let id = sim.spawn_anim_object(rules, descriptor).unwrap();
    let native = sim.anim(id).unwrap().native_unique_id();
    let runtime = sim.anim(id).unwrap().runtime().clone();
    let cursor = sim.native_identity_cursor();
    let hash = rules.simulation_config_hash();
    let bytes = snapshot_bytes(&save.incoming, &save.owner, &save.map, None, hash);
    let clock = || LOAD_CLOCK_MS;
    let frontend = frontend_main();
    let cache = TileVariantSelectorCache::default();
    let view = SavedGamePreparationView {
        repository: &save.repository,
        current_simulation: None,
        frontend_main: &frontend,
        rules_owner: &save.owner,
        tile_cache: &cache,
        load_clock: &clock,
    };
    let prepared = save
        .assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap();
    // AnimTypeLoad428855 supplies image residency without ReadINI427D00
    // or LoadImageAndResolveFrameBounds427B50: all ctor bounds stay zero.
    let config = prepared
        .resources
        .rules
        .art()
        .anim_runtime_config("SEAL")
        .unwrap();
    assert!(!config.art_body_read);
    assert_eq!(config.raw_shp_frame_count, Some(count));
    assert_eq!(
        (config.end, config.loop_end, config.rate_logic_frames),
        (0, 0, 1)
    );
    let anim = prepared.world.simulation.anim(id).unwrap();
    assert_eq!(anim.native_unique_id(), native);
    assert_eq!(anim.runtime(), &runtime);
    assert_eq!(prepared.world.simulation.native_identity_cursor(), cursor);

    let PreparedSavedResources {
        world,
        resources,
        processed_rules,
        map,
        ..
    } = prepared;
    let (simulation, _) = world.into_committed(SharedCellDummy::fresh(), &frontend);
    save.owner.install_saved_scenario_rules(processed_rules);
    let loaded = SimRuntime {
        simulation,
        resources,
    };
    let loaded_hash = loaded.resources.rules.simulation_config_hash();
    let bytes = snapshot_bytes(&loaded, &save.owner, &map, None, loaded_hash);
    let view = SavedGamePreparationView {
        repository: &save.repository,
        current_simulation: None,
        frontend_main: &frontend,
        rules_owner: &save.owner,
        tile_cache: &cache,
        load_clock: &clock,
    };
    let again = save
        .assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap();
    assert_eq!(again.resources.rules.simulation_config_hash(), loaded_hash);
    assert_eq!(again.world.simulation.native_identity_cursor(), cursor);
    assert_eq!(again.world.simulation.anim(id).unwrap().runtime(), &runtime);
    let PreparedSavedResources {
        world, resources, ..
    } = again;
    let (mut sim, _) = world.into_committed(SharedCellDummy::fresh(), &frontend);
    for _ in 0..4 {
        sim.advance_tick(
            &[],
            Some(&resources.rules),
            None,
            Some(&resources.overlay_registry),
            33,
        );
    }
    assert!(
        sim.anim(id).is_none(),
        "zero-End Anim uses normal AI and deferred cleanup"
    );
}

#[test]
fn unknown_terrain_type_load_validates_then_commits_and_resaves_its_same_owner() {
    use crate::sim::terrain_spawn::{
        TerrainConstructionMode, TerrainConstructionRules, construct_terrain_objects_with_rules,
    };

    let Some(mut save) = incoming_save(
        "TEMPERATE",
        "[Terrain]\n5005=SMOKEY\n05005=TREE01\n[SMOKEY]\nStrength=9000\n",
    ) else {
        return;
    };
    // This existing persistence fixture has a declared synthetic prefix; the
    // headless retail regression covers the complete fresh Scenario prefix.
    *save.incoming.simulation.native_unique_ids_mut_for_test() =
        Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
    let overlays = save.incoming.resources.overlay_registry.clone();
    let rng = save.incoming.simulation.rng_state();
    construct_terrain_objects_with_rules(
        &mut save.incoming.simulation,
        save.map.terrain_objects(),
        &mut TerrainConstructionRules::Authored {
            rules: &mut save.incoming.resources.rules,
            owner: &mut save.owner,
        },
        false,
        TerrainConstructionMode::Authored(&overlays),
    )
    .unwrap();
    assert_eq!(save.incoming.simulation.rng_state(), rng);
    let unknown = save
        .incoming
        .simulation
        .terrain_objects()
        .objects()
        .values()
        .find(|object| {
            save.incoming
                .simulation
                .interner()
                .resolve(object.type_ref())
                == "SMOKEY"
        })
        .unwrap();
    let unknown_id = unknown.stable_id();
    let native_id = unknown.native_unique_id();
    let known_id = save
        .incoming
        .simulation
        .terrain_objects()
        .id_at((5, 5))
        .unwrap();
    assert_ne!(known_id, unknown_id);
    assert!(
        !save
            .incoming
            .resources
            .rules
            .terrain_object_type_case_insensitive("SMOKEY")
            .unwrap()
            .has_occupy_list()
    );
    let cursor = save.incoming.simulation.native_identity_cursor();
    let hash = save.incoming.resources.rules.simulation_config_hash();
    let bytes = snapshot_bytes(&save.incoming, &save.owner, &save.map, None, hash);
    let bad = snapshot_bytes(&save.incoming, &save.owner, &save.map, None, hash ^ 1);
    let before = save_codec::serialize(save.owner.saved_scenario_rules().unwrap()).unwrap();
    let clock = || LOAD_CLOCK_MS;
    let frontend = frontend_main();
    let cache = TileVariantSelectorCache::default();
    let view = SavedGamePreparationView {
        repository: &save.repository,
        current_simulation: None,
        frontend_main: &frontend,
        rules_owner: &save.owner,
        tile_cache: &cache,
        load_clock: &clock,
    };
    let rejected = save
        .assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bad, &view, assets));
    assert!(matches!(
        rejected,
        Err(PreparedLoadError::Snapshot(
            SnapshotError::RulesMismatch { .. }
        ))
    ));
    let prepared = save
        .assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap();
    // TerrainTypeLoad71E22D binds index0 while CellLoad483B9F restores only
    // the saved known member. The initial saved digest remains mandatory.
    assert_ne!(prepared.resources.rules.simulation_config_hash(), hash);
    assert!(
        prepared
            .resources
            .rules
            .terrain_object_type_case_insensitive("SMOKEY")
            .unwrap()
            .has_occupy_list()
    );
    assert_eq!(
        prepared.resources.rules.terrain_shape_filename("SMOKEY"),
        None
    );
    assert_eq!(
        prepared
            .world
            .simulation
            .terrain_objects()
            .members_at((5, 5))
            .collect::<Vec<_>>(),
        [known_id]
    );
    assert_eq!(
        prepared
            .world
            .simulation
            .terrain_objects()
            .get(unknown_id)
            .unwrap()
            .native_unique_id(),
        native_id
    );
    assert_eq!(prepared.world.simulation.native_identity_cursor(), cursor);
    assert_eq!(
        save_codec::serialize(save.owner.saved_scenario_rules().unwrap()).unwrap(),
        before
    );

    let PreparedSavedResources {
        world,
        resources,
        processed_rules,
        map,
        ..
    } = prepared;
    let (simulation, _) = world.into_committed(SharedCellDummy::fresh(), &frontend);
    save.owner.install_saved_scenario_rules(processed_rules);
    let runtime = SimRuntime {
        simulation,
        resources,
    };
    let loaded_hash = runtime.resources.rules.simulation_config_hash();
    let bytes = snapshot_bytes(&runtime, &save.owner, &map, None, loaded_hash);
    let view = SavedGamePreparationView {
        repository: &save.repository,
        current_simulation: None,
        frontend_main: &frontend,
        rules_owner: &save.owner,
        tile_cache: &cache,
        load_clock: &clock,
    };
    let again = save
        .assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap();
    assert_eq!(again.resources.rules.simulation_config_hash(), loaded_hash);
    assert_eq!(again.world.simulation.native_identity_cursor(), cursor);
    assert_eq!(
        again
            .world
            .simulation
            .terrain_objects()
            .members_at((5, 5))
            .collect::<Vec<_>>(),
        [known_id]
    );
    assert_eq!(
        again
            .world
            .simulation
            .terrain_objects()
            .get(unknown_id)
            .unwrap()
            .native_unique_id(),
        native_id
    );
}

#[test]
fn arctic_type_load_validates_before_mutation_and_resaves_the_same_owner() {
    let Some(mut save) = incoming_save(
        "SNOW",
        "[BlimpBombP]\nImage=ZBOMB\nInviso=yes\nAlternateArcticArt=yes\n\
         [GAWALL]\nImage=SEAL\nAlternateArcticArt=yes\n",
    ) else {
        return;
    };
    let before = save_codec::serialize(save.owner.saved_scenario_rules().unwrap()).unwrap();
    let bullet = save
        .incoming
        .resources
        .rules
        .projectile("BlimpBombP")
        .unwrap();
    assert!(bullet.voxel && bullet.inviso);
    assert_eq!(bullet.image.as_deref(), Some("ZBOMB"));
    let wall = save
        .incoming
        .resources
        .overlay_registry
        .id_for_name("GAWALL")
        .unwrap();
    assert_eq!(
        save.incoming
            .resources
            .overlay_registry
            .flags(wall)
            .unwrap()
            .damage_levels,
        3
    );
    let clock = || LOAD_CLOCK_MS;
    let frontend = frontend_main();
    let cache = TileVariantSelectorCache::default();
    let view = SavedGamePreparationView {
        repository: &save.repository,
        current_simulation: None,
        frontend_main: &frontend,
        rules_owner: &save.owner,
        tile_cache: &cache,
        load_clock: &clock,
    };
    let rejected = save.assets.prepare_saved_load(|assets| {
        PreparedSavedGame::prepare_resources(&save.bad_rules, &view, assets)
    });
    assert!(matches!(
        rejected,
        Err(PreparedLoadError::Snapshot(
            SnapshotError::RulesMismatch { .. }
        ))
    ));
    let rejected = save.assets.prepare_saved_load(|assets| {
        PreparedSavedGame::prepare_resources(&save.bad_follow, &view, assets)
    });
    assert!(matches!(
        rejected,
        Err(PreparedLoadError::LocalView(
            SavedLocalViewError::MissingEntityReference {
                field: "Follow",
                ..
            }
        ))
    ));
    assert_eq!(
        save_codec::serialize(save.owner.saved_scenario_rules().unwrap()).unwrap(),
        before
    );

    let prepared = save
        .assets
        .prepare_saved_load(|assets| {
            PreparedSavedGame::prepare_resources(&save.bytes, &view, assets)
        })
        .unwrap();
    // BulletLoad46C6E1/E8 always requests entry VXL before Arctic SHP,
    // even for saved Voxel/Inviso. Missing ZBOMBA.SHP cannot undo the latch.
    let bullet = prepared.resources.rules.projectile("BlimpBombP").unwrap();
    assert_eq!(bullet.image.as_deref(), Some("ZBOMBA"));
    assert_eq!(bullet.voxel_image_load(), Some("ZBOMB"));
    assert_eq!(bullet.image_loads[0].image, "ZBOMBA");
    assert_eq!(
        prepared.resources.rules.object("GHOST").unwrap().image,
        "SEALA"
    );
    // OverlayLoad5FEB45 followed by InitTheater5BE668 must project the
    // updated image owner into both gameplay flags and physical binding.
    assert_eq!(
        prepared
            .resources
            .overlay_registry
            .flags(wall)
            .unwrap()
            .damage_levels,
        3
    );
    let (filename, _) = crate::rules::effect_asset_catalog::load_overlay_shape(
        &save.assets,
        &prepared.resources.overlay_registry,
        "GAWALL",
        "SNO",
        "SNOW",
    )
    .unwrap();
    assert_eq!(filename, "SEALA.SHP");
    assert_eq!(
        save_codec::serialize(save.owner.saved_scenario_rules().unwrap()).unwrap(),
        before
    );

    let PreparedSavedResources {
        world,
        resources,
        processed_rules,
        map,
        ..
    } = prepared;
    let (simulation, _) = world.into_committed(SharedCellDummy::fresh(), &frontend);
    save.owner.install_saved_scenario_rules(processed_rules);
    let runtime = SimRuntime {
        simulation,
        resources,
    };
    let bytes = snapshot_bytes(
        &runtime,
        &save.owner,
        &map,
        None,
        runtime.resources.rules.simulation_config_hash(),
    );
    let view = SavedGamePreparationView {
        rules_owner: &save.owner,
        repository: &save.repository,
        current_simulation: None,
        frontend_main: &frontend,
        tile_cache: &cache,
        load_clock: &clock,
    };
    let again = save
        .assets
        .prepare_saved_load(|assets| PreparedSavedGame::prepare_resources(&bytes, &view, assets))
        .unwrap();
    let bullet = again.resources.rules.projectile("BlimpBombP").unwrap();
    assert_eq!(
        bullet.image.as_deref(),
        Some("ZBOMBA"),
        "saved latch prevents AA"
    );
    assert_eq!(
        bullet.voxel_image_load(),
        Some("ZBOMBA"),
        "next Load uses its new entry image"
    );
    let (filename, _) = crate::rules::effect_asset_catalog::load_overlay_shape(
        &save.assets,
        &again.resources.overlay_registry,
        "GAWALL",
        "SNO",
        "SNOW",
    )
    .unwrap();
    assert_eq!(filename, "SEALA.SHP", "Overlay's saved latch prevents AA");
}

#[test]
fn retail_saved_resources_prepare_cross_map_and_roll_back_late_failure() {
    let Some(save) = incoming_save("TEMPERATE", "") else {
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
