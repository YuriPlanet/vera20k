//! Bridge flag stamps on real cells and the shared dummy, runtime bridge
//! overlay identity, and the serialized `CellClass+0x140` bridge subset.

use super::native_cell::native_resolved_cell_index;
use super::*;

const UNALLOCATED_REAL_CELL_BRIDGE_FLAGS: u16 = u16::MAX;

/// Serialized value authority for the exact `CellClass+0x140 & 0x1D80`
/// subset of every allocated real cell.
///
/// Native `CellClass::Load` restores real object flag words directly, while
/// `MapClass::Resize` reconstructs only the process-global dummy. Rust keeps
/// the derived terrain grid out of Scenario serialization, so this compact
/// aligned projection carries the five future-affecting bits without retaining
/// setter history. `u16::MAX` marks native-unallocated slots; every other
/// value is exactly one masked `0x1D80` subset. The historical type/field name
/// remains1180 for schema continuity; snapshot141 rejects pre-restamp saves.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
pub struct RealCellBridgeFlags0x1180 {
    width: u16,
    height: u16,
    flags_or_unallocated: Vec<u16>,
}

impl RealCellBridgeFlags0x1180 {
    fn from_grid(grid: &ResolvedTerrainGrid) -> Self {
        let flags_or_unallocated = grid
            .cells
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                if grid
                    .native_allocated
                    .as_deref()
                    .is_none_or(|mask| mask.get(index).copied().unwrap_or(false))
                {
                    (cell.bridge_facts.raw_flags & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK) as u16
                } else {
                    UNALLOCATED_REAL_CELL_BRIDGE_FLAGS
                }
            })
            .collect();
        Self {
            width: grid.width,
            height: grid.height,
            flags_or_unallocated,
        }
    }

    fn matches_grid_shape(&self, grid: &ResolvedTerrainGrid) -> bool {
        self.width == grid.width
            && self.height == grid.height
            && self.flags_or_unallocated.len() == grid.cells.len()
    }

    pub(crate) fn set_allocated_cell(&mut self, index: usize, flags: u32) {
        let Some(slot) = self.flags_or_unallocated.get_mut(index) else {
            debug_assert!(false, "real-cell bridge flag update index must be in range");
            return;
        };
        debug_assert_ne!(*slot, UNALLOCATED_REAL_CELL_BRIDGE_FLAGS);
        *slot = (flags & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK) as u16;
    }
}

/// Route one exact `SetBridgeDirection_*` transaction through native fixed
/// indexing. A real slot mutates its retained facts; every miss stamps and
/// mutates the same process-global dummy identity.
pub(super) fn apply_native_bridge_flag_stamp_to_parts(
    cells: &mut [ResolvedTerrainCell],
    width: u16,
    height: u16,
    native_allocated: Option<&[bool]>,
    shared_cell_dummy: &SharedCellDummy,
    stamp: BridgeFlagStamp,
    map_family: Option<BridgeStampFamily>,
) -> Vec<(usize, u32)> {
    let Some(slots) = stamp.slots() else {
        return Vec::new();
    };
    let anchor = native_resolved_cell_index(
        width,
        height,
        native_allocated,
        cells.len(),
        i32::from(stamp.anchor.0),
        i32::from(stamp.anchor.1),
    )
    .map_or(NativeCellIdentity::Dummy, NativeCellIdentity::Real);
    let mut real_cell_updates = Vec::with_capacity(slots.len());
    for (slot, requested) in slots {
        let Some((x, y)) = requested else {
            continue;
        };
        if let Some(index) =
            native_resolved_cell_index(width, height, native_allocated, cells.len(), x, y)
        {
            let facts = &mut cells[index].bridge_facts;
            if let Some(family) = map_family {
                crate::map::bridge_facts::apply_bridge_fact_slot(
                    facts,
                    slot,
                    BridgeAnchorRelation {
                        anchor: (stamp.anchor.0 as u16, stamp.anchor.1 as u16),
                        slot,
                        family,
                        direction: stamp.direction,
                    },
                    stamp.set,
                );
                if slot.writes_native_anchor() {
                    facts.native_anchor = stamp.set.then_some(anchor);
                }
            } else {
                crate::map::bridge_facts::apply_retained_cellclass_bridge_slot(
                    &mut facts.raw_flags,
                    slot,
                    stamp.set,
                    stamp.direction,
                );
            }
            real_cell_updates.push((index, facts.raw_flags & RETAINED_CELLCLASS_BRIDGE_FLAG_MASK));
        } else {
            shared_cell_dummy.stamp_coord(x, y);
            if let Some(family) = map_family {
                shared_cell_dummy.apply_full_bridge_flag_slot(slot, stamp, family, anchor);
            } else {
                shared_cell_dummy.apply_retained_bridge_flag_slot(slot, stamp.set, stamp.direction);
            }
        }
    }
    real_cell_updates
}

pub(super) fn refresh_runtime_bridge_projection(
    cell: &mut ResolvedTerrainCell,
    structural_removed: bool,
) {
    let facts = cell.bridge_facts;
    if facts.has_structural_bridge() {
        cell.has_bridge_deck = true;
        cell.bridge_walkable = !cell.terrain_object_blocks && !cell.overlay_blocks;
        cell.bridge_deck_level = cell.level.saturating_add(4);
    } else if structural_removed
        || (facts.family != BridgeStampFamily::None
            && cell
                .bridge_layer
                .as_ref()
                .is_some_and(|layer| layer.direction != BridgeDirection::Low))
    {
        cell.has_bridge_deck = false;
        cell.bridge_walkable = false;
        cell.bridge_deck_level = cell.level;
    }
    if facts.has_transition_flag() {
        cell.bridge_transition = true;
    }
}

impl ResolvedTerrainGrid {
    /// Give every fixture deck the CellClass bit0x100 a loaded deck carries.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_mark_decks_structural(mut self) -> Self {
        for cell in &mut self.cells {
            if cell.has_bridge_deck {
                cell.bridge_facts.raw_flags |= BRIDGE_FLAG_STRUCTURAL;
            }
        }
        self
    }

    /// Apply one represented runtime `SetBridgeDirection_*` flag transaction
    /// through the same real-or-dummy seam used by map load.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn apply_runtime_bridge_flag_stamp(
        &mut self,
        stamp: BridgeFlagStamp,
    ) -> Vec<(usize, u32)> {
        apply_native_bridge_flag_stamp_to_parts(
            &mut self.cells,
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            &self.shared_cell_dummy,
            stamp,
            None,
        )
    }

    /// Apply one authored high-bridge setter slot in native call order. The
    /// caller synchronizes the slot's `Cell+0x11E` write on the live overlay
    /// surface immediately after this flag/fact mutation, without repeating a
    /// fixed-grid lookup or restamping the shared dummy.
    pub(crate) fn apply_authored_bridge_flag_slot(
        &mut self,
        anchor: NativeCellIdentity,
        stamp: BridgeFlagStamp,
        family: crate::map::bridge_facts::BridgeStampFamily,
        slot: crate::map::bridge_facts::BridgeStampSlot,
        requested: Option<(i32, i32)>,
    ) -> Option<usize> {
        let (x, y) = requested?;
        if let Some(index) = native_resolved_cell_index(
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            self.cells.len(),
            x,
            y,
        ) {
            crate::map::bridge_facts::apply_bridge_fact_slot(
                &mut self.cells[index].bridge_facts,
                slot,
                BridgeAnchorRelation {
                    anchor: (stamp.anchor.0 as u16, stamp.anchor.1 as u16),
                    slot,
                    family,
                    direction: stamp.direction,
                },
                stamp.set,
            );
            if slot.writes_native_anchor() {
                self.cells[index].bridge_facts.native_anchor = stamp.set.then_some(anchor);
            }
            Some(index)
        } else {
            self.shared_cell_dummy.stamp_coord(x, y);
            self.shared_cell_dummy
                .apply_full_bridge_flag_slot(slot, stamp, family, anchor);
            None
        }
    }

    /// Apply the high-anchor `OverlayClass::Mark` setter and refresh the
    /// Rust-native bridge projection carried beside the literal CellClass
    /// flag word. Unlike damage/repair runtime setters, a newly constructed
    /// anchor establishes its family/anchor relations at this call site.
    pub fn apply_runtime_bridge_mark_stamp(
        &mut self,
        stamp: BridgeFlagStamp,
        family: BridgeStampFamily,
    ) -> Vec<(usize, u32)> {
        let updates = apply_native_bridge_flag_stamp_to_parts(
            &mut self.cells,
            self.width,
            self.height,
            self.native_allocated.as_deref(),
            &self.shared_cell_dummy,
            stamp,
            Some(family),
        );
        for &(index, _) in &updates {
            if let Some(cell) = self.cells.get_mut(index) {
                refresh_runtime_bridge_projection(cell, false);
            }
        }
        updates
    }

    /// Project a direct runtime overlay-field write into the map-owned bridge
    /// cache. The literal identity/data remain owned by OverlayGrid; this is
    /// only the derived bridge metadata that pathing and bridge damage read.
    pub(crate) fn set_runtime_overlay_bridge_identity(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_id: u8,
        overlay_data: u8,
        overlay_name: &str,
    ) {
        let Some(cell) = self.cell_mut(rx, ry) else {
            return;
        };
        cell.bridge_facts.overlay_id = Some(overlay_id);
        cell.bridge_facts.state_byte = overlay_data;
        if !crate::rules::overlay_types::is_bridge_overlay_index(overlay_id) {
            return;
        }

        let direction = match overlay_id {
            0x18 | 0xED => BridgeDirection::EastWest,
            0x19 | 0xEE => BridgeDirection::NorthSouth,
            _ => BridgeDirection::Low,
        };
        let deck_level = match direction {
            BridgeDirection::EastWest | BridgeDirection::NorthSouth => cell.level.saturating_add(4),
            BridgeDirection::Low => cell.level,
        };
        cell.bridge_layer = Some(BridgeLayer {
            overlay_id,
            overlay_name: overlay_name.to_string(),
            deck_level,
            direction,
        });
        cell.has_bridge_deck = true;
        cell.bridge_walkable = direction != BridgeDirection::Low
            && !cell.terrain_object_blocks
            && !cell.overlay_blocks;
        cell.bridge_deck_level = deck_level;
    }

    /// Project a direct `CellClass::OverlayData` write into the derived bridge
    /// facts without inventing an overlay identity. The native high-bridge
    /// setters write this byte on four cells before `OverlayClass::Mark`
    /// decides whether the anchor itself receives an overlay.
    pub(crate) fn set_runtime_overlay_bridge_state_byte(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_data: u8,
    ) -> bool {
        let Some(cell) = self.cell_mut(rx, ry) else {
            return false;
        };
        cell.bridge_facts.state_byte = overlay_data;
        true
    }

    /// Project the two literal field writes a crate removal performs.
    ///
    /// `CrateSlot__RemoveCrateOverlayFromCell @ 0x004A1AA0` stores
    /// `CellClass+0x44 = -1` at `0x004A1BE1` and `CellClass+0x11E = 0` at
    /// `0x004A1BE8` and touches nothing else — no `RecalcAttributes` tail and
    /// no bridge setter. The derived bridge layer/deck projection therefore
    /// stays as it is: the random placer only admits cells that already carry
    /// no overlay, so a removal can never be undoing a bridge identity in
    /// stock rules.
    pub(crate) fn clear_runtime_overlay_identity(&mut self, rx: u16, ry: u16) -> bool {
        let Some(cell) = self.cell_mut(rx, ry) else {
            return false;
        };
        cell.bridge_facts.overlay_id = None;
        cell.bridge_facts.state_byte = 0;
        true
    }

    pub fn capture_real_cell_bridge_flags_0x1180(&self) -> RealCellBridgeFlags0x1180 {
        RealCellBridgeFlags0x1180::from_grid(self)
    }

    /// One original586BF0 write through packed fixed lookup. Missing slots
    /// publish their requested coordinate and mutate the retained shared dummy.
    /// Other real flag bits are preserved; no topology/zone rebuild is implied.
    pub(crate) fn apply_bridge_gap_flag_write(
        &mut self,
        coord: (i16, i16),
        horizontal: bool,
    ) -> (Option<usize>, u32) {
        let update = |flags: u32| {
            if horizontal {
                flags | 0xC00
            } else {
                (flags & !0x800) | 0x400
            }
        };
        if let Some(index) = self.native_fixed_cell_index(coord.0, coord.1) {
            let flags = &mut self.cells[index].bridge_facts.raw_flags;
            *flags = update(*flags);
            (Some(index), *flags)
        } else {
            self.shared_cell_dummy
                .stamp_coord(i32::from(coord.0), i32::from(coord.1));
            self.shared_cell_dummy.update_retained_bridge_flags(update);
            (None, self.shared_cell_dummy.retained_bridge_flags())
        }
    }

    pub(crate) fn bridge_flag_authority_matches_shape(
        &self,
        authority: &RealCellBridgeFlags0x1180,
    ) -> bool {
        authority.matches_grid_shape(self)
    }

    /// Restore serialized real CellClass values by aligned direct writes.
    /// This deliberately never performs a MapClass lookup or setter call, so
    /// native-unallocated slots cannot stamp or mutate the shared dummy.
    pub(crate) fn restore_real_cell_bridge_flags_0x1180(
        &mut self,
        authority: &RealCellBridgeFlags0x1180,
    ) -> bool {
        if !authority.matches_grid_shape(self)
            || self
                .native_allocated
                .as_ref()
                .is_some_and(|mask| mask.len() != self.cells.len())
        {
            return false;
        }

        for (index, &saved) in authority.flags_or_unallocated.iter().enumerate() {
            let allocated = self
                .native_allocated
                .as_deref()
                .is_none_or(|mask| mask[index]);
            if allocated == (saved == UNALLOCATED_REAL_CELL_BRIDGE_FLAGS)
                || (saved != UNALLOCATED_REAL_CELL_BRIDGE_FLAGS
                    && u32::from(saved) & !RETAINED_CELLCLASS_BRIDGE_FLAG_MASK != 0)
            {
                return false;
            }
        }

        for (cell, &saved) in self.cells.iter_mut().zip(&authority.flags_or_unallocated) {
            if saved != UNALLOCATED_REAL_CELL_BRIDGE_FLAGS {
                cell.bridge_facts.raw_flags = (cell.bridge_facts.raw_flags
                    & !RETAINED_CELLCLASS_BRIDGE_FLAG_MASK)
                    | u32::from(saved);
            }
        }
        true
    }
}
