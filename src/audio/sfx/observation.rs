//! Opt-in bounded observation of the existing Player output queue.
//!
//! The production device mixer's pulls traverse this forwarding Source after
//! Player gain, pause and stop processing, before resampling and summing.
//! Mixer initialization may prefetch samples before its device callback. This
//! is not a second mixer, an OS loopback, or proof of physical speaker output.
//! The callback allocates nothing and takes no diagnostic locks. Samples are
//! written once into preallocated atomic slots; the app copies a published
//! prefix. Queue filler silence is deliberately retained.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use rodio::Source;
use serde::Serialize;

use super::playback::{PlaybackObservation, PlaybackObservationSnapshot};
use super::{EventId, HandleOwner};
use crate::audio::arbiter::{EventToken, SoundArbiter};
use crate::audio::voice_queue::VoiceQueue;

const MAX_ACTIONS: usize = 64;
const MAX_SAMPLE_NAMES: usize = 128;
const MAX_FORMATS: usize = 64;

#[derive(Clone, Debug)]
pub(crate) struct PcmObservationConfig {
    pub sound_ids: Vec<String>,
    pub max_events: usize,
    pub max_samples_per_event: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::arbiter::{EntryFacts, PlayRequest, TestPlayback};
    use crate::rules::sound_ini::VOLUME_SCALE;
    use rodio::{Player, buffer::SamplesBuffer};
    use std::num::NonZero;

    fn token(event: u32, serial: u32) -> EventToken {
        EventToken {
            event: EventId(event),
            serial,
        }
    }

    fn observer(capacity: usize) -> PcmObserver {
        let mut observer = PcmObserver::new(PcmObservationConfig {
            sound_ids: vec!["Test".to_owned()],
            max_events: 1,
            max_samples_per_event: capacity,
        })
        .unwrap();
        observer.submitted(token(1, 1), "TEST", Some(vec!["sample".to_owned()]), 4, 0);
        observer
    }

    fn source(samples: Vec<f32>) -> SamplesBuffer {
        SamplesBuffer::new(
            NonZero::new(2).unwrap(),
            NonZero::new(22_050).unwrap(),
            samples,
        )
    }

    #[test]
    fn forwarding_preserves_all_sample_bits_and_metadata_even_when_capture_overflows() {
        let samples = vec![
            0.0,
            -0.0,
            0.25,
            -0.5,
            f32::INFINITY,
            f32::from_bits(0x7fc0_4321),
        ];
        let mut recorder = observer(4);
        let queue = source(samples.clone());
        let span = queue.current_span_len();
        let duration = queue.total_duration();
        let mut tap = recorder.wrap(token(1, 1), queue, 34).unwrap();
        assert_eq!(tap.channels().get(), 2);
        assert_eq!(tap.sample_rate().get(), 22_050);
        assert_eq!(tap.current_span_len(), span);
        assert_eq!(tap.total_duration(), duration);
        assert_eq!(
            tap.by_ref().map(f32::to_bits).collect::<Vec<_>>(),
            samples.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
        let report = recorder.finish();
        assert_eq!(
            report.outputs[0].sample_bits,
            samples[..4].iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
        assert!(report.outputs[0].truncated);
        assert_eq!(report.outputs[0].formats[0].sample_rate, 22_050);
    }

    #[test]
    fn actual_player_gain_is_upstream_and_completion_is_not_inferred_from_silence() {
        let mut recorder = observer(2048);
        let (player, queue) = Player::new();
        player.set_volume(0.25);
        player.append(source(vec![1.0, -1.0, 0.0, 0.0]));
        let mut tap = recorder.wrap(token(1, 1), queue, 34).unwrap();
        assert_eq!(
            tap.by_ref().take(4).collect::<Vec<_>>(),
            [0.25, -0.25, 0.0, 0.0]
        );
        // Queue completion is reported only when a subsequent pull observes
        // the consumed source's end. Its keep-alive silence is still forwarded.
        assert_eq!(tap.next(), Some(0.0));
        assert!(player.empty());
        assert!(!recorder.settled());
        recorder.action(token(1, 1), "completed", 68);
        assert!(!recorder.settled(), "the real queue must also finish");
        drop(player);
        while tap.next().is_some() {}
        assert!(recorder.settled());
        let report = recorder.finish().into_json();
        assert_eq!(
            report["outputs"][0]["pcm"]["finite_count"],
            report["outputs"][0]["pcm"]["sample_count"]
        );
        assert_eq!(report["outputs"][0]["pcm"]["nonzero_count"], 2);
        assert_eq!(report["outputs"][0]["actions"][2]["kind"], "completed");
    }

    #[test]
    fn observed_connection_preserves_actual_mixer_initialization_and_output() {
        // rodio 0.22.2 Mixer::add synchronously initializes its resampler
        // from an empty Player queue, before ordinary playback appends audio.
        // Use the production observed connection and the actual normal API;
        // moving append before the observed connection changes this output.
        for device_rate in [44_100, 48_000] {
            let render = |observed| {
                let mut recorder = observer(8192);
                let (mixer, mut mixed) = rodio::mixer::mixer(
                    NonZero::new(2).unwrap(),
                    NonZero::new(device_rate).unwrap(),
                );
                let player = if observed {
                    recorder.connect_player(token(1, 1), &mixer, 34).unwrap()
                } else {
                    Player::connect_new(&mixer)
                };
                player.set_volume(0.25);
                let samples = (0..2048).map(|index| ((index % 257) as f32 - 128.0) / 128.0);
                player.append(source(samples.collect()));
                let mut bits = Vec::new();
                while !player.empty() {
                    assert!(bits.len() < 16_384, "bounded actual mixer playout");
                    bits.push(mixed.next().expect("connected queue").to_bits());
                }
                if observed {
                    recorder.action(token(1, 1), "completed", 68);
                }
                drop(player);
                for sample in mixed {
                    assert!(bits.len() < 16_384, "bounded actual mixer drain");
                    bits.push(sample.to_bits());
                }
                if observed {
                    assert!(recorder.settled());
                    let report = recorder.finish();
                    assert!(!report.truncated && !report.outputs[0].truncated);
                    assert!(report.outputs[0].source_ended);
                    assert!(report.outputs[0].sample_bits.iter().any(|bits| *bits != 0));
                }
                bits
            };
            assert_eq!(render(true), render(false), "device rate {device_rate}");
        }
    }

    #[test]
    fn release_and_detach_do_not_complete_the_observed_output_or_rebind_old_owners() {
        for operation in ["release", "detach"] {
            let mut recorder = observer(8);
            recorder.bind_owner(token(1, 1), HandleOwner::Positional(7));
            recorder.owner_action(HandleOwner::Positional(7), operation, 40);
            recorder.owner_action(HandleOwner::Positional(7), "release", 50);
            assert!(!recorder.settled());
            recorder.action(token(1, 1), "completed", 100);
            recorder.action(token(1, 1), "stopped", 101);
            let report = recorder.finish();
            assert_eq!(
                report.outputs[0]
                    .actions
                    .iter()
                    .map(|action| action.kind)
                    .collect::<Vec<_>>(),
                ["submitted", operation, "completed"]
            );
            assert_eq!(report.outputs[0].owner, Some(7));
        }
    }

    #[test]
    fn reused_event_slot_keeps_old_draining_pcm_separate_from_the_new_submission() {
        let mut recorder = PcmObserver::new(PcmObservationConfig {
            sound_ids: vec!["Test".to_owned()],
            max_events: 2,
            max_samples_per_event: 16,
        })
        .unwrap();
        let old_token = token(0, 1);
        let new_token = token(0, 2);
        recorder.submitted(old_token, "TEST", Some(vec!["old".to_owned()]), 4, 0);
        recorder.bind_owner(old_token, HandleOwner::Positional(7));
        let mut old = recorder
            .wrap(old_token, source(vec![1.0, 2.0, 3.0, 4.0]), 34)
            .unwrap();
        assert_eq!(old.next(), Some(1.0));
        recorder.submitted(new_token, "TEST", Some(vec!["new".to_owned()]), 2, 69);
        recorder.bind_owner(new_token, HandleOwner::Positional(8));
        let mut new = recorder
            .wrap(new_token, source(vec![10.0, 20.0]), 103)
            .unwrap();
        // The older output is deliberately nonterminal when the slot is
        // reused. Its delayed stop must reach its serial, not the newest row.
        recorder.action(old_token, "stopped", 104);
        recorder.bind_owner(old_token, HandleOwner::Positional(99));
        recorder.samples(old_token, Some(vec!["stale".to_owned()]), 99);
        assert!(recorder.contains(new_token));
        old.by_ref().for_each(drop);
        new.by_ref().for_each(drop);
        assert!(!recorder.settled());
        recorder.action(new_token, "completed", 137);
        assert!(recorder.settled());
        assert_eq!(
            recorder.observed_tokens().collect::<Vec<_>>(),
            [old_token, new_token]
        );
        let report = recorder.finish();
        let first = &report.outputs[0];
        let second = &report.outputs[1];
        assert_eq!((first.submission, second.submission), (0, 1));
        assert_eq!((first.event, second.event), (0, 0));
        assert_eq!((first.event_serial, second.event_serial), (1, 2));
        assert_eq!((first.owner, second.owner), (Some(7), Some(8)));
        assert_eq!(first.resolved_samples, ["old"]);
        assert_eq!(second.resolved_samples, ["new"]);
        assert_eq!(
            first
                .sample_bits
                .iter()
                .copied()
                .map(f32::from_bits)
                .collect::<Vec<_>>(),
            [1.0, 2.0, 3.0, 4.0]
        );
        assert_eq!(
            second
                .sample_bits
                .iter()
                .copied()
                .map(f32::from_bits)
                .collect::<Vec<_>>(),
            [10.0, 20.0]
        );
        assert_eq!(
            first
                .actions
                .iter()
                .map(|action| action.kind)
                .collect::<Vec<_>>(),
            ["submitted", "started", "stopped"]
        );
        assert_eq!(
            second
                .actions
                .iter()
                .map(|action| action.kind)
                .collect::<Vec<_>>(),
            ["submitted", "started", "completed"]
        );
    }

    fn submit(arbiter: &mut SoundArbiter) -> EventToken {
        let event = arbiter
            .submit(
                &PlayRequest {
                    key: "TEST".to_owned(),
                    facts: EntryFacts {
                        priority: 2,
                        limit: 5,
                        control: 0,
                        loop_count: 0,
                        delay_ms: (0, 0),
                        entry_volume_linear: VOLUME_SCALE,
                        samples_ok: true,
                        samples_resolved: true,
                    },
                    volume_linear: VOLUME_SCALE,
                    pan: VOLUME_SCALE / 2,
                },
                0,
            )
            .unwrap();
        arbiter.event_token(event).unwrap()
    }

    #[test]
    fn final_snapshot_reads_real_owner_after_pcm_completion_and_slot_reuse() {
        let mut arbiter = SoundArbiter::new(0);
        let old_token = submit(&mut arbiter);
        let mut recorder = PcmObserver::new(PcmObservationConfig {
            sound_ids: vec!["Test".to_owned()],
            max_events: 2,
            max_samples_per_event: 8,
        })
        .unwrap();
        recorder.submitted(old_token, "TEST", None, 2, 0);
        let mut old = recorder.wrap(old_token, source(vec![1.0, 2.0]), 0).unwrap();
        old.by_ref().for_each(drop);
        recorder.action(old_token, "completed", 34);
        assert!(recorder.settled());

        // Admission uses the actual arbiter owner. Decoder readiness is only
        // a fixture input; no audio, native arithmetic or timing is claimed.
        arbiter.update_tick(34, &mut TestPlayback::default());
        recorder.snapshot(&arbiter, &[old_token], &[old_token]);
        assert_eq!(arbiter.event_token(old_token.event), Some(old_token));
        assert!(arbiter.token_has_channel(old_token));
        assert_eq!(recorder.outputs[0].event_live_at_finish, Some(true));
        assert_eq!(recorder.outputs[0].channel_live_at_finish, Some(true));
        assert_eq!(
            recorder.outputs[0].token_slot_occupied_at_finish,
            Some(true)
        );
        assert_eq!(recorder.outputs[0].pending_payload_at_finish, Some(true));
        assert_eq!(recorder.outputs[0].live_payload_at_finish, Some(true));
        assert_eq!(recorder.live_event_count_at_finish, Some(1));
        assert_eq!(recorder.busy_channel_count_at_finish, Some(1));

        // Stop invalidates this serial and releases its channel immediately,
        // but only the ordinary reaping pass frees the occupied pool slot.
        arbiter.stop(old_token.event);
        recorder.snapshot(&arbiter, &[], &[]);
        assert_eq!(recorder.outputs[0].event_live_at_finish, Some(false));
        assert_eq!(recorder.outputs[0].channel_live_at_finish, Some(false));
        assert_eq!(
            recorder.outputs[0].token_slot_occupied_at_finish,
            Some(true)
        );
        assert_eq!(recorder.live_event_count_at_finish, Some(1));
        assert_eq!(recorder.busy_channel_count_at_finish, Some(0));
        arbiter.update_tick(68, &mut TestPlayback::default());
        recorder.snapshot(&arbiter, &[], &[]);
        assert_eq!(
            recorder.outputs[0].token_slot_occupied_at_finish,
            Some(false)
        );
        assert_eq!(recorder.live_event_count_at_finish, Some(0));

        arbiter.clear_for_world_replacement();
        let new_token = submit(&mut arbiter);
        assert_eq!(old_token.event, new_token.event);
        assert_ne!(old_token.serial, new_token.serial);
        recorder.submitted(new_token, "TEST", None, 0, 68);
        // An orphan old payload is observed even after its pool token died;
        // it must not be hidden by checking only the new slot's liveness.
        recorder.snapshot(&arbiter, &[new_token], &[old_token]);
        assert_eq!(recorder.outputs[0].event_live_at_finish, Some(false));
        assert_eq!(recorder.outputs[0].channel_live_at_finish, Some(false));
        assert_eq!(
            recorder.outputs[0].token_slot_occupied_at_finish,
            Some(true)
        );
        assert_eq!(recorder.outputs[0].pending_payload_at_finish, Some(false));
        assert_eq!(recorder.outputs[0].live_payload_at_finish, Some(true));
        assert_eq!(recorder.outputs[1].event_live_at_finish, Some(true));
        assert_eq!(recorder.outputs[1].channel_live_at_finish, Some(false));
        assert_eq!(
            recorder.outputs[1].token_slot_occupied_at_finish,
            Some(true)
        );
        assert_eq!(recorder.outputs[1].pending_payload_at_finish, Some(true));
        assert_eq!(recorder.outputs[1].live_payload_at_finish, Some(false));

        arbiter.clear_for_world_replacement();
        recorder.snapshot(&arbiter, &[], &[]);
        let report = recorder.finish();
        assert_eq!(report.live_event_count_at_finish, Some(0));
        assert_eq!(report.busy_channel_count_at_finish, Some(0));
        for output in report.outputs {
            assert_eq!(output.event_live_at_finish, Some(false));
            assert_eq!(output.channel_live_at_finish, Some(false));
            assert_eq!(output.token_slot_occupied_at_finish, Some(false));
            assert_eq!(output.pending_payload_at_finish, Some(false));
            assert_eq!(output.live_payload_at_finish, Some(false));
        }
    }

    #[test]
    fn missing_final_owner_snapshot_is_explicitly_unobserved() {
        let mut recorder = observer(8);
        recorder.action(token(1, 1), "completed", 34);
        assert!(recorder.settled());
        let report = recorder.finish().into_json();
        let output = &report["outputs"][0];
        assert_eq!(output["event_serial"], 1);
        for key in [
            "event_live_at_finish",
            "channel_live_at_finish",
            "token_slot_occupied_at_finish",
            "pending_payload_at_finish",
            "live_payload_at_finish",
        ] {
            assert!(output[key].is_null());
        }
        assert!(report["live_event_count_at_finish"].is_null());
        assert!(report["busy_channel_count_at_finish"].is_null());
        assert!(report["coordinate_request_count_at_finish"].is_null());
        assert!(output.get("playback_clips").is_none());
        assert!(output.get("source_state_alive_at_finish").is_none());
    }

    #[test]
    fn coordinate_request_identity_is_distinct_from_an_actor_with_the_same_number() {
        let mut recorder = observer(8);
        recorder.bind_owner(token(1, 1), HandleOwner::Coordinate(7));
        recorder.owner_action(HandleOwner::Positional(7), "stopped", 12);
        assert!(!recorder.settled());
        recorder.action(token(1, 1), "completed", 34);
        let report = recorder.finish().into_json();
        let output = &report["outputs"][0];
        assert_eq!(output["owner"], 7);
        assert_eq!(output["owner_role"], "coordinate_request");
        assert_eq!(output["actions"].as_array().unwrap().len(), 2);
        assert!(report["coordinate_request_count_at_finish"].is_null());
    }

    #[test]
    fn playback_owner_records_real_fill_pull_and_retirement_independent_of_queue_end() {
        use super::super::playback::{PlaylistCursor, RingSource};
        use super::super::{DecodedAudio, LoadedPlayback, PlayShifts, PlaybackDraws};
        use crate::rules::{ini_parser::IniFile, sound_ini::SoundRegistry};

        // Synthetic decoded fixture bytes through the actual sound reader,
        // loaded owner and ring. Counts are observation checks, not native
        // playback arithmetic or production timing goldens.
        let registry = SoundRegistry::from_ini(&IniFile::from_str(
            "[SoundList]\n1=Test\n[Test]\nSounds=attack body decay\nControl=attack all decay\n",
        ));
        let entry = registry.get("Test").unwrap();
        let mut draws = PlaybackDraws::new(|low, _| low);
        let loaded = LoadedPlayback::load(entry, &mut draws, |_| {
            Some(DecodedAudio {
                samples: vec![0.25; 8],
                sample_rate: 22_050,
                channels: 2,
                native_channels: 1,
                native_sample_bytes: 2,
            })
        })
        .unwrap();
        let mut cursor = PlaylistCursor::new(loaded.selected.clone(), entry.control, (0, 0), 1);
        let initial = cursor.prepare(&mut draws, true).unwrap();
        let (source, control) = RingSource::new(
            loaded,
            cursor,
            initial,
            draws,
            PlayShifts::UNITY,
            VOLUME_SCALE / 2,
            Some(&entry.sounds),
        )
        .unwrap();
        let playback = source.observation().unwrap();
        let mut recorder = observer(16_384);
        recorder.playback(token(1, 2), playback.clone());
        assert!(
            recorder.outputs[0].playback.is_none(),
            "wrong serial cannot bind"
        );
        recorder.playback(token(1, 1), playback.clone());
        let mut tap = recorder.wrap(token(1, 1), source, 34).unwrap();
        tap.by_ref().take(3).for_each(drop);
        let prefix = playback.snapshot();
        assert_eq!(prefix.source_sample_count, 24);
        assert_eq!(
            prefix
                .clips
                .iter()
                .map(|clip| clip.pulled_samples)
                .sum::<usize>(),
            3
        );
        assert!(prefix.source_state_alive);

        tap.by_ref().for_each(drop);
        recorder.action(token(1, 1), "completed", 68);
        assert!(
            !recorder.settled(),
            "ended queue still owns the loaded state"
        );
        drop(tap);
        assert!(
            !recorder.settled(),
            "retained control still owns the loaded state"
        );
        drop(control);
        assert!(recorder.settled());
        let report = recorder.finish().into_json();
        let output = &report["outputs"][0];
        assert_eq!(
            output["source_sample_count"], 24,
            "queued count was replaced"
        );
        assert_eq!(
            output["resolved_samples"],
            serde_json::json!(["attack", "body", "decay"])
        );
        assert_eq!(output["source_state_alive_at_finish"], false);
        assert!(
            output["pcm"]["sample_count"].as_u64().unwrap() > 24,
            "ring padding is observed PCM, not actual clip pulls"
        );
        let clips = output["playback_clips"].as_array().unwrap();
        assert_eq!(clips.len(), 3);
        for clip in clips {
            assert_eq!(clip["filled_samples"], 8);
            assert_eq!(clip["pulled_samples"], 8);
        }
    }
}

impl PcmObservationConfig {
    pub(crate) fn validate(&self) -> Result<(), &'static str> {
        if self.sound_ids.is_empty()
            || self.sound_ids.len() > 16
            || self
                .sound_ids
                .iter()
                .any(|id| id.is_empty() || id.len() > 128 || !id.is_ascii())
            // The ordinary low-power row has more than16 UTF-16 units and
            // requests one cue per paint.32 retains its full typing history;
            // worst-case sample hex still occupies at most64MiB of a128MiB
            // map receipt. This opt-in limit never changes audio admission.
            || !(1..=32).contains(&self.max_events)
            || !(1..=262_144).contains(&self.max_samples_per_event)
        {
            return Err("audio observation exceeds its sound/event/sample bounds");
        }
        Ok(())
    }
}

/// Last completed exact-step capture boundary, supplied by the diagnostic
/// controller. It stays fixed while a step runs, including that step's sound
/// submission, and is refreshed after its receipt commits. These fields are
/// not an exact simulation timestamp for an audio action or a PCM sample.
/// Input-driven restore refreshes the loaded frame/tick without adding a step.
#[derive(Clone, Copy, Debug, Default, Serialize)]
pub(crate) struct PcmObservationContext {
    pub completed_steps: u64,
    pub simulation_tick: u64,
    pub binary_frame: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct OutputAction {
    pub kind: &'static str,
    /// SfxPlayer's last serviced wall-clock value. Actions between service
    /// passes retain it; this is not the action's independently sampled time.
    pub service_ms: u64,
    pub context: PcmObservationContext,
}

/// A read-only snapshot of the actual per-object latch. Observation never
/// supplies an owner visit, a handle result, or a voice admission decision.
#[derive(Debug, Serialize)]
pub(super) struct VoiceLatchState {
    pending: Option<String>,
    playing: Option<String>,
}

impl VoiceLatchState {
    pub(super) fn read(queue: &VoiceQueue, owner: u64) -> Self {
        Self {
            pending: queue.pending_for(owner).map(str::to_owned),
            playing: queue.playing_for(owner).map(str::to_owned),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct VoiceAction {
    owner: u64,
    action: OutputAction,
    before: VoiceLatchState,
    after: VoiceLatchState,
    /// Present only for a reached head's actual ValidateOrClear result.
    live_event_before: Option<u32>,
    submitted_event: Option<u32>,
}

#[derive(Debug, Serialize)]
pub(crate) struct PcmFormat {
    pub first_sample: usize,
    pub channels: u16,
    pub sample_rate: u32,
}

pub(crate) struct ObservedOutput {
    pub submission: usize,
    pub event: u32,
    pub event_serial: u32,
    pub owner: Option<u64>,
    pub owner_role: Option<&'static str>,
    pub sound_id: String,
    pub resolved_samples: Vec<String>,
    pub source_sample_count: usize,
    pub source_ended: bool,
    pub event_live_at_finish: Option<bool>,
    pub channel_live_at_finish: Option<bool>,
    pub token_slot_occupied_at_finish: Option<bool>,
    pub pending_payload_at_finish: Option<bool>,
    pub live_payload_at_finish: Option<bool>,
    playback: Option<PlaybackObservationSnapshot>,
    pub actions: Vec<OutputAction>,
    pub formats: Vec<PcmFormat>,
    pub sample_bits: Vec<u32>,
    pub truncated: bool,
}

pub(crate) struct PcmObservationReport {
    pub outputs: Vec<ObservedOutput>,
    pub voice_actions: Vec<VoiceAction>,
    pub truncated: bool,
    pub live_event_count_at_finish: Option<usize>,
    pub busy_channel_count_at_finish: Option<usize>,
    /// Read from SfxPlayer's request owner when attached. A standalone PCM
    /// observer has no coordinate pool and reports None, never a guessed0.
    pub coordinate_request_count_at_finish: Option<usize>,
}

impl PcmObservationReport {
    pub(crate) fn into_json(self) -> serde_json::Value {
        let outputs = self.outputs.into_iter().map(|output| {
            let bytes: Vec<u8> = output.sample_bits.iter().flat_map(|bits| bits.to_le_bytes()).collect();
            let mut hex = String::with_capacity(bytes.len() * 2);
            const DIGITS: &[u8; 16] = b"0123456789abcdef";
            for byte in &bytes {
                hex.push(DIGITS[usize::from(byte >> 4)] as char);
                hex.push(DIGITS[usize::from(byte & 15)] as char);
            }
            let finite = output.sample_bits.iter().filter(|bits| f32::from_bits(**bits).is_finite()).count();
            let nonzero = output.sample_bits.iter().filter(|bits| f32::from_bits(**bits) != 0.0).count();
            let mut row = serde_json::json!({"submission": output.submission, "event": output.event, "event_serial": output.event_serial, "owner": output.owner, "owner_role": output.owner_role, "sound_id": output.sound_id,
                "resolved_samples": output.resolved_samples, "source_sample_count": output.source_sample_count,
                "source_ended": output.source_ended,
                "event_live_at_finish": output.event_live_at_finish,
                "channel_live_at_finish": output.channel_live_at_finish,
                "token_slot_occupied_at_finish": output.token_slot_occupied_at_finish,
                "pending_payload_at_finish": output.pending_payload_at_finish,
                "live_payload_at_finish": output.live_payload_at_finish,
                "actions": output.actions, "pcm": {"encoding": "f32le", "sample_count": output.sample_bits.len(),
                    "finite_count": finite, "nonzero_count": nonzero, "formats": output.formats,
                    "sha256": crate::util::sha256::sha256_hex(&bytes), "hex": hex, "truncated": output.truncated}});
            if let Some(playback) = output.playback {
                row["playback_clips"] = serde_json::json!(playback.clips);
                row["source_state_alive_at_finish"] = serde_json::json!(playback.source_state_alive);
            }
            row
        }).collect::<Vec<_>>();
        serde_json::json!({"outputs": outputs, "voice_actions": self.voice_actions, "truncated": self.truncated,
            "live_event_count_at_finish": self.live_event_count_at_finish,
            "busy_channel_count_at_finish": self.busy_channel_count_at_finish,
            "coordinate_request_count_at_finish": self.coordinate_request_count_at_finish})
    }
}

#[derive(Default)]
struct FormatSlot {
    first_sample: AtomicUsize,
    channels: AtomicU32,
    sample_rate: AtomicU32,
}

struct PcmStorage {
    samples: Box<[AtomicU32]>,
    written: AtomicUsize,
    formats: [FormatSlot; MAX_FORMATS],
    format_count: AtomicUsize,
    truncated: AtomicBool,
    enabled: AtomicBool,
    ended: AtomicBool,
}

impl PcmStorage {
    fn new(capacity: usize) -> Self {
        Self {
            samples: (0..capacity).map(|_| AtomicU32::new(0)).collect(),
            written: AtomicUsize::new(0),
            formats: std::array::from_fn(|_| FormatSlot::default()),
            format_count: AtomicUsize::new(0),
            truncated: AtomicBool::new(false),
            enabled: AtomicBool::new(true),
            ended: AtomicBool::new(false),
        }
    }

    fn snapshot(&self) -> (Vec<u32>, Vec<PcmFormat>, bool) {
        self.enabled.store(false, Ordering::Release);
        // Release-published lengths expose only fully written, immutable slots.
        // A simultaneous last pull may publish a longer prefix afterwards; it
        // cannot change any sample returned by this snapshot.
        let count = self.written.load(Ordering::Acquire);
        let bits = self.samples[..count]
            .iter()
            .map(|slot| slot.load(Ordering::Relaxed))
            .collect();
        let formats = self.formats[..self.format_count.load(Ordering::Acquire)]
            .iter()
            .map(|slot| PcmFormat {
                first_sample: slot.first_sample.load(Ordering::Relaxed),
                channels: slot.channels.load(Ordering::Relaxed) as u16,
                sample_rate: slot.sample_rate.load(Ordering::Relaxed),
            })
            .filter(|format| format.first_sample < count)
            .collect();
        (bits, formats, self.truncated.load(Ordering::Relaxed))
    }
}

/// Metadata and samples still come from the original queue. Additional span
/// queries only read it, once per source span, rather than locking per sample.
pub(super) struct ObservedSource<S> {
    inner: S,
    storage: Arc<PcmStorage>,
    position: usize,
    span_remaining: usize,
    last_format: Option<(u16, u32)>,
    format_count: usize,
}

impl<S: Source> Iterator for ObservedSource<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let enabled = self.storage.enabled.load(Ordering::Relaxed);
        if enabled && self.span_remaining == 0 {
            self.span_remaining = self.inner.current_span_len().unwrap_or(512).max(1);
            let format = (self.inner.channels().get(), self.inner.sample_rate().get());
            if self.last_format != Some(format) {
                if let Some(slot) = self.storage.formats.get(self.format_count) {
                    slot.first_sample.store(self.position, Ordering::Relaxed);
                    slot.channels.store(u32::from(format.0), Ordering::Relaxed);
                    slot.sample_rate.store(format.1, Ordering::Relaxed);
                    self.format_count += 1;
                    self.storage
                        .format_count
                        .store(self.format_count, Ordering::Release);
                } else {
                    self.storage.truncated.store(true, Ordering::Relaxed);
                }
                self.last_format = Some(format);
            }
        }
        let Some(sample) = self.inner.next() else {
            self.storage.ended.store(true, Ordering::Release);
            return None;
        };
        if enabled {
            self.span_remaining = self.span_remaining.saturating_sub(1);
            if let Some(slot) = self.storage.samples.get(self.position) {
                slot.store(sample.to_bits(), Ordering::Relaxed);
                self.position += 1;
                self.storage.written.store(self.position, Ordering::Release);
            } else {
                self.storage.truncated.store(true, Ordering::Relaxed);
                self.storage.enabled.store(false, Ordering::Release);
            }
        }
        Some(sample)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source> Source for ObservedSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> rodio::ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> rodio::SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), rodio::source::SeekError> {
        self.inner.try_seek(pos)?;
        self.span_remaining = 0;
        Ok(())
    }
}

struct OutputRecord {
    token: EventToken,
    owner: Option<HandleOwner>,
    sound_id: String,
    samples: Vec<String>,
    source_sample_count: usize,
    bound: bool,
    actions: Vec<OutputAction>,
    pcm: Option<Arc<PcmStorage>>,
    terminal: bool,
    playback: Option<PlaybackObservation>,
    event_live_at_finish: Option<bool>,
    channel_live_at_finish: Option<bool>,
    token_slot_occupied_at_finish: Option<bool>,
    pending_payload_at_finish: Option<bool>,
    live_payload_at_finish: Option<bool>,
}

pub(super) struct PcmObserver {
    config: PcmObservationConfig,
    context: PcmObservationContext,
    outputs: Vec<OutputRecord>,
    voice_actions: Vec<VoiceAction>,
    truncated: bool,
    live_event_count_at_finish: Option<usize>,
    busy_channel_count_at_finish: Option<usize>,
}

impl PcmObserver {
    // The serial comes from the arbiter allocation. A delayed action cannot
    // reach a later submission that reused its slot; an earlier queue keeps
    // its own PCM storage while the real device drains it.
    fn active_index(&self, token: EventToken) -> Option<usize> {
        self.outputs
            .iter()
            .rposition(|output| output.token == token && !output.terminal)
    }

    pub(super) fn new(config: PcmObservationConfig) -> Result<Self, &'static str> {
        config.validate()?;
        Ok(Self {
            config,
            context: PcmObservationContext::default(),
            outputs: Vec::new(),
            voice_actions: Vec::new(),
            truncated: false,
            live_event_count_at_finish: None,
            busy_channel_count_at_finish: None,
        })
    }

    pub(super) fn wants(&self, sound_id: &str) -> bool {
        self.config
            .sound_ids
            .iter()
            .any(|id| id.eq_ignore_ascii_case(sound_id))
    }

    pub(super) fn context(&mut self, context: PcmObservationContext) {
        self.context = context;
    }

    pub(super) fn voice_action(
        &mut self,
        owner: u64,
        kind: &'static str,
        before: VoiceLatchState,
        after: VoiceLatchState,
        handles: (Option<EventId>, Option<EventId>),
        now_ms: u64,
    ) {
        if ![
            &before.pending,
            &before.playing,
            &after.pending,
            &after.playing,
        ]
        .into_iter()
        .flatten()
        .any(|sound| self.wants(sound))
        {
            return;
        }
        if self.voice_actions.len() == self.config.max_events * MAX_ACTIONS {
            self.truncated = true;
            return;
        }
        self.voice_actions.push(VoiceAction {
            owner,
            action: OutputAction {
                kind,
                service_ms: now_ms,
                context: self.context,
            },
            before,
            after,
            live_event_before: handles.0.map(|event| event.0),
            submitted_event: handles.1.map(|event| event.0),
        });
    }

    pub(super) fn submitted(
        &mut self,
        token: EventToken,
        key: &str,
        samples: Option<Vec<String>>,
        source_sample_count: usize,
        now_ms: u64,
    ) {
        if !self.wants(key) {
            return;
        }
        if self.outputs.len() == self.config.max_events {
            self.truncated = true;
            return;
        }
        self.outputs.push(OutputRecord {
            token,
            owner: None,
            sound_id: key.to_owned(),
            samples: samples.unwrap_or_default(),
            source_sample_count,
            bound: false,
            actions: Vec::new(),
            pcm: None,
            terminal: false,
            playback: None,
            event_live_at_finish: None,
            channel_live_at_finish: None,
            token_slot_occupied_at_finish: None,
            pending_payload_at_finish: None,
            live_payload_at_finish: None,
        });
        self.action(token, "submitted", now_ms);
    }

    pub(super) fn bind_owner(&mut self, token: EventToken, owner: HandleOwner) {
        if let Some(index) = self.active_index(token) {
            let output = &mut self.outputs[index];
            output.owner = Some(owner);
            output.bound = true;
        }
    }

    pub(super) fn samples(
        &mut self,
        token: EventToken,
        samples: Option<Vec<String>>,
        sample_count: usize,
    ) {
        if let Some(index) = self.active_index(token) {
            let output = &mut self.outputs[index];
            output.source_sample_count = output.source_sample_count.saturating_add(sample_count);
            for sample in samples.into_iter().flatten() {
                if output.samples.len() == MAX_SAMPLE_NAMES {
                    self.truncated = true;
                    break;
                }
                output.samples.push(sample);
            }
        }
    }

    pub(super) fn owner_action(&mut self, owner: HandleOwner, kind: &'static str, now_ms: u64) {
        // This diagnostic association is recorded when the production handle
        // binds. It never supplies a gameplay/audio admission decision.
        if let Some(output) = self
            .outputs
            .iter_mut()
            .rev()
            .find(|output| output.owner == Some(owner) && output.bound && !output.terminal)
        {
            output.bound = false;
            let token = output.token;
            self.action(token, kind, now_ms);
        }
    }

    /// Keep only the playback owner's passive observation handle. It owns
    /// neither loaded samples, the source cursor nor its RNG capability.
    pub(super) fn playback(&mut self, token: EventToken, playback: PlaybackObservation) {
        if let Some(index) = self.active_index(token) {
            self.outputs[index].playback = Some(playback);
        }
    }

    pub(super) fn action(&mut self, token: EventToken, kind: &'static str, now_ms: u64) {
        if let Some(index) = self.active_index(token) {
            let output = &mut self.outputs[index];
            if output.actions.len() == MAX_ACTIONS {
                self.truncated = true;
                return;
            }
            output.actions.push(OutputAction {
                kind,
                service_ms: now_ms,
                context: self.context,
            });
            output.terminal = matches!(kind, "completed" | "stopped");
        }
    }

    pub(super) fn wrap<S: Source>(
        &mut self,
        token: EventToken,
        source: S,
        now_ms: u64,
    ) -> Option<ObservedSource<S>> {
        let index = self.active_index(token)?;
        let output = &mut self.outputs[index];
        let pcm = Arc::new(PcmStorage::new(self.config.max_samples_per_event));
        output.pcm = Some(Arc::clone(&pcm));
        self.action(token, "started", now_ms);
        Some(ObservedSource {
            inner: source,
            storage: pcm,
            position: 0,
            span_remaining: 0,
            last_format: None,
            format_count: 0,
        })
    }

    pub(super) fn connect_player(
        &mut self,
        token: EventToken,
        mixer: &rodio::mixer::Mixer,
        now_ms: u64,
    ) -> Option<rodio::Player> {
        // Preserve Player::connect_new's exact new -> add ordering. The
        // caller sets gain and appends samples afterwards on both paths.
        let (player, queue) = rodio::Player::new();
        mixer.add(self.wrap(token, queue, now_ms)?);
        Some(player)
    }

    pub(super) fn contains(&self, token: EventToken) -> bool {
        self.active_index(token).is_some()
    }

    /// Retirement must check terminal rows too, even after their slot is reused.
    pub(super) fn observed_tokens(&self) -> impl Iterator<Item = EventToken> + '_ {
        self.outputs.iter().map(|output| output.token)
    }

    /// Final read-only ownership, independent of terminal actions or PCM.
    /// Payload tokens come from SfxPlayer's existing pending/live owners,
    /// including a stale payload retained after arbiter invalidation.
    pub(super) fn snapshot(
        &mut self,
        arbiter: &SoundArbiter,
        pending: &[EventToken],
        live: &[EventToken],
    ) {
        self.live_event_count_at_finish = Some(arbiter.live_event_count());
        self.busy_channel_count_at_finish = Some(arbiter.busy_channel_count());
        for output in &mut self.outputs {
            output.event_live_at_finish = Some(arbiter.is_live_token(output.token));
            output.channel_live_at_finish = Some(arbiter.token_has_channel(output.token));
            output.token_slot_occupied_at_finish =
                Some(arbiter.event_slot_occupied(output.token.event));
            output.pending_payload_at_finish = Some(pending.contains(&output.token));
            output.live_payload_at_finish = Some(live.contains(&output.token));
        }
    }

    pub(super) fn settled(&self) -> bool {
        self.outputs.iter().all(|output| {
            output.terminal
                && output
                    .pcm
                    .as_ref()
                    .is_none_or(|pcm| pcm.ended.load(Ordering::Acquire))
                && output
                    .playback
                    .as_ref()
                    .is_none_or(|playback| !playback.snapshot().source_state_alive)
        })
    }

    #[cfg(test)]
    pub(super) fn recorded_samples(&self) -> usize {
        self.outputs
            .iter()
            .filter_map(|output| output.pcm.as_ref())
            .map(|pcm| pcm.written.load(Ordering::Acquire))
            .sum()
    }

    #[cfg(test)]
    pub(super) fn pulled_nonzero(&self) -> bool {
        self.outputs.iter().any(|output| {
            output.pcm.as_ref().is_some_and(|pcm| {
                let count = pcm.written.load(Ordering::Acquire);
                count >= 4096
                    && pcm.samples[..count]
                        .iter()
                        .any(|sample| f32::from_bits(sample.load(Ordering::Relaxed)) != 0.0)
            })
        })
    }

    pub(super) fn finish(self) -> PcmObservationReport {
        let outputs: Vec<ObservedOutput> = self
            .outputs
            .into_iter()
            .enumerate()
            .map(|(submission, output)| {
                let (sample_bits, formats, truncated) = output
                    .pcm
                    .as_ref()
                    .map_or_else(|| (Vec::new(), Vec::new(), false), |pcm| pcm.snapshot());
                let playback = output.playback.as_ref().map(PlaybackObservation::snapshot);
                let playback_truncated =
                    playback.as_ref().is_some_and(|snapshot| snapshot.truncated);
                let (resolved_samples, source_sample_count) = playback.as_ref().map_or_else(
                    || (output.samples, output.source_sample_count),
                    |snapshot| {
                        (
                            snapshot.resolved_samples.clone(),
                            snapshot.source_sample_count,
                        )
                    },
                );
                ObservedOutput {
                    submission,
                    event: output.token.event.0,
                    event_serial: output.token.serial,
                    owner: output.owner.map(|owner| match owner {
                        HandleOwner::Positional(id)
                        | HandleOwner::UnitVoice(id)
                        | HandleOwner::Coordinate(id) => id,
                        HandleOwner::Global(id) => u64::from(id),
                    }),
                    owner_role: output.owner.map(|owner| match owner {
                        HandleOwner::Positional(_) => "positional",
                        HandleOwner::UnitVoice(_) => "unit_voice",
                        HandleOwner::Coordinate(_) => "coordinate_request",
                        HandleOwner::Global(_) => "global_handle",
                    }),
                    sound_id: output.sound_id,
                    resolved_samples,
                    source_sample_count,
                    source_ended: output
                        .pcm
                        .as_ref()
                        .is_none_or(|pcm| pcm.ended.load(Ordering::Acquire)),
                    event_live_at_finish: output.event_live_at_finish,
                    channel_live_at_finish: output.channel_live_at_finish,
                    token_slot_occupied_at_finish: output.token_slot_occupied_at_finish,
                    pending_payload_at_finish: output.pending_payload_at_finish,
                    live_payload_at_finish: output.live_payload_at_finish,
                    playback,
                    actions: output.actions,
                    formats,
                    sample_bits,
                    truncated: truncated || playback_truncated,
                }
            })
            .collect();
        let playback_truncated = outputs.iter().any(|output| {
            output
                .playback
                .as_ref()
                .is_some_and(|snapshot| snapshot.truncated)
        });
        PcmObservationReport {
            outputs,
            voice_actions: self.voice_actions,
            truncated: self.truncated || playback_truncated,
            live_event_count_at_finish: self.live_event_count_at_finish,
            busy_channel_count_at_finish: self.busy_channel_count_at_finish,
            coordinate_request_count_at_finish: None,
        }
    }
}
