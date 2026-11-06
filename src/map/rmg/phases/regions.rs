//! Region partition: groups same-level, same-class connected cells into
//! region objects that the starts, tech and tiberium phases consume.
//!
//! Order: reset -> seed scan (water/green-class cells) -> per-region
//! multi-pass expansion + water propagation -> fallback seeding of every
//! remaining cell. The flood fill is a LIFO stack walk with claim-at-enqueue;
//! small blobs of the plain-land class get dissolved or merged into a
//! neighbor instead of becoming regions.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::phases::blob::{CellHeap, by_key};
use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::{Gaussian, TruncF64, approx_sqrt, narrow_to_f32};
use crate::util::direction_tables::native_atan_from_table;
use crate::util::native_x87::X87Chop53;

/// Pop-count threshold at or below which a plain-class blob is dissolved or
/// merged rather than registered.
const SMALL_BLOB_POPS: i32 = 0x4A;
/// Large-region threshold for the extra expansion pass.
const LARGE_REGION_CELLS: i32 = 8000;

/// One region object (the fields later phases read).
#[derive(Debug, Clone)]
pub struct RmgRegion {
    pub id: i32,
    /// Seed cell's level; expansion claims only level-matching cells.
    pub level: u8,
    /// Class flag: seed was water-ish or green (drives the expansion loop).
    pub active: bool,
    pub cell_count: i32,
    /// Per-region cell list, in descending linear-cell order: rebuilt by the
    /// region split (map types 3/4) and by the starts phase, not by the
    /// partition itself.
    pub cells: Vec<(i16, i16)>,
    /// Start slots assigned by the starts phase.
    pub start_quota: i32,
    /// Tiberium field slots selected by the starts phase. `None` = the
    /// selector produced no list for this region (its region+0x00 stays null);
    /// `Some(vec)` = a list (possibly empty after the waypoints were removed).
    /// The tiberium driver treats the two differently — a `None` region is
    /// skipped before any RNG, a `Some` region runs the gem-anchor draw.
    pub field_slots: Option<Vec<(i16, i16)>>,
}

/// The region registry: creation-ordered objects plus the id counter.
#[derive(Debug, Default)]
pub struct Regions {
    pub list: Vec<RmgRegion>,
    pub id_counter: i32,
}

impl RmgRegion {
    /// A region produced by the inland/mountainous rebuild, which derives all
    /// of its fields from the flood rather than from the seed scan.
    pub fn rebuilt(id: i32, level: u8, active: bool, cell_count: i32) -> Self {
        Self {
            id,
            level,
            active,
            cell_count,
            cells: Vec::new(),
            start_quota: 0,
            field_slots: None,
        }
    }
}

pub struct RegionCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    pub rng: &'a mut RmgRng,
    pub map_type: i32,
    /// Fallback level for dissolve writes when the adopted level reads 0xFF.
    pub default_level: u8,
}

/// Water-ish class half. This is the exact tile-only family predicate called
/// by the native seed scan, flood fill, and first-region dissolve path.
fn water_ish(ids: &TileIds, tile: i32) -> bool {
    ids.is_water_shore_or_waterfall(tile)
}

/// The region class bit: water-ish OR green membership.
fn region_class(ids: &TileIds, tile: i32) -> bool {
    water_ish(ids, tile) || ids.is_green_lat(tile)
}

pub fn run(ctx: &mut RegionCtx<'_>) -> Regions {
    let mut regions = Regions::default();
    ctx.scratch.reset_region_ids();

    // Seed scan: linear scratch order, class cells only.
    let width = ctx.scratch.width();
    for index in 0..width * width {
        let record = ctx.scratch.cells()[index];
        let coord = (i32::from(record.x), i32::from(record.y));
        if record.region != -1 || coord == (0, 0) {
            continue;
        }
        let tile = ctx.grid.cell_native(coord.0, coord.1).tile;
        if region_class(ctx.ids, tile) {
            flood_fill(ctx, &mut regions, coord);
        }
    }

    // Expansion over the snapshot of seeded regions.
    let mode34 = matches!(ctx.map_type, 3 | 4);
    let seeded = regions.list.len();
    for index in 0..seeded {
        if !regions.list[index].active {
            continue;
        }
        let passes =
            4 + i32::from(regions.list[index].cell_count > LARGE_REGION_CELLS) + i32::from(!mode34);
        let (id, level) = (regions.list[index].id, regions.list[index].level);
        let _ = expand(ctx, id, level, passes);
        water_propagate(ctx, id);
    }

    // Fallback: every remaining unassigned cell seeds a region.
    for index in 0..width * width {
        let record = ctx.scratch.cells()[index];
        let coord = (i32::from(record.x), i32::from(record.y));
        if record.region != -1 || coord == (0, 0) {
            continue;
        }
        flood_fill(ctx, &mut regions, coord);
    }

    regions
}

/// The flood-fill constructor: LIFO stack, claim-at-enqueue, level+class
/// matched growth, small-blob dissolve/merge, and the registry append.
fn flood_fill(ctx: &mut RegionCtx<'_>, regions: &mut Regions, seed: (i32, i32)) {
    let id = regions.id_counter;
    let seed_tile = ctx.grid.cell_native(seed.0, seed.1).tile;
    let seed_class = region_class(ctx.ids, seed_tile);
    let seed_level = ctx.grid.cell_native(seed.0, seed.1).level;

    let mut stack: Vec<(i32, i32)> = vec![seed];
    let mut pops = 0i32;
    while let Some(coord) = stack.pop() {
        pops += 1;
        {
            let record = ctx.scratch.get_mut(coord.0, coord.1);
            record.stamp = id;
            record.region = id;
        }
        for dir in 0..8 {
            let (nx, ny) = RmgGrid::step(coord.0, coord.1, dir);
            if !ctx.scratch.in_diamond(nx, ny) {
                continue;
            }
            if ctx.scratch.get(nx, ny).region != -1 || ctx.scratch.get(nx, ny).stamp == id {
                continue;
            }
            // Claim the stamp before the level/class tests: rejected cells
            // stay stamped and are never re-examined by this fill.
            ctx.scratch.get_mut(nx, ny).stamp = id;
            let neighbor = *ctx.grid.cell_native(nx, ny);
            if neighbor.level != seed_level {
                continue;
            }
            if region_class(ctx.ids, neighbor.tile) == seed_class {
                stack.push((nx, ny));
            }
        }
    }

    // Small plain-class blobs are dissolved (first region) or merged.
    if pops <= SMALL_BLOB_POPS && !seed_class {
        if id == 0 {
            if dissolve_first_region(ctx) {
                return;
            }
        } else {
            let mut cand = (seed.0 - 1, seed.1);
            if !ctx.scratch.in_diamond(cand.0, cand.1) {
                cand = (seed.0, seed.1 - 1);
            }
            if ctx.scratch.in_diamond(cand.0, cand.1) {
                let adopt_id = ctx.scratch.get(cand.0, cand.1).region;
                let adopt_level = ctx.grid.cell_native(cand.0, cand.1).level;
                rewrite_blob(ctx, id, adopt_id, adopt_level);
                return;
            }
        }
    }

    // Register the region. The ctor's water-ish class flag is immediately
    // overwritten by the fill's own (water OR green) bit, so only the latter
    // is modeled.
    regions.id_counter += 1;
    let mut region = RmgRegion {
        id,
        level: seed_level,
        active: seed_class,
        cell_count: 0,
        cells: Vec::new(),
        start_quota: 0,
        field_slots: None,
    };
    let width = ctx.scratch.width();
    for index in 0..width * width {
        let record = ctx.scratch.cells()[index];
        if record.region == id && (record.x, record.y) != (0, 0) {
            region.cell_count += 1;
        }
    }
    regions.list.push(region);
}

/// First-region dissolve: pick a random border cell of blob 0, look for a
/// differently-leveled non-water neighbor, and if found dissolve the whole
/// blob back to unassigned clear land at that neighbor's level.
fn dissolve_first_region(ctx: &mut RegionCtx<'_>) -> bool {
    let width = ctx.scratch.width();
    let mut border: Vec<(i32, i32)> = Vec::new();
    for index in 0..width * width {
        let record = ctx.scratch.cells()[index];
        let coord = (i32::from(record.x), i32::from(record.y));
        if coord == (0, 0) || record.region != 0 {
            continue;
        }
        let is_border = (0..8).any(|dir| {
            let (nx, ny) = RmgGrid::step(coord.0, coord.1, dir);
            ctx.scratch.in_diamond(nx, ny) && ctx.scratch.get(nx, ny).region != 0
        });
        if is_border {
            border.push(coord);
        }
    }
    if border.is_empty() {
        return false;
    }
    let pick = border[ctx.rng.uniform(0, border.len() as i32 - 1) as usize];
    let pick_level = ctx.grid.cell_native(pick.0, pick.1).level;

    for dir in 0..8 {
        let (nx, ny) = RmgGrid::step(pick.0, pick.1, dir);
        if !ctx.scratch.in_diamond(nx, ny) {
            continue;
        }
        let neighbor = *ctx.grid.cell_native(nx, ny);
        if neighbor.level != pick_level && !water_ish(ctx.ids, neighbor.tile) {
            rewrite_blob(ctx, 0, -1, neighbor.level);
            return true;
        }
    }
    false
}

/// Rewrite every cell of `blob_id` to `adopt_id` / clear land at the adopted
/// level (0xFF adopts the default), clearing the water flag.
fn rewrite_blob(ctx: &mut RegionCtx<'_>, blob_id: i32, adopt_id: i32, adopt_level: u8) {
    let level = if adopt_level != 0xFF {
        adopt_level
    } else {
        ctx.default_level
    };
    for (x, y) in ctx.grid.native_cells().collect::<Vec<_>>() {
        if ctx.scratch.get(x, y).region != blob_id {
            continue;
        }
        {
            let record = ctx.scratch.get_mut(x, y);
            record.region = adopt_id;
            record.water_region = false;
        }
        let cell = ctx.grid.get_mut(x, y).expect("native cell");
        cell.tile = 0;
        cell.sub_tile = 0;
        cell.level = level;
    }
}

/// Multi-pass ring expansion: pass 1 seeds from the full region boundary,
/// later passes from the previous pass's claims. Returns false on the abort
/// conditions (foreign region contact, or an unclaimed level-matching
/// neighbor failing the clear/green class test).
fn expand(ctx: &mut RegionCtx<'_>, id: i32, level: u8, passes: i32) -> bool {
    let mut frontier = boundary(ctx, id);
    for _ in 0..passes {
        let mut next: Vec<(i32, i32)> = Vec::new();
        for &(x, y) in &frontier {
            for dir in 0..8 {
                let (nx, ny) = RmgGrid::step(x, y, dir);
                if !ctx.scratch.in_diamond(nx, ny) {
                    continue;
                }
                let neighbor = *ctx.grid.cell_native(nx, ny);
                if neighbor.level != level {
                    continue;
                }
                let owner = ctx.scratch.get(nx, ny).region;
                if owner == id {
                    continue;
                }
                if owner != -1 {
                    return false;
                }
                if !(ctx.ids.is_clear(neighbor.tile) || ctx.ids.is_green_lat(neighbor.tile)) {
                    return false;
                }
                ctx.scratch.get_mut(nx, ny).region = id;
                ctx.grid.get_mut(nx, ny).expect("in-diamond cell").level = level;
                next.push((nx, ny));
            }
        }
        frontier = next;
    }
    true
}

/// The boundary collector: region cells with at least one in-diamond
/// neighbor owned by a different region, in native scan order.
fn boundary(ctx: &mut RegionCtx<'_>, id: i32) -> Vec<(i32, i32)> {
    let mut out = Vec::new();
    for (x, y) in ctx.grid.native_cells().collect::<Vec<_>>() {
        if ctx.scratch.get(x, y).region != id {
            continue;
        }
        let is_border = (0..8).any(|dir| {
            let (nx, ny) = RmgGrid::step(x, y, dir);
            ctx.scratch.in_diamond(nx, ny) && ctx.scratch.get(nx, ny).region != id
        });
        if is_border {
            out.push((x, y));
        }
    }
    out
}

/// The water-propagation pass: claims unassigned water-flagged cells
/// reachable from the region's boundary. A no-op on the base path (nothing
/// sets the flag before the mode-3/4 carvers).
fn water_propagate(ctx: &mut RegionCtx<'_>, id: i32) {
    let mut queue = boundary(ctx, id);
    let mut cursor = 0;
    while cursor < queue.len() {
        let (x, y) = queue[cursor];
        cursor += 1;
        for dir in 0..8 {
            let (nx, ny) = RmgGrid::step(x, y, dir);
            if !ctx.scratch.in_diamond(nx, ny) {
                continue;
            }
            let record = ctx.scratch.get(nx, ny);
            if record.water_region && record.region == -1 {
                ctx.scratch.get_mut(nx, ny).region = id;
                queue.push((nx, ny));
            }
        }
    }
}

/// Region split markers in the scratch region field (`+0x38`): a cell of the
/// region being split, and a cell the best-first growth claimed.
const SPLIT_CANDIDATE: i32 = -2;
const SPLIT_GROWN: i32 = -3;
/// Region size at or below which a split-off piece merges into its largest
/// land neighbour instead of taking a new level (`CMP 0x65` at 0x0058E4A0).
const SPLIT_MERGE_MAX_CELLS: i32 = 100;
/// `[0x007ED8A8]`: 2*pi/(2^32-1), the split heading's scale on one draw.
const SPLIT_HEADING_SCALE_BITS: u64 = 0x3E19_21FB_545D_4F13;

/// `RandomMapGenerator::SplitOversizedRegions @ 0x0058EBC0`, run between the
/// partition and the island passes for map types 3 and 4 (`Generate`
/// 0x00598D55..0x00598D62).
///
/// Rebuilds each region's cell list in descending linear-cell order (the seed
/// draw indexes it), then splits every land region larger than
/// `trunc(2 * (RegionSize*0.005 + 0.05) * gen_h * gen_w)` with
/// [`split_region`], restarting the scan after each split. The bounding box
/// the rebuild also keeps has no reader before the island passes delete every
/// region, so it is not modelled.
pub fn split_oversized(
    ctx: &mut RegionCtx<'_>,
    gauss: &mut Gaussian,
    regions: &mut Regions,
    region_size: i32,
    gen_w: i32,
    gen_h: i32,
) {
    debug_assert!(
        regions
            .list
            .iter()
            .enumerate()
            .all(|(pos, r)| r.id == pos as i32),
        "the rebuild indexes regions by id; ids must equal list positions"
    );
    // 0x0058EBC0..0x0058ED83: clear, then recount in descending scratch order.
    for region in &mut regions.list {
        region.cells.clear();
        region.cell_count = 0;
    }
    let width = ctx.scratch.width();
    for index in (0..width * width).rev() {
        let record = ctx.scratch.cells()[index];
        let (x, y) = (i32::from(record.x), i32::from(record.y));
        // The rebuild indexes the list by id: ids equal list positions here.
        if ctx.scratch.in_diamond(x, y) && (0..regions.list.len() as i32).contains(&record.region) {
            let region = &mut regions.list[record.region as usize];
            region.cell_count += 1;
            region.cells.push((record.x, record.y));
        }
    }

    // 0x0058ED89: FILD RegionSize; FMUL 0.005; FADD 0.05; FIMUL gen_h
    // ([0x00ABE15C]); FIMUL gen_w ([0x00ABE158]); FADD ST0,ST0; ftol (chop).
    let scaled = TruncF64::from_f64(f64::from(region_size))
        .mul(TruncF64::from_f64(0.005))
        .add(TruncF64::from_f64(0.05))
        .mul(TruncF64::from_f64(f64::from(gen_h)))
        .mul(TruncF64::from_f64(f64::from(gen_w)));
    let threshold = X87Chop53::ftol_f64_low_masked(scaled.add(scaled).to_f64());

    // 0x0058EDB5: the finalised flag (`+0x1A`) is per region object.
    let mut finalised: Vec<bool> = Vec::new();
    'restart: loop {
        for index in 0..regions.list.len() {
            let region = &regions.list[index];
            let id = region.id as usize;
            if region.active || finalised.get(id).copied().unwrap_or(false) {
                continue;
            }
            if region.cell_count > threshold {
                split_region(ctx, gauss, regions, index);
                continue 'restart;
            }
            if finalised.len() <= id {
                finalised.resize(id + 1, false);
            }
            finalised[id] = true;
        }
        break;
    }
}

/// `RmgRegion::Split @ 0x0058D620`: grow a random-heading blob from a random
/// cell, cut the region into the grown and remaining components, give each
/// large piece a level a step away from its neighbours and merge each small
/// piece into its largest land neighbour. Removes the split region (the
/// caller's delist at 0x0058EE05) and every piece merged away.
fn split_region(
    ctx: &mut RegionCtx<'_>,
    gauss: &mut Gaussian,
    regions: &mut Regions,
    index: usize,
) {
    let own_id = regions.list[index].id;
    let own_cells = std::mem::take(&mut regions.list[index].cells);
    let area = regions.list[index].cell_count;

    for record in ctx.scratch.cells_mut() {
        record.stamp = -1;
    }
    for &(x, y) in &own_cells {
        ctx.scratch.get_mut(i32::from(x), i32::from(y)).region = SPLIT_CANDIDATE;
    }

    // Draws 1 and 2 (0x0058D782, 0x0058D7CD).
    let max_steps = ctx.rng.uniform(area / 8, area / 3);
    let pick = ctx.rng.uniform(0, own_cells.len() as i32 - 1) as usize;
    let seed = (i32::from(own_cells[pick].0), i32::from(own_cells[pick].1));

    // The node arena and heap are both `2 * area + 10` (0x0058D640..0x0058D6B0).
    let mut heap = CellHeap::new(2 * area.max(0) as usize + 10);
    ctx.scratch.get_mut(seed.0, seed.1).stamp = SPLIT_GROWN;
    heap.push((0.0, seed), by_key);
    let mut node = heap.pop(by_key);
    // Draw 3 (0x0058D8C6): the initial heading.
    let mut heading = TruncF64::from_f64(f64::from(ctx.rng.next_u32()))
        .mul(TruncF64::from_f64(f64::from_bits(SPLIT_HEADING_SCALE_BITS)));
    let quarter_turn_half = TruncF64::from_f64(std::f64::consts::FRAC_PI_8);
    let mut steps = 0;
    while let Some((_, cell)) = node {
        if steps >= max_steps {
            break;
        }
        ctx.scratch.get_mut(cell.0, cell.1).region = SPLIT_GROWN;
        for dir in [0usize, 2, 4, 6] {
            let (nx, ny) = RmgGrid::step(cell.0, cell.1, dir);
            if !ctx.scratch.in_diamond(nx, ny) {
                continue;
            }
            let record = ctx.scratch.get(nx, ny);
            if record.region != SPLIT_CANDIDATE || record.stamp == SPLIT_GROWN {
                continue;
            }
            // `CellClass::IsClearTile @ 0x00486380`.
            if !ctx.ids.is_clear(ctx.grid.cell_native(nx, ny).tile) {
                continue;
            }
            let key = split_frontier_cost(ctx.rng, seed, (nx, ny), heading);
            ctx.scratch.get_mut(nx, ny).stamp = SPLIT_GROWN;
            heap.push((key, (nx, ny)), by_key);
        }
        steps += 1;
        // Draw 4 (0x0058DAF0): the heading wanders by a Gaussian * pi/8.
        heading = TruncF64::from_f64(gauss.next(ctx.rng))
            .mul(quarter_turn_half)
            .add(heading);
        if heap.is_empty() {
            break;
        }
        node = heap.pop(by_key);
    }
    // 0x0058DB78..0x0058DC1C: clear the stamps, then the open frontier joins
    // the grown side. The node held when the step cap ended the loop was
    // already popped, so it stays a candidate.
    for record in ctx.scratch.cells_mut() {
        record.stamp = -1;
    }
    while let Some((_, cell)) = heap.pop(by_key) {
        ctx.scratch.get_mut(cell.0, cell.1).region = SPLIT_GROWN;
    }

    // 0x0058DC4C..0x0058DEB0: one new region per 8-connected component of each
    // marker, found in descending cell-list order (`RmgRegion::RmgRegion @
    // 0x0058BF70` takes a fresh id and appends itself to the list).
    let mut pieces: Vec<i32> = Vec::new();
    for &(x, y) in own_cells.iter().rev() {
        let start = (i32::from(x), i32::from(y));
        let marker = ctx.scratch.get(start.0, start.1).region;
        if marker >= -1 {
            continue;
        }
        let id = regions.id_counter;
        regions.id_counter += 1;
        let seed_cell = *ctx.grid.cell_native(start.0, start.1);
        let mut piece =
            RmgRegion::rebuilt(id, seed_cell.level, water_ish(ctx.ids, seed_cell.tile), 1);
        piece.cells.push((x, y));
        ctx.scratch.get_mut(start.0, start.1).region = id;
        let mut stack = vec![start];
        while let Some((cx, cy)) = stack.pop() {
            for dir in 0..8 {
                let (nx, ny) = RmgGrid::step(cx, cy, dir);
                if ctx.scratch.in_diamond(nx, ny) && ctx.scratch.get(nx, ny).region == marker {
                    piece.cells.push((nx as i16, ny as i16));
                    piece.cell_count += 1;
                    ctx.scratch.get_mut(nx, ny).region = id;
                    stack.push((nx, ny));
                }
            }
        }
        regions.list.push(piece);
        pieces.push(id);
    }

    // 0x0058DED0..0x0058E4F0, per piece in descending creation order.
    let mut merged: Vec<i32> = Vec::new();
    for &piece_id in pieces.iter().rev() {
        if merged.contains(&piece_id) {
            continue;
        }
        let piece_pos = position_of(regions, piece_id).expect("split piece is listed");

        // `RmgRegion::CollectBorderCells @ 0x0058E5D0` keeps the cells with a
        // foreign in-diamond neighbour; an interior cell's neighbours are all
        // its own, so scanning every cell gives the same neighbour set.
        let mut adjacent = vec![false; regions.id_counter.max(0) as usize];
        for &(x, y) in &regions.list[piece_pos].cells {
            for dir in 0..8 {
                let (nx, ny) = RmgGrid::step(i32::from(x), i32::from(y), dir);
                if !ctx.scratch.in_diamond(nx, ny) {
                    continue;
                }
                let owner = ctx.scratch.get(nx, ny).region;
                if owner >= 0 && owner != piece_id {
                    adjacent[owner as usize] = true;
                }
            }
        }

        // Ascending ids: the largest land neighbour (strict, first wins) and
        // the level span of every live neighbour. Levels are the signed byte
        // the constructor widens (`MOVSX`).
        let mut best: Option<(i32, i32)> = None;
        let mut best_count = -1;
        let (mut low, mut high) = (-1i32, -1i32);
        for (neighbour_id, _) in adjacent.iter().enumerate().filter(|(_, on)| **on) {
            let Some(pos) = position_of(regions, neighbour_id as i32) else {
                continue;
            };
            let neighbour = &regions.list[pos];
            if merged.contains(&neighbour.id) {
                continue;
            }
            let level = i32::from(neighbour.level as i8);
            if best_count < neighbour.cell_count && !neighbour.active {
                best = Some((neighbour.id, level));
                best_count = neighbour.cell_count;
            }
            if low == -1 {
                (low, high) = (level, level);
            } else {
                high = high.max(level);
                low = low.min(level);
            }
        }
        let mut candidates: Vec<i32> = Vec::new();
        if high == low {
            if low > 3 {
                candidates.push(low - 4);
            }
            if high < 8 {
                candidates.push(high + 4);
            }
        } else if high - low == 4 {
            candidates.extend([high, low]);
        } else if high - low == 8 {
            candidates.push((high + low) / 2);
        }
        // 0x0058E2F9: map types 3/4 drop the sea level.
        if matches!(ctx.map_type, 3 | 4) {
            candidates.retain(|&level| level != 0);
        }

        let old_level = i32::from(regions.list[piece_pos].level as i8);
        if regions.list[piece_pos].cell_count <= SPLIT_MERGE_MAX_CELLS {
            let Some((best_id, best_level)) = best else {
                continue;
            };
            regions.list[piece_pos].level = best_level as u8;
            shift_region_level(ctx, piece_id, best_level - old_level);
            let cells = regions.list[piece_pos].cells.clone();
            let best_pos = position_of(regions, best_id).expect("neighbour is listed");
            for &(x, y) in cells.iter().rev() {
                regions.list[best_pos].cells.push((x, y));
                regions.list[best_pos].cell_count += 1;
                ctx.scratch.get_mut(i32::from(x), i32::from(y)).region = best_id;
            }
            merged.push(piece_id);
        } else if candidates.is_empty() {
            // Native indexes the empty candidate list (0x0058E3A0) after one
            // accepted draw: a read through a null array that faults. The
            // draw is kept and the level left as it is.
            ctx.rng.next_u32();
        } else {
            // Draw 6 (0x0058E37D).
            let level = candidates[ctx.rng.uniform(0, candidates.len() as i32 - 1) as usize];
            regions.list[piece_pos].level = level as u8;
            shift_region_level(ctx, piece_id, level - old_level);
        }
    }

    // 0x0058E4F8 frees every merged piece (its destructor `0x0058C070`
    // delists it); the caller then delists the split region itself.
    regions
        .list
        .retain(|region| region.id != own_id && !merged.contains(&region.id));
}

fn position_of(regions: &Regions, id: i32) -> Option<usize> {
    regions.list.iter().position(|region| region.id == id)
}

/// Add `delta` (as the byte `ADD`) to the level of every cell the region owns,
/// walking `MapClass::CellIterator` order and reading ownership through
/// `Rmg__GetCellRegionId @ 0x0058D0A0` (0x0058E3C0..0x0058E40C).
fn shift_region_level(ctx: &mut RegionCtx<'_>, id: i32, delta: i32) {
    for (x, y) in ctx.grid.native_cells().collect::<Vec<_>>() {
        if ctx.scratch.get(x, y).region == id {
            let cell = ctx.grid.get_mut(x, y).expect("native cell");
            cell.level = cell.level.wrapping_add(delta as u8);
        }
    }
}

/// `RandomMapGenerator::SplitFrontierCost @ 0x0058C6F0`: distance from the
/// seed, angular distance from the heading and one draw of noise, stored as
/// the f32 heap key (`FSTP m32` under chop at 0x0058DA36).
fn split_frontier_cost(
    rng: &mut RmgRng,
    seed: (i32, i32),
    cell: (i32, i32),
    heading: TruncF64,
) -> f32 {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let tf = TruncF64::from_f64;
    let dx = cell.0 - seed.0;
    let dy = cell.1 - seed.1;
    let distance = approx_sqrt(tf(f64::from(dy * dy + dx * dx)));
    let angle = if dx == 0 {
        tf(FRAC_PI_2)
    } else {
        let ratio = tf(f64::from(dy)).div(tf(f64::from(dx))).neg();
        let angle = TruncF64::from(native_atan_from_table(ratio.into()));
        if dx < 0 { angle.add(tf(PI)) } else { angle }
    };
    let mut off = angle.sub(heading).abs();
    while !off.lt(tf(TAU)) {
        off = off.sub(tf(TAU));
    }
    if tf(PI).lt(off) {
        off = tf(TAU).sub(off);
    }
    let noise = tf(f64::from(rng.next_u32()))
        .mul(tf(f64::from_bits(RANGE_K_BITS)))
        .mul(tf(2.5));
    let cost = noise.add(tf(1.5).mul(off)).add(tf(0.15).mul(distance));
    narrow_to_f32(cost).to_f64() as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::theater::TheaterCliffRanges;

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

    fn world() -> (RmgGrid, RmgScratch) {
        let (map_w, map_h) = (20, 16);
        let stride = (map_w + map_h + 1) as usize;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = RmgGrid::new(stride, dmin, dmax);
        let scratch = RmgScratch::new(stride, dmin, dmax);
        let coords: Vec<(i32, i32)> = grid.native_cells().collect();
        for (x, y) in coords {
            // Clear land everywhere at the default level.
            grid.get_mut(x, y).unwrap().tile = 0;
        }
        (grid, scratch)
    }

    fn ctx<'a>(
        grid: &'a mut RmgGrid,
        scratch: &'a mut RmgScratch,
        identity: &'a TileIds,
        rng: &'a mut RmgRng,
    ) -> RegionCtx<'a> {
        RegionCtx {
            grid,
            scratch,
            ids: identity,
            rng,
            map_type: 1,
            default_level: 4,
        }
    }

    #[test]
    fn partition_is_deterministic() {
        let outcome = |seed| {
            let (mut grid, mut scratch) = world();
            let identity = ids();
            // A couple of small features to exercise dissolve paths.
            for &(x, y) in &[(14, 9), (15, 9), (16, 10)] {
                if grid.is_valid(x, y) {
                    grid.get_mut(x, y).unwrap().tile = 500;
                }
            }
            let mut rng = RmgRng::new(seed);
            let mut context = ctx(&mut grid, &mut scratch, &identity, &mut rng);
            let regions = run(&mut context);
            let map: Vec<i32> = grid
                .native_cells()
                .collect::<Vec<_>>()
                .iter()
                .map(|&(x, y)| scratch.get(x, y).region)
                .collect();
            (regions.list.len(), map)
        };
        assert_eq!(outcome(42), outcome(42));
    }

    /// Whole original `SplitOversizedRegions @ 0x0058EBC0` calls from
    /// `tools/rmg_oracle/region_split.py`: a declared partition (not a full
    /// Generate prefix), seeded streams, and every level, region id, list
    /// entry and draw the original produced.
    #[test]
    fn native_split_oversized_controls() {
        let doc = crate::test_fixture::json("tools/rmg_oracle/region_split.json");
        let state = |rng: RmgRng| {
            let (locked, words, a, b) = rng.into_continuation().into_native_parts();
            let mut bytes = vec![locked, 0, 0, 0];
            bytes.extend_from_slice(&(a as u32).to_le_bytes());
            bytes.extend_from_slice(&(b as u32).to_le_bytes());
            for word in words {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let cliff = doc["scene"]["cliff_base"].as_i64().unwrap() as i32;
        let rows = doc["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        for row in rows {
            let input = &row["input"];
            let context = input["id"].as_str().unwrap();
            let banded = input["scene"] == "banded";
            let mut grid = RmgGrid::new(33, 16, 48);
            let mut scratch = RmgScratch::new(33, 16, 48);
            scratch.reset_region_ids();
            let diamond: Vec<(i32, i32)> = (0..33)
                .flat_map(|y| (0..33).map(move |x| (x, y)))
                .filter(|&(x, y)| scratch.in_diamond(x, y))
                .collect();
            // The oracle's declared scene: region, level and tile per cell.
            for &(x, y) in &diamond {
                let (region, level, tile) = if !banded {
                    (0, 4, 0)
                } else if x + y <= 21 {
                    (0, 0, 0)
                } else if x - y >= 9 {
                    (1, 8, 0)
                } else if [(24, 24), (25, 24), (24, 25), (30, 14)].contains(&(x, y)) {
                    (2, 4, cliff)
                } else {
                    (2, 4, 0)
                };
                let cell = grid.get_mut(x, y).unwrap();
                (cell.tile, cell.level) = (tile, level);
                scratch.get_mut(x, y).region = region;
            }
            let mut regions = Regions::default();
            for entry in row["partition"].as_array().unwrap() {
                let id = entry["id"].as_i64().unwrap() as i32;
                let seed = &entry["seed"];
                let (x, y) = (
                    seed[0].as_i64().unwrap() as i32,
                    seed[1].as_i64().unwrap() as i32,
                );
                let level = grid.get(x, y).unwrap().level;
                regions
                    .list
                    .push(RmgRegion::rebuilt(id, level, banded && id == 0, 0));
                regions.id_counter += 1;
            }
            // HasBridgeOverlay's bases as the fixture published them; the
            // four waterfall sets are compared for membership only.
            let family = &doc["scene"]["water_family"];
            let base = |v: &serde_json::Value| v.as_i64().unwrap() as i32;
            let mut identity = ids();
            identity.water_base = base(&family["water"]);
            identity.shore = base(&family["shore"]);
            let falls: Vec<Option<u16>> = family["waterfalls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| u16::try_from(base(v)).ok())
                .collect();
            identity.cliffs.waterfall_north = falls[0];
            identity.cliffs.waterfall_east = falls[1];
            identity.cliffs.waterfall_south = falls[2];
            identity.cliffs.waterfall_west = falls[3];
            let mut rng = RmgRng::new(input["seed"].as_u64().unwrap() as u16);
            let before = row["rng_before"].as_u64().unwrap() as usize;
            assert_eq!(
                state(rng.clone()),
                doc["rng_states_hex"][before],
                "{context}: entry RNG"
            );
            let mut gauss = Gaussian::default();
            let gen_dims = &input["gen"];
            let mut context_ = RegionCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                rng: &mut rng,
                map_type: 3,
                default_level: 4,
            };
            split_oversized(
                &mut context_,
                &mut gauss,
                &mut regions,
                input["region_size"].as_i64().unwrap() as i32,
                gen_dims[0].as_i64().unwrap() as i32,
                gen_dims[1].as_i64().unwrap() as i32,
            );

            let after = row["rng_after"].as_u64().unwrap() as usize;
            assert_eq!(
                state(rng),
                doc["rng_states_hex"][after],
                "{context}: exit RNG"
            );
            let levels = row["levels_hex"].as_str().unwrap();
            for &(x, y) in &diamond {
                let at = 2 * (y as usize * 32 + x as usize);
                let native = u8::from_str_radix(&levels[at..at + 2], 16).unwrap();
                assert_eq!(
                    grid.get(x, y).unwrap().level,
                    native,
                    "{context}: level at {x},{y}"
                );
            }
            let owners: Vec<i64> = diamond
                .iter()
                .map(|&(x, y)| i64::from(scratch.get(x, y).region))
                .collect();
            let native_owners: Vec<i64> = row["regions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap())
                .collect();
            assert_eq!(owners, native_owners, "{context}: region ids");
            let listed: Vec<[i64; 4]> = regions
                .list
                .iter()
                .map(|r| {
                    [
                        i64::from(r.id),
                        i64::from(r.cell_count),
                        i64::from(r.level as i8),
                        i64::from(r.active),
                    ]
                })
                .collect();
            let native_listed: Vec<[i64; 4]> = row["listed"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| std::array::from_fn(|i| r[i].as_i64().unwrap()))
                .collect();
            assert_eq!(listed, native_listed, "{context}: region list");
            assert_eq!(
                i64::from(regions.id_counter),
                row["next_region_id"].as_i64().unwrap(),
                "{context}: next id"
            );
        }
    }
}
