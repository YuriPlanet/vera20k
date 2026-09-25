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

use glam::{Mat3, Mat4, Quat, Vec3, Vec4};

use crate::assets::hva_file::HvaFile;
use crate::assets::vpl_file::VplFile;
use crate::assets::vxl_file::{VxlFile, VxlLimb};
use crate::render::vxl_normals;

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

/// Isometric camera pitch (60°, matching the original engine's isometric projection).
#[cfg(test)]
const CAMERA_PITCH_DEG: f32 = 60.0;

/// World yaw offset (45°) to align model north with isometric grid.
#[cfg(test)]
const WORLD_YAW_OFFSET_DEG: f32 = 45.0;

/// Number of distinct orientations a voxel body/turret/barrel can render at.
///
/// The original quantizes facing to 5 bits before building the rotation matrix and
/// reuses those same 5 bits as its draw-cache key, so there is no finer sub-facing
/// anywhere in the voxel pipeline: a rendered voxel is always exactly 1 of 32.
pub const VOXEL_FACING_STEPS: u32 = 32;

/// Angular size of one voxel facing step: 360° / 32 = 11.25° = π/16.
#[cfg(test)]
const VOXEL_FACING_STEP_RAD: f32 = std::f32::consts::PI / 16.0;

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

/// The body-rotation angle the original installs for a given facing step.
///
/// Built as `RotateZ((step - 8) * -π/16)`. The `-8` bias is a constant +90° folded
/// into the body term; the camera carries the matching `-45°` yaw, so the net world
/// rotation works out to `45° − facing°` — but the two terms sit on opposite sides
/// of the terrain-slope matrix and therefore cannot be collapsed into one another.
#[cfg(test)]
fn voxel_facing_angle(step: u8) -> f32 {
    (step as f32 - 8.0) * -VOXEL_FACING_STEP_RAD
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

/// `BuildFacingRotationMatrix @ 0x0055A730`: signed step -8..23, native
/// double angle and float store before table trig. Share the established
/// table with FLH rather than recomputing an approximately equal sine.
fn voxel_body_facing(step: u8) -> Mat4 {
    let (sin, cos) = crate::util::native_trig::native_sin_cos_by_step(i32::from(step) - 8)
        .expect("five-bit voxel facing fits native trig table");
    Mat4::from_cols(
        Vec4::new(cos, sin, 0.0, 0.0),
        Vec4::new(if sin == 0.0 { 0.0 } else { -sin }, cos, 0.0, 0.0),
        Vec4::Z,
        Vec4::W,
    )
}

/// Rotation part of native `MatrixMultiply @ 0x005AF980`: each dot product
/// evaluates (z + y) + x in x87 precision, then stores a float. Inputs here
/// are rotations without translation. HVA scale/translation stays downstream.
fn voxel_rotation_product(left: Mat4, right: Mat4) -> Mat4 {
    use crate::util::native_x87::{NativeF32Bits, X87Chop53 as Fpu};
    let load = |value: f32| {
        Fpu::load_f32(NativeF32Bits::from_bits(value.to_bits())).expect("voxel basis is finite")
    };
    let mut result = Mat4::IDENTITY;
    for col in 0..3 {
        for row in 0..3 {
            let products: [_; 3] =
                std::array::from_fn(|i| Fpu::mul(load(left.col(i)[row]), load(right.col(col)[i])));
            let value = Fpu::add(Fpu::add(products[2], products[1]), products[0]);
            result.col_mut(col)[row] = f32::from_bits(
                Fpu::store_f32(value)
                    .expect("rotation product fits a float")
                    .bits(),
            );
        }
    }
    result
}

fn voxel_draw_rotation(slope: Mat4, facing: Mat4) -> Mat4 {
    // DriveLocomotion 0x004B03DA first multiplies slope * facing;
    // UnitClass 0x0073B71C then multiplies camera * that result.
    voxel_rotation_product(voxel_camera_view(), voxel_rotation_product(slope, facing))
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
/// Native reads the table sine/cosine (`Math__SinFromTable`); glam's are
/// within a pixel of it on these small bodies.
fn voxel_crash_rotation(step: u8, tilt: [f32; 2]) -> Mat4 {
    voxel_rotation_product(
        voxel_camera_view(),
        voxel_crash_locomotor_matrix(step, tilt),
    )
}

/// The locomotor half of [`voxel_crash_rotation`], before the camera.
fn voxel_crash_locomotor_matrix(step: u8, tilt: [f32; 2]) -> Mat4 {
    voxel_rotation_product(
        voxel_rotation_product(voxel_body_facing(step), Mat4::from_rotation_x(tilt[0])),
        Mat4::from_rotation_y(tilt[1]),
    )
}

/// The draw matrix of one body draw: the crashing arm when a tilt is present,
/// else the slope/facing products.
fn voxel_params_draw_rotation(params: &VxlRenderParams, step: u8) -> Mat4 {
    match params.body_tilt {
        Some(tilt) => voxel_crash_rotation(step, tilt),
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

/// Retained VERA offset approximation. Native UnitClass at 0x0073BA4C
/// instead stores float(Type+0x720 * B1D008); integer division here is not
/// established equivalent. Offset scalar and relative turret transform are
/// separate outstanding parity work; this increment fixes the shared basis.
const TURRET_OFFSET_DIVISOR: i32 = 8;

/// Screen displacement of a turret's pivot from the hull centre, in pixels.
///
/// Native translates the body matrix along its own X column before rotating
/// the turret, so the pivot inherits the hull's terrain tilt. This uses the
/// shared body basis with the retained scalar approximation described above.
/// The separately cached turret sprite still uses its own absolute facing;
/// equivalence to native relative turret/body composition remains unchecked.
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
    let offset_units: f32 = (turret_offset_leptons / TURRET_OFFSET_DIVISOR) as f32;
    let chain =
        voxel_draw_rotation_for_state(slope_type, slope_blend, voxel_facing_step(body_facing));
    let disp: Vec3 = chain.transform_vector3(Vec3::new(offset_units, 0.0, 0.0));
    (disp.x * scale, -disp.y * scale)
}

/// Margin in pixels added around the sprite to avoid clipping.
const SPRITE_MARGIN: u32 = 2;

/// Retained 16.16 projection for magnified previews and shadow preparation.
/// Ordinary encoded VXL pixels instead use the native packed 8.8 owner.
const FP_SHIFT: i32 = 16;
const FP_SCALE: f32 = (1 << FP_SHIFT) as f32; // 65536.0

/// Edge ramp tilt angle (slope types 1-4): `atan(2 × LevelHeight / cellDiagonal)`.
///
/// Reduces analytically to `atan(13√2 / 32) ≈ 0.521_476_7 rad ≈ 29.88°`, where
/// `LevelHeight = 104 leptons` (the canonical RA2 vertical step) and
/// `cellDiagonal = 256√2 leptons`. The factor of 2 reflects the rise across
/// the full diagonal of a one-level edge ramp (two adjacent corners raised).
const EDGE_TILT_RAD: f32 = 0.521_476_7;

/// Corner ramp tilt angle (slope types 5-8): `atan(LevelHeight / cellSide)`.
///
/// Reduces analytically to `atan(13 / 32) ≈ 0.385_882_7 rad ≈ 22.10°`, where
/// `LevelHeight = 104 leptons` and `cellSide = 256 leptons`. One corner of
/// the cell is raised half a level; the run is one cell side (not the
/// diagonal) — that's what distinguishes corner from edge ramps.
const CORNER_TILT_RAD: f32 = 0.385_882_7;

/// Render-only blend between two gamemd slope matrix table entries.
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
    /// tilt at NW/NE/SE/SW. Slopes 17-20 are unpopulated in gamemd (BSS-zero
    /// matrix); the consumer clamps them to 0 before this field is set.
    pub slope_type: u8,
    /// Optional 3-frame slope transition. When present, this replaces
    /// `slope_type` with an interpolated slope orientation.
    pub slope_blend: Option<VxlSlopeBlend>,
    /// A crashing Fly body's roll and pitch in radians (`TechnoClass+0x328`,
    /// `+0x32C`). When present the body draws through Fly Draw_Matrix's
    /// crashing arm ([`voxel_crash_rotation`]) instead of the slope matrices.
    pub body_tilt: Option<[f32; 2]>,
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
            scale: 1.0,
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
        let geometry = native::prepare_geometry(vxl, hva, params.frame, draw_matrix)?;
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

/// Compute the slope rotation matrix for a given terrain slope type (0–16).
///
/// Formula: `slope_matrix = Rz(compass) * Rx(tilt) * Rz(-compass)`
/// where compass is the slope direction angle and tilt is the pitch amount.
///
/// Slopes 9-12 produce the same matrices as 5-8 (corner tilt at NW/NE/SE/SW).
/// Slopes 13-16 reuse the corner directions but with the steeper edge tilt
/// magnitude — a combination not present in slopes 1-8.
///
/// Returns `Mat4::IDENTITY` for slope_type 0 (flat) and as a defensive
/// fallback for any value ≥ 17 that bypasses the consumer-side clamp.
#[allow(clippy::approx_constant)] // Preserve the existing f32 slope matrices; 0.7854 is not FRAC_PI_4.
fn compute_slope_rotation(slope_type: u8) -> Mat4 {
    let (compass_rad, tilt_rad): (f32, f32) = match slope_type {
        0 => return Mat4::IDENTITY,
        // Edge ramps (two adjacent corners raised one height level).
        1 => (4.7124, EDGE_TILT_RAD),               // West,  270°
        2 => (std::f32::consts::PI, EDGE_TILT_RAD), // North, 180°
        3 => (std::f32::consts::FRAC_PI_2, EDGE_TILT_RAD), // East,  90°
        4 => (0.0, EDGE_TILT_RAD),                  // South, 0°
        // Corner ramps (one corner raised one height level).
        5 => (3.9270, CORNER_TILT_RAD), // NW, 225°
        6 => (2.3562, CORNER_TILT_RAD), // NE, 135°
        7 => (0.7854, CORNER_TILT_RAD), // SE, 45°
        8 => (5.4978, CORNER_TILT_RAD), // SW, 315°
        // Diagonal-corner CORNER tilt (byte-identical aliases of 5-8).
        9 => (3.9270, CORNER_TILT_RAD),  // NW, 225°
        10 => (2.3562, CORNER_TILT_RAD), // NE, 135°
        11 => (0.7854, CORNER_TILT_RAD), // SE, 45°
        12 => (5.4978, CORNER_TILT_RAD), // SW, 315°
        // Diagonal-corner EDGE tilt (steeper variant of 9-12).
        13 => (3.9270, EDGE_TILT_RAD), // NW, 225°
        14 => (2.3562, EDGE_TILT_RAD), // NE, 135°
        15 => (0.7854, EDGE_TILT_RAD), // SE, 45°
        16 => (5.4978, EDGE_TILT_RAD), // SW, 315°
        _ => return Mat4::IDENTITY,    // slopes 17-20: defensive identity clamp
    };
    Mat4::from_rotation_z(compass_rad)
        * Mat4::from_rotation_x(tilt_rad)
        * Mat4::from_rotation_z(-compass_rad)
}

fn compute_slope_blend_rotation(blend: VxlSlopeBlend) -> Mat4 {
    let den = blend.phase_den.max(1) as f32;
    let t = blend.phase_num as f32 / den;
    if blend.from_slope == blend.to_slope {
        return compute_slope_rotation(blend.from_slope);
    }
    if t >= 1.0 {
        return compute_slope_rotation(blend.to_slope);
    }

    let from_mat = compute_slope_rotation(blend.from_slope);
    let to_mat = compute_slope_rotation(blend.to_slope);
    let from_quat = Quat::from_mat4(&from_mat).normalize();
    let to_quat = Quat::from_mat4(&to_mat).normalize();
    Mat4::from_quat(from_quat.slerp(to_quat, t).normalize())
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
    let model_rotation = voxel_rotation_product(slope_mat, body_facing);
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
        let vpl_pages: [u8; 256] = vxl_normals::blinn_phong_pages(limb.normals_mode, draw_rotation);

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
    if let Some(sprite) = shadow::render(vxl, hva, params) {
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
    if let Some(draw) = prepare_native_draw(vxl, hva, params, vpl) {
        if let Some(sprite) = draw.render_cpu() {
            return sprite;
        }
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
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/voxel_oracle/lighting.json")).unwrap();
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

    #[test]
    #[ignore = "manual timing: 20,000 pivots, cold blend cache and uncached LUT preparation"]
    fn native_voxel_preparation_timing() {
        use std::hint::black_box;
        use std::time::Instant;
        let same = VxlSlopeBlend {
            from_slope: 0,
            to_slope: 4,
            phase_num: 1,
            phase_den: 3,
        };
        let mixed = |unit: u32| VxlSlopeBlend {
            from_slope: (unit % 17) as u8,
            to_slope: ((unit % 17 + 1) % 17) as u8,
            phase_num: [-1, 1, 2, 3][(unit / 17 % 4) as usize],
            phase_den: 3,
        };
        black_box(turret_pivot_screen_offset(50, 0, 0, 1.0));
        for unit in 0..68 {
            black_box(turret_pivot_screen_offset_for_slope_state(
                50,
                0,
                0,
                Some(mixed(unit)),
                1.0,
            ));
        }
        for label in ["stationary", "same transition", "mixed transitions"] {
            let started = Instant::now();
            for unit in 0..20_000 {
                let blend = match label {
                    "stationary" => None,
                    "same transition" => Some(same),
                    _ => Some(mixed(unit)),
                };
                black_box(turret_pivot_screen_offset_for_slope_state(
                    black_box(50),
                    black_box((unit % 256) as u8),
                    black_box((unit % 17) as u8),
                    black_box(blend),
                    1.0,
                ));
            }
            eprintln!("20,000 warm {label} pivot calls: {:?}", started.elapsed());
        }
        BLENDED_ROTATIONS.with(|cache| cache.borrow_mut().clear());
        let started = Instant::now();
        for unit in 0..128 {
            let blend = VxlSlopeBlend {
                from_slope: (unit / 16) as u8,
                to_slope: (unit % 16) as u8,
                phase_num: 1,
                phase_den: 3,
            };
            black_box(turret_pivot_screen_offset_for_slope_state(
                50,
                (unit % 32) * 8,
                0,
                Some(blend),
                1.0,
            ));
        }
        eprintln!(
            "128 cold blend keys (4096 native bases): {:?}",
            started.elapsed()
        );
        let vxl = make_test_vxl();
        let started = Instant::now();
        for step in 0..64 {
            let params = VxlRenderParams {
                facing: (step % 32) * 8,
                slope_type: step / 32,
                ..Default::default()
            };
            black_box(prepare_limb_data(black_box(&vxl), None, &params));
        }
        eprintln!("64 uncached limb/LUT preparations: {:?}", started.elapsed());
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
    fn test_render_produces_nonempty_sprite() {
        let vxl: VxlFile = make_test_vxl();
        let params: VxlRenderParams = VxlRenderParams::default();
        let sprite: VxlSprite = render_vxl(&vxl, None, &params, None);

        assert!(sprite.width > 0);
        assert!(sprite.height > 0);
        assert_eq!(
            sprite.palette_indices.len(),
            (sprite.width * sprite.height) as usize
        );

        let opaque_count: usize = sprite.palette_indices.iter().filter(|&&b| b != 0).count();
        assert!(
            opaque_count >= 2,
            "Expected at least 2 opaque pixels, got {}",
            opaque_count
        );
    }

    #[test]
    fn test_empty_model_returns_transparent() {
        let vxl: VxlFile = VxlFile {
            limb_count: 0,
            body_size: 0,
            palette: vec![],
            limbs: vec![],
        };
        let params: VxlRenderParams = VxlRenderParams::default();
        let sprite: VxlSprite = render_vxl(&vxl, None, &params, None);
        assert_eq!(sprite.width, 1);
        assert_eq!(sprite.height, 1);
        // Empty model → single transparent byte (palette index 0).
        assert_eq!(sprite.palette_indices[0], 0);
    }

    #[test]
    fn test_facing_changes_output() {
        let vxl: VxlFile = make_test_vxl();

        let sprite_0: VxlSprite = render_vxl(
            &vxl,
            None,
            &VxlRenderParams {
                facing: 0,
                ..Default::default()
            },
            None,
        );
        let sprite_128: VxlSprite = render_vxl(
            &vxl,
            None,
            &VxlRenderParams {
                facing: 128,
                ..Default::default()
            },
            None,
        );

        let same_offset: bool = (sprite_0.offset_x - sprite_128.offset_x).abs() < 0.01
            && (sprite_0.offset_y - sprite_128.offset_y).abs() < 0.01;
        assert!(
            !same_offset || sprite_0.palette_indices != sprite_128.palette_indices,
            "Facing 0 and 128 should produce different output"
        );
    }

    #[test]
    fn test_point_plot_fills_pixels() {
        let vxl: VxlFile = make_test_vxl();
        let params: VxlRenderParams = VxlRenderParams::default();
        let sprite: VxlSprite = render_vxl(&vxl, None, &params, None);

        let opaque: usize = sprite.palette_indices.iter().filter(|&&b| b != 0).count();
        assert!(
            opaque >= 2,
            "Point-plot should produce at least 2 opaque pixels, got {}",
            opaque
        );
    }

    #[test]
    fn test_voxel_grid_packing() {
        // Verify packed voxel round-trips correctly.
        let packed: PackedVoxel = pack_voxel(42, 137);
        assert_eq!(unpack_color(packed), 42);
        assert_eq!(unpack_normal(packed), 137);

        // Color index 0 = empty sentinel.
        let empty: PackedVoxel = pack_voxel(0, 99);
        assert_eq!(empty, 0x0063); // color 0 still packs but...
        // Our grid check uses `packed == 0` which requires both to be 0.
        // Color index 0 means transparent, so we skip it during grid build
        // (the original voxel list already excludes color_index 0 in practice).
        let truly_empty: PackedVoxel = 0;
        assert_eq!(unpack_color(truly_empty), 0);
        assert_eq!(unpack_normal(truly_empty), 0);
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
    fn shadow_is_the_bottom_face_through_the_draw_matrix_shifted_right() {
        // Each occupied column's shadow point is its (x, y, 0) grid point through
        // camera x model_to_world, plus 3 px right: on a slope the footprint
        // follows the ramp because the slope matrix is inside model_to_world.
        let vxl: VxlFile = make_test_vxl();
        let flat_params: VxlRenderParams = VxlRenderParams::default();
        let ramp_params: VxlRenderParams = VxlRenderParams {
            slope_type: 4,
            ..VxlRenderParams::default()
        };
        let (flat_limbs, _) = prepare_limb_data(&vxl, None, &flat_params);
        let (ramp_limbs, _) = prepare_limb_data(&vxl, None, &ramp_params);
        let m_flat: Mat4 = voxel_camera_view() * flat_limbs[0].model_to_world;
        let m_ramp: Mat4 = voxel_camera_view() * ramp_limbs[0].model_to_world;
        let p_flat: Vec3 = m_flat.transform_point3(Vec3::new(1.0, 1.0, 0.0));
        let p_ramp: Vec3 = m_ramp.transform_point3(Vec3::new(1.0, 1.0, 0.0));
        assert!(
            (p_flat.y - p_ramp.y).abs() > 1e-4,
            "the ramp must move the bottom face on screen"
        );
        let shifted: Mat4 =
            Mat4::from_translation(Vec3::new(SHADOW_LIGHT_OFFSET_PX, 0.0, 0.0)) * m_flat;
        let sp: Vec3 = shifted.transform_point3(Vec3::new(1.0, 1.0, 0.0));
        assert!((sp.x - p_flat.x - 3.0).abs() < 1e-5 && (sp.y - p_flat.y).abs() < 1e-5);
    }

    #[test]
    fn test_axis_order_positive_depth() {
        // Positive depth contribution → iterate low to high.
        let iter: AxisIter = axis_order(5, 1.0);
        let vals: Vec<i32> = iter.iter().collect();
        assert_eq!(vals, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn test_axis_order_negative_depth() {
        // Negative depth contribution → iterate high to low.
        let iter: AxisIter = axis_order(5, -1.0);
        let vals: Vec<i32> = iter.iter().collect();
        assert_eq!(vals, vec![4, 3, 2, 1, 0]);
    }

    #[test]
    fn test_edge_tilt_magnitude_matches_gamemd_formula() {
        // Tripwire: catches accidental edits to EDGE_TILT_RAD that don't recompute
        // the derivation chain. atan(2 × 104 / (256 × √2)) is the value the
        // original game's edge-tilt init stores after reducing LevelHeight=104.
        // Compute in f64 then cast to f32 to avoid f32 sqrt() rounding error
        // bleeding into the reference value.
        let expected: f32 = (2.0_f64 * 104.0 / (256.0 * 2.0_f64.sqrt())).atan() as f32;
        assert!(
            (EDGE_TILT_RAD - expected).abs() < 1e-6,
            "EDGE_TILT_RAD={} drifted from formula {}",
            EDGE_TILT_RAD,
            expected
        );
    }

    #[test]
    fn test_corner_tilt_magnitude_matches_gamemd_formula() {
        // Tripwire: catches accidental edits to CORNER_TILT_RAD. atan(104 / 256)
        // is what the corner-tilt init stores after reducing LevelHeight=104.
        let expected: f32 = (104.0_f64 / 256.0).atan() as f32;
        assert!(
            (CORNER_TILT_RAD - expected).abs() < 1e-6,
            "CORNER_TILT_RAD={} drifted from formula {}",
            CORNER_TILT_RAD,
            expected
        );
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
    fn test_camera_yaw_sign_is_load_bearing_on_slopes() {
        // The camera's -45° yaw and the body term's +90° bias cancel on flat ground,
        // which is why a sign flip in either one hides for as long as every unit is
        // on level terrain. They do not cancel once a slope matrix sits between
        // them: Rz does not commute with a tilt about a horizontal axis, so the
        // wrong sign tilts a ramped unit about an axis 90° away from the original's.
        //
        // Pin both halves of that: identical on flat ground, different on a ramp.
        let facing: u8 = 64;
        let step = voxel_facing_step(facing) as f32;
        let facing_rad = facing as f32 / 256.0 * std::f32::consts::TAU;

        let correct_camera = Mat4::from_rotation_x(-CAMERA_PITCH_DEG.to_radians())
            * Mat4::from_rotation_z(-WORLD_YAW_OFFSET_DEG.to_radians());
        let correct_body = Mat4::from_rotation_z((step - 8.0) * -VOXEL_FACING_STEP_RAD);

        // The pre-fix formulation: yaw sign flipped, +90° folded out of the body.
        let flipped_camera = Mat4::from_rotation_x(-CAMERA_PITCH_DEG.to_radians())
            * Mat4::from_rotation_z(WORLD_YAW_OFFSET_DEG.to_radians());
        let flipped_body = Mat4::from_rotation_z(-facing_rad);

        let flat = compute_slope_rotation(0);
        assert_mat4_close(
            correct_camera * flat * correct_body,
            flipped_camera * flat * flipped_body,
            1e-5,
        );

        let ramp = compute_slope_rotation(4);
        let sample = Vec3::new(0.0, 1.0, 0.0);
        let correct_point = (correct_camera * ramp * correct_body).transform_point3(sample);
        let flipped_point = (flipped_camera * ramp * flipped_body).transform_point3(sample);
        assert!(
            (correct_point - flipped_point).length() > 0.1,
            "camera yaw sign must change the rendered tilt on a ramp; got {:?} vs {:?}",
            correct_point,
            flipped_point
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
    fn test_voxel_facing_angle_nets_to_45_minus_facing_on_flat_ground() {
        // Sanity-check the +90° bias hiding in the `step - 8` term: composed with
        // the camera's -45° yaw, the net world rotation on flat ground must come out
        // at `45° - facing°` for the quantized facing. This is the identity that
        // makes the sign pair self-consistent, and the reason the old formulation
        // looked right for as long as nothing drove onto a ramp.
        for step in 0..32u8 {
            let net = -WORLD_YAW_OFFSET_DEG.to_radians() + voxel_facing_angle(step);
            let expected =
                WORLD_YAW_OFFSET_DEG.to_radians() - (step as f32) * VOXEL_FACING_STEP_RAD;
            assert!(
                (net - expected).abs() < 1e-5,
                "step {}: net {} != expected {}",
                step,
                net,
                expected
            );
        }
    }

    #[test]
    fn test_turret_pivot_rides_the_hull_tilt() {
        // The original translates the body matrix along its own X column;
        // this retained scalar approximation still makes the pivot a point on
        // the tilted hull. A fixed screen-space nudge cannot reproduce that: on a ramp
        // the pivot has to move with the hull it is bolted to.
        let flat = turret_pivot_screen_offset(50, 0, 0, 1.0);
        let ramp = turret_pivot_screen_offset(50, 0, 4, 1.0);
        assert!(
            (flat.0 - ramp.0).abs() > 0.05 || (flat.1 - ramp.1).abs() > 0.05,
            "turret pivot must shift on a ramp; flat={:?} ramp={:?}",
            flat,
            ramp
        );

        // Check the native body basis for the retained six-model-unit offset.
        // This is a basis regression, not equivalence of the offset scalar.
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/voxel_oracle/lighting.json")).unwrap();
        let raw = &vectors["cases"][0]["draw_matrix_bits"];
        let expected_x = f32::from_bits(raw[0].as_u64().unwrap() as u32) * 6.0;
        let expected_y = -f32::from_bits(raw[4].as_u64().unwrap() as u32) * 6.0;
        assert_eq!(flat, (expected_x, expected_y));

        // Integer divide by 8, truncating toward zero — not a lepton-per-cell scale.
        // 32..=39 all land on 4 model units, so they must agree exactly, while 40
        // crosses into 5 and must not.
        assert_eq!(
            turret_pivot_screen_offset(32, 0, 0, 1.0),
            turret_pivot_screen_offset(39, 0, 0, 1.0)
        );
        assert_ne!(
            turret_pivot_screen_offset(39, 0, 0, 1.0),
            turret_pivot_screen_offset(40, 0, 0, 1.0)
        );
        // Negative offsets (stock artmd.ini ships -100 and -80) mirror the pivot.
        let back = turret_pivot_screen_offset(-40, 0, 0, 1.0);
        let fwd = turret_pivot_screen_offset(40, 0, 0, 1.0);
        assert!((back.0 + fwd.0).abs() < 1e-4 && (back.1 + fwd.1).abs() < 1e-4);

        assert_eq!(turret_pivot_screen_offset(0, 0, 0, 1.0), (0.0, 0.0));
    }

    #[test]
    fn test_default_scale_is_unity() {
        // The original applies no magnification between the section transform and
        // the pixel write: the camera is a pure rotation, and the only constants on
        // the path are a x256 (8.8 fixed point — the rasterizer indexes its 256x256
        // visibility map with the high byte of each 16-bit coordinate) and a +128
        // that centres the model in that buffer. VXL bounds are already pixels.
        // The former 1.045 here was an eyeballed fudge with no such provenance.
        assert_eq!(VxlRenderParams::default().scale, 1.0);
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
        let vectors: serde_json::Value =
            serde_json::from_str(include_str!("../../tools/voxel_oracle/lighting.json")).unwrap();
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
    fn test_slope_4_geometry_locks_current_direction() {
        // Slope type 4 = "South" per the slope-type table (south corners
        // raised). With compass=0° this reduces to Rx(EDGE_TILT_RAD), so
        // glam's right-handed convention puts +Y at z=+sin(tilt) and -Y at
        // z=-sin(tilt). Lock in those exact values — any silent sign flip
        // in compute_slope_rotation, EDGE_TILT_RAD, or glam's convention
        // will fail this test loudly.
        //
        // VXL_SLOPE_MATRIX_SIGN_GHIDRA_REPORT.md verifies this sign matches
        // gamemd; downhill-looking tilt bugs should be fixed in matrix
        // composition/sampling, not by negating tilt_rad.
        let slope_mat: Mat4 = compute_slope_rotation(4);

        let plus_y: Vec3 = slope_mat.transform_point3(Vec3::Y);
        let minus_y: Vec3 = slope_mat.transform_point3(-Vec3::Y);

        let expected: f32 = EDGE_TILT_RAD.sin();
        assert!(
            (plus_y.z - expected).abs() < 1e-5,
            "Expected (+Y).z = +sin(EDGE_TILT_RAD) = {} for slope_type=4; got {}",
            expected,
            plus_y.z
        );
        assert!(
            (minus_y.z + expected).abs() < 1e-5,
            "Expected (-Y).z = -sin(EDGE_TILT_RAD) = {} for slope_type=4; got {}",
            -expected,
            minus_y.z
        );
    }

    #[test]
    fn test_slopes_9_to_12_alias_corner_ramps_5_to_8() {
        // gamemd's VXL_MasterLighting_Init populates slope-table entries 9-12
        // with the same compass+tilt arguments as 5-8 (CORNER tilt at
        // NW/NE/SE/SW). The matrices are byte-identical at runtime.
        // A regression that swapped CORNER for EDGE on 9-12 would tilt these
        // cells more steeply than gamemd does — a player-visible drift.
        for (extended, base) in [(9, 5), (10, 6), (11, 7), (12, 8)] {
            let ext_mat: Mat4 = compute_slope_rotation(extended);
            let base_mat: Mat4 = compute_slope_rotation(base);
            assert_eq!(
                ext_mat, base_mat,
                "slope_type={} should produce the same matrix as slope_type={}",
                extended, base
            );
        }
    }

    #[test]
    fn test_slopes_13_to_16_use_edge_tilt_at_corner_directions() {
        // Slopes 13-16 reuse the corner compass directions (NW/NE/SE/SW from
        // 5-8) but with the steeper EDGE tilt magnitude — a combination not
        // present in slopes 1-8. The matrix must therefore differ from the
        // CORNER-tilt variant at the same compass.
        for (steep, corner) in [(13, 5), (14, 6), (15, 7), (16, 8)] {
            let steep_mat: Mat4 = compute_slope_rotation(steep);
            let corner_mat: Mat4 = compute_slope_rotation(corner);
            assert_ne!(
                steep_mat, corner_mat,
                "slope_type={} (EDGE tilt) must not equal slope_type={} (CORNER tilt)",
                steep, corner
            );
            // Sanity: also not identity.
            assert_ne!(
                steep_mat,
                Mat4::IDENTITY,
                "slope_type={} should produce a tilt, not identity",
                steep
            );
        }
    }

    #[test]
    fn test_slopes_17_to_20_return_identity() {
        // gamemd has no matrix populated for slopes 17-20 (BSS-zero region
        // at DAT_00b454B8). We deliberately diverge from gamemd's invisible-
        // unit failure mode and clamp these to identity (flat) at the
        // renderer. The consumer clamp in app/presentation/instances/units.rs is the
        // primary boundary; this defensive arm catches any value that
        // bypasses it.
        for slope in 17..=20u8 {
            assert_eq!(
                compute_slope_rotation(slope),
                Mat4::IDENTITY,
                "slope_type={} must clamp to identity",
                slope
            );
        }
    }

    #[test]
    fn test_vxl_slope_blend_phase_zero_matches_previous_slope() {
        let blend = VxlSlopeBlend {
            from_slope: 4,
            to_slope: 8,
            phase_num: 0,
            phase_den: SLOPE_TRANSITION_FRAMES,
        };
        assert_mat4_close(
            compute_slope_blend_rotation(blend),
            compute_slope_rotation(4),
            1e-6,
        );
    }

    #[test]
    fn test_vxl_slope_blend_phase_full_matches_current_slope() {
        let blend = VxlSlopeBlend {
            from_slope: 4,
            to_slope: 8,
            phase_num: i32::from(SLOPE_TRANSITION_FRAMES),
            phase_den: SLOPE_TRANSITION_FRAMES,
        };
        assert_mat4_close(
            compute_slope_blend_rotation(blend),
            compute_slope_rotation(8),
            1e-6,
        );
    }

    #[test]
    fn test_vxl_slope_blend_midphase_differs_from_both_endpoints() {
        let blend = VxlSlopeBlend {
            from_slope: 4,
            to_slope: 8,
            phase_num: 1,
            phase_den: SLOPE_TRANSITION_FRAMES,
        };
        let mid = compute_slope_blend_rotation(blend);
        let from = compute_slope_rotation(4);
        let to = compute_slope_rotation(8);
        let sample = Vec3::new(0.25, 0.75, 1.0);
        assert!((mid.transform_point3(sample) - from.transform_point3(sample)).length() > 0.001);
        assert!((mid.transform_point3(sample) - to.transform_point3(sample)).length() > 0.001);
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

    #[test]
    fn test_vxl_slope_blend_preserves_camera_slope_facing_order() {
        let vxl = make_test_vxl();
        let params = VxlRenderParams {
            facing: 64,
            slope_type: 8,
            slope_blend: Some(VxlSlopeBlend {
                from_slope: 4,
                to_slope: 8,
                phase_num: 1,
                phase_den: SLOPE_TRANSITION_FRAMES,
            }),
            ..Default::default()
        };

        let (limbs, _) = prepare_limb_data(&vxl, None, &params);
        let camera_view = voxel_camera_view();
        let body_facing = voxel_body_facing(voxel_facing_step(params.facing));
        let section_transform = Mat4::from_translation(Vec3::new(-1.0, -1.0, -1.0));
        let expected = camera_view
            * compute_slope_blend_rotation(params.slope_blend.unwrap())
            * body_facing
            * section_transform;

        assert_mat4_close(limbs[0].combined, expected, 1e-6);
    }
    /// Fly Draw_Matrix's crashing arm against the executable
    /// (`tools/spatial_oracle/aircraft_crash.json`, `draw_matrix`): four
    /// facings by six poses, the grounded and uncrashed bodies keying their
    /// ordinary draw. Native reads table trig; within 2e-3 of glam's.
    #[test]
    fn crash_draw_matrix_matches_native_fly_draw_matrix() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/aircraft_crash.json"
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
