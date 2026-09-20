//! Compare the real pure caller owner with original-byte continuation outputs.
//! This proves sampled coercion/dispatch and normalization only: callbacks are
//! external supplied returns, and no response body/world effect executes here.

use super::*;
use serde_json::Value;

fn rows() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/track_fresh_admission.json"
    ))
    .expect("fresh native corpus")
}

fn input_flag(input: &Value, field: &str, default: bool) -> bool {
    input[field].as_bool().unwrap_or(default)
}

fn native_boundary(stage: FreshStage, dispatch: FreshDispatch) -> &'static str {
    match dispatch {
        FreshDispatch::Accept => match stage {
            FreshStage::First => "accepted_before_terrain",
            FreshStage::Second => "before_two_node_shift",
        },
        FreshDispatch::OwnerNotAlive => "owner_not_alive",
        FreshDispatch::Redraw { .. } => "redraw_response",
        FreshDispatch::BlockedDelay => "code2_blocked_delay_response",
        FreshDispatch::Gate { .. } => "gate_response",
        FreshDispatch::WallOrObject { .. } => "wall_override_response",
        FreshDispatch::ScatterOrStop { .. } => "scatter_response",
        FreshDispatch::FirstOtherBlocked { .. } => "other_blocked_response",
        FreshDispatch::SecondRetryOrStop { .. } => "code7_retry_or_stop",
        FreshDispatch::Retry(_) => "recursive_process_movement",
        FreshDispatch::ClearSecondThenRetry(_) => "clear_track_then_retry_or_stop",
    }
}

#[test]
fn fresh_coercions_and_response_boundaries_match_all_100_native_query_rows() {
    let corpus = rows();
    let mut counts = [[0usize; 2]; 2];
    let mut differing_overlay_rows = 0;
    let mut mark_clobber_rows = 0;
    for row in &corpus {
        let input = &row["input"];
        let stage = match input["stage"].as_str().unwrap() {
            "first" => FreshStage::First,
            "second" => FreshStage::Second,
            "chain" | "recursive_normalization" => continue,
            stage => panic!("unreviewed native stage: {stage}"),
        };
        let family_index = match input["family"].as_str().unwrap() {
            "drive" => 0,
            "ship" => 1,
            family => panic!("unreviewed native family: {family}"),
        };
        counts[family_index][usize::from(stage == FreshStage::Second)] += 1;
        assert_eq!(row["original_code_unchanged"], true);
        let code = u8::try_from(input["code"].as_u64().unwrap()).unwrap();
        let overlay_field = match stage {
            FreshStage::First => "first_overlay",
            FreshStage::Second => "second_overlay",
        };
        let overlay = i32::try_from(input[overlay_field].as_i64().unwrap_or(-1)).unwrap();
        let effective = coerce_entry_code(
            code,
            input_flag(input, "train", false),
            input_flag(input, "crusher", false),
            overlay,
        )
        .unwrap();
        assert_eq!(
            u64::from(effective),
            row["saved_effective_code"].as_u64().unwrap(),
            "caller coercion: {input}"
        );
        let dispatch = dispatch_entry(
            stage,
            effective,
            true, // The witness's supplied original argument2.
            input_flag(input, "alive", true),
        )
        .unwrap();
        assert_eq!(native_boundary(stage, dispatch), row["boundary"], "{input}");
        let callbacks = row["callbacks"].as_array().unwrap();
        match stage {
            FreshStage::First => {
                // Corpus integrity, not a claim that this pure function runs
                // Mark. The saved result is the query return, never Mark EAX.
                assert_eq!(callbacks.len(), 3);
                assert_eq!(callbacks[0]["callback"], "mark");
                assert_eq!(callbacks[0]["mode"], 0);
                assert_eq!(callbacks[1]["callback"], "can_enter");
                assert_eq!(callbacks[1]["supplied_return"], code);
                assert_eq!(callbacks[2]["callback"], "mark");
                assert_eq!(callbacks[2]["mode"], 1);
                mark_clobber_rows += usize::from(input.get("mark_return").is_some());
            }
            FreshStage::Second => {
                assert_eq!(callbacks.len(), 1);
                assert_eq!(callbacks[0]["callback"], "can_enter");
                assert_eq!(callbacks[0]["arguments"][0], "second_candidate");
                assert_eq!(
                    row["produced_second_candidate"][0]["saved_cell"],
                    serde_json::json!([11, 8])
                );
                differing_overlay_rows += usize::from(input.get("crusher").is_some());
            }
        }
        if let FreshDispatch::Retry(flags) = dispatch {
            // Only this recursive response CALL executes in the corpus. Other
            // response metadata below has static-instruction provenance.
            let args = row["recursive_arguments"].as_array().unwrap();
            assert_eq!(args[0], 0xABCD0135u64);
            assert_eq!(args[1].as_u64().unwrap(), u64::from(flags.allow_retry));
            assert_eq!(
                args[2].as_u64().unwrap(),
                u64::from(flags.force_single_direction)
            );
        }
    }
    assert_eq!(corpus.len(), 148);
    assert_eq!(counts, [[26, 24], [26, 24]]);
    assert_eq!(differing_overlay_rows, 12);
    assert_eq!(mark_clobber_rows, 8);
}

#[test]
fn fresh_direction_normalization_matches_all_16_native_rows() {
    let corpus = rows();
    let mut counts = [0; 2];
    for row in &corpus {
        let input = &row["input"];
        if input["stage"] != "recursive_normalization" {
            continue;
        }
        let family = match input["family"].as_str().unwrap() {
            "drive" => 0,
            "ship" => 1,
            family => panic!("unreviewed native family: {family}"),
        };
        counts[family] += 1;
        let first = i32::try_from(input["first_direction"].as_i64().unwrap()).unwrap();
        let second = i32::try_from(input["second_direction"].as_i64().unwrap()).unwrap();
        let recursive = input["recursive_flag"].as_u64().unwrap() != 0;
        assert_eq!(
            i64::from(normalize_second_direction(first, second, recursive)),
            row["selected_second_direction"].as_i64().unwrap(),
            "{input}"
        );
        assert_eq!(row["original_code_unchanged"], true);
    }
    assert_eq!(counts, [8, 8]);
    // The remaining32 rows exercise a different production caller. Do not
    // implement another chain decision owner just to consume this corpus.
    assert_eq!(
        corpus
            .iter()
            .filter(|row| row["input"]["stage"] == "chain")
            .count(),
        32
    );
}

#[test]
fn retry_flags_preserve_stage_and_required_preceding_effects() {
    // Rust regression from original response-body instructions, not native
    // executable comparison: the148-row witness stops before these bodies.
    for allow_retry in [false, true] {
        let single = FreshRetry {
            allow_retry,
            force_single_direction: true,
        };
        assert_eq!(
            dispatch_entry(FreshStage::Second, 2, allow_retry, true),
            Some(FreshDispatch::Retry(single))
        );
        for code in [4, 5] {
            assert_eq!(
                dispatch_entry(FreshStage::Second, code, allow_retry, true),
                Some(FreshDispatch::ClearSecondThenRetry(single))
            );
        }
        let retry = allow_retry.then_some(FreshRetry {
            allow_retry: false,
            force_single_direction: false,
        });
        for stage in [FreshStage::First, FreshStage::Second] {
            assert_eq!(
                dispatch_entry(stage, 1, allow_retry, true),
                Some(FreshDispatch::Redraw { retry })
            );
        }
        assert_eq!(
            dispatch_entry(FreshStage::First, 7, allow_retry, true),
            Some(FreshDispatch::FirstOtherBlocked { retry })
        );
        assert_eq!(
            dispatch_entry(FreshStage::Second, 7, allow_retry, true),
            Some(FreshDispatch::SecondRetryOrStop { retry })
        );
    }
    assert_eq!(
        dispatch_entry(FreshStage::First, 0, true, false),
        Some(FreshDispatch::Accept),
        "the first response boundary has no post-query alive gate"
    );
    assert_eq!(coerce_entry_code(8, true, true, 0), None);
    assert_eq!(dispatch_entry(FreshStage::First, 8, true, true), None);
}
