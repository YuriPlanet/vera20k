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
//! Over a bridge the rectangle is split as `0x0073B2FE` splits the unit
//! composite: two blits, the upper rows flat and 5 lower, the bottom 16 rows
//! vertical, each seeded on its own ([`crate::render::native_z::voxel_depth_region`]).
//!
//! The parts are posed by the model's clips at the HVA frames the voxel
//! parts would take ([`model_frames`]). The shadow is a mask the GPU draws
//! on the same page ([`crate::render::model_draw::ModelFrame::request_shadow`]),
//! drawn before the body through the SHP shadow edit
//! (`ObjectTexture::SpriteAtlasEdit(page, TerrainPiece::Shadow)`), so its
//! darkening counts follow the native rule and the body covers it as the
//! native body mask does. A `NoSpawnAlt=` unit with its spawns away draws
//! `NAMEWO.glb` ([`declared_model_of`]).
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::sync::Arc;

use super::helpers::compute_sprite_depth;
use super::units::{
    CompositeDrawBounds, animation_frames_with, body_sort_depth, unit_body_draw_state,
};
use crate::app::AppState;
use crate::app::presentation::render::draw_plan_lowering::{ObjectPieceInstance, ObjectTexture};
use crate::assets::model_file::{Model, ModelPart};
use crate::map::voxel_pose::VxlSlopeBlend;
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::DrawState;
use crate::render::model_draw::{ModelLight, ModelNodes, ModelPose, ModelShadow};
use crate::render::native_z::{
    VoxelDepthRegion, ZGradient, pack_voxel_z_gradient, pack_z_gradient, voxel_depth_region,
};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw::TerrainPiece;
use crate::render::unit_atlas::PoseTilt;
use crate::render::vxl_raster::{BodyTilt, GroundPose, VxlRenderParams};
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::voxel_asset_catalog::DeclaredModel;
use crate::rules::voxel_frame_catalog::NO_SPAWN_ALT_SUFFIX;

/// The declared model `type_id`, the drawn type of a unit's voxel draw
/// (`instances::units::drawn_model_id`), draws, if any. A `NoSpawnAlt=`
/// unit with its spawns away draws `<type>WO`, as `%sWO` names its VXL: when
/// the type declares `Model=`, that is the model's `NAMEWO.glb`, else the
/// main model ([`DeclaredModel::no_spawn_alt`]).
pub(super) fn declared_model_of<'r>(
    rules: &'r crate::rules::ruleset::RuleSet,
    type_id: &str,
) -> Option<&'r Arc<DeclaredModel>> {
    if let Some(loaded) = rules.loaded_voxel_model(type_id) {
        return loaded.declared_model();
    }
    let split = type_id.len().checked_sub(NO_SPAWN_ALT_SUFFIX.len())?;
    let (base, suffix) = (type_id.get(..split)?, type_id.get(split..)?);
    if !suffix.eq_ignore_ascii_case(NO_SPAWN_ALT_SUFFIX)
        || !rules
            .object(base)
            .is_some_and(|object| object.no_spawn_alt())
    {
        return None;
    }
    let declared = rules.loaded_voxel_model(base)?.declared_model()?;
    Some(declared.no_spawn_alt.as_ref().unwrap_or(declared))
}

/// The body's and turret's HVA frame numbers for a declared model, as the
/// voxel parts take them ([`animation_frames_with`], `Foot+0x538` modulo the
/// body's count, else `Techno+0x148` modulo the turret's): each count is the
/// frame count of the part's clip ([`Model::part_clip`]), 1 without one.
fn model_frames(
    entity: &crate::app::presentation::frame::ObjectRecord,
    model: &Model,
    gun: Option<u32>,
) -> [u32; 2] {
    let frames = |part| super::model_art::part_frame_count(model, part);
    let (body, turret) =
        animation_frames_with(entity, frames(ModelPart::Body).unwrap_or(1), || {
            frames(ModelPart::Turret(gun.unwrap_or(0)))
        });
    [body, turret]
}

/// The blits of a model body whose drawn rectangle is `[top, height]` in
/// world rows: one, or over a bridge (`split`) the two `0x0073B2FE` takes
/// when the rectangle is taller than 16 rows, each a region of
/// [`voxel_depth_region`] with its own rows, gradient and Z adjustment.
fn body_blits(top: i32, height: i32, z_adjust: i32, split: bool) -> [Option<VoxelDepthRegion>; 2] {
    let packed = pack_voxel_z_gradient(ZGradient::Vertical, split);
    let bottom = top.saturating_add(height);
    let region = |row| voxel_depth_region([top, height], packed, z_adjust, row, [top, bottom]);
    let first = region(top);
    let last = region(bottom.saturating_sub(1)).filter(|last| Some(*last) != first);
    [first, last]
}

/// Where the shadow of a declared model draws
/// (`instances::units::model_shadow_draw`).
pub(super) struct ModelShadowDraw {
    /// The shadow's anchor on screen, in world pixels.
    pub anchor: [f32; 2],
    /// The shadow key's facing (a step times 8) and slope.
    pub facing: u8,
    pub slope: u8,
    pub considered_aircraft: bool,
    pub depth: ShadowDepth,
    pub z_adjust: f32,
}

/// The sort depth a model shadow takes, as each voxel shadow path takes it.
pub(super) enum ShadowDepth {
    /// The body's (a ground Unit, `emit_unit_shadow_sprite`).
    Body,
    /// Its own bottom row's at the unit's height (a lifted Unit,
    /// `emit_lifted_unit_shadow`).
    Own { lift_correction: f32 },
    /// Its own bottom row's on the ground (an aircraft,
    /// `emit_aircraft_shadow_sprite`).
    Ground,
}

/// Start the frame's model drawing: hold the model page in the sprite atlas,
/// drawn at `camera_zoom`, the world camera's zoom (wheel zoom times the
/// Options scale), so a model is as sharp as the camera draws it
/// ([`crate::render::model_draw::ModelFrame::begin_frame`]). Nothing happens
/// while no type declares a model, so retail data allocates and walks
/// nothing here.
pub(crate) fn begin_model_frame(state: &mut AppState, camera_zoom: f32) {
    if !crate::app::state::active_rules(&state.frontend, &state.match_state)
        .is_some_and(|rules| rules.declares_models())
    {
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
        camera_zoom,
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
    pub depth_xyz: [i32; 3],
    pub lift_correction: f32,
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

/// Draw `declared` for the unit in `draw` at `pose`: its shadow, when
/// `shadow` admits one, then its body, from the model page; nothing when the
/// frame has no room for the body.
/// The match state a unit's model draw reads.
#[derive(Clone, Copy)]
pub(super) struct ModelUnitContext<'a> {
    pub platform: &'a crate::app::state::PlatformState,
    pub renderer: &'a crate::app::renderer_state::RendererState,
    pub frontend: &'a crate::app::frontend::state::FrontendState,
    pub match_state: &'a crate::app::match_runtime::state::MatchState,
    pub persistence: &'a crate::app::persistence::PersistenceState,
}

pub(super) fn emit_model_unit(
    state: ModelUnitContext<'_>,
    declared: &Arc<DeclaredModel>,
    pose: ModelUnitPose,
    draw: ModelUnitDraw<'_>,
    shadow: Option<ModelShadowDraw>,
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    let ramp = crate::app::state::active_rules(state.frontend, state.match_state)
        .and_then(|rules| super::model_art::model_ramp(rules, draw.house));
    let request = ModelPose {
        model: Arc::clone(declared),
        matrices: pose_matrices(declared, &pose),
        gun: pose.gun,
        ramp,
        frames: model_frames(draw.entity, &declared.model, pose.gun),
        light: ModelLight::Unit,
    };
    let Some(image) = state.renderer.model_frame.borrow_mut().request(request) else {
        return;
    };
    let [x, y, width, height] = image.rect;
    let [center_x, center_y] = draw.center;
    let top = center_y + y as f32;
    let bottom = top + height as f32;
    let depth = body_sort_depth(
        &state.match_state.match_presentation,
        draw.entity,
        bottom + draw.dock_depth_y_offset,
        draw.z,
        draw.lift_correction,
    );
    if let Some(shadow) = shadow {
        emit_model_shadow(state, declared, draw.entity, shadow, depth, pieces);
    }
    // The drawn rectangle is the composite's, so the sinking waterline is
    // captured from it as from the voxel composite's (`0x0073BEA4`).
    let native = [
        x.wrapping_add(center_x as i32),
        y.wrapping_add(center_y as i32),
        width,
        height,
    ];
    let bounds = CompositeDrawBounds {
        padded: [top, height as f32],
        native: Some(native),
    };
    // The bridge split is two SHP blits here, so the voxel shader's split
    // flag and clip height are not set.
    let draw_state = unit_body_draw_state(
        state.platform,
        state.renderer,
        &state.frontend.screen,
        state.match_state,
        state.persistence,
        draw.entity,
        draw.draw_state,
        bounds,
    );
    let split = super::foot_depth::unit_bridge_split(
        &state.match_state.sim_runtime,
        draw.entity,
        true,
        draw.depth_xyz,
    );
    let z_adjust = super::foot_depth::unit_z_adjust(
        &state.match_state.sim_runtime,
        draw.entity,
        true,
        Some(draw.depth_xyz),
    );
    for region in body_blits(native[1], height, z_adjust, split)
        .into_iter()
        .flatten()
    {
        let row = region.top.wrapping_sub(native[1]);
        let rows = region.height as f32 / height as f32;
        let instance = SpriteInstance {
            position: [center_x + x as f32, top + row as f32],
            size: [width as f32, region.height as f32],
            uv_origin: [
                image.uv_origin[0],
                image.uv_origin[1] + image.uv_size[1] * row as f32 / height as f32,
            ],
            uv_size: [image.uv_size[0], image.uv_size[1] * rows],
            // The blitters' RGBA source mode (`sprite_source.wgsl`).
            source_palette: 0,
            depth,
            tint: draw.tint,
            palette_light: draw.light,
            alpha: 1.0,
            draw_state,
            z_adjust: region.z_adjust as f32,
            z_gradient: pack_z_gradient(region.gradient, false),
            zshape_origin: [0.0, 0.0],
        };
        pieces.push(ObjectPieceInstance {
            target: ObjectTexture::ShpPage(image.page),
            render_z: RenderZPolicy::ReadOnly,
            instance,
        });
    }
}

/// The shadow piece of `declared` at `shadow`: the mask the model page holds
/// for the shadow key's facing and slope, through the SHP shadow edit, Z
/// read as the voxel stencil's (`0x00707280`'s blit, gradient Vertical).
fn emit_model_shadow(
    state: ModelUnitContext<'_>,
    declared: &Arc<DeclaredModel>,
    entity: &crate::app::presentation::frame::ObjectRecord,
    shadow: ModelShadowDraw,
    body_depth: f32,
    pieces: &mut Vec<ObjectPieceInstance>,
) {
    let request = ModelShadow {
        model: Arc::clone(declared),
        matrix: crate::render::vxl_raster::body_draw_matrix(&VxlRenderParams {
            facing: shadow.facing,
            slope_type: shadow.slope,
            ..VxlRenderParams::default()
        }),
        considered_aircraft: shadow.considered_aircraft,
        nodes: ModelNodes::Unit,
    };
    let Some(image) = state
        .renderer
        .model_frame
        .borrow_mut()
        .request_shadow(request)
    else {
        return;
    };
    let [x, y, width, height] = image.rect;
    let [anchor_x, anchor_y] = shadow.anchor;
    let bottom = anchor_y + y as f32 + height as f32;
    let depth = match shadow.depth {
        ShadowDepth::Body => body_depth,
        ShadowDepth::Own { lift_correction } => body_sort_depth(
            &state.match_state.match_presentation,
            entity,
            bottom,
            entity.position.z(),
            lift_correction,
        ),
        ShadowDepth::Ground => {
            compute_sprite_depth(&state.match_state.match_presentation, bottom, 0)
        }
    };
    pieces.push(ObjectPieceInstance {
        target: ObjectTexture::SpriteAtlasEdit(image.page, TerrainPiece::Shadow),
        render_z: RenderZPolicy::ReadOnly,
        instance: SpriteInstance {
            position: [anchor_x + x as f32, anchor_y + y as f32],
            size: [width as f32, height as f32],
            uv_origin: image.uv_origin,
            uv_size: image.uv_size,
            source_palette: 0,
            depth,
            tint: crate::map::lighting::DEFAULT_TINT,
            alpha: 1.0,
            // As an SHP shadow, the body's translucency is not the shadow's.
            draw_state: DrawState::default(),
            z_adjust: shadow.z_adjust,
            z_gradient: pack_z_gradient(ZGradient::Vertical, false),
            ..Default::default()
        },
    });
}

#[cfg(all(test, tests_app_presentation_instances))]
mod tests {
    use super::*;

    /// Over a bridge a body taller than 16 rows is two blits, as `0x0073B2FE`
    /// draws it: the upper rows flat and 5 lower, the bottom 16 vertical;
    /// a shorter body, or one off a bridge, is one vertical blit.
    #[test]
    fn a_bridge_splits_the_body_into_two_blits() {
        let [upper, lower] = body_blits(100, 40, 7, true);
        assert_eq!(
            upper,
            Some(VoxelDepthRegion {
                top: 100,
                height: 24,
                gradient: ZGradient::Flat,
                z_adjust: 2,
            })
        );
        assert_eq!(
            lower,
            Some(VoxelDepthRegion {
                top: 124,
                height: 16,
                gradient: ZGradient::Vertical,
                z_adjust: 7,
            })
        );
        let whole = Some(VoxelDepthRegion {
            top: 100,
            height: 40,
            gradient: ZGradient::Vertical,
            z_adjust: 7,
        });
        assert_eq!(body_blits(100, 40, 7, false), [whole, None]);
        assert_eq!(
            body_blits(100, 12, 7, true),
            [
                Some(VoxelDepthRegion {
                    top: 100,
                    height: 12,
                    gradient: ZGradient::Vertical,
                    z_adjust: 7,
                }),
                None
            ]
        );
    }
}
