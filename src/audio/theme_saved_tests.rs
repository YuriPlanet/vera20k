//! Executed Theme storage/volume controls. Stream calls are observed transports;
//! these tests do not claim native SAV compatibility or physical audio parity.

use super::*;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/storage_oracle/sound_theme_metadata.json",
    ))
    .unwrap()
}

fn slots(value: &Value) -> ThemeSlots {
    ThemeSlots {
        active: value[0].as_i64().unwrap() as i32,
        retained: value[1].as_i64().unwrap() as i32,
        pending: value[2].as_i64().unwrap() as i32,
    }
}

#[test]
fn score_volume_conversion_matches_native_stored_options_and_setter() {
    let fixture = corpus();
    let cases = fixture["score_volume_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 16);
    for case in cases {
        let mut theme = ThemeRuntime::default();
        assert_eq!(theme.score_volume, case["constructor_volume"]);
        let stored = f32::from_bits(case["stored_f32_bits"].as_u64().unwrap() as u32);
        theme.set_score_volume(stored);
        assert_eq!(theme.score_volume, case["theme_volume"], "{case}");
    }
    let setters = fixture["volume_setter_cases"].as_array().unwrap();
    assert_eq!(setters.len(), 9);
    for case in setters {
        let mut theme = ThemeRuntime::default();
        theme.set_volume(case["input"].as_i64().unwrap() as i32);
        assert_eq!(theme.score_volume, case["stored"], "{case}");
    }
}

#[test]
fn saved_record_contains_native_retained_and_pending_slots() {
    let fixture = corpus();
    let cases = fixture["save_load_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 19);
    for case in cases {
        let theme = ThemeRuntime {
            slots: slots(&case["input"]["saved_slots"]),
            ..Default::default()
        };
        assert_eq!(serde_json::json!(theme.saved_slots()), case["wire"]);
    }
}

#[test]
fn saved_theme_load_and_volume_ai_follow_native_transitions() {
    let fixture = corpus();
    let root =
        std::env::temp_dir().join(format!("vera20k-saved-theme-native-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    // A tiny decoded stream is only a transport fixture. The native witness
    // supplies Track0..7 and records PlayFile without decoding any waveform.
    let mut wav = b"RIFF".to_vec();
    wav.extend_from_slice(&40u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&22_050u32.to_le_bytes());
    wav.extend_from_slice(&88_200u32.to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&4u32.to_le_bytes());
    wav.extend_from_slice(&[0; 4]);
    for index in 0..8 {
        std::fs::write(root.join(format!("Track{index}.wav")), &wav).unwrap();
    }
    let assets = AssetManager::from_loose_root_for_test(&root);
    let entries = catalog_from_ini(&IniFile::from_str(
        "[Themes]\n0=Track0\n1=Track1\n2=Track2\n3=Track3\n4=Track4\n5=Track5\n6=Track6\n7=Track7\n\
         [Track0]\nSound=Track0\n[Track1]\nSound=Track1\n[Track2]\nSound=Track2\n\
         [Track3]\nSound=Track3\n[Track4]\nSound=Track4\n[Track5]\nSound=Track5\n\
         [Track6]\nSound=Track6\n[Track7]\nSound=Track7\n",
    ));
    let mut covered = 0;
    for case in fixture["save_load_cases"]
        .as_array()
        .unwrap()
        .iter()
        .chain(fixture["ai_volume_cases"].as_array().unwrap())
        .chain(fixture["settle_cases"].as_array().unwrap())
    {
        // The two native IStream failure controls establish the native leaf;
        // VERA's independent snapshot decoder rejects failures before commit.
        if case["load_hresult"].as_i64().unwrap() != 0 {
            continue;
        }
        let input = &case["input"];
        let name = input["name"].as_str().unwrap();
        let mut theme = ThemeRuntime::with_entries(entries.clone());
        theme.catalog_loaded = input["scores_present"].as_i64().unwrap() != 0;
        theme.slots = slots(&input["outgoing_slots"]);
        theme.set_volume(input["volume"].as_i64().unwrap() as i32);
        theme.global_repeat = input["global_repeat"].as_i64().unwrap() != 0;
        for index in input["repeat_entries"].as_array().unwrap() {
            theme.entries[index.as_u64().unwrap() as usize].repeat = true;
        }
        theme.fading = input["fading"].as_i64().unwrap() != 0;
        theme.fade_started_at_ms =
            theme
                .fading
                .then_some(if input["interpolating"].as_bool().unwrap() {
                    1_500
                } else {
                    0
                });
        let gates = ThemeGates {
            launcher_audio_available: input["device"].as_i64().unwrap() != 0,
            startup_suppressed: input["startup_suppressed"].as_i64().unwrap() != 0,
        };
        let mut physical = if !input["stream_exists"].as_bool().unwrap() {
            MusicOutputState::Unavailable
        } else if input["stream_playing"].as_bool().unwrap() {
            MusicOutputState::Playing
        } else {
            MusicOutputState::Idle
        };
        let saved = slots(&input["saved_slots"]);
        let mut starts = Vec::new();
        let mut stopped = false;
        theme.restore_saved_slots(
            [saved.retained, saved.pending],
            input["in_game_music"].as_i64().unwrap() != 0,
            &assets,
            gates,
            physical,
            2_000,
            |action| {
                if physical.stream_exists() {
                    if action.stop_output {
                        stopped = true;
                        physical = MusicOutputState::Idle;
                    }
                    if let Some(track) = action.start {
                        starts.push(format!("{}.WAV", track.stem));
                        physical = MusicOutputState::Playing;
                    }
                }
                physical
            },
        );
        assert_eq!(theme.slots, slots(&case["loaded_slots"]), "{name}");
        assert_eq!(theme.fading, case["fading"], "{name}");
        assert_eq!(
            physical == MusicOutputState::Playing,
            case["stream_playing"],
            "{name}"
        );
        let events = case["events"].as_array().unwrap();
        let native_starts: Vec<_> = events
            .iter()
            .filter(|event| event["event"] == "play_file")
            .map(|event| event["filename"].as_str().unwrap())
            .collect();
        assert_eq!(starts, native_starts, "{name}");
        assert_eq!(
            stopped,
            events.iter().any(|event| event["event"] == "stop_stream"),
            "{name}"
        );
        if let Some(after) = case.get("after_ai") {
            let action = theme.ai(
                gates,
                physical,
                2_001,
                &mut |_| panic!("{name}: absent stream must not decode"),
                &mut |_, _| panic!("{name}: native consumed no Main draws"),
            );
            assert!(action.start.is_none() && !action.stop_output, "{name}");
            assert_eq!(theme.slots, slots(&after["slots"]), "{name} AI");
            assert_eq!(theme.fading, after["fading"], "{name} AI");
        }
        if let Some(after) = case.get("after_settle_consumer") {
            let action = match after["consumer"].as_str().unwrap() {
                "query" => {
                    let (playing, action) = theme.still_playing(physical, 2_000);
                    assert_eq!(playing, after["query_playing"], "{name}");
                    action
                }
                "queue" => theme.queue_song(
                    after["arguments"][0].as_i64().unwrap() as i32,
                    gates,
                    physical,
                    2_000,
                ),
                "stop" => theme.stop(
                    gates,
                    after["arguments"][0].as_i64().unwrap() != 0,
                    physical,
                    2_000,
                ),
                "ai_hold" => theme.ai(
                    gates,
                    physical,
                    2_000,
                    &mut |_| panic!("{name}: held pending must not decode"),
                    &mut |_, _| panic!("{name}: native consumed no Main draws"),
                ),
                consumer => panic!("unknown native settle consumer {consumer}"),
            };
            assert!(action.start.is_none(), "{name}");
            assert_eq!(
                action.stop_output,
                after["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|event| event["event"] == "stop_stream"),
                "{name}"
            );
            if action.stop_output && physical.stream_exists() {
                physical = MusicOutputState::Idle;
            }
            assert_eq!(theme.slots, slots(&after["slots"]), "{name}");
            assert_eq!(theme.fading, after["fading"], "{name}");
            assert_eq!(
                physical == MusicOutputState::Playing,
                after["stream_playing"],
                "{name}"
            );
        }
        covered += 1;
    }
    assert_eq!(
        covered, 25,
        "17 successful loads, four AI volume and four completed-fade continuations"
    );
    std::fs::remove_dir_all(root).unwrap();
}
