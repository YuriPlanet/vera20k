//! Retained Bullet rendering: original Object5F4B10 -> Bullet468090.
//! Coordinates, lifetime and Display order stay with the simulation; this
//! read-only adapter resolves the current bridge surface and emits draw pieces.

use super::helpers::projection_admitted;
use crate::app::AppState;
use crate::app::presentation::interpolation::{PointKind, PointPose, PointSample};
use crate::app::presentation::render::draw_plan_lowering::{
    NativeDisplayOrder, ObjectPieceInstance, ObjectTexture, PlannedObjectInstance,
};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::render::batch::{DepthAxis, SpriteInstance};
use crate::render::palette_light::PaletteLight;
use crate::render::sprite_atlas::SpriteAtlas;
use crate::render::tactical_draw_plan::{BlitPolicy, RenderZPolicy, SpriteEncoding};
use crate::render::terrain_draw::TerrainPiece;
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::projectile_type::ProjectileType;
use crate::sim::projectile::{Projectile, ProjectileCoord};
use crate::util::lepton::absolute_leptons_to_screen;
use crate::util::native_x87::adjust_for_z_standard;

#[derive(Debug, Clone, Copy, PartialEq)]
struct BulletPieceGeometry {
    point: [f32; 2],
    z_adjust: i32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BulletGeometry {
    body: BulletPieceGeometry,
    shadow: Option<BulletPieceGeometry>,
}

/// Full original draw-call arguments are pinned in bridge_render.json.
/// Raw Z is in leptons, and OnBridge is independent of structural Cell flags.
fn geometry(
    coord: ProjectileCoord,
    ground_z: i32,
    structural: bool,
    on_bridge: bool,
    shadow: bool,
) -> BulletGeometry {
    let (x, y) = absolute_leptons_to_screen(coord.x, coord.y, coord.z);
    let body = BulletPieceGeometry {
        point: [x, y],
        z_adjust: (-30i32).wrapping_sub(adjust_for_z_standard(coord.z)),
    };
    let mut height = coord.z.wrapping_sub(ground_z).wrapping_sub(if on_bridge {
        crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS
    } else {
        0
    });
    let mut surface_z = ground_z;
    if !on_bridge && structural && height >= crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS {
        height = height.wrapping_sub(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
        surface_z = surface_z.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS);
    }
    let shadow = (shadow && height > 0).then(|| BulletPieceGeometry {
        point: [x, y + adjust_for_z_standard(height) as f32],
        z_adjust: (-10i32).wrapping_sub(adjust_for_z_standard(surface_z)),
    });
    BulletGeometry { body, shadow }
}

pub(crate) fn build_projectile_visual_instances(
    state: &AppState,
    objects: &mut Vec<PlannedObjectInstance>,
    order: &NativeDisplayOrder,
) {
    let Some(rt) = state.match_state.sim_runtime.as_ref() else {
        return;
    };
    let atlas = state.match_state.match_presentation.sprite_atlas.as_ref();
    let assets = state.process_assets.manager();
    let mut pose_cache = state.renderer.vxl_pose_frame_cache.borrow_mut();
    let sim = &rt.simulation;
    let Some(terrain) = sim.resolved_terrain() else {
        return;
    };
    let rules = &rt.resources.rules;
    let input = &state.match_state.input;
    let (_, _, width, height) = crate::app::input::camera::tactical_viewport_px(state);
    let width = width as f32 / input.zoom_level;
    let height = height as f32 / input.zoom_level;
    let axis = super::helpers::depth_axis(state);
    // Bullet468090's FirersPalette arm (`0x004683A1..0x004683D1`): the
    // retained +114 House's scheme, else the local player's.
    let house_colors = &state.match_state.match_presentation.house_color_map;
    let local_owner = crate::app::input::commands::preferred_local_owner_name(state);
    let local_scheme = local_owner
        .as_deref()
        .and_then(|owner| house_colors.get(owner).copied())
        .unwrap_or(crate::rules::house_colors::NO_REMAP);
    let viewer = local_owner
        .as_deref()
        .and_then(|name| sim.interner().get(name));
    for (_, projectile) in sim.projectiles().iter() {
        let Some(type_id) = projectile
            .payload()
            .projectile_type()
            .map(|id| sim.interner().resolve(id))
        else {
            continue;
        };
        let Some(kind) = rules.projectile(type_id) else {
            continue;
        };
        let scheme = projectile
            .firer_house()
            .and_then(|house| house_colors.get(sim.interner().resolve(house)).copied())
            .unwrap_or(local_scheme);
        let painted = state
            .match_state
            .match_presentation
            .interpolation
            .sample_point(
                PointKind::Bullet,
                projectile.id(),
                PointPose::bullet(projectile),
                sim.session().binary_frame(),
            );
        // VERA's translucent fog: enemy bullets and their trails would outline
        // an unseen firing position. Superweapon impact visuals are separate
        // effect owners and stay visible.
        if state.translucent_fog_enabled()
            && !state.match_state.sandbox_full_visibility
            && super::helpers::translucent_fog_conceals_bullet(
                sim,
                projectile,
                painted.coord,
                local_owner.as_deref(),
                viewer,
                sim.session().game_options().fog_of_war,
            )
        {
            continue;
        }
        if let Some(object) = projectile_draw_instance_sampled(
            projectile,
            painted,
            kind,
            type_id,
            scheme,
            atlas,
            terrain,
            [input.camera_x, input.camera_y],
            [width, height],
            axis,
            order,
            |image, matrix| {
                pose_cache.render_matrix(
                    assets?,
                    image,
                    matrix,
                    0,
                    true,
                    crate::render::vxl_raster::VxlLighting::Diffuse,
                )
            },
        ) {
            objects.push(object);
        }
    }
}

/// Shared production adapter used by full SHP/atlas/GPU native comparisons.
/// Retained Display membership is admission; storage alone never draws a Bullet.
#[cfg(test)]
pub(crate) fn projectile_draw_instance(
    projectile: &Projectile,
    kind: &ProjectileType,
    type_id: &str,
    scheme: HouseColorIndex,
    atlas: Option<&SpriteAtlas>,
    terrain: &ResolvedTerrainGrid,
    camera: [f32; 2],
    viewport: [f32; 2],
    axis: DepthAxis,
    order: &NativeDisplayOrder,
    render_voxel: impl FnOnce(&str, glam::Mat4) -> Option<crate::render::unit_atlas::UnitSpriteEntry>,
) -> Option<PlannedObjectInstance> {
    projectile_draw_instance_sampled(
        projectile,
        PointPose::bullet(projectile).current(),
        kind,
        type_id,
        scheme,
        atlas,
        terrain,
        camera,
        viewport,
        axis,
        order,
        render_voxel,
    )
}

pub(crate) fn projectile_draw_instance_sampled(
    projectile: &Projectile,
    painted: PointSample,
    kind: &ProjectileType,
    type_id: &str,
    scheme: HouseColorIndex,
    atlas: Option<&SpriteAtlas>,
    terrain: &ResolvedTerrainGrid,
    camera: [f32; 2],
    viewport: [f32; 2],
    axis: DepthAxis,
    order: &NativeDisplayOrder,
    render_voxel: impl FnOnce(&str, glam::Mat4) -> Option<crate::render::unit_atlas::UnitSpriteEntry>,
) -> Option<PlannedObjectInstance> {
    let encoding = if kind.voxel {
        SpriteEncoding::Voxel
    } else {
        SpriteEncoding::Plain
    };
    let mut parent = order.object_draw(projectile.id(), encoding)?;
    // `0x004680E2..0x004680FE`: an `Inviso=` type and a bullet waiting on
    // its `NUKE` anim (`+0x158`) draw nothing.
    if kind.inviso || projectile.awaiting_anim() {
        return None;
    }
    let point = painted.screen;
    if !projection_admitted(point, camera, viewport) {
        return None;
    }
    let depth = crate::render::native_z::depth_for_row(
        point[1] + painted.lift,
        axis.origin_y,
        axis.world_height,
    );
    if kind.voxel {
        // Bullet468104..46824A returns before the SHP frame, shadow and
        // palette arms. Its retained LoadVoxelAssets5F8110 pair supplies
        // frame0, diffuse lighting, type Color and no owner/cell lighting.
        let image = kind.voxel_image_load()?;
        let matrix = painted.voxel_matrix()?;
        let entry = render_voxel(image, matrix)?;
        let runtime_scheme = u32::try_from(kind.color_scheme()).ok()?;
        let remap_row = runtime_scheme / 2 + 1;
        let shades = if runtime_scheme & 1 == 0 { 1 } else { 53 };
        let palette_light = PaletteLight::new([1000; 3], shades, 1000, true);
        // Flat46B1AC..46B21A: flags2800 select Convert+98 ->494B60,
        // a strict Z-read/no-write leaf, gradient0 and file-backed81AF00=-65.
        // Non-Flat flags800 retain Convert+6C ->493DF0 (no Z test/store),
        // gradient2 and zero adjustment. Both use the same body and palette.
        let (policy, z_adjust, gradient) = if kind.flat {
            (
                BlitPolicy::z_read(encoding),
                -65.0,
                crate::render::native_z::ZGradient::Flat,
            )
        } else {
            (
                BlitPolicy::z_none(encoding),
                0.0,
                crate::render::native_z::ZGradient::Vertical,
            )
        };
        parent.policy = policy;
        let piece = ObjectPieceInstance {
            target: ObjectTexture::UnitPosePage(entry.page),
            render_z: policy.render_z,
            instance: SpriteInstance {
                position: [point[0] + entry.offset_x, point[1] + entry.offset_y],
                size: entry.pixel_size,
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                depth,
                tint: crate::map::lighting::DEFAULT_TINT,
                palette_light,
                alpha: 1.0,
                draw_state: crate::render::draw_state::DrawState {
                    remap_row,
                    ..Default::default()
                },
                z_adjust,
                z_gradient: crate::render::native_z::pack_z_gradient(gradient, false),
                ..Default::default()
            },
        };
        return Some(PlannedObjectInstance::object(parent, vec![piece]));
    }
    let frame = u16::from(crate::sim::projectile::projectile_shp_frame_with_velocity(
        projectile,
        kind,
        painted.bullet_velocity(projectile.velocity()),
    ));
    let entry = atlas?.projectile_sprite(type_id, kind, frame, scheme)?;
    let (ground_z, structural) =
        super::foot_depth::floor_probe(terrain, painted.coord[0], painted.coord[1]);
    let mut geometry = geometry(
        painted.projectile_coord(),
        ground_z,
        structural,
        projectile.on_bridge(),
        kind.shadow,
    );
    // Keep body and projected ground shadow on the same fractional XY sample;
    // Z lift belongs to the body alone, not to the ground shadow.
    let raw = geometry.body.point;
    let delta = [point[0] - raw[0], point[1] - raw[1]];
    geometry.body.point = point;
    geometry.body.z_adjust = (-30. - painted.lift).round() as i32;
    if let Some(shadow) = geometry.shadow.as_mut() {
        let raw_lift = adjust_for_z_standard(painted.coord[2]) as f32;
        shadow.point[0] += delta[0];
        shadow.point[1] += delta[1] + painted.lift - raw_lift;
    }
    let mut pieces = Vec::with_capacity(2);
    for (piece, geometry) in geometry
        .shadow
        .map(|shadow| (TerrainPiece::Shadow, shadow))
        .into_iter()
        .chain(std::iter::once((TerrainPiece::Body, geometry.body)))
    {
        pieces.push(ObjectPieceInstance {
            target: ObjectTexture::SpriteAtlasEdit(entry.page as usize, piece),
            render_z: RenderZPolicy::ReadOnly,
            instance: SpriteInstance {
                position: [
                    geometry.point[0] + entry.offset_x,
                    geometry.point[1] + entry.offset_y,
                ],
                size: entry.pixel_size,
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
                source_palette: entry.source_palette,
                depth,
                tint: crate::map::lighting::DEFAULT_TINT,
                palette_light: PaletteLight::plain(53, 1000),
                alpha: 1.0,
                z_adjust: geometry.z_adjust as f32,
                z_gradient: 0,
                ..Default::default()
            },
        });
    }
    Some(PlannedObjectInstance::object(parent, pieces))
}

#[cfg(test)]
#[path = "projectile_render_tests.rs"]
mod native_tests;

#[cfg(test)]
#[path = "projectile_flight_tests.rs"]
mod flight_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projectile_bridge_height_matches_original_object_projection() {
        // Original5F4B10 ->6D2140: native(-300,315), VERA world row bias15.
        // This regression first failed on the old production builder: Y494.
        let actual = geometry(
            ProjectileCoord::new(2688, 5248, 1040),
            624,
            true,
            false,
            true,
        );
        assert_eq!(actual.body.point, [-300.0, 330.0]);
        assert_eq!(actual.body.z_adjust, -180);
        assert!(actual.shadow.is_none());
    }
}

#[cfg(test)]
#[path = "projectile_voxel_tests.rs"]
mod voxel_tests;

#[cfg(test)]
pub(crate) use voxel_tests::{admitted_kirov_bomb, launched_stock_submarine_torpedo};
