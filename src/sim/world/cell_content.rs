//! Live CellClass object-list and overlay queries shared by bridge, debris,
//! cell-entry and movement callbacks.
//!
//! Occupancy owns entity list order; TerrainObjectStore owns Terrain membership;
//! OverlayGrid owns a real cell's overlay. This view adds no stored links.
//! Callers choose the native before/after-hit continuation point
//! (BlowUpBridge47DD96 versus AnimClass423A83).

use super::Simulation;
use crate::map::cell_index::NativeCellIdentity;
use crate::sim::movement::locomotor::MovementLayer;
use crate::sim::occupancy::CellObjectMember;
use crate::sim::overlay_grid::OverlayCell;

impl Simulation {
    /// Cell+0x44 overlay index, +0x11E data and +0x50 wall owner of a cell
    /// already looked up, as Mark (0x005FC570), DestroyOverlay (0x00480CB0),
    /// ore growth and the bridge writers leave them during play. OverlayGrid
    /// owns a real cell's; the shared dummy keeps its own index and data.
    pub(crate) fn cell_overlay(&self, cell: NativeCellIdentity) -> OverlayCell {
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return OverlayCell::default();
        };
        match cell {
            NativeCellIdentity::Real(index) => {
                let c = &terrain.cells()[index];
                self.overlay_grid
                    .as_ref()
                    .map_or_else(OverlayCell::default, |grid| *grid.cell(c.rx, c.ry))
            }
            NativeCellIdentity::Dummy => {
                let (identity, state) = terrain.shared_cell_dummy().overlay_identity_state();
                OverlayCell {
                    overlay_id: u8::try_from(identity).ok(),
                    overlay_data: state,
                    wall_owner: None,
                    rubble_building_type: None,
                }
            }
        }
    }

    pub(crate) fn cell_objects(
        &self,
        cell: (u16, u16),
        layer: MovementLayer,
    ) -> impl Iterator<Item = CellObjectMember> + '_ {
        self.substrate.occupancy.cell_objects(
            cell.0,
            cell.1,
            layer,
            self.terrain_objects.members_at(cell),
        )
    }

    /// The head of a cell's ground list (`CellClass+0xE4`), the object
    /// `Tactical::PickObjectAtScreenPoint @ 0x006DA380` falls back to when no
    /// registered object is near the point (`0x006DA4B8..0x006DA4FB`).
    pub fn first_ground_cell_object(&self, cell: (u16, u16)) -> Option<CellObjectMember> {
        self.cell_objects(cell, MovementLayer::Ground).next()
    }

    pub(crate) fn cell_object_list_location(
        &self,
        object: CellObjectMember,
    ) -> Option<((u16, u16), MovementLayer)> {
        match object {
            CellObjectMember::Entity(id) => {
                let entity = self.substrate.entities.get(id)?;
                let cell = (entity.position.rx(), entity.position.ry());
                // Object+30 links the list the object's Mark put it in, which
                // follows the object's own layer query, not its path layer:
                // a landed Jumpjet is listed on the ground.
                let layer = self.substrate.occupancy.listed_layer(cell.0, cell.1, id)?;
                entity.lifecycle.cell_marked().then_some((cell, layer))
            }
            CellObjectMember::Terrain(id) => {
                let terrain = self.terrain_objects.objects().get(&id)?;
                terrain
                    .is_live()
                    .then_some((terrain.cell(), MovementLayer::Ground))
            }
        }
    }

    /// Read Object+30 at the caller's before/after-hit continuation. Movement
    /// replaces the link; a found removal clears it. Never retain a snapshot
    /// of all targets across a synchronous receiver.
    pub(crate) fn next_cell_object(&self, object: CellObjectMember) -> Option<CellObjectMember> {
        if let CellObjectMember::Terrain(id) = object {
            // A non-head Terrain bypassed by PointerExpired5F5262 can
            // retain Next after Limbo (RemoveContent47EAF7). An after-hit
            // walker still follows that retained Terrain successor.
            if let Some(next) = self.terrain_objects.next_in_cell(id) {
                return Some(CellObjectMember::Terrain(next));
            }
        }
        let (cell, layer) = self.cell_object_list_location(object)?;
        if matches!(object, CellObjectMember::Terrain(_)) {
            // The represented Terrain-before-Techno subsequence ends at
            // the first appended Building on a live object's ground list.
            return self
                .substrate
                .occupancy
                .first_building_on_layer(cell.0, cell.1, layer)
                .map(CellObjectMember::Entity);
        }
        let mut members = self.cell_objects(cell, layer);
        members.find(|candidate| *candidate == object)?;
        members.next()
    }
}
