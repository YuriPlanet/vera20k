//! Loading request, phase, lease, failure and progress regressions.

use super::*;
use crate::app::loading::composition::MmpbRegionRect;
use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishAiSlot,
    SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLocalSlot,
};

pub(super) fn test_launch_session(country: LaunchCountry) -> SkirmishLaunchSession {
    SkirmishLaunchSession {
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
        selected_map_file: Some("mp01t4.map".to_string()),
        player_name: "Player".to_string(),
        local: SkirmishLocalSlot {
            country,
            country_random: false,
            color_index: 0,
            color_random: false,
            start_position: LaunchStartPosition::Position(0),
            team: LaunchTeam::None,
        },
        opponents: vec![SkirmishAiSlot {
            country: LaunchCountry::Russia,
            country_random: false,
            color_index: 1,
            color_random: false,
            start_position: LaunchStartPosition::Position(1),
            team: LaunchTeam::None,
            difficulty: AiDifficulty::Easy,
        }],
        pre_fill_house_roster: crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(1),
        options: SkirmishLaunchOptions::default(),
    }
}

struct TestClock(u32);

impl crate::match_bootstrap::MatchSeedClock for TestClock {
    fn low_u32(&mut self) -> u32 {
        self.0
    }

    fn source(&self) -> crate::match_bootstrap::MatchSeedSource {
        crate::match_bootstrap::MatchSeedSource::Controlled
    }

    fn seed_authority_certifying(&self) -> bool {
        true
    }
}

fn prepared_startup(next: &mut u64, seed: u32) -> crate::match_bootstrap::PreparedMatchStartup {
    let launch = test_launch_session(LaunchCountry::America);
    let accepted = match crate::match_bootstrap::classify_startup_session(&launch) {
        crate::match_bootstrap::StartupSessionClassification::AcceptedExplicitFixedBattle(
            accepted,
        ) => accepted,
        other => panic!("fixture was not accepted: {other:?}"),
    };
    let correlation = crate::match_bootstrap::allocate_match_correlation(next).unwrap();
    crate::match_bootstrap::prepare_match_startup(correlation, accepted, &mut TestClock(seed))
}

pub(super) fn unverified_seed(value: u32) -> crate::match_bootstrap::MatchSeed {
    crate::match_bootstrap::MatchSeed {
        value,
        source: crate::match_bootstrap::MatchSeedSource::Controlled,
        seed_authority_certifying: false,
    }
}

fn prefix_test_map(starts: &[(u8, u16, u16)]) -> crate::map::map_file::MapFile {
    let mut map =
        crate::map::rmg::emit::empty_map_file(&crate::map::rmg::RmgOptions::default(), 32, 32);
    map.waypoints.extend(starts.iter().map(|&(slot, rx, ry)| {
        (
            u32::from(slot),
            crate::map::waypoints::Waypoint {
                index: u32::from(slot),
                rx,
                ry,
            },
        )
    }));
    map
}

fn generated_preview_with_starts(
    seed: u16,
    starts: &[(u8, u16, u16)],
) -> crate::map::rmg::GeneratedMap {
    crate::map::rmg::GeneratedMap {
        map_file: prefix_test_map(starts),
        mapgen_continuation: crate::map::rmg::RmgRng::new(seed).into_continuation(),
        construction_trace: crate::map::rmg::RmgConstructionTrace::default(),
        start_waypoints: starts.to_vec(),
        stages_run: Vec::new(),
        unfilled_start_slots: 0,
    }
}

fn accepted_random_map_with_starts(
    selected_map_file: &str,
    seed: u16,
    preview_waypoints: &[(u8, u16, u16)],
    staged_starts: &[(u8, u16, u16)],
) -> crate::app::shell_random_map::AcceptedRandomMapLaunch {
    let mut retention = crate::app::shell_random_map::RandomMapGenerationRetention::default();
    let mut generated = generated_preview_with_starts(seed, staged_starts);
    generated.map_file = prefix_test_map(preview_waypoints);
    retention.finish_generation(generated);
    retention.accept_setup(selected_map_file);
    retention
        .take_acceptance_for_loading(Some(selected_map_file))
        .expect("matching accepted random-map fixture")
}

fn receipt_for(
    startup: &crate::match_bootstrap::PreparedMatchStartup,
) -> crate::match_bootstrap::RustL0Receipt {
    let simulation = crate::sim::world::Simulation::with_seed(u64::from(startup.seed.value));
    crate::match_bootstrap::RustL0Observation {
        startup,
        simulation: &simulation,
        active_correlation: startup.correlation,
        prior_receipt: None,
        screen_is_loading: true,
        spawn_pick_active: false,
    }
    .acknowledge()
    .expect("valid test startup must acknowledge")
}

fn test_audio() -> crate::app::audio_runtime::AppAudioRuntime {
    crate::app::audio_runtime::AppAudioRuntime {
        theme: crate::audio::theme::ThemeRuntime::default(),
        last_theme_poll_ms: None,
        music_player: None,
        sfx_player: None,
        sound_registry: Default::default(),
        audio_indices: Vec::new(),
        audio_indices_enabled: false,
        launcher_audio_available: true,
        theme_startup_suppressed: false,
        eva_registry: Default::default(),
    }
}

/// `begin_loading`'s shell -> scenario boundary: the LOADING request
/// (`Start_Scenario @ 0x00683AB0`, `Play_Song(From_Name("LOADING"))` @
/// `0x00683D1A`) must resolve the process asset manager *through the
/// loading-job lease*, and the audio pump's Theme poll must keep reaching
/// AI while that lease is out. Exercises the production leaf that
/// `begin_loading` calls and the resolver `pump_audio_service` uses.
#[test]
fn begin_loading_plays_loading_theme_and_polls_theme_through_the_lease() {
    let dir = std::env::temp_dir().join(format!("vera20k-loading-theme-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("test asset dir");
    std::fs::write(
        dir.join("thememd.ini"),
        b"[Themes]\n1=INTRO\n2=LOADING\n\n[INTRO]\nName=Intro\nSound=intro\nRepeat=yes\n\n\
          [LOADING]\nName=Loading\nSound=loading\nRepeat=yes\n",
    )
    .expect("write thememd.ini");
    let mut process_assets = crate::app::process_assets::ProcessAssets::from_startup(
        Some(AssetManager::from_loose_root_for_test(&dir)),
        None,
        None,
    );
    let mut audio = test_audio();
    let session = LoadingSession::from_request(LoadingRequest::unverified_legacy_skirmish(
        test_launch_session(LaunchCountry::America),
        unverified_seed(1),
        SkirmishSettings::default(),
    ));

    let mut slot = None;
    let mut startup = crate::app::match_runtime::startup::MatchStartup::default();
    replace_loading_attempt(
        &mut slot,
        &mut startup,
        &mut process_assets,
        &mut audio,
        session,
        500,
    );
    let session = slot.as_ref().unwrap();

    assert!(
        process_assets.manager().is_none() && process_assets.is_leased(),
        "the manager is leased into the loading job"
    );
    let loading = audio.theme.from_name("LOADING");
    assert!(loading >= 0, "catalog resolves LOADING");
    let slots = audio.theme.slots();
    assert_eq!(
        slots.retained, loading,
        "Play_Song(LOADING) was issued while the resident slot was empty"
    );
    assert_eq!(
        slots.pending, loading,
        "LOADING repeats under the loading screen"
    );

    // The audio pump's poll resolves the leased manager and advances AI.
    assert!(
        audio_service_asset_manager(&process_assets, None).is_none(),
        "resident slot alone cannot serve the poll during a lease"
    );
    let assets = audio_service_asset_manager(&process_assets, Some(session))
        .expect("leased manager serves the Theme poll");
    audio.update_theme(assets, 600);
    assert_eq!(audio.last_theme_poll_ms, Some(600));
}

#[test]
fn loading_replacement_and_terminal_retirement_preserve_cache_and_admission_order() {
    let dir = std::env::temp_dir().join(format!("vera20k-loading-owner-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sentinel.bin"), b"first winner").unwrap();
    let mut assets = crate::app::process_assets::ProcessAssets::from_startup(
        Some(AssetManager::from_loose_root_for_test(&dir)),
        None,
        None,
    );
    let original = assets
        .manager()
        .unwrap()
        .load_file_from_mix("sentinel.bin")
        .unwrap();
    let mut audio = test_audio();
    let mut startup = crate::app::match_runtime::startup::MatchStartup::default();
    let mut slot = None;
    let mut next = 1;
    let first = prepared_startup(&mut next, 7);
    let replacement = prepared_startup(&mut next, 8);
    for prepared in [&first, &replacement] {
        replace_loading_attempt(
            &mut slot,
            &mut startup,
            &mut assets,
            &mut audio,
            LoadingSession::from_request(LoadingRequest::accepted_skirmish(
                prepared.clone(),
                SkirmishSettings::default(),
            )),
            500,
        );
        let cached = loading_asset_manager(slot.as_ref().unwrap())
            .unwrap()
            .load_file_from_mix("SENTINEL.BIN")
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(&original.bytes, &cached.bytes));
    }
    assert!(
        startup
            .acknowledge(
                first.clone(),
                Some(&crate::sim::world::Simulation::with_seed(7)),
                true,
                false
            )
            .is_err()
    );
    // The installer's resource-only cleanup must preserve the replacement
    // admission until its actual L0 observation commits.
    retire_loading_attempt(&mut slot, &mut assets);
    assert!(slot.is_none() && assets.is_available() && !assets.is_leased());
    startup
        .acknowledge(
            replacement.clone(),
            Some(&crate::sim::world::Simulation::with_seed(8)),
            true,
            false,
        )
        .unwrap();
    assert_eq!(startup.startup(), Some(&replacement));

    for consumed_during_preparation in [false, true] {
        let attempt = prepared_startup(&mut next, 9);
        replace_loading_attempt(
            &mut slot,
            &mut startup,
            &mut assets,
            &mut audio,
            LoadingSession::from_request(LoadingRequest::accepted_skirmish(
                attempt.clone(),
                SkirmishSettings::default(),
            )),
            600,
        );
        if consumed_during_preparation {
            // Exact production preparation failure before initial selection;
            // no config exists, while the prior lease is already owned.
            let err = prepare_loading_session(&mut assets, slot.take().unwrap(), true, None)
                .err()
                .unwrap();
            assert!(err.to_string().contains("missing game config"));
            assert!(assets.is_available());
        }
        assert_eq!(
            retire_failed_loading_attempt(
                &mut slot,
                &mut startup,
                &mut assets,
                LoadingFailurePolicy::ReportNativeFailure
            ),
            LoadingFailurePolicy::ReportNativeFailure
        );
        assert!(slot.is_none() && assets.is_available() && !assets.is_leased());
        assert!(
            startup
                .acknowledge(
                    attempt,
                    Some(&crate::sim::world::Simulation::with_seed(9)),
                    true,
                    false
                )
                .is_err()
        );
        let cached = assets
            .manager()
            .unwrap()
            .load_file_from_mix("sentinel.bin")
            .unwrap();
        assert!(std::sync::Arc::ptr_eq(&original.bytes, &cached.bytes));
    }

    replace_loading_attempt(
        &mut slot,
        &mut startup,
        &mut assets,
        &mut audio,
        LoadingSession::from_request(LoadingRequest::generic_map_load(
            "auto",
            SkirmishSettings::default(),
        )),
        700,
    );
    let before_fallback_install = startup.clone();
    assert_eq!(
        retire_failed_loading_attempt(
            &mut slot,
            &mut startup,
            &mut assets,
            LoadingFailurePolicy::InstallGenericFallback
        ),
        LoadingFailurePolicy::InstallGenericFallback
    );
    assert_eq!(startup, before_fallback_install);
    assert!(assets.is_available() && slot.is_none());
    assert!(std::sync::Arc::ptr_eq(
        &original.bytes,
        &assets
            .manager()
            .unwrap()
            .load_file_from_mix("sentinel.bin")
            .unwrap()
            .bytes
    ));
}

/// Uses the production initial-map reader and real retail trig tables;
/// assertions distinguish initial read failure from context admission.
#[test]
#[ignore = "requires RA2_DIR with verified retail gamemd.exe"]
fn loading_preparation_consumes_real_source_and_returns_lease_on_initial_and_admission_failures() {
    let ra2_dir = PathBuf::from(std::env::var_os("RA2_DIR").expect("RA2_DIR"));
    crate::map::retail_trig::install_from_dir(&ra2_dir);
    assert!(crate::map::retail_trig::wave_tables_available());
    let dir = std::env::temp_dir().join(format!(
        "vera20k-loading-preparation-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let map_path = dir.join("mp01t4.map");
    let map_bytes = AssetManager::new(&ra2_dir)
        .unwrap()
        .get("Fight.MAP")
        .expect("retail Fight.MAP fixture");
    std::fs::write(&map_path, map_bytes).unwrap();
    std::fs::write(dir.join("sentinel.bin"), b"keep this cache").unwrap();
    let mut assets = crate::app::process_assets::ProcessAssets::from_startup(
        Some(AssetManager::from_loose_root_for_test(&dir)),
        None,
        None,
    );
    let original = assets
        .manager()
        .unwrap()
        .load_file_from_mix("sentinel.bin")
        .unwrap();
    let mut launch = test_launch_session(LaunchCountry::America);
    launch.selected_map_file = Some(map_path.to_string_lossy().into_owned());
    let request = LoadingRequest::unverified_legacy_skirmish(
        launch,
        unverified_seed(7),
        SkirmishSettings::default(),
    );
    let mut session = LoadingSession::from_request(request);
    session.job.asset_manager = assets.lease_for_loading();
    let session =
        prepare_loading_session(&mut assets, session, true, Some(ra2_dir.clone())).unwrap();
    assert!(!session.first_frame_presented);
    assert_eq!(
        session.native.as_ref().unwrap().progress.current_value(),
        0.0
    );
    let LoadingStage::Prepared(prepared) = &session.stage else {
        panic!("prepared payload");
    };
    assert_eq!(
        prepared.context.physical_source(),
        prepared.initial.map_source()
    );
    assert_eq!(prepared.context.signed_new_ini_format(), 4);
    assert!(!prepared.initial.map_data().waypoints.is_empty());
    session.job.retire(&mut assets);

    for (selected, expected_error) in [
        (map_path.to_string_lossy().into_owned(), "Generic startup"),
        (
            dir.join("absent.map").to_string_lossy().into_owned(),
            "absent.map",
        ),
    ] {
        let mut session = LoadingSession::from_request(LoadingRequest::generic_map_load(
            selected,
            SkirmishSettings::default(),
        ));
        session.job.asset_manager = assets.lease_for_loading();
        let err = prepare_loading_session(&mut assets, session, false, Some(ra2_dir.clone()))
            .err()
            .unwrap();
        assert!(format!("{err:#}").contains(expected_error), "{err:#}");
        assert!(assets.is_available() && !assets.is_leased());
        assert!(std::sync::Arc::ptr_eq(
            &original.bytes,
            &assets
                .manager()
                .unwrap()
                .load_file_from_mix("sentinel.bin")
                .unwrap()
                .bytes
        ));
    }
}

#[test]
fn loading_side_comes_from_first_launch_node_country() {
    let session = LoadingSession::from_request(LoadingRequest::unverified_legacy_skirmish(
        test_launch_session(LaunchCountry::Korea),
        unverified_seed(1),
        SkirmishSettings::default(),
    ));

    assert_eq!(
        session.native.as_ref().map(|native| native.variant),
        Some(LoadingArtVariant::Alliance)
    );
}

#[test]
fn loading_progress_row_snapshots_the_launch_player_name() {
    let mut launch = test_launch_session(LaunchCountry::America);
    launch.player_name = "Commander".to_owned();
    let session = LoadingSession::from_request(LoadingRequest::unverified_legacy_skirmish(
        launch,
        unverified_seed(22),
        SkirmishSettings::default(),
    ));

    assert_eq!(
        session
            .native
            .as_ref()
            .map(|native| native.progress_row.label.as_str()),
        Some("Commander"),
    );
}

#[test]
fn loading_session_preserves_selected_map_filename() {
    let session = LoadingSession::from_request(LoadingRequest::unverified_legacy_skirmish(
        test_launch_session(LaunchCountry::Yuri),
        unverified_seed(2),
        SkirmishSettings::default(),
    ));

    assert_eq!(session.stage.request().selected_map_file(), "mp01t4.map");
    assert_eq!(
        session
            .stage
            .request()
            .skirmish_launch_session()
            .and_then(|launch| launch.selected_map_file.as_deref()),
        Some("mp01t4.map")
    );
}

#[test]
fn loading_session_selects_native_progress_cadence_from_map_kind() {
    let selected = LoadingSession::from_request(LoadingRequest::unverified_legacy_skirmish(
        test_launch_session(LaunchCountry::America),
        unverified_seed(20),
        SkirmishSettings::default(),
    ));
    assert_eq!(
        selected
            .native
            .as_ref()
            .map(|native| native.progress_cadence),
        Some(NativeLoadingProgressCadence::SelectedMap)
    );

    let mut random_map = test_launch_session(LaunchCountry::America);
    random_map.selected_map_file = Some("RandMap.Sed".to_string());
    let random = LoadingSession::from_request(LoadingRequest::unverified_legacy_skirmish(
        random_map,
        unverified_seed(21),
        SkirmishSettings::default(),
    ));
    assert_eq!(
        random.native.as_ref().map(|native| native.progress_cadence),
        Some(NativeLoadingProgressCadence::RandomMapHalved)
    );
}

#[test]
fn gsi_04_12_generated_prefix_uses_accepted_staging_once() {
    use crate::map::map_file::MapCell;

    let selected = "RandMap.Sed";
    let staged = [(0, 70, 70)];
    let preview_waypoints = [(0, 110, 110), (1, 130, 110)];
    let regenerated = [(0, 70, 90), (1, 90, 90)];
    let mut launch = test_launch_session(LaunchCountry::America);
    launch.selected_map_file = Some(selected.to_string());
    let accepted = accepted_random_map_with_starts(selected, 0x1212, &preview_waypoints, &staged);
    let mut regenerated_map =
        crate::map::rmg::emit::empty_map_file(&crate::map::rmg::RmgOptions::default(), 100, 100);
    regenerated_map
        .waypoints
        .extend(regenerated.iter().map(|&(slot, rx, ry)| {
            (
                u32::from(slot),
                crate::map::waypoints::Waypoint {
                    index: u32::from(slot),
                    rx,
                    ry,
                },
            )
        }));
    regenerated_map.cells = [(60, 60), (60, 140), (140, 60), (140, 140)]
        .into_iter()
        .map(|(rx, ry)| MapCell {
            rx,
            ry,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        })
        .collect();
    let initial = crate::app::loading::init::MapLoadInitial::from_test_map_source(
        regenerated_map,
        crate::app::frontend::list_maps::LoadedMapSource::Generated {
            seed_name: selected.to_ascii_lowercase(),
        },
    );
    let request = LoadingRequest::unverified_legacy_skirmish(
        launch.clone(),
        unverified_seed(0x1212),
        SkirmishSettings::default(),
    )
    .with_accepted_random_map(Some(accepted));

    let prepared = request.prepare_initial(initial).unwrap();
    let context = &prepared.context;
    let initial = &prepared.initial;
    let request = &prepared.request;
    let projection = context.stock_offline_projection();
    let final_starts = projection
        .final_gathered_starts()
        .iter()
        .map(|waypoint| (waypoint.index as u8, waypoint.rx, waypoint.ry))
        .collect::<Vec<_>>();
    assert_eq!(final_starts.len(), 2);
    assert_eq!(final_starts[0], staged[0]);
    let gathered_fallback = final_starts[1];
    assert!(
        !staged.contains(&gathered_fallback)
            && !preview_waypoints.contains(&gathered_fallback)
            && !regenerated.contains(&gathered_fallback),
        "deficient Gather must add a distinct temporary start"
    );
    assert_ne!(final_starts, preview_waypoints);
    assert_ne!(final_starts, regenerated);
    let active_starts =
        crate::map::waypoints::multiplayer_start_waypoints(projection.active_scenario_waypoints())
            .into_iter()
            .map(|waypoint| (waypoint.index as u8, waypoint.rx, waypoint.ry))
            .collect::<Vec<_>>();
    assert_eq!(active_starts, staged);

    let assignments = selected_map_start_assignments(&launch, Some(projection));
    let composition = build_random_map_loading_composition(
        &launch,
        None,
        [800, 600],
        Some(DecodedPreview {
            width: 300,
            height: 100,
            rgba: vec![255; 300 * 100 * 4],
        }),
        initial.map_data(),
        projection.active_scenario_waypoints(),
        &assignments,
    );
    assert_eq!(
        composition
            .markers
            .iter()
            .map(|marker| {
                (
                    marker.waypoint.index as u8,
                    marker.waypoint.rx,
                    marker.waypoint.ry,
                )
            })
            .collect::<Vec<_>>(),
        staged,
        "random loading markers read the active Scenario staging copy"
    );

    let staged_session = crate::app::loading::init::scenario_start_waypoints_for_load(
        initial.map_data(),
        Some(projection),
    );
    let regenerated_session =
        crate::app::loading::init::scenario_start_waypoints_for_load(initial.map_data(), None);
    assert_eq!(
        staged_session
            .iter()
            .map(|(&index, &(rx, ry))| (index as u8, rx, ry))
            .collect::<Vec<_>>(),
        staged
    );
    assert_ne!(staged_session, regenerated_session);
    let gathered_session = final_starts
        .iter()
        .map(|&(index, rx, ry)| (u32::from(index), (rx, ry)))
        .collect();
    assert_ne!(
        staged_session, gathered_session,
        "the active Scenario table excludes Gather-only fallback starts"
    );
    let staged_sim = crate::sim::world::Simulation::from_descriptor(
        &crate::sim::scenario_session::ScenarioDescriptor {
            seed: 0x1212,
            map_width: 256,
            map_height: 256,
            mp_start_waypoints: staged_session,
            ..Default::default()
        },
    );
    let regenerated_sim = crate::sim::world::Simulation::from_descriptor(
        &crate::sim::scenario_session::ScenarioDescriptor {
            seed: 0x1212,
            map_width: 256,
            map_height: 256,
            mp_start_waypoints: regenerated_session,
            ..Default::default()
        },
    );
    let gathered_sim = crate::sim::world::Simulation::from_descriptor(
        &crate::sim::scenario_session::ScenarioDescriptor {
            seed: 0x1212,
            map_width: 256,
            map_height: 256,
            mp_start_waypoints: gathered_session,
            ..Default::default()
        },
    );
    assert_eq!(
        staged_sim
            .session
            .mp_start_waypoints
            .iter()
            .map(|(&index, &(rx, ry))| (index as u8, rx, ry))
            .collect::<Vec<_>>(),
        staged
    );
    assert_ne!(staged_sim.state_hash(), gathered_sim.state_hash());
    assert_ne!(staged_sim.state_hash(), regenerated_sim.state_hash());
    assert!(
        request.accepted_rmg_start_staging.is_none(),
        "accepted setup staging transfers exactly once"
    );
    assert!(
        request.random_map_preview().is_some(),
        "presentation preview remains available to loading composition"
    );
}

#[test]
fn load_descriptor_source_family_format_matrix() {
    use crate::app::loading::fresh_scenario::{
        FreshMapMaterialization, FreshScenarioFamily, FreshStartupProvenance,
    };

    let starts = [(0, 20, 24), (1, 42, 46)];
    let signed_formats = [
        (None, 0, false),
        (Some(1), 1, false),
        (Some(2), 2, true),
        (Some(4), 4, true),
        (Some(-7), -7, false),
    ];
    for source in [
        crate::app::frontend::list_maps::LoadedMapSource::Loose {
            path: std::path::PathBuf::from("mp01t4.map"),
            payload_len: 17,
        },
        crate::app::frontend::list_maps::LoadedMapSource::Mix {
            logical_name: "mp01t4.map".to_string(),
            source_archive: "mapsmd03.mix".to_string(),
            entry_id: 0x1234,
            payload_len: 19,
        },
    ] {
        for (format, expected_signed, expected_pack_gate) in signed_formats {
            let mut map = prefix_test_map(&starts);
            map.basic.new_ini_format = format;
            let initial = crate::app::loading::init::MapLoadInitial::from_test_map_source(
                map,
                source.clone(),
            );
            let request = LoadingRequest::unverified_legacy_skirmish(
                test_launch_session(LaunchCountry::America),
                unverified_seed(0x1A2B_3C4D),
                SkirmishSettings::default(),
            );
            let prepared = request
                .prepare_initial(initial)
                .expect("authored Loose/MIX stock context");
            let context = &prepared.context;
            let request = &prepared.request;
            assert_eq!(context.physical_source(), &source);
            assert_eq!(context.materialization(), FreshMapMaterialization::Authored);
            assert_eq!(context.family(), FreshScenarioFamily::StockOffline);
            assert_eq!(
                context.startup_provenance(),
                FreshStartupProvenance::ResolvedLegacy
            );
            assert_eq!(context.match_seed(), 0x1A2B_3C4D);
            assert_eq!(context.signed_new_ini_format(), expected_signed);
            context
                .validate_terminal_transfer(request.startup(), &source, expected_signed)
                .expect("the prepared bundle retains matching terminal inputs");
            assert_eq!(
                context.authored_pack_bodies_enabled(),
                expected_pack_gate,
                "only signed NewINIFormat > 1 gates authored pack bodies"
            );
        }
    }

    for mode_id in [1, 2] {
        let selected = format!("Accepted{mode_id}.SED");
        let mut launch = test_launch_session(LaunchCountry::America);
        launch.selected_map_file = Some(selected.clone());
        if mode_id == 2 {
            launch.mode = SkirmishLaunchMode {
                id: 2,
                ui_name_key: "GUI:FreeForAll".to_string(),
                tooltip_key: "STT:ModeFreeForAll".to_string(),
                override_file: "MPFreeForAllMD.ini".to_string(),
                map_filter: "standard".to_string(),
                random_maps_allowed: true,
                allies_allowed: false,
                must_ally: false,
            };
        }
        let accepted = accepted_random_map_with_starts(&selected, 0x2345, &starts, &starts);
        let mut map = prefix_test_map(&starts);
        map.basic.new_ini_format = Some(4);
        let source = crate::app::frontend::list_maps::LoadedMapSource::Generated {
            seed_name: selected.to_ascii_lowercase(),
        };
        let initial =
            crate::app::loading::init::MapLoadInitial::from_test_map_source(map, source.clone());
        let request = LoadingRequest::unverified_legacy_skirmish(
            launch,
            unverified_seed(0x2345),
            SkirmishSettings::default(),
        )
        .with_accepted_random_map(Some(accepted));
        let prepared = request
            .prepare_initial(initial)
            .expect("accepted Battle/FFA generated context");
        let context = &prepared.context;
        let request = &prepared.request;
        assert_eq!(context.physical_source(), &source);
        assert_eq!(
            context.materialization(),
            FreshMapMaterialization::AcceptedGenerated
        );
        assert_eq!(context.signed_new_ini_format(), 4);
        context
            .validate_terminal_transfer(request.startup(), &source, 4)
            .expect("generated terminal owners still agree");
        assert!(
            !context.authored_pack_bodies_enabled(),
            "serialized format cannot turn generated materialization into authored Mark"
        );
    }
}

#[test]
fn accepted_and_resolved_legacy_share_one_stock_cursor_shape() {
    use crate::app::loading::fresh_scenario::{FreshScenarioFamily, FreshStartupProvenance};

    let starts = [(0, 20, 24), (1, 42, 46)];
    let source = crate::app::frontend::list_maps::LoadedMapSource::Loose {
        path: std::path::PathBuf::from("mp01t4.map"),
        payload_len: 23,
    };
    let mut map = prefix_test_map(&starts);
    map.basic.new_ini_format = Some(4);
    let initial =
        crate::app::loading::init::MapLoadInitial::from_test_map_source(map, source.clone());
    let mut legacy_map = prefix_test_map(&starts);
    legacy_map.basic.new_ini_format = Some(4);
    let legacy_initial =
        crate::app::loading::init::MapLoadInitial::from_test_map_source(legacy_map, source);
    let seed = 0x3456_789A;
    let mut next = 1;
    let prepared = prepared_startup(&mut next, seed);
    let legacy_session = prepared.session.launch_session().clone();
    let accepted = LoadingRequest::accepted_skirmish(prepared, SkirmishSettings::default());
    let resolved_legacy = LoadingRequest::unverified_legacy_skirmish(
        legacy_session,
        unverified_seed(seed),
        SkirmishSettings::default(),
    );
    let accepted = accepted.prepare_initial(initial).unwrap();
    let resolved_legacy = resolved_legacy.prepare_initial(legacy_initial).unwrap();
    let accepted_context = &accepted.context;
    let legacy_context = &resolved_legacy.context;
    assert_eq!(accepted_context.family(), FreshScenarioFamily::StockOffline);
    assert_eq!(accepted_context.family(), legacy_context.family());
    assert_eq!(accepted_context.match_seed(), legacy_context.match_seed());
    assert_eq!(
        accepted_context
            .stock_offline_projection()
            .final_gathered_starts(),
        legacy_context
            .stock_offline_projection()
            .final_gathered_starts()
    );
    assert_eq!(
        accepted_context.stock_offline_projection().start_table(),
        legacy_context.stock_offline_projection().start_table()
    );
    assert_eq!(
        accepted_context.startup_provenance(),
        FreshStartupProvenance::Accepted
    );
    assert_eq!(
        legacy_context.startup_provenance(),
        FreshStartupProvenance::ResolvedLegacy
    );

    let accepted_parts = accepted.context.into_stock_offline_parts();
    let legacy_parts = resolved_legacy.context.into_stock_offline_parts();
    let mut accepted_owner = crate::sim::scenario_bootstrap::ScenarioBootstrapRng::new(seed);
    let mut legacy_owner = crate::sim::scenario_bootstrap::ScenarioBootstrapRng::new(seed);
    let _ = accepted_owner
        .install_pre_fill_scenario_prefix_plan(accepted_parts.scenario_prefix)
        .unwrap();
    let _ = legacy_owner
        .install_pre_fill_scenario_prefix_plan(legacy_parts.scenario_prefix)
        .unwrap();
    assert_eq!(
        accepted_owner.logical_states_for_test(),
        legacy_owner.logical_states_for_test(),
        "startup provenance cannot change the verified stock prefix cursor"
    );
}

#[test]
fn generic_manual_and_unresolved_legacy_reject_before_receipt_or_staging() {
    let selected = "Rejected.SED";
    let starts = [(0, 20, 24), (1, 42, 46)];
    let source = crate::app::frontend::list_maps::LoadedMapSource::Generated {
        seed_name: selected.to_string(),
    };
    let initial = crate::app::loading::init::MapLoadInitial::from_test_map_source(
        prefix_test_map(&starts),
        source,
    );
    let accepted = accepted_random_map_with_starts(selected, 0x4567, &starts, &starts);
    let mut generic = LoadingRequest::generic_map_load(selected, SkirmishSettings::default())
        .with_accepted_random_map(Some(accepted));
    let generic_err = generic.admit_context(&initial).unwrap_err();
    assert!(format!("{generic_err:#}").contains("Generic startup"));
    assert!(generic.accepted_rmg_start_staging.is_some());

    let accepted = accepted_random_map_with_starts(selected, 0x4567, &starts, &starts);
    let mut unresolved_session = test_launch_session(LaunchCountry::America);
    unresolved_session.selected_map_file = Some(selected.to_string());
    unresolved_session.local.country_random = true;
    let mut unresolved = LoadingRequest::unverified_legacy_skirmish(
        unresolved_session,
        unverified_seed(0x4567),
        SkirmishSettings::default(),
    )
    .with_accepted_random_map(Some(accepted));
    let unresolved_err = unresolved.admit_context(&initial).unwrap_err();
    assert!(format!("{unresolved_err:#}").contains("local slot still has a random country"));
    assert!(unresolved.accepted_rmg_start_staging.is_some());

    let authored = crate::app::loading::init::MapLoadInitial::from_test_map_source(
        prefix_test_map(&starts),
        crate::app::frontend::list_maps::LoadedMapSource::Loose {
            path: std::path::PathBuf::from("manual.map"),
            payload_len: 1,
        },
    );
    let mut manual_session = test_launch_session(LaunchCountry::America);
    manual_session.selected_map_file = Some(" auto ".to_string());
    let mut manual = LoadingRequest::unverified_legacy_skirmish(
        manual_session,
        unverified_seed(0x4567),
        SkirmishSettings::default(),
    );
    let manual_err = manual.admit_context(&authored).unwrap_err();
    assert!(format!("{manual_err:#}").contains("no exact selected map record"));
}

#[test]
fn loading_assignments_keep_gather_table_slots_separate_from_sparse_waypoint_indices() {
    use crate::app::loading::composition::{
        PreviewAspectFit, ProjectedPlayfieldBounds, build_mmpb_marker_records,
        native_loading_waypoint_prefix,
    };

    let mut launch = test_launch_session(LaunchCountry::America);
    let mut second_opponent = launch.opponents[0].clone();
    second_opponent.color_index = 2;
    second_opponent.start_position = LaunchStartPosition::Position(2);
    launch.opponents.push(second_opponent);
    launch.pre_fill_house_roster =
        crate::skirmish_launch::PreFillHouseRoster::from_compact_skirmish(2);

    // Gather target 3 retains raw slots 0 and 2, then appends a fallback
    // at vector position 2. The retained slot and fallback consequently
    // carry the same Waypoint::index even though the assignment table has
    // three distinct positions.
    let map = prefix_test_map(&[(0, 20, 24), (2, 42, 46), (3, 54, 58)]);
    let descriptor =
        crate::sim::scenario_bootstrap::MatchLaunchDescriptor::from_resolved(launch.clone())
            .unwrap();
    let plan = crate::sim::scenario_bootstrap::prepare_stock_offline_scenario_prefix_plan(
        &descriptor,
        &map,
        &map.waypoints,
        0x1223,
    )
    .unwrap();
    assert_eq!(
        plan.final_gathered_starts()
            .iter()
            .map(|waypoint| waypoint.index)
            .collect::<Vec<_>>(),
        vec![0, 2, 2]
    );

    let assignments = selected_map_start_assignments(&launch, Some(plan.projection()));
    assert_eq!(
        assignments
            .iter()
            .map(|assignment| (assignment.start_index, assignment.participant))
            .collect::<Vec<_>>(),
        vec![
            (0, LoadingParticipantId::Local),
            (1, LoadingParticipantId::Opponent(0)),
            (2, LoadingParticipantId::Opponent(1)),
        ],
        "loading assignments are keyed by Scenario start-table position"
    );

    // The compositor still reads original Scenario geometry. Native's odd
    // sparse-prefix rule visits raw waypoint 2 and colors it from table[2],
    // not from the retained Gather vector entry at position 1.
    let raw_prefix = native_loading_waypoint_prefix(&map.waypoints);
    let markers = build_mmpb_marker_records(
        &raw_prefix,
        &assignments,
        ProjectedPlayfieldBounds {
            min_x: 0,
            min_y: 0,
            extent_x: 1_000,
            extent_y: 1_000,
        },
        MmpbRegionRect {
            x: 0,
            y: 0,
            width: 200,
            height: 200,
        },
        PreviewAspectFit {
            scale_1000: 1_000,
            width: 200,
            height: 200,
            pad_x: 0,
            pad_y: 0,
        },
    );
    assert_eq!(markers.len(), 2);
    assert_eq!(markers[1].waypoint.index, 2);
    assert_eq!(markers[1].participant, LoadingParticipantId::Opponent(1));
}

#[test]
fn gsi_04_12_generated_prefix_rejects_presentation_only_preview() {
    let selected = "RandMap.Sed";
    let starts = [(0, 20, 24), (1, 42, 46)];
    let mut launch = test_launch_session(LaunchCountry::America);
    launch.selected_map_file = Some(selected.to_string());
    let initial = crate::app::loading::init::MapLoadInitial::from_test_map_source(
        prefix_test_map(&starts),
        crate::app::frontend::list_maps::LoadedMapSource::Generated {
            seed_name: selected.to_string(),
        },
    );
    let mut request = LoadingRequest::unverified_legacy_skirmish(
        launch,
        unverified_seed(0x1313),
        SkirmishSettings::default(),
    )
    .with_random_map_preview(Some(generated_preview_with_starts(0x1313, &starts)));

    let err = request.admit_context(&initial).unwrap_err();
    assert!(
        format!("{err:#}").contains("no accepted setup start staging"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn generated_prefix_rejects_mismatched_source_name() {
    let selected = "RandMap.Sed";
    let starts = [(0, 20, 24), (1, 42, 46)];
    let mut launch = test_launch_session(LaunchCountry::America);
    launch.selected_map_file = Some(selected.to_string());
    let accepted = accepted_random_map_with_starts(selected, 0x1414, &starts, &starts);
    let initial = crate::app::loading::init::MapLoadInitial::from_test_map_source(
        prefix_test_map(&starts),
        crate::app::frontend::list_maps::LoadedMapSource::Generated {
            seed_name: "Other.Sed".to_string(),
        },
    );
    let mut request = LoadingRequest::unverified_legacy_skirmish(
        launch,
        unverified_seed(0x1414),
        SkirmishSettings::default(),
    )
    .with_accepted_random_map(Some(accepted));

    let err = request.admit_context(&initial).unwrap_err();
    assert!(
        format!("{err:#}").contains("does not match selected record"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn generated_prefix_rejects_cooperative_mode() {
    let selected = "RandMap.Sed";
    let starts = [(0, 20, 24), (1, 42, 46)];
    let mut launch = test_launch_session(LaunchCountry::America);
    launch.selected_map_file = Some(selected.to_string());
    let cooperative = crate::skirmish_modes::stock_skirmish_modes()
        .into_iter()
        .find(|mode| mode.id == 3)
        .expect("retail Cooperative row");
    launch.mode = SkirmishLaunchMode::from_game_mode(&cooperative);
    let accepted = accepted_random_map_with_starts(selected, 0x1515, &starts, &starts);
    let initial = crate::app::loading::init::MapLoadInitial::from_test_map_source(
        prefix_test_map(&starts),
        crate::app::frontend::list_maps::LoadedMapSource::Generated {
            seed_name: selected.to_string(),
        },
    );
    let mut request = LoadingRequest::unverified_legacy_skirmish(
        launch,
        unverified_seed(0x1515),
        SkirmishSettings::default(),
    )
    .with_accepted_random_map(Some(accepted));

    let err = request.admit_context(&initial).unwrap_err();
    assert!(
        format!("{err:#}").contains("unsupported for stock mode id 3"),
        "unexpected error: {err:#}"
    );
    assert!(
        request.accepted_rmg_start_staging.is_some(),
        "unsupported family rejects before consuming accepted staging"
    );
}

#[test]
fn generated_prefix_rejects_a_spoofed_stock_row_before_consuming_staging() {
    let selected = "RandMap.SED";
    let starts = [(0, 20, 24), (1, 42, 46)];
    let mut launch = test_launch_session(LaunchCountry::America);
    launch.selected_map_file = Some(selected.to_string());
    launch.mode.tooltip_key = "STT:SpoofedBattle".to_string();
    let accepted = accepted_random_map_with_starts(selected, 0x1516, &starts, &starts);
    let initial = crate::app::loading::init::MapLoadInitial::from_test_map_source(
        prefix_test_map(&starts),
        crate::app::frontend::list_maps::LoadedMapSource::Generated {
            seed_name: selected.to_string(),
        },
    );
    let mut request = LoadingRequest::unverified_legacy_skirmish(
        launch,
        unverified_seed(0x1516),
        SkirmishSettings::default(),
    )
    .with_accepted_random_map(Some(accepted));

    let err = request.admit_context(&initial).unwrap_err();
    assert!(format!("{err:#}").contains("not the validated active-retail stock row"));
    assert!(request.accepted_rmg_start_staging.is_some());
}

#[test]
fn authored_prefix_rejects_random_map_staging_for_loose_and_mix_sources() {
    let selected = "mp01t4.map";
    let starts = [(0, 20, 24), (1, 42, 46)];
    let sources = [
        crate::app::frontend::list_maps::LoadedMapSource::Loose {
            path: std::path::PathBuf::from(selected),
            payload_len: 1,
        },
        crate::app::frontend::list_maps::LoadedMapSource::Mix {
            logical_name: selected.to_string(),
            source_archive: "mapsmd03.mix".to_string(),
            entry_id: 7,
            payload_len: 1,
        },
    ];
    for source in sources {
        let mut launch = test_launch_session(LaunchCountry::America);
        launch.selected_map_file = Some(selected.to_string());
        let accepted = accepted_random_map_with_starts(selected, 0x1616, &starts, &starts);
        let initial = crate::app::loading::init::MapLoadInitial::from_test_map_source(
            prefix_test_map(&starts),
            source,
        );
        let mut request = LoadingRequest::unverified_legacy_skirmish(
            launch,
            unverified_seed(0x1616),
            SkirmishSettings::default(),
        )
        .with_accepted_random_map(Some(accepted));

        let err = request.admit_context(&initial).unwrap_err();
        assert!(
            format!("{err:#}").contains("cannot attach to an authored map source"),
            "unexpected error: {err:#}"
        );
    }
}

#[test]
fn stock_prefix_rejects_legacy_fallback_source() {
    let starts = [(0, 20, 24), (1, 42, 46)];
    let launch = test_launch_session(LaunchCountry::America);
    let initial = crate::app::loading::init::MapLoadInitial::from_test_map_source(
        prefix_test_map(&starts),
        crate::app::frontend::list_maps::LoadedMapSource::LegacyFallback {
            label: "fixture".to_string(),
        },
    );
    let mut request = LoadingRequest::unverified_legacy_skirmish(
        launch,
        unverified_seed(0x1717),
        SkirmishSettings::default(),
    );

    let err = request.admit_context(&initial).unwrap_err();
    assert!(
        format!("{err:#}").contains("requires an exact Loose, MIX, or accepted generated source"),
        "unexpected error: {err:#}"
    );

    let mut next = 1;
    let mut accepted = LoadingRequest::accepted_skirmish(
        prepared_startup(&mut next, 0x1717),
        SkirmishSettings::default(),
    );
    let accepted_err = accepted.admit_context(&initial).unwrap_err();
    assert!(
        format!("{accepted_err:#}")
            .contains("requires an exact Loose, MIX, or accepted generated source"),
        "unexpected error: {accepted_err:#}"
    );
}

#[test]
fn gsi_04_12_stock_ffa_preview_can_only_supply_loading_fallback_pixels() {
    use crate::map::map_file::MapCell;
    use crate::map::waypoints::Waypoint;

    let mut launch = test_launch_session(LaunchCountry::America);
    launch.mode = SkirmishLaunchMode {
        id: 2,
        ui_name_key: "GUI:FreeForAll".to_string(),
        tooltip_key: "STT:ModeFreeForAll".to_string(),
        override_file: "MPFreeForAllMD.ini".to_string(),
        map_filter: "standard".to_string(),
        random_maps_allowed: true,
        allies_allowed: false,
        must_ally: false,
    };
    launch.selected_map_file = Some("RandMap.Sed".to_string());

    let mut map =
        crate::map::rmg::emit::empty_map_file(&crate::map::rmg::RmgOptions::default(), 100, 100);
    map.cells = vec![
        MapCell {
            rx: 60,
            ry: 60,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        },
        MapCell {
            rx: 60,
            ry: 140,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        },
        MapCell {
            rx: 140,
            ry: 60,
            tile_index: 0,
            sub_tile: 0,
            z: 0,
        },
    ];
    map.waypoints.insert(
        0,
        Waypoint {
            index: 0,
            rx: 70,
            ry: 70,
        },
    );
    map.waypoints.insert(
        1,
        Waypoint {
            index: 1,
            rx: 90,
            ry: 70,
        },
    );
    map.waypoints.insert(
        2,
        Waypoint {
            index: 2,
            rx: 70,
            ry: 90,
        },
    );
    let mut launch_map =
        crate::map::rmg::emit::empty_map_file(&crate::map::rmg::RmgOptions::default(), 100, 100);
    launch_map.cells = map.cells.clone();
    let active_scenario_waypoints = map.waypoints.clone();
    let generated = crate::map::rmg::GeneratedMap {
        map_file: map,
        mapgen_continuation: crate::map::rmg::RmgRng::new(0x4567).into_continuation(),
        construction_trace: crate::map::rmg::RmgConstructionTrace::default(),
        start_waypoints: vec![(0, 70, 70), (1, 90, 70), (2, 70, 90)],
        stages_run: Vec::new(),
        unfilled_start_slots: 0,
    };
    let _request = LoadingRequest::unverified_legacy_skirmish(
        launch.clone(),
        unverified_seed(0x4567),
        SkirmishSettings::default(),
    )
    .with_random_map_preview(Some(generated));
    let assignments = selected_map_start_assignments(&launch, None);
    assert!(
        assignments.is_empty(),
        "only the launch-time `.SED` regeneration may resolve participants"
    );
    let preview = DecodedPreview {
        width: 300,
        height: 100,
        rgba: vec![255; 300 * 100 * 4],
    };
    let composition = build_random_map_loading_composition(
        &launch,
        None,
        [800, 600],
        Some(preview),
        &launch_map,
        &active_scenario_waypoints,
        &assignments,
    );
    let prepared = composition.preview.expect("FFA retained preview");
    assert_eq!(
        prepared
            .image
            .rgba
            .chunks_exact(4)
            .filter(|pixel| *pixel == [0, 0, 0, 255])
            .count(),
        3 * 4 * 4,
        "all three valid FFA starts are burned black"
    );
    assert!(composition.markers.is_empty());
}

#[test]
fn random_map_progress_uses_native_integer_halving_and_raw_200_terminal() {
    let cadence = NativeLoadingProgressCadence::RandomMapHalved;

    assert_eq!(cadence.effective_percent(3), 1);
    assert_eq!(cadence.effective_percent(90), 45);
    assert_eq!(
        cadence.effective_percent(cadence.terminal_raw_percent()),
        100
    );
    assert_eq!(
        NativeLoadingProgressCadence::SelectedMap.effective_percent(3),
        3
    );
    assert_eq!(
        NativeLoadingProgressCadence::SelectedMap.terminal_raw_percent(),
        100
    );
}

#[test]
fn loading_session_falls_back_without_native_session_only_outside_parity_path() {
    let session = LoadingSession::from_request(LoadingRequest::generic_map_load(
        "auto",
        SkirmishSettings::default(),
    ));

    assert!(session.native.is_none());
    assert!(session.stage.request().skirmish_launch_session().is_none());
    assert_eq!(session.stage.request().selected_map_file(), "auto");
}

#[test]
fn loading_session_starts_at_initial_map_selection_phase() {
    let session = LoadingSession::from_request(LoadingRequest::unverified_legacy_skirmish(
        test_launch_session(LaunchCountry::America),
        unverified_seed(3),
        SkirmishSettings::default(),
    ));

    assert!(matches!(session.stage, LoadingStage::Selected(_)));
}

#[test]
fn loading_request_moves_exact_startup_authority_once() {
    struct Clock;
    impl crate::match_bootstrap::MatchSeedClock for Clock {
        fn low_u32(&mut self) -> u32 {
            0x1234_5678
        }

        fn source(&self) -> crate::match_bootstrap::MatchSeedSource {
            crate::match_bootstrap::MatchSeedSource::Controlled
        }

        fn seed_authority_certifying(&self) -> bool {
            true
        }
    }

    let session = test_launch_session(LaunchCountry::America);
    let accepted = match crate::match_bootstrap::classify_startup_session(&session) {
        crate::match_bootstrap::StartupSessionClassification::AcceptedExplicitFixedBattle(
            accepted,
        ) => accepted,
        other => panic!("fixture was not accepted: {other:?}"),
    };
    let mut next = 1;
    let correlation = crate::match_bootstrap::allocate_match_correlation(&mut next).unwrap();
    let mut clock = Clock;
    let prepared = crate::match_bootstrap::prepare_match_startup(correlation, accepted, &mut clock);
    let request = LoadingRequest::accepted_skirmish(prepared.clone(), SkirmishSettings::default());

    assert_eq!(request.startup().accepted(), Some(&prepared));
    let initial = MapLoadInitial::from_test_map_source(
        prefix_test_map(&[(0, 20, 24), (1, 42, 46)]),
        crate::app::frontend::list_maps::LoadedMapSource::Loose {
            path: PathBuf::from("mp01t4.map"),
            payload_len: 1,
        },
    );
    let loaded = request.prepare_initial(initial).unwrap();
    assert_eq!(loaded.request.startup, LoadingStartup::Accepted(prepared));
    assert_eq!(
        loaded.context.physical_source(),
        loaded.initial.map_source()
    );
}

#[test]
fn replacing_loading_startup_retires_prior_admission() {
    use crate::app::match_runtime::startup::MatchStartup;
    let mut next = 1;
    let prior = prepared_startup(&mut next, 0x1111_2222);
    let replacement = prepared_startup(&mut next, 0x3333_4444);
    let mut authority = MatchStartup::default();
    assert!(authority.admits_ordinary_tick());
    assert!(!authority.admits_exact_step());
    authority.begin(Some(prior.correlation));
    let simulation = crate::sim::world::Simulation::with_seed(u64::from(prior.seed.value));
    authority
        .acknowledge(prior.clone(), Some(&simulation), true, false)
        .unwrap();
    assert_eq!(authority.receipt(), Some(&receipt_for(&prior)));
    assert!(authority.admits_exact_step());

    // New loading attempt cannot expose the previous pair to captures.
    authority.begin(Some(replacement.correlation));
    assert!(authority.accepted().is_none());
    assert!(!authority.admits_exact_step());
    let baseline = authority.clone();
    assert!(
        authority
            .acknowledge(prior, Some(&simulation), true, false)
            .is_err()
    );
    assert_eq!(authority, baseline, "stale completion cannot install");
    let simulation = crate::sim::world::Simulation::with_seed(u64::from(replacement.seed.value));
    authority
        .acknowledge(replacement.clone(), Some(&simulation), true, false)
        .unwrap();
    assert_eq!(authority.startup(), Some(&replacement));
    let accepted = authority.clone();
    assert!(
        authority
            .acknowledge(replacement, Some(&simulation), true, false)
            .is_err()
    );
    assert_eq!(
        authority, accepted,
        "duplicate completion cannot overwrite evidence"
    );
    authority.clear();
    assert!(authority.accepted().is_none());
    assert!(authority.admits_ordinary_tick());
    assert!(!authority.admits_exact_step());
}

#[test]
fn rejected_startup_observation_does_not_publish_partial_admission() {
    use crate::app::match_runtime::startup::MatchStartup;
    let mut next = 1;
    let prepared = prepared_startup(&mut next, 0x5555_6666);
    let mut authority = MatchStartup::default();
    authority.begin(Some(prepared.correlation));
    let baseline = authority.clone();
    let simulation = crate::sim::world::Simulation::with_seed(u64::from(prepared.seed.value));
    for (sim, loading, spawn) in [
        (None, true, false),
        (Some(&simulation), false, false),
        (Some(&simulation), true, true),
    ] {
        assert!(
            authority
                .acknowledge(prepared.clone(), sim, loading, spawn)
                .is_err()
        );
        assert_eq!(authority, baseline);
        assert!(!authority.admits_exact_step());
    }
    authority.clear();
    assert!(authority.accepted().is_none());
    authority.begin(None);
    assert!(authority.admits_ordinary_tick());
    assert!(!authority.admits_exact_step());
}

#[test]
fn loading_progress_standard_skirmish_initializes_one_lane_max_100() {
    let progress = LoadingProgressState::standard_skirmish();

    assert_eq!(progress.max_value(), 100.0);
    assert_eq!(progress.current_value(), 0.0);
    assert_eq!(progress.current_percent(), 0.0);
}

#[test]
fn loading_progress_duplicate_milestones_do_not_redraw() {
    let mut progress = LoadingProgressState::standard_skirmish();

    assert!(progress.advance_progress(3));
    assert!(!progress.advance_progress(3));
    assert_eq!(progress.current_value(), 3.0);
}

#[test]
fn loading_progress_lower_milestone_does_not_redraw() {
    let mut progress = LoadingProgressState::standard_skirmish();

    assert!(progress.advance_progress(8));
    assert!(!progress.advance_progress(6));
    assert_eq!(progress.current_value(), 8.0);
}

#[test]
fn loading_progress_advancing_milestone_requests_redraw() {
    let mut progress = LoadingProgressState::standard_skirmish();

    assert!(progress.advance_progress(3));
    assert!(progress.advance_progress(8));
    assert_eq!(progress.current_value(), 8.0);
}

#[test]
fn loading_progress_clipped_width_matches_native_formula_for_exact_values() {
    let mut progress = LoadingProgressState::standard_skirmish();

    assert_eq!(progress.fill_width_gamemd_ftol_positive_domain(326), 0);
    assert!(progress.advance_progress(50));
    assert_eq!(progress.fill_width_gamemd_ftol_positive_domain(326), 163);
    assert!(progress.advance_progress(100));
    assert_eq!(progress.fill_width_gamemd_ftol_positive_domain(326), 326);
}

#[test]
fn loading_progress_suppresses_nonadvancing_raw_native_calls() {
    let mut progress = LoadingProgressState::standard_skirmish();

    assert!(progress.advance_progress(8));
    assert!(!progress.advance_progress(6));
    assert!(progress.advance_progress(60));
    assert!(!progress.advance_progress(58));
    assert!(!progress.advance_progress(60));
}

#[test]
fn loading_progress_theater_ramp_stock_rulesmd_count_emits_13_through_25() {
    let emitted = theater_ramp_changed_values(42);

    assert_eq!(emitted, (13..=25).collect::<Vec<_>>());
}

#[test]
fn loading_progress_theater_ramp_nonmultiple_base_count_uses_native_quotient() {
    let emitted = theater_ramp_changed_values(38);

    assert_eq!(emitted, (13..=25).collect::<Vec<_>>());
}

#[test]
fn loading_progress_theater_ramp_zero_or_invalid_small_count_has_no_dynamic_values() {
    assert!(theater_ramp_changed_values(0).is_empty());
    assert!(theater_ramp_changed_values(12).is_empty());
}

#[test]
fn loading_theater_cache_mismatch_covers_first_same_and_changed_cases() {
    assert!(theater_cache_mismatch(false, "TEMPERATE", "TEMPERATE"));
    assert!(!theater_cache_mismatch(true, "TEMPERATE", "temperate"));
    assert!(theater_cache_mismatch(true, "TEMPERATE", "SNOW"));
}

#[test]
fn selected_native_gate_preserves_metadata_and_raw_progress_cadence() {
    for (cadence, expected) in [
        (NativeLoadingProgressCadence::SelectedMap, 12.0),
        (NativeLoadingProgressCadence::RandomMapHalved, 6.0),
    ] {
        let mut native = NativeLoadingScreenState::standard_skirmish(
            LoadingArtVariant::Americans,
            0,
            HouseColorIndex(0),
            LoadingProgressRowSnapshot {
                label: "Player".into(),
            },
            cadence,
        );
        native.runtime_color_scheme_count = 16;
        {
            let mut phase = LoadingPhaseProgress::select(Some(&mut native), true, |_| {
                panic!("no atlas must select the real gated sink")
            });
            assert!(matches!(&phase.sink, SelectedProgressSink::Gated(_)));
            assert!(phase.native_theater_cache_mismatch);
            assert_eq!(phase.runtime_color_scheme_count, 16);
            for raw in [8, 6, 8, 12, 12] {
                phase.sink.milestone(raw);
            }
        }
        assert_eq!(native.progress.current_percent(), expected);
        {
            let mut phase = LoadingPhaseProgress::select(Some(&mut native), false, |_| {
                panic!("no atlas must select the real gated sink")
            });
            assert!(!phase.native_theater_cache_mismatch);
            phase.sink.milestone(cadence.terminal_raw_percent());
        }
        assert_eq!(native.progress.current_percent(), 100.0);
    }
}

#[test]
fn native_loading_state_keeps_the_full_player_progress_ramp() {
    let mk = |name: &str, hsv: [u8; 3]| ColorSchemeEntry {
        name: name.into(),
        hsv,
    };
    let ramps = HouseColorRamps::from_schemes(&[
        mk("Gold", [43, 239, 255]),
        mk("DarkBlue", [153, 214, 212]),
    ]);
    let mut native = NativeLoadingScreenState::standard_skirmish(
        LoadingArtVariant::Americans,
        0,
        HouseColorIndex(1),
        LoadingProgressRowSnapshot {
            label: "Player".to_owned(),
        },
        NativeLoadingProgressCadence::SelectedMap,
    );
    native.resolve_player_colors(
        &[mk("Gold", [43, 239, 255]), mk("DarkBlue", [153, 214, 212])],
        &ramps,
    );

    assert_eq!(native.runtime_color_scheme_count, 4);
    assert_eq!(native.progress_ramp, *ramps.ramp(HouseColorIndex(1)));
    assert_ne!(native.progress_ramp[0], native.progress_ramp[15]);
    assert!(native.progress_ramp[0].b > native.progress_ramp[0].r);
}

#[test]
fn both_native_loading_cadences_prepare_scenario_before_first_frame() {
    assert!(NativeLoadingProgressCadence::SelectedMap.prepares_scenario_before_first_frame());
    assert!(NativeLoadingProgressCadence::RandomMapHalved.prepares_scenario_before_first_frame());
}

#[test]
fn selected_generic_progress_uses_no_native_loader_metadata() {
    let mut phase = LoadingPhaseProgress::select(None, true, |_| {
        panic!("generic loading must never construct rendering progress")
    });
    assert!(matches!(&phase.sink, SelectedProgressSink::Generic(_)));
    assert!(!phase.native_theater_cache_mismatch);
    assert_eq!(phase.runtime_color_scheme_count, 0);
    for raw in [8, 6, 8, 12, 100, 200] {
        phase.sink.milestone(raw);
    }
}
