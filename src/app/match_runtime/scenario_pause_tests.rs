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
        sim.session.set_input_locked(true);
        let mut clock = ScenarioPauseClock::default();
        for step in row["steps"].as_array().unwrap() {
            let ms = step["bucket"].as_u64().unwrap() * 16;
            let op = step["operation"].as_str().unwrap();
            let mut events = Vec::new();
            let effects = match op {
                "enter" => {
                    let value = step["value"].as_i64().unwrap() as i32;
                    clock.rearm(value, ms);
                    sim.pause_scenario(value)
                }
                "exit" => sim.resume_scenario(),
                "poll"
                    if matches!(mode, SessionMode::Campaign | SessionMode::Skirmish)
                        && clock.elapsed(ms)
                        && sim.session.scenario_pause_timed() =>
                {
                    sim.expire_scenario_pause()
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
                    scenario_paused: sim.session.scenario_paused(),
                    menu_open: false,
                    session_mode: mode,
                    pacer_timing_admits: true,
                });
                events.push(if decision.run_sim {
                    "world_branch"
                } else {
                    "paused_branch"
                });
                if matches!(mode, SessionMode::Campaign | SessionMode::Skirmish)
                    && sim.session.scenario_paused()
                {
                    assert!(
                        !decision.scroll_input,
                        "native pause bypasses55D8AB input poll"
                    );
                }
            }
            let words = serde_json::to_value(&sim.session).unwrap();
            assert_eq!(words["scenario_pause"]["depth"], step["depth"], "{step}");
            assert_eq!(
                u64::from(sim.session.scenario_pause_timed()),
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
fn saved_timed_hold_rearms_zero_without_losing_an_untimed_hold() {
    let mut sim = crate::sim::world::Simulation::with_seed(0);
    sim.pause_scenario(0);
    sim.pause_scenario(120);
    let bytes = crate::sim::snapshot::GameSnapshot::save(&sim, 0, 0, "", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&bytes)
        .unwrap()
        .sim;
    restored.restore_after_snapshot_load().unwrap();
    assert!(restored.session.scenario_paused());
    assert!(restored.session.scenario_pause_timed());
    let clock = ScenarioPauseClock::after_load(999_000); // PostRead starts now, duration0.
    assert!(clock.elapsed(999_000));
    assert!(restored.expire_scenario_pause().is_empty());
    assert!(restored.session.scenario_paused());
    assert!(!restored.session.scenario_pause_timed());
    assert!(
        restored
            .resume_scenario()
            .contains(&TriggerEffect::ScenarioResume)
    );
    assert!(!restored.session.scenario_paused());
}
