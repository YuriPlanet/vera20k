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
//! terrain-attached animations. A Battle launch session ([`battle_session`]) creates
//! the player houses, the local one and any computer opponents, and places them on
//! their starts. What a headless scenario still lacks versus an app launch is
//! atlas-derived voxel animation frame counts (a GPU concern).
//!
//! The seed contract mirrors the original engine: one 32-bit word seeds the scenario and
//! main streams identically, fixed before any setup-phase draw.

#[cfg(any(test, feature = "test-support"))]
pub mod radiation_test_evidence;

use std::path::Path;

use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
use crate::map::map_file::MapFile;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::map::theater;
use crate::map::tile_variant_selector::TileVariantSelectorCache;
use crate::map::waypoints;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::sim::intern::InternedId;
use crate::sim::scenario_bootstrap::ScenarioBootstrapRng;
use crate::sim::scenario_session::ScenarioDescriptor;
use crate::sim::world::Simulation;
use crate::skirmish_launch::{AiDifficulty, LaunchCountry, SkirmishLaunchSession};

/// A Battle session on `selected_map_file`: the local player against one
/// computer house for each of `opponents` (its country and difficulty), every
/// house on a start the launch draws. The lobby allows seven opponents.
pub fn battle_session(
    selected_map_file: &str,
    opponents: &[(LaunchCountry, AiDifficulty)],
) -> SkirmishLaunchSession {
    use crate::skirmish_launch::{
        LaunchStartPosition, LaunchTeam, PreFillHouseRoster, SkirmishAiSlot, SkirmishLaunchMode,
        SkirmishLaunchOptions, SkirmishLocalSlot,
    };

    SkirmishLaunchSession {
        mode: SkirmishLaunchMode {
            class: crate::skirmish_modes::MpModeClass::Battle,
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
        humans: vec![crate::skirmish_launch::SkirmishHumanSlot {
            raw_slot_index: Some(0),
            name: "Player".to_string(),
            slot: SkirmishLocalSlot {
                country: LaunchCountry::America,
                country_random: false,
                color_index: 0,
                color_random: false,
                start_position: LaunchStartPosition::Auto,
                team: LaunchTeam::None,
            },
            role: crate::skirmish_launch::LaunchHumanRole::from_observer(false),
        }],
        local_player_index: 0,
        ai_player_count: opponents.len() as i32,
        opponents: opponents
            .iter()
            .enumerate()
            .map(|(slot, &(country, difficulty))| SkirmishAiSlot {
                raw_slot_index: (slot + 1) as u8,
                country,
                country_random: false,
                color_index: slot as u8 + 1,
                color_random: false,
                start_position: LaunchStartPosition::Auto,
                team: LaunchTeam::None,
                difficulty,
            })
            .collect(),
        pre_fill_house_roster: PreFillHouseRoster::from_compact_skirmish(opponents.len()),
        options: SkirmishLaunchOptions::default(),
    }
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

    /// Add a human player's house of `country` after the load, built by the
    /// launch's own house constructor with the session's credits and tech
    /// level. The launch gave it no start, starting force or alliance, so it
    /// is at war with every house.
    pub fn add_human_house(&mut self, owner_name: &str, country: LaunchCountry) -> InternedId {
        let slot = crate::sim::scenario_bootstrap::NormalizedSkirmishSlot {
            owner_name: owner_name.to_string(),
            country,
            color_index: 0,
            start_position: crate::skirmish_launch::LaunchStartPosition::Auto,
            team: crate::skirmish_launch::LaunchTeam::None,
            is_human: true,
            observer: false,
            source_human_index: None,
            handle: None,
            difficulty: crate::sim::house_state::HouseDifficulty::Normal,
        };
        let runtime = &mut self.runtime;
        crate::sim::scenario_bootstrap::populate_launch_houses(
            &mut runtime.simulation,
            std::slice::from_ref(&slot),
            &runtime.resources.rules,
        );
        runtime
            .simulation
            .interner
            .get(owner_name)
            .expect("the house constructor interns its name")
    }
}

/// Native logic-frame cadence (66 ms). NOTE: this does NOT match the
/// client, which steps the sim at `app::types::SIM_TICK_MS` = 22 ms — a
/// recorded 3x tooling divergence; headless digests are not tick-comparable
/// to client runs until it is resolved.
pub const SIM_TICK_MS: u32 = 1000 / crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND;

/// Load `map_file_name` from the retail install at `retail_dir` with a pinned seed.
///
/// `map_file_name` uses the game's shared loose/MIX source resolver (e.g.
/// `"Dustbowl.mmx"` or `"XMP03T4.MAP"`).
pub fn load(retail_dir: &Path, map_file_name: &str, seed: u32) -> Result<HeadlessScenario, String> {
    load_session(
        retail_dir,
        map_file_name,
        seed,
        battle_session(map_file_name, &[]),
    )
}

/// Load `map_file_name` as `session` launches it (see [`battle_session`]).
pub fn load_session(
    retail_dir: &Path,
    map_file_name: &str,
    seed: u32,
    session: SkirmishLaunchSession,
) -> Result<HeadlessScenario, String> {
    let launch = crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(session)
        .map_err(|error| format!("resolve Battle launch: {error}"))?;
    load_with_launch(retail_dir, map_file_name, seed, launch)
}

/// Run the same construction and bound-resource handoff with an explicit
/// resolved launch. The quickplay regression uses its real app descriptor here
/// so house creation and starting forces cannot drift from the tested session.
pub fn load_with_launch(
    retail_dir: &Path,
    map_file_name: &str,
    seed: u32,
    launch: crate::sim::scenario_bootstrap::MatchLaunchDescriptor,
) -> Result<HeadlessScenario, String> {
    load_with_launch_and_ini(retail_dir, map_file_name, seed, launch, &Default::default())
}

/// Explicit host policy for non-YR callers; no hidden cwd/config reread.
/// Rules, fixed Art and AI all consume the one resolved provider choice.
pub fn load_with_launch_and_ini(
    retail_dir: &Path,
    map_file_name: &str,
    seed: u32,
    launch: crate::sim::scenario_bootstrap::MatchLaunchDescriptor,
    ini: &crate::util::ini_roots::IniRootConfig,
) -> Result<HeadlessScenario, String> {
    let mut assets = AssetManager::new(retail_dir, MediaArchiveMode::STOCK_DIGITAL)
        .and_then(|assets| assets.with_ini_roots(ini))
        .map_err(|error| {
            format!(
                "open retail MIX archives in {}: {error}",
                retail_dir.display()
            )
        })?;
    let mut map = crate::map::source::load_map_by_name_or_path_with_assets(
        retail_dir,
        map_file_name,
        &assets,
    )
    .map_err(|error| format!("load map {map_file_name}: {error}"))?
    .map;
    // Preserve the process-owned cold Rules registry before theater archive
    // priority changes. The same owner then performs the active noncampaign
    // reset/rebuild and transfers its move-only native-ID receipt.
    let audio_definitions = crate::rules::audio_sources::AudioDefinitions::select(&assets);
    let (_, _, mut native_rules_owner) = crate::rules::retail_sources::load_startup_rules(
        &assets,
        std::sync::Arc::clone(audio_definitions.sounds()),
        std::sync::Arc::clone(audio_definitions.eva()),
    )
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
    let mode_override = crate::rules::retail_sources::select_mode_ini(
        &assets,
        Some(&launch.session().mode.override_file),
    )?;
    let (mut rules, rules_ini, art_ini, native_rules_receipt) = native_rules_owner
        .load_scenario(
            crate::rules::process_owner::NativeScenarioRulesPrefix::NonCampaign(
                mode_override.as_ref(),
            ),
            &map.ini,
        )
        .map_err(|error| format!("load native noncampaign rules: {error}"))?
        .into_parts();
    let bound_scenario_prefix = scenario_prefix_plan
        .bind_native_rules_receipt(native_rules_receipt, &launch, &rules)
        .map_err(|error| format!("admit rebuilt launch Countries: {error}"))?;
    let team_ai_registry =
        crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(&assets, &map.ini, true)?;
    let theater = theater::load_theater(&mut assets, &map.header.theater)
        .ok_or_else(|| format!("load theater {}", map.header.theater))?;
    rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art_ini));
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
        // Non-campaign: the session staging word's Harvester Truce byte
        // (`0x005E7495..0x005E74D4`) replaces the map's bit at
        // `0x00687C16..0x00687C29`.
        harvester_immune: launch.session().options.harvester_truce,
        free_radar: map.basic.free_radar.unwrap_or(false),
        ignore_global_ai_triggers: map.basic.ignore_global_ai_triggers.unwrap_or(false),
        train_crate: map.basic.train_crate.unwrap_or(false),
        truck_crate: map.basic.truck_crate.unwrap_or(false),
        ice_growth_enabled: map.basic.ice_growth_enabled,
        vein_growth_enabled: map.basic.vein_growth_enabled,
        // Skirmish start forces `TiberiumGrows|TiberiumSpreads` (`OR 0xC0`
        // at `0x005E74CD`), copied into the scenario at `0x00687C23`.
        tiberium_grows_flag: true,
        tiberium_spreads_flag: true,
        map_width: scenario_cell_extent,
        map_height: scenario_cell_extent,
        mp_start_waypoints: waypoints::multiplayer_start_waypoints(
            bound_scenario_prefix
                .projection()
                .active_scenario_waypoints(),
        )
        .into_iter()
        .map(|wp| (wp.index, (wp.rx, wp.ry)))
        .collect(),
        pixel_conversion_bounds: Default::default(),
        lighting: crate::sim::scenario_session::ScenarioLightingState::from_map(&lighting_profiles),
    };
    let bootstrap_rng = ScenarioBootstrapRng::new(seed);
    let (mut sim, scenario_prefix_projection) = bootstrap_rng
        .into_stock_offline_staged_simulation(&descriptor, bound_scenario_prefix)
        .map_err(|error| format!("stage stock-offline Simulation: {error}"))?;
    let shared_cell_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();
    shared_cell_dummy.reconstruct_for_map_resize();
    sim.bind_shared_cell_dummy(shared_cell_dummy.clone());
    sim.session.install_waypoints(&map.waypoints);
    sim.initialize_trigger_definitions(
        &map,
        &team_ai_registry,
        Some(audio_definitions.eva()),
        Some(audio_definitions.sounds().as_ref()),
    );
    let (mut scenario_fill_rng, variant_main_rng) = sim.terrain_load_draws();
    let mut scenario_fill_ranged =
        |low, high| scenario_fill_rng.next_range_u32_inclusive(low, high);
    let mut variant_draw = move || variant_main_rng.next_u32();
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
    drop(variant_draw);

    // Pending authored Fill has no eager Tile##Anim list. Bind damage-fire
    // roots now and let the authored host bind each actually reached map Anim
    // synchronously before its native constructor spends an ID.
    let mut scheduler_roots =
        crate::map::resolved_terrain::scheduler_anim_roots(&rules, &overlay_registry, &[]);
    scheduler_roots.extend(crate::map::actions::trigger_anim_class_roots(
        &rules,
        &map.actions,
    ));
    rules
        .bind_scheduler_anim_assets(
            &scheduler_roots,
            &assets,
            theater.extension,
            &map.header.theater,
        )
        .map_err(|error| format!("bind authoritative animation assets: {error}"))?;
    // Every other AnimClass producer and the building animations: tolerant pass,
    // after the strict one, which rewrites the scheduler-owned set wholesale.
    let tolerant_roots = crate::rules::effect_asset_catalog::tolerant_anim_class_roots(&rules);
    let unbound_explosion_roots = rules.bind_anim_class_assets(
        &tolerant_roots,
        &assets,
        theater.extension,
        &map.header.theater,
    );
    crate::rules::effect_asset_catalog::log_unbound_combat_explosion_roots(unbound_explosion_roots);
    let (populated_smudge_dims, fallback_smudge_dims) =
        rules.populate_anim_frame_dims(&assets, theater.extension, &map.header.theater);
    log::info!(
        "Anim frame dims: {} populated, {} fallback (defaults to 30x30)",
        populated_smudge_dims,
        fallback_smudge_dims,
    );
    rules.bind_effect_assets(&assets, theater.extension, &map.header.theater);
    rules.bind_building_buildup_assets(&assets, &map.header.theater);
    rules.bind_building_rubble_assets(&assets, theater.extension, &map.header.theater);
    rules.bind_terrain_spawner_assets(&rules_ini, &assets, theater.extension, &map.header.theater);
    rules.bind_animation_sequences(&infantry_sequences);
    let overlay_shp_ids = crate::rules::effect_asset_catalog::resolved_overlay_shp_ids(
        &overlay_registry,
        &rules_ini,
        rules.art(),
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
    let cliff_back_impassability = rules.general.cliff_back_impassability;
    let output = crate::sim::runtime::finalize_and_populate_staged_authored_scenario(
        &mut sim,
        &mut map,
        terrain_fill,
        &theater,
        &assets,
        &mut rules,
        &overlay_registry,
        &overlay_shp_ids,
        signed_new_ini_format,
        true,
        cliff_back_impassability,
        theater.extension,
        &descriptor.theater,
        bridge_mode,
        &descriptor,
        |sim, rules| {
            crate::sim::scenario_bootstrap::initialize_skirmish_launch_houses(
                sim,
                &house_roster,
                rules,
                &launch,
            );
        },
    )
    .map_err(|error| format!("finalize authored headless load: {error}"))?;
    let resolved_terrain = output.resolved_terrain;
    let overlay_grid = output.overlay_grid;
    // Match the ordinary app's pre-launch binding: production options need
    // identities for unspawned types too, and snapshots carry this interner.
    sim.intern_rule_type_ids(&rules);
    sim.resolve_type_handles(&rules);
    sim.install_team_ai_registry(&team_ai_registry, &rules)
        .map_err(|refused| format!("active YR aimd.ini failed RuleSet resolution: {refused:?}"))?;
    let _launch_result = crate::sim::scenario_bootstrap::
        apply_pre_fill_scenario_prefix_launch_session_with_overlay_registry(
            &mut sim,
            &map,
            &house_roster,
            &rules,
            &resolved_terrain,
            &launch,
            &overlay_registry,
            &scenario_prefix_projection,
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
    sim.session.publish_started();
    Ok(HeadlessScenario {
        runtime: crate::sim::runtime::SimRuntime {
            simulation: sim,
            resources: crate::sim::runtime::SimResources {
                overlay_registry,
                terrain_template: None,
                rules,
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
            .advance_frame(
                &[],
                SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
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
        // This synthetic fixture omits the production native Rules prefix.
        sim.native_unique_ids =
            Some(crate::sim::native_identity::NativeUniqueIdCursor::for_synthetic_simulation());
        let resolved = {
            let (mut scenario_fill_rng, variant_main_rng) = sim.terrain_load_draws();
            let mut scenario_fill_ranged =
                |low, high| scenario_fill_rng.next_range_u32_inclusive(low, high);
            let mut variant_draw = move || variant_main_rng.next_u32();
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
        crate::sim::runtime::populate_staged_scenario_with_generated_inits(
            &mut sim,
            map,
            &resolved,
            theater_name,
            rules,
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
                            let americans = sim.interner.intern("Americans");
                            sim.houses
                                .register(crate::sim::house_state::HouseState::new(
                                    americans, 0, None, true, 0, 10,
                                ));
                        },
                    );
                    Ok::<_, ()>(sim)
                },
            )
            .unwrap();
            assert_eq!(playback.state.session.free_radar(), expected);
            // House 0x00508DF0 reads Scenario FreeRadar when the House's
            // first update rechecks radar.
            let mut sim = playback.state;
            sim.advance_tick(&[], Some(&rules), None, None, 0);
            assert_eq!(
                crate::sim::radar::has_radar_for_owner(&sim, &rules, "Americans"),
                expected
            );
            assert_eq!(sim.session.map_name(), "radar.map");
        }
    }

    #[test]
    fn headless_battle_launch_is_the_exact_single_local_stock_session() {
        let launch = crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(
            battle_session("TEST.MAP", &[]),
        )
        .expect("one-player Battle launch");
        let session = launch.session();

        assert_eq!(session.mode.id, 1);
        assert_eq!(session.mode.override_file, "MPBattleMD.ini");
        assert_eq!(session.selected_map_file.as_deref(), Some("TEST.MAP"));
        assert!(session.opponents.is_empty());
        assert_eq!(session.local_slot().color_index, 0);
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

        let rules = &a.runtime.resources.rules;
        for name in rules
            .infantry_ids
            .iter()
            .chain(&rules.vehicle_ids)
            .chain(&rules.aircraft_ids)
            .chain(&rules.building_ids)
        {
            let id = a
                .sim()
                .interner
                .get(name)
                .expect("bound unspawned type identity");
            assert!(a.sim().interner.resolve(id).eq_ignore_ascii_case(name));
            assert_eq!(
                a.sim().object_type(id, rules).map(|object| &object.id),
                rules.object(name).map(|object| &object.id)
            );
        }
        assert!(
            a.sim().resolved_rule_handles().is_some(),
            "headless combat rule bindings"
        );
        assert_eq!(rules.general.metallic_debris.len(), 15);
        assert_eq!(
            rules.general.metallic_debris.last().map(String::as_str),
            Some("D")
        );
        let d = rules
            .art()
            .anim_runtime_config("D")
            .expect("native unread type receipt");
        assert!(!d.art_body_read);
        assert!(!d.bouncer);
        assert_eq!(d.raw_shp_frame_count, None);
        assert!(rules.art().scheduler_anim_types().contains("D"));
        assert_eq!(
            rules.simulation_config_hash(),
            b.runtime.resources.rules.simulation_config_hash(),
            "each fresh load publishes the same final bound ART owner"
        );

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
            a.sim().authored_tiberium_value_total().is_some(),
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

        // The same pair as a LAN session, whose GameMode takes the multiplayer
        // Queue_AI route: both instances store an identical retail sync
        // checksum every frame.
        for scenario in [&mut a, &mut b] {
            let session = &mut scenario.runtime.simulation.session;
            session.set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Lan);
            session.install_network_timing(10);
        }
        for tick in 0..60u32 {
            let [checksum_a, checksum_b] = [&mut a, &mut b].map(|scenario| {
                tick_with_due_commands(scenario)
                    .multiplayer_checksum
                    .expect("multiplayer route checksums every frame")
            });
            assert!(
                checksum_a.checksum.is_ok(),
                "retail registries cover every display/Logic entry: {checksum_a:?}"
            );
            assert_eq!(
                checksum_a, checksum_b,
                "multiplayer sync checksum diverged at tick {tick}"
            );
        }
    }

    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_authored_terrain_anims_consume_the_final_bound_art_owner() {
        let ra2 = std::path::PathBuf::from(
            std::env::var("RA2_DIR").expect("set RA2_DIR to the retail RA2/YR install directory"),
        );
        // Stock Anytown supplies waterfall tiles. Exercise the full authored
        // finalizer, whose pending Fill has no eagerly constructed tile Anims.
        let scenario = load(&ra2, "XMP03T4.MAP", 0x00C0_FFEE).expect("Anytown headless load");
        let rules = &scenario.runtime.resources.rules;
        let mut terrain_anims = 0;
        for (id, anim) in scenario
            .sim()
            .anims()
            .filter(|(_, anim)| anim.terrain_attached())
        {
            terrain_anims += 1;
            let name = scenario.sim().interner.resolve(anim.type_id());
            let config = rules
                .art()
                .anim_runtime_config(name)
                .expect("bound terrain type");
            assert!(rules.art().scheduler_anim_types().contains(name));
            assert!(config.art_body_read && config.raw_shp_frame_count.is_some());
            assert_eq!(anim.effective_end(), config.end, "{name} final bound End");
            assert_eq!(
                anim.effective_loop_end(),
                config.loop_end,
                "{name} final bound LoopEnd"
            );
            assert!(anim.use_cell_drawer());
            assert!(
                scenario
                    .sim()
                    .anim_display_layer(*id, Some(rules))
                    .is_some()
            );
        }
        assert!(
            terrain_anims > 0,
            "retail finalizer coverage must not be vacuous"
        );
        eprintln!("Anytown final bound ART owner: {terrain_anims} live terrain Anims");
    }

    /// XMP03T4 with the local house idle and three computer houses: Russia
    /// (Hard), Yuri (Normal) and France (Easy). Returns the computer houses.
    fn retail_skirmish() -> (HeadlessScenario, Vec<crate::sim::intern::InternedId>) {
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        retail_battle(
            |_| {},
            &[
                (LaunchCountry::Russia, AiDifficulty::Hard),
                (LaunchCountry::Yuri, AiDifficulty::Normal),
                (LaunchCountry::France, AiDifficulty::Easy),
            ],
        )
    }

    /// XMP03T4 with the local house idle, as `configure` adjusts the battle
    /// session, against the computer `opponents`. Returns the computer houses.
    fn retail_battle(
        configure: impl FnOnce(&mut crate::skirmish_launch::SkirmishLaunchSession),
        opponents: &[(
            crate::skirmish_launch::LaunchCountry,
            crate::skirmish_launch::AiDifficulty,
        )],
    ) -> (HeadlessScenario, Vec<crate::sim::intern::InternedId>) {
        use crate::sim::intern::InternedId;

        let ra2 = std::path::PathBuf::from(
            std::env::var("RA2_DIR").expect("set RA2_DIR to the retail RA2/YR install directory"),
        );
        // A four-start map: fewer start waypoints than houses would seed the
        // extra starts at random, which the lobby never allows.
        let map = "XMP03T4.MAP";
        let mut session = battle_session(map, opponents);
        configure(&mut session);
        let scenario = load_session(&ra2, map, 0x00C0_FFEE, session).unwrap();
        let computers: Vec<InternedId> = scenario
            .sim()
            .houses
            .iter()
            .filter(|(_, house)| !house.is_controlled_by_human(true) && !house.multiplay_passive())
            .map(|(&id, _)| id)
            .collect();
        assert_eq!(computers.len(), opponents.len());
        (scenario, computers)
    }

    /// Whether `entity` aims at an object of another house.
    fn aims_at_another_house(
        sim: &Simulation,
        entity: &crate::sim::game_entity::GameEntity,
    ) -> bool {
        match entity.attack_target.target() {
            Some(crate::sim::combat::TargetKind::Entity(id)) => sim
                .entities()
                .get(id)
                .is_some_and(|target| target.owner() != entity.owner()),
            _ => false,
        }
    }

    /// A retail skirmish: the local house idles while three computer houses
    /// build their bases, create teams from their AI triggers and build what
    /// the teams need. Every 1500 frames it prints each computer house's
    /// teams (with the script action each is on), choices and Foot objects;
    /// each house must have created a team.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_skirmish_computer_houses_build_their_teams() {
        use crate::map::entities::EntityCategory;

        let (mut scenario, computers) = retail_skirmish();
        let mut created = std::collections::BTreeMap::new();
        for frame in 1..=6_000u32 {
            scenario.tick();
            let sim = scenario.sim();
            created.extend(
                sim.team_script_vm
                    .teams_in_order()
                    .map(|team| (team.id(), team.owner())),
            );
            if frame % 1500 != 0 {
                continue;
            }
            for &owner in &computers {
                let house = &sim.houses[&owner];
                let (teams, members) = sim
                    .team_script_vm
                    .teams_in_order()
                    .filter(|team| team.owner() == owner)
                    .fold((0, 0), |(teams, members), team| {
                        (teams + 1, members + team.member_count())
                    });
                let foot = sim
                    .entities()
                    .values()
                    .filter(|entity| {
                        entity.owner() == owner && entity.category() != EntityCategory::Structure
                    })
                    .count();
                let buildings = sim
                    .entities()
                    .values()
                    .filter(|entity| {
                        entity.owner() == owner && entity.category() == EntityCategory::Structure
                    })
                    .count();
                eprintln!(
                    "frame {frame} {}: {buildings} buildings, {foot} foot, {teams} live teams \
                     ({members} members), choices {:?}",
                    sim.interner.resolve(owner),
                    house.ai_unit_choices(),
                );
            }
            for team in sim.team_script_vm.teams_in_order() {
                eprintln!(
                    "  team {} of {}: action {} at line {}, formed {}, {} members",
                    team.id(),
                    sim.interner.resolve(team.owner()),
                    team.current_action(&sim.team_script_vm),
                    team.cursor(),
                    team.formed(),
                    team.member_count(),
                );
            }
            eprintln!("frame {frame}: {} teams created so far", created.len());
        }
        for owner in computers {
            assert!(
                created.values().any(|&creator| creator == owner),
                "{} created no team",
                scenario.sim().interner.resolve(owner)
            );
        }
    }

    /// Spawns `types` for the first house of `country`, in order, on free
    /// infantry cells within 15 cells of its base centre.
    fn spawn_beside_base(
        sim: &mut Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        country: &str,
        types: &[&str],
    ) -> (crate::sim::intern::InternedId, Vec<u64>) {
        let house = house_of(sim, country);
        let (bx, by) = sim.houses[&house]
            .base_center()
            .expect("the base has a centre");
        let cells = (0..900).map(|n| (i32::from(bx) + n % 30 - 15, i32::from(by) + n / 30 - 15));
        let spawned = spawn_on_free_cells(sim, rules, house, cells, types);
        (house, spawned)
    }

    /// The first house of `country`.
    fn house_of(sim: &Simulation, country: &str) -> crate::sim::intern::InternedId {
        let country_id = sim.interner.get(country).expect("retail country interned");
        sim.houses
            .iter()
            .find(|(_, house)| house.country() == Some(country_id))
            .map(|(&id, _)| id)
            .expect("a house of the country")
    }

    /// Spawns `types` for `house`, in order, each on the next of `cells`
    /// that is a free infantry cell.
    fn spawn_on_free_cells(
        sim: &mut Simulation,
        rules: &crate::rules::ruleset::RuleSet,
        house: crate::sim::intern::InternedId,
        cells: impl IntoIterator<Item = (i32, i32)>,
        types: &[&str],
    ) -> Vec<u64> {
        use crate::sim::movement::locomotor::MovementLayer;

        let owner = sim.interner.resolve(house).to_string();
        let free: Vec<(u16, u16)> = cells
            .into_iter()
            .filter_map(|(rx, ry)| {
                let (rx, ry) = (u16::try_from(rx).ok()?, u16::try_from(ry).ok()?);
                let walkable = sim
                    .path_grid()
                    .is_some_and(|grid| grid.is_walkable_for_infantry(rx, ry));
                let empty = sim
                    .substrate
                    .occupancy
                    .get(rx, ry)
                    .is_none_or(|cell| cell.is_empty_on(MovementLayer::Ground));
                (walkable && empty).then_some((rx, ry))
            })
            .collect();
        let mut free = free.into_iter();
        types
            .iter()
            .map(|type_name| {
                free.by_ref()
                    .find_map(|(rx, ry)| {
                        let z = sim.terrain_cell_level(rx, ry).unwrap_or(0);
                        sim.spawn_object_at_height(type_name, &owner, rx, ry, 0, z, rules)
                    })
                    .expect("a free cell")
            })
            .collect()
    }

    /// The cells within 10 of `centre` on both axes, nearest first.
    fn cells_nearest(centre: (i32, i32)) -> Vec<(i32, i32)> {
        let mut offsets: Vec<(i32, i32)> = (0..441).map(|n| (n % 21 - 10, n / 21 - 10)).collect();
        offsets.sort_by_key(|&(dx, dy)| (dx * dx + dy * dy, dy, dx));
        offsets
            .into_iter()
            .map(|(dx, dy)| (centre.0 + dx, centre.1 + dy))
            .collect()
    }

    /// Two retail Yuri TeamTypes whose members are spawned beside the Yuri
    /// house's base once the bases stand: "Yuri Infantry - M1" (`05FFB46C-G`,
    /// script `0,4 / 49 / 0,1`) and "Yuri Engineers" (`08B95EFC-G`,
    /// `54 / 0,6 / 49 / 0,9 / 0,2`). Action 0's scan finds the first team
    /// another house's infantry, which its members attack. For the second it
    /// prints whether an engineer aimed at another house's building: in the
    /// first run that team disbanded before frame 1000, not yet explained.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_yuri_teams_attack() {
        use crate::map::entities::EntityCategory;

        let (mut scenario, _) = retail_skirmish();
        for _ in 0..1500 {
            scenario.tick();
        }
        let runtime = &mut scenario.runtime;
        let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
        let (yuri, spawned) = spawn_beside_base(
            sim,
            &resources.rules,
            "YuriCountry",
            &[
                "INIT",
                "INIT",
                "INIT",
                "INIT",
                "INIT",
                "YENGINEER",
                "YENGINEER",
                "YENGINEER",
            ],
        );
        let factory_scan = crate::sim::world::direct_greatest_threat(
            sim,
            &resources.rules,
            None,
            spawned[5],
            crate::sim::combat::ScanMission::Quarry {
                mask: 0x1000,
                only_target_house_enemy: false,
            },
        );
        eprintln!(
            "a Yuri engineer's quarry-6 scan: {:?}",
            factory_scan.map(|id| sim
                .interner
                .resolve(sim.entities().get(id).unwrap().type_ref()))
        );
        let teams: Vec<u64> = ["05FFB46C-G", "08B95EFC-G"]
            .into_iter()
            .map(|team_type| {
                let id = sim
                    .interner
                    .get(team_type)
                    .expect("retail TeamType interned");
                sim.create_team(id, Some(yuri), &resources.rules)
                    .expect("under the TeamType's Max=")
            })
            .collect();
        let initiate = sim.interner.get("INIT").expect("INIT interned");
        let engineer = sim.interner.get("YENGINEER").expect("YENGINEER interned");
        let aims_at_a_building = |sim: &Simulation, id: u64| {
            let Some(entity) = sim.entities().get(id) else {
                return false;
            };
            aims_at_another_house(sim, entity)
                && matches!(
                    entity.attack_target.target(),
                    Some(crate::sim::combat::TargetKind::Entity(target))
                        if sim.entities().get(target).is_some_and(|target| {
                            target.category() == EntityCategory::Structure
                        })
                )
        };
        let (mut infantry_attacked, mut engineer_attacked) = (false, false);
        for frame in 1..=3_000u32 {
            scenario.tick();
            let sim = scenario.sim();
            infantry_attacked |= spawned.iter().any(|&id| {
                sim.entities().get(id).is_some_and(|entity| {
                    entity.type_ref() == initiate && aims_at_another_house(sim, entity)
                })
            });
            engineer_attacked |= spawned.iter().any(|&id| {
                sim.entities()
                    .get(id)
                    .is_some_and(|entity| entity.type_ref() == engineer)
                    && aims_at_a_building(sim, id)
            });
            let report = if frame <= 1000 {
                frame % 50 == 0
            } else {
                frame % 500 == 0
            };
            if report {
                for &team_id in &teams {
                    let Some(team) = sim.team_script_vm.team(team_id) else {
                        eprintln!("frame {frame}: team {team_id} is gone");
                        continue;
                    };
                    let members: Vec<String> = team
                        .members()
                        .filter_map(|id| sim.entities().get(id))
                        .map(|entity| {
                            format!(
                                "{}@({},{}) m{} aims away {}",
                                sim.interner.resolve(entity.type_ref()),
                                entity.position.rx(),
                                entity.position.ry(),
                                entity.mission.effective().raw(),
                                aims_at_another_house(sim, entity),
                            )
                        })
                        .collect();
                    eprintln!(
                        "frame {frame}: team {team_id} action {} at line {}, formed {}: {members:?}",
                        team.current_action(&sim.team_script_vm),
                        team.cursor(),
                        team.formed(),
                    );
                }
            }
            if infantry_attacked && engineer_attacked {
                break;
            }
        }
        eprintln!("an engineer aimed at another house's building: {engineer_attacked}");
        assert!(
            infantry_attacked,
            "no Initiate aimed at another house's object"
        );
    }

    /// Two retail transport TeamTypes, each in a duel against an idle local
    /// house that starts with only its MCV (`UnitCount=0`), deployed on the
    /// first frame, so that undefended construction yard is the team's
    /// least-threat target (mode 0):
    /// - "Soviet Engineer vs Ally" (`0C143EFC-G`: an HTK and five SENGINEER,
    ///   `58,131086 / 14 / 43 / 47,2 / 8,0 / 49 / 46,131074`), Russia against
    ///   America;
    /// - "Allied Tanya vs Soviet Con" (`0AD4D7BC-G`: a Night Hawk, Tanya,
    ///   three GIs and a Guardian GI, `TransportsReturnOnUnload=yes`,
    ///   `58,7 / 14 / 43 / 47,25 / 8,2 / 46,131097 / 49`), France against
    ///   Russia.
    ///
    /// Each team's members are spawned beside the computer base once it
    /// stands. The infantry board the transport, leaving the team (actions
    /// 14 and 43); it carries them to the yard (47) and unloads them, and
    /// they rejoin the team (8). Five riders fill the Night Hawk, so action
    /// 14 finishes on its no-transport arm without archiving a cell (only a
    /// transport with a seat left reaches `Set_ArchiveTarget @ 0x0070C610`),
    /// and the unload sends it out of the team with no destination.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_transport_teams_load_and_unload() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        for (local, opponent, country, team_type, types) in [
            (
                LaunchCountry::America,
                LaunchCountry::Russia,
                "Russians",
                "0C143EFC-G",
                [
                    "HTK",
                    "SENGINEER",
                    "SENGINEER",
                    "SENGINEER",
                    "SENGINEER",
                    "SENGINEER",
                ],
            ),
            (
                LaunchCountry::Russia,
                LaunchCountry::France,
                "French",
                "0AD4D7BC-G",
                ["SHAD", "TANY", "E1", "E1", "E1", "GGI"],
            ),
        ] {
            let (mut scenario, _) = retail_battle(
                |session| {
                    session.local_slot_mut().country = local;
                    session.options.unit_count = 0;
                },
                &[(opponent, AiDifficulty::Hard)],
            );
            let sim = scenario.sim();
            let human = sim
                .houses
                .iter()
                .find(|(_, house)| house.is_controlled_by_human(true))
                .map(|(&id, _)| id)
                .expect("the local house");
            let mcv = sim
                .substrate
                .entities
                .iter_sorted()
                .find(|(_, entity)| entity.owner() == human)
                .map(|(id, _)| id)
                .expect("the local MCV");
            let deploy = CommandEnvelope::new(
                human,
                sim.session.tick() + 1,
                Command::DeployMcv { entity_id: mcv },
            );
            scenario
                .runtime
                .advance_frame(
                    &[deploy],
                    SIM_TICK_MS,
                    crate::sim::world::TickLane::Ordinary,
                    crate::sim::world::FrameEffects::default(),
                )
                .expect("simulation frame failed; prior world mutations remain");
            for _ in 0..1500 {
                scenario.tick();
            }
            let runtime = &mut scenario.runtime;
            let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
            let yards: Vec<String> = sim
                .substrate
                .entities
                .iter_sorted()
                .filter(|(_, entity)| entity.owner() == human)
                .map(|(_, entity)| sim.interner.resolve(entity.type_ref()).to_string())
                .collect();
            eprintln!("{team_type}: the local house owns {yards:?}");
            let (house, spawned) = spawn_beside_base(sim, &resources.rules, country, &types);
            let id = sim
                .interner
                .get(team_type)
                .expect("retail TeamType interned");
            // The computer house may already have made the team; then it
            // recruits the new members.
            let team_id = sim
                .create_team(id, Some(house), &resources.rules)
                .or_else(|| {
                    sim.team_script_vm
                        .teams_in_order()
                        .find(|team| team.team_type_id() == Some(id) && team.owner() == house)
                        .map(|team| team.id())
                })
                .expect("a team of the TeamType");
            let (transport, infantry) = (spawned[0], &spawned[1..]);
            // The first frame all the infantry were aboard the transport and
            // its archive then, the first frame after that they were all
            // members again, and the first frame the transport was not, with
            // its NavCom then.
            let (mut loaded, mut archive, mut unloaded) = (None, None, None);
            let mut transport_left = None;
            let mut present = (true, vec![true; spawned.len()]);
            for frame in 1..=6_000u32 {
                scenario.tick();
                let sim = scenario.sim();
                let team = sim.team_script_vm.team(team_id);
                let aboard = infantry.iter().all(|&id| {
                    sim.entities().get(id).is_some_and(|entity| {
                        entity.passenger_role.inside_transport_id() == Some(transport)
                    })
                });
                if loaded.is_none() && aboard {
                    loaded = Some(frame);
                }
                if loaded.is_some() && unloaded.is_none() {
                    archive = sim
                        .entities()
                        .get(transport)
                        .and_then(|entity| entity.archive_target());
                }
                let rejoined = team.is_some_and(|team| {
                    infantry
                        .iter()
                        .all(|&id| team.members().any(|member| member == id))
                });
                if loaded.is_some() && unloaded.is_none() && rejoined {
                    unloaded = Some(frame);
                }
                let now = (
                    team.is_some(),
                    spawned
                        .iter()
                        .map(|&id| sim.entities().get(id).is_some())
                        .collect::<Vec<_>>(),
                );
                let changed = now != present;
                present = now;
                if frame % 250 == 0 || unloaded == Some(frame) || changed {
                    let members: Vec<String> = spawned
                        .iter()
                        .map(|&id| {
                            let Some(entity) = sim.entities().get(id) else {
                                return format!("{id}:gone");
                            };
                            format!(
                                "{}@({},{}) hp{} m{} {} nav {:?}{}",
                                sim.interner.resolve(entity.type_ref()),
                                entity.position.rx(),
                                entity.position.ry(),
                                entity.health.current(),
                                entity.mission.effective().raw(),
                                match sim.team_script_vm.team_for_member(id) {
                                    Some((member_team, _)) if member_team == team_id => "in",
                                    Some(_) => "in another team",
                                    None => "out",
                                },
                                entity.navigation.nav_com(),
                                if entity.passenger_role.is_inside_transport() {
                                    " aboard"
                                } else {
                                    ""
                                },
                            )
                        })
                        .collect();
                    match team {
                        Some(team) => eprintln!(
                            "frame {frame}: {team_type} action {} at line {}: {members:?}",
                            team.current_action(&sim.team_script_vm),
                            team.cursor(),
                        ),
                        None => eprintln!("frame {frame}: {team_type} is gone: {members:?}"),
                    }
                }
                if let Some(done) = unloaded {
                    if transport_left.is_none()
                        && !team
                            .is_some_and(|team| team.members().any(|member| member == transport))
                    {
                        transport_left = Some((
                            frame,
                            sim.entities()
                                .get(transport)
                                .and_then(|entity| entity.navigation.nav_com()),
                        ));
                    }
                    if frame >= done + 60 {
                        break;
                    }
                }
            }
            eprintln!(
                "{team_type}: aboard at {loaded:?}, transport archive {archive:?}, \
                 rejoined after unloading at {unloaded:?}, transport left with its \
                 NavCom at {transport_left:?}"
            );
            assert!(loaded.is_some(), "{team_type}: the infantry never boarded");
            assert!(
                unloaded.is_some(),
                "{team_type}: the infantry never rejoined"
            );
            if team_type == "0AD4D7BC-G" {
                assert_eq!(archive, None, "the full Night Hawk archived a cell");
                assert_eq!(
                    transport_left.map(|(_, nav_com)| nav_com),
                    Some(None),
                    "the Night Hawk left its team with no destination"
                );
            } else {
                assert_eq!(transport_left, None, "the HTK left its team");
            }
        }
    }

    /// Computer engineers capture once hit on Guard, in two XMP03T4 duels
    /// of a Hard Russia against an idle America whose MCV deploys on frame 1:
    /// - a lone Soviet Engineer by the yard, inside an American GI's range:
    ///   shot on Guard, it queues Hunt (`0x00518BF2`), and Mission_Hunt's
    ///   engineer arm (`0x004D53BA`) sends it away from the GI to capture
    ///   what its scan finds;
    /// - "Soviet Engineer vs Ally" (`0C143EFC-G`, an HTK and five Soviet
    ///   Engineers) against three Rhino Tanks by the yard: unloaded, the
    ///   engineers stand on Guard until action 46 gives them Attack, and one
    ///   hit on Guard hunts and captures.
    ///
    /// Each prints when an engineer first hunted and first took Capture, and
    /// must take the American Construction Yard.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_computer_engineers_capture_once_hit() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::mission::MissionType;
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        for lone in [true, false] {
            let (mut scenario, _) = retail_battle(
                |session| {
                    session.local_slot_mut().country = LaunchCountry::America;
                    session.options.unit_count = 0;
                },
                &[(LaunchCountry::Russia, AiDifficulty::Hard)],
            );
            let sim = scenario.sim();
            let human = sim
                .houses
                .iter()
                .find(|(_, house)| house.is_controlled_by_human(true))
                .map(|(&id, _)| id)
                .expect("the local house");
            let mcv = sim
                .substrate
                .entities
                .iter_sorted()
                .find(|(_, entity)| entity.owner() == human)
                .map(|(id, _)| id)
                .expect("the local MCV");
            let deploy = CommandEnvelope::new(
                human,
                sim.session.tick() + 1,
                Command::DeployMcv { entity_id: mcv },
            );
            scenario
                .runtime
                .advance_frame(
                    &[deploy],
                    SIM_TICK_MS,
                    crate::sim::world::TickLane::Ordinary,
                    crate::sim::world::FrameEffects::default(),
                )
                .expect("simulation frame failed; prior world mutations remain");
            for _ in 0..1500 {
                scenario.tick();
            }
            let runtime = &mut scenario.runtime;
            let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
            let yard = sim
                .substrate
                .entities
                .iter_sorted()
                .find(|(_, entity)| entity.owner() == human)
                .map(|(id, _)| id)
                .expect("the local Construction Yard");
            let at = |sim: &Simulation, id| {
                let entity = sim.entities().get(id).expect("spawned");
                (
                    i32::from(entity.position.rx()),
                    i32::from(entity.position.ry()),
                )
            };
            let (yx, yy) = at(sim, yard);
            let russians = house_of(sim, "Russians");
            let engineers = if lone {
                let engineer = spawn_on_free_cells(
                    sim,
                    &resources.rules,
                    russians,
                    cells_nearest((yx - 2, yy - 2)),
                    &["SENGINEER"],
                );
                let (ex, ey) = at(sim, engineer[0]);
                spawn_on_free_cells(
                    sim,
                    &resources.rules,
                    human,
                    cells_nearest((ex - 3, ey - 2)),
                    &["E1"],
                );
                engineer
            } else {
                let (house, spawned) = spawn_beside_base(
                    sim,
                    &resources.rules,
                    "Russians",
                    &[
                        "HTK",
                        "SENGINEER",
                        "SENGINEER",
                        "SENGINEER",
                        "SENGINEER",
                        "SENGINEER",
                    ],
                );
                let id = sim
                    .interner
                    .get("0C143EFC-G")
                    .expect("retail TeamType interned");
                // The computer house may already have made the team; then it
                // recruits the new members.
                sim.create_team(id, Some(house), &resources.rules)
                    .or_else(|| {
                        sim.team_script_vm
                            .teams_in_order()
                            .find(|team| team.team_type_id() == Some(id) && team.owner() == house)
                            .map(|team| team.id())
                    })
                    .expect("a team of the TeamType");
                spawn_on_free_cells(
                    sim,
                    &resources.rules,
                    human,
                    cells_nearest((yx + 3, yy + 3)),
                    &["MTNK", "MTNK", "MTNK"],
                );
                spawned[1..].to_vec()
            };
            let label = if lone { "lone engineer" } else { "0C143EFC-G" };
            let (mut hunted, mut capturing, mut captured) = (None, None, None);
            for frame in 1..=4_000u32 {
                scenario.tick();
                let sim = scenario.sim();
                for &id in &engineers {
                    let Some(entity) = sim.entities().get(id) else {
                        continue;
                    };
                    let current = entity.mission.current().known();
                    if hunted.is_none()
                        && (current == Some(MissionType::Hunt)
                            || entity.mission.queued().known() == Some(MissionType::Hunt))
                    {
                        hunted = Some((frame, id));
                    }
                    if capturing.is_none() && current == Some(MissionType::Capture) {
                        capturing = Some((frame, id));
                    }
                }
                if sim
                    .entities()
                    .get(yard)
                    .is_some_and(|entity| entity.owner() == russians)
                {
                    captured = Some(frame);
                    break;
                }
            }
            eprintln!(
                "{label}: hunted at {hunted:?}, took Capture at {capturing:?}, \
                 captured the yard at {captured:?}"
            );
            assert!(hunted.is_some(), "{label}: no engineer hunted");
            assert!(capturing.is_some(), "{label}: no engineer took Capture");
            assert!(captured.is_some(), "{label}: the yard was not captured");
        }
    }

    /// "Yuri Armor - E1" (`05FC195C-G`: three Lashers and a Gattling Tank,
    /// the "Yuri Unit Hunter" script `54 / 61 / 0,5 / 49 / 0,4 / 0,1`) for a
    /// Normal Yuri against an idle America on XMP03T4. Once the base stands,
    /// a Yuri Tank Bunker is placed on the nearest open ground beside it and
    /// the team's members are spawned there. Action 61 sends the first
    /// member that the bunker admits into it, and the team goes on past it.
    /// Then the stock "Yuri Chaos - M1" (`07C0304C-G`, one Chaos Drone, the
    /// same script) reaches action 61 beside a second, free bunker: the
    /// drone (`Bunkerable=no`) stays in its team and the team goes on.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_yuri_armor_team_mans_a_tank_bunker() {
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) =
            retail_battle(|_| {}, &[(LaunchCountry::Yuri, AiDifficulty::Normal)]);
        for _ in 0..1500 {
            scenario.tick();
        }
        let runtime = &mut scenario.runtime;
        let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
        let yuri = house_of(sim, "YuriCountry");
        let (bx, by) = sim.houses[&yuri]
            .base_center()
            .expect("the base has a centre");
        // A 2x2 foundation inside a free 4x4 square, so a tank can reach it.
        let open = |sim: &Simulation, rx: i32, ry: i32| {
            (-1..3).all(|dy| {
                (-1..3).all(|dx| {
                    let (Ok(x), Ok(y)) = (u16::try_from(rx + dx), u16::try_from(ry + dy)) else {
                        return false;
                    };
                    sim.path_grid()
                        .is_some_and(|grid| grid.is_walkable_for_infantry(x, y))
                        && sim
                            .substrate
                            .occupancy
                            .get(x, y)
                            .is_none_or(|cell| cell.is_empty_on(MovementLayer::Ground))
                })
            })
        };
        let (rx, ry) = cells_nearest((i32::from(bx), i32::from(by)))
            .into_iter()
            .find(|&(rx, ry)| open(&*sim, rx, ry))
            .and_then(|(rx, ry)| Some((u16::try_from(rx).ok()?, u16::try_from(ry).ok()?)))
            .expect("open ground beside the base");
        let z = sim.terrain_cell_level(rx, ry).unwrap_or(0);
        let owner = sim.interner.resolve(yuri).to_string();
        let bunker = sim
            .spawn_object_at_height("NATBNK", &owner, rx, ry, 0, z, &resources.rules)
            .expect("the bunker spawns");
        let (_, members) = spawn_beside_base(
            sim,
            &resources.rules,
            "YuriCountry",
            &["LTNK", "LTNK", "LTNK", "YTNK"],
        );
        let team_type = sim
            .interner
            .get("05FC195C-G")
            .expect("retail TeamType interned");
        // The computer house may already have made the team; then it
        // recruits the new members.
        let team = sim
            .create_team(team_type, Some(yuri), &resources.rules)
            .or_else(|| {
                sim.team_script_vm
                    .teams_in_order()
                    .find(|team| team.team_type_id() == Some(team_type) && team.owner() == yuri)
                    .map(|team| team.id())
            })
            .expect("a team of the TeamType");
        let (mut seeking, mut installed, mut passed) = (None, None, None);
        for frame in 1..=3_000u32 {
            scenario.tick();
            let sim = scenario.sim();
            if seeking.is_none() {
                seeking = members
                    .iter()
                    .find(|&&id| {
                        sim.entities()
                            .get(id)
                            .is_some_and(|entity| entity.radio_contacts.slot(0) == Some(bunker))
                    })
                    .map(|&id| (frame, id));
            }
            if installed.is_none() {
                installed = sim
                    .entities()
                    .get(bunker)
                    .and_then(|entity| entity.bunker.occupant())
                    .filter(|occupant| members.contains(occupant))
                    .map(|occupant| (frame, occupant));
            }
            let line = sim
                .team_script_vm
                .team(team)
                .map(|team| (team.cursor(), team.current_action(&sim.team_script_vm)));
            if passed.is_none() && line.is_none_or(|(cursor, _)| cursor >= 2) {
                passed = Some((frame, line));
            }
            if frame % 250 == 0 {
                eprintln!(
                    "frame {frame}: team line {line:?}, seeking {seeking:?}, installed {installed:?}"
                );
            }
            if installed.is_some() && passed.is_some() {
                break;
            }
        }
        eprintln!(
            "bunker at ({rx},{ry}): sought at {seeking:?}, manned at {installed:?}, \
             the team passed action 61 at {passed:?}"
        );
        assert!(installed.is_some(), "no team member manned the bunker");
        assert!(passed.is_some(), "the team stayed on action 61");

        let runtime = &mut scenario.runtime;
        let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
        let (fx, fy) = cells_nearest((i32::from(bx), i32::from(by)))
            .into_iter()
            .find(|&(rx, ry)| open(&*sim, rx, ry))
            .and_then(|(rx, ry)| Some((u16::try_from(rx).ok()?, u16::try_from(ry).ok()?)))
            .expect("more open ground beside the base");
        let z = sim.terrain_cell_level(fx, fy).unwrap_or(0);
        let free = sim
            .spawn_object_at_height("NATBNK", &owner, fx, fy, 0, z, &resources.rules)
            .expect("the second bunker spawns");
        spawn_beside_base(sim, &resources.rules, "YuriCountry", &["CAOS"]);
        let team_type = sim
            .interner
            .get("07C0304C-G")
            .expect("retail TeamType interned");
        let team = sim
            .create_team(team_type, Some(yuri), &resources.rules)
            .or_else(|| {
                sim.team_script_vm
                    .teams_in_order()
                    .find(|team| team.team_type_id() == Some(team_type) && team.owner() == yuri)
                    .map(|team| team.id())
            })
            .expect("a team of the TeamType");
        let (mut on_61, mut past) = (None, None);
        for frame in 1..=3_000u32 {
            scenario.tick();
            let sim = scenario.sim();
            let Some(line) = sim.team_script_vm.team(team) else {
                break;
            };
            let members: Vec<u64> = line.members().collect();
            if line.cursor() == 1 {
                on_61 = Some((frame, members));
            } else if line.cursor() >= 2 && on_61.is_some() {
                let held = sim.entities().get(free).is_none_or(|bunker| {
                    bunker.bunker.occupant().is_some() || bunker.radio_contacts.slot(0).is_some()
                });
                past = Some((frame, members, held));
                break;
            }
        }
        eprintln!("Chaos Drone team: on action 61 {on_61:?}, past it {past:?}");
        let (_, on_61) = on_61.expect("the Chaos Drone team never reached action 61");
        let (_, members, held) = past.expect("the Chaos Drone team stayed on action 61");
        assert!(
            !on_61.is_empty(),
            "the team reached action 61 without a drone"
        );
        assert_eq!(members, on_61, "a drone left its team at action 61");
        assert!(!held, "the second bunker was taken");
    }

    /// "Soviet SChop vs All" (`07C3092C-G`: two Siege Choppers, script
    /// `54 / 53 / 47,131072 / 9 / 0,9 / 49 / 0,2 / 0,1`) for a Normal Soviet
    /// AI against an idle America on XMP03T4, with two Siege Choppers spawned
    /// beside the standing base for the team.
    ///
    /// The team holds on action 54, as natively. Each chopper reaches the
    /// gather point and hovers there: a `DeployToLand=` simple deployer's
    /// State 2 holds without clearing the moving byte (`0x0054BE09..0x0054BE2F`).
    /// So `Coordinate_Move` keeps its NavCom (`Is_Moving` at `0x006EBDA2`) and
    /// counts it unarrived (`0x006EBF01..0x006EBF13`); only a `BalloonHover=`
    /// member is let off (`0x006EBED5..0x006EBEFB`). Ares bug 895814 reports
    /// this of the game: the AI's Siege Choppers run their script only once an
    /// enemy is within range, and `BalloonHover=yes` cures it. That release is
    /// not reproduced here.
    ///
    /// Put on line 3 instead, as action 6 would, the team's action 9 orders
    /// each chopper (`IsSimpleDeployer=`) to Unload, and they start to land.
    /// Action 9 is done at once, and the team's next attack order takes them
    /// off Unload in the air, as `Coordinate_Attack` does natively
    /// (`0x006EB79A..0x006EB7B5`): Unit vt+0x4E4 (`0x00736D50`) keeps only a
    /// `DeployToFire=` unit, or a computer one deploying into a `TickTank=`
    /// building, on Unload, and the Siege Chopper is neither.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_soviet_siege_chopper_team_holds_then_attacks_undeployed() {
        use crate::sim::mission::MissionType;
        use crate::sim::movement::jumpjet_movement::jumpjet_flight::{STATE_DESCEND, STATE_HOLD};
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) =
            retail_battle(|_| {}, &[(LaunchCountry::Russia, AiDifficulty::Normal)]);
        for _ in 0..1500 {
            scenario.tick();
        }
        let runtime = &mut scenario.runtime;
        let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
        let (soviet, choppers) =
            spawn_beside_base(sim, &resources.rules, "Russians", &["SCHP", "SCHP"]);
        let team_type = sim
            .interner
            .get("07C3092C-G")
            .expect("retail TeamType interned");
        let team = sim
            .create_team(team_type, Some(soviet), &resources.rules)
            .or_else(|| {
                sim.team_script_vm
                    .teams_in_order()
                    .find(|team| team.team_type_id() == Some(team_type) && team.owner() == soviet)
                    .map(|team| team.id())
            })
            .expect("a team of the TeamType");
        // Hovering on Move at its destination in State 2, with a NavCom and
        // the moving byte set (State 1 passes through State 2 at lift-off);
        // the team recruits the choppers on its first frames.
        let holding = |sim: &Simulation, id: u64| {
            sim.entities().get(id).is_some_and(|entity| {
                let at = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
                entity.mission.current().known() == Some(MissionType::Move)
                    && entity.navigation.nav_com().is_some()
                    && entity
                        .locomotor
                        .as_ref()
                        .and_then(|locomotor| locomotor.jumpjet_runtime())
                        .is_some_and(|jumpjet| {
                            let to = jumpjet.destination();
                            jumpjet.phase() == STATE_HOLD
                                && jumpjet.moving()
                                && (to.x, to.y) == (at.x, at.y)
                        })
            })
        };
        let mut held = None;
        for frame in 1..=1_500u32 {
            scenario.tick();
            let sim = scenario.sim();
            let line = sim.team_script_vm.team(team).expect("the team lives");
            assert!(
                line.cursor() <= 0,
                "the team left action 54 at frame {frame}"
            );
            let both = choppers
                .iter()
                .all(|&id| line.members().any(|member| member == id) && holding(sim, id));
            match held {
                None if both => held = Some(frame),
                Some(since) => assert!(
                    both,
                    "a chopper left the team or its hold at frame {frame} (held from {since})"
                ),
                None => {}
            }
        }
        let since = held.expect("the choppers never held at the gather point");
        let sim = scenario.sim();
        let cells: Vec<_> = choppers
            .iter()
            .filter_map(|&id| sim.entities().get(id))
            .map(|entity| {
                (
                    (entity.position.rx(), entity.position.ry()),
                    entity.navigation.nav_com(),
                )
            })
            .collect();
        eprintln!("Siege Chopper team: held on action 54 from frame {since}: {cells:?}");
        assert!(since <= 1_000, "the choppers held only from frame {since}");

        scenario
            .runtime
            .simulation
            .team_script_vm
            .seek_line_for_test(team, 3);
        // The phase of each chopper's jumpjet, its height, whether it is
        // deployed and its mission.
        let state = |sim: &Simulation, id: u64| {
            let entity = sim.entities().get(id).expect("the chopper lives");
            (
                entity
                    .locomotor
                    .as_ref()
                    .and_then(|locomotor| locomotor.jumpjet_runtime())
                    .map(|jumpjet| jumpjet.phase()),
                crate::sim::movement::air_movement::current_fly_height(
                    entity,
                    sim.resolved_terrain.as_ref(),
                ),
                entity.is_deployed(),
                entity.mission.current().known(),
            )
        };
        let (mut on_9, mut past, mut attacking) = (None, None, None);
        for frame in 1..=600u32 {
            scenario.tick();
            let sim = scenario.sim();
            let states: Vec<_> = choppers.iter().map(|&id| state(sim, id)).collect();
            assert!(
                states.iter().all(|state| !state.2),
                "a chopper deployed at frame {frame}: {states:?}"
            );
            let cursor = sim.team_script_vm.team(team).map(|team| team.cursor());
            if on_9.is_none() && cursor == Some(3) {
                on_9 = Some((frame, states.clone()));
            }
            if past.is_none() && on_9.is_some() && cursor.is_none_or(|cursor| cursor >= 4) {
                past = Some(frame);
            }
            if states
                .iter()
                .all(|state| state.3 == Some(MissionType::Attack))
            {
                attacking = Some((frame, cursor, states));
                break;
            }
        }
        eprintln!(
            "Siege Chopper team: on action 9 {on_9:?}, past it at {past:?}, attacking at \
             {attacking:?}"
        );
        let (_, unloading) = on_9.expect("the team never ran action 9");
        assert!(
            unloading.iter().all(|state| {
                state.0 == Some(STATE_DESCEND) && state.3 == Some(MissionType::Unload)
            }),
            "action 9 did not set both choppers landing to unload: {unloading:?}"
        );
        assert!(past.is_some(), "the team stayed on action 9");
        let (_, _, airborne) = attacking.expect("no attack order took the choppers off Unload");
        assert!(
            airborne.iter().all(|state| state.1 > 0),
            "a chopper landed before its attack order: {airborne:?}"
        );
    }

    /// "Soviet MCV" (`043F83EC-G`: one SMCV, script `043F23CC-G`
    /// `54 / 9 / 49`, which the Hard "Soviet MCV - H" trigger makes for a
    /// house without a Construction Yard) for a Hard Soviet AI against an
    /// idle America on XMP03T4. Once the base stands its yard is removed, and
    /// an SMCV is spawned beside the base for the team. The MCV becomes a
    /// yard again, it leaves the team as it does, and the team is done with
    /// its TeamType's `Max=` slot free. The house's own MCV routes act on a
    /// team member too (UnitClass::AI's Hunt queue has no team check), so
    /// the deploy may be theirs: the test prints the MCV's orders.
    /// [`retail_soviet_mcv_team_deploys_its_mcv_by_action_9`] isolates
    /// action 9.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_hard_soviet_mcv_team_rebuilds_its_yard() {
        use crate::sim::mission::MissionType;
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) =
            retail_battle(|_| {}, &[(LaunchCountry::Russia, AiDifficulty::Hard)]);
        for _ in 0..1500 {
            scenario.tick();
        }
        let yards = |sim: &Simulation, owner| -> Vec<u64> {
            sim.entities()
                .values()
                .filter(|entity| {
                    !entity.death.is_dying()
                        && entity.owner() == owner
                        && sim.interner.resolve(entity.type_ref()) == "NACNST"
                })
                .map(crate::sim::game_entity::GameEntity::stable_id)
                .collect()
        };
        let runtime = &mut scenario.runtime;
        let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
        let soviet = house_of(sim, "Russians");
        let lost = yards(sim, soviet);
        assert!(!lost.is_empty(), "the base has a yard");
        for yard in &lost {
            sim.uninit_with_rules(*yard, &resources.rules);
        }
        let (_, spawned) = spawn_beside_base(sim, &resources.rules, "Russians", &["SMCV"]);
        let mcv = spawned[0];
        let team_type = sim
            .interner
            .get("043F83EC-G")
            .expect("retail TeamType interned");
        let team = sim
            .create_team(team_type, Some(soviet), &resources.rules)
            .or_else(|| {
                sim.team_script_vm
                    .teams_in_order()
                    .find(|team| team.team_type_id() == Some(team_type) && team.owner() == soviet)
                    .map(|team| team.id())
            })
            .expect("a team of the TeamType");
        let (mut joined, mut on_9, mut rebuilt, mut done) = (None, None, None, None);
        let mut orders: Vec<(u32, Option<MissionType>, Option<MissionType>)> = Vec::new();
        for frame in 1..=3_000u32 {
            scenario.tick();
            let sim = scenario.sim();
            let line = sim.team_script_vm.team(team);
            if joined.is_none() && line.is_some_and(|line| line.members().any(|id| id == mcv)) {
                joined = Some(frame);
            }
            if on_9.is_none() && line.is_some_and(|line| line.cursor() == 1) {
                on_9 = Some((frame, line.map(|line| line.member_count())));
            }
            if let Some(entity) = sim
                .entities()
                .get(mcv)
                .filter(|entity| !entity.death.is_dying())
            {
                let order = (
                    entity.mission.current().known(),
                    entity.mission.queued().known(),
                );
                if orders.last().is_none_or(|last| (last.1, last.2) != order) {
                    orders.push((frame, order.0, order.1));
                }
            }
            if rebuilt.is_none() && !yards(sim, soviet).is_empty() {
                rebuilt = Some(frame);
            }
            if done.is_none() && line.is_none_or(|line| line.cursor() >= 2) {
                done = Some(frame);
            }
            if rebuilt.is_some() && done.is_some() {
                break;
            }
        }
        let sim = scenario.sim();
        let live = sim
            .team_script_vm
            .teams_in_order()
            .filter(|line| line.team_type_id() == Some(team_type) && line.owner() == soviet)
            .count();
        eprintln!(
            "MCV team: joined at {joined:?}, on action 9 {on_9:?}, yard rebuilt at {rebuilt:?}, \
             done at {done:?}, {live} left of the type; MCV orders (frame, current, queued) \
             {orders:?}"
        );
        assert!(joined.is_some(), "the MCV never joined the team");
        assert!(rebuilt.is_some(), "no yard was rebuilt");
        assert!(done.is_some(), "the team stayed on its script");
        assert_eq!(live, 0, "the TeamType's Max= slot is still taken");
    }

    /// The "Soviet MCV" team (`043F83EC-G`) of a Hard Soviet AI on XMP03T4,
    /// with an SMCV spawned beside the standing base for it and the house's
    /// `AutoBaseBuilding` latch cleared, so neither UnitClass::AI's Hunt
    /// queue nor Mission_Guard's Unload arm deploys the MCV. Put on line 1,
    /// action 9 asks the yard to stand at the MCV's own cell and orders it
    /// to Unload; Mission_Unload deploys it, it leaves the team as a yard, and
    /// the team, empty, is done.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_soviet_mcv_team_deploys_its_mcv_by_action_9() {
        use crate::sim::mission::{MissionId, MissionType};
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) =
            retail_battle(|_| {}, &[(LaunchCountry::Russia, AiDifficulty::Hard)]);
        for _ in 0..1500 {
            scenario.tick();
        }
        let yards = |sim: &Simulation, owner| -> usize {
            sim.entities()
                .values()
                .filter(|entity| {
                    !entity.death.is_dying()
                        && entity.owner() == owner
                        && sim.interner.resolve(entity.type_ref()) == "NACNST"
                })
                .count()
        };
        let runtime = &mut scenario.runtime;
        let (sim, resources) = (&mut runtime.simulation, &runtime.resources);
        let soviet = house_of(sim, "Russians");
        let standing = yards(sim, soviet);
        let house = sim.houses.get_mut(&soviet).expect("the Soviet house");
        house.set_ai_activation_for_test(crate::sim::house_state::HouseAiActivationLatches {
            auto_base_building: false,
            ..house.ai_activation()
        });
        // The first cell near the base where a yard can stand from every
        // origin within a cell of it: the MCV's own cell (action 9's test),
        // Deploy's (-1, -1), and a cell of drift.
        let yard = resources.rules.object("NACNST").expect("retail NACNST");
        let (bx, by) = sim.houses[&soviet]
            .base_center()
            .expect("the base has a centre");
        let mut around: Vec<(i32, i32)> = (0..1681).map(|n| (n % 41 - 20, n / 41 - 20)).collect();
        around.sort_by_key(|&(dx, dy)| (dx * dx + dy * dy, dy, dx));
        let (rx, ry) = around
            .into_iter()
            .map(|(dx, dy)| (i32::from(bx) + dx, i32::from(by) + dy))
            .find(|&(rx, ry)| {
                (-1..=1).all(|dy| {
                    (-1..=1).all(|dx| {
                        let (Ok(x), Ok(y)) = (i16::try_from(rx + dx), i16::try_from(ry + dy))
                        else {
                            return false;
                        };
                        crate::sim::build_site::can_place_building_at(
                            sim,
                            &resources.rules,
                            Some(&resources.overlay_registry),
                            yard,
                            (x, y),
                            None,
                            None,
                            None,
                        )
                    })
                })
            })
            .and_then(|(rx, ry)| Some((u16::try_from(rx).ok()?, u16::try_from(ry).ok()?)))
            .expect("room for a yard near the base");
        let z = sim.terrain_cell_level(rx, ry).unwrap_or(0);
        let owner = sim.interner.resolve(soviet).to_string();
        let mcv = sim
            .spawn_object_at_height("SMCV", &owner, rx, ry, 0, z, &resources.rules)
            .expect("the MCV spawns");
        let team_type = sim
            .interner
            .get("043F83EC-G")
            .expect("retail TeamType interned");
        let team = sim
            .create_team(team_type, Some(soviet), &resources.rules)
            .or_else(|| {
                sim.team_script_vm
                    .teams_in_order()
                    .find(|team| team.team_type_id() == Some(team_type) && team.owner() == soviet)
                    .map(|team| team.id())
            })
            .expect("a team of the TeamType");
        // A full team forms and starts its script over (`0x006E91AD..
        // 0x006E91F5`), so it is put on line 1 once formed.
        let mut joined = None;
        for frame in 1..=60u32 {
            scenario.tick();
            if scenario
                .sim()
                .team_script_vm
                .team(team)
                .is_some_and(|line| line.formed() && line.members().any(|id| id == mcv))
            {
                joined = Some(frame);
                break;
            }
        }
        assert!(joined.is_some(), "the MCV never joined the team");
        scenario
            .runtime
            .simulation
            .team_script_vm
            .seek_line_for_test(team, 1);
        let (mut ordered, mut deployed, mut done) = (None, None, None);
        for frame in 1..=1_500u32 {
            scenario.tick();
            let sim = scenario.sim();
            let line = sim.team_script_vm.team(team);
            if ordered.is_none()
                && line.is_some_and(|line| line.cursor() == 1)
                && sim.entities().get(mcv).is_some_and(|entity| {
                    entity.mission.effective() == MissionId::from_known(MissionType::Unload)
                })
            {
                ordered = Some(frame);
            }
            if deployed.is_none() && yards(sim, soviet) > standing {
                deployed = Some(frame);
            }
            if done.is_none() && line.is_none_or(|line| line.cursor() >= 2) {
                done = Some(frame);
            }
            if deployed.is_some() && done.is_some() {
                break;
            }
        }
        eprintln!(
            "MCV team, house routes off, MCV at ({rx},{ry}): joined at {joined:?}, Unload on \
             action 9 at {ordered:?}, deployed at {deployed:?}, done at {done:?}"
        );
        assert!(
            ordered.is_some(),
            "action 9 never ordered the MCV to Unload"
        );
        assert!(deployed.is_some(), "the MCV did not deploy");
        assert!(done.is_some(), "the team stayed on its script");
    }

    /// Advances one frame with the commands now due, as `tick` does with none.
    fn tick_with_due_commands(
        scenario: &mut HeadlessScenario,
    ) -> crate::sim::world::SimFrameOutput {
        let commands = scenario.runtime.simulation.take_due_commands();
        scenario
            .runtime
            .advance_frame(
                &commands,
                SIM_TICK_MS,
                crate::sim::world::TickLane::Ordinary,
                crate::sim::world::FrameEffects::default(),
            )
            .expect("simulation frame failed")
    }

    /// A Normal Soviet house's Rhino, in no team, shot by a GI beside it:
    /// `UnitClass::ReceiveDamage`'s computer-owner response gives it the GI
    /// as its NavCom and queues Move (`combat::unit_damage_response`), it
    /// drives off its cell at the GI, and crushes it.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_computer_rhino_runs_at_the_gi_that_shoots_it() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::components::NavTargetRef;
        use crate::sim::mission::{MissionId, MissionType};
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) =
            retail_battle(|_| {}, &[(LaunchCountry::Russia, AiDifficulty::Normal)]);
        for _ in 0..60 {
            scenario.tick();
        }
        let runtime = &mut scenario.runtime;
        let (sim, rules) = (&mut runtime.simulation, &runtime.resources.rules);
        let (_, spawned) = spawn_beside_base(sim, rules, "Russians", &["HTNK"]);
        let rhino = spawned[0];
        let start = {
            let entity = sim.entities().get(rhino).expect("the Rhino");
            (entity.position.rx(), entity.position.ry())
        };
        let america = house_of(sim, "Americans");
        let gi = spawn_on_free_cells(
            sim,
            rules,
            america,
            cells_nearest((i32::from(start.0), i32::from(start.1))),
            &["E1"],
        )[0];
        let tick = sim.session.tick();
        sim.queue_command(CommandEnvelope::new(
            america,
            tick + 1,
            Command::Attack {
                attacker_id: gi,
                target_id: rhino,
            },
        ));
        let full = sim.entities().get(rhino).unwrap().health.current();
        let move_mission = MissionId::from_known(MissionType::Move);
        let (mut hit, mut charged, mut moved, mut died) = (None, None, None, None);
        let (mut gi_cell, mut crushed) = (None, false);
        for frame in 1..=600u32 {
            let output = tick_with_due_commands(&mut scenario);
            let sim = scenario.sim();
            let entity = sim.entities().get(rhino).expect("the Rhino lives");
            if hit.is_none() && entity.health.current() < full {
                assert!(
                    sim.team_script_vm.team_for_member(rhino).is_none(),
                    "the Rhino joined a team before the hit"
                );
                hit = Some(frame);
            }
            if charged.is_none()
                && entity.navigation.nav_com() == Some(NavTargetRef::object(gi))
                && (entity.mission.queued() == move_mission
                    || entity.mission.current() == move_mission)
            {
                charged = Some(frame);
            }
            if charged.is_some()
                && moved.is_none()
                && (entity.position.rx(), entity.position.ry()) != start
            {
                moved = Some(frame);
            }
            match sim
                .entities()
                .get(gi)
                .filter(|gi| !gi.death.is_dying() && gi.health.current() > 0)
            {
                Some(gi) => gi_cell = Some((gi.position.rx(), gi.position.ry())),
                None => {
                    died = Some(frame);
                    // The per-cell crush plays the victim's `CrushSound=` at
                    // the crusher's cell, the victim's own.
                    crushed = output.sound_events.iter().any(|event| {
                        matches!(
                            event,
                            crate::sim::world::SimSoundEvent::EntityCrushed { rx, ry, .. }
                                if gi_cell == Some((*rx, *ry))
                        )
                    });
                    break;
                }
            }
        }
        eprintln!(
            "hit {hit:?}, charged {charged:?}, moved {moved:?}, GI died {died:?}, crushed {crushed}"
        );
        assert!(hit.is_some(), "the GI never hit the Rhino");
        assert!(
            charged >= hit,
            "the Rhino never took the GI as its NavCom on Move"
        );
        assert!(moved.is_some(), "the Rhino never left its cell");
        assert!(died.is_some(), "the GI survived");
        assert!(crushed, "the GI died, but not under the Rhino");
    }

    /// A Normal Soviet house's Rhino sent to Attack a GI beside it drives over
    /// it: `UnitClass::Approach_Target`'s computer crush arm gives it the GI
    /// as its destination (`movement::foot_approach::ApproachCrush`).
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_computer_rhino_drives_over_the_gi_it_attacks() {
        use crate::sim::combat::TargetKind;
        use crate::sim::components::NavTargetRef;
        use crate::sim::mission::{MissionId, MissionType};
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) =
            retail_battle(|_| {}, &[(LaunchCountry::Russia, AiDifficulty::Normal)]);
        for _ in 0..60 {
            scenario.tick();
        }
        let runtime = &mut scenario.runtime;
        let (sim, rules) = (&mut runtime.simulation, &runtime.resources.rules);
        let (_, spawned) = spawn_beside_base(sim, rules, "Russians", &["HTNK"]);
        let rhino = spawned[0];
        let start = {
            let entity = sim.entities().get(rhino).expect("the Rhino");
            (entity.position.rx(), entity.position.ry())
        };
        let america = house_of(sim, "Americans");
        let gi = spawn_on_free_cells(
            sim,
            rules,
            america,
            cells_nearest((i32::from(start.0), i32::from(start.1))),
            &["E1"],
        )[0];
        sim.assign_target_represented(
            rhino,
            Some(TargetKind::Entity(gi)),
            Some(rules),
            None,
            crate::sim::world::FrameEffects::default(),
        )
        .expect("the Rhino takes the GI as its target");
        let now = sim.session.binary_frame();
        sim.substrate
            .mission_assign_exact(rhino, MissionId::from_known(MissionType::Attack), now)
            .expect("the Rhino takes Attack");
        let _ = sim.substrate.mission_commence_exact(rhino, now);
        let (mut charged, mut died, mut gi_cell, mut crushed) = (None, None, None, false);
        for frame in 1..=600u32 {
            let output = tick_with_due_commands(&mut scenario);
            let sim = scenario.sim();
            let entity = sim.entities().get(rhino).expect("the Rhino lives");
            if charged.is_none() && entity.navigation.nav_com() == Some(NavTargetRef::object(gi)) {
                charged = Some(frame);
            }
            match sim
                .entities()
                .get(gi)
                .filter(|gi| !gi.death.is_dying() && gi.health.current() > 0)
            {
                Some(gi) => gi_cell = Some((gi.position.rx(), gi.position.ry())),
                None => {
                    died = Some(frame);
                    crushed = output.sound_events.iter().any(|event| {
                        matches!(
                            event,
                            crate::sim::world::SimSoundEvent::EntityCrushed { rx, ry, .. }
                                if gi_cell == Some((*rx, *ry))
                        )
                    });
                    break;
                }
            }
        }
        // Its first Attack tick, the frame after Commence, takes the charge.
        assert_eq!(charged, Some(1), "the Rhino's first Attack tick");
        assert!(died.is_some(), "the GI survived");
        assert!(crushed, "the GI died, but not under the Rhino");
    }

    /// The local Allied player's Spy sent into a Normal Soviet house's Tesla
    /// Reactor walks in and leaves the game; the Soviet power blacks out for
    /// `SpyPowerBlackout=` frames, and the sim announces it for the local
    /// player's EVA (`sim::spy_infiltration`).
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_player_spy_blacks_out_a_computer_power_plant() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::spy_infiltration::InfiltrationEffect;
        use crate::sim::world::SimSoundEvent;
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) =
            retail_battle(|_| {}, &[(LaunchCountry::Russia, AiDifficulty::Normal)]);
        let soviet = house_of(scenario.sim(), "Russians");
        let mut reactor = None;
        for _ in 0..6000 {
            scenario.tick();
            let sim = scenario.sim();
            reactor = sim
                .entities()
                .values()
                .find(|entity| {
                    entity.owner() == soviet
                        && !entity.death.is_dying()
                        && !entity.lifecycle.in_limbo()
                        && sim.interner.resolve(entity.type_ref()) == "NAPOWR"
                })
                .map(|entity| entity.stable_id());
            if reactor.is_some() {
                break;
            }
        }
        let reactor = reactor.expect("a Tesla Reactor");
        // Past its build-up.
        for _ in 0..300 {
            scenario.tick();
        }
        let runtime = &mut scenario.runtime;
        let (sim, rules) = (&mut runtime.simulation, &runtime.resources.rules);
        let at = {
            let entity = sim.entities().get(reactor).expect("the reactor stands");
            (
                i32::from(entity.position.rx()),
                i32::from(entity.position.ry()),
            )
        };
        let america = house_of(sim, "Americans");
        let cells = cells_nearest(at)
            .into_iter()
            .filter(|&(rx, ry)| (rx - at.0).abs().max((ry - at.1).abs()) >= 3);
        let spy = spawn_on_free_cells(sim, rules, america, cells, &["SPY"])[0];
        let tick = sim.session.tick();
        sim.queue_command(CommandEnvelope::new(
            america,
            tick + 1,
            Command::CaptureBuilding {
                engineer_id: spy,
                target_building_id: reactor,
            },
        ));
        let mut announced = None;
        for _ in 0..600 {
            let output = tick_with_due_commands(&mut scenario);
            announced = announced.or(output.sound_events.iter().find_map(|event| match event {
                SimSoundEvent::BuildingInfiltrated {
                    owner,
                    infiltrator,
                    effect,
                    ..
                } => Some((*owner, *infiltrator, *effect)),
                _ => None,
            }));
            if scenario.sim().entities().get(spy).is_none() {
                break;
            }
        }
        let sim = scenario.sim();
        assert_eq!(
            announced,
            Some((soviet, america, InfiltrationEffect::Power)),
            "the spy never went in"
        );
        assert!(sim.entities().get(spy).is_none(), "the spy stayed");
        let frame = sim.session.binary_frame();
        assert!(sim.power_states[&soviet].blackout_remaining(frame) > 0);
    }

    /// "Allied Spy vs Soviet Power" (`09B5091C-G`: one SPY, the script
    /// `0,4 / 47,131081 / 46,9 / 47,131083 / 46,11`) for a Normal American
    /// house against the idle local Soviet house, which is given a Tesla
    /// Reactor beside its MCV, guarded by a Sentry Gun and a Conscript. The
    /// team's spy is spawned twelve cells from the reactor, towards its own
    /// house's MCV. Action 0 (attack infantry) takes the flat walk over every
    /// object (`0x006F8FE0`) and finds the Conscript, which the `MakeupKit`
    /// (`Range=-2`) reaches from anywhere: its `MakesDisguise=` shot dresses
    /// the spy as a Soviet Conscript, and the `FireOnce=` shot ends the
    /// action. Action 47 brings it beside the reactor past guards that take
    /// it for their own, and action 46 hands it the reactor:
    /// `Assign_Target`'s infiltration arm heads it there, Mission_Attack's
    /// computer arm puts it on Capture, and it walks in and blacks the local
    /// house out.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_computer_spy_team_infiltrates_a_tesla_reactor() {
        use crate::sim::mission::MissionType;
        use crate::sim::spy_infiltration::InfiltrationEffect;
        use crate::sim::world::SimSoundEvent;
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) = retail_battle(
            |session| {
                session.local_slot_mut().country = LaunchCountry::Russia;
                session.options.unit_count = 0;
            },
            &[(LaunchCountry::America, AiDifficulty::Normal)],
        );
        let runtime = &mut scenario.runtime;
        let (sim, rules) = (&mut runtime.simulation, &runtime.resources.rules);
        let soviet = house_of(sim, "Russians");
        let america = house_of(sim, "Americans");
        let at = |sim: &Simulation, id| {
            let entity = sim.entities().get(id).expect("spawned");
            (
                i32::from(entity.position.rx()),
                i32::from(entity.position.ry()),
            )
        };
        let mcv = sim
            .substrate
            .entities
            .iter_sorted()
            .find(|(_, entity)| entity.owner() == soviet)
            .map(|(id, _)| id)
            .expect("the local MCV");
        let (mx, my) = at(sim, mcv);
        let reactor = spawn_on_free_cells(
            sim,
            rules,
            soviet,
            cells_nearest((mx + 5, my + 5)),
            &["NAPOWR"],
        )[0];
        let (rx, ry) = at(sim, reactor);
        spawn_on_free_cells(
            sim,
            rules,
            soviet,
            cells_nearest((rx, ry)),
            &["NALASR", "E2"],
        );
        // Twelve cells out towards the American MCV. The nearest cells the
        // other way lie in no zone (0xFFFF), and Evaluate_Candidate's zone
        // test, which only DWORD -1 skips (`0x006F7E36`), then rejects every
        // quarry of a spy standing there.
        let american_mcv = sim
            .substrate
            .entities
            .iter_sorted()
            .find(|(_, entity)| entity.owner() == america)
            .map(|(id, _)| id)
            .expect("the American MCV");
        let (ax, ay) = at(sim, american_mcv);
        let reach = (ax - rx).abs().max((ay - ry).abs()).max(1);
        let toward = (rx + (ax - rx) * 12 / reach, ry + (ay - ry) * 12 / reach);
        let cells = cells_nearest(toward)
            .into_iter()
            .filter(|&(x, y)| (x - rx).abs().max((y - ry).abs()) >= 10);
        let spy = spawn_on_free_cells(sim, rules, america, cells, &["SPY"])[0];
        let id = sim
            .interner
            .get("09B5091C-G")
            .expect("retail TeamType interned");
        // The computer house may already have made the team; then it
        // recruits the new spy.
        sim.create_team(id, Some(america), rules)
            .or_else(|| {
                sim.team_script_vm
                    .teams_in_order()
                    .find(|team| team.team_type_id() == Some(id) && team.owner() == america)
                    .map(|team| team.id())
            })
            .expect("a team of the TeamType");
        let (mut capturing, mut announced, mut hurt) = (None, None, false);
        for frame in 1..=2_000u32 {
            let output = tick_with_due_commands(&mut scenario);
            announced = announced.or(output.sound_events.iter().find_map(|event| match event {
                SimSoundEvent::BuildingInfiltrated {
                    owner,
                    infiltrator,
                    effect,
                    ..
                } => Some((frame, *owner, *infiltrator, *effect)),
                _ => None,
            }));
            if let Some(entity) = scenario.sim().entities().get(spy) {
                hurt |= entity.health.current() < 100;
                if capturing.is_none()
                    && entity.mission.current().known() == Some(MissionType::Capture)
                {
                    let worn = entity.disguise.as_ref().filter(|d| d.is_disguised());
                    capturing = Some((frame, worn.map(|d| (d.type_id(), d.house()))));
                }
            }
            if announced.is_some() {
                break;
            }
        }
        eprintln!("09B5091C-G: took Capture at {capturing:?}, went in: {announced:?}");
        let conscript = scenario.sim().interner.get("E2");
        assert_eq!(
            capturing.map(|(_, worn)| worn),
            Some(Some((conscript, Some(soviet)))),
            "the spy took Capture dressed as a Soviet Conscript"
        );
        assert!(!hurt, "a guard shot the disguised spy");
        assert_eq!(
            announced.map(|(_, owner, infiltrator, effect)| (owner, infiltrator, effect)),
            Some((soviet, america, InfiltrationEffect::Power)),
            "the spy never went in"
        );
        let sim = scenario.sim();
        assert!(sim.power_states[&soviet].blackout_remaining(sim.session.binary_frame()) > 0);
    }

    /// A Normal American house's Tanya (retail `[TANY]`: `C4=yes`,
    /// `Secondary=Sapper`) handed a Soviet Tesla Reactor eight cells away
    /// with Attack, as a team's action 46 does. Mission_Attack's C4 arm heads
    /// her in on Sabotage, she plants on the first reactor cell she reaches,
    /// and retail `C4Delay=.03` sets the charge off 26 frames later. The
    /// reactor dies; she lives.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_computer_tanya_plants_c4_on_a_tesla_reactor() {
        use crate::sim::combat::TargetKind;
        use crate::sim::mission::{MissionId, MissionType};
        use crate::sim::world::FrameEffects;
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) = retail_battle(
            |session| {
                session.local_slot_mut().country = LaunchCountry::Russia;
                session.options.unit_count = 0;
            },
            &[(LaunchCountry::America, AiDifficulty::Normal)],
        );
        let runtime = &mut scenario.runtime;
        let (sim, rules) = (&mut runtime.simulation, &runtime.resources.rules);
        let soviet = house_of(sim, "Russians");
        let america = house_of(sim, "Americans");
        let at = |sim: &Simulation, id| {
            let entity = sim.entities().get(id).expect("spawned");
            (
                i32::from(entity.position.rx()),
                i32::from(entity.position.ry()),
            )
        };
        let mcv_of = |sim: &Simulation, house| {
            sim.substrate
                .entities
                .iter_sorted()
                .find(|(_, entity)| entity.owner() == house)
                .map(|(id, _)| id)
                .expect("the house's MCV")
        };
        let (mx, my) = at(sim, mcv_of(sim, soviet));
        let reactor = spawn_on_free_cells(
            sim,
            rules,
            soviet,
            cells_nearest((mx + 5, my + 5)),
            &["NAPOWR"],
        )[0];
        let (rx, ry) = at(sim, reactor);
        // Out towards the American MCV: some nearer cells the other way lie
        // in no zone (see the spy team's test).
        let (ax, ay) = at(sim, mcv_of(sim, america));
        let reach = (ax - rx).abs().max((ay - ry).abs()).max(1);
        let toward = (rx + (ax - rx) * 8 / reach, ry + (ay - ry) * 8 / reach);
        let cells = cells_nearest(toward)
            .into_iter()
            .filter(|&(x, y)| (x - rx).abs().max((y - ry).abs()) >= 6);
        let tanya = spawn_on_free_cells(sim, rules, america, cells, &["TANY"])[0];
        let now = sim.session.binary_frame();
        sim.substrate
            .mission_assign_exact(tanya, MissionId::from_known(MissionType::Attack), now)
            .unwrap();
        sim.assign_target_represented(
            tanya,
            Some(TargetKind::Entity(reactor)),
            Some(rules),
            None,
            FrameEffects::default(),
        )
        .unwrap();
        let (mut sabotage, mut planted, mut died) = (None, None, None);
        for frame in 1..=1_500u32 {
            tick_with_due_commands(&mut scenario);
            let sim = scenario.sim();
            if sabotage.is_none()
                && sim.entities().get(tanya).is_some_and(|tanya| {
                    tanya.mission.current().known() == Some(MissionType::Sabotage)
                })
            {
                sabotage = Some(frame);
            }
            if planted.is_none()
                && let Some(pending) = sim
                    .entities()
                    .get(reactor)
                    .and_then(|reactor| reactor.pending_c4_detonation.pending())
            {
                planted = Some((frame, pending));
            }
            if sim
                .entities()
                .get(reactor)
                .is_none_or(|reactor| reactor.death.is_dying())
            {
                died = Some(frame);
                break;
            }
        }
        eprintln!("TANY: Sabotage at {sabotage:?}, planted {planted:?}, reactor died {died:?}");
        assert!(sabotage.is_some(), "Tanya never took Sabotage");
        let (plant_frame, pending) = planted.expect("Tanya planted no charge");
        assert_eq!(pending.source_entity_id(), Some(tanya));
        assert_eq!(pending.timer().duration(), 26);
        assert_eq!(died, Some(plant_frame + 26));
        assert!(
            scenario
                .sim()
                .entities()
                .get(tanya)
                .is_some_and(|tanya| !tanya.death.is_dying() && tanya.health.current() > 0),
            "Tanya died"
        );
    }

    /// A Normal Soviet house's War Miner out on the ore with a load, brought
    /// to half strength and shot by a Grizzly: its refinery answers its HELLO
    /// and it queues Enter (`combat::unit_damage_response`), then docks.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_damaged_computer_miner_heads_for_its_refinery() {
        use crate::sim::command::{Command, CommandEnvelope};
        use crate::sim::mission::{MissionId, MissionType};
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) =
            retail_battle(|_| {}, &[(LaunchCountry::Russia, AiDifficulty::Normal)]);
        let soviet = house_of(scenario.sim(), "Russians");
        let harvest = MissionId::from_known(MissionType::Harvest);
        let mut miner = None;
        for _ in 0..6000 {
            scenario.tick();
            let sim = scenario.sim();
            miner = sim
                .entities()
                .values()
                .find(|entity| {
                    entity.owner() == soviet
                        && !entity.death.is_dying()
                        && sim.interner.resolve(entity.type_ref()) == "HARV"
                        && entity.radio_contacts.is_empty()
                        && entity.mission.current() == harvest
                        && entity
                            .miner
                            .as_ref()
                            .is_some_and(|miner| miner.cargo().len() >= 5)
                })
                .map(|entity| entity.stable_id());
            if miner.is_some() {
                break;
            }
        }
        let miner = miner.expect("a loaded War Miner on the ore");
        let runtime = &mut scenario.runtime;
        let (sim, rules) = (&mut runtime.simulation, &runtime.resources.rules);
        let strength = rules.object("HARV").expect("retail HARV").strength;
        let at = {
            let entity = sim.substrate.entities.get_mut(miner).unwrap();
            entity.health.set_for_test(strength / 2);
            (
                i32::from(entity.position.rx()),
                i32::from(entity.position.ry()),
            )
        };
        let america = house_of(sim, "Americans");
        let cells = cells_nearest(at)
            .into_iter()
            .filter(|&(rx, ry)| (rx - at.0).abs().max((ry - at.1).abs()) >= 2);
        let tank = spawn_on_free_cells(sim, rules, america, cells, &["MTNK"])[0];
        let tick = sim.session.tick();
        sim.queue_command(CommandEnvelope::new(
            america,
            tick + 1,
            Command::Attack {
                attacker_id: tank,
                target_id: miner,
            },
        ));
        let enter = MissionId::from_known(MissionType::Enter);
        let (mut entered, mut docked) = (None, None);
        for frame in 1..=900u32 {
            tick_with_due_commands(&mut scenario);
            let sim = scenario.sim();
            let entity = sim.entities().get(miner).expect("the miner lives");
            let refinery = entity.radio_contacts.slot(0).filter(|&dock| {
                sim.entities().get(dock).is_some_and(|dock| {
                    dock.owner() == soviet && sim.interner.resolve(dock.type_ref()) == "NAREFN"
                })
            });
            if entered.is_none()
                && refinery.is_some()
                && (entity.mission.queued() == enter || entity.mission.current() == enter)
            {
                entered = Some(frame);
            }
            if entered.is_some() && entity.tether.partner().is_some() {
                docked = Some(frame);
                break;
            }
        }
        eprintln!("entered {entered:?}, docked {docked:?}");
        assert!(entered.is_some(), "the miner never headed for its refinery");
        assert!(docked.is_some(), "the miner never docked");
    }

    /// A Normal Soviet house on XMP03T4 whose refineries and War Miners are
    /// gone needs no rebuild a credit above a refinery's price, and does a
    /// credit below it (`sim::ai_build_need`). There, the strategy tick's
    /// Manage_Build_Queue sells the last sellable building of its base plan,
    /// which pays, and chooses a refinery. The yard stands, nothing is sent
    /// to hunt, and the yard builds the refinery, which brings a War Miner
    /// back.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn retail_broke_computer_house_sells_to_rebuild_its_refinery() {
        use crate::map::entities::EntityCategory;
        use crate::sim::ai_build_need::MANAGE_CALLS;
        use crate::sim::mission::{MissionId, MissionType};
        use crate::skirmish_launch::{AiDifficulty, LaunchCountry};

        let (mut scenario, _) =
            retail_battle(|_| {}, &[(LaunchCountry::Russia, AiDifficulty::Normal)]);
        let soviet = house_of(scenario.sim(), "Russians");
        // The house's live objects of type `name`, on the map or, with
        // `limbo`, in limbo too (a factory's product).
        let of_type = |sim: &Simulation, name: &str, limbo: bool| -> Vec<u64> {
            sim.entities()
                .values()
                .filter(|entity| {
                    !entity.death.is_dying()
                        && (limbo || !entity.lifecycle.in_limbo())
                        && entity.owner() == soviet
                        && sim.interner.resolve(entity.type_ref()) == name
                })
                .map(crate::sim::game_entity::GameEntity::stable_id)
                .collect()
        };
        let mut standing = None;
        for frame in 1..=9000u32 {
            scenario.tick();
            let sim = scenario.sim();
            let economy = ["NAREFN", "HARV"]
                .iter()
                .all(|name| of_type(sim, name, true).len() == of_type(sim, name, false).len());
            if economy
                && ["NAREFN", "NAWEAP", "HARV"]
                    .iter()
                    .all(|name| !of_type(sim, name, false).is_empty())
            {
                standing = Some(frame);
                break;
            }
        }
        assert!(
            standing.is_some(),
            "the base never stood with a refinery, a war factory and a miner"
        );
        let runtime = &mut scenario.runtime;
        let (sim, rules) = (&mut runtime.simulation, &runtime.resources.rules);
        let lost: Vec<u64> = ["NAREFN", "HARV"]
            .iter()
            .flat_map(|name| of_type(&*sim, name, false))
            .collect();
        for id in lost {
            sim.uninit_with_rules(id, rules);
        }
        let price = sim.cost_of(
            soviet,
            rules.object("NAREFN").expect("retail NAREFN"),
            rules,
        );
        // A yard's house can make money above the refinery's price, and must
        // rebuild below it.
        for (credits, need) in [(price + 1, false), (price - 1, true)] {
            sim.houses
                .get_mut(&soviet)
                .unwrap()
                .economy_mut()
                .set_credits_for_test(credits);
            assert_eq!(
                crate::sim::ai_build_need::check_build_need(sim, rules, soviet),
                need,
                "{credits} credits"
            );
        }
        let refinery = rules.building_type_index("NAREFN").expect("retail NAREFN");
        let (selling, construction) = (
            MissionId::from_known(MissionType::Selling),
            MissionId::from_known(MissionType::Construction),
        );
        MANAGE_CALLS.set(Some(0));
        let (mut managed, mut chosen, mut rebuilt, mut mined) = (None, None, None, None);
        let mut sold = std::collections::BTreeMap::new();
        for frame in 1..=9000u32 {
            scenario.tick();
            let sim = scenario.sim();
            if managed.is_none() && MANAGE_CALLS.get().is_some_and(|calls| calls > 0) {
                managed = Some(frame);
            }
            for entity in sim.entities().values() {
                if entity.owner() == soviet
                    && entity.category() == EntityCategory::Structure
                    && entity.mission.current() == selling
                {
                    sold.entry(entity.stable_id()).or_insert_with(|| {
                        (frame, sim.interner.resolve(entity.type_ref()).to_string())
                    });
                }
            }
            if chosen.is_none()
                && managed.is_some()
                && sim.houses[&soviet].ai_production().building_choice() == refinery
            {
                chosen = Some(frame);
            }
            if rebuilt.is_none()
                && of_type(sim, "NAREFN", false).iter().any(|&id| {
                    sim.entities()
                        .get(id)
                        .is_some_and(|building| building.mission.current() != construction)
                })
            {
                rebuilt = Some(frame);
            }
            if mined.is_none() && !of_type(sim, "HARV", false).is_empty() {
                mined = Some(frame);
            }
            if rebuilt.is_some() && mined.is_some() {
                break;
            }
        }
        let calls = MANAGE_CALLS.take().unwrap_or(0);
        let sim = scenario.sim();
        let hunting = sim.houses[&soviet].strategy_emergency().all_to_hunt_bias();
        let yards = of_type(sim, "NACNST", false).len();
        eprintln!(
            "base standing at {standing:?}; Manage_Build_Queue first at {managed:?} ({calls} \
             calls), refinery chosen at {chosen:?}, standing at {rebuilt:?}, miner back at \
             {mined:?}; {yards} yards, hunting {hunting}; sold (frame, type) {:?}",
            sold.values().collect::<Vec<_>>()
        );
        assert!(managed.is_some(), "Manage_Build_Queue never ran");
        assert!(!sold.is_empty(), "the house sold nothing");
        assert!(
            sold.values().all(|(_, name)| name != "NACNST") && yards > 0,
            "the house sold its yard"
        );
        assert!(!hunting, "the house sent everything to hunt");
        assert!(chosen.is_some(), "the house never chose a refinery");
        assert!(rebuilt.is_some(), "no refinery was rebuilt");
        assert!(mined.is_some(), "no War Miner came back");
    }
}

#[cfg(test)]
mod launch_special_flags_tests {
    use super::*;

    /// The nonzero game mode's session-flags replacement (`0x00687C16..
    /// 0x00687C29`) for Scenario bit `0x800`: `PregameSetup` stores the
    /// session's Harvester Truce byte (`0x00A8B31D`) in the staging word
    /// (`0x005E7495..0x005E74D4`), and the launch copies it over the map's
    /// flags. A launched session with the truce on carries the bit; the
    /// default skirmish session — whose dialog clears the byte
    /// (`0x006AD89D`) — leaves it clear.
    #[test]
    #[ignore = "requires RA2_DIR with installed retail RA2/YR assets"]
    fn headless_launch_harvester_truce_sets_the_scenario_bit() {
        let root = std::path::PathBuf::from(
            std::env::var("RA2_DIR").expect("set RA2_DIR to the retail RA2/YR install directory"),
        );
        let mut truce = battle_session(
            "XMP03T4.MAP",
            &[(
                crate::skirmish_launch::LaunchCountry::America,
                crate::skirmish_launch::AiDifficulty::Easy,
            )],
        );
        truce.options.harvester_truce = true;
        let scene = load_session(&root, "XMP03T4.MAP", 0x1234_5678, truce)
            .expect("launch with Harvester Truce on");
        assert!(
            scene.sim().session.harvester_immune(),
            "the session's truce byte replaces the map's bit"
        );

        let plain = load_session(
            &root,
            "XMP03T4.MAP",
            0x1234_5678,
            battle_session(
                "XMP03T4.MAP",
                &[(
                    crate::skirmish_launch::LaunchCountry::America,
                    crate::skirmish_launch::AiDifficulty::Easy,
                )],
            ),
        )
        .expect("launch with the default options");
        assert!(
            !plain.sim().session.harvester_immune(),
            "the default skirmish session leaves the bit clear"
        );
    }
}
