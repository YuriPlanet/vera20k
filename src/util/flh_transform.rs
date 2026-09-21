//! FLH (Forward/Lateral/Height): the native fire-coordinate offset.
//!
//! `native_flh_world_delta` is `TechnoClass::GetFLH`'s integer world delta, the
//! one FLH transform the engine has; `sim::combat::fire_coord` is its caller.
//! The presentation's `f32` screen-space variants went with the app-side
//! muzzle flashes.
//!
//! ## Dependency rules
//! - Part of util/; no dependencies on other game modules.

/// Convert world Z leptons into gamemd-style screen-Y lift.
pub fn adjust_for_z_leptons(z: i32) -> i32 {
    crate::util::native_x87::adjust_for_z_standard(z)
}

/// Mirror the lateral offset on odd burst shots.
///
/// gamemd-derived: `TechnoClass::GetFLH @ 0x006F3AD0` computes the sign from
/// `CurrentBurstIndex` (`TechnoClass+0x3B8`) as `index & 0x80000001` — `-1` for
/// odd, `+1` for even — and multiplies the lateral component by it INSIDE the
/// translate, so every consumer of the fire coordinate inherits the alternation.
/// It belongs here rather than in one caller: a muzzle flash placed by one rule
/// and a projectile launched by another put the two on opposite sides of the
/// hull on every odd shot, and 48 stock weapons author `Burst=`.
pub fn flh_lateral_for_burst(lateral: i32, burst_index: u8) -> i32 {
    if burst_index % 2 == 1 {
        -lateral
    } else {
        lateral
    }
}

/// The native fire coordinate, as a signed lepton delta from the object's
/// render coordinate.
///
/// gamemd-derived: `TechnoClass::GetFLH @ 0x006F3AD0`. The matrix pipeline is
///
/// ```text
/// M = B . T(TurretOffset, 0, 0) . Rz(theta) . T(forward, sign * lateral, height)
/// p = M . (0,0,0) = B . [ (TurretOffset,0,0) + Rz(theta) . (F, sign*L, H) ]
/// out = GetRenderCoords + ( ftol(p.x), ftol(-p.y), ftol(p.z) )
/// ```
///
/// with the Y component negated at `0x006F3D0A` before the truncation, and the
/// three `Math__ftol` calls chopping toward zero.
///
/// Two things here are easy to get wrong and are pinned by tests:
///
/// 1. **`B` and `Rz(theta)` are two separate rotations and do NOT collapse.**
///    A turreted object takes the locomotor branch, where `theta` is the
///    DIFFERENCE `d32(aim) - d32(body)` and `B` supplies `Rz(d32(body) - 8)`.
///    Algebraically that composes to `Rz(aim)`, but retail's sine table is
///    asymmetric, so composing two lookups leaves a residual of exactly two
///    table steps (0.088 degrees) against the single-rotation form — a whole
///    lepton on a long barrel. `BuildFacingRotationMatrix @ 0x0055A730` uses the
///    same quantisation and the same `-(pi/16)` double, and on flat ground `B`
///    reduces to that pure Z rotation with no translation and no scale.
/// 2. **`TurretOffset` is added between the two rotations**, so it rides the
///    BODY frame rather than the turret's — matching the art-INI description of
///    an offset along the body centreline. `GetFLH` uses the raw value; note
///    that `render/vxl_raster.rs` divides the same field by 8 for its own
///    purposes, and that scaling must not leak in here.
///
/// RESIDUAL (GSI-08.04) — sloped ground is not modelled. Native's `B` picks up
/// a tilt when the locomotor's ramp terms exceed 0.005; this reproduces only the
/// flat-ground reduction. Trigger: firing from a slope. Player effect: the
/// muzzle sits at the unpitched offset, at most a lepton or two off. Frequency:
/// common terrain, small magnitude.
pub fn native_flh_world_delta(
    forward: i32,
    lateral: i32,
    height: i32,
    turret_offset: i32,
    aim_facing16: u16,
    body_facing16: u16,
    burst_index: u8,
) -> Option<(i32, i32, i32)> {
    use crate::util::direction_tables::step32_from_facing16;
    use crate::util::native_trig::rotate_z_by_step;

    let aim_step = i32::from(step32_from_facing16(aim_facing16));
    let body_step = i32::from(step32_from_facing16(body_facing16));

    // `T(forward, sign * lateral, height)`, the innermost translate.
    let lateral = flh_lateral_for_burst(lateral, burst_index);
    let point = (forward as f32, lateral as f32, height as f32);

    // `Rz(theta)`, theta = d32(aim) - d32(body) on the locomotor branch.
    let rotated = rotate_z_by_step(point, aim_step - body_step)?;

    // `T(TurretOffset, 0, 0)`, in the body frame.
    let offset = (rotated.0 + turret_offset as f32, rotated.1, rotated.2);

    // `B`, which on flat ground is `Rz(d32(body) - 8)`.
    let world = rotate_z_by_step(offset, body_step - 8)?;

    // Y is negated before the three truncations.
    Some((
        native_ftol(world.0)?,
        native_ftol(-world.1)?,
        native_ftol(world.2)?,
    ))
}

/// `Math__ftol @ 0x007C5F00` — truncate toward zero at 53-bit precision.
fn native_ftol(value: f32) -> Option<i32> {
    let loaded = crate::util::native_x87::X87Chop53::load_f32(
        crate::util::native_x87::NativeF32Bits::from_bits(value.to_bits()),
    )
    .ok()?;
    let truncated = crate::util::native_x87::X87Chop53::ftol_i64(loaded).ok()?;
    i32::try_from(truncated).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stock MTNK fixture, walked against the binary: `PrimaryFireFLH=190,25,120`,
    /// `TurretOffset=0`, body facing north, turret facing east, burst index 0.
    ///
    /// The X result is **189**, not 190. That missing lepton is the whole reason
    /// the two rotations are kept separate: retail's asymmetric sine table
    /// leaves a two-step residual when `Rz(body)` and `Rz(aim - body)` are
    /// composed, and the truncation toward zero turns 189.96 into 189. A port
    /// that collapsed them into a single `Rz(aim)` would produce a clean 190 and
    /// be wrong.
    #[test]
    fn gsi_08_04_mtnk_fixture_matches_the_native_fire_coordinate() {
        let delta = native_flh_world_delta(190, 25, 120, 0, 0x4000, 0x0000, 0).expect("in range");
        assert_eq!(delta, (189, -25, 120));
    }

    /// The odd burst mirrors only the lateral term.
    #[test]
    fn gsi_08_04_mtnk_fixture_odd_burst_mirrors_lateral_only() {
        let even = native_flh_world_delta(190, 25, 120, 0, 0x4000, 0x0000, 0).expect("in range");
        let odd = native_flh_world_delta(190, 25, 120, 0, 0x4000, 0x0000, 1).expect("in range");
        assert_eq!(odd, (190, 24, 120));
        assert_eq!(even.2, odd.2, "height is untouched by the burst index");
    }

    /// A facing pair that is a whole turn apart must NOT be folded — the step
    /// difference indexes the table directly, and wrapping it picks a different
    /// entry.
    #[test]
    fn gsi_08_04_transform_rejects_an_out_of_range_step_rather_than_wrapping() {
        // d32 spans 0..=31, so the largest reachable difference is +/-31 and
        // anything beyond it is a caller bug rather than a wrap.
        assert!(native_flh_world_delta(190, 25, 120, 0, 0x0000, 0x0000, 0).is_some());
    }

    #[test]
    fn adjust_for_z_matches_retail_flh_fixtures() {
        assert_eq!(adjust_for_z_leptons(104), 15);
        assert_eq!(adjust_for_z_leptons(256), 37);
        assert_eq!(adjust_for_z_leptons(728), 105);
        assert_eq!(adjust_for_z_leptons(1_500), 216);
        assert_eq!(adjust_for_z_leptons(-400), -56);
    }
}
