//! Stock YTNK low-delay Voc playback and shared GIMove cleanup controls.
//! Original7509E0 initial allocation leaves flags8 clear; only750D40
//! reallocation invokes SkipAttack4052E0.
//! Executed source: tools/input_oracle/gattling_loop (original gamemd).

use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::rng::{MainRng, SimRng, trace_draws};
use serde_json::{Value, json};
use std::fmt::Write;
use std::sync::{Mutex, OnceLock};

fn native() -> &'static Value {
    static FIXTURE: OnceLock<Value> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        serde_json::from_str(crate::test_fixture::text(
            "tools/input_oracle/gattling_loop.json",
        ))
        .unwrap()
    })
}

fn history(name: &str) -> &'static Value {
    native()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .chain(native()["shared_one_shot_controls"].as_array().unwrap())
        .find(|row| row["name"] == name)
        .unwrap()
}

fn main_hex<'a>(row: &'a Value, state: &Value) -> &'a str {
    row["complete_rng_states"][state["rng"]["main"].as_str().unwrap()]["bytes"]
        .as_str()
        .unwrap()
}

fn assert_main(main: &MainRng, row: &Value, state: &Value, label: &str) {
    assert_eq!(
        main.native_state_hex(),
        main_hex(row, state),
        "{}: {label}: complete Main object",
        row["name"]
    );
}

fn registry(reader: &Value) -> SoundRegistry {
    let mut sections = reader["inherited"]["sound_sections"].clone();
    for (name, values) in reader["sound_sections"].as_object().unwrap() {
        let section = sections
            .as_object_mut()
            .unwrap()
            .entry(name.clone())
            .or_insert_with(|| json!({}));
        section
            .as_object_mut()
            .unwrap()
            .extend(values.as_object().unwrap().clone());
    }
    let mut text = String::new();
    for (name, values) in sections.as_object().unwrap() {
        writeln!(text, "[{name}]").unwrap();
        for (key, value) in values.as_object().unwrap() {
            writeln!(text, "{key}={}", value.as_str().unwrap()).unwrap();
        }
    }
    SoundRegistry::from_ini(&IniFile::from_str(&text))
}

fn assert_reader(registry: &SoundRegistry, reader: &Value) {
    for sound in reader["sounds"]
        .as_array()
        .unwrap()
        .iter()
        .chain(reader["inherited"]["sounds"].as_array().unwrap())
    {
        let entry = registry.get(sound["name"].as_str().unwrap()).unwrap();
        let fields = &sound["fields"];
        for (name, actual) in [
            ("control", i64::from(entry.control)),
            ("type", i64::from(entry.type_flags)),
            ("limit", i64::from(entry.limit)),
            ("priority", i64::from(entry.priority)),
            ("delay_low", i64::from(entry.delay_ms.0)),
            ("delay_high", i64::from(entry.delay_ms.1)),
            ("fshift_low", i64::from(entry.fshift.0)),
            ("fshift_high", i64::from(entry.fshift.1)),
            ("vshift", i64::from(entry.vshift)),
            ("sample_count", entry.sounds.len() as i64),
        ] {
            assert_eq!(
                actual,
                fields[name].as_i64().unwrap(),
                "{} {name}",
                entry.id
            );
        }
        assert_eq!(
            i64::from(entry.volume_linear) << 16,
            fields["volume_fixed16"].as_i64().unwrap(),
            "{} volume",
            entry.id
        );
        for (name, actual) in [
            ("attack", entry.attack),
            ("decay", entry.decay),
            ("loop", entry.loop_count),
            ("range", entry.range),
        ] {
            if let Some(expected) = fields[name].as_i64() {
                assert_eq!(i64::from(actual), expected, "{} {name}", entry.id);
            }
        }
    }
}

fn bytes_from_hex(hex: &str) -> Vec<u8> {
    assert_eq!(hex.len() % 2, 0);
    (0..hex.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
        .collect()
}

/// Transport of executed mono16 PCM, without a selector or decoder port.
fn fixture_clip(name: &str) -> Option<DecodedAudio> {
    let clip = native()["physical_clips"].get(name)?;
    assert_eq!(clip["native_format"][3], 1);
    assert_eq!(clip["native_format"][4], 2);
    let pcm = bytes_from_hex(clip["pcm_hex"].as_str().unwrap());
    let samples = pcm
        .as_chunks::<2>()
        .0
        .iter()
        .flat_map(|pair| {
            let value = f32::from(i16::from_le_bytes(*pair)) / 32768.0;
            [value, value]
        })
        .collect();
    Some(DecodedAudio {
        samples,
        sample_rate: clip["rate"].as_u64().unwrap() as u32,
        channels: 2,
        native_frame_bytes: 2,
    })
}

fn mono_bytes(samples: impl Iterator<Item = f32>) -> Vec<u8> {
    let samples: Vec<_> = samples.collect();
    assert_eq!(samples.len() % 2, 0);
    samples
        .as_chunks::<2>()
        .0
        .iter()
        .flat_map(|pair| {
            assert_eq!(pair[0].to_bits(), pair[1].to_bits(), "mono upmix");
            let scaled = pair[0] * 32768.0;
            let value = scaled as i16;
            assert_eq!(scaled, f32::from(value), "lossless PCM representation");
            value.to_le_bytes()
        })
        .collect()
}

struct DrawObserver {
    main: Arc<MainRng>,
    requests: Arc<Mutex<Vec<Value>>>,
    draws: PlaybackDraws,
}

impl DrawObserver {
    fn new(row: &Value, before: &Value) -> Self {
        let main = Arc::new(MainRng::from(SimRng::from_native_state_hex_for_test(
            main_hex(row, before),
        )));
        let retained = main.draws();
        let observed_main = Arc::clone(&main);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed_requests = Arc::clone(&requests);
        let draws = PlaybackDraws::new(move |low, high| {
            let before = observed_main.native_state_hex();
            let result = retained.ranged(low, high);
            let after = observed_main.native_state_hex();
            observed_requests.lock().unwrap().push(json!({
                "args": [low, high], "result": result,
                "state_before": before, "state_after": after,
            }));
            result
        });
        Self {
            main,
            requests,
            draws,
        }
    }

    fn assert_phase(&self, row: &Value, boundary: &Value, raw: Vec<Value>) {
        let label = boundary["label"].as_str().unwrap();
        // The Unit/report prerequisite is supplied by the native boundary.
        // Observe every range in the actual4041D0/404700/4047B0 audio chain.
        let expected: Vec<_> = boundary["requests"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|request| (0x404000..0x406000).contains(&request["site"].as_u64().unwrap()))
            .map(|request| {
                assert_eq!(request["this"], 0x886b88_u64, "native Main owner");
                json!({
                    "args": request["args"], "result": request["result"],
                    "state_before": request["state_before"],
                    "state_after": request["state_after"],
                })
            })
            .collect();
        assert_eq!(
            std::mem::take(&mut *self.requests.lock().unwrap()),
            expected,
            "{}: {label}: range requests/results/full states",
            row["name"]
        );
        let direct_raw: Vec<_> = boundary["requests"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|request| {
                request["entry"] == 0x65c780_u64
                    && request["this"] == 0x886b88_u64
                    && request["args"].as_array().unwrap().is_empty()
            })
            .map(|request| &request["result"])
            .collect();
        let expected_raw = if direct_raw.is_empty() {
            boundary["advances"]
                .as_array()
                .unwrap()
                .iter()
                .map(|draw| &draw["raw"])
                .collect::<Vec<_>>()
        } else {
            // This corpus's one standalone report call is logged as a
            // Random65C780 request, not an internal ranged-rejection word.
            assert!(boundary["advances"].as_array().unwrap().is_empty());
            direct_raw
        };
        assert_eq!(
            raw.iter().map(|draw| &draw["value"]).collect::<Vec<_>>(),
            expected_raw,
            "{}: {label}: every raw word, including direct report and rejection",
            row["name"]
        );
        assert_main(&self.main, row, &boundary["after"], label);
    }
}

/// Transport of the supplied Unit tail's report prerequisite into its
/// existing GattlingState owner. It consumes the same process Main cell;
/// the body, modulo pick and report latch are not reimplemented here.
fn apply_native_stage_report(
    row: &Value,
    boundary: &Value,
    reader: &Value,
    observer: &DrawObserver,
) {
    use crate::rules::gattling_type::GattlingStages;
    use crate::sim::combat::gattling::GattlingState;

    let before = &boundary["before"]["gattling"];
    let after = &boundary["after"]["gattling"];
    let fields: Vec<i32> = reader["layers"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find_map(|layer| layer["thresholds_rates"].as_array())
        .unwrap()
        .iter()
        .map(|field| field.as_i64().unwrap() as i32)
        .collect();
    let table = GattlingStages::from_fields(
        fields[0],
        fields[1..7].try_into().unwrap(),
        fields[7..13].try_into().unwrap(),
        fields[13],
        fields[14],
    );
    let mut state = GattlingState::from_fields(
        before["stage"].as_i64().unwrap() as i32,
        before["value"].as_i64().unwrap() as i32,
        before["latch"].as_i64().unwrap() != 0,
    );
    let label = boundary["label"].as_str().unwrap();
    let call = row["ordered_calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|call| call["phase"] == label && call["kind"] == "GattlingIncrease")
        .unwrap();
    let request = boundary["requests"].as_array().unwrap();
    assert_eq!(request.len(), 1, "one original stage report draw");
    let request = &request[0];
    assert_eq!(request["entry"], 0x65c780_u64);
    assert_eq!(request["site"], 0x70dfc6_u64);
    assert_eq!(request["this"], 0x886b88_u64);
    assert!(request["args"].as_array().unwrap().is_empty());
    // The selected stock stage1 ground weapon is AGGattling2. Supply its
    // Report vector from the original selected WeaponType reader output.
    let reports = reader["weapons"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|weapon| weapon["weapon"] == "AGGattling2")
        .unwrap()["reports"]
        .as_array()
        .unwrap();
    let veterancy = u32::from_le_bytes(
        bytes_from_hex(before["veterancy_hex"].as_str().unwrap())
            .try_into()
            .unwrap(),
    );
    let elite = crate::sim::combat::veterancy::rank_of(
        crate::util::native_x87::NativeF32Bits::from_bits(veterancy),
    ) == crate::sim::combat::veterancy::VeterancyRank::Elite;
    assert!(!elite, "recorded stock stage1 base weapon prerequisite");
    let retained = observer.main.draws();
    let effects = state.increase(
        &table,
        elite,
        call["args"][0].as_i64().unwrap() as i32,
        |index| {
            assert_eq!(index, 2, "original stage1 ground weapon slot");
            Some(reports.len() as i32)
        },
        || {},
        || {
            assert_eq!(observer.main.native_state_hex(), request["state_before"]);
            let word = retained.next_u32();
            assert_eq!(u64::from(word), request["result"].as_u64().unwrap());
            assert_eq!(observer.main.native_state_hex(), request["state_after"]);
            word
        },
    );
    assert!(effects.stage_up);
    assert_eq!(i64::from(state.stage()), after["stage"].as_i64().unwrap());
    assert_eq!(i64::from(state.value()), after["value"].as_i64().unwrap());
    assert_eq!(state.report_latch(), after["latch"].as_i64().unwrap() != 0);
    let report = effects.report.unwrap();
    let play = row["ordered_calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|call| call["phase"] == label && call["kind"] == "PlayAtPosition")
        .unwrap();
    assert_eq!(reports[report.item as usize], play["this"]);
    assert_eq!(report.weapon_index, 2);
}

struct HeadlessOutput {
    source: RingSource,
    control: PlaybackControl,
    backend: u64,
    // Native compact slots are diagnostic identities, never selection state.
    compact_slots: BTreeMap<usize, usize>,
    names: Vec<String>,
}

impl HeadlessOutput {
    fn pull_quarter(&mut self) -> usize {
        let quarter = self.source.native_snapshot()["quarter_samples"]
            .as_u64()
            .unwrap() as usize;
        self.source.by_ref().take(quarter).count()
    }

    fn assert_fill(&self, main: &MainRng, row: &Value, fill: &Value) {
        let snapshot = self.source.native_snapshot();
        let pcm = mono_bytes(
            snapshot["ring"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32),
        );
        assert_eq!(
            pcm.len() as u64,
            fill["after"]["ring_bytes"].as_u64().unwrap()
        );
        assert_eq!(snapshot["quarter_samples"], fill["after"]["quantum_bytes"]);
        assert_eq!(
            crate::util::sha256::sha256_hex(&pcm),
            fill["ring_sha256"].as_str().unwrap(),
            "{}: {}: full original ring",
            row["name"],
            fill["phase"]
        );
        assert_main(
            main,
            row,
            &json!({"rng": fill["rng_after"]}),
            fill["phase"].as_str().unwrap(),
        );
    }

    fn assert_cursor(&self, event: &Value, label: &str) {
        let snapshot = self.source.native_snapshot();
        let compact_remaining: Vec<_> = snapshot["remaining"]
            .as_array()
            .unwrap()
            .iter()
            .map(|index| self.compact_slots[&(index.as_u64().unwrap() as usize)])
            .collect();
        assert_eq!(
            json!(compact_remaining),
            event["playlist"],
            "{label}: compact playlist"
        );
        assert_eq!(
            snapshot["iteration"], event["loop_count"],
            "{label}: body pass"
        );
        let flags = event["flags"].as_u64().unwrap();
        assert_eq!(
            snapshot["no_replay"],
            flags & 0x20 != 0,
            "{label}: native flags20"
        );
        assert_eq!(
            snapshot["decay_used"],
            flags & 0x10 != 0,
            "{label}: native flags10"
        );
        if let Some(pointer) = event["driver"]["source_buffer"].as_u64() {
            let current_name = snapshot["current"]
                .as_u64()
                .map(|index| self.names[index as usize].as_str());
            let native_name = event["loaded_sample_identities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|clip| clip["pointer"].as_u64().unwrap() + 28 == pointer)
                .map(|clip| clip["name"].as_str().unwrap());
            assert_eq!(
                current_name, native_name,
                "{label}: actual current cached sample"
            );
            assert!(
                pointer == 0 || native_name.is_some(),
                "{label}: represented native source"
            );
        }
    }
}

/// Drive production buffer owners at the supplied native worker boundaries.
/// Clock/cursor/world priors and event admission/retirement remain fixture
/// inputs; the arbiter integration tests independently exercise those owners.
fn replay(name: &str) {
    let row = history(name);
    assert_eq!(row["original_code_unchanged"], true);
    let reader = if name == "declared_Loop3_original_reader_release_and_budget_control" {
        &native()["counted_control_reader"]
    } else {
        &native()["retail"]
    };
    let registry = registry(reader);
    assert_reader(&registry, reader);
    let boundaries = row["boundaries"].as_array().unwrap();
    let first = boundaries
        .iter()
        .position(|b| b["label"] == "audio_pump_1034")
        .unwrap();
    let mut observer = DrawObserver::new(row, &boundaries[first]["before"]);
    let dropped_slot = row["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["kind"] == "declared_unresolved_AudioIndex_component")
        .map(|input| {
            assert_eq!(input["physical_list_slot"], 2);
            registry
                .get(input["sound_name"].as_str().unwrap())
                .unwrap()
                .sounds[2]
                .clone()
        });
    let mut outputs: BTreeMap<u64, HeadlessOutput> = BTreeMap::new();
    let mut cancelled = Vec::new();
    let mut observations = Vec::new();
    let mut fill_count = 0;

    for boundary in &boundaries[first..] {
        let label = boundary["label"].as_str().unwrap();
        assert_main(&observer.main, row, &boundary["before"], label);
        let (_, raw) = trace_draws(|| {
            let fills: Vec<_> = row["native_ring_fills"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|fill| fill["phase"] == label)
                .collect();
            if label.starts_with("audio_pump_") {
                let loads: Vec<_> = row["ordered_calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|call| call["kind"] == "LoadSamples" && call["phase"] == label)
                    .collect();
                let prepares: Vec<_> = row["native_sites"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|site| {
                        site["phase"] == label
                            && matches!(site["pc"].as_u64(), Some(0x4045d6 | 0x404678))
                    })
                    .collect();
                assert_eq!(prepares.len(), loads.len() * 2);
                // Original admission draws both ready events' shifts before
                // the per-event Load/Prepare/DriverStart sequence.
                let mut shifts = BTreeMap::new();
                for event in boundary["before"]["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|event| event["state"] == 0)
                {
                    let entry = registry.get(event["sound_name"].as_str().unwrap()).unwrap();
                    shifts.insert(
                        event["pointer"].as_u64().unwrap(),
                        PlayShifts::draw(entry, &mut observer.draws),
                    );
                }
                for (ordinal, load) in loads.iter().enumerate() {
                    let pointer = load["this"].as_u64().unwrap();
                    let event = boundary["before"]["events"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|event| event["pointer"] == pointer)
                        .unwrap();
                    let entry = registry.get(event["sound_name"].as_str().unwrap()).unwrap();
                    let loaded = LoadedPlayback::load(entry, &mut observer.draws, |name| {
                        if dropped_slot.as_deref() == Some(name) {
                            None
                        } else {
                            fixture_clip(name)
                        }
                    })
                    .unwrap();
                    let compact_slots: BTreeMap<_, _> = loaded
                        .clips
                        .keys()
                        .copied()
                        .enumerate()
                        .map(|(compact, index)| (index, compact))
                        .collect();
                    let mut cursor = PlaylistCursor::new(
                        loaded.selected.clone(),
                        entry.control,
                        entry.delay_ms,
                        entry.loop_count,
                    );
                    let mut initial = None;
                    for site in &prepares[ordinal * 2..ordinal * 2 + 2] {
                        initial = cursor.prepare(
                            &mut observer.draws,
                            event["flags"].as_u64().unwrap() & 8 == 0,
                        );
                        let prepared = site["state"]["events"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|event| event["pointer"] == pointer)
                            .unwrap();
                        let native_clips = prepared["loaded_sample_identities"].as_array().unwrap();
                        let names: Vec<_> = loaded
                            .clips
                            .keys()
                            .map(|&index| &entry.sounds[index])
                            .collect();
                        assert_eq!(
                            json!(names),
                            json!(
                                native_clips
                                    .iter()
                                    .map(|clip| &clip["name"])
                                    .collect::<Vec<_>>()
                            ),
                            "{label}: compact successful loads"
                        );
                        let native_selected = native_clips
                            .iter()
                            .find(|clip| {
                                clip["pointer"].as_u64().unwrap() + 28
                                    == site["eax"].as_u64().unwrap()
                            })
                            .unwrap();
                        assert_eq!(
                            entry.sounds[initial.unwrap()],
                            native_selected["name"].as_str().unwrap(),
                            "{label}: Prepare{:X}",
                            site["pc"].as_u64().unwrap()
                        );
                        assert_main(&observer.main, row, &site["state"], "Prepare404700 return");
                    }
                    let fill = fills[ordinal];
                    assert_main(
                        &observer.main,
                        row,
                        &json!({"rng": fill["rng_before"]}),
                        label,
                    );
                    let rate = shifts[&pointer]
                        .shifted_sample_rate(loaded.clips[&initial.unwrap()].sample_rate);
                    let (source, control) = RingSource::new(
                        loaded,
                        cursor,
                        initial.unwrap(),
                        observer.draws.clone(),
                        rate,
                        PAN_CENTRE,
                        Some(&entry.sounds),
                    )
                    .unwrap();
                    observations.push(source.observation().unwrap());
                    let mut output = HeadlessOutput {
                        source,
                        control,
                        backend: fill["backend"].as_u64().unwrap(),
                        compact_slots,
                        names: entry.sounds.clone(),
                    };
                    output.assert_fill(&observer.main, row, fill);
                    assert_eq!(
                        output.pull_quarter(),
                        output.source.native_snapshot()["quarter_samples"]
                            .as_u64()
                            .unwrap() as usize,
                        "first quarter has PCM"
                    );
                    assert!(outputs.insert(pointer, output).is_none());
                    fill_count += 1;
                }
                if loads.is_empty() {
                    for fill in &fills {
                        let output = outputs
                            .values_mut()
                            .find(|output| output.backend == fill["backend"].as_u64().unwrap())
                            .unwrap();
                        assert_main(
                            &observer.main,
                            row,
                            &json!({"rng": fill["rng_before"]}),
                            label,
                        );
                        assert!(
                            output.control.release_to_decay(),
                            "{label}: actual decay ring"
                        );
                        output.assert_fill(&observer.main, row, fill);
                        assert!(output.pull_quarter() > 0);
                        fill_count += 1;
                    }
                } else {
                    assert_eq!(loads.len(), fills.len());
                }
                let live: BTreeSet<_> = boundary["after"]["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|event| event["pointer"].as_u64().unwrap())
                    .collect();
                outputs.retain(|pointer, _| live.contains(pointer));
            } else if label.starts_with("original_fill_") {
                assert_eq!(fills.len(), 1);
                let fill = fills[0];
                let output = outputs
                    .values()
                    .find(|output| output.backend == fill["backend"].as_u64().unwrap())
                    .unwrap();
                let ring = fill["before"]["ring_bytes"].as_u64().unwrap();
                let start = (fill["before"]["write_offset"].as_u64().unwrap() % ring) as usize;
                output.source.fill_capacity_for_test(
                    start,
                    fill["requested_bytes"].as_u64().unwrap() as usize,
                );
                output.assert_fill(&observer.main, row, fill);
                fill_count += 1;
            } else if label.starts_with("original_worker_") {
                for fill in &fills {
                    let output = outputs
                        .values_mut()
                        .find(|output| output.backend == fill["backend"].as_u64().unwrap())
                        .unwrap();
                    assert_main(
                        &observer.main,
                        row,
                        &json!({"rng": fill["rng_before"]}),
                        label,
                    );
                    output.pull_quarter();
                    output.assert_fill(&observer.main, row, fill);
                    fill_count += 1;
                }
                for event in boundary["after"]["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|event| event["state"] == 4)
                {
                    if let Some(output) = outputs.get_mut(&event["pointer"].as_u64().unwrap()) {
                        assert_eq!(
                            output.source.next(),
                            None,
                            "{label}: native worker endpoint"
                        );
                    }
                }
            } else if label.contains("no_target_") {
                for event in boundary["after"]["events"].as_array().unwrap() {
                    if let Some(output) = outputs.get(&event["pointer"].as_u64().unwrap()) {
                        // Native flags20 is the supplied Release/Detach
                        // decision; this exercises the real boundary gate.
                        output
                            .control
                            .update_no_replay(|| event["flags"].as_u64().unwrap() & 0x20 != 0);
                    }
                }
            } else if label == "original_Unit_tail_hard_stage_transition" {
                let pointer = boundary["before"]["events"][0]["pointer"].as_u64().unwrap();
                let mut old = outputs.remove(&pointer).unwrap();
                old.control.cancel();
                assert_eq!(old.source.next(), None, "hard stop rejects cached PCM");
                assert_eq!(old.source.native_snapshot()["cancelled"], true);
                cancelled.push(old);
                // Native70DF79 hard-stops before70DFC6 draws the new
                // report. Delegate that prerequisite to the shared owner.
                apply_native_stage_report(row, boundary, reader, &observer);
            }
            for event in boundary["after"]["events"].as_array().unwrap() {
                if let Some(output) = outputs.get(&event["pointer"].as_u64().unwrap()) {
                    output.assert_cursor(event, label);
                }
            }
            // Retain the stopped old source across the new stage's real
            // draws, proving it cannot spend any later Main words.
            for old in &mut cancelled {
                assert_eq!(old.pull_quarter(), 0);
                assert!(!old.control.release_to_decay());
            }
        });
        observer.assert_phase(row, boundary, raw);
    }
    assert_eq!(
        fill_count,
        row["native_ring_fills"].as_array().unwrap().len(),
        "every native fill consumed once"
    );
    assert!(outputs.is_empty(), "native service retired all outputs");
    drop(cancelled);
    assert!(
        observations
            .iter()
            .all(|observation| !observation.snapshot().source_state_alive),
        "retirement releases the production ring and PCM owners"
    );
}

#[test]
fn native_initial_attack_body_soft_release_ring_and_main() {
    replay("stock_initial_audible_loop1_soft_release");
}

#[test]
fn native_reallocation_compares_both_body_preparations() {
    replay("stock_inaudible_then_loop1_reallocation");
}

#[test]
fn native_exact_source_end_waits_for_next_positive_capacity_fill() {
    replay("stock_loop1_exact_source_end_fill_control");
}

#[test]
fn native_hard_stage_stop_cancels_old_ring_before_loop2() {
    replay("stock_loop1_hard_stage_stop_then_loop2_control");
}

fn recorded_boundary<'a>(row: &'a Value, label: &str) -> &'a Value {
    row["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|boundary| boundary["label"] == label)
        .unwrap()
}

fn native_gattling_sound_facts(row: &Value, boundary: &Value) -> Vec<&'static str> {
    row["ordered_calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|call| call["phase"] == boundary["label"])
        .filter_map(|call| match call["kind"].as_str() {
            // These returns follow the two +4A4 Stop calls, at70DF74 and
            //70DF9C. The quiet +4C0 handle's70DF07 call is a different owner.
            Some("HandleStopClear")
                if matches!(call["return_pc"].as_u64(), Some(0x70df79 | 0x70dfa1)) =>
            {
                Some("stop")
            }
            Some("HandleRelease") if call["return_pc"] == 0x70e013_u64 => Some("release"),
            Some("PlayAtPosition") if call["return_pc"] == 0x70dfe2_u64 => Some("loop"),
            _ => None,
        })
        .collect()
}

/// Fixture PCM transport through the actual load, playlist and ring owners,
/// using the Simulation's retained Main capability rather than another RNG.
fn start_sim_ring(
    sim: &crate::sim::world::Simulation,
    row: &Value,
    boundary: &Value,
    entry: &SoundEntry,
) -> HeadlessOutput {
    assert_main(
        sim.main_rng(),
        row,
        &boundary["before"],
        "before ring start",
    );
    let retained = sim.presentation_main_draws();
    let mut draws = PlaybackDraws::new(move |low, high| retained.ranged(low, high));
    let shifts = PlayShifts::draw(entry, &mut draws);
    let loaded = LoadedPlayback::load(entry, &mut draws, fixture_clip).unwrap();
    let compact_slots = loaded
        .clips
        .keys()
        .copied()
        .enumerate()
        .map(|(compact, index)| (index, compact))
        .collect();
    let mut cursor = PlaylistCursor::new(
        loaded.selected.clone(),
        entry.control,
        entry.delay_ms,
        entry.loop_count,
    );
    cursor.prepare(&mut draws, true).unwrap();
    let initial = cursor.prepare(&mut draws, true).unwrap();
    let rate = shifts.shifted_sample_rate(loaded.clips[&initial].sample_rate);
    let (source, control) = RingSource::new(
        loaded,
        cursor,
        initial,
        draws,
        rate,
        PAN_CENTRE,
        Some(&entry.sounds),
    )
    .unwrap();
    let fill = row["native_ring_fills"]
        .as_array()
        .unwrap()
        .iter()
        .find(|fill| fill["phase"] == boundary["label"])
        .unwrap();
    let mut output = HeadlessOutput {
        source,
        control,
        backend: fill["backend"].as_u64().unwrap(),
        compact_slots,
        names: entry.sounds.clone(),
    };
    output.assert_fill(sim.main_rng(), row, fill);
    assert!(output.pull_quarter() > 0, "retained actual attack PCM");
    assert_main(sim.main_rng(), row, &boundary["after"], "after ring start");
    output
}

/// A device-free boundary consumer: it records facts and calls only the real
/// retained source's controls when Simulation reaches Stop/Release. It neither
/// chooses a sample nor pre-cancels the source. SfxPlayer's device transport and
/// arbiter retirement are independently covered by their owner regressions.
struct RecordingBoundary<'a> {
    delivered: std::cell::Cell<usize>,
    calls: std::cell::Cell<usize>,
    source: std::cell::RefCell<HeadlessOutput>,
    owner: u64,
    entity: u64,
    row: &'a Value,
    boundary: &'a Value,
    release: bool,
}

impl crate::sim::world::SoundBoundary for RecordingBoundary<'_> {
    fn consume(
        &self,
        sim: &crate::sim::world::Simulation,
        _rules: &crate::rules::ruleset::RuleSet,
        events: &[crate::sim::world::SimSoundEvent],
    ) {
        use crate::sim::world::SimSoundEvent;
        assert!(self.delivered.get() <= events.len());
        for event in &events[self.delivered.get()..] {
            let reached = match event {
                SimSoundEvent::GattlingLoopStop { owner } => {
                    assert!(!self.release, "soft boundary must not hard stop");
                    assert_eq!(*owner, self.owner);
                    true
                }
                SimSoundEvent::GattlingLoopRelease { owner } => {
                    assert!(self.release, "hard boundary must not release");
                    assert_eq!(*owner, self.owner);
                    true
                }
                _ => false,
            };
            if !reached {
                continue;
            }
            self.calls.set(self.calls.get() + 1);
            assert_main(
                sim.main_rng(),
                self.row,
                &self.boundary["before"],
                "native Stop/Release before later Main report",
            );
            let mut output = self.source.borrow_mut();
            if self.release {
                // Original Update70E000 reaches Release406060 before clearing
                // the report latch or subtracting RateDown from the value.
                let before = &self.boundary["before"]["gattling"];
                let state = sim.entities().get(self.entity).unwrap().gattling;
                assert_eq!(i64::from(state.stage()), before["stage"].as_i64().unwrap());
                assert_eq!(i64::from(state.value()), before["value"].as_i64().unwrap());
                assert_eq!(state.report_latch(), before["latch"].as_i64().unwrap() != 0);
                output.control.update_no_replay(|| true);
                assert_eq!(output.source.native_snapshot()["no_replay"], true);
                assert_eq!(output.source.native_snapshot()["cancelled"], false);
            } else {
                output.control.cancel();
                let quarter = output.source.native_snapshot()["quarter_samples"]
                    .as_u64()
                    .unwrap() as usize;
                assert_eq!(
                    output.source.by_ref().take(quarter + 2).count(),
                    0,
                    "reached Stop must prevent cached PCM and later refill draws",
                );
                assert!(
                    !output.control.release_to_decay(),
                    "old Stop cannot turn into decay"
                );
            }
            assert_main(
                sim.main_rng(),
                self.row,
                &self.boundary["before"],
                "control has no Main draw",
            );
        }
        self.delivered.set(events.len());
    }

    fn delivered(&self) -> usize {
        self.delivered.get()
    }
}

#[test]
fn production_gattling_stop_and_release_reach_retained_source_at_native_boundaries() {
    use crate::sim::combat::gattling::{GattlingState, gattling_sound_owner};
    use crate::sim::world::{FrameEffects, SimSoundEvent, Simulation};

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let row = history("stock_loop1_hard_stage_stop_then_loop2_control");
    let registry = registry(&native()["retail"]);
    assert_reader(&registry, &native()["retail"]);
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object("YTNK", "Americans", 13, 14, 0, rules)
        .unwrap();
    let owner = gattling_sound_owner(id);
    let initial = recorded_boundary(row, "original_admitted_Unit_fire_tail");
    *sim.main_rng_mut_for_test() =
        SimRng::from_native_state_hex_for_test(main_hex(row, &initial["before"])).into();
    sim.entities_mut_for_test().get_mut(id).unwrap().gattling =
        GattlingState::from_fields(0, 0, false);
    let (_, raw) = trace_draws(|| sim.gattling_increase(id, rules, 1, FrameEffects::default()));
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0]["value"], initial["requests"][0]["result"]);
    assert_main(
        sim.main_rng(),
        row,
        &initial["after"],
        "initial production report",
    );
    let initial_reports: Vec<_> = sim
        .sound_events()
        .iter()
        .filter(|event| matches!(event, SimSoundEvent::GattlingLoop { .. }))
        .collect();
    assert_eq!(
        initial_reports.len(),
        1,
        "sole initial report without a sink"
    );
    assert_eq!(
        initial_reports.len(),
        native_gattling_sound_facts(row, initial)
            .iter()
            .filter(|&&fact| fact == "loop")
            .count()
    );
    assert!(
        matches!(initial_reports[0], SimSoundEvent::GattlingLoop { owner: actual, sound_id, .. }
        if *actual == owner && sound_id == "GattlingGunAttackLoop1")
    );
    let old = start_sim_ring(
        &sim,
        row,
        recorded_boundary(row, "audio_pump_1034"),
        registry.get("GattlingGunAttackLoop1").unwrap(),
    );
    let threshold = recorded_boundary(row, "original_Increase_threshold_component_ticks199");
    let (_, raw) = trace_draws(|| sim.gattling_increase(id, rules, 199, FrameEffects::default()));
    assert!(
        raw.is_empty(),
        "original retained latch crosses no new report"
    );
    assert_main(
        sim.main_rng(),
        row,
        &threshold["after"],
        "threshold component",
    );
    let transition = recorded_boundary(row, "original_Unit_tail_hard_stage_transition");
    let native_transition_facts = native_gattling_sound_facts(row, transition);
    let transition_prefix = sim.sound_events().len();
    let hard = RecordingBoundary {
        delivered: std::cell::Cell::new(transition_prefix),
        calls: std::cell::Cell::new(0),
        source: std::cell::RefCell::new(old),
        owner,
        entity: id,
        row,
        boundary: transition,
        release: false,
    };
    let (_, raw) =
        trace_draws(|| sim.gattling_increase(id, rules, 1, FrameEffects::for_sound(&hard)));
    assert_eq!(
        hard.calls.get(),
        native_transition_facts
            .iter()
            .filter(|&&fact| fact == "stop")
            .count(),
        "production increase must reach both native Stops before drawing its report"
    );
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0]["value"], transition["requests"][0]["result"]);
    assert_main(
        sim.main_rng(),
        row,
        &transition["after"],
        "production stage1 report",
    );
    assert_eq!(
        sim.sound_events()[transition_prefix..]
            .iter()
            .map(|event| match event {
                SimSoundEvent::GattlingLoopStop { owner: stopped } => {
                    assert_eq!(*stopped, owner);
                    "stop"
                }
                SimSoundEvent::GattlingLoop {
                    owner: started,
                    sound_id,
                    ..
                } => {
                    assert_eq!(*started, owner);
                    assert_eq!(sound_id, "GattlingGunAttackLoop2");
                    "loop"
                }
                other => panic!("unexpected production stage-transition fact: {other:?}"),
            })
            .collect::<Vec<_>>(),
        native_transition_facts,
        "native Stop/Stop/Loop2 order after the retained prefix"
    );
    let state = sim.entities().get(id).unwrap().gattling;
    assert_eq!(
        i64::from(state.stage()),
        transition["after"]["gattling"]["stage"].as_i64().unwrap()
    );
    assert_eq!(
        i64::from(state.value()),
        transition["after"]["gattling"]["value"].as_i64().unwrap()
    );
    assert_eq!(
        state.report_latch(),
        transition["after"]["gattling"]["latch"].as_i64().unwrap() != 0
    );
    let new = start_sim_ring(
        &sim,
        row,
        recorded_boundary(row, "audio_pump_1094"),
        registry.get("GattlingGunAttackLoop2").unwrap(),
    );
    assert_eq!(
        hard.source.borrow_mut().pull_quarter(),
        0,
        "old source stays cancelled across real Loop2 draws"
    );
    let release = recorded_boundary(row, "original_Unit_no_target_soft_release");
    let soft = RecordingBoundary {
        delivered: std::cell::Cell::new(sim.sound_events().len()),
        calls: std::cell::Cell::new(0),
        source: std::cell::RefCell::new(new),
        owner,
        entity: id,
        row,
        boundary: release,
        release: true,
    };
    let (_, raw) =
        trace_draws(|| sim.gattling_update(id, rules, 1, FrameEffects::for_sound(&soft)));
    assert_eq!(
        soft.calls.get(),
        native_gattling_sound_facts(row, release)
            .iter()
            .filter(|&&fact| fact == "release")
            .count(),
        "production decay must reach Release before changing the owner"
    );
    assert!(raw.is_empty());
    assert_main(
        sim.main_rng(),
        row,
        &release["after"],
        "production soft release",
    );
    let state = sim.entities().get(id).unwrap().gattling;
    let after = &release["after"]["gattling"];
    assert_eq!(i64::from(state.stage()), after["stage"].as_i64().unwrap());
    assert_eq!(i64::from(state.value()), after["value"].as_i64().unwrap());
    assert_eq!(state.report_latch(), after["latch"].as_i64().unwrap() != 0);
}

#[test]
fn production_limbo_preserves_frame_sound_scope_without_local_rules() {
    use crate::sim::combat::gattling::{GattlingState, gattling_sound_owner};
    use crate::sim::world::{ConcealOutcome, FrameEffects, Simulation, UninitContext};

    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let row = history("stock_loop1_hard_stage_stop_then_loop2_control");
    let registry = registry(&native()["retail"]);
    let playback = recorded_boundary(row, "audio_pump_1094");
    let release = recorded_boundary(row, "original_Unit_no_target_soft_release");
    let mut sim = Simulation::new();
    let id = sim
        .spawn_object("YTNK", "Americans", 13, 14, 0, rules)
        .unwrap();
    let before = &release["before"]["gattling"];
    sim.entities_mut_for_test().get_mut(id).unwrap().gattling = GattlingState::from_fields(
        before["stage"].as_i64().unwrap() as i32,
        before["value"].as_i64().unwrap() as i32,
        before["latch"].as_i64().unwrap() != 0,
    );
    *sim.main_rng_mut_for_test() =
        SimRng::from_native_state_hex_for_test(main_hex(row, &playback["before"])).into();
    let source = start_sim_ring(
        &sim,
        row,
        playback,
        registry.get("GattlingGunAttackLoop2").unwrap(),
    );
    let boundary = RecordingBoundary {
        delivered: std::cell::Cell::new(sim.sound_events().len()),
        calls: std::cell::Cell::new(0),
        source: std::cell::RefCell::new(source),
        owner: gattling_sound_owner(id),
        entity: id,
        row,
        boundary: release,
        release: true,
    };
    // Rust lifecycle regression: recorded Loop2/Main inputs and real PCM
    // controls, through a rules-free nested context. Limbo6F6C6B's Release
    // before6F6C76 is instruction-established; this is not an executed native
    // comparison of the complete Limbo transaction.
    let effects = FrameEffects::for_sound(&boundary).with_rules(Some(rules));
    let context = UninitContext::new(None, None).with_effects(effects);
    assert!(context.rules().is_none());
    let (outcome, raw) = trace_draws(|| sim.techno_limbo_with_context(id, context));
    assert_eq!(outcome, ConcealOutcome::Concealed);
    assert!(raw.is_empty());
    assert_eq!(boundary.calls.get(), 1);
    let entity = sim.entities().get(id).unwrap();
    assert!(entity.lifecycle.in_limbo());
    assert!(!entity.gattling.report_latch());
    assert_eq!(
        i64::from(entity.gattling.stage()),
        before["stage"].as_i64().unwrap()
    );
    assert_eq!(
        i64::from(entity.gattling.value()),
        before["value"].as_i64().unwrap()
    );
}

#[test]
fn native_failed_middle_load_compacts_successful_physical_slots() {
    replay("nonretail_unresolved_middle_index_native_slot_control");
}

#[test]
fn native_initial_loop_ring_precedes_same_service_gi_sample_draw() {
    replay("stock_loop1_then_GIMove_same_service_order_control");
}

#[test]
fn native_counted_loop_release_retains_three_body_passes_then_decay() {
    replay("declared_Loop3_original_reader_release_and_budget_control");
}

/// Fixture PCM and a headless device transport around the production payload,
/// cursor and source owners. Admission, Release/Detach and retirement are
/// decisions of the actual SoundArbiter, not supplied fixture flags.
struct OneShotService<'a> {
    entry: &'a SoundEntry,
    main: &'a MainRng,
    draws: PlaybackDraws,
    pending: Option<PendingPlayback>,
    output: Option<HeadlessOutput>,
    compact_slots: BTreeMap<usize, usize>,
    preparations: Vec<Value>,
    observation: Option<playback::PlaybackObservation>,
    delivered: Vec<f32>,
    stops: usize,
}

impl OneShotService<'_> {
    fn pull_quarter(&mut self) -> usize {
        let output = self.output.as_mut().unwrap();
        let quarter = output.source.native_snapshot()["quarter_samples"]
            .as_u64()
            .unwrap() as usize;
        let pcm: Vec<_> = output.source.by_ref().take(quarter).collect();
        let count = pcm.len();
        self.delivered.extend(pcm);
        count
    }
}

impl PlaybackService for OneShotService<'_> {
    fn channel_acquired(&mut self, event: EventId) -> i32 {
        let pending = self.pending.as_mut().unwrap();
        assert_eq!(event, pending.token.event);
        let shifts = PlayShifts::draw(self.entry, &mut self.draws);
        pending.shifts = Some(shifts);
        shifts.predelay_ms
    }

    fn load_samples(&mut self, event: EventId, plays_attack: bool) -> bool {
        let pending = self.pending.as_mut().unwrap();
        assert_eq!(event, pending.token.event);
        pending.load_entry_samples(self.entry, plays_attack, &mut self.draws, fixture_clip)
    }

    fn prepare_playout(&mut self, event: EventId, plays_attack: bool) -> bool {
        let pending = self.pending.as_mut().unwrap();
        assert_eq!(event, pending.token.event);
        let prepared = pending.prepare_entry_playout(self.entry, plays_attack, &mut self.draws);
        let loaded = pending.loaded.as_ref().unwrap();
        self.compact_slots = loaded
            .clips
            .keys()
            .copied()
            .enumerate()
            .map(|(compact, index)| (index, compact))
            .collect();
        self.preparations.push(json!({
            "sample": pending.initial.map(|index| &self.entry.sounds[index]),
            "loaded": loaded.clips.keys().map(|&index| &self.entry.sounds[index]).collect::<Vec<_>>(),
            "main": self.main.native_state_hex(),
        }));
        prepared
    }

    fn start_playout(&mut self, event: EventId, pan: i32) -> bool {
        let pending = self.pending.as_mut().unwrap();
        assert_eq!(event, pending.token.event);
        let loaded = pending.loaded.take().unwrap();
        let initial = pending.initial.unwrap();
        let rate = pending
            .shifts
            .unwrap()
            .shifted_sample_rate(loaded.clips[&initial].sample_rate);
        pending.source = RingSource::new(
            loaded,
            pending.cursor.take().unwrap(),
            initial,
            self.draws.clone(),
            rate,
            pan,
            Some(&self.entry.sounds),
        );
        pending.source.is_some()
    }

    fn stop_playout(&mut self, event: EventId) {
        if let Some(output) = &self.output {
            output.control.cancel();
            self.stops += 1;
        }
        if let Some(pending) = &self.pending {
            assert_eq!(event, pending.token.event);
            if let Some((_, control)) = &pending.source {
                control.cancel();
                self.stops += 1;
            }
        }
    }
}

/// Original750920/7509E0 requests are supplied after GI construction, with
/// their recorded gain and clock/cursor inputs. Compare the shared audio path
/// through endpoint405A00 and the subsequent405974 service/reap. The native
/// handle's stale pointer/serial layout is not a Rust representation contract;
/// its tagged type word determines whether an owner still names the event.
fn replay_shared_one_shot(name: &str) {
    let row = history(name);
    assert_eq!(row["original_code_unchanged"], true);
    let registry = registry(&native()["retail"]);
    assert_reader(&registry, &native()["retail"]);
    let entry = registry.get("GIMove").unwrap();
    let input = row["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|input| input["kind"] == "declared_GIMove_request_with_original_GI_handle")
        .unwrap();
    let owner = match input["route"].as_str().unwrap() {
        "global" => HandleOwner::UnitVoice(1),
        "positional" => HandleOwner::Positional(1),
        route => panic!("unrepresented request route {route}"),
    };
    let boundaries = row["boundaries"].as_array().unwrap();
    let first = boundaries
        .iter()
        .position(|boundary| boundary["label"] == "empty_audio_pump_1000")
        .unwrap();
    let observer = DrawObserver::new(row, &boundaries[first]["before"]);
    let mut playback = OneShotService {
        entry,
        main: &observer.main,
        draws: observer.draws.clone(),
        pending: None,
        output: None,
        compact_slots: BTreeMap::new(),
        preparations: Vec::new(),
        observation: None,
        delivered: Vec::new(),
        stops: 0,
    };
    let mut arbiter = SoundArbiter::new(1000);
    let mut clock = arbiter::AudioServiceClock::default();
    let mut token = None;
    let mut fills_compared = 0;
    let mut endpoint_delivered = false;

    for boundary in &boundaries[first..] {
        let label = boundary["label"].as_str().unwrap();
        assert_main(&observer.main, row, &boundary["before"], label);
        let (_, raw) = trace_draws(|| {
            let fills: Vec<_> = row["native_ring_fills"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|fill| fill["phase"] == label)
                .collect();
            if label.ends_with("_GIMove_request") {
                let native_event = &boundary["after"]["events"][0];
                let event = arbiter
                    .submit(
                        &PlayRequest {
                            key: entry.id.clone(),
                            facts: EntryFacts::from(entry),
                            volume_linear: (native_event["volume_fixed16"].as_i64().unwrap() >> 16)
                                as i32,
                            pan: PAN_CENTRE,
                        },
                        1000,
                    )
                    .unwrap();
                arbiter.set_loop_handle(owner, Some(event), &entry.id);
                let submitted = arbiter.event_token(event).unwrap();
                token = Some(submitted);
                playback.pending = Some(PendingPlayback::new(
                    submitted,
                    entry.id.clone(),
                    SfxChannel::Sound,
                ));
            } else if label.starts_with("audio_pump_") || label == "empty_audio_pump_1000" {
                let now_ms = label.rsplit('_').next().unwrap().parse().unwrap();
                assert!(
                    clock.admit(now_ms),
                    "{label}: supplied native service clock"
                );
                for action in arbiter.update_tick(now_ms, &mut playback) {
                    match action {
                        ArbiterAction::Start { event, .. } => {
                            let mut pending = playback.pending.take().unwrap();
                            assert_eq!(pending.token.event, event);
                            let (source, control) = pending.source.take().unwrap();
                            playback.observation = source.observation();
                            assert_eq!(fills.len(), 1, "one original initial ring fill");
                            playback.output = Some(HeadlessOutput {
                                source,
                                control,
                                backend: fills[0]["backend"].as_u64().unwrap(),
                                compact_slots: std::mem::take(&mut playback.compact_slots),
                                names: entry.sounds.clone(),
                            });
                            playback.output.as_ref().unwrap().assert_fill(
                                &observer.main,
                                row,
                                fills[0],
                            );
                            fills_compared += 1;
                            assert!(playback.pull_quarter() > 0, "initial cached quarter");
                            let prepares: Vec<_> = row["native_sites"]
                                .as_array()
                                .unwrap()
                                .iter()
                                .filter(|site| {
                                    site["phase"] == label
                                        && matches!(site["pc"].as_u64(), Some(0x4045d6 | 0x404678))
                                })
                                .collect();
                            assert_eq!(prepares.len(), 2, "both original preparations");
                            assert_eq!(playback.preparations.len(), prepares.len());
                            for (actual, site) in playback.preparations.iter().zip(prepares) {
                                let native_event = &site["state"]["events"][0];
                                let loaded =
                                    native_event["loaded_sample_identities"].as_array().unwrap();
                                let selected = loaded
                                    .iter()
                                    .find(|clip| {
                                        clip["pointer"].as_u64().unwrap() + 28
                                            == site["eax"].as_u64().unwrap()
                                    })
                                    .unwrap();
                                assert_eq!(actual["sample"], selected["name"]);
                                assert_eq!(
                                    actual["loaded"],
                                    json!(
                                        loaded.iter().map(|clip| &clip["name"]).collect::<Vec<_>>()
                                    )
                                );
                                assert_eq!(actual["main"], main_hex(row, &site["state"]));
                            }
                        }
                        ArbiterAction::Gain { pan, .. } => {
                            playback.output.as_ref().unwrap().control.set_pan(pan);
                        }
                        ArbiterAction::Decay { event } => {
                            assert_eq!(event, token.unwrap().event);
                            assert!(
                                !playback.output.as_ref().unwrap().control.release_to_decay(),
                                "one-shot has no decay payload"
                            );
                            arbiter.notify_playout_stopped(token.unwrap());
                            endpoint_delivered = true;
                        }
                        ArbiterAction::Stop { event } => {
                            assert_eq!(event, token.unwrap().event);
                            let mut output = playback.output.take().unwrap();
                            assert_eq!(output.source.native_snapshot()["cancelled"], true);
                            assert_eq!(output.source.next(), None);
                            drop(output);
                        }
                    }
                }
            } else if matches!(
                label,
                "original_handle_release" | "original_generic_handle_detach"
            ) {
                let control = playback.output.as_ref().unwrap().control.clone();
                control.update_no_replay(|| {
                    if label == "original_handle_release" {
                        arbiter.release_owner(owner);
                    } else {
                        arbiter.detach_owner(owner);
                    }
                    arbiter.playout_no_replay(token.unwrap().event)
                });
            } else if label.starts_with("original_worker_") {
                for fill in &fills {
                    assert_main(
                        &observer.main,
                        row,
                        &json!({"rng": fill["rng_before"]}),
                        label,
                    );
                    playback.pull_quarter();
                    playback
                        .output
                        .as_ref()
                        .unwrap()
                        .assert_fill(&observer.main, row, fill);
                    fills_compared += 1;
                }
                let endpoint = row["native_sites"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|site| site["phase"] == label && site["pc"] == 0x405a00_u64);
                if endpoint {
                    assert_eq!(playback.output.as_mut().unwrap().source.next(), None);
                    arbiter.notify_playout_ended(token.unwrap(), PlayoutEnd::default(), 0);
                    endpoint_delivered = true;
                }
            } else {
                panic!("unrepresented native boundary {label}");
            }
        });
        observer.assert_phase(row, boundary, raw);
        let after = &boundary["after"];
        assert_eq!(
            arbiter.live_event_count() as u64,
            after["live_count"].as_u64().unwrap()
        );
        if let Some(native_event) = after["events"].as_array().unwrap().first() {
            let token = token.unwrap();
            let snapshot = arbiter.native_event_snapshot(token.event).unwrap();
            assert_eq!(
                json!({"state": snapshot["state"], "flags": snapshot["flags"], "serial": snapshot["serial"]}),
                json!({"state": native_event["state"], "flags": native_event["flags"], "serial": native_event["serial"]}),
                "{name}: {label}: original event state/flags/serial",
            );
            assert!(arbiter.is_live_token(token));
            assert!(!arbiter.is_token_retired(token));
            assert!(arbiter.event_slot_occupied(token.event));
            assert_eq!(
                arbiter.token_has_channel(token),
                native_event["channel"].as_u64().unwrap() != 0,
                "{label}: channel survives endpoint until service",
            );
            let tagged_handle = after["actors"][0]["handle"][2].as_u64().unwrap() != 0;
            assert_eq!(arbiter.validate_loop_handle(owner).is_some(), tagged_handle);
            if let Some(output) = &mut playback.output {
                output.assert_cursor(native_event, label);
                if native_event["state"] == 4 {
                    assert_eq!(output.source.next(), None, "ended source stays exhausted");
                }
            }
        } else if let Some(token) = token {
            assert_eq!(arbiter.native_event_snapshot(token.event), None);
            assert!(arbiter.is_token_retired(token));
            assert!(!arbiter.is_live_token(token));
            assert!(!arbiter.event_slot_occupied(token.event));
            assert!(!arbiter.token_has_channel(token));
            assert_eq!(arbiter.validate_loop_handle(owner), None);
            assert_eq!(arbiter.busy_channel_count(), 0);
            assert!(playback.output.is_none());
        }
    }
    assert!(endpoint_delivered);
    assert_eq!(
        fills_compared,
        row["native_ring_fills"].as_array().unwrap().len()
    );
    assert_eq!(
        playback.stops, 1,
        "one synchronous stop before payload retirement"
    );
    let observation = playback.observation.unwrap().snapshot();
    assert!(
        !observation.source_state_alive,
        "retirement drops ring and PCM owners"
    );
    assert_eq!(observation.resolved_samples, ["igimod"]);
    let pcm = mono_bytes(playback.delivered.into_iter());
    let native_pcm = bytes_from_hex(
        native()["physical_clips"]["igimod"]["pcm_hex"]
            .as_str()
            .unwrap(),
    );
    if name.contains("generic_detach") {
        assert_eq!(
            pcm,
            native_pcm[..pcm.len()],
            "only the initial quarter was consumed before supplied Detach"
        );
        assert!(pcm.len() < native_pcm.len());
    } else {
        assert_eq!(
            pcm[..native_pcm.len()],
            native_pcm,
            "Release retains every original decoded byte"
        );
        assert!(
            pcm[native_pcm.len()..].iter().all(|byte| *byte == 0),
            "last native quarter carries silence padding"
        );
        assert_eq!(
            observation.clips[0].pulled_samples,
            observation.source_sample_count
        );
    }
}

#[test]
fn native_global_gi_release_finishes_then_retires_shared_source() {
    replay_shared_one_shot("stock_GIMove_global_release_cleanup_control");
}

#[test]
fn native_global_gi_generic_detach_ends_before_shared_retirement() {
    replay_shared_one_shot("stock_GIMove_global_generic_detach_cleanup_control");
}

#[test]
fn native_positional_gi_release_finishes_then_retires_shared_source() {
    replay_shared_one_shot("stock_GIMove_positional_release_cleanup_control");
}

#[test]
fn native_positional_gi_generic_detach_ends_before_shared_retirement() {
    replay_shared_one_shot("stock_GIMove_positional_generic_detach_cleanup_control");
}

#[test]
fn native_uniform_consumer_continues_across_first_ring_quarter() {
    // rodio0.22.2 UniformSourceIterator::bootstrap trusts Source's span:
    // Some(0) means exhaustion, including when a PCM quarter was just read.
    let row = history("stock_initial_audible_loop1_soft_release");
    let start = row["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|boundary| boundary["label"] == "audio_pump_1034")
        .unwrap();
    let worker = row["boundaries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|boundary| boundary["label"] == "original_worker_1290")
        .unwrap();
    let entry = registry(&native()["retail"])
        .get("GattlingGunAttackLoop1")
        .unwrap()
        .clone();
    let mut observer = DrawObserver::new(row, &start["before"]);
    let (output, raw) = trace_draws(|| {
        let shifts = PlayShifts::draw(&entry, &mut observer.draws);
        let loaded = LoadedPlayback::load(&entry, &mut observer.draws, fixture_clip).unwrap();
        let compact_slots = loaded
            .clips
            .keys()
            .copied()
            .enumerate()
            .map(|(compact, index)| (index, compact))
            .collect();
        let mut cursor = PlaylistCursor::new(
            loaded.selected.clone(),
            entry.control,
            entry.delay_ms,
            entry.loop_count,
        );
        assert_eq!(
            cursor.prepare(&mut observer.draws, true),
            loaded.selected.attack
        );
        let initial = cursor.prepare(&mut observer.draws, true).unwrap();
        let rate = shifts.shifted_sample_rate(loaded.clips[&initial].sample_rate);
        let (source, control) = RingSource::new(
            loaded,
            cursor,
            initial,
            observer.draws.clone(),
            rate,
            PAN_CENTRE,
            None,
        )
        .unwrap();
        HeadlessOutput {
            source,
            control,
            backend: row["native_ring_fills"][0]["backend"].as_u64().unwrap(),
            compact_slots,
            names: entry.sounds.clone(),
        }
    });
    observer.assert_phase(row, start, raw);
    output.assert_fill(&observer.main, row, &row["native_ring_fills"][0]);
    let quarter = output.source.native_snapshot()["quarter_samples"]
        .as_u64()
        .unwrap() as usize;
    let mut uniform = rodio::source::UniformSourceIterator::new(
        output.source,
        NonZero::new(2).unwrap(),
        NonZero::new(22050).unwrap(),
    );
    let (delivered, raw) = trace_draws(|| uniform.by_ref().take(quarter + 2).collect::<Vec<_>>());
    assert_eq!(
        delivered.len(),
        quarter + 2,
        "real rodio consumer must cross a quarter boundary"
    );
    assert_eq!(
        mono_bytes(delivered.into_iter()),
        bytes_from_hex(
            native()["physical_clips"]["vgatlo1a"]["pcm_hex"]
                .as_str()
                .unwrap()
        )[..quarter + 2],
        "unchanged channel/rate transport retains the native attack PCM",
    );
    observer.assert_phase(row, worker, raw);
    let before = observer.main.native_state_hex();
    output.control.cancel();
    assert_eq!(
        uniform.next(),
        None,
        "consumer also observes hard cancellation"
    );
    assert_eq!(observer.main.native_state_hex(), before);
}

#[test]
fn retail_gattling_registry_and_all_executed_pcm_clips_match_native() {
    let Some((_root, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let definitions = crate::rules::retail_ini_fixture::retail_audio_definitions()
        .expect("retail audio definitions");
    assert_reader(definitions.sounds(), &native()["retail"]);
    let selected =
        crate::rules::retail_ini_fixture::retail_audio_index().expect("retail audio index");
    for (name, clip) in native()["physical_clips"].as_object().unwrap() {
        let (entry, source) = selected.index.get(name).unwrap();
        assert_eq!(
            u64::from(entry.offset),
            clip["bag_offset"].as_u64().unwrap(),
            "{name}: BAG offset"
        );
        assert_eq!(
            u64::from(entry.size),
            clip["bytes"].as_u64().unwrap(),
            "{name}: source bytes"
        );
        assert_eq!(u64::from(entry.sample_rate), clip["rate"].as_u64().unwrap());
        assert_eq!(u64::from(entry.flags), clip["flags"].as_u64().unwrap());
        assert_eq!(
            u64::from(entry.chunk_size),
            clip["chunk_bytes"].as_u64().unwrap()
        );
        assert_eq!(
            crate::util::sha256::sha256_hex(source),
            clip["sha256"].as_str().unwrap(),
            "{name}: physical source"
        );
        let decoded = load_sfx(name, &assets, Some(&selected.index)).unwrap();
        assert_eq!(
            u64::from(decoded.sample_rate),
            clip["rate"].as_u64().unwrap()
        );
        assert_eq!((decoded.channels, decoded.native_frame_bytes), (2, 2));
        assert_eq!(
            mono_bytes(decoded.samples.into_iter()),
            bytes_from_hex(clip["pcm_hex"].as_str().unwrap()),
            "{name}: every original decoded PCM byte"
        );
    }
}

#[test]
fn cursor_low_delay_replay_selects_from_the_same_loaded_bodies() {
    // Below-floor control flow from4047B0. The above-floor loader and its
    // gaps belong to ambient_playout_tests.
    let entry = registry(&native()["retail"])
        .get("GattlingGunAttackLoop1")
        .unwrap()
        .clone();
    for delay in [0, arbiter::PREDELAY_FLOOR_MS - 1] {
        let main = MainRng::new(31);
        let retained = main.draws();
        let mut draws = PlaybackDraws::new(move |low, high| retained.ranged(low, high));
        let loaded = LoadedPlayback::load(&entry, &mut draws, fixture_clip).unwrap();
        let mut cursor =
            PlaylistCursor::new(loaded.selected.clone(), entry.control, (delay, delay), 2);
        assert_eq!(cursor.prepare(&mut draws, true), loaded.selected.attack);
        for _ in 0..loaded.selected.middle.len() {
            assert!(
                loaded
                    .selected
                    .middle
                    .contains(&cursor.advance(&mut draws).unwrap())
            );
        }
        let next = cursor.advance(&mut draws).unwrap();
        assert!(loaded.selected.middle.contains(&next));
    }
}

#[test]
fn cursor_counted_budget_and_reprepare_keep_decay_used_once() {
    let entry = registry(&native()["counted_control_reader"])
        .get("GattlingGunAttackLoop1")
        .unwrap()
        .clone();
    assert_eq!(entry.loop_count, 3, "original reader control");
    let main = MainRng::new(31);
    let retained = main.draws();
    let mut draws = PlaybackDraws::new(move |low, high| retained.ranged(low, high));
    let loaded = LoadedPlayback::load(&entry, &mut draws, fixture_clip).unwrap();
    let mut cursor = PlaylistCursor::new(
        loaded.selected.clone(),
        entry.control,
        entry.delay_ms,
        entry.loop_count,
    );
    assert_eq!(cursor.prepare(&mut draws, true), loaded.selected.attack);
    let mut bodies = Vec::new();
    while let Some(next) = cursor.advance(&mut draws) {
        if Some(next) == loaded.selected.decay {
            break;
        }
        assert!(loaded.selected.middle.contains(&next));
        bodies.push(next);
        assert!(bodies.len() <= 9, "counted loop terminates");
    }
    assert_eq!(bodies.len(), 9);
    assert_eq!(cursor.advance(&mut draws), None);
    cursor.prevent_replay();
    let before = main.native_state_hex();
    assert_eq!(
        cursor.prepare(&mut draws, false),
        None,
        "Prepare404700 retains used decay"
    );
    assert_eq!(main.native_state_hex(), before);
}

#[test]
fn initial_positional_loop_preserves_native_attack_admission() {
    let Some(definitions) = crate::rules::retail_ini_fixture::retail_audio_definitions() else {
        return;
    };
    let Some(mut player) = SfxPlayer::new() else {
        return;
    };
    let registry = definitions.sounds();
    assert!(player.play_animation_sound_spatial(
        79_u64,
        "GattlingGunAttackLoop1",
        SpatialGain::CENTRED_FULL,
        registry,
    ));
    let event = player
        .arbiter
        .validate_loop_handle(HandleOwner::Positional(79));
    assert!(event.is_some());
    assert!(player.arbiter.plays_attack_sample(event.unwrap()));
    player.stop_all();
}
