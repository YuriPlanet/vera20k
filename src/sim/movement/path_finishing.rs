//! Production projection of the successful A* tail42A406..42A423.
//!
//! The finishing algorithm and height construction each keep their existing
//! pathfinding owner. This adapter supplies map identity, concrete Foot entry
//! and the marker lifetime; it does not split paths at layer transitions.

use super::PathfindingContext;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::sim::pathfinding::path_smooth::{self, PathFinishingContext};
use crate::sim::pathfinding::{
    LayeredPathStep, PathGrid, SearchEntryCandidate, SearchEntryQuery, SearchFootEntry,
    SearchMarkerOverlay,
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
    foot: &'a dyn SearchFootEntry,
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
    fn fixed_deck_level(&self, cell: Cell) -> Option<i32> {
        let Cell::Native(cell) = cell else {
            return None;
        };
        self.context
            .resolved_terrain?
            .corrected_high_bridge_deck_at(cell)
            .map(i32::from)
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
        self.foot.classify(SearchEntryQuery {
            from: None,
            candidate: match cell {
                Cell::Native(cell) => SearchEntryCandidate::RetainedNativeCell(cell),
                Cell::Compatibility(coord) => {
                    SearchEntryCandidate::CopiedCoord((coord.0 as u16, coord.1 as u16))
                }
            },
            direction,
            path_height: height,
        })
    }
    fn house_threat(&self, coord: (i16, i16)) -> Result<i32, String> {
        self.foot.finishing_house_threat(coord)
    }
    fn threat_coefficient(&self) -> Result<NativeF64Bits, String> {
        self.foot.finishing_threat_coefficient()
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
    foot: &dyn SearchFootEntry,
    markers: Option<&SearchMarkerOverlay>,
) -> Result<Vec<i32>, String> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    let markers = foot.prepared_search_markers().or(markers);
    let start = (path[0].rx as i16, path[0].ry as i16);
    let ctx = Finishing {
        cells: context.resolved_terrain.map(NativeCellQuery::canonical),
        grid,
        context,
        foot,
        markers,
    };
    // AStar reconstruction (0x0042AA90) reads adjacent descriptors' cells.
    // Each edge is a compass step or a nonadjacent Tube jump (0x0042AB76).
    let mut directions = path
        .windows(2)
        .map(|pair| {
            let from = (pair[0].rx as i16, pair[0].ry as i16);
            i32::from(crate::sim::components::direction_from_step(
                from,
                (pair[1].rx, pair[1].ry),
            ))
        })
        .collect::<Vec<_>>();
    // The outgoing-node heights are the steps' own, one per direction.
    let heights = &path[..directions.len()];
    path_smooth::finish_path(start, &mut directions, heights, &ctx)?;
    // AStar_main_loop (0x0042A432) returns these words directly. Smoothing
    // can admit the native Dummy beyond the edge of VERA's PathGrid.
    Ok(directions)
}

#[cfg(all(test, tests_sim_movement))]
#[path = "path_finishing_identity_tests.rs"]
mod identity_tests;
