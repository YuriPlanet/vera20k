//! Executed405190 pool and4053E0/405430 pause controls. Native clocks and
//! listener inputs are declared in the corpus; these tests compare the
//! existing event owner without supplying a second allocation or timer model.

use super::*;
use serde_json::Value;
use std::sync::OnceLock;

pub(crate) fn native() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/building_damage_sound_joined.json",
        ))
        .unwrap()
    })
}

pub(crate) fn control(label: &str) -> &'static Value {
    native()["audio_core_controls"]["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["label"] == label)
        .unwrap_or_else(|| panic!("missing executed audio control {label}"))
}

fn request() -> Option<PlayRequest> {
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()?;
    let index = native()["audio_core_controls"]["input"]["sound_index"]
        .as_u64()
        .unwrap();
    let name = native()["selected_sounds"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["fixture_index"] == index)
        .unwrap()["name"]
        .as_str()
        .unwrap();
    let entry = definitions.sounds().get(name).unwrap();
    let facts = EntryFacts::from(entry);
    Some(PlayRequest {
        key: name.to_uppercase(),
        volume_linear: facts.entry_volume_linear,
        facts,
        pan: crate::audio::sfx::SpatialGain::CENTRED_FULL.pan,
    })
}

#[test]
fn full_pool_retries_only_after_native_dead_flag_for_both_voc_callers() {
    let Some(request) = request() else { return };
    let mut arbiter = SoundArbiter::new(0);
    let capacity = control("native300event_pool_full")["returns"]
        .as_array()
        .unwrap()
        .len();
    let events: Vec<_> = (0..capacity)
        .map(|_| arbiter.submit(&request, 1000).unwrap())
        .collect();
    for label in ["full_pool_position_bind", "full_pool_nonpositional_bind"] {
        assert_eq!(control(label)["returned_event"], 0);
        assert!(arbiter.submit(&request, 1000).is_none(), "{label}");
        assert_eq!(
            arbiter.live_event_count(),
            control(label)["state"]["events"].as_array().unwrap().len()
        );
    }
    for (event, label) in events
        .into_iter()
        .zip(["dead_only_position_retry", "dead_only_nonpositional_retry"])
    {
        arbiter.stop(event);
        assert_ne!(control(label)["returned_event"], 0);
        assert!(arbiter.submit(&request, 1000).is_some(), "{label}");
        assert_eq!(
            arbiter.live_event_count(),
            control(label)["state"]["events"].as_array().unwrap().len()
        );
    }
}

#[test]
fn natural_state_four_still_occupies_full_pool_until_stopped() {
    let Some(request) = request() else { return };
    let mut arbiter = SoundArbiter::new(0);
    let ended = arbiter.submit(&request, 1000).unwrap();
    let token = arbiter.event_token(ended).unwrap();
    // Rust's existing endpoint owner is invoked explicitly. Native's state4
    // golden comes from its original PCM worker, not a supplied endpoint.
    arbiter.notify_playout_ended(token, PlayoutEnd::default(), 1000);
    let state = &control("state4_plus299_pool_full")["state"];
    let native_ended = state["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["state"] == 4)
        .unwrap();
    assert_eq!(
        arbiter.native_event_snapshot(ended).unwrap()["state"],
        native_ended["state"]
    );
    let count = state["events"].as_array().unwrap().len();
    for _ in 1..count {
        assert!(arbiter.submit(&request, 1000).is_some());
    }
    for label in [
        "state4_is_not_DEAD_position_fails",
        "state4_is_not_DEAD_nonpositional_fails",
    ] {
        assert_eq!(control(label)["returned_event"], 0);
        assert!(arbiter.submit(&request, 1000).is_none(), "{label}");
    }
    arbiter.stop(ended);
    assert_ne!(
        control("state4_now_DEAD_position_retry")["returned_event"],
        0
    );
    assert!(arbiter.submit(&request, 1000).is_some());
    assert_eq!(arbiter.live_event_count(), count);
}

#[test]
fn shared_event_pause_depths_match_executed_nested_game_pause() {
    let Some(request) = request() else { return };
    let mut arbiter = SoundArbiter::new(0);
    let existing = arbiter.submit(&request, 1000).unwrap();
    for label in ["after_game_pause_first", "after_game_pause_second"] {
        arbiter.suspend_all(1000);
        arbiter.suspend_event(existing, 1000);
        assert_eq!(
            i64::from(arbiter.event(existing).unwrap().suspend_depth),
            control(label)["state"]["events"][0]["suspend_depth"]
                .as_i64()
                .unwrap()
        );
    }
    let newborn = arbiter.submit(&request, 1000).unwrap();
    assert_eq!(
        i64::from(arbiter.event(newborn).unwrap().suspend_depth),
        control("new_request_during_nested_pause_depth_zero")["state"]["events"][1]
            ["suspend_depth"].as_i64().unwrap()
    );
    for label in [
        "after_game_resume_first",
        "after_game_resume_second",
        "after_game_resume_extra",
    ] {
        arbiter.resume_event(existing, 1068);
        arbiter.resume_all(1068);
        assert_eq!(
            i64::from(arbiter.event(existing).unwrap().suspend_depth),
            control(label)["state"]["events"][0]["suspend_depth"]
                .as_i64()
                .unwrap()
        );
        assert_eq!(arbiter.event(newborn).unwrap().suspend_depth, 0);
    }
}
