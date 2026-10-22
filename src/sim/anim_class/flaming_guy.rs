//! Anim425670, destination425D10 and admission4260F0. This is independent
//! of BounceClass: the runner uses ObjectClass's retained free-fall state.

use super::{AnimId, AnimWorldCoord};
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::{
    art_data::AnimTypeRuntimeConfig, overlay_types::OverlayTypeRegistry, ruleset::RuleSet,
};
use crate::sim::components::DriveCoord;
use crate::sim::movement::{ground_pose::query_ground_height, locomotor::MovementLayer};
use crate::sim::object_fall::ObjectFallState;
use crate::sim::occupancy::RawCellKey;
use crate::sim::world::Simulation;
use crate::util::direction::direction_delta;
use crate::util::direction_tables::{dir_from_facing16, facing16_between};
use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, GROUND_LEVEL_HEIGHT_LEPTONS};

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(super) struct FlamingGuyState {
    destination: Option<AnimWorldCoord>,
    retries: i32,
    expired: bool,
    fall: ObjectFallState,
}

#[cfg(test)]
#[path = "flaming_guy_tests.rs"]
mod tests;

fn point(coord: AnimWorldCoord) -> DriveCoord {
    DriveCoord {
        x: coord.x,
        y: coord.y,
        z: coord.z,
    }
}

fn center((x, y): (i16, i16)) -> AnimWorldCoord {
    AnimWorldCoord {
        x: i32::from(x) * 256 + 128,
        y: i32::from(y) * 256 + 128,
        z: 0,
    }
}

fn neighbor(cell: (i16, i16), direction: u8) -> (i16, i16) {
    let (dx, dy) = direction_delta(direction).expect("native eight-way direction");
    (
        cell.0.wrapping_add(dx as i16),
        cell.1.wrapping_add(dy as i16),
    )
}

fn expire(state: &mut FlamingGuyState, runtime: &mut super::AnimRuntime, running: i32, now: i32) {
    state.expired = true;
    runtime.rate_reload = 1;
    runtime.frame_timer.start(now, 1);
    runtime.current_frame = running.wrapping_mul(8).wrapping_add(1);
}

impl Simulation {
    /// Native4260F0 reads raw occupation, not mover pathfinding costs.
    fn flaming_guy_can_enter(
        &self,
        coord: AnimWorldCoord,
        candidate: (i16, i16),
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let cells = NativeCellQuery::canonical(terrain);
        let cell = cells.lookup(candidate);
        let level_z =
            i32::from(cells.ground_fields(cell).0 as i8).wrapping_mul(GROUND_LEVEL_HEIGHT_LEPTONS);
        let key = RawCellKey::from_native(terrain, cell);
        if cells.flags(cell) & 0x100 != 0
            && coord.z.wrapping_sub(level_z) > 2 * GROUND_LEVEL_HEIGHT_LEPTONS
        {
            return self
                .substrate
                .raw_cell_occupation
                .bits_at(key, MovementLayer::Bridge)
                & 0xE0
                == 0;
        }
        if matches!(cells.land_type(cell), 3 | 10 | 4) {
            return false;
        }
        let overlay = self.cell_overlay(cell).overlay_id;
        if overlay
            .and_then(|id| registry.and_then(|registry| registry.flags(id)))
            .is_some_and(|flags| flags.wall)
        {
            return false;
        }
        self.substrate
            .raw_cell_occupation
            .bits_at(key, MovementLayer::Ground)
            & 0xE0
            == 0
            && level_z.wrapping_sub(coord.z) <= 2 * GROUND_LEVEL_HEIGHT_LEPTONS
    }

    ///425D10: X-outer/Y-inner [-5,+5), biased strict nearest, then diagonal,
    ///X, Y. Only a failed water route spends the one Scenario fallback draw.
    fn flaming_guy_next_coord(
        &mut self,
        coord: AnimWorldCoord,
        registry: Option<&OverlayTypeRegistry>,
    ) -> Option<AnimWorldCoord> {
        let current = ((coord.x / 256) as i16, (coord.y / 256) as i16);
        let mut best = None;
        let mut distance = 10000;
        if let Some(terrain) = self.resolved_terrain.as_ref() {
            let cells = NativeCellQuery::canonical(terrain);
            for dx in -5i32..5 {
                for dy in -5i32..5 {
                    let candidate = (
                        current.0.wrapping_add(dx as i16),
                        current.1.wrapping_add(dy as i16),
                    );
                    if !crate::sim::cell_rect::cell_is_in_playfield_height_aware_in_query(
                        (i32::from(candidate.0), i32::from(candidate.1)),
                        self.playfield_bounds(),
                        Some(terrain),
                        Some(&cells),
                    ) {
                        continue;
                    }
                    let cell = cells.lookup(candidate);
                    if cells.land_type(cell) != 2 || cells.flags(cell) & 0x100 != 0 {
                        continue;
                    }
                    let selected = cells.coord(cell);
                    if cells.flags(cells.lookup(neighbor(selected, 6))) & 0x100 != 0
                        || cells.flags(cells.lookup(neighbor(selected, 0))) & 0x100 != 0
                    {
                        continue;
                    }
                    let raw = if cells.coord(cell) == current {
                        0
                    } else {
                        crate::util::native_x87::sqrt_approx_length([dx, dy, 0])
                    };
                    let score = raw - if dx > 0 || dy > 0 { 3 } else { 0 };
                    if score < distance {
                        best = Some(candidate);
                        distance = score;
                    }
                }
            }
        }
        if let Some(target) = best {
            let x = current
                .0
                .wrapping_add((i32::from(target.0) - i32::from(current.0)).signum() as i16);
            let y = current
                .1
                .wrapping_add((i32::from(target.1) - i32::from(current.1)).signum() as i16);
            for cell in [(x, y), (x, current.1), (current.0, y)] {
                if self.flaming_guy_can_enter(coord, cell, registry) {
                    return Some(center(cell));
                }
            }
        }
        let start = self.scenario_rng.next_range_i32_inclusive(0, 7) as u8;
        for offset in 0..8 {
            let candidate = neighbor(current, start.wrapping_add(offset) & 7);
            if self.flaming_guy_can_enter(coord, candidate, registry) {
                return Some(center(candidate));
            }
        }
        None
    }

    /// Original425670 then ObjectAI5F3E70, before the ordinary Anim AI tail.
    pub(super) fn anim_flaming_guy_ai(
        &mut self,
        id: AnimId,
        config: &AnimTypeRuntimeConfig,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let now = self.session.binary_frame() as i32;
        let Some(anim) = self.substrate.anims.get_mut(id) else {
            return;
        };
        let mut state = anim.flaming_guy.take().unwrap_or_default();
        let mut coord = anim.world_coord;
        if !state.expired {
            anim.runtime.rate_reload = 0;
            anim.runtime.frame_timer.start(now, 0);
        }
        let mut begin_expiry = false;
        if state.destination.is_none() || state.expired {
            if !state.fall.falling() && !state.expired {
                state.destination = self.flaming_guy_next_coord(coord, registry);
                begin_expiry = state.destination.is_none();
            }
        } else {
            let destination = state.destination.expect("running destination");
            let distance = crate::util::native_x87::distance_3d_leptons(
                [coord.x, coord.y, 0],
                [destination.x, destination.y, 0],
            );
            let mut moved = false;
            if distance < 19 {
                if !state.fall.falling() {
                    // Selection/RNG precedes the water/retry termination tests.
                    let next = self.flaming_guy_next_coord(coord, registry);
                    let water = self.resolved_terrain.as_ref().is_some_and(|terrain| {
                        let cells = NativeCellQuery::canonical(terrain);
                        let cell = cells.lookup_world(destination.x, destination.y);
                        matches!(cells.land_type(cell), 2 | 6)
                            && crate::sim::movement::air_movement::object_height_in_query(
                                point(coord),
                                false,
                                Some(&cells),
                            ) <= GROUND_LEVEL_HEIGHT_LEPTONS
                    });
                    if next.is_none() || state.retries > 6 || water {
                        begin_expiry = true;
                    } else {
                        state.retries = state.retries.wrapping_add(1);
                        coord.x = destination.x;
                        coord.y = destination.y;
                        state.destination = next;
                        moved = true;
                    }
                }
            } else {
                let facing = facing16_between([coord.x, coord.y], [destination.x, destination.y]);
                [coord.x, coord.y] =
                    crate::util::native_trig::facing_step_world_xy([coord.x, coord.y], facing, 18);
                moved = true;
            }
            if moved && let Some(terrain) = self.resolved_terrain.as_ref() {
                let cells = NativeCellQuery::canonical(terrain);
                let old_z = coord.z;
                let ground =
                    query_ground_height(&cells, point(coord)).expect("native runner ground slope");
                coord.z = ground.wrapping_add(
                    if old_z > ground.wrapping_add(2 * GROUND_LEVEL_HEIGHT_LEPTONS) {
                        BRIDGE_DECK_HEIGHT_LEPTONS
                    } else {
                        0
                    },
                );
                if !state.fall.falling() {
                    let ground = query_ground_height(&cells, point(coord))
                        .expect("native runner ground slope");
                    if coord.z >= ground.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS) {
                        let cell = cells.lookup_world(coord.x, coord.y);
                        let position = cells.coord(cell);
                        if cells.flags(cell) & 0x100 == 0
                            && cells.flags(cells.lookup(neighbor(position, 6))) & 0x100 == 0
                            && cells.flags(cells.lookup(neighbor(position, 0))) & 0x100 == 0
                        {
                            state.fall.begin_fall();
                        }
                    }
                }
            }
        }
        let mut retire = false;
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            if begin_expiry {
                state.destination = None;
                expire(&mut state, &mut anim.runtime, config.running_frames, now);
            } else if !state.expired && config.running_frames != 0 {
                let offset = state.destination.map_or(0, |destination| {
                    let facing =
                        facing16_between([coord.x, coord.y], [destination.x, destination.y]);
                    i32::from(dir_from_facing16(facing).wrapping_neg().wrapping_sub(1) & 7)
                        .wrapping_mul(config.running_frames)
                });
                anim.runtime.current_frame = (now / 3) % config.running_frames + offset;
            } else if state.expired {
                // GetImage returns raw frames, including Shadow's second half.
                retire = anim.runtime.current_frame
                    >= config
                        .raw_shp_frame_count
                        .unwrap_or(anim.effective_end)
                        .wrapping_div(2)
                        .wrapping_sub(1);
            }
            anim.world_coord = coord;
        }
        if retire {
            self.destroy_anim(id, rules);
        }

        // ObjectAI5F3F11..5F413A. Anim's PerCell callback is the base no-op;
        // no parachute/bomb producers select this stock runner.
        if state.fall.falling() {
            coord.z = coord.z.wrapping_add(state.fall.rate());
            let ground = self
                .resolved_terrain
                .as_ref()
                .map(|terrain| {
                    query_ground_height(&NativeCellQuery::canonical(terrain), point(coord))
                        .expect("native falling runner ground slope")
                })
                .unwrap_or(0);
            let landed = coord.z <= ground;
            if landed {
                coord.z = ground;
                state.fall.finish();
            }
            state.fall.advance_rate(
                rules.general.parachute_max_fall_rate,
                rules.general.no_parachute_max_fall_rate,
            );
            if let Some(anim) = self.substrate.anims.get_mut(id) {
                anim.world_coord = coord;
                if landed {
                    expire(&mut state, &mut anim.runtime, config.running_frames, now);
                }
            }
            if landed
                && self.resolved_terrain.as_ref().is_some_and(|terrain| {
                    let cells = NativeCellQuery::canonical(terrain);
                    cells.land_type(cells.lookup_world(coord.x, coord.y)) == 2
                })
                && let Some(name) = rules.combat_damage.splash_list.first()
            {
                self.spawn_named_anim(
                    rules,
                    name,
                    AnimWorldCoord {
                        z: coord.z.wrapping_add(3),
                        ..coord
                    },
                    0,
                    0x600,
                    0,
                );
            }
        }
        if let Some(anim) = self.substrate.anims.get_mut(id) {
            anim.flaming_guy = Some(state);
        }
    }
}
