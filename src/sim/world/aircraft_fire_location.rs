//! Aircraft FindFireLocation4197C0 and Is_Cell_Free_For_Landing419B00. Search
//! reads live objects/reservations; only a successful cell search consumes
//! Scenario RNG. Executable witnesses:
//! tools/spatial_oracle/aircraft_fire_location.{py,json} and, for
//! Find_Attack_Cell's `include_self = false`, aircraft_move.{py,json}.

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
/// Is_Cell_Free_For_Landing419B00 is an any-match over live objects, so one
/// pass answers every candidate of one search; nothing mutates between them.
/// One search passes one `include_self`, which the pass already applied.
/// VERA-only: the sets are bitmaps, built from every live object and asked
/// about a few cells.
pub(super) struct LandingCellClaims {
    occupied: CellSet,
    /// Reserved fixed cells: real cells by index, and whether the shared
    /// Dummy is reserved; or raw reserved coordinates when no terrain is
    /// loaded.
    reserved: Vec<u64>,
    reserved_dummy: bool,
    reserved_cells: CellSet,
}

/// A set of cells built once and then only asked: a bitmap over the box its
/// members span, or a sorted list when that box is too large to allocate.
enum CellSet {
    Bitmap {
        min: (i32, i32),
        width: usize,
        height: usize,
        bits: Vec<u64>,
    },
    Sorted(Vec<(i16, i16)>),
}

impl CellSet {
    /// The most cells a bitmap covers: 512 KiB of bits.
    const MOST_BITMAP_CELLS: usize = 1 << 22;

    fn new(mut cells: Vec<(i16, i16)>) -> Self {
        let span = cells
            .iter()
            .fold(None::<(i32, i32, i32, i32)>, |span, &(x, y)| {
                let (x, y) = (i32::from(x), i32::from(y));
                Some(span.map_or((x, y, x, y), |(min_x, min_y, max_x, max_y)| {
                    (min_x.min(x), min_y.min(y), max_x.max(x), max_y.max(y))
                }))
            });
        let Some((min_x, min_y, max_x, max_y)) = span else {
            return Self::Sorted(cells);
        };
        let width = (max_x - min_x + 1) as usize;
        let height = (max_y - min_y + 1) as usize;
        if width * height > Self::MOST_BITMAP_CELLS {
            cells.sort_unstable();
            cells.dedup();
            return Self::Sorted(cells);
        }
        let mut bits = vec![0u64; (width * height).div_ceil(64)];
        for (x, y) in cells {
            let index = (i32::from(y) - min_y) as usize * width + (i32::from(x) - min_x) as usize;
            bits[index / 64] |= 1 << (index % 64);
        }
        Self::Bitmap {
            min: (min_x, min_y),
            width,
            height,
            bits,
        }
    }

    fn contains(&self, cell: (i16, i16)) -> bool {
        match self {
            Self::Bitmap {
                min,
                width,
                height,
                bits,
            } => {
                let (dx, dy) = (i32::from(cell.0) - min.0, i32::from(cell.1) - min.1);
                if dx < 0 || dy < 0 || dx as usize >= *width || dy as usize >= *height {
                    return false;
                }
                let index = dy as usize * width + dx as usize;
                bits[index / 64] >> (index % 64) & 1 != 0
            }
            Self::Sorted(cells) => cells.binary_search(&cell).is_ok(),
        }
    }
}

impl Simulation {
    /// Target+48 is its physical center, independently of the destination
    /// receiver+4C. In particular, do not read a Building's helipad offset here.
    pub(super) fn fire_location_center(&self, target: NavTargetRef) -> Option<DriveCoord> {
        let id = match target {
            NavTargetRef::Cell { rx, ry } => {
                return Some(target_cell_coord(
                    rx,
                    ry,
                    self.resolved_terrain
                        .as_ref()
                        .map(crate::map::resolved_terrain::NativeCellQuery::canonical)
                        .as_ref(),
                ));
            }
            NavTargetRef::Object { id } => id,
        };
        let entity = self.substrate.entities.get(id)?;
        Some(ground_pose::object_get_coords(
            entity,
            self.resolved_terrain.as_ref(),
        ))
    }

    pub(crate) fn aircraft_find_fire_location(
        &mut self,
        id: u64,
        target: Option<NavTargetRef>,
        rules: &RuleSet,
    ) -> Option<NavTargetRef> {
        let target = target?;
        let entity = self.substrate.entities.get(id)?;
        let object = self.interner.object_type(entity.type_ref(), rules)?;
        if combat_weapon::aircraft_strafes(rules, object, entity.veterancy.rank()) {
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
        let center = self.fire_location_center(target)?;
        let destination = match target {
            NavTargetRef::Cell { .. } => None,
            NavTargetRef::Object { id } => self
                .substrate
                .entities
                .get(id)
                .filter(|e| {
                    matches!(
                        e.category(),
                        EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                    )
                })
                .and_then(|e| e.navigation.nav_com()),
        };
        let reference = destination
            .and_then(|d| self.fire_location_center(d))
            .filter(|c| *c != (DriveCoord { x: 0, y: 0, z: 0 }))
            .unwrap_or_else(|| {
                ground_pose::object_get_coords(entity, self.resolved_terrain.as_ref())
            });
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
                if !self.aircraft_cell_free_for_landing(id, cell, true, rules, &mut claims) {
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

    /// `AircraftClass::Is_Cell_Free_For_Landing @ 0x00419B00`. Free without
    /// looking at objects for a cell outside the playfield, a spawned
    /// aircraft over a spawner or a spawned object, and an AirportBound
    /// aircraft (`0x00419B8A`). `include_self` is its second argument:
    /// FindFireLocation and Mission_Move pass true, so this aircraft's own
    /// cell and Cell NavCom count against `cell` (past those early answers a
    /// Cell NavCom claims its own cell); Find_Attack_Cell passes false. The
    /// outer searches already admitted the playfield; native repeats that
    /// query here.
    pub(super) fn aircraft_cell_free_for_landing(
        &self,
        id: u64,
        cell: (i16, i16),
        include_self: bool,
        rules: &RuleSet,
        claims: &mut Option<LandingCellClaims>,
    ) -> bool {
        if !crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(cell.0), i32::from(cell.1)),
            self.playfield_bounds,
            self.resolved_terrain.as_ref(),
        ) {
            return true;
        }
        let entity = self.substrate.entities.get(id).unwrap();
        let object = self.interner.object_type(entity.type_ref(), rules).unwrap();
        if object.spawned {
            let resolved = self
                .resolved_terrain
                .as_ref()
                .map_or(cell, |t| t.native_cell_coord(t.native_cell_identity(cell)));
            if self
                .nearest_cell_object(
                    (resolved.0 as u16, resolved.1 as u16),
                    crate::sim::movement::locomotor::MovementLayer::Ground,
                    None,
                )
                .and_then(|other| self.substrate.entities.get(other))
                .is_some_and(|other| {
                    other.spawn_manager.is_some()
                        || self
                            .interner
                            .object_type(other.type_ref(), rules)
                            .is_some_and(|object| object.spawned)
                })
            {
                return true;
            }
        }
        if object.airport_bound {
            return true;
        }
        let skip = if object.carryall {
            match entity.navigation.nav_com() {
                Some(NavTargetRef::Object { id }) => self
                    .substrate
                    .entities
                    .get(id)
                    .filter(|e| e.category() == EntityCategory::Unit)
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
        let own = (!include_self).then_some(id);
        let claims = claims.get_or_insert_with(|| self.landing_cell_claims(skip, own));
        if claims.occupied.contains(cell) {
            return false;
        }
        match identity {
            Some(crate::map::cell_index::NativeCellIdentity::Real(index)) => {
                claims.reserved[index / 64] >> (index % 64) & 1 == 0
            }
            Some(crate::map::cell_index::NativeCellIdentity::Dummy) => !claims.reserved_dummy,
            None => !claims.reserved_cells.contains(cell),
        }
    }

    /// Do not use occupancy: air, limbo and Cell NavCom are distinct here.
    /// `carried` is a Carryall's Unit NavCom (`0x00419BF1`), `own` the
    /// aircraft itself when the call excludes it (`0x00419C05`).
    fn landing_cell_claims(&self, carried: Option<u64>, own: Option<u64>) -> LandingCellClaims {
        let terrain = self.resolved_terrain.as_ref();
        let mut occupied = Vec::new();
        let mut reserved = vec![0u64; terrain.map_or(0, |t| t.cells().len().div_ceil(64))];
        let mut reserved_dummy = false;
        let mut reserved_cells = Vec::new();
        for other in self.substrate.entities.values() {
            if carried == Some(other.stable_id())
                || own == Some(other.stable_id())
                || !other.lifecycle.object_alive()
                || other.lifecycle.in_limbo()
                || !matches!(
                    other.category(),
                    EntityCategory::Unit | EntityCategory::Infantry | EntityCategory::Aircraft
                )
            {
                continue;
            }
            let coord = ground_pose::position_world_coord(&other.position);
            occupied.push(((coord.x / 256) as i16, (coord.y / 256) as i16));
            if let Some(NavTargetRef::Cell { rx, ry }) = other.navigation.nav_com() {
                match terrain {
                    // Pointer equality must not restamp the shared Dummy.
                    Some(t) => match t.native_fixed_cell_index(rx as i16, ry as i16) {
                        Some(index) => reserved[index / 64] |= 1 << (index % 64),
                        None => reserved_dummy = true,
                    },
                    None => reserved_cells.push((rx as i16, ry as i16)),
                }
            }
        }
        LandingCellClaims {
            occupied: CellSet::new(occupied),
            reserved,
            reserved_dummy,
            reserved_cells: CellSet::new(reserved_cells),
        }
    }
}
