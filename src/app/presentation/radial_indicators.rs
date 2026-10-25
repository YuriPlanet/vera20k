//! Tactical selected-list6DBE20 -> Building456750 -> Game456980.
//! Reads the existing selection, online, current-house, coordinate, color and
//! clock and presentation-pulse owners. No RNG draws, timer writes or detach calls.

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

/// Process-lived Tactical superweapon range fade state. Native
/// Tactical::DrawUnitActionVisuals @0x006DBE20 advances these globals only
/// after the selected superweapon passes its positive-Range gate. Positive
/// LineMultiplier rings advance a local copy; those per-ring steps do not
/// change this persistent phase/direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SuperweaponRangePulse {
    phase: i32,
    direction: i32,
}

impl Default for SuperweaponRangePulse {
    fn default() -> Self {
        Self {
            phase: 0,
            direction: 1,
        }
    }
}

impl SuperweaponRangePulse {
    /// Native 0x006DC25A..0x006DC28C and its positive-LM local ring step.
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
}

pub(crate) fn build(
    sim: Option<&Simulation>,
    rules: Option<&RuleSet>,
    selected: &[u64],
    armed_super_weapon: Option<&str>,
    cursor_cell: Option<(u16, u16)>,
    superweapon_pulse: &mut SuperweaponRangePulse,
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
        let Some(object) = sim.interner().object_type(entity.type_ref(), rules) else {
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
            sim.interner(),
        );
        let range = object.building_sensor_range(
            entity.gap_supercharged(),
            weapon.map(|weapon| weapon.range_leptons),
        );
        //456768 reads+660, the existing temporal online query;
        //it does NOT sample4555D0 operational/power or a fog predicate.
        if range <= 0 || !entity.building_online() || !sim.house_is_human_player(entity.owner()) {
            continue;
        }
        let position =
            crate::sim::movement::ground_pose::object_get_coords(entity, sim.resolved_terrain());
        let (x, y) =
            crate::util::lepton::absolute_leptons_to_screen(position.x, position.y, position.z);
        let center = [
            x as i32 - viewport.camera[0] + viewport.clip[0],
            y as i32 - viewport.camera[1] + viewport.clip[1],
        ];
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
            rays: true,
            frame: sim.session().binary_frame() as i32,
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
        let Some(pixels) = radius_pixels(range as f32, true) else {
            continue;
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
    if let Some(section) = armed_super_weapon
        && let Some(super_weapon) = rules.super_weapon(section)
        && super_weapon.range > 0.0
    {
        // Native outer pulse state advances once per active draw, before the
        // line-multiplier branch; Range<=0 and no armed SW freeze it.
        let outer_factor = superweapon_pulse.advance();
        if !super_weapon.range.is_finite() {
            return draws;
        }
        let position = cursor_cell.and_then(|(cell_x, cell_y)| {
            let center = || {
                crate::sim::cell_rect::get_cellclass_fallback(
                    sim.resolved_terrain(),
                    i32::from(cell_x),
                    i32::from(cell_y),
                )
                .center_coordinate()
            };
            // Missing-cell drawing may read the fallback's retained height,
            // but its coordinate stamp must stay private to presentation.
            match sim.resolved_terrain() {
                Some(terrain) => terrain.shared_cell_dummy().detached(center),
                None => center(),
            }
        });
        if let Some(position) = position {
            // Tactical::DrawUnitActionVisuals @0x006DBE20 reads the current
            // CurrentFoundationCenter, subtracts 0x7E from its ground cell center,
            // and calls Game::DrawRadialIndicator @0x00456980. Use the launch
            // cursor-cell owner supplied by render_game, preserving Z.
            let (x, y) = crate::util::lepton::absolute_leptons_to_screen(
                position.x.wrapping_sub(126),
                position.y.wrapping_sub(126),
                position.z,
            );
            let center = [
                x as i32 - viewport.camera[0] + viewport.clip[0],
                y as i32 - viewport.camera[1] + viewport.clip[1],
            ];
            // The native caller selects CurrentPlayerPtr's HouseClass RGB. The
            // current house's existing map/ramp projection owns that same color.
            let color_index = sim
                .session()
                .current_house()
                .and_then(|house| colors.get(sim.interner().resolve(house)).copied())
                .unwrap_or(crate::rules::house_colors::NO_REMAP);
            let house_rgb = crate::render::palette_light::house_color_rgb(
                &rules.house_color_ramps,
                color_index,
            );
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
                    return draws;
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
    draws
}

#[cfg(test)]
#[path = "radial_indicator_app_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "radial_indicator_gpu_tests.rs"]
mod gpu_tests;
