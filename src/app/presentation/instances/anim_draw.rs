//! Ordinary AnimClass DrawIt422CA0 body and optional shadow passes.
//!
//! Callers retain admission, palette selection, coordinates and Display order.
//! This owner resolves the two source rectangles and their separate draw flags.
//! Tiled, bouncing/meteor and the special RING1 path retain their previous body
//! presentation; their separate native draw arms are outside this port.

use super::helpers::{
    apply_shape_z_adjust, compute_sprite_depth_params_lifted, in_view, lifted_z_adjust,
};
use crate::app::presentation::render::draw_plan_lowering::{ObjectPieceInstance, ObjectTexture};
use crate::render::batch::{DepthAxis, SpriteInstance};
use crate::render::draw_state::DrawState;
use crate::render::native_z::{ZGradient, pack_z_gradient};
use crate::render::sprite_atlas::{ShpSpriteEntry, ShpSpriteKey, SpriteAtlas};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw::TerrainPiece;
use crate::rules::art_data::AnimTypeRuntimeConfig;

pub(super) struct AnimDrawInput<'a> {
    pub key: &'a ShpSpriteKey,
    /// A zero-size body frame has no atlas entry but can have a visible shadow.
    pub entry: Option<&'a ShpSpriteEntry>,
    /// Resolved Anim draw point, before either frame's crop or YDrawOffset.
    pub point: [f32; 2],
    /// Prepared body material and depth from the owning caller. Its
    /// ordinary Z term already includes YDrawOffset + instance ZAdjust - 2.
    pub body: SpriteInstance,
    pub config: Option<&'a AnimTypeRuntimeConfig>,
    /// The result of the shared DrawIt translucency selection, before its
    /// body OR0x2000 and shadow AND~6/OR0x601.
    pub flags: u32,
    pub lift_px: i32,
    pub axis: DepthAxis,
    pub viewport: [f32; 4],
    pub legacy_policy: RenderZPolicy,
}

/// Convert490B90/490E50: a write bit takes precedence over either read bit.
/// In particular a general Anim's shadow inherits600 and never tests Z,
/// whereas BuildingCreateAnimForSlot451890 supplies1600 and does test it.
fn depth_policy(flags: u32) -> RenderZPolicy {
    if flags & 0x4000 != 0 {
        RenderZPolicy::ReadWrite
    } else if flags & 0x3000 != 0 {
        RenderZPolicy::ReadOnly
    } else {
        RenderZPolicy::None
    }
}

pub(super) fn ordinary_anim_type(name: &str, config: Option<&AnimTypeRuntimeConfig>) -> bool {
    !name.eq_ignore_ascii_case("RING1")
        && config.is_none_or(|config| !config.tiled && !config.bouncer && !config.is_meteor)
}

pub(super) fn anim_draw_pieces(
    atlas: &SpriteAtlas,
    input: AnimDrawInput<'_>,
) -> Vec<ObjectPieceInstance> {
    let mut body = input.body;
    if let Some(entry) = input.entry {
        body.position = [
            input.point[0] + entry.offset_x,
            input.point[1] + entry.offset_y,
        ];
        body.size = entry.pixel_size;
        body.uv_origin = entry.uv_origin;
        body.uv_size = entry.uv_size;
        body.source_palette = entry.source_palette;
    }
    let ordinary = ordinary_anim_type(&input.key.type_id, input.config);
    let mut pieces = Vec::with_capacity(2);
    let visible = |instance: &SpriteInstance| {
        in_view(
            instance.position[0],
            instance.position[1],
            instance.size[0],
            instance.size[1],
            input.viewport[0],
            input.viewport[1],
            input.viewport[2],
            input.viewport[3],
            200.0,
        )
    };
    if !ordinary {
        if let Some(entry) = input.entry.filter(|_| visible(&body)) {
            pieces.push(ObjectPieceInstance {
                target: ObjectTexture::ShpPage(entry.page as usize),
                render_z: input.legacy_policy,
                instance: body,
            });
        }
        return pieces;
    }

    // Keep the existing body's alpha/palette path. Its selected flags are
    // still needed below: the later shadow clears translucency, but retains
    // the producer's depth bits rather than the body's added read flag.
    let flat = input.config.is_some_and(|config| config.flat);
    body.z_gradient = pack_z_gradient(
        if flat {
            ZGradient::Flat
        } else {
            ZGradient::Vertical
        },
        false,
    );
    if flat {
        // Flat423798 subtracts3; the ordinary body subtracts2 at423803.
        body.z_adjust -= 1.0;
        body.depth = apply_shape_z_adjust(body.depth, -1, input.axis.world_height);
    }
    if let Some(entry) = input.entry.filter(|_| visible(&body)) {
        pieces.push(ObjectPieceInstance {
            target: ObjectTexture::ShpPage(entry.page as usize),
            render_z: input.legacy_policy,
            instance: body,
        });
    }

    // Flat returns after its one draw. Ordinary42382C then tests Shadow372
    // even when the preceding body call had empty bounds and stored nothing.
    // It adds signed SHP header+6 / 2 to the already selected body frame.
    let Some(config) = input.config.filter(|config| !flat && config.shadow) else {
        return pieces;
    };
    let Some(raw_count) = config.raw_shp_frame_count else {
        return pieces;
    };
    let Ok(frame) = u16::try_from(i32::from(input.key.frame).wrapping_add(raw_count / 2)) else {
        return pieces;
    };
    let mut key = input.key.clone();
    key.frame = frame;
    let Some(entry) = atlas.get(&key) else {
        return pieces;
    };
    let flags = (input.flags & !6) | 0x601;
    let mut shadow = input.body;
    shadow.position = [
        input.point[0] + entry.offset_x,
        // The atlas includes YDrawOffset for every frame;423886 passes the
        // original point rather than the body's YDrawOffset-adjusted point.
        input.point[1] + entry.offset_y - config.y_draw_offset as f32,
    ];
    shadow.size = entry.pixel_size;
    shadow.uv_origin = entry.uv_origin;
    shadow.uv_size = entry.uv_size;
    shadow.source_palette = entry.source_palette;
    shadow.draw_state = DrawState::from_native_selector(0, 1);
    shadow.palette_light = shadow
        .palette_light
        .with_brightness(1000)
        .with_colour_word(0);
    shadow.alpha = 1.0;
    shadow.depth = apply_shape_z_adjust(
        compute_sprite_depth_params_lifted(
            input.axis.origin_y,
            input.axis.world_height,
            input.point[1],
            input.lift_px,
        ),
        -2,
        input.axis.world_height,
    );
    shadow.z_adjust = lifted_z_adjust(input.lift_px, -2);
    shadow.z_gradient = pack_z_gradient(ZGradient::Flat, false);
    shadow.zshape_origin = [0.0; 2];
    if visible(&shadow) {
        pieces.push(ObjectPieceInstance {
            target: ObjectTexture::SpriteAtlasEdit(entry.page as usize, TerrainPiece::Shadow),
            render_z: depth_policy(flags),
            instance: shadow,
        });
    }
    pieces
}

#[cfg(test)]
#[path = "anim_draw_tests.rs"]
mod tests;
