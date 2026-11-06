//! Start-position generation (`RandomMapGenerator::CreateStartingPoints @
//! 0x00594B50`): rebuilds the region registry over the final terrain,
//! distributes the start quota across the surviving regions, gathers and
//! selects candidate cells per region and writes the start waypoints. Its
//! map-type-0 tail, the tech-building coin flips, belongs to the tech phase;
//! [`mark_start_areas`] then floods a protected clearing around each start.
//!
//! Order within the phase is load-bearing for the draw stream: threshold,
//! zone recompute, teardown + rebuild, small-region deletion, score + sort,
//! quota split, per-region gather/select/write (one lane draw plus one
//! candidate draw per iteration).

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::{TruncF64, approx_sqrt_f32};
use crate::util::native_x87::X87Chop53;

use super::area;
use super::blob::{CellHeap, by_key};
use super::regions::{Regions, RmgRegion};
use super::zones::{ZoneField, diamond_frame_contains};

/// Region-size survival floor: 3% of the interior area but at least 400.
const AREA_FRACTION: f64 = 0.03;
const MIN_REGION_CELLS: f64 = 400.0;
/// Score base for the size-descending sort key.
const SCORE_BASE: i32 = 500_000;
/// Rounding offset in the cumulative quota split.
const HALF: f64 = 0.5;
/// Candidate-gather bounds: iterations cap and per-quota multiplier.
const GATHER_ITER_CAP: i32 = 300;
const GATHER_PER_QUOTA: i32 = 15;
/// Selector target-count constants: layout scale, spread, base slots.
const LAYOUT_SCALE: f64 = 0.01;
const LAYOUT_SPREAD: f64 = 12.0;
const BASE_SLOTS: f64 = 2.0;
/// Cross-region distance bonus in the selector.
const CROSS_REGION_BONUS: f64 = 20.0;
/// Greedy-selection sentinel for "no distance yet".
const MIN_DIST_INIT: f64 = 9_999_999.0;
/// Per-start clearing flood: heap capacity and pop cap.
const CLEARING_HEAP_CAP: usize = 800;
const CLEARING_POP_CAP: i32 = 400;

/// Phase inputs.
#[derive(Debug, Clone, Copy)]
pub struct StartsArgs {
    /// Global MapSeed NumPlayers (ABE028), read by CreateStartingPoints594DF0.
    pub start_quota: i32,
    /// `.SED NumPlayers` — bound of the per-start clearing floods only.
    pub num_players: i32,
    /// The TiberiumLayout option (drives the selector's slot target).
    pub tiberium_layout: i32,
    pub gen_w: i32,
    pub gen_h: i32,
    /// The padded map width (frame-test base).
    pub map_w: i32,
}

/// Everything the phase borrows.
pub struct StartsCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    pub rng: &'a mut RmgRng,
    pub regions: &'a mut Regions,
}

/// Phase result.
#[derive(Debug, Default)]
pub struct StartsOutcome {
    /// Waypoint slot -> cell. `None` = the owning bucket selected nothing
    /// (the original leaves the scenario slot unwritten in that case).
    pub waypoints: Vec<Option<(i16, i16)>>,
    /// Region-list indices in bucket order (ascending sort key), the order the
    /// map-type-0 tech coin flips walk.
    pub region_order: Vec<usize>,
    /// How many waypoint slots a region could not fill because its selection
    /// came up shorter than its quota.
    ///
    /// Non-zero means the map is short of spawns: those slots stay `None` and
    /// no start is placed for them. Carried out to `GeneratedMap` so a release
    /// build reports the condition rather than shipping it silently — without
    /// this the shortfall is invisible, because the only other signal was a
    /// `debug_assert!` that vanishes in release.
    pub unfilled_start_slots: usize,
}

pub fn run(ctx: &mut StartsCtx<'_>, args: &StartsArgs, zones: &ZoneField) -> StartsOutcome {
    // Survival threshold: max(genH * genW * 0.03, 400), truncated.
    let area = TruncF64::from_f64(f64::from(args.gen_h))
        .mul(TruncF64::from_f64(f64::from(args.gen_w)))
        .mul(TruncF64::from_f64(AREA_FRACTION));
    let floor = TruncF64::from_f64(MIN_REGION_CELLS);
    let threshold =
        X87Chop53::ftol_f64_low_masked(if area.lt(floor) { floor } else { area }.to_f64());

    rebuild_regions(ctx, zones);
    let order = assign_start_quotas(ctx.regions, threshold, args.start_quota);

    // Per-region gather + select + waypoint writes, with a running slot
    // offset that advances by the bucket quota whether or not the bucket
    // produced waypoints.
    let mut outcome = StartsOutcome {
        waypoints: vec![None; args.start_quota.max(0) as usize],
        region_order: order.clone(),
        unfilled_start_slots: 0,
    };
    let mut offset = 0i32;
    for &index in &order {
        place_region_starts(ctx, args, index, offset, &mut outcome);
        offset += ctx.regions.list[index].start_quota;
    }

    // The original ends by publishing start/extent preview metadata into the
    // scenario (packed cells plus isometric pixel mins/extents). Nothing in
    // the port reads it yet; the emitter task owns that surface.
    outcome
}

/// The small-region deletion, score sort and cumulative quota split
/// (`CreateStartingPoints` 0x00594C47..0x00594E52). Returns the surviving
/// region indices in bucket order.
fn assign_start_quotas(regions: &mut Regions, threshold: i32, start_quota: i32) -> Vec<usize> {
    // Delete undersized regions, iterating downward so removals cannot skip
    // entries. The multiplier of the later sort key is captured BEFORE the
    // deletions.
    let pre_deletion_count = regions.list.len() as i32;
    for index in (0..regions.list.len()).rev() {
        if regions.list[index].cell_count < threshold {
            regions.list.remove(index);
        }
    }

    // Score and sort: ascending key = descending size, unique ids break ties.
    let mut order: Vec<(i32, usize)> = regions
        .list
        .iter()
        .enumerate()
        .map(|(index, region)| {
            let key = (SCORE_BASE.wrapping_sub(region.cell_count))
                .wrapping_mul(pre_deletion_count)
                .wrapping_add(region.id);
            (key, index)
        })
        .collect();
    order.sort_unstable();

    // Cumulative-fraction quota split; the last bucket takes the remainder.
    let total: i32 = order
        .iter()
        .map(|&(_, index)| regions.list[index].cell_count)
        .sum();
    let mut cumulative = 0i32;
    let mut assigned = 0i32;
    for (position, &(_, index)) in order.iter().enumerate() {
        cumulative += regions.list[index].cell_count;
        let quota = if position == order.len() - 1 {
            start_quota - assigned
        } else {
            X87Chop53::ftol_f64_low_masked(
                TruncF64::from_f64(f64::from(cumulative))
                    .div(TruncF64::from_f64(f64::from(total)))
                    .mul(TruncF64::from_f64(f64::from(start_quota)))
                    .add(TruncF64::from_f64(HALF))
                    .sub(TruncF64::from_f64(f64::from(assigned)))
                    .to_f64(),
            )
        };
        regions.list[index].start_quota = quota;
        assigned += quota;
    }

    order.into_iter().map(|(_, index)| index).collect()
}

/// Teardown + fresh region build over the final map: every unclaimed
/// reference-ground cell in native scan order seeds a flood fill whose
/// popped cells are appended to the region's cell list.
fn rebuild_regions(ctx: &mut StartsCtx<'_>, zones: &ZoneField) {
    // Teardown `RmgRegion::ClearAll @ 0x0058CE90`.
    ctx.scratch.reset_region_ids();
    ctx.regions.list.clear();
    ctx.regions.id_counter = 0;

    let coords: Vec<(i32, i32)> = ctx.grid.native_cells().collect();
    for &(x, y) in &coords {
        if ctx.scratch.get(x, y).region != -1 || !zones.is_reference_ground(x, y) {
            continue;
        }
        let id = ctx.regions.id_counter;
        ctx.regions.id_counter += 1;
        // `RmgRegion::RmgRegion @ 0x0058BF70`: the seed's level and its
        // Cell 0x004865D0 water test.
        let seed = ctx.grid.get(x, y);
        let mut region = RmgRegion::rebuilt(
            id,
            seed.map_or(0, |cell| cell.level),
            seed.is_some_and(|cell| ctx.ids.is_water_shore_or_waterfall(cell.tile)),
            0,
        );

        // LIFO flood `RmgRegion::FloodFromCell @ 0x00594420`: stamp at
        // enqueue, claim + append at pop.
        ctx.scratch.get_mut(x, y).stamp = id;
        let mut stack: Vec<(i16, i16)> = vec![(x as i16, y as i16)];
        while let Some((cx, cy)) = stack.pop() {
            let (cx32, cy32) = (i32::from(cx), i32::from(cy));
            ctx.scratch.get_mut(cx32, cy32).region = id;
            region.cell_count += 1;
            region.cells.push((cx, cy));
            for dir in 0..8 {
                let (nx, ny) = RmgGrid::step(cx32, cy32, dir);
                // Out-of-band lookups resolve to the border cell, whose
                // stamped coordinate always fails the diamond test.
                if !ctx.scratch.in_diamond(nx, ny) {
                    continue;
                }
                if !zones.is_reference_ground(nx, ny) {
                    continue;
                }
                if ctx.scratch.get(nx, ny).stamp != -1 {
                    continue;
                }
                ctx.scratch.get_mut(nx, ny).stamp = id;
                stack.push((nx as i16, ny as i16));
            }
        }
        ctx.regions.list.push(region);
    }
}

/// The lane draw (0x005948D2): `ftol(draw * [0x007ED8C0])`, redrawn while
/// above 2. That constant is exactly `3 * K`, and `draw * 3` is exact, so
/// this rounds the same product as `uniform(0, 2)`.
fn lane_draw(rng: &mut RmgRng) -> i32 {
    rng.uniform(0, 2)
}

/// The 6x6 passability window around a candidate.
///
/// The routine behind it is shared with the low-bridge placer's corridor walk —
/// **the same function in the original**, not merely a similar one. Both
/// override bytes are zero on this path: the register carrying the road-end
/// override is cleared before the gather loop and only written again after it,
/// so roads and road-ends reject on every iteration.
fn gate_6x6(ctx: &StartsCtx<'_>, x: i32, y: i32) -> bool {
    area::area_is_paved_clear(ctx.grid, ctx.scratch, ctx.ids, (x - 3, y - 3, 6, 6))
}

/// `RmgRegion::PlaceStartingPoints @ 0x00594870` (from
/// `CreateStartingPoints` at 0x00594E66): gather candidates for one region,
/// select, and write its waypoint slots. Lane draw 0x005948D2; up to 300
/// gather iterations (0x00594910), each drawing a cell (0x0059493A) through
/// the 6x6 test (0x00594992) and `MapClass::IsCellInViewRegion @
/// 0x00578640`; the quota-bounded write loop 0x00594A76..0x00594ADD, then
/// the count-guarded drain 0x00594AEA.
fn place_region_starts(
    ctx: &mut StartsCtx<'_>,
    args: &StartsArgs,
    region_index: usize,
    offset: i32,
    outcome: &mut StartsOutcome,
) {
    // The view frame the candidates must sit in: the local rect inset by
    // (4, 4, -8, -8).
    let view_rect = [6, 9, args.gen_w - 8, args.gen_h - 8];

    let lane = lane_draw(ctx.rng);
    let quota = ctx.regions.list[region_index].start_quota;
    let wanted = lane + quota * GATHER_PER_QUOTA;

    let mut buffer: Vec<(i16, i16)> = Vec::new();
    let mut iterations = 0;
    while (buffer.len() as i32) < wanted && iterations < GATHER_ITER_CAP {
        iterations += 1;
        let count = ctx.regions.list[region_index].cells.len() as i32;
        debug_assert!(count > 0, "regions always own at least their seed");
        let pick = ctx.rng.uniform(0, count - 1);
        let (cx, cy) = ctx.regions.list[region_index].cells[pick as usize];
        let (x, y) = (i32::from(cx), i32::from(cy));
        if !gate_6x6(ctx, x, y) {
            continue;
        }
        let cell = ctx.grid.get(x, y).expect("region cells are in-band");
        if !diamond_frame_contains(args.map_w, view_rect, x, y, cell.level, cell.slope != 0) {
            continue;
        }
        // Duplicates are allowed: repeated draws of the same cell append
        // repeated entries.
        buffer.push((cx, cy));
    }

    // Returning here cannot desynchronise the draw stream: `select_candidates`
    // borrows `ctx` immutably, so it is incapable of drawing at all — the type
    // system enforces that, not a convention that could quietly be broken.
    let Some(mut selection) = select_candidates(ctx, args, region_index, &buffer) else {
        return;
    };

    for slot_index in 0..quota {
        let slot = (offset + slot_index) as usize;
        let Some(&cell) = selection.get(slot_index as usize) else {
            // The original has nothing here worth copying, and its two starved
            // cases differ. Selecting nothing at all walks a null array pointer
            // and faults; a short selection reads uninitialised heap, whose
            // contents follow the process's allocation history rather than the
            // seed, so even two generations in one session disagree. Neither is
            // reproducible, so stopping short is a deliberate divergence —
            // counted and reported, never claimed as parity.
            outcome.unfilled_start_slots += (quota - slot_index) as usize;
            break;
        };
        if let Some(way) = outcome.waypoints.get_mut(slot) {
            *way = Some(cell);
        }
        if let Some(grid_cell) = ctx.grid.get_mut(i32::from(cell.0), i32::from(cell.1)) {
            grid_cell.start_marker = true;
        }
    }

    // Consume the written entries from the front; the leftover selection
    // becomes the region's tiberium field slots.
    let consumed = (quota.max(0) as usize).min(selection.len());
    selection.drain(..consumed);
    ctx.regions.list[region_index].field_slots = Some(selection);
}

/// `RmgRegion::SelectStartCandidates @ 0x00594F40`, the best-first
/// selector: target count from the tiberium layout, a
/// max-distance pair seed, then farthest-point-first growth. Distances are
/// the table square root plus a +20 bonus between cells whose region stamps
/// differ (inert here — one bucket's candidates share a region — but the
/// mechanism is the original's).
fn select_candidates(
    ctx: &StartsCtx<'_>,
    args: &StartsArgs,
    region_index: usize,
    buffer: &[(i16, i16)],
) -> Option<Vec<(i16, i16)>> {
    let quota = ctx.regions.list[region_index].start_quota;
    let count = buffer.len() as i32;

    let mut target = X87Chop53::ftol_f64_low_masked(
        TruncF64::from_f64(f64::from(args.tiberium_layout))
            .mul(TruncF64::from_f64(LAYOUT_SCALE))
            .mul(TruncF64::from_f64(LAYOUT_SPREAD))
            .div(TruncF64::from_f64(f64::from(args.start_quota)))
            .add(TruncF64::from_f64(BASE_SLOTS))
            .mul(TruncF64::from_f64(f64::from(quota)))
            .to_f64(),
    );
    if quota == 0 && count > 0 {
        target = count;
    }
    if count < target || target == 0 {
        target = count;
        if quota == 0 {
            return None;
        }
    }

    let stamp = |cell: (i16, i16)| ctx.scratch.get(i32::from(cell.0), i32::from(cell.1)).region;
    let dist = |a: (i16, i16), b: (i16, i16)| {
        let dx = i32::from(a.0) - i32::from(b.0);
        let dy = i32::from(a.1) - i32::from(b.1);
        let base = f64::from(approx_sqrt_f32(dy * dy + dx * dx));
        if stamp(a) != stamp(b) {
            base + CROSS_REGION_BONUS
        } else {
            base
        }
    };

    let mut selection: Vec<(i16, i16)> = Vec::new();
    if count >= 2 {
        // Pair scan: the first element of the farthest pair seeds the list.
        let mut best = (-1.0f64, usize::MAX);
        for i in 0..buffer.len() - 1 {
            for j in i + 1..buffer.len() {
                let d = dist(buffer[i], buffer[j]);
                if best.0 < d {
                    best = (d, i);
                }
            }
        }
        selection.push(buffer[best.1]);
    } else if count == 1 {
        // Single candidate short-circuits: no greedy growth at all.
        selection.push(buffer[0]);
        return Some(selection);
    }

    while (selection.len() as i32) < target {
        // Farthest-point-first: pick the candidate maximising its minimum
        // distance to everything already selected. Already-selected cells
        // stay in the pool (min distance zero keeps them un-picked until
        // duplicates are all that is left).
        let mut best = (-1.0f64, usize::MAX);
        for (index, &candidate) in buffer.iter().enumerate() {
            let mut min_dist = MIN_DIST_INIT;
            for &selected in &selection {
                let d = dist(candidate, selected);
                if d < min_dist {
                    min_dist = d;
                }
            }
            if best.0 < min_dist {
                best = (min_dist, index);
            }
        }
        if best.1 == usize::MAX {
            break;
        }
        selection.push(buffer[best.1]);
    }
    Some(selection)
}

/// Per-start protected clearings (`RandomMapGenerator::MarkStartAreas @
/// 0x005A1FB0`, which `Generate` calls after `CreateStartingPoints`): a
/// closest-first flood (distance keys from the start cell) over clear tiles,
/// marking up to 400 cells around each of the first `num_players` waypoints.
pub fn mark_start_areas(ctx: &mut StartsCtx<'_>, args: &StartsArgs, outcome: &StartsOutcome) {
    let coords: Vec<(i32, i32)> = ctx.grid.native_cells().collect();
    for player in 0..args.num_players.max(0) {
        let marker = player + 1;
        // The whole stamp plane resets to zero before every flood.
        for &(x, y) in &coords {
            ctx.scratch.get_mut(x, y).stamp = 0;
        }
        let Some(Some(seed)) = outcome.waypoints.get(player as usize) else {
            // The original reads whatever the scenario slot holds even when
            // no bucket wrote it; an unwritten slot has no defined content
            // to reproduce, so the flood is skipped.
            continue;
        };
        let seed = (i32::from(seed.0), i32::from(seed.1));
        ctx.scratch.get_mut(seed.0, seed.1).stamp = marker;
        let mut heap = CellHeap::new(CLEARING_HEAP_CAP);
        heap.push((0.0, seed), by_key);

        let mut pops = 0;
        while pops < CLEARING_POP_CAP {
            let Some((_, (x, y))) = heap.pop(by_key) else {
                break;
            };
            pops += 1;
            ctx.scratch.get_mut(x, y).water_lock = true;
            for dir in 0..8 {
                let (nx, ny) = RmgGrid::step(x, y, dir);
                if !ctx.scratch.in_diamond(nx, ny) {
                    continue;
                }
                if ctx.scratch.get(nx, ny).stamp != 0 {
                    continue;
                }
                let clear = ctx
                    .grid
                    .get(nx, ny)
                    .is_some_and(|cell| ctx.ids.is_clear(cell.tile));
                if !clear {
                    continue;
                }
                let (dx, dy) = (nx - seed.0, ny - seed.1);
                let key = approx_sqrt_f32(dx * dx + dy * dy);
                // The stamp is taken even when the heap is full and drops
                // the node.
                ctx.scratch.get_mut(nx, ny).stamp = marker;
                heap.push((key, (nx, ny)), by_key);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock, TileBlocks};
    use crate::map::rmg::phases::zones::{self, ZoneKind, ZoneParams};
    use crate::map::theater::TheaterCliffRanges;

    struct ClearBlocks;

    impl TileBlocks for ClearBlocks {
        fn block(&self, tile: i32) -> Option<&TileBlock> {
            static CLEAR: std::sync::OnceLock<TileBlock> = std::sync::OnceLock::new();
            static WATER: std::sync::OnceLock<TileBlock> = std::sync::OnceLock::new();
            let block = |terrain: u8| TileBlock {
                width: 1,
                height: 1,
                subtiles: vec![Some(SubTile {
                    height: 0,
                    terrain,
                    slope: 0,
                })],
            };
            Some(if (500..600).contains(&tile) {
                WATER.get_or_init(|| block(9))
            } else {
                CLEAR.get_or_init(|| block(0))
            })
        }
    }

    fn ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: -1,
            ramp_smooth: -1,
            slope_set_pieces: -1,
            rough: -1,
            sand: -1,
            green: 100,
            rough_lat: -1,
            sand_lat: -1,
            green_lat: 110,
            pave_lat: -1,
            pave: -1,
            water_base: 500,
            shore: 400,
            water_bridge: -1,
            misc_pave: -1,
            paved_roads: -1,
            paved_road_ends: -1,
            medians: -1,
            cliffs: TheaterCliffRanges::default(),
        }
    }

    fn args() -> StartsArgs {
        StartsArgs {
            start_quota: 4,
            num_players: 4,
            tiberium_layout: 50,
            gen_w: 40,
            gen_h: 36,
            map_w: 44,
        }
    }

    fn world(args: &StartsArgs) -> (RmgGrid, RmgScratch) {
        let (map_w, map_h) = (args.gen_w + 4, args.gen_h + 12);
        let stride = (map_w + map_h + 1) as usize;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = RmgGrid::new(stride, dmin, dmax);
        let scratch = RmgScratch::new(stride, dmin, dmax);
        let coords: Vec<(i32, i32)> = grid.native_cells().collect();
        for (x, y) in coords {
            grid.get_mut(x, y).unwrap().tile = 0;
        }
        (grid, scratch)
    }

    fn zone_field(grid: &RmgGrid, args: &StartsArgs) -> zones::ZoneField {
        let mut wheel = [false; zones::LAND_TYPES];
        wheel[3] = true;
        zones::compute(
            grid,
            &ClearBlocks,
            &ZoneParams {
                map_w: args.map_w,
                local_rect: [2, 5, args.gen_w, args.gen_h],
                wheel_impassable: wheel,
                cliff_back_impassability: 2,
            },
            ZoneKind::Amphibious,
        )
    }

    fn run_phase(seed: u16) -> (StartsOutcome, Regions, RmgGrid, RmgScratch) {
        let args = args();
        let (mut grid, mut scratch) = world(&args);
        let identity = ids();
        let field = zone_field(&grid, &args);
        let mut rng = RmgRng::new(seed);
        let mut regions = Regions::default();
        let outcome = {
            let mut ctx = StartsCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                rng: &mut rng,
                regions: &mut regions,
            };
            let outcome = run(&mut ctx, &args, &field);
            mark_start_areas(&mut ctx, &args, &outcome);
            outcome
        };
        (outcome, regions, grid, scratch)
    }

    #[test]
    fn the_phase_is_deterministic() {
        let snapshot = |seed| {
            let (outcome, regions, _, scratch) = run_phase(seed);
            let quotas: Vec<i32> = regions.list.iter().map(|r| r.start_quota).collect();
            let slots: Vec<Option<Vec<(i16, i16)>>> =
                regions.list.iter().map(|r| r.field_slots.clone()).collect();
            let locks = scratch
                .cells()
                .iter()
                .filter(|record| record.water_lock)
                .count();
            (outcome.waypoints.clone(), quotas, slots, locks)
        };
        assert_eq!(snapshot(2024), snapshot(2024));
    }

    fn int(value: &serde_json::Value) -> i32 {
        value.as_i64().unwrap() as i32
    }

    fn pairs(value: &serde_json::Value) -> Vec<(i16, i16)> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (int(&c[0]) as i16, int(&c[1]) as i16))
            .collect()
    }

    /// The harness scene on its 16x16 map: clear level-0 cells, then the
    /// declared tile families, `[level, slope]` and scratch owners.
    fn native_scene(doc: &serde_json::Value, input: &serde_json::Value) -> (RmgGrid, RmgScratch) {
        let mut grid = RmgGrid::new(33, 16, 48);
        let mut scratch = RmgScratch::new(33, 16, 48);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            let cell = grid.get_mut(x, y).unwrap();
            (cell.tile, cell.sub_tile, cell.level, cell.slope) = (0, 0, 0, 0);
        }
        let list = |key: &str| input.get(key).map_or(&[][..], |v| v.as_array().unwrap());
        for c in list("tiles") {
            let family = c[2].as_str().unwrap();
            grid.get_mut(int(&c[0]), int(&c[1])).unwrap().tile =
                int(&doc["scene"]["tiles"][family]);
        }
        for c in list("rough") {
            grid.get_mut(int(&c[0]), int(&c[1])).unwrap().tile =
                int(&doc["scene"]["tiles"]["rough"]);
        }
        for c in list("terrain") {
            let cell = grid.get_mut(int(&c[0]), int(&c[1])).unwrap();
            (cell.level, cell.slope) = (int(&c[2]) as u8, int(&c[3]) as u8);
        }
        for c in list("regions") {
            scratch.get_mut(int(&c[0]), int(&c[1])).region = int(&c[2]);
        }
        (grid, scratch)
    }

    fn native_ids(doc: &serde_json::Value) -> TileIds {
        let tile = |name: &str| int(&doc["scene"]["tiles"][name]);
        TileIds {
            paved_roads: tile("paved_roads"),
            paved_road_ends: tile("paved_road_ends"),
            misc_pave: tile("misc_pave"),
            pave: tile("pave"),
            ..ids()
        }
    }

    fn assert_native_rng(doc: &serde_json::Value, row: &serde_json::Value, rng: RmgRng) {
        assert_eq!(
            rng.into_continuation().into_native_parts(),
            super::super::cliff_native_tests::native_state(doc, &row["rng_after"])
                .into_native_parts(),
            "{}: exit RNG",
            row["input"]["id"]
        );
    }

    /// Whole original `RmgRegion::PlaceStartingPoints @ 0x00594870` calls
    /// from `tools/rmg_oracle/start_points.py`: waypoint slots, start flags,
    /// the drained field slots and the exit RNG.
    #[test]
    fn native_place_starting_points_controls() {
        let doc = crate::test_fixture::json("tools/rmg_oracle/start_points.json");
        let identity = native_ids(doc);
        let rows = doc["place"].as_array().unwrap();
        assert_eq!(rows.len(), 9);
        for row in rows {
            let input = &row["input"];
            let context = input["id"].as_str().unwrap();
            let (mut grid, mut scratch) = native_scene(doc, input);
            let cells = pairs(&input["cells"]);
            let mut regions = Regions::default();
            regions.list.push(RmgRegion {
                id: 0,
                level: 0,
                active: false,
                cell_count: cells.len() as i32,
                cells,
                start_quota: int(&input["quota"]),
                field_slots: None,
            });
            let args = StartsArgs {
                start_quota: int(&input["start_quota"]),
                num_players: int(&input["start_quota"]),
                tiberium_layout: int(&input["layout"]),
                gen_w: int(&input["gen"][0]),
                gen_h: int(&input["gen"][1]),
                map_w: 16,
            };
            let slots = row["waypoints"].as_array().unwrap().len();
            let mut outcome = StartsOutcome {
                waypoints: vec![None; slots],
                ..Default::default()
            };
            let mut rng = RmgRng::new(input["seed"].as_u64().unwrap() as u16);
            let mut ctx = StartsCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                rng: &mut rng,
                regions: &mut regions,
            };
            place_region_starts(&mut ctx, &args, 0, int(&input["offset"]), &mut outcome);
            assert_eq!(outcome.unfilled_start_slots, 0, "{context}: unfilled");
            let waypoints: Vec<(i16, i16)> = outcome
                .waypoints
                .iter()
                .map(|slot| slot.unwrap_or((-1, -1)))
                .collect();
            assert_eq!(waypoints, pairs(&row["waypoints"]), "{context}: waypoints");
            let flags: Vec<(i16, i16)> = (0..33)
                .flat_map(|y| (0..33).map(move |x| (x, y)))
                .filter(|&(x, y)| grid.get(x, y).is_some_and(|c| c.start_marker))
                .map(|(x, y)| (x as i16, y as i16))
                .collect();
            assert_eq!(flags, pairs(&row["start_flags"]), "{context}: start flags");
            let native_slots = (!row["field_slots"].is_null()).then(|| pairs(&row["field_slots"]));
            assert_eq!(
                regions.list[0].field_slots, native_slots,
                "{context}: field slots"
            );
            assert_native_rng(doc, row, rng);
        }
    }

    /// Whole original `RandomMapGenerator::MarkStartAreas @ 0x005A1FB0`
    /// calls from the same harness: every changed scratch stamp and lock.
    #[test]
    fn native_mark_start_areas_controls() {
        let doc = crate::test_fixture::json("tools/rmg_oracle/start_points.json");
        let identity = native_ids(doc);
        let rows = doc["mark"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        for row in rows {
            let input = &row["input"];
            let context = input["id"].as_str().unwrap();
            let (mut grid, mut scratch) = native_scene(doc, input);
            let record = |scratch: &RmgScratch, x: i32, y: i32| {
                let cell = scratch.get(x, y);
                [cell.stamp, i32::from(cell.water_lock)]
            };
            let diamond: Vec<(i32, i32)> = (0..33)
                .flat_map(|y| (0..33).map(move |x| (x, y)))
                .filter(|&(x, y)| scratch.in_diamond(x, y))
                .collect();
            let prior: Vec<[i32; 2]> = diamond
                .iter()
                .map(|&(x, y)| record(&scratch, x, y))
                .collect();
            let players = int(&input["players"]);
            let args = StartsArgs {
                start_quota: players,
                num_players: players,
                tiberium_layout: 0,
                gen_w: 12,
                gen_h: 12,
                map_w: 16,
            };
            let outcome = StartsOutcome {
                waypoints: pairs(&input["waypoints"]).into_iter().map(Some).collect(),
                ..Default::default()
            };
            let mut rng = RmgRng::new(input["seed"].as_u64().unwrap() as u16);
            let mut regions = Regions::default();
            let mut ctx = StartsCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                rng: &mut rng,
                regions: &mut regions,
            };
            mark_start_areas(&mut ctx, &args, &outcome);
            let changed: Vec<[i32; 4]> = diamond
                .iter()
                .zip(prior)
                .filter_map(|(&(x, y), before)| {
                    let now = record(&scratch, x, y);
                    (now != before).then_some([x, y, now[0], now[1]])
                })
                .collect();
            let native: Vec<[i32; 4]> = row["records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| std::array::from_fn(|i| int(&c[i])))
                .collect();
            assert_eq!(changed, native, "{context}: records");
            assert_eq!(int(&row["draws"]), 0, "{context}: draws");
            assert_native_rng(doc, row, rng);
        }
    }

    /// Two slices of original `RandomMapGenerator::CreateStartingPoints @
    /// 0x00594B50` from `tools/rmg_oracle/create_starting_points.py`, with
    /// declared zones: the teardown and region rebuild (0x00594B50..
    /// 0x00594C47) on the fixture's physical tiles, and the deletion, sort and
    /// quota split (0x00594C47..0x00594E52) on declared region records.
    #[test]
    fn native_create_starting_points_controls() {
        let doc = crate::test_fixture::json("tools/rmg_oracle/create_starting_points.json");
        let (mut identity, _) = super::super::cliff_native_tests::physical_tiles();
        identity.water_base = int(&doc["scene"]["water_base"]);
        let rebuild = doc["rebuild"].as_array().unwrap();
        assert_eq!(rebuild.len(), 5);
        for row in rebuild {
            let input = &row["input"];
            let context = input["id"].as_str().unwrap();
            let set = |key: &str| pairs(&input[key]).into_iter().collect::<Vec<_>>();
            let (ground, typed) = (set("ground"), set("typed"));
            let mut grid = RmgGrid::new(33, 16, 48);
            let mut scratch = RmgScratch::new(33, 16, 48);
            for (x, y) in grid.native_cells().collect::<Vec<_>>() {
                let cell = grid.get_mut(x, y).unwrap();
                (cell.tile, cell.level) = (0, 0);
            }
            for c in input["terrain"].as_array().unwrap() {
                grid.get_mut(int(&c[0]), int(&c[1])).unwrap().level = int(&c[2]) as u8;
            }
            for (x, y) in set("water") {
                grid.get_mut(i32::from(x), i32::from(y)).unwrap().tile = identity.water_base;
            }
            let has = |cells: &[(i16, i16)], x: i32, y: i32| cells.contains(&(x as i16, y as i16));
            let zones =
                ZoneField::declared(33, |x, y| has(&typed, x, y), |x, y| has(&ground, x, y));
            let mut regions = Regions::default();
            let mut rng = RmgRng::new(1);
            let mut ctx = StartsCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                rng: &mut rng,
                regions: &mut regions,
            };
            rebuild_regions(&mut ctx, &zones);
            let listed: Vec<serde_json::Value> = regions
                .list
                .iter()
                .map(|r| {
                    let cells: Vec<[i16; 2]> = r.cells.iter().map(|&(x, y)| [x, y]).collect();
                    serde_json::json!([
                        r.id,
                        r.cell_count,
                        r.level,
                        u8::from(r.active),
                        r.start_quota,
                        cells
                    ])
                })
                .collect();
            assert_eq!(
                &listed[..],
                row["regions"].as_array().unwrap(),
                "{context}: regions"
            );
            assert_eq!(
                regions.id_counter,
                int(&row["next_region_id"]),
                "{context}: next id"
            );
            let mut records = Vec::new();
            for y in 0..33 {
                for x in 0..33 {
                    let record = scratch.get(x, y);
                    if scratch.in_diamond(x, y) && (record.region, record.stamp) != (-1, -1) {
                        records.push(serde_json::json!([x, y, record.region, record.stamp]));
                    }
                }
            }
            assert_eq!(
                &records[..],
                row["records"].as_array().unwrap(),
                "{context}: records"
            );
            // The rebuild draws nothing.
            assert_native_rng(&doc, row, rng);
        }
        let split = doc["split"].as_array().unwrap();
        assert_eq!(split.len(), 6);
        for row in split {
            let input = &row["input"];
            let context = input["id"].as_str().unwrap();
            let mut regions = Regions::default();
            for r in input["regions"].as_array().unwrap() {
                regions.list.push(RmgRegion {
                    id: int(&r[0]),
                    level: 0,
                    active: false,
                    cell_count: int(&r[1]),
                    cells: Vec::new(),
                    start_quota: 0,
                    field_slots: None,
                });
            }
            let order =
                assign_start_quotas(&mut regions, int(&input["threshold"]), int(&input["quota"]));
            let ids = |values: &serde_json::Value| -> Vec<i32> {
                values.as_array().unwrap().iter().map(int).collect()
            };
            let survivors: Vec<i32> = regions.list.iter().map(|r| r.id).collect();
            assert_eq!(survivors, ids(&row["survivors"]), "{context}: survivors");
            let sorted: Vec<i32> = order.iter().map(|&i| regions.list[i].id).collect();
            assert_eq!(sorted, ids(&row["order"]), "{context}: order");
            let quotas: Vec<i32> = order.iter().map(|&i| regions.list[i].start_quota).collect();
            assert_eq!(quotas, ids(&row["quotas"]), "{context}: quotas");
            // Nor do the deletion, sort and split (the scene seeds 1).
            assert_native_rng(&doc, row, RmgRng::new(1));
        }
    }
}
