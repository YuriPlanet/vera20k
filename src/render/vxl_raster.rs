//! VXL palette-index sprite rendering and separate voxel-shadow geometry.
//!
//! Parsed ordinary VXLs use vxl_native.rs: original x87 box/crop preparation,
//! encoded span traversal and literal 256x256 visibility writes. CPU and GPU
//! share that owner. Native executable fixtures live in tools/voxel_oracle.
//! Magnified previews and constructed diagnostics retain the legacy general
//! projection below; its 16.16 splats are not the native 8.8 packed raster.
//!
//! ## Dependency rules
//! - Part of render/ — depends on assets/ (VxlFile, HvaFile, VplFile).
//! - Uses glam for vector/matrix math.

use glam::{Mat3, Mat4, Vec3, Vec4};

use crate::assets::hva_file::HvaFile;
use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::{VxlFile, VxlLimb};
#[cfg(test)]
use crate::map::voxel_pose::voxel_tilt_applies;
use crate::map::voxel_pose::{
    VxlSlopeBlend, compute_slope_blend_rotation, compute_slope_rotation,
    crash_fly_locomotor_matrix as voxel_crash_locomotor_matrix, ground_locomotor_matrix,
    normal_fly_locomotor_matrix, voxel_body_facing, voxel_tilt_frame,
};
use crate::render::vxl_normals;
#[cfg(test)]
use crate::rules::voxel_asset_catalog::voxel_tilt_half_sizes;
use crate::util::native_affine as affine;

#[path = "vxl_native.rs"]
mod native;

#[path = "vxl_shadow.rs"]
pub(crate) mod shadow;

pub(crate) use native::PreparedDraw;

/// Shared ordinary VXL preparation for CPU and GPU visibility writes. Preview
/// magnification and constructed models without encoded spans keep their
/// explicitly separate legacy renderer.
pub(crate) fn prepare_native_draw(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
    vpl: Option<&VplFile>,
) -> Option<PreparedDraw> {
    native::prepare_draw(vxl, hva, params, vpl)
}

/// Number of distinct orientations a voxel body/turret/barrel can render at.
///
/// The original quantizes facing to 5 bits before building the rotation matrix and
/// reuses those same 5 bits as its draw-cache key, so there is no finer sub-facing
/// anywhere in the voxel pipeline: a rendered voxel is always exactly 1 of 32.
pub const VOXEL_FACING_STEPS: u32 = 32;

/// Quantize an 8-bit facing to the voxel renderer's 5-bit facing step (0–31).
///
/// The original computes this from the 16-bit facing as
/// `((facing16 >> 10) + 1 >> 1) & 0x1F`. An 8-bit facing is the high byte of that
/// 16-bit value, so the shift reduces to `facing8 >> 2` and the `+1 >> 1` is a
/// round-half-up to the nearest of 32 steps (the boundary sits at facing 4, i.e.
/// exactly half of one 11.25° step).
pub fn voxel_facing_step(facing: u8) -> u8 {
    ((((u32::from(facing) >> 2) + 1) >> 1) & (VOXEL_FACING_STEPS - 1)) as u8
}

/// Quantize a 16-bit facing to the voxel renderer's 5-bit facing step (0–31).
///
/// This is the form the original uses directly; prefer it wherever the full 16-bit
/// facing is still in hand, since it rounds off the true value rather than off an
/// already-truncated byte.
pub fn voxel_facing_step_u16(facing16: u16) -> u8 {
    crate::util::direction_tables::step32_from_facing16(facing16)
}

/// The camera/view basis every voxel draw shares.
///
/// Built the way the original does: identity, rotate-X(-pitch), rotate-Z(-yaw).
/// Both of its rotate helpers post-multiply a right-handed counter-clockwise
/// rotation — the same convention `glam::Mat4::from_rotation_*` uses — so the two
/// expressions transcribe one-for-one. This is a pure rotation: there is no scale
/// anywhere in the camera.
fn voxel_camera_view() -> Mat4 {
    // Original startup setters 0x00754980/0x007549A0 and the camera block
    // 0x007558CE..0x00755904 produce these bits. Its lookup-table trig is
    // asymmetric; evaluating sin/cos at exact -60/-45 degrees is different.
    // Reproduce with tools/voxel_oracle/lighting.py; shared by geometry/light.
    Mat4::from_cols_array(
        &[
            0x3f354bfb, 0xbeb4d24b, 0x3f1c80e9, 0x00000000, 0x3f34bdcf, 0x3eb56087, 0xbf1cfc05,
            0x00000000, 0x00000000, 0x3f5dab76, 0x3f000e82, 0x00000000, 0x00000000, 0x00000000,
            0x00000000, 0x3f800000,
        ]
        .map(f32::from_bits),
    )
}

fn voxel_draw_rotation(slope: Mat4, facing: Mat4) -> Mat4 {
    // DriveLocomotion 0x004B03DA first multiplies slope * facing;
    // UnitClass 0x0073B71C then multiplies camera * that result.
    affine::matrix_product(
        voxel_camera_view(),
        affine::matrix_product(slope, facing).expect("finite voxel rotation"),
    )
    .expect("finite voxel rotation")
}

/// A barrel part's pitch. `UnitClass::DrawVoxelBody` converts the barrel
/// elevation (`TechnoClass+0x370`) to a signed step `d32 - 8` and rotates
/// the part matrix by `-(step * -π/16)` before the camera
/// (`0x0073BB7E..0x0073BBAC`); negating the step instead forms the same
/// float angle, the one the retail table answers for `-step`.
fn voxel_barrel_pitch(matrix: Mat4, pitch: i8) -> Mat4 {
    let (sin, cos) = crate::util::native_trig::native_sin_cos_by_step(-i32::from(pitch))
        .expect("a five-bit pitch step fits the native trig table");
    affine::rotate_y(matrix, sin, cos)
}

/// The draw rotation of a pitched barrel: slope * facing, then the pitch,
/// then the camera.
fn voxel_pitched_draw_rotation(
    slope_type: u8,
    blend: Option<VxlSlopeBlend>,
    step: u8,
    pitch: i8,
) -> Mat4 {
    let slope = blend.map_or_else(
        || compute_slope_rotation(if slope_type < 17 { slope_type } else { 0 }),
        compute_slope_blend_rotation,
    );
    affine::matrix_product(
        voxel_camera_view(),
        voxel_barrel_pitch(
            affine::matrix_product(slope, voxel_body_facing(step)).expect("finite voxel rotation"),
            pitch,
        ),
    )
    .expect("finite voxel rotation")
}

thread_local! {
    // VERA-internal performance cache, not native state. Exact blend fields
    // own its key; each miss builds all 32 facings from the existing matrix.
    // At most 2 MiB of matrices per worker; clearing only causes recomputation.
    static BLENDED_ROTATIONS: std::cell::RefCell<std::collections::HashMap<VxlSlopeBlend, [Mat4; 32]>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

/// `FlyLocomotionClass` Draw_Matrix's crashing arm (`0x004CF610`, reached at
/// `0x004CF6A3` while the owner's height is positive and its crash latch is
/// set): the facing rotation, then `Matrix_rotate_x_axis @ 0x005AEF60` by the
/// sideways angle and `Matrix_rotate_y_axis @ 0x005AF080` by the forwards
/// angle, each post-multiplied like glam's rotations; the owner's draw then
/// takes camera * that. The crashing arm keys its draw -1 (no cache): the
/// pose is new every frame.
///
/// The PitchSpeed term (`+ type+0x3B0` when the current speed exceeds
/// `type+0x3A8`) is omitted: the Fly speed of a crash never changes and no
/// stock aircraft both exceeds its `PitchSpeed=` and authors a `PitchAngle=`.
/// Native reads table sine/cosine; the same in-place rotation owner serves
/// Fly, Jumpjet, Drive and Ship so callers cannot diverge.
fn voxel_crash_rotation(step: u8, tilt: [f32; 2]) -> Mat4 {
    affine::matrix_product(
        voxel_camera_view(),
        voxel_crash_locomotor_matrix(step, tilt),
    )
    .expect("finite voxel rotation")
}

/// Normal-flight `FlyLocomotionClass::Draw_Matrix @ 0x004CF610` uses
/// SecondaryFacing yaw, then optional PitchAngle `RotateY @ 0x005AF080`, then
/// the selected signed RollAngle `RotateX @ 0x005AEF60`. Keep this separate
/// from the crash arm's X-then-Y matrix above; both share the same facing,
/// lookup-table rotations, affine owner and camera.
fn voxel_normal_fly_rotation(step: u8, pitch: f32, roll: f32) -> Mat4 {
    affine::matrix_product(
        voxel_camera_view(),
        normal_fly_locomotor_matrix(step, pitch, roll),
    )
    .expect("finite normal Fly matrix")
}

/// `RocketLocomotionClass::Draw_Matrix @ 0x00663470`: the facing matrix of
/// `LocomotionClass::Draw_Matrix @ 0x0055A730`, then, while CurrentPitch
/// (`+0x54`) is not zero, `Matrix_rotate_y_axis @ 0x005AF080` by its
/// negation, which raises the nose. RotateY takes the table sine and cosine
/// of that angle widened to double. Executed in
/// `tools/rocket_oracle/draw_matrix.py`.
fn voxel_rocket_locomotor_matrix(step: u8, pitch: f32) -> Mat4 {
    use crate::util::native_x87::{NativeF32Bits, X87Chop53 as Fpu, X87Value};
    let facing = voxel_body_facing(step);
    if pitch == 0.0 {
        return facing;
    }
    let trig = crate::util::retail_trig::TrigTable::embedded();
    let angle = Fpu::load_f32(NativeF32Bits::from_bits((-pitch).to_bits()))
        .expect("CurrentPitch is finite");
    // The table entries are binary32, so the stores are exact.
    let entry = |value: X87Value| {
        f32::from_bits(
            Fpu::store_f32(value)
                .expect("a table entry fits a float")
                .bits(),
        )
    };
    affine::rotate_y(
        facing,
        entry(trig.sin_from_table(angle)),
        entry(trig.cos_from_table(angle)),
    )
}

/// `JumpjetLocomotionClass` Draw_Matrix's `TiltCrashJumpjet=` arm
/// (`0x0054DD19..0x0054DF06`): the facing matrix of
/// `LocomotionClass::Draw_Matrix @ 0x0055A730` between a lift and a shifted,
/// rolled and pitched frame, `T_z * F * (T_xy * R_x(roll) * R_y(pitch))`, each
/// product `MatrixMultiply @ 0x005AF980` in the native order. The offsets are
/// the whole voxels [`crate::map::voxel_pose::voxel_tilt_offsets`] keeps the tilted body's rim on;
/// the in-place rotations share the native table/float-store owners.
fn voxel_jumpjet_tilt_locomotor_matrix(step: u8, angles: [f32; 2], half_sizes: [f32; 2]) -> Mat4 {
    let (lift, frame) = voxel_tilt_frame(angles, half_sizes);
    let product = |left, right| affine::matrix_product(left, right).expect("finite tilt matrix");
    product(product(lift, voxel_body_facing(step)), frame)
}

/// Drive4AFF60 / Ship69F670 (and Hover513F40 rocking) render-only per-part inputs. The locomotor owns slope/blend; the
/// object owns facing and angles; ART owns TurretOffset. The loaded model
/// supplies the main first-section half sizes, including for its gun parts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundPose {
    pub angles: [f32; 2],
    pub body_facing: u8,
    pub slope_blend: Option<VxlSlopeBlend>,
    pub turret_offset: i32,
}

/// Unit73BA4C..73BC11's uncached body/turret/barrel matrices, each before
/// HVA and after the shared camera. Secondary facing is relative to the
/// already tilted hull, and barrel pitch retains the translated pivot.
pub(crate) fn ground_unit_pose_matrices(
    slope_type: u8,
    pose: GroundPose,
    half_sizes: [f32; 2],
    turret_facing: u8,
    barrel_pitch: i8,
) -> [Mat4; 3] {
    let body = ground_locomotor_matrix(
        slope_type,
        pose.slope_blend,
        voxel_facing_step(pose.body_facing),
        pose.angles,
        half_sizes,
    );
    let translated = affine::translate(body, &[turret_offset_units(pose.turret_offset)]);
    let relative = i32::from(voxel_facing_step(turret_facing))
        - i32::from(voxel_facing_step(pose.body_facing));
    let (sin, cos) = crate::util::native_trig::native_sin_cos_by_step(relative)
        .expect("relative five-bit facing fits table");
    let turret = affine::rotate_z(translated, sin, cos);
    let pivot = turret.w_axis.truncate();
    let barrel = affine::translate(turret, &(-pivot).to_array());
    let barrel = voxel_barrel_pitch(barrel, barrel_pitch);
    let barrel = affine::translate(barrel, &pivot.to_array());
    [body, turret, barrel].map(|matrix| {
        affine::matrix_product(voxel_camera_view(), matrix).expect("finite ground draw matrix")
    })
}

/// The locomotor Draw_Matrix arm a tilted body draws through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BodyTilt {
    /// `FlyLocomotionClass` Draw_Matrix's crashing arm (`0x004CF610`): roll
    /// and pitch in radians.
    Fly([f32; 2]),
    /// Normal-flight `FlyLocomotionClass::Draw_Matrix @ 0x004CF610`: after
    /// the strict current-speed/PitchSpeed gate, RotateY(PitchAngle) followed
    /// by signed RotateX(RollAngle), both in radians.
    FlyNormal { pitch: f32, roll: f32 },
    /// `JumpjetLocomotionClass` Draw_Matrix's `TiltCrashJumpjet=` arm
    /// (`0x0054DCC0`): roll and pitch in radians, and the main voxel's half
    /// sizes ([`crate::rules::voxel_asset_catalog::voxel_tilt_half_sizes`]).
    Jumpjet {
        angles: [f32; 2],
        half_sizes: [f32; 2],
    },
    /// `RocketLocomotionClass::Draw_Matrix` (`0x00663470`): CurrentPitch in
    /// radians above the horizon.
    Rocket(f32),
    /// A complete caller-supplied draw matrix. Lighting and HVA admission
    /// remain independent: Bullet468090 uses a camera/velocity matrix and
    /// HVA frame0, while VoxelAnim749B70 uses its bounce matrix without HVA.
    Matrix(Mat4),
}

/// Native light setup selected by the caller, independently of its matrix.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum VxlLighting {
    #[default]
    BlinnPhong,
    Diffuse,
}

impl VxlLighting {
    fn pages(self, mode: u8, rotation: Mat3) -> [u8; 256] {
        match self {
            Self::BlinnPhong => vxl_normals::blinn_phong_pages(mode, rotation),
            Self::Diffuse => vxl_normals::diffuse_pages(mode, rotation),
        }
    }
}

/// Bullet468090's velocity-derived body matrix, before its HVA frame0.
/// Original468132..46820A uses atan2(-vy,vx), Sqrt_Approx(vx²+vy²), then
/// atan2(vz,horizontal), direction-word quantization, RotateZ/RotateY and the
/// standard voxel camera. Preserve represented velocity, including tiny XY
/// components: a nominally vertical Kirov launch can face the opposite way.
/// Numeric witnesses: tools/voxel_oracle/bullet_fixtures.json.
fn projectile_voxel_angles(
    velocity: crate::sim::projectile::ProjectileVelocity,
) -> Option<[u16; 2]> {
    use crate::util::native_x87::X87Chop53 as Fpu;
    let [x, y, z] = velocity.native().map(Fpu::load_f64);
    let (x, y, z) = (x.ok()?, y.ok()?, z.ok()?);
    let atan = |y, x| {
        Some(crate::util::direction_tables::native_atan2_f32(
            Fpu::store_f32(y).ok()?,
            Fpu::store_f32(x).ok()?,
        ))
    };
    let heading = crate::util::direction_tables::facing16_from_angle(atan(Fpu::neg(y), x)?);
    // 468198 stores the sum as binary64 before Sqrt_Approx's f32 store.
    let sum = Fpu::add(Fpu::mul(x, x), Fpu::mul(y, y));
    let sum = Fpu::load_f64(Fpu::store_f64(sum).ok()?).ok()?;
    let horizontal = Fpu::load_f32(crate::util::native_x87::sqrt_approx_f32(sum).ok()?).ok()?;
    let horizontal = Fpu::load_f64(Fpu::store_f64(horizontal).ok()?).ok()?;
    let pitch = crate::util::direction_tables::facing16_from_angle(atan(z, horizontal)?);
    Some([heading, pitch])
}

pub(crate) fn projectile_voxel_matrix(
    velocity: crate::sim::projectile::ProjectileVelocity,
) -> Option<Mat4> {
    let [heading, pitch] = projectile_voxel_angles(velocity)?;
    let heading = voxel_facing_step_u16(heading);
    let pitch = voxel_facing_step_u16(pitch) as i8 - 8;
    let orientation = voxel_barrel_pitch(voxel_body_facing(heading), pitch);
    // The same original5AF980 affine product used by ordinary voxel bodies.
    affine::matrix_product(voxel_camera_view(), orientation)
}

/// The draw matrix of one body draw: a tilt arm when one is present, else the
/// slope/facing products, pitched for a barrel. `UnitClass 0x0073B71C` takes
/// camera * the locomotor's matrix.
fn voxel_params_draw_rotation(params: &VxlRenderParams, step: u8) -> Mat4 {
    match params.body_tilt {
        Some(BodyTilt::Fly(tilt)) => voxel_crash_rotation(step, tilt),
        Some(BodyTilt::FlyNormal { pitch, roll }) => voxel_normal_fly_rotation(step, pitch, roll),
        Some(BodyTilt::Jumpjet { angles, half_sizes }) => affine::matrix_product(
            voxel_camera_view(),
            voxel_jumpjet_tilt_locomotor_matrix(step, angles, half_sizes),
        )
        .expect("finite tilt matrix"),
        Some(BodyTilt::Rocket(pitch)) => affine::matrix_product(
            voxel_camera_view(),
            voxel_rocket_locomotor_matrix(step, pitch),
        )
        .expect("finite voxel rotation"),
        // The section transform applies the identity viewer itself.
        Some(BodyTilt::Matrix(matrix)) => matrix,
        None if params.barrel_pitch != 0 => voxel_pitched_draw_rotation(
            params.slope_type,
            params.slope_blend,
            step,
            params.barrel_pitch,
        ),
        None => voxel_draw_rotation_for_state(params.slope_type, params.slope_blend, step),
    }
}

/// Ordinary stationary slope/facing combinations are finite. Build their
/// native products once: turret pivots consume this every displayed frame,
/// unlike the atlas which only prepares geometry on a cache miss. Blended
/// results are memoized by the complete input state, including signed phase
/// and denominator; invalid/fallback states still use the existing computation.
fn voxel_draw_rotation_for_state(slope_type: u8, blend: Option<VxlSlopeBlend>, step: u8) -> Mat4 {
    if let Some(blend) = blend {
        return BLENDED_ROTATIONS.with(|cache| {
            let mut cache = cache.borrow_mut();
            if let Some(rotations) = cache.get(&blend) {
                return rotations[usize::from(step)];
            }
            if cache.len() >= 1024 {
                cache.clear();
            }
            let slope = compute_slope_blend_rotation(blend);
            let rotations = std::array::from_fn(|facing| {
                voxel_draw_rotation(slope, voxel_body_facing(facing as u8))
            });
            let result = rotations[usize::from(step)];
            cache.insert(blend, rotations);
            result
        });
    }
    static ROTATIONS: std::sync::OnceLock<[[Mat4; 32]; 17]> = std::sync::OnceLock::new();
    let rotations = ROTATIONS.get_or_init(|| {
        std::array::from_fn(|slope| {
            std::array::from_fn(|facing| {
                voxel_draw_rotation(
                    compute_slope_rotation(slope as u8),
                    voxel_body_facing(facing as u8),
                )
            })
        })
    });
    rotations[if slope_type < 17 {
        usize::from(slope_type)
    } else {
        0
    }][usize::from(step)]
}

/// The double at `0x00B1D008`: model units per lepton. Static initializers
/// `0x00735180`, `0x007351B0` and `0x007351D0` form `1 / (sqrt(2 * 256^2) / 60)`,
/// a cell's 60-pixel width over its lepton diagonal, but take the root with
/// the table square root `0x004CAC40`, so the value is not the exact quotient.
/// Executed in `tools/voxel_oracle/barrel_pitch.py`.
const TURRET_OFFSET_UNITS_PER_LEPTON: u64 = 0x3FC5_3694_862E_9021;

/// `TurretOffset=` (`Type+0x720`) in model units, as `UnitClass::DrawVoxelBody`
/// forms it before translating the turret along the hull's X column: `FILD`
/// the leptons, `FMUL` the double at `0x00B1D008`, store a float
/// (`0x0073BA4C..0x0073BA60`).
fn turret_offset_units(turret_offset_leptons: i32) -> f32 {
    use crate::util::native_x87::{NativeF64Bits, X87Chop53 as Fpu};
    let product = Fpu::mul(
        Fpu::load_i32(turret_offset_leptons),
        Fpu::load_f64(NativeF64Bits::from_bits(TURRET_OFFSET_UNITS_PER_LEPTON))
            .expect("the offset scale is finite"),
    );
    f32::from_bits(
        Fpu::store_f32(product)
            .expect("a turret offset fits a float")
            .bits(),
    )
}

/// Screen displacement of a turret's pivot from the hull centre, in pixels.
///
/// Native translates the body matrix along its own X column before rotating
/// the turret, so the pivot inherits the hull's terrain tilt. The separately
/// cached turret sprite still uses its own absolute facing; equivalence to
/// native relative turret/body composition remains unchecked.
///
/// Returns pixels in the rasterizer's screen convention (+X right, +Y down), matching
/// how `render_vxl` projects a voxel: `x * scale` and `-y * scale`.
#[cfg(test)]
pub fn turret_pivot_screen_offset(
    turret_offset_leptons: i32,
    body_facing: u8,
    slope_type: u8,
    scale: f32,
) -> (f32, f32) {
    turret_pivot_screen_offset_for_slope_state(
        turret_offset_leptons,
        body_facing,
        slope_type,
        None,
        scale,
    )
}

/// Blend-aware form of [`turret_pivot_screen_offset`]. The optional blend is
/// consumed by the same quaternion-SLERP matrix helper as VXL limb rastering,
/// so a separated turret/barrel pivot cannot lead the hull to its destination
/// slope during an in-progress transition.
pub fn turret_pivot_screen_offset_for_slope_state(
    turret_offset_leptons: i32,
    body_facing: u8,
    slope_type: u8,
    slope_blend: Option<VxlSlopeBlend>,
    scale: f32,
) -> (f32, f32) {
    if turret_offset_leptons == 0 {
        return (0.0, 0.0);
    }
    let chain =
        voxel_draw_rotation_for_state(slope_type, slope_blend, voxel_facing_step(body_facing));
    let offset = Vec3::new(turret_offset_units(turret_offset_leptons), 0.0, 0.0);
    let disp: Vec3 = chain.transform_vector3(offset);
    (disp.x * scale, -disp.y * scale)
}

/// Screen displacement of a pitched barrel from its turret, in pixels.
///
/// `UnitClass::DrawVoxelBody` pitches the barrel in the turret's own frame
/// about the point `-t`, where `t` is the turret matrix's translation column:
/// the TurretOffset carried through the hull's slope and facing
/// (`0x0073BAD8..0x0073BBC2`: translate by `-t`, rotate about Y, translate by
/// `t`). So a barrel on an offset turret also moves by the turret rotation of
/// `Ry * t - t`; without an offset it pitches in place.
pub fn barrel_pivot_screen_offset(
    turret_offset_leptons: i32,
    body_facing: u8,
    turret_facing: u8,
    slope_type: u8,
    slope_blend: Option<VxlSlopeBlend>,
    barrel_pitch: i8,
) -> (f32, f32) {
    if turret_offset_leptons == 0 || barrel_pitch == 0 {
        return (0.0, 0.0);
    }
    let slope = slope_blend.map_or_else(
        || compute_slope_rotation(if slope_type < 17 { slope_type } else { 0 }),
        compute_slope_blend_rotation,
    );
    let hull = affine::matrix_product(slope, voxel_body_facing(voxel_facing_step(body_facing)))
        .expect("finite voxel rotation");
    let t = hull.transform_vector3(Vec3::new(
        turret_offset_units(turret_offset_leptons),
        0.0,
        0.0,
    ));
    let pitched = voxel_barrel_pitch(Mat4::IDENTITY, barrel_pitch).transform_vector3(t);
    let turret =
        voxel_draw_rotation_for_state(slope_type, slope_blend, voxel_facing_step(turret_facing));
    let disp = turret.transform_vector3(pitched - t);
    (disp.x, -disp.y)
}

/// Building draw43DA80's B8/C0 offsets, in pixels (+Y down), before their
/// shared TurretAnimX/Y anchor. Buildings truncate TurretOffset / 8 instead
/// of using the unit's lepton scale. Each recoil translates its own part
/// along local -X; the barrel then pitches about the turret translation.
/// A C0-only building does none of these translations and must not call it.
///
/// The rotation/pitch owners are shared with the voxel rasterizer. Executed
/// native matrices in tools/voxel_oracle/building_barrel.json bound the
/// separate float projection to less than 0.0001 pixel roundoff.
pub fn building_gun_screen_offsets(
    turret_offset: i32,
    facing: u8,
    barrel_pitch: i8,
    recoil: [f32; 2],
) -> [[f32; 2]; 2] {
    let yaw = voxel_body_facing(voxel_facing_step(facing));
    let translation = yaw.transform_vector3(Vec3::new((turret_offset / 8) as f32, 0.0, 0.0));
    let turret = translation - yaw.x_axis.truncate() * recoil[0];
    let pitched = voxel_barrel_pitch(Mat4::IDENTITY, barrel_pitch).transform_vector3(translation);
    let barrel = translation + yaw.transform_vector3(pitched - translation)
        - yaw.x_axis.truncate() * recoil[1];
    [turret, barrel].map(|translation| {
        let screen = voxel_camera_view().transform_vector3(translation);
        [screen.x, -screen.y]
    })
}

#[cfg(test)]
#[path = "vxl_building_tests.rs"]
mod building_tests;

/// Margin in pixels added around the sprite to avoid clipping.
const SPRITE_MARGIN: u32 = 2;

/// Retained 16.16 projection for magnified previews and shadow preparation.
/// Ordinary encoded VXL pixels instead use the native packed 8.8 owner.
const FP_SHIFT: i32 = 16;
const FP_SCALE: f32 = (1 << FP_SHIFT) as f32; // 65536.0

/// Configuration for rendering a single VXL model frame.
#[derive(Debug, Clone)]
pub struct VxlRenderParams {
    /// HVA animation frame index (0-based). Use 0 for idle pose.
    pub frame: u32,
    /// Facing angle: 0–255 maps to 0–360° (RA2 convention).
    pub facing: u8,
    /// Terrain slope type (0–16). 0 = flat, 1-4 = edge ramps (full-edge tilt),
    /// 5-8 = corner ramps (corner tilt at NW/NE/SE/SW), 9-12 = corner tilt at
    /// NW/NE/SE/SW (byte-identical aliases of 5-8 in gamemd.exe), 13-16 = edge
    /// tilt at NW/NE/SE/SW. Original startup initializes slopes17-20 to identity.
    pub slope_type: u8,
    /// Optional 3-frame slope transition. When present, this replaces
    /// `slope_type` with an interpolated slope orientation.
    pub slope_blend: Option<VxlSlopeBlend>,
    /// A posed body's locomotor Draw_Matrix arm: a crash tilt's roll and
    /// pitch (`TechnoClass+0x328`, `+0x32C`) or a rocket's CurrentPitch
    /// (`RocketLocomotionClass+0x54`). When present the body draws through
    /// that arm instead of the slope matrices.
    pub body_tilt: Option<BodyTilt>,
    /// Original753D00 for ordinary bodies; Bullet46B0C0 and VoxelAnim749B70
    /// choose diffuse753C80 regardless of their matrix/HVA policy.
    pub lighting: VxlLighting,
    /// A barrel part's pitch step, `d32 - 8` of the barrel elevation
    /// (`TechnoClass+0x370`); 0 draws it level. Applies to the slope arm
    /// only: no tilting body carries a barrel in retail data.
    pub barrel_pitch: i8,
    /// Model-space-unit to pixel scale. Default: 1.0 — one unit is one pixel.
    ///
    /// The original applies no magnification anywhere between the section
    /// transform and the pixel write: the camera matrix is a pure rotation, and
    /// the only constants on the path are a ×256 (an 8.8 fixed-point shift, since
    /// the rasterizer indexes its 256×256 visibility map with the *high byte* of
    /// each 16-bit coordinate) and a +128 that centres the model in that buffer.
    /// So the VXL bounding-box units are already screen pixels.
    ///
    /// Kept as a field rather than a constant because the asset contact-sheet
    /// tool magnifies deliberately; production always leaves it at 1.0.
    pub scale: f32,
    /// The drawn TechnoType's `UseBuffer=` (0xB43180 during `0x00706ED0`):
    /// rasterize through the depth-tested leaves (`vxl_native`). False for
    /// every draw outside TechnoClass's voxel body, turret and barrel.
    pub use_buffer: bool,
    /// Ambient light intensity for fallback N·L shading. Default: 0.6.
    pub ambient: f32,
    /// Diffuse light intensity for fallback N·L shading. Default: 0.4.
    pub diffuse: f32,
    /// Light direction for fallback shading (when no VPL).
    pub light_dir: Vec3,
}

impl Default for VxlRenderParams {
    fn default() -> Self {
        let pitch: f32 = 50.0_f32.to_radians();
        let yaw: f32 = 240.0_f32.to_radians();
        let light_dir: Vec3 = Vec3::new(
            yaw.cos() * pitch.cos(),
            yaw.sin() * pitch.cos(),
            pitch.sin(),
        );
        Self {
            frame: 0,
            facing: 0,
            slope_type: 0,
            slope_blend: None,
            body_tilt: None,
            lighting: VxlLighting::BlinnPhong,
            barrel_pitch: 0,
            scale: 1.0,
            use_buffer: false,
            ambient: 0.6,
            diffuse: 0.4,
            light_dir,
        }
    }
}

/// A rendered 2D sprite produced by the software voxel rasterizer.
#[derive(Debug, Clone)]
pub struct VxlSprite {
    /// Palette-index pixel data (row-major, width × height bytes).
    /// Each byte is the post-VPL-shaded, pre-house-remap palette index.
    /// Byte 0 = transparent, including a zero VPL result that erased a prior
    /// voxel. This matches the original visibility-map convention. House remap
    /// and theater palette lookup happen at fragment-shader time.
    pub palette_indices: Vec<u8>,
    /// Per-pixel depth buffer (width × height floats). Used for depth-correct
    /// compositing of body/turret/barrel layers. NEG_INFINITY = no voxel.
    pub depth: Vec<f32>,
    /// Sprite width in pixels.
    pub width: u32,
    /// Sprite height in pixels.
    pub height: u32,
    /// X offset from model center to sprite top-left.
    pub offset_x: f32,
    /// Y offset from model center to sprite top-left.
    pub offset_y: f32,
}

// ---------------------------------------------------------------------------
// Packed voxel grid — 3D lookup built from the sparse Vec<VxlVoxel>.
// High byte = color_index, low byte = normal_index. Zero = empty cell.
// ---------------------------------------------------------------------------

/// Packed voxel data: `(color_index << 8) | normal_index`. Zero = empty.
type PackedVoxel = u16;

fn pack_voxel(color_index: u8, normal_index: u8) -> PackedVoxel {
    (color_index as u16) << 8 | normal_index as u16
}

fn unpack_color(v: PackedVoxel) -> u8 {
    (v >> 8) as u8
}

fn unpack_normal(v: PackedVoxel) -> u8 {
    (v & 0xFF) as u8
}

/// Build a dense 3D grid from a limb's sparse voxel list.
/// Indexed as `grid[x * sy * sz + y * sz + z]`.
fn build_voxel_grid(limb: &VxlLimb) -> Vec<PackedVoxel> {
    let sy: usize = limb.size_y as usize;
    let sz: usize = limb.size_z as usize;
    let total: usize = limb.size_x as usize * sy * sz;
    let mut grid: Vec<PackedVoxel> = vec![0u16; total];
    for v in &limb.voxels {
        let idx: usize = v.x as usize * sy * sz + v.y as usize * sz + v.z as usize;
        grid[idx] = pack_voxel(v.color_index, v.normal_index);
    }
    grid
}

// ---------------------------------------------------------------------------
// Back-to-front axis iteration — determines which direction to walk each
// axis so that farther voxels are processed first (painter's algorithm).
// ---------------------------------------------------------------------------

/// Iterator parameters for one axis: start, exclusive end, step direction.
struct AxisIter {
    start: i32,
    end: i32,
    step: i32,
}

impl AxisIter {
    /// Produce an iterator yielding values from start toward end by step.
    fn iter(&self) -> AxisRange {
        AxisRange {
            current: self.start,
            end: self.end,
            step: self.step,
        }
    }
}

struct AxisRange {
    current: i32,
    end: i32,
    step: i32,
}

impl Iterator for AxisRange {
    type Item = i32;
    fn next(&mut self) -> Option<i32> {
        if self.step > 0 && self.current >= self.end {
            return None;
        }
        if self.step < 0 && self.current <= self.end {
            return None;
        }
        let val: i32 = self.current;
        self.current += self.step;
        Some(val)
    }
}

/// Choose iteration direction for one axis based on the camera transform.
/// If moving along +axis increases depth (closer to camera), iterate
/// low→high so far voxels are drawn first and near voxels overwrite.
fn axis_order(size: u8, depth_contribution: f32) -> AxisIter {
    if depth_contribution >= 0.0 {
        // +axis = closer to camera → iterate low (far) to high (near).
        AxisIter {
            start: 0,
            end: size as i32,
            step: 1,
        }
    } else {
        // +axis = farther from camera → iterate high (far) to low (near).
        AxisIter {
            start: size as i32 - 1,
            end: -1,
            step: -1,
        }
    }
}

// ---------------------------------------------------------------------------
// Precomputed per-limb data gathered before rendering begins.
// ---------------------------------------------------------------------------

/// Per-limb data for retained previews and separate shadow preparation.
/// Ordinary CPU/GPU VXL paint uses the encoded native owner instead.
pub struct LimbRenderData {
    pub grid: Vec<PackedVoxel>,
    pub combined: Mat4,
    /// `slope × body facing × section` — the draw matrix before the camera,
    /// kept so the shadow bake can flatten in world space.
    pub model_to_world: Mat4,
    pub vpl_pages: [u8; 256],
    pub normals_mode: u8,
    pub size_x: u8,
    pub size_y: u8,
    pub size_z: u8,
}

/// Bounding box and layout info for a rendered VXL sprite.
/// Produced by `compute_sprite_bounds` for retained previews and shadow bakes.
pub struct SpriteBounds {
    pub width: u32,
    pub height: u32,
    pub fill_size: i32,
    pub half_fill: i32,
    pub buf_off_x_fp: i32,
    pub buf_off_y_fp: i32,
    pub offset_x: f32,
    pub offset_y: f32,
}

/// Native VXL destination rectangle relative to the part's draw anchor.
/// `VXL_Submit_BoundingBox 0x7540F0` transforms the full tailer min/max
/// extents (grid 0..size), unions all limbs, and `0x754510` pads that one
/// union. This is intentionally independent of atlas voxel-center storage
/// bounds, SPRITE_MARGIN, splat footprint and zero/nonzero palette pixels.
///
/// Parsed production VXLs use the same exact corner/crop owner as their
/// raster. Magnified previews and manually constructed diagnostics retain
/// the old bounds calculation below. Caller slope/turret matrices retain
/// their separately documented parity boundaries.
pub fn native_vxl_draw_bounds(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
) -> Option<[i32; 4]> {
    let draw_matrix = voxel_params_draw_rotation(params, voxel_facing_step(params.facing));
    if params.scale == 1.0 && vxl.limbs.iter().all(|limb| limb.native_spans.is_some()) {
        let geometry =
            native::prepare_geometry(vxl, hva, params.frame, draw_matrix, params.use_buffer)?;
        let [x, y, _, _, width, height] = geometry.rect;
        return Some([x, y, width, height]);
    }
    let mut minimum = [f32::INFINITY; 2];
    let mut maximum = [f32::NEG_INFINITY; 2];
    for (limb_index, limb) in vxl.limbs.iter().enumerate() {
        if limb.size_x == 0 || limb.size_y == 0 || limb.size_z == 0 {
            return None;
        }
        let sizes = Vec3::new(limb.size_x as f32, limb.size_y as f32, limb.size_z as f32);
        let minimum_model = Vec3::new(limb.bounds[0], limb.bounds[1], limb.bounds[2]);
        let maximum_model = Vec3::new(limb.bounds[3], limb.bounds[4], limb.bounds[5]);
        let section_scale = Mat4::from_scale((maximum_model - minimum_model) / sizes);
        let raw = hva
            .and_then(|h| h.get_transform(params.frame, limb_index as u32))
            .unwrap_or(&limb.transform);
        let section =
            Mat4::from_translation(minimum_model) * hva_to_mat4(raw, limb.scale) * section_scale;
        let combined = draw_matrix * section;
        for corner in 0..8 {
            let point = Vec3::new(
                if corner & 1 != 0 { sizes.x } else { 0.0 },
                if corner & 2 != 0 { sizes.y } else { 0.0 },
                if corner & 4 != 0 { sizes.z } else { 0.0 },
            );
            let projected = combined.transform_point3(point);
            let xy = [projected.x * params.scale, -projected.y * params.scale];
            for axis in 0..2 {
                if !xy[axis].is_finite() {
                    return None;
                }
                minimum[axis] = minimum[axis].min(xy[axis]);
                maximum[axis] = maximum[axis].max(xy[axis]);
            }
        }
    }
    native_draw_bounds_from_extents(minimum, maximum)
}

/// `0x754513..0x75470F`: extrema are stored f32, center is computed in x87
/// and stored f32 once, whereas span reaches truncating ftol without an
/// intermediate f32 store. Use the shared x87 owner for the chopped f32
/// center store; a Rust f64-to-f32 cast rounds to nearest instead.
/// Returns destination x/y/width/height; native's additional source x/y
/// output is for its 256x256 temporary surface, not this atlas allocation.
pub fn native_draw_bounds_from_extents(minimum: [f32; 2], maximum: [f32; 2]) -> Option<[i32; 4]> {
    use crate::util::native_x87::{NativeF32Bits, NativeF64Bits, X87Chop53};

    let mut result = [0; 4];
    for axis in 0..2 {
        if maximum[axis] < minimum[axis] {
            return None;
        }
        let min = X87Chop53::load_f32(NativeF32Bits::from_bits(minimum[axis].to_bits())).ok()?;
        let max = X87Chop53::load_f32(NativeF32Bits::from_bits(maximum[axis].to_bits())).ok()?;
        let half = X87Chop53::load_f64(NativeF64Bits::HALF).ok()?;
        let center = X87Chop53::store_f32(X87Chop53::mul(X87Chop53::add(min, max), half)).ok()?;
        let center =
            i32::try_from(X87Chop53::ftol_i64(X87Chop53::load_f32(center).ok()?).ok()?).ok()?;
        let span = i32::try_from(X87Chop53::ftol_i64(X87Chop53::sub(max, min)).ok()?).ok()?;
        // Invalid or unrepresentable assets yield explicit missing metadata.
        let extent = span.checked_add(8)?;
        result[axis] = center.checked_sub(extent / 2)?;
        result[axis + 2] = extent;
    }
    Some(result)
}

/// Ordered dirty-rectangle update from cached VXL blit 0x70755D..0x707678.
/// Rectangles are x/y/width/height in one shared pixel coordinate frame.
/// Expanding a maximum edge adds one; expanding only a minimum edge does
/// not. An empty accumulator is replaced. This is not a normal AABB union.
pub fn union_native_voxel_draw_bounds(accumulated: &mut Option<[i32; 4]>, next: [i32; 4]) {
    let Some(mut current) = *accumulated else {
        *accumulated = Some(next);
        return;
    };
    if current[2] <= 0 || current[3] <= 0 {
        *accumulated = Some(next);
        return;
    }
    if next[2] <= 0 || next[3] <= 0 {
        return;
    }
    for axis in 0..2 {
        if next[axis] < current[axis] {
            current[axis + 2] += current[axis] - next[axis];
            current[axis] = next[axis];
        }
        let next_end = next[axis] + next[axis + 2];
        if next_end > current[axis] + current[axis + 2] {
            current[axis + 2] = next_end - current[axis] + 1;
        }
    }
    *accumulated = Some(current);
}

/// Precompute per-limb transforms, voxel grids, lighting pages, and footprints.
///
/// This is Phase 1 of the VXL render pipeline. It builds the combined
/// world+section transform for each non-empty limb, computes VPL brightness
/// pages, and returns the maximum voxel footprint for retained previews and
/// the separate shadow owner. Ordinary CPU/GPU paint uses native preparation.
pub fn prepare_limb_data(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
) -> (Vec<LimbRenderData>, f32) {
    // Facing is quantized to 32 steps before the rotation matrix is built, so
    // every voxel body, turret and barrel renders at exactly 1 of 32 orientations.
    let facing_step: u8 = voxel_facing_step(params.facing);
    let scale: f32 = params.scale;

    // The simple (no-rock) draw path builds `slope_matrix * facing_rotation`, then
    // the render step applies the camera/view matrix outside that result. Keep
    // facing separate so terrain slope stays world/cell-oriented instead of
    // rotating with the unit body.
    //
    // Both camera rotations are negative. The signs are load-bearing: the body term
    // carries a compensating +90°, and although the two cancel on flat ground they
    // sit on opposite sides of `slope_mat`, which does not commute with a Z rotation.
    // Flipping either sign tilts ramped units about an axis 90° away from the
    // original's while leaving flat ground looking correct.
    let body_facing: Mat4 = voxel_body_facing(facing_step);

    // Terrain slope is a property of the cell, not the limb: one matrix per draw.
    let slope_mat: Mat4 = params
        .slope_blend
        .map(compute_slope_blend_rotation)
        .unwrap_or_else(|| compute_slope_rotation(params.slope_type));

    let draw_matrix = voxel_params_draw_rotation(params, facing_step);
    let draw_rotation = Mat3::from_mat4(draw_matrix);
    let model_rotation =
        affine::matrix_product(slope_mat, body_facing).expect("finite voxel rotation");
    let mut limb_data: Vec<LimbRenderData> = Vec::new();
    let mut max_footprint: f32 = 1.0;

    for (limb_idx, limb) in vxl.limbs.iter().enumerate() {
        if limb.voxels.is_empty() {
            continue;
        }

        // Ordinary UnitClass body draw at 0x0073B70E..0x0073B742 passes
        // camera * locomotor to TechnoClass::Render, so lighting includes
        // the camera but precedes the HVA transform. See the executable
        // vectors in tools/voxel_oracle/lighting.py and vxl_normals.rs.
        let vpl_pages: [u8; 256] = params.lighting.pages(limb.normals_mode, draw_rotation);

        // Section scale: maps grid coordinates to model-space units.
        let sx: f32 = if limb.size_x > 0 {
            (limb.bounds[3] - limb.bounds[0]) / limb.size_x as f32
        } else {
            1.0
        };
        let sy: f32 = if limb.size_y > 0 {
            (limb.bounds[4] - limb.bounds[1]) / limb.size_y as f32
        } else {
            1.0
        };
        let sz: f32 = if limb.size_z > 0 {
            (limb.bounds[5] - limb.bounds[2]) / limb.size_z as f32
        } else {
            1.0
        };

        let section_scale: Mat4 = Mat4::from_scale(Vec3::new(sx, sy, sz));
        let section_translate: Mat4 =
            Mat4::from_translation(Vec3::new(limb.bounds[0], limb.bounds[1], limb.bounds[2]));

        let bone_mat: Mat4 = match hva {
            Some(h) => match h.get_transform(params.frame, limb_idx as u32) {
                Some(raw) => hva_to_mat4(raw, limb.scale),
                None => hva_to_mat4(&limb.transform, limb.scale),
            },
            None => hva_to_mat4(&limb.transform, limb.scale),
        };

        let section_transform: Mat4 = section_translate * bone_mat * section_scale;
        let combined: Mat4 = draw_matrix * section_transform;

        let footprint: f32 = compute_voxel_footprint(&combined, scale);
        if footprint > max_footprint {
            max_footprint = footprint;
        }

        let grid: Vec<PackedVoxel> = build_voxel_grid(limb);

        limb_data.push(LimbRenderData {
            grid,
            combined,
            model_to_world: model_rotation * section_transform,
            vpl_pages,
            normals_mode: limb.normals_mode,
            size_x: limb.size_x,
            size_y: limb.size_y,
            size_z: limb.size_z,
        });
    }

    (limb_data, max_footprint)
}

/// Stencil value written for shadow pixels; the voxel sprite shader only tests
/// non-zero when `FX_SHADOW` is set, so the value itself is not a palette index.
pub const SHADOW_STENCIL_INDEX: u8 = 1;

/// Screen-space shift of the shadow footprint, in pixels.
///
/// `VXL_LightDirection_Setup` (0x00754C00) stores, next to the light vector,
/// `(-6 × light.x, 0, 0)` at 0x00887420 (constant -6.0 at 0x007F6950): the
/// voxel shadow light vector. With the binary's light.x = -0.5 that is
/// (3, 0, 0), added to every flattened corner after the camera transform and
/// before the Y flip in `VXL_Submit_Billboard` (0x00753F90), i.e. three pixels
/// to the right. Kept as the binary's constant rather than derived from this
/// renderer's model-space lighting vector (see `vxl_normals`).
const SHADOW_LIGHT_OFFSET_PX: f32 = 3.0;

/// Ground shadow stencil. The ordinary flat single-section route follows
/// original 753F90/754510/756860 corner, 8.8 column and crop ownership.
/// Its first composed-body mask is applied by UnitAtlas before presentation.
/// Other geometry retains the approximate occupied-column fallback below.
/// Final destination darkening is separate: the current voxel fragment uses
/// a linear-alpha approximation to native packed RGB565 half, not exact half.
#[cfg(test)]
pub fn render_vxl_shadow(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
) -> VxlSprite {
    if let Some(sprite) = shadow::render(vxl, hva, params, 0, false) {
        return sprite;
    }
    render_legacy_vxl_shadow(vxl, hva, params)
}

pub(crate) fn render_legacy_vxl_shadow(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
) -> VxlSprite {
    let (limbs, _) = prepare_limb_data(vxl, hva, params);
    if limbs.is_empty() {
        return VxlSprite {
            palette_indices: vec![0u8; 4],
            depth: Vec::new(),
            width: 2,
            height: 2,
            offset_x: 0.0,
            offset_y: 0.0,
        };
    }
    let scale: f32 = params.scale;
    let shift: Mat4 = Mat4::from_translation(Vec3::new(SHADOW_LIGHT_OFFSET_PX, 0.0, 0.0));
    let camera: Mat4 = voxel_camera_view();

    // Bottom-face limbs: same grids, the full draw matrix, one voxel of height
    // so the shared bounds routine measures the z-min face's parallelogram.
    let flat: Vec<LimbRenderData> = limbs
        .iter()
        .map(|ld| LimbRenderData {
            grid: Vec::new(),
            combined: shift * camera * ld.model_to_world,
            model_to_world: ld.model_to_world,
            vpl_pages: ld.vpl_pages,
            normals_mode: ld.normals_mode,
            size_x: ld.size_x,
            size_y: ld.size_y,
            size_z: 1,
        })
        .collect();
    let mut max_footprint: f32 = 1.0;
    for ld in &flat {
        max_footprint = max_footprint.max(compute_voxel_footprint(&ld.combined, scale));
    }
    let bounds: SpriteBounds = compute_sprite_bounds(&flat, scale, max_footprint);
    let width: u32 = bounds.width;
    let height: u32 = bounds.height;
    let mut stencil: Vec<u8> = vec![0u8; (width * height) as usize];
    let fill_size: i32 = bounds.fill_size;
    let half_fill: i32 = bounds.half_fill;

    for (src, ld) in limbs.iter().zip(flat.iter()) {
        let sy: usize = src.size_y as usize;
        let sz: usize = src.size_z as usize;
        for ix in 0..src.size_x {
            for iy in 0..src.size_y {
                let base: usize = ix as usize * sy * sz + iy as usize * sz;
                if !src.grid[base..base + sz].iter().any(|&v| v != 0) {
                    continue;
                }
                let world: Vec3 = ld
                    .combined
                    .transform_point3(Vec3::new(ix as f32, iy as f32, 0.0));
                let sx_fp: i32 = (world.x * scale * FP_SCALE) as i32;
                let sy_fp: i32 = (-world.y * scale * FP_SCALE) as i32;
                let px: i32 = (sx_fp + bounds.buf_off_x_fp) >> FP_SHIFT;
                let py: i32 = (sy_fp + bounds.buf_off_y_fp) >> FP_SHIFT;
                for dy in -half_fill..=(fill_size - 1 - half_fill) {
                    for dx in -half_fill..=(fill_size - 1 - half_fill) {
                        let fx: i32 = px + dx;
                        let fy: i32 = py + dy;
                        if fx < 0 || fy < 0 || fx >= width as i32 || fy >= height as i32 {
                            continue;
                        }
                        stencil[fy as usize * width as usize + fx as usize] = SHADOW_STENCIL_INDEX;
                    }
                }
            }
        }
    }

    VxlSprite {
        palette_indices: stencil,
        depth: Vec::new(),
        width,
        height,
        offset_x: bounds.offset_x,
        offset_y: bounds.offset_y,
    }
}

/// Compute the sprite bounding box from precomputed limb transforms.
///
/// This is Phase 2 of the VXL render pipeline. It projects the 8 corners of
/// each limb's voxel grid through the combined transform to find the screen-
/// space bounding box, then computes pixel dimensions and buffer offsets.
/// Used by retained previews and shadow bakes, not ordinary encoded paint.
pub fn compute_sprite_bounds(
    limb_data: &[LimbRenderData],
    scale: f32,
    max_footprint: f32,
) -> SpriteBounds {
    let fill_size: i32 = max_footprint.ceil().max(1.0) as i32;
    let half_fill: i32 = fill_size / 2;

    let mut min_x_fp: i32 = i32::MAX;
    let mut max_x_fp: i32 = i32::MIN;
    let mut min_y_fp: i32 = i32::MAX;
    let mut max_y_fp: i32 = i32::MIN;

    for ld in limb_data {
        let gx: f32 = (ld.size_x as i32 - 1).max(0) as f32;
        let gy: f32 = (ld.size_y as i32 - 1).max(0) as f32;
        let gz: f32 = (ld.size_z as i32 - 1).max(0) as f32;
        let corners: [Vec3; 8] = [
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(gx, 0.0, 0.0),
            Vec3::new(0.0, gy, 0.0),
            Vec3::new(gx, gy, 0.0),
            Vec3::new(0.0, 0.0, gz),
            Vec3::new(gx, 0.0, gz),
            Vec3::new(0.0, gy, gz),
            Vec3::new(gx, gy, gz),
        ];
        for corner in &corners {
            let world_pos: Vec3 = ld.combined.transform_point3(*corner);
            let sx_fp: i32 = (world_pos.x * scale * FP_SCALE) as i32;
            let sy_fp: i32 = (-world_pos.y * scale * FP_SCALE) as i32;
            if sx_fp < min_x_fp {
                min_x_fp = sx_fp;
            }
            if sx_fp > max_x_fp {
                max_x_fp = sx_fp;
            }
            if sy_fp < min_y_fp {
                min_y_fp = sy_fp;
            }
            if sy_fp > max_y_fp {
                max_y_fp = sy_fp;
            }
        }
    }

    let width: u32 =
        ((max_x_fp - min_x_fp) >> FP_SHIFT) as u32 + 1 + SPRITE_MARGIN * 2 + fill_size as u32;
    let height: u32 =
        ((max_y_fp - min_y_fp) >> FP_SHIFT) as u32 + 1 + SPRITE_MARGIN * 2 + fill_size as u32;

    let margin_fp: i32 = (SPRITE_MARGIN as i32 + half_fill) << FP_SHIFT;
    let buf_off_x_fp: i32 = -min_x_fp + margin_fp;
    let buf_off_y_fp: i32 = -min_y_fp + margin_fp;

    let offset_x: f32 = (min_x_fp >> FP_SHIFT) as f32 - SPRITE_MARGIN as f32 - half_fill as f32;
    let offset_y: f32 = (min_y_fp >> FP_SHIFT) as f32 - SPRITE_MARGIN as f32 - half_fill as f32;

    SpriteBounds {
        width,
        height,
        fill_size,
        half_fill,
        buf_off_x_fp,
        buf_off_y_fp,
        offset_x,
        offset_y,
    }
}

/// Render a VXL model to a 2D RGBA sprite using back-to-front spatial iteration.
///
/// Voxels are iterated in spatial order determined by the camera transform so
/// that closer voxels naturally overwrite farther ones (painter's algorithm).
/// Each voxel is projected and drawn as a small filled rectangle. Z-buffer is
/// maintained for inter-limb occlusion and downstream body/turret compositing.
pub fn render_vxl(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
    vpl: Option<&VplFile>,
) -> VxlSprite {
    if let Some(draw) = prepare_native_draw(vxl, hva, params, vpl)
        && let Some(sprite) = draw.render_cpu()
    {
        return sprite;
    }
    // Magnified previews, constructed diagnostic models, or invalid native
    // streams/crops use the prior general projection. Production stock VXLs
    // use the encoded native path above; this fallback is not a parity claim.
    render_vxl_legacy(vxl, hva, params, vpl)
}

fn render_vxl_legacy(
    vxl: &VxlFile,
    hva: Option<&HvaFile>,
    params: &VxlRenderParams,
    vpl: Option<&VplFile>,
) -> VxlSprite {
    let scale: f32 = params.scale;

    // Phase 1: Precompute per-limb transforms, grids, and footprints.
    let (limb_data, max_footprint) = prepare_limb_data(vxl, hva, params);

    // Handle empty models.
    if limb_data.is_empty() {
        return VxlSprite {
            palette_indices: vec![0],
            depth: vec![f32::NEG_INFINITY],
            width: 1,
            height: 1,
            offset_x: 0.0,
            offset_y: 0.0,
        };
    }

    // Phase 2: Compute bounding box from grid corners (fixed-point).
    let bounds: SpriteBounds = compute_sprite_bounds(&limb_data, scale, max_footprint);
    let width: u32 = bounds.width;
    let height: u32 = bounds.height;
    let pixel_count: usize = width as usize * height as usize;

    // Atlas pixels = post-VPL palette indices. Byte 0 = transparent (matches
    // the engine's visibility-map convention; the fragment shader discards 0).
    let mut palette_indices: Vec<u8> = vec![0u8; pixel_count];
    let mut depth_buf: Vec<f32> = vec![f32::NEG_INFINITY; pixel_count];

    let fill_size: i32 = bounds.fill_size;
    let half_fill: i32 = bounds.half_fill;
    let buf_off_x_fp: i32 = bounds.buf_off_x_fp;
    let buf_off_y_fp: i32 = bounds.buf_off_y_fp;

    // --- Phase 3: Back-to-front spatial iteration per limb ---

    for ld in &limb_data {
        // Determine iteration direction per axis from the camera transform.
        // The Z component of the transformed axis vector tells us which
        // direction along that axis is "toward the camera" (higher depth).
        let depth_x: f32 = ld.combined.transform_vector3(Vec3::X).z;
        let depth_y: f32 = ld.combined.transform_vector3(Vec3::Y).z;
        let depth_z: f32 = ld.combined.transform_vector3(Vec3::Z).z;

        let iter_x: AxisIter = axis_order(ld.size_x, depth_x);
        let iter_y: AxisIter = axis_order(ld.size_y, depth_y);
        let iter_z: AxisIter = axis_order(ld.size_z, depth_z);

        let sy: usize = ld.size_y as usize;
        let sz: usize = ld.size_z as usize;

        for ix in iter_x.iter() {
            for iy in iter_y.iter() {
                for iz in iter_z.iter() {
                    let grid_idx: usize = ix as usize * sy * sz + iy as usize * sz + iz as usize;
                    let packed: PackedVoxel = ld.grid[grid_idx];
                    if packed == 0 {
                        continue;
                    }

                    let color_index: u8 = unpack_color(packed);
                    let normal_index: u8 = unpack_normal(packed);

                    // Post-VPL palette index — the byte the engine writes to
                    // its visibility map. House remap + RGB lookup happen at
                    // fragment-shader time. No-VPL path falls back to raw
                    // color_index (no per-pixel diffuse shading on this path;
                    // production always has VPL loaded).
                    let final_color_index: u8 = match vpl {
                        Some(vpl_file) => {
                            let page: u8 = ld.vpl_pages[normal_index as usize];
                            vpl_file.get_palette_index(page, color_index)
                        }
                        None => color_index,
                    };
                    // Retained preview behavior, not a native invariant. The
                    // ordinary encoded raster always stores zero VPL results.
                    let final_color_index: u8 = if final_color_index == 0 {
                        color_index
                    } else {
                        final_color_index
                    };

                    // Project voxel center to screen space (fixed-point truncation).
                    let center: Vec3 = Vec3::new(ix as f32, iy as f32, iz as f32);
                    let world_pos: Vec3 = ld.combined.transform_point3(center);
                    let depth: f32 = world_pos.z;

                    // 16.16 fixed-point projection with truncation (>> 16).
                    // The `as i32` cast truncates toward zero, matching the
                    // original RA2/TS integer math pixel-snapping behavior.
                    let sx_fp: i32 = (world_pos.x * scale * FP_SCALE) as i32;
                    let sy_fp: i32 = (-world_pos.y * scale * FP_SCALE) as i32;
                    let px: i32 = (sx_fp + buf_off_x_fp) >> FP_SHIFT;
                    let py: i32 = (sy_fp + buf_off_y_fp) >> FP_SHIFT;

                    // Plot a filled rectangle centered on the projected point.
                    // Z-buffer check needed for inter-limb occlusion.
                    for dy in -half_fill..=(fill_size - 1 - half_fill) {
                        for dx in -half_fill..=(fill_size - 1 - half_fill) {
                            let fx: i32 = px + dx;
                            let fy: i32 = py + dy;
                            if fx < 0 || fy < 0 || fx >= width as i32 || fy >= height as i32 {
                                continue;
                            }
                            let buf_idx: usize = fy as usize * width as usize + fx as usize;
                            if depth < depth_buf[buf_idx] {
                                continue;
                            }
                            depth_buf[buf_idx] = depth;
                            palette_indices[buf_idx] = final_color_index;
                        }
                    }
                }
            }
        }
    }

    VxlSprite {
        palette_indices,
        depth: depth_buf,
        width,
        height,
        offset_x: bounds.offset_x,
        offset_y: bounds.offset_y,
    }
}

/// Compute the screen-space pixel footprint for one voxel unit.
///
/// Projects the 3 unit axis vectors (1,0,0), (0,1,0), (0,0,1) through the
/// combined section+world transform and returns the maximum screen-space
/// distance. This determines how large a rectangle to draw for each voxel.
pub fn compute_voxel_footprint(combined: &Mat4, scale: f32) -> f32 {
    let origin: Vec3 = combined.transform_point3(Vec3::ZERO);
    let mut max_dist: f32 = 0.0;
    for axis in &[Vec3::X, Vec3::Y, Vec3::Z] {
        let projected: Vec3 = combined.transform_point3(*axis);
        let dx: f32 = (projected.x - origin.x) * scale;
        let dy: f32 = (projected.y - origin.y) * scale;
        let dist: f32 = (dx * dx + dy * dy).sqrt();
        if dist > max_dist {
            max_dist = dist;
        }
    }
    max_dist
}

/// Convert HVA 3×4 row-major transform to glam Mat4.
/// Translation (indices 3, 7, 11) scaled by limb_scale.
pub fn hva_to_mat4(raw: &[f32; 12], limb_scale: f32) -> Mat4 {
    Mat4::from_cols(
        Vec4::new(raw[0], raw[4], raw[8], 0.0),
        Vec4::new(raw[1], raw[5], raw[9], 0.0),
        Vec4::new(raw[2], raw[6], raw[10], 0.0),
        Vec4::new(
            raw[3] * limb_scale,
            raw[7] * limb_scale,
            raw[11] * limb_scale,
            1.0,
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::vxl_file::{VxlLimb, VxlVoxel};
    use crate::sim::movement::slope_transition::SLOPE_TRANSITION_FRAMES;

    #[test]
    fn native_draw_rectangle_matches_executed_754510_vectors() {
        // Native execution, 2026-09-08: unmodified retail 0x754510, x87 CW
        // 0x0e7f, write f32 min/max at B2D5E0/B2D948, empty submission lists
        // B2D820/B2FB70, read its six-i32 result. SHA256/provenance in
        // UNIT_COMPOSITE_BRIDGE_SPLIT_73B140_GHIDRA_REPORT.md. Expected rows
        // retain destination x/y and width/height, excluding source x/y.
        let vectors = [
            ([-10.25, -6.75, 11.75, 8.5], [-15, -11, 30, 23]),
            ([-0.5, -0.5, 0.5, 0.5], [-4, -4, 9, 9]),
            ([-10.0, -20.0, 10.0, 20.0], [-14, -24, 28, 48]),
            ([-3.9, -2.9, 8.1, 7.1], [-8, -7, 20, 18]),
            ([0.0, 0.0, 30.0, 16.0], [-4, -4, 38, 24]),
            ([-20.0, -11.0, -5.0, -2.0], [-23, -14, 23, 17]),
            ([-1.1, -1.1, 1.0, 1.0], [-5, -5, 10, 10]),
            ([1.1, 1.1, 4.0, 4.0], [-3, -3, 10, 10]),
            ([-16.75, -9.5, 27.5, 21.25], [-21, -14, 52, 38]),
            ([-0.1, -0.1, 0.2, 0.2], [-4, -4, 8, 8]),
        ];
        for ([min_x, min_y, max_x, max_y], expected) in vectors {
            assert_eq!(
                native_draw_bounds_from_extents([min_x, min_y], [max_x, max_y]),
                Some(expected),
                "extents {min_x},{min_y}..{max_x},{max_y}"
            );
        }
    }

    #[test]
    fn native_draw_union_matches_executed_cached_blit_block() {
        // Execute original 70755D..707678 with old rectangle at B1CFC0 and
        // signed cached x/y, unsigned w/h. These are native outputs, not a
        // mathematical AABB-union oracle. The +1 maximum edge is deliberate.
        let vectors = [
            ([5, 6, 10, 12], [6, 7, 2, 3], [5, 6, 10, 12]),
            ([5, 6, 10, 12], [5, 6, 10, 12], [5, 6, 10, 12]),
            ([5, 6, 10, 12], [1, 2, 8, 8], [1, 2, 14, 16]),
            ([5, 6, 10, 12], [12, 16, 10, 10], [5, 6, 18, 21]),
            ([5, 6, 10, 12], [1, 2, 30, 40], [1, 2, 31, 41]),
            ([5, 6, 10, 12], [15, 18, 0, 0], [5, 6, 10, 12]),
            ([5, 6, 0, 12], [15, 18, 0, 0], [15, 18, 0, 0]),
            ([-20, -10, 8, 6], [-12, -4, 4, 4], [-20, -10, 13, 11]),
            ([0, 0, 0, 0], [124, 123, 20, 30], [124, 123, 20, 30]),
            ([124, 123, 20, 30], [124, 123, 20, 30], [124, 123, 20, 30]),
        ];
        for (initial, next, expected) in vectors {
            let mut accumulated = Some(initial);
            union_native_voxel_draw_bounds(&mut accumulated, next);
            assert_eq!(accumulated, Some(expected), "{initial:?} then {next:?}");
        }
    }

    #[test]
    fn native_draw_bounds_use_model_extents_even_when_grid_or_opaque_pixels_change() {
        let mut model = make_test_vxl();
        model.limbs[0].bounds = [-12.0, -8.0, -3.0, 12.0, 8.0, 7.0];
        let params = VxlRenderParams {
            facing: 40,
            slope_type: 3,
            ..Default::default()
        };
        let original = native_vxl_draw_bounds(&model, None, &params).unwrap();
        // Same physical tailer box, different grid resolution and no opaque
        // voxels. Native submits the header box, not occupied voxel centers.
        model.limbs[0].size_x = 4;
        model.limbs[0].size_y = 4;
        model.limbs[0].size_z = 4;
        model.limbs[0].voxels.clear();
        assert_eq!(
            native_vxl_draw_bounds(&model, None, &params),
            Some(original)
        );
        let mut second = make_test_vxl().limbs.remove(0);
        second.bounds = model.limbs[0].bounds;
        second.bounds[1] += 40.0;
        second.bounds[4] += 40.0;
        model.limbs.push(second);
        let expanded = native_vxl_draw_bounds(&model, None, &params).unwrap();
        assert!(expanded[2] > original[2] || expanded[3] > original[3]);
        assert!(native_draw_bounds_from_extents([f32::NAN, 0.0], [1.0, 1.0]).is_none());
    }

    #[test]
    fn prepared_lighting_matches_native_all_flat_facings() {
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/lighting.json",
        ))
        .unwrap();
        let mut vxl = make_test_vxl();
        for case in vectors["cases"].as_array().unwrap() {
            let step = case["step"].as_u64().unwrap() as u8;
            let mode = case["mode"].as_u64().unwrap() as u8;
            vxl.limbs[0].normals_mode = mode;
            let params = VxlRenderParams {
                facing: step * 8,
                ..Default::default()
            };
            let (limbs, _) = prepare_limb_data(&vxl, None, &params);
            let matrix = voxel_draw_rotation(Mat4::IDENTITY, voxel_body_facing(step));
            for row in 0..3 {
                for col in 0..3 {
                    assert_eq!(
                        matrix.col(col)[row].to_bits(),
                        case["draw_matrix_bits"][row * 4 + col].as_u64().unwrap() as u32,
                        "matrix step {step} row {row} col {col}"
                    );
                }
            }
            let expected = case["pages"].as_str().unwrap();
            for normal in 0..256 {
                let page = u8::from_str_radix(&expected[normal * 2..normal * 2 + 2], 16).unwrap();
                assert_eq!(
                    limbs[0].vpl_pages[normal], page,
                    "prepared step {step} mode {mode} normal {normal}"
                );
            }
        }
    }

    #[test]
    fn blended_rotation_cache_preserves_exact_inputs_and_fallbacks() {
        for blend in [
            VxlSlopeBlend {
                from_slope: 0,
                to_slope: 4,
                phase_num: -1,
                phase_den: 3,
            },
            VxlSlopeBlend {
                from_slope: 0,
                to_slope: 4,
                phase_num: 1,
                phase_den: 3,
            },
            VxlSlopeBlend {
                from_slope: 0,
                to_slope: 4,
                phase_num: 2,
                phase_den: 3,
            },
            VxlSlopeBlend {
                from_slope: 4,
                to_slope: 0,
                phase_num: 1,
                phase_den: 3,
            },
            VxlSlopeBlend {
                from_slope: 99,
                to_slope: 20,
                phase_num: 1,
                phase_den: 0,
            },
        ] {
            for step in 0..32 {
                let expected = voxel_draw_rotation(
                    compute_slope_blend_rotation(blend),
                    voxel_body_facing(step),
                );
                for _ in 0..2 {
                    let actual = voxel_draw_rotation_for_state(0, Some(blend), step);
                    assert_eq!(
                        actual.to_cols_array().map(f32::to_bits),
                        expected.to_cols_array().map(f32::to_bits)
                    );
                }
            }
        }
    }

    fn make_test_vxl() -> VxlFile {
        let identity: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        VxlFile {
            limb_count: 1,
            body_size: 0,
            palette: vec![[0; 3]; 256],
            limbs: vec![VxlLimb {
                native_spans: None,
                name: "body".to_string(),
                scale: 1.0,
                bounds: [-1.0, -1.0, -1.0, 1.0, 1.0, 1.0],
                transform: identity,
                size_x: 2,
                size_y: 2,
                size_z: 2,
                normals_mode: 4,
                voxels: vec![
                    VxlVoxel {
                        x: 1,
                        y: 1,
                        z: 1,
                        color_index: 10,
                        normal_index: 0,
                    },
                    VxlVoxel {
                        x: 0,
                        y: 0,
                        z: 0,
                        color_index: 20,
                        normal_index: 1,
                    },
                ],
            }],
        }
    }

    #[test]
    fn shadow_marks_every_occupied_column_and_nothing_else() {
        // The fixture has voxels in columns (1,1) and (0,0) only; the shadow is
        // those two columns flattened, shifted right, and nothing more.
        let vxl: VxlFile = make_test_vxl();
        let params: VxlRenderParams = VxlRenderParams::default();
        let shadow: VxlSprite = render_vxl_shadow(&vxl, None, &params);
        let lit: usize = shadow.palette_indices.iter().filter(|&&b| b != 0).count();
        assert!(lit > 0, "shadow must have pixels");
        assert!(
            shadow
                .palette_indices
                .iter()
                .all(|&b| b == 0 || b == SHADOW_STENCIL_INDEX),
            "shadow bytes are a stencil, not palette indices"
        );
        assert!(shadow.depth.is_empty());

        // A model with an empty column casts nothing there: on a wider grid
        // (8x8x2, columns (0,0) and (7,7) far apart on screen) removing the
        // (0,0) voxel must shrink the shadow.
        let mut wide: VxlFile = make_test_vxl();
        {
            let limb = &mut wide.limbs[0];
            limb.size_x = 8;
            limb.size_y = 8;
            limb.bounds = [-4.0, -4.0, -1.0, 4.0, 4.0, 1.0];
            limb.voxels[0].x = 7;
            limb.voxels[0].y = 7;
        }
        let shadow_wide: VxlSprite = render_vxl_shadow(&wide, None, &params);
        let lit: usize = shadow_wide
            .palette_indices
            .iter()
            .filter(|&&b| b != 0)
            .count();
        let mut one: VxlFile = make_test_vxl();
        {
            let limb = &mut one.limbs[0];
            limb.size_x = 8;
            limb.size_y = 8;
            limb.bounds = [-4.0, -4.0, -1.0, 4.0, 4.0, 1.0];
            limb.voxels[0].x = 7;
            limb.voxels[0].y = 7;
            limb.voxels.retain(|v| !(v.x == 0 && v.y == 0));
        }
        let shadow_one: VxlSprite = render_vxl_shadow(&one, None, &params);
        let lit_one: usize = shadow_one
            .palette_indices
            .iter()
            .filter(|&&b| b != 0)
            .count();
        assert!(lit_one < lit, "{lit_one} vs {lit}");
    }

    #[test]
    fn all_renderer_slope_entries_match_original_startup_bits() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/projectile_oracle/ordinary_collision_vectors.json",
        ))
        .unwrap();
        for (slope, row) in corpus["slope_matrices"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            let actual = compute_slope_rotation(slope as u8);
            let bits: [u32; 12] = std::array::from_fn(|i| actual.col(i % 4)[i / 4].to_bits());
            let expected: [u32; 12] = std::array::from_fn(|i| row[i].as_u64().unwrap() as u32);
            assert_eq!(bits, expected, "slope {slope}");
        }
    }

    fn assert_mat4_close(actual: Mat4, expected: Mat4, epsilon: f32) {
        let actual_cols = actual.to_cols_array();
        let expected_cols = expected.to_cols_array();
        for (idx, (a, e)) in actual_cols.iter().zip(expected_cols.iter()).enumerate() {
            assert!(
                (*a - *e).abs() <= epsilon,
                "matrix element {} mismatch: got {}, expected {}",
                idx,
                a,
                e
            );
        }
    }

    #[test]
    fn test_vxl_simple_slope_applies_after_body_facing_before_camera() {
        let vxl = make_test_vxl();
        // Facing 32 = step 4, a 45° body rotation. Facing 64 would be step 8, whose
        // body rotation is exactly identity — that makes the slope-order check below
        // vacuously true, since `slope * I` and `I * slope` are the same matrix.
        let params = VxlRenderParams {
            facing: 32,
            slope_type: 4,
            ..Default::default()
        };

        let (limbs, _) = prepare_limb_data(&vxl, None, &params);
        let combined = limbs[0].combined;

        // Native camera/facing values have independent executable goldens;
        // this regression specifically distinguishes slope composition order.
        let camera_view = voxel_camera_view();
        let body_facing = voxel_body_facing(voxel_facing_step(params.facing));
        let section_transform = Mat4::from_translation(Vec3::new(-1.0, -1.0, -1.0));
        let slope_mat = compute_slope_rotation(params.slope_type);
        let expected = camera_view * slope_mat * body_facing * section_transform;

        assert_mat4_close(combined, expected, 1e-6);

        let old_body_local_order = camera_view * body_facing * slope_mat * section_transform;
        let sample = Vec3::new(0.25, 0.75, -0.5);
        let new_point = combined.transform_point3(sample);
        let old_point = old_body_local_order.transform_point3(sample);
        assert!(
            (new_point - old_point).length() > 0.01,
            "sloped facing 32 must not use the old body-local slope order"
        );
    }

    #[test]
    fn test_voxel_facing_quantizes_to_32_steps_with_round_half_up() {
        // The original reduces facing to 5 bits before building the rotation matrix
        // and reuses those same 5 bits as its draw-cache key, so 32 is the complete
        // set of voxel orientations. Boundaries matter: the step changes at facing 4,
        // half of one 11.25° step, and facing 255 wraps back to step 0 (north).
        assert_eq!(voxel_facing_step(0), 0);
        assert_eq!(voxel_facing_step(3), 0);
        assert_eq!(voxel_facing_step(4), 1, "round-half-up boundary");
        assert_eq!(voxel_facing_step(8), 1);
        assert_eq!(voxel_facing_step(64), 8, "east");
        assert_eq!(voxel_facing_step(128), 16, "south");
        assert_eq!(voxel_facing_step(192), 24, "west");
        assert_eq!(voxel_facing_step(252), 0, "wraps to north, not 32");
        assert_eq!(voxel_facing_step(255), 0);

        // Every step must be reachable, and each bucket representative must map
        // back onto its own step or the atlas key would not round-trip.
        for step in 0..32u8 {
            assert_eq!(
                voxel_facing_step(step * 8),
                step,
                "bucket {} representative",
                step
            );
        }

        // The 16-bit form is the one the original actually evaluates; feeding it a
        // facing whose low byte is zero must agree with the 8-bit form.
        for facing in 0..=255u8 {
            assert_eq!(
                voxel_facing_step_u16(u16::from(facing) << 8),
                voxel_facing_step(facing),
                "8-bit and 16-bit quantization disagree at facing {}",
                facing
            );
        }
    }

    #[test]
    fn test_turret_pivot_rides_the_hull_tilt() {
        // The original translates the body matrix along its own X column, so
        // the pivot is a point on the tilted hull. A fixed screen-space nudge
        // cannot reproduce that: on a ramp the pivot has to move with the hull
        // it is bolted to.
        let flat = turret_pivot_screen_offset(50, 0, 0, 1.0);
        let ramp = turret_pivot_screen_offset(50, 0, 4, 1.0);
        assert!(
            (flat.0 - ramp.0).abs() > 0.05 || (flat.1 - ramp.1).abs() > 0.05,
            "turret pivot must shift on a ramp; flat={:?} ramp={:?}",
            flat,
            ramp
        );

        // The offset rides the native body basis.
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/lighting.json",
        ))
        .unwrap();
        let raw = &vectors["cases"][0]["draw_matrix_bits"];
        let units = turret_offset_units(50);
        let expected_x = f32::from_bits(raw[0].as_u64().unwrap() as u32) * units;
        let expected_y = -f32::from_bits(raw[4].as_u64().unwrap() as u32) * units;
        assert_eq!(flat, (expected_x, expected_y));

        // Negative offsets (stock artmd.ini ships -100 and -80) mirror the pivot.
        let back = turret_pivot_screen_offset(-40, 0, 0, 1.0);
        let fwd = turret_pivot_screen_offset(40, 0, 0, 1.0);
        assert!((back.0 + fwd.0).abs() < 1e-4 && (back.1 + fwd.1).abs() < 1e-4);

        assert_eq!(turret_pivot_screen_offset(0, 0, 0, 1.0), (0.0, 0.0));
    }

    fn barrel_vectors() -> &'static serde_json::Value {
        crate::test_fixture::json("tools/voxel_oracle/barrel_pitch.json")
    }

    /// A native row-major 3x4 matrix from its twelve binary32 words, bottom
    /// row `0 0 0 1`.
    fn row_major_matrix(words: impl IntoIterator<Item = u32>) -> Mat4 {
        let words: Vec<u32> = words.into_iter().collect();
        assert_eq!(words.len(), 12, "a 3x4 matrix");
        let mut matrix = Mat4::IDENTITY;
        for row in 0..3 {
            for col in 0..4 {
                matrix.col_mut(col)[row] = f32::from_bits(words[row * 4 + col]);
            }
        }
        matrix
    }

    /// A native row-major 3x4 matrix written as twelve integer words.
    fn native_matrix(bits: &serde_json::Value) -> Mat4 {
        row_major_matrix(
            bits.as_array()
                .unwrap()
                .iter()
                .map(|word| word.as_u64().unwrap() as u32),
        )
    }

    #[test]
    fn turret_offset_units_match_native_block() {
        let vectors = barrel_vectors();
        assert_eq!(
            vectors["offset_scale_bits"].as_u64().unwrap(),
            TURRET_OFFSET_UNITS_PER_LEPTON
        );
        for case in vectors["offset_cases"].as_array().unwrap() {
            let leptons = case["turret_offset"].as_i64().unwrap() as i32;
            assert_eq!(
                turret_offset_units(leptons).to_bits(),
                case["units_bits"].as_u64().unwrap() as u32,
                "TurretOffset {leptons}"
            );
        }
    }

    #[test]
    fn barrel_pitch_matches_native_rotate_y_block() {
        for case in barrel_vectors()["pitch_cases"].as_array().unwrap() {
            let raw = case["raw"].as_u64().unwrap() as u16;
            let pitch = voxel_facing_step_u16(raw) as i8 - 8;
            let input = native_matrix(&case["matrix_bits"]);
            let native = native_matrix(&case["result_bits"]);
            let pitched = voxel_barrel_pitch(input, pitch);
            for row in 0..3 {
                for col in 0..4 {
                    assert_eq!(
                        pitched.col(col)[row].to_bits(),
                        native.col(col)[row].to_bits(),
                        "elevation {raw:#06X} row {row} col {col}"
                    );
                }
            }
            if pitch == 0 {
                // Level rotates by sin 0, cos 1: only zero signs change, so
                // the draw skips it.
                assert_eq!(native, input, "elevation {raw:#06X}");
            }
        }
    }

    #[test]
    fn barrel_arm_matches_native_composition() {
        // Native composes the turret as the hull times RotateZ of the
        // turret-minus-body angle; VERA draws it at its absolute facing. Equal
        // facings match up to zero signs and rounding of the offset; unequal
        // ones carry the table's error in the composed angle (measured 1.5e-3
        // and 0.05 px at worst).
        for case in barrel_vectors()["arm_cases"].as_array().unwrap() {
            let body_step = case["body_step"].as_u64().unwrap() as u8;
            let turret_step = case["turret_step"].as_u64().unwrap() as u8;
            let elevation = case["barrel"].as_u64().unwrap() as u16;
            let leptons = case["turret_offset"].as_i64().unwrap() as i32;
            let (body_facing, turret_facing) = (body_step << 3, turret_step << 3);
            assert_eq!(voxel_facing_step(body_facing), body_step);
            assert_eq!(voxel_facing_step(turret_facing), turret_step);
            let pitch = voxel_facing_step_u16(elevation) as i8 - 8;
            let body = native_matrix(&case["body_draw_bits"]);
            let turret_pivot = turret_pivot_screen_offset(leptons, body_facing, 0, 1.0);
            let barrel_pivot =
                barrel_pivot_screen_offset(leptons, body_facing, turret_facing, 0, None, pitch);
            let barrel_params = VxlRenderParams {
                barrel_pitch: pitch,
                ..Default::default()
            };
            let parts = [
                (
                    "turret_draw_bits",
                    voxel_draw_rotation_for_state(0, None, turret_step),
                    turret_pivot,
                ),
                (
                    "barrel_draw_bits",
                    voxel_params_draw_rotation(&barrel_params, turret_step),
                    (
                        turret_pivot.0 + barrel_pivot.0,
                        turret_pivot.1 + barrel_pivot.1,
                    ),
                ),
            ];
            let composed = usize::from(body_step != turret_step);
            for (part, rotation, (x, y)) in parts {
                let native = native_matrix(&case[part]);
                let case = format!(
                    "{part} body {body_step} turret {turret_step} elevation {elevation:#06X} \
                     offset {leptons}"
                );
                for row in 0..3 {
                    for col in 0..3 {
                        let error = (rotation.col(col)[row] - native.col(col)[row]).abs();
                        assert!(
                            error <= [0.0, 3e-3][composed],
                            "{case}: row {row} col {col}"
                        );
                    }
                }
                let shift = native.col(3) - body.col(3);
                assert!(
                    (x - shift.x).abs().max((y + shift.y).abs()) <= [1e-4, 0.1][composed],
                    "{case}: ({x}, {y}) vs native ({}, {})",
                    shift.x,
                    -shift.y
                );
            }
        }
    }

    /// A native row-major 3x4 matrix written as twelve hex binary32 words.
    fn native_words(words: &serde_json::Value) -> Mat4 {
        row_major_matrix(
            words
                .as_str()
                .expect("matrix words")
                .split(' ')
                .map(|word| u32::from_str_radix(word, 16).expect("hex word")),
        )
    }

    /// Parity with `tools/rocket_oracle/draw_matrix.json`: the original
    /// Rocket Draw_Matrix along the native flight corpus and over every
    /// facing step, and the camera product the aircraft draw takes of it.
    #[test]
    fn rocket_draw_matrix_matches_native() {
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/rocket_oracle/draw_matrix.json",
        ))
        .unwrap();
        // Bit for bit, zero signs included.
        let bits = |matrix: Mat4| matrix.to_cols_array().map(f32::to_bits);
        assert_eq!(
            bits(native_words(&vectors["camera"])),
            bits(voxel_camera_view())
        );
        let (mut samples, mut draws) = (0, 0);
        for row in vectors["rows"].as_array().unwrap() {
            let name = row["name"].as_str().unwrap();
            for sample in row["samples"].as_array().unwrap() {
                let facing = sample["facing"].as_u64().unwrap() as u16;
                let pitch = f32::from_bits(sample["current_pitch"].as_u64().unwrap() as u32);
                // Presentation keys the body by the facing's high byte.
                let facing_byte =
                    crate::render::unit_atlas::canonical_unit_facing((facing >> 8) as u8);
                let step = voxel_facing_step(facing_byte);
                assert_eq!(step, voxel_facing_step_u16(facing), "{name}: {facing:#06X}");
                let case = format!(
                    "{name}: facing {facing:#06X} pitch {:#010X}",
                    pitch.to_bits()
                );
                assert_eq!(
                    bits(voxel_rocket_locomotor_matrix(step, pitch)),
                    bits(native_words(&sample["matrix"])),
                    "{case}: Draw_Matrix"
                );
                samples += 1;
                if !sample["draw"].is_null() {
                    let params = VxlRenderParams {
                        facing: facing_byte,
                        body_tilt: Some(BodyTilt::Rocket(pitch)),
                        ..VxlRenderParams::default()
                    };
                    assert_eq!(
                        bits(voxel_params_draw_rotation(&params, step)),
                        bits(native_words(&sample["draw"])),
                        "{case}: camera product"
                    );
                    draws += 1;
                }
            }
        }
        assert_eq!((samples, draws), (2247, 504));
    }

    #[test]
    fn flat_geometry_uses_native_camera_and_facing_basis() {
        let vxl = make_test_vxl();
        let params = VxlRenderParams {
            facing: 64,
            slope_type: 0,
            ..Default::default()
        };

        let (limbs, _) = prepare_limb_data(&vxl, None, &params);
        let combined = limbs[0].combined;

        // The independent native matrix includes table-trig asymmetry. It
        // cannot be collapsed into a mathematically exact single yaw.
        let vectors: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/voxel_oracle/lighting.json",
        ))
        .unwrap();
        let raw: Vec<f32> = vectors["cases"][16]["draw_matrix_bits"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| f32::from_bits(value.as_u64().unwrap() as u32))
            .collect();
        let native = Mat4::from_cols(
            Vec4::new(raw[0], raw[4], raw[8], 0.0),
            Vec4::new(raw[1], raw[5], raw[9], 0.0),
            Vec4::new(raw[2], raw[6], raw[10], 0.0),
            Vec4::W,
        );
        let flat_order = native * Mat4::from_translation(Vec3::new(-1.0, -1.0, -1.0));

        assert_mat4_close(combined, flat_order, 1e-6);
    }

    #[test]
    fn original_native_slope_interpolation_reaches_renderer_unchanged() {
        let golden: serde_json::Value =
            serde_json::from_str(crate::test_fixture::text("tools/native_slope_blend.json"))
                .unwrap();
        for row in golden["cases"].as_array().unwrap() {
            let input = &row["input"];
            let phase = f64::from_bits(input["phase_bits"].as_u64().unwrap());
            // The bounded native corpus supplies exact quarters and halves.
            let matrix = compute_slope_blend_rotation(VxlSlopeBlend {
                from_slope: input["from_slope"].as_u64().unwrap() as u8,
                to_slope: input["to_slope"].as_u64().unwrap() as u8,
                phase_num: (phase * 4.0) as i32,
                phase_den: 4,
            });
            let actual: [u32; 12] = std::array::from_fn(|i| matrix.col(i % 4)[i / 4].to_bits());
            let expected: [u32; 12] =
                std::array::from_fn(|i| row["matrix_bits"][i].as_u64().unwrap() as u32);
            assert_eq!(actual, expected, "{input}");
        }
    }

    #[test]
    fn drive_ship_slope_negative_one_third_extrapolates_without_lower_clamp() {
        let negative = compute_slope_blend_rotation(VxlSlopeBlend {
            from_slope: 4,
            to_slope: 8,
            phase_num: -1,
            phase_den: SLOPE_TRANSITION_FRAMES,
        });
        let from = compute_slope_rotation(4);
        let sample = Vec3::new(0.25, 0.75, 1.0);
        assert!(
            (negative.transform_point3(sample) - from.transform_point3(sample)).length() > 0.001,
            "native signed -1/3 phase must reach SLERP instead of clamping to the source"
        );
    }

    /// Jumpjet Draw_Matrix against the executable
    /// (`tools/spatial_oracle/jumpjet_crash.json`, `draw_matrix`): the
    /// `TiltCrashJumpjet=` arm at four facings, over and under the 0.005 gate
    /// and out to the balloon clamp, with three half-size pairs; and the base
    /// arm under the gate or without the flag. The offsets are native to the
    /// bit; the rotations within 2e-3 of the table's.
    #[test]
    fn jumpjet_tilt_draw_matrix_matches_native() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/jumpjet_crash.json",
        ))
        .unwrap();
        let rows = corpus["draw_matrix"].as_array().unwrap();
        assert_eq!(rows.len(), 86);
        let mut tilted = 0;
        for row in rows {
            let input = &row["input"];
            let name = row["name"].as_str().unwrap();
            let pair = |key: &str| {
                let values = input[key].as_array().unwrap();
                [0, 1].map(|i| values[i].as_f64().unwrap() as f32)
            };
            let (angles, half_sizes) = (pair("angles"), pair("half_sizes"));
            let native: Vec<f32> = row["output"]["matrix"]
                .as_array()
                .unwrap()
                .iter()
                .map(|hex| f32::from_bits(u32::from_str_radix(hex.as_str().unwrap(), 16).unwrap()))
                .collect();
            let step = voxel_facing_step_u16(input["facing"].as_u64().unwrap() as u16);
            let tilt = input["tilt"] == 1 && voxel_tilt_applies(angles);
            assert_eq!(row["output"]["key"] == u32::MAX, tilt, "{name}: the arm");
            let expected = if tilt {
                tilted += 1;
                voxel_jumpjet_tilt_locomotor_matrix(step, angles, half_sizes)
            } else {
                voxel_body_facing(step)
            };
            for r in 0..3 {
                for c in 0..4 {
                    let actual = expected.col(c)[r];
                    let native = native[r * 4 + c];
                    let tolerance = if c == 3 { 1e-5 } else { 2e-3 };
                    assert!(
                        (actual - native).abs() < tolerance,
                        "{name} m[{r}][{c}]: {actual} vs native {native}"
                    );
                }
            }
        }
        assert_eq!(tilted, 34);
    }

    /// Fly Draw_Matrix's crashing arm against the executable
    /// (`tools/spatial_oracle/aircraft_crash.json`, `draw_matrix`): four
    /// facings by six poses, the grounded and uncrashed bodies keying their
    /// ordinary draw. Native reads table trig; within 2e-3 of glam's.
    #[test]
    fn crash_draw_matrix_matches_native_fly_draw_matrix() {
        let oracle: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/aircraft_crash.json",
        ))
        .unwrap();
        let rows = oracle["draw_matrix"].as_array().unwrap();
        assert_eq!(rows.len(), 26);
        let mut crashing = 0;
        for row in rows {
            let input = &row["input"];
            let name = input["name"].as_str().unwrap();
            let native: Vec<f32> = row["matrix"]
                .as_array()
                .unwrap()
                .iter()
                .map(|hex| f32::from_bits(u32::from_str_radix(hex.as_str().unwrap(), 16).unwrap()))
                .collect();
            let step = voxel_facing_step_u16(input["facing"].as_u64().unwrap() as u16);
            let angles = input["angles"].as_array().unwrap();
            let tilt = [
                angles[0].as_f64().unwrap() as f32,
                angles[1].as_f64().unwrap() as f32,
            ];
            let crash = row["key"].as_i64() == Some(-1);
            let expected = if crash {
                crashing += 1;
                voxel_crash_locomotor_matrix(step, tilt)
            } else {
                // The ordinary arm: the facing alone (no stock pitch/roll).
                voxel_body_facing(step)
            };
            for r in 0..3 {
                for c in 0..3 {
                    let actual = expected.col(c)[r];
                    let native = native[r * 4 + c];
                    assert!(
                        (actual - native).abs() < 2e-3,
                        "{name} m[{r}][{c}]: {actual} vs native {native}"
                    );
                }
            }
        }
        assert_eq!(crashing, 24);
    }
}

#[cfg(test)]
#[path = "ship_pose_tests.rs"]
pub(crate) mod ship_pose_tests;
