//! Live56EB80 adapter and the raw-tile tails of572230..573170.
//! Scalar stores are retained before another native callback can reenter.
//! The host admits stock middle TMP entries; generic dummy writes and other
//! source types require their own resident authority, not a synthetic fallback.

use super::*;
use crate::map::iso_tile_flood::{self, IsoTileFloodHost};

impl LivePublication<'_> {
    /// Original568E40/569760 and573540/570050 raw Cell+11B stores. Preserve
    /// the completed write before another callback can fail. These stores do
    /// not invoke47D2B0: cached class/height, cost rows and zone IDs stay intact.
    pub(super) fn write_raw_bridge_level(&mut self, cell: Cell, level: u8) -> Result<(), String> {
        let sim = &mut self.sim;
        let terrain = sim
            .resolved_terrain
            .as_mut()
            .ok_or("raw bridge height has no terrain")?;
        terrain.write_native_cell_level(cell, level);
        let Cell::Real(index) = cell else {
            return Ok(());
        };
        let resolved = &terrain.cells()[index];
        let coord = (resolved.rx, resolved.ry);
        self.retain_real_write(cell);
        let sim = &mut self.sim;
        crate::sim::world::navigation::NavigationCaches {
            terrain_costs: &mut sim.terrain_costs,
            zones: &mut sim.zone_grid,
            path: &mut sim.path_grid,
            playfield_bounds: sim.playfield_bounds,
        }
        .publish_current_path_cell(
            sim.resolved_terrain.as_ref().unwrap(),
            &sim.substrate.entities,
            &sim.interner,
            self.rules,
            coord,
        )
    }

    /// Shared47D2B0 entry for tile replacement and OverlayClass's common tail.
    pub(super) fn recalc_cell(&mut self, cell: Cell, level: i32) -> Result<(), String> {
        LiveTileFlood { publication: self }.recalc(cell, level)
    }

    pub(super) fn tile(&self, cell: Cell) -> i32 {
        match cell {
            Cell::Real(index) => self.terrain().cells()[index].final_tile_index,
            // Constructor47BBF0. No middle-family caller writes the dummy from
            // this sentinel: its raw gate and recursive old-tile test fail.
            Cell::Dummy => 0xffff,
        }
    }

    pub(super) fn subtile(&self, cell: Cell) -> u8 {
        match cell {
            Cell::Real(index) => self.terrain().cells()[index].final_sub_tile,
            Cell::Dummy => 0,
        }
    }

    pub(super) fn level(&self, cell: Cell) -> i8 {
        match cell {
            Cell::Real(index) => self.terrain().cells()[index].level as i8,
            Cell::Dummy => self.terrain().shared_cell_dummy().snapshot().level,
        }
    }

    pub(super) fn replace_tiles(
        &mut self,
        requested: CellCoord,
        replacement: i32,
        level_override: i32,
    ) -> Result<(), String> {
        iso_tile_flood::replace_connected(
            &mut LiveTileFlood { publication: self },
            requested,
            replacement,
            level_override,
        )
    }

    /// Read the raw tile only after the overlay-state branch and any recursive
    /// collapse there have completed. The two branches are independent.
    pub(super) fn perpendicular_tile_tail(
        &mut self,
        requested: CellCoord,
        retained: Cell,
        axis: Axis,
        phase: Phase,
        direction: u8,
        family: Family,
    ) -> Result<(), String> {
        let Some(keys) = super::super::family_rim_tiles(self.terrain(), family) else {
            return Ok(());
        };
        let relative = self.tile(retained).wrapping_sub(keys.base).wrapping_add(1);
        let side_a = matches!(phase, Phase::DamageA | Phase::CollapseA);
        let pavement = match (axis, side_a) {
            (Axis::NS, true) => keys.bottom_right,
            (Axis::NS, false) => keys.top_left,
            (Axis::EW, true) => keys.bottom_left,
            (Axis::EW, false) => keys.top_right,
        };
        if pavement.contains(&relative) {
            self.pavement_at(requested, true);
            return Ok(());
        }
        let middle = keys.middle[usize::from(axis == Axis::EW)];
        let variant = relative.wrapping_sub(middle);
        let replacement = match (phase, variant) {
            (Phase::DamageA, 0) => Some(1),
            (Phase::DamageB, 0) => Some(2),
            (Phase::DamageA, 2)
            | (Phase::DamageB, 1)
            | (Phase::CollapseA, 0 | 2)
            | (Phase::CollapseB, 0 | 1) => Some(3),
            _ => None,
        };
        let first = keys.base.wrapping_add(middle).wrapping_sub(1);
        if let Some(replacement) = replacement {
            return self.replace_tiles(requested, first.wrapping_add(replacement), -1);
        }
        if variant != 3 || !matches!(phase, Phase::CollapseA | Phase::CollapseB) {
            return Ok(());
        }

        self.perpendicular(requested, axis, phase, direction, family);
        // Native rereads the retained receiver's +11A after recursion. Fallout
        // uses the stack coordinate, with the half-footprint adjustment below.
        let sub = self.subtile(retained);
        let center = match axis {
            Axis::NS => (requested.0.wrapping_sub(i16::from(sub & 1)), requested.1),
            Axis::EW => (requested.0, requested.1.wrapping_sub(i16::from(sub >= 5))),
        };
        let footprint = match axis {
            Axis::NS => [
                center,
                (center.0, center.1.wrapping_sub(1)),
                (center.0, center.1.wrapping_add(1)),
            ],
            Axis::EW => [
                (center.0.wrapping_sub(1), center.1),
                center,
                (center.0.wrapping_add(1), center.1),
            ],
        };
        for coord in footprint {
            let cell = self.lookup(coord);
            self.fallout(cell);
        }
        // All three synchronous callbacks precede a fresh level lookup.
        let sample = self.lookup(center);
        let level = i32::from(self.level(sample)) - 4;
        self.replace_tiles(requested, first.wrapping_add(4), level)
    }
}

struct LiveTileFlood<'a, 'world> {
    publication: &'a mut LivePublication<'world>,
}

impl IsoTileFloodHost for LiveTileFlood<'_, '_> {
    type Cell = Cell;
    type Error = String;

    fn lookup(&mut self, requested: CellCoord) -> Cell {
        self.publication.lookup(requested)
    }

    fn tile(&self, cell: Cell) -> i32 {
        self.publication.tile(cell)
    }

    fn write_tile(&mut self, cell: Cell, tile: i32) -> Result<(), String> {
        let Cell::Real(index) = cell else {
            return Err(
                "dummy tile write is outside the admitted stock middle-bridge closure".into(),
            );
        };
        let registry = self
            .publication
            .registry
            .ok_or("missing live bridge overlay registry")?;
        let coord = self.publication.real_coord(cell).unwrap();
        let overlay = self
            .publication
            .sim
            .overlay_grid
            .as_ref()
            .and_then(|grid| grid.finalized_map_cell(coord.0, coord.1))
            .ok_or("missing live bridge overlay cell")?;
        self.publication
            .sim
            .resolved_terrain
            .as_mut()
            .unwrap()
            .write_resident_bridge_tile(index, tile, overlay, registry)
            .map_err(|error| error.to_string())?;
        self.publication.retain_real_write(cell);
        Ok(())
    }

    fn recalc(&mut self, cell: Cell, level_override: i32) -> Result<(), String> {
        let Cell::Real(index) = cell else {
            // The original47D2B0 immediate dummy guard has no side effects.
            return Ok(());
        };
        let registry = self
            .publication
            .registry
            .ok_or("missing live bridge overlay registry")?;
        let coord = self.publication.real_coord(cell).unwrap();
        let sim = &mut self.publication.sim;
        let overlay = sim
            .overlay_grid
            .as_ref()
            .and_then(|grid| grid.finalized_map_cell(coord.0, coord.1))
            .ok_or("missing live bridge overlay cell")?;
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        let outcome = terrain
            .recalc_resident_bridge_cell(
                index,
                overlay,
                level_override,
                registry,
                sim.playfield_bounds,
            )
            .map_err(|error| error.to_string())?;
        // Original47D2B0 publishes Map+68 class/height and Map+70 height
        // before returning to the next ordered repair callback. Publication
        // must survive a later presentation failure and must not rebuild IDs.
        // Recalc owns the finalized pair; never leave a later reader on the
        // input overlay when native has removed it during this invocation.
        sim.overlay_grid.as_mut().unwrap().write_finalized_map_cell(
            coord.0,
            coord.1,
            outcome.finalized,
        );
        crate::sim::world::navigation::NavigationCaches {
            terrain_costs: &mut sim.terrain_costs,
            zones: &mut sim.zone_grid,
            path: &mut sim.path_grid,
            playfield_bounds: sim.playfield_bounds,
        }
        .publish_recalculated_cell(
            terrain,
            &sim.substrate.entities,
            &sim.interner,
            self.publication.rules,
            coord,
        )?;
        self.publication.retain_real_write(cell);
        let sim = &mut self.publication.sim;
        sim.resolved_terrain
            .as_mut()
            .unwrap()
            .refresh_resident_bridge_presentation(index, &mut || sim.main_rng.next_u32())
            .map_err(|error| error.to_string())?;
        self.publication.retain_real_write(cell);
        Ok(())
    }

    fn radar(&mut self, cell: Cell) {
        self.publication.radar(cell);
    }

    fn initial_screen(&mut self, _: CellCoord, _: Cell) {
        // VERA redraws every frame. The live terrain-to-presentation handoff
        // must consume these mutations before this mechanism's PR can close.
    }
}
