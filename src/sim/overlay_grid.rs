//! Per-cell mutable overlay state — runtime fork of the immutable map OverlayPack.
//!
//! Mirrors CellClass +0x44 (OverlayTypeIndex) and +0x11E (OverlayData) from gamemd.exe.
//! Seeded from map data at init, mutated during gameplay by ore growth, wall damage,
//! and bridge overlay damage.
//!
//! Dependency rules: depends on map/overlay (OverlayEntry for seeding).
//! Never depends on render/, ui/, sidebar/, audio/, net/.

use crate::map::authored_overlay::{FinalizedOverlayCell, FinalizedOverlayPayload};
use crate::map::overlay::{OverlayDataPack, OverlayEntry};
use crate::map::overlay_types::{
    OverlayTypeRegistry, clears_tiberium_on_slope, is_bridge_overlay_index,
};
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::sim::intern::InternedId;
use crate::util::lepton::{LEPTONS_PER_LEVEL, ground_height_leptons};
use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};
use std::collections::BTreeSet;

const MARK_MAX_SLOPE: u8 = 4;
const MARK_STEEP_SLOPE_EXCEPTION_ID: u8 = 0xB2;

/// Universal pre-stamp slope gate shared by native-style Mark entry points.
fn mark_rejects_steep_slope(slope_type: u8, overlay_id: u8) -> bool {
    slope_type > MARK_MAX_SLOPE && overlay_id != MARK_STEEP_SLOPE_EXCEPTION_ID
}

/// `FUN_005F6360`'s x87/LUT/ftol 3-D distance sequence.
fn native_wall_owner_distance(dx: i32, dy: i32, dz: i32) -> i32 {
    let x = X87Chop53::load_i32(dx);
    let y = X87Chop53::load_i32(dy);
    let z = X87Chop53::load_i32(dz);
    let squared = X87Chop53::add(
        X87Chop53::add(X87Chop53::mul(x, x), X87Chop53::mul(z, z)),
        X87Chop53::mul(y, y),
    );
    let root_bits =
        sqrt_approx_f32(squared).expect("map-space squared distance stays in finite f32 range");
    let root =
        X87Chop53::load_f32(root_bits).expect("Sqrt_Approx always returns a finite normal or zero");
    X87Chop53::ftol_i64(root).expect("map-space distance fits a signed integer") as i32
}

/// Building facts consumed by the one-shot map-wall owner reconstruction pass.
/// Callers preserve BuildingClass allocation order in this slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapWallOwnerCandidate {
    pub owner: InternedId,
    pub world_x: i32,
    pub world_y: i32,
    pub world_z: i32,
    pub foundation_width: u16,
    pub foundation_height: u16,
    pub object_alive: bool,
    pub cell_marked: bool,
    pub house_wall_owner: bool,
}

/// Per-cell mutable overlay state — mirrors CellClass +0x44 / +0x11E.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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
}

impl Default for OverlayCell {
    fn default() -> Self {
        Self {
            overlay_id: None,
            overlay_data: 0,
            wall_owner: None,
        }
    }
}

/// Mutable overlay state grid — seeded from map [OverlayPack] at init,
/// mutated during gameplay by ore growth, wall damage, bridge damage.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OverlayGrid {
    width: u16,
    height: u16,
    cells: Vec<OverlayCell>,
    /// Authoritative wall-only contribution to CellClass+0x122. `Some`, even
    /// when all zero, means the finalized authored plane was retained and final
    /// wall identities must never be scanned as a substitute. `None` is the
    /// temporary legacy-constructor compatibility mode.
    #[serde(default)]
    retained_wall_neighbor_counts: Option<Vec<u8>>,
    /// Cells mutated this tick — drained by Simulation's end-of-frame finalizer
    /// before the authoritative hash. Not part of game state; never serialized.
    #[serde(skip, default)]
    dirty_cells: Vec<(u16, u16)>,
    /// Bumped by every mutator that can change an overlay identity or the
    /// retained wall-neighbour plane (the movement blocker plane keys on it).
    /// Not every internal write bumps it; the public and wall-transaction entry
    /// points do. Not game state; never serialized.
    #[serde(skip)]
    mutation_epoch: u64,
    /// Bumped only when the retained wall-neighbour plane itself changes. The
    /// movement blocker plane reads nothing else from a grid that retains one,
    /// so ore growth and harvesting, which move `mutation_epoch` most frames,
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
    /// A synchronous sim-side recalc already observed a passability change for
    /// one of the dirty cells. The frame finalizer must preserve that first result
    /// even though recalculating the now-current terrain returns `false`.
    #[serde(skip, default)]
    synchronous_passability_changed: bool,
    /// Coordinates whose synchronous overlay recalc changed movement authority.
    /// Drained by `World` before post-combat order readers rebuild paths/zones.
    #[serde(skip, default)]
    synchronous_navigation_cells: Vec<(u16, u16)>,
}

/// Which existing world-reader boundary needs the synchronous projection.
/// VERA-internal delivery policy, gamemd equivalent UNCHECKED. Native callback
/// order remains with the mutation owner; these receipts do not run callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NavigationPublication {
    /// The owner already publishes inline, or its first reader is the frame tail.
    FrameBoundary,
    /// Publish the changed cell before the next path/zone reader as well.
    NextPathReader,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OverlayRecalcOutcome {
    pub navigation_changed: bool,
    pub zone_changed: bool,
}

/// `OverlayClass::Mark @ 0x005FC570`'s wall tail (`0x005FC758..0x005FC775`):
/// eight `MapCoord_StepByDir_GetCell @ 0x00481810` steps over
/// `g_DirectionOffsets` in N, NE, E, SE, S, SW, W, NW order, each
/// wrapping-incrementing the byte at `CellClass+0x122` (`INC DL`). The anchor
/// itself is never incremented.
///
/// `MapClass::Get_CellClass @ 0x005657A0` returns the shared dummy both for an
/// index outside the cell array and for a NULL pointer-table slot, so a
/// neighbour inside the storage rectangle but outside the allocated playfield
/// also lands on the dummy, whose byte no real cell reads. Resolution
/// therefore goes through the same allocation-aware lookup the runtime wall
/// path uses, not a rectangle test.
fn increment_wall_neighbor_plane(
    plane: &mut [u8],
    width: u16,
    height: u16,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
) {
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
    for (dx, dy) in ADJACENT_8 {
        let target = native_runtime_overlay_cell_lookup(
            width,
            height,
            resolved_terrain,
            i32::from(rx) + dx,
            i32::from(ry) + dy,
        );
        let Some(NativeRuntimeOverlayCell::Real(nx, ny)) = target else {
            continue;
        };
        let Some(index) = index_of(width, height, nx, ny) else {
            continue;
        };
        plane[index] = plane[index].wrapping_add(1);
    }
}

impl OverlayGrid {
    /// Create an empty grid with no overlays.
    pub fn new(width: u16, height: u16) -> Self {
        let count = width as usize * height as usize;
        Self {
            width,
            height,
            cells: vec![OverlayCell::default(); count],
            retained_wall_neighbor_counts: None,
            dirty_cells: Vec::new(),
            mutation_epoch: 0,
            wall_plane_epoch: 0,
            synchronous_passability_changed: false,
            removed_render_cells: Vec::new(),
            synchronous_navigation_cells: Vec::new(),
        }
    }

    /// Test-only stand-in for a production map-authority grid: an empty grid
    /// that already retains its `CellClass+0x122` wall plane, as both
    /// production constructors do. Fixtures that cross the snapshot
    /// map-authority restore need this rather than the legacy `new`.
    #[cfg(test)]
    pub(crate) fn new_with_retained_wall_plane(width: u16, height: u16) -> Self {
        let mut grid = Self::new(width, height);
        grid.retained_wall_neighbor_counts =
            Some(vec![0u8; usize::from(width) * usize::from(height)]);
        grid
    }

    /// Attach an all-zero `CellClass+0x122` wall plane to a fixture grid that
    /// was built through a legacy constructor but has to cross the snapshot
    /// map-authority restore, which every production grid now satisfies.
    #[cfg(test)]
    pub(crate) fn retain_zero_wall_plane_for_tests(&mut self) {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        self.wall_plane_epoch = self.wall_plane_epoch.wrapping_add(1);
        self.retained_wall_neighbor_counts = Some(vec![0u8; self.cells.len()]);
    }

    /// Consume the one finalized map payload. This boundary has no raw-pack,
    /// rules, source-filter, RNG, Mark, or Recalc capability.
    pub(crate) fn from_finalized_map_payload(payload: FinalizedOverlayPayload) -> Self {
        let (width, height, finalized, retained_wall_neighbor_counts) = payload.into_parts();
        let expected = usize::from(width) * usize::from(height);
        assert_eq!(finalized.len(), expected, "finalized overlay cell shape");
        assert_eq!(
            retained_wall_neighbor_counts.len(),
            expected,
            "finalized wall-neighbor plane shape"
        );
        let cells = finalized
            .into_iter()
            .map(|cell| OverlayCell {
                overlay_id: cell.overlay_id(),
                overlay_data: cell.state(),
                wall_owner: None,
            })
            .collect();
        Self {
            width,
            height,
            cells,
            retained_wall_neighbor_counts: Some(retained_wall_neighbor_counts),
            dirty_cells: Vec::new(),
            mutation_epoch: 0,
            wall_plane_epoch: 0,
            synchronous_passability_changed: false,
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

    /// Seed from parsed map overlay entries.
    ///
    /// Bridge overlays are intentionally excluded: bridge body/bridgehead
    /// overlay bytes are owned by `BridgeRuntimeState`, while this grid owns
    /// mutable non-bridge overlay bytes such as ore and walls.
    pub fn from_overlay_entries(entries: &[OverlayEntry], width: u16, height: u16) -> Self {
        let mut grid = Self::new(width, height);
        for entry in entries {
            if is_bridge_overlay_index(entry.overlay_id) {
                continue;
            }
            if let Some(idx) = index_of(width, height, entry.rx, entry.ry) {
                grid.cells[idx] = OverlayCell {
                    overlay_id: Some(entry.overlay_id),
                    overlay_data: entry.frame,
                    wall_owner: None,
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
    #[cfg(test)]
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

    /// Native map-pack initialization boundary used after art and mode resolve.
    ///
    /// `ScenarioClass::Full_Init` keeps its placement-suppression counter
    /// nonzero through this reader and does not read `[Terrain]` until after it.
    /// Consequently pass one deliberately does not apply ordinary runtime
    /// CheckCellPassability, the TerrainClass ground-list scan, or Overrides.
    /// The freshly allocated one-byte-per-cell pack also cannot carry a prior
    /// overlay identity into the pass.
    pub fn from_native_overlay_packs(
        entries: &[OverlayEntry],
        data: &OverlayDataPack,
        terrain: &mut ResolvedTerrainGrid,
        registry: &OverlayTypeRegistry,
        shp_available: &BTreeSet<u8>,
        game_mode_nonzero: bool,
    ) -> Self {
        let width = terrain.width();
        let height = terrain.height();
        let mut grid = Self::new(width, height);
        // `OverlayClass::Mark`'s wall tail increments `CellClass+0x122` on the
        // anchor's eight neighbours, and nothing rescans final wall identities
        // afterwards. This pass owns that authority at the map-pack boundary,
        // so it starts the plane at zero and increments per accepted wall
        // stamp instead of leaving the legacy `None` compatibility mode. The
        // retail random-map generator emits only tiberium, low-bridge deck and
        // rock overlay indices, so a generated launch keeps an all-zero plane.
        // Only the `+0x122` tail is modelled here: the wall branch's own
        // acceptance gate, the constructor-side gate that decides whether
        // `ObjectClass::Unlimbo` runs at all, `PostDestructionWallCleanup`, the
        // zone merge/rebuild pair and the `Cell+0x50` write are not, so this
        // boundary is a partial Mark that owns the counts and nothing else.
        let mut wall_neighbor_counts = vec![0u8; usize::from(width) * usize::from(height)];

        // The identity pass is deliberately stricter than raw/internal setup.
        // Each accepted stamp completes its attribute recalculation before the
        // data pass begins. Rejected identities still restore the pristine
        // terrain that the earlier raw map materialization may have decorated.
        for entry in entries {
            let Some(slope_type) = terrain.cell(entry.rx, entry.ry).map(|cell| cell.slope_type)
            else {
                continue;
            };
            let accepted_flags = registry.flags(entry.overlay_id).filter(|flags| {
                (shp_available.contains(&entry.overlay_id) || flags.cell_anim.is_some())
                    && !(game_mode_nonzero && flags.crate_type)
            });
            if let Some(flags) = accepted_flags
                && !mark_rejects_steep_slope(slope_type, entry.overlay_id)
            {
                let idx = entry.ry as usize * width as usize + entry.rx as usize;
                grid.cells[idx] = OverlayCell {
                    overlay_id: Some(entry.overlay_id),
                    overlay_data: if flags.crate_type { u8::MAX } else { 0 },
                    wall_owner: None,
                };
                if flags.wall {
                    increment_wall_neighbor_plane(
                        &mut wall_neighbor_counts,
                        width,
                        height,
                        Some(terrain),
                        entry.rx,
                        entry.ry,
                    );
                }
            }
            recalc_overlay_passability(&mut grid, terrain, registry, entry.rx, entry.ry);
        }

        // Native's data pass has no identity or bridge exception. Rejected and
        // identity-empty allocated cells still retain their raw data byte.
        if data.is_present() {
            for ry in 0..height {
                for rx in 0..width {
                    if terrain.index(rx, ry).is_none() {
                        continue;
                    }
                    let idx = ry as usize * width as usize + rx as usize;
                    grid.cells[idx].overlay_data = data.byte_at(rx, ry);
                }
            }
        }
        grid.retained_wall_neighbor_counts = Some(wall_neighbor_counts);
        grid
    }

    /// Read cell at (rx, ry). Returns default (no overlay) for out-of-bounds.
    pub fn cell(&self, rx: u16, ry: u16) -> &OverlayCell {
        match index_of(self.width, self.height, rx, ry) {
            Some(idx) => &self.cells[idx],
            None => &DEFAULT_CELL,
        }
    }

    /// Mutable access to cell. Panics if out-of-bounds.
    /// Epoch of mutating entry points; see the field note.
    pub fn mutation_epoch(&self) -> u64 {
        self.mutation_epoch
    }

    pub fn cell_mut(&mut self, rx: u16, ry: u16) -> &mut OverlayCell {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        let idx =
            index_of(self.width, self.height, rx, ry).expect("OverlayGrid::cell_mut out of bounds");
        &mut self.cells[idx]
    }

    /// Update only CellClass's bridge-overlay identity.
    ///
    /// Bridge damage/repair owns neither OverlayData nor wall ownership, and
    /// it performs its terrain recalc synchronously in the world cascade. This
    /// deliberately avoids the ordinary dirty-cell channel used by independent
    /// runtime overlay placement/removal.
    pub(crate) fn write_bridge_overlay_identity(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_byte: u8,
    ) -> bool {
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
    /// current: the wall plane's own when one is retained, else (legacy
    /// constructors, which scan wall identities instead) every mutation.
    pub(crate) fn blocker_plane_epoch(&self) -> (bool, u64) {
        match self.retained_wall_neighbor_counts {
            Some(_) => (true, self.wall_plane_epoch),
            None => (false, self.mutation_epoch),
        }
    }

    /// Read the retained wall contribution plane. `Some(all-zero)` is
    /// authoritative and must not fall back to a final-identity scan.
    pub(crate) fn retained_wall_neighbor_counts(&self) -> Option<&[u8]> {
        self.retained_wall_neighbor_counts.as_deref()
    }

    pub(crate) fn retained_wall_neighbor_count_storage_len(&self) -> Option<usize> {
        self.retained_wall_neighbor_counts.as_ref().map(Vec::len)
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
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        self.wall_plane_epoch = self.wall_plane_epoch.wrapping_add(1);
        // Native wall lifecycle evidence: OverlayClass::Mark increments at
        // 0x005FC762..0x005FC775; DestroyOverlay decrements at
        // 0x00481070..0x00481082; cleanup auto-removal's conditional decrement
        // uses the zone comparison returned by recalculate_runtime_cell below.
        let (width, height) = (self.width, self.height);
        let Some(counts) = self.retained_wall_neighbor_counts.as_mut() else {
            return;
        };
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
        for (dx, dy) in ADJACENT_8 {
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
                // Native still performed the lookup and stamped the process
                // dummy. The retained Rust count plane intentionally exports
                // only allocated real CellClass storage.
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

    pub(crate) fn add_retained_wall_neighbor_source(
        &mut self,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        rx: u16,
        ry: u16,
    ) {
        self.adjust_retained_wall_neighbor_source(resolved_terrain, rx, ry, true);
    }

    pub(crate) fn remove_retained_wall_neighbor_source(
        &mut self,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        rx: u16,
        ry: u16,
    ) {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
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
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        let idx = index_of(self.width, self.height, rx, ry)?;
        let prev = self.cells[idx].overlay_id;
        self.cells[idx] = OverlayCell::default();
        self.dirty_cells.push((rx, ry));
        prev
    }

    /// Place overlay at cell.
    pub fn place_overlay(&mut self, rx: u16, ry: u16, overlay_id: u8, data: u8) {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        if let Some(idx) = index_of(self.width, self.height, rx, ry) {
            self.cells[idx] = OverlayCell {
                overlay_id: Some(overlay_id),
                overlay_data: data,
                wall_owner: None,
            };
            self.dirty_cells.push((rx, ry));
        }
    }

    /// Write the two literal CellClass overlay fields without running the
    /// common `RecalcAttributes` tail yet. Ordinary Mark needs this split so
    /// Road germination, the Crate-data override, and CellAnim construction
    /// occur in their native order before the tail recalc.
    pub(crate) fn write_crate_mark_fields(
        &mut self,
        resolved_terrain: &mut ResolvedTerrainGrid,
        registry: &OverlayTypeRegistry,
        rx: u16,
        ry: u16,
        overlay_id: u8,
        overlay_data: u8,
    ) -> bool {
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        if registry.flags(overlay_id).is_none() {
            return false;
        }
        let Some(name) = registry.name(overlay_id) else {
            return false;
        };
        self.cells[idx].overlay_id = Some(overlay_id);
        self.cells[idx].overlay_data = overlay_data;
        self.dirty_cells.push((rx, ry));
        resolved_terrain.set_runtime_overlay_bridge_identity(
            rx,
            ry,
            overlay_id,
            overlay_data,
            name,
        );
        true
    }

    /// Erase the two literal CellClass overlay fields a crate removal clears.
    ///
    /// `CrateSlot__RemoveCrateOverlayFromCell @ 0x004A1AA0` writes
    /// `CellClass+0x44 = -1` and `CellClass+0x11E = 0` after its screen-dirty
    /// request and changes no other field, so this deliberately keeps
    /// `wall_owner` and runs no passability recalc of its own — unlike
    /// [`OverlayGrid::clear_overlay`], which resets the whole cell.
    pub(crate) fn clear_crate_mark_fields(
        &mut self,
        resolved_terrain: &mut ResolvedTerrainGrid,
        rx: u16,
        ry: u16,
    ) -> bool {
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        self.cells[idx].overlay_id = None;
        self.cells[idx].overlay_data = 0;
        self.removed_render_cells.push((rx, ry));
        resolved_terrain.clear_runtime_overlay_identity(rx, ry);
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
        resolved_terrain: &mut ResolvedTerrainGrid,
        rx: u16,
        ry: u16,
        overlay_data: u8,
    ) -> bool {
        if !self.write_literal_bridge_state(resolved_terrain, rx, ry, overlay_data) {
            return false;
        }
        self.dirty_cells.push((rx, ry));
        true
    }

    /// Literal47E040 +11E store, including cells without an overlay. Its
    /// caller owns the later fallout/radar sequence; no Recalc receipt here.
    pub(crate) fn write_literal_bridge_state(
        &mut self,
        resolved_terrain: &mut ResolvedTerrainGrid,
        rx: u16,
        ry: u16,
        overlay_data: u8,
    ) -> bool {
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        if !resolved_terrain.set_runtime_overlay_bridge_state_byte(rx, ry, overlay_data) {
            return false;
        }
        self.cells[idx].overlay_data = overlay_data;
        true
    }

    /// Literal576BA0 +44=-1 after the separately published +11E store. Keep
    /// the state, wall owner and retained attributes; only presentation drops
    /// the removed identity. This must not enqueue a frame-tail Recalc.
    pub(crate) fn clear_literal_bridge_identity(&mut self, rx: u16, ry: u16) -> bool {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        let Some(idx) = index_of(self.width, self.height, rx, ry) else {
            return false;
        };
        self.cells[idx].overlay_id = None;
        self.removed_render_cells.push((rx, ry));
        true
    }

    /// Pending overlay/radar dirty coordinates without consuming the runtime
    /// receipt. Initial presentation uses this to include every real cell
    /// written by a multi-cell startup Mark transaction.
    pub(crate) fn pending_dirty_cells(&self) -> &[(u16, u16)] {
        &self.dirty_cells
    }

    /// Reconstruct owners for map-loaded wall overlays after buildings exist.
    /// Strict distance improvement preserves the first candidate on ties.
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
                let wall_x = i32::from(rx).wrapping_mul(256).wrapping_add(128);
                let wall_y = i32::from(ry).wrapping_mul(256).wrapping_add(128);
                let wall_z = ground_height_leptons(cell.level, cell.slope_type, wall_x, wall_y)
                    .unwrap_or(i32::from(cell.level) * LEPTONS_PER_LEVEL as i32);
                let mut best_distance = i64::MAX;
                let mut best_owner = None;
                for building in buildings {
                    if !building.object_alive || !building.cell_marked || !building.house_wall_owner
                    {
                        continue;
                    }
                    let dx = wall_x.wrapping_sub(building.world_x);
                    let dy = wall_y.wrapping_sub(building.world_y);
                    let dz = wall_z.wrapping_sub(building.world_z);
                    let raw = i64::from(native_wall_owner_distance(dx, dy, dz));
                    let adjustment =
                        64 * i64::from(building.foundation_width + building.foundation_height);
                    let adjusted = raw.saturating_sub(adjustment).max(0);
                    if adjusted < best_distance {
                        best_distance = adjusted;
                        best_owner = Some(building.owner);
                    }
                }
                self.cells[idx].wall_owner = best_owner;
            }
        }
    }

    /// Stamp a player-owned wall overlay at a cell.
    #[cfg(test)]
    pub fn place_owned_wall(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_id: u8,
        data: u8,
        owner: InternedId,
    ) {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        self.stamp_wall_identity(rx, ry, overlay_id, data);
        self.set_wall_owner(rx, ry, owner);
    }

    /// Stamp runtime wall identity/data before native cleanup. OverlayClass::Mark
    /// does not make its pending House visible until cleanup and the explicit
    /// anchor Merge/graph step have completed.
    pub(crate) fn stamp_wall_identity(&mut self, rx: u16, ry: u16, overlay_id: u8, data: u8) {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        if let Some(idx) = index_of(self.width, self.height, rx, ry) {
            self.cells[idx] = OverlayCell {
                overlay_id: Some(overlay_id),
                overlay_data: data,
                wall_owner: None,
            };
            self.dirty_cells.push((rx, ry));
        }
    }

    pub(crate) fn set_wall_owner(&mut self, rx: u16, ry: u16, owner: InternedId) {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        if let Some(idx) = index_of(self.width, self.height, rx, ry) {
            self.cells[idx].wall_owner = Some(owner);
        }
    }

    /// Clear type/data while retaining the wall owner.
    fn clear_overlay_preserving_owner(&mut self, rx: u16, ry: u16) -> Option<u8> {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
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
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        if let Some(idx) = index_of(self.width, self.height, rx, ry) {
            if self.cells[idx].overlay_id.is_some() {
                self.cells[idx].overlay_data = data;
                self.dirty_cells.push((rx, ry));
            }
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

    /// Drain the list of cells mutated since last call. Simulation's frame
    /// finalizer recalculates passability and may trigger a navigation rebuild.
    ///
    /// Drained at the first frame boundary with rules, resolved terrain, and an
    /// overlay registry. Partial-input frames retain it so derived terrain and
    /// navigation cannot miss the mutation.
    #[cfg(test)]
    pub fn take_dirty_cells(&mut self) -> Vec<(u16, u16)> {
        self.take_dirty_cells_with_passability_signal().0
    }

    /// Recalculate one runtime mutation and retain its delivery obligations.
    /// A later unchanged projection cannot erase an earlier change. The
    /// next-reader receipt preserves first-seen order independently of the
    /// presentation dirty list and the frame signal.
    ///
    /// Zone comparison serves ordered wall cleanup: CellClass cleanup
    /// @ 0x00480630 runs Recalc @ 0x00480969, compares old/new zone at
    /// 0x0048096E..0x00480977, then repairs the graph before its neighbor-count
    /// tail. The caller still owns those callbacks in their native slots.
    pub(crate) fn recalculate_runtime_cell(
        &mut self,
        terrain: &mut ResolvedTerrainGrid,
        registry: &OverlayTypeRegistry,
        cell: (u16, u16),
        publication: NavigationPublication,
    ) -> OverlayRecalcOutcome {
        self.mutation_epoch = self.mutation_epoch.wrapping_add(1);
        let old_zone = terrain.cell(cell.0, cell.1).map(|cell| cell.zone_type);
        let navigation_changed =
            recalc_overlay_passability(self, terrain, registry, cell.0, cell.1);
        let zone_changed = old_zone
            .zip(terrain.cell(cell.0, cell.1).map(|cell| cell.zone_type))
            .is_some_and(|(old, new)| old != new);
        self.synchronous_passability_changed |= navigation_changed;
        if navigation_changed
            && publication == NavigationPublication::NextPathReader
            && !self.synchronous_navigation_cells.contains(&cell)
        {
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

    /// Drain overlay dirtiness and any already-observed passability change as
    /// one runtime-only result. Neither component is serialized or hashed.
    pub(crate) fn take_dirty_cells_with_passability_signal(&mut self) -> (Vec<(u16, u16)>, bool) {
        (
            std::mem::take(&mut self.dirty_cells),
            std::mem::take(&mut self.synchronous_passability_changed),
        )
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
pub(crate) fn recalc_overlay_passability(
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
        *overlay_grid.cell_mut(rx, ry) = OverlayCell::default();
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

/// A request to damage a wall overlay at a specific cell.
///
/// Native `DestroyOverlay` receives the weapon's raw signed damage. Literal
/// `-1` is the forced-destruction sentinel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WallDamageEvent {
    pub rx: u16,
    pub ry: u16,
    pub damage: i32,
}

/// Ordered writes produced by one `DestroyOverlay` call, including its inline
/// recursive cardinal chain and fixed post-destruction cleanup fan-out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallMutationKind {
    DirectUpdated,
    DirectRemoved,
    CleanupUpdated,
    CleanupRemoved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WallMutation {
    pub rx: u16,
    pub ry: u16,
    pub kind: WallMutationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WallZoneRepairKind {
    AssignOrphaned,
    MergeAdjacent,
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
        cell: (u16, u16),
        navigation_changed: bool,
        repair: WallZoneRepairKind,
    );

    fn pointer_expired(&mut self, target: WallPointerTarget);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativeRuntimeOverlayCell {
    Real(u16, u16),
    Dummy,
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
    #[cfg(test)]
    pub mutations: Vec<WallMutation>,
    /// Diagnostic trace of packed CellStruct values submitted to
    /// MarkTerrainDirty, deduplicated in first-call order. Production mutation
    /// authority is the synchronous host callback, never replay of this list.
    /// True-dummy coordinates retain their raw signed-word bit patterns.
    #[cfg(test)]
    pub radar_dirty_cells: Vec<(u16, u16)>,
}

#[cfg(test)]
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
    #[cfg(test)]
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
    damage_wall_overlay_with_terrain(overlay_grid, registry, None, rx, ry, damage, rng)
}

/// Runtime-authoritative wall damage. Finalized grids require the resolved
/// CellClass lookup state so each terminal removal reverses exactly one
/// wrapping eight-neighbor wall contribution, including fixed-stride aliases.
#[cfg(test)]
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
        rx,
        ry,
        damage,
        rng,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn damage_wall_overlay_with_runtime_host(
    overlay_grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    mut resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
    damage: i32,
    rng: &mut crate::sim::rng::SimRng,
    mut host: Option<&mut dyn WallDamageTransactionHost>,
) -> WallDamageResult {
    assert!(
        overlay_grid.retained_wall_neighbor_counts().is_none() || resolved_terrain.is_some(),
        "retained wall-neighbor authority requires resolved terrain for wall damage"
    );
    let mut result = WallDamageResult::default();
    damage_wall_recursive(
        overlay_grid,
        registry,
        &mut resolved_terrain,
        &mut host,
        NativeRuntimeOverlayCell::Real(rx, ry),
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
            #[cfg(test)]
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
        #[cfg(test)]
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
        let recalc = grid.recalculate_runtime_cell(
            terrain,
            registry,
            (rx, ry),
            if host.is_some() {
                NavigationPublication::FrameBoundary
            } else {
                NavigationPublication::NextPathReader
            },
        );
        if let Some(host) = host.as_deref_mut() {
            host.navigation_step(
                terrain,
                (rx, ry),
                recalc.navigation_changed,
                WallZoneRepairKind::AssignOrphaned,
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
#[cfg(test)]
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
#[cfg(test)]
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
        if native_runtime_overlay_target_state(grid, resolved_terrain, neighbor).0
            == Some(overlay_id)
        {
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
        && let Some(host) = host.as_deref_mut()
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

/// Refresh a newly stamped wall and its four cardinal neighbors only.
pub fn refresh_wall_connectivity_after_placement(
    grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
) {
    refresh_wall_connectivity_after_placement_with_host(
        grid,
        registry,
        resolved_terrain,
        rx,
        ry,
        None,
    );
}

/// Runtime-hosted `PostDestructionWallCleanup(anchor, 1)` used by one wall
/// Mark transaction. Load-time materialization uses the hostless wrapper
/// above because no live tactical/radar/entity/navigation observers exist.
pub(crate) fn refresh_wall_connectivity_after_placement_with_host(
    grid: &mut OverlayGrid,
    registry: &OverlayTypeRegistry,
    mut resolved_terrain: Option<&mut ResolvedTerrainGrid>,
    rx: u16,
    ry: u16,
    mut host: Option<&mut dyn WallDamageTransactionHost>,
) {
    assert!(
        grid.retained_wall_neighbor_counts().is_none() || resolved_terrain.is_some(),
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
            let recalc = grid.recalculate_runtime_cell(
                terrain,
                registry,
                (nx, ny),
                if host.is_some() {
                    NavigationPublication::FrameBoundary
                } else {
                    NavigationPublication::NextPathReader
                },
            );
            if recalc.zone_changed
                && let Some(host) = host.as_deref_mut()
            {
                host.navigation_step(
                    terrain,
                    (nx, ny),
                    recalc.navigation_changed,
                    if result == RecomputeResult::Destroyed {
                        WallZoneRepairKind::AssignOrphaned
                    } else {
                        WallZoneRepairKind::MergeAdjacent
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
#[cfg(test)]
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
                    #[cfg(test)]
                    result.mutations.push(WallMutation {
                        rx: nx,
                        ry: ny,
                        kind: WallMutationKind::CleanupUpdated,
                    });
                }
                RecomputeResult::Destroyed => {
                    result.destroyed_cells.push((nx, ny));
                    #[cfg(test)]
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
                let recalc = grid.recalculate_runtime_cell(
                    terrain,
                    registry,
                    (nx, ny),
                    if host.is_some() {
                        NavigationPublication::FrameBoundary
                    } else {
                        NavigationPublication::NextPathReader
                    },
                );
                if recalc.zone_changed
                    && let Some(host) = host.as_deref_mut()
                {
                    host.navigation_step(
                        terrain,
                        (nx, ny),
                        recalc.navigation_changed,
                        if recomputed == RecomputeResult::Destroyed {
                            WallZoneRepairKind::AssignOrphaned
                        } else {
                            WallZoneRepairKind::MergeAdjacent
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
#[cfg(test)]
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
};

fn index_of(width: u16, height: u16, rx: u16, ry: u16) -> Option<usize> {
    (rx < width && ry < height).then_some(ry as usize * width as usize + rx as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn retained_wall_plane_for_sources(
        terrain: &ResolvedTerrainGrid,
        sources: &[(u16, u16)],
    ) -> Vec<u8> {
        let mut counts = vec![0u8; terrain.cells.len()];
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
        assert_eq!(grid.retained_wall_neighbor_counts(), Some(&[5, 8][..]));
    }

    #[test]
    fn retained_wall_neighbor_plane_roundtrips_with_overlay_authority() {
        let grid =
            OverlayGrid::from_finalized_map_payload(FinalizedOverlayPayload::from_cells_for_test(
                2,
                2,
                vec![(-1, 0), (1, 2), (-1, 0), (-1, 0)],
                vec![0, 7, 255, 3],
            ));
        let bytes = bincode::serialize(&grid).expect("serialize overlay authority");
        let restored: OverlayGrid =
            bincode::deserialize(&bytes).expect("deserialize overlay authority");

        assert_eq!(
            restored.retained_wall_neighbor_counts(),
            Some(&[0, 7, 255, 3][..])
        );
        assert_eq!(restored.cell(1, 0), grid.cell(1, 0));
    }

    #[test]
    fn new_grid_is_empty() {
        let grid = OverlayGrid::new(4, 4);
        assert_eq!(grid.cell(0, 0).overlay_id, None);
        assert_eq!(grid.cell(3, 3).overlay_id, None);
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
    fn from_overlay_entries_seeds_cells() {
        let entries = vec![
            OverlayEntry {
                rx: 1,
                ry: 2,
                overlay_id: 5,
                frame: 7,
            },
            OverlayEntry {
                rx: 3,
                ry: 0,
                overlay_id: 10,
                frame: 0,
            },
        ];
        let grid = OverlayGrid::from_overlay_entries(&entries, 4, 4);
        assert_eq!(grid.cell(1, 2).overlay_id, Some(5));
        assert_eq!(grid.cell(1, 2).overlay_data, 7);
        assert_eq!(grid.cell(3, 0).overlay_id, Some(10));
        assert_eq!(grid.cell(0, 0).overlay_id, None);
    }

    #[test]
    fn from_overlay_entries_skips_bridge_overlay_bytes() {
        let entries = vec![
            OverlayEntry {
                rx: 1,
                ry: 1,
                overlay_id: 24,
                frame: 3,
            },
            OverlayEntry {
                rx: 2,
                ry: 1,
                overlay_id: 5,
                frame: 7,
            },
        ];

        let grid = OverlayGrid::from_overlay_entries(&entries, 4, 4);

        assert_eq!(
            grid.cell(1, 1).overlay_id,
            None,
            "bridge overlay byte is owned by BridgeRuntimeState"
        );
        assert_eq!(grid.cell(2, 1).overlay_id, Some(5));
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
    fn place_and_clear_overlay() {
        let mut grid = OverlayGrid::new(4, 4);
        grid.place_overlay(2, 2, 42, 11);
        assert_eq!(grid.cell(2, 2).overlay_id, Some(42));
        assert_eq!(grid.cell(2, 2).overlay_data, 11);

        let prev = grid.clear_overlay(2, 2);
        assert_eq!(prev, Some(42));
        assert_eq!(grid.cell(2, 2).overlay_id, None);
    }

    #[test]
    fn set_overlay_data_updates_existing() {
        let mut grid = OverlayGrid::new(4, 4);
        grid.place_overlay(1, 1, 5, 3);
        grid.set_overlay_data(1, 1, 9);
        assert_eq!(grid.cell(1, 1).overlay_data, 9);
        assert_eq!(grid.cell(1, 1).overlay_id, Some(5));
    }

    #[test]
    fn set_overlay_data_noop_on_empty_cell() {
        let mut grid = OverlayGrid::new(4, 4);
        grid.set_overlay_data(1, 1, 9);
        assert_eq!(grid.cell(1, 1).overlay_id, None);
        assert_eq!(grid.cell(1, 1).overlay_data, 0);
    }

    #[test]
    fn bridge_publication_literal_fields_do_not_schedule_attribute_recalc() {
        let mut terrain = single_cell_terrain(0, Default::default(), false, false);
        let mut grid = OverlayGrid::new(1, 1);
        assert!(grid.write_literal_bridge_state(&mut terrain, 0, 0, 15));
        assert_eq!(grid.cell(0, 0).overlay_id, None);
        assert_eq!(grid.cell(0, 0).overlay_data, 15);
        assert_eq!(terrain.cell(0, 0).unwrap().bridge_facts.state_byte, 15);
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
    fn out_of_bounds_returns_default() {
        let grid = OverlayGrid::new(2, 2);
        assert_eq!(grid.cell(5, 5).overlay_id, None);
    }

    #[test]
    fn iter_occupied_skips_empty() {
        let mut grid = OverlayGrid::new(3, 3);
        grid.place_overlay(0, 0, 1, 0);
        grid.place_overlay(2, 2, 2, 5);
        let occupied: Vec<_> = grid.iter_occupied().collect();
        assert_eq!(occupied.len(), 2);
        assert_eq!(occupied[0].0, 0); // rx
        assert_eq!(occupied[0].1, 0); // ry
        assert_eq!(occupied[1].0, 2);
        assert_eq!(occupied[1].1, 2);
    }

    #[test]
    fn place_overlay_pushes_dirty() {
        let mut grid = OverlayGrid::new(10, 10);
        assert!(grid.dirty_cells.is_empty());
        grid.place_overlay(3, 4, 7, 0);
        assert_eq!(grid.dirty_cells, vec![(3, 4)]);
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
    fn clear_overlay_no_push_when_out_of_bounds() {
        let mut grid = OverlayGrid::new(10, 10);
        let prev = grid.clear_overlay(100, 100);
        assert_eq!(prev, None);
        assert!(grid.dirty_cells.is_empty());
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
    fn take_dirty_cells_returns_and_clears() {
        let mut grid = OverlayGrid::new(10, 10);
        grid.place_overlay(0, 0, 1, 0);
        grid.place_overlay(1, 1, 2, 0);
        let drained = grid.take_dirty_cells();
        assert_eq!(drained, vec![(0, 0), (1, 1)]);
        assert!(grid.dirty_cells.is_empty());
        // Second take returns empty.
        assert!(grid.take_dirty_cells().is_empty());
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
        is_water: bool,
        base_ground_walk_blocked: bool,
    ) -> crate::map::resolved_terrain::ResolvedTerrainGrid {
        use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
        use crate::rules::terrain_rules::TerrainClass;

        let terrain_class = if is_water {
            TerrainClass::Water
        } else {
            TerrainClass::Clear
        };
        ResolvedTerrainGrid::from_cells(
            1,
            1,
            vec![ResolvedTerrainCell {
                rx: 0,
                ry: 0,
                source_tile_index: 0,
                source_sub_tile: 0,
                final_tile_index: 0,
                final_sub_tile: 0,
                is_wood_bridge_repair_tile: false,
                level: 0,
                filled_clear: true,
                tileset_index: None,
                land_type: base_land_type,
                yr_cell_land_type: base_land_type,
                slope_type: 0,
                template_height: 0,
                render_offset_x: 0,
                render_offset_y: 0,
                terrain_class,
                speed_costs,
                is_water,
                is_cliff_like: base_ground_walk_blocked,
                is_rough: false,
                is_road: false,
                accepts_smudge: true,
                allows_tiberium: false,
                height_in_pixels: 0,
                variant: 0,
                has_ramp: false,
                canonical_ramp: None,
                ground_walk_blocked: base_ground_walk_blocked,
                terrain_object_blocks: false,
                terrain_object_occupation: None,
                overlay_blocks: false,
                overlay_zone_type: None,
                outside_playfield: false,
                zone_type: zone_class::GROUND,
                base_ground_walk_blocked,
                base_build_blocked: false,
                base_land_type,
                base_yr_cell_land_type: base_land_type,
                base_terrain_class: terrain_class,
                base_speed_costs: speed_costs,
                build_blocked: false,
                has_bridge_deck: false,
                bridge_walkable: false,
                bridge_transition: false,
                bridge_deck_level: 0,
                bridge_layer: None,
                bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                tube_index: None,
                radar_left: [0; 3],
                radar_right: [0; 3],
                has_damaged_data: false,
                bridgehead_anchor_class_at_load: None,
            }],
        )
    }

    /// `Wall=yes` at overlay id 2, with two non-wall neighbours in the list.
    fn map_pack_wall_registry() -> OverlayTypeRegistry {
        let ini = crate::rules::ini_parser::IniFile::from_str(
            "[OverlayTypes]
             0=SAND
             1=ROCK
             2=GAWALL
             [GAWALL]
             Wall=yes
             Strength=400
",
        );
        OverlayTypeRegistry::from_ini(&ini, None)
    }

    /// `OverlayClass::Mark`'s wall tail increments `CellClass+0x122` on the
    /// anchor's eight neighbours, and nothing rescans final wall identities.
    /// The map-pack boundary owns that authority, so it always retains the
    /// plane: all zero when the pack carries no wall (every retail random-map
    /// overlay index), and the eight-neighbour increments when it does.
    #[test]
    fn map_pack_boundary_retains_the_wall_neighbor_plane() {
        let registry = map_pack_wall_registry();
        let mut terrain = clear_terrain_grid(4, 3);
        terrain.test_set_native_allocated_cells(
            &(0..3)
                .flat_map(|y| (0..4).map(move |x| (x, y)))
                .collect::<Vec<_>>(),
        );
        let data = OverlayDataPack::from_cells([(1, 1, 0)]);
        let shp_available = BTreeSet::from([0u8, 1, 2]);

        let no_wall = OverlayGrid::from_native_overlay_packs(
            &[OverlayEntry {
                rx: 1,
                ry: 1,
                overlay_id: 1,
                frame: 0,
            }],
            &data,
            &mut terrain.clone(),
            &registry,
            &shp_available,
            true,
        );
        assert_eq!(
            no_wall.cell(1, 1).overlay_id,
            Some(1),
            "the non-wall overlay was accepted, so the zero plane is not a filter artifact"
        );
        assert_eq!(
            no_wall.retained_wall_neighbor_counts(),
            Some(&[0u8; 12][..]),
            "a stamped non-wall overlay still retains an all-zero plane"
        );

        let walled = OverlayGrid::from_native_overlay_packs(
            &[OverlayEntry {
                rx: 1,
                ry: 1,
                overlay_id: 2,
                frame: 0,
            }],
            &data,
            &mut terrain,
            &registry,
            &shp_available,
            true,
        );
        assert_eq!(walled.cell(1, 1).overlay_id, Some(2));
        let plane = walled
            .retained_wall_neighbor_counts()
            .expect("map-pack boundary retains the plane");
        // Every neighbour of (1,1) took one increment; the anchor took none,
        // and (3, y) is two cells away.
        assert_eq!(
            plane,
            &[1, 1, 1, 0, 1, 0, 1, 0, 1, 1, 1, 0][..],
            "the eight neighbours of the wall anchor each took one increment"
        );
    }

    /// `MapClass::Get_CellClass @ 0x005657A0` returns the shared dummy for a
    /// NULL pointer-table slot as well as an out-of-array index, so a
    /// neighbour inside the storage rectangle but outside the allocated
    /// playfield takes no increment either.
    #[test]
    fn map_pack_wall_plane_skips_unallocated_neighbors() {
        let registry = map_pack_wall_registry();
        let mut terrain = clear_terrain_grid(3, 3);
        // Everything around (1,1) is allocated except its east neighbour.
        terrain.test_set_native_allocated_cells(&[
            (0, 0),
            (1, 0),
            (2, 0),
            (0, 1),
            (1, 1),
            (0, 2),
            (1, 2),
            (2, 2),
        ]);
        let grid = OverlayGrid::from_native_overlay_packs(
            &[OverlayEntry {
                rx: 1,
                ry: 1,
                overlay_id: 2,
                frame: 0,
            }],
            &OverlayDataPack::from_cells([(1, 1, 0)]),
            &mut terrain,
            &registry,
            &BTreeSet::from([2u8]),
            true,
        );
        assert_eq!(
            grid.retained_wall_neighbor_counts(),
            Some(&[1u8, 1, 1, 1, 0, 0, 1, 1, 1][..]),
            "the unallocated east neighbour resolves to the shared dummy"
        );
    }

    /// An anchor on the map edge drops the off-grid steps: native resolves
    /// them to the shared dummy CellClass, whose byte no real cell reads.
    #[test]
    fn map_pack_wall_plane_drops_off_grid_neighbors() {
        let registry = map_pack_wall_registry();
        let mut terrain = clear_terrain_grid(2, 2);
        terrain.test_set_native_allocated_cells(&[(0, 0), (1, 0), (0, 1), (1, 1)]);
        let grid = OverlayGrid::from_native_overlay_packs(
            &[OverlayEntry {
                rx: 0,
                ry: 0,
                overlay_id: 2,
                frame: 0,
            }],
            &OverlayDataPack::from_cells([(0, 0, 0)]),
            &mut terrain,
            &registry,
            &BTreeSet::from([2u8]),
            true,
        );
        assert_eq!(
            grid.retained_wall_neighbor_counts(),
            Some(&[0u8, 1, 1, 1][..]),
            "only the three in-grid neighbours took an increment"
        );
    }

    pub(super) fn clear_terrain_grid(width: u16, height: u16) -> ResolvedTerrainGrid {
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile};

        let mut single = single_cell_terrain(
            LandType::Clear.as_index(),
            SpeedCostProfile::default(),
            false,
            false,
        );
        let template = single.cells.remove(0);
        let mut cells = Vec::with_capacity(width as usize * height as usize);
        for ry in 0..height {
            for rx in 0..width {
                let mut cell = template.clone();
                cell.rx = rx;
                cell.ry = ry;
                cells.push(cell);
            }
        }
        ResolvedTerrainGrid::from_cells(width, height, cells)
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

    fn gsi_04_07_placement_steep_slope_registry() -> OverlayTypeRegistry {
        use crate::rules::ini_parser::IniFile;
        use std::fmt::Write as _;

        let mut ini = String::from("[OverlayTypes]\n");
        for raw_id in 0..=usize::from(MARK_STEEP_SLOPE_EXCEPTION_ID) {
            let name = match raw_id {
                0xAB => "SROCK01".to_string(),
                0xB2 => "TROCK03".to_string(),
                _ => format!("DUMMY_{raw_id:03}"),
            };
            writeln!(ini, "{raw_id}={name}").expect("write overlay registry fixture");
        }
        ini.push_str("[SROCK01]\nIsARock=yes\n[TROCK03]\nIsARock=yes\n");

        let registry = OverlayTypeRegistry::from_ini(&IniFile::from_str(&ini), None);
        assert_eq!(registry.name(0xAB), Some("SROCK01"));
        assert_eq!(registry.name(0xB2), Some("TROCK03"));
        registry
    }

    #[test]
    fn gsi_04_07_placement_map_pack_filters_identity_but_retains_allocated_data() {
        let registry = gsi_04_07_placement_registry();
        assert_eq!(registry.name(0), Some("PROTECTED"));
        assert_eq!(registry.name(1), Some("REPLACEABLE"));
        assert_eq!(registry.name(2), Some("NEWROCK"));

        let mut terrain = clear_terrain_grid(4, 2);
        terrain.test_set_native_allocated_cells(&[(0, 0), (1, 0), (2, 0), (3, 0), (0, 1)]);
        let entries = [
            OverlayEntry {
                rx: 0,
                ry: 0,
                overlay_id: 1,
                frame: 99,
            },
            OverlayEntry {
                rx: 1,
                ry: 0,
                overlay_id: 3,
                frame: 99,
            },
            OverlayEntry {
                rx: 2,
                ry: 0,
                overlay_id: 4,
                frame: 99,
            },
            OverlayEntry {
                rx: 3,
                ry: 0,
                overlay_id: 2,
                frame: 99,
            },
            OverlayEntry {
                rx: 3,
                ry: 1,
                overlay_id: 2,
                frame: 99,
            },
        ];
        let data = OverlayDataPack::from_cells([
            (0, 0, 11),
            (1, 0, 12),
            (2, 0, 13),
            (3, 0, 14),
            (0, 1, 15),
            (3, 1, 16),
        ]);
        let shp_available = BTreeSet::from([2u8]);
        let grid = OverlayGrid::from_native_overlay_packs(
            &entries,
            &data,
            &mut terrain,
            &registry,
            &shp_available,
            true,
        );

        assert_eq!(
            (grid.cell(0, 0).overlay_id, grid.cell(0, 0).overlay_data),
            (None, 11)
        );
        assert_eq!(
            (grid.cell(1, 0).overlay_id, grid.cell(1, 0).overlay_data),
            (Some(3), 12),
            "CellAnim-only overlay is accepted"
        );
        assert_eq!(
            (grid.cell(2, 0).overlay_id, grid.cell(2, 0).overlay_data),
            (None, 13),
            "nonzero game mode rejects Crate identity but not pass-two data"
        );
        assert_eq!(
            (grid.cell(3, 0).overlay_id, grid.cell(3, 0).overlay_data),
            (Some(2), 14)
        );
        assert_eq!(
            (grid.cell(0, 1).overlay_id, grid.cell(0, 1).overlay_data),
            (None, 15)
        );
        assert_eq!(
            (grid.cell(3, 1).overlay_id, grid.cell(3, 1).overlay_data),
            (None, 0),
            "rectangular but native-unallocated cell remains default in both passes"
        );
    }

    #[test]
    fn gsi_04_07_placement_map_crate_mark_sentinel_precedes_raw_data_pass() {
        let registry = gsi_04_07_placement_registry();
        let entries = [OverlayEntry {
            rx: 0,
            ry: 0,
            overlay_id: 4,
            frame: 0,
        }];

        let mut terrain_without_data = clear_terrain_grid(1, 1);
        terrain_without_data.test_set_native_allocated_cells(&[(0, 0)]);
        let mark_only = OverlayGrid::from_native_overlay_packs(
            &entries,
            &OverlayDataPack::missing(),
            &mut terrain_without_data,
            &registry,
            &BTreeSet::from([4]),
            false,
        );
        assert_eq!(mark_only.cell(0, 0).overlay_id, Some(4));
        assert_eq!(
            mark_only.cell(0, 0).overlay_data,
            u8::MAX,
            "Crate=yes writes the Mark-time sentinel before Recalc",
        );

        let mut terrain_with_data = clear_terrain_grid(1, 1);
        terrain_with_data.test_set_native_allocated_cells(&[(0, 0)]);
        let raw_pass = OverlayGrid::from_native_overlay_packs(
            &entries,
            &OverlayDataPack::from_cells([(0, 0, 41)]),
            &mut terrain_with_data,
            &registry,
            &BTreeSet::from([4]),
            false,
        );
        assert_eq!(raw_pass.cell(0, 0).overlay_id, Some(4));
        assert_eq!(
            raw_pass.cell(0, 0).overlay_data,
            41,
            "pass two remains the final raw Cell+0x11E authority",
        );
    }

    #[test]
    fn gsi_04_07_placement_production_init_recalc_precedes_raw_data_pass() {
        let registry = gsi_04_07_placement_registry();
        let mut terrain = clear_terrain_grid(2, 1);
        terrain.cells[0].slope_type = 1;
        terrain.test_set_native_allocated_cells(&[(0, 0), (1, 0)]);
        let entries = [
            OverlayEntry {
                rx: 0,
                ry: 0,
                overlay_id: 6,
                frame: 0,
            },
            OverlayEntry {
                rx: 1,
                ry: 0,
                overlay_id: 6,
                frame: 0,
            },
        ];
        let data = OverlayDataPack::from_cells([(0, 0, 7), (1, 0, 7)]);

        let grid = OverlayGrid::from_native_overlay_packs(
            &entries,
            &data,
            &mut terrain,
            &registry,
            &BTreeSet::from([6]),
            false,
        );

        assert_eq!(
            (grid.cell(0, 0).overlay_id, grid.cell(0, 0).overlay_data),
            (None, 7),
            "Mark-time slope recalculation clears identity before pass two restores raw data",
        );
        assert_eq!(
            (grid.cell(1, 0).overlay_id, grid.cell(1, 0).overlay_data),
            (Some(6), 7),
            "flat accepted Tiberium survives Mark and receives the same raw data byte",
        );
    }

    #[test]
    fn gsi_04_07_placement_map_load_bypasses_runtime_mark_passability_before_terrain_read() {
        let registry = gsi_04_07_placement_registry();
        let mut terrain = clear_terrain_grid(2, 1);
        terrain.cells[0].speed_costs.track = Some(0);
        terrain.cells[0].terrain_object_occupation = Some(0);
        terrain.test_set_native_allocated_cells(&[(0, 0), (1, 0)]);
        let entries = [
            OverlayEntry {
                rx: 0,
                ry: 0,
                overlay_id: 2,
                frame: 0,
            },
            OverlayEntry {
                rx: 1,
                ry: 0,
                overlay_id: 2,
                frame: 0,
            },
        ];
        let data = OverlayDataPack::from_cells([(0, 0, 7), (1, 0, 9)]);

        let grid = OverlayGrid::from_native_overlay_packs(
            &entries,
            &data,
            &mut terrain,
            &registry,
            &BTreeSet::from([2]),
            false,
        );

        assert_eq!(
            (grid.cell(0, 0).overlay_id, grid.cell(0, 0).overlay_data),
            (Some(2), 7),
            "Full_Init keeps the placement-suppression counter nonzero and reads TerrainClass objects only after OverlayPack",
        );
        assert_eq!(
            (grid.cell(1, 0).overlay_id, grid.cell(1, 0).overlay_data),
            (Some(2), 9),
            "ordinary flat control is stamped before the blind data pass",
        );
    }

    #[test]
    fn gsi_04_07_placement_map_mark_rejects_steep_slope_but_retains_raw_data() {
        let registry = gsi_04_07_placement_steep_slope_registry();
        let mut terrain = clear_terrain_grid(2, 1);
        terrain.cells[0].slope_type = 5;
        terrain.cells[1].slope_type = 5;
        terrain.test_set_native_allocated_cells(&[(0, 0), (1, 0)]);
        let entries = [
            OverlayEntry {
                rx: 0,
                ry: 0,
                overlay_id: 0xAB,
                frame: 0,
            },
            OverlayEntry {
                rx: 1,
                ry: 0,
                overlay_id: MARK_STEEP_SLOPE_EXCEPTION_ID,
                frame: 0,
            },
        ];
        let data = OverlayDataPack::from_cells([(0, 0, 23), (1, 0, 29)]);

        let grid = OverlayGrid::from_native_overlay_packs(
            &entries,
            &data,
            &mut terrain,
            &registry,
            &BTreeSet::from([0xAB, MARK_STEEP_SLOPE_EXCEPTION_ID]),
            false,
        );

        assert_eq!(
            (grid.cell(0, 0).overlay_id, grid.cell(0, 0).overlay_data),
            (None, 23),
            "SROCK01 is rejected before stamp, but pass two still writes raw data",
        );
        assert_eq!(
            (grid.cell(1, 0).overlay_id, grid.cell(1, 0).overlay_data),
            (Some(MARK_STEEP_SLOPE_EXCEPTION_ID), 29),
            "raw overlay id 0xB2 is the sole steep-slope Mark exception",
        );
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
            grid.retained_wall_neighbor_counts()
                .expect("retained authority")
                .iter()
                .all(|&count| count == 0),
            "direct and cleanup removals each reverse one retained source"
        );

        let mut unchanged_zone_terrain = clear_terrain_grid(5, 5);
        let cleanup_cell = unchanged_zone_terrain.cell_mut(2, 1).expect("cleanup cell");
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
            unchanged_zone_grid.retained_wall_neighbor_counts(),
            Some(retained_cleanup_plane.as_slice()),
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
        let retained = grid
            .retained_wall_neighbor_counts()
            .expect("retained authority")
            .to_vec();

        assert_eq!(grid.clear_overlay(0, 0), Some(0));
        grid.place_overlay(1, 1, 3, 4);
        assert_eq!(
            grid.retained_wall_neighbor_counts(),
            Some(retained.as_slice()),
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
        let _ = damage_wall_overlay(&mut grid, &registry, 0, 0, -1, &mut rng);
    }

    #[test]
    fn gsi_04_07_placement_map_wall_owner_filters_and_uses_adjusted_distance() {
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
                foundation_width: 1,
                foundation_height: 1,
                object_alive: false,
                cell_marked: true,
                house_wall_owner: true,
            },
            MapWallOwnerCandidate {
                owner: InternedId::from_index(2),
                world_x: wall_center + 500,
                world_y: wall_center,
                world_z: 0,
                foundation_width: 1,
                foundation_height: 1,
                object_alive: true,
                cell_marked: true,
                house_wall_owner: true,
            },
            MapWallOwnerCandidate {
                owner: InternedId::from_index(3),
                world_x: wall_center + 600,
                world_y: wall_center,
                world_z: 0,
                foundation_width: 4,
                foundation_height: 4,
                object_alive: true,
                cell_marked: true,
                house_wall_owner: true,
            },
        ];

        grid.reconstruct_map_wall_owners(&terrain, &registry, &candidates);
        assert_eq!(grid.cell(2, 2).wall_owner, Some(InternedId::from_index(3)));

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
            foundation_width: 1,
            foundation_height: 1,
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
        assert_eq!(native_wall_owner_distance(0, 3968, 1040), 4101);
        assert_eq!(native_wall_owner_distance(0, 4096, 208), 4101);

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
            foundation_width: 1,
            foundation_height: 1,
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
            let mut terrain = single_cell_terrain(clear, SpeedCostProfile::default(), false, false);
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
        let mut water_costs = SpeedCostProfile::default();
        water_costs.wheel = Some(0);

        let mut terrain = single_cell_terrain(LandType::Water.as_index(), water_costs, true, true);
        let mut overlay_grid = OverlayGrid::new(1, 1);
        overlay_grid.place_overlay(0, 0, 0, 1);
        recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);

        let deck = terrain.cell(0, 0).expect("bridge deck cell");
        assert_eq!(deck.land_type, LandType::Road.as_index());
        assert!(!deck.is_water);
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
        *overlay_grid.cell_mut(0, 0) = OverlayCell::default();
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
        let mut zero_wheel = SpeedCostProfile::default();
        zero_wheel.wheel = Some(0);

        let mut water = single_cell_terrain(LandType::Water.as_index(), zero_wheel, true, true);
        recalc_overlay_passability(&mut overlay_grid, &mut water, &registry, 0, 0);
        assert_eq!(water.cell(0, 0).unwrap().zone_type, zone_class::WATER);

        let mut beach = single_cell_terrain(LandType::Beach.as_index(), zero_wheel, false, false);
        recalc_overlay_passability(&mut overlay_grid, &mut beach, &registry, 0, 0);
        assert_eq!(beach.cell(0, 0).unwrap().zone_type, zone_class::BEACH);

        let mut rock = single_cell_terrain(LandType::Rock.as_index(), zero_wheel, false, true);
        recalc_overlay_passability(&mut overlay_grid, &mut rock, &registry, 0, 0);
        assert_eq!(rock.cell(0, 0).unwrap().zone_type, zone_class::IMPASSABLE);
    }

    #[test]
    fn gsi_04_04_recalc_zone_exact_priority_thresholds_and_non_predicates() {
        use crate::map::resolved_terrain::zone_class;
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile};

        let registry = gsi_04_04_registry();
        let clear = LandType::Clear.as_index();
        let mut clear_speed = SpeedCostProfile::default();
        clear_speed.wheel = Some(100);

        for (overlay_id, expected_zone, expected_blocks) in [
            (0, zone_class::CRUSHABLE, false),
            (1, zone_class::WALL, true),
            (2, zone_class::IMPASSABLE, true),
            (3, zone_class::IMPASSABLE, true),
            (4, zone_class::GROUND, false),
        ] {
            let mut overlay_grid = OverlayGrid::new(1, 1);
            overlay_grid.place_overlay(0, 0, overlay_id, 0);
            let mut terrain = single_cell_terrain(clear, clear_speed, false, false);
            terrain.cell_mut(0, 0).unwrap().terrain_object_occupation = Some(7);
            terrain.cell_mut(0, 0).unwrap().terrain_object_blocks = true;
            recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);
            let cell = terrain.cell(0, 0).expect("priority cell");
            assert_eq!(cell.zone_type, expected_zone, "overlay {overlay_id}");
            assert_eq!(cell.overlay_blocks, expected_blocks, "overlay {overlay_id}");
        }

        let mut zero_wheel = SpeedCostProfile::default();
        zero_wheel.wheel = Some(0);
        let mut overlay_grid = OverlayGrid::new(1, 1);
        overlay_grid.place_overlay(0, 0, 4, 0);
        let mut rubble_on_water =
            single_cell_terrain(LandType::Water.as_index(), zero_wheel, true, false);
        recalc_overlay_passability(&mut overlay_grid, &mut rubble_on_water, &registry, 0, 0);
        assert_eq!(
            rubble_on_water.cell(0, 0).unwrap().zone_type,
            zone_class::GROUND,
            "rubble is a terminal overlay result, not absence"
        );

        overlay_grid.place_overlay(0, 0, 5, 0);
        let mut gate_on_water =
            single_cell_terrain(LandType::Water.as_index(), zero_wheel, true, false);
        recalc_overlay_passability(&mut overlay_grid, &mut gate_on_water, &registry, 0, 0);
        assert_eq!(
            gate_on_water.cell(0, 0).unwrap().zone_type,
            zone_class::WATER
        );

        overlay_grid.place_overlay(0, 0, 6, 0);
        let mut exact_one_overlay = single_cell_terrain(clear, clear_speed, false, false);
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
            let mut speed = SpeedCostProfile::default();
            speed.wheel = Some(wheel);
            let mut terrain = single_cell_terrain(clear, speed, false, false);
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
            let mut object = single_cell_terrain(clear, clear_speed, false, false);
            let object_cell = object.cell_mut(0, 0).unwrap();
            object_cell.terrain_object_occupation = Some(occupation);
            object_cell.terrain_object_blocks = expected_blocks;
            recalc_overlay_passability(&mut empty_grid, &mut object, &registry, 0, 0);
            assert_eq!(object.cell(0, 0).unwrap().zone_type, expected_zone);
        }

        let mut blocked_base = single_cell_terrain(clear, clear_speed, false, true);
        recalc_overlay_passability(&mut empty_grid, &mut blocked_base, &registry, 0, 0);
        assert_eq!(
            blocked_base.cell(0, 0).unwrap().zone_type,
            zone_class::GROUND,
            "base_ground_walk_blocked is not a RecalcZoneType predicate"
        );
    }

    #[test]
    fn gsi_04_04_runtime_land_writer_retains_profiles_and_restores_pristine_tile() {
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};

        let registry = gsi_04_04_registry();
        let mut base_speed = SpeedCostProfile::default();
        base_speed.wheel = Some(100);

        for (overlay_id, land, class) in [
            (7, LandType::Road, TerrainClass::Road),
            (9, LandType::Wall, TerrainClass::Wall),
            (10, LandType::Railroad, TerrainClass::Railroad),
            (12, LandType::Water, TerrainClass::Water),
            (13, LandType::Rough, TerrainClass::Rough),
        ] {
            let mut overlay_grid = OverlayGrid::new(1, 1);
            overlay_grid.place_overlay(0, 0, overlay_id, 0);
            let mut terrain =
                single_cell_terrain(LandType::Clear.as_index(), base_speed, false, false);
            recalc_overlay_passability(&mut overlay_grid, &mut terrain, &registry, 0, 0);
            let cell = terrain.cell(0, 0).expect("retained land cell");
            assert_eq!(cell.land_type, land.as_index(), "overlay {overlay_id}");
            assert_eq!(cell.terrain_class, class, "overlay {overlay_id}");
            assert_eq!(
                cell.speed_costs,
                registry
                    .flags(overlay_id)
                    .unwrap()
                    .land_speed_costs
                    .unwrap(),
                "overlay {overlay_id} profile"
            );
            assert_eq!(cell.is_water, land == LandType::Water);
            assert_eq!(cell.is_road, land == LandType::Road);
            assert_eq!(cell.is_rough, land == LandType::Rough);
        }

        let mut overlay_grid = OverlayGrid::new(1, 1);
        overlay_grid.place_overlay(0, 0, 8, 0);
        let mut terrain = single_cell_terrain(LandType::Clear.as_index(), base_speed, false, false);
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
        assert_eq!(restored.terrain_class, TerrainClass::Clear);
        assert_eq!(restored.speed_costs, base_speed);
        assert!(!restored.is_water);
        assert!(!restored.is_road);
        assert!(!restored.is_rough);
    }

    #[test]
    fn gsi_04_04_runtime_tiberium_slope_branches_clear_exact_overlays() {
        use crate::map::resolved_terrain::zone_class;
        use crate::rules::terrain_rules::{LandType, SpeedCostProfile};

        let registry = gsi_04_04_registry();
        let mut base_speed = SpeedCostProfile::default();
        base_speed.wheel = Some(100);
        let ordinary_ore_speed = registry
            .flags(11)
            .expect("ordinary ore flags")
            .land_speed_costs
            .unwrap_or_default();

        for slope in [0, 4, 5] {
            let mut overlay_grid = OverlayGrid::new(1, 1);
            overlay_grid.place_overlay(0, 0, 11, 7);
            let mut terrain =
                single_cell_terrain(LandType::Clear.as_index(), base_speed, false, false);
            terrain.cell_mut(0, 0).unwrap().slope_type = slope;

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
                    single_cell_terrain(LandType::Clear.as_index(), base_speed, false, false);
                terrain.cell_mut(0, 0).unwrap().slope_type = slope;

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
                assert_eq!(cell.yr_cell_land_type, land.as_index());
                assert_eq!(cell.terrain_class, land.terrain_class());
                assert_eq!(cell.speed_costs, expected_speed);
                assert_eq!(cell.is_water, land.is_water());
                assert_eq!(cell.is_cliff_like, land.is_cliff_like());
                assert_eq!(cell.is_rough, land.is_rough());
                assert_eq!(cell.is_road, land.is_road());
                assert_eq!(cell.base_land_type, LandType::Clear.as_index());
                assert_eq!(cell.base_speed_costs, base_speed);
                assert_eq!(cell.overlay_zone_type, None);
                assert!(!cell.overlay_blocks);
                assert_eq!(cell.zone_type, zone_class::GROUND);
            }
        }
    }

    /// Round-trip on tiberium overlay add/remove: a fresh-spread or TIBTRE-spawned
    /// ore cell must inherit Tiberium-mode `land_type` / `terrain_class` /
    /// `speed_costs`, and a harvested-to-zero ore cell must revert to the
    /// underlying terrain values. Regression test for the §10 parity bug where
    /// runtime ore placement bypassed RecalcAttributes-equivalent logic.
    #[test]
    fn gsi_04_04_tiberium_overlay_round_trip_updates_terrain_metadata() {
        use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
        use crate::rules::ini_parser::IniFile;
        use crate::rules::terrain_rules::LandType;
        use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};

        // Registry with overlay id=0 marked Tiberium=yes (mirrors stock TIB01).
        let ini = IniFile::from_str(
            "[OverlayTypes]\n0=TIB01\n[Tiberium]\nTrack=70%\nFoot=90%\n[TIB01]\nTiberium=yes\n",
        );
        let registry = OverlayTypeRegistry::from_ini(&ini, None);

        // A single Clear-base cell at (5, 5). Underlying values mirror what
        // `resolved_terrain::build()` would produce on a clear-grass tile.
        let clear_lt = LandType::Clear.as_index();
        let base_speed = SpeedCostProfile::default();
        let mut cells = Vec::with_capacity(100);
        for ry in 0..10u16 {
            for rx in 0..10u16 {
                cells.push(ResolvedTerrainCell {
                    rx,
                    ry,
                    source_tile_index: 0,
                    source_sub_tile: 0,
                    final_tile_index: 0,
                    final_sub_tile: 0,
                    is_wood_bridge_repair_tile: false,
                    level: 0,
                    filled_clear: true,
                    tileset_index: None,
                    land_type: clear_lt,
                    yr_cell_land_type: clear_lt,
                    slope_type: 0,
                    template_height: 0,
                    render_offset_x: 0,
                    render_offset_y: 0,
                    terrain_class: TerrainClass::Clear,
                    speed_costs: base_speed,
                    is_water: false,
                    is_cliff_like: false,
                    is_rough: false,
                    is_road: false,
                    accepts_smudge: true,
                    allows_tiberium: false,
                    height_in_pixels: 0,
                    variant: 0,
                    has_ramp: false,
                    canonical_ramp: None,
                    ground_walk_blocked: false,
                    terrain_object_blocks: false,
                    terrain_object_occupation: None,
                    overlay_blocks: false,
                    overlay_zone_type: None,
                    outside_playfield: false,
                    zone_type: zone_class::GROUND,
                    base_ground_walk_blocked: false,
                    base_build_blocked: false,
                    base_land_type: clear_lt,
                    base_yr_cell_land_type: clear_lt,
                    base_terrain_class: TerrainClass::Clear,
                    base_speed_costs: base_speed,
                    build_blocked: false,
                    has_bridge_deck: false,
                    bridge_walkable: false,
                    bridge_transition: false,
                    bridge_deck_level: 0,
                    bridge_layer: None,
                    bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                    tube_index: None,
                    radar_left: [0; 3],
                    radar_right: [0; 3],
                    has_damaged_data: false,
                    bridgehead_anchor_class_at_load: None,
                });
            }
        }
        let mut terrain = ResolvedTerrainGrid::from_cells(10, 10, cells);

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
        let cell = &terrain.cells[idx];
        assert_eq!(cell.land_type, tib_lt, "land_type → Tiberium");
        assert_eq!(cell.yr_cell_land_type, tib_lt, "yr_cell_land_type → 5");
        assert_eq!(cell.terrain_class, TerrainClass::Tiberium);
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
        let cell = &terrain.cells[idx];
        assert_eq!(cell.land_type, clear_lt, "land_type → underlying Clear");
        assert_eq!(cell.yr_cell_land_type, clear_lt);
        assert_eq!(cell.terrain_class, TerrainClass::Clear);
        assert_eq!(
            cell.speed_costs, base_speed,
            "speed_costs → underlying default"
        );
        assert_eq!(cell.zone_type, zone_class::GROUND);
        assert!(!cell.overlay_blocks);
    }
}

#[cfg(test)]
mod recompute_tests {
    use super::*;
    use crate::rules::ini_parser::IniFile;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum WallHostEvent {
        Dirty(WallDirtyStep, (u16, u16)),
        Navigation {
            cell: (u16, u16),
            navigation_changed: bool,
            repair: WallZoneRepairKind,
        },
        Pointer(WallPointerTarget),
    }

    #[derive(Default)]
    struct WallHostSpy {
        events: Vec<WallHostEvent>,
    }

    impl WallDamageTransactionHost for WallHostSpy {
        fn dirty_step(&mut self, step: WallDirtyStep, packed_coord: (u16, u16)) {
            self.events.push(WallHostEvent::Dirty(step, packed_coord));
        }

        fn navigation_step(
            &mut self,
            _terrain: &ResolvedTerrainGrid,
            cell: (u16, u16),
            navigation_changed: bool,
            repair: WallZoneRepairKind,
        ) {
            self.events.push(WallHostEvent::Navigation {
                cell,
                navigation_changed,
                repair,
            });
        }

        fn pointer_expired(&mut self, target: WallPointerTarget) {
            self.events.push(WallHostEvent::Pointer(target));
        }
    }

    /// Build a registry whose overlay_id=2 maps to GAWALL (for auto-destruct
    /// threshold matching). Filler entries at id 0 and 1 get Wall=yes too so
    /// tests can exercise other ids if needed.
    fn make_wall_registry() -> OverlayTypeRegistry {
        let text = "\
[OverlayTypes]
0=GASAND
1=CYCL
2=GAWALL
[GASAND]
Wall=yes
Strength=400
[CYCL]
Wall=yes
Strength=400
[GAWALL]
Wall=yes
Strength=400
";
        let ini = IniFile::from_str(text);
        OverlayTypeRegistry::from_ini(&ini, None)
    }

    fn make_retail_stage_registry() -> OverlayTypeRegistry {
        let rules = IniFile::from_str(
            "[OverlayTypes]\n\
             0=GASAND\n\
             1=CYCL\n\
             2=GAWALL\n\
             [GASAND]\n\
             Wall=yes\n\
             Armor=wood\n\
             Strength=100\n\
             [CYCL]\n\
             [GAWALL]\n\
             Wall=yes\n\
             Armor=concrete\n\
             Strength=100\n",
        );
        let art = IniFile::from_str(
            "[GASAND]\n\
             DamageLevels=2\n\
             [GAWALL]\n\
             DamageLevels=3\n",
        );
        OverlayTypeRegistry::from_ini(&rules, Some(&art))
    }

    #[test]
    fn gsi_04_07_damage_stage_connection_forced_and_chain_order() {
        let registry = make_retail_stage_registry();
        let mut rng = crate::sim::rng::SimRng::new(7);

        let mut isolated = OverlayGrid::new(12, 12);
        isolated.place_overlay(5, 5, 0, 0);
        let result = damage_wall_overlay(&mut isolated, &registry, 5, 5, 100, &mut rng);
        assert_eq!(result.destroyed_cells, vec![(5, 5)]);

        let mut connected = OverlayGrid::new(12, 12);
        connected.place_overlay(5, 5, 0, 0x02);
        connected.place_overlay(6, 5, 0, 0x08);
        let result = damage_wall_overlay(&mut connected, &registry, 5, 5, 100, &mut rng);
        assert!(result.destroyed_cells.is_empty());
        assert_eq!(connected.cell(5, 5).overlay_data, 0x12);
        let result = damage_wall_overlay(&mut connected, &registry, 5, 5, 100, &mut rng);
        assert_eq!(result.destroyed_cells, vec![(5, 5)]);

        let mut forced_chain = OverlayGrid::new(12, 12);
        forced_chain.place_overlay(5, 5, 2, 0x1A);
        forced_chain.place_overlay(6, 5, 2, 0x08);
        let result = damage_wall_overlay(&mut forced_chain, &registry, 5, 5, -1, &mut rng);
        assert!(result.destroyed_cells.contains(&(5, 5)));
        assert_eq!(
            forced_chain.cell(6, 5).overlay_data & 0xF0,
            0x10,
            "forced terminal removal chains into pristine neighbors first"
        );
    }

    #[test]
    fn runtime_wall_damage_chain_and_cleanup_follow_fixed_aliases() {
        let registry = make_retail_stage_registry();
        let mut terrain = super::tests::clear_terrain_grid(512, 2);
        let mut rng = crate::sim::rng::SimRng::new(19);

        let mut chain = OverlayGrid::new(512, 2);
        chain.place_overlay(0, 1, 2, 0x18);
        chain.place_overlay(511, 0, 2, 0x02);
        let result = damage_wall_overlay_with_terrain(
            &mut chain,
            &registry,
            Some(&mut terrain),
            0,
            1,
            100,
            &mut rng,
        );
        assert_eq!(
            result.mutations.first(),
            Some(&WallMutation {
                rx: 511,
                ry: 0,
                kind: WallMutationKind::DirectUpdated,
            }),
            "penultimate chain reaches west's aliased real CellClass first"
        );
        assert_eq!(chain.cell(511, 0).overlay_data & 0xF0, 0x10);

        let mut cleanup_terrain = super::tests::clear_terrain_grid(512, 2);
        let cleanup_plane =
            super::tests::retained_wall_plane_for_sources(&cleanup_terrain, &[(0, 1), (511, 0)]);
        let mut cleanup =
            OverlayGrid::from_finalized_map_payload(FinalizedOverlayPayload::from_cells_for_test(
                512,
                2,
                vec![(-1, 0); 1024],
                cleanup_plane,
            ));
        cleanup.place_overlay(0, 1, 0, 0);
        cleanup.place_overlay(511, 0, 0, 0x12);
        for (rx, ry) in [(0, 1), (511, 0)] {
            let _ =
                recalc_overlay_passability(&mut cleanup, &mut cleanup_terrain, &registry, rx, ry);
        }
        let result = damage_wall_overlay_with_terrain(
            &mut cleanup,
            &registry,
            Some(&mut cleanup_terrain),
            0,
            1,
            -1,
            &mut rng,
        );
        assert_eq!(result.destroyed_cells, vec![(0, 1), (511, 0)]);
        assert_eq!(cleanup.cell(511, 0).overlay_id, None);
        assert!(
            cleanup
                .retained_wall_neighbor_counts()
                .expect("retained authority")
                .iter()
                .all(|&count| count == 0),
            "direct and cleanup removals reverse both fixed-alias count sources"
        );
        assert!(result.mutations.contains(&WallMutation {
            rx: 511,
            ry: 0,
            kind: WallMutationKind::CleanupRemoved,
        }));
    }

    #[test]
    fn wall_radar_dirty_is_captured_inline_in_native_visit_order() {
        let registry = make_retail_stage_registry();
        let mut rng = crate::sim::rng::SimRng::new(0x407);

        let mut partial = OverlayGrid::new(12, 12);
        partial.place_overlay(5, 5, 0, 0x02);
        let partial_result = damage_wall_overlay(&mut partial, &registry, 5, 5, 100, &mut rng);
        assert!(partial_result.radar_dirty_cells.is_empty());

        let mut direct = OverlayGrid::new(12, 12);
        direct.place_overlay(5, 5, 0, 0);
        let direct_result = damage_wall_overlay(&mut direct, &registry, 5, 5, -1, &mut rng);
        assert_eq!(
            direct_result.radar_dirty_cells,
            vec![
                (5, 5),
                (5, 3),
                (6, 4),
                (4, 4),
                (5, 4),
                (4, 6),
                (3, 5),
                (4, 5),
                (6, 6),
                (5, 7),
                (5, 6),
                (7, 5),
                (6, 5),
            ],
            "direct cell precedes N/W/S/E receivers, each with N/E/S/W/self visits",
        );
    }

    #[test]
    fn wall_damage_host_publishes_native_dirty_navigation_and_pointer_order() {
        let registry = make_retail_stage_registry();

        let mut rejected = OverlayGrid::new(12, 12);
        rejected.place_overlay(5, 5, 0, 0);
        let mut rejected_terrain = super::tests::clear_terrain_grid(12, 12);
        let mut rejected_rng = crate::sim::rng::SimRng::new(0x407);
        let mut rejected_host = WallHostSpy::default();
        let _ = damage_wall_overlay_with_runtime_host(
            &mut rejected,
            &registry,
            Some(&mut rejected_terrain),
            5,
            5,
            0,
            &mut rejected_rng,
            Some(&mut rejected_host),
        );
        assert!(
            rejected_host.events.is_empty(),
            "a failed Strength roll emits no native dirty callback"
        );

        let mut retained = OverlayGrid::new(12, 12);
        retained.place_overlay(5, 5, 0, 0x02);
        let mut retained_terrain = super::tests::clear_terrain_grid(12, 12);
        let mut retained_rng = crate::sim::rng::SimRng::new(0x408);
        let mut retained_host = WallHostSpy::default();
        let _ = damage_wall_overlay_with_runtime_host(
            &mut retained,
            &registry,
            Some(&mut retained_terrain),
            5,
            5,
            100,
            &mut retained_rng,
            Some(&mut retained_host),
        );
        assert_eq!(
            retained_host.events,
            vec![WallHostEvent::Dirty(WallDirtyStep::Tactical, (5, 5))],
            "accepted retained damage dirties tactical before its state write and emits no radar"
        );

        let mut terminal = OverlayGrid::new(12, 12);
        terminal.place_overlay(5, 5, 0, 0);
        let mut terminal_terrain = super::tests::clear_terrain_grid(12, 12);
        let mut terminal_rng = crate::sim::rng::SimRng::new(0x409);
        let mut terminal_host = WallHostSpy::default();
        let _ = damage_wall_overlay_with_runtime_host(
            &mut terminal,
            &registry,
            Some(&mut terminal_terrain),
            5,
            5,
            -1,
            &mut terminal_rng,
            Some(&mut terminal_host),
        );

        let mut expected = vec![
            WallHostEvent::Dirty(WallDirtyStep::Tactical, (5, 5)),
            WallHostEvent::Navigation {
                cell: (5, 5),
                navigation_changed: false,
                repair: WallZoneRepairKind::AssignOrphaned,
            },
            WallHostEvent::Dirty(WallDirtyStep::Radar, (5, 5)),
        ];
        for coord in [
            (5, 3),
            (6, 4),
            (5, 5),
            (4, 4),
            (5, 4),
            (4, 4),
            (5, 5),
            (4, 6),
            (3, 5),
            (4, 5),
            (5, 5),
            (6, 6),
            (5, 7),
            (4, 6),
            (5, 6),
            (6, 4),
            (7, 5),
            (6, 6),
            (5, 5),
            (6, 5),
        ] {
            expected.push(WallHostEvent::Dirty(WallDirtyStep::Tactical, coord));
            expected.push(WallHostEvent::Dirty(WallDirtyStep::Radar, coord));
        }
        expected.push(WallHostEvent::Pointer(WallPointerTarget::Real(5, 5)));
        assert_eq!(terminal_host.events, expected);

        let mut dummy_grid = OverlayGrid::new(3, 3);
        let mut dummy_terrain = super::tests::clear_terrain_grid(3, 3);
        let dummy = dummy_terrain.shared_cell_dummy();
        dummy.stamp_coord(-1, -1);
        dummy.write_overlay_identity_state(0, 0);
        let mut dummy_rng = crate::sim::rng::SimRng::new(0x40A);
        let mut dummy_host = WallHostSpy::default();
        let mut dummy_result = WallDamageResult::default();
        let mut terrain_authority = Some(&mut dummy_terrain);
        let mut host_authority: Option<&mut dyn WallDamageTransactionHost> = Some(&mut dummy_host);
        damage_wall_recursive(
            &mut dummy_grid,
            &registry,
            &mut terrain_authority,
            &mut host_authority,
            NativeRuntimeOverlayCell::Dummy,
            -1,
            &mut dummy_rng,
            &mut dummy_result,
        );
        assert_eq!(
            dummy_host.events.first(),
            Some(&WallHostEvent::Dirty(
                WallDirtyStep::Tactical,
                (u16::MAX, u16::MAX),
            ))
        );
        assert_eq!(
            dummy_host.events.last(),
            Some(&WallHostEvent::Pointer(WallPointerTarget::SharedDummy)),
            "the shared CellClass pointer-expiry dispatch remains ordered even though Rust has no represented dummy target"
        );
    }

    #[test]
    fn wall_radar_dirty_retains_legacy_edges_fixed_aliases_and_dummy_words() {
        let registry = make_retail_stage_registry();
        let mut rng = crate::sim::rng::SimRng::new(0x407_512);

        let mut edge = OverlayGrid::new(3, 3);
        edge.place_overlay(0, 0, 0, 0);
        let edge_result = damage_wall_overlay(&mut edge, &registry, 0, 0, -1, &mut rng);
        assert_eq!(
            edge_result.radar_dirty_cells,
            vec![(0, 0), (1, 1), (0, 2), (0, 1), (2, 0), (1, 0)],
        );

        let mut alias_terrain = super::tests::clear_terrain_grid(512, 2);
        let mut aliased_edge = OverlayGrid::new(512, 2);
        aliased_edge.place_overlay(0, 1, 0, 0);
        let alias_result = damage_wall_overlay_with_terrain(
            &mut aliased_edge,
            &registry,
            Some(&mut alias_terrain),
            0,
            1,
            -1,
            &mut rng,
        );
        assert_eq!(
            alias_result.radar_dirty_cells,
            vec![
                (0, 1),
                (0, u16::MAX),
                (1, 0),
                (u16::MAX, 0),
                (0, 0),
                (511, u16::MAX),
                (511, 1),
                (510, 0),
                (511, 0),
                (1, 2),
                (1, 3),
                (0, 3),
                (2, 1),
                (1, 1),
            ],
            "fixed-grid output preserves real aliases and raw signed dummy coordinates",
        );
    }

    #[test]
    fn gsi_04_07_damage_cleanup_has_fixed_scope_and_preserves_owner_quirks() {
        let registry = make_retail_stage_registry();
        let owner = crate::sim::intern::InternedId::from_index(9);
        let mut grid = OverlayGrid::new(16, 8);
        grid.place_owned_wall(4, 3, 2, 0x20, owner);
        grid.place_owned_wall(6, 3, 2, 0x20, owner);

        let destroyed = cleanup_wall_neighbors(&mut grid, &registry, 2, 3);
        assert_eq!(destroyed, vec![(4, 3)]);
        assert_eq!(grid.cell(4, 3).overlay_id, None);
        assert_eq!(grid.cell(4, 3).overlay_data, 0);
        assert_eq!(
            grid.cell(4, 3).wall_owner,
            Some(owner),
            "GAWALL isolated cleanup leaves its stale owner"
        );
        assert_eq!(
            grid.cell(6, 3).overlay_id,
            Some(2),
            "cleanup does not recursively flood beyond the fixed cross fan-out"
        );

        grid.place_owned_wall(4, 4, 0, 0x10, owner);
        let _ = cleanup_wall_neighbors(&mut grid, &registry, 2, 4);
        assert_eq!(grid.cell(4, 4).overlay_id, None);
        assert_eq!(grid.cell(4, 4).wall_owner, None);
    }

    #[test]
    fn gsi_04_07_damage_raw_signed_gate_preserves_rng_classification() {
        let registry = make_wall_registry();

        let mut over_strength = OverlayGrid::new(8, 8);
        over_strength.place_overlay(3, 3, 2, 0);
        let mut over_strength_rng = crate::sim::rng::SimRng::new(11);
        let before = over_strength_rng.state();
        let result = damage_wall_overlay(
            &mut over_strength,
            &registry,
            3,
            3,
            65_537,
            &mut over_strength_rng,
        );
        assert_eq!(
            over_strength_rng.state(),
            before,
            "damage >= Strength draws none"
        );
        assert_eq!(result.destroyed_cells, vec![(3, 3)]);

        let mut negative = OverlayGrid::new(8, 8);
        negative.place_overlay(3, 3, 2, 0);
        let mut negative_rng = crate::sim::rng::SimRng::new(11);
        let before = negative_rng.state();
        let result = damage_wall_overlay(&mut negative, &registry, 3, 3, -2, &mut negative_rng);
        assert_ne!(
            negative_rng.state(),
            before,
            "negative non-sentinel damage still draws"
        );
        assert!(result.mutations.is_empty());
        assert_eq!(negative.cell(3, 3).overlay_id, Some(2));

        let mut forced = OverlayGrid::new(8, 8);
        forced.place_overlay(3, 3, 2, 0);
        let mut forced_rng = crate::sim::rng::SimRng::new(11);
        let before = forced_rng.state();
        let result = damage_wall_overlay(&mut forced, &registry, 3, 3, -1, &mut forced_rng);
        assert_eq!(
            forced_rng.state(),
            before,
            "literal -1 bypasses the Strength gate"
        );
        assert_eq!(result.destroyed_cells, vec![(3, 3)]);
    }

    #[test]
    fn gsi_04_07_damage_chain_is_inline_then_cleanup_removal_is_retained() {
        let registry = make_retail_stage_registry();
        let owner = crate::sim::intern::InternedId::from_index(12);
        let mut rng = crate::sim::rng::SimRng::new(3);

        let mut chain = OverlayGrid::new(12, 12);
        chain.place_overlay(5, 5, 2, 0x10);
        chain.place_overlay(5, 4, 2, 0x04);
        chain.place_overlay(6, 5, 2, 0x08);
        chain.place_overlay(5, 6, 2, 0x01);
        chain.place_overlay(4, 5, 2, 0x02);
        let result = damage_wall_overlay(&mut chain, &registry, 5, 5, 100, &mut rng);
        let ordered: Vec<((u16, u16), WallMutationKind)> = result
            .mutations
            .iter()
            .map(|mutation| ((mutation.rx, mutation.ry), mutation.kind))
            .collect();
        assert_eq!(
            ordered,
            vec![
                ((5, 4), WallMutationKind::DirectUpdated),
                ((6, 5), WallMutationKind::DirectUpdated),
                ((5, 6), WallMutationKind::DirectUpdated),
                ((4, 5), WallMutationKind::DirectUpdated),
                ((5, 5), WallMutationKind::DirectRemoved),
                ((5, 4), WallMutationKind::CleanupUpdated),
                ((4, 5), WallMutationKind::CleanupUpdated),
                ((5, 6), WallMutationKind::CleanupUpdated),
                ((6, 5), WallMutationKind::CleanupUpdated),
            ],
            "each recursive cardinal completes before the parent clear, then fixed cleanup runs"
        );

        let mut cleanup_removal = OverlayGrid::new(12, 12);
        cleanup_removal.place_overlay(5, 5, 2, 0);
        cleanup_removal.place_owned_wall(5, 4, 2, 0x24, owner);
        let result = damage_wall_overlay(&mut cleanup_removal, &registry, 5, 5, -1, &mut rng);
        assert_eq!(
            result.mutations,
            vec![
                WallMutation {
                    rx: 5,
                    ry: 5,
                    kind: WallMutationKind::DirectRemoved,
                },
                WallMutation {
                    rx: 5,
                    ry: 4,
                    kind: WallMutationKind::CleanupRemoved,
                },
            ]
        );
        assert_eq!(result.destroyed_cells, vec![(5, 5), (5, 4)]);
        assert_eq!(cleanup_removal.cell(5, 4).overlay_id, None);
        assert_eq!(cleanup_removal.cell(5, 4).wall_owner, Some(owner));
    }

    #[test]
    fn recompute_no_op_for_non_wall_cell() {
        let mut grid = OverlayGrid::new(10, 10);
        let reg = OverlayTypeRegistry::empty();
        let r = recompute_wall_connectivity_at(&mut grid, &reg, 5, 5);
        assert_eq!(r, RecomputeResult::NoChange);
    }

    #[test]
    fn wall_damage_inclusive_range_and_ge_boundary() {
        // RandomRanged(0, 400) on seed=1: span=400, mask=0x1FF; first masked
        // sample 0x78B76ED5 & 0x1FF = 0xD5 = 213 (<= 400 -> accepted). gamemd
        // applies damage only when roll < damage, so with roll == 213:
        //   damage=213 -> roll >= damage -> NO-OP (the old `>` test applied here)
        //   damage=214 -> roll <  damage -> damage applied
        let reg = make_wall_registry();

        // No-op exactly at roll == damage (the key old-vs-new discriminator).
        let mut grid = OverlayGrid::new(10, 10);
        grid.place_overlay(5, 5, 2, 0);
        let mut rng = crate::sim::rng::SimRng::new(1);
        let r = damage_wall_overlay(&mut grid, &reg, 5, 5, 213, &mut rng);
        assert!(
            r.changed_cells.is_empty() && r.destroyed_cells.is_empty(),
            "roll == damage must be a no-op"
        );
        assert_eq!(
            grid.cell(5, 5).overlay_data,
            0,
            "no nibble change at roll == damage"
        );
        assert_eq!(
            grid.cell(5, 5).overlay_id,
            Some(2),
            "wall intact at roll == damage"
        );

        // Damage applied just below the boundary (same seed -> same roll 213).
        // (This registry leaves DamageLevels at its low default, so a single hit
        // fully destroys the wall rather than only bumping the nibble — either
        // outcome proves damage was applied, which is what the boundary tests.)
        let mut grid = OverlayGrid::new(10, 10);
        grid.place_overlay(5, 5, 2, 0);
        let mut rng = crate::sim::rng::SimRng::new(1);
        let r = damage_wall_overlay(&mut grid, &reg, 5, 5, 214, &mut rng);
        assert!(
            !r.changed_cells.is_empty() || !r.destroyed_cells.is_empty(),
            "roll < damage must apply damage"
        );
        let cell = grid.cell(5, 5);
        assert!(
            cell.overlay_id != Some(2) || cell.overlay_data != 0,
            "wall must be destroyed or have its damage nibble advanced"
        );
    }

    #[test]
    fn recompute_updates_nibble_when_neighbor_changes() {
        let mut grid = OverlayGrid::new(10, 10);
        let reg = make_wall_registry();
        // Place two adjacent GAWALL at (5,5) and (6,5). Initialize (5,5) with
        // stale connectivity (0b0001 = N) so the recompute changes it to
        // 0b0010 (E neighbor only).
        grid.place_overlay(5, 5, 2, 0b0001);
        grid.place_overlay(6, 5, 2, 0);
        let r = recompute_wall_connectivity_at(&mut grid, &reg, 5, 5);
        assert_eq!(r, RecomputeResult::Updated);
        assert_eq!(grid.cell(5, 5).overlay_data, 0b0010);
    }

    #[test]
    fn recompute_destroys_isolated_max_damage_gawall() {
        let mut grid = OverlayGrid::new(10, 10);
        let reg = make_wall_registry();
        // Isolated GAWALL with damage stage 3 (= 0x30) and connectivity 0 → 0x30
        // matches the auto-destruct threshold.
        grid.place_overlay(5, 5, 2, 0x30);
        let r = recompute_wall_connectivity_at(&mut grid, &reg, 5, 5);
        assert_eq!(r, RecomputeResult::Destroyed);
        assert_eq!(grid.cell(5, 5).overlay_id, None);
    }

    #[test]
    fn cleanup_auto_destruct_thresholds_cover_active_retail_wall_ids_only() {
        for (overlay_id, accepted) in [
            (0x00, &[0x10, 0x20][..]),
            (0x02, &[0x20, 0x30][..]),
            (0x1A, &[0x20, 0x30][..]),
        ] {
            for full_byte in [0x00, 0x10, 0x20, 0x30] {
                assert_eq!(
                    auto_destruct_threshold(overlay_id, full_byte),
                    accepted.contains(&full_byte),
                    "overlay {overlay_id:#04x}, byte {full_byte:#04x}"
                );
            }
        }
        for (dormant, hardcoded_bytes) in [
            (0x01, &[0x20][..]),
            (0x03, &[0x10][..]),
            (0x16, &[0x10, 0x20][..]),
        ] {
            for &full_byte in hardcoded_bytes {
                assert!(
                    !auto_destruct_threshold(dormant, full_byte),
                    "TS/mod-only hardcoded rows stay excluded from active-retail behavior"
                );
            }
        }
        assert!(!auto_destruct_threshold(0x04, 0x20));
    }

    #[test]
    fn recompute_keeps_max_damage_wall_when_connected() {
        let mut grid = OverlayGrid::new(10, 10);
        let reg = make_wall_registry();
        // GAWALL at (5,5) with damage stage 3 PLUS connection to E neighbor.
        // Connected → byte = 0x30 | 0b0010 = 0x32 → not in auto-destruct set → kept.
        grid.place_overlay(5, 5, 2, 0x30);
        grid.place_overlay(6, 5, 2, 0);
        let r = recompute_wall_connectivity_at(&mut grid, &reg, 5, 5);
        assert_eq!(r, RecomputeResult::Updated);
        assert_eq!(grid.cell(5, 5).overlay_data, 0x32);
        assert!(grid.cell(5, 5).overlay_id.is_some());
    }

    #[test]
    fn cleanup_chain_does_not_destroy_intact_segment() {
        let mut grid = OverlayGrid::new(10, 10);
        let reg = make_wall_registry();
        // Row of 3 GAWALL at (4,5), (5,5), (6,5), all at max damage stage 3.
        // Connectivity nibbles: (4,5)=0b0010 (E only), (5,5)=0b1010 (E+W),
        // (6,5)=0b1000 (W only). Full bytes: 0x32, 0x3A, 0x38.
        grid.place_overlay(4, 5, 2, 0x32);
        grid.place_overlay(5, 5, 2, 0x3A);
        grid.place_overlay(6, 5, 2, 0x38);
        // Destroy the leftmost cell directly (simulating damage_wall_overlay's destroy).
        grid.clear_overlay(4, 5);
        // Run cleanup starting from (4, 5).
        let destroyed = cleanup_wall_neighbors(&mut grid, &reg, 4, 5);
        // (5,5) loses W connection → byte = 0x32 → not in {0x20, 0x30} → keeps.
        // (6,5) keeps W connection (5,5 still present) → byte = 0x38 → keeps.
        assert!(destroyed.is_empty());
        assert!(grid.cell(5, 5).overlay_id.is_some());
        assert!(grid.cell(6, 5).overlay_id.is_some());
    }

    #[test]
    fn cleanup_chain_terminates_via_visited_set() {
        let mut grid = OverlayGrid::new(10, 10);
        let reg = make_wall_registry();
        // 5-cell row of GAWALL at max damage; recompute initial connectivity
        // via the helper to set up coherent state, then destroy the leftmost
        // cell and run cleanup. Test passes if it returns in finite time.
        let cells: [(u16, u16); 5] = [(2, 5), (3, 5), (4, 5), (5, 5), (6, 5)];
        for &(rx, ry) in &cells {
            grid.place_overlay(rx, ry, 2, 0x30);
        }
        for &(rx, ry) in &cells {
            let _ = recompute_wall_connectivity_at(&mut grid, &reg, rx, ry);
        }
        // After initial recompute, cells with neighbors survive; isolated ones
        // would already be gone. The endpoints had only one neighbor → byte
        // 0x32 or 0x38, both kept. Middle three: 0x3A, kept.
        // Now destroy (2,5) and trigger cleanup.
        grid.clear_overlay(2, 5);
        let _ = cleanup_wall_neighbors(&mut grid, &reg, 2, 5);
        // No assertion: termination is the test.
    }

    #[test]
    fn cleanup_handles_oob_neighbors() {
        let mut grid = OverlayGrid::new(10, 10);
        let reg = make_wall_registry();
        grid.place_overlay(0, 0, 2, 0x30);
        grid.clear_overlay(0, 0);
        // Cleanup at (0,0) — neighbors include (-1, 0) and (0, -1) which are OOB.
        let _ = cleanup_wall_neighbors(&mut grid, &reg, 0, 0);
        // No panic = pass.
    }
}
