//! Offline fast scrolling follows Main_Tick's input poll, not display refresh.
//! Original 0x55D8AB polls GScreen once; ThrottleFrame 0x55E160 skips its
//! additional input poll at 0x55E253 in campaign (0) and skirmish (5).

use super::{RuntimePassInputs, SessionMode, decide_runtime_pass};
use crate::app::match_runtime::frame_pacer::LocalFramePacer;

fn ordinary_pass(session_mode: SessionMode, pacer_timing_admits: bool) -> RuntimePassInputs {
    RuntimePassInputs {
        exact_step: false,
        window_active: true,
        focus_frozen: false,
        startup_admitted: true,
        frame_stepping: false,
        paused: false,
        menu_open: false,
        session_mode,
        pacer_timing_admits,
    }
}

#[test]
fn offline_fast_scroll_does_not_repeat_between_game_frames() {
    for mode in [SessionMode::Campaign, SessionMode::Skirmish] {
        let accepted = decide_runtime_pass(ordinary_pass(mode, true));
        assert!(accepted.run_sim && accepted.scroll_input);
        let redraw = decide_runtime_pass(ordinary_pass(mode, false));
        assert!(!redraw.run_sim);
        assert!(
            !redraw.scroll_input,
            "{mode:?}: a redraw in the native throttle wait must not repeat held fast scrolling"
        );
    }
}

#[test]
fn repeated_redraws_cannot_multiply_offline_scroll_steps() {
    for mode in [SessionMode::Campaign, SessionMode::Skirmish] {
        for game_speed in 1..=6 {
            let mut pacer = LocalFramePacer::new();
            let mut frames = 0;
            let mut camera_steps = 0;
            // Poll far faster than every supported paced frame. The production
            // pacer decides admission; the assertion concerns extra input work.
            for now_ms in 0..=192 {
                let admitted = pacer.should_admit(now_ms, game_speed, false);
                let decision = decide_runtime_pass(ordinary_pass(mode, admitted));
                if decision.run_sim {
                    frames += 1;
                    pacer.record_admitted_frame(now_ms);
                }
                camera_steps += usize::from(decision.scroll_input);
            }
            assert_eq!(camera_steps, frames, "{mode:?}, GameSpeed={game_speed}");
        }
    }
}

#[test]
fn scroll_poll_obeys_lifecycle_without_freezing_presentation() {
    let normal = ordinary_pass(SessionMode::Skirmish, false);
    let redraw = decide_runtime_pass(normal);
    assert!(!redraw.scroll_input);
    assert!(
        redraw.tactical_mutation,
        "placement and zoom remain per redraw"
    );

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
        assert!(!decide_runtime_pass(blocked).scroll_input, "{blocked:?}");
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
    // Networking is not live in VERA; preserve its existing extra-poll policy.
    for mode in [SessionMode::Lan, SessionMode::Wol] {
        assert!(decide_runtime_pass(ordinary_pass(mode, false)).scroll_input);
    }
}
