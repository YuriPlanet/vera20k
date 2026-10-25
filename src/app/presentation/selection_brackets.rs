//! Active-retail selected-building bracket producers.
//!
//! Frame-local line commands use the existing selection, Display, projection
//! and surface A/Z owners. There is no bracket raster or retained state here.

use crate::app::AppState;
use crate::map::entities::EntityCategory;
use crate::render::surface_line::{SurfaceLine, SurfaceLineBlend};
use crate::sim::movement::air_movement::current_fly_height;
use crate::sim::movement::ground_pose::object_get_coords;
use crate::util::lepton::absolute_leptons_to_screen;
use crate::util::native_x87::adjust_for_z_standard;
use std::collections::BTreeMap;

/// Tactical 6D913C/6D9153 submits both before this parent's body.
/// The separate retained Display sweep 6D9789 repeats extras.
#[derive(Debug, Default)]
pub(crate) struct BuildingBracketLines {
    pub behind: Vec<SurfaceLine>,
    pub extras: Vec<SurfaceLine>,
}

/// Quarter-stub arithmetic shared by `6F5EF0` and the three inline Extras
/// stubs: wrap in world leptons before signed division. Higher-Z endpoint
/// first; equal-Z uses the quarter first. Shared `4BFD30` clips and rasterizes.
fn push_stub(
    lines: &mut Vec<SurfaceLine>,
    corner: [i32; 3],
    toward: [i32; 3],
    camera: [i32; 2],
    color: u16,
) {
    let quarter: [i32; 3] =
        std::array::from_fn(|axis| corner[axis].wrapping_mul(3).wrapping_add(toward[axis]) / 4);
    let (from, to) = if corner[2] > quarter[2] {
        (corner, quarter)
    } else {
        (quarter, corner)
    };
    // Tactical 6DBB60: projection and 14 - AdjustForZ(Z) per endpoint.
    // The shared projection and camera both use VERA's +15 world-row frame.
    let project = |coord: [i32; 3]| {
        let (x, y) = absolute_leptons_to_screen(coord[0], coord[1], coord[2]);
        [
            (x as i32).wrapping_sub(camera[0]),
            (y as i32).wrapping_sub(camera[1]),
        ]
    };
    lines.push(SurfaceLine {
        from: project(from),
        to: project(to),
        z_adjust: [
            14i32.wrapping_sub(adjust_for_z_standard(from[2])),
            14i32.wrapping_sub(adjust_for_z_standard(to[2])),
        ],
        blend: SurfaceLineBlend::Replace(color),
    });
}

/// Techno DrawBracketCorner @0x006F5EF0 emits both ends in this order.
fn push_corner(
    lines: &mut Vec<SurfaceLine>,
    from: [i32; 3],
    to: [i32; 3],
    camera: [i32; 2],
    color: u16,
) {
    push_stub(lines, from, to, camera, color);
    push_stub(lines, to, from, camera, color);
}

/// DrawBehind @0x006F60D0 / selected-Building DrawExtras 6F539C..6F59FD.
/// Building vtable 7E3EBC +10C/+110 inherits these Techno functions. Original
/// execution: procedural_drawing_oracle/building_brackets.json.
///
/// This ordinary selected-building route submits ten behind/eleven extras stubs;
/// actual body/BUILDNGZ and shared surface A/Z decide their visible pixels.
pub(super) fn building_bracket_lines(
    center: [i32; 3],
    dimensions: [i32; 3],
    camera: [i32; 2],
    color: u16,
) -> BuildingBracketLines {
    let half_width = dimensions[0] / 2;
    let half_height = dimensions[1] / 2;
    let x0 = center[0].wrapping_sub(half_width);
    let x1 = center[0].wrapping_add(half_width);
    let y0 = center[1].wrapping_sub(half_height);
    let y1 = center[1].wrapping_add(half_height);
    let ground = [
        [x0, y0, center[2]],
        [x1, y0, center[2]],
        [x0, y1, center[2]],
        [x1, y1, center[2]],
    ];
    let roof = ground.map(|p| [p[0], p[1], p[2].wrapping_add(dimensions[2])]);
    let mut out = BuildingBracketLines {
        behind: Vec::with_capacity(10),
        extras: Vec::with_capacity(11),
    };
    // 6F623C/62D8/6378/6406/648F, original submission order.
    for (from, to) in [
        (ground[0], roof[0]),
        (ground[0], ground[1]),
        (ground[0], ground[2]),
        (roof[0], roof[2]),
        (roof[0], roof[1]),
    ] {
        push_corner(&mut out.behind, from, to, camera, color);
    }
    // 6F54B7/5531/55C1/5639, then inline 57A0/58CD/59E4. The dominated
    // RTTI15 arm and 6F6030 are not reached by this stable Building receiver.
    for (from, to) in [
        (ground[3], ground[2]),
        (ground[3], ground[1]),
        (ground[2], roof[2]),
        (ground[1], roof[1]),
    ] {
        push_corner(&mut out.extras, from, to, camera, color);
    }
    for (from, to) in [(ground[3], roof[3]), (roof[1], roof[3]), (roof[2], roof[3])] {
        push_stub(&mut out.extras, from, to, camera, color);
    }
    out
}

/// Prepare selected-building commands keyed by existing entity identity.
/// The Display planner consumes only admitted parents in its retained order;
/// entity storage order never becomes drawing order. Rebuilt every frame,
/// deselection/Display removal/camera rejection retain no bracket state.
///
/// Palette entries are the existing PALETTE.PAL Convert's packed indices 15/12.
/// GetHeight 5F5F40 chooses 12 below -4; DrawExtras 6F51AA skips sinking objects.
pub(crate) fn build_selection_bracket_lines(
    state: &AppState,
    camera: [i32; 2],
    palette: [u16; 2],
    parents: impl IntoIterator<Item = u64>,
) -> BTreeMap<u64, BuildingBracketLines> {
    let Some(sim) = state
        .match_state
        .sim_runtime
        .as_ref()
        .map(|rt| &rt.simulation)
    else {
        return BTreeMap::new();
    };
    let Some(rules) = state.rules() else {
        return BTreeMap::new();
    };
    let terrain = sim.resolved_terrain();
    parents
        .into_iter()
        .filter_map(|id| sim.entities().get(id))
        .filter(|e| e.category() == EntityCategory::Structure && e.selection.is_selected())
        .filter_map(|e| {
            let object = rules.object(sim.interner().resolve(e.type_ref()))?;
            let center = object_get_coords(e, terrain);
            let color = palette[usize::from(current_fly_height(e, terrain) < -4)];
            let mut lines = building_bracket_lines(
                [center.x, center.y, center.z],
                rules.building_dimensions(object),
                camera,
                color,
            );
            if e.sinking.is_active() {
                lines.extras.clear();
            }
            Some((e.stable_id(), lines))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::components::DriveCoord;
    use crate::sim::game_entity::GameEntity;
    use serde_json::Value;

    fn native() -> &'static Value {
        crate::test_fixture::json("tools/procedural_drawing_oracle/building_brackets.json")
    }
    fn point<const N: usize>(v: &Value) -> [i32; N] {
        std::array::from_fn(|i| v[i].as_i64().unwrap() as i32)
    }
    fn prepared_lines() -> BuildingBracketLines {
        let g = &native()["geometry"];
        let [x, y, z] = point(&g["input"]["location"]);
        let mut e =
            GameEntity::test_default_of_category(1, "GAPOWR", "A", 0, 0, EntityCategory::Structure);
        e.position.put_location(DriveCoord { x, y, z });
        let f = &crate::rules::foundation::FOUNDATION_TABLE
            [g["type_result"]["foundation"].as_u64().unwrap() as usize];
        e.type_facts
            .set_foundation_for_test(f.name.to_owned().into());
        let center = object_get_coords(&e, None);
        let mut camera: [i32; 2] = point(&g["input"]["camera"]);
        camera[1] += 15; // Shared VERA world-row bias.
        building_bracket_lines(
            [center.x, center.y, center.z],
            point(&g["type_result"]["dimensions"]),
            camera,
            g["passes"][0]["segments"][0]["packed_color"]
                .as_u64()
                .unwrap() as u16,
        )
    }

    /// This regression first failed on the old live reduced emitter. Native
    /// producer/projection/packed4BFD30 stores supply every expected result.
    #[test]
    fn native_building_brackets_match_original_pixels() {
        let produced = prepared_lines();
        let geometry = &native()["geometry"];
        for (name, lines) in [("behind", &produced.behind), ("extras", &produced.extras)] {
            let pass = geometry["passes"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["name"] == name)
                .unwrap();
            let expected = pass["segments"].as_array().unwrap();
            assert_eq!(lines.len(), expected.len(), "native {name} line count");
            for (line, expected) in lines.iter().zip(expected) {
                assert_eq!(line.from, point(&expected["from_screen"]), "{name} start");
                assert_eq!(line.to, point(&expected["to_screen"]), "{name} end");
                assert_eq!(line.z_adjust, point(&expected["z_adjust"]), "{name} depth");
                assert_eq!(
                    line.blend,
                    SurfaceLineBlend::Replace(expected["packed_color"].as_u64().unwrap() as u16),
                    "{name} packed color"
                );
                assert_eq!(expected["write_z"], 0);
            }
        }
        let clear = native()["raster_controls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["name"] == "clear")
            .unwrap();
        let mut actual = Vec::new();
        for line in produced.behind.iter().chain(&produced.extras) {
            crate::render::surface_line::rasterize_z_clipped(
                line.from,
                line.to,
                line.z_adjust,
                point(&geometry["input"]["clip"]),
                0,
                |p| actual.push(p.point),
            );
        }
        let expected: Vec<[i32; 2]> = clear["stores"]
            .as_array()
            .unwrap()
            .iter()
            .map(point)
            .collect();
        assert_eq!(actual, expected, "native complete GAPOWR ordered coverage");
        let actual: std::collections::BTreeSet<[i32; 2]> = actual.into_iter().collect();
        let expected: std::collections::BTreeSet<[i32; 2]> = clear["pixels"]
            .as_array()
            .unwrap()
            .iter()
            .map(point)
            .collect();
        assert_eq!(actual, expected, "native complete GAPOWR bracket coverage");
    }

    #[test]
    fn retail_gapowr_dimensions_match_original_art_reader() {
        let Some(retail) =
            crate::rules::retail_ini_fixture::retail_battle_rules_for_map("XMP03T4.MAP")
        else {
            return;
        };
        let object = retail.rules.object("GAPOWR").unwrap();
        assert_eq!(
            retail.rules.building_dimensions(object),
            point::<3>(&native()["geometry"]["type_result"]["dimensions"]),
            "production Battle rules/fixed ART versus original readers"
        );
    }
}
