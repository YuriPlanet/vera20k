//! Deterministic fixed-point math for the simulation layer.
//!
//! `SimFixed` (`I16F16`) is the default for fractional simulation quantities.
//! Integers and wider intermediates serve other ranges; selected mechanisms
//! preserve native rounding through the shared numeric owners. Determinism also
//! requires explicit conversion/overflow semantics, RNG and update ordering.
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
/// Choose units and bound intermediate values for each mechanism. Absolute
/// world leptons on large maps do not fit; Location stores cell plus sub-cell.
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

/// RA2's native game-logic frame rate at normal speed: 15 logic frames per
/// second. The single authority for every INI minutes/seconds->frames
/// conversion (ROF, ReloadRate, C4Delay, ore growth, trigger time, ...).
pub const RA2_LOGIC_FRAMES_PER_SECOND: u32 = 15;

/// VERA's fixed simulation step rate in Hz (22 ms per tick on the client;
/// see `app::types::SIM_TICK_MS`). NOT the native logic-frame rate — INI
/// timing conversions use `RA2_LOGIC_FRAMES_PER_SECOND` above.
pub const SIM_TICK_HZ: u32 = 45;

// ---------------------------------------------------------------------------
// RA2 speed conversion
// ---------------------------------------------------------------------------

/// TechnoType ReadINI71465F..71469F: clamp authored Speed to0..100,
/// multiply by256/100 and cap at255 whole leptons per native frame.
/// A missing/-1 override retains native type initialization; callers here
/// resolve that to their existing zero default before conversion.
pub fn ra2_speed_to_leptons_per_frame(raw_speed: i32) -> i32 {
    (raw_speed.clamp(0, 100) * 256 / 100).min(255)
}

/// Convert a rules.ini `Speed=` value to leptons per second using the authentic
/// RA2 formula.
///
/// `ra2_speed_to_leptons_per_frame` times the 15-frame second. The result is
/// in leptons/second, suitable for the lepton-based movement system where
/// progress counts to 256 per cell.
///
/// Examples: Speed=4 (HARV) → ~150 lep/sec, Speed=11 (E1) → ~420 lep/sec,
/// Speed=100 → ~3825 lep/sec. Speed=0 → 0 (immobile).
#[cfg(test)]
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
            let from_f64 = SimFixed::from_num(reference);
            let from_f32 = SimFixed::from_num(reference as f32);
            // The f32 and f64 paths agree after 16.16 quantization.
            assert_eq!(
                from_f64, from_f32,
                "path divergence for {s:?}: f64={from_f64} f32={from_f32}"
            );
        }
    }

    // -----------------------------------------------------------------------
    // RA2 speed conversion tests
    // -----------------------------------------------------------------------

    #[test]
    fn leptons_per_frame_clamps_scales_and_caps() {
        // Clamp to 0..100, scale by 256/100 (truncating), cap at 255.
        for (speed, leptons) in [
            (-5, 0),
            (0, 0),
            (1, 2),
            (4, 10),
            (99, 253),
            (100, 255),
            (120, 255),
        ] {
            assert_eq!(
                ra2_speed_to_leptons_per_frame(speed),
                leptons,
                "Speed={speed}"
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
}
