//! FLH (Forward/Lateral/Height): the native fire-coordinate offset.
//!
//! `native_flh_world_delta` is `TechnoClass::GetFLH`'s integer world delta, the
//! one FLH transform the engine has; `sim::combat::fire_coord` is its caller.
//! The presentation's `f32` screen-space variants went with the app-side
//! muzzle flashes.
//!
//! ## Dependency rules
//! - Part of util/; no simulation or presentation dependency.

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

/// GetFLH's relative aim/body rotation and the locomotor's independent basis.
/// Fly DrawMatrix4CF610 uses SecondaryFacing; ordinary ground locomotors use
/// PrimaryFacing. Keeping the basis explicit prevents firing and steering
/// from silently assuming that those retained headings always agree.
#[derive(Clone, Copy)]
pub struct FlhFacings {
    pub aim: u16,
    pub body: u16,
    pub matrix: u16,
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
/// The optional locomotor basis is the same GPU-independent Draw_Matrix
/// consumed by rendering. With no supplied basis, the existing flat heading
/// remains the input. All callers use the original affine rotation and scalar
/// translation stores, including GetFLH's distinct Translate5AE890 kernel.
///
/// Two things here are easy to get wrong and are pinned by tests:
///
/// 1. **`B` and `Rz(theta)` are two separate rotations and do NOT collapse.**
///    A turreted object takes the locomotor branch, where `theta` is the
///    DIFFERENCE `d32(aim) - d32(body)` and `B` supplies its own heading.
///    Fly4CF651 reads SecondaryFacing for B; ground55A730 reads PrimaryFacing.
///    On ground that algebraically composes to `Rz(aim)`, but retail's sine table is
///    asymmetric, so composing two lookups leaves a residual of exactly two
///    table steps (0.088 degrees) against the single-rotation form — a whole
///    lepton on a long barrel. `LocomotionClass::Draw_Matrix @ 0x0055A730` uses the
///    same quantisation and the same `-(pi/16)` double, and on flat ground `B`
///    reduces to that pure Z rotation with no translation and no scale.
/// 2. **`TurretOffset` is added between the two rotations**, so it rides the
///    locomotor matrix frame — matching the art-INI description of
///    an offset along the body centreline. `GetFLH` uses the raw value; note
///    that `render/vxl_raster.rs` divides the same field by 8 for its own
///    purposes, and that scaling must not leak in here.
///
/// The caller supplies Drive/Ship slopes and rocking through their shared
/// matrix owner. Other locomotor tilt arms retain their existing flat basis
/// limitation; this does not certify arbitrary Fly/Jumpjet firing poses.
pub fn native_flh_world_delta(
    forward: i32,
    lateral: i32,
    height: i32,
    turret_offset: i32,
    facings: FlhFacings,
    burst_index: u8,
    locomotor_basis: Option<glam::Mat4>,
) -> Option<(i32, i32, i32)> {
    use crate::util::direction_tables::step32_from_facing16;
    use crate::util::native_affine::{rotate_z, translate_xyz};
    use crate::util::native_trig::native_sin_cos_by_step;
    use crate::util::native_x87::X87Chop53;

    let aim_step = i32::from(step32_from_facing16(facings.aim));
    let body_step = i32::from(step32_from_facing16(facings.body));
    let matrix_step = i32::from(step32_from_facing16(facings.matrix));

    let basis = if let Some(basis) = locomotor_basis {
        basis
    } else {
        let (sin, cos) = native_sin_cos_by_step(matrix_step - 8)?;
        rotate_z(glam::Mat4::IDENTITY, sin, cos)
    };
    let matrix = translate_xyz(basis, [turret_offset as f32, 0., 0.]);
    let (sin, cos) = native_sin_cos_by_step(aim_step - body_step)?;
    let matrix = rotate_z(matrix, sin, cos);
    let matrix = translate_xyz(
        matrix,
        [
            forward as f32,
            flh_lateral_for_burst(lateral, burst_index) as f32,
            height as f32,
        ],
    );
    let world = matrix.w_axis;

    // Y is negated before the three `Math::ftol @ 0x007C5F00` truncations.
    let ftol = |value: f32| X87Chop53::ftol_f64_low_masked(f64::from(value));
    Some((ftol(world.x), ftol(-world.y), ftol(world.z)))
}
