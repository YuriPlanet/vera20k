//! Bridge damage orchestrator — 4-path dispatcher + cascade consumers.
//!
//! Each area-damage continuation calls this owner synchronously after its
//! receivers and before returning to the bullet's animation/cluster tail.
//! The four native admission blocks run in fixed order, selecting drivers from
//! live state. Concrete-body publication is synchronous; other driver outcomes
//! still feed the existing cascade. Tagged collapse notification and debris
//! construction remain required dependencies (see bridge-damage-admission.md).
//!
//! ## Dependency rules
//! Same as sim/world: depends on sim/bridge_state, sim/rng, rules/, map/;
//! never render / ui / audio / net.

use std::collections::BTreeSet;

#[path = "bridge_damage_dispatch.rs"]
mod damage_dispatch;

#[path = "bridge_ground.rs"]
mod ground_fallout;

#[path = "bridge_publication.rs"]
mod live_publication;

pub(crate) use live_publication::repair_from_engineer;

use crate::map::bridge_facts::{
    BRIDGE_FLAG_ANCHOR_SELF, BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_DIRECTION_ZERO,
    BRIDGE_FLAG_STRUCTURAL,
};
use crate::map::resolved_terrain::{BridgeDirection, ResolvedTerrainGrid};
use crate::rules::ruleset::RuleSet;
use crate::sim::bridge_state::{
    Axis, BridgeCellRole, BridgeDamageEvent, BridgeOverlayProjectionOp,
    BridgeRuntimeCell, BridgeRuntimeState, DamageState, DispatchPath, StateOutcome,
};
use crate::sim::world::Simulation;
use crate::sim::{intern::InternedId, rng::SimRng};
use crate::util::fixed_math::SimFixed;
use crate::util::lepton::CELL_CENTER_LEPTON;

/// Apply bridge inputs through the four native admission blocks.
///
/// Per-event behavior:
/// 1. Outer gate: if `SpecialFlags::DestroyableBridges` is clear, bail
///    early — bridges are immune.
/// 2. For each event, evaluate paths in fixed order
///    concrete/wood admission, then low/high direct-overlay admission.
/// 3. For each matching path, run the per-path RNG gate against
///    BridgeStrength (`damage > rand(1..=BridgeStrength)`). IonCannon
///    bypasses the gate.
/// 4. State-machine paths get up to 3 retries when the warhead is
///    IonCannon (4 attempts total). Direct-overlay paths are single-shot.
/// 5. All four blocks run against live post-callback state, even after a
///    prior block succeeds. Successful calls detach the targeted cell.
///
/// Returns `true` if any event in the batch produced a `StateOutcome::Collapsed`
/// — i.e. at least one bridge cell transitioned to `DamageState::Destroyed`.
/// Callers use this to signal `TickResult.bridge_state_changed` so the app
/// consumes the already-published navigation and refreshes presentation.
///
/// Cascade side-effects (kill / DropIn / debris / rim / zone) run unconditionally
/// when matching outcomes are present in this batch — they don't depend on
/// the return value.
#[cfg(test)]
pub(crate) fn apply_bridge_damage_events(
    sim: &mut Simulation,
    rules: &RuleSet,
    events: &[BridgeDamageEvent],
) -> bool {
    apply_bridge_damage_events_with_overlay_registry(sim, rules, events, None)
}

pub(crate) fn apply_bridge_damage_events_with_overlay_registry(
    sim: &mut Simulation,
    rules: &RuleSet,
    events: &[BridgeDamageEvent],
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let mut collapsed = false;
    // Finish each event's existing callbacks before another event can enter
    // the live body driver. Otherwise its immediate fallout would overtake an
    // earlier event's still-pending direct/head fallout.
    for event in events {
        collapsed |= apply_one_bridge_damage_event(sim, rules, event, overlay_registry);
    }
    collapsed
}

fn apply_one_bridge_damage_event(
    sim: &mut Simulation,
    rules: &RuleSet,
    event: &BridgeDamageEvent,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let events = std::slice::from_ref(event);

    // Outer gate + read bridge_strength up front (immutable borrow scope).
    let bridge_strength = match sim.bridge_state.as_ref() {
        Some(bs) if bs.is_destroyable() => bs.bridge_strength(),
        _ => return false,
    };

    // The structural body publishes synchronously. Other drivers still return
    // outcomes for their existing cascade below.
    let (outcomes, published_collapse) = run_dispatch_loop(
        sim,
        events,
        bridge_strength,
        Some((rules, overlay_registry)),
    );
    // `ToggleBridgePavement @ 0x0056E990` marks each changed cell before its
    // direction-0..7 recursion. Outcomes retain that pre-order per event.
    // Keep this sequence until the collapse dirty set is known so the Rust
    // presentation generation advances once for the native damage batch.
    let damaged_variant_dirty: Vec<(u16, u16)> = outcomes
        .iter()
        .flat_map(|outcome| outcome.damaged_variant_cells().iter().copied())
        .collect();
    project_pending_low_bridge_overlay_writes(sim, overlay_registry);

    // Aggregate destroyed cells + the subset receiving BlowUpBridge from
    // the dispatcher's outcomes. BTreeSet keeps deterministic order for
    // the cascade walk.
    let mut destroyed_set: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut blow_up_cells: Vec<(u16, u16)> = Vec::new();
    let mut radar_dirty: BTreeSet<(u16, u16)> = BTreeSet::new();
    for outcome in &outcomes {
        if let StateOutcome::Collapsed {
            destroyed_cells,
            set_bridge_direction,
            radar_cells,
            ..
        } = outcome
        {
            destroyed_set.extend(destroyed_cells.iter().copied());
            radar_dirty.extend(radar_cells.iter().copied());
            for (cell, _slot, action) in &set_bridge_direction.actions {
                if matches!(action, crate::sim::bridge_specs::CellAction::BlowUpBridge) {
                    blow_up_cells.push(*cell);
                    destroyed_set.insert(*cell);
                }
            }
        }
    }

    // BlowUpBridge fallout is per write-cell: ground occupants die with
    // C4Warhead semantics, bridge-deck occupants DropIn, then that cell emits
    // debris. Keeping the effects inside this helper preserves the binary's
    // per-cell fallout order instead of batching kills, drops, and debris.
    for &(rx, ry) in &blow_up_cells {
        blow_up_bridge_cell_fallout(sim, rules, rx, ry, overlay_registry);
    }

    // Aggregate rim cells + zones-dirty flag from the dispatcher's
    // outcomes so the trailing cascade hooks see them in one pass.
    let mut rim_cells: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut any_zones_dirty = false;
    for outcome in &outcomes {
        if let StateOutcome::Collapsed {
            adjacent_bridges_dirty,
            zones_dirty,
            ..
        } = outcome
        {
            rim_cells.extend(adjacent_bridges_dirty.iter().copied());
            any_zones_dirty |= *zones_dirty;
        }
    }

    // Cascade Step 4: rim refresh (HIGH §11.9). Stub today — see helper.
    update_adjacent_bridges(sim, &rim_cells);
    project_pending_low_bridge_overlay_writes(sim, overlay_registry);

    // Cascade Step 5: TriggerEvent 31 broadcast (HIGH §11.3). No-op on
    // skirmish; hook stub for future campaign / map-trigger support.
    notify_bridge_span_collapse(sim, &destroyed_set);

    // Cascade Step 6: zone graph rebuild (HIGH §12.8). Triggered when
    // any final-stage walker cell flagged the bridge endpoint records
    // dirty.
    refresh_bridge_zones_if_dirty(sim, rules, any_zones_dirty);

    // BR-16: feed the minimap radar-dirty channel — the collapsed triple plus
    // every cascade-leaf cell touched (carried in each outcome's `radar_cells`),
    // unioned with the destroyed/BlowUpBridge set so the SetBridgeDirection
    // cells are covered too. Same channel the engineer-repair path uses. The
    // union may harmlessly over-mark a cell that did not change this tick (e.g.
    // an already-Destroyed perpendicular neighbor); the minimap recomputes its
    // color from current bridge state, so over-marking is a render-side no-op.
    radar_dirty.extend(destroyed_set.iter().copied());
    sim.mark_radar_terrain_dirty_cells(
        damaged_variant_dirty
            .into_iter()
            .chain(radar_dirty.iter().copied()),
    );

    // state_changed = "at least one cell collapsed this batch". The destroyed_set
    // is built from StateOutcome::Collapsed outcomes earlier in this function;
    // if it's non-empty, real work happened.
    published_collapse || !destroyed_set.is_empty()
}

/// Bridge-collapse dispatch from a `BridgeRepairHut` death event (C4 timer
/// expired, demo-truck explosion). Chooses low/high from hut-local evidence,
/// finds an overlay entry directly or through a bounded bridge/ramp fallback,
/// then runs the direct-overlay collapse sweep and the same BlowUpBridge
/// cascade as `apply_bridge_damage_events`.
///
/// Returns `true` if any bridge cell transitioned (caller ORs into
/// `bridge_state_changed` so the app rebuilds the PathGrid).
///
/// Caller ensures the hut itself is not damaged — the hut survives the
/// collapse, mirroring the original game's `BridgeRepairHut` death branch.
#[cfg(test)]
/// `MapClass::DestroyBridge_High_OnHutDeath` 0x00574000 and
/// `DestroyBridge_Low_OnHutDeath` 0x00574C20 — the CABHUT death entry.
/// Both callers are `BombClass::Detonate` 0x00438720 (callsite 0x0043896A)
/// and `BuildingClass::Update` 0x0043FB20 (callsite 0x00440301). The 5x5
/// overlay scan hands its first hit to
/// `MapClass::DestroyBridgeFromCell_Low` 0x00574780 /
/// `_High` 0x005749C0, which classify the anchor overlay into the NS or EW
/// band, walk back up to two cells to the canonical edge anchor, and call
/// the matching `CollapseBridge_*`.
#[cfg(test)]
pub(crate) fn dispatch_bridge_collapse_from_hut(
    sim: &mut Simulation,
    rules: &RuleSet,
    hut_center: (u16, u16),
) -> bool {
    dispatch_bridge_collapse_from_hut_with_overlay_registry(sim, rules, hut_center, None)
}

pub(crate) fn dispatch_bridge_collapse_from_hut_with_overlay_registry(
    sim: &mut Simulation,
    rules: &RuleSet,
    hut_center: (u16, u16),
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    let scan: Vec<(u16, u16)> = hut_destroy_5x5_scan(hut_center).collect();
    let family = choose_hut_bridge_family(sim, &scan);

    let fallback_plan = build_hut_fallback_plan(sim, hut_center);

    // gamemd's hut-death dispatch runs a BOUNDED 4-step walker, NOT a
    // full-span collapse. Per `BRIDGE_COLLAPSE_CHAIN_MECHANISM_GHIDRA_REPORT.md`
    // §4: `CollapseBridge_*_*` measures both axial extents from the seed,
    // shifts the start cell toward the shorter side, then walks at most 4
    // axial cells in the longer direction calling `DestroyBridge_*` per
    // cell. Each per-cell call writes a 3-cell axial overlay range plus
    // perpendicular ApplyBridgeDestruction (3×3 grid). Net cell coverage
    // ~18 cells for a 3-wide bridge (3 perp × 6 axial after overlap).
    let mut outcomes: Vec<StateOutcome> = Vec::new();
    let mut fallback_zones_dirty = false;
    let mut fallback_adjacent_dirty_anchor = None;
    let mut anim_spawns = Vec::new();
    {
        let Some(terrain) = sim.resolved_terrain.as_mut() else {
            return false;
        };
        let Some(bs) = sim.bridge_state.as_mut() else {
            return false;
        };
        let mut presentation = BridgePresentationContext {
            // bridge collapse/repair — scenario stream. Direct field (NOT bridge_rng()):
            // sits inside a live sim.bridge_state borrow.
            rng: &mut sim.scenario_rng,
            anim_spawns: &mut anim_spawns,
            bridge_explosions: &sim.bridge_explosions,
        };

        // Look for a seed cell whose overlay is already in the destroy-band.
        // The no-overlay fallback below is a separate flag/ramp walk, not
        // another traced overlay search.
        let seed_axis = find_destroy_overlay_seed(bs, &scan, family);

        if let Some((seed_rx, seed_ry, axis)) = seed_axis {
            outcomes.extend(run_hut_collapse_bounded(
                bs,
                terrain,
                &mut presentation,
                family,
                axis,
                seed_rx,
                seed_ry,
            ));
        } else {
            let fallback = run_hut_fallback_plan(bs, terrain, fallback_plan);
            outcomes.extend(fallback.outcomes);
            fallback_zones_dirty = fallback.zones_dirty;
            fallback_adjacent_dirty_anchor = fallback.adjacent_dirty_anchor;
        }
    }
    for descriptor in anim_spawns {
        construct_bridge_explosion(sim, rules, descriptor);
    }

    apply_hut_bridge_execution(
        sim,
        rules,
        &outcomes,
        fallback_zones_dirty,
        fallback_adjacent_dirty_anchor,
        overlay_registry,
    )
}

/// Overlay values a fully collapsed span leaves on its anchor: `0xE7` / `0xE8`
/// on the high side, `0x64` / `0x65` on the low side. `DestroyBridgeWalker_*`
/// writes them on final collapse and nothing else produces them.
const HIGH_COLLAPSED_ANCHORS: [u8; 2] = [0xE7, 0xE8];
const LOW_COLLAPSED_ANCHORS: [u8; 2] = [0x64, 0x65];

/// The overlay half of `MapClass::FindBridgeConnection_Predicate` 0x00587410 -
/// the cursor-side "is there anything here to repair" test behind the engineer
/// cursor over a bridge repair hut.
///
/// Native shape: scan the 5x5 block around the hovered cell; a cell whose
/// overlay falls in a destroy band selects the low or high family, and the walk
/// then follows the axis PERPENDICULAR to the overlay's own class - an NS-class
/// overlay walks along X, an EW-class overlay along Y - in both directions,
/// continuing while the overlay stays inside the family band and returning true
/// the moment a collapsed anchor appears.
///
/// **Two things about the native's branch selection are NOT reproduced here,
/// and neither is a corner case.**
///
/// 1. *Which branch runs.* All four scan cases write the same `[ESP+0x12]`
///    selector, read once after the loop at 0x005876BA, so whichever of the
///    four matched LAST decides whether the native walks overlays (this
///    function) or walks `BridgeRecord`s through the per-tile geometry tables
///    at 0x0082AA04 / 0x0082AA24 / 0x0082AA44. This port always walks overlays.
/// 2. *Per-cell precedence.* The 5x5 body is a strict if / else-if chain that
///    tests `cell+0x38` against the two tileset windows at 0x00587483 and
///    0x00587503 BEFORE it looks at `cell+0x44` against either overlay band at
///    0x00587580 / 0x00587613. A cell that is both a bridge iso-tile and
///    carries a destroy-band overlay is therefore a TILESET match and its
///    overlay is never read. This port has no tile-index reader at all, so it
///    would treat such a cell as an overlay match.
///
/// Whether cells satisfying both conditions occur on stock maps is UNCHECKED,
/// and so is the record branch's answer for an intact span: its only
/// true-returns are a record byte `+0x08` of zero and a coordinate matching
/// neither endpoint, and the "inactive" reading of `+0x08` is itself
/// UNVERIFIED. Do not assume the two branches agree anywhere. What IS
/// established is that this port is correct whenever the native takes the
/// overlay branch. See
/// `bridge_hut_repair_cursor_always_takes_the_overlay_branch`.
pub(crate) fn bridge_hut_has_collapsed_span(sim: &Simulation, hut_center: (u16, u16)) -> bool {
    let Some(bs) = sim.bridge_state.as_ref() else {
        return false;
    };
    hut_span_has_collapsed_anchor(bs, hut_center)
}

/// State-only half of [`bridge_hut_has_collapsed_span`], split out so the walk
/// can be tested without standing up a `Simulation`.
///
/// Seed selection follows 0x00587410 exactly, and it is not what it looks
/// like: the native's 5x5 loop has **no break**. Every one of its four cases
/// falls through to `INC EDI` at 0x005876A6, so the cell it finally walks from
/// is the LAST cell that matched, not the first, and the iteration is Y-major
/// (`EBP` outer adds to the coord's Y at 0x00587443, `EDI` inner adds to X at
/// 0x00587447). Scanning every cell and accepting any hit would be more
/// permissive than the binary, which is the direction of the bug this port
/// exists to remove.
pub(crate) fn hut_span_has_collapsed_anchor(
    bs: &BridgeRuntimeState,
    hut_center: (u16, u16),
) -> bool {
    let Some((seed, axis, anchors, is_high)) = hut_scan_last_overlay_seed(bs, hut_center) else {
        return false;
    };
    // Perpendicular convention: an NS-class overlay is walked along X.
    let walk_axis = match axis {
        Axis::NS => Axis::EW,
        Axis::EW => Axis::NS,
    };
    [1i16, -1]
        .into_iter()
        .any(|step| walk_span_for_collapsed_anchor(bs, seed, walk_axis, step, anchors, is_high))
}

/// The native's surviving seed: the last destroy-band cell in Y-major order
/// over the 5x5 block. Returns its family and axis alongside it.
fn hut_scan_last_overlay_seed(
    bs: &BridgeRuntimeState,
    hut_center: (u16, u16),
) -> Option<((u16, u16), Axis, &'static [u8; 2], bool)> {
    let mut seed = None;
    for (rx, ry) in hut_scan_5x5_y_major(hut_center) {
        let Some(overlay) = bs.cell(rx, ry).map(|c| c.overlay_byte) else {
            continue;
        };
        if let Some(axis) = BridgeRuntimeState::high_destroy_overlay_axis(overlay) {
            seed = Some(((rx, ry), axis, &HIGH_COLLAPSED_ANCHORS, true));
        } else if let Some(axis) = BridgeRuntimeState::low_destroy_overlay_axis(overlay) {
            seed = Some(((rx, ry), axis, &LOW_COLLAPSED_ANCHORS, false));
        }
    }
    seed
}

/// 0x00587410's scan order: outer counter on Y, inner on X. Distinct from
/// [`hut_destroy_5x5_scan`], which is the X-major order the CABHUT death path
/// uses; the two natives genuinely disagree and must not be shared.
fn hut_scan_5x5_y_major(center: (u16, u16)) -> impl Iterator<Item = (u16, u16)> {
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

fn walk_span_for_collapsed_anchor(
    bs: &BridgeRuntimeState,
    from: (u16, u16),
    axis: Axis,
    step: i16,
    anchors: &[u8; 2],
    is_high: bool,
) -> bool {
    let mut cur = from;
    loop {
        let Some(overlay) = bs.cell(cur.0, cur.1).map(|c| c.overlay_byte) else {
            return false;
        };
        let in_band = if is_high {
            BridgeRuntimeState::is_high_destroy_overlay(overlay)
        } else {
            BridgeRuntimeState::is_low_destroy_overlay(overlay)
        };
        if !in_band {
            return false;
        }
        if anchors.contains(&overlay) {
            return true;
        }
        let Some(next) = step_axis(cur, axis, step) else {
            return false;
        };
        cur = next;
    }
}

fn hut_destroy_5x5_scan(center: (u16, u16)) -> impl Iterator<Item = (u16, u16)> {
    let (cx, cy) = (center.0 as i32, center.1 as i32);
    (-2..=2i32).flat_map(move |dx| {
        (-2..=2i32).filter_map(move |dy| {
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

/// Find the first cell in `scan` whose overlay maps to a physical collapse
/// sweep axis for this bridge family.
///
/// This is intentionally separate from `BridgeRuntimeState::*_destroy_overlay_axis`:
/// those helpers classify the per-cell walker/write family, while the hut
/// `CollapseBridge_*` entry walks along the bridge's physical span. The binary
/// dispatcher proves these are opposite for the bridge overlay subranges:
/// `0xCD`/`0x4A` families route to `CollapseBridge_EW_*`, which steps in X.
fn find_destroy_overlay_seed(
    bridge_state: &BridgeRuntimeState,
    scan: &[(u16, u16)],
    family: HutBridgeFamily,
) -> Option<(u16, u16, Axis)> {
    scan.iter().copied().find_map(|(rx, ry)| {
        let overlay = bridge_state.cell(rx, ry).map(|c| c.overlay_byte)?;
        let axis = physical_span_axis_for_destroy_overlay(family, overlay)?;
        let seed = canonicalize_hut_destroy_seed(bridge_state, family, (rx, ry), axis)?;
        Some((seed.0, seed.1, axis))
    })
}

fn canonicalize_hut_destroy_seed(
    bridge_state: &BridgeRuntimeState,
    family: HutBridgeFamily,
    matched: (u16, u16),
    physical_axis: Axis,
) -> Option<(u16, u16)> {
    // `DestroyBridgeFromCell_*` recenters the first hut-scan hit onto the
    // bridge lane before calling the bounded walker. The probes are
    // perpendicular to the physical span: EW walkers probe Y, NS walkers probe X.
    let probe_axis = match physical_axis {
        Axis::EW => Axis::NS,
        Axis::NS => Axis::EW,
    };
    let back_one = step_axis(matched, probe_axis, -1);
    let back_two = step_axis(matched, probe_axis, -2);
    let back_one_in_band = back_one
        .and_then(|(rx, ry)| bridge_state.cell(rx, ry).map(|c| c.overlay_byte))
        .is_some_and(|overlay| in_bridge_band(family, overlay));
    if !back_one_in_band {
        return step_axis(matched, probe_axis, 1);
    }
    let back_two_in_band = back_two
        .and_then(|(rx, ry)| bridge_state.cell(rx, ry).map(|c| c.overlay_byte))
        .is_some_and(|overlay| in_bridge_band(family, overlay));
    if back_two_in_band {
        back_one
    } else {
        Some(matched)
    }
}

fn apply_hut_bridge_execution(
    sim: &mut Simulation,
    rules: &RuleSet,
    outcomes: &[StateOutcome],
    extra_zones_dirty: bool,
    extra_adjacent_dirty_anchor: Option<(u16, u16)>,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) -> bool {
    if outcomes.is_empty() && !extra_zones_dirty && extra_adjacent_dirty_anchor.is_none() {
        return false;
    }

    for outcome in outcomes {
        apply_runtime_bridge_flag_transcript_from_outcome(sim, outcome);
    }

    let damaged_variant_dirty: Vec<(u16, u16)> = outcomes
        .iter()
        .flat_map(|outcome| outcome.damaged_variant_cells().iter().copied())
        .collect();

    let mut destroyed_set: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut blow_up_cells: Vec<(u16, u16)> = Vec::new();
    let mut rim_cells: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut radar_dirty: BTreeSet<(u16, u16)> = BTreeSet::new();
    let mut any_zones_dirty = false;
    for outcome in outcomes {
        if let StateOutcome::Collapsed {
            destroyed_cells,
            set_bridge_direction,
            adjacent_bridges_dirty,
            zones_dirty,
            radar_cells,
            ..
        } = outcome
        {
            destroyed_set.extend(destroyed_cells.iter().copied());
            radar_dirty.extend(radar_cells.iter().copied());
            for (cell, _slot, action) in &set_bridge_direction.actions {
                if matches!(action, crate::sim::bridge_specs::CellAction::BlowUpBridge) {
                    blow_up_cells.push(*cell);
                    destroyed_set.insert(*cell);
                }
            }
            rim_cells.extend(adjacent_bridges_dirty.iter().copied());
            any_zones_dirty |= *zones_dirty;
        }
    }
    if let Some(anchor) = extra_adjacent_dirty_anchor {
        rim_cells.insert(anchor);
    }
    any_zones_dirty |= extra_zones_dirty;

    project_pending_low_bridge_overlay_writes(sim, overlay_registry);
    for &(rx, ry) in &blow_up_cells {
        blow_up_bridge_cell_fallout(sim, rules, rx, ry, overlay_registry);
    }
    update_adjacent_bridges(sim, &rim_cells);
    project_pending_low_bridge_overlay_writes(sim, overlay_registry);
    notify_bridge_span_collapse(sim, &destroyed_set);
    refresh_bridge_zones_if_dirty(sim, rules, any_zones_dirty);

    // BR-16: feed the minimap radar-dirty channel (see apply_bridge_damage_events).
    radar_dirty.extend(destroyed_set.iter().copied());
    sim.mark_radar_terrain_dirty_cells(
        damaged_variant_dirty
            .into_iter()
            .chain(radar_dirty.iter().copied()),
    );

    !destroyed_set.is_empty() || extra_zones_dirty || extra_adjacent_dirty_anchor.is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HutBridgeFamily {
    Low,
    High,
}

struct BridgePresentationContext<'a> {
    rng: &'a mut SimRng,
    /// `BridgeExplosions` constructor rows in draw order. The walker runs
    /// inside the bridge-state and terrain borrows, so the owner constructs
    /// them as soon as those end.
    anim_spawns: &'a mut Vec<crate::sim::components::AnimClassSpawnDescriptor>,
    bridge_explosions: &'a [InternedId],
}

const HUT_FALLBACK_DIRS: [(i16, i16); 8] = [
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
];
const HUT_FALLBACK_STARTER_MASK: u32 = BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DESTROYED_OR_RAMP;
// Hard cap of the bounded walker: gamemd's `CollapseBridge_*_*` uses
// `local_2c = 4`. See `BRIDGE_COLLAPSE_CHAIN_MECHANISM_GHIDRA_REPORT.md` §4.
const MAX_HUT_SWEEP_STEPS: usize = 4;
const MAX_HUT_ATTEMPTS_PER_STEP: usize = 3;
const NORMALIZED_RNG_MAX_INCLUSIVE: u32 = 0x7FFF_FFFE;
const NORMALIZED_RNG_DENOMINATOR: u64 = 0x8000_0000;
// Bridge-debris RNG gate boundaries. The original engine compares
// `(double)draw * scale` against 0.95 / 0.5, where `scale` is the bit-exact
// double `2^-31 + 2^-61` (NOT `1/2^31`). The tiny `2^-61` term pushes each
// integer boundary just below the naive `threshold * 2^31`, so a draw landing
// exactly on the float boundary must FAIL the gate. Gate passes iff
// `draw < EXCLUSIVE`. Do NOT "simplify" these back to 2^31-scaled values
// (…466 / 0x4000_0000): the off-by-{2,1} reproduces the float boundary, and a
// spurious pass spends extra slot draws -> lockstep desync.
const BRIDGE_DEBRIS_OUTER_GATE_EXCLUSIVE: u32 = 2_040_109_464;
const BRIDGE_METALLIC_GATE_EXCLUSIVE: u32 = 0x3FFF_FFFF;
const BRIDGE_JITTER_SPAN_LEPTONS: u64 = 50;
const BRIDGE_JITTER_HALF_LEPTONS: i32 = 25;
// The collapse fallout is closed against `CellClass::BlowUpBridge @
// 0x0047DD70`: the outer 95% gate, both jitter draws, the 50% metallic gate, the
// metallic slot draw inside it, the `RandomRanged(1, 5)` start delay and the
// explosion slot draw all match, in order.
//
// RESIDUAL (GSI-04.14, M11b) — `MetallicDebris=` is not constructed.
// - The entries resolve to `[DBRIS*]` AnimTypes with `Bouncer=yes`,
//   `RandomRate=220,600`, `Damage=10/20`, `DamageRadius=50/80`, `Warhead=HE`
//   and `ExpireAnim=`: retail's bridge debris bounces and hurts what it lands
//   on. `AnimStore` has no bouncer arm (`sim::anim_class` module header), so
//   VERA keeps the gate and slot draws and builds nothing. `ReadINI` converts
//   the rate pair to 1..1 and native's `AnimClass::Constructor` still runs
//   `RandomRanged(1, 1)` for it, a draw VERA does not take.
// - The legacy effect list this replaced never drew the debris either: no
//   atlas source lists `MetallicDebris=` names, so their sprites were never
//   loaded and every record was skipped at draw time.
// - Trigger: every bridge span destroyed. Frequency: routine on maps with
//   bridges. Downstream risk: the missing draw shifts the stream for every
//   collapse, so it must land with the damage half rather than alone.
// Safety cap on the extent-measurement walk (Phase 1 of the bounded
// walker). gamemd has no explicit cap — the off-bridge band check
// terminates the walk — but a runaway count would only happen if the
// overlay band check were buggy. 64 cells is well beyond any realistic
// YR bridge length.
const MAX_EXTENT_PROBE: usize = 64;
const MAX_HUT_ENDPOINT_PROBE: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HutFallbackStarter {
    pos: (u16, u16),
    flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HutFallbackPlan {
    NoAcceptedStarter,
    MissingAnchor,
    PureBridgeheadTooLong,
    RampWalk {
        starter: HutFallbackStarter,
        anchor: (u16, u16),
    },
}

#[derive(Debug, Default)]
struct HutFallbackExecution {
    outcomes: Vec<StateOutcome>,
    zones_dirty: bool,
    adjacent_dirty_anchor: Option<(u16, u16)>,
}

fn choose_hut_bridge_family(sim: &Simulation, scan: &[(u16, u16)]) -> HutBridgeFamily {
    if scan
        .iter()
        .any(|&(rx, ry)| is_low_hut_scan_evidence(sim, rx, ry))
    {
        HutBridgeFamily::Low
    } else {
        HutBridgeFamily::High
    }
}

fn is_low_hut_scan_evidence(sim: &Simulation, rx: u16, ry: u16) -> bool {
    bridge_overlay_at(sim, rx, ry).is_some_and(BridgeRuntimeState::is_low_destroy_overlay)
        || sim
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(rx, ry))
            .is_some_and(|cell| cell.is_wood_bridge_repair_tile)
}

fn bridge_overlay_at(sim: &Simulation, rx: u16, ry: u16) -> Option<u8> {
    sim.bridge_state
        .as_ref()
        .and_then(|bs| bs.cell(rx, ry))
        .map(|cell| cell.overlay_byte)
        .or_else(|| {
            sim.resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(rx, ry))
                .and_then(|cell| cell.bridge_layer.as_ref())
                .map(|layer| layer.overlay_id)
        })
}

/// RESIDUAL (GSI-04.14) — the fallback starter and anchor search is a VERA
/// heuristic with no cited native address, unlike the primary hut path, whose
/// 5x5 scan order, seed canonicalisation and repair walk are all pinned to
/// addresses and tests. It runs when the primary path finds no anchor.
///
/// **The native fallback is identified.** It is the tail of
/// `MapClass::DestroyBridge_Low_OnHutDeath` 0x00574C20 and its high twin
/// 0x00574000, decompiled 2026-08-19: when the 5x5 overlay scan finds nothing,
/// the native walks the eight directions out to three cells looking for a cell
/// with `+0x140 & 0x500`, resolves an anchor from that cell's 0x100 / 0x400 /
/// 0x80 bits (pure-bridgehead cells walk up to four perpendicular cells then
/// offset two more), walks forward in direction `(flags & 0x800) ? 6 : 0`
/// calling `ApplyDamageToCell` up to three times on each cell
/// `MapClass::IsBridgeRampTile` 0x005746C0 accepts, and stops when
/// `MapClass::IsLowBridgeEndpointTile` 0x00574600 fires. The structure here —
/// starter search, `find_hut_fallback_ramp_cell`, `apply_hut_damage_retries`,
/// `find_hut_fallback_endpoint_cell` — follows that shape.
/// - Trigger: destroying a `BridgeRepairHut=yes` structure whose span does not
///   resolve through the primary search — an irregular or already-damaged
///   bridge.
/// - Player effect: if the ported shape diverges, the wrong span may collapse,
///   or none at all, where retail picks deterministically.
/// - Frequency: uncommon; the primary path covers the ordinary intact-bridge
///   case that stock maps present.
/// - Downstream risk: a different span collapsing changes occupancy, zone
///   connectivity and which units drop. Now that the native address is known,
///   a term-by-term comparison against 0x00574C20 is the next step rather than
///   more search.
fn build_hut_fallback_plan(sim: &Simulation, hut_center: (u16, u16)) -> HutFallbackPlan {
    let Some(starter) = find_hut_fallback_starter(sim, hut_center) else {
        return HutFallbackPlan::NoAcceptedStarter;
    };
    resolve_hut_fallback_anchor(sim, starter)
}

fn find_hut_fallback_starter(
    sim: &Simulation,
    hut_center: (u16, u16),
) -> Option<HutFallbackStarter> {
    let hut_flags = hut_fallback_flags(sim, hut_center);
    if hut_flags & HUT_FALLBACK_STARTER_MASK != 0 {
        return Some(HutFallbackStarter {
            pos: hut_center,
            flags: hut_flags,
        });
    }

    for &(dx, dy) in &HUT_FALLBACK_DIRS {
        for distance in 1..=3i16 {
            let rx = hut_center.0 as i32 + dx as i32 * distance as i32;
            let ry = hut_center.1 as i32 + dy as i32 * distance as i32;
            if rx < 0 || ry < 0 || rx > u16::MAX as i32 || ry > u16::MAX as i32 {
                continue;
            }
            let pos = (rx as u16, ry as u16);
            let flags = hut_fallback_flags(sim, pos);
            if flags & HUT_FALLBACK_STARTER_MASK != 0 {
                return Some(HutFallbackStarter { pos, flags });
            }
        }
    }

    None
}

fn resolve_hut_fallback_anchor(sim: &Simulation, starter: HutFallbackStarter) -> HutFallbackPlan {
    if starter.flags & BRIDGE_FLAG_STRUCTURAL != 0 {
        if starter.flags & BRIDGE_FLAG_ANCHOR_SELF != 0 {
            return HutFallbackPlan::RampWalk {
                starter,
                anchor: starter.pos,
            };
        }
        let anchor = sim
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(starter.pos.0, starter.pos.1))
            .and_then(|cell| cell.bridge_facts.anchor)
            .map(|relation| relation.anchor)
            .or_else(|| {
                sim.bridge_state
                    .as_ref()
                    .and_then(|bs| bs.cell(starter.pos.0, starter.pos.1))
                    .and_then(|cell| cell.anchor_span_id)
                    .and_then(|span_id| sim.bridge_state.as_ref()?.anchor_span(span_id))
                    .map(|span| span.anchor)
            });
        return anchor.map_or(HutFallbackPlan::MissingAnchor, |anchor| {
            HutFallbackPlan::RampWalk { starter, anchor }
        });
    }

    if starter.flags & BRIDGE_FLAG_DESTROYED_OR_RAMP != 0 {
        return resolve_pure_bridgehead_anchor(sim, starter);
    }

    HutFallbackPlan::NoAcceptedStarter
}

fn resolve_pure_bridgehead_anchor(
    sim: &Simulation,
    starter: HutFallbackStarter,
) -> HutFallbackPlan {
    let (scan_dir, opposite_dir) = if starter.flags & BRIDGE_FLAG_DIRECTION_ZERO != 0 {
        (4usize, 0usize)
    } else {
        (2usize, 6usize)
    };
    let mut cur = starter.pos;
    let mut continuations = 0usize;
    loop {
        let Some(next) = step_hut_dir(cur, scan_dir) else {
            return HutFallbackPlan::MissingAnchor;
        };
        let flags = hut_fallback_flags(sim, next);
        if flags & BRIDGE_FLAG_DESTROYED_OR_RAMP == 0 {
            let Some(anchor) =
                step_hut_dir(next, opposite_dir).and_then(|p| step_hut_dir(p, opposite_dir))
            else {
                return HutFallbackPlan::MissingAnchor;
            };
            return HutFallbackPlan::RampWalk { starter, anchor };
        }
        continuations += 1;
        if continuations >= 4 {
            return HutFallbackPlan::PureBridgeheadTooLong;
        }
        cur = next;
    }
}

fn hut_fallback_flags(sim: &Simulation, pos: (u16, u16)) -> u32 {
    sim.resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(pos.0, pos.1))
        .map(|cell| cell.bridge_flags())
        .unwrap_or(0)
}

fn run_hut_fallback_plan(
    bridge_state: &mut BridgeRuntimeState,
    terrain: &mut ResolvedTerrainGrid,
    plan: HutFallbackPlan,
) -> HutFallbackExecution {
    let HutFallbackPlan::RampWalk { starter, anchor } = plan else {
        return HutFallbackExecution::default();
    };

    let forward_dir = if starter.flags & BRIDGE_FLAG_DIRECTION_ZERO != 0 {
        6usize
    } else {
        0usize
    };
    let Some(ramp_cell) = find_hut_fallback_ramp_cell(terrain, anchor, forward_dir) else {
        return HutFallbackExecution {
            zones_dirty: true,
            ..Default::default()
        };
    };

    // Native ApplyDamageToCell and every SetBridgeDirection call are
    // synchronous. Keep one exact live flag view for the whole fallback plan:
    // all retries on the ramp cell and the optional dependent endpoint target.
    // Allocated real terrain values and serialized authority remain deferred
    // until apply_hut_bridge_execution. Missing-slot setters and every helper
    // GetCell already mutate the actual shared dummy at their native call
    // points, so retries and dependent targets observe its live interleaving.
    let mut live_flags = terrain.bridge_flag_execution_state();
    let mut execution = HutFallbackExecution::default();
    execution.outcomes.extend(apply_hut_damage_retries(
        bridge_state,
        terrain,
        ramp_cell,
        &mut live_flags,
    ));

    let reverse_dir = (forward_dir + 4) & 7;
    let Some(endpoint) = find_hut_fallback_endpoint_cell(terrain, ramp_cell, reverse_dir) else {
        execution.zones_dirty = true;
        execution.adjacent_dirty_anchor = Some(anchor);
        return execution;
    };

    if hut_endpoint_needs_beyond_damage(terrain, endpoint) {
        if let Some(target) = step_hut_dir(endpoint, forward_dir) {
            execution.outcomes.extend(apply_hut_damage_retries(
                bridge_state,
                terrain,
                target,
                &mut live_flags,
            ));
        }
    }
    execution.zones_dirty = true;
    execution.adjacent_dirty_anchor = Some(anchor);
    execution
}

fn find_hut_fallback_ramp_cell(
    terrain: &ResolvedTerrainGrid,
    anchor: (u16, u16),
    forward_dir: usize,
) -> Option<(u16, u16)> {
    let mut cur = anchor;
    for _ in 0..MAX_EXTENT_PROBE {
        let cell = terrain.cell(cur.0, cur.1)?;
        if cell.bridge_facts.ramp_tile.is_some() {
            return Some(cur);
        }
        cur = step_hut_dir(cur, forward_dir)?;
    }
    None
}

fn find_hut_fallback_endpoint_cell(
    terrain: &ResolvedTerrainGrid,
    ramp_cell: (u16, u16),
    reverse_dir: usize,
) -> Option<(u16, u16)> {
    let mut cur = ramp_cell;
    for _ in 0..MAX_HUT_ENDPOINT_PROBE {
        cur = step_hut_dir(cur, reverse_dir)?;
        let cell = terrain.cell(cur.0, cur.1)?;
        if cell.bridge_facts.ramp_tile.is_some() || cell.bridge_facts.anchor.is_some() {
            return Some(cur);
        }
    }
    None
}

fn hut_endpoint_needs_beyond_damage(terrain: &ResolvedTerrainGrid, endpoint: (u16, u16)) -> bool {
    terrain
        .cell(endpoint.0, endpoint.1)
        .and_then(|cell| cell.bridge_facts.ramp_tile)
        .is_none_or(|tile| tile.relative_tile_index != u16::MAX - 1)
}

fn apply_hut_damage_retries(
    bridge_state: &mut BridgeRuntimeState,
    terrain: &mut ResolvedTerrainGrid,
    target: (u16, u16),
    live_flags: &mut crate::map::resolved_terrain::CellClassBridgeFlagState,
) -> Vec<StateOutcome> {
    if bridge_state.cell(target.0, target.1).is_none() {
        return Vec::new();
    }

    let mut outcomes = Vec::new();
    for _ in 0..MAX_HUT_ATTEMPTS_PER_STEP {
        let outcome =
            apply_hut_damage_to_cell(bridge_state, terrain, target.0, target.1, live_flags);
        let success = outcome.apply_damage_success();
        if outcome.has_effect() {
            outcomes.push(outcome);
        }
        if success {
            break;
        }
    }
    outcomes
}

fn step_hut_dir(pos: (u16, u16), direction: usize) -> Option<(u16, u16)> {
    let (dx, dy) = HUT_FALLBACK_DIRS[direction & 7];
    let rx = pos.0 as i32 + dx as i32;
    let ry = pos.1 as i32 + dy as i32;
    if rx < 0 || ry < 0 || rx > u16::MAX as i32 || ry > u16::MAX as i32 {
        return None;
    }
    Some((rx as u16, ry as u16))
}

fn physical_span_axis_for_destroy_overlay(family: HutBridgeFamily, overlay: u8) -> Option<Axis> {
    let walker_axis = match family {
        HutBridgeFamily::Low => BridgeRuntimeState::low_destroy_overlay_axis(overlay),
        HutBridgeFamily::High => BridgeRuntimeState::high_destroy_overlay_axis(overlay),
    }?;
    match walker_axis {
        Axis::NS => Some(Axis::EW),
        Axis::EW => Some(Axis::NS),
    }
}

fn step_axis(pos: (u16, u16), axis: Axis, dir: i16) -> Option<(u16, u16)> {
    let (rx, ry) = pos;
    match axis {
        Axis::EW => {
            let next = rx as i32 + dir as i32;
            (0..=u16::MAX as i32)
                .contains(&next)
                .then_some((next as u16, ry))
        }
        Axis::NS => {
            let next = ry as i32 + dir as i32;
            (0..=u16::MAX as i32)
                .contains(&next)
                .then_some((rx, next as u16))
        }
    }
}

/// Bounded 4-iteration collapse walker — mirror of gamemd's
/// `MapClass::CollapseBridge_{NS,EW}_{High,Low}` at
/// `0x00575BA0` / `0x00575870` / `0x00575540` / `0x00575220`.
///
/// Per `BRIDGE_COLLAPSE_CHAIN_MECHANISM_GHIDRA_REPORT.md` §4:
///
/// 1. **Extent measurement.** Walk both axial directions from `seed`
///    counting cells still inside the bridge overlay band
///    (`[0xCD..=0xE8]` high / `[0x4A..=0x65]` low). Counts → `back` and
///    `fwd`.
/// 2. **Direction + start.** Step direction is `-1` if `fwd < back`,
///    else `+1` (walk toward the longer-extent side). Start cell is
///    `seed - (back - fwd) / 2` using signed integer division — biases
///    the starting position toward the shorter side so the 4-step walk
///    can cover the maximum bridge length.
/// 3. **4-iteration walker.** For each of `MAX_HUT_SWEEP_STEPS` (= 4)
///    axial steps, call the per-cell primitive `destroy_bridge_high/low`
///    up to `MAX_HUT_ATTEMPTS_PER_STEP` (= 3) retries. Each primitive
///    call writes a 3-cell axial overlay range and triggers
///    `ApplyBridgeDestruction_*` on the X±1 perpendicular columns,
///    producing a 3×3 destruction footprint per call. Step `cur` along
///    the chosen axial direction after each iteration, break early when
///    the next cell leaves the bridge band.
///
/// Net coverage for a 3-wide bridge: ~3 perp × 6 axial = ~18 cells per
/// invocation (axial 3-cell windows overlap by 2 across iterations).
/// For the 1-wide bridges in test fixtures, ~4 axial cells.
fn run_hut_collapse_bounded(
    bridge_state: &mut BridgeRuntimeState,
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    presentation: &mut BridgePresentationContext<'_>,
    family: HutBridgeFamily,
    axis: Axis,
    seed_rx: u16,
    seed_ry: u16,
) -> Vec<StateOutcome> {
    // Phase 1: extent measurement in both axial directions.
    let seed = (seed_rx, seed_ry);
    let back = measure_extent(bridge_state, family, seed, axis, -1);
    let fwd = measure_extent(bridge_state, family, seed, axis, 1);

    // Phase 2: pick step direction + biased start cell. Signed integer
    // division (round toward zero) matches gamemd's Asm `idiv` semantics.
    let step: i16 = if fwd < back { -1 } else { 1 };
    let bias: i32 = (back as i32 - fwd as i32) / 2;
    // start = seed - bias (gamemd: `uVar9 - (iVar11 - iVar10) / 2`).
    let Some(mut cur) = step_axis_by(seed, axis, -bias) else {
        return Vec::new();
    };

    // Phase 3: 4-iteration walker.
    let mut outcomes: Vec<StateOutcome> = Vec::new();
    for _ in 0..MAX_HUT_SWEEP_STEPS {
        spawn_hut_walker_pre_destroy_effects(
            bridge_state,
            terrain,
            presentation,
            family,
            axis,
            cur,
        );
        // Inner retry: a healthy cell takes 2 calls to reach Destroyed
        // (Healthy → Damaged → Destroyed). gamemd's `iVar10 < 3` retry
        // loop covers this.
        for _ in 0..MAX_HUT_ATTEMPTS_PER_STEP {
            let outcome = call_destroy_per_family(bridge_state, terrain, family, cur);
            match outcome {
                StateOutcome::NoChange => {}
                absorbed @ StateOutcome::Absorbed { .. } => {
                    outcomes.push(absorbed);
                    // Retry: the cell took a state step but is not yet
                    // collapsed — another call may push it to Destroyed.
                }
                collapsed @ StateOutcome::Collapsed { .. } => {
                    let success = collapsed.apply_damage_success();
                    outcomes.push(collapsed);
                    if success {
                        break;
                    }
                }
            }
        }

        // Step along the chosen axial direction. Break if the step
        // would leave the map.
        let Some(next) = step_axis(cur, axis, step) else {
            break;
        };
        // Break if the next cell is outside the bridge overlay band.
        // gamemd's check is identical: `cellclass.overlay < 0xCD ||
        // cellclass.overlay > 0xE8` for high (and `0x4A`/`0x65` for low).
        let Some(overlay) = bridge_state.cell(next.0, next.1).map(|c| c.overlay_byte) else {
            break;
        };
        if !in_bridge_band(family, overlay) {
            break;
        }
        cur = next;
    }

    outcomes
}

fn spawn_hut_walker_pre_destroy_effects(
    bridge_state: &BridgeRuntimeState,
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    presentation: &mut BridgePresentationContext<'_>,
    family: HutBridgeFamily,
    physical_axis: Axis,
    center: (u16, u16),
) {
    if presentation.bridge_explosions.is_empty() {
        return;
    }
    let Some(overlay) = bridge_state
        .cell(center.0, center.1)
        .map(|c| c.overlay_byte)
    else {
        return;
    };
    if overlay == hut_walker_terminal_cap(family, physical_axis) {
        return;
    }
    let perpendicular = match physical_axis {
        Axis::EW => Axis::NS,
        Axis::NS => Axis::EW,
    };
    for delta in [-1, 0, 1] {
        if let Some((rx, ry)) = step_axis(center, perpendicular, delta) {
            // All four `CollapseBridge_*` walkers build Z as `MOVSX Cell+0x11B
            // (Level); IMUL [0x00ABDE88]` (`0x00575391` EW_Low, `0x005756B3`
            // NS_Low, `0x005759EC` EW_High, `0x00575D1E` NS_High) and add no
            // deck offset, so these play on the ground or water under a high
            // span. `BlowUpBridge` alone adds the structural deck offset.
            let z = terrain.cell(rx, ry).map(|c| c.level).unwrap_or(0);
            queue_walker_bridge_explosion(presentation, rx, ry, z);
        }
    }
}

fn hut_walker_terminal_cap(family: HutBridgeFamily, physical_axis: Axis) -> u8 {
    match (family, physical_axis) {
        (HutBridgeFamily::High, Axis::EW) => 0xE7,
        (HutBridgeFamily::High, Axis::NS) => 0xE8,
        (HutBridgeFamily::Low, Axis::EW) => 0x64,
        (HutBridgeFamily::Low, Axis::NS) => 0x65,
    }
}

/// Count cells in the bridge overlay band along `axis` in direction
/// `dir` from `seed`. Stops at the first off-band cell, off-map step, or
/// `MAX_EXTENT_PROBE` iterations (safety cap).
fn measure_extent(
    bridge_state: &BridgeRuntimeState,
    family: HutBridgeFamily,
    seed: (u16, u16),
    axis: Axis,
    dir: i16,
) -> u32 {
    let mut count: u32 = 0;
    let mut cur = seed;
    for _ in 0..MAX_EXTENT_PROBE {
        let Some(next) = step_axis(cur, axis, dir) else {
            break;
        };
        let Some(overlay) = bridge_state.cell(next.0, next.1).map(|c| c.overlay_byte) else {
            break;
        };
        if !in_bridge_band(family, overlay) {
            break;
        }
        count += 1;
        cur = next;
    }
    count
}

fn in_bridge_band(family: HutBridgeFamily, overlay: u8) -> bool {
    match family {
        HutBridgeFamily::High => (0xCD..=0xE8).contains(&overlay),
        HutBridgeFamily::Low => (0x4A..=0x65).contains(&overlay),
    }
}

fn call_destroy_per_family(
    bridge_state: &mut BridgeRuntimeState,
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    family: HutBridgeFamily,
    cell: (u16, u16),
) -> StateOutcome {
    match family {
        HutBridgeFamily::High => bridge_state.destroy_bridge_high(cell.0, cell.1, terrain),
        HutBridgeFamily::Low => bridge_state.destroy_bridge_low(cell.0, cell.1, terrain),
    }
}

/// Multi-cell axial step. Saturates to u16 bounds — out-of-map → None.
fn step_axis_by(pos: (u16, u16), axis: Axis, delta: i32) -> Option<(u16, u16)> {
    let (rx, ry) = pos;
    match axis {
        Axis::EW => {
            let next = rx as i32 + delta;
            (0..=u16::MAX as i32)
                .contains(&next)
                .then_some((next as u16, ry))
        }
        Axis::NS => {
            let next = ry as i32 + delta;
            (0..=u16::MAX as i32)
                .contains(&next)
                .then_some((rx, next as u16))
        }
    }
}

fn apply_hut_damage_to_cell(
    bridge_state: &mut BridgeRuntimeState,
    terrain: &mut crate::map::resolved_terrain::ResolvedTerrainGrid,
    rx: u16,
    ry: u16,
    live_flags: &mut crate::map::resolved_terrain::CellClassBridgeFlagState,
) -> StateOutcome {
    let Some(cell) = bridge_state.cell(rx, ry).copied() else {
        return StateOutcome::NoChange;
    };

    if (0x4A..=0x63).contains(&cell.overlay_byte) {
        return bridge_state.destroy_bridge_low(rx, ry, terrain);
    }
    if (0xCD..=0xE6).contains(&cell.overlay_byte) {
        return bridge_state.destroy_bridge_high(rx, ry, terrain);
    }

    let is_high = !hut_cell_is_low_bridge(bridge_state, terrain, rx, ry);
    match cell.role {
        BridgeCellRole::Bridgehead => {
            bridge_state.bridgehead_advance_state_with_flags(rx, ry, is_high, terrain, live_flags)
        }
        BridgeCellRole::Anchor | BridgeCellRole::Body | BridgeCellRole::Tail => {
            bridge_state.body_cell_advance_state_with_flags(rx, ry, is_high, terrain, live_flags)
        }
    }
}

fn hut_cell_is_low_bridge(
    bridge_state: &BridgeRuntimeState,
    terrain: &crate::map::resolved_terrain::ResolvedTerrainGrid,
    rx: u16,
    ry: u16,
) -> bool {
    bridge_state
        .cell(rx, ry)
        .is_some_and(|cell| BridgeRuntimeState::is_low_destroy_overlay(cell.overlay_byte))
        || terrain.cell(rx, ry).is_some_and(|cell| {
            cell.is_wood_bridge_repair_tile
                || cell.bridge_layer.as_ref().is_some_and(|layer| {
                    BridgeRuntimeState::is_low_destroy_overlay(layer.overlay_id)
                })
                || cell
                    .bridge_facts
                    .overlay_id
                    .is_some_and(BridgeRuntimeState::is_low_destroy_overlay)
        })
}

/// Complete ground receivers before the deck pass and debris RNG.
fn blow_up_bridge_cell_fallout(
    sim: &mut Simulation,
    rules: &RuleSet,
    rx: u16,
    ry: u16,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    kill_ground_occupants_at(sim, rules, rx, ry, overlay_registry);
    drop_in_bridge_deck_entities(sim, rx, ry);
    let mut one_cell = BTreeSet::new();
    one_cell.insert((rx, ry));
    spawn_bridge_debris(sim, rules, &one_cell);
}

/// `CellClass::BlowUpBridge @ 0x0047DDAE`: every ground occupant takes
/// `ReceiveDamage` (`+0x16C`) with its own HP and `C4Warhead=`, so the kill
/// runs the normal death branch including `Death_Announcement` (`+0x3B8`).
pub(super) fn kill_ground_occupants_at(
    sim: &mut Simulation,
    rules: &RuleSet,
    rx: u16,
    ry: u16,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    ground_fallout::apply(sim, rules, overlay_registry, rx, ry);
}

/// Rim refresh. For each just-collapsed rim cell, walk along the bridge
/// in the direction of an adjacent bridge head (Bridgehead role or already
/// Destroyed) and reset orphaned stub cells whose anchor span has gone
/// away. A reset cell becomes:
///   - `overlay_byte = 0xFF` (sentinel: no overlay / -1)
///   - `damage_state = Healthy { variant: 0 }`
///   - `bridge_group_id = None`
///   - `deck_present = false`
///
/// Walk-length cap = 30 cells per RE doc §7.2 to bound the worst-case
/// linear-bridge length.
fn update_adjacent_bridges(sim: &mut Simulation, rim_cells: &BTreeSet<(u16, u16)>) {
    let Some(bridge_state) = sim.bridge_state.as_mut() else {
        return;
    };

    const WALK_LIMIT: usize = 30;
    const DIRECTIONS: [(i32, i32); 8] = [
        (0, -1),
        (1, -1),
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
    ];

    for &(rx, ry) in rim_cells {
        // Phase A: find adjacent bridge-head candidate among 8 neighbors.
        let mut head_dir: Option<(i32, i32)> = None;
        for &(dx, dy) in &DIRECTIONS {
            let nx = rx as i32 + dx;
            let ny = ry as i32 + dy;
            if nx < 0 || ny < 0 {
                continue;
            }
            let Some(neigh) = bridge_state.cell(nx as u16, ny as u16) else {
                continue;
            };
            let is_head_candidate = matches!(neigh.role, BridgeCellRole::Bridgehead)
                || matches!(neigh.damage_state, DamageState::Destroyed);
            if is_head_candidate {
                head_dir = Some((dx, dy));
                break;
            }
        }
        let Some((dx, dy)) = head_dir else { continue };

        // Phase B: walk along the bridge from (rx, ry) toward the head and
        // reset dangling stubs whose anchor span no longer exists.
        let mut walk_x = rx as i32;
        let mut walk_y = ry as i32;
        for _ in 0..WALK_LIMIT {
            walk_x += dx;
            walk_y += dy;
            if walk_x < 0 || walk_y < 0 {
                break;
            }
            let Some(cell) = bridge_state.cell(walk_x as u16, walk_y as u16) else {
                break;
            };
            if !cell.deck_present {
                break;
            }
            // Just-walker-destroyed cells render their own destroyed-bridge
            // overlay tile (0xE8 etc) — skip them so we don't blank that sprite.
            if matches!(cell.damage_state, DamageState::Destroyed) {
                continue;
            }
            let stub_now = cell
                .anchor_span_id
                .map(|sid| !bridge_state.anchor_spans().contains_key(&sid))
                .unwrap_or(false);
            if !stub_now {
                continue;
            }
            if bridge_state.cell(walk_x as u16, walk_y as u16).is_some() {
                let _ = bridge_state.write_overlay_byte(walk_x as u16, walk_y as u16, 0xFF);
                let c = bridge_state
                    .cell_mut(walk_x as u16, walk_y as u16)
                    .expect("bridge stub existed before overlay write");
                c.damage_state = DamageState::Healthy { variant: 0 };
                c.bridge_group_id = None;
                c.deck_present = false;
            }
        }
    }
}

/// TriggerEvent 31 broadcast. Mirror of binary
/// `MapClass::RepairBridgeSegment @ 0x00575EE0` (binary name is
/// misleading — the function actually fires `TriggerEvent 31` on bridge
/// span collapse; HIGH §11.3 + §12.6).
///
/// No-op on skirmish maps — RA2 skirmish has no triggers bound to
/// event 31. Wired as a hook so future campaign and map-trigger
/// support can drop in without changing the orchestrator's cascade
/// order.
fn notify_bridge_span_collapse(sim: &mut Simulation, cells: &BTreeSet<(u16, u16)>) {
    let _ = (sim, cells);
}

/// Select the ground-level bridge surface from map/deck facts, never from the
/// numeric overlay band: urban low bridges share bytes with elevated bridges.
fn is_low_surface_bridge_cell(
    bridge_cell: &BridgeRuntimeCell,
    terrain_cell: &crate::map::resolved_terrain::ResolvedTerrainCell,
) -> bool {
    if let Some(layer) = terrain_cell.bridge_layer.as_ref() {
        return layer.direction == BridgeDirection::Low;
    }
    terrain_cell.bridge_facts.family == crate::map::bridge_facts::BridgeStampFamily::None
        && terrain_cell.bridge_facts.overlay_id.is_some()
        && bridge_cell.deck_level == terrain_cell.level
}

fn project_low_bridge_overlay_ops(
    sim: &mut Simulation,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
    ops: Vec<BridgeOverlayProjectionOp>,
) {
    let mut terrain_changed = false;
    for op in ops {
        let (rx, ry) = match op {
            BridgeOverlayProjectionOp::Write { rx, ry, .. }
            | BridgeOverlayProjectionOp::Recalc { rx, ry } => (rx, ry),
        };
        let low_surface = sim
            .bridge_state
            .as_ref()
            .and_then(|state| state.cell(rx, ry))
            .zip(
                sim.resolved_terrain
                    .as_ref()
                    .and_then(|terrain| terrain.cell(rx, ry)),
            )
            .is_some_and(|(bridge_cell, terrain_cell)| {
                is_low_surface_bridge_cell(bridge_cell, terrain_cell)
            });
        if !low_surface {
            continue;
        }

        match op {
            BridgeOverlayProjectionOp::Write { overlay_byte, .. } => {
                if let Some(overlay_grid) = sim.overlay_grid.as_mut() {
                    let _ = overlay_grid.write_bridge_overlay_identity(rx, ry, overlay_byte);
                }
            }
            BridgeOverlayProjectionOp::Recalc { .. } => {
                let Some(registry) = overlay_registry else {
                    continue;
                };
                if let (Some(overlay_grid), Some(terrain)) =
                    (sim.overlay_grid.as_mut(), sim.resolved_terrain.as_mut())
                {
                    terrain_changed |= crate::sim::overlay_grid::recalc_overlay_passability(
                        overlay_grid,
                        terrain,
                        registry,
                        rx,
                        ry,
                    );
                }
            }
        }
    }

    if terrain_changed && let Some(terrain) = sim.resolved_terrain.as_ref() {
        sim.terrain_costs =
            crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(terrain);
    }
}

pub(crate) fn project_pending_low_bridge_overlay_writes(
    sim: &mut Simulation,
    overlay_registry: Option<&crate::map::overlay_types::OverlayTypeRegistry>,
) {
    let ops = sim
        .bridge_state
        .as_mut()
        .map(BridgeRuntimeState::take_overlay_projection_ops)
        .unwrap_or_default();
    project_low_bridge_overlay_ops(sim, overlay_registry, ops);
}

/// Reconcile serialized low-bridge authority with the fresh map-derived
/// terrain cache. This also repairs older saves whose OverlayGrid identity
/// still contains the load-time Road overlay after a terminal collapse.
pub(crate) fn reconcile_low_bridge_surface_after_cache_load(
    sim: &mut Simulation,
    overlay_registry: &crate::map::overlay_types::OverlayTypeRegistry,
) {
    let ops = sim.bridge_state.as_mut().map(|state| {
        let _ = state.take_overlay_projection_ops();
        state
            .iter_cells()
            .flat_map(|((rx, ry), cell)| {
                [
                    BridgeOverlayProjectionOp::Write {
                        rx,
                        ry,
                        overlay_byte: cell.overlay_byte,
                    },
                    BridgeOverlayProjectionOp::Recalc { rx, ry },
                ]
            })
            .collect()
    });
    project_low_bridge_overlay_ops(sim, Some(overlay_registry), ops.unwrap_or_default());
}

/// Zone graph refresh. Per HIGH §12.8: walker emits `zones_dirty=true`
/// only when a final-stage cell flips a `BridgeEndpointRecord.active`
/// flag, mirroring the binary's `MapClass::InvalidateBridgeZones`
/// @ `0x0056DAE0` → `MapClass::RebuildZoneConnectivity` @ `0x0056C510`
/// chain. When set:
///   1. Recompute every endpoint record's `active` flag from current
///      cell damage state — first destroyed cell in a group flips its
///      endpoint pair to `active = false`. Replaces the side-effect of
///      the legacy single-shot `apply_damage`.
///   2. Ask the world navigation owner to publish current terrain costs,
///      structure blockers, bridge passability and zone connectivity together.
pub(crate) fn refresh_bridge_zones_if_dirty(
    sim: &mut Simulation,
    rules: &RuleSet,
    any_zones_dirty: bool,
) {
    if !any_zones_dirty {
        return;
    }
    if let Some(bs) = sim.bridge_state.as_mut() {
        bs.refresh_endpoint_active_flags();
    }
    // VERA-internal projection ownership, gamemd equivalent UNCHECKED.
    // This publishes the canonical path as well as zones. Use the shared
    // structure/terrain projection so unrelated foundations survive bridge
    // changes before the next reader; endpoint refresh must stay first.
    // Native zone-tail ordering: BRIDGE_COLLAPSE_FALLOUT_ORDERING_GHIDRA_REPORT.md
    // in docs/research/bridges/05-damage-collapse-repair-cabhut/, section
    // "Zone/path invalidation" (UpdateBridgeZonesHelper @ 0x0056C510).
    let _ = sim.rebuild_dynamic_navigation(rules);
}

/// Per-cell debris spawn. Mirror of binary `BlowUpBridge` step 4. RNG draw
/// order is parity-critical for lockstep; the binary draws in this exact
/// sequence per cell that passes the outer gate:
/// 1. Outer normalized 95% gate.
/// 2. Two normalized jitter draws, converted to in-cell offsets.
/// 3. MetallicDebris normalized 50% gate.
/// 4. Optional MetallicDebris slot when the gate passed and debris exists.
/// 5. BridgeExplosion delay in 1..=5 frames.
/// 6. BridgeExplosion slot.
///
/// Replaces the wrong-shape legacy `Simulation::spawn_bridge_explosions`,
/// which drew 1 immediate BridgeExplosion + a 50% delayed BridgeExplosion
/// — visible every collapse.
fn spawn_bridge_debris(sim: &mut Simulation, rules: &RuleSet, cells: &BTreeSet<(u16, u16)>) {
    let explosion_count = sim.bridge_explosions.len() as u32;
    let metallic_count = sim.metallic_debris.len() as u32;

    if explosion_count == 0 {
        return;
    }

    for &(rx, ry) in cells {
        // Step 1: outer 95% gate.
        let outer_draw = sim
            .bridge_rng()
            .next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
        if outer_draw >= BRIDGE_DEBRIS_OUTER_GATE_EXCLUSIVE {
            continue;
        }

        // Step 2: two normalized jitter draws become the in-cell offsets.
        let (sub_x, sub_y) = bridge_jittered_subcells(sim.bridge_rng());

        let deck_level = sim
            .resolved_terrain
            .as_ref()
            .and_then(|t| t.cell(rx, ry))
            .map(|c| c.bridge_deck_level_if_any().unwrap_or(c.level))
            .unwrap_or(0);

        // Step 3: MetallicDebris 50% gate.
        let metallic_draw = sim
            .bridge_rng()
            .next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
        let metallic_pass = metallic_draw < BRIDGE_METALLIC_GATE_EXCLUSIVE;
        // Step 4: MetallicDebris slot pick + spawn (no delay). Slot draw
        // only happens when all three gates pass — short-circuit matches
        // the binary's call order.
        if metallic_pass && metallic_count > 0 {
            // The slot draw is kept for stream parity; the debris itself is
            // not constructed (see the RESIDUAL on the debris block above).
            let _slot = sim.bridge_rng().next_range_u32(metallic_count);
        }

        // Step 5 + 6: always BridgeExplosion, delayed 1-5 frames.
        let delay_frames = sim.bridge_rng().next_range_u32_inclusive(1, 5);
        let idx = sim.bridge_rng().next_range_u32(explosion_count) as usize;
        let descriptor = bridge_explosion_descriptor(
            sim.bridge_explosions[idx],
            (rx, ry),
            (sub_x, sub_y),
            deck_level,
            delay_frames as u16,
        );
        construct_bridge_explosion(sim, rules, descriptor);
    }
}

/// `AnimClass` draw flags every bridge collapse animation is constructed with.
const BRIDGE_ANIM_DRAW_FLAGS: u32 = 0x600;

/// The `AnimClass::Constructor @ 0x00421EA0` row of a `BridgeExplosions=`
/// animation: `(type, &coord, delay 1..=5, loop 1, flags 0x600, zAdjust 0,
/// reverse 0)`, identical at `CellClass::BlowUpBridge` `0x0047E02C` and in the
/// four hut walkers. The start delay keeps the constructor from calling
/// `AnimClass::Start @ 0x00424CE0`; the store calls it, and plays the type's
/// `Report=`, on the visit that counts the delay to zero.
///
/// RESIDUAL: all four stock types author `Scorch=yes` and `Crater=yes`.
/// `AnimClass::Middle @ 0x00424F00` places them, behind a height gate
/// (`vtable+0x1C8 < 0x1E`) and, with both keys set, one
/// `RandomRanged(0, 0x7FFFFFFE)` for the pick. `Start` calls `Middle` when
/// `AnimType+0x298` is zero, otherwise `AnimClass::AI` does at that frame; the
/// value for these types is UNCHECKED. The store does not run `Middle` (see
/// `sim::anim_class`) and these producers queue no smudge rows, so the hut
/// walker explosions, which sit at cell level and should pass the gate, leave
/// no scorch or crater and skip that draw; the `BlowUpBridge` ones sit a deck
/// offset up and should fail it (offset value UNCHECKED). Downstream risk: one
/// scenario draw per walker explosion. The legacy list took neither.
///
/// UNCHECKED: the store draws an anim during its start delay, so frame 0 shows
/// for the delay plus the first-AI visit before the sound;
/// `AnimClass::DrawIt @ 0x00422CA0` shows no delay test, which suggests native
/// does the same. The
/// `BlowUpBridge` Z uses the deck level only while the cell still reports a
/// deck, where native adds the offset unconditionally.
fn bridge_explosion_descriptor(
    type_name: InternedId,
    cell: (u16, u16),
    sub: (SimFixed, SimFixed),
    level: u8,
    delay: u16,
) -> crate::sim::components::AnimClassSpawnDescriptor {
    crate::sim::components::AnimClassSpawnDescriptor {
        delay,
        loop_count: 1,
        draw_flags: BRIDGE_ANIM_DRAW_FLAGS,
        z_adjust: 0,
        reverse: false,
        ..crate::sim::components::AnimClassSpawnDescriptor::new(
            type_name, cell.0, cell.1, sub.0, sub.1, level,
        )
    }
}

fn construct_bridge_explosion(
    sim: &mut Simulation,
    rules: &RuleSet,
    descriptor: crate::sim::components::AnimClassSpawnDescriptor,
) {
    if let Err(error) = sim.spawn_anim_object(rules, descriptor) {
        // `anim_class_roots` binds every `BridgeExplosions=` type, so this is
        // a type with no art section or no SHP.
        log::warn!("bridge explosion anim did not construct: {error}");
    }
}

/// One walker explosion: two jitter draws, the `RandomRanged(1, 5)` start
/// delay, then the slot, in that order (`0x00575540`, `0x00575BA0`).
///
/// RESIDUAL: the row is constructed when the walk's borrows end, not between
/// these draws and the next cell's. No stock `BridgeExplosions=` type authors
/// `RandomRate=`, so the constructor draws nothing and the scenario stream is
/// unchanged; a modded one would take its rate draw after the walk's own draws.
/// Stable ids follow the same order. Separately, and older than this producer,
/// VERA runs every walker step's draws before the `BlowUpBridge` fallout draws,
/// where native calls `DestroyBridge_*` inside each step; whether that body
/// draws inline is UNCHECKED.
fn queue_walker_bridge_explosion(
    presentation: &mut BridgePresentationContext<'_>,
    rx: u16,
    ry: u16,
    z: u8,
) {
    if presentation.bridge_explosions.is_empty() {
        return;
    }
    let sub = bridge_jittered_subcells(presentation.rng);
    let delay_frames = presentation.rng.next_range_u32_inclusive(1, 5);
    let idx = presentation
        .rng
        .next_range_u32(presentation.bridge_explosions.len() as u32) as usize;
    presentation.anim_spawns.push(bridge_explosion_descriptor(
        presentation.bridge_explosions[idx],
        (rx, ry),
        sub,
        z,
        delay_frames as u16,
    ));
}

fn bridge_jittered_subcells(rng: &mut SimRng) -> (SimFixed, SimFixed) {
    let x_draw = rng.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
    let y_draw = rng.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
    (
        bridge_jittered_subcell(x_draw),
        bridge_jittered_subcell(y_draw),
    )
}

fn bridge_jittered_subcell(draw: u32) -> SimFixed {
    let offset = ((u64::from(draw) * BRIDGE_JITTER_SPAN_LEPTONS) / NORMALIZED_RNG_DENOMINATOR)
        as i32
        - BRIDGE_JITTER_HALF_LEPTONS;
    CELL_CENTER_LEPTON + SimFixed::from_num(offset)
}

/// BlowUpBridge's deck pass (0x0047DDBA..0x0047DDC9): read the selected
/// head after ground callbacks, capture next BEFORE DropIn removes current.
/// Membership owns selection, including retained corpses and non-anchor cells.
fn drop_in_bridge_deck_entities(sim: &mut Simulation, rx: u16, ry: u16) {
    use crate::sim::movement::locomotor::MovementLayer;
    let mut next = sim
        .substrate
        .occupancy
        .get(rx, ry)
        .and_then(|cell| cell.first_on_layer(MovementLayer::Bridge));
    while let Some(id) = next {
        next = sim
            .substrate
            .occupancy
            .get(rx, ry)
            .and_then(|cell| cell.next_on_layer(MovementLayer::Bridge, id));
        sim.drop_in_bridge_member(id);
    }
}

/// Inner dispatch loop. Owns the split borrow of `Simulation` so the
/// dispatcher can read terrain immutably while mutating bridge_state +
/// rng. Returns a `StateOutcome` per event whose path matched and whose
/// driver did real work.
fn run_dispatch_loop(
    sim: &mut Simulation,
    events: &[BridgeDamageEvent],
    bridge_strength: i32,
    publication: Option<(&RuleSet, Option<&crate::map::overlay_types::OverlayTypeRegistry>)>,
) -> (Vec<StateOutcome>, bool) {
    damage_dispatch::run(sim, events, bridge_strength, publication)
}

fn apply_runtime_bridge_flag_transcript_from_outcome(sim: &mut Simulation, outcome: &StateOutcome) {
    for &stamp in outcome.setter_transcript() {
        sim.apply_planned_bridge_flag_stamp_to_real_cells(stamp);
    }
    // The legacy caller has already executed56E990 synchronously on the live
    // grid. Retain plain pavement cells too, before fallout/another dispatch.
    if let Some(terrain) = sim.resolved_terrain.as_ref() {
        for &(rx, ry) in outcome.damaged_variant_cells() {
            if let Some(cell) = terrain.cell(rx, ry) {
                sim.dynamic_terrain_cells.insert(
                    (rx, ry),
                    crate::map::resolved_terrain::DynamicTerrainCellState::capture(cell),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};
    use crate::sim::components::{BridgeOccupancy, Health};
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::test_intern;
    use crate::sim::movement::locomotion::LocomotorSlot;
    use crate::sim::movement::locomotor::{GroundMovePhase, LocomotorState, MovementLayer};
    use crate::sim::occupancy::CellListInsertion;
    use crate::util::fixed_math::{SIM_ZERO, SimFixed};

    pub(super) fn seed_bridge_cell(
        overlay_byte: u8,
    ) -> crate::sim::bridge_state::BridgeRuntimeCell {
        crate::sim::bridge_state::BridgeRuntimeCell {
            deck_present: true,
            destroyable: true,
            deck_level: 4,
            bridge_group_id: Some(1),
            damage_state: DamageState::Healthy { variant: 0 },
            axis: None,
            role: BridgeCellRole::Body,
            anchor_span_id: None,
            overlay_byte,
            bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
        }
    }

    /// Build a single-cell terrain grid where (5,5) is a bridge deck at
    /// `deck_level`, ground level=0, water below (`is_water=true`,
    /// `ground_walk_blocked=true`). Used to verify DropIn lets deck units
    /// survive even with no walkable ground.
    pub(super) fn water_below_bridge_terrain(deck_level: u8) -> ResolvedTerrainGrid {
        let mut cells = Vec::new();
        for y in 0..=5u16 {
            for x in 0..=5u16 {
                let is_bridge = x == 5 && y == 5;
                cells.push(ResolvedTerrainCell {
                    rx: x,
                    ry: y,
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
                    is_water: is_bridge,
                    is_cliff_like: false,
                    height_in_pixels: 0,
                    variant: 0,
                    is_rough: false,
                    is_road: false,
                    accepts_smudge: false,
                    allows_tiberium: false,
                    has_ramp: false,
                    canonical_ramp: None,
                    ground_walk_blocked: is_bridge,
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
                    build_blocked: is_bridge,
                    has_bridge_deck: is_bridge,
                    bridge_walkable: is_bridge,
                    bridge_transition: is_bridge,
                    bridge_deck_level: if is_bridge { deck_level } else { 0 },
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
        ResolvedTerrainGrid::from_cells(6, 6, cells)
    }

    fn gsi_04_13_stock_low_registry() -> crate::map::overlay_types::OverlayTypeRegistry {
        use std::fmt::Write as _;

        let mut ini = String::from("[OverlayTypes]\n");
        for id in 0..=101u8 {
            let name = if (74..=101).contains(&id) {
                format!("LOBRDG{:02}", id - 73)
            } else {
                format!("FILL{id:03}")
            };
            let _ = writeln!(ini, "{id}={name}");
        }
        ini.push_str(
            "[Road]\nFoot=100%\nTrack=100%\nWheel=100%\n\
             [Rough]\nFoot=100%\nTrack=100%\nWheel=100%\n",
        );
        for variant in 1..=26u8 {
            let _ = writeln!(
                ini,
                "[LOBRDG{variant:02}]\nLand=Road\nNoUseTileLandType=yes"
            );
        }
        for variant in 27..=28u8 {
            let _ = writeln!(ini, "[LOBRDG{variant:02}]\nLand=Road\nNoUseTileLandType=no");
        }
        crate::map::overlay_types::OverlayTypeRegistry::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(&ini),
            None,
        )
    }

    fn gsi_04_13_low_cell(
        ry: u16,
        road_speed: SpeedCostProfile,
        rough_speed: SpeedCostProfile,
    ) -> ResolvedTerrainCell {
        use crate::rules::terrain_rules::LandType;

        let mut bridge_facts = crate::map::bridge_facts::BridgeCellFacts::default();
        bridge_facts.overlay_id = Some(0x4A);
        ResolvedTerrainCell {
            rx: 0,
            ry,
            source_tile_index: 0,
            source_sub_tile: 0,
            final_tile_index: 0,
            final_sub_tile: 0,
            is_wood_bridge_repair_tile: true,
            level: 0,
            filled_clear: false,
            tileset_index: Some(0),
            land_type: LandType::Road.as_index(),
            yr_cell_land_type: LandType::Road.as_index(),
            slope_type: 0,
            template_height: 0,
            height_in_pixels: 0,
            render_offset_x: 0,
            render_offset_y: 0,
            terrain_class: TerrainClass::Road,
            speed_costs: road_speed,
            is_water: false,
            is_cliff_like: false,
            is_rough: false,
            is_road: true,
            accepts_smudge: false,
            allows_tiberium: false,
            variant: 0,
            has_ramp: false,
            canonical_ramp: None,
            ground_walk_blocked: false,
            terrain_object_blocks: false,
            terrain_object_occupation: None,
            overlay_blocks: false,
            overlay_zone_type: None,
            outside_playfield: false,
            zone_type: crate::map::resolved_terrain::zone_class::GROUND,
            base_ground_walk_blocked: false,
            base_build_blocked: false,
            base_land_type: LandType::Rough.as_index(),
            base_yr_cell_land_type: LandType::Rough.as_index(),
            base_terrain_class: TerrainClass::Rough,
            base_speed_costs: rough_speed,
            build_blocked: true,
            has_bridge_deck: true,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: Some(crate::map::resolved_terrain::BridgeLayer {
                overlay_id: 0x4A,
                overlay_name: "LOBRDG01".to_owned(),
                deck_level: 0,
                direction: BridgeDirection::Low,
            }),
            bridge_facts,
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    fn gsi_04_13_assert_ground_surface(sim: &Simulation, expected_land: u8) {
        use crate::sim::movement::locomotor::MovementLayer;

        let terrain = sim.resolved_terrain.as_ref().expect("terrain");
        let path = crate::sim::pathfinding::PathGrid::from_resolved_terrain_with_bridges(
            terrain,
            sim.bridge_state.as_ref(),
        );
        for ry in 0..3 {
            let cell = terrain.cell(0, ry).expect("low bridge cell");
            assert_eq!(cell.land_type, expected_land, "land at y={ry}");
            assert_eq!(
                cell.yr_cell_land_type, expected_land,
                "CellClass land at y={ry}"
            );
            if expected_land == crate::rules::terrain_rules::LandType::Road.as_index() {
                assert_eq!(cell.terrain_class, TerrainClass::Road);
                assert!(cell.is_road);
                assert!(!cell.is_rough);
            } else {
                assert_eq!(cell.terrain_class, TerrainClass::Rough);
                assert!(cell.is_rough);
                assert!(!cell.is_road);
                assert_eq!(cell.speed_costs, cell.base_speed_costs);
            }
            assert!(!cell.is_elevated_bridge_cell(), "low bridge at y={ry}");
            assert!(path.is_walkable_on_layer(0, ry, MovementLayer::Ground));
            assert!(!path.is_walkable_on_layer(0, ry, MovementLayer::Bridge));
        }
        assert!(terrain.tube_facts().is_empty());
        assert!(
            sim.bridge_state
                .as_ref()
                .expect("bridge state")
                .endpoint_records()
                .is_empty(),
            "stock low surface has no synthetic BridgeZone record"
        );
    }

    #[test]
    fn gsi_04_13_urban_low_uses_deck_facts_not_high_numeric_overlay_band() {
        let mut runtime = seed_bridge_cell(0xCD);
        runtime.deck_level = 0;
        let mut terrain =
            gsi_04_13_low_cell(0, SpeedCostProfile::default(), SpeedCostProfile::default());
        terrain.bridge_facts.overlay_id = Some(0xCD);
        terrain
            .bridge_layer
            .as_mut()
            .expect("bridge layer")
            .overlay_id = 0xCD;

        assert!(is_low_surface_bridge_cell(&runtime, &terrain));
        terrain
            .bridge_layer
            .as_mut()
            .expect("bridge layer")
            .direction = BridgeDirection::EastWest;
        assert!(!is_low_surface_bridge_cell(&runtime, &terrain));
    }

    #[test]
    fn gsi_04_13_stock_low_strip_recalc_collapse_repair_and_cache_reconcile() {
        use crate::rules::locomotor_type::SpeedType;
        use crate::rules::terrain_rules::LandType;

        let registry = gsi_04_13_stock_low_registry();
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("")).unwrap();
        let road_speed = registry
            .flags(0x4A)
            .and_then(|flags| flags.land_speed_costs)
            .expect("LOBRDG01 Road profile");
        let rough_speed = SpeedCostProfile {
            foot: Some(100),
            track: Some(100),
            wheel: Some(100),
            ..SpeedCostProfile::default()
        };
        let terrain = ResolvedTerrainGrid::from_cells(
            1,
            3,
            (0..3)
                .map(|ry| gsi_04_13_low_cell(ry, road_speed, rough_speed))
                .collect(),
        );
        let bridge_state = BridgeRuntimeState::from_resolved_terrain(&terrain, true, 1);
        let preserved_owner = test_intern("GSI0413OWNER");
        let mut overlay_grid = crate::sim::overlay_grid::OverlayGrid::new(1, 3);
        for ry in 0..3 {
            overlay_grid.place_overlay(0, ry, 0x4A, 0xA0 + ry as u8);
            overlay_grid.cell_mut(0, ry).wall_owner = Some(preserved_owner);
        }

        let mut sim = Simulation::new();
        sim.terrain_costs =
            crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(&terrain);
        sim.resolved_terrain = Some(terrain);
        sim.bridge_state = Some(bridge_state);
        sim.overlay_grid = Some(overlay_grid);
        gsi_04_13_assert_ground_surface(&sim, LandType::Road.as_index());

        let first = {
            let terrain = sim.resolved_terrain.as_ref().expect("terrain");
            sim.bridge_state
                .as_mut()
                .expect("bridge state")
                .destroy_bridge_low(0, 1, terrain)
        };
        assert!(matches!(first, StateOutcome::Absorbed { .. }));
        project_pending_low_bridge_overlay_writes(&mut sim, Some(&registry));
        gsi_04_13_assert_ground_surface(&sim, LandType::Road.as_index());
        for ry in 0..3 {
            let overlay = sim.overlay_grid.as_ref().expect("overlay grid").cell(0, ry);
            assert_eq!(overlay.overlay_id, Some(0x50));
            assert_eq!(overlay.overlay_data, 0xA0 + ry as u8);
            assert_eq!(overlay.wall_owner, Some(preserved_owner));
        }

        let second = {
            let terrain = sim.resolved_terrain.as_ref().expect("terrain");
            sim.bridge_state
                .as_mut()
                .expect("bridge state")
                .destroy_bridge_low(0, 1, terrain)
        };
        let StateOutcome::Collapsed { zones_dirty, .. } = second else {
            panic!("second stock low hit must reach terminal collapse");
        };
        assert!(zones_dirty);
        project_pending_low_bridge_overlay_writes(&mut sim, Some(&registry));
        refresh_bridge_zones_if_dirty(&mut sim, &rules, zones_dirty);
        gsi_04_13_assert_ground_surface(&sim, LandType::Rough.as_index());
        assert_eq!(
            sim.terrain_costs
                .get(&SpeedType::Track)
                .expect("Track costs")
                .cost_at(0, 1),
            100,
            "terminal low bridge uses the restored TMP Rough speed"
        );
        for ry in 0..3 {
            let overlay = sim.overlay_grid.as_ref().expect("overlay grid").cell(0, ry);
            assert_eq!(overlay.overlay_id, Some(0x64));
            assert_eq!(overlay.overlay_data, 0xA0 + ry as u8);
            assert_eq!(overlay.wall_owner, Some(preserved_owner));
        }

        // A loaded save receives the pristine map cache again. Reconcile the
        // serialized bridge owner before movement can observe stale Road.
        for ry in 0..3 {
            let cell = sim
                .resolved_terrain
                .as_mut()
                .expect("terrain")
                .cell_mut(0, ry)
                .expect("low bridge cell");
            cell.land_type = LandType::Road.as_index();
            cell.yr_cell_land_type = LandType::Road.as_index();
            cell.terrain_class = TerrainClass::Road;
            cell.speed_costs = road_speed;
            cell.is_rough = false;
            cell.is_road = true;
            let _ = sim
                .overlay_grid
                .as_mut()
                .expect("overlay grid")
                .write_bridge_overlay_identity(0, ry, 0x4A);
        }
        let stale = sim.resolved_terrain.as_ref().expect("terrain");
        sim.terrain_costs =
            crate::sim::pathfinding::terrain_cost::build_canonical_terrain_cost_grids(stale);
        reconcile_low_bridge_surface_after_cache_load(&mut sim, &registry);
        gsi_04_13_assert_ground_surface(&sim, LandType::Rough.as_index());

        let repair = {
            let terrain = sim.resolved_terrain.as_ref().expect("terrain");
            let bridge_state = sim.bridge_state.as_mut().expect("bridge state");
            bridge_state.repair_bridge_from_engineer_scan(&[(0, 1)], &mut sim.mapgen_rng, terrain)
        };
        assert!(repair.zones_dirty);
        project_pending_low_bridge_overlay_writes(&mut sim, Some(&registry));
        refresh_bridge_zones_if_dirty(&mut sim, &rules, repair.zones_dirty);
        gsi_04_13_assert_ground_surface(&sim, LandType::Road.as_index());
        for ry in 0..3 {
            let overlay = sim.overlay_grid.as_ref().expect("overlay grid").cell(0, ry);
            assert!(matches!(overlay.overlay_id, Some(0x4A..=0x4D)));
            assert_eq!(overlay.overlay_data, 0xA0 + ry as u8);
            assert_eq!(overlay.wall_owner, Some(preserved_owner));
        }
    }

    /// Build a Drive locomotor on the Bridge layer (mimics `high=true` spawn).
    fn drive_loco_on_bridge() -> LocomotorState {
        LocomotorState {
            kind: LocomotorKind::Drive,
            slot: LocomotorSlot::from_kind(LocomotorKind::Drive),
            powered: true,
            piggyback: None,
            runtime_payload: crate::sim::movement::locomotion::LocomotorRuntimePayload::for_kind(
                LocomotorKind::Drive,
                0,
            ),
            layer: MovementLayer::Bridge,
            phase: GroundMovePhase::Cruising,

            speed_multiplier: SimFixed::from_num(1),
            speed_fraction: SimFixed::from_num(1),
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
        }
    }

    /// Insert a vehicle on the bridge deck at (5,5) with deck_level=3.
    fn spawn_deck_unit(sim: &mut Simulation) -> u64 {
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            5,
            5,
            3,
            64,
            test_intern("Americans"),
            Health { current: 256 },
            test_intern("MTNK"),
            crate::map::entities::EntityCategory::Unit,
            0,
            5,
            true,
        );
        entity.on_bridge = true;
        entity.bridge_occupancy = Some(BridgeOccupancy { deck_level: 3 });
        entity.locomotor = Some(drive_loco_on_bridge());
        // Give it a short fake movement target so we can verify it gets
        // halted on collapse.
        entity.movement_target = Some(crate::sim::components::MovementTarget::default());
        sim.substrate.entities.insert(entity);
        1
    }

    /// Task 11 — DropIn correction: bridge-deck entities snap to ground
    /// level + survive even when the destination is unwalkable (water
    /// below). The legacy `resolve_bridge_state_changes` despawned in
    /// this case; vanilla never does (HIGH §12.7 / §12.9).
    #[test]
    fn drop_in_snaps_deck_entity_to_ground_over_water_no_despawn() {
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(water_below_bridge_terrain(3));
        let id = spawn_deck_unit(&mut sim);
        sim.substrate.occupancy.add(
            5,
            5,
            id,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        drop_in_bridge_deck_entities(&mut sim, 5, 5);

        let e = sim
            .substrate
            .entities
            .get(id)
            .expect("deck entity must SURVIVE collapse over water");
        assert_eq!(e.position.z, 0, "snapped to ground level");
        assert!(!e.on_bridge, "OnBridge cleared by DropIn");
        assert!(e.bridge_occupancy.is_none(), "bridge_occupancy cleared");
        assert!(e.movement_target.is_none(), "movement halted on collapse");
        assert_eq!(e.health.current, 256, "DropIn never harms — no damage");
        let loco = e.locomotor.as_ref().expect("locomotor");
        assert_eq!(
            loco.layer,
            MovementLayer::Ground,
            "layer flipped Bridge → Ground"
        );
        assert_eq!(loco.phase, GroundMovePhase::Idle, "phase reset to Idle");
        let cell = sim
            .substrate
            .occupancy
            .get(5, 5)
            .expect("occupancy retained");
        assert_eq!(cell.count_on(MovementLayer::Ground), 1);
        assert_eq!(cell.count_on(MovementLayer::Bridge), 0);
    }

    /// Build a minimal RuleSet whose bridge voxel maximum matches the argument.
    /// Used by debris tests to toggle the voxel-max gate.
    fn rules_with_voxel_max(voxel_max: u32) -> crate::rules::ruleset::RuleSet {
        let body = format!(
            "[InfantryTypes]\n\
             [VehicleTypes]\n\
             [AircraftTypes]\n\
             [BuildingTypes]\n\
             [General]\n\
             BridgeVoxelMax={}\n",
            voxel_max
        );
        let ini = crate::rules::ini_parser::IniFile::from_str(&body);
        let mut rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("rules parse");
        // Stock-shaped `BridgeExplosions=` art: an unbound type constructs no
        // AnimClass, natively or here.
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(
                "[BRIDGEEXP1]\nTranslucent=yes\nReport=Explosion06\n\
                 [BRIDGEEXP2]\nTranslucent=yes\nReport=Explosion07\n",
            ),
        );
        art.bind_anim_frame_count_for_test("BRIDGEEXP1", 8);
        art.bind_anim_frame_count_for_test("BRIDGEEXP2", 8);
        rules.art_registry = art;
        rules
    }

    fn bridge_explosion_anims(sim: &Simulation) -> Vec<&crate::sim::anim_class::AnimObject> {
        sim.substrate
            .anims
            .iter()
            .map(|(_, anim)| anim)
            .filter(|anim| sim.bridge_explosions.contains(&anim.type_id))
            .collect()
    }

    #[test]
    fn hut_destroy_overlay_seed_uses_physical_span_axis_not_walker_family() {
        use crate::sim::bridge_state::BridgeRuntimeState;

        let mut high_ew_range = BridgeRuntimeState::default();
        high_ew_range.test_seed_cell(5, 4, seed_bridge_cell(0xCD));
        high_ew_range.test_seed_cell(5, 5, seed_bridge_cell(0xCD));
        assert_eq!(
            find_destroy_overlay_seed(&high_ew_range, &[(5, 5)], HutBridgeFamily::High),
            Some((5, 5, Axis::EW)),
            "0xCD high range dispatches to CollapseBridge_EW_High, so the hut sweep must step X"
        );

        let mut high_ns_range = BridgeRuntimeState::default();
        high_ns_range.test_seed_cell(4, 5, seed_bridge_cell(0xD6));
        high_ns_range.test_seed_cell(5, 5, seed_bridge_cell(0xD6));
        assert_eq!(
            find_destroy_overlay_seed(&high_ns_range, &[(5, 5)], HutBridgeFamily::High),
            Some((5, 5, Axis::NS)),
            "0xD6 high range dispatches to CollapseBridge_NS_High, so the hut sweep must step Y"
        );

        let mut low_ew_range = BridgeRuntimeState::default();
        low_ew_range.test_seed_cell(5, 4, seed_bridge_cell(0x4A));
        low_ew_range.test_seed_cell(5, 5, seed_bridge_cell(0x4A));
        assert_eq!(
            find_destroy_overlay_seed(&low_ew_range, &[(5, 5)], HutBridgeFamily::Low),
            Some((5, 5, Axis::EW)),
            "0x4A low range dispatches to CollapseBridge_EW_Low, so the hut sweep must step X"
        );

        let mut low_ns_range = BridgeRuntimeState::default();
        low_ns_range.test_seed_cell(4, 5, seed_bridge_cell(0x53));
        low_ns_range.test_seed_cell(5, 5, seed_bridge_cell(0x53));
        assert_eq!(
            find_destroy_overlay_seed(&low_ns_range, &[(5, 5)], HutBridgeFamily::Low),
            Some((5, 5, Axis::NS)),
            "0x53 low range dispatches to CollapseBridge_NS_Low, so the hut sweep must step Y"
        );
    }

    #[test]
    fn cabhut_seed_canonicalization_shifts_edge_hit_forward() {
        use crate::sim::bridge_state::BridgeRuntimeState;

        let mut state = BridgeRuntimeState::default();
        state.test_seed_cell(5, 5, seed_bridge_cell(0xCD));

        assert_eq!(
            find_destroy_overlay_seed(&state, &[(5, 5)], HutBridgeFamily::High),
            Some((5, 6, Axis::EW)),
            "when no back lane is in the bridge band, DestroyBridgeFromCell shifts one cell forward"
        );
    }

    #[test]
    fn cabhut_seed_canonicalization_keeps_middle_hit() {
        use crate::sim::bridge_state::BridgeRuntimeState;

        let mut state = BridgeRuntimeState::default();
        state.test_seed_cell(5, 4, seed_bridge_cell(0xCD));
        state.test_seed_cell(5, 5, seed_bridge_cell(0xCD));

        assert_eq!(
            find_destroy_overlay_seed(&state, &[(5, 5)], HutBridgeFamily::High),
            Some((5, 5, Axis::EW)),
            "one in-band back lane and one off-band second back lane keeps the matched cell"
        );
    }

    #[test]
    fn cabhut_seed_canonicalization_shifts_two_cells_in_backward() {
        use crate::sim::bridge_state::BridgeRuntimeState;

        let mut state = BridgeRuntimeState::default();
        state.test_seed_cell(5, 3, seed_bridge_cell(0xCD));
        state.test_seed_cell(5, 4, seed_bridge_cell(0xCD));
        state.test_seed_cell(5, 5, seed_bridge_cell(0xCD));

        assert_eq!(
            find_destroy_overlay_seed(&state, &[(5, 5)], HutBridgeFamily::High),
            Some((5, 4, Axis::EW)),
            "two in-band back probes shift the canonical seed one cell backward"
        );
    }

    #[test]
    fn hut_destroy_scan_uses_gamemd_x_major_order() {
        let cells: Vec<(u16, u16)> = hut_destroy_5x5_scan((10, 10)).collect();
        assert_eq!(cells.len(), 25);
        assert_eq!(
            &cells[..6],
            &[(8, 8), (8, 9), (8, 10), (8, 11), (8, 12), (9, 8)],
            "hut death scan must walk each X column before advancing X"
        );

        let edge_cells: Vec<(u16, u16)> = hut_destroy_5x5_scan((0, 0)).collect();
        assert_eq!(edge_cells.len(), 9);
        assert_eq!(
            &edge_cells[..3],
            &[(0, 0), (0, 1), (0, 2)],
            "off-map negative cells are skipped while preserving X-major order"
        );
    }

    #[test]
    fn hut_destroy_overlay_seed_prefers_x_major_first_match() {
        use crate::sim::bridge_state::BridgeRuntimeState;

        let scan: Vec<(u16, u16)> = hut_destroy_5x5_scan((10, 10)).collect();
        let mut state = BridgeRuntimeState::default();
        state.test_seed_cell(9, 8, seed_bridge_cell(0xCD));
        state.test_seed_cell(8, 12, seed_bridge_cell(0xCD));

        assert_eq!(
            find_destroy_overlay_seed(&state, &scan, HutBridgeFamily::High),
            Some((8, 13, Axis::EW)),
            "Y-major scan would find (9,8) first; gamemd hut death finds the earlier X column"
        );
    }

    /// Task 12 - RNG draw-order parity: per cell, `spawn_bridge_debris`
    /// MUST consume RNG draws in the exact binary order:
    /// outer-95% → jitter×2 → metallic-50% → optional metallic-slot →
    /// explosion-delay → explosion-slot. Wrong order desyncs lockstep.
    #[test]
    fn bridge_blowup_debris_uses_scenario_only() {
        let mut sim = Simulation::new();
        let seed = 0xDEAD_BEEF_u64;
        sim.reseed_scenario_and_main(seed);
        let main_before = sim.main_rng.logical_state();
        let mapgen_before = sim.mapgen_rng.logical_state();
        sim.resolved_terrain = Some(water_below_bridge_terrain(3));
        let bridge_exp_1 = sim.interner.intern("BRIDGEEXP1");
        let bridge_exp_2 = sim.interner.intern("BRIDGEEXP2");
        let metallic_debris = sim.interner.intern("METALDEB1");
        let metallic_debris_2 = sim.interner.intern("METALDEB2");
        sim.bridge_explosions.extend([bridge_exp_1, bridge_exp_2]);
        // Two entries: a slot draw over a one-entry list consumes nothing.
        sim.metallic_debris
            .extend([metallic_debris, metallic_debris_2]);
        let rules = rules_with_voxel_max(3);

        let mut cells = BTreeSet::new();
        cells.insert((5, 5));

        // Predict the exact draw sequence on a parallel RNG. The helper
        // MUST match this sequence step-for-step to maintain lockstep.
        let mut predicted = crate::sim::rng::SimRng::new(seed);
        let outer = predicted.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
        if outer < BRIDGE_DEBRIS_OUTER_GATE_EXCLUSIVE {
            let _jx = predicted.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
            let _jy = predicted.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
            let metallic_draw = predicted.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
            // Metallic slot draw is gated on the 50% predicate and the
            // presence of MetallicDebris entries. BridgeVoxelMax is not part
            // of standard BlowUpBridge debris gating.
            if metallic_draw < BRIDGE_METALLIC_GATE_EXCLUSIVE {
                let _slot = predicted.next_range_u32(2);
            }
            let _delay = predicted.next_range_u32_inclusive(1, 5);
            let _exp_slot = predicted.next_range_u32(2);
        }

        spawn_bridge_debris(&mut sim, &rules, &cells);

        assert_eq!(
            sim.scenario_rng.logical_state(),
            predicted.logical_state(),
            "RNG draw order/count diverged from binary parity sequence"
        );
        if outer < BRIDGE_DEBRIS_OUTER_GATE_EXCLUSIVE {
            // `CellClass::BlowUpBridge` `0x0047E02C`: one delayed explosion at
            // the structural deck height, which is where this differs from
            // the hut walkers.
            let anims = bridge_explosion_anims(&sim);
            assert_eq!(anims.len(), 1);
            assert_eq!(anims[0].draw_flags, 0x600);
            assert!((1..=5).contains(&anims[0].runtime.delay_remaining));
            let (rx, ry, _, _, level) = anims[0].world_coord.to_cell_sub_z();
            assert_eq!((rx, ry, level), (5, 5, 3));
            assert!(
                sim.substrate.anims.iter().all(|(_, anim)| {
                    anim.type_id != metallic_debris && anim.type_id != metallic_debris_2
                }),
                "MetallicDebris is drawn for but not constructed (RESIDUAL M11b)"
            );
        }
        assert_eq!(
            sim.main_rng.logical_state(),
            main_before,
            "bridge destruction presentation must not consume Main"
        );
        assert_eq!(
            sim.mapgen_rng.logical_state(),
            mapgen_before,
            "bridge destruction presentation must not consume MapGen"
        );
    }

    /// The hut-collapse walk creates its pre-destroy presentation effects from
    /// Scenario only. A non-terminal one-step fixture makes the receiver real
    /// without adding BlowUpBridge fallout draws after the walker effects.
    #[test]
    fn hut_collapse_walker_presentation_uses_scenario_only() {
        let mut sim = Simulation::new();
        let seed = 0x48A7_51DE_u64;
        sim.reseed_scenario_and_main(seed);
        sim.resolved_terrain = Some(water_below_bridge_terrain(3));
        let bridge_explosion = sim.interner.intern("BRIDGEEXP1");
        sim.bridge_explosions.push(bridge_explosion);

        // The X-major hut scan first sees (4,3), then canonicalizes to (4,4).
        // 0xE0 is a non-terminal presentation overlay whose destruction
        // transition is NoChange, while the missing next EW cell bounds the
        // physical walker to exactly one step with no fallout outcome.
        let mut bridge_state = BridgeRuntimeState::default();
        bridge_state.test_seed_cell(4, 3, seed_bridge_cell(0xE0));
        bridge_state.test_seed_cell(4, 4, seed_bridge_cell(0xE0));
        sim.bridge_state = Some(bridge_state);

        let before = sim.rng_state();
        let mut predicted = crate::sim::rng::SimRng::new(seed);
        for _ in 0..3 {
            predicted.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
            predicted.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
            predicted.next_range_u32_inclusive(1, 5);
            predicted.next_range_u32(1);
        }

        let rules = rules_with_voxel_max(3);
        let collapsed = dispatch_bridge_collapse_from_hut(&mut sim, &rules, (4, 4));

        assert!(
            !collapsed,
            "intermediate-only fixture must not add BlowUpBridge fallout draws"
        );
        assert_eq!(
            sim.bridge_state
                .as_ref()
                .and_then(|state| state.cell(4, 4))
                .map(|cell| cell.overlay_byte),
            Some(0xE0),
            "fixture must run the real hut dispatcher without a terminal transition"
        );
        let anims = bridge_explosion_anims(&sim);
        assert_eq!(
            anims.len(),
            3,
            "one walker step must construct one explosion for each perpendicular cell"
        );
        for anim in &anims {
            // `CollapseBridge_NS_Low @ 0x00575540`: row `(type, &coord,
            // RandomRanged(1, 5), 1, 0x600, 0, 0)`, Z from the cell level with
            // no deck offset.
            assert_eq!(anim.draw_flags, 0x600);
            assert_eq!(anim.z_adjust, 0);
            assert!((1..=5).contains(&anim.runtime.delay_remaining));
            let (.., level) = anim.world_coord.to_cell_sub_z();
            assert_eq!(
                level, 0,
                "walker explosions sit on the cell level, not the deck"
            );
        }
        assert!(
            sim.sound_events.is_empty(),
            "a delayed anim plays its Report= when the delay expires, not at construction"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            predicted.logical_state(),
            "hut walker presentation must consume exactly three jitter/delay/slot groups"
        );
        assert_eq!(
            sim.main_rng.logical_state(),
            before.main,
            "hut walker presentation must not consume Main"
        );
        assert_eq!(
            sim.mapgen_rng.logical_state(),
            before.mapgen,
            "hut walker presentation must not consume MapGen"
        );
    }

    /// BR-01 + BR-02 (lockstep determinism): a single in-band high body cell
    /// matches BOTH the High SM block (binary block A — its overlay-first
    /// driver routes an in-band cell to the direct walker) AND the High direct
    /// block (block D). With the inter-block early-out removed, the dispatcher
    /// consumes exactly TWO `RandomRanged(1,BridgeStrength)` draws for the one
    /// cell. Before BR-01/02 it consumed one; the missing draw desynced
    /// lockstep on every multi-match cell. `run_dispatch_loop` is used directly
    /// so only the per-block gate draws are measured (the debris/explosion
    /// draws happen later in the cascade).
    #[test]
    fn dispatcher_in_band_cell_consumes_two_block_strength_draws() {
        let mut sim = Simulation::new();
        let seed = 0x0B11_D6E5_u64;
        sim.reseed_scenario_and_main(seed);
        let mut terrain = water_below_bridge_terrain(4);
        // A raw overlay alone admits only D. A also needs a real concrete
        // Middle tile class (without0x100 its height gate is bypassed).
        terrain.cell_mut(5, 5).unwrap().final_tile_index = 1019;
        terrain.test_set_high_bridge_rim_tiles(
            crate::map::bridge_rim_tiles::HighBridgeRimTiles::from_ini(
                1000, b"[General]\nBridgeMiddle1=20\nBridgeMiddle2=40\n"));
        sim.resolved_terrain = Some(terrain);
        let mut bs = BridgeRuntimeState::default();
        bs.test_seed_cell(5, 5, seed_bridge_cell(0xCD));
        sim.bridge_state = Some(bs);

        let bridge_strength = 1500i32;
        // Predict exactly two BridgeStrength gate draws (block A + block D).
        let mut predicted = crate::sim::rng::SimRng::new(seed);
        predicted.next_range_u32_inclusive(1, bridge_strength as u32);
        predicted.next_range_u32_inclusive(1, bridge_strength as u32);

        let event = BridgeDamageEvent {
            rx: 5,
            ry: 5,
            // damage > bridge_strength so both gates pass; the draw COUNT is the
            // lockstep-significant property under test, not the gate outcome.
            damage: 2000,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: false,
            impact_z_leptons: 0,
        };
        let _ = run_dispatch_loop(&mut sim, &[event], bridge_strength, None);

        assert_eq!(
            sim.scenario_rng.state(),
            predicted.state(),
            "in-band high cell must consume exactly 2 BridgeStrength draws (block A + block D)"
        );
    }

    #[test]
    fn gsi_04_01_dispatcher_applies_direct_setter_descriptor() {
        use crate::map::bridge_facts::{BridgeStampSlot, MODELED_CELLCLASS_BRIDGE_FLAG_MASK};
        use crate::sim::bridge_state::{AnchorSpan, Direction};

        let mut sim = Simulation::new();
        let mut terrain = water_below_bridge_terrain(4);
        terrain.test_set_native_allocated_cells(&[(1, 1)]);
        terrain.cell_mut(1, 1).unwrap().bridge_facts.raw_flags = MODELED_CELLCLASS_BRIDGE_FLAG_MASK;
        let dummy = terrain.shared_cell_dummy();
        dummy.apply_bridge_flag_slot(BridgeStampSlot::Anchor, true);
        sim.resolved_terrain = Some(terrain);

        let mut bridge_state = BridgeRuntimeState::default();
        let mut anchor = seed_bridge_cell(24);
        anchor.deck_level = 4;
        anchor.damage_state = DamageState::Damaged;
        anchor.axis = Some(Axis::NS);
        anchor.role = BridgeCellRole::Anchor;
        anchor.anchor_span_id = Some(1);
        bridge_state.test_seed_cell(1, 1, anchor);
        bridge_state.test_seed_anchor_span(AnchorSpan {
            id: 1,
            anchor: (1, 1),
            cells: [
                Some((1, 1)),
                Some((2, 1)),
                Some((3, 1)),
                Some((4, 1)),
                Some((0, 1)),
                None,
            ],
            axis: Axis::NS,
            direction: Direction::E,
            damage_state: DamageState::Damaged,
            bridge_group_id: 1,
        });
        sim.bridge_state = Some(bridge_state);

        let event = BridgeDamageEvent {
            rx: 1,
            ry: 1,
            damage: 1,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: true,
            impact_z_leptons: 416,
        };
        let (outcomes, _) = run_dispatch_loop(&mut sim, &[event], 1500, None);
        assert_eq!(outcomes.len(), 1);
        assert!(matches!(outcomes[0], StateOutcome::Collapsed { .. }));
        assert_eq!(
            dummy.bridge_flags_0x1180(),
            crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF,
            "missing non-anchor slots clear 0x1100 but preserve the dummy's pre-existing 0x80"
        );
        assert_eq!(dummy.snapshot().coord, (0, 1));
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(1, 1)
                .unwrap()
                .bridge_facts
                .raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0
        );
    }

    #[test]
    fn gsi_04_01_ns_perpendicular_dir0_precedes_parent_setter_on_real_and_dummy() {
        use crate::map::bridge_facts::{
            BridgeFlagStamp, BridgeStampSlot, MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        };
        use crate::sim::bridge_state::{AnchorSpan, Direction};

        let mut sim = Simulation::new();
        let mut terrain = water_below_bridge_terrain(4);
        terrain.test_set_native_allocated_cells(&[(2, 2), (3, 2)]);
        for (rx, ry) in [(2, 2), (3, 2)] {
            terrain.cell_mut(rx, ry).unwrap().bridge_facts.raw_flags =
                MODELED_CELLCLASS_BRIDGE_FLAG_MASK;
        }
        let dummy = terrain.shared_cell_dummy();
        dummy.apply_bridge_flag_slot(BridgeStampSlot::Anchor, true);
        sim.install_resolved_terrain_for_new_map(terrain);

        let mut bridge_state = BridgeRuntimeState::default();
        let mut parent = seed_bridge_cell(0);
        parent.damage_state = DamageState::Damaged;
        parent.axis = Some(Axis::NS);
        parent.role = BridgeCellRole::Anchor;
        parent.anchor_span_id = Some(1);
        bridge_state.test_seed_cell(2, 2, parent);

        let mut perpendicular = seed_bridge_cell(0);
        perpendicular.damage_state = DamageState::PartialCollapseB;
        perpendicular.axis = Some(Axis::NS);
        perpendicular.role = BridgeCellRole::Anchor;
        perpendicular.anchor_span_id = None;
        bridge_state.test_seed_cell(3, 2, perpendicular);
        bridge_state.test_seed_anchor_span(AnchorSpan {
            id: 1,
            anchor: (2, 2),
            cells: [
                Some((2, 2)),
                Some((3, 2)),
                Some((4, 2)),
                Some((5, 2)),
                Some((1, 2)),
                None,
            ],
            axis: Axis::NS,
            direction: Direction::E,
            damage_state: DamageState::Damaged,
            bridge_group_id: 1,
        });
        sim.bridge_state = Some(bridge_state);

        let outcome = {
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            sim.bridge_state
                .as_mut()
                .unwrap()
                .body_cell_advance_state(2, 2, true, terrain)
        };
        assert_eq!(
            outcome.setter_transcript(),
            &[
                BridgeFlagStamp::new((3, 2), Direction::N as u8, false),
                BridgeFlagStamp::new((2, 2), Direction::E as u8, false),
            ],
            "native CollapseA perpendicular dir0 setter precedes parent span setter"
        );
        let dummy_after_planning = dummy.snapshot();
        assert_eq!(
            dummy_after_planning.coord,
            (1, 2),
            "with no later helper lookup, the parent direction-2 opposite slot remains the final dummy writer"
        );
        apply_runtime_bridge_flag_transcript_from_outcome(&mut sim, &outcome);

        let terrain = sim.resolved_terrain.as_mut().unwrap();
        assert_eq!(
            terrain.cell(3, 2).unwrap().bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "perpendicular allocated CellClass is cleared through the live seam"
        );
        assert_eq!(
            terrain.cell(2, 2).unwrap().bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "later parent setter clears its allocated anchor"
        );
        assert_eq!(
            sim.real_cell_bridge_flags_0x1180,
            terrain.capture_real_cell_bridge_flags_0x1180(),
            "ordered live projection keeps serialized real-cell value authority exact"
        );
        assert_eq!(
            dummy.bridge_flags_0x1180(),
            crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF,
            "missing non-anchor slots preserve the dummy's independent live anchor bit"
        );
        assert_eq!(
            dummy.snapshot(),
            dummy_after_planning,
            "real-only transcript projection cannot replay the parent setter into the dummy"
        );
    }

    #[test]
    fn gsi_04_01_recursive_dir0_chain_projects_deepest_first_to_real_and_dummy() {
        use crate::map::bridge_facts::{
            BridgeFlagStamp, BridgeStampSlot, MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        };
        use crate::sim::bridge_state::{Direction, Phase};

        let mut sim = Simulation::new();
        let mut terrain = water_below_bridge_terrain(4);
        terrain.test_set_native_allocated_cells(&[(3, 2), (4, 2), (5, 2)]);
        for x in 3..=5 {
            terrain.cell_mut(x, 2).unwrap().bridge_facts.raw_flags =
                MODELED_CELLCLASS_BRIDGE_FLAG_MASK;
        }
        let dummy = terrain.shared_cell_dummy();
        dummy.apply_bridge_flag_slot(BridgeStampSlot::Anchor, true);
        sim.install_resolved_terrain_for_new_map(terrain);

        let mut bridge_state = BridgeRuntimeState::default();
        let mut anchor = seed_bridge_cell(0);
        anchor.axis = Some(Axis::NS);
        anchor.role = BridgeCellRole::Anchor;
        bridge_state.test_seed_cell(2, 2, anchor);
        let mut chained = anchor;
        chained.damage_state = DamageState::PartialCollapseB;
        for x in 3..=5 {
            bridge_state.test_seed_cell(x, 2, chained);
        }
        sim.bridge_state = Some(bridge_state);

        let outcome = {
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            crate::sim::bridge_specs::update_ramp_perpendicular(
                sim.bridge_state.as_mut().unwrap(),
                (2, 2),
                Axis::NS,
                Phase::CollapseA,
                true,
                terrain,
            )
        };
        assert_eq!(
            outcome.setter_transcript,
            vec![
                BridgeFlagStamp::new((5, 2), Direction::N as u8, false),
                BridgeFlagStamp::new((4, 2), Direction::N as u8, false),
                BridgeFlagStamp::new((3, 2), Direction::N as u8, false),
            ]
        );
        for &stamp in &outcome.setter_transcript {
            sim.apply_planned_bridge_flag_stamp_to_real_cells(stamp);
        }

        let terrain = sim.resolved_terrain.as_mut().unwrap();
        for x in 3..=5 {
            assert_eq!(
                terrain.cell(x, 2).unwrap().bridge_facts.raw_flags
                    & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
                0,
                "every allocated recursive anchor is cleared"
            );
        }
        assert_eq!(
            sim.real_cell_bridge_flags_0x1180,
            terrain.capture_real_cell_bridge_flags_0x1180(),
            "each recursive setter updates serialized real-cell value authority"
        );
        assert_eq!(
            dummy.bridge_flags_0x1180(),
            crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF,
            "none of the recursive setter anchors resolve to the dummy"
        );
        assert_eq!(
            dummy.snapshot().coord,
            (3, 3),
            "outermost dir0 opposite lookup is last after deepest-first projection"
        );
    }

    #[test]
    fn gsi_04_01_later_about_to_fall_lookup_wins_after_synchronous_setter() {
        use crate::map::bridge_facts::{
            BRIDGE_FLAG_ANCHOR_SELF, BridgeFlagStamp, MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        };
        use crate::sim::bridge_state::{BridgeheadAnchorClass, Direction, Phase};

        let mut sim = Simulation::new();
        let mut terrain = water_below_bridge_terrain(4);
        terrain.test_set_native_allocated_cells(&[(3, 2)]);
        terrain.cell_mut(3, 2).unwrap().bridge_facts.raw_flags = MODELED_CELLCLASS_BRIDGE_FLAG_MASK;
        // AboutToFall recursion is a raw middle-tile branch, not a role gate.
        terrain.cell_mut(3, 2).unwrap().final_tile_index = 9;
        terrain.test_set_high_bridge_rim_tiles(crate::map::bridge_rim_tiles::HighBridgeRimTiles::from_ini(
            0,b"[General]\nBridgeMiddle1=7\nBridgeMiddle2=12\nBridgeBottomRight1=3\nBridgeBottomRight2=3\n"));
        terrain.test_set_dummy_cell_level_slope(2, 0);
        let dummy = terrain.shared_cell_dummy();
        dummy.set_bridge_flags_0x1180(MODELED_CELLCLASS_BRIDGE_FLAG_MASK);
        sim.install_resolved_terrain_for_new_map(terrain);

        let mut bridge_state = BridgeRuntimeState::default();
        let mut perpendicular = seed_bridge_cell(0);
        perpendicular.damage_state = DamageState::PartialCollapseB;
        perpendicular.axis = Some(Axis::NS);
        perpendicular.role = BridgeCellRole::Anchor;
        perpendicular.bridgehead_anchor_class = BridgeheadAnchorClass::AboutToFall;
        bridge_state.test_seed_cell(3, 2, perpendicular);
        sim.bridge_state = Some(bridge_state);

        let outcome = {
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            crate::sim::bridge_specs::update_ramp_perpendicular(
                sim.bridge_state.as_mut().unwrap(),
                (2, 2),
                Axis::NS,
                Phase::CollapseA,
                true,
                terrain,
            )
        };
        assert_eq!(
            outcome.setter_transcript,
            vec![BridgeFlagStamp::new((3, 2), Direction::N as u8, false)]
        );
        let dummy_after_planning = dummy.snapshot();
        assert_eq!(
            dummy_after_planning.coord,
            (4, 2),
            "the independent AboutToFall recursion's later GetCell miss wins after the setter's final slot"
        );
        assert_eq!(
            dummy_after_planning.bridge_flags_0x1180, BRIDGE_FLAG_ANCHOR_SELF,
            "the synchronous setter clears missing-slot 0x1100 while preserving non-anchor 0x80"
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
            crate::sim::projectile::ProjectileCoord::new(4 * 256 + 128, 2 * 256 + 128, 2 * 104,)
        );

        for &stamp in &outcome.setter_transcript {
            sim.apply_planned_bridge_flag_stamp_to_real_cells(stamp);
        }
        assert_eq!(
            dummy.snapshot(),
            dummy_after_planning,
            "deferred real-only commit must not overwrite the later live GetCell coordinate"
        );
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        assert_eq!(
            terrain.cell(3, 2).unwrap().bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0
        );
        assert_eq!(
            sim.real_cell_bridge_flags_0x1180,
            terrain.capture_real_cell_bridge_flags_0x1180()
        );
    }

    #[test]
    fn gsi_04_01_bridgehead_perpendicular_dir6_projects_without_outer_setter() {
        use crate::map::bridge_facts::{
            BridgeFlagStamp, BridgeStampSlot, MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        };
        use crate::sim::bridge_state::{BridgeheadAnchorClass, Direction};

        let mut sim = Simulation::new();
        let mut terrain = water_below_bridge_terrain(4);
        terrain.cell_mut(2, 2).unwrap().template_height = 2;
        terrain.test_set_native_allocated_cells(&[(2, 2), (2, 3)]);
        terrain.cell_mut(2, 3).unwrap().bridge_facts.raw_flags = MODELED_CELLCLASS_BRIDGE_FLAG_MASK;
        let dummy = terrain.shared_cell_dummy();
        dummy.apply_bridge_flag_slot(BridgeStampSlot::Anchor, true);
        sim.install_resolved_terrain_for_new_map(terrain);

        let mut bridge_state = BridgeRuntimeState::default();
        let mut bridgehead = seed_bridge_cell(0);
        bridgehead.axis = Some(Axis::EW);
        bridgehead.role = BridgeCellRole::Bridgehead;
        bridgehead.bridgehead_anchor_class = BridgeheadAnchorClass::AboutToFall;
        bridge_state.test_seed_cell(2, 2, bridgehead);

        let mut perpendicular = seed_bridge_cell(0);
        perpendicular.damage_state = DamageState::PartialCollapseB;
        perpendicular.axis = Some(Axis::EW);
        perpendicular.role = BridgeCellRole::Anchor;
        bridge_state.test_seed_cell(2, 3, perpendicular);
        sim.bridge_state = Some(bridge_state);

        let outcome = {
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            sim.bridge_state
                .as_mut()
                .unwrap()
                .bridgehead_advance_state(2, 2, false, terrain)
        };
        let StateOutcome::Collapsed {
            set_bridge_direction,
            ..
        } = &outcome
        else {
            panic!("final bridgehead must collapse");
        };
        assert!(
            set_bridge_direction.flag_stamp.is_none(),
            "bridgehead row has no parent SetBridgeDirection header"
        );
        assert_eq!(
            outcome.setter_transcript(),
            &[BridgeFlagStamp::new((2, 3), Direction::W as u8, false)],
            "EW complementary partial emits the native direction-6 helper setter"
        );
        let dummy_after_planning = dummy.snapshot();
        assert_eq!(
            dummy_after_planning.coord,
            (2, 1),
            "the later CollapseB perpendicular GetCell miss follows the direction-6 setter"
        );
        apply_runtime_bridge_flag_transcript_from_outcome(&mut sim, &outcome);

        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(2, 3)
                .unwrap()
                .bridge_facts
                .raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0
        );
        assert_eq!(
            dummy.bridge_flags_0x1180(),
            crate::map::bridge_facts::BRIDGE_FLAG_ANCHOR_SELF,
            "direction-6 non-anchor dummy visits preserve pre-existing 0x80"
        );
        assert_eq!(
            dummy.snapshot(),
            dummy_after_planning,
            "real-only projection preserves the later CollapseB lookup over the earlier direction-6 extra"
        );
    }

    #[test]
    fn gsi_04_01_hut_low_bridgehead_retries_share_live_anchor_flags() {
        use crate::map::bridge_facts::{
            BRIDGE_FLAG_ANCHOR_SELF, BridgeFlagStamp, MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
        };
        use crate::sim::bridge_state::{BridgeheadAnchorClass, Direction};

        let mut sim = Simulation::new();
        let mut terrain = water_below_bridge_terrain(4);
        terrain.test_set_native_allocated_cells(&[(2, 2), (2, 3), (2, 4), (3, 2)]);
        for (ry, height) in [(4, 8), (3, 6), (2, 4)] {
            terrain.cell_mut(2, ry).unwrap().template_height = height;
        }
        terrain.cell_mut(2, 4).unwrap().is_wood_bridge_repair_tile = true;
        terrain.cell_mut(3, 2).unwrap().bridge_facts.raw_flags = MODELED_CELLCLASS_BRIDGE_FLAG_MASK;
        let dummy = terrain.shared_cell_dummy();
        dummy.set_bridge_flags_0x1180(MODELED_CELLCLASS_BRIDGE_FLAG_MASK);
        sim.install_resolved_terrain_for_new_map(terrain);

        let mut bridge_state = BridgeRuntimeState::default();
        let mut bridgehead = seed_bridge_cell(0x18);
        bridgehead.axis = Some(Axis::NS);
        bridgehead.role = BridgeCellRole::Bridgehead;
        bridgehead.bridgehead_anchor_class = BridgeheadAnchorClass::AboutToFall;
        bridge_state.test_seed_cell(2, 4, bridgehead);

        let mut anchor = seed_bridge_cell(0x20);
        anchor.axis = Some(Axis::NS);
        anchor.role = BridgeCellRole::Anchor;
        anchor.bridgehead_anchor_class = BridgeheadAnchorClass::AboutToFall;
        bridge_state.test_seed_cell(2, 2, anchor);

        let mut perpendicular = seed_bridge_cell(0x21);
        perpendicular.axis = Some(Axis::NS);
        perpendicular.role = BridgeCellRole::Anchor;
        perpendicular.damage_state = DamageState::PartialCollapseB;
        bridge_state.test_seed_cell(3, 2, perpendicular);
        sim.bridge_state = Some(bridge_state);

        let outcomes = {
            let terrain = sim.resolved_terrain.as_mut().unwrap();
            let mut live_flags = terrain.bridge_flag_execution_state();
            let outcomes = apply_hut_damage_retries(
                sim.bridge_state.as_mut().unwrap(),
                terrain,
                (2, 4),
                &mut live_flags,
            );
            assert_eq!(
                live_flags.flags_at((3, 2)) & BRIDGE_FLAG_ANCHOR_SELF,
                0,
                "the first synchronous helper setter clears live 0x80 before retry two"
            );
            outcomes
        };

        assert_eq!(outcomes.len(), MAX_HUT_ATTEMPTS_PER_STEP);
        assert!(outcomes.iter().all(|outcome| matches!(
            outcome,
            StateOutcome::Collapsed {
                binary_success: false,
                ..
            }
        )));
        let transcript: Vec<_> = outcomes
            .iter()
            .flat_map(|outcome| outcome.setter_transcript().iter().copied())
            .collect();
        assert_eq!(
            transcript,
            vec![BridgeFlagStamp::new((3, 2), Direction::N as u8, false)],
            "later low-return retries observe cleared 0x80 and cannot invent another setter"
        );
        assert_eq!(
            sim.bridge_state
                .as_ref()
                .unwrap()
                .cell(3, 2)
                .unwrap()
                .damage_state,
            DamageState::Destroyed
        );

        // Planning/retries defer allocated real CellClass values, but missing
        // slots have already mutated the actual shared dummy synchronously.
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .unwrap()
                .cell(3, 2)
                .unwrap()
                .bridge_facts
                .raw_flags
                & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            MODELED_CELLCLASS_BRIDGE_FLAG_MASK
        );
        let dummy_after_planning = dummy.snapshot();
        assert_eq!(
            dummy_after_planning.bridge_flags_0x1180, BRIDGE_FLAG_ANCHOR_SELF,
            "the first setter's missing non-anchor slots are live during later CABHUT retries"
        );

        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str("[General]\n"))
            .expect("minimal hut retry rules");
        sim.resolve_type_handles(&rules);
        assert!(apply_hut_bridge_execution(
            &mut sim, &rules, &outcomes, false, None, None,
        ));

        let terrain = sim.resolved_terrain.as_mut().unwrap();
        assert_eq!(
            terrain.cell(3, 2).unwrap().bridge_facts.raw_flags & MODELED_CELLCLASS_BRIDGE_FLAG_MASK,
            0,
            "the single retained transcript projects once to the allocated real cell"
        );
        assert_eq!(
            dummy.snapshot(),
            dummy_after_planning,
            "real-only outcome projection must leave the synchronously mutated dummy byte-for-byte unchanged"
        );
        assert_eq!(
            sim.real_cell_bridge_flags_0x1180,
            terrain.capture_real_cell_bridge_flags_0x1180(),
            "the one actual projection keeps serialized value authority exact"
        );
    }

    /// Seed search for a cell that passes the outer 95% gate and lands on the
    /// wanted side of the 50% metallic gate.
    fn debris_seed(metallic_passes: bool) -> u64 {
        (1u64..10_000)
            .find(|seed| {
                let mut rng = crate::sim::rng::SimRng::new(*seed);
                let outer = rng.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
                if outer >= BRIDGE_DEBRIS_OUTER_GATE_EXCLUSIVE {
                    return false;
                }
                let _ = rng.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
                let _ = rng.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
                let metallic = rng.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
                (metallic < BRIDGE_METALLIC_GATE_EXCLUSIVE) == metallic_passes
            })
            .expect("fixture seed")
    }

    /// Multi-entry lists: a slot draw over a one-entry list consumes nothing,
    /// which would make the draw-count assertions below vacuous.
    fn debris_sim(seed: u64) -> Simulation {
        let mut sim = Simulation::new();
        sim.reseed_scenario_and_main(seed);
        sim.resolved_terrain = Some(water_below_bridge_terrain(3));
        for name in ["BRIDGEEXP1", "BRIDGEEXP2"] {
            let id = sim.interner.intern(name);
            sim.bridge_explosions.push(id);
        }
        for name in ["METALDEB1", "METALDEB2", "METALDEB3"] {
            let id = sim.interner.intern(name);
            sim.metallic_debris.push(id);
        }
        sim
    }

    /// The scenario stream after one fallout cell, with or without the
    /// MetallicDebris slot draw.
    fn debris_stream(seed: u64, metallic_slot: bool) -> crate::sim::rng::SimRng {
        let mut rng = crate::sim::rng::SimRng::new(seed);
        for _ in 0..4 {
            rng.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
        }
        if metallic_slot {
            rng.next_range_u32(3);
        }
        rng.next_range_u32_inclusive(1, 5);
        rng.next_range_u32(2);
        rng
    }

    /// A failed 50% gate takes no MetallicDebris slot draw, even with
    /// BridgeVoxelMax at zero.
    #[test]
    fn bridge_debris_no_metallic_when_gate_fails_even_with_voxel_zero() {
        let seed = debris_seed(false);
        let mut sim = debris_sim(seed);
        let rules = rules_with_voxel_max(0);

        let mut cells = BTreeSet::new();
        cells.insert((5, 5));
        spawn_bridge_debris(&mut sim, &rules, &cells);

        assert_ne!(
            debris_stream(seed, false).logical_state(),
            debris_stream(seed, true).logical_state(),
            "fixture must tell the two sequences apart"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            debris_stream(seed, false).logical_state(),
            "metallic gate failure must skip the MetallicDebris slot draw"
        );
    }

    /// A passed gate takes the slot draw whatever BridgeVoxelMax says.
    #[test]
    fn bridge_debris_ignores_bridge_voxel_max_when_metallic_gate_passes() {
        let seed = debris_seed(true);
        let mut sim = debris_sim(seed);
        let rules = rules_with_voxel_max(0);

        let mut cells = BTreeSet::new();
        cells.insert((5, 5));
        spawn_bridge_debris(&mut sim, &rules, &cells);

        assert_ne!(
            debris_stream(seed, false).logical_state(),
            debris_stream(seed, true).logical_state(),
            "fixture must tell the two sequences apart"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            debris_stream(seed, true).logical_state(),
            "BridgeVoxelMax=0 must not suppress the BlowUpBridge metallic slot draw"
        );
    }

    /// Debris helper short-circuits when the required BridgeExplosion list is
    /// empty. MetallicDebris alone does not enable BlowUpBridge presentation.
    #[test]
    fn bridge_debris_requires_bridge_explosion_list() {
        let mut sim = Simulation::new();
        sim.reseed_scenario_and_main(7);
        let baseline_state = sim.scenario_rng.state();
        sim.resolved_terrain = Some(water_below_bridge_terrain(3));
        let metallic_debris = sim.interner.intern("METALDEB1");
        sim.metallic_debris.push(metallic_debris);
        let rules = rules_with_voxel_max(3);

        let mut cells = BTreeSet::new();
        cells.insert((5, 5));
        cells.insert((4, 5));
        spawn_bridge_debris(&mut sim, &rules, &cells);

        assert_eq!(
            sim.scenario_rng.state(),
            baseline_state,
            "no RNG draws when BridgeExplosion metadata is absent"
        );
        assert!(sim.substrate.anims.iter().next().is_none());
    }

    /// Task 11 — DropIn must NOT touch entities that aren't on the bridge
    /// Rim refresh resets dangling stub cells whose anchor span has gone
    /// away. Layout: anchor span 1 owns (4,2)+(5,2)+(6,2). After collapse,
    /// drop the span entry from the registry, mark (5,2) Destroyed (the
    /// "head" candidate), and call `update_adjacent_bridges` with rim cell
    /// (4,2). Expected: (4,2)→(5,2) walks east, (5,2) is the head so the
    /// loop continues past it; once it sees an orphan-anchor cell, the
    /// reset fires.
    #[test]
    fn rim_refresh_clears_dangling_stub_cells() {
        use crate::sim::bridge_state::{BridgeRuntimeCell, BridgeRuntimeState};
        let mut sim = Simulation::new();
        let mut bs = BridgeRuntimeState::default();
        // (5,2): destroyed head (acts as direction beacon).
        bs.test_seed_cell(
            5,
            2,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 4,
                bridge_group_id: Some(1),
                damage_state: DamageState::Destroyed,
                axis: Some(crate::sim::bridge_state::Axis::EW),
                role: BridgeCellRole::Body,
                anchor_span_id: Some(99),
                overlay_byte: 0xE8,
                bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
            },
        );
        // (6,2): dangling stub — anchor_span_id=99 but no AnchorSpan entry.
        bs.test_seed_cell(
            6,
            2,
            BridgeRuntimeCell {
                deck_present: true,
                destroyable: true,
                deck_level: 4,
                bridge_group_id: Some(1),
                damage_state: DamageState::Healthy { variant: 0 },
                axis: Some(crate::sim::bridge_state::Axis::EW),
                role: BridgeCellRole::Body,
                anchor_span_id: Some(99),
                overlay_byte: 0xDC,
                bridgehead_anchor_class: crate::sim::bridge_state::BridgeheadAnchorClass::Variant0,
            },
        );
        sim.bridge_state = Some(bs);

        let mut rim: BTreeSet<(u16, u16)> = BTreeSet::new();
        rim.insert((4, 2));
        update_adjacent_bridges(&mut sim, &rim);

        let stub = sim.bridge_state.as_ref().unwrap().cell(6, 2).unwrap();
        assert_eq!(stub.overlay_byte, 0xFF, "stub overlay reset to NONE");
        assert!(matches!(
            stub.damage_state,
            DamageState::Healthy { variant: 0 }
        ));
        assert!(stub.bridge_group_id.is_none());
        assert!(!stub.deck_present);
    }

    /// layer at the destroyed cell. Ground-layer entities are handled by
    /// `kill_ground_occupants_at` (Step 1), not DropIn.
    #[test]
    fn drop_in_ignores_ground_layer_entities_at_destroyed_cell() {
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(water_below_bridge_terrain(3));
        let mut entity = GameEntity::new_at_frame_zero_for_test(
            1,
            5,
            5,
            0,
            64,
            test_intern("Americans"),
            Health { current: 256 },
            test_intern("MTNK"),
            crate::map::entities::EntityCategory::Unit,
            0,
            5,
            true,
        );
        entity.on_bridge = false; // ground-layer occupant
        let mut loco = drive_loco_on_bridge();
        loco.layer = MovementLayer::Bridge;
        entity.locomotor = Some(loco);
        sim.substrate.entities.insert(entity);
        sim.substrate.occupancy.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        drop_in_bridge_deck_entities(&mut sim, 5, 5);

        // Ground entity untouched — still alive, still ground layer.
        let e = sim
            .substrate
            .entities
            .get(1)
            .expect("ground entity untouched");
        assert_eq!(e.health.current, 256);
        assert!(!e.on_bridge);
        assert_eq!(e.locomotor.as_ref().unwrap().layer, MovementLayer::Bridge);
        let cell = sim.substrate.occupancy.get(5, 5).expect("ground occupancy");
        assert_eq!(cell.count_on(MovementLayer::Ground), 1);
        assert_eq!(cell.count_on(MovementLayer::Bridge), 0);
    }

    /// Debris RNG gate boundaries reproduce the original engine's float
    /// truncation (`scale = 2^-31 + 2^-61`), NOT the naive `threshold * 2^31`.
    /// A draw landing exactly on the float boundary must FAIL the gate; a
    /// spurious pass spends extra slot draws and desyncs lockstep. This test
    /// fails if either constant is "simplified" back to its 2^31-scaled value.
    #[test]
    fn bridge_debris_gate_boundaries_match_float_truncation() {
        // Outer 95% gate: largest passing draw is 2_040_109_463.
        assert!(2_040_109_463 < BRIDGE_DEBRIS_OUTER_GATE_EXCLUSIVE);
        assert!(2_040_109_464 >= BRIDGE_DEBRIS_OUTER_GATE_EXCLUSIVE);
        // Metallic 50% gate: largest passing draw is 0x3FFF_FFFE; the value
        // that maps to exactly 0.5 (0x3FFF_FFFF) must fail under strict `<`.
        assert!(0x3FFF_FFFE_u32 < BRIDGE_METALLIC_GATE_EXCLUSIVE);
        assert!(0x3FFF_FFFF_u32 >= BRIDGE_METALLIC_GATE_EXCLUSIVE);
    }

    /// BlowUpBridge force-kills only the cell's ground object-list occupants.
    /// An aircraft overflying the collapse cell (air layer, `on_bridge=false`)
    /// is not on that list and must survive; a ground unit at the cell dies.
    #[test]
    fn bridge_collapse_kill_spares_airborne_units() {
        let mut sim = Simulation::new();

        // Ground unit at (5,5): no locomotor => Ground layer, on_bridge=false.
        let ground = GameEntity::new_at_frame_zero_for_test(
            1,
            5,
            5,
            0,
            64,
            sim.interner.intern("Americans"),
            Health { current: 256 },
            sim.interner.intern("MTNK"),
            crate::map::entities::EntityCategory::Unit,
            0,
            5,
            true,
        );
        sim.substrate.entities.insert(ground);

        // Aircraft hovering over (5,5): Air layer, on_bridge=false.
        let mut air = GameEntity::new_at_frame_zero_for_test(
            2,
            5,
            5,
            12,
            64,
            sim.interner.intern("Americans"),
            Health { current: 256 },
            sim.interner.intern("ORCA"),
            crate::map::entities::EntityCategory::Aircraft,
            0,
            5,
            true,
        );
        let mut loco = drive_loco_on_bridge();
        loco.layer = MovementLayer::Air;
        air.locomotor = Some(loco);
        air.on_bridge = false;
        sim.substrate.entities.insert(air);

        // Consumed Engineers can retain positive HP after UnInit until the
        // deferred drain. The legacy coordinate scan must not restart their
        // Infantry terminal lifetime when it visits this stored identity.
        let retired = GameEntity::new_at_frame_zero_for_test(
            3,
            5,
            5,
            0,
            0,
            sim.interner.intern("Americans"),
            Health { current: 100 },
            sim.interner.intern("E1"),
            crate::map::entities::EntityCategory::Infantry,
            0,
            5,
            false,
        );
        sim.substrate.entities.insert(retired);
        sim.uninit(3);
        assert!(sim.substrate.entities.get(3).unwrap().health.current > 0);

        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=256\nArmor=heavy\n[Warheads]\n0=Super\n[Super]\nInfDeath=1\n",
        )).unwrap();
        let ground = sim.substrate.entities.get_mut(1).unwrap();
        ground.lifecycle.in_limbo = false;
        ground.lifecycle.cell_marked = true;
        sim.substrate.occupancy.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
        kill_ground_occupants_at(&mut sim, &rules, 5, 5, None);

        let retired = sim.substrate.entities.get(3).unwrap();
        assert_eq!(
            retired.health.current, 100,
            "unlinked retired object is not a ground receiver"
        );
        assert!(!retired.lifecycle.object_alive);
        assert!(retired.infantry_terminal.is_none());
        assert_eq!(sim.substrate.pending_delete, vec![3, 1]);

        let g = sim.substrate.entities.get(1).expect("ground unit present");
        assert_eq!(g.health.current, 0, "ground occupant is force-killed");
        assert!(g.dying, "ground occupant flagged dying");

        let a = sim.substrate.entities.get(2).expect("aircraft present");
        assert_eq!(
            a.health.current, 256,
            "aircraft overflying the collapse cell must NOT be killed"
        );
        assert!(!a.dying, "aircraft not flagged dying");
    }

    /// `CellClass::BlowUpBridge @ 0x0047DDAE` kills each ground occupant
    /// through `ReceiveDamage` (`+0x16C`, `C4Warhead=`), so the deaths reach
    /// `Death_Announcement` (`+0x3B8`): every human-owned death publishes the
    /// radar type-7 request (`0x004D98FE`) that rate-limits "Unit lost" on the
    /// owner's client; an AI owner publishes nothing.
    #[test]
    fn bridge_collapse_kill_publishes_unit_lost_per_human_death() {
        use crate::sim::house_state::HouseState;
        use crate::sim::world::SimSoundEvent;

        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=300\nArmor=heavy\n[Warheads]\n0=Super\n[Super]\nInfDeath=1\n",
        ))
        .expect("rules parse");
        let mut sim = Simulation::new();
        let human = sim.interner.intern("Americans");
        let ai = sim.interner.intern("Russians");
        sim.houses.insert(
            human,
            HouseState::new(human, 0, Some(human), true, 5_000, 10),
        );
        sim.houses
            .insert(ai, HouseState::new(ai, 0, Some(ai), false, 5_000, 10));
        sim.session.house_order.push(human);
        sim.session.house_order.push(ai);

        let mtnk = sim.interner.intern("MTNK");
        for (id, owner) in [(1u64, human), (2, human), (3, ai)] {
            let unit = GameEntity::new_at_frame_zero_for_test(
                id,
                5,
                5,
                0,
                64,
                owner,
                Health { current: 256 },
                mtnk,
                crate::map::entities::EntityCategory::Unit,
                0,
                5,
                true,
            );
            sim.substrate.entities.insert(unit);
        }

        for id in 1..=3 {
            let unit = sim.substrate.entities.get_mut(id).unwrap();
            unit.lifecycle.in_limbo = false;
            unit.lifecycle.cell_marked = true;
            sim.substrate.occupancy.add(
                5,
                5,
                id,
                MovementLayer::Ground,
                None,
                crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
            );
        }
        kill_ground_occupants_at(&mut sim, &rules, 5, 5, None);

        let lost: Vec<_> = sim
            .sound_events
            .iter()
            .filter_map(|event| match event {
                SimSoundEvent::UnitLost { owner, .. } => Some(*owner),
                _ => None,
            })
            .collect();
        assert_eq!(
            lost,
            vec![human, human],
            "each human kill publishes its type-7 request (the client's radar              array dedupes them); the AI kill is silent"
        );
        for id in 1..=3 {
            assert!(sim.substrate.entities.get(id).unwrap().dying);
        }
    }
}

#[cfg(test)]
#[path = "bridge_deck_tests.rs"]
mod deck_tests;
