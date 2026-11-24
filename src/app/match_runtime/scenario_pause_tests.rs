use super::*;
use crate::app::match_runtime::session::SessionMode;
use crate::app::match_runtime::sim_tick::{RuntimePassInputs, decide_runtime_pass};
use crate::sim::trigger_runtime::TriggerEffect;

#[test]
fn original_pause_histories_pin_overflow_rearm_nesting_and_world_branch() {
    let controls = crate::test_fixture::json("tools/scenario_pause.json");
    assert_eq!(controls["controls"].as_array().unwrap().len(), 13);
    for row in controls["controls"].as_array().unwrap() {
        let mode = SessionMode::from_game_mode(row["mode"].as_i64().unwrap() as i32);
        let mut sim = crate::sim::world::Simulation::with_seed(0);
        sim.session_mut_for_test().set_input_locked_for_test(true);
        let mut clock = ScenarioPauseClock::default();
        for step in row["steps"].as_array().unwrap() {
            let ms = step["bucket"].as_u64().unwrap() * 16;
            let op = step["operation"].as_str().unwrap();
            let mut events = Vec::new();
            let effects = match op {
                "enter" => {
                    let value = step["value"].as_i64().unwrap() as i32;
                    clock.rearm(value, ms);
                    sim.pause_scenario_for_test(value)
                }
                "exit" => sim.resume_scenario(),
                "poll"
                    if matches!(mode, SessionMode::Campaign | SessionMode::Skirmish)
                        && clock.elapsed(ms)
                        && sim.session().scenario_pause_timed() =>
                {
                    sim.expire_scenario_pause_for_test()
                }
                "poll" => Vec::new(),
                _ => unreachable!(),
            };
            for effect in effects {
                match effect {
                    TriggerEffect::ScenarioPause {
                        outermost: true, ..
                    } => events.push("enter_audio"),
                    TriggerEffect::ScenarioResume => events.push("exit_audio"),
                    _ => {}
                }
            }
            if op == "poll" {
                let decision = decide_runtime_pass(RuntimePassInputs {
                    exact_step: false,
                    window_active: true,
                    focus_frozen: false,
                    startup_admitted: true,
                    frame_stepping: false,
                    paused: false,
                    scenario_paused: sim.session().scenario_paused(),
                    menu_open: false,
                    session_mode: mode,
                    pacer_timing_admits: true,
                    network_wait: false,
                    network_dialog_open: false,
                });
                events.push(if decision.run_sim {
                    "world_branch"
                } else {
                    "paused_branch"
                });
                if matches!(mode, SessionMode::Campaign | SessionMode::Skirmish)
                    && sim.session().scenario_paused()
                {
                    assert!(
                        !decision.scroll_input && !decision.camera_scroll,
                        "native pause bypasses55D8AB input poll"
                    );
                }
            }
            let words = serde_json::to_value(sim.session()).unwrap();
            assert_eq!(words["scenario_pause"]["depth"], step["depth"], "{step}");
            assert_eq!(
                u64::from(sim.session().scenario_pause_timed()),
                step["timed"].as_u64().unwrap(),
                "{step}"
            );
            assert_eq!(
                clock.timer.start_frame() as i64,
                step["timer_start"].as_i64().unwrap(),
                "{step}"
            );
            assert_eq!(
                clock.timer.duration() as i64,
                step["timer_duration"].as_i64().unwrap(),
                "{step}"
            );
            assert_eq!(serde_json::json!(events), step["events"], "{step}");
        }
    }
}

#[test]
fn radar_movie_service_cannot_add_or_release_a_recorded_pause() {
    use crate::sim::replay::{REPLAY_VERSION, ReplayHeader, ReplayLog};
    use crate::sim::runtime::SimRuntime;
    use crate::sim::world::Simulation;

    let mut runtime = SimHost::default();
    runtime.install(Some(SimRuntime::from_simulation(Simulation::with_seed(7))));
    let mut diagnostics = MatchDiagnosticsState::default();

    apply_live_service(&mut runtime, &mut diagnostics, ScenarioServiceInput::Pause);
    let sim = &runtime.runtime().unwrap().simulation;
    assert!(sim.session().scenario_paused());
    let paused_hash = sim.state_hash();

    let log = ReplayLog::new(ReplayHeader {
        version: REPLAY_VERSION,
        tick_hz: 15,
        seed: 7,
        map_name: String::new(),
        rules_hash: 0,
        pixel_conversion_bounds: Default::default(),
    });
    diagnostics.begin_playback(std::sync::Arc::new(log), None, "0".repeat(64));
    for input in [ScenarioServiceInput::Resume, ScenarioServiceInput::Pause] {
        assert!(apply_live_service(&mut runtime, &mut diagnostics, input).is_empty());
        assert_eq!(
            runtime.runtime().unwrap().simulation.state_hash(),
            paused_hash
        );
    }
}

#[test]
fn saved_timed_hold_rearms_zero_without_losing_an_untimed_hold() {
    let mut sim = crate::sim::world::Simulation::with_seed(0);
    sim.pause_scenario_for_test(0);
    sim.pause_scenario_for_test(120);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .restore_references_for_test()
        .unwrap();
    assert!(restored.session().scenario_paused());
    assert!(restored.session().scenario_pause_timed());
    let clock = ScenarioPauseClock::after_load(999_000); // PostRead starts now, duration0.
    assert!(clock.elapsed(999_000));
    assert!(restored.expire_scenario_pause_for_test().is_empty());
    assert!(restored.session().scenario_paused());
    assert!(!restored.session().scenario_pause_timed());
    assert!(
        restored
            .resume_scenario()
            .contains(&TriggerEffect::ScenarioResume)
    );
    assert!(!restored.session().scenario_paused());
}
