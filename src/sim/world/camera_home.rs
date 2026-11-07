//! Current-player base view selection, shared by TAction145 and the H command.
use super::Simulation;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::movement::ground_pose::object_get_coords;

const NO_COORD: [i32; 3] = [0, 0, 0];

impl Simulation {
    /// `DisplayClass::CenterCameraOnPlayerTechnos @ 0x004AC4F0`, which
    /// `ScenarioClass::Post_Map_Init @ 0x00686890` calls once (`0x006869A6`),
    /// after the starting units and crates: the opening view of a
    /// non-campaign scenario. It averages the Location XY (`+0x9C`, `+0xA0`)
    /// of every TechnoClass::Array object out of Limbo (`+0x81`) whose
    /// current-player byte (`+0x41A`) is set, in 32-bit wrapping sums divided
    /// by the count, at Z 0. With none, it takes the current player's
    /// `HouseClass::Get_Base_Or_Starting_Coord @ 0x0050DF30` XY at Z 0. A map
    /// object that a trigger hands the player later is not counted.
    ///
    /// Returns the view coordinate and the Cell that native stores in
    /// Scenario View1..4 (the coordinate divided by 256 toward zero). `None`
    /// without a current player, or (VERA adaptation for an observer, which
    /// owns nothing) when the base coordinate is empty too. Native's
    /// AltHomeCell = HomeCell store is campaign carry-over state and is not
    /// ported here.
    pub(crate) fn opening_view_on_player_technos(
        &self,
        cells: Option<&NativeCellQuery<'_>>,
    ) -> Option<([i32; 3], (u16, u16))> {
        let owner = self.session.current_house()?;
        let entities = &self.substrate.entities;
        let (mut sum_x, mut sum_y, mut count) = (0i32, 0i32, 0i32);
        for ordinal in 0..entities.techno_registry_len() {
            let Some(entity) = entities
                .techno_registry_at(ordinal)
                .and_then(|id| entities.get(id))
            else {
                continue;
            };
            if entity.lifecycle.in_limbo() || !entity.discovery.owned_by_current_house() {
                continue;
            }
            let [x, y] = crate::sim::movement::ground_pose::position_world_xy(&entity.position);
            sum_x = sum_x.wrapping_add(x);
            sum_y = sum_y.wrapping_add(y);
            count += 1;
        }
        let coord = if count == 0 {
            let [x, y, _] = self.house_base_coordinate(owner, cells);
            if (x, y) == (0, 0) {
                return None;
            }
            [x, y, 0]
        } else {
            [sum_x.wrapping_div(count), sum_y.wrapping_div(count), 0]
        };
        let cell = (
            u16::try_from(coord[0] / 256).ok()?,
            u16::try_from(coord[1] / 256).ok()?,
        );
        Some((coord, cell))
    }

    /// Native Home7315A0 (Steam gamemd). The current
    /// player's live House Building list is walked backwards: the first
    /// primary ConstructionYard wins, else the earliest remaining yard;
    /// the last other Building is retained as a fallback. Only when both
    /// coordinates are empty and House+2E8 is nonzero is UnitClass::Array
    /// searched backwards for a human-player-owned General.BaseUnit type.
    /// House50DF30 then overrides the other-Building fallback, unless its
    /// coordinate is empty or Cell0,0's center (730930/730950 initializers).
    ///
    /// Native checks Limbo81, without adding health/alive/placement gates.
    /// The caller supplies its existing cell query: triggers use
    /// [`NativeCellQuery::for_local_input`], while client-only H queries
    /// isolate its writes from simulation.
    pub fn home_camera_coordinate(
        &self,
        rules: &RuleSet,
        cells: Option<&NativeCellQuery<'_>>,
    ) -> Option<[i32; 3]> {
        let owner = self.session.current_house()?;
        let house = self.houses.get(&owner)?;
        let mut preferred = NO_COORD;
        let mut fallback = NO_COORD;
        for &id in house.base_projection().buildings().iter().rev() {
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
        if preferred == NO_COORD && fallback == NO_COORD && house.tracking().vehicles() != 0 {
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
