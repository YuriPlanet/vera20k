//! One event's start passes and playouts against the original
//! LoadSamples/SelectNextSample, PreparePlayout/AdvancePlaylist,
//! OnChannelSampleEnd and OnChannelDone executions in
//! tools/spatial_oracle/ambient_playout.json. The production arbiter decides
//! when each loader runs; its playback service is the production sample owner
//! with recorded Main draws and stub decoded clips. Each native step's draws
//! must be the ones the matching Rust step consumes.

use super::*;
use crate::rules::ini_parser::IniFile;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::OnceLock;

fn native() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/ambient_playout.json",
        ))
        .unwrap()
    })
}

const CONTROL_NAMES: [(u32, &str); 8] = [
    (control::LOOP, "loop"),
    (control::RANDOM, "random"),
    (control::ALL, "all"),
    (control::PREDELAY, "predelay"),
    (control::INTERRUPT, "interrupt"),
    (control::ATTACK, "attack"),
    (control::DECAY, "decay"),
    (control::AMBIENT, "ambient"),
];

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

/// The case's AudioEventClass fields through the production reader, with
/// sample `i` named `s{i}`. A retail section must read the same fields from
/// retail soundmd.ini.
fn case_entry(fields: &Value) -> SoundEntry {
    let flags = fields["control"].as_u64().unwrap() as u32;
    let control = CONTROL_NAMES
        .iter()
        .filter(|(bit, _)| flags & bit != 0)
        .map(|(_, name)| *name)
        .collect::<Vec<_>>()
        .join(" ");
    let sounds = (0..int(&fields["count"]))
        .map(|index| format!("s{index}"))
        .collect::<Vec<_>>()
        .join(" ");
    let registry = SoundRegistry::from_ini(&IniFile::from_str(&format!(
        "[SoundList]\n0=Case\n[Case]\nSounds={sounds}\nControl={control}\nDelay={} {}\n\
         Attack={}\nDecay={}\nLoop={}\n",
        fields["delay"][0], fields["delay"][1], fields["attack"], fields["decay"], fields["loop"],
    )));
    let entry = registry.get("Case").unwrap().clone();
    let read = |entry: &SoundEntry| {
        (
            entry.control,
            entry.delay_ms,
            entry.sounds.len() as i32,
            entry.attack,
            entry.decay,
            entry.loop_count,
        )
    };
    let expected = (
        flags,
        (int(&fields["delay"][0]), int(&fields["delay"][1])),
        int(&fields["count"]),
        int(&fields["attack"]),
        int(&fields["decay"]),
        int(&fields["loop"]),
    );
    assert_eq!(read(&entry), expected, "case fields {fields}");
    if let Some(section) = fields["section"].as_str()
        && let Some(retail) = crate::rules::retail_ini_fixture::retail_ini("soundmd.ini")
    {
        let retail = SoundRegistry::from_ini(&retail);
        assert_eq!(
            read(retail.get(section).unwrap()),
            expected,
            "retail {section}"
        );
    }
    entry
}

/// Recorded Main draws for one native step at a time.
#[derive(Default)]
struct RecordedDraws(VecDeque<(i32, i32, i32)>);

impl RecordedDraws {
    fn expect(&mut self, step: &Value) {
        assert!(self.0.is_empty(), "unconsumed draws {:?}", self.0);
        for entry in step["trace"].as_array().into_iter().flatten() {
            if entry[0] == "draw" {
                self.0
                    .push_back((int(&entry[1]), int(&entry[2]), int(&entry[3])));
            }
        }
    }

    fn consumed(&self, step: &Value) {
        assert!(
            self.0.is_empty(),
            "{} left draws {:?}",
            step["call"],
            self.0
        );
    }
}

impl SampleRng for RecordedDraws {
    fn ranged(&mut self, low: i32, high: i32) -> i32 {
        let (native_low, native_high, result) =
            self.0.pop_front().expect("native step drew no more");
        assert_eq!((low, high), (native_low, native_high), "draw bounds");
        result
    }
}

fn clip() -> DecodedAudio {
    DecodedAudio {
        samples: vec![0.25; 8],
        sample_rate: 22_050,
        channels: 2,
        native_channels: 1,
        native_sample_bytes: 2,
    }
}

/// The production sample owner for the case event; other events (the
/// declared eviction victims) play without samples.
struct CorpusService<'a> {
    entry: &'a SoundEntry,
    event: EventId,
    pending: PendingPlayback,
    steps: VecDeque<&'a Value>,
    failed: Vec<usize>,
    draws: RecordedDraws,
    playing: Option<(LoadedPlayback, PlaylistCursor)>,
}

impl<'a> CorpusService<'a> {
    fn step(&mut self, call: &str) -> &'a Value {
        let step = self
            .steps
            .pop_front()
            .unwrap_or_else(|| panic!("no native {call}"));
        assert_eq!(step["call"], call);
        self.draws.expect(step);
        step
    }

    /// A PreparePlayout native ran after a LoadSamples that left `+0xA8` zero:
    /// it returns NULL before any draw (`0x00404714`), and the Rust start pass
    /// does not run it.
    fn empty_prepare(&mut self) {
        let step = self.step("PreparePlayout");
        assert_eq!(step["buffer"], Value::Null);
        self.draws.consumed(step);
    }
}

impl PlaybackService for CorpusService<'_> {
    fn channel_acquired(&mut self, _: EventId) -> i32 {
        0
    }

    fn load_samples(&mut self, event: EventId, plays_attack: bool) -> bool {
        if event != self.event {
            return true;
        }
        let step = self.step("LoadSamples");
        let mut loads = Vec::new();
        let failed = &self.failed;
        let loaded = self.pending.load_entry_samples(
            self.entry,
            plays_attack,
            &mut self.draws,
            |name: &str| {
                let index: usize = name[1..].parse().unwrap();
                let ok = !failed.contains(&index);
                loads.push(serde_json::json!(["load", index, ok]));
                ok.then(clip)
            },
        );
        let native_loads: Vec<_> = step["trace"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry[0] == "load")
            .cloned()
            .collect();
        assert_eq!(loads, native_loads, "LoadSamples loads");
        assert_eq!(loaded, !step["loaded"].as_array().unwrap().is_empty());
        self.draws.consumed(step);
        if !loaded {
            self.empty_prepare();
        }
        loaded
    }

    fn prepare_playout(&mut self, event: EventId, plays_attack: bool) -> bool {
        if event != self.event {
            return true;
        }
        let step = self.step("PreparePlayout");
        let prepared =
            self.pending
                .prepare_entry_playout(self.entry, plays_attack, &mut self.draws);
        let buffer = prepared.then_some(self.pending.initial).flatten();
        assert_eq!(
            buffer.map(|index| index as u64),
            step["buffer"].as_u64(),
            "PreparePlayout buffer"
        );
        self.draws.consumed(step);
        prepared
    }

    fn start_playout(&mut self, event: EventId, _: i32) -> bool {
        if event == self.event {
            self.playing = Some((
                self.pending.loaded.take().unwrap(),
                self.pending.cursor.take().unwrap(),
            ));
        }
        true
    }

    fn stop_playout(&mut self, _: EventId) {}

    fn channel_volume(&self, _: EventId) -> i32 {
        0
    }
}

fn native_state(arbiter: &SoundArbiter, event: EventId) -> u64 {
    arbiter.native_event_snapshot(event).unwrap()["state"]
        .as_u64()
        .unwrap()
}

#[test]
fn start_passes_and_playouts_match_the_original_sequence() {
    let native = native();
    assert_eq!(native["lifecycles"].as_array().unwrap().len(), 16);
    for case in native["lifecycles"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let entry = case_entry(&case["entry"]);
        let mut arbiter = SoundArbiter::new(0);
        // Declared eviction victims: lower-priority one-shots that play on.
        for victim in 0..case["victims"].as_u64().unwrap() {
            let request = PlayRequest {
                key: format!("VICTIM{victim}"),
                facts: EntryFacts {
                    priority: 0,
                    limit: 0,
                    control: 0,
                    loop_count: 0,
                    delay_ms: (0, 0),
                    entry_volume_linear: VOLUME_SCALE,
                    samples_ok: true,
                    samples_resolved: true,
                },
                volume_linear: VOLUME_SCALE,
                pan: VOLUME_SCALE / 2,
            };
            arbiter.submit(&request, 0).unwrap();
        }
        let mut facts = EntryFacts::from(&entry);
        facts.priority = 4;
        let request = PlayRequest {
            key: "CASE".to_owned(),
            facts,
            volume_linear: VOLUME_SCALE,
            pan: VOLUME_SCALE / 2,
        };
        let event = arbiter.submit(&request, 0).unwrap();
        // PlayAt7509E0's handle keeps a Loop entry's leash satisfied.
        arbiter.set_loop_handle(HandleOwner::Positional(1), Some(event), "CASE");
        let token = arbiter.event_token(event).unwrap();
        let mut service = CorpusService {
            entry: &entry,
            event,
            pending: PendingPlayback::new(token, "CASE".to_owned(), SfxChannel::Sound),
            steps: case["steps"].as_array().unwrap().iter().collect(),
            failed: case["failed"]
                .as_array()
                .unwrap()
                .iter()
                .map(|index| index.as_u64().unwrap() as usize)
                .collect(),
            draws: RecordedDraws::default(),
            playing: None,
        };
        let mut now = 40;
        loop {
            let actions = arbiter.update_tick(now, &mut service);
            let stopped = actions
                .iter()
                .any(|action| matches!(action, ArbiterAction::Stop { event: e } if *e == event));
            if stopped {
                // 0x00404673's final PreparePlayout after the failed retries.
                service.empty_prepare();
                break;
            }
            let Some((loaded, mut cursor)) = service.playing.take() else {
                panic!("{name}: no playout at {now}");
            };
            // The channel's sample ends until it holds no next buffer.
            loop {
                let step = service.step("OnChannelSampleEnd");
                let native_next = step["trace"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rfind(|entry| entry[0] == "set_next")
                    .unwrap()[1]
                    .as_u64();
                let next = cursor.sample_end(&mut service.draws);
                assert_eq!(next.map(|index| index as u64), native_next, "{name}");
                service.draws.consumed(step);
                if next.is_none() {
                    break;
                }
            }
            let step = service.step("OnChannelDone");
            let end = cursor.playout_end();
            let continues = arbiter.notify_playout_ended(token, end, now);
            assert_eq!(
                native_state(&arbiter, event),
                step["state"].as_u64().unwrap(),
                "{name}: OnChannelDone state"
            );
            if !continues {
                assert!(service.steps.is_empty(), "{name}: steps after the end");
                break;
            }
            service.pending.resume_after(loaded, cursor);
            if native_state(&arbiter, event) == 2 {
                // The pre-delay holds until the clock passes now + gap.
                let gap = u64::from(end.gap_ms);
                let actions = arbiter.update_tick(now + gap, &mut service);
                assert!(
                    !actions
                        .iter()
                        .any(|action| matches!(action, ArbiterAction::Start { .. })),
                    "{name}: started inside the gap"
                );
                assert_eq!(native_state(&arbiter, event), 2, "{name}");
                service.step("Expire");
                now += gap + 1;
            } else {
                now += 40;
            }
            if service.steps.is_empty() {
                break;
            }
        }
        assert!(service.steps.is_empty(), "{name}: unconsumed native steps");
        assert!(service.draws.0.is_empty(), "{name}: unconsumed draws");
    }
}
