//! GPU-independent original locomotor body pose and shared tilt support.
//! Rendering and TechnoGetFLH6F3B9B consume the same raw native matrix.
//! Its translation is whole native voxels; no render TurretOffset /8 scale
//! or camera projection belongs in this owner.
use crate::util::native_affine as affine;
use glam::{Mat4, Vec3, Vec4};

/// Locomotor matrix blend between two gamemd slope matrix table entries.
///
/// gamemd's `VXL_InterpolatedFacing` path interpolates slope orientation through
/// quaternion SLERP and converts the result back to a matrix before composition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VxlSlopeBlend {
    pub from_slope: u8,
    pub to_slope: u8,
    pub phase_num: i32,
    pub phase_den: u8,
}

/// `LocomotionClass::Draw_Matrix @ 0x0055A730`: signed step -8..23, native
/// double angle and float store before table trig. Share the established
/// table with FLH rather than recomputing an approximately equal sine.
/// `RotateZ @ 0x005AF1A0` turns each row of the identity into `x*cos +
/// y*sin` and `y*cos - x*sin`; on ones and zeros that is exact in binary32,
/// so the zeros keep the signs those products give (row 2 is -0 in one
/// column for steps 17..31; `tools/rocket_oracle/draw_matrix.json`).
pub fn voxel_body_facing(step: u8) -> Mat4 {
    let (sin, cos) = crate::util::native_trig::native_sin_cos_by_step(i32::from(step) - 8)
        .expect("five-bit voxel facing fits native trig table");
    affine::rotate_z(Mat4::IDENTITY, sin, cos)
}

/// Fly Draw_Matrix4CF610, before camera projection: Secondary yaw, then
/// normal-flight type pitch about Y and signed bank about X (4CF71E..4CF804).
/// GetFLH6F3B9B passes a null cache key but still consumes these rotations.
pub fn normal_fly_locomotor_matrix(step: u8, pitch: f32, roll: f32) -> Mat4 {
    let mut frame = voxel_body_facing(step);
    if pitch != 0.0 {
        let (sin, cos) = voxel_table_rotation(pitch);
        frame = affine::rotate_y(frame, sin, cos);
    }
    if roll != 0.0 {
        let (sin, cos) = voxel_table_rotation(roll);
        frame = affine::rotate_x(frame, sin, cos);
    }
    frame
}

/// Fly4CF6A0..4CF71E crashing matrix: yaw, sideways X, then forwards Y.
/// The caller samples height and adds the gated type pitch to forwards.
pub fn crash_fly_locomotor_matrix(step: u8, [sideways, forwards]: [f32; 2]) -> Mat4 {
    let (sin, cos) = voxel_table_rotation(sideways);
    let frame = affine::rotate_x(voxel_body_facing(step), sin, cos);
    let (sin, cos) = voxel_table_rotation(forwards);
    affine::rotate_y(frame, sin, cos)
}

/// Drive4B0033..4B0117 / Jumpjet54DD48..54DE53 / Ship69F790..69F827 support offsets,
/// `[x, y, z]` in whole voxels
/// (`0x0054DD48..0x0054DE53`), as the original evaluates them: the table sine
/// and cosine stored as binary32, `Math::ftol` truncation, and the signs of
/// the pitch and roll. `half_sizes` are `TechnoTypeClass+0x360` and `+0x368`
/// ([`crate::rules::voxel_asset_catalog::voxel_tilt_half_sizes`]); `angles` are the roll and pitch
/// (`TechnoClass+0x328`, `+0x32C`).
pub(crate) fn voxel_tilt_offsets(
    trig: &crate::util::retail_trig::TrigTable,
    [roll, pitch]: [f32; 2],
    [half_y, half_x]: [f32; 2],
) -> [i32; 3] {
    use crate::util::native_x87::{NativeF32Bits, X87Chop53 as Fpu, X87Ordering, X87Value};
    let single = |value: f32| {
        Fpu::load_f32(NativeF32Bits::from_bits(value.to_bits())).unwrap_or(Fpu::load_i32(0))
    };
    // `FSTP float`: the table entries are binary32 already, so the store is
    // exact; kept to mirror the four stack slots.
    let stored = |value: X87Value| {
        Fpu::store_f32(value).map_or(Fpu::load_i32(0), |bits| {
            Fpu::load_f32(bits).unwrap_or(Fpu::load_i32(0))
        })
    };
    let abs = |value: X87Value| {
        if Fpu::compare(value, Fpu::load_i32(0)) == X87Ordering::Less {
            Fpu::neg(value)
        } else {
            value
        }
    };
    let (a, b) = (single(half_y), single(half_x));
    let cos_pitch = stored(trig.cos_from_table(single(pitch)));
    let sin_pitch = stored(trig.sin_from_table(single(pitch)));
    let cos_roll = stored(trig.cos_from_table(single(roll)));
    let sin_roll = trig.sin_from_table(single(roll));
    let z = Fpu::ftol_i32_low_masked(Fpu::add(
        Fpu::mul(abs(sin_roll), a),
        Fpu::mul(abs(sin_pitch), b),
    ));
    let shifted = |cosine: X87Value, half: X87Value| {
        let truncated = Fpu::ftol_i32_low_masked(Fpu::mul(cosine, half));
        Fpu::ftol_i32_low_masked(Fpu::sub(half, Fpu::load_i32(truncated)))
    };
    let mut x = shifted(cos_pitch, b);
    let mut y = shifted(cos_roll, a);
    // `FCOMP float 0.0`: a negative pitch and a positive roll flip their shift.
    if pitch < 0.0 {
        x = x.wrapping_neg();
    }
    if roll > 0.0 {
        y = y.wrapping_neg();
    }
    [x, y, z]
}

/// Jumpjet54DCE8..54DD13 and Ship69F6CD..69F6F8 tilt when either
/// angle's magnitude, widened to double, is at least 0.005 (`0x007E44E8`); a
/// NaN never tilts.
pub fn voxel_tilt_applies(angles: [f32; 2]) -> bool {
    angles
        .into_iter()
        .any(|angle| f64::from(angle.abs()) >= 0.005)
}

pub fn voxel_tilt_frame(angles: [f32; 2], half_sizes: [f32; 2]) -> (Mat4, Mat4) {
    let [x, y, z] = voxel_tilt_offsets(
        crate::util::retail_trig::TrigTable::embedded(),
        angles,
        half_sizes,
    );
    let lift = Mat4::from_translation(Vec3::new(0., 0., z as f32));
    let frame = Mat4::from_translation(Vec3::new(x as f32, y as f32, 0.));
    let (sin, cos) = voxel_table_rotation(angles[0]);
    let frame = affine::rotate_x(frame, sin, cos);
    let (sin, cos) = voxel_table_rotation(angles[1]);
    (lift, affine::rotate_y(frame, sin, cos))
}

pub fn voxel_table_rotation(angle: f32) -> (f32, f32) {
    let trig = crate::util::retail_trig::TrigTable::embedded();
    let angle = affine::load(angle).expect("finite voxel angle");
    (
        affine::store(trig.sin_from_table(angle)).expect("finite table entry"),
        affine::store(trig.cos_from_table(angle)).expect("finite table entry"),
    )
}

/// Drive4AFF60 / Ship69F670 dynamic matrices. Three original5AF980 calls retain
/// **((lift * slope) * facing) * shifted_rotation**, with a float store at
/// every product. The cached ordinary arm remains the existing slope/facing
/// owner. Offsets are shared with Jumpjet54DCC0, not another support model.
pub fn ground_locomotor_matrix(
    slope_type: u8,
    slope_blend: Option<VxlSlopeBlend>,
    facing_step: u8,
    angles: [f32; 2],
    half_sizes: [f32; 2],
) -> Mat4 {
    let product = |left, right| affine::matrix_product(left, right).expect("finite ground pose");
    let slope = slope_blend.map_or_else(
        || compute_slope_rotation(if slope_type < 17 { slope_type } else { 0 }),
        compute_slope_blend_rotation,
    );
    let facing = voxel_body_facing(facing_step);
    if slope_blend.is_none() && !voxel_tilt_applies(angles) {
        return product(slope, facing);
    }
    let (lift, frame) = voxel_tilt_frame(angles, half_sizes);
    product(product(product(lift, slope), facing), frame)
}

/// Original `7559B0` table selection, shared with particle/projectile collision.
/// Native initialization/executable controls: tools/native_slope.py.
pub fn compute_slope_rotation(slope_type: u8) -> Mat4 {
    slope_matrix_to_mat4(
        crate::map::voxel_slope::slope_matrix(slope_type)
            .unwrap_or_else(|| crate::map::voxel_slope::slope_matrix(0).unwrap()),
    )
}

fn slope_matrix_to_mat4(rows: [crate::util::native_x87::NativeF32Bits; 12]) -> Mat4 {
    let r = rows.map(|v| f32::from_bits(v.bits()));
    Mat4::from_cols(
        Vec4::new(r[0], r[4], r[8], 0.0),
        Vec4::new(r[1], r[5], r[9], 0.0),
        Vec4::new(r[2], r[6], r[10], 0.0),
        Vec4::new(r[3], r[7], r[11], 1.0),
    )
}

pub fn compute_slope_blend_rotation(blend: VxlSlopeBlend) -> Mat4 {
    use crate::util::native_x87::X87Chop53 as Fpu;
    // The locomotor passes a double ratio;755A40 stores it to float before
    //646590. Preserve the signed extrapolation and the original stores.
    let phase = Fpu::store_f64(
        Fpu::div(
            Fpu::load_i32(blend.phase_num),
            Fpu::load_i32(i32::from(blend.phase_den.max(1))),
        )
        .expect("nonzero slope duration"),
    )
    .expect("finite slope fraction");
    slope_matrix_to_mat4(
        crate::map::voxel_slope::blend_matrix(
            if blend.from_slope <= 20 {
                blend.from_slope
            } else {
                0
            },
            if blend.to_slope <= 20 {
                blend.to_slope
            } else {
                0
            },
            phase,
        )
        .expect("finite native slope interpolation"),
    )
}
