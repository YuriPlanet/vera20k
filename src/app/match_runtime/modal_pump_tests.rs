use super::{
    SessionMode, audio_pump_follows_main_tick, resolve_score_rows, score_title_key,
    should_record_replay_tick, wall_clock_service_admission,
};

fn lend_plain_frame(host: &mut crate::app::match_runtime::sim_host::SimHost) {
    use super::{FrameEffects, FrameFinish, SIM_TICK_MS, ThreadedTick, TickLane};

    assert!(host.lend(Box::new(|runtime| {
        let native_frame = runtime.simulation.session().binary_frame();
        Box::new(ThreadedTick {
            finish: FrameFinish {
                native_frame,
                local_owner_for_fog: None,
                local_owner_name: None,
                defer_verdict_to_clip: false,
                detailed_eva_text: false,
                prefix_eva_text: Default::default(),
                deferred: None,
            },
            output: runtime.advance_frame(
                &[],
                SIM_TICK_MS,
                TickLane::Ordinary,
                &mut FrameEffects::empty(),
            ),
            tick_cost: Default::default(),
            logic_ms: (0, 0),
        })
    })));
    assert!(host.is_lent());
    assert!(host.home_runtime().is_none());
}

#[test]
fn replay_flush_drains_and_records_a_lent_frame() {
    use super::{ReplayFlush, replay_flush_facts};
    use crate::app::match_diagnostics::MatchDiagnosticsState;
    use crate::app::match_runtime::sim_host::SimHost;
    use crate::sim::{runtime::SimRuntime, world::Simulation};

    let mut sim = Simulation::new();
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    let runtime = SimRuntime::from_simulation(sim);
    let mut diagnostics = MatchDiagnosticsState::default();
    let replay = diagnostics.get_or_begin_replay(&runtime).unwrap();
    let mut host = SimHost::default();
    host.install(Some(runtime));
    lend_plain_frame(&mut host);

    let ReplayFlush::Sealed(tick) = replay_flush_facts(&mut host, Some(replay), false, false)
    else {
        panic!("an offline frame does not hand off to the network queue");
    };
    assert!(!host.is_lent());
    let runtime = host.home_runtime().unwrap();
    assert_eq!(tick, 1);
    assert_eq!(runtime.simulation.session().tick(), tick);
    assert_eq!(replay.ticks.len(), 1);
    assert_eq!(replay.ticks[0].tick, tick);
    assert_eq!(
        replay.ticks[0].state_hash,
        Some(runtime.simulation.state_hash())
    );
}

#[test]
#[ignore = "slow; verifies a lent frame's flushed replay against retail assets"]
fn flushing_lent_frame_verifies_its_completed_replay() {
    use super::{
        FrameEffects, ReplayFlush, SIM_TICK_MS, TickLane, record_replay_frame, replay_flush_facts,
    };
    use crate::app::match_diagnostics::MatchDiagnosticsState;
    use crate::app::match_runtime::sim_host::SimHost;
    use crate::map::source::test_support::TestDirectory;
    use crate::sim::replay::{ReplayLog, ReplayRunner};
    use crate::sim::scenario_bootstrap::MatchLaunchDescriptor;
    use crate::sim::scenario_session::ScenarioGameMode;

    const MAP: &str = "XMP03T4.MAP";
    let (retail, _) = crate::rules::retail_ini_fixture::retail_assets().expect("retail RA2_DIR");
    let launch = crate::headless_scenario::battle_session(
        MAP,
        &[(
            crate::skirmish_launch::LaunchCountry::Russia,
            crate::skirmish_launch::AiDifficulty::Normal,
        )],
    );
    let mut scene = crate::headless_scenario::load_with_launch_and_ini(
        &retail,
        MAP,
        73,
        MatchLaunchDescriptor::from_resolved(launch.clone()).unwrap(),
        &Default::default(),
        ScenarioGameMode::Skirmish,
        None,
        None,
    )
    .unwrap();
    let sim = &mut scene.runtime.simulation;
    sim.batch_path_searches();
    sim.batch_target_scans();
    sim.present_effect_anims();
    sim.separate_turn_streams(true);
    sim.sleep_idle_objects_in_match();
    sim.hash_state_every(std::num::NonZeroU64::MIN);
    let mut diagnostics = MatchDiagnosticsState::default();
    diagnostics.get_or_begin_replay(&scene.runtime).unwrap();
    diagnostics.retain_replay_startup(
        &scene.runtime,
        Some(crate::sim::replay::ReplayLaunchInput::Slots(&launch)),
        Some(&scene.source),
        Some(scene.map.ini.content_hash()),
        Default::default(),
    );
    let mut first = scene
        .runtime
        .advance_frame(
            &[],
            SIM_TICK_MS,
            TickLane::Ordinary,
            &mut FrameEffects::empty(),
        )
        .unwrap();
    record_replay_frame(diagnostics.replay_log.as_mut(), &mut first);
    let mut host = SimHost::default();
    host.install(Some(scene.runtime));
    lend_plain_frame(&mut host);

    let ReplayFlush::Sealed(tick) =
        replay_flush_facts(&mut host, diagnostics.replay_log.as_mut(), false, false)
    else {
        panic!("offline flush must seal its completed worker frame");
    };
    let scratch = TestDirectory::new("lent-frame-replay");
    let artifact = diagnostics
        .flush_to(tick, scratch.path(), 1)
        .unwrap()
        .unwrap();
    let replay = ReplayLog::load(&artifact.path).unwrap();
    let verified = ReplayRunner::verify(&retail, &replay, None).unwrap();
    assert_eq!(verified.final_tick, tick);
    assert_eq!(verified.checked_hashes, 2);
    assert_eq!(
        verified.final_state_hash,
        host.home_runtime().unwrap().simulation.state_hash()
    );
}

/// 0x005C9BE7: only GameMode 5 retitles 0x108.
#[test]
fn only_skirmish_retitles_the_score_dialog() {
    assert_eq!(score_title_key(SessionMode::Skirmish), "GUI:SkirmishScore");
    for mode in [SessionMode::Lan, SessionMode::Wol, SessionMode::Other] {
        assert_eq!(score_title_key(mode), "GUI:MultiplayerScore");
    }
}

#[test]
fn completed_cooperative_terminal_frame_without_score_is_recorded() {
    let tick = crate::sim::world::TickResult {
        tick: 10,
        frame_committed: false,
        executed_commands: 0,
        state_hash: Some(123),
        terminal_score_finalized: false,
        spawned_entities: false,
        destroyed_structure: false,
        ownership_changed: false,
        bridge_state_changed: false,
    };

    assert!(should_record_replay_tick(&tick));
    assert!(!should_record_replay_tick(&crate::sim::world::TickResult {
        state_hash: None,
        ..tick
    }));
}

#[test]
fn gsi_01_03_debug_pause_does_not_impersonate_an_offline_modal() {
    let debug_pause = wall_clock_service_admission(true, false, SessionMode::Skirmish);
    assert!(!debug_pause.simulation);
    assert!(debug_pause.tactical_mutation);

    let actual_modal = wall_clock_service_admission(true, true, SessionMode::Skirmish);
    assert!(!actual_modal.simulation);
    assert!(!actual_modal.tactical_mutation);
}

#[test]
fn gsi_01_03_tactical_view_mutators_follow_modal_service_admission() {
    let admitted = super::WallClockServiceAdmission {
        simulation: true,
        tactical_mutation: true,
    };
    // No modal means the ordinary app loop owns camera/placement updates.
    for mode in [
        SessionMode::Campaign,
        SessionMode::Skirmish,
        SessionMode::Lan,
        SessionMode::Wol,
    ] {
        assert_eq!(wall_clock_service_admission(false, false, mode), admitted);
    }

    // The active offline Menu, Abort and Options callers all enter the
    // service-only return: their frozen battlefield cannot pan or rebuild a
    // placement preview underneath the dialog.
    for mode in [SessionMode::Campaign, SessionMode::Skirmish] {
        assert_eq!(
            wall_clock_service_admission(true, true, mode),
            super::WallClockServiceAdmission {
                simulation: false,
                tactical_mutation: false,
            }
        );
    }

    // A network match keeps running behind the modal.
    for mode in [SessionMode::Lan, SessionMode::Wol] {
        assert_eq!(wall_clock_service_admission(true, true, mode), admitted);
    }
}

#[test]
fn offline_matches_leave_the_audio_pump_to_main_tick() {
    // g_GameMode 0 and 5 only: Main::ThrottleFrame 0x0055E160 only sleeps.
    assert!(audio_pump_follows_main_tick(
        SessionMode::Campaign,
        true,
        false
    ));
    assert!(audio_pump_follows_main_tick(
        SessionMode::Skirmish,
        true,
        false
    ));
    assert!(!audio_pump_follows_main_tick(SessionMode::Lan, true, false));
    assert!(!audio_pump_follows_main_tick(
        SessionMode::Other,
        true,
        false
    ));
    // The in-game menu's modal pump 0x00623120 services its own passes.
    assert!(!audio_pump_follows_main_tick(
        SessionMode::Skirmish,
        true,
        true
    ));
}

#[test]
fn retail_score_row_colours_match_the_retail_still() {
    // Retail still at 800x600: the Easy AI (DarkRed) and the local player
    // (DarkBlue, Color=2) rows, in RGB565 units.
    let Some(rules) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let schemes = crate::rules::color_scheme::parse_color_schemes(&rules);
    for (name, units) in [("DarkRed", [31, 6, 3]), ("DarkBlue", [4, 26, 26])] {
        let entry = crate::rules::color_scheme::scheme_entry_by_name(&schemes, name)
            .unwrap_or_else(|| panic!("[Colors] {name}"));
        let rgb = crate::rules::house_colors::house_text_rgb(
            &schemes,
            crate::rules::house_colors::HouseColorIndex(entry as u8),
        );
        assert_eq!([rgb[0] >> 3, rgb[1] >> 2, rgb[2] >> 3], units, "{name}");
    }
}

#[test]
fn score_rows_follow_the_houses_names_colours_and_the_native_order() {
    use crate::sim::house_state::HouseState;
    // Retail still: the local player (DarkBlue) and one Easy AI (DarkRed),
    // both at score 0; the dialog lists "Computer" first.
    let schemes = crate::rules::color_scheme::parse_color_schemes(
        &crate::rules::ini_parser::IniFile::from_str(
            "[Colors]\nDarkRed=0,230,255\nDarkBlue=153,214,212\n",
        ),
    );
    let mut sim = crate::sim::world::Simulation::with_seed(1);
    let player = sim.intern("Player");
    let computer = sim.intern("Computer1");
    let mut player_house = HouseState::new(player, 0, None, true, 0, 10);
    player_house.set_ui_name_for_test(crate::sim::house_state::HouseUiName::handle("[New Player]"));
    let mut computer_house = HouseState::new(computer, 1, None, false, 0, 10);
    computer_house.set_ui_name_for_test(crate::sim::house_state::HouseUiName::Computer);
    sim.houses_mut_for_test().register(player_house);
    sim.houses_mut_for_test().register(computer_house);
    let row = |owner, losses| crate::sim::score::TerminalScoreRowSnapshot {
        owner,
        country: None,
        survived: owner == computer,
        kills: 0,
        losses,
        built: 0,
        raw_score: 0,
        score: 0,
    };
    let snapshot = crate::sim::score::TerminalScoreSnapshot {
        rows: vec![row(player, 15), row(computer, 0)],
    };
    let colors = crate::map::houses::HouseColorMap::from([
        (
            "Player".to_string(),
            crate::rules::house_colors::HouseColorIndex(1),
        ),
        (
            "Computer1".to_string(),
            crate::rules::house_colors::HouseColorIndex(0),
        ),
    ]);
    let rows = resolve_score_rows(
        &sim,
        &snapshot,
        &crate::app::presentation::house_ui::HouseUiSources {
            csf: None,
            computer: "Computer",
            schemes: &schemes,
            colors: &colors,
        },
        |_| None,
    );
    let shown: Vec<(&str, [u8; 3], u32)> = rows
        .iter()
        .map(|row| (row.name.as_str(), row.rgb, row.losses))
        .collect();
    assert_eq!(
        shown,
        [
            ("Computer", [255, 25, 25], 0),
            ("[New Player]", [34, 105, 212], 15)
        ]
    );
    // A House still holding its type's UIName shows its country, else
    // its House name.
    sim.houses_mut_for_test()
        .get_mut(&player)
        .unwrap()
        .set_ui_name_for_test(crate::sim::house_state::HouseUiName::TypeName);
    let rows = resolve_score_rows(
        &sim,
        &snapshot,
        &crate::app::presentation::house_ui::HouseUiSources {
            csf: None,
            computer: "Computer",
            schemes: &schemes,
            colors: &colors,
        },
        |_| None,
    );
    assert_eq!(rows[1].name, "Player");
    // A map `UIName=` key resolves through the string table, then the
    // field keeps 20 characters (500CD7..500CEC).
    sim.houses_mut_for_test()
        .get_mut(&player)
        .unwrap()
        .set_ui_name_for_test(crate::sim::house_state::HouseUiName::Csf(
            "Name:AVeryLongHouseUiNameKey".into(),
        ));
    let rows = resolve_score_rows(
        &sim,
        &snapshot,
        &crate::app::presentation::house_ui::HouseUiSources {
            csf: None,
            computer: "Computer",
            schemes: &schemes,
            colors: &colors,
        },
        |_| None,
    );
    assert_eq!(rows[1].name, "Name:AVeryLongHouseU");
}
