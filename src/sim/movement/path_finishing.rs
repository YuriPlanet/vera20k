//! Production projection of the successful A* tail42A406..42A423.
//!
//! The finishing algorithm and height construction each keep their existing
//! pathfinding owner. This adapter supplies map identity, concrete Foot entry
//! and the marker lifetime; it does not split paths at layer transitions.

use super::{MoverPathFacts, PathfindingContext};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::MovementZone;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::pathfinding::cell_entry::{CanEnterCellContext, evaluate_can_enter_cell};
use crate::sim::pathfinding::path_smooth::{self, PathFinishingContext};
use crate::sim::pathfinding::terrain_cost::TerrainCostGrid;
use crate::sim::pathfinding::{
    LayeredPath, LayeredPathStep, PathGrid, SearchFootEntry, SearchMarkerOverlay,
};
use crate::util::native_x87::NativeF64Bits;

#[derive(Clone, Copy)]
enum Cell {
    Native(NativeCellIdentity),
    Compatibility((i16, i16)),
}

struct Finishing<'a> {
    cells: Option<NativeCellQuery<'a>>,
    grid: &'a PathGrid,
    context: PathfindingContext<'a>,
    foot: Option<&'a dyn SearchFootEntry>,
    terrain_costs: Option<&'a TerrainCostGrid>,
    movement_zone: Option<MovementZone>,
    facts: MoverPathFacts,
    markers: Option<&'a SearchMarkerOverlay>,
}

impl Finishing<'_> {
    fn coord(&self, cell: Cell) -> (i16, i16) {
        match cell {
            Cell::Native(cell) => self
                .cells
                .as_ref()
                .expect("native identity domain")
                .coord(cell),
            Cell::Compatibility(coord) => coord,
        }
    }
}

impl PathFinishingContext for Finishing<'_> {
    type Cell = Cell;
    fn get_cell(&self, coord: (i16, i16)) -> Cell {
        self.cells
            .as_ref()
            .map_or(Cell::Compatibility(coord), |cells| {
                Cell::Native(cells.lookup(coord))
            })
    }
    fn ground_level(&self, cell: Cell) -> i32 {
        match cell {
            Cell::Native(cell) => {
                i32::from(self.cells.as_ref().unwrap().ground_fields(cell).0 as i8)
            }
            Cell::Compatibility((x, y)) => self
                .grid
                .cell(x as u16, y as u16)
                .map_or(0, |cell| i32::from(cell.signed_level())),
        }
    }
    fn flags(&self, cell: Cell) -> u32 {
        let coord = self.coord(cell);
        let raw = match cell {
            Cell::Native(cell) => self.cells.as_ref().unwrap().flags(cell),
            Cell::Compatibility((x, y)) => self.grid.cell(x as u16, y as u16).map_or(0, |cell| {
                if cell.has_structural_bridge() {
                    0x100
                } else {
                    0
                }
            }),
        };
        raw | if self
            .markers
            .is_some_and(|markers| markers.contains((coord.0 as u16, coord.1 as u16)))
        {
            0x40000
        } else {
            0
        }
    }
    fn can_enter(&self, cell: Cell, direction: i32, height: i32) -> Result<u8, String> {
        let coord = self.coord(cell);
        let target = (coord.0 as u16, coord.1 as u16);
        if let Some(foot) = self.foot {
            let result = foot.classify(crate::sim::pathfinding::SearchEntryQuery {
                from: None,
                candidate: match cell {
                    Cell::Native(cell) => {
                        crate::sim::pathfinding::SearchEntryCandidate::RetainedNativeCell(cell)
                    }
                    Cell::Compatibility(_) => {
                        crate::sim::pathfinding::SearchEntryCandidate::CopiedCoord(target)
                    }
                },
                direction,
                path_height: height,
            })?;
            return Ok(result);
        }
        // Searches without a class receiver carry a terrain projection. Keep
        // that boundary explicit; the live Foot route above never reduces
        // CanEnter to a walkability predicate.
        let layer = if self.flags(cell) & 0x100 != 0 && height - self.ground_level(cell) == 4 {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        Ok(
            if evaluate_can_enter_cell(CanEnterCellContext {
                target,
                terrain_layer: layer,
                movement_zone: self.movement_zone,
                speed_type: self.facts.speed_type,
                path_grid: Some(self.grid),
                resolved_terrain: self.context.resolved_terrain,
                terrain_costs: self.terrain_costs,
                is_infantry: self.facts.is_infantry,
                mover_is_crusher: self.facts.mover_is_crusher,
            })
            .is_clear()
            {
                0
            } else {
                7
            },
        )
    }
    fn house_threat(&self, coord: (i16, i16)) -> Result<i32, String> {
        if let Some(foot) = self.foot {
            foot.finishing_house_threat(coord)
        } else {
            // No House is represented by the legacy grid-only API. Its
            // compatibility domain has zero coefficient, never a live claim.
            let _ = self.get_cell(coord);
            Ok(0)
        }
    }
    fn threat_coefficient(&self) -> Result<NativeF64Bits, String> {
        self.foot.map_or(
            Ok(NativeF64Bits::POSITIVE_ZERO),
            SearchFootEntry::finishing_threat_coefficient,
        )
    }
    fn tube_exit(&self, cell: Cell) -> Option<(i16, i16)> {
        let Cell::Native(cell) = cell else {
            return None;
        };
        self.context
            .resolved_terrain?
            .tube_for_native_cell(cell)
            .map(|tube| (tube.exit.0 as i16, tube.exit.1 as i16))
    }
}

pub(super) fn finish(
    path: Vec<LayeredPathStep>,
    context: PathfindingContext<'_>,
    grid: &PathGrid,
    foot: Option<&dyn SearchFootEntry>,
    terrain_costs: Option<&TerrainCostGrid>,
    movement_zone: Option<MovementZone>,
    facts: MoverPathFacts,
    markers: Option<&SearchMarkerOverlay>,
) -> Result<LayeredPath, String> {
    if path.is_empty() {
        return Ok((Vec::new(), Vec::new()));
    }
    let prepared_markers = foot.and_then(SearchFootEntry::prepared_search_markers);
    let markers = prepared_markers.as_deref().or(markers);
    let start = (path[0].rx as i16, path[0].ry as i16);
    let ctx = Finishing {
        cells: context.resolved_terrain.map(NativeCellQuery::canonical),
        grid,
        context,
        foot,
        terrain_costs,
        movement_zone,
        facts,
        markers,
    };
    let mut directions = Vec::with_capacity(path.len() - 1);
    for pair in path.windows(2) {
        let from = (pair[0].rx as i16, pair[0].ry as i16);
        let to = (pair[1].rx as i16, pair[1].ry as i16);
        let delta = (to.0.wrapping_sub(from.0), to.1.wrapping_sub(from.1));
        let direction = crate::util::direction::DIRECTION_DELTAS
            .iter()
            .position(|&(x, y)| delta == (x as i16, y as i16))
            .map(|dir| dir as i32);
        directions.push(
            direction
                .or_else(|| (ctx.tube_exit(ctx.get_cell(from)) == Some(to)).then_some(8))
                .ok_or("reconstructed path has neither compass nor Tube edge")?,
        );
    }
    let heights: Vec<i32> = path
        .iter()
        .take(directions.len())
        .map(|step| i32::from(step.path_height()))
        .collect();
    path_smooth::finish_path(start, &mut directions, &heights, &ctx)?;
    let mut coord = start;
    let mut coords = vec![(path[0].rx, path[0].ry)];
    let mut layers = vec![path[0].layer];
    let mut height = path[0].path_height();
    for direction in directions {
        let from = coord;
        coord = if direction == 8 {
            ctx.tube_exit(ctx.get_cell(from)).unwrap_or((0, 0))
        } else {
            let (dx, dy) = crate::util::direction::DIRECTION_DELTAS[direction as usize];
            (
                from.0.wrapping_add(dx as i16),
                from.1.wrapping_add(dy as i16),
            )
        };
        let from_cell = grid.cell(from.0 as u16, from.1 as u16);
        let to_cell = grid
            .cell(coord.0 as u16, coord.1 as u16)
            .ok_or("finished path left represented PathGrid")?;
        // Layers are the movement adapter's projection, not a second path
        // authority. Reuse the original descriptor-height construction owner
        // after finishing rewrites coordinates; do not carry stale old layers.
        height = crate::sim::pathfinding::compute_node_height(height, from_cell, to_cell);
        coords.push((coord.0 as u16, coord.1 as u16));
        layers.push(
            if to_cell.has_structural_bridge() && height == to_cell.signed_level() + 4 {
                MovementLayer::Bridge
            } else {
                MovementLayer::Ground
            },
        );
    }
    Ok((coords, layers))
}

#[cfg(test)]
#[path = "path_finishing_identity_tests.rs"]
mod identity_tests;
