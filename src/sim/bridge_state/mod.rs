//! Mutable bridge runtime state layered on top of resolved terrain.
//!
//! Bridges are modeled as terrain, not spawned entities. This module owns the
//! destroyable runtime state used by combat, layered pathing, and bridge-deck
//! fallout handling.
//!
//! The low-bridge overlay progression this note used to say was unwired has
//! since landed: `world::bridge_orchestrator` dispatches both the
//! `LowStateMachine` and `LowDirect` paths, writes overlay bytes through the
//! mutable state below, and applies the per-path BridgeStrength RNG gate. The
//! remaining recorded gaps for this system live on their own owners in
//! `world::bridge_orchestrator` — the VERA-chosen effect frame delay and
//! frame-count fallbacks, and the hut fallback starter/anchor heuristic.

//!
//! ## Tagged natives with no counterpart in this crate
//!
//! Four members of the Ghidra BRIDGE_HIGH / BRIDGE_LOW tag sets have no Rust
//! behaviour to compare against, each for a different and verified reason:
//!
//! - `MapClass::RecalcBridgeShroudFlags` 0x00578100 — **not bridge code.**
//!   Body decompiled 2026-08-19: two whole-map cell iterations that recompute
//!   shroud edge bitmasks through `Shroud_EdgeBitmask_Calculator` into
//!   `cell+0x120` and enqueue tactical redraws. No bridge field, flag or
//!   tileset appears in it, and the `+0x140` bit 0x20 its first pass gates on
//!   is not a bridge bit — `SetBridgeDirection`'s clear mask 0xFFFEE07F
//!   preserves it. The tag is wrong; removing it needs a Ghidra write.
//! - `FUN_0056A080` — bridge code with zero xrefs. Whether that is genuine
//!   dead code or a lost indirect reference is unresolved; either way nothing
//!   reaches it at runtime, so there is nothing to be faithful to.
//! - `MapClass::IncrementBridgeCounter` 0x00578AC0 — single caller
//!   `FUN_004F42F0`, itself untraced, so no trigger is established. No
//!   counterpart here.
//! - `ShipLocomotionClass::Compute_BridgeZOffset` 0x0069EBB0 — no CALL xrefs.
//!   Its Drive-side analogue `DriveLocomotionClass::ComputeBridgeZOffset`
//!   0x004AF4A0 is a one-shot constant initializer and is bound in
//!   `sim::movement::movement_bridge`; this one cannot be bound without a
//!   reference.
pub mod walker;
pub(crate) mod damage_dispatch;
mod damaged_variant;
mod record_scan;
mod zone_activation;
pub(crate) mod gap_restamp;
pub(crate) mod publication;
pub(crate) mod ramp_repair;
pub(crate) mod ordinary_repair;
pub(crate) mod repair_occupants;
pub(crate) mod rim;

use crate::map::resolved_terrain::ResolvedTerrainGrid;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use damaged_variant::extend_unique_cells;

/// Sentinel `overlay_byte` value meaning "no bridge overlay" (the original
/// engine's -1 / 0xFF). A cell carrying this byte renders empty and is treated
/// as non-walkable by `effective_render_state` / `is_bridge_walkable`. Also
/// written by the orchestrator's `update_adjacent_bridges` stub-reset path.
const OVERLAY_BYTE_NONE: u8 = 0xFF;

/// High-bridge theater slots used by the map-load bridge-record walk.
/// A negative entry is a deliberately unused slot.
const HIGH_BRIDGE_START_SUBTILE: [i32; 16] = [7, 7, -1, 7, 7, -1, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2];
const HIGH_BRIDGE_WALK_DIRECTION: [i32; 16] = [2, 2, -1, 4, 4, -1, 2, 2, 2, 2, 2, 4, 4, 4, 4, 4];
const HIGH_BRIDGE_END_SUBTILE: [i32; 16] = [-1, -1, 4, -1, -1, 2, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2];
// Static bridge axis/anchor vocabulary is map-owned (map::bridge_facts, F05);
// sim re-exports so runtime and serialized consumers keep their paths.
pub use crate::map::bridge_facts::{Axis, BridgeheadAnchorClass};

/// Per-cell damage state encoding all 18 state-byte values.
///
/// Body cells transition Healthy → Damaged → Destroyed under repeated
/// damage (per axis). Partial-collapse states are reached only via
/// bridgehead final-step cascade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum DamageState {
    /// Healthy body — `variant` carries the 6-frame jitter (0..=5 per axis,
    /// map-load-deterministic, never advances during gameplay).
    /// Maps to state byte 0–5 (NS) or 9–14 (EW).
    Healthy { variant: u8 },
    /// Damaged body — next hit collapses. State byte 6 (NS) / 15 (EW).
    Damaged,
    /// Partial collapse: ramp B already collapsed; this cell will fire
    /// CollapseA. State byte 7 (NS) / 17 (EW).
    PartialCollapseA,
    /// Partial collapse: ramp A already collapsed; this cell will fire
    /// CollapseB. State byte 8 (NS) / 16 (EW).
    PartialCollapseB,
    /// Fully destroyed.
    Destroyed,
}

impl DamageState {
    /// Encode to binary state byte (`CellClass+0x11E`).
    ///
    /// Per HIGH §3.1 / `apply_ramp_transition` docstring:
    /// - NS axis: Healthy{variant: 0..=5} → 0..=5; Damaged → 6;
    ///   PartialCollapseA → 7; PartialCollapseB → 8; Destroyed → 0.
    /// - EW axis: Healthy{variant: 0..=5} → 9..=14; Damaged → 0xF;
    ///   PartialCollapseA → 0x11; PartialCollapseB → 0x10; Destroyed → 0.
    ///
    /// **Note:** `Destroyed` always maps to byte 0, which is also the encoding
    /// for `Healthy{variant: 0}` initial state. Callers must use context
    /// (phase + prior state) to disambiguate after a `from_state_byte(0)` decode.
    /// `to_state_byte` is unambiguous (every variant has exactly one encoding).
    pub fn to_state_byte(self, axis: Axis) -> u8 {
        let ns_base: u8 = 0;
        let ew_base: u8 = 9;
        let base = match axis {
            Axis::NS => ns_base,
            Axis::EW => ew_base,
        };
        match self {
            DamageState::Healthy { variant } => base + variant.min(5),
            DamageState::Damaged => match axis {
                Axis::NS => 6,
                Axis::EW => 0xF,
            },
            DamageState::PartialCollapseA => match axis {
                Axis::NS => 7,
                Axis::EW => 0x11,
            },
            DamageState::PartialCollapseB => match axis {
                Axis::NS => 8,
                Axis::EW => 0x10,
            },
            DamageState::Destroyed => 0,
        }
    }

    /// Render-side state byte. Returns the *base* byte for `Healthy { variant }`
    /// (`0` for NS, `9` for EW) regardless of the stored variant. The renderer
    /// re-derives Latin-square jitter from cell `(x, y)` per the binary
    /// `DrawOverlay_Body` path (RE doc §3.3.1, ledger #4).
    #[cfg(test)]
    pub fn render_state_byte(self, axis: Axis) -> u8 {
        match self {
            DamageState::Healthy { .. } => match axis {
                Axis::NS => 0,
                Axis::EW => 9,
            },
            other => other.to_state_byte(axis),
        }
    }

    /// Decode from binary state byte. Returns `None` for bytes outside the
    /// defined ranges (NS: 0..=8; EW: 9..=0x11).
    ///
    /// **State 0 ambiguity:** byte 0 always decodes to `Healthy{variant: 0}`.
    /// Post-collapse `Destroyed` cells also have byte 0 in the binary, but the
    /// caller (body driver) writes `Destroyed` directly without round-tripping
    /// through `from_state_byte`. Test fixtures and snapshot consistency checks
    /// should not rely on this method to recover `Destroyed`.
    pub fn from_state_byte(byte: u8) -> Option<Self> {
        match byte {
            0..=5 => Some(DamageState::Healthy { variant: byte }),
            6 => Some(DamageState::Damaged),
            7 => Some(DamageState::PartialCollapseA),
            8 => Some(DamageState::PartialCollapseB),
            9..=14 => Some(DamageState::Healthy { variant: byte - 9 }),
            0xF => Some(DamageState::Damaged),
            0x10 => Some(DamageState::PartialCollapseB),
            0x11 => Some(DamageState::PartialCollapseA),
            _ => None,
        }
    }
}

/// Cell role within an `AnchorSpan`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BridgeCellRole {
    /// Anchor cell: primary cell of an anchor span; carries the canonical state byte.
    Anchor,
    /// Body cell: non-anchor structural cell; follows `anchor_span_id` for state-machine processing.
    Body,
    /// Bridgehead cell: ramp connection-piece off the body.
    Bridgehead,
    /// Tail cell (cell 5 of anchor pattern, walked in `–direction` from anchor).
    Tail,
}

/// Compass-direction enum.
///
/// Discriminant values must match the binary's table indices because
/// `set_bridge_direction` uses them to index into the offsets table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u8)]
pub enum Direction {
    N = 0,
    NE = 1,
    E = 2,
    SE = 3,
    S = 4,
    SW = 5,
    W = 6,
    NW = 7,
}

impl Direction {
    /// Cell-coord offset `(dx, dy)`. Signed because directions can decrement.
    pub const fn offset(self) -> (i32, i32) {
        match self {
            Direction::N => (0, -1),
            Direction::NE => (1, -1),
            Direction::E => (1, 0),
            Direction::SE => (1, 1),
            Direction::S => (0, 1),
            Direction::SW => (-1, 1),
            Direction::W => (-1, 0),
            Direction::NW => (-1, -1),
        }
    }

    /// `(self - 4) & 7` — opposite direction. Used by `set_bridge_direction`
    /// to compute cell 5 (walked in –direction from anchor).
    pub const fn opposite(self) -> Direction {
        match self {
            Direction::N => Direction::S,
            Direction::NE => Direction::SW,
            Direction::E => Direction::W,
            Direction::SE => Direction::NW,
            Direction::S => Direction::N,
            Direction::SW => Direction::NE,
            Direction::W => Direction::E,
            Direction::NW => Direction::SE,
        }
    }
}

/// `apply_ramp_transition` phase. Maps to one of the 16 ramp transition helpers
/// (NS/EW × DamageA/DamageB/CollapseA/CollapseB).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    DamageA,
    DamageB,
    CollapseA,
    CollapseB,
}

/// First-class anchor-span representation. One span per anchor cell.
///
/// Walker pattern: up to 6 cells (anchor + 3 walked +dir + 1 walked –dir +
/// optional fixed-offset cell when direction == W). Per-cell action
/// (BlowUpBridge vs flag-only) is determined by slot index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AnchorSpan {
    /// Stable ID, matches `BridgeRuntimeCell.anchor_span_id`.
    pub id: u16,
    /// The anchor cell. Slot 0.
    pub anchor: (u16, u16),
    /// All cells in walker order:
    /// `[0]=anchor, [1..=3]=+direction × 1/2/3, [4]=-direction × 1, [5]=fixed-offset (only when direction == W)`.
    /// `None` for unused slots when the optional fixed-offset cell isn't present.
    pub cells: [Option<(u16, u16)>; 6],
    /// Body axis (NS or EW). Determined from `bridge_layer.direction`.
    pub axis: Axis,
    /// Walk direction (compass index 0–7). Used to compute walked cells.
    pub direction: Direction,
    /// Mirror of anchor cell's damage state. Convenience for queries.
    pub damage_state: DamageState,
    /// Group ID (existing `BridgeRuntimeState::group_cells`) — preserved for
    /// connectivity queries.
    pub bridge_group_id: u16,
}

impl AnchorSpan {
    /// Cells receiving `BlowUpBridge` on destruction path: slots 0, 1, 2, 4.
    /// Slot 3 (cell 4) and slot 5 (cell 6) are flag-only.
    pub const BLOW_UP_SLOTS: [usize; 4] = [0, 1, 2, 4];

    /// Iterate `(slot, cell)` for present cells (skips `None`).
    pub fn iter_cells(&self) -> impl Iterator<Item = (usize, (u16, u16))> + '_ {
        self.cells
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.map(|cell| (i, cell)))
    }

    /// Cells that get `BlowUpBridge` on destruction. Skips slots 3 and 5
    /// which are flag-only.
    pub fn blow_up_cells(&self) -> impl Iterator<Item = (u16, u16)> + '_ {
        Self::BLOW_UP_SLOTS
            .iter()
            .filter_map(|&slot| self.cells[slot])
    }
}

/// One area's bridge-damage input. Combat calls the world orchestrator
/// synchronously after that area's receivers; it is never queued across the
/// bullet's animation/cluster tail. The orchestrator owns native admission,
/// strength RNG, driver retries, publication and target release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BridgeDamageEvent {
    pub rx: u16,
    pub ry: u16,
    pub damage: i32,
    /// Interned warhead ID — used for IonCannon identity check (combat
    /// boundary pre-resolves `is_ion_cannon`) and for InfDeath selection in
    /// the C4Warhead ground-kill cascade.
    pub warhead_ref: crate::sim::intern::InternedId,
    /// Pre-resolved at combat: `warhead_ref == rules.ion_cannon_warhead_id()`.
    /// Bypasses the BridgeStrength RNG gate; enables the 3-retry loop on
    /// state-machine paths only (direct-overlay paths are single-shot).
    pub is_ion_cannon: bool,
    /// Exact signed world height in leptons, retained from the detonation.
    /// Structural state-machine paths admit (ground + 208, ground + 520];
    /// nonstructural tiles and direct-overlay paths have no height gate.
    pub impact_z_leptons: i32,
}

/// Path discriminator for the bridge-damage 4-path dispatcher.
/// Order matches the binary's outer-dispatch evaluation order
/// (HighSM → LowSM → LowDirect → HighDirect).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchPath {
    /// HIGH state-machine: anchor / body / tail / bridgehead cell whose
    /// overlay byte has already transitioned out of the raw body range.
    /// Includes Z-height range gate.
    HighStateMachine,
    /// LOW state-machine: same shape as `HighStateMachine` for low bridges.
    /// Includes Z-height range gate.
    LowStateMachine,
    /// LOW direct-overlay: `cell.overlay_byte ∈ [0x4A..=0x63]`. Single-shot;
    /// no Z-gate.
    LowDirect,
    /// HIGH direct-overlay: `cell.overlay_byte ∈ [0xCD..=0xE6]`. Single-shot;
    /// no Z-gate.
    HighDirect,
}

/// `ApplyDamageToCell` 0x00587180 dispatch bands.
///
/// The driver tests `(0x49 < overlay) && (overlay < 100)` for the low walker
/// and `(0xCC < overlay) && (overlay < 0xE7)` for the high one, so 0x64/0x65
/// and 0xE7/0xE8 are NOT routed to a walker from here — they fall through to
/// the tileset-family branch.
///
/// That is deliberately narrower than the bands `DestroyBridge_Low` 0x0057BAA0
/// and `DestroyBridge_High` 0x0057CCF0 accept once they are already running:
/// their own axis classes union to 0x4A..=0x65 and 0xCD..=0xE8, and their
/// neighbour probes test `overlay < 0x4A || 0x65 < overlay` and
/// `overlay < 0xCD || 0xE8 < overlay`. Both bands are real; see
/// `BridgeRuntimeState::is_low_destroy_overlay` for the wider pair.
const fn is_low_dispatch_overlay(overlay: u8) -> bool {
    0x49 < overlay && overlay < 100
}

/// High twin of [`is_low_dispatch_overlay`]. See its doc comment.
const fn is_high_dispatch_overlay(overlay: u8) -> bool {
    0xCC < overlay && overlay < 0xE7
}

impl DispatchPath {
    /// State-machine paths support the IonCannon 3-retry loop. Direct-overlay
    /// paths are single-shot regardless of warhead.
    pub fn is_state_machine(self) -> bool {
        matches!(
            self,
            DispatchPath::HighStateMachine | DispatchPath::LowStateMachine
        )
    }
}

/// Outcome of one `body_cell_advance_state` invocation. Mirrors the return
/// codes of binary `ProcessBridgeDamageStateMachine_High @ 0x576BA0` body
/// branch (0 = absorbed, 1 = collapse), with structured fallout for the
/// orchestrator to dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateOutcome {
    /// Damage absorbed — anchor advanced from `Healthy` to `Damaged`. Bridge
    /// still passable. Renderer should redraw.
    Absorbed {
        /// Cells whose `ToggleBridgePavement @ 0x0056E990` equivalent changed
        /// the current TMP damage selector. Native marks each cell before its
        /// direction-0..7 recursion, so order is presentation-significant.
        damaged_variant_cells: Vec<(u16, u16)>,
    },
    /// Anchor collapsed — `damage_state` became `Destroyed`. Cascade actions
    /// for orchestrator follow.
    Collapsed {
        /// Boolean returned by the underlying gamemd bridge damage helper.
        /// Usually true for collapse, except low bridgehead slot `+3`: that
        /// branch performs collapse side effects but returns false.
        binary_success: bool,
        /// Cells whose `damage_state` was set to `Destroyed` in this call
        /// (typically just the anchor; perpendicular targets that hit
        /// collapse-final via `update_ramp_perpendicular` also appear here).
        destroyed_cells: Vec<(u16, u16)>,
        /// `BlowUpBridge` cascade actions emitted by `set_bridge_direction`.
        /// Orchestrator dispatches these (kill ground occupants, Limbo
        /// bridge-deck, spawn debris).
        set_bridge_direction: crate::sim::bridge_specs::SetBridgeDirectionResult,
        /// Exact native setter call order for the represented 0x1180 subset.
        /// Perpendicular ramp-helper setters precede the parent span setter;
        /// bridgehead collapse can carry a helper setter even when the legacy
        /// action result above has no setter header. Execution-only: snapshot
        /// persistence stores final real-cell values, never this transcript.
        setter_transcript: Vec<crate::map::bridge_facts::BridgeFlagStamp>,
        /// Cells where `UpdateAdjacentBridges_High` should run for rim
        /// re-evaluation. Orchestrator (Phase F Task 27) runs the actual
        /// rim helper.
        adjacent_bridges_dirty: Vec<(u16, u16)>,
        /// Whether the zone graph needs rebuild
        /// (`MapClass::InvalidateBridgeZones` @ `0x0056DAE0` →
        /// `MapClass::RebuildZoneConnectivity` @ `0x0056C510`). Orchestrator
        /// dispatches.
        zones_dirty: bool,
        /// Cells whose visible terrain changed and must be marked dirty on the
        /// minimap. On a collapse this is the collapsed triple PLUS every
        /// cascade-leaf cell touched — including intermediate `Damaged`
        /// perpendicular neighbors, not only the finals — so a partially-
        /// damaged neighbor's minimap variant does not go stale. The
        /// orchestrator feeds these into `mark_radar_terrain_dirty_cells`,
        /// the same channel the engineer-repair path uses.
        radar_cells: Vec<(u16, u16)>,
        /// Ordered cells changed by the ramp helpers' nested
        /// `ToggleBridgePavement @ 0x0056E990` calls. Kept separate so the
        /// existing collapse dirty-set ordering remains unchanged.
        damaged_variant_cells: Vec<(u16, u16)>,
    },
    /// Cell is not a body-bridge cell, anchor span lookup failed, or anchor
    /// is already `Destroyed`. No-op.
    NoChange,
}

impl StateOutcome {
    /// Whether this invocation produced any local state/cascade side effect.
    pub fn has_effect(&self) -> bool {
        !matches!(self, StateOutcome::NoChange)
    }

    /// Boolean success value returned by gamemd's bridge damage helper.
    ///
    /// This is separate from `has_effect`: low bridgehead slot `+3` performs
    /// collapse side effects while returning false.
    pub fn apply_damage_success(&self) -> bool {
        matches!(
            self,
            StateOutcome::Collapsed {
                binary_success: true,
                ..
            }
        )
    }

    pub fn damaged_variant_cells(&self) -> &[(u16, u16)] {
        match self {
            StateOutcome::Absorbed {
                damaged_variant_cells,
            }
            | StateOutcome::Collapsed {
                damaged_variant_cells,
                ..
            } => damaged_variant_cells,
            StateOutcome::NoChange => &[],
        }
    }

    pub fn setter_transcript(&self) -> &[crate::map::bridge_facts::BridgeFlagStamp] {
        match self {
            StateOutcome::Collapsed {
                setter_transcript, ..
            } => setter_transcript,
            StateOutcome::Absorbed { .. } | StateOutcome::NoChange => &[],
        }
    }
}

/// Outcome of a single `body_cell_repair_state` call. Carries the
/// side-effects the caller must fire AFTER state mutation.
///
/// Side-effect gating mirrors the original engine's repair-walker semantics:
///   - `zones_dirty`: rebuild PathGrid + zone grid. Set only when a
///     **main-deck damaged or destroyed** cell was repaired —
///     bridgehead-only repairs do NOT trigger zones rebuild.
///   - `radar_cells`: mark these cells dirty in the minimap. Includes exact
///     damage-variant clears plus cells restored **from `Destroyed`**.
///   - `repaired_cells`: total mutated cell count for caller's
///     `bridge_state_changed` decision and metrics.
#[derive(Debug, Clone, Default)]
pub struct RepairOutcome {
    pub zones_dirty: bool,
    pub radar_cells: Vec<(u16, u16)>,
    pub repaired_cells: u32,
}

/// One ordered CellClass bridge-overlay projection operation.
///
/// Native low-bridge walkers write a complete three-cell identity strip before
/// calling `RecalcAttributes` on any member. A flat write-only queue cannot
/// represent that boundary, so the transient stream carries both operations.
/// Repeated writes and recalculations are retained verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BridgeOverlayProjectionOp {
    Write { rx: u16, ry: u16, overlay_byte: u8 },
    Recalc { rx: u16, ry: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BridgeRuntimeCell {
    pub deck_present: bool,
    pub destroyable: bool,
    pub deck_level: u8,
    pub bridge_group_id: Option<u16>,

    /// Per-cell damage state. Drives state-machine progression and renderer
    /// display-tile selection. Replaces the old `destroyed: bool`.
    pub damage_state: DamageState,

    /// Bridge body axis (NS or EW). `None` for cells where axis is not
    /// meaningful (orphan body cells, edge cases). Filled by Task 7 anchor walker.
    pub axis: Option<Axis>,

    /// Cell role within its anchor span. Drives state-machine branch dispatch.
    /// Filled by Task 7 anchor walker.
    pub role: BridgeCellRole,

    /// Stable ID of containing `AnchorSpan` (for body cells); `None` for
    /// bridgehead cells.
    pub anchor_span_id: Option<u16>,

    /// Per-cell visible overlay byte (mirrors binary `CellClass+0x44`).
    /// Populated at map-load from `ResolvedTerrainCell.bridge_layer.overlay_id`;
    /// mutated at runtime by the body-cell state machine and (future) perpendicular
    /// overlay-write branch. Renderer queries this to pick the visible tile.
    pub overlay_byte: u8,

    /// Anchor tile-class mirror written by the bridgehead state machine when
    /// damage lands on a bridgehead-class cell. Carries the visual variant
    /// of the anchor (or neighbor bridgehead progressed via `DamageB`).
    /// Defaults to `Variant0` at map load. The renderer follow-up will read
    /// this to pick the anchor's TMP tile variant; G3 lands the sim-side
    /// write only.
    #[serde(default)]
    pub bridgehead_anchor_class: BridgeheadAnchorClass,
}

/// Binary bridge record kind (`BridgeRecord+0x0C`).
///
/// Verified against `MapClass__ComputeBridgeZones @ 0x0056D6E0`:
/// high bridges write `0`, accepted Tube endpoints write `1`. The historical
/// `Low` Rust name is retained for compatibility. `MapClass__FindBridgeRecord`
/// skips non-zero kinds, so callers must choose high-only vs all-record use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum BridgeRecordKind {
    High,
    Low,
}

impl Default for BridgeRecordKind {
    fn default() -> Self {
        Self::High
    }
}

/// A bridge's map-load endpoint pair for zone connectivity.
/// High records retain the matching theater tiles found by the map-load walk;
/// low records retain their tube-span endpoints.
/// Mirrors gamemd.exe BridgeRecord at MapClass+0x54 (16 bytes each).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BridgeEndpointRecord {
    /// First endpoint discovered by the record builder.
    pub endpoint_a: (u16, u16),
    /// Matching far endpoint discovered by the record builder.
    pub endpoint_b: (u16, u16),
    /// Which bridge group this record belongs to.
    pub group_id: u16,
    /// Whether the bridge is traversable (false = destroyed).
    pub active: bool,
    /// High vs low bridge record kind.
    #[serde(default)]
    pub bridge_kind: BridgeRecordKind,
}

impl BridgeEndpointRecord {
    pub fn is_high(&self) -> bool {
        self.bridge_kind == BridgeRecordKind::High
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BridgeRuntimeState {
    width: u16,
    height: u16,
    cells: Vec<Option<BridgeRuntimeCell>>,
    group_cells: BTreeMap<u16, Vec<(u16, u16)>>,
    /// Strength constant from `[CombatDamage] BridgeStrength=` (default 1000).
    /// Used by the dispatcher's per-path BridgeStrength RNG gate.
    bridge_strength: i32,
    endpoint_records: Vec<BridgeEndpointRecord>,
    /// Source Map Size paired with this derived record set. Only construction
    /// writes it; this receipt is not an independently editable map authority.
    /// Native base-zone endpoint lookup uses stride W+H+1, not terrain width.
    #[serde(default)]
    native_zone_source_size: Option<(i32, i32)>,
    /// First-class anchor spans (one per anchor cell). Replaces emergent
    /// flag-bit detection.
    anchor_spans: BTreeMap<u16, AnchorSpan>,
    /// Active `SpecialFlags::DestroyableBridges` bit. Read by the weapon AoE
    /// bridge-damage outer gate.
    bridge_destroyable_flag: bool,
    /// Ordered bridge-overlay identity writes waiting for the world-owned
    /// CellClass/terrain projection. Runtime cache only: snapshots serialize
    /// the resulting bridge and OverlayGrid identities, never this queue.
    #[serde(skip, default)]
    overlay_projection_ops: Vec<BridgeOverlayProjectionOp>,
}

impl BridgeRuntimeState {
    /// Production entry with the raw Map Size authority, not backing dimensions.
    pub(crate) fn from_resolved_terrain_with_map_size(
        terrain: &ResolvedTerrainGrid,
        destroyable: bool,
        bridge_strength: i32,
        size: (i32, i32),
    ) -> Self {
        Self::build_from_terrain(terrain, destroyable, bridge_strength, Some(size))
    }

    #[cfg(test)]
    pub fn from_resolved_terrain(
        terrain: &ResolvedTerrainGrid,
        destroyable: bool,
        bridge_strength: i32,
    ) -> Self {
        Self::build_from_terrain(terrain, destroyable, bridge_strength, None)
    }

    fn build_from_terrain(
        terrain: &ResolvedTerrainGrid,
        destroyable: bool,
        bridge_strength: i32,
        size: Option<(i32, i32)>,
    ) -> Self {
        let width = terrain.width();
        let height = terrain.height();
        let mut cells = vec![None; width as usize * height as usize];
        let mut group_cells: BTreeMap<u16, Vec<(u16, u16)>> = BTreeMap::new();
        let mut anchor_spans: BTreeMap<u16, AnchorSpan> = BTreeMap::new();
        let mut visited = vec![false; cells.len()];
        let mut next_group_id: u16 = 1;
        let mut next_span_id: u16 = 1;

        // Pass 1: BFS-group structural bridge cells. High bridges use the
        // authoritative SetBridgeDirection-equivalent facts; low bridges and
        // existing test fixtures keep the legacy deck fallback.
        for cell in terrain.iter() {
            let Some(index) = index_of(width, height, cell.rx, cell.ry) else {
                continue;
            };
            if visited[index] || !resolved_cell_has_runtime_deck(cell) {
                continue;
            }
            let group_id = next_group_id;
            next_group_id = next_group_id.saturating_add(1);
            let mut queue = VecDeque::from([(cell.rx, cell.ry)]);
            let mut members = Vec::new();
            while let Some((rx, ry)) = queue.pop_front() {
                let Some(idx) = index_of(width, height, rx, ry) else {
                    continue;
                };
                if visited[idx] {
                    continue;
                }
                let Some(resolved) = terrain.cell(rx, ry) else {
                    continue;
                };
                if !resolved_cell_has_runtime_deck(resolved) {
                    continue;
                }
                visited[idx] = true;
                members.push((rx, ry));
                cells[idx] = Some(BridgeRuntimeCell {
                    deck_present: true,
                    destroyable,
                    deck_level: resolved.bridge_deck_level,
                    bridge_group_id: Some(group_id),
                    damage_state: initial_bridge_damage_state(resolved),
                    axis: bridge_fact_axis(resolved)
                        .or_else(|| bridge_layer_to_axis(resolved.bridge_layer.as_ref())),
                    role: BridgeCellRole::Body, // overwritten in pass 2
                    anchor_span_id: None,
                    overlay_byte: resolved
                        .bridge_facts
                        .overlay_id
                        .or_else(|| resolved.bridge_layer.as_ref().map(|bl| bl.overlay_id))
                        .unwrap_or(0),
                    bridgehead_anchor_class: resolved
                        .bridgehead_anchor_class_at_load
                        .unwrap_or(BridgeheadAnchorClass::Variant0),
                });
                for (nx, ny) in cardinal_neighbors(rx, ry, width, height) {
                    if let Some(neighbor) = terrain.cell(nx, ny) {
                        if resolved_cell_has_runtime_deck(neighbor) {
                            queue.push_back((nx, ny));
                        }
                    }
                }
            }
            if !members.is_empty() {
                group_cells.insert(group_id, members);
            }
        }

        // Pass 2: walk anchor patterns. High bridges trust the 0x80 anchor
        // fact. Low/legacy bridges keep the previous bridge_layer fallback.
        for (&group_id, members) in &group_cells {
            for &(rx, ry) in members {
                let Some(resolved) = terrain.cell(rx, ry) else {
                    continue;
                };
                let fact_anchor = resolved.bridge_facts.is_anchor_self();
                let legacy_anchor = resolved.bridge_facts.family
                    == crate::map::bridge_facts::BridgeStampFamily::None
                    && resolved
                        .bridge_layer
                        .as_ref()
                        .is_some_and(|bl| is_anchor_overlay(bl.overlay_id));
                if !fact_anchor && !legacy_anchor {
                    continue;
                }
                let (axis, direction) = if fact_anchor {
                    let stamp_direction = resolved.bridge_facts.direction.unwrap_or(0);
                    (
                        bridge_stamp_direction_to_axis(stamp_direction),
                        bridge_stamp_direction_to_direction(stamp_direction),
                    )
                } else {
                    let bl = resolved
                        .bridge_layer
                        .as_ref()
                        .expect("legacy anchor checked bridge_layer above");
                    let axis = bridge_direction_to_axis(bl.direction);
                    (axis, anchor_walk_direction(axis))
                };
                let span_id = next_span_id;
                next_span_id = next_span_id.saturating_add(1);
                let span = walk_anchor_pattern(
                    span_id,
                    (rx, ry),
                    axis,
                    direction,
                    group_id,
                    width,
                    height,
                );
                // Tag each cell in span.
                for (slot, cell_pos) in span.iter_cells() {
                    if let Some(idx) = index_of(width, height, cell_pos.0, cell_pos.1) {
                        if let Some(c) = cells[idx].as_mut() {
                            c.role = if slot == 0 {
                                BridgeCellRole::Anchor
                            } else if slot == 4 {
                                BridgeCellRole::Tail
                            } else {
                                BridgeCellRole::Body
                            };
                            c.anchor_span_id = Some(span_id);
                            c.axis = Some(axis);
                        }
                    }
                }
                anchor_spans.insert(span_id, span);
            }
        }

        // Pass 3: classify bridgehead cells (have bridge_layer but not
        // anchor-overlay; not part of an AnchorSpan).
        for cell in terrain.iter() {
            let Some(idx) = index_of(width, height, cell.rx, cell.ry) else {
                continue;
            };
            let Some(resolved) = terrain.cell(cell.rx, cell.ry) else {
                continue;
            };
            let Some(bl) = resolved.bridge_layer.as_ref() else {
                continue;
            };
            if is_anchor_overlay(bl.overlay_id) {
                continue;
            }
            // Bridgehead cells: ramp/connection cells. May not have deck_present
            // if treated purely as ground transition. Mark role only when
            // a BridgeRuntimeCell already exists.
            if let Some(c) = cells[idx].as_mut() {
                c.role = BridgeCellRole::Bridgehead;
                c.anchor_span_id = None;
                c.axis = Some(bridge_direction_to_axis(bl.direction));
            }
        }

        // Pass 4: register bridgehead cells. ResolvedTerrainCell sets
        // bridge_walkable=true and has_bridge_deck=false at every bridgehead
        // (see resolved_terrain.rs bridgehead pass). Bridgeheads are NOT
        // created in pass 1 (no deck) and NOT touched by pass 3 (no
        // bridge_layer). Without this pass the rebuild silently flips
        // PathCell.bridge_walkable to false on every rebuild_dynamic_path_grid.
        //
        // Contract: deck_present=true permanently, damage_state=Healthy
        // permanently, bridge_group_id=None, anchor_span_id=None, axis=None,
        // overlay_byte=0. The dispatcher (path_matches_cell HighSM/LowSM)
        // rejects Bridgehead+axis.is_none() so no damage-event RNG fires on
        // these cells. Pass-3 bridgeheads (axis=Some) stay in the allowed set.
        for cell in terrain.iter() {
            if !cell.bridge_walkable || cell.has_bridge_deck {
                continue;
            }
            let Some(idx) = index_of(width, height, cell.rx, cell.ry) else {
                continue;
            };
            if cells[idx].is_some() {
                // Defensive: pass 1 already registered a cell here. The
                // condition (bw && !has_deck) should be mutually exclusive
                // with pass 1's has_deck, so this branch is unreachable
                // unless the resolved terrain is internally inconsistent.
                continue;
            }
            cells[idx] = Some(BridgeRuntimeCell {
                deck_present: true,
                destroyable,
                deck_level: cell.bridge_deck_level,
                bridge_group_id: None,
                damage_state: DamageState::Healthy { variant: 0 },
                axis: None,
                role: BridgeCellRole::Bridgehead,
                anchor_span_id: None,
                overlay_byte: 0,
                bridgehead_anchor_class: BridgeheadAnchorClass::Variant0,
            });
        }

        let endpoint_records = record_scan::compute_bridge_endpoints(terrain, size, &cells);

        Self {
            width,
            height,
            cells,
            group_cells,
            bridge_strength,
            endpoint_records,
            native_zone_source_size: size,
            anchor_spans,
            bridge_destroyable_flag: destroyable,
            overlay_projection_ops: Vec::new(),
        }
    }

    /// Look up an anchor span by ID.
    pub fn anchor_span(&self, id: u16) -> Option<&AnchorSpan> {
        self.anchor_spans.get(&id)
    }

    /// Mutable counterpart to `anchor_span`. Used by `body_cell_repair_state`
    /// to sync the span's mirror `damage_state` field after per-cell repair.
    pub fn anchor_span_mut(&mut self, id: u16) -> Option<&mut AnchorSpan> {
        self.anchor_spans.get_mut(&id)
    }

    /// All anchor spans, sorted by ID (BTreeMap iteration order).
    pub fn anchor_spans(&self) -> &BTreeMap<u16, AnchorSpan> {
        &self.anchor_spans
    }

    pub fn cell(&self, rx: u16, ry: u16) -> Option<&BridgeRuntimeCell> {
        index_of(self.width, self.height, rx, ry)
            .and_then(|idx| self.cells.get(idx))
            .and_then(|cell| cell.as_ref())
    }

    /// Mutable cell access. Returns `None` if `(rx, ry)` is out of bounds or
    /// the cell is not a bridge runtime cell.
    pub fn cell_mut(&mut self, rx: u16, ry: u16) -> Option<&mut BridgeRuntimeCell> {
        index_of(self.width, self.height, rx, ry)
            .and_then(move |idx| self.cells.get_mut(idx))
            .and_then(|cell| cell.as_mut())
    }

    /// Write one live bridge-overlay byte as a complete one-cell native
    /// transaction. Multi-cell walkers use the deferred form below so they can
    /// place every identity before queueing their ordered recalculations.
    pub(crate) fn write_overlay_byte(&mut self, rx: u16, ry: u16, overlay_byte: u8) -> bool {
        let changed = self.write_overlay_byte_deferred_recalc(rx, ry, overlay_byte);
        if self.cell(rx, ry).is_some() {
            self.queue_overlay_recalc(rx, ry);
        }
        changed
    }

    pub(crate) fn write_overlay_byte_deferred_recalc(
        &mut self,
        rx: u16,
        ry: u16,
        overlay_byte: u8,
    ) -> bool {
        let changed = match self.cell_mut(rx, ry) {
            Some(cell) => {
                let changed = cell.overlay_byte != overlay_byte;
                cell.overlay_byte = overlay_byte;
                changed
            }
            None => return false,
        };
        self.overlay_projection_ops
            .push(BridgeOverlayProjectionOp::Write {
                rx,
                ry,
                overlay_byte,
            });
        changed
    }

    pub(crate) fn queue_overlay_recalc(&mut self, rx: u16, ry: u16) {
        if self.cell(rx, ry).is_some() {
            self.overlay_projection_ops
                .push(BridgeOverlayProjectionOp::Recalc { rx, ry });
        }
    }

    pub(crate) fn take_overlay_projection_ops(&mut self) -> Vec<BridgeOverlayProjectionOp> {
        std::mem::take(&mut self.overlay_projection_ops)
    }

    /// Map width in cells. Needed by walker code in the `walker` submodule
    /// (Rust privacy: child modules can't read parent's private fields
    /// without a getter or `pub(super)`).
    pub fn width(&self) -> u16 {
        self.width
    }

    /// Map height in cells. See `width()` rationale.
    pub fn height(&self) -> u16 {
        self.height
    }

    /// `[CombatDamage] BridgeStrength=` value used by the per-path RNG gate
    /// in the bridge-damage dispatcher. Read-only; set at construction.
    pub fn bridge_strength(&self) -> i32 {
        self.bridge_strength
    }

    /// Whether the global `SpecialFlags::DestroyableBridges` is set. Outer
    /// gate of the bridge-damage dispatcher; if false, bridges are immune.
    pub fn is_destroyable(&self) -> bool {
        self.bridge_destroyable_flag
    }

    /// Overlay-first inner dispatcher for a state-machine block (binary
    /// `ApplyDamageToCell @ 0x00587180`, the driver of `Apply_area_damage`
    /// blocks A/B). The visible overlay byte is checked FIRST: a cell already
    /// in a destroy band routes straight to the matching direct walker; only
    /// overlay-miss cells reach the damage state machine. `is_high` selects the
    /// SM family for the overlay-miss fallback (bridgehead vs body branch is
    /// then chosen by the cell's role).
    ///
    /// Both bands are tested regardless of `is_high` — the binary's overlay
    /// short-circuit is family-agnostic; family only decides the SM fallback.
    pub(crate) fn apply_damage_to_cell(
        &mut self,
        rx: u16,
        ry: u16,
        is_high: bool,
        terrain: &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
    ) -> StateOutcome {
        let overlay = self.cell(rx, ry).map(|c| c.overlay_byte);
        match overlay {
            Some(o) if is_high_dispatch_overlay(o) => self.destroy_bridge_high(rx, ry, terrain),
            Some(o) if is_low_dispatch_overlay(o) => self.destroy_bridge_low(rx, ry, terrain),
            _ => match self.cell(rx, ry).map(|c| c.role) {
                Some(BridgeCellRole::Bridgehead) => {
                    self.bridgehead_advance_state(rx, ry, is_high, terrain)
                }
                _ => self.body_cell_advance_state(rx, ry, is_high, terrain),
            },
        }
    }

    /// Test-only: insert a `BridgeRuntimeCell` at `(rx, ry)`, growing the
    /// internal `cells` Vec and `width`/`height` to fit if needed. Used by
    /// unit tests that need precise control over cell placement and state
    /// without going through `from_resolved_terrain`.
    #[cfg(test)]
    pub(crate) fn test_seed_cell(&mut self, rx: u16, ry: u16, cell: BridgeRuntimeCell) {
        let needed_w = (rx + 1).max(self.width);
        let needed_h = (ry + 1).max(self.height);
        if needed_w != self.width || needed_h != self.height {
            // Resize while preserving existing (rx, ry) → cell mappings.
            let mut new_cells = vec![None; needed_w as usize * needed_h as usize];
            for old_ry in 0..self.height {
                for old_rx in 0..self.width {
                    let old_idx = old_ry as usize * self.width as usize + old_rx as usize;
                    let new_idx = old_ry as usize * needed_w as usize + old_rx as usize;
                    new_cells[new_idx] = self.cells[old_idx];
                }
            }
            self.cells = new_cells;
            self.width = needed_w;
            self.height = needed_h;
        }
        let idx = ry as usize * self.width as usize + rx as usize;
        self.cells[idx] = Some(cell);
    }

    /// Test-only: insert an `AnchorSpan` directly into the registry.
    #[cfg(test)]
    pub(crate) fn test_seed_anchor_span(&mut self, span: AnchorSpan) {
        self.anchor_spans.insert(span.id, span);
    }

    #[cfg(test)]
    pub(crate) fn test_set_endpoint_records(&mut self, records: Vec<BridgeEndpointRecord>) {
        self.endpoint_records = records;
    }

    pub fn effective_render_state(cell: &BridgeRuntimeCell) -> Option<DamageState> {
        let state_from_overlay = match cell.overlay_byte {
            0x4A..=0x4D => Some(DamageState::Healthy {
                variant: cell.overlay_byte - 0x4A,
            }),
            0x4E..=0x52 => Some(DamageState::Damaged),
            0x53..=0x56 => Some(DamageState::Healthy {
                variant: cell.overlay_byte - 0x53,
            }),
            0x57..=0x5B => Some(DamageState::Damaged),
            0x64 | 0x65 => None,
            0xCD..=0xD0 => Some(DamageState::Healthy {
                variant: cell.overlay_byte - 0xCD,
            }),
            0xD1..=0xD5 => Some(DamageState::Damaged),
            0xD6..=0xD9 => Some(DamageState::Healthy {
                variant: cell.overlay_byte - 0xD6,
            }),
            0xDA..=0xDE => Some(DamageState::Damaged),
            0xE7 | 0xE8 => None,
            OVERLAY_BYTE_NONE => None,
            _ => Some(cell.damage_state),
        };
        match state_from_overlay {
            Some(DamageState::Destroyed) | None => None,
            other => other,
        }
    }

    pub fn is_bridge_walkable(&self, rx: u16, ry: u16) -> bool {
        self.cell(rx, ry)
            .is_some_and(|cell| cell.deck_present && Self::effective_render_state(cell).is_some())
    }

    fn clear_collapsed_span_overlay_bytes(&mut self, span: &AnchorSpan) -> Vec<(u16, u16)> {
        let mut cleared = Vec::new();
        for (_, pos) in span.iter_cells() {
            if self.cell(pos.0, pos.1).is_some() {
                let _ = self.write_overlay_byte(pos.0, pos.1, OVERLAY_BYTE_NONE);
                let cell = self
                    .cell_mut(pos.0, pos.1)
                    .expect("bridge cell existed before overlay write");
                cell.damage_state = DamageState::Destroyed;
                if !cleared.contains(&pos) {
                    cleared.push(pos);
                }
            }
        }
        cleared
    }

    /// Body-cell state-machine driver. Mirrors the body branch of binary
    /// `ProcessBridgeDamageStateMachine_High @ 0x576BA0` (HIGH §3.1).
    ///
    /// Receives damage on a body-bridge cell at `(rx, ry)`. Resolves anchor
    /// (follows `anchor_span_id` if input cell is `Body` or `Tail`), reads
    /// anchor's current `damage_state`, transitions per binary switch arms,
    /// fires perpendicular `UpdateRamp_*` writes via `update_ramp_perpendicular`,
    /// and on collapse emits `set_bridge_direction(span, false)` for the
    /// `BlowUpBridge` cascade.
    ///
    /// Returns `StateOutcome::Absorbed` for `Healthy → Damaged`,
    /// `StateOutcome::Collapsed { ... }` for `Damaged → Destroyed` and
    /// partial-collapse → `Destroyed`, and `StateOutcome::NoChange` for
    /// already-destroyed / non-body / unresolvable-anchor inputs.
    ///
    /// `is_high_bridge` is currently unused (state transitions identical for
    /// HIGH and LOW per HIGH §11.1) but kept for API symmetry with the
    /// future overlay-write branch.
    /// `ProcessBridgeDamageStateMachine_High` 0x00576BA0 and its LOW twin
    /// `ProcessBridgeDamageStateMachine_Low` 0x00571490.
    pub fn body_cell_advance_state(
        &mut self,
        rx: u16,
        ry: u16,
        is_high_bridge: bool,
        terrain: &mut ResolvedTerrainGrid,
    ) -> StateOutcome {
        let mut live_flags = terrain.bridge_flag_execution_state();
        self.body_cell_advance_state_with_flags(rx, ry, is_high_bridge, terrain, &mut live_flags)
    }

    /// Same native body driver with a caller-owned live flag transaction.
    /// CABHUT fallback retries keep this value across immediate attempts so
    /// synchronous setters from one attempt gate the next one.
    pub(crate) fn body_cell_advance_state_with_flags(
        &mut self,
        rx: u16,
        ry: u16,
        is_high_bridge: bool,
        terrain: &mut ResolvedTerrainGrid,
        live_flags: &mut crate::map::resolved_terrain::CellClassBridgeFlagState,
    ) -> StateOutcome {
        // 1. Resolve input cell.
        let Some(input_cell) = self.cell(rx, ry).copied() else {
            return StateOutcome::NoChange;
        };

        // 2. Filter: must be body-bridge (Anchor / Body / Tail). Bridgehead
        //    cells route to Task 14's bridgehead driver (not part of this plan).
        if !matches!(
            input_cell.role,
            BridgeCellRole::Anchor | BridgeCellRole::Body | BridgeCellRole::Tail
        ) {
            return StateOutcome::NoChange;
        }

        // 3. Resolve anchor.
        let anchor_pos = if matches!(input_cell.role, BridgeCellRole::Anchor) {
            (rx, ry)
        } else {
            // Non-anchor body cell: follow anchor_span_id to span.anchor.
            let Some(span_id) = input_cell.anchor_span_id else {
                return StateOutcome::NoChange;
            };
            let Some(span) = self.anchor_span(span_id) else {
                return StateOutcome::NoChange;
            };
            span.anchor
        };

        let Some(anchor_cell) = self.cell(anchor_pos.0, anchor_pos.1).copied() else {
            return StateOutcome::NoChange;
        };
        let Some(axis) = anchor_cell.axis else {
            return StateOutcome::NoChange;
        };
        let span_id = match anchor_cell.anchor_span_id {
            Some(id) => id,
            None => return StateOutcome::NoChange,
        };
        let span_clone = match self.anchor_span(span_id) {
            Some(s) => s.clone(),
            None => return StateOutcome::NoChange,
        };

        // 4. Switch on anchor's damage_state.
        match anchor_cell.damage_state {
            DamageState::Healthy { .. } => {
                // Anchor advances to Damaged.
                if let Some(c) = self.cell_mut(anchor_pos.0, anchor_pos.1) {
                    c.damage_state = DamageState::Damaged;
                }
                // Fire UpdateRamp_*A and _*B on perpendicular targets.
                let mut damaged_variant_cells = Vec::new();
                let ramp_a = crate::sim::bridge_specs::update_ramp_perpendicular_with_flags(
                    self,
                    anchor_pos,
                    axis,
                    Phase::DamageA,
                    is_high_bridge,
                    terrain,
                    live_flags,
                );
                extend_unique_cells(
                    &mut damaged_variant_cells,
                    ramp_a.damaged_variant_cells,
                );
                let ramp_b = crate::sim::bridge_specs::update_ramp_perpendicular_with_flags(
                    self,
                    anchor_pos,
                    axis,
                    Phase::DamageB,
                    is_high_bridge,
                    terrain,
                    live_flags,
                );
                extend_unique_cells(
                    &mut damaged_variant_cells,
                    ramp_b.damaged_variant_cells,
                );
                StateOutcome::Absorbed {
                    damaged_variant_cells,
                }
            }
            DamageState::Damaged => {
                // Full collapse — fire CollapseA + CollapseB perpendicular,
                // anchor → Destroyed, set_bridge_direction cascade.
                let ramp_a = crate::sim::bridge_specs::update_ramp_perpendicular_with_flags(
                    self,
                    anchor_pos,
                    axis,
                    Phase::CollapseA,
                    is_high_bridge,
                    terrain,
                    live_flags,
                );
                let ramp_b = crate::sim::bridge_specs::update_ramp_perpendicular_with_flags(
                    self,
                    anchor_pos,
                    axis,
                    Phase::CollapseB,
                    is_high_bridge,
                    terrain,
                    live_flags,
                );
                let mut damaged_variant_cells = ramp_a.damaged_variant_cells;
                let mut setter_transcript = ramp_a.setter_transcript;
                extend_unique_cells(
                    &mut damaged_variant_cells,
                    ramp_b.damaged_variant_cells,
                );
                setter_transcript.extend(ramp_b.setter_transcript);
                let mut destroyed = self.clear_collapsed_span_overlay_bytes(&span_clone);
                if !destroyed.contains(&anchor_pos) {
                    destroyed.push(anchor_pos);
                }
                // Collect any perpendicular cells that hit collapse-final
                // (became Destroyed via update_ramp_perpendicular).
                for &perp_dir in &[Direction::E, Direction::W, Direction::N, Direction::S] {
                    let (dx, dy) = perp_dir.offset();
                    let nx = anchor_pos.0 as i32 + dx;
                    let ny = anchor_pos.1 as i32 + dy;
                    if nx < 0 || ny < 0 {
                        continue;
                    }
                    let pos = (nx as u16, ny as u16);
                    if let Some(c) = self.cell(pos.0, pos.1) {
                        if matches!(c.damage_state, DamageState::Destroyed)
                            && !destroyed.contains(&pos)
                        {
                            destroyed.push(pos);
                        }
                    }
                }
                let sbd = crate::sim::bridge_specs::set_bridge_direction(&span_clone, false);
                if let Some(stamp) = sbd.flag_stamp {
                    live_flags.apply_stamp(stamp);
                    setter_transcript.push(stamp);
                }
                let adj = compute_adjacent_bridges_dirty(rx, ry, axis);
                StateOutcome::Collapsed {
                    binary_success: true,
                    // Cloned before the move below; the collapsed anchor + any
                    // perpendicular finals are the minimap-dirty set (BR-16).
                    radar_cells: destroyed.clone(),
                    destroyed_cells: destroyed,
                    set_bridge_direction: sbd,
                    setter_transcript,
                    adjacent_bridges_dirty: adj,
                    zones_dirty: true,
                    damaged_variant_cells,
                }
            }
            DamageState::PartialCollapseA => {
                // Single CollapseA call, then collapse-finalize.
                let ramp = crate::sim::bridge_specs::update_ramp_perpendicular_with_flags(
                    self,
                    anchor_pos,
                    axis,
                    Phase::CollapseA,
                    is_high_bridge,
                    terrain,
                    live_flags,
                );
                let mut destroyed = self.clear_collapsed_span_overlay_bytes(&span_clone);
                if !destroyed.contains(&anchor_pos) {
                    destroyed.push(anchor_pos);
                }
                let sbd = crate::sim::bridge_specs::set_bridge_direction(&span_clone, false);
                let mut setter_transcript = ramp.setter_transcript;
                if let Some(stamp) = sbd.flag_stamp {
                    live_flags.apply_stamp(stamp);
                    setter_transcript.push(stamp);
                }
                let adj = compute_adjacent_bridges_dirty(rx, ry, axis);
                StateOutcome::Collapsed {
                    binary_success: true,
                    destroyed_cells: destroyed.clone(),
                    set_bridge_direction: sbd,
                    setter_transcript,
                    adjacent_bridges_dirty: adj,
                    zones_dirty: true,
                    radar_cells: destroyed,
                    damaged_variant_cells: ramp.damaged_variant_cells,
                }
            }
            DamageState::PartialCollapseB => {
                let ramp = crate::sim::bridge_specs::update_ramp_perpendicular_with_flags(
                    self,
                    anchor_pos,
                    axis,
                    Phase::CollapseB,
                    is_high_bridge,
                    terrain,
                    live_flags,
                );
                let mut destroyed = self.clear_collapsed_span_overlay_bytes(&span_clone);
                if !destroyed.contains(&anchor_pos) {
                    destroyed.push(anchor_pos);
                }
                let sbd = crate::sim::bridge_specs::set_bridge_direction(&span_clone, false);
                let mut setter_transcript = ramp.setter_transcript;
                if let Some(stamp) = sbd.flag_stamp {
                    live_flags.apply_stamp(stamp);
                    setter_transcript.push(stamp);
                }
                let adj = compute_adjacent_bridges_dirty(rx, ry, axis);
                StateOutcome::Collapsed {
                    binary_success: true,
                    destroyed_cells: destroyed.clone(),
                    set_bridge_direction: sbd,
                    setter_transcript,
                    adjacent_bridges_dirty: adj,
                    zones_dirty: true,
                    radar_cells: destroyed,
                    damaged_variant_cells: ramp.damaged_variant_cells,
                }
            }
            DamageState::Destroyed => StateOutcome::NoChange,
        }
    }

    /// Reverse counterpart to `body_cell_advance_state`. Repairs cells found
    /// in `scan_cells`: collects unique `anchor_span_id`s, iterates each
    /// span's cells (slots 0..6), and transitions
    /// `Damaged`/`Destroyed`/`PartialCollapse{A,B}` → `Healthy { variant }`.
    ///
    /// The Rust model uses anchor-span iteration in place of the binary's
    /// 3-cell-perpendicular-strip walker — the cell-state mutations are
    /// equivalent; the binary's RNG draw count differs (per-strip vs
    /// per-cell), locked across our Rust clients by the iteration-order pin
    /// test.
    ///
    /// **Side-effect gating:**
    ///   - `outcome.zones_dirty = true` iff at least one **main-deck**
    ///     (Anchor/Body/Tail role) damaged or destroyed cell was repaired.
    ///     Bridgehead-only repairs do NOT set this flag.
    ///   - `outcome.radar_cells` contains the exact cells changed by a
    ///     damage-variant clear, followed by destroyed-anchor restoration
    ///     cells not already present. Native's radar queue rejects duplicates
    ///     while retaining first insertion order.
    ///
    /// **RNG draws** (locked for lockstep across Rust clients):
    ///   - Main-deck damaged/destroyed/partial-collapse → 1 draw per cell
    ///     (`rng.next_range_u32(4)` → variant `0..=3`). MUST stay in `0..=3`
    ///     because variants 4/5 are RESERVED for `update_ramp_perpendicular`
    ///     to encode NS DamageA/B (they would render as damage-progression
    ///     SHP frames).
    ///   - Bridgehead damaged → write `Healthy { variant: 0 }`, **0 draws**.
    ///   - Already-`Healthy` or non-bridge cells → skip, **0 draws**.
    ///
    /// **Iteration order** (parity-critical, locked by test):
    ///   1. Anchor spans collected into `BTreeSet<u16>` for sorted iteration.
    ///   2. Within each span, cells iterated in slot order 0..=5.
    ///   3. `None` slots skipped.
    #[cfg(test)]
    pub fn body_cell_repair_state(
        &mut self,
        scan_cells: &[(u16, u16)],
        rng: &mut crate::sim::rng::SimRng,
        _terrain: &ResolvedTerrainGrid,
    ) -> RepairOutcome {
        let mut outcome = RepairOutcome::default();

        // Step 1: Collect unique anchor spans from scan cells.
        let mut spans: BTreeSet<u16> = BTreeSet::new();
        for &(rx, ry) in scan_cells {
            if let Some(cell) = self.cell(rx, ry) {
                if let Some(span_id) = cell.anchor_span_id {
                    spans.insert(span_id);
                }
            }
        }

        // Step 2: Iterate each span; for each cell, transition damage_state.
        for span_id in spans {
            // Clone span cell list to avoid borrow conflict.
            let cells_list: [Option<(u16, u16)>; 6] = match self.anchor_span(span_id) {
                Some(span) => span.cells,
                None => continue,
            };

            for slot in 0..6 {
                let Some(cell_pos) = cells_list[slot] else {
                    continue;
                };
                let Some(prior_state) = self.cell(cell_pos.0, cell_pos.1).map(|c| c.damage_state)
                else {
                    continue;
                };
                let Some(role) = self.cell(cell_pos.0, cell_pos.1).map(|c| c.role) else {
                    continue;
                };

                let new_state: DamageState = match (role, prior_state) {
                    // Already healthy: skip, no RNG draw.
                    (_, DamageState::Healthy { .. }) => continue,

                    // Bridgehead: fixed variant, no RNG.
                    (BridgeCellRole::Bridgehead, _) => DamageState::Healthy { variant: 0 },

                    // Main-deck (Anchor/Body/Tail) damaged/destroyed/partial: RNG variant.
                    (
                        BridgeCellRole::Anchor | BridgeCellRole::Body | BridgeCellRole::Tail,
                        DamageState::Damaged
                        | DamageState::Destroyed
                        | DamageState::PartialCollapseA
                        | DamageState::PartialCollapseB,
                    ) => {
                        // Variant range MUST be 0..=3 (rng.next_range_u32(4));
                        // variants 4/5 encode NS DamageA/B in our render model
                        // and would draw damage-progression SHP frames.
                        let variant = rng.next_range_u32(4) as u8;
                        DamageState::Healthy { variant }
                    }
                };

                if let Some(cell) = self.cell_mut(cell_pos.0, cell_pos.1) {
                    cell.damage_state = new_state;
                }
                // This legacy state-only helper has no production caller.
                // Ordinary native strip repair does not clear pavement here.
                outcome.repaired_cells += 1;

                let is_main_deck = matches!(
                    role,
                    BridgeCellRole::Anchor | BridgeCellRole::Body | BridgeCellRole::Tail
                );
                if is_main_deck {
                    outcome.zones_dirty = true;
                }
                if matches!(prior_state, DamageState::Destroyed) {
                    extend_unique_cells(&mut outcome.radar_cells, [cell_pos]);
                }
            }

            // Step 3: Sync the AnchorSpan's mirror `damage_state` field with
            // the anchor cell's new state (the span struct caches this for
            // queries; existing forward state machine does the same).
            let anchor_pos = self.anchor_span(span_id).map(|s| s.anchor);
            if let Some((arx, ary)) = anchor_pos {
                let new_anchor_state = self.cell(arx, ary).map(|c| c.damage_state);
                if let (Some(state), Some(span)) = (new_anchor_state, self.anchor_span_mut(span_id))
                {
                    span.damage_state = state;
                }
            }
        }

        outcome
    }

    /// Bridgehead-cell state-machine driver.
    ///
    /// Sparse-by-design: most bridgehead cells absorb damage via the per-axis
    /// start-cell gate inside `bridgehead_walk_to_anchor` (NS rejects odd
    /// heights; EW rejects heights > 4). Only the small subset that passes
    /// the gate reaches the anchor-write path.
    ///
    /// On a successful walk:
    /// - Writes `bridgehead_anchor_class = AboutToFall` on the anchor cell.
    ///   This is the **most-damaged variant** (4th slot in the enum, matching
    ///   the reference engine's anchor-tile write target). A later hit that
    ///   resolves this slot enters the collapse path below.
    /// - Fires `update_ramp_perpendicular(DamageA)` and `DamageB` on the
    ///   anchor's perpendicular neighbors. These do both the existing
    ///   state-byte bump (on Anchor targets) AND the asymmetric A/B
    ///   tile-class progression (on Anchor and Bridgehead targets) —
    ///   `Variant0 → Variant1 → Damaged` via DamageB; DamageA preserves.
    /// - The hit bridgehead cell's own `damage_state` is NEVER modified.
    ///
    /// Returns:
    /// - `StateOutcome::Absorbed` on a successful walk + anchor write.
    /// - `StateOutcome::NoChange` on role mismatch, missing axis, gated
    ///   start cell, or walk-off-map.
    /// - `StateOutcome::Collapsed` when the resolved bridgehead/anchor class
    ///   is already `AboutToFall` (binary slot `+3`).
    ///
    /// `is_high_bridge` selects the slot `+3` binary return value
    /// (high true; low false after collapse side effects).
    ///
    /// Height-source: `ResolvedTerrainCell.template_height`.
    pub fn bridgehead_advance_state(
        &mut self,
        rx: u16,
        ry: u16,
        is_high_bridge: bool,
        terrain: &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
    ) -> StateOutcome {
        let mut live_flags = terrain.bridge_flag_execution_state();
        self.bridgehead_advance_state_with_flags(rx, ry, is_high_bridge, terrain, &mut live_flags)
    }

    /// Same native bridgehead driver with a caller-owned live flag
    /// transaction. This is required by synchronous CABHUT fallback retries.
    pub(crate) fn bridgehead_advance_state_with_flags(
        &mut self,
        rx: u16,
        ry: u16,
        is_high_bridge: bool,
        terrain: &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
        live_flags: &mut crate::map::resolved_terrain::CellClassBridgeFlagState,
    ) -> StateOutcome {
        // 1. Resolve input cell.
        let Some(input_cell) = self.cell(rx, ry).copied() else {
            return StateOutcome::NoChange;
        };

        // 2. Filter: must be a Bridgehead. Body / Anchor / Tail route to the
        //    body driver.
        if !matches!(input_cell.role, BridgeCellRole::Bridgehead) {
            return StateOutcome::NoChange;
        }
        let Some(axis) = input_cell.axis else {
            return StateOutcome::NoChange;
        };

        // 3. Walk to anchor via the height-based predicate. The helper
        //    computes walk direction internally per the start cell's height
        //    and applies the per-axis start-cell gate. Failures (odd-h NS,
        //    h>4 EW, off-map) yield None — the damage is absorbed without
        //    state change.
        let map_w = self.width;
        let map_h = self.height;
        let height_lookup = |pos: (u16, u16)| -> Option<u8> {
            terrain.cell(pos.0, pos.1).map(|c| c.template_height)
        };
        let Some(anchor_pos) = crate::sim::bridge_specs::bridgehead_walk_to_anchor(
            (rx, ry),
            axis,
            height_lookup,
            map_w,
            map_h,
        ) else {
            return StateOutcome::NoChange;
        };

        let Some(anchor_snapshot) = self.cell(anchor_pos.0, anchor_pos.1).copied() else {
            return StateOutcome::NoChange;
        };
        let input_is_final = matches!(
            input_cell.bridgehead_anchor_class,
            BridgeheadAnchorClass::AboutToFall
        );
        let anchor_is_final = matches!(
            anchor_snapshot.bridgehead_anchor_class,
            BridgeheadAnchorClass::AboutToFall
        );

        if input_is_final || anchor_is_final {
            use crate::sim::bridge_specs::{CellAction, SetBridgeDirectionResult};

            if let Some(anchor_cell) = self.cell_mut(anchor_pos.0, anchor_pos.1) {
                anchor_cell.bridgehead_anchor_class = BridgeheadAnchorClass::AboutToFall;
            }

            let anchor_height = terrain
                .cell(anchor_pos.0, anchor_pos.1)
                .map(|c| c.template_height)
                .unwrap_or(anchor_snapshot.deck_level);
            let mut destroyed = Vec::new();
            let mut actions = Vec::new();
            for (slot, pos) in crate::sim::bridge_specs::bridgehead_blow_up_row(
                anchor_pos,
                axis,
                anchor_height,
                map_w,
                map_h,
            )
            .into_iter()
            .enumerate()
            .filter_map(|(slot, pos)| pos.map(|pos| (slot, pos)))
            {
                if !destroyed.contains(&pos) {
                    destroyed.push(pos);
                }
                actions.push((pos, slot, CellAction::BlowUpBridge));
                if self.cell(pos.0, pos.1).is_some() {
                    let _ = self.write_overlay_byte(pos.0, pos.1, OVERLAY_BYTE_NONE);
                    let c = self
                        .cell_mut(pos.0, pos.1)
                        .expect("bridge cell existed before overlay write");
                    c.damage_state = DamageState::Destroyed;
                    if matches!(c.role, BridgeCellRole::Anchor | BridgeCellRole::Bridgehead) {
                        c.bridgehead_anchor_class = BridgeheadAnchorClass::AboutToFall;
                    }
                }
            }

            let ramp_a = crate::sim::bridge_specs::update_ramp_perpendicular_with_flags(
                self,
                anchor_pos,
                axis,
                Phase::CollapseA,
                is_high_bridge,
                terrain,
                live_flags,
            );
            let ramp_b = crate::sim::bridge_specs::update_ramp_perpendicular_with_flags(
                self,
                anchor_pos,
                axis,
                Phase::CollapseB,
                is_high_bridge,
                terrain,
                live_flags,
            );
            let mut damaged_variant_cells = ramp_a.damaged_variant_cells;
            let mut setter_transcript = ramp_a.setter_transcript;
            extend_unique_cells(
                &mut damaged_variant_cells,
                ramp_b.damaged_variant_cells,
            );
            setter_transcript.extend(ramp_b.setter_transcript);
            for &perp_dir in &[Direction::E, Direction::W, Direction::N, Direction::S] {
                let (dx, dy) = perp_dir.offset();
                let nx = anchor_pos.0 as i32 + dx;
                let ny = anchor_pos.1 as i32 + dy;
                if nx < 0 || ny < 0 {
                    continue;
                }
                let pos = (nx as u16, ny as u16);
                if self
                    .cell(pos.0, pos.1)
                    .is_some_and(|c| matches!(c.damage_state, DamageState::Destroyed))
                    && !destroyed.contains(&pos)
                {
                    destroyed.push(pos);
                }
            }

            let adj = compute_adjacent_bridges_dirty(anchor_pos.0, anchor_pos.1, axis);
            return StateOutcome::Collapsed {
                binary_success: is_high_bridge,
                // Cloned before the move below; the BlowUpBridge triple + any
                // perpendicular finals are the minimap-dirty set (BR-16).
                radar_cells: destroyed.clone(),
                destroyed_cells: destroyed,
                set_bridge_direction: SetBridgeDirectionResult {
                    actions,
                    flag_stamp: None,
                },
                setter_transcript,
                adjacent_bridges_dirty: adj,
                zones_dirty: true,
                damaged_variant_cells,
            };
        }

        // 4. Write the anchor's bridgehead_anchor_class to AboutToFall
        //    (the most-damaged variant, 4th enum slot). Matches the
        //    reference engine's first-hit write to the anchor's tile-class
        //    field. A later hit that resolves AboutToFall enters the
        //    collapse path above. The hit bridgehead cell's own
        //    damage_state is never touched.
        if let Some(anchor_cell) = self.cell_mut(anchor_pos.0, anchor_pos.1) {
            anchor_cell.bridgehead_anchor_class = BridgeheadAnchorClass::AboutToFall;
        }

        // 5. Fire the perpendicular DamageA + DamageB writes. These do the
        //    state-byte bump on Anchor targets and the asymmetric A/B
        //    tile-class progression on both Anchor and Bridgehead targets.
        let ramp_a = crate::sim::bridge_specs::update_ramp_perpendicular_with_flags(
            self,
            anchor_pos,
            axis,
            Phase::DamageA,
            is_high_bridge,
            terrain,
            live_flags,
        );
        let ramp_b = crate::sim::bridge_specs::update_ramp_perpendicular_with_flags(
            self,
            anchor_pos,
            axis,
            Phase::DamageB,
            is_high_bridge,
            terrain,
            live_flags,
        );
        let mut damaged_variant_cells = ramp_a.damaged_variant_cells;
        extend_unique_cells(
            &mut damaged_variant_cells,
            ramp_b.damaged_variant_cells,
        );

        StateOutcome::Absorbed {
            damaged_variant_cells,
        }
    }

    /// Bridge endpoint records for zone connectivity.
    /// Each active record connects ground zones on opposite sides of a bridge.
    pub fn endpoint_records(&self) -> &[BridgeEndpointRecord] {
        &self.endpoint_records
    }

    pub(crate) fn native_zone_source_size(&self) -> Option<(i32, i32)> {
        self.native_zone_source_size
    }

    /// Recompute `endpoint_records[*].active` flags from current cell render
    /// state, BIDIRECTIONALLY. A record is active iff its bridge group is
    /// intact — i.e. no cell in the group is severed. When any cell of a group
    /// is destroyed, its record deactivates so the zone graph (`zone_build`)
    /// stops treating the endpoint pair as connected; when an engineer repair
    /// restores every cell, the same test re-activates the record so the
    /// long-range A* zone edge (gated on `record.active` in
    /// `zone_build::bridge_record_matches`) is restored.
    ///
    /// "Severed" is keyed on `effective_render_state(cell).is_none()`, NOT on
    /// `damage_state`. Decision A: the overlay byte is authoritative. The
    /// repair path restores the overlay byte to a healthy band but
    /// intentionally leaves `damage_state` stale at `Destroyed` (the original
    /// engine leaves the body damage byte stale after repair), so keying on
    /// `damage_state` would never re-activate a repaired record. The healthy
    /// overlay-band arms of `effective_render_state` take precedence over its
    /// `damage_state` fallback, so a repaired cell (healthy overlay, stale
    /// `Destroyed`) reports `Some` and re-activates, while a collapsed cell
    /// (destroyed-overlay byte, or `0xFF` + `Destroyed`) reports `None`.
    ///
    /// Granularity is whole-group (the group is the 4-cardinal BFS blob from
    /// construction). Per-record geometric tolerance is the separate deferred
    /// BR-40 work — do not narrow this here.
    pub fn refresh_endpoint_active_flags(&mut self) {
        let mut severed_groups: BTreeSet<u16> = BTreeSet::new();
        for cell_opt in &self.cells {
            if let Some(cell) = cell_opt {
                if Self::effective_render_state(cell).is_none() {
                    if let Some(gid) = cell.bridge_group_id {
                        severed_groups.insert(gid);
                    }
                }
            }
        }
        for record in &mut self.endpoint_records {
            // Recompute in BOTH directions: active iff no cell of the group is
            // severed. Repair restores the overlay byte (-> `Some`), removing
            // the group from `severed_groups` and flipping the record active.
            // A map-load record spanning no structural runtime group uses the
            // Rust-only zero sentinel and retains its load-time inactive state.
            if record.group_id != 0 {
                record.active = !severed_groups.contains(&record.group_id);
            }
        }
    }

    pub fn iter_cells(&self) -> impl Iterator<Item = ((u16, u16), &BridgeRuntimeCell)> {
        self.cells
            .iter()
            .enumerate()
            .filter_map(move |(idx, cell)| {
                let cell = cell.as_ref()?;
                let rx = (idx % self.width as usize) as u16;
                let ry = (idx / self.width as usize) as u16;
                Some(((rx, ry), cell))
            })
    }
}

fn index_of(width: u16, height: u16, rx: u16, ry: u16) -> Option<usize> {
    (rx < width && ry < height).then_some(ry as usize * width as usize + rx as usize)
}

/// Enumerate the 25 cells in a 5×5 inclusive `[-2..=+2]` scan around
/// `center`. Yields cell coordinates clamped to non-negative `(u16, u16)`
/// (cells with negative computed coords are skipped — they're off-map).
///
/// Used by the engineer-repair trigger. Inclusive bounds `-2..=+2` produce
/// exactly 25 cells when the center is interior; off-map negative cells are
/// silently dropped.
#[cfg(test)]
pub fn cells_in_5x5_scan(center: (u16, u16)) -> impl Iterator<Item = (u16, u16)> {
    let (cx, cy) = (center.0 as i32, center.1 as i32);
    (-2..=2i32).flat_map(move |dy| {
        (-2..=2i32).filter_map(move |dx| {
            let nx = cx + dx;
            let ny = cy + dy;
            if nx < 0 || ny < 0 || nx > u16::MAX as i32 || ny > u16::MAX as i32 {
                None
            } else {
                Some((nx as u16, ny as u16))
            }
        })
    })
}

fn bridge_runtime_group_at(
    runtime_cells: &[Option<BridgeRuntimeCell>],
    width: u16,
    height: u16,
    rx: u16,
    ry: u16,
) -> Option<u16> {
    index_of(width, height, rx, ry)
        .and_then(|index| runtime_cells.get(index))
        .and_then(|cell| cell.as_ref())
        .and_then(|cell| cell.bridge_group_id)
}

fn cardinal_neighbors(
    rx: u16,
    ry: u16,
    width: u16,
    height: u16,
) -> impl Iterator<Item = (u16, u16)> {
    const OFFSETS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    OFFSETS.into_iter().filter_map(move |(dx, dy)| {
        let nx = rx as i32 + dx;
        let ny = ry as i32 + dy;
        (nx >= 0 && ny >= 0 && (nx as u16) < width && (ny as u16) < height)
            .then_some((nx as u16, ny as u16))
    })
}

fn bridge_layer_to_axis(layer: Option<&crate::map::resolved_terrain::BridgeLayer>) -> Option<Axis> {
    layer.map(|bl| bridge_direction_to_axis(bl.direction))
}

fn resolved_cell_has_runtime_deck(
    cell: &crate::map::resolved_terrain::ResolvedTerrainCell,
) -> bool {
    cell.bridge_facts.has_structural_bridge()
        || (cell.has_bridge_deck
            && cell.bridge_facts.family == crate::map::bridge_facts::BridgeStampFamily::None)
}

fn bridge_fact_axis(cell: &crate::map::resolved_terrain::ResolvedTerrainCell) -> Option<Axis> {
    cell.bridge_facts
        .direction
        .map(bridge_stamp_direction_to_axis)
}

fn initial_bridge_damage_state(
    cell: &crate::map::resolved_terrain::ResolvedTerrainCell,
) -> DamageState {
    if cell.bridge_facts.has_structural_bridge()
        || cell.bridge_facts.family != crate::map::bridge_facts::BridgeStampFamily::None
    {
        DamageState::from_state_byte(cell.bridge_facts.state_byte)
            .unwrap_or(DamageState::Healthy { variant: 0 })
    } else {
        DamageState::Healthy { variant: 0 }
    }
}

fn bridge_stamp_direction_to_axis(direction: u8) -> Axis {
    match direction & 7 {
        2 | 6 => Axis::EW,
        _ => Axis::NS,
    }
}

fn bridge_stamp_direction_to_direction(direction: u8) -> Direction {
    match direction & 7 {
        0 => Direction::N,
        1 => Direction::NE,
        2 => Direction::E,
        3 => Direction::SE,
        4 => Direction::S,
        5 => Direction::SW,
        6 => Direction::W,
        _ => Direction::NW,
    }
}

fn bridge_direction_to_axis(d: crate::map::resolved_terrain::BridgeDirection) -> Axis {
    use crate::map::resolved_terrain::BridgeDirection;
    match d {
        BridgeDirection::EastWest => Axis::EW,
        BridgeDirection::NorthSouth => Axis::NS,
        // Low bridges (wood) read bridge_layer separately; treat as NS for now.
        // Phase C may revisit if low needs distinct axis handling.
        BridgeDirection::Low => Axis::NS,
    }
}

/// HIGH bridge anchor overlays = 0x18, 0x19; LOW bridge anchor overlays = 0xED, 0xEE.
///
/// NOTE (Phase B): These overlay IDs are also used to mark every HIGH-bridge
/// deck cell's direction, so under this predicate every HIGH-bridge cell with
/// a bridge_layer becomes an anchor. Phase C anchor-walker correctness tests
/// (Task 27) will tighten this to only true anchor cells.
fn is_anchor_overlay(overlay_id: u8) -> bool {
    matches!(overlay_id, 0x18 | 0x19 | 0xED | 0xEE)
}

/// State-machine convention: NS-axis collapse walks E (dir=2) for ramp A;
/// EW-axis collapse walks S (dir=4) for ramp A. We pick A-direction as the
/// canonical anchor walk direction (cell 5 then walks the opposite from anchor).
fn anchor_walk_direction(axis: Axis) -> Direction {
    match axis {
        Axis::NS => Direction::E,
        Axis::EW => Direction::S,
    }
}

/// Walk the 6-cell anchor pattern. Cells beyond the map edge become `None`.
fn walk_anchor_pattern(
    span_id: u16,
    anchor: (u16, u16),
    axis: Axis,
    direction: Direction,
    bridge_group_id: u16,
    width: u16,
    height: u16,
) -> AnchorSpan {
    let mut cells: [Option<(u16, u16)>; 6] = [None; 6];
    cells[0] = Some(anchor);

    let (dx, dy) = direction.offset();
    // Slot 1, 2, 3: walk +direction × 1, 2, 3.
    for step in 1..=3 {
        let nx = anchor.0 as i32 + dx * step;
        let ny = anchor.1 as i32 + dy * step;
        if nx >= 0 && ny >= 0 && (nx as u16) < width && (ny as u16) < height {
            cells[step as usize] = Some((nx as u16, ny as u16));
        }
    }

    // Slot 4: walk -direction × 1.
    let opp = direction.opposite();
    let (odx, ody) = opp.offset();
    let ox = anchor.0 as i32 + odx;
    let oy = anchor.1 as i32 + ody;
    if ox >= 0 && oy >= 0 && (ox as u16) < width && (oy as u16) < height {
        cells[4] = Some((ox as u16, oy as u16));
    }

    // Slot 5: optional extra cell, present only for the dir-W anchor. It is
    // the OPPOSITE step taken twice — `anchor + 2·E` — i.e. one cell beyond the
    // slot-4 opposite cell, NOT a duplicate of it. Matches
    // `bridge_facts::stamp_slots` (`ExtraDir6 = step(opposite, E)`). Writing
    // `+1` here aliased slot 4, which (a) left the true extra cell untagged and
    // (b) flipped the opposite cell's role Tail->Body via last-write-wins in
    // the pass-2 tagging loop.
    if direction == Direction::W {
        let ex = anchor.0 as i32 + 2;
        let ey = anchor.1 as i32;
        if ex >= 0 && ey >= 0 && (ex as u16) < width && (ey as u16) < height {
            cells[5] = Some((ex as u16, ey as u16));
        }
    }

    AnchorSpan {
        id: span_id,
        anchor,
        cells,
        axis,
        direction,
        damage_state: DamageState::Healthy { variant: 0 },
        bridge_group_id,
    }
}

/// Compute the two perpendicular cells where `UpdateAdjacentBridges_High`
/// should fire after a body-cell collapse. Per binary `0x576BA0`, the call
/// passes the ORIGINAL damaged cell coord (not the anchor); the offsets are
/// directional.
fn compute_adjacent_bridges_dirty(rx: u16, ry: u16, axis: Axis) -> Vec<(u16, u16)> {
    let mut out = Vec::with_capacity(2);
    let perpendiculars: [Direction; 2] = match axis {
        Axis::NS => [Direction::E, Direction::W],
        Axis::EW => [Direction::S, Direction::N],
    };
    for d in perpendiculars {
        let (dx, dy) = d.offset();
        let nx = rx as i32 + dx;
        let ny = ry as i32 + dy;
        if nx >= 0 && ny >= 0 {
            out.push((nx as u16, ny as u16));
        }
    }
    out
}

#[cfg(test)]
mod repair_tests;
#[cfg(test)]
mod scan_tests;
#[cfg(test)]
mod tests;
