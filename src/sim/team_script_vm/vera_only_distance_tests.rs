//! Nearest-candidate kernels on VERA-only maps: the original's 32-bit squared
//! distance wraps past roughly 181 cells per axis, so a far candidate can read
//! as nearer. Maps past the original's `[Map] Size` limit compare the true
//! 64-bit square instead; readable maps keep the native result.

use super::membership::{RecruitCandidate, recruit_pick};
use super::team_ai::{CenterSample, center_of};

/// A 250-cell offset on both axes: `(64_000)² * 2` wraps a signed 32-bit sum
/// negative, while the true square stays far above [`NEAR`].
const NEAR: [i32; 2] = [1_000, 0];
const FAR: [i32; 2] = [64_000, 64_000];

fn center_closest(vera_only_map: bool) -> Option<u64> {
    let samples = [
        CenterSample {
            id: 1,
            xy: NEAR,
            doubled: false,
            naval_transport: false,
        },
        CenterSample {
            id: 2,
            xy: FAR,
            doubled: false,
            naval_transport: false,
        },
    ];
    center_of(samples, Some([0, 0]), false, vera_only_map).and_then(|(_, closest)| closest)
}

fn recruit_winner(vera_only_map: bool) -> Option<u64> {
    let candidates = [
        RecruitCandidate {
            id: 1,
            xy: NEAR,
            group: 0,
            matches: true,
        },
        RecruitCandidate {
            id: 2,
            xy: FAR,
            group: 0,
            matches: true,
        },
    ];
    recruit_pick(candidates, [0, 0], 0, false, |_| true, false, vera_only_map)
}

#[test]
fn calc_center_skips_the_wrapped_far_sample_on_a_vera_only_map() {
    assert_eq!(center_closest(false), Some(2));
    assert_eq!(center_closest(true), Some(1));
}

#[test]
fn recruit_skips_the_wrapped_far_candidate_on_a_vera_only_map() {
    assert_eq!(recruit_winner(false), Some(2));
    assert_eq!(recruit_winner(true), Some(1));
}
