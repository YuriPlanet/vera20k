//! BuildingClass laser fence placement and frame updates.
//!
//! Native sections are Building objects. Their +618 value is both their
//! art frame and the input to the movement and projectile receivers.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{CombatLightRequest, EntityDamageEvent, ReceiverCallFlags};
use crate::sim::game_entity::GameEntity;
use crate::sim::intern::InternedId;
use crate::sim::mission::MissionType;
use crate::sim::movement::{ground_pose::object_center_xy, locomotor::MovementLayer};
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::{FrameEffects, PlacementEvidence, Simulation, UninitContext};

#[derive(
    Clone, Copy, Debug, Default, Eq, Hash, PartialEq, serde::Serialize, serde::Deserialize,
)]
pub(crate) struct LaserFenceFrame(u8);

impl GameEntity {
    pub(crate) fn laser_fence_frame(&self) -> u8 {
        self.building.laser_fence_frame.0
    }

    pub(crate) fn laser_fence_open(&self) -> bool {
        matches!(self.laser_fence_frame(), 8 | 12)
    }
}

// The even indices of the native neighbor table 0x89F688. The frame
// triples at 0x45360B..0x453655 are (near, middle, far); the fourth
// entry is the one-piece frame at 0x4537CB/0x4537D7.
const DIRECTIONS: [(i32, i32, u8, [u8; 4]); 4] = [
    (0, -1, 0, [5, 4, 6, 7]),
    (1, 0, 2, [1, 0, 2, 3]),
    (0, 1, 0, [6, 4, 5, 7]),
    (-1, 0, 2, [2, 0, 1, 3]),
];

fn fence_axis(facing: u16) -> u8 {
    ((((u32::from(facing) >> 12) + 1) >> 1) & 3) as u8
}

struct FencePost {
    center: [i32; 2],
    owner: InternedId,
    range: i32,
}

impl FencePost {
    fn cell(&self, dx: i32, dy: i32, distance: i32) -> Option<(u16, u16)> {
        Some((
            u16::try_from(self.center[0] + dx * distance).ok()?,
            u16::try_from(self.center[1] + dy * distance).ok()?,
        ))
    }
}

impl Simulation {
    /// Ordinary Building Unlimbo starts a section in its open art frame
    /// at 0x440A32..0x440A63, before Mark(PUT) publishes its occupation.
    pub(crate) fn initialize_laser_fence_frame(
        &mut self,
        id: u64,
        facing: Option<u8>,
        rules: &RuleSet,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.category() != EntityCategory::Structure
            || !self
                .object_type(entity.type_ref(), rules)
                .is_some_and(|ty| ty.laser_fence)
        {
            return;
        }
        let direction = facing.map_or_else(
            || entity.body_facing_current(self.session.binary_frame()),
            |facing| u16::from(facing) << 8,
        );
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.building.laser_fence_frame.0 = if fence_axis(direction) == 2 { 8 } else { 12 };
        }
    }

    fn laser_fence_post(&self, id: u64, rules: &RuleSet) -> Option<FencePost> {
        let entity = self.substrate.entities.get(id)?;
        if entity.category() != EntityCategory::Structure || entity.lifecycle.in_limbo() {
            return None;
        }
        let object = self.object_type(entity.type_ref(), rules)?;
        if !object.laser_fence_post {
            return None;
        }
        Some(FencePost {
            center: object_center_xy(entity).map(|leptons| leptons / 256),
            owner: entity.owner(),
            // 0x452DEA and 0x4533DA shift TechnoType+5B8 by eight.
            range: object.guard_range.map_or(1, |range| {
                (range.to_bits() >> 16).clamp(1, crate::map::cell_index::CELL_ROW_STRIDE as i32)
            }),
        })
    }

    fn laser_fence_post_powered(&self, id: u64, rules: &RuleSet) -> bool {
        self.substrate.entities.get(id).is_some_and(|post| {
            post.lifecycle.object_alive()
                && post.building_art.stuff_enabled()
                && !matches!(
                    post.mission.effective().known(),
                    Some(MissionType::Construction | MissionType::Selling)
                )
                && self.building_operational_state(id, rules) == Some(true)
        })
    }

    fn laser_fence_section_on_axis(
        &self,
        id: u64,
        owner: InternedId,
        axis: u8,
        rules: &RuleSet,
    ) -> bool {
        self.substrate.entities.get(id).is_some_and(|section| {
            section.owner() == owner
                && self
                    .object_type(section.type_ref(), rules)
                    .is_some_and(|object| object.laser_fence)
                && fence_axis(section.body_facing_current(self.session.binary_frame())) == axis
        })
    }

    /// BuildingClass::Put 0x440E33..0x440E57, also reached on capture and by
    /// the scenario pass 0x452D40. 0x452BB0 stops at the first Building
    /// during placement, and 0x452DC0 creates the first LaserFence type.
    pub(crate) fn place_laser_fence_at_post(
        &mut self,
        id: u64,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        if self.object_placement_scope_active() {
            return;
        }
        let Some(post) = self.laser_fence_post(id, rules) else {
            return;
        };
        let Some(section_type) = rules
            .all_objects()
            .find(|object| object.category == ObjectCategory::Building && object.laser_fence)
        else {
            return;
        };
        // A dual post/section type would recursively enter this creation
        // path during its own Unlimbo. A section must be distinct from a post.
        if section_type.laser_fence_post {
            return;
        }
        'direction: for (dx, dy, axis, _) in DIRECTIONS {
            let endpoint = (1..=post.range)
                .map_while(|distance| post.cell(dx, dy, distance).map(|cell| (distance, cell)))
                .find_map(|(distance, (x, y))| {
                    self.substrate
                        .occupancy
                        .first_building_on_layer(x, y, MovementLayer::Ground)
                        .map(|target| (distance, target))
                });
            let Some((distance, target)) = endpoint else {
                continue;
            };
            if !self
                .laser_fence_post(target, rules)
                .is_some_and(|other| other.owner == post.owner)
                || self.substrate.entities.get(target).is_some_and(|entity| {
                    entity.mission.effective().known() == Some(MissionType::Selling)
                })
            {
                continue;
            }
            for (x, y) in (1..distance).map_while(|step| post.cell(dx, dy, step)) {
                let Some(z) = self.terrain_cell_level(x, y) else {
                    self.remove_laser_fence_in_direction(
                        &post,
                        (dx, dy, axis),
                        false,
                        rules,
                        effects,
                    );
                    continue 'direction;
                };
                let facing = axis * 32;
                let Some(section) = self.construct_resolved_object_limbo_at_height(
                    section_type,
                    post.owner,
                    x,
                    y,
                    facing,
                    z,
                    rules,
                    effects,
                ) else {
                    self.remove_laser_fence_in_direction(
                        &post,
                        (dx, dy, axis),
                        false,
                        rules,
                        effects,
                    );
                    continue 'direction;
                };
                if self
                    .reveal_constructed_object_at_height(
                        section,
                        x,
                        y,
                        facing,
                        z,
                        PlacementEvidence::EvaluateMark,
                        rules,
                        effects,
                    )
                    .is_none()
                {
                    self.uninit_with_context(
                        section,
                        UninitContext::new(rules).with_effects(effects),
                    );
                    self.remove_laser_fence_in_direction(
                        &post,
                        (dx, dy, axis),
                        false,
                        rules,
                        effects,
                    );
                    continue 'direction;
                }
            }
            // 0x452FD9..0x453048 reduces every tiberium cell of a completed
            // span by 12, from the placing post up to the far post.
            for cell @ (x, y) in (0..distance).map_while(|step| post.cell(dx, dy, step)) {
                if !self.overlay_grid.as_ref().is_some_and(|grid| {
                    x < grid.width() && y < grid.height() && grid.cell(x, y).overlay_id.is_some()
                }) {
                    continue;
                }
                let outcome = Self::reduce_tiberium_at_with_native_context(
                    &self.terrain_objects,
                    &self.ore_growth_config,
                    &mut self.ore_growth_state,
                    &self.session,
                    &mut self.scenario_rng,
                    &self.substrate,
                    &mut self.resolved_terrain,
                    &mut self.overlay_grid,
                    &mut self.dirty_cells,
                    cell,
                    12,
                    rules,
                    Some(rules.overlay_registry()),
                );
                if outcome.is_ok_and(|result| result.fully_removed) {
                    self.publish_recalculated_cells(rules, &[cell]);
                }
            }
        }
        self.recalculate_laser_fence(id, true, rules, effects);
    }

    pub(crate) fn place_scenario_laser_fences(
        &mut self,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        if !rules.all_objects().any(|object| object.laser_fence_post) {
            return;
        }
        for index in 0..self.substrate.entities.building_registry_len() {
            if let Some(id) = self.substrate.entities.building_registry_at(index) {
                self.place_laser_fence_at_post(id, rules, effects);
            }
        }
    }

    /// 0x452A40 calls 0x453060 for a post's destruction, first Limbo or
    /// old-owner capture side. A missing cell does not stop teardown.
    pub(crate) fn remove_laser_fence_at_post(
        &mut self,
        id: u64,
        destroyed: bool,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        if self.object_placement_scope_active() {
            return;
        }
        let Some(post) = self.laser_fence_post(id, rules) else {
            return;
        };
        for (dx, dy, axis, _) in DIRECTIONS {
            let connected = post
                .cell(dx, dy, 1)
                .and_then(|(x, y)| {
                    self.substrate
                        .occupancy
                        .first_building_on_layer(x, y, MovementLayer::Ground)
                })
                .is_some_and(|section| {
                    self.laser_fence_section_on_axis(section, post.owner, axis, rules)
                });
            if connected {
                self.remove_laser_fence_in_direction(
                    &post,
                    (dx, dy, axis),
                    destroyed,
                    rules,
                    effects,
                );
            }
        }
    }

    fn remove_laser_fence_in_direction(
        &mut self,
        post: &FencePost,
        (dx, dy, axis): (i32, i32, u8),
        destroyed: bool,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        for (x, y) in (1..=post.range).map_while(|step| post.cell(dx, dy, step)) {
            let Some(section) =
                self.substrate
                    .occupancy
                    .first_building_on_layer(x, y, MovementLayer::Ground)
            else {
                continue;
            };
            if !self.laser_fence_section_on_axis(section, post.owner, axis, rules) {
                break;
            }
            if destroyed {
                // 0x45319C..0x4531FE: a destroyed post's teardown flashes
                // each section and passes its Health to the C4 receiver.
                if let Some(entity) = self.substrate.entities.get(section)
                    && let Some(warhead) = rules.warhead(&rules.bridge_warheads.c4_name)
                {
                    let coord = crate::sim::movement::ground_pose::object_get_coords(
                        entity,
                        self.resolved_terrain.as_ref(),
                    );
                    self.combat_light_requests.emit(CombatLightRequest::Impact {
                        target_id: None,
                        coord: crate::sim::projectile::ProjectileCoord::new(
                            coord.x, coord.y, coord.z,
                        ),
                        warhead_ref: self.interner.intern(&warhead.id),
                        damage: entity.health.current(),
                        force_create: false,
                        flags: 0,
                    });
                    self.receive_own_health_c4(
                        section,
                        rules,
                        Some(rules.overlay_registry()),
                        effects,
                    );
                }
            } else {
                self.uninit_with_context(section, UninitContext::new(rules).with_effects(effects));
            }
        }
    }

    /// The Gate Put arm at 0x44075F dispatches 0x453240 for an owned
    /// section. Follow its encoded direction to a post, then remove the
    /// post's run toward the Gate through the existing 0x453060 owner.
    pub(crate) fn remove_laser_fence_under_gate(
        &mut self,
        (x, y): (u16, u16),
        owner: InternedId,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some(section) =
            self.substrate
                .occupancy
                .first_building_on_layer(x, y, MovementLayer::Ground)
        else {
            return;
        };
        let Some(entity) = self.substrate.entities.get(section) else {
            return;
        };
        if entity.owner() != owner
            || !self
                .object_type(entity.type_ref(), rules)
                .is_some_and(|ty| ty.laser_fence)
        {
            return;
        }
        let axis = fence_axis(entity.body_facing_current(self.session.binary_frame()));
        let (dx, dy) = [(0, -1), (1, -1), (1, 0), (1, 1)][usize::from(axis)];
        for distance in 1..=crate::map::cell_index::CELL_ROW_STRIDE as i32 {
            let (Ok(rx), Ok(ry)) = (
                u16::try_from(i32::from(x) + dx * distance),
                u16::try_from(i32::from(y) + dy * distance),
            ) else {
                break;
            };
            let Some(next) =
                self.substrate
                    .occupancy
                    .first_building_on_layer(rx, ry, MovementLayer::Ground)
            else {
                break;
            };
            if let Some(post) = self.laser_fence_post(next, rules) {
                self.remove_laser_fence_in_direction(
                    &post,
                    (-dx, -dy, axis),
                    false,
                    rules,
                    effects,
                );
                return;
            }
            if !self
                .substrate
                .entities
                .get(next)
                .and_then(|entity| self.object_type(entity.type_ref(), rules))
                .is_some_and(|ty| ty.laser_fence)
            {
                break;
            }
        }
        self.uninit_with_context(section, UninitContext::new(rules).with_effects(effects));
    }

    /// BuildingClass::UpdateLaserFence 0x4533A0. Ares 3.0p1's 0x43FB6D
    /// hook (0x100103C0 -> 0x10010350) adds force=true on the operational
    /// edge. EnableStuff 0x452441 and DisableStuff 0x452503 pass false,
    /// using the just-written +6EA and the real operational predicate.
    pub(crate) fn recalculate_laser_fence(
        &mut self,
        id: u64,
        force: bool,
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        let Some(post) = self.laser_fence_post(id, rules) else {
            return;
        };
        let powered = self.laser_fence_post_powered(id, rules);
        let mut changed_cells = Vec::new();
        for (dx, dy, axis, frames) in DIRECTIONS {
            let mut sections = Vec::new();
            let mut endpoint = None;
            let mut contiguous = true;
            for cell @ (x, y) in (1..=post.range).map_while(|step| post.cell(dx, dy, step)) {
                let Some(found) =
                    self.substrate
                        .occupancy
                        .first_building_on_layer(x, y, MovementLayer::Ground)
                else {
                    // Find the remote power source through gaps (0x452BB0
                    // with pass-sections), but only write the uninterrupted
                    // run adjacent to this post (0x4537A6).
                    contiguous = false;
                    continue;
                };
                if !self.laser_fence_section_on_axis(found, post.owner, axis, rules) {
                    endpoint = Some(found);
                    break;
                }
                if contiguous {
                    sections.push((found, cell));
                }
            }
            let Some(&(first, _)) = sections.first() else {
                continue;
            };
            if !force
                && self
                    .substrate
                    .entities
                    .get(first)
                    .is_some_and(|entity| entity.laser_fence_open() != powered)
            {
                continue;
            }
            let active = powered
                && endpoint.is_some_and(|other| {
                    self.laser_fence_post(other, rules)
                        .is_some_and(|other| other.owner == post.owner)
                        && self.laser_fence_post_powered(other, rules)
                });
            let length = sections.len();
            for (index, (section, cell)) in sections.into_iter().enumerate() {
                let frame = if !active {
                    if axis == 2 { 8 } else { 12 }
                } else if length == 1 {
                    frames[3]
                } else if index == 0 {
                    frames[0]
                } else if index + 1 == length {
                    frames[2]
                } else {
                    frames[1]
                };
                if let Some(entity) = self.substrate.entities.get_mut(section)
                    && entity.laser_fence_frame() != frame
                {
                    entity.building.laser_fence_frame.0 = frame;
                    changed_cells.push(cell);
                }
                if active {
                    self.laser_fence_kill_occupants(section, cell, rules, effects);
                }
            }
        }
        self.publish_recalculated_cells(rules, &changed_cells);
    }

    /// 0x4536F7..0x45375C walks Cell+E4 and passes the fence section as
    /// source, C4Warhead, ignore defenses, arg6=true and a null SourceHouse.
    fn laser_fence_kill_occupants(
        &mut self,
        section: u64,
        cell: (u16, u16),
        rules: &RuleSet,
        effects: &mut FrameEffects<'_>,
    ) {
        let victims = self
            .cell_objects(cell, MovementLayer::Ground)
            .filter_map(|entry| {
                let CellObjectMember::Entity(id) = entry else {
                    return None;
                };
                let actor = self.substrate.entities.get(id)?;
                actor.category().is_foot().then_some(id)
            })
            .collect::<Vec<_>>();
        if victims.is_empty() {
            return;
        }
        let warhead = self.interner.intern(&rules.bridge_warheads.c4_name);
        for target in victims {
            if let Some(health) = self
                .substrate
                .entities
                .get(target)
                .map(|entity| entity.health.current())
                && health > 0
            {
                self.commit_direct_damage_receiver(
                    rules,
                    Some(rules.overlay_registry()),
                    EntityDamageEvent::direct_receiver(
                        target,
                        health,
                        0,
                        section,
                        None,
                        warhead,
                        ReceiverCallFlags {
                            ignore_defenses: true,
                            arg6: true,
                        },
                    ),
                    effects,
                );
            }
        }
    }
}
