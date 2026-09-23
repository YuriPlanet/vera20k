use super::*;
use serde_json::Value;

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: integer(&value[0]),
        y: integer(&value[1]),
        z: integer(&value[2]),
    }
}

fn assert_progress(progress: &TrackProgress, expected: &Value) {
    assert_eq!(progress.turn_index, integer(&expected["turn"]));
    assert_eq!(progress.cursor, integer(&expected["cursor"]));
    assert_eq!(progress.reversed, expected["reversed"].as_bool().unwrap());
}

#[test]
fn post_placement_gates_reload_cursor_and_preserve_the_paid_raw_descriptor() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/locomotor_track_point_gates.json"
    ))
    .unwrap();
    let mut descriptors = 0;
    let mut cases = 0;
    let mut within_point_cursor_changes = 0;
    let mut selector_changes = 0;
    for (name, family) in [("drive", TrackFamily::Drive), ("ship", TrackFamily::Ship)] {
        descriptors += integer(&corpus[name]["raw_descriptors"]);
        for case in corpus[name]["cases"].as_array().unwrap() {
            let (input, output) = (&case["input"], &case["output"]);
            let initial = &input["initial"];
            let mut progress = TrackProgress {
                turn_index: integer(&initial["turn"]),
                cursor: 0,
                reversed: initial["reversed"].as_bool().unwrap(),
                residual: 6,
            };
            let mut call = TrackProcess::begin(family, &progress, 9, false);
            let Some(TrackPayment::Sample(sample)) = call.pay_current(&progress) else {
                panic!("native case pays the initial point: {name} {input}");
            };
            assert!(!sample.terminal);
            assert_eq!(
                i32::from(sample.raw_index),
                integer(&output["cached"]["raw"])
            );
            let mutation = &input["mutation"];
            progress.turn_index = integer(&mutation["turn"]);
            progress.cursor = integer(&mutation["cursor"]);
            progress.reversed = mutation["reversed"].as_bool().unwrap();
            progress.residual = integer(&mutation["residual"]);
            assert_eq!(
                call.is_at_occupation_handoff(&progress),
                output["handoff"].as_bool().unwrap(),
                "{name} handoff {input}"
            );
            assert_progress(&progress, &output["after_handoff"]);
            let chain_cursor = integer(&input["chain_cursor"]);
            within_point_cursor_changes += usize::from(progress.cursor != chain_cursor);
            selector_changes += usize::from(progress.turn_index != integer(&initial["turn"]));
            progress.cursor = chain_cursor;
            assert_eq!(
                call.is_at_chain_cursor(&progress),
                output["chain"].as_bool().unwrap(),
                "{name} chain {input}"
            );
            assert_progress(&progress, &output["after_chain"]);
            assert_eq!(call.budget(), integer(&output["local_budget"]));
            assert_eq!(progress.residual, integer(&output["retained_residual"]));
            cases += 1;
        }
    }
    assert_eq!(descriptors, 25);
    assert_eq!(cases, 510);
    assert_eq!(within_point_cursor_changes, 230);
    assert!(selector_changes > 0);
}

#[test]
fn retained_cursor_and_paid_samples_match_original_drive_and_ship() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/locomotor_track_cursor.json"
    ))
    .unwrap();
    let mut samples = 0;
    let mut fresh = 0;
    let mut chains = 0;
    let mut terminals = 0;
    let mut arrays = 0;
    let mut tables = 0;
    for (name, family) in [("drive", TrackFamily::Drive), ("ship", TrackFamily::Ship)] {
        let data = &corpus[name];
        for (index, expected) in data["tables"].as_array().unwrap().iter().enumerate() {
            let actual = family.turn(index as i32).unwrap();
            assert_eq!(
                [
                    i32::from(actual.normal_track),
                    i32::from(actual.short_track),
                    i32::from(actual.target_facing),
                    i32::from(actual.flags)
                ],
                [
                    integer(&expected["normal"]),
                    integer(&expected["short"]),
                    integer(&expected["facing"]),
                    integer(&expected["flags"])
                ]
            );
            tables += 1;
        }
        for (raw_index, data) in data["raw"].as_object().unwrap() {
            let raw_index = raw_index.parse::<u8>().unwrap();
            let meta = drive_track::raw_track_meta(raw_index).unwrap();
            assert_eq!(i32::from(meta.entry_index), integer(&data["entry"]));
            assert_eq!(i32::from(meta.chain_index), integer(&data["chain"]));
            assert_eq!(
                i32::from(meta.occupation_handoff_point_index),
                integer(&data["handoff"])
            );
            let points = data["points"].as_array().unwrap();
            assert_eq!(
                points.len(),
                drive_track::raw_track_points(raw_index).len() + 1
            );
            for (cursor, expected) in points.iter().enumerate() {
                let actual = raw_point(raw_index, cursor as i32).unwrap();
                assert_eq!(
                    [
                        i32::from(actual.x),
                        i32::from(actual.y),
                        i32::from(actual.facing)
                    ],
                    [
                        integer(&expected[0]),
                        integer(&expected[1]),
                        integer(&expected[2])
                    ]
                );
            }
            assert!(raw_point(raw_index, points.len() as i32).is_none());
            arrays += 1;
        }
        for case in data["samples"].as_array().unwrap() {
            let (input, output) = (&case["input"], &case["output"]);
            let mut progress = TrackProgress {
                turn_index: integer(&input["turn"]),
                cursor: integer(&input["cursor"]),
                reversed: input["reverse"].as_bool().unwrap(),
                residual: integer(&input["budget"]),
            };
            let residual_before = progress.residual;
            let mut call = TrackProcess::begin(family, &progress, 0, false);
            match call.pay_current(&progress).unwrap() {
                TrackPayment::Exhausted => assert_eq!(output["kind"], "unpaid"),
                TrackPayment::Sample(sample) => {
                    assert_eq!(
                        sample.xy,
                        [integer(&output["xy"][0]), integer(&output["xy"][1])]
                    );
                    assert_eq!(
                        sample.cursor, progress.cursor,
                        "payment leaves cursor for callbacks"
                    );
                    if sample.terminal {
                        assert_eq!(output["kind"], "terminal");
                    } else {
                        assert_eq!(output["kind"], "point");
                        assert_eq!(
                            call.finish_surviving_point(&mut progress),
                            output["continue_paid"].as_bool().unwrap()
                        );
                    }
                }
            }
            assert_progress(&progress, &output["after"]);
            assert_eq!(call.budget(), integer(&output["budget"]));
            assert_eq!(
                progress.residual, residual_before,
                "callback barrier cannot eagerly store local budget"
            );
            samples += 1;
        }
        for case in data["fresh"].as_array().unwrap() {
            let (input, output) = (&case["input"], &case["output"]);
            let mut progress = TrackProgress {
                turn_index: -7,
                cursor: 77,
                reversed: true,
                residual: 123,
            };
            assert!(progress.select_fresh(
                family,
                integer(&input["first"]) as u8,
                integer(&input["second"]) as u8
            ));
            assert_progress(&progress, &output["selected"]);
            progress.accept_fresh();
            assert_progress(&progress, &output["accepted"]);
            assert_eq!(progress.residual, 123);
            fresh += 1;
        }
        for case in data["chains"].as_array().unwrap() {
            let (input, output) = (&case["input"], &case["output"]);
            let mut progress = TrackProgress {
                turn_index: 9,
                cursor: 45,
                reversed: true,
                residual: 971,
            };
            // Like the native corpus, supply an already-reached chain callback
            // seam. This does not claim the preceding owner callbacks ran.
            let mut call = TrackProcess {
                family,
                budget: integer(&input["budget"]),
                phase: ProcessPhase::PointCallbacks,
                selection: PaidTrackSelection::from_progress(family, &progress),
            };
            assert!(call.accept_chain(&mut progress, integer(&input["turn"])));
            assert_progress(&progress, &output["accepted"]);
            assert_eq!(
                call.finish_surviving_point(&mut progress),
                output["continue_paid"].as_bool().unwrap()
            );
            assert_progress(&progress, &output["after"]);
            assert_eq!(call.budget(), integer(&output["budget"]));
            assert_eq!(progress.residual, 971);
            chains += 1;
        }
        for case in data["terminals"].as_array().unwrap() {
            let (input, output) = (&case["input"], &case["output"]);
            let mut progress = TrackProgress {
                turn_index: 9,
                cursor: 45,
                reversed: true,
                residual: 971,
            };
            let mut call = TrackProcess {
                family,
                budget: integer(&input["budget"]),
                phase: ProcessPhase::TerminalCallbacks,
                selection: PaidTrackSelection::from_progress(family, &progress),
            };
            call.adjust_terminal_budget(coord(&input["current"]), coord(&input["head"]));
            assert_eq!(
                call.budget(),
                integer(&output["budget"]),
                "{name} terminal {input}"
            );
            progress.clear_selector();
            assert_progress(&progress, &output["selectors_after_clear"]);
            assert_eq!(progress.residual, 971);
            terminals += 1;
        }
    }
    assert_eq!(
        (samples, fresh, chains, terminals, arrays, tables),
        (3540, 128, 128, 45, 25, 136)
    );
}

#[test]
fn residual_scalar_and_cell_identity_gate_match_original_instructions() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/locomotor_track_residual.json"
    ))
    .unwrap();
    let mut scalars = 0;
    let mut gates = 0;
    for name in ["drive", "ship"] {
        for case in corpus[name]["scalar"].as_array().unwrap() {
            let (input, output) = (&case["input"], &case["output"]);
            let delta = [
                integer(&input["delta"][0]),
                integer(&input["delta"][1]),
                integer(&input["delta"][2]),
            ];
            let (bits, scaled) = residual_delta(delta, integer(&input["residual"]));
            assert_eq!(
                bits.bits(),
                output["factor_bits"].as_u64().unwrap() as u32,
                "{name} {input}"
            );
            assert_eq!(
                scaled,
                [
                    integer(&output["delta"][0]),
                    integer(&output["delta"][1]),
                    integer(&output["delta"][2])
                ],
                "{name} {input}"
            );
            scalars += 1;
        }
        for case in corpus[name]["gates"].as_array().unwrap() {
            let (input, output) = (&case["input"], &case["output"]);
            let step = ResidualStep {
                current: coord(&input["current"]),
                full: coord(&input["full"]),
                interpolated: coord(&input["interpolated"]),
            };
            assert_eq!(
                step.choose(
                    input["cells"][0] == input["cells"][2],
                    input["cells"][0] == input["cells"][1],
                    integer(&input["residual"])
                ),
                coord(&output["chosen"])
            );
            gates += 1;
        }
    }
    assert_eq!((scalars, gates), (84, 32));
}

#[test]
fn rejected_chain_cannot_publish_the_null_raw_record_as_a_curve() {
    let retained = TrackProgress {
        turn_index: 1,
        cursor: 37,
        reversed: true,
        residual: 971,
    };
    for family in [TrackFamily::Drive, TrackFamily::Ship] {
        for rejected in [-1, 0, 3, 72] {
            let mut candidate = retained;
            assert!(!candidate.accept_chain(family, rejected));
            assert_eq!(candidate, retained);
        }
    }
}

#[test]
fn accepted_chain_advances_refetched_cursor_and_preserves_old_residual_until_store() {
    let mut progress = TrackProgress {
        turn_index: 0,
        cursor: 0,
        reversed: false,
        residual: 6,
    };
    let mut call = TrackProcess::begin(TrackFamily::Drive, &progress, 9, false);
    let TrackPayment::Sample(sample) = call.pay_current(&progress).unwrap() else {
        panic!("paid first sample")
    };
    assert_eq!(sample.xy, [0, 245]);
    assert_eq!(call.budget(), 8);
    assert!(call.accept_chain(&mut progress, 1));
    assert_eq!(progress.cursor, 11);
    // A native callback exit stops here, preserving entry-1 and old residual.
    assert_eq!(progress.residual, 6);
    let callback_exit_state = progress;
    assert!(call.finish_surviving_point(&mut progress));
    assert_eq!(progress.cursor, 12);
    assert_eq!(callback_exit_state.cursor, 11);
    let TrackPayment::Sample(next) = call.pay_current(&progress).unwrap() else {
        panic!("same-process successor")
    };
    assert_eq!(next.xy, [-256, 373]);
    assert!(!call.finish_surviving_point(&mut progress));
    call.store_residual(&mut progress);
    assert_eq!(progress.residual, 1);
}

#[test]
fn callback_mutations_keep_paid_raw_cache_but_transform_and_residual_use_live_state() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/locomotor_track_callback.json"
    ))
    .unwrap();
    let mut cases = 0;
    let mut chains = 0;
    let mut changed_raw_cases = 0;
    for (name, family) in [("drive", TrackFamily::Drive), ("ship", TrackFamily::Ship)] {
        for case in corpus[name].as_array().unwrap() {
            let (input, output) = (&case["input"], &case["output"]);
            let initial = &input["initial"];
            let mut progress = TrackProgress {
                turn_index: integer(&initial["turn"]),
                cursor: integer(&initial["cursor"]),
                reversed: initial["reversed"].as_bool().unwrap(),
                residual: integer(&initial["residual"]),
            };
            // The fixture enters after budget calculation with budget15.
            let mut call = TrackProcess::begin(family, &progress, 9, false);
            let TrackPayment::Sample(first) = call.pay_current(&progress).unwrap() else {
                panic!("{name} first paid point: {input}")
            };
            assert_native_cached_sample(&call, first, &output["first"]);
            if let Some(chain) = input["chain"].as_i64() {
                assert!(call.accept_chain(&mut progress, chain as i32));
                chains += 1;
            }
            assert_progress(&progress, &output["accepted"]["progress"]);
            assert_eq!(
                call.chain_target_facing().map(i32::from),
                Some(integer(&output["accepted"]["selected"]["target_facing"]))
            );
            // These are the same supplied mutations as the native witness;
            // this test does not claim a gameplay callback produces each one.
            let mutation = &input["mutation"];
            if let Some(turn) = mutation["turn"].as_i64() {
                progress.turn_index = turn as i32;
            }
            if let Some(cursor) = mutation["cursor"].as_i64() {
                progress.cursor = cursor as i32;
            }
            if let Some(reversed) = mutation["reversed"].as_bool() {
                progress.reversed = reversed;
            }
            if let Some(residual) = mutation["residual"].as_i64() {
                progress.residual = residual as i32;
            }
            let head = coord(&mutation["head"]);
            assert_progress(&progress, &output["before_tail"]);
            assert!(call.finish_surviving_point(&mut progress));
            let TrackPayment::Sample(second) = call.pay_current(&progress).unwrap() else {
                panic!("{name} second paid point: {input}")
            };
            assert_native_cached_sample(&call, second, &output["second"]);
            assert_progress(&progress, &output["after_payment"]);
            assert_eq!(progress.residual, integer(&output["retained_residual"]));
            let (xy, facing) = second.transform(family, &progress, head).unwrap();
            let transformed = &output["second"]["transformed"];
            assert_eq!(
                xy,
                [
                    integer(&transformed["xy"][0]),
                    integer(&transformed["xy"][1])
                ],
                "{name} live transform: {input}"
            );
            assert_eq!(i32::from(facing), integer(&transformed["facing"]));
            if progress.selected_raw(family).unwrap().0 != second.raw_index {
                changed_raw_cases += 1;
            }
            assert!(!call.finish_surviving_point(&mut progress));
            call.store_residual(&mut progress);
            let expected = &output["residual"];
            assert_progress(&progress, &expected["progress"]);
            assert_eq!(progress.residual, integer(&expected["budget"]));
            let residual = progress.residual_step(family, head, head).unwrap();
            assert_eq!(
                [residual.full.x, residual.full.y],
                [
                    integer(&expected["transformed_xy"][0]),
                    integer(&expected["transformed_xy"][1]),
                ],
                "{name} live residual reselect: {input}"
            );
            cases += 1;
        }
    }
    assert_eq!((cases, chains), (54, 12));
    assert!(
        changed_raw_cases > 0,
        "corpus must distinguish cached and live raw selection"
    );
}

fn assert_native_cached_sample(call: &TrackProcess, sample: PaidSample, expected: &Value) {
    assert_eq!(sample.cursor, integer(&expected["cursor"]));
    assert_eq!(
        i32::from(sample.raw_index),
        integer(&expected["selected"]["raw"])
    );
    assert_eq!(
        call.chain_target_facing().map(i32::from),
        Some(integer(&expected["selected"]["target_facing"]))
    );
    assert_eq!(
        [sample.xy[0], sample.xy[1], i32::from(sample.facing)],
        [
            integer(&expected["point"][0]),
            integer(&expected["point"][1]),
            integer(&expected["point"][2]),
        ]
    );
    assert_eq!(call.budget(), integer(&expected["budget"]));
}

#[test]
fn forced_selector_clearing_does_not_reconstruct_constructor_state() {
    let mut progress = TrackProgress::default();
    assert_eq!((progress.turn_index, progress.cursor), (-1, -1));
    progress.reversed = true;
    progress.residual = 971;
    progress.select_forced(-1);
    assert_eq!(
        (
            progress.turn_index,
            progress.cursor,
            progress.reversed,
            progress.residual
        ),
        (-1, 0, true, 971)
    );
    progress.select_forced(71);
    assert_eq!(
        (
            progress.turn_index,
            progress.cursor,
            progress.reversed,
            progress.residual
        ),
        (71, 0, true, 971)
    );
    progress.clear_selector();
    assert_eq!(
        (
            progress.turn_index,
            progress.cursor,
            progress.reversed,
            progress.residual
        ),
        (-1, 0, true, 971)
    );
}

#[test]
fn residual_only_continuation_uses_current_pose_and_preserves_height_and_selectors() {
    let progress = TrackProgress {
        turn_index: 0,
        cursor: 0,
        reversed: false,
        residual: 4,
    };
    let head = DriveCoord {
        x: 2176,
        y: 1920,
        z: -731,
    };
    let current = DriveCoord {
        x: 2176,
        y: 2169,
        z: 731,
    };
    let first = progress
        .residual_step(TrackFamily::Drive, current, head)
        .unwrap();
    assert_eq!(
        first.full,
        DriveCoord {
            x: 2176,
            y: 2165,
            z: 731
        }
    );
    let second = progress
        .residual_step(TrackFamily::Drive, first.interpolated, head)
        .unwrap();
    assert_ne!(
        first.interpolated, second.interpolated,
        "each residual-only pass starts from current XYZ"
    );
    assert_eq!(second.interpolated.z, 731);
    assert_eq!(progress.cursor, 0);
    assert!(
        TrackProgress {
            cursor: 23,
            ..progress
        }
        .residual_step(TrackFamily::Drive, current, head)
        .is_none(),
        "unpaid sentinel cannot interpolate to the head"
    );
}
