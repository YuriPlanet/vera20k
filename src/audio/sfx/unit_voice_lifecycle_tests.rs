//! Original unit voice handles across endpoint, Limit, destruction and load.
//!
//! These comparisons start with the native post-head Main state. G's list
//! draws and the Infantry AI's other work are covered by their caller tests.
//! Real retail samples enter the existing SfxPlayer service, but its Players
//! are paused after admission. Endpoint405A00 is invoked explicitly below;
//! wall time and silence never stand in for native natural completion.

use super::unit_voice_tests::native;
use super::*;
use crate::sim::rng::{SimRng, trace_draws};
use serde_json::Value;

fn history(name: &str) -> &'static Value {
    native()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .chain(native()["voice_controls"].as_array().unwrap())
        .find(|row| row["name"] == name)
        .unwrap()
}

fn boundary<'a>(history: &'a Value, label: &str) -> &'a Value {
    history["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["label"] == label)
        .unwrap()
}

fn main_hex<'a>(history: &'a Value, state: &Value) -> &'a str {
    history["complete_rng_states"][state["rng"]["main"].as_str().unwrap()]["bytes"]
        .as_str()
        .unwrap()
}

fn sound_name(index: i64) -> Option<&'static str> {
    if index == -1 {
        return None;
    }
    Some(
        native()["retail"]["sounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|sound| sound["index"] == index)
            .unwrap()["name"]
            .as_str()
            .unwrap(),
    )
}

fn actor_id(history: &Value, pointer: u64) -> u64 {
    history["initial"]["actors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|actor| actor["pointer"] == pointer)
        .unwrap()["id"]
        .as_u64()
        .unwrap()
}

/// Comparison plumbing only: all mutable audio state remains in the actual
/// SfxPlayer/VoiceQueue/arbiter and the caller's canonical Main generator.
struct Playback<'a> {
    player: SfxPlayer,
    main: SimRng,
    history: &'static Value,
    registry: &'a SoundRegistry,
    assets: &'a AssetManager,
    index: Option<&'a crate::assets::audio_bag::AudioIndex>,
}

fn with_playback(name: &str, run: impl FnOnce(&mut Playback<'_>)) {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    player.set_volume(0.0);
    let definitions = crate::rules::audio_sources::AudioDefinitions::select(&assets);
    let selected_index = assets.load_audio_index().unwrap();
    let history = history(name);
    let main = SimRng::from_native_state_hex_for_test(main_hex(
        history,
        &boundary(history, "audio_pump_1034")["before"],
    ));
    let mut playback = Playback {
        player,
        main,
        history,
        registry: definitions.sounds(),
        assets: &assets,
        index: selected_index.as_ref().map(|selected| &selected.index),
    };
    // Native constructed an empty pool and admitted its 1000ms service before
    // G. This empty service has no draw; do not substitute a new RNG seed.
    playback.player.service_events(
        1000,
        playback.registry,
        playback.assets,
        playback.index,
        &mut |lo, hi| playback.main.next_range_i32_inclusive(lo, hi),
    );
    assert!(playback.player.play_registered_sound_spatial(
        "CommandBar",
        SpatialGain::CENTRED_FULL,
        playback.registry,
    ));
    playback.queue_requests("actual_G");
    run(&mut playback);
    playback.player.stop_all();
}

impl Playback<'_> {
    fn queue_requests(&mut self, phase: &str) {
        let requests: Vec<_> = self.history["ordered_calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| call["phase"] == phase && call["kind"] == "QueueVoice")
            .collect();
        assert!(!requests.is_empty(), "native QueueVoice calls in {phase}");
        for request in requests {
            self.player.queue_unit_voice(
                actor_id(self.history, request["this"].as_u64().unwrap()),
                sound_name(request["args"][0].as_i64().unwrap()).unwrap(),
            );
        }
    }

    fn visit(&mut self, phase: &str) {
        let owners: Vec<_> = self.history["ordered_calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|call| {
                call["phase"] == phase
                    && call["kind"] == "native_site"
                    && call["pc"] == 0x6f9ebb_u64
            })
            .map(|call| actor_id(self.history, call["this"].as_u64().unwrap()))
            .collect();
        assert!(!owners.is_empty(), "executed native heads in {phase}");
        for owner in owners {
            self.player.visit_unit_voice(owner, self.registry);
        }
        self.assert_state(&boundary(self.history, phase)["after"]);
    }

    fn service(&mut self, milliseconds: u64) {
        let step = boundary(self.history, &format!("audio_pump_{milliseconds}"));
        assert_eq!(
            self.main.native_state_hex(),
            main_hex(self.history, &step["before"])
        );
        let mut ranges = Vec::new();
        let (_, draws) = trace_draws(|| {
            self.player.service_events(
                milliseconds,
                self.registry,
                self.assets,
                self.index,
                &mut |lo, hi| {
                    let result = self.main.next_range_i32_inclusive(lo, hi);
                    if lo != hi {
                        ranges.push((lo, hi, result));
                    }
                    result
                },
            );
        });
        // Freeze the real source queues; subsequent checks supply endpoint
        // and pool-service transitions explicitly, without sleeping.
        for output in self.player.live.values() {
            output.player.pause();
        }
        let expected_ranges: Vec<_> = step["requests"]
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
        assert_eq!(ranges, expected_ranges, "native service {milliseconds}");
        assert_eq!(
            draws.iter().map(|draw| &draw["value"]).collect::<Vec<_>>(),
            step["advances"]
                .as_array()
                .unwrap()
                .iter()
                .map(|draw| &draw["raw"])
                .collect::<Vec<_>>()
        );
        assert_eq!(
            self.main.native_state_hex(),
            main_hex(self.history, &step["after"])
        );
        self.assert_state(&step["after"]);
    }

    fn assert_state(&mut self, expected: &Value) {
        let events = expected["events"].as_array().unwrap();
        assert_eq!(
            self.player.arbiter.live_event_count(),
            expected["live_count"].as_u64().unwrap() as usize
        );
        assert_eq!(
            self.player.arbiter.busy_channel_count(),
            events.iter().filter(|event| event["channel"] != 0).count()
        );
        assert!(
            events
                .iter()
                .all(|event| matches!(event["state"].as_u64(), Some(0 | 3 | 4)))
        );
        assert_eq!(
            self.player.pending.len(),
            events.iter().filter(|event| event["state"] == 0).count()
        );
        assert_eq!(
            self.player.live.len(),
            events.iter().filter(|event| event["state"] == 3).count()
        );
        for actor in expected["actors"].as_array().unwrap() {
            let owner = actor["id"].as_u64().unwrap();
            if actor["alive"] == 0 {
                // The native snapshot still reads freed object bytes. Its
                // pending/last fields no longer describe a living owner.
                assert_eq!(self.player.voice_queue.pending_for(owner), None);
                assert_eq!(self.player.voice_queue.playing_for(owner), None);
                assert_eq!(
                    self.player
                        .arbiter
                        .validate_loop_handle(HandleOwner::UnitVoice(owner)),
                    None
                );
                continue;
            }
            for (field, actual) in [
                ("pending", self.player.voice_queue.pending_for(owner)),
                ("last", self.player.voice_queue.playing_for(owner)),
            ] {
                let expected =
                    sound_name(actor[field].as_i64().unwrap()).map(str::to_ascii_uppercase);
                assert_eq!(actual, expected.as_deref(), "owner {owner} {field}");
            }
            // A native handle is [event pointer, serial, Voc pointer, tag].
            // Compare its identity with the captured pool, not playback state:
            // an ended state4 event retains a valid handle until retirement.
            let handle = &actor["handle"];
            let expected_event = events.iter().find(|event| {
                event["pointer"] == handle[0]
                    && event["serial"] == handle[1]
                    && event["sound_pointer"] == handle[2]
            });
            let actual_event = self
                .player
                .arbiter
                .validate_loop_handle(HandleOwner::UnitVoice(owner));
            assert_eq!(
                actual_event.is_some(),
                expected_event.is_some(),
                "owner {owner} native handle identity"
            );
            if let (Some(actual), Some(expected)) = (actual_event, expected_event) {
                assert_eq!(
                    self.player.live.contains_key(&actual),
                    expected["state"] == 3,
                    "owner {owner} device output follows its own event"
                );
            }
        }
    }
}

#[test]
fn ended_unit_voice_holds_a_different_request_until_native_pool_retirement() {
    with_playback("two_live_E1_G_logic_pool", |playback| {
        playback.visit("original_pre_delivery_Logic_object_loop");
        playback.service(1034);
        playback.queue_requests("different_pending_while_playing");
        playback.queue_requests("same_pending_while_playing");
        playback.visit("original_Logic_holds_different_clears_same");

        let endpoint = boundary(playback.history, "original_worker_2601");
        assert!(
            endpoint["after"]["events"]
                .as_array()
                .unwrap()
                .iter()
                .all(|event| event["state"] == 4)
        );
        assert_eq!(
            main_hex(playback.history, &endpoint["before"]),
            main_hex(playback.history, &endpoint["after"])
        );
        let events: Vec<_> = playback.player.live.keys().copied().collect();
        for event in events {
            // Existing device endpoint and output cleanup, not a substitute
            // voice completion rule. Production capture covers actual pulls.
            playback.player.arbiter.notify_playout_ended(event);
            playback.player.release_output(event);
        }
        playback.assert_state(&endpoint["after"]);
        playback.visit("original_Logic_holds_until_pool_retirement");
        playback.service(2635);
        playback.visit("original_Logic_dispatches_held_after_pool_retirement");
        playback.service(2669);
    });
}

#[test]
fn sixth_gi_voice_retires_the_oldest_at_the_native_limit_service() {
    with_playback("six_G_requests", |playback| {
        let native_entry = native()["retail"]["sounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["name"] == "GIMove")
            .unwrap();
        assert_eq!(
            i64::from(playback.registry.get("GIMove").unwrap().limit),
            native_entry["fields"]["limit"].as_i64().unwrap()
        );
        playback.visit("original_Logic");
        assert_eq!(playback.player.pending_unit_voice_owners().len(), 0);
        let owners: Vec<_> = playback.history["initial"]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|actor| actor["id"].as_u64().unwrap())
            .collect();
        assert_eq!(owners.len(), 6, "executed Limit5 control");
        for owner in &owners {
            assert!(
                playback
                    .player
                    .arbiter
                    .validate_loop_handle(HandleOwner::UnitVoice(*owner))
                    .is_some()
            );
        }
        playback.service(1034);
        let retained: Vec<_> = owners
            .into_iter()
            .filter(|owner| {
                playback
                    .player
                    .arbiter
                    .validate_loop_handle(HandleOwner::UnitVoice(*owner))
                    .is_some()
            })
            .collect();
        assert_eq!(
            retained,
            [2, 3, 4, 5, 6],
            "native actor1 is the oldest reached request"
        );
    });
}

#[test]
fn native_destruction_forgets_only_its_pending_or_playing_unit_voice() {
    for name in ["destroy_pending_before_head", "destroy_while_playing"] {
        with_playback(name, |playback| {
            if name == "destroy_while_playing" {
                playback.visit("original_Logic");
                playback.service(1034);
            }
            let uninit = boundary(playback.history, "original_Foot_UnInit");
            let destroyed = uninit["after"]["actors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|actor| actor["alive"] == 0)
                .unwrap();
            let owner = destroyed["id"].as_u64().unwrap();
            // UnInit removes Logic membership, but the original deferred
            // destructor is the handle cleanup boundary represented here.
            assert_eq!(uninit["before"]["events"], uninit["after"]["events"]);
            let destructor = boundary(playback.history, "original_deferred_destructor");
            assert_eq!(
                main_hex(playback.history, &destructor["before"]),
                main_hex(playback.history, &destructor["after"])
            );
            let before = playback.main.native_state_hex();
            playback.player.destroy_unit_voice(owner);
            assert_eq!(playback.main.native_state_hex(), before);
            playback.assert_state(&destructor["after"]);
            if name == "destroy_pending_before_head" {
                playback.visit("original_survivor_Logic");
                playback.service(1034);
            } else {
                playback.service(1068);
            }
        });
    }
}

#[test]
fn native_load_reset_discards_pending_playing_handles_and_real_outputs() {
    with_playback("load_clear_and_reset", |playback| {
        playback.visit("original_Logic");
        playback.service(1034);
        playback.queue_requests("held_pending");
        playback.assert_state(&boundary(playback.history, "held_pending")["after"]);
        let before = playback.main.native_state_hex();
        playback.player.stop_all();
        // The common app reset combines SoundSystem404E70's pool cleanup
        // with TechnoLoad70C21F..70C246's reconstructed latches/handles.
        let last_reset = playback.history["boundaries"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|step| {
                step["label"]
                    .as_str()
                    .unwrap()
                    .starts_with("original_TechnoLoad_voice_reset_")
            })
            .unwrap();
        playback.assert_state(&last_reset["after"]);
        assert!(playback.player.live.is_empty());
        assert!(playback.player.pending.is_empty());
        assert!(playback.player.loops.is_empty());
        assert_eq!(playback.main.native_state_hex(), before);
        assert_eq!(before, main_hex(playback.history, &last_reset["after"]));
        playback.service(1068);
        // A later real head cannot resurrect the outgoing held request.
        for actor in playback.history["initial"]["actors"].as_array().unwrap() {
            playback
                .player
                .visit_unit_voice(actor["id"].as_u64().unwrap(), playback.registry);
        }
        playback.assert_state(&playback.history["final"]);
    });
}
