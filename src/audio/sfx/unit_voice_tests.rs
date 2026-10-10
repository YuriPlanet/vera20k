//! Original two-E1 G -> reached Techno heads -> shared audio service.
//!
//! The native fixture completes its pre-delivery Logic loop and supplies
//! explicit OS service times. These tests adopt that service boundary; they
//! do not assert whole-frame event delivery or asynchronous device scheduling.

use super::*;
use crate::sim::rng::{SimRng, trace_draws};
use serde_json::Value;
use std::sync::OnceLock;

pub(super) fn native() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/unit_voice_playback.json",
        ))
        .unwrap()
    })
}

#[test]
fn service_clock_matches_original_zero_origin_and_full_64bit_boundaries() {
    let history = &native()["clock_controls"][0];
    assert_eq!(history["name"], "empty_service_64bit_clock");
    let mut clock = arbiter::AudioServiceClock::default();
    let mut admitted_at = history["initial"]["pump_last_ms"].as_u64().unwrap();
    assert_eq!(admitted_at, 0);
    let inputs: Vec<_> = history["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|input| input["kind"] == "os_counter")
        .collect();
    let boundaries: Vec<_> = history["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|boundary| boundary["entry"] == 0x406f70_u64)
        .collect();
    assert_eq!(inputs.len(), boundaries.len());
    assert_eq!(inputs.len(), 11);
    for (input, boundary) in inputs.into_iter().zip(boundaries) {
        let now = input["milliseconds"].as_u64().unwrap();
        let before = boundary["before"]["pump_last_ms"].as_u64().unwrap();
        let after = boundary["after"]["pump_last_ms"].as_u64().unwrap();
        assert_eq!(admitted_at, before);
        assert_eq!(clock.admit(now), after != before, "native clock at {now}");
        admitted_at = after;
        assert!(boundary["requests"].as_array().unwrap().is_empty());
        assert!(boundary["advances"].as_array().unwrap().is_empty());
    }
}

fn core() -> &'static Value {
    native()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"] == "two_live_E1_G_logic_pool")
        .unwrap()
}

fn boundary(label: &str) -> &'static Value {
    core()["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["label"] == label)
        .unwrap()
}

fn main_hex(snapshot: &Value) -> &'static str {
    let key = snapshot["rng"]["main"].as_str().unwrap();
    core()["complete_rng_states"][key]["bytes"]
        .as_str()
        .unwrap()
}

fn actor_id(pointer: u64) -> u64 {
    core()["initial"]["actors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|actor| actor["pointer"] == pointer)
        .unwrap()["id"]
        .as_u64()
        .unwrap()
}

#[test]
fn two_reached_gi_voices_share_admission_and_original_main_sample_continuation() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    player.set_volume(0.0);
    let definitions = crate::rules::audio_sources::AudioDefinitions::select(&assets);
    let registry = definitions.sounds();
    let selected_index = assets.load_audio_index().unwrap();
    let index = selected_index.as_ref().map(|selected| &selected.index);
    for sound in native()["retail"]["sounds"].as_array().unwrap() {
        let entry = registry.get(sound["name"].as_str().unwrap()).unwrap();
        let facts = &sound["fields"];
        assert_eq!(
            i64::from(entry.volume_linear),
            facts["volume_fixed16"].as_i64().unwrap() >> 16
        );
        assert_eq!(i64::from(entry.limit), facts["limit"].as_i64().unwrap());
        assert_eq!(
            i64::from(entry.priority),
            facts["priority"].as_i64().unwrap()
        );
        assert_eq!(u64::from(entry.control), facts["control"].as_u64().unwrap());
        assert_eq!(
            entry.sounds.len() as u64,
            facts["sample_count"].as_u64().unwrap()
        );
    }

    let service = boundary("audio_pump_1034");
    let mut main = SimRng::from_native_state_hex_for_test(main_hex(&service["before"]));
    let initial_main = main.native_state_hex();
    let mut clock = arbiter::AudioServiceClock::default();
    assert!(clock.admit(1000));
    player.service_events(1000, registry, &assets, index, &mut |lo, hi| {
        main.next_range_i32_inclusive(lo, hi)
    });
    player
        .observe_pcm(PcmObservationConfig {
            sound_ids: vec!["GIMove".into(), "CommandBar".into()],
            max_events: 3,
            max_samples_per_event: 64,
        })
        .unwrap();
    assert!(player.play_registered_sound_spatial(
        "CommandBar",
        SpatialGain::CENTRED_FULL,
        registry
    ));

    let selection = core()["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"] == "selection_array")
        .unwrap();
    let selected: Vec<_> = selection["actors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|pointer| actor_id(pointer.as_u64().unwrap()))
        .collect();
    for owner in &selected {
        player.queue_unit_voice(*owner, "GIMove");
    }
    let visits: Vec<_> = core()["ordered_calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| {
            call["phase"] == "original_pre_delivery_Logic_object_loop"
                && call["kind"] == "native_site"
                && call["pc"] == 0x6f9ebb_u64
        })
        .map(|call| actor_id(call["this"].as_u64().unwrap()))
        .collect();
    assert_ne!(
        selected, visits,
        "the original selection and reached orders differ"
    );
    assert_eq!(visits.len(), 2);
    for owner in &visits {
        player.visit_unit_voice(*owner, registry);
        assert_eq!(player.voice_queue.pending_for(*owner), None);
        assert_eq!(player.voice_queue.playing_for(*owner), Some("GIMOVE"));
    }
    assert_eq!(
        player.arbiter.live_event_count(),
        service["before"]["live_count"].as_u64().unwrap() as usize
    );
    assert_eq!(player.pending.len(), 3);
    assert_eq!(player.arbiter.busy_channel_count(), 0);
    assert_eq!(
        main.native_state_hex(),
        initial_main,
        "requests cannot choose samples"
    );
    assert!(!clock.admit(1033));
    assert_eq!(
        main.native_state_hex(),
        main_hex(&boundary("audio_pump_1033")["after"])
    );
    assert!(clock.admit(1034));

    let mut ranges = Vec::new();
    let (_, raw) = trace_draws(|| {
        player.service_events(1034, registry, &assets, index, &mut |lo, hi| {
            let value = main.next_range_i32_inclusive(lo, hi);
            if lo != hi {
                ranges.push((lo, hi, value));
            }
            value
        })
    });
    let expected_ranges: Vec<_> = service["requests"]
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
    assert_eq!(ranges, expected_ranges);
    assert_eq!(
        raw.iter()
            .map(|draw| draw["value"].clone())
            .collect::<Vec<_>>(),
        service["advances"]
            .as_array()
            .unwrap()
            .iter()
            .map(|draw| draw["raw"].clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        main.native_state_hex(),
        main_hex(&service["after"]),
        "all 250 words and both cursors"
    );
    let native_events = service["after"]["events"].as_array().unwrap();
    assert!(native_events.iter().all(|event| event["state"] == 3));
    assert_eq!(player.arbiter.busy_channel_count(), native_events.len());
    assert_eq!(player.live.len(), native_events.len());
    assert!(player.pending.is_empty());
    let report = player.finish_pcm_observation().unwrap();
    assert_eq!(report.outputs.len(), native_events.len());
    for (actual, expected) in report.outputs.iter().zip(native_events) {
        assert!(
            actual
                .sound_id
                .eq_ignore_ascii_case(expected["sound_name"].as_str().unwrap())
        );
        let samples: Vec<_> = expected["loaded_sample_identities"]
            .as_array()
            .unwrap()
            .iter()
            .map(|sample| sample["name"].as_str().unwrap().to_ascii_lowercase())
            .collect();
        assert_eq!(
            actual
                .resolved_samples
                .iter()
                .map(|s| s.to_ascii_lowercase())
                .collect::<Vec<_>>(),
            samples
        );
        let gain = player.live[&EventId(actual.event)].gain;
        assert_eq!(
            i64::from(gain.base_linear),
            expected["volume_fixed16"].as_i64().unwrap() >> 16
        );
        assert_eq!(gain.buffer_linear, VOLUME_SCALE);
    }
    assert_eq!(
        report
            .outputs
            .iter()
            .filter(|output| output.owner_role == Some("unit_voice"))
            .map(|output| output.owner.unwrap())
            .collect::<Vec<_>>(),
        visits
    );
    player.stop_all();
}

#[test]
fn both_selected_gi_samples_decode_to_every_original_callback_pcm_byte() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let selected_index = assets.load_audio_index().unwrap();
    let index = selected_index.as_ref().map(|selected| &selected.index);
    let callbacks = core()["pcm_observation"]["callbacks"].as_array().unwrap();
    let mut checked = 0;
    for event in boundary("audio_pump_1034")["after"]["events"]
        .as_array()
        .unwrap()
    {
        if event["sound_name"] != "GIMove" {
            continue;
        }
        let identity = &event["loaded_sample_identities"][0];
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
        let actual: Vec<u8> = decoded
            .samples
            .chunks_exact(2)
            .flat_map(|frame| {
                assert_eq!(frame[0].to_bits(), frame[1].to_bits(), "native mono upmix");
                ((frame[0] * 32768.0) as i16).to_le_bytes()
            })
            .collect();
        let expected: Vec<u8> = callbacks
            .iter()
            .filter(|callback| {
                callback["backend"] == event["backend"]["pointer"]
                    && callback["phase"]
                        .as_str()
                        .unwrap()
                        .ends_with("audio_pump_1034")
            })
            .flat_map(|callback| {
                let hex = callback["output_bytes_hex"].as_str().unwrap();
                (0..hex.len())
                    .step_by(2)
                    .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
                    .collect::<Vec<_>>()
            })
            .collect();
        assert!(!expected.is_empty(), "native decoder witness for {name}");
        assert_eq!(
            actual, expected,
            "all original pre-gain mono PCM bytes for {name}"
        );
        checked += 1;
    }
    assert_eq!(checked, 2);
}

#[test]
fn shared_pool_admission_and_channel_shifts_match_original_main_draws() {
    use crate::rules::ini_parser::IniFile;
    use std::fmt::Write;

    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let selected_index = assets.load_audio_index().unwrap();
    let index = selected_index.as_ref().map(|selected| &selected.index);
    for history in native()["pool_controls"].as_array().unwrap() {
        let Some(mut player) = SfxPlayer::new() else {
            return;
        };
        player.set_volume(0.0);
        // Both engines parse the same declared authored rows alongside the
        // physical selected entries. No test supplies resolved entry facts.
        let mut sections = native()["retail"]["sound_sections"].clone();
        let authored = history["authored_sections"].as_object().unwrap();
        for (ordinal, (name, section)) in authored.iter().enumerate() {
            sections[name] = section.clone();
            sections["SoundList"][format!("probe{ordinal}")] = name.clone().into();
        }
        let mut text = String::new();
        for (name, section) in sections.as_object().unwrap() {
            writeln!(text, "[{name}]").unwrap();
            for (key, value) in section.as_object().unwrap() {
                writeln!(text, "{key}={}", value.as_str().unwrap()).unwrap();
            }
        }
        let registry = SoundRegistry::from_ini(&IniFile::from_str(&text));
        let mut identities = BTreeMap::new();
        for sound in native()["retail"]["sounds"].as_array().unwrap() {
            identities.insert(
                sound["index"].as_u64().unwrap(),
                sound["name"].as_str().unwrap(),
            );
        }
        for (name, id) in history["authored_ids"].as_object().unwrap() {
            identities.insert(id.as_u64().unwrap(), name.as_str());
        }
        let main_hex = |snapshot: &Value| {
            history["complete_rng_states"][snapshot["rng"]["main"].as_str().unwrap()]["bytes"]
                .as_str()
                .unwrap()
        };
        let mut main = SimRng::from_native_state_hex_for_test(main_hex(&history["initial"]));
        let mut clock = arbiter::AudioServiceClock::default();
        let mut submitted = Vec::new();
        for boundary in history["boundaries"].as_array().unwrap() {
            let entry = boundary["entry"].as_u64().unwrap();
            if entry == 0x750920 {
                let name = identities[&boundary["this"].as_u64().unwrap()];
                let sound = registry.get(name).unwrap();
                let event = player
                    .submit_request(
                        name,
                        EntryFacts::from(sound),
                        sound.volume_linear,
                        boundary["args"][1].as_i64().unwrap() as i32,
                        SfxChannel::Sound,
                    )
                    .unwrap();
                submitted.push((event, name));
                assert_eq!(main.native_state_hex(), main_hex(&boundary["after"]));
            } else if entry == 0x406f70 {
                let label = boundary["label"].as_str().unwrap();
                let now: u64 = label.rsplit('_').next().unwrap().parse().unwrap();
                assert_eq!(
                    main.native_state_hex(),
                    main_hex(&boundary["before"]),
                    "{label}"
                );
                let mut ranges = Vec::new();
                let (_, raw) = trace_draws(|| {
                    if clock.admit(now) {
                        player.service_events(now, &registry, &assets, index, &mut |lo, hi| {
                            let value = main.next_range_i32_inclusive(lo, hi);
                            if lo != hi {
                                ranges.push((lo, hi, value));
                            }
                            value
                        });
                    }
                });
                // Native controls run no device-worker iteration here. Keep
                // rodio output alive while testing that explicit boundary.
                for output in player.live.values() {
                    output.player.pause();
                }
                let expected_ranges: Vec<_> = boundary["requests"]
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
                assert_eq!(ranges, expected_ranges, "{label}");
                assert_eq!(
                    raw.iter()
                        .map(|draw| draw["value"].clone())
                        .collect::<Vec<_>>(),
                    boundary["advances"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|draw| draw["raw"].clone())
                        .collect::<Vec<_>>(),
                    "{label}"
                );
                assert_eq!(
                    main.native_state_hex(),
                    main_hex(&boundary["after"]),
                    "{label}"
                );
                let events: Vec<_> = boundary["after"]["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|event| event["state"] == 3)
                    .collect();
                assert_eq!(
                    player.live.len(),
                    events.len(),
                    "{} {label}",
                    history["name"]
                );
                assert_eq!(player.arbiter.busy_channel_count(), events.len(), "{label}");
                let mut actual_names = Vec::new();
                for (id, output) in &player.live {
                    let name = submitted
                        .iter()
                        .rev()
                        .find(|(event, _)| event == id)
                        .unwrap()
                        .1;
                    actual_names.push(name);
                    let expected = events
                        .iter()
                        .find(|event| event["sound_name"] == name)
                        .unwrap();
                    assert_eq!(
                        i64::from(output.gain.base_linear),
                        expected["volume_fixed16"].as_i64().unwrap() >> 16
                    );
                    assert_eq!(
                        u64::try_from(output.gain.buffer_linear).unwrap(),
                        expected["channel_parameters"]["volume_words"][1]
                            .as_u64()
                            .unwrap()
                            >> 16,
                        "channel-local shift for {name}"
                    );
                }
                let mut expected_names: Vec<_> = events
                    .iter()
                    .map(|event| event["sound_name"].as_str().unwrap())
                    .collect();
                actual_names.sort_unstable();
                expected_names.sort_unstable();
                assert_eq!(actual_names, expected_names, "{label}");
            }
        }
        player.stop_all();
    }
}
