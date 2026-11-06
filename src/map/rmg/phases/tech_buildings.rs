//! Neutral tech-building placement.
//!
//! Map type 0 never reaches the driver (`Generate` gates `AddTechBuildings`
//! on a non-zero type); its buildings come from the coin flips at the end of
//! `CreateStartingPoints` instead ([`run_start_flips_traced`]). The driver's
//! two paths share one foundation gate:
//! - map type 2: `uniform(0,2)` passes; each pass walks the regions in
//!   creation order and, for every region with a nonzero start quota, makes
//!   one building whose anchor is drawn from that region's cell list.
//! - map types 1/3/4: `uniform(0,4)` buildings, each anchored at a uniform
//!   random clear scratch cell over the whole map (200-try inner reject).
//!
//! Per building exactly one type is drawn from the resolved
//! `NeutralTechBuildings` list, then up to 100 placement attempts run. The
//! foundation walk consumes no RNG; it only decides whether the building
//! commits, so the draw stream depends solely on the pass/count draw, the
//! per-building type draw, and the anchor draws.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::TruncF64;
use crate::map::rmg::{RmgConstructionPhase, RmgConstructionTrace};

use super::regions::Regions;
use super::shore::TileBlocks;
use super::zones::{LAND_ROCK, diamond_frame_contains, recalc_land};

/// Placement attempts per building.
const MAX_ATTEMPTS: i32 = 100;
/// Inner clear-anchor draws per attempt on the whole-map path.
const MAX_ANCHOR_DRAWS: i32 = 200;

/// A resolved neutral tech building: its type name plus foundation footprint
/// cell offsets (NW-anchor-relative). The footprint is the base rectangle the
/// original walks from `type+0xDFC`.
#[derive(Debug, Clone)]
pub struct TechType {
    pub name: String,
    pub footprint: Vec<(i16, i16)>,
}

/// Phase inputs.
#[derive(Debug, Clone, Copy)]
pub struct TechArgs {
    pub map_type: i32,
    pub map_w: i32,
    /// Playfield local rect `{x, y, w, h}` — generated maps use `(2, 5, w, h)`.
    pub local_rect: [i32; 4],
    /// Scratch/grid stride: the whole-map anchor draw is `uniform(0, S²−1)`.
    pub stride: i32,
    /// Low byte of `[General] CliffBackImpassability=`.
    pub cliff_back_impassability: u8,
}

/// One recorded placement, emitted later as a neutral `[Structures]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TechPlacement {
    pub name: String,
    pub x: i16,
    pub y: i16,
}

/// Everything the phase borrows.
pub struct TechCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    /// Tile blocks, for the foundation gate's land type.
    pub blocks: &'a dyn TileBlocks,
    pub rng: &'a mut RmgRng,
    pub regions: &'a Regions,
    pub types: &'a [TechType],
}

/// Native placement plus the Scenario-constructor trace. Each selected type
/// constructs before its placement attempts, so an exhausted building remains
/// as a discarded event even though it contributes no MapFile entity.
pub(crate) fn run_traced(
    ctx: &mut TechCtx<'_>,
    args: &TechArgs,
    trace: &mut RmgConstructionTrace,
    entity_index_base: usize,
) -> Vec<TechPlacement> {
    // `RandomMapGenerator::AddTechBuildings @ 0x005A95B0`, which `Generate`
    // calls only for a non-zero map type. Its pass and building counts are
    // `ftol(draw * [0x007ED8C0])` (0x005A95D9) and `ftol(draw * [0x007EDAB0])`
    // (0x005A965F), redrawn while above 2 and 4: those constants are exactly
    // 3K and 5K, so they round the same product as `uniform`.
    let mut placements = Vec::new();
    // The driver is not called at all for map type 0.
    if args.map_type == 0 || ctx.types.is_empty() {
        return placements;
    }

    if args.map_type == 2 {
        let passes = ctx.rng.uniform(0, 2);
        for _ in 0..passes {
            // Regions in creation order; only quota-bearing regions build.
            for index in 0..ctx.regions.list.len() {
                if ctx.regions.list[index].start_quota <= 0 {
                    continue;
                }
                place_from_region(ctx, args, index, &mut placements, trace, entity_index_base);
            }
        }
    } else {
        let count = ctx.rng.uniform(0, 4);
        for _ in 0..count {
            place_whole_map(ctx, args, &mut placements, trace, entity_index_base);
        }
    }
    placements
}

/// Map type 0's tech buildings: `CreateStartingPoints @ 0x00594B50`'s tail
/// (0x00594EA6..0x00594ED5) walks the regions in bucket order and, for each
/// with no start quota, draws `draw * K < 0.5`; heads place one building in
/// that region at once, so its draws interleave with the later flips. It runs
/// before `MarkStartAreas`, whose clearings the foundation gate would reject.
pub(crate) fn run_start_flips_traced(
    ctx: &mut TechCtx<'_>,
    args: &TechArgs,
    region_order: &[usize],
    trace: &mut RmgConstructionTrace,
    entity_index_base: usize,
) -> Vec<TechPlacement> {
    let mut placements = Vec::new();
    if args.map_type != 0 {
        return placements;
    }
    for &index in region_order {
        if ctx.regions.list[index].start_quota != 0 {
            continue;
        }
        let draw = TruncF64::from_f64(f64::from(ctx.rng.next_u32()))
            .mul(TruncF64::from_f64(f64::from_bits(RANGE_K_BITS)));
        // With no NeutralTechBuildings the original would construct from an
        // empty list; retail always lists some, so the port only flips.
        if draw.to_f64() < 0.5 && !ctx.types.is_empty() {
            place_from_region(ctx, args, index, &mut placements, trace, entity_index_base);
        }
    }
    placements
}

/// Draw one building type index into the resolved list.
fn draw_type(ctx: &mut TechCtx<'_>) -> usize {
    ctx.rng.uniform(0, ctx.types.len() as i32 - 1) as usize
}

/// One building in a region (`RandomMapGenerator::PlaceTechBuildingInRegion @
/// 0x00595400`, for the map-type-2 driver and map type 0's start flips): one
/// type draw, then up to 100 anchors drawn from the region's cell list.
fn place_from_region(
    ctx: &mut TechCtx<'_>,
    args: &TechArgs,
    region_index: usize,
    out: &mut Vec<TechPlacement>,
    trace: &mut RmgConstructionTrace,
    entity_index_base: usize,
) {
    let type_index = draw_type(ctx);
    let event = trace.push_discarded(
        RmgConstructionPhase::NeutralTech,
        ctx.types[type_index].name.clone(),
    );
    let cell_count = ctx.regions.list[region_index].cells.len() as i32;
    debug_assert!(cell_count > 0, "surviving regions own cells");

    for _ in 0..MAX_ATTEMPTS {
        let pick = ctx.rng.uniform(0, cell_count - 1) as usize;
        let (cx, cy) = ctx.regions.list[region_index].cells[pick];
        let anchor = (i32::from(cx), i32::from(cy));
        if try_place(ctx, args, type_index, anchor, out) {
            let placement = out.last().expect("successful placement was appended");
            trace.mark_emitted(
                event,
                entity_index_base + out.len() - 1,
                (placement.x as u16, placement.y as u16),
            );
            return;
        }
    }
}

/// Map-types-1/3/4 path: anchor is a uniform random clear scratch cell over
/// the whole array, with the inner empty-slot / non-clear rejection.
fn place_whole_map(
    ctx: &mut TechCtx<'_>,
    args: &TechArgs,
    out: &mut Vec<TechPlacement>,
    trace: &mut RmgConstructionTrace,
    entity_index_base: usize,
) {
    let type_index = draw_type(ctx);
    let event = trace.push_discarded(
        RmgConstructionPhase::NeutralTech,
        ctx.types[type_index].name.clone(),
    );
    for _ in 0..MAX_ATTEMPTS {
        let anchor = draw_clear_anchor(ctx, args.stride);
        if try_place(ctx, args, type_index, anchor, out) {
            let placement = out.last().expect("successful placement was appended");
            trace.mark_emitted(
                event,
                entity_index_base + out.len() - 1,
                (placement.x as u16, placement.y as u16),
            );
            return;
        }
    }
}

/// Draw a clear map anchor: reject empty (coord `(0,0)`) scratch slots on
/// every draw, and reject non-clear cells for up to 200 non-empty draws. On
/// exhaustion the original keeps a `(0,0)` anchor, which fails the gate — so
/// the port returns that sentinel and lets `try_place` reject it.
fn draw_clear_anchor(ctx: &mut TechCtx<'_>, stride: i32) -> (i32, i32) {
    let max = stride * stride - 1;
    let mut non_empty_draws = 0;
    loop {
        // Redraw until the drawn slot is a real cell (non-`(0,0)` coord).
        let (x, y) = loop {
            let idx = ctx.rng.uniform(0, max);
            let (cx, cy) = (idx % stride, idx / stride);
            let record = ctx.scratch.get(cx, cy);
            if (record.x, record.y) != (0, 0) {
                break (cx, cy);
            }
        };
        non_empty_draws += 1;
        if non_empty_draws > MAX_ANCHOR_DRAWS {
            return (0, 0);
        }
        let clear = ctx
            .grid
            .get(x, y)
            .is_some_and(|cell| ctx.ids.is_clear(cell.tile));
        if clear {
            return (x, y);
        }
    }
}

/// Validate a building's footprint at `anchor` and, on success, mark the
/// cells occupied and record the placement. Returns whether it placed.
fn try_place(
    ctx: &mut TechCtx<'_>,
    args: &TechArgs,
    type_index: usize,
    anchor: (i32, i32),
    out: &mut Vec<TechPlacement>,
) -> bool {
    let anchor_level = match ctx.grid.get(anchor.0, anchor.1) {
        Some(cell) => cell.level,
        // A `(0,0)`/out-of-band anchor fails immediately (border cell).
        None => return false,
    };

    let footprint = ctx.types[type_index].footprint.clone();
    for &(dx, dy) in &footprint {
        let (fx, fy) = (anchor.0 + i32::from(dx), anchor.1 + i32::from(dy));
        if !foundation_cell_ok(ctx, args, fx, fy, anchor_level) {
            return false;
        }
    }

    // All foundation cells passed: occupy them and record the building.
    for &(dx, dy) in &footprint {
        let (fx, fy) = (anchor.0 + i32::from(dx), anchor.1 + i32::from(dy));
        if let Some(cell) = ctx.grid.get_mut(fx, fy) {
            cell.occupied = true;
        }
    }
    out.push(TechPlacement {
        name: ctx.types[type_index].name.clone(),
        x: anchor.0 as i16,
        y: anchor.1 as i16,
    });
    true
}

/// One foundation cell's gate (`AddTechBuildings @ 0x005A95B0` and
/// `PlaceTechBuildingInRegion @ 0x00595400`): unoccupied, clear,
/// level-matched, not Rock, inside the playfield frame, and outside every
/// start's protected clearing. A clear tile is Rock when the CliffBack
/// reclass of `RecalcAttributes @ 0x0047D2B0` put it behind a cliff.
fn foundation_cell_ok(
    ctx: &TechCtx<'_>,
    args: &TechArgs,
    x: i32,
    y: i32,
    anchor_level: u8,
) -> bool {
    let Some(cell) = ctx.grid.get(x, y) else {
        return false;
    };
    if cell.occupied {
        return false;
    }
    if !ctx.ids.is_clear(cell.tile) {
        return false;
    }
    if cell.level != anchor_level {
        return false;
    }
    if recalc_land(ctx.grid, ctx.blocks, args.cliff_back_impassability, x, y) == LAND_ROCK {
        return false;
    }
    if !diamond_frame_contains(
        args.map_w,
        args.local_rect,
        x,
        y,
        cell.level,
        cell.slope != 0,
    ) {
        return false;
    }
    // Protected start-clearing flag (scratch +0x45).
    if ctx.scratch.get(x, y).water_lock {
        return false;
    }
    true
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

    /// One-cell clear blocks: every tile reads land type Clear.
    struct ClearBlocks;

    impl TileBlocks for ClearBlocks {
        fn block(&self, _tile: i32) -> Option<&crate::map::rmg::phases::shore::TileBlock> {
            None
        }
    }

    fn types() -> Vec<TechType> {
        // A 2x2 and a 1x1, so the type draw picks between distinct footprints.
        vec![
            TechType {
                name: "CATHOSP".into(),
                footprint: vec![(0, 0), (1, 0), (0, 1), (1, 1)],
            },
            TechType {
                name: "CAPOWR".into(),
                footprint: vec![(0, 0)],
            },
        ]
    }

    struct World {
        grid: RmgGrid,
        scratch: RmgScratch,
        args: TechArgs,
    }

    fn world(map_type: i32) -> World {
        let (gen_w, gen_h) = (40, 36);
        let (map_w, map_h) = (gen_w + 4, gen_h + 12);
        let stride = map_w + map_h + 1;
        let (dmin, dmax) = (map_w, map_w + 2 * map_h);
        let mut grid = RmgGrid::new(stride as usize, dmin, dmax);
        let scratch = RmgScratch::new(stride as usize, dmin, dmax);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().tile = 0;
        }
        World {
            grid,
            scratch,
            args: TechArgs {
                map_type,
                map_w,
                local_rect: [2, 5, gen_w, gen_h],
                stride,
                cliff_back_impassability: 2,
            },
        }
    }

    #[test]
    fn tech_placement_is_deterministic() {
        let snapshot = |seed| {
            let mut w = world(1);
            let identity = ids();
            let type_list = types();
            let regions = Regions::default();
            let mut rng = RmgRng::new(seed);
            let mut ctx = TechCtx {
                grid: &mut w.grid,
                scratch: &mut w.scratch,
                ids: &identity,
                blocks: &ClearBlocks,
                rng: &mut rng,
                regions: &regions,
                types: &type_list,
            };
            let mut ignored_trace = RmgConstructionTrace::default();
            let placements = run_traced(&mut ctx, &w.args, &mut ignored_trace, 0);
            (placements, rng.next_u32())
        };
        assert_eq!(snapshot(2024), snapshot(2024));
    }

    #[test]
    fn foundation_cell_behind_a_cliff_is_rejected() {
        let mut w = world(1);
        let (x, y) = (60, 40);
        let level = w.grid.get(x, y).unwrap().level;
        w.grid.get_mut(x, y - 1).unwrap().level = level + 4;
        let identity = ids();
        let regions = Regions::default();
        let mut rng = RmgRng::new(1);
        let ctx = TechCtx {
            grid: &mut w.grid,
            scratch: &mut w.scratch,
            ids: &identity,
            blocks: &ClearBlocks,
            rng: &mut rng,
            regions: &regions,
            types: &[],
        };
        // A cell four levels under its (x, y-1) neighbour reads Rock
        // (0x0047D2B0); its (x+1, y) neighbour probes other cells.
        assert!(!foundation_cell_ok(&ctx, &w.args, x, y, level));
        assert!(foundation_cell_ok(&ctx, &w.args, x + 1, y, level));
        let off = TechArgs {
            cliff_back_impassability: 1,
            ..w.args
        };
        assert!(foundation_cell_ok(&ctx, &off, x, y, level));
    }
}
