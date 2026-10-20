//! Live56DB70 repair callback. Keep the record owner and navigation's retained
//! input mirror synchronized without replacing the ordered hierarchy graph.
use super::*;
use crate::sim::pathfinding::zone_incremental::{
    ZoneBatchHost, recalculate_zone_batch, repair_zone_hierarchy_around_cell,
};

impl LivePublication<'_> {
    /// Original586990. The repair/walker owns the ordered input vector; this
    /// callback preserves its duplicates and never rebuilds base connectivity.
    pub(super) fn recalculate_bridge_zones(&mut self, cells: &[CellCoord]) -> Result<(), String> {
        recalculate_zone_batch(self, cells)
    }
    /// Original56C510, after the ramp walker has completed its synchronous
    /// terrain/constructor writes and requested connectivity. Recalc must
    /// already have projected each touched class and cached height. No hierarchy
    /// rebuild or group-derived activation is part of this callback.
    pub(super) fn rebuild_bridge_connectivity(&mut self) -> Result<(), String> {
        let sim = &mut self.sim;
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .ok_or("repair has no live terrain")?;
        let bridges = sim
            .bridge_state
            .as_ref()
            .ok_or("repair has no bridge record owner")?;
        let path = sim
            .path_grid
            .as_deref()
            .ok_or("repair has no live path owner")?;
        let zones = sim
            .zone_grid
            .as_mut()
            .ok_or("repair has no live zone owner")?;
        zones.rebuild_base_connectivity_preserving_hierarchy(
            path,
            terrain,
            bridges.endpoint_records(),
        );
        Ok(())
    }

    pub(super) fn validate_bridge_zones(&mut self, query: CellCoord) -> Result<bool, String> {
        let sim = &mut self.sim;
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .ok_or("repair has no live terrain")?;
        let bridges = sim
            .bridge_state
            .as_mut()
            .ok_or("repair has no bridge record owner")?;
        let zones = sim
            .zone_grid
            .as_mut()
            .ok_or("repair has no live zone owner")?;
        let path = sim
            .path_grid
            .as_deref()
            .ok_or("repair has no live path owner")?;
        let bounds = sim.playfield_bounds;
        let result = bridges.validate_repaired_zones(terrain, query, |record| {
            zones.activate_repaired_bridge(terrain, record, bounds)
        });
        // Even a partially completed callback has already changed its active
        // byte. Preserve the same record state in later navigation consumers.
        zones.retain_repaired_bridge_records(path, terrain, bridges.endpoint_records());
        result
    }
}

impl ZoneBatchHost for LivePublication<'_> {
    type Error = String;

    fn admitted(&mut self, coord: CellCoord) -> Result<bool, String> {
        let terrain = self
            .sim
            .resolved_terrain
            .as_ref()
            .ok_or("zone batch has no live terrain")?;
        Ok(crate::sim::cell_rect::cell_is_in_playfield_height_aware(
            (i32::from(coord.0), i32::from(coord.1)),
            self.sim.playfield_bounds,
            Some(terrain),
        ))
    }

    fn clear_fine_zone(&mut self, coord: CellCoord) -> Result<(), String> {
        let zones = self
            .sim
            .zone_grid
            .as_mut()
            .ok_or("zone batch has no live zone owner")?;
        let (base, hierarchy) = zones.base_and_hierarchy_mut();
        hierarchy.levels_mut()[0]
            .set_native_zone_at(coord, base.native_bridge_source_size(), 0)
            .ok_or_else(|| "zone batch coordinate has no native zone storage".into())
    }

    fn recalc_at(&mut self, coord: CellCoord) -> Result<(), String> {
        // Query and lookup are distinct native operations; both stamp a missing
        // Cell's dummy coordinate before the immediate47D2B0 dummy guard.
        let cell = self.lookup(coord);
        self.recalc_cell(cell, -1)
    }

    fn fine_zone(&self, coord: CellCoord) -> Result<u16, String> {
        self.sim
            .zone_grid
            .as_ref()
            .and_then(|zones| zones.hierarchy_zone_at_native(0, (coord.0 as u16, coord.1 as u16)))
            .ok_or_else(|| "zone batch has no native fine-zone storage".into())
    }

    fn patch_hierarchy(&mut self, coord: CellCoord) -> Result<(), String> {
        let sim = &mut self.sim;
        let terrain = sim
            .resolved_terrain
            .as_ref()
            .ok_or("zone batch has no live terrain")?;
        let bridges = sim
            .bridge_state
            .as_ref()
            .ok_or("zone batch has no bridge record owner")?;
        let zones = sim
            .zone_grid
            .as_mut()
            .ok_or("zone batch has no live zone owner")?;
        repair_zone_hierarchy_around_cell(
            zones,
            coord,
            sim.playfield_bounds,
            terrain,
            bridges.endpoint_records(),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("bridge_batch_native_tests.rs");
    use crate::rules::ini_parser::IniFile;
    use crate::rules::locomotor_type::MovementZone;
    use crate::sim::pathfinding::{PathGrid, zone_map::ZoneGrid};
    use std::sync::Arc;

    fn edge_state(sim: &Simulation) -> Vec<Vec<Vec<(u16, u8)>>> {
        let hierarchy = sim
            .zone_grid
            .as_ref()
            .unwrap()
            .hierarchy_for(MovementZone::Normal)
            .unwrap();
        (0..3)
            .map(|level| {
                let graph = hierarchy.level(level).unwrap();
                (0..graph.record_slot_count())
                    .map(|zone| {
                        graph
                            .edges(zone as u16)
                            .iter()
                            .map(|edge| (edge.neighbor, edge.flag))
                            .collect()
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn live_bridge_validation_recomputes_missing_records_and_survives_cache_reuse() {
        let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
        let mut terrain = ResolvedTerrainGrid::from_cells(
            16,
            16,
            (0..16)
                .flat_map(|y| {
                    (0..16).map(move |x| {
                        crate::sim::world::lifecycle_tests::common_raw_terrain_cell(x, y, 0, false)
                    })
                })
                .collect(),
        );
        terrain.test_set_high_bridge_set_starts(Some(100), Some(200));
        for (x, y) in [(5, 5), (10, 5)] {
            let cell = terrain.cell_mut_for_test(x, y).unwrap();
            cell.final_tile_index = 106;
            cell.final_sub_tile = 4;
        }
        let mut bridges =
            BridgeRuntimeState::from_resolved_terrain_with_map_size(&terrain, true, 1, (8, 8));
        // A bridge loaded already broken keeps its native kind0 record inactive.
        assert_eq!(bridges.endpoint_records().len(), 1);
        assert!(!bridges.endpoint_records()[0].active);
        bridges.test_set_endpoint_records(Vec::new());
        let mut path = PathGrid::from_resolved_terrain_with_bridges(&terrain, None);
        path.set_blocked(7, 7, true); // Retained structure blocking.
        let mut sim = Simulation::with_seed(31);
        sim.install_resolved_terrain_for_new_map(terrain);
        sim.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 8,
            off_fc: 0,
            off_100: 0,
            off_104: 8,
            off_108: 8,
        });
        sim.zone_grid = Some(ZoneGrid::build_with_native_bridge_geometry(
            &path,
            sim.resolved_terrain.as_ref().unwrap(),
            bridges.endpoint_records(),
            16,
            16,
            Some((8, 8)),
        ));
        sim.bridge_state = Some(bridges);
        sim.path_grid = Some(Arc::new(path.clone()));
        let before = edge_state(&sim);
        let mut host = LivePublication {
            frame_effects: crate::sim::world::FrameEffects::default(),
            sim: &mut sim,
            rules: &rules,
            registry: None,
            collapsed: false,
        };
        assert!(!host.validate_bridge_zones((5, 5)).unwrap());
        assert!(sim.bridge_state.as_ref().unwrap().endpoint_records()[0].active);
        let after = edge_state(&sim);
        let count =
            |state: &Vec<Vec<Vec<(u16, u8)>>>| state.iter().flatten().map(Vec::len).sum::<usize>();
        assert_eq!(count(&after), count(&before) + 18);
        let mut host = LivePublication {
            frame_effects: crate::sim::world::FrameEffects::default(),
            sim: &mut sim,
            rules: &rules,
            registry: None,
            collapsed: false,
        };
        assert!(!host.validate_bridge_zones((5, 5)).unwrap());
        assert_eq!(edge_state(&sim), after);
        sim.rebuild_zone_grid(&path);
        assert_eq!(
            edge_state(&sim),
            after,
            "navigation reuse erased native repair edge history"
        );
        assert_eq!(sim.path_grid().unwrap().cell(7, 7), path.cell(7, 7));
        let mut host = LivePublication {
            frame_effects: crate::sim::world::FrameEffects::default(),
            sim: &mut sim,
            rules: &rules,
            registry: None,
            collapsed: false,
        };
        host.rebuild_bridge_connectivity().unwrap();
        assert_eq!(
            edge_state(&sim),
            after,
            "base connectivity replaced repair hierarchy edges"
        );
        assert_eq!(sim.path_grid().unwrap().cell(7, 7), path.cell(7, 7));
    }
}
