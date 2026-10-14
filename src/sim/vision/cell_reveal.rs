//! MapCell4A9890/4A9CA0, RevealCell4AA050 and Tactical6D8700 cache authority.
//! Counters and ground knowledge stay with OwnerVisibility. MapCell4A9CA0
//! updates these derived cache bytes only for the cell it visits, leaving its
//! neighbors stale; RevealCell4AA050 also rewrites its neighbors' ground
//! frames. Native constructor47BD3A initializes -2/-2.

use super::{FogState, GROUND_OPEN, GROUND_VISIBLE, InternedId, OwnerVisibility, ShroudKnowledge};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

#[cfg(test)]
#[path = "cell_reveal_tests.rs"]
pub(crate) mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct MapCellEdgeCache {
    initialized: bool, // Cell140 bit2
    fog_open: bool,    // Cell140 bit1
    frames: [i8; 2],   // Cell120/121
}
impl Default for MapCellEdgeCache {
    fn default() -> Self {
        Self {
            initialized: false,
            fog_open: false,
            frames: [-2, -2],
        }
    }
}
impl MapCellEdgeCache {
    /// This cell's state hash record.
    fn hash_record(&self) -> [u8; 4] {
        let Self {
            initialized,
            fog_open,
            frames: [first, second],
        } = *self;
        [
            u8::from(initialized),
            u8::from(fog_open),
            first as u8,
            second as u8,
        ]
    }
}

/// The canonical map Dummy has one retained visibility record per represented
/// player plane, shared by every missing coordinate. Its coordinate/height
/// remain with NativeCellQuery; a miss does not invent a rectangular real cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) struct DummyVisibility {
    knowledge: ShroudKnowledge,
    ground: u8,
    cache: MapCellEdgeCache,
}

#[derive(Clone, Copy)]
enum VisibilityCell {
    Real(usize),
    Dummy,
}

/// Synchronous MapCell receiver arguments. The coordinate is read after both
/// edge queries from the cached Cell pointer. A Dummy may have been restamped.
pub(crate) struct MapCellNotification {
    pub(crate) cell: (u16, u16),
    pub(crate) fresh: bool,
}

impl OwnerVisibility {
    /// Map577C90's per-cell receiver577D1F..577D47, unlike577AB0's
    /// counter overwrite. Reuse the unique Cell487690 leave arithmetic and
    /// clear knowledge/cache bits only when the resulting counter is>=0.
    /// Gap counts, edge-frame bytes and pending20 survive the flag clear.
    pub(super) fn store_reshrouded_map_cell(&mut self, cell: (u16, u16)) {
        let Some(index) = self.index(cell.0, cell.1) else {
            return;
        };
        let state = &mut self.shroud_knowledge[index];
        state.leave();
        if state.counter >= 0 {
            state.open = false;
            state.transient_visible = false;
            self.ground_flags[index] &= !(GROUND_VISIBLE | GROUND_OPEN);
            self.map_cell_edge_cache[index].initialized = false;
            self.map_cell_edge_cache[index].fog_open = false;
        }
        self.publish_knowledge(index);
    }

    /// Original bulk577D90/577AB0 stores, including Cell12C bits18 and
    /// Cell140 bits3. These writers leave edge-frame bytes and pending20
    /// unchanged; their next4A9890 receiver reads the same cache authority.
    pub(super) fn store_bulk_map_cell(&mut self, cell: (u16, u16), reset: bool) {
        let Some(index) = self.index(cell.0, cell.1) else {
            return;
        };
        let state = &mut self.shroud_knowledge[index];
        state.counter = i32::from(reset);
        state.gap_counter = 0;
        state.open = !reset;
        let ground = &mut self.ground_flags[index];
        if reset {
            *ground &= !(GROUND_VISIBLE | GROUND_OPEN);
        } else {
            *ground |= GROUND_VISIBLE | GROUND_OPEN;
        }
        let cache = &mut self.map_cell_edge_cache[index];
        cache.initialized = !reset;
        cache.fog_open = !reset;
        self.publish_knowledge(index);
    }

    fn visibility_cell(
        &self,
        cell: (u16, u16),
        cells: Option<&NativeCellQuery<'_>>,
    ) -> VisibilityCell {
        let coord = match cells {
            Some(cells) => match cells.lookup((cell.0 as i16, cell.1 as i16)) {
                NativeCellIdentity::Real(identity) => {
                    let source = &cells.terrain().cells()[identity];
                    (source.rx, source.ry)
                }
                NativeCellIdentity::Dummy => return VisibilityCell::Dummy,
            },
            None => cell,
        };
        self.index(coord.0, coord.1)
            .map_or(VisibilityCell::Dummy, VisibilityCell::Real)
    }

    fn map_cell_parts(&self, key: VisibilityCell) -> (u8, MapCellEdgeCache) {
        match key {
            VisibilityCell::Real(index) => {
                (self.ground_flags[index], self.map_cell_edge_cache[index])
            }
            VisibilityCell::Dummy => (self.dummy_visibility.ground, self.dummy_visibility.cache),
        }
    }

    fn map_cell_parts_mut(
        &mut self,
        key: VisibilityCell,
    ) -> (&mut ShroudKnowledge, &mut u8, &mut MapCellEdgeCache) {
        match key {
            VisibilityCell::Real(index) => (
                &mut self.shroud_knowledge[index],
                &mut self.ground_flags[index],
                &mut self.map_cell_edge_cache[index],
            ),
            VisibilityCell::Dummy => (
                &mut self.dummy_visibility.knowledge,
                &mut self.dummy_visibility.ground,
                &mut self.dummy_visibility.cache,
            ),
        }
    }

    pub(crate) fn hash_map_cell_edge_cache(&self, hasher: &mut impl Hasher) {
        // Every cell's fields, initialized or not: the reveal and edge-frame
        // readers consult `frames` and `fog_open` of uninitialized cells too.
        crate::sim::state_hasher::hash_records(
            &self.map_cell_edge_cache,
            hasher,
            MapCellEdgeCache::hash_record,
        );
        if self.dummy_visibility != DummyVisibility::default() {
            b"native-dummy-visibility-v1".hash(hasher);
            self.dummy_visibility.hash(hasher);
        }
    }

    #[cfg(test)]
    pub(crate) fn map_cell_edge_cache_for_test(&self, cell: (u16, u16)) -> ([i8; 2], u32) {
        let cache = self.map_cell_edge_cache[self.index(cell.0, cell.1).unwrap()];
        (
            cache.frames,
            u32::from(cache.initialized) * 2 | u32::from(cache.fog_open),
        )
    }
}

impl FogState {
    /// Original Display4A9890, via Map653810/Execute16. Unlike4A9CA0 it
    /// does not touch sight/gap counters; unlike4AA050 it initializes both
    /// ground and fog edge channels. Fresh and changed are distinct: the
    /// receiver5865F0 runs only on changed, with the original fresh argument.
    /// Original leaf/frontier execution: tools/spatial_oracle/
    /// trigger_forced_map.{json,meta.json}, Steam3e81a617. Its active caller
    /// is Execute16 at6DE3A8 through Map653810.
    /// The unrepresented Cell140 bit40 is cleared natively; no represented
    /// counter/pending20 substitutes for that bit. First initialization's
    /// FogOfWar486BF0 cleanup remains the existing fogged-object residual;
    /// the selected ALL01 Scenario has that flag clear.
    pub(crate) fn reveal_forced_map_cell(
        &mut self,
        viewer: InternedId,
        cell: (u16, u16),
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> Option<MapCellNotification> {
        let vis = self
            .by_owner
            .entry(viewer)
            .or_insert_with(|| OwnerVisibility::new(self.width, self.height));
        let key = vis.visibility_cell(cell, native_cells);
        let (ground, old) = vis.map_cell_parts(key);
        let fresh = !(old.initialized && ground & GROUND_VISIBLE != 0);
        let (state, ground, cache) = vis.map_cell_parts_mut(key);
        cache.initialized = true;
        *ground |= GROUND_VISIBLE;
        state.open = true;
        if let VisibilityCell::Real(index) = key {
            vis.publish_knowledge(index);
        }
        let ground = self.map_cell_edge_frame(viewer, cell, false, native_cells);
        let vis = self.by_owner.get_mut(&viewer).unwrap();
        let (_, flags, cache) = vis.map_cell_parts_mut(key);
        cache.frames[0] = ground;
        if ground == -1 {
            *flags |= GROUND_OPEN;
        }
        let fog = self.map_cell_edge_frame(viewer, cell, true, native_cells);
        let vis = self.by_owner.get_mut(&viewer).unwrap();
        let (_, _, cache) = vis.map_cell_parts_mut(key);
        cache.frames[1] = fog;
        if fog == -1 {
            cache.fog_open = true;
        }
        //4A9981..4A9C48 interleaves the two channels for each neighbor.
        // The guarded recursive +90 arms cannot occur: a neighbor without
        // its eligible or open bit returns-2 in6D8700, not another frame.
        for neighbor in cell_neighbors(cell) {
            self.propagate_map_cell_edge(viewer, neighbor, false, native_cells);
            self.propagate_map_cell_edge(viewer, neighbor, true, native_cells);
        }
        if !fresh && old.frames == [ground, fog] {
            return None;
        }
        let cell = match (key, native_cells) {
            (VisibilityCell::Dummy, Some(cells)) => {
                let coord = cells.coord(NativeCellIdentity::Dummy);
                (coord.0 as u16, coord.1 as u16)
            }
            _ => cell,
        };
        Some(MapCellNotification { cell, fresh })
    }

    /// Original MapCell4A9CA0..4A9DC9, with ordinary final0/1 counter leaves.
    /// Emit only when4A9D94 calls CellChangeNotify; fresh is its separate
    /// new-knowledge argument, independently of cache-change admission.
    /// Executed cell/cache/channel/Dummy controls:
    /// tools/spatial_oracle/trigger_action_values.{py,json,meta.json}.
    /// FogOfWar486BF0's fogged-object tail is outside the stock-clear path.
    pub(crate) fn reveal_map_cell(
        &mut self,
        viewer: InternedId,
        cell: (u16, u16),
        final_pass: bool,
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> Option<MapCellNotification> {
        let vis = self
            .by_owner
            .entry(viewer)
            .or_insert_with(|| OwnerVisibility::new(self.width, self.height));
        //4A9CA9 is distinct from each6D8713 center lookup below.
        let key = vis.visibility_cell(cell, native_cells);
        let (ground, old) = vis.map_cell_parts(key);
        let fresh = !(old.initialized && ground & GROUND_VISIBLE != 0);
        let (state, ground, cache) = vis.map_cell_parts_mut(key);
        cache.initialized = true;
        if final_pass {
            state.leave();
        } else {
            state.reveal();
        }
        state.transient_visible = matches!(key, VisibilityCell::Real(_));
        let open = state.open;
        if open {
            *ground |= GROUND_VISIBLE | GROUND_OPEN;
        } else {
            *ground &= !(GROUND_VISIBLE | GROUND_OPEN);
        }
        if let VisibilityCell::Real(index) = key {
            vis.tracked.transient.push(index as u32);
            if !open {
                // The next pass's sight puts back cleared ground bits under
                // its footprints (`SightApply::Mark`), so it visits them all.
                vis.tracked.complete = false;
            }
            vis.publish_knowledge(index);
        }
        let ground = self.map_cell_edge_frame(viewer, cell, false, native_cells);
        let vis = self.by_owner.get_mut(&viewer).unwrap();
        let (_, flags, cache) = vis.map_cell_parts_mut(key);
        cache.frames[0] = ground;
        if ground == -1 {
            *flags |= GROUND_OPEN;
        }
        let fog = self.map_cell_edge_frame(viewer, cell, true, native_cells);
        let vis = self.by_owner.get_mut(&viewer).unwrap();
        let (_, _, cache) = vis.map_cell_parts_mut(key);
        cache.frames[1] = fog;
        if fog == -1 {
            cache.fog_open = true;
        }
        if !fresh && old.frames == [ground, fog] {
            return None;
        }
        let cell = match (key, native_cells) {
            (VisibilityCell::Dummy, Some(cells)) => {
                let coord = cells.coord(NativeCellIdentity::Dummy);
                (coord.0 as u16, coord.1 as u16)
            }
            _ => cell,
        };
        Some(MapCellNotification { cell, fresh })
    }

    /// Original6D8700..6D8ADA: center lookup/gates, eight signed WORD
    /// neighbor lookups and sign-extended7F4194 table byte. Missing neighbors
    /// read the canonical Dummy's retained flags and stamp its coordinate.
    pub(crate) fn map_cell_edge_frame(
        &self,
        viewer: InternedId,
        cell: (u16, u16),
        fog_channel: bool,
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> i8 {
        let Some(vis) = self.by_owner.get(&viewer) else {
            return -2;
        };
        let center = vis.visibility_cell(cell, native_cells);
        let (ground, cache) = vis.map_cell_parts(center);
        let (any, eligible) = if fog_channel {
            (cache.initialized || cache.fog_open, cache.initialized)
        } else {
            (
                ground & (GROUND_VISIBLE | GROUND_OPEN) != 0,
                ground & GROUND_VISIBLE != 0,
            )
        };
        let fallback = if any { -1 } else { -2 };
        if !eligible {
            return fallback;
        }
        let mut mask = 0u8;
        for (dx, dy, bit) in [
            (-1i16, -1i16, 0x40),
            (0, -1, 0x80),
            (1, -1, 0x01),
            (-1, 0, 0x20),
            (1, 0, 0x02),
            (-1, 1, 0x10),
            (0, 1, 0x08),
            (1, 1, 0x04),
        ] {
            let neighbor = (
                (cell.0 as i16).wrapping_add(dx) as u16,
                (cell.1 as i16).wrapping_add(dy) as u16,
            );
            let key = vis.visibility_cell(neighbor, native_cells);
            let (ground, cache) = vis.map_cell_parts(key);
            let set = if fog_channel {
                cache.initialized
            } else {
                ground & GROUND_VISIBLE != 0
            };
            if !set {
                mask |= bit;
            }
        }
        SHROUD_EDGE_LUT[usize::from(mask)] as i8
    }

    /// `DisplayClass::RevealCell @ 0x004AA050` (DisplayClass vtable `+0x9C`)
    /// for `viewer`, on the ground bits (`Cell+0x12C`) and ground edge frame
    /// (`Cell+0x120`) that [`Self::map_cell_edge_frame`] reads. Its Tactical
    /// redraw marks (`0x006DA7D0`) are presentation.
    /// - The cell becomes explored (`0x08`); its frame is recomputed and
    ///   stored, and -1 opens it (`0x10`, `0x004AA05D..0x004AA0B4`).
    /// - Each neighbor in `0x0089F688` order that is not open has its frame
    ///   recomputed (`0x004AA0CA..0x004AA25E`). -1 opens it and stores the
    ///   frames of its own eight neighbors, but not its own; any other
    ///   non-negative frame is stored.
    /// - Its two recursive calls (`0x004AA171`, `0x004AA229`) need a neighbor
    ///   that is neither explored nor open and whose frame is not -2, which
    ///   the ground gate of `0x006D8700` (`0x006D88F4..0x006D890A`) never
    ///   returns.
    ///
    /// No shroud counter, `Cell+0x140` bit or fog frame (`+0x121`) is
    /// written. [`ShroudKnowledge::open`] stands for both ground bits, while
    /// `MapClass::IsShrouded @ 0x00586360` reads `0x08` alone, so the explored
    /// cell opens there too. A cell left with `0x08` but not `0x10` (one beside
    /// an unallocated cell) then reads as open to `0x00487630`, which clears
    /// its pending conceal where native sets `0x10` instead.
    ///
    /// RESIDUAL (#1117): the two bits are read from `ground_flags`, which the
    /// gap, conceal, fire and whole-map writers (`0x006FB170`, `0x006FB470`,
    /// `0x00578100`, `0x004876F0`, `0x00577D90`, `0x00577AB0`) leave as they
    /// were. A cell a hostile gap re-hid still reads explored: its reveal is
    /// not fresh and, with its frame unchanged, sends no CellChangeNotify, and
    /// as a neighbor it is skipped when its stale `0x10` is set. A cell those
    /// writers opened through `open` alone reads unexplored and notifies again.
    /// Effect: on a later whole-map reveal (a local defeat) those cells'
    /// discovery, radar marks and stored edge frames differ from native.
    ///
    /// Returns CellChangeNotify's arguments when the explored bit or the frame
    /// changed (`0x004AA264..0x004AA27C`).
    pub(crate) fn reveal_cell(
        &mut self,
        viewer: InternedId,
        cell: (u16, u16),
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> Option<MapCellNotification> {
        let vis = self
            .by_owner
            .entry(viewer)
            .or_insert_with(|| OwnerVisibility::new(self.width, self.height));
        let center = vis.visibility_cell(cell, native_cells);
        let (state, ground, _) = vis.map_cell_parts_mut(center);
        let fresh = *ground & GROUND_VISIBLE == 0;
        *ground |= GROUND_VISIBLE;
        state.open = true;
        if let VisibilityCell::Real(index) = center {
            vis.publish_knowledge(index);
        }
        let frame = self.map_cell_edge_frame(viewer, cell, false, native_cells);
        let vis = self.by_owner.get_mut(&viewer).unwrap();
        let (_, ground, cache) = vis.map_cell_parts_mut(center);
        let changed = fresh || frame != cache.frames[0];
        cache.frames[0] = frame;
        if frame == -1 {
            *ground |= GROUND_OPEN;
        }
        for neighbor in cell_neighbors(cell) {
            self.propagate_map_cell_edge(viewer, neighbor, false, native_cells);
        }
        if !changed {
            return None;
        }
        let cell = match (center, native_cells) {
            (VisibilityCell::Dummy, Some(cells)) => {
                let coord = cells.coord(NativeCellIdentity::Dummy);
                (coord.0 as u16, coord.1 as u16)
            }
            _ => cell,
        };
        Some(MapCellNotification { cell, fresh })
    }

    /// Shared neighbor-channel responsibility in4A9890 and4AA050. It
    /// preserves native delta order and updates adjacent frames only when
    /// this neighbor becomes fully open; counters and notification remain
    /// with the calling entry. Tactical redraw is presentation-owned.
    fn propagate_map_cell_edge(
        &mut self,
        viewer: InternedId,
        neighbor: (u16, u16),
        fog_channel: bool,
        native_cells: Option<&NativeCellQuery<'_>>,
    ) {
        let key = self.by_owner[&viewer].visibility_cell(neighbor, native_cells);
        let (ground, cache) = self.by_owner[&viewer].map_cell_parts(key);
        if if fog_channel {
            cache.fog_open
        } else {
            ground & GROUND_OPEN != 0
        } {
            return;
        }
        let frame = self.map_cell_edge_frame(viewer, neighbor, fog_channel, native_cells);
        let vis = self.by_owner.get_mut(&viewer).unwrap();
        let (_, ground, cache) = vis.map_cell_parts_mut(key);
        let channel = usize::from(fog_channel);
        if frame == -1 {
            if fog_channel {
                cache.fog_open = true;
            } else {
                *ground |= GROUND_OPEN;
            }
            for next in cell_neighbors(neighbor) {
                let key = self.by_owner[&viewer].visibility_cell(next, native_cells);
                let frame = self.map_cell_edge_frame(viewer, next, fog_channel, native_cells);
                let vis = self.by_owner.get_mut(&viewer).unwrap();
                vis.map_cell_parts_mut(key).2.frames[channel] = frame;
            }
        } else if frame >= 0 {
            cache.frames[channel] = frame;
        }
    }

    pub(super) fn is_dummy_ground_unshrouded(&self, viewer: InternedId) -> bool {
        self.by_owner
            .get(&viewer)
            .is_some_and(|vis| vis.dummy_visibility.ground & GROUND_VISIBLE != 0)
    }
}

/// The eight cells around `cell` in `0x0089F688` order, with the native signed
/// WORD adds.
fn cell_neighbors(cell: (u16, u16)) -> impl Iterator<Item = (u16, u16)> {
    crate::util::direction_tables::cell::CELL_DELTAS
        .into_iter()
        .map(move |(dx, dy)| {
            (
                (cell.0 as i16).wrapping_add(dx as i16) as u16,
                (cell.1 as i16).wrapping_add(dy as i16) as u16,
            )
        })
}

/// Shroud edge frame lookup table.
///
/// Indexed by 8-bit neighbor bitmask (see `FogState::shroud_edge_mask_8bit`).
/// Values: 0xFF = no edge needed, 0xFE = fully surrounded, 0-46 = frame index.
#[rustfmt::skip]
pub const SHROUD_EDGE_LUT: [u8; 256] = [
    // 0x00–0x0F
    0xFF, 0x21, 0x02, 0x02, 0x22, 0x25, 0x02, 0x02,
    0x04, 0x1A, 0x06, 0x06, 0x04, 0x1A, 0x06, 0x06,
    // 0x10–0x1F
    0x23, 0x2D, 0x11, 0x11, 0x26, 0x29, 0x11, 0x11,
    0x04, 0x1A, 0x06, 0x06, 0x04, 0x1A, 0x06, 0x06,
    // 0x20–0x2F
    0x08, 0x15, 0x0A, 0x0A, 0x1B, 0x1F, 0x0A, 0x0A,
    0x0C, 0x17, 0x0E, 0x0E, 0x0C, 0x17, 0x0E, 0x0E,
    // 0x30–0x3F
    0x08, 0x15, 0x0A, 0x0A, 0x1B, 0x1F, 0x0A, 0x0A,
    0x0C, 0x17, 0x0E, 0x0E, 0x0C, 0x17, 0x0E, 0x0E,
    // 0x40–0x4F
    0x20, 0x24, 0x19, 0x19, 0x2C, 0x28, 0x19, 0x19,
    0x13, 0x1E, 0x14, 0x14, 0x13, 0x1E, 0x14, 0x14,
    // 0x50–0x5F
    0x27, 0x2B, 0x1D, 0x1D, 0x2A, 0x2E, 0x1D, 0x1D,
    0x13, 0x1E, 0x14, 0x14, 0x13, 0x1E, 0x14, 0x14,
    // 0x60–0x6F
    0x08, 0x15, 0x0A, 0x0A, 0x1B, 0x1F, 0x0A, 0x0A,
    0x0C, 0x17, 0x0E, 0x0E, 0x0C, 0x17, 0x0E, 0x0E,
    // 0x70–0x7F
    0x08, 0x15, 0x0A, 0x0A, 0x1B, 0x1F, 0x0A, 0x0A,
    0x0C, 0x17, 0x0E, 0x0E, 0x0C, 0x17, 0x0E, 0x0E,
    // 0x80–0x8F
    0x01, 0x01, 0x03, 0x03, 0x10, 0x10, 0x03, 0x03,
    0x05, 0x05, 0x07, 0x07, 0x05, 0x05, 0x07, 0x07,
    // 0x90–0x9F
    0x18, 0x18, 0x12, 0x12, 0x1C, 0x1C, 0x12, 0x12,
    0x05, 0x05, 0x07, 0x07, 0x05, 0x05, 0x07, 0x07,
    // 0xA0–0xAF
    0x09, 0x09, 0x0B, 0x0B, 0x16, 0x16, 0x0B, 0x0B,
    0x0D, 0x0D, 0xFE, 0xFE, 0x0D, 0x0D, 0xFE, 0xFE,
    // 0xB0–0xBF
    0x09, 0x09, 0x0B, 0x0B, 0x16, 0x16, 0x0B, 0x0B,
    0x0D, 0x0D, 0xFE, 0xFE, 0x0D, 0x0D, 0xFE, 0xFE,
    // 0xC0–0xCF
    0x01, 0x01, 0x03, 0x03, 0x10, 0x10, 0x03, 0x03,
    0x05, 0x05, 0x07, 0x07, 0x05, 0x05, 0x07, 0x07,
    // 0xD0–0xDF
    0x18, 0x18, 0x12, 0x12, 0x1C, 0x1C, 0x12, 0x12,
    0x05, 0x05, 0x07, 0x07, 0x05, 0x05, 0x07, 0x07,
    // 0xE0–0xEF
    0x09, 0x09, 0x0B, 0x0B, 0x16, 0x16, 0x0B, 0x0B,
    0x0D, 0x0D, 0xFE, 0xFE, 0x0D, 0x0D, 0xFE, 0xFE,
    // 0xF0–0xFF
    0x09, 0x09, 0x0B, 0x0B, 0x16, 0x16, 0x0B, 0x0B,
    0x0D, 0x0D, 0xFE, 0xFE, 0x0D, 0x0D, 0xFE, 0xFE,
];
