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
    /// AreaDamage48A2C4..48A319 (original gamemd.exe): redraw, clear only
    /// Overlay identity, Recalc, AssignOrphaned/hierarchy, then70D4A0. The
    /// Cell pointer was looked up at489EA4, before bridge callbacks; a Dummy
    /// reads its current fields here and is never looked up a second time.
    pub(crate) fn remove_exploding_overlay(
        &mut self,
        cell: NativeCellIdentity,
        rules: &crate::rules::ruleset::RuleSet,
        registry: &crate::rules::overlay_types::OverlayTypeRegistry,
        effects: super::FrameEffects<'_>,
    ) -> bool {
        if !self
            .cell_overlay(cell)
            .overlay_id
            .and_then(|id| registry.flags(id))
            .is_some_and(|flags| flags.explodes)
        {
            return false;
        }
        let Some(terrain) = self.resolved_terrain.as_ref() else {
            return false;
        };
        let (x, y) = terrain.native_cell_coord(cell);
        let coord = (x as u16, y as u16);
        // CellRedraw486E70 unions the tactical rectangles and marks radar.
        // Tactical presentation rebuilds each frame; DirtyCells owns radar.
        self.dirty_cells.mark_cell(coord);
        match cell {
            NativeCellIdentity::Real(_) => {
                let (Some(grid), Some(terrain)) =
                    (self.overlay_grid.as_mut(), self.resolved_terrain.as_mut())
                else {
                    return false;
                };
                grid.clear_literal_overlay_identity(coord.0, coord.1);
                grid.recalculate_runtime_cell(terrain, registry, coord);
            }
            NativeCellIdentity::Dummy => {
                // Recalc47D2B8 returns immediately for the shared Dummy.
                terrain.shared_cell_dummy().write_overlay_identity(-1);
            }
        }
        let blocked = super::navigation::structure_blocked_among(
            &self.substrate.entities,
            &self.interner,
            rules,
            &[coord],
        );
        self.publish_cell_and_assign_orphaned_zone(rules, coord, Some(&blocked));
        self.stop_all_targeting_cell(coord.0, coord.1, rules, Some(registry), effects);
        true
    }

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

    /// `CellClass::GetContent`: the deck list (`+0xE8`) when the cell has a
    /// `BridgeHead` (`+0x140 & 0x100`), otherwise the ground list (`+0xE4`).
    #[cfg(feature = "phobos")]
    pub(crate) fn native_cell_content(
        &self,
        cell: (u16, u16),
    ) -> impl Iterator<Item = CellObjectMember> + '_ {
        let layer = if self.cell_has_bridge_head(cell) {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
        self.cell_objects(cell, layer)
    }

    /// `CellClass::ContainsBridge`: the cell's `BridgeHead` flag.
    #[cfg(feature = "phobos")]
    pub(crate) fn cell_has_bridge_head(&self, cell: (u16, u16)) -> bool {
        self.resolved_terrain.as_ref().is_some_and(|terrain| {
            crate::sim::cell_rect::get_cellclass_fallback(
                Some(terrain),
                i32::from(cell.0),
                i32::from(cell.1),
            )
            .bridge_flags_0x1180()
                & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                != 0
        })
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
                if !terrain.is_live() {
                    return None;
                }
                // CellAddContent47E8D2 appends Buildings to the actual Cell
                // list; FogCell486BA4/contact483495 follow Object+30. The
                // list identity of an alias differs from Get_Cell_Packed.
                let (x, y) = terrain.cell();
                let cell = self
                    .cell_grid()
                    .canonical_coord(i32::from(x), i32::from(y))?;
                Some((cell, MovementLayer::Ground))
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
