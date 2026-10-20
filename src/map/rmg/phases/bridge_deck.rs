//! Ground checks for a low bridge deck.
//!
//! gamemd: `RandomMapGenerator::PlaceLowBridgeDeck` 0x0058F2C0,
//! `ValidateLowBridgeDeckArea` 0x005902C0 and `PlaceBridgeRepairHut`
//! 0x005904B0, reached from `BridgeAndConnectorPass` 0x0058EF10 for active
//! random-map types 3 and 4.
//!
//! The deck placer tries up to two hundred spots. Each attempt starts by
//! rolling a cell out of [`pick_seed_cell`]. Candidate search is RNG-free; only
//! a committed deck may additionally draw the two conditional end-piece
//! coins. The span goes through [`deck_area_is_clear`] and each end through
//! [`end_area_is_placeable`]. A refusal just moves the placer on.
//!
//! **Those two ground checks look like the same check and are not.** They
//! disagree about the
//! swept extent, the order the reference level is read in, whether overlays
//! matter, and which tiles they will accept — see each one's own note. Sharing
//! a sweep between them would judge the wrong cells in both directions.

use crate::map::rmg::grid::RmgGrid;
use crate::map::rmg::phases::carve::{CarveCtx, stamp_iso_block};
use crate::map::rmg::rng::{RANGE_K_BITS, RmgRng};
use crate::map::rmg::scratch::RmgScratch;
use crate::map::rmg::tiles::TileIds;
use crate::map::rmg::x87::TruncF64;
use crate::map::rmg::{RmgConstructionPhase, RmgConstructionTrace};
use crate::util::native_x87::X87Chop53;

use super::area::{area_is_paved_clear, corners_in_diamond, tile_is_placeable};

/// One uniform index over the whole scratch array.
///
/// `rnd * span * K`, in that order, with `K` the original's own
/// `(1 + 2^-32) * 2^-32` rather than a folded-in division — the two round
/// differently at the last bit and are not interchangeable.
///
/// The bound is an **unsigned** compare in the original (`JA`), so this keeps
/// the cast rather than comparing as `i32`.
///
/// Whether the bound is `<=` or `<` cannot be observed, and the reason is worth
/// keeping. Tightening it only changes what happens to the single index
/// `span - 1`, which is the record at `(width-1, width-1)` — the far corner of
/// the square array. That corner is always outside the map diamond: being
/// inside needs `x + y <= diamond_max`, and `2*(width-1)` exceeds `diamond_max`
/// by the diamond's own minimum for every grid the generator builds. So the
/// record is never owned by a region and is always refused a step later. Either
/// spelling therefore spends exactly one draw on that value, and the stream is
/// identical.
fn draw_cell_index(rng: &mut RmgRng, span: i32) -> u32 {
    let scale = TruncF64::from_f64(f64::from_bits(RANGE_K_BITS));
    let span_f = TruncF64::from_f64(f64::from(span));
    loop {
        let value = X87Chop53::ftol_f64_low_masked(
            TruncF64::from_f64(f64::from(rng.next_u32()))
                .mul(span_f)
                .mul(scale)
                .to_f64(),
        ) as u32;
        if value <= (span - 1) as u32 {
            return value;
        }
    }
}

/// Pick the cell a low-bridge attempt starts from.
///
/// **This is the one place in the whole bridge and connector subtree where a
/// rejected draw is really thrown away.** Everywhere else the redraw loops are
/// arithmetic guards that cannot fire, because the generator truncates towards
/// zero and the largest possible draw lands exactly on the bound. Here two of
/// the three rejections are conditions on the map, they fire constantly, and
/// each one costs a fresh draw. Getting the count wrong by one desynchronises
/// every phase that follows.
///
/// The two real rejections:
///
/// - **The cell must belong to this region.** The draw ranges over the entire
///   scratch array, so on any ordinary map the overwhelming majority of draws
///   land outside the region and are spent.
/// - **The record's stored coordinate must not be (0, 0).** Records outside the
///   map diamond never had a coordinate stamped into them, so they still read
///   (0, 0) — that is how the original tells an unused slot apart from a real
///   one. It is not a test for the corner cell.
///
/// Returns `None` only where the original would spin forever, which is when no
/// cell in the scratch array satisfies both conditions. That impossible case
/// is detected without consuming RNG. Once an eligible record exists, the
/// draw/reject loop is deliberately unbounded just like the original.
pub fn pick_seed_cell(rng: &mut RmgRng, scratch: &RmgScratch, region: i32) -> Option<(i32, i32)> {
    let width = scratch.width() as i32;
    let span = width * width;
    pick_seed_cell_from_draws(scratch, region, || draw_cell_index(rng, span) as usize)
}

fn pick_seed_cell_from_draws(
    scratch: &RmgScratch,
    region: i32,
    mut draw_index: impl FnMut() -> usize,
) -> Option<(i32, i32)> {
    if !scratch
        .cells()
        .iter()
        .any(|cell| cell.region == region && (cell.x != 0 || cell.y != 0))
    {
        return None;
    }

    loop {
        let cell = scratch.cells()[draw_index()];
        if cell.region != region {
            continue;
        }
        // Reading this as "not on the top row or left column" would be the
        // natural misreading, and it happens to be harmless: no stamped record
        // can sit on either. A record at x = 0 would need `diamond_min < y` and
        // `y < diamond_min` at once, and the same contradiction holds at
        // y = 0 — so the two spellings cannot be told apart. Kept as the
        // original's AND because the intent is "this slot was never filled in",
        // not a statement about the map edge.
        if cell.x == 0 && cell.y == 0 {
            continue;
        }
        return Some((i32::from(cell.x), i32::from(cell.y)));
    }
}

/// Is this area clear enough to lay a bridge deck across?
///
/// Three things have to hold, and two of them are easy to get subtly wrong:
///
/// - **The swept area is one row and one column LARGER than the deck.** The
///   walk runs inclusive on both axes, so a deck of `w x h` is judged on
///   `(w+1) x (h+1)` cells. That margin is why decks never end up flush
///   against something they should not touch, and shrinking the sweep to the
///   deck itself would let them.
/// - **The tile rule is "clear or water", not "clear or bridge".** The
///   original's second term is the shore/water family test; the name it
///   carries in the disassembly says bridge, and that name is wrong. Porting
///   it from the name would accept cells that already hold a bridge and reject
///   the open water a deck is for.
/// - The reference level is read from the deck's own origin **before** the
///   corner checks, and every swept cell must match it exactly.
///
/// The four corners are checked after that read, so the sweep never runs on an
/// area that hangs off the map.
pub fn deck_area_is_clear(
    grid: &mut RmgGrid,
    scratch: &RmgScratch,
    ids: &TileIds,
    rect: (i32, i32, i32, i32),
) -> bool {
    let (rx, ry, w, h) = rect;

    // Read first, exactly as the original does — the origin's level is the bar
    // every other cell is held to, even if the corner checks then reject.
    let Some(origin) = grid.get(rx, ry) else {
        return false;
    };
    let reference_level = origin.level;

    if !corners_in_diamond(scratch, rect) {
        return false;
    }

    // Inclusive on both axes: one row and one column more than the deck.
    for y in ry..=(ry + h) {
        for x in rx..=(rx + w) {
            // The corner probes cover the deck itself, not the inclusive
            // margin. Preserve MapClass's shared fallback semantics for that
            // one-past row/column rather than strengthening the probes.
            let cell = *grid.cell_native(x, y);
            if cell.overlay != -1 {
                return false;
            }
            if cell.level != reference_level {
                return false;
            }
            // Exact tile-only 0x004865D0 family: 14 water, 42 shore, and four
            // four-tile waterfall bands. It intentionally ignores sub-tile.
            if !ids.is_clear(cell.tile) && !ids.is_water_shore_or_waterfall(cell.tile) {
                return false;
            }
        }
    }
    true
}

const MAX_DECK_ATTEMPTS: i32 = 200;
const CABHUT: &str = "CABHUT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeckAxis {
    EastWest,
    NorthSouth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeckCandidate {
    axis: DeckAxis,
    rect: (i32, i32, i32, i32),
    span: i32,
}

fn exact_region_pair(a: i32, b: i32, first: i32, second: i32) -> bool {
    (a == first && b == second) || (a == second && b == first)
}

fn outer_cells_are_usable(ctx: &CarveCtx<'_>, a: (i32, i32), b: (i32, i32)) -> bool {
    [a, b].into_iter().all(|(x, y)| {
        x >= 0
            && y >= 0
            && ctx.playfield.contains_geometry_packed(x, y)
            && ctx.grid.get(x, y).is_some_and(|cell| {
                !ctx.ids
                    .cliffs
                    .is_special_terrain_tile(cell.tile, cell.sub_tile)
            })
    })
}

/// Build the two orthogonal native candidates around one accepted seed.
/// North/south is computed first, but an equal-span tie selects east/west.
fn find_candidate(
    ctx: &CarveCtx<'_>,
    seed: (i32, i32),
    first_region: i32,
    second_region: i32,
) -> Option<DeckCandidate> {
    let (x, y) = seed;

    let mut north = (x - 1, y, 3, 1);
    let mut south = north;
    let mut ns_ok = true;
    while !area_is_paved_clear(ctx.grid, ctx.scratch, ctx.ids, north) {
        north.1 -= 1;
        if !outer_cells_are_usable(ctx, (north.0, north.1), (north.0 + 2, north.1)) {
            ns_ok = false;
            break;
        }
    }
    if ns_ok && !area_is_paved_clear(ctx.grid, ctx.scratch, ctx.ids, (north.0, north.1 - 3, 3, 3)) {
        ns_ok = false;
    }
    if ns_ok {
        while !area_is_paved_clear(ctx.grid, ctx.scratch, ctx.ids, south) {
            south.1 += 1;
            if !outer_cells_are_usable(ctx, (south.0, south.1), (south.0 + 2, south.1)) {
                ns_ok = false;
                break;
            }
        }
        if ns_ok
            && !area_is_paved_clear(ctx.grid, ctx.scratch, ctx.ids, (south.0, south.1 + 1, 3, 3))
        {
            ns_ok = false;
        }
    }

    let mut west = (x, y - 1, 1, 3);
    let mut east = west;
    let mut ew_ok = true;
    while !area_is_paved_clear(ctx.grid, ctx.scratch, ctx.ids, west) {
        west.0 -= 1;
        if !outer_cells_are_usable(ctx, (west.0, west.1), (west.0, west.1 + 2)) {
            ew_ok = false;
            break;
        }
    }
    if ew_ok && !area_is_paved_clear(ctx.grid, ctx.scratch, ctx.ids, (west.0 - 3, west.1, 3, 3)) {
        ew_ok = false;
    }
    if ew_ok {
        while !area_is_paved_clear(ctx.grid, ctx.scratch, ctx.ids, east) {
            east.0 += 1;
            if !outer_cells_are_usable(ctx, (east.0, east.1), (east.0, east.1 + 2)) {
                ew_ok = false;
                break;
            }
        }
        if ew_ok && !area_is_paved_clear(ctx.grid, ctx.scratch, ctx.ids, (east.0 + 1, east.1, 3, 3))
        {
            ew_ok = false;
        }
    }

    let ns_span = (south.1 - north.1).abs();
    if ns_ok {
        let a = ctx.scratch.get(north.0, north.1).region;
        let b = ctx.scratch.get(south.0, south.1).region;
        ns_ok = exact_region_pair(a, b, first_region, second_region);
    }
    let ew_span = (east.0 - west.0).abs();
    if ew_ok {
        let a = ctx.scratch.get(east.0, east.1).region;
        let b = ctx.scratch.get(west.0, west.1).region;
        ew_ok = exact_region_pair(a, b, first_region, second_region);
    }

    if ns_ok && ew_ok {
        if ns_span < ew_span {
            ew_ok = false;
        } else {
            ns_ok = false;
        }
    }

    if ew_ok {
        Some(DeckCandidate {
            axis: DeckAxis::EastWest,
            rect: (west.0, west.1, ew_span + 1, 3),
            span: ew_span,
        })
    } else if ns_ok {
        Some(DeckCandidate {
            axis: DeckAxis::NorthSouth,
            rect: (north.0, north.1, 3, ns_span + 1),
            span: ns_span,
        })
    } else {
        None
    }
}

fn stamp_deck(ctx: &mut CarveCtx<'_>, candidate: DeckCandidate) {
    let (rx, ry, w, h) = candidate.rect;
    for y in ry..ry + h {
        for x in rx..rx + w {
            let cell = ctx.grid.cell_native_mut(x, y);
            match candidate.axis {
                DeckAxis::EastWest => {
                    cell.overlay = if x == rx {
                        0x5E
                    } else if x == rx + w - 1 {
                        0x5C
                    } else {
                        0x4A + x % 4
                    };
                    cell.density = (y - ry) as u8;
                }
                DeckAxis::NorthSouth => {
                    cell.overlay = if y == ry {
                        0x60
                    } else if y == ry + h - 1 {
                        0x62
                    } else {
                        0x53 + y % 4
                    };
                    cell.density = (x - rx) as u8;
                }
            }
        }
    }
}

fn stamp_end(
    ctx: &mut CarveCtx<'_>,
    validator: (i32, i32, i32, i32),
    alternate: (i32, (i32, i32)),
    default: (i32, (i32, i32)),
) {
    let use_alternate = end_area_is_placeable(ctx.grid, ctx.scratch, ctx.ids, validator)
        && ctx.rng.uniform(0, 1) != 0;
    let (tile, origin) = if use_alternate { alternate } else { default };
    // Native passes scratch id -1 and level base -1 to the unconditional
    // block stamper: every present TMP subcell clears its region owner while
    // preserving the existing cell level.
    stamp_iso_block(ctx.grid, ctx.scratch, ctx.blocks, tile, origin, -1, None);
}

fn place_hut_in_rect(
    ctx: &mut CarveCtx<'_>,
    rect: (i32, i32, i32, i32),
    structures: &mut Vec<(String, i16, i16)>,
    trace: &mut RmgConstructionTrace,
) -> bool {
    let (rx, ry, w, h) = rect;
    for y in ry..=ry + h {
        for x in rx..=rx + w {
            let qualifies = ctx.grid.get(x, y).is_some_and(|cell| {
                cell.overlay == -1 && ctx.ids.is_clear(cell.tile) && !cell.occupied
            });
            if !qualifies {
                continue;
            }
            ctx.grid.cell_native_mut(x, y).occupied = true;
            let entity_index = structures.len();
            structures.push((CABHUT.to_string(), x as i16, y as i16));
            trace.push_emitted(
                RmgConstructionPhase::BridgeRepairHut,
                CABHUT.to_string(),
                entity_index,
                (x as u16, y as u16),
            );
            return true;
        }
    }
    false
}

fn place_hut(
    ctx: &mut CarveCtx<'_>,
    primary: (i32, i32, i32, i32),
    fallback: (i32, i32, i32, i32),
    structures: &mut Vec<(String, i16, i16)>,
    trace: &mut RmgConstructionTrace,
) {
    if !place_hut_in_rect(ctx, primary, structures, trace) {
        let _ = place_hut_in_rect(ctx, fallback, structures, trace);
    }
}

fn commit_candidate(
    ctx: &mut CarveCtx<'_>,
    candidate: DeckCandidate,
    structures: &mut Vec<(String, i16, i16)>,
    trace: &mut RmgConstructionTrace,
) -> bool {
    if !deck_area_is_clear(ctx.grid, ctx.scratch, ctx.ids, candidate.rect) {
        return false;
    }
    stamp_deck(ctx, candidate);
    let (x, y, w, h) = candidate.rect;
    match candidate.axis {
        DeckAxis::EastWest => {
            stamp_end(
                ctx,
                (x + w, y - 2, 6, 6),
                (ctx.ids.paved_roads + 10, (x + w, y)),
                (ctx.ids.paved_road_ends, (x + w, y)),
            );
            stamp_end(
                ctx,
                (x - 6, y - 2, 6, 6),
                (ctx.ids.paved_roads + 9, (x - 4, y)),
                (ctx.ids.paved_road_ends + 2, (x - 1, y)),
            );
            place_hut(
                ctx,
                (x, y - 1, 2, 5),
                (x - 1, y - 2, 3, 7),
                structures,
                trace,
            );
            place_hut(
                ctx,
                (x + w - 2, y - 1, 2, 5),
                (x + w - 2, y - 2, 3, 7),
                structures,
                trace,
            );
        }
        DeckAxis::NorthSouth => {
            stamp_end(
                ctx,
                (x - 2, y - 6, 7, 6),
                (ctx.ids.paved_roads + 13, (x, y - 4)),
                (ctx.ids.paved_road_ends + 1, (x, y - 1)),
            );
            stamp_end(
                ctx,
                (x - 2, y + h, 7, 6),
                (ctx.ids.paved_roads + 12, (x, y + h)),
                (ctx.ids.paved_road_ends + 3, (x, y + h)),
            );
            place_hut(
                ctx,
                (x - 1, y, 5, 2),
                (x - 2, y - 1, 7, 3),
                structures,
                trace,
            );
            place_hut(
                ctx,
                (x - 1, y + h - 2, 5, 2),
                (x - 2, y + h - 2, 7, 3),
                structures,
                trace,
            );
        }
    }
    true
}

/// Active type-3/type-4 low-deck placement. The only MapGen draws in failed
/// attempts are seed-cell rejection draws; a committed candidate additionally
/// takes one coin per end whose exact ground validator succeeds.
pub(crate) fn place_low_bridge_deck(
    ctx: &mut CarveCtx<'_>,
    flood_region: i32,
    first_region: i32,
    second_region: i32,
    structures: &mut Vec<(String, i16, i16)>,
    trace: &mut RmgConstructionTrace,
) -> bool {
    for attempt in 0..MAX_DECK_ATTEMPTS {
        let Some(seed) = pick_seed_cell(ctx.rng, ctx.scratch, flood_region) else {
            return false;
        };
        let Some(candidate) = find_candidate(ctx, seed, first_region, second_region) else {
            continue;
        };
        if !span_allowed(attempt, candidate.span) {
            continue;
        }
        if commit_candidate(ctx, candidate, structures, trace) {
            return true;
        }
    }
    false
}

const fn span_allowed(attempt: i32, span: i32) -> bool {
    span < attempt / 25 + 8
}

/// Can a bridge's end piece anchor on this ground?
///
/// Read as a twin of [`deck_area_is_clear`] and it will mislead you. Every one
/// of these differences flips verdicts on ordinary ground:
///
/// - **This walks exactly `w x h`** — no inclusive margin. The deck validator
///   walks one row and one column further.
/// - **The corners are probed before the reference level is read**, the
///   opposite order. Here that ordering is load-bearing rather than cosmetic:
///   with the probes first, a rect hanging off the map never reaches a cell
///   read at all.
/// - **Overlays are not looked at.** A cell already carrying one can still
///   anchor an end piece, where it would refuse a deck.
/// - **Water refuses.** The deck spans water; its ends have to stand on land.
///
/// The tile rule is the paving rule the start-placement window uses: a paved
/// road or a road-end refuses outright, and what remains must be clear ground,
/// misc-pave or pave.
///
/// The original carries a byte parameter that would make roads and road-ends
/// acceptable instead of refusing. **All four call sites zero it**, so that
/// branch cannot be reached in play and is folded away here rather than
/// modelled as a flag no caller sets.
pub fn end_area_is_placeable(
    grid: &RmgGrid,
    scratch: &RmgScratch,
    ids: &TileIds,
    rect: (i32, i32, i32, i32),
) -> bool {
    let (rx, ry, w, h) = rect;

    if !corners_in_diamond(scratch, rect) {
        return false;
    }

    let reference_level = grid
        .get(rx, ry)
        .expect("validated end origin is in-band")
        .level;

    // Exclusive on both axes: exactly the rect, nothing around it.
    for y in ry..(ry + h) {
        for x in rx..(rx + w) {
            let cell = *grid.get(x, y).expect("validated end cell is in-band");
            if cell.level != reference_level {
                return false;
            }
            if !tile_is_placeable(ids, cell.tile) {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::playfield::PlayfieldBounds;
    use crate::map::rmg::phases::carve_driver::{ConnectorRegion, carve_connectors_for_region};
    use crate::map::rmg::phases::shore::{SubTile, TileBlock, TileBlocks};
    use crate::map::theater::TheaterCliffRanges;

    /// Tile bases the fixtures address families by. Spaced far enough apart
    /// that no span reaches the next.
    const WATER: i32 = 500;
    const GREEN: i32 = 100;
    const PAVE: i32 = 600;
    const MISC_PAVE: i32 = 620;
    const PAVED_ROAD: i32 = 640;
    const PAVED_ROAD_END: i32 = 660;

    fn ids() -> TileIds {
        TileIds {
            clear: 0,
            ramp_base: 200,
            ramp_smooth: 220,
            slope_set_pieces: -1,
            rough: -1,
            sand: -1,
            green: GREEN,
            rough_lat: -1,
            sand_lat: -1,
            green_lat: 110,
            pave_lat: -1,
            pave: PAVE,
            water_base: WATER,
            shore: 400,
            water_bridge: -1,
            misc_pave: MISC_PAVE,
            paved_roads: PAVED_ROAD,
            paved_road_ends: PAVED_ROAD_END,
            medians: -1,
            cliffs: TheaterCliffRanges::default(),
        }
    }

    fn harness() -> (RmgGrid, RmgScratch) {
        let (dmin, dmax) = (34, 34 + 2 * 42);
        let stride = (34 + 42 + 1) as usize;
        let mut grid = RmgGrid::new(stride, dmin, dmax);
        let scratch = RmgScratch::new(stride, dmin, dmax);
        for (x, y) in grid.native_cells().collect::<Vec<_>>() {
            let cell = grid.get_mut(x, y).expect("native cell");
            cell.tile = 0;
            cell.level = 4;
            cell.overlay = -1;
        }
        (grid, scratch)
    }

    struct OneByOne(TileBlock);

    impl OneByOne {
        fn new() -> Self {
            Self(TileBlock {
                width: 1,
                height: 1,
                subtiles: vec![Some(SubTile {
                    height: 9,
                    terrain: 0,
                    slope: 7,
                })],
            })
        }
    }

    impl TileBlocks for OneByOne {
        fn block(&self, _tile: i32) -> Option<&TileBlock> {
            Some(&self.0)
        }
    }

    struct EndMatrixBlock(TileBlock);

    impl EndMatrixBlock {
        fn new() -> Self {
            Self(TileBlock {
                width: 2,
                height: 2,
                subtiles: vec![
                    Some(SubTile {
                        height: 2,
                        terrain: 0,
                        slope: 7,
                    }),
                    None,
                    Some(SubTile {
                        height: 5,
                        terrain: 0,
                        slope: 3,
                    }),
                    Some(SubTile {
                        height: 8,
                        terrain: 0,
                        slope: 6,
                    }),
                ],
            })
        }
    }

    impl TileBlocks for EndMatrixBlock {
        fn block(&self, _tile: i32) -> Option<&TileBlock> {
            Some(&self.0)
        }
    }

    #[derive(Debug, Clone, Copy)]
    enum MatrixEnd {
        East,
        West,
        North,
        South,
    }

    #[derive(Debug, Clone, Copy)]
    enum MatrixPath {
        AreaFalse,
        CoinFalse,
        CoinTrue,
    }

    #[derive(Clone, Copy)]
    struct MatrixEndSpec {
        axis: DeckAxis,
        deck_rect: (i32, i32, i32, i32),
        validator: (i32, i32, i32, i32),
        alternate: (i32, (i32, i32)),
        default: (i32, (i32, i32)),
        coin_index: usize,
    }

    fn matrix_end_spec(end: MatrixEnd) -> MatrixEndSpec {
        match end {
            MatrixEnd::East => MatrixEndSpec {
                axis: DeckAxis::EastWest,
                deck_rect: (40, 48, 6, 3),
                validator: (46, 46, 6, 6),
                alternate: (PAVED_ROAD + 10, (46, 48)),
                default: (PAVED_ROAD_END, (46, 48)),
                coin_index: 0,
            },
            MatrixEnd::West => MatrixEndSpec {
                axis: DeckAxis::EastWest,
                deck_rect: (40, 48, 6, 3),
                validator: (34, 46, 6, 6),
                alternate: (PAVED_ROAD + 9, (36, 48)),
                default: (PAVED_ROAD_END + 2, (39, 48)),
                coin_index: 1,
            },
            MatrixEnd::North => MatrixEndSpec {
                axis: DeckAxis::NorthSouth,
                deck_rect: (40, 48, 3, 6),
                validator: (38, 42, 7, 6),
                alternate: (PAVED_ROAD + 13, (40, 44)),
                default: (PAVED_ROAD_END + 1, (40, 47)),
                coin_index: 0,
            },
            MatrixEnd::South => MatrixEndSpec {
                axis: DeckAxis::NorthSouth,
                deck_rect: (40, 48, 3, 6),
                validator: (38, 54, 7, 6),
                alternate: (PAVED_ROAD + 12, (40, 54)),
                default: (PAVED_ROAD_END + 3, (40, 54)),
                coin_index: 1,
            },
        }
    }

    fn seed_for_ordered_end_coins(target_index: usize, target_value: bool) -> u16 {
        let mut wanted = [false; 2];
        wanted[target_index] = target_value;
        wanted[1 - target_index] = !target_value;
        (0..=u16::MAX)
            .find(|seed| {
                let mut probe = RmgRng::new(*seed);
                [probe.uniform(0, 1) != 0, probe.uniform(0, 1) != 0] == wanted
            })
            .expect("the retail MapGen stream reaches both ordered coin patterns")
    }

    fn prepare_end_footprint(
        grid: &mut RmgGrid,
        scratch: &mut RmgScratch,
        block: &TileBlock,
        origin: (i32, i32),
    ) {
        for row in 0..block.height {
            for col in 0..block.width {
                let (x, y) = (origin.0 + col, origin.1 + row);
                let cell = grid.get_mut(x, y).expect("matrix end footprint");
                cell.tile = 0;
                cell.sub_tile = 201;
                cell.slope = 202;
                cell.level = 4;
                let record = scratch.get_mut(x, y);
                record.region = 17;
                record.stamp = 29;
            }
        }
    }

    fn assert_stamped_end_footprint(
        grid: &RmgGrid,
        scratch: &RmgScratch,
        block: &TileBlock,
        origin: (i32, i32),
        expected_tile: i32,
        label: &str,
    ) {
        for (index, sub) in block.subtiles.iter().copied().enumerate() {
            let x = origin.0 + index as i32 % block.width;
            let y = origin.1 + index as i32 / block.width;
            let cell = grid.get(x, y).expect("stamped matrix end footprint");
            let record = scratch.get(x, y);
            if let Some(sub) = sub {
                assert_eq!(
                    (cell.tile, cell.sub_tile, cell.slope),
                    (expected_tile, index as u8, sub.slope),
                    "{label}: exact tile id, TMP index, and slope at subcell {index}"
                );
                assert_eq!(cell.level, 4, "{label}: level preserved at subcell {index}");
                assert_eq!(
                    (record.region, record.stamp),
                    (-1, 29),
                    "{label}: region clears but independent stamp survives at subcell {index}"
                );
            } else {
                assert_eq!(
                    (cell.tile, cell.sub_tile, cell.slope, cell.level),
                    (0, 201, 202, 4),
                    "{label}: a null TMP subcell stays untouched at index {index}"
                );
                assert_eq!((record.region, record.stamp), (17, 29));
            }
        }
    }

    fn assert_untouched_end_footprint(
        grid: &RmgGrid,
        scratch: &RmgScratch,
        block: &TileBlock,
        origin: (i32, i32),
        label: &str,
    ) {
        for index in 0..block.subtiles.len() {
            let x = origin.0 + index as i32 % block.width;
            let y = origin.1 + index as i32 / block.width;
            let cell = grid.get(x, y).expect("unstamped matrix end footprint");
            assert_eq!(
                (cell.tile, cell.sub_tile, cell.slope, cell.level),
                (0, 201, 202, 4),
                "{label}: unselected anchor stays untouched at subcell {index}"
            );
            assert_eq!(
                (scratch.get(x, y).region, scratch.get(x, y).stamp),
                (17, 29)
            );
        }
    }

    fn assert_bridge_hut_trace(structures: &[(String, i16, i16)], trace: &RmgConstructionTrace) {
        assert_eq!(trace.events.len(), structures.len());
        for (ordinal, (event, structure)) in trace.events.iter().zip(structures.iter()).enumerate()
        {
            assert_eq!(event.ordinal, ordinal);
            assert_eq!(event.phase, RmgConstructionPhase::BridgeRepairHut);
            assert_eq!(event.techno_type, CABHUT);
            assert_eq!(structure.0, CABHUT);
            assert_eq!(
                event.outcome,
                crate::map::rmg::RmgConstructionOutcome::Emitted {
                    entity_index: ordinal,
                    cell: (structure.1 as u16, structure.2 as u16),
                }
            );
        }
    }

    fn playfield() -> PlayfieldBounds {
        PlayfieldBounds::from_normalized_local_size(74, 2, 5, 70, 70)
    }

    fn cross_candidate(horizontal_radius: i32, vertical_radius: i32) -> DeckCandidate {
        let (mut grid, mut scratch) = harness();
        for y in 49..=51 {
            for x in 50 - horizontal_radius..=50 + horizontal_radius {
                grid.get_mut(x, y).unwrap().tile = WATER;
                scratch.get_mut(x, y).region = 0;
            }
        }
        for y in 50 - vertical_radius..=50 + vertical_radius {
            for x in 49..=51 {
                grid.get_mut(x, y).unwrap().tile = WATER;
                scratch.get_mut(x, y).region = 0;
            }
        }
        let west = 50 - horizontal_radius - 1;
        let east = 50 + horizontal_radius + 1;
        let north = 50 - vertical_radius - 1;
        let south = 50 + vertical_radius + 1;
        scratch.get_mut(west, 49).region = 1;
        scratch.get_mut(east, 49).region = 2;
        scratch.get_mut(49, north).region = 1;
        scratch.get_mut(49, south).region = 2;

        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(1);
        let ctx = CarveCtx {
            grid: &mut grid,
            scratch: &mut scratch,
            ids: &identity,
            blocks: &blocks,
            rng: &mut rng,
            playfield: &playfield,
        };
        find_candidate(&ctx, (50, 50), 1, 2).expect("cross has a valid axis")
    }

    /// A rect that sits inside the grid array but outside the map diamond,
    /// painted so that every check except the corner probes would pass.
    fn off_diamond_rect() -> ((i32, i32, i32, i32), RmgGrid, RmgScratch) {
        let rect = (16, 16, 6, 3);
        let (mut grid, scratch) = harness();
        for y in 16..=19 {
            for x in 16..=22 {
                let cell = grid.cell_native_mut(x, y);
                cell.tile = 0;
                cell.level = 4;
                cell.overlay = -1;
            }
        }
        assert!(!scratch.in_diamond(16, 16), "the rect really does hang off");
        (rect, grid, scratch)
    }

    /// A scratch whose records are all unowned except the listed cells, which
    /// must be inside the diamond so that they carry a stamped coordinate.
    fn scratch_owning(cells: &[(i32, i32)], region: i32) -> RmgScratch {
        let (dmin, dmax) = (34, 34 + 2 * 42);
        let width = (34 + 42 + 1) as usize;
        let mut scratch = RmgScratch::new(width, dmin, dmax);
        for cell in scratch.cells_mut() {
            cell.region = -1;
        }
        for &(x, y) in cells {
            assert!(scratch.in_diamond(x, y), "fixture cell ({x},{y}) is inside");
            scratch.get_mut(x, y).region = region;
        }
        scratch
    }

    /// Independently walk the seed acceptance condition while retaining the
    /// exact native index reducer. This deliberately does not call
    /// `pick_seed_cell`: production-entry tests use it as their cursor oracle.
    fn walk_accepted_seeds(
        rng: &mut RmgRng,
        scratch: &RmgScratch,
        region: i32,
        accepted_count: usize,
    ) -> usize {
        let span = (scratch.width() * scratch.width()) as i32;
        let mut rejections = 0;
        for _ in 0..accepted_count {
            loop {
                let cell = scratch.cells()[draw_cell_index(rng, span) as usize];
                if cell.region == region && (cell.x != 0 || cell.y != 0) {
                    break;
                }
                rejections += 1;
            }
        }
        rejections
    }

    fn connector_region(
        id: i32,
        level: u8,
        waterish: bool,
        cell_count: i32,
        neighbours: &[i32],
    ) -> ConnectorRegion {
        ConnectorRegion {
            id,
            level,
            waterish,
            cell_count,
            neighbours: neighbours.to_vec(),
        }
    }

    /// Drive the real flood-region owner into the real low-deck placer on a
    /// clear fixture that cannot reach either endpoint region. Every dispatched
    /// pair therefore exhausts its 200 attempts without mutating the map.
    fn next_word_after_failed_production_driver(
        regions: &[ConnectorRegion],
        flood: &ConnectorRegion,
        seed: u16,
    ) -> (u32, RmgScratch) {
        let (mut grid, mut scratch) = harness();
        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(seed);
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        let placed = {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            carve_connectors_for_region(&mut ctx, regions, flood, 0, &mut structures, &mut trace)
        };

        assert!(!placed);
        assert!(structures.is_empty() && trace.events.is_empty());
        (rng.next_u32(), scratch)
    }

    #[test]
    fn a_record_that_never_got_a_coordinate_is_rejected_not_returned_as_zero_zero() {
        // The out-of-diamond record is given the region id too, so the region
        // test alone cannot refuse it. Only its unstamped (0, 0) coordinate
        // can. Drop that check and this returns (0, 0) about half the time.
        let mut scratch = scratch_owning(&[(40, 48)], 7);
        let unstamped = scratch.get_mut(16, 16);
        assert_eq!((unstamped.x, unstamped.y), (0, 0), "really unstamped");
        unstamped.region = 7;

        let mut rng = RmgRng::new(11);
        for _ in 0..64 {
            assert_eq!(pick_seed_cell(&mut rng, &scratch, 7), Some((40, 48)));
        }
    }

    #[test]
    fn every_rejection_costs_exactly_one_draw() {
        // The load-bearing property of this whole function. Counted against an
        // independent walk of the same stream whose accept condition is spelled
        // out here rather than borrowed from the code under test, so a dropped
        // filter shows up as a stream mismatch and not just a different cell.
        let scratch = scratch_owning(&[(40, 48)], 7);
        let span = (scratch.width() * scratch.width()) as i32;

        let mut probe = RmgRng::new(9);
        let mut rejections = 0;
        loop {
            let cell = scratch.cells()[draw_cell_index(&mut probe, span) as usize];
            if cell.region == 7 && (cell.x != 0 || cell.y != 0) {
                break;
            }
            rejections += 1;
        }
        assert!(
            rejections > 0,
            "the fixture has to reject before it accepts"
        );

        let mut rng = RmgRng::new(9);
        assert_eq!(pick_seed_cell(&mut rng, &scratch, 7), Some((40, 48)));
        assert_eq!(
            rng.next_u32(),
            probe.next_u32(),
            "the picker and the independent walk must leave the stream at the same place"
        );
    }

    #[test]
    fn an_eligible_region_has_no_non_native_draw_cap() {
        const FORMER_SPIN_LIMIT: u32 = 10_000_000;

        let scratch = scratch_owning(&[(40, 48)], 7);
        let rejected_index = 0;
        let accepted_index = 48 * scratch.width() + 40;
        let mut draws = 0_u32;

        let picked = pick_seed_cell_from_draws(&scratch, 7, || {
            draws += 1;
            if draws <= FORMER_SPIN_LIMIT {
                rejected_index
            } else {
                accepted_index
            }
        });

        assert_eq!(picked, Some((40, 48)));
        assert_eq!(draws, FORMER_SPIN_LIMIT + 1);
    }

    #[test]
    fn production_ew_placer_spends_rejections_reaches_attempt_25_and_uses_6x6_ends() {
        let (mut grid, mut scratch) = harness();
        for y in 49..=51 {
            for x in 47..=53 {
                grid.get_mut(x, y).unwrap().tile = WATER;
            }
        }
        for cell in scratch.cells_mut() {
            cell.region = -1;
        }
        scratch.get_mut(50, 50).region = 0;
        scratch.get_mut(46, 49).region = 1;
        scratch.get_mut(54, 49).region = 2;

        // Candidate rect is (46,49,9,3). The east validator is exactly
        // (55,47,6,6), so its far corner must suppress that end's coin while
        // remaining outside the deck's inclusive clearance margin.
        grid.get_mut(60, 52).unwrap().tile = GREEN;

        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(9);
        let mut probe = RmgRng::new(9);
        let rejections = walk_accepted_seeds(&mut probe, &scratch, 0, 26);
        let west_alternate = probe.uniform(0, 1) != 0;
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(place_low_bridge_deck(
                &mut ctx,
                0,
                1,
                2,
                &mut structures,
                &mut trace,
            ));
        }

        assert!(
            rejections > 0,
            "the production picker must reject before accepting"
        );
        assert_eq!(grid.get(46, 49).unwrap().overlay, 0x5E);
        assert_eq!(grid.get(54, 49).unwrap().overlay, 0x5C);
        assert_eq!(
            grid.get(55, 49).unwrap().tile,
            PAVED_ROAD_END,
            "blocked east 6x6 validator uses the default without a coin"
        );
        assert_eq!(
            grid.get(if west_alternate { 42 } else { 45 }, 49)
                .unwrap()
                .tile,
            if west_alternate {
                PAVED_ROAD + 9
            } else {
                PAVED_ROAD_END + 2
            }
        );
        assert_eq!(
            rng.next_u32(),
            probe.next_u32(),
            "25 rejected length-band attempts, attempt 25, and one live end coin"
        );
    }

    #[test]
    fn production_ns_placer_selects_ns_and_uses_7x6_ends() {
        let (mut grid, mut scratch) = harness();
        for y in 48..=52 {
            for x in 49..=51 {
                grid.get_mut(x, y).unwrap().tile = WATER;
            }
        }
        for cell in scratch.cells_mut() {
            cell.region = -1;
        }
        scratch.get_mut(50, 50).region = 0;
        scratch.get_mut(49, 47).region = 1;
        scratch.get_mut(49, 53).region = 2;

        // Candidate rect is (49,47,3,7). The north validator is exactly
        // (47,41,7,6); its far corner suppresses only the north coin.
        grid.get_mut(53, 46).unwrap().tile = GREEN;

        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(17);
        let mut probe = RmgRng::new(17);
        let rejections = walk_accepted_seeds(&mut probe, &scratch, 0, 1);
        let south_alternate = probe.uniform(0, 1) != 0;
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(place_low_bridge_deck(
                &mut ctx,
                0,
                1,
                2,
                &mut structures,
                &mut trace,
            ));
        }

        assert!(
            rejections > 0,
            "the production picker must reject before accepting"
        );
        assert_eq!(grid.get(49, 47).unwrap().overlay, 0x60);
        assert_eq!(grid.get(49, 53).unwrap().overlay, 0x62);
        assert_eq!(
            grid.get(49, 46).unwrap().tile,
            PAVED_ROAD_END + 1,
            "blocked north 7x6 validator uses the default without a coin"
        );
        assert_eq!(
            grid.get(49, 54).unwrap().tile,
            if south_alternate {
                PAVED_ROAD + 12
            } else {
                PAVED_ROAD_END + 3
            }
        );
        assert_eq!(
            rng.next_u32(),
            probe.next_u32(),
            "one accepted seed and one live south-end coin"
        );
    }

    #[test]
    fn production_placer_exhausts_exactly_200_attempts_at_span_15() {
        let (mut grid, mut scratch) = harness();
        for y in 49..=51 {
            for x in 44..=57 {
                grid.get_mut(x, y).unwrap().tile = WATER;
            }
        }
        for cell in scratch.cells_mut() {
            cell.region = -1;
        }
        scratch.get_mut(50, 50).region = 0;
        scratch.get_mut(43, 49).region = 1;
        scratch.get_mut(58, 49).region = 2;

        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(29);
        let mut probe = RmgRng::new(29);
        let rejections = walk_accepted_seeds(&mut probe, &scratch, 0, MAX_DECK_ATTEMPTS as usize);
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        let placed = {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            place_low_bridge_deck(&mut ctx, 0, 1, 2, &mut structures, &mut trace)
        };

        assert!(
            !placed,
            "span 15 is still refused at zero-based attempt 199"
        );
        assert!(rejections > 0);
        assert!(structures.is_empty() && trace.events.is_empty());
        assert_eq!(grid.get(43, 49).unwrap().overlay, -1);
        assert_eq!(
            rng.next_u32(),
            probe.next_u32(),
            "the real placer must stop after exactly 200 accepted seed attempts"
        );
    }

    #[test]
    fn production_flood_driver_dispatches_every_pair_once_and_ineligible_pairs_draw_nothing() {
        let flood = connector_region(0, 4, true, 90, &[1, 2, 3]);
        let qualified = vec![
            flood.clone(),
            connector_region(1, 4, false, 90, &[0, 9]),
            connector_region(2, 4, false, 90, &[0, 9]),
            connector_region(3, 4, false, 90, &[0, 9]),
        ];
        let (next, scratch) = next_word_after_failed_production_driver(&qualified, &flood, 41);
        let mut probe = RmgRng::new(41);
        walk_accepted_seeds(&mut probe, &scratch, 0, 3 * MAX_DECK_ATTEMPTS as usize);
        assert_eq!(
            next,
            probe.next_u32(),
            "three qualified unordered pairs must each enter the real 200-attempt placer once"
        );

        let water_first = connector_region(0, 4, true, 90, &[1, 2]);
        let water_first_regions = vec![
            water_first.clone(),
            connector_region(1, 4, true, 90, &[0, 9]),
            connector_region(2, 4, false, 90, &[0, 9]),
        ];
        let (next, scratch) =
            next_word_after_failed_production_driver(&water_first_regions, &water_first, 42);
        let mut probe = RmgRng::new(42);
        walk_accepted_seeds(&mut probe, &scratch, 0, MAX_DECK_ATTEMPTS as usize);
        assert_eq!(
            next,
            probe.next_u32(),
            "water-first/land-second must dispatch the real placer exactly once"
        );

        let ineligible_flood = connector_region(0, 4, true, 90, &[1, 2, 3]);
        let ineligible = vec![
            ineligible_flood.clone(),
            connector_region(1, 4, false, 50, &[0]),
            connector_region(2, 4, true, 90, &[0, 9]),
            connector_region(3, 5, false, 90, &[0, 9]),
        ];
        let (next, _scratch) =
            next_word_after_failed_production_driver(&ineligible, &ineligible_flood, 43);
        let mut unchanged = RmgRng::new(43);
        assert_eq!(
            next,
            unchanged.next_u32(),
            "pair gates run before the placer and spend no MapGen draw"
        );

        let water_second = connector_region(0, 4, true, 90, &[1, 2]);
        let water_second_regions = vec![
            water_second.clone(),
            connector_region(1, 4, false, 90, &[0, 9]),
            connector_region(2, 4, true, 90, &[0, 9]),
        ];
        let (next, _scratch) =
            next_word_after_failed_production_driver(&water_second_regions, &water_second, 44);
        let mut unchanged = RmgRng::new(44);
        assert_eq!(
            next,
            unchanged.next_u32(),
            "land-first/water-second must be rejected before the real placer"
        );
    }

    #[test]
    fn low_end_stamper_writes_multicell_tmp_fields_and_clears_region_only() {
        let (mut grid, mut scratch) = harness();
        let origin = (40, 48);
        let end_tile = PAVED_ROAD_END;
        let blocks = OneByOne(TileBlock {
            width: 3,
            height: 2,
            subtiles: vec![
                Some(SubTile {
                    height: 2,
                    terrain: 0,
                    slope: 7,
                }),
                None,
                Some(SubTile {
                    height: 5,
                    terrain: 0,
                    slope: 3,
                }),
                Some(SubTile {
                    height: 8,
                    terrain: 0,
                    slope: 0,
                }),
                None,
                Some(SubTile {
                    height: 11,
                    terrain: 0,
                    slope: 6,
                }),
            ],
        });
        for row in 0..2 {
            for col in 0..3 {
                let (x, y) = (origin.0 + col, origin.1 + row);
                let cell = grid.get_mut(x, y).expect("fixture footprint");
                cell.tile = 321;
                cell.sub_tile = 201;
                cell.slope = 202;
                cell.level = 13;
                let record = scratch.get_mut(x, y);
                record.region = 17;
                record.stamp = 29;
            }
        }

        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(19);
        let mut expected_rng = RmgRng::new(19);
        {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            // This validator hangs outside the diamond, forcing the default
            // end without a coin while still exercising the production owner.
            stamp_end(
                &mut ctx,
                (16, 16, 6, 6),
                (PAVED_ROAD, origin),
                (end_tile, origin),
            );
        }

        for (index, sub) in blocks.0.subtiles.iter().copied().enumerate() {
            let x = origin.0 + index as i32 % blocks.0.width;
            let y = origin.1 + index as i32 / blocks.0.width;
            let cell = grid.get(x, y).expect("fixture footprint");
            let record = scratch.get(x, y);
            if let Some(sub) = sub {
                assert_eq!((cell.tile, cell.sub_tile), (end_tile, index as u8));
                assert_eq!(cell.slope, sub.slope, "TMP slope at subcell {index}");
                assert_eq!(cell.level, 13, "level is preserved at subcell {index}");
                assert_eq!(record.region, -1, "region clears at subcell {index}");
                assert_eq!(record.stamp, 29, "stamp survives at subcell {index}");
            } else {
                assert_eq!(
                    (cell.tile, cell.sub_tile, cell.slope, cell.level),
                    (321, 201, 202, 13),
                    "TMP hole stays untouched at subcell {index}"
                );
                assert_eq!((record.region, record.stamp), (17, 29));
            }
        }
        assert_eq!(
            rng.next_u32(),
            expected_rng.next_u32(),
            "a failed end validator spends no coin"
        );
    }

    #[test]
    fn four_end_coin_and_stamp_matrix_matches_native_production_owner() {
        for end in [
            MatrixEnd::East,
            MatrixEnd::West,
            MatrixEnd::North,
            MatrixEnd::South,
        ] {
            for path in [
                MatrixPath::AreaFalse,
                MatrixPath::CoinFalse,
                MatrixPath::CoinTrue,
            ] {
                let spec = matrix_end_spec(end);
                let target_alternate = matches!(path, MatrixPath::CoinTrue);
                let seed = match path {
                    MatrixPath::AreaFalse => match end {
                        MatrixEnd::East => 0xE101,
                        MatrixEnd::West => 0xE102,
                        MatrixEnd::North => 0xE103,
                        MatrixEnd::South => 0xE104,
                    },
                    MatrixPath::CoinFalse | MatrixPath::CoinTrue => {
                        seed_for_ordered_end_coins(spec.coin_index, target_alternate)
                    }
                };
                let label = format!("{end:?}/{path:?}");
                let (mut grid, mut scratch) = harness();
                for (x, y) in grid.native_cells().collect::<Vec<_>>() {
                    grid.get_mut(x, y).expect("native matrix cell").occupied = true;
                }
                let blocks = EndMatrixBlock::new();
                prepare_end_footprint(&mut grid, &mut scratch, &blocks.0, spec.alternate.1);
                if spec.default.1 != spec.alternate.1 {
                    prepare_end_footprint(&mut grid, &mut scratch, &blocks.0, spec.default.1);
                }
                if matches!(path, MatrixPath::AreaFalse) {
                    let (x, y, w, h) = spec.validator;
                    grid.get_mut(x + w - 1, y + h - 1)
                        .expect("exact native end-area far corner")
                        .tile = GREEN;
                }

                let mut rng = RmgRng::new(seed);
                let mut expected_rng = RmgRng::new(seed);
                match path {
                    MatrixPath::AreaFalse => {
                        let _other_end_coin = expected_rng.uniform(0, 1);
                    }
                    MatrixPath::CoinFalse | MatrixPath::CoinTrue => {
                        let ordered_coins = [
                            expected_rng.uniform(0, 1) != 0,
                            expected_rng.uniform(0, 1) != 0,
                        ];
                        assert_eq!(
                            ordered_coins[spec.coin_index], target_alternate,
                            "{label}: target coin fixture"
                        );
                        assert_ne!(
                            ordered_coins[1 - spec.coin_index],
                            target_alternate,
                            "{label}: the opposite coin must distinguish native end order"
                        );
                    }
                }

                let playfield = playfield();
                let identity = ids();
                let mut structures = Vec::new();
                let mut trace = RmgConstructionTrace::default();
                {
                    let mut ctx = CarveCtx {
                        grid: &mut grid,
                        scratch: &mut scratch,
                        ids: &identity,
                        blocks: &blocks,
                        rng: &mut rng,
                        playfield: &playfield,
                    };
                    assert!(
                        commit_candidate(
                            &mut ctx,
                            DeckCandidate {
                                axis: spec.axis,
                                rect: spec.deck_rect,
                                span: 5,
                            },
                            &mut structures,
                            &mut trace,
                        ),
                        "{label}: production candidate commits"
                    );
                }
                assert!(structures.is_empty(), "{label}: occupied hut fixture");
                assert!(trace.events.is_empty(), "{label}: no hut constructors");

                let expected_end = if target_alternate {
                    spec.alternate
                } else {
                    spec.default
                };
                assert_stamped_end_footprint(
                    &grid,
                    &scratch,
                    &blocks.0,
                    expected_end.1,
                    expected_end.0,
                    &label,
                );
                if spec.alternate.1 != spec.default.1 {
                    let unselected_origin = if target_alternate {
                        spec.default.1
                    } else {
                        spec.alternate.1
                    };
                    assert_untouched_end_footprint(
                        &grid,
                        &scratch,
                        &blocks.0,
                        unselected_origin,
                        &label,
                    );
                }
                for continuation_draw in 0..8 {
                    assert_eq!(
                        rng.next_u32(),
                        expected_rng.next_u32(),
                        "{label}: conditional coin cursor at continuation draw {continuation_draw}"
                    );
                }
            }
        }
    }

    #[test]
    fn an_empty_region_gives_up_where_the_original_would_spin_forever() {
        let scratch = scratch_owning(&[], 7);
        let mut rng = RmgRng::new(2);
        let mut expected_rng = RmgRng::new(2);
        assert_eq!(pick_seed_cell(&mut rng, &scratch, 7), None);
        assert_eq!(
            rng.next_u32(),
            expected_rng.next_u32(),
            "the impossible-case pre-scan must not spend a draw"
        );
    }

    #[test]
    fn candidate_search_builds_both_axes_and_east_west_wins_the_tie() {
        let (mut grid, mut scratch) = harness();
        // Equal 7-cell arms make both walks stop eight coordinates apart.
        // Both endpoint pairs name the same two land regions, so only the
        // native equal-length preference can decide the result.
        for y in 49..=51 {
            for x in 47..=53 {
                grid.get_mut(x, y).unwrap().tile = WATER;
                scratch.get_mut(x, y).region = 0;
            }
        }
        for y in 47..=53 {
            for x in 49..=51 {
                grid.get_mut(x, y).unwrap().tile = WATER;
                scratch.get_mut(x, y).region = 0;
            }
        }
        scratch.get_mut(46, 49).region = 1;
        scratch.get_mut(54, 49).region = 2;
        scratch.get_mut(49, 46).region = 1;
        scratch.get_mut(49, 54).region = 2;

        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(1);
        let ctx = CarveCtx {
            grid: &mut grid,
            scratch: &mut scratch,
            ids: &identity,
            blocks: &blocks,
            rng: &mut rng,
            playfield: &playfield,
        };
        assert_eq!(
            find_candidate(&ctx, (50, 50), 1, 2),
            Some(DeckCandidate {
                axis: DeckAxis::EastWest,
                rect: (46, 49, 9, 3),
                span: 8,
            })
        );
    }

    #[test]
    fn candidate_search_selects_the_strictly_shorter_axis() {
        assert_eq!(cross_candidate(2, 4).axis, DeckAxis::EastWest);
        assert_eq!(cross_candidate(4, 2).axis, DeckAxis::NorthSouth);
        assert_eq!(cross_candidate(3, 3).axis, DeckAxis::EastWest);
    }

    #[test]
    fn endpoint_regions_must_be_the_exact_requested_unordered_pair() {
        assert!(exact_region_pair(1, 2, 2, 1));
        assert!(!exact_region_pair(1, 3, 1, 2));
        assert!(!exact_region_pair(1, 1, 1, 2));
    }

    #[test]
    fn candidate_outer_cells_stop_at_special_terrain_or_playfield_edge() {
        let (mut grid, mut scratch) = harness();
        let mut identity = ids();
        identity.cliffs.cliff_set = Some(900);
        grid.get_mut(40, 48).unwrap().tile = 900;
        let blocks = OneByOne::new();
        let playfield = playfield();
        let mut rng = RmgRng::new(1);
        let ctx = CarveCtx {
            grid: &mut grid,
            scratch: &mut scratch,
            ids: &identity,
            blocks: &blocks,
            rng: &mut rng,
            playfield: &playfield,
        };
        assert!(!outer_cells_are_usable(&ctx, (40, 48), (41, 48)));
        assert!(!outer_cells_are_usable(&ctx, (0, 0), (41, 48)));
        assert!(outer_cells_are_usable(&ctx, (41, 48), (42, 48)));
    }

    #[test]
    fn strict_attempt_length_bands_use_the_zero_based_attempt() {
        for (attempt, refused, accepted) in [
            (24, 8, 7),
            (25, 9, 8),
            (49, 9, 8),
            (50, 10, 9),
            (174, 14, 13),
            (175, 15, 14),
            (199, 15, 14),
        ] {
            assert!(!span_allowed(attempt, refused), "attempt {attempt}");
            assert!(span_allowed(attempt, accepted), "attempt {attempt}");
        }
    }

    #[test]
    fn committed_east_west_deck_stamps_overlay_ends_and_two_ordered_huts() {
        let (mut grid, mut scratch) = harness();
        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(123);
        let mut expected_rng = RmgRng::new(123);
        let east_alternate = expected_rng.uniform(0, 1) != 0;
        let west_alternate = expected_rng.uniform(0, 1) != 0;
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(commit_candidate(
                &mut ctx,
                DeckCandidate {
                    axis: DeckAxis::EastWest,
                    rect: (40, 48, 6, 3),
                    span: 5,
                },
                &mut structures,
                &mut trace,
            ));
        }

        for y in 48..51 {
            for x in 40..46 {
                let cell = grid.get(x, y).unwrap();
                let expected_overlay = if x == 40 {
                    0x5E
                } else if x == 45 {
                    0x5C
                } else {
                    0x4A + x % 4
                };
                assert_eq!(
                    (cell.overlay, cell.density),
                    (expected_overlay, (y - 48) as u8)
                );
                assert_eq!((cell.level, cell.tile), (4, 0));
            }
        }
        let east_anchor = (46, 48);
        let west_anchor = if west_alternate { (36, 48) } else { (39, 48) };
        let east_tile = if east_alternate {
            PAVED_ROAD + 10
        } else {
            PAVED_ROAD_END
        };
        let west_tile = if west_alternate {
            PAVED_ROAD + 9
        } else {
            PAVED_ROAD_END + 2
        };
        assert_eq!(
            grid.get(east_anchor.0, east_anchor.1).unwrap().tile,
            east_tile
        );
        assert_eq!(
            grid.get(west_anchor.0, west_anchor.1).unwrap().tile,
            west_tile
        );
        assert_eq!(grid.get(east_anchor.0, east_anchor.1).unwrap().level, 4);
        assert_eq!(scratch.get(east_anchor.0, east_anchor.1).region, -1);
        assert_eq!(scratch.get(east_anchor.0, east_anchor.1).stamp, 0);
        assert_eq!(
            rng.next_u32(),
            expected_rng.next_u32(),
            "exactly two end coins"
        );

        assert_eq!(
            structures,
            vec![(CABHUT.to_string(), 40, 47), (CABHUT.to_string(), 44, 47),]
        );
        assert_eq!(trace.events.len(), 2);
        for (ordinal, event) in trace.events.iter().enumerate() {
            assert_eq!(event.ordinal, ordinal);
            assert_eq!(event.phase, RmgConstructionPhase::BridgeRepairHut);
            assert_eq!(event.techno_type, CABHUT);
            assert_eq!(
                event.outcome,
                crate::map::rmg::RmgConstructionOutcome::Emitted {
                    entity_index: ordinal,
                    cell: (structures[ordinal].1 as u16, structures[ordinal].2 as u16),
                }
            );
        }
    }

    #[test]
    fn committed_north_south_deck_uses_distinct_overlay_and_end_geometry() {
        let (mut grid, mut scratch) = harness();
        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(321);
        let mut expected_rng = RmgRng::new(321);
        let north_alternate = expected_rng.uniform(0, 1) != 0;
        let south_alternate = expected_rng.uniform(0, 1) != 0;
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(commit_candidate(
                &mut ctx,
                DeckCandidate {
                    axis: DeckAxis::NorthSouth,
                    rect: (40, 48, 3, 6),
                    span: 5,
                },
                &mut structures,
                &mut trace,
            ));
        }

        for y in 48..54 {
            for x in 40..43 {
                let cell = grid.get(x, y).unwrap();
                let expected_overlay = if y == 48 {
                    0x60
                } else if y == 53 {
                    0x62
                } else {
                    0x53 + y % 4
                };
                assert_eq!(
                    (cell.overlay, cell.density),
                    (expected_overlay, (x - 40) as u8)
                );
                assert_eq!((cell.level, cell.tile), (4, 0));
            }
        }
        let north_anchor = if north_alternate { (40, 44) } else { (40, 47) };
        let south_anchor = (40, 54);
        let north_tile = if north_alternate {
            PAVED_ROAD + 13
        } else {
            PAVED_ROAD_END + 1
        };
        let south_tile = if south_alternate {
            PAVED_ROAD + 12
        } else {
            PAVED_ROAD_END + 3
        };
        assert_eq!(
            grid.get(north_anchor.0, north_anchor.1).unwrap().tile,
            north_tile
        );
        assert_eq!(
            grid.get(south_anchor.0, south_anchor.1).unwrap().tile,
            south_tile
        );
        assert_eq!(
            rng.next_u32(),
            expected_rng.next_u32(),
            "exactly two end coins"
        );
        assert_eq!(
            structures,
            vec![(CABHUT.to_string(), 39, 48), (CABHUT.to_string(), 39, 52),]
        );
        assert_eq!(trace.events.len(), 2);
    }

    #[test]
    fn a_committed_deck_survives_when_both_hut_searches_fail() {
        let (mut grid, mut scratch) = harness();
        for y in 46..=55 {
            for x in 38..=48 {
                grid.get_mut(x, y).unwrap().occupied = true;
            }
        }
        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(99);
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        let mut ctx = CarveCtx {
            grid: &mut grid,
            scratch: &mut scratch,
            ids: &identity,
            blocks: &blocks,
            rng: &mut rng,
            playfield: &playfield,
        };
        assert!(commit_candidate(
            &mut ctx,
            DeckCandidate {
                axis: DeckAxis::EastWest,
                rect: (40, 48, 6, 3),
                span: 5,
            },
            &mut structures,
            &mut trace,
        ));
        assert!(structures.is_empty());
        assert!(trace.events.is_empty());
        assert_eq!(ctx.grid.get(40, 48).unwrap().overlay, 0x5E);
    }

    #[test]
    fn committed_hut_primary_scan_takes_the_first_later_y_major_cell() {
        let (mut grid, mut scratch) = harness();
        grid.get_mut(40, 47).unwrap().overlay = 1;
        grid.get_mut(41, 47).unwrap().tile = GREEN;
        grid.get_mut(42, 47).unwrap().occupied = true;
        grid.get_mut(40, 51).unwrap().occupied = true;
        grid.get_mut(41, 51).unwrap().occupied = true;
        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(0xC131);
        let mut expected_rng = RmgRng::new(0xC131);
        let _east_end_coin = expected_rng.uniform(0, 1);
        let _west_end_coin = expected_rng.uniform(0, 1);
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(commit_candidate(
                &mut ctx,
                DeckCandidate {
                    axis: DeckAxis::EastWest,
                    rect: (40, 48, 6, 3),
                    span: 5,
                },
                &mut structures,
                &mut trace,
            ));
        }

        assert_eq!(
            structures,
            vec![(CABHUT.to_string(), 42, 51), (CABHUT.to_string(), 44, 47)]
        );
        assert_bridge_hut_trace(&structures, &trace);
        assert!(grid.get(42, 51).unwrap().occupied);
        assert!(grid.get(44, 47).unwrap().occupied);
        for continuation_draw in 0..8 {
            assert_eq!(
                rng.next_u32(),
                expected_rng.next_u32(),
                "hut scans spend no MapGen word at continuation draw {continuation_draw}"
            );
        }
    }

    #[test]
    fn committed_hut_primary_scan_reaches_its_inclusive_maximum_y() {
        let (mut grid, mut scratch) = harness();
        for y in 47..=51 {
            for x in 40..=42 {
                grid.get_mut(x, y).unwrap().occupied = true;
            }
        }
        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(0xC134);
        let mut expected_rng = RmgRng::new(0xC134);
        let _east_end_coin = expected_rng.uniform(0, 1);
        let _west_end_coin = expected_rng.uniform(0, 1);
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(commit_candidate(
                &mut ctx,
                DeckCandidate {
                    axis: DeckAxis::EastWest,
                    rect: (40, 48, 6, 3),
                    span: 5,
                },
                &mut structures,
                &mut trace,
            ));
        }

        assert_eq!(
            structures,
            vec![(CABHUT.to_string(), 40, 52), (CABHUT.to_string(), 44, 47)]
        );
        assert_bridge_hut_trace(&structures, &trace);
        assert!(grid.get(40, 52).unwrap().occupied);
        assert!(grid.get(44, 47).unwrap().occupied);
        for continuation_draw in 0..8 {
            assert_eq!(
                rng.next_u32(),
                expected_rng.next_u32(),
                "inclusive maximum-Y hut scan spends no MapGen word at continuation draw {continuation_draw}"
            );
        }
    }

    #[test]
    fn committed_hut_fallback_takes_its_first_cell_after_primary_failure() {
        let (mut grid, mut scratch) = harness();
        for y in 47..=52 {
            for x in 40..=42 {
                grid.get_mut(x, y).unwrap().occupied = true;
            }
        }
        for y in 46..=53 {
            for x in 44..=47 {
                grid.get_mut(x, y).unwrap().occupied = true;
            }
        }
        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(0xC132);
        let mut expected_rng = RmgRng::new(0xC132);
        let _east_end_coin = expected_rng.uniform(0, 1);
        let _west_end_coin = expected_rng.uniform(0, 1);
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(commit_candidate(
                &mut ctx,
                DeckCandidate {
                    axis: DeckAxis::EastWest,
                    rect: (40, 48, 6, 3),
                    span: 5,
                },
                &mut structures,
                &mut trace,
            ));
        }

        assert_eq!(structures, vec![(CABHUT.to_string(), 39, 46)]);
        assert_bridge_hut_trace(&structures, &trace);
        assert!(grid.get(39, 46).unwrap().occupied);
        for continuation_draw in 0..8 {
            assert_eq!(
                rng.next_u32(),
                expected_rng.next_u32(),
                "fallback search spends no MapGen word at continuation draw {continuation_draw}"
            );
        }
    }

    #[test]
    fn overlapping_committed_hut_searches_skip_the_prior_occupied_cell() {
        assert!(
            span_allowed(0, 2),
            "the overlap fixture is admitted by the first production attempt band"
        );
        let (mut grid, mut scratch) = harness();
        grid.get_mut(40, 47).unwrap().overlay = 1;
        let blocks = OneByOne::new();
        let playfield = playfield();
        let identity = ids();
        let mut rng = RmgRng::new(0xC133);
        let mut expected_rng = RmgRng::new(0xC133);
        let _east_end_coin = expected_rng.uniform(0, 1);
        let _west_end_coin = expected_rng.uniform(0, 1);
        let mut structures = Vec::new();
        let mut trace = RmgConstructionTrace::default();
        {
            let mut ctx = CarveCtx {
                grid: &mut grid,
                scratch: &mut scratch,
                ids: &identity,
                blocks: &blocks,
                rng: &mut rng,
                playfield: &playfield,
            };
            assert!(commit_candidate(
                &mut ctx,
                DeckCandidate {
                    axis: DeckAxis::EastWest,
                    rect: (40, 48, 3, 3),
                    span: 2,
                },
                &mut structures,
                &mut trace,
            ));
        }

        assert_eq!(
            structures,
            vec![(CABHUT.to_string(), 41, 47), (CABHUT.to_string(), 42, 47)]
        );
        assert_bridge_hut_trace(&structures, &trace);
        assert!(grid.get(41, 47).unwrap().occupied);
        assert!(grid.get(42, 47).unwrap().occupied);
        for continuation_draw in 0..8 {
            assert_eq!(
                rng.next_u32(),
                expected_rng.next_u32(),
                "overlapping hut scans spend no MapGen word at continuation draw {continuation_draw}"
            );
        }
    }

    #[test]
    fn the_sweep_reaches_one_past_the_deck_on_both_axes() {
        // The row below and the column right of the deck are still judged. A
        // sweep confined to the deck itself would miss both, which is what the
        // margin exists to prevent.
        for (bx, by, label) in [
            (46, 49, "one column past"),
            (43, 51, "one row past"),
            (46, 51, "the far corner"),
        ] {
            let (mut grid, scratch) = harness();
            grid.get_mut(bx, by).expect("native cell").level = 9;
            assert!(
                !deck_area_is_clear(&mut grid, &scratch, &ids(), (40, 48, 6, 3)),
                "{label} must be swept"
            );
        }
        // And one further out is genuinely outside, so it must NOT refuse —
        // otherwise the test above would pass for a sweep of any size.
        let (mut grid, scratch) = harness();
        grid.get_mut(47, 52).expect("native cell").level = 9;
        assert!(deck_area_is_clear(
            &mut grid,
            &scratch,
            &ids(),
            (40, 48, 6, 3)
        ));
    }

    #[test]
    fn an_existing_overlay_refuses() {
        let (mut grid, scratch) = harness();
        grid.get_mut(42, 49).expect("native cell").overlay = 0x4A;
        assert!(!deck_area_is_clear(
            &mut grid,
            &scratch,
            &ids(),
            (40, 48, 6, 3)
        ));
    }

    #[test]
    fn a_rect_hanging_off_the_map_refuses_at_the_corners() {
        // Every OTHER condition is made to pass over the swept area first, so
        // the corner probes are the only thing left that can refuse. Without
        // that the level check does the work and the probes could be deleted
        // with this test still green.
        let (rect, mut grid, scratch) = off_diamond_rect();
        assert!(!deck_area_is_clear(&mut grid, &scratch, &ids(), rect));
    }

    #[test]
    fn the_end_area_sweep_stops_at_the_rect() {
        // The sharp contrast with the deck validator: one cell past the rect on
        // either axis is NOT judged here. Giving this walk the deck's inclusive
        // margin would refuse end pieces the original places.
        for (bx, by, label) in [
            (46, 49, "one column past"),
            (43, 51, "one row past"),
            (46, 51, "the far corner"),
        ] {
            let (mut grid, scratch) = harness();
            grid.get_mut(bx, by).expect("native cell").level = 9;
            assert!(
                end_area_is_placeable(&grid, &scratch, &ids(), (40, 48, 6, 3)),
                "{label} is outside the end area"
            );
        }
        // The last cell that IS inside, so the pair pins the boundary rather
        // than just proving the sweep is small.
        let (mut grid, scratch) = harness();
        grid.get_mut(45, 50).expect("native cell").level = 9;
        assert!(!end_area_is_placeable(
            &grid,
            &scratch,
            &ids(),
            (40, 48, 6, 3)
        ));
    }

    #[test]
    fn paved_roads_and_road_ends_refuse_an_end_piece() {
        // Two separate tile families, each with its own base — testing one
        // would leave the other free to be dropped.
        for (base, label) in [(PAVED_ROAD, "paved road"), (PAVED_ROAD_END, "road end")] {
            let (mut grid, scratch) = harness();
            grid.get_mut(42, 49).expect("native cell").tile = base;
            assert!(
                !end_area_is_placeable(&grid, &scratch, &ids(), (40, 48, 6, 3)),
                "{label} must refuse"
            );
        }
    }

    #[test]
    fn a_road_tile_refuses_even_when_it_is_also_inside_the_pave_span() {
        // Why the refusal is a separate step and not just an absence from the
        // accept list: every one of these spans is a hardcoded length, not the
        // tileset's real size, so a short set lets the span run on into its
        // neighbour and a tile can land in two of them at once. Where that
        // happens the refusal has to win. With disjoint bases the ordering is
        // invisible, which is exactly why it needs its own fixture.
        let overlapping = TileIds {
            pave: PAVED_ROAD - 2,
            ..ids()
        };
        assert!(
            overlapping.is_pave(PAVED_ROAD) && overlapping.is_paved_road(PAVED_ROAD),
            "the fixture really does put one tile in both spans"
        );
        let (mut grid, scratch) = harness();
        grid.get_mut(42, 49).expect("native cell").tile = PAVED_ROAD;
        assert!(!end_area_is_placeable(
            &grid,
            &scratch,
            &overlapping,
            (40, 48, 6, 3)
        ));
    }

    #[test]
    fn pave_and_misc_pave_still_anchor_an_end_piece() {
        // The accept list is wider than clear ground alone.
        for (base, label) in [(PAVE, "pave"), (MISC_PAVE, "misc pave")] {
            let (mut grid, scratch) = harness();
            grid.get_mut(42, 49).expect("native cell").tile = base;
            assert!(
                end_area_is_placeable(&grid, &scratch, &ids(), (40, 48, 6, 3)),
                "{label} must be accepted"
            );
        }
    }

    #[test]
    fn water_refuses_an_end_piece_even_though_a_deck_spans_it() {
        // Same cell, same tile, opposite verdicts from the two checks. A bridge
        // crosses water; its ends have to stand on land.
        let (mut grid, scratch) = harness();
        grid.get_mut(42, 49).expect("native cell").tile = WATER;
        assert!(!end_area_is_placeable(
            &grid,
            &scratch,
            &ids(),
            (40, 48, 6, 3)
        ));
        let (mut grid, scratch) = harness();
        grid.get_mut(42, 49).expect("native cell").tile = WATER;
        assert!(deck_area_is_clear(
            &mut grid,
            &scratch,
            &ids(),
            (40, 48, 6, 3)
        ));
    }

    #[test]
    fn ordinary_terrain_refuses_an_end_piece() {
        let (mut grid, scratch) = harness();
        grid.get_mut(42, 49).expect("native cell").tile = GREEN;
        assert!(!end_area_is_placeable(
            &grid,
            &scratch,
            &ids(),
            (40, 48, 6, 3)
        ));
    }

    #[test]
    fn an_overlay_does_not_stop_an_end_piece() {
        // The other direction of the same contrast: this check never looks at
        // overlays, so copying the deck validator's rule here would refuse
        // ground the original accepts.
        let (mut grid, scratch) = harness();
        grid.get_mut(42, 49).expect("native cell").overlay = 0x4A;
        assert!(end_area_is_placeable(
            &grid,
            &scratch,
            &ids(),
            (40, 48, 6, 3)
        ));
    }

    #[test]
    fn an_end_area_hanging_off_the_map_refuses_at_the_corners() {
        // Nothing else can refuse: the cells are real, uniform and clear. With
        // the probes gone this returns true, which is what makes the fixture
        // worth having — the corner test is the only thing standing between the
        // placer and an end piece off the edge of the map.
        let (rect, grid, scratch) = off_diamond_rect();
        assert!(!end_area_is_placeable(&grid, &scratch, &ids(), rect));
    }

    #[test]
    #[ignore = "requires RA2_DIR with installed active-retail YR assets"]
    fn active_retail_low_end_tmp_blocks_stamp_native_fields() {
        use std::path::PathBuf;

        use crate::assets::asset_manager::{AssetManager, MediaArchiveMode};
        use crate::map::rmg::theater_blocks::TheaterTileBlocks;

        let retail_dir = std::env::var_os("RA2_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                crate::test_fixture::game_config()
                    .ok()
                    .map(|config| config.paths.ra2_dir)
            })
            .expect("set RA2_DIR to the installed active-retail YR directory");
        let mut assets = AssetManager::new(&retail_dir, MediaArchiveMode::STOCK_DIGITAL)
            .expect("load retail MIX stack");
        let mut saw_multicell = false;
        let mut saw_hole = false;
        let mut saw_nonzero_slope = false;
        let mut corpus = Vec::new();
        for theater_name in ["TEMPERATE", "SNOW", "URBAN", "NEWURBAN", "DESERT"] {
            let theater = crate::map::theater::load_theater(&mut assets, theater_name)
                .unwrap_or_else(|| panic!("load active-retail {theater_name} theater"));
            let identity = TileIds::resolve(&theater);
            let blocks = TheaterTileBlocks::build(&theater.lookup, |name| assets.get(name));
            let end_tiles = [
                identity.paved_roads + 10,
                identity.paved_roads + 9,
                identity.paved_roads + 13,
                identity.paved_roads + 12,
                identity.paved_road_ends,
                identity.paved_road_ends + 2,
                identity.paved_road_ends + 1,
                identity.paved_road_ends + 3,
            ];
            assert!(identity.paved_roads >= 0 && identity.paved_road_ends >= 0);

            for tile in end_tiles {
                let block = blocks
                    .block(tile)
                    .unwrap_or_else(|| {
                        panic!("retail {theater_name} end tile {tile} has a TMP block")
                    })
                    .clone();
                let present = block.subtiles.iter().flatten().count();
                let holes = block.subtiles.len() - present;
                let slopes = block
                    .subtiles
                    .iter()
                    .flatten()
                    .filter(|sub| sub.slope != 0)
                    .count();
                saw_multicell |= block.width * block.height > 1;
                saw_hole |= holes > 0;
                saw_nonzero_slope |= slopes > 0;
                corpus.push((
                    theater_name,
                    tile,
                    block.width,
                    block.height,
                    present,
                    holes,
                    slopes,
                ));

                let (mut grid, mut scratch) = harness();
                let origin = (40, 48);
                for (index, _) in block.subtiles.iter().enumerate() {
                    let x = origin.0 + index as i32 % block.width;
                    let y = origin.1 + index as i32 / block.width;
                    let cell = grid.get_mut(x, y).expect("retail TMP fixture footprint");
                    cell.tile = 321;
                    cell.sub_tile = 201;
                    cell.slope = 202;
                    cell.level = 13 + (index % 5) as u8;
                    let record = scratch.get_mut(x, y);
                    record.region = 17;
                    record.stamp = 29;
                }

                let playfield = playfield();
                let mut rng = RmgRng::new(19);
                {
                    let mut ctx = CarveCtx {
                        grid: &mut grid,
                        scratch: &mut scratch,
                        ids: &identity,
                        blocks: &blocks,
                        rng: &mut rng,
                        playfield: &playfield,
                    };
                    stamp_end(&mut ctx, (16, 16, 6, 6), (tile, origin), (tile, origin));
                }

                for (index, sub) in block.subtiles.iter().copied().enumerate() {
                    let x = origin.0 + index as i32 % block.width;
                    let y = origin.1 + index as i32 / block.width;
                    let cell = grid.get(x, y).expect("retail TMP fixture footprint");
                    let record = scratch.get(x, y);
                    let original_level = 13 + (index % 5) as u8;
                    if let Some(sub) = sub {
                        assert_eq!((cell.tile, cell.sub_tile), (tile, index as u8));
                        assert_eq!(
                            cell.slope, sub.slope,
                            "retail {theater_name} tile {tile} subcell {index}"
                        );
                        assert_eq!(
                            cell.level, original_level,
                            "retail {theater_name} tile {tile} level"
                        );
                        assert_eq!(
                            record.region, -1,
                            "retail {theater_name} tile {tile} region"
                        );
                        assert_eq!(record.stamp, 29, "retail {theater_name} tile {tile} stamp");
                    } else {
                        assert_eq!(
                            (cell.tile, cell.sub_tile, cell.slope, cell.level),
                            (321, 201, 202, original_level),
                            "retail {theater_name} tile {tile} hole {index} stays untouched"
                        );
                        assert_eq!((record.region, record.stamp), (17, 29));
                    }
                }
            }
        }

        assert!(saw_multicell, "retail low-end TMP corpus: {corpus:?}");
        // The complete populated retail end corpus is rectangular and flat.
        // The non-ignored synthetic fixture above pins the same native helper's
        // hole and nonzero-slope behavior without inventing those properties
        // for the stock data.
        assert!(!saw_hole, "retail low-end TMP corpus: {corpus:?}");
        assert!(!saw_nonzero_slope, "retail low-end TMP corpus: {corpus:?}");
    }
}
