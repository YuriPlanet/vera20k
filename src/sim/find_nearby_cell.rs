//! Nearby-passable-cell search (engine `Find_Nearby_Passable_Cell`).
//!
//! Square (Chebyshev) ring expansion around a seed cell — concentric perimeters,
//! NOT a Manhattan diamond (see [`square_ring_cells`]); per-candidate passability (plus an
//! optional occupancy check that always SKIPS reservations); frame-counter
//! selection when no target cell is given, nearest-distance selection when a
//! target is given. It consumes no RNG stream. Exact CellClass projection misses
//! do stamp the process-shared dummy coordinate, so lookup order is deterministic,
//! future-affecting state; no other grid state is changed. Depends only on `map/`,
//! `sim/`, and `rules/`; never on render/ui/audio/net.
//!
//! Determinism contract:
//! - The square-ring candidate ORDER is fully deterministic; it feeds both the
//!   frame-counter modulo index and the nearest-distance tie-break.
//! - Nearest-distance compares the game's table roots of `dy*dy + dx*dx`
//!   ([`nearest_to_target`]); the earlier of two equal roots wins.
//! - `frame_counter % pool.len()` reproduces the engine's same-tick aliasing by
//!   construction: two no-target calls on the same frame with the same candidate
//!   count return the same index. Do NOT add any per-call perturbation.

use crate::map::cell_index::square_ring_cells;
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
use crate::sim::pathfinding::zone_map::ZoneGrid;
use crate::util::native_x87::{X87Chop53, sqrt_approx_f32};

/// Foot4CBC40..4CBEE2: search the side of `target` facing `from`, in
/// increasing radii (exclusive upper bound). Each edge starts AFTER its
/// corner. This is distinct from Map56DC20's full-ring/pool search below.
/// Original direction, wrapping and visit controls: team_recruit_oracle --retreat.
fn find_nearby_foot_cell(
    from: (i16, i16),
    target: (i16, i16),
    radii: std::ops::Range<i32>,
    mut can_enter: impl FnMut((i16, i16), u8) -> bool,
) -> Option<(i16, i16)> {
    use crate::util::direction::DIRECTION_DELTAS;
    use crate::util::direction_tables::{dir_from_facing16, facing16_between};
    let facing = facing16_between(
        [i32::from(from.0), i32::from(from.1)],
        [i32::from(target.0), i32::from(target.1)],
    );
    let start = (dir_from_facing16(facing) ^ 4).wrapping_sub(1) & 7;
    let delta = DIRECTION_DELTAS[usize::from(start)];
    for radius in radii {
        let mut cell = (
            target
                .0
                .wrapping_add((radius as i16).wrapping_mul(delta.0 as i16)),
            target
                .1
                .wrapping_add((radius as i16).wrapping_mul(delta.1 as i16)),
        );
        let segments = if start & 1 != 0 {
            [((start + 3) & 7, radius.wrapping_mul(2)), (0, 0)]
        } else {
            [
                ((start + 2) & 7, radius),
                (start.wrapping_sub(4) & 7, radius),
            ]
        };
        for (direction, count) in segments {
            let step = DIRECTION_DELTAS[usize::from(direction)];
            for _ in 0..count {
                cell.0 = cell.0.wrapping_add(step.0 as i16);
                cell.1 = cell.1.wrapping_add(step.1 as i16);
                if can_enter(cell, direction) {
                    return Some(cell);
                }
            }
        }
    }
    None
}

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
    /// The mover's `GetZoneID` DWORD (`0x0056D230`).
    pub required_zone_id: Option<u32>,
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
    pub(crate) raw_occupation: Option<&'a RawCellOccupationGrid>,
    /// Per-candidate passability config.
    pub passability: PassabilityArgs,
    /// Top-left rectangle dimensions forwarded to passability and, when enabled,
    /// final occupancy.
    pub footprint: NearbyFootprint,
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
    /// by the candidate-anchor gate every candidate runs and by the occupancy
    /// check's later corner test. Absent bounds reject every candidate: there is
    /// no rectangular substitute.
    pub playfield_bounds: Option<crate::sim::cell_rect::PlayfieldBounds>,
}

/// A surviving FNPC candidate. `direct` (kept for tests) records only the
/// ordinary collection-time projection used for per-ring early-stop;
/// bridge-aware collection deliberately leaves it false, and final partition
/// always projects again rather than reading it.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    cell: (i32, i32),
    #[cfg(any(test, feature = "test-support"))]
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
        // Native56E6A8..56E6DE uses signed CDQ/IDIV. Nonnegative Frame has
        // this exact selection. After signed overflow a negative remainder
        // reads before the native stack array (even uninitialized bytes), so
        // VERA deliberately keeps a deterministic unsigned modulo extension.
        // The Building443860 corpus preserves these boundary controls but does
        // not certify their stack-dependent output as gameplay parity.
        None => pool[(frame_counter as usize) % pool.len()],
        Some(target) => {
            pool[nearest_to_target(pool.iter().map(|c| c.cell), target).expect("pool is non-empty")]
        }
    };

    cell_to_u16(chosen.cell)
}

/// The index of the candidate `Find_Nearby_Passable_Cell` picks for a target
/// (`0x0056E6F0..0x0056E780`): the first whose `Sqrt_Approx` distance, of
/// `dy*dy + dx*dx` in 32-bit integers, is strictly below the best so far. The
/// best starts at 100000.0, above any such root, so the first candidate always
/// takes it. Squares from 16384 (128 cells) on can share a table root, and the
/// earlier candidate then keeps the pick.
fn nearest_to_target(
    cells: impl IntoIterator<Item = (i32, i32)>,
    target: (i32, i32),
) -> Option<usize> {
    cells
        .into_iter()
        .enumerate()
        .min_by_key(|&(_, (x, y))| {
            let [dx, dy] = [x.wrapping_sub(target.0), y.wrapping_sub(target.1)];
            let squared = dy.wrapping_mul(dy).wrapping_add(dx.wrapping_mul(dx));
            // A root is nonnegative, so its bits order as its value.
            sqrt_approx_f32(X87Chop53::load_i32(squared))
                .expect("an i32 is a finite f32")
                .bits()
        })
        .map(|(index, _)| index)
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
/// engine exactly (see [`square_ring_cells`]).
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
#[cfg(any(test, feature = "test-support"))]
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
        level = level.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS as i16);
    }
    let seed_level = q.check_height.then_some(level);

    let mut r = 0;
    while r < cap {
        for (cx, cy) in square_ring_cells(seed, r) {
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
                #[cfg(any(test, feature = "test-support"))]
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
    project_candidate_with_lookup(
        i32::from(crate::util::lepton::lepton_to_cell_packed(x)),
        i32::from(crate::util::lepton::lepton_to_cell_packed(y)),
        lookup,
    )
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
            i32::from(crate::util::lepton::lepton_to_cell_packed(probe_world_x)),
            i32::from(crate::util::lepton::lepton_to_cell_packed(probe_world_y)),
        );
        let probe_view = lookup(probe.0, probe.1);
        let mut level_delta = probe_view.signed_level.wrapping_sub(candidate_level);
        if candidate_is_forward_side
            && probe_view.raw_flags_0x1180 & crate::map::bridge_facts::BRIDGE_FLAG_STRUCTURAL != 0
        {
            level_delta = level_delta.wrapping_add(crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS);
        }

        let projection_shift = level_delta.wrapping_mul(PROJECTION_LEVEL_LEPTONS);
        let projected_x = i32::from(crate::util::lepton::lepton_to_cell_packed(
            probe_world_x.wrapping_sub(projection_shift),
        ));
        let projected_y = i32::from(crate::util::lepton::lepton_to_cell_packed(
            probe_world_y.wrapping_sub(projection_shift),
        ));

        if projected_x <= cx && projected_y <= cy {
            return probe;
        }
        if probe == (cx, cy) {
            return (cx, cy);
        }
    }
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
    if !cell_is_in_playfield_height_aware_in_query(
        (cx, cy),
        q.playfield_bounds,
        q.resolved_terrain,
        q.native_cells,
    ) {
        return false;
    }

    let rect = CellRect::new(cx, cy, q.footprint.width, q.footprint.height);

    let passable = check_passability_rect_with_raw_occupation(
        CellRectPassabilityContext {
            native_cells: q.native_cells,
            rect,
            speed_type: q.passability.speed_type,
            // FNPC's entry turns a zone argument of 0xFFFF into -1
            // (`0x0056DC43..0x0056DC60`, a full DWORD compare); a missing
            // bridge record's 0xFFFFFFFF already is -1. Either disables the
            // comparison.
            required_zone_id: q
                .passability
                .required_zone_id
                .filter(|&zone| zone != 0xFFFF && zone != u32::MAX),
            movement_zone: q.passability.movement_zone,
            required_height_or_level: None, // the search always passes -1 (L21)
            bridge_aware_zone: q.passability.bridge_aware_zone,
            // Caller argument, forwarded verbatim: the engine's two free-unit attempts
            // differ in this one value and nothing else.
            reject_any_overlay: options.reject_any_overlay,
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
            terrain_object_cells: None,
            resolved_terrain: q.resolved_terrain,
            overlay_grid: q.overlay_grid,
            playfield_bounds: q.playfield_bounds,
        })
}

/// The caller's height gate: `abs(seedLevel - bridgeRise - candidateLevel) < 2`,
/// where `bridgeRise` is [`crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS as i16`] when the candidate carries a bridge
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
        crate::util::lepton::BRIDGE_DECK_HEIGHT_LEVELS as i16
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

/// What a caller of `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20`
/// pushes besides the seed. Every static caller loads `g_Map` as ECX, so
/// [`Simulation::nearby_passable_cell`] supplies the map itself.
#[derive(Debug, Clone, Copy)]
pub(crate) struct NearbyArgs {
    pub passability: PassabilityArgs,
    pub footprint: NearbyFootprint,
    pub allow_bridge_cells: bool,
    pub check_height: bool,
    pub check_occupancy: bool,
    pub target_cell: Option<(i32, i32)>,
    pub options: NearbySearchOptions,
}

impl NearbyArgs {
    /// The shape most callers push: bridge cells allowed; no overlay,
    /// height, obstacle or occupancy test; no target cell, so the pick is
    /// the frame-counter modulo and draws no RNG; no quadrant skip.
    pub(crate) const fn plain(passability: PassabilityArgs, footprint: NearbyFootprint) -> Self {
        Self {
            passability,
            footprint,
            allow_bridge_cells: true,
            check_height: false,
            check_occupancy: false,
            target_cell: None,
            options: NearbySearchOptions {
                reject_any_overlay: false,
            },
        }
    }
}

impl crate::sim::world::Simulation {
    /// Foot4CBC40's class-specific entry receiver. Native looks up each
    /// candidate before dispatching +1AC, including an Aircraft receiver.
    pub(crate) fn foot_nearby_location(
        &self,
        id: u64,
        from: (i16, i16),
        target: (i16, i16),
        radii: std::ops::Range<i32>,
        rules: &crate::rules::ruleset::RuleSet,
        registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    ) -> Option<(i16, i16)> {
        use crate::sim::movement::infantry_entry::{EntryQueryMode, InfantryEntryArgs};
        let cells = crate::map::resolved_terrain::NativeCellQuery::canonical(
            self.resolved_terrain.as_ref()?,
        );
        find_nearby_foot_cell(from, target, radii, |cell, direction| {
            let _ = cells.lookup(cell);
            self.mover_can_enter(
                id,
                cell,
                InfantryEntryArgs {
                    direction: i32::from(direction),
                    height: -1,
                    previous_cell: None,
                },
                EntryQueryMode::CheckLocomotor,
                rules,
                registry,
            )
            .is_ok_and(|code| code == 0)
        })
        .filter(|&cell| cell != (0, 0))
    }

    /// `MapClass::Find_Nearby_Passable_Cell @ 0x0056DC20` on the live map:
    /// its cells and their raw occupation, the path, overlay, object and
    /// zone grids, the playfield, the radius cap from the map's Size
    /// ([`map_owned_radius_cap`]) and the current frame for the no-target
    /// pick. `None` when nothing passes or the map size is unknown.
    pub(crate) fn nearby_passable_cell(
        &self,
        seed: (i32, i32),
        args: NearbyArgs,
    ) -> Option<(u16, u16)> {
        #[cfg(test)]
        if let Some(answer) =
            crate::sim::movement::fresh_oracle_seam::supplied_nearby_cell(seed, args)
        {
            return answer;
        }
        let (width, height) = self.map_size_diamond()?;
        find_nearby_passable_cell_with_options(
            seed,
            &NearbyQuery {
                native_cells: None,
                raw_occupation: Some(&self.substrate.raw_cell_occupation),
                passability: args.passability,
                footprint: args.footprint,
                allow_bridge_cells: args.allow_bridge_cells,
                check_height: args.check_height,
                check_occupancy: args.check_occupancy,
                radius_cap: map_owned_radius_cap(width, height),
                target_cell: args.target_cell,
                path_grid: self.path_grid(),
                resolved_terrain: self.resolved_terrain.as_ref(),
                overlay_grid: self.overlay_grid.as_ref(),
                occupancy: Some(&self.substrate.occupancy),
                entities: Some(&self.substrate.entities),
                zone_grid: self.zone_grid.as_ref(),
                playfield_bounds: self.playfield_bounds(),
            },
            args.options,
            // The frame counter is committed late, so during an advance it
            // still holds the current frame, the value the pick aliases.
            self.session.binary_frame(),
        )
    }

    /// TechnoClass NearbyLocation `0x00703590`: FNPC seeded at the cell of
    /// `anchor`'s coordinate (its vt+0x48; the object's own when `anchor` is
    /// NULL) with the object's SpeedType (Winged read as Track), its
    /// MovementZone, `GetZoneID(seed, MovementZone, OnBridge)` and OnBridge.
    pub(crate) fn techno_nearby_location(
        &self,
        id: u64,
        anchor: Option<u64>,
        rules: &crate::rules::ruleset::RuleSet,
    ) -> Option<(i16, i16)> {
        let entity = self.substrate.entities.get(id)?;
        let object = self.object_type(entity.type_ref(), rules)?;
        let speed_type = if object.speed_type == SpeedType::Winged {
            SpeedType::Track
        } else {
            object.speed_type
        };
        let seed = match anchor {
            None => (
                i32::from(entity.position.rx()),
                i32::from(entity.position.ry()),
            ),
            Some(anchor) => {
                let [x, y] = crate::sim::movement::ground_pose::object_center_xy(
                    self.substrate.entities.get(anchor)?,
                );
                // 0x007035E8: `(c + (c >> 31 & 0xFF)) >> 8`, toward zero.
                (x / 256, y / 256)
            }
        };
        let zone = self
            .zone_grid
            .as_ref()
            .zip(self.resolved_terrain.as_ref())
            .and_then(|(zones, terrain)| {
                zones.get_zone_id_native(
                    terrain,
                    (seed.0 as u16, seed.1 as u16),
                    object.movement_zone,
                    entity.on_bridge(),
                )
            });
        let cell = self.nearby_passable_cell(
            seed,
            NearbyArgs::plain(
                PassabilityArgs {
                    speed_type,
                    required_zone_id: zone,
                    movement_zone: object.movement_zone,
                    bridge_aware_zone: entity.on_bridge(),
                },
                NearbyFootprint::SINGLE,
            ),
        )?;
        Some((cell.0 as i16, cell.1 as i16))
    }

    /// `MapClass::Find_Nearby_Passable_Cell` in the argument shape
    /// OnConstructionComplete's FreeUnit searches (`0x00446BF4..0x00446CCD`,
    /// `0x00446D42..0x00446E10`) and the Unit setter's Dock arm
    /// (`0x00743032..0x007430EE`) push: SpeedType 2 (Wheel), whatever the
    /// mover's own; the zone `MapClass::GetZoneID @ 0x0056D230` gives the seed
    /// for `movement_zone` (`Type+0x5B4`) without the bridge lookup, and that
    /// MovementZone; not bridge-aware; 1x1; the height gate on; no obstacle
    /// gate; bridge cells refused; a zero target cell, so selection is the
    /// frame-counter modulo and draws no RNG; no quadrant skip; no final
    /// occupancy check. `options` carries the overlay rejection. `None` when
    /// nothing passes or the map size is unknown.
    pub(crate) fn nearby_wheel_cell(
        &self,
        seed: (u16, u16),
        movement_zone: MovementZone,
        options: NearbySearchOptions,
    ) -> Option<(u16, u16)> {
        let zone = self
            .zone_grid
            .as_ref()
            .zip(self.resolved_terrain.as_ref())
            .and_then(|(zones, terrain)| {
                zones.get_zone_id_native(terrain, seed, movement_zone, false)
            });
        self.nearby_passable_cell(
            (i32::from(seed.0), i32::from(seed.1)),
            NearbyArgs {
                passability: PassabilityArgs {
                    speed_type: SpeedType::Wheel,
                    required_zone_id: zone,
                    movement_zone,
                    bridge_aware_zone: false,
                },
                footprint: NearbyFootprint::SINGLE,
                allow_bridge_cells: false,
                check_height: true,
                check_occupancy: false,
                target_cell: None,
                options,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::bridge_facts::{BRIDGE_FLAG_FORWARD_SIDE, BRIDGE_FLAG_STRUCTURAL};
    use crate::map::resolved_terrain::ResolvedTerrainGrid;

    #[test]
    fn retreat_directional_foot_search_matches_original_visits_and_null_result() {
        let corpus = crate::test_fixture::json("tools/team_retreat.json");
        let pair =
            |v: &serde_json::Value| (v[0].as_i64().unwrap() as i16, v[1].as_i64().unwrap() as i16);
        for row in corpus["nearby"].as_array().unwrap() {
            let mut visits = Vec::new();
            let accept = row["accept"].as_i64().unwrap();
            let result = find_nearby_foot_cell(
                pair(&row["source"]),
                pair(&row["target"]),
                row["minimum"].as_i64().unwrap() as i32..row["maximum"].as_i64().unwrap() as i32,
                |cell, direction| {
                    let admitted = visits.len() as i64 == accept;
                    visits.push((cell, direction));
                    admitted
                },
            );
            let expected: Vec<_> = row["visits"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| {
                    assert_eq!(v["height"], -1);
                    assert_eq!(v["previous"], 0);
                    assert_eq!(v["mode"], 1);
                    (pair(&v["cell"]), v["direction"].as_u64().unwrap() as u8)
                })
                .collect();
            assert_eq!(visits, expected, "{row}");
            assert_eq!(result.unwrap_or((0, 0)), pair(&row["result"]), "{row}");
            assert_eq!(
                result.is_some(),
                accept >= 0 && (accept as usize) < visits.len()
            );
        }
    }

    /// Clear cells carrying the retail Clear row, with the grid's land rows
    /// bound as a loaded scenario binds them.
    fn flat_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        crate::map::resolved_terrain::test_retail_clear_grid(width, height)
    }

    /// A grid whose every cell sits at the same raised terrain level — a plateau. The
    /// occlusion test is relative, so this must behave exactly like `flat_terrain`.
    fn plateau_terrain(width: u16, height: u16, level: u8) -> ResolvedTerrainGrid {
        let mut terrain = flat_terrain(width, height);
        for cell in terrain.cells_mut_for_test().iter_mut() {
            cell.level = level;
        }
        terrain
    }

    /// A grid that climbs one terrain level per south-east diagonal step, so EVERY cell
    /// is occluded by its immediate SE neighbour and no candidate is ever direct.
    fn diagonal_ramp_terrain(width: u16, height: u16) -> ResolvedTerrainGrid {
        let mut terrain = flat_terrain(width, height);
        for cell in terrain.cells_mut_for_test().iter_mut() {
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
            playfield_bounds: Some(covering_bounds()),
        }
    }

    /// A playfield diamond that admits every cell of these small grids, so the
    /// leaf tests isolate the predicates after the anchor gate.
    fn covering_bounds() -> crate::sim::cell_rect::PlayfieldBounds {
        crate::sim::cell_rect::PlayfieldBounds {
            base: 0,
            off_fc: -100,
            off_100: -100,
            off_104: 200,
            off_108: 200,
        }
    }

    #[test]
    fn nearby_raw_and_retained_seed_match_original_bounded_queries() {
        let cases: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/nearby_raw_occupation.json",
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
            let cell = &mut terrain.cells_mut_for_test()[10 * 21 + 10];
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
                        raw.occupant_at(RawCellKey::Dummy, layer)
                            .1
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
    fn find_nearby_ring_visit_order_matches_engine_segments() {
        // Square (Chebyshev) rings in the engine's fixed 4-segment order:
        //   seg1: for d=-r..=r  -> North (ox+d, oy-r) then South (ox+d, oy+r)
        //   seg2: for e=1-r..=r-1 -> West (ox-r, oy+e) then East (ox+r, oy+e)
        // Ring 0 emits the seed TWICE (N and S coincide at r==0); seg2 range is empty.
        assert_eq!(
            square_ring_cells((5, 5), 0).collect::<Vec<_>>(),
            [(5, 5), (5, 5)]
        );
        // Ring 1: seg1 d=-1,0,1 then seg2 e=0. Derived directly from the engine's
        // (ox+d, oy-r)/(ox+d, oy+r) and (ox-r, oy+e)/(ox+r, oy+e) sequence.
        assert_eq!(
            square_ring_cells((5, 5), 1).collect::<Vec<_>>(),
            [
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
        assert_eq!(square_ring_cells((5, 5), 2).count(), 16);
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
        terrain.cells_mut_for_test()[3 + 1].bridge_facts.raw_flags = BRIDGE_FLAG_FORWARD_SIDE;
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
                terrain.cells_mut_for_test()[probe * GRID + probe].level = rise as u8;
                assert_eq!(
                    direct_at(&terrain, (5, 5)),
                    expect_direct,
                    "step {step}: a rise of {rise} against a threshold of {threshold}"
                );
            }
        }
    }

    #[test]
    fn find_nearby_direct_ignores_the_candidates_own_bridge_bit() {
        // The projection reads terrain LEVELS only. A candidate's own bridge bit is read
        // by the height gate and the bridge filter, never by this test — folding it in
        // is what made the placeholder call every bridge cell on flat ground indirect.
        let mut terrain = flat_terrain(20, 20);
        terrain.cells_mut_for_test()[5 * 20 + 5]
            .bridge_facts
            .raw_flags = BRIDGE_FLAG_STRUCTURAL;
        assert!(direct_at(&terrain, (5, 5)));
    }

    #[test]
    fn find_nearby_bridge_projection_adds_exactly_four_probe_levels() {
        const GRID: usize = 20;
        let mut terrain = flat_terrain(GRID as u16, GRID as u16);
        terrain.cells_mut_for_test()[5 * GRID + 5]
            .bridge_facts
            .raw_flags = BRIDGE_FLAG_FORWARD_SIDE;
        terrain.cells_mut_for_test()[8 * GRID + 8]
            .bridge_facts
            .raw_flags = BRIDGE_FLAG_STRUCTURAL;

        assert!(
            direct_at(&terrain, (5, 5)),
            "step-three threshold is five, so a structural probe's +4 remains direct"
        );
        terrain.cells_mut_for_test()[8 * GRID + 8].level = 1;
        assert!(
            !direct_at(&terrain, (5, 5)),
            "the same +1 probe becomes +5 only after the exact four-level correction"
        );
    }

    #[test]
    fn find_nearby_bridge_projection_ignores_structural_probe_without_forward_candidate() {
        const GRID: usize = 20;
        let mut terrain = flat_terrain(GRID as u16, GRID as u16);
        terrain.cells_mut_for_test()[8 * GRID + 8].level = 1;
        terrain.cells_mut_for_test()[8 * GRID + 8]
            .bridge_facts
            .raw_flags = BRIDGE_FLAG_STRUCTURAL;

        assert!(
            direct_at(&terrain, (5, 5)),
            "probe structural bit is ignored while candidate 0x1000 is clear"
        );
        terrain.cells_mut_for_test()[5 * GRID + 5]
            .bridge_facts
            .raw_flags = BRIDGE_FLAG_FORWARD_SIDE;
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
        projected.cells_mut_for_test()[5 * GRID + 5]
            .bridge_facts
            .raw_flags = BRIDGE_FLAG_FORWARD_SIDE;
        projected.cells_mut_for_test()[6 * GRID + 6]
            .bridge_facts
            .raw_flags = BRIDGE_FLAG_STRUCTURAL;
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

        for (frame, expected) in square_ring_cells(seed, 1).enumerate() {
            assert_eq!(
                find_nearby_passable_cell(seed, &projected_query, frame as u32),
                cell_to_u16(expected),
                "frame modulo must walk the final direct pool in ring order"
            );
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
        let walls = crate::sim::overlay_grid::test_wall_overlays(&mut terrain, &[(5, 5)]);
        terrain.cells_mut_for_test()[5 * GRID + 5].level = 2;
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.overlay_grid = Some(&walls);

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

        let found = find_nearby_passable_cell((2, 2), &q, 0);
        // The seed (2,2) is occupied; FNPC must pick a different, free cell.
        assert!(found.is_some());
        assert_ne!(found, Some((2, 2)));
    }

    #[test]
    fn find_nearby_allow_bridge_filters_after_passability() {
        // A passable bridge cell at the seed is dropped when bridges are disallowed.
        let mut terrain = flat_terrain(3, 3);
        terrain.cells_mut_for_test()[3 + 1].bridge_facts.raw_flags = BRIDGE_FLAG_STRUCTURAL;
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
        let walls = crate::sim::overlay_grid::test_wall_overlays(&mut terrain, &[(2, 2)]); // seed rejected
        terrain.cells_mut_for_test()[5 + 2].level = 3; // (2,1): three levels above the seed
        terrain.cells_mut_for_test()[2 * 5 + 1].level = 1; // (1,2): one level above the seed
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.overlay_grid = Some(&walls);

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

    /// `Find_Nearby_Passable_Cell`'s target pick executed on each row's
    /// candidate array (`tools/spatial_oracle/sqrt_approx_distances.py`).
    #[test]
    fn nearest_to_target_matches_original_rows() {
        let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
            "tools/spatial_oracle/sqrt_approx_distances.json",
        ))
        .unwrap();
        let rows = corpus["nearest"].as_array().unwrap();
        for row in rows {
            let cells: Vec<(i32, i32)> = serde_json::from_value(row["candidates"].clone()).unwrap();
            let target: (i32, i32) = serde_json::from_value(row["target"].clone()).unwrap();
            let expected: (i32, i32) = serde_json::from_value(row["expected"].clone()).unwrap();
            let index = nearest_to_target(cells.iter().copied(), target).unwrap();
            assert_eq!(cells[index], expected, "{}", row["name"]);
        }
        assert_eq!(rows.len(), 6);
    }

    /// The final occupancy check tests the footprint's four corners against the
    /// playfield diamond (`CellRect::CheckOccupancy @ 0x00586780`); the anchor
    /// gate tests only the anchor. A 2x2 footprint whose anchor is inside the
    /// diamond but whose far corner is outside passes without the occupancy
    /// check and fails with it. Missing bounds reject every candidate.
    #[test]
    fn find_nearby_occupancy_rejects_a_footprint_corner_off_the_diamond() {
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
        q.footprint = NearbyFootprint::new(2, 2);
        q.radius_cap = 1;
        q.playfield_bounds = Some(bounds);
        // Anchor (13,12): sum 25, inside; far corner (14,13): sum 27, outside.
        assert_eq!(find_nearby_passable_cell((13, 12), &q, 0), Some((13, 12)));
        q.check_occupancy = true;
        assert_eq!(find_nearby_passable_cell((13, 12), &q, 0), None);
        q.check_occupancy = false;
        q.playfield_bounds = None;
        assert_eq!(
            find_nearby_passable_cell((13, 12), &q, 0),
            None,
            "missing bounds must not approximate MapClass with a rectangle"
        );
    }

    /// Every cell but `open` blocks a Track mover.
    fn track_blocked_but(width: u16, height: u16, open: &[(u16, u16)]) -> ResolvedTerrainGrid {
        let mut terrain = flat_terrain(width, height);
        for cell in terrain.cells_mut_for_test() {
            cell.ground_walk_blocked = true;
            cell.base_ground_walk_blocked = true;
            cell.speed_costs.track = Some(0);
            cell.base_speed_costs.track = Some(0);
        }
        for &(x, y) in open {
            let cell = terrain.cell_mut_for_test(x, y).expect("open cell");
            cell.ground_walk_blocked = false;
            cell.base_ground_walk_blocked = false;
            cell.speed_costs.track = Some(100);
            cell.base_speed_costs.track = Some(100);
        }
        terrain
    }

    /// The radius cap bounds the rings searched: a MapClass Size sum of 40
    /// caps at 32 and reaches ring 12; a cap of 12 scans rings 0 to 11 only;
    /// a nonpositive Size sum searches no ring.
    #[test]
    fn find_nearby_radius_cap_bounds_the_rings_searched() {
        let terrain = track_blocked_but(40, 40, &[(8, 8)]);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        for (size, found) in [((20, 20), Some((8, 8))), ((6, 6), None), ((-10, 10), None)] {
            q.radius_cap = map_owned_radius_cap(size.0, size.1);
            assert_eq!(
                find_nearby_passable_cell((20, 20), &q, 0),
                found,
                "{size:?}"
            );
        }
    }

    /// Each candidate's own playfield gate drops a passable cell inside the
    /// terrain rectangle but off the diamond, here the ring-two cell that
    /// ring order visits first, so frame 0 picks the other one.
    #[test]
    fn find_nearby_anchor_gate_skips_an_earlier_off_diamond_candidate() {
        // Diamond: pass iff 12 < x+y <= 26, x−y < 14, y−x < 6.
        let terrain = track_blocked_but(40, 40, &[(7, 2), (7, 6)]);
        let path_grid = PathGrid::from_resolved_terrain(&terrain);
        let mut q = base_query(&terrain, &path_grid);
        q.playfield_bounds = Some(crate::sim::cell_rect::PlayfieldBounds {
            base: 10,
            off_fc: 2,
            off_100: 1,
            off_104: 10,
            off_108: 6,
        });
        assert_eq!(find_nearby_passable_cell((9, 4), &q, 0), Some((7, 6)));
        q.playfield_bounds = Some(covering_bounds());
        assert_eq!(
            find_nearby_passable_cell((9, 4), &q, 0),
            Some((7, 2)),
            "without the gate, ring order puts (7,2) first"
        );
    }
}
