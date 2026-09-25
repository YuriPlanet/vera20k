//! Deterministic fog/shroud visibility state computed from unit vision radii.
//!
//! Each cell has two independent flags: "revealed" (seen at least once) and
//! "visible" (currently in line of sight). State is stored in a flat Vec<u8>
//! grid per owner for O(1) lookup.
//!
//! ## Performance
//! Direct source-aware writes publish viewer knowledge. Queries use a cached
//! visibility grid so each cell lookup is O(1) instead of iterating all owners.

mod gap_source;
mod map_reveal;
mod shroud_knowledge;
pub(crate) use gap_source::GapGeneratorRuntime;
pub(crate) use shroud_knowledge::SightRefreshTimers;
use shroud_knowledge::{ShroudKnowledge, SightAdmission};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::map::houses::{HouseAllianceMap, are_houses_friendly};
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::pathfinding::PathGrid;

/// Bit flag: cell has been seen at least once (persists across ticks).
const FLAG_REVEALED: u8 = 0x01;
/// Bit flag: cell is currently in line of sight (rebuilt each tick).
const FLAG_VISIBLE: u8 = 0x02;
/// Bit flag: cell is covered by an enemy gap generator (rebuilt each tick).
/// Entities on gap-covered cells are hidden from the local player; terrain
/// renders black (treated as unrevealed).
const FLAG_GAP_COVERED: u8 = 0x04;
/// Bit flag: cell is covered by a friendly (own/allied) gap generator (rebuilt
/// each tick). This bookkeeping does not darken the owner's terrain.
const FLAG_GAP_FOG: u8 = 0x08;
// Native70AF50/70B1D0 contribute/remove sustained sight via MapCell4A9CA0.
// Fire4876F0 and Psychic6CD773/6CD79C do not leave that contribution alive.
const FLAG_SUSTAINED_SIGHT: u8 = 0x10;
// Pending native Cell+140 bit20, consumed by578100 on the120-frame Logic rung.
// This is the live bitmap authority; legacy CellVisibilityRuntime is not an
// exact native counter/cache model and its flags must not drive this transition.
const FLAG_PENDING_GAP_CONCEAL: u8 = 0x20;
const FLAG_HOSTILE_GAP_PRESENT: u8 = 0x40;
// Effective viewer sight is never re-exported as a local allied source.
const FLAG_EFFECTIVE_GAP_SIGHT: u8 = 0x80;

/// Serialized CellClass visibility fields for one owner/cell projection.
///
/// The renderer continues to consume the compact `OwnerVisibility::cells`
/// bitmap. This state preserves the native transition contract underneath it:
/// signed shroud counters, the split CellClass flag words, and the two signed
/// occlusion caches. It is kept per owner because VERA's visibility authority
/// is per house, while the retail CellClass helpers read the current player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CellVisibilityRuntime {
    /// CellClass +0x130. `-1` is the native inactive sentinel.
    pub shroud_counter: i32,
    /// CellClass +0x134 upper clamp for `shroud_counter`.
    pub gap_shroud_counter: i32,
    /// CellClass +0x12C: ground visible (`0x08`) and ground cache open (`0x10`).
    pub alt_flags: u8,
    /// CellClass +0x140 visibility/fog transition flags.
    pub flags: u32,
    /// CellClass +0x120 ground occlusion cache.
    pub visibility: i8,
    /// CellClass +0x121 fog/air occlusion cache.
    pub foggedness: i8,
}

impl Default for CellVisibilityRuntime {
    fn default() -> Self {
        Self {
            shroud_counter: -1,
            gap_shroud_counter: i32::MAX,
            alt_flags: 0,
            flags: 0,
            visibility: 0,
            foggedness: 0,
        }
    }
}

/// Ordered side-effect boundary of the native map-cell visibility update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellVisibilityEvent {
    /// TacticalClass::RegisterCellAsVisible: the redraw invalidation boundary.
    RegisterCellAsVisible,
    /// MapClass::RevealCheck, always before an eligible clean-fog operation.
    RevealCheck,
    /// CellClass::CleanFog on the first mapped/fog-display transition only.
    CleanFog,
}

impl CellVisibilityRuntime {
    const ALT_GROUND_VISIBLE: u8 = 0x08;
    const ALT_GROUND_OPEN: u8 = 0x10;
    const FLAG_FOG_OPEN: u32 = 0x01;
    const FLAG_MAPPED: u32 = 0x02;
    const FLAG_SHROUD_POSITIVE: u32 = 0x20;
    const FLAG_TRANSIENT: u32 = 0x40;
    const FLAG_FOGGED_OBJECT_SNAPSHOT: u32 = 0x400000;

    #[cfg(test)]
    fn set_fogged_object_snapshot(&mut self, present: bool) {
        if present {
            self.flags |= Self::FLAG_FOGGED_OBJECT_SNAPSHOT;
        } else {
            self.flags &= !Self::FLAG_FOGGED_OBJECT_SNAPSHOT;
        }
    }

    /// Native `CellClass::IncreaseShroudCounter`: no redraw side effect.
    pub fn increase_shroud_counter(&mut self) {
        let old = self.shroud_counter;
        if self.shroud_counter == -1 {
            self.shroud_counter = 0;
        }
        self.shroud_counter = self.shroud_counter.saturating_add(1);
        self.shroud_counter = self.shroud_counter.min(self.gap_shroud_counter);
        if old <= 0 && self.shroud_counter > 0 {
            self.flags |= Self::FLAG_SHROUD_POSITIVE;
        }
    }

    /// Native `CellClass::ReduceShroudCounter`, including the `1 -> -1` edge.
    /// Counter mutation itself deliberately emits no redraw event.
    pub fn reduce_shroud_counter(&mut self) {
        if self.shroud_counter == 1 {
            self.shroud_counter = 0;
        }
        self.shroud_counter = self.shroud_counter.saturating_sub(1);
        if self.shroud_counter > 0 {
            return;
        }
        if self.alt_flags & (Self::ALT_GROUND_VISIBLE | Self::ALT_GROUND_OPEN)
            == (Self::ALT_GROUND_VISIBLE | Self::ALT_GROUND_OPEN)
        {
            self.flags &= !Self::FLAG_SHROUD_POSITIVE;
        } else {
            self.alt_flags |= Self::ALT_GROUND_VISIBLE | Self::ALT_GROUND_OPEN;
        }
    }

    /// Native `CellClass::Unshroud` flag projection; it is not a traversal.
    #[cfg(test)]
    pub fn unshroud(&mut self) {
        self.alt_flags |= Self::ALT_GROUND_VISIBLE | Self::ALT_GROUND_OPEN;
        if self.shroud_counter > 0 {
            self.flags |= Self::FLAG_SHROUD_POSITIVE;
        }
    }

    /// Apply the full MapCell-style projection. The event callback makes the
    /// `RevealCheck`-before-`CleanFog` order explicit without coupling sim to
    /// renderer invalidation or the unported fogged-object render records.
    pub fn map_visible(&mut self, fog_of_war: bool, mut emit: impl FnMut(CellVisibilityEvent)) {
        let had_mapped = self.flags & Self::FLAG_MAPPED != 0;
        let before = *self;

        self.flags = (self.flags & !(Self::FLAG_MAPPED | Self::FLAG_TRANSIENT)) | Self::FLAG_MAPPED;
        self.increase_shroud_counter();
        self.alt_flags |= Self::ALT_GROUND_VISIBLE | Self::ALT_GROUND_OPEN;
        self.visibility = -1;
        self.flags |= Self::FLAG_FOG_OPEN;
        self.foggedness = -1;

        if *self != before {
            emit(CellVisibilityEvent::RegisterCellAsVisible);
            emit(CellVisibilityEvent::RevealCheck);
        }
        if !had_mapped && fog_of_war {
            // CellClass::CleanFog clears the snapshot bit before freeing the
            // shared footprint records; the record store is intentionally not
            // represented until its owner/link lifetime has a Rust authority.
            self.flags &= !Self::FLAG_FOGGED_OBJECT_SNAPSHOT;
            emit(CellVisibilityEvent::CleanFog);
        }
    }
}

/// RA2 hard-caps effective sight at 10 cells. Going past 10 was a crash
/// in the original engine — we clamp to this limit for compatibility.
pub const MAX_SIGHT_RANGE: u16 = 10;

/// World height of one terrain level, in leptons. Same retail value the
/// coordinate-Z evaluator uses (`util::lepton::LEPTONS_PER_LEVEL`); kept local
/// because the reveal kernel's input is a height, not a bridge or range query.
const LEPTONS_PER_HEIGHT_LEVEL: i32 = 104;

/// Screen height of one isometric cell, in pixels. The reveal centre is pushed
/// toward isometric north by however many whole cells of screen lift the
/// viewer's height buys, so the revealed disc sits under the sprite rather than
/// under its ground shadow.
const CELL_HEIGHT_PX: i32 = crate::map::terrain::TILE_HEIGHT as i32;

/// Percentage of base sight added per elevation step.
///
/// Original: `TechnoClass::UpdateReveal` derives the step count by dividing the
/// object's world Z in leptons by `[General] LeptonsPerSightIncrease`, scales it
/// by ten, then computes `Sight * (1 + 0.01 * that)` — so the combination is
/// multiplicative off world Z, not an additive per-terrain-level bonus, and one
/// step is +10%. Verified 2026-08-04.
const ELEVATION_SIGHT_PERCENT_PER_STEP: i32 = 10;

/// Screen lift, in whole pixels, for an object at `height_leptons` above the
/// map plane. Reproduces the engine's height→screen conversion including its
/// extra-pixel threshold and the `+0.5` that precedes a truncating float→int.
fn height_lift_px(height_leptons: i32) -> i32 {
    crate::util::native_x87::adjust_for_z_standard(height_leptons)
}

/// Cells the reveal spiral's centre is shifted toward isometric north.
///
/// For a ground object standing on terrain level `L` this is `L / 2`, matching
/// the shorthand this used to be written as; for an airborne object it is
/// driven by its lepton altitude instead, which at stock `FlightLevel=1500`
/// works out to 7 cells — the same distance its sprite is drawn above its
/// ground cell.
fn iso_height_shift_cells(height_leptons: i32) -> i32 {
    height_lift_px(height_leptons) / CELL_HEIGHT_PX
}

/// Height above the map plane, in leptons, for one entity.
///
/// The engine keeps a single 3-D world coordinate per object and feeds its Z to
/// both the reveal-centre shift and the line-of-sight viewer level, so terrain
/// elevation and flight altitude are one quantity here too. The precedence
/// between the three things that can hold an object up mirrors
/// `render::locomotor_visual` exactly, so the shroud and the sprite cannot
/// disagree about where the object is.
fn entity_height_leptons(entity: &crate::sim::game_entity::GameEntity) -> i32 {
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::movement::locomotor::MovementLayer;

    let terrain: i32 = i32::from(entity.position.z) * LEPTONS_PER_HEIGHT_LEVEL;
    let above_ground: i32 = if let Some(state) = entity.parachute_state.as_ref() {
        state.altitude.to_num::<i32>()
    } else if let Some(state) = entity.rocket_state.as_ref() {
        state.altitude.to_num::<i32>()
    } else {
        match entity.locomotor.as_ref() {
            Some(loco)
                if loco.layer == MovementLayer::Air && loco.kind != LocomotorKind::Rocket =>
            {
                loco.altitude.to_num::<i32>()
            }
            _ => 0,
        }
    };
    terrain + above_ground
}

/// Per-owner visibility stored as a flat grid of flag bytes.
///
/// Indexed by `ry * width + rx`. Each byte holds public visibility plus
/// sustained-sight, current hostile coverage and pending-conceal provenance. This gives O(1) per-cell lookups instead of O(log n)
/// with the previous BTreeSet design.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnerVisibility {
    cells: Vec<u8>,
    width: u16,
    height: u16,
    /// CellClass-like transition state aligned with `cells`. Old snapshots
    /// deserialize this as empty and are expanded lazily before the next tick.
    #[serde(default)]
    cell_runtime: Vec<CellVisibilityRuntime>,
    /// Number of current-frame visibility contributors per cell. This lets the
    /// next recompute apply the same number of native counter reductions that
    /// this frame admitted; it is serialized because a snapshot can occur
    /// between visibility rebuilds.
    #[serde(default)]
    visibility_marks: Vec<u16>,
    shroud_knowledge: Vec<ShroudKnowledge>,
}

impl Default for OwnerVisibility {
    fn default() -> Self {
        Self {
            cells: Vec::new(),
            width: 0,
            height: 0,
            cell_runtime: Vec::new(),
            visibility_marks: Vec::new(),
            shroud_knowledge: Vec::new(),
        }
    }
}

impl OwnerVisibility {
    /// Create a new zeroed visibility grid of the given dimensions.
    pub fn new(width: u16, height: u16) -> Self {
        let len: usize = (width as usize) * (height as usize);
        Self {
            cells: vec![0u8; len],
            width,
            height,
            cell_runtime: vec![CellVisibilityRuntime::default(); len],
            visibility_marks: vec![0; len],
            shroud_knowledge: vec![ShroudKnowledge::default(); len],
        }
    }

    /// Index into the flat grid, or None if out of bounds.
    fn index(&self, rx: u16, ry: u16) -> Option<usize> {
        if rx < self.width && ry < self.height {
            Some((ry as usize) * (self.width as usize) + (rx as usize))
        } else {
            None
        }
    }

    /// Returns true if the cell is currently visible (in line of sight).
    pub fn is_visible(&self, rx: u16, ry: u16) -> bool {
        self.index(rx, ry)
            .map_or(false, |i| self.cells[i] & FLAG_VISIBLE != 0)
    }

    /// Returns true if the cell has been revealed at least once.
    pub fn is_revealed(&self, rx: u16, ry: u16) -> bool {
        self.index(rx, ry)
            .map_or(false, |i| self.cells[i] & FLAG_REVEALED != 0)
    }

    /// Returns true if the cell is covered by an enemy gap generator this tick.
    pub fn is_gap_covered(&self, rx: u16, ry: u16) -> bool {
        self.index(rx, ry)
            .map_or(false, |i| self.cells[i] & FLAG_GAP_COVERED != 0)
    }

    /// Returns true if the cell is covered by a friendly gap generator this tick.
    pub fn is_gap_fog(&self, rx: u16, ry: u16) -> bool {
        self.index(rx, ry)
            .map_or(false, |i| self.cells[i] & FLAG_GAP_FOG != 0)
    }

    /// Mark a cell as both visible and revealed.
    #[cfg(test)]
    pub fn mark_visible(&mut self, rx: u16, ry: u16) {
        self.mark_visible_with_fog_of_war(rx, ry, true);
        if let Some(index) = self.index(rx, ry) {
            self.shroud_knowledge[index].counter = 0;
            self.shroud_knowledge[index].open = true;
            self.shroud_knowledge[index].transient_visible = true;
            self.publish_knowledge(index);
        }
    }

    /// Same as [`Self::mark_visible`], with the scenario fog rule carried to
    /// the CellClass first-map transition.
    pub fn mark_visible_with_fog_of_war(&mut self, rx: u16, ry: u16, fog_of_war: bool) {
        if let Some(i) = self.index(rx, ry) {
            self.ensure_cell_runtime();
            self.cell_runtime[i].map_visible(fog_of_war, |_| {});
            self.visibility_marks[i] = self.visibility_marks[i].saturating_add(1);
            self.cells[i] |= FLAG_VISIBLE | FLAG_REVEALED;
        }
    }

    /// Clear all visible flags while preserving revealed flags.
    /// Called each tick by `recompute_owner_visibility_in_place` so existing
    /// grids can be reused without reallocation.
    pub fn clear_all_visible(&mut self) {
        self.ensure_cell_runtime();
        for knowledge in &mut self.shroud_knowledge {
            knowledge.transient_visible = false;
        }
        for ((cell, runtime), marks) in self
            .cells
            .iter_mut()
            .zip(&mut self.cell_runtime)
            .zip(&mut self.visibility_marks)
        {
            if *cell & FLAG_VISIBLE != 0 {
                for _ in 0..(*marks).max(1) {
                    runtime.reduce_shroud_counter();
                }
            }
            *marks = 0;
            *cell &= !(FLAG_VISIBLE
                | FLAG_GAP_COVERED
                | FLAG_GAP_FOG
                | FLAG_SUSTAINED_SIGHT
                | FLAG_EFFECTIVE_GAP_SIGHT);
        }
        for index in 0..self.cells.len() {
            self.publish_knowledge(index);
        }
    }

    fn publish_knowledge(&mut self, index: usize) {
        let knowledge = self.shroud_knowledge[index];
        let cell = &mut self.cells[index];
        *cell &= !(FLAG_REVEALED
            | FLAG_VISIBLE
            | FLAG_GAP_COVERED
            | FLAG_SUSTAINED_SIGHT
            | FLAG_EFFECTIVE_GAP_SIGHT
            | FLAG_PENDING_GAP_CONCEAL
            | FLAG_HOSTILE_GAP_PRESENT);
        if knowledge.open {
            *cell |= FLAG_REVEALED;
        }
        if knowledge.local_sources > 0 {
            *cell |= FLAG_SUSTAINED_SIGHT;
        }
        if knowledge.allied_sources > 0 {
            *cell |= FLAG_EFFECTIVE_GAP_SIGHT;
        }
        if knowledge.open
            && (knowledge.local_sources > 0
                || knowledge.allied_sources > 0
                || knowledge.transient_visible)
        {
            *cell |= FLAG_VISIBLE;
        }
        if knowledge.pending {
            *cell |= FLAG_PENDING_GAP_CONCEAL;
        }
        if knowledge.gap_counter > 0 {
            *cell |= FLAG_HOSTILE_GAP_PRESENT;
            if !knowledge.open {
                *cell |= FLAG_GAP_COVERED;
            }
        }
    }

    /// Clear only transient Gap Generator flags while preserving line of sight
    /// and persisted map knowledge.
    fn clear_gap_flags(&mut self) {
        for cell in &mut self.cells {
            *cell &= !(FLAG_GAP_COVERED | FLAG_GAP_FOG);
        }
    }

    /// Zero all flags (visible + revealed). Used when reusing the merged
    /// grid buffer in `build_merged_for`.
    fn clear_all(&mut self) {
        for cell in &mut self.cells {
            *cell = 0;
        }
    }

    /// Return the raw cells slice for deterministic hashing.
    pub fn cells_raw(&self) -> &[u8] {
        &self.cells
    }

    /// Serialized CellClass-style visibility state, in the same row-major
    /// order as `cells`, for deterministic hashing and snapshot inspection.
    pub fn cell_runtime_raw(&self) -> &[CellVisibilityRuntime] {
        &self.cell_runtime
    }

    /// Current-frame counter contributions aligned with [`Self::cells_raw`].
    pub fn visibility_marks_raw(&self) -> &[u16] {
        &self.visibility_marks
    }

    #[cfg(test)]
    fn set_fogged_object_snapshot(&mut self, rx: u16, ry: u16, present: bool) {
        let Some(index) = self.index(rx, ry) else {
            return;
        };
        self.ensure_cell_runtime();
        self.cell_runtime[index].set_fogged_object_snapshot(present);
    }

    pub(crate) fn shroud_knowledge_raw(&self) -> &[ShroudKnowledge] {
        &self.shroud_knowledge
    }

    fn ensure_cell_runtime(&mut self) {
        self.shroud_knowledge
            .resize(self.cells.len(), ShroudKnowledge::default());
        if self.cell_runtime.len() != self.cells.len() {
            self.cell_runtime
                .resize(self.cells.len(), CellVisibilityRuntime::default());
        }
        if self.visibility_marks.len() != self.cells.len() {
            self.visibility_marks.resize(self.cells.len(), 0);
        }
    }

    #[cfg(test)]
    fn resized_preserving_state(&self, width: u16, height: u16) -> Self {
        let mut expanded = Self::new(width, height);
        for ry in 0..self.height.min(height) {
            for rx in 0..self.width.min(width) {
                let old = ry as usize * self.width as usize + rx as usize;
                let new = ry as usize * width as usize + rx as usize;
                expanded.cells[new] = self.cells[old];
                expanded.shroud_knowledge[new] =
                    self.shroud_knowledge.get(old).copied().unwrap_or_default();
                if let Some(runtime) = self.cell_runtime.get(old) {
                    expanded.cell_runtime[new] = *runtime;
                }
                if let Some(marks) = self.visibility_marks.get(old) {
                    expanded.visibility_marks[new] = *marks;
                }
            }
        }
        expanded
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Merge revealed bits from a previous tick's grid into this one.
    /// Cells that were revealed before stay revealed even if no unit sees them now.
    #[cfg(test)]
    pub fn merge_revealed_from(&mut self, other: &OwnerVisibility) {
        // If dimensions differ, fall back to per-cell copy for the overlapping region.
        if self.width == other.width && self.height == other.height {
            for (dst, src) in self.cells.iter_mut().zip(other.cells.iter()) {
                *dst |= *src & FLAG_REVEALED;
            }
        } else {
            let overlap_w: u16 = self.width.min(other.width);
            let overlap_h: u16 = self.height.min(other.height);
            for ry in 0..overlap_h {
                for rx in 0..overlap_w {
                    if other.is_revealed(rx, ry) {
                        if let Some(i) = self.index(rx, ry) {
                            self.cells[i] |= FLAG_REVEALED;
                        }
                    }
                }
            }
        }
    }
}

/// Stable Rust identity for one native FoggedObjectClass allocation.
pub type FoggedObjectId = u64;

/// Shared frozen-building footprint ownership. Rendering payload is
/// deliberately absent until `FreezeInFog`'s draw-record fields are closed;
/// this record only represents the proven cross-cell lifetime contract.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FoggedObjectFootprintRecord {
    pub id: FoggedObjectId,
    /// VERA's per-house projection of native `CurrentPlayer` fog state.
    pub viewer: InternedId,
    pub source_entity_id: u64,
    /// Occupy-list order, with the anchor already applied and invalid cells
    /// omitted by the caller that owns map bounds.
    pub occupied_cells: Vec<(u16, u16)>,
}

/// Nonserialized merged-visibility cache for one owner (F10 `FogViewCache`).
///
/// Presentation-only: discarded by every snapshot load (serde skip) and
/// rebuilt before the first tactical render; never part of save bytes or the
/// state hash, so building it any number of times cannot affect determinism.
#[derive(Debug, Clone, Default)]
pub(crate) struct FogViewCache {
    /// The owner the merged grid was built for, plus the merged grid. All
    /// alliance-aware queries (is_cell_visible, edge masks) use this for
    /// O(1) lookups instead of iterating all owners per cell.
    pub(crate) merged: Option<(InternedId, OwnerVisibility)>,
    /// Bumps on every rebuild. The fog mask renderer and minimap dirty-gate
    /// on this runtime counter, never on the serialized wire shadow.
    pub(crate) generation: u64,
}

/// Last admitted generator identity/geometry, retained to distinguish a new
/// native6FB170 write from repeat Rust view materialization. Not a second map
/// authority: production always supplies the live entity's identity and facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub(crate) struct GapGeneratorSource {
    pub stable_id: u64,
    pub owner: InternedId,
    pub rx: u16,
    pub ry: u16,
    pub radius: i32,
}

/// Global fog/shroud state keyed by owner name.
///
/// Stores per-viewer knowledge grids plus a lazily-copied presentation cache
/// for fast queries. Direct alliance admission happens at fresh source writers,
/// never by merging derived viewer knowledge. The cache is built via
/// `build_merged_for()` and then used by `is_cell_visible`, edge masks, etc.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FogState {
    pub width: u16,
    pub height: u16,
    pub by_owner: BTreeMap<InternedId, OwnerVisibility>,
    pub alliances: HouseAllianceMap,
    pub(crate) sight_admissions: BTreeMap<(u64, InternedId), SightAdmission>,
    /// Native local House240: idempotent whole-map reveal, distinct from577A.
    pub(crate) whole_map_revealed_owners: std::collections::BTreeSet<InternedId>,
    /// Per-viewer hostile admission receipts; preserved across save/load so a
    /// continuing generator does not become a fresh write after restoration.
    pub(crate) gap_sources: BTreeMap<InternedId, std::collections::BTreeSet<GapGeneratorSource>>,
    /// The merged local-owner view (F10): nonserialized presentation cache.
    #[serde(skip)]
    pub(crate) view_cache: FogViewCache,
    /// Version-81 wire-compatibility shadow (F10): the exact serialized `u64`
    /// slot the pre-split view generation occupied, still updated in lockstep
    /// with the cache so round-trip bytes stay identical. Render consumes
    /// `view_generation()`, never this field. Do not bump SNAPSHOT_VERSION
    /// for this; retiring the slot waits for the next planned bump.
    pub generation_wire_shadow: u64,
    /// Native CellClass::FoggedObjects vectors, keyed by viewer and cell. IDs
    /// may be shared across every cell in one building footprint.
    #[serde(default)]
    pub fogged_object_cells: BTreeMap<(InternedId, u16, u16), Vec<FoggedObjectId>>,
    /// Owning allocation table for shared fogged-object IDs.
    #[serde(default)]
    pub fogged_objects: BTreeMap<FoggedObjectId, FoggedObjectFootprintRecord>,
    /// Allocation cursor for the Rust-stable counterpart of native pointers.
    #[serde(default)]
    pub next_fogged_object_id: FoggedObjectId,
    /// Native signed-word SensorsOfHouses counters, row-major per house.
    #[serde(default)]
    pub sensors_by_house: BTreeMap<InternedId, Vec<i16>>,
    /// Native `CellClass+0xAC[house]` signed-word disguise-detect counters,
    /// row-major per house. Only `BuildingClass::AddDetectDisguiseAt @
    /// 0x00455A80` / `RemoveDetectDisguiseAt @ 0x00455980` write them, and
    /// `FUN_004870F0` (`+0xAC[house] > 0`) is the only reader — consumed by
    /// `UnitClass::IsDisguisedTo @ 0x00746750` and
    /// `InfantryClass::IsDisguisedTo @ 0x005227F0`.
    #[serde(default)]
    pub disguise_detect_by_house: BTreeMap<InternedId, Vec<i16>>,
    /// Native CellClass::CloakedByHouses words, row-major by cell. House
    /// selection uses the original x86-masked bit index (`h & 31`).
    #[serde(default)]
    pub cloaked_by_houses: Vec<u32>,
}

impl FogState {
    /// Techno6F6AC0's ordinary release precedes its gap removal and Object
    /// Conceal. Only actual retained admissions emit the native leave event.
    pub(crate) fn release_entity_sight(&mut self, stable_id: u64) {
        let keys: Vec<_> = self
            .sight_admissions
            .range(
                (stable_id, InternedId::default())..=(stable_id, InternedId::from_index(u32::MAX)),
            )
            .map(|(&key, _)| key)
            .collect();
        for key in keys {
            self.release_sight_for_viewer(key);
        }
    }

    fn release_sight_for_viewer(&mut self, key: (u64, InternedId)) {
        let Some(admission) = self.sight_admissions.remove(&key) else {
            return;
        };
        let viewer = key.1;
        if let Some(vis) = self.by_owner.get_mut(&viewer) {
            for &(x, y) in &admission.cells {
                let Some(index) = vis.index(x, y) else {
                    continue;
                };
                let state = &mut vis.shroud_knowledge[index];
                state.leave();
                let count = if viewer == admission.owner {
                    &mut state.local_sources
                } else {
                    &mut state.allied_sources
                };
                *count = count
                    .checked_sub(1)
                    .expect("retained sight admission is balanced");
                vis.publish_knowledge(index);
            }
        }
        self.view_cache.merged = None;
    }

    fn reconcile_sight_admission(
        &mut self,
        stable_id: u64,
        viewer: InternedId,
        admission: SightAdmission,
        force_refresh: bool,
        fog_of_war: bool,
    ) {
        let key = (stable_id, viewer);
        let changed = force_refresh || self.sight_admissions.get(&key) != Some(&admission);
        if changed {
            self.release_sight_for_viewer(key);
        }
        let vis = self
            .by_owner
            .entry(viewer)
            .or_insert_with(|| OwnerVisibility::new(self.width, self.height));
        for &(x, y) in &admission.cells {
            let index = vis.index(x, y).expect("admitted source cell");
            // Legacy cache projection is not a source of knowledge events.
            vis.mark_visible_with_fog_of_war(x, y, fog_of_war);
            if changed {
                let state = &mut vis.shroud_knowledge[index];
                state.reveal();
                let count = if viewer == admission.owner {
                    &mut state.local_sources
                } else {
                    &mut state.allied_sources
                };
                *count = count.wrapping_add(1);
            }
            vis.publish_knowledge(index);
        }
        if changed {
            self.sight_admissions.insert(key, admission);
        }
        self.view_cache.merged = None;
    }

    /// Insert one shared frozen-building footprint record. Named location:
    /// `BuildingClass::FreezeInFog` installs the same FoggedObjectClass pointer
    /// into every occupy-list cell, not one allocation per cell.
    #[cfg(test)]
    pub fn insert_fogged_object_footprint(
        &mut self,
        viewer: InternedId,
        receiver_cell: (u16, u16),
        source_entity_id: u64,
        occupied_cells: Vec<(u16, u16)>,
    ) -> FoggedObjectId {
        let id = self.next_fogged_object_id.max(1);
        self.next_fogged_object_id = id.wrapping_add(1);
        let record = FoggedObjectFootprintRecord {
            id,
            viewer,
            source_entity_id,
            occupied_cells,
        };
        for &(rx, ry) in &record.occupied_cells {
            self.fogged_object_cells
                .entry((viewer, rx, ry))
                .or_default()
                .push(id);
        }
        self.fogged_objects.insert(id, record);
        if self.width > 0 && self.height > 0 {
            self.by_owner
                .entry(viewer)
                .or_insert_with(|| OwnerVisibility::new(self.width, self.height))
                .set_fogged_object_snapshot(receiver_cell.0, receiver_cell.1, true);
        }
        id
    }

    /// Clear one cell's vector in native reverse order. Each shared record is
    /// first unlinked by exact ID from all footprint cells, then returned to
    /// the caller in destruction/invalidation order. Other empty vectors stay
    /// allocated.
    ///
    /// **Provenance UNKNOWN.** The address this used to cite, `0x004802D0`, is
    /// not a function entry — it lands mid-`CALL` inside `FUN_004802A0`, which
    /// is a tile blitter (it reads `g_IsometricTileTypeClass_Array`, calls
    /// `IsometricTileTypeClass::SubtileHasDamagedData` and blits to
    /// `g_PrimarySurface`) and has nothing to do with fogged objects. And
    /// `ClearFoggedObjects` is not a symbol in this program. The address stays
    /// unbound rather than swapped for a positive match against an unrelated
    /// function.
    #[cfg(test)]
    pub fn clear_fogged_objects_at(
        &mut self,
        viewer: InternedId,
        rx: u16,
        ry: u16,
    ) -> Vec<FoggedObjectFootprintRecord> {
        if let Some(vis) = self.by_owner.get_mut(&viewer) {
            vis.set_fogged_object_snapshot(rx, ry, false);
        }
        let Some(mut ids) = self.fogged_object_cells.remove(&(viewer, rx, ry)) else {
            return Vec::new();
        };
        let mut removed = Vec::with_capacity(ids.len());
        while let Some(id) = ids.pop() {
            let Some(record) = self.fogged_objects.remove(&id) else {
                continue;
            };
            for &(record_rx, record_ry) in &record.occupied_cells {
                if (record_rx, record_ry) == (rx, ry) {
                    continue;
                }
                if let Some(cell_ids) = self
                    .fogged_object_cells
                    .get_mut(&(viewer, record_rx, record_ry))
                {
                    cell_ids.retain(|candidate| *candidate != id);
                }
            }
            removed.push(record);
        }
        removed
    }

    /// Recreate CellClass sensor storage for current map bounds. Production
    /// deposits are owned by `sim::sensor_lifecycle`; this remains the explicit
    /// map-storage recreation edge.
    pub fn reset_sensor_counts(&mut self) {
        self.sensors_by_house.clear();
        self.disguise_detect_by_house.clear();
    }

    /// Recreate the serialized CellClass cloak-owner words for current map
    /// bounds without inventing a cloak-generator mask producer.
    pub fn reset_cloaked_by_houses(&mut self) {
        self.cloaked_by_houses.clear();
        self.cloaked_by_houses
            .resize(usize::from(self.width) * usize::from(self.height), 0);
    }

    /// Native `FUN_00487110 (unlabelled; name not a symbol)`.
    #[cfg(test)]
    pub fn set_cloaked_by_house(&mut self, house_index: u8, rx: u16, ry: u16) -> bool {
        let Some(word) = self.cloak_word_mut(rx, ry) else {
            return false;
        };
        let bit = 1_u32 << (u32::from(house_index) & 31);
        let changed = *word & bit == 0;
        *word |= bit;
        changed
    }

    /// Native `FUN_00487130 (unlabelled; name not a symbol)`.
    #[cfg(test)]
    pub fn clear_cloaked_by_house(&mut self, house_index: u8, rx: u16, ry: u16) -> bool {
        let Some(word) = self.cloak_word_mut(rx, ry) else {
            return false;
        };
        let bit = 1_u32 << (u32::from(house_index) & 31);
        let changed = *word & bit != 0;
        *word &= !bit;
        changed
    }

    /// Tests bit `house` of `CellClass+0x78` — the word
    /// `CellClass::IsVisibleToHouse @ 0x004870B0` reads, whose body is exactly
    /// `return (this->dwVisibleToHouses & 1 << (house & 0x1F)) != 0;`.
    ///
    /// **The Ghidra label is drift and the field is NOT shroud visibility.**
    /// `get_xrefs_to 0x00487110 / 0x00487130` (the only OR/AND-NOT writers of
    /// `+0x78`) returns exactly two callsites, both inside
    /// `BuildingClass::UpdateGapGenerator_Tick @ 0x004551B9 / 0x004553B3`, and
    /// that write branch is gated on `BuildingType+0x16C7`
    /// (`BuildingTypeClass::ReadINI @ 0x00460C06`, key string `0x0081A998` =
    /// `CloakGenerator`). This is YRpp's `CloakedByHouses` — the cloak-field
    /// footprint of a `CloakGenerator=yes` building. **No stock YR building
    /// sets `CloakGenerator=`** (`grep -c '^CloakGenerator' ini/rulesmd.ini`
    /// is zero), so gamemd itself never sets the bit in an ordinary skirmish.
    ///
    /// VERA therefore keeps this plane write-dead ON PURPOSE — that is parity,
    /// not a gap. Its three consumers (`TechnoClass::ShouldUncloak @
    /// 0x006FBDA2`, `CanAutoCloak @ 0x006FBE90`, and the vt+0x420 cloak-field
    /// entry hook at `0x006F4F46`) all read a constantly-false bit in stock,
    /// and VERA's counterparts must read this and not cell visibility.
    /// `cloaked_by_houses` is folded into the world hash, so wiring a producer
    /// (a mod with a cloak generator) would shift every golden.
    pub fn is_cloaked_by_house(&self, house_index: u8, rx: u16, ry: u16) -> bool {
        let Some(word) = self.cloak_word(rx, ry) else {
            return false;
        };
        let bit = 1_u32 << (u32::from(house_index) & 31);
        word & bit != 0
    }

    fn cloak_word(&self, rx: u16, ry: u16) -> Option<u32> {
        if rx >= self.width || ry >= self.height {
            return None;
        }
        let index = usize::from(ry) * usize::from(self.width) + usize::from(rx);
        self.cloaked_by_houses.get(index).copied()
    }

    #[cfg(test)]
    fn cloak_word_mut(&mut self, rx: u16, ry: u16) -> Option<&mut u32> {
        if rx >= self.width || ry >= self.height {
            return None;
        }
        let cell_count = usize::from(self.width) * usize::from(self.height);
        if self.cloaked_by_houses.len() != cell_count {
            self.reset_cloaked_by_houses();
        }
        let index = usize::from(ry) * usize::from(self.width) + usize::from(rx);
        self.cloaked_by_houses.get_mut(index)
    }

    /// Native `TechnoClass::AddSensorsAt @ 0x004DE7B0`: outer-Y/inner-X strict
    /// circle and signed-word increment. Returned cells are the exact ordered
    /// boundary where native forces resident objects through virtual `+0x420`.
    /// RESIDUAL (GSI-12.06) — the deposit walk is right; everything around it is
    /// missing, and the radius pass 1 named is the wrong key.
    /// - The radius is `SensorsSight=` (`TechnoTypeClass+0x5F0`), not `Sensors=`.
    ///   `Sensors=` is a separate rule: an adjacent enemy carrying it forces a
    ///   cloaked object to surface.
    /// - The callers are FootClass-only: `Unlimbo @ 0x004D7318`, `Limbo @
    ///   0x004DB376`, `PerCellProcess` removing the old cell at `0x004D8611` and
    ///   adding the new one at `0x004D8621`, and `ChangeOwner @ 0x004DBEFB` /
    ///   `0x004DBF81`. Buildings deposit through their own
    ///   `BuildingClass::AddSensorArrayAt @ 0x00455820`. VERA's production
    ///   lifecycle now routes these writers through `sim::sensor_lifecycle`.
    /// - The detection rule is: a cloaked object is visible to house H when H
    ///   has a sensor count at the object's OWN cell, or when H and the owner
    ///   are mutually allied. `DetectDisguise=` is a separate predicate and is
    ///   not parsed.
    /// - Trigger: any `SensorsSight=` unit near a submerged submarine.
    /// - Player effect: live radar registration consumes this plane; tactical
    ///   cloak rendering still has separately owned presentation work.
    /// - Frequency: continuous around stock submarines and detector units.
    /// - Downstream risk: sensor-driven resident-object `+0x420` reevaluation
    ///   and cloak-generator ownership remain separate mechanisms.
    #[cfg(test)]
    pub fn sensors_add_at(
        &mut self,
        house: InternedId,
        center: (u16, u16),
        radius: u16,
    ) -> Vec<(u16, u16)> {
        let touched = self.sensor_circle_cells(center, radius);
        for &(rx, ry) in &touched {
            self.increment_sensor_at(house, rx, ry);
        }
        touched
    }

    /// Paired `TechnoClass::RemoveSensorsAt` @ `0x004DE940` decrement walk.
    #[cfg(test)]
    pub fn sensors_remove_at(
        &mut self,
        house: InternedId,
        center: (u16, u16),
        radius: u16,
    ) -> Vec<(u16, u16)> {
        let touched = self.sensor_circle_cells(center, radius);
        for &(rx, ry) in &touched {
            let _ = self.decrement_sensor_at_if_positive(house, rx, ry);
        }
        touched
    }

    /// Exact outer-Y / inner-X strict-circle cell order shared by the four
    /// active sensor writers. Mutation stays separate so Simulation can issue
    /// each native resident callback immediately after that cell's counter.
    pub(crate) fn sensor_circle_cells(&self, center: (u16, u16), radius: u16) -> Vec<(u16, u16)> {
        let radius = i32::from(radius);
        if radius <= 0 || self.width == 0 || self.height == 0 {
            return Vec::new();
        }
        let mut touched = Vec::new();
        for dy in -radius..radius {
            for dx in -radius..radius {
                if dx * dx + dy * dy >= radius * radius {
                    continue;
                }
                let cell_x = i32::from(center.0) + dx;
                let cell_y = i32::from(center.1) + dy;
                if cell_x < 0
                    || cell_y < 0
                    || cell_x >= i32::from(self.width)
                    || cell_y >= i32::from(self.height)
                {
                    continue;
                }
                let rx = cell_x as u16;
                let ry = cell_y as u16;
                touched.push((rx, ry));
            }
        }
        touched
    }

    fn sensor_counter_mut(&mut self, house: InternedId, rx: u16, ry: u16) -> &mut i16 {
        let cell_count = usize::from(self.width) * usize::from(self.height);
        let counters = self
            .sensors_by_house
            .entry(house)
            .or_insert_with(|| vec![0; cell_count]);
        if counters.len() != cell_count {
            counters.clear();
            counters.resize(cell_count, 0);
        }
        let index = usize::from(ry) * usize::from(self.width) + usize::from(rx);
        &mut counters[index]
    }

    pub(crate) fn increment_sensor_at(&mut self, house: InternedId, rx: u16, ry: u16) {
        let counter = self.sensor_counter_mut(house, rx, ry);
        *counter = counter.wrapping_add(1);
    }

    /// Unit `RemoveSensorsAt @ 0x004DE940` queries `count > 0` before both the
    /// decrement and resident callbacks. Returns whether native entered them.
    pub(crate) fn decrement_sensor_at_if_positive(
        &mut self,
        house: InternedId,
        rx: u16,
        ry: u16,
    ) -> bool {
        let counter = self.sensor_counter_mut(house, rx, ry);
        if *counter <= 0 {
            return false;
        }
        *counter = counter.wrapping_sub(1);
        true
    }

    /// BuildingClass::RemoveSensorArrayAt @ 0x004556D0 decrements the signed
    /// word unconditionally. Its asymmetric remove radius therefore creates
    /// active negative fringe counts.
    pub(crate) fn decrement_sensor_at_unconditional(
        &mut self,
        house: InternedId,
        rx: u16,
        ry: u16,
    ) {
        let counter = self.sensor_counter_mut(house, rx, ry);
        *counter = counter.wrapping_sub(1);
    }

    pub fn has_sensor_for_house(&self, house: InternedId, rx: u16, ry: u16) -> bool {
        if rx >= self.width || ry >= self.height {
            return false;
        }
        let index = usize::from(ry) * usize::from(self.width) + usize::from(rx);
        self.sensors_by_house
            .get(&house)
            .and_then(|counters| counters.get(index))
            .is_some_and(|count| *count > 0)
    }

    fn disguise_detect_counter_mut(&mut self, house: InternedId, rx: u16, ry: u16) -> &mut i16 {
        let cell_count = usize::from(self.width) * usize::from(self.height);
        let counters = self
            .disguise_detect_by_house
            .entry(house)
            .or_insert_with(|| vec![0; cell_count]);
        if counters.len() != cell_count {
            counters.clear();
            counters.resize(cell_count, 0);
        }
        let index = usize::from(ry) * usize::from(self.width) + usize::from(rx);
        &mut counters[index]
    }

    /// `BuildingClass::AddDetectDisguiseAt @ 0x00455A80` — the same strict
    /// outer-Y/inner-X circle the sensor deposit walks, incrementing
    /// `CellClass+0xAC[house]` (`CellClass::IncrementDisguiseDetectCount`).
    /// The caller owns the powered gate (native vt+0x350).
    pub fn disguise_detect_add_at(
        &mut self,
        house: InternedId,
        center: (u16, u16),
        radius: u16,
    ) -> Vec<(u16, u16)> {
        let touched = self.sensor_circle_cells(center, radius);
        for &(rx, ry) in &touched {
            let counter = self.disguise_detect_counter_mut(house, rx, ry);
            *counter = counter.wrapping_add(1);
        }
        touched
    }

    /// `BuildingClass::RemoveDetectDisguiseAt @ 0x00455980`. Unlike the sensor
    /// array's remove, this one reads the SAME `DetectDisguiseRange=` field
    /// (`0x00455991` loads `+0x5F4`) that the add used, so the walk is
    /// symmetric and leaves no fringe residue.
    pub fn disguise_detect_remove_at(
        &mut self,
        house: InternedId,
        center: (u16, u16),
        radius: u16,
    ) -> Vec<(u16, u16)> {
        let touched = self.sensor_circle_cells(center, radius);
        for &(rx, ry) in &touched {
            let counter = self.disguise_detect_counter_mut(house, rx, ry);
            *counter = counter.wrapping_sub(1);
        }
        touched
    }

    /// `FUN_004870F0` — `CellClass+0xAC[house] > 0`. The observer-side half of
    /// `IsDisguisedTo`: a house covering the cell sees through the disguise.
    pub fn detects_disguise_for_house(&self, house: InternedId, rx: u16, ry: u16) -> bool {
        if rx >= self.width || ry >= self.height {
            return false;
        }
        let index = usize::from(ry) * usize::from(self.width) + usize::from(rx);
        self.disguise_detect_by_house
            .get(&house)
            .and_then(|counters| counters.get(index))
            .is_some_and(|count| *count > 0)
    }

    /// Native `CellClass::DrawObjectsCloaked`: no observer-mode bypass.
    #[cfg(test)]
    pub fn draw_objects_cloaked(
        &self,
        current_player: Option<InternedId>,
        object_owner: InternedId,
        object_owner_index: u8,
        rx: u16,
        ry: u16,
    ) -> bool {
        let Some(current_player) = current_player else {
            return false;
        };
        if !self.is_cloaked_by_house(object_owner_index, rx, ry) {
            return false;
        }
        current_player == object_owner || !self.has_sensor_for_house(current_player, rx, ry)
    }

    /// Cache the selected viewer's already-resolved knowledge. The historical
    /// method/cache name is retained for callers and serialized generation shadow;
    /// this does not merge another viewer's derived knowledge or visibility.
    /// Fresh Techno/Psychic writers own direct-alliance publication at5678E0.
    pub fn build_merged_for(&mut self, owner: InternedId, _interner: &StringInterner) {
        // Reuse existing buffer if dimensions match; otherwise allocate.
        let mut merged = match self.view_cache.merged.take() {
            Some((_, mut vis)) if vis.width == self.width && vis.height == self.height => {
                vis.clear_all();
                vis
            }
            _ => OwnerVisibility::new(self.width, self.height),
        };
        //5678E0 applies each fresh source's direct-alliance gate before the
        //Cell writer. Stored planes already belong to viewers; OR-ing another
        //viewer here would re-export derived knowledge through A-B-C alliances.
        if let Some(viewer) = self.by_owner.get(&owner) {
            if viewer.width == self.width && viewer.height == self.height {
                merged.cells.copy_from_slice(&viewer.cells);
            } else {
                // Fixture auto-expansion can leave an older viewer rectangle.
                for y in 0..viewer.height.min(self.height) {
                    for x in 0..viewer.width.min(self.width) {
                        merged.cells[usize::from(y) * usize::from(self.width) + usize::from(x)] =
                            viewer.cells
                                [usize::from(y) * usize::from(viewer.width) + usize::from(x)];
                    }
                }
            }
        }
        self.view_cache.merged = Some((owner, merged));
        self.view_cache.generation = self.view_cache.generation.wrapping_add(1);
        // Kept in lockstep purely for v81 byte compatibility (see field doc).
        self.generation_wire_shadow = self.generation_wire_shadow.wrapping_add(1);
    }

    /// The runtime view-cache generation render dirty-gates on (F10). Resets
    /// with the cache on every load; never the serialized wire shadow.
    pub fn view_generation(&self) -> u64 {
        self.view_cache.generation
    }

    /// Get the selected viewer cache; callers fall back to its authoritative plane.
    fn merged_vis(&self, owner: InternedId) -> Option<&OwnerVisibility> {
        if let Some((cached_owner, ref vis)) = self.view_cache.merged {
            if cached_owner == owner {
                return Some(vis);
            }
        }
        None
    }

    /// Native586360 tests Cell+12C bit8, which persists independently of the
    /// current sight flag. Gameplay reads the viewer authority directly;
    /// building a presentation cache cannot change destination admission.
    pub(crate) fn is_ground_unshrouded(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        self.by_owner.get(&owner).is_some_and(|view| {
            view.index(rx, ry)
                .and_then(|i| view.cell_runtime.get(i))
                .is_some_and(|cell| cell.alt_flags & CellVisibilityRuntime::ALT_GROUND_VISIBLE != 0)
        })
    }

    /// Aircraft FindFireLocation419986 reads Cell+12C bit16 directly. This is
    /// independent of bit8 and of the presentation's merged sight cache.
    pub(crate) fn is_ground_open(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        self.by_owner.get(&owner).is_some_and(|view| {
            view.index(rx, ry)
                .and_then(|i| view.cell_runtime.get(i))
                .is_some_and(|cell| cell.alt_flags & CellVisibilityRuntime::ALT_GROUND_OPEN != 0)
        })
    }

    /// Returns true if the owner (or a friendly ally) currently sees the cell.
    pub fn is_cell_visible(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        // Fast path: use pre-merged grid.
        if let Some(vis) = self.merged_vis(owner) {
            return vis.is_visible(rx, ry);
        }
        // Identical viewer authority when no presentation cache has been built.
        self.by_owner
            .get(&owner)
            .is_some_and(|s| s.is_visible(rx, ry))
    }

    /// Returns true if the owner (or a friendly ally) has revealed the cell.
    pub fn is_cell_revealed(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        if let Some(vis) = self.merged_vis(owner) {
            return vis.is_revealed(rx, ry);
        }
        self.by_owner
            .get(&owner)
            .is_some_and(|s| s.is_revealed(rx, ry))
    }

    /// Returns true if the cell is covered by an enemy gap generator for this owner.
    pub fn is_cell_gap_covered(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        if let Some(vis) = self.merged_vis(owner) {
            return vis.is_gap_covered(rx, ry);
        }
        self.by_owner
            .get(&owner)
            .is_some_and(|s| s.is_gap_covered(rx, ry))
    }

    /// Returns true if the cell is covered by a friendly gap generator for this owner.
    pub fn is_cell_gap_fog(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        if let Some(vis) = self.merged_vis(owner) {
            return vis.is_gap_fog(rx, ry);
        }
        self.by_owner
            .get(&owner)
            .is_some_and(|s| s.is_gap_fog(rx, ry))
    }

    /// Returns true if two owners should be treated as friendly.
    pub fn is_friendly(&self, a: &str, b: &str) -> bool {
        are_houses_friendly(&self.alliances, a, b)
    }

    /// Returns true if two interned owners should be treated as friendly.
    pub fn is_friendly_id(&self, a: InternedId, b: InternedId, interner: &StringInterner) -> bool {
        a == b || are_houses_friendly(&self.alliances, interner.resolve(a), interner.resolve(b))
    }

    /// Clear all explored/revealed state for the given owner.
    /// Used by spy infiltration to reset an enemy's map knowledge.
    #[cfg(test)]
    pub fn reset_explored_for_owner(&mut self, owner: InternedId) {
        let cells = self.rectangular_cells();
        self.transition_whole_map_for_owner(owner, cells, true, false);
    }

    /// Clear the transient enemy/friendly Gap result on every viewer plane.
    pub fn clear_gap_flags(&mut self) {
        for visibility in self.by_owner.values_mut() {
            visibility.clear_gap_flags();
        }
        self.view_cache.merged = None;
    }

    /// Original Logic55B29A/55B2AD ->578100: signed native frame modulo120.
    /// Pending survives removal; a fresh reveal cancels it only if already open.
    pub(crate) fn flush_pending_gap_conceal(&mut self, native_frame: i32) {
        if native_frame % 120 != 0 {
            return;
        }
        for vis in self.by_owner.values_mut() {
            vis.ensure_cell_runtime();
            for index in 0..vis.cells.len() {
                vis.shroud_knowledge[index].sweep();
                vis.publish_knowledge(index);
            }
        }
        self.view_cache.merged = None;
    }

    /// Lift unexplored shroud for one viewer without granting current sight.
    pub fn reveal_all_for_owner(&mut self, owner: InternedId) {
        let cells = self.rectangular_cells();
        self.transition_whole_map_for_owner(owner, cells, false, false);
    }

    /// Whole-map reveal over the actual allocated-cell iterator. Repeated
    /// materialization is suppressed by the native House240 counterpart.
    pub fn reveal_cells_for_owner<I>(&mut self, owner: InternedId, cells: I)
    where
        I: IntoIterator<Item = (u16, u16)>,
    {
        self.transition_whole_map_for_owner(owner, cells.into_iter().collect(), false, false);
    }

    pub(crate) fn rectangular_cells(&self) -> Vec<(u16, u16)> {
        (0..self.height)
            .flat_map(|y| (0..self.width).map(move |x| (x, y)))
            .collect()
    }

    /// 4-bit neighbor mask for shroud edge rendering.
    ///
    /// Returns a mask where each bit indicates that the corresponding iso
    /// edge-sharing neighbor is ALSO shrouded (never revealed). A set bit means
    /// the neighbor is in the same state (shrouded), so no edge fade is needed
    /// on that side.
    ///
    /// Bit layout matches the diamond's 4 edges (same as LAT adjacency):
    /// Bit 0 = NE (rx, ry-1), Bit 1 = SE (rx+1, ry), Bit 2 = SW (rx, ry+1),
    /// Bit 3 = NW (rx-1, ry).
    ///
    /// Out-of-bounds neighbors are treated as shrouded (bit set).
    #[cfg(test)]
    pub fn shroud_edge_mask(&self, owner: InternedId, rx: u16, ry: u16) -> u8 {
        let mut mask: u8 = 0;
        if ry == 0 || !self.is_cell_revealed(owner, rx, ry - 1) {
            mask |= 0x01;
        }
        if !self.is_cell_revealed(owner, rx + 1, ry) {
            mask |= 0x02;
        }
        if !self.is_cell_revealed(owner, rx, ry + 1) {
            mask |= 0x04;
        }
        if rx == 0 || !self.is_cell_revealed(owner, rx - 1, ry) {
            mask |= 0x08;
        }
        mask
    }

    /// 8-bit neighbor mask for SHROUD.SHP edge rendering.
    ///
    /// Each bit is SET when that neighbor IS shrouded (unexplored).
    /// The 8-bit value indexes directly into the 256-byte frame lookup table
    /// to select which SHROUD.SHP frame to render.
    ///
    /// Only meaningful for cells that ARE revealed — call on explored cells only.
    ///
    /// Bit layout (cell-relative dx,dy):
    /// ```text
    ///   NW(-1,-1)=bit6   N(0,-1)=bit7   NE(+1,-1)=bit0
    ///   W(-1, 0)=bit5       *            E(+1, 0)=bit1
    ///   SW(-1,+1)=bit4   S(0,+1)=bit3   SE(+1,+1)=bit2
    /// ```
    ///
    /// Out-of-bounds neighbors are treated as shrouded (bit set).
    pub fn shroud_edge_mask_8bit(&self, owner: InternedId, rx: u16, ry: u16) -> u8 {
        let mut mask: u8 = 0;
        // bit 0 = NE (+1, -1)
        if ry == 0 || !self.is_cell_revealed(owner, rx + 1, ry - 1) {
            mask |= 0x01;
        }
        // bit 1 = E (+1, 0)
        if !self.is_cell_revealed(owner, rx + 1, ry) {
            mask |= 0x02;
        }
        // bit 2 = SE (+1, +1)
        if !self.is_cell_revealed(owner, rx + 1, ry + 1) {
            mask |= 0x04;
        }
        // bit 3 = S (0, +1)
        if !self.is_cell_revealed(owner, rx, ry + 1) {
            mask |= 0x08;
        }
        // bit 4 = SW (-1, +1)
        if rx == 0 || !self.is_cell_revealed(owner, rx - 1, ry + 1) {
            mask |= 0x10;
        }
        // bit 5 = W (-1, 0)
        if rx == 0 || !self.is_cell_revealed(owner, rx - 1, ry) {
            mask |= 0x20;
        }
        // bit 6 = NW (-1, -1)
        if rx == 0 || ry == 0 || !self.is_cell_revealed(owner, rx - 1, ry - 1) {
            mask |= 0x40;
        }
        // bit 7 = N (0, -1)
        if ry == 0 || !self.is_cell_revealed(owner, rx, ry - 1) {
            mask |= 0x80;
        }
        mask
    }

    /// Test helper: mark a cell visible for the given owner.
    /// Auto-expands the grid dimensions if needed so tests don't need to
    /// pre-set width/height.
    #[cfg(test)]
    pub fn mark_visible_for_owner(&mut self, owner: InternedId, rx: u16, ry: u16) {
        let needed_w: u16 = rx.saturating_add(1);
        let needed_h: u16 = ry.saturating_add(1);
        if self.width < needed_w {
            self.width = needed_w;
        }
        if self.height < needed_h {
            self.height = needed_h;
        }
        let w = self.width;
        let h = self.height;
        let state = self
            .by_owner
            .entry(owner)
            .or_insert_with(|| OwnerVisibility::new(w, h));
        if state.width() < w || state.height() < h {
            *state = state.resized_preserving_state(w, h);
        }
        state.mark_visible(rx, ry);
    }
}

/// Configuration for visibility computation, passed to `recompute_owner_visibility`.
pub struct VisionConfig {
    /// When true, Techno reveal/update readers require the canonical stored
    /// TechnoClass+0x3D5 membership byte. Headless fixtures without live
    /// MapClass authority leave this false.
    pub require_playfield_membership: bool,
    /// `[General] VeteranSight=` (`RulesClass+0x680`), the multiplier a
    /// `SIGHT`-ability holder applies to its elevation-scaled sight. Stock
    /// `0.0` disables it (see `veterancy::veteran_sight_cells`).
    pub veteran_sight: f64,
    /// Leptons of elevation per +1 sight cell (from [General] LeptonsPerSightIncrease=).
    /// 256 leptons = 1 z-level. 0 disables the elevation bonus.
    pub leptons_per_sight_increase: i32,
    /// Height-based LOS obstruction (from [General] RevealByHeight=).
    /// When true, terrain 4+ levels above the viewer at the midpoint blocks sight.
    /// Default true (the standard RA2/YR setting).
    pub reveal_by_height: bool,
    /// Scenario `FogOfWar=` governs the first-map `CleanFog` transition. It
    /// does not change the compact visibility bitmap's existing semantics.
    pub fog_of_war: bool,
}

impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            require_playfield_membership: false,
            veteran_sight: 0.0,
            leptons_per_sight_increase: 0,
            reveal_by_height: true,
            fog_of_war: false,
        }
    }
}

/// Recompute deterministic fog/shroud state for all owners (allocating variant).
///
/// Creates a fresh `FogState` and populates it. Used by tests; production code
/// calls `recompute_owner_visibility_in_place` to avoid per-tick allocation.
#[cfg(test)]
pub fn recompute_owner_visibility(
    entities: &EntityStore,
    path_grid: Option<&PathGrid>,
    alliances: &HouseAllianceMap,
    config: &VisionConfig,
    interner: &crate::sim::intern::StringInterner,
) -> FogState {
    recompute_owner_visibility_with_rules(entities, path_grid, alliances, config, interner, None)
}

/// [`recompute_owner_visibility`] with the rules the `SIGHT` ability gate
/// needs; the rules-less form reads every object as having no ability list.
pub fn recompute_owner_visibility_with_rules(
    entities: &EntityStore,
    path_grid: Option<&PathGrid>,
    alliances: &HouseAllianceMap,
    config: &VisionConfig,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> FogState {
    let mut fog = FogState::default();
    recompute_owner_visibility_in_place(
        &mut fog, entities, path_grid, alliances, config, None, interner, rules,
    );
    fog
}

/// The `SIGHT`-ability half of `TechnoClass::UpdateReveal @ 0x0070AF50`'s
/// veteran gate (`0x0070B01E..0x0070B07A`), resolved off the type's ability
/// arrays through the shared `HasWeaponAbility` predicate. No rules — as in
/// headless fixtures — reads as no ability.
pub(crate) fn entity_has_sight_ability(
    entity: &crate::sim::game_entity::GameEntity,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> bool {
    rules
        .and_then(|rules| rules.object(interner.resolve(entity.type_ref())))
        .is_some_and(|object| {
            crate::sim::combat::veterancy::has_weapon_ability(
                crate::sim::combat::veterancy::rank_of(entity.veterancy_raw),
                object,
                crate::rules::object_type::Ability::Sight,
            )
        })
}

/// Recompute deterministic fog/shroud visibility in-place, reusing existing grids.
///
/// Clears `FLAG_VISIBLE` on all existing owner grids (preserving `FLAG_REVEALED`),
/// then re-reveals from entity positions. New owners get a fresh grid; dead owners
/// keep their revealed state with no visible cells.
///
/// This avoids the per-tick allocation of `Vec<u8>` grids and the subsequent
/// `merge_revealed_from` pass — revealed bits are never destroyed.
pub fn recompute_owner_visibility_in_place(
    fog: &mut FogState,
    entities: &EntityStore,
    path_grid: Option<&PathGrid>,
    alliances: &HouseAllianceMap,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) {
    // Construction-seeded session bounds are authoritative; the lazy
    // derivation stays only as the fallback for fixture sims built without a
    // descriptor (zero-dim fog).
    let (width, height) = if fog.width > 0 && fog.height > 0 {
        (fog.width, fog.height)
    } else {
        resolve_bounds(entities, path_grid)
    };
    if width == 0 || height == 0 {
        *fog = FogState::default();
        return;
    }

    // First tick or dimension change: recreate all grids (cold path).
    if fog.width != width || fog.height != height {
        fog.by_owner.clear();
        fog.gap_sources.clear();
        fog.sight_admissions.clear();
        fog.whole_map_revealed_owners.clear();
        fog.fogged_object_cells.clear();
        fog.fogged_objects.clear();
        fog.next_fogged_object_id = 0;
        fog.reset_sensor_counts();
        fog.width = width;
        fog.height = height;
    } else {
        // Hot path: clear visible flags, preserve revealed.
        for vis in fog.by_owner.values_mut() {
            vis.clear_all_visible();
        }
    }

    fog.alliances = alliances.clone();
    fog.view_cache.merged = None;

    let admitted: Vec<_> = entities
        .values()
        .filter(|entity| {
            !entity.dying
                && !entity.lifecycle.in_limbo
                && (!config.require_playfield_membership || entity.in_playfield)
                && !entity.passenger_role.is_inside_transport()
        })
        .collect();
    let identities: std::collections::BTreeSet<_> =
        admitted.iter().map(|entity| entity.stable_id()).collect();
    let removed: Vec<_> = fog
        .sight_admissions
        .keys()
        .copied()
        .filter(|key| !identities.contains(&key.0))
        .collect();
    for key in removed {
        fog.release_sight_for_viewer(key);
    }
    for entity in admitted {
        let sight_ability = entity_has_sight_ability(entity, interner, rules);
        reveal_entity_vision(fog, entity, config, height_grid, sight_ability, interner);
    }
}

/// Run the same effective-sight writer used by the ordinary Techno
/// reveal/update pass for one already-admitted entity.
///
/// This is also the exact Rust-owned callback boundary for the false->true
/// TechnoClass+0x3D5 transition in `MapClass::Set_Clipped_LocalSize`: action 40
/// must not fall back to the raw `Sight=` radius and thereby lose elevation,
/// veterancy, shifted-center, or height-LOS semantics.
pub(crate) fn reveal_entity_vision(
    fog: &mut FogState,
    entity: &crate::sim::game_entity::GameEntity,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    sight_ability: bool,
    interner: &StringInterner,
) {
    update_entity_sight_admission(
        fog,
        entity,
        config,
        height_grid,
        sight_ability,
        interner,
        false,
        None,
    );
}

/// Explicit admitted release/readmit event (e.g. FootAI4DA6F7/4DA706),
/// distinct from unchanged view reconciliation. The caller owns its timer/gates.
pub(crate) fn force_refresh_entity_vision(
    fog: &mut FogState,
    entity: &crate::sim::game_entity::GameEntity,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    sight_ability: bool,
    interner: &StringInterner,
) {
    update_entity_sight_admission(
        fog,
        entity,
        config,
        height_grid,
        sight_ability,
        interner,
        true,
        None,
    );
}

pub(crate) fn refresh_entity_vision_for_viewer(
    fog: &mut FogState,
    entity: &crate::sim::game_entity::GameEntity,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    sight_ability: bool,
    interner: &StringInterner,
    viewer: InternedId,
) {
    update_entity_sight_admission(
        fog,
        entity,
        config,
        height_grid,
        sight_ability,
        interner,
        true,
        Some(viewer),
    );
}

fn update_entity_sight_admission(
    fog: &mut FogState,
    entity: &crate::sim::game_entity::GameEntity,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    sight_ability: bool,
    interner: &StringInterner,
    force_refresh: bool,
    only_viewer: Option<InternedId>,
) {
    let width = fog.width;
    let height = fog.height;
    if width == 0 || height == 0 {
        return;
    }
    let height_leptons: i32 = entity_height_leptons(entity);

    // Elevation raises sight MULTIPLICATIVELY, off the object's world Z in
    // leptons, not additively off its terrain level:
    //   sight = trunc(Sight * (1 + 0.10 * trunc(Z_leptons / LeptonsPerSightIncrease)))
    let base_range: i32 = entity.vision_range as i32;
    let elev_steps: i32 = if config.leptons_per_sight_increase > 0 {
        height_leptons / config.leptons_per_sight_increase
    } else {
        0
    };
    let with_elevation: i32 =
        (base_range * (100 + ELEVATION_SIGHT_PERCENT_PER_STEP * elev_steps)) / 100;
    // `TechnoClass::UpdateReveal @ 0x0070AF50`: the elevation-scaled integer
    // is then `ftol(sight * Rules.VeteranSight)` for a `SIGHT` holder, gated
    // on the double not being exactly 0.0 (`0x0070B082..0x0070B0A4`).
    let with_veterancy: i32 = crate::sim::combat::veterancy::veteran_sight_cells(
        with_elevation,
        sight_ability,
        config.veteran_sight,
    );
    let effective: u16 = (with_veterancy.max(0) as u16).min(MAX_SIGHT_RANGE);
    let cells = collect_reveal_cells(
        entity.position.rx,
        entity.position.ry,
        effective,
        height_leptons,
        config.reveal_by_height,
        height_grid,
        width,
        height,
    );
    let viewers = only_viewer.map_or_else(
        || direct_reveal_viewers(fog, entity.owner(), interner),
        |viewer| vec![viewer],
    );
    if only_viewer.is_none() {
        let removed: Vec<_> = fog
            .sight_admissions
            // Source-prefix range: never rescan all S*V admissions per entity.
            .range(
                (entity.stable_id(), InternedId::default())
                    ..=(entity.stable_id(), InternedId::from_index(u32::MAX)),
            )
            .map(|(&key, _)| key)
            .filter(|key| !viewers.contains(&key.1))
            .collect();
        for key in removed {
            fog.release_sight_for_viewer(key);
        }
    }
    let admission = SightAdmission {
        owner: entity.owner(),
        origin: (entity.position.rx, entity.position.ry, height_leptons),
        radius: effective,
        fog_of_war: config.fog_of_war,
        cells,
    };
    for viewer in viewers {
        fog.reconcile_sight_admission(
            entity.stable_id(),
            viewer,
            admission.clone(),
            force_refresh,
            config.fog_of_war,
        );
    }
}

fn resolve_bounds(entities: &EntityStore, path_grid: Option<&PathGrid>) -> (u16, u16) {
    if let Some(grid) = path_grid {
        return (grid.width(), grid.height());
    }

    let mut max_x = 0u16;
    let mut max_y = 0u16;
    let mut found = false;
    for entity in entities.values() {
        if entity.lifecycle.in_limbo {
            continue;
        }
        found = true;
        max_x = max_x.max(entity.position.rx);
        max_y = max_y.max(entity.position.ry);
    }
    if found {
        (max_x.saturating_add(1), max_y.saturating_add(1))
    } else {
        (0, 0)
    }
}

/// Mark all cells within `range` of `(center_rx, center_ry)` as visible+revealed.
///
/// Iterates the engine's reveal spiral table — `REVEAL_SPIRAL[0 .. RING_SIZES[sight]]`
/// — with no special case at any radius. Every entry, ring 10 included, passes
/// through the same height line-of-sight gate.
///
/// ## Elevation Z-shift
/// The spiral is centered on the viewer's *screen* cell, not its raw foot cell.
/// A raised object's sprite renders toward isometric north, so the engine shifts
/// the reveal center by the same whole number of cells to keep the revealed
/// footprint under the sprite. Without this an elevated unit over-reveals toward
/// isometric south, and an aircraft lifts shroud under its shadow instead of
/// under itself. The shift is applied unconditionally (independent of
/// `reveal_by_height`).
///
/// The height-LOS obstruction check is *not* affected by the shift: in the
/// engine the shift cancels out of the obstruction-cell math, leaving it
/// relative to the raw foot cell. We reproduce that by adding `z_shift` back when
/// computing the obstruction cell below.
///
/// `viewer_height_leptons` is the viewer's world Z — terrain elevation plus any
/// flight altitude — because that is the single quantity the engine feeds to
/// both the shift and the LOS viewer level.
// Geometry-only adapter for existing kernel fixtures; production owns mapping
// events in the source/fire/Psychic writers above, never in this test adapter.
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn reveal_radius_into(
    vis: &mut OwnerVisibility,
    rx: u16,
    ry: u16,
    range: u16,
    height: i32,
    by_height: bool,
    fog_of_war: bool,
    height_grid: Option<&[u8]>,
    width: u16,
    grid_height: u16,
) {
    for (x, y) in collect_reveal_cells(
        rx,
        ry,
        range,
        height,
        by_height,
        height_grid,
        width,
        grid_height,
    ) {
        vis.mark_visible_with_fog_of_war(x, y, fog_of_war);
    }
}

fn collect_reveal_cells(
    center_rx: u16,
    center_ry: u16,
    range: u16,
    viewer_height_leptons: i32,
    reveal_by_height: bool,
    height_grid: Option<&[u8]>,
    width: u16,
    height: u16,
) -> Vec<(u16, u16)> {
    if range == 0 {
        return Vec::new();
    }

    let mut cells = Vec::new();
    let viewer_level = viewer_height_leptons / LEPTONS_PER_HEIGHT_LEVEL;
    let z_shift = iso_height_shift_cells(viewer_height_leptons);
    let cx = i32::from(center_rx) - z_shift;
    let cy = i32::from(center_ry) - z_shift;
    let w = i32::from(width);
    let h = i32::from(height);

    // Clamp range to MAX_SIGHT_RANGE (the original also clamps to 10).
    let clamped = (range as usize).min(MAX_SIGHT_RANGE as usize);
    let spiral_end = REVEAL_RING_SIZES[clamped];

    for i in 0..spiral_end {
        let (dx, dy) = REVEAL_SPIRAL[i];
        let rx = cx + dx as i32;
        let ry = cy + dy as i32;
        if rx >= 0 && rx < w && ry >= 0 && ry < h {
            // Height-based LOS: check whether terrain at the obstruction cell
            // blocks sight. The original engine samples the cell at
            // `foot_target + mirror[i] + (2, 2)` — the per-entry mirror steps one
            // cell back toward the viewer, plus a fixed +2 on each axis baked into
            // the original's obstruction math. The obstruction is relative to the
            // raw foot cell, so we add `z_shift` back to undo the spiral's Z-shift
            // (in the original this cancellation is implicit). If that cell's Level
            // exceeds viewer_level + 3, the target is not revealed (LOS blocked).
            if reveal_by_height {
                if let Some(hg) = height_grid {
                    let (mdx, mdy) = REVEAL_MIRROR[i];
                    let obs_x = rx + mdx as i32 + 2 + z_shift;
                    let obs_y = ry + mdy as i32 + 2 + z_shift;
                    if obs_x >= 0 && obs_x < w && obs_y >= 0 && obs_y < h {
                        let obs_level = hg[(obs_y * w + obs_x) as usize] as i32;
                        if viewer_level + 3 < obs_level {
                            continue; // terrain blocks LOS
                        }
                    }
                }
            }
            cells.push((rx as u16, ry as u16));
        }
    }
    cells
}

/// Reveal spiral table extracted from the original engine.
/// Each (dx, dy) is a cell offset from the revealing unit's position.
/// Entries are ordered in expanding rings by sight radius.
///
/// Recovered whole from the engine's table initialiser, which writes every
/// entry as a literal `(dy << 16) | dx` store or a two-argument coordinate
/// call — the table itself lives in zero-initialised data, so reading the
/// image gives nothing. Ring membership is exactly
/// `max(|dx|,|dy|) + min(|dx|,|dy|)/2 == r` (truncating division), which
/// independently reproduces all twelve cumulative counts in
/// [`REVEAL_RING_SIZES`].
#[rustfmt::skip]
const REVEAL_SPIRAL: [(i8, i8); 309] = [
    // Sight 0: 1 entry
    (0, 0),
    // Sight 1: entries 1..9 (8 new)
    (1, -1), (0, -1), (-1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1),
    // Sight 2: entries 9..21 (12 new)
    (-1, -2), (0, -2), (1, -2), (-2, -1), (2, -1), (-2, 0), (2, 0), (-2, 1), (2, 1), (-1, 2),
    (0, 2), (1, 2),
    // Sight 3: entries 21..37 (16 new)
    (-1, -3), (0, -3), (1, -3), (-2, -2), (2, -2), (-3, -1), (3, -1), (-3, 0), (3, 0), (-3, 1),
    (3, 1), (-2, 2), (2, 2), (-1, 3), (0, 3), (1, 3),
    // Sight 4: entries 37..61 (24 new)
    (-1, -4), (0, -4), (1, -4), (-3, -3), (-2, -3), (2, -3), (3, -3), (-3, -2), (3, -2),
    (-4, -1), (4, -1), (-4, 0), (4, 0), (-4, 1), (4, 1), (-3, 2), (3, 2), (-3, 3), (-2, 3),
    (2, 3), (3, 3), (-1, 4), (0, 4), (1, 4),
    // Sight 5: entries 61..89 (28 new)
    (-1, -5), (0, -5), (1, -5), (-3, -4), (-2, -4), (2, -4), (3, -4), (-4, -3), (4, -3),
    (-4, -2), (4, -2), (-5, -1), (5, -1), (-5, 0), (5, 0), (-5, 1), (5, 1), (-4, 2), (4, 2),
    (-4, 3), (4, 3), (-3, 4), (-2, 4), (2, 4), (3, 4), (-1, 5), (0, 5), (1, 5),
    // Sight 6: entries 89..121 (32 new)
    (-1, -6), (0, -6), (1, -6), (-3, -5), (-2, -5), (2, -5), (3, -5), (-4, -4), (4, -4),
    (-5, -3), (5, -3), (-5, -2), (5, -2), (-6, -1), (6, -1), (-6, 0), (6, 0), (-6, 1), (6, 1),
    (-5, 2), (5, 2), (-5, 3), (5, 3), (-4, 4), (4, 4), (-3, 5), (-2, 5), (2, 5), (3, 5),
    (-1, 6), (0, 6), (1, 6),
    // Sight 7: entries 121..161 (40 new)
    (-1, -7), (0, -7), (1, -7), (-3, -6), (-2, -6), (2, -6), (3, -6), (-5, -5), (-4, -5),
    (4, -5), (5, -5), (-5, -4), (5, -4), (-6, -3), (6, -3), (-6, -2), (6, -2), (-7, -1), (7, -1),
    (-7, 0), (7, 0), (-7, 1), (7, 1), (-6, 2), (6, 2), (-6, 3), (6, 3), (-5, 4), (5, 4),
    (-5, 5), (-4, 5), (4, 5), (5, 5), (-3, 6), (-2, 6), (2, 6), (3, 6), (-1, 7), (0, 7), (1, 7),
    // Sight 8: entries 161..205 (44 new)
    (-1, -8), (0, -8), (1, -8), (-3, -7), (-2, -7), (2, -7), (3, -7), (-5, -6), (-4, -6),
    (4, -6), (5, -6), (-6, -5), (6, -5), (-6, -4), (6, -4), (-7, -3), (7, -3), (-7, -2), (7, -2),
    (-8, -1), (8, -1), (-8, 0), (8, 0), (-8, 1), (8, 1), (-7, 2), (7, 2), (-7, 3), (7, 3),
    (-6, 4), (6, 4), (-6, 5), (6, 5), (-5, 6), (-4, 6), (4, 6), (5, 6), (-3, 7), (-2, 7),
    (2, 7), (3, 7), (-1, 8), (0, 8), (1, 8),
    // Sight 9: entries 205..253 (48 new)
    // Native applies two further per-entry filters — `|dx| <= sight` and
    // `ftol(Sqrt_Approx(dx² + dy²)) <= sight` — that VERA omits. Both are
    // **provably inert over this whole table**, not merely unobserved: with ring
    // membership `r = a + floor(b/2)` for `a = max(|dx|,|dy|)`, `b = min(...)`,
    // `|dx| <= a <= r` always; and `a² + b² < (a + floor(b/2) + 1)²` reduces to
    // `3k² + 2k < 2a(k + 1)` with `k = floor(b/2)`, which holds for every entry
    // under truncation, and to `0 < k² + k + 0.25` under round-to-nearest.
    (-1, -9), (0, -9), (1, -9), (-3, -8), (-2, -8), (2, -8), (3, -8), (-5, -7), (-4, -7),
    (4, -7), (5, -7), (-6, -6), (6, -6), (-7, -5), (7, -5), (-7, -4), (7, -4), (-8, -3), (8, -3),
    (-8, -2), (8, -2), (-9, -1), (9, -1), (-9, 0), (9, 0), (-9, 1), (9, 1), (-8, 2), (8, 2),
    (-8, 3), (8, 3), (-7, 4), (7, 4), (-7, 5), (7, 5), (-6, 6), (6, 6), (-5, 7), (-4, 7),
    (4, 7), (5, 7), (-3, 8), (-2, 8), (2, 8), (3, 8), (-1, 9), (0, 9), (1, 9),
    // Sight 10: entries 253..309 (56 new)
    (-1, -10), (0, -10), (1, -10), (-3, -9), (-2, -9), (2, -9), (3, -9), (-5, -8), (-4, -8),
    (4, -8), (5, -8), (-7, -7), (-6, -7), (6, -7), (7, -7), (-7, -6), (7, -6), (-8, -5), (8, -5),
    (-8, -4), (8, -4), (-9, -3), (9, -3), (-9, -2), (9, -2), (-10, -1), (10, -1), (-10, 0),
    (10, 0), (-10, 1), (10, 1), (-9, 2), (9, 2), (-9, 3), (9, 3), (-8, 4), (8, 4), (-8, 5),
    (8, 5), (-7, 6), (7, 6), (-7, 7), (-6, 7), (6, 7), (7, 7), (-5, 8), (-4, 8), (4, 8), (5, 8),
    (-3, 9), (-2, 9), (2, 9), (3, 9), (-1, 10), (0, 10), (1, 10),
];

/// Cumulative entry count for each sight radius 0–10, read from the engine's
/// read-only data. To reveal cells for sight N, iterate
/// `REVEAL_SPIRAL[0..REVEAL_RING_SIZES[N]]`.
///
/// The table continues past this with 369 for sight 11, which the kernel's
/// clamp to 10 makes unreachable for object reveals.
const REVEAL_RING_SIZES: [usize; 11] = [1, 9, 21, 37, 61, 89, 121, 161, 205, 253, 309];

/// Mirror/direction table for height-based LOS checks (RevealByHeight).
///
/// Each entry corresponds to the same index in `REVEAL_SPIRAL`. The (mdx, mdy)
/// offset is added to the target cell position to find the obstruction cell — the
/// cell one step closer to the viewer along the line of sight. If that cell's
/// terrain Level exceeds `viewer_level + 3`, LOS is blocked.
///
/// Recovered from the engine's mirror-table initialiser the same way as
/// [`REVEAL_SPIRAL`]. That table stops at 309 entries — one per spiral entry
/// the sight clamp can reach — which is why this one does too.
#[rustfmt::skip]
const REVEAL_MIRROR: [(i8, i8); 309] = [
    // Sight 0: 1 entry
    (0, 0),
    // Sight 1: entries 1..9 (8 new)
    (-1, 1), (0, 1), (1, 1), (1, 0), (-1, 0), (1, -1), (0, -1), (-1, -1),
    // Sight 2: entries 9..21 (12 new)
    (1, 1), (0, 1), (-1, 1), (1, 1), (-1, 1), (1, 0), (-1, 0), (1, -1), (-1, -1), (1, -1),
    (0, -1), (-1, -1),
    // Sight 3: entries 21..37 (16 new)
    (0, 1), (0, 1), (0, 1), (1, 1), (-1, 1), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0),
    (-1, 0), (1, -1), (-1, -1), (0, -1), (0, -1), (0, -1),
    // Sight 4: entries 37..61 (24 new)
    (0, 1), (0, 1), (0, 1), (1, 1), (1, 1), (-1, 1), (-1, 1), (1, 1), (-1, 1), (1, 0),
    (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, -1), (-1, -1), (1, -1), (1, -1), (-1, -1),
    (-1, -1), (0, -1), (0, -1), (0, -1),
    // Sight 5: entries 61..89 (28 new)
    (0, 1), (0, 1), (0, 1), (1, 1), (1, 1), (-1, 1), (-1, 1), (1, 1), (-1, 1), (1, 1),
    (-1, 1), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, -1), (-1, -1), (1, -1),
    (-1, -1), (1, -1), (1, -1), (-1, -1), (-1, -1), (0, -1), (0, -1), (0, -1),
    // Sight 6: entries 89..121 (32 new)
    (0, 1), (0, 1), (0, 1), (1, 1), (0, 1), (0, 1), (-1, 1), (1, 1), (-1, 1), (1, 1),
    (-1, 1), (1, 0), (1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0),
    (-1, 0), (1, -1), (-1, -1), (1, -1), (-1, -1), (1, -1), (0, -1), (0, -1), (-1, -1), (0, -1),
    (0, -1), (0, -1),
    // Sight 7: entries 121..161 (40 new)
    (0, 1), (0, 1), (0, 1), (1, 1), (0, 1), (0, 1), (-1, 1), (1, 1), (1, 1), (-1, 1),
    (-1, 1), (1, 1), (-1, 1), (1, 1), (-1, 1), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0),
    (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, -1), (-1, -1), (1, -1), (-1, -1), (1, -1),
    (1, -1), (-1, -1), (-1, -1), (1, -1), (0, -1), (0, -1), (-1, -1), (0, -1), (0, -1), (0, -1),
    // Sight 8: entries 161..205 (44 new)
    (0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (1, 1), (1, 1), (-1, 1),
    (-1, 1), (1, 1), (-1, 1), (1, 1), (-1, 1), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0),
    (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, -1),
    (-1, -1), (1, -1), (-1, -1), (1, -1), (1, -1), (-1, -1), (-1, -1), (0, -1), (0, -1), (0, -1),
    (0, -1), (0, -1), (0, -1), (0, -1),
    // Sight 9: entries 205..253 (48 new)
    (0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (1, 1), (1, 1), (-1, 1),
    (-1, 1), (1, 1), (-1, 1), (1, 1), (-1, 1), (1, 1), (-1, 1), (1, 0), (-1, 0), (1, 0),
    (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0),
    (-1, 0), (1, -1), (-1, -1), (1, -1), (-1, -1), (1, -1), (-1, -1), (1, -1), (1, -1), (-1, -1),
    (-1, -1), (0, -1), (0, -1), (0, -1), (0, -1), (0, -1), (0, -1), (0, -1),
    // Sight 10: entries 253..309 (56 new)
    (0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (0, 1), (1, 1), (1, 1), (-1, 1),
    (-1, 1), (1, 1), (1, 1), (-1, 1), (-1, 1), (1, 1), (-1, 1), (1, 1), (-1, 1), (1, 1),
    (-1, 1), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, 0),
    (-1, 0), (1, 0), (-1, 0), (1, 0), (-1, 0), (1, -1), (-1, -1), (1, -1), (-1, -1), (1, -1),
    (-1, -1), (1, -1), (1, -1), (-1, -1), (-1, -1), (1, -1), (1, -1), (-1, -1), (-1, -1),
    (0, -1), (0, -1), (0, -1), (0, -1), (0, -1), (0, -1), (0, -1),
];

/// `MapClass::IsShrouded @ 0x00586360`: the Cell under `point`, projected up
/// by its height level, is not yet mapped (`open` answers the Cell's
/// ground-open bit, `+0x12C & 0x08`); an odd level also tests the next cell
/// (`0x00481810(3)`).
pub(crate) fn coordinate_is_shrouded(
    cells: &crate::map::resolved_terrain::NativeCellQuery<'_>,
    point: crate::sim::components::DriveCoord,
    open: &impl Fn(crate::map::cell_index::NativeCellIdentity) -> Result<bool, String>,
) -> Result<bool, String> {
    let level = point.z / crate::util::lepton::GROUND_LEVEL_HEIGHT_LEPTONS;
    let shift = level / 2 + i32::from(level & 1 != 0);
    let first = cells.lookup((
        ((point.x / 256) as i16).wrapping_sub(shift as i16),
        ((point.y / 256) as i16).wrapping_sub(shift as i16),
    ));
    if open(first)? {
        return Ok(false);
    }
    if level & 1 != 0 {
        let (x, y) = cells.coord(first);
        let next = cells.lookup((x.wrapping_add(1), y.wrapping_add(1)));
        return open(next).map(|open| !open);
    }
    Ok(true)
}

/// A flat fire-leaf reveal of `range` cells around a cell, with no height
/// shift or height line of sight (fixtures and flat callers).
pub fn reveal_radius(
    fog: &mut FogState,
    owner: InternedId,
    center_rx: u16,
    center_ry: u16,
    range: u16,
) {
    let cells = collect_reveal_cells(
        center_rx, center_ry, range, 0, false, None, fog.width, fog.height,
    );
    fire_reveal_cells(fog, owner, cells);
}

/// `MapClass::RevealShroud @ 0x005673A0` as FireAt's RevealOnFire calls it
/// (`0x006FF6F2`): radius 3 around the firer's coordinate, centred on its
/// height-shifted cell, with `RevealByHeight=`'s line of sight (arg 7 = 1),
/// for `house`'s map. Each cell takes the fire leaf `0x004876F0`.
pub(crate) fn reveal_shroud_on_fire(
    fog: &mut FogState,
    house: InternedId,
    coord: crate::sim::components::DriveCoord,
    reveal_by_height: bool,
    height_grid: Option<&[u8]>,
) {
    if coord.x < 0 || coord.y < 0 {
        return;
    }
    let cells = collect_reveal_cells(
        (coord.x / 256) as u16,
        (coord.y / 256) as u16,
        REVEAL_ON_FIRE_RADIUS,
        coord.z,
        reveal_by_height,
        height_grid,
        fog.width,
        fog.height,
    );
    fire_reveal_cells(fog, house, cells);
}

/// FireAt's reveal radius (`PUSH 0x3` at `0x006FF6E0`).
const REVEAL_ON_FIRE_RADIUS: u16 = 3;

/// The fire leaf `0x004876F0` over collected cells: unlike Psychic's
/// reduce/increase pair, it never changes the shroud counter and tests
/// counter > 0 for pending.
fn fire_reveal_cells(fog: &mut FogState, owner: InternedId, cells: Vec<(u16, u16)>) {
    let width = fog.width;
    let height = fog.height;
    if width == 0 || height == 0 {
        return;
    }
    let vis = fog
        .by_owner
        .entry(owner)
        .or_insert_with(|| OwnerVisibility::new(width, height));
    for (rx, ry) in cells {
        vis.mark_visible_with_fog_of_war(rx, ry, true);
        let index = vis.index(rx, ry).expect("collected cell is in bounds");
        vis.shroud_knowledge[index].fire_unshroud();
        vis.shroud_knowledge[index].transient_visible = true;
        vis.publish_knowledge(index);
    }
    fog.view_cache.merged = None;
}

pub(crate) fn direct_reveal_viewers(
    fog: &FogState,
    owner: InternedId,
    interner: &StringInterner,
) -> Vec<InternedId> {
    let mut candidates: std::collections::BTreeSet<_> = fog.by_owner.keys().copied().collect();
    candidates.insert(owner);
    for (house, allies) in &fog.alliances {
        for name in std::iter::once(house).chain(allies) {
            if let Some(id) = interner.get(name) {
                candidates.insert(id);
            }
        }
    }
    candidates
        .into_iter()
        .filter(|&viewer| {
            are_houses_friendly(
                &fog.alliances,
                interner.resolve(owner),
                interner.resolve(viewer),
            )
        })
        .collect()
}

/// Publish a fresh transient reveal to the source and its direct allied viewers.
/// Psychic6CD773/6CD79C route through5678E0's source-House/AllyReveal gate.
/// Snapshot/display merges must not stand in for this writer: derived knowledge
/// from A's view is not a new B-owned reveal that can reach C. Uses the existing
/// admitted alliance policy (ordinary retail AllyReveal=yes); the full optional
/// policy is a separate owner. Fire5673A0 keeps its separately owned admission.
pub(crate) fn reveal_radius_for_direct_allies(
    fog: &mut FogState,
    owner: InternedId,
    center_rx: u16,
    center_ry: u16,
    range: u16,
    interner: &StringInterner,
) {
    let viewers = direct_reveal_viewers(fog, owner, interner);
    let cells = collect_reveal_cells(
        center_rx, center_ry, range, 0, false, None, fog.width, fog.height,
    );
    //6CD773 final0 completes before6CD79C final1. No retained Techno source.
    for release in [false, true] {
        for &viewer in &viewers {
            let vis = fog
                .by_owner
                .entry(viewer)
                .or_insert_with(|| OwnerVisibility::new(fog.width, fog.height));
            for &(rx, ry) in &cells {
                vis.mark_visible_with_fog_of_war(rx, ry, true);
                let index = vis.index(rx, ry).expect("collected cell is in bounds");
                if release {
                    vis.shroud_knowledge[index].leave();
                } else {
                    vis.shroud_knowledge[index].reveal();
                }
                vis.shroud_knowledge[index].transient_visible = true;
                vis.publish_knowledge(index);
            }
        }
    }
    fog.view_cache.merged = None;
}

/// Materialize active SpySat house latches by marking every synthetic-grid cell
/// **revealed** for those owners. Production world code routes the same write
/// through the native allocated-cell iterator instead. Call after normal vision.
///
/// gamemd's whole-map reveal sets only the explored bit on every cell. It does
/// not create a "currently in sight" state — with `FogOfWar=no` no such per-cell
/// state exists at all — so the uplink lifts the shroud and nothing more. It
/// must not mark cells *visible* here: that layer is VERA-internal and gates
/// combat target acquisition, so writing it map-wide would let every unit
/// acquire across the whole map, which the engine never does.
///
/// Writing it repeatedly is idempotent. The persisted per-house aggregate latch
/// decides activation and last-provider loss; this helper only materializes the
/// active owners and never infers a transition from an absent list entry.
///
/// Takes the owner names whose persisted SpySat latch is active.
#[cfg(test)]
pub fn apply_spy_sat(
    fog: &mut FogState,
    spy_sat_owners: &[InternedId],
    _interner: &StringInterner,
) {
    for &owner_id in spy_sat_owners {
        fog.reveal_all_for_owner(owner_id);
    }
}

/// Materialize selected GapGenerator coverage after ordinary sight.
/// Native6FB170 preserves cells with a sustained reveal contribution; fire-only
/// and Psychic-only mapping does not provide that immunity. Native6FB470 leaves
/// already-concealed knowledge erased unless the House577A SpySatActive gate restores
/// it. Leaving sight under a gap schedules the578100 periodic conceal instead of
/// immediately destroying knowledge. See PHASE3_SHROUD_CURRENT_SIGHT_NATIVE_REPORT.
///
/// Radius/power/owner admission is supplied by the existing production collector;
/// this does not certify its entire Building4555D0 operational policy. Friendly
/// gap fog remains the existing separate projection of native Cell+13C.
#[cfg(test)]
pub fn apply_gap_generators(
    fog: &mut FogState,
    gap_generators: &[(InternedId, u16, u16, i32)],
    interner: &StringInterner,
) {
    let sources: Vec<_> = gap_generators
        .iter()
        .enumerate()
        .map(|(index, &(owner, rx, ry, radius))| GapGeneratorSource {
            stable_id: index as u64,
            owner,
            rx,
            ry,
            radius,
        })
        .collect();
    apply_gap_generator_sources(fog, &sources, interner);
}

#[cfg(test)]
pub(crate) fn apply_gap_generator_sources(
    fog: &mut FogState,
    gap_generators: &[GapGeneratorSource],
    interner: &StringInterner,
) {
    apply_gap_generator_sources_with_spy_sat(
        fog,
        gap_generators,
        interner,
        &std::collections::BTreeSet::new(),
    );
}

#[cfg(test)]
pub(crate) fn apply_gap_generator_sources_with_spy_sat(
    fog: &mut FogState,
    gap_generators: &[GapGeneratorSource],
    interner: &StringInterner,
    spy_sat_active_owners: &std::collections::BTreeSet<InternedId>,
) {
    let width = usize::from(fog.width);
    let height = usize::from(fog.height);
    if width == 0 || height == 0 {
        return;
    }
    for (&viewer, vis) in &mut fog.by_owner {
        vis.ensure_cell_runtime();
        let previous = fog.gap_sources.entry(viewer).or_default();
        let mut admitted = std::collections::BTreeSet::new();
        for cell in &mut vis.cells {
            *cell &= !FLAG_GAP_FOG;
        }
        for &generator in gap_generators {
            if generator.radius <= 0 {
                continue;
            }
            if !are_houses_friendly(
                &fog.alliances,
                interner.resolve(generator.owner),
                interner.resolve(viewer),
            ) {
                admitted.insert(generator);
            } else {
                for index in gap_footprint(generator, width, height) {
                    vis.cells[index] |= FLAG_GAP_FOG;
                }
            }
        }
        // Reconciliation is not a new generator write. Only actual source
        // removal/admission executes6FB470/6FB170, preserving pending20.
        for &generator in previous.difference(&admitted) {
            for index in gap_footprint(generator, width, height) {
                vis.shroud_knowledge[index].remove_gap(spy_sat_active_owners.contains(&viewer));
            }
        }
        for &generator in admitted.difference(previous) {
            for index in gap_footprint(generator, width, height) {
                vis.shroud_knowledge[index].add_gap();
            }
        }
        for index in 0..vis.cells.len() {
            vis.publish_knowledge(index);
        }
        *previous = admitted;
    }
    fog.view_cache.merged = None;
}

fn gap_footprint(generator: GapGeneratorSource, width: usize, height: usize) -> Vec<usize> {
    let (cx, cy, radius) = (
        i32::from(generator.rx),
        i32::from(generator.ry),
        generator.radius,
    );
    if radius <= 0 {
        return Vec::new();
    }
    let threshold = (radius + 1) * (radius + 1);
    let mut cells = Vec::new();
    for y in (cy - radius).max(0)..=(cy + radius).min(height as i32 - 1) {
        for x in (cx - radius).max(0)..=(cx + radius).min(width as i32 - 1) {
            if (x - cx) * (x - cx) + (y - cy) * (y - cy) < threshold {
                cells.push(y as usize * width + x as usize);
            }
        }
    }
    cells
}

/// One admitted6FB170/6FB470 event. The Building caller owns its269 latch;
/// this ledger owns which viewer cell writes that deposit contributed.
pub(crate) fn publish_gap_generator_event(
    fog: &mut FogState,
    viewer: InternedId,
    source: GapGeneratorSource,
    active: bool,
    interner: &StringInterner,
    spy_sat_active_owners: &std::collections::BTreeSet<InternedId>,
) {
    // Both complete6FB170/6FB470 leaves clear local House240 after an
    // admitted269 transition, including friendly events with no hostile cells.
    fog.whole_map_revealed_owners.remove(&viewer);
    let width = usize::from(fog.width);
    let height = usize::from(fog.height);
    if let Some(vis) = fog.by_owner.get_mut(&viewer) {
        vis.ensure_cell_runtime();
        let receipts = fog.gap_sources.entry(viewer).or_default();
        if active {
            if !are_houses_friendly(
                &fog.alliances,
                interner.resolve(source.owner),
                interner.resolve(viewer),
            ) && receipts.insert(source)
            {
                for index in gap_footprint(source, width, height) {
                    vis.shroud_knowledge[index].add_gap();
                    vis.publish_knowledge(index);
                }
            }
        } else {
            // Source-prefix lookup avoids scanning every generator on every
            // Building event. Static GAGAP keeps its admitted cell geometry;
            // mobile/warp/owner-transfer callback policy is outside this owner.
            let lower = GapGeneratorSource {
                stable_id: source.stable_id,
                owner: InternedId::default(),
                rx: 0,
                ry: 0,
                radius: i32::MIN,
            };
            let removed: Vec<_> = receipts
                .range(lower..)
                .take_while(|old| old.stable_id == source.stable_id)
                .copied()
                .collect();
            for old in removed {
                receipts.remove(&old);
                for index in gap_footprint(old, width, height) {
                    vis.shroud_knowledge[index].remove_gap(spy_sat_active_owners.contains(&viewer));
                    vis.publish_knowledge(index);
                }
            }
        }
    }
    fog.view_cache.merged = None;
}

/// Passive projection of retained deposits. It must never reclassify power or
/// emit add/remove events during House, restore, or ordinary view rebuilding.
/// Friendly Cell13C remains the existing boolean fog projection.
pub(crate) fn materialize_gap_generator_sources(
    fog: &mut FogState,
    sources: &BTreeMap<InternedId, Vec<GapGeneratorSource>>,
    interner: &StringInterner,
) {
    let width = usize::from(fog.width);
    let height = usize::from(fog.height);
    if width == 0 || height == 0 {
        return;
    }
    for (&viewer, vis) in &mut fog.by_owner {
        // Preserve the existing per-viewer receipt container, including empty
        // sets. Materialization does not admit or remove any generator.
        fog.gap_sources.entry(viewer).or_default();
        vis.ensure_cell_runtime();
        for cell in &mut vis.cells {
            *cell &= !FLAG_GAP_FOG;
        }
        for &source in sources.get(&viewer).into_iter().flatten() {
            if are_houses_friendly(
                &fog.alliances,
                interner.resolve(source.owner),
                interner.resolve(viewer),
            ) {
                for index in gap_footprint(source, width, height) {
                    vis.cells[index] |= FLAG_GAP_FOG;
                }
            }
        }
        for index in 0..vis.cells.len() {
            vis.publish_knowledge(index);
        }
    }
    fog.view_cache.merged = None;
}

#[cfg(test)]
mod vision_tests;

#[cfg(test)]
mod adjust_for_z_tests {
    use super::{height_lift_px, iso_height_shift_cells};

    #[test]
    fn adjust_for_z_reveal_shift_uses_retail_integer_lift() {
        assert_eq!(height_lift_px(104), 15);
        assert_eq!(height_lift_px(256), 37);
        assert_eq!(height_lift_px(727), 104);
        assert_eq!(height_lift_px(728), 105);
        assert_eq!(height_lift_px(1_500), 216);
        assert_eq!(height_lift_px(-400), -56);
        assert_eq!(iso_height_shift_cells(1_500), 7);
        assert_eq!(iso_height_shift_cells(-400), -1);
    }
}
