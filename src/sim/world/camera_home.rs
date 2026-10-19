//! Current-player base view selection, shared by TAction145 and the H command.
use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose::object_get_coords;

const NO_COORD: [i32; 3] = [0, 0, 0];

impl Simulation {
    /// Native Home7315A0 (Steam gamemd3e81a617..08600). The current
    /// player's live House Building list is walked backwards: the first
    /// primary ConstructionYard wins, else the earliest remaining yard;
    /// the last other Building is retained as a fallback. Only when both
    /// coordinates are empty and House+2E8 is nonzero is UnitClass::Array
    /// searched backwards for a human-player-owned General.BaseUnit type.
    /// House50DF30 then overrides the other-Building fallback, unless its
    /// coordinate is empty or Cell0,0's center (730930/730950 initializers).
    ///
    /// Native checks Limbo81, without adding health/alive/placement gates.
    /// The caller supplies its existing cell query: triggers use the canonical
    /// Dummy, while client-only H queries isolate its writes from simulation.
    pub fn home_camera_coordinate(
        &self,
        rules: &RuleSet,
        cells: Option<&NativeCellQuery<'_>>,
    ) -> Option<[i32; 3]> {
        let owner = self.session.current_house()?;
        let house = self.houses.get(&owner)?;
        let mut preferred = NO_COORD;
        let mut fallback = NO_COORD;
        for &id in house.base_projection.buildings().iter().rev() {
            let Some(entity) = self.substrate.entities.get(id) else {
                continue;
            };
            if entity.lifecycle.in_limbo() {
                continue;
            }
            let Some(kind) = self.object_type(entity.type_ref(), rules) else {
                continue;
            };
            if kind.construction_yard {
                let point = object_get_coords(entity, self.resolved_terrain.as_ref());
                preferred = [point.x, point.y, point.z];
                if self.production.is_primary_factory(id) {
                    break;
                }
            } else if fallback == NO_COORD {
                let point = object_get_coords(entity, self.resolved_terrain.as_ref());
                fallback = [point.x, point.y, point.z];
            }
        }
        if preferred == NO_COORD && fallback == NO_COORD && house.tracking.vehicles() != 0 {
            // UnitClass::Array uses stable removal (7358B3..7358DE). The
            // store's monotone stable handles retain that order independently
            // of Logic's active-object order, including constructor Limbo.
            for (_, entity) in self.substrate.entities.iter_sorted().rev() {
                if entity.category() != EntityCategory::Unit
                    || entity.lifecycle.in_limbo()
                    || !self.house_is_human_player(entity.owner())
                    || !rules.general.base_unit_types.iter().any(|name| {
                        self.interner
                            .resolve(entity.type_ref())
                            .eq_ignore_ascii_case(name)
                    })
                {
                    continue;
                }
                let point = object_get_coords(entity, self.resolved_terrain.as_ref());
                preferred = [point.x, point.y, point.z];
                break;
            }
        }
        // Native always executes this query, even with a preferred Building.
        // In particular, preserve its canonical Map/Dummy side effects.
        let base = self.house_base_coordinate(owner, cells);
        if base != NO_COORD && base != [128, 128, 0] {
            fallback = base;
        }
        if preferred != NO_COORD {
            Some(preferred)
        } else {
            (fallback != NO_COORD).then_some(fallback)
        }
    }
}

#[cfg(test)]
#[path = "camera_home_tests.rs"]
mod tests;
