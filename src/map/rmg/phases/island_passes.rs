//! The inland/mountainous extra pass: cliff repair, then a full region rebuild
//! (`RandomMapGenerator::BridgeAndConnectorPass @ 0x0058EF10`, steps 2-5).
//!
//! Map types 3 and 4 run one more pass after the region partition. It repairs
//! cliff lines by raising cells that are pinched between higher ground, and
//! because that repair stamps the working grid's region tag as a side effect,
//! it then throws the whole partition away and rebuilds it from the repaired
//! terrain.
//!
//! **The destroy-and-rebuild is not incidental — it is why this has to land as
//! one piece.** Porting the raise alone would leave every later phase (starts,
//! tech buildings, tiberium) reading region ids the raise had stomped.
//!
//! After this rebuild the pipeline hands the refreshed region-neighbour list to
//! the active connector/low-deck owner in `carve_driver`; that separation keeps
//! this module's only draw, in the rebuild flood's narrow dissolve case.

use crate::map::rmg::grid::{CellRef, RmgGrid};
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;

use super::regions::{self, RegionCtx, Regions};

/// One terrain step. Cliff lines are exactly this far apart, which is what
/// makes "is my neighbour one step up" a test for equality rather than
/// ordering.
const LEVEL_STEP: u8 = 4;

/// Higher-neighbour ring mask: bit set per direction whose neighbour sits
/// **exactly** one terrain step above this cell and carries ordinary ground.
///
/// Zero for a cell outside the map rect or whose shore-enable flag is clear —
/// the original bails before looking at a single neighbour in both cases.
///
/// The bit layout is `1 << ((dir + 7) mod 8)` over the standard direction
/// order, i.e. N is the high bit and NE the low one. That is the same layout
/// the cliff-face selector uses.
pub(crate) fn ring_mask(
    grid: &mut RmgGrid,
    scratch: &RmgScratch,
    ids: &TileIds,
    x: i32,
    y: i32,
) -> u8 {
    // Original579B70 reads signed Cell+11B, then acquires all eight
    // neighbor pointers before classifying NW,N,NE,W,E,SW,S,SE. Retain the
    // shared border identity rather than copying its changing coordinate.
    let target = i32::from(grid.cell_native(x, y).level as i8) + 4;
    if !scratch.in_diamond(x, y) || !scratch.get(x, y).shore_enable {
        return 0;
    }
    let mut neighbors = [(CellRef::Border, (0, 0)); 8];
    for dir in [1, 2, 3, 4, 5, 6, 7, 0] {
        let (nx, ny) = RmgGrid::step(x, y, dir);
        let (nx, ny) = (i32::from(nx as i16), i32::from(ny as i16));
        neighbors[dir] = (grid.cell_ref(nx, ny), (nx, ny));
    }
    let mut mask = 0;
    for dir in [7, 0, 1, 6, 2, 5, 4, 3] {
        let (held, (nx, ny)) = neighbors[dir];
        if !scratch.in_diamond(nx, ny) {
            continue;
        }
        let cell = grid.ref_cell(held);
        if i32::from(cell.level as i8) == target
            && !ids.cliffs.is_special_terrain_tile(cell.tile, cell.sub_tile)
        {
            mask |= 1 << ((dir + 7) % 8);
        }
    }
    mask
}

/// Is this cell pinched between higher ground, and therefore due a step up?
///
/// Transcribed rule by rule. Four of the tests look past the cell itself and
/// re-derive a *neighbour's* mask — that is what "pinched" means here: high
/// ground on one side, and on the far side a cell that in turn has high ground
/// beyond it, so the cell sits in a one-wide trench.
fn is_pinched(
    grid: &mut RmgGrid,
    scratch: &RmgScratch,
    ids: &TileIds,
    x: i32,
    y: i32,
    mask: u8,
) -> bool {
    const N: u8 = 0x80;
    const NE: u8 = 0x01;
    const E: u8 = 0x02;
    const SE: u8 = 0x04;
    const S: u8 = 0x08;
    const SW: u8 = 0x10;
    const W: u8 = 0x20;
    const NW: u8 = 0x40;

    let mut pinched = mask & (N | W) == (N | W) && mask & (NE | SW) == (NE | SW);
    // The four trench tests, each against the opposite bit of the far cell.
    let trench = [
        (W, (x + 1, y), E),
        (E, (x - 1, y), W),
        (S, (x, y - 1), N),
        (N, (x, y + 1), S),
    ];
    for (near, (fx, fy), far) in trench {
        if mask & near != 0
            && ring_mask(
                grid,
                scratch,
                ids,
                i32::from(fx as i16),
                i32::from(fy as i16),
            ) & far
                != 0
        {
            pinched = true;
        }
    }
    if mask & (NE | SW) == (NE | SW)
        && (mask & (E | S) != (E | S) || mask & (N | W | NW) != 0)
        && (mask & (N | W) != (N | W) || mask & (E | SE | S) != 0)
    {
        pinched = true;
    }
    if mask & (SE | NW) == (SE | NW)
        && (mask & (S | W) != (S | W) || mask & (N | NE | E) != 0)
        && (mask & (N | E) != (N | E) || mask & (S | SW | W) != 0)
    {
        pinched = true;
    }
    // Eight two-bit corner patterns: a diagonal present with its flanking
    // cardinal absent.
    const PATTERNS: [(u8, u8); 8] = [
        (0x2C, 0x24),
        (0xA1, 0x21),
        (0x1A, 0x12),
        (0xC2, 0x42),
        (0x0B, 0x09),
        (0x68, 0x48),
        (0x86, 0x84),
        (0xB0, 0x90),
    ];
    pinched || PATTERNS.iter().any(|&(m, want)| mask & m == want)
}

/// Raise a pinched cell one step and recurse into its neighbours.
///
/// Raising removes the exact-one-step relation that set the bits, so the
/// recursion terminates on its own. The region tag it writes is what forces
/// the rebuild that follows.
pub(super) fn raise_pinched(
    grid: &mut RmgGrid,
    scratch: &mut RmgScratch,
    ids: &TileIds,
    x: i32,
    y: i32,
    tag: i32,
) -> bool {
    // Original579010 recursively visits directions0..7 and ignores child
    // refusal. Explicit frames preserve that depth-first order without a
    // host-stack limit that silently truncates a valid native chain.
    let mut frames: Vec<(CellRef, usize)> = Vec::new();
    let mut pending = Some((x, y));
    loop {
        if let Some((cx, cy)) = pending.take() {
            let held = grid.cell_ref(cx, cy);
            let mask = ring_mask(grid, scratch, ids, cx, cy);
            // All trench probes precede the final opposite-cardinal test.
            let pinched = is_pinched(grid, scratch, ids, cx, cy, mask);
            let forced = mask & 0x88 == 0x88 || mask & 0x22 == 0x22;
            if forced || pinched {
                let (tx, ty) = grid.ref_coord(held);
                let owner = scratch.get(tx, ty).region;
                if owner > 0 && owner != tag && tag != -1 {
                    if frames.is_empty() {
                        return false;
                    }
                } else {
                    let cell = grid.ref_cell_mut(held);
                    cell.level = cell.level.wrapping_add(LEVEL_STEP);
                    scratch.get_mut(tx, ty).region = tag;
                    frames.push((held, 0));
                }
            }
        }
        let Some((held, next_dir)) = frames.last_mut() else {
            return true;
        };
        if *next_dir == 8 {
            frames.pop();
            continue;
        }
        let (px, py) = grid.ref_coord(*held);
        let (nx, ny) = RmgGrid::step(px, py, *next_dir);
        *next_dir += 1;
        pending = Some((i32::from(nx as i16), i32::from(ny as i16)));
    }
}

/// Run the pass. Only map types 3 and 4 reach it.
///
/// Returns the rebuilt partition, which replaces the one the region phase
/// produced.
pub fn run(ctx: &mut RegionCtx<'_>) -> Regions {
    // Step 1: repair the cliff lines. The tag is the original's -1, which is
    // why its own foreign-zone bail can never fire from here.
    let cells: Vec<(i32, i32)> = ctx.grid.native_cells().collect();
    for &(x, y) in &cells {
        let _ = raise_pinched(ctx.grid, ctx.scratch, ctx.ids, x, y, -1);
    }

    // Step 2: the repair stomped region tags, so throw the partition away:
    // every slot's +0x38 and +0x3C to -1.
    ctx.scratch.reset_region_ids();

    // Step 3: rebuild with the partition's own flood, one per still-unowned
    // slot in linear order.
    let mut rebuilt = Regions::default();
    regions::flood_unowned(ctx, &mut rebuilt);
    rebuilt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::rng::RmgRng;
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

    fn harness(level: u8) -> (RmgGrid, RmgScratch) {
        let (dmin, dmax) = (34, 34 + 2 * 42);
        let stride = (34 + 42 + 1) as usize;
        let mut grid = RmgGrid::new(stride, dmin, dmax);
        let scratch = RmgScratch::new(stride, dmin, dmax);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            let cell = grid.get_mut(x, y).expect("native cell");
            cell.tile = 0;
            cell.level = level;
        }
        (grid, scratch)
    }

    fn region_ctx<'a>(
        grid: &'a mut RmgGrid,
        scratch: &'a mut RmgScratch,
        ids: &'a TileIds,
        rng: &'a mut RmgRng,
    ) -> RegionCtx<'a> {
        RegionCtx {
            grid,
            scratch,
            ids,
            rng,
            map_type: 3,
            default_level: 4,
        }
    }

    /// `FloodBuildRegion @ 0x0058C800`: a small first land blob whose border
    /// pick finds no differently-levelled land neighbour is kept as region 0
    /// (0x0058CC70 -> 0x0058CDF6) after its one draw, not repainted.
    #[test]
    fn a_small_blob_with_no_other_level_stays_a_region() {
        let (mut grid, mut scratch) = harness(4);
        let ids = ids();
        // A land tile outside every water/shore/waterfall/green family; the
        // default cliff ranges put a waterfall family at tile 0.
        let mut land = 0;
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            let cell = grid.get_mut(x, y).unwrap();
            if y > 3 {
                cell.tile = ids.water_base;
            } else {
                cell.tile = 300;
                land += 1;
            }
        }
        assert!((1..=0x4A).contains(&land));
        let mut rng = RmgRng::new(1);
        let mut ctx = region_ctx(&mut grid, &mut scratch, &ids, &mut rng);
        let regions = run(&mut ctx);
        assert_eq!(regions.list.len(), 2);
        assert!(!regions.list[0].active);
        assert_eq!(regions.list[0].cell_count, land);
        let mut once = RmgRng::new(1);
        once.next_u32();
        assert_eq!(ctx.rng.next_u32(), once.next_u32(), "one dissolve draw");
    }

    #[test]
    fn the_ring_mask_puts_north_in_the_high_bit() {
        // Pins the bit layout the whole predicate is written against: N is
        // 0x80 and NE is 0x01, i.e. `1 << ((dir + 7) mod 8)`. Getting this
        // rotated would silently mirror every raise decision.
        let (mut grid, scratch) = harness(4);
        let ids = ids();
        grid.get_mut(40, 49).expect("native cell").level = 8;
        assert_eq!(ring_mask(&mut grid, &scratch, &ids, 40, 50), 0x80, "north");
        grid.get_mut(40, 49).expect("native cell").level = 4;
        grid.get_mut(41, 49).expect("native cell").level = 8;
        assert_eq!(
            ring_mask(&mut grid, &scratch, &ids, 40, 50),
            0x01,
            "north-east"
        );
    }

    #[test]
    fn only_an_exact_step_counts_as_higher() {
        // Two steps up is not a cliff edge — the test is equality, not
        // ordering, because cliff lines are exactly one quantum apart.
        let (mut grid, scratch) = harness(4);
        let ids = ids();
        grid.get_mut(40, 49).expect("native cell").level = 12;
        assert_eq!(ring_mask(&mut grid, &scratch, &ids, 40, 50), 0);
    }

    #[test]
    fn a_cell_between_two_high_sides_is_raised() {
        // North and south both one step up: the unconditional arm.
        let (mut grid, mut scratch) = harness(4);
        let ids = ids();
        grid.get_mut(40, 49).expect("native cell").level = 8;
        grid.get_mut(40, 51).expect("native cell").level = 8;
        let _ = raise_pinched(&mut grid, &mut scratch, &ids, 40, 50, -1);
        assert_eq!(grid.cell_native(40, 50).level, 8, "raised one step");
        assert_eq!(scratch.get(40, 50).region, -1, "tag stamped");
    }

    #[test]
    fn an_open_cell_is_left_alone() {
        let (mut grid, mut scratch) = harness(4);
        let ids = ids();
        grid.get_mut(40, 49).expect("native cell").level = 8;
        let _ = raise_pinched(&mut grid, &mut scratch, &ids, 40, 50, -1);
        assert_eq!(grid.cell_native(40, 50).level, 4, "one high side only");
    }

    #[test]
    fn the_rebuild_partitions_flat_ground_into_one_region() {
        // Flat clear ground is one class and one level, so the whole diamond
        // floods as a single region — and it is far above the small-blob
        // threshold, so no dissolve draw happens.
        let (mut grid, mut scratch) = harness(4);
        let ids = ids();
        let mut rng = RmgRng::new(1);
        let before = rng.next_u32();
        let mut probe = RmgRng::new(1);
        assert_eq!(before, probe.next_u32(), "probe tracks the generator");

        let mut rng = RmgRng::new(1);
        let mut ctx = region_ctx(&mut grid, &mut scratch, &ids, &mut rng);
        let regions = run(&mut ctx);
        assert_eq!(regions.list.len(), 1, "one flat region");
        assert!(regions.list[0].cell_count > 0x4A);
        // No draw was consumed: the flood was never small.
        let mut fresh = RmgRng::new(1);
        assert_eq!(ctx.rng.next_u32(), fresh.next_u32(), "rebuild drew nothing");
    }
}
