//! Coordinate750E20 -> Sound4041D0 -> request750EC0 comparisons against the
//! original gamemd worker. Listener/OS inputs are declared consumer inputs;
//! these tests do not certify the native camera producer or a real device's
//! natural endpoint. Rust sources are paused after admission and endpoint405A00
//! is invoked explicitly. Production captures separately cover actual pulls.

use super::coordinate_sound_tests::{coordinate, cue, gain, request_flags, source, with_player};
use super::unit_voice_tests::{recorded_draws, retained_draws};
use super::*;
use crate::audio::arbiter::coordinate_tests::{control, native};
use crate::sim::rng::{MainRng, SimRng, trace_draws};
use serde_json::Value;

fn history() -> &'static Value {
    &native()["audio_core_controls"]["history"]
}

fn boundary(label: &str) -> &'static Value {
    history()["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["label"] == label)
        .unwrap_or_else(|| panic!("missing executed coordinate boundary {label}"))
}

fn main_hex(state: &Value) -> &'static str {
    history()["complete_rng_states"][state["rng"]["main"].as_str().unwrap()]["bytes"]
        .as_str()
        .unwrap()
}

fn main_at(label: &str) -> MainRng {
    SimRng::from_native_state_hex_for_test(main_hex(&boundary(label)["before"])).into()
}

fn service(
    player: &mut SfxPlayer,
    registry: &SoundRegistry,
    assets: &AssetManager,
    index: Option<&crate::assets::audio_bag::AudioIndex>,
    main: &MainRng,
    clock: &mut arbiter::AudioServiceClock,
    milliseconds: u64,
) {
    let label = format!("audio_pump_{milliseconds}");
    let expected = boundary(&label);
    assert_eq!(
        main.native_state_hex(),
        main_hex(&expected["before"]),
        "{label}"
    );
    let admitted = clock.admit(milliseconds);
    assert_eq!(
        admitted,
        expected["before"]["pump_last_ms"] != expected["after"]["pump_last_ms"],
        "{label}: original strict service clock"
    );
    let (mut draws, ranges) = recorded_draws(main);
    let (_, raw) = trace_draws(|| {
        if admitted {
            player.service_events(
                milliseconds,
                registry,
                &crate::rules::sound_ini::EvaRegistry::default(),
                assets,
                index,
                &mut draws,
            );
        }
    });
    for output in player.live.values() {
        output.player.pause();
    }
    let expected_ranges: Vec<_> = expected["requests"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|request| {
            let args = request["args"].as_array().unwrap();
            (args.len() == 2 && args[0] != args[1]).then(|| {
                (
                    args[0].as_i64().unwrap() as i32,
                    args[1].as_i64().unwrap() as i32,
                    request["result"].as_i64().unwrap() as i32,
                )
            })
        })
        .collect();
    assert_eq!(
        *ranges.lock().unwrap(),
        expected_ranges,
        "{label}: ranges/order"
    );
    assert_eq!(
        raw.iter().map(|draw| &draw["value"]).collect::<Vec<_>>(),
        expected["advances"]
            .as_array()
            .unwrap()
            .iter()
            .map(|draw| &draw["raw"])
            .collect::<Vec<_>>(),
        "{label}: every raw Main word including rejections"
    );
    assert_eq!(
        main.native_state_hex(),
        main_hex(&expected["after"]),
        "{label}: full Main continuation"
    );
}

/// Compare identities within each engine. Native pointers/serials are never
/// supplied as Rust allocation answers, and reused physical slots are not owners.
fn assert_requests(player: &mut SfxPlayer, state: &Value, owners: &[(u64, u64)]) {
    let expected_requests = state["coordinate_requests"].as_array().unwrap();
    let actual_requests = player.coordinate_sound_observation();
    assert_eq!(actual_requests.len(), expected_requests.len());
    assert_eq!(
        player.arbiter.live_event_count(),
        state["live_count"].as_u64().unwrap() as usize,
        "physical event census includes retained DEAD records"
    );
    assert_eq!(
        player.arbiter.busy_channel_count(),
        state["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["channel"] != 0)
            .count(),
        "ordinary channels exclude the three process stream reservations"
    );
    for (owner, native_pointer) in owners {
        let expected = expected_requests
            .iter()
            .find(|row| row["pointer"] == *native_pointer);
        let actual = actual_requests
            .iter()
            .find(|row| row["request_id"] == *owner);
        assert_eq!(
            actual.is_some(),
            expected.is_some(),
            "coordinate owner {owner}"
        );
        let Some(expected) = expected else { continue };
        let actual = actual.unwrap();
        assert_eq!(actual["suspend_depth"], expected["suspend_depth"]);
        assert_eq!(actual["coordinate_leptons"], expected["location"]);
        assert_eq!(actual["flags"], expected["caller_argument"]);
        let expected_event = state["events"].as_array().unwrap().iter().find(|event| {
            event["pointer"] == expected["handle"][0]
                && event["serial"] == expected["handle"][1]
                && event["sound_pointer"] == expected["handle"][2]
        });
        let actual_event = player
            .arbiter
            .validate_loop_handle(HandleOwner::Coordinate(*owner));
        assert_eq!(
            actual_event.is_some(),
            expected_event.is_some(),
            "coordinate handle {owner}"
        );
        if let (Some(event), Some(expected)) = (actual_event, expected_event) {
            let snapshot = player.arbiter.native_event_snapshot(event).unwrap();
            assert_eq!(
                snapshot["state"], expected["state"],
                "coordinate state {owner}"
            );
            assert_eq!(
                snapshot["flags"], expected["flags"],
                "coordinate flags {owner}"
            );
            for (field, words, index) in [
                ("volume_current_fixed", "volume_controller_words", 1),
                ("volume_target_fixed", "volume_controller_words", 2),
                ("pan_current_fixed", "pan_controller_words", 1),
                ("pan_target_fixed", "pan_controller_words", 2),
            ] {
                assert_eq!(
                    snapshot[field], expected[words][index],
                    "coordinate owner {owner} exact native {field}"
                );
            }
            if let Some(output) = player.live.get(&event) {
                assert_eq!(
                    i64::from(output.gain.base_linear),
                    expected["volume_controller_words"][1].as_i64().unwrap() >> 16
                );
            }
        }
    }
}

fn admit_request(player: &mut SfxPlayer, registry: &SoundRegistry, label: &str) -> (u64, u64) {
    let sound = cue("sound_index");
    let expected = boundary(label);
    assert!(player.play_sound_at_coord(
        sound,
        coordinate(),
        request_flags(),
        source(),
        gain(label, sound, registry),
        registry,
    ));
    let actual_owner = player.coordinate_sound_observation().last().unwrap()["request_id"]
        .as_u64()
        .unwrap();
    let old = expected["before"]["coordinate_requests"]
        .as_array()
        .unwrap();
    let newborn: Vec<_> = expected["after"]["coordinate_requests"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|request| {
            !old.iter()
                .any(|prior| prior["pointer"] == request["pointer"])
        })
        .collect();
    assert_eq!(newborn.len(), 1, "{label}: actual original node allocation");
    (actual_owner, newborn[0]["pointer"].as_u64().unwrap())
}

#[test]
fn coordinate_real_sample_service_gate_main_and_explicit_endpoint_cleanup_match_original() {
    with_player(|player, registry, assets, index| {
        let main = main_at("coordinate_natural_end");
        let mut clock = arbiter::AudioServiceClock::default();
        let start = boundary("coordinate_natural_end")["before"]["pump_last_ms"]
            .as_u64()
            .unwrap();
        assert!(clock.admit(start));
        player.service_events(
            start,
            registry,
            &crate::rules::sound_ini::EvaRegistry::default(),
            assets,
            index,
            &mut retained_draws(&main),
        );
        player
            .observe_pcm(PcmObservationConfig {
                sound_ids: vec![cue("sound_index").to_owned()],
                max_events: 1,
                max_samples_per_event: 64,
            })
            .unwrap();
        let owner = admit_request(player, registry, "coordinate_natural_end");
        assert_requests(
            player,
            &control("coordinate_natural_end")["state"],
            &[owner],
        );
        assert_eq!(
            main.native_state_hex(),
            main_hex(&boundary("coordinate_natural_end")["after"])
        );
        service(player, registry, assets, index, &main, &mut clock, 1135);
        assert!(player.live.is_empty());
        assert_eq!(player.pending.len(), 1);
        service(player, registry, assets, index, &main, &mut clock, 1136);
        assert_requests(player, &boundary("audio_pump_1136")["after"], &[owner]);
        let event = player
            .arbiter
            .validate_loop_handle(HandleOwner::Coordinate(owner.0))
            .unwrap();
        let token = player.arbiter.event_token(event).unwrap();
        assert!(player.pending.is_empty());
        assert_eq!(player.live.len(), 1);
        // The original worker naturally wrote state4. Only Rust's existing
        // endpoint is supplied here; this is not a Rust device-completion test.
        let ended = &native()["audio_core_controls"]["natural_end"]["ended_before_main"];
        player
            .arbiter
            .notify_playout_ended(token, PlayoutEnd::default(), 0);
        player.release_output(event);
        assert_requests(player, ended, &[owner]);
        assert_eq!(main.native_state_hex(), main_hex(ended));
        let reap_ms = control("after_natural_end_and_reap")["state"]["pump_last_ms"]
            .as_u64()
            .unwrap();
        service(player, registry, assets, index, &main, &mut clock, reap_ms);
        assert_requests(
            player,
            &control("after_natural_end_and_reap")["state"],
            &[owner],
        );
        assert!(player.arbiter.is_token_retired(token));
        assert!(player.pending.is_empty());
        assert!(player.live.is_empty());
        let report = player.finish_pcm_observation().unwrap();
        assert_eq!(report.coordinate_request_count_at_finish, Some(0));
        assert_eq!(report.outputs.len(), 1);
        let output = &report.outputs[0];
        assert_eq!(output.owner_role, Some("coordinate_request"));
        assert_eq!(output.owner, Some(owner.0));
        let expected_samples: Vec<_> = ended["events"][0]["loaded_sample_identities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|sample| sample["name"].as_str().unwrap().to_ascii_lowercase())
            .collect();
        assert_eq!(
            output
                .resolved_samples
                .iter()
                .map(|name| name.to_ascii_lowercase())
                .collect::<Vec<_>>(),
            expected_samples
        );
        assert_eq!(output.event_live_at_finish, Some(false));
        assert_eq!(output.channel_live_at_finish, Some(false));
        assert_eq!(output.pending_payload_at_finish, Some(false));
        assert_eq!(output.live_payload_at_finish, Some(false));
    });
}

#[test]
fn coordinate_selected_retail_clip_matches_every_original_worker_pcm_byte() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let selected_index = crate::rules::retail_ini_fixture::retail_audio_index();
    let index = selected_index.map(|selected| &selected.index);
    let natural = &native()["audio_core_controls"]["natural_end"];
    let event = &natural["ended_before_main"]["events"][0];
    let identities = event["loaded_sample_identities"].as_array().unwrap();
    assert_eq!(
        identities.len(),
        1,
        "selected stock one-shot worker witness"
    );
    let identity = &identities[0];
    let name = identity["name"].as_str().unwrap();
    let (entry, _) = index.unwrap().get(name).unwrap();
    assert_eq!(
        u64::from(entry.offset),
        identity["source_offset"].as_u64().unwrap()
    );
    assert_eq!(
        u64::from(entry.size),
        identity["source_bytes"].as_u64().unwrap()
    );
    let decoded = load_sfx(name, &assets, index).unwrap();
    assert_eq!(decoded.channels, 2);
    assert!(decoded.samples.iter().all(|sample| sample.is_finite()));
    assert!(decoded.samples.iter().any(|sample| *sample != 0.0));
    let (frames, remainder) = decoded.samples.as_chunks::<2>();
    assert!(remainder.is_empty(), "complete original mono upmix");
    let actual: Vec<u8> = frames
        .iter()
        .flat_map(|pair| {
            assert_eq!(pair[0].to_bits(), pair[1].to_bits(), "original mono upmix");
            let scaled = pair[0] * 32768.0;
            let sample = scaled as i16;
            assert_eq!(scaled, f32::from(sample), "lossless original PCM");
            sample.to_le_bytes()
        })
        .collect();
    // The observer ledger continues into later controls, where native reuses
    // this physical backend. Select the original event's recorded lifetime,
    // not every callback that happens to share its backend pointer.
    let phases: Vec<_> = history()["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|boundary| {
            boundary["after"]["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|candidate| {
                    candidate["pointer"] == event["pointer"]
                        && candidate["serial"] == event["serial"]
                })
        })
        .map(|boundary| boundary["label"].as_str().unwrap())
        .collect();
    let expected: Vec<u8> = natural["pcm_observation"]["callbacks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|callback| {
            callback["backend"] == event["backend"]["pointer"]
                && phases
                    .iter()
                    .any(|phase| callback["phase"].as_str().unwrap().ends_with(*phase))
        })
        .flat_map(|callback| {
            let hex = callback["output_bytes_hex"].as_str().unwrap();
            assert_eq!(hex.len() % 2, 0);
            (0..hex.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        !expected.is_empty(),
        "actual original callback bytes for {name}"
    );
    assert_eq!(
        actual.len(),
        expected.len(),
        "complete original pre-gain mono PCM byte count for {name}"
    );
    if let Some((index, (actual, expected))) = actual
        .iter()
        .zip(&expected)
        .enumerate()
        .find(|(_, (actual, expected))| actual != expected)
    {
        panic!("original PCM {name} byte {index}: Rust {actual}, native {expected}");
    }
}

#[test]
fn coordinate_limit_two_and_current_listener_redrive_match_original_service_order() {
    with_player(|player, registry, assets, index| {
        let main = main_at("registered_Limit2_request_0");
        let mut clock = arbiter::AudioServiceClock::default();
        let start = boundary("registered_Limit2_request_0")["before"]["pump_last_ms"]
            .as_u64()
            .unwrap();
        assert!(clock.admit(start));
        player.service_events(
            start,
            registry,
            &crate::rules::sound_ini::EvaRegistry::default(),
            assets,
            index,
            &mut retained_draws(&main),
        );
        let owners: Vec<_> = (0..3)
            .map(|ordinal| {
                admit_request(
                    player,
                    registry,
                    &format!("registered_Limit2_request_{ordinal}"),
                )
            })
            .collect();
        assert_eq!(
            main.native_state_hex(),
            main_hex(&boundary("registered_Limit2_request_2")["after"])
        );
        service(player, registry, assets, index, &main, &mut clock, 3534);
        assert_requests(
            player,
            &control("registered_Limit2_service")["state"],
            &owners,
        );
        for milliseconds in [3568, 3602, 3636] {
            let label = format!("native_camera_redrive_{milliseconds}");
            let sound = cue("sound_index");
            player.refresh_coordinate_sound_gains(|name, _source| {
                assert!(name.eq_ignore_ascii_case(sound));
                gain(&label, sound, registry)
            });
            service(
                player,
                registry,
                assets,
                index,
                &main,
                &mut clock,
                milliseconds,
            );
            assert_requests(player, &control(&label)["state"], &owners);
        }
        let sound = cue("sound_index");
        player.refresh_coordinate_sound_gains(|_name, _source| {
            gain(
                "stopped_one_shot_stays_reaped_after_view_returns",
                sound,
                registry,
            )
        });
        service(player, registry, assets, index, &main, &mut clock, 3670);
        assert_requests(
            player,
            &control("stopped_one_shot_stays_reaped_after_view_returns")["state"],
            &owners,
        );
        assert!(player.pending.is_empty());
        assert!(player.live.is_empty());
    });
}
