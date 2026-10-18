//! Original5FC380/5FC570 for the four active bridge reconstruction types.
//! Evidence: tools/spatial_oracle/bridge_constructor. The caller supplies the
//! registered stock OverlayType and frame-1, and retains partial synchronous
//! writes if an unsupported resident Recalc input prevents continuation.

use super::*;
use crate::sim::world::load_object_lifecycle::LoadOverlayHandle;

#[cfg(test)]
#[path = "bridge_constructor_publication_tests.rs"]
mod tests;

impl LivePublication<'_> {
    pub(super) fn construct_bridge_overlay(
        &mut self,
        requested: CellCoord,
        overlay: u8,
        frame: i32,
    ) -> Result<LoadOverlayHandle, String> {
        if !matches!(overlay, 24 | 25 | 237 | 238) || frame != -1 {
            return Err(
                "bridge reconstruction requires native type24/25/237/238 and frame-1".into(),
            );
        }
        let registry = self
            .registry
            .ok_or("missing live bridge OverlayType registry")?;
        let flags = registry
            .flags(overlay)
            .ok_or("missing bridge OverlayType")?;
        // Active RULESMD's BRIDGE1/2 and BRIDGEB1/2 have Clear land and no
        // Wall, Crate or CellAnim branch. Reject a different type configuration
        // explicitly until its extra constructor callbacks are delivered.
        if flags.land != crate::rules::terrain_rules::LandType::Clear
            || flags.wall
            || flags.crate_type
            || flags.cell_anim.is_some()
        {
            return Err("bridge OverlayType requires additional Mark callbacks".into());
        }
        if self.sim.native_unique_ids.is_none() {
            return Err("live bridge constructor has no Scenario native-ID continuation".into());
        }
        let id = self.sim.allocate_stable_id();
        let handle = self
            .sim
            .load_objects
            .construct_overlay(id, (requested.0 as u16, requested.1 as u16), || {
                self.sim.native_unique_ids.as_mut().unwrap().next_id()
            })
            .map_err(|error| error.to_string())?;

        // Startup5FC310 initializes EmptyCell to(0,0). This gate occurs AFTER
        // Object construction/ID/registry append and before any map lookup.
        if requested == (0, 0) {
            self.sim
                .load_objects
                .finish_unrevealed_survivor(handle)
                .map_err(|error| error.to_string())?;
            return Ok(handle);
        }
        //5FC42D..5FC479 converts signed CellStruct to a world center, then
        // divides by256 toward zero for the Terrain gate and virtual Mark.
        // Negative coordinates may therefore alias a different real slot.
        let world = [
            i32::from(requested.0) * 256 + 128,
            i32::from(requested.1) * 256 + 128,
        ];
        let lookup = ((world[0] / 256) as i16, (world[1] / 256) as i16);
        let cell = self.lookup(lookup);
        let terrain_present = self.real_coord(cell).is_some_and(|coord| {
            self.sim
                .production
                .terrain_object_cells
                .contains_key(&coord)
        });
        if terrain_present {
            self.sim
                .load_objects
                .finish_unrevealed_survivor(handle)
                .map_err(|error| error.to_string())?;
            return Ok(handle);
        }
        self.sim
            .load_objects
            .begin_mark(handle, None)
            .map_err(|error| error.to_string())?;
        self.sim
            .tactical_dirty_cells
            .push((lookup.0 as u16, lookup.1 as u16));
        // Reveal's virtual Mark obtains its own current CellClass receiver.
        let cell = self.lookup(lookup);
        let slope = match cell {
            Cell::Real(index) => self.terrain().cells()[index].slope_type,
            Cell::Dummy => self.terrain().shared_cell_dummy().snapshot().slope_type,
        };
        if slope > 4 {
            self.sim
                .load_objects
                .finish_slope_survivor(handle)
                .map_err(|error| error.to_string())?;
            return Ok(handle);
        }

        //47E040 and47E470 have identical decoded instructions after internal
        // branch relocation; their absolute callees also match. Preserve each
        // scalar store/lookup through the shared publisher, including aliases.
        publication::set_bridge_direction(
            self,
            cell,
            if matches!(overlay, 24 | 237) { 0 } else { 6 },
            true,
        );
        let current = match cell {
            Cell::Real(index) => self.terrain().cells()[index].bridge_facts.overlay_id,
            Cell::Dummy => self.terrain().shared_cell_dummy().overlay_fields().0,
        };
        let overridden = current
            .and_then(|id| registry.flags(id))
            .is_some_and(|f| f.overrides);
        if !overridden {
            let state = self.terrain().native_cell_state(cell);
            match cell {
                Cell::Real(index) => {
                    let coord = self.real_coord(cell).unwrap();
                    let sim = &mut self.sim;
                    let grid = sim
                        .overlay_grid
                        .as_mut()
                        .ok_or("missing live bridge overlay grid")?;
                    if !grid.write_crate_mark_fields(
                        sim.resolved_terrain.as_mut().unwrap(),
                        registry,
                        coord.0,
                        coord.1,
                        overlay,
                        state,
                    ) {
                        return Err("live bridge Mark could not publish its real overlay".into());
                    }
                    self.retain_real_write(Cell::Real(index));
                }
                Cell::Dummy => self
                    .terrain()
                    .shared_cell_dummy()
                    .write_overlay_identity(i32::from(overlay)),
            }
        }
        self.recalc_cell(cell, -1)?;
        self.sim
            .load_objects
            .finish_common(handle, &mut self.sim.substrate.pending_delete)
            .map_err(|error| error.to_string())?;
        Ok(handle)
    }
}
