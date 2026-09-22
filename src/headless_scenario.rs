//! Headless retail-scenario loading for cross-engine parity runs.
//!
//! Loads a real retail map with production rules, art, theater and terrain, then
//! constructs a `Simulation` from an explicit seed — no GPU, no window, no atlases.
//! Depends on `assets`/`rules`/`map`/`sim` only; nothing here may reach into `render`,
//! `ui`, `sidebar`, `audio` or `net`.
//!
//! **Scope.** Construction goes through the same GPU-free funnel the app uses
//! (`sim::runtime::finalize_and_populate_staged_authored_scenario`, on a `Simulation`
//! staged before terrain Fill): map-roster houses are created before objects,
//! terrain objects before map entities, and map-placed units/structures spawn with
//! terrain-attached animations. What a headless scenario still lacks versus an app
//! launch is the launch *session* — skirmish player houses, start-position placement,
//! and atlas-derived voxel animation frame counts (a GPU concern).
//!
//! The seed contract mirrors the original engine: one 32-bit word seeds the scenario and
//! main streams identically, fixed before any setup-phase draw.

use std::path::Path;

use crate::assets::asset_manager::AssetManager;
use crate::map::map_file::{self, MapFile};
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::theater;
use crate::map::tile_variant_selector::TileVariantSelectorCache;
use crate::map::waypoints;
use crate::sim::scenario_bootstrap::ScenarioBootstrapRng;
use crate::sim::scenario_session::ScenarioDescriptor;
use crate::sim::world::Simulation;

fn one_player_battle_launch(
    selected_map_file: &str,
) -> Result<crate::sim::scenario_bootstrap::MatchLaunchDescriptor, String> {
    use crate::skirmish_launch::{
        LaunchCountry, LaunchStartPosition, LaunchTeam, PreFillHouseRoster, SkirmishLaunchMode,
        SkirmishLaunchOptions, SkirmishLaunchSession, SkirmishLocalSlot,
    };

    crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(SkirmishLaunchSession {
        mode: SkirmishLaunchMode {
            id: 1,
            ui_name_key: "GUI:Battle".to_string(),
            tooltip_key: "STT:ModeBattle".to_string(),
            override_file: "MPBattleMD.ini".to_string(),
            map_filter: "standard".to_string(),
            random_maps_allowed: true,
            allies_allowed: true,
            must_ally: false,
        },
        selected_map_file: Some(selected_map_file.to_string()),
        player_name: "Player".to_string(),
        local: SkirmishLocalSlot {
            country: LaunchCountry::America,
            country_random: false,
            color_index: 0,
            color_random: false,
            start_position: LaunchStartPosition::Auto,
            team: LaunchTeam::None,
        },
        opponents: Vec::new(),
        pre_fill_house_roster: PreFillHouseRoster::from_compact_skirmish(0),
        options: SkirmishLaunchOptions::default(),
    })
    .map_err(|error| format!("resolve one-player Battle launch: {error}"))
}

/// A loaded scenario plus the per-tick inputs `advance_tick` needs.
pub struct HeadlessScenario {
    /// The runtime owner (F09): simulation plus its bound immutable
    /// resources - no independently swappable execution inputs remain.
    pub runtime: crate::sim::runtime::SimRuntime,
    pub map: MapFile,
}

impl HeadlessScenario {
    /// Read access for digest/tooling callers.
    pub fn sim(&self) -> &Simulation {
        &self.runtime.simulation
    }
}

/// Native logic-frame cadence (66 ms). NOTE: this does NOT match the
/// client, which steps the sim at `app::types::SIM_TICK_MS` = 22 ms — a
/// recorded 3x tooling divergence; headless digests are not tick-comparable
/// to client runs until it is resolved.
pub const SIM_TICK_MS: u32 = 1000 / crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND;

/// Load `map_file_name` from the retail install at `retail_dir` with a pinned seed.
///
/// `map_file_name` is resolved relative to the retail root (e.g. `"Dustbowl.mmx"`).
pub fn load(retail_dir: &Path, map_file_name: &str, seed: u32) -> Result<HeadlessScenario, String> {
    load_with_launch(
        retail_dir,
        map_file_name,
        seed,
        one_player_battle_launch(map_file_name)?,
    )
}

/// Run the same construction and bound-resource handoff with an explicit
/// resolved launch. The quickplay regression uses its real app descriptor here
/// so house creation and starting forces cannot drift from the tested session.
pub(crate) fn load_with_launch(
    retail_dir: &Path,
    map_file_name: &str,
    seed: u32,
    launch: crate::sim::scenario_bootstrap::MatchLaunchDescriptor,
) -> Result<HeadlessScenario, String> {
    crate::map::retail_trig::install_from_dir(retail_dir);
    if !crate::map::retail_trig::wave_tables_available() {
        return Err(format!(
            "{} does not provide the verified gamemd sine/Acos tables required by stock Sonic Wave simulation",
            retail_dir.join("gamemd.exe").display()
        ));
    }
    let map_path = retail_dir.join(map_file_name);
    let mut map = map_file::load_from_path(&map_path)
        .map_err(|error| format!("parse {}: {error}", map_path.display()))?;

    let mut assets = AssetManager::new(retail_dir).map_err(|error| {
        format!(
            "open retail MIX archives in {}: {error}",
            retail_dir.display()
        )
    })?;
    // Preserve the process-owned cold Rules registry before theater archive
    // priority changes. The same owner then performs the active noncampaign
    // reset/rebuild and transfers its move-only native-ID receipt.
    let (_, _, mut native_rules_owner) =
        crate::app::loading::init_helpers::load_startup_rules(&assets)
            .ok_or_else(|| "load native startup rules".to_string())?
            .into_parts();
    let scenario_prefix_plan =
        crate::sim::scenario_bootstrap::prepare_stock_offline_scenario_prefix_plan(
            &launch,
            &map,
            &map.waypoints,
            seed,
        )
        .map_err(|error| format!("prepare stock-offline scenario prefix: {error}"))?;
    let override_file = launch.session().mode.override_file.trim();
    let (override_bytes, override_source) = assets
        .get_with_source(override_file)
        .ok_or_else(|| format!("load game-mode rules override {override_file}"))?;
    log::info!(
        "Loading game-mode rules override {} ({} bytes) from {}",
        override_file,
        override_bytes.len(),
        override_source,
    );
    let mode_override = crate::rules::ini_parser::IniFile::from_bytes(&override_bytes)
        .map_err(|error| format!("parse game-mode rules override {override_file}: {error}"))?;
    let (mut rules, rules_ini, art_ini, native_rules_receipt) = native_rules_owner
        .load_noncampaign_scenario(Some(&mode_override), &map.ini)
        .map_err(|error| format!("load native noncampaign rules: {error}"))?
        .into_parts();
    let bound_scenario_prefix =
        scenario_prefix_plan.bind_native_rules_receipt(native_rules_receipt);
    let theater = theater::load_theater(&mut assets, &map.header.theater)
        .ok_or_else(|| format!("load theater {}", map.header.theater))?;
    let mut art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    rules.merge_art_data(&art);
    rules.general.resolve_art_rates(&art_ini);
    let infantry_sequences =
        crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art_ini);
    let overlay_registry = OverlayTypeRegistry::from_ini(&rules_ini, Some(&art_ini));
    let house_roster = crate::map::houses::parse_house_roster(
        &map.ini,
        rules.color_schemes.as_slice(),
        Some(&rules),
    );
    let lighting_profiles = crate::map::lighting::parse_lighting_profiles(&map.ini);
    let native_start_bounds =
        crate::sim::scenario_bootstrap::NativeStartBounds::from_map_header(&map.header)
            .ok_or_else(|| "map Size does not produce a valid fresh cell array".to_string())?;
    let scenario_cell_extent = native_start_bounds
        .min_rx
        .checked_add(native_start_bounds.width)
        .ok_or_else(|| "fresh cell-array extent overflow".to_string())?;
    let descriptor = ScenarioDescriptor {
        seed,
        map_name: map_file_name.to_string(),
        theater: map.header.theater.clone(),
        game_mode_nonzero: true,
        no_damage: false,
        free_radar: map.basic.free_radar.unwrap_or(false),
        // Skirmish start forces `TiberiumGrows|TiberiumSpreads` (`OR 0xC0`
        // at `0x005E74CD`), copied into the scenario at `0x00687C23`.
        tiberium_grows_flag: true,
        tiberium_spreads_flag: true,
        map_width: scenario_cell_extent,
        map_height: scenario_cell_extent,
        local_left: map.header.local_left as u16,
        local_top: map.header.local_top as u16,
        local_width: map.header.local_width as u16,
        local_height: map.header.local_height as u16,
        mp_start_waypoints: waypoints::multiplayer_start_waypoints(
            bound_scenario_prefix
                .projection()
                .active_scenario_waypoints(),
        )
        .into_iter()
        .map(|wp| (wp.index, (wp.rx, wp.ry)))
        .collect(),
        pixel_conversion_bounds: Default::default(),
        lighting: crate::sim::scenario_session::ScenarioLightingState::new(
            crate::sim::scenario_session::ScenarioLightProfileUnits {
                ambient_percent: lighting_profiles.normal.ambient_percent,
                red_percent: lighting_profiles.normal.red_percent,
                green_percent: lighting_profiles.normal.green_percent,
                blue_percent: lighting_profiles.normal.blue_percent,
                ground_units: lighting_profiles.normal.ground_units,
                level_units: lighting_profiles.normal.level_units,
            },
            crate::sim::scenario_session::ScenarioLightProfileUnits {
                ambient_percent: lighting_profiles.ion.ambient_percent,
                red_percent: lighting_profiles.ion.red_percent,
                green_percent: lighting_profiles.ion.green_percent,
                blue_percent: lighting_profiles.ion.blue_percent,
                ground_units: lighting_profiles.ion.ground_units,
                level_units: lighting_profiles.ion.level_units,
            },
        ),
    };
    let bootstrap_rng = ScenarioBootstrapRng::new(seed);
    let (mut sim, scenario_prefix_projection) = bootstrap_rng
        .into_stock_offline_staged_simulation(&descriptor, bound_scenario_prefix)
        .map_err(|error| format!("stage stock-offline Simulation: {error}"))?;
    let shared_cell_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();
    shared_cell_dummy.reconstruct_for_map_resize();
    sim.bind_shared_cell_dummy(shared_cell_dummy.clone());
    let (mut scenario_fill_rng, mut variant_main_rng) = sim.terrain_load_draws();
    let mut scenario_fill_ranged =
        |low, high| scenario_fill_rng.next_range_u32_inclusive(low, high);
    let mut variant_draw = || variant_main_rng.next_u32();
    let mut variant_selector_cache = TileVariantSelectorCache::default();
    let mut variant_selector = variant_selector_cache.begin_load(&mut variant_draw);
    let terrain_fill =
        ResolvedTerrainGrid::build_pending_authored_with_variant_selector_and_shared_dummy(
            &map,
            Some(&theater),
            Some(&assets),
            Some(&rules.terrain_rules),
            Some(&overlay_registry),
            true,
            rules.general.cliff_back_impassability,
            &mut scenario_fill_ranged,
            &mut variant_selector,
            shared_cell_dummy,
        );
    drop(variant_selector);
    drop(variant_draw);
    drop(scenario_fill_ranged);
    drop(variant_main_rng);
    drop(scenario_fill_rng);

    // Pending authored Fill has no eager Tile##Anim list. Bind damage-fire
    // roots now and let the authored host bind each actually reached map Anim
    // synchronously before its native constructor spends an ID.
    let scheduler_roots =
        crate::app::loading::init_helpers::scheduler_anim_roots(&rules, &overlay_registry, &[]);
    art.bind_scheduler_anim_assets(
        &scheduler_roots,
        &assets,
        theater.extension,
        &map.header.theater,
    )
    .map_err(|error| format!("bind authoritative animation assets: {error}"))?;
    // Every other AnimClass producer and the building animations: tolerant pass,
    // after the strict one, which rewrites the scheduler-owned set wholesale.
    let unbound_explosion_roots = art.bind_anim_class_assets(
        &crate::app::loading::init_helpers::tolerant_anim_class_roots(&rules, &art),
        &assets,
        theater.extension,
        &map.header.theater,
    );
    crate::rules::effect_asset_catalog::log_unbound_combat_explosion_roots(unbound_explosion_roots);
    let (populated_smudge_dims, fallback_smudge_dims) =
        art.populate_anim_frame_dims(&assets, theater.extension, &map.header.theater);
    log::info!(
        "Anim frame dims: {} populated, {} fallback (defaults to 30x30)",
        populated_smudge_dims,
        fallback_smudge_dims,
    );
    rules.art_registry = art.clone();
    rules.bind_effect_assets(&assets, theater.extension, &map.header.theater);
    rules.bind_terrain_spawner_assets(&rules_ini, &assets, theater.extension, &map.header.theater);
    rules.bind_animation_sequences(&infantry_sequences);
    let overlay_shp_ids = crate::app::loading::init::resolved_overlay_shp_ids(
        &overlay_registry,
        &rules_ini,
        &art,
        &assets,
        theater.extension,
        &map.header.theater,
    );
    sim.construct_native_map_tubes(&map.ini)
        .map_err(|error| format!("construct native [Tubes]: {error}"))?;
    let bridge_mode = crate::map::basic::BridgeDestroyabilityMode::SkirmishOrMultiplayer {
        bridge_destruction: true,
    };
    let signed_new_ini_format = map.basic.new_ini_format.unwrap_or(0);
    let output = crate::sim::runtime::finalize_and_populate_staged_authored_scenario(
        &mut sim,
        &mut map,
        terrain_fill,
        &theater,
        &assets,
        &rules,
        &mut art,
        &overlay_registry,
        &overlay_shp_ids,
        signed_new_ini_format,
        true,
        rules.general.cliff_back_impassability,
        theater.extension,
        &descriptor.theater,
        bridge_mode,
        &descriptor,
        |sim| {
            crate::sim::scenario_bootstrap::initialize_skirmish_launch_houses(
                sim,
                &house_roster,
                &rules,
                &launch,
            );
        },
    )
    .map_err(|error| format!("finalize authored headless load: {error}"))?;
    rules.art_registry = art.clone();
    let resolved_terrain = output.resolved_terrain;
    let overlay_grid = output.overlay_grid;
    let height_map = resolved_terrain.build_height_map();
    let bridge_height_map = resolved_terrain.build_bridge_height_map();
    let _launch_result = crate::sim::scenario_bootstrap::
        apply_pre_fill_scenario_prefix_launch_session_with_overlay_registry(
            &mut sim,
            &map,
            &house_roster,
            &rules,
            &height_map,
            &resolved_terrain,
            &launch,
            &overlay_registry,
            &scenario_prefix_projection,
        );
    crate::app::loading::init_helpers::bind_staged_app_scenario_metadata(
        &mut sim,
        &assets,
        Some(&rules),
        Some(&art),
    );
    let post_map = crate::sim::runtime::finalize_constructed_scenario(
        &mut sim,
        &map,
        &rules,
        &overlay_registry,
        overlay_grid,
        &house_roster,
        Some(&launch),
    );
    if !post_map.navigation_published {
        return Err("publish headless post-map navigation".to_string());
    }
    if sim.path_grid().is_none() {
        return Err("headless post-map navigation is unavailable".to_string());
    }
    Ok(HeadlessScenario {
        runtime: crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources: crate::sim::runtime::SimResources {
                height_map,
                bridge_height_map,
                overlay_registry,
                terrain_template: None,
                rules,
                trigger_graph: map.trigger_graph.clone(),
                triggers: map.triggers.clone(),
                events: map.events.clone(),
                actions: map.actions.clone(),
                waypoints: map.waypoints.clone(),
            },
        },
        map,
    })
}

impl HeadlessScenario {
    /// Advance one committed simulation frame with no player commands,
    /// through the same bound-resource runtime transaction the app uses.
    pub fn tick(&mut self) {
        let _ = self
            .runtime
            .advance_frame(&[], SIM_TICK_MS, crate::sim::world::TickLane::Ordinary)
            .expect("simulation frame failed; prior world mutations remain");
    }
}

#[cfg(test)]
mod retail_construction_tests {
    use super::*;
    use crate::sim::rng::SimRng;

    /// A synthetic map through the load in production order, with production
    /// functions only: stage the one `Simulation`, let terrain Fill draw from
    /// it, then populate it. No theater, assets or rules are needed, which the
    /// retail funnel (`load`) cannot do without.
    ///
    /// Active YR `MapClass::Clear @ 0x00565B00` clears the fixed cell table, then
    /// `MapClass::Resize @ 0x00565C10` allocates the complete Size diamond before
    /// IsoMapPack records overwrite it (allocation loop `0x0056639E..0x00566451`).
    fn stage_fill_populate<F>(
        map: &MapFile,
        theater_name: &str,
        rules: Option<&crate::rules::ruleset::RuleSet>,
        bridge_destroyability_mode: crate::map::basic::BridgeDestroyabilityMode,
        descriptor: ScenarioDescriptor,
        initialize_houses_before_objects: F,
    ) -> (Simulation, ResolvedTerrainGrid)
    where
        F: FnOnce(&mut Simulation),
    {
        // Native Resize constructs a square cell-array extent of SizeW+SizeH;
        // the descriptor carries it before Fill exists, as the app load does.
        let bounds =
            crate::sim::scenario_bootstrap::NativeStartBounds::from_map_header(&map.header)
                .expect("map Size produces a fresh cell array");
        let extent = bounds.min_rx + bounds.width;
        let descriptor = ScenarioDescriptor {
            map_width: extent,
            map_height: extent,
            ..descriptor
        };
        let mut sim = ScenarioBootstrapRng::new(descriptor.seed).into_simulation(&descriptor);
        let resolved = {
            let (mut scenario_fill_rng, mut variant_main_rng) = sim.terrain_load_draws();
            let mut scenario_fill_ranged =
                |low, high| scenario_fill_rng.next_range_u32_inclusive(low, high);
            let mut variant_draw = || variant_main_rng.next_u32();
            let mut variant_selector_cache = TileVariantSelectorCache::default();
            let mut variant_selector = variant_selector_cache.begin_load(&mut variant_draw);
            let shared_cell_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();
            shared_cell_dummy.reconstruct_for_map_resize();
            ResolvedTerrainGrid::build_with_variant_selector_and_shared_dummy(
                map,
                None,
                None,
                None,
                None,
                None,
                false,
                2,
                &mut scenario_fill_ranged,
                &mut variant_selector,
                shared_cell_dummy,
                crate::map::resolved_terrain::OverlayLoadSource::Authored,
                descriptor.pixel_conversion_bounds,
            )
        };
        assert_eq!(
            (resolved.width(), resolved.height()),
            (extent, extent),
            "Fill allocates the extent the descriptor announced"
        );
        let height_map = resolved.build_height_map();
        crate::sim::runtime::populate_staged_scenario_with_generated_inits(
            &mut sim,
            map,
            &resolved,
            theater_name,
            rules,
            &height_map,
            None,
            None,
            bridge_destroyability_mode,
            &descriptor,
            None,
            initialize_houses_before_objects,
        )
        .expect("fixed-map Techno constructor projection cannot fail");
        (sim, resolved)
    }

    /// One valid LZO chunk whose decompressed bytes are the `(0, 0)`
    /// IsoMapPack terminator, so the parsed map has no explicit cell records.
    const EMPTY_ISO_MAP_PACK: &str = "CAAEABUAAAAAEQAA";

    #[test]
    fn free_radar_map_reload_and_recording_callback_rebuild_scenario_authority() {
        use crate::sim::replay::{NativeReplayHeader, NativeReplayStream};
        let rules =
            crate::rules::ruleset::RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
                "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
            ))
            .unwrap();
        // A native recording supplies identity/seed to a normal-load callback.
        // This exercises that API with the actual GPU-free construction funnel;
        // the app's native recording UI/loader is not yet connected to it.
        for (key, expected) in [
            ("FreeRadar=yes\n", true),
            ("", false),
            ("FreeRadar=no\n", false),
            ("FreeRadar=invalid\n", false),
        ] {
            let map_bytes = format!(
                "[Basic]\n{key}[Map]\nTheater=TEMPERATE\nSize=0,0,2,2\nLocalSize=0,0,2,2\nFill=Water\n[IsoMapPack5]\n1={EMPTY_ISO_MAP_PACK}\n"
            );
            let map = MapFile::from_bytes(map_bytes.as_bytes()).unwrap();
            let header = NativeReplayHeader::new(0x12345678, "radar.map");
            let recording = header.encode();
            let playback = NativeReplayStream::initialize(
                &recording,
                NativeReplayHeader::default(),
                |loaded| {
                    let descriptor = ScenarioDescriptor {
                        free_radar: map.basic.free_radar.unwrap_or(false),
                        ..ScenarioDescriptor::from_native_replay_header(loaded)
                    };
                    let (sim, _terrain) = stage_fill_populate(
                        &map,
                        "TEMPERATE",
                        Some(&rules),
                        crate::map::basic::BridgeDestroyabilityMode::CampaignOrEditor,
                        descriptor,
                        |sim| {
                            sim.interner.intern("Americans");
                        },
                    );
                    Ok::<_, ()>(sim)
                },
            )
            .unwrap();
            assert_eq!(playback.state.session.free_radar, expected);
            assert_eq!(
                crate::sim::radar::has_radar_for_owner(&playback.state, &rules, "Americans"),
                expected
            );
            assert_eq!(playback.state.session.map_name, "radar.map");
        }
    }

    #[test]
    fn headless_battle_launch_is_the_exact_single_local_stock_session() {
        let launch = one_player_battle_launch("TEST.MAP").expect("one-player Battle launch");
        let session = launch.session();

        assert_eq!(session.mode.id, 1);
        assert_eq!(session.mode.override_file, "MPBattleMD.ini");
        assert_eq!(session.selected_map_file.as_deref(), Some("TEST.MAP"));
        assert!(session.opponents.is_empty());
        assert_eq!(session.local.color_index, 0);
    }

    #[test]
    fn gsi_04_01_headless_sparse_water_load_materializes_and_draws_on_the_staged_owner() {
        let seed = 0x0401_5EED;
        let map_bytes = format!(
            "[Map]\n\
             Theater=TEMPERATE\n\
             Size=0,0,2,1\n\
             LocalSize=0,0,2,1\n\
             Fill=Water\n\
             [IsoMapPack5]\n\
             1={EMPTY_ISO_MAP_PACK}\n"
        );
        let map = MapFile::from_bytes(map_bytes.as_bytes()).expect("parse sparse INI map");
        assert!(
            map.cells.is_empty(),
            "fixture has no explicit terrain cells"
        );

        let descriptor = ScenarioDescriptor {
            seed,
            local_left: map.header.local_left as u16,
            local_top: map.header.local_top as u16,
            local_width: map.header.local_width as u16,
            local_height: map.header.local_height as u16,
            ..ScenarioDescriptor::default()
        };
        let (sim, resolved) = stage_fill_populate(
            &map,
            &map.header.theater,
            None,
            crate::map::basic::BridgeDestroyabilityMode::SkirmishOrMultiplayer {
                bridge_destruction: true,
            },
            descriptor,
            |_| {},
        );
        assert_eq!(
            (resolved.width(), resolved.height()),
            (3, 3),
            "the canonical cell array spans the Size diamond's highest coordinate"
        );
        let mut allocated: Vec<_> = resolved.iter().map(|cell| (cell.rx, cell.ry)).collect();
        allocated.sort_unstable();
        assert_eq!(allocated, vec![(1, 2), (2, 1), (2, 2)]);

        let mut expected_scenario = SimRng::new(u64::from(seed));
        for _ in 0..3 {
            let _ = expected_scenario.next_range_u32_inclusive(0, 3);
        }
        let state = sim.rng_state();
        assert_eq!(state.scenario, expected_scenario.logical_state());
        assert_eq!(
            state.main,
            SimRng::new(u64::from(seed)).logical_state(),
            "no theater TMP selection means the Main cursor stays at its seed"
        );
    }

    /// F09 certification: the shared GPU-free funnel produces a deterministic,
    /// fully populated headless scenario on a retail map. Two loads of the same
    /// map and seed must yield identical parity digests at construction and on
    /// every tick, and the construction gains the funnel promises — stock launch
    /// houses, overlay/smudge/bridge authority, published navigation — must all
    /// be present. App-vs-headless assembly drift is prevented structurally:
    /// both call `finalize_and_populate_staged_authored_scenario` followed by
    /// `finalize_constructed_scenario`.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_headless_funnel_construction_is_deterministic_and_populated() {
        let ra2 = std::path::PathBuf::from(
            std::env::var("RA2_DIR").expect("set RA2_DIR to the retail RA2/YR install directory"),
        );
        let seed = 0x00C0_FFEE;
        let mut a = load(&ra2, "Dustbowl.mmx", seed).expect("first headless load");
        let mut b = load(&ra2, "Dustbowl.mmx", seed).expect("second headless load");

        assert_eq!(
            a.sim().parity_digest(),
            b.sim().parity_digest(),
            "same map+seed must produce an identical construction fingerprint"
        );
        assert!(
            !a.sim().houses.is_empty(),
            "map-roster houses must be constructed before objects"
        );
        assert!(
            a.sim().overlay_grid.is_some(),
            "overlay authority must be installed by the shared finalization"
        );
        assert!(
            a.sim().smudge_grid.is_some(),
            "smudge authority must be installed by the shared finalization"
        );
        assert!(
            a.sim().bridge_state.is_some(),
            "bridge runtime state must be constructed by the funnel"
        );
        assert!(
            a.sim().authored_tiberium_value_total.is_some(),
            "authored Full_Init stores the value-only InitCellAttributes(0) total"
        );

        for tick in 0..30u32 {
            a.tick();
            b.tick();
            assert_eq!(
                a.sim().parity_digest(),
                b.sim().parity_digest(),
                "runtime-backed headless execution diverged at tick {tick}"
            );
        }
    }
}
