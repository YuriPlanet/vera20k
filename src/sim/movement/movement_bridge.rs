//! Bridge layer transitions — applies the on_bridge cell-flag predicate at each
//! cell boundary crossing, and decouples `on_bridge` from `loco.layer` so that the
//! A* path layer (walkability-driving) and the runtime bridge state (predicate-driven)
//! can disagree at ramp cells — which they must, to match the reference behavior.
//!
//! Predicate:
//!   Enter:  dst.height_level == src.height_level - 4 AND dst has bridge structural flag
//!   Exit:   !(dst has bridge structural flag) AND src has bridge structural flag
//! Both conditions independent; signed i8 height arithmetic.
//!
//! Native source for that predicate is each ground locomotor's own movement
//! step, not a shared helper - see `compute_bridge_transition` below for the
//! two verified copies and their addresses.
//!
//! See docs/plans/2026-05-11-bridge-locomotor-layer-correctness-design.md.

use super::movement_occupancy::BRIDGE_DECK_LEVEL_DELTA;
use crate::sim::components::{BridgeOccupancy, Position};
use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
use crate::sim::pathfinding::PathGrid;
use crate::util::fixed_math::SimFixed;

/// Result of the gamemd on_bridge transition predicate at a cell boundary.
///
/// Two independent conditions:
///   Enter: dst.height_level == src.height_level - 4 AND dst has bridge structural flag
///   Exit:  !(dst has bridge structural flag) AND src has bridge structural flag
/// Both conditions are mutually exclusive on retail data but evaluated
/// independently to match the original behavior exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BridgeTransition {
    /// Unit just entered the bridge body deck. Sets on_bridge=true.
    ///
    /// `deck_level` is the predicate's own reading of the destination cell and is
    /// NOT the height the mover receives — Z is recomputed from the terrain level
    /// plus the post-transition flag, per `FootClass::Set_Height_On_Bridge`
    /// 0x005F5FA0. The two agree on well-formed data and are allowed to disagree
    /// on malformed data, where the native model wins.
    Enter { deck_level: u8 },
    /// Unit just exited the bridge structure. Sets on_bridge=false.
    Exit,
    /// No layer-state change at this transition.
    NoChange,
}

/// Bridge state update produced by `resolve_cell_transition_bridge_state`.
/// Drives `on_bridge` and `BridgeOccupancy` independently from `loco.layer`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BridgeStateUpdate {
    /// on_bridge = true; bridge_occupancy = Some(BridgeOccupancy { deck_level })
    Set(u8),
    /// on_bridge = false; bridge_occupancy = None
    Clear,
    /// Leave on_bridge and bridge_occupancy unchanged
    Unchanged,
}

/// Persisted analogue of the runtime Foot bridge-state mismatch byte.
///
/// The owner integration stores this alongside locomotor runtime state. It is
/// intentionally independent from A* bridge-layer selection and from the
/// `on_bridge` occupancy byte.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct RuntimeBridgeTransitionState {
    /// Set when candidate bridge bit and current bridge state differ.
    pub pending_mismatch: bool,
}

/// Result supplied by the runtime-only bridge policy seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeBridgePolicyResult {
    Continue,
    Reject,
}

/// Evaluate the runtime bridge mismatch branch without borrowing A* policy.
///
/// Original: each locomotor's `Process_Movement` compares
/// candidate `CellClass+0x140 & 0x100` with Foot `+0x8C`, sets `+0x68B` on a
/// mismatch, then calls virtual `+0x29C` to select the local return path.
///
/// The comparison lives in each locomotor's own `Process_Movement`, not in a
/// shared helper: `DriveLocomotionClass` @ `0x004B3376`-`0x004B339D`
/// (`EDX = (cell->Flags >> 8) & 1`; `AL = [foot+0x8C]`; `XOR EDX,EAX; JZ`;
/// `MOV byte [ECX+0x68B],1`; `CALL [vtable+0x29C]`), with twins at
/// `0x0075B662` (Walk), `0x00515513` (Hover) and `0x006A29E0` (Ship) — four
/// sites carrying the whole sequence. `0x006A3C19` and `0x00736038` are bare
/// `[+0x68B] = 1` stores in unrelated branches, not twins of this comparison.
/// There is no
/// `FootClass::EvaluateCellEnterabilityOrCost` in this program.
///
/// **VERA-internal, gamemd equivalent UNCHECKED:** `pending_mismatch` is
/// written on every call, so a matching tick clears it. Native `[foot+0x68B]`
/// is write-1-only — initialised once in `FootClass::Constructor` @
/// `0x004D33BA`, set at eleven locomotor sites, never cleared, and read by
/// `FootClass::ComputeChecksum` @ `0x004DBAD0` (the read is at `0x004DBD0C`),
/// so it is part of the sync
/// checksum. Trigger: any tick where the candidate's bridge bit matches the
/// mover's. Player effect: none today — the field has no reader outside tests
/// and the only production caller passes a policy that never rejects.
/// Frequency: zero while that holds. Downstream risk: a flip to authoritative
/// would start from the wrong latch semantics and a wrong checksum input.
pub(crate) fn evaluate_runtime_bridge_transition(
    state: &mut RuntimeBridgeTransitionState,
    candidate_bridge_bit: bool,
    current_bridge_state: bool,
    policy: impl FnOnce() -> RuntimeBridgePolicyResult,
) -> RuntimeBridgePolicyResult {
    let mismatch = candidate_bridge_bit != current_bridge_state;
    state.pending_mismatch = mismatch;
    if mismatch {
        policy()
    } else {
        RuntimeBridgePolicyResult::Continue
    }
}

/// Read-only runtime bridge row for oracle diagnostics.
#[cfg(test)]
#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct BridgeRuntimeOracleTick {
    pub tick: u64,
    pub current_cell: (u16, u16),
    pub next_cell: (u16, u16),
    pub loco_layer_before: MovementLayer,
    pub next_path_layer: MovementLayer,
    pub on_bridge_before: bool,
    pub on_bridge_after: bool,
    pub bridge_update: String,
    pub bridge_occupancy_before: Option<u8>,
    pub bridge_occupancy_after: Option<u8>,
    pub visible_z_after: u8,
}

pub(super) fn projected_on_bridge(current: bool, update: BridgeStateUpdate) -> bool {
    match update {
        BridgeStateUpdate::Set(_) => true,
        BridgeStateUpdate::Clear => false,
        BridgeStateUpdate::Unchanged => current,
    }
}

/// Bridge vertical clearance in leptons.
/// 416 == 104 * 4 — the verified Foot-role Z distance from water surface to bridge deck.
///
/// gamemd produces this number twice, independently, and both producers agree
/// with the constant here:
/// - `DriveLocomotionClass::ComputeBridgeZOffset` 0x004AF4A0 is a one-shot
///   initializer computing `ftol(4 * g_DriveHeightStep + 0.5)` into
///   `g_BridgeZOffset_Drive` 0x008A07C4, consumed by `Set_Destination`
///   0x004AFDE2 and `Process_Drive_Track` 0x004B0FE7 / 0x004B18CC.
/// - `FootClass::Set_Height_On_Bridge` 0x005F5FA0 adds
///   `g_nFootOnBridgeDeckOffsetLeptons` (416) to its height argument whenever
///   the object's `+0x8C` OnBridge byte is set, then writes
///   `CellClass::GetGroundHeight(coords) + offset` into `+0xA4`, bracketing the
///   write with vtable+0x124 REMOVE/PUT when `+0x74` IsMarked is set.
/// Added to braking distance when a ship passes under a bridge cell.
/// Same physical fact as `sim::map::bridge_topology::BRIDGE_DECK_HEIGHT_LEPTONS`
/// (a `SimFixed` literal cannot be const-derived from it; the equality is
/// pinned by `bridge_z_offset_matches_deck_height` below).
pub(super) const BRIDGE_Z_OFFSET: SimFixed = SimFixed::lit("416");

/// The on_bridge cell-flag predicate at a cell-boundary crossing.
///
/// gamemd has no shared helper for this: each ground locomotor carries its
/// own copy of the same block, and the two that matter agree instruction for
/// instruction.
///
/// `WalkLocomotionClass::ProcessMovement` 0x0075C154 - 0x0075C199 and
/// `DriveLocomotionClass::Process_Drive_Track` 0x004B2561 - 0x004B259B, with
/// ESI/ECX the source cell and EAX the destination cell:
///
/// ```text
/// MOVSX EDX, byte [src + 0x11B]      ; source ground level, SIGNED
/// MOVSX EDI, byte [dst + 0x11B]      ; destination ground level, SIGNED
/// SUB   EDX, 4
/// CMP   EDI, EDX          ; JNZ over the enter arm
///   TEST [dst + 0x140], 0x100        ; JZ over the enter arm
///     foot->+0x8C = 1                ; ENTER
/// TEST  [dst + 0x140], 0x100         ; JNZ done - destination is a bridge cell
/// TEST  [src + 0x140], 0x100         ; JZ  done - source was not
///   foot->+0x8C = 0                  ; EXIT
/// ```
///
/// So the enter arm is an exact equality on level indices and the exit arm
/// reads only the two bridge flags. Both arms are evaluated in sequence, not
/// as if/else, which is what the function below does.
///
/// `ObjectClass::ShouldBeOnBridge` 0x005F6A70 is NOT this predicate despite
/// the name - it compares ground heights in leptons at the object's
/// destination coordinate and feeds zone reachability queries. See
/// `should_be_on_bridge_is_a_reachability_input_not_the_transition` for the
/// evidence and the one place the two really do diverge.
///
/// Height arithmetic uses signed i8 via `wrapping_sub` where the native
/// sign-extends to 32 bits first and subtracts without wrapping. The two
/// disagree only for a source level in 128..=131, where `wrapping_sub` lands
/// back inside the i8 range and can match a destination level of 124..=127
/// that the native's -132..=-129 never equals. Retail maps use levels 0-15,
/// so no map reaches it; recorded rather than papered over.
pub(super) fn compute_bridge_transition(
    src: &crate::sim::pathfinding::PathCell,
    dst: &crate::sim::pathfinding::PathCell,
) -> BridgeTransition {
    let src_h = src.ground_level as i8;
    let dst_h = dst.ground_level as i8;

    let entry = dst_h == src_h.wrapping_sub(4) && dst.has_structural_bridge();
    let exit = !dst.has_structural_bridge() && src.has_structural_bridge();

    if entry {
        return BridgeTransition::Enter {
            deck_level: dst.bridge_deck_level_if_any().unwrap_or(dst.ground_level),
        };
    }
    if exit {
        return BridgeTransition::Exit;
    }
    BridgeTransition::NoChange
}

/// Apply the on_bridge cell-flag predicate at a cell-boundary crossing.
///
/// Reads src and dst PathCells and computes the BridgeStateUpdate using
/// `compute_bridge_transition`. Writes `position.z` to the post-transition height
/// (deck_level on Enter, dst.ground_level on Exit, or next_layer's effective height
/// on NoChange).
///
/// Does NOT return a layer — the caller continues to use `next_layer` from A*'s
/// `path_layers` for `loco.layer`. The predicate's role is independent: it drives
/// `on_bridge` and `BridgeOccupancy` via the returned `BridgeStateUpdate`.
///
/// Fallback: returns `Unchanged` (no position.z modification) when `path_grid` is
/// `None` or either cell lookup is out-of-bounds. Out-of-bounds at the boundary
/// crossing indicates a path-data bug elsewhere; the resolver is not the recovery point.
pub(super) fn resolve_cell_transition_bridge_state(
    position: &mut Position,
    path_grid: Option<&PathGrid>,
    src: (u16, u16),
    dst: (u16, u16),
    on_bridge_before: bool,
) -> BridgeStateUpdate {
    let Some(grid) = path_grid else {
        return BridgeStateUpdate::Unchanged;
    };
    let (Some(src_cell), Some(dst_cell)) = (grid.cell(src.0, src.1), grid.cell(dst.0, dst.1))
    else {
        return BridgeStateUpdate::Unchanged;
    };

    // The predicate owns the flag; the height function owns Z. Keeping them
    // separate is the native split, and it is why Z is recomputed here from the
    // post-transition flag rather than carried out of the transition arm.
    let transition = compute_bridge_transition(src_cell, dst_cell);
    let update = match transition {
        BridgeTransition::Enter { .. } => BridgeStateUpdate::Set(0),
        BridgeTransition::Exit => BridgeStateUpdate::Clear,
        BridgeTransition::NoChange => BridgeStateUpdate::Unchanged,
    };
    let on_bridge_after = projected_on_bridge(on_bridge_before, update);

    // `FootClass::Set_Height_On_Bridge` 0x005F5FA0:
    //   Location.Z = CellClass::GetGroundHeight(own cell) + arg
    //                + (OnBridge ? g_nFootOnBridgeDeckOffsetLeptons : 0)
    // The ground term (`CellClass::ComputeGroundHeightAtCoord` 0x0047B3A0) reads
    // the destination cell's own signed terrain level and consults no bridge
    // field; the deck term is gated on the object's `+0x8C` OnBridge byte and
    // nothing else. There is no stored per-cell deck height in the native model
    // and therefore no value that can be absent, so this must not consult the A*
    // layer or any cell-side walkability permission — both are VERA inventions
    // that silently substituted ground level for the deck.
    let z = (dst_cell.signed_level()
        + if on_bridge_after {
            BRIDGE_DECK_LEVEL_DELTA
        } else {
            0
        }) as u8;
    position.z = z;
    // Raw Object Z has a separate setter cadence. Paid Drive/Ship points and
    // Walk commits refresh it; a residual crossing retains it (0x4B253F).

    // Carry the same number into BridgeOccupancy so the deck height has one
    // source rather than two independently-derived ones.
    match update {
        BridgeStateUpdate::Set(_) => BridgeStateUpdate::Set(z),
        other => other,
    }
}

/// Diagnostic wrapper for a single boundary crossing. It computes the same
/// bridge update as `resolve_cell_transition_bridge_state` and returns an
/// oracle row; callers opt in explicitly from tests/tools.
#[cfg(test)]
pub(super) fn resolve_cell_transition_bridge_state_oracle(
    position: &mut Position,
    path_grid: Option<&PathGrid>,
    src: (u16, u16),
    dst: (u16, u16),
    current_layer: MovementLayer,
    next_layer: MovementLayer,
    on_bridge_before: bool,
    bridge_occupancy_before: Option<BridgeOccupancy>,
    tick: u64,
) -> (BridgeStateUpdate, BridgeRuntimeOracleTick) {
    let update =
        resolve_cell_transition_bridge_state(position, path_grid, src, dst, on_bridge_before);
    let on_bridge_after = projected_on_bridge(on_bridge_before, update);
    let bridge_occupancy_after = match update {
        BridgeStateUpdate::Set(deck_level) => Some(BridgeOccupancy { deck_level }),
        BridgeStateUpdate::Clear => None,
        BridgeStateUpdate::Unchanged => bridge_occupancy_before,
    };
    let row = BridgeRuntimeOracleTick {
        tick,
        current_cell: src,
        next_cell: dst,
        loco_layer_before: current_layer,
        next_path_layer: next_layer,
        on_bridge_before,
        on_bridge_after,
        bridge_update: format!("{:?}", update),
        bridge_occupancy_before: bridge_occupancy_before.map(|occ| occ.deck_level),
        bridge_occupancy_after: bridge_occupancy_after.map(|occ| occ.deck_level),
        visible_z_after: position.z,
    };
    (update, row)
}

/// Apply the post-resolver bridge state to entity components.
///
/// `loco.layer` follows `active_layer` (= A*'s path_layer for this step), which drives
/// walkability and cell_entry occupancy lookup.
///
/// `on_bridge` and `bridge_occupancy` are driven INDEPENDENTLY by `bridge_update` from
/// the cell-flag predicate. This is the load-bearing G2 parity fix: the runtime
/// on_bridge state is NOT derivable from the A* layer, because on a ramp going up
/// loco.layer=Bridge but on_bridge=false (predicate hasn't fired Enter yet), and on a
/// ramp going down loco.layer=Ground but on_bridge=true.
pub(super) fn apply_pending_bridge_render_state(
    locomotor: &mut Option<LocomotorState>,
    bridge_occupancy: &mut Option<BridgeOccupancy>,
    on_bridge: &mut bool,
    active_layer: MovementLayer,
    bridge_update: BridgeStateUpdate,
    _diag_entity_id: u64,
) {
    if let Some(loco) = locomotor {
        loco.layer = active_layer;
    }
    match bridge_update {
        BridgeStateUpdate::Set(deck_level) => {
            *on_bridge = true;
            *bridge_occupancy = Some(BridgeOccupancy { deck_level });
        }
        BridgeStateUpdate::Clear => {
            *on_bridge = false;
            *bridge_occupancy = None;
        }
        BridgeStateUpdate::Unchanged => {
            // on_bridge and bridge_occupancy retain their previous values
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::movement::locomotion::LocomotorSlot;
    use crate::sim::pathfinding::PathCell;

    /// Construct a synthetic PathCell with the bridge fields we care about.
    fn cell(ground_level: u8, bridge_walkable: bool, transition: bool) -> PathCell {
        let bridge_deck_level = if bridge_walkable {
            ground_level.saturating_add(4)
        } else {
            0
        };
        PathCell {
            ground_walkable: true,
            bridge_walkable,
            bridge_structural: bridge_walkable && !transition,
            bridge_marker_0x80: false,
            transition,
            ground_level,
            bridge_deck_level,
            slope_type: 0,
            tube_index: None,
            low_bridge_tube_cell: false,
        }
    }

    #[test]
    fn gsi_04_03b_water_mover_bridge_clearance_crosses_braking_boundary() {
        let planar_distance = SimFixed::from_num(100);
        let slowdown_distance = SimFixed::from_num(500);
        let distance_under_bridge = planar_distance + BRIDGE_Z_OFFSET;

        assert_eq!(distance_under_bridge, SimFixed::from_num(516));
        assert!(
            !(distance_under_bridge < slowdown_distance),
            "the verified 416-lepton deck offset must keep this mover outside braking range"
        );
        assert!(
            planar_distance + SimFixed::from_num(360) < slowdown_distance,
            "the stale 360-lepton offset would incorrectly begin braking at this boundary"
        );
    }

    #[test]
    fn entry_from_ramp_to_body() {
        // Ramp at height 4 (bridge_walkable, transition=true) → Body at height 0
        // (bridge_walkable, no transition).
        let src = cell(4, true, true);
        let dst = cell(0, true, false);
        match compute_bridge_transition(&src, &dst) {
            BridgeTransition::Enter { deck_level } => assert_eq!(deck_level, 4),
            other => panic!("expected Enter, got {:?}", other),
        }
    }

    #[test]
    fn exit_from_body_to_ground() {
        // Body at height 0 (bridge_walkable) → Ground at height 0 (NOT bridge_walkable).
        let src = cell(0, true, false);
        let dst = cell(0, false, false);
        assert_eq!(
            compute_bridge_transition(&src, &dst),
            BridgeTransition::Exit
        );
    }

    #[test]
    fn body_to_body_no_change() {
        let src = cell(0, true, false);
        let dst = cell(0, true, false);
        assert_eq!(
            compute_bridge_transition(&src, &dst),
            BridgeTransition::NoChange
        );
    }

    #[test]
    fn ground_to_ground_no_change() {
        let src = cell(0, false, false);
        let dst = cell(0, false, false);
        assert_eq!(
            compute_bridge_transition(&src, &dst),
            BridgeTransition::NoChange
        );
    }

    #[test]
    fn ground_to_bridgehead_no_change() {
        // Going UP onto a ramp is NOT an on_bridge transition: dst is HIGHER than src.
        // src=ground 0, dst=ramp 4. dst_h(4) == src_h(0) - 4 (=-4)? No. Entry doesn't fire.
        // src has no bridge_walkable; exit doesn't fire. NoChange.
        let src = cell(0, false, false);
        let dst = cell(4, true, true);
        assert_eq!(
            compute_bridge_transition(&src, &dst),
            BridgeTransition::NoChange
        );
    }

    #[test]
    fn cliff_drop_off_bridge_ramp() {
        // Edge case: src=ramp (h=4, bridge_walkable, transition),
        // dst=ground at lower elevation (h=0, NOT bridge_walkable). Ramps are bridge-layer
        // pathing cells but are not structural bridge body cells, so the on_bridge predicate
        // does not fire Exit here.
        let src = cell(4, true, true);
        let dst = cell(0, false, false);
        assert_eq!(
            compute_bridge_transition(&src, &dst),
            BridgeTransition::NoChange
        );
    }

    /// The two cases where reading `ObjectClass::ShouldBeOnBridge` as the
    /// cell-transition predicate would have changed the answer. Both are
    /// `NoChange` natively; a lepton-height port would make the first an
    /// `Enter` and the second an `Exit`.
    #[test]
    fn five_level_drop_onto_a_deck_is_not_an_entry() {
        // Enter needs dst == src - 4 exactly. A five-level drop is not it,
        // even though the height difference clears three level heights.
        let src = cell(5, true, true);
        let dst = cell(0, true, false);
        assert_eq!(
            compute_bridge_transition(&src, &dst),
            BridgeTransition::NoChange
        );
    }

    #[test]
    fn deck_to_deck_over_rising_terrain_is_not_an_exit() {
        // Exit reads the two bridge flags only. The terrain under the deck
        // climbing four levels between the cells does not end the crossing.
        let src = cell(0, true, false);
        let dst = cell(4, true, false);
        assert_eq!(
            compute_bridge_transition(&src, &dst),
            BridgeTransition::NoChange
        );
    }

    #[test]
    fn signed_height_arithmetic() {
        // Verify wrapping_sub handles the i8 boundary. src.ground_level = 4 (as i8 = 4),
        // dst.ground_level = 0 (as i8 = 0). 4.wrapping_sub(4) == 0. Entry should fire.
        let src = cell(4, true, true);
        let dst = cell(0, true, false);
        assert!(matches!(
            compute_bridge_transition(&src, &dst),
            BridgeTransition::Enter { deck_level: 4 }
        ));
    }

    #[test]
    fn projected_on_bridge_applies_pending_update_without_mutation() {
        assert!(projected_on_bridge(false, BridgeStateUpdate::Set(4)));
        assert!(!projected_on_bridge(true, BridgeStateUpdate::Clear));
        assert!(projected_on_bridge(true, BridgeStateUpdate::Unchanged));
        assert!(!projected_on_bridge(false, BridgeStateUpdate::Unchanged));
    }

    #[test]
    fn runtime_bridge_mismatch_sets_pending_state_and_uses_policy_seam() {
        let mut state = RuntimeBridgeTransitionState::default();
        let outcome = evaluate_runtime_bridge_transition(&mut state, true, false, || {
            RuntimeBridgePolicyResult::Reject
        });

        assert!(state.pending_mismatch);
        assert_eq!(outcome, RuntimeBridgePolicyResult::Reject);
    }

    #[test]
    fn matching_runtime_bridge_state_skips_policy_seam() {
        let mut state = RuntimeBridgeTransitionState::default();
        let mut policy_called = false;
        let outcome = evaluate_runtime_bridge_transition(&mut state, true, true, || {
            policy_called = true;
            RuntimeBridgePolicyResult::Reject
        });

        assert!(!state.pending_mismatch);
        assert!(!policy_called);
        assert_eq!(outcome, RuntimeBridgePolicyResult::Continue);
    }

    #[test]
    fn entry_without_bridge_walkable_no_change() {
        // Height-diff matches 4 but dst is NOT bridge_walkable. Entry must NOT fire.
        let src = cell(4, false, false);
        let dst = cell(0, false, false);
        assert_eq!(
            compute_bridge_transition(&src, &dst),
            BridgeTransition::NoChange
        );
    }

    // ------------------------------------------------------------------------
    // Resolver tests (resolve_cell_transition_bridge_state)
    // ------------------------------------------------------------------------

    use crate::sim::components::Position;
    use crate::sim::pathfinding::PathGrid;
    use crate::util::fixed_math::SimFixed;

    fn make_grid_with_cells(cells: &[(u16, u16, u8, bool, bool)]) -> PathGrid {
        let mut g = PathGrid::new(16, 16);
        for &(x, y, ground_level, bridge_walkable, transition) in cells {
            g.set_cell_for_test(x, y, ground_level, bridge_walkable, transition);
        }
        g
    }

    // ------------------------------------------------------------------------
    // Full-span coarse bridge projection at each explicit cell transition.
    //
    // `FootClass::Set_Height_On_Bridge` 0x005F5FA0 recomputes
    //   Location.Z = GroundHeight(own cell) + (OnBridge ? 4 levels : 0)
    // with no stored per-cell deck height and no second gate. These tests walk a
    // multi-cell span because a two-cell fixture is satisfied by the Enter
    // arm alone and proves nothing about the deck-to-deck steps in between,
    // which are where a tank actually spends the crossing. These assertions
    // concern legacy Position.z only. Authoritative raw Z follows SetHeight
    // cadence and can lag XY during Drive/Ship residual movement.
    // ------------------------------------------------------------------------

    const SPAN_Y: u16 = 5;
    const RAMP_LEVEL: u8 = 5;
    const RIVERBED_LEVEL: u8 = 1;
    const ENTRY_RAMP_X: u16 = 1;
    const EXIT_RAMP_X: u16 = 10;

    /// Entry ramp at level 5, eight deck cells over a riverbed at level 1, exit
    /// ramp at level 5. The Enter predicate needs `dst == src - 4`, so 1 == 5-4.
    ///
    /// The ramps are built with the decoupled setter because they must be
    /// transition cells that are NOT structural: the Exit arm fires on
    /// `src structural && !dst structural`, so a structural exit ramp would leave
    /// the mover flagged on-bridge after it had driven off the span.
    /// `set_cell_for_test` ties `bridge_structural` to `bridge_walkable` and
    /// cannot express that.
    fn full_span_grid() -> PathGrid {
        let mut g = PathGrid::new(16, 16);
        for ramp_x in [ENTRY_RAMP_X, EXIT_RAMP_X] {
            g.set_bridge_cell_decoupled_for_test(
                ramp_x, SPAN_Y, RAMP_LEVEL,
                false, // ramps are transition cells, not structural deck
                true, RAMP_LEVEL, true,
            );
        }
        for x in (ENTRY_RAMP_X + 1)..EXIT_RAMP_X {
            g.set_cell_for_test(x, SPAN_Y, RIVERBED_LEVEL, true, false);
        }
        g
    }

    /// Legacy coarse cell/deck projection, independent of the authoritative
    /// raw-Z producer. This helper does not establish native GetHeight parity.
    fn assert_native_height(grid: &PathGrid, pos: &Position, on_bridge: bool, step: &str) {
        let cell = grid.cell(pos.rx, pos.ry).expect("cell in bounds");
        let expected = (cell.signed_level()
            + if on_bridge {
                BRIDGE_DECK_LEVEL_DELTA
            } else {
                0
            }) as u8;
        assert_eq!(
            pos.z,
            expected,
            "{step}: z must be terrain({}) + {} when on_bridge={on_bridge}",
            cell.signed_level(),
            if on_bridge {
                BRIDGE_DECK_LEVEL_DELTA
            } else {
                0
            }
        );
    }

    /// Drive west-to-east across the span, asserting the invariant after every
    /// single cell step. Returns the per-cell `(z, on_bridge)` trace.
    fn drive_span(grid: &PathGrid) -> Vec<(u8, bool)> {
        let mut pos = pos_at(ENTRY_RAMP_X, SPAN_Y, RAMP_LEVEL);
        let mut on_bridge = false;
        let mut trace = Vec::new();
        for x in ENTRY_RAMP_X..EXIT_RAMP_X {
            let src = (x, SPAN_Y);
            let dst = (x + 1, SPAN_Y);
            let update =
                resolve_cell_transition_bridge_state(&mut pos, Some(grid), src, dst, on_bridge);
            on_bridge = projected_on_bridge(on_bridge, update);
            pos.rx = dst.0;
            pos.ry = dst.1;
            assert_native_height(grid, &pos, on_bridge, &format!("step {x}->{}", x + 1));
            if let BridgeStateUpdate::Set(deck_level) = update {
                assert_eq!(
                    deck_level, pos.z,
                    "BridgeOccupancy.deck_level must be the same number as position.z"
                );
            }
            trace.push((pos.z, on_bridge));
        }
        trace
    }

    #[test]
    fn tank_drives_full_span_and_never_leaves_the_deck() {
        let grid = full_span_grid();
        let trace = drive_span(&grid);

        // Nine steps: onto the deck, seven deck-to-deck, then off onto the ramp.
        assert_eq!(trace.len(), 9);
        let deck_z = RIVERBED_LEVEL + BRIDGE_DECK_LEVEL_DELTA as u8;
        for (i, &(z, on_bridge)) in trace[..trace.len() - 1].iter().enumerate() {
            assert!(on_bridge, "deck step {i} must keep on_bridge set");
            assert_eq!(
                z, deck_z,
                "deck step {i} must stay at deck height, not drop to the riverbed"
            );
        }
        let (final_z, final_on_bridge) = trace[trace.len() - 1];
        assert!(!final_on_bridge, "leaving the span must clear on_bridge");
        assert_eq!(final_z, RAMP_LEVEL, "the exit ramp is solid ground at 5");
    }

    #[test]
    fn mid_span_height_survives_a_cell_whose_walkable_permission_is_off() {
        // Regression for the drop this fix removes: the height used to come from
        // an Option gated on `bridge_walkable`, so a structural deck cell whose
        // permission bit was clear substituted ground level and dropped the tank
        // into the riverbed mid-crossing. gamemd has no such input.
        let mut grid = full_span_grid();
        grid.set_bridge_cell_decoupled_for_test(
            5,
            SPAN_Y,
            RIVERBED_LEVEL,
            true,  // structural: still part of the span
            false, // walkable permission clear
            0,     // and its stored deck level is unusable
            false,
        );

        let trace = drive_span(&grid);
        let deck_z = RIVERBED_LEVEL + BRIDGE_DECK_LEVEL_DELTA as u8;
        for (i, &(z, on_bridge)) in trace[..trace.len() - 1].iter().enumerate() {
            assert!(on_bridge, "deck step {i} must keep on_bridge set");
            assert_eq!(
                z, deck_z,
                "deck step {i} must not follow the permission bit"
            );
        }
    }

    #[test]
    fn entry_height_ignores_a_bogus_stored_deck_level() {
        // The Enter arm used to publish the cell's stored deck value verbatim.
        // Native recomputes from terrain, so a wrong stored value cannot move a
        // mover.
        let mut grid = full_span_grid();
        grid.set_bridge_cell_decoupled_for_test(
            ENTRY_RAMP_X + 1,
            SPAN_Y,
            RIVERBED_LEVEL,
            true,
            true,
            99, // nonsense stored deck level
            false,
        );

        let mut pos = pos_at(ENTRY_RAMP_X, SPAN_Y, RAMP_LEVEL);
        let update = resolve_cell_transition_bridge_state(
            &mut pos,
            Some(&grid),
            (ENTRY_RAMP_X, SPAN_Y),
            (ENTRY_RAMP_X + 1, SPAN_Y),
            false,
        );
        assert!(matches!(update, BridgeStateUpdate::Set(_)));
        assert_eq!(
            pos.z,
            RIVERBED_LEVEL + BRIDGE_DECK_LEVEL_DELTA as u8,
            "entry height is terrain + 4, never the stored deck field"
        );
    }

    #[test]
    fn ordinary_ground_movement_is_unaffected_by_the_bridge_height_model() {
        // The same code path runs for every non-bridge step in the game, so it
        // must leave plain ground movement exactly where it was.
        let grid = make_grid_with_cells(&[(2, 2, 7, false, false), (3, 2, 7, false, false)]);
        let mut pos = pos_at(2, 2, 7);
        let update =
            resolve_cell_transition_bridge_state(&mut pos, Some(&grid), (2, 2), (3, 2), false);
        assert_eq!(update, BridgeStateUpdate::Unchanged);
        assert_eq!(pos.z, 7, "ground movement keeps the terrain level");
    }

    fn pos_at(rx: u16, ry: u16, z: u8) -> Position {
        Position {
            rx,
            ry,
            z,
            exact_z_leptons: None,
            sub_x: SimFixed::ZERO,
            sub_y: SimFixed::ZERO,
            // screen_x/screen_y are #[serde(skip, default)] but Position has no
            // Default impl, so we must initialize them explicitly in struct literals.
        }
    }

    #[test]
    fn resolver_fallback_when_path_grid_none() {
        let mut p = pos_at(5, 5, 10);
        let update = resolve_cell_transition_bridge_state(&mut p, None, (5, 5), (6, 5), false);
        assert_eq!(update, BridgeStateUpdate::Unchanged);
        assert_eq!(p.z, 10, "position.z must be untouched on Unchanged");
    }

    #[test]
    fn resolver_fallback_when_cell_out_of_bounds() {
        let g = make_grid_with_cells(&[]);
        let mut p = pos_at(0, 0, 10);
        // src in bounds, dst out of bounds:
        let update =
            resolve_cell_transition_bridge_state(&mut p, Some(&g), (0, 0), (999, 999), false);
        assert_eq!(update, BridgeStateUpdate::Unchanged);
        assert_eq!(p.z, 10);
    }

    #[test]
    fn resolver_enter_writes_deck_level_and_set() {
        // src=ramp at h=4, dst=body at h=0 with bridge_walkable
        let g = make_grid_with_cells(&[
            (5, 5, 4, true, true),  // ramp
            (6, 5, 0, true, false), // body
        ]);
        let mut p = pos_at(6, 5, 4);
        let update = resolve_cell_transition_bridge_state(&mut p, Some(&g), (5, 5), (6, 5), false);
        assert_eq!(update, BridgeStateUpdate::Set(4));
        assert_eq!(
            p.z, 4,
            "Enter height is the destination terrain level (0) plus the deck delta"
        );
    }

    #[test]
    fn resolver_oracle_reports_runtime_bridge_split_fields() {
        let g = make_grid_with_cells(&[(5, 5, 4, true, true), (6, 5, 0, true, false)]);
        let mut p = pos_at(6, 5, 0);
        let (update, row) = resolve_cell_transition_bridge_state_oracle(
            &mut p,
            Some(&g),
            (5, 5),
            (6, 5),
            MovementLayer::Ground,
            MovementLayer::Bridge,
            false,
            None,
            123,
        );

        assert_eq!(update, BridgeStateUpdate::Set(4));
        assert_eq!(row.tick, 123);
        assert_eq!(row.next_path_layer, MovementLayer::Bridge);
        assert!(row.on_bridge_after);
        assert_eq!(row.bridge_occupancy_after, Some(4));
        assert_eq!(row.visible_z_after, 4);
    }

    #[test]
    fn resolver_exit_writes_ground_level_and_clear() {
        // src=body at h=0 bridge_walkable, dst=ground at h=0 NOT bridge_walkable
        let g = make_grid_with_cells(&[(5, 5, 0, true, false), (6, 5, 0, false, false)]);
        let mut p = pos_at(6, 5, 4);
        let update = resolve_cell_transition_bridge_state(&mut p, Some(&g), (5, 5), (6, 5), true);
        assert_eq!(update, BridgeStateUpdate::Clear);
        assert_eq!(p.z, 0, "position.z must equal dst.ground_level on Exit");
    }

    #[test]
    fn resolver_no_change_on_deck_keeps_deck_height_from_the_mover_flag() {
        // Body-to-body. NoChange. The mover is already on the deck, so its height
        // stays terrain + delta. This is the mid-span step the old layer-driven
        // code got wrong: it re-derived Z from the A* layer, and a planner that
        // said Ground dropped the tank into the riverbed halfway across.
        let g = make_grid_with_cells(&[(5, 5, 0, true, false), (6, 5, 0, true, false)]);
        let mut p = pos_at(6, 5, 4);
        let update = resolve_cell_transition_bridge_state(&mut p, Some(&g), (5, 5), (6, 5), true);
        assert_eq!(update, BridgeStateUpdate::Unchanged);
        assert_eq!(p.z, 4, "a mover on the deck stays at terrain + 4");
    }

    #[test]
    fn resolver_no_change_under_a_span_keeps_ground_height() {
        // The same two deck cells, but the mover is NOT on the bridge — it is
        // driving underneath. Same cells, same call, opposite answer, decided
        // solely by the mover's own flag exactly as gamemd decides it.
        let g = make_grid_with_cells(&[(5, 5, 0, true, false), (6, 5, 0, true, false)]);
        let mut p = pos_at(6, 5, 0);
        let update = resolve_cell_transition_bridge_state(&mut p, Some(&g), (5, 5), (6, 5), false);
        assert_eq!(update, BridgeStateUpdate::Unchanged);
        assert_eq!(p.z, 0, "a mover under the span stays on the terrain");
    }

    // ------------------------------------------------------------------------
    // Render-state apply tests (apply_pending_bridge_render_state)
    // ------------------------------------------------------------------------

    use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
    use crate::sim::movement::locomotor::GroundMovePhase;
    use crate::util::fixed_math::{SIM_ONE, SIM_ZERO};

    /// Build a minimal `LocomotorState` for tests. Lists all fields explicitly
    /// with sensible defaults — LocomotorState has no `Default` impl.
    fn make_loco(layer: MovementLayer) -> Option<LocomotorState> {
        Some(LocomotorState {
            kind: LocomotorKind::Drive,
            slot: LocomotorSlot::from_kind(LocomotorKind::Drive),
            powered: true,
            piggyback: None,
            runtime_payload: crate::sim::movement::locomotion::LocomotorRuntimePayload::for_kind(
                LocomotorKind::Drive,
                0,
            ),
            layer,
            phase: GroundMovePhase::Idle,

            speed_multiplier: SIM_ONE,
            speed_fraction: SIM_ONE,
            fly_current_speed: SIM_ZERO,
            altitude: SIM_ZERO,

            balloon_hover: false,
            hover_attack: false,
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
            rot: 0,
            air_progress: SIM_ZERO,
            infantry_wobble_phase: 0.0,
            subcell_dest: None,
            hover_throttle: crate::util::fixed_math::SIM_ZERO,
            hover_speed_request: crate::util::fixed_math::SIM_ZERO,
            hover_bob_offset: crate::util::fixed_math::SIM_ZERO,
        })
    }

    #[test]
    fn render_state_on_bridge_decoupled_from_loco_layer() {
        // active_layer=Bridge but bridge_update=Unchanged.
        // on_bridge must retain its prior value (does NOT become true just because layer is Bridge).
        let mut loco = make_loco(MovementLayer::Ground);
        let mut occ: Option<BridgeOccupancy> = None;
        let mut on_b = false;
        apply_pending_bridge_render_state(
            &mut loco,
            &mut occ,
            &mut on_b,
            MovementLayer::Bridge,
            BridgeStateUpdate::Unchanged,
            42,
        );
        assert_eq!(loco.as_ref().unwrap().layer, MovementLayer::Bridge);
        assert!(!on_b, "on_bridge must NOT be derived from active_layer");
        assert!(occ.is_none(), "bridge_occupancy must be unchanged");
    }

    #[test]
    fn render_state_ramp_going_up_keeps_on_bridge_false() {
        // Going up onto a ramp: A*'s path puts the ramp on Bridge layer, but the
        // predicate doesn't fire Enter until Ramp→Body next tick. So this tick:
        //   active_layer = Bridge, bridge_update = Unchanged, on_bridge = false (prior).
        let mut loco = make_loco(MovementLayer::Ground);
        let mut occ: Option<BridgeOccupancy> = None;
        let mut on_b = false;
        apply_pending_bridge_render_state(
            &mut loco,
            &mut occ,
            &mut on_b,
            MovementLayer::Bridge,
            BridgeStateUpdate::Unchanged,
            42,
        );
        assert_eq!(loco.as_ref().unwrap().layer, MovementLayer::Bridge);
        assert!(!on_b, "on_bridge must stay false on the ramp tick going up");
    }

    #[test]
    fn render_state_ramp_going_down_keeps_on_bridge_true() {
        // Coming off a bridge: A*'s path puts the ramp on Ground layer (is_at_bridge_level
        // returns false), but the predicate hasn't fired Exit yet. on_bridge stays true.
        let mut loco = make_loco(MovementLayer::Bridge);
        let mut occ = Some(BridgeOccupancy { deck_level: 4 });
        let mut on_b = true;
        apply_pending_bridge_render_state(
            &mut loco,
            &mut occ,
            &mut on_b,
            MovementLayer::Ground,
            BridgeStateUpdate::Unchanged,
            42,
        );
        assert_eq!(loco.as_ref().unwrap().layer, MovementLayer::Ground);
        assert!(on_b, "on_bridge must stay true on the ramp tick going down");
        assert!(
            occ.is_some(),
            "bridge_occupancy must be unchanged on Unchanged"
        );
    }

    #[test]
    fn render_state_set_writes_occupancy() {
        let mut loco = make_loco(MovementLayer::Bridge);
        let mut occ: Option<BridgeOccupancy> = None;
        let mut on_b = false;
        apply_pending_bridge_render_state(
            &mut loco,
            &mut occ,
            &mut on_b,
            MovementLayer::Bridge,
            BridgeStateUpdate::Set(4),
            42,
        );
        assert!(on_b);
        assert_eq!(occ.unwrap().deck_level, 4);
    }

    #[test]
    fn render_state_clear_drops_occupancy() {
        let mut loco = make_loco(MovementLayer::Ground);
        let mut occ = Some(BridgeOccupancy { deck_level: 4 });
        let mut on_b = true;
        apply_pending_bridge_render_state(
            &mut loco,
            &mut occ,
            &mut on_b,
            MovementLayer::Ground,
            BridgeStateUpdate::Clear,
            42,
        );
        assert!(!on_b);
        assert!(occ.is_none());
    }
}

#[cfg(test)]
mod bridge_constant_tests {
    use super::*;

    /// Ship braking clearance and the deck snap height are the same physical
    /// fact; if `LEPTONS_PER_LEVEL` is ever corrected, both must move.
    #[test]
    fn bridge_z_offset_matches_deck_height() {
        assert_eq!(
            BRIDGE_Z_OFFSET,
            SimFixed::from_num(crate::sim::map::bridge_topology::BRIDGE_DECK_HEIGHT_LEPTONS)
        );
    }

    /// RESIDUAL - gamemd 0x005F6A70 `ObjectClass::ShouldBeOnBridge`, its
    /// FootClass override 0x004DDC40, and the reachability queries they feed.
    ///
    /// Corrected 2026-08-19. An earlier note in this file claimed this
    /// function was the native counterpart of `compute_bridge_transition` and
    /// that the port diverged from it. That pairing was wrong, and acting on
    /// it would have replaced a correct port. Slot +0xBC is genuinely
    /// ShouldBeOnBridge, but nothing behind that slot writes the OnBridge byte:
    ///
    /// * The transition really is the locomotor block cited on
    ///   `compute_bridge_transition` - the only `+0x8C = 1` stores on the
    ///   ground-movement path are 0x0075C179 (Walk) and 0x004B1830 / 0x004B2586
    ///   (Drive), each guarded by the level-equality and flag tests ported here.
    /// * The boundary-time clear is `FootClass::PerCellProcess`'s own tail:
    ///   `if (OnBridge && !(currentCell->+0x140 & 0x100)) vtable+0xEC`, where
    ///   +0xEC is `ObjectClass::DropIn` 0x005F4160 and the byte it zeroes at
    ///   `this+0x8C` is OnBridge.
    /// * `ShouldBeOnBridge` takes its candidate coordinate from vtable+0x4C,
    ///   which for a Foot is `FootClass::GetDestinationCoords` 0x004DBDF0 - the
    ///   NavCom destination or tube exit, not the next cell. Its result is
    ///   consumed as the source-side on-bridge argument of
    ///   `MapClass::Can_Reach_Zone` 0x0056D100. Three call sites verified with
    ///   the receiver read out of the disassembly - `FootClass::Locomotion_AI`
    ///   0x005210E9, `WalkLocomotionClass::ProcessMovement` 0x0075B163 and
    ///   `FootClass::PerCellProcess` 0x004D8BE6 - each pushing the result, then
    ///   `TechnoType+0x5B4` as the MovementZone, into the same query. Further
    ///   `[reg+0xBC]` sites on what is probably the same slot
    ///   (`FootClass::CanReachDestination` 0x004D38F3,
    ///   `TechnoClass::Set_Destination` 0x00741FE3,
    ///   `FootClass::Is_Cell_Harvestable` 0x004DCF6F) have NOT had their
    ///   receiver verified individually and are candidates only.
    ///
    /// What IS unported, and it is only this: gamemd answers those queries on
    /// the layer `ShouldBeOnBridge` returns, which is the mover's OnBridge byte
    /// *adjusted by where it is headed* -
    ///
    /// ```text
    /// if (!OnBridge && (gh_location - gh_destination) > 3 * 104
    ///     && (destinationCell->+0x140 & 0x100))   return 1;
    /// if ( OnBridge && (gh_destination - gh_location) > 3 * 104) return 0;
    /// return OnBridge;
    /// ```
    ///
    /// where both terms are `CellClass::GetGroundHeight` 0x00578080 results in
    /// leptons at exact coordinates, so they carry slope interpolation and are
    /// terrain heights, not deck heights. `104` is `g_nFootLevelHeightLeptons`
    /// 0x00AC13C8; its two derived globals, 2x for the flight threshold and 4x
    /// for the 416-lepton deck offset, are already pinned in `util::lepton`.
    /// VERA instead passes the mover's raw layer as `start_layer` and the goal
    /// cell's own bridge flag as `goal_layer` (`movement_path::goal_zone_layer`,
    /// `zone_map::ZoneGrid::can_reach`).
    ///
    /// Trigger: a reachability query by a ground unit whose destination ground
    /// is four or more levels below its current ground and carries a bridge
    /// flag, or four or more levels above it - i.e. ordering a unit that is not
    /// yet on a bridge to a cell on the deck, or a unit on the deck to a cell
    /// up on the bank.
    ///
    /// Effect: the zone id is looked up on the wrong layer, so a move order
    /// across a bridge can be judged unreachable and substituted with a nearby
    /// cell, or judged reachable and pathed on the wrong layer. It moves where
    /// the unit ends up, not whether it renders on the deck.
    ///
    /// Frequency: `FootClass::Locomotion_AI` is a per-tick caller, so this is
    /// per tick per moving ground unit whose destination sits four or more
    /// levels off its current ground - not per order. On a bridged map with
    /// traffic crossing, that is often. The retracted note put the cadence at
    /// every cell crossing; the real one is lower than that but not by much,
    /// and it lands on a different system.
    ///
    /// Blocker: the layer arguments are threaded through every move-order
    /// caller as `MovementLayer`, and this needs the mover's location and its
    /// destination coordinate at the same point, in leptons, to compute two
    /// `GetGroundHeight`s. That is a signature change across the order path,
    /// not a change here.
    #[test]
    #[ignore = "gamemd 0x005F6A70 adjusts the reachability on-bridge layer by destination ground height; VERA passes the raw mover layer"]
    fn should_be_on_bridge_is_a_reachability_input_not_the_transition() {
        panic!("unimplemented: destination-height on-bridge layer for Can_Reach_Zone 0x0056D100");
    }
}
