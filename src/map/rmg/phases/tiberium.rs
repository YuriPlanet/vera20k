//! Tiberium (ore/gem) field placement, the last resource-shaping phase.
//!
//! Per surviving region (native creation order, a running `global_start_base`
//! across them):
//! - an optional gem-anchor pick (one pass-1 field becomes gem),
//! - a two-stage-truncated field-count formula → per-field size,
//! - pass 1: one BFS-grown field per field slot, size jittered by a Gaussian,
//! - pass 2 (unconditional): one field per start, sized by the start's mean
//!   distance to the region's field slots.
//!
//! The BFS placer grows a closest-first blob from an origin: a min-heap keyed
//! by `approx_sqrt(dist²) + uniform[0,5]`, up to 10 re-seeds, writing ore/gem
//! overlays and one `TIBTRE` tree per seed generation.
//!
//! Native: `RandomMapGenerator::CreateTiberium @ 0x005A23A0` (the driver) and
//! its field placer `RandomMapGenerator::PlaceTiberiumField @ 0x005A28C0`.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::RmgRng;
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::{self, Gaussian, TruncF64, approx_sqrt_f32};
use crate::util::native_x87::X87Chop53;

use super::regions::Regions;
use super::zones::diamond_frame_contains;
use crate::util::native_heap::NativeHeap;

/// Ore/gem overlay base indices (`0x66` / `0x1B`).
pub(crate) const ORE_BASE: i32 = 102;
pub(crate) const GEM_BASE: i32 = 27;
/// Density levels an overlay spans, from `base` to `base + 11`.
pub(crate) const DENSITY_LEVELS: i32 = 12;
/// Percent scaling of the Tiberium option.
const PERCENT: f64 = 0.01;
/// Gaussian field-size jitter scale and its rejection window.
const GAUSS_SCALE: f64 = 50.0;
const JITTER_LOW: f64 = -100.0;
const JITTER_HIGH: f64 = 100.0;
/// Start-count multiplier floor.
const MULT_FLOOR: f64 = 0.5;
/// Gem-anchor running-min seed.
const ANCHOR_MIN_INIT: i32 = 500_000;
/// Pass-2 size = `trunc(Δscore * 15) + 500`.
const GEM_SIZE_SCALE: f64 = 15.0;
const GEM_SIZE_BASE: i32 = 500;
/// Pass-2 score running-min seed.
const SCORE_MIN_INIT: f64 = 9_999_999.0;
/// Heap capacity multiplier and re-seed cap.
const HEAP_CAP_MULT: i32 = 10;
const MAX_SEEDS: i32 = 10;
/// TIBTRE variant draw scale (`~3·2⁻³²`, same constant as the start lane draw).
const K3_BITS: u64 = 0x3E08_0000_0018_0000;
/// Overlay density draw scale (`~12·2⁻³²`).
const K12_BITS: u64 = 0x3E28_0000_0018_0000;

/// Phase inputs (the dialog / RMGMD.INI fields the driver reads).
#[derive(Debug, Clone, Copy)]
pub struct TiberiumArgs {
    pub map_type: i32,
    /// `Resources` option; gems appear only when this is 3.
    pub resources: i32,
    /// `Tiberium` percent option (0..100), the field-count lerp parameter.
    pub tib_option: i32,
    /// `RMGMinimumTiberium` / `RMGMaximumTiberium`.
    pub min_tib: i32,
    pub max_tib: i32,
    pub map_w: i32,
    /// Playfield local rect `{x, y, w, h}`.
    pub local_rect: [i32; 4],
}

/// Everything the phase borrows.
pub struct TiberiumCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    pub rng: &'a mut RmgRng,
    /// Shared with the water/blob phases — the Box-Muller cache persists.
    pub gauss: &'a mut Gaussian,
    pub regions: &'a Regions,
    /// Start waypoints from the starts phase, indexed by slot.
    pub waypoints: &'a [Option<(i16, i16)>],
}

/// Phase result: the TIBTRE trees placed (overlays/densities land on the grid).
#[derive(Debug, Default)]
pub struct TiberiumOutcome {
    /// `(name, x, y)` per placed TIBTRE tree.
    pub trees: Vec<(String, i16, i16)>,
}

/// The pass-1 gem flag, by map type.
fn pass1_gem_flag(map_type: i32, resources: i32) -> bool {
    match map_type {
        0 => resources == 3,
        1 | 3 | 4 => resources != 3,
        2 => true,
        _ => false,
    }
}

pub fn run(ctx: &mut TiberiumCtx<'_>, args: &TiberiumArgs) -> TiberiumOutcome {
    let mut outcome = TiberiumOutcome::default();
    let bvar12 = pass1_gem_flag(args.map_type, args.resources);
    let gem2 = args.resources == 3 && matches!(args.map_type, 1 | 3 | 4);
    let span = args.max_tib - args.min_tib;

    let mut global_start_base = 0i32;
    for index in 0..ctx.regions.list.len() {
        // Skip regions the start selector produced no field-slot list for.
        if ctx.regions.list[index].field_slots.is_none() {
            continue;
        }
        let quota = ctx.regions.list[index].start_quota;

        // Optional gem-anchor slot (one pass-1 field becomes gem).
        let nearest_slot = if bvar12 {
            gem_anchor(ctx, index, global_start_base, quota)
        } else {
            -1
        };

        let field_count = ctx.regions.list[index].field_slots.as_ref().unwrap().len() as i32;
        // Field-count formula: two-stage truncation, no RNG.
        let lerp = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(args.tib_option))
                .mul(TruncF64::from_f64(PERCENT))
                .mul(TruncF64::from_f64(f64::from(span)))
                .add(TruncF64::from_f64(f64::from(args.min_tib)))
                .to_f64(),
        );
        let mult = if (f64::from(quota)) < MULT_FLOOR {
            MULT_FLOOR
        } else {
            f64::from(quota)
        };
        let region_total = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(lerp))
                .mul(TruncF64::from_f64(mult))
                .to_f64(),
        );
        if field_count == 0 || region_total == 0 {
            continue;
        }
        let per_field_base = region_total / field_count;

        // Pass 1: one field per slot, Gaussian-jittered size.
        for i in 0..field_count {
            let j = loop {
                let value = TruncF64::from_f64(ctx.gauss.next(ctx.rng))
                    .mul(TruncF64::from_f64(GAUSS_SCALE))
                    .to_f64();
                if (JITTER_LOW..=JITTER_HIGH).contains(&value) {
                    break value;
                }
            };
            let size = jittered_field_size(per_field_base, j);
            if size >= 0 {
                let origin = ctx.regions.list[index].field_slots.as_ref().unwrap()[i as usize];
                let is_gem = i == nearest_slot;
                place_field(
                    ctx,
                    args,
                    (i32::from(origin.0), i32::from(origin.1)),
                    size,
                    global_start_base + i + 1,
                    is_gem,
                    &mut outcome,
                );
            }
        }

        // Pass 2: one field per start, distance-compensated size.
        // 0x005A26D0..0x005A2719: the sum and the mean are chopped double
        // operations (`FADD`, `FDIVR`) under the generator's control word.
        let mut scores: Vec<f64> = Vec::with_capacity(quota.max(0) as usize);
        for s in 0..quota {
            let start = waypoint(ctx, global_start_base + s);
            let mut sum = TruncF64::zero();
            for &slot in ctx.regions.list[index].field_slots.as_ref().unwrap() {
                let dx = i32::from(start.0) - i32::from(slot.0);
                let dy = i32::from(start.1) - i32::from(slot.1);
                sum = TruncF64::from_f64(f64::from(approx_sqrt_f32(dx * dx + dy * dy))).add(sum);
            }
            scores.push(mean_distance(sum, field_count));
        }
        let min_score = scores.iter().copied().fold(SCORE_MIN_INIT, f64::min);
        for s in 0..quota {
            let size = X87Chop53::ftol_f64_low_masked(
                TruncF64::from_f64(scores[s as usize])
                    .sub(TruncF64::from_f64(min_score))
                    .mul(TruncF64::from_f64(GEM_SIZE_SCALE))
                    .to_f64(),
            ) + GEM_SIZE_BASE;
            let start = waypoint(ctx, global_start_base + s);
            place_field(
                ctx,
                args,
                (i32::from(start.0), i32::from(start.1)),
                size,
                global_start_base + s + 1,
                gem2,
                &mut outcome,
            );
        }

        global_start_base += quota;
    }
    outcome
}

/// A pass-1 field's size: `ftol(FILD base; FADD jitter)` at 0x005A25FB, a
/// chopped double add.
fn jittered_field_size(per_field_base: i32, jitter: f64) -> i32 {
    X87Chop53::ftol_f64_low_masked(
        TruncF64::from_f64(f64::from(per_field_base))
            .add(TruncF64::from_f64(jitter))
            .to_f64(),
    )
}

/// A start's mean distance to the field slots: `sum / count`, the chopped
/// `FDIVR` at 0x005A2715.
fn mean_distance(sum: TruncF64, count: i32) -> f64 {
    sum.div(TruncF64::from_f64(f64::from(count))).to_f64()
}

/// Read a start waypoint slot, treating an unwritten slot as the origin (the
/// original reads whatever the scenario slot holds; unwritten slots have no
/// defined content to reproduce).
fn waypoint(ctx: &TiberiumCtx<'_>, slot: i32) -> (i16, i16) {
    ctx.waypoints
        .get(slot.max(0) as usize)
        .copied()
        .flatten()
        .unwrap_or((0, 0))
}

/// The pass-1 gem-anchor slot: the field slot nearest the region's reference
/// point (average start waypoint, or a random region cell when there are no
/// starts). Running min is truncated on each update, matching the original.
fn gem_anchor(
    ctx: &mut TiberiumCtx<'_>,
    region_index: usize,
    global_start_base: i32,
    quota: i32,
) -> i32 {
    let reference = if quota >= 1 {
        let mut sum = (0i32, 0i32);
        for i in 0..quota {
            let way = waypoint(ctx, global_start_base + i);
            sum.0 += i32::from(way.0);
            sum.1 += i32::from(way.1);
        }
        (sum.0 / quota, sum.1 / quota)
    } else {
        let cells = &ctx.regions.list[region_index].cells;
        let idx = ctx.rng.uniform(0, cells.len() as i32 - 1) as usize;
        let cell = cells[idx];
        (i32::from(cell.0), i32::from(cell.1))
    };

    let mut min = ANCHOR_MIN_INIT;
    let mut nearest = -1i32;
    for (i, slot) in ctx.regions.list[region_index]
        .field_slots
        .as_ref()
        .unwrap()
        .iter()
        .enumerate()
    {
        let dx = reference.0 - i32::from(slot.0);
        let dy = reference.1 - i32::from(slot.1);
        let dist = approx_sqrt_f32(dx * dx + dy * dy);
        if f64::from(dist) < f64::from(min) {
            min = X87Chop53::ftol_f64_low_masked(f64::from(dist));
            nearest = i as i32;
        }
    }
    nearest
}

/// The TIBTRE variant draw: `trunc(rand * ~3·2⁻³² + 1.0)`, rejected above 3.
fn tibtre_draw(rng: &mut RmgRng) -> i32 {
    let scale = TruncF64::from_f64(f64::from_bits(K3_BITS));
    let one = TruncF64::from_f64(1.0);
    loop {
        let value = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(rng.next_u32()))
                .mul(scale)
                .add(one)
                .to_f64(),
        );
        if value <= 3 {
            return value;
        }
    }
}

/// The overlay density draw: `trunc(rand * ~12·2⁻³²)`, rejected above 11.
fn density_draw(rng: &mut RmgRng) -> i32 {
    let scale = TruncF64::from_f64(f64::from_bits(K12_BITS));
    loop {
        let value = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(rng.next_u32()))
                .mul(scale)
                .to_f64(),
        );
        if value <= 11 {
            return value;
        }
    }
}

/// Whether an overlay index is one this phase's tiberium (matches the
/// original's `GetTiberiumType != -1` on generated maps, where the only
/// overlays present are the ore/gem it places).
fn is_tiberium_overlay(overlay: i32) -> bool {
    (GEM_BASE..GEM_BASE + DENSITY_LEVELS).contains(&overlay)
        || (ORE_BASE..ORE_BASE + DENSITY_LEVELS).contains(&overlay)
}

/// Grow one tiberium field from `origin`. Closest-first BFS with up to 10
/// re-seeds; writes overlays/densities on the grid and records TIBTRE trees.
///
/// `PlaceTiberiumField @ 0x005A28C0`. The heap holds pointers into one node
/// pool of `10 * size` nodes; `local_64` is the next free node. A re-seed puts
/// the origin in node 0 and sets it to 1 (0x005A2A25); the first write rebinds
/// the anchor, empties the heap and sets it to 0 (0x005A2B80). Every admission writes the
/// next node (0x005A2D40) before the heap's full check, and the expanding node
/// re-reads its coordinate from its node for each direction (0x005A2C8E). So
/// when the expansion right after the first write starts from node 0 (or any
/// node below the admissions), an admitted neighbour overwrites it, and the
/// remaining directions step from that neighbour.
///
/// Residual: native's pool is a fixed `10 * size` allocation; admissions past
/// it write beyond the block (heap corruption). Here the pool grows. Reaching
/// it needs more than ten admissions per placed cell since the last reset.
/// The heap's own `count + 1 < 10 * size` refusal (0x005A2DEF) is reachable
/// only after that overrun: every push uses a fresh node and every pass pops
/// one, so the heap holds fewer nodes than the pool has handed out.
///
/// `tools/rmg_oracle/tiberium_field.py` replays whole original calls of this
/// function and of `CreateTiberium` against it.
fn place_field(
    ctx: &mut TiberiumCtx<'_>,
    args: &TiberiumArgs,
    origin: (i32, i32),
    target_size: i32,
    field_id: i32,
    is_gem: bool,
    out: &mut TiberiumOutcome,
) {
    if target_size <= 0 {
        return;
    }
    let base_overlay = if is_gem { GEM_BASE } else { ORE_BASE };
    let cap = (target_size * HEAP_CAP_MULT) as usize;

    let mut anchor = origin;
    let mut placed = 0i32;
    let mut seed_count = 0i32;
    let mut heap = NativeHeap::<(f32, usize)>::new(cap);
    let by_priority = |a: &(f32, usize), b: &(f32, usize)| a.0 < b.0;
    let mut pool: Vec<(i32, i32)> = Vec::with_capacity(cap);
    let mut next_node = 0usize;
    let mut current: Option<usize> = None;
    let mut first_written = false;

    while placed < target_size {
        if seed_count >= MAX_SEEDS {
            break;
        }
        if current.is_none() {
            // New seed: wipe every claim, re-seed from the same origin.
            ctx.scratch.clear_stamps();
            heap.clear();
            write_node(&mut pool, 0, origin);
            next_node = 1;
            ctx.scratch.get_mut(origin.0, origin.1).stamp = field_id;
            heap.push((0.0, 0), by_priority);
            current = heap.pop(by_priority).map(|(_, node)| node);
            seed_count += 1;
            anchor = origin;
            first_written = false;
            if current.is_none() {
                break;
            }
        }
        let node = current.unwrap();
        let cur = pool[node];

        // Pop-write path, gated on the protected/blocked flag (scratch +0x45).
        let blocked = ctx.scratch.get(cur.0, cur.1).water_lock;
        if !blocked {
            if !first_written {
                anchor = cur;
                first_written = true;
                // Rebinding the anchor discards the current frontier and
                // restarts the pool.
                heap.clear();
                next_node = 0;
                if !is_gem {
                    let variant = tibtre_draw(ctx.rng);
                    out.trees
                        .push((format!("TIBTRE0{variant}"), cur.0 as i16, cur.1 as i16));
                    // RMG PlaceField5A28C0 calls TerrainCtor71BB90 at
                    //5A2C05, before painting5A2C12. Its Unlimbo clears an
                    // old resource first, including on overlapping/reseeded
                    // anchors, so the following empty-cell draw still occurs.
                    // ObjectClass::Unlimbo 5F4EC0 admits it without
                    // Can_Enter_Cell (5F4F1B): Generate holds ScenarioInit
                    // A8E7AC raised from 598A9A to 59934A around this phase
                    // (598EF4), so an ore cell does not refuse the tree.
                    if let Some(cell) = ctx.grid.get_mut(cur.0, cur.1) {
                        (cell.overlay, cell.density) =
                            crate::map::tiberium_cell::terrain_unlimbo_overlay_fields(
                                cell.overlay,
                                cell.density,
                                is_tiberium_overlay,
                            );
                        cell.occupied = true;
                    }
                }
            }
            let mut counted = true;
            if let Some(cell) = ctx.grid.get_mut(cur.0, cur.1) {
                if cell.overlay == -1 {
                    let d = density_draw(ctx.rng);
                    cell.overlay = d + base_overlay;
                } else if i32::from(cell.density) < 11 {
                    cell.density += 1;
                } else {
                    // Saturated: no new resource, but neighbors still expand.
                    counted = false;
                }
            }
            if counted {
                placed += 1;
            }
        }

        // Neighbor admission (runs whether or not the pop wrote resource).
        for dir in 0..8 {
            let from = pool[node];
            let (nx, ny) = RmgGrid::step(from.0, from.1, dir);
            let Some(cell) = ctx.grid.get(nx, ny) else {
                continue;
            };
            if !diamond_frame_contains(
                args.map_w,
                args.local_rect,
                nx,
                ny,
                cell.level,
                cell.slope != 0,
            ) {
                continue;
            }
            if !ctx.ids.is_clear(cell.tile) {
                continue;
            }
            let overlay = cell.overlay;
            let density = i32::from(cell.density);
            let claimed = ctx.scratch.get(nx, ny).stamp == field_id;
            let admit =
                (overlay == -1 && !claimed) || (density < 11 && is_tiberium_overlay(overlay));
            if !admit {
                continue;
            }
            let dx = anchor.0 - nx;
            let dy = anchor.1 - ny;
            let priority = x87::jittered_distance_key(dx * dx + dy * dy, ctx.rng.next_u32());
            write_node(&mut pool, next_node, (nx, ny));
            ctx.scratch.get_mut(nx, ny).stamp = field_id;
            heap.push((priority, next_node), by_priority);
            next_node += 1;
        }

        current = heap.pop(by_priority).map(|(_, node)| node);
    }
}

/// Store a coordinate in pool node `index`, growing the pool past the native
/// allocation instead of overrunning it.
fn write_node(pool: &mut Vec<(i32, i32)>, index: usize, coord: (i32, i32)) {
    if index == pool.len() {
        pool.push(coord);
    } else {
        pool[index] = coord;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::phases::regions::RmgRegion;
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

    fn args(map_type: i32, resources: i32) -> (TiberiumArgs, i32) {
        let (gen_w, gen_h) = (40, 36);
        let (map_w, map_h) = (gen_w + 4, gen_h + 12);
        let stride = map_w + map_h + 1;
        (
            TiberiumArgs {
                map_type,
                resources,
                tib_option: 50,
                min_tib: 900,
                max_tib: 1050,
                map_w,
                local_rect: [2, 5, gen_w, gen_h],
            },
            stride,
        )
    }

    struct World {
        grid: RmgGrid,
        scratch: RmgScratch,
    }

    fn world(stride: i32) -> World {
        let (dmin, dmax) = (44, 44 + 2 * 48);
        let mut grid = RmgGrid::new(stride as usize, dmin, dmax);
        let scratch = RmgScratch::new(stride as usize, dmin, dmax);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            grid.get_mut(x, y).unwrap().tile = 0;
        }
        World { grid, scratch }
    }

    /// One region whose field slots and cells sit in a compact central block.
    fn regions_with_field(grid: &RmgGrid, field_slots: Vec<(i16, i16)>, quota: i32) -> Regions {
        let cells: Vec<(i16, i16)> = grid
            .native_cells()
            .filter(|&(x, y)| (25..40).contains(&x) && (25..40).contains(&y))
            .map(|(x, y)| (x as i16, y as i16))
            .collect();
        let mut regions = Regions::default();
        regions.list.push(RmgRegion {
            id: 0,
            level: 4,
            active: false,
            cell_count: cells.len() as i32,
            cells,
            start_quota: quota,
            field_slots: Some(field_slots),
        });
        regions.id_counter = 1;
        regions
    }

    /// 0x005A2B80 restarts the pool under the node being expanded: the
    /// origin's first admitted neighbour overwrites node 0, and the remaining
    /// directions step from that neighbour, two cells out.
    #[test]
    fn the_first_expansion_steps_from_the_overwritten_node() {
        let (targs, stride) = args(1, 0);
        let mut w = world(stride);
        let identity = ids();
        let regions = Regions::default();
        let mut rng = RmgRng::new(7);
        let mut gauss = Gaussian::default();
        let origin = (32, 32);
        let mut out = TiberiumOutcome::default();
        let mut ctx = TiberiumCtx {
            grid: &mut w.grid,
            scratch: &mut w.scratch,
            ids: &identity,
            rng: &mut rng,
            gauss: &mut gauss,
            regions: &regions,
            waypoints: &[],
        };
        place_field(&mut ctx, &targs, origin, 1, 5, true, &mut out);
        let first = RmgGrid::step(origin.0, origin.1, 0);
        let (x, y) = RmgGrid::step(first.0, first.1, 1);
        assert_eq!((x - origin.0).abs().max((y - origin.1).abs()), 2);
        assert_eq!(w.scratch.get(x, y).stamp, 5);
    }

    fn int(value: &serde_json::Value) -> i32 {
        value.as_i64().unwrap() as i32
    }

    fn pair(value: &serde_json::Value) -> (i16, i16) {
        (int(&value[0]) as i16, int(&value[1]) as i16)
    }

    /// The harness's declared scene: clear level-0 cells, optional non-clear
    /// (CliffSet base) cells, `+0x45` locks and prior overlay/density pairs.
    fn native_scene(input: &serde_json::Value, cliff: i32) -> (RmgGrid, RmgScratch) {
        let mut grid = RmgGrid::new(33, 16, 48);
        let mut scratch = RmgScratch::new(33, 16, 48);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            let cell = grid.get_mut(x, y).unwrap();
            (cell.tile, cell.sub_tile, cell.level) = (0, 0, 0);
        }
        let list = |key: &str| input.get(key).map_or(&[][..], |v| v.as_array().unwrap());
        for c in list("rough") {
            grid.get_mut(int(&c[0]), int(&c[1])).unwrap().tile = cliff;
        }
        for c in list("locked") {
            scratch.get_mut(int(&c[0]), int(&c[1])).water_lock = true;
        }
        for c in list("ore") {
            let cell = grid.get_mut(int(&c[0]), int(&c[1])).unwrap();
            (cell.overlay, cell.density) = (int(&c[2]), int(&c[3]) as u8);
        }
        (grid, scratch)
    }

    /// `[x, y, overlay, density, stamp]` per in-diamond cell, row-major.
    fn native_fields(grid: &RmgGrid, scratch: &RmgScratch) -> Vec<[i32; 5]> {
        let mut out = Vec::new();
        for y in 0..33 {
            for x in 0..33 {
                if scratch.in_diamond(x, y) {
                    let cell = grid.get(x, y).unwrap();
                    let stamp = scratch.get(x, y).stamp;
                    out.push([x, y, cell.overlay, i32::from(cell.density), stamp]);
                }
            }
        }
        out
    }

    /// Compare the changed cells, the TIBTRE list and the exit RNG.
    fn assert_native_row(
        doc: &serde_json::Value,
        row: &serde_json::Value,
        prior: Vec<[i32; 5]>,
        world: (&RmgGrid, &RmgScratch),
        out: &TiberiumOutcome,
        rng: RmgRng,
    ) {
        let context = row["input"]["id"].as_str().unwrap();
        let trees: Vec<(String, i16, i16)> = row["trees"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                let name = t[0].as_str().unwrap().to_owned();
                (name, int(&t[1]) as i16, int(&t[2]) as i16)
            })
            .collect();
        assert_eq!(out.trees, trees, "{context}: trees");
        let cells: Vec<[i32; 5]> = native_fields(world.0, world.1)
            .into_iter()
            .zip(prior)
            .filter(|(now, before)| now != before)
            .map(|(now, _)| now)
            .collect();
        let native: Vec<[i32; 5]> = row["cells"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| std::array::from_fn(|i| int(&c[i])))
            .collect();
        assert_eq!(cells, native, "{context}: cells");
        assert_eq!(
            rng.into_continuation().into_native_parts(),
            super::super::cliff_native_tests::native_state(doc, &row["rng_after"])
                .into_native_parts(),
            "{context}: exit RNG"
        );
    }

    fn native_args(input: &serde_json::Value) -> TiberiumArgs {
        let field = |key: &str| input.get(key).map_or(0, int);
        let rect = input
            .get("rect")
            .map_or([0, 0, 16, 16], |r| std::array::from_fn(|i| int(&r[i])));
        TiberiumArgs {
            map_type: field("map_type"),
            resources: field("resources"),
            tib_option: field("tib_option"),
            min_tib: field("min_tib"),
            max_tib: field("max_tib"),
            map_w: 16,
            local_rect: rect,
        }
    }

    /// Whole original `PlaceTiberiumField @ 0x005A28C0` calls from
    /// `tools/rmg_oracle/tiberium_field.py`: overlays, densities, stamps,
    /// TIBTRE names and cells, and the exit RNG.
    #[test]
    fn native_place_field_controls() {
        let doc = crate::test_fixture::json("tools/rmg_oracle/tiberium_field.json");
        let (identity, _) = super::super::cliff_native_tests::physical_tiles();
        let cliff = int(&doc["scene"]["cliff_base"]);
        let rows = doc["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 9);
        for row in rows {
            let input = &row["input"];
            let (mut grid, mut scratch) = native_scene(input, cliff);
            let prior = native_fields(&grid, &scratch);
            let mut rng = RmgRng::new(input["seed"].as_u64().unwrap() as u16);
            let mut gauss = Gaussian::default();
            let mut out = TiberiumOutcome::default();
            let origin = pair(&input["origin"]);
            place_field(
                &mut TiberiumCtx {
                    grid: &mut grid,
                    scratch: &mut scratch,
                    ids: &identity,
                    rng: &mut rng,
                    gauss: &mut gauss,
                    regions: &Regions::default(),
                    waypoints: &[],
                },
                &native_args(input),
                (i32::from(origin.0), i32::from(origin.1)),
                int(&input["size"]),
                int(&input["field"]),
                int(&input["gem"]) != 0,
                &mut out,
            );
            assert_native_row(doc, row, prior, (&grid, &scratch), &out, rng);
        }
    }

    /// Whole original `CreateTiberium @ 0x005A23A0` calls from the same
    /// harness: gem anchors, field-count formula, Gaussian jitter, pass-2
    /// sizes and every field they place, plus the Gaussian cache left behind.
    #[test]
    fn native_create_tiberium_controls() {
        let doc = crate::test_fixture::json("tools/rmg_oracle/tiberium_field.json");
        let (identity, _) = super::super::cliff_native_tests::physical_tiles();
        let cliff = int(&doc["scene"]["cliff_base"]);
        let rows = doc["drivers"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        for row in rows {
            let input = &row["input"];
            let context = input["id"].as_str().unwrap();
            let (mut grid, mut scratch) = native_scene(input, cliff);
            let prior = native_fields(&grid, &scratch);
            let pairs = |v: &serde_json::Value| -> Vec<(i16, i16)> {
                v.as_array().unwrap().iter().map(pair).collect()
            };
            let mut regions = Regions::default();
            for (id, region) in input["regions"].as_array().unwrap().iter().enumerate() {
                let cells = pairs(&region["cells"]);
                regions.list.push(RmgRegion {
                    id: id as i32,
                    level: 0,
                    active: false,
                    cell_count: cells.len() as i32,
                    cells,
                    start_quota: int(&region["quota"]),
                    field_slots: (!region["slots"].is_null()).then(|| pairs(&region["slots"])),
                });
            }
            let waypoints: Vec<Option<(i16, i16)>> =
                pairs(&input["waypoints"]).into_iter().map(Some).collect();
            let mut rng = RmgRng::new(input["seed"].as_u64().unwrap() as u16);
            let mut gauss = Gaussian::default();
            let out = run(
                &mut TiberiumCtx {
                    grid: &mut grid,
                    scratch: &mut scratch,
                    ids: &identity,
                    rng: &mut rng,
                    gauss: &mut gauss,
                    regions: &regions,
                    waypoints: &waypoints,
                },
                &native_args(input),
            );
            // A cached variate comes back without a draw.
            let mut probe = RmgRng::new(0);
            let value = gauss.next(&mut probe);
            let cached = probe.next_u32() == RmgRng::new(0).next_u32();
            assert_eq!(
                cached,
                row["gauss_cached"].as_u64().unwrap() != 0,
                "{context}: cache"
            );
            if cached {
                assert_eq!(
                    value.to_bits(),
                    row["gauss_value_bits"].as_u64().unwrap(),
                    "{context}: cached variate"
                );
            }
            assert_native_row(doc, row, prior, (&grid, &scratch), &out, rng);
        }
    }

    /// The pass sizes round toward zero: the jittered size's add (0x005A25FF)
    /// and the start's mean distance (0x005A2715).
    #[test]
    fn field_size_arithmetic_chops() {
        assert_eq!(jittered_field_size(3000, -1e-13), 2999);
        let mean = mean_distance(TruncF64::from_f64(1.0), 5);
        assert_eq!(mean.to_bits(), 0.2f64.to_bits() - 1);
    }

    #[test]
    fn pass1_gem_flag_decision_table() {
        // From the driver's switch on map type + Resources == 3.
        assert!(pass1_gem_flag(0, 3));
        assert!(!pass1_gem_flag(0, 1));
        assert!(!pass1_gem_flag(1, 3));
        assert!(pass1_gem_flag(1, 0));
        assert!(!pass1_gem_flag(3, 3));
        assert!(!pass1_gem_flag(4, 3));
        assert!(pass1_gem_flag(2, 0));
        assert!(pass1_gem_flag(2, 3));
    }

    fn run_once(seed: u16, map_type: i32, resources: i32) -> (TiberiumOutcome, World, usize) {
        let (targs, stride) = args(map_type, resources);
        let mut w = world(stride);
        let identity = ids();
        let field_slots = vec![(30, 30), (34, 32), (31, 36)];
        let regions = regions_with_field(&w.grid, field_slots, 2);
        let waypoints = vec![Some((29i16, 29i16)), Some((35, 35))];
        let mut rng = RmgRng::new(seed);
        let mut gauss = Gaussian::default();
        let outcome = {
            let mut ctx = TiberiumCtx {
                grid: &mut w.grid,
                scratch: &mut w.scratch,
                ids: &identity,
                rng: &mut rng,
                gauss: &mut gauss,
                regions: &regions,
                waypoints: &waypoints,
            };
            run(&mut ctx, &targs)
        };
        let ore_cells = w
            .grid
            .native_cells()
            .filter(|&(x, y)| w.grid.get(x, y).unwrap().overlay != -1)
            .count();
        (outcome, w, ore_cells)
    }

    #[test]
    fn a_field_of_ore_is_laid_down() {
        let (outcome, world, ore_cells) = run_once(1234, 1, 0);
        assert!(ore_cells > 0, "ore overlays were written");
        // Every written overlay is a tiberium overlay in the ore or gem range.
        for (x, y) in world.grid.native_cells().collect::<Vec<_>>() {
            let overlay = world.grid.get(x, y).unwrap().overlay;
            if overlay != -1 {
                assert!(is_tiberium_overlay(overlay), "({x},{y}) overlay {overlay}");
            }
        }
        // Ore maps place TIBTRE trees (at least one per placed field).
        assert!(!outcome.trees.is_empty());
        for (name, _, _) in &outcome.trees {
            assert!(
                ["TIBTRE01", "TIBTRE02", "TIBTRE03"].contains(&name.as_str()),
                "tree name {name} is a valid TIBTRE variant"
            );
        }
    }

    #[test]
    fn ore_seed_constructor_replaces_prior_resource_before_the_painter_draw() {
        // RMG PlaceField5A28C0 calls TerrainCtor71BB90 at5A2C05 before
        // the overlay branch5A2C12. Unlimbo71D112..71D12B clears old
        // resource, so an overlapping field has the empty-cell RNG path.
        let place_seed = |prior_overlay, prior_density| {
            let (targs, stride) = args(1, 0);
            let mut w = world(stride);
            let source = w.grid.get_mut(32, 32).unwrap();
            source.overlay = prior_overlay;
            source.density = prior_density;
            let identity = ids();
            let mut rng = RmgRng::new(9);
            let mut gauss = Gaussian::default();
            let mut out = TiberiumOutcome::default();
            place_field(
                &mut TiberiumCtx {
                    grid: &mut w.grid,
                    scratch: &mut w.scratch,
                    ids: &identity,
                    rng: &mut rng,
                    gauss: &mut gauss,
                    regions: &Regions::default(),
                    waypoints: &[],
                },
                &targs,
                (32, 32),
                1,
                1,
                false,
                &mut out,
            );
            (out.trees, *w.grid.get(32, 32).unwrap(), rng.next_u32())
        };
        let empty = place_seed(-1, 0);
        let overlap = place_seed(ORE_BASE + 3, 9);
        assert_eq!(
            overlap, empty,
            "constructor clears before the resource draw"
        );
        assert!(overlap.1.occupied, "the constructed drill retains its cell");
    }

    #[test]
    fn gems_place_no_tibtre_trees() {
        // Water type + Resources 3 → gem2 pass 2 with no trees; pass 1 all ore.
        let (outcome, world, _) = run_once(77, 1, 3);
        // Gem overlays appear (pass 2 is gem).
        let has_gem = world.grid.native_cells().any(|(x, y)| {
            (GEM_BASE..GEM_BASE + DENSITY_LEVELS).contains(&world.grid.get(x, y).unwrap().overlay)
        });
        assert!(has_gem, "gem overlays laid by the gem second pass");
        // TIBTRE trees only come from ore (non-gem) fields in pass 1.
        assert!(
            outcome
                .trees
                .iter()
                .all(|(name, _, _)| name.starts_with("TIBTRE")),
        );
    }

    #[test]
    fn protected_cells_take_no_resource_but_still_conduct() {
        let (targs, stride) = args(1, 0);
        let mut w = world(stride);
        let identity = ids();
        let field_slots = vec![(32, 32)];
        let regions = regions_with_field(&w.grid, field_slots, 1);
        let waypoints = vec![Some((32i16, 32i16))];
        // Protect a ring around the origin: those cells conduct but stay bare.
        for (x, y) in w.grid.native_cells().collect::<Vec<_>>() {
            if (x - 32).abs() <= 1 && (y - 32).abs() <= 1 && (x, y) != (32, 32) {
                w.scratch.get_mut(x, y).water_lock = true;
            }
        }
        let mut rng = RmgRng::new(5);
        let mut gauss = Gaussian::default();
        {
            let mut ctx = TiberiumCtx {
                grid: &mut w.grid,
                scratch: &mut w.scratch,
                ids: &identity,
                rng: &mut rng,
                gauss: &mut gauss,
                regions: &regions,
                waypoints: &waypoints,
            };
            run(&mut ctx, &targs);
        }
        // The protected ring cells never received an overlay.
        for (x, y) in w.grid.native_cells().collect::<Vec<_>>() {
            if (x - 32).abs() <= 1 && (y - 32).abs() <= 1 && (x, y) != (32, 32) {
                assert_eq!(
                    w.grid.get(x, y).unwrap().overlay,
                    -1,
                    "protected ({x},{y}) stays bare"
                );
            }
        }
    }

    #[test]
    fn placement_is_deterministic() {
        let snapshot = |seed| {
            let (outcome, world, _) = run_once(seed, 1, 3);
            let overlays: Vec<(i32, u8)> = world
                .grid
                .native_cells()
                .collect::<Vec<_>>()
                .iter()
                .map(|&(x, y)| {
                    let cell = world.grid.get(x, y).unwrap();
                    (cell.overlay, cell.density)
                })
                .collect();
            (outcome.trees, overlays)
        };
        assert_eq!(snapshot(4242), snapshot(4242));
    }

    #[test]
    fn no_field_list_region_is_skipped() {
        let (targs, stride) = args(1, 0);
        let mut w = world(stride);
        let identity = ids();
        // A region with field_slots = None: no placement, no draws.
        let mut regions = Regions::default();
        regions.list.push(RmgRegion {
            id: 0,
            level: 4,
            active: false,
            cell_count: 1,
            cells: vec![(30, 30)],
            start_quota: 1,
            field_slots: None,
        });
        let waypoints = vec![Some((30i16, 30i16))];
        let mut rng = RmgRng::new(9);
        let mut gauss = Gaussian::default();
        let outcome = {
            let mut ctx = TiberiumCtx {
                grid: &mut w.grid,
                scratch: &mut w.scratch,
                ids: &identity,
                rng: &mut rng,
                gauss: &mut gauss,
                regions: &regions,
                waypoints: &waypoints,
            };
            run(&mut ctx, &targs)
        };
        assert!(outcome.trees.is_empty());
        let mut fresh = RmgRng::new(9);
        assert_eq!(rng.next_u32(), fresh.next_u32(), "no draws consumed");
    }
}
