//! Live Cell selection for Bounce439B00/439A10 and VoxelAnim749F30.
//! Evidence: docs/research/PHASE3_BOUNCE_GROUND_QUERY_DELIVERY_NATIVE_REPORT.md.
use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::bounce::BounceTerrain;
use crate::sim::cell_rect::{CellRef, get_cellclass_fallback_leptons};
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::world::Simulation;
use glam::IVec3;

pub(super) struct ResolvedBounceTerrain<'a> {
    pub sim: &'a Simulation,
    pub rules: Option<&'a RuleSet>,
}

impl<'a> BounceTerrain for ResolvedBounceTerrain<'a> {
    type Cell = CellRef<'a>;

    fn select_cell(&self, coord: IVec3) -> Self::Cell {
        get_cellclass_fallback_leptons(self.sim.resolved_terrain.as_ref(), coord.x, coord.y)
    }

    fn ground_height_leptons(&self, coord: IVec3) -> i32 {
        // 578080 selects independently of the subsequent565730 pointer.
        let (level, slope) = match self.select_cell(coord) {
            CellRef::Real(cell) => (cell.level, cell.slope_type),
            CellRef::Dummy { cell } => {
                let snapshot = cell.snapshot();
                (snapshot.level as u8, snapshot.slope_type)
            }
        };
        crate::util::lepton::ground_height_leptons(level, slope, coord.x, coord.y).unwrap_or_else(
            |_| {
                log::warn!("Bounce ground slope {slope} outside native comparison domain");
                0
            },
        )
    }

    fn selected_is_bridge(&self, cell: &Self::Cell) -> bool {
        // 439C23/439A6F test raw CellClass Flags100, not low-bridge identity.
        cell.bridge_flags_0x1180() & 0x100 != 0
    }

    fn cell_height_level(&self, coord: IVec3) -> i32 {
        match self.select_cell(coord) {
            CellRef::Real(cell) => i32::from(cell.level as i8),
            CellRef::Dummy { cell } => i32::from(cell.snapshot().level),
        }
    }

    fn ramp(&self, coord: IVec3) -> u8 {
        // 6D6AD0 performs its own565730 lookup before reading slope11C.
        match self.select_cell(coord) {
            CellRef::Real(cell) => cell.slope_type,
            CellRef::Dummy { cell } => cell.snapshot().slope_type,
        }
    }

    fn has_bounce_surface(&self, selected: &Self::Cell) -> bool {
        // 439CAB retains the initial NEW Cell pointer across the optional OLD
        // lookup. Dummy has no native object list; its overlay identity persists.
        let cell = match selected {
            CellRef::Dummy { cell } => {
                return matches!(cell.overlay_identity_state().0, 2 | 26 | 243);
            }
            CellRef::Real(cell) => cell,
        };
        if let Some(occupancy) = self.sim.occupancy().get(cell.rx, cell.ry) {
            for occupant in occupancy.iter_layer(MovementLayer::Ground) {
                let Some(entity) = self.sim.entities().get(occupant.entity_id) else {
                    continue;
                };
                if entity.category() != EntityCategory::Structure {
                    continue;
                }
                // 47C520 first normal-list RTTI6; 457620->465D40 rejects
                // exactly a1x1 structure with resolved UndeploysInto type.
                return self
                    .rules
                    .and_then(|rules| self.sim.object_type(entity.type_ref(), rules))
                    .is_some_and(|kind| !kind.is_1x1_with_undeploy());
            }
        }
        // 480510(-1,-1) accepts these three raw overlay IDs only.
        self.sim
            .overlay_grid
            .as_ref()
            .filter(|grid| cell.rx < grid.width() && cell.ry < grid.height())
            .is_some_and(|grid| {
                matches!(grid.cell(cell.rx, cell.ry).overlay_id, Some(2 | 26 | 243))
            })
    }

    fn is_water(&self, coord: IVec3) -> bool {
        match self.select_cell(coord) {
            CellRef::Real(cell) => cell.yr_cell_land_type == 2,
            // Constructor dummy land is Clear; no modelled live writer changes it.
            CellRef::Dummy { .. } => false,
        }
    }
}

#[cfg(test)]
#[path = "bounce_terrain_tests.rs"]
mod tests;
