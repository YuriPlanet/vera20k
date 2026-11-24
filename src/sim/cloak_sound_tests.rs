//! StartUncloaking sound-argument and transition acceptance.

use super::*;

#[test]
fn start_cloaking_arg_zero_sounds_only_on_accepted_transition() {
    let mut audible = CloakRuntime::new(0);
    assert_eq!(
        audible.start_cloaking(12, 5, false),
        StartCloakingResult {
            transitioned: true,
            play_sound: true,
        }
    );
    assert_eq!(
        audible.start_cloaking(13, 5, false),
        StartCloakingResult {
            transitioned: false,
            play_sound: false,
        },
        "a repeated state-one visit neither transitions nor replays the cue"
    );
    audible.state = 2;
    assert_eq!(
        audible.start_cloaking(14, 5, false),
        StartCloakingResult {
            transitioned: false,
            play_sound: false,
        },
        "fully cloaked state two rejects StartCloaking"
    );

    let mut silent_reversal = CloakRuntime::new(0);
    silent_reversal.state = 3;
    assert_eq!(
        silent_reversal.start_cloaking(12, 1, true),
        StartCloakingResult {
            transitioned: true,
            play_sound: false,
        },
        "native state-three reversal passes arg one and preserves state writes"
    );
}

#[test]
fn start_uncloaking_arg_zero_sounds_only_on_accepted_transition() {
    let mut audible = CloakRuntime::new(0);
    audible.establish_unlimbo_fully_cloaked();
    let started = audible.start_uncloaking(12, 1, 9, false);
    assert_eq!(
        started,
        StartUncloakingResult {
            transitioned: true,
            play_sound: true,
        }
    );
    assert_eq!(
        audible.start_uncloaking(13, 1, 9, false),
        StartUncloakingResult {
            transitioned: false,
            play_sound: false,
        },
        "a repeated state-3 visit neither transitions nor replays the cue"
    );

    let mut silent = CloakRuntime::new(0);
    silent.state = 1;
    assert_eq!(
        silent.start_uncloaking(12, 1, 9, true),
        StartUncloakingResult {
            transitioned: true,
            play_sound: false,
        },
        "native arg one preserves state writes while suppressing CloakSound"
    );
}

#[test]
fn cloak_tick_reports_arg_zero_sound_for_entering_and_leaving_cloak() {
    let facts = |health_above_red, should_uncloak| CloakTickFacts {
        current_frame: 0,
        state_zero_head_allows: true,
        can_auto_cloak: true,
        should_uncloak,
        health_above_red,
        cloaking_speed: 1,
        cloak_delay_frames: 18,
        cloaking_stages: 9,
        invisible: false,
        owned_by_current_house: false,
        is_building: false,
    };

    let seed = (0..100_000)
        .find(|seed| {
            let mut rng = SimRng::new(*seed);
            rng.next_range_u32_inclusive(0, 99) <= 9
        })
        .expect("bounded seed search finds the native ten-percent branch");
    let mut rng = SimRng::new(seed);
    let mut abort = CloakRuntime::new(0);
    abort.state = 1;
    abort.depth = 3;
    abort.step_delta = 1;
    abort.step_timer = CloakStepTimer::started(0, 1);
    let result = abort.tick(facts(false, false), &mut rng);
    assert!(result.transitioned);
    assert!(
        !result.play_cloak_sound,
        "mid-cloak abort calls StartUncloaking(1)"
    );

    let mut entering = CloakRuntime::new(0);
    let result = entering.tick(facts(true, false), &mut rng);
    assert!(result.transitioned && result.play_cloak_sound);

    let mut reversal = CloakRuntime::new(0);
    reversal.state = 3;
    reversal.depth = 1;
    let result = reversal.tick(facts(true, false), &mut rng);
    assert!(result.transitioned);
    assert!(
        !result.play_cloak_sound,
        "state-three reversal calls StartCloaking(1)"
    );

    let mut ordinary = CloakRuntime::new(0);
    ordinary.establish_unlimbo_fully_cloaked();
    let result = ordinary.tick(facts(true, true), &mut rng);
    assert!(result.transitioned && result.play_cloak_sound);
}
