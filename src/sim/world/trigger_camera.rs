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

    /// Scenario GetWaypointLocation68BD00: retain Map5657A0's receiver,
    /// call its Cell486840 GetCoords, then read its flags500 for the deck.
    /// Unlike the inline camera/animation path above, packed aliases use the
    /// selected Cell's own coordinate, and height reads never re-query it.
    /// Eleven original Steam3e81a617..08600 controls cover aliases, both deck
    /// flags, signed levels/slopes and the shared Dummy. Scenario CRTs derive
    /// B054BC=416; the Cell coordinate kernel owns all height arithmetic.
    pub(crate) fn trigger_waypoint_location(&self, index: i32) -> Option<[i32; 3]> {
        let cell = self.session.waypoint_cell(index)?;
        let terrain = self.resolved_terrain.as_ref()?;
        let cells = NativeCellQuery::canonical(terrain);
        let identity = cells.lookup(cell);
        let (x, y, z) = crate::sim::cell_kernel::native_cell_own_coords(identity, &cells)?;
        let z = (z as i32).wrapping_add(if cells.flags(identity) & 0x500 != 0 {
            BRIDGE_DECK_HEIGHT_LEPTONS
        } else {
            0
        });
        Some([x as i32, y as i32, z])
    }
}
