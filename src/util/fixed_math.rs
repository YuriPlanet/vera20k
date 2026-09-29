//! Deterministic fixed-point math for the simulation layer.
//!
//! All sim-critical arithmetic uses `SimFixed` (`I16F16`) instead of `f32`.
//! This guarantees identical results across CPU architectures, compilers,
//! and optimization levels — required for lockstep multiplayer and replays.
//!
//! ## Type
//! - `SimFixed` = `FixedI32<U16>` — 16 integer bits, 16 fractional bits
//!   - Range: −32768.0 to +32767.99998
//!   - Precision: ~0.000015 (1/65536)
//!
//! ## Dependency rules
//! - util/ has NO dependencies on other game modules.

use fixed::types::I16F16;

/// Primary simulation fixed-point type: 16 integer bits, 16 fractional bits.
/// Backed by `FixedI32<U16>` (aliased as `I16F16` in the `fixed` crate).
///
/// Range: −32768.0 to +32767.99998. Precision: ~0.000015 (1/65536).
/// Sufficient for cell coordinates (0–511), speeds (0–60), altitudes (0–1200),
/// damage multipliers (0.0–2.0), and timer durations (0–30s).
pub type SimFixed = I16F16;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Zero.
pub const SIM_ZERO: SimFixed = SimFixed::ZERO;

/// One.
pub const SIM_ONE: SimFixed = SimFixed::ONE;

/// One half (0.5).
pub const SIM_HALF: SimFixed = SimFixed::lit("0.5");

/// Two (2.0).
pub const SIM_TWO: SimFixed = SimFixed::lit("2");

/// One and a half (1.5) — used for jumpjet deceleration multiplier.
pub const SIM_1_5: SimFixed = SimFixed::lit("1.5");

/// Smallest representable positive value (1/65536 ≈ 0.000015).
pub const SIM_EPSILON: SimFixed = SimFixed::DELTA;

/// Duration of one native movement frame, in seconds.
///
/// Retail locomotors integrate their per-second values once for each 15 Hz
/// game frame. Keeping this fraction here gives every locomotor the same
/// deterministic fixed-point rounding.
#[inline]
pub fn native_movement_frame_fraction() -> SimFixed {
    SIM_ONE / SimFixed::from_num(RA2_LOGIC_FRAMES_PER_SECOND as u8)
}

/// RA2's native game-logic frame rate at normal speed: 15 logic frames per
/// second. The single authority for every INI minutes/seconds->frames
/// conversion (ROF, ReloadRate, C4Delay, ore growth, trigger time, ...).
pub const RA2_LOGIC_FRAMES_PER_SECOND: u32 = 15;

/// VERA's fixed simulation step rate in Hz (22 ms per tick on the client;
/// see `app::types::SIM_TICK_MS`). NOT the native logic-frame rate — INI
/// timing conversions use `RA2_LOGIC_FRAMES_PER_SECOND` above.
pub const SIM_TICK_HZ: u32 = 45;

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Truncate `SimFixed` to `i32` (rounds toward zero).
#[inline]
pub fn sim_to_i32(val: SimFixed) -> i32 {
    val.to_num::<i32>()
}

/// Convert `SimFixed` to `f32` for render-layer output.
/// Only use at the sim→render boundary, never within sim logic.
#[inline]
pub fn sim_to_f32(val: SimFixed) -> f32 {
    val.to_num::<f32>()
}

/// Convert `f32` to `SimFixed` for loading INI data into the sim layer.
/// Only use at data-load boundaries (rules.ini, art.ini parsing).
#[inline]
pub fn sim_from_f32(val: f32) -> SimFixed {
    SimFixed::from_num(val)
}

/// Convert `f64` to `SimFixed` for intermediate calculations during data loading.
/// Only use at data-load boundaries — never within sim tick logic.
#[cfg(test)]
#[inline]
pub fn sim_from_f64(val: f64) -> SimFixed {
    SimFixed::from_num(val)
}

// ---------------------------------------------------------------------------
// Math helpers
// ---------------------------------------------------------------------------

/// Euclidean distance for SimFixed values: `sqrt(dx² + dy²)`.
///
/// Widens to `I48F16` (i64-backed) for the intermediate `dx*dx + dy*dy` to
/// avoid I16F16 overflow when dx or dy exceed ~181. The result (a distance)
/// always fits back in SimFixed — e.g., max subcell delta 256 gives
/// `sqrt(256² + 256²) ≈ 362`, well within I16F16 range.
pub fn fixed_distance(dx: SimFixed, dy: SimFixed) -> SimFixed {
    use fixed::types::I48F16;
    let dx_w = I48F16::from(dx);
    let dy_w = I48F16::from(dy);
    let sum = dx_w * dx_w + dy_w * dy_w;
    if sum <= I48F16::ZERO {
        return SIM_ZERO;
    }
    // Newton's method sqrt in the wider type.
    let two = I48F16::from_num(2u8);
    let mut guess = if sum < two { sum } else { sum / two };
    for _ in 0..16 {
        if guess <= I48F16::ZERO {
            return SIM_ZERO;
        }
        guess = (guess + sum / guess) / two;
    }
    // Both types use 16 fractional bits, so truncate the backing i64→i32.
    SimFixed::from_bits(guess.to_bits() as i32)
}

/// Integer-based Euclidean distance: `sqrt(dx*dx + dy*dy)` returned as SimFixed.
///
/// Uses `i32` arithmetic for squaring to avoid I16F16 overflow on large maps.
/// SimFixed (I16F16) max integer is 32,767, so `dx * dx` overflows when `dx > 181`.
/// By computing in i32 (max ~2 billion), this handles maps up to ~46,000 cells.
///
/// The *result* (the distance itself, not the squared distance) always fits in
/// SimFixed — e.g., a 500x500 diagonal is ~707 cells, well within 32,767.
pub fn int_distance_to_sim(dx: i32, dy: i32) -> SimFixed {
    let sum: i32 = dx * dx + dy * dy;
    if sum <= 0 {
        return SIM_ZERO;
    }
    // Newton's method integer sqrt.
    let mut guess: i32 = sum;
    for _ in 0..16 {
        guess = (guess + sum / guess) / 2;
    }
    SimFixed::from_num(guess)
}

/// Integer square root of an `i64` via Newton's method.
///
/// Returns `sqrt(val)` as `i64`. Used for lepton-space distance where the
/// squared distance exceeds i32 range. 32 iterations guarantees convergence
/// for the full i64 positive range.
///
/// Returns 0 for zero or negative inputs.
pub fn isqrt_i64(val: i64) -> i64 {
    if val <= 0 {
        return 0;
    }
    // Initial guess: use bit-length to pick a reasonable start.
    // guess = 1 << ((bit_length(val) + 1) / 2)
    let bits: u32 = 64 - (val as u64).leading_zeros();
    let mut guess: i64 = 1i64 << ((bits + 1) / 2);
    for _ in 0..32 {
        let next: i64 = (guess + val / guess) / 2;
        if next >= guess {
            break;
        }
        guess = next;
    }
    guess
}

/// Facing calculation from a screen-relative coordinate delta.
///
/// Returns the high byte of the active-retail full-word conversion. Computed
/// east and south are therefore 63 and 127; authored quarter-turn facings
/// remain the exact table values 64 and 128.
pub fn facing_from_delta_int(dx: i32, dy: i32) -> u8 {
    crate::util::direction_tables::facing8_from_delta(dx, dy)
}

/// Full-word facing from a screen-relative coordinate delta.
///
/// This preserves the native lookup, float-store, quadrant, and 65,534-scale
/// boundaries used before a value enters `FacingClass`.
pub fn facing_from_delta_int_u16(dx: i32, dy: i32) -> u16 {
    crate::util::direction_tables::facing16_from_delta(dx, dy)
}

/// Inverse of `facing_from_delta_int` for quantized 8-direction facings.
///
/// Quantizes the input facing to the nearest of 8 compass points (32
/// facing units apart) and returns the unit cell delta for that
/// direction. Used by movement code that needs to step one cell in
/// the unit's current facing without a path waypoint.
///
/// Iso-grid convention (matches the canonical `DIR_DELTAS` in
/// `sim/pathfinding/path_smooth.rs`):
///   N (0)   → ( 0, -1)
///   NE (32) → ( 1, -1)
///   E (64)  → ( 1,  0)
///   SE (96) → ( 1,  1)
///   S (128) → ( 0,  1)
///   SW (160)→ (-1,  1)
///   W (192) → (-1,  0)
///   NW (224)→ (-1, -1)
pub fn dir_to_cell_delta(facing: u8) -> (i32, i32) {
    crate::util::direction::delta_from_facing(facing)
}

// ---------------------------------------------------------------------------
// RA2 speed conversion
// ---------------------------------------------------------------------------

/// TechnoType ReadINI71465F..71469F: clamp authored Speed to0..100,
/// multiply by256/100 and cap at255 whole leptons per native frame.
/// A missing/-1 override retains native type initialization; callers here
/// resolve that to their existing zero default before conversion.
pub(crate) fn ra2_speed_to_leptons_per_frame(raw_speed: i32) -> i32 {
    (raw_speed.clamp(0, 100) * 256 / 100).min(255)
}

/// Convert a rules.ini `Speed=` value to cells per second using the authentic
/// RA2 formula.
///
/// RA2 internally computes speed as leptons per game frame at 15 FPS:
///   1. Cap Speed at 100 (values above 100 are treated as 100).
///   2. `leptons_per_tick = min(Speed * 256 / 100, 255)` — leptons per frame.
///   3. Convert to cells/second: `leptons_per_tick * 15 / 256`.
///
/// The result is a `SimFixed` value in cells per second, suitable for the
/// movement system's `progress += speed * dt` formula.
///
/// Examples: Speed=4 (HARV) → ~0.586 cells/sec, Speed=6 (MTNK) → ~0.879,
/// Speed=11 (E1) → ~1.641, Speed=100 → ~14.941. Speed=0 → 0 (immobile).
#[cfg(test)]
pub fn ra2_speed_to_cells_per_second(raw_speed: i32) -> SimFixed {
    let leptons_per_tick = ra2_speed_to_leptons_per_frame(raw_speed);
    SimFixed::from_num(leptons_per_tick * 15) / SimFixed::from_num(256)
}

/// Convert a rules.ini `Speed=` value to leptons per second using the authentic
/// RA2 formula.
///
/// Same computation as `ra2_speed_to_cells_per_second()` but without the final
/// `/256` division. The result is in leptons/second (256× larger), suitable for
/// the lepton-based movement system where progress counts to 256 per cell.
///
/// Examples: Speed=4 (HARV) → ~150 lep/sec, Speed=11 (E1) → ~420 lep/sec,
/// Speed=100 → ~3825 lep/sec. Speed=0 → 0 (immobile).
pub fn ra2_speed_to_leptons_per_second(raw_speed: i32) -> SimFixed {
    // Conversion: leptons_per_tick = speed * 256 / 100, capped at 255.
    // leptons_per_second = leptons_per_tick * 15 (baseline = gamemd Slowest).
    let leptons_per_tick = ra2_speed_to_leptons_per_frame(raw_speed);
    SimFixed::from_num(leptons_per_tick * 15)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(SIM_ZERO, SimFixed::from_num(0));
        assert_eq!(SIM_ONE, SimFixed::from_num(1));
        let half: f32 = SIM_HALF.to_num();
        assert!((half - 0.5).abs() < 0.001);
    }

    // -----------------------------------------------------------------------
    // Facing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_facing_cardinals() {
        // Computed directions use the native 65,534-unit scale before exposing
        // the high byte. Authored quarter turns remain 64/128/192.
        assert_eq!(facing_from_delta_int(0, -1), 0);
        assert_eq!(facing_from_delta_int(1, 0), 63);
        assert_eq!(facing_from_delta_int(0, 1), 127);
        assert_eq!(facing_from_delta_int(-1, 0), 192);
    }

    #[test]
    fn test_facing_diagonals() {
        // Iso diagonals map to screen diagonals.
        // (1,-1) = NE on screen → facing 32
        let ne: u8 = facing_from_delta_int(1, -1);
        assert!((ne as i16 - 32).abs() <= 1, "NE facing={ne}");
        // (1,1) = SE on screen → facing 96
        let se: u8 = facing_from_delta_int(1, 1);
        assert!((se as i16 - 96).abs() <= 1, "SE facing={se}");
        // (-1,1) = SW on screen → facing 160
        let sw: u8 = facing_from_delta_int(-1, 1);
        assert!((sw as i16 - 160).abs() <= 1, "SW facing={sw}");
        // (-1,-1) = NW on screen → facing 224
        let nw: u8 = facing_from_delta_int(-1, -1);
        assert!(nw >= 223 || nw <= 1, "NW facing={nw}");
    }

    #[test]
    fn test_facing_zero_delta() {
        assert_eq!(facing_from_delta_int(0, 0), 63);
    }

    /// Verify facing produces sane quadrant values for a grid of deltas.
    /// +dx = east, -dy = north, so (dx>0,dy<0) = NE quadrant.
    #[test]
    fn test_facing_quadrants() {
        // NE quadrant (dx>0, dy<0): facing 0..64 (N..E)
        for dx in 1..=5 {
            for dy in -5..=-1 {
                let f: u8 = facing_from_delta_int(dx, dy);
                assert!((0..=64).contains(&f), "NE: dx={dx}, dy={dy} -> {f}");
            }
        }
        // SE quadrant (dx>0, dy>0): facing 64..128 (E..S)
        for dx in 1..=5 {
            for dy in 1..=5 {
                let f: u8 = facing_from_delta_int(dx, dy);
                assert!((64..=128).contains(&f), "SE: dx={dx}, dy={dy} -> {f}");
            }
        }
        // SW quadrant (dx<0, dy>0): facing 128..192 (S..W)
        for dx in -5..=-1 {
            for dy in 1..=5 {
                let f: u8 = facing_from_delta_int(dx, dy);
                assert!((128..=192).contains(&f), "SW: dx={dx}, dy={dy} -> {f}");
            }
        }
        // NW quadrant (dx<0, dy<0): facing 192..256 (W..N, wraps around 0)
        for dx in -5..=-1 {
            for dy in -5..=-1 {
                let f: u8 = facing_from_delta_int(dx, dy);
                assert!(f >= 192 || f == 0, "NW: dx={dx}, dy={dy} -> {f}");
            }
        }
    }

    // -----------------------------------------------------------------------
    // 16-bit facing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_facing_u16_cardinals() {
        assert_eq!(facing_from_delta_int_u16(0, -1), 0);
        assert_eq!(facing_from_delta_int_u16(1, 0), 0x3fff);
        assert_eq!(facing_from_delta_int_u16(0, 1), 0x7fff);
        assert_eq!(facing_from_delta_int_u16(-1, 0), 0xc001);
    }

    #[test]
    fn test_facing_u16_diagonals() {
        let ne: u16 = facing_from_delta_int_u16(1, -1);
        assert_eq!(ne, 8_315);
        let se: u16 = facing_from_delta_int_u16(1, 1);
        assert_eq!(se, 24_451);
        let sw: u16 = facing_from_delta_int_u16(-1, 1);
        assert_eq!(sw, 41_082);
        let nw: u16 = facing_from_delta_int_u16(-1, -1);
        assert_eq!(nw, 57_221);
    }

    #[test]
    fn test_facing_u16_zero_delta() {
        assert_eq!(facing_from_delta_int_u16(0, 0), 0x3fff);
    }

    #[test]
    fn test_facing_u16_consistent_with_u8() {
        // u16 facing >> 8 should match the u8 facing for cardinal/diagonal deltas.
        for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0), (1, -1), (1, 1), (-1, 1)] {
            let f8: u8 = facing_from_delta_int(dx, dy);
            let f16: u16 = facing_from_delta_int_u16(dx, dy);
            let f16_as_u8: u8 = (f16 >> 8) as u8;
            assert!(
                (f8 as i16 - f16_as_u8 as i16).abs() <= 1,
                "dx={dx}, dy={dy}: u8={f8}, u16>>8={f16_as_u8}"
            );
        }
    }

    #[test]
    fn test_facing_u16_lepton_scale() {
        // Lepton-scale deltas should produce the same result as cell-scale for
        // the same direction — atan2 depends on ratio, not magnitude.
        let cell: u16 = facing_from_delta_int_u16(3, 4);
        let lepton: u16 = facing_from_delta_int_u16(3 * 256, 4 * 256);
        assert_eq!(cell, lepton);
    }

    #[test]
    fn test_new_constants() {
        assert_eq!(SIM_TWO, SimFixed::from_num(2));
        let one_five: f32 = SIM_1_5.to_num();
        assert!((one_five - 1.5).abs() < 0.001);
    }

    /// S0 GATE (design Correction 1): pin the ReadDouble->SimFixed quantization.
    /// gamemd computes `(double)(float)sscanf("%f")`, times the binary64 0.01
    /// under its chop control word if the value holds a '%', and keeps the raw
    /// double (`rules::ini_value::parse_read_double`, the production reader).
    /// SimFixed (I16F16) is coarser (16 frac bits), so there is no
    /// "bit-identical to gamemd" target — the gate is the quantization rounding mode.
    /// `SimFixed::from_num` rounds NEAREST-TIES-EVEN (NOT truncate), so the f32-path
    /// and f64-path must land on the SAME 16.16 value over the stock boundary domain.
    #[test]
    fn test_read_double_precision_matches_gamemd() {
        let rows = [
            "0", "1", "7", "0.5", ".9", "0.016", "100%", "50%", "12.5%",
            "-50%", // NEGATIVE + percent (design §10 requires it)
            "10%0", // "%f" reads 10 (stops at '%'), strchr('%') matches
                    // ANYWHERE -> ×0.01 -> 0.1 (plan-review C-R1). NOT 0.0.
        ];
        for s in rows {
            let reference = crate::rules::ini_value::parse_read_double(s);
            let from_f64 = sim_from_f64(reference);
            let from_f32 = sim_from_f32(reference as f32);
            // (a) f32-path vs f64-path agree after 16.16 quantization:
            assert_eq!(
                from_f64, from_f32,
                "path divergence for {s:?}: f64={from_f64} f32={from_f32}"
            );
            // (b) chosen path equals from_num(reference_double):
            assert_eq!(from_f64, SimFixed::from_num(reference), "row {s:?}");
        }
    }

    #[test]
    fn test_int_distance_to_sim_345() {
        // 3-4-5 right triangle.
        let dist: SimFixed = int_distance_to_sim(3, 4);
        assert_eq!(dist, SimFixed::from_num(5));
    }

    #[test]
    fn test_int_distance_to_sim_large_map() {
        // 500x500 diagonal — would overflow SimFixed if done as dx*dx in I16F16.
        let dist: f32 = int_distance_to_sim(500, 500).to_num();
        let expected: f32 = (500.0f32 * 500.0 + 500.0 * 500.0).sqrt(); // ~707.1
        assert!(
            (dist - expected).abs() < 1.0,
            "dist={dist}, expected={expected}"
        );
    }

    #[test]
    fn test_int_distance_to_sim_zero() {
        assert_eq!(int_distance_to_sim(0, 0), SIM_ZERO);
    }

    #[test]
    fn test_int_distance_to_sim_axis_aligned() {
        let dist: SimFixed = int_distance_to_sim(100, 0);
        assert_eq!(dist, SimFixed::from_num(100));
        let dist: SimFixed = int_distance_to_sim(0, -250);
        assert_eq!(dist, SimFixed::from_num(250));
    }

    // -----------------------------------------------------------------------
    // RA2 speed conversion tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_ra2_speed_zero_is_immobile() {
        assert_eq!(ra2_speed_to_cells_per_second(0), SIM_ZERO);
        assert_eq!(ra2_speed_to_cells_per_second(-5), SIM_ZERO);
    }

    #[test]
    fn test_ra2_speed_harvester() {
        // HARV: Speed=4. leptons/tick = 4*256/100 = 10. cells/sec = 10*15/256 ≈ 0.586
        let speed: f32 = ra2_speed_to_cells_per_second(4).to_num();
        assert!(
            (speed - 0.586).abs() < 0.01,
            "Speed=4: got {speed}, expected ~0.586"
        );
    }

    #[test]
    fn test_ra2_speed_medium_tank() {
        // MTNK: Speed=6. leptons/tick = 6*256/100 = 15. cells/sec = 15*15/256 ≈ 0.879
        let speed: f32 = ra2_speed_to_cells_per_second(6).to_num();
        assert!(
            (speed - 0.879).abs() < 0.01,
            "Speed=6: got {speed}, expected ~0.879"
        );
    }

    #[test]
    fn test_ra2_speed_infantry() {
        // E1: Speed=11. leptons/tick = 11*256/100 = 28. cells/sec = 28*15/256 ≈ 1.641
        let speed: f32 = ra2_speed_to_cells_per_second(11).to_num();
        assert!(
            (speed - 1.641).abs() < 0.02,
            "Speed=11: got {speed}, expected ~1.641"
        );
    }

    #[test]
    fn test_ra2_speed_fast_unit() {
        // Speed=40. leptons/tick = 40*256/100 = 102. cells/sec = 102*15/256 ≈ 5.977
        let speed: f32 = ra2_speed_to_cells_per_second(40).to_num();
        assert!(
            (speed - 5.977).abs() < 0.02,
            "Speed=40: got {speed}, expected ~5.977"
        );
    }

    #[test]
    fn test_ra2_speed_max() {
        // Speed=100. leptons/tick = min(100*256/100, 255) = 255. cells/sec = 255*15/256 ≈ 14.941
        let speed: f32 = ra2_speed_to_cells_per_second(100).to_num();
        assert!(
            (speed - 14.941).abs() < 0.02,
            "Speed=100: got {speed}, expected ~14.941"
        );
    }

    #[test]
    fn test_ra2_speed_capped_above_100() {
        // Speed=120 is capped to 100. Same result as Speed=100.
        let s100: SimFixed = ra2_speed_to_cells_per_second(100);
        let s120: SimFixed = ra2_speed_to_cells_per_second(120);
        assert_eq!(
            s100, s120,
            "Speed=120 should be capped to same as Speed=100"
        );
    }

    #[test]
    fn test_ra2_speed_one() {
        // Speed=1. leptons/tick = 1*256/100 = 2. cells/sec = 2*15/256 ≈ 0.117
        let speed: f32 = ra2_speed_to_cells_per_second(1).to_num();
        assert!(
            (speed - 0.117).abs() < 0.01,
            "Speed=1: got {speed}, expected ~0.117"
        );
    }

    // -----------------------------------------------------------------------
    // RA2 lepton speed conversion tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_lepton_speed_is_256x_cell_speed() {
        // Lepton speed should be exactly 256× the cell speed for any input.
        for raw in [1, 4, 6, 11, 40, 100] {
            let cell_speed: SimFixed = ra2_speed_to_cells_per_second(raw);
            let lepton_speed: SimFixed = ra2_speed_to_leptons_per_second(raw);
            let ratio: f32 = lepton_speed.to_num::<f32>() / cell_speed.to_num::<f32>();
            assert!(
                (ratio - 256.0).abs() < 0.1,
                "Speed={raw}: lepton/cell ratio = {ratio}, expected 256.0"
            );
        }
    }

    #[test]
    fn test_lepton_speed_zero_is_immobile() {
        assert_eq!(ra2_speed_to_leptons_per_second(0), SIM_ZERO);
        assert_eq!(ra2_speed_to_leptons_per_second(-5), SIM_ZERO);
    }

    #[test]
    fn test_lepton_speed_harvester() {
        // HARV: Speed=4. leptons/tick = 4*256/100 = 10. leptons/sec = 10*15 = 150.
        let speed: f32 = ra2_speed_to_leptons_per_second(4).to_num();
        assert!(
            (speed - 150.0).abs() < 1.0,
            "Speed=4: got {speed}, expected ~150"
        );
    }

    #[test]
    fn test_lepton_speed_max() {
        // Speed=100. leptons/tick = 255. leptons/sec = 255*15 = 3825
        let speed: f32 = ra2_speed_to_leptons_per_second(100).to_num();
        assert!(
            (speed - 3825.0).abs() < 1.0,
            "Speed=100: got {speed}, expected ~3825"
        );
    }

    // -----------------------------------------------------------------------
    // dir_to_cell_delta tests
    // -----------------------------------------------------------------------

    #[test]
    fn dir_to_cell_delta_quantized_directions() {
        assert_eq!(dir_to_cell_delta(0), (0, -1)); // N
        assert_eq!(dir_to_cell_delta(32), (1, -1)); // NE
        assert_eq!(dir_to_cell_delta(64), (1, 0)); // E
        assert_eq!(dir_to_cell_delta(96), (1, 1)); // SE
        assert_eq!(dir_to_cell_delta(128), (0, 1)); // S
        assert_eq!(dir_to_cell_delta(160), (-1, 1)); // SW
        assert_eq!(dir_to_cell_delta(192), (-1, 0)); // W
        assert_eq!(dir_to_cell_delta(224), (-1, -1)); // NW
    }

    #[test]
    fn dir_to_cell_delta_rounds_to_nearest() {
        // 16 is the boundary, rounds up to NE.
        assert_eq!(dir_to_cell_delta(16), (1, -1));
        assert_eq!(dir_to_cell_delta(17), (1, -1));
        // 240 wraps: (240 + 16) % 256 = 0 → N
        assert_eq!(dir_to_cell_delta(240), (0, -1));
        // 248 wraps: (248 + 16) % 256 = 8 → 8/32 = 0 → N
        assert_eq!(dir_to_cell_delta(248), (0, -1));
    }

    #[test]
    fn dir_to_cell_delta_round_trips_through_facing_from_delta() {
        for &f in &[0u8, 32, 64, 96, 128, 160, 192, 224] {
            let (dx, dy) = dir_to_cell_delta(f);
            let recovered = facing_from_delta_int(dx, dy);
            let diff = (recovered as i16 - f as i16).rem_euclid(256);
            let dist = diff.min(256 - diff);
            assert!(
                dist <= 4,
                "facing {} → ({},{}) → {} (dist {})",
                f,
                dx,
                dy,
                recovered,
                dist
            );
        }
    }
}
