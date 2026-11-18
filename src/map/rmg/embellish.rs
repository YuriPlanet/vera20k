//! VERA extension: the Extras that add to a finished random map; gamemd's
//! generator has no counterpart.
//!
//! Each runs after the last native stage, from a stream of its own, so no
//! native draw moves and a map with every Extra off is the map it was:
//!
//! - Base Room clears the trees round every start.
//! - Roads pave a track from every start to the middle of the map.
//! - Fair Tech stands one tech building, an oil derrick unless `RMGMD.INI
//!   [Extras] FairTech=` names another, at the same distance from every start.
//! - Landmark stands one monument at the middle of the map.
//! - Centre Gems lays a gem field at the middle of the map.
//! - Countryside scatters farmsteads over open ground, away from the starts.
//! - Fences lines the sides of town buildings with fence.
//!
//! War Damage (`build::war_damage`) and the moods (`settings`) are the two
//! Extras that change no ground.
//!
//! Buildings are neutral, traced like the urban pass's (`0x005A6510`), and
//! follow its objects in the emitted list.

use std::collections::VecDeque;

use super::RmgConstructionTrace;
use super::fairness::FairCtx;
use super::grid::RmgGrid;
use super::phases::tech_buildings::TechType;
use super::phases::tiberium::{DENSITY_LEVELS, GEM_BASE};
use super::phases::urban_areas::{SpawnFoots, UrbanCatalog, UrbanObject, UrbanStructure};
use super::rng::RmgRng;
use crate::map::construction_trace::RmgConstructionPhase;
use crate::map::entities::EntityCategory;

/// Mixed into the map seed for each Extra's own stream.
const LANDMARK_SALT: u16 = 0x1A4D;
const GEM_SALT: u16 = 0x6E35;
const FARM_SALT: u16 = 0xFA43;
const FENCE_SALT: u16 = 0xFE9C;
const TECH_SALT: u16 = 0x7EC1;
const DERELICT_SALT: u16 = 0x5B17;
/// Independent streams for the three Neutral foot stages, so a player moving
/// one slider never shifts the others' picks or any native stage's draws.
const CIVILIAN_SALT: u16 = 0x3A71;
const ANIMAL_SALT: u16 = 0x6C2D;
const WILD_SALT: u16 = 0x51BE;

/// Half the side of the square round a start that Base Room clears of trees:
/// the square Fair Starts counts as a start's building room.
const BASE_ROOM_RADIUS: i32 = 9;
/// A Fair Tech derrick stands this far from its start, give or take
/// [`TECH_REACH`]: past the base, inside the home ore.
const TECH_DISTANCE: i32 = 13;
const TECH_REACH: i32 = 7;
/// Nothing an Extra adds stands nearer a start than this.
const START_CLEARANCE: i32 = 14;
/// How far from the middle of the map a landmark or the gem field may begin.
const CENTRE_REACH: i32 = 60;
/// Cells of the gem field, by the Extra's value from 1.
const GEM_CELLS: [i32; 2] = [70, 160];
/// Play cells to one farmstead, by the Extra's value from 1, the most
/// farmsteads a map takes and the least distance between two.
const CELLS_PER_FARM: [i32; 2] = [3200, 1500];
const MAX_FARMS: i32 = 90;
const FARM_SPACING: i32 = 14;
/// Pavement cells beside a building to one run of town fence, and the cells
/// a run may take.
const KERB_PER_FENCE: i32 = 6;
const FENCE_RUN: (i32, i32) = (3, 6);

/// Which Extras run, each by its value (0 is off), and what they place.
#[derive(Debug, Clone, Copy, Default)]
pub struct EmbellishArgs<'a> {
    pub base_room: u8,
    pub roads: u8,
    pub fair_tech: u8,
    pub landmark: u8,
    pub centre_gems: u8,
    pub countryside: u8,
    pub fences: u8,
    /// Advanced Tech Buildings raises the stock count toward 40.
    pub tech_building_amount: i32,
    /// VERA extension: the Advanced Derelicts slider, 0 = none; stands neutral
    /// unpowered combat buildings over open ground.
    pub derelict_amount: i32,
    /// VERA extension: the Advanced Civilians slider, 0 = none; extra
    /// civilian infantry and vehicles on Neutral over open ground.
    pub civilian_amount: i32,
    /// VERA extension: the Advanced Animals slider, 0 = none; `Name=Animal …`
    /// foots on Neutral over open ground.
    pub animal_amount: i32,
    /// VERA extension: the Advanced Neutral Defenders slider, 0 = none;
    /// player-buildable units left on Neutral over open ground.
    pub wild_amount: i32,
    pub seed: u16,
    /// The neutral tech building Fair Tech stands, by name (`FairTech=`).
    pub fair_tech_type: &'a str,
    /// The overlay ids of the farm and the town fence (`FarmFence=`,
    /// `TownFence=`), where the Rules have them.
    pub farm_fence: Option<i32>,
    pub town_fence: Option<i32>,
}

impl EmbellishArgs<'_> {
    pub fn any(&self) -> bool {
        [
            self.base_room,
            self.roads,
            self.fair_tech,
            self.landmark,
            self.centre_gems,
            self.countryside,
            self.fences,
        ]
        .iter()
        .any(|&value| value != 0)
    }
}

/// Where the Extras' objects go: the urban pass's own lists.
pub struct EmbellishOutput<'a> {
    pub objects: &'a mut Vec<UrbanObject>,
    pub trace: &'a mut RmgConstructionTrace,
    /// Entity index of `objects[0]` in the emitted list.
    pub entity_base: usize,
    pub terrain: &'a mut Vec<(String, i16, i16)>,
}

fn dist_sq(a: (i32, i32), b: (i32, i32)) -> i64 {
    let (dx, dy) = (i64::from(a.0 - b.0), i64::from(a.1 - b.1));
    dx * dx + dy * dy
}

fn clear_of_starts(starts: &[(i32, i32)], cell: (i32, i32), clearance: i32) -> bool {
    let least = i64::from(clearance * clearance);
    starts.iter().all(|&start| dist_sq(start, cell) >= least)
}

/// The cells at Chebyshev distance `radius` from `centre`, in a fixed order.
fn ring(centre: (i32, i32), radius: i32) -> Vec<(i32, i32)> {
    if radius == 0 {
        return vec![centre];
    }
    let mut cells = Vec::with_capacity(radius as usize * 8);
    for step in -radius..=radius {
        cells.push((centre.0 + step, centre.1 - radius));
        cells.push((centre.0 + step, centre.1 + radius));
    }
    for step in -radius + 1..radius {
        cells.push((centre.0 - radius, centre.1 + step));
        cells.push((centre.0 + radius, centre.1 + step));
    }
    cells
}

/// Run the Extras `args` turns on over the finished grid. True when ground
/// was paved, which the caller follows with a LAT pass.
pub fn run(
    ctx: &mut FairCtx<'_>,
    args: &EmbellishArgs<'_>,
    starts: &[(i32, i32)],
    catalog: &UrbanCatalog,
    tech: &[TechType],
    out: &mut EmbellishOutput<'_>,
) -> bool {
    let play: Vec<(i32, i32)> = ctx
        .grid
        .native_cells()
        .filter(|&(x, y)| ctx.in_play(x, y))
        .collect();
    if play.is_empty() {
        return false;
    }
    let (sum_x, sum_y) = play.iter().fold((0i64, 0i64), |(sx, sy), &(x, y)| {
        (sx + i64::from(x), sy + i64::from(y))
    });
    let count = play.len() as i64;
    let centre = ((sum_x / count) as i32, (sum_y / count) as i32);

    if args.base_room != 0 {
        clear_base_trees(ctx, starts, out);
    }
    let paved = args.roads != 0 && pave_roads(ctx, starts, centre);
    if args.fair_tech != 0 {
        fair_tech(ctx, starts, args.fair_tech_type, tech, out);
    }
    if args.landmark != 0 {
        landmark(ctx, args, starts, centre, catalog, out);
    }
    if args.centre_gems != 0 {
        centre_gems(ctx, args, starts, centre);
    }
    if args.countryside != 0 {
        countryside(ctx, args, starts, &play, catalog, out);
    }
    if args.fences != 0 {
        town_fences(ctx, args, &play);
    }
    for fence in [args.farm_fence, args.town_fence].into_iter().flatten() {
        link_fences(ctx, &play, fence);
    }
    paved
}

/// Add the optional population to the finished map. Each slider draws from
/// its own seed. Occupancy is shared so two added objects cannot overlap.
pub(super) fn place_slider_objects(
    ctx: &mut FairCtx<'_>,
    args: &EmbellishArgs<'_>,
    starts: &[(i32, i32)],
    catalog: &UrbanCatalog,
    tech: &[TechType],
    stock_tech: usize,
    out: &mut EmbellishOutput<'_>,
) {
    let tech_target = if args.tech_building_amount > 0 {
        (args.tech_building_amount.min(100) * 40 / 100).max(1)
    } else {
        0
    };
    let additional = (tech_target as usize).saturating_sub(stock_tech);
    if (additional == 0 || tech.is_empty())
        && args.derelict_amount <= 0
        && args.civilian_amount <= 0
        && args.animal_amount <= 0
        && args.wild_amount <= 0
    {
        return;
    }
    let play: Vec<_> = ctx
        .grid
        .native_cells()
        .filter(|&(x, y)| ctx.in_play(x, y))
        .collect();
    if play.is_empty() {
        return;
    }
    if additional > 0 && !tech.is_empty() {
        let mut rng = RmgRng::new(args.seed ^ TECH_SALT);
        for _ in 0..additional {
            let kind = &tech[rng.uniform(0, tech.len() as i32 - 1) as usize];
            if kind.footprint.is_empty() {
                continue;
            }
            // Like the native per-building placer (0x00595400), give each
            // building a finite number of anchors. A full map receives fewer.
            for _ in 0..100 {
                let cell = play[rng.uniform(0, play.len() as i32 - 1) as usize];
                if clear_of_starts(starts, cell, START_CLEARANCE)
                    && site_fits(ctx, cell, &kind.footprint, 0, Also::Nothing)
                {
                    place_building(ctx, out, &kind.name, &kind.footprint, cell);
                    break;
                }
            }
        }
    }
    if args.derelict_amount > 0 {
        derelicts(ctx, args, starts, &play, catalog, out);
    }
    if args.civilian_amount > 0 {
        spawn_foots(
            ctx,
            starts,
            &play,
            &catalog.civilians,
            args.civilian_amount,
            args.seed ^ CIVILIAN_SALT,
            out,
        );
    }
    if args.animal_amount > 0 {
        spawn_foots(
            ctx,
            starts,
            &play,
            &catalog.animals,
            args.animal_amount,
            args.seed ^ ANIMAL_SALT,
            out,
        );
    }
    if args.wild_amount > 0 {
        spawn_foots(
            ctx,
            starts,
            &play,
            &catalog.wild,
            args.wild_amount,
            args.seed ^ WILD_SALT,
            out,
        );
    }
}

/// Flat, unclaimed pavement of a town (the urban pass locks its districts):
/// where a landmark may stand as well as on open ground.
fn town_pavement(ctx: &FairCtx<'_>, x: i32, y: i32) -> bool {
    ctx.in_play(x, y)
        && ctx.scratch.get(x, y).water_lock
        && ctx.grid.get(x, y).is_some_and(|cell| {
            (cell.tile == ctx.ids.pave || ctx.ids.is_misc_pave(cell.tile))
                && cell.slope == 0
                && cell.overlay == -1
                && !cell.occupied
                && !cell.start_marker
        })
}

/// Where a site may lie besides free open ground.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Also {
    Nothing,
    /// The pavement of a town.
    Pavement,
    /// Ground the generator keeps for a base and its ore: flat, clear and
    /// bare all the same.
    Kept,
}

/// Free open ground: flat, clear or a ground patch, with nothing on it and
/// not kept by the generator for a base, an ore field or a town.
fn free_ground(ctx: &FairCtx<'_>, x: i32, y: i32) -> bool {
    ctx.in_play(x, y)
        && !ctx.scratch.get(x, y).water_lock
        && ctx.grid.get(x, y).is_some_and(|cell| {
            ctx.ids.is_open_ground(cell.tile)
                && cell.slope == 0
                && cell.overlay == -1
                && !cell.occupied
                && !cell.start_marker
        })
}

/// Flat clear ground with nothing on it, kept by the generator or not.
fn bare(ctx: &FairCtx<'_>, x: i32, y: i32) -> bool {
    ctx.in_play(x, y)
        && ctx.grid.get(x, y).is_some_and(|cell| {
            ctx.ids.is_clear(cell.tile)
                && cell.slope == 0
                && cell.overlay == -1
                && !cell.occupied
                && !cell.start_marker
        })
}

/// Whether a building of `footprint` at `anchor` stands on free ground of
/// one level with `margin` free cells all round, so it touches nothing.
fn site_fits(
    ctx: &FairCtx<'_>,
    anchor: (i32, i32),
    footprint: &[(i16, i16)],
    margin: i32,
    also: Also,
) -> bool {
    let Some(level) = ctx.grid.get(anchor.0, anchor.1).map(|cell| cell.level) else {
        return false;
    };
    footprint.iter().all(|&(dx, dy)| {
        (-margin..=margin).all(|my| {
            (-margin..=margin).all(|mx| {
                let (x, y) = (anchor.0 + i32::from(dx) + mx, anchor.1 + i32::from(dy) + my);
                (free_ground(ctx, x, y)
                    || (also == Also::Pavement && town_pavement(ctx, x, y))
                    || (also == Also::Kept && bare(ctx, x, y)))
                    && ctx.grid.get(x, y).is_some_and(|cell| cell.level == level)
            })
        })
    })
}

/// Stand a neutral building: its cells are taken, its constructor traced.
fn place_building(
    ctx: &mut FairCtx<'_>,
    out: &mut EmbellishOutput<'_>,
    name: &str,
    footprint: &[(i16, i16)],
    anchor: (i32, i32),
) {
    for &(dx, dy) in footprint {
        if let Some(cell) = ctx
            .grid
            .get_mut(anchor.0 + i32::from(dx), anchor.1 + i32::from(dy))
        {
            cell.occupied = true;
        }
    }
    let event = out
        .trace
        .push_discarded(RmgConstructionPhase::UrbanAreas, name.to_string());
    out.trace.mark_emitted(
        event,
        out.entity_base + out.objects.len(),
        (anchor.0 as u16, anchor.1 as u16),
    );
    out.objects.push(UrbanObject {
        name: name.to_string(),
        category: EntityCategory::Structure,
        cell: (anchor.0 as i16, anchor.1 as i16),
        facing: 0,
        derelict: false,
    });
}

/// Stand a derelict combat building: same placement and trace as
/// [`place_building`], but flagged so the build post-pass powers it down and
/// damages it.
fn place_derelict(
    ctx: &mut FairCtx<'_>,
    out: &mut EmbellishOutput<'_>,
    name: &str,
    footprint: &[(i16, i16)],
    anchor: (i32, i32),
) {
    place_building(ctx, out, name, footprint, anchor);
    if let Some(object) = out.objects.last_mut() {
        object.derelict = true;
    }
}

/// Base Room: the trees in the square round each start go, the ore trees
/// (`TIBTRE`) stay.
fn clear_base_trees(ctx: &mut FairCtx<'_>, starts: &[(i32, i32)], out: &mut EmbellishOutput<'_>) {
    out.terrain.retain(|(name, x, y)| {
        let cell = (i32::from(*x), i32::from(*y));
        let near = starts.iter().any(|&start| {
            (start.0 - cell.0).abs() <= BASE_ROOM_RADIUS
                && (start.1 - cell.1).abs() <= BASE_ROOM_RADIUS
        });
        let tree = name
            .get(..4)
            .is_some_and(|head| head.eq_ignore_ascii_case("TREE"));
        if near && tree {
            if let Some(ground) = ctx.grid.get_mut(cell.0, cell.1) {
                ground.occupied = false;
            }
            false
        } else {
            true
        }
    });
}

/// Ground a track may cross: any slope (hills and ramps), or flat ground
/// that is clear, a ground patch or pavement. Cliffs and water are neither.
fn crossable(ctx: &FairCtx<'_>, x: i32, y: i32) -> bool {
    ctx.in_play(x, y)
        && ctx.grid.get(x, y).is_some_and(|cell| {
            cell.slope != 0
                || ctx.ids.is_open_ground(cell.tile)
                || ctx.ids.is_pave(cell.tile)
                || ctx.ids.is_misc_pave(cell.tile)
                || ctx.ids.is_paved_road(cell.tile)
        })
}

/// Flat open ground with nothing on it: what a track paves.
fn pavable(ctx: &FairCtx<'_>, x: i32, y: i32) -> bool {
    ctx.in_play(x, y)
        && ctx.grid.get(x, y).is_some_and(|cell| {
            ctx.ids.is_open_ground(cell.tile)
                && cell.slope == 0
                && cell.overlay == -1
                && !cell.occupied
        })
}

/// Roads: one flood over crossable ground from the middle of the map gives
/// every start its shortest way there; each way is paved two cells wide (a 2x2 brush)
/// where the ground is flat and free, so the tracks join toward the middle
/// and break at ramps, ore and trees. A start the flood does not reach (an
/// island) gets none. False on a theater without pavement.
fn pave_roads(ctx: &mut FairCtx<'_>, starts: &[(i32, i32)], centre: (i32, i32)) -> bool {
    if ctx.ids.pave <= 0 {
        return false;
    }
    let Some(origin) = (0..=CENTRE_REACH)
        .flat_map(|radius| ring(centre, radius))
        .find(|&(x, y)| crossable(ctx, x, y))
    else {
        return false;
    };
    let width = ctx.grid.width() as i32;
    let slot = |(x, y): (i32, i32)| (y * width + x) as usize;
    // The direction back toward the origin, by cell; `NONE` where unreached.
    const NONE: u8 = u8::MAX;
    const ORIGIN: u8 = 8;
    let mut back = vec![NONE; (width * width) as usize];
    back[slot(origin)] = ORIGIN;
    let mut queue = VecDeque::from([origin]);
    while let Some(cell) = queue.pop_front() {
        let level = |ctx: &FairCtx<'_>, (x, y): (i32, i32)| {
            ctx.grid
                .get(x, y)
                .map(|ground| (ground.level, ground.slope != 0))
        };
        let Some((here, here_sloped)) = level(ctx, cell) else {
            continue;
        };
        // Cardinal steps first, so a straight way wins over a diagonal one.
        for dir in [0usize, 2, 4, 6, 1, 3, 5, 7] {
            let next = RmgGrid::step(cell.0, cell.1, dir);
            if next.0 < 0 || next.1 < 0 || next.0 >= width || next.1 >= width {
                continue;
            }
            if back[slot(next)] != NONE || !crossable(ctx, next.0, next.1) {
                continue;
            }
            // Two flat cells of different levels have a cliff edge between.
            let Some((there, there_sloped)) = level(ctx, next) else {
                continue;
            };
            if !here_sloped && !there_sloped && here != there {
                continue;
            }
            // A diagonal step cuts no corner.
            if dir % 2 == 1 {
                let (a, b) = (
                    RmgGrid::step(cell.0, cell.1, dir - 1),
                    RmgGrid::step(cell.0, cell.1, (dir + 1) % 8),
                );
                if !crossable(ctx, a.0, a.1) || !crossable(ctx, b.0, b.1) {
                    continue;
                }
            }
            back[slot(next)] = ((dir + 4) % 8) as u8;
            queue.push_back(next);
        }
    }

    let mut paved = false;
    for &start in starts {
        if start.0 < 0 || start.1 < 0 || start.0 >= width || start.1 >= width {
            continue;
        }
        let mut cell = start;
        while back[slot(cell)] < ORIGIN {
            for (x, y) in [
                cell,
                (cell.0 + 1, cell.1),
                (cell.0, cell.1 + 1),
                (cell.0 + 1, cell.1 + 1),
            ] {
                if pavable(ctx, x, y)
                    && let Some(ground) = ctx.grid.get_mut(x, y)
                {
                    ground.tile = ctx.ids.pave;
                    ground.sub_tile = 0;
                    paved = true;
                }
            }
            cell = RmgGrid::step(cell.0, cell.1, usize::from(back[slot(cell)]));
        }
    }
    paved
}

/// Fair Tech: one of the neutral tech buildings named `name` to a start, on
/// the free site nearest [`TECH_DISTANCE`] from it that is nearer to it than
/// to any other start.
fn fair_tech(
    ctx: &mut FairCtx<'_>,
    starts: &[(i32, i32)],
    name: &str,
    tech: &[TechType],
    out: &mut EmbellishOutput<'_>,
) {
    let Some(derrick) = tech
        .iter()
        .find(|kind| kind.name.eq_ignore_ascii_case(name) && !kind.footprint.is_empty())
    else {
        return;
    };
    let reach = TECH_DISTANCE + TECH_REACH;
    let (least, most) = (
        i64::from((TECH_DISTANCE - TECH_REACH).pow(2)),
        i64::from(reach.pow(2)),
    );
    let aim = i64::from(TECH_DISTANCE.pow(2));
    for &start in starts {
        let mut sites: Vec<(i32, i32)> = (start.1 - reach..=start.1 + reach)
            .flat_map(|y| (start.0 - reach..=start.0 + reach).map(move |x| (x, y)))
            .filter(|&cell| {
                let distance = dist_sq(start, cell);
                (least..=most).contains(&distance)
                    && starts
                        .iter()
                        .all(|&other| other == start || dist_sq(other, cell) > distance)
            })
            .collect();
        sites.sort_by_key(|&cell| ((dist_sq(start, cell) - aim).abs(), cell.1, cell.0));
        if let Some(site) = sites
            .into_iter()
            .find(|&site| site_fits(ctx, site, &derrick.footprint, 1, Also::Kept))
        {
            place_building(ctx, out, &derrick.name, &derrick.footprint, site);
        }
    }
}

/// Landmark: one of the theater's monuments, drawn from the list, on the
/// free site nearest the middle of the map: open ground, or the pavement of
/// a town that lies there.
fn landmark(
    ctx: &mut FairCtx<'_>,
    args: &EmbellishArgs<'_>,
    starts: &[(i32, i32)],
    centre: (i32, i32),
    catalog: &UrbanCatalog,
    out: &mut EmbellishOutput<'_>,
) {
    let known: Vec<(&UrbanStructure, &Vec<(i16, i16)>)> = catalog
        .landmarks
        .iter()
        .filter_map(|entry| entry.footprint.as_ref().map(|footprint| (entry, footprint)))
        .collect();
    if known.is_empty() {
        return;
    }
    let mut rng = RmgRng::new(args.seed ^ LANDMARK_SALT);
    let (entry, footprint) = known[rng.uniform(0, known.len() as i32 - 1) as usize];
    let site = (0..=CENTRE_REACH)
        .flat_map(|radius| ring(centre, radius))
        .find(|&site| {
            clear_of_starts(starts, site, START_CLEARANCE)
                && site_fits(ctx, site, footprint, 1, Also::Pavement)
        });
    if let Some(site) = site {
        place_building(ctx, out, &entry.name, footprint, site);
    }
}

/// Centre Gems: a gem field grown outward from the free ground nearest the
/// middle of the map, thickest at its heart, clear of every start.
fn centre_gems(
    ctx: &mut FairCtx<'_>,
    args: &EmbellishArgs<'_>,
    starts: &[(i32, i32)],
    centre: (i32, i32),
) {
    let free = |ctx: &FairCtx<'_>, cell: (i32, i32)| {
        ctx.takes_ore(cell.0, cell.1) && clear_of_starts(starts, cell, START_CLEARANCE)
    };
    let Some(heart) = (0..=CENTRE_REACH)
        .flat_map(|radius| ring(centre, radius))
        .find(|&cell| free(ctx, cell))
    else {
        return;
    };
    let wanted = GEM_CELLS[usize::from(args.centre_gems.clamp(1, 2)) - 1];
    let mut rng = RmgRng::new(args.seed ^ GEM_SALT);
    let mut queue = VecDeque::from([heart]);
    let mut planted = 0;
    while planted < wanted {
        let Some(cell) = queue.pop_front() else {
            break;
        };
        if !free(ctx, cell) {
            continue;
        }
        // Twelve thicknesses, thinning a step every two cells from the heart.
        let spread = (dist_sq(heart, cell) as f64).sqrt() as i32;
        let density = (DENSITY_LEVELS - 1 - spread / 2).clamp(2, DENSITY_LEVELS - 1);
        let look = rng.uniform(0, DENSITY_LEVELS - 1);
        if let Some(ground) = ctx.grid.get_mut(cell.0, cell.1) {
            ground.overlay = GEM_BASE + look;
            ground.density = density as u8;
            planted += 1;
        }
        for dir in 0..8 {
            queue.push_back(RmgGrid::step(cell.0, cell.1, dir));
        }
    }
}

/// Countryside: farmsteads on open ground away from the starts and from
/// each other. A farmstead is a house or barn, one to three outbuildings
/// round it and, two times in three, a fence along two of its sides with a
/// gap in each.
fn countryside(
    ctx: &mut FairCtx<'_>,
    args: &EmbellishArgs<'_>,
    starts: &[(i32, i32)],
    play: &[(i32, i32)],
    catalog: &UrbanCatalog,
    out: &mut EmbellishOutput<'_>,
) {
    let farms: Vec<(&UrbanStructure, &Vec<(i16, i16)>)> = catalog
        .farms
        .iter()
        .filter_map(|entry| entry.footprint.as_ref().map(|footprint| (entry, footprint)))
        .collect();
    let houses: Vec<(&UrbanStructure, &Vec<(i16, i16)>)> = farms
        .iter()
        .copied()
        .filter(|(_, footprint)| footprint.len() > 1)
        .collect();
    if houses.is_empty() {
        return;
    }
    let open: Vec<(i32, i32)> = play
        .iter()
        .copied()
        .filter(|&cell| {
            free_ground(ctx, cell.0, cell.1) && clear_of_starts(starts, cell, START_CLEARANCE + 2)
        })
        .collect();
    if open.is_empty() {
        return;
    }
    let per_farm = CELLS_PER_FARM[usize::from(args.countryside.clamp(1, 2)) - 1];
    let wanted = (play.len() as i32 / per_farm).clamp(2, MAX_FARMS);
    let mut rng = RmgRng::new(args.seed ^ FARM_SALT);
    let mut steads: Vec<(i32, i32)> = Vec::new();
    for _ in 0..wanted * 100 {
        if steads.len() as i32 >= wanted {
            break;
        }
        let anchor = open[rng.uniform(0, open.len() as i32 - 1) as usize];
        let (house, footprint) = houses[rng.uniform(0, houses.len() as i32 - 1) as usize];
        if !clear_of_starts(&steads, anchor, FARM_SPACING)
            || !site_fits(ctx, anchor, footprint, 1, Also::Nothing)
        {
            continue;
        }
        place_building(ctx, out, &house.name, footprint, anchor);
        steads.push(anchor);
        for _ in 0..rng.uniform(1, 3) {
            let (shed, footprint) = farms[rng.uniform(0, farms.len() as i32 - 1) as usize];
            for _ in 0..8 {
                let site = (anchor.0 + rng.uniform(-5, 5), anchor.1 + rng.uniform(-5, 5));
                if site_fits(ctx, site, footprint, 1, Also::Nothing) {
                    place_building(ctx, out, &shed.name, footprint, site);
                    break;
                }
            }
        }
        if let Some(fence) = args.farm_fence
            && rng.uniform(0, 2) != 0
        {
            // Two arms from the corner north-west of the house.
            let corner = (anchor.0 - 4, anchor.1 - 4);
            for step in 0..10 {
                // The fifth cell of each arm is the gap.
                if step == 5 {
                    continue;
                }
                for cell in [(corner.0 + step, corner.1), (corner.0, corner.1 + step)] {
                    if free_ground(ctx, cell.0, cell.1)
                        && let Some(ground) = ctx.grid.get_mut(cell.0, cell.1)
                    {
                        ground.overlay = fence;
                        ground.density = 0;
                    }
                }
            }
        }
    }
}

/// VERA extension: the Advanced Derelicts slider. Stands neutral, unpowered
/// combat buildings — radar, lab, refinery, war factory, barracks, power,
/// yard — over open ground away from the starts, from its own RNG stream so
/// no other stage's draws move. The build post-pass powers them down and
/// damages them; here they only take their cells and construction trace.
const DERELICT_SPACING: i32 = 12;
const DERELICT_MAX: i32 = 60;

fn derelicts(
    ctx: &mut FairCtx<'_>,
    args: &EmbellishArgs<'_>,
    starts: &[(i32, i32)],
    play: &[(i32, i32)],
    catalog: &UrbanCatalog,
    out: &mut EmbellishOutput<'_>,
) {
    let kinds: Vec<(&UrbanStructure, &Vec<(i16, i16)>)> = catalog
        .derelicts
        .iter()
        .filter_map(|entry| entry.footprint.as_ref().map(|f| (entry, f)))
        .collect();
    if kinds.is_empty() {
        return;
    }
    // 0..100 -> a capped share of the map's open cells; small maps get fewer.
    let wanted = ((args.derelict_amount.clamp(0, 100) * DERELICT_MAX / 100)
        .min(play.len() as i32 / 900))
    .max(1);
    let open: Vec<(i32, i32)> = play
        .iter()
        .copied()
        .filter(|&cell| {
            free_ground(ctx, cell.0, cell.1) && clear_of_starts(starts, cell, START_CLEARANCE)
        })
        .collect();
    if open.is_empty() {
        return;
    }
    let mut rng = RmgRng::new(args.seed ^ DERELICT_SALT);
    let mut placed: Vec<(i32, i32)> = Vec::new();
    for _ in 0..wanted * 120 {
        if placed.len() as i32 >= wanted {
            break;
        }
        let anchor = open[rng.uniform(0, open.len() as i32 - 1) as usize];
        let (kind, footprint) = kinds[rng.uniform(0, kinds.len() as i32 - 1) as usize];
        if !clear_of_starts(&placed, anchor, DERELICT_SPACING)
            || !site_fits(ctx, anchor, footprint, 1, Also::Nothing)
        {
            continue;
        }
        place_derelict(ctx, out, &kind.name, footprint, anchor);
        placed.push(anchor);
    }
}

/// VERA extension: the three spawn sliders. Scatters foot-sized units over
/// clear open ground and, for the types the Rules say stand only on water,
/// over open water — away from the starts, each slider from its own RNG
/// stream. The types come from the rules (a mod's own units are picked up
/// automatically) and are emitted on Neutral through [`UrbanObject`], the
/// same path the urban pass' civilians and the tech buildings use. Infantry
/// and vehicles keep their registry category; no other stage's draw moves.
const SPAWN_SPACING: i32 = 3;
const SPAWN_MAX: i32 = 120;
/// Of a slider's spawns, one in this many stands on water (at least one),
/// when the map has water and the slurry lists a type for it.
const SPAWN_WATER_DIVISOR: i32 = 3;

/// A cell a water-only unit may stand on: open sea (`WaterSet` tiles only —
/// a shore piece is walkable beach, not water) with nothing on it.
fn open_water(ctx: &FairCtx<'_>, cell: (i32, i32)) -> bool {
    ctx.in_play(cell.0, cell.1)
        && ctx
            .grid
            .get(cell.0, cell.1)
            .is_some_and(|cell| ctx.ids.is_water(cell.tile) && !cell.occupied && !cell.start_marker)
}

/// Shuffle `order` in place, so a pass over it takes every kind once.
fn shuffle(rng: &mut RmgRng, order: &mut [usize]) {
    for index in (1..order.len()).rev() {
        let other = rng.uniform(0, index as i32) as usize;
        order.swap(index, other);
    }
}

/// Stand up to `wanted` units of `kinds` on `open`, at least
/// [`SPAWN_SPACING`] apart. The kinds are taken in shuffled turns, so a small
/// budget still shows many of them rather than one type's run of luck.
fn scatter(
    ctx: &mut FairCtx<'_>,
    open: &[(i32, i32)],
    kinds: &[(String, EntityCategory)],
    wanted: i32,
    rng: &mut RmgRng,
    out: &mut EmbellishOutput<'_>,
    placed: &mut Vec<(i32, i32)>,
) {
    if wanted <= 0 || open.is_empty() || kinds.is_empty() {
        return;
    }
    let mut order: Vec<usize> = (0..kinds.len()).collect();
    shuffle(rng, &mut order);
    let mut cursor = order.len();
    let mut count = 0;
    for _ in 0..wanted * 60 {
        if count >= wanted {
            break;
        }
        let cell = open[rng.uniform(0, open.len() as i32 - 1) as usize];
        if !clear_of_starts(placed, cell, SPAWN_SPACING) {
            continue;
        }
        if cursor == order.len() {
            shuffle(rng, &mut order);
            cursor = 0;
        }
        let (name, category) = &kinds[order[cursor]];
        cursor += 1;
        // A foot does not claim its cell for placement (towns put several on
        // one tile), but marking it keeps two spawns off the same spot here.
        ctx.grid.cell_native_mut(cell.0, cell.1).occupied = true;
        let event = out
            .trace
            .push_discarded(RmgConstructionPhase::UrbanAreas, name.clone());
        out.trace.mark_emitted(
            event,
            out.entity_base + out.objects.len(),
            (cell.0 as u16, cell.1 as u16),
        );
        out.objects.push(UrbanObject {
            name: name.clone(),
            category: *category,
            cell: (cell.0 as i16, cell.1 as i16),
            facing: ((rng.uniform(0, 7) << 5) & 0xFF) as u8,
            derelict: false,
        });
        placed.push(cell);
        count += 1;
    }
}

fn spawn_foots(
    ctx: &mut FairCtx<'_>,
    starts: &[(i32, i32)],
    play: &[(i32, i32)],
    kinds: &SpawnFoots,
    amount: i32,
    seed: u16,
    out: &mut EmbellishOutput<'_>,
) {
    if kinds.is_empty() {
        return;
    }
    // 0..100 -> a capped share of the map's open cells; small maps get fewer.
    let wanted = ((amount.clamp(0, 100) * SPAWN_MAX / 100).min(play.len() as i32 / 90)).max(1);
    let land_open: Vec<(i32, i32)> = play
        .iter()
        .copied()
        .filter(|&cell| {
            free_ground(ctx, cell.0, cell.1) && clear_of_starts(starts, cell, START_CLEARANCE)
        })
        .collect();
    let water_open: Vec<(i32, i32)> = play
        .iter()
        .copied()
        .filter(|&cell| open_water(ctx, cell) && clear_of_starts(starts, cell, START_CLEARANCE))
        .collect();

    let land: Vec<(String, EntityCategory)> = kinds
        .infantry
        .iter()
        .map(|name| (name.clone(), EntityCategory::Infantry))
        .chain(
            kinds
                .vehicles
                .iter()
                .map(|name| (name.clone(), EntityCategory::Unit)),
        )
        .collect();
    let water: Vec<(String, EntityCategory)> = kinds
        .water_infantry
        .iter()
        .map(|name| (name.clone(), EntityCategory::Infantry))
        .chain(
            kinds
                .water_vehicles
                .iter()
                .map(|name| (name.clone(), EntityCategory::Unit)),
        )
        .collect();

    // Water types get their share only where the map has water to hold them;
    // a slider of land types alone fills the land as before.
    let water_share = if water.is_empty() || water_open.is_empty() {
        0
    } else {
        (wanted / SPAWN_WATER_DIVISOR).max(1)
    };
    let land_share = if land.is_empty() {
        0
    } else {
        (wanted - water_share).max(0)
    };

    let mut rng = RmgRng::new(seed);
    let mut placed: Vec<(i32, i32)> = Vec::new();
    scatter(
        ctx,
        &land_open,
        &land,
        land_share,
        &mut rng,
        out,
        &mut placed,
    );
    scatter(
        ctx,
        &water_open,
        &water,
        water_share,
        &mut rng,
        out,
        &mut placed,
    );
}

/// Fences: short runs of fence on a town's pavement along the sides of its
/// buildings, never on a road, so the streets stay open.
fn town_fences(ctx: &mut FairCtx<'_>, args: &EmbellishArgs<'_>, play: &[(i32, i32)]) {
    let Some(fence) = args.town_fence else {
        return;
    };
    let beside =
        |ctx: &FairCtx<'_>, cell: (i32, i32), test: &dyn Fn(&FairCtx<'_>, i32, i32) -> bool| {
            (0..8).step_by(2).any(|dir| {
                let (x, y) = RmgGrid::step(cell.0, cell.1, dir);
                test(ctx, x, y)
            })
        };
    let built = |ctx: &FairCtx<'_>, x: i32, y: i32| {
        ctx.grid
            .get(x, y)
            .is_some_and(|cell| cell.occupied && ctx.ids.is_pave(cell.tile))
    };
    let kerb = |ctx: &FairCtx<'_>, cell: (i32, i32)| {
        town_pavement(ctx, cell.0, cell.1) && beside(ctx, cell, &built)
    };
    let sides: Vec<(i32, i32)> = play
        .iter()
        .copied()
        .filter(|&cell| kerb(ctx, cell))
        .collect();
    if sides.is_empty() {
        return;
    }
    let mut rng = RmgRng::new(args.seed ^ FENCE_SALT);
    for _ in 0..sides.len() as i32 / KERB_PER_FENCE {
        let from = sides[rng.uniform(0, sides.len() as i32 - 1) as usize];
        let dir = if rng.uniform(0, 1) == 0 { 2 } else { 4 };
        let length = rng.uniform(FENCE_RUN.0, FENCE_RUN.1);
        let mut run = Vec::new();
        let mut cell = from;
        while (run.len() as i32) < length && kerb(ctx, cell) {
            run.push(cell);
            cell = RmgGrid::step(cell.0, cell.1, dir);
        }
        if run.len() < 2 {
            continue;
        }
        for (x, y) in run {
            if let Some(ground) = ctx.grid.get_mut(x, y) {
                ground.overlay = fence;
                ground.density = 0;
            }
        }
    }
}

/// Give every cell of fence `fence` the frame that joins it to the fence
/// north, east, south and west of it (bits 0..=3), as a map file stores a
/// wall's connections.
fn link_fences(ctx: &mut FairCtx<'_>, play: &[(i32, i32)], fence: i32) {
    let is_fence = |ctx: &FairCtx<'_>, (x, y): (i32, i32)| {
        ctx.grid.get(x, y).is_some_and(|cell| cell.overlay == fence)
    };
    let frames: Vec<((i32, i32), u8)> = play
        .iter()
        .copied()
        .filter(|&cell| is_fence(ctx, cell))
        .map(|cell| {
            let mut frame = 0u8;
            for (bit, dir) in [0usize, 2, 4, 6].into_iter().enumerate() {
                if is_fence(ctx, RmgGrid::step(cell.0, cell.1, dir)) {
                    frame |= 1 << bit;
                }
            }
            (cell, frame)
        })
        .collect();
    for ((x, y), frame) in frames {
        if let Some(ground) = ctx.grid.get_mut(x, y) {
            ground.density = frame;
        }
    }
}
