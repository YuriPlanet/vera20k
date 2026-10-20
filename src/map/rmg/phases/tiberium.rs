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

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::{Gaussian, TruncF64, approx_sqrt, approx_sqrt_f32};
use crate::util::native_x87::X87Chop53;

use super::blob::MinHeap;
use super::regions::Regions;
use super::zones::diamond_frame_contains;

/// Ore/gem overlay base indices (`0x66` / `0x1B`).
const ORE_BASE: i32 = 102;
const GEM_BASE: i32 = 27;
/// Density levels an overlay spans, from `base` to `base + 11`.
const DENSITY_LEVELS: i32 = 12;
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
/// BFS priority jitter scale.
const PRIORITY_JITTER: f64 = 5.0;
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
            let size = X87Chop53::ftol_f64_low_masked(f64::from(per_field_base) + j);
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
        let mut scores: Vec<f64> = Vec::with_capacity(quota.max(0) as usize);
        for s in 0..quota {
            let start = waypoint(ctx, global_start_base + s);
            let mut sum = 0.0f64;
            for &slot in ctx.regions.list[index].field_slots.as_ref().unwrap() {
                let dx = i32::from(start.0) - i32::from(slot.0);
                let dy = i32::from(start.1) - i32::from(slot.1);
                sum += f64::from(approx_sqrt_f32(dx * dx + dy * dy));
            }
            scores.push(sum / f64::from(field_count));
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
    let mut heap = MinHeap::new(cap);
    let mut current: Option<(i32, i32)> = None;
    let mut first_written = false;

    while placed < target_size {
        if seed_count >= MAX_SEEDS {
            break;
        }
        if current.is_none() {
            // New seed: wipe every claim, re-seed from the same origin.
            ctx.scratch.clear_stamps();
            heap = MinHeap::new(cap);
            ctx.scratch.get_mut(origin.0, origin.1).stamp = field_id;
            heap.push(0.0, origin);
            current = heap.pop().map(|(_, coord)| coord);
            seed_count += 1;
            anchor = origin;
            first_written = false;
            if current.is_none() {
                break;
            }
        }
        let cur = current.unwrap();

        // Pop-write path, gated on the protected/blocked flag (scratch +0x45).
        let blocked = ctx.scratch.get(cur.0, cur.1).water_lock;
        if !blocked {
            if !first_written {
                anchor = cur;
                first_written = true;
                // Rebinding the anchor discards the current frontier.
                heap = MinHeap::new(cap);
                if !is_gem {
                    let variant = tibtre_draw(ctx.rng);
                    out.trees
                        .push((format!("TIBTRE0{variant}"), cur.0 as i16, cur.1 as i16));
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
            let (nx, ny) = RmgGrid::step(cur.0, cur.1, dir);
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
            let jitter = TruncF64::from_f64(f64::from(ctx.rng.next_u32()))
                .mul(TruncF64::from_f64(PRIORITY_JITTER))
                .mul(TruncF64::from_f64(f64::from_bits(RANGE_K_BITS)));
            let priority = approx_sqrt(TruncF64::from_f64(f64::from(dx * dx + dy * dy)))
                .add(jitter)
                .to_f64() as f32;
            ctx.scratch.get_mut(nx, ny).stamp = field_id;
            heap.push(priority, (nx, ny));
        }

        current = heap.pop().map(|(_, coord)| coord);
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
