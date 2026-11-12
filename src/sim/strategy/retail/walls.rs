//! Walls around the buildings that are worth more than the wall.
//!
//! What gets walled is the retail data's own answer: `ProtectWithWall=yes`
//! marks a BuildingType the computer may wall in
//! (`BuildingTypeClass ReadINI 0x0046026B..0x0046027F`), and retail sets it on
//! exactly the objects a house cannot afford to lose — the Construction Yard,
//! the tech lab and the superweapons. The same key drives
//! `sim::ai_base_building`'s site border, so a mod that marks a building is
//! walled without a name list here.
//!
//! Two limits come from the engine rather than from the data:
//!
//! - A wall is a `Wall=yes` BuildingType with a one-cell foundation
//!   (`[GAWALL] Adjacent=8`, `Cost=100`, `BuildCat=Combat`), built out of the
//!   **Defense** line, not the yard's building queue. One segment in the queue
//!   is not one cell on the map: `MapClass588750 @ 0x00588750` scans the four
//!   cardinal directions from the cell a wall lands on and constructs every
//!   cell between it and this house's own wall within `GuardRange`
//!   ([`crate::sim::production::wall_placement`]). A straight run is therefore
//!   closed by placing its two ends, the way a player drags one down a line,
//!   and it costs two slots rather than one per cell.
//! - A wall blocks its owner's units as well, and retail has no gate this house
//!   can build, so the ring is closed all the way round except one cell on the
//!   side away from the enemy ([`gate_cell`]): that is where the yard's own
//!   units come and go, and one cell is the smallest hole an attack can use. A
//!   half-ring — the enemy-facing arc alone — is not a wall: it leaves three
//!   sides of a captured lab open, which is what "the base is only half
//!   enclosed" looks like on the map.
//!
//! The result is a segment plan, not a picture: the next cell to fill is
//! computed from the base as it stands, so a wall that was interrupted, a
//! building that died, or a base that grew all answer the next question
//! correctly without a stored plan to go stale.

use std::collections::BTreeSet;

use crate::rules::object_type::{ObjectCategory, ObjectType};
use crate::rules::ruleset::RuleSet;
use crate::sim::control::brain::Outbox;
use crate::sim::control::intent::Intent;
use crate::sim::control::observation::Observation;
use crate::sim::intern::InternedId;
use crate::sim::production::{CanBuild, ProductionCategory};

use super::Goal;
use super::RetailBrain;
use super::tracked::TrackedKind;

/// Credits in hand before a segment is worth a production slot. A wall costs
/// the price of a soldier and stalls nothing, but a house that builds walls on
/// its last credits has an empty base with a pretty outline.
pub(super) const WALL_CREDITS: i32 = 2_000;

/// Structures on the map before the base is worth walling: a yard and a
/// refinery do not need an outline, and the first minutes belong to the
/// opening.
pub(super) const WALL_MIN_STRUCTURES: usize = 6;

/// Cells a house may have walled. The outline of a base with a yard, two
/// refineries, a factory, a barracks and a lab is around fifty cells, and the
/// engine's own fill closes most of them from a handful of segments. Past this
/// the walls are eating the defenses' whole queue.
pub(super) const WALL_SEGMENT_CAP: usize = 56;

/// The wall segment the base wants: the cell to put it on, the run it closes,
/// and how much of the wall already stands.
pub(super) struct Segment {
    /// The house's own wall type.
    pub(super) type_id: InternedId,
    /// The cell to place on: the end of a run, not the cell next to the last
    /// one placed.
    pub(super) cell: (u16, u16),
    /// Wall cells already standing on the rings the plan fills.
    pub(super) standing: usize,
    /// Cells the engine will build in the gap this segment closes.
    pub(super) fill: usize,
}

/// The wall's own scan order: north, east, south, west
/// (`sim/production/wall_placement::CARDINAL_DIRECTIONS`).
const CARDINAL: [(i32, i32); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

/// How many cells the engine fills in when a wall goes down on `cell`: the
/// cells between it and this house's own wall, in one cardinal direction, up to
/// the type's `GuardRange` away. Zero when there is nothing to close.
///
/// `MapClass588750 @ 0x00588750` walks each direction from the cell placed and
/// constructs the gap it finds, discarding any direction whose cells are not
/// clear to build. So a run is paid for by its two ends, and the cell worth a
/// production slot is the one that closes the longest gap — a wall next to the
/// one already standing closes nothing at all.
pub(super) fn autofill_gain(
    cell: (u16, u16),
    limit: usize,
    walled: impl Fn(u16, u16) -> bool,
    free: impl Fn(u16, u16) -> bool,
) -> usize {
    if limit == 0 {
        return 0;
    }
    let mut best = 0;
    for (dx, dy) in CARDINAL {
        let (mut x, mut y) = (i32::from(cell.0), i32::from(cell.1));
        for step in 1..=limit {
            x += dx;
            y += dy;
            let (Ok(cx), Ok(cy)) = (u16::try_from(x), u16::try_from(y)) else {
                break;
            };
            if walled(cx, cy) {
                best = best.max(step - 1);
                break;
            }
            if !free(cx, cy) {
                break;
            }
        }
    }
    best
}

/// Whether `ty` is a building the retail data says to wall in.
pub(super) fn protected(ty: &ObjectType) -> bool {
    ty.protect_with_wall && ty.category == ObjectCategory::Building
}

/// The cells around a building's foundation, one ring out.
///
/// This is *not* where the wall stands: it is the cell beside the foundation,
/// the lane the house's own units are born on. [`ring_at`] is the door to a
/// ring at a stated offset, and the wall's is `WALL_OFFSET_CELLS`.
pub(super) fn perimeter(cell: (u16, u16), foundation: (u16, u16)) -> Vec<(u16, u16)> {
    let (width, height) = (
        i32::from(foundation.0.max(1)),
        i32::from(foundation.1.max(1)),
    );
    let (x0, y0) = (i32::from(cell.0), i32::from(cell.1));
    let (x1, y1) = (x0 + width - 1, y0 + height - 1);
    let mut cells: Vec<(u16, u16)> = Vec::new();
    for y in (y0 - 1)..=(y1 + 1) {
        for x in (x0 - 1)..=(x1 + 1) {
            let on_ring = x < x0 || x > x1 || y < y0 || y > y1;
            if !on_ring || x < 0 || y < 0 {
                continue;
            }
            let (Ok(x), Ok(y)) = (u16::try_from(x), u16::try_from(y)) else {
                continue;
            };
            cells.push((x, y));
        }
    }
    cells
}

/// The ring standing `offset` cells out from a foundation.
///
/// [`perimeter`] answers the ring one cell out, which is the cell beside the
/// foundation and what the wall takes (`WALL_OFFSET_CELLS`); a larger offset
/// grows the footprint by `offset - 1` before it is asked. Written once,
/// because "two cells out" was implemented twice and the second copy was three.
pub(super) fn ring_at(cell: (u16, u16), foundation: (u16, u16), offset: i32) -> Vec<(u16, u16)> {
    let back = u16::try_from(offset.max(1) - 1).unwrap_or(0);
    perimeter(
        (cell.0.saturating_sub(back), cell.1.saturating_sub(back)),
        (
            foundation.0.saturating_add(2 * back),
            foundation.1.saturating_add(2 * back),
        ),
    )
}

/// The one cell of a ring left open as the way in and out: the cell furthest
/// from the enemy, and the most centred among those.
///
/// A closed ring walls the house's own units in, and retail has no gate this
/// house can build. One cell is enough for a column of tanks and is the
/// smallest hole an attacker can use. A house with no enemy keeps every cell:
/// a wall is only ever built toward something.
pub(super) fn gate_cell(
    ring: &[(u16, u16)],
    centre: (u16, u16),
    direction: (i32, i32),
) -> Option<(u16, u16)> {
    if direction == (0, 0) {
        return None;
    }
    ring.iter().copied().min_by_key(|(x, y)| {
        let dx = i32::from(*x) - i32::from(centre.0);
        let dy = i32::from(*y) - i32::from(centre.1);
        (
            // Furthest from the enemy first, then the middle of that side, so
            // a 4x4's gate is the centre of its back wall and not a corner.
            dx * direction.0 + dy * direction.1,
            (dx * direction.1 - dy * direction.0).abs(),
            (*x, *y),
        )
    })
}

/// The house's wall type, resolved once from the rules and cached: a
/// `Wall=yes` BuildingType the house may actually build (`[GAWALL]
/// Prerequisite=GAPILE`).
pub(super) fn wall_type(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
) -> Option<InternedId> {
    if let Some(type_id) = brain.wall_type {
        return Some(type_id);
    }
    let found = rules
        .all_objects()
        .filter(|ty| ty.wall && ty.category == ObjectCategory::Building)
        .find(|ty| matches!(obs.can_build(rules, ty, true), CanBuild::Yes))
        .and_then(|ty| obs.type_id(&ty.id))?;
    brain.wall_type = Some(found);
    Some(found)
}

/// The next wall segment the base wants, when it wants one.
///
/// `None` while the base is too small, too poor, already walled enough, or has
/// no enemy to face: a wall is only ever built toward something.
pub(super) fn next_segment(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
) -> Option<Segment> {
    if brain.census.home_cells.is_empty() {
        return None;
    }
    let structures = obs
        .own_entities()
        .filter(|entity| {
            obs.object_type(rules, entity.type_id)
                .is_some_and(|ty| ty.category == ObjectCategory::Building && !ty.wall)
        })
        .count();
    if structures < WALL_MIN_STRUCTURES {
        return None;
    }
    let (centre, ring) = planned_ring(rules, obs)?;
    let blocked = blocked_cells(rules, obs);
    let house = obs.house();
    // A cell that already holds a wall, its own or a hostile one, is not walled
    // again; the walkability of the path projection says the same, one step
    // later than the grid does.
    // A cell anything stands on is not free, whoever it belongs to. The
    // placement would be refused while the object is still there, and a plan
    // that keeps naming that cell never finishes: the ring closed everything
    // except the cell a rifleman happened to be standing on, and the wall sat
    // in the factory until it was abandoned. Own units standing there are not
    // moved out of the way either — the plan goes round them, and comes back
    // for the cell once they have walked.
    let occupied = obs.ground_object_cells(rules);
    // The wall type, before the cells are judged: a cell that carries a
    // tiberium overlay refuses a wall, and a plan that cannot ask that
    // question keeps naming the same ore cell. Every wall the plan had
    // queued was refused at the same two cells, which is what this answers.
    let wall = wall_type(brain, rules, obs)?;
    let wall_ty = obs.object_type(rules, wall);
    let free = |x: u16, y: u16| {
        !blocked.contains(&(x, y))
            && !occupied.contains(&(x, y))
            && obs.wall_owner_at(x, y).is_none()
            && obs.is_ground_walkable(x, y)
            && wall_ty.is_some_and(|ty| obs.wall_site_admits(rules, ty, x, y))
    };
    // The ring is `planned_ring`'s — the same cells `clear_the_ring` moves a
    // unit off, so the plan and the unblocking pass cannot disagree about the
    // one cell that matters.
    // What the cap counts is the wall on the outline the plan fills. A standing
    // wall is an overlay rather than an object (`Observation::wall_owner_at`),
    // so it is counted where it stands.
    let standing = ring
        .iter()
        .filter(|(x, y)| obs.wall_owner_at(*x, *y) == Some(house))
        .count();
    if standing >= WALL_SEGMENT_CAP {
        return None;
    }
    // The enemy side: the nearest hostile object to the base's centre, so the
    // wall faces the way an attack has to come from. A house that has seen no
    // enemy builds no wall.
    let enemy = brain
        .census
        .enemies
        .iter()
        .map(|enemy| (enemy.cell, chebyshev(enemy.cell, centre)))
        .min_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))?
        .0;
    let direction = (
        i32::from(enemy.0) - i32::from(centre.0),
        i32::from(enemy.1) - i32::from(centre.1),
    );
    // How far the engine looks for this house's own wall when it fills a gap:
    // the wall type's own `GuardRange`, in cells. A type without one closes
    // nothing, and the plan falls back to the cell beside the last one.
    let limit = obs
        .object_type(rules, wall)
        .and_then(|ty| ty.guard_range)
        .map_or(0, |range| range.to_num::<i32>().max(0) as usize);
    // One gate for the whole outline, on the side away from the enemy: the
    // house's own units come and go through it, and one cell is the smallest
    // hole an attack can use. Everything else is closed — a three-sided pen is
    // what "the base is only half enclosed" looks like on the map.
    //
    // The gate is treated as blocked by the fill scan as well: `MapClass` fills
    // whatever lies between two of this house's walls, so a run planned straight
    // across the gate would build over it.
    // Two cells of gate, not one: one cell is a doorway a column of tanks
    // wedges in, and the whole point of the ring is that the house's units get
    // out faster than an attack gets in.
    let mut gate: Vec<(u16, u16)> = Vec::new();
    if let Some(first) = gate_cell(&ring, centre, direction) {
        gate.push(first);
        if let Some(next) = ring
            .iter()
            .copied()
            .filter(|cell| chebyshev(*cell, first) == 1)
            .min_by_key(|cell| {
                let dx = i32::from(cell.0) - i32::from(centre.0);
                let dy = i32::from(cell.1) - i32::from(centre.1);
                (dx * direction.0 + dy * direction.1, *cell)
            })
        {
            gate.push(next);
        }
    }
    let clear = |x: u16, y: u16| free(x, y) && !gate.contains(&(x, y));
    let best = ring
        .iter()
        .copied()
        .filter(|(x, y)| free(*x, *y))
        .map(|cell| {
            let fill = autofill_gain(
                cell,
                limit,
                |x, y| obs.wall_owner_at(x, y) == Some(house),
                &clear,
            );
            let side = (i32::from(cell.0) - i32::from(centre.0)) * direction.0
                + (i32::from(cell.1) - i32::from(centre.1)) * direction.1;
            // The end of the run, not the cell beside the last one: a segment is
            // worth the gap it closes, because the engine builds the cells
            // between for free. Among equal gaps the cell nearest the enemy goes
            // first, so an empty outline starts on the side an attack comes
            // from; then the lower cell, so the answer does not depend on the
            // scan's own order.
            (fill, side, std::cmp::Reverse(cell))
        })
        .max();
    let (fill, _, std::cmp::Reverse(cell)) = best?;
    Some(Segment {
        type_id: wall,
        cell,
        standing,
        fill,
    })
}

/// The cells any building's foundation stands on, own and hostile. Walls are
/// not here: they are overlays, and the grid answers them (`wall_owner_at`).
fn blocked_cells(rules: &RuleSet, obs: &Observation<'_>) -> BTreeSet<(u16, u16)> {
    let mut cells: BTreeSet<(u16, u16)> = BTreeSet::new();
    for (cell, type_id) in obs
        .own_entities()
        .map(|entity| (entity.cell, entity.type_id))
        .chain(
            obs.enemy_entities()
                .map(|entity| (entity.cell, entity.type_id)),
        )
    {
        let Some(ty) = obs.object_type(rules, type_id) else {
            continue;
        };
        if ty.category != ObjectCategory::Building {
            continue;
        }
        let (width, height) = ty.foundation.dimensions();
        for dy in 0..i32::from(height.max(1)) {
            for dx in 0..i32::from(width.max(1)) {
                let (Some(x), Some(y)) = (
                    u16::try_from(i32::from(cell.0) + dx).ok(),
                    u16::try_from(i32::from(cell.1) + dy).ok(),
                ) else {
                    continue;
                };
                cells.insert((x, y));
            }
        }
    }
    cells
}

/// How far from a protected building's foundation its wall ring stands, in
/// cells. One is the cell beside the foundation: the wall hugs the base, with no
/// empty lane between it and the buildings it encloses. Two left a corridor the
/// player could not use and read as a misplaced wall, so the ring closed onto
/// the foundations (decision 2026-10-31).
pub(super) const WALL_OFFSET_CELLS: i32 = 1;

/// How far from the base's centre a building still counts as part of it, in
/// cells: the wall runs around the base, not around a captured derrick on the
/// other side of the map.
const BASE_WALL_RADIUS: i32 = 12;

/// The base's centre and the buildings its outline is taken from, in entity
/// order so the plan is deterministic.
///
/// Every building, not only the ones retail marks `ProtectWithWall=yes`: the
/// wall's job is to stop an attack reaching the base, and a ring per protected
/// building guards the yard while the refineries and the barracks sit outside
/// it. Because one foundation covers its neighbours' rings, the union's free
/// cells are the base's edge — the inside is building on building.
fn base_targets(
    rules: &RuleSet,
    obs: &Observation<'_>,
) -> Option<((u16, u16), Vec<((u16, u16), (u16, u16))>)> {
    let mut targets: Vec<(u64, (u16, u16), (u16, u16))> = Vec::new();
    for entity in obs.own_entities() {
        let Some(ty) = obs.object_type(rules, entity.type_id) else {
            continue;
        };
        if !protected(ty) {
            continue;
        }
        let (width, height) = ty.foundation.dimensions();
        targets.push((entity.entity_id, entity.cell, (width, height)));
    }
    if targets.is_empty() {
        return None;
    }
    targets.sort_by_key(|(entity_id, ..)| *entity_id);
    let centre = |targets: &[(u64, (u16, u16), (u16, u16))]| {
        let (mut sx, mut sy) = (0_i64, 0_i64);
        for (_, cell, _) in targets {
            sx += i64::from(cell.0);
            sy += i64::from(cell.1);
        }
        let count = i64::try_from(targets.len()).unwrap_or(1).max(1);
        (
            u16::try_from(sx / count).unwrap_or(0),
            u16::try_from(sy / count).unwrap_or(0),
        )
    };
    // Centred once on everything to drop the far-flung outliers, then again on
    // what is left: one captured derrick must not drag the wall across the map.
    let rough = centre(&targets);
    targets.retain(|(_, cell, _)| chebyshev(*cell, rough) <= BASE_WALL_RADIUS);
    Some((
        centre(&targets),
        targets
            .into_iter()
            .map(|(_, cell, foundation)| (cell, foundation))
            .collect(),
    ))
}

fn chebyshev(a: (u16, u16), b: (u16, u16)) -> i32 {
    i32::from(a.0)
        .abs_diff(i32::from(b.0))
        .max(i32::from(a.1).abs_diff(i32::from(b.1))) as i32
}

/// Queue one segment, when the base wants one and the Defense line is free.
///
/// Returns `true` when this frame's structure decision was spent on a wall, so
/// the plan does not start something else on top of it.

/// The cells the wall plan wants to fill: one ring around every protected
/// building's foundation, minus the cells a structure or terrain closes.
///
/// One owner for the outline: the planner picks segments from it and the
/// unblocking pass clears the units off it, and two computations of "the ring"
/// would disagree about the cell that matters.
fn planned_ring(rules: &RuleSet, obs: &Observation<'_>) -> Option<((u16, u16), Vec<(u16, u16)>)> {
    let (centre, targets) = base_targets(rules, obs)?;
    let blocked = blocked_cells(rules, obs);
    let mut ring: Vec<(u16, u16)> = Vec::new();
    for (cell, foundation) in &targets {
        for cell in ring_at(*cell, *foundation, WALL_OFFSET_CELLS) {
            if !ring.contains(&cell) {
                ring.push(cell);
            }
        }
    }
    ring.retain(|(x, y)| !blocked.contains(&(*x, *y)));
    ring.sort_unstable();
    Some((centre, ring))
}

/// Ask the house's own units standing on the ring to step outside it.
///
/// The plan goes around whatever occupies a cell, so a single rifleman parked
/// on the ring is a permanent gap: the wall closes everywhere else and the one
/// cell it cannot have is never revisited, which is the base the play test
/// reports as "the wall never went round the yard". A player presses X and the
/// unit steps aside, so this does the same for one unit a frame, to the free
/// cell outside the ring nearest to where it stands.
///
/// Only the house's own orderable ground units are moved, only outwards, and
/// only while nothing else is being built on the ring — a unit already walking
/// somewhere is not this pass's.
fn clear_the_ring(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    out: &mut Outbox,
    tick: u64,
) -> bool {
    let Some((centre, ring)) = planned_ring(rules, obs) else {
        return false;
    };
    let occupied = obs.ground_object_cells(rules);
    // The cell has to be one the wall would otherwise take: open ground, no
    // wall yet, and holding a unit — not a building, a cliff or the sea.
    let blocked_by_structure = blocked_cells(rules, obs);
    let waiting: Vec<(u64, (u16, u16))> = brain
        .census
        .units
        .iter()
        .filter(|unit| unit.orderable && !unit.hand && !unit.in_service())
        .filter(|unit| {
            ring.contains(&unit.cell)
                && !blocked_by_structure.contains(&unit.cell)
                && obs.wall_owner_at(unit.cell.0, unit.cell.1).is_none()
                && obs.is_ground_walkable(unit.cell.0, unit.cell.1)
        })
        .map(|unit| (unit.entity_id, unit.cell))
        .collect();
    let mut moved = false;
    for (entity_id, cell) in waiting {
        if moved {
            break;
        }
        let Some(target) = outward_cell(rules, obs, &ring, &occupied, centre, cell) else {
            continue;
        };
        if !brain
            .tracked
            .admits(tick, entity_id, TrackedKind::Avoid, None, Some(target))
        {
            continue;
        }
        brain
            .tracked
            .record(tick, entity_id, TrackedKind::Avoid, None, Some(target));
        log::debug!("auto play: clear {entity_id} off the wall at {cell:?} to {target:?}");
        if super::orders::issue(
            brain,
            out,
            rules,
            obs,
            entity_id,
            Goal::Move { cell: target },
        ) {
            moved = true;
        } else {
            brain.tracked.void(entity_id, TrackedKind::Avoid);
        }
    }
    moved
}

/// The free cell outside the ring nearest to `from`, preferring the side away
/// from the base's centre so the unit steps out rather than through.
fn outward_cell(
    rules: &RuleSet,
    obs: &Observation<'_>,
    ring: &[(u16, u16)],
    occupied: &std::collections::BTreeSet<(u16, u16)>,
    centre: (u16, u16),
    from: (u16, u16),
) -> Option<(u16, u16)> {
    let (width, height) = obs.map_extent();
    let mut best: Option<((u16, u16), i32, i32)> = None;
    for dy in -3i32..=3 {
        for dx in -3i32..=3 {
            let (Some(x), Some(y)) = (
                u16::try_from(i32::from(from.0) + dx).ok(),
                u16::try_from(i32::from(from.1) + dy).ok(),
            ) else {
                continue;
            };
            if width > 0 && (x >= width || y >= height) {
                continue;
            }
            let cell = (x, y);
            if ring.contains(&cell)
                || occupied.contains(&cell)
                || obs.wall_owner_at(x, y).is_some()
                || !obs.is_ground_walkable(x, y)
            {
                continue;
            }
            // Further from the centre than the cell it leaves: a unit parked
            // on the ring steps outward, not into the yard it is walling.
            let outward = chebyshev(cell, centre) > chebyshev(from, centre);
            let key = (
                cell,
                i32::from(u8::from(outward)),
                super::squared_distance(cell, from),
            );
            if best.is_none_or(|(_, held_out, held_distance)| {
                (key.1, -key.2) > (held_out, -held_distance)
            }) {
                best = Some((cell, key.1, key.2));
            }
        }
    }
    let _ = rules;
    best.map(|(cell, _, _)| cell)
}

pub(super) fn queue_segment(
    brain: &mut RetailBrain,
    rules: &RuleSet,
    obs: &Observation<'_>,
    free_credits: i32,
    out: &mut Outbox,
) -> bool {
    if free_credits < WALL_CREDITS {
        return false;
    }
    // A unit standing on the ring is a hole the plan keeps going round, and
    // it is also how a unit ends up inside the pen: nudge it off every frame,
    // not only once the last piece has been placed and the plan gives up.
    clear_the_ring(brain, rules, obs, out, obs.tick());
    let line = brain.census.line(ProductionCategory::Defense);
    if line.busy() {
        return false;
    }
    let Some(segment) = next_segment(brain, rules, obs) else {
        return false;
    };
    log::info!(
        "auto play: wall segment {} at tick {} credits={} cell={:?} standing={} fill={}",
        obs.object_type(rules, segment.type_id)
            .map_or("?", |ty| ty.id.as_str()),
        obs.tick(),
        free_credits,
        segment.cell,
        segment.standing,
        segment.fill,
    );
    out.intent(Intent::QueueProduction {
        type_id: segment.type_id,
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The plan asks the placement's own question about a cell.
    ///
    /// Every wall the plan had queued was refused at the same two cells, and
    /// the plan went on naming them: a cell that carries tiberium refuses a
    /// wall, and a plan that cannot see that keeps drawing the same ore.
    #[test]
    fn a_wall_cell_is_one_the_placement_admits() {
        let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules() else {
            return;
        };
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0xB04);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let owner = sim.interner.intern("Americans");
        let obs = Observation::new(&sim, owner);
        let wall = rules.object("GAWALL").expect("the wall");
        assert!(
            obs.wall_site_admits(&rules, wall, 10, 10),
            "flat ground takes a wall"
        );
    }
    /// The ring around a 2x2 foundation is twelve cells: the eight sides plus
    /// the four corners, all one cell out.
    #[test]
    fn a_ring_surrounds_the_foundation() {
        let ring = perimeter((10, 10), (2, 2));
        assert_eq!(ring.len(), 12);
        assert!(ring.contains(&(9, 9)), "corner");
        assert!(ring.contains(&(12, 12)), "far corner");
        assert!(ring.contains(&(11, 9)), "edge");
        assert!(!ring.contains(&(10, 10)), "the foundation itself");
        assert!(!ring.contains(&(11, 11)), "inside the foundation");
    }

    /// The ring stands on the cell beside the foundation, so the wall touches
    /// the base: no empty lane is left between the two, and the plan's ring is
    /// the same ring `build.rs` reserves.
    #[test]
    fn the_ring_stands_beside_the_foundation() {
        let ring = ring_at((10, 10), (2, 2), WALL_OFFSET_CELLS);
        assert_eq!(ring, perimeter((10, 10), (2, 2)));
        assert!(ring.contains(&(9, 11)), "beside the foundation is the wall");
        assert!(ring.contains(&(11, 9)), "and so is the far side");
        assert!(!ring.contains(&(8, 11)), "two cells out is not");
        assert!(!ring.contains(&(11, 11)), "inside the foundation is not");
    }

    /// The ring is closed all the way round except one cell on the side away
    /// from the enemy: the house's own units keep a way out, and the wall has
    /// no other hole.
    #[test]
    fn the_gate_is_the_one_cell_facing_away_from_the_enemy() {
        let ring = perimeter((10, 10), (2, 2));
        // A 2x2 standing at (10,10) has its centre at (11,11); the enemy is
        // east, so the gate is the middle of the west wall.
        assert_eq!(gate_cell(&ring, (11, 11), (1, 0)), Some((9, 11)));
        // With the enemy to the south the gate moves to the north wall.
        assert_eq!(gate_cell(&ring, (11, 11), (0, 1)), Some((11, 9)));
        // One gate, and it is on the ring itself.
        assert!(ring.contains(&gate_cell(&ring, (11, 11), (1, 0)).expect("a gate")));
        // Nothing to face means no gate; the ring stays whole.
        assert_eq!(gate_cell(&ring, (11, 11), (0, 0)), None);
    }

    /// The cell worth a production slot is the end of the run, because the
    /// engine builds the gap between it and the wall that already stands.
    #[test]
    fn the_far_end_of_a_run_is_the_cell_that_closes_it() {
        // One wall at (10,10), then four cells a wall could go on.
        let walled = |x: u16, _y: u16| x == 10;
        let free = |x: u16, _y: u16| (11..=14).contains(&x);
        assert_eq!(
            autofill_gain((14, 10), 8, walled, free),
            3,
            "three cells between the two ends"
        );
        assert_eq!(
            autofill_gain((11, 10), 8, walled, free),
            0,
            "a wall beside the one standing closes nothing"
        );
        assert_eq!(
            autofill_gain((14, 10), 3, walled, free),
            0,
            "past GuardRange the wall is never found"
        );
        // A cell that cannot be built on discards the whole direction.
        assert_eq!(
            autofill_gain((14, 10), 8, walled, |x, y| free(x, y) && x != 13),
            0,
            "a blocked cell between the two ends"
        );
        // A type with no GuardRange closes nothing at all.
        assert_eq!(autofill_gain((14, 10), 0, walled, free), 0);
    }

    /// A standing wall is an overlay, not an object, so the plan counts it from
    /// the grid; and a sold cell keeps its owner there, which is not a wall.
    #[test]
    fn a_standing_wall_is_read_from_the_overlay_grid() {
        use crate::sim::overlay_grid::OverlayGrid;
        use crate::sim::world::Simulation;
        let mut sim = Simulation::new();
        let house = sim.interner.intern("Americans");
        let wall = 2;
        let mut grid = OverlayGrid::new(4, 4);
        grid.place_owned_wall(2, 1, wall, 0, house);
        sim.overlay_grid = Some(grid);
        let obs = Observation::new(&sim, house);
        assert_eq!(obs.wall_owner_at(2, 1), Some(house));
        assert_eq!(obs.wall_owner_at(0, 0), None, "an empty cell");

        // A sale clears the overlay and leaves the owner behind for the grid's
        // retained neighbour counts.
        sim.overlay_grid
            .as_mut()
            .unwrap()
            .cell_mut_for_test(2, 1)
            .overlay_id = None;
        let obs = Observation::new(&sim, house);
        assert_eq!(
            sim.overlay_grid.as_ref().unwrap().cell(2, 1).wall_owner,
            Some(house),
            "the grid keeps the last owner of a sold wall"
        );
        assert_eq!(obs.wall_owner_at(2, 1), None, "a sold wall");
    }

    /// The key is the retail one: a building the data marks is walled, and one
    /// it does not is not, whatever its name.
    #[test]
    fn protected_comes_from_the_data() {
        let rules = match crate::rules::retail_ini_fixture::retail_unlayered_rules() {
            Some(rules) => rules,
            None => return,
        };
        let yard = rules.object("GACNST").expect("the Construction Yard");
        assert!(protected(yard), "retail marks the yard ProtectWithWall=yes");
        let refinery = rules.object("GAREFN").expect("a refinery");
        assert!(
            !protected(refinery),
            "retail does not mark the refinery, so it is not walled"
        );
        let wall = rules.object("GAWALL").expect("the Allied wall");
        assert!(wall.wall, "GAWALL is a wall segment");
    }
}
