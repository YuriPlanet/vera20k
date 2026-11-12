//! Tactical::DrawUnitActionVisuals @0x006DBE20 -> Building456750 -> Game456980.
//! Reads the existing selection, online, current-house, coordinate, color and
//! clock and presentation-pulse owners. No RNG draws, timer writes or detach calls.

use crate::app::presentation::frame::{ObjectRecord, PresentationFrame};
use crate::map::entities::EntityCategory;
use crate::map::houses::HouseColorMap;
use crate::render::radial_indicator::{RadialIndicatorDraw, radius_pixels};
use crate::render::surface_line::SurfaceLineViewport;
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// Presentation budget for authored Range/LineMultiplier combinations. The
/// stock Nuke emits 15 rings. Larger sets are omitted as a whole; the native
/// outer pulse still advances and no simulation value is clamped.
const MAX_SUPERWEAPON_RINGS: i32 = 64;

/// One process-lived Tactical range fade state. Native
/// Tactical::DrawUnitActionVisuals @0x006DBE20 keeps two: the placement ring's
/// (`0x00B0E648`/`0x00842950`), advanced once the placing building's sensor
/// range is positive, and the superweapon ring's (`0x00B0E64C`/`0x00842954`),
/// advanced once the armed superweapon's Range is positive. Positive
/// LineMultiplier rings advance a local copy; those per-ring steps do not
/// change this persistent phase/direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RangePulse {
    phase: i32,
    direction: i32,
}

impl Default for RangePulse {
    fn default() -> Self {
        Self {
            phase: 0,
            direction: 1,
        }
    }
}

impl RangePulse {
    /// Native 0x006DBF15..0x006DBF4A (placement), 0x006DC25A..0x006DC28C
    /// (superweapon) and the superweapon's positive-LM local ring step.
    /// The direction changes after reaching either endpoint; the phase itself
    /// remains at 0/100 for the following step to move inward.
    fn advance(&mut self) -> i32 {
        self.phase += 10 * self.direction;
        if self.phase >= 100 {
            self.direction = -1;
        } else if self.phase <= 0 {
            self.direction = 1;
        }
        self.phase
    }

    /// The outer phase after `steps` composites; a draw between composites
    /// shows the last one's.
    fn advance_by(&mut self, steps: u32) -> i32 {
        for _ in 0..steps {
            self.advance();
        }
        self.phase
    }
}

/// One draw's radial indicators.
pub(crate) struct RadialIndicators {
    pub(crate) draws: Vec<RadialIndicatorDraw>,
}

pub(crate) fn build(
    sim: Option<&Simulation>,
    frame: Option<&PresentationFrame>,
    rules: Option<&RuleSet>,
    selected: &[u64],
    armed_super_weapon: Option<&str>,
    armed_building: Option<&str>,
    cursor_cell: Option<(u16, u16)>,
    show_power_plant_enhancer_range: bool,
    unit_action_lines: bool,
    placement_pulse: &mut RangePulse,
    superweapon_pulse: &mut RangePulse,
    colors: &HouseColorMap,
    viewport: SurfaceLineViewport,
    time_ms: u64,
    steps: u32,
) -> RadialIndicators {
    let (Some(sim), Some(frame), Some(rules)) = (sim, frame, rules) else {
        return RadialIndicators { draws: Vec::new() };
    };
    #[cfg(not(feature = "phobos"))]
    let _ = show_power_plant_enhancer_range;
    let mut draws = Vec::new();
    //6DBE39..72 walks CurrentObjects FORWARD. Overlapping destination edits
    //must retain this order, independently of EntityStore allocation order.
    for &id in selected {
        let Some(entity) = frame.object(id) else {
            continue;
        };
        let Some(object) = sim.interner().object_type(entity.type_ref(), rules) else {
            continue;
        };
        if !object.has_radial_indicator() || entity.category() != EntityCategory::Structure {
            continue;
        }
        push_building_indicator(
            &mut draws, sim, rules, entity, object, true, colors, viewport, time_ms,
        );
    }
    // 0x006DBE74: with Unit Action Lines off (Options+0x1E) the pass ends
    // here, before the placement and superweapon rings.
    if !unit_action_lines {
        return RadialIndicators { draws };
    }
    // Phobos "Show power plant enhancer range": while a building is being
    // placed, the current player's active enhancers that name the placing type
    // draw their own range ring. TacticalClass_DrawRadialIndicator_Building_
    // Extras @ 0x6DBEA3 filters by 1.5 * range cells from the current
    // foundation center cell, then calls Game::DrawRadialIndicator(false,
    // true, coords, owner color, range, false, true).
    #[cfg(feature = "phobos")]
    if show_power_plant_enhancer_range
        && let (Some(armed), Some(center_cell)) = (armed_building, cursor_cell)
        && let Some(player) = sim.session().current_house()
    {
        let center = (i32::from(center_cell.0), i32::from(center_cell.1));
        for entity in frame.objects() {
            if entity.category() != EntityCategory::Structure || entity.owner() != player {
                continue;
            }
            let Some(object) = sim.interner().object_type(entity.type_ref(), rules) else {
                continue;
            };
            if !object.is_power_plant_enhancer() || !object.power_plant_enhancer_enhances(armed) {
                continue;
            }
            let range =
                object.power_plant_enhancer_range() / crate::util::lepton::LEPTONS_PER_CELL_I32;
            if range <= 0 || !entity.is_power_plant_enhancer_active() {
                continue;
            }
            let coords = entity.hud.get_coords;
            let enhancer_cell = (
                coords.x / crate::util::lepton::LEPTONS_PER_CELL_I32,
                coords.y / crate::util::lepton::LEPTONS_PER_CELL_I32,
            );
            let dx = i64::from(enhancer_cell.0) - i64::from(center.0);
            let dy = i64::from(enhancer_cell.1) - i64::from(center.1);
            // Native compares CellStruct::DistanceFromSquared against
            // range * range * 2.25; 4 * d² > 9 * range² is the exact integer
            // form of that 1.5 * range prefilter.
            if 4 * (dx * dx + dy * dy) > 9 * i64::from(range) * i64::from(range) {
                continue;
            }
            let (x, y) =
                crate::util::lepton::absolute_leptons_to_screen(coords.x, coords.y, coords.z);
            let center = [
                x as i32 - viewport.camera[0] + viewport.clip[0],
                y as i32 - viewport.camera[1] + viewport.clip[1],
            ];
            let index = colors
                .get(sim.interner().resolve(entity.owner()))
                .copied()
                .unwrap_or(crate::rules::house_colors::NO_REMAP);
            let rgb =
                crate::render::palette_light::house_color_rgb(&rules.house_color_ramps, index);
            draws.push(RadialIndicatorDraw {
                center,
                radius: range as f32,
                truncate_radius: true,
                pixel_radius: false,
                rgb,
                fade: true,
                rays: false,
                frame: sim.session().binary_frame() as i32,
            });
        }
    }
    // 0x006DBE81: while a building is being placed (Display+0x11A4), the
    // placement rings replace the superweapon ring.
    if let Some(armed) = armed_building {
        if let Some(cell) = cursor_cell {
            push_placement_rings(
                &mut draws,
                sim,
                frame,
                rules,
                armed,
                cell,
                placement_pulse,
                colors,
                viewport,
                time_ms,
                steps,
            );
        }
        return RadialIndicators { draws };
    }
    if let Some(section) = armed_super_weapon
        && let Some(super_weapon) = rules.super_weapon(section)
        && super_weapon.range > 0.0
    {
        // Native outer pulse state advances once per active composite,
        // before the line-multiplier branch; Range<=0 and no armed SW freeze it.
        let outer_factor = superweapon_pulse.advance_by(steps);
        if !super_weapon.range.is_finite() {
            return RadialIndicators { draws };
        }
        if let Some(position) = cursor_cell.and_then(|cell| foundation_center_coords(sim, cell)) {
            // 0x006DC1E1..0x006DC21C subtracts 0x7E from the
            // CurrentFoundationCenter cell's center coordinates, preserving Z.
            let center = screen_center(
                viewport,
                position.x.wrapping_sub(126),
                position.y.wrapping_sub(126),
                position.z,
            );
            // The native caller selects CurrentPlayerPtr's HouseClass RGB.
            let house_rgb = current_house_rgb(sim, rules, colors);
            let base = RadialIndicatorDraw {
                center,
                radius: super_weapon.range,
                truncate_radius: false,
                pixel_radius: false,
                rgb: house_rgb,
                fade: true,
                rays: false,
                frame: sim.session().binary_frame() as i32,
            };
            if super_weapon.line_multiplier < 1 {
                // Native +0x20 pre-ftol flag is enabled for the single-ring path.
                draws.push(RadialIndicatorDraw {
                    rgb: crate::render::native_surface_format::interpolate_rgb8(
                        house_rgb,
                        [0; 3],
                        outer_factor as u32,
                    ),
                    truncate_radius: true,
                    ..base
                });
            } else {
                use crate::util::native_x87::X87Chop53 as X;
                let multiplier = super_weapon.line_multiplier;
                let range = X::load_f32(crate::util::native_x87::NativeF32Bits::from_bits(
                    super_weapon.range.to_bits(),
                ))
                .expect("finite superweapon range");
                let final_index = X::mul(X::load_i32(multiplier), range);
                if X::compare(final_index, X::load_i32(MAX_SUPERWEAPON_RINGS))
                    != crate::util::native_x87::X87Ordering::Less
                {
                    return RadialIndicators { draws };
                }
                // Native stores outer phase/direction once, then advances only
                // these local registers for each positive-LM ring.
                let mut ring_pulse = *superweapon_pulse;
                for index in 0..MAX_SUPERWEAPON_RINGS {
                    // 6DC2F5..6DC3A3 compares the exact integer FILD against the
                    // full PC53 product of LineMultiplier * stored f32 Range.
                    if X::compare(X::load_i32(index), final_index)
                        == crate::util::native_x87::X87Ordering::Greater
                    {
                        break;
                    }
                    // Native stores the integer loop index to m32 before using
                    // that f32 numerator in FDIVR, then stores the quotient back
                    // to m32 for 456980. Keep both native f32 conversion points.
                    let numerator =
                        X::load_f32(X::store_f32(X::load_i32(index)).expect("finite ring index"))
                            .expect("finite f32 ring index");
                    let radius = X::div(numerator, X::load_i32(multiplier))
                        .expect("positive LineMultiplier");
                    let radius = f32::from_bits(
                        X::store_f32(radius)
                            .expect("finite superweapon ring radius")
                            .bits(),
                    );
                    let factor = ring_pulse.advance();
                    draws.push(RadialIndicatorDraw {
                        radius,
                        rgb: crate::render::native_surface_format::interpolate_rgb8(
                            house_rgb,
                            [0; 3],
                            factor as u32,
                        ),
                        ..base
                    });
                }
            }
        }
    }
    RadialIndicators { draws }
}

/// `BuildingClass::DrawRadialIndicator @ 0x00456750(rays)`: the building's
/// sensor range ring, or its concentric pair. `rays` reaches only the plain
/// ring (`0x00456959`); the concentric rings always pass 0.
#[allow(clippy::too_many_arguments)]
fn push_building_indicator(
    draws: &mut Vec<RadialIndicatorDraw>,
    sim: &Simulation,
    rules: &RuleSet,
    entity: &ObjectRecord,
    object: &crate::rules::object_type::ObjectType,
    rays: bool,
    colors: &HouseColorMap,
    viewport: SurfaceLineViewport,
    time_ms: u64,
) {
    // GetSensorRange 0x004566B0 with its current weapon, captured for its
    // own type, which every caller passes as `object`.
    let range = entity.hud.sensor_range();
    //456768 reads+660, the existing temporal online query;
    //it does NOT sample4555D0 operational/power or a fog predicate.
    if range <= 0 || !entity.building_online() || !sim.house_is_human_player(entity.owner()) {
        return;
    }
    let position = entity.hud.get_coords;
    let center = screen_center(viewport, position.x, position.y, position.z);
    let index = colors
        .get(sim.interner().resolve(entity.owner()))
        .copied()
        .unwrap_or(crate::rules::house_colors::NO_REMAP);
    let rgb = crate::render::palette_light::house_color_rgb(&rules.house_color_ramps, index);
    let draw = RadialIndicatorDraw {
        center,
        radius: range as f32,
        truncate_radius: true,
        pixel_radius: false,
        rgb,
        fade: true,
        rays,
        frame: sim.session().binary_frame() as i32,
    };
    if !object.concentric_radial_indicator() {
        draws.push(draw);
        return;
    }
    //4567EE uses the last composite's timeGetTime (`time_ms`). It never
    //starts a simulation timer or advances a new clock.
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
    let Some(pixels) = radius_pixels(range as f32, true) else {
        return;
    };
    let expanding = (pixels as u32).wrapping_mul(phase) >> 10;
    if expanding > 32 {
        draws.push(RadialIndicatorDraw {
            radius: expanding as f32,
            truncate_radius: true,
            pixel_radius: true,
            rgb,
            ..outer
        });
    }
}

/// The placing building's range ring and the rings of its nearest same-type
/// buildings, `0x006DBEA3..0x006DC1A1`.
#[allow(clippy::too_many_arguments)]
fn push_placement_rings(
    draws: &mut Vec<RadialIndicatorDraw>,
    sim: &Simulation,
    frame: &PresentationFrame,
    rules: &RuleSet,
    armed: &str,
    (cell_x, cell_y): (u16, u16),
    pulse: &mut RangePulse,
    colors: &HouseColorMap,
    viewport: SurfaceLineViewport,
    time_ms: u64,
    steps: u32,
) {
    let Some(object) = rules.object(armed) else {
        return;
    };
    // GetSensorRange 0x004566B0 of the placing object, which is not on the
    // map yet: no Gap supercharge, and its first weapon.
    let range = object.building_sensor_range(
        false,
        crate::sim::combat::combat_weapon::fresh_object_current_weapon(object, rules)
            .map(|weapon| weapon.range_leptons),
        rules.ares(),
    );
    if range <= 0 {
        return;
    }
    let phase = pulse.advance_by(steps);
    // 0x006DBEB2..0x006DBFAE: the CurrentFoundationCenter cell's center,
    // without the superweapon ring's 0x7E shift; the owner's color toward
    // black by the pulse; Game::DrawRadialIndicator(0, 1, .., range, 0, 1).
    if let Some(position) = foundation_center_coords(sim, (cell_x, cell_y)) {
        draws.push(RadialIndicatorDraw {
            center: screen_center(viewport, position.x, position.y, position.z),
            radius: range as f32,
            truncate_radius: true,
            pixel_radius: false,
            rgb: crate::render::native_surface_format::interpolate_rgb8(
                current_house_rgb(sim, rules, colors),
                [0; 3],
                phase as u32,
            ),
            fade: true,
            rays: false,
            frame: sim.session().binary_frame() as i32,
        });
    }
    // 0x006DBFB3..0x006DC1A1: CurrentPlayer's alive buildings of the same type
    // within 2 * range cells; every one at the smallest whole-cell distance
    // (Sqrt_Approx, ftol, short) draws its own ring without rays. Stable-id
    // order stands in for the house's Buildings vector; only overlap differs.
    let Some(player) = sim.session().current_house() else {
        return;
    };
    let center = (cell_x as i16, cell_y as i16);
    let nearby: Vec<_> = frame
        .objects()
        .filter(|entity| entity.category() == EntityCategory::Structure)
        .filter_map(|entity| {
            if entity.owner() != player || !entity.lifecycle.object_alive() {
                return None;
            }
            let kind = sim.interner().object_type(entity.type_ref(), rules)?;
            if !std::ptr::eq(kind, object) {
                return None;
            }
            let coords = entity.hud.get_coords;
            let dx = center.0.wrapping_sub((coords.x / 256) as i16);
            let dy = center.1.wrapping_sub((coords.y / 256) as i16);
            let distance =
                crate::util::native_x87::sqrt_approx_length([i32::from(dx), i32::from(dy)]) as i16;
            Some((entity, kind, i32::from(distance)))
        })
        .collect();
    let nearest = nearby
        .iter()
        .map(|&(_, _, distance)| distance)
        .filter(|&distance| distance <= 2 * range)
        .min();
    for &(entity, kind, distance) in &nearby {
        if Some(distance) == nearest {
            push_building_indicator(
                draws, sim, rules, entity, kind, false, colors, viewport, time_ms,
            );
        }
    }
}

/// The CurrentFoundationCenter cell's center coordinates (`0x005657A0`,
/// `0x00480A30`). Missing-cell drawing may read the fallback's retained
/// height, but its coordinate stamp must stay private to presentation.
fn foundation_center_coords(
    sim: &Simulation,
    (cell_x, cell_y): (u16, u16),
) -> Option<crate::sim::components::DriveCoord> {
    let center = || {
        crate::sim::cell_rect::get_cellclass_fallback(
            sim.resolved_terrain(),
            i32::from(cell_x),
            i32::from(cell_y),
        )
        .center_coordinate()
    };
    match sim.resolved_terrain() {
        Some(terrain) => terrain.shared_cell_dummy().detached(center),
        None => center(),
    }
}

/// CurrentPlayer's HouseClass RGB through the house color map and ramps.
fn current_house_rgb(sim: &Simulation, rules: &RuleSet, colors: &HouseColorMap) -> [u8; 3] {
    let index = sim
        .session()
        .current_house()
        .and_then(|house| colors.get(sim.interner().resolve(house)).copied())
        .unwrap_or(crate::rules::house_colors::NO_REMAP);
    crate::render::palette_light::house_color_rgb(&rules.house_color_ramps, index)
}

fn screen_center(viewport: SurfaceLineViewport, x: i32, y: i32, z: i32) -> [i32; 2] {
    let (x, y) = crate::util::lepton::absolute_leptons_to_screen(x, y, z);
    [
        x as i32 - viewport.camera[0] + viewport.clip[0],
        y as i32 - viewport.camera[1] + viewport.clip[1],
    ]
}

#[cfg(test)]
#[path = "radial_indicator_app_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "radial_indicator_gpu_tests.rs"]
mod gpu_tests;
