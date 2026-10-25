//! Enter5683C0/Exit5687F0 relookup and Recalc(-1) for ordinary track Mark.
//! The resident catalog currently admits ordinary nonanimated/non-Tunnel
//! cells in bridge theaters. Unsupported constructors remain an explicit
//! runtime Recalc gap; the caller retains the preceding list/raw writes.

use super::{
    Simulation,
    navigation::{self, NavigationCaches},
};
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;

impl Simulation {
    pub(crate) fn recalculate_track_cell(
        &mut self,
        coord: (u16, u16),
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
    ) {
        let Some(registry) = registry else {
            return;
        };
        let Some(index) = self
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.native_fixed_cell_index(coord.0 as i16, coord.1 as i16))
        else {
            return;
        };
        let Some(overlay) = self
            .overlay_grid
            .as_ref()
            .and_then(|grid| grid.finalized_map_cell(coord.0, coord.1))
        else {
            return;
        };
        let structure_blocked =
            self.substrate
                .occupancy
                .get(coord.0, coord.1)
                .is_some_and(|list| {
                    list.iter_layer(crate::sim::movement::locomotor::MovementLayer::Ground)
                        .filter_map(|entry| self.substrate.entities.get(entry.entity_id))
                        .any(|entity| {
                            navigation::marked_structure_movement_cells(
                                entity,
                                &self.interner,
                                rules,
                            )
                            .contains(&coord)
                        })
                });
        let terrain = self.resolved_terrain.as_mut().unwrap();
        let outcome = match terrain.recalc_resident_bridge_cell(
            index,
            overlay,
            -1,
            registry,
            self.playfield_bounds,
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                log::warn!("ordinary movement Recalc at {coord:?} is not admitted: {error}");
                return;
            }
        };
        self.overlay_grid
            .as_mut()
            .unwrap()
            .write_finalized_map_cell(coord.0, coord.1, outcome.finalized);
        if let Err(error) = (NavigationCaches {
            zones: &mut self.zone_grid,
            path: &mut self.path_grid,
            overlays: self.overlay_grid.as_ref(),
            playfield_bounds: self.playfield_bounds,
        })
        .publish_recalculated_cell_with_presence(terrain, coord, structure_blocked)
        {
            log::warn!("ordinary movement Recalc publication at {coord:?} failed: {error}");
        }
    }
}
