//! VERA extension: Fair Starts for an opted-in random map
//! ([`RmgOptions::fair_starts`](super::options::RmgOptions)).
//!
//! gamemd's generator spreads the starts as far apart as each region allows
//! and sizes one field per start by distance (`CreateTiberium`, pass 2), but
//! it neither measures nor bounds the result. This module does both, after
//! the last native stage so no native draw moves:
//!
//! - [`equalize_ore`] brings the ore near each start to the median
//!   start's, from its own RNG stream: first the fields beside the base,
//!   then the ring beyond them, where every start is also given an
//!   expansion field.
//! - [`measure`] reports the closest pair of starts, the ore and the flat
//!   open ground near each, which the seed search in
//!   [`build`](super::build) scores.

use std::collections::VecDeque;

use super::grid::RmgGrid;
use super::phases::tiberium::{DENSITY_LEVELS, GEM_BASE, ORE_BASE};
use super::phases::zones::diamond_frame_contains;
use super::rng::RmgRng;
use super::scratch::RmgScratch;
use super::tiles::TileIds;

/// Topped-up ore keeps this far from a start, leaving room for the base.
const BASE_CLEARANCE: i32 = 7;
/// A band of cells around each start whose ore is evened: a cell counts
/// toward its nearest start when its distance lies in `inner..=outer`.
#[derive(Clone, Copy)]
struct Ring {
    inner: i32,
    outer: i32,
    /// A new field begins as near this distance as free ground allows.
    origin: i32,
}
/// The fields a base harvests first.
const HOME: Ring = Ring {
    inner: BASE_CLEARANCE,
    outer: 24,
    origin: BASE_CLEARANCE,
};
/// The ring beyond them, where a base expands to its second field, and the
/// share of the home value every start's ring is brought up to at least.
const EXPANSION_SHARE: i64 = 4;
const EXPANSION: Ring = Ring {
    inner: HOME.outer + 1,
    outer: 44,
    origin: 34,
};
/// Half the side of the square of ground counted as a start's building room.
const ROOM_RADIUS: i32 = 9;
/// Mixed into the map seed for the top-up's own draws.
const FAIR_SALT: u16 = 0xFA12;

/// What a generated map offers each start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FairnessReport {
    /// Squared cell distance between the two closest starts.
    pub closest_starts_sq: i64,
    /// Squared spacing the starts would have spread evenly over the map.
    pub even_spacing_sq: i64,
    /// Least and most ore value near a start (a gem cell counts double).
    pub ore_min: i64,
    pub ore_max: i64,
    /// The same for the expansion ring beyond it.
    pub expansion_min: i64,
    pub expansion_max: i64,
    /// Least and most flat, open cells around a start.
    pub room_min: i64,
    pub room_max: i64,
    /// Start slots the generator could not place.
    pub unfilled_starts: i64,
}

impl FairnessReport {
    /// The four shares the search weighs, each 0..=1000: start spacing
    /// against 60% of an even spread, and the poorest start's ore, expansion
    /// ore and room against the richest's.
    fn shares(&self) -> [i64; 4] {
        let share = |part: i64, whole: i64| {
            if whole <= 0 {
                1000
            } else {
                (part * 1000 / whole).clamp(0, 1000)
            }
        };
        [
            share(self.closest_starts_sq * 100, self.even_spacing_sq * 36),
            share(self.ore_min, self.ore_max),
            share(self.expansion_min, self.expansion_max),
            share(self.room_min, self.room_max),
        ]
    }

    /// Higher is fairer; a map short of starts ranks below every full one.
    pub fn score(&self) -> i64 {
        self.shares().iter().sum::<i64>() - self.unfilled_starts * 1_000_000
    }

    /// Fair enough to stop searching: every start placed, the closest pair at
    /// 60% of an even spread or more, ore and expansion ore within 15% and
    /// room within 40%.
    pub fn is_fair(&self) -> bool {
        let [spacing, ore, expansion, room] = self.shares();
        self.unfilled_starts == 0
            && spacing >= 1000
            && ore >= 850
            && expansion >= 850
            && room >= 600
    }
}

/// The grid and the playfield test the fairness passes share.
pub struct FairCtx<'a> {
    pub grid: &'a mut RmgGrid,
    pub scratch: &'a RmgScratch,
    pub ids: &'a TileIds,
    pub map_w: i32,
    pub local_rect: [i32; 4],
}

impl FairCtx<'_> {
    fn in_play(&self, x: i32, y: i32) -> bool {
        self.grid.get(x, y).is_some_and(|cell| {
            diamond_frame_contains(
                self.map_w,
                self.local_rect,
                x,
                y,
                cell.level,
                cell.slope != 0,
            )
        })
    }

    /// Flat, clear, unclaimed ground: where ore may be added.
    fn takes_ore(&self, x: i32, y: i32) -> bool {
        self.in_play(x, y)
            && self.grid.get(x, y).is_some_and(|cell| {
                self.ids.is_clear(cell.tile)
                    && cell.slope == 0
                    && cell.overlay == -1
                    && !cell.occupied
                    && !cell.start_marker
            })
            && !self.scratch.get(x, y).water_lock
    }
}

fn ore_value(overlay: i32, density: u8) -> i64 {
    let amount = i64::from(density) + 1;
    if (GEM_BASE..GEM_BASE + DENSITY_LEVELS).contains(&overlay) {
        amount * 2
    } else if (ORE_BASE..ORE_BASE + DENSITY_LEVELS).contains(&overlay) {
        amount
    } else {
        0
    }
}

fn dist_sq(a: (i32, i32), b: (i32, i32)) -> i64 {
    let (dx, dy) = (i64::from(a.0 - b.0), i64::from(a.1 - b.1));
    dx * dx + dy * dy
}

/// The start `cell` belongs to in `ring`: the nearest one, when the cell
/// lies in its band.
fn owner(starts: &[(i32, i32)], cell: (i32, i32), ring: Ring) -> Option<usize> {
    let (index, nearest) = starts
        .iter()
        .enumerate()
        .map(|(index, &start)| (index, dist_sq(start, cell)))
        .min_by_key(|&(index, distance)| (distance, index))?;
    (i64::from(ring.inner * ring.inner)..=i64::from(ring.outer * ring.outer))
        .contains(&nearest)
        .then_some(index)
}

/// The cells of the square of `radius` around `centre`, in scan order.
fn square(centre: (i32, i32), radius: i32) -> impl Iterator<Item = (i32, i32)> {
    (centre.1 - radius..=centre.1 + radius)
        .flat_map(move |y| (centre.0 - radius..=centre.0 + radius).map(move |x| (x, y)))
}

/// Ore value in each start's band of `ring`, by start.
fn ore_by_start(ctx: &FairCtx<'_>, starts: &[(i32, i32)], ring: Ring) -> Vec<i64> {
    let mut ore = vec![0i64; starts.len()];
    for (index, &start) in starts.iter().enumerate() {
        for cell in square(start, ring.outer) {
            if owner(starts, cell, ring) != Some(index) || !ctx.in_play(cell.0, cell.1) {
                continue;
            }
            if let Some(ground) = ctx.grid.get(cell.0, cell.1) {
                ore[index] += ore_value(ground.overlay, ground.density);
            }
        }
    }
    ore
}

/// Even the ore around every start: the fields beside the base to the median
/// start's, then the expansion ring to its own median or a quarter of the home
/// value, whichever is more, so every base has a second field to move out to
/// at the same distance. The map's total stays about what the generator
/// placed.
///
/// New cells are as thick as the map's own ore on average, so an added field
/// covers the ground a generated one of its value would.
pub fn equalize_ore(ctx: &mut FairCtx<'_>, starts: &[(i32, i32)], seed: u16) {
    let mut rng = RmgRng::new(seed ^ FAIR_SALT);
    let (mut value, mut cells) = (0i64, 0i64);
    for (x, y) in ctx.grid.native_cells() {
        if let Some(ground) = ctx.grid.get(x, y)
            && (ORE_BASE..ORE_BASE + DENSITY_LEVELS).contains(&ground.overlay)
        {
            value += ore_value(ground.overlay, ground.density);
            cells += 1;
        }
    }
    let thickness = if cells == 0 {
        1
    } else {
        (value / cells).clamp(1, 12)
    };
    let home = equalize_ring(ctx, starts, HOME, 0, thickness, &mut rng);
    equalize_ring(
        ctx,
        starts,
        EXPANSION,
        home / EXPANSION_SHARE,
        thickness,
        &mut rng,
    );
}

/// Bring the ore in every start's band of `ring` to the median start's, or
/// to `floor` when that is more; returns the value aimed for.
///
/// A richer start loses its farthest ore cells. Each poorer start's fields
/// grow outward over free ground, and a new field begins on free ground
/// nearest the ring's origin distance when they are hemmed in or it has none;
/// where no ground is left, its ore cells thicken instead. Changed cells stay
/// in the band and nearer to their start than to any other.
fn equalize_ring(
    ctx: &mut FairCtx<'_>,
    starts: &[(i32, i32)],
    ring: Ring,
    floor: i64,
    thickness: i64,
    rng: &mut RmgRng,
) -> i64 {
    let ore = ore_by_start(ctx, starts, ring);
    let mut sorted = ore.clone();
    sorted.sort_unstable();
    let Some(&median) = sorted.get(sorted.len() / 2) else {
        return 0;
    };
    let target = median.max(floor);
    for (index, &start) in starts.iter().enumerate() {
        let mut missing = target - ore[index];
        if missing == 0 {
            continue;
        }
        let mine = |cell: (i32, i32)| owner(starts, cell, ring) == Some(index);
        // The start's ore cells, nearest first: the fields to grow from.
        let mut fields: Vec<(i32, i32)> = square(start, ring.outer)
            .filter(|&cell| {
                mine(cell)
                    && ctx.in_play(cell.0, cell.1)
                    && ctx
                        .grid
                        .get(cell.0, cell.1)
                        .is_some_and(|ground| ore_value(ground.overlay, ground.density) > 0)
            })
            .collect();
        fields.sort_by_key(|&cell| (dist_sq(start, cell), cell.1, cell.0));
        if missing < 0 {
            // Richer than the median: clear cells from the far edge inward,
            // never going below the target.
            for &cell in fields.iter().rev() {
                let Some(ground) = ctx.grid.get_mut(cell.0, cell.1) else {
                    continue;
                };
                let value = ore_value(ground.overlay, ground.density);
                if missing + value > 0 {
                    continue;
                }
                ground.overlay = -1;
                ground.density = 0;
                missing += value;
                if missing == 0 {
                    break;
                }
            }
            continue;
        }

        // Free ground of the start's own, nearest the ring's origin distance
        // first: where a new field begins once the fields there are have
        // nowhere left to grow.
        let mut ground: Vec<(i32, i32)> = square(start, ring.outer)
            .filter(|&cell| mine(cell) && ctx.takes_ore(cell.0, cell.1))
            .collect();
        let origin_sq = i64::from(ring.origin * ring.origin);
        ground.sort_by_key(|&cell| ((dist_sq(start, cell) - origin_sq).abs(), cell.1, cell.0));
        let mut ground = ground.into_iter();

        let mut queue: VecDeque<(i32, i32)> = fields.iter().copied().collect();
        while missing > 0 {
            let Some(cell) = queue.pop_front() else {
                // Every field is hemmed in: begin another on free ground.
                let Some(first) = ground.find(|&cell| ctx.takes_ore(cell.0, cell.1)) else {
                    break;
                };
                missing -= plant(ctx, first, missing.min(thickness), rng);
                fields.push(first);
                queue.push_back(first);
                continue;
            };
            for dir in 0..8 {
                let next = RmgGrid::step(cell.0, cell.1, dir);
                if missing > 0 && mine(next) && ctx.takes_ore(next.0, next.1) {
                    missing -= plant(ctx, next, missing.min(thickness), rng);
                    fields.push(next);
                    queue.push_back(next);
                }
            }
        }
        // Out of ground: thicken what is there, a level a pass.
        while missing > 0 {
            let mut thickened = false;
            for &cell in &fields {
                if missing <= 0 {
                    break;
                }
                if let Some(ground) = ctx.grid.get_mut(cell.0, cell.1)
                    && ground.density < 11
                {
                    missing -= ore_value(ground.overlay, 0);
                    ground.density += 1;
                    thickened = true;
                }
            }
            if !thickened {
                break;
            }
        }
    }
    target
}

/// A new ore cell of a drawn look, as `PlaceTiberiumField` paints one, worth
/// `amount` (1..=12); returns the value planted.
fn plant(ctx: &mut FairCtx<'_>, cell: (i32, i32), amount: i64, rng: &mut RmgRng) -> i64 {
    let look = rng.uniform(0, DENSITY_LEVELS - 1);
    let amount = amount.clamp(1, 12);
    if let Some(ground) = ctx.grid.get_mut(cell.0, cell.1) {
        ground.overlay = ORE_BASE + look;
        ground.density = (amount - 1) as u8;
    }
    amount
}

/// Measure what the finished grid offers each of `starts`, for a map of
/// `players` starts wanted and `unfilled` of them not placed.
pub fn measure(
    ctx: &FairCtx<'_>,
    starts: &[(i32, i32)],
    players: i32,
    unfilled: usize,
) -> FairnessReport {
    let closest_starts_sq = starts
        .iter()
        .enumerate()
        .flat_map(|(index, &a)| starts[index + 1..].iter().map(move |&b| dist_sq(a, b)))
        .min()
        .unwrap_or(0);
    let cells = ctx
        .grid
        .native_cells()
        .filter(|&(x, y)| ctx.in_play(x, y))
        .count() as i64;
    let ore = ore_by_start(ctx, starts, HOME);
    let expansion = ore_by_start(ctx, starts, EXPANSION);
    let room: Vec<i64> = starts
        .iter()
        .map(|&start| {
            square(start, ROOM_RADIUS)
                .filter(|&(x, y)| {
                    ctx.in_play(x, y)
                        && ctx.grid.get(x, y).is_some_and(|cell| {
                            cell.slope == 0
                                && cell.overlay == -1
                                && (ctx.ids.is_clear(cell.tile) || cell.tile == ctx.ids.pave)
                        })
                })
                .count() as i64
        })
        .collect();
    FairnessReport {
        closest_starts_sq,
        even_spacing_sq: cells / i64::from(players.max(1)),
        ore_min: ore.iter().copied().min().unwrap_or(0),
        ore_max: ore.iter().copied().max().unwrap_or(0),
        expansion_min: expansion.iter().copied().min().unwrap_or(0),
        expansion_max: expansion.iter().copied().max().unwrap_or(0),
        room_min: room.iter().copied().min().unwrap_or(0),
        room_max: room.iter().copied().max().unwrap_or(0),
        unfilled_starts: unfilled as i64,
    }
}
