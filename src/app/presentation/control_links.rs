//! CaptureManager472160 -> Techno704E40 relationship curves, at Tactical's tail.
//! The simulation owns nodes and expiry; drawing never mutates those owners.

use super::target_lines::{TacticalViewport, push_surface_word};
use crate::map::houses::HouseColorMap;
use crate::render::batch::SpriteInstance;
use crate::rules::house_colors::NO_REMAP;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::world::Simulation;

pub(crate) fn prepare(
    sim: &Simulation,
    rules: &RuleSet,
    colors: &HouseColorMap,
    wall_ms: u32,
    view: TacticalViewport,
) -> std::collections::HashMap<u64, Vec<SpriteInstance>> {
    // Tactical6D47A6/6D47CB visits a Techno's manager, then its controller's.
    // Opaque repeated draws at this frame's fixed clock are idempotent: keep
    // each manager's last visit, preserving overlap order without quadratic
    // work for a manager with many captives. This temporary index has no state.
    let mut last_visit = std::collections::HashMap::new();
    for (rank, (id, entity)) in sim.entities().iter_sorted().enumerate() {
        if entity.health.current() == 0 {
            continue;
        }
        if entity.capture_manager.is_some() {
            last_visit.insert(id, (rank * 2, id));
        }
        if let Some(controller) = entity.mind_control.controller() {
            last_visit.insert(controller, (rank * 2 + 1, id));
        }
    }
    let mut managers: Vec<_> = last_visit.into_iter().collect();
    managers.sort_unstable_by_key(|&(_, (rank, _))| rank);
    let mut by_entity: std::collections::HashMap<u64, Vec<SpriteInstance>> =
        std::collections::HashMap::new();
    for (controller, (_, after_entity)) in managers {
        for link in sim.capture_link_coordinates(controller, rules) {
            let color = colors
                .get(sim.interner().resolve(link.owner))
                .copied()
                .unwrap_or(NO_REMAP);
            let rgb =
                crate::render::palette_light::house_color_rgb(&rules.house_color_ramps, color);
            append_line_instances(
                by_entity.entry(after_entity).or_default(),
                link.from,
                link.to,
                rgb,
                wall_ms,
                view,
            );
        }
    }
    by_entity
}

/// The shared geometry/material producer used by the live overlay and GPU comparison.
pub(crate) fn append_line_instances(
    instances: &mut Vec<SpriteInstance>,
    from: DriveCoord,
    to: DriveCoord,
    rgb: [u8; 3],
    wall_ms: u32,
    view: TacticalViewport,
) {
    emit(from, to, rgb, wall_ms, view, |rect, word| {
        push_surface_word(instances, rect, word, view);
    });
}

/// Original704E40..705260: two opaque3x3 endpoint boxes, then32 solid1px
/// chords. The pulse lerps House RGB toward white using6612C0, never LaserDraw.
/// The x87 height expression is exact in integers: 512*(1-((i-16)/16)^2).
/// Native executable pixel controls: tools/procedural_drawing_oracle/control_links.
fn emit(
    from: DriveCoord,
    to: DriveCoord,
    rgb: [u8; 3],
    wall_ms: u32,
    view: TacticalViewport,
    mut draw: impl FnMut([i32; 4], u16),
) {
    if view.clip[2] <= 0 || view.clip[3] <= 0 {
        return;
    }
    use crate::render::native_surface_format::{RGB565, interpolate_rgb8};
    use crate::render::surface_line::{clip_line, solid_line};
    let project = |point: DriveCoord| {
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(point.x, point.y, point.z);
        [
            x as i32 - view.camera[0],
            y as i32 - view.camera[1] + view.clip[1],
        ]
    };
    let base = RGB565.pack_rgb8(rgb);
    super::target_lines::endpoint_boxes([project(from), project(to)], view.clip, |rect| {
        draw(rect, base)
    });
    let mut previous = from;
    let phase = (wall_ms >> 4).wrapping_mul(8);
    for i in 1i32..=32 {
        let axis = |a: i32, b: i32| a.wrapping_mul(32 - i).wrapping_add(b.wrapping_mul(i)) / 32;
        let next = DriveCoord {
            x: axis(from.x, to.x),
            y: axis(from.y, to.y),
            z: axis(from.z, to.z).wrapping_add(512 - 2 * (i - 16) * (i - 16)),
        };
        let mut start = project(previous);
        let mut end = project(next);
        if clip_line(&mut start, &mut end, view.clip) {
            let amount = (i as u32 * 8).wrapping_sub(phase) & 0x1f8;
            let color = if amount > 0 && amount <= 0xf8 {
                interpolate_rgb8(rgb, [255; 3], amount)
            } else {
                rgb
            };
            let word = RGB565.pack_rgb8(color);
            solid_line(start, end, |rect| draw(rect, word));
        }
        previous = next;
    }
}

#[cfg(test)]
#[path = "control_links_tests.rs"]
mod tests;
