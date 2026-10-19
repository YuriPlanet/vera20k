//! Original5FC380 for the four active bridge reconstruction types, whose
//! Reveal reaches the one `OverlayClass::Mark @ 0x005FC570` port
//! ([`crate::map::overlay_mark`]) on this publication's live scalar cells.
//! Evidence: tools/spatial_oracle/bridge_constructor. The caller supplies the
//! registered stock OverlayType and frame-1, and retains partial synchronous
//! writes if an unsupported resident Recalc input prevents continuation.

use super::*;
use crate::map::bridge_facts::BridgeStampFamily;
use crate::map::overlay_mark::{
    MarkCrateImages, NO_OVERLAY, OverlayCellAnim, OverlayMarkHost, OverlayMarkOutcome,
    OverlayMarkRequest, mark_overlay,
};
use crate::rules::locomotor_type::SpeedType;
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
        // Mark's wall arm asks Is_Clear_To_Build and the wall owners, which
        // this publication does not host. Active RULESMD's BRIDGE1/2 and
        // BRIDGEB1/2 are not walls; reject that configuration before the
        // constructor's effects.
        if flags.wall {
            return Err("bridge OverlayType requires the wall Mark callbacks".into());
        }
        let rules = self.rules;
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
        let request = OverlayMarkRequest {
            overlay_id: overlay,
            flags,
            cell: lookup,
            location: [world[0], world[1], 0],
            registry,
            tiberium_types: Some(&rules.tiberium_types),
            crate_images: MarkCrateImages::resolve(&rules.crate_rules, registry),
        };
        let mut host = BridgeMarkHost {
            publication: self,
            registry,
        };
        match mark_overlay(&mut host, &request)? {
            OverlayMarkOutcome::SlopeRefused => {
                self.sim
                    .load_objects
                    .finish_slope_survivor(handle)
                    .map_err(|error| error.to_string())?;
                return Ok(handle);
            }
            OverlayMarkOutcome::WallRefused => {
                unreachable!("bridge reconstruction admits no wall OverlayType")
            }
            OverlayMarkOutcome::Marked => {}
        }
        self.sim
            .load_objects
            .finish_common(handle, &mut self.sim.substrate.pending_delete)
            .map_err(|error| error.to_string())?;
        Ok(handle)
    }
}

/// The bridge publication as the CellClass surface of `OverlayClass::Mark`.
/// Its overlay identity and data are the live scalar cells the publication
/// reads (`bridge_facts`), which the OverlayGrid mirrors on every store.
struct BridgeMarkHost<'p, 'a> {
    publication: &'p mut LivePublication<'a>,
    registry: &'a crate::rules::overlay_types::OverlayTypeRegistry,
}

impl OverlayMarkHost for BridgeMarkHost<'_, '_> {
    type Error = String;

    fn cell(&mut self, coord: (i16, i16)) -> Cell {
        self.publication.lookup(coord)
    }

    fn coord(&self, cell: Cell) -> (i16, i16) {
        self.publication.coord(cell)
    }

    fn in_bounds(&mut self, coord: (i16, i16)) -> Result<bool, String> {
        Ok(self.publication.sim.map_cell_in_bounds(coord))
    }

    fn slope(&self, cell: Cell) -> u8 {
        self.publication.terrain().native_cell_ground_fields(cell).1
    }

    fn identity(&self, cell: Cell) -> i32 {
        let terrain = self.publication.terrain();
        let identity = match cell {
            Cell::Real(index) => terrain.cells()[index].bridge_facts.overlay_id,
            Cell::Dummy => terrain.shared_cell_dummy().overlay_fields().0,
        };
        identity.map_or(NO_OVERLAY, i32::from)
    }

    fn data(&self, cell: Cell) -> u8 {
        self.publication.state(cell)
    }

    fn write_identity(&mut self, cell: Cell, overlay_id: u8) -> Result<(), String> {
        let registry = self.registry;
        let Some((x, y)) = self.publication.real_coord(cell) else {
            self.publication
                .terrain()
                .shared_cell_dummy()
                .write_overlay_identity(i32::from(overlay_id));
            return Ok(());
        };
        let sim = &mut *self.publication.sim;
        let grid = sim
            .overlay_grid
            .as_mut()
            .ok_or("missing live bridge overlay grid")?;
        if !grid.write_mark_identity(sim.resolved_terrain.as_mut(), registry, x, y, overlay_id) {
            return Err("live bridge Mark could not publish its real overlay".into());
        }
        self.publication.retain_real_write(cell);
        Ok(())
    }

    fn write_data(&mut self, cell: Cell, data: u8) -> Result<(), String> {
        self.publication.write_state(cell, data);
        Ok(())
    }

    fn scenario_init(&self) -> bool {
        self.publication.sim.object_placement_scope_active()
    }

    fn next_scenario_raw(&mut self) -> u32 {
        self.publication.sim.scenario_rng.next_u32()
    }

    fn passable(&mut self, cell: Cell, speed: SpeedType, ignore: bool) -> Result<bool, String> {
        Ok(crate::sim::overlay_mark_host::mark_cell_passable(
            self.publication.sim,
            cell,
            speed,
            ignore,
        ))
    }

    fn set_bridge_direction(
        &mut self,
        anchor: Cell,
        _family: BridgeStampFamily,
        direction: u8,
    ) -> Result<(), String> {
        //47E040 and47E470 have identical decoded instructions after internal
        // branch relocation; their absolute callees also match. Preserve each
        // scalar store/lookup through the shared publisher, including aliases.
        publication::set_bridge_direction(self.publication, anchor, direction, true);
        Ok(())
    }

    fn wall_clear_to_build(&mut self, _anchor: Cell) -> Result<bool, String> {
        Err("bridge reconstruction hosts no wall Mark".into())
    }

    fn wall_cleanup(&mut self, _anchor: Cell) -> Result<(), String> {
        Err("bridge reconstruction hosts no wall Mark".into())
    }

    fn merge_wall_zones(&mut self, _anchor: Cell) -> Result<(), String> {
        Err("bridge reconstruction hosts no wall Mark".into())
    }

    fn increment_wall_neighbor_count(&mut self, _cell: Cell) -> Result<(), String> {
        Err("bridge reconstruction hosts no wall Mark".into())
    }

    fn spawn_cell_anim(&mut self, request: OverlayCellAnim<'_>) -> Result<(), String> {
        let rules = self.publication.rules;
        let ground = crate::sim::overlay_mark_host::OverlayCellAnimGround::read(
            self.publication.terrain(),
            &request,
        );
        let sim = &mut *self.publication.sim;
        let lighting = sim
            .session
            .lighting()
            .profile(crate::sim::scenario_session::ScenarioLightingProfile::Normal)
            .into();
        sim.construct_overlay_cell_anim(
            rules,
            request,
            ground,
            lighting,
            crate::sim::anim_class::AnimConstructorInput::Runtime,
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    }

    fn recalc(&mut self, cell: Cell) -> Result<(), String> {
        self.publication.recalc_cell(cell, -1)
    }
}
