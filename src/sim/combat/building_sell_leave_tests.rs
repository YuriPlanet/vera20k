//! Tests for the `BuildingUndeploy.Leave` relocation pick.

use super::{ThreatRecord, leave_target_cell, strongest_bin, threat_bin, weighted_bins};

/// `GetValue<4>` rounds the 16-bit facing to 16 bins, half up: bin 0 keeps
/// the wrap-around half, bin 15's upper edge hands back to 0.
#[test]
fn threat_bin_rounds_the_facing_word_to_sixteen_bins_half_up() {
    assert_eq!(threat_bin(0), 0);
    assert_eq!(threat_bin(2047), 0, "the first half of the 4096-wide bin");
    assert_eq!(threat_bin(2048), 1);
    assert_eq!(threat_bin(4095), 1);
    assert_eq!(
        threat_bin(4096),
        1,
        "the upper half rounds into the same bin"
    );
    assert_eq!(threat_bin(6143), 1);
    assert_eq!(threat_bin(6144), 2);
    assert_eq!(threat_bin(32768), 8, "half the circle is bin 8");
    assert_eq!(threat_bin(61440), 15);
    assert_eq!(threat_bin(63487), 15);
    assert_eq!(threat_bin(63488), 0, "the last 2048 wrap back to bin 0");
    assert_eq!(threat_bin(65535), 0);
}

/// Every one of the 16 bins is reachable, so the ring is fully covered.
#[test]
fn every_bin_of_the_circle_is_reachable() {
    let mut seen = [false; 16];
    for raw in (0..=u16::MAX).step_by(512) {
        seen[threat_bin(raw)] = true;
    }
    assert_eq!(seen, [true; 16]);
}

/// The kernel is a triangle: a lone spike at bin 0 weighs its own bin 8, the
/// neighbours 7 down to 1, and the antipode is not reached at all.
#[test]
fn a_lone_spike_weighs_a_triangle_across_the_ring() {
    let mut record = ThreatRecord::default();
    record[0] = 1;
    assert_eq!(
        weighted_bins(&record),
        [8, 7, 6, 5, 4, 3, 2, 1, 0, 1, 2, 3, 4, 5, 6, 7]
    );
}

/// The kernel is symmetric and additive, so a uniform record is flat.
#[test]
fn a_uniform_record_weighs_every_bin_the_same() {
    let record = [3i32; 16];
    // Each output sums (8 + 2*(7+6+5+4+3+2+1)) = 64 of the constant 3.
    assert_eq!(weighted_bins(&record), [192; 16]);
}

/// Native keeps `int` wrapping arithmetic (x86 two's complement); a runaway
/// `Cost=` must wrap rather than panic the sim.
#[test]
fn a_runaway_cost_wraps_instead_of_panicking() {
    let mut record = ThreatRecord::default();
    record[0] = i32::MAX;
    let weights = weighted_bins(&record);
    // 8 * i32::MAX wraps to -8 (mod 2^32) for bin 0.
    assert_eq!(weights[0], 8i32.wrapping_mul(i32::MAX));
}

/// The pick is the strict-greater argmax, so a tie keeps the earlier bin.
#[test]
fn a_tie_keeps_the_earlier_bin() {
    let mut record = ThreatRecord::default();
    record[3] = 1;
    record[9] = 1;
    assert_eq!(strongest_bin(&record), Some(3), "bin 3 is visited first");
}

/// The far edge is never reached by a lone spike, so its bin cannot win.
#[test]
fn the_argmax_follows_the_heaviest_bin() {
    let mut record = ThreatRecord::default();
    record[0] = 1;
    assert_eq!(strongest_bin(&record), Some(0));
    record[0] = 0;
    record[5] = 2;
    assert_eq!(strongest_bin(&record), Some(5));
}

/// With no armed non-ally in range the record is empty and the caller has to
/// draw `RandomRanged(0, 15)`.
#[test]
fn an_empty_record_hands_the_decision_to_the_die() {
    assert_eq!(strongest_bin(&ThreatRecord::default()), None);
}

/// `radian = -(((dir - 4) / 16.0) * TwoPi)`, then `X -= 14*cos`, `Y += 14*sin`.
/// The four cardinal bins are exact.
#[test]
fn the_four_cardinal_bins_step_fourteen_cells() {
    assert_eq!(leave_target_cell((100, 200), 0), (100, 214), "bin 0 is +Y");
    assert_eq!(leave_target_cell((100, 200), 4), (86, 200), "bin 4 is -X");
    assert_eq!(leave_target_cell((100, 200), 8), (100, 186), "bin 8 is -Y");
    assert_eq!(
        leave_target_cell((100, 200), 12),
        (114, 200),
        "bin 12 is +X"
    );
}

/// The quarter turn in `(dir - 4)` rotates the threat bin into the step, so
/// consecutive bins move around the compass by 22.5 degrees.
#[test]
fn consecutive_bins_rotate_a_quarter_step_apart() {
    let origin = (0, 0);
    let cells: Vec<(i16, i16)> = (0..16).map(|dir| leave_target_cell(origin, dir)).collect();
    // The eight cardinal/intercardinal bins land on distinct cells.
    let mut unique = cells.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        16,
        "all 16 bins land on distinct cells: {cells:?}"
    );
}

/// The short casts truncate toward zero and the `CellStruct` adds wrap, so a
/// cell near the map edge wraps instead of saturating.
#[test]
fn the_step_wraps_at_the_cell_edges() {
    assert_eq!(leave_target_cell((0, 0), 4), (-14, 0));
    // bin 12 steps +X by 14, which wraps past `i16::MAX`.
    assert_eq!(
        leave_target_cell((i16::MAX, 0), 12),
        (i16::MAX.wrapping_add(14), 0)
    );
    // bin 4 steps -X by 14, which wraps past `i16::MIN`.
    assert_eq!(
        leave_target_cell((i16::MIN, 0), 4),
        (i16::MIN.wrapping_sub(14), 0)
    );
    // bin 8 steps -Y by 14.
    assert_eq!(
        leave_target_cell((0, i16::MIN), 8),
        (0, i16::MIN.wrapping_sub(14))
    );
}
