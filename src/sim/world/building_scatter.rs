//! Synchronous Building evacuation, after capture or bridge repair.
//!
//! Native Building4576F0 visits the Infantry registry and calls
//! Infantry51D0D0(NULL,true,true). The successful FNPC arm installs a Cell
//! destination and invokes only locomotor Process before the next receiver.

use super::{FrameAdvanceError, Simulation};
use crate::map::cell_index::NativeCellIdentity;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::components::NavTargetRef;
use crate::sim::movement::{ScatterFlags, locomotor::MovementLayer};
use crate::sim::world::FrameEffects;

impl Simulation {
    fn building_scatter_error(&self, id: u64, cause: String) -> FrameAdvanceError {
        FrameAdvanceError::new(self.session.tick, self.session.binary_frame, id, cause)
    }

    pub(super) fn scatter_building_infantry(
        &mut self,
        building: u64,
        rules: &RuleSet,
        registry: Option<&OverlayTypeRegistry>,
        frame_effects: FrameEffects<'_>,
    ) -> Result<bool, FrameAdvanceError> {
        let mut index = 0;
        let mut changed = false;
        // Ordinary +F8 Uninit5F65F0 retains the Infantry class entry until
        // deferred destruction (517EA2). A direct removal would compact the
        // indexed registry; incrementing the cursor still skips its successor.
        while let Some(id) = self.substrate.entities.infantry_registry_at(index) {
            index += 1;
            let coord = self
                .foot_navigation_coordinate(id)
                .map_err(|cause| self.building_scatter_error(id, cause))?;
            let terrain = self.resolved_terrain.as_ref().ok_or_else(|| {
                self.building_scatter_error(
                    id,
                    "Building coordinate requires live map cells".into(),
                )
            })?;
            // 457716..45772A: +4C, Map565730, first ground Building47C520.
            let cell =
                terrain.native_cell_identity(((coord.x / 256) as i16, (coord.y / 256) as i16));
            let first_building = match cell {
                NativeCellIdentity::Real(_) => {
                    let xy = terrain.native_cell_coord(cell);
                    self.substrate.occupancy.first_building_on_layer(
                        xy.0 as u16,
                        xy.1 as u16,
                        MovementLayer::Ground,
                    )
                }
                NativeCellIdentity::Dummy => None,
            };
            let e = self
                .substrate
                .entities
                .get(id)
                .expect("query does not remove listener");
            if !e.lifecycle.object_alive() || first_building != Some(building) {
                continue;
            }
            if e.navigation
                .nav_com()
                .is_some_and(|target| target != NavTargetRef::object(building))
            {
                continue;
            }
            changed |= self
                .scatter(
                    id,
                    None,
                    ScatterFlags::new(true, true),
                    rules,
                    registry,
                    frame_effects,
                )
                .map_err(|cause| self.building_scatter_error(id, cause))?;
        }
        Ok(changed)
    }
}
