//! Nearby-passable-cell search (engine `Find_Nearby_Passable_Cell`).
//!
//! Square (Chebyshev) ring expansion around a seed cell — concentric perimeters,
//! NOT a Manhattan diamond (see `ring_cells`); per-candidate passability (plus an
//! optional occupancy check that always SKIPS reservations); frame-counter
//! selection when no target cell is given, nearest-distance selection when a
//! target is given. It consumes no RNG stream. Exact CellClass projection misses
//! do stamp the process-shared dummy coordinate, so lookup order is deterministic,
//! future-affecting state; no other grid state is changed. Depends only on `map/`,
//! `sim/`, and `rules/`; never on render/ui/sidebar/audio/net.
//!
//! Determinism contract:
//! - The square-ring candidate ORDER is fully deterministic; it feeds both the
//!   frame-counter modulo index and the nearest-distance tie-break.
//! - Nearest-distance uses integer squared Euclidean distance (`dx*dx + dy*dy`)
//!   so the comparison stays fixed-point per the sim layering invariant.
//! - `frame_counter % pool.len()` reproduces the engine's same-tick aliasing by
//!   construction: two no-target calls on the same frame with the same candidate
//!   count return the same index. Do NOT add any per-call perturbation.

use crate::map::resolved_terrain::{CellClassProjectionView, ResolvedTerrainGrid};
use crate::rules::locomotor_type::{MovementZone, SpeedType};
use crate::sim::cell_rect::{
    CellRect, CellRectOccupancyContext, CellRectPassabilityContext,
    cell_is_in_playfield_height_aware_in_query, check_occupancy_rect,
    check_passability_rect_with_raw_occupation,
};
use crate::sim::entity_store::EntityStore;
use crate::sim::occupancy::{OccupancyGrid, RawCellOccupationGrid};
use crate::sim::overlay_grid::OverlayGrid;
use crate::sim::pathfinding::PathGrid;
use crate::sim::pathfinding::zone_map::{ZoneGrid, ZoneId};

/// Hard radius cap for the ring search.
///
/// The engine derives its own cap from the two map-rectangle scalars —
/// `min(mapRectWidth + mapRectHeight, 32)` — and that sum exceeds 32 on any
/// playable map, so the effective cap IS this constant there. It is NOT
/// `Speed + Sight`: that reading is refuted, and a caller that derives its radius
/// from a unit's speed and sight abandons the search around 21 rings early.
pub const RADIUS_HARD_CAP: u16 = 32;
/// Candidate-pool early-terminate count — the search stops collecting once it has
/// this many surviving candidates.
pub const MAX_CANDIDATES: usize = 24;

/// Convert the active MapClass `[Map] Size=` pair into FNPC's signed ring cap.
///
/// Native adds `MapClass+0xF4/+0xF8`, clamps only values above 32, and runs no
/// ring when the resulting signed value is non-positive. Keeping this conversion
/// beside the search prevents callers from reviving the refuted unit-owned
/// Speed+Sight radius.
// gamemd-derived: `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20`, radius
// block `0x0056DCE1..0x0056DD03`; all 99 static xrefs load `g_Map` as ECX.
pub(crate) const fn map_owned_radius_cap(size_width: i32, size_height: i32) -> u16 {
    let sum = size_width.wrapping_add(size_height);
    if sum <= 0 {
        0
    } else if sum > RADIUS_HARD_CAP as i32 {
        RADIUS_HARD_CAP
    } else {
        sum as u16
    }
}

/// Terrain-level delta the height gate admits, exclusive: a candidate passes when
/// `abs(seedLevel - bridgeRise - candidateLevel) < 2`. For an ordinary candidate
/// (`bridgeRise == 0`) that reads "at most one level from the seed"; for a bridge
/// candidate it does not — see [`candidate_height_ok`].
const MAX_SEED_LEVEL_DELTA_EXCLUSIVE: i16 = 2;
/// Levels a bridge deck sits above the ground cell that carries it. When the
/// candidate carries a bridge the height gate subtracts this from the SEED level —
/// not from the candidate's — so it does NOT normalize a deck to ground; the
/// arithmetic and what it actually admits are spelled out in [`candidate_height_ok`].
const BRIDGE_LEVEL_RISE: i16 = 4;

/// Candidate-cell origin plus centre (`0x80`) plus native's `0x600`-lepton
/// south-east projection reach.
const PROJECTION_START_LEPTONS: i32 = 0x680;
/// Native walks the projection ray back toward the candidate by eight leptons
/// before every CellClass lookup.
const PROJECTION_STEP_LEPTONS: i32 = 8;
/// One signed CellClass terrain level shifts both projected map axes by half a
/// cell (`0x80` leptons).
const PROJECTION_LEVEL_LEPTONS: i32 = 0x80;

/// The subset of the passability config the search always supplies the same way for
/// each candidate rectangle: `required_height_or_level = -1` (None) is fixed by the search.
/// Overlay rejection is NOT fixed here — it is a per-call caller argument and lives
/// in [`NearbySearchOptions`].
#[derive(Debug, Clone, Copy)]
pub struct PassabilityArgs {
    pub speed_type: SpeedType,
    pub required_zone_id: Option<ZoneId>,
    pub movement_zone: MovementZone,
    pub bridge_aware_zone: bool,
}

/// Top-left CellRect dimensions forwarded to both native FNPC validators.
///
/// This is intentionally not normalized: the verified active callers supply
/// positive dimensions, while `CellRect` owns native span semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NearbyFootprint {
    pub width: i32,
    pub height: i32,
}

impl NearbyFootprint {
    pub const SINGLE: Self = Self::new(1, 1);

    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

/// Ownership of FNPC's independent candidate-anchor playfield predicate.
///
/// Every active native candidate runs the height-aware MapClass diamond gate
/// before rectangle passability. Callers not yet parity-bound must name their
/// compatibility bypass explicitly; an exact query therefore cannot silently
/// lose the mandatory gate because a bounds `Option` was absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NearbyAnchorGate {
    /// Use this query's `playfield_bounds`; missing MapClass authority rejects
    /// every candidate through the native predicate instead of bypassing it.
    NativeHeightAware,
    UnverifiedCompatibilityBypass,
}

/// Per-call arguments the CALLER varies between otherwise identical searches.
///
/// The engine's free-unit placement is the worked example: it runs the same search
/// twice, and the only argument that differs between the two calls is overlay
/// rejection — the first pass refuses any cell carrying an overlay (so a fresh unit
/// is not dropped onto the ore field), the second pass accepts one. Defaulting to
/// "accept" keeps every caller that does not name the option on today's behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NearbySearchOptions {
    /// Reject any candidate cell that carries an overlay (ore, gems, walls).
    pub reject_any_overlay: bool,
}

/// FNPC query — mirrors the engine `Find_Nearby_Passable_Cell` caller args.
pub struct NearbyQuery<'a> {
    /// Optional input-owned fallback identity; bulk grids remain borrowed.
    pub native_cells: Option<&'a crate::map::resolved_terrain::NativeCellQuery<'a>>,
    /// Live CellClass occupation, including retained heads not in Cell lists.
    /// None preserves the older caller's explicit list-only projection.
    pub raw_occupation: Option<&'a RawCellOccupationGrid>,
    /// Per-candidate passability config.
    pub passability: PassabilityArgs,
    /// Top-left rectangle dimensions forwarded to passability and, when enabled,
    /// final occupancy.
    pub footprint: NearbyFootprint,
    /// Independent candidate-anchor gate that runs before rectangle passability.
    pub anchor_gate: NearbyAnchorGate,
    /// Bridge filter applied AFTER passability (an FNPC filter, not a passability arg).
    pub allow_bridge_cells: bool,
    /// Caller's terrain-level gate. When set, a candidate is admitted only while it
    /// stays within one level of the seed cell, with a four-level correction for a
    /// candidate that carries a bridge. Both free-unit placement attempts set it.
    pub check_height: bool,
    /// When set, each candidate also runs `check_occupancy_rect(.., reservation_arg = -1)`.
    pub check_occupancy: bool,
    /// Radius cap the caller supplies. The engine's own expression is
    /// `min(map-rect width + map-rect height, RADIUS_HARD_CAP)`, which is the cap
    /// itself on any playable map; this value is clamped to the cap internally.
    pub radius_cap: u16,
    /// `None` => frame-counter selection; `Some` => nearest-distance to target.
    pub target_cell: Option<(i32, i32)>,
    // Borrowed grids the per-candidate predicates read:
    pub path_grid: Option<&'a PathGrid>,
    pub resolved_terrain: Option<&'a ResolvedTerrainGrid>,
    pub overlay_grid: Option<&'a OverlayGrid>,
    pub occupancy: Option<&'a OccupancyGrid>,
    pub entities: Option<&'a EntityStore>,
    pub zone_grid: Option<&'a ZoneGrid>,
    /// The map's isometric playfield diamond ([Map] Size width + LocalSize), used
    /// by the exact candidate-anchor gate and the occupancy check's later corner
    /// test. Active native queries have no rectangular substitute when absent.
    pub playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
}

/// A surviving FNPC candidate. `direct` records only the ordinary collection-time
/// projection used for per-ring early-stop; bridge-aware collection deliberately
/// leaves it false, and final partition always projects again rather than reading it.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    cell: (i32, i32),
    direct: bool,
}

/// The two native semantic call sites for `FUN_006D6410`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProjectionUse {
    Collection,
    FinalPartition,
}

/// Engine `Find_Nearby_Passable_Cell`.
///
/// `frame_counter` MUST be the sim per-tick counter (`Simulation.session.binary_frame`),
/// read as the current frame — never an RNG draw. Returns `None` for the
/// no-candidate case (engine null-cell `{0,0}`), which the caller interprets as
/// "no cell": clear the destination, retry next tick.
pub fn find_nearby_passable_cell(
    seed: (i32, i32),
    q: &NearbyQuery<'_>,
    frame_counter: u32,
) -> Option<(u16, u16)> {
    find_nearby_passable_cell_with_options(seed, q, NearbySearchOptions::default(), frame_counter)
}

/// Engine `Find_Nearby_Passable_Cell` with the caller's per-call options spelled
/// out. [`find_nearby_passable_cell`] is this with the default options.
pub fn find_nearby_passable_cell_with_options(
    seed: (i32, i32),
    q: &NearbyQuery<'_>,
    options: NearbySearchOptions,
    frame_counter: u32,
) -> Option<(u16, u16)> {
    let mut project = |_site: ProjectionUse, cx: i32, cy: i32| is_direct_candidate(q, cx, cy);
    find_nearby_passable_cell_with_projection(seed, q, options, frame_counter, &mut project)
}

/// Internal seam that keeps projection ownership observable in focused tests.
/// Production always supplies [`is_direct_candidate`].
fn find_nearby_passable_cell_with_projection<F>(
    seed: (i32, i32),
    q: &NearbyQuery<'_>,
    options: NearbySearchOptions,
    frame_counter: u32,
    project: &mut F,
) -> Option<(u16, u16)>
where
    F: FnMut(ProjectionUse, i32, i32) -> bool,
{
    let candidates = collect_candidates_with_projection(seed, q, options, project);
    if candidates.is_empty() {
        return None;
    }

    // Native re-runs the projection while partitioning the stored candidates;
    // collection-time classification only owns the per-ring early-out.
    let mut direct_pool: Vec<&Candidate> = Vec::new();
    let mut indirect_pool: Vec<&Candidate> = Vec::new();
    for candidate in &candidates {
        if project(
            ProjectionUse::FinalPartition,
            candidate.cell.0,
            candidate.cell.1,
        ) {
            direct_pool.push(candidate);
        } else {
            indirect_pool.push(candidate);
        }
    }
    // Direct candidates are preferred; only fall back to indirects when there are
    // no directs at all.
    let pool = if direct_pool.is_empty() {
        indirect_pool
    } else {
        direct_pool
    };
    if pool.is_empty() {
        return None;
    }

    let chosen = match q.target_cell {
        // No target: deterministic frame-counter modulo over the preferred pool.
        None => pool[(frame_counter as usize) % pool.len()],
        // Target given: nearest by integer squared Euclidean distance; ties resolve
        // to the earlier ring-order candidate (stable, no frame/RNG input).
        Some((tx, ty)) => pool
            .iter()
            .copied()
            .min_by_key(|c| {
                let dx = (c.cell.0 - tx) as i64;
                let dy = (c.cell.1 - ty) as i64;
                dx * dx + dy * dy
            })
            .expect("pool is non-empty"),
    };

    cell_to_u16(chosen.cell)
}

/// Walk concentric square (Chebyshev-perimeter) rings outward from the seed,
/// collecting surviving candidates in the engine's fixed visit order, capping at
/// `MAX_CANDIDATES` and applying the per-ring early-out.
///
/// The outer loop runs `r = 0 .. cap` where `cap = min(caller radius_cap,
/// [`RADIUS_HARD_CAP`])`; the largest ring actually scanned is `cap - 1`. The engine
/// derives its own cap from a map-rectangle sum capped at the same constant, and that
/// sum exceeds it on any playable map, so the cap IS [`RADIUS_HARD_CAP`] there. It is
/// NOT `Speed + Sight` — that reading is refuted. Ring shape and order match the
/// engine exactly (see `ring_cells`).
///
/// Per-ring early-out: once ANY direct candidate has been accepted, the search
/// finishes the *current* ring and then STOPS scanning further rings — biasing the
/// result toward the nearest ring that yields a direct hit. The 24-candidate cap is
/// also honored mid-ring (the engine compares the running count to the same 24 after
/// every accept and jumps straight to selection on equality).
///
/// Arming the early-out is ASYMMETRIC with the pool split: in the bridge-aware zone the
/// engine skips the occlusion projection entirely, so *any* accepted candidate arms the
/// early-out there, while selection still re-runs the projection on every stored
/// candidate to split the pools. That is why bridge-aware collection stores no
/// projection result below.
#[cfg(test)]
fn collect_candidates(
    seed: (i32, i32),
    q: &NearbyQuery<'_>,
    options: NearbySearchOptions,
) -> Vec<Candidate> {
    let mut project = |_site: ProjectionUse, cx: i32, cy: i32| is_direct_candidate(q, cx, cy);
    collect_candidates_with_projection(seed, q, options, &mut project)
}

fn collect_candidates_with_projection<F>(
    seed: (i32, i32),
    q: &NearbyQuery<'_>,
    options: NearbySearchOptions,
    project: &mut F,
) -> Vec<Candidate>
where
    F: FnMut(ProjectionUse, i32, i32) -> bool,
{
    let mut out: Vec<Candidate> = Vec::new();
    let cap = q.radius_cap.min(RADIUS_HARD_CAP) as i32;
    let mut direct_found = false;
    //56DC6B..56DC92 always performs the seed lookup, even with height gate
    //disabled. The bridge-aware second lookup56DCA8..56DCDC adds four to
    //this retained level when the seed has structural bit100.
    let mut level = projection_cell_view(q, seed.0, seed.1).signed_level as i16;
    if q.passability.bridge_aware_zone
        && if q.resolved_terrain.is_some() {
            projection_cell_view(q, seed.0, seed.1).raw_flags_0x1180
                & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                != 0
        } else {
            candidate_is_bridge_cell(q, seed.0, seed.1)
        }
    {
        level = level.wrapping_add(BRIDGE_LEVEL_RISE);
    }
    let seed_level = q.check_height.then_some(level);

    let mut r = 0;
    while r < cap {
        for (cx, cy) in ring_cells(seed, r) {
            if !candidate_passes(q, options, seed_level, cx, cy) {
                continue;
            }
            // gamemd-derived: FNPC's bridge-aware collection branch skips
            // `FUN_006D6410` entirely; final partition below is a separate call
            // site and still projects every stored candidate.
            let direct = if q.passability.bridge_aware_zone {
                false
            } else {
                project(ProjectionUse::Collection, cx, cy)
            };
            direct_found |= q.passability.bridge_aware_zone || direct;
            out.push(Candidate {
                cell: (cx, cy),
                direct,
            });
            // The candidate cap is checked after every accept, mid-ring — the
            // engine stops the moment the 24th candidate lands.
            if out.len() >= MAX_CANDIDATES {
                return out;
            }
        }
        // Per-ring early-out: a direct hit anywhere so far finishes this ring then stops.
        if direct_found {
            return out;
        }
        r += 1;
    }
    out
}

/// Cells of square ring `r` (Chebyshev perimeter, `max(|dx|, |dy|) == r`) around
/// the seed `(ox, oy)`, in the engine's fixed 4-segment order:
///
/// 1. for `d = -r ..= r`: North cell `(ox + d, oy - r)` then South cell `(ox + d, oy + r)`
///    — the two full horizontal apex rows, scanned together W→E by `d`.
/// 2. for `e = 1-r ..= r-1`: West cell `(ox - r, oy + e)` then East cell `(ox + r, oy + e)`
///    — the two vertical side columns (interior rows only), scanned together N→S by `e`.
///
/// This is NOT a Manhattan diamond and NOT a continuous clockwise walk. At `r == 0`
/// segment 1 runs once with `d == 0`: North and South coincide on the seed, so the
/// engine emits the seed cell TWICE (segment 2's range `1..=-1` is empty). The
/// duplicate is intentional — it is the engine's actual candidate stream, and it
/// feeds the candidate count / frame-modulo index, so we reproduce it rather than
/// dedup.
fn ring_cells(seed: (i32, i32), r: i32) -> Vec<(i32, i32)> {
    let (ox, oy) = seed;
    let mut cells: Vec<(i32, i32)> = Vec::with_capacity((8 * r.max(1)) as usize);
    // Segment 1: North row then South row, for d = -r..=r (N then S per d).
    for d in -r..=r {
        cells.push((ox + d, oy - r)); // North
        cells.push((ox + d, oy + r)); // South (coincides with North at r == 0)
    }
    // Segment 2: West column then East column, interior rows e = 1-r..=r-1 (W then E per e).
    for e in (1 - r)..=(r - 1) {
        cells.push((ox - r, oy + e)); // West
        cells.push((ox + r, oy + e)); // East
    }
    cells
}

/// Engine "direct" classification — exact `FUN_006D6410` returned-cell test.
///
/// The native helper performs two independent candidate CellClass lookups, then
/// starts at candidate centre plus `0x600` leptons south-east and subtracts eight
/// leptons before every probe. This produces repeated far-to-near CellClass reads;
/// their lookup order and shared-dummy stamps are future-affecting behavior, not a
/// threshold-table optimization opportunity.
///
/// Candidate `CellClass+0x140 & 0x1000` gates a four-level addition when the same
/// probe view carries `CellClass+0x140 & 0x100`. The probe level is signed. Native
/// tests the projected axes before testing whether the current probe cell reached
/// the candidate; the returned cell is direct exactly when it equals the candidate.
// gamemd-derived: `FUN_006D6410 @ 0x006D6410`; candidate conversions/lookups
// `0x006D641B..0x006D648E`, far-to-near 8-lepton probe loop and signed level/flag
// correction `0x006D64B1..0x006D6513`, return decisions `0x006D6516..0x006D6588`.
fn is_direct_candidate(q: &NearbyQuery<'_>, cx: i32, cy: i32) -> bool {
    let candidate = crate::map::cell_index::packed_cell_coord(cx, cy);
    project_candidate(q, candidate.0, candidate.1) == candidate
}

fn project_candidate(q: &NearbyQuery<'_>, cx: i32, cy: i32) -> (i32, i32) {
    let (cx, cy) = crate::map::cell_index::packed_cell_coord(cx, cy);
    project_world_coordinate_with_lookup(cx * 256 + 128, cy * 256 + 128, |x, y| {
        projection_cell_view(q, x, y)
    })
}

fn projection_cell_view(q: &NearbyQuery<'_>, cx: i32, cy: i32) -> CellClassProjectionView {
    if let Some(cells) = q.native_cells {
        return cells.projection_view(cx, cy);
    }
    q.resolved_terrain.map_or_else(
        || CellClassProjectionView {
            // Compatibility-only callers without MapClass authority retain the
            // existing path-grid level facade and have no dummy/flag state to read.
            signed_level: i32::from(cell_level(q, cx, cy)),
            raw_flags_0x1180: 0,
        },
        |terrain| terrain.cellclass_projection_view(cx, cy),
    )
}

/// 6D641B..6D643D consumes world XY, divides toward zero, then narrows to
/// signed cell words. A negative cell's center can project from a different
/// seed: center(-1)=-128, whose /256 quotient is zero. Scatter and FNPC must
/// both pass through this conversion before the cell-based kernel.
pub(crate) fn project_world_coordinate_with_lookup<F>(x: i32, y: i32, lookup: F) -> (i32, i32)
where
    F: FnMut(i32, i32) -> CellClassProjectionView,
{
    project_candidate_with_lookup(native_lepton_to_cell(x), native_lepton_to_cell(y), lookup)
}

/// Instruction-faithful projection kernel after world-to-cell conversion.
/// The lookup closure represents one
/// native `MapClass::Get_CellClass` call and must return level and flags together.
fn project_candidate_with_lookup<F>(cx: i32, cy: i32, mut lookup: F) -> (i32, i32)
where
    F: FnMut(i32, i32) -> CellClassProjectionView,
{
    let (cx, cy) = crate::map::cell_index::packed_cell_coord(cx, cy);

    // Native looks up the candidate twice: first for +0x140, then for signed
    // +0x11B. Do not merge these calls; misses stamp the shared dummy twice.
    let candidate_is_forward_side =
        lookup(cx, cy).raw_flags_0x1180 & crate::map::bridge_facts::BRIDGE_FLAG_FORWARD_SIDE != 0;
    let candidate_level = lookup(cx, cy).signed_level;

    let mut probe_world_x = cx
        .wrapping_mul(crate::util::lepton::LEPTONS_PER_CELL_I32)
        .wrapping_add(PROJECTION_START_LEPTONS);
    let mut probe_world_y = cy
        .wrapping_mul(crate::util::lepton::LEPTONS_PER_CELL_I32)
        .wrapping_add(PROJECTION_START_LEPTONS);

    loop {
        probe_world_x = probe_world_x.wrapping_sub(PROJECTION_STEP_LEPTONS);
        probe_world_y = probe_world_y.wrapping_sub(PROJECTION_STEP_LEPTONS);
        let probe = (
            native_lepton_to_cell(probe_world_x),
            native_lepton_to_cell(probe_world_y),
        );
        let probe_view = lookup(probe.0, probe.1);
        let mut level_delta = probe_view.signed_level.wrapping_sub(candidate_level);
        if candidate_is_forward_side
            && probe_view.raw_flags_0x1180 & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0
        {
            level_delta = level_delta.wrapping_add(i32::from(BRIDGE_LEVEL_RISE));
        }

        let projection_shift = level_delta.wrapping_mul(PROJECTION_LEVEL_LEPTONS);
        let projected_x = native_lepton_to_cell(probe_world_x.wrapping_sub(projection_shift));
        let projected_y = native_lepton_to_cell(probe_world_y.wrapping_sub(projection_shift));

        if projected_x <= cx && projected_y <= cy {
            return probe;
        }
        if probe == (cx, cy) {
            return (cx, cy);
        }
    }
}

/// Native signed divide-by-256 conversion followed by packed-short truncation.
fn native_lepton_to_cell(leptons: i32) -> i32 {
    let adjusted = leptons.wrapping_add((leptons >> 31) & 0xff);
    (adjusted >> 8) as i16 as i32
}

/// Run the per-candidate predicates in engine order: the independent height-aware
/// anchor diamond, rectangle passability (`required_height_or_level = -1`,
/// caller-supplied overlay rejection), the caller's height gate, bridge filter,
/// then optional occupancy with reservations SKIPPED (`-1`).
// gamemd-derived: `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20`; anchor
// calls `0x0056DDC0/0x0056DFD6/0x0056E217/0x0056E419` dispatch to
// `MapClass::Is_Cell_In_Playfield_CellClass @ 0x00578540` immediately before
// `CellRect::CheckPassability @ 0x0056E7C0`.
fn candidate_passes(
    q: &NearbyQuery<'_>,
    options: NearbySearchOptions,
    seed_level: Option<i16>,
    cx: i32,
    cy: i32,
) -> bool {
    match q.anchor_gate {
        NearbyAnchorGate::NativeHeightAware => {
            if !cell_is_in_playfield_height_aware_in_query(
                (cx, cy),
                q.playfield_bounds,
                q.resolved_terrain,
                q.native_cells,
            ) {
                return false;
            }
        }
        NearbyAnchorGate::UnverifiedCompatibilityBypass => {}
    }

    let rect = CellRect::new(cx, cy, q.footprint.width, q.footprint.height);

    let passable = check_passability_rect_with_raw_occupation(
        CellRectPassabilityContext {
            native_cells: q.native_cells,
            rect,
            speed_type: q.passability.speed_type,
            // FNPC's entry turns a zone argument of 0xFFFF into -1, which
            // disables the comparison (`0x0056DC43..0x0056DC60`).
            required_zone_id: q
                .passability
                .required_zone_id
                .filter(|&zone| zone != 0xFFFF),
            movement_zone: q.passability.movement_zone,
            required_height_or_level: None, // the search always passes -1 (L21)
            bridge_aware_zone: q.passability.bridge_aware_zone,
            // Caller argument, forwarded verbatim: the engine's two free-unit attempts
            // differ in this one value and nothing else.
            reject_any_overlay: options.reject_any_overlay,
            path_grid: q.path_grid,
            resolved_terrain: q.resolved_terrain,
            overlay_grid: q.overlay_grid,
            occupancy: q.occupancy,
            zone_grid: q.zone_grid,
        },
        q.raw_occupation,
    );
    if !passable {
        return false;
    }

    if let Some(seed_level) = seed_level
        && !candidate_height_ok(q, seed_level, cx, cy)
    {
        return false;
    }

    //56DE6C bridge filter follows height; final occupancy56DE9D is last.
    if !q.allow_bridge_cells && candidate_is_bridge_cell(q, cx, cy) {
        return false;
    }

    !q.check_occupancy
        || check_occupancy_rect(CellRectOccupancyContext {
            native_cells: q.native_cells,
            rect,
            reservation_arg: -1,
            reservations: None,
            occupancy: q.occupancy,
            entities: q.entities,
            terrain_object_cells: None,
            resolved_terrain: q.resolved_terrain,
            overlay_grid: q.overlay_grid,
            playfield_bounds: q.playfield_bounds,
        })
}

/// The caller's height gate: `abs(seedLevel - bridgeRise - candidateLevel) < 2`,
/// where `bridgeRise` is [`BRIDGE_LEVEL_RISE`] when the candidate carries a bridge
/// and 0 otherwise.
///
/// For an ordinary candidate that is "stay within one level of the seed". For a
/// bridge candidate the rise comes off the SEED side of the subtraction, so a deck
/// does NOT compare as ground: a bridge candidate sitting at the seed's own level
/// reads as four levels away and is REJECTED, and the only bridge candidates the
/// gate admits are those three to five levels BELOW the seed. Direction and signum
/// arithmetic is a recurring bug class here — this describes the subtraction as
/// written, which is the audited engine expression; do not "fix" it toward the
/// intent the name suggests.
///
/// At both free-unit placement callsites the bridge term never decides anything:
/// those callsites also set `allow_bridge_cells = false`, so a bridge candidate is
/// dropped by the bridge filter whatever this gate says.
///
/// The seed is independently raised by four at56DCDC when the caller is
/// bridge-aware and that seed is structural; collection retains that value.
fn candidate_height_ok(q: &NearbyQuery<'_>, seed_level: i16, cx: i32, cy: i32) -> bool {
    let bridge_rise = if candidate_is_bridge_cell(q, cx, cy) {
        BRIDGE_LEVEL_RISE
    } else {
        0
    };
    let delta = seed_level
        .saturating_sub(bridge_rise)
        .saturating_sub(cell_level(q, cx, cy));
    delta.abs() < MAX_SEED_LEVEL_DELTA_EXCLUSIVE
}

/// Read the selected Cell's live level without another map lookup. Native
/// retains that identity across rectangle checks. PathGrid is only the
/// terrain-less compatibility fallback, never an override of a resident Cell.
fn cell_level(q: &NearbyQuery<'_>, cx: i32, cy: i32) -> i16 {
    if let Some(terrain) = q.resolved_terrain {
        return terrain
            .native_fixed_cell_index(cx as i16, cy as i16)
            .map_or_else(
                || {
                    i16::from(
                        q.native_cells
                            .map_or_else(|| terrain.shared_cell_dummy(), |cells| cells.dummy())
                            .snapshot()
                            .level,
                    )
                },
                |index| i16::from(terrain.cells()[index].level as i8),
            );
    }
    let (Ok(rx), Ok(ry)) = (u16::try_from(cx), u16::try_from(cy)) else {
        return 0;
    };
    q.path_grid
        .and_then(|grid| grid.cell(rx, ry))
        .map(|cell| cell.signed_level())
        .unwrap_or(0)
}

/// Read the retained real/dummy structural bit; do not merge a stale PathGrid.
fn candidate_is_bridge_cell(q: &NearbyQuery<'_>, cx: i32, cy: i32) -> bool {
    if let Some(terrain) = q.resolved_terrain {
        return terrain
            .native_fixed_cell_index(cx as i16, cy as i16)
            .map_or_else(
                || {
                    q.native_cells
                        .map_or_else(|| terrain.shared_cell_dummy(), |cells| cells.dummy())
                        .bridge_flags_0x1180()
                        & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL
                        != 0
                },
                |index| terrain.cells()[index].bridge_facts.has_structural_bridge(),
            );
    }
    let (Ok(rx), Ok(ry)) = (u16::try_from(cx), u16::try_from(cy)) else {
        return false;
    };
    q.path_grid
        .and_then(|g| g.cell(rx, ry))
        .is_some_and(|c| c.has_structural_bridge())
}

fn cell_to_u16(cell: (i32, i32)) -> Option<(u16, u16)> {
    match (u16::try_from(cell.0), u16::try_from(cell.1)) {
        (Ok(x), Ok(y)) => Some((x, y)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::bridge_facts::{
        BRIDGE_FLAG_FORWARD_SIDE, BRIDGE_FLAG_STRUCTURAL, BridgeCellFacts,
    };
    use crate::map::resolved_terrain::{ResolvedTerrainCell, ResolvedTerrainGrid, zone_class};
    use crate::rules::terrain_rules::{SpeedCostProfile, TerrainClass};

    fn terrain_cell(rx: u16, ry: u16) -> ResolvedTerrainCell {
        ResolvedTerrainCell {
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
            is_rough: false,
            is_road: false,
            accepts_smudge: false,
            allows_tiberium: false,
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
            base_land_type: 0,
            base_yr_cell_land_type: 0,
            base_terrain_class: TerrainClass::Clear,
            base_speed_costs: SpeedCostProfile::default(),
            build_blocked: false,
            has_bridge_deck: false,
            bridge_walkable: false,
            bridge_transition: false,
            bridge_deck_level: 0,
            bridge_layer: None,
            bridge_facts: BridgeCellFacts::default(),
            tube_index: None,
            radar_left: [0, 0, 0],
            radar_right: [0, 0, 0],
            has_damaged_data: false,
            bridgehead_anchor_class_at_load: None,
        }
    }

    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let cells = (0..height)
            .flat_map(|ry| (0..width).map(move |rx| terrain_cell(rx, ry)))
            .collect();
        ResolvedTerrainGrid::from_cells(width, height, cells)
    }

    /// A grid whose every cell sits at the same raised terrain level — a plateau. The
    /// occlusion test is relative, so this must behave exactly like `flat_terrain`.
    fn plateau_terrain(width: u16, height: u16, level: u8) -> ResolvedTerrainGrid {
        let mut terrain = flat_terrain(width, height);
        for cell in terrain.cells.iter_mut() {
            cell.level = level;
        }
        terrain
    }

    /// A grid that climbs one terrain level per south-east diagonal step, so EVERY cell
    /// is occluded by its immediate SE neighbour and no candidate is ever direct.
    fn diagonal_ramp_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let mut terrain = flat_terrain(width, height);
        for cell in terrain.cells.iter_mut() {
            cell.level = ((cell.rx as u32 + cell.ry as u32) / 2) as u8;
        }
        terrain
    }

    /// Classify one cell against a terrain grid, with a path grid built from it.
    fn direct_at(terrain: &ResolvedTerrainGrid, cell: (i32, i32)) -> bool {
        let path_grid = PathGrid::from_resolved_terrain(terrain);
        let q = base_query(terrain, &path_grid);
        is_direct_candidate(&q, cell.0, cell.1)
    }

    fn track_args() -> PassabilityArgs {
        PassabilityArgs {
            speed_type: SpeedType::Track,
            required_zone_id: None,
            movement_zone: MovementZone::Normal,
            bridge_aware_zone: false,
        }
    }

    fn base_query<'a>(
        terrain: &'a ResolvedTerrainGrid,
        path_grid: &'a PathGrid,
    ) -> NearbyQuery<'a> {
        NearbyQuery {
            native_cells: None,
            raw_occupation: None,
            passability: track_args(),
            footprint: NearbyFootprint::SINGLE,
            anchor_gate: NearbyAnchorGate::UnverifiedCompatibilityBypass,
            allow_bridge_cells: true,
            check_height: false,
            check_occupancy: false,
            radius_cap: RADIUS_HARD_CAP,
            target_cell: None,
            path_grid: Some(path_grid),
            resolved_terrain: Some(terrain),
            overlay_grid: None,
            occupancy: None,
            entities: None,
            zone_grid: None,
            playfield_bounds: None,
        }
    }

    #[test]
    fn nearby_raw_and_retained_seed_match_original_bounded_queries() {
        let cases: serde_json::Value = serde_json::from_str(include_str!(
            "../../tools/spatial_oracle/nearby_raw_occupation.json"
        ))
        .unwrap();
        let cases = cases.as_array().unwrap();
        assert_eq!(cases.len(), 8);
        for case in cases {
            let input = &case["input"];
            let output = &case["output"];
            let mut terrain = flat_terrain(21, 21);
            // Leave the cache at its initial level/flags to ensure the exact
            // query uses the live CellClass fields sampled by original code.
            let grid = PathGrid::from_resolved_terrain(&terrain);
            let cell = &mut terrain.cells[10 * 21 + 10];
            cell.level = input["level"].as_u64().unwrap() as u8;
            cell.bridge_facts.raw_flags = if input["structural"].as_bool().unwrap() {
                BRIDGE_FLAG_STRUCTURAL
            } else {
                0
            };
            terrain.shared_cell_dummy().stamp_coord(0, 0);
            let mut raw = RawCellOccupationGrid::default();
            raw.mark_ground(10, 10, input["ground"].as_u64().unwrap() as u8);
            raw.mark_deck(10, 10, input["deck"].as_u64().unwrap() as u8);
            if let Some(actions) = input["dummy_actions"].as_array() {
                for action in actions {
                    let c = &action["coord"];
                    crate::sim::movement::walk_head::raw_at(
                        &mut raw,
                        crate::sim::intern::InternedId::from_index(
                            action["owner"].as_u64().unwrap() as u32,
                        ),
                        crate::sim::components::DriveCoord {
                            x: c[0].as_i64().unwrap() as i32,
                            y: c[1].as_i64().unwrap() as i32,
                            z: c[2].as_i64().unwrap() as i32,
                        },
                        action["put"].as_bool().unwrap(),
                        Some(&terrain),
                        None,
                    );
                }
            }
            let seed = (
                input["seed"][0].as_i64().unwrap() as i32,
                input["seed"][1].as_i64().unwrap() as i32,
            );
            let mut q = base_query(&terrain, &grid);
            q.raw_occupation = Some(&raw);
            q.passability.speed_type = SpeedType::Foot;
            q.passability.bridge_aware_zone = input["bridge_aware"].as_bool().unwrap();
            q.check_height = input["height"].as_bool().unwrap();
            q.radius_cap = input["cap"].as_u64().unwrap() as u16;
            q.target_cell = Some(seed);
            // Match the native corpus's declared playfield/direct-projection
            // substitutions; the live query/rectangle/raw bodies stay shared.
            let mut projections = 0;
            let mut project = |_, _, _| {
                projections += 1;
                true
            };
            let result = find_nearby_passable_cell_with_projection(
                seed,
                &q,
                NearbySearchOptions::default(),
                0,
                &mut project,
            )
            .unwrap_or((0, 0));
            assert_eq!(
                serde_json::json!([result.0, result.1]),
                output["cell"],
                "{case}"
            );
            let dummy = terrain.shared_cell_dummy().snapshot().coord;
            assert_eq!(
                serde_json::json!([dummy.0, dummy.1]),
                output["dummy"],
                "{case}"
            );
            assert_eq!(
                projections,
                output["events"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|event| event.as_str() == Some("projection"))
                    .count(),
                "{case}"
            );
            if !output["dummy_raw"].is_null() {
                use crate::sim::{movement::locomotor::MovementLayer, occupancy::RawCellKey};
                for (index, layer) in [MovementLayer::Ground, MovementLayer::Bridge]
                    .into_iter()
                    .enumerate()
                {
                    assert_eq!(
                        raw.bits_at(RawCellKey::Dummy, layer),
                        output["dummy_raw"][index].as_u64().unwrap() as u8,
                        "{case}"
                    );
                    assert_eq!(
                        raw.owner_at(RawCellKey::Dummy, layer)
                            .map_or(u32::MAX, |owner| owner.index()),
                        output["dummy_owners"][index].as_u64().unwrap() as u32,
                        "{case}"
                    );
                }
                assert_eq!(
                    raw.entry_count(),
                    0,
                    "missing lookups must not create coordinate-key entries"
                );
            }
        }
    }

    #[test]
    fn nearby_live_raw_reservation_selects_the_actual_structural_plane() {
        let mut terrain = flat_terrain(7, 7);
        // Intentionally pin the old cache before a structural repair changes
        // the resident cell. The raw query must observe the new Cell authority.
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut raw = RawCellOccupationGrid::default();
        raw.mark_ground(3, 3, 4);
        let selected = |terrain: &ResolvedTerrainGrid, raw: &RawCellOccupationGrid| {
            let mut q = base_query(terrain, &path_grid);
            q.raw_occupation = Some(raw);
            q.target_cell = Some((3, 3));
            find_nearby_passable_cell((3, 3), &q, 0)
        };
        assert_ne!(
            selected(&terrain, &raw),
            Some((3, 3)),
            "an unlisted retained head blocks the ground plane"
        );
        terrain.cells[3 * 7 + 3].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        assert_eq!(
            selected(&terrain, &raw),
            Some((3, 3)),
            "structural Cell selects its clear deck despite the stale ground cache"
        );
        raw.mark_deck(3, 3, 8);
        assert_ne!(
            selected(&terrain, &raw),
            Some((3, 3)),
            "the selected deck reservation blocks independently"
        );
    }

    #[test]
    fn nearby_seed_height_uses_live_cell_and_bridge_aware_rise() {
        let mut terrain = flat_terrain(7, 7);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        terrain.cells[3 * 7 + 3].level = 2;
        terrain.cells[3 * 7 + 3].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        let mut q = base_query(&terrain, &path_grid);
        q.check_height = true;
        q.radius_cap = 1;
        assert!(collect_candidates((3, 3), &q, NearbySearchOptions::default()).is_empty());
        q.passability.bridge_aware_zone = true;
        let candidates = collect_candidates((3, 3), &q, NearbySearchOptions::default());
        assert_eq!(candidates.len(), 2);
        assert!(
            candidates.iter().all(|c| c.cell == (3, 3)),
            "retained seed6 minus candidate deck rise4 and live level2 is zero"
        );
    }

    #[test]
    fn nearby_seed_lookup_occurs_even_without_height_gate_or_search_rings() {
        let terrain = flat_terrain(3, 3);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.radius_cap = 0;
        assert!(!q.check_height);
        assert!(collect_candidates((40, 41), &q, NearbySearchOptions::default()).is_empty());
        assert_eq!(terrain.shared_cell_dummy().snapshot().coord, (40, 41));
    }

    #[test]
    fn find_nearby_ring_visit_order_matches_engine_segments() {
        // Square (Chebyshev) rings in the engine's fixed 4-segment order:
        //   seg1: for d=-r..=r  -> North (ox+d, oy-r) then South (ox+d, oy+r)
        //   seg2: for e=1-r..=r-1 -> West (ox-r, oy+e) then East (ox+r, oy+e)
        // Ring 0 emits the seed TWICE (N and S coincide at r==0); seg2 range is empty.
        assert_eq!(ring_cells((5, 5), 0), vec![(5, 5), (5, 5)]);
        // Ring 1: seg1 d=-1,0,1 then seg2 e=0. Derived directly from the engine's
        // (ox+d, oy-r)/(ox+d, oy+r) and (ox-r, oy+e)/(ox+r, oy+e) sequence.
        assert_eq!(
            ring_cells((5, 5), 1),
            vec![
                (4, 4), // d=-1 N
                (4, 6), // d=-1 S
                (5, 4), // d=0  N
                (5, 6), // d=0  S
                (6, 4), // d=1  N
                (6, 6), // d=1  S
                (4, 5), // e=0  W
                (6, 5), // e=0  E
            ]
        );
        // Ring 2 is the 16-cell square perimeter (10 from seg1, 6 from seg2).
        assert_eq!(ring_cells((5, 5), 2).len(), 16);
    }

    #[test]
    fn map_owned_radius_uses_signed_size_sum_and_native_cap() {
        assert_eq!(map_owned_radius_cap(8, 7), 15);
        assert_eq!(map_owned_radius_cap(80, 58), RADIUS_HARD_CAP);
        assert_eq!(map_owned_radius_cap(-10, 10), 0);
        assert_eq!(map_owned_radius_cap(i32::MAX, 1), 0);
    }

    /// The start caller's radius-zero north/south paths both store the seed.
    /// This pool-level assertion is deliberately stronger than observing the
    /// selected coordinate, because modulo over two identical entries returns
    /// the same cell as a wrongly deduplicated one-entry pool.
    #[test]
    fn find_nearby_8x8_radius_zero_pool_retains_both_seed_entries() {
        let terrain = flat_terrain(20, 20);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.footprint = NearbyFootprint::new(8, 8);
        q.anchor_gate = NearbyAnchorGate::NativeHeightAware;
        q.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        });
        q.radius_cap = 1;

        let candidates = collect_candidates((5, 5), &q, NearbySearchOptions::default());
        assert_eq!(candidates.len(), 2);
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.cell == (5, 5) && candidate.direct)
        );
        assert_eq!(find_nearby_passable_cell((5, 5), &q, 0), Some((5, 5)));
        assert_eq!(find_nearby_passable_cell((5, 5), &q, 1), Some((5, 5)));
    }

    #[test]
    fn find_nearby_per_ring_early_out_stops_after_first_direct_ring() {
        // On flat terrain every accepted cell is DIRECT (nothing south-east of it rises
        // above it), so ring 0 alone yields a direct hit: the early-out finishes ring 0
        // (which emits the seed twice) and STOPS — it never walks out to fill 24.
        let terrain = flat_terrain(40, 40);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.radius_cap = 100; // requests beyond the hard cap; clamps to 32
        let candidates = collect_candidates((20, 20), &q, NearbySearchOptions::default());
        // Ring 0 = seed twice, both direct -> early-out after ring 0.
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|c| c.cell == (20, 20) && c.direct));
    }

    #[test]
    fn find_nearby_early_out_fires_on_a_raised_plateau_exactly_as_at_level_zero() {
        // THE BUG THIS CLOSES. The occlusion test is relative: on a uniformly raised
        // plateau every south-east difference is 0, so every accepted cell is DIRECT and
        // the per-ring early-out fires on ring 0 — byte-for-byte the level-0 result. The
        // absolute `level == 0` placeholder this replaced marked every cell above level 0
        // indirect, so the early-out never armed, collection ran on to MAX_CANDIDATES
        // across rings 1-3, and a refinery's free miner could land three cells out.
        for level in [0u8, 2, 7] {
            let terrain = plateau_terrain(40, 40, level);
            let path_grid = PathGrid::from_resolved_terrain(&terrain);
            let mut q = base_query(&terrain, &path_grid);
            q.radius_cap = 100; // requests beyond the hard cap; clamps to 32
            let candidates = collect_candidates((20, 20), &q, NearbySearchOptions::default());
            assert_eq!(
                candidates.len(),
                2,
                "level {level}: the early-out must stop after ring 0"
            );
            assert!(candidates.iter().all(|c| c.cell == (20, 20) && c.direct));
        }
    }

    #[test]
    fn find_nearby_projection_kernel_reads_candidate_then_repeated_far_to_near_cells() {
        let mut transcript = Vec::new();
        let returned = project_candidate_with_lookup(5, 5, |x, y| {
            transcript.push((x, y));
            CellClassProjectionView {
                signed_level: 0,
                raw_flags_0x1180: 0,
            }
        });

        let mut expected = vec![(5, 5), (5, 5)];
        expected.extend(std::iter::repeat_n((11, 11), 16));
        for cell in [(10, 10), (9, 9), (8, 8), (7, 7), (6, 6)] {
            expected.extend(std::iter::repeat_n(cell, 32));
        }
        expected.push((5, 5));

        assert_eq!(returned, (5, 5));
        assert_eq!(transcript, expected);
        assert_eq!(transcript.len(), 179, "two candidate reads plus 177 probes");
    }

    #[test]
    fn find_nearby_sparse_edge_uses_one_live_dummy_view_for_level_and_flags() {
        let mut terrain = flat_terrain(3, 3);
        terrain.cells[1 * 3 + 1].bridge_facts.raw_flags = BRIDGE_FLAG_FORWARD_SIDE;
        terrain.test_set_native_allocated_cells(&[(1, 1)]);
        let dummy = terrain.shared_cell_dummy();
        dummy.set_level_slope(-3, 0);
        dummy.set_bridge_flags_0x1180(0);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let q = base_query(&terrain, &path_grid);

        assert_eq!(
            project_candidate(&q, 1, 1),
            (1, 1),
            "negative dummy level alone leaves the candidate direct"
        );

        dummy.set_bridge_flags_0x1180(BRIDGE_FLAG_STRUCTURAL);
        assert_eq!(
            project_candidate(&q, 1, 1),
            (2, 2),
            "the same dummy lookup contributes -3 + 4 and returns the probe"
        );
        assert!(!is_direct_candidate(&q, 1, 1));
        let snapshot = dummy.snapshot();
        assert_eq!(snapshot.coord, (2, 2));
        assert_eq!(snapshot.level, -3);
        assert_eq!(snapshot.bridge_flags_0x1180, BRIDGE_FLAG_STRUCTURAL);
    }

    #[test]
    fn find_nearby_collection_and_final_partition_project_separately_in_order() {
        let terrain = flat_terrain(3, 3);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.radius_cap = 1;
        let mut events = Vec::new();
        let mut project = |site, x, y| {
            events.push((site, (x, y)));
            matches!(site, ProjectionUse::FinalPartition)
        };

        assert_eq!(
            find_nearby_passable_cell_with_projection(
                (1, 1),
                &q,
                NearbySearchOptions::default(),
                0,
                &mut project,
            ),
            Some((1, 1))
        );
        assert_eq!(
            events,
            vec![
                (ProjectionUse::Collection, (1, 1)),
                (ProjectionUse::Collection, (1, 1)),
                (ProjectionUse::FinalPartition, (1, 1)),
                (ProjectionUse::FinalPartition, (1, 1)),
            ]
        );
    }

    #[test]
    fn find_nearby_bridge_aware_collection_skips_projection_but_final_still_runs() {
        let terrain = flat_terrain(3, 3);
        let dummy = terrain.shared_cell_dummy();
        dummy.stamp_coord(99, 99);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.radius_cap = 1;
        q.passability.bridge_aware_zone = true;

        let candidates = collect_candidates((1, 1), &q, NearbySearchOptions::default());
        assert_eq!(candidates.len(), 2);
        assert_eq!(
            dummy.snapshot().coord,
            (99, 99),
            "bridge-aware collection must perform no hidden dummy lookup"
        );

        let mut events = Vec::new();
        let mut project = |site, x, y| {
            events.push((site, (x, y)));
            true
        };
        assert_eq!(
            find_nearby_passable_cell_with_projection(
                (1, 1),
                &q,
                NearbySearchOptions::default(),
                0,
                &mut project,
            ),
            Some((1, 1))
        );
        assert_eq!(
            events,
            vec![
                (ProjectionUse::FinalPartition, (1, 1)),
                (ProjectionUse::FinalPartition, (1, 1)),
            ]
        );
    }

    #[test]
    fn find_nearby_direct_threshold_is_two_levels_per_diagonal_step_minus_one() {
        // A cell `q` diagonal steps SOUTH-EAST of the candidate occludes it — making the
        // candidate INDIRECT — once it rises `2q - 1` levels above it. One level below
        // that threshold leaves the candidate direct. Walked as a fixture per step
        // rather than asserted from the same formula the code uses.
        const GRID: usize = 20;
        for (step, threshold) in [(1i32, 1i16), (2, 3), (3, 5), (4, 7), (5, 9), (6, 11)] {
            for (rise, expect_direct) in [(threshold - 1, true), (threshold, false)] {
                let mut terrain = flat_terrain(GRID as u16, GRID as u16);
                let probe = 5 + step as usize;
                terrain.cells[probe * GRID + probe].level = rise as u8;
                assert_eq!(
                    direct_at(&terrain, (5, 5)),
                    expect_direct,
                    "step {step}: a rise of {rise} against a threshold of {threshold}"
                );
            }
        }
    }

    #[test]
    fn find_nearby_direct_probes_only_six_cells_down_the_south_east_diagonal() {
        // The ray starts a fixed six cells south-east and walks back, so step 7 is never
        // probed; and only the exact diagonal is probed, because a cliff due east or due
        // south is a different screen direction and cannot draw over the candidate.
        const GRID: usize = 20;
        let mut terrain = flat_terrain(GRID as u16, GRID as u16);
        terrain.cells[12 * GRID + 12].level = 13; // (12,12): seven diagonal steps out
        terrain.cells[5 * GRID + 6].level = 10; // (6,5): due EAST of the candidate
        terrain.cells[6 * GRID + 5].level = 10; // (5,6): due SOUTH of the candidate
        assert!(direct_at(&terrain, (5, 5)));
    }

    #[test]
    fn find_nearby_direct_reads_the_relative_rise_not_the_absolute_level() {
        const GRID: usize = 20;
        // Absolute height is irrelevant; only the south-east difference decides.
        let high_plateau = plateau_terrain(GRID as u16, GRID as u16, 7);
        assert!(
            direct_at(&high_plateau, (5, 5)),
            "a level-7 cell among level-7 cells is direct"
        );
        let mut stepped_up = plateau_terrain(GRID as u16, GRID as u16, 7);
        stepped_up.cells[6 * GRID + 6].level = 8;
        assert!(
            !direct_at(&stepped_up, (5, 5)),
            "the same cell under a single level of rise at step 1 is indirect"
        );

        // Ground falling away to the south-east can never occlude.
        let mut downhill = plateau_terrain(GRID as u16, GRID as u16, 5);
        for step in 1..=6usize {
            downhill.cells[(5 + step) * GRID + (5 + step)].level = 5u8.saturating_sub(step as u8);
        }
        assert!(direct_at(&downhill, (5, 5)));

        // At the map's south-east corner every probe leaves the grid and reads the
        // fixture's fresh shared dummy (level 0, never null), so a raised corner cell
        // is still direct.
        let mut corner = flat_terrain(GRID as u16, GRID as u16);
        corner.cells[(GRID - 1) * GRID + (GRID - 1)].level = 3;
        assert!(direct_at(&corner, (GRID as i32 - 1, GRID as i32 - 1)));
    }

    #[test]
    fn find_nearby_direct_ignores_the_candidates_own_bridge_bit() {
        // The projection reads terrain LEVELS only. A candidate's own bridge bit is read
        // by the height gate and the bridge filter, never by this test — folding it in
        // is what made the placeholder call every bridge cell on flat ground indirect.
        let mut terrain = flat_terrain(20, 20);
        terrain.cells[5 * 20 + 5].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        assert!(direct_at(&terrain, (5, 5)));
    }

    #[test]
    fn find_nearby_bridge_projection_adds_exactly_four_probe_levels() {
        const GRID: usize = 20;
        let mut terrain = flat_terrain(GRID as u16, GRID as u16);
        terrain.cells[5 * GRID + 5].bridge_facts.raw_flags = BRIDGE_FLAG_FORWARD_SIDE;
        terrain.cells[8 * GRID + 8].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;

        assert!(
            direct_at(&terrain, (5, 5)),
            "step-three threshold is five, so a structural probe's +4 remains direct"
        );
        terrain.cells[8 * GRID + 8].level = 1;
        assert!(
            !direct_at(&terrain, (5, 5)),
            "the same +1 probe becomes +5 only after the exact four-level correction"
        );
    }

    #[test]
    fn find_nearby_bridge_projection_ignores_structural_probe_without_forward_candidate() {
        const GRID: usize = 20;
        let mut terrain = flat_terrain(GRID as u16, GRID as u16);
        terrain.cells[8 * GRID + 8].level = 1;
        terrain.cells[8 * GRID + 8].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;

        assert!(
            direct_at(&terrain, (5, 5)),
            "probe structural bit is ignored while candidate 0x1000 is clear"
        );
        terrain.cells[5 * GRID + 5].bridge_facts.raw_flags = BRIDGE_FLAG_FORWARD_SIDE;
        assert!(
            !direct_at(&terrain, (5, 5)),
            "the same probe contributes four only after candidate 0x1000 is set"
        );
    }

    #[test]
    fn find_nearby_bridge_projection_changes_collection_stop_and_frame_pool_exactly() {
        const GRID: usize = 20;
        let seed = (5, 5);

        let baseline = flat_terrain(GRID as u16, GRID as u16);
        let baseline_path = PathGrid::from_resolved_terrain(&baseline);
        let mut baseline_query = base_query(&baseline, &baseline_path);
        baseline_query.radius_cap = 2;
        let baseline_candidates =
            collect_candidates(seed, &baseline_query, NearbySearchOptions::default());
        assert_eq!(baseline_candidates.len(), 2);
        assert!(baseline_candidates.iter().all(|candidate| candidate.direct));
        assert_eq!(
            find_nearby_passable_cell(seed, &baseline_query, 0),
            Some((5, 5))
        );
        assert_eq!(
            find_nearby_passable_cell(seed, &baseline_query, 1),
            Some((5, 5))
        );

        let mut projected = flat_terrain(GRID as u16, GRID as u16);
        let projected_path = PathGrid::from_resolved_terrain(&projected);
        projected.cells[5 * GRID + 5].bridge_facts.raw_flags = BRIDGE_FLAG_FORWARD_SIDE;
        projected.cells[6 * GRID + 6].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        let mut projected_query = base_query(&projected, &projected_path);
        projected_query.radius_cap = 2;

        let projected_candidates =
            collect_candidates(seed, &projected_query, NearbySearchOptions::default());
        assert_eq!(
            projected_candidates.len(),
            10,
            "two indirect radius-zero entries defer early-stop through ring one"
        );
        assert!(
            projected_candidates[..2]
                .iter()
                .all(|candidate| !candidate.direct)
        );
        assert!(
            projected_candidates[2..]
                .iter()
                .all(|candidate| candidate.direct)
        );

        let ring_one = ring_cells(seed, 1);
        for (frame, expected) in ring_one.into_iter().enumerate() {
            assert_eq!(
                find_nearby_passable_cell(seed, &projected_query, frame as u32),
                cell_to_u16(expected),
                "frame modulo must walk the final direct pool in ring order"
            );
        }
    }

    #[test]
    fn find_nearby_bridge_projection_leaves_nonbridge_callers_unchanged() {
        let terrain = plateau_terrain(20, 20, 7);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.radius_cap = 2;

        let candidates = collect_candidates((5, 5), &q, NearbySearchOptions::default());
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().all(|candidate| candidate.direct));
        for frame in 0..4 {
            assert_eq!(find_nearby_passable_cell((5, 5), &q, frame), Some((5, 5)));
        }
    }

    #[test]
    fn find_nearby_slope_keeps_the_cell_under_the_rise_indirect() {
        // A genuine slope still splits the pools. Seed (5,5) is walled off so the search
        // reaches ring 1, and is itself raised two levels — which is exactly one diagonal
        // step south-east of ring-1's (4,4). So (4,4) is occluded and lands in the
        // indirect pool while every other ring-1 cell stays on flat ground and is direct,
        // and selection — which consults the indirect pool only when there are no directs
        // — can never return (4,4).
        const GRID: usize = 20;
        let mut terrain = flat_terrain(GRID as u16, GRID as u16);
        terrain.cells[5 * GRID + 5].zone_type = zone_class::WALL;
        terrain.cells[5 * GRID + 5].level = 2;
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let q = base_query(&terrain, &path_grid);

        let candidates = collect_candidates((5, 5), &q, NearbySearchOptions::default());
        let occluded = candidates
            .iter()
            .find(|c| c.cell == (4, 4))
            .expect("(4,4) is collected on ring 1");
        assert!(
            !occluded.direct,
            "(4,4) sits under a +2 rise one diagonal step south-east"
        );
        assert!(
            candidates
                .iter()
                .filter(|c| c.cell != (4, 4))
                .all(|c| c.direct),
            "every other ring-1 cell is on flat ground and stays direct"
        );
        for frame in 0..12u32 {
            assert_ne!(
                find_nearby_passable_cell((5, 5), &q, frame),
                Some((4, 4)),
                "frame {frame}: the direct pool is non-empty, so the occluded cell is never picked"
            );
        }
    }

    #[test]
    fn find_nearby_bridge_aware_zone_arms_the_early_out_without_the_projection() {
        // The engine skips the projection entirely in the bridge-aware zone, so ANY
        // accepted candidate arms the per-ring early-out there — while selection still
        // re-runs the projection to split the pools. On terrain climbing one level per
        // south-east step nothing is ever direct, so without the asymmetry the search
        // runs on to the 24-candidate cap.
        let terrain = diagonal_ramp_terrain(20, 20);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);

        let unarmed = collect_candidates((8, 8), &q, NearbySearchOptions::default());
        assert_eq!(unarmed.len(), MAX_CANDIDATES);
        assert!(unarmed.iter().all(|c| !c.direct));

        q.passability.bridge_aware_zone = true;
        let armed = collect_candidates((8, 8), &q, NearbySearchOptions::default());
        assert_eq!(armed.len(), 2, "ring 0 alone arms the early-out");
        assert!(
            armed.iter().all(|c| !c.direct),
            "bridge-aware collection stores no projection classification"
        );
    }

    #[test]
    fn find_nearby_calls_occupancy_with_skip_reservation() {
        // FNPC's per-candidate occupancy check always uses reservation_arg = -1
        // (SkipReservation) and never a house index. With an occupant in the seed cell,
        // the seed is dropped (its Ground layer is non-empty) and a free neighbour is
        // returned instead — exercising the occupancy gate on the SkipReservation path.
        use crate::sim::movement::locomotor::MovementLayer;
        use crate::sim::occupancy::{CellListInsertion, OccupancyGrid};
        let terrain = flat_terrain(5, 5);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut occupancy = OccupancyGrid::new();
        occupancy.add(
            2,
            2,
            7,
            MovementLayer::Ground,
            None,
            CellListInsertion::PrependNonBuilding,
        );
        let entities = EntityStore::new();
        let mut q = base_query(&terrain, &path_grid);
        q.check_occupancy = true;
        q.occupancy = Some(&occupancy);
        q.entities = Some(&entities);
        q.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        });

        let found = find_nearby_passable_cell((2, 2), &q, 0);
        // The seed (2,2) is occupied; FNPC must pick a different, free cell.
        assert!(found.is_some());
        assert_ne!(found, Some((2, 2)));
    }

    #[test]
    fn find_nearby_allow_bridge_filters_after_passability() {
        // A passable bridge cell at the seed is dropped when bridges are disallowed.
        let mut terrain = flat_terrain(3, 3);
        terrain.cells[1 * 3 + 1].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
        let path_grid = PathGrid::from_resolved_terrain(&terrain);

        let mut q = base_query(&terrain, &path_grid);
        q.allow_bridge_cells = false;
        let found = find_nearby_passable_cell((1, 1), &q, 0);
        // Bridge seed filtered out; a non-bridge neighbour is chosen instead.
        assert!(found.is_some());
        assert_ne!(found, Some((1, 1)));

        // With bridges allowed, the seed is eligible again.
        q.allow_bridge_cells = true;
        assert!(find_nearby_passable_cell((1, 1), &q, 0).is_some());
    }

    #[test]
    fn find_nearby_no_candidate_returns_none() {
        // Every cell rejected -> no candidate -> None. WALL zone_type with a Normal
        // movement-zone rejects in speed_type_allows_cell (only the destroyer family
        // crosses walls), so every candidate fails passability.
        let mut terrain = flat_terrain(3, 3);
        for c in terrain.cells.iter_mut() {
            c.zone_type = zone_class::WALL;
        }
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.radius_cap = 2;
        assert_eq!(find_nearby_passable_cell((1, 1), &q, 0), None);
    }

    #[test]
    fn find_nearby_overlay_rejection_is_a_caller_argument() {
        // The engine's two free-unit attempts differ in exactly one argument: the
        // first rejects a cell carrying an overlay, the second accepts one. So the
        // same query over the same grid must return the ore seed with the default
        // options and refuse it with `reject_any_overlay`.
        const ORE_OVERLAY_ID: u8 = 102;
        let terrain = flat_terrain(5, 5);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut overlay = OverlayGrid::new(5, 5);
        overlay.place_overlay(2, 2, ORE_OVERLAY_ID, 0);
        let mut q = base_query(&terrain, &path_grid);
        q.overlay_grid = Some(&overlay);

        assert_eq!(
            find_nearby_passable_cell((2, 2), &q, 0),
            Some((2, 2)),
            "the overlay-allowed attempt keeps the ore cell"
        );
        let rejecting = NearbySearchOptions {
            reject_any_overlay: true,
        };
        let found = find_nearby_passable_cell_with_options((2, 2), &q, rejecting, 0)
            .expect("clear ground exists on the next ring");
        assert_ne!(
            found,
            (2, 2),
            "the overlay-rejecting attempt must walk off the ore cell"
        );
    }

    #[test]
    fn find_nearby_height_gate_drops_candidates_more_than_one_level_from_seed() {
        // `check_height` admits a candidate only while it stays within one level of
        // the seed. Seed (2,2) is walled off so the search reaches ring 1, where
        // (2,1) sits three levels up: it survives with the gate off and is dropped
        // with the gate on, while the level-1 cell at (1,2) survives either way.
        let mut terrain = flat_terrain(5, 5);
        terrain.cells[2 * 5 + 2].zone_type = zone_class::WALL; // seed rejected
        terrain.cells[1 * 5 + 2].level = 3; // (2,1): three levels above the seed
        terrain.cells[2 * 5 + 1].level = 1; // (1,2): one level above the seed
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);

        let without_gate: Vec<(i32, i32)> =
            collect_candidates((2, 2), &q, NearbySearchOptions::default())
                .into_iter()
                .map(|c| c.cell)
                .collect();
        assert!(without_gate.contains(&(2, 1)));
        assert!(without_gate.contains(&(1, 2)));

        q.check_height = true;
        let with_gate: Vec<(i32, i32)> =
            collect_candidates((2, 2), &q, NearbySearchOptions::default())
                .into_iter()
                .map(|c| c.cell)
                .collect();
        assert!(
            !with_gate.contains(&(2, 1)),
            "a candidate three levels from the seed must be rejected"
        );
        assert!(
            with_gate.contains(&(1, 2)),
            "a candidate one level from the seed must survive"
        );
    }

    #[test]
    fn find_nearby_passes_required_height_minus_one() {
        // FNPC always supplies required_height_or_level = -1 (None) regardless of any
        // caller height: a flat seed cell is found with no height gating applied.
        let terrain = flat_terrain(3, 3);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let q = base_query(&terrain, &path_grid);
        let found = find_nearby_passable_cell((1, 1), &q, 0);
        assert_eq!(found, Some((1, 1)));
    }

    #[test]
    fn find_nearby_selection_uses_frame_counter_modulo() {
        // No target: the chosen index is frame_counter % pool.len(), direct-preferred.
        // Walking the frame counter cycles deterministically through the direct pool.
        let terrain = flat_terrain(7, 7);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let q = base_query(&terrain, &path_grid);

        let directs: Vec<_> = collect_candidates((3, 3), &q, NearbySearchOptions::default())
            .into_iter()
            .filter(|c| c.direct)
            .collect();
        assert!(!directs.is_empty());

        for frame in 0..directs.len() as u32 * 2 {
            let expected = directs[(frame as usize) % directs.len()].cell;
            assert_eq!(
                find_nearby_passable_cell((3, 3), &q, frame),
                cell_to_u16(expected),
                "frame {frame} selection mismatch"
            );
        }
    }

    #[test]
    fn find_nearby_same_tick_aliasing() {
        // Two no-target calls on the same frame with the same candidate set return the
        // SAME cell (reproduce gamemd aliasing; do not spread).
        let terrain = flat_terrain(7, 7);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let q = base_query(&terrain, &path_grid);
        let a = find_nearby_passable_cell((3, 3), &q, 42);
        let b = find_nearby_passable_cell((3, 3), &q, 42);
        assert_eq!(a, b);
        assert!(a.is_some());
    }

    #[test]
    fn find_nearby_selection_is_bit_identical_across_runs() {
        // Determinism guard: replaying the same FNPC query over the same grid yields a
        // bit-identical sequence of chosen cells across a frame sweep. Projection may
        // deterministically stamp the shared dummy, so repeatability includes that
        // ordered side effect rather than claiming the helper is hash-neutral.
        let terrain = flat_terrain(11, 11);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let q = base_query(&terrain, &path_grid);
        let run = |seed: (i32, i32)| -> Vec<Option<(u16, u16)>> {
            (0..40u32)
                .map(|f| find_nearby_passable_cell(seed, &q, f))
                .collect()
        };
        assert_eq!(run((5, 5)), run((5, 5)));
    }

    #[test]
    fn find_nearby_target_selection_uses_nearest_distance() {
        // With a target, selection is nearest-Euclidean over the preferred pool, with
        // no frame-counter influence: the same target gives the same cell for any frame.
        let terrain = flat_terrain(9, 9);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.target_cell = Some((7, 4)); // east of the seed (4,4)
        let pick0 = find_nearby_passable_cell((4, 4), &q, 0);
        let pick9 = find_nearby_passable_cell((4, 4), &q, 9);
        assert_eq!(
            pick0, pick9,
            "target selection must ignore the frame counter"
        );
        // On flat terrain the per-ring early-out stops at ring 0 (seed is direct), so
        // the nearest-distance pool is the seed itself; the chosen cell is at/east of
        // the seed and never west of it.
        let pick = pick0.expect("a candidate exists");
        assert!(
            pick.0 >= 4,
            "nearest-to-target should not lean away from the target"
        );
    }

    /// With the map's playfield diamond threaded into the query, occupancy rejects
    /// rectangle-inside but diamond-outside cells. Missing fields reject the exact
    /// MapClass query rather than substituting the retired rectangle fallback.
    #[test]
    fn find_nearby_occupancy_rejects_off_diamond_cells_when_bounds_threaded() {
        use crate::sim::cell_rect::PlayfieldBounds;
        // Diamond (same fixture as the cell_rect acceptance tests): pass iff
        // 12 < x+y <= 26, x−y < 14, y−x < 6.
        let bounds = PlayfieldBounds {
            base: 10,
            off_fc: 2,
            off_100: 1,
            off_104: 10,
            off_108: 6,
        };
        let terrain = flat_terrain(30, 30);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.check_occupancy = true;

        // Without configured MapClass fields no exact query can be made.
        assert_eq!(
            find_nearby_passable_cell((14, 13), &q, 0),
            None,
            "missing bounds must not approximate MapClass with a rectangle"
        );

        // With the diamond: the seed and every other sum>26 ring cell are rejected;
        // the pool is exactly the three in-diamond ring-1 survivors in ring order
        // ((13,12), (14,12), (13,13)) and the frame counter walks that pool.
        q.playfield_bounds = Some(bounds);
        let in_diamond = |c: (u16, u16)| {
            let (x, y) = (c.0 as i32, c.1 as i32);
            12 < x + y && x + y <= 26 && x - y < 14 && y - x < 6
        };
        for frame in 0..6u32 {
            let pick = find_nearby_passable_cell((14, 13), &q, frame)
                .expect("in-diamond candidates exist on ring 1");
            assert!(
                in_diamond(pick),
                "FNPC picked off-diamond cell {pick:?} with bounds threaded"
            );
        }
        assert_eq!(
            find_nearby_passable_cell((14, 13), &q, 0),
            Some((13, 12)),
            "frame 0 picks the first in-diamond survivor in engine ring order"
        );
    }
}
