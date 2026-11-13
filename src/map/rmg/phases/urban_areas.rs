//! Urban areas: paved districts with roads, civilian buildings, foot traffic
//! and plazas — the random-map urban pass `0x005A5020` and its subtree.
//!
//! Stock `RandomMapGenerator::Generate @ 0x00598960` never calls it. Ares'
//! `MapSeedClass_Generate_UrbanAreas` (Ares.dll 1006ABD0, hook 0x598FB8)
//! calls it right after the region reset when `RMG::UrbanAreas` is set; VERA's
//! opted-in Urban Presence runs it at the same point in any build
//! ([`UrbanArgs::vera_fill`]). Ares also patches the subtree, and this port
//! carries those patches in every build:
//! - `PlacePavedRoads @ 0x005A5B50`: the four road-end coins jump to their
//!   default piece (MapSeedClass_Generate_PlacePavedRoads_RoadEnd*, Ares.dll
//!   1006AA90/1006AAC0/1006AAA0/1006AAB0, hooks 0x5A5C6A/0x5A5D6F/0x5A5F6A/
//!   0x5A6464), so neither the probe's verdict nor a coin is used;
//! - `0x005A6510`: `[Urban] Structures=` replaces gamemd's eighteen CITY
//!   names (_Start 1006ABA0 hook 0x5A65CA, _Loop 1006AB70 hook 0x5A6619), and
//!   a name that is no BuildingType is skipped (_SanityCheck 1006AB90, hook
//!   0x5A66B0) where gamemd would index its array at -1;
//! - `0x005A6920`: the foot type is one raw process-Main draw modulo the
//!   `[Urban] Infantry=` plus `Vehicles=` count (_PlaceUrbanFoots 1006AAD0,
//!   hook 0x5A6998), replacing gamemd's MapGen draw over seven names.
//!
//! The 0x005A6C10 tile stamps all pass scratch id -1 and level -1.

use crate::map::entities::EntityCategory;
use crate::map::playfield::PlayfieldBounds;
use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::{TILE_UNASSIGNED, TileIds};
use crate::map::rmg::x87::{self, Gaussian, TruncF64};
use crate::map::rmg::{RmgConstructionPhase, RmgConstructionTrace};
use crate::util::native_x87::X87Chop53;

use super::area::{area_admits_paving, corners_in_diamond};
use super::blob::{CellHeap, by_key};
use super::carve::stamp_iso_block;
use super::shore::TileBlocks;
use super::trees::RecalcRock;

/// `[Urban] Structures=` entry: the footprint of a BuildingType, or `None`
/// when `BuildingTypeClass::FindIndex @ 0x0045E7B0` finds no such type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrbanStructure {
    pub name: String,
    /// NW-anchor-relative foundation cells (`GetFoundationData @ 0x0045EC20`
    /// returns Type+0xDFC whatever its argument).
    pub footprint: Option<Vec<(i16, i16)>>,
}

/// An `[Urban] Infantry=` or `Vehicles=` entry and whether
/// `InfantryTypeClass::Find` / `UnitTypeClass::Find` resolves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrbanFoot {
    pub name: String,
    pub known: bool,
}

/// The resolved `[Urban]` lists of `RMGMD.INI`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UrbanCatalog {
    pub structures: Vec<UrbanStructure>,
    pub infantry: Vec<UrbanFoot>,
    pub vehicles: Vec<UrbanFoot>,
    /// VERA extension: one-cell street furniture (lights, signs, booths) and
    /// the theater's street trees, set along the roads of a filled district
    /// ([`decorate_streets`]). Empty for Ares' lists.
    pub props: Vec<UrbanStructure>,
    pub trees: Vec<String>,
    /// VERA extension: the theater's farm buildings and monuments, for the
    /// Countryside and Landmark Extras (`embellish`). Empty for Ares' lists.
    pub farms: Vec<UrbanStructure>,
    pub landmarks: Vec<UrbanStructure>,
}

/// What the app supplies for an urban run.
pub struct UrbanInputs<'a> {
    pub catalog: &'a UrbanCatalog,
    /// One raw process-Main draw: Ares' foot pick calls
    /// `Randomizer::Global()->Random()` (0x886B88), not the MapGen stream.
    pub main_draw: &'a dyn Fn() -> u32,
}

/// One generated urban object, in placement order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UrbanObject {
    pub name: String,
    pub category: EntityCategory,
    pub cell: (i16, i16),
    /// The `Put` facing: zero for buildings, `0..=7 << 5` for foots.
    pub facing: u8,
}

/// Scalars the urban pass reads.
#[derive(Debug, Clone, Copy)]
pub struct UrbanArgs {
    /// `MapSeed+0x60`, see [`run`].
    pub urban_presence: i32,
    /// The map rect width and height (`MapClass+0xF4/+0xF8`).
    pub map_w: i32,
    pub map_h: i32,
    /// Scratch stride (`0x0089C2DC`).
    pub stride: i32,
    /// VERA extension: an opted-in Buildings value (1..=100) that sets how
    /// much open land the districts cover (see [`fill_plan`]). `None` keeps
    /// the native target from `urban_presence`.
    pub vera_fill: Option<i32>,
}

/// Everything the pass borrows.
pub struct UrbanCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a mut RmgScratch,
    pub ids: &'a TileIds,
    pub blocks: &'a dyn TileBlocks,
    pub rng: &'a mut RmgRng,
    pub gauss: &'a mut Gaussian,
    pub playfield: &'a PlayfieldBounds,
    /// Land types of the last full recalc (before the starts): the building
    /// gate reads the stored Rock verdict, not one for the paved tiles.
    pub recalc_rock: &'a RecalcRock,
    pub inputs: &'a UrbanInputs<'a>,
}

/// The objects placed and their constructor events.
pub struct UrbanOutput<'a> {
    pub objects: &'a mut Vec<UrbanObject>,
    pub trace: &'a mut RmgConstructionTrace,
    /// Entity index of the first urban object in the emitted list.
    pub entity_base: usize,
    /// VERA extension: street trees as `(name, x, y)` terrain objects.
    pub trees: &'a mut Vec<(String, i16, i16)>,
}

const NORTH: usize = 0;
const EAST: usize = 2;
const SOUTH: usize = 4;
/// Seed picks a pass makes before taking the `(0, 0)` cell.
const MAX_SEED_PICKS: i32 = 200;
/// Seed scans before the special-terrain test is skipped (`local_40`).
const MAX_SEED_SCANS: i32 = 20;
/// Urban attempts per run (`local_3c`).
const MAX_ATTEMPTS: i32 = 10;
/// Placement tries per building or foot.
const MAX_PLACEMENTS: i32 = 60;
/// Building names past this index are used once (`0x005A67E9`).
const REUSABLE_STRUCTURES: usize = 18;
/// VERA fill: the district size range [`fill_plan`] assumes, the share of
/// open land a full Buildings value aims to pave (past 100: pruned edges and
/// gaps leave districts smaller than drawn), the attempts allowed per wanted
/// district, and the bounding-rect slack a district may take (native 1.6), so
/// broken-up land still takes districts. Fill seeds also skip locked cells,
/// which native accepts and then cannot grow from.
const FILL_MIN_AREA: i32 = 300;
const FILL_MAX_AREA: i32 = 1700;
const FILL_COVER_PERCENT: i64 = 120;
const FILL_ATTEMPTS_PER_DISTRICT: i32 = 3;
const FILL_COMPACTNESS: f64 = 2.2;
/// VERA fill: the buildings and props a whole run may place before each
/// district's share is cut down, so a city over the largest sizes keeps its
/// streets but stays playable (thousands more objects slow the match), and
/// the buildings a district of [`FILL_MAX_AREA`] cells takes on average.
const FILL_STRUCTURE_BUDGET: i64 = 2400;
const FILL_STRUCTURES_PER_DISTRICT: i64 = 30;
/// VERA fill street furniture, from authored towns: about one prop to four
/// buildings and one tree a building, each from up to twenty kerb cells.
const BUILDINGS_PER_PROP: i32 = 4;
const TREES_PER_BUILDING: i32 = 1;
const KERB_TRIES: i32 = 20;
/// Plaza sizes in try order (`0x005A78D9..0x005A7964`).
const PLAZAS: [(i32, i32); 5] = [(6, 4), (4, 6), (4, 4), (4, 2), (2, 4)];

fn t(value: f64) -> TruncF64 {
    TruncF64::from_f64(value)
}

fn ftol(value: TruncF64) -> i32 {
    X87Chop53::ftol_f64_low_masked(value.to_f64())
}

fn step(cell: (i32, i32), dir: usize) -> (i32, i32) {
    RmgGrid::step(cell.0, cell.1, dir)
}

/// `0x005A5020`: grow up to `ftol((urban + 0.32) * 3)` urban areas over at
/// most ten attempts.
///
/// RESIDUAL: Ares calls it with ESI as `this`, and at 0x598FB8 ESI is the
/// region the free loop (0x598F4A..0x598FB6) just deleted, a 0x50-byte block
/// whose `+0x60` lies past its end: native Ares reads heap memory there. The
/// port reads `MapSeed+0x60` (UrbanPresence), the receiver Ares' source names.
/// For a presence of 1..=100 the gate below always takes the urban branch;
/// presence 0 leaves the target at zero, so the dirt-road branch
/// (`0x005A89D0`, `0x005A82E0`, `0x005A8720`) is unreachable and not ported.
pub fn run(ctx: &mut UrbanCtx<'_>, args: &UrbanArgs, out: &mut UrbanOutput<'_>) {
    let presence = f64::from(args.urban_presence);
    let (target, max_attempts, share) = match args.vera_fill {
        Some(fill) => fill_plan(ctx, args, fill),
        // 0x005A502E: FILD +0x60; FADD 0.32 [0x007EDAF0]; FMUL 3.0 [0x007E3D90].
        None => (
            ftol(t(presence).add(t(0.32)).mul(t(3.0))),
            MAX_ATTEMPTS,
            FULL_SHARE,
        ),
    };
    let mut successes = 0;
    let mut attempts = 0;
    while successes < target {
        let (seed, scans) = pick_seed(ctx, args.stride, args.vera_fill.is_some());
        // 0x005A51C6: the urban branch runs when 1.0 - presence <= draw * K.
        let draw = t(f64::from(ctx.rng.next_u32())).mul(t(f64::from_bits(RANGE_K_BITS)));
        if draw.lt(t(1.0).sub(t(presence))) {
            return;
        }
        // The twentieth scan skips the area but still counts the attempt.
        if scans != MAX_SEED_SCANS {
            let size = area_size(ctx, args);
            let compact = if args.vera_fill.is_some() {
                FILL_COMPACTNESS
            } else {
                1.6
            };
            if let Some(cells) = grow_area(ctx, seed, size, compact) {
                place_paved_roads(ctx, &cells);
                let buildings = place_structures(ctx, &cells, out, share);
                place_foots(ctx, &cells, out);
                place_plazas(ctx, &cells);
                if args.vera_fill.is_some() {
                    decorate_streets(ctx, &cells, out, buildings);
                }
                successes += 1;
            }
        }
        attempts += 1;
        if attempts >= max_attempts {
            return;
        }
    }
}

/// VERA extension: the districts and attempts for an opted-in Buildings
/// value. One district at 1; at 100, enough districts of the mean size
/// [`area_size`] draws to pave most of the open land, the clear cells the
/// earlier phases left unlocked (start bases, ore fields and tech buildings
/// stay locked). Values between scale linearly.
///
/// The third value is each district's share of its buildings, in thousandths:
/// under [`FULL_SHARE`] once the districts wanted would together pass
/// [`FILL_STRUCTURE_BUDGET`].
fn fill_plan(ctx: &mut UrbanCtx<'_>, args: &UrbanArgs, fill: i32) -> (i32, i32, i32) {
    let mut open = 0i64;
    for (x, y) in ctx.grid.native_cells() {
        let cell = *ctx.grid.cell_native(x, y);
        if ctx.ids.is_clear(cell.tile) && cell.overlay == -1 && !ctx.scratch.get(x, y).water_lock {
            open += 1;
        }
    }
    let district =
        i64::from((args.map_w * args.map_h * 2 / 30).clamp(FILL_MIN_AREA, FILL_MAX_AREA));
    let full = (open * FILL_COVER_PERCENT / 100 / district).max(1);
    let target = 1 + (i64::from(fill.clamp(1, 100)) - 1) * (full - 1) / 99;
    // Buildings and their props (one to four) the districts would take.
    let expected = target * FILL_STRUCTURES_PER_DISTRICT * district / i64::from(FILL_MAX_AREA)
        * i64::from(BUILDINGS_PER_PROP + 1)
        / i64::from(BUILDINGS_PER_PROP);
    let share = if expected > FILL_STRUCTURE_BUDGET {
        (FILL_STRUCTURE_BUDGET * i64::from(FULL_SHARE) / expected) as i32
    } else {
        FULL_SHARE
    };
    let target = target as i32;
    (
        target,
        target * FILL_ATTEMPTS_PER_DISTRICT + MAX_ATTEMPTS,
        share,
    )
}

/// A district's whole building count, in thousandths.
const FULL_SHARE: i32 = 1000;

/// The seed loop of `0x005A5020` (0x005A50B1..0x005A51C2): a clear cell drawn
/// over the whole scratch array, `(0, 0)` after two hundred picks. A
/// water-locked seed with special terrain in its 14x14 square restarts the
/// picks; every pass counts one scan, and from the twentieth on the square
/// is not tested.
fn pick_seed(ctx: &mut UrbanCtx<'_>, stride: i32, fill: bool) -> ((i32, i32), i32) {
    let mut scans = 0;
    let mut picks = 0;
    loop {
        let index = ctx.rng.uniform(0, stride * stride - 1) as usize;
        let record = ctx.scratch.cells()[index];
        if (record.x, record.y) == (0, 0) {
            continue;
        }
        picks += 1;
        let seed = if picks > MAX_SEED_PICKS {
            (0, 0)
        } else {
            let cell = (i32::from(record.x), i32::from(record.y));
            if !ctx.ids.is_clear(ctx.grid.cell_native(cell.0, cell.1).tile)
                || (fill && ctx.scratch.get(cell.0, cell.1).water_lock)
            {
                continue;
            }
            cell
        };
        scans += 1;
        if scans < MAX_SEED_SCANS
            && ctx.scratch.get(seed.0, seed.1).water_lock
            && special_terrain_near(ctx, seed)
        {
            picks = 0;
            continue;
        }
        return (seed, scans);
    }
}

fn special_terrain_near(ctx: &mut UrbanCtx<'_>, (sx, sy): (i32, i32)) -> bool {
    for x in sx - 7..sx + 7 {
        for y in sy - 7..sy + 7 {
            let cell = *ctx.grid.cell_native(x, y);
            if ctx
                .ids
                .cliffs
                .is_special_terrain_tile(cell.tile, cell.sub_tile)
            {
                return true;
            }
        }
    }
    false
}

/// The area size (0x005A5243..0x005A52FB): a Gaussian around a fifteenth of
/// the map rect's area, redrawn outside 100..=2000.
fn area_size(ctx: &mut UrbanCtx<'_>, args: &UrbanArgs) -> i32 {
    let mut mean = f64::from(args.map_w * args.map_h * 2 / 30);
    let mut sigma = 300.0;
    if 2000.0 < mean - 300.0 || mean + 300.0 < 100.0 {
        mean = 1050.0;
        sigma = 950.0;
    }
    loop {
        let value = t(ctx.gauss.next(ctx.rng)).mul(t(sigma)).add(t(mean));
        if value.lt(t(100.0)) || t(2000.0).lt(value) {
            continue;
        }
        return ftol(value);
    }
}

/// `0x005A7640`: the bounding rect `{x, y, w, h}` of the area's cells.
fn bounds(cells: &[(i32, i32)]) -> (i32, i32, i32, i32) {
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (999, 999, 0, 0);
    for &(x, y) in cells.iter().rev() {
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
    }
    (min_x, min_y, max_x - min_x + 1, max_y - min_y + 1)
}

/// `0x005A5360`: pave a jittered-Chebyshev flood of `size` cells from
/// `seed`. More than a hundred steps and a bounding rect under 1.6 cells per
/// cell keep the area: cells next to unpaved ground leave the list, the rest
/// are locked and a third of them get a misc-pave decoration. Otherwise every
/// listed cell returns to the unassigned tile and there is no area.
fn grow_area(
    ctx: &mut UrbanCtx<'_>,
    seed: (i32, i32),
    size: i32,
    compact: f64,
) -> Option<Vec<(i32, i32)>> {
    let mut heap = CellHeap::new(size.max(0) as usize * 10);
    for (x, y) in ctx.grid.native_cells() {
        ctx.scratch.get_mut(x, y).stamp = 0;
    }
    ctx.scratch.get_mut(seed.0, seed.1).stamp = 1;
    heap.push((0.0, seed), by_key);
    let mut node = heap.pop(by_key);
    let mut cells = Vec::new();
    let mut steps = 0;
    if size > 0 {
        while let Some((_, cell)) = node {
            ctx.grid.cell_native_mut(cell.0, cell.1).tile = ctx.ids.pave;
            cells.push(cell);
            for dir in 0..8 {
                let next = step(cell, dir);
                if !ctx.scratch.in_diamond(next.0, next.1) {
                    continue;
                }
                let ground = *ctx.grid.cell_native(next.0, next.1);
                if !ctx.ids.is_clear(ground.tile) || ground.overlay != -1 {
                    continue;
                }
                let record = ctx.scratch.get(next.0, next.1);
                if record.stamp == 1 || record.water_lock {
                    continue;
                }
                let distance = (seed.1 - next.1).abs().max((seed.0 - next.0).abs());
                // 0x005A56C0..0x005A56FA: (draw * 3.0 * K) + distance, FSTP m32.
                let key = x87::store_f32(
                    t(f64::from(ctx.rng.next_u32()))
                        .mul(t(3.0))
                        .mul(t(f64::from_bits(RANGE_K_BITS)))
                        .add(t(f64::from(distance))),
                );
                ctx.scratch.get_mut(next.0, next.1).stamp = 1;
                heap.push((key, next), by_key);
            }
            steps += 1;
            node = heap.pop(by_key);
            if steps >= size {
                break;
            }
        }
    }

    let (_, _, w, h) = bounds(&cells);
    let count = cells.len() as i32;
    if steps > 100 && t(f64::from(w * h)).lt(t(f64::from(count)).mul(t(compact))) {
        prune_edges(ctx, &mut cells);
        for &(x, y) in &cells {
            ctx.scratch.get_mut(x, y).water_lock = true;
        }
        decorate(ctx, &cells);
        return Some(cells);
    }
    for &(x, y) in cells.iter().rev() {
        ctx.grid.cell_native_mut(x, y).tile = TILE_UNASSIGNED;
    }
    None
}

/// 0x005A5859..0x005A5940: from the last cell back, each in-diamond
/// neighbour that is not pave removes the entry at that index — up to eight
/// removals per index, each taking whichever cell has moved into it.
fn prune_edges(ctx: &mut UrbanCtx<'_>, cells: &mut Vec<(i32, i32)>) {
    let mut index = cells.len();
    while index > 0 {
        index -= 1;
        let cell = cells[index];
        for dir in 0..8 {
            let next = step(cell, dir);
            if ctx.scratch.in_diamond(next.0, next.1)
                && ctx.grid.cell_native(next.0, next.1).tile != ctx.ids.pave
                && index < cells.len()
            {
                cells.remove(index);
            }
        }
    }
}

/// 0x005A5975..0x005A5A8D: a third of the cells draw a misc-pave piece;
/// pieces 4..=6 are 2x2 and stamp only over four locked pave cells.
fn decorate(ctx: &mut UrbanCtx<'_>, cells: &[(i32, i32)]) {
    let count = cells.len() as i32;
    for _ in 0..count / 3 {
        let origin = cells[ctx.rng.uniform(0, count - 1) as usize];
        let kind = ctx.rng.uniform(0, 6);
        let mut fits = true;
        if kind >= 4 {
            for dx in 0..2 {
                for dy in 0..2 {
                    let (x, y) = (origin.0 + dx, origin.1 + dy);
                    if !ctx.scratch.get(x, y).water_lock
                        || ctx.grid.cell_native(x, y).tile != ctx.ids.pave
                    {
                        fits = false;
                    }
                }
            }
        }
        if fits {
            let tile = ctx.ids.misc_pave + kind;
            stamp(ctx, tile, origin);
        }
    }
}

fn stamp(ctx: &mut UrbanCtx<'_>, tile: i32, origin: (i32, i32)) {
    stamp_iso_block(ctx.grid, ctx.scratch, ctx.blocks, tile, origin, -1, None);
}

fn paved_clear(ctx: &UrbanCtx<'_>, rect: (i32, i32, i32, i32), roads: bool, ends: bool) -> bool {
    area_admits_paving(ctx.grid, ctx.scratch, ctx.ids, rect, roads, ends)
}

/// `CellClass::Tile_Is_Median @ 0x00486730`: fourteen tiles from `Medians`,
/// with no absent-base guard.
fn is_median(ids: &TileIds, tile: i32) -> bool {
    tile >= ids.medians && tile < ids.medians.wrapping_add(14)
}

/// `0x005A76E0`: a road ahead. The rect's four corners are in the diamond,
/// its first three rows of three are paved road (`PavedRoads + 0..=1`) or
/// median, and the fourth row is pave, clear or misc-pave.
fn road_ahead(ctx: &mut UrbanCtx<'_>, rect: (i32, i32, i32, i32)) -> bool {
    if !corners_in_diamond(ctx.scratch, rect) {
        return false;
    }
    let (x, y, _, _) = rect;
    let ids = ctx.ids;
    for cy in y..y + 3 {
        for cx in x..x + 3 {
            let tile = ctx.grid.cell_native(cx, cy).tile;
            let road = ids.paved_roads <= tile && tile <= ids.paved_roads + 1;
            if !road && !is_median(ids, tile) {
                return false;
            }
        }
    }
    for cx in x..x + 3 {
        let tile = ctx.grid.cell_native(cx, y + 3).tile;
        if !ids.is_pave(tile) && !ids.is_clear(tile) && !ids.is_misc_pave(tile) {
            return false;
        }
    }
    true
}

/// The probe frame a road walk tests around its cell: nine cells across the
/// road, one along it (`0x005A6D80` and `0x005A7070`).
fn road_probe(cell: (i32, i32), dir: usize) -> (i32, i32, i32, i32) {
    if dir == EAST {
        (cell.0, cell.1 - 3, 1, 9)
    } else {
        (cell.0 - 3, cell.1, 9, 1)
    }
}

/// `0x005A6D80`: walk from `from` to the first pave cell inside `rect`, then
/// on to the first cell whose probe is clear (roads and road ends admitted
/// for a south walk), and back off up to `uniform(0, span)` cells while the
/// strict probe stays clear. `(0, 0)` when the walk leaves the rect.
fn road_start(
    ctx: &mut UrbanCtx<'_>,
    from: (i32, i32),
    rect: (i32, i32, i32, i32),
    dir: usize,
) -> (i32, i32) {
    let (rx, ry, rw, rh) = rect;
    let crossing = dir == SOUTH;
    let mut cell = from;
    loop {
        if cell.0 >= rx + rw || cell.1 >= ry + rh {
            return (0, 0);
        }
        if ctx.grid.cell_native(cell.0, cell.1).tile == ctx.ids.pave {
            break;
        }
        cell = step(cell, dir);
    }
    while cell.0 < rx + rw && cell.1 < ry + rh {
        if paved_clear(ctx, road_probe(cell, dir), crossing, crossing) {
            let span = (cell.1 - from.1).max(cell.0 - from.0);
            if span > 0 {
                let mut back = ctx.rng.uniform(0, span);
                while back > 0 {
                    let previous = step(cell, (dir + 4) & 7);
                    if !paved_clear(ctx, road_probe(previous, dir), false, false) {
                        break;
                    }
                    back -= 1;
                    cell = previous;
                }
            }
            return cell;
        }
        cell = step(cell, dir);
    }
    (0, 0)
}

/// `0x005A7070`: walk on from the road start while the probe stays clear
/// (roads and road ends admitted for a south walk), step back one, and on a
/// road longer than six cells step back `uniform(0, 3)` more.
fn road_end(
    ctx: &mut UrbanCtx<'_>,
    from: (i32, i32),
    rect: (i32, i32, i32, i32),
    dir: usize,
) -> (i32, i32) {
    let (rx, ry, rw, rh) = rect;
    let crossing = dir == SOUTH;
    let mut cell = from;
    while cell.0 < rx + rw + 1 && cell.1 < ry + rh + 1 {
        if !paved_clear(ctx, road_probe(cell, dir), crossing, crossing) {
            break;
        }
        cell = step(cell, dir);
    }
    let back = (dir + 4) & 7;
    cell = step(cell, back);
    if (cell.0 - from.0).max(cell.1 - from.1) > 6 {
        for _ in 0..ctx.rng.uniform(0, 3) {
            cell = step(cell, back);
        }
    }
    cell
}

/// `PlacePavedRoads @ 0x005A5B50`: east-west roads every ten rows, then
/// north-south roads every ten columns with crossings and medians.
fn place_paved_roads(ctx: &mut UrbanCtx<'_>, cells: &[(i32, i32)]) {
    let rect = bounds(cells);
    let (x, y, w, h) = rect;
    let (roads, ends) = (ctx.ids.paved_roads, ctx.ids.paved_road_ends);

    if h > 5 {
        let mut cursor = (x, y + 2);
        let mut remaining = h - 2;
        while remaining > 3 {
            let start = road_start(ctx, cursor, rect, EAST);
            let end = road_end(ctx, start, rect, EAST);
            if end == (0, 0) || cursor == (0, 0) || end.0 - cursor.0 < 6 {
                cursor.1 += 2;
                remaining -= 2;
                continue;
            }
            // Ares RoadEndNE (hook 0x5A5C6A -> 0x5A5CC8): default west end.
            stamp(ctx, ends + 2, start);
            for i in 0..end.0 - start.0 - 1 {
                stamp(ctx, roads, (start.0 + i + 1, start.1));
            }
            // Ares RoadEndSW (hook 0x5A5D6F -> 0x5A5DB8): default east end.
            stamp(ctx, ends, end);
            remaining -= 10;
            cursor.1 += 10;
        }
    }

    if w > 8 {
        let mut cursor = (x + 5, y);
        // The native count reads the height here too (0x005A5E10).
        let mut remaining = h - 5;
        while remaining > 3 {
            let start = road_start(ctx, cursor, rect, SOUTH);
            let end = road_end(ctx, start, rect, SOUTH);
            if end == (0, 0) || cursor == (0, 0) || end.1 - cursor.1 < 6 {
                cursor.0 += 2;
                remaining -= 2;
                continue;
            }
            place_north_south_road(ctx, start, end);
            remaining -= 10;
            cursor.0 += 10;
        }
    }
}

/// One north-south road of `0x005A5B50` (0x005A5EA7..0x005A64C9).
fn place_north_south_road(ctx: &mut UrbanCtx<'_>, start: (i32, i32), end: (i32, i32)) {
    let ids = ctx.ids;
    let (roads, ends, medians) = (ids.paved_roads, ids.paved_road_ends, ids.medians);
    let mut cell = start;
    let mut started = false;
    // The median beside the road: open after a crossing, capped at its first
    // following step.
    let mut median = false;
    let mut median_cap = false;
    if cell.1 < end.1 {
        loop {
            if !started {
                if end.1 - cell.1 < 2 {
                    return;
                }
                // Ares RoadEndNW (hook 0x5A5F6A -> 0x5A5FF8) skips the first
                // step's coin arm.
                if road_ahead(ctx, (cell.0, cell.1 - 3, 3, 4)) {
                    stamp(ctx, roads + 5, (cell.0, cell.1 - 3));
                    stamp(ctx, roads + 1, cell);
                    started = true;
                } else if paved_clear(ctx, (cell.0, cell.1, 3, 2), false, false) {
                    stamp(ctx, ends + 1, cell);
                    started = true;
                }
            } else if paved_clear(ctx, (cell.0, cell.1, 3, 1), false, false) {
                stamp(ctx, roads + 1, cell);
                if median {
                    if median_cap {
                        stamp(ctx, medians + 5, (cell.0 + 1, cell.1));
                        median_cap = false;
                    } else {
                        stamp(ctx, medians + 4, (cell.0 + 1, cell.1));
                    }
                }
            } else {
                if median {
                    stamp(ctx, medians + 3, (cell.0 + 1, cell.1 - 1));
                    median = false;
                }
                if !road_ahead(ctx, (cell.0, cell.1, 3, 4)) {
                    stamp(ctx, ends + 3, (cell.0, cell.1 - 1));
                    // The medians this road ran through become road.
                    let mut median_cell = (cell.0 + 1, cell.1 - 2);
                    while is_median(ids, ctx.grid.cell_native(median_cell.0, median_cell.1).tile) {
                        let ground = ctx.grid.cell_native_mut(median_cell.0, median_cell.1);
                        ground.tile = roads + 1;
                        ground.sub_tile = 1;
                        median_cell = step(median_cell, NORTH);
                    }
                    started = false;
                } else {
                    if end.1 - cell.1 < 4 {
                        stamp(ctx, roads + 7, cell);
                        return;
                    }
                    if paved_clear(ctx, (cell.0, cell.1 + 3, 3, 2), false, false)
                        && end.1 - cell.1 > 5
                    {
                        median = true;
                        median_cap = true;
                    }
                    stamp(ctx, roads + 3, cell);
                    if paved_clear(ctx, (cell.0 - 1, cell.1, 1, 3), true, false) {
                        stamp(ctx, medians + 0xC, (cell.0 - 1, cell.1));
                    }
                    if paved_clear(ctx, (cell.0 + 3, cell.1, 1, 3), true, false) {
                        stamp(ctx, medians + 0xC, (cell.0 + 3, cell.1));
                    }
                    cell.1 += 2;
                }
            }
            cell.1 += 1;
            if cell.1 >= end.1 {
                break;
            }
        }
        if median {
            stamp(ctx, medians + 3, (cell.0 + 1, cell.1 - 1));
        }
    }
    // Ares RoadEndSE (hook 0x5A6464 -> 0x5A64AD): default south end.
    stamp(ctx, ends + 3, cell);
}

/// The foundation gate of `0x005A6510`: every cell pave or misc-pave,
/// unoccupied, not stored Rock, in the playfield.
fn foundation_fits(ctx: &mut UrbanCtx<'_>, anchor: (i32, i32), footprint: &[(i16, i16)]) -> bool {
    footprint.iter().all(|&(dx, dy)| {
        let (x, y) = (anchor.0 + i32::from(dx), anchor.1 + i32::from(dy));
        let cell = *ctx.grid.cell_native(x, y);
        (ctx.ids.is_pave(cell.tile) || ctx.ids.is_misc_pave(cell.tile))
            && !cell.occupied
            && !(ctx.grid.is_valid(x, y) && ctx.recalc_rock.get(x, y))
            && ctx
                .playfield
                .contains_height_aware_packed(x, y, cell.level as i8, cell.slope)
    })
}

/// `0x005A6510` under Ares' structure hooks: `uniform(cells/20, cells/10) / 2`
/// neutral buildings, each from up to sixty anchors, until twice that many
/// picks; a list of fewer than two names places none.
///
/// The constructor (a Scenario word) precedes the anchors, so a building
/// that never fits still leaves a discarded event. `Put` admits every gate
/// survivor while Generate holds ScenarioInit (see the tiberium phase).
///
/// VERA extension: `share` (thousandths) cuts the count for a filled map past
/// its budget, never under one building; returns the buildings placed.
fn place_structures(
    ctx: &mut UrbanCtx<'_>,
    cells: &[(i32, i32)],
    out: &mut UrbanOutput<'_>,
    share: i32,
) -> i32 {
    let count = cells.len() as i32;
    let mut target = (ctx.rng.uniform(count / 20, count / 10) as u32 >> 1) as i32;
    if share < FULL_SHARE && target > 0 {
        target = (target * share / FULL_SHARE).max(1);
    }
    let catalog = ctx.inputs.catalog;
    if catalog.structures.is_empty() {
        return 0;
    }
    let mut names: Vec<&UrbanStructure> = catalog.structures.iter().collect();
    let mut placed = 0;
    let mut picks = 0;
    if target == 0 {
        return 0;
    }
    loop {
        if target * 2 <= picks || names.len() < 2 {
            return placed;
        }
        let pick = ctx.rng.uniform(0, names.len() as i32 - 1) as usize;
        let entry = names[pick];
        if let Some(footprint) = &entry.footprint {
            let event = out
                .trace
                .push_discarded(RmgConstructionPhase::UrbanAreas, entry.name.clone());
            for _ in 0..MAX_PLACEMENTS {
                let anchor = cells[ctx.rng.uniform(0, count - 1) as usize];
                if !foundation_fits(ctx, anchor, footprint) {
                    continue;
                }
                for &(dx, dy) in footprint {
                    let (x, y) = (anchor.0 + i32::from(dx), anchor.1 + i32::from(dy));
                    ctx.grid.cell_native_mut(x, y).occupied = true;
                }
                record(
                    out,
                    event,
                    entry.name.clone(),
                    EntityCategory::Structure,
                    anchor,
                    0,
                );
                placed += 1;
                if pick >= REUSABLE_STRUCTURES && pick < names.len() {
                    names.remove(pick);
                }
                break;
            }
        }
        picks += 1;
        if placed >= target {
            return placed;
        }
    }
}

/// VERA extension: street furniture for a filled district of `buildings`
/// buildings. Authored towns set their props beside roads (95% of them in a
/// survey of 2,576 towns) at about one to four buildings, and about a tree a
/// building. Each goes on a free kerb cell: pavement that touches a road.
/// Props are neutral one-cell buildings, traced like the district's others;
/// trees are terrain objects.
fn decorate_streets(
    ctx: &mut UrbanCtx<'_>,
    cells: &[(i32, i32)],
    out: &mut UrbanOutput<'_>,
    buildings: i32,
) {
    let kerb: Vec<(i32, i32)> = cells
        .iter()
        .copied()
        .filter(|&cell| {
            (0..8).step_by(2).any(|dir| {
                let (x, y) = step(cell, dir);
                ctx.scratch.in_diamond(x, y)
                    && ctx.ids.is_paved_road(ctx.grid.cell_native(x, y).tile)
            })
        })
        .collect();
    if kerb.is_empty() {
        return;
    }
    let catalog = ctx.inputs.catalog;
    let single = [(0i16, 0i16)];
    // A free kerb cell, from up to `KERB_TRIES` draws.
    let free_kerb = |ctx: &mut UrbanCtx<'_>| {
        (0..KERB_TRIES).find_map(|_| {
            let cell = kerb[ctx.rng.uniform(0, kerb.len() as i32 - 1) as usize];
            foundation_fits(ctx, cell, &single).then_some(cell)
        })
    };
    if !catalog.props.is_empty() {
        for _ in 0..buildings / BUILDINGS_PER_PROP {
            let prop = &catalog.props[ctx.rng.uniform(0, catalog.props.len() as i32 - 1) as usize];
            let event = out
                .trace
                .push_discarded(RmgConstructionPhase::UrbanAreas, prop.name.clone());
            if let Some(cell) = free_kerb(ctx) {
                ctx.grid.cell_native_mut(cell.0, cell.1).occupied = true;
                record(
                    out,
                    event,
                    prop.name.clone(),
                    EntityCategory::Structure,
                    cell,
                    0,
                );
            }
        }
    }
    if !catalog.trees.is_empty() {
        for _ in 0..buildings * TREES_PER_BUILDING {
            let tree = &catalog.trees[ctx.rng.uniform(0, catalog.trees.len() as i32 - 1) as usize];
            if let Some(cell) = free_kerb(ctx) {
                ctx.grid.cell_native_mut(cell.0, cell.1).occupied = true;
                out.trees.push((tree.clone(), cell.0 as i16, cell.1 as i16));
            }
        }
    }
}

/// `0x005A6920` under Ares' foot hook: `uniform(1, 5)` neutral foots over up
/// to twice that many picks, each from up to sixty cells.
///
/// RESIDUAL: an `[Urban]` name that is no type leaves native Ares with a null
/// object, which it dereferences at its first eligible cell, after the facing
/// draw (a crash). The port spends the same draws and then skips the foot.
fn place_foots(ctx: &mut UrbanCtx<'_>, cells: &[(i32, i32)], out: &mut UrbanOutput<'_>) {
    let quota = ctx.rng.uniform(1, 5);
    let catalog = ctx.inputs.catalog;
    let count = cells.len() as i32;
    // Native would index an emptied list; pruning never empties a real area.
    if count == 0 {
        return;
    }
    let mut placed = 0;
    for _ in 0..quota * 2 {
        let length = catalog.infantry.len() + catalog.vehicles.len();
        if length == 0 {
            return;
        }
        let index = ((ctx.inputs.main_draw)() % length as u32) as usize;
        let (foot, category) = match catalog.infantry.get(index) {
            Some(foot) => (foot, EntityCategory::Infantry),
            None => (
                &catalog.vehicles[index - catalog.infantry.len()],
                EntityCategory::Unit,
            ),
        };
        let event = foot.known.then(|| {
            out.trace
                .push_discarded(RmgConstructionPhase::UrbanAreas, foot.name.clone())
        });
        for _ in 0..MAX_PLACEMENTS {
            let cell = cells[ctx.rng.uniform(0, count - 1) as usize];
            let ground = *ctx.grid.cell_native(cell.0, cell.1);
            if !ctx.playfield.contains_height_aware_packed(
                cell.0,
                cell.1,
                ground.level as i8,
                ground.slope,
            ) || ground.occupied
            {
                continue;
            }
            let facing = (ctx.rng.uniform(0, 7) << 5) as u8;
            if let Some(event) = event {
                ctx.grid.cell_native_mut(cell.0, cell.1).occupied = true;
                record(out, event, foot.name.clone(), category, cell, facing);
                placed += 1;
            }
            break;
        }
        if placed >= quota {
            return;
        }
    }
}

fn record(
    out: &mut UrbanOutput<'_>,
    event: usize,
    name: String,
    category: EntityCategory,
    cell: (i32, i32),
    facing: u8,
) {
    out.trace.mark_emitted(
        event,
        out.entity_base + out.objects.len(),
        (cell.0 as u16, cell.1 as u16),
    );
    out.objects.push(UrbanObject {
        name,
        category,
        cell: (cell.0 as i16, cell.1 as i16),
        facing,
    });
}

/// `0x005A7B40`: a plaza fits where its corners are in the diamond and every
/// cell is pave or `MiscPaveTile + 0..=7` (0x005A7CAC..0x005A7CBA) with
/// nothing on it, so a plaza's own `+8..=13` pieces refuse a second one.
fn plaza_fits(ctx: &mut UrbanCtx<'_>, rect: (i32, i32, i32, i32)) -> bool {
    if !corners_in_diamond(ctx.scratch, rect) {
        return false;
    }
    let (rx, ry, w, h) = rect;
    let misc = ctx.ids.misc_pave;
    for y in ry..ry + h {
        for x in rx..rx + w {
            let cell = *ctx.grid.cell_native(x, y);
            let misc_pave = misc <= cell.tile && cell.tile <= misc + 7;
            if (!ctx.ids.is_pave(cell.tile) && !misc_pave) || cell.occupied {
                return false;
            }
        }
    }
    true
}

/// `0x005A78D0`: `cells / 50` plazas over ten times as many picks, each the
/// first size of [`PLAZAS`] that fits, tiled with one of the six 2x2
/// `MiscPaveTile + 8..=13` pieces.
fn place_plazas(ctx: &mut UrbanCtx<'_>, cells: &[(i32, i32)]) {
    let count = cells.len() as i32;
    let quota = count / 50;
    let mut placed = 0;
    for _ in 0..quota * 10 {
        if placed >= quota {
            return;
        }
        let origin = cells[ctx.rng.uniform(0, count - 1) as usize];
        let mut size = None;
        for (w, h) in PLAZAS {
            if plaza_fits(ctx, (origin.0, origin.1, w, h)) {
                size = Some((w, h));
                break;
            }
        }
        let Some((w, h)) = size else {
            continue;
        };
        let tile = ctx.ids.misc_pave + 8 + ctx.rng.uniform(0, 5);
        for iy in 0..h / 2 {
            for ix in 0..w / 2 {
                stamp(ctx, tile, (origin.0 + 2 * ix, origin.1 + 2 * iy));
            }
        }
        placed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::rmg::phases::shore::{SubTile, TileBlock};
    use crate::map::theater::TheaterCliffRanges;
    use std::cell::Cell;

    const PAVE: i32 = 300;
    const MISC_PAVE: i32 = 320;
    const ROADS: i32 = 340;
    const ROAD_ENDS: i32 = 360;
    const MEDIANS: i32 = 380;

    fn ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: -1,
            ramp_smooth: -1,
            slope_set_pieces: -1,
            rough: -1,
            sand: -1,
            green: -1,
            rough_lat: -1,
            sand_lat: -1,
            green_lat: -1,
            pave_lat: -1,
            pave: PAVE,
            water_base: -1,
            shore: -1,
            water_bridge: -1,
            misc_pave: MISC_PAVE,
            paved_roads: ROADS,
            paved_road_ends: ROAD_ENDS,
            medians: MEDIANS,
            cliffs: TheaterCliffRanges::default(),
        }
    }

    /// Every tile stamps as one flat clear sub-tile.
    struct OneByOne(TileBlock);

    impl TileBlocks for OneByOne {
        fn block(&self, _tile: i32) -> Option<&TileBlock> {
            Some(&self.0)
        }
    }

    fn blocks() -> OneByOne {
        OneByOne(TileBlock {
            width: 1,
            height: 1,
            subtiles: vec![Some(SubTile::default())],
        })
    }

    struct World {
        grid: RmgGrid,
        scratch: RmgScratch,
        args: UrbanArgs,
        playfield: PlayfieldBounds,
    }

    fn world() -> World {
        let (gen_w, gen_h) = (70, 70);
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
            args: UrbanArgs {
                urban_presence: 50,
                map_w,
                map_h,
                stride,
                vera_fill: None,
            },
            playfield: PlayfieldBounds::from_normalized_local_size(map_w, 2, 5, gen_w, gen_h),
        }
    }

    fn catalog() -> UrbanCatalog {
        let foot = |name: &str, known| UrbanFoot {
            name: name.into(),
            known,
        };
        UrbanCatalog {
            structures: vec![
                UrbanStructure {
                    name: "CAHUT".into(),
                    footprint: Some(vec![(0, 0), (1, 0), (0, 1), (1, 1)]),
                },
                UrbanStructure {
                    name: "CANOTYPE".into(),
                    footprint: None,
                },
                UrbanStructure {
                    name: "CABOX".into(),
                    footprint: Some(vec![(0, 0)]),
                },
            ],
            infantry: vec![foot("CIV1", true), foot("CIVBAD", false)],
            vehicles: vec![foot("TRUCKA", true)],
            ..Default::default()
        }
    }

    struct Run {
        objects: Vec<UrbanObject>,
        trace: RmgConstructionTrace,
        main_draws: u32,
        tiles: Vec<i32>,
        next_mapgen: u32,
    }

    fn run_once(seed: u16) -> Run {
        let mut w = world();
        let identity = ids();
        let blocks = blocks();
        let rock = RecalcRock::snapshot(&w.grid, &blocks, 2);
        let catalog = catalog();
        let main_draws = Cell::new(0u32);
        let main_draw = || {
            main_draws.set(main_draws.get() + 1);
            main_draws.get().wrapping_mul(0x9E37_79B9)
        };
        let inputs = UrbanInputs {
            catalog: &catalog,
            main_draw: &main_draw,
        };
        let mut rng = RmgRng::new(seed);
        let mut gauss = Gaussian::default();
        let mut objects = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        {
            let mut ctx = UrbanCtx {
                grid: &mut w.grid,
                scratch: &mut w.scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                gauss: &mut gauss,
                playfield: &w.playfield,
                recalc_rock: &rock,
                inputs: &inputs,
            };
            let mut out = UrbanOutput {
                objects: &mut objects,
                trace: &mut trace,
                entity_base: 7,
                trees: &mut Vec::new(),
            };
            run(&mut ctx, &w.args, &mut out);
        }
        let tiles = w
            .grid
            .native_cells()
            .map(|(x, y)| w.grid.get(x, y).unwrap().tile)
            .collect();
        Run {
            objects,
            trace,
            main_draws: main_draws.get(),
            tiles,
            next_mapgen: rng.next_u32(),
        }
    }

    /// The whole urban pass on open clear ground: deterministic, roads end in
    /// Ares' default pieces (hooks 0x5A5C6A..0x5A6464), unknown `[Urban]`
    /// names construct nothing, and each foot pick spends one Main draw
    /// (hook 0x5A6998).
    #[test]
    fn urban_pass_builds_a_district_with_ares_default_road_ends() {
        let first = run_once(4242);
        let repeat = run_once(4242);
        assert_eq!(first.objects, repeat.objects);
        assert_eq!(first.tiles, repeat.tiles);
        assert_eq!(first.next_mapgen, repeat.next_mapgen);

        let count = |tile: i32| first.tiles.iter().filter(|&&t| t == tile).count();
        assert!(count(PAVE) > 100, "an area was paved");
        assert!(count(ROADS) > 0, "an east-west road was laid");
        assert!(count(ROAD_ENDS) + count(ROAD_ENDS + 2) > 0);
        for stock_coin_piece in [ROADS + 9, ROADS + 0xA, ROADS + 0xC, ROADS + 0xD] {
            assert_eq!(count(stock_coin_piece), 0, "tile {stock_coin_piece}");
        }

        assert!(!first.objects.is_empty());
        let structures = first
            .objects
            .iter()
            .filter(|object| object.category == EntityCategory::Structure)
            .count();
        assert!(structures > 0 && structures < first.objects.len());
        assert!(
            first
                .objects
                .iter()
                .all(|object| !matches!(object.name.as_str(), "CANOTYPE" | "CIVBAD"))
        );
        assert!(
            first
                .trace
                .events
                .iter()
                .all(|event| !matches!(event.techno_type.as_str(), "CANOTYPE" | "CIVBAD"))
        );
        let feet = first.objects.len() - structures;
        assert!(first.main_draws as usize >= feet && first.main_draws > 0);
        let mut emitted = 0;
        for event in &first.trace.events {
            assert_eq!(event.phase, RmgConstructionPhase::UrbanAreas);
            if let crate::map::rmg::RmgConstructionOutcome::Emitted { entity_index, cell } =
                event.outcome
            {
                let object = &first.objects[entity_index - 7];
                assert_eq!(object.name, event.techno_type);
                assert_eq!((object.cell.0 as u16, object.cell.1 as u16), cell);
                emitted += 1;
            }
        }
        assert_eq!(emitted, first.objects.len());
    }

    /// Run `check` over a context on `w` with an empty catalog.
    fn with_ctx<R>(w: &mut World, check: impl FnOnce(&mut UrbanCtx<'_>) -> R) -> R {
        let identity = ids();
        let blocks = blocks();
        let rock = RecalcRock::snapshot(&w.grid, &blocks, 2);
        let catalog = UrbanCatalog::default();
        let main_draw = || 0;
        let inputs = UrbanInputs {
            catalog: &catalog,
            main_draw: &main_draw,
        };
        let mut rng = RmgRng::new(7);
        let mut gauss = Gaussian::default();
        let mut ctx = UrbanCtx {
            grid: &mut w.grid,
            scratch: &mut w.scratch,
            ids: &identity,
            blocks: &blocks,
            rng: &mut rng,
            gauss: &mut gauss,
            playfield: &w.playfield,
            recalc_rock: &rock,
            inputs: &inputs,
        };
        check(&mut ctx)
    }

    /// `0x005A5360`'s failure tail: a flood of a hundred steps or fewer keeps
    /// no area and returns every cell to the unassigned tile.
    #[test]
    fn a_small_area_is_rolled_back_to_unassigned_tiles() {
        let mut w = world();
        with_ctx(&mut w, |ctx| {
            assert_eq!(grow_area(ctx, (80, 80), 100, 1.6), None);
        });
        assert_eq!(w.grid.get(80, 80).unwrap().tile, TILE_UNASSIGNED);
        assert!(
            w.grid
                .native_cells()
                .all(|(x, y)| w.grid.get(x, y).unwrap().tile != PAVE)
        );
    }

    /// `0x005A7B40` (0x005A7CAC..0x005A7CBA) admits `MiscPaveTile + 0..=7`:
    /// a plaza's own `+8..=13` pieces refuse a second plaza.
    #[test]
    fn a_plaza_fits_over_misc_pave_up_to_plus_seven() {
        let fits = |tile: i32| {
            let mut w = world();
            for (x, y) in [(80, 80), (81, 80), (80, 81), (81, 81)] {
                w.grid.get_mut(x, y).unwrap().tile = tile;
            }
            with_ctx(&mut w, |ctx| plaza_fits(ctx, (80, 80, 2, 2)))
        };
        assert!(fits(PAVE));
        assert!(fits(MISC_PAVE + 7));
        assert!(!fits(MISC_PAVE + 8));
    }
}
