//! Retained `VoxelAnimClass` rendering: `VoxelAnimClass::DrawIfVisible @
//! 0x00749B20` -> `ObjectClass::DrawIfVisible @ 0x005F4B10` ->
//! `VoxelAnimClass::DrawIt @ 0x00749B70`.
//!
//! The simulation owns each piece's coordinate, tumble, lifetime and Air-layer
//! Display order. This read-only adapter resolves the floor under a piece,
//! rasterizes its type's model at the bounce drawing matrix on the pose page,
//! and emits the ground shadow and then the body as one Air-layer object.
//! DrawIt's fog test (`0x00749B70..0x00749BB6`) needs no port: the function it
//! calls, `0x005865E0`, is `xor al, al; ret 4`, so it never skips a piece.
//! The draw takes no RNG draws, writes no timers and detaches nothing.
//!
//! RESIDUAL: DrawIt arms VERA does not draw.
//! - ObjectClass::DrawIfVisible's redraw bookkeeping (`+0x80` and the
//!   NeedsRedraw gates), which no VERA object builder models.
//! - Until its first AI visit, gamemd draws a piece at its Unlimbo coordinate
//!   (`0x007494E9`), 10 leptons below the bounce body it launches
//!   (`0x0074950C`); VERA draws the body's. At most one frame, about a pixel.
//!
//! ## Dependency rules
//! - Part of the app layer — may depend on everything.

use glam::IVec3;

use super::helpers::projection_admitted;
use crate::app::AppState;
use crate::app::presentation::render::draw_plan_lowering::{
    NativeDisplayOrder, ObjectPieceInstance, ObjectTexture, PlannedObjectInstance,
};
use crate::map::houses::HouseColorMap;
use crate::map::lighting::CellLightGrid;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::render::batch::{DepthAxis, SpriteInstance};
use crate::render::draw_state::{DrawState, FX_SHADOW};
use crate::render::native_z::{ZGradient, pack_z_gradient};
use crate::render::palette_light::PaletteLight;
use crate::render::tactical_draw_plan::{RenderZPolicy, SpriteEncoding};
use crate::render::unit_atlas::UnitSpriteEntry;
use crate::rules::house_colors::NO_REMAP;
use crate::rules::voxel_anim_type::VoxelAnimType;
use crate::sim::voxel_anim::VoxelAnimObject;
use crate::sim::world::Simulation;
use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, absolute_leptons_to_screen};
use crate::util::native_x87::adjust_for_z_standard;

/// Both blits walk the voxel blitter's vertical gradient: DrawIt blits through
/// `0x004AF2A0`, as TechnoClass's voxel body and uncached shadow do.
const VOXEL_Z_GRADIENT: u32 = pack_z_gradient(ZGradient::Vertical, false);

#[derive(Debug, Clone, Copy, PartialEq)]
struct PieceGeometry {
    point: [f32; 2],
    z_adjust: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct DebrisGeometry {
    body: PieceGeometry,
    shadow: PieceGeometry,
}

/// DrawIt's draw points and Z arguments; raw Z is in leptons. `ground_z` is
/// Cell578080's floor height under the piece and `bridge` the Cell's
/// `+0x140 & 0x100` flag. A piece at or above that bridge's deck shadows one
/// lepton above the deck (`0x00749C0E..0x00749C60`). The shadow's Z argument
/// is `-4 - AdjustForZ(surface)` (`0x00749D2A..0x00749D36`) and the body's
/// `-2 - AdjustForZ(GetCoordZ)` (`0x00749EA4..0x00749EB6`); the body point is
/// ObjectClass::DrawIfVisible's projection of the Location.
fn geometry(location: IVec3, ground_z: i32, bridge: bool) -> DebrisGeometry {
    let deck = ground_z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS);
    let (shadow_z, surface_z) = if bridge && location.z >= deck {
        (deck.wrapping_add(1), deck)
    } else {
        (ground_z, ground_z)
    };
    let (x, y) = absolute_leptons_to_screen(location.x, location.y, location.z);
    let (shadow_x, shadow_y) = absolute_leptons_to_screen(location.x, location.y, shadow_z);
    DebrisGeometry {
        body: PieceGeometry {
            point: [x, y],
            z_adjust: (-2i32).wrapping_sub(adjust_for_z_standard(location.z)),
        },
        shadow: PieceGeometry {
            point: [shadow_x, shadow_y],
            z_adjust: (-4i32).wrapping_sub(adjust_for_z_standard(surface_z)),
        },
    }
}

/// DrawIt's body Convert and intensity (`0x00749E05..0x00749E78`): an owned
/// piece takes its house's ColorScheme Convert (`+0x30C`, the one a Techno's
/// voxel body selects at `0x00705D70`) at the intensity of the cell under it.
/// An unowned piece takes the global UNITSNO plain Convert, at top-cell
/// intensity for IsTiberium and at 1000 otherwise (749E3F..749E78).
fn piece_palette(
    piece: &VoxelAnimObject,
    kind: &VoxelAnimType,
    sim: &Simulation,
    house_colors: &HouseColorMap,
    light: &CellLightGrid,
) -> (u32, u32, PaletteLight) {
    let location = piece.world_coord();
    let [x, y] = [location.x, location.y]
        .map(|lepton| lepton.div_euclid(256).clamp(0, i32::from(u16::MAX)) as u16);
    let Some(owner) = piece.owner_house() else {
        let brightness = if kind.is_tiberium {
            light
                .cell_light_at((x, y))
                .map_or(1000, |cell| cell.top_scalar)
        } else {
            1000
        };
        return (0, 1, PaletteLight::plain(53, brightness));
    };
    let scheme = house_colors
        .get(sim.interner().resolve(owner))
        .copied()
        .unwrap_or(NO_REMAP);
    (
        super::units::house_color_to_remap_row(scheme),
        0,
        crate::app::presentation::lighting::body_palette_light(light, (x, y), 0),
    )
}

/// What every piece of one frame shares.
struct DebrisView<'a> {
    terrain: &'a ResolvedTerrainGrid,
    camera: [f32; 2],
    viewport: [f32; 2],
    axis: DepthAxis,
}

/// One Display member's piece, if `id` names one that draws. The caller
/// visits the members in Display order.
pub(crate) fn voxel_anim_visitor<'a>(
    state: &'a AppState,
    order: &'a NativeDisplayOrder,
) -> Option<impl FnMut(&mut Vec<PlannedObjectInstance>, u64) + 'a> {
    let (Some(rt), Some(assets)) = (
        state.match_state.sim_runtime.presentation(),
        state.process_assets.manager(),
    ) else {
        return None;
    };
    let sim = &rt.simulation;
    let terrain = sim.resolved_terrain()?;
    if sim.voxel_anims().is_empty() {
        return None;
    }
    let rules = &rt.resources.rules;
    let input = &state.match_state.input;
    // Draw admission measures from the camera, the frame's corner, so it
    // takes the centring size: the battlefield and its offset on both sides.
    let (width, height) = crate::app::input::camera::tactical_centring_size(state);
    let view = DebrisView {
        terrain,
        camera: [input.camera_x, input.camera_y],
        viewport: [
            width as f32 / input.zoom_level,
            height as f32 / input.zoom_level,
        ],
        axis: super::helpers::depth_axis(state),
    };
    let presentation = &state.match_state.match_presentation;
    Some(move |objects: &mut Vec<PlannedObjectInstance>, id: u64| {
        let Some(piece) = sim.voxel_anims().get(id) else {
            return;
        };
        let kind = rules.voxel_anim_type(piece.type_id());
        let Some(model) = rules.voxel_anim_model(piece.type_id()) else {
            return;
        };
        let palette = piece_palette(
            piece,
            kind,
            sim,
            &presentation.house_color_map,
            presentation.lighting.grid(),
        );
        if let Some(object) = voxel_anim_draw_instance(
            id,
            piece,
            kind,
            palette,
            &view,
            order,
            // VoxelAnimClass draws uncached in the style 0xB43180 holds: its
            // shadow renders, then its body (`0x00749CBE`, `0x00749DA8`).
            |section, matrix| {
                let mut images = presentation.voxel_images.borrow_mut();
                let mut cache = state.renderer.vxl_pose_frame_cache.borrow_mut();
                let depth_tested = images.part_style(false);
                let (body, shadow) =
                    cache.render_tumble(assets, model, section, matrix, depth_tested, None)?;
                if let Some(shadow) = shadow {
                    images.render_shadow(None, false, 0, shadow.native_size());
                }
                if !images.render_part(false, body.native_size()).stale {
                    return Some((body, shadow));
                }
                let (body, _) = cache.render_tumble(
                    assets,
                    model,
                    section,
                    matrix,
                    true,
                    Some(images.depth_map()),
                )?;
                Some((body, shadow))
            },
        ) {
            objects.push(object);
        }
    })
}

/// One piece's draw. Display membership is admission: storage alone never
/// draws a piece. `render` rasterizes the bound physical model at the drawing
/// matrix and places its body and shadow on the pose page.
fn voxel_anim_draw_instance(
    id: u64,
    piece: &VoxelAnimObject,
    kind: &VoxelAnimType,
    (remap_row, source_palette, palette_light): (u32, u32, PaletteLight),
    view: &DebrisView<'_>,
    order: &NativeDisplayOrder,
    render: impl FnOnce(i32, glam::Mat4) -> Option<(UnitSpriteEntry, Option<UnitSpriteEntry>)>,
) -> Option<PlannedObjectInstance> {
    let parent = order.object_draw(id, SpriteEncoding::Voxel)?;
    let location = piece.world_coord();
    let (ground_z, bridge) = super::foot_depth::floor_probe(view.terrain, location.x, location.y);
    let geometry = geometry(location, ground_z, bridge);
    // Object6D2140's padded projection admission precedes DrawIt.
    if !projection_admitted(geometry.body.point, view.camera, view.viewport) {
        return None;
    }
    let matrix = piece.bounce().drawing_matrix().ok().map(|matrix| {
        crate::render::vxl_raster::hva_to_mat4(&matrix.map(|v| f32::from_bits(v.bits())), 1.)
    })?;
    let (body, shadow) = render(kind.voxel_index, matrix)?;
    let depth = crate::render::native_z::depth_for_row(
        geometry.body.point[1] + adjust_for_z_standard(location.z) as f32,
        view.axis.origin_y,
        view.axis.world_height,
    );
    let mut pieces = Vec::with_capacity(2);
    if let Some(shadow) = shadow {
        // `0x2001`: the shadow blitter darkens what lies beneath.
        let draw_state = DrawState {
            fx_flags: FX_SHADOW,
            ..DrawState::default()
        };
        pieces.push(pose_piece(
            shadow,
            geometry.shadow,
            depth,
            draw_state,
            PaletteLight::default(),
            source_palette,
        ));
    }
    // `0x2800`, or `0x2804` for a `Translucent=` type (`0x00749DEB..0x00749E00`).
    let draw_state = if kind.translucent {
        DrawState::translucent(remap_row)
    } else {
        DrawState {
            remap_row,
            ..DrawState::default()
        }
    };
    pieces.push(pose_piece(
        body,
        geometry.body,
        depth,
        draw_state,
        palette_light,
        source_palette,
    ));
    Some(PlannedObjectInstance::object(parent, pieces))
}

fn pose_piece(
    entry: UnitSpriteEntry,
    at: PieceGeometry,
    depth: f32,
    draw_state: DrawState,
    palette_light: PaletteLight,
    source_palette: u32,
) -> ObjectPieceInstance {
    ObjectPieceInstance {
        target: ObjectTexture::UnitPosePage(entry.page),
        render_z: RenderZPolicy::ReadOnly,
        instance: SpriteInstance {
            position: [at.point[0] + entry.offset_x, at.point[1] + entry.offset_y],
            size: entry.pixel_size,
            uv_origin: entry.uv_origin,
            uv_size: entry.uv_size,
            depth,
            tint: crate::map::lighting::DEFAULT_TINT,
            palette_light,
            source_palette,
            alpha: 1.0,
            draw_state,
            z_adjust: at.z_adjust as f32,
            z_gradient: VOXEL_Z_GRADIENT,
            ..Default::default()
        },
    }
}

#[cfg(test)]
#[path = "voxel_anims_tests.rs"]
mod tests;
