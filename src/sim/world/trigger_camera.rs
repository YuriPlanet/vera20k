//! Authored camera coordinates use the simulation's retained waypoint and
//! CellClass query authorities before the frame hash is finalized.

use super::Simulation;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::sim::components::DriveCoord;
use crate::sim::movement::ground_pose::{deck_surface_z, query_ground_height};

impl Simulation {
    /// Original TActionExecute6DD8B0 cases48/112: GetWaypoint68BCC0,
    /// signed cell center, Ground578080, then Map5657A0's short-circuit
    /// bridge flags100/400. Ground and packed-cell lookups are distinct;
    /// invalid coordinates stamp the same process CellClass Dummy in order.
    /// Native executable controls for the downstream FocusOn/TacticalAI:
    /// tools/input_oracle/fast_scroll.{py,json,meta.json}.
    pub(crate) fn trigger_waypoint_ground_coord(&self, index: i32) -> Option<[i32; 3]> {
        self.trigger_cell_ground_coord(self.session.waypoint_cell(index)?)
    }

    /// [`Self::trigger_waypoint_ground_coord`] after GetWaypoint68BCC0, for a
    /// cell (a Wasm module's cell target).
    pub(crate) fn trigger_cell_ground_coord(&self, cell: (i16, i16)) -> Option<[i32; 3]> {
        let terrain = self.resolved_terrain.as_ref()?;
        let cells = NativeCellQuery::canonical(terrain);
        let center = crate::sim::cell_kernel::cell_center(
            crate::sim::cell_kernel::CellCoordinate {
                x: i32::from(cell.0),
                y: i32::from(cell.1),
            },
            0,
        );
        let (x, y) = (center.x, center.y);
        let z = query_ground_height(&cells, DriveCoord { x, y, z: 0 })
            .expect("authored camera ground query requires a valid native slope");
        Some([x, y, z])
    }

    /// The same query plus the deck height, shared by cases 41 (6E2290) and
    /// 43 (6E2520).
    pub(crate) fn trigger_waypoint_coord(&self, index: i32) -> Option<[i32; 3]> {
        self.trigger_cell_coord(self.session.waypoint_cell(index)?)
    }

    /// [`Self::trigger_waypoint_coord`] for a cell (a Wasm module's cell
    /// target).
    pub fn trigger_cell_coord(&self, cell: (i16, i16)) -> Option<[i32; 3]> {
        let [x, y, mut z] = self.trigger_cell_ground_coord(cell)?;
        let cells = NativeCellQuery::canonical(self.resolved_terrain.as_ref()?);
        let first = cells.lookup(cell);
        if cells.flags(first) & 0x100 != 0 || cells.flags(cells.lookup(cell)) & 0x400 != 0 {
            z = deck_surface_z(&cells, first, z);
        }
        Some([x, y, z])
    }

    /// Scenario GetWaypointLocation68BD00: retain Map5657A0's receiver,
    /// call its Cell486840 GetCoords, then read its flags500 for the deck.
    /// Unlike the inline camera/animation path above, packed aliases use the
    /// selected Cell's own coordinate, and height reads never re-query it.
    /// Eleven original Steam gamemd controls cover aliases, both deck
    /// flags, signed levels/slopes and the shared Dummy. Scenario CRTs derive
    /// B054BC=416; the Cell coordinate kernel owns all height arithmetic.
    pub(crate) fn trigger_waypoint_location(&self, index: i32) -> Option<[i32; 3]> {
        let cell = self.session.waypoint_cell(index)?;
        let terrain = self.resolved_terrain.as_ref()?;
        let cells = NativeCellQuery::canonical(terrain);
        let identity = cells.lookup(cell);
        let (x, y, z) = crate::sim::cell_kernel::native_cell_own_coords(identity, &cells)?;
        let z = if cells.flags(identity) & 0x500 != 0 {
            deck_surface_z(&cells, identity, z as i32)
        } else {
            z as i32
        };
        Some([x as i32, y as i32, z])
    }
}
