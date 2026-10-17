//! Tactical selected-list6DBE20 -> Building456750 -> Game456980.
//! Reads the existing selection, online, current-house, coordinate, color and
//! clock owners. No RNG draws, timer writes, detach calls or retained draw state.

use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::render::radial_indicator::{RadialIndicatorDraw, radius_pixels};
use crate::render::surface_line::SurfaceLineViewport;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

pub(crate) fn build(
    sim: Option<&Simulation>,
    rules: Option<&RuleSet>,
    selected: &[u64],
    colors: &HouseColorMap,
    viewport: SurfaceLineViewport,
    time_ms: u64,
) -> Vec<RadialIndicatorDraw> {
    let (Some(sim), Some(rules)) = (sim, rules) else {
        return Vec::new();
    };
    let mut draws = Vec::new();
    //6DBE39..72 walks CurrentObjects FORWARD. Overlapping destination edits
    //must retain this order, independently of EntityStore allocation order.
    for &id in selected {
        let Some(entity) = sim.entities().get(id) else {
            continue;
        };
        let Some(object) = sim.interner.object_type(entity.type_ref(), rules) else {
            continue;
        };
        if !object.has_radial_indicator() || entity.category() != EntityCategory::Structure {
            continue;
        }
        let weapon = crate::sim::combat::combat_weapon::current_weapon(
            entity,
            object,
            sim.entities(),
            rules,
            &sim.interner,
        );
        //Residual: Mission_Unload44E29D..44E36B's+268/+669 toggle and the
        //454B91 operational-off reset are a separate gap lifecycle. Stock
        //GAGAP10/10 makes this range query invariant under that missing flag.
        let range = object.building_sensor_range(false, weapon.map(|weapon| weapon.range_leptons));
        //456768 reads+660, the existing temporal online query;
        //it does NOT sample4555D0 operational/power or a fog predicate.
        if range <= 0 || !entity.building_online() || !sim.house_is_human_player(entity.owner()) {
            continue;
        }
        let position = crate::sim::movement::ground_pose::object_get_coords(
            entity,
            sim.resolved_terrain.as_ref(),
        );
        let (x, y) =
            crate::util::lepton::absolute_leptons_to_screen(position.x, position.y, position.z);
        let center = [
            x as i32 - viewport.camera[0] + viewport.clip[0],
            y as i32 - viewport.camera[1] + viewport.clip[1],
        ];
        let index = colors
            .get(sim.interner.resolve(entity.owner()))
            .copied()
            .unwrap_or(crate::rules::house_colors::NO_REMAP);
        let rgb = crate::render::palette_light::house_color_rgb(&rules.house_color_ramps, index);
        let draw = RadialIndicatorDraw {
            center,
            radius: range,
            pixel_radius: false,
            rgb,
            fade: true,
            rays: true,
            frame: sim.session.binary_frame as i32,
        };
        if !object.concentric_radial_indicator() {
            draws.push(draw);
            continue;
        }
        //4567EE uses the frame owner's one timeGetTime input (also used by
        //radar). It never starts a simulation timer or advances a new clock.
        let phase = ((time_ms as u32) >> 1) & 0x3ff;
        let mut outer = draw;
        outer.fade = false;
        outer.rays = false;
        if phase & 0x200 == 0 {
            if phase & 0x100 == 0 {
                outer.rgb = crate::render::native_surface_format::interpolate_rgb8(
                    rgb,
                    [255; 3],
                    (!phase) & 255,
                );
            }
            draws.push(outer);
        }
        let expanding = (radius_pixels(range) as u32).wrapping_mul(phase) >> 10;
        if expanding > 32 {
            draws.push(RadialIndicatorDraw {
                radius: expanding as i32,
                pixel_radius: true,
                rgb,
                ..outer
            });
        }
    }
    draws
}

#[cfg(test)]
#[path = "radial_indicator_app_tests.rs"]
mod tests;
