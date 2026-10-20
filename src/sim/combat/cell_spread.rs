//! Cell-spread tables — gamemd-exact filled-disk cell enumeration for area-of-effect.
//!
//! Embeds gamemd's two cooperating static tables verbatim:
//! - the count table (cumulative cells per integer radius band 0..=11), and
//! - the 369-entry signed cell-offset sweep, in the **exact** order the engine walks it
//!   (scan order is player-observable: it fixes target/ore/wall processing order, which in
//!   turn fixes damage-application order and RNG-consumption order).
//!
//! The offset sweep is transcribed verbatim from the startup initializer (the static table
//! lives in BSS and is zero in the image; the initializer is the only ground truth). The
//! R=11 duplicate entry — index 322 repeats index 319 = `(-3, 11)`, and the mirror `(3, -11)`
//! is never written — is the real gamemd data defect, preserved verbatim.
//! Damage caps its band at10; House50D6D0's campaign placement sweep may
//! visit the first351 entries, including that original duplicate.
//!
//! Pure, read-only, deterministic. No allocation, no state.
//!
//! ## Dependency rules
//! - Part of sim/combat/ — depends only on `crate::util::native_x87`. Never on render/ui/audio/net.

use crate::util::native_x87::X87Chop53;

/// Cumulative filled-disk cell counts per integer radius band 0..=11, matching gamemd's cell-spread
/// count table. `COUNT_TABLE[r]` = number of offset-table entries to walk for radius band `r`.
const COUNT_TABLE: [u32; 12] = [1, 9, 21, 37, 61, 89, 121, 161, 205, 253, 309, 369];

/// gamemd's hand-authored cell-offset sweep, transcribed verbatim from its startup initializer
/// (source citation in the cell-spread substrate study). Each entry is a signed `(dx, dy)` cell
/// offset from the center; index 0 is `(0, 0)` (the impact cell, always scanned first). Order is
/// verbatim and load-bearing. Index 322 == index 319 == `(-3, 11)` and `(3, -11)` is absent — the
/// real R11 data defect, preserved. `rustfmt::skip` keeps the 9-per-row layout stable across rustfmt
/// versions (the values are the contract, not the wrapping).
#[rustfmt::skip]
const OFFSET_TABLE: [(i16, i16); 369] = [
    (0, 0), (1, -1), (0, -1), (-1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1),
    (-1, -2), (0, -2), (1, -2), (-2, -1), (2, -1), (-2, 0), (2, 0), (-2, 1), (2, 1),
    (-1, 2), (0, 2), (1, 2), (-1, -3), (0, -3), (1, -3), (-2, -2), (2, -2), (-3, -1),
    (3, -1), (-3, 0), (3, 0), (-3, 1), (3, 1), (-2, 2), (2, 2), (-1, 3), (0, 3),
    (1, 3), (-1, -4), (0, -4), (1, -4), (-3, -3), (-2, -3), (2, -3), (3, -3), (-3, -2),
    (3, -2), (-4, -1), (4, -1), (-4, 0), (4, 0), (-4, 1), (4, 1), (-3, 2), (3, 2),
    (-3, 3), (-2, 3), (2, 3), (3, 3), (-1, 4), (0, 4), (1, 4), (-1, -5), (0, -5),
    (1, -5), (-3, -4), (-2, -4), (2, -4), (3, -4), (-4, -3), (4, -3), (-4, -2), (4, -2),
    (-5, -1), (5, -1), (-5, 0), (5, 0), (-5, 1), (5, 1), (-4, 2), (4, 2), (-4, 3),
    (4, 3), (-3, 4), (-2, 4), (2, 4), (3, 4), (-1, 5), (0, 5), (1, 5), (-1, -6),
    (0, -6), (1, -6), (-3, -5), (-2, -5), (2, -5), (3, -5), (-4, -4), (4, -4), (-5, -3),
    (5, -3), (-5, -2), (5, -2), (-6, -1), (6, -1), (-6, 0), (6, 0), (-6, 1), (6, 1),
    (-5, 2), (5, 2), (-5, 3), (5, 3), (-4, 4), (4, 4), (-3, 5), (-2, 5), (2, 5),
    (3, 5), (-1, 6), (0, 6), (1, 6), (-1, -7), (0, -7), (1, -7), (-3, -6), (-2, -6),
    (2, -6), (3, -6), (-5, -5), (-4, -5), (4, -5), (5, -5), (-5, -4), (5, -4), (-6, -3),
    (6, -3), (-6, -2), (6, -2), (-7, -1), (7, -1), (-7, 0), (7, 0), (-7, 1), (7, 1),
    (-6, 2), (6, 2), (-6, 3), (6, 3), (-5, 4), (5, 4), (-5, 5), (-4, 5), (4, 5),
    (5, 5), (-3, 6), (-2, 6), (2, 6), (3, 6), (-1, 7), (0, 7), (1, 7), (-1, -8),
    (0, -8), (1, -8), (-3, -7), (-2, -7), (2, -7), (3, -7), (-5, -6), (-4, -6), (4, -6),
    (5, -6), (-6, -5), (6, -5), (-6, -4), (6, -4), (-7, -3), (7, -3), (-7, -2), (7, -2),
    (-8, -1), (8, -1), (-8, 0), (8, 0), (-8, 1), (8, 1), (-7, 2), (7, 2), (-7, 3),
    (7, 3), (-6, 4), (6, 4), (-6, 5), (6, 5), (-5, 6), (-4, 6), (4, 6), (5, 6),
    (-3, 7), (-2, 7), (2, 7), (3, 7), (-1, 8), (0, 8), (1, 8), (-1, -9), (0, -9),
    (1, -9), (-3, -8), (-2, -8), (2, -8), (3, -8), (-5, -7), (-4, -7), (4, -7), (5, -7),
    (-6, -6), (6, -6), (-7, -5), (7, -5), (-7, -4), (7, -4), (-8, -3), (8, -3), (-8, -2),
    (8, -2), (-9, -1), (9, -1), (-9, 0), (9, 0), (-9, 1), (9, 1), (-8, 2), (8, 2),
    (-8, 3), (8, 3), (-7, 4), (7, 4), (-7, 5), (7, 5), (-6, 6), (6, 6), (-5, 7),
    (-4, 7), (4, 7), (5, 7), (-3, 8), (-2, 8), (2, 8), (3, 8), (-1, 9), (0, 9),
    (1, 9), (-1, -10), (0, -10), (1, -10), (-3, -9), (-2, -9), (2, -9), (3, -9), (-5, -8),
    (-4, -8), (4, -8), (5, -8), (-7, -7), (-6, -7), (6, -7), (7, -7), (-7, -6), (7, -6),
    (-8, -5), (8, -5), (-8, -4), (8, -4), (-9, -3), (9, -3), (-9, -2), (9, -2), (-10, -1),
    (10, -1), (-10, 0), (10, 0), (-10, 1), (10, 1), (-9, 2), (9, 2), (-9, 3), (9, 3),
    (-8, 4), (8, 4), (-8, 5), (8, 5), (-7, 6), (7, 6), (-7, 7), (-6, 7), (6, 7),
    (7, 7), (-5, 8), (-4, 8), (4, 8), (5, 8), (-3, 9), (-2, 9), (2, 9), (3, 9),
    (-1, 10), (0, 10), (1, 10), (0, 11), (0, -11), (-1, 11), (1, 11), (-1, -11), (1, -11),
    (-2, 11), (2, 11), (-2, -11), (2, -11), (-3, 11), (3, 11), (-3, -11), (-3, 11), (-4, 9),
    (4, 9), (-4, -9), (4, -9), (-5, 9), (5, 9), (-5, -9), (5, -9), (-6, 8), (6, 8),
    (-6, -8), (6, -8), (-7, 8), (7, 8), (-7, -8), (7, -8), (-8, 7), (8, 7), (-8, -7),
    (8, -7), (-8, 6), (8, 6), (-8, -6), (8, -6), (-9, 5), (9, 5), (-9, -5), (9, -5),
    (-9, 4), (9, 4), (-9, -4), (9, -4), (-10, 3), (10, 3), (-10, -3), (10, -3), (-10, 2),
    (10, 2), (-10, -2), (10, -2), (-11, 1), (11, 1), (-11, -1), (11, -1), (11, 0), (-11, 0),
];

/// Maximum valid index into [`COUNT_TABLE`] (radius band 11). Stock CellSpread never exceeds 10,
/// so band 11 is reachable only with modded `CellSpread > 10`.
const MAX_COUNT_INDEX: usize = COUNT_TABLE.len() - 1;

/// The count-table band `Apply_area_damage` walks: `ftol(CellSpread + 0.99)`
/// (`0x00489592..0x0048959E`: `FLD float [Warhead+0x124]`, `FADD double 0.99`,
/// `Math::ftol`). `cell_spread` is the native binary32 widened exactly. An f64
/// add and truncation give the native chop-53 result for every binary32 below
/// 2^15 ([`tests::splash_band_matches_the_x87_sum_at_every_band_edge`]).
/// Unclamped, as the native read is. A spread at or below zero walks band 0;
/// natively so does any above -1.99, as ftol truncates toward zero, and one at
/// or below -1.99 indexes before the table.
pub fn splash_count_index(cell_spread: f64) -> usize {
    if cell_spread <= 0.0 {
        return 0;
    }
    (cell_spread + 0.99) as usize
}

/// gamemd splash cell sweep: `offset_table[..count_table[ftol(CS + 0.99)]]`, exact order. The
/// count index is clamped to the 12-entry table bound (stock `CS <= 10` never reaches 11; a modded
/// out-of-range value clamps to band 11 rather than reading past the table).
pub fn splash_cells(cell_spread: f64) -> &'static [(i16, i16)] {
    sweep(splash_count_index(cell_spread))
}

/// `ftol(CellSpread)`, the spread in whole cells: `Apply_area_damage`'s
/// airborne query (`0x004893C3`), the radiation site of
/// `BulletClass::Detonate` (`0x00469180`) and the PostMortem delay.
pub fn whole_cells(cell_spread: f64) -> i32 {
    X87Chop53::ftol_f64_low_masked(cell_spread)
}

/// The cells of radius band `band`: the first `count_table[band]` offsets,
/// the band clamped to the table. `PsyDom::MindControlArea @ 0x0053B080`
/// walks them with its own cap of 10 (`0x0053B17C..0x0053B186`).
pub fn sweep(band: usize) -> &'static [(i16, i16)] {
    &OFFSET_TABLE[..COUNT_TABLE[band.min(MAX_COUNT_INDEX)] as usize]
}

/// The sweep through the entry at band `band`'s count, inclusive:
/// `offset_table[..=count_table[band]]`, for the walks that read the count
/// table as their last index rather than their length
/// (`HouseClass::AI_Fire_GenMutator @ 0x00509F60` with band 1, `0x00509FEB`
/// and `0x0050A0AD..0x0050A0BD`: ten cells, the last `(-1, -2)`).
pub fn inclusive_sweep(band: usize) -> &'static [(i16, i16)] {
    let last = (COUNT_TABLE[band.min(MAX_COUNT_INDEX)] as usize).min(OFFSET_TABLE.len() - 1);
    &OFFSET_TABLE[..=last]
}

/// The receiver radius in leptons, `ftol(CellSpread * 256.0f)` (`0x004892DD`):
/// an object is a receiver only if its 3D lepton distance is `<=` this; the cell
/// sweep is a coarse pre-filter. Scaling a binary32 by 256 is exact, so the
/// f64 product is the native one.
pub fn splash_threshold_leptons(cell_spread: f64) -> i64 {
    i64::from(X87Chop53::ftol_f64_low_masked(cell_spread * 256.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_table_exact_gamemd() {
        // matches gamemd's cell-spread count table
        assert_eq!(
            COUNT_TABLE,
            [1, 9, 21, 37, 61, 89, 121, 161, 205, 253, 309, 369]
        );
    }

    #[test]
    fn offset_r1_sweep_exact_order() {
        // NE, N, NW, W, E, SW, S, SE — verified from the initializer body.
        assert_eq!(
            OFFSET_TABLE[1..9],
            [
                (1, -1),
                (0, -1),
                (-1, -1),
                (-1, 0),
                (1, 0),
                (-1, 1),
                (0, 1),
                (1, 1)
            ]
        );
    }

    #[test]
    fn offset_band_starts_exact() {
        let t = &OFFSET_TABLE;
        assert_eq!(t[96], (-4, -4)); // R6 interior — NOT (-5,-4) (the dump doc is stale here)
        assert_eq!(t[121], (-1, -7));
        assert_eq!(t[161], (-1, -8));
        assert_eq!(t[205], (-1, -9));
        assert_eq!(t[253], (-1, -10));
        assert_eq!(t[309], (0, 11));
        assert_eq!(t[368], (-11, 0));
    }

    #[test]
    fn r11_duplicate_preserved_verbatim() {
        let t = &OFFSET_TABLE;
        assert_eq!(t[319], (-3, 11));
        assert_eq!(t[322], (-3, 11));
        assert!(
            !t.contains(&(3, -11)),
            "gamemd never writes the (3,-11) mirror"
        );
    }

    #[test]
    fn full_table_symmetric_except_r11_defect() {
        // Every cell except the R11 defect entries has its point mirror present.
        let t = &OFFSET_TABLE;
        let set: std::collections::HashSet<(i16, i16)> = t.iter().copied().collect();
        for (i, &(dx, dy)) in t.iter().enumerate() {
            if i == 319 || i == 322 || (dx == 0 && dy == 0) {
                continue;
            }
            assert!(
                set.contains(&(-dx, -dy)),
                "idx {i} {:?} missing mirror",
                (dx, dy)
            );
        }
    }

    /// Every binary32 next to a band edge (`N - 0.99` for each band `N` below
    /// 2^15) lands in the band of the native sum: the binary32 loaded, 0.99
    /// added at 53-bit precision with truncation, then `Math::ftol`.
    #[test]
    fn splash_band_matches_the_x87_sum_at_every_band_edge() {
        use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53};
        let addend = X87Chop53::load_f64(NativeF64Bits::from_bits(0x3FEF_AE14_7AE1_47AE))
            .expect("the 0.99 constant at 0x007E5160");
        for band in 1..=(1_u32 << 15) {
            let edge = (f64::from(band) - 0.99) as f32;
            for step in -4..=4 {
                let bits = edge.to_bits().wrapping_add_signed(step);
                let cell_spread = f32::from_bits(bits);
                if cell_spread <= 0.0 {
                    continue;
                }
                let spread =
                    X87Chop53::load_f32(NativeF32Bits::from_bits(bits)).expect("finite binary32");
                let native = X87Chop53::ftol_i32_low_masked(X87Chop53::add(spread, addend));
                assert_eq!(
                    splash_count_index(f64::from(cell_spread)),
                    native as usize,
                    "CS={cell_spread:e}"
                );
            }
        }
    }

    #[test]
    fn splash_cells_count_matches_index() {
        assert_eq!(splash_cells(0.0).len(), 1);
        assert_eq!(splash_cells(0.5).len(), 9);
        assert_eq!(splash_cells(2.0).len(), 21);
        assert_eq!(splash_cells(10.0).len(), 309);
        // out-of-range clamps to band 11, never reads past the table
        assert_eq!(splash_cells(99.0).len(), 369);
    }

    #[test]
    fn splash_threshold_leptons_boundaries() {
        for (cs, want) in [
            (0.0, 0i64),
            (0.5, 128),
            (1.0, 256),
            (2.0, 512),
            (2.5, 640),
            (10.0, 2560),
            (0.1, 25), // ftol(25.6) = 25 — pins truncation direction
        ] {
            assert_eq!(
                splash_threshold_leptons(f64::from(cs as f32)),
                want,
                "CS={cs}"
            );
        }
    }

    #[test]
    fn whole_cells_truncate_toward_zero() {
        for (cs, want) in [(0.0, 0), (0.9, 0), (1.0, 1), (2.5, 2), (10.0, 10)] {
            assert_eq!(whole_cells(f64::from(cs as f32)), want, "CS={cs}");
        }
    }
}
