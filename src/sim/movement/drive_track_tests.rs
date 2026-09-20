//! Tests for drive track data validation, selection and retained projections.
//!
//! The retired detached executor tests are covered by the native cursor corpus
//! in track_process_tests::retained_cursor_and_paid_samples_match_original_drive_and_ship
//! (3,540 samples, fresh/chain selectors, all arrays, terminal budgets), its
//! residual_scalar_and_cell_identity_gate_match_original_instructions comparison,
//! and movement_step_tests' production-host budget and terminal regressions.
//! Coordinate relinking, chain callbacks and occupation are exercised by
//! track_host_tests; ground_pose_tests retain the exact-Z and snapshot routes.

use super::*;

fn exit_octant(turn: &TurnTrack) -> u8 {
    (((u32::from(turn.target_facing) << 8) >> 12).wrapping_add(1) >> 1 & 7) as u8
}

/// `Can_Use_Track 0x004B4B00` ratchet over the shipped tables: true only on a
/// turning curve whose cursor sits on its chain point, with a path head that
/// leaves the curve's exit octant into another curve.
#[test]
fn occupant_can_use_track_answers_only_at_the_chain_point_of_a_turning_curve() {
    use crate::sim::components::TrackProgress;

    let mut checked_true = 0;
    for (turn_index, turn) in TURN_TRACKS.iter().enumerate().take(64) {
        let raw = &RAW_TRACKS[usize::from(turn.normal_track)];
        let exit = exit_octant(turn);
        for head in 0u8..8 {
            let chained = &TURN_TRACKS[usize::from(head) + usize::from(exit) * 8];
            let chained_entry_nonzero = chained.normal_track != 0
                && RAW_TRACKS[usize::from(chained.normal_track)].entry_index != 0;
            // Native compares `RawTrack+0x04` against the cursor and refuses
            // only a zero cursor, so a straight run's -1 chain index answers
            // true at cursor -1 — the never-tracked state — exactly as
            // `0x004B4B80..4B8A` does.
            let expected = raw.chain_index != 0 && head != exit && chained_entry_nonzero;
            let at_chain = TrackProgress {
                turn_index: turn_index as i32,
                cursor: i32::from(raw.chain_index),
                reversed: false,
                residual: 0,
            };
            assert_eq!(
                occupant_can_use_track(&at_chain, Some(head)),
                expected,
                "turn {turn_index} head {head} at chain point"
            );
            if expected {
                checked_true += 1;
                for off in [-1, 1] {
                    let elsewhere = TrackProgress {
                        cursor: i32::from(raw.chain_index) + off,
                        ..at_chain
                    };
                    assert!(
                        !occupant_can_use_track(&elsewhere, Some(head)),
                        "turn {turn_index} head {head} cursor off by {off}"
                    );
                }
                assert!(!occupant_can_use_track(&at_chain, Some(8)), "tube head");
                assert!(!occupant_can_use_track(&at_chain, None), "empty queue");
                assert!(
                    !occupant_can_use_track(
                        &TrackProgress {
                            turn_index: -1,
                            ..at_chain
                        },
                        Some(head)
                    ),
                    "no retained track"
                );
            }
        }
    }
    assert!(
        checked_true > 0,
        "the shipped tables must expose at least one true case"
    );
    // A straight run carries chain index -1 and is never answered true at a
    // real cursor; the cursor 0 exit closes the fresh-acceptance state too.
    let straight = TrackProgress {
        turn_index: 0,
        cursor: 0,
        reversed: false,
        residual: 0,
    };
    assert!(!occupant_can_use_track(&straight, Some(2)));
}

/// Native comparison: every recorded `Can_Use_Track` answer from
/// `tools/spatial_oracle/locomotor_can_use_track.json` (Drive `0x004B4B00`,
/// Ship `0x006A4130`, executed from the retail image) against the Rust port.
/// The Ship family runs over the Drive tables VERA shares between the two
/// locomotors; a Ship-only divergence would surface here as a failed case.
#[test]
fn occupant_can_use_track_matches_native_oracle() {
    use crate::sim::components::TrackProgress;
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/locomotor_can_use_track.json"
    ))
    .unwrap();
    let mut checked = 0;
    for family in ["drive", "ship"] {
        for case in corpus[family].as_array().unwrap() {
            let track = TrackProgress {
                turn_index: case["turn"].as_i64().unwrap() as i32,
                cursor: case["cursor"].as_i64().unwrap() as i32,
                reversed: case["reversed"].as_bool().unwrap(),
                residual: 0,
            };
            let head = match case["head"].as_i64().unwrap() {
                -1 => None,
                head => Some(head as u8),
            };
            let expected = case["answer"].as_i64().unwrap() != 0;
            assert_eq!(
                occupant_can_use_track(&track, head),
                expected,
                "{family} case {case}"
            );
            checked += 1;
        }
    }
    assert_eq!(checked, 6400 + 5920);
}

#[test]
fn bunker_install_force_tracks_present_with_diagonal_targets() {
    // Tank-bunker install approach curves (0x43 NE / 0x44 SE / 0x45 SW / 0x46 NW),
    // siblings of the refinery-exit track 0x47. Each uses raw track 14 and ends at
    // a diagonal facing — the octant→track map depends on these target facings.
    for (idx, target) in [(0x43u8, 0x20u8), (0x44, 0x60), (0x45, 0xA0), (0x46, 0xE0)] {
        let tt = turn_track_at(idx as usize).expect("install track present");
        assert_eq!(tt.normal_track, 14, "track 0x{idx:02X} raw index");
        assert_eq!(tt.target_facing, target, "track 0x{idx:02X} target facing");
    }
}

#[test]
fn turn_tracks_have_valid_raw_track_indices() {
    for (i, tt) in TURN_TRACKS.iter().enumerate() {
        assert!(
            (tt.normal_track as usize) < RAW_TRACKS.len(),
            "TurnTrack[{}].normal_track={} exceeds RAW_TRACKS len {}",
            i,
            tt.normal_track,
            RAW_TRACKS.len()
        );
        assert!(
            (tt.short_track as usize) < RAW_TRACKS.len(),
            "TurnTrack[{}].short_track={} exceeds RAW_TRACKS len {}",
            i,
            tt.short_track,
            RAW_TRACKS.len()
        );
    }
}

#[test]
fn turn_tracks_count_is_72() {
    assert_eq!(TURN_TRACKS.len(), 72);
}

#[test]
fn raw_tracks_count_is_16() {
    assert_eq!(RAW_TRACKS.len(), 16);
}

#[test]
fn turn_track_0x47_selects_raw_track_15_not_facing_0x47() {
    let turn = turn_track_at(0x47).expect("TurnTrack 0x47 exists");
    assert_eq!(select_raw_track_index(turn, false), 15);
    assert_eq!(select_raw_track_index(turn, true), 15);
    assert_eq!(turn.target_facing, 0xC0);
    assert_ne!(turn.target_facing, 0x47);
    assert_eq!(turn.flags & 0x07, 0);

    let meta = raw_track_meta(15).expect("RawTrack 15 exists");
    assert_eq!(meta.points_count, 16);
    assert_eq!(meta.entry_index, 0);
    assert_eq!(meta.chain_index, -1);
    assert_eq!(meta.occupation_handoff_point_index, -1);

    let points = raw_track_points(15);
    let first = points.first().expect("Track 15 first point");
    let last = points.last().expect("Track 15 last point");
    assert_eq!((first.x, first.y, first.facing), (128, -128, 0x80));
    assert_eq!((last.x, last.y, last.facing), (16, -4, 0xBC));
}

#[test]
fn raw_track_0_is_empty() {
    let track = &RAW_TRACKS[0];
    assert_eq!(track.points_count, 0);
}

#[test]
fn raw_track_1_is_straight_north() {
    let points = raw_track_points(1);
    assert_eq!(points.len(), 23);
    // All points have x=0 (straight), face=0 (north)
    for (i, p) in points.iter().enumerate() {
        assert_eq!(p.x, 0, "Track1 point {} should have x=0", i);
        assert_eq!(p.facing, 0, "Track1 point {} should face north", i);
    }
    // Y should decrease (moving northward)
    assert!(
        points[0].y > points[22].y,
        "Track1 Y should decrease (northward)"
    );
    // First point near cell edge, last near/past center
    assert!(points[0].y > 200, "Track1 starts near cell edge");
    assert!(points[22].y < 10, "Track1 ends near/past cell center");
}

#[test]
fn track1_y_step_is_consistent() {
    let points = raw_track_points(1);
    // Each step decreases Y by ~11 leptons
    for i in 1..points.len() {
        let step = points[i - 1].y - points[i].y;
        assert!(
            step >= 10 && step <= 12,
            "Track1 step {} has Y delta={}, expected ~11",
            i,
            step
        );
    }
}

#[test]
fn raw_track_2_is_straight_ne_diagonal() {
    let points = raw_track_points(2);
    assert_eq!(points.len(), 31);
    // All points face NE (0x20 = 32)
    for (i, p) in points.iter().enumerate() {
        assert_eq!(p.facing, 32, "Track2 point {} should face NE (32)", i);
    }
    // Retail Raw2 has (-129,129) at point15, not a uniform (-128,128)
    // midpoint. Saved original points: locomotor_track_cursor.json raw_tracks.
    for i in 1..points.len() {
        let dx = points[i].x - points[i - 1].x;
        let dy = points[i - 1].y - points[i].y;
        let step = match i {
            15 => 7,
            16 => 9,
            _ => 8,
        };
        assert_eq!(dx, step, "Track2 point {i} X step");
        assert_eq!(dy, step, "Track2 point {i} Y step");
    }
    assert_eq!(points[0].x, -248);
    assert_eq!(points[0].y, 248);
    assert_eq!(points[30].x, -8);
    assert_eq!(points[30].y, 8);
}

#[test]
fn raw_track_3_is_north_to_ne_curve() {
    let points = raw_track_points(3);
    assert_eq!(points.len(), 54);
    // Phase 1 (0-13): straight north, x=-256, face=0
    for i in 0..=13 {
        assert_eq!(points[i].x, -256, "Track3 phase1 point {} x", i);
        assert_eq!(points[i].facing, 0, "Track3 phase1 point {} face", i);
    }
    // Entry index is 12: vehicle starts here
    assert_eq!(points[12].y, 373);
    // Phase 2 (14-36): turning — face increases from 1 toward 31
    assert_eq!(points[14].facing, 1, "first turn point");
    assert_eq!(points[36].facing, 31, "last turn point before jump");
    // Jump index 37: face reaches 32 (NE), cell transition
    assert_eq!(points[37].facing, 32, "jump point faces NE");
    assert_eq!(points[37].x, -136);
    assert_eq!(points[37].y, 136);
    // Phase 3 (37-53): straight NE exit, face=32
    for i in 37..=53 {
        assert_eq!(points[i].facing, 32, "Track3 phase3 point {} face", i);
    }
    // Final point near origin (sentinel removed, last real point is index 53)
    assert_eq!(points[53].x, -8);
    assert_eq!(points[53].y, 8);
}

#[test]
fn select_drive_track_ne_diagonal_gives_track_2() {
    // Facing NE (32), moving NE (32) → entry 9: normal_track=2 (straight diagonal).
    let sel = select_drive_track(32, 32, false);
    assert!(sel.is_some(), "NE diagonal should find Track 2");
    let sel = sel.unwrap();
    assert_eq!(
        sel.raw_track_index, 2,
        "should be Track 2 (straight diagonal)"
    );
}

#[test]
fn gsi_04_05_turning_tracks_preserve_valid_occupation_handoff_metadata() {
    // RawTrack +0x0C is the occupation handoff point, not object-list crossing.
    for idx in 3..=6 {
        let track = &RAW_TRACKS[idx];
        assert!(
            track.occupation_handoff_point_index >= 0,
            "RawTrack[{}] should have a non-negative occupation handoff point, got {}",
            idx,
            track.occupation_handoff_point_index
        );
        assert!(
            track.occupation_handoff_point_index < track.points_count as i16,
            "RawTrack[{}] occupation handoff point {} exceeds points_count {}",
            idx,
            track.occupation_handoff_point_index,
            track.points_count
        );
        assert!(
            track.entry_index < track.points_count,
            "RawTrack[{}] entry_index {} exceeds points_count {}",
            idx,
            track.entry_index,
            track.points_count
        );
    }
}

#[test]
fn select_raw_track_index_picks_correct_variant() {
    let tt = &TURN_TRACKS[1]; // normal=3, short=7
    assert_eq!(select_raw_track_index(tt, false), 3);
    assert_eq!(select_raw_track_index(tt, true), 7);
}

#[test]
fn turn_track_lookup_in_range() {
    assert!(turn_track_at(0).is_some());
    assert!(turn_track_at(71).is_some());
    assert!(turn_track_at(72).is_none());
}

// ---------------------------------------------------------------------------
// facing_to_dir tests
// ---------------------------------------------------------------------------

#[test]
fn facing_to_dir_quantizes_8_directions() {
    // Exact facing boundaries: 0=N, 32=NE, 64=E, 96=SE, 128=S, 160=SW, 192=W, 224=NW
    assert_eq!(facing_to_dir(0), 0, "0 → N (dir 0)");
    assert_eq!(facing_to_dir(32), 1, "32 → NE (dir 1)");
    assert_eq!(facing_to_dir(64), 2, "64 → E (dir 2)");
    assert_eq!(facing_to_dir(96), 3, "96 → SE (dir 3)");
    assert_eq!(facing_to_dir(128), 4, "128 → S (dir 4)");
    assert_eq!(facing_to_dir(160), 5, "160 → SW (dir 5)");
    assert_eq!(facing_to_dir(192), 6, "192 → W (dir 6)");
    assert_eq!(facing_to_dir(224), 7, "224 → NW (dir 7)");
}

#[test]
fn facing_to_dir_rounds_near_boundaries() {
    // 15 is within 16 of 0 → should round to N (dir 0)
    assert_eq!(facing_to_dir(15), 0, "15 → N (rounds down)");
    // 16 is the boundary, rounds to NE (dir 1)
    assert_eq!(facing_to_dir(16), 1, "16 → NE (rounds up)");
    // 240 is 16 units from 224 (NW) and 16 units from 256/0 (N)
    // 240 + 16 = 256 = 0 wrapping, 0 / 32 = 0 → N
    assert_eq!(facing_to_dir(240), 0, "240 → N (wraps around)");
    // 241 wraps to 1, 1/32 = 0 → N
    assert_eq!(facing_to_dir(241), 0, "241 → N (wraps around)");
}

// ---------------------------------------------------------------------------
// select_drive_track tests
// ---------------------------------------------------------------------------

#[test]
fn select_drive_track_straight_north_gives_track_1() {
    // Facing N (0), moving N (0) → entry 0: normal_track=1 (straight north).
    // Track 1 has point data → should succeed.
    let sel = select_drive_track(0, 0, false);
    // Entry 0 has normal_track=1, but no facing change means same dir.
    // Actually entry 0 is straight ahead — it should give Track 1.
    assert!(sel.is_some(), "straight north should find Track 1");
    let sel = sel.unwrap();
    assert_eq!(sel.raw_track_index, 1, "should be Track 1 (straight north)");
    assert_eq!(sel.target_facing, 0x00, "target facing should be 0 (north)");
}

#[test]
fn select_drive_track_null_track_returns_none() {
    // Facing N (0), moving SE (96) → entry 3: normal_track=0 (null).
    // Too sharp a turn — should return None.
    let sel = select_drive_track(0, 96, false);
    assert!(sel.is_none(), "135° turn should return None (null track)");
}

#[test]
fn select_drive_track_north_to_ne_gives_track_3() {
    // Facing N (0), moving NE (32) → entry 1: normal_track=3 (turning curve A).
    let sel = select_drive_track(0, 32, false);
    assert!(sel.is_some(), "N→NE slight turn should give Track 3");
    let sel = sel.unwrap();
    assert_eq!(sel.raw_track_index, 3);
    assert_eq!(sel.target_facing, 0x20);
    assert_eq!(sel.chain_index, 37);
    assert_eq!(sel.occupation_handoff_point_index, 22);
    assert_eq!(sel.entry_index, 12);
}

#[test]
fn select_drive_track_all_cardinal_straights_give_track_1() {
    // All 8 cardinal/diagonal straight-ahead cases should resolve to Track 1 or 2.
    // Cardinals (N, E, S, W) → Track 1; diagonals (NE, SE, SW, NW) → Track 2.
    for facing in [0u8, 64, 128, 192] {
        let sel = select_drive_track(facing, facing, false);
        assert!(
            sel.is_some(),
            "cardinal facing {} should have a track",
            facing
        );
        assert_eq!(
            sel.unwrap().raw_track_index,
            1,
            "cardinal {} → Track 1",
            facing
        );
    }
    for facing in [32u8, 96, 160, 224] {
        let sel = select_drive_track(facing, facing, false);
        assert!(
            sel.is_some(),
            "diagonal facing {} should have a track",
            facing
        );
        assert_eq!(
            sel.unwrap().raw_track_index,
            2,
            "diagonal {} → Track 2",
            facing
        );
    }
}

// ---------------------------------------------------------------------------
// build_sharp_turn_fallback tests
// ---------------------------------------------------------------------------

#[test]
fn build_sharp_turn_fallback_cardinals_use_raw_track_1() {
    for facing in [0u8, 64, 128, 192] {
        let fb = build_sharp_turn_fallback(facing)
            .unwrap_or_else(|| panic!("fallback should exist for cardinal facing {}", facing));
        assert_eq!(
            fb.raw_track_index, 1,
            "cardinal facing {} should use RawTrack 1 (straight)",
            facing
        );
    }
}

#[test]
fn build_sharp_turn_fallback_diagonals_use_raw_track_2() {
    for facing in [32u8, 96, 160, 224] {
        let fb = build_sharp_turn_fallback(facing)
            .unwrap_or_else(|| panic!("fallback should exist for diagonal facing {}", facing));
        assert_eq!(
            fb.raw_track_index, 2,
            "diagonal facing {} should use RawTrack 2 (straight diagonal)",
            facing
        );
    }
}

#[test]
fn build_sharp_turn_fallback_transform_flags_match_binary() {
    // Verified-from-binary: cur_dir → low3 of TURN_TRACKS[cur_dir*9].flags
    //   N=0, NE=0, E=3, SE=4, S=4, SW=1, W=1, NW=2
    let cases: &[(u8, u8)] = &[
        (0, 0),   // N
        (32, 0),  // NE
        (64, 3),  // E
        (96, 4),  // SE
        (128, 4), // S
        (160, 1), // SW
        (192, 1), // W
        (224, 2), // NW
    ];
    for &(facing, expected_low3) in cases {
        let fb = build_sharp_turn_fallback(facing).unwrap();
        assert_eq!(
            fb.flags & 0x07,
            expected_low3,
            "facing {} should have transform flags low3 = {}",
            facing,
            expected_low3
        );
    }
}

#[test]
fn build_sharp_turn_fallback_target_facing_matches_quantized_cur_dir() {
    let cases: &[(u8, u8)] = &[
        (0, 0x00),
        (32, 0x20),
        (64, 0x40),
        (96, 0x60),
        (128, 0x80),
        (160, 0xA0),
        (192, 0xC0),
        (224, 0xE0),
    ];
    for &(facing, expected_target) in cases {
        let fb = build_sharp_turn_fallback(facing).unwrap();
        assert_eq!(
            fb.target_facing, expected_target,
            "facing {} substitute should have target_facing 0x{:02X}",
            facing, expected_target
        );
    }
}

#[test]
fn build_sharp_turn_fallback_rounds_to_nearest_dir() {
    // Non-quantized facings round to the nearest 8-direction bucket.
    let fb_17 = build_sharp_turn_fallback(17).unwrap();
    let fb_32 = build_sharp_turn_fallback(32).unwrap();
    assert_eq!(fb_17.raw_track_index, fb_32.raw_track_index);
    assert_eq!(fb_17.flags, fb_32.flags);
    assert_eq!(fb_17.target_facing, fb_32.target_facing);
}

#[test]
fn sharp_turn_fallback_produces_valid_track_for_all_8_dirs() {
    use crate::util::fixed_math::dir_to_cell_delta;
    for facing in [0u8, 32, 64, 96, 128, 160, 192, 224] {
        let fallback = build_sharp_turn_fallback(facing).unwrap();
        let delta = dir_to_cell_delta(facing);
        let plan = expect_plan(facing, delta, None);
        assert_eq!(plan.selection.raw_track_index, fallback.raw_track_index);
        assert_eq!(plan.selection.flags, fallback.flags);
        assert_eq!(plan.selection.target_facing, fallback.target_facing);
        let position = crate::sim::components::Position {
            rx: 10,
            ry: 10,
            z: 0,
            exact_z_leptons: Some(731),
            sub_x: crate::util::fixed_math::SimFixed::from_num(85),
            sub_y: crate::util::fixed_math::SimFixed::from_num(153),
        };
        let head = super::super::track_head::begin_fresh(&plan, &position).unwrap();
        assert_eq!(head.x, 10 * 256 + 85 + delta.0 * 256);
        assert_eq!(head.y, 10 * 256 + 153 + delta.1 * 256);
        assert_eq!(head.z, 731);
    }
}

#[test]
fn raw_track_4_is_north_to_east_90_degree() {
    let points = raw_track_points(4);
    assert_eq!(points.len(), 38);
    // Starts facing north (0), ends facing east (64)
    assert_eq!(points[0].facing, 0);
    assert_eq!(points[37].facing, 64);
    // Entry at point 11
    assert_eq!(points[11].facing, 5);
    // Jump at point 26: face near 64 (east)
    assert_eq!(points[26].facing, 60);
    // Phase 3 exit: face=64 (east), y≈0
    for i in 29..=37 {
        assert!(
            points[i].facing >= 64,
            "Track4 exit point {} face={}",
            i,
            points[i].facing
        );
        assert!(
            points[i].y <= 9,
            "Track4 exit point {} y={}",
            i,
            points[i].y
        );
    }
}

// ---------------------------------------------------------------------------
// GSI-06.13 — the path-window selection basis
// ---------------------------------------------------------------------------

/// Facing bytes used by the fixtures. `0x00`=N, `0x40`=E, `0x80`=S, `0xC0`=W.
const FACE_E: u8 = 0x40;
const FACE_S: u8 = 0x80;
const FACE_W: u8 = 0xC0;

#[test]
fn fresh_heading_gate_and_facing_setter_match_original_native_rows() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/drive_fresh_turn.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 120);
    let mut sub_byte_refusals = 0;
    for row in rows {
        let input = &row["input"];
        let initial = input["initial"].as_u64().unwrap() as u16;
        let direction = input["direction"].as_u64().unwrap() as usize;
        let call = &row["calls"][0];
        let decision =
            plan_drive_track_from_path(initial, OCTANT_CELL_DELTA[direction], None, false);
        let refused = matches!(decision, DriveTrackDecision::TurnFirst { .. });
        assert_eq!(refused, call["boundary"] == "turn_then_return", "{input}");
        if refused && initial >> 8 == (direction as u16) << 5 {
            sub_byte_refusals += 1;
        }
        let rate = input["rate"].as_i64().unwrap();
        if rate < 0 {
            // A supplied raw negative Facing rate is not produced by SetROT.
            continue;
        }
        let mut facing =
            crate::sim::movement::facing_class::FacingClass::new(initial, (rate >> 8) as u8);
        if refused {
            facing.set((direction as u16) << 13, 2);
        }
        for call in row["calls"].as_array().unwrap() {
            assert_eq!(
                u64::from(facing.current(call["frame"].as_u64().unwrap() as u32)),
                call["sampled_after"].as_u64().unwrap(),
                "{input}"
            );
        }
    }
    assert_eq!(sub_byte_refusals, 10);
}

fn expect_plan(body_facing: u8, from: (i32, i32), to: Option<(i32, i32)>) -> DriveTrackPlan {
    match plan_drive_track_from_path(u16::from(body_facing) << 8, from, to, false) {
        DriveTrackDecision::Select(plan) => plan,
        other => panic!("expected a curve, got {other:?}"),
    }
}

/// Fixture A. Tank at (10,10) facing E, path [(10,10), (11,10), (11,11)].
/// gamemd indexes `path[1]_dir + path[0]_dir * 8` = S + E*8 = 4 + 16 = 20, whose
/// flags carry the "turns" bit, so the curve spans two cells and reserves
/// (11,11) — not the straight-east entry 18 with a one-cell head that the body
/// facing would have produced.
#[test]
fn gsi_06_13_path_window_indexes_from_the_two_leading_path_directions() {
    let plan = expect_plan(FACE_E, (1, 0), Some((0, 1)));
    assert_eq!(plan.selection.turn_track_index, 20, "E->S turn table entry");
    assert_eq!(plan.selection.raw_track_index, 4, "90 degree curve");
    assert_eq!(plan.selection.flags, 11);
    assert_eq!(plan.selection.target_facing, FACE_S);
    assert_eq!(
        (plan.head_dx, plan.head_dy),
        (1, 1),
        "the reserved head is the two-cell endpoint (11,11)"
    );
    assert!(
        plan.spans_two_nodes(),
        "turning curve consumes two path nodes"
    );
}

/// Fixture B. Tank at (20,20) facing E, path [(20,20), (21,20), (22,21)] —
/// a 45 degree kink. Entry 19 (E->SE), raw track 3, head two cells at (22,21).
#[test]
fn gsi_06_13_forty_five_degree_kink_uses_entry_19_and_a_two_cell_head() {
    let plan = expect_plan(FACE_E, (1, 0), Some((1, 1)));
    assert_eq!(plan.selection.turn_track_index, 19);
    assert_eq!(plan.selection.raw_track_index, 3);
    assert_eq!(plan.selection.flags, 11);
    assert_eq!((plan.head_dx, plan.head_dy), (2, 1));
    assert!(plan.spans_two_nodes());
}

/// Fixture C. Tank at (30,30) facing E, path [(30,30), (31,30), (30,31)] —
/// a three-octant kink. Entry 21 (E->SW) is a null curve, so gamemd falls back
/// to `path[0]_dir * 9` = 18: a straight run along the *head node's* direction,
/// one node, head (31,30). The mover still drives to its real next cell.
#[test]
fn gsi_06_13_null_curve_falls_back_to_head_node_direction_not_body_facing() {
    let plan = expect_plan(FACE_E, (1, 0), Some((-1, 1)));
    assert_eq!(plan.selection.turn_track_index, 18, "E*9 straight entry");
    assert_eq!(plan.selection.raw_track_index, 1);
    assert_eq!(plan.selection.target_facing, FACE_E);
    assert_eq!(
        (plan.head_dx, plan.head_dy),
        (1, 0),
        "fallback still heads for the real path node, never a synthesized cell"
    );
    assert!(!plan.spans_two_nodes());
}

/// The last step of a path has no successor direction; gamemd's `-1` queue
/// terminator normalises `to := from`, giving the straight entry.
#[test]
fn gsi_06_13_last_step_normalises_to_the_straight_entry() {
    let plan = expect_plan(FACE_E, (1, 0), None);
    assert_eq!(plan.selection.turn_track_index, 18);
    assert!(!plan.spans_two_nodes());
    assert_eq!((plan.head_dx, plan.head_dy), (1, 0));
}

/// The exact-facing precondition. gamemd compares the body facing against
/// `path[0]_dir << 13` with zero tolerance and, on any difference, commands the
/// turn and returns without selecting a curve or consuming a node.
#[test]
fn gsi_06_13_body_off_the_head_octant_turns_before_any_selection() {
    match plan_drive_track_from_path(u16::from(FACE_W) << 8, (1, 0), Some((0, 1)), false) {
        DriveTrackDecision::TurnFirst { desired_facing } => {
            assert_eq!(desired_facing, FACE_E, "turn onto the head node's octant");
        }
        other => panic!("expected TurnFirst, got {other:?}"),
    }
    // One facing unit off is still off — the comparison has no tolerance.
    assert!(matches!(
        plan_drive_track_from_path((u16::from(FACE_E) << 8) + 1, (1, 0), Some((0, 1)), false),
        DriveTrackDecision::TurnFirst { .. }
    ));
}

/// Every entry the selector can pick agrees with the table: the "turns" flag is
/// set exactly when the two path directions differ, and the head is two cells
/// exactly then.
#[test]
fn gsi_06_13_turns_flag_and_head_span_agree_across_all_direction_pairs() {
    for from_dir in 0..8usize {
        let from = OCTANT_CELL_DELTA[from_dir];
        let body = (from_dir as u8) * 0x20;
        for to_dir in 0..8usize {
            let plan = expect_plan(body, from, Some(OCTANT_CELL_DELTA[to_dir]));
            let turn = &TURN_TRACKS[plan.selection.turn_track_index];
            let turns = turn.flags & TURN_TRACK_TURNS_FLAG != 0;
            assert_eq!(
                turns,
                plan.spans_two_nodes(),
                "from {from_dir} to {to_dir}: flag 8 must drive the node count"
            );
            let (to_dx, to_dy) = OCTANT_CELL_DELTA[to_dir];
            let expected_head = if turns {
                (from.0 + to_dx, from.1 + to_dy)
            } else {
                from
            };
            assert_eq!(
                (plan.head_dx, plan.head_dy),
                expected_head,
                "from {from_dir} to {to_dir}: head cell"
            );
            // Every selection either uses the ordinary entry or the from*9
            // straight; nothing else is reachable.
            assert!(
                plan.selection.turn_track_index == from_dir * 8 + to_dir
                    || plan.selection.turn_track_index == from_dir * 9
            );
        }
    }
}

/// The selection finalize resets the track cursor to 0. The lead-in points are
/// what carry the mover from its own cell centre into the arc: with the two-cell
/// head the first point of the E->S curve lands inside the mover's current cell,
/// which is only true at cursor 0.
#[test]
fn gsi_06_13_selected_curve_starts_at_the_movers_own_cell_centre() {
    let plan = expect_plan(FACE_E, (1, 0), Some((0, 1)));
    let position = crate::sim::components::Position {
        rx: 0,
        ry: 0,
        z: 0,
        exact_z_leptons: None,
        sub_x: crate::util::lepton::CELL_CENTER_LEPTON,
        sub_y: crate::util::lepton::CELL_CENTER_LEPTON,
    };
    let head = super::super::track_head::begin_fresh(&plan, &position).unwrap();
    let points = raw_track_points(plan.selection.raw_track_index);
    let (tx, ty, tf) = transform_track_point(
        points[0].x,
        points[0].y,
        points[0].facing,
        plan.selection.flags,
    );
    let sub_x = head.x + i32::from(tx);
    let sub_y = head.y + i32::from(ty);
    assert!(
        (0..256).contains(&sub_x) && (0..256).contains(&sub_y),
        "curve point 0 must sit in the mover's own cell, got ({sub_x},{sub_y})"
    );
    assert_eq!(tf, FACE_E, "the curve begins on the entry facing");
    // ... and its last point lands on the head cell, two cells away.
    let last = points.len() - 1;
    let (lx, ly, lf) = transform_track_point(
        points[last].x,
        points[last].y,
        points[last].facing,
        plan.selection.flags,
    );
    assert_eq!(
        (
            (head.x + i32::from(lx)).div_euclid(256),
            (head.y + i32::from(ly)).div_euclid(256),
        ),
        (1, 1),
        "the curve ends on the two-cell endpoint"
    );
    assert_eq!(lf, FACE_S, "and on the table's target facing");
}

/// Every `TURN_TRACKS` and `RAW_TRACKS` entry, as gamemd.exe stores them.
///
/// Read from the retail binary (SHA-256 `1cdd1180...4298c`) at `0x007E7B28`
/// (72 entries of 12 bytes) and `0x007E7A28` (16 entries of 16 bytes). Native
/// layout: TurnTrack byte +0x00 normal track, +0x01 short track, dword +0x04
/// target facing, dword +0x08 flags; RawTrack dword +0x00 points pointer,
/// +0x04 chain index, +0x08 entry index, +0x0C occupation handoff index, the
/// last three signed with -1 meaning none.
///
/// The A3 examination sampled 8 of the 72 TurnTrack entries and 6 of the 16
/// RawTrack entries and carried the remainder as a coverage limit. This pins
/// all of both, so an edit to either table has to answer to the binary.
///
/// The pointer column is deliberately absent: VERA stores the points inline
/// through `points_start`/`points_count`, so there is no address to compare.
#[test]
fn track_tables_match_the_retail_bytes_entry_for_entry() {
    // (normal_track, short_track, target_facing, flags)
    const NATIVE_TURN_TRACKS: [(u8, u8, u8, u8); 72] = [
        (1, 0, 0x00, 0x00),
        (3, 7, 0x20, 0x08),
        (4, 9, 0x40, 0x08),
        (0, 0, 0x60, 0x00),
        (0, 0, 0x80, 0x00),
        (0, 0, 0xA0, 0x00),
        (4, 9, 0xC0, 0x0A),
        (3, 7, 0xE0, 0x0A),
        (6, 8, 0x00, 0x0F),
        (2, 0, 0x20, 0x00),
        (6, 8, 0x40, 0x08),
        (5, 10, 0x60, 0x08),
        (0, 0, 0x80, 0x00),
        (0, 0, 0xA0, 0x00),
        (0, 0, 0xC0, 0x00),
        (5, 10, 0xE0, 0x0F),
        (4, 9, 0x00, 0x0F),
        (3, 7, 0x20, 0x0F),
        (1, 0, 0x40, 0x03),
        (3, 7, 0x60, 0x0B),
        (4, 9, 0x80, 0x0B),
        (0, 0, 0xA0, 0x00),
        (0, 0, 0xC0, 0x00),
        (0, 0, 0xE0, 0x00),
        (0, 0, 0x00, 0x00),
        (5, 10, 0x20, 0x0C),
        (6, 8, 0x40, 0x0C),
        (2, 0, 0x60, 0x04),
        (6, 8, 0x80, 0x0B),
        (5, 10, 0xA0, 0x0B),
        (0, 0, 0xC0, 0x00),
        (0, 0, 0xE0, 0x00),
        (0, 0, 0x00, 0x00),
        (0, 0, 0x20, 0x00),
        (4, 9, 0x40, 0x0C),
        (3, 7, 0x60, 0x0C),
        (1, 0, 0x80, 0x04),
        (3, 7, 0xA0, 0x0E),
        (4, 9, 0xC0, 0x0E),
        (0, 0, 0xE0, 0x00),
        (0, 0, 0x00, 0x00),
        (0, 0, 0x20, 0x00),
        (0, 0, 0x40, 0x00),
        (5, 10, 0x60, 0x09),
        (6, 8, 0x80, 0x09),
        (2, 0, 0xA0, 0x01),
        (6, 8, 0xC0, 0x0E),
        (5, 10, 0xE0, 0x0E),
        (4, 9, 0x00, 0x0D),
        (0, 0, 0x20, 0x00),
        (0, 0, 0x40, 0x00),
        (0, 0, 0x60, 0x00),
        (4, 9, 0x80, 0x09),
        (3, 7, 0xA0, 0x09),
        (1, 0, 0xC0, 0x01),
        (3, 7, 0xE0, 0x0D),
        (6, 8, 0x00, 0x0D),
        (5, 10, 0x20, 0x0D),
        (0, 0, 0x40, 0x00),
        (0, 0, 0x60, 0x00),
        (0, 0, 0x80, 0x00),
        (5, 10, 0xA0, 0x0A),
        (6, 8, 0xC0, 0x0A),
        (2, 0, 0xE0, 0x02),
        (11, 11, 0xA0, 0x00),
        (12, 12, 0xA0, 0x00),
        (13, 13, 0xA0, 0x00),
        (14, 14, 0x20, 0x00),
        (14, 14, 0x60, 0x04),
        (14, 14, 0xA0, 0x01),
        (14, 14, 0xE0, 0x02),
        (15, 15, 0xC0, 0x00),
    ];
    // (chain_index, entry_index, occupation_handoff_point_index)
    const NATIVE_RAW_TRACKS: [(i16, u16, i16); 16] = [
        (0, 192, 0),
        (-1, 0, -1),
        (-1, 0, -1),
        (37, 12, 22),
        (26, 11, 19),
        (45, 15, 31),
        (44, 16, 27),
        (-1, 0, -1),
        (-1, 0, -1),
        (-1, 0, -1),
        (-1, 0, -1),
        (-1, 0, -1),
        (-1, 0, -1),
        (-1, 0, -1),
        (-1, 0, -1),
        (-1, 0, -1),
    ];

    for (index, native) in NATIVE_TURN_TRACKS.iter().enumerate() {
        let ours = &TURN_TRACKS[index];
        assert_eq!(
            (
                ours.normal_track,
                ours.short_track,
                ours.target_facing,
                ours.flags
            ),
            *native,
            "TURN_TRACKS[{index}] disagrees with gamemd 0x007E7B28 + {index} * 12",
        );
    }
    for (index, native) in NATIVE_RAW_TRACKS.iter().enumerate() {
        let ours = &RAW_TRACKS[index];
        assert_eq!(
            (
                ours.chain_index,
                ours.entry_index,
                ours.occupation_handoff_point_index
            ),
            *native,
            "RAW_TRACKS[{index}] disagrees with gamemd 0x007E7A28 + {index} * 16",
        );
    }
}

/// The immutable catalog omits the separately paid terminal sentinel; no
/// noninitial real point may masquerade as that zero-XY terminator.
#[test]
fn no_shipped_track_holds_an_interior_sentinel_point() {
    for index in 1..=15u8 {
        for (slot, point) in raw_track_points(index).iter().enumerate().skip(1) {
            assert!(
                point.x != 0 || point.y != 0,
                "track {index} slot {slot} is an interior zero-XY terminator"
            );
        }
    }
}

/// Drive4B49B6 compares RawTrack+0x0C against the retained cursor with JGE.
#[test]
fn occupation_handoff_releases_on_the_retained_native_cursor() {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::components::DriveCoord;
    use crate::sim::movement::at_coord::{AtCoordQuery, AtCoordTrack};
    assert_eq!(
        raw_track_meta(3).unwrap().occupation_handoff_point_index,
        22
    );
    for kind in [LocomotorKind::Drive, LocomotorKind::Ship] {
        let claim_at = |cursor| {
            AtCoordQuery::from_state(
                kind,
                DriveCoord::cell(10, 10, 0),
                Some(DriveCoord::cell(11, 8, 0)),
                AtCoordTrack {
                    turn_index: 1,
                    cursor,
                    reversed: false,
                },
            )
            .unwrap()
            .cells()
            .0
        };
        assert!(claim_at(0).is_some(), "fresh acceptance claims the handoff");
        assert!(claim_at(21).is_some(), "last cursor before handoff");
        assert!(
            claim_at(22).is_none(),
            "the native JGE releases on cursor 22"
        );
    }
}
