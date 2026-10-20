//! Per-cell mutable overlay state — runtime fork of the immutable map OverlayPack.
//!
//! Mirrors CellClass +0x44 (OverlayTypeIndex) and +0x11E (OverlayData) from gamemd.exe.
//! Seeded from map data at init, mutated during gameplay by ore growth, wall damage,
//! and bridge overlay damage.
//!
//! Dependency rules: depends on map/overlay (OverlayEntry for seeding).
//! Never depends on render/, ui/, audio/, net/.

use crate::map::authored_overlay::{FinalizedOverlayCell, FinalizedOverlayPayload};
use crate::map::overlay::{OverlayDataPack, OverlayEntry};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::overlay_types::{OverlayTypeRegistry, clears_tiberium_on_slope};
use crate::sim::intern::InternedId;
use crate::sim::pathfinding::zone_incremental::ZoneRepairKind;
use crate::util::direction::DIRECTION_DELTAS;
use crate::util::native_x87::object_distance;

/// The overlay index `BuildingClass::AfterDestruction` stamps on a destroyed
/// building's foundation (`MOV [ESI+0x44],0xEF` at `0x0044202C`). Retail
/// `[OverlayTypes]` puts `RUBBLE_OVERLAY` (`IsRubble=yes`) there.
pub const RUBBLE_OVERLAY_INDEX: u8 = 0xEF;

/// Building facts consumed by the one-shot map-wall owner reconstruction pass.
/// Callers preserve BuildingClass allocation order in this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapWallOwnerCandidate {
    pub owner: InternedId,
    pub world_x: i32,
    pub world_y: i32,
    pub world_z: i32,
    pub object_alive: bool,
    pub cell_marked: bool,
    pub house_wall_owner: bool,
}

/// Per-cell mutable overlay state — mirrors CellClass +0x44 / +0x11E.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
pub struct OverlayCell {
    /// Overlay type index into OverlayTypeRegistry. None = no overlay.
    pub overlay_id: Option<u8>,
    /// Multi-purpose data byte:
    /// - Ore/gems (Tiberium=true): density 0-11 (SHP frame index)
    /// - Walls (Wall=true): (damage_level << 4) | connectivity_bitmask
    ///   Connectivity: N=1, E=2, S=4, W=8
    /// - Bridges: damage state 0-17 (EW 0-8, NS 9-17)
    /// - Other: raw frame index
    pub overlay_data: u8,
    /// Owning house for a placed wall. Map-pack overlays start unowned.
    ///
    /// A cleared GAWALL/NAWALL cell may retain this value after native's
    /// isolated-neighbor cleanup. It is inert while `overlay_id` is `None` and
    /// is overwritten by the next placement.
    pub wall_owner: Option<InternedId>,
    /// The destroyed building type whose rubble this cell draws
    /// (`CellClass+0x40`). Only `BuildingClass::AfterDestruction @
    /// 0x00441F60` writes it: null on every foundation cell, then the type on
    /// the origin cell. The overlay draws read it only under an `IsRubble`
    /// identity (`0x0047F6A0`, `0x0047F510`), which no other runtime writer
    /// creates, so the whole-cell overlay writers resetting it is invisible.
    /// Serialized but not hashed: no simulation reader.
    pub rubble_building_type: Option<InternedId>,
}

impl OverlayCell {
    /// This cell's state hash record. `rubble_building_type` has no
    /// simulation reader (see its note), so the hash leaves it out.
    pub(crate) fn hash_record(&self) -> [u8; 8] {
        let Self {
            overlay_id,
            overlay_data,
            wall_owner,
            rubble_building_type: _,
        } = *self;
        let [o0, o1, o2, o3] = wall_owner.map_or(0, InternedId::index).to_le_bytes();
        [
            u8::from(overlay_id.is_some()),
            overlay_id.unwrap_or(0),
            overlay_data,
            u8::from(wall_owner.is_some()),
            o0,
            o1,
            o2,
            o3,
        ]
    }
}

/// Mutable overlay state grid — seeded from map [OverlayPack] at init,
/// mutated during gameplay by ore growth, wall damage, bridge damage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OverlayGrid {
    width: u16,
    height: u16,
    cells: Vec<OverlayCell>,
    /// Retained wall and Foot contribution to CellClass+0x122, one byte per
    /// cell. It is the sole wall authority, even when all zero: final wall
    /// identities are never scanned as a substitute.
    retained_neighbor_counts: Vec<u8>,
    #[serde(skip, default)]
    foot_neighbor_changes: crate::sim::cell_neighbors::NeighborCountChanges,
    /// Cells mutated this tick — drained by Simulation's end-of-frame finalizer
    /// before the authoritative hash. Not part of game state; never serialized.
    #[serde(skip, default)]
    dirty_cells: Vec<(u16, u16)>,
    /// Test-only epoch bumped by mutators that can change overlay identity or
    /// retained neighbor bytes; movement reads `wall_plane_epoch` instead.
    /// Not every internal write bumps it; the public and wall-transaction entry
    /// points do. Not game state; never serialized.
    #[cfg(test)]
    #[serde(skip)]
    mutation_epoch: u64,
    /// Bumped only when the retained wall-neighbour plane itself changes. The
    /// movement blocker plane reads nothing else from a grid that retains one,
    /// so ore growth and harvesting, which change overlay bytes most frames,
    /// do not make it rebuild. Not game state; never serialized.
    #[serde(skip)]
    wall_plane_epoch: u64,
    /// Cells whose overlay identity was erased this tick *without* a native
    /// attribute recalc. Presentation must drop their render entry, but
    /// `CrateSlot__RemoveCrateOverlayFromCell @ 0x004A1AA0` ends at its two
    /// field writes with no `CellClass::RecalcAttributes @ 0x0047D2B0` tail —
    /// so the cell keeps the land type the removed overlay gave it, and these
    /// coordinates must NOT enter `dirty_cells`, whose frame-tail drain
    /// re-derives land, speed and zone from the pristine tile.
    /// Not part of game state; never serialized.
    #[serde(skip, default)]
    removed_render_cells: Vec<(u16, u16)>,
    /// Coordinates whose synchronous overlay recalc changed movement authority,
    /// in first-seen order. `Simulation` publishes each through the one-cell
    /// navigation owner at its next path reader or the frame tail, whichever
    /// comes first; zone IDs stay with the mutation owner's native helper.
    #[serde(skip, default)]
    synchronous_navigation_cells: Vec<(u16, u16)>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OverlayRecalcOutcome {
    pub navigation_changed: bool,
    pub zone_changed: bool,
}

impl OverlayGrid {
    /// Create an empty grid with no overlays and an all-zero wall plane.
    pub fn new(width: u16, height: u16) -> Self {
        let count = width as usize * height as usize;
        Self {
            width,
            height,
            cells: vec![OverlayCell::default(); count],
            retained_neighbor_counts: vec![0u8; count],
            foot_neighbor_changes: Default::default(),
            dirty_cells: Vec::new(),
            #[cfg(test)]
            mutation_epoch: 0,
            wall_plane_epoch: 0,
            removed_render_cells: Vec::new(),
            synchronous_navigation_cells: Vec::new(),
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn seed_neighbor_counts_for_tests(&mut self, value: u8) {
        self.retained_neighbor_counts.fill(value);
        self.wall_plane_epoch = self.wall_plane_epoch.wrapping_add(1);
    }

    /// Consume the one finalized map payload. This boundary has no raw-pack,
    /// rules, source-filter, RNG, Mark, or Recalc capability.
    pub(crate) fn from_finalized_map_payload(payload: FinalizedOverlayPayload) -> Self {
        let (width, height, finalized, retained_neighbor_counts) = payload.into_parts();
        let expected = usize::from(width) * usize::from(height);
        assert_eq!(finalized.len(), expected, "finalized overlay cell shape");
        assert_eq!(
            retained_neighbor_counts.len(),
            expected,
            "finalized wall-neighbor plane shape"
        );
        let cells = finalized
            .into_iter()
            .map(|cell| OverlayCell {
                overlay_id: cell.overlay_id(),
                overlay_data: cell.state(),
                wall_owner: None,
                rubble_building_type: None,
            })
            .collect();
        Self {
            width,
            height,
            cells,
            retained_neighbor_counts,
            foot_neighbor_changes: Default::default(),
            dirty_cells: Vec::new(),
            #[cfg(test)]
            mutation_epoch: 0,
            wall_plane_epoch: 0,
            removed_render_cells: Vec::new(),
            synchronous_navigation_cells: Vec::new(),
        }
    }

    /// Borrow the exact live CellClass identity/state pair for one load-time
    /// Recalc. Runtime-only owner metadata is deliberately excluded.
    pub(crate) fn finalized_map_cell(&self, rx: u16, ry: u16) -> Option<FinalizedOverlayCell> {
        index_of(self.width, self.height, rx, ry).map(|index| {
            let cell = &self.cells[index];
            FinalizedOverlayCell::from_parts(
                cell.overlay_id.map_or(-1, i32::from),
                cell.overlay_data,
            )
        })
    }

    /// Commit the one identity/state pair returned by the final authored-load
    /// Recalc without disturbing retained wall counts or later wall ownership.
    pub(crate) fn write_finalized_map_cell(
        &mut self,
        rx: u16,
        ry: u16,
        finalized: FinalizedOverlayCell,
    ) -> bool {
        let Some(index) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        let cell = &mut self.cells[index];
        cell.overlay_id = finalized.overlay_id();
        cell.overlay_data = finalized.state();
        true
    }

    /// Seed from parsed map overlay entries, bridges included.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_overlay_entries(entries: &[OverlayEntry], width: u16, height: u16) -> Self {
        let mut grid = Self::new(width, height);
        for entry in entries {
            if let Some(idx) = index_of(width, height, entry.rx, entry.ry) {
                grid.cells[idx] = OverlayCell {
                    overlay_id: Some(entry.overlay_id),
                    overlay_data: entry.frame,
                    wall_owner: None,
                    rubble_building_type: None,
                };
            }
        }
        grid
    }

    /// Seed overlay identities from `[OverlayPack]`, then apply the raw
    /// `[OverlayDataPack]` byte to every cell when that pack is present.
    ///
    /// The native map reader performs the data-pack overwrite after stamping
    /// overlay identities, including cells whose overlay identity is empty.
    /// Initialization writes directly so it creates no runtime dirtiness.
    #[cfg(any(test, feature = "test-support"))]
    pub fn from_overlay_packs(
        entries: &[OverlayEntry],
        data: &OverlayDataPack,
        width: u16,
        height: u16,
    ) -> Self {
        let mut grid = Self::from_overlay_entries(entries, width, height);
        if data.is_present() {
            for ry in 0..height {
                for rx in 0..width {
                    let idx = ry as usize * width as usize + rx as usize;
                    grid.cells[idx].overlay_data = data.byte_at(rx, ry);
                }
            }
        }
        grid
    }

    /// The random-map generator's overlays, written into their cells the way
    /// the generator writes them. Native never runs `OverlayClass::Mark` for
    /// them: `ReadMapOverlayPacks @ 0x005FD2E0` has one caller,
    /// `InitializeScenarioFromINI` (`0x00687A34`), which a generated launch
    /// reaches only through `RandomMapGenerator::InitMapFromSyntheticINI @
    /// 0x00599650` (`0x00598A74`), on a synthetic INI with no overlay packs
    /// and before any overlay exists. The generator then stores each overlay
    /// index (`CellClass+0x44`) and data byte (`+0x11E`) itself, with no
    /// `OverlayClass` constructed: ore through
    /// `RandomMapGenerator::CreateTiberium` (`0x00598EF4`, writer
    /// `0x005A28C0`), rocks through `0x005A3AE0` (`0x00599232`), low-bridge
    /// decks through `0x0058F2C0`. So no slope, art, crate or passability
    /// gate applies, no crate sentinel is written and no wall neighbour count
    /// moves; `CellClass::RecalcAttributes` afterwards clears the resources
    /// the slope rule rejects, data included.
    pub fn from_generated_overlays(
        entries: &[OverlayEntry],
        data: &OverlayDataPack,
        terrain: &mut ResolvedTerrainGrid,
        registry: &OverlayTypeRegistry,
    ) -> Self {
        let width = terrain.width();
        let height = terrain.height();
        let mut grid = Self::new(width, height);
        for entry in entries {
            if let Some(index) = terrain.index(entry.rx, entry.ry) {
                grid.cells[index].overlay_id = Some(entry.overlay_id);
            }
        }
        if data.is_present() {
            for ry in 0..height {
                for rx in 0..width {
                    if let Some(index) = terrain.index(rx, ry) {
                        grid.cells[index].overlay_data = data.byte_at(rx, ry);
                    }
                }
            }
        }
        for entry in entries {
            recalc_overlay_passability(&mut grid, terrain, registry, entry.rx, entry.ry);
        }
        grid
    }

    /// Read cell at (rx, ry). Returns default (no overlay) for out-of-bounds.
    /// Every cell in row-major order (`ry * width + rx`).
    pub(crate) fn cells(&self) -> &[OverlayCell] {
        &self.cells
    }

    pub fn cell(&self, rx: u16, ry: u16) -> &OverlayCell {
        match index_of(self.width, self.height, rx, ry) {
            Some(idx) => &self.cells[idx],
            None => &DEFAULT_CELL,
        }
    }

    /// Epoch of mutating entry points; see the field note.
    #[cfg(test)]
    pub fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }

    /// Bump the test-only `mutation_epoch`; a no-op in other builds.
    #[inline]
    fn bump_mutation_epoch(&mut self) {
        #[cfg(test)]
        {
            self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        }
    }

    /// Mutable access to cell. Panics if out-of-bounds.
    #[cfg(any(test, feature = "test-support"))]
    pub fn cell_mut_for_test(&mut self, rx: u16, ry: u16) -> &mut OverlayCell {
        self.bump_mutation_epoch();
        let idx =
            index_of(self.width, self.height, rx, ry).expect("OverlayGrid::cell_mut out of bounds");
        &mut self.cells[idx]
    }

    /// Empty the whole cell without dirty-cell or neighbor bookkeeping: a
    /// resource that cannot lie on its slope ([`recalc_overlay_passability`])
    /// and the resource under an authored terrain object at map load.
    /// Panics if out-of-bounds.
    pub(crate) fn clear_cell_silently(&mut self, rx: u16, ry: u16) {
        self.bump_mutation_epoch();
        let idx = index_of(self.width, self.height, rx, ry)
            .expect("OverlayGrid::clear_cell_silently out of bounds");
        self.cells[idx] = OverlayCell::default();
    }

    /// Update only CellClass's bridge-overlay identity.
    ///
    /// Bridge damage/repair owns neither OverlayData nor wall ownership, and
    /// it performs its terrain recalc synchronously in the world cascade. This
    /// deliberately avoids the ordinary dirty-cell channel used by independent
    /// runtime overlay placement/removal.
    pub fn write_bridge_overlay_identity(&mut self, rx: u16, ry: u16, overlay_byte: u8) -> bool {
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        let overlay_id = (overlay_byte != u8::MAX).then_some(overlay_byte);
        if self.cells[idx].overlay_id == overlay_id {
            return false;
        }
        self.cells[idx].overlay_id = overlay_id;
        true
    }

    /// The epoch under which the movement blocker plane's wall part is
    /// current: the wall plane's own.
    pub(crate) fn blocker_plane_epoch(&self) -> u64 {
        self.wall_plane_epoch
    }

    /// Read the retained wall and Foot contribution plane. All-zero is
    /// authoritative and must not fall back to a final-identity scan.
    pub(crate) fn retained_neighbor_counts(&self) -> &[u8] {
        &self.retained_neighbor_counts
    }

    pub(crate) fn retained_neighbor_count_storage_len(&self) -> usize {
        self.retained_neighbor_counts.len()
    }

    pub(crate) fn foot_neighbor_revision(&self) -> u64 {
        self.foot_neighbor_changes.revision()
    }

    pub(crate) fn foot_neighbor_changes_since(
        &self,
        revision: u64,
    ) -> Option<impl Iterator<Item = (usize, u8)> + '_> {
        self.foot_neighbor_changes.since(revision)
    }

    /// Foot counter loops use one saved packed source for all eight probes,
    /// unlike wall Adjacent_Cell which rereads a potentially restamped Dummy.
    pub(crate) fn adjust_foot_neighbor_source(
        &mut self,
        terrain: Option<&ResolvedTerrainGrid>,
        source: (i16, i16),
        add: bool,
    ) {
        self.bump_mutation_epoch();
        let counts = &mut self.retained_neighbor_counts;
        let terrain = terrain.expect("retained neighbor counts need native CellClass lookup");
        for (dx, dy) in DIRECTION_DELTAS {
            let target = native_runtime_overlay_cell_lookup(
                self.width,
                self.height,
                Some(terrain),
                i32::from(source.0.wrapping_add(dx as i16)),
                i32::from(source.1.wrapping_add(dy as i16)),
            );
            match target {
                Some(NativeRuntimeOverlayCell::Real(x, y)) => {
                    let index = usize::from(y) * usize::from(self.width) + usize::from(x);
                    counts[index] = if add {
                        counts[index].wrapping_add(1)
                    } else {
                        counts[index].wrapping_sub(1)
                    };
                    if !self.foot_neighbor_changes.record(index, add) {
                        self.wall_plane_epoch = self.wall_plane_epoch.wrapping_add(1);
                    }
                }
                Some(NativeRuntimeOverlayCell::Dummy) => {
                    terrain.shared_cell_dummy().adjust_neighbor_count(add)
                }
                None => unreachable!("native lookup always yields CellClass"),
            }
        }
    }

    fn adjust_retained_wall_neighbor_source(
        &mut self,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        rx: u16,
        ry: u16,
        add: bool,
    ) {
        self.adjust_retained_wall_neighbor_source_target(
            resolved_terrain,
            NativeRuntimeOverlayCell::Real(rx, ry),
            add,
        );
    }

    fn adjust_retained_wall_neighbor_source_target(
        &mut self,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source: NativeRuntimeOverlayCell,
        add: bool,
    ) {
        self.bump_mutation_epoch();
        self.wall_plane_epoch = self.wall_plane_epoch.wrapping_add(1);
        // Native wall lifecycle evidence: OverlayClass::Mark increments at
        // 0x005FC762..0x005FC775; DestroyOverlay decrements at
        // 0x00481070..0x00481082; cleanup auto-removal's conditional decrement
        // uses the zone comparison returned by recalculate_runtime_cell below.
        let (width, height) = (self.width, self.height);
        let counts = &mut self.retained_neighbor_counts;
        let terrain = resolved_terrain
            .expect("retained wall-neighbor authority requires resolved CellClass lookup state");
        assert_eq!(
            (width, height),
            (terrain.width(), terrain.height()),
            "retained wall-neighbor authority must match resolved terrain"
        );
        assert_eq!(
            counts.len(),
            usize::from(width) * usize::from(height),
            "retained wall-neighbor authority must match overlay shape"
        );

        for (dx, dy) in DIRECTION_DELTAS {
            // Adjacent_Cell rereads the receiver's packed coordinate for each
            // probe. This matters when the receiver itself is the shared
            // dummy: a miss restamps +0x24 before the next direction.
            let Some((base_x, base_y)) = native_runtime_overlay_target_coord(Some(terrain), source)
            else {
                continue;
            };
            let Some(NativeRuntimeOverlayCell::Real(nx, ny)) = native_runtime_overlay_cell_lookup(
                width,
                height,
                Some(terrain),
                base_x + dx,
                base_y + dy,
            ) else {
                // The lookup already stamped the shared fallback identity.
                // Its counter lives with that identity, not in the real plane.
                terrain.shared_cell_dummy().adjust_neighbor_count(add);
                continue;
            };
            let Some(index) = index_of(width, height, nx, ny) else {
                continue;
            };
            counts[index] = if add {
                counts[index].wrapping_add(1)
            } else {
                counts[index].wrapping_sub(1)
            };
        }
    }

    /// One wrapping increment of `CellClass+0x122` on a cell
    /// `OverlayClass::Mark`'s wall tail resolved (`0x005FC762..0x005FC773`).
    /// The shared dummy keeps its own count.
    pub(crate) fn increment_retained_wall_neighbor_count(
        &mut self,
        resolved_terrain: &ResolvedTerrainGrid,
        cell: crate::map::cell_index::NativeCellIdentity,
    ) {
        self.bump_mutation_epoch();
        let crate::map::cell_index::NativeCellIdentity::Real(_) = cell else {
            resolved_terrain
                .shared_cell_dummy()
                .adjust_neighbor_count(true);
            return;
        };
        let (x, y) = resolved_terrain.native_cell_coord(cell);
        let Some(index) = index_of(self.width, self.height, x as u16, y as u16) else {
            return;
        };
        self.wall_plane_epoch = self.wall_plane_epoch.wrapping_add(1);
        self.retained_neighbor_counts[index] = self.retained_neighbor_counts[index].wrapping_add(1);
    }

    pub(crate) fn remove_retained_wall_neighbor_source(
        &mut self,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        rx: u16,
        ry: u16,
    ) {
        self.bump_mutation_epoch();
        self.adjust_retained_wall_neighbor_source(resolved_terrain, rx, ry, false);
    }

    fn remove_retained_wall_neighbor_source_target(
        &mut self,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source: NativeRuntimeOverlayCell,
    ) {
        self.adjust_retained_wall_neighbor_source_target(resolved_terrain, source, false);
    }

    /// Remove overlay from cell entirely. Returns previous overlay_id if any.
    pub fn clear_overlay(&mut self, rx: u16, ry: u16) -> Option<u8> {
        self.bump_mutation_epoch();
        let idx = index_of(self.width, self.height, rx, ry)?;
        let prev = self.cells[idx].overlay_id;
        self.cells[idx] = OverlayCell::default();
        self.dirty_cells.push((rx, ry));
        prev
    }

    /// Place overlay at cell.
    pub fn place_overlay(&mut self, rx: u16, ry: u16, overlay_id: u8, data: u8) {
        self.bump_mutation_epoch();
        if let Some(idx) = index_of(self.width, self.height, rx, ry) {
            self.cells[idx] = OverlayCell {
                overlay_id: Some(overlay_id),
                overlay_data: data,
                wall_owner: None,
                rubble_building_type: None,
            };
            self.dirty_cells.push((rx, ry));
        }
    }

    /// `OverlayClass::Mark @ 0x005FC570`'s literal `CellClass+0x44` store,
    /// without the `RecalcAttributes` tail. Mark interleaves its data stores,
    /// wall cleanup, germination and CellAnim before its own Recalc. A bridge
    /// identity also projects the terrain's bridge layer and deck, so a caller
    /// that can place one passes the terrain; ore placement passes none.
    /// False outside the grid or for an unregistered type.
    pub(crate) fn write_mark_identity(
        &mut self,
        resolved_terrain: Option<&mut ResolvedTerrainGrid>,
        registry: &OverlayTypeRegistry,
        rx: u16,
        ry: u16,
        overlay_id: u8,
    ) -> bool {
        let wrote = self
            .write_identity_fields(resolved_terrain, registry, (rx, ry), overlay_id, None)
            .is_some();
        if wrote {
            self.bump_mutation_epoch();
        }
        wrote
    }

    /// The literal `CellClass+0x11E` store, with or without an identity and
    /// without a dirty entry or Recalc: `OverlayClass::Mark`'s data stores and
    /// the stores that follow an Overlay constructor
    /// (`CellClass::PlaceTiberium @ 0x004872A0`, `AnimClass::AI @
    /// 0x00424155`), and map-load germination's densities. False outside the
    /// grid.
    pub(crate) fn write_cell_data(&mut self, rx: u16, ry: u16, overlay_data: u8) -> bool {
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        self.bump_mutation_epoch();
        self.cells[idx].overlay_data = overlay_data;
        true
    }

    /// `BuildingClass::AfterDestruction @ 0x00441F60`'s foundation-cell
    /// write: `CellClass+0x44 = 0xEF` (the rubble overlay index) and
    /// `+0x40 = NULL` (`0x0044202C..0x00442033`). The data byte (`+0x11E`)
    /// and wall owner keep their values; the caller runs the RecalcAttributes
    /// tail ([`Self::recalculate_runtime_cell`]). False when the cell is
    /// outside the grid or the table has no index 0xEF.
    pub(crate) fn write_rubble_fields(
        &mut self,
        resolved_terrain: &mut ResolvedTerrainGrid,
        registry: &OverlayTypeRegistry,
        rx: u16,
        ry: u16,
    ) -> bool {
        let Some(idx) = self.write_identity_fields(
            Some(resolved_terrain),
            registry,
            (rx, ry),
            RUBBLE_OVERLAY_INDEX,
            None,
        ) else {
            return false;
        };
        self.cells[idx].rubble_building_type = None;
        true
    }

    /// The literal `CellClass+0x44` store, with `+0x11E` when `overlay_data`
    /// is given; a bridge identity projects the terrain's bridge layer when
    /// the terrain is given. No Recalc tail. Returns the cell index, or `None` outside the grid or
    /// for an unregistered type.
    fn write_identity_fields(
        &mut self,
        resolved_terrain: Option<&mut ResolvedTerrainGrid>,
        registry: &OverlayTypeRegistry,
        (rx, ry): (u16, u16),
        overlay_id: u8,
        overlay_data: Option<u8>,
    ) -> Option<usize> {
        let idx = index_of(self.width, self.height, rx, ry)?;
        registry.flags(overlay_id)?;
        let cell = &mut self.cells[idx];
        cell.overlay_id = Some(overlay_id);
        if let Some(overlay_data) = overlay_data {
            cell.overlay_data = overlay_data;
        }
        self.dirty_cells.push((rx, ry));
        if let Some(terrain) = resolved_terrain {
            terrain.set_runtime_overlay_bridge_identity(rx, ry, overlay_id);
        }
        Some(idx)
    }

    /// AfterDestruction's origin-cell write, `CellClass+0x40 = Type`
    /// (`0x00442082`), after every foundation cell took the rubble overlay.
    pub(crate) fn set_rubble_building_type(
        &mut self,
        rx: u16,
        ry: u16,
        building_type: InternedId,
    ) -> bool {
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        self.cells[idx].rubble_building_type = Some(building_type);
        self.dirty_cells.push((rx, ry));
        true
    }

    /// Erase the two literal CellClass overlay fields a crate removal clears.
    ///
    /// `CrateSlot__RemoveCrateOverlayFromCell @ 0x004A1AA0` writes
    /// `CellClass+0x44 = -1` (`0x004A1BE1`) and `CellClass+0x11E = 0`
    /// (`0x004A1BE8`) after its screen-dirty request and changes no other
    /// field, so this deliberately keeps `wall_owner`, the terrain's bridge
    /// layer and every Recalc-derived attribute — unlike
    /// [`OverlayGrid::clear_overlay`], which resets the whole cell. The random
    /// placer admits only cells without an overlay, so in stock rules a
    /// removal never undoes a bridge identity.
    pub(crate) fn clear_crate_mark_fields(&mut self, rx: u16, ry: u16) -> bool {
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        self.cells[idx].overlay_id = None;
        self.cells[idx].overlay_data = 0;
        self.removed_render_cells.push((rx, ry));
        true
    }

    /// Drain the coordinates whose overlay identity was erased without a
    /// recalc. The frame finalizer forwards these to presentation so the
    /// removed sprite stops drawing.
    pub(crate) fn take_removed_render_cells(&mut self) -> Vec<(u16, u16)> {
        std::mem::take(&mut self.removed_render_cells)
    }

    /// Write only the raw `CellClass::OverlayData` byte and emit the setter's
    /// immediate radar-dirty event. High-bridge setters do this even when the
    /// target cell has no overlay identity yet.
    pub(crate) fn write_crate_mark_data_field(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_data: u8,
    ) -> bool {
        if !self.write_literal_bridge_state(rx, ry, overlay_data) {
            return false;
        }
        self.dirty_cells.push((rx, ry));
        true
    }

    /// Literal47E040 +11E store, including cells without an overlay. Its
    /// caller owns the later fallout/radar sequence; no Recalc receipt here.
    pub(crate) fn write_literal_bridge_state(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_data: u8,
    ) -> bool {
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        self.cells[idx].overlay_data = overlay_data;
        true
    }

    /// Literal576BA0 +44=-1 after the separately published +11E store. Keep
    /// the state, wall owner and retained attributes; only presentation drops
    /// the removed identity. This must not enqueue a frame-tail Recalc.
    pub(crate) fn clear_literal_bridge_identity(&mut self, rx: u16, ry: u16) -> bool {
        self.bump_mutation_epoch();
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        self.cells[idx].overlay_id = None;
        self.removed_render_cells.push((rx, ry));
        true
    }

    /// Building458A80's clear-site stores458B3F..458B48: erase a present
    /// overlay's identity/data/rubble reference, retaining the wall owner and
    /// all Recalc-derived cell attributes. There is no neighbor or Recalc
    /// callback here; subsequent Limbo/Unlimbo supplies its own cell callbacks.
    pub(crate) fn clear_building_placement_fields(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        rx: u16,
        ry: u16,
    ) -> bool {
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        if self.cells[idx].overlay_id.is_none() {
            return false;
        }
        self.bump_mutation_epoch();
        self.cells[idx].overlay_id = None;
        self.cells[idx].overlay_data = 0;
        self.cells[idx].rubble_building_type = None;
        self.removed_render_cells.push((rx, ry));
        // A coordinate outside the native allocation resolves to the shared
        // dummy, which clears its own identity and state.
        let cell = terrain.native_cell_identity((rx as i16, ry as i16));
        if cell == crate::map::cell_index::NativeCellIdentity::Dummy {
            terrain.shared_cell_dummy().set_overlay_fields(None, 0);
        }
        true
    }

    /// Pending overlay/radar dirty coordinates without consuming the runtime
    /// receipt. Initial presentation uses this to include every real cell
    /// written by a multi-cell startup Mark transaction.
    pub fn pending_dirty_cells(&self) -> &[(u16, u16)] {
        &self.dirty_cells
    }

    /// Cell47D210, called by InitCellAttributes568E16 after Buildings load.
    /// Building5F6360 receives the Cell as its target (47D278..27B): its
    /// RTTI487E60 is11, so no Building-target foundation discount applies.
    /// Strict signed improvement from INT_MAX preserves the first tie.
    /// Source: original47D210..47D2A6; shared numeric controls are consumed by
    /// team_script_vm::recruit_oracle_tests::object_distance_matches_the_original.
    pub fn reconstruct_map_wall_owners(
        &mut self,
        resolved_terrain: &ResolvedTerrainGrid,
        registry: &OverlayTypeRegistry,
        buildings: &[MapWallOwnerCandidate],
    ) {
        for ry in 0..self.height {
            for rx in 0..self.width {
                let Some(idx) = index_of(self.width, self.height, rx, ry) else {
                    continue;
                };
                let is_wall = self.cells[idx]
                    .overlay_id
                    .and_then(|id| registry.flags(id))
                    .is_some_and(|flags| flags.wall);
                if !is_wall {
                    continue;
                }

                self.cells[idx].wall_owner = None;
                let Some(cell) = resolved_terrain.cell(rx, ry) else {
                    continue;
                };
                let Some(wall) = crate::sim::cell_rect::CellRef::Real(cell).center_coordinate()
                else {
                    continue;
                };
                let mut best_distance = i32::MAX;
                let mut best_owner = None;
                for building in buildings {
                    if !building.object_alive || !building.cell_marked || !building.house_wall_owner
                    {
                        continue;
                    }
                    let distance = object_distance(
                        [building.world_x, building.world_y, building.world_z],
                        [wall.x, wall.y, wall.z],
                        None,
                    );
                    if distance < best_distance {
                        best_distance = distance;
                        best_owner = Some(building.owner);
                    }
                }
                self.cells[idx].wall_owner = best_owner;
            }
        }
    }

    /// Stamp a player-owned wall overlay at a cell.
    #[cfg(any(test, feature = "test-support"))]
    pub fn place_owned_wall(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_id: u8,
        data: u8,
        owner: InternedId,
    ) {
        self.bump_mutation_epoch();
        self.stamp_wall_identity(rx, ry, overlay_id, data);
        self.set_wall_owner(rx, ry, owner);
    }

    /// Initialize a test wall fixture before assigning its owner. Runtime
    /// OverlayClass::Mark uses `write_mark_identity` to retain cell state.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn stamp_wall_identity(&mut self, rx: u16, ry: u16, overlay_id: u8, data: u8) {
        self.bump_mutation_epoch();
        if let Some(idx) = index_of(self.width, self.height, rx, ry) {
            self.cells[idx] = OverlayCell {
                overlay_id: Some(overlay_id),
                overlay_data: data,
                wall_owner: None,
                rubble_building_type: None,
            };
            self.dirty_cells.push((rx, ry));
        }
    }

    pub(crate) fn set_wall_owner(&mut self, rx: u16, ry: u16, owner: InternedId) {
        self.bump_mutation_epoch();
        if let Some(idx) = index_of(self.width, self.height, rx, ry) {
            self.cells[idx].wall_owner = Some(owner);
        }
    }

    /// Clear type/data while retaining the wall owner.
    fn clear_overlay_preserving_owner(&mut self, rx: u16, ry: u16) -> Option<u8> {
        self.bump_mutation_epoch();
        let idx = index_of(self.width, self.height, rx, ry)?;
        let prev = self.cells[idx].overlay_id;
        self.cells[idx].overlay_id = None;
        self.cells[idx].overlay_data = 0;
        self.dirty_cells.push((rx, ry));
        prev
    }

    /// Update overlay_data in place (density change, damage increment).
    /// No-op if out-of-bounds or cell has no overlay.
    pub fn set_overlay_data(&mut self, rx: u16, ry: u16, data: u8) {
        self.bump_mutation_epoch();
        if let Some(idx) = index_of(self.width, self.height, rx, ry)
            && self.cells[idx].overlay_id.is_some()
        {
            self.cells[idx].overlay_data = data;
            self.dirty_cells.push((rx, ry));
        }
    }

    /// Iterate all cells that have an overlay (for hashing).
    pub fn iter_occupied(&self) -> impl Iterator<Item = (u16, u16, &OverlayCell)> {
        self.cells
            .iter()
            .enumerate()
            .filter_map(move |(idx, cell)| {
                if cell.overlay_id.is_some() {
                    let rx = (idx % self.width as usize) as u16;
                    let ry = (idx / self.width as usize) as u16;
                    Some((rx, ry, cell))
                } else {
                    None
                }
            })
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Serialized cell storage must remain exactly width × height. Snapshot
    /// restoration validates this before any coordinate-indexed sweep.
    pub(crate) fn cell_storage_len(&self) -> usize {
        self.cells.len()
    }

    /// Recalculate one runtime mutation and queue its navigation receipt.
    /// A later unchanged projection cannot erase an earlier change: the
    /// receipt keeps first-seen order independently of the presentation
    /// dirty list until `Simulation` publishes it.
    ///
    /// Zone comparison serves ordered wall cleanup: CellClass cleanup
    /// @ 0x00480630 runs Recalc @ 0x00480969, compares old/new zone at
    /// 0x0048096E..0x00480977, then repairs the graph before its neighbor-count
    /// tail. The caller still owns those callbacks in their native slots.
    pub fn recalculate_runtime_cell(
        &mut self,
        terrain: &mut ResolvedTerrainGrid,
        registry: &OverlayTypeRegistry,
        cell: (u16, u16),
    ) -> OverlayRecalcOutcome {
        self.bump_mutation_epoch();
        let old_zone = terrain.cell(cell.0, cell.1).map(|cell| cell.zone_type);
        let navigation_changed =
            recalc_overlay_passability(self, terrain, registry, cell.0, cell.1);
        let zone_changed = old_zone
            .zip(terrain.cell(cell.0, cell.1).map(|cell| cell.zone_type))
            .is_some_and(|(old, new)| old != new);
        if navigation_changed && !self.synchronous_navigation_cells.contains(&cell) {
            self.synchronous_navigation_cells.push(cell);
        }
        OverlayRecalcOutcome {
            navigation_changed,
            zone_changed,
        }
    }

    /// Drain movement-authority changes after all inline overlay callbacks that
    /// precede the next path/zone reader have completed.
    pub(crate) fn take_synchronous_navigation_cells(&mut self) -> Vec<(u16, u16)> {
        std::mem::take(&mut self.synchronous_navigation_cells)
    }

    /// Drain the cells mutated since the last call. Simulation's frame
    /// finalizer recalculates their attributes and publishes navigation.
    ///
    /// Drained at the first frame boundary with rules, resolved terrain, and an
    /// overlay registry. Partial-input frames retain it so derived terrain and
    /// navigation cannot miss the mutation. Runtime-only; never serialized.
    pub fn take_dirty_cells(&mut self) -> Vec<(u16, u16)> {
        std::mem::take(&mut self.dirty_cells)
    }
}

/// Recompute overlay-driven fields on ResolvedTerrainCell after an overlay mutation.
///
/// Reads overlay_id from the grid, checks registry flags for reduced ZoneType,
/// computes new overlay_blocks / zone_type / land_type / speed_costs values, writes
/// them to resolved_terrain. Returns true if any passability- or zone-relevant value
/// changed (caller should trigger zone rebuild).
///
/// Mirrors gamemd.exe RecalcAttributes stage 3a, scoped to overlay->passability +
/// overlay->LandType (+0xEC).
///
/// Receipt-free projection for map admission, snapshot reconstruction, frame
/// finalizer replay and bridge-owned immediate publication. Ordinary runtime
/// overlay mutations use `OverlayGrid::recalculate_runtime_cell` to keep projection
/// and downstream notification together.
pub fn recalc_overlay_passability(
    overlay_grid: &mut OverlayGrid,
    resolved_terrain: &mut ResolvedTerrainGrid,
    registry: &OverlayTypeRegistry,
    rx: u16,
    ry: u16,
) -> bool {
    let Some(slope_type) = resolved_terrain.cell(rx, ry).map(|cell| cell.slope_type) else {
        return false;
    };
    let mut overlay_id = overlay_grid.cell(rx, ry).overlay_id;
    let source_flags = overlay_id.and_then(|id| registry.flags(id));
    let mut cleared_resource_for_slope = false;
    if source_flags.is_some_and(|flags| clears_tiberium_on_slope(flags, slope_type)) {
        overlay_grid.clear_cell_silently(rx, ry);
        overlay_id = None;
        cleared_resource_for_slope = true;
    }
    let flags = overlay_id.and_then(|id| registry.flags(id));
    let land_flags = if cleared_resource_for_slope {
        source_flags
    } else {
        flags
    };
    // Do not short circuit the projection when literal storage was cleared.
    let projection_changed = resolved_terrain.apply_overlay_attributes(rx, ry, flags, land_flags);
    cleared_resource_for_slope || projection_changed
}

/// Ordered writes produced by one `DestroyOverlay` call, including its inline
/// recursive cardinal chain and fixed post-destruction cleanup fan-out.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallMutationKind {
    DirectUpdated,
    DirectRemoved,
    CleanupUpdated,
    CleanupRemoved,
}

#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallMutation {
    pub rx: u16,
    pub ry: u16,
    pub kind: WallMutationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WallDirtyStep {
    Tactical,
    Radar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WallPointerTarget {
    Real(u16, u16),
    SharedDummy,
}

/// World-owned synchronous effects inside one native DestroyOverlay call.
/// Implementations publish dirty state, update live navigation authority, and
/// broadcast the distinct cell-pointer expiry callback. None can be
/// reconstructed from an after-return mutation list without losing native
/// recursive and cleanup-visit order.
pub(crate) trait WallDamageTransactionHost {
    fn dirty_step(&mut self, step: WallDirtyStep, packed_coord: (u16, u16));

    fn navigation_step(
        &mut self,
        terrain: &ResolvedTerrainGrid,
        overlays: &OverlayGrid,
        cell: (u16, u16),
        navigation_changed: bool,
        repair: ZoneRepairKind,
    );

    fn pointer_expired(&mut self, target: WallPointerTarget);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeRuntimeOverlayCell {
    Real(u16, u16),
    Dummy,
}

impl From<WallPointerTarget> for NativeRuntimeOverlayCell {
    fn from(target: WallPointerTarget) -> Self {
        match target {
            WallPointerTarget::Real(rx, ry) => Self::Real(rx, ry),
            WallPointerTarget::SharedDummy => Self::Dummy,
        }
    }
}

impl From<NativeRuntimeOverlayCell> for WallPointerTarget {
    fn from(target: NativeRuntimeOverlayCell) -> Self {
        match target {
            NativeRuntimeOverlayCell::Real(rx, ry) => Self::Real(rx, ry),
            NativeRuntimeOverlayCell::Dummy => Self::SharedDummy,
        }
    }
}

/// Resolve one runtime CellClass lookup, retaining the shared-dummy result.
///
/// Native `Adjacent_Cell @ 0x00481810` narrows each coordinate to a signed
/// word, then `MapClass::Get_CellClass @ 0x005657A0` selects `y*512+x` without
/// rejecting either axis independently. Consequently a request such as west
/// of `(0,1)` aliases real cell `(511,0)`. A true miss stamps CellClass+0x24 on
/// the shared dummy and returns that same live object. `None` terrain retains
/// rectangular behavior only for legacy focused helpers with no CellClass
/// authority.
fn native_runtime_overlay_cell_lookup(
    width: u16,
    height: u16,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    x: i32,
    y: i32,
) -> Option<NativeRuntimeOverlayCell> {
    let Some(terrain) = resolved_terrain else {
        return (x >= 0 && y >= 0 && x < i32::from(width) && y < i32::from(height))
            .then_some(NativeRuntimeOverlayCell::Real(x as u16, y as u16));
    };
    assert_eq!(
        (width, height),
        (terrain.width(), terrain.height()),
        "runtime overlay lookup must match resolved terrain"
    );
    let narrowed_x = i32::from(x as i16);
    let narrowed_y = i32::from(y as i16);
    if let Some(index) = terrain.native_fixed_cell_index(x as i16, y as i16)
        && let Some(cell) = terrain.cells().get(index)
        && cell.rx < width
        && cell.ry < height
    {
        return Some(NativeRuntimeOverlayCell::Real(cell.rx, cell.ry));
    }
    terrain.stamp_dummy_cell_requested_coord(narrowed_x, narrowed_y);
    Some(NativeRuntimeOverlayCell::Dummy)
}

/// Resolve one inner N/E/S/W/self cleanup-table entry from a retained outer
/// receiver pointer. A real receiver keeps its own coordinate throughout. A
/// dummy receiver rereads shared CellClass+0x24 before every directional entry,
/// so successive misses can walk the dummy and a later request can alias a real
/// fixed-grid cell. The self entry reuses the receiver pointer and therefore has
/// no represented real coordinate when that receiver is the dummy.
fn native_runtime_overlay_target_coord(
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    target: NativeRuntimeOverlayCell,
) -> Option<(i32, i32)> {
    match target {
        NativeRuntimeOverlayCell::Real(rx, ry) => Some((i32::from(rx), i32::from(ry))),
        NativeRuntimeOverlayCell::Dummy => Some(resolved_terrain?.dummy_cell_requested_coord()),
    }
}

fn native_runtime_overlay_target_packed_coord(
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    target: NativeRuntimeOverlayCell,
) -> Option<(u16, u16)> {
    let (x, y) = native_runtime_overlay_target_coord(resolved_terrain, target)?;
    Some((x as i16 as u16, y as i16 as u16))
}

fn native_cleanup_visit_target(
    width: u16,
    height: u16,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    receiver: NativeRuntimeOverlayCell,
    dx: i32,
    dy: i32,
) -> Option<NativeRuntimeOverlayCell> {
    if (dx, dy) == (0, 0) {
        return Some(receiver);
    }
    let (base_x, base_y) = native_runtime_overlay_target_coord(resolved_terrain, receiver)?;
    native_runtime_overlay_cell_lookup(width, height, resolved_terrain, base_x + dx, base_y + dy)
}

/// Result of a wall damage attempt.
#[derive(Debug, Clone, Default)]
pub struct WallDamageResult {
    /// Cells where overlay_data changed (need re-render).
    pub changed_cells: Vec<(u16, u16)>,
    /// Cells where wall was fully destroyed (need zone rebuild + render removal).
    pub destroyed_cells: Vec<(u16, u16)>,
    /// Exact mutation order. This is deliberately a wall-local trace, not a
    /// generalized gameplay event system.
    #[cfg(any(test, feature = "test-support"))]
    pub mutations: Vec<WallMutation>,
    /// Diagnostic trace of packed CellStruct values submitted to
    /// MarkTerrainDirty, deduplicated in first-call order. Production mutation
    /// authority is the synchronous host callback, never replay of this list.
    /// True-dummy coordinates retain their raw signed-word bit patterns.
    #[cfg(any(test, feature = "test-support"))]
    pub radar_dirty_cells: Vec<(u16, u16)>,
}

#[cfg(any(test, feature = "test-support"))]
fn push_wall_radar_dirty(result: &mut WallDamageResult, coord: (u16, u16)) {
    if !result.radar_dirty_cells.contains(&coord) {
        result.radar_dirty_cells.push(coord);
    }
}

fn publish_wall_dirty_step(
    host: &mut Option<&mut dyn WallDamageTransactionHost>,
    _result: &mut WallDamageResult,
    step: WallDirtyStep,
    packed_coord: (u16, u16),
) {
    if let Some(host) = host.as_deref_mut() {
        host.dirty_step(step, packed_coord);
    }
    #[cfg(any(test, feature = "test-support"))]
    if step == WallDirtyStep::Radar {
        push_wall_radar_dirty(_result, packed_coord);
    }
}

fn publish_wall_dirty_step_without_result(
    host: &mut Option<&mut dyn WallDamageTransactionHost>,
    step: WallDirtyStep,
    packed_coord: (u16, u16),
) {
    if let Some(host) = host.as_deref_mut() {
        host.dirty_step(step, packed_coord);
    }
}

/// Damage a wall overlay, matching gamemd.exe CellClass::DestroyOverlay (0x00480CB0).
///
/// 1. Random damage check against Strength
/// 2. Increment damage level (upper nibble of overlay_data)
/// 3. At penultimate damage level: chain-damage cardinal neighbors
/// 4. At full destruction: clear overlay, add to destroyed list
///
/// `damage == -1` bypasses the random check (forced destruction).
#[cfg(test)]
pub fn damage_wall_overlay(
    overlay_grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    rx: u16,
    ry: u16,
    damage: i32,
    rng: &mut crate::sim::rng::SimRng,
) -> WallDamageResult {
    // Production always damages walls with its resolved CellClass grid; a
    // flat clear one of the grid's size stands in.
    let mut terrain = tests::clear_terrain_grid(overlay_grid.width(), overlay_grid.height());
    damage_wall_overlay_with_terrain(
        overlay_grid,
        registry,
        Some(&mut terrain),
        rx,
        ry,
        damage,
        rng,
    )
}

/// Runtime-authoritative wall damage. Finalized grids require the resolved
/// CellClass lookup state so each terminal removal reverses exactly one
/// wrapping eight-neighbor wall contribution, including fixed-stride aliases.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn damage_wall_overlay_with_terrain(
    overlay_grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
    damage: i32,
    rng: &mut crate::sim::rng::SimRng,
) -> WallDamageResult {
    damage_wall_overlay_with_runtime_host(
        overlay_grid,
        registry,
        resolved_terrain,
        WallPointerTarget::Real(rx, ry),
        damage,
        rng,
        None,
    )
}

/// Damage the already retained Cell receiver, including its shared-Dummy
/// identity. The caller has performed Map5657A0; do not look it up again.
pub(crate) fn damage_wall_overlay_with_runtime_host(
    overlay_grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    mut resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    target: WallPointerTarget,
    damage: i32,
    rng: &mut crate::sim::rng::SimRng,
    mut host: Option<&mut dyn WallDamageTransactionHost>,
) -> WallDamageResult {
    assert!(
        resolved_terrain.is_some(),
        "retained wall-neighbor authority requires resolved terrain for wall damage"
    );
    let mut result = WallDamageResult::default();
    damage_wall_recursive(
        overlay_grid,
        registry,
        &mut resolved_terrain,
        &mut host,
        target.into(),
        damage,
        rng,
        &mut result,
    );
    result
}

fn damage_wall_recursive(
    grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    resolved_terrain: &mut Option<&mut ResolvedTerrainGrid>,
    host: &mut Option<&mut dyn WallDamageTransactionHost>,
    target: NativeRuntimeOverlayCell,
    damage: i32,
    rng: &mut crate::sim::rng::SimRng,
    result: &mut WallDamageResult,
) {
    let (overlay_identity, overlay_data) =
        native_runtime_overlay_target_state(grid, resolved_terrain.as_deref(), target);
    let Some(overlay_id) = overlay_identity else {
        return;
    };
    let Some(flags) = registry.flags(overlay_id) else {
        return;
    };
    if !flags.wall {
        return;
    }

    // Random damage check (gamemd): when damage < Strength, draw
    // RandomRanged(0, Strength) — INCLUSIVE on the high end, range [0, Strength]
    // — and apply damage only when roll < damage; otherwise no effect. The
    // engine uses `< damage` (so roll == damage is a no-op) and the inclusive
    // top, both of which differ from an exclusive `[0, Strength-1]` / `>` test.
    if damage != -1 && damage < i32::from(flags.strength) {
        let roll = rng.next_range_u32_inclusive(0, u32::from(flags.strength)) as i32;
        if roll >= damage {
            return;
        }
    }

    // CellClass::DestroyOverlay dirties the tactical footprint for every
    // accepted wall hit before writing the upper damage nibble. A rejected RNG
    // attempt emits nothing; a retained partial hit emits no radar step.
    if let Some(coord) =
        native_runtime_overlay_target_packed_coord(resolved_terrain.as_deref(), target)
    {
        publish_wall_dirty_step(host, result, WallDirtyStep::Tactical, coord);
    }

    // Increment damage level (upper nibble).
    let new_data = overlay_data.wrapping_add(0x10);
    // Native writes CellClass+0x11E before entering the recursive cardinal
    // chain. Besides making the increment visible to each nested call, this
    // prevents a stateful shared-dummy receiver from recursively selecting
    // itself as a pristine wall.
    write_native_runtime_overlay_target_state_untracked(
        grid,
        resolved_terrain.as_deref(),
        target,
        new_data,
    );
    let damage_level = new_data >> 4;

    // At penultimate damage level: chain-damage cardinal neighbors.
    if flags.damage_levels > 2 && u16::from(damage_level) == flags.damage_levels.saturating_sub(1) {
        const CARDINAL: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
        for (dx, dy) in CARDINAL {
            let Some((base_x, base_y)) =
                native_runtime_overlay_target_coord(resolved_terrain.as_deref(), target)
            else {
                continue;
            };
            let Some(neighbor_target) = native_runtime_overlay_cell_lookup(
                grid.width(),
                grid.height(),
                resolved_terrain.as_deref(),
                base_x + dx,
                base_y + dy,
            ) else {
                continue;
            };
            let (neighbor_id, neighbor_data) = native_runtime_overlay_target_state(
                grid,
                resolved_terrain.as_deref(),
                neighbor_target,
            );
            if neighbor_id == Some(overlay_id) && (neighbor_data >> 4) == 0 {
                damage_wall_recursive(
                    grid,
                    registry,
                    resolved_terrain,
                    host,
                    neighbor_target,
                    200,
                    rng,
                    result,
                );
            }
        }
    }

    // Check if fully destroyed.
    let post_chain_data =
        native_runtime_overlay_target_state(grid, resolved_terrain.as_deref(), target).1;
    let post_chain_level = post_chain_data >> 4;
    let penultimate = flags.damage_levels.saturating_sub(1);
    let connected = post_chain_data & 0x0F != 0;
    let retain = u16::from(post_chain_level) < penultimate
        || (u16::from(post_chain_level) == penultimate && connected);
    if damage != -1 && retain {
        // The native state write preceded the chain. Export a real-cell render
        // mutation only; the shared dummy's pair stays live through its handle.
        if let NativeRuntimeOverlayCell::Real(rx, ry) = target {
            grid.dirty_cells.push((rx, ry));
            result.changed_cells.push((rx, ry));
            #[cfg(any(test, feature = "test-support"))]
            result.mutations.push(WallMutation {
                rx,
                ry,
                kind: WallMutationKind::DirectUpdated,
            });
        }
        return;
    }

    // Full destruction.
    clear_native_runtime_overlay_target(grid, resolved_terrain.as_deref(), target, false);
    if let NativeRuntimeOverlayCell::Real(rx, ry) = target {
        result.destroyed_cells.push((rx, ry));
        #[cfg(any(test, feature = "test-support"))]
        result.mutations.push(WallMutation {
            rx,
            ry,
            kind: WallMutationKind::DirectRemoved,
        });
    }

    // Native clears the direct cell and runs its RecalcAttributes before any
    // cardinal PostDestructionWallCleanup call. Keep the modeled terrain and
    // navigation publication signal inside that same recursive transaction.
    if let NativeRuntimeOverlayCell::Real(rx, ry) = target
        && let Some(terrain) = resolved_terrain.as_deref_mut()
    {
        let recalc = grid.recalculate_runtime_cell(terrain, registry, (rx, ry));
        if let Some(host) = host.as_deref_mut() {
            host.navigation_step(
                terrain,
                grid,
                (rx, ry),
                recalc.navigation_changed,
                ZoneRepairKind::AssignOrphaned,
            );
        }
    }

    // Native publishes the direct target's radar terrain dirty only after its
    // Recalc + AssignOrphaned/graph tail, before cardinal cleanup begins. A
    // shared dummy is read here, after any recursive chain may have restamped
    // its packed coordinate.
    if let Some(coord) =
        native_runtime_overlay_target_packed_coord(resolved_terrain.as_deref(), target)
    {
        publish_wall_dirty_step(host, result, WallDirtyStep::Radar, coord);
    }

    // Native cleanup is part of this direct-destruction call. A recursive
    // cardinal therefore completes its own direct clear + fixed cleanup before
    // the parent advances to the next cardinal.
    cleanup_wall_neighbors_into(grid, registry, resolved_terrain, host, target, result);
    if let Some(host) = host.as_deref_mut() {
        host.pointer_expired(target.into());
    }
    // CellClass::DestroyOverlay @ 0x00480CB0 reverses this wall's
    // CellClass+0x122 contribution at 0x00481070..0x00481082 only after its
    // complete cardinal cleanup fan-out.
    grid.remove_retained_wall_neighbor_source_target(resolved_terrain.as_deref(), target);
}

/// Outcome of `recompute_wall_connectivity_at`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecomputeResult {
    /// Cell was not a wall, or no nibble change.
    NoChange,
    /// Connectivity nibble changed; cell remains.
    Updated,
    /// Auto-destruct threshold tripped; cell cleared.
    Destroyed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeWallCleanupVisit {
    pub packed_coord: (u16, u16),
    pub real_cell: Option<(u16, u16)>,
    pub was_wall: bool,
    pub recomputed: RecomputeResult,
}

/// Per-overlay-type byte-value thresholds at which neighbor cleanup destroys an
/// already-damaged isolated wall.
///
/// PostDestructionWallCleanup @ 0x00480630 also hardcodes CYCL/BARB/FENC rows,
/// but retail never sets those types Wall=yes; the outer wall gate keeps those
/// TS/mod-only rows dormant. Only the three active-retail rows belong here.
fn auto_destruct_threshold(overlay_id: u8, full_byte: u8) -> bool {
    match overlay_id {
        0x00 => matches!(full_byte, 0x10 | 0x20), // GASAND
        0x02 => matches!(full_byte, 0x20 | 0x30), // GAWALL
        0x1A => matches!(full_byte, 0x20 | 0x30), // NAWALL
        _ => false,
    }
}

/// Refresh one cell's connectivity nibble against its 4 cardinal neighbors,
/// then apply the per-type auto-destruct safety net.
///
/// Same-type-only matching.
#[cfg(any(test, feature = "test-support"))]
pub fn recompute_wall_connectivity_at(
    grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    rx: u16,
    ry: u16,
) -> RecomputeResult {
    recompute_wall_connectivity_at_with_terrain(grid, registry, None, rx, ry)
}

/// Runtime form of [`recompute_wall_connectivity_at`] using native fixed-grid
/// cardinal lookup. The coordinate being recomputed is already a real cell;
/// only its neighbor probes pass through Get_CellClass semantics.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn recompute_wall_connectivity_at_with_terrain(
    grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
) -> RecomputeResult {
    recompute_wall_connectivity_target(
        grid,
        registry,
        resolved_terrain,
        NativeRuntimeOverlayCell::Real(rx, ry),
    )
}

fn native_runtime_overlay_target_state(
    grid: &OverlayGrid,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    target: NativeRuntimeOverlayCell,
) -> (Option<u8>, u8) {
    match target {
        NativeRuntimeOverlayCell::Real(rx, ry) => {
            let cell = grid.cell(rx, ry);
            (cell.overlay_id, cell.overlay_data)
        }
        NativeRuntimeOverlayCell::Dummy => {
            let Some(terrain) = resolved_terrain else {
                return (None, 0);
            };
            let (identity, state) = terrain.shared_cell_dummy().overlay_identity_state();
            (u8::try_from(identity).ok(), state)
        }
    }
}

fn write_native_runtime_overlay_target_state(
    grid: &mut OverlayGrid,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    target: NativeRuntimeOverlayCell,
    state: u8,
) {
    match target {
        NativeRuntimeOverlayCell::Real(rx, ry) => grid.set_overlay_data(rx, ry, state),
        NativeRuntimeOverlayCell::Dummy => {
            if let Some(terrain) = resolved_terrain {
                terrain.shared_cell_dummy().write_overlay_state(state);
            }
        }
    }
}

fn write_native_runtime_overlay_target_state_untracked(
    grid: &mut OverlayGrid,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    target: NativeRuntimeOverlayCell,
    state: u8,
) {
    match target {
        NativeRuntimeOverlayCell::Real(rx, ry) => {
            if let Some(index) = index_of(grid.width, grid.height, rx, ry)
                && grid.cells[index].overlay_id.is_some()
            {
                grid.cells[index].overlay_data = state;
            }
        }
        NativeRuntimeOverlayCell::Dummy => {
            if let Some(terrain) = resolved_terrain {
                terrain.shared_cell_dummy().write_overlay_state(state);
            }
        }
    }
}

fn clear_native_runtime_overlay_target(
    grid: &mut OverlayGrid,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    target: NativeRuntimeOverlayCell,
    preserve_owner: bool,
) {
    match target {
        NativeRuntimeOverlayCell::Real(rx, ry) => {
            if preserve_owner {
                grid.clear_overlay_preserving_owner(rx, ry);
            } else {
                grid.clear_overlay(rx, ry);
            }
        }
        NativeRuntimeOverlayCell::Dummy => {
            if let Some(terrain) = resolved_terrain {
                // The shared dummy's owner field is independent and is not part
                // of this overlay identity/state authority. Both native clear
                // branches zero +0x11E and write +0x44=-1.
                terrain
                    .shared_cell_dummy()
                    .write_overlay_identity_state(-1, 0);
            }
        }
    }
}

fn recompute_wall_connectivity_target(
    grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    target: NativeRuntimeOverlayCell,
) -> RecomputeResult {
    let (overlay_id, overlay_data) =
        native_runtime_overlay_target_state(grid, resolved_terrain, target);
    let Some(overlay_id) = overlay_id else {
        return RecomputeResult::NoChange;
    };
    let Some(flags) = registry.flags(overlay_id) else {
        return RecomputeResult::NoChange;
    };
    if !flags.wall {
        return RecomputeResult::NoChange;
    }

    // Cardinal neighbor connectivity scan. Bit assignment matches existing
    // compute_wall_connectivity: N=0, E=1, S=2, W=3.
    const CARDINAL: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];
    let mut connectivity: u8 = 0;
    for (bit, (dx, dy)) in CARDINAL.iter().enumerate() {
        // A real target retains its canonical coordinate. A dummy target is
        // the same shared pointer that each miss restamps, so reread +0x24
        // before every Adjacent_Cell call.
        let Some((base_x, base_y)) = native_runtime_overlay_target_coord(resolved_terrain, target)
        else {
            continue;
        };
        let Some(neighbor) = native_runtime_overlay_cell_lookup(
            grid.width(),
            grid.height(),
            resolved_terrain,
            base_x + dx,
            base_y + dy,
        ) else {
            continue;
        };
        let neighbor_id = native_runtime_overlay_target_state(grid, resolved_terrain, neighbor).0;
        let connects = match crate::sim::cell_kernel::wall_connection_step(
            neighbor_id.map_or(-1, i32::from),
            i32::from(overlay_id),
        ) {
            crate::sim::cell_kernel::WallConnectionStep::Return(connected) => connected,
            // Existing residual: this cleanup view does not implement native
            //480552..620's Building/Gate list arms for requested0/2/26. Retain
            // its prior false answer there; CAFNCB203 resolves in the prefix.
            crate::sim::cell_kernel::WallConnectionStep::GdiBuildings
            | crate::sim::cell_kernel::WallConnectionStep::NodBuildings => false,
        };
        if connects {
            connectivity |= 1 << bit;
        }
    }

    let damage_nibble = overlay_data & 0xF0;
    let new_byte = damage_nibble | connectivity;

    // Auto-destruct threshold fires whenever cleanup runs over an already-
    // damaged isolated wall, even if the connectivity nibble itself is
    // unchanged. Mirrors PostDestructionWallCleanup §5.2.
    if auto_destruct_threshold(overlay_id, new_byte) {
        clear_native_runtime_overlay_target(
            grid,
            resolved_terrain,
            target,
            matches!(overlay_id, 0x02 | 0x1A),
        );
        return RecomputeResult::Destroyed;
    }

    if new_byte == overlay_data {
        return RecomputeResult::NoChange;
    }

    write_native_runtime_overlay_target_state(grid, resolved_terrain, target, new_byte);
    RecomputeResult::Updated
}

/// Execute the overlay-identity/connectivity half of one native
/// PostDestructionWallCleanup visit selected by Get_CellClass. The returned
/// packed coordinate is captured before wall logic and therefore includes a
/// true shared-dummy receiver exactly as MarkTerrainDirty sees it. Real-cell
/// Recalc/zone/count consequences remain with the world transaction owner.
pub(crate) fn runtime_wall_cleanup_visit_at(
    grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    x: i32,
    y: i32,
    mut host: Option<&mut dyn WallDamageTransactionHost>,
) -> Option<RuntimeWallCleanupVisit> {
    let target =
        native_runtime_overlay_cell_lookup(grid.width(), grid.height(), resolved_terrain, x, y)?;
    let packed_coord = native_runtime_overlay_target_packed_coord(resolved_terrain, target)?;
    publish_wall_dirty_step_without_result(&mut host, WallDirtyStep::Tactical, packed_coord);
    publish_wall_dirty_step_without_result(&mut host, WallDirtyStep::Radar, packed_coord);
    let was_wall = native_runtime_overlay_target_state(grid, resolved_terrain, target)
        .0
        .and_then(|overlay_id| registry.flags(overlay_id))
        .is_some_and(|flags| flags.wall);
    let recomputed = if was_wall {
        recompute_wall_connectivity_target(grid, registry, resolved_terrain, target)
    } else {
        RecomputeResult::NoChange
    };
    if recomputed == RecomputeResult::Destroyed
        && let Some(host) = host
    {
        host.pointer_expired(target.into());
    }
    let real_cell = match target {
        NativeRuntimeOverlayCell::Real(rx, ry) => Some((rx, ry)),
        NativeRuntimeOverlayCell::Dummy => None,
    };
    Some(RuntimeWallCleanupVisit {
        packed_coord,
        real_cell,
        was_wall,
        recomputed,
    })
}

/// Runtime-hosted `PostDestructionWallCleanup(anchor, 1)` used by one wall
/// Mark transaction.
pub(crate) fn refresh_wall_connectivity_after_placement_with_host(
    grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    mut resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
    mut host: Option<&mut dyn WallDamageTransactionHost>,
) {
    assert!(
        resolved_terrain.is_some(),
        "retained wall-neighbor authority requires resolved terrain for placement cleanup"
    );
    const CLEANUP_CROSS: [(i32, i32); 5] = [(0, -1), (1, 0), (0, 1), (-1, 0), (0, 0)];
    for (dx, dy) in CLEANUP_CROSS {
        let Some(target) = native_runtime_overlay_cell_lookup(
            grid.width(),
            grid.height(),
            resolved_terrain.as_deref(),
            i32::from(rx) + dx,
            i32::from(ry) + dy,
        ) else {
            continue;
        };
        if let Some(coord) =
            native_runtime_overlay_target_packed_coord(resolved_terrain.as_deref(), target)
        {
            publish_wall_dirty_step_without_result(&mut host, WallDirtyStep::Tactical, coord);
            publish_wall_dirty_step_without_result(&mut host, WallDirtyStep::Radar, coord);
        }
        let was_wall =
            native_runtime_overlay_target_state(grid, resolved_terrain.as_deref(), target)
                .0
                .and_then(|overlay_id| registry.flags(overlay_id))
                .is_some_and(|flags| flags.wall);
        let result =
            recompute_wall_connectivity_target(grid, registry, resolved_terrain.as_deref(), target);
        if result == RecomputeResult::Destroyed
            && let Some(host) = host.as_deref_mut()
        {
            host.pointer_expired(target.into());
        }
        let NativeRuntimeOverlayCell::Real(nx, ny) = target else {
            continue;
        };
        if was_wall && let Some(terrain) = resolved_terrain.as_deref_mut() {
            let recalc = grid.recalculate_runtime_cell(terrain, registry, (nx, ny));
            if recalc.zone_changed
                && let Some(host) = host.as_deref_mut()
            {
                host.navigation_step(
                    terrain,
                    grid,
                    (nx, ny),
                    recalc.navigation_changed,
                    if result == RecomputeResult::Destroyed {
                        ZoneRepairKind::AssignOrphaned
                    } else {
                        ZoneRepairKind::MergeAdjacent
                    },
                );
            }
            // gamemd-derived: `CellClass::PostDestructionWallCleanup @
            // 0x00480630` runs its own eight-step `+0x122` decrement
            // (`0x00480999..0x004809EF`) only when this cell's hardcoded
            // isolated removal fired (`TEST BL,BL` at `0x0048097D`) AND
            // `RecalcAttributes` changed its zone type (load at
            // `0x00480972`, compare at `0x00480975`). A removed wall whose zone is unchanged keeps its
            // eight contributions, natively.
            if result == RecomputeResult::Destroyed && recalc.zone_changed {
                grid.remove_retained_wall_neighbor_source(Some(terrain), nx, ny);
            }
        }
    }
}

/// Reproduce the fixed cleanup fan-out after direct destruction.
///
/// N/W/S/E receivers each receive a N/E/S/W/self cross update in table order.
/// Cleanup removals do not recursively expand this fixed scope.
#[cfg(any(test, feature = "test-support"))]
pub fn cleanup_wall_neighbors(
    grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    rx: u16,
    ry: u16,
) -> Vec<(u16, u16)> {
    let mut result = WallDamageResult::default();
    let mut resolved_terrain = None;
    let mut host = None;
    cleanup_wall_neighbors_into(
        grid,
        registry,
        &mut resolved_terrain,
        &mut host,
        NativeRuntimeOverlayCell::Real(rx, ry),
        &mut result,
    );
    result.destroyed_cells
}

fn cleanup_wall_neighbors_into(
    grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    resolved_terrain: &mut Option<&mut ResolvedTerrainGrid>,
    host: &mut Option<&mut dyn WallDamageTransactionHost>,
    center: NativeRuntimeOverlayCell,
    result: &mut WallDamageResult,
) {
    const CLEANUP_RECEIVERS: [(i32, i32); 4] = [(0, -1), (-1, 0), (0, 1), (1, 0)];
    const CROSS: [(i32, i32); 5] = [(0, -1), (1, 0), (0, 1), (-1, 0), (0, 0)];

    for (outer_dx, outer_dy) in CLEANUP_RECEIVERS {
        let Some((center_x, center_y)) =
            native_runtime_overlay_target_coord(resolved_terrain.as_deref(), center)
        else {
            continue;
        };
        let Some(receiver) = native_runtime_overlay_cell_lookup(
            grid.width(),
            grid.height(),
            resolved_terrain.as_deref(),
            center_x + outer_dx,
            center_y + outer_dy,
        ) else {
            continue;
        };
        for (dx, dy) in CROSS {
            let Some(target) = native_cleanup_visit_target(
                grid.width(),
                grid.height(),
                resolved_terrain.as_deref(),
                receiver,
                dx,
                dy,
            ) else {
                continue;
            };
            if let Some(coord) =
                native_runtime_overlay_target_packed_coord(resolved_terrain.as_deref(), target)
            {
                // PostDestructionWallCleanup submits both calls before it
                // checks overlay identity or Wall=. Capture once so later
                // shared-dummy probes cannot split the pair across coords.
                publish_wall_dirty_step(host, result, WallDirtyStep::Tactical, coord);
                publish_wall_dirty_step(host, result, WallDirtyStep::Radar, coord);
            }
            let was_wall =
                native_runtime_overlay_target_state(grid, resolved_terrain.as_deref(), target)
                    .0
                    .and_then(|overlay_id| registry.flags(overlay_id))
                    .is_some_and(|flags| flags.wall);
            if !was_wall {
                continue;
            }
            let recomputed = recompute_wall_connectivity_target(
                grid,
                registry,
                resolved_terrain.as_deref(),
                target,
            );
            if recomputed == RecomputeResult::Destroyed
                && let Some(host) = host.as_deref_mut()
            {
                host.pointer_expired(target.into());
            }
            let NativeRuntimeOverlayCell::Real(nx, ny) = target else {
                // RecalcAttributes' first identity check suppresses the shared
                // dummy. Its overlay state and coordinate walk above remain
                // live, including pointer expiry when cleanup cleared it, but
                // no real-cell mutation or zone/count output exists.
                continue;
            };
            match recomputed {
                RecomputeResult::NoChange => {}
                RecomputeResult::Updated => {
                    result.changed_cells.push((nx, ny));
                    #[cfg(any(test, feature = "test-support"))]
                    result.mutations.push(WallMutation {
                        rx: nx,
                        ry: ny,
                        kind: WallMutationKind::CleanupUpdated,
                    });
                }
                RecomputeResult::Destroyed => {
                    result.destroyed_cells.push((nx, ny));
                    #[cfg(any(test, feature = "test-support"))]
                    result.mutations.push(WallMutation {
                        rx: nx,
                        ry: ny,
                        kind: WallMutationKind::CleanupRemoved,
                    });
                }
            }
            // PostDestructionWallCleanup Recalcs every visited wall, including
            // a wall whose connectivity byte was already current. A cleanup
            // removal's retained-count reversal is completed here only after
            // that Recalc proves the reduced zone changed.
            if let Some(terrain) = resolved_terrain.as_deref_mut() {
                let recalc = grid.recalculate_runtime_cell(terrain, registry, (nx, ny));
                if recalc.zone_changed
                    && let Some(host) = host.as_deref_mut()
                {
                    host.navigation_step(
                        terrain,
                        grid,
                        (nx, ny),
                        recalc.navigation_changed,
                        if recomputed == RecomputeResult::Destroyed {
                            ZoneRepairKind::AssignOrphaned
                        } else {
                            ZoneRepairKind::MergeAdjacent
                        },
                    );
                }
                // gamemd-derived: `CellClass::PostDestructionWallCleanup @
                // 0x00480630` runs its own eight-step `+0x122` decrement
                // (`0x00480999..0x004809EF`) only when this cell's hardcoded
                // isolated removal fired (`TEST BL,BL` at `0x0048097D`) AND
                // `RecalcAttributes` changed its zone type (load at
                // `0x00480972`, compare at `0x00480975`). A removed wall whose zone is unchanged keeps its
                // eight contributions, natively.
                if recomputed == RecomputeResult::Destroyed && recalc.zone_changed {
                    grid.remove_retained_wall_neighbor_source(Some(terrain), nx, ny);
                }
            }
        }
    }
}

/// 8-direction offsets: N, NE, E, SE, S, SW, W, NW.
#[cfg(any(test, feature = "test-support"))]
const ADJACENT_8: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// Static default for out-of-bounds reads.
const DEFAULT_CELL: OverlayCell = OverlayCell {
    overlay_id: None,
    overlay_data: 0,
    wall_owner: None,
    rubble_building_type: None,
};

fn index_of(width: u16, height: u16, rx: u16, ry: u16) -> Option<usize> {
    (rx < width && ry < height).then_some(ry as usize * width as usize + rx as usize)
}

/// A `Wall=yes` overlay on each of `cells`, written by the production
/// overlay attribute writer, with its type bound to `terrain` as a loaded map
/// binds its registry. Its land is retail `[Clear]`, so `0x004834A0`'s wall
/// test, not a missing land row, is what refuses a mover.
#[cfg(test)]
pub(crate) fn test_wall_overlays(
    terrain: &mut ResolvedTerrainGrid,
    cells: &[(u16, u16)],
) -> OverlayGrid {
    let registry = OverlayTypeRegistry::from_ini(
        &crate::rules::ini_parser::IniFile::from_str(&format!(
            "[OverlayTypes]\n0=TESTWALL\n[TESTWALL]\nWall=yes\n{}",
            crate::map::resolved_terrain::TEST_RETAIL_CLEAR_INI
        )),
        None,
    );
    terrain.set_overlay_types_for_test(&registry);
    let mut overlays = OverlayGrid::new(terrain.width(), terrain.height());
    for &(x, y) in cells {
        overlays.place_overlay(x, y, 0, 0);
        recalc_overlay_passability(&mut overlays, terrain, &registry, x, y);
    }
    overlays
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn retained_wall_plane_for_sources(
        terrain: &ResolvedTerrainGrid,
        sources: &[(u16, u16)],
    ) -> Vec<u8> {
        let mut counts = vec![0u8; terrain.cells().len()];
        for &(rx, ry) in sources {
            let anchor = (rx as i16, ry as i16);
            for (dx, dy) in ADJACENT_8 {
                let Some(index) = terrain.native_fixed_cell_index(
                    anchor.0.wrapping_add(dx as i16),
                    anchor.1.wrapping_add(dy as i16),
                ) else {
                    continue;
                };
                counts[index] = counts[index].wrapping_add(1);
            }
        }
        counts
    }

    #[test]
    fn finalized_payload_is_the_only_input_and_preserves_data_only_cells() {
        let payload = FinalizedOverlayPayload::from_cells_for_test(
            2,
            1,
            vec![(-1, 37), (0x18, 9)],
            vec![5, 8],
        );
        let grid = OverlayGrid::from_finalized_map_payload(payload);

        assert_eq!(
            (grid.cell(0, 0).overlay_id, grid.cell(0, 0).overlay_data),
            (None, 37)
        );
        assert_eq!(
            (grid.cell(1, 0).overlay_id, grid.cell(1, 0).overlay_data),
            (Some(0x18), 9)
        );
        assert!(grid.dirty_cells.is_empty());
        assert_eq!(grid.retained_neighbor_counts(), &[5, 8][..]);
    }

    #[test]
    fn gsi_04_07_map_seeded_overlay_is_unowned() {
        let grid = OverlayGrid::from_overlay_entries(
            &[OverlayEntry {
                rx: 1,
                ry: 2,
                overlay_id: 2,
                frame: 0x18,
            }],
            4,
            4,
        );
        let cell = grid.cell(1, 2);
        assert_eq!(cell.overlay_id, Some(2));
        assert_eq!(cell.overlay_data, 0x18);
        assert_eq!(cell.wall_owner, None);
    }

    #[test]
    fn gsi_04_09_overlay_pack_init_preserves_all_raw_data_without_dirtying() {
        let entries = [OverlayEntry {
            rx: 1,
            ry: 1,
            overlay_id: 5,
            frame: 7,
        }];
        let data = OverlayDataPack::from_cells([(0, 0, 42), (1, 1, 9)]);
        let mut present = OverlayGrid::from_overlay_packs(&entries, &data, 2, 2);

        assert_eq!(present.cell(0, 0).overlay_id, None);
        assert_eq!(present.cell(0, 0).overlay_data, 42);
        assert_eq!(present.cell(1, 1).overlay_id, Some(5));
        assert_eq!(
            present.cell(1, 1).overlay_data,
            9,
            "raw data pack overwrites the frame stamped with OverlayPack"
        );
        assert!(present.take_dirty_cells().is_empty());

        let mut missing =
            OverlayGrid::from_overlay_packs(&entries, &OverlayDataPack::missing(), 2, 2);
        assert_eq!(missing.cell(0, 0).overlay_id, None);
        assert_eq!(missing.cell(0, 0).overlay_data, 0);
        assert_eq!(missing.cell(1, 1).overlay_id, Some(5));
        assert_eq!(missing.cell(1, 1).overlay_data, 7);
        assert!(missing.take_dirty_cells().is_empty());
    }

    #[test]
    fn bridge_publication_literal_fields_do_not_schedule_attribute_recalc() {
        let mut grid = OverlayGrid::new(1, 1);
        assert!(grid.write_literal_bridge_state(0, 0, 15));
        assert_eq!(grid.cell(0, 0).overlay_id, None);
        assert_eq!(grid.cell(0, 0).overlay_data, 15);
        assert!(grid.pending_dirty_cells().is_empty());
        grid.cells[0].overlay_id = Some(24);
        grid.cells[0].wall_owner = Some(crate::sim::intern::test_intern("RetainedOwner"));
        let owner = grid.cells[0].wall_owner;
        assert!(grid.clear_literal_bridge_identity(0, 0));
        assert_eq!(grid.cell(0, 0).overlay_id, None);
        assert_eq!(
            grid.cell(0, 0).overlay_data,
            15,
            "identity and state are independent stores"
        );
        assert_eq!(grid.cell(0, 0).wall_owner, owner);
        assert!(grid.pending_dirty_cells().is_empty());
        assert_eq!(grid.take_removed_render_cells(), vec![(0, 0)]);
    }

    #[test]
    fn clear_overlay_pushes_dirty_when_in_bounds() {
        let mut grid = OverlayGrid::new(10, 10);
        grid.place_overlay(2, 2, 5, 0);
        grid.dirty_cells.clear();
        let prev = grid.clear_overlay(2, 2);
        assert_eq!(prev, Some(5));
        assert_eq!(grid.dirty_cells, vec![(2, 2)]);
    }

    #[test]
    fn set_overlay_data_pushes_only_when_cell_has_overlay() {
        let mut grid = OverlayGrid::new(10, 10);
        // No overlay → no push.
        grid.set_overlay_data(1, 1, 42);
        assert!(grid.dirty_cells.is_empty());
        // With overlay → push.
        grid.place_overlay(1, 1, 9, 0);
        grid.dirty_cells.clear();
        grid.set_overlay_data(1, 1, 42);
        assert_eq!(grid.dirty_cells, vec![(1, 1)]);
    }

    #[test]
    fn dirty_cells_preserve_push_order() {
        // Determinism: drain must iterate in push order.
        let mut grid = OverlayGrid::new(10, 10);
        grid.place_overlay(5, 5, 1, 0);
        grid.place_overlay(0, 0, 2, 0);
        grid.place_overlay(9, 3, 3, 0);
        assert_eq!(grid.take_dirty_cells(), vec![(5, 5), (0, 0), (9, 3)]);
    }

    fn single_cell_terrain(
        base_land_type: u8,
        speed_costs: crate::rules::terrain_rules::SpeedCostProfile,
        base_ground_walk_blocked: bool,
    ) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
        use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};

        ResolvedTerrainGrid::from_cells(
            1,
            1,
            vec![ResolvedTerrainCell {
                filled_clear: true,
                tileset_index: None,
                land_type: base_land_type,
                speed_costs,
                accepts_smudge: true,
                ground_walk_blocked: base_ground_walk_blocked,
                zone_type: zone_class::GROUND,
                base_ground_walk_blocked,
                base_land_type,
                base_speed_costs: speed_costs,
                ..crate::map::resolved_terrain::test_flat_cell(0, 0)
            }],
        )
    }

    pub(super) fn clear_terrain_grid(width: u16, height: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_grid(
            width,
            height,
            crate::map::resolved_terrain::test_loader_clear_cell,
        )
    }

    fn gsi_04_07_placement_registry() -> OverlayTypeRegistry {
        use crate::rules::ini_parser::IniFile;

        let ini = IniFile::from_str(
            "[OverlayTypes]\n\
             9=PROTECTED\n\
             2=REPLACEABLE\n\
             7=NEWROCK\n\
             4=ANIMONLY\n\
             1=CRATEOVL\n\
             3=WALL\n\
             6=TIBERIUM\n\
             [Animations]\n0=SPARK\n\
             [PROTECTED]\nOverrides=yes\n\
             [REPLACEABLE]\nOverrides=no\n\
             [NEWROCK]\nIsARock=yes\n\
             [ANIMONLY]\nCellAnim=SPARK\n\
             [CRATEOVL]\nCrate=yes\n\
             [WALL]\nWall=yes\n\
             [TIBERIUM]\nTiberium=yes\n",
        );
        OverlayTypeRegistry::from_ini(&ini, None)
    }

    /// The generator's overlays land as written (see
    /// [`OverlayGrid::from_generated_overlays`]), with no `OverlayClass::Mark`
    /// effect: a rock on a steep cell stays (the slope gate at `0x005FC5CD`
    /// would refuse it), a crate keeps its byte (Mark writes 0xFF at
    /// `0x005FD0FB`) and a wall moves no neighbour count
    /// (`0x005FC758..0x005FC775`). The later `RecalcAttributes` still drops a
    /// sloped resource together with its data, and an unallocated cell takes
    /// neither identity nor data.
    #[test]
    fn generated_overlays_are_written_without_mark() {
        let registry = gsi_04_07_placement_registry();
        let id = |name| registry.id_for_name(name).expect(name);
        let (rock, crate_overlay, wall, ore) =
            (id("NEWROCK"), id("CRATEOVL"), id("WALL"), id("TIBERIUM"));
        let mut terrain = clear_terrain_grid(6, 3);
        terrain.test_set_native_allocated_cells(
            &(0..2)
                .flat_map(|y| (0..6).map(move |x| (x, y)))
                .collect::<Vec<_>>(),
        );
        terrain
            .cell_mut_for_test(0, 0)
            .expect("rock cell")
            .slope_type = 5;
        terrain
            .cell_mut_for_test(3, 0)
            .expect("sloped ore cell")
            .slope_type = 1;
        let placed = [
            ((0, 0), rock, 3),
            ((1, 0), crate_overlay, 41),
            ((2, 1), wall, 0),
            ((3, 0), ore, 7),
            ((4, 0), ore, 7),
            ((5, 2), ore, 7),
        ];
        let entries = placed.map(|((rx, ry), overlay_id, frame)| OverlayEntry {
            rx,
            ry,
            overlay_id,
            frame,
        });
        let data = OverlayDataPack::from_cells(placed.map(|((rx, ry), _, frame)| (rx, ry, frame)));

        let grid = OverlayGrid::from_generated_overlays(&entries, &data, &mut terrain, &registry);

        let cell = |rx, ry| {
            let overlay = grid.cell(rx, ry);
            (overlay.overlay_id, overlay.overlay_data)
        };
        assert_eq!(cell(0, 0), (Some(rock), 3), "the steep rock stays");
        assert_eq!(cell(1, 0), (Some(crate_overlay), 41), "no crate sentinel");
        assert_eq!(cell(2, 1), (Some(wall), 0));
        assert!(
            grid.retained_neighbor_counts()
                .iter()
                .all(|&count| count == 0),
            "the wall counts no neighbours"
        );
        assert_eq!(
            cell(3, 0),
            (None, 0),
            "Recalc drops the sloped ore and its data"
        );
        assert_eq!(cell(4, 0), (Some(ore), 7));
        assert_eq!(cell(5, 2), (None, 0), "an unallocated cell takes neither");
    }

    #[test]
    fn retained_wall_plane_tracks_cleanup_removal_with_changed_and_unchanged_zones() {
        use crate::rules::ini_parser::IniFile;

        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=GASAND\n\
             [GASAND]\nWall=yes\nStrength=100\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let mut terrain = clear_terrain_grid(5, 5);
        let plane = retained_wall_plane_for_sources(&terrain, &[(2, 2), (2, 1)]);
        let mut grid = OverlayGrid::from_finalized_map_payload(
            FinalizedOverlayPayload::from_cells_for_test(5, 5, vec![(-1, 0); 25], plane),
        );
        grid.place_overlay(2, 2, 0, 0x01);
        grid.place_overlay(2, 1, 0, 0x24);
        for (rx, ry) in [(2, 2), (2, 1)] {
            let _ = recalc_overlay_passability(&mut grid, &mut terrain, &registry, rx, ry);
        }

        let mut rng = crate::sim::rng::SimRng::new(7);
        let result = damage_wall_overlay_with_terrain(
            &mut grid,
            &registry,
            Some(&mut terrain),
            2,
            2,
            -1,
            &mut rng,
        );
        assert_eq!(result.destroyed_cells, vec![(2, 2), (2, 1)]);
        assert_eq!(
            grid.take_synchronous_navigation_cells(),
            vec![(2, 2), (2, 1)],
            "direct Recalc precedes the cleanup-removal Recalc"
        );
        assert!(
            grid.retained_neighbor_counts()
                .iter()
                .all(|&count| count == 0),
            "direct and cleanup removals each reverse one retained source"
        );

        let mut unchanged_zone_terrain = clear_terrain_grid(5, 5);
        let cleanup_cell = unchanged_zone_terrain
            .cell_mut_for_test(2, 1)
            .expect("cleanup cell");
        cleanup_cell.outside_playfield = true;
        cleanup_cell.zone_type = crate::map::resolved_terrain::zone_class::OUTSIDE;
        let full_plane =
            retained_wall_plane_for_sources(&unchanged_zone_terrain, &[(2, 2), (2, 1)]);
        let retained_cleanup_plane =
            retained_wall_plane_for_sources(&unchanged_zone_terrain, &[(2, 1)]);
        let mut unchanged_zone_grid = OverlayGrid::from_finalized_map_payload(
            FinalizedOverlayPayload::from_cells_for_test(5, 5, vec![(-1, 0); 25], full_plane),
        );
        unchanged_zone_grid.place_overlay(2, 2, 0, 0x01);
        unchanged_zone_grid.place_overlay(2, 1, 0, 0x24);
        for (rx, ry) in [(2, 2), (2, 1)] {
            let _ = recalc_overlay_passability(
                &mut unchanged_zone_grid,
                &mut unchanged_zone_terrain,
                &registry,
                rx,
                ry,
            );
        }
        let result = damage_wall_overlay_with_terrain(
            &mut unchanged_zone_grid,
            &registry,
            Some(&mut unchanged_zone_terrain),
            2,
            2,
            -1,
            &mut rng,
        );
        assert_eq!(result.destroyed_cells, vec![(2, 2), (2, 1)]);
        assert_eq!(
            unchanged_zone_grid.retained_neighbor_counts(),
            retained_cleanup_plane.as_slice(),
            "cleanup removal retains its source when Recalc leaves zone type unchanged"
        );
    }

    #[test]
    fn generic_overlay_mutation_is_retained_count_neutral() {
        let mut grid =
            OverlayGrid::from_finalized_map_payload(FinalizedOverlayPayload::from_cells_for_test(
                2,
                2,
                vec![(0, 0), (-1, 0), (-1, 0), (-1, 0)],
                vec![7, 8, 9, 10],
            ));
        let retained = grid.retained_neighbor_counts().to_vec();

        assert_eq!(grid.clear_overlay(0, 0), Some(0));
        grid.place_overlay(1, 1, 3, 4);
        assert_eq!(
            grid.retained_neighbor_counts(),
            retained.as_slice(),
            "generic identity writers cannot infer or reverse historical wall sources"
        );
    }

    #[test]
    #[should_panic(
        expected = "retained wall-neighbor authority requires resolved terrain for wall damage"
    )]
    fn finalized_wall_damage_rejects_missing_resolved_terrain() {
        use crate::rules::ini_parser::IniFile;

        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=WALL\n\
             [WALL]\nWall=yes\nStrength=100\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let mut grid = OverlayGrid::from_finalized_map_payload(
            FinalizedOverlayPayload::from_cells_for_test(1, 1, vec![(0, 0)], vec![0]),
        );
        let mut rng = crate::sim::rng::SimRng::new(1);
        let _ = damage_wall_overlay_with_terrain(&mut grid, &registry, None, 0, 0, -1, &mut rng);
    }

    #[test]
    fn gsi_04_07_placement_map_wall_owner_filters_and_measures_cell_target() {
        let registry = gsi_04_07_placement_registry();
        let terrain = clear_terrain_grid(5, 5);
        let mut grid = OverlayGrid::new(5, 5);
        grid.place_overlay(2, 2, 5, 0);
        grid.take_dirty_cells();
        let wall_center = 2 * 256 + 128;
        let candidates = [
            MapWallOwnerCandidate {
                owner: InternedId::from_index(1),
                world_x: wall_center,
                world_y: wall_center,
                world_z: 0,
                object_alive: false,
                cell_marked: true,
                house_wall_owner: true,
            },
            MapWallOwnerCandidate {
                owner: InternedId::from_index(2),
                world_x: wall_center + 500,
                world_y: wall_center,
                world_z: 0,
                object_alive: true,
                cell_marked: true,
                house_wall_owner: true,
            },
            MapWallOwnerCandidate {
                owner: InternedId::from_index(3),
                world_x: wall_center + 600,
                world_y: wall_center,
                world_z: 0,
                object_alive: true,
                cell_marked: true,
                house_wall_owner: true,
            },
        ];

        grid.reconstruct_map_wall_owners(&terrain, &registry, &candidates);
        assert_eq!(grid.cell(2, 2).wall_owner, Some(InternedId::from_index(2)));

        let mut filtered = candidates;
        filtered[0].object_alive = true;
        filtered[0].cell_marked = false;
        filtered[1].house_wall_owner = false;
        filtered[2].object_alive = false;
        grid.reconstruct_map_wall_owners(&terrain, &registry, &filtered);
        assert_eq!(grid.cell(2, 2).wall_owner, None);
    }

    #[test]
    fn gsi_04_07_placement_map_wall_owner_strict_tie_keeps_first() {
        let registry = gsi_04_07_placement_registry();
        let terrain = clear_terrain_grid(5, 5);
        let mut grid = OverlayGrid::new(5, 5);
        grid.place_overlay(2, 2, 5, 0);
        let wall_center = 2 * 256 + 128;
        let candidate = |owner, delta| MapWallOwnerCandidate {
            owner: InternedId::from_index(owner),
            world_x: wall_center + delta,
            world_y: wall_center,
            world_z: 0,
            object_alive: true,
            cell_marked: true,
            house_wall_owner: true,
        };
        grid.reconstruct_map_wall_owners(
            &terrain,
            &registry,
            &[candidate(7, 400), candidate(8, -400)],
        );
        assert_eq!(grid.cell(2, 2).wall_owner, Some(InternedId::from_index(7)));
    }

    #[test]
    fn gsi_04_07_placement_map_wall_owner_uses_native_lut_distance_tie() {
        assert_eq!(object_distance([0, 3968, 1040], [0, 0, 0], None), 4101);
        assert_eq!(object_distance([0, 4096, 208], [0, 0, 0], None), 4101);

        let registry = gsi_04_07_placement_registry();
        let terrain = clear_terrain_grid(5, 5);
        let mut grid = OverlayGrid::new(5, 5);
        grid.place_overlay(2, 2, 5, 0);
        let wall_center = 2 * 256 + 128;
        let candidate = |owner, dy: i32, dz: i32| MapWallOwnerCandidate {
            owner: InternedId::from_index(owner),
            world_x: wall_center,
            world_y: wall_center.wrapping_sub(dy),
            world_z: 0i32.wrapping_sub(dz),
            object_alive: true,
            cell_marked: true,
            house_wall_owner: true,
        };

        grid.reconstruct_map_wall_owners(
            &terrain,
            &registry,
            &[candidate(9, 3968, 1040), candidate(10, 4096, 208)],
        );

        assert_eq!(grid.cell(2, 2).wall_owner, Some(InternedId::from_index(9)));
    }

    fn gsi_04_04_registry() -> OverlayTypeRegistry {
        use crate::rules::ini_parser::IniFile;

        let ini = IniFile::from_str(
            "\
[OverlayTypes]
0=CRUSHWALL
1=WALLFLAG
2=ZEROLAND
3=ROCKFLAG
4=RUBBLE
5=GATEOVL
6=ONELAND
7=ROADKEEP
8=ROADRESTORE
9=WALLLAND
10=RAILLAND
11=ORE
12=WATERKEEP
13=ROUGHKEEP
14=STOCKORE
15=WALLORE
16=RAILORE
[Clear]
Wheel=100%
[Road]
Foot=80%
Track=70%
Wheel=55%
[Water]
Float=66%
Wheel=44%
[Rock]
Wheel=0%
[Wall]
Wheel=40%
[Tiberium]
Foot=90%
Track=70%
Wheel=80%
[Rough]
Foot=77%
Wheel=33%
[Ice]
Wheel=1%
[Railroad]
Wheel=60%
[CRUSHWALL]
Crushable=yes
Wall=yes
NoUseTileLandType=no
[WALLFLAG]
Wall=yes
NoUseTileLandType=no
[ZEROLAND]
Land=Rock
NoUseTileLandType=no
[ROCKFLAG]
IsARock=yes
NoUseTileLandType=no
[RUBBLE]
IsRubble=yes
NoUseTileLandType=no
[GATEOVL]
Gate=yes
NoUseTileLandType=no
[ONELAND]
Land=Ice
NoUseTileLandType=no
[ROADKEEP]
Land=Road
NoUseTileLandType=yes
[ROADRESTORE]
Land=Road
NoUseTileLandType=no
[WALLLAND]
Land=Wall
NoUseTileLandType=no
[RAILLAND]
Land=Railroad
NoUseTileLandType=no
[ORE]
Tiberium=yes
NoUseTileLandType=no
[WATERKEEP]
Land=Water
NoUseTileLandType=yes
[ROUGHKEEP]
Land=Rough
NoUseTileLandType=yes
[STOCKORE]
Tiberium=yes
[WALLORE]
Tiberium=yes
Land=Wall
NoUseTileLandType=no
[RAILORE]
Tiberium=yes
Land=Railroad
NoUseTileLandType=no
",
        );
        OverlayTypeRegistry::from_ini(&ini, None)
    }

    #[test]
    fn gsi_04_04_recalc_zone_type_overlay_priority_matches_gamemd() {
        use crate::map::resolved_terrain::zone_class;
        use crate::rules::ini_parser::IniFile;
        use crate::rules::terrain_rules::LandType;
        use crate::rules::terrain_rules::SpeedCostProfile;

        let ini = IniFile::from_str(
            "\
[OverlayTypes]
0=SANDBAG
1=HARDWALL
2=ROCKOVL
3=RUBBLE
[Clear]
Wheel=100%
[Rock]
Wheel=0%
[SANDBAG]
Crushable=yes
Wall=yes
Land=Clear
[HARDWALL]
Wall=yes
Land=Clear
[ROCKOVL]
Land=Rock
[RUBBLE]
IsRubble=yes
",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let clear = LandType::Clear.as_index();

        let cases = [
            (0, zone_class::CRUSHABLE, false),
            (1, zone_class::WALL, true),
            (2, zone_class::IMPASSABLE, true),
            (3, zone_class::GROUND, false),
        ];
        for (overlay_id, expected_zone, expected_blocks) in cases {
            let mut overlay_grid = OverlayGrid::new(1, 1);
            overlay_grid.place_overlay(0, 0, overlay_id, 0);
            let mut terrain = single_cell_terrain(clear, SpeedCostProfile::default(), false);
            recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);
            let cell = terrain.cell(0, 0).expect("cell");
            assert_eq!(cell.zone_type, expected_zone, "overlay id {overlay_id}");
            assert_eq!(
                cell.overlay_blocks, expected_blocks,
                "overlay id {overlay_id}"
            );
        }
    }

    #[test]
    fn low_bridge_land_override_clears_the_water_tiles_ground_block() {
        use crate::rules::ini_parser::IniFile;
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile};

        // A low bridge is an overlay, not a raised deck: LOBRDG01..28 /
        // LOBRDB01..28 carry `Land=Road` and `NoUseTileLandType=yes`, and their
        // span cells sit directly on the water tiles they cross.
        let ini = IniFile::from_str(
            "[OverlayTypes]
0=LOBRDG10
[Road]
Foot=100%
Track=100%
Wheel=100%
[Water]
Foot=0%
Track=0%
Wheel=0%
Float=100%
[LOBRDG10]
Land=Road
NoUseTileLandType=yes
",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);
        let water_costs = SpeedCostProfile {
            wheel: Some(0),
            ..Default::default()
        };

        let mut terrain = single_cell_terrain(LandType::Water.as_index(), water_costs, true);
        let mut overlay_grid = OverlayGrid::new(1, 1);
        overlay_grid.place_overlay(0, 0, 0, 1);
        recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);

        let deck = terrain.cell(0, 0).expect("bridge deck cell");
        assert_eq!(deck.land_type, LandType::Road.as_index());
        // The regression: `ground_walk_blocked` used to be rebuilt from
        // `base_ground_walk_blocked`, so the water underneath kept the deck
        // closed to every ground unit even though its LandType was Road.
        assert!(
            !deck.ground_walk_blocked,
            "Land=Road + NoUseTileLandType must carry its own passability onto the cell"
        );
        // The pristine snapshot stays water — it is the restoration value.
        assert!(deck.base_ground_walk_blocked);
        assert_eq!(deck.base_land_type, LandType::Water.as_index());

        // Removing the bridge hands the cell back to the water tile.
        *overlay_grid.cell_mut_for_test(0, 0) = OverlayCell::default();
        recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);
        let bare = terrain.cell(0, 0).expect("cleared cell");
        assert_eq!(bare.land_type, LandType::Water.as_index());
        assert!(bare.ground_walk_blocked);
    }

    #[test]
    fn gsi_04_04_recalc_zone_type_water_and_beach_precede_speed_threshold() {
        use crate::map::resolved_terrain::zone_class;
        use crate::rules::terrain_rules::LandType;
        use crate::rules::terrain_rules::SpeedCostProfile;

        let registry = OverlayTypeRegistry::empty();
        let mut overlay_grid = OverlayGrid::new(1, 1);
        let zero_wheel = SpeedCostProfile {
            wheel: Some(0),
            ..Default::default()
        };

        let mut water = single_cell_terrain(LandType::Water.as_index(), zero_wheel, true);
        recalc_overlay_passability(&mut overlay_grid, &mut water, &registry, 0, 0);
        assert_eq!(water.cell(0, 0).unwrap().zone_type, zone_class::WATER);

        let mut beach = single_cell_terrain(LandType::Beach.as_index(), zero_wheel, false);
        recalc_overlay_passability(&mut overlay_grid, &mut beach, &registry, 0, 0);
        assert_eq!(beach.cell(0, 0).unwrap().zone_type, zone_class::BEACH);

        let mut rock = single_cell_terrain(LandType::Rock.as_index(), zero_wheel, true);
        recalc_overlay_passability(&mut overlay_grid, &mut rock, &registry, 0, 0);
        assert_eq!(rock.cell(0, 0).unwrap().zone_type, zone_class::IMPASSABLE);
    }

    #[test]
    fn gsi_04_04_recalc_zone_exact_priority_thresholds_and_non_predicates() {
        use crate::map::resolved_terrain::zone_class;
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile};

        let registry = gsi_04_04_registry();
        let clear = LandType::Clear.as_index();
        let clear_speed = SpeedCostProfile {
            wheel: Some(100),
            ..Default::default()
        };

        for (overlay_id, expected_zone, expected_blocks) in [
            (0, zone_class::CRUSHABLE, false),
            (1, zone_class::WALL, true),
            (2, zone_class::IMPASSABLE, true),
            (3, zone_class::IMPASSABLE, true),
            (4, zone_class::GROUND, false),
        ] {
            let mut overlay_grid = OverlayGrid::new(1, 1);
            overlay_grid.place_overlay(0, 0, overlay_id, 0);
            let mut terrain = single_cell_terrain(clear, clear_speed, false);
            terrain
                .cell_mut_for_test(0, 0)
                .unwrap()
                .terrain_object_occupation = Some(7);
            terrain
                .cell_mut_for_test(0, 0)
                .unwrap()
                .terrain_object_blocks = true;
            recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);
            let cell = terrain.cell(0, 0).expect("priority cell");
            assert_eq!(cell.zone_type, expected_zone, "overlay {overlay_id}");
            assert_eq!(cell.overlay_blocks, expected_blocks, "overlay {overlay_id}");
        }

        let zero_wheel = SpeedCostProfile {
            wheel: Some(0),
            ..Default::default()
        };
        let mut overlay_grid = OverlayGrid::new(1, 1);
        overlay_grid.place_overlay(0, 0, 4, 0);
        let mut rubble_on_water =
            single_cell_terrain(LandType::Water.as_index(), zero_wheel, false);
        recalc_overlay_passability(&mut overlay_grid, &mut rubble_on_water, &registry, 0, 0);
        assert_eq!(
            rubble_on_water.cell(0, 0).unwrap().zone_type,
            zone_class::GROUND,
            "rubble is a terminal overlay result, not absence"
        );

        overlay_grid.place_overlay(0, 0, 5, 0);
        let mut gate_on_water = single_cell_terrain(LandType::Water.as_index(), zero_wheel, false);
        recalc_overlay_passability(&mut overlay_grid, &mut gate_on_water, &registry, 0, 0);
        assert_eq!(
            gate_on_water.cell(0, 0).unwrap().zone_type,
            zone_class::WATER
        );

        overlay_grid.place_overlay(0, 0, 6, 0);
        let mut exact_one_overlay = single_cell_terrain(clear, clear_speed, false);
        recalc_overlay_passability(&mut overlay_grid, &mut exact_one_overlay, &registry, 0, 0);
        assert_eq!(
            exact_one_overlay.cell(0, 0).unwrap().zone_type,
            zone_class::GROUND,
            "overlay Land wheel=1 is not the exact-zero overlay predicate"
        );

        let mut empty_grid = OverlayGrid::new(1, 1);
        for (wheel, expected) in [
            (0, zone_class::IMPASSABLE),
            (1, zone_class::IMPASSABLE),
            (2, zone_class::GROUND),
        ] {
            let speed = SpeedCostProfile {
                wheel: Some(wheel),
                ..Default::default()
            };
            let mut terrain = single_cell_terrain(clear, speed, false);
            recalc_overlay_passability(&mut empty_grid, &mut terrain, &registry, 0, 0);
            assert_eq!(
                terrain.cell(0, 0).unwrap().zone_type,
                expected,
                "wheel={wheel}"
            );
        }

        for (occupation, expected_zone, expected_blocks) in [
            (7, zone_class::WALL, true),
            (4, zone_class::BUILDING, true),
            (0, zone_class::BUILDING, false),
        ] {
            let mut object = single_cell_terrain(clear, clear_speed, false);
            let object_cell = object.cell_mut_for_test(0, 0).unwrap();
            object_cell.terrain_object_occupation = Some(occupation);
            object_cell.terrain_object_blocks = expected_blocks;
            recalc_overlay_passability(&mut empty_grid, &mut object, &registry, 0, 0);
            assert_eq!(object.cell(0, 0).unwrap().zone_type, expected_zone);
        }

        let mut blocked_base = single_cell_terrain(clear, clear_speed, true);
        recalc_overlay_passability(&mut empty_grid, &mut blocked_base, &registry, 0, 0);
        assert_eq!(
            blocked_base.cell(0, 0).unwrap().zone_type,
            zone_class::GROUND,
            "base_ground_walk_blocked is not a RecalcZoneType predicate"
        );
    }

    #[test]
    fn gsi_04_04_runtime_land_writer_retains_profiles_and_restores_pristine_tile() {
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile};

        let registry = gsi_04_04_registry();
        let base_speed = SpeedCostProfile {
            wheel: Some(100),
            ..Default::default()
        };

        for (overlay_id, land) in [
            (7, LandType::Road),
            (9, LandType::Wall),
            (10, LandType::Railroad),
            (12, LandType::Water),
            (13, LandType::Rough),
        ] {
            let mut overlay_grid = OverlayGrid::new(1, 1);
            overlay_grid.place_overlay(0, 0, overlay_id, 0);
            let mut terrain = single_cell_terrain(LandType::Clear.as_index(), base_speed, false);
            recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);
            let cell = terrain.cell(0, 0).expect("retained land cell");
            assert_eq!(cell.land_type, land.as_index(), "overlay {overlay_id}");
            assert_eq!(
                cell.speed_costs,
                registry
                    .flags(overlay_id)
                    .unwrap()
                    .land_speed_costs
                    .unwrap(),
                "overlay {overlay_id} profile"
            );
        }

        let mut overlay_grid = OverlayGrid::new(1, 1);
        overlay_grid.place_overlay(0, 0, 8, 0);
        let mut terrain = single_cell_terrain(LandType::Clear.as_index(), base_speed, false);
        recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);
        assert_eq!(
            terrain.cell(0, 0).unwrap().land_type,
            LandType::Clear.as_index()
        );

        overlay_grid.place_overlay(0, 0, 7, 0);
        recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);
        assert_eq!(
            terrain.cell(0, 0).unwrap().land_type,
            LandType::Road.as_index()
        );
        overlay_grid.clear_overlay(0, 0);
        recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);
        let restored = terrain.cell(0, 0).unwrap();
        assert_eq!(restored.land_type, LandType::Clear.as_index());
        assert_eq!(restored.speed_costs, base_speed);
    }

    #[test]
    fn gsi_04_04_runtime_tiberium_slope_branches_clear_exact_overlays() {
        use crate::map::resolved_terrain::zone_class;
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile};

        let registry = gsi_04_04_registry();
        let base_speed = SpeedCostProfile {
            wheel: Some(100),
            ..Default::default()
        };
        let ordinary_ore_speed = registry
            .flags(11)
            .expect("ordinary ore flags")
            .land_speed_costs
            .unwrap_or_default();

        for slope in [0, 4, 5] {
            let mut overlay_grid = OverlayGrid::new(1, 1);
            overlay_grid.place_overlay(0, 0, 11, 7);
            let mut terrain = single_cell_terrain(LandType::Clear.as_index(), base_speed, false);
            terrain.cell_mut_for_test(0, 0).unwrap().slope_type = slope;

            assert!(recalc_overlay_passability(
                &mut overlay_grid,
                &mut terrain,
                &registry,
                0,
                0
            ));

            let cell = terrain.cell(0, 0).unwrap();
            if slope < 5 {
                assert_eq!(overlay_grid.cell(0, 0).overlay_id, Some(11));
                assert_eq!(cell.land_type, LandType::Tiberium.as_index());
                assert_eq!(cell.speed_costs, ordinary_ore_speed);
            } else {
                assert_eq!(overlay_grid.cell(0, 0), &OverlayCell::default());
                assert_eq!(cell.land_type, LandType::Clear.as_index());
                assert_eq!(cell.speed_costs, base_speed);
            }
            assert_eq!(cell.overlay_zone_type, None);
            assert!(!cell.overlay_blocks);
            assert_eq!(cell.zone_type, zone_class::GROUND);
        }

        for (overlay_id, land) in [
            (14, LandType::Tiberium),
            (15, LandType::Wall),
            (16, LandType::Railroad),
        ] {
            let expected_speed = registry
                .flags(overlay_id)
                .expect("early resource flags")
                .land_speed_costs
                .unwrap_or_default();
            for slope in [0, 1, 4, 5] {
                let mut overlay_grid = OverlayGrid::new(1, 1);
                overlay_grid.place_overlay(0, 0, overlay_id, 7);
                let mut terrain =
                    single_cell_terrain(LandType::Clear.as_index(), base_speed, false);
                terrain.cell_mut_for_test(0, 0).unwrap().slope_type = slope;

                assert!(recalc_overlay_passability(
                    &mut overlay_grid,
                    &mut terrain,
                    &registry,
                    0,
                    0
                ));
                if slope == 0 {
                    assert_eq!(overlay_grid.cell(0, 0).overlay_id, Some(overlay_id));
                } else {
                    assert_eq!(overlay_grid.cell(0, 0), &OverlayCell::default());
                }
                let cell = terrain.cell(0, 0).unwrap();
                assert_eq!(cell.land_type, land.as_index());
                assert_eq!(cell.land_type, land.as_index());
                assert_eq!(cell.speed_costs, expected_speed);
                assert_eq!(cell.base_land_type, LandType::Clear.as_index());
                assert_eq!(cell.base_speed_costs, base_speed);
                assert_eq!(cell.overlay_zone_type, None);
                assert!(!cell.overlay_blocks);
                assert_eq!(cell.zone_type, zone_class::GROUND);
            }
        }
    }

    /// Round-trip on tiberium overlay add/remove: a fresh-spread or TIBTRE-spawned
    /// ore cell must inherit Tiberium-mode `land_type` / `speed_costs`, and a harvested-to-zero ore cell must revert to the
    /// underlying terrain values. Regression test for the §10 parity bug where
    /// runtime ore placement bypassed RecalcAttributes-equivalent logic.
    #[test]
    fn gsi_04_04_tiberium_overlay_round_trip_updates_terrain_metadata() {
        use crate::map::resolved_terrain::{ResolvedTerrainCell, zone_class};
        use crate::rules::ini_parser::IniFile;
        use crate::rules::terrain_rules::LandType;
        use crate::rules::terrain_rules::SpeedCostProfile;

        // Registry with overlay id=0 marked Tiberium=yes (mirrors stock TIB01).
        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=TIB01\n[Tiberium]\nTrack=70%\nFoot=90%\n[TIB01]\nTiberium=yes\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);

        // A single Clear-base cell at (5, 5). Underlying values mirror what
        // `resolved_terrain::build()` would produce on a clear-grass tile.
        let clear_lt = LandType::Clear.as_index();
        let base_speed = SpeedCostProfile::default();
        let mut terrain =
            crate::map::resolved_terrain::test_grid(10, 10, |rx, ry| ResolvedTerrainCell {
                filled_clear: true,
                tileset_index: None,
                land_type: clear_lt,
                speed_costs: base_speed,
                accepts_smudge: true,
                zone_type: zone_class::GROUND,
                base_land_type: clear_lt,
                base_speed_costs: base_speed,
                ..crate::map::resolved_terrain::test_flat_cell(rx, ry)
            });

        // Install a distinct Tiberium-mode speed profile so we can prove the
        // round-trip actually copies it (not the same default).
        let tib_speed = SpeedCostProfile {
            foot: Some(90),
            track: Some(70),
            wheel: Some(100),
            float: Some(100),
            amphibious: Some(100),
            float_beach: Some(100),
            hover: Some(100),
        };
        let mut overlay_grid = OverlayGrid::new(10, 10);
        let tib_lt = LandType::Tiberium.as_index();

        // 1. Place ore overlay → recalc must flip cell to Tiberium-mode.
        overlay_grid.place_overlay(5, 5, 0, 3);
        let changed = recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 5, 5);
        assert!(changed, "tiberium placement must report a change");
        let idx = 5 * 10 + 5;
        let cell = &terrain.cells()[idx];
        assert_eq!(cell.land_type, tib_lt, "land_type → Tiberium");
        assert_eq!(cell.land_type, tib_lt, "land_type → 5");
        assert_eq!(
            cell.speed_costs, tib_speed,
            "speed_costs sourced from [Tiberium]"
        );
        assert_eq!(cell.zone_type, zone_class::GROUND);
        assert!(!cell.overlay_blocks);

        // 2. Remove ore overlay (harvested to zero) → recalc must restore base values.
        overlay_grid.clear_overlay(5, 5);
        let changed = recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 5, 5);
        assert!(changed, "tiberium removal must report a change");
        let cell = &terrain.cells()[idx];
        assert_eq!(cell.land_type, clear_lt, "land_type → underlying Clear");
        assert_eq!(cell.land_type, clear_lt);
        assert_eq!(
            cell.speed_costs, base_speed,
            "speed_costs → underlying default"
        );
        assert_eq!(cell.zone_type, zone_class::GROUND);
        assert!(!cell.overlay_blocks);
    }
}
