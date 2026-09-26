//! Tests for path smoothing (pass 1: zigzag and run replacement).

use super::*;

// ---- Helper: always-walkable closure ----
fn all_walkable(_x: u16, _y: u16) -> bool {
    true
}

/// Whether a direction index represents a diagonal (NE, SE, SW, NW).
fn is_diagonal_dir(d: u8) -> bool {
    d < 8 && (d & 1) != 0
}

fn blocked_set(blocked: &[(u16, u16)]) -> impl Fn(u16, u16) -> bool + '_ {
    move |x, y| !blocked.contains(&(x, y))
}

// ---- Direction utility tests ----

#[test]
fn direction_between_cardinals() {
    assert_eq!(direction_between((5, 5), (5, 4)), 0); // N
    assert_eq!(direction_between((5, 5), (6, 5)), 2); // E
    assert_eq!(direction_between((5, 5), (5, 6)), 4); // S
    assert_eq!(direction_between((5, 5), (4, 5)), 6); // W
}

#[test]
fn direction_between_diagonals() {
    assert_eq!(direction_between((5, 5), (6, 4)), 1); // NE
    assert_eq!(direction_between((5, 5), (6, 6)), 3); // SE
    assert_eq!(direction_between((5, 5), (4, 6)), 5); // SW
    assert_eq!(direction_between((5, 5), (4, 4)), 7); // NW
}

#[test]
fn direction_between_non_adjacent() {
    assert_eq!(direction_between((5, 5), (7, 5)), DIR_INVALID);
    assert_eq!(direction_between((5, 5), (5, 5)), DIR_INVALID);
}

#[test]
fn dir_diff_same() {
    assert_eq!(dir_diff(0, 0), 0);
    assert_eq!(dir_diff(3, 3), 0);
}

#[test]
fn dir_diff_adjacent() {
    assert_eq!(dir_diff(0, 1), 1); // N ↔ NE
    assert_eq!(dir_diff(7, 0), 1); // NW ↔ N (wraparound)
}

#[test]
fn dir_diff_right_angle() {
    assert_eq!(dir_diff(0, 2), 2); // N ↔ E
    assert_eq!(dir_diff(6, 0), 2); // W ↔ N (wraparound)
    assert_eq!(dir_diff(7, 1), 2); // NW ↔ NE
}

#[test]
fn dir_diff_opposite() {
    assert_eq!(dir_diff(0, 4), 4); // N ↔ S
    assert_eq!(dir_diff(2, 6), 4); // E ↔ W
}

#[test]
fn midpoint_dir_basic() {
    assert_eq!(midpoint_dir(0, 2), 1); // N+E → NE
    assert_eq!(midpoint_dir(2, 4), 3); // E+S → SE
    assert_eq!(midpoint_dir(4, 6), 5); // S+W → SW
}

#[test]
fn midpoint_dir_wraparound() {
    assert_eq!(midpoint_dir(6, 0), 7); // W+N → NW
    assert_eq!(midpoint_dir(7, 1), 0); // NW+NE → N
}

// ---- Pass 1: Zigzag smoothing tests ----

#[test]
fn smooth_straight_path_unchanged() {
    let path = vec![(0, 0), (1, 0), (2, 0), (3, 0)]; // all East
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(result, path);
}

#[test]
fn smooth_diagonal_path_unchanged() {
    let path = vec![(0, 0), (1, 1), (2, 2), (3, 3)]; // all SE
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(result, path);
}

#[test]
fn smooth_cardinal_zigzag_n_then_e_unchanged() {
    // N then E (cardinal→cardinal 90° turn) is NOT smoothed by gamemd.exe —
    // Path_smooth_corners only anchors zigzags on diagonal directions.
    let path = vec![(5, 5), (5, 4), (6, 4)];
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(result, path);
}

#[test]
fn smooth_cardinal_zigzag_e_then_s_unchanged() {
    // E→S is also cardinal→cardinal — binary leaves it alone.
    let path = vec![(5, 5), (6, 5), (6, 6)];
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(result, path);
}

#[test]
fn smooth_zigzag_blocked_shortcut() {
    // The shortcut cell is blocked — path should be unchanged.
    // W(6) then N(0): diff = 2, midpoint = NW(7). Shortcut from (5,5) is (4,4).
    let path = vec![(5, 5), (4, 5), (4, 4)];
    let blocked = blocked_set(&[(4, 4)]);
    let result = smooth_path(path.clone(), &blocked);
    // (4,4) is blocked, so the NW shortcut fails. Path unchanged.
    assert_eq!(result, path);
}

#[test]
fn smooth_zigzag_corner_cutting_blocked() {
    // Diagonal shortcut cell is walkable, but one cardinal neighbor is blocked.
    // S(4) then E(2): diff = 2, midpoint = SE(3). From (5,5), SE = (6,6).
    // Cardinal neighbors from (5,5): S → (5,6), E → (6,5).
    // Block (5,6) — the south cardinal — should prevent the shortcut.
    let path = vec![(5, 5), (5, 6), (6, 6)];
    let blocked = blocked_set(&[(5, 6)]);
    let result = smooth_path(path.clone(), &blocked);
    // (5,6) is on the path but blocked for walkability check — shortcut fails.
    assert_eq!(result, path);
}

#[test]
fn smooth_cardinal_alternation_unchanged() {
    // N-E-N-E alternating cardinals — classic RA2 staircase pattern. gamemd.exe
    // leaves this alone at Pass 1 (only Pass 2 drift correction can straighten it).
    let path = vec![(5, 5), (5, 4), (6, 4), (6, 3), (7, 3)];
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(result, path);
}

#[test]
fn smooth_diagonal_zigzag_ne_then_se_smoothed() {
    // NE(1) then SE(3): diff = 2, both diagonal — smoothed to midpoint E(2).
    // (0,0) → (1,-1) → (2,0) becomes (0,0) → (1,0) → (2,0) via E shortcut.
    // Use (5,5) base so we don't underflow u16.
    let path = vec![(5, 5), (6, 4), (7, 5)];
    let result = smooth_path(path, &all_walkable);
    assert_eq!(result, vec![(5, 5), (6, 5), (7, 5)]);
}

#[test]
fn smooth_45_degree_not_smoothed() {
    // N(0) then NE(1): dir_diff = 1, not 2. Should be unchanged.
    let path = vec![(5, 5), (5, 4), (6, 3)];
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(result, path);
}

#[test]
fn smooth_135_degree_not_smoothed() {
    // N(0) then SW(5): dir_diff = 3 (via min(5, 3)), not 2. Should be unchanged.
    let path = vec![(5, 5), (5, 4), (4, 5)];
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(result, path);
}

#[test]
fn smooth_two_cell_path_unchanged() {
    let path = vec![(5, 5), (6, 5)];
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(result, path);
}

#[test]
fn smooth_single_cell_path_unchanged() {
    let path = vec![(5, 5)];
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(result, path);
}

// ---- Layered path smoothing tests ----

#[test]
fn smooth_layered_same_layer_diagonal_zigzag() {
    // NE(1) then SE(3) — both diagonal — on a single layer, should smooth to E.
    let path = vec![(5, 5), (6, 4), (7, 5)];
    let layers = vec![
        MovementLayer::Ground,
        MovementLayer::Ground,
        MovementLayer::Ground,
    ];
    let (coords, lyrs) = smooth_layered_path(path, layers, &|_x, _y, _l| true);
    assert_eq!(coords, vec![(5, 5), (6, 5), (7, 5)]);
    assert_eq!(
        lyrs,
        vec![
            MovementLayer::Ground,
            MovementLayer::Ground,
            MovementLayer::Ground
        ]
    );
}

#[test]
fn smooth_layered_cardinal_unchanged() {
    // Cardinal-cardinal zigzag on same layer — not smoothed (matches binary).
    let path = vec![(5, 5), (5, 4), (6, 4)];
    let layers = vec![
        MovementLayer::Ground,
        MovementLayer::Ground,
        MovementLayer::Ground,
    ];
    let (coords, _lyrs) = smooth_layered_path(path.clone(), layers, &|_x, _y, _l| true);
    assert_eq!(coords, path);
}

#[test]
fn smooth_layered_skips_layer_transition() {
    // Zigzag crosses a Ground→Bridge transition — should NOT be smoothed.
    let path = vec![(5, 5), (5, 4), (6, 4)];
    let layers = vec![
        MovementLayer::Ground,
        MovementLayer::Bridge,
        MovementLayer::Bridge,
    ];
    let (coords, _lyrs) = smooth_layered_path(path.clone(), layers, &|_x, _y, _l| true);
    // Path unchanged because layers differ at the zigzag point.
    assert_eq!(coords, path);
}

// ---- Pass 1: run-based replacement (Path_smooth_single_segment) ----

#[test]
fn smooth_replaces_two_times_min_run_and_zigzag() {
    // 3 x NE then 2 x SE. m = min(3, 2) = 2, mid = E, write offset = run - m = 1,
    // write count = 2m = 4. gamemd re-lays the whole run pair; replacing a single
    // cell would leave the first two NE steps and only move one interior cell.
    let path = vec![(5, 10), (6, 9), (7, 8), (8, 7), (9, 8), (10, 9)];
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(
        result,
        vec![(5, 10), (6, 9), (7, 9), (8, 9), (9, 9), (10, 9)]
    );
    // Step count and endpoint are preserved by construction.
    assert_eq!(result.len(), path.len());
    assert_eq!(result.last(), path.last());
    assert_eq!(result.first(), path.first());
}

#[test]
fn smooth_degrades_to_fewer_pairs_when_full_replacement_is_blocked() {
    // Same geometry; (7,9) is the first cell the m = 2 replacement needs, so that
    // attempt fails and gamemd retries with m = 1 starting one step later:
    // offset becomes 2 and the replacement covers (8,8), (9,8).
    let path = vec![(5, 10), (6, 9), (7, 8), (8, 7), (9, 8), (10, 9)];
    let blocked = blocked_set(&[(7, 9)]);
    let result = smooth_path(path.clone(), &blocked);
    assert_eq!(
        result,
        vec![(5, 10), (6, 9), (7, 8), (8, 8), (9, 8), (10, 9)]
    );
    assert_eq!(result.len(), path.len());
    assert_eq!(result.last(), path.last());
}

#[test]
fn smooth_keeps_zigzag_when_every_replacement_size_is_blocked() {
    // Blocking the first cell of both the m = 2 and the m = 1 attempt exhausts
    // the degrade loop, and the original zigzag stands.
    let path = vec![(5, 10), (6, 9), (7, 8), (8, 7), (9, 8), (10, 9)];
    let blocked = blocked_set(&[(7, 9), (8, 8)]);
    let result = smooth_path(path.clone(), &blocked);
    assert_eq!(result, path);
}

#[test]
fn smooth_preserves_step_count_and_endpoint_for_equal_runs() {
    // 2 x SE then 2 x NE. m = 2, mid = E, offset = 0, 4 replacement steps.
    let path = vec![(2, 2), (3, 3), (4, 4), (5, 3), (6, 2)];
    let result = smooth_path(path.clone(), &all_walkable);
    assert_eq!(result, vec![(2, 2), (3, 2), (4, 2), (5, 2), (6, 2)]);
    assert_eq!(result.len(), path.len());
    assert_eq!(result.last(), path.last());
}

#[test]
fn segment_midpoint_dir_matches_reference_midpoint() {
    // The literal native form and the wheel-average reference agree on all four
    // legal diagonal pairs, including the {NW, NE} wrap that folds to N.
    for &(a, b) in &[
        (1u8, 3u8),
        (3, 1),
        (3, 5),
        (5, 3),
        (5, 7),
        (7, 5),
        (7, 1),
        (1, 7),
    ] {
        assert_eq!(
            segment_midpoint_dir(a, b),
            midpoint_dir(a, b),
            "a={a} b={b}"
        );
        assert!(!is_diagonal_dir(segment_midpoint_dir(a, b)));
        assert_eq!(dir_diff(a, b), 2);
    }
}
