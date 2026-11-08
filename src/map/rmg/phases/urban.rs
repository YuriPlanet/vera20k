//! VERA extension: towns of civilian buildings, sized by the Urban Presence
//! (Buildings) option.
//!
//! The original stores `UrbanPresence` in every seed and randomizes it with
//! the map type (`MapSeedClass::RandomizeDerivedFields @ 0x00597260`), but no
//! generation stage reads it. This stage gives it an effect when the player
//! opts in ([`crate::map::rmg::RmgOptions::urban_enabled`]). It runs after
//! the rocks, on its own RNG seeded from the map seed, so the terrain, starts,
//! ore, trees and tech buildings stay exactly what the original would make;
//! the towns only add buildings on cells those left free. A seed without the
//! opt-in never reaches it.

use crate::map::rmg::RmgRng;

use super::tech_buildings::{TechArgs, TechCtx, TechPlacement};
use super::zones::{diamond_frame_contains, recalc_land};

/// Free land cells per town at full presence: a 120x120 interior gets two to
/// three towns, a 500x500 one about forty.
const CELLS_PER_TOWN: i64 = 4000;
/// Mixed into the map seed so the towns' stream is not the terrain's.
const URBAN_SEED_SALT: u16 = 0xB71D;
/// Placement tries per building a town still wants.
const TRIES_PER_BUILDING: i32 = 12;
/// Land a building may stand on: Clear, Road and Rough
/// (`crate::rules::terrain_rules::LandType`). The neutral tech buildings
/// insist on the clear tile itself; towns also take the LAT patches
/// (rough, sand, grass) that cover most open ground.
const BUILDABLE_LAND: [u8; 3] = [0, 1, 7];

/// Place towns for `presence` (1..100) from `ctx.types`, the theater's
/// civilian buildings. Returns the placements in placement order.
pub fn run(ctx: &mut TechCtx<'_>, args: &TechArgs, presence: i32, seed: u16) -> Vec<TechPlacement> {
    let mut placements = Vec::new();
    if presence <= 0 || ctx.types.is_empty() {
        return placements;
    }
    let mut rng = RmgRng::new(seed ^ URBAN_SEED_SALT);

    // Free buildable land, in native scan order so the draws stay stable.
    let candidates: Vec<(i32, i32)> = ctx
        .grid
        .native_cells()
        .filter(|&(x, y)| {
            let level = ctx.grid.get(x, y).map_or(0, |cell| cell.level);
            cell_fits(ctx, args, x, y, level)
        })
        .collect();
    if candidates.is_empty() {
        return placements;
    }
    let towns = (candidates.len() as i64 * i64::from(presence) / 100 / CELLS_PER_TOWN).max(1);
    let per_town = 2 + presence * 6 / 100;
    let radius = 4 + presence / 12;
    let last_type = ctx.types.len() as i32 - 1;
    let last_candidate = candidates.len() as i32 - 1;

    for _ in 0..towns {
        let (cx, cy) = candidates[rng.uniform(0, last_candidate) as usize];
        let mut built = 0;
        for _ in 0..per_town * TRIES_PER_BUILDING {
            if built >= per_town {
                break;
            }
            let type_index = rng.uniform(0, last_type) as usize;
            let anchor = (
                cx + rng.uniform(-radius, radius),
                cy + rng.uniform(-radius, radius),
            );
            if place(ctx, args, type_index, anchor, &mut placements) {
                built += 1;
            }
        }
    }
    placements
}

/// One cell a building may cover: buildable flat land at `level`, free of
/// objects and overlays (ore, rocks), inside the playfield frame and outside
/// every start's protected clearing.
fn cell_fits(ctx: &TechCtx<'_>, args: &TechArgs, x: i32, y: i32, level: u8) -> bool {
    let Some(cell) = ctx.grid.get(x, y) else {
        return false;
    };
    !cell.occupied
        && cell.overlay == -1
        && cell.slope == 0
        && cell.level == level
        && !ctx.scratch.get(x, y).water_lock
        && BUILDABLE_LAND.contains(&recalc_land(
            ctx.grid,
            ctx.blocks,
            args.cliff_back_impassability,
            None,
            x,
            y,
        ))
        && diamond_frame_contains(args.map_w, args.local_rect, x, y, cell.level, false)
}

/// Place `ctx.types[type_index]` with its NW corner at `anchor` when every
/// foundation cell fits and the one-cell ring around it is unoccupied, so
/// buildings keep a lane between them and a town never walls off a path.
fn place(
    ctx: &mut TechCtx<'_>,
    args: &TechArgs,
    type_index: usize,
    anchor: (i32, i32),
    out: &mut Vec<TechPlacement>,
) -> bool {
    let Some(level) = ctx.grid.get(anchor.0, anchor.1).map(|cell| cell.level) else {
        return false;
    };
    let footprint = &ctx.types[type_index].footprint;
    if !footprint.iter().all(|&(dx, dy)| {
        cell_fits(
            ctx,
            args,
            anchor.0 + i32::from(dx),
            anchor.1 + i32::from(dy),
            level,
        )
    }) {
        return false;
    }
    let (w, h) = footprint.iter().fold((0, 0), |(w, h), &(dx, dy)| {
        (w.max(i32::from(dx) + 1), h.max(i32::from(dy) + 1))
    });
    let ring_free = (-1..=h).all(|dy| {
        (-1..=w).all(|dx| {
            ctx.grid
                .get(anchor.0 + dx, anchor.1 + dy)
                .is_none_or(|cell| !cell.occupied)
        })
    });
    if !ring_free {
        return false;
    }
    for &(dx, dy) in footprint {
        if let Some(cell) = ctx
            .grid
            .get_mut(anchor.0 + i32::from(dx), anchor.1 + i32::from(dy))
        {
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
