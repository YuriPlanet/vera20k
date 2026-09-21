//! RE-backed bridge helper algorithms not yet fully wired into the live runtime.
//!
//! These helpers mirror closed behavior from the RE repo:
//! - low-bridge overlay damage step (RA2)
//! - low-bridge connected-section selector (YR)
//! - ZoneConnection record decode + proximity matching
//! - bridge-layer zone-id policy gate (RA2/YR)
//!
//! They are kept as pure functions so the runtime can adopt them incrementally
//! once mutable overlay state and ZoneConnection records are available.

use crate::sim::bridge_state::{
    AnchorSpan, Axis, BridgeRuntimeState, DamageState, Direction, Phase,
};

#[cfg(test)]
const BRIDGE_GATE_BIT: u32 = 0x0100;
#[cfg(test)]
const NO_ZONE_CONNECTION: i16 = -1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeOverlayTriple {
    pub a: i32,
    pub center: i32,
    pub b: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowBridgeOverlayDamageReason {
    NotBridgeOverlay,
    GateFailed,
    NoTransition,
    Changed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LowBridgeOverlayDamageStepResult {
    pub ok: bool,
    pub reason: LowBridgeOverlayDamageReason,
    pub changed: bool,
    pub triple_out: BridgeOverlayTriple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowBridgeConnectedBand {
    WoodBand1,
    WoodBand2,
    ConcreteBand1,
    ConcreteBand2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowBridgeConnectedAnchor {
    OppositeAdjacent,
    Center,
    PrimaryAdjacent,
    ConnectedChainHelper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowBridgeConnectedPattern {
    A,
    B,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LowBridgeConnectedSectionSelectorResult {
    pub handled: bool,
    pub reason_not_bridge_overlay: bool,
    pub pattern: Option<LowBridgeConnectedPattern>,
    pub band: Option<LowBridgeConnectedBand>,
    pub anchor: Option<LowBridgeConnectedAnchor>,
    pub neighbor_range_lo: Option<i32>,
    pub neighbor_range_hi: Option<i32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneConnectionRecord {
    pub cell_a: (i16, i16),
    pub cell_b: (i16, i16),
    pub flags: u32,
    pub flags_byte8: u8,
    pub skip_if_nonzero: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BridgeZoneIdPolicyTarget {
    Ra21006,
    Yr1001,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BridgeZoneIdPolicyDecision {
    pub use_bridge_path: bool,
    pub call_bridge_remap_fallback: bool,
    pub return_no_zone: bool,
}

#[cfg(test)]
pub fn low_bridge_overlay_damage_step_ra2(
    triple: BridgeOverlayTriple,
    damage: i32,
    bridge_strength: i32,
    atom_damage: i32,
    random_ranged_1_bridge_strength: i32,
) -> LowBridgeOverlayDamageStepResult {
    let center = triple.center;
    let in_a = in_range_inclusive(center, 0x4a, 0x63);
    let in_b = in_range_inclusive(center, 0xcd, 0xe6);

    if !in_a && !in_b {
        return LowBridgeOverlayDamageStepResult {
            ok: true,
            reason: LowBridgeOverlayDamageReason::NotBridgeOverlay,
            changed: false,
            triple_out: triple,
        };
    }

    if damage != atom_damage {
        if bridge_strength <= 0 || random_ranged_1_bridge_strength >= damage {
            return LowBridgeOverlayDamageStepResult {
                ok: true,
                reason: LowBridgeOverlayDamageReason::GateFailed,
                changed: false,
                triple_out: triple,
            };
        }
    }

    let new_index = if in_a {
        pattern_a_new_index(center)
    } else {
        pattern_b_new_index(center)
    };

    match new_index {
        Some(new_index) => LowBridgeOverlayDamageStepResult {
            ok: true,
            reason: LowBridgeOverlayDamageReason::Changed,
            changed: true,
            triple_out: BridgeOverlayTriple {
                a: new_index,
                center: new_index,
                b: new_index,
            },
        },
        None => LowBridgeOverlayDamageStepResult {
            ok: true,
            reason: LowBridgeOverlayDamageReason::NoTransition,
            changed: false,
            triple_out: triple,
        },
    }
}

#[cfg(test)]
pub fn low_bridge_connected_section_selector_yr(
    center_overlay_type_index: i32,
    primary_probe_in_family_range: bool,
    secondary_probe_in_family_range: bool,
) -> LowBridgeConnectedSectionSelectorResult {
    let Some(band) = classify_low_bridge_band(center_overlay_type_index) else {
        return LowBridgeConnectedSectionSelectorResult {
            handled: false,
            reason_not_bridge_overlay: true,
            pattern: None,
            band: None,
            anchor: None,
            neighbor_range_lo: None,
            neighbor_range_hi: None,
        };
    };

    let (pattern, neighbor_range_lo, neighbor_range_hi) = match band {
        LowBridgeConnectedBand::WoodBand1 | LowBridgeConnectedBand::WoodBand2 => {
            (LowBridgeConnectedPattern::A, 0x4a, 0x65)
        }
        LowBridgeConnectedBand::ConcreteBand1 | LowBridgeConnectedBand::ConcreteBand2 => {
            (LowBridgeConnectedPattern::B, 0xcd, 0xe8)
        }
    };

    let anchor = if !primary_probe_in_family_range {
        LowBridgeConnectedAnchor::OppositeAdjacent
    } else if !secondary_probe_in_family_range {
        LowBridgeConnectedAnchor::Center
    } else if matches!(
        band,
        LowBridgeConnectedBand::WoodBand1 | LowBridgeConnectedBand::ConcreteBand1
    ) {
        LowBridgeConnectedAnchor::PrimaryAdjacent
    } else {
        LowBridgeConnectedAnchor::ConnectedChainHelper
    };

    LowBridgeConnectedSectionSelectorResult {
        handled: true,
        reason_not_bridge_overlay: false,
        pattern: Some(pattern),
        band: Some(band),
        anchor: Some(anchor),
        neighbor_range_lo: Some(neighbor_range_lo),
        neighbor_range_hi: Some(neighbor_range_hi),
    }
}

pub fn decode_zone_connection_record(record: &[u8]) -> ZoneConnectionRecord {
    assert_eq!(record.len(), 16, "expected 16-byte ZoneConnection record");

    let flags = read_u32_le(record, 0x08);
    ZoneConnectionRecord {
        cell_a: (read_i16_le(record, 0x00), read_i16_le(record, 0x02)),
        cell_b: (read_i16_le(record, 0x04), read_i16_le(record, 0x06)),
        flags,
        flags_byte8: (flags & 0xff) as u8,
        skip_if_nonzero: read_u32_le(record, 0x0c),
    }
}

#[cfg(test)]
pub fn zone_connection_matches_cell(record: &[u8], cell: (i16, i16), dist: i16) -> bool {
    let decoded = decode_zone_connection_record(record);
    if decoded.skip_if_nonzero != 0 {
        return false;
    }

    let dist = dist.max(0);
    let ((ax, ay), (bx, by)) = (decoded.cell_a, decoded.cell_b);

    if ax == bx {
        let y_min = ay.min(by);
        let y_max = ay.max(by);
        cell.1 >= y_min && cell.1 <= y_max && (cell.0 - ax).abs() <= dist
    } else {
        let x_min = ax.min(bx);
        let x_max = ax.max(bx);
        cell.0 >= x_min && cell.0 <= x_max && (cell.1 - ay).abs() <= dist
    }
}

#[cfg(test)]
pub fn get_cell_zone_id_bridge_policy_decision(
    target: BridgeZoneIdPolicyTarget,
    on_bridge: bool,
    cell_flags_dword: u32,
    zone_connection_index: i16,
) -> BridgeZoneIdPolicyDecision {
    let use_bridge_path = on_bridge && (cell_flags_dword & BRIDGE_GATE_BIT) != 0;
    if !use_bridge_path {
        return BridgeZoneIdPolicyDecision {
            use_bridge_path: false,
            call_bridge_remap_fallback: false,
            return_no_zone: false,
        };
    }

    if zone_connection_index != NO_ZONE_CONNECTION {
        return BridgeZoneIdPolicyDecision {
            use_bridge_path: true,
            call_bridge_remap_fallback: false,
            return_no_zone: false,
        };
    }

    match target {
        BridgeZoneIdPolicyTarget::Yr1001 => BridgeZoneIdPolicyDecision {
            use_bridge_path: true,
            call_bridge_remap_fallback: true,
            return_no_zone: false,
        },
        BridgeZoneIdPolicyTarget::Ra21006 => BridgeZoneIdPolicyDecision {
            use_bridge_path: true,
            call_bridge_remap_fallback: false,
            return_no_zone: true,
        },
    }
}

#[cfg(test)]
fn in_range_inclusive(x: i32, lo: i32, hi: i32) -> bool {
    x >= lo && x <= hi
}

#[cfg(test)]
fn pattern_a_new_index(center_overlay_type_index: i32) -> Option<i32> {
    match center_overlay_type_index {
        0x60 => Some(0x61),
        0x62 => Some(0x63),
        x if x < 0x59 => Some(0x59),
        x if x < 0x5c => Some(0x65),
        _ => None,
    }
}

#[cfg(test)]
fn pattern_b_new_index(center_overlay_type_index: i32) -> Option<i32> {
    match center_overlay_type_index {
        0xe3 => Some(0xe4),
        0xe5 => Some(0xe6),
        x if x < 0xdc => Some(0xdc),
        x if x < 0xdf => Some(0xe8),
        _ => None,
    }
}

#[cfg(test)]
fn classify_low_bridge_band(center_overlay_type_index: i32) -> Option<LowBridgeConnectedBand> {
    let x = center_overlay_type_index;

    if in_range_inclusive(x, 0x4a, 0x52) || in_range_inclusive(x, 0x5c, 0x5f) || x == 0x64 {
        return Some(LowBridgeConnectedBand::WoodBand1);
    }
    if in_range_inclusive(x, 0x53, 0x5b) || in_range_inclusive(x, 0x60, 0x63) || x == 0x65 {
        return Some(LowBridgeConnectedBand::WoodBand2);
    }
    if in_range_inclusive(x, 0xcd, 0xd5) || in_range_inclusive(x, 0xdf, 0xe2) || x == 0xe7 {
        return Some(LowBridgeConnectedBand::ConcreteBand1);
    }
    if in_range_inclusive(x, 0xd6, 0xde) || in_range_inclusive(x, 0xe3, 0xe6) || x == 0xe8 {
        return Some(LowBridgeConnectedBand::ConcreteBand2);
    }

    None
}

fn read_u16_le(bytes: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([bytes[off], bytes[off + 1]])
}

fn read_i16_le(bytes: &[u8], off: usize) -> i16 {
    read_u16_le(bytes, off) as i16
}

fn read_u32_le(bytes: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([bytes[off], bytes[off + 1], bytes[off + 2], bytes[off + 3]])
}

/// Apply a single ramp state transition. Mirrors one of the binary's 16
/// `UpdateRamp_*_High/_Low` helpers (HIGH §11.1).
///
/// State byte semantics (CellClass+0x11E):
/// - NS-axis range: 0..=8 (0..=3 healthy, 4 = DamageA-set, 5 = DamageB-set,
///   6 = both halves damaged, 7 = PartialCollapseA, 8 = PartialCollapseB)
/// - EW-axis range: 9..=17 (9..=12 healthy, 0x0D = DamageB-set, 0x0E =
///   DamageA-set, 0x0F = both halves damaged, 0x10 = PartialCollapseB,
///   0x11 = PartialCollapseA)
///
/// Returns `Some(next_state)` on a defined transition, `None` if the
/// `(axis, phase, current_state)` combination has no transition (cell
/// unchanged).
///
/// **Collapse-final special case:** when input matches the "opposite-already-
/// collapsed" partial state (NS Collapse{A,B}: state 8/7; EW Collapse{A,B}:
/// state 0x10/0x11), the function returns `Some(0)` — but the caller MUST
/// also clear the bridge-direction flag, set `IsoTileTypeIndex = -1`, fire
/// `UpdateAdjacentBridges`, and zone-refresh. Body-cell driver detects this
/// via `(prev_state.is_partial_collapse() && phase.is_collapse() && next == 0)`.
///
/// `_Low` variants are intentionally not a parameter: state transitions are
/// identical, so the same function serves both. Overlay propagation (§11.2 +
/// `pick_destruction_overlay`) is what distinguishes HIGH from LOW.
pub fn apply_ramp_transition(current_state: u8, axis: Axis, phase: Phase) -> Option<u8> {
    match (axis, phase, current_state) {
        // --- NS axis (state 0..=8) ---
        // NS_DamageA: 0..=3 → 4, 5 → 6
        (Axis::NS, Phase::DamageA, 0..=3) => Some(4),
        (Axis::NS, Phase::DamageA, 5) => Some(6),
        // NS_DamageB: 0..=3 → 5, 4 → 6
        (Axis::NS, Phase::DamageB, 0..=3) => Some(5),
        (Axis::NS, Phase::DamageB, 4) => Some(6),
        // NS_CollapseA: 0..=6 → 7, 8 → 0 (collapse-final)
        (Axis::NS, Phase::CollapseA, 0..=6) => Some(7),
        (Axis::NS, Phase::CollapseA, 8) => Some(0),
        // NS_CollapseB: 0..=6 → 8, 7 → 0 (collapse-final)
        (Axis::NS, Phase::CollapseB, 0..=6) => Some(8),
        (Axis::NS, Phase::CollapseB, 7) => Some(0),

        // --- EW axis (state 9..=17 / 0x09..=0x11) ---
        // EW_DamageA: 9..=12 → 0x0E, 0x0D → 0x0F
        (Axis::EW, Phase::DamageA, 9..=12) => Some(0x0E),
        (Axis::EW, Phase::DamageA, 0x0D) => Some(0x0F),
        // EW_DamageB: 9..=12 → 0x0D, 0x0E → 0x0F
        (Axis::EW, Phase::DamageB, 9..=12) => Some(0x0D),
        (Axis::EW, Phase::DamageB, 0x0E) => Some(0x0F),
        // EW_CollapseA: 9..=15 → 0x11, 0x10 → 0 (collapse-final)
        (Axis::EW, Phase::CollapseA, 9..=15) => Some(0x11),
        (Axis::EW, Phase::CollapseA, 0x10) => Some(0),
        // EW_CollapseB: 9..=15 → 0x10, 0x11 → 0 (collapse-final)
        (Axis::EW, Phase::CollapseB, 9..=15) => Some(0x10),
        (Axis::EW, Phase::CollapseB, 0x11) => Some(0),

        // No defined transition.
        _ => None,
    }
}

/// Pick the next overlay byte for a destroying bridge cell. Mirrors
/// `ApplyBridgeDestruction_NS_High @ 0x57E7A0` and `_EW_High @ 0x57ED00`
/// (HIGH §11.2). Indexed by the result of `CheckBridgeNeighbors_*` —
/// i.e., a small integer encoding which adjacent cells still hold bridge
/// overlay. Distinct from `apply_ramp_transition` which handles state
/// (CellClass+0x11E); this one writes the visible overlay byte (+0x44).
///
/// `0xFF` in the table represents the binary's `-1` sentinel ("no
/// transition for this neighbor pattern" — leave overlay alone).
pub fn pick_destruction_overlay(
    neighbor_check: u8,
    axis: Axis,
    is_high_bridge: bool,
) -> Option<u8> {
    if neighbor_check >= 16 {
        return None;
    }
    let table: &[u8; 16] = match (axis, is_high_bridge) {
        (Axis::NS, true) => &DESTRUCTION_OVERLAY_HIGH_NS,
        (Axis::EW, true) => &DESTRUCTION_OVERLAY_HIGH_EW,
        (Axis::NS, false) => &DESTRUCTION_OVERLAY_LOW_NS,
        (Axis::EW, false) => &DESTRUCTION_OVERLAY_LOW_EW,
    };
    let val = table[neighbor_check as usize];
    if val == 0xFF { None } else { Some(val) }
}

/// HIGH NS destruction overlay table per HIGH §11.2 (`ApplyBridgeDestruction_NS_High`
/// @ `0x57E7A0`). Indexed by `CheckBridgeNeighbors_EW_High` result.
/// All 16 entries verified live byte-for-byte (indices 11..=15 explicitly
/// initialized to `0xffffffff` in the function prologue — no fall-through).
static DESTRUCTION_OVERLAY_HIGH_NS: [u8; 16] = [
    0xFF, 0xD2, 0xD5, 0xFF, 0xD1, 0xD3, 0xD5, 0xFF, 0xD4, 0xD4, 0xE7, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// HIGH EW destruction overlay table per HIGH §11.2 (`ApplyBridgeDestruction_EW_High`
/// @ `0x57ED00`). Indexed by `CheckBridgeNeighbors_NS_High` result.
static DESTRUCTION_OVERLAY_HIGH_EW: [u8; 16] = [
    0xFF, 0xDB, 0xDE, 0xFF, 0xDA, 0xDC, 0xDE, 0xFF, 0xDD, 0xDD, 0xE8, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// LOW NS destruction overlay table per `ApplyBridgeDestruction_NS_Low`
/// @ `0x0057DD50` (verified live, see HIGH_BRIDGE_DAMAGE_STATE_MACHINE_GHIDRA_REPORT.md
/// §11.2-LOW). Indexed by `CheckBridgeNeighbors_EW_Low` result. Final
/// destroyed byte = `0x64`. Outer overlay gate: `0x4A..=0x65`.
/// Progressive intermediates (handled by caller, not table):
/// `0x5C → 0x5D`, `0x5E → 0x5F`.
static DESTRUCTION_OVERLAY_LOW_NS: [u8; 16] = [
    0xFF, 0x4F, 0x52, 0xFF, 0x4E, 0x50, 0x52, 0xFF, 0x51, 0x51, 0x64, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// LOW EW destruction overlay table per `ApplyBridgeDestruction_EW_Low`
/// @ `0x0057E2A0` (verified live). Indexed by `CheckBridgeNeighbors_NS_Low`
/// result. Final destroyed byte = `0x65`. Outer overlay gate: `0x4A..=0x65`.
/// Progressive intermediates: `0x60 → 0x61`, `0x62 → 0x63`.
static DESTRUCTION_OVERLAY_LOW_EW: [u8; 16] = [
    0xFF, 0x58, 0x5B, 0xFF, 0x57, 0x59, 0x5B, 0xFF, 0x5A, 0x5A, 0x65, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
];

/// Per-cell action emitted by `set_bridge_direction` walker. The orchestrator
/// in `world::bridge_orchestrator::apply_bridge_damage_events` consumes these
/// and dispatches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellAction {
    /// Cell receives `BlowUpBridge` (kill ground, Limbo bridge-deck, debris).
    /// Destruction path slots 0, 1, 2, 4 (binary cells 1, 2, 3, 5).
    BlowUpBridge,
    /// Cell receives flag-only update — no BlowUpBridge. Slot 3 (binary
    /// cell 4) and slot 5 (binary cell 6) on destruction path.
    FlagOnly,
}

/// Result from `set_bridge_direction` walker. Each entry is one cell + its
/// action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetBridgeDirectionResult {
    pub actions: Vec<((u16, u16), usize, CellAction)>,
    /// Present only when this outcome represents an actual native
    /// `SetBridgeDirection_*` group transaction. Other bridge walkers may emit
    /// BlowUpBridge actions without that setter and therefore leave it absent.
    pub flag_stamp: Option<crate::map::bridge_facts::BridgeFlagStamp>,
}

/// Emit the per-cell action list for an anchor span. Mirrors binary's
/// `SetBridgeDirection_NESW @ 0x47E040`.
///
/// `set == false` is the destruction path (4 BlowUpBridge calls + 1–2
/// flag-only). `set == true` is the build/intact path (no BlowUpBridge —
/// flag writes only). Tier 2 only consumes destruction path; build path
/// is exercised by map-load anchor walker construction (Task 7).
pub fn set_bridge_direction(span: &AnchorSpan, set: bool) -> SetBridgeDirectionResult {
    let mut actions = Vec::with_capacity(6);
    for (slot, cell) in span.iter_cells() {
        let action = if !set {
            // `CellClass::BlowUpBridge` 0x0047DD70 is what a BlowUpBridge slot
            // means: gated on `g_IsMapEditor == 0`, it walks the cell's
            // FirstObject list calling vtable+0x16C with `RulesClass+0xFA8`,
            // walks the AltObject list calling vtable+0xEC, appends the coord
            // to the global at 0x0087F8C0, and then — only when
            // `RulesClass+0x168 > 0` and a `RandomRanged(0, 0x7FFFFFFE)` roll
            // lands under 0.95, draws five or six MORE times, i.e. six or
            // seven in total including the gate roll. Call sites:
            // 0x0047DE54 (gate), 0x0047DEC6 (x jitter), 0x0047DF04 (y
            // jitter), 0x0047DF43 (the < 0.5 test), 0x0047DF91 (first anim
            // index, only on the < 0.5 branch), 0x0047DFE1
            // (`RandomRanged(1, 5)`) and 0x0047E004 (second anim index).
            // Those draws are lockstep-visible, so the count matters as
            // much as the anims.
            // Destruction path: slots 0, 1, 2, 4 = BlowUpBridge; 3, 5 = FlagOnly.
            if AnchorSpan::BLOW_UP_SLOTS.contains(&slot) {
                CellAction::BlowUpBridge
            } else {
                CellAction::FlagOnly
            }
        } else {
            // Build path: every cell is FlagOnly (no BlowUpBridge). Used by
            // map-load construction.
            CellAction::FlagOnly
        };
        actions.push((cell, slot, action));
    }
    SetBridgeDirectionResult {
        actions,
        flag_stamp: Some(crate::map::bridge_facts::BridgeFlagStamp::new(
            span.anchor,
            span.direction as u8,
            set,
        )),
    }
}

/// Outcome of one perpendicular `UpdateRamp_*` call. One Rust function stands
/// in for all sixteen native ramp updaters, which are compiled twins differing
/// only in the direction constant their caller passes:
///
/// HIGH — `UpdateRamp_NS_DamageA` 0x00572230, `NS_DamageB` 0x00572330,
/// `NS_CollapseA` 0x00572440, `NS_CollapseB` 0x005727E0,
/// `EW_DamageA` 0x00572B80, `EW_DamageB` 0x00572C90,
/// `EW_CollapseA` 0x00572DA0, `EW_CollapseB` 0x00573170.
/// LOW — `NS_DamageA` 0x0056ED40, `NS_DamageB` 0x0056EE40,
/// `NS_CollapseA` 0x0056EF50, `NS_CollapseB` 0x0056F2F0,
/// `EW_DamageA` 0x0056F690, `EW_DamageB` 0x0056F7A0,
/// `EW_CollapseA` 0x0056F8B0, `EW_CollapseB` 0x0056FC80.
///
/// 0x00572230 decompiled 2026-08-19. It steps one cell along
/// `g_DirectionOffsets[dir & 7]`, then does two independent things:
/// 1. If the target carries `+0x140` bit 0x80, promote its `+0x11E` state
///    byte: `< 4` becomes 4, `== 5` becomes 6. **This is the part modelled
///    here.**
/// 2. Unconditionally, map `cell+0x38 - g_BridgeSet_TileSetBase + 1` against
///    three runtime tile-class constants and either call
///    `MapClass::ToggleBridgePavement` 0x0056E990 or
///    `MapClass::FloodFillIsoTileType`. The damaged-TMP
///    `ToggleBridgePavement` case is modelled below; the alternate iso-tile
///    repaint remains the narrower residual pinned in bridge-state tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RampOutcome {
    /// True if this frame or any recursive same-helper frame mutated modeled
    /// damage state or bridgehead tile class.
    pub state_changed: bool,
    /// Ordered cells whose TMP damaged selector changed through
    /// `MapClass::ToggleBridgePavement @ 0x0056E990`.
    pub damaged_variant_cells: Vec<(u16, u16)>,
    /// Ordered native `SetBridgeDirection_*` calls made inside this ramp
    /// helper, including every recursive same-helper invocation. Recursive
    /// calls remain in call order and duplicates are never removed. This is an
    /// ephemeral execution transcript, never snapshot authority.
    pub setter_transcript: Vec<crate::map::bridge_facts::BridgeFlagStamp>,
}

/// Compute the perpendicular-walk direction for a body-driver UpdateRamp call.
/// A-side and B-side perpendiculars per `[GHIDRA 0x576BA0]` body branch:
/// NS axis: A → E (dir 2), B → W (dir 6).
/// EW axis: A → S (dir 4), B → N (dir 0).
fn perpendicular_direction(axis: Axis, phase: Phase) -> Direction {
    let is_a_side = matches!(phase, Phase::DamageA | Phase::CollapseA);
    match (axis, is_a_side) {
        (Axis::NS, true) => Direction::E,
        (Axis::NS, false) => Direction::W,
        (Axis::EW, true) => Direction::S,
        (Axis::EW, false) => Direction::N,
    }
}

/// Walk one perpendicular cell from `anchor_pos` and fire the appropriate
/// per-target-role side effects, mirroring the reference UpdateRamp helpers.
///
/// - **Live `CellClass+0x140 & 0x80` target**: state-byte transition
///   (`apply_ramp_transition`) regardless of Rust's persistent topology role.
/// - **Anchor / Bridgehead role with a current middle tile**: tile-class transition
///   (`apply_anchor_class_transition`). This is the mechanism by which the
///   body-damage cascade shows the intermediate variant on a neighboring
///   bridgehead.
/// - **Matching raw pavement tiles**: independently update the connected
///   terrain's raw damage flag, including cells without bridge-runtime entries.
/// `is_high_bridge` selects the native concrete or Wood tile-set base.
#[cfg(test)]
pub fn update_ramp_perpendicular(
    state: &mut BridgeRuntimeState,
    anchor_pos: (u16, u16),
    axis: Axis,
    phase: Phase,
    is_high_bridge: bool,
    terrain: &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
) -> RampOutcome {
    let mut live_flags = terrain.bridge_flag_execution_state();
    update_ramp_perpendicular_with_flags(
        state,
        anchor_pos,
        axis,
        phase,
        is_high_bridge,
        terrain,
        &mut live_flags,
    )
}

pub(crate) fn update_ramp_perpendicular_with_flags(
    state: &mut BridgeRuntimeState,
    anchor_pos: (u16, u16),
    axis: Axis,
    phase: Phase,
    is_high_bridge: bool,
    terrain: &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
    live_flags: &mut crate::map::resolved_terrain::CellClassBridgeFlagState,
) -> RampOutcome {
    let dir = perpendicular_direction(axis, phase);
    update_ramp_perpendicular_recursive(
        state,
        anchor_pos,
        axis,
        phase,
        is_high_bridge,
        terrain,
        dir,
        live_flags,
    )
}

/// One native same-helper frame. Every recursive call keeps `dir` unchanged
/// and advances its anchor to this frame's target, exactly like the eight
/// CollapseA/B helpers in each low/high family. There is no depth constant:
/// the cardinal step is monotonic and recursion only continues from an
/// existing real cell in the finite runtime grid. Every frame still performs
/// native's fixed-stride GetCell before deciding whether the returned cell can
/// continue: a missing/off-grid lookup restamps the shared dummy, then ends the
/// represented setter path because its +0x11E/tile selectors are not modeled.
fn update_ramp_perpendicular_recursive(
    state: &mut BridgeRuntimeState,
    anchor_pos: (u16, u16),
    axis: Axis,
    phase: Phase,
    is_high_bridge: bool,
    terrain: &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
    dir: Direction,
    live_flags: &mut crate::map::resolved_terrain::CellClassBridgeFlagState,
) -> RampOutcome {
    use crate::sim::bridge_state::{BridgeCellRole, BridgeheadAnchorClass};

    let (dx, dy) = dir.offset();
    let target_x = anchor_pos.0 as i32 + dx;
    let target_y = anchor_pos.1 as i32 + dy;
    let target_pos =
        match crate::sim::cell_rect::get_cellclass_fallback(Some(terrain), target_x, target_y) {
            // MapClass applies signed packed/fixed-stride indexing before this
            // helper reads CellClass state. Retain the resolved cell's canonical
            // coordinate rather than re-casting the requested components.
            crate::sim::cell_rect::CellRef::Real(cell) => (cell.rx, cell.ry),
            // Native GetCell has already stamped the one shared dummy identity at
            // this exact call point. Its +0x11E/tile selectors remain unmodeled,
            // so no represented transition or setter follows from this frame.
            crate::sim::cell_rect::CellRef::Dummy { .. } => {
                return RampOutcome {
                    state_changed: false,
                    damaged_variant_cells: Vec::new(),
                    setter_transcript: Vec::new(),
                };
            }
        };

    // Snapshot target read (avoids borrow conflict with subsequent mut access).
    let Some(target_cell) = state.cell(target_pos.0, target_pos.1).copied() else {
        let changed = apply_perpendicular_pavement(terrain, target_pos, (target_x as i16, target_y as i16), axis, phase, is_high_bridge);
        return RampOutcome {
            state_changed: !changed.is_empty(),
            damaged_variant_cells: changed,
            setter_transcript: Vec::new(),
        };
    };

    let mut local_state_byte_changed = false;
    let mut local_tile_class_changed = false;
    let mut recursive_state_changed = false;
    let mut damaged_variant_cells = Vec::new();
    let mut setter_transcript = Vec::new();

    // Native state-byte writes are gated only by the selected live
    // CellClass+0x140 anchor bit. BridgeCellRole is persistent Rust topology
    // metadata and must not stand in for this setter-mutable flag.
    if live_flags.flags_at(target_pos) & crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF != 0
        && let Some(target_axis) = target_cell.axis
    {
        let current_byte = target_cell.damage_state.to_state_byte(target_axis);
        if let Some(next_byte) = apply_ramp_transition(current_byte, axis, phase) {
            // Decode next byte. Per `apply_ramp_transition` docstring,
            // next_byte == 0 fires only for the collapse-final case
            // (state 7/8/0x10/0x11 + matching CollapseA/B phase) and
            // means Destroyed in our model.
            let next_state = if next_byte == 0 {
                DamageState::Destroyed
            } else {
                match DamageState::from_state_byte(next_byte) {
                    Some(s) => s,
                    None => {
                        return RampOutcome {
                            state_changed: false,
                            damaged_variant_cells: Vec::new(),
                            setter_transcript: Vec::new(),
                        };
                    }
                }
            };
            if next_byte == 0 {
                // Collapse-final state branch order is recursive helper,
                // current SetBridgeDirection, then current +0x11E clear.
                let recursive = update_ramp_perpendicular_recursive(
                    state,
                    target_pos,
                    axis,
                    phase,
                    is_high_bridge,
                    terrain,
                    dir,
                    live_flags,
                );
                recursive_state_changed |= recursive.state_changed;
                damaged_variant_cells.extend(recursive.damaged_variant_cells);
                setter_transcript.extend(recursive.setter_transcript);

                // gamemd-derived collapse-final calls, all with set=false:
                // NS low/high CollapseA/B 0x56EF50/0x56F2F0 and
                // 0x572440/0x5727E0 call direction 0; EW low/high
                // 0x56F8B0/0x56FC80 and 0x572DA0/0x573170 call direction 6.
                // The low NWSE and high NESW setters are byte-identical for
                // the represented CellClass+0x140 0x1180 subset.
                let setter_direction = match axis {
                    Axis::NS => Direction::N,
                    Axis::EW => Direction::W,
                };
                let stamp = crate::map::bridge_facts::BridgeFlagStamp::new(
                    target_pos,
                    setter_direction as u8,
                    false,
                );
                // Native SetBridgeDirection mutates +0x140 before the
                // later independent AboutToFall recursion. Update the
                // transaction-local live seam now, then record the same
                // call for terrain/authority projection.
                live_flags.apply_stamp(stamp);
                setter_transcript.push(stamp);
            }
            if let Some(cell_mut) = state.cell_mut(target_pos.0, target_pos.1) {
                cell_mut.damage_state = next_state;
                local_state_byte_changed = true;
            }
        }
    }

    // The native tile branch selects actual middle-family IDs. A pavement
    // endpoint can also carry an Anchor/Bridgehead role; that role must not
    // turn its bit2000-only change into a middle-tile display override.
    let middle_tile = terrain.high_bridge_rim_tiles().is_some_and(|keys| {
        let base = if is_high_bridge { keys.base } else { terrain.wood_bridge_set_base() };
        terrain.cell(target_pos.0, target_pos.1).is_some_and(|cell| {
            let relative = cell.final_tile_index.wrapping_sub(base).wrapping_add(1);
            !perpendicular_pavement_tiles(keys, axis, phase).contains(&relative)
                && (0..4).any(|variant| relative == keys.middle[usize::from(axis == Axis::EW)].wrapping_add(variant))
        })
    });
    if middle_tile {
    match target_cell.role {
        BridgeCellRole::Anchor => {
            // The independent tile-class +3 branch recursively calls the same
            // helper before its footprint/final +3 write. It still runs after
            // a collapse-final state recursion and current setter, so both
            // recursion paths must be retained in the transcript.
            if matches!(phase, Phase::CollapseA | Phase::CollapseB)
                && target_cell.bridgehead_anchor_class == BridgeheadAnchorClass::AboutToFall
            {
                let recursive = update_ramp_perpendicular_recursive(
                    state,
                    target_pos,
                    axis,
                    phase,
                    is_high_bridge,
                    terrain,
                    dir,
                    live_flags,
                );
                recursive_state_changed |= recursive.state_changed;
                damaged_variant_cells.extend(recursive.damaged_variant_cells);
                setter_transcript.extend(recursive.setter_transcript);
            }

            // Tile-class branch: asymmetric A/B progression on the anchor's
            // bridgehead_anchor_class.
            let new_class =
                apply_anchor_class_transition(target_cell.bridgehead_anchor_class, phase);
            if new_class != target_cell.bridgehead_anchor_class
                && let Some(cell_mut) = state.cell_mut(target_pos.0, target_pos.1)
            {
                cell_mut.bridgehead_anchor_class = new_class;
                local_tile_class_changed = true;
            }
        }
        BridgeCellRole::Bridgehead => {
            // The role selects this tile-class write only. A state-byte
            // transition above remains possible only when the independently
            // selected live CellClass carries raw anchor bit 0x80.
            if matches!(phase, Phase::CollapseA | Phase::CollapseB)
                && target_cell.bridgehead_anchor_class == BridgeheadAnchorClass::AboutToFall
            {
                let recursive = update_ramp_perpendicular_recursive(
                    state,
                    target_pos,
                    axis,
                    phase,
                    is_high_bridge,
                    terrain,
                    dir,
                    live_flags,
                );
                recursive_state_changed |= recursive.state_changed;
                damaged_variant_cells.extend(recursive.damaged_variant_cells);
                setter_transcript.extend(recursive.setter_transcript);
            }
            let new_class =
                apply_anchor_class_transition(target_cell.bridgehead_anchor_class, phase);
            if new_class != target_cell.bridgehead_anchor_class
                && let Some(cell_mut) = state.cell_mut(target_pos.0, target_pos.1)
            {
                cell_mut.bridgehead_anchor_class = new_class;
                local_tile_class_changed = true;
            }
        }
        _ => {}
    }

    }

    // Original high572230..573170 and low56ED40..570036 run this
    // raw-tile branch independently after the overlay branch/recursion.
    damaged_variant_cells.extend(apply_perpendicular_pavement(
        terrain, target_pos, (target_x as i16, target_y as i16), axis, phase, is_high_bridge,
    ));

    RampOutcome {
        state_changed: !damaged_variant_cells.is_empty() || recursive_state_changed
            || local_state_byte_changed
            || local_tile_class_changed,
        damaged_variant_cells,
        setter_transcript,
    }
}

/// Native pavement selection uses the callee's concrete/Wood base and signed
/// General keys. Structural bridge membership and overlay-state writes do not
/// gate56E990. The retained cell selects the branch; the stack coordinate is
/// passed to the new lookup inside56E990 (important for fixed-stride aliases).
fn apply_perpendicular_pavement(
    terrain: &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
    retained: (u16, u16),
    requested: (i16, i16),
    axis: Axis,
    phase: Phase,
    is_high: bool,
) -> Vec<(u16, u16)> {
    let Some(keys) = terrain.high_bridge_rim_tiles() else { return Vec::new() };
    let Some(cell) = terrain.cell(retained.0, retained.1) else { return Vec::new() };
    let base = if is_high { keys.base } else { terrain.wood_bridge_set_base() };
    let relative = cell.final_tile_index.wrapping_sub(base).wrapping_add(1);
    let tiles = perpendicular_pavement_tiles(keys, axis, phase);
    if tiles.contains(&relative) { terrain.apply_native_pavement(requested, true) }
    else { Vec::new() }
}

fn perpendicular_pavement_tiles(
    keys: crate::map::bridge_rim_tiles::HighBridgeRimTiles, axis: Axis, phase: Phase,
) -> [i32; 2] {
    let side_a = matches!(phase, Phase::DamageA | Phase::CollapseA);
    match (axis, side_a) {
        (Axis::NS, true) => keys.bottom_right,
        (Axis::NS, false) => keys.top_left,
        (Axis::EW, true) => keys.bottom_left,
        (Axis::EW, false) => keys.top_right,
    }
}

/// Asymmetric tile-class transition table for `update_ramp_perpendicular`.
///
/// Mirrors the per-phase tile-class writes of the reference UpdateRamp
/// helpers:
/// - **DamageA**: preserve Variant0 and Damaged; no-op on Variant1 and
///   AboutToFall.
/// - **DamageB**: progress Variant0 → Variant1 and Variant1 → Damaged;
///   no-op on Damaged and AboutToFall.
/// - **CollapseA / CollapseB**: advance Variant0 / Variant1 / Damaged to
///   Damaged; preserve AboutToFall in this legacy projection. Native572440
///   normalizes tile-base+1, so its absolute base+Middle+3 terminal write
///   produces relative Middle+4, outside the four-class model. Full56EB80
///   tile/level/subtile delivery remains open; this is not a native no-op.
///   The former `+3 → +3` interpretation omitted that +1 normalization.
pub(crate) fn apply_anchor_class_transition(
    current: crate::sim::bridge_state::BridgeheadAnchorClass,
    phase: Phase,
) -> crate::sim::bridge_state::BridgeheadAnchorClass {
    use crate::sim::bridge_state::BridgeheadAnchorClass as BC;
    match (current, phase) {
        // DamageA: preserve all.
        (BC::Variant0, Phase::DamageA) => BC::Variant0,
        (BC::Variant1, Phase::DamageA) => BC::Variant1,
        (BC::Damaged, Phase::DamageA) => BC::Damaged,
        (BC::AboutToFall, Phase::DamageA) => BC::AboutToFall,

        // DamageB: progress Variant0 → Variant1, Variant1 → Damaged.
        (BC::Variant0, Phase::DamageB) => BC::Variant1,
        (BC::Variant1, Phase::DamageB) => BC::Damaged,
        (BC::Damaged, Phase::DamageB) => BC::Damaged,
        (BC::AboutToFall, Phase::DamageB) => BC::AboutToFall,

        // CollapseA / CollapseB: advance to Damaged, preserve AboutToFall.
        (BC::Variant0, Phase::CollapseA | Phase::CollapseB) => BC::Damaged,
        (BC::Variant1, Phase::CollapseA | Phase::CollapseB) => BC::Damaged,
        (BC::Damaged, Phase::CollapseA | Phase::CollapseB) => BC::Damaged,
        (BC::AboutToFall, Phase::CollapseA | Phase::CollapseB) => BC::AboutToFall,
    }
}

/// Walk from a bridgehead cell to its anchor body cell. Returns the anchor
/// cell coord, or `None` if the start cell fails the per-axis parity /
/// upper-bound gate or the walk runs off the map.
///
/// Per the HIGH bridge damage state machine:
/// - **NS branch (start-cell gate):** reject `(h & 1) != 0` — odd heights
///   (h=5, h=7) absorb damage with no state change.
/// - **EW branch (start-cell gate):** reject `h > 4` — high-ramp peak
///   (h=0xC) and other oversized heights early-return.
/// - **Walk direction (NS):** `h < 4 → S`, `h == 4 → at anchor`, `h > 4 → N`.
/// - **Walk direction (EW):** `h < 2 → E`, `h == 2 → at anchor`, `h > 2 → W`.
/// - **Mid-walk parity:** none. The walk silently passes through odd-h
///   intermediates. (The previous Rust check was stricter than the
///   reference behavior and caused damage absorption on multi-cell ramps.)
///
/// Walk terminates when `height == target` (4 NS / 2 EW). The 16-iter cap
/// is an internal defensive bound — there is no equivalent cap in the
/// reference, but bridges aren't placed near map edges in practice.
///
/// `cell_height` should read `ResolvedTerrainCell.template_height` (the
/// TMP per-tile byte at offset 40, mirroring the reference's
/// `CellClass+0x11A`).
pub fn bridgehead_walk_to_anchor(
    start: (u16, u16),
    axis: Axis,
    cell_height: impl Fn((u16, u16)) -> Option<u8>,
    map_width: u16,
    map_height: u16,
) -> Option<(u16, u16)> {
    let target_height: u8 = match axis {
        Axis::NS => 4,
        Axis::EW => 2,
    };

    // Start-cell gate (parity check / upper-bound check). Only the START
    // cell is gated; mid-walk intermediates pass through.
    let start_h = cell_height(start)?;
    match axis {
        Axis::NS => {
            if start_h & 1 != 0 {
                return None;
            }
        }
        Axis::EW => {
            if start_h > 4 {
                return None;
            }
        }
    }
    if start_h == target_height {
        return Some(start);
    }

    let mut current = start;
    let mut h = start_h;
    for _ in 0..16 {
        // Walk direction is recomputed each iteration. Height converges
        // monotonically toward the target, so direction never flips in
        // practice, but the recompute matches the reference loop body.
        let dir = match axis {
            Axis::NS => {
                if h < 4 {
                    Direction::S
                } else {
                    Direction::N
                }
            }
            Axis::EW => {
                if h < 2 {
                    Direction::E
                } else {
                    Direction::W
                }
            }
        };
        let (dx, dy) = dir.offset();
        let nx = current.0 as i32 + dx;
        let ny = current.1 as i32 + dy;
        if nx < 0 || ny < 0 || nx as u16 >= map_width || ny as u16 >= map_height {
            return None;
        }
        current = (nx as u16, ny as u16);
        h = cell_height(current)?;
        // No mid-walk parity check.
        if h == target_height {
            return Some(current);
        }
    }
    None
}

/// Three cells receiving `BlowUpBridge` on bridgehead final-step collapse.
/// Geometry verified live `[GHIDRA 0x576BA0]` step-3 branch.
///
/// Body-axis-aligned 3-cell row (NOT perpendicular). Offset to which row /
/// column is chosen depends on `anchor_height`'s bit predicate:
///
/// | Axis | predicate                 | row geometry                                                       |
/// |------|---------------------------|---------------------------------------------------------------------|
/// | NS   | `h & 1 == 0` (even)       | column at `anchor.X`,    Y in `{anchor.Y-1, anchor.Y, anchor.Y+1}` |
/// | NS   | `h & 1 != 0` (odd)        | column at `anchor.X-1`,  Y in `{anchor.Y-1, anchor.Y, anchor.Y+1}` |
/// | EW   | `h < 5`                   | row    at `anchor.Y`,    X in `{anchor.X-1, anchor.X, anchor.X+1}` |
/// | EW   | `h >= 5`                  | row    at `anchor.Y-1`,  X in `{anchor.X-1, anchor.X, anchor.X+1}` |
///
/// Off-map cells return `None` and are skipped by the caller.
///
/// `anchor_height` is whatever the consumer of `bridgehead_walk_to_anchor`
/// uses for its closure (currently `ResolvedTerrainCell.template_height`).
pub fn bridgehead_blow_up_row(
    anchor_pos: (u16, u16),
    axis: Axis,
    anchor_height: u8,
    map_width: u16,
    map_height: u16,
) -> [Option<(u16, u16)>; 3] {
    let (anchor_x, anchor_y) = (anchor_pos.0 as i32, anchor_pos.1 as i32);
    let (col_x, row_y) = match axis {
        Axis::NS => {
            let x_offset = if anchor_height & 1 == 0 { 0 } else { -1 };
            (anchor_x + x_offset, anchor_y)
        }
        Axis::EW => {
            let y_offset = if anchor_height < 5 { 0 } else { -1 };
            (anchor_x, anchor_y + y_offset)
        }
    };
    let mut out: [Option<(u16, u16)>; 3] = [None; 3];
    for (i, delta) in [-1i32, 0, 1].iter().enumerate() {
        let (cx, cy) = match axis {
            Axis::NS => (col_x, row_y + delta),
            Axis::EW => (col_x + delta, row_y),
        };
        if cx >= 0 && cy >= 0 && (cx as u16) < map_width && (cy as u16) < map_height {
            out[i] = Some((cx as u16, cy as u16));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::bridge_state::{
        AnchorSpan, Axis, BridgeCellRole, BridgeRuntimeCell, BridgeRuntimeState, DamageState,
        Direction, Phase,
    };

    /// Build a minimal 20x20 flat terrain for `update_ramp_perpendicular`
    /// tests. has_damaged_data=false so the embedded flood-fill writer's
    /// gate fails — these tests assert only the `RampOutcome.state_changed`
    /// path, not the damaged-variant propagation (covered by the dedicated
    /// flood-fill tests in `bridge_state`).
    fn ramp_test_terrain() -> ResolvedTerrainGrid {
        let mut cells = Vec::with_capacity(20 * 20);
        for ry in 0..20u16 {
            for rx in 0..20u16 {
                cells.push(ResolvedTerrainCell {
                    rx,
                    ry,
                    source_tile_index: 0,
                    source_sub_tile: 0,
                    final_tile_index: 0,
                    final_sub_tile: 0,
                    is_wood_bridge_repair_tile: false,
                    level: 0,
                    filled_clear: false,
                    tileset_index: Some(0),
                    land_type: 0,
                    yr_cell_land_type: 0,
                    slope_type: 0,
                    template_height: 0,
                    render_offset_x: 0,
                    render_offset_y: 0,
                    terrain_class: TerrainClass::Clear,
                    speed_costs: SpeedCostProfile::default(),
                    is_water: false,
                    is_cliff_like: false,
                    height_in_pixels: 0,
                    variant: 0,
                    is_rough: false,
                    is_road: false,
                    accepts_smudge: false,
                    allows_tiberium: false,
                    has_ramp: false,
                    canonical_ramp: None,
                    ground_walk_blocked: false,
                    terrain_object_blocks: false,
                    terrain_object_occupation: None,
                    overlay_blocks: false,
                    overlay_zone_type: None,
                    outside_playfield: false,
                    zone_type: 0,
                    base_ground_walk_blocked: false,
                    base_build_blocked: false,
                    base_land_type: 0,
                    base_yr_cell_land_type: 0,
                    base_terrain_class: Default::default(),
                    base_speed_costs: Default::default(),
                    build_blocked: false,
                    has_bridge_deck: false,
                    bridge_walkable: false,
                    bridge_transition: false,
                    bridge_deck_level: 0,
                    bridge_layer: None,
                    bridge_facts: crate::map::bridge_facts::BridgeCellFacts::default(),
                    tube_index: None,
                    radar_left: [0, 0, 0],
                    radar_right: [0, 0, 0],
                    has_damaged_data: false,
                    bridgehead_anchor_class_at_load: None,
                });
            }
        }
        let mut terrain = ResolvedTerrainGrid::from_cells(20, 20, cells);
        terrain.test_set_high_bridge_rim_tiles(crate::map::bridge_rim_tiles::HighBridgeRimTiles::from_ini(
            0, b"[General]\nBridgeMiddle1=1\nBridgeMiddle2=1\nBridgeTopLeft1=11\nBridgeTopLeft2=12\nBridgeBottomRight1=13\nBridgeBottomRight2=14\nBridgeTopRight1=15\nBridgeTopRight2=16\nBridgeBottomLeft1=17\nBridgeBottomLeft2=18\n"));
        terrain
    }

    fn ramp_test_terrain_with_anchor_bits(coords: &[(u16, u16)]) -> ResolvedTerrainGrid {
        let mut terrain = ramp_test_terrain();
        for &(rx, ry) in coords {
            terrain.cell_mut(rx, ry).unwrap().bridge_facts.raw_flags |=
                crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
        }
        terrain
    }

    #[test]
    fn low_bridge_damage_step_ignores_non_bridge_overlay() {
        let out = low_bridge_overlay_damage_step_ra2(
            BridgeOverlayTriple {
                a: 1,
                center: 1234,
                b: 2,
            },
            50,
            150,
            999,
            1,
        );
        assert_eq!(out.reason, LowBridgeOverlayDamageReason::NotBridgeOverlay);
        assert!(!out.changed);
        assert_eq!(out.triple_out.center, 1234);
    }

    #[test]
    fn low_bridge_damage_step_applies_rng_gate() {
        let out = low_bridge_overlay_damage_step_ra2(
            BridgeOverlayTriple {
                a: 96,
                center: 96,
                b: 96,
            },
            10,
            150,
            999,
            10,
        );
        assert_eq!(out.reason, LowBridgeOverlayDamageReason::GateFailed);
        assert!(!out.changed);
    }

    #[test]
    fn low_bridge_damage_step_atom_damage_bypasses_gate() {
        let out = low_bridge_overlay_damage_step_ra2(
            BridgeOverlayTriple {
                a: 96,
                center: 96,
                b: 96,
            },
            999,
            150,
            999,
            150,
        );
        assert_eq!(out.reason, LowBridgeOverlayDamageReason::Changed);
        assert!(out.changed);
        assert_eq!(
            out.triple_out,
            BridgeOverlayTriple {
                a: 97,
                center: 97,
                b: 97,
            }
        );
    }

    #[test]
    fn low_bridge_damage_step_maps_wood_family() {
        let out = low_bridge_overlay_damage_step_ra2(
            BridgeOverlayTriple {
                a: 74,
                center: 74,
                b: 74,
            },
            2,
            150,
            999,
            1,
        );
        assert_eq!(out.triple_out.center, 89);

        let out = low_bridge_overlay_damage_step_ra2(
            BridgeOverlayTriple {
                a: 89,
                center: 89,
                b: 90,
            },
            2,
            150,
            999,
            1,
        );
        assert_eq!(out.triple_out.center, 101);
    }

    #[test]
    fn low_bridge_damage_step_maps_concrete_family_and_no_transition() {
        let out = low_bridge_overlay_damage_step_ra2(
            BridgeOverlayTriple {
                a: 227,
                center: 227,
                b: 227,
            },
            2,
            150,
            999,
            1,
        );
        assert_eq!(out.triple_out.center, 228);

        let no_change = low_bridge_overlay_damage_step_ra2(
            BridgeOverlayTriple {
                a: 223,
                center: 223,
                b: 223,
            },
            2,
            150,
            999,
            1,
        );
        assert_eq!(no_change.reason, LowBridgeOverlayDamageReason::NoTransition);
        assert!(!no_change.changed);
    }

    #[test]
    fn low_bridge_selector_rejects_non_bridge_overlay() {
        let out = low_bridge_connected_section_selector_yr(1, false, false);
        assert!(!out.handled);
        assert!(out.reason_not_bridge_overlay);
    }

    #[test]
    fn low_bridge_selector_uses_exact_anchor_policy() {
        let out = low_bridge_connected_section_selector_yr(74, false, false);
        assert_eq!(out.pattern, Some(LowBridgeConnectedPattern::A));
        assert_eq!(out.band, Some(LowBridgeConnectedBand::WoodBand1));
        assert_eq!(out.anchor, Some(LowBridgeConnectedAnchor::OppositeAdjacent));
        assert_eq!(out.neighbor_range_lo, Some(74));
        assert_eq!(out.neighbor_range_hi, Some(101));

        let out = low_bridge_connected_section_selector_yr(74, true, false);
        assert_eq!(out.anchor, Some(LowBridgeConnectedAnchor::Center));

        let out = low_bridge_connected_section_selector_yr(74, true, true);
        assert_eq!(out.anchor, Some(LowBridgeConnectedAnchor::PrimaryAdjacent));

        let out = low_bridge_connected_section_selector_yr(83, true, true);
        assert_eq!(out.band, Some(LowBridgeConnectedBand::WoodBand2));
        assert_eq!(
            out.anchor,
            Some(LowBridgeConnectedAnchor::ConnectedChainHelper)
        );

        let out = low_bridge_connected_section_selector_yr(205, false, false);
        assert_eq!(out.pattern, Some(LowBridgeConnectedPattern::B));
        assert_eq!(out.band, Some(LowBridgeConnectedBand::ConcreteBand1));
        assert_eq!(out.anchor, Some(LowBridgeConnectedAnchor::OppositeAdjacent));
        assert_eq!(out.neighbor_range_lo, Some(205));
        assert_eq!(out.neighbor_range_hi, Some(232));

        let out = low_bridge_connected_section_selector_yr(214, true, true);
        assert_eq!(out.band, Some(LowBridgeConnectedBand::ConcreteBand2));
        assert_eq!(
            out.anchor,
            Some(LowBridgeConnectedAnchor::ConnectedChainHelper)
        );
    }

    #[test]
    fn zone_connection_record_decodes_layout() {
        let record = [10, 0, 254, 255, 10, 0, 5, 0, 1, 0, 0, 0, 0, 0, 0, 0];
        let decoded = decode_zone_connection_record(&record);
        assert_eq!(decoded.cell_a, (10, -2));
        assert_eq!(decoded.cell_b, (10, 5));
        assert_eq!(decoded.flags, 1);
        assert_eq!(decoded.flags_byte8, 1);
        assert_eq!(decoded.skip_if_nonzero, 0);
    }

    #[test]
    fn zone_connection_match_uses_axis_aligned_segment_proximity() {
        let record = [10, 0, 254, 255, 10, 0, 5, 0, 1, 0, 0, 0, 0, 0, 0, 0];
        assert!(zone_connection_matches_cell(&record, (9, 0), 1));
        assert!(!zone_connection_matches_cell(&record, (8, 0), 1));
        assert!(!zone_connection_matches_cell(&record, (10, 6), 1));
    }

    #[test]
    fn zone_connection_match_respects_skip_flag() {
        let record = [10, 0, 254, 255, 10, 0, 5, 0, 1, 0, 0, 0, 1, 0, 0, 0];
        assert!(!zone_connection_matches_cell(&record, (10, 0), 1));
    }

    #[test]
    fn bridge_zone_policy_turns_off_when_on_bridge_false() {
        let out = get_cell_zone_id_bridge_policy_decision(
            BridgeZoneIdPolicyTarget::Yr1001,
            false,
            0x0100,
            -1,
        );
        assert_eq!(
            out,
            BridgeZoneIdPolicyDecision {
                use_bridge_path: false,
                call_bridge_remap_fallback: false,
                return_no_zone: false,
            }
        );
    }

    #[test]
    fn bridge_zone_policy_turns_off_when_bridge_bit_clear() {
        let out =
            get_cell_zone_id_bridge_policy_decision(BridgeZoneIdPolicyTarget::Ra21006, true, 0, -1);
        assert!(!out.use_bridge_path);
        assert!(!out.call_bridge_remap_fallback);
        assert!(!out.return_no_zone);
    }

    #[test]
    fn bridge_zone_policy_matches_ra2_and_yr_fallback_split() {
        let hit = get_cell_zone_id_bridge_policy_decision(
            BridgeZoneIdPolicyTarget::Ra21006,
            true,
            0x0100,
            3,
        );
        assert_eq!(
            hit,
            BridgeZoneIdPolicyDecision {
                use_bridge_path: true,
                call_bridge_remap_fallback: false,
                return_no_zone: false,
            }
        );

        let ra2_miss = get_cell_zone_id_bridge_policy_decision(
            BridgeZoneIdPolicyTarget::Ra21006,
            true,
            0x0100,
            -1,
        );
        assert_eq!(
            ra2_miss,
            BridgeZoneIdPolicyDecision {
                use_bridge_path: true,
                call_bridge_remap_fallback: false,
                return_no_zone: true,
            }
        );

        let yr_miss = get_cell_zone_id_bridge_policy_decision(
            BridgeZoneIdPolicyTarget::Yr1001,
            true,
            0x0100,
            -1,
        );
        assert_eq!(
            yr_miss,
            BridgeZoneIdPolicyDecision {
                use_bridge_path: true,
                call_bridge_remap_fallback: true,
                return_no_zone: false,
            }
        );
    }

    #[test]
    fn ramp_ns_damage_a_healthy_to_4() {
        for s in 0..=3 {
            assert_eq!(
                apply_ramp_transition(s, Axis::NS, Phase::DamageA),
                Some(4),
                "state {s}"
            );
        }
    }

    #[test]
    fn ramp_ns_damage_a_5_to_6() {
        assert_eq!(apply_ramp_transition(5, Axis::NS, Phase::DamageA), Some(6));
    }

    #[test]
    fn ramp_ns_damage_b_healthy_to_5() {
        for s in 0..=3 {
            assert_eq!(apply_ramp_transition(s, Axis::NS, Phase::DamageB), Some(5));
        }
    }

    #[test]
    fn ramp_ns_damage_b_4_to_6() {
        assert_eq!(apply_ramp_transition(4, Axis::NS, Phase::DamageB), Some(6));
    }

    #[test]
    fn ramp_ns_collapse_a_to_7() {
        for s in 0..=6 {
            assert_eq!(
                apply_ramp_transition(s, Axis::NS, Phase::CollapseA),
                Some(7)
            );
        }
    }

    #[test]
    fn ramp_ns_collapse_a_final_state_8_to_0() {
        // Collapse-final: caller must also clear bridge dir + IsoTileTypeIndex.
        assert_eq!(
            apply_ramp_transition(8, Axis::NS, Phase::CollapseA),
            Some(0)
        );
    }

    #[test]
    fn ramp_ns_collapse_b_to_8() {
        for s in 0..=6 {
            assert_eq!(
                apply_ramp_transition(s, Axis::NS, Phase::CollapseB),
                Some(8)
            );
        }
    }

    #[test]
    fn ramp_ns_collapse_b_final_state_7_to_0() {
        assert_eq!(
            apply_ramp_transition(7, Axis::NS, Phase::CollapseB),
            Some(0)
        );
    }

    #[test]
    fn ramp_ew_damage_a_healthy_to_e() {
        for s in 9..=12 {
            assert_eq!(
                apply_ramp_transition(s, Axis::EW, Phase::DamageA),
                Some(0x0E)
            );
        }
    }

    #[test]
    fn ramp_ew_damage_a_d_to_f() {
        assert_eq!(
            apply_ramp_transition(0x0D, Axis::EW, Phase::DamageA),
            Some(0x0F)
        );
    }

    #[test]
    fn ramp_ew_damage_b_healthy_to_d() {
        for s in 9..=12 {
            assert_eq!(
                apply_ramp_transition(s, Axis::EW, Phase::DamageB),
                Some(0x0D)
            );
        }
    }

    #[test]
    fn ramp_ew_damage_b_e_to_f() {
        assert_eq!(
            apply_ramp_transition(0x0E, Axis::EW, Phase::DamageB),
            Some(0x0F)
        );
    }

    #[test]
    fn ramp_ew_collapse_a_to_11() {
        for s in 9..=15 {
            assert_eq!(
                apply_ramp_transition(s, Axis::EW, Phase::CollapseA),
                Some(0x11)
            );
        }
    }

    #[test]
    fn ramp_ew_collapse_a_final_state_10_to_0() {
        assert_eq!(
            apply_ramp_transition(0x10, Axis::EW, Phase::CollapseA),
            Some(0)
        );
    }

    #[test]
    fn ramp_ew_collapse_b_to_10() {
        for s in 9..=15 {
            assert_eq!(
                apply_ramp_transition(s, Axis::EW, Phase::CollapseB),
                Some(0x10)
            );
        }
    }

    #[test]
    fn ramp_ew_collapse_b_final_state_11_to_0() {
        assert_eq!(
            apply_ramp_transition(0x11, Axis::EW, Phase::CollapseB),
            Some(0)
        );
    }

    #[test]
    fn ramp_undefined_combination_returns_none() {
        // EW phase on NS-range state, etc.
        assert_eq!(apply_ramp_transition(0, Axis::EW, Phase::DamageA), None);
        assert_eq!(apply_ramp_transition(15, Axis::NS, Phase::DamageA), None);
        // State outside both ranges.
        assert_eq!(apply_ramp_transition(0xFF, Axis::NS, Phase::DamageA), None);
    }

    #[test]
    fn destruction_overlay_high_ns_known_entries() {
        // Spot-check verified entries from HIGH §11.2.
        assert_eq!(pick_destruction_overlay(1, Axis::NS, true), Some(0xD2));
        assert_eq!(pick_destruction_overlay(2, Axis::NS, true), Some(0xD5));
        assert_eq!(pick_destruction_overlay(4, Axis::NS, true), Some(0xD1));
        assert_eq!(pick_destruction_overlay(10, Axis::NS, true), Some(0xE7)); // final destroyed
    }

    #[test]
    fn destruction_overlay_high_ew_known_entries() {
        assert_eq!(pick_destruction_overlay(1, Axis::EW, true), Some(0xDB));
        assert_eq!(pick_destruction_overlay(2, Axis::EW, true), Some(0xDE));
        assert_eq!(pick_destruction_overlay(10, Axis::EW, true), Some(0xE8)); // final destroyed
    }

    #[test]
    fn destruction_overlay_unused_indices_return_none() {
        assert_eq!(pick_destruction_overlay(0, Axis::NS, true), None);
        assert_eq!(pick_destruction_overlay(3, Axis::NS, true), None);
        assert_eq!(pick_destruction_overlay(11, Axis::NS, true), None);
    }

    #[test]
    fn destruction_overlay_out_of_range_returns_none() {
        assert_eq!(pick_destruction_overlay(16, Axis::NS, true), None);
        assert_eq!(pick_destruction_overlay(0xFF, Axis::EW, true), None);
    }

    #[test]
    fn destruction_overlay_low_ns_known_entries() {
        // Verified from ApplyBridgeDestruction_NS_Low @ 0x0057DD50.
        assert_eq!(pick_destruction_overlay(1, Axis::NS, false), Some(0x4F));
        assert_eq!(pick_destruction_overlay(2, Axis::NS, false), Some(0x52));
        assert_eq!(pick_destruction_overlay(4, Axis::NS, false), Some(0x4E));
        assert_eq!(pick_destruction_overlay(10, Axis::NS, false), Some(0x64)); // final destroyed
    }

    #[test]
    fn destruction_overlay_low_ew_known_entries() {
        // Verified from ApplyBridgeDestruction_EW_Low @ 0x0057E2A0.
        assert_eq!(pick_destruction_overlay(1, Axis::EW, false), Some(0x58));
        assert_eq!(pick_destruction_overlay(2, Axis::EW, false), Some(0x5B));
        assert_eq!(pick_destruction_overlay(4, Axis::EW, false), Some(0x57));
        assert_eq!(pick_destruction_overlay(10, Axis::EW, false), Some(0x65)); // final destroyed
    }

    #[test]
    fn destruction_overlay_low_unused_indices_return_none() {
        // Slots 0/3/7/11..=15 unused in both NS and EW LOW tables.
        for i in [0, 3, 7, 11, 12, 13, 14, 15] {
            assert_eq!(
                pick_destruction_overlay(i, Axis::NS, false),
                None,
                "NS slot {i}"
            );
            assert_eq!(
                pick_destruction_overlay(i, Axis::EW, false),
                None,
                "EW slot {i}"
            );
        }
    }

    #[test]
    fn set_bridge_direction_destruction_emits_4_blow_up_actions() {
        let span = AnchorSpan {
            id: 1,
            anchor: (5, 5),
            cells: [
                Some((5, 5)),
                Some((6, 5)),
                Some((7, 5)),
                Some((8, 5)),
                Some((4, 5)),
                None,
            ],
            axis: Axis::NS,
            direction: Direction::E,
            damage_state: DamageState::Damaged,
            bridge_group_id: 1,
        };
        let result = set_bridge_direction(&span, false);
        let blow_ups = result
            .actions
            .iter()
            .filter(|(_, _, a)| matches!(a, CellAction::BlowUpBridge))
            .count();
        assert_eq!(blow_ups, 4);
        let flag_only = result
            .actions
            .iter()
            .filter(|(_, _, a)| matches!(a, CellAction::FlagOnly))
            .count();
        assert_eq!(flag_only, 1); // slot 3 (cell 4)
    }

    #[test]
    fn set_bridge_direction_build_emits_no_blow_up_actions() {
        let span = AnchorSpan {
            id: 1,
            anchor: (0, 0),
            cells: [Some((0, 0)), None, None, None, None, None],
            axis: Axis::NS,
            direction: Direction::E,
            damage_state: DamageState::Healthy { variant: 0 },
            bridge_group_id: 1,
        };
        let result = set_bridge_direction(&span, true);
        assert!(
            result
                .actions
                .iter()
                .all(|(_, _, a)| matches!(a, CellAction::FlagOnly))
        );
    }

    #[test]
    fn set_bridge_direction_includes_slot_5_only_when_present() {
        let span = AnchorSpan {
            id: 1,
            anchor: (5, 5),
            cells: [
                Some((5, 5)),
                Some((6, 5)),
                Some((7, 5)),
                Some((8, 5)),
                Some((4, 5)),
                Some((6, 5)), // hypothetical slot 5
            ],
            axis: Axis::NS,
            direction: Direction::W,
            damage_state: DamageState::Damaged,
            bridge_group_id: 1,
        };
        let result = set_bridge_direction(&span, false);
        let slot_5_action = result
            .actions
            .iter()
            .find(|(_, slot, _)| *slot == 5)
            .map(|(_, _, a)| *a);
        assert_eq!(slot_5_action, Some(CellAction::FlagOnly));
    }

    /// Build a minimal BridgeRuntimeState for update_ramp tests:
    /// anchors at (4,5), (5,5), (6,5), all NS axis, Healthy{variant: 0}.
    /// Uses `test_seed_cell` (Task 1 Step 5).
    fn make_perpendicular_test_state() -> BridgeRuntimeState {
        let mut state = BridgeRuntimeState::default();
        let template = BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 0,
            bridge_group_id: Some(1),
            damage_state: DamageState::Healthy { variant: 0 },
            axis: Some(Axis::NS),
            role: BridgeCellRole::Anchor,
            anchor_span_id: Some(1),
            overlay_byte: 0x18, // HIGH bridge anchor overlay
            bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
        };
        for (rx, ry) in [(4u16, 5u16), (5, 5), (6, 5)] {
            state.test_seed_cell(rx, ry, template);
        }
        state
    }

    #[test]
    fn update_ramp_perpendicular_ns_damage_a_anchor_target_transitions_to_4() {
        let mut state = make_perpendicular_test_state();
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(6, 5)]);
        let outcome =
            update_ramp_perpendicular(&mut state, (5, 5), Axis::NS, Phase::DamageA, true, &mut terrain);
        assert!(outcome.state_changed);
        let target = state.cell(6, 5).expect("E target");
        assert_eq!(target.damage_state, DamageState::Healthy { variant: 4 });
    }

    #[test]
    fn update_ramp_perpendicular_ns_damage_b_anchor_target_walks_west() {
        let mut state = make_perpendicular_test_state();
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(4, 5)]);
        let outcome =
            update_ramp_perpendicular(&mut state, (5, 5), Axis::NS, Phase::DamageB, true, &mut terrain);
        assert!(outcome.state_changed);
        let target = state.cell(4, 5).expect("W target");
        assert_eq!(target.damage_state, DamageState::Healthy { variant: 5 });
    }

    #[test]
    fn update_ramp_perpendicular_non_anchor_target_no_change() {
        let mut state = make_perpendicular_test_state();
        // Patch (6,5) to Body role (not Anchor).
        state.cell_mut(6, 5).unwrap().role = BridgeCellRole::Body;
        let outcome = update_ramp_perpendicular(
            &mut state,
            (5, 5),
            Axis::NS,
            Phase::DamageA,
            true,
            &mut ramp_test_terrain(),
        );
        assert!(!outcome.state_changed);
        assert_eq!(
            state.cell(6, 5).unwrap().damage_state,
            DamageState::Healthy { variant: 0 }
        );
    }

    #[test]
    fn gsi_04_01_update_ramp_perpendicular_negative_probe_restamps_dummy() {
        use crate::map::bridge_facts::{BRIDGE_FLAG_STRUCTURAL, BridgeStampSlot};

        let mut state = make_perpendicular_test_state();
        let mut terrain = ramp_test_terrain();
        terrain.test_set_dummy_cell_level_slope(2, 0);
        let dummy = terrain.shared_cell_dummy();
        dummy.apply_bridge_flag_slot(BridgeStampSlot::Anchor, true);
        // Anchor at (0, 0) calling NS DamageB → walks W → target x = -1 → out of bounds.
        let outcome =
            update_ramp_perpendicular(&mut state, (0, 0), Axis::NS, Phase::DamageB, true, &mut terrain);
        assert!(!outcome.state_changed);
        assert!(outcome.setter_transcript.is_empty());
        assert_eq!(dummy.snapshot().coord, (-1, 0));
        assert_ne!(
            dummy.snapshot().bridge_flags_0x1180 & BRIDGE_FLAG_STRUCTURAL,
            0,
            "mandatory GetCell restamps only the dummy coordinate"
        );
        let retained_target = crate::sim::projectile::ProjectileTarget::DummyCell;
        let observed_target = match retained_target {
            crate::sim::projectile::ProjectileTarget::DummyCell => {
                crate::sim::projectile::dummy_cell_target_coord(&dummy)
            }
            _ => unreachable!("fixture retains the shared dummy pointer kind"),
        };
        assert_eq!(
            observed_target,
            crate::sim::projectile::ProjectileCoord::new(-128, 128, 2 * 104 + 416),
            "a retained DummyCell target observes the ramp helper's live restamp and structural height"
        );
    }

    #[test]
    fn gsi_04_01_update_ramp_perpendicular_unallocated_probe_restamps_dummy() {
        use crate::sim::bridge_state::BridgeheadAnchorClass;

        let mut state = perpendicular_neighbor_state(
            (3, 2),
            BridgeCellRole::Anchor,
            BridgeheadAnchorClass::Variant0,
        );
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(3, 2)]);
        terrain.test_set_native_allocated_cells(&[(0, 0)]);
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(8, 9);

        let outcome =
            update_ramp_perpendicular(&mut state, (2, 2), Axis::NS, Phase::DamageA, true, &mut terrain);

        assert!(!outcome.state_changed);
        assert!(outcome.setter_transcript.is_empty());
        assert_eq!(dummy.snapshot().coord, (3, 2));
        assert_eq!(
            state.cell(3, 2).unwrap().damage_state,
            DamageState::Healthy { variant: 0 },
            "an unallocated CellClass cannot borrow the runtime cache cell's modeled selectors"
        );
    }

    #[test]
    fn gsi_04_01_update_ramp_perpendicular_real_fixed_alias_preserves_dummy() {
        use crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF;
        use crate::sim::bridge_state::BridgeheadAnchorClass;

        let template = ramp_test_terrain()
            .cell(0, 0)
            .expect("flat template cell")
            .clone();
        let cells = (0..512u16)
            .map(|rx| {
                let mut cell = template.clone();
                cell.rx = rx;
                cell.ry = 0;
                cell
            })
            .collect();
        let mut terrain = ResolvedTerrainGrid::from_cells(512, 1, cells);
        terrain.test_set_native_allocated_cells(&[(511, 0)]);
        terrain.cell_mut(511, 0).unwrap().bridge_facts.raw_flags |= BRIDGE_FLAG_ANCHOR_SELF;
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(7, 8);
        terrain.test_set_dummy_cell_level_slope(2, 0);
        let dummy_before = dummy.snapshot();
        let mut state = perpendicular_neighbor_state(
            (511, 0),
            BridgeCellRole::Anchor,
            BridgeheadAnchorClass::Variant0,
        );

        // Requested (-1,1) aliases fixed slot 511, whose canonical coordinate
        // is (511,0). Native returns that real CellClass without stamping the
        // shared dummy.
        let outcome =
            update_ramp_perpendicular(&mut state, (0, 1), Axis::NS, Phase::DamageB, true, &mut terrain);

        assert!(outcome.state_changed);
        assert!(outcome.setter_transcript.is_empty());
        assert_eq!(
            state.cell(511, 0).unwrap().damage_state,
            DamageState::Healthy { variant: 5 }
        );
        assert_eq!(dummy.snapshot(), dummy_before);
    }

    #[test]
    fn update_ramp_perpendicular_collapse_final_target_to_destroyed() {
        let mut state = make_perpendicular_test_state();
        state.cell_mut(6, 5).unwrap().damage_state = DamageState::PartialCollapseB;
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(6, 5)]);
        let outcome = update_ramp_perpendicular(
            &mut state,
            (5, 5),
            Axis::NS,
            Phase::CollapseA,
            true,
            &mut terrain,
        );
        assert!(outcome.state_changed);
        let target = state.cell(6, 5).expect("E target");
        assert_eq!(target.damage_state, DamageState::Destroyed);
    }

    #[test]
    fn update_ramp_perpendicular_ew_collapse_walks_south() {
        // Build a separate fixture with EW-axis anchors at (5,4), (5,5), (5,6).
        // (The default fixture is NS-axis at (4,5)/(5,5)/(6,5); using it for an
        // EW test would require re-seeding cells AND axes, so we just build
        // fresh.)
        let mut state = BridgeRuntimeState::default();
        let template = BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 0,
            bridge_group_id: Some(1),
            damage_state: DamageState::Healthy { variant: 0 },
            axis: Some(Axis::EW),
            role: BridgeCellRole::Anchor,
            anchor_span_id: Some(1),
            overlay_byte: 0x18,
            bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
        };
        for (rx, ry) in [(5u16, 4u16), (5, 5), (5, 6)] {
            state.test_seed_cell(rx, ry, template);
        }
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(5, 6)]);
        // EW CollapseA → walks S → target (5, 6).
        // Target state byte 9 (Healthy{0} EW) → apply_ramp_transition EW
        // CollapseA: 9..=15 → 0x11 = PartialCollapseA.
        let outcome = update_ramp_perpendicular(
            &mut state,
            (5, 5),
            Axis::EW,
            Phase::CollapseA,
            true,
            &mut terrain,
        );
        assert!(outcome.state_changed);
        let target = state.cell(5, 6).expect("S target");
        assert_eq!(target.damage_state, DamageState::PartialCollapseA);
    }

    /// Build a state with an Anchor at (2,2) and a perpendicular neighbor
    /// at `neighbor_pos` of the given role and tile-class. NS axis fixed —
    /// DamageA walks E (+X), DamageB walks W (-X).
    fn perpendicular_neighbor_state(
        neighbor_pos: (u16, u16),
        neighbor_role: BridgeCellRole,
        neighbor_class: crate::sim::bridge_state::BridgeheadAnchorClass,
    ) -> BridgeRuntimeState {
        let mut state = BridgeRuntimeState::default();
        // Anchor at (2, 2).
        state.test_seed_cell(
            2,
            2,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 0,
                bridge_group_id: Some(1),
                damage_state: DamageState::Healthy { variant: 0 },
                axis: Some(Axis::NS),
                role: BridgeCellRole::Anchor,
                anchor_span_id: Some(1),
                overlay_byte: 0x18,
                bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
            },
        );
        // Neighbor.
        state.test_seed_cell(
            neighbor_pos.0,
            neighbor_pos.1,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 0,
                bridge_group_id: Some(1),
                damage_state: DamageState::Healthy { variant: 0 },
                axis: Some(Axis::NS),
                role: neighbor_role,
                anchor_span_id: if matches!(neighbor_role, BridgeCellRole::Anchor) {
                    Some(1)
                } else {
                    None
                },
                overlay_byte: 0,
                bridgehead_anchor_class: neighbor_class,
            },
        );
        state
    }

    #[test]
    fn update_ramp_perpendicular_damageb_progresses_bridgehead_variant0_to_variant1() {
        use crate::sim::bridge_state::BridgeheadAnchorClass;
        // NS DamageB walks W (-X) → neighbor at (1, 2).
        let mut state = perpendicular_neighbor_state(
            (1, 2),
            BridgeCellRole::Bridgehead,
            BridgeheadAnchorClass::Variant0,
        );
        let outcome = update_ramp_perpendicular(
            &mut state,
            (2, 2),
            Axis::NS,
            Phase::DamageB,
            true,
            &mut ramp_test_terrain(),
        );
        assert!(outcome.state_changed);
        let neighbor = state.cell(1, 2).unwrap();
        assert_eq!(
            neighbor.bridgehead_anchor_class,
            BridgeheadAnchorClass::Variant1,
            "DamageB on Variant0 bridgehead must progress to Variant1",
        );
        // damage_state must NOT be modified on Bridgehead targets.
        assert!(matches!(neighbor.damage_state, DamageState::Healthy { .. }));
    }

    #[test]
    fn update_ramp_perpendicular_damageb_progresses_variant1_to_damaged() {
        use crate::sim::bridge_state::BridgeheadAnchorClass;
        let mut state = perpendicular_neighbor_state(
            (1, 2),
            BridgeCellRole::Bridgehead,
            BridgeheadAnchorClass::Variant1,
        );
        let outcome = update_ramp_perpendicular(
            &mut state,
            (2, 2),
            Axis::NS,
            Phase::DamageB,
            true,
            &mut ramp_test_terrain(),
        );
        assert!(outcome.state_changed);
        let neighbor = state.cell(1, 2).unwrap();
        assert_eq!(
            neighbor.bridgehead_anchor_class,
            BridgeheadAnchorClass::Damaged
        );
    }

    #[test]
    fn update_ramp_perpendicular_damagea_preserves_bridgehead_variant0() {
        use crate::sim::bridge_state::BridgeheadAnchorClass;
        // NS DamageA walks E (+X) → neighbor at (3, 2).
        let mut state = perpendicular_neighbor_state(
            (3, 2),
            BridgeCellRole::Bridgehead,
            BridgeheadAnchorClass::Variant0,
        );
        let outcome = update_ramp_perpendicular(
            &mut state,
            (2, 2),
            Axis::NS,
            Phase::DamageA,
            true,
            &mut ramp_test_terrain(),
        );
        // DamageA preserves Variant0 → no change → state_changed=false.
        assert!(!outcome.state_changed);
        let neighbor = state.cell(3, 2).unwrap();
        assert_eq!(
            neighbor.bridgehead_anchor_class,
            BridgeheadAnchorClass::Variant0
        );
    }

    #[test]
    fn update_ramp_perpendicular_damagea_preserves_bridgehead_damaged() {
        use crate::sim::bridge_state::BridgeheadAnchorClass;
        let mut state = perpendicular_neighbor_state(
            (3, 2),
            BridgeCellRole::Bridgehead,
            BridgeheadAnchorClass::Damaged,
        );
        let outcome = update_ramp_perpendicular(
            &mut state,
            (2, 2),
            Axis::NS,
            Phase::DamageA,
            true,
            &mut ramp_test_terrain(),
        );
        assert!(!outcome.state_changed);
        let neighbor = state.cell(3, 2).unwrap();
        assert_eq!(
            neighbor.bridgehead_anchor_class,
            BridgeheadAnchorClass::Damaged
        );
    }

    #[test]
    fn gsi_04_01_ramp_same_side_partial_emits_no_invented_setter() {
        use crate::sim::bridge_state::BridgeheadAnchorClass;

        let mut state = perpendicular_neighbor_state(
            (3, 2),
            BridgeCellRole::Anchor,
            BridgeheadAnchorClass::Variant0,
        );
        state.cell_mut(3, 2).unwrap().damage_state = DamageState::PartialCollapseA;
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(3, 2)]);

        let outcome = update_ramp_perpendicular(
            &mut state,
            (2, 2),
            Axis::NS,
            Phase::CollapseA,
            true,
            &mut terrain,
        );

        assert!(outcome.setter_transcript.is_empty());
        assert_eq!(
            state.cell(3, 2).unwrap().damage_state,
            DamageState::PartialCollapseA,
            "NS CollapseA only finalizes the complementary state 8, not same-side state 7"
        );
    }

    #[test]
    fn gsi_04_01_ramp_state_branch_gates_on_live_raw_anchor_bit_not_role() {
        use crate::sim::bridge_state::BridgeheadAnchorClass;

        let mut cleared = perpendicular_neighbor_state(
            (3, 2),
            BridgeCellRole::Anchor,
            BridgeheadAnchorClass::Variant0,
        );
        let cleared_outcome = update_ramp_perpendicular(
            &mut cleared,
            (2, 2),
            Axis::NS,
            Phase::DamageA,
            true,
            &mut ramp_test_terrain(),
        );
        assert!(!cleared_outcome.state_changed);
        assert_eq!(
            cleared.cell(3, 2).unwrap().damage_state,
            DamageState::Healthy { variant: 0 },
            "persistent Anchor role cannot replace a cleared live +0x140 bit 0x80"
        );

        let mut live = perpendicular_neighbor_state(
            (3, 2),
            BridgeCellRole::Anchor,
            BridgeheadAnchorClass::Variant0,
        );
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(3, 2)]);
        let live_outcome =
            update_ramp_perpendicular(&mut live, (2, 2), Axis::NS, Phase::DamageA, true, &mut terrain);
        assert!(live_outcome.state_changed);
        assert_eq!(
            live.cell(3, 2).unwrap().damage_state,
            DamageState::Healthy { variant: 4 },
            "selected real CellClass raw 0x80 admits the native state-byte branch"
        );
    }

    #[test]
    fn gsi_04_01_ramp_recursive_state_chain_emits_deepest_first_setters() {
        use crate::map::bridge_facts::BridgeFlagStamp;
        use crate::sim::bridge_state::BridgeheadAnchorClass;

        let mut state = perpendicular_neighbor_state(
            (3, 2),
            BridgeCellRole::Anchor,
            BridgeheadAnchorClass::Variant0,
        );
        let mut chained = *state.cell(3, 2).expect("first perpendicular anchor");
        chained.damage_state = DamageState::PartialCollapseB;
        for x in 3..=5 {
            state.test_seed_cell(x, 2, chained);
        }
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(3, 2), (4, 2), (5, 2)]);

        let outcome = update_ramp_perpendicular(
            &mut state,
            (2, 2),
            Axis::NS,
            Phase::CollapseA,
            true,
            &mut terrain,
        );

        assert_eq!(
            outcome.setter_transcript,
            vec![
                BridgeFlagStamp::new((5, 2), Direction::N as u8, false),
                BridgeFlagStamp::new((4, 2), Direction::N as u8, false),
                BridgeFlagStamp::new((3, 2), Direction::N as u8, false),
            ],
            "recursive CollapseA returns deepest-first before each current dir0 setter"
        );
        for x in 3..=5 {
            assert_eq!(
                state.cell(x, 2).unwrap().damage_state,
                DamageState::Destroyed,
                "every complementary state is cleared while recursion unwinds"
            );
        }
    }

    #[test]
    fn gsi_04_01_ramp_about_to_fall_branch_recurses_without_state_branch() {
        use crate::map::bridge_facts::BridgeFlagStamp;
        use crate::sim::bridge_state::BridgeheadAnchorClass;

        let mut state = perpendicular_neighbor_state(
            (3, 2),
            BridgeCellRole::Anchor,
            BridgeheadAnchorClass::AboutToFall,
        );
        state.cell_mut(3, 2).unwrap().damage_state = DamageState::PartialCollapseA;
        let mut successor = *state.cell(3, 2).unwrap();
        successor.damage_state = DamageState::PartialCollapseB;
        successor.bridgehead_anchor_class = BridgeheadAnchorClass::Variant0;
        state.test_seed_cell(4, 2, successor);
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(3, 2), (4, 2)]);

        let outcome = update_ramp_perpendicular(
            &mut state,
            (2, 2),
            Axis::NS,
            Phase::CollapseA,
            true,
            &mut terrain,
        );

        assert_eq!(
            outcome.setter_transcript,
            vec![BridgeFlagStamp::new((4, 2), Direction::N as u8, false)],
            "AboutToFall +3 branch independently reaches the successor even when the current state branch does not recurse"
        );
        assert_eq!(
            state.cell(3, 2).unwrap().damage_state,
            DamageState::PartialCollapseA,
            "same-side current state remains unchanged"
        );
        assert_eq!(
            state.cell(4, 2).unwrap().damage_state,
            DamageState::Destroyed,
            "tile-class recursion executes the successor's complementary collapse"
        );
    }

    #[test]
    fn gsi_04_01_ramp_setter_clear_precedes_about_to_fall_second_traversal() {
        use crate::map::bridge_facts::BridgeFlagStamp;
        use crate::sim::bridge_state::BridgeheadAnchorClass;

        let mut state = perpendicular_neighbor_state(
            (3, 2),
            BridgeCellRole::Anchor,
            BridgeheadAnchorClass::AboutToFall,
        );
        state.cell_mut(3, 2).unwrap().damage_state = DamageState::PartialCollapseB;

        let mut recursive_bridgehead = *state.cell(3, 2).unwrap();
        recursive_bridgehead.role = BridgeCellRole::Bridgehead;
        recursive_bridgehead.damage_state = DamageState::Healthy { variant: 0 };
        recursive_bridgehead.bridgehead_anchor_class = BridgeheadAnchorClass::AboutToFall;
        state.test_seed_cell(4, 2, recursive_bridgehead);

        let mut deepest = *state.cell(3, 2).unwrap();
        deepest.damage_state = DamageState::PartialCollapseB;
        deepest.bridgehead_anchor_class = BridgeheadAnchorClass::Variant0;
        state.test_seed_cell(5, 2, deepest);
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(3, 2), (5, 2)]);

        let outcome = update_ramp_perpendicular(
            &mut state,
            (2, 2),
            Axis::NS,
            Phase::CollapseA,
            true,
            &mut terrain,
        );

        assert_eq!(
            outcome.setter_transcript,
            vec![
                BridgeFlagStamp::new((5, 2), Direction::N as u8, false),
                BridgeFlagStamp::new((3, 2), Direction::N as u8, false),
            ],
            "state recursion reaches the deep setter before the current setter; the later tile recursion sees live 0x80 already cleared"
        );
        assert_eq!(
            state.cell(5, 2).unwrap().damage_state,
            DamageState::Destroyed,
            "the first setter clears live x5 anchor bit before the independent AboutToFall traversal reaches it again"
        );
    }

    #[test]
    fn update_ramp_perpendicular_body_role_is_noop() {
        use crate::sim::bridge_state::BridgeheadAnchorClass;
        // Body role is not Anchor and not Bridgehead — should be a no-op.
        let mut state = perpendicular_neighbor_state(
            (1, 2),
            BridgeCellRole::Body,
            BridgeheadAnchorClass::Variant0,
        );
        let outcome = update_ramp_perpendicular(
            &mut state,
            (2, 2),
            Axis::NS,
            Phase::DamageB,
            true,
            &mut ramp_test_terrain(),
        );
        assert!(!outcome.state_changed);
        let neighbor = state.cell(1, 2).unwrap();
        assert_eq!(
            neighbor.bridgehead_anchor_class,
            BridgeheadAnchorClass::Variant0
        );
    }

    /// G4 regression guard: when both the state-byte and tile-class writes
    /// fire on the same Anchor target, the damaged-variant flood-fill must
    /// still fire (it gates only on state_byte_changed).
    #[test]
    fn update_ramp_perpendicular_flood_fill_still_fires_with_tile_class_write() {
        use crate::sim::bridge_state::BridgeheadAnchorClass;
        // Anchor target with Variant0 anchor class and damaged_data tile.
        // DamageA on Anchor: state byte 0 → 4 (Healthy{4}); tile class
        // Variant0 → Variant0 (no change). state_byte_changed=true →
        // flood-fill fires. (The has_damaged_data flag is false in
        // ramp_test_terrain, so the flood-fill no-ops out, but the call
        // path is exercised.)
        let mut state = perpendicular_neighbor_state(
            (3, 2),
            BridgeCellRole::Anchor,
            BridgeheadAnchorClass::Variant0,
        );
        let mut terrain = ramp_test_terrain_with_anchor_bits(&[(3, 2)]);
        let outcome =
            update_ramp_perpendicular(&mut state, (2, 2), Axis::NS, Phase::DamageA, true, &mut terrain);
        assert!(outcome.state_changed);
        // State byte advanced.
        assert_eq!(
            state.cell(3, 2).unwrap().damage_state,
            DamageState::Healthy { variant: 4 }
        );
        // Tile class preserved (DamageA on Variant0).
        assert_eq!(
            state.cell(3, 2).unwrap().bridgehead_anchor_class,
            BridgeheadAnchorClass::Variant0
        );
    }

    /// Helper: build a height-lookup from a (X, Y) → height map.
    fn walker_height_lookup(cells: Vec<((u16, u16), u8)>) -> impl Fn((u16, u16)) -> Option<u8> {
        move |pos: (u16, u16)| {
            cells
                .iter()
                .find_map(|&(p, h)| if p == pos { Some(h) } else { None })
        }
    }

    #[test]
    fn bridgehead_walk_ns_odd_height_returns_none() {
        // h=5 (NS ramp) — start-cell parity gate fires.
        let lookup = walker_height_lookup(vec![((5, 5), 5)]);
        let out = bridgehead_walk_to_anchor((5, 5), Axis::NS, lookup, 32, 32);
        assert!(out.is_none(), "h=5 must return None (parity gate)");
    }

    #[test]
    fn bridgehead_walk_ns_h7_returns_none() {
        let lookup = walker_height_lookup(vec![((5, 5), 7)]);
        let out = bridgehead_walk_to_anchor((5, 5), Axis::NS, lookup, 32, 32);
        assert!(out.is_none(), "h=7 must return None (parity gate)");
    }

    #[test]
    fn bridgehead_walk_ns_h8_walks_north() {
        // h=8 (high-ramp peak) — walks N (decreasing Y) → finds h=4 anchor.
        let lookup = walker_height_lookup(vec![((5, 5), 8), ((5, 4), 4)]);
        let out = bridgehead_walk_to_anchor((5, 5), Axis::NS, lookup, 32, 32);
        assert_eq!(out, Some((5, 4)), "h=8 must walk N and find anchor");
    }

    #[test]
    fn bridgehead_walk_ns_h0_walks_south() {
        // h=0 → walks S (+Y).
        let lookup = walker_height_lookup(vec![((5, 5), 0), ((5, 6), 4)]);
        let out = bridgehead_walk_to_anchor((5, 5), Axis::NS, lookup, 32, 32);
        assert_eq!(out, Some((5, 6)), "h=0 must walk S and find anchor");
    }

    #[test]
    fn bridgehead_walk_ns_h4_returns_start() {
        // h=4 is already at the anchor — return immediately.
        let lookup = walker_height_lookup(vec![((5, 5), 4)]);
        let out = bridgehead_walk_to_anchor((5, 5), Axis::NS, lookup, 32, 32);
        assert_eq!(out, Some((5, 5)), "h=4 must return start");
    }

    #[test]
    fn bridgehead_walk_ns_walks_through_odd_intermediate() {
        // Mid-walk parity tolerance: walk passes through odd h=5 between
        // h=8 start and h=4 anchor (previous Rust check rejected this).
        let lookup = walker_height_lookup(vec![((5, 5), 8), ((5, 4), 5), ((5, 3), 4)]);
        let out = bridgehead_walk_to_anchor((5, 5), Axis::NS, lookup, 32, 32);
        assert_eq!(
            out,
            Some((5, 3)),
            "walk must pass through odd-h intermediates and reach anchor",
        );
    }

    #[test]
    fn bridgehead_walk_ew_h_gt_4_returns_none() {
        // h=0xC (EW high-ramp peak) — upper-bound gate fires.
        let lookup = walker_height_lookup(vec![((5, 5), 0x0C)]);
        let out = bridgehead_walk_to_anchor((5, 5), Axis::EW, lookup, 32, 32);
        assert!(
            out.is_none(),
            "h=0xC EW must return None (upper-bound gate)"
        );
    }

    #[test]
    fn bridgehead_walk_ew_h0_walks_east() {
        // h=0 → walks E (+X) → finds h=2 anchor.
        let lookup = walker_height_lookup(vec![((5, 5), 0), ((6, 5), 2)]);
        let out = bridgehead_walk_to_anchor((5, 5), Axis::EW, lookup, 32, 32);
        assert_eq!(out, Some((6, 5)), "EW h=0 must walk E and find anchor");
    }

    #[test]
    fn bridgehead_walk_ew_h4_walks_west() {
        // h=4 (EW, > 2 but ≤ 4) → walks W (-X) → finds h=2 anchor.
        let lookup = walker_height_lookup(vec![((5, 5), 4), ((4, 5), 2)]);
        let out = bridgehead_walk_to_anchor((5, 5), Axis::EW, lookup, 32, 32);
        assert_eq!(out, Some((4, 5)), "EW h=4 must walk W and find anchor");
    }

    #[test]
    fn bridgehead_walk_ew_h2_returns_start() {
        // h=2 is at the anchor — return immediately.
        let lookup = walker_height_lookup(vec![((5, 5), 2)]);
        let out = bridgehead_walk_to_anchor((5, 5), Axis::EW, lookup, 32, 32);
        assert_eq!(out, Some((5, 5)), "EW h=2 must return start");
    }

    #[test]
    fn bridgehead_walk_off_map_returns_none() {
        // Start at (0, 0) h=8, walk N → off map → None.
        let lookup = walker_height_lookup(vec![((0, 0), 8)]);
        let out = bridgehead_walk_to_anchor((0, 0), Axis::NS, lookup, 32, 32);
        assert!(out.is_none(), "off-map walk must return None");
    }

    #[test]
    fn bridgehead_blow_up_row_ns_even_height() {
        let row = bridgehead_blow_up_row((5, 5), Axis::NS, 4, 10, 10);
        assert_eq!(row[0], Some((5, 4)));
        assert_eq!(row[1], Some((5, 5)));
        assert_eq!(row[2], Some((5, 6)));
    }

    #[test]
    fn bridgehead_blow_up_row_ns_odd_height() {
        let row = bridgehead_blow_up_row((5, 5), Axis::NS, 5, 10, 10);
        assert_eq!(row[0], Some((4, 4)));
        assert_eq!(row[1], Some((4, 5)));
        assert_eq!(row[2], Some((4, 6)));
    }

    #[test]
    fn bridgehead_blow_up_row_ew_low_height() {
        let row = bridgehead_blow_up_row((5, 5), Axis::EW, 2, 10, 10);
        assert_eq!(row[0], Some((4, 5)));
        assert_eq!(row[1], Some((5, 5)));
        assert_eq!(row[2], Some((6, 5)));
    }

    #[test]
    fn bridgehead_blow_up_row_ew_high_height() {
        let row = bridgehead_blow_up_row((5, 5), Axis::EW, 5, 10, 10);
        assert_eq!(row[0], Some((4, 4)));
        assert_eq!(row[1], Some((5, 4)));
        assert_eq!(row[2], Some((6, 4)));
    }

    #[test]
    fn bridgehead_blow_up_row_clamps_off_map_cells() {
        let row = bridgehead_blow_up_row((0, 0), Axis::NS, 4, 10, 10);
        assert_eq!(row[0], None);
        assert_eq!(row[1], Some((0, 0)));
        assert_eq!(row[2], Some((0, 1)));
    }

    #[test]
    fn bridgehead_blow_up_row_clamps_negative_x_for_ns_odd() {
        let row = bridgehead_blow_up_row((0, 5), Axis::NS, 5, 10, 10);
        assert_eq!(row[0], None);
        assert_eq!(row[1], None);
        assert_eq!(row[2], None);
    }

    #[test]
    fn bridgehead_blow_up_row_clamps_at_map_max() {
        let row = bridgehead_blow_up_row((9, 9), Axis::NS, 4, 10, 10);
        assert_eq!(row[0], Some((9, 8)));
        assert_eq!(row[1], Some((9, 9)));
        assert_eq!(row[2], None);
    }
}
