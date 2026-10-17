//! Original House508DF0 -> Radar656DF0/656BE0 -> NULL-handle750920 cues.
//!
//! The corpus supplies reached requests and explicit OS service/worker inputs.
//! Actual retail entries, decoding, pool admission and Main retain their owners.
//! Players are paused after admission; native natural endpoints are supplied to
//! the existing Rust endpoint API. This does not assert host device scheduling,
//! House provider admission or the app's production of the requests.

use super::coordinate_sound_tests::with_player;
use super::unit_voice_tests::retained_draws;
use super::*;
use crate::sim::rng::{MainRng, SimRng, trace_draws};
use serde_json::{Value, json};
use std::sync::{Mutex, OnceLock};

fn native() -> &'static Value {
    static CORPUS: OnceLock<Value> = OnceLock::new();
    CORPUS.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/radar_availability_audio.json",
        ))
        .unwrap()
    })
}

fn main_hex(snapshot: &Value) -> &'static str {
    native()["complete_rng_states"][snapshot["rng"]["main"].as_str().unwrap()]["bytes"]
        .as_str()
        .unwrap()
}

/// Native pointers/serials only identify an observed allocation. They never
/// supply Rust pool slots, serials or playback decisions.
struct Allocation {
    pointer: u64,
    serial: u64,
    token: EventToken,
    sound: String,
    pan: i32,
}

fn assert_pool(player: &SfxPlayer, expected: &Value, allocations: &[Allocation], label: &str) {
    let events = expected["events"].as_array().unwrap();
    assert_eq!(
        player.arbiter.live_event_count(),
        expected["live_count"].as_u64().unwrap() as usize,
        "{label}: physical event census"
    );
    assert_eq!(
        player.arbiter.busy_channel_count(),
        events.iter().filter(|event| event["channel"] != 0).count(),
        "{label}: ordinary bound channels"
    );
    assert_eq!(
        player.pending.len(),
        events.iter().filter(|event| event["state"] == 0).count(),
        "{label}: retained requests"
    );
    assert_eq!(
        player.live.len(),
        events.iter().filter(|event| event["state"] == 3).count(),
        "{label}: retained outputs"
    );
    for allocation in allocations {
        let expected = events.iter().find(|event| {
            event["pointer"] == allocation.pointer && event["serial"] == allocation.serial
        });
        let Some(expected) = expected else {
            assert!(
                player.arbiter.is_token_retired(allocation.token),
                "{label}: retired allocation {}",
                allocation.sound
            );
            continue;
        };
        assert_eq!(expected["sound_name"], allocation.sound);
        assert!(matches!(expected["state"].as_u64(), Some(0 | 3 | 4)));
        let actual = player
            .arbiter
            .native_event_snapshot(allocation.token.event)
            .unwrap();
        assert_eq!(actual["state"], expected["state"], "{label}: event state");
        assert_eq!(actual["flags"], expected["flags"], "{label}: event flags");
        assert_eq!(actual["serial"], allocation.token.serial);
        assert_eq!(
            actual["volume_current_fixed"], expected["volume_fixed16"],
            "{label}: entry gain"
        );
        assert_eq!(
            actual["pan_current_fixed"].as_i64().unwrap() >> 16,
            i64::from(allocation.pan),
            "{label}: centered request"
        );
        if let Some(output) = player.live.get(&allocation.token.event) {
            let shifts = output.parked.as_ref().unwrap().shifts.unwrap();
            assert_eq!(
                i64::from(shifts.frequency_pct),
                expected["pitch"].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(shifts.volume_shift_pct),
                expected["volume_shift"].as_i64().unwrap()
            );
            assert_eq!(
                i64::from(output.gain.buffer_linear),
                expected["channel_parameters"]["volume_words"][1]
                    .as_i64()
                    .unwrap()
                    >> 16,
                "{label}: admitted channel gain"
            );
        }
    }
}

#[test]
fn registered_radar_requests_service_main_overlap_and_retirement_match_original() {
    for history in native()["rows"].as_array().unwrap() {
        with_player(|player, registry, assets, index| {
            let name = history["name"].as_str().unwrap();
            for sound in native()["retail"]["sounds"].as_array().unwrap() {
                let entry = registry.get(sound["name"].as_str().unwrap()).unwrap();
                let facts = &sound["fields"];
                assert_eq!(
                    i64::from(entry.volume_linear),
                    facts["volume_fixed16"].as_i64().unwrap() >> 16
                );
                assert_eq!(u64::from(entry.control), facts["control"].as_u64().unwrap());
                assert_eq!(u64::from(entry.type_flags), facts["type"].as_u64().unwrap());
                assert_eq!(i64::from(entry.limit), facts["limit"].as_i64().unwrap());
                assert_eq!(
                    i64::from(entry.priority),
                    facts["priority"].as_i64().unwrap()
                );
                assert_eq!(
                    entry.sounds.len() as u64,
                    facts["sample_count"].as_u64().unwrap()
                );
            }
            let main = Arc::new(MainRng::from(SimRng::from_native_state_hex_for_test(
                main_hex(&history["initial"]),
            )));
            let mut clock = arbiter::AudioServiceClock::default();
            let start = history["initial"]["pump_last_ms"].as_u64().unwrap();
            assert!(clock.admit(start));
            player.service_events(start, registry, assets, index, &mut retained_draws(&main));
            assert_eq!(main.native_state_hex(), main_hex(&history["initial"]));
            player
                .observe_pcm(PcmObservationConfig {
                    sound_ids: native()["retail"]["sounds"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|sound| sound["name"].as_str().unwrap().to_owned())
                        .collect(),
                    max_events: 4,
                    max_samples_per_event: 64,
                })
                .unwrap();
            let mut allocations = Vec::new();
            for phase in history["phases"].as_array().unwrap() {
                let label = format!("{name}: {}", phase["label"]);
                assert_pool(player, &phase["before"], &allocations, &label);
                assert_eq!(main.native_state_hex(), main_hex(&phase["before"]));
                for call in phase["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|call| call["kind"] == "PlaySound")
                {
                    assert_eq!(call["pc"], 0x750920_u64);
                    assert_eq!(
                        call["handle"], 0,
                        "ordinary radar cues own no retained handle"
                    );
                    assert_eq!(call["pan"], PAN_CENTRE);
                    assert_eq!(call["volume_f32_bits"], 1.0_f32.to_bits());
                    let Some(sound) = call["sound"].as_str() else {
                        assert_eq!(call["sound_index"], -1);
                        assert_eq!(call["returned_eax"], 0);
                        continue; // The app drops an unbound Rules reference before queuing audio.
                    };
                    let before: Vec<_> = player
                        .pending
                        .values()
                        .map(|pending| pending.token)
                        .collect();
                    assert!(player.play_registered_sound_spatial(
                        sound,
                        SpatialGain::CENTRED_FULL,
                        registry
                    ));
                    let added: Vec<_> = player
                        .pending
                        .values()
                        .filter(|pending| !before.contains(&pending.token))
                        .collect();
                    assert_eq!(added.len(), 1);
                    let expected = phase["after"]["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|event| event["pointer"] == call["returned_eax"])
                        .unwrap();
                    allocations.push(Allocation {
                        pointer: expected["pointer"].as_u64().unwrap(),
                        serial: expected["serial"].as_u64().unwrap(),
                        token: added[0].token,
                        sound: sound.to_owned(),
                        pan: call["pan"].as_i64().unwrap() as i32,
                    });
                }
                assert!(phase["requests"].as_array().unwrap().is_empty());
                assert!(phase["raw_advances"].as_array().unwrap().is_empty());
                assert_eq!(
                    main.native_state_hex(),
                    main_hex(&phase["after"]),
                    "{label}: admission draws no words"
                );
                assert_pool(player, &phase["after"], &allocations, &label);
            }
            for boundary in history["playback"]["phases"].as_array().unwrap() {
                let label = boundary["label"].as_str().unwrap();
                assert_pool(player, &boundary["before"], &allocations, label);
                assert_eq!(main.native_state_hex(), main_hex(&boundary["before"]));
                // The shared recorded_draws helper omits equal bounds. Here
                // the executed stock corpus makes six (0,0) requests, so log
                // every call while lending the canonical Main capability.
                let capability = main.draws();
                let request_main = Arc::clone(&main);
                let requests = Arc::new(Mutex::new(Vec::new()));
                let observed = Arc::clone(&requests);
                let mut draws = PlaybackDraws::new(move |low, high| {
                    let before = request_main.native_state_hex();
                    let result = capability.ranged(low, high);
                    observed.lock().unwrap().push(json!({
                        "args": [low, high], "result": result,
                        "state_before": before, "state_after": request_main.native_state_hex(),
                    }));
                    result
                });
                let (_, raw) = trace_draws(|| {
                    if boundary["entry"] == 0x406f70_u64 {
                        let now = label.rsplit('_').next().unwrap().parse().unwrap();
                        let admitted = clock.admit(now);
                        assert_eq!(
                            admitted,
                            boundary["before"]["pump_last_ms"] != boundary["after"]["pump_last_ms"]
                        );
                        if admitted {
                            player.service_events(now, registry, assets, index, &mut draws);
                        }
                        for output in player.live.values() {
                            output.player.pause();
                        }
                    } else {
                        assert!(matches!(
                            boundary["entry"].as_u64(),
                            Some(0x4095b0 | 0x409844)
                        ));
                        // Native worker inputs, not elapsed host time, select
                        // the endpoint boundary. Use the existing405A00 owner.
                        for expected in boundary["after"]["events"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|event| event["state"] == 4)
                        {
                            let allocation = allocations
                                .iter()
                                .find(|allocation| {
                                    expected["pointer"] == allocation.pointer
                                        && expected["serial"] == allocation.serial
                                })
                                .unwrap();
                            if player.live.contains_key(&allocation.token.event) {
                                assert!(!player.arbiter.notify_playout_ended(
                                    allocation.token,
                                    PlayoutEnd::default(),
                                    0
                                ));
                                player.release_output(allocation.token.event);
                            }
                        }
                    }
                });
                let expected: Vec<_> = boundary["requests"].as_array().unwrap().iter().map(|request| {
                    assert_eq!(request["this"], 0x886b88_u64, "native process Main owner");
                    json!({"args": request["args"], "result": request["result"],
                        "state_before": request["state_before"], "state_after": request["state_after"]})
                }).collect();
                assert_eq!(
                    *requests.lock().unwrap(),
                    expected,
                    "{name}: {label}: all requests, including equal bounds"
                );
                assert_eq!(
                    raw.iter().map(|draw| &draw["value"]).collect::<Vec<_>>(),
                    boundary["advances"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|draw| &draw["raw"])
                        .collect::<Vec<_>>(),
                    "{name}: {label}: every raw Main word"
                );
                assert_eq!(
                    main.native_state_hex(),
                    main_hex(&boundary["after"]),
                    "{name}: {label}: all Main words/cursors"
                );
                assert_pool(player, &boundary["after"], &allocations, label);
            }
            assert_pool(player, &history["final"], &allocations, name);
            let report = player.finish_pcm_observation().unwrap();
            assert_eq!(report.outputs.len(), allocations.len());
            for allocation in &allocations {
                let output = report
                    .outputs
                    .iter()
                    .find(|output| {
                        output.event == allocation.token.event.0
                            && output.event_serial == allocation.token.serial
                    })
                    .unwrap();
                assert!(output.sound_id.eq_ignore_ascii_case(&allocation.sound));
                assert_eq!(output.owner, None);
                let expected = history["playback"]["pcm_by_backend"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| {
                        row["event"] == allocation.pointer && row["serial"] == allocation.serial
                    })
                    .unwrap();
                assert_eq!(
                    output
                        .resolved_samples
                        .iter()
                        .map(|name| name.to_ascii_lowercase())
                        .collect::<Vec<_>>(),
                    expected["samples"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|sample| sample["name"].as_str().unwrap().to_ascii_lowercase())
                        .collect::<Vec<_>>()
                );
                assert_eq!(output.event_live_at_finish, Some(false));
                assert_eq!(output.channel_live_at_finish, Some(false));
                assert_eq!(output.pending_payload_at_finish, Some(false));
                assert_eq!(output.live_payload_at_finish, Some(false));
            }
        });
    }
}

#[test]
fn radar_selected_retail_samples_match_all_original_callback_pcm_ranges() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let index = &crate::rules::retail_ini_fixture::retail_audio_index()
        .expect("selected retail audio index")
        .index;
    let mut checked = 0;
    for history in native()["rows"].as_array().unwrap() {
        for expected in history["playback"]["pcm_by_backend"].as_array().unwrap() {
            let samples = expected["samples"].as_array().unwrap();
            assert_eq!(samples.len(), 1, "selected original stock one-shot");
            let identity = &samples[0];
            let name = identity["name"].as_str().unwrap();
            let (entry, _) = index.get(name).unwrap();
            assert_eq!(
                u64::from(entry.offset),
                identity["source_offset"].as_u64().unwrap()
            );
            assert_eq!(
                u64::from(entry.size),
                identity["source_bytes"].as_u64().unwrap()
            );
            assert_eq!(entry.channels(), 1, "selected physical mono source");
            let decoded = load_sfx(name, &assets, Some(index)).unwrap();
            assert_eq!(decoded.channels, 2);
            let (frames, remainder) = decoded.samples.as_chunks::<2>();
            assert!(remainder.is_empty());
            let actual: Vec<u8> = frames
                .iter()
                .flat_map(|pair| {
                    assert_eq!(pair[0].to_bits(), pair[1].to_bits(), "lossless mono upmix");
                    let scaled = pair[0] * 32768.0;
                    let sample = scaled as i16;
                    assert_eq!(scaled, f32::from(sample));
                    sample.to_le_bytes()
                })
                .collect();
            assert_eq!(actual.len() as u64, expected["bytes"].as_u64().unwrap());
            assert_eq!(
                crate::util::sha256::sha256_hex(&actual),
                expected["sha256"].as_str().unwrap(),
                "every original decoded byte for {name}"
            );
            assert_eq!(
                frames.iter().filter(|frame| frame[0] != 0.0).count() as u64,
                expected["nonzero_i16_samples"].as_u64().unwrap()
            );
            // Include zero-input drain callbacks: the shared decoder already
            // owns its final stale-padded ADPCM block. No test decoder or
            // callback-sized padding/cropping substitutes for those bytes.
            let mut offset = 0;
            for callback in expected["callbacks"].as_array().unwrap() {
                let end = offset + callback["returned_output_bytes"].as_u64().unwrap() as usize;
                assert_eq!(
                    crate::util::sha256::sha256_hex(&actual[offset..end]),
                    callback["output_sha256"].as_str().unwrap(),
                    "{name}: original callback {}",
                    callback["phase"]
                );
                offset = end;
            }
            assert_eq!(offset, actual.len());
            checked += 1;
        }
    }
    assert!(checked > 0, "executed original PCM associations");
}
