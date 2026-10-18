//! Authored camera coordinates use the simulation's retained waypoint and
//! CellClass query authorities before the frame hash is finalized.

use super::Simulation;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::sim::components::DriveCoord;
use crate::sim::movement::ground_pose::query_ground_height;
use crate::util::lepton::BRIDGE_DECK_HEIGHT_LEPTONS;

impl Simulation {
    /// Original TActionExecute6DD8B0 cases48/112: GetWaypoint68BCC0,
    /// signed cell center, Ground578080, then Map5657A0's short-circuit
    /// bridge flags100/400. Ground and packed-cell lookups are distinct;
    /// invalid coordinates stamp the same process CellClass Dummy in order.
    /// Native executable controls for the downstream FocusOn/TacticalAI:
    /// tools/input_oracle/fast_scroll.{py,json,meta.json}.
    /// Case99's GetWaypointLocation68BD00 (Map5657A0, CellGetCoords486840,
    /// then the bridge height under flags500) reaches the same point: the
    /// cell's own coordinate is its stamped centre and 486840's height is
    /// 578080's.
    pub(crate) fn trigger_waypoint_coord(&self, index: i32) -> Option<[i32; 3]> {
        let cell = self.session.waypoint_cell(index)?;
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
        let mut z = query_ground_height(&cells, DriveCoord { x, y, z: 0 })
            .expect("authored camera ground query requires a valid native slope");
        let first = cells.lookup(cell);
        if cells.flags(first) & 0x100 != 0 || cells.flags(cells.lookup(cell)) & 0x400 != 0 {
            z = z.wrapping_add(BRIDGE_DECK_HEIGHT_LEPTONS);
        }
        Some([x, y, z])
    }
}
