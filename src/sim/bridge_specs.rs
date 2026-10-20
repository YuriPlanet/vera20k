//! Live bridge ramp and destruction helpers: ramp state transitions and the
//! destroyed-overlay pick.

use crate::sim::bridge_state::{Axis, Phase};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_undefined_combination_returns_none() {
        // EW phase on NS-range state, etc.
        assert_eq!(apply_ramp_transition(0, Axis::EW, Phase::DamageA), None);
        assert_eq!(apply_ramp_transition(15, Axis::NS, Phase::DamageA), None);
        // State outside both ranges.
        assert_eq!(apply_ramp_transition(0xFF, Axis::NS, Phase::DamageA), None);
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
}
