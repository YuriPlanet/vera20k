//! Retained Bullet rendering: original Object5F4B10 -> Bullet468090.
//! Coordinates, lifetime and Display order stay with the simulation; this
//! read-only adapter resolves the current bridge surface and emits draw pieces.

use super::helpers::projection_admitted;
use crate::app::AppState;
use crate::app::presentation::render::draw_plan_lowering::{
    NativeDisplayOrder, ObjectPieceInstance, ObjectTexture, PlannedObjectInstance,
};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::render::batch::{DepthAxis, SpriteInstance};
use crate::render::palette_light::PaletteLight;
use crate::render::sprite_atlas::SpriteAtlas;
use crate::render::tactical_draw_plan::{RenderZPolicy, SpriteEncoding};
use crate::render::terrain_draw::TerrainPiece;
use crate::rules::house_colors::HouseColorIndex;
use crate::rules::projectile_type::ProjectileType;
use crate::sim::projectile::{Projectile, ProjectileCoord, projectile_shp_frame};
use crate::util::lepton::{absolute_leptons_to_screen, ground_height_leptons};
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

/// Cell578080 arithmetic without its shared dummy-coordinate write. Rendering
/// must not change a retained DummyCell target or future gameplay/RNG state.
fn ground_probe(terrain: &ResolvedTerrainGrid, coord: ProjectileCoord) -> (i32, bool) {
    let cell = super::foot_depth::depth_cell(
        terrain,
        None,
        None,
        [(coord.x / 256) as i16, (coord.y / 256) as i16],
    );
    let ground = ground_height_leptons(cell.level as u8, cell.ramp, coord.x, coord.y)
        .expect("Bullet draw requires a supported native Cell slope");
    (ground, cell.flags & 0x100 != 0)
}

pub(crate) fn build_projectile_visual_instances(
    state: &AppState,
    objects: &mut Vec<PlannedObjectInstance>,
    order: &NativeDisplayOrder,
) {
    let (Some(rt), Some(atlas)) = (
        state.match_state.sim_runtime.as_ref(),
        state.match_state.match_presentation.sprite_atlas.as_ref(),
    ) else {
        return;
    };
    let sim = &rt.simulation;
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
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
    let local_scheme = crate::app::input::commands::preferred_local_owner_name(state)
        .and_then(|owner| house_colors.get(&owner).copied())
        .unwrap_or(crate::rules::house_colors::NO_REMAP);
    for (_, projectile) in sim.projectiles.iter() {
        let Some(type_id) = rules
            .weapon(sim.interner.resolve(projectile.payload.weapon))
            .and_then(|weapon| weapon.projectile.as_deref())
        else {
            continue;
        };
        let Some(kind) = rules.projectile(type_id) else {
            continue;
        };
        let scheme = projectile
            .firer_house()
            .and_then(|house| house_colors.get(sim.interner.resolve(house)).copied())
            .unwrap_or(local_scheme);
        if let Some(object) = projectile_draw_instance(
            projectile,
            kind,
            type_id,
            scheme,
            atlas,
            terrain,
            [input.camera_x, input.camera_y],
            [width, height],
            axis,
            order,
        ) {
            objects.push(object);
        }
    }
}

/// Shared production adapter used by full SHP/atlas/GPU native comparisons.
/// Retained Display membership is admission; storage alone never draws a Bullet.
#[allow(clippy::too_many_arguments)]
pub(crate) fn projectile_draw_instance(
    projectile: &Projectile,
    kind: &ProjectileType,
    type_id: &str,
    scheme: HouseColorIndex,
    atlas: &SpriteAtlas,
    terrain: &ResolvedTerrainGrid,
    camera: [f32; 2],
    viewport: [f32; 2],
    axis: DepthAxis,
    order: &NativeDisplayOrder,
) -> Option<PlannedObjectInstance> {
    let parent = order.object_draw(projectile.id, SpriteEncoding::Plain)?;
    if kind.inviso || kind.voxel {
        return None;
    }
    let frame = u16::from(projectile_shp_frame(projectile, kind));
    let entry = atlas.projectile_sprite(type_id, kind, frame, scheme)?;
    let (ground_z, structural) = ground_probe(terrain, projectile.position);
    let geometry = geometry(
        projectile.position,
        ground_z,
        structural,
        projectile.on_bridge,
        kind.shadow,
    );
    // Object6D2140's padded projection admission precedes the shape clip.
    if !projection_admitted(geometry.body.point, camera, viewport) {
        return None;
    }
    let depth = crate::render::native_z::depth_for_row(
        geometry.body.point[1] + adjust_for_z_standard(projectile.position.z) as f32,
        axis.origin_y,
        axis.world_height,
    );
    let mut pieces = Vec::with_capacity(2);
    for (piece, geometry) in geometry
        .shadow
        .map(|shadow| (TerrainPiece::Shadow, shadow))
        .into_iter()
        .chain(std::iter::once((TerrainPiece::Body, geometry.body)))
    {
        pieces.push(ObjectPieceInstance {
            target: ObjectTexture::ProjectileShp(entry.page as usize, piece),
            render_z: RenderZPolicy::ReadOnly,
            instance: SpriteInstance {
                position: [
                    geometry.point[0] + entry.offset_x,
                    geometry.point[1] + entry.offset_y,
                ],
                size: entry.pixel_size,
                uv_origin: entry.uv_origin,
                uv_size: entry.uv_size,
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
