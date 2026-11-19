//! Phobos `BuildingUndeploy.Leave=`'s relocation pick
//! (`Ext/WarheadType/Detonate.cpp:304-349`).
//!
//! When a warhead packs a building into its `UndeploysInto=` unit, `Leave`
//! sends that unit to a low-threat cell instead of letting it stand where the
//! building stood. The pick is three steps:
//!
//! 1. every armed, non-allied Techno within `CellSpread` 20 adds its type
//!    `Cost=` to the 16-way direction bin it sits in relative to the building
//!    (`pBuilding->GetTargetDirection(pItem).GetValue<4>()`);
//! 2. the ring is smoothed with a triangular kernel and the heaviest bin wins;
//! 3. the building's cell moves ~14 cells along that bin's radian and is
//!    snapped to a deployable cell.
//!
//! This module owns the pure half of that (steps 1's binning, 2 and 3's
//! offset). The scan and the snap live in `super::building_sell`.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on rules/ and sim/ only; never audio/ or ui/.

/// The 16 direction bins `record[...]` accumulates `Cost=` into.
pub(crate) type ThreatRecord = [i32; 16];

/// `DirectionClass::GetValue<4>()` (`YRpp/Dir.h:44-47, 100-111`): round the
/// raw 16-bit facing word to 16 bins, half up.
///
/// The general form is `(((raw >> (16 - N - 1)) + 1) >> 1) & ((1 << N) - 1)`,
/// which the codebase already spells out for `N = 7` in
/// `phobos_jumpjet_tilt::dir_value_128` and for `N = 5` in
/// `phobos_airburst::radial_fire_warp`. Bin 0 keeps the wrap-around half
/// (`raw` in `63488..=65535` and `0..=2047`).
pub(crate) fn threat_bin(raw_facing: u16) -> usize {
    (((u32::from(raw_facing) >> 11).wrapping_add(1)) >> 1) as usize & 0xF
}

/// The triangular smoothing kernel, `Detonate.cpp:318-328`:
///
/// ```text
/// for i in 16..32 {
///     let mut new_costs = 0;
///     for j in -7..8 { new_costs += (8 - |j|) * record[(i + j) & 15]; }
///     if new_costs > costs { dir = i - 16; costs = new_costs; }
/// }
/// ```
///
/// Each output bin is the weighted sum of its 15-wide neighbourhood, the
/// centre counting 8 and the far edges 1. Every product and sum keeps the
/// native `int`'s wrapping arithmetic (x86 two's complement), so a runaway
/// `Cost=` cannot panic the sim.
pub(crate) fn weighted_bins(record: &ThreatRecord) -> [i32; 16] {
    let mut out = [0i32; 16];
    for (d, slot) in out.iter_mut().enumerate() {
        let mut sum = 0i32;
        for j in -7i32..=7 {
            let weight = 8i32.wrapping_sub(j.abs());
            let index = (d as i32).wrapping_add(j).rem_euclid(16) as usize;
            sum = sum.wrapping_add(weight.wrapping_mul(record[index]));
        }
        *slot = sum;
    }
    out
}

/// The bin the undeploy moves along: the strict-greater argmax of
/// [`weighted_bins`] (a tie keeps the earlier bin, exactly as the native
/// `if (newCosts > costs)` does), or `None` when nothing is in range so the
/// caller draws `RandomRanged(0, 15)` (`Detonate.cpp:337-338`).
///
/// Native seeds `dir = 0` and only writes it while `costs > 0`, so an all-zero
/// record leaves the decision to the die.
pub(crate) fn strongest_bin(record: &ThreatRecord) -> Option<usize> {
    let weights = weighted_bins(record);
    let mut best = 0usize;
    let mut costs = 0i32;
    for (d, &value) in weights.iter().enumerate() {
        if value > costs {
            best = d;
            costs = value;
        }
    }
    (costs > 0).then_some(best)
}

/// `Detonate.cpp:338-348`: reverse the chosen bin into a radian and step the
/// cell ~14 cells along it.
///
/// ```text
/// const double radian = -(((dir - 4) / 16.0) * Math::TwoPi);
/// cell.X -= static_cast<short>(14 * cos(radian));
/// cell.Y += static_cast<short>(14 * sin(radian));
/// ```
///
/// The `- 4` quarter turn and the outer negation are both real; the short
/// casts truncate toward zero and the `CellStruct` adds wrap as the native
/// `short` arithmetic does. `libm` (musl) keeps the result identical on every
/// target; native calls the CRT, so an ulp-level difference is a recorded
/// deviation.
pub(crate) fn leave_target_cell(origin: (i16, i16), dir: usize) -> (i16, i16) {
    let radian = -(((dir as f64 - 4.0) / 16.0) * std::f64::consts::TAU);
    let dx = (14.0 * libm::cos(radian)) as i16;
    let dy = (14.0 * libm::sin(radian)) as i16;
    (origin.0.wrapping_sub(dx), origin.1.wrapping_add(dy))
}

#[cfg(all(test, tests_sim_combat_a_c))]
#[path = "building_sell_leave_tests.rs"]
mod tests;
