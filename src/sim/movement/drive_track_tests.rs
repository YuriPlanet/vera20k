//! Tests for drive track data validation and lookup functions.

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
fn track_3_begin_starts_at_entry_12() {
    let state = begin_drive_track(3, 0, 1, -1, 0x20).unwrap();
    assert_eq!(state.point_index, 12, "Track 3 entry_index is 12");
}

#[test]
fn drive_track_budget_equal_seven_does_not_consume_point() {
    let mut state = begin_drive_track_with_head_offset(1, 0, 0, 0, 0).unwrap();
    let start_index = state.point_index;
    let mut residual = 7;

    let advance = advance_drive_track_with_budget(&mut state, 0, &mut residual);

    assert_eq!(state.point_index, start_index);
    assert_eq!(state.residual, 7);
    assert_eq!(residual, 7);
    assert!(!advance.finished);
    assert!(!advance.cell_jump);
}

#[test]
fn drive_runtime_residual_carries_across_track_ticks() {
    let mut state = begin_drive_track_with_head_offset(1, 0, 0, 0, 0).unwrap();
    let start_index = state.point_index;
    let mut residual = 6;

    let advance = advance_drive_track_with_budget(&mut state, 2, &mut residual);

    assert_eq!(state.point_index, start_index + 1);
    assert_eq!(state.residual, 1);
    assert_eq!(residual, 1);
    assert!(!advance.finished);
    assert!(!advance.cell_jump);
}

#[test]
fn drive_track_finish_preserves_residual_for_same_tick_retry() {
    let mut state = begin_drive_track(15, 0, 0, 0, 0xC0).unwrap();
    let last_index = raw_track_meta(15).unwrap().points_count - 1;
    state.point_index = last_index - 1;
    let mut residual = 0;

    let advance = advance_drive_track_with_budget(&mut state, 20, &mut residual);

    assert!(advance.finished);
    assert_eq!(state.point_index, last_index);
    assert_eq!(state.residual, 13);
    assert_eq!(residual, 13);
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
    // Wiring test: build_sharp_turn_fallback + dir_to_cell_delta +
    // begin_drive_track must combine into a valid DriveTrackState for
    // every quantized current_facing. This is what the
    // configure_motion_after_transition fallback branch does.
    use crate::util::fixed_math::dir_to_cell_delta;
    for facing in [0u8, 32, 64, 96, 128, 160, 192, 224] {
        let fb = build_sharp_turn_fallback(facing)
            .unwrap_or_else(|| panic!("fallback should exist for facing {}", facing));
        let (cdx, cdy) = dir_to_cell_delta(facing);
        let state = begin_drive_track(fb.raw_track_index, fb.flags, cdx, cdy, fb.target_facing);
        assert!(
            state.is_some(),
            "fallback track should initialize for facing {}",
            facing
        );
        let state = state.unwrap();
        assert_eq!(
            state.target_facing, fb.target_facing,
            "DriveTrackState.target_facing should match selection's target_facing for facing {}",
            facing
        );
        // head_offset = head_d * 256 + 128 — verify deltas were applied.
        assert_eq!(
            state.head_offset_x,
            cdx * 256 + 128,
            "head_offset_x for facing {}",
            facing
        );
        assert_eq!(
            state.head_offset_y,
            cdy * 256 + 128,
            "head_offset_y for facing {}",
            facing
        );
    }
}

// ---------------------------------------------------------------------------
// begin_drive_track tests
// ---------------------------------------------------------------------------

#[test]
fn begin_drive_track_1_starts_at_entry() {
    let state = begin_drive_track(1, 0, 0, 0, 0);
    assert!(state.is_some(), "Track 1 should be startable");
    let state = state.unwrap();
    assert_eq!(state.raw_track_index, 1);
    assert_eq!(state.point_index, 0, "Track 1 entry_index is 0");
}

#[test]
fn begin_drive_track_0_returns_none() {
    // Track 0 is the null track (no points).
    let state = begin_drive_track(0, 0, 0, 0, 0);
    assert!(state.is_none(), "Track 0 (null) should not be startable");
}

#[test]
fn begin_drive_track_missing_data_returns_none() {
    // Out-of-range track index (only 0-15 exist) should return None.
    let state = begin_drive_track(16, 0, 0, 0, 0);
    assert!(state.is_none(), "Track with no metadata should return None");
    // Track 5 now has point data and should be startable.
    let state5 = begin_drive_track(5, 0, 1, -1, 0);
    assert!(
        state5.is_some(),
        "Track 5 should be startable (has 61 points)"
    );
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
// advance_drive_track tests
// ---------------------------------------------------------------------------

#[test]
fn advance_drive_track_1_progresses() {
    // Track 1 (straight north) with head_to = one cell north (dx=0, dy=-1).
    // head_offset = (128, -128). Point 0: sub = (128, -128+245=117).
    let mut state = begin_drive_track(1, 0, 0, -1, 0).unwrap();
    let dt = SimFixed::lit("0.066"); // ~66ms tick (15fps)
    let speed = SimFixed::from_num(256); // 256 leptons/sec = 1 cell/sec

    // Advance one tick.
    let result = advance_drive_track(&mut state, speed, dt);
    // Budget = 256 * 0.066 ≈ 16, cost per step = 7, so 2 steps this tick.
    assert!(!result.finished, "track should not be done after 1 tick");
    assert_eq!(
        result.facing, 0,
        "facing should stay 0 (north) on straight track"
    );
    // sub_x should be 128 (center, since track x=0, head_offset_x=128).
    assert_eq!(
        result.sub_x.to_num::<i32>(),
        128,
        "sub_x should be centered"
    );
    // sub_y should be positive and decreasing (northward).
    let sy = result.sub_y.to_num::<i32>();
    assert!(sy > 0 && sy < 120, "sub_y should decrease: got {}", sy);
}

#[test]
fn advance_drive_track_1_completes() {
    // Track 1 (straight north) with head_to one cell north.
    let mut state = begin_drive_track(1, 0, 0, -1, 0).unwrap();
    let dt = SimFixed::lit("0.066");
    let speed = SimFixed::from_num(256);

    // Advance many ticks until track finishes.
    let mut finished = false;
    for _ in 0..100 {
        let result = advance_drive_track(&mut state, speed, dt);
        if result.finished {
            finished = true;
            // Final y point is 3 → sub_y = -128 + 3 = -125.
            // After cell_jump offset (+256): sub_y ≈ 131.
            let sy = result.sub_y.to_num::<i32>();
            assert!(
                sy > 100 && sy < 160,
                "final sub_y should be ~131 after cell offset: got {}",
                sy
            );
            break;
        }
    }
    assert!(finished, "track should complete within 100 ticks");
}

#[test]
fn advance_drive_track_1_cell_jump_fires_once() {
    // Track 1 (straight north) with head_to one cell north.
    // Coordinate-based detection should fire cell_jump exactly once
    // when sub_y crosses below 0 (around step 11 where y drops below 128).
    let mut state = begin_drive_track(1, 0, 0, -1, 0).unwrap();
    let dt = SimFixed::lit("0.066");
    let speed = SimFixed::from_num(256);

    let mut jump_count = 0;
    for _ in 0..100 {
        let result = advance_drive_track(&mut state, speed, dt);
        if result.cell_jump {
            jump_count += 1;
        }
        if result.finished {
            break;
        }
    }
    assert_eq!(
        jump_count, 1,
        "straight north track should cross exactly one cell boundary"
    );
}

#[test]
fn advance_drive_track_strictly_requires_budget_above_step_cost() {
    let mut state = begin_drive_track(15, 0, 0, 0, 0xC0).unwrap();
    let dt = SimFixed::from_num(1);

    let exact = advance_drive_track(&mut state, SimFixed::from_num(7), dt);
    assert_eq!(state.point_index, 0);
    assert_eq!(state.residual, 7);
    assert_eq!(exact.facing, 0x80);
    assert!(!exact.cell_jump);

    let one_over = advance_drive_track(&mut state, SimFixed::from_num(1), dt);
    assert_eq!(state.point_index, 1);
    assert_eq!(state.residual, 1);
    assert_eq!(one_over.facing, 0x84);
}

#[test]
fn advance_drive_track_budget_14_consumes_one_point_not_two() {
    let mut state = begin_drive_track(15, 0, 0, 0, 0xC0).unwrap();
    let result = advance_drive_track(&mut state, SimFixed::from_num(14), SimFixed::from_num(1));

    assert_eq!(state.point_index, 1);
    assert_eq!(state.residual, 7);
    assert_eq!(result.facing, 0x84);
}

#[test]
fn raw_track_15_advances_without_cell_jump_or_chain() {
    let mut state = begin_drive_track(15, 0, 0, 0, 0xC0).unwrap();
    let mut finished = false;
    for _ in 0..32 {
        let result = advance_drive_track(&mut state, SimFixed::from_num(8), SimFixed::from_num(1));
        assert!(!result.cell_jump, "Track 15 must not cross cells");
        assert!(!result.chain_ready, "Track 15 must not chain");
        if result.finished {
            finished = true;
            break;
        }
    }
    assert!(finished, "Track 15 should finish within guard");
}

#[test]
fn raw_track_15_facing_changes_only_when_point_is_consumed() {
    let mut state = begin_drive_track(15, 0, 0, 0, 0xC0).unwrap();

    let residual_only =
        advance_drive_track(&mut state, SimFixed::from_num(6), SimFixed::from_num(1));
    assert_eq!(state.point_index, 0);
    assert_eq!(state.residual, 6);
    assert_eq!(residual_only.facing, 0x80);
    assert_ne!(residual_only.facing, 0x47);

    let consumed = advance_drive_track(&mut state, SimFixed::from_num(2), SimFixed::from_num(1));
    assert_eq!(state.point_index, 1);
    assert_eq!(state.residual, 1);
    assert_eq!(consumed.facing, 0x84);
}

// ---------------------------------------------------------------------------
// interp_sub_step tests
// ---------------------------------------------------------------------------

#[test]
fn interp_sub_step_residual_zero_returns_none() {
    let result = interp_sub_step(
        SimFixed::from_num(128),
        SimFixed::from_num(128),
        14,
        0,
        0,
        true,
    );
    assert_eq!(result, None, "residual=0 must yield no interp");
}

#[test]
fn interp_sub_step_no_next_step_returns_none() {
    let result = interp_sub_step(
        SimFixed::from_num(128),
        SimFixed::from_num(128),
        14,
        0,
        3,
        false,
    );
    assert_eq!(result, None, "had_next_step=false must yield no interp");
}

#[test]
fn interp_sub_step_fraction_at_residual_1() {
    // delta=14, residual=1 → 14 * 1 / 7 = 2.
    let result = interp_sub_step(
        SimFixed::from_num(100),
        SimFixed::from_num(100),
        14,
        0,
        1,
        true,
    )
    .expect("interp should apply");
    assert_eq!(
        result.sub_x,
        SimFixed::from_num(102),
        "saved 100 + 14*1/7=2 → 102"
    );
    assert_eq!(result.sub_y, SimFixed::from_num(100));
}

#[test]
fn interp_sub_step_fraction_at_residual_6() {
    // delta=14, residual=6 → 14 * 6 / 7 = 12.
    let result = interp_sub_step(
        SimFixed::from_num(100),
        SimFixed::from_num(100),
        14,
        0,
        6,
        true,
    )
    .expect("interp should apply");
    assert_eq!(
        result.sub_x,
        SimFixed::from_num(112),
        "saved 100 + 14*6/7=12 → 112"
    );
}

#[test]
fn interp_sub_step_negative_delta_truncates_toward_zero() {
    // delta=-15, residual=3 → -15 * 3 / 7 = -45 / 7 = -6 (truncated from -6.43).
    let result = interp_sub_step(
        SimFixed::from_num(200),
        SimFixed::from_num(100),
        -15,
        0,
        3,
        true,
    )
    .expect("interp should apply");
    assert_eq!(
        result.sub_x,
        SimFixed::from_num(194),
        "saved 200 + (-15)*3/7=-6 → 194 (truncate toward zero on negative)"
    );
}

#[test]
fn interp_sub_step_diagonal_delta() {
    // dx=14, dy=-7, residual=4 → dx*4/7=8, dy*4/7=-4.
    let result = interp_sub_step(
        SimFixed::from_num(100),
        SimFixed::from_num(100),
        14,
        -7,
        4,
        true,
    )
    .expect("interp should apply");
    assert_eq!(result.sub_x, SimFixed::from_num(108));
    assert_eq!(result.sub_y, SimFixed::from_num(96));
}

#[test]
fn interp_sub_step_all_residual_values_monotonic() {
    // For positive delta, sub_x must increase monotonically with residual.
    let mut last = SimFixed::from_num(100);
    for r in 1..=7 {
        let result = interp_sub_step(
            SimFixed::from_num(100),
            SimFixed::from_num(100),
            14,
            0,
            r,
            true,
        )
        .expect("interp should apply");
        assert!(
            result.sub_x > last,
            "residual {} produced sub_x {:?} not greater than previous {:?}",
            r,
            result.sub_x,
            last
        );
        last = result.sub_x;
    }
}

#[test]
fn interp_sub_step_lands_in_saved_cell() {
    // saved=(100, 100), delta=(14, 0), residual=2 → interp_dx=4 → 104.
    // floor_div(100+4, 256) = 0, floor_div(100, 256) = 0. interp_cell == saved.
    let result = interp_sub_step(
        SimFixed::from_num(100),
        SimFixed::from_num(100),
        14,
        0,
        2,
        true,
    )
    .expect("interp should apply");
    assert_eq!(result.sub_x, SimFixed::from_num(104));
}

#[test]
fn interp_sub_step_lands_in_full_step_cell() {
    // saved=(250, 100), full delta=(14, 0). residual=6 → interp_dx = 12.
    // saved_lx + interp_dx = 262 → cell offset (1, 0).
    // saved_lx + full_dx = 264 → cell offset (1, 0).
    // interp_cell == full_cell, so use interp.
    let result = interp_sub_step(
        SimFixed::from_num(250),
        SimFixed::from_num(100),
        14,
        0,
        6,
        true,
    )
    .expect("interp should apply");
    // 250 + 14*6/7 = 250 + 12 = 262.
    assert_eq!(result.sub_x, SimFixed::from_num(262));
}

#[test]
fn interp_sub_step_third_cell_with_high_residual_uses_interp() {
    // Third-cell construction: saved=(0, 0), delta=(770, 0), residual=4.
    // interp_dx = 770*4/7 = 440. saved+interp = 440 → cell offset 1.
    // full_dx = 770. saved+full = 770 → cell offset 3 (770 = 3*256+2).
    // saved cell offset 0, interp 1, full 3. Third-cell case.
    // residual=4 > 3 → use interp despite third-cell classification.
    let result = interp_sub_step(
        SimFixed::from_num(0),
        SimFixed::from_num(0),
        770,
        0,
        4,
        true,
    )
    .expect("interp should apply");
    // residual > 3 → use interp: 0 + 440 = 440.
    assert_eq!(result.sub_x, SimFixed::from_num(440));
}

#[test]
fn interp_sub_step_third_cell_with_low_residual_falls_back() {
    // saved=(0, 0), delta=(2000, 0), residual=2.
    // interp_dx = 2000*2/7 = 571. saved+interp = 571 → cell offset 2.
    // full_dx = 2000. saved+full = 2000 → cell offset 7.
    // saved 0, interp 2, full 7. Third-cell case.
    // residual=2 ≤ 3 → fall back to full-step coords.
    let result = interp_sub_step(
        SimFixed::from_num(0),
        SimFixed::from_num(0),
        2000,
        0,
        2,
        true,
    )
    .expect("interp should apply (fallback path)");
    // L4 fallback: use full-step coords.
    assert_eq!(
        result.sub_x,
        SimFixed::from_num(2000),
        "low residual + third-cell interp must fall back to full-step coords"
    );
}

#[test]
fn interp_sub_step_residual_threshold_is_strict_greater() {
    // residual = 3 must NOT trigger the trust window (gate is > 3, not >= 3).
    // saved=(0, 0), delta=(2000, 0), residual=3.
    // interp_dx = 2000*3/7 = 857. saved+interp = 857 → cell offset 3.
    // full = 2000 → cell offset 7. Third-cell case.
    // residual=3 NOT > 3 → fall back to full.
    let result = interp_sub_step(
        SimFixed::from_num(0),
        SimFixed::from_num(0),
        2000,
        0,
        3,
        true,
    )
    .expect("interp should apply (fallback path)");
    assert_eq!(
        result.sub_x,
        SimFixed::from_num(2000),
        "residual=3 with third-cell interp must fall back (gate is > 3, not >= 3)"
    );
}

#[test]
fn interp_sub_step_residual_4_triggers_trust_window() {
    // Same construction with residual=4 — should now USE interp.
    // interp_dx = 2000*4/7 = 1142. saved+interp = 1142 → cell offset 4.
    // full = 2000 → cell offset 7. Third-cell, residual=4 > 3 → trust window.
    let result = interp_sub_step(
        SimFixed::from_num(0),
        SimFixed::from_num(0),
        2000,
        0,
        4,
        true,
    )
    .expect("interp should apply");
    assert_eq!(
        result.sub_x,
        SimFixed::from_num(1142),
        "residual=4 trust window: use interp despite third-cell"
    );
}

#[test]
fn end_to_end_sub_step_smoothness_no_stalls() {
    // Pick a speed that produces less than one step's worth of budget per tick
    // (budget ≈ 4, step cost = 7). Without interp the vehicle would visibly
    // stall on every "no-step" tick (point_index unchanged, residual carries
    // forward) and snap forward on "step" ticks. With interp, every tick's
    // visual position advances because the residual contributes a fractional
    // offset toward the next track point.
    let mut state = begin_drive_track(1, 0, 0, -1, 0).expect("track 1 exists");
    let dt = SimFixed::lit("0.066");
    let speed = SimFixed::from_num(60); // 60 * 0.066 ≈ 4 leptons/tick budget

    let mut prev_y: Option<SimFixed> = None;
    let mut zero_delta_ticks: i32 = 0;
    let mut tick_count: i32 = 0;

    // 10 ticks ~ 40 budget ~ 5 steps — well below this track's first cell jump.
    for _ in 0..10 {
        let advance = advance_drive_track(&mut state, speed, dt);
        if advance.finished || advance.cell_jump {
            break;
        }

        let mut sub_y = advance.sub_y;
        if let Some(interp) = interp_sub_step(
            advance.sub_x,
            advance.sub_y,
            advance.next_step_delta_x,
            advance.next_step_delta_y,
            state.residual,
            advance.had_next_step,
        ) {
            sub_y = interp.sub_y;
        }

        if let Some(py) = prev_y
            && sub_y == py
        {
            zero_delta_ticks += 1;
        }
        prev_y = Some(sub_y);
        tick_count += 1;
    }

    assert!(
        tick_count >= 8,
        "test setup error: only {} ticks ran before cell_jump/finished",
        tick_count
    );
    // With sub-step interp wired in, every tick should advance because residual
    // varies from one tick to the next (4 leptons added per tick into a step
    // cost of 7 produces a non-trivial residual cycle: 3, 6, 2, 5, 1, 4, 0, ...).
    // Zero-delta ticks happen only when residual lands at 0 after a step (no
    // interp on that frame); allowing up to 2 covers the residual-cycle floor.
    assert!(
        zero_delta_ticks <= 2,
        "{} zero-delta ticks (out of {}) indicates interp didn't fire — \
         vehicle is stalling between steps instead of drifting smoothly",
        zero_delta_ticks,
        tick_count
    );
}

// ---------------------------------------------------------------------------
// GSI-06.13 — the path-window selection basis
// ---------------------------------------------------------------------------

/// Facing bytes used by the fixtures. `0x00`=N, `0x40`=E, `0x80`=S, `0xC0`=W.
const FACE_E: u8 = 0x40;
const FACE_S: u8 = 0x80;
const FACE_W: u8 = 0xC0;

fn expect_plan(body_facing: u8, from: (i32, i32), to: Option<(i32, i32)>) -> DriveTrackPlan {
    match plan_drive_track_from_path(body_facing, from, to, false) {
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
    match plan_drive_track_from_path(FACE_W, (1, 0), Some((0, 1)), false) {
        DriveTrackDecision::TurnFirst { desired_facing } => {
            assert_eq!(desired_facing, FACE_E, "turn onto the head node's octant");
        }
        other => panic!("expected TurnFirst, got {other:?}"),
    }
    // One facing unit off is still off — the comparison has no tolerance.
    assert!(matches!(
        plan_drive_track_from_path(FACE_E + 1, (1, 0), Some((0, 1)), false),
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
    let state = begin_selected_drive_track(&plan).expect("curve installed");
    assert_eq!(state.point_index, 0, "fresh selection starts at cursor 0");
    let points = raw_track_points(state.raw_track_index);
    let (tx, ty, tf) = transform_track_point(
        points[0].x,
        points[0].y,
        points[0].facing,
        state.transform_flags,
    );
    let sub_x = state.head_offset_x + i32::from(tx);
    let sub_y = state.head_offset_y + i32::from(ty);
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
        state.transform_flags,
    );
    assert_eq!(
        (
            (state.head_offset_x + i32::from(lx)).div_euclid(256),
            (state.head_offset_y + i32::from(ly)).div_euclid(256),
        ),
        (1, 1),
        "the curve ends on the two-cell endpoint"
    );
    assert_eq!(lf, FACE_S, "and on the table's target facing");
}

/// Retail Raw2 point15 is (-129,129), so its NE projection about head
/// (384,-128) is (255,1). Point16 is (264,-8): both cell axes change together.
/// The former Rust (-128,128) transcription invented an exact-corner sample
/// and split that crossing. Exercise all four orientations of the corrected
/// catalog through the existing geometric adapter; this is not a full native
/// Process_Track comparison.
#[test]
fn advance_drive_track_2_diagonals_cross_both_axes_together() {
    fn count_boundaries(transform_flags: u8, head_dx: i32, head_dy: i32, facing: u8) -> u32 {
        let mut state = begin_drive_track(2, transform_flags, head_dx, head_dy, facing).unwrap();
        let dt = SimFixed::lit("0.066");
        let speed = SimFixed::from_num(256);
        let mut jumps = 0;
        for _ in 0..200 {
            let result = advance_drive_track(&mut state, speed, dt);
            if result.cell_jump {
                jumps += 1;
            }
            if result.finished {
                break;
            }
        }
        jumps
    }

    assert_eq!(
        count_boundaries(0, 1, -1, 0x20),
        1,
        "NE retail points skip the exact cell corner"
    );
    assert_eq!(
        count_boundaries(1, -1, 1, 0xA0),
        1,
        "SW mirrors the same combined crossing"
    );
    assert_eq!(
        count_boundaries(4, 1, 1, 0x60),
        1,
        "SE straight crosses both axes on one point"
    );
    assert_eq!(
        count_boundaries(2, -1, -1, 0xE0),
        1,
        "NW straight crosses both axes on one point"
    );
}

/// The reported cell delta is the one the coordinate applied, and the crossings
/// of a curve always sum to its head delta.
///
/// This is the contract the caller relies on: moving the mover's cell by the
/// reported delta keeps `cell * 256 + sub` continuous, because the same delta
/// is what shifted `cell_offset_*` inside the stepping loop.
#[test]
fn advance_drive_track_reported_cell_deltas_sum_to_the_head_delta() {
    fn deltas(
        raw: u8,
        transform_flags: u8,
        head_dx: i32,
        head_dy: i32,
        facing: u8,
    ) -> Vec<(i32, i32)> {
        let mut state = begin_drive_track(raw, transform_flags, head_dx, head_dy, facing).unwrap();
        let dt = SimFixed::lit("0.066");
        let speed = SimFixed::from_num(256);
        let mut out = Vec::new();
        for _ in 0..400 {
            let result = advance_drive_track(&mut state, speed, dt);
            if result.cell_jump {
                out.push((result.cell_jump_dx, result.cell_jump_dy));
            } else {
                assert_eq!(
                    (result.cell_jump_dx, result.cell_jump_dy),
                    (0, 0),
                    "no crossing must report a zero delta"
                );
            }
            if result.finished {
                break;
            }
        }
        out
    }

    // All orientations of retail Raw2 cross both axes on a single point.
    assert_eq!(deltas(2, 0, 1, -1, 0x20), vec![(1, -1)]);
    assert_eq!(deltas(2, 1, -1, 1, 0xA0), vec![(-1, 1)]);
    assert_eq!(deltas(2, 4, 1, 1, 0x60), vec![(1, 1)]);
    assert_eq!(deltas(2, 2, -1, -1, 0xE0), vec![(-1, -1)]);
    // The cardinals are single-axis by construction.
    assert_eq!(deltas(1, 0, 0, -1, 0x00), vec![(0, -1)]);
    assert_eq!(deltas(1, 3, 1, 0, 0x40), vec![(1, 0)]);
    assert_eq!(deltas(1, 4, 0, 1, 0x80), vec![(0, 1)]);
    assert_eq!(deltas(1, 1, -1, 0, 0xC0), vec![(-1, 0)]);
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
