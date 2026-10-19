//! Bridge damage orchestrator — 4-path dispatcher + cascade consumers.
//!
//! Each area-damage continuation calls this owner synchronously after its
//! receivers and before returning to the bullet's animation/cluster tail.
//! The four native admission blocks run in fixed order, selecting drivers from
//! live state. Every driver publishes synchronously through the live host.
//! Debris uses the shared AnimClass lifecycle;
//! tagged collapse notification remains a required dependency.
//!
//! ## Dependency rules
//! Same as sim/world: depends on sim/bridge_state, sim/rng, rules/, map/;
//! never render / ui / audio / net.

use crate::sim::world::FrameEffects;
use std::collections::BTreeSet;

#[path = "bridge_damage_dispatch.rs"]
mod damage_dispatch;
pub(crate) use damage_dispatch::BridgeDamageDrivers;
use damage_dispatch::LiveCells;
pub(super) use damage_dispatch::PendingBridgeCells;

#[path = "bridge_moving_collapse.rs"]
mod moving_collapse;

#[path = "bridge_ground.rs"]
mod ground_fallout;

#[path = "bridge_publication.rs"]
mod live_publication;

pub(crate) use live_publication::repair_from_engineer;

#[cfg(test)]
pub(super) use live_publication::{ready_engineer, ready_repair_fixture, repair_frame};

use crate::map::bridge_facts::{BRIDGE_FLAG_ANCHOR_SELF, BRIDGE_FLAG_STRUCTURAL};
use crate::map::bridge_rim_tiles::HighBridgeRimTiles;
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::ResolvedTerrainGrid;
use crate::rules::ruleset::RuleSet;
use crate::sim::anim_class::AnimWorldCoord;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::bridge_state::BridgeRuntimeState;
use crate::sim::bridge_state::ramp_repair::{self, Family as HutBridgeFamily};
use crate::sim::bridge_state::{Axis, BridgeDamageEvent, DispatchPath};
use crate::sim::bridge_state::{ordinary, rim};
use crate::sim::scenario_bootstrap::NativeStartBounds;
use crate::sim::world::Simulation;
use crate::sim::{intern::InternedId, rng::SimRng};
use crate::util::lepton::{BRIDGE_DECK_HEIGHT_LEPTONS, LEPTONS_PER_LEVEL};
use crate::util::native_x87::{NativeF64Bits, X87Chop53};

#[cfg(test)]
#[path = "bridge_debris_tests.rs"]
mod debris_tests;

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
/// Returns `true` if any driver collapsed a span. Callers use this to signal
/// `TickResult.bridge_state_changed` so the app consumes the already-published
/// navigation and refreshes presentation. Each driver runs its own cascade
/// (fallout, rim, zones) synchronously.
#[cfg(any(test, feature = "test-support"))]
pub fn apply_bridge_damage_events(
    sim: &mut Simulation,
    rules: &RuleSet,
    events: &[BridgeDamageEvent],
) -> bool {
    apply_bridge_damage_events_with_overlay_registry(
        sim,
        rules,
        events,
        None,
        crate::sim::world::FrameEffects::default(),
    )
}

pub(crate) fn apply_bridge_damage_events_with_overlay_registry(
    sim: &mut Simulation,
    rules: &RuleSet,
    events: &[BridgeDamageEvent],
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let mut collapsed = false;
    // Finish each event's existing callbacks before another event can enter
    // the live body driver. Otherwise its immediate fallout would overtake an
    // earlier event's still-pending direct/head fallout.
    for event in events {
        collapsed |=
            apply_one_bridge_damage_event(sim, rules, event, overlay_registry, frame_effects);
    }
    collapsed
}

/// `TActionClass::Execute`63 (`0x006DDDC9`) -> DamageAtWaypoint6E0490.
/// Five synchronous AreaDamage489280 visits precede the direct587180 tail.
/// Re-query the waypoint before each visit: an earlier bridge collapse can
/// change the following height. +90 is the waypoint, not the trailing+44.
/// Original dispatch/geometry and NANRCT receiver controls:
/// `tools/spatial_oracle/trigger_damage_at_waypoint.json` (+ metadata).
/// Receiver and bridge state remain with their existing shared owners.
pub(crate) fn damage_at_waypoint(
    sim: &mut Simulation,
    rules: &RuleSet,
    waypoint: i32,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> Option<bool> {
    use crate::sim::combat::{RAD_NO_ATTACKER, world_receiver::apply_area_damage};
    use crate::sim::projectile::ProjectileCoord;

    let warhead = rules.warhead(&rules.bridge_warheads.c4_name)?;
    let warhead_id = sim.interner.intern(&rules.bridge_warheads.c4_name);
    let mut changed = false;
    for (dx, dy) in [(0, 0), (85, 85), (-85, 85), (85, -85), (-85, -85)] {
        let [x, y, z] = sim.trigger_waypoint_location(waypoint)?;
        changed |= apply_area_damage(
            sim,
            rules,
            overlay_registry,
            ProjectileCoord::new(x.wrapping_add(dx), y.wrapping_add(dy), z),
            100,
            warhead,
            (RAD_NO_ATTACKER, None, warhead_id),
            frame_effects,
        );
    }
    // IsWaypointValid68BD80 gates only the tail, after all five area visits.
    // The retained unset coordinate is zero; the shared session bounds the index.
    if sim.session.waypoint_is_valid(waypoint)
        && let Some(cell) = sim.session.waypoint_cell(waypoint)
    {
        let mut drivers = BridgeDamageDrivers::new(sim, rules, overlay_registry, frame_effects);
        for _ in 0..4 {
            if drivers.apply_damage_to_cell(cell) {
                break;
            }
        }
        changed |= drivers.collapsed;
        // 6E0705..6E077A invalidates a256px tactical rectangle. VERA redraws
        // the whole tactical scene each frame; no retained screen pixels exist.
    }
    Some(changed)
}

fn apply_one_bridge_damage_event(
    sim: &mut Simulation,
    rules: &RuleSet,
    event: &BridgeDamageEvent,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let events = std::slice::from_ref(event);

    // Outer gate + read bridge_strength up front (immutable borrow scope).
    let bridge_strength = match sim.bridge_state.as_ref() {
        Some(bs) if bs.is_destroyable() => bs.bridge_strength(),
        _ => return false,
    };

    // Every driver publishes its cascade synchronously.
    damage_dispatch::run(
        sim,
        events,
        bridge_strength,
        rules,
        overlay_registry,
        frame_effects,
    )
}

/// Bridge-collapse dispatch from a `BridgeRepairHut` death event (C4 timer
/// expired, demo-truck explosion). Chooses low/high from hut-local evidence;
/// a 5x5 overlay seed runs the bounded collapse walker, and without one the
/// fallback walk ([`run_hut_fallback`]) damages the bridge through
/// ApplyDamageToCell. Both ordinary overlay families publish Recalc and
/// occupant callbacks synchronously.
///
/// Returns `true` if a bridge cell collapsed or the fallback found an anchor,
/// after which native always rebuilds zones (caller ORs into
/// `bridge_state_changed` so the app rebuilds the PathGrid).
///
/// Caller ensures the hut itself is not damaged — the hut survives the
/// collapse, mirroring the original game's `BridgeRepairHut` death branch.
#[cfg(any(test, feature = "test-support"))]
/// `MapClass::DestroyBridge_High_OnHutDeath` 0x00574000 and
/// `DestroyBridge_Low_OnHutDeath` 0x00574C20 — the CABHUT death entry.
/// Concrete callers are `BombClass::Detonate` 0x00438720 (0x00438982)
/// and `BuildingClass::Update` 0x0043FB20 (0x0044031B). The wooden alternatives
/// call574C20 at43896A/440301. The 5x5
/// overlay scan hands its first hit to
/// `MapClass::DestroyBridgeFromCell_Low` 0x00574780 /
/// `_High` 0x005749C0, which classify the anchor overlay into the NS or EW
/// band, walk back up to two cells to the canonical edge anchor, and call
/// the matching `CollapseBridge_*`.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn dispatch_bridge_collapse_from_hut(
    sim: &mut Simulation,
    rules: &RuleSet,
    hut_center: (u16, u16),
) -> bool {
    dispatch_bridge_collapse_from_hut_with_overlay_registry(
        sim,
        rules,
        hut_center,
        None,
        crate::sim::world::FrameEffects::default(),
    )
}

pub(crate) fn dispatch_bridge_collapse_from_hut_with_overlay_registry(
    sim: &mut Simulation,
    rules: &RuleSet,
    hut_center: (u16, u16),
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let scan: Vec<(u16, u16)> = hut_destroy_5x5_scan(hut_center).collect();
    let family = choose_hut_bridge_family(sim, &scan);
    if sim.bridge_state.is_none() || sim.resolved_terrain.is_none() {
        return false;
    }
    let seed_axis = find_destroy_overlay_seed(&|x, y| bridge_overlay_at(sim, x, y), &scan, family);
    let mut host = BridgeDamageDrivers::new(sim, rules, overlay_registry, frame_effects);
    let fallback = if let Some((rx, ry, axis)) = seed_axis {
        run_hut_collapse_bounded(&mut host, family, axis, rx, ry);
        HutFallbackExecution::default()
    } else {
        run_hut_fallback(&mut host, family, hut_center)
    };
    let BridgeDamageDrivers { sim, collapsed, .. } = host;
    finish_hut_fallback(sim, rules, fallback, overlay_registry, frame_effects) || collapsed
}

/// Engineer WhatAction's complete Map587410 admission. Each ordinary input
/// producer gets an isolated fallback identity, so hover cadence cannot change
/// simulation state. The query still retains that identity across all nested
/// lookups; the live records and registered TMP metadata remain map-owned.
pub fn bridge_hut_can_repair(sim: &Simulation, hut_center: (u16, u16)) -> bool {
    let Some(terrain) = sim.resolved_terrain.as_ref() else {
        return false;
    };
    let records = sim
        .bridge_state
        .as_ref()
        .map_or(&[][..], |state| state.endpoint_records());
    crate::sim::bridge_state::repair_query::can_repair(
        &crate::map::resolved_terrain::NativeCellQuery::isolated(terrain),
        records,
        (hut_center.0 as i16, hut_center.1 as i16),
    )
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn hut_span_can_repair(terrain: &ResolvedTerrainGrid, hut_center: (u16, u16)) -> bool {
    crate::sim::bridge_state::repair_query::can_repair(
        &crate::map::resolved_terrain::NativeCellQuery::canonical(terrain),
        &[],
        (hut_center.0 as i16, hut_center.1 as i16),
    )
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
    overlay_at: &impl Fn(u16, u16) -> Option<u8>,
    scan: &[(u16, u16)],
    family: HutBridgeFamily,
) -> Option<(u16, u16, Axis)> {
    scan.iter().copied().find_map(|(rx, ry)| {
        let overlay = overlay_at(rx, ry)?;
        let axis = physical_span_axis_for_destroy_overlay(family, overlay)?;
        let seed = canonicalize_hut_destroy_seed(overlay_at, family, (rx, ry), axis)?;
        Some((seed.0, seed.1, axis))
    })
}

fn canonicalize_hut_destroy_seed(
    overlay_at: &impl Fn(u16, u16) -> Option<u8>,
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
        .and_then(|(rx, ry)| overlay_at(rx, ry))
        .is_some_and(|overlay| in_bridge_band(family, overlay));
    if !back_one_in_band {
        return step_axis(matched, probe_axis, 1);
    }
    let back_two_in_band = back_two
        .and_then(|(rx, ry)| overlay_at(rx, ry))
        .is_some_and(|overlay| in_bridge_band(family, overlay));
    if back_two_in_band {
        back_one
    } else {
        Some(matched)
    }
}

/// The CABHUT fallback's own cascade after its damage retries: the ramp's
/// rim refresh through the concrete 576770 (0x005745B4, 0x005751D0) and the
/// unconditional 56C510 (0x005745CC). Returns whether it asked for either.
fn finish_hut_fallback(
    sim: &mut Simulation,
    rules: &RuleSet,
    extra: HutFallbackExecution,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> bool {
    let rim_collapsed = extra.rim_cell.is_some_and(|ramp| {
        live_publication::update_adjacent_bridges(
            sim,
            rules,
            overlay_registry,
            ramp,
            HutBridgeFamily::High,
            frame_effects,
        )
    });
    if extra.zones_dirty {
        publish_bridge_navigation(sim, rules);
    }
    extra.zones_dirty || rim_collapsed
}

// Hard cap of the bounded walker: gamemd's `CollapseBridge_*_*` uses
// `local_2c = 4`. See `BRIDGE_COLLAPSE_CHAIN_MECHANISM_GHIDRA_REPORT.md` §4.
const MAX_HUT_SWEEP_STEPS: usize = 4;
const MAX_HUT_ATTEMPTS_PER_STEP: usize = 3;
const NORMALIZED_RNG_MAX_INCLUSIVE: u32 = 0x7FFF_FFFE;
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
// Safety cap on the extent-measurement walk (Phase 1 of the bounded
// walker). gamemd has no explicit cap — the off-bridge band check
// terminates the walk — but a runaway count would only happen if the
// overlay band check were buggy. 64 cells is well beyond any realistic
// YR bridge length.
const MAX_EXTENT_PROBE: usize = 64;

/// The CABHUT fallback's own cascade requests.
#[derive(Debug, Default)]
struct HutFallbackExecution {
    /// RebuildZoneConnectivity (0x0056C510) after any anchor was found.
    zones_dirty: bool,
    /// UpdateAdjacentBridges_High (576770) at the ramp cell, which both
    /// twins call (0x005745B4, 0x005751D0).
    rim_cell: Option<(i16, i16)>,
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
    bridge_overlay_at(sim, rx, ry)
        .is_some_and(|overlay| ordinary::member(i32::from(overlay), HutBridgeFamily::Low))
        || sim
            .resolved_terrain
            .as_ref()
            .and_then(|terrain| terrain.cell(rx, ry))
            .is_some_and(|cell| cell.is_wood_bridge_repair_tile)
}

fn bridge_overlay_at(sim: &Simulation, rx: u16, ry: u16) -> Option<u8> {
    // Every bridge writer publishes CellClass+44 synchronously; hut searches
    // read that authority.
    sim.resolved_terrain
        .as_ref()
        .and_then(|terrain| terrain.cell(rx, ry))
        .and_then(|cell| cell.bridge_facts.overlay_id)
}

/// The CABHUT death fallback: the tail of
/// `MapClass::DestroyBridge_High_OnHutDeath` 0x00574000 and of its wooden twin
/// 0x00574C20, run when the 5x5 overlay scan finds no seed. On stock maps
/// that is every hut beside a bridgehead rather than over the span.
///
/// 1. Starter and anchor ([`ramp_repair::hut_anchor`], shared with the
///    repair). No starter: return without a zone rebuild (0x00574244).
/// 2. From the anchor, walk direction 6 (starter 0x800) or 0 through the
///    MapClass cell-array rectangle to the first allocated cell
///    [`HighBridgeRimTiles::is_ramp`] accepts, and call ApplyDamageToCell
///    there up to three times until one returns true. No ramp: only the zone
///    rebuild (0x00574465).
/// 3. Walk back from the ramp through live cells to the first one
///    [`HighBridgeRimTiles::is_end`] accepts. Unless its tile is two below the
///    tile base (0x00574551), call ApplyDamageToCell up to three times on the
///    cell one step back toward the ramp. Leaving the rectangle skips this.
/// 4. UpdateAdjacentBridges_High (576770) at the ramp (0x005745B4), then the
///    zone rebuild (0x005745CC), both in [`finish_bridge_damage`].
fn run_hut_fallback(
    host: &mut BridgeDamageDrivers<'_>,
    family: HutBridgeFamily,
    hut_center: (u16, u16),
) -> HutFallbackExecution {
    let center = (hut_center.0 as i16, hut_center.1 as i16);
    // Err is a 0x100 starter without 0x80 or +0x2C, which faults natively;
    // stamped structural cells always carry +0x2C.
    let Ok(Some((anchor, forward))) =
        ramp_repair::hut_anchor(&mut LiveCells::new(host.sim), center)
    else {
        return HutFallbackExecution::default();
    };
    let sim: &Simulation = host.sim;
    let terrain = sim
        .resolved_terrain
        .as_ref()
        .expect("bridge damage terrain");
    let tiles = hut_tile_keys(terrain, family);
    let bounds = NativeStartBounds::from_session(sim, terrain);
    let Some(ramp) = find_hut_fallback_ramp(terrain, bounds, tiles, anchor, forward) else {
        return HutFallbackExecution {
            zones_dirty: true,
            rim_cell: None,
        };
    };
    apply_hut_damage_retries(host, ramp);

    let terrain = host
        .sim
        .resolved_terrain
        .as_ref()
        .expect("bridge damage terrain");
    let back = (forward + 4) & 7;
    if let Some((end, tile)) = find_hut_fallback_end(terrain, bounds, tiles, ramp, back)
        && tile.wrapping_sub(tiles.base) != -2
    {
        apply_hut_damage_retries(host, rim::step(end, forward));
    }
    HutFallbackExecution {
        zones_dirty: true,
        rim_cell: Some(ramp),
    }
}

/// The live theater keys with the family's tileset base, absent on synthetic
/// grids without an active theater. Each wooden bridge helper reads
/// g_WoodBridgeSet_TileSetBase 0xABAD1C where its concrete twin reads
/// g_BridgeSet_TileSetBase 0xAA0E28.
fn family_rim_tiles(
    terrain: &ResolvedTerrainGrid,
    family: HutBridgeFamily,
) -> Option<HighBridgeRimTiles> {
    terrain
        .high_bridge_rim_tiles()
        .map(|_| hut_tile_keys(terrain, family))
}

/// The `Bridge*` theater keys both CABHUT tails compare, with the family's
/// base. Grids without an active theater read every key as -1.
fn hut_tile_keys(terrain: &ResolvedTerrainGrid, family: HutBridgeFamily) -> HighBridgeRimTiles {
    let mut keys = terrain
        .high_bridge_rim_tiles()
        .unwrap_or_else(|| HighBridgeRimTiles::from_ini(-1, &[]));
    if family == HutBridgeFamily::Low {
        keys.base = terrain.wood_bridge_set_base();
    }
    keys
}

/// The ramp walk (0x00574372..0x00574465). Unallocated cells are stepped
/// over without a lookup.
fn find_hut_fallback_ramp(
    terrain: &ResolvedTerrainGrid,
    bounds: NativeStartBounds,
    tiles: HighBridgeRimTiles,
    mut at: (i16, i16),
    forward: u8,
) -> Option<(i16, i16)> {
    while bounds.contains(at) {
        if let Some(index) = terrain.native_fixed_cell_index(at.0, at.1) {
            let cell = NativeCellIdentity::Real(index);
            if tiles.is_ramp(
                terrain.native_cell_tile_index(cell),
                terrain.native_cell_sub_tile(cell),
            ) {
                return Some(at);
            }
        }
        at = rim::step(at, forward);
    }
    None
}

/// The end walk (0x0057449E..0x0057454B): the end cell and its tile, or
/// `None` when the walk leaves the rectangle first.
fn find_hut_fallback_end(
    terrain: &ResolvedTerrainGrid,
    bounds: NativeStartBounds,
    tiles: HighBridgeRimTiles,
    mut at: (i16, i16),
    back: u8,
) -> Option<((i16, i16), i32)> {
    loop {
        at = rim::step(at, back);
        let cell = terrain.native_cell_identity(at);
        if !bounds.contains(at) {
            return None;
        }
        let tile = terrain.native_cell_tile_index(cell);
        if tiles.is_end(tile, terrain.native_cell_sub_tile(cell), back) {
            return Some((at, tile));
        }
    }
}

/// The fallback's ApplyDamageToCell at `target`, up to three times until it
/// returns true (`0x0057447A..0x00574490`, `0x00574591..0x005745A7`; the
/// wooden twin's `0x0057509F`, `0x005751B6`).
fn apply_hut_damage_retries(host: &mut BridgeDamageDrivers<'_>, target: (i16, i16)) {
    for _ in 0..MAX_HUT_ATTEMPTS_PER_STEP {
        if host.apply_damage_to_cell(target) {
            break;
        }
    }
}

fn physical_span_axis_for_destroy_overlay(family: HutBridgeFamily, overlay: u8) -> Option<Axis> {
    let walker_axis = ordinary::axis(i32::from(overlay), family)?;
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
///    axial steps, call the shared concrete or wooden damage primitive
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
    host: &mut BridgeDamageDrivers<'_>,
    family: HutBridgeFamily,
    axis: Axis,
    seed_rx: u16,
    seed_ry: u16,
) {
    let seed = (seed_rx, seed_ry);
    let back = measure_extent(host.sim, family, seed, axis, -1);
    let fwd = measure_extent(host.sim, family, seed, axis, 1);
    let step: i16 = if fwd < back { -1 } else { 1 };
    let bias = (back as i32 - fwd as i32) / 2;
    let Some(mut cur) = step_axis_by(seed, axis, -bias) else {
        return;
    };
    for _ in 0..MAX_HUT_SWEEP_STEPS {
        spawn_hut_walker_pre_destroy_effects(host, family, axis, cur);
        let direct = match family {
            HutBridgeFamily::Low => DispatchPath::LowDirect,
            HutBridgeFamily::High => DispatchPath::HighDirect,
        };
        for _ in 0..MAX_HUT_ATTEMPTS_PER_STEP {
            if host.run((cur.0 as i16, cur.1 as i16), direct) {
                break;
            }
        }
        let Some(next) = step_axis(cur, axis, step) else {
            break;
        };
        let Some(overlay) = bridge_overlay_at(host.sim, next.0, next.1) else {
            break;
        };
        if !in_bridge_band(family, overlay) {
            break;
        }
        cur = next;
    }
}

fn spawn_hut_walker_pre_destroy_effects(
    host: &mut BridgeDamageDrivers<'_>,
    family: HutBridgeFamily,
    physical_axis: Axis,
    center: (u16, u16),
) {
    if host.sim.bridge_explosions.is_empty() {
        return;
    }
    let Some(overlay) = bridge_overlay_at(host.sim, center.0, center.1) else {
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
            //57538A/5756AC (wood) and5759EC/575D1E (concrete) use
            // ground Level*104, without a deck offset.
            let z = host
                .sim
                .resolved_terrain
                .as_ref()
                .unwrap()
                .cell(rx, ry)
                .map(|c| c.level)
                .unwrap_or(0);
            let rules = host.rules;
            spawn_walker_bridge_explosion(host.sim, rules, rx, ry, z);
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
    sim: &Simulation,
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
        let Some(overlay) = bridge_overlay_at(sim, next.0, next.1) else {
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
    ordinary::member(i32::from(overlay), family)
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

/// Complete ground receivers before the deck pass and debris RNG.
pub(super) fn blow_up_bridge_cell_fallout(
    sim: &mut Simulation,
    rules: &RuleSet,
    rx: u16,
    ry: u16,
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    kill_ground_occupants_at(sim, rules, rx, ry, overlay_registry, frame_effects);
    drop_in_bridge_deck_entities(
        sim,
        rx,
        ry,
        super::UninitContext::new(Some(rules), overlay_registry).with_effects(frame_effects),
    );
    //47DDD5..47DE2B publishes after ground/deck receivers, before debris
    //draws/constructors. Direct callers also leave pending entries for save;
    //only Map587180 owns the clear/consume bracket.
    sim.pending_bridge_collapses.append((rx as i16, ry as i16));
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
    overlay_registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) {
    ground_fallout::apply(sim, rules, overlay_registry, rx, ry, frame_effects);
}

/// MapClass::InvalidateBridgeZones (`0x0056DAE0`) on the live record owner.
/// Returns whether a record deactivated (the native RebuildZoneConnectivity
/// `0x0056C510` request).
pub(crate) fn invalidate_bridge_zones(sim: &mut Simulation, query: (i16, i16)) -> bool {
    let (Some(bridges), Some(terrain)) = (sim.bridge_state.as_mut(), sim.resolved_terrain.as_ref())
    else {
        return false;
    };
    bridges.invalidate_bridge_zones(terrain, query)
}

/// Zone graph refresh after a collapse or CABHUT fallback. The endpoint
/// records' active bytes are already current: collapses cleared theirs through
/// [`invalidate_bridge_zones`], repairs set theirs through56DB70.
///
/// VERA-internal projection ownership, gamemd equivalent UNCHECKED. It stands
/// in for the native per-cell Recalc, RemoveBridgeZoneEdges (`0x00584E50`) and
/// RebuildZoneConnectivity (`0x0056C510`), and runs even when no record
/// changed. It publishes the canonical path as well as zones through the
/// shared structure/terrain projection, so unrelated foundations survive
/// bridge changes before the next reader. Native zone-tail ordering:
/// BRIDGE_COLLAPSE_FALLOUT_ORDERING_GHIDRA_REPORT.md in
/// docs/research/bridges/05-damage-collapse-repair-cabhut/, section
/// "Zone/path invalidation".
pub(crate) fn publish_bridge_navigation(sim: &mut Simulation, rules: &RuleSet) {
    let _ = sim.rebuild_dynamic_navigation(rules);
}

/// CellClass::BlowUpBridge47DD70 after its ground/deck receiver passes.
/// Constructor/Start completes before the sibling explosion's delay/index
/// draws. Native comparisons: tools/spatial_oracle/bridge_debris_producer.
fn spawn_bridge_debris(sim: &mut Simulation, rules: &RuleSet, cells: &BTreeSet<(u16, u16)>) {
    let explosion_count = sim.bridge_explosions.len() as u32;
    let metallic_count = sim.metallic_debris.len() as u32;
    if explosion_count == 0 {
        return;
    }
    for &(rx, ry) in cells {
        let outer_draw = sim
            .bridge_rng()
            .next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
        if outer_draw >= BRIDGE_DEBRIS_OUTER_GATE_EXCLUSIVE {
            continue;
        }
        // 47DE78..47DEB6 reads signed Cell.Level and adds416 unconditionally,
        // including after SetBridgeDirection has removed structural flags.
        let level = sim
            .resolved_terrain
            .as_ref()
            .map(|terrain| terrain.collapse_animation_level(rx as i16, ry as i16))
            .unwrap_or(0);
        let z = i32::from(level) * LEPTONS_PER_LEVEL as i32 + BRIDGE_DECK_HEIGHT_LEPTONS;
        let coord = bridge_jittered_coord(sim.bridge_rng(), (rx as i16, ry as i16), z);
        let metallic_draw = sim
            .bridge_rng()
            .next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
        if metallic_draw < BRIDGE_METALLIC_GATE_EXCLUSIVE && metallic_count > 0 {
            let slot = sim.bridge_rng().next_range_u32(metallic_count) as usize;
            let spawn = bridge_anim_spawn(sim.metallic_debris[slot], coord, 0);
            construct_bridge_anim(sim, rules, spawn);
        }
        // Native does not guard an empty MetallicDebris vector before indexing.
        // Rust safely skips that invalid-list spawn; no successful native
        // construction equivalence is claimed for this authored input.
        let delay = sim.bridge_rng().next_range_u32_inclusive(1, 5) as u16;
        let slot = sim.bridge_rng().next_range_u32(explosion_count) as usize;
        let spawn = bridge_anim_spawn(sim.bridge_explosions[slot], coord, delay);
        construct_bridge_anim(sim, rules, spawn);
    }
}

const BRIDGE_ANIM_DRAW_FLAGS: u32 = 0x600;

struct BridgeAnimSpawn {
    descriptor: crate::sim::components::AnimClassSpawnDescriptor,
    coord: AnimWorldCoord,
}

/// Both native bridge producers pass loop1/flags600/ZAdjust0/reverse0.
/// Exact world coordinates remain authoritative; descriptor cell fields are
/// only compatibility projections and cannot represent negative Cell levels.
fn bridge_anim_spawn(type_name: InternedId, coord: AnimWorldCoord, delay: u16) -> BridgeAnimSpawn {
    let (rx, ry, sub_x, sub_y, level) = coord.to_cell_sub_z();
    BridgeAnimSpawn {
        descriptor: crate::sim::components::AnimClassSpawnDescriptor {
            delay,
            loop_count: 1,
            draw_flags: BRIDGE_ANIM_DRAW_FLAGS,
            z_adjust: 0,
            reverse: false,
            ..crate::sim::components::AnimClassSpawnDescriptor::new(
                type_name, rx, ry, sub_x, sub_y, level,
            )
        },
        coord,
    }
}

fn construct_bridge_anim(sim: &mut Simulation, rules: &RuleSet, spawn: BridgeAnimSpawn) {
    if let Err(error) = sim.spawn_anim_at_world(rules, spawn.descriptor, spawn.coord) {
        // The loader binds both resolved lists, including registered types
        // without ART such as retail's truncated final MetallicDebris token D.
        log::warn!("bridge anim did not construct: {error}");
    }
}

/// One walker explosion: two jitter draws, the `RandomRanged(1, 5)` start
/// delay, then the slot, in that order (`0x00575540`, `0x00575BA0`).
///
/// Construct before the next cell's draws and before the live damage receiver:
/// wood575481/5757A3 and the concrete575870/575BA0 walkers call421EA0
/// inside the three-cell loop, ahead of57BAA0/57CCF0. Constructor draws and native IDs
/// therefore precede those of any dying occupant.
fn spawn_walker_bridge_explosion(sim: &mut Simulation, rules: &RuleSet, rx: u16, ry: u16, z: u8) {
    if sim.bridge_explosions.is_empty() {
        return;
    }
    let coord = bridge_jittered_coord(
        &mut sim.scenario_rng,
        (rx as i16, ry as i16),
        i32::from(z as i8) * LEPTONS_PER_LEVEL as i32,
    );
    let delay = sim.scenario_rng.next_range_u32_inclusive(1, 5) as u16;
    let slot = sim
        .scenario_rng
        .next_range_u32(sim.bridge_explosions.len() as u32) as usize;
    construct_bridge_anim(
        sim,
        rules,
        bridge_anim_spawn(sim.bridge_explosions[slot], coord, delay),
    );
}

fn bridge_jittered_coord(rng: &mut SimRng, cell: (i16, i16), z: i32) -> AnimWorldCoord {
    let x_draw = rng.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
    let y_draw = rng.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
    AnimWorldCoord {
        x: bridge_jittered_axis(cell.0, x_draw),
        y: bridge_jittered_axis(cell.1, y_draw),
        z,
    }
}

/// 47DECF..47DEE9 and47DF0D..47DF27 truncate the final absolute coordinate,
/// not the relative jitter. This matters at negative Cell coordinates. Reuse
/// the deterministic native arithmetic owner for the chained multiply/sub/add;
/// a host floating-point expression can change the emitted flight coordinate.
fn bridge_jittered_axis(cell: i16, draw: u32) -> i32 {
    let scale = X87Chop53::load_f64(NativeF64Bits::from_bits(0x3e00_0000_0040_0000))
        .expect("finite native normalized random scale");
    let half = X87Chop53::load_f64(NativeF64Bits::from_bits(0x3fe0_0000_0000_0000))
        .expect("finite native one-half");
    let normalized = X87Chop53::mul(X87Chop53::load_i32(draw as i32), scale);
    let offset = X87Chop53::mul(X87Chop53::sub(normalized, half), X87Chop53::load_i32(50));
    let center = i32::from(cell) * 256 + 128;
    X87Chop53::ftol_i32_low_masked(X87Chop53::add(offset, X87Chop53::load_i32(center)))
}

/// BlowUpBridge's deck pass (0x0047DDBA..0x0047DDC9): read the selected
/// head after ground callbacks, capture next BEFORE DropIn removes current.
/// Membership owns selection, including retained corpses and non-anchor cells.
fn drop_in_bridge_deck_entities(
    sim: &mut Simulation,
    rx: u16,
    ry: u16,
    context: super::UninitContext<'_>,
) {
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
        sim.drop_as_bomb(id, context);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::bridge_facts::{BRIDGE_FLAG_DESTROYED_OR_RAMP, BRIDGE_FLAG_DIRECTION_ZERO};
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid};
    use crate::rules::locomotor_type::LocomotorKind;
    use crate::sim::combat::Health;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::intern::test_intern;
    use crate::sim::movement::locomotor::{LocomotorState, MovementLayer};
    use crate::sim::occupancy::CellListInsertion;

    /// Write CellClass+44 on each listed cell.
    pub(super) fn seed_bridge_overlay(
        terrain: &mut ResolvedTerrainGrid,
        cells: &[(u16, u16)],
        overlay: u8,
    ) {
        for &(x, y) in cells {
            terrain.cell_mut(x, y).unwrap().bridge_facts.overlay_id = Some(overlay);
        }
    }

    /// A hut-scan overlay reader over the listed CellClass+44 identities.
    fn overlay_lookup(cells: &[((u16, u16), u8)]) -> impl Fn(u16, u16) -> Option<u8> + '_ {
        move |x, y| {
            cells
                .iter()
                .find(|(cell, _)| *cell == (x, y))
                .map(|(_, overlay)| *overlay)
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
                    is_water: is_bridge,
                    ground_walk_blocked: is_bridge,
                    has_bridge_deck: is_bridge,
                    bridge_walkable: is_bridge,
                    bridge_transition: is_bridge,
                    bridge_deck_level: if is_bridge { deck_level } else { 0 },
                    ..crate::map::resolved_terrain::test_flat_cell(x, y)
                });
            }
        }
        ResolvedTerrainGrid::from_cells(6, 6, cells)
    }

    /// Build a Drive locomotor on the Bridge layer (mimics `high=true` spawn).
    fn drive_loco_on_bridge() -> LocomotorState {
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Drive);
        loco.set_layer(MovementLayer::Bridge);
        loco
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
            Health::new(256),
            test_intern("MTNK"),
            crate::map::entities::EntityCategory::Unit,
            0,
            5,
            true,
        );
        entity.set_on_bridge(true);
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.lifecycle.set_cell_marked(true);
        entity.locomotor = Some(drive_loco_on_bridge());
        sim.substrate.entities.insert(entity);
        1
    }

    /// DropAsBomb5F4160 preserves Location and orders while arming the
    /// shared ObjectAI fall. The receiver kills the object only on landing.
    #[test]
    fn bridge_drop_arms_a_bomb_without_snapping_or_clearing_its_order() {
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(water_below_bridge_terrain(3));
        let id = spawn_deck_unit(&mut sim);
        let order = Some(crate::sim::components::NavTargetRef::cell(7, 5));
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .navigation
            .set_nav_com_for_test(order);
        sim.substrate.occupancy.add(
            5,
            5,
            id,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        drop_in_bridge_deck_entities(&mut sim, 5, 5, super::super::UninitContext::default());

        let e = sim
            .substrate
            .entities
            .get(id)
            .expect("deck entity remains until the landing receiver");
        assert_eq!(e.position.z(), 3, "Location is retained");
        assert!(e.was_dropped_as_bomb() && e.is_falling_down());
        assert!(!e.on_bridge(), "OnBridge cleared by DropIn");
        assert_eq!(
            e.navigation.nav_com(),
            order,
            "DropAsBomb does not write orders"
        );
        assert_eq!(e.health.current(), 256, "DropIn never harms — no damage");
        let loco = e.locomotor.as_ref().expect("locomotor");
        assert_eq!(
            loco.layer(),
            MovementLayer::Bridge,
            "DropAsBomb does not write locomotor storage; live list queries use OnBridge"
        );
        let cell = sim
            .substrate
            .occupancy
            .get(5, 5)
            .expect("occupancy retained");
        assert_eq!(cell.count_on(MovementLayer::Ground), 1);
        assert_eq!(cell.count_on(MovementLayer::Bridge), 0);
    }

    /// DropIn (`0x005F4160`) writes no Location: an object falling onto the
    /// deck keeps its Z when the deck goes, and falls on to the ground.
    #[test]
    fn drop_in_keeps_a_falling_objects_z_and_it_falls_to_the_ground() {
        let mut sim = Simulation::new();
        sim.resolved_terrain = Some(water_below_bridge_terrain(3));
        let id = spawn_deck_unit(&mut sim);
        let entity = sim.substrate.entities.get_mut(id).unwrap();
        entity.position.set_exact_z_leptons_for_test(Some(1000));
        entity.set_falling_down_for_test(true);
        sim.substrate.occupancy.add(
            5,
            5,
            id,
            MovementLayer::Bridge,
            None,
            CellListInsertion::PrependNonBuilding,
        );

        drop_in_bridge_deck_entities(&mut sim, 5, 5, super::super::UninitContext::default());

        let e = sim.substrate.entities.get(id).unwrap();
        assert!(!e.on_bridge());
        assert!(e.is_falling_down());
        assert_eq!(e.position.exact_z_leptons(), Some(1000));
        let mut frames = 0;
        while !sim.advance_fall(
            id,
            -8,
            None,
            None,
            crate::sim::world::FrameEffects::default(),
        ) {
            frames += 1;
            assert!(frames < 200, "the fall reaches the ground");
        }
        assert_eq!(
            sim.substrate
                .entities
                .get(id)
                .unwrap()
                .position
                .exact_z_leptons(),
            Some(0)
        );
    }

    /// Bound synthetic explosion types for the hut-walker fixture.
    fn bridge_explosion_rules() -> crate::rules::ruleset::RuleSet {
        let ini = crate::rules::ini_parser::IniFile::from_str("");
        let mut rules = crate::rules::ruleset::RuleSet::from_ini(&ini).expect("rules parse");
        let mut art = crate::rules::art_data::ArtRegistry::from_ini(
            &crate::rules::ini_parser::IniFile::from_str(
                "[BRIDGEEXP1]\nTranslucent=yes\nReport=Explosion06\n\
                 [BRIDGEEXP2]\nTranslucent=yes\nReport=Explosion07\n",
            ),
        );
        art.bind_anim_frame_count_for_test("BRIDGEEXP1", 8);
        art.bind_anim_frame_count_for_test("BRIDGEEXP2", 8);
        rules.replace_art_registry_for_test(art);
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
        let high_ew_range = [((5, 4), 0xCD), ((5, 5), 0xCD)];
        assert_eq!(
            find_destroy_overlay_seed(
                &overlay_lookup(&high_ew_range),
                &[(5, 5)],
                HutBridgeFamily::High
            ),
            Some((5, 5, Axis::EW)),
            "0xCD high range dispatches to CollapseBridge_EW_High, so the hut sweep must step X"
        );

        let high_ns_range = [((4, 5), 0xD6), ((5, 5), 0xD6)];
        assert_eq!(
            find_destroy_overlay_seed(
                &overlay_lookup(&high_ns_range),
                &[(5, 5)],
                HutBridgeFamily::High
            ),
            Some((5, 5, Axis::NS)),
            "0xD6 high range dispatches to CollapseBridge_NS_High, so the hut sweep must step Y"
        );

        let low_ew_range = [((5, 4), 0x4A), ((5, 5), 0x4A)];
        assert_eq!(
            find_destroy_overlay_seed(
                &overlay_lookup(&low_ew_range),
                &[(5, 5)],
                HutBridgeFamily::Low
            ),
            Some((5, 5, Axis::EW)),
            "0x4A low range dispatches to CollapseBridge_EW_Low, so the hut sweep must step X"
        );

        let low_ns_range = [((4, 5), 0x53), ((5, 5), 0x53)];
        assert_eq!(
            find_destroy_overlay_seed(
                &overlay_lookup(&low_ns_range),
                &[(5, 5)],
                HutBridgeFamily::Low
            ),
            Some((5, 5, Axis::NS)),
            "0x53 low range dispatches to CollapseBridge_NS_Low, so the hut sweep must step Y"
        );
    }

    #[test]
    fn cabhut_seed_canonicalization_shifts_edge_hit_forward() {
        let state = [((5, 5), 0xCD)];

        assert_eq!(
            find_destroy_overlay_seed(&overlay_lookup(&state), &[(5, 5)], HutBridgeFamily::High),
            Some((5, 6, Axis::EW)),
            "when no back lane is in the bridge band, DestroyBridgeFromCell shifts one cell forward"
        );
    }

    #[test]
    fn cabhut_seed_canonicalization_keeps_middle_hit() {
        let state = [((5, 4), 0xCD), ((5, 5), 0xCD)];

        assert_eq!(
            find_destroy_overlay_seed(&overlay_lookup(&state), &[(5, 5)], HutBridgeFamily::High),
            Some((5, 5, Axis::EW)),
            "one in-band back lane and one off-band second back lane keeps the matched cell"
        );
    }

    #[test]
    fn cabhut_seed_canonicalization_shifts_two_cells_in_backward() {
        let state = [((5, 3), 0xCD), ((5, 4), 0xCD), ((5, 5), 0xCD)];

        assert_eq!(
            find_destroy_overlay_seed(&overlay_lookup(&state), &[(5, 5)], HutBridgeFamily::High),
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
        let scan: Vec<(u16, u16)> = hut_destroy_5x5_scan((10, 10)).collect();
        let state = [((9, 8), 0xCD), ((8, 12), 0xCD)];

        assert_eq!(
            find_destroy_overlay_seed(&overlay_lookup(&state), &scan, HutBridgeFamily::High),
            Some((8, 13, Axis::EW)),
            "Y-major scan would find (9,8) first; gamemd hut death finds the earlier X column"
        );
    }

    /// The hut-collapse walk creates its pre-destroy presentation effects from
    /// Scenario only. A non-terminal one-step fixture makes the receiver real
    /// without adding BlowUpBridge fallout draws after the walker effects.
    #[test]
    fn hut_collapse_walker_presentation_uses_scenario_only() {
        let mut sim = Simulation::new();
        let seed = 0x48A7_51DE_u64;
        sim.initialize_random_number_system(seed as u32);
        let mut terrain = water_below_bridge_terrain(3);
        seed_bridge_overlay(&mut terrain, &[(4, 3), (4, 4)], 0xE0);
        sim.resolved_terrain = Some(terrain);
        sim.bridge_state = Some(crate::sim::bridge_state::BridgeRuntimeState::default());
        let bridge_explosion = sim.interner.intern("BRIDGEEXP1");
        sim.bridge_explosions.push(bridge_explosion);

        // The X-major hut scan first sees (4,3), then canonicalizes to (4,4).
        // 0xE0 is a non-terminal presentation overlay whose destruction
        // transition is NoChange, while the missing next EW cell bounds the
        // physical walker to exactly one step with no fallout outcome.

        let before = sim.rng_state();
        let mut predicted = crate::sim::rng::SimRng::new(seed);
        for _ in 0..3 {
            predicted.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
            predicted.next_range_u32_inclusive(0, NORMALIZED_RNG_MAX_INCLUSIVE);
            predicted.next_range_u32_inclusive(1, 5);
            predicted.next_range_u32(1);
        }

        let rules = bridge_explosion_rules();
        let collapsed = dispatch_bridge_collapse_from_hut(&mut sim, &rules, (4, 4));

        assert!(
            !collapsed,
            "intermediate-only fixture must not add BlowUpBridge fallout draws"
        );
        assert_eq!(
            sim.resolved_terrain
                .as_ref()
                .and_then(|terrain| terrain.cell(4, 4))
                .and_then(|cell| cell.bridge_facts.overlay_id),
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
    /// lockstep on every multi-match cell. `damage_dispatch::run` is used directly
    /// so only the per-block gate draws are measured (the debris/explosion
    /// draws happen later in the cascade).
    #[test]
    fn dispatcher_in_band_cell_consumes_two_block_strength_draws() {
        let mut sim = Simulation::new();
        let seed = 0x0B11_D6E5_u64;
        sim.initialize_random_number_system(seed as u32);
        let mut terrain = water_below_bridge_terrain(4);
        // A raw overlay alone admits only D. A also needs a real concrete
        // Middle tile class (without0x100 its height gate is bypassed).
        terrain.cell_mut(5, 5).unwrap().final_tile_index = 1019;
        terrain.test_set_high_bridge_rim_tiles(
            crate::map::bridge_rim_tiles::HighBridgeRimTiles::from_ini(
                1000,
                b"[General]\nBridgeMiddle1=20\nBridgeMiddle2=40\n",
            ),
        );
        seed_bridge_overlay(&mut terrain, &[(5, 5)], 0xCD);
        sim.resolved_terrain = Some(terrain);
        sim.bridge_state = Some(BridgeRuntimeState::default());

        let bridge_strength = 1500i32;
        // Predict exactly two BridgeStrength gate draws (block A + block D).
        let mut predicted = crate::sim::rng::SimRng::new(seed);
        predicted.next_range_u32_inclusive(1, bridge_strength as u32);
        predicted.next_range_u32_inclusive(1, bridge_strength as u32);

        let event = BridgeDamageEvent {
            rx: 5,
            ry: 5,
            // Reject both callbacks so this fixture isolates admission RNG.
            // Concrete callback outcomes are covered by the native corpus.
            damage: 0,
            warhead_ref: crate::sim::intern::InternedId::default(),
            is_ion_cannon: false,
            impact_z_leptons: 0,
        };
        let rules = bridge_explosion_rules();
        let _ = damage_dispatch::run(
            &mut sim,
            &[event],
            bridge_strength,
            &rules,
            None,
            crate::sim::world::FrameEffects::default(),
        );

        assert_eq!(
            sim.scenario_rng.state(),
            predicted.state(),
            "in-band high cell must consume exactly 2 BridgeStrength draws (block A + block D)"
        );
    }

    /// The fallback's starter and anchor (0x0057418A..0x00574355) through the
    /// live cells, for each starter kind, with the hut at (9, 10) and the
    /// starter three cells east.
    #[test]
    fn the_hut_fallback_anchor_follows_the_starter_flags() {
        let hut = (9, 10);
        let grid = |cells: &[((u16, u16), u32)]| {
            let mut terrain = crate::map::resolved_terrain::test_grid(
                20,
                20,
                crate::map::resolved_terrain::test_flat_cell,
            );
            for &((x, y), flags) in cells {
                terrain.cell_mut(x, y).unwrap().bridge_facts.raw_flags = flags;
            }
            terrain
        };
        let hut_fallback_anchor = |terrain: ResolvedTerrainGrid, hut| {
            let mut sim = Simulation::new();
            sim.resolved_terrain = Some(terrain);
            ramp_repair::hut_anchor(&mut LiveCells::new(&sim), hut)
                .expect("stamped structural starters carry +0x2C")
        };
        let ramp = BRIDGE_FLAG_DESTROYED_OR_RAMP;

        // 0x100 with 0x80 anchors on the starter; 0x800 walks direction 6.
        let terrain = grid(&[(
            (12, 10),
            BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_ANCHOR_SELF | BRIDGE_FLAG_DIRECTION_ZERO,
        )]);
        assert_eq!(hut_fallback_anchor(terrain, hut), Some(((12, 10), 6)));

        // 0x100 alone anchors on +0x2C.
        let mut terrain = grid(&[((12, 10), BRIDGE_FLAG_STRUCTURAL)]);
        let anchor = terrain.native_cell_identity((13, 12));
        terrain.cell_mut(12, 10).unwrap().bridge_facts.native_anchor = Some(anchor);
        assert_eq!(hut_fallback_anchor(terrain, hut), Some(((13, 12), 0)));

        // A pure 0x400 starter scans direction 2 over at most three more
        // 0x400 cells and anchors two cells back from the first without it.
        let terrain = grid(&[
            ((12, 10), ramp),
            ((13, 10), ramp),
            ((14, 10), ramp),
            ((15, 10), ramp),
        ]);
        assert_eq!(hut_fallback_anchor(terrain, hut), Some(((14, 10), 0)));
        let terrain = grid(&[
            ((12, 10), ramp),
            ((13, 10), ramp),
            ((14, 10), ramp),
            ((15, 10), ramp),
            ((16, 10), ramp),
        ]);
        assert_eq!(hut_fallback_anchor(terrain, hut), None);
        // With 0x800 it scans direction 4 and steps back along direction 0.
        let terrain = grid(&[((12, 10), ramp | BRIDGE_FLAG_DIRECTION_ZERO)]);
        assert_eq!(hut_fallback_anchor(terrain, hut), Some(((12, 9), 6)));

        // 0x80 and 0x800 alone start nothing.
        let terrain = grid(&[(
            (12, 10),
            BRIDGE_FLAG_ANCHOR_SELF | BRIDGE_FLAG_DIRECTION_ZERO,
        )]);
        assert_eq!(hut_fallback_anchor(terrain, hut), None);
    }

    /// The fallback's walks. The ramp walk (0x00574372..0x00574465) steps
    /// over unallocated cells without a lookup and stops at the first tile
    /// `is_ramp` accepts; the end walk (0x0057449E..0x0057454B) stops at the
    /// first tile `is_end` accepts in the back direction. Both give up where
    /// they leave the cell-array rectangle.
    #[test]
    fn the_hut_fallback_walks_stop_at_ramp_and_end_tiles() {
        let tiles = HighBridgeRimTiles::from_ini(
            100,
            b"[General]\nBridgeTopRight1=5\nBridgeBottomLeft1=7\n",
        );
        let mut terrain = crate::map::resolved_terrain::test_grid(
            20,
            20,
            crate::map::resolved_terrain::test_flat_cell,
        );
        // TopRight1 at sub-tile 12 is a ramp; BottomLeft1 at sub-tile 2 ends
        // a walk heading direction 4. Tiles are base + key - 1.
        for (cell, tile, sub_tile) in [((10, 4), 104, 12), ((10, 6), 104, 12), ((10, 9), 106, 2)] {
            let cell = terrain.cell_mut(cell.0, cell.1).unwrap();
            cell.final_tile_index = tile;
            cell.final_sub_tile = sub_tile;
        }
        let allocated: Vec<_> = (0..20)
            .flat_map(|y| (0..20).map(move |x| (x, y)))
            .filter(|&cell| cell != (10, 6))
            .collect();
        terrain.test_set_native_allocated_cells(&allocated);
        let bounds = NativeStartBounds {
            min_rx: 1,
            min_ry: 1,
            width: 18,
            height: 18,
        };

        assert_eq!(
            find_hut_fallback_ramp(&terrain, bounds, tiles, (10, 12), 0),
            Some((10, 4)),
            "the unallocated ramp tile at (10, 6) is stepped over"
        );
        assert_eq!(
            find_hut_fallback_ramp(&terrain, bounds, tiles, (3, 12), 0),
            None
        );
        assert_eq!(
            find_hut_fallback_end(&terrain, bounds, tiles, (10, 4), 4),
            Some(((10, 9), 106))
        );
        assert_eq!(
            find_hut_fallback_end(&terrain, bounds, tiles, (3, 4), 4),
            None
        );
    }

    /// Stock bridge-repair huts stand beside a bridgehead, where the 5x5
    /// overlay scan of 0x00574000 / 0x00574C20 finds no seed, so their death
    /// drops the bridge through the fallback walk: ApplyDamageToCell on the
    /// ramp it reaches. One concrete-family hut (Barrel) and one wooden-family
    /// hut (Carville): the anchor the walk starts from loses its structural
    /// 0x100 flag and turns 0x400.
    #[test]
    fn a_stock_bridgehead_hut_drops_its_bridge() {
        let Some((root, _)) = crate::rules::retail_ini_fixture::retail_assets() else {
            return;
        };
        for (map, hut, family) in [
            ("Barrel.mmx", (65, 54), HutBridgeFamily::High),
            ("Carville.mmx", (117, 107), HutBridgeFamily::Low),
        ] {
            let mut scene = crate::headless_scenario::load(&root, map, 0)
                .unwrap_or_else(|error| panic!("{map}: {error}"));
            let sim = &scene.runtime.simulation;
            assert!(
                sim.entities().values().any(|e| {
                    (e.position.rx(), e.position.ry()) == hut
                        && sim.resolve(e.type_ref()) == "CABHUT"
                }),
                "{map}: a CABHUT stands at {hut:?}"
            );
            let scan: Vec<_> = hut_destroy_5x5_scan(hut).collect();
            assert_eq!(choose_hut_bridge_family(sim, &scan), family, "{map}");
            assert_eq!(
                find_destroy_overlay_seed(&|x, y| bridge_overlay_at(sim, x, y), &scan, family),
                None,
                "{map}: the overlay scan finds no seed"
            );
            let (anchor, _) =
                ramp_repair::hut_anchor(&mut LiveCells::new(sim), (hut.0 as i16, hut.1 as i16))
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| panic!("{map}: a fallback anchor"));
            let flags = |sim: &Simulation| {
                let terrain = sim.resolved_terrain.as_ref().unwrap();
                terrain.native_cell_flags(terrain.native_cell_identity(anchor))
                    & (BRIDGE_FLAG_STRUCTURAL | BRIDGE_FLAG_DESTROYED_OR_RAMP)
            };
            assert_eq!(
                flags(sim),
                BRIDGE_FLAG_STRUCTURAL,
                "{map}: the bridge stands"
            );

            let crate::sim::runtime::SimRuntime {
                simulation,
                resources,
            } = &mut scene.runtime;
            dispatch_bridge_collapse_from_hut_with_overlay_registry(
                simulation,
                &resources.rules,
                hut,
                Some(&resources.overlay_registry),
                crate::sim::world::FrameEffects::default(),
            );
            assert_eq!(
                flags(simulation),
                BRIDGE_FLAG_DESTROYED_OR_RAMP,
                "{map}: the span falls"
            );
        }
    }

    /// Debris helper short-circuits when the required BridgeExplosion list is
    /// empty. MetallicDebris alone does not enable BlowUpBridge presentation.
    #[test]
    fn bridge_debris_requires_bridge_explosion_list() {
        let mut sim = Simulation::new();
        sim.initialize_random_number_system(7);
        let baseline_state = sim.scenario_rng.state();
        sim.resolved_terrain = Some(water_below_bridge_terrain(3));
        let metallic_debris = sim.interner.intern("METALDEB1");
        sim.metallic_debris.push(metallic_debris);
        let rules = bridge_explosion_rules();

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
            Health::new(256),
            test_intern("MTNK"),
            crate::map::entities::EntityCategory::Unit,
            0,
            5,
            true,
        );
        entity.set_on_bridge(false); // ground-layer occupant
        let mut loco = drive_loco_on_bridge();
        loco.set_layer(MovementLayer::Bridge);
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

        drop_in_bridge_deck_entities(&mut sim, 5, 5, super::super::UninitContext::default());

        // Ground entity untouched — still alive, still ground layer.
        let e = sim
            .substrate
            .entities
            .get(1)
            .expect("ground entity untouched");
        assert_eq!(e.health.current(), 256);
        assert!(!e.on_bridge());
        assert_eq!(e.locomotor.as_ref().unwrap().layer(), MovementLayer::Bridge);
        let cell = sim.substrate.occupancy.get(5, 5).expect("ground occupancy");
        assert_eq!(cell.count_on(MovementLayer::Ground), 1);
        assert_eq!(cell.count_on(MovementLayer::Bridge), 0);
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
            Health::new(256),
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
            Health::new(256),
            sim.interner.intern("ORCA"),
            crate::map::entities::EntityCategory::Aircraft,
            0,
            5,
            true,
        );
        let mut loco = drive_loco_on_bridge();
        loco.set_layer(MovementLayer::Air);
        air.locomotor = Some(loco);
        air.set_on_bridge(false);
        sim.substrate.entities.insert(air);

        // Consumed Engineers can retain positive HP after UnInit until the
        // deferred drain. The legacy coordinate scan must not restart their
        // death when it visits this stored identity.
        let retired = GameEntity::new_at_frame_zero_for_test(
            3,
            5,
            5,
            0,
            0,
            sim.interner.intern("Americans"),
            Health::new(100),
            sim.interner.intern("E1"),
            crate::map::entities::EntityCategory::Infantry,
            0,
            5,
            false,
        );
        sim.substrate.entities.insert(retired);
        sim.uninit(3);
        assert!(sim.substrate.entities.get(3).unwrap().health.current() > 0);

        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=256\nArmor=heavy\n[Warheads]\n0=Super\n[Super]\nInfDeath=1\n",
        )).unwrap();
        let ground = sim.substrate.entities.get_mut(1).unwrap();
        ground.lifecycle.set_in_limbo_for_test(false);
        ground.lifecycle.set_cell_marked(true);
        sim.substrate.occupancy.add(
            5,
            5,
            1,
            MovementLayer::Ground,
            None,
            crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
        );
        kill_ground_occupants_at(
            &mut sim,
            &rules,
            5,
            5,
            None,
            crate::sim::world::FrameEffects::default(),
        );

        let retired = sim.substrate.entities.get(3).unwrap();
        assert_eq!(
            retired.health.current(),
            100,
            "unlinked retired object is not a ground receiver"
        );
        assert!(!retired.lifecycle.object_alive());
        assert_eq!(sim.substrate.pending_delete.object_ids(), vec![3, 1]);

        let g = sim.substrate.entities.get(1).expect("ground unit present");
        assert_eq!(g.health.current(), 0, "ground occupant is force-killed");
        assert!(g.death.is_dying(), "ground occupant flagged dying");

        let a = sim.substrate.entities.get(2).expect("aircraft present");
        assert_eq!(
            a.health.current(),
            256,
            "aircraft overflying the collapse cell must NOT be killed"
        );
        assert!(!a.death.is_dying(), "aircraft not flagged dying");
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
                Health::new(256),
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
            unit.lifecycle.set_in_limbo_for_test(false);
            unit.lifecycle.set_cell_marked(true);
            sim.substrate.occupancy.add(
                5,
                5,
                id,
                MovementLayer::Ground,
                None,
                crate::sim::occupancy::CellListInsertion::PrependNonBuilding,
            );
        }
        kill_ground_occupants_at(
            &mut sim,
            &rules,
            5,
            5,
            None,
            crate::sim::world::FrameEffects::default(),
        );

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
            assert!(sim.substrate.entities.get(id).unwrap().death.is_dying());
        }
    }
}

#[cfg(test)]
#[path = "bridge_deck_tests.rs"]
mod deck_tests;
