use super::RandomMapGenerationRetention;
use super::frontend::skirmish_session::{OfflineSkirmishRuntime, skirmish_global_defaults};
use super::loading::pump::{LoadingProgressSink, LoadingRequest};
use super::shell_random_map::{
    RandomMapUpdate, accept_random_map_setup_owners, begin_random_map_generation_owners,
    cancel_random_map_setup_owners, finish_random_map_generation_owners,
    prepare_random_map_generation, spawn_random_map_generation_worker,
};
use crate::map::rmg::{RmgConstructionPhase, RmgConstructionTrace, RmgOptions, RmgRng};
use crate::skirmish_launch::{
    AiDifficulty, LaunchCountry, LaunchStartPosition, LaunchTeam, SkirmishAiSlot,
    SkirmishLaunchMode, SkirmishLaunchOptions, SkirmishLaunchSession, SkirmishLocalSlot,
};
use crate::ui::skirmish_shell::{
    AcceptOutcome, ChooseMapSelection, RandomMapSetupModalState, SkirmishShellState,
};

struct SilentProgress;

impl LoadingProgressSink for SilentProgress {
    fn milestone(&mut self, _percent: u32) {}
}

fn launch_session(seed_name: &str) -> SkirmishLaunchSession {
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
        selected_map_file: Some(seed_name.to_string()),
        player_name: "Player".to_string(),
        local: SkirmishLocalSlot {
            country: LaunchCountry::America,
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

fn start_production_worker(
    assets: &mut crate::assets::asset_manager::AssetManager,
    options: &RmgOptions,
    runtime: &mut OfflineSkirmishRuntime,
    retention: &mut RandomMapGenerationRetention,
    shared_cell_dummy: &crate::map::resolved_terrain::SharedCellDummy,
) -> std::sync::mpsc::Receiver<RandomMapUpdate> {
    begin_random_map_generation_owners(runtime, retention, options);
    let prepared = prepare_random_map_generation(assets, options)
        .expect("production retail generation preparation");
    let decision = retention.map_storage_decision(options);
    let receiver = spawn_random_map_generation_worker(
        options.clone(),
        prepared.settings,
        prepared.resolved_inputs,
        prepared.blocks,
        prepared.tech_types,
        shared_cell_dummy.clone(),
        decision,
    )
    .expect("production random-map worker");
    retention.commit_map_storage_decision(decision);
    receiver
}

fn receive_production_generation(
    receiver: std::sync::mpsc::Receiver<RandomMapUpdate>,
) -> (usize, crate::map::rmg::GeneratedMap) {
    let mut entries = 0;
    loop {
        match receiver
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("production random-map worker receipt")
        {
            RandomMapUpdate::Started => entries += 1,
            RandomMapUpdate::Progress(_) => {}
            RandomMapUpdate::Finished(generated) => {
                assert_eq!(entries, 1, "each production worker enters generation once");
                return (entries, *generated);
            }
        }
    }
}

fn lifecycle_preview(color: [u8; 3]) -> crate::map::rmg::preview::PreviewImage {
    crate::map::rmg::preview::PreviewImage {
        width: 1,
        height: 1,
        rgba: vec![color[0], color[1], color[2], 0xFF],
    }
}

#[test]
#[ignore] // Requires RA2_DIR (active-retail YR files).
fn gsi_04_12_random_map_ui_to_sed_launch_lifecycle_converges() {
    let retail_dir = std::path::PathBuf::from(
        std::env::var("RA2_DIR").expect("set RA2_DIR to the active-retail YR directory"),
    );
    assert!(retail_dir.join("gamemd.exe").is_file());
    crate::map::rmg::trig::install_from_dir(&retail_dir);
    let mut assets = crate::assets::asset_manager::AssetManager::new(&retail_dir)
        .expect("active-retail AssetManager");
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    let seed_dir = std::env::temp_dir().join(format!(
        "vera20k-rmg-ui-launch-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&seed_dir).expect("lifecycle seed directory");
    let seed_name = "Unit2Lifecycle.Sed";
    let options = RmgOptions {
        theater: 0,
        map_type: 3,
        resources: 2,
        ruggedness: 35,
        water_amount: 50,
        num_players: 2,
        tiberium: 35,
        vegetation: 30,
        urban_presence: 40,
        seed: 4242,
        description: "Unit 2 lifecycle".into(),
        ..Default::default()
    };
    std::fs::write(seed_dir.join(seed_name), options.to_sed_bytes()).expect("write lifecycle .SED");
    const MATCH_SEED: u32 = 0x0412_0419;

    // U2-02: collect cross-thread receipts from the production worker. Every
    // Generate enters the real generator once; accepting a valid preview adds
    // no run, while accepting an empty setup adds exactly one.
    let previous_selection = ChooseMapSelection {
        mode_id: 1,
        record_index: Some(7),
    };
    let mut runtime = OfflineSkirmishRuntime::initialize(
        0x0412_0003,
        None,
        None,
        None,
        skirmish_global_defaults(&SkirmishShellState::default()),
    );
    let shared_cell_dummy = crate::map::resolved_terrain::SharedCellDummy::fresh();
    let mut generation_entries = 0;
    let mut retention = RandomMapGenerationRetention::default();
    let mut generated_modal = Some(RandomMapSetupModalState::open(
        options.clone(),
        Some(previous_selection),
        false,
    ));
    generated_modal
        .as_mut()
        .expect("generated modal")
        .begin_generate();
    let receiver = start_production_worker(
        &mut assets,
        &options,
        &mut runtime,
        &mut retention,
        &shared_cell_dummy,
    );
    let (entries, generated_a) = receive_production_generation(receiver);
    generation_entries += entries;
    let signature_a = (
        generated_a.map_file.ini.content_hash(),
        generated_a.construction_trace.clone(),
        generated_a.start_waypoints.clone(),
        generated_a.stages_run.clone(),
        generated_a.unfilled_start_slots,
    );
    assert!(!finish_random_map_generation_owners(
        &mut runtime,
        &mut retention,
        generated_modal.as_mut().expect("generated modal"),
        generated_a,
        Some(lifecycle_preview([0x10, 0x20, 0x30])),
        false,
    ));

    generated_modal
        .as_mut()
        .expect("generated modal")
        .begin_generate();
    let receiver = start_production_worker(
        &mut assets,
        &options,
        &mut runtime,
        &mut retention,
        &shared_cell_dummy,
    );
    let (entries, generated_b) = receive_production_generation(receiver);
    generation_entries += entries;
    let signature_b = (
        generated_b.map_file.ini.content_hash(),
        generated_b.construction_trace.clone(),
        generated_b.start_waypoints.clone(),
        generated_b.stages_run.clone(),
        generated_b.unfilled_start_slots,
    );
    assert!(!finish_random_map_generation_owners(
        &mut runtime,
        &mut retention,
        generated_modal.as_mut().expect("generated modal"),
        generated_b,
        Some(lifecycle_preview([0x11, 0x21, 0x31])),
        false,
    ));
    assert_eq!(generation_entries, 2);
    assert_eq!(
        signature_a, signature_b,
        "repeated Generate restarts MapGen"
    );

    let randmap_img = seed_dir.join("RandMap.img");
    let randmap_sed = seed_dir.join("RandMap.Sed");
    let committed = accept_random_map_setup_owners(
        &mut runtime,
        &mut generated_modal,
        &mut retention,
        Some(&seed_dir),
    )
    .expect("Use Map accepts the completed production result");
    assert_eq!(generation_entries, 2, "valid preview adds no worker run");
    assert!(randmap_img.is_file());
    assert!(!randmap_sed.exists(), "accepted caller follows teardown");
    std::fs::write(&randmap_sed, committed.to_sed_bytes()).expect("accepted caller writes .SED");

    let mut empty_modal = Some(RandomMapSetupModalState::open(
        options.clone(),
        Some(previous_selection),
        false,
    ));
    assert_eq!(
        empty_modal.as_ref().expect("empty modal").accept(),
        AcceptOutcome::NeedsGenerate
    );
    empty_modal.as_mut().expect("empty modal").begin_generate();
    let receiver = start_production_worker(
        &mut assets,
        &options,
        &mut runtime,
        &mut retention,
        &shared_cell_dummy,
    );
    let (entries, generated_for_accept) = receive_production_generation(receiver);
    generation_entries += entries;
    let signature_for_accept = (
        generated_for_accept.map_file.ini.content_hash(),
        generated_for_accept.construction_trace.clone(),
        generated_for_accept.start_waypoints.clone(),
        generated_for_accept.stages_run.clone(),
        generated_for_accept.unfilled_start_slots,
    );
    assert!(finish_random_map_generation_owners(
        &mut runtime,
        &mut retention,
        empty_modal.as_mut().expect("empty modal"),
        generated_for_accept,
        Some(lifecycle_preview([0x12, 0x22, 0x32])),
        true,
    ));
    assert_eq!(signature_a, signature_for_accept);
    assert_eq!(generation_entries, 3, "empty OK adds exactly one run");
    let _ = accept_random_map_setup_owners(
        &mut runtime,
        &mut empty_modal,
        &mut retention,
        Some(&seed_dir),
    )
    .expect("deferred OK accepts after production completion");

    // U2-03: preview constructor effects remain on the process shell Scenario
    // owner after Cancel; the chooser selection and edited options survive,
    // common teardown publishes only the preview product.
    std::fs::remove_file(&randmap_sed).expect("clear accepted caller fixture");
    let shell_before_cancel_generation = runtime.scenario_rng_logical_state_for_test();
    let mut cancel_modal = Some(RandomMapSetupModalState::open(
        options.clone(),
        Some(previous_selection),
        false,
    ));
    cancel_modal
        .as_mut()
        .expect("cancel modal")
        .begin_generate();
    let receiver = start_production_worker(
        &mut assets,
        &options,
        &mut runtime,
        &mut retention,
        &shared_cell_dummy,
    );
    let (entries, generated_for_cancel) = receive_production_generation(receiver);
    generation_entries += entries;
    assert!(!finish_random_map_generation_owners(
        &mut runtime,
        &mut retention,
        cancel_modal.as_mut().expect("cancel modal"),
        generated_for_cancel,
        Some(lifecycle_preview([0x13, 0x23, 0x33])),
        false,
    ));
    let shell_after_preview = runtime.scenario_rng_logical_state_for_test();
    assert_ne!(shell_after_preview, shell_before_cancel_generation);
    assert_eq!(
        cancel_random_map_setup_owners(
            &mut runtime,
            &mut cancel_modal,
            &mut retention,
            Some(&seed_dir),
        ),
        Some(previous_selection)
    );
    assert_eq!(generation_entries, 4);
    assert!(randmap_img.is_file(), "Cancel publishes completed preview");
    assert!(
        !randmap_sed.exists(),
        "Cancel never reaches accepted caller"
    );
    assert!(
        retention
            .take_preview_for_loading(Some(seed_name))
            .is_none(),
        "Cancel discards the generated candidate"
    );
    assert_eq!(
        runtime.random_map_options_for_setup(),
        options,
        "Cancel/reopen preserves the process MapSeed record"
    );
    assert_eq!(
        runtime.scenario_rng_logical_state_for_test(),
        shell_after_preview,
        "Cancel/reopen continues the preview-advanced shell Scenario cursor"
    );
    let reopened = RandomMapSetupModalState::open(
        runtime.random_map_options_for_setup(),
        Some(previous_selection),
        false,
    );
    assert_eq!(reopened.options, options);
    assert_eq!(reopened.previous_selection, Some(previous_selection));
    assert_eq!(
        runtime.scenario_rng_logical_state_for_test(),
        shell_after_preview,
        "reopening presentation state cannot reseed the shell Scenario owner"
    );

    // U2-04/U2-05/U2-19: transport a deliberately poisoned accepted preview
    // through the same retention/request boundary used by Start. The request's
    // production initial-map entry must regenerate from .SED and converge with
    // a direct .SED launch in every generated gameplay fact.
    let direct_initial = super::loading::init::load_map_initial_with_assets(
        seed_dir.clone(),
        &mut assets,
        Some(seed_name),
        &mut SilentProgress,
    )
    .expect("direct .SED launch regeneration");
    let accepted_start_waypoints: Vec<_> = (0..crate::skirmish_launch::SKIRMISH_PLAYER_SLOT_COUNT)
        .filter_map(|slot| {
            direct_initial
                .map_data()
                .waypoints
                .get(&(slot as u32))
                .map(|waypoint| (slot as u8, waypoint.rx, waypoint.ry))
        })
        .collect();
    let mut poison_options = options.clone();
    poison_options.seed = 0x7BAD;
    let mut poison_map = crate::map::rmg::emit::empty_map_file(&poison_options, 32, 32);
    poison_map.header.theater = "POISON_PREVIEW".to_string();
    let mut poison_trace = RmgConstructionTrace::default();
    poison_trace.push_discarded(RmgConstructionPhase::NeutralTech, "POISON_TECH".to_string());
    let poison_trace_reference = poison_trace.clone();
    let poison_mapgen_reference = crate::sim::rng::SimRng::from_mapgen_continuation(
        RmgRng::new(poison_options.seed_u16()).into_continuation(),
    )
    .logical_state();
    let poison = crate::map::rmg::GeneratedMap {
        map_file: poison_map,
        mapgen_continuation: RmgRng::new(poison_options.seed_u16()).into_continuation(),
        construction_trace: poison_trace,
        start_waypoints: accepted_start_waypoints.clone(),
        stages_run: Vec::new(),
        unfilled_start_slots: 0,
    };
    let mut retention = RandomMapGenerationRetention::default();
    retention.finish_generation(poison);
    retention.accept_setup(seed_name);
    retention.destroy_map_storage();
    let accepted_poison = retention
        .take_acceptance_for_loading(Some(seed_name))
        .expect("accepted setup transfers preview and staged starts once");
    let launch = launch_session(seed_name);
    let ui_request = LoadingRequest::unverified_legacy_skirmish(
        launch.clone(),
        crate::match_bootstrap::MatchSeed {
            value: MATCH_SEED,
            source: crate::match_bootstrap::MatchSeedSource::Controlled,
            seed_authority_certifying: false,
        },
        crate::ui::main_menu::SkirmishSettings::default(),
    )
    .with_accepted_random_map(Some(accepted_poison));
    let ui_launch = ui_request
        .load_random_map_snapshot_for_test(seed_dir.clone(), &mut assets, &mut SilentProgress)
        .expect("accepted UI .SED regeneration and admitted bundle transfer");

    // The reference arm must cross the same accepted-staging admission seam;
    // an arbitrary direct `.SED` is intentionally not a parity builder.
    let direct_preview = crate::map::rmg::GeneratedMap {
        map_file: crate::map::rmg::emit::empty_map_file(&options, 32, 32),
        mapgen_continuation: RmgRng::new(options.seed_u16()).into_continuation(),
        construction_trace: RmgConstructionTrace::default(),
        start_waypoints: accepted_start_waypoints.clone(),
        stages_run: Vec::new(),
        unfilled_start_slots: 0,
    };
    let mut direct_retention = RandomMapGenerationRetention::default();
    direct_retention.finish_generation(direct_preview);
    direct_retention.accept_setup(seed_name);
    let direct_accepted = direct_retention
        .take_acceptance_for_loading(Some(seed_name))
        .expect("reference setup supplies accepted staging");
    let direct_request = LoadingRequest::unverified_legacy_skirmish(
        launch,
        crate::match_bootstrap::MatchSeed {
            value: MATCH_SEED,
            source: crate::match_bootstrap::MatchSeedSource::Controlled,
            seed_authority_certifying: false,
        },
        crate::ui::main_menu::SkirmishSettings::default(),
    )
    .with_accepted_random_map(Some(direct_accepted));
    let direct_launch = direct_request
        .prepare_random_map_snapshot_for_test(direct_initial, &mut assets)
        .expect("reference generated bundle crosses accepted admission");
    assert_eq!(ui_launch, direct_launch);
    assert_ne!(ui_launch.map.header.0, "POISON_PREVIEW");
    assert_ne!(ui_launch.trace, poison_trace_reference);
    assert_ne!(ui_launch.mapgen_continuation, poison_mapgen_reference);
    assert!(
        !ui_launch.trace.events.is_empty(),
        "retail fixture must exercise launch constructor replay"
    );
    assert_eq!(
        ui_launch.emitted_constructor_words,
        direct_launch.emitted_constructor_words
    );
    assert_eq!(
        ui_launch.scenario_after_trace,
        direct_launch.scenario_after_trace
    );
    assert!(
        !ui_launch.installed_constructor_words.is_empty(),
        "retail generated entities must enter the production construction funnel"
    );
    assert!(
        ui_launch
            .installed_constructor_words
            .iter()
            .all(|(_, expected, installed)| expected == installed),
        "every replayed constructor word must be installed on its Simulation entity"
    );
    assert_eq!(
        ui_launch.emitted_constructor_words,
        ui_launch
            .installed_constructor_words
            .iter()
            .map(|(index, expected, _)| (*index, *expected))
            .collect::<Vec<_>>(),
        "the emitted trace table and installed generated entities must be one-to-one"
    );
    assert_eq!(ui_launch.final_rng, direct_launch.final_rng);
    assert_ne!(
        ui_launch.final_rng.scenario, ui_launch.scenario_after_trace,
        "Battle projection and Post_Map_Init must continue the replayed Scenario cursor"
    );
    assert_eq!(
        ui_launch.final_rng.mapgen, ui_launch.mapgen_continuation,
        "Full Init and Post Map preserve the launch-generated MapGen continuation"
    );
    assert!(
        ui_launch.post_map_output.navigation_published,
        "shared Post_Map_Init must publish first navigation authority"
    );
    assert!(
        ui_launch.post_map_output.tiberium_queues.is_none(),
        "the generator tail built the queues before germination; Post_Map_Init must not rebuild them"
    );
    assert_eq!(
        ui_launch.post_map_output.crates,
        Some(crate::sim::crates::CratePlacement {
            requested: 1,
            accepted: 1,
            visible: ui_launch
                .startup_crate_slots
                .iter()
                .filter(|(_, _, overlay)| overlay.is_some())
                .count() as u32,
        }),
        "accepted playable RMG must reach the ordinary startup-crate post-map seam"
    );
    assert_eq!(ui_launch.startup_crate_slots.len(), 1);
    let (slot_index, slot, live_overlay) = ui_launch.startup_crate_slots[0];
    assert_eq!(slot_index, 0, "first accepted crate owns slot zero");
    assert!(!slot.is_empty());
    assert_eq!(slot.start_frame, 0);
    assert_ne!(slot.aux, 0);
    assert!(slot.duration > 0);
    let (_, data) = live_overlay.expect("retail lifecycle fixture places a visible startup crate");
    assert_eq!(data, u8::MAX);
    assert!(ui_launch.startup_crate.crates_enabled);
    assert_eq!(ui_launch.startup_crate.minimum, 1);
    assert_eq!(ui_launch.startup_crate.maximum, 255);
    assert_eq!(ui_launch.startup_crate.regen_bits, 3.0_f64.to_bits());
    assert_eq!(ui_launch.startup_crate.wood_name.as_deref(), Some("CRATE"));
    assert_eq!(
        ui_launch.startup_crate.common_name.as_deref(),
        Some("CRATE")
    );
    assert_eq!(
        ui_launch.startup_crate.water_name.as_deref(),
        Some("WCRATE")
    );
    assert_eq!(
        ui_launch.startup_crate.wood_id, ui_launch.startup_crate.common_id,
        "stock wood/common image aliases resolve to one overlay identity"
    );
    let wood_id = ui_launch
        .startup_crate
        .wood_id
        .expect("installed CRATE overlay identity");
    let water_id = ui_launch
        .startup_crate
        .water_id
        .expect("installed WCRATE overlay identity");
    assert_ne!(wood_id, water_id);
    assert_eq!(ui_launch.startup_crate.body_frame, 0);
    assert!(
        ui_launch
            .startup_crate
            .runtime_names
            .contains(&(wood_id, "CRATE".to_string()))
    );
    assert!(
        ui_launch
            .startup_crate
            .runtime_names
            .contains(&(water_id, "WCRATE".to_string()))
    );
    assert_eq!(ui_launch.startup_crate.presented.len(), 1);
    assert_eq!(
        ui_launch.startup_crate.presented[0],
        (
            u16::try_from(slot.cell_x).expect("positive crate x"),
            u16::try_from(slot.cell_y).expect("positive crate y"),
            live_overlay.expect("visible overlay").0,
            u8::MAX,
        )
    );
    println!(
        "STARTUP_CRATE_PRODUCTION rules=min:{} max:{} regen_bits:{:#018X} wood:{} common:{} water:{} ids:{wood_id}/{water_id} frame:{}",
        ui_launch.startup_crate.minimum,
        ui_launch.startup_crate.maximum,
        ui_launch.startup_crate.regen_bits,
        ui_launch.startup_crate.wood_name.as_deref().unwrap(),
        ui_launch.startup_crate.common_name.as_deref().unwrap(),
        ui_launch.startup_crate.water_name.as_deref().unwrap(),
        ui_launch.startup_crate.body_frame,
    );
    println!(
        "STARTUP_CRATE_SLOT index:{slot_index} cell:{},{} start:{} aux:{:#010X} duration:{} overlay:{:?}",
        slot.cell_x, slot.cell_y, slot.start_frame, slot.aux, slot.duration, live_overlay
    );
    for asset_name in ["CRATE.TEM", "WCRATE.TEM"] {
        let bytes = assets
            .get_ref(asset_name)
            .unwrap_or_else(|| panic!("active theater archive resolves {asset_name}"));
        let shp = crate::assets::shp_file::ShpFile::from_bytes(bytes)
            .unwrap_or_else(|err| panic!("parse active {asset_name} as SHP(TS): {err}"));
        assert_eq!((shp.width, shp.height, shp.frames.len()), (60, 60, 2));
        println!(
            "STARTUP_CRATE_ASSET name:{asset_name} bytes:{} format:SHP(TS) canvas:{}x{} frames:{}",
            bytes.len(),
            shp.width,
            shp.height,
            shp.frames.len()
        );
    }

    std::fs::remove_file(seed_dir.join(seed_name)).expect("remove lifecycle .SED");
    std::fs::remove_file(randmap_img).expect("remove common-teardown preview");
    std::fs::remove_dir(seed_dir).expect("remove lifecycle seed directory");
}
