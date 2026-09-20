//! Ore growth and spread system — data-driven from rules.ini and map INI.
//!
//! The active YR per-type queues read and write `OverlayGrid` directly. The
//! older scan/reservoir path remains only for tests without native registries.
//! Native gates (see [`OreGrowthConfig::resolve`]):
//! - map INI [Basic] `TiberiumGrowthEnabled` (`ScenarioClass+0x34A6`) gates
//!   both per-type drivers;
//! - `ScenarioClass` flags bits `0x40`/`0x80` (`TiberiumGrows`/
//!   `TiberiumSpreads`): forced on at every skirmish/multiplayer start, read
//!   from the map `[SpecialFlags]` only in GameMode 0;
//! - rules.ini [General] `GrowthRate` drives only the legacy scan fallback.
//!
//! ## Algorithm (matching RA1 MapClass::Logic)
//! 1. Incremental scan: each tick processes a fraction of the map
//! 2. Collect growth/spread candidates via reservoir sampling
//! 3. When full scan completes: execute growth, then spread
//! 4. Growth = increase ore remaining by one richness level (ore only, not gems)
//! 5. Spread = spawn new ore in a random adjacent empty+walkable cell
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/miner (ResourceNode, ResourceType),
//!   sim/pathfinding (PathGrid), sim/rng (SimRng), rules/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

use crate::map::authored_overlay::NativeOverlayMapShape;
use crate::map::basic::{BasicSection, SpecialFlagsSection};
use crate::map::overlay_types::OverlayTypeRegistry;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::GeneralRules;
use crate::rules::tiberium_type::{TiberiumTypeId, TiberiumTypeRegistry};
use crate::sim::miner::{ResourceNode, ResourceType};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::rng::SimRng;
use crate::sim::tiberium::{
    NativeCellObjectView, NewTiberiumAdmission, PlaceTiberiumContext,
    TiberiumPlacementObjectContext, can_place_new_tiberium, place_tiberium,
};
use crate::util::fixed_math::SimFixed;
use crate::util::native_x87::{NativeF64Bits, X87Chop53, X87Ordering};

/// The `1e-05` double at `0x007E3810` every tiberium percentage gate compares
/// against (`CanGrowTiberium @ 0x00483620`, `CanSpreadTiberium @ 0x00483690`,
/// `GrowthProcessor @ 0x00722F00`, `SpreadProcessor @ 0x00722440`).
const NATIVE_PERCENT_MIN_BITS: u64 = 0x3EE4_F8B5_88E3_68F1;
/// Growth queue admission literal: `AddToGrowthQueue @ 0x007235A0` and the
/// processor reinsert test `OverlayData < 0x0B` (not `MaxDensity - 1`).
const GROWTH_QUEUE_DENSITY_LIMIT: u8 = 0x0B;
/// `GrowthProcessor`: rebuild when `heap count > capacity - 2 * batch`.
const GROWTH_PROCESSOR_REBUILD_BATCH_FACTOR: i64 = 2;
/// `SpreadProcessor`: rebuild when `heap count > capacity - 0x14`.
const SPREAD_PROCESSOR_REBUILD_SLACK: i64 = 0x14;

/// Base ore stock per richness level — matches seed_resource_nodes_from_overlays().
const ORE_BASE_PER_LEVEL: u16 = 120;
/// Maximum ore richness = 12 levels (OverlayData 0-11 in RA1).
const MAX_ORE_LEVELS: u16 = 12;
/// Maximum ore `remaining` value (12 levels * 120 per level).
const MAX_ORE_REMAINING: u16 = ORE_BASE_PER_LEVEL * MAX_ORE_LEVELS;
/// Ore must be above this threshold to spread (>6 levels, matching RA1 OverlayData > 6).
const SPREAD_THRESHOLD: u16 = ORE_BASE_PER_LEVEL * 6;
/// Max candidates collected per scan cycle (bounded like RA1's fixed-size arrays).
const MAX_CANDIDATES: usize = 50;
/// Native AddToGrowthQueue priority jitter span.
const GROWTH_QUEUE_PRIORITY_WINDOW: u32 = 50;
const GROWTH_BATCH_MIN: u32 = 5;
const GROWTH_BATCH_MAX: u32 = 50;
const SPREAD_BATCH_MIN: u32 = 5;
const SPREAD_BATCH_MAX: u32 = 25;
/// `GrowthDriver_AllTypes @ 0x00722CA9`: the `0.3` double at `0x007E5138`
/// (bytes `33 33 33 33 33 33 D3 3F`) loaded when `ScenarioClass` flags bit
/// `0x40` (`TiberiumGrows`) is set.
const NATIVE_GROWTH_RELOAD_FAST_MULTIPLIER_BITS: u64 = 0x3FD3_3333_3333_3333;
/// `0x00722CB1`: the `1.0` double at `0x007E1718` (`00 .. F0 3F`) loaded
/// when the bit is clear.
const NATIVE_GROWTH_RELOAD_UNIT_MULTIPLIER_BITS: u64 = 0x3FF0_0000_0000_0000;
const SPREAD_GERMINATION_DENSITY: u8 = 3;

/// 8 adjacent directions for spread: N, NE, E, SE, S, SW, W, NW.
const ADJACENT_OFFSETS: [(i32, i32); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];

/// Effective ore growth configuration resolved once at map load.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OreGrowthConfig {
    /// `ScenarioClass+0x34A6` (`[Basic] TiberiumGrowthEnabled`, default 1 from
    /// `Set_Defaults @ 0x00683848`, read at `Read_INI_Basic @ 0x0068A57A`):
    /// the entry gate of both `GrowthDriver_AllTypes @ 0x00722C48` and
    /// `SpreadDriver_AllTypes @ 0x007221B8`, and of `CanGrowTiberium`.
    pub grows: bool,
    /// `ScenarioClass` flags bit `0x80` (`TiberiumSpreads`): the
    /// `CellClass::CanSpreadTiberium @ 0x00483690` gate.
    pub spreads: bool,
    /// `ScenarioClass` flags bit `0x40` (`TiberiumGrows`): selects the
    /// `Growth * 0.3` growth-timer reload (`0x00722CA4`).
    #[serde(default)]
    pub tiberium_grows_flag: bool,
    /// Seconds per full map growth scan cycle (from GrowthRate= in minutes, converted
    /// to integer seconds at config construction to avoid f32 in the tick path).
    pub growth_rate_seconds: u32,
}

impl OreGrowthConfig {
    /// Resolve the native gates for this session.
    ///
    /// `[Basic] TiberiumGrowthEnabled` is read in every game mode. The
    /// `TiberiumGrows`/`TiberiumSpreads` flag bits come from the session when
    /// GameMode != 0 (`Full_Init @ 0x00687C23` overwrites the scenario dword
    /// with the session dword every skirmish/multiplayer start forces to
    /// `| 0xC0`) and from the map `[SpecialFlags]` only in GameMode 0
    /// (`0x006B8CA0`, default = the pre-existing bit; VERA keeps `true` there,
    /// gamemd's pre-read campaign default UNCHECKED). `[General]
    /// TiberiumGrows/TiberiumSpreads` have no gamemd reader (the only readers of
    /// those key strings are `[SpecialFlags]` I/O at `0x006B8B30`/`0x006B8CA0`
    /// and `[MultiplayerDialogSettings]` at `0x006720AA`) and no longer gate.
    /// GrowthRate comes only from rules.ini (legacy scan fallback).
    pub fn resolve(
        general: &GeneralRules,
        basic: &BasicSection,
        special_flags: &SpecialFlagsSection,
        session: &crate::sim::scenario_session::ScenarioSession,
    ) -> Self {
        let grows = basic.tiberium_growth_enabled.unwrap_or(true);
        let (tiberium_grows_flag, spreads) = if session.game_mode_nonzero {
            (session.tiberium_grows_flag, session.tiberium_spreads_flag)
        } else {
            (
                special_flags.tiberium_grows.unwrap_or(true),
                special_flags.tiberium_spreads.unwrap_or(true),
            )
        };
        let growth_rate_minutes = general.growth_rate_minutes.max(0.01);
        // Convert f32 minutes → integer seconds at the INI boundary via
        // fixed-point to avoid platform-dependent f32 multiplication rounding.
        let rate_fixed = SimFixed::saturating_from_num(growth_rate_minutes);
        let growth_rate_seconds =
            (rate_fixed * SimFixed::from_num(60)).to_num::<i32>().max(1) as u32;

        log::info!(
            "OreGrowthConfig: grows={}, spreads={}, tiberium_grows_flag={}, rate={}s",
            grows,
            spreads,
            tiberium_grows_flag,
            growth_rate_seconds,
        );

        Self {
            grows,
            spreads,
            tiberium_grows_flag,
            growth_rate_seconds,
        }
    }

    /// Disabled config — no growth or spread.
    pub fn disabled() -> Self {
        Self {
            grows: false,
            spreads: false,
            tiberium_grows_flag: false,
            growth_rate_seconds: 300, // 5 minutes
        }
    }
}

/// `TiberiumClass::GrowthDriver_AllTypes @ 0x00722C40`, reload at
/// `0x00722C9E..0x00722CE3`: after `GrowthProcessor` returns, `FLD` the `0.3`
/// double (`ScenarioClass` flags `& 0x40`) or `1.0`, `FILD` the `Growth=`
/// int (`TiberiumClass+0xA8`), `FMUL`, then `Math::ftol @ 0x007C5F00`
/// (`FISTP qword`), storing the low dword into the timer duration
/// (`TiberiumClass+0x124`).
///
/// Rounding: `WinMain @ 0x006BBFC1` calls `_controlfp(_RC_CHOP, _MCW_RC)`
/// (`0x300, 0x300`) and `0x007C5EE4` then captures that control word
/// (`0x0E7F`: 53-bit precision, round toward zero) for `ftol`, so both the
/// multiply and the conversion truncate. Stock `Growth=2200` therefore reloads
/// to **659** (2200 x 0.3 chops just below 660), `Growth=10000` to 2999.
pub(crate) fn native_growth_timer_reload(growth: u32, tiberium_grows_flag: bool) -> u32 {
    let multiplier_bits = if tiberium_grows_flag {
        NATIVE_GROWTH_RELOAD_FAST_MULTIPLIER_BITS
    } else {
        NATIVE_GROWTH_RELOAD_UNIT_MULTIPLIER_BITS
    };
    let multiplier = X87Chop53::load_f64(NativeF64Bits::from_bits(multiplier_bits))
        .expect("retail reload multipliers are finite normals");
    let product = X87Chop53::mul(X87Chop53::load_i32(growth as i32), multiplier);
    // The signed-dword input and multiplier at most 1.0 fit signed64;
    // native keeps EAX (the low dword).
    X87Chop53::ftol_i32_low_masked(product) as u32
}

/// Queued ore growth cell inserted by native-style AddToGrowthQueue callers.
///
/// Native stores queue priority as a float. This keeps the same observable
/// priority shape while leaving execution to an explicit future queue processor.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OreGrowthQueueEntry {
    pub rx: u16,
    pub ry: u16,
    pub priority: f32,
}

/// Native-style spread queue entry inserted by `Reduce_Tiberium` full removal.
///
/// The full queue processor is still being ported; this state captures the
/// deterministic membership/reseed side effect so depletion no longer drops it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct OreSpreadQueueEntry {
    pub resource_type: ResourceType,
    pub rx: u16,
    pub ry: u16,
}

/// Native `TiberiumClass` queue/timer state shell.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct NativeTiberiumState {
    pub classes: Vec<NativeTiberiumClassState>,
}

/// Per-type native growth/spread scheduler state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NativeTiberiumClassState {
    pub growth_timer: NativeTiberiumTimer,
    pub spread_timer: NativeTiberiumTimer,
    /// `TiberiumClass+0x110/+0x114/+0x118` growth queue store.
    pub growth: NativeTiberiumQueue,
    /// `TiberiumClass+0xF4/+0xF8/+0xFC` spread queue store.
    pub spread: NativeTiberiumQueue,
    /// Growth flag-byte plane (`+0x114`), one flag per real cell.
    pub growth_bitmap: BTreeSet<(u16, u16)>,
    /// Spread flag-byte plane (`+0xF8`).
    pub spread_bitmap: BTreeSet<(u16, u16)>,
}

/// One native `TiberiumClass` queue store: the append-only entry array with
/// its monotonically increasing counter, the float min-heap of entry
/// references, and the store capacity.
///
/// gamemd-derived: `TiberiumClass::InitGrowthQueues_All @ 0x00722D00` /
/// `InitSpreadQueues_All @ 0x00722240` size the entry array, the flag plane,
/// and the heap from `FUN_0042B1F0 = (MapRect.Height + 4) * MapRect.Width *
/// 2`. Every insert site (`RebuildGrowthQueue @ 0x007233A0`,
/// `RebuildSpreadQueue @ 0x007228B0`, `AddToGrowthQueue @ 0x007235A0`,
/// `AddToSpreadQueue @ 0x00722AF0`, the processor reinserts at
/// `0x00723060..0x007230D8` and `0x0072259A..0x00722614`) appends the entry
/// at the array counter unconditionally, increments the counter, and only
/// when `count + 1 < capacity` sifts the new reference up while the parent's
/// priority is strictly greater (`parent <= new` breaks the loop). The
/// processors pop slot 1, move the last slot to the root, decrement the
/// count, and run `FloatMinHeap::SiftDown @ 0x005AD870` (a child replaces
/// its parent only when strictly smaller, left before right). The array
/// contents past the popped references are never reused before a rebuild.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeTiberiumQueue {
    /// Entry array in insertion order (the native counter is its length).
    entries: Vec<NativeTiberiumQueueEntry>,
    /// 1-based heap of entry-array indices; slot 0 is unused.
    heap: Vec<u32>,
    /// Native store capacity.
    capacity: u32,
}

impl Default for NativeTiberiumQueue {
    /// An unsized store (capacity 0): every insert stays array-only until a
    /// rebuild sizes it. Slot 0 is present so the 1-based heap invariant
    /// holds from construction.
    fn default() -> Self {
        Self::with_capacity(0)
    }
}

/// Where one native insert landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeQueueInsert {
    /// Appended to the array and sifted into the heap.
    Heaped,
    /// Appended to the array only: the heap already holds `capacity - 1`
    /// references, so the native insert skips the heap.
    ArrayOnly,
}

impl NativeTiberiumQueue {
    pub fn with_capacity(capacity: u32) -> Self {
        Self {
            entries: Vec::new(),
            heap: vec![0],
            capacity,
        }
    }

    /// Heap count (`+0x110` / `+0xF4` first dword).
    pub fn len(&self) -> usize {
        self.heap.len().saturating_sub(1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Entry-array counter (`+0x10C` / `+0xF0`).
    pub fn array_len(&self) -> usize {
        self.entries.len()
    }

    pub fn capacity(&self) -> u32 {
        self.capacity
    }

    /// Entries referenced by the heap, in heap slot order (slot 1 first).
    pub fn iter_heap(&self) -> impl Iterator<Item = &NativeTiberiumQueueEntry> + '_ {
        self.heap
            .iter()
            .skip(1)
            .map(move |&index| &self.entries[index as usize])
    }

    /// The entry at heap slot `slot + 1` (slot 0 is the root).
    pub fn heap_entry(&self, slot: usize) -> Option<&NativeTiberiumQueueEntry> {
        self.heap
            .get(slot + 1)
            .map(|&index| &self.entries[index as usize])
    }

    /// The root entry without popping it.
    pub fn peek_root(&self) -> Option<&NativeTiberiumQueueEntry> {
        self.heap_entry(0)
    }

    /// Native insert: append, then heap-insert while capacity allows.
    pub fn push(&mut self, entry: NativeTiberiumQueueEntry) -> NativeQueueInsert {
        let index = u32::try_from(self.entries.len()).expect("queue array fits u32");
        self.entries.push(entry);
        let slot = self.len() + 1;
        if u32::try_from(slot).map_or(true, |slot| slot >= self.capacity) {
            return NativeQueueInsert::ArrayOnly;
        }
        self.heap.push(index);
        let priority = priority_f32(&entry);
        let mut hole = slot;
        while hole > 1 {
            let parent_index = self.heap[hole >> 1];
            if !priority_greater(self.entry_priority(parent_index), priority) {
                break;
            }
            self.heap[hole] = parent_index;
            hole >>= 1;
        }
        self.heap[hole] = index;
        NativeQueueInsert::Heaped
    }

    /// Native processor pop: take slot 1, move the last slot to the root, and
    /// sift it down.
    pub fn pop_root(&mut self) -> Option<NativeTiberiumQueueEntry> {
        if self.is_empty() {
            return None;
        }
        let root = self.heap[1];
        let last = self.heap.pop().expect("heap holds the root");
        if !self.is_empty() {
            self.heap[1] = last;
            self.sift_down(1);
        }
        Some(self.entries[root as usize])
    }

    /// `FloatMinHeap::SiftDown @ 0x005AD870`.
    fn sift_down(&mut self, mut slot: usize) {
        let count = self.len();
        loop {
            let mut smallest = slot;
            let left = slot * 2;
            let right = left + 1;
            if left <= count && priority_less(self.slot_priority(left), self.slot_priority(slot)) {
                smallest = left;
            }
            if right <= count
                && priority_less(self.slot_priority(right), self.slot_priority(smallest))
            {
                smallest = right;
            }
            if smallest == slot {
                return;
            }
            self.heap.swap(slot, smallest);
            slot = smallest;
        }
    }

    fn slot_priority(&self, slot: usize) -> f32 {
        self.entry_priority(self.heap[slot])
    }

    fn entry_priority(&self, index: u32) -> f32 {
        priority_f32(&self.entries[index as usize])
    }

    fn hash_into(&self, hasher: &mut impl Hasher) {
        self.capacity.hash(hasher);
        self.entries.len().hash(hasher);
        for entry in &self.entries {
            entry.rx.hash(hasher);
            entry.ry.hash(hasher);
            entry.priority_bits.hash(hasher);
        }
        self.heap.hash(hasher);
    }
}

/// Native `FCOMP` ordering of two finite float priorities.
fn priority_less(lhs: f32, rhs: f32) -> bool {
    lhs.partial_cmp(&rhs) == Some(std::cmp::Ordering::Less)
}

fn priority_greater(lhs: f32, rhs: f32) -> bool {
    lhs.partial_cmp(&rhs) == Some(std::cmp::Ordering::Greater)
}

/// `FUN_0042B1F0`: `(MapRect.Height + 4) * MapRect.Width * 2`.
pub fn native_tiberium_queue_capacity(native_rect: (u16, u16)) -> u32 {
    (u32::from(native_rect.1) + 4) * u32::from(native_rect.0) * 2
}

/// `pct >= 1e-05` in native double arithmetic: the admission gate of
/// `CanGrowTiberium`/`CanSpreadTiberium` (reject when `pct < 1e-05`).
pub(crate) fn native_percentage_admits(bits: u64) -> bool {
    let Ok(value) = X87Chop53::load_f64(NativeF64Bits::from_bits(bits)) else {
        return false;
    };
    let Ok(min) = X87Chop53::load_f64(NativeF64Bits::from_bits(NATIVE_PERCENT_MIN_BITS)) else {
        return false;
    };
    X87Chop53::compare(value, min) != X87Ordering::Less
}

/// `pct > 1e-05`: the processor entry gate (`FCOMP` then `TEST AH,0x41`
/// returns on below-or-equal).
fn native_percentage_drives(bits: u64) -> bool {
    let Ok(value) = X87Chop53::load_f64(NativeF64Bits::from_bits(bits)) else {
        return false;
    };
    let Ok(min) = X87Chop53::load_f64(NativeF64Bits::from_bits(NATIVE_PERCENT_MIN_BITS)) else {
        return false;
    };
    X87Chop53::compare(value, min) == X87Ordering::Greater
}

/// Processor batch: `FILD heap_count; FMUL [pct]; call _ftol` under the
/// process's 53-bit chop control word, then the clamp `[min, max]`
/// (`0x00722F3C..0x00722F68` growth, `0x00722480..0x007224AC` spread).
fn native_processor_batch(heap_count: usize, percentage_bits: u64, min: u32, max: u32) -> u32 {
    let count = i32::try_from(heap_count).unwrap_or(i32::MAX);
    let product = X87Chop53::load_f64(NativeF64Bits::from_bits(percentage_bits))
        .map(|percentage| X87Chop53::mul(X87Chop53::load_i32(count), percentage))
        .ok();
    let scaled = product
        .and_then(|product| X87Chop53::ftol_i64(product).ok())
        .unwrap_or(0);
    scaled.clamp(i64::from(min), i64::from(max)) as u32
}

/// CDTimer-shaped fields used by native tiberium drivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeTiberiumTimer {
    pub start_frame: u32,
    pub interval: u32,
}

/// Heap entry shell for native growth/spread queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeTiberiumQueueEntry {
    pub rx: u16,
    pub ry: u16,
    /// Raw IEEE bits for GameMD's float priority.
    pub priority_bits: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeTiberiumRebuildStats {
    pub growth_entries: usize,
    pub spread_entries: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeGrowthProcessStats {
    pub processor_calls: u32,
    pub attempt_rng_draws: u32,
    pub requested_attempts: u32,
    pub popped_entries: u32,
    pub stale_entries: u32,
    pub grown_entries: u32,
    pub reinserted_entries: u32,
    pub full_clears: u32,
    pub spread_feed_calls: u32,
    pub spread_enqueued_entries: u32,
}

impl NativeGrowthProcessStats {
    fn add(&mut self, other: Self) {
        self.processor_calls += other.processor_calls;
        self.attempt_rng_draws += other.attempt_rng_draws;
        self.requested_attempts += other.requested_attempts;
        self.popped_entries += other.popped_entries;
        self.stale_entries += other.stale_entries;
        self.grown_entries += other.grown_entries;
        self.reinserted_entries += other.reinserted_entries;
        self.full_clears += other.full_clears;
        self.spread_feed_calls += other.spread_feed_calls;
        self.spread_enqueued_entries += other.spread_enqueued_entries;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeSpreadProcessStats {
    pub processor_calls: u32,
    pub budget_rng_draws: u32,
    pub requested_budget: u32,
    pub popped_entries: u32,
    pub zero_target_entries: u32,
    pub spread_calls: u32,
    pub placed_entries: u32,
    pub reinserted_entries: u32,
    pub bitmap_clears: u32,
}

impl NativeSpreadProcessStats {
    fn add(&mut self, other: Self) {
        self.processor_calls += other.processor_calls;
        self.budget_rng_draws += other.budget_rng_draws;
        self.requested_budget += other.requested_budget;
        self.popped_entries += other.popped_entries;
        self.zero_target_entries += other.zero_target_entries;
        self.spread_calls += other.spread_calls;
        self.placed_entries += other.placed_entries;
        self.reinserted_entries += other.reinserted_entries;
        self.bitmap_clears += other.bitmap_clears;
    }
}

impl NativeTiberiumClassState {
    pub fn new_due(current_frame: u32, queue_capacity: u32) -> Self {
        Self {
            growth_timer: NativeTiberiumTimer::due(current_frame),
            spread_timer: NativeTiberiumTimer::due(current_frame),
            growth: NativeTiberiumQueue::with_capacity(queue_capacity),
            spread: NativeTiberiumQueue::with_capacity(queue_capacity),
            growth_bitmap: BTreeSet::new(),
            spread_bitmap: BTreeSet::new(),
        }
    }
}

impl NativeTiberiumTimer {
    pub fn due(current_frame: u32) -> Self {
        Self {
            start_frame: current_frame,
            interval: 0,
        }
    }
}

/// Persistent state for the incremental map scanner.
///
/// Lives in ProductionState. The scanner processes a fraction of the map each
/// tick and collects candidates via reservoir sampling (fair random selection
/// from a stream of unknown length, bounded to MAX_CANDIDATES).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OreGrowthState {
    /// Current position in the cell iteration (wraps to 0 after full scan).
    scan_cursor: usize,
    /// Total number of cells to scan (map_width * map_height).
    total_cells: usize,
    /// Map dimensions for cell coordinate conversion.
    map_width: u16,
    /// Map height for native neighbor bounds checks.
    #[serde(default)]
    map_height: u16,
    /// Cells eligible for growth this scan cycle.
    growth_candidates: Vec<(u16, u16)>,
    /// Cells eligible for spread this scan cycle.
    spread_candidates: Vec<(u16, u16)>,
    /// Reservoir sampling counter for growth (total candidates seen).
    growth_seen: usize,
    /// Reservoir sampling counter for spread (total candidates seen).
    spread_seen: usize,
    /// Native AddToGrowthQueue-style entries inserted by explicit placement paths.
    #[serde(default)]
    growth_queue: Vec<OreGrowthQueueEntry>,
    /// Native AddToSpreadQueue-style entries inserted by explicit cell events.
    #[serde(default)]
    spread_queue: Vec<OreSpreadQueueEntry>,
    /// Deterministic membership guard for `spread_queue`.
    #[serde(default)]
    spread_membership: BTreeSet<(ResourceType, u16, u16)>,
    /// Native per-`TiberiumClass` state shell for the YR queue model.
    #[serde(default)]
    native_tiberium: NativeTiberiumState,
    /// The native `MapRect` (`[Map] Size` width/height, `0x0087F8DC` /
    /// `0x0087F8E0`) that sizes every queue store and orders every rebuild
    /// walk. The storage dimensions stand in until a rebuild supplies it.
    #[serde(default)]
    native_rect: (u16, u16),
}

impl OreGrowthState {
    /// Create a new scanner for a map of the given dimensions.
    pub fn new(map_width: u16, map_height: u16) -> Self {
        Self {
            scan_cursor: 0,
            total_cells: map_width as usize * map_height as usize,
            map_width,
            map_height,
            growth_candidates: Vec::with_capacity(MAX_CANDIDATES),
            spread_candidates: Vec::with_capacity(MAX_CANDIDATES),
            growth_seen: 0,
            spread_seen: 0,
            growth_queue: Vec::new(),
            spread_queue: Vec::new(),
            spread_membership: BTreeSet::new(),
            native_tiberium: NativeTiberiumState::default(),
            native_rect: (map_width, map_height),
        }
    }

    /// The native `MapRect` the queue stores are sized and walked from.
    pub fn native_rect(&self) -> (u16, u16) {
        self.native_rect
    }

    /// Mimic a snapshot written before the rect was serialized, whose
    /// `native_rect` deserializes to the serde default.
    #[cfg(test)]
    pub(crate) fn set_native_rect_for_tests(&mut self, native_rect: (u16, u16)) {
        self.native_rect = native_rect;
    }

    /// Store the native `MapRect` the caller loaded with, then allocate the
    /// class stores from it. Used where no overlay grid exists yet, so that a
    /// later snapshot restore still rebuilds from the `[Map] Size` rect.
    pub fn reset_native_tiberium_classes_for_rect(
        &mut self,
        native_rect: (u16, u16),
        type_count: usize,
        current_frame: u32,
    ) {
        self.native_rect = native_rect;
        self.reset_native_tiberium_classes(type_count, current_frame);
    }

    /// Allocate native per-type tiberium state with due timers and stores
    /// sized from the current native rect.
    pub fn reset_native_tiberium_classes(&mut self, type_count: usize, current_frame: u32) {
        let capacity = native_tiberium_queue_capacity(self.native_rect);
        self.native_tiberium.classes = (0..type_count)
            .map(|_| NativeTiberiumClassState::new_due(current_frame, capacity))
            .collect();
    }

    /// Native per-type tiberium queue/timer shell.
    pub fn native_tiberium_state(&self) -> &NativeTiberiumState {
        &self.native_tiberium
    }

    /// Native-shaped `AddToGrowthQueue`: no dedupe, density-gated, one RNG on insert.
    pub fn add_native_growth_queue_cell(
        &mut self,
        overlay_grid: &OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        rx: u16,
        ry: u16,
        native_frame: u32,
        rng: &mut SimRng,
    ) -> Option<NativeTiberiumQueueEntry> {
        let cell = overlay_grid.cell(rx, ry);
        let overlay_id = cell.overlay_id?;
        let type_id = overlay_registry.tiberium_type_for_overlay(tiberium_types, overlay_id)?;
        // `AddToGrowthQueue @ 0x007235A0`: the literal `OverlayData < 0x0B`
        // gate; its array-counter rebuild trigger (`counter > capacity - 10`)
        // is recorded DRIFT (unreachable in ordinary play, see OQ-38).
        if cell.overlay_data >= GROWTH_QUEUE_DENSITY_LIMIT {
            return None;
        }
        let class = self.native_tiberium.classes.get_mut(type_id.0 as usize)?;
        let entry = NativeTiberiumQueueEntry {
            rx,
            ry,
            priority_bits: growth_queue_priority(native_frame, rng.next_u32()).to_bits(),
        };
        class.growth.push(entry);
        class.growth_bitmap.insert((rx, ry));
        Some(entry)
    }

    /// Native-shaped `AddToSpreadQueue @ 0x00722AF0`: `CanSpreadTiberium`
    /// source gate (`source_has_object` is the cell's `FirstObject != 0`
    /// test), bitmap-deduped, one RNG on insert. Its array-counter rebuild
    /// trigger (`counter >= capacity - 0x14`) is recorded DRIFT (OQ-38).
    #[allow(clippy::too_many_arguments)]
    pub fn add_native_spread_queue_cell(
        &mut self,
        overlay_grid: &OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source_has_object: bool,
        rx: u16,
        ry: u16,
        native_frame: u32,
        spread_enabled: bool,
        rng: &mut SimRng,
    ) -> Option<NativeTiberiumQueueEntry> {
        let type_id =
            current_tiberium_type(overlay_grid, overlay_registry, tiberium_types, rx, ry)?;
        self.add_native_spread_queue_cell_for_type(
            type_id,
            overlay_grid,
            overlay_registry,
            tiberium_types,
            resolved_terrain,
            source_has_object,
            rx,
            ry,
            native_frame,
            spread_enabled,
            rng,
        )
    }

    /// `AddToSpreadQueue @ 0x00722AF0` with an explicit receiver (`this`):
    /// `store_type` names the store and flag plane that take the cell, while
    /// `CanSpreadTiberium @ 0x00483690` admits the cell on its OWN class.
    /// `Reduce_Tiberium @ 0x00480A80` calls the REMOVED class's
    /// `AddToSpreadQueue` on every in-bounds neighbour, so a store may hold a
    /// cell of another class (a harvested gem queues its ore neighbours).
    #[allow(clippy::too_many_arguments)]
    fn add_native_spread_queue_cell_for_type(
        &mut self,
        store_type: TiberiumTypeId,
        overlay_grid: &OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source_has_object: bool,
        rx: u16,
        ry: u16,
        native_frame: u32,
        spread_enabled: bool,
        rng: &mut SimRng,
    ) -> Option<NativeTiberiumQueueEntry> {
        native_can_spread_tiberium(
            overlay_grid,
            overlay_registry,
            tiberium_types,
            resolved_terrain,
            source_has_object,
            rx,
            ry,
            spread_enabled,
        )?;
        let class = self
            .native_tiberium
            .classes
            .get_mut(store_type.0 as usize)?;
        if class.spread_bitmap.contains(&(rx, ry)) {
            return None;
        }
        let entry = NativeTiberiumQueueEntry {
            rx,
            ry,
            priority_bits: growth_queue_priority(native_frame, rng.next_u32()).to_bits(),
        };
        class.spread.push(entry);
        class.spread_bitmap.insert((rx, ry));
        Some(entry)
    }

    /// Process all due native growth queues through the shared cell mutation boundary.
    #[allow(clippy::too_many_arguments)]
    pub fn tick_native_growth_driver(
        &mut self,
        overlay_grid: &mut OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source_object_cells: &BTreeSet<(u16, u16)>,
        live_objects: Option<TiberiumPlacementObjectContext<'_>>,
        resource_nodes: &mut BTreeMap<(u16, u16), ResourceNode>,
        rng: &mut SimRng,
        current_frame: u32,
        growth_enabled: bool,
        spread_enabled: bool,
        tiberium_grows_flag: bool,
        mut radar_dirty_cells: Option<&mut Vec<(u16, u16)>>,
        mut radar_dirty_generation: Option<&mut u64>,
        mut tactical_dirty_cells: Option<&mut Vec<(u16, u16)>>,
    ) -> NativeGrowthProcessStats {
        // `0x00722C48`: `ScenarioClass+0x34A6` gates the whole driver.
        if !growth_enabled {
            return NativeGrowthProcessStats::default();
        }
        let due_ids: Vec<TiberiumTypeId> = self
            .native_tiberium
            .classes
            .iter()
            .enumerate()
            .filter_map(|(idx, class)| {
                native_timer_due(class.growth_timer, current_frame)
                    .then(|| u8::try_from(idx).ok().map(TiberiumTypeId))
                    .flatten()
            })
            .collect();
        let mut stats = NativeGrowthProcessStats::default();
        for type_id in due_ids {
            stats.add(self.process_native_growth_for_type_with_placement(
                type_id,
                overlay_grid,
                overlay_registry,
                tiberium_types,
                resolved_terrain,
                source_object_cells,
                live_objects,
                resource_nodes,
                rng,
                current_frame,
                growth_enabled,
                spread_enabled,
                radar_dirty_cells.as_deref_mut(),
                radar_dirty_generation.as_deref_mut(),
                tactical_dirty_cells.as_deref_mut(),
            ));
            if let (Some(class), Some(ty)) = (
                self.native_tiberium.classes.get_mut(type_id.0 as usize),
                tiberium_types.get(type_id),
            ) {
                // `0x00722C9E..0x00722CE3`: see `native_growth_timer_reload`.
                class.growth_timer = NativeTiberiumTimer {
                    start_frame: current_frame,
                    interval: native_growth_timer_reload(ty.growth, tiberium_grows_flag),
                };
            }
        }
        stats
    }

    /// Native `GrowthProcessor` for one tiberium type.
    pub fn process_native_growth_for_type(
        &mut self,
        type_id: TiberiumTypeId,
        overlay_grid: &mut OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source_object_cells: &BTreeSet<(u16, u16)>,
        _resource_nodes: &mut BTreeMap<(u16, u16), ResourceNode>,
        rng: &mut SimRng,
        current_frame: u32,
        spread_enabled: bool,
    ) -> NativeGrowthProcessStats {
        self.process_native_growth_for_type_with_placement(
            type_id,
            overlay_grid,
            overlay_registry,
            tiberium_types,
            resolved_terrain,
            source_object_cells,
            None,
            _resource_nodes,
            rng,
            current_frame,
            true,
            spread_enabled,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn process_native_growth_for_type_with_placement(
        &mut self,
        type_id: TiberiumTypeId,
        overlay_grid: &mut OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source_object_cells: &BTreeSet<(u16, u16)>,
        live_objects: Option<TiberiumPlacementObjectContext<'_>>,
        _resource_nodes: &mut BTreeMap<(u16, u16), ResourceNode>,
        rng: &mut SimRng,
        current_frame: u32,
        growth_enabled: bool,
        spread_enabled: bool,
        mut radar_dirty_cells: Option<&mut Vec<(u16, u16)>>,
        mut radar_dirty_generation: Option<&mut u64>,
        mut tactical_dirty_cells: Option<&mut Vec<(u16, u16)>>,
    ) -> NativeGrowthProcessStats {
        let Some(ty) = tiberium_types.get(type_id) else {
            return NativeGrowthProcessStats::default();
        };
        let class_idx = type_id.0 as usize;
        let Some(class) = self.native_tiberium.classes.get(class_idx) else {
            return NativeGrowthProcessStats::default();
        };
        // `GrowthProcessor @ 0x00722F00` entry gate (`0x00722F09..0x00722F36`):
        // no heap, empty heap, or `GrowthPercentage <= 1e-05` returns.
        if class.growth.is_empty() || !native_percentage_drives(ty.growth_percentage_bits) {
            return NativeGrowthProcessStats::default();
        }

        let batch = native_processor_batch(
            class.growth.len(),
            ty.growth_percentage_bits,
            GROWTH_BATCH_MIN,
            GROWTH_BATCH_MAX,
        );
        let actual_attempts = signed_abs_mod_plus_one(rng.next_u32(), batch);
        let mut stats = NativeGrowthProcessStats {
            processor_calls: 1,
            attempt_rng_draws: 1,
            requested_attempts: actual_attempts.max(0) as u32,
            ..NativeGrowthProcessStats::default()
        };
        // `0x00722F85..0x00722F9C`: `heap count > capacity - 2 * attempts`
        // rebuilds this type's growth queue before the first pop.
        {
            let class = &self.native_tiberium.classes[class_idx];
            let heap_count = class.growth.len() as i64;
            let threshold = i64::from(class.growth.capacity())
                - GROWTH_PROCESSOR_REBUILD_BATCH_FACTOR * i64::from(actual_attempts);
            if threshold < heap_count {
                let cells = native_rebuild_cells(self.native_rect, overlay_grid);
                self.rebuild_growth_queue_for_type(
                    type_id,
                    overlay_grid,
                    overlay_registry,
                    tiberium_types,
                    resolved_terrain,
                    growth_enabled,
                    &cells,
                );
            }
        }

        // `0x00722F9C..0x00722FEA`: the first root is popped before the
        // attempts test, so a non-positive attempts word (`0x80000000` only)
        // drops one entry and returns.
        let mut pending = self.native_tiberium.classes[class_idx].growth.pop_root();
        if pending.is_some() {
            stats.popped_entries += 1;
        }
        if actual_attempts < 1 {
            return stats;
        }
        let mut processed: i32 = 0;
        while let Some(entry) = pending.take() {
            let current_type = current_tiberium_type(
                overlay_grid,
                overlay_registry,
                tiberium_types,
                entry.rx,
                entry.ry,
            );
            if current_type == Some(type_id) {
                let spread_was_queued = self.native_tiberium.classes[class_idx]
                    .spread_bitmap
                    .contains(&(entry.rx, entry.ry));
                let object_view = live_objects.map(|objects| objects.object_view());
                // `CellClass::GrowTiberium @ 0x00483710` (= `PlaceTiberium(type,
                // 1)`, whose existing-cell branch feeds `AddToSpreadQueue`). Its
                // result is not consulted: the processor continues on the
                // cell's data byte.
                let placed = {
                    let mut context = PlaceTiberiumContext {
                        overlay_grid,
                        ore_growth_state: self,
                        overlay_registry,
                        tiberium_types,
                        resolved_terrain,
                        source_object_cells,
                        new_cell_admission: None,
                        live_objects: object_view,
                        rng,
                        binary_frame: current_frame,
                        growth_enabled: true,
                        spread_enabled,
                        radar_dirty_cells: radar_dirty_cells.as_deref_mut(),
                        radar_dirty_generation: radar_dirty_generation.as_deref_mut(),
                        tactical_dirty_cells: tactical_dirty_cells.as_deref_mut(),
                    };
                    place_tiberium(&mut context, (entry.rx, entry.ry), type_id, 1)
                };
                if placed {
                    stats.grown_entries += 1;
                    stats.spread_feed_calls += 1;
                    if !spread_was_queued
                        && self.native_tiberium.classes[class_idx]
                            .spread_bitmap
                            .contains(&(entry.rx, entry.ry))
                    {
                        stats.spread_enqueued_entries += 1;
                    }
                }

                // `0x00723030..0x007230D8`: the literal `< 0x0B` reinsert
                // test, one Scenario draw for the new priority
                // (`abs(raw % 50)`), heap insert, flag set, then
                // `AddToSpreadQueue` (a no-op when the `PlaceTiberium` feed
                // already set the spread flag).
                let post_data = overlay_grid.cell(entry.rx, entry.ry).overlay_data;
                if post_data < GROWTH_QUEUE_DENSITY_LIMIT {
                    let replacement = NativeTiberiumQueueEntry {
                        rx: entry.rx,
                        ry: entry.ry,
                        priority_bits: processor_reinsert_priority(current_frame, rng.next_u32())
                            .to_bits(),
                    };
                    let class = &mut self.native_tiberium.classes[class_idx];
                    class.growth.push(replacement);
                    class.growth_bitmap.insert((entry.rx, entry.ry));
                    stats.reinserted_entries += 1;
                    let source_has_object = cell_has_native_object(
                        source_object_cells,
                        object_view,
                        (entry.rx, entry.ry),
                    );
                    self.add_native_spread_queue_cell(
                        overlay_grid,
                        overlay_registry,
                        tiberium_types,
                        resolved_terrain,
                        source_has_object,
                        entry.rx,
                        entry.ry,
                        current_frame,
                        spread_enabled,
                        rng,
                    );
                } else {
                    self.native_tiberium.classes[class_idx]
                        .growth_bitmap
                        .remove(&(entry.rx, entry.ry));
                    stats.full_clears += 1;
                }
            } else {
                // A type mismatch drops the entry without touching the flag.
                stats.stale_entries += 1;
            }
            // `0x0072312B..0x0072313E`: every popped entry counts as one
            // attempt; the next root is popped only while attempts remain.
            processed += 1;
            if actual_attempts <= processed {
                break;
            }
            pending = self.native_tiberium.classes[class_idx].growth.pop_root();
            if pending.is_some() {
                stats.popped_entries += 1;
            }
        }

        stats
    }

    /// Process all due native spread queues.
    #[allow(clippy::too_many_arguments)]
    pub fn tick_native_spread_driver(
        &mut self,
        overlay_grid: &mut OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        resource_nodes: &mut BTreeMap<(u16, u16), ResourceNode>,
        path_grid: Option<&PathGrid>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source_object_cells: &BTreeSet<(u16, u16)>,
        live_objects: Option<TiberiumPlacementObjectContext<'_>>,
        rng: &mut SimRng,
        current_frame: u32,
        growth_enabled: bool,
        spread_enabled: bool,
        mut radar_dirty_cells: Option<&mut Vec<(u16, u16)>>,
        mut radar_dirty_generation: Option<&mut u64>,
        mut tactical_dirty_cells: Option<&mut Vec<(u16, u16)>>,
    ) -> NativeSpreadProcessStats {
        if !growth_enabled || !spread_enabled {
            return NativeSpreadProcessStats::default();
        }
        let due_ids: Vec<TiberiumTypeId> = self
            .native_tiberium
            .classes
            .iter()
            .enumerate()
            .filter_map(|(idx, class)| {
                native_timer_due(class.spread_timer, current_frame)
                    .then(|| u8::try_from(idx).ok().map(TiberiumTypeId))
                    .flatten()
            })
            .collect();
        let mut stats = NativeSpreadProcessStats::default();
        let new_cell_admission = resolved_terrain
            .zip(live_objects)
            .map(|(terrain, objects)| NewTiberiumAdmission::runtime(terrain, path_grid, objects));
        for type_id in due_ids {
            stats.add(self.process_native_spread_for_type_with_placement(
                type_id,
                overlay_grid,
                overlay_registry,
                tiberium_types,
                resource_nodes,
                resolved_terrain,
                source_object_cells,
                new_cell_admission,
                rng,
                current_frame,
                spread_enabled,
                radar_dirty_cells.as_deref_mut(),
                radar_dirty_generation.as_deref_mut(),
                tactical_dirty_cells.as_deref_mut(),
            ));
            if let (Some(class), Some(ty)) = (
                self.native_tiberium.classes.get_mut(type_id.0 as usize),
                tiberium_types.get(type_id),
            ) {
                // `SpreadDriver_AllTypes @ 0x00722205..0x00722227`: the raw
                // `Spread=` int (`TiberiumClass+0x9C`) reloads the timer; no
                // multiplier and no flag test on this driver.
                class.spread_timer = NativeTiberiumTimer {
                    start_frame: current_frame,
                    interval: ty.spread,
                };
            }
        }
        stats
    }

    /// Compatibility-only processor for fixtures without a live map context.
    #[cfg(test)]
    pub fn process_native_spread_for_type_without_native_context(
        &mut self,
        type_id: TiberiumTypeId,
        overlay_grid: &mut OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        _resource_nodes: &mut BTreeMap<(u16, u16), ResourceNode>,
        path_grid: Option<&PathGrid>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source_object_cells: &BTreeSet<(u16, u16)>,
        rng: &mut SimRng,
        current_frame: u32,
        spread_enabled: bool,
    ) -> NativeSpreadProcessStats {
        let new_cell_admission = Some(NewTiberiumAdmission::compatibility_without_native_context(
            resolved_terrain,
            path_grid,
            None,
        ));
        self.process_native_spread_for_type_with_placement(
            type_id,
            overlay_grid,
            overlay_registry,
            tiberium_types,
            _resource_nodes,
            resolved_terrain,
            source_object_cells,
            new_cell_admission,
            rng,
            current_frame,
            spread_enabled,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn process_native_spread_for_type_with_placement(
        &mut self,
        type_id: TiberiumTypeId,
        overlay_grid: &mut OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        _resource_nodes: &mut BTreeMap<(u16, u16), ResourceNode>,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source_object_cells: &BTreeSet<(u16, u16)>,
        new_cell_admission: Option<NewTiberiumAdmission<'_>>,
        rng: &mut SimRng,
        current_frame: u32,
        spread_enabled: bool,
        mut radar_dirty_cells: Option<&mut Vec<(u16, u16)>>,
        mut radar_dirty_generation: Option<&mut u64>,
        mut tactical_dirty_cells: Option<&mut Vec<(u16, u16)>>,
    ) -> NativeSpreadProcessStats {
        let Some(ty) = tiberium_types.get(type_id) else {
            return NativeSpreadProcessStats::default();
        };
        let class_idx = type_id.0 as usize;
        if self.native_tiberium.classes.get(class_idx).is_none() {
            return NativeSpreadProcessStats::default();
        };
        // `SpreadProcessor @ 0x00722440` entry gate: empty heap or
        // `SpreadPercentage <= 1e-05` returns.
        if self.native_tiberium.classes[class_idx].spread.is_empty()
            || !native_percentage_drives(ty.spread_percentage_bits)
        {
            return NativeSpreadProcessStats::default();
        }

        let batch = native_processor_batch(
            self.native_tiberium.classes[class_idx].spread.len(),
            ty.spread_percentage_bits,
            SPREAD_BATCH_MIN,
            SPREAD_BATCH_MAX,
        );
        let budget = signed_abs_mod_plus_one(rng.next_u32(), batch);
        let mut stats = NativeSpreadProcessStats {
            processor_calls: 1,
            budget_rng_draws: 1,
            requested_budget: budget.max(0) as u32,
            ..NativeSpreadProcessStats::default()
        };
        // `0x007224C9..0x007224E3`: `heap count > capacity - 0x14` rebuilds
        // this type's spread queue before the first pop.
        {
            let class = &self.native_tiberium.classes[class_idx];
            let heap_count = class.spread.len() as i64;
            if i64::from(class.spread.capacity()) - SPREAD_PROCESSOR_REBUILD_SLACK < heap_count {
                let cells = native_rebuild_cells(self.native_rect, overlay_grid);
                let occupied_cells = native_occupied_cells(
                    source_object_cells,
                    new_cell_admission
                        .and_then(|admission| admission.live_objects())
                        .map(|objects| objects.object_view()),
                );
                self.rebuild_spread_queue_for_type(
                    type_id,
                    overlay_grid,
                    overlay_registry,
                    tiberium_types,
                    resolved_terrain,
                    &occupied_cells,
                    spread_enabled,
                    &cells,
                );
            }
        }
        // `0x007224E8..0x0072252E`: the first root is popped before the budget
        // test (a non-positive budget word drops it and returns).
        let mut pending = self.native_tiberium.classes[class_idx].spread.pop_root();
        if pending.is_some() {
            stats.popped_entries += 1;
        }
        if budget < 1 {
            return stats;
        }
        let mut processed_sources: i32 = 0;
        while let Some(entry) = pending.take() {
            let valid_targets = count_native_spread_targets(
                overlay_grid,
                source_object_cells,
                new_cell_admission,
                entry.rx,
                entry.ry,
                self.map_width,
                self.effective_map_height(),
            );
            if valid_targets == 0 {
                self.native_tiberium.classes[class_idx]
                    .spread_bitmap
                    .remove(&(entry.rx, entry.ry));
                stats.zero_target_entries += 1;
                stats.bitmap_clears += 1;
                // Zero targets spend no budget; the loop pops the next root.
                pending = self.native_tiberium.classes[class_idx].spread.pop_root();
                if pending.is_some() {
                    stats.popped_entries += 1;
                }
                continue;
            }

            stats.spread_calls += 1;
            processed_sources += 1;
            // `CellClass::SpreadTiberium @ 0x00483780` spreads the cell's
            // CURRENT TiberiumClass (a stale entry of another class still
            // spreads whatever now sits there; an empty cell fails its own
            // `CanSpreadTiberium` re-check without a draw).
            if spread_tiberium_from_source(
                overlay_grid,
                overlay_registry,
                tiberium_types,
                resolved_terrain,
                source_object_cells,
                new_cell_admission,
                entry.rx,
                entry.ry,
                self.map_width,
                self.effective_map_height(),
                spread_enabled,
                self,
                rng,
                current_frame,
                radar_dirty_cells.as_deref_mut(),
                radar_dirty_generation.as_deref_mut(),
                tactical_dirty_cells.as_deref_mut(),
            )
            .is_some()
            {
                stats.placed_entries += 1;
            }

            // `0x0072259A..0x00722614`: more than one valid target reinserts
            // the source at priority 0 without an RNG draw.
            if valid_targets > 1 {
                let class = &mut self.native_tiberium.classes[class_idx];
                class.spread.push(NativeTiberiumQueueEntry {
                    rx: entry.rx,
                    ry: entry.ry,
                    priority_bits: 0.0f32.to_bits(),
                });
                class.spread_bitmap.insert((entry.rx, entry.ry));
                stats.reinserted_entries += 1;
            }
            // `0x00722619..0x00722630`: the budget is checked after each
            // processed source; the next root is popped only while it remains.
            if budget <= processed_sources {
                break;
            }
            pending = self.native_tiberium.classes[class_idx].spread.pop_root();
            if pending.is_some() {
                stats.popped_entries += 1;
            }
        }

        stats
    }

    /// Rebuild native growth then spread queues from the current cells.
    ///
    /// gamemd-derived: `TiberiumClass::InitGrowthQueues_All @ 0x00722D00`
    /// then `InitSpreadQueues_All @ 0x00722240` (authored `Full_Init` between
    /// the Terrain and Techno sections, the generator tail before
    /// `InitCellAttributes(1)`, and `Load_Game_From_File`), each freeing and
    /// re-sizing every TiberiumClass's store from the `MapRect` and calling
    /// that type's `RebuildGrowthQueue @ 0x007233A0` / `RebuildSpreadQueue @
    /// 0x007228B0`, which walk every real cell in `CellIterator` order.
    /// `occupied_cells` is the set of cells whose `CellClass+0xE4 FirstObject`
    /// is non-null (terrain objects and every ground-list Techno).
    #[allow(clippy::too_many_arguments)]
    pub fn rebuild_native_tiberium_queues_from_overlays(
        &mut self,
        overlay_grid: &OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        occupied_cells: &BTreeSet<(u16, u16)>,
        basic_growth_enabled: bool,
        tiberium_spreads_enabled: bool,
        current_frame: u32,
        native_rect: (u16, u16),
    ) -> NativeTiberiumRebuildStats {
        self.native_rect = native_rect;
        self.reset_native_tiberium_classes(tiberium_types.len(), current_frame);
        let cells = native_rebuild_cells(native_rect, overlay_grid);
        let mut stats = NativeTiberiumRebuildStats::default();
        for ty in tiberium_types.types() {
            stats.growth_entries += self.rebuild_growth_queue_for_type(
                ty.id,
                overlay_grid,
                overlay_registry,
                tiberium_types,
                resolved_terrain,
                basic_growth_enabled,
                &cells,
            );
        }
        for ty in tiberium_types.types() {
            stats.spread_entries += self.rebuild_spread_queue_for_type(
                ty.id,
                overlay_grid,
                overlay_registry,
                tiberium_types,
                resolved_terrain,
                occupied_cells,
                tiberium_spreads_enabled,
                &cells,
            );
        }
        stats
    }

    /// `TiberiumClass::RebuildGrowthQueue @ 0x007233A0`: reset this type's
    /// growth store, then push every real cell of this type that
    /// `CellClass::CanGrowTiberium @ 0x00483620` admits, priority 0, in
    /// `CellIterator` order.
    #[allow(clippy::too_many_arguments)]
    fn rebuild_growth_queue_for_type(
        &mut self,
        type_id: TiberiumTypeId,
        overlay_grid: &OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        growth_enabled: bool,
        cells: &[(u16, u16)],
    ) -> usize {
        let capacity = native_tiberium_queue_capacity(self.native_rect);
        let Some(ty) = tiberium_types.get(type_id) else {
            return 0;
        };
        let Some(class) = self.native_tiberium.classes.get_mut(type_id.0 as usize) else {
            return 0;
        };
        class.growth = NativeTiberiumQueue::with_capacity(capacity);
        class.growth_bitmap.clear();
        let mut seeded = 0;
        for &(rx, ry) in cells {
            if current_tiberium_type(overlay_grid, overlay_registry, tiberium_types, rx, ry)
                != Some(type_id)
                || !native_can_grow_tiberium(
                    ty,
                    overlay_grid.cell(rx, ry).overlay_data,
                    resolved_terrain,
                    (rx, ry),
                    growth_enabled,
                )
            {
                continue;
            }
            class.growth.push(NativeTiberiumQueueEntry {
                rx,
                ry,
                priority_bits: 0.0f32.to_bits(),
            });
            class.growth_bitmap.insert((rx, ry));
            seeded += 1;
        }
        seeded
    }

    /// `TiberiumClass::RebuildSpreadQueue @ 0x007228B0`: the spread twin,
    /// admitting through `CellClass::CanSpreadTiberium @ 0x00483690`.
    #[allow(clippy::too_many_arguments)]
    fn rebuild_spread_queue_for_type(
        &mut self,
        type_id: TiberiumTypeId,
        overlay_grid: &OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        occupied_cells: &BTreeSet<(u16, u16)>,
        spread_enabled: bool,
        cells: &[(u16, u16)],
    ) -> usize {
        let capacity = native_tiberium_queue_capacity(self.native_rect);
        if tiberium_types.get(type_id).is_none() {
            return 0;
        }
        if self
            .native_tiberium
            .classes
            .get(type_id.0 as usize)
            .is_none()
        {
            return 0;
        }
        {
            let class = &mut self.native_tiberium.classes[type_id.0 as usize];
            class.spread = NativeTiberiumQueue::with_capacity(capacity);
            class.spread_bitmap.clear();
        }
        let mut seeded = 0;
        for &(rx, ry) in cells {
            // `RebuildSpreadQueue` alone tests `GetTiberiumType == class`
            // before `CanSpreadTiberium`.
            if current_tiberium_type(overlay_grid, overlay_registry, tiberium_types, rx, ry)
                != Some(type_id)
            {
                continue;
            }
            if native_can_spread_tiberium(
                overlay_grid,
                overlay_registry,
                tiberium_types,
                resolved_terrain,
                occupied_cells.contains(&(rx, ry)),
                rx,
                ry,
                spread_enabled,
            )
            .is_none()
            {
                continue;
            }
            let class = &mut self.native_tiberium.classes[type_id.0 as usize];
            class.spread.push(NativeTiberiumQueueEntry {
                rx,
                ry,
                priority_bits: 0.0f32.to_bits(),
            });
            class.spread_bitmap.insert((rx, ry));
            seeded += 1;
        }
        seeded
    }

    /// Enqueue a newly placed ore cell with native AddToGrowthQueue priority.
    ///
    /// Verified TIBTRE placement consumes one raw Random::Next word and stores
    /// priority as `currentFrame + (signed_abs(raw) % 50)`.
    pub fn enqueue_growth_queue_cell(
        &mut self,
        rx: u16,
        ry: u16,
        native_frame: u32,
        rng: &mut SimRng,
    ) -> OreGrowthQueueEntry {
        let priority = growth_queue_priority(native_frame, rng.next_u32());
        let entry = OreGrowthQueueEntry { rx, ry, priority };
        self.growth_queue.push(entry);
        entry
    }

    /// Native-style growth queue entries waiting for an explicit processor.
    pub fn growth_queue_entries(&self) -> &[OreGrowthQueueEntry] {
        &self.growth_queue
    }

    /// Native-style spread queue entries waiting for a future queue processor.
    pub fn spread_queue_entries(&self) -> &[OreSpreadQueueEntry] {
        &self.spread_queue
    }

    /// Clear all spread memberships for a removed cell across tiberium types.
    pub fn clear_spread_memberships_for_cell(&mut self, rx: u16, ry: u16) {
        self.spread_membership
            .retain(|&(_, cell_rx, cell_ry)| cell_rx != rx || cell_ry != ry);
        self.spread_queue
            .retain(|entry| entry.rx != rx || entry.ry != ry);
    }

    /// Native `ClearSpreadBitmaps_AllTypes` for one removed cell. Heap entries
    /// intentionally remain stale and are rejected when popped.
    pub fn clear_native_spread_bitmap_cell(&mut self, rx: u16, ry: u16) {
        for class in &mut self.native_tiberium.classes {
            class.spread_bitmap.remove(&(rx, ry));
        }
    }

    /// Add one cell to the per-type spread queue if it is not already queued.
    pub fn enqueue_spread_queue_cell(
        &mut self,
        resource_type: ResourceType,
        rx: u16,
        ry: u16,
    ) -> bool {
        if !self.spread_membership.insert((resource_type, rx, ry)) {
            return false;
        }
        self.spread_queue.push(OreSpreadQueueEntry {
            resource_type,
            rx,
            ry,
        });
        true
    }

    /// Reseed same-type resource neighbors around a just-depleted cell.
    pub fn reseed_spread_neighbors_after_reduction(
        &mut self,
        resource_type: ResourceType,
        cell: (u16, u16),
        resource_nodes: &BTreeMap<(u16, u16), ResourceNode>,
    ) {
        self.clear_spread_memberships_for_cell(cell.0, cell.1);
        let map_height = self.effective_map_height();
        for &(dx, dy) in &ADJACENT_OFFSETS {
            let nx = cell.0 as i32 + dx;
            let ny = cell.1 as i32 + dy;
            if nx < 0 || ny < 0 || nx >= self.map_width as i32 || ny >= map_height as i32 {
                continue;
            }
            let neighbor = (nx as u16, ny as u16);
            let Some(node) = resource_nodes.get(&neighbor) else {
                continue;
            };
            if node.resource_type == resource_type && node.remaining > 0 {
                self.enqueue_spread_queue_cell(resource_type, neighbor.0, neighbor.1);
            }
        }
    }

    /// Native `Reduce_Tiberium @ 0x00480A80` full-removal spread reseed.
    ///
    /// Clears this removed cell's spread bitmap bit for every tiberium class
    /// (`ClearSpreadBitmaps_AllTypes @ 0x00722AB0`), then calls the REMOVED
    /// class's `AddToSpreadQueue` for each in-bounds neighbour, which admits
    /// the neighbour on its own class. Existing heap entries are
    /// intentionally left stale.
    #[allow(clippy::too_many_arguments)]
    pub fn reseed_native_spread_neighbors_after_reduction(
        &mut self,
        removed_type: TiberiumTypeId,
        overlay_grid: &OverlayGrid,
        overlay_registry: &OverlayTypeRegistry,
        tiberium_types: &TiberiumTypeRegistry,
        resolved_terrain: Option<&ResolvedTerrainGrid>,
        source_object_cells: &BTreeSet<(u16, u16)>,
        live_objects: Option<NativeCellObjectView<'_>>,
        removed_cell: (u16, u16),
        native_frame: u32,
        spread_enabled: bool,
        rng: &mut SimRng,
    ) -> usize {
        self.clear_spread_memberships_for_cell(removed_cell.0, removed_cell.1);
        self.clear_native_spread_bitmap_cell(removed_cell.0, removed_cell.1);

        let map_height = self.effective_map_height();
        // `Reduce_Tiberium @ 0x00480A80` offers each direction to
        // `Cell_in_bounds_check @ 0x00568300`, the MapRect diamond, not a
        // rectangle. The storage bounds stay because Rust indexes a
        // rectangular grid; a zero rect means no rebuild has supplied the
        // MapRect yet (headless fixtures only), where the storage extent is
        // the only bound available.
        let shape = (self.native_rect != (0, 0)).then(|| {
            NativeOverlayMapShape::new(i32::from(self.native_rect.0), i32::from(self.native_rect.1))
        });
        let mut inserted = 0usize;
        for &(dx, dy) in &ADJACENT_OFFSETS {
            let nx = removed_cell.0 as i32 + dx;
            let ny = removed_cell.1 as i32 + dy;
            if nx < 0 || ny < 0 || nx >= self.map_width as i32 || ny >= map_height as i32 {
                continue;
            }
            if let (Some(shape), Ok(sx), Ok(sy)) = (shape, i16::try_from(nx), i16::try_from(ny))
                && !shape.admits(sx, sy)
            {
                continue;
            }
            let neighbor = (nx as u16, ny as u16);
            if self
                .add_native_spread_queue_cell_for_type(
                    removed_type,
                    overlay_grid,
                    overlay_registry,
                    tiberium_types,
                    resolved_terrain,
                    cell_has_native_object(source_object_cells, live_objects, neighbor),
                    neighbor.0,
                    neighbor.1,
                    native_frame,
                    spread_enabled,
                    rng,
                )
                .is_some()
            {
                inserted += 1;
            }
        }
        inserted
    }

    fn effective_map_height(&self) -> u16 {
        if self.map_height != 0 || self.map_width == 0 {
            return self.map_height;
        }
        (self.total_cells / self.map_width as usize) as u16
    }

    /// Hash persistent ore-growth scheduler state for replay/desync checks.
    pub fn hash_state(&self, hasher: &mut impl Hasher) {
        self.scan_cursor.hash(hasher);
        self.total_cells.hash(hasher);
        self.map_width.hash(hasher);
        self.effective_map_height().hash(hasher);
        self.growth_candidates.hash(hasher);
        self.spread_candidates.hash(hasher);
        self.growth_seen.hash(hasher);
        self.spread_seen.hash(hasher);
        for entry in &self.growth_queue {
            entry.rx.hash(hasher);
            entry.ry.hash(hasher);
            entry.priority.to_bits().hash(hasher);
        }
        for entry in &self.spread_queue {
            entry.resource_type.hash(hasher);
            entry.rx.hash(hasher);
            entry.ry.hash(hasher);
        }
        for &(resource_type, rx, ry) in &self.spread_membership {
            resource_type.hash(hasher);
            rx.hash(hasher);
            ry.hash(hasher);
        }
        self.native_rect.hash(hasher);
        self.native_tiberium.classes.len().hash(hasher);
        for class in &self.native_tiberium.classes {
            class.growth_timer.start_frame.hash(hasher);
            class.growth_timer.interval.hash(hasher);
            class.spread_timer.start_frame.hash(hasher);
            class.spread_timer.interval.hash(hasher);
            class.growth.hash_into(hasher);
            class.spread.hash_into(hasher);
            class.growth_bitmap.hash(hasher);
            class.spread_bitmap.hash(hasher);
        }
    }
}

/// Real cells of the native `MapRect` in `CellIterator_Init/Next @
/// 0x00578350/0x00578290` order, restricted to the rectangular overlay
/// storage.
fn native_rebuild_cells(native_rect: (u16, u16), overlay_grid: &OverlayGrid) -> Vec<(u16, u16)> {
    NativeOverlayMapShape::new(i32::from(native_rect.0), i32::from(native_rect.1))
        .recalc_cells()
        .into_iter()
        .filter_map(|(x, y)| {
            let (rx, ry) = (u16::try_from(x).ok()?, u16::try_from(y).ok()?);
            (rx < overlay_grid.width() && ry < overlay_grid.height()).then_some((rx, ry))
        })
        .collect()
}

/// `CellClass+0xE4 FirstObject != 0` for one cell: a terrain object or any
/// ground-list Techno stands there.
pub(crate) fn cell_has_native_object(
    source_object_cells: &BTreeSet<(u16, u16)>,
    live_objects: Option<NativeCellObjectView<'_>>,
    cell: (u16, u16),
) -> bool {
    source_object_cells.contains(&cell)
        || live_objects.is_some_and(|objects| objects.ground_object_present(cell))
}

/// The complete `FirstObject != 0` cell set for a rebuild.
fn native_occupied_cells(
    source_object_cells: &BTreeSet<(u16, u16)>,
    live_objects: Option<NativeCellObjectView<'_>>,
) -> BTreeSet<(u16, u16)> {
    let mut cells = source_object_cells.clone();
    if let Some(objects) = live_objects {
        cells.extend(objects.occupied_ground_cells());
    }
    cells
}

/// `CellClass::CanGrowTiberium @ 0x00483620` after the type match: the
/// scenario growth flag (`+0x34A6`), flat slope (`+0x11C == 0`),
/// `OverlayData < MaxDensity - 1`, and `GrowthPercentage >= 1e-05`.
fn native_can_grow_tiberium(
    ty: &crate::rules::tiberium_type::TiberiumType,
    overlay_data: u8,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    cell: (u16, u16),
    growth_enabled: bool,
) -> bool {
    growth_enabled
        && cell_is_flat(resolved_terrain, cell.0, cell.1)
        && overlay_data < ty.max_density.saturating_sub(1)
        && native_percentage_admits(ty.growth_percentage_bits)
}

fn cell_is_flat(resolved_terrain: Option<&ResolvedTerrainGrid>, rx: u16, ry: u16) -> bool {
    resolved_terrain
        .and_then(|grid| grid.cell(rx, ry))
        .map_or(true, |cell| cell.slope_type == 0)
}

fn native_timer_due(timer: NativeTiberiumTimer, current_frame: u32) -> bool {
    current_frame.wrapping_sub(timer.start_frame) >= timer.interval
}

fn priority_f32(entry: &NativeTiberiumQueueEntry) -> f32 {
    f32::from_bits(entry.priority_bits)
}

/// Processor attempts/budget: `CDQ; XOR; SUB` (signed `abs`, which keeps
/// `0x80000000` as `i32::MIN`), `CDQ; IDIV modulus` (signed remainder), `+1`
/// (`0x00722F7A..0x00722F8C`, `0x007224BE..0x007224D0`). The result is at
/// most zero only for the one word `0x80000000`; the caller then returns after
/// its first pop (`0x00722FEA JLE`).
fn signed_abs_mod_plus_one(raw: u32, modulus: u32) -> i32 {
    debug_assert!(modulus > 0);
    let signed = raw as i32;
    signed
        .wrapping_abs()
        .wrapping_rem(modulus as i32)
        .wrapping_add(1)
}

/// `GrowthProcessor` reinsert priority (`0x0072303F..0x00723064`):
/// `abs(raw % 50) + frame` as a float, versus `AddToGrowthQueue`'s
/// `abs(raw) % 50 + frame`; the two differ only for the word `0x80000000`.
fn processor_reinsert_priority(native_frame: u32, raw: u32) -> f32 {
    let remainder = (raw as i32).wrapping_rem(GROWTH_QUEUE_PRIORITY_WINDOW as i32);
    (native_frame as i32).wrapping_add(remainder.wrapping_abs()) as f32
}

fn current_tiberium_type(
    overlay_grid: &OverlayGrid,
    overlay_registry: &OverlayTypeRegistry,
    tiberium_types: &TiberiumTypeRegistry,
    rx: u16,
    ry: u16,
) -> Option<TiberiumTypeId> {
    let overlay_id = overlay_grid.cell(rx, ry).overlay_id?;
    overlay_registry.tiberium_type_for_overlay(tiberium_types, overlay_id)
}

/// `CellClass::CanSpreadTiberium @ 0x00483690`: the scenario spread flag,
/// the cell's OWN TiberiumClass (`OverlayToTiberiumIndex != -1`),
/// `OverlayData > that index / 2` (the index, not `MaxDensity`, is native),
/// flat slope, that class's `SpreadPercentage >= 1e-05`, and `CellClass+0xE4
/// FirstObject == 0` (`source_has_object`). The predicate takes no class:
/// `AddToSpreadQueue @ 0x00722AF0` admits a cell of any class into its
/// receiver's store, and `SpreadTiberium @ 0x00483780` spreads the admitted
/// cell's own class. Returns that class.
#[allow(clippy::too_many_arguments)]
fn native_can_spread_tiberium(
    overlay_grid: &OverlayGrid,
    overlay_registry: &OverlayTypeRegistry,
    tiberium_types: &TiberiumTypeRegistry,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    source_has_object: bool,
    rx: u16,
    ry: u16,
    spread_enabled: bool,
) -> Option<TiberiumTypeId> {
    if !spread_enabled || source_has_object {
        return None;
    }
    let own_type = current_tiberium_type(overlay_grid, overlay_registry, tiberium_types, rx, ry)?;
    let cell = overlay_grid.cell(rx, ry);
    if cell.overlay_data <= own_type.0 / 2 {
        return None;
    }
    if !cell_is_flat(resolved_terrain, rx, ry) {
        return None;
    }
    let ty = tiberium_types.get(own_type)?;
    native_percentage_admits(ty.spread_percentage_bits).then_some(own_type)
}

#[allow(clippy::too_many_arguments)]
fn count_native_spread_targets(
    overlay_grid: &OverlayGrid,
    source_object_cells: &BTreeSet<(u16, u16)>,
    new_cell_admission: Option<NewTiberiumAdmission<'_>>,
    rx: u16,
    ry: u16,
    map_width: u16,
    map_height: u16,
) -> u8 {
    let mut count = 0u8;
    for &(dx, dy) in &ADJACENT_OFFSETS {
        let nx = rx as i32 + dx;
        let ny = ry as i32 + dy;
        if nx < 0 || ny < 0 || nx >= map_width as i32 || ny >= map_height as i32 {
            continue;
        }
        if new_cell_admission.is_some_and(|admission| {
            can_place_new_tiberium(
                overlay_grid,
                source_object_cells,
                admission,
                (nx as u16, ny as u16),
            )
        }) {
            count = count.saturating_add(1);
        }
    }
    count
}

/// `CellClass::SpreadTiberium(0) @ 0x00483780`: re-run `CanSpreadTiberium`
/// on the source, draw `RandomRanged(0,7)`, and germinate the source's OWN
/// class into the first admitted neighbour.
#[allow(clippy::too_many_arguments)]
fn spread_tiberium_from_source(
    overlay_grid: &mut OverlayGrid,
    overlay_registry: &OverlayTypeRegistry,
    tiberium_types: &TiberiumTypeRegistry,
    resolved_terrain: Option<&ResolvedTerrainGrid>,
    source_object_cells: &BTreeSet<(u16, u16)>,
    new_cell_admission: Option<NewTiberiumAdmission<'_>>,
    rx: u16,
    ry: u16,
    map_width: u16,
    map_height: u16,
    spread_enabled: bool,
    ore_growth_state: &mut OreGrowthState,
    rng: &mut SimRng,
    binary_frame: u32,
    mut radar_dirty_cells: Option<&mut Vec<(u16, u16)>>,
    mut radar_dirty_generation: Option<&mut u64>,
    mut tactical_dirty_cells: Option<&mut Vec<(u16, u16)>>,
) -> Option<(u16, u16)> {
    let admission = new_cell_admission?;
    let source_type = native_can_spread_tiberium(
        overlay_grid,
        overlay_registry,
        tiberium_types,
        resolved_terrain,
        cell_has_native_object(
            source_object_cells,
            admission
                .live_objects()
                .map(|objects| objects.object_view()),
            (rx, ry),
        ),
        rx,
        ry,
        spread_enabled,
    )?;
    let start_dir = rng.next_range_u32(8) as usize;
    for i in 0..8 {
        let dir = (start_dir + i) % 8;
        let (dx, dy) = ADJACENT_OFFSETS[dir];
        let nx = rx as i32 + dx;
        let ny = ry as i32 + dy;
        if nx < 0 || ny < 0 || nx >= map_width as i32 || ny >= map_height as i32 {
            continue;
        }
        let target = (nx as u16, ny as u16);
        if !can_place_new_tiberium(overlay_grid, source_object_cells, admission, target) {
            continue;
        }
        let mut context = PlaceTiberiumContext {
            overlay_grid,
            ore_growth_state,
            overlay_registry,
            tiberium_types,
            resolved_terrain,
            source_object_cells,
            new_cell_admission: Some(admission),
            live_objects: admission
                .live_objects()
                .map(|objects| objects.object_view()),
            rng,
            binary_frame,
            growth_enabled: true,
            spread_enabled,
            radar_dirty_cells: radar_dirty_cells.as_deref_mut(),
            radar_dirty_generation: radar_dirty_generation.as_deref_mut(),
            tactical_dirty_cells: tactical_dirty_cells.as_deref_mut(),
        };
        if !place_tiberium(
            &mut context,
            target,
            source_type,
            SPREAD_GERMINATION_DENSITY,
        ) {
            return None;
        }
        return Some(target);
    }
    None
}

/// Advance ore growth/spread by one sim tick.
///
/// This is the main entry point called from advance_tick(). It scans a fraction
/// of the map each tick and executes growth/spread when a full cycle completes.
pub fn tick_ore_growth(
    config: &OreGrowthConfig,
    state: &mut OreGrowthState,
    resource_nodes: &mut BTreeMap<(u16, u16), ResourceNode>,
    path_grid: Option<&PathGrid>,
    mut overlay_grid: Option<&mut crate::sim::overlay_grid::OverlayGrid>,
    rng: &mut SimRng,
) {
    if !config.grows && !config.spreads {
        return;
    }
    if state.total_cells == 0 {
        return;
    }

    // `GrowthRate` is authored against the engine's legacy 15-frame timebase.
    // Game speed changes frame admission, not the number of simulation visits.
    let rate_seconds: u32 = config.growth_rate_seconds.max(1);
    const LEGACY_ORE_GROWTH_FRAMES_PER_RATE_SECOND: u32 =
        crate::util::fixed_math::RA2_LOGIC_FRAMES_PER_SECOND;
    let ticks_per_cycle: u32 = rate_seconds
        .saturating_mul(LEGACY_ORE_GROWTH_FRAMES_PER_RATE_SECOND)
        .max(1);
    let cells_per_tick: usize =
        (state.total_cells as u32).div_ceil(ticks_per_cycle).max(1) as usize;

    // Scan a chunk of cells from the cursor position.
    let scan_end = (state.scan_cursor + cells_per_tick).min(state.total_cells);

    // We iterate over resource_nodes rather than all cells — much more efficient
    // since only a small fraction of cells have ore. We filter by coordinate range
    // corresponding to the current scan chunk.
    for (&(rx, ry), node) in resource_nodes.iter() {
        let cell_index = ry as usize * state.map_width as usize + rx as usize;
        if cell_index < state.scan_cursor || cell_index >= scan_end {
            continue;
        }

        // Only ore grows/spreads (not gems), matching RA1 behavior.
        if node.resource_type != ResourceType::Ore {
            continue;
        }

        // Can this cell grow? (ore present, below max richness)
        if config.grows && node.remaining < MAX_ORE_REMAINING {
            reservoir_sample(
                &mut state.growth_candidates,
                &mut state.growth_seen,
                (rx, ry),
                rng,
            );
        }

        // Can this cell spread? (ore present, above spread threshold)
        if config.spreads && node.remaining > SPREAD_THRESHOLD {
            reservoir_sample(
                &mut state.spread_candidates,
                &mut state.spread_seen,
                (rx, ry),
                rng,
            );
        }
    }

    state.scan_cursor = scan_end;

    // When full scan completes, execute collected growth and spread actions.
    if state.scan_cursor >= state.total_cells {
        // Phase 1: Growth — increase remaining by one richness level.
        if config.grows {
            for &(rx, ry) in &state.growth_candidates {
                if let Some(node) = resource_nodes.get_mut(&(rx, ry)) {
                    if node.resource_type == ResourceType::Ore && node.remaining < MAX_ORE_REMAINING
                    {
                        let new_remaining = node.remaining + ORE_BASE_PER_LEVEL;
                        node.remaining = new_remaining.min(MAX_ORE_REMAINING);
                        // Sync overlay frame to match new density.
                        if let Some(grid) = overlay_grid.as_deref_mut() {
                            let frame = (node.remaining / ORE_BASE_PER_LEVEL)
                                .saturating_sub(1)
                                .min(11) as u8;
                            grid.set_overlay_data(rx, ry, frame);
                        }
                    }
                }
            }
        }

        // Phase 2: Spread — spawn new ore in a random adjacent empty cell.
        if config.spreads {
            for &(rx, ry) in &state.spread_candidates {
                try_spread_ore(
                    resource_nodes,
                    path_grid,
                    overlay_grid.as_deref_mut(),
                    rng,
                    rx,
                    ry,
                    state.map_width,
                );
            }
        }

        // Reset for next cycle.
        state.scan_cursor = 0;
        state.growth_candidates.clear();
        state.spread_candidates.clear();
        state.growth_seen = 0;
        state.spread_seen = 0;

        let node_count = resource_nodes.len();
        log::debug!(
            "Ore growth cycle complete: {} resource nodes on map",
            node_count
        );
    }
}

/// Reservoir sampling: maintain a bounded random sample from a stream.
///
/// Ensures each candidate has an equal probability of being in the final sample,
/// regardless of the total stream length. Matches RA1's MapClass::Logic approach.
fn reservoir_sample(
    candidates: &mut Vec<(u16, u16)>,
    seen: &mut usize,
    cell: (u16, u16),
    rng: &mut SimRng,
) {
    *seen += 1;
    if candidates.len() < MAX_CANDIDATES {
        candidates.push(cell);
    } else {
        // Replace a random existing candidate with probability MAX_CANDIDATES / seen.
        let r = rng.next_range_u32(*seen as u32) as usize;
        if r < MAX_CANDIDATES {
            candidates[r] = cell;
        }
    }
}

/// Native-shaped AddToGrowthQueue priority from one raw RNG word.
/// `AddToGrowthQueue @ 0x007235F9..0x00723612` / `AddToSpreadQueue @
/// 0x00722B5B..0x00722B74`: `abs(raw) % 50 + frame` in signed 32-bit
/// arithmetic, stored as a float. `abs(0x80000000)` stays `i32::MIN`, whose
/// remainder is `-48`.
fn growth_queue_priority(native_frame: u32, raw: u32) -> f32 {
    (native_frame as i32).wrapping_add(growth_queue_priority_delay(raw)) as f32
}

fn growth_queue_priority_delay(raw: u32) -> i32 {
    (raw as i32)
        .wrapping_abs()
        .wrapping_rem(GROWTH_QUEUE_PRIORITY_WINDOW as i32)
}

/// Try to spread ore from (rx, ry) to a random adjacent cell.
///
/// Picks a random starting direction and checks all 8 neighbors. The first
/// cell that passes `can_germinate()` gets a new ore node at level 1.
fn try_spread_ore(
    resource_nodes: &mut BTreeMap<(u16, u16), ResourceNode>,
    path_grid: Option<&PathGrid>,
    overlay_grid: Option<&mut crate::sim::overlay_grid::OverlayGrid>,
    rng: &mut SimRng,
    rx: u16,
    ry: u16,
    map_width: u16,
) {
    // Random starting direction for fairness (matching RA1 Random_Pick(FACING_N, FACING_NW)).
    let start_dir = rng.next_range_u32(8) as usize;

    for i in 0..8 {
        let dir = (start_dir + i) % 8;
        let (dx, dy) = ADJACENT_OFFSETS[dir];
        let nx = rx as i32 + dx;
        let ny = ry as i32 + dy;

        // Bounds check.
        if nx < 0 || ny < 0 || nx >= map_width as i32 {
            continue;
        }
        let nx = nx as u16;
        let ny = ny as u16;

        if can_germinate(resource_nodes, path_grid, nx, ny) {
            resource_nodes.insert(
                (nx, ny),
                ResourceNode {
                    resource_type: ResourceType::Ore,
                    remaining: ORE_BASE_PER_LEVEL,
                },
            );
            // New ore at level 1 -> frame 0. Copy overlay_id from source cell.
            if let Some(grid) = overlay_grid {
                if let Some(source_id) = grid.cell(rx, ry).overlay_id {
                    grid.place_overlay(nx, ny, source_id, 0);
                }
            }
            return;
        }
    }
}

/// Whether a cell can receive new ore via spread.
///
/// Matches RA1 CellClass::Can_Tiberium_Germinate:
/// - No existing resource node on the cell
/// - Cell is within map bounds
/// - Cell is walkable (not water, cliff, or building footprint)
fn can_germinate(
    resource_nodes: &BTreeMap<(u16, u16), ResourceNode>,
    path_grid: Option<&PathGrid>,
    rx: u16,
    ry: u16,
) -> bool {
    // Already has a resource node — can't place another.
    if resource_nodes.contains_key(&(rx, ry)) {
        return false;
    }

    // Must be walkable terrain (not water, cliff, or building).
    if let Some(grid) = path_grid {
        if !grid.is_walkable(rx, ry) {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::bridge_facts::BridgeCellFacts;
    use crate::map::entities::EntityCategory;
    use crate::map::overlay::OverlayEntry;
    use crate::map::overlay_types::OverlayTypeRegistry;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    use crate::rules::ini_parser::IniFile;
    use crate::rules::ruleset::RuleSet;
    use crate::rules::terrain_rules::{LandType, SpeedCostProfile, TerrainClass};
    use crate::rules::tiberium_type::{TiberiumTypeId, TiberiumTypeRegistry};
    use crate::sim::entity_store::EntityStore;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::StringInterner;
    use crate::sim::miner::{ResourceNode, ResourceType};
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
    use crate::sim::overlay_grid::OverlayGrid;
    use crate::sim::rng::SimRng;

    fn make_config(grows: bool, spreads: bool) -> OreGrowthConfig {
        OreGrowthConfig {
            grows,
            spreads,
            tiberium_grows_flag: false,
            growth_rate_seconds: 1, // Very fast for testing
        }
    }

    fn make_state(width: u16, height: u16) -> OreGrowthState {
        OreGrowthState::new(width, height)
    }

    fn flat_clear_resolved_grid(width: u16, height: u16) -> ResolvedTerrainGrid {
        let land_type = LandType::Clear.as_index();
        let speed_costs = SpeedCostProfile::default();
        let mut cells = Vec::with_capacity(width as usize * height as usize);
        for ry in 0..height {
            for rx in 0..width {
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
                    land_type,
                    yr_cell_land_type: land_type,
                    slope_type: 0,
                    template_height: 0,
                    render_offset_x: 0,
                    render_offset_y: 0,
                    terrain_class: TerrainClass::Clear,
                    speed_costs,
                    is_water: false,
                    is_cliff_like: false,
                    is_rough: false,
                    is_road: false,
                    accepts_smudge: true,
                    allows_tiberium: true,
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
                    base_land_type: land_type,
                    base_yr_cell_land_type: land_type,
                    base_terrain_class: TerrainClass::Clear,
                    base_speed_costs: speed_costs,
                    build_blocked: false,
                    has_bridge_deck: false,
                    bridge_walkable: false,
                    bridge_transition: false,
                    bridge_deck_level: 0,
                    bridge_layer: None,
                    bridge_facts: BridgeCellFacts::default(),
                    tube_index: None,
                    radar_left: [0; 3],
                    radar_right: [0; 3],
                    has_damaged_data: false,
                    bridgehead_anchor_class_at_load: None,
                });
            }
        }
        ResolvedTerrainGrid::from_cells(width, height, cells)
    }

    fn ore_node(remaining: u16) -> ResourceNode {
        ResourceNode {
            resource_type: ResourceType::Ore,
            remaining,
        }
    }

    fn gem_node(remaining: u16) -> ResourceNode {
        ResourceNode {
            resource_type: ResourceType::Gem,
            remaining,
        }
    }

    fn tiberium_rebuild_fixture() -> (IniFile, OverlayTypeRegistry, TiberiumTypeRegistry) {
        let mut text = String::from(
            "\
[Tiberiums]
0=Riparius
1=Cruentus
2=Vinifera

[Riparius]
Image=1
Value=25
Growth=2200
GrowthPercentage=.06
Spread=2200
SpreadPercentage=.06

[Cruentus]
Image=2
Value=50
Growth=10000
GrowthPercentage=0
Spread=10000
SpreadPercentage=0

[Vinifera]
Image=3
Value=25
Growth=2200
GrowthPercentage=.06
Spread=2200
SpreadPercentage=.06

[OverlayTypes]
",
        );
        let mut tiberium_names = Vec::new();
        for raw_key in (1..=149).filter(|key| *key != 40 && *key != 41) {
            let name = match raw_key {
                28..=39 => format!("GEM{:02}", raw_key - 27),
                105..=124 => format!("TIB{:02}", raw_key - 104),
                130..=149 => format!("TIB2_{:02}", raw_key - 129),
                _ => format!("FILL{raw_key:03}"),
            };
            text.push_str(&format!("{raw_key}={name}\n"));
            if name.starts_with("TIB") || name.starts_with("GEM") {
                tiberium_names.push(name);
            }
        }
        for name in tiberium_names {
            text.push_str(&format!("[{name}]\nTiberium=yes\n"));
        }
        let ini = IniFile::from_str(&text);
        let overlay_registry = OverlayTypeRegistry::from_ini(&ini, None);
        let tiberium_types = TiberiumTypeRegistry::from_ini(&ini);
        (ini, overlay_registry, tiberium_types)
    }

    /// Run enough ticks to complete one full scan cycle.
    fn run_full_cycle(
        config: &OreGrowthConfig,
        state: &mut OreGrowthState,
        nodes: &mut BTreeMap<(u16, u16), ResourceNode>,
        rng: &mut SimRng,
    ) {
        for _ in 0..10000 {
            tick_ore_growth(config, state, nodes, None, None, rng);
            if state.scan_cursor == 0 {
                return;
            }
        }
        panic!("Full cycle did not complete within 10000 ticks");
    }

    #[test]
    fn growth_increments_ore_remaining() {
        let config = make_config(true, false);
        let mut state = make_state(10, 10);
        let mut nodes = BTreeMap::new();
        nodes.insert((5, 5), ore_node(120)); // Level 1
        let mut rng = SimRng::new(42);

        run_full_cycle(&config, &mut state, &mut nodes, &mut rng);

        let node = nodes.get(&(5, 5)).expect("node still exists");
        assert_eq!(node.remaining, 240, "Should grow by one level (120)");
    }

    #[test]
    fn growth_caps_at_max_remaining() {
        let config = make_config(true, false);
        let mut state = make_state(10, 10);
        let mut nodes = BTreeMap::new();
        nodes.insert((3, 3), ore_node(MAX_ORE_REMAINING - 10)); // Near max
        let mut rng = SimRng::new(42);

        run_full_cycle(&config, &mut state, &mut nodes, &mut rng);

        let node = nodes.get(&(3, 3)).expect("node still exists");
        assert_eq!(node.remaining, MAX_ORE_REMAINING, "Should cap at max");
    }

    #[test]
    fn gems_do_not_grow_or_spread() {
        let config = make_config(true, true);
        let mut state = make_state(10, 10);
        let mut nodes = BTreeMap::new();
        nodes.insert((5, 5), gem_node(900)); // Rich gems — above spread threshold
        let mut rng = SimRng::new(42);

        run_full_cycle(&config, &mut state, &mut nodes, &mut rng);

        let node = nodes.get(&(5, 5)).expect("node still exists");
        assert_eq!(node.remaining, 900, "Gems should not grow");
        // Only the original gem node should exist (no spread).
        assert_eq!(nodes.len(), 1, "Gems should not spread");
    }

    #[test]
    fn spread_creates_new_ore_node() {
        let config = make_config(false, true);
        let mut state = make_state(10, 10);
        let mut nodes = BTreeMap::new();
        // Rich ore above spread threshold.
        nodes.insert((5, 5), ore_node(SPREAD_THRESHOLD + 120));
        let mut rng = SimRng::new(42);

        run_full_cycle(&config, &mut state, &mut nodes, &mut rng);

        assert!(
            nodes.len() > 1,
            "Should have spread to at least one adjacent cell"
        );
        // New node should be ore at base level.
        for (&(rx, ry), node) in &nodes {
            if rx == 5 && ry == 5 {
                continue;
            }
            assert_eq!(node.resource_type, ResourceType::Ore);
            assert_eq!(node.remaining, ORE_BASE_PER_LEVEL);
            // Must be adjacent to (5,5).
            let dx = (rx as i32 - 5).unsigned_abs();
            let dy = (ry as i32 - 5).unsigned_abs();
            assert!(dx <= 1 && dy <= 1, "Spread node must be adjacent");
        }
    }

    #[test]
    fn ore_below_threshold_does_not_spread() {
        let config = make_config(false, true);
        let mut state = make_state(10, 10);
        let mut nodes = BTreeMap::new();
        nodes.insert((5, 5), ore_node(SPREAD_THRESHOLD - 1)); // Below threshold
        let mut rng = SimRng::new(42);

        run_full_cycle(&config, &mut state, &mut nodes, &mut rng);

        assert_eq!(nodes.len(), 1, "Low ore should not spread");
    }

    #[test]
    fn disabled_flags_prevent_all_activity() {
        let config = make_config(false, false);
        let mut state = make_state(10, 10);
        let mut nodes = BTreeMap::new();
        nodes.insert((5, 5), ore_node(120));
        let mut rng = SimRng::new(42);

        // Run many ticks — nothing should change.
        for _ in 0..100 {
            tick_ore_growth(&config, &mut state, &mut nodes, None, None, &mut rng);
        }

        let node = nodes.get(&(5, 5)).expect("node still exists");
        assert_eq!(node.remaining, 120, "Nothing should change when disabled");
    }

    #[test]
    fn cannot_germinate_on_existing_node() {
        let mut nodes = BTreeMap::new();
        nodes.insert((5, 5), ore_node(120));

        assert!(!can_germinate(&nodes, None, 5, 5));
        assert!(can_germinate(&nodes, None, 5, 6));
    }

    #[test]
    fn reservoir_sampling_stays_bounded() {
        let mut candidates: Vec<(u16, u16)> = Vec::new();
        let mut seen: usize = 0;
        let mut rng = SimRng::new(99);

        for i in 0..500 {
            reservoir_sample(&mut candidates, &mut seen, (i, 0), &mut rng);
        }

        assert_eq!(seen, 500);
        assert!(
            candidates.len() <= MAX_CANDIDATES,
            "Candidates should not exceed MAX_CANDIDATES"
        );
    }

    #[test]
    fn growth_queue_priority_uses_signed_abs_raw_modulo() {
        assert_eq!(growth_queue_priority_delay(0), 0);
        assert_eq!(growth_queue_priority_delay(0xFFFF_FFFF), 1);
        assert_eq!(growth_queue_priority_delay(51), 1);
        // `abs(0x80000000)` stays `i32::MIN` (`CDQ; XOR; SUB`), whose signed
        // remainder by 50 is -48; the processor's `abs(raw % 50)` form yields
        // +48 for the same word, and both are the native results.
        assert_eq!(growth_queue_priority_delay(0x8000_0000), -48);
        assert_eq!(growth_queue_priority(100, 0x8000_0000), 52.0);
        assert_eq!(processor_reinsert_priority(100, 0x8000_0000), 148.0);
        assert_eq!(processor_reinsert_priority(100, 51), 101.0);
        assert_eq!(signed_abs_mod_plus_one(0x8000_0000, 5), -2);
        assert_eq!(signed_abs_mod_plus_one(0xFFFF_FFFB, 5), 1);
        assert_eq!(signed_abs_mod_plus_one(7, 5), 3);
    }

    #[test]
    fn enqueue_growth_queue_cell_consumes_one_raw_draw_and_stores_priority() {
        let mut state = make_state(20, 20);
        let mut rng = SimRng::new(1);
        let before = rng.state();

        let entry = state.enqueue_growth_queue_cell(4, 7, 1234, &mut rng);

        assert_ne!(rng.state(), before, "queue insertion consumes one raw draw");
        assert_eq!(entry.rx, 4);
        assert_eq!(entry.ry, 7);
        assert_eq!(
            entry.priority,
            growth_queue_priority(1234, 0x78B7_6ED5),
            "first raw draw for seed 1 should set native-style priority"
        );
        assert_eq!(state.growth_queue_entries(), &[entry]);
    }

    #[test]
    fn native_tiberium_shell_allocates_per_type_due_timers() {
        let mut state = make_state(20, 20);

        state.reset_native_tiberium_classes(4, 1234);

        let native = state.native_tiberium_state();
        assert_eq!(native.classes.len(), 4);
        for class in &native.classes {
            assert_eq!(class.growth_timer, NativeTiberiumTimer::due(1234));
            assert_eq!(class.spread_timer, NativeTiberiumTimer::due(1234));
            assert!(class.growth.is_empty());
            assert!(class.spread.is_empty());
            assert!(class.growth_bitmap.is_empty());
            assert!(class.spread_bitmap.is_empty());
        }
    }

    #[test]
    fn native_tiberium_shell_hashes_timers_heaps_and_bitmaps() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::Hasher;

        let mut base = make_state(20, 20);
        base.reset_native_tiberium_classes(1, 10);

        let mut changed = base.clone();
        let class = &mut changed.native_tiberium.classes[0];
        class.growth_timer.interval = 2200;
        class.growth.push(NativeTiberiumQueueEntry {
            rx: 4,
            ry: 7,
            priority_bits: 0.0f32.to_bits(),
        });
        class.spread_bitmap.insert((5, 8));

        let mut base_hasher = DefaultHasher::new();
        base.hash_state(&mut base_hasher);
        let mut changed_hasher = DefaultHasher::new();
        changed.hash_state(&mut changed_hasher);

        assert_ne!(base_hasher.finish(), changed_hasher.finish());
    }

    #[test]
    fn native_tiberium_rebuild_seeds_growth_and_spread_from_overlay_cells() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let tib02 = overlay_registry.id_for_name("TIB02").expect("TIB02");
        let gem01 = overlay_registry.id_for_name("GEM01").expect("GEM01");
        let gem02 = overlay_registry.id_for_name("GEM02").expect("GEM02");
        let overlay_grid = OverlayGrid::from_overlay_entries(
            &[
                OverlayEntry {
                    rx: 5,
                    ry: 5,
                    overlay_id: tib01,
                    frame: 10,
                },
                OverlayEntry {
                    rx: 6,
                    ry: 5,
                    overlay_id: tib02,
                    frame: 11,
                },
                OverlayEntry {
                    rx: 5,
                    ry: 6,
                    overlay_id: gem01,
                    frame: 0,
                },
                OverlayEntry {
                    rx: 6,
                    ry: 6,
                    overlay_id: gem02,
                    frame: 1,
                },
            ],
            8,
            8,
        );
        let mut state = make_state(8, 8);

        let stats = state.rebuild_native_tiberium_queues_from_overlays(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            &BTreeSet::new(),
            true,
            true,
            77,
            (8, 8),
        );

        assert_eq!(
            stats,
            NativeTiberiumRebuildStats {
                growth_entries: 1,
                spread_entries: 2,
            }
        );
        let native = state.native_tiberium_state();
        let riparius = &native.classes[TiberiumTypeId(0).0 as usize];
        let cruentus = &native.classes[TiberiumTypeId(1).0 as usize];
        assert_eq!(riparius.growth.len(), 1, "data 11 does not grow");
        assert_eq!(riparius.spread.len(), 2, "data 10 and 11 spread");
        assert_eq!(
            cruentus.growth.len(),
            0,
            "a zero GrowthPercentage fails the CanGrowTiberium 1e-05 gate"
        );
        assert_eq!(
            cruentus.spread.len(),
            0,
            "a zero SpreadPercentage fails the CanSpreadTiberium 1e-05 gate"
        );
        assert!(
            riparius
                .growth
                .iter_heap()
                .all(|entry| entry.priority_bits == 0.0f32.to_bits())
        );
        assert_eq!(riparius.growth_bitmap.len(), riparius.growth.len());
        assert_eq!(cruentus.spread_bitmap.len(), cruentus.spread.len());
    }

    #[test]
    fn native_queue_rebuild_classifies_nonzero_extra_variant_through_production_method() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib2_20 = overlay_registry.id_for_name("TIB2_20").expect("TIB2_20");
        assert_eq!(tib2_20, 146);
        let overlay_grid = OverlayGrid::from_overlay_entries(
            &[OverlayEntry {
                rx: 7,
                ry: 7,
                overlay_id: tib2_20,
                frame: 3,
            }],
            8,
            8,
        );
        let mut state = make_state(8, 8);

        let stats = state.rebuild_native_tiberium_queues_from_overlays(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            &BTreeSet::new(),
            true,
            false,
            77,
            (8, 8),
        );

        assert_eq!(
            stats,
            NativeTiberiumRebuildStats {
                growth_entries: 1,
                spread_entries: 0,
            }
        );
        for (class_index, class) in state.native_tiberium_state().classes.iter().enumerate() {
            if class_index == 2 {
                assert_eq!(class.growth.len(), 1);
                assert_eq!(
                    (
                        class.growth.heap_entry(0).unwrap().rx,
                        class.growth.heap_entry(0).unwrap().ry
                    ),
                    (7, 7)
                );
                assert_eq!(
                    class.growth.heap_entry(0).unwrap().priority_bits,
                    0.0f32.to_bits()
                );
                assert_eq!(class.growth_bitmap, BTreeSet::from([(7, 7)]));
            } else {
                assert!(class.growth.is_empty(), "wrong class {class_index}");
                assert!(class.growth_bitmap.is_empty(), "wrong class {class_index}");
            }
            assert!(class.spread.is_empty());
            assert!(class.spread_bitmap.is_empty());
        }
    }

    #[test]
    fn native_tiberium_rebuild_respects_basic_growth_and_source_object_gates() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let tib02 = overlay_registry.id_for_name("TIB02").expect("TIB02");
        let overlay_grid = OverlayGrid::from_overlay_entries(
            &[
                OverlayEntry {
                    rx: 5,
                    ry: 5,
                    overlay_id: tib01,
                    frame: 10,
                },
                OverlayEntry {
                    rx: 6,
                    ry: 5,
                    overlay_id: tib02,
                    frame: 10,
                },
            ],
            8,
            8,
        );
        let mut source_object_cells = BTreeSet::new();
        source_object_cells.insert((5, 5));
        let mut state = make_state(8, 8);

        let stats = state.rebuild_native_tiberium_queues_from_overlays(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            &source_object_cells,
            false,
            true,
            99,
            (8, 8),
        );

        assert_eq!(
            stats,
            NativeTiberiumRebuildStats {
                growth_entries: 0,
                spread_entries: 1,
            }
        );
        let riparius = &state.native_tiberium_state().classes[0];
        assert!(riparius.growth.is_empty());
        assert_eq!(riparius.spread.len(), 1);
        assert_eq!(
            (
                riparius.spread.heap_entry(0).unwrap().rx,
                riparius.spread.heap_entry(0).unwrap().ry
            ),
            (6, 5)
        );
    }

    #[test]
    fn native_tiberium_rebuild_clears_previous_native_queue_state() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let populated = OverlayGrid::from_overlay_entries(
            &[OverlayEntry {
                rx: 5,
                ry: 5,
                overlay_id: tib01,
                frame: 10,
            }],
            8,
            8,
        );
        let empty = OverlayGrid::new(8, 8);
        let mut state = make_state(8, 8);
        state.rebuild_native_tiberium_queues_from_overlays(
            &populated,
            &overlay_registry,
            &tiberium_types,
            None,
            &BTreeSet::new(),
            true,
            true,
            1,
            (8, 8),
        );
        assert!(!state.native_tiberium_state().classes[0].growth.is_empty());

        let stats = state.rebuild_native_tiberium_queues_from_overlays(
            &empty,
            &overlay_registry,
            &tiberium_types,
            None,
            &BTreeSet::new(),
            true,
            true,
            2,
            (8, 8),
        );

        assert_eq!(stats, NativeTiberiumRebuildStats::default());
        for class in &state.native_tiberium_state().classes {
            assert!(class.growth.is_empty());
            assert!(class.spread.is_empty());
            assert!(class.growth_bitmap.is_empty());
            assert!(class.spread_bitmap.is_empty());
            assert_eq!(class.growth_timer, NativeTiberiumTimer::due(2));
            assert_eq!(class.spread_timer, NativeTiberiumTimer::due(2));
        }
    }

    #[test]
    fn native_add_to_growth_queue_allows_duplicates_and_rejects_density_11_without_rng() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        overlay_grid.place_overlay(1, 1, tib01, 3);
        overlay_grid.place_overlay(2, 1, tib01, 11);
        let mut state = make_state(8, 8);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        let mut rng = SimRng::new(1);
        let mut expected_after_accepts = rng.clone();
        expected_after_accepts.next_u32();
        expected_after_accepts.next_u32();

        let first = state.add_native_growth_queue_cell(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            1,
            1,
            100,
            &mut rng,
        );
        let second = state.add_native_growth_queue_cell(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            1,
            1,
            100,
            &mut rng,
        );
        assert_eq!(
            rng.logical_state(),
            expected_after_accepts.logical_state(),
            "each accepted growth insertion consumes exactly one raw draw"
        );
        let before_reject = rng.state();
        let before_reject_logical = rng.logical_state();
        let rejected = state.add_native_growth_queue_cell(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            2,
            1,
            100,
            &mut rng,
        );

        assert!(first.is_some());
        assert!(second.is_some());
        assert_eq!(
            state.native_tiberium_state().classes[0].growth.len(),
            2,
            "growth inserts are not deduped by bitmap"
        );
        assert_eq!(
            state.native_tiberium_state().classes[0].growth_bitmap.len(),
            1
        );
        assert_eq!(rejected, None);
        assert_eq!(
            rng.state(),
            before_reject,
            "density-11 rejection consumes no RNG"
        );
        assert_eq!(
            rng.logical_state(),
            before_reject_logical,
            "density-11 rejection preserves every logical RNG field"
        );
    }

    #[test]
    fn native_growth_processor_zero_percentage_exits_without_rng() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let gem01 = overlay_registry.id_for_name("GEM01").expect("GEM01");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        overlay_grid.place_overlay(1, 1, gem01, 1);
        let mut state = make_state(8, 8);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        state.native_tiberium.classes[1]
            .growth
            .push(NativeTiberiumQueueEntry {
                rx: 1,
                ry: 1,
                priority_bits: 0.0f32.to_bits(),
            });
        let mut nodes = BTreeMap::new();
        let mut rng = SimRng::new(7);
        let before = rng.state();

        let stats = state.process_native_growth_for_type(
            TiberiumTypeId(1),
            &mut overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            &BTreeSet::new(),
            &mut nodes,
            &mut rng,
            100,
            true,
        );

        assert_eq!(stats, NativeGrowthProcessStats::default());
        assert_eq!(rng.state(), before);
        assert_eq!(state.native_tiberium_state().classes[1].growth.len(), 1);
    }

    /// `GrowthProcessor @ 0x00722F00` (`0x00723021..0x0072312B`): a popped
    /// entry whose `GrowTiberium` fails (here an entry popping at the clamp
    /// density 11) still takes the `OverlayData < 0x0B` test, so its growth
    /// flag is cleared without a priority draw and without a reinsert.
    #[test]
    fn native_growth_processor_failed_growth_pop_still_clears_the_full_cell_flag() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        overlay_grid.place_overlay(5, 5, tib01, 11);
        let mut state = make_state(8, 8);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        state.native_tiberium.classes[0]
            .growth
            .push(NativeTiberiumQueueEntry {
                rx: 5,
                ry: 5,
                priority_bits: 0.0f32.to_bits(),
            });
        state.native_tiberium.classes[0]
            .growth_bitmap
            .insert((5, 5));
        let mut nodes = BTreeMap::new();
        let mut rng = SimRng::new(9);
        let mut expected_rng = rng.clone();
        expected_rng.next_u32();

        let stats = state.process_native_growth_for_type(
            TiberiumTypeId(0),
            &mut overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            &BTreeSet::new(),
            &mut nodes,
            &mut rng,
            100,
            true,
        );

        assert_eq!(overlay_grid.cell(5, 5).overlay_data, 11);
        assert_eq!(stats.popped_entries, 1);
        assert_eq!(stats.grown_entries, 0);
        assert_eq!(stats.reinserted_entries, 0);
        assert_eq!(stats.full_clears, 1);
        let class = &state.native_tiberium_state().classes[0];
        assert!(class.growth.is_empty());
        assert!(!class.growth_bitmap.contains(&(5, 5)));
        assert_eq!(
            rng.logical_state(),
            expected_rng.logical_state(),
            "only the attempts draw"
        );
    }

    #[test]
    fn native_growth_processor_drops_stale_entry_without_clearing_bitmap() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let mut overlay_grid = OverlayGrid::new(8, 8);
        let mut state = make_state(8, 8);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        state.native_tiberium.classes[0]
            .growth
            .push(NativeTiberiumQueueEntry {
                rx: 1,
                ry: 1,
                priority_bits: 0.0f32.to_bits(),
            });
        state.native_tiberium.classes[0]
            .growth_bitmap
            .insert((1, 1));
        let mut nodes = BTreeMap::new();
        let mut rng = SimRng::new(3);

        let stats = state.process_native_growth_for_type(
            TiberiumTypeId(0),
            &mut overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            &BTreeSet::new(),
            &mut nodes,
            &mut rng,
            100,
            true,
        );

        assert_eq!(stats.processor_calls, 1);
        assert_eq!(stats.attempt_rng_draws, 1);
        assert_eq!(stats.popped_entries, 1);
        assert_eq!(stats.stale_entries, 1);
        assert!(state.native_tiberium_state().classes[0].growth.is_empty());
        assert!(
            state.native_tiberium_state().classes[0]
                .growth_bitmap
                .contains(&(1, 1)),
            "stale pop does not clear the growth bitmap"
        );
    }

    #[test]
    fn gsi_04_09_scheduled_growth_10_to_11_uses_shared_mutation_contract() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        overlay_grid.place_overlay(1, 1, tib01, 10);
        let mut state = make_state(8, 8);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        state.native_tiberium.classes[0]
            .growth
            .push(NativeTiberiumQueueEntry {
                rx: 1,
                ry: 1,
                priority_bits: 0.0f32.to_bits(),
            });
        state.native_tiberium.classes[0]
            .growth_bitmap
            .insert((1, 1));
        let mut nodes = BTreeMap::new();
        nodes.insert((1, 1), ore_node(10 * ORE_BASE_PER_LEVEL));
        let mut rng = SimRng::new(5);
        let mut expected_rng = rng.clone();
        expected_rng.next_u32(); // GrowthProcessor attempt budget.
        let spread_priority_raw = expected_rng.next_u32();
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();

        let stats = state.tick_native_growth_driver(
            &mut overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            &BTreeSet::new(),
            None,
            &mut nodes,
            &mut rng,
            100,
            true,
            true,
            false,
            Some(&mut radar_dirty),
            Some(&mut radar_generation),
            Some(&mut tactical_dirty),
        );

        assert_eq!(overlay_grid.cell(1, 1).overlay_data, 11);
        assert_eq!(stats.grown_entries, 1);
        assert_eq!(stats.full_clears, 1);
        assert_eq!(stats.spread_feed_calls, 1);
        assert_eq!(stats.spread_enqueued_entries, 1);
        assert!(state.native_tiberium_state().classes[0].growth.is_empty());
        assert!(
            !state.native_tiberium_state().classes[0]
                .growth_bitmap
                .contains(&(1, 1))
        );
        let spread_class = &state.native_tiberium_state().classes[0];
        assert_eq!(spread_class.spread.len(), 1);
        assert!(spread_class.spread_bitmap.contains(&(1, 1)));
        assert_eq!(
            spread_class.spread.heap_entry(0).unwrap().priority_bits,
            growth_queue_priority(100, spread_priority_raw).to_bits()
        );
        assert_eq!(tactical_dirty, vec![(1, 1)]);
        assert!(
            radar_dirty.is_empty(),
            "existing growth does not dirty radar"
        );
        assert_eq!(radar_generation, 0);
        assert_eq!(
            nodes.get(&(1, 1)).map(|node| node.remaining),
            Some(10 * ORE_BASE_PER_LEVEL),
            "the native overlay path does not maintain a duplicate ResourceNode stock"
        );
        assert_eq!(rng.logical_state(), expected_rng.logical_state());
    }

    /// `GrowthDriver_AllTypes @ 0x00722C9E..0x00722CE3` under the chop control
    /// word `0x0E7F`: `ftol(2200 * 0.3)` is 659, not 660, because the 53-bit
    /// truncated product sits one ulp below 660; `Growth=10000` gives 2999.
    /// Without flags bit `0x40` the raw `Growth=` reloads.
    #[test]
    fn gsi_09_04_growth_reload_is_chopped_growth_times_point_three_when_flag_set() {
        assert_eq!(native_growth_timer_reload(2200, true), 659);
        assert_eq!(native_growth_timer_reload(10000, true), 2999);
        assert_eq!(native_growth_timer_reload(2200, false), 2200);
        assert_eq!(native_growth_timer_reload(10000, false), 10000);
        assert_eq!(native_growth_timer_reload(0, true), 0);
        assert_eq!(native_growth_timer_reload(1, true), 0);
        // `10 * 0.3` chops one ulp below 3.0, so the truncating `ftol` gives 2.
        assert_eq!(native_growth_timer_reload(10, true), 2);
    }

    /// The driver fires on frame 0 (due timers), reloads to 659, and fires
    /// again on frames 659 and 1318 in skirmish; the same `Growth=2200` type
    /// without the flag reloads to 2200 and does not refire inside 1400 frames.
    #[test]
    fn gsi_09_04_growth_driver_fires_at_native_skirmish_cadence() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let riparius = TiberiumTypeId(0);
        assert_eq!(tiberium_types.get(riparius).unwrap().growth, 2200);
        let mut nodes = BTreeMap::new();
        let mut run = |tiberium_grows_flag: bool| -> Vec<u32> {
            let mut overlay_grid = OverlayGrid::new(8, 8);
            let mut state = make_state(8, 8);
            state.reset_native_tiberium_classes(tiberium_types.len(), 0);
            let mut rng = SimRng::new(3);
            let mut fired = Vec::new();
            for frame in 0..1400u32 {
                let before = state.native_tiberium_state().classes[0].growth_timer;
                state.tick_native_growth_driver(
                    &mut overlay_grid,
                    &overlay_registry,
                    &tiberium_types,
                    None,
                    &BTreeSet::new(),
                    None,
                    &mut nodes,
                    &mut rng,
                    frame,
                    true,
                    true,
                    tiberium_grows_flag,
                    None,
                    None,
                    None,
                );
                let after = state.native_tiberium_state().classes[0].growth_timer;
                if after != before || frame == 0 {
                    assert_eq!(after.start_frame, frame);
                    fired.push(frame);
                }
            }
            let interval = state.native_tiberium_state().classes[0]
                .growth_timer
                .interval;
            assert_eq!(
                interval,
                native_growth_timer_reload(2200, tiberium_grows_flag)
            );
            fired
        };

        assert_eq!(run(true), vec![0, 659, 1318]);
        assert_eq!(run(false), vec![0]);
    }

    /// `OreGrowthConfig::resolve`: GameMode != 0 takes the session bits and
    /// ignores the map `[SpecialFlags]` keys (never read there, `0x006B8CA0`);
    /// GameMode 0 reads the map keys; `[General] TiberiumGrows/TiberiumSpreads`
    /// gate nothing; `[Basic] TiberiumGrowthEnabled` gates in every mode.
    #[test]
    fn gsi_09_04_config_resolves_flag_bits_from_session_in_nonzero_game_mode() {
        use crate::map::basic::{BasicSection, SpecialFlagsSection};
        use crate::rules::ini_parser::IniFile;
        use crate::sim::scenario_session::{ScenarioDescriptor, ScenarioSession};

        let general = crate::rules::ruleset::RuleSet::from_ini(&IniFile::from_str(
            "[General]\nTiberiumGrows=no\nTiberiumSpreads=no\n",
        ))
        .expect("rules")
        .general;
        assert!(!general.tiberium_grows && !general.tiberium_spreads);
        let map_off = SpecialFlagsSection {
            tiberium_grows: Some(false),
            tiberium_spreads: Some(false),
            ..SpecialFlagsSection::default()
        };
        let basic = BasicSection::default();

        let skirmish = ScenarioSession::from_descriptor(&ScenarioDescriptor {
            game_mode_nonzero: true,
            tiberium_grows_flag: true,
            tiberium_spreads_flag: true,
            ..ScenarioDescriptor::default()
        });
        let config = OreGrowthConfig::resolve(&general, &basic, &map_off, &skirmish);
        assert!(config.grows && config.spreads && config.tiberium_grows_flag);

        let campaign = ScenarioSession::from_descriptor(&ScenarioDescriptor::default());
        let config = OreGrowthConfig::resolve(&general, &basic, &map_off, &campaign);
        assert!(config.grows && !config.spreads && !config.tiberium_grows_flag);
        let config =
            OreGrowthConfig::resolve(&general, &basic, &SpecialFlagsSection::default(), &campaign);
        assert!(config.grows && config.spreads && config.tiberium_grows_flag);

        let basic_off = BasicSection {
            tiberium_growth_enabled: Some(false),
            ..BasicSection::default()
        };
        let config = OreGrowthConfig::resolve(&general, &basic_off, &map_off, &skirmish);
        assert!(!config.grows && config.spreads && config.tiberium_grows_flag);
    }

    /// `GrowthProcessor @ 0x00722F00`: seed 9 draws five attempts, and with
    /// a single heap entry every reinsert is popped straight back
    /// (`0x00723030..0x007230D8` then the `attempts <= processed` test), so
    /// one cell grows once per attempt while `AddToSpreadQueue` dedupes.
    #[test]
    fn native_growth_processor_reinserts_submax_cell_and_counts_spread_feed() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        overlay_grid.place_overlay(1, 1, tib01, 3);
        let mut state = make_state(8, 8);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        state.native_tiberium.classes[0]
            .growth
            .push(NativeTiberiumQueueEntry {
                rx: 1,
                ry: 1,
                priority_bits: 0.0f32.to_bits(),
            });
        let mut nodes = BTreeMap::new();
        nodes.insert((1, 1), ore_node(3 * ORE_BASE_PER_LEVEL));
        let mut rng = SimRng::new(9);

        let stats = state.process_native_growth_for_type(
            TiberiumTypeId(0),
            &mut overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            &BTreeSet::new(),
            &mut nodes,
            &mut rng,
            100,
            true,
        );

        assert_eq!(overlay_grid.cell(1, 1).overlay_data, 8);
        assert_eq!(stats.requested_attempts, 5);
        assert_eq!(stats.grown_entries, 5);
        assert_eq!(stats.reinserted_entries, 5);
        assert_eq!(stats.spread_feed_calls, 5);
        assert_eq!(stats.spread_enqueued_entries, 1);
        assert_eq!(state.native_tiberium_state().classes[0].growth.len(), 1);
        assert_eq!(state.native_tiberium_state().classes[0].spread.len(), 1);
        assert!(
            state.native_tiberium_state().classes[0]
                .growth_bitmap
                .contains(&(1, 1))
        );
    }

    /// `PlaceTiberium @ 0x00487190`'s existing-cell branch feeds
    /// `AddToSpreadQueue @ 0x00722AF0`, whose `CanSpreadTiberium @ 0x00483690`
    /// returns `Cell+0xE4 FirstObject == 0`. `TerrainClass::Mark @ 0x0071BFF8`
    /// links every terrain object into `FirstObject`, spawning or not, so ore
    /// under a plain tree still grows and re-queues for growth but never
    /// becomes a spread source. The production view joins the terrain cell
    /// index with the ground occupancy list.
    #[test]
    fn native_growth_processor_skips_spread_feed_under_a_plain_terrain_object() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let rules_ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
        );
        let rules = RuleSet::from_ini(&rules_ini).expect("rules");
        let interner = StringInterner::default();
        let entities = EntityStore::new();
        let occupancy = OccupancyGrid::new();

        let run = |terrain_object_cells: &BTreeMap<(u16, u16), u64>| {
            let mut overlay_grid = OverlayGrid::new(8, 8);
            overlay_grid.place_overlay(5, 5, tib01, 3);
            let mut state = make_state(8, 8);
            state.reset_native_tiberium_classes(tiberium_types.len(), 10);
            state.native_tiberium.classes[0]
                .growth
                .push(NativeTiberiumQueueEntry {
                    rx: 5,
                    ry: 5,
                    priority_bits: 0.0f32.to_bits(),
                });
            let live_objects = TiberiumPlacementObjectContext::new(
                &entities,
                &occupancy,
                &rules,
                &interner,
                terrain_object_cells,
            );
            let mut nodes = BTreeMap::new();
            nodes.insert((5, 5), ore_node(3 * ORE_BASE_PER_LEVEL));
            let mut rng = SimRng::new(9);
            let stats = state.process_native_growth_for_type_with_placement(
                TiberiumTypeId(0),
                &mut overlay_grid,
                &overlay_registry,
                &tiberium_types,
                None,
                &BTreeSet::new(),
                Some(live_objects),
                &mut nodes,
                &mut rng,
                100,
                true,
                true,
                None,
                None,
                None,
            );
            (overlay_grid.cell(5, 5).overlay_data, stats, state)
        };

        // Seed 9 draws five attempts; a one-entry heap re-pops the
        // reinserted cell each time, exactly as the native loop does.
        let (open_data, open_stats, open_state) = run(&BTreeMap::new());
        let open_class = &open_state.native_tiberium_state().classes[0];
        assert_eq!(open_data, 8);
        assert_eq!(open_stats.grown_entries, 5);
        assert_eq!(open_stats.reinserted_entries, 5);
        assert_eq!(open_stats.spread_feed_calls, 5);
        assert_eq!(open_stats.spread_enqueued_entries, 1);
        assert!(open_class.spread_bitmap.contains(&(5, 5)));
        assert_eq!(open_class.spread.len(), 1);

        let tree_cells = BTreeMap::from([((5u16, 5u16), 77u64)]);
        let (tree_data, tree_stats, tree_state) = run(&tree_cells);
        let tree_class = &tree_state.native_tiberium_state().classes[0];
        assert_eq!(tree_data, 8, "the tree does not stop growth");
        assert_eq!(tree_stats.grown_entries, 5);
        assert_eq!(tree_stats.reinserted_entries, 5);
        assert_eq!(
            tree_stats.spread_feed_calls, 5,
            "AddToSpreadQueue is still called and rejects inside"
        );
        assert_eq!(tree_stats.spread_enqueued_entries, 0);
        assert!(tree_class.growth_bitmap.contains(&(5, 5)));
        assert!(tree_class.spread.is_empty());
        assert!(!tree_class.spread_bitmap.contains(&(5, 5)));
    }

    #[test]
    fn native_add_to_spread_queue_dedupes_and_rejects_without_rng() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        overlay_grid.place_overlay(1, 1, tib01, 3);
        let mut state = make_state(8, 8);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        let mut rng = SimRng::new(11);

        let first = state.add_native_spread_queue_cell(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            false,
            1,
            1,
            100,
            true,
            &mut rng,
        );
        let before_dedupe = rng.state();
        let second = state.add_native_spread_queue_cell(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            false,
            1,
            1,
            100,
            true,
            &mut rng,
        );
        let disabled = state.add_native_spread_queue_cell(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            false,
            1,
            1,
            100,
            false,
            &mut rng,
        );

        assert!(first.is_some());
        assert_eq!(second, None);
        assert_eq!(disabled, None);
        assert_eq!(rng.state(), before_dedupe);
        assert_eq!(state.native_tiberium_state().classes[0].spread.len(), 1);
        assert!(
            state.native_tiberium_state().classes[0]
                .spread_bitmap
                .contains(&(1, 1))
        );
    }

    fn block_all_neighbors_except(
        overlay_grid: &mut OverlayGrid,
        blocker_id: u8,
        source: (u16, u16),
        open: Option<(u16, u16)>,
    ) {
        for &(dx, dy) in &ADJACENT_OFFSETS {
            let cell = ((source.0 as i32 + dx) as u16, (source.1 as i32 + dy) as u16);
            if Some(cell) != open {
                overlay_grid.place_overlay(cell.0, cell.1, blocker_id, 0);
            }
        }
    }

    #[test]
    fn native_spread_processor_zero_target_entries_do_not_spend_budget() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let blocker = overlay_registry.id_for_name("GEM01").expect("GEM01");
        let mut overlay_grid = OverlayGrid::new(10, 10);
        overlay_grid.place_overlay(2, 2, tib01, 3);
        overlay_grid.place_overlay(7, 7, tib01, 3);
        block_all_neighbors_except(&mut overlay_grid, blocker, (2, 2), None);
        block_all_neighbors_except(&mut overlay_grid, blocker, (7, 7), Some((8, 7)));
        let mut state = make_state(10, 10);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        state.native_tiberium.classes[0]
            .spread
            .push(NativeTiberiumQueueEntry {
                rx: 2,
                ry: 2,
                priority_bits: 0.0f32.to_bits(),
            });
        state.native_tiberium.classes[0]
            .spread
            .push(NativeTiberiumQueueEntry {
                rx: 7,
                ry: 7,
                priority_bits: 1.0f32.to_bits(),
            });
        state.native_tiberium.classes[0]
            .spread_bitmap
            .insert((2, 2));
        state.native_tiberium.classes[0]
            .spread_bitmap
            .insert((7, 7));
        let mut nodes = BTreeMap::new();
        let mut rng = SimRng::new(12);

        let stats = state.process_native_spread_for_type_without_native_context(
            TiberiumTypeId(0),
            &mut overlay_grid,
            &overlay_registry,
            &tiberium_types,
            &mut nodes,
            None,
            None,
            &BTreeSet::new(),
            &mut rng,
            200,
            true,
        );

        assert_eq!(stats.processor_calls, 1);
        assert_eq!(stats.zero_target_entries, 1);
        assert_eq!(stats.spread_calls, 1);
        assert_eq!(stats.popped_entries, 2);
        assert!(
            !state.native_tiberium_state().classes[0]
                .spread_bitmap
                .contains(&(2, 2))
        );
    }

    /// `SpreadProcessor @ 0x00722440` never re-checks the popped entry's
    /// class, and `CellClass::SpreadTiberium @ 0x00483780` spreads whatever
    /// TiberiumClass the cell holds NOW. A Riparius entry left stale by a full
    /// removal and a Vinifera re-germination therefore spreads Vinifera from
    /// the Riparius processor, spending its budget and its `RandomRanged(0,7)`
    /// draw.
    #[test]
    fn native_spread_processor_spreads_the_cells_current_type_for_a_stale_entry() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let vinifera = TiberiumTypeId(2);
        let tib2_01 = overlay_registry.id_for_name("TIB2_01").expect("TIB2_01");
        let blocker = overlay_registry.id_for_name("GEM01").expect("GEM01");
        let vinifera_variants = overlay_registry
            .flat_tiberium_variant_ids(tiberium_types.get(vinifera).expect("Vinifera"))
            .expect("Vinifera variants");
        let mut overlay_grid = OverlayGrid::new(10, 10);
        overlay_grid.place_overlay(5, 5, tib2_01, 3);
        block_all_neighbors_except(&mut overlay_grid, blocker, (5, 5), Some((6, 5)));
        let mut state = make_state(10, 10);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        state.native_tiberium.classes[0]
            .spread
            .push(NativeTiberiumQueueEntry {
                rx: 5,
                ry: 5,
                priority_bits: 0.0f32.to_bits(),
            });
        state.native_tiberium.classes[0]
            .spread_bitmap
            .insert((5, 5));
        let mut nodes = BTreeMap::new();
        let mut rng = SimRng::new(12);
        let before = rng.state();

        let stats = state.process_native_spread_for_type_without_native_context(
            TiberiumTypeId(0),
            &mut overlay_grid,
            &overlay_registry,
            &tiberium_types,
            &mut nodes,
            None,
            None,
            &BTreeSet::new(),
            &mut rng,
            200,
            true,
        );

        assert_eq!(stats.popped_entries, 1);
        assert_eq!(stats.zero_target_entries, 0);
        assert_eq!(stats.spread_calls, 1);
        assert_ne!(rng.state(), before, "the spread drew its start direction");
        let placed = overlay_grid
            .cell(6, 5)
            .overlay_id
            .expect("spread landed on the open cell");
        assert!(
            vinifera_variants.contains(&placed),
            "the stale Riparius entry spread the cell's current Vinifera"
        );
        assert!(
            state.native_tiberium_state().classes[2]
                .growth_bitmap
                .contains(&(6, 5)),
            "the new cell joins Vinifera's growth queue"
        );
        assert!(
            !state.native_tiberium_state().classes[0]
                .growth_bitmap
                .contains(&(6, 5))
        );
    }

    #[test]
    fn gsi_04_09_scheduled_spread_uses_shared_new_placement_contract() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let blocker = overlay_registry.id_for_name("GEM01").expect("GEM01");
        let variants = overlay_registry
            .flat_tiberium_variant_ids(tiberium_types.get(TiberiumTypeId(0)).unwrap())
            .expect("Riparius variants");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        overlay_grid.place_overlay(3, 3, tib01, 3);
        block_all_neighbors_except(&mut overlay_grid, blocker, (3, 3), Some((4, 3)));
        let terrain = flat_clear_resolved_grid(8, 8);
        let rules_ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
        );
        let rules = RuleSet::from_ini(&rules_ini).expect("rules");
        let interner = StringInterner::default();
        let entities = EntityStore::new();
        let occupancy = OccupancyGrid::new();
        let terrain_object_cells = BTreeMap::new();
        let live_objects = TiberiumPlacementObjectContext::new(
            &entities,
            &occupancy,
            &rules,
            &interner,
            &terrain_object_cells,
        );
        let mut state = make_state(8, 8);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        state.native_tiberium.classes[0]
            .spread
            .push(NativeTiberiumQueueEntry {
                rx: 3,
                ry: 3,
                priority_bits: 0.0f32.to_bits(),
            });
        state.native_tiberium.classes[0]
            .spread_bitmap
            .insert((3, 3));
        let mut nodes = BTreeMap::new();
        let mut rng = SimRng::new(13);
        let mut expected_rng = rng.clone();
        expected_rng.next_u32(); // SpreadProcessor budget.
        expected_rng.next_range_u32(8); // Initial neighbor direction.
        let expected_overlay = variants[expected_rng.next_range_u32(12) as usize];
        let growth_priority_raw = expected_rng.next_u32();
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();

        let stats = state.tick_native_spread_driver(
            &mut overlay_grid,
            &overlay_registry,
            &tiberium_types,
            &mut nodes,
            None,
            Some(&terrain),
            &BTreeSet::new(),
            Some(live_objects),
            &mut rng,
            200,
            true,
            true,
            Some(&mut radar_dirty),
            Some(&mut radar_generation),
            Some(&mut tactical_dirty),
        );

        assert_eq!(stats.spread_calls, 1);
        assert_eq!(stats.reinserted_entries, 0);
        assert!(state.native_tiberium_state().classes[0].spread.is_empty());
        assert!(
            state.native_tiberium_state().classes[0]
                .spread_bitmap
                .contains(&(3, 3))
        );
        assert_eq!(
            overlay_grid.cell(4, 3).overlay_data,
            SPREAD_GERMINATION_DENSITY
        );
        assert_eq!(overlay_grid.cell(4, 3).overlay_id, Some(expected_overlay));
        assert!(
            nodes.is_empty(),
            "native spread writes only the authoritative overlay cell"
        );
        let class = &state.native_tiberium_state().classes[0];
        assert_eq!(class.growth.len(), 1);
        assert_eq!(
            (
                class.growth.heap_entry(0).unwrap().rx,
                class.growth.heap_entry(0).unwrap().ry
            ),
            (4, 3)
        );
        assert_eq!(
            class.growth.heap_entry(0).unwrap().priority_bits,
            growth_queue_priority(200, growth_priority_raw).to_bits(),
            "AddToGrowthQueue runs immediately after the zero-data overlay stamp"
        );
        assert!(class.growth_bitmap.contains(&(4, 3)));
        assert_eq!(radar_dirty, vec![(4, 3)]);
        assert_eq!(radar_generation, 1);
        assert_eq!(tactical_dirty, vec![(4, 3)]);
        assert_eq!(rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn gsi_04_09_scheduled_spread_rejects_visible_live_building_target() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let blocker = overlay_registry.id_for_name("GEM01").expect("GEM01");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        overlay_grid.place_overlay(3, 3, tib01, 3);
        block_all_neighbors_except(&mut overlay_grid, blocker, (3, 3), Some((4, 3)));
        let terrain = flat_clear_resolved_grid(8, 8);

        let rules_ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n\
             [BuildingTypes]\n0=GAPOWR\n[GAPOWR]\nStrength=100\n",
        );
        let rules = RuleSet::from_ini(&rules_ini).expect("rules");
        let mut interner = StringInterner::default();
        let mut entities = EntityStore::new();
        let mut building = GameEntity::test_default(1, "GAPOWR", "Neutral", 4, 3);
        building.category = EntityCategory::Structure;
        building.type_ref = interner.intern("GAPOWR");
        entities.insert(building);
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            4,
            3,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::AppendBuilding,
        );
        let terrain_object_cells = BTreeMap::new();
        let live_objects = TiberiumPlacementObjectContext::new(
            &entities,
            &occupancy,
            &rules,
            &interner,
            &terrain_object_cells,
        );

        let mut state = make_state(8, 8);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        state.native_tiberium.classes[0]
            .spread
            .push(NativeTiberiumQueueEntry {
                rx: 3,
                ry: 3,
                priority_bits: 0.0f32.to_bits(),
            });
        state.native_tiberium.classes[0]
            .spread_bitmap
            .insert((3, 3));
        let mut nodes = BTreeMap::new();
        let mut rng = SimRng::new(0x409);
        let mut expected_rng = rng.clone();
        expected_rng.next_u32(); // SpreadProcessor budget only; target count is zero.
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();

        let stats = state.tick_native_spread_driver(
            &mut overlay_grid,
            &overlay_registry,
            &tiberium_types,
            &mut nodes,
            None,
            Some(&terrain),
            &BTreeSet::new(),
            Some(live_objects),
            &mut rng,
            200,
            true,
            true,
            Some(&mut radar_dirty),
            Some(&mut radar_generation),
            Some(&mut tactical_dirty),
        );

        assert_eq!(stats.processor_calls, 1);
        assert_eq!(stats.zero_target_entries, 1);
        assert_eq!(stats.spread_calls, 0);
        assert_eq!(overlay_grid.cell(4, 3).overlay_id, None);
        assert!(state.native_tiberium_state().classes[0].growth.is_empty());
        assert!(nodes.is_empty());
        assert!(radar_dirty.is_empty());
        assert_eq!(radar_generation, 0);
        assert!(tactical_dirty.is_empty());
        assert_eq!(rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn gsi_04_09_scheduled_spread_rejects_outside_playfield_target() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let blocker = overlay_registry.id_for_name("GEM01").expect("GEM01");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        overlay_grid.place_overlay(3, 3, tib01, 3);
        block_all_neighbors_except(&mut overlay_grid, blocker, (3, 3), Some((4, 3)));
        let mut terrain = flat_clear_resolved_grid(8, 8);
        terrain.cell_mut(4, 3).unwrap().outside_playfield = true;

        let rules_ini = IniFile::from_str(
            "[InfantryTypes]\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n",
        );
        let rules = RuleSet::from_ini(&rules_ini).expect("rules");
        let interner = StringInterner::default();
        let entities = EntityStore::new();
        let occupancy = OccupancyGrid::new();
        let terrain_object_cells = BTreeMap::new();
        let live_objects = TiberiumPlacementObjectContext::new(
            &entities,
            &occupancy,
            &rules,
            &interner,
            &terrain_object_cells,
        );

        let mut state = make_state(8, 8);
        state.reset_native_tiberium_classes(tiberium_types.len(), 10);
        state.native_tiberium.classes[0]
            .spread
            .push(NativeTiberiumQueueEntry {
                rx: 3,
                ry: 3,
                priority_bits: 0.0f32.to_bits(),
            });
        state.native_tiberium.classes[0]
            .spread_bitmap
            .insert((3, 3));
        let mut nodes = BTreeMap::new();
        let mut rng = SimRng::new(0x409);
        let mut expected_rng = rng.clone();
        expected_rng.next_u32(); // SpreadProcessor budget only.
        let mut radar_dirty = Vec::new();
        let mut radar_generation = 0;
        let mut tactical_dirty = Vec::new();

        let stats = state.tick_native_spread_driver(
            &mut overlay_grid,
            &overlay_registry,
            &tiberium_types,
            &mut nodes,
            None,
            Some(&terrain),
            &BTreeSet::new(),
            Some(live_objects),
            &mut rng,
            200,
            true,
            true,
            Some(&mut radar_dirty),
            Some(&mut radar_generation),
            Some(&mut tactical_dirty),
        );

        assert_eq!(stats.processor_calls, 1);
        assert_eq!(stats.zero_target_entries, 1);
        assert_eq!(stats.spread_calls, 0);
        assert_eq!(overlay_grid.cell(4, 3).overlay_id, None);
        assert!(state.native_tiberium_state().classes[0].growth.is_empty());
        assert!(nodes.is_empty());
        assert!(radar_dirty.is_empty());
        assert_eq!(radar_generation, 0);
        assert!(tactical_dirty.is_empty());
        assert_eq!(rng.logical_state(), expected_rng.logical_state());
    }

    #[test]
    fn full_scan_cycle_resets_cursor() {
        let config = make_config(true, false);
        let mut state = make_state(5, 5); // 25 cells — very small
        let mut nodes = BTreeMap::new();
        nodes.insert((2, 2), ore_node(120));
        let mut rng = SimRng::new(42);

        // Run ticks until cursor wraps.
        let mut wrapped = false;
        for _ in 0..1000 {
            tick_ore_growth(&config, &mut state, &mut nodes, None, None, &mut rng);
            if state.scan_cursor == 0 {
                wrapped = true;
                break;
            }
        }

        assert!(wrapped, "Scan cursor should wrap to 0 after full cycle");
    }

    #[test]
    fn growth_rate_uses_the_legacy_fifteen_frame_scale() {
        let config = OreGrowthConfig {
            grows: true,
            spreads: false,
            tiberium_grows_flag: false,
            growth_rate_seconds: 1,
        };
        let mut state = make_state(10, 10);
        let mut nodes = BTreeMap::new();
        let mut rng = SimRng::new(42);

        tick_ore_growth(&config, &mut state, &mut nodes, None, None, &mut rng);

        assert_eq!(state.scan_cursor, 7, "ceil(100 cells / 15 frames)");
    }

    #[test]
    fn growth_rate_controls_scan_speed() {
        // Fast rate: 0.01 minutes → scans many cells per tick.
        let fast = make_config(true, false);
        let mut state_fast = make_state(100, 100); // 10000 cells
        let mut nodes_fast = BTreeMap::new();
        nodes_fast.insert((50, 50), ore_node(120));
        let mut rng = SimRng::new(42);

        tick_ore_growth(
            &fast,
            &mut state_fast,
            &mut nodes_fast,
            None,
            None,
            &mut rng,
        );
        let fast_progress = state_fast.scan_cursor;

        // Slow rate: 100 minutes → scans very few cells per tick.
        let slow = OreGrowthConfig {
            grows: true,
            spreads: false,
            tiberium_grows_flag: false,
            growth_rate_seconds: 6000, // 100 minutes
        };
        let mut state_slow = make_state(100, 100);
        let mut nodes_slow = BTreeMap::new();
        nodes_slow.insert((50, 50), ore_node(120));
        let mut rng2 = SimRng::new(42);

        tick_ore_growth(
            &slow,
            &mut state_slow,
            &mut nodes_slow,
            None,
            None,
            &mut rng2,
        );
        let slow_progress = state_slow.scan_cursor;

        assert!(
            fast_progress > slow_progress,
            "Fast rate ({}) should scan more cells per tick than slow rate ({})",
            fast_progress,
            slow_progress,
        );
    }

    #[test]
    fn spread_does_not_overwrite_existing_nodes() {
        let config = make_config(false, true);
        let mut state = make_state(10, 10);
        let mut nodes = BTreeMap::new();
        // Rich source at center.
        nodes.insert((5, 5), ore_node(SPREAD_THRESHOLD + 120));
        // Surround with existing gem nodes — spread should not overwrite them.
        for &(dx, dy) in &ADJACENT_OFFSETS {
            let nx = (5 + dx) as u16;
            let ny = (5 + dy) as u16;
            nodes.insert((nx, ny), gem_node(500));
        }
        let mut rng = SimRng::new(42);

        run_full_cycle(&config, &mut state, &mut nodes, &mut rng);

        // Should still have exactly 9 nodes (center + 8 neighbors).
        assert_eq!(nodes.len(), 9, "No new nodes should appear when surrounded");
        // All neighbors should still be gems.
        for &(dx, dy) in &ADJACENT_OFFSETS {
            let nx = (5 + dx) as u16;
            let ny = (5 + dy) as u16;
            let node = nodes.get(&(nx, ny)).expect("neighbor exists");
            assert_eq!(
                node.resource_type,
                ResourceType::Gem,
                "Neighbors should be unchanged gems"
            );
        }
    }

    fn entry(rx: u16, ry: u16, priority: f32) -> NativeTiberiumQueueEntry {
        NativeTiberiumQueueEntry {
            rx,
            ry,
            priority_bits: priority.to_bits(),
        }
    }

    /// `RebuildGrowthQueue @ 0x007233A0` sift-up stops on `parent <= new`, and
    /// the processors pop slot 1 then move the last slot to the root through
    /// `FloatMinHeap::SiftDown @ 0x005AD870`, which swaps only on a strictly
    /// smaller child: four equal-priority entries pop as first, last, third,
    /// second.
    #[test]
    fn native_queue_equal_priorities_pop_root_then_last_slot() {
        let mut queue = NativeTiberiumQueue::with_capacity(16);
        for (index, cell) in [(1u16, 1u16), (2, 1), (3, 1), (4, 1)]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                queue.push(entry(cell.0, cell.1, 0.0)),
                NativeQueueInsert::Heaped
            );
            assert_eq!(queue.len(), index + 1);
        }
        let popped: Vec<(u16, u16)> = std::iter::from_fn(|| queue.pop_root())
            .map(|entry| (entry.rx, entry.ry))
            .collect();
        assert_eq!(popped, vec![(1, 1), (4, 1), (3, 1), (2, 1)]);
        assert_eq!(queue.array_len(), 4, "the entry array is never compacted");
    }

    /// Distinct priorities pop ascending. Equal priorities resolve by heap
    /// position, not insertion order: pushing (1,1)@30, (2,1)@10, (3,1)@20,
    /// (4,1)@10, (5,1)@5 in that order leaves the heap
    /// `[(5,1)@5, (2,1)@10, (3,1)@20, (1,1)@30, (4,1)@10]`; popping (5,1)
    /// moves the last slot (4,1)@10 to the root, where the strict `<`
    /// sift-down keeps it above the equal (2,1)@10, so the later-pushed 10
    /// pops first.
    #[test]
    fn native_queue_orders_by_priority_with_heap_position_tie_breaks() {
        let mut queue = NativeTiberiumQueue::with_capacity(16);
        queue.push(entry(1, 1, 30.0));
        queue.push(entry(2, 1, 10.0));
        queue.push(entry(3, 1, 20.0));
        queue.push(entry(4, 1, 10.0));
        queue.push(entry(5, 1, 5.0));
        let heap_order: Vec<(u16, u16)> = queue.iter_heap().map(|e| (e.rx, e.ry)).collect();
        assert_eq!(heap_order, vec![(5, 1), (2, 1), (3, 1), (1, 1), (4, 1)]);
        let popped: Vec<(u16, u16)> = std::iter::from_fn(|| queue.pop_root())
            .map(|entry| (entry.rx, entry.ry))
            .collect();
        assert_eq!(popped, vec![(5, 1), (4, 1), (2, 1), (3, 1), (1, 1)]);
    }

    /// `count + 1 < capacity` guards the heap insert only: the entry still
    /// lands in the array and the counter still advances.
    #[test]
    fn native_queue_over_capacity_insert_appends_to_the_array_only() {
        let mut queue = NativeTiberiumQueue::with_capacity(3);
        assert_eq!(queue.push(entry(1, 1, 0.0)), NativeQueueInsert::Heaped);
        assert_eq!(queue.push(entry(2, 1, 0.0)), NativeQueueInsert::Heaped);
        assert_eq!(queue.push(entry(3, 1, 0.0)), NativeQueueInsert::ArrayOnly);
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.array_len(), 3);
        assert_eq!(
            native_tiberium_queue_capacity((100, 100)),
            20_800,
            "FUN_0042B1F0: (H + 4) * W * 2"
        );
    }

    /// `RebuildGrowthQueue` walks `CellIterator_Next @ 0x00578290` order:
    /// increasing `x + y`, and increasing `x` within one anti-diagonal.
    #[test]
    fn native_rebuild_seeds_in_cell_iterator_order() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        // Listed out of native order on purpose.
        for cell in [(7u16, 7u16), (5, 5), (6, 5), (5, 6), (4, 6)] {
            overlay_grid.place_overlay(cell.0, cell.1, tib01, 3);
        }
        let mut state = make_state(8, 8);
        state.rebuild_native_tiberium_queues_from_overlays(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            &BTreeSet::new(),
            true,
            true,
            77,
            (8, 8),
        );
        let class = &state.native_tiberium_state().classes[0];
        let heap_order: Vec<(u16, u16)> = class
            .growth
            .iter_heap()
            .map(|entry| (entry.rx, entry.ry))
            .collect();
        assert_eq!(
            heap_order,
            vec![(4, 6), (5, 5), (5, 6), (6, 5), (7, 7)],
            "equal priorities keep the CellIterator insertion order"
        );
        assert_eq!(
            class.growth.capacity(),
            native_tiberium_queue_capacity((8, 8))
        );
        assert_eq!(state.native_rect(), (8, 8));
    }

    /// `GrowthProcessor @ 0x00722F3C..0x00722F68`: `FILD count; FMUL [pct];
    /// _ftol` under the 53-bit chop control word. `50 * 0.06` chops to 2
    /// (clamped up to 5) where round-to-nearest would give 3; `150 * 0.06`
    /// chops to 8.
    #[test]
    fn native_processor_batch_chops_the_x87_product() {
        let pct = 0.06f64.to_bits();
        assert_eq!(native_processor_batch(50, pct, 5, 50), 5);
        assert_eq!(native_processor_batch(100, pct, 5, 50), 5);
        assert_eq!(native_processor_batch(150, pct, 5, 50), 8);
        assert_eq!(native_processor_batch(175, pct, 5, 50), 10);
        assert_eq!(native_processor_batch(1000, pct, 5, 50), 50);
        assert_eq!(native_processor_batch(1000, pct, 5, 25), 25);
        assert_eq!(native_processor_batch(0, pct, 5, 50), 5);
    }

    /// `CanGrow/CanSpreadTiberium` reject `pct < 1e-05`; the processors
    /// return on `pct <= 1e-05`.
    #[test]
    fn native_percentage_gates_compare_against_the_1e_minus_5_double() {
        let min = 0.00001f64.to_bits();
        assert_eq!(min, NATIVE_PERCENT_MIN_BITS);
        assert!(native_percentage_admits(min));
        assert!(!native_percentage_drives(min));
        assert!(!native_percentage_admits(0.000009f64.to_bits()));
        assert!(!native_percentage_admits(0.0f64.to_bits()));
        assert!(native_percentage_drives(0.00001001f64.to_bits()));
        assert!(native_percentage_drives(0.06f64.to_bits()));
    }

    /// `CanSpreadTiberium @ 0x00483690` returns `CellClass+0xE4 == 0`: a cell
    /// with a linked object never seeds or joins the spread queue, while the
    /// growth queue has no such gate.
    #[test]
    fn native_spread_admission_rejects_occupied_source_cells() {
        let (_ini, overlay_registry, tiberium_types) = tiberium_rebuild_fixture();
        let tib01 = overlay_registry.id_for_name("TIB01").expect("TIB01");
        let mut overlay_grid = OverlayGrid::new(8, 8);
        overlay_grid.place_overlay(5, 5, tib01, 4);
        overlay_grid.place_overlay(6, 5, tib01, 4);
        let occupied = BTreeSet::from([(5u16, 5u16)]);
        let mut state = make_state(8, 8);
        let stats = state.rebuild_native_tiberium_queues_from_overlays(
            &overlay_grid,
            &overlay_registry,
            &tiberium_types,
            None,
            &occupied,
            true,
            true,
            77,
            (8, 8),
        );
        assert_eq!(
            stats,
            NativeTiberiumRebuildStats {
                growth_entries: 2,
                spread_entries: 1,
            }
        );
        let class = &state.native_tiberium_state().classes[0];
        assert_eq!(class.spread_bitmap, BTreeSet::from([(6, 5)]));
        assert_eq!(class.growth_bitmap, BTreeSet::from([(5, 5), (6, 5)]));

        let mut rng = SimRng::new(4);
        let before = rng.state();
        assert_eq!(
            state.add_native_spread_queue_cell(
                &overlay_grid,
                &overlay_registry,
                &tiberium_types,
                None,
                true,
                5,
                5,
                100,
                true,
                &mut rng,
            ),
            None,
            "an occupied source is rejected before the priority draw"
        );
        assert_eq!(rng.state(), before);
    }
}
