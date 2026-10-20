//! MapCell4A9890/4A9CA0, RevealCell4AA050 and Tactical6D8700 cache authority.
//! Counters and ground knowledge stay with OwnerVisibility. MapCell4A9CA0
//! updates these derived cache bytes only for the cell it visits, leaving its
//! neighbors stale; RevealCell4AA050 also rewrites its neighbors' ground
//! frames. Native constructor47BD28/47BD2E initializes -2/-2;
//! the separate47BD3A store initializes the sight counter to1.

use super::{
    FogState, GROUND_OPEN, GROUND_VISIBLE, InternedId, OwnerVisibility, PlaneWrite, ShroudKnowledge,
};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::{NativeCellQuery, ROW_NEIGHBORS};
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

/// VERA's derived current-sight projection; the original Cell counter/cache
/// operation is shared. Sustained sources already have an indexed lifetime,
/// while action/weapon reveal pairs retain a transient visible observation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MapRevealProjection {
    Source,
    Transient,
    /// Map5673A0 S32=true: skip a fully initialized/open receiver first;
    /// uncached4A9CA0 final0, not a retained source admission.
    UncachedConditional,
    /// Map5673A0 final0: Cell4876F0, without MapCell cache initialization.
    Fire,
}

/// Synchronous MapCell receiver arguments. The coordinate is read after both
/// edge queries from the cached Cell pointer. A Dummy may have been restamped.
pub(crate) struct MapCellNotification {
    pub(crate) cell: (u16, u16),
    pub(crate) fresh: bool,
}

/// The cells the two passes of `Map578100` can change
/// ([`FogState::sweep_pending_map_cell`],
/// [`FogState::refresh_map_cell_ground_frame`]), so an update visits what
/// changed rather than every allocated cell. Derived from the grids: when
/// `complete`, every pending cell is in `pending`, and every cell whose
/// cached ground frame differs from the one its ground bits and its
/// neighbours' give is in `changed`, beside a cell there, or beside the Dummy
/// while `dummy_changed`. The lists may repeat cells. A load clears
/// `complete`, and the next update visits every cell.
#[derive(Debug, Clone, Default)]
pub(super) struct EdgeCells {
    complete: bool,
    /// Cells whose pending bit rose since the last sweep.
    pub(super) pending: Vec<u32>,
    /// Cells whose ground bits or ground frame changed since the last
    /// refresh, other than by the refresh itself.
    changed: Vec<u32>,
    /// Whether the Dummy's ground bits or ground frame changed since then.
    dummy_changed: bool,
    /// Passes that hold cells taken from the lists and not yet visited. A
    /// nested update cannot see those cells, so it visits every cell.
    walking: u32,
}

impl EdgeCells {
    /// The lists of a grid whose cells all hold their constructor state: a
    /// cell without ground bits caches the -2 its frame gives.
    pub(super) fn complete() -> Self {
        Self {
            complete: true,
            ..Self::default()
        }
    }
}

/// The cells one pass of a shroud-edge update must visit
/// ([`FogState::take_edge_cells`]).
#[derive(Debug, Default)]
pub(crate) struct ListedEdgeCells {
    /// For the sweep, the cells that may be pending; for the refresh, the
    /// cells whose ground bits or ground frame changed.
    pub(crate) cells: Vec<(u16, u16)>,
    /// For the refresh, whether the Dummy's ground bits changed, so that
    /// every cell beside it may read a new frame.
    pub(crate) beside_dummy: bool,
}

/// `Tactical::6D8700`'s gates for one channel of a cell: whether it is
/// explored or open at all (its fallback frame is -1 rather than -2), and
/// whether its frame reads its neighbours.
fn edge_frame_gates(ground: u8, cache: MapCellEdgeCache, fog_channel: bool) -> (bool, bool) {
    if fog_channel {
        (cache.initialized || cache.fog_open, cache.initialized)
    } else {
        (
            ground & (GROUND_VISIBLE | GROUND_OPEN) != 0,
            ground & GROUND_VISIBLE != 0,
        )
    }
}

/// Whether a neighbour with these ground bits and cache sets its
/// [`EDGE_FRAME_BITS`] bit on the `[ground, fog]` channels: its visible bit,
/// or its cache's initialized bit, is clear.
fn edge_neighbor_clear(ground: u8, cache: MapCellEdgeCache) -> [bool; 2] {
    [ground & GROUND_VISIBLE == 0, !cache.initialized]
}

/// The neighbour bits of [`SHROUD_EDGE_LUT`]'s index, for [`ROW_NEIGHBORS`].
const EDGE_FRAME_BITS: [u8; 8] = [0x40, 0x80, 0x01, 0x20, 0x02, 0x10, 0x08, 0x04];

impl OwnerVisibility {
    /// Store `index`'s ground bits, listing the cell for the next shroud-edge
    /// refresh when they change.
    pub(super) fn set_ground_bits(&mut self, index: usize, bits: u8) {
        if self.ground_flags[index] != bits {
            self.ground_flags[index] = bits;
            self.edge_cells.changed.push(index as u32);
        }
    }

    /// What the ground channel of `Tactical::6D8700` reads of `key`, and the
    /// ground frame cached there.
    fn edge_inputs(&self, key: VisibilityCell) -> (u8, i8) {
        let (ground, cache) = self.map_cell_parts(key);
        (ground, cache.frames[0])
    }

    /// List `key` for the next shroud-edge refresh when its edge inputs
    /// differ from `before`.
    fn note_edge_inputs(&mut self, key: VisibilityCell, before: (u8, i8)) {
        if self.edge_inputs(key) == before {
            return;
        }
        match key {
            VisibilityCell::Real(index) => self.edge_cells.changed.push(index as u32),
            VisibilityCell::Dummy => self.edge_cells.dummy_changed = true,
        }
    }

    /// Map577C90's per-cell receiver577D1F..577D47, unlike577AB0's
    /// counter overwrite. Reuse the unique Cell487690 leave arithmetic and
    /// clear knowledge/cache bits only when the resulting counter is>=0.
    /// Gap counts, edge-frame bytes and pending20 survive the flag clear.
    pub(super) fn store_reshrouded_map_cell(&mut self, cell: (u16, u16)) {
        let Some(index) = self.index(cell.0, cell.1) else {
            return;
        };
        if self.shroud_knowledge[index].leave_reshrouded() {
            self.set_ground_bits(
                index,
                self.ground_flags[index] & !(GROUND_VISIBLE | GROUND_OPEN),
            );
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
        self.shroud_knowledge[index].store_bulk(reset);
        let ground = if reset {
            self.ground_flags[index] & !(GROUND_VISIBLE | GROUND_OPEN)
        } else {
            self.ground_flags[index] | GROUND_VISIBLE | GROUND_OPEN
        };
        self.set_ground_bits(index, ground);
        let cache = &mut self.map_cell_edge_cache[index];
        cache.initialized = !reset;
        cache.fog_open = !reset;
        self.publish_knowledge(index);
    }

    /// `cell`'s index when it and its eight neighbours are real cells at
    /// their own coordinates inside this grid: their lookups then neither
    /// miss nor stamp the Dummy, so their visibility cells are plain indices
    /// ([`crate::map::resolved_terrain::ResolvedTerrainGrid::interior_cells`]).
    pub(super) fn interior_index(
        &self,
        cell: (u16, u16),
        cells: Option<&NativeCellQuery<'_>>,
    ) -> Option<usize> {
        let (x, y) = (u32::from(cell.0), u32::from(cell.1));
        if x == 0 || y == 0 || x + 1 >= u32::from(self.width) || y + 1 >= u32::from(self.height) {
            return None;
        }
        if let Some(cells) = cells
            && !cells
                .terrain()
                .interior_cells()
                .is_some_and(|interior| interior.contains(cell))
        {
            return None;
        }
        Some(usize::from(cell.1) * usize::from(self.width) + usize::from(cell.0))
    }

    fn visibility_cell(
        &self,
        cell: (u16, u16),
        cells: Option<&NativeCellQuery<'_>>,
    ) -> VisibilityCell {
        if let Some(index) = self.interior_index(cell, cells) {
            return VisibilityCell::Real(index);
        }
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

    /// Original6D8700..6D8ADA: center lookup/gates, eight signed WORD
    /// neighbor lookups and sign-extended7F4194 table byte. Missing neighbors
    /// read the canonical Dummy's retained flags and stamp its coordinate.
    fn map_cell_edge_frame(
        &self,
        cell: (u16, u16),
        fog_channel: bool,
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> i8 {
        let channel = usize::from(fog_channel);
        if let Some(index) = self.interior_index(cell, native_cells) {
            return self.center_edge_frame(VisibilityCell::Real(index), fog_channel, || {
                self.interior_edge_masks(index)[channel]
            });
        }
        let center = self.visibility_cell(cell, native_cells);
        self.center_edge_frame(center, fog_channel, || {
            let mut mask = 0u8;
            for ((dx, dy), bit) in ROW_NEIGHBORS.into_iter().zip(EDGE_FRAME_BITS) {
                let key = self.visibility_cell(
                    (
                        (cell.0 as i16).wrapping_add(dx) as u16,
                        (cell.1 as i16).wrapping_add(dy) as u16,
                    ),
                    native_cells,
                );
                let (ground, cache) = self.map_cell_parts(key);
                if edge_neighbor_clear(ground, cache)[channel] {
                    mask |= bit;
                }
            }
            mask
        })
    }

    /// [`Self::map_cell_edge_frame`] for `center` from its gates and the
    /// neighbour bits `mask` reads, which `6D8700` reads only for an
    /// eligible center.
    fn center_edge_frame(
        &self,
        center: VisibilityCell,
        fog_channel: bool,
        mask: impl FnOnce() -> u8,
    ) -> i8 {
        let (ground, cache) = self.map_cell_parts(center);
        let (any, eligible) = edge_frame_gates(ground, cache, fog_channel);
        if !eligible {
            return if any { -1 } else { -2 };
        }
        SHROUD_EDGE_LUT[usize::from(mask())] as i8
    }

    /// The `[ground, fog]` neighbour bits of interior cell `index` that
    /// [`Self::map_cell_edge_frame`] sets. An interior cell's neighbours are
    /// the cells beside it in the grid, so reading them looks nothing up.
    fn interior_edge_masks(&self, index: usize) -> [u8; 2] {
        let width = self.width as isize;
        let mut masks = [0u8; 2];
        for ((dx, dy), bit) in ROW_NEIGHBORS.into_iter().zip(EDGE_FRAME_BITS) {
            let neighbor = index.wrapping_add_signed(isize::from(dy) * width + isize::from(dx));
            let clear = edge_neighbor_clear(
                self.ground_flags[neighbor],
                self.map_cell_edge_cache[neighbor],
            );
            for (mask, clear) in masks.iter_mut().zip(clear) {
                if clear {
                    *mask |= bit;
                }
            }
        }
        masks
    }

    /// The two frames MapCell `0x004A9CA0` (`0x004A9D09..0x004A9D41`) and
    /// `0x004A9890` (`0x004A98FD..0x004A9935`) store after a reveal changes
    /// `key` (`cell`): the ground frame and its open bit, the edge-input
    /// note, then the fog frame and its open bit. The ground store writes
    /// only the cell's own frame and open bit, so an interior cell's
    /// neighbours are read once for both frames, the fog frame's before the
    /// ground store rather than after it.
    fn store_map_cell_frames(
        &mut self,
        key: VisibilityCell,
        cell: (u16, u16),
        before: (u8, i8),
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> [i8; 2] {
        let masks = self
            .interior_index(cell, native_cells)
            .map(|index| self.interior_edge_masks(index));
        let frame = |vis: &Self, fog_channel: bool| match masks {
            Some(masks) => {
                vis.center_edge_frame(key, fog_channel, || masks[usize::from(fog_channel)])
            }
            None => vis.map_cell_edge_frame(cell, fog_channel, native_cells),
        };
        let ground = frame(self, false);
        let (_, flags, cache) = self.map_cell_parts_mut(key);
        cache.frames[0] = ground;
        if ground == -1 {
            *flags |= GROUND_OPEN;
        }
        self.note_edge_inputs(key, before);
        debug_assert!(
            self.interior_index(cell, native_cells)
                .is_none_or(|index| masks == Some(self.interior_edge_masks(index))),
            "the ground store changed a neighbour of {cell:?}"
        );
        let fog = frame(self, true);
        let (_, _, cache) = self.map_cell_parts_mut(key);
        cache.frames[1] = fog;
        if fog == -1 {
            cache.fog_open = true;
        }
        [ground, fog]
    }

    /// Original MapCell4A9CA0 on this plane; see [`FogState::reveal_map_cell`].
    fn reveal_map_cell(
        &mut self,
        cell: (u16, u16),
        final_pass: bool,
        native_cells: Option<&NativeCellQuery<'_>>,
        projection: MapRevealProjection,
    ) -> Option<MapCellNotification> {
        //4A9CA9 is distinct from each6D8713 center lookup below.
        let key = self.visibility_cell(cell, native_cells);
        let before = self.edge_inputs(key);
        if projection == MapRevealProjection::Fire {
            // Cell4876F0 ORs12C with18 and conditionally140 with20.
            // It neither initializes cache140 bits1/2 nor writes120/121.
            let (state, ground, _) = self.map_cell_parts_mut(key);
            state.fire_unshroud();
            *ground |= GROUND_VISIBLE | GROUND_OPEN;
            if let VisibilityCell::Real(index) = key {
                self.set_transient_visible(index);
                self.publish_knowledge(index);
            }
            self.note_edge_inputs(key, before);
            return None;
        }
        let (ground, old) = self.map_cell_parts(key);
        let fresh = !(old.initialized && ground & GROUND_VISIBLE != 0);
        let (state, ground, cache) = self.map_cell_parts_mut(key);
        cache.initialized = true;
        if final_pass {
            state.leave();
        } else {
            state.reveal();
        }
        if projection != MapRevealProjection::Source {
            state.project_reveal(matches!(key, VisibilityCell::Real(_)));
        }
        let open = state.open();
        if open {
            *ground |= GROUND_VISIBLE | GROUND_OPEN;
        } else {
            *ground &= !(GROUND_VISIBLE | GROUND_OPEN);
        }
        if let VisibilityCell::Real(index) = key {
            if projection != MapRevealProjection::Source {
                self.tracked.transient.push(index as u32);
            }
            if !open {
                self.tracked.cleared.push(index as u32);
            }
            self.publish_knowledge(index);
        }
        let frames = self.store_map_cell_frames(key, cell, before, native_cells);
        if !fresh && old.frames == frames {
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

    /// [`Self::reveal_map_cell`] with `projection` [`MapRevealProjection::Source`]
    /// for each of `cells` in turn ([`FogState::reveal_source_map_cells`]).
    pub(super) fn reveal_source_cells(
        &mut self,
        cells: &[(u16, u16)],
        final_pass: bool,
        native_cells: Option<&NativeCellQuery<'_>>,
    ) {
        for &cell in cells {
            self.reveal_map_cell(cell, final_pass, native_cells, MapRevealProjection::Source);
        }
    }

    /// Recompute and store each of `cells`' ground frame in turn
    /// ([`FogState::refresh_sight_border_cells`]).
    pub(super) fn refresh_ground_frames(
        &mut self,
        cells: &[(u16, u16)],
        native_cells: Option<&NativeCellQuery<'_>>,
    ) {
        for &cell in cells {
            let frame = self.map_cell_edge_frame(cell, false, native_cells);
            self.store_ground_frame(cell, frame, native_cells);
        }
    }

    /// Store `frame` as `cell`'s ground frame (`Cell+0x120`), `0x00567DA0`'s
    /// store (`0x00567F34`). Native looks the cell up before its 6D8700 call
    /// (`0x00567EE6..0x00567F15`); VERA computes the frame first and looks the
    /// cell up here. Residual: both reach the same cell, but for an off-map
    /// border cell with an off-map neighbour the shared Dummy is left on the
    /// border cell rather than on that neighbour, which only a Dummy read
    /// before the next missing lookup could see.
    fn store_ground_frame(
        &mut self,
        cell: (u16, u16),
        frame: i8,
        native_cells: Option<&NativeCellQuery<'_>>,
    ) {
        let key = self.visibility_cell(cell, native_cells);
        self.map_cell_parts_mut(key).2.frames[0] = frame;
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

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn map_cell_edge_cache_for_test(&self, cell: (u16, u16)) -> ([i8; 2], u32) {
        let cache = self.map_cell_edge_cache[self.index(cell.0, cell.1).unwrap()];
        (
            cache.frames,
            u32::from(cache.initialized) * 2 | u32::from(cache.fog_open),
        )
    }
}

/// Validate source projections through the same real/Dummy identity as the
/// receiver, including sparse native Cell registries. The supplied query must
/// be isolated so diagnostics cannot stamp the retained shared Dummy.
#[cfg(any(test, feature = "test-support"))]
impl FogState {
    pub(crate) fn assert_sight_projection_consistent(&self, cells: Option<&NativeCellQuery<'_>>) {
        for (&(id, viewer), admission) in &self.sight_admissions {
            if !admission.latched {
                continue;
            }
            let vis = &self.by_owner[&viewer];
            for &cell in &admission.cells {
                let key = vis.visibility_cell(cell, cells);
                let (ground, _) = vis.map_cell_parts(key);
                assert_eq!(
                    ground & (GROUND_VISIBLE | GROUND_OPEN),
                    GROUND_VISIBLE | GROUND_OPEN,
                    "source {id}: footprint cell {cell:?} lost its ground bits"
                );
            }
        }
        for vis in self.by_owner.values() {
            vis.assert_tracked();
        }
    }
}

impl FogState {
    /// Display::GrowShroud4ACAC0, Steam gamemd. Both
    /// passes scan the fixed512-square map in y-major order, admitting only
    /// In_Bounds568300. Capture every8&&!10 cell before any neighbor loses10.
    /// Existing pending140/20 bits participate too. Unlike578100, the
    /// receiver4ACDA0 changes neither counters nor cache bits/edge frames.
    pub(crate) fn grow_shroud(
        &mut self,
        viewer: InternedId,
        size: (i32, i32),
        native_cells: Option<&NativeCellQuery<'_>>,
    ) {
        let vis = self
            .by_owner
            .get_or_insert_with(viewer, || OwnerVisibility::new(self.width, self.height));
        for y in 0..512u16 {
            for x in 0..512u16 {
                if !crate::map::playfield::size_diamond_contains(
                    size.0,
                    size.1,
                    (x as i16, y as i16),
                ) {
                    continue;
                }
                let key = vis.visibility_cell((x, y), native_cells);
                let (state, ground, _) = vis.map_cell_parts_mut(key);
                if *ground & (GROUND_VISIBLE | GROUND_OPEN) == GROUND_VISIBLE {
                    state.mark_pending();
                    if let VisibilityCell::Real(index) = key {
                        vis.publish_knowledge(index);
                    }
                }
            }
        }
        for y in 0..512u16 {
            for x in 0..512u16 {
                if !crate::map::playfield::size_diamond_contains(
                    size.0,
                    size.1,
                    (x as i16, y as i16),
                ) {
                    continue;
                }
                let cell = (x, y);
                let key = vis.visibility_cell(cell, native_cells);
                let before = vis.edge_inputs(key);
                let (state, ground, _) = vis.map_cell_parts_mut(key);
                //4ACB68 clears20 BEFORE4ACDA0 tests8, including closed cells.
                if !state.take_pending() {
                    continue;
                }
                let hide = *ground & GROUND_VISIBLE != 0;
                if hide {
                    *ground &= !(GROUND_VISIBLE | GROUND_OPEN);
                    state.hide();
                }
                vis.note_edge_inputs(key, before);
                if let VisibilityCell::Real(index) = key {
                    if hide {
                        vis.tracked.cleared.push(index as u32);
                    }
                    vis.publish_knowledge(index);
                }
                if hide {
                    for neighbor in cell_neighbors(cell) {
                        let key = vis.visibility_cell(neighbor, native_cells);
                        let before = vis.edge_inputs(key);
                        let (_, ground, _) = vis.map_cell_parts_mut(key);
                        *ground &= !GROUND_OPEN;
                        vis.note_edge_inputs(key, before);
                        if let VisibilityCell::Real(index) = key {
                            vis.publish_knowledge(index);
                        }
                    }
                }
            }
        }
    }

    /// Original Map567F70's cell receiver: only counter==0 is concealed.
    /// Negative sustained sight and positive shroud/gap counters survive;
    /// pending20, cache bits/frames and gap count are untouched.
    pub(crate) fn conceal_map_cell(
        &mut self,
        viewer: InternedId,
        cell: (u16, u16),
        native_cells: Option<&NativeCellQuery<'_>>,
    ) {
        let vis = self
            .by_owner
            .get_or_insert_with(viewer, || OwnerVisibility::new(self.width, self.height));
        let key = vis.visibility_cell(cell, native_cells);
        let before = vis.edge_inputs(key);
        let (state, ground, _) = vis.map_cell_parts_mut(key);
        if !state.conceal() {
            return;
        }
        *ground &= !(GROUND_VISIBLE | GROUND_OPEN);
        vis.note_edge_inputs(key, before);
        if let VisibilityCell::Real(index) = key {
            vis.tracked.cleared.push(index as u32);
            vis.publish_knowledge(index);
        }
    }

    /// First578100 pass, one allocated Cell: pending20 clears ground18 and
    /// cache3/20 before its synchronous notification. Ground frame bytes do
    /// not change until the second pass. Native notifies only old frame-2.
    pub(crate) fn sweep_pending_map_cell(
        &mut self,
        viewer: InternedId,
        cell: (u16, u16),
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> Option<MapCellNotification> {
        let vis = self.by_owner.get_mut(&viewer)?;
        let key = vis.visibility_cell(cell, native_cells);
        let before = vis.edge_inputs(key);
        let (state, ground, cache) = vis.map_cell_parts_mut(key);
        if !state.pending() {
            return None;
        }
        state.sweep();
        *ground &= !(GROUND_VISIBLE | GROUND_OPEN);
        cache.initialized = false;
        cache.fog_open = false;
        let notify = cache.frames[0] == -2;
        vis.note_edge_inputs(key, before);
        if let VisibilityCell::Real(index) = key {
            vis.tracked.cleared.push(index as u32);
            vis.publish_knowledge(index);
        }
        notify.then_some(MapCellNotification { cell, fresh: true })
    }

    /// Second578100 pass. Recompute every allocated Cell's ground channel
    /// through the sole Tactical6D8700 owner; changed frame-2 synchronously
    /// notifies. No counter, pending bit, fog channel or open-bit write. The
    /// stored frame is the one the ground bits give, so the cell is not
    /// listed for the next refresh ([`EdgeCells`]).
    pub(crate) fn refresh_map_cell_ground_frame(
        &mut self,
        viewer: InternedId,
        cell: (u16, u16),
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> Option<MapCellNotification> {
        let vis = self.by_owner.get_mut(&viewer)?;
        let frame = vis.map_cell_edge_frame(cell, false, native_cells);
        let key = vis.visibility_cell(cell, native_cells);
        let (_, _, cache) = vis.map_cell_parts_mut(key);
        if frame == cache.frames[0] {
            return None;
        }
        cache.frames[0] = frame;
        (frame == -2).then_some(MapCellNotification { cell, fresh: true })
    }

    /// Map5673A0 at567856..567870, selected S24=false/S32=true:
    /// skip only when both ground12C bits8/10 and cache140 bits1/2 are set.
    /// Applies the plane's waiting writes first ([`super::Planes`]).
    pub(crate) fn map_cell_fully_known(
        &mut self,
        viewer: InternedId,
        cell: (u16, u16),
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> bool {
        let Some(vis) = self.by_owner.get_mut(&viewer) else {
            return false;
        };
        let key = vis.visibility_cell(cell, native_cells);
        let (ground, cache) = vis.map_cell_parts(key);
        ground & (GROUND_VISIBLE | GROUND_OPEN) == GROUND_VISIBLE | GROUND_OPEN
            && cache.initialized
            && cache.fog_open
    }

    /// Map567DA0 border tail for each of `cells` in turn: recompute only
    /// ground frame120 through Tactical6D8700. The native dirty138/6DA7D0 is
    /// presentation invalidation; no sight counter, ground bits, fog frame or
    /// discovery callback changes. Full A reconstruction per Fog view
    /// generation uses current knowledge and neighbours, replacing native
    /// dirty138/6DA7D0. The stored frame remains authoritative for later Cell
    /// change/notification decisions. A viewer without a plane gets one,
    /// whose cells read -2 as a missing plane does.
    pub(crate) fn refresh_sight_border_cells(
        &mut self,
        viewer: InternedId,
        cells: &[(u16, u16)],
        native_cells: Option<&NativeCellQuery<'_>>,
    ) {
        if cells.is_empty() {
            return;
        }
        self.by_owner.write(
            viewer,
            (self.width, self.height),
            PlaneWrite::Border,
            cells,
            native_cells,
        );
    }

    /// Original Display4A9890, via Map653810/Execute16. Unlike4A9CA0 it
    /// does not touch sight/gap counters; unlike4AA050 it initializes both
    /// ground and fog edge channels. Fresh and changed are distinct: the
    /// receiver5865F0 runs only on changed, with the original fresh argument.
    /// Original leaf/frontier execution: tools/spatial_oracle/
    /// trigger_forced_map.{json,meta.json}, Steam gamemd. Its active caller
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
            .get_or_insert_with(viewer, || OwnerVisibility::new(self.width, self.height));
        let key = vis.visibility_cell(cell, native_cells);
        let before = vis.edge_inputs(key);
        let (ground, old) = vis.map_cell_parts(key);
        let fresh = !(old.initialized && ground & GROUND_VISIBLE != 0);
        let (state, ground, cache) = vis.map_cell_parts_mut(key);
        cache.initialized = true;
        *ground |= GROUND_VISIBLE;
        state.open_mapped();
        if let VisibilityCell::Real(index) = key {
            vis.publish_knowledge(index);
        }
        let frames = vis.store_map_cell_frames(key, cell, before, native_cells);
        //4A9981..4A9C48 interleaves the two channels for each neighbor.
        // The guarded recursive +90 arms cannot occur: a neighbor without
        // its eligible or open bit returns-2 in6D8700, not another frame.
        for neighbor in cell_neighbors(cell) {
            self.propagate_map_cell_edge(viewer, neighbor, false, native_cells);
            self.propagate_map_cell_edge(viewer, neighbor, true, native_cells);
        }
        if !fresh && old.frames == frames {
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

    /// Map receiver state: MapCell4A9CA0..4A9DC9's final0/1 leaves, or
    /// Map5673A0's fire4876F0 leaf without cache initialization/notification.
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
        projection: MapRevealProjection,
    ) -> Option<MapCellNotification> {
        self.by_owner
            .get_or_insert_with(viewer, || OwnerVisibility::new(self.width, self.height))
            .reveal_map_cell(cell, final_pass, native_cells, projection)
    }

    /// [`Self::reveal_map_cell`] with `projection` [`MapRevealProjection::Source`]
    /// for each of `cells` in turn, for a viewer whose notifications no
    /// receiver hears: the plane is looked up once for the whole walk.
    pub(crate) fn reveal_source_map_cells(
        &mut self,
        viewer: InternedId,
        cells: &[(u16, u16)],
        final_pass: bool,
        native_cells: Option<&NativeCellQuery<'_>>,
    ) {
        if cells.is_empty() {
            return;
        }
        self.by_owner.write(
            viewer,
            (self.width, self.height),
            PlaneWrite::Reveal { final_pass },
            cells,
            native_cells,
        );
    }

    /// [`OwnerVisibility::map_cell_edge_frame`] on `viewer`'s plane; -2
    /// without one.
    pub(crate) fn map_cell_edge_frame(
        &self,
        viewer: InternedId,
        cell: (u16, u16),
        fog_channel: bool,
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> i8 {
        self.by_owner.get(&viewer).map_or(-2, |vis| {
            vis.map_cell_edge_frame(cell, fog_channel, native_cells)
        })
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
    /// gap, fire and some whole-map writers (`0x006FB170`, `0x006FB470`,
    /// `0x004876F0`, `0x00577D90`, `0x00577AB0`) leave as they
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
            .get_or_insert_with(viewer, || OwnerVisibility::new(self.width, self.height));
        let center = vis.visibility_cell(cell, native_cells);
        let before = vis.edge_inputs(center);
        let (state, ground, _) = vis.map_cell_parts_mut(center);
        let fresh = *ground & GROUND_VISIBLE == 0;
        *ground |= GROUND_VISIBLE;
        state.open_mapped();
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
        vis.note_edge_inputs(center, before);
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
        let before = vis.edge_inputs(key);
        let (_, ground, cache) = vis.map_cell_parts_mut(key);
        let channel = usize::from(fog_channel);
        if frame == -1 {
            if fog_channel {
                cache.fog_open = true;
            } else {
                *ground |= GROUND_OPEN;
            }
            vis.note_edge_inputs(key, before);
            for next in cell_neighbors(neighbor) {
                let key = self.by_owner[&viewer].visibility_cell(next, native_cells);
                let frame = self.map_cell_edge_frame(viewer, next, fog_channel, native_cells);
                let vis = self.by_owner.get_mut(&viewer).unwrap();
                let before = vis.edge_inputs(key);
                vis.map_cell_parts_mut(key).2.frames[channel] = frame;
                vis.note_edge_inputs(key, before);
            }
        } else if frame >= 0 {
            cache.frames[channel] = frame;
            vis.note_edge_inputs(key, before);
        }
    }

    pub(super) fn is_dummy_ground_unshrouded(&self, viewer: InternedId) -> bool {
        self.by_owner
            .get(&viewer)
            .is_some_and(|vis| vis.dummy_visibility.ground & GROUND_VISIBLE != 0)
    }
}

/// What `Map578100`'s walk takes from [`EdgeCells`]: the sweep's list
/// (`refresh` false) or the ground-frame refresh's.
impl FogState {
    /// Take what `viewer`'s next sweep, or with `refresh` its refresh, must
    /// visit on a walk over `terrain_size` storage, until
    /// [`Self::finish_edge_walk`]. `None` when it must visit every cell: the
    /// lists are incomplete, a walk that holds taken cells is under way, or
    /// this grid does not cover the storage, so a real cell can read the
    /// Dummy's knowledge.
    pub(crate) fn take_edge_cells(
        &mut self,
        viewer: InternedId,
        refresh: bool,
        terrain_size: (u16, u16),
    ) -> Option<ListedEdgeCells> {
        let width = self.width;
        let covers = terrain_size.0 <= self.width && terrain_size.1 <= self.height;
        let Some(vis) = self.by_owner.get_mut(&viewer) else {
            // Without a map plane neither pass reads or writes anything.
            return Some(ListedEdgeCells::default());
        };
        let edges = &mut vis.edge_cells;
        let listed = if refresh {
            std::mem::take(&mut edges.changed)
        } else {
            std::mem::take(&mut edges.pending)
        };
        let beside_dummy = refresh && std::mem::take(&mut edges.dummy_changed);
        let nested = edges.walking > 0;
        edges.walking += 1;
        if !edges.complete || !covers || nested {
            return None;
        }
        Some(ListedEdgeCells {
            cells: cells_of(&listed, width),
            beside_dummy,
        })
    }

    /// End the walk [`Self::take_edge_cells`] began.
    pub(crate) fn finish_edge_walk(&mut self, viewer: InternedId) {
        // A plane its callbacks created took nothing.
        if let Some(vis) = self.by_owner.get_mut(&viewer) {
            vis.edge_cells.walking = vis.edge_cells.walking.saturating_sub(1);
        }
    }

    /// How many cells that pass's list holds now: a mark for
    /// [`Self::edge_cells_since`].
    pub(crate) fn edge_cells_listed(&self, viewer: InternedId, refresh: bool) -> usize {
        self.by_owner.get(&viewer).map_or(0, |vis| {
            if refresh {
                vis.edge_cells.changed.len()
            } else {
                vis.edge_cells.pending.len()
            }
        })
    }

    /// The cells that pass's list gained after `mark`, and for the refresh
    /// whether the Dummy's ground bits changed since the take.
    pub(crate) fn edge_cells_since(
        &self,
        viewer: InternedId,
        refresh: bool,
        mark: usize,
    ) -> ListedEdgeCells {
        let Some(vis) = self.by_owner.get(&viewer) else {
            return ListedEdgeCells::default();
        };
        let edges = &vis.edge_cells;
        let list = if refresh {
            &edges.changed
        } else {
            &edges.pending
        };
        ListedEdgeCells {
            cells: cells_of(list.get(mark..).unwrap_or_default(), self.width),
            beside_dummy: refresh && edges.dummy_changed,
        }
    }

    /// List `cells` for that pass of `viewer`'s next update.
    pub(crate) fn relist_edge_cells(
        &mut self,
        viewer: InternedId,
        refresh: bool,
        cells: impl IntoIterator<Item = (u16, u16)>,
    ) {
        let Some(vis) = self.by_owner.get_mut(&viewer) else {
            return;
        };
        for (x, y) in cells {
            if let Some(index) = vis.index(x, y) {
                let edges = &mut vis.edge_cells;
                if refresh {
                    edges.changed.push(index as u32);
                } else {
                    edges.pending.push(index as u32);
                }
            }
        }
    }

    /// Drop `viewer`'s lists for an update that visits every cell without
    /// them; the next listed update visits every cell too.
    pub(crate) fn untrack_edge_cells(&mut self, viewer: InternedId) {
        if let Some(vis) = self.by_owner.get_mut(&viewer) {
            vis.edge_cells = EdgeCells::default();
        }
    }

    /// Mark `viewer`'s lists complete: its update just visited every cell.
    pub(crate) fn complete_edge_cells(&mut self, viewer: InternedId) {
        if let Some(vis) = self.by_owner.get_mut(&viewer) {
            vis.edge_cells.complete = true;
        }
    }

    /// Whether `cell`'s ground frame reads its neighbours, the gate of
    /// [`Self::map_cell_edge_frame`] that decides its neighbour lookups.
    pub(crate) fn ground_frame_reads_neighbors(
        &self,
        viewer: InternedId,
        cell: (u16, u16),
        native_cells: Option<&NativeCellQuery<'_>>,
    ) -> bool {
        self.by_owner.get(&viewer).is_some_and(|vis| {
            let (ground, cache) = vis.map_cell_parts(vis.visibility_cell(cell, native_cells));
            edge_frame_gates(ground, cache, false).1
        })
    }
}

#[cfg(any(test, feature = "test-support"))]
impl FogState {
    /// Panics unless `viewer`'s lists, when complete, cover `cell` as
    /// [`EdgeCells`] promises. `beside_dummy`: the cell has an unallocated
    /// neighbour. `cells` must not stamp the canonical Dummy.
    pub(crate) fn assert_edge_cell_tracked(
        &self,
        viewer: InternedId,
        cell: (u16, u16),
        beside_dummy: bool,
        cells: &NativeCellQuery<'_>,
    ) {
        let Some(vis) = self.by_owner.get(&viewer) else {
            return;
        };
        let VisibilityCell::Real(index) = vis.visibility_cell(cell, Some(cells)) else {
            return;
        };
        let edges = &vis.edge_cells;
        if !edges.complete {
            return;
        }
        assert!(
            !vis.shroud_knowledge[index].pending() || edges.pending.contains(&(index as u32)),
            "{cell:?}: unlisted pending cell"
        );
        if self.map_cell_edge_frame(viewer, cell, false, Some(cells))
            == vis.map_cell_edge_cache[index].frames[0]
        {
            return;
        }
        let listed = std::iter::once((0, 0))
            .chain(ROW_NEIGHBORS)
            .any(|(dx, dy)| {
                let (x, y) = (
                    (cell.0 as i16).wrapping_add(dx) as u16,
                    (cell.1 as i16).wrapping_add(dy) as u16,
                );
                vis.index(x, y)
                    .is_some_and(|index| edges.changed.contains(&(index as u32)))
            });
        assert!(
            listed || beside_dummy && edges.dummy_changed,
            "{cell:?}: unlisted stale ground frame"
        );
    }
}

/// The cells at `indices` of a grid `width` wide.
fn cells_of(indices: &[u32], width: u16) -> Vec<(u16, u16)> {
    let width = u32::from(width);
    indices
        .iter()
        .map(|&index| ((index % width) as u16, (index / width) as u16))
        .collect()
}

/// The eight cells around `cell` in `0x0089F688` order, with the native signed
/// WORD adds.
fn cell_neighbors(cell: (u16, u16)) -> impl Iterator<Item = (u16, u16)> {
    crate::util::direction::DIRECTION_DELTAS
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
