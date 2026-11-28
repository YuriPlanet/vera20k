//! Enter5683C0/Exit5687F0 relookup and Recalc(-1) for ordinary track Mark.
//! The resident catalog currently admits ordinary nonanimated/non-Tunnel
//! cells in bridge theaters. Unsupported constructors remain an explicit
//! runtime Recalc gap; the caller retains the preceding list/raw writes.

use super::{Simulation, navigation::NavigationCaches};
use crate::rules::overlay_types::OverlayTypeRegistry;

impl Simulation {
    pub(crate) fn recalculate_track_cell(
        &mut self,
        coord: (u16, u16),
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(registry) = registry else {
            return;
        };
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return;
        };
        let Some(index) = terrain.native_fixed_cell_index(coord.0 as i16, coord.1 as i16) else {
            return;
        };
        let cell = &terrain.cells()[index];
        let coord = (cell.rx, cell.ry);
        let Some(overlay) = self
            .overlay_grid
            .as_ref()
            .and_then(|grid| grid.finalized_map_cell(coord.0, coord.1))
        else {
            return;
        };
        // Building Mark publishes its movement blockers synchronously. A Foot's
        // Recalc keeps that presence at the real receiver, including foundation aliases.
        let structure_blocked = self.structure_navigation_cells.contains(&coord);
        let terrain = self.resolved_terrain.as_mut().unwrap();
        let outcome = match terrain.recalc_resident_bridge_cell(
            index,
            overlay,
            -1,
            registry,
            *self.playfield_bounds,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                log::warn!("ordinary movement Recalc at {coord:?} is not admitted: {error}");
                return;
            }
        };
        // A Recalc that leaves the cell's pair as it is takes neither the
        // overlay grid nor, for a current cell, the navigation caches for
        // writing (`util::forked`).
        if !self
            .overlay_grid
            .as_ref()
            .unwrap()
            .holds_finalized_map_cell(coord.0, coord.1, outcome.finalized)
        {
            self.overlay_grid
                .as_mut()
                .unwrap()
                .write_finalized_map_cell(coord.0, coord.1, outcome.finalized);
        }
        if let Err(error) = (NavigationCaches {
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            overlays: self.overlay_grid.as_ref(),
            playfield_bounds: *self.playfield_bounds,
        })
        .publish_recalculated_cell_with_presence(terrain, coord, structure_blocked)
        {
            log::warn!("ordinary movement Recalc publication at {coord:?} failed: {error}");
        }
    }
}
