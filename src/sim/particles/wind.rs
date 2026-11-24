//! Wind drift tables for particle systems.
//!
//! Two table pairs — gas and smoke — indexed by `[General] WindDirection=`
//! (FacingType 0..7: N, NE, E, SE, S, SW, W, NW). The two pairs differ at
//! index 3 (SE) by exactly one unit on the X axis: smoke drifts further east
//! than gas at SE wind. The Y deltas are identical between the two pairs.
//! This asymmetry is intentional in retail and parity-critical — keep both.

/// Gas particles: per-tick X drift (lepton units).
pub const GAS_WIND_DX: [i32; 8] = [0, 2, 2, 1, 0, -2, -2, -2];
/// Gas particles: per-tick Y drift (lepton units).
pub const GAS_WIND_DY: [i32; 8] = [-2, -2, 0, 2, 2, 2, 0, -2];

/// Smoke particles: per-tick X drift (lepton units).
/// Differs from `GAS_WIND_DX` at index 3 (SE): smoke=2, gas=1.
pub const SMOKE_WIND_DX: [i32; 8] = [0, 2, 2, 2, 0, -2, -2, -2];
/// Smoke particles: per-tick Y drift (lepton units).
pub const SMOKE_WIND_DY: [i32; 8] = [-2, -2, 0, 2, 2, 2, 0, -2];
