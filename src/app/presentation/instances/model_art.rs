//! VERA-only, experimental model art (`Model=`, `docs/picture-model-art.md`),
//! chain 5: the voxel art that is not a unit body. A building's voxel guns, a
//! `Voxel=yes` projectile and a debris piece whose art section declares
//! `Model=` draw the model in place of their VXL, through the same model
//! page as the voxel units ([`crate::render::model_draw::ModelFrame`]).
//!
//! Each asks the frame for its pose at the matrices its VXL would take and
//! draws the page through the SHP blitters' RGBA source mode
//! (`source_palette` 0), with its VXL piece's blit policy, Z argument and
//! gradient. The mask's red takes the ramp of the scheme the VXL's remap
//! would take, and the light is the VXL's: a building's guns the unit light,
//! a projectile and a debris piece the diffuse light (`0x00753C80`). A
//! building's guns animate at the HVA frames of its `+0x148` counter; a
//! debris piece casts the ground shadow its VXL section casts. Nothing here
//! runs for art that declares no model.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use std::cell::RefCell;
use std::sync::Arc;

use crate::app::presentation::render::draw_plan_lowering::{
    ObjectTexture, PlannedBuildingPieceInstance,
};
use crate::assets::model_file::{Model, ModelPart};
use crate::render::batch::SpriteInstance;
use crate::render::draw_state::DrawState;
use crate::render::model_draw::{
    ModelFrame, ModelImage, ModelLight, ModelNodes, ModelPose, ModelShadow,
};
use crate::render::native_z::{ZGradient, pack_z_gradient};
use crate::render::palette_light::PaletteLight;
use crate::render::tactical_draw_plan::{BlitPolicy, BuildingPieceKind, SpriteEncoding};
use crate::render::terrain_draw::TerrainPiece;
use crate::render::unit_atlas::UnitSpriteEntry;
use crate::rules::house_colors::{HouseColorIndex, NO_REMAP};
use crate::rules::ruleset::RuleSet;
use crate::rules::voxel_asset_catalog::DeclaredModel;

/// The ramp the mask's red pixels take for remap scheme `house`, as a VXL's
/// remap indices take its row; None for no remap.
pub(super) fn model_ramp(rules: &RuleSet, house: HouseColorIndex) -> Option<[[u8; 3]; 16]> {
    (house != NO_REMAP).then(|| {
        rules
            .house_color_ramps
            .ramp(house)
            .map(|color| [color.r, color.g, color.b])
    })
}

/// The frame count of `part`'s clip ([`Model::part_clip`]), the count an
/// HVA frame number is taken modulo; None without a clip.
pub(super) fn part_frame_count(model: &Model, part: ModelPart) -> Option<u32> {
    model
        .part_clip(part)
        .and_then(|clip| model.animations.get(clip))
        .map(|animation| animation.frame_count())
}

/// What a pose draws besides its matrices: its nodes, ramp, HVA frame
/// numbers ([`ModelPose::frames`]) and light.
struct PoseDraw {
    nodes: ModelNodes,
    ramp: Option<[[u8; 3]; 16]>,
    frames: [u32; 2],
    light: ModelLight,
}

/// This frame's image of `model` drawing `draw` at `matrices`, as a pose
/// page entry: its anchor-relative rectangle as the entry's offset and size.
/// None when the frame has no room or the pose draws nothing.
fn model_entry(
    frame: &RefCell<ModelFrame>,
    model: &Arc<DeclaredModel>,
    matrices: [glam::Mat4; 3],
    draw: PoseDraw,
) -> Option<UnitSpriteEntry> {
    let pose = ModelPose {
        model: Arc::clone(model),
        matrices,
        gun: None,
        ramp: draw.ramp,
        frames: draw.frames,
        light: draw.light,
    };
    let image = frame.borrow_mut().request_nodes(pose, draw.nodes)?;
    Some(entry_of(image))
}

fn entry_of(image: ModelImage) -> UnitSpriteEntry {
    let [x, y, width, height] = image.rect;
    UnitSpriteEntry {
        uv_origin: image.uv_origin,
        uv_size: image.uv_size,
        pixel_size: [width as f32, height as f32],
        offset_x: x as f32,
        offset_y: y as f32,
        native_draw_bounds: Some(image.rect),
        page: image.page,
    }
}

/// How a building's voxel guns draw: everything `emit_building_turret_vxl`
/// takes for its pieces.
pub(super) struct BuildingGunDraw {
    /// The primary facing (`+0x3A0`, 16-bit) and the barrel's elevation
    /// (`+0x370`, 16-bit).
    pub turret_facing: u16,
    pub barrel_elevation: u16,
    /// The turret's and the barrel's recoil translations.
    pub recoil: [f32; 2],
    /// `TurretOffset=` of the building's art, in leptons.
    pub turret_offset: i32,
    /// The building's draw point plus `TurretAnimX=`/`TurretAnimY=`.
    pub anchor: [f32; 2],
    pub lift_px: i32,
    pub depth: f32,
    pub tint: [f32; 3],
    pub light: PaletteLight,
    pub colour_word: u16,
    pub draw_state: DrawState,
    /// `TurretAnimZAdjust=`.
    pub z_adjust: i32,
    pub house: HouseColorIndex,
    /// The building's HVA frame counter (`Techno+0x148`), as
    /// `emit_building_turret_vxl` takes it.
    pub frame_counter: i32,
}

/// A building whose voxel guns' art declares `Model=` (Building draw
/// `43DA80`): the model's `body` as the B8 turret, when the building has one
/// (`TUR` in `TurretAnim=`, 45FA90), and its `barrel` as the C0 barrel, in
/// one image at the matrices the VXL parts take, translations included
/// ([`crate::render::vxl_raster::building_gun_draw_matrices`]). The piece
/// takes the VXL guns' policy (`0x2800`, Z read only), Z seed
/// (`TurretAnimZAdjust`, lift cancelled) and gradient. The parts animate at
/// the HVA frames the VXL guns take ([`super::shp::building_gun_frames`],
/// `0x0043E2FA`), each modulo its clip's frame count, so `body` and
/// `barrel` are posed by their clips as a unit's parts are.
pub(super) fn emit_building_gun_model(
    frame: &RefCell<ModelFrame>,
    rules: &RuleSet,
    type_id: &str,
    model: &Arc<DeclaredModel>,
    draw: BuildingGunDraw,
    pieces: &mut Vec<PlannedBuildingPieceInstance>,
) {
    let has_turret = crate::rules::voxel_frame_catalog::building_voxel_names(type_id, Some(rules))
        .is_some_and(|(turret, _)| turret.is_some());
    let facing = crate::render::unit_atlas::canonical_turret_facing(draw.turret_facing);
    let pitch = crate::render::vxl_raster::voxel_facing_step_u16(draw.barrel_elevation) as i8 - 8;
    let [turret, barrel] = crate::render::vxl_raster::building_gun_draw_matrices(
        draw.turret_offset,
        facing,
        pitch,
        draw.recoil,
        has_turret,
    );
    let count = |part| Some(part_frame_count(&model.model, part).unwrap_or(1));
    let frames = super::shp::building_gun_frames(
        draw.frame_counter,
        has_turret.then(|| count(ModelPart::Body)).flatten(),
        count(ModelPart::Barrel(0)),
    );
    let Some(entry) = model_entry(
        frame,
        model,
        [turret, turret, barrel],
        PoseDraw {
            nodes: ModelNodes::BuildingGuns { turret: has_turret },
            ramp: model_ramp(rules, draw.house),
            frames,
            light: ModelLight::Unit,
        },
    ) else {
        return;
    };
    let policy = BlitPolicy::z_read(SpriteEncoding::Voxel);
    pieces.push(PlannedBuildingPieceInstance {
        kind: BuildingPieceKind::PoweredOrActiveOverlay,
        z_bias: 0,
        policy,
        target: ObjectTexture::ShpPage(entry.page),
        instance: SpriteInstance {
            position: [
                draw.anchor[0] + entry.offset_x,
                draw.anchor[1] + entry.offset_y,
            ],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            // The blitters' RGBA source mode (`sprite_source.wgsl`).
            source_palette: 0,
            depth: draw.depth,
            tint: draw.tint,
            palette_light: draw
                .light
                .with_colour_word(if policy.ors_colour_word(true) {
                    draw.colour_word
                } else {
                    0
                }),
            alpha: 1.0,
            draw_state: draw.draw_state,
            z_adjust: super::helpers::lifted_z_adjust(draw.lift_px, draw.z_adjust),
            z_gradient: pack_z_gradient(ZGradient::Vertical, false),
            ..Default::default()
        },
    });
}

/// A `Voxel=yes` projectile whose `Image=` art declares `Model=` (Bullet
/// draw `46B0C0`): the model's `body` at HVA frame zero at the bullet's
/// velocity matrix, lit diffusely as its VXL is, its mask's red in the
/// type's `Color=` scheme (`runtime_scheme`, two shade variants per Colors
/// entry, as the VXL's remap row takes it).
pub(super) fn projectile_model_entry(
    frame: &RefCell<ModelFrame>,
    rules: &RuleSet,
    model: &Arc<DeclaredModel>,
    matrix: glam::Mat4,
    runtime_scheme: i32,
) -> Option<(ObjectTexture, UnitSpriteEntry)> {
    let house = u32::try_from(runtime_scheme)
        .ok()
        .and_then(|scheme| u8::try_from(scheme / 2).ok())
        .map_or(NO_REMAP, HouseColorIndex);
    let entry = model_entry(
        frame,
        model,
        [matrix; 3],
        PoseDraw {
            nodes: ModelNodes::Unit,
            ramp: model_ramp(rules, house),
            frames: [0, 0],
            light: ModelLight::Diffuse,
        },
    )?;
    Some((ObjectTexture::ShpPage(entry.page), entry))
}

/// A debris image on the texture it was placed on.
pub(super) type DebrisImage = (ObjectTexture, UnitSpriteEntry);

/// A debris piece whose type draws a declared model (VoxelAnim DrawIt
/// `749B70`, [`crate::rules::voxel_asset_catalog::DebrisModel`]): the
/// `VoxelIndex`th mesh node of its own model, or the part it shares
/// (`ShareSource=`), at the bounce drawing matrix in its rest pose, lit
/// diffusely, as the VXL draws its section (`749DCE`, `749D97`), its mask's
/// red in the owner's scheme (`remap_row`, 0 for none). And its ground
/// shadow, as the VXL section's bottom-face stencil (`749CBE`): the same
/// nodes flattened onto their bottom face and projected at the same matrix,
/// a mask for the SHP shadow edit. The body is None when the frame has no
/// room for it.
pub(super) fn voxel_anim_model_entry(
    frame: &RefCell<ModelFrame>,
    rules: &RuleSet,
    model: &Arc<DeclaredModel>,
    shared: Option<ModelPart>,
    voxel_index: i32,
    matrix: glam::Mat4,
    remap_row: u32,
) -> Option<(DebrisImage, Option<DebrisImage>)> {
    // An invalid index draws nothing, as the VXL draw omits it; a shared
    // part is the whole VXL the index would pick a section of.
    let node = usize::try_from(voxel_index).ok()?;
    let nodes = shared.map_or(ModelNodes::MeshNode(node), ModelNodes::Part);
    let house = remap_row
        .checked_sub(1)
        .and_then(|row| u8::try_from(row).ok())
        .map_or(NO_REMAP, HouseColorIndex);
    let body = model_entry(
        frame,
        model,
        [matrix; 3],
        PoseDraw {
            nodes,
            ramp: model_ramp(rules, house),
            frames: [0, 0],
            light: ModelLight::Diffuse,
        },
    )?;
    let shadow = frame
        .borrow_mut()
        .request_shadow(ModelShadow {
            model: Arc::clone(model),
            matrix,
            considered_aircraft: false,
            nodes,
        })
        .map(entry_of)
        .map(|entry| {
            (
                ObjectTexture::SpriteAtlasEdit(entry.page, TerrainPiece::Shadow),
                entry,
            )
        });
    Some(((ObjectTexture::ShpPage(body.page), body), shadow))
}
