//! AnimClass DrawIt422CA0: the HasBounce ground shadow, the Tiled, Flat or
//! ordinary body and the optional Shadow= pass.
//!
//! Callers retain admission, palette selection, coordinates and Display order.
//! This owner resolves the source rectangles and their separate draw flags,
//! and the Tiled column's own palette ([`tiled_column`]). Of the two retail
//! Tiled types only IONBEAM is spawned, by IonBlast's frame 0 for TAction94,
//! which no retail map uses; gamemd reads Rules ChronoBeam (CHRONOBM) only in
//! its reader. RING1 takes the ordinary arms: the D3D ring (422CB9..422FC9)
//! needs 8A0DF0, which is 0 in the image and only ever stored 0 (WinMain
//! 6BDD98/6BDE37). Bouncer and IsMeteor types have no body arm of their own:
//! the instance's HasBounce byte only adds the ground shadow ahead of the
//! body.

use super::helpers::{
    apply_shape_z_adjust, compute_sprite_depth_params_lifted, in_view, lifted_z_adjust,
};
use crate::app::presentation::render::draw_plan_lowering::{ObjectPieceInstance, ObjectTexture};
use crate::render::batch::{DepthAxis, SpriteInstance};
use crate::render::draw_state::DrawState;
use crate::render::native_z::{ZGradient, pack_z_gradient};
use crate::render::palette_light::PaletteLight;
use crate::render::sprite_atlas::{ShpSpriteEntry, ShpSpriteKey, SpriteAtlas, tiled_anim_key};
use crate::render::tactical_draw_plan::RenderZPolicy;
use crate::render::terrain_draw::TerrainPiece;
use crate::rules::art_data::AnimTypeRuntimeConfig;

pub(super) struct AnimDrawInput<'a> {
    pub key: &'a ShpSpriteKey,
    /// A zero-size body frame has no atlas entry but can have a visible shadow.
    /// A Tiled body draws its frame from [`tiled_anim_key`] instead.
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
    /// The HasBounce ground shadow's drop below `point`, from
    /// [`anim_bounce_drop_px`]. `None` draws no ground shadow.
    pub bounce_drop_px: Option<i32>,
}

/// DrawIt4233D2 pushes 2601 for the HasBounce ground shadow: Darken, Center,
/// the untested 400 and the read-only Z bit 2000, whatever the body's flags.
/// Select_Blitter490B90 picks DarkenZRead (leaves 492D20 plain, 496820 RLE).
const BOUNCE_SHADOW_FLAGS: u32 = 0x2601;

/// `AdjustForZ(GetHeight)`, which DrawIt adds to the draw point for the
/// HasBounce ground shadow (`Anim+0x194`, tested at 423358), or `None` without
/// a bounce body. The constructor sets HasBounce only for Bouncer= and IsMeteor=
/// types (4222EA); the simulation's bounce body is that byte.
///
/// GetHeight (vtable +1C8, Object5F5F40, called at 42310A) measures the stored
/// Location against Map578080's ground there, and anims never set OnBridge.
/// A free anim stores its absolute coordinate, so this takes the drawn
/// (possibly interpolated) one; an attached anim stores its owner-relative
/// coordinate (SetOwnerObject424B50), which GetHeight reads unchanged.
pub(super) fn anim_bounce_drop_px(
    sim: &crate::sim::world::Simulation,
    anim: &crate::sim::anim_class::AnimObject,
    drawn: [i32; 3],
) -> Option<i32> {
    if anim.bounce().is_none() {
        return None;
    }
    let [x, y, z] = match anim.owner_entity() {
        None => drawn,
        Some(_) => {
            let stored = anim.world_coord();
            [stored.x, stored.y, stored.z]
        }
    };
    // Rendering must not stamp the simulation's shared dummy cell.
    let cells = sim
        .resolved_terrain()
        .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
    let height = crate::sim::movement::air_movement::object_height_in_query(
        crate::sim::components::DriveCoord { x, y, z },
        false,
        cells.as_ref(),
    );
    Some(crate::util::native_x87::adjust_for_z_standard(height))
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

pub(super) fn anim_draw_pieces(
    atlas: &SpriteAtlas,
    input: AnimDrawInput<'_>,
) -> Vec<ObjectPieceInstance> {
    // Tiled423636 precedes the Flat and ordinary arms; a Bouncer or IsMeteor
    // body takes them like any other, after its HasBounce ground shadow.
    let tiled = input.config.is_some_and(|config| config.tiled);
    let entry = if tiled {
        atlas.get(&tiled_anim_key(&input.key.type_id, input.key.frame))
    } else {
        input.entry
    };
    let mut body = input.body;
    if let Some(entry) = entry {
        body.position = [
            input.point[0] + entry.offset_x,
            input.point[1] + entry.offset_y,
        ];
        body.size = entry.pixel_size;
        body.uv_origin = entry.uv_origin;
        body.uv_size = entry.uv_size;
        body.source_palette = entry.source_palette;
    }
    let mut pieces = Vec::with_capacity(3);
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
    // HasBounce423358 precedes the building tint and every body arm.
    if let Some(shadow) =
        bounce_ground_shadow(&input, entry, body).filter(|shadow| visible(&shadow.instance))
    {
        pieces.push(shadow);
    }
    if tiled {
        if let Some(entry) = entry {
            let frame_zero = tiled_anim_key(&input.key.type_id, 0);
            let height = atlas
                .get(&frame_zero)
                .map_or(0, |frame| frame.pixel_size[1] as i32);
            pieces.extend(
                tiled_column(&input, entry, body, height)
                    .into_iter()
                    .filter(|tile| visible(&tile.instance)),
            );
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
    if let Some(entry) = entry.filter(|_| visible(&body)) {
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

/// DrawIt423358..4233E9: the body's frame (ESP+2C) through the body's Convert
/// (ESP+30), at `point + (0, AdjustForZ(GetHeight) + YDrawOffset)`, with no
/// remap, Z term `YDrawOffset - AdjustForZ(GetCoordZ)`, gradient 0, intensity
/// 1000 and no colour word. The Darken leaf reads only the frame's nonzero
/// indices: it stores `(destination >> 1) & 0x7BEF` where the strict signed Z
/// test admits and never writes Z (executed for these 2601 leaves:
/// tools/projectile_oracle/bridge_render.md). An empty body frame has no
/// entry, so it draws no shadow either.
fn bounce_ground_shadow(
    input: &AnimDrawInput<'_>,
    entry: Option<&ShpSpriteEntry>,
    body: SpriteInstance,
) -> Option<ObjectPieceInstance> {
    let (drop, entry) = (input.bounce_drop_px?, entry?);
    let y_draw_offset = input.config.map_or(0, |config| config.y_draw_offset);
    let row = input.point[1] + drop as f32;
    let mut shadow = body;
    // The atlas offset carries YDrawOffset, as the body's does.
    shadow.position = [input.point[0] + entry.offset_x, row + entry.offset_y];
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
            row,
            input.lift_px,
        ),
        0,
        input.axis.world_height,
    );
    // The atlas offset's YDrawOffset rides the Z term, as natively.
    shadow.z_adjust = lifted_z_adjust(input.lift_px, y_draw_offset);
    shadow.z_gradient = pack_z_gradient(ZGradient::Flat, false);
    shadow.zshape_origin = [0.0; 2];
    Some(ObjectPieceInstance {
        target: ObjectTexture::SpriteAtlasEdit(entry.page as usize, TerrainPiece::Shadow),
        render_z: depth_policy(BOUNCE_SHADOW_FLAGS),
        instance: shadow,
    })
}

/// DrawIt's Tiled arm (`0x00423644..0x0042371B`): the body frame again and
/// again up the screen. `height` is frame 0's stored height h
/// (`SHPStruct::GetFrameBounds @ 0x0069E7E0`). The first tile's row is the
/// draw point less h/2, and its Z term `YDrawOffset + ZAdjust -
/// AdjustForZ - 50`, 48 below the ordinary body's; each next tile rises h
/// rows and its Z term falls h/2 + h. The loop stops after the tile whose
/// row, before YDrawOffset, is above the view. Every tile draws through the
/// literal ANIM.PAL Convert (`[0x0087F6C0]`) with no remap, the selected
/// flags plus Z read (`0x004236A4`), gradient 2 and the shared intensity and
/// colour word: neither the cell, ColorScheme nor AltPalette Convert DrawIt
/// selected reaches it. No Shadow= pass follows.
///
/// RESIDUAL: an empty frame 0 (h = 0) never moves the row, so gamemd loops
/// forever unless the first row is already above the view; VERA draws the
/// first tile alone.
fn tiled_column(
    input: &AnimDrawInput<'_>,
    entry: &ShpSpriteEntry,
    body: SpriteInstance,
    height: i32,
) -> Vec<ObjectPieceInstance> {
    let half = height / 2;
    let mut tile = body;
    tile.palette_light = PaletteLight::plain(53, body.palette_light.brightness())
        .with_colour_word(body.palette_light.colour_word());
    tile.tint = [1.0; 3];
    tile.alpha = 1.0;
    tile.draw_state = DrawState::from_native_selector(0, (input.flags & 6) as u8);
    tile.z_gradient = pack_z_gradient(ZGradient::Vertical, false);
    let mut tiles = Vec::new();
    for k in 0.. {
        // The row and Z term relative to the ordinary body's.
        let rise = half + k * height;
        let z = -48 - k * (half + height);
        let row = input.point[1] - rise as f32;
        tile.position = [input.point[0] + entry.offset_x, row + entry.offset_y];
        tile.z_adjust = body.z_adjust + z as f32;
        tile.depth = apply_shape_z_adjust(body.depth, z + rise, input.axis.world_height);
        tiles.push(ObjectPieceInstance {
            target: ObjectTexture::ShpPage(entry.page as usize),
            render_z: input.legacy_policy,
            instance: tile,
        });
        // `y < 0` at 0x004236FC, after the draw; NaN stops it too.
        if height <= 0 || !(row >= input.viewport[1]) {
            break;
        }
    }
    tiles
}

#[cfg(test)]
#[path = "anim_draw_tests.rs"]
mod tests;
