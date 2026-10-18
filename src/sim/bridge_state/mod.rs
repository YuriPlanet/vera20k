//! Mutable bridge runtime state layered on top of resolved terrain.
//!
//! Bridges are modeled as terrain, not spawned entities. This module owns the
//! destroyable runtime state used by combat, layered pathing, and bridge-deck
//! fallout handling.
//!
//! Ordinary wooden74..101 and concrete205..232 overlays share scalar
//! `ordinary_damage` / `ordinary_repair` controllers and synchronous world
//! publication. CellClass (`ResolvedTerrainGrid`) is the only owner of per-cell
//! bridge state: the +140 flags, +11E state byte, +44 overlay identity and
//! level. This module keeps only the map-wide bridge records and constants;
//! [`cell_render_state`] decodes the live CellClass fields for readers.

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
//! - `MapClass::IncrementBridgeCounter` 0x00578AC0 — **drawing cache epoch,
//!   not bridge simulation.**4F42F0(1), reached by Execute1186E2C20, increments
//!   Map+1158; overlay drawing47F77B/47F843 compares/stamps Cell+118 against
//!   it to suppress cached blits. Current rendering rebuilds/uploads each
//!   frame and needs no simulation counterpart. Original Steam byte/caller
//!   evidence: ALL01 action118-redraw-original packet.
//! - `ShipLocomotionClass::Compute_BridgeZOffset` 0x0069EBB0 — no CALL xrefs.
//!   Its Drive-side analogue `DriveLocomotionClass::ComputeBridgeZOffset`
//!   0x004AF4A0 is a one-shot constant initializer and is bound in
//!   `sim::movement::movement_bridge`; this one cannot be bound without a
//!   reference.
pub(crate) mod damage_dispatch;
pub(crate) mod gap_restamp;
pub(crate) mod occupants;
pub mod ordinary;
pub(crate) mod ordinary_damage;
pub(crate) mod ordinary_repair;
pub(crate) mod publication;
pub mod ramp_repair;
mod record_scan;
pub(crate) mod repair_query;
pub(crate) mod rim;
mod zone_activation;

use crate::map::bridge_facts::{
    BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts,
};
use crate::map::resolved_terrain::ResolvedTerrainGrid;

/// High-bridge theater slots used by the map-load bridge-record walk.
/// A negative entry is a deliberately unused slot.
const HIGH_BRIDGE_START_SUBTILE: [i32; 16] = [7, 7, -1, 7, 7, -1, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2];
const HIGH_BRIDGE_WALK_DIRECTION: [i32; 16] = [2, 2, -1, 4, 4, -1, 2, 2, 2, 2, 2, 4, 4, 4, 4, 4];
const HIGH_BRIDGE_END_SUBTILE: [i32; 16] = [-1, -1, 4, -1, -1, 2, 4, 4, 4, 4, 4, 2, 2, 2, 2, 2];
// Static bridge axis/anchor vocabulary is map-owned (map::bridge_facts, F05);
// sim re-exports so runtime and serialized consumers keep their paths.
pub use crate::map::bridge_facts::Axis;

/// Stateless decode of the CellClass+0x11E bridge state byte.
///
/// Body cells transition Healthy → Damaged → destroyed under repeated
/// damage (per axis). Partial-collapse states are reached only via
/// bridgehead final-step cascade. The byte writers are the ramp transitions
/// ([`crate::sim::bridge_specs::apply_ramp_transition`]); a destroyed cell is
/// not a byte value: see [`cell_render_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl DamageState {
    /// Decode from binary state byte. Returns `None` for bytes outside the
    /// defined ranges (NS: 0..=8; EW: 9..=0x11). Byte 0 always decodes to
    /// `Healthy{variant: 0}`; [`cell_render_state`] adds the flag context.
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

/// Visible bridge state of one CellClass cell, decoded from its live +44
/// overlay identity, +11E state byte and +140 flags. `None` means the cell
/// draws no bridge sprite: no overlay, a terminal ordinary overlay, an
/// undecodable state byte, or a destroyed structural cell.
///
/// Ordinary overlays carry their state and axis in the identity. Every other
/// overlay (high anchors 0x18/0x19/0xED/0xEE, ordinary end pieces) reads the
/// state byte, whose range also selects the axis (`<= 8` NS, else EW; the
/// setters write only 0..=17). A destroyed cell is state 0 with the 0x100
/// deck cleared and the setter's 0x400 destroyed bit set (47E040 writes both
/// on every BlowUpBridge slot); cells no setter destroyed keep state 0 as
/// Healthy NS. This is not a structural-presence query: native
/// constructor5FC380 leaves nonanchor structural cells at overlay -1
/// (constructor corpus cases8..11), and the anchor supplies their sprite.
pub fn cell_render_state(facts: BridgeCellFacts) -> Option<(DamageState, Axis)> {
    let state = facts.state_byte;
    let overlay = facts.overlay_id?;
    // Ordinary overlays carry their axis in the identity (the ordinary
    // selector's classification); every other overlay reads the byte range.
    let axis = [ramp_repair::Family::Low, ramp_repair::Family::High]
        .into_iter()
        .find_map(|family| ordinary::axis(i32::from(overlay), family))
        .unwrap_or(if state <= 8 { Axis::NS } else { Axis::EW });
    let render = match overlay {
        0x4A..=0x4D => DamageState::Healthy {
            variant: overlay - 0x4A,
        },
        0x4E..=0x52 => DamageState::Damaged,
        0x53..=0x56 => DamageState::Healthy {
            variant: overlay - 0x53,
        },
        0x57..=0x5B => DamageState::Damaged,
        0x64 | 0x65 => return None,
        0xCD..=0xD0 => DamageState::Healthy {
            variant: overlay - 0xCD,
        },
        0xD1..=0xD5 => DamageState::Damaged,
        0xD6..=0xD9 => DamageState::Healthy {
            variant: overlay - 0xD6,
        },
        0xDA..=0xDE => DamageState::Damaged,
        0xE7 | 0xE8 | 0xFF => return None,
        _ => {
            if state == 0
                && facts.raw_flags & (BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DESTROYED_OR_RAMP)
                    == BRIDGE_FLAG_DESTROYED_OR_RAMP
            {
                return None;
            }
            DamageState::from_state_byte(state)?
        }
    };
    Some((render, axis))
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

/// Binary bridge record kind (`BridgeRecord+0x0C`).
///
/// Verified against `MapClass__ComputeBridgeZones @ 0x0056D6E0`:
/// high bridges write `0`, accepted Tube endpoints write `1`. The historical
/// `Low` Rust name is retained for compatibility. `MapClass__FindBridgeRecord`
/// skips non-zero kinds, so callers must choose high-only vs all-record use.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, Default,
)]
pub enum BridgeRecordKind {
    #[default]
    High,
    Low,
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

/// Map-wide bridge state that CellClass does not hold: the MapClass+0x54
/// record vector, its source Map Size, and the two rules/scenario constants
/// the damage dispatcher reads. Per-cell bridge state is CellClass-owned.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct BridgeRuntimeState {
    /// Strength constant from `[CombatDamage] BridgeStrength=` (default 1000).
    /// Used by the dispatcher's per-path BridgeStrength RNG gate.
    bridge_strength: i32,
    endpoint_records: Vec<BridgeEndpointRecord>,
    /// Source Map Size paired with this derived record set. Only construction
    /// writes it; this receipt is not an independently editable map authority.
    /// Native base-zone endpoint lookup uses stride W+H+1, not terrain width.
    #[serde(default)]
    native_zone_source_size: Option<(i32, i32)>,
    /// Active `SpecialFlags::DestroyableBridges` bit. Read by the weapon AoE
    /// bridge-damage outer gate.
    bridge_destroyable_flag: bool,
}

impl BridgeRuntimeState {
    /// Production entry with the raw Map Size authority, not backing dimensions.
    pub fn from_resolved_terrain_with_map_size(
        terrain: &ResolvedTerrainGrid,
        destroyable: bool,
        bridge_strength: i32,
        size: (i32, i32),
    ) -> Self {
        Self::build_from_terrain(terrain, destroyable, bridge_strength, Some(size))
    }

    #[cfg(any(test, feature = "test-support"))]
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
        Self {
            bridge_strength,
            endpoint_records: record_scan::compute_bridge_endpoints(terrain, size),
            native_zone_source_size: size,
            bridge_destroyable_flag: destroyable,
        }
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

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn test_set_endpoint_records(&mut self, records: Vec<BridgeEndpointRecord>) {
        self.endpoint_records = records;
    }

    /// Bridge endpoint records for zone connectivity.
    /// Each active record connects ground zones on opposite sides of a bridge.
    pub fn endpoint_records(&self) -> &[BridgeEndpointRecord] {
        &self.endpoint_records
    }

    pub(crate) fn native_zone_source_size(&self) -> Option<(i32, i32)> {
        self.native_zone_source_size
    }
}

#[cfg(test)]
mod tests;
