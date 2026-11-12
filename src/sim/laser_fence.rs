//! Laser fences: a `LaserFencePost=` building raises `LaserFence=` sections
//! toward the next post of its house in each straight direction and keeps
//! them on while both ends are online (`BuildingClass` `0x00452A40..
//! 0x00453820`).
//!
//! - Building+0x618 ([`LaserFenceFrame`]) is a section's draw frame
//!   (`BuildingClass::GetCurrentFrame @ 0x0043EF90` returns it): 0..7 while
//!   the section is on, 8 (east-west) or 12 (north-south) while it is off.
//!   A post keeps the connection bits ConnectWalls ORs into its own +0x618,
//!   which only ComputeCRC (`0x0045436D`) reads.
//! - Every post raises the first `LaserFence=` BuildingType in array order
//!   (`0x00452E2C..0x00452E52`).
//! - A post reaches `GuardRange=` cells (`Type+0x5B8 >> 8`, at least 1).
//! - A section lies along its body facing: `((raw >> 12) + 1) >> 1 & 3` is
//!   0 for north-south and 2 for east-west, compared with a link
//!   direction `& 3`.
//!
//! Readers of the frame live with their owners: Can_Enter_Cell
//! (`world::object_entry`), InfantryClass::AI's scatter (`world::object_turn`),
//! FootClass::PerCellProcess's burn ([`Simulation::laser_fence_burn_foot`]),
//! the bullet, bounce and particle collisions, the body frame
//! (`building_art`) and the fogged copy (`world::fog_objects`).
//!
//! No RNG draw of its own: a section's construction and the C4 damage
//! receivers draw as their owners do.
//!
//! RESIDUAL: ExtendWallInDirection's vein removal (`CellClass::RemoveVeinGrowth
//! @ 0x00486E30` on overlay 0x7E, `0x00452FFF..0x00453006`) is not ported;
//! no retail map places veins. The redraw byte each recalculated section
//! sets (`+0x80`, `0x004536EC`, `0x004537ED`) has no VERA counterpart: the
//! renderer reads the frame every draw.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::DriveCoord;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::MissionType;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::{PlacementEvidence, Simulation, UninitContext};
use crate::util::direction::DIRECTION_DELTAS;

/// The link directions a post walks, in ConnectWalls' and
/// RecalculateWallConnections' order: N, E, S, W.
const LINK_DIRECTIONS: [u8; 4] = [0, 2, 4, 6];
/// ConnectWalls' connection bit per link direction (`0x00818CA0`).
const CONNECTION_BITS: [i32; 4] = [1, 2, 4, 8];
/// An off section's frame: east-west 8, north-south 12 (`0x00440A57`,
/// `0x00440A63`, `0x0045357D..0x00453599`).
const OFF_EAST_WEST: i32 = 8;
const OFF_NORTH_SOUTH: i32 = 12;

/// Building+0x618, LaserFenceFrame. The constructor writes 0 (`0x0043B7E9`).
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub(crate) struct LaserFenceFrame(i32);

impl LaserFenceFrame {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) const fn with_frame(frame: i32) -> Self {
        Self(frame)
    }

    pub(crate) fn frame(self) -> i32 {
        self.0
    }

    pub(crate) fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// The `< 8` test of PerCellProcess (`0x004D8CC4`), the bullet, bounce
    /// and particle collisions and RecalculateWallConnections: a section
    /// that is on.
    pub(crate) fn is_on(self) -> bool {
        self.0 < OFF_EAST_WEST
    }

    /// The `== 12 || == 8` test of Can_Enter_Cell (Unit `0x0073F798`,
    /// Infantry `0x0051C4B0`), InfantryClass::AI (`0x0051BD3A`),
    /// RecalcZoneType (`0x00483DBA`) and the fogged copy (`0x004D10F5`).
    pub(crate) fn is_off_frame(self) -> bool {
        matches!(self.0, OFF_EAST_WEST | OFF_NORTH_SOUTH)
    }
}

/// A post's reach in cells: `GuardRange=` leptons `>> 8`, at least 1
/// (`0x00452DEA..0x00452DF8` and its copies).
fn post_range(object: &ObjectType) -> i32 {
    let leptons = object.guard_range.map_or(0, |range| range.to_bits() >> 8);
    (leptons >> 8).max(1)
}

/// The section axis of a building's body facing (FacingClass::Current at
/// `+0x388`): `((raw >> 12) + 1) >> 1 & 3`.
fn facing_axis(raw: u16) -> u8 {
    (((u32::from(raw) >> 12) + 1) >> 1 & 3) as u8
}

/// The cell of a building's GetCoords (vt+0x48, the foundation centre),
/// each axis divided toward zero as `CDQ; AND EDX,0xFF; ADD; SAR 8` does.
fn center_cell(entity: &GameEntity) -> (i16, i16) {
    let [x, y] = crate::sim::movement::ground_pose::object_center_xy(entity);
    ((x / 256) as i16, (y / 256) as i16)
}

/// One step through the direction table (`0x0089F688`), in CellStruct's
/// wrapping 16-bit arithmetic.
fn step((x, y): (i16, i16), dir: u8) -> (i16, i16) {
    let (dx, dy) = DIRECTION_DELTAS[usize::from(dir & 7)];
    (x.wrapping_add(dx as i16), y.wrapping_add(dy as i16))
}

impl Simulation {
    /// `Look_up_building_in_cell @ 0x0047C520` on the ground list of the
    /// cell MapClass::Get_CellClass returns; a cell off the map is the
    /// shared dummy, which holds none.
    fn laser_fence_building_at(&self, (x, y): (i16, i16)) -> Option<u64> {
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            return None;
        };
        self.substrate
            .occupancy
            .first_building_on_layer(x, y, MovementLayer::Ground)
    }

    fn laser_fence_type<'r>(&self, id: u64, rules: &'r RuleSet) -> Option<&'r ObjectType> {
        let entity = self.substrate.entities.get(id)?;
        if entity.category() != EntityCategory::Structure {
            return None;
        }
        self.object_type(entity.type_ref(), rules)
    }

    /// Whether `id` is a `LaserFence=` section of `owner` laid along `dir`.
    fn laser_fence_section_along(
        &self,
        id: u64,
        owner: crate::sim::intern::InternedId,
        dir: u8,
        rules: &RuleSet,
    ) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        self.laser_fence_type(id, rules)
            .is_some_and(|object| object.laser_fence)
            && entity.owner() == owner
            && facing_axis(entity.body_facing_current(self.session.binary_frame())) == dir & 3
    }

    /// RecalculateWallConnections' "online" test of a post (`0x004533F4..
    /// 0x0045342F`, repeated for the far post at `0x004535B8..0x004535F4`):
    /// Is_Operational_For_Output (vt+0x350), StuffEnabled (`+0x6EA`), and a
    /// Get_Mission (vt+0x184) other than Selling and Construction.
    fn laser_fence_post_online(&self, id: u64, rules: &RuleSet) -> bool {
        let Some(entity) = self.substrate.entities.get(id) else {
            return false;
        };
        self.building_operational_state(id, rules) == Some(true)
            && entity.building_art.stuff_enabled()
            && !matches!(
                entity.mission.effective().known(),
                Some(MissionType::Selling | MissionType::Construction)
            )
    }

    fn set_laser_fence_frame(&mut self, id: u64, frame: i32) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.laser_fence_frame = LaserFenceFrame(frame);
        }
    }

    /// `BuildingClass::FindNearestFencePost @ 0x00452BB0`: walking `range`
    /// cells from `post` along `dir` (a negative `range` takes a post's own
    /// reach, else 1), the last post of the same house seen. Without
    /// `through_sections` the first building ends the walk; with it the walk
    /// continues only through sections of that house laid along `dir`.
    fn laser_fence_find_post(
        &self,
        post: u64,
        dir: u8,
        through_sections: bool,
        range: i32,
        rules: &RuleSet,
    ) -> Option<u64> {
        let entity = self.substrate.entities.get(post)?;
        let object = self.laser_fence_type(post, rules)?;
        let range = if range >= 0 {
            range
        } else if object.laser_fence_post {
            post_range(object)
        } else {
            1
        };
        let owner = entity.owner();
        let mut cell = center_cell(entity);
        let mut found = None;
        for _ in 0..range {
            cell = step(cell, dir);
            let Some(building) = self.laser_fence_building_at(cell) else {
                continue;
            };
            let same_house = self
                .substrate
                .entities
                .get(building)
                .is_some_and(|other| other.owner() == owner);
            if same_house
                && self
                    .laser_fence_type(building, rules)
                    .is_some_and(|other| other.laser_fence_post)
            {
                found = Some(building);
            }
            if !through_sections || !self.laser_fence_section_along(building, owner, dir, rules) {
                return found;
            }
        }
        found
    }

    /// `BuildingClass::ExtendWallInDirection @ 0x00452DC0`: outside the
    /// placement scope, a post whose first building along `dir` within its
    /// reach is a post of its house that is not being sold raises a section
    /// in each cell up to it: the first `LaserFence=` type, constructed for
    /// the post's house and put at the cell centre (Z 0) facing
    /// `(dir & 3) << 5`. Reaching the far post, the ore of each cell from
    /// this post's up to it is reduced by 12 (`0x00452FD9..0x00453048`);
    /// a refused section is deleted and the run so far is UnInit
    /// (AdjustWallConnections(dir, 0), `0x00452F83`).
    pub(crate) fn laser_fence_extend(&mut self, post: u64, dir: u8, ctx: UninitContext<'_>) {
        if self.object_placement_scope_active() {
            return;
        }
        let rules = ctx.rules();
        let Some(object) = self
            .laser_fence_type(post, rules)
            .filter(|object| object.laser_fence_post)
        else {
            return;
        };
        let range = post_range(object);
        let Some(far) = self.laser_fence_find_post(post, dir, false, -1, rules) else {
            return;
        };
        if self
            .substrate
            .entities
            .get(far)
            .is_some_and(|far| far.mission.effective().known() == Some(MissionType::Selling))
        {
            return;
        }
        let Some(section_type) = rules.first_laser_fence_type() else {
            return;
        };
        let section_id = section_type.id.clone();
        let Some((owner, origin)) = self
            .substrate
            .entities
            .get(post)
            .map(|entity| (entity.owner(), center_cell(entity)))
        else {
            return;
        };
        let owner_name = self.interner.resolve(owner).to_owned();
        let facing = (dir & 3) << 5;
        let mut cell = origin;
        // Native loops until it meets the far post, which FindNearestFencePost
        // found within `range` cells with nothing in between.
        for _ in 0..range {
            cell = step(cell, dir);
            if self.laser_fence_building_at(cell) == Some(far) {
                self.laser_fence_clear_ore(origin, dir, far, range, ctx);
                return;
            }
            let placed = self.laser_fence_raise_section(&section_id, &owner_name, cell, facing, ctx);
            if !placed {
                break;
            }
        }
        self.laser_fence_adjust(post, dir, false, ctx);
    }

    /// One section of ExtendWallInDirection: CreateObject (`0x00452F09`) and
    /// Unlimbo at the cell centre (`0x00452F68`); a refused one is deleted
    /// (vt+0x20, `0x00452F80`).
    fn laser_fence_raise_section(
        &mut self,
        section_id: &str,
        owner_name: &str,
        (x, y): (i16, i16),
        facing: u8,
        ctx: UninitContext<'_>,
    ) -> bool {
        let rules = ctx.rules();
        let (Ok(rx), Ok(ry)) = (u16::try_from(x), u16::try_from(y)) else {
            return false;
        };
        let Some(id) = self.construct_object_limbo_at_height(
            section_id,
            owner_name,
            rx,
            ry,
            facing,
            0,
            rules,
            ctx.effects(),
        ) else {
            return false;
        };
        let coord = DriveCoord {
            x: i32::from(x) * 256 + 128,
            y: i32::from(y) * 256 + 128,
            z: 0,
        };
        let placed = self
            .reveal_constructed_object_at_coord_with_overlay_context(
                id,
                coord,
                facing,
                PlacementEvidence::EvaluateMark,
                rules,
                ctx.registry(),
                ctx.effects(),
            )
            .is_some();
        if !placed
            && self
                .substrate
                .entities
                .get(id)
                .is_some_and(|entity| entity.lifecycle.in_limbo())
        {
            self.discard_constructed_limbo(id, rules, ctx.effects());
        }
        placed
    }

    /// `0x00452F97..0x00453048`: Reduce_Tiberium(12) (`0x00480A80`) on each
    /// cell from the post's own up to the one before the far post.
    fn laser_fence_clear_ore(
        &mut self,
        origin: (i16, i16),
        dir: u8,
        far: u64,
        range: i32,
        ctx: UninitContext<'_>,
    ) {
        let rules = ctx.rules();
        let mut cell = origin;
        for _ in 0..range {
            if let (Ok(x), Ok(y)) = (u16::try_from(cell.0), u16::try_from(cell.1))
                && let Err(error) = Self::reduce_tiberium_at_with_native_context(
                    &self.terrain_objects,
                    &self.ore_growth_config,
                    &mut self.ore_growth_state,
                    &self.session,
                    &mut self.scenario_rng,
                    &self.substrate,
                    &mut self.resolved_terrain,
                    &mut self.overlay_grid,
                    &mut self.dirty_cells,
                    (x, y),
                    12,
                    rules,
                    ctx.registry(),
                )
            {
                log::error!("Laser fence ore clearing failed: {error}");
                return;
            }
            cell = step(cell, dir);
            if self.laser_fence_building_at(cell) == Some(far) {
                return;
            }
        }
    }

    /// `BuildingClass::AdjustWallConnections @ 0x00453060`: walking a post's
    /// reach along `dir`, every section of its house laid along `dir` until
    /// another building: with `destroy`, the C4 flash (`0x0048A620`) of the
    /// section's Health at its GetCoords and ReceiveDamage of that Health
    /// with Rules' C4Warhead (no attacker, IgnoreDefenses, `0x004531FE`);
    /// without, UnInit (vt+0xF8, `0x00453216`).
    pub(crate) fn laser_fence_adjust(
        &mut self,
        post: u64,
        dir: u8,
        destroy: bool,
        ctx: UninitContext<'_>,
    ) {
        let rules = ctx.rules();
        let Some(object) = self
            .laser_fence_type(post, rules)
            .filter(|object| object.laser_fence_post)
        else {
            return;
        };
        let range = post_range(object);
        let Some((owner, mut cell)) = self
            .substrate
            .entities
            .get(post)
            .map(|entity| (entity.owner(), center_cell(entity)))
        else {
            return;
        };
        for _ in 0..range {
            cell = step(cell, dir);
            let Some(section) = self.laser_fence_building_at(cell) else {
                continue;
            };
            if !self.laser_fence_section_along(section, owner, dir, rules) {
                return;
            }
            if destroy {
                self.laser_fence_explode_section(section, ctx);
            } else {
                self.uninit_with_context(section, ctx);
            }
        }
    }

    fn laser_fence_explode_section(&mut self, section: u64, ctx: UninitContext<'_>) {
        let rules = ctx.rules();
        let Some(entity) = self.substrate.entities.get(section) else {
            return;
        };
        let health = entity.health.current();
        let terrain = ctx.terrain().or(self.resolved_terrain.as_ref());
        let at = crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
        let c4 = self.interner.intern(&rules.bridge_warheads.c4_name);
        self.combat_light_requests
            .emit(crate::sim::combat::CombatLightRequest::Impact {
                target_id: None,
                damage: health,
                warhead_ref: c4,
                coord: crate::sim::projectile::ProjectileCoord::new(at.x, at.y, at.z),
                force_create: false,
                flags: 0,
            });
        self.commit_direct_damage_receiver(
            rules,
            ctx.registry(),
            crate::sim::combat::EntityDamageEvent::direct_receiver(
                section,
                health,
                0,
                crate::sim::combat::RAD_NO_ATTACKER,
                None,
                c4,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            ),
            ctx.effects(),
        );
    }

    /// `BuildingClass::ConnectWalls @ 0x00452A40`: outside the placement
    /// scope, for each link direction whose adjacent cell holds a section of
    /// the post's house laid along it, the post ORs that direction's bit
    /// into its frame and takes the run down (AdjustWallConnections, with
    /// `destroy`). Limbo passes 0 (`0x00445A25`), DestructionEffects 1
    /// (`0x004416FD`), ChangeOwner 0 (`0x004483BB`).
    pub(crate) fn laser_fence_connect_walls(
        &mut self,
        post: u64,
        destroy: bool,
        ctx: UninitContext<'_>,
    ) {
        if self.object_placement_scope_active() {
            return;
        }
        let rules = ctx.rules();
        if !self
            .laser_fence_type(post, rules)
            .is_some_and(|object| object.laser_fence_post)
        {
            return;
        }
        for (bit, dir) in CONNECTION_BITS.into_iter().zip(LINK_DIRECTIONS) {
            let Some((owner, origin, frame)) = self.substrate.entities.get(post).map(|entity| {
                (
                    entity.owner(),
                    center_cell(entity),
                    entity.laser_fence_frame.0,
                )
            }) else {
                return;
            };
            let Some(section) = self.laser_fence_building_at(step(origin, dir)) else {
                continue;
            };
            if !self.laser_fence_section_along(section, owner, dir, rules) {
                continue;
            }
            self.set_laser_fence_frame(post, frame | bit);
            self.laser_fence_adjust(post, dir, destroy, ctx);
        }
    }

    /// `BuildingClass::RecalculateWallConnections @ 0x004533A0` for a live
    /// post out of limbo: in each link direction whose adjacent cell holds a
    /// section of its house laid along it, and unless `force`, only when that
    /// first section's on/off state differs from the post's
    /// ([`Self::laser_fence_post_online`]), the run is rewritten. While the
    /// post and the far post (FindNearestFencePost through sections) are both
    /// online it is on: frames 5/4/6 north, 1/0/2 east, 6/4/5 south, 2/0/1
    /// west (first/middle/last, jump table `0x00453820`), a lone section 7
    /// (north-south) or 3 (east-west), and each section burns the Foot in
    /// its cell; otherwise it is off (8 east-west, 12 north-south). The walk
    /// covers the post's reach and stops at the first cell without a section
    /// of the run.
    pub(crate) fn laser_fence_recalculate(&mut self, post: u64, force: bool, ctx: UninitContext<'_>) {
        let rules = ctx.rules();
        let Some(object) = self
            .laser_fence_type(post, rules)
            .filter(|object| object.laser_fence_post)
        else {
            return;
        };
        if !self
            .substrate
            .entities
            .get(post)
            .is_some_and(|entity| entity.is_object_alive() && !entity.in_limbo())
        {
            return;
        }
        let range = post_range(object);
        let online = self.laser_fence_post_online(post, rules);
        for dir in LINK_DIRECTIONS {
            let Some((owner, origin)) = self
                .substrate
                .entities
                .get(post)
                .map(|entity| (entity.owner(), center_cell(entity)))
            else {
                return;
            };
            let mut cell = step(origin, dir);
            let Some(first) = self.laser_fence_building_at(cell) else {
                continue;
            };
            if !self.laser_fence_section_along(first, owner, dir, rules) {
                continue;
            }
            let first_on = self
                .substrate
                .entities
                .get(first)
                .is_some_and(|entity| entity.laser_fence_frame.is_on());
            if !force && first_on == online {
                continue;
            }
            let far = if online {
                self.laser_fence_find_post(post, dir, true, -1, rules)
            } else {
                None
            };
            let off = if dir & 3 == 2 {
                OFF_EAST_WEST
            } else {
                OFF_NORTH_SOUTH
            };
            let both_on = online && far.is_some_and(|far| self.laser_fence_post_online(far, rules));
            let [first_frame, middle_frame, last_frame] = match dir {
                0 if both_on => [5, 4, 6],
                2 if both_on => [1, 0, 2],
                4 if both_on => [6, 4, 5],
                6 if both_on => [2, 0, 1],
                _ => [off; 3],
            };
            let mut last = first;
            let mut building = Some(first);
            for _ in 0..range {
                let Some(section) = building else {
                    break;
                };
                if !self.laser_fence_section_along(section, owner, dir, rules) {
                    break;
                }
                last = section;
                let frame = if section == first {
                    first_frame
                } else {
                    middle_frame
                };
                self.set_laser_fence_frame(section, frame);
                if both_on {
                    self.laser_fence_burn_cell(cell, section, ctx);
                }
                cell = step(cell, dir);
                building = self.laser_fence_building_at(cell);
            }
            if both_on {
                let frame = if last != first {
                    last_frame
                } else if dir & 3 == 2 {
                    3
                } else {
                    7
                };
                self.set_laser_fence_frame(last, frame);
            }
        }
    }

    /// RecalculateWallConnections' burn (`0x004536F7..0x0045375C`): each
    /// Unit, Aircraft or Infantry with Health above zero in the cell's
    /// ground list takes its Health as ReceiveDamage with Rules' C4Warhead,
    /// the section as attacker, IgnoreDefenses.
    fn laser_fence_burn_cell(&mut self, (x, y): (i16, i16), section: u64, ctx: UninitContext<'_>) {
        let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
            return;
        };
        // Native reads each object's next pointer before its ReceiveDamage.
        let occupants: Vec<u64> = self
            .substrate
            .occupancy
            .get(x, y)
            .map(|list| {
                list.iter_layer(MovementLayer::Ground)
                    .map(|entry| entry.entity_id)
                    .collect()
            })
            .unwrap_or_default();
        for victim in occupants {
            self.laser_fence_c4(victim, section, ctx);
        }
    }

    /// The C4 burn RecalculateWallConnections and PerCellProcess share: a
    /// Unit, Aircraft or Infantry (WhatAmI 1, 2, 0xF) with Health above
    /// zero takes its Health with Rules' C4Warhead, `section` attacking,
    /// IgnoreDefenses and no house.
    fn laser_fence_c4(&mut self, victim: u64, section: u64, ctx: UninitContext<'_>) {
        let rules = ctx.rules();
        let Some(health) = self
            .substrate
            .entities
            .get(victim)
            .filter(|entity| {
                matches!(
                    entity.category(),
                    EntityCategory::Unit | EntityCategory::Aircraft | EntityCategory::Infantry
                )
            })
            .map(|entity| entity.health.current())
            .filter(|&health| health > 0)
        else {
            return;
        };
        let c4 = self.interner.intern(&rules.bridge_warheads.c4_name);
        self.commit_direct_damage_receiver(
            rules,
            ctx.registry(),
            crate::sim::combat::EntityDamageEvent::direct_receiver(
                victim,
                health,
                0,
                section,
                None,
                c4,
                crate::sim::combat::ReceiverCallFlags {
                    ignore_defenses: true,
                    arg6: true,
                },
            ),
            ctx.effects(),
        );
    }

    /// `FootClass::PerCellProcess @ 0x004D8CA5..0x004D8D0B`: a Foot entering
    /// a cell whose first building is an on section takes the C4 burn.
    pub(crate) fn laser_fence_burn_foot(&mut self, foot: u64, ctx: UninitContext<'_>) {
        let rules = ctx.rules();
        let Some(cell) = self
            .substrate
            .entities
            .get(foot)
            .map(|entity| {
                let at = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
                ((at.x / 256) as i16, (at.y / 256) as i16)
            })
        else {
            return;
        };
        let Some(section) = self.laser_fence_building_at(cell) else {
            return;
        };
        let on = self
            .substrate
            .entities
            .get(section)
            .is_some_and(|entity| entity.laser_fence_frame.is_on());
        if on
            && self
                .laser_fence_type(section, rules)
                .is_some_and(|object| object.laser_fence)
        {
            self.laser_fence_c4(foot, section, ctx);
        }
    }

    /// The post link of Unlimbo (`0x00440E02..0x00440E57`), ChangeOwner
    /// (`0x00448CFB..0x00448D48`) and the scenario pass (`0x00452D40`): a
    /// live post out of limbo, outside the placement scope, extends N, E, S
    /// and W and recalculates with force.
    pub(crate) fn laser_fence_link_post(&mut self, post: u64, ctx: UninitContext<'_>) {
        if self.object_placement_scope_active()
            || !self
                .laser_fence_type(post, ctx.rules())
                .is_some_and(|object| object.laser_fence_post)
            || !self
                .substrate
                .entities
                .get(post)
                .is_some_and(|entity| entity.is_object_alive() && !entity.in_limbo())
        {
            return;
        }
        for dir in LINK_DIRECTIONS {
            self.laser_fence_extend(post, dir, ctx);
        }
        self.laser_fence_recalculate(post, true, ctx);
    }

    /// `FUN_00452D40`, called by `InitializeScenarioFromINI` with the
    /// placement scope cleared (`0x00687C2B..0x00687C44`): every post of
    /// BuildingClass::Array links, sections it raises included in the walk.
    pub(crate) fn laser_fence_link_scenario_posts(&mut self, ctx: UninitContext<'_>) {
        self.without_object_placement_scope(|sim| {
            let mut index = 0;
            while let Some(id) = sim.substrate.entities.building_registry_at(index) {
                sim.laser_fence_link_post(id, ctx);
                index += 1;
            }
        });
    }

    /// `BuildingClass::Unlimbo @ 0x00440A32..0x00440A63`: a `LaserFence=`
    /// building starts off, east-west (8) when its Unlimbo direction's
    /// `((dir << 8 >> 12) + 1) & 6` is 4, else north-south (12).
    pub(crate) fn laser_fence_section_unlimbo_frame(
        &mut self,
        id: u64,
        facing: u8,
        rules: &RuleSet,
    ) {
        if !self
            .laser_fence_type(id, rules)
            .is_some_and(|object| object.laser_fence)
        {
            return;
        }
        let frame = if ((u32::from(facing) << 8 >> 12) + 1) & 6 == 4 {
            OFF_EAST_WEST
        } else {
            OFF_NORTH_SOUTH
        };
        self.set_laser_fence_frame(id, frame);
    }

    /// A post's frame cleared: Unlimbo's admitted post (`0x00440A83`),
    /// Limbo outside the placement scope (`0x00445E32`) and ChangeOwner
    /// outside it (`0x00448D65`).
    pub(crate) fn laser_fence_clear_post_frame(&mut self, post: u64, rules: &RuleSet) {
        if self
            .laser_fence_type(post, rules)
            .is_some_and(|object| object.laser_fence_post)
        {
            self.set_laser_fence_frame(post, 0);
        }
    }

    /// `BuildingClass::RemoveLaserFenceAtCell @ 0x00453240` (the Gate arm of
    /// Unlimbo, `0x0044075F`): from a section at `cell`, the walk along its
    /// axis (N or E) past further sections; reaching a post, that post UnInits
    /// the run back toward the cell (AdjustWallConnections(axis | 4, 0));
    /// otherwise the section at `cell` alone is UnInit.
    pub(crate) fn laser_fence_remove_at_cell(&mut self, cell: (i16, i16), ctx: UninitContext<'_>) {
        let rules = ctx.rules();
        let Some(fence) = self.laser_fence_building_at(cell) else {
            return;
        };
        if !self
            .laser_fence_type(fence, rules)
            .is_some_and(|object| object.laser_fence)
        {
            return;
        }
        let Some(axis) = self
            .substrate
            .entities
            .get(fence)
            .map(|entity| facing_axis(entity.body_facing_current(self.session.binary_frame())))
        else {
            return;
        };
        let mut at = cell;
        let post = loop {
            at = step(at, axis);
            let Some(building) = self.laser_fence_building_at(at) else {
                break None;
            };
            let Some(object) = self.laser_fence_type(building, rules) else {
                break None;
            };
            if object.laser_fence_post {
                break Some(building);
            }
            if !object.laser_fence {
                break None;
            }
        };
        match post {
            Some(post) => self.laser_fence_adjust(post, axis | 4, false, ctx),
            None => self.uninit_with_context(fence, ctx),
        }
    }
}

#[cfg(test)]
#[path = "laser_fence_tests.rs"]
mod tests;
