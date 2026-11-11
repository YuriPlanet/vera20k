//! Offline input polls follow Main_Tick, not display refresh: original 0x55D8AB
//! polls GScreen once and ThrottleFrame 0x55E160 skips its additional input poll
//! at 0x55E253 in campaign (0) and skirmish (5). The camera's scroll steps follow
//! neither, by design, so the scroll speed does not change with the game speed.

use super::{RuntimePassInputs, SessionMode, decide_runtime_pass};
use crate::app::input::camera::EdgeScrollState;
use crate::app::match_runtime::frame_pacer::LocalFramePacer;
use crate::sim::game_options::GAME_SPEED_90_TPS;

fn ordinary_pass(session_mode: SessionMode, pacer_timing_admits: bool) -> RuntimePassInputs {
    RuntimePassInputs {
        exact_step: false,
        window_active: true,
        focus_frozen: false,
        startup_admitted: true,
        frame_stepping: false,
        paused: false,
        scenario_paused: false,
        menu_open: false,
        session_mode,
        pacer_timing_admits,
        network_wait: false,
        network_dialog_open: false,
    }
}

#[test]
fn scroll_steps_are_the_same_at_every_game_speed() {
    // The slowest and fastest paced speeds, Unlimited and the online preset.
    for (mode, game_speed) in [
        (SessionMode::Skirmish, 6),
        (SessionMode::Skirmish, GAME_SPEED_90_TPS),
        (SessionMode::Skirmish, 0),
        (SessionMode::Lan, 1),
    ] {
        let mut pacer = LocalFramePacer::new();
        let mut clock = EdgeScrollState::default();
        let mut steps = 0;
        // One pass per service wake, as a battlefield between draws runs them.
        for now_ms in (0..=1000).step_by(8) {
            let admitted = pacer.should_admit(now_ms, game_speed, None, false);
            let decision = decide_runtime_pass(ordinary_pass(mode, admitted));
            if decision.run_sim {
                pacer.record_admitted_frame(now_ms);
            }
            let due = clock.take_scroll_steps(
                (now_ms >> 4) as u32,
                decision.camera_scroll,
                false,
                decision.run_sim,
            );
            assert!(
                due <= 1,
                "steps waited for a game frame: {mode:?} {game_speed}"
            );
            steps += due;
        }
        // One step per 16 ms bucket touched: buckets 0..=62.
        assert_eq!(steps, 63, "{mode:?}, GameSpeed={game_speed}");
    }
}

#[test]
fn repeated_redraws_cannot_multiply_offline_input_polls() {
    for mode in [SessionMode::Campaign, SessionMode::Skirmish] {
        for game_speed in 1..=6 {
            let mut pacer = LocalFramePacer::new();
            let mut frames = 0;
            let mut input_polls = 0;
            // Poll far faster than every supported paced frame. The production
            // pacer decides admission; the assertion concerns extra input work.
            for now_ms in 0..=192 {
                let admitted = pacer.should_admit(now_ms, game_speed, None, false);
                let decision = decide_runtime_pass(ordinary_pass(mode, admitted));
                if decision.run_sim {
                    frames += 1;
                    pacer.record_admitted_frame(now_ms);
                }
                input_polls += usize::from(decision.scroll_input);
            }
            assert_eq!(input_polls, frames, "{mode:?}, GameSpeed={game_speed}");
        }
    }
}

#[test]
fn scroll_poll_obeys_lifecycle_without_freezing_presentation() {
    let normal = ordinary_pass(SessionMode::Skirmish, false);
    let redraw = decide_runtime_pass(normal);
    assert!(!redraw.scroll_input);
    assert!(redraw.camera_scroll, "scroll steps need no game frame");
    assert!(
        redraw.frames_running,
        "the wait between frames still holds a view"
    );
    assert!(
        redraw.tactical_mutation,
        "placement and zoom remain per redraw"
    );
    // With no frame coming, nothing rewrites a view Follow or a pan holds.
    for stopped in [
        RuntimePassInputs {
            paused: true,
            ..normal
        },
        RuntimePassInputs {
            network_wait: true,
            ..normal
        },
    ] {
        assert!(!decide_runtime_pass(stopped).frames_running, "{stopped:?}");
    }

    for blocked in [
        RuntimePassInputs {
            window_active: false,
            pacer_timing_admits: true,
            ..normal
        },
        RuntimePassInputs {
            startup_admitted: false,
            pacer_timing_admits: true,
            ..normal
        },
        RuntimePassInputs {
            paused: true,
            menu_open: true,
            pacer_timing_admits: true,
            ..normal
        },
    ] {
        let decision = decide_runtime_pass(blocked);
        assert!(
            !decision.scroll_input && !decision.camera_scroll,
            "{blocked:?}"
        );
    }
    let developer_pause = decide_runtime_pass(RuntimePassInputs {
        paused: true,
        ..normal
    });
    assert!(!developer_pause.run_sim && developer_pause.scroll_input);
    for explicit_step in [
        RuntimePassInputs {
            exact_step: true,
            ..normal
        },
        RuntimePassInputs {
            frame_stepping: true,
            ..normal
        },
    ] {
        let decision = decide_runtime_pass(explicit_step);
        assert!(decision.run_sim && decision.scroll_input);
        assert!(!decision.admitted_by_pacer);
    }
    // The network modes keep their extra-poll policy.
    for mode in [SessionMode::Lan, SessionMode::Wol] {
        assert!(decide_runtime_pass(ordinary_pass(mode, false)).scroll_input);
    }
}
