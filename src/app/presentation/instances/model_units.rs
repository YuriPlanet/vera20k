//! VERA-only, experimental model art (`Model=`, `docs/picture-model-art.md`):
//! the piece a voxel unit or aircraft whose type declares a model draws in
//! place of its VXL body, turret and barrel.
//!
//! The model is drawn by the GPU into this frame's model page
//! ([`crate::render::model_draw`]) at the matrices the voxel parts would
//! take, composited into one image as `UnitClass` composites hull, turret
//! and barrel (`0x0073B140`), and placed in the rectangle a voxel draw of the
//! pose takes. The piece reads that page through the SHP blitters' RGBA
//! source mode with the unit's Z-read policy (`0x2E00`), so its Z comes from
//! the drawn rectangle, seeded at its bottom row (`ZGradient::Vertical`).
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::sync::Arc;

use super::units::{CompositeDrawBounds, body_sort_depth, unit_body_draw_state};
use crate::app::AppState;
use crate::app::presentation::render::draw_plan_lowering::{ObjectPieceInstance, ObjectTexture};
use crate::map::voxel_pose::VxlSlopeBlend;
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::DrawState;
use crate::render::model_draw::ModelPose;
use crate::render::native_z::{ZGradient, pack_z_gradient};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::unit_atlas::PoseTilt;
use crate::render::vxl_raster::{BodyTilt, GroundPose, VxlRenderParams};
use crate::rules::house_colors::{self, HouseColorIndex};
use crate::rules::voxel_asset_catalog::DeclaredModel;

/// Start the frame's model drawing: hold the model page in the sprite atlas
/// ([`crate::render::model_draw::ModelFrame::begin_frame`]). Nothing happens
/// while no type declares a model, so retail data allocates and walks
/// nothing here.
pub(crate) fn begin_model_frame(state: &mut AppState) {
    if !state.rules().is_some_and(|rules| rules.declares_models()) {
        return;
    }
    let renderer = &mut state.renderer;
    let Some(atlas) = state.match_state.match_presentation.sprite_atlas.as_mut() else {
        return;
    };
    renderer.model_frame.get_mut().begin_frame(
        &renderer.gpu.device,
        &renderer.batch_renderer,
        atlas,
    );
}

/// The pose the unit's draw resolved for its voxel parts.
pub(super) struct ModelUnitPose {
    /// The locomotor arm of a posed body (crash, flight pitch and roll,
    /// rocket pitch, ground rocking), if it is posed.
    pub tilt: Option<PoseTilt>,
    /// The Drive/Ship/Hover per-part pose, if the locomotor draws one.
    pub ground: Option<GroundPose>,
    pub slope_type: u8,
    pub slope_blend: Option<VxlSlopeBlend>,
    pub body_facing: u8,
    pub turret_facing: u8,
    /// The barrel's pitch step, `d32 - 8` of its elevation (`0x0073BB85`).
    pub barrel_pitch: i8,
    /// `TurretOffset=` in leptons.
    pub turret_offset: i32,
    /// The gun whose turret and barrel nodes draw; None for none.
    pub gun: Option<u32>,
}

/// Where and how the unit's body draws.
pub(super) struct ModelUnitDraw<'a> {
    pub entity: &'a crate::app::presentation::frame::ObjectRecord,
    /// The body's draw point on screen, in world pixels.
    pub center: [f32; 2],
    pub dock_depth_y_offset: f32,
    pub z: u8,
    pub tint: [f32; 3],
    pub light: crate::render::palette_light::PaletteLight,
    pub draw_state: DrawState,
    pub house: HouseColorIndex,
}

/// The body, turret and barrel matrices of `pose`, in the voxel camera's
/// space: a posed body's locomotor arm draws every part with the body's
/// matrix, as the posed composite does (`unit_atlas` PoseTilt arms); other
/// bodies take Unit `0x0073BA4C..0x0073BC11`'s per-part matrices, level when
/// the locomotor gives no ground pose.
fn pose_matrices(declared: &DeclaredModel, pose: &ModelUnitPose) -> [glam::Mat4; 3] {
    let half_sizes = declared.half_sizes;
    let ground = |ground: GroundPose| {
        crate::render::vxl_raster::ground_unit_pose_matrices(
            pose.slope_type,
            GroundPose {
                turret_offset: pose.turret_offset,
                ..ground
            },
            half_sizes,
            pose.turret_facing,
            pose.barrel_pitch,
        )
    };
    let body_tilt = match pose.tilt {
        Some(PoseTilt::Ground(ground_pose)) => return ground(ground_pose),
        Some(PoseTilt::Fly(angles)) => BodyTilt::Fly(angles),
        Some(PoseTilt::FlyNormal { pitch, roll }) => BodyTilt::FlyNormal { pitch, roll },
        Some(PoseTilt::Jumpjet(angles)) => BodyTilt::Jumpjet { angles, half_sizes },
        Some(PoseTilt::Rocket(pitch)) => BodyTilt::Rocket(pitch),
        None => {
            return ground(pose.ground.unwrap_or(GroundPose {
                angles: [0.0; 2],
                tunnel_tilt: None,
                body_facing: pose.body_facing,
                slope_blend: pose.slope_blend,
                turret_offset: pose.turret_offset,
            }));
        }
    };
    let matrix = crate::render::vxl_raster::body_draw_matrix(&VxlRenderParams {
        facing: pose.body_facing,
        slope_type: pose.slope_type,
        slope_blend: pose.slope_blend,
        body_tilt: Some(body_tilt),
        ..VxlRenderParams::default()
    });
    [matrix; 3]
}

/// Draw `declared` for the unit in `draw` at `pose`: one piece on the model
/// page, or nothing when the frame has no room for it.
pub(super) fn emit_model_unit(
    state: &AppState,
    declared: &Arc<DeclaredModel>,
    pose: ModelUnitPose,
    draw: ModelUnitDraw<'_>,
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    let ramp = (draw.house != house_colors::NO_REMAP)
        .then(|| {
            state
                .rules()
                .map(|rules| rules.house_color_ramps.ramp(draw.house))
        })
        .flatten()
        .map(|ramp| ramp.map(|color| [color.r, color.g, color.b]));
    let request = ModelPose {
        model: Arc::clone(declared),
        matrices: pose_matrices(declared, &pose),
        gun: pose.gun,
        ramp,
    };
    let Some(image) = state.renderer.model_frame.borrow_mut().request(request) else {
        return;
    };
    let [x, y, width, height] = image.rect;
    let [center_x, center_y] = draw.center;
    let top = center_y + y as f32;
    let bottom = top + height as f32;
    // The drawn rectangle is the composite's, so the sinking waterline is
    // captured from it as from the voxel composite's (`0x0073BEA4`).
    let bounds = CompositeDrawBounds {
        padded: [top, height as f32],
        native: Some([
            x.wrapping_add(center_x as i32),
            y.wrapping_add(center_y as i32),
            width,
            height,
        ]),
    };
    let draw_state = unit_body_draw_state(state, draw.entity, draw.draw_state, false, bounds);
    let instance = SpriteInstance {
        position: [center_x + x as f32, top],
        size: [width as f32, height as f32],
        uv_origin: image.uv_origin,
        uv_size: image.uv_size,
        // The blitters' RGBA source mode (`sprite_source.wgsl`).
        source_palette: 0,
        depth: body_sort_depth(
            state,
            draw.entity,
            bottom + draw.dock_depth_y_offset,
            draw.z,
        ),
        tint: draw.tint,
        palette_light: draw.light,
        alpha: 1.0,
        draw_state,
        z_adjust: super::foot_depth::unit_z_adjust(state, draw.entity, true, None) as f32,
        z_gradient: pack_z_gradient(ZGradient::Vertical, false),
        zshape_origin: [0.0, 0.0],
    };
    pieces.push(ObjectPieceInstance {
        target: ObjectTexture::ShpPage(image.page),
        render_z: RenderZPolicy::ReadOnly,
        instance,
    });
}
