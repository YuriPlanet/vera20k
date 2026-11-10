//! A visceroid's own AI: `UnitClass::UpdateVisceroid @ 0x00737180`, which
//! `UnitClass::AI @ 0x007360C0` calls for a `SmallVisceroid=` or
//! `LargeVisceroid=` type (`0x0073666B..0x00736685`), after Fire_At_Target
//! and Facing_Update and before its Reload (vt+0x424).
//!
//! - Idle frames: on a Stage value of 90 or more, a value `% 5 == 4` restarts
//!   the Stage at `RandomRanged(0, 2) * 30` and the update ends there, as it
//!   does for any other value; below 90, a value `% 30 == 29` takes a new
//!   `RandomRanged(0, 2) * 30` and the Stage restarts at rate 1 either way
//!   (`0x00737198..0x00737255`). Both draws are Scenario's.
//! - A moving one (locomotor vt+0x10) does nothing more.
//! - A small one looks at the first ground object of each neighbour cell, N
//!   first (`0x0089F688`): the first small visceroid Unit it finds is sent to
//!   it (vt+0x480(this, 1)) unless that one has a NavCom (`+0x5A4`) or a
//!   TarCom (`+0x2B4`), when nothing happens; then its wander direction
//!   resets (`0x00737280..0x0073736C`).
//! - With neither NavCom nor TarCom of its own it wanders: it keeps its
//!   direction (`+0x6D4`) on a `RandomRanged(0, 2)` other than 0, else (or
//!   with none) stores `RandomRanged(0, 7)`; it heads for the neighbour cell
//!   there when its Can_Enter_Cell (vt+0x1AC) admits it, and otherwise the
//!   direction resets (`0x0073730E..0x00737419`).
//!
//! gamemd has no merge: two small visceroids come together and stay small.
//! Ares merges them ([`Simulation::ares_visceroid_merge`], hook `0x00739F21`).

use crate::map::entities::EntityCategory;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::movement::infantry_entry::InfantryEntryArgs;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::world::{FrameEffects, Simulation};
use crate::util::direction::DIRECTION_DELTAS;

/// UnitClass `+0x6D4` `IdleWanderDirection`: the direction a visceroid's
/// wander last took, -1 for none (constructor `0x00735410`). Only
/// [`Simulation::unit_update_visceroid`] writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) struct IdleWander {
    direction: i32,
}

impl Default for IdleWander {
    fn default() -> Self {
        Self { direction: -1 }
    }
}

impl IdleWander {
    pub(crate) fn hash_state(&self, hasher: &mut impl std::hash::Hasher) {
        std::hash::Hash::hash(&self.direction, hasher);
    }
}

impl Simulation {
    /// `UnitClass::UpdateVisceroid @ 0x00737180` for the Unit `id`; see the
    /// module doc.
    pub(crate) fn unit_update_visceroid(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) {
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        let Some(object) = self.object_type(entity.type_ref(), rules) else {
            return;
        };
        if entity.category() != EntityCategory::Unit
            || !(object.small_visceroid || object.large_visceroid)
        {
            return;
        }
        let small = object.small_visceroid;
        let now = self.session.binary_frame() as i32;
        let value = entity.native_stage().value();
        if value >= 90 {
            if value % 5 == 4 {
                let value = self.scenario_rng.next_range_i32_inclusive(0, 2) * 30;
                if let Some(entity) = self.substrate.entities.get_mut(id) {
                    entity.restart_native_stage(value, now, 1);
                }
            }
            return;
        }
        let value = if value % 30 == 29 {
            self.scenario_rng.next_range_i32_inclusive(0, 2) * 30
        } else {
            value
        };
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return;
        };
        entity.restart_native_stage(value, now, 1);
        if crate::sim::movement::motion_query::is_moving(entity) != Some(false) {
            return;
        }
        let cell = (
            i32::from(entity.position.rx()),
            i32::from(entity.position.ry()),
        );
        if small {
            for (dx, dy) in DIRECTION_DELTAS {
                let Some(other) = self.first_ground_unit(cell.0 + dx, cell.1 + dy) else {
                    continue;
                };
                let Some(other_entity) = self.substrate.entities.get(other) else {
                    continue;
                };
                if !self
                    .object_type(other_entity.type_ref(), rules)
                    .is_some_and(|object| object.small_visceroid)
                {
                    continue;
                }
                if other_entity.navigation.nav_com().is_some()
                    || other_entity.attack_target.target().is_some()
                {
                    return;
                }
                let _ = self.assign_destination_represented(
                    other,
                    Some(NavTargetRef::object(id)),
                    rules,
                    registry,
                    effects,
                );
                self.set_idle_wander(id, -1);
                return;
            }
        }
        let Some(entity) = self.substrate.entities.get(id) else {
            return;
        };
        if entity.navigation.nav_com().is_some() || entity.attack_target.target().is_some() {
            return;
        }
        let kept = entity.idle_wander.direction;
        let direction = if kept == -1 || self.scenario_rng.next_range_i32_inclusive(0, 2) == 0 {
            let direction = self.scenario_rng.next_range_i32_inclusive(0, 7);
            self.set_idle_wander(id, direction);
            direction
        } else {
            kept
        };
        let (dx, dy) = DIRECTION_DELTAS[(direction & 7) as usize];
        let target = (cell.0 + dx, cell.1 + dy);
        if self.wander_cell_admits(id, cell, target, direction, rules, registry)
            && let (Ok(rx), Ok(ry)) = (u16::try_from(target.0), u16::try_from(target.1))
        {
            let _ = self.assign_destination_represented(
                id,
                Some(NavTargetRef::cell(rx, ry)),
                rules,
                registry,
                effects,
            );
            return;
        }
        self.set_idle_wander(id, -1);
    }

    /// The first ground object of a cell (`CellClass+0xE4`), when it is a
    /// Unit (What_Am_I 1).
    fn first_ground_unit(&self, x: i32, y: i32) -> Option<u64> {
        let cell = (u16::try_from(x).ok()?, u16::try_from(y).ok()?);
        match self.first_ground_cell_object(cell)? {
            CellObjectMember::Entity(id) => self
                .substrate
                .entities
                .get(id)
                .filter(|entity| entity.category() == EntityCategory::Unit)
                .map(|_| id),
            _ => None,
        }
    }

    /// The wander's Can_Enter_Cell (vt+0x1AC) for `target` in `direction`,
    /// at the visceroid's effective cell level (`0x005F5F00`) from its own
    /// cell: code 0 admits.
    fn wander_cell_admits(
        &self,
        id: u64,
        cell: (i32, i32),
        target: (i32, i32),
        direction: i32,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) -> bool {
        let (Some(terrain), Some(entity)) = (
            self.resolved_terrain.as_ref(),
            self.substrate.entities.get(id),
        ) else {
            return false;
        };
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(terrain);
        let narrow = |(x, y): (i32, i32)| (x as i16, y as i16);
        let coord = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let args = InfantryEntryArgs {
            direction,
            height: crate::sim::movement::ground_pose::query_object_cell_height(
                &cells,
                coord,
                entity.on_bridge(),
            ),
            previous_cell: Some(cells.lookup(narrow(cell))),
        };
        self.foot_can_enter(id, cells.lookup(narrow(target)), args, rules, registry)
            .is_ok_and(|code| code == 0)
    }

    /// Ares 3.0 `UnitClass_UpdatePosition_Visceroid @ 1004C530` (hook
    /// `0x00739F21`, Per_Cell_Process's arrival arm right after
    /// Contact_With_Whom): a small visceroid whose NavCom (`+0x5A4`) is
    /// another small visceroid Unit, arriving in that one's cell (both
    /// GetCoords), makes it the `[General] LargeVisceroid=` type at that
    /// type's Strength ([`Self::store_techno_type`]), refreshes its cell's
    /// threat (`TechnoClass::UpdateThreatInCell`) and leaves the game itself
    /// (UnInit, no death or kill); Per_Cell_Process ends there
    /// (`0x0073B0A5`). Answers whether it merged.
    #[cfg(feature = "ares")]
    pub(crate) fn ares_visceroid_merge(
        &mut self,
        id: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        effects: FrameEffects<'_>,
    ) -> bool {
        let small = |sim: &Self, entity: &crate::sim::game_entity::GameEntity| {
            entity.category() == EntityCategory::Unit
                && sim
                    .object_type(entity.type_ref(), rules)
                    .is_some_and(|object| object.small_visceroid)
        };
        let Some(entity) = self.substrate.entities.get(id).filter(|e| small(self, e)) else {
            return false;
        };
        let Some(large) = rules.general.large_visceroid.as_deref().and_then(|name| {
            rules.object_in_category(crate::rules::object_type::ObjectCategory::Vehicle, name)
        }) else {
            return false;
        };
        let Some(other) = entity
            .navigation
            .nav_com()
            .and_then(NavTargetRef::object_id)
            .filter(|&other| {
                self.substrate
                    .entities
                    .get(other)
                    .is_some_and(|e| small(self, e))
            })
        else {
            return false;
        };
        let terrain = self.resolved_terrain.as_ref();
        let cell = |id: u64| {
            self.substrate.entities.get(id).map(|entity| {
                let coord = crate::sim::movement::ground_pose::object_get_coords(entity, terrain);
                (coord.x >> 8, coord.y >> 8)
            })
        };
        if cell(id) != cell(other) {
            return false;
        }
        self.store_techno_type(other, large, rules);
        self.refresh_spatial_threat(other, rules, None);
        self.uninit_with_context(
            id,
            crate::sim::world::UninitContext::new(rules, registry).with_effects(effects),
        );
        true
    }

    fn set_idle_wander(&mut self, id: u64, direction: i32) {
        if let Some(entity) = self.substrate.entities.get_mut(id) {
            entity.idle_wander.direction = direction;
        }
    }
}

#[cfg(test)]
#[path = "visceroid_tests.rs"]
mod tests;
