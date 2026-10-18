//! Deterministic fog/shroud visibility state computed from unit vision radii.
//!
//! Each cell has two independent flags: "revealed" (seen at least once) and
//! "visible" (inside a source's current sight). State is stored in a flat
//! Vec<u8> grid per owner for O(1) lookup.
//!
//! A source's sight moves only at native reveal events
//! (`Simulation::refresh_object_sight`):
//! `ReReveal @ 0x0070B1D0` then `UpdateReveal @ 0x0070AF50`, from Unit and
//! Infantry cell arrivals, the Jumpjet descent, Fly's phase transition, the
//! high-flyer refresh and the Spy Plane. Between events its footprint stays
//! where the last one put it (the Simulation sight-frame projection).
//!
//! ## Performance
//! Direct source-aware writes publish viewer knowledge. Queries use a cached
//! visibility grid so each cell lookup is O(1) instead of iterating all owners.

mod cell_reveal;
mod map_reveal;
pub(crate) use map_reveal::MapTransition;
mod shroud_knowledge;
pub use cell_reveal::SHROUD_EDGE_LUT;
#[cfg(test)]
pub(crate) use cell_reveal::tests::{
    assert_native_cell_history, discovery_corpus, native_map_cell_fixture,
};
use cell_reveal::{DummyVisibility, EdgeCells, MapCellEdgeCache};
pub(crate) use cell_reveal::{ListedEdgeCells, MapCellNotification, MapRevealProjection};
use shroud_knowledge::ShroudKnowledge;
pub(crate) use shroud_knowledge::SightAdmission;
pub(crate) use shroud_knowledge::{SightFacts, SightRefreshTimers};

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::map::houses::{HouseAllianceMap, are_houses_friendly};
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::{InternedId, StringInterner};
use crate::sim::movement::ground_pose;
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
// This is the live bitmap authority; the ground bits must not drive this
// transition.
const FLAG_PENDING_GAP_CONCEAL: u8 = 0x20;
const FLAG_HOSTILE_GAP_PRESENT: u8 = 0x40;
// Effective viewer sight is never re-exported as a local allied source.
const FLAG_EFFECTIVE_GAP_SIGHT: u8 = 0x80;

/// CellClass `+0x12C` ground bits as VERA's sight cache and the cell writers
/// keep them: ground visible (`0x08`) and ground open (`0x10`). Sight sets
/// both; `0x004A9CA0` sets or clears both with `ShroudKnowledge::open`;
/// `0x004AA050` sets `0x08`, and `0x10` where its frame is open. The gap
/// and some whole-map writers move `ShroudKnowledge::open` alone (#1117),
/// so `MapClass::IsShrouded` reads that copy ([`cell_ground_mapped`]); the
/// edge frames and the aircraft fire-location query
/// ([`FogState::is_ground_open`]) read this one.
const GROUND_VISIBLE: u8 = 0x08;
const GROUND_OPEN: u8 = 0x10;

/// The cell flags [`published_flags`] derives from a cell's knowledge; the
/// friendly gap fog is the only flag outside them.
const PUBLISHED_FLAGS: u8 = FLAG_REVEALED
    | FLAG_VISIBLE
    | FLAG_GAP_COVERED
    | FLAG_SUSTAINED_SIGHT
    | FLAG_EFFECTIVE_GAP_SIGHT
    | FLAG_PENDING_GAP_CONCEAL
    | FLAG_HOSTILE_GAP_PRESENT;

/// The published flags of a cell with `knowledge`.
fn published_flags(knowledge: ShroudKnowledge) -> u8 {
    let mut flags = 0;
    if knowledge.open {
        flags |= FLAG_REVEALED;
    }
    if knowledge.local_sources > 0 {
        flags |= FLAG_SUSTAINED_SIGHT;
    }
    if knowledge.allied_sources > 0 {
        flags |= FLAG_EFFECTIVE_GAP_SIGHT;
    }
    if knowledge.open
        && (knowledge.local_sources > 0
            || knowledge.allied_sources > 0
            || knowledge.transient_visible)
    {
        flags |= FLAG_VISIBLE;
    }
    if knowledge.pending {
        flags |= FLAG_PENDING_GAP_CONCEAL;
    }
    if knowledge.gap_counter > 0 {
        flags |= FLAG_HOSTILE_GAP_PRESENT;
        if !knowledge.open {
            flags |= FLAG_GAP_COVERED;
        }
    }
    flags
}

/// Map5678E0 clamps its visited disc to10. Native source cached260 retains
/// the raw effective radius; the independent border traversal clamps to11.
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

/// Original567F70 conceal geometry, distinct from RevealArea's spiral and
/// sight-height geometry. AdjustForZ6D20E0 is divided by-30 with signed
/// truncation, then applied in leptons before signed /256 and WORD storage.
/// DX is the outer loop and the strict signed squared-radius test wraps32.
/// Executed geometry/counter controls: trigger_conceal_area.json (Steam
/// 3e81a617); no RNG, timer or detach in this traversal.
pub(crate) fn conceal_area_cells(coord: [i32; 3], radius: i32) -> Vec<(u16, u16)> {
    let shift = crate::util::native_x87::adjust_for_z_standard(coord[2]) / -30;
    let x = (coord[0].wrapping_add(shift.wrapping_shl(8)) / 256) as i16;
    let y = (coord[1].wrapping_add(shift.wrapping_shl(8)) / 256) as i16;
    let lower = (-2i32).wrapping_sub(radius);
    let upper = radius.wrapping_add(2);
    let squared_radius = radius.wrapping_add(1).wrapping_mul(radius.wrapping_add(1));
    let mut cells = Vec::new();
    for dx in lower..upper {
        for dy in lower..upper {
            if dx.wrapping_mul(dx).wrapping_add(dy.wrapping_mul(dy)) < squared_radius {
                cells.push((
                    x.wrapping_add(dx as i16) as u16,
                    y.wrapping_add(dy as i16) as u16,
                ));
            }
        }
    }
    cells
}

/// Per-owner visibility stored as a flat grid of flag bytes.
///
/// Indexed by `ry * width + rx`. Each byte holds public visibility plus
/// sustained-sight, current hostile coverage and pending-conceal provenance. This gives O(1) per-cell lookups instead of O(log n)
/// with the previous BTreeSet design.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OwnerVisibility {
    cells: Vec<u8>,
    width: u16,
    height: u16,
    /// [`GROUND_VISIBLE`] / [`GROUND_OPEN`] per cell, aligned with `cells`.
    ground_flags: Vec<u8>,
    shroud_knowledge: Vec<ShroudKnowledge>,
    map_cell_edge_cache: Vec<MapCellEdgeCache>,
    dummy_visibility: DummyVisibility,
    /// The cells the per-frame passes must visit; never saved or hashed.
    #[serde(skip)]
    tracked: TrackedCells,
    /// The cells the next shroud-edge update must visit; never saved or
    /// hashed.
    #[serde(skip)]
    edge_cells: EdgeCells,
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Tests turn this on to run the frame passes over every cell and every
    /// footprint, the reference the tracked passes must match. It is
    /// per-thread: fog work moved to worker threads would not see it.
    pub(crate) static VISIT_EVERY_CELL: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

/// Whether the frame passes skip [`TrackedCells`] (tests only).
pub(crate) fn visit_every_cell() -> bool {
    #[cfg(any(test, feature = "test-support"))]
    return VISIT_EVERY_CELL.get();
    #[cfg(not(any(test, feature = "test-support")))]
    false
}

/// The cells [`OwnerVisibility::clear_frame_sight`] and the friendly gap pass
/// change, so a frame's work follows what changed rather than the map size.
/// Derived from the grids: when `complete`, every cell's published bits match
/// its knowledge (`publish_knowledge`) and each list holds every cell of its
/// kind, perhaps more than once. A load or a writer that bypasses publishing
/// clears `complete`, and the next clear rebuilds by visiting every cell.
#[derive(Debug, Clone, Default)]
struct TrackedCells {
    complete: bool,
    /// Cells whose `transient_visible` was set.
    transient: Vec<u32>,
    /// Cells holding [`FLAG_GAP_FOG`].
    gap_fog: Vec<u32>,
    /// Cells published visible while their ground bits lacked
    /// `GROUND_VISIBLE | GROUND_OPEN`.
    unground: Vec<u32>,
    /// Cells whose ground bits a cell writer took off since the last frame
    /// pass (a conceal, a shroud-edge sweep or a reveal that leaves the cell
    /// closed). A pass that marks every retained footprint gives them back
    /// under the footprints that cover them; a tracked pass does that for
    /// these cells alone ([`FogState::mark_cleared_cells`]).
    cleared: Vec<u32>,
}

impl OwnerVisibility {
    /// Create a new zeroed visibility grid of the given dimensions.
    pub fn new(width: u16, height: u16) -> Self {
        let len: usize = (width as usize) * (height as usize);
        Self {
            cells: vec![0u8; len],
            width,
            height,
            ground_flags: vec![0; len],
            shroud_knowledge: vec![ShroudKnowledge::default(); len],
            map_cell_edge_cache: vec![MapCellEdgeCache::default(); len],
            dummy_visibility: DummyVisibility::default(),
            // Every zeroed cell already matches its default knowledge.
            tracked: TrackedCells {
                complete: true,
                ..TrackedCells::default()
            },
            edge_cells: EdgeCells::complete(),
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
            .is_some_and(|i| self.cells[i] & FLAG_VISIBLE != 0)
    }

    /// Returns true if the cell has been revealed at least once.
    pub fn is_revealed(&self, rx: u16, ry: u16) -> bool {
        self.index(rx, ry)
            .is_some_and(|i| self.cells[i] & FLAG_REVEALED != 0)
    }

    /// Returns true if the cell is covered by an enemy gap generator this tick.
    pub fn is_gap_covered(&self, rx: u16, ry: u16) -> bool {
        self.index(rx, ry)
            .is_some_and(|i| self.cells[i] & FLAG_GAP_COVERED != 0)
    }

    /// Returns true if the cell is covered by a friendly gap generator this tick.
    pub fn is_gap_fog(&self, rx: u16, ry: u16) -> bool {
        self.index(rx, ry)
            .is_some_and(|i| self.cells[i] & FLAG_GAP_FOG != 0)
    }

    /// Mark a cell as both visible and revealed.
    #[cfg(any(test, feature = "test-support"))]
    pub fn mark_visible(&mut self, rx: u16, ry: u16) {
        self.mark_sight(rx, ry);
        if let Some(index) = self.index(rx, ry) {
            self.shroud_knowledge[index].counter = 0;
            self.shroud_knowledge[index].open = true;
            self.set_transient_visible(index);
            self.publish_knowledge(index);
        }
    }

    /// Give `index` this frame's transient sight, which the next clear ends.
    fn set_transient_visible(&mut self, index: usize) {
        self.shroud_knowledge[index].transient_visible = true;
        self.tracked.transient.push(index as u32);
    }

    /// Clear the friendly gap fog: a tracked grid's listed cells, or else
    /// every cell.
    fn clear_gap_fog(&mut self) {
        if self.tracked.complete && !visit_every_cell() {
            for &index in &self.tracked.gap_fog {
                self.cells[index as usize] &= !FLAG_GAP_FOG;
            }
        } else {
            for cell in &mut self.cells {
                *cell &= !FLAG_GAP_FOG;
            }
        }
        self.tracked.gap_fog.clear();
    }

    /// Put `index` under a friendly Gap Generator's fog.
    fn set_gap_fog(&mut self, index: usize) {
        self.cells[index] |= FLAG_GAP_FOG;
        self.tracked.gap_fog.push(index as u32);
    }

    /// Mark a cell visible and revealed in this frame's sight, and set its
    /// ground bits.
    ///
    /// RESIDUAL: a cell's first mapping runs `CellClass::CleanFog` when
    /// `FogOfWar=` is on, freeing the cell's fogged objects. VERA keeps no
    /// fogged-object records in production (the footprint store is test-only),
    /// so there is nothing to clean; a fogged-object port needs that call here.
    fn mark_sight(&mut self, rx: u16, ry: u16) {
        if let Some(i) = self.index(rx, ry) {
            self.set_ground_bits(i, self.ground_flags[i] | GROUND_VISIBLE | GROUND_OPEN);
            self.cells[i] |= FLAG_VISIBLE | FLAG_REVEALED;
        }
    }

    /// Clear this frame's sight as the sight pass does, visiting every cell.
    /// The grid stays untracked, so the next pass marks every footprint again.
    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_all_visible(&mut self) {
        self.clear_every_cell();
        self.tracked.complete = false;
    }

    /// The sight pass's clear, preserving revealed flags: a visible cell
    /// first gets its ground bits; the friendly gap fog and transient sight
    /// end; each cell's bits are published again from its knowledge.
    ///
    /// Returns whether the grid was tracked ([`TrackedCells`]): then only
    /// the listed cells can change, and only they are visited.
    fn clear_frame_sight(&mut self) -> bool {
        if !self.tracked.complete || visit_every_cell() {
            self.clear_every_cell();
            return false;
        }
        for unground in 0..self.tracked.unground.len() {
            let index = self.tracked.unground[unground] as usize;
            if self.cells[index] & FLAG_VISIBLE != 0 {
                self.set_ground_bits(
                    index,
                    self.ground_flags[index] | GROUND_VISIBLE | GROUND_OPEN,
                );
            }
        }
        self.tracked.unground.clear();
        self.clear_gap_fog();
        let mut transient = std::mem::take(&mut self.tracked.transient);
        for &index in &transient {
            self.shroud_knowledge[index as usize].transient_visible = false;
        }
        // Every other cell's knowledge is unchanged, so publishing it would
        // put back the bits a full clear takes off.
        for &index in &transient {
            self.publish_knowledge(index as usize);
        }
        transient.clear();
        self.tracked.transient = transient;
        true
    }

    /// [`Self::clear_frame_sight`] over every cell, which also makes the
    /// grid tracked: afterwards no cell is transient or holds gap fog, every
    /// visible cell has its ground bits, and publishing lists the rest.
    fn clear_every_cell(&mut self) {
        for knowledge in &mut self.shroud_knowledge {
            knowledge.transient_visible = false;
        }
        for index in 0..self.cells.len() {
            if self.cells[index] & FLAG_VISIBLE != 0 {
                self.set_ground_bits(
                    index,
                    self.ground_flags[index] | GROUND_VISIBLE | GROUND_OPEN,
                );
            }
            self.cells[index] &= !(FLAG_VISIBLE
                | FLAG_GAP_COVERED
                | FLAG_GAP_FOG
                | FLAG_SUSTAINED_SIGHT
                | FLAG_EFFECTIVE_GAP_SIGHT);
        }
        self.tracked.transient.clear();
        self.tracked.gap_fog.clear();
        self.tracked.unground.clear();
        // This pass marks every retained footprint.
        self.tracked.cleared.clear();
        self.tracked.complete = true;
        for index in 0..self.cells.len() {
            self.publish_knowledge(index);
        }
    }

    /// Set `index`'s published bits from its knowledge, and list it when it
    /// is visible without its ground bits ([`TrackedCells::unground`]).
    fn publish_knowledge(&mut self, index: usize) {
        let cell = &mut self.cells[index];
        let was_pending = *cell & FLAG_PENDING_GAP_CONCEAL != 0;
        *cell = *cell & !PUBLISHED_FLAGS | published_flags(self.shroud_knowledge[index]);
        // Every pending writer publishes, so the bit's rise lists the cell.
        if *cell & FLAG_PENDING_GAP_CONCEAL != 0 && !was_pending {
            self.edge_cells.pending.push(index as u32);
        }
        if *cell & FLAG_VISIBLE != 0
            && self.ground_flags[index] & (GROUND_VISIBLE | GROUND_OPEN)
                != GROUND_VISIBLE | GROUND_OPEN
        {
            self.tracked.unground.push(index as u32);
        }
    }

    /// Panics unless a tracked grid holds what [`TrackedCells`] promises.
    #[cfg(any(test, feature = "test-support"))]
    fn assert_tracked(&self) {
        if !self.tracked.complete {
            return;
        }
        let listed = |list: &[u32]| {
            let mut listed = vec![false; self.cells.len()];
            for &index in list {
                listed[index as usize] = true;
            }
            listed
        };
        let transient = listed(&self.tracked.transient);
        let gap_fog = listed(&self.tracked.gap_fog);
        let unground = listed(&self.tracked.unground);
        for (index, (&cell, knowledge)) in self.cells.iter().zip(&self.shroud_knowledge).enumerate()
        {
            assert_eq!(
                cell & PUBLISHED_FLAGS,
                published_flags(*knowledge),
                "cell {index}: published bits"
            );
            assert!(
                !knowledge.transient_visible || transient[index],
                "cell {index}: unlisted transient sight"
            );
            assert!(
                cell & FLAG_GAP_FOG == 0 || gap_fog[index],
                "cell {index}: unlisted gap fog"
            );
            assert!(
                cell & FLAG_VISIBLE == 0
                    || self.ground_flags[index] & (GROUND_VISIBLE | GROUND_OPEN)
                        == GROUND_VISIBLE | GROUND_OPEN
                    || unground[index],
                "cell {index}: unlisted visible cell without ground bits"
            );
        }
    }

    /// The raw cell flags in row-major order.
    pub fn cells_raw(&self) -> &[u8] {
        &self.cells
    }

    /// The ground bits in the same row-major order as `cells`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn ground_flags_raw(&self) -> &[u8] {
        &self.ground_flags
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn shroud_knowledge_raw(&self) -> &[ShroudKnowledge] {
        &self.shroud_knowledge
    }

    /// Fold every per-cell grid and the Dummy's state into `hasher`, for the
    /// simulation state hash.
    pub(crate) fn hash_cells(&self, hasher: &mut impl std::hash::Hasher) {
        use std::hash::Hash;
        self.cells.hash(hasher);
        crate::sim::state_hasher::hash_records(
            &self.shroud_knowledge,
            hasher,
            ShroudKnowledge::hash_record,
        );
        // The CellClass ground bits the aircraft queries read.
        self.ground_flags.hash(hasher);
        self.hash_map_cell_edge_cache(hasher);
    }

    #[cfg(any(test, feature = "test-support"))]
    fn resized_preserving_state(&self, width: u16, height: u16) -> Self {
        let mut expanded = Self::new(width, height);
        expanded.dummy_visibility = self.dummy_visibility;
        for ry in 0..self.height.min(height) {
            for rx in 0..self.width.min(width) {
                let old = ry as usize * self.width as usize + rx as usize;
                let new = ry as usize * width as usize + rx as usize;
                expanded.cells[new] = self.cells[old];
                expanded.shroud_knowledge[new] =
                    self.shroud_knowledge.get(old).copied().unwrap_or_default();
                expanded.ground_flags[new] = self.ground_flags[old];
                expanded.map_cell_edge_cache[new] = self.map_cell_edge_cache[old];
            }
        }
        // The copied transient and gap-fog cells are not listed.
        expanded.tracked.complete = false;
        expanded.edge_cells = EdgeCells::default();
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn merge_revealed_from(&mut self, other: &OwnerVisibility) {
        // A published bit written without its knowledge.
        self.tracked.complete = false;
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
                    if other.is_revealed(rx, ry)
                        && let Some(i) = self.index(rx, ry)
                    {
                        self.cells[i] |= FLAG_REVEALED;
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

/// What a fire check (GetFireError and its range and line-of-fire questions)
/// reads of the houses and their fog: the ally graph
/// (`Simulation::house_alliances`) and the sensor counts, borrowed live
/// ([`FogState::fire_view`]).
#[derive(Clone, Copy)]
pub(crate) struct FireFog<'a> {
    pub(crate) alliances: &'a HouseAllianceMap,
    sensors_by_house: &'a BTreeMap<InternedId, Vec<i16>>,
    width: u16,
    height: u16,
}

impl FireFog<'_> {
    /// Whether `house` holds a sensor count on cell (`rx`, `ry`)
    /// ([`FogState::has_sensor_for_house`]).
    pub(crate) fn has_sensor_for_house(&self, house: InternedId, rx: u16, ry: u16) -> bool {
        sensor_counted(
            self.sensors_by_house,
            self.width,
            self.height,
            house,
            rx,
            ry,
        )
    }
}

/// Whether `house`'s sensor count on cell (`rx`, `ry`) is positive.
fn sensor_counted(
    sensors_by_house: &BTreeMap<InternedId, Vec<i16>>,
    width: u16,
    height: u16,
    house: InternedId,
    rx: u16,
    ry: u16,
) -> bool {
    if rx >= width || ry >= height {
        return false;
    }
    let index = usize::from(ry) * usize::from(width) + usize::from(rx);
    sensors_by_house
        .get(&house)
        .and_then(|counters| counters.get(index))
        .is_some_and(|count| *count > 0)
}

/// Global fog/shroud state keyed by owner name.
///
/// Stores per-viewer knowledge grids. Direct alliance admission happens at
/// fresh source writers, never by merging derived viewer knowledge, so every
/// query reads the viewer's own plane.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FogState {
    pub width: u16,
    pub height: u16,
    pub by_owner: BTreeMap<InternedId, OwnerVisibility>,
    /// One native source cache per logical viewer. Inactive entries retain
    /// cached254/260 after250 is cleared, until actual object teardown.
    pub(crate) sight_admissions: BTreeMap<(u64, InternedId), SightAdmission>,
    /// Admissions `TechnoClass::UpdateReveal @ 0x0070AF50` makes for an
    /// alternate house (`RevealToAll=`;
    /// `Simulation::reveal_sight_to_current_house`), keyed like
    /// `sight_admissions`. Kept apart so the source's own reconciliation
    /// never releases them, and a viewer that also shares the source's
    /// ordinary sight holds both contributions, as native does.
    /// Native keeps no record: each call reveals and each release
    /// (the Simulation source release) leaves the counters. VERA
    /// retains the first per viewer, which holds its source count until a
    /// release takes it.
    pub(crate) alternate_sight_admissions: BTreeMap<(u64, InternedId), SightAdmission>,
    /// Native local House+240: whole-map reveal guard, distinct from the
    /// checksum's map-clear byte+241 and SpySatActive+577A. TAction17 at
    ///6E0FE0 reads this guard without writing it; Map577D90 and gap writers
    ///own its transitions (tools/spatial_oracle/trigger_action_values).
    whole_map_revealed_owners: std::collections::BTreeSet<InternedId>,
    /// Per-viewer hostile admission receipts; preserved across save/load so a
    /// continuing generator does not become a fresh write after restoration.
    pub(crate) gap_sources: BTreeMap<InternedId, std::collections::BTreeSet<GapGeneratorSource>>,
    /// Bumps each time presentation prepares a view (F10); the fog mask
    /// renderer and minimap dirty-gate on it. Runtime only: never saved or
    /// hashed, so it restarts at 0 after a load.
    #[serde(skip)]
    pub(crate) view_generation: u64,
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
    /// An empty viewer domain with the allocated map dimensions. Reveal and
    /// gap admission always initialize their own private House+240 state.
    #[cfg(any(test, feature = "test-support"))]
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            ..Self::default()
        }
    }

    /// Source metadata is published before Map5678E0 can call a receiver.
    /// These counts are a derived sight index, not native Cell counters or
    /// ground knowledge. Only the subsequent per-cell writer publishes them.
    pub(crate) fn retain_sight_admission(
        &mut self,
        id: u64,
        viewer: InternedId,
        mut admission: SightAdmission,
        alternate: bool,
    ) {
        admission.latched = true;
        let map = if alternate {
            &mut self.alternate_sight_admissions
        } else {
            &mut self.sight_admissions
        };
        let old = map.insert((id, viewer), admission.clone());
        assert!(
            old.is_none_or(|old| !old.latched),
            "source already retained"
        );
        self.adjust_sight_sources(viewer, &admission, true);
    }

    /// Clear the latch before any release receiver. Clearing every retained
    /// contribution first makes a nested Limbo idempotent;254/260 persist.
    pub(crate) fn take_sight_admission(
        &mut self,
        id: u64,
        viewer: InternedId,
        alternate: bool,
    ) -> Option<SightAdmission> {
        let map = if alternate {
            &mut self.alternate_sight_admissions
        } else {
            &mut self.sight_admissions
        };
        let admission = if alternate {
            map.remove(&(id, viewer))?
        } else {
            let retained = map.get_mut(&(id, viewer)).filter(|source| source.latched)?;
            retained.latched = false;
            let admission = retained.clone();
            //Only254/260 survive; the derived active-footprint index does not.
            retained.cells.clear();
            admission
        };
        self.adjust_sight_sources(viewer, &admission, false);
        Some(admission)
    }

    fn adjust_sight_sources(&mut self, viewer: InternedId, admission: &SightAdmission, add: bool) {
        let vis = self
            .by_owner
            .entry(viewer)
            .or_insert_with(|| OwnerVisibility::new(self.width, self.height));
        for &(x, y) in &admission.cells {
            let Some(index) = vis.index(x, y) else {
                continue;
            };
            let state = &mut vis.shroud_knowledge[index];
            let count = if viewer == admission.owner {
                &mut state.local_sources
            } else {
                &mut state.allied_sources
            };
            *count = if add {
                count.wrapping_add(1)
            } else {
                count
                    .checked_sub(1)
                    .expect("retained sight admission is balanced")
            };
        }
    }

    pub(crate) fn source_sight_viewers(&self, id: u64) -> Vec<InternedId> {
        self.sight_admissions
            .range(source_keys(id))
            .filter(|(_, source)| source.latched)
            .map(|(&(_, viewer), _)| viewer)
            .collect()
    }

    pub(crate) fn sight_is_latched(&self, id: u64, viewer: InternedId) -> bool {
        self.sight_admissions
            .get(&(id, viewer))
            .is_some_and(|source| source.latched)
    }

    /// Source `id`'s admissions into `into`, which the fog pass reuses.
    pub(crate) fn read_source_admissions(&self, id: u64, into: &mut SourceAdmissions) {
        into.0.clear();
        into.0.extend(
            self.sight_admissions
                .range(source_keys(id))
                .map(|(&(_, viewer), source)| (viewer, source.owner, source.latched)),
        );
    }

    /// Destructor cleanup of cached254/260 and alternate derived indexes.
    /// Native UnInit/Limbo must have released their live contributions first.
    pub(crate) fn forget_sight_source(&mut self, id: u64) {
        for admissions in [
            &mut self.sight_admissions,
            &mut self.alternate_sight_admissions,
        ] {
            while let Some((&key, _)) = admissions.range(source_keys(id)).next() {
                admissions.remove(&key);
            }
        }
    }

    /// Rebuild the derived current-sight projection without emitting another
    /// native reveal or changing Cell cache bytes.
    pub(crate) fn mark_sight_admission(&mut self, viewer: InternedId, admission: &SightAdmission) {
        let vis = self
            .by_owner
            .entry(viewer)
            .or_insert_with(|| OwnerVisibility::new(self.width, self.height));
        for &(x, y) in &admission.cells {
            let Some(index) = vis.index(x, y) else {
                continue;
            };
            vis.mark_sight(x, y);
            vis.publish_knowledge(index);
        }
    }

    /// Mark each footprint source `stable_id` retains as current sight on
    /// its viewer's map, where it stands.
    pub(crate) fn mark_retained_sight(&mut self, stable_id: u64) {
        let retained: Vec<_> = self
            .sight_admissions
            .range(source_keys(stable_id))
            .filter(|(_, source)| source.latched)
            .map(|(&(_, viewer), admission)| (viewer, admission.clone()))
            .collect();
        for (viewer, admission) in retained {
            self.mark_sight_admission(viewer, &admission);
        }
    }

    /// Mark each tracked map's cleared cells ([`TrackedCells::cleared`]) that
    /// a retained footprint covers, after the frame pass's sources, as the
    /// pass's marks of every retained footprint would on an untracked map.
    /// A cell's source counts count its retained footprints, alternate ones
    /// too (the alternate source registry), which the pass does not mark.
    pub(crate) fn mark_cleared_cells(&mut self) {
        for (&viewer, vis) in &mut self.by_owner {
            if vis.tracked.cleared.is_empty() {
                continue;
            }
            let mut alternate: Vec<usize> = self
                .alternate_sight_admissions
                .iter()
                .filter(|&(&(_, alternate_viewer), _)| alternate_viewer == viewer)
                .flat_map(|(_, admission)| &admission.cells)
                .filter_map(|&(x, y)| vis.index(x, y))
                .collect();
            alternate.sort_unstable();
            let mut cleared = std::mem::take(&mut vis.tracked.cleared);
            for &index in &cleared {
                let index = index as usize;
                let knowledge = vis.shroud_knowledge[index];
                let sources =
                    u64::from(knowledge.local_sources) + u64::from(knowledge.allied_sources);
                let alternates = alternate.partition_point(|&cell| cell <= index)
                    - alternate.partition_point(|&cell| cell < index);
                if sources > alternates as u64 {
                    let width = usize::from(vis.width);
                    vis.mark_sight((index % width) as u16, (index / width) as u16);
                    vis.publish_knowledge(index);
                }
            }
            cleared.clear();
            vis.tracked.cleared = cleared;
        }
    }

    /// Alternate ReReveal drops the ordinary source index without a native
    /// leave on those planes. Its native counters deliberately remain held.
    pub(crate) fn clear_sight_latch(
        &mut self,
        stable_id: u64,
        preferred_viewer: InternedId,
    ) -> Option<SightAdmission> {
        let footprint = self
            .sight_admissions
            .get(&(stable_id, preferred_viewer))
            .cloned()
            .or_else(|| {
                self.sight_admissions
                    .range(source_keys(stable_id))
                    .next()
                    .map(|(_, source)| source.clone())
            });
        for viewer in self.source_sight_viewers(stable_id) {
            let admission = self.take_sight_admission(stable_id, viewer, false).unwrap();
            if let Some(vis) = self.by_owner.get_mut(&viewer) {
                for &(x, y) in &admission.cells {
                    if let Some(index) = vis.index(x, y) {
                        vis.publish_knowledge(index);
                    }
                }
            }
        }
        footprint
    }

    /// Insert one shared frozen-building footprint record. Named location:
    /// `BuildingClass::FreezeInFog` installs the same FoggedObjectClass pointer
    /// into every occupy-list cell, not one allocation per cell.
    #[cfg(any(test, feature = "test-support"))]
    pub fn insert_fogged_object_footprint(
        &mut self,
        viewer: InternedId,
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn clear_fogged_objects_at(
        &mut self,
        viewer: InternedId,
        rx: u16,
        ry: u16,
    ) -> Vec<FoggedObjectFootprintRecord> {
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
    #[cfg(any(test, feature = "test-support"))]
    pub fn reset_cloaked_by_houses(&mut self) {
        self.cloaked_by_houses.clear();
        self.cloaked_by_houses
            .resize(usize::from(self.width) * usize::from(self.height), 0);
    }

    /// Native `FUN_00487110 (unlabelled; name not a symbol)`.
    #[cfg(any(test, feature = "test-support"))]
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
    #[cfg(any(test, feature = "test-support"))]
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

    #[cfg(any(test, feature = "test-support"))]
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

    /// Native `FootClass::AddSensorsAt @ 0x004DE7B0`: outer-Y/inner-X strict
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
    #[cfg(any(test, feature = "test-support"))]
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

    /// Paired `FootClass::RemoveSensorsAt` @ `0x004DE940` decrement walk.
    #[cfg(any(test, feature = "test-support"))]
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

    pub fn increment_sensor_at(&mut self, house: InternedId, rx: u16, ry: u16) {
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
        sensor_counted(
            &self.sensors_by_house,
            self.width,
            self.height,
            house,
            rx,
            ry,
        )
    }

    /// What a fire check reads of this fog and the houses' ally graph.
    pub(crate) fn fire_view<'a>(&'a self, alliances: &'a HouseAllianceMap) -> FireFog<'a> {
        FireFog {
            alliances,
            sensors_by_house: &self.sensors_by_house,
            width: self.width,
            height: self.height,
        }
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
    #[cfg(any(test, feature = "test-support"))]
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

    /// Presentation prepared a view (F10): bump the runtime generation the
    /// fog mask renderer and minimap dirty-gate on. Queries read the viewer
    /// plane directly, so there is nothing to copy.
    pub fn bump_view_generation(&mut self) {
        self.view_generation = self.view_generation.wrapping_add(1);
    }

    /// The runtime view generation render dirty-gates on (F10). Restarts at 0
    /// on every load.
    pub fn view_generation(&self) -> u64 {
        self.view_generation
    }

    /// Store a recorded native `Cell+0x12C` ground byte for `owner`: its
    /// `0x08`/`0x10` in [`GROUND_VISIBLE`]/[`GROUND_OPEN`], and its `0x08` as
    /// the mapped knowledge that [`cell_ground_mapped`] reads.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_ground_bits_for_test(&mut self, owner: InternedId, rx: u16, ry: u16, bits: u8) {
        let (width, height) = (self.width, self.height);
        let vis = self
            .by_owner
            .entry(owner)
            .or_insert_with(|| OwnerVisibility::new(width, height));
        let index = vis.index(rx, ry).expect("ground byte cell in bounds");
        vis.set_ground_bits(index, bits & (GROUND_VISIBLE | GROUND_OPEN));
        vis.shroud_knowledge[index].open = bits & GROUND_VISIBLE != 0;
        // Cleared ground bits are restored by the sight that covers them.
        vis.tracked.complete = false;
        vis.publish_knowledge(index);
    }

    /// The ground-visible bit (`Cell+0x12C & 0x08`) as [`GROUND_VISIBLE`]
    /// holds it, for checking what the cell writers stored.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn is_ground_unshrouded(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        self.by_owner.get(&owner).is_some_and(|view| {
            view.index(rx, ry)
                .and_then(|i| view.ground_flags.get(i))
                .is_some_and(|&flags| flags & GROUND_VISIBLE != 0)
        })
    }

    /// Aircraft FindFireLocation419986 reads Cell+12C bit16 directly. This is
    /// independent of bit8.
    pub(crate) fn is_ground_open(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        self.by_owner.get(&owner).is_some_and(|view| {
            view.index(rx, ry)
                .and_then(|i| view.ground_flags.get(i))
                .is_some_and(|&flags| flags & GROUND_OPEN != 0)
        })
    }

    /// Returns true if the owner (or a friendly ally) currently sees the cell.
    pub fn is_cell_visible(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        self.by_owner
            .get(&owner)
            .is_some_and(|s| s.is_visible(rx, ry))
    }

    /// Returns true if the owner (or a friendly ally) has revealed the cell.
    pub fn is_cell_revealed(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        self.by_owner
            .get(&owner)
            .is_some_and(|s| s.is_revealed(rx, ry))
    }

    /// Returns true if the cell is covered by an enemy gap generator for this owner.
    pub fn is_cell_gap_covered(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        self.by_owner
            .get(&owner)
            .is_some_and(|s| s.is_gap_covered(rx, ry))
    }

    /// Returns true if the cell is covered by a friendly gap generator for this owner.
    pub fn is_cell_gap_fog(&self, owner: InternedId, rx: u16, ry: u16) -> bool {
        self.by_owner
            .get(&owner)
            .is_some_and(|s| s.is_gap_fog(rx, ry))
    }

    /// Shroud `owner`'s whole buffer again for fog-only fixtures, re-admitting
    /// only its own admitted sight: no live callbacks, gap pass or map-clear
    /// byte. The game resets a map through `Simulation::reset_shroud_for_owner`
    /// (`MapClass::Reset_Shroud @ 0x00577AB0`).
    #[cfg(any(test, feature = "test-support"))]
    pub fn reset_explored_for_owner(&mut self, owner: InternedId) {
        let cells = self.rectangular_cells();
        self.transition_whole_map_for_owner(owner, cells, true, false);
    }

    /// Fog-only test adapter for Logic55B29A's signed modulo120. Production
    /// uses Simulation's shared578100 traversal so occupied callbacks and
    /// the actual allocated cursor execute between individual cell writes.
    #[cfg(any(test, feature = "test-support"))]
    pub fn flush_pending_gap_conceal(&mut self, native_frame: i32) {
        if native_frame % 120 != 0 {
            return;
        }
        let viewers: Vec<_> = self.by_owner.keys().copied().collect();
        let cells = self.rectangular_cells();
        for viewer in viewers {
            for &cell in &cells {
                self.sweep_pending_map_cell(viewer, cell, None);
            }
            for &cell in &cells {
                self.refresh_map_cell_ground_frame(viewer, cell, None);
            }
        }
    }

    /// Explore `owner`'s whole buffer for fog-only fixtures, without granting
    /// current sight. The game reveals a map through
    /// `Simulation::reveal_map_for_owner` (`MapClass::Reveal @ 0x00577D90`) or
    /// `Simulation::reveal_entire_map` (`MapClass::RevealEntireMap @
    /// 0x00577F30`).
    #[cfg(any(test, feature = "test-support"))]
    pub fn reveal_all_for_owner(&mut self, owner: InternedId) {
        let cells = self.rectangular_cells();
        self.transition_whole_map_for_owner(owner, cells, false, false);
    }

    /// [`Self::reveal_all_for_owner`] over the given cells only. Repeated
    /// materialization is suppressed by the native House240 counterpart.
    #[cfg(any(test, feature = "test-support"))]
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
    #[cfg(any(test, feature = "test-support"))]
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
    #[cfg(any(test, feature = "test-support"))]
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

/// The settings a reveal reads, from the rules, the map and the game
/// ([`VisionConfig::new`]; production builds it in
/// `Simulation::sight_reveal_config`).
pub struct VisionConfig {
    /// When true, Techno reveal/update readers require the canonical stored
    /// TechnoClass+0x3D5 membership byte. Headless fixtures without live
    /// MapClass authority leave this false.
    require_playfield_membership: bool,
    /// `[General] VeteranSight=` (`RulesClass+0x680`), the multiplier a
    /// `SIGHT`-ability holder applies to its elevation-scaled sight. Stock
    /// `0.0` disables it (see `veterancy::veteran_sight_cells`).
    veteran_sight: f64,
    /// Leptons of elevation per +1 sight cell (from [General] LeptonsPerSightIncrease=).
    /// 256 leptons = 1 z-level. 0 disables the elevation bonus.
    leptons_per_sight_increase: i32,
    /// Height-based LOS obstruction (from [General] RevealByHeight=).
    /// When true, terrain 4+ levels above the viewer at the midpoint blocks sight.
    /// Default true (the standard RA2/YR setting).
    reveal_by_height: bool,
    /// The map `Size=` diamond (`MapClass+0xF4/+0xF8`) that bounds a reveal's
    /// lifted centre and each of its cells ([`collect_reveal_cells`]). A
    /// headless fixture without a map has none; the fog grid bounds it alone.
    map_size: Option<(i32, i32)>,
    /// `[AudioVisual] AllyReveal=`: an allied house's reveals map this
    /// house's cells too ([`direct_reveal_viewers`]).
    ally_reveal: bool,
    /// The houses whose type is `MultiplayPassive=` (HouseType `+0x1A6`):
    /// no reveal of theirs passes its gates ([`holds_sight`]).
    passive_owners: Vec<InternedId>,
}

#[cfg(any(test, feature = "test-support"))]
impl Default for VisionConfig {
    fn default() -> Self {
        Self {
            require_playfield_membership: false,
            veteran_sight: 0.0,
            leptons_per_sight_increase: 0,
            reveal_by_height: true,
            map_size: None,
            ally_reveal: true,
            passive_owners: Vec::new(),
        }
    }
}

impl VisionConfig {
    /// The rules' sight keys (none in a fixture without rules: no veteran
    /// or elevation sight, `RevealByHeight=` and `AllyReveal=` at their
    /// constructor defaults), the map's `Size=` diamond, whether a Techno
    /// needs the stored playfield byte, and the `MultiplayPassive=` houses.
    pub(crate) fn new(
        rules: Option<&crate::rules::ruleset::RuleSet>,
        map_size: Option<(i32, i32)>,
        require_playfield_membership: bool,
        passive_owners: Vec<InternedId>,
    ) -> Self {
        Self {
            require_playfield_membership,
            veteran_sight: rules.map_or(0.0, |r| r.general.veteran_sight),
            leptons_per_sight_increase: rules.map_or(0, |r| r.general.leptons_per_sight_increase),
            reveal_by_height: rules.is_none_or(|r| r.general.reveal_by_height),
            map_size,
            ally_reveal: rules.is_none_or(|r| r.general.ally_reveal),
            passive_owners,
        }
    }

    /// Whether `owner`'s house type is `MultiplayPassive=`, which stops
    /// both reveal functions (`0x0070AF66..0x0070AF77`,
    /// `0x0070B1DF..0x0070B1F0`).
    pub(crate) fn passive_owner(&self, owner: InternedId) -> bool {
        self.passive_owners.contains(&owner)
    }

    /// Whether `entity`'s reveal under this configuration applies the
    /// `SIGHT` ability ([`entity_has_sight_ability`]). The ability changes
    /// the radius only through a nonzero `VeteranSight=`
    /// (`veteran_sight_cells`, `0x0070B082..0x0070B0A4`), so under retail's
    /// `0.0` its rules lookup is skipped.
    pub(crate) fn sight_ability(
        &self,
        entity: &crate::sim::game_entity::GameEntity,
        interner: &StringInterner,
        rules: Option<&crate::rules::ruleset::RuleSet>,
    ) -> bool {
        self.veteran_sight != 0.0 && entity_has_sight_ability(entity, interner, rules)
    }

    /// `[General] RevealByHeight=`: whether a sight reveal tests its cells'
    /// line of sight, so whether the caller needs the height grid.
    pub(crate) fn reveal_by_height(&self) -> bool {
        self.reveal_by_height
    }

    pub(crate) fn require_playfield_membership(&self) -> bool {
        self.require_playfield_membership
    }

    /// Ordered geometry and the admitted local XYZ for the MapClass area
    /// route. Rejected zero/Size calls differ from an admitted empty LOS walk.
    /// The world owns synchronous receivers and current-player admission.
    pub(crate) fn collect_area_cells(
        &self,
        cell: (u16, u16),
        radius: u16,
        z: i32,
        line_of_sight: Option<&[u8]>,
        dimensions: (u16, u16),
    ) -> (Vec<(u16, u16)>, Option<(u16, u16, i32)>) {
        let origin = reveal_area_origin((cell.0, cell.1, z), radius, self.map_size)
            .map(|(x, y)| (x as u16, y as u16, z));
        let cells = collect_reveal_cells(
            cell.0,
            cell.1,
            radius,
            z,
            line_of_sight.is_some(),
            line_of_sight,
            dimensions,
            self.map_size,
        );
        (cells, origin)
    }
}

/// The `SIGHT`-ability half of `TechnoClass::UpdateReveal @ 0x0070AF50`'s
/// veteran gate (`0x0070B01E..0x0070B07A`), resolved off the type's ability
/// arrays through the shared `HasWeaponAbility` predicate. No rules — as in
/// headless fixtures — reads as no ability.
fn entity_has_sight_ability(
    entity: &crate::sim::game_entity::GameEntity,
    interner: &crate::sim::intern::StringInterner,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) -> bool {
    rules
        .and_then(|rules| interner.object_type(entity.type_ref(), rules))
        .is_some_and(|object| {
            crate::sim::combat::veterancy::has_weapon_ability(
                entity.veterancy.rank_kind(),
                object,
                crate::rules::object_type::Ability::Sight,
            )
        })
}

impl FogState {
    /// Begin the projection frame without manufacturing a source event.
    /// Source reconciliation and synchronous receivers belong to Simulation.
    pub(crate) fn begin_sight_frame(
        &mut self,
        entities: &EntityStore,
        path_grid: Option<&PathGrid>,
    ) -> std::collections::BTreeSet<InternedId> {
        // Construction-seeded session bounds are authoritative; the lazy
        // derivation stays only as the fallback for fixture sims built without a
        // descriptor (zero-dim fog).
        let (width, height) = if self.width > 0 && self.height > 0 {
            (self.width, self.height)
        } else {
            resolve_bounds(entities, path_grid)
        };
        if width == 0 || height == 0 {
            *self = FogState::default();
            return std::collections::BTreeSet::new();
        }

        // First tick or dimension change: recreate all grids (cold path). A grid
        // whose clear visited every cell re-marks every footprint this frame.
        let mut untracked = std::collections::BTreeSet::new();
        if self.width != width || self.height != height {
            self.by_owner.clear();
            self.gap_sources.clear();
            self.sight_admissions.clear();
            self.alternate_sight_admissions.clear();
            self.whole_map_revealed_owners.clear();
            self.fogged_object_cells.clear();
            self.fogged_objects.clear();
            self.next_fogged_object_id = 0;
            self.reset_sensor_counts();
            self.width = width;
            self.height = height;
        } else {
            // Hot path: clear visible flags, preserve revealed.
            for (&viewer, vis) in &mut self.by_owner {
                if !vis.clear_frame_sight() {
                    untracked.insert(viewer);
                }
            }
        }

        untracked
    }
}

/// Caller/projection eligibility: alive, out of limbo and not carried,
/// plus the native entry gates shared by UpdateReveal70AF50/ReReveal70B1D0: under live map authority the stored playfield
/// byte `+0x3D5`, and an owner that is not `MultiplayPassive=`
/// (`0x0070AF66..0x0070AF77`, `0x0070B1DF..0x0070B1F0`). Lifetime filters
/// belong to callers; they are not extra gates inside either native function.
pub(crate) fn holds_sight(
    entity: &crate::sim::game_entity::GameEntity,
    config: &VisionConfig,
) -> bool {
    !entity.death.is_dying()
        && !entity.lifecycle.in_limbo()
        && (!config.require_playfield_membership || entity.in_playfield())
        && !entity.passenger_role.is_inside_transport()
        && !config.passive_owner(entity.owner())
}

/// The ordinary `TechnoClass::UpdateReveal @ 0x0070AF50` footprint for
/// the entity's own House. Exact zero type Sight is rejected at `0x0070AF87`;
/// an admitted zero effective radius can still retain an empty footprint.
/// Returns `None` without a fog plane. Callers own [`holds_sight`] eligibility.
pub(crate) fn entity_own_sight(
    fog: &FogState,
    entity: &crate::sim::game_entity::GameEntity,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    sight_ability: bool,
) -> Option<SightAdmission> {
    entity_sight_admission(
        fog,
        entity,
        entity.owner(),
        config,
        height_grid,
        terrain,
        sight_ability,
        None,
    )
}

/// The footprint `TechnoClass::UpdateReveal @ 0x0070AF50` reveals for
/// `entity`'s sight, computed as the ordinary writer computes it and recorded
/// as a reveal by `revealer`; none for a zero radius (`0x0070B186`).
pub(crate) fn entity_sight_footprint(
    fog: &FogState,
    entity: &crate::sim::game_entity::GameEntity,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    sight_ability: bool,
    revealer: InternedId,
) -> Option<SightAdmission> {
    entity_sight_admission(
        fog,
        entity,
        revealer,
        config,
        height_grid,
        terrain,
        sight_ability,
        None,
    )
    .filter(|admission| admission.radius != 0)
}

/// The footprint `TechnoClass::UpdateReveal @ 0x0070AF50` reveals for
/// `entity`, recorded as a reveal by `revealer`; `None` without a fog plane.
pub(crate) fn entity_sight_admission(
    fog: &FogState,
    entity: &crate::sim::game_entity::GameEntity,
    revealer: InternedId,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    sight_ability: bool,
    radius_override: Option<i32>,
) -> Option<SightAdmission> {
    if entity.sight.is_zero() && radius_override.is_none() {
        return None;
    }
    let geometry = entity_sight_geometry(
        entity,
        revealer,
        config,
        terrain,
        sight_ability,
        radius_override,
    );
    sight_admission(fog, geometry, config, height_grid, terrain)
}

/// The sight admission of a source with `geometry`: its footprint under
/// `config` and `height_grid` ([`collect_reveal_cells`]); none on an
/// unallocated map.
fn sight_admission(
    fog: &FogState,
    geometry: SightGeometry,
    config: &VisionConfig,
    height_grid: Option<&[u8]>,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
) -> Option<SightAdmission> {
    let width = fog.width;
    let height = fog.height;
    if width == 0 || height == 0 {
        return None;
    }
    let (rx, ry, height_leptons) = geometry.origin;
    let cells = collect_reveal_cells(
        rx,
        ry,
        geometry.radius.clamp(0, i32::from(MAX_SIGHT_RANGE)) as u16,
        height_leptons,
        config.reveal_by_height,
        height_grid,
        (width, height),
        config.map_size,
    );
    // This vector indexes derived real-cell sight counts, not native receiver
    // calls. Map5678E0 still visits every geometry target (including the shared
    // Dummy) through reveal_map_cell. Missing native slots must not fabricate
    // source counts in unrelated rectangular storage; aliases use the actual
    // Cell's coordinates. An isolated lookup preserves the live Dummy here.
    let cells = if let Some(terrain) = terrain {
        let query = crate::map::resolved_terrain::NativeCellQuery::isolated(terrain);
        cells
            .into_iter()
            .filter_map(|(x, y)| {
                let identity = query.lookup((x as i16, y as i16));
                match identity {
                    crate::map::cell_index::NativeCellIdentity::Real(_) => {
                        let (x, y) = query.coord(identity);
                        Some((x as u16, y as u16))
                    }
                    crate::map::cell_index::NativeCellIdentity::Dummy => None,
                }
            })
            .collect()
    } else {
        cells
    };
    Some(SightAdmission {
        latched: true,
        owner: geometry.owner,
        origin: geometry.origin,
        radius: geometry.radius,
        cells,
    })
}

/// A sight admission's revealer, origin and radius.
#[derive(Clone, Copy)]
pub(crate) struct SightGeometry {
    pub(crate) owner: InternedId,
    pub(crate) origin: (u16, u16, i32),
    pub(crate) radius: i32,
}

/// [`entity_sight_admission`] without its footprint.
pub(crate) fn entity_sight_geometry(
    entity: &crate::sim::game_entity::GameEntity,
    revealer: InternedId,
    config: &VisionConfig,
    terrain: Option<&crate::map::resolved_terrain::ResolvedTerrainGrid>,
    sight_ability: bool,
    radius_override: Option<i32>,
) -> SightGeometry {
    // `0x0070AF9D..0x0070AFB8` copies the object's Location (`+0x9C..+0xA4`):
    // its world Z, a Hover's lift, a slope, a deck and a flight altitude
    // included ([`ground_pose::object_world_z_leptons`]).
    let height_leptons: i32 = ground_pose::object_world_z_leptons(entity, terrain);

    // Elevation raises sight MULTIPLICATIVELY, off the object's world Z in
    // leptons, not additively off its terrain level:
    //   sight = trunc(Sight * (1 + 0.10 * trunc(Z_leptons / LeptonsPerSightIncrease)))
    let base_range: i32 = entity.sight.range();
    let elev_steps: i32 = if config.leptons_per_sight_increase > 0 {
        height_leptons / config.leptons_per_sight_increase
    } else {
        0
    };
    //Widen the existing deterministic hundredths scaling before truncation;
    //raw Type Sight no longer silently truncates to WORD/clamps10. An i128
    //intermediate covers every i32 Sight/Z product before division, including
    //LeptonsPerSightIncrease=1; narrowing consumes the existing EAX model.
    //RESIDUAL: x87 threshold rounding outside the consumed sight controls is
    //not certified by this rational form; no x87 emulation is introduced.
    let scale = 100_i128 + i128::from(ELEVATION_SIGHT_PERCENT_PER_STEP) * i128::from(elev_steps);
    let with_elevation = ((i128::from(base_range) * scale) / 100) as i32;
    // `TechnoClass::UpdateReveal @ 0x0070AF50`: the elevation-scaled integer
    // is then `ftol(sight * Rules.VeteranSight)` for a `SIGHT` holder, gated
    // on the double not being exactly 0.0 (`0x0070B082..0x0070B0A4`).
    let with_veterancy: i32 = crate::sim::combat::veterancy::veteran_sight_cells(
        with_elevation,
        sight_ability,
        config.veteran_sight,
    );
    //70B0D8 stores the raw effective260. Map5678E0 clamps only its walk,
    //and border567DA0 independently clamps cached260+3 to3..11.
    let effective = radius_override.unwrap_or(with_veterancy);
    SightGeometry {
        owner: revealer,
        origin: (entity.position.rx(), entity.position.ry(), height_leptons),
        radius: effective,
    }
}

/// One source's admissions as the fog pass reads them, in viewer order:
/// (viewer, owner revealed for, latched) ([`FogState::read_source_admissions`]).
#[derive(Debug, Default)]
pub(crate) struct SourceAdmissions(Vec<(InternedId, InternedId, bool)>);

impl SourceAdmissions {
    /// An inactive source cache belongs to this owner until an explicit
    /// native reveal event updates it. A frame must never relatch it.
    pub(crate) fn cache_is_inactive(&self, owner: InternedId) -> bool {
        let mut owned = self.0.iter().filter(|&&(_, by, _)| by == owner).peekable();
        owned.peek().is_some() && owned.all(|&(_, _, latched)| !latched)
    }

    /// Whether the retained admissions are exactly one per viewer of
    /// `viewers` (in key order), each revealed for `owner`. Then the source's
    /// footprints stand until a reveal event moves them.
    pub(crate) fn retained_cover(&self, owner: InternedId, viewers: &[InternedId]) -> bool {
        self.0
            .iter()
            .filter(|&&(_, _, latched)| latched)
            .map(|&(viewer, by, _)| (viewer, by))
            .eq(viewers.iter().map(|&viewer| (viewer, owner)))
    }
}

/// Every admission key of one source, by its stable id.
fn source_keys(stable_id: u64) -> std::ops::RangeInclusive<(u64, InternedId)> {
    (stable_id, InternedId::default())..=(stable_id, InternedId::from_index(u32::MAX))
}

/// VERA-only: fog bounds for fixture sims built without a map descriptor; the
/// path grid, else the farthest unlimboed entity cell, is the source of truth.
fn resolve_bounds(entities: &EntityStore, path_grid: Option<&PathGrid>) -> (u16, u16) {
    if let Some(grid) = path_grid {
        return (grid.width(), grid.height());
    }

    let mut max_x = 0u16;
    let mut max_y = 0u16;
    let mut found = false;
    for entity in entities.values() {
        if entity.lifecycle.in_limbo() {
            continue;
        }
        found = true;
        max_x = max_x.max(entity.position.rx());
        max_y = max_y.max(entity.position.ry());
    }
    if found {
        (max_x.saturating_add(1), max_y.saturating_add(1))
    } else {
        (0, 0)
    }
}

// Geometry-only adapter for existing kernel fixtures; production owns mapping
// events in the source/fire/Psychic writers above, never in this test adapter.
#[cfg(any(test, feature = "test-support"))]
fn reveal_radius_into(
    vis: &mut OwnerVisibility,
    rx: u16,
    ry: u16,
    range: u16,
    height: i32,
    by_height: bool,
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
        (width, grid_height),
        None,
    ) {
        vis.mark_sight(x, y);
    }
}

/// The local XY after Map5673A0/5678E0's first height translation.
/// Border567DA0 applies this same numeric owner to its own supplied XYZ;
/// the uncached5673A0 tail supplies its already-translated local XYZ.
fn height_shifted_cell_origin(origin: (u16, u16, i32)) -> (i32, i32) {
    let shift = iso_height_shift_cells(origin.2);
    (i32::from(origin.0) - shift, i32::from(origin.1) - shift)
}

/// Map5673A0 validates the lifted Size center5674AC..E4 and zero radius
///5674F0 before its walk. Return admission separately from LOS membership:
/// an admitted walk with every receiver blocked still calls border5678CA.
fn reveal_area_origin(
    origin: (u16, u16, i32),
    radius: u16,
    map_size: Option<(i32, i32)>,
) -> Option<(i32, i32)> {
    if radius == 0 {
        return None;
    }
    let center = height_shifted_cell_origin(origin);
    if map_size.is_some_and(|(width, height)| {
        !crate::map::playfield::size_diamond_contains(
            width,
            height,
            (center.0 as i16, center.1 as i16),
        )
    }) {
        return None;
    }
    Some(center)
}

/// The cells `MapClass::RevealArea @ 0x005678E0` hands its leaf, in its
/// order; `RevealShroud @ 0x005673A0` walks the same cells.
///
/// The walk takes the engine's reveal spiral, `REVEAL_SPIRAL[0 ..
/// RING_SIZES[range]]` (the offset table `0xABD490` and the count table
/// `0x007ED3D0`), with no special case at any radius: 0 reveals nothing and
/// more than 10 is 10 (`0x00567A3E..0x00567A59`). Every entry, ring 10
/// included, passes through the same height line-of-sight gate.
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
///
/// ## Map bounds
/// With a map `Size=` diamond (`MapClass+0xF4/+0xF8`), a lifted centre off
/// it reveals nothing (`0x005679FC..0x00567A38`) and each cell off it is
/// skipped (`0x00567B66..0x00567BA0`); `grid` (the fog grid's width and
/// height) bounds what VERA stores. The walk's `|dx| > range` and truncated
/// distance tests (`0x00567BA6..0x00567C0A`) never reject a spiral cell.
///
/// RESIDUALS, dormant:
/// - In GameMode 3 or 4 with a session object (`0x00A8B23C`) whose vt+4
///   answers false, three fixed cells are skipped (`0x005679C8..0x005679F7`,
///   `0x00567D17..0x00567D5E`); VERA has no session object.
/// - With `RevealByHeight=` off, a caller's outline argument (arg 3, the
///   Techno sight callers') walks only the outer three rings
///   (`0x00567A77..0x00567AA5`); retail rules leave the key on.
pub(crate) fn collect_reveal_cells(
    center_rx: u16,
    center_ry: u16,
    range: u16,
    viewer_height_leptons: i32,
    reveal_by_height: bool,
    height_grid: Option<&[u8]>,
    (width, height): (u16, u16),
    map_size: Option<(i32, i32)>,
) -> Vec<(u16, u16)> {
    let Some((cx, cy)) = reveal_area_origin(
        (center_rx, center_ry, viewer_height_leptons),
        range,
        map_size,
    ) else {
        return Vec::new();
    };

    let mut cells = Vec::new();
    let viewer_level = viewer_height_leptons / LEPTONS_PER_HEIGHT_LEVEL;
    let z_shift = iso_height_shift_cells(viewer_height_leptons);
    let w = i32::from(width);
    let h = i32::from(height);
    let on_map = |x: i32, y: i32| {
        map_size.is_none_or(|(size_w, size_h)| {
            crate::map::playfield::size_diamond_contains(size_w, size_h, (x as i16, y as i16))
        })
    };
    // Clamp range to MAX_SIGHT_RANGE (the original also clamps to 10).
    let clamped = (range as usize).min(MAX_SIGHT_RANGE as usize);
    let spiral_end = REVEAL_RING_SIZES[clamped];

    for i in 0..spiral_end {
        let (dx, dy) = REVEAL_SPIRAL[i];
        let rx = cx + dx as i32;
        let ry = cy + dy as i32;
        if on_map(rx, ry) && rx >= 0 && rx < w && ry >= 0 && ry < h {
            // Height-based LOS: check whether terrain at the obstruction cell
            // blocks sight. The original engine samples the cell at
            // `foot_target + mirror[i] + (2, 2)` — the per-entry mirror steps one
            // cell back toward the viewer, plus a fixed +2 on each axis baked into
            // the original's obstruction math. The obstruction is relative to the
            // raw foot cell, so we add `z_shift` back to undo the spiral's Z-shift
            // (in the original this cancellation is implicit). If that cell's Level
            // exceeds viewer_level + 3, the target is not revealed (LOS blocked).
            if reveal_by_height && let Some(hg) = height_grid {
                let (mdx, mdy) = REVEAL_MIRROR[i];
                let obs_x = rx + mdx as i32 + 2 + z_shift;
                let obs_y = ry + mdy as i32 + 2 + z_shift;
                if obs_x >= 0 && obs_x < w && obs_y >= 0 && obs_y < h {
                    // Original567CB5 MOVSX reads Cell+11B as a signed byte.
                    let obs_level = i32::from(hg[(obs_y * w + obs_x) as usize] as i8);
                    if viewer_level + 3 < obs_level {
                        continue; // terrain blocks LOS
                    }
                }
            }
            cells.push((rx as u16, ry as u16));
        }
    }
    cells
}

/// Original Map567DA0 ground-frame refresh geometry. Unlike RevealArea,
/// height zero takes the whole clamped3..11 disc, without LOS. The selected
/// frontier path uses finalarg0; nonzero skips the traversal. Executed table:
/// original561910, tools' consumed reveal spiral corpus (Steam3e81a617).
/// Its independent inner-height argument selects an inner ring; the ordinary
/// Unlimbo caller passes0 even when source XYZ.Z is nonzero.
pub(crate) fn sight_border_cells(
    origin: (u16, u16, i32),
    radius: i32,
    inner_height: i32,
) -> Vec<(u16, u16)> {
    let center = height_shifted_cell_origin(origin);
    let center = (center.0 as i16, center.1 as i16);
    let end = REVEAL_RING_SIZES[radius.clamp(3, 11) as usize];
    let start = if inner_height <= 0 {
        0
    } else {
        REVEAL_RING_SIZES[inner_height.min(8) as usize - 1]
    };
    REVEAL_SPIRAL[start..end]
        .iter()
        .map(|&(x, y)| {
            (
                center.0.wrapping_add(i16::from(x)) as u16,
                center.1.wrapping_add(i16::from(y)) as u16,
            )
        })
        .collect()
}

/// Reveal spiral table extracted from the original engine.
/// Each (dx, dy) is a cell offset from the revealing unit's position.
/// Entries are ordered in expanding rings by sight radius.
///
/// Recovered whole from the engine's table initialiser, which writes every
/// entry as a literal `(dy << 16) | dx` store or a two-argument coordinate
/// call — the table itself lives in zero-initialised data, so reading the
/// image gives nothing. Ordinary radius0..10 membership follows
/// `max(|dx|,|dy|) + min(|dx|,|dy|)/2 == r` (truncating division).
/// Radius11 is the original border-only tail, including its irregular entries
/// and duplicate. Executed561910 in shroud_frontier.json checks all369 pairs
/// and cumulative counts; the ordinary prefix also has independent metric tests.
#[rustfmt::skip]
const REVEAL_SPIRAL: [(i8, i8); 369] = [
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
    // Border567DA0 radius11, original561910 execution (shared spiral owner).
    (0, 11), (0, -11), (-1, 11), (1, 11), (-1, -11), (1, -11), (-2, 11), (2, 11),
    (-2, -11), (2, -11), (-3, 11), (3, 11), (-3, -11), (-3, 11), (-4, 9), (4, 9),
    (-4, -9), (4, -9), (-5, 9), (5, 9), (-5, -9), (5, -9), (-6, 8), (6, 8),
    (-6, -8), (6, -8), (-7, 8), (7, 8), (-7, -8), (7, -8), (-8, 7), (8, 7),
    (-8, -7), (8, -7), (-8, 6), (8, 6), (-8, -6), (8, -6), (-9, 5), (9, 5),
    (-9, -5), (9, -5), (-9, 4), (9, 4), (-9, -4), (9, -4), (-10, 3), (10, 3),
    (-10, -3), (10, -3), (-10, 2), (10, 2), (-10, -2), (10, -2), (-11, 1), (11, 1),
    (-11, -1), (11, -1), (11, 0), (-11, 0),

];

/// Cumulative entry count for each sight radius 0–11, read from the engine's
/// read-only data. To reveal cells for sight N, iterate
/// `REVEAL_SPIRAL[0..REVEAL_RING_SIZES[N]]`.
///
/// RevealArea clamps at10; the shared Border567DA0 traversal reaches11.
const REVEAL_RING_SIZES: [usize; 12] = [1, 9, 21, 37, 61, 89, 121, 161, 205, 253, 309, 369];

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

/// `0x00487950`: `viewer`'s map shrouds the centre of the Cell at `cell`,
/// raised to its floor (`0x0047B3A0`): [`coordinate_is_shrouded`] there, a
/// Cell being open when `viewer` has mapped its ground, including retained
/// canonical Dummy knowledge after a mapping of an absent allocated slot.
/// A query error counts as unshrouded. Callers that test a Cell's own
/// coordinate (CellClass vt+0x48) against `MapClass::IsShrouded` ask the same.
pub fn cell_is_shrouded(
    fog: &FogState,
    cells: &crate::map::resolved_terrain::NativeCellQuery<'_>,
    viewer: InternedId,
    cell: (u16, u16),
) -> bool {
    let point = crate::sim::movement::target_cell_coord(cell.0, cell.1, Some(cells));
    point_is_shrouded(fog, cells, viewer, point)
}

/// `viewer`'s map shrouds `point`: [`coordinate_is_shrouded`] with
/// [`cell_ground_mapped`] as its `open`. A query error counts as unshrouded.
pub fn point_is_shrouded(
    fog: &FogState,
    cells: &crate::map::resolved_terrain::NativeCellQuery<'_>,
    viewer: InternedId,
    point: crate::sim::components::DriveCoord,
) -> bool {
    let terrain = cells.terrain();
    let open = |cell| Ok(cell_ground_mapped(fog, terrain, viewer, cell));
    matches!(coordinate_is_shrouded(cells, point, &open), Ok(true))
}

/// `viewer` has mapped `cell`'s ground, the `Cell+0x12C & 0x08` that
/// `MapClass::IsShrouded @ 0x00586360` tests: [`ShroudKnowledge::open`] as
/// published to [`FogState::is_cell_revealed`], the copy every native writer
/// of that bit reaches (the gap `0x006FB170`/`0x006FB470` and some
/// whole-map writers still move it alone). The canonical Dummy answers its
/// retained record, which only the
/// cell writers touch. [`point_is_shrouded`] and the Foot click queries use
/// it; Display4AE5D0 reads this same bit on its unprojected plane receiver.
/// This read-only decision is public so app consumers use the same authority,
/// rather than the legacy edge-frame ground cache. Gap6FB3B8 clears bit8;
/// every such writer reaches the published knowledge this owner reads.
pub fn cell_ground_mapped(
    fog: &FogState,
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    viewer: InternedId,
    cell: crate::map::cell_index::NativeCellIdentity,
) -> bool {
    match cell {
        crate::map::cell_index::NativeCellIdentity::Real(index) => {
            let cell = &terrain.cells()[index];
            fog.is_cell_revealed(viewer, cell.rx, cell.ry)
        }
        crate::map::cell_index::NativeCellIdentity::Dummy => fog.is_dummy_ground_unshrouded(viewer),
    }
}

/// A flat fire-leaf reveal of `range` cells around a cell, with no height
/// shift, height line of sight or map diamond (fixtures).
#[cfg(any(test, feature = "test-support"))]
pub fn reveal_radius(
    fog: &mut FogState,
    owner: InternedId,
    center_rx: u16,
    center_ry: u16,
    range: u16,
) {
    let cells = collect_reveal_cells(
        center_rx,
        center_ry,
        range,
        0,
        false,
        None,
        (fog.width, fog.height),
        None,
    );
    fire_reveal_cells(fog, owner, cells);
}

/// Test adapter for the fire receiver's radius3 geometry. Production FireAt
/// uses Simulation::reveal_area(Fire) and both shared border calls; this
/// adapter alone does not reproduce the enclosing Map/cache sequence.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn reveal_shroud_on_fire(
    fog: &mut FogState,
    house: InternedId,
    coord: crate::sim::components::DriveCoord,
    config: &VisionConfig,
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
        config.reveal_by_height,
        height_grid,
        (fog.width, fog.height),
        config.map_size,
    );
    fire_reveal_cells(fog, house, cells);
}

/// FireAt's literal radius3 for Map5673A0 at6FDF30/6FF71B.
#[cfg(any(test, feature = "test-support"))]
const REVEAL_ON_FIRE_RADIUS: u16 = 3;

/// The fire leaf `0x004876F0` over collected cells: unlike Psychic's
/// reduce/increase pair, it never changes the shroud counter and tests
/// counter > 0 for pending.
#[cfg(any(test, feature = "test-support"))]
fn fire_reveal_cells(fog: &mut FogState, owner: InternedId, cells: Vec<(u16, u16)>) {
    let width = fog.width;
    let height = fog.height;
    if width == 0 || height == 0 {
        return;
    }
    for cell in cells {
        fog.reveal_map_cell(owner, cell, false, None, MapRevealProjection::Fire);
    }
}

/// The houses a reveal by `owner` can reach (every house with a map or an
/// alliance entry, and `owner`) that are `owner` itself, when `with_owner`,
/// or whose name `admits` passes; ascending, without repeats.
fn reveal_candidates(
    fog: &FogState,
    alliances: &HouseAllianceMap,
    owner: InternedId,
    interner: &StringInterner,
    with_owner: bool,
    admits: impl Fn(&str) -> bool,
) -> Vec<InternedId> {
    let mut viewers: Vec<InternedId> = fog
        .by_owner
        .keys()
        .copied()
        .chain(std::iter::once(owner))
        .filter(|&viewer| (with_owner && viewer == owner) || admits(interner.resolve(viewer)))
        .collect();
    // An alliance entry's house is the interned name itself; `owner` is
    // already in when `with_owner` keeps it.
    for (house, allies) in alliances {
        for name in std::iter::once(house).chain(allies) {
            if admits(name)
                && let Some(id) = interner.get(name)
            {
                viewers.push(id);
            }
        }
    }
    viewers.sort_unstable();
    viewers.dedup();
    viewers
}

/// The house gate of `MapClass::RevealArea @ 0x005678E0`
/// (`0x00567AB0..0x00567B12`), asked with `viewer` as the player whose map
/// the leaf writes: a reveal by `owner` maps `owner`'s own cells, and an
/// allied house's when `[AudioVisual] AllyReveal=` is set (`0x00567AEF`).
///
/// RESIDUAL: a house that infiltrated `owner`'s radar (the `+0x54E4` bit
/// of its index, `0x00567AC1..0x00567ADB`) is mapped too, allied or not;
/// VERA keeps no radar-spy bits.
pub(crate) fn reveal_admits(
    alliances: &HouseAllianceMap,
    owner: InternedId,
    viewer: InternedId,
    config: &VisionConfig,
    interner: &StringInterner,
) -> bool {
    viewer == owner
        || ally_reveal_test(alliances, owner, config, interner)(interner.resolve(viewer))
}

/// [`reveal_admits`]'s allied arm for one `owner`, to ask of many viewers by
/// name: `AllyReveal=` and `owner`'s IsAlliedWith ([`allied_viewers`]).
fn ally_reveal_test<'a>(
    alliances: &'a HouseAllianceMap,
    owner: InternedId,
    config: &'a VisionConfig,
    interner: &'a StringInterner,
) -> impl Fn(&str) -> bool + 'a {
    let allied = crate::map::houses::ally_test(alliances, interner.resolve(owner));
    move |viewer| config.ally_reveal && allied(viewer)
}

/// The maps a reveal by `owner` writes ([`reveal_admits`]).
pub(crate) fn direct_reveal_viewers(
    fog: &FogState,
    alliances: &HouseAllianceMap,
    owner: InternedId,
    config: &VisionConfig,
    interner: &StringInterner,
) -> Vec<InternedId> {
    reveal_candidates(
        fog,
        alliances,
        owner,
        interner,
        true,
        ally_reveal_test(alliances, owner, config, interner),
    )
}

/// `owner` and every house `owner`'s IsAlliedWith names, whatever `AllyReveal=`
/// says. `HouseClass::IsAlliedWith @ 0x004F9A50` reads `owner`'s own ally bits
/// (`+0x5788`), one-way; a house is its own ally.
pub(crate) fn allied_viewers(
    fog: &FogState,
    alliances: &HouseAllianceMap,
    owner: InternedId,
    interner: &StringInterner,
) -> Vec<InternedId> {
    let allied = crate::map::houses::ally_test(alliances, interner.resolve(owner));
    reveal_candidates(fog, alliances, owner, interner, false, allied)
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
#[cfg(any(test, feature = "test-support"))]
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
#[cfg(any(test, feature = "test-support"))]
pub fn apply_gap_generators(
    fog: &mut FogState,
    alliances: &HouseAllianceMap,
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
    apply_gap_generator_sources(fog, alliances, &sources, interner);
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn apply_gap_generator_sources(
    fog: &mut FogState,
    alliances: &HouseAllianceMap,
    gap_generators: &[GapGeneratorSource],
    interner: &StringInterner,
) {
    apply_gap_generator_sources_with_spy_sat(
        fog,
        alliances,
        gap_generators,
        interner,
        &std::collections::BTreeSet::new(),
    );
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn apply_gap_generator_sources_with_spy_sat(
    fog: &mut FogState,
    alliances: &HouseAllianceMap,
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
        let previous = fog.gap_sources.entry(viewer).or_default();
        let mut admitted = std::collections::BTreeSet::new();
        vis.clear_gap_fog();
        for &generator in gap_generators {
            if generator.radius <= 0 {
                continue;
            }
            if !are_houses_friendly(
                alliances,
                interner.resolve(generator.owner),
                interner.resolve(viewer),
            ) {
                admitted.insert(generator);
            } else {
                for index in gap_footprint(generator, width, height) {
                    vis.set_gap_fog(index);
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
    alliances: &HouseAllianceMap,
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
        let receipts = fog.gap_sources.entry(viewer).or_default();
        if active {
            if !are_houses_friendly(
                alliances,
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
}

/// Passive projection of retained deposits. It must never reclassify power or
/// emit add/remove events during House, restore, or ordinary view rebuilding.
/// Friendly Cell13C remains the existing boolean fog projection.
pub(crate) fn materialize_gap_generator_sources(
    fog: &mut FogState,
    alliances: &HouseAllianceMap,
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
        vis.clear_gap_fog();
        for &source in sources.get(&viewer).into_iter().flatten() {
            if are_houses_friendly(
                alliances,
                interner.resolve(source.owner),
                interner.resolve(viewer),
            ) {
                for index in gap_footprint(source, width, height) {
                    vis.set_gap_fog(index);
                }
            }
        }
        // The gap fog is not a published bit, so a tracked grid's published
        // bits still match its knowledge.
        if !vis.tracked.complete || visit_every_cell() {
            for index in 0..vis.cells.len() {
                vis.publish_knowledge(index);
            }
        }
    }
}

#[cfg(test)]
mod tracked_cells_tests;

#[cfg(test)]
mod vision_tests;

#[cfg(test)]
mod reveal_area_tests;

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
