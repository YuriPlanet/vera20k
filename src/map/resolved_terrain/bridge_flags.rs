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
/// Cell storage a bridge stamp writes: the loader's vector or the live
/// grid's shared chunks.
pub(super) trait StampCells:
    std::ops::IndexMut<usize, Output = ResolvedTerrainCell>
{
    fn cell_count(&self) -> usize;
}

impl StampCells for Vec<ResolvedTerrainCell> {
    fn cell_count(&self) -> usize {
        self.len()
    }
}

impl StampCells for crate::util::synced::ChunkedVec<ResolvedTerrainCell> {
    fn cell_count(&self) -> usize {
        self.len()
    }
}

pub(super) fn apply_native_bridge_flag_stamp_to_parts(
    cells: &mut impl StampCells,
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
        cells.cell_count(),
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
            native_resolved_cell_index(width, height, native_allocated, cells.cell_count(), x, y)
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

/// High bridge stamping (47E040/47E470) is the one point that establishes
/// the deck. Start from the anchor's authored geometry and retain that level
/// when the native bridge routines subsequently change Cell+11B or its flags.
fn establish_high_bridge_deck(cell: &mut ResolvedTerrainCell) {
    let facts = &mut cell.bridge_facts;
    if facts.has_structural_bridge() && facts.has_flag(BRIDGE_FLAG_ANCHOR_SELF) {
        facts.fixed_deck_level.get_or_insert_with(|| {
            i16::from(cell.level as i8) + crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS as i16
        });
    }
}

pub(super) fn refresh_runtime_bridge_projection(
    cell: &mut ResolvedTerrainCell,
    structural_removed: bool,
) {
    establish_high_bridge_deck(cell);
    let facts = cell.bridge_facts;
    if facts.has_structural_bridge() {
        cell.has_bridge_deck = true;
        cell.bridge_walkable = !cell.terrain_object_blocks && !cell.overlay_blocks;
        cell.refresh_current_height_projection();
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
    /// Both authored load paths recover one deck for the full run. Stock
    /// Mark (0x005FD36D..0x005FD55D) sees only one anchor. Use the lower of
    /// the identified approaches, falling back to the end anchors only when
    /// neither approach can be identified.
    pub(crate) fn recover_authored_high_bridge_decks(
        &self,
        anchors: impl IntoIterator<Item = ((u16, u16), (BridgeStampFamily, u8))>,
        admits: impl Fn((u16, u16)) -> bool,
    ) -> std::collections::BTreeMap<(u16, u16), i16> {
        use std::collections::BTreeMap;
        let terrain_cell = |(x, y)| {
            if x >= self.width || y >= self.height || !admits((x, y)) {
                return None;
            }
            let index = self.native_fixed_cell_index(x as i16, y as i16)?;
            self.cells.get(index)
        };
        let anchors = anchors
            .into_iter()
            .filter_map(|(coord, stamp)| Some((coord, (stamp, terrain_cell(coord)?))))
            .collect::<BTreeMap<_, _>>();
        let mut decks = BTreeMap::new();
        let stock_deck = |level: u8| {
            i16::from(level as i8) + crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS as i16
        };
        let concrete = self.high_bridge_rim_tiles.filter(|rim| rim.base >= 0);
        let wood = &self.wood_bridge_rim_tiles;
        for (&(x, y), &(stamp, first)) in &anchors {
            // A north/south setter stamps across an east/west body run.
            let horizontal = stamp.1 == 0;
            let direction = if horizontal { 2 } else { 4 };
            let at = |along| if horizontal { (along, y) } else { (x, along) };
            let start = if horizontal { x } else { y };
            let run_cell = |along| {
                let &(key, cell) = anchors.get(&at(along))?;
                (key == stamp).then_some(cell)
            };
            if start.checked_sub(1).and_then(run_cell).is_some() {
                continue;
            }
            let (mut end, mut last) = (start, first);
            while let Some(cell) = end.checked_add(1).and_then(run_cell) {
                end += 1;
                last = cell;
            }

            // Concrete follows the theater keys (0x00576770, 0x00576200).
            // A wooden rim uses its own pristine template, even if the map
            // gives the corresponding span a concrete body overlay.
            let start_deck = if let Some(approach) =
                start.checked_sub(1).and_then(|p| terrain_cell(at(p)))
                && (wood.is_start(first, approach, direction)
                    || concrete.is_some_and(|rim| {
                        approach.final_tile_index == first.final_tile_index
                            && rim.start_direction(first.final_tile_index, first.final_sub_tile)
                                == Some(direction)
                    })) {
                Some(i16::from(approach.level as i8))
            } else {
                None
            };
            let end_deck = if let Some(approach) =
                end.checked_add(1).and_then(|p| terrain_cell(at(p)))
                && (wood.is_end(approach, direction)
                    || concrete.is_some_and(|rim| {
                        rim.is_end(
                            approach.final_tile_index,
                            approach.final_sub_tile,
                            direction,
                        )
                    })) {
                Some(i16::from(approach.level as i8))
            } else {
                None
            };
            let deck = match (start_deck, end_deck) {
                (Some(start), Some(end)) => start.min(end),
                (Some(deck), None) | (None, Some(deck)) => deck,
                (None, None) => stock_deck(first.level).min(stock_deck(last.level)),
            };
            for along in start..=end {
                decks.insert(at(along), deck);
            }
        }
        decks
    }

    /// The live high-bridge deck, shared by an anchor and the lanes it stamped.
    /// This replaces the local ground + 4 result for ObjectClass::SetHeight
    /// (0x005F5FA0), AStar node height (0x0042A460), and other surface consumers.
    pub fn corrected_high_bridge_deck_level(&self, coord: (u16, u16)) -> Option<i16> {
        let index = self.native_fixed_cell_index(coord.0 as i16, coord.1 as i16)?;
        self.corrected_high_bridge_deck_at(NativeCellIdentity::Real(index))
    }

    pub fn corrected_high_bridge_base_level(&self, coord: (u16, u16)) -> Option<u8> {
        let deck = self.corrected_high_bridge_deck_level(coord)?;
        Some((deck - crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS as i16) as u8)
    }

    pub(crate) fn set_authored_high_bridge_deck(&mut self, anchor: NativeCellIdentity, deck: i16) {
        if let NativeCellIdentity::Real(index) = anchor
            && let Some(cell) = self.cells.get_mut(index)
        {
            cell.bridge_facts.fixed_deck_level = Some(deck);
            if let Some(base) = cell.retained_high_bridge_base_level() {
                self.level_span.widen(base);
            }
        }
    }

    pub(crate) fn corrected_high_bridge_deck_at(&self, cell: NativeCellIdentity) -> Option<i16> {
        let NativeCellIdentity::Real(index) = cell else {
            return None;
        };
        let cell = self.cells.get(index)?;
        if !cell.bridge_facts.has_structural_bridge() {
            return None;
        }
        let anchor = if cell.bridge_facts.has_flag(BRIDGE_FLAG_ANCHOR_SELF) {
            cell
        } else if let Some(NativeCellIdentity::Real(anchor)) = cell.bridge_facts.native_anchor {
            self.cells.get(anchor)?
        } else {
            return None;
        };
        let facts = anchor.bridge_facts;
        if !facts.has_structural_bridge() || !facts.has_flag(BRIDGE_FLAG_ANCHOR_SELF) {
            return None;
        }
        let deck = facts.fixed_deck_level?;
        // A saved value must be representable by the authored signed cell level.
        anchor.retained_high_bridge_base_level()?;
        Some(deck)
    }

    /// Give every fixture deck the CellClass bit0x100 a loaded deck carries.
    #[cfg(any(test, feature = "test-support"))]
    pub fn test_mark_decks_structural(mut self) -> Self {
        for cell in self.cells.iter_mut() {
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
            self.cells.all_mut(),
            self.width,
            self.height,
            self.native_allocated.as_deref().map(Vec::as_slice),
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
            self.native_allocated.as_deref().map(Vec::as_slice),
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
            establish_high_bridge_deck(&mut self.cells[index]);
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
            self.cells.all_mut(),
            self.width,
            self.height,
            self.native_allocated.as_deref().map(Vec::as_slice),
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

    /// Project a runtime bridge-overlay identity write into the bridge layer
    /// and deck that pathing and bridge damage read. OverlayGrid owns the
    /// identity itself; a non-bridge identity changes nothing here.
    pub(crate) fn set_runtime_overlay_bridge_identity(&mut self, rx: u16, ry: u16, overlay_id: u8) {
        if !crate::rules::overlay_types::is_bridge_overlay_index(overlay_id) {
            return;
        }
        let Some(cell) = self.cell_mut(rx, ry) else {
            return;
        };

        let layer = BridgeLayer::for_overlay(overlay_id, cell.level);
        cell.has_bridge_deck = true;
        cell.bridge_walkable = layer.direction != BridgeDirection::Low
            && !cell.terrain_object_blocks
            && !cell.overlay_blocks;
        cell.bridge_deck_level = layer.deck_level;
        cell.bridge_layer = Some(layer);
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

#[cfg(all(test, tests_map))]
mod tests {
    use super::*;
    use crate::util::save_codec;

    #[test]
    fn a_raised_lane_keeps_the_same_deck_as_its_anchor() {
        let mut terrain = test_grid(16, 16, |rx, ry| ResolvedTerrainCell {
            level: if (rx, ry) == (8, 9) { 5 } else { 4 },
            ..test_flat_cell(rx, ry)
        });
        terrain.apply_runtime_bridge_mark_stamp(
            BridgeFlagStamp::new((8, 8), 0, true),
            BridgeStampFamily::Nesw,
        );
        let lane = terrain.native_cell_identity((8, 9));
        assert_eq!(terrain.corrected_high_bridge_deck_at(lane), Some(8));
        assert_eq!(terrain.corrected_high_bridge_deck_level((8, 8)), Some(8));
    }

    #[test]
    fn collapsed_span_keeps_its_elevation_after_save_and_repair() {
        let mut terrain = test_grid(16, 16, |rx, ry| ResolvedTerrainCell {
            level: if (rx, ry) == (8, 8) { 5 } else { 0 },
            ..test_flat_cell(rx, ry)
        });
        let stamp = |set| BridgeFlagStamp::new((8, 8), 0, set);
        let family = BridgeStampFamily::Nesw;
        let anchor = terrain.native_cell_identity((8, 8));
        let lane = terrain.native_cell_identity((8, 9));
        terrain.apply_runtime_bridge_mark_stamp(stamp(true), family);
        assert_eq!(terrain.corrected_high_bridge_deck_at(lane), Some(9));
        let baseline = terrain.capture_saved_template().unwrap();

        terrain.write_native_cell_flags(anchor, 0);
        terrain.write_native_cell_flags(lane, 0);
        terrain.write_native_cell_anchor(lane, None);
        terrain.write_native_cell_level(anchor, 4);
        assert_eq!(terrain.corrected_high_bridge_deck_at(lane), None);

        let mut changes = DynamicTerrainCells::default();
        changes.record(terrain.cell(8, 8).unwrap());
        changes.record(terrain.cell(8, 9).unwrap());
        let bytes = save_codec::serialize(&(baseline, changes)).unwrap();
        let (baseline, changes): (SavedTerrainTemplate, DynamicTerrainCells) =
            save_codec::deserialize(&bytes).unwrap();
        let mut restored = baseline.restore_values(SharedCellDummy::fresh()).unwrap();
        changes.apply_to(&mut restored).unwrap();
        assert_eq!(restored.corrected_high_bridge_deck_at(lane), None);
        restored.write_native_cell_flags(anchor, BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_ANCHOR_SELF);
        restored.write_native_cell_anchor(lane, Some(anchor));
        restored.write_native_cell_flags(lane, BRIDGE_FLAG_STRUCTURAL);
        assert_eq!(restored.corrected_high_bridge_deck_at(lane), Some(9));
        assert_eq!(restored.corrected_high_bridge_deck_at(anchor), Some(9));
    }
}
