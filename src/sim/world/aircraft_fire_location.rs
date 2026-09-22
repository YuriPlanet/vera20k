//! Aircraft FindFireLocation4197C0 and IsCellFree419B00. Search reads live
//! objects/reservations; only a successful cell search consumes Scenario RNG.
//! Executable witnesses: tools/spatial_oracle/aircraft_fire_location.{py,json}.

use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::combat_weapon;
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::movement::{ground_pose, target_cell_coord};

#[cfg(test)]
#[path = "aircraft_fire_location_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "aircraft_approach_tests.rs"]
mod approach_tests;

/// Live occupants and Cell NavCom reservations read once per search.
/// IsCellFree419B00 is an any-match over live objects, so one pass answers
/// every candidate of one FindFireLocation call; nothing mutates between them.
struct FireCellClaims {
    occupied: std::collections::BTreeSet<(i16, i16)>,
    /// Reserved fixed-cell identities (`None` = the shared Dummy), or raw
    /// reserved coordinates when no terrain is loaded.
    reserved: std::collections::BTreeSet<Option<usize>>,
    reserved_cells: std::collections::BTreeSet<(i16, i16)>,
}

impl Simulation {
    /// Target+48 is its physical center, independently of the destination
    /// receiver+4C. In particular, do not read a Building's helipad offset here.
    pub(super) fn fire_location_center(
        &self,
        target: NavTargetRef,
        rules: &RuleSet,
    ) -> Option<DriveCoord> {
        let id = match target {
            NavTargetRef::Cell { rx, ry } => {
                return Some(target_cell_coord(rx, ry, self.resolved_terrain.as_ref()));
            }
            NavTargetRef::Entity { id }
            | NavTargetRef::Object { id }
            | NavTargetRef::Building { id } => id,
        };
        let entity = self.substrate.entities.get(id)?;
        let object = rules.object(self.interner.resolve(entity.type_ref()))?;
        Some(ground_pose::object_center_coord(entity, object))
    }

    pub(crate) fn aircraft_find_fire_location(
        &mut self,
        id: u64,
        target: Option<NavTargetRef>,
        rules: &RuleSet,
    ) -> Option<NavTargetRef> {
        let target = target?;
        let entity = self.substrate.entities.get(id)?;
        let object = rules.object(self.interner.resolve(entity.type_ref()))?;
        if combat_weapon::aircraft_strafes(rules, object, entity.veterancy) {
            return Some(target);
        }
        let range = combat_weapon::weapon_range(
            entity,
            object,
            0,
            &self.substrate.entities,
            rules,
            &self.interner,
        );
        let center = self.fire_location_center(target, rules)?;
        let destination = match target {
            NavTargetRef::Cell { .. } => None,
            NavTargetRef::Entity { id }
            | NavTargetRef::Object { id }
            | NavTargetRef::Building { id } => self
                .substrate
                .entities
                .get(id)
                .filter(|e| {
                    matches!(
                        e.category,
                        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                    )
                })
                .and_then(|e| e.navigation.nav_com),
        };
        let reference = destination
            .and_then(|d| self.fire_location_center(d, rules))
            .filter(|c| *c != (DriveCoord { x: 0, y: 0, z: 0 }))
            .unwrap_or_else(|| ground_pose::object_center_coord(entity, object));
        let check_shroud = !self.session.game_mode_nonzero && !entity.is_mission_only();
        let mut claims = None;
        let mut radius = range.wrapping_sub(256);
        while radius > 256 {
            let mut best: Option<((i16, i16), i32)> = None;
            let mut previous = None;
            for direction in 0..16 {
                let xy = crate::util::native_trig::facing_step_world_xy(
                    [center.x, center.y],
                    direction << 12,
                    radius,
                );
                let cell = ((xy[0] / 256) as i16, (xy[1] / 256) as i16);
                if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
                    (i32::from(cell.0), i32::from(cell.1)),
                    self.playfield_bounds,
                    self.resolved_terrain.as_ref(),
                ) {
                    continue;
                }
                if check_shroud {
                    let resolved = self
                        .resolved_terrain
                        .as_ref()
                        .map_or(cell, |t| t.native_cell_coord(t.native_cell_identity(cell)));
                    if !self.session.current_house.is_some_and(|viewer| {
                        self.fog
                            .is_ground_open(viewer, resolved.0 as u16, resolved.1 as u16)
                    }) {
                        continue;
                    }
                }
                if !self.aircraft_fire_cell_free(id, cell, rules, &mut claims) {
                    continue;
                }
                let distance = crate::util::native_x87::distance_3d_leptons(
                    [xy[0], xy[1], 0],
                    [reference.x, reference.y, 0],
                );
                if best.is_none_or(|(_, old)| distance < old) {
                    previous = best.map(|(cell, _)| cell);
                    best = Some((cell, distance));
                }
            }
            if let Some((best, _)) = best {
                let selected = if self.scenario_rng.next_range_i32_inclusive(0, 99) < 50 {
                    best
                } else {
                    previous.unwrap_or(best)
                };
                let resolved = self.resolved_terrain.as_ref().map_or(selected, |t| {
                    t.native_cell_coord(t.native_cell_identity(selected))
                });
                return Some(NavTargetRef::cell(resolved.0 as u16, resolved.1 as u16));
            }
            radius -= 256;
        }
        None
    }

    /// 419B00 with includeSelf=true, as passed by FindFireLocation. The outer
    /// search already admitted the playfield; native repeats that query here.
    fn aircraft_fire_cell_free(
        &self,
        id: u64,
        cell: (i16, i16),
        rules: &RuleSet,
        claims: &mut Option<FireCellClaims>,
    ) -> bool {
        if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(cell.0), i32::from(cell.1)),
            self.playfield_bounds,
            self.resolved_terrain.as_ref(),
        ) {
            return true;
        }
        let entity = self.substrate.entities.get(id).unwrap();
        let object = rules
            .object(self.interner.resolve(entity.type_ref()))
            .unwrap();
        if object.spawned {
            let resolved = self
                .resolved_terrain
                .as_ref()
                .map_or(cell, |t| t.native_cell_coord(t.native_cell_identity(cell)));
            if super::techno_ai_cloak::find_nearest_object_in_cell(
                self,
                (resolved.0 as u16, resolved.1 as u16),
            )
            .and_then(|other| self.substrate.entities.get(other))
            .is_some_and(|other| {
                other.spawn_manager.is_some()
                    || rules
                        .object(self.interner.resolve(other.type_ref()))
                        .is_some_and(|object| object.spawned)
            }) {
                return true;
            }
        }
        if object.airport_bound {
            return true;
        }
        let skip = if object.carryall {
            match entity.navigation.nav_com {
                Some(
                    NavTargetRef::Entity { id }
                    | NavTargetRef::Object { id }
                    | NavTargetRef::Building { id },
                ) => self
                    .substrate
                    .entities
                    .get(id)
                    .filter(|e| e.category == EntityCategory::Unit)
                    .map(|e| e.stable_id()),
                _ => None,
            }
        } else {
            None
        };
        let identity = self
            .resolved_terrain
            .as_ref()
            .map(|t| t.native_cell_identity(cell));
        let claims = claims.get_or_insert_with(|| self.fire_cell_claims(skip));
        if claims.occupied.contains(&cell) {
            return false;
        }
        match identity {
            Some(identity) => !claims.reserved.contains(&identity_key(identity)),
            None => !claims.reserved_cells.contains(&cell),
        }
    }

    /// Do not use occupancy: air, limbo and Cell NavCom are distinct here.
    fn fire_cell_claims(&self, skip: Option<u64>) -> FireCellClaims {
        let mut claims = FireCellClaims {
            occupied: Default::default(),
            reserved: Default::default(),
            reserved_cells: Default::default(),
        };
        for other in self.substrate.entities.values() {
            if skip == Some(other.stable_id())
                || !other.lifecycle.object_alive
                || other.lifecycle.in_limbo
                || !matches!(
                    other.category,
                    EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                )
            {
                continue;
            }
            let coord = ground_pose::position_world_coord(&other.position);
            claims
                .occupied
                .insert(((coord.x / 256) as i16, (coord.y / 256) as i16));
            if let Some(NavTargetRef::Cell { rx, ry }) = other.navigation.nav_com {
                match self.resolved_terrain.as_ref() {
                    // Pointer equality must not restamp the shared Dummy.
                    Some(t) => {
                        claims
                            .reserved
                            .insert(t.native_fixed_cell_index(rx as i16, ry as i16));
                    }
                    None => {
                        claims.reserved_cells.insert((rx as i16, ry as i16));
                    }
                }
            }
        }
        claims
    }
}

fn identity_key(identity: crate::map::cell_index::NativeCellIdentity) -> Option<usize> {
    match identity {
        crate::map::cell_index::NativeCellIdentity::Real(index) => Some(index),
        crate::map::cell_index::NativeCellIdentity::Dummy => None,
    }
}
