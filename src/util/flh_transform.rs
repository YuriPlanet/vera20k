//! FLH (Forward/Lateral/Height): the native fire-coordinate offset.
//!
//! `native_flh_world_delta` is `TechnoClass::GetFLH`'s integer world delta;
//! `native_building_barrel_world_delta` is the `BarrelAnimIsVoxel=` building's
//! replacement projection through `GetTurretDrawPosition`. `sim::combat::
//! fire_coord` is their caller. The presentation's `f32` screen-space variants
//! went with the app-side muzzle flashes.
//!
//! ## Dependency rules
//! - Part of util/; no simulation or presentation dependency.

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

/// GetFLH6F3B59 distinguishes a native locomotor from the identity arm.
/// An absent supplied Draw_Matrix still retains a locomotor's flat fallback.
#[derive(Clone, Copy)]
pub enum FlhBasis {
    NoLocomotor,
    Locomotor(Option<glam::Mat4>),
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
/// A supplied locomotor basis is the same GPU-independent Draw_Matrix
/// consumed by rendering. A locomotor without a supplied matrix retains its
/// existing flat heading. The native no-locomotor arm starts at identity and
/// rotates once by the absolute fire facing. All callers use the original
/// affine rotation and scalar translation stores, including GetFLH's distinct
/// Translate5AE890 kernel.
///
/// Two things here are easy to get wrong and are pinned by tests:
///
/// 1. **A locomotor's `B` and `Rz(theta)` remain separate rotations.**
///    In the locomotor branch, `theta` is the
///    DIFFERENCE `d32(aim) - d32(body)` and `B` supplies its own heading.
///    Fly4CF651 reads SecondaryFacing for B; ground55A730 reads PrimaryFacing.
///    On ground that algebraically composes to `Rz(aim)`, but retail's sine table is
///    asymmetric, so composing two lookups leaves a residual of exactly two
///    table steps (0.088 degrees) against the single-rotation form — a whole
///    lepton on a long barrel. `LocomotionClass::Draw_Matrix @ 0x0055A730` uses the
///    same quantisation and the same `-(pi/16)` double, and on flat ground `B`
///    reduces to that pure Z rotation with no translation and no scale.
/// 2. **`TurretOffset` is added before the aim rotation**, so it rides the
///    locomotor matrix frame when one exists; otherwise it stays on world X.
///    This matches the art-INI description of
///    an offset along the body centreline. `GetFLH` uses the raw value; note
///    that `render/vxl_raster.rs` divides the same field by 8 for its own
///    purposes, and that scaling must not leak in here.
///
/// The caller supplies Drive/Ship/Hover slopes and rocking, and Fly yaw/pitch/bank,
/// through the shared pre-camera matrix owners. Other locomotor tilt arms
/// retain their existing flat basis limitation.
pub fn native_flh_world_delta(
    forward: i32,
    lateral: i32,
    height: i32,
    turret_offset: i32,
    facings: FlhFacings,
    burst_index: u8,
    basis: FlhBasis,
) -> Option<(i32, i32, i32)> {
    use crate::util::direction_tables::step32_from_facing16;
    use crate::util::native_affine::{rotate_z, translate_xyz};
    use crate::util::native_trig::native_sin_cos_by_step;
    use crate::util::native_x87::X87Chop53;

    let aim_step = i32::from(step32_from_facing16(facings.aim));
    let body_step = i32::from(step32_from_facing16(facings.body));
    let matrix_step = i32::from(step32_from_facing16(facings.matrix));

    let (basis, relative_step) = match basis {
        //6F3C1A..6F3C52: no Foot locomotor means identity plus one absolute
        // fire-facing turn. A Building's authored body heading is not read.
        FlhBasis::NoLocomotor => (glam::Mat4::IDENTITY, aim_step - 8),
        FlhBasis::Locomotor(Some(basis)) => (basis, aim_step - body_step),
        FlhBasis::Locomotor(None) => {
            let (sin, cos) = native_sin_cos_by_step(matrix_step - 8)?;
            (
                rotate_z(glam::Mat4::IDENTITY, sin, cos),
                aim_step - body_step,
            )
        }
    };
    let matrix = translate_xyz(basis, [turret_offset as f32, 0., 0.]);
    let (sin, cos) = native_sin_cos_by_step(relative_step)?;
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

/// `BuildingClass::GetTurretDrawPosition @ 0x00453BF0`'s voxel-barrel world
/// delta for a `BarrelAnimIsVoxel=` type: the composed pivot matrix's
/// translation, truncated into the integer add the fire coordinate takes.
///
/// gamemd-derived. The matrix chain `0x00458810` post-multiplies, per
/// row-stored floats, from the identity (`0x5AE860`):
///
/// ```text
/// M = T(building_pivot) . Rz(theta_z) . T(rotate_pivot)
///     . RotY(-theta_y) . T(pitch_pivot) . Scale(s)
/// p = M . (barrel_end with the burst-signed Y)
/// out = ( ftol(p.x), ftol(-p.y), ftol(p.z) )   -- Y negated at 0x00453CC9
/// ```
///
/// Both angles come from a `FacingClass::Current @ 0x4C93D0` read quantized
/// to `((d >> 10) + 1) >> 1 & 0x1F - 8` quarter-facing steps, then scaled by
/// the double `-(pi/16)` at `0x7E4408` (`0x0045887B..0x0045889F` and
/// `0x004588EF..0x00458922`): `theta_z` from the body heading (`+0x388`),
/// `theta_y` from the barrel elevation (`+0x370`), whose input the chain
/// additionally negates (`fchs` at `0x00458920`). The negated angle is a
/// step of the same table, answered at the mirrored step, not by negating a
/// sine: the retail table is asymmetric. Each pivot rides its own
/// single-axis translate ([`crate::util::native_affine::translate_axis`]).
///
/// `Scale` (`0x00458975..0x004589A2`; X/Y/Z at `0x5AEAD0/0x5AEAF0/0x5AEB20`)
/// multiplies the three basis columns by the type's `+0x1728` double, read
/// at `0x0045897B`. Nothing initializes that field: no instruction stores
/// it (the `VoxelBarrelFile=` loader `0x0045FCAF` loads only the model, and
/// `BuildingTypeClass::ComputeCRC @ 0x00464B30` only hashes it), the
/// constructor `0x0045DD90` skips `+0x1718..+0x175F`, and
/// `FindOrAllocate @ 0x004653C0` allocates the type without zeroing
/// (`HeapAlloc` at `0x007C94B3`). Native scales by whatever the heap block
/// held; this port uses 0.0, a never-used page's content (a residual of
/// `fire_coordinate`'s barrel arm). The zero scale clears the basis, so the
/// point transform `0x5AFB80` (`0x00453CA3`) answers the translation column
/// exactly, whatever the burst-signed barrel end: this port returns that
/// translation (`building_voxel_flh.json`'s barrel rows record it natively).
pub fn native_building_barrel_world_delta(
    building_pivot: [i32; 3],
    rotate_pivot: [i32; 3],
    pitch_pivot: [i32; 3],
    facing16: u16,
    elevation16: u16,
) -> Option<(i32, i32, i32)> {
    use crate::util::direction_tables::quantize::step32_from_facing16;
    use crate::util::native_affine::{rotate_y, rotate_z, translate_axis};
    use crate::util::native_trig::native_sin_cos_by_step;
    use crate::util::native_x87::X87Chop53;

    // 0x458827..0x45886B: T(+0x1748/4C/50), one axis per kernel call.
    let mut matrix = glam::Mat4::IDENTITY;
    for (axis, pivot) in building_pivot.into_iter().enumerate() {
        matrix = translate_axis(matrix, axis, pivot as f32);
    }
    // 0x45886F..0x45889F: Rz of the body-facing step.
    let theta_z = i32::from(step32_from_facing16(facing16)) - 8;
    let (sin_z, cos_z) = native_sin_cos_by_step(theta_z)?;
    let matrix = rotate_z(matrix, sin_z, cos_z);
    // 0x4588A4..0x4588EA: T(+0x173C/40/44).
    let mut matrix = matrix;
    for (axis, pivot) in rotate_pivot.into_iter().enumerate() {
        matrix = translate_axis(matrix, axis, pivot as f32);
    }
    // 0x4588EF..0x458925: RotY of the negated elevation step.
    let theta_y = i32::from(step32_from_facing16(elevation16)) - 8;
    let (sin_y, cos_y) = native_sin_cos_by_step(-theta_y)?;
    let matrix = rotate_y(matrix, sin_y, cos_y);
    // 0x45892A..0x458970: T(+0x1730/34/38).
    let mut matrix = matrix;
    for (axis, pivot) in pitch_pivot.into_iter().enumerate() {
        matrix = translate_axis(matrix, axis, pivot as f32);
    }
    // 0x458975..0x4589A2 then 0x453CA3: the zero scale clears the basis, so
    // the transformed barrel end is the translation column.
    let origin = matrix.col(3);
    // Y is negated before the three `Math::ftol @ 0x007C5F00` truncations
    // (`0x00453CB1..0x00453CDD`).
    let ftol = |value: f32| X87Chop53::ftol_f64_low_masked(f64::from(value));
    Some((ftol(origin.x), ftol(-origin.y), ftol(origin.z)))
}
