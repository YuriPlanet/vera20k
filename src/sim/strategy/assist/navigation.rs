//! V36 bounded terrain-route recovery for *demonstrably stalled* native walks.
//!
//! This is not a replacement for the native locomotor/pathfinder. The engine
//! owns terrain movement and the original strategic goal; the assist brain
//! computes a short, reachable *intermediate* post only after the real walker
//! made no progress. The local route uses the existing ground projection and
//! the unit's movement zone. It never creates teleportation or alters world
//! position. It is bounded in nodes, calls and issued orders so 120-player
//! matches do not run per-unit A* each frame.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::rules::locomotor_type::MovementZone;
use crate::rules::object_type::ObjectCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::control::observation::Observation;

use super::advance::{cell_distance, weapon_range_cells};

const MAX_MAP_CELLS: usize = 256 * 256;
const MAX_EXPANSIONS: usize = 4_096;
const WAYPOINT_STEPS: usize = 9;

/// The ground projection is not the native flight, ship or amphibious path
/// grid. Never invent a land route for units with a different locomotor row.
fn ground_route_zone(zone: MovementZone) -> bool {
    matches!(zone,
        MovementZone::Normal | MovementZone::Crusher | MovementZone::Destroyer
        | MovementZone::Subterranean | MovementZone::Infantry
        | MovementZone::InfantryDestroyer | MovementZone::CrusherAll)
}

const NEIGHBORS: [(i32,i32,i32);8] = [
    (-1,0,10),(1,0,10),(0,-1,10),(0,1,10),
    (-1,-1,14),(1,-1,14),(-1,1,14),(1,1,14),
];

fn diagonal_cost(a:(u16,u16), b:(u16,u16)) -> i32 {
    let dx = i32::from(a.0.abs_diff(b.0));
    let dy = i32::from(a.1.abs_diff(b.1));
    dx.max(dy).saturating_mul(10).saturating_add(dx.min(dy).saturating_mul(4))
}

/// Pure shortest route, in actual grid cells, with diagonal corner-cutting
/// forbidden. A caller can provide any terrain passability; production uses
/// `Observation::is_ground_walkable` and `can_reach` on this same ground layer.
fn route<F>(extent:(u16,u16), start:(u16,u16), end:(u16,u16),
    mut passable:F,
) -> Option<Vec<(u16,u16)>>
where F:FnMut((u16,u16))->bool {
    let width = usize::from(extent.0);
    let height = usize::from(extent.1);
    let area = width.checked_mul(height)?;
    if width == 0 || height == 0 || area > MAX_MAP_CELLS
        || usize::from(start.0) >= width || usize::from(start.1) >= height
        || usize::from(end.0) >= width || usize::from(end.1) >= height
    { return None; }
    let index = |cell:(u16,u16)| usize::from(cell.1)*width + usize::from(cell.0);
    let from = index(start);
    let goal = index(end);
    if from == goal { return Some(vec![start]); }
    if !passable(end) { return None; }
    let mut score = vec![i32::MAX;area];
    let mut parent = vec![usize::MAX;area];
    let mut queue = BinaryHeap::new();
    score[from] = 0;
    queue.push((Reverse(diagonal_cost(start,end)),Reverse(0i32),Reverse(from)));
    let mut expanded = 0;
    while let Some((_,Reverse(cost),Reverse(current))) = queue.pop() {
        if cost != score[current] { continue; }
        if current == goal {
            let mut cells = Vec::new();
            let mut at = current;
            loop {
                cells.push(((at % width) as u16,(at / width) as u16));
                if at == from { break; }
                at = parent[at];
                if at == usize::MAX { return None; }
            }
            cells.reverse();
            return Some(cells);
        }
        expanded += 1;
        if expanded >= MAX_EXPANSIONS { return None; }
        let x = (current % width) as i32;
        let y = (current / width) as i32;
        for (dx,dy,move_cost) in NEIGHBORS {
            let nx = x + dx;
            let ny = y + dy;
            if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                continue;
            }
            let next = (nx as u16,ny as u16);
            if !passable(next) { continue; }
            if dx != 0 && dy != 0 {
                // A diagonal between two obstructing building corners is not
                // a passable corridor, no matter what the diagonal cell says.
                if !passable(((x+dx) as u16,y as u16))
                    || !passable((x as u16,(y+dy) as u16))
                { continue; }
            }
            let n = index(next);
            let candidate = cost.saturating_add(move_cost);
            if candidate >= score[n] { continue; }
            score[n] = candidate;
            parent[n] = current;
            queue.push((Reverse(candidate.saturating_add(diagonal_cost(next,end))),
                Reverse(candidate),Reverse(n)));
        }
    }
    None
}

/// The last cell of a short native-followable portion of the path, **not**
/// the global destination itself. A single native walk to this post lets the
/// engine run its own locomotor rules, then the original strategic mission
/// will be restored by the mission ledger. Long excursions are intentionally
/// broken at bends; a ten-cell waypoint is an escape from a cliff corner,
/// not a command to follow 100 private grid steps.
fn next_post(path:&[(u16,u16)]) -> Option<(u16,u16)> {
    if path.len() < 3 { return None; }
    let end = (path.len()-1).min(WAYPOINT_STEPS);
    // Prefer a post just past the first significant bend. With no bend, the
    // 9-cell intermediate goal is still shorter than the stuck long walk.
    let mut turn = None;
    for i in 2..=end {
        let dx0 = i32::from(path[i-1].0)-i32::from(path[i-2].0);
        let dy0 = i32::from(path[i-1].1)-i32::from(path[i-2].1);
        let dx1 = i32::from(path[i].0)-i32::from(path[i-1].0);
        let dy1 = i32::from(path[i].1)-i32::from(path[i-1].1);
        if dx0 != dx1 || dy0 != dy1 { turn = Some(i); break; }
    }
    Some(path[turn.map_or(end, |i| (i+2).min(end))])
}

/// A target building occupies its own cell. When native has handed the brain
/// a movement-to-object destination inside that footprint, choose a nearby
/// *passable* arrival cell instead of asking A* to walk onto the building.
fn approachable_goal(
    obs:&Observation<'_>, zone:MovementZone, start:(u16,u16),
    goal:(u16,u16),
) -> Option<(u16,u16)> {
    if obs.is_ground_walkable(goal.0,goal.1)
        && obs.can_reach(zone,start,goal) { return Some(goal); }
    let mut best = None;
    for radius in 1i32..=4 {
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx.abs().max(dy.abs()) != radius { continue; }
                let (Ok(x),Ok(y)) = (
                    u16::try_from(i32::from(goal.0)+dx),
                    u16::try_from(i32::from(goal.1)+dy),
                ) else { continue; };
                let point=(x,y);
                if !obs.is_cell_in_playfield(x,y)
                    || !obs.is_ground_walkable(x,y)
                    || !obs.can_reach(zone,start,point) { continue; }
                let key=(radius,cell_distance(start,point),point);
                if best.is_none_or(|(old,_)|key<old) { best=Some((key,point)); }
            }
        }
        if best.is_some() { break; }
    }
    best.map(|(_,cell)|cell)
}

pub(super) fn recovery_post(rules:&RuleSet, obs:&Observation<'_>,
    id:u64, zone:MovementZone, start:(u16,u16), goal:(u16,u16),
) -> Option<(u16,u16)> {
    if !ground_route_zone(zone) || !obs.has_ground_grid() || !obs.has_zone_grid() {
        return None;
    }
    let goal = approachable_goal(obs,zone,start,goal)?;
    if cell_distance(start,goal) < 5 { return None; }
    // Only presently visible dangers can be considered. Threat geometry is
    // consulted as a *path safety gate*, never to reveal hidden enemy units.
    let threats: Vec<_> = obs.visible_enemy_entities().take(96).filter_map(|enemy| {
        let ty=obs.object_type(rules,enemy.type_id)?;
        let radius=if ty.is_base_defense
            && obs.visible_enemy_building_operational(rules,enemy.entity_id)
        { weapon_range_cells(rules,ty,false).clamp(4,16)+2 }
        else if ty.category == ObjectCategory::Building { 2 }
        else { 4 };
        Some((enemy.cell,radius))
    }).collect();
    let traversable = |cell:(u16,u16)| {
        (cell==start || (obs.is_cell_in_playfield(cell.0,cell.1)
            && obs.is_ground_walkable(cell.0,cell.1)
            && obs.can_reach(zone,start,cell)))
            && !threats.iter().any(|(enemy,radius)|
                cell != start && cell_distance(cell,*enemy) <= *radius)
    };
    let path=route(obs.map_extent(),start,goal,traversable)?;
    let post=next_post(&path)?;
    (cell_distance(start,post) >= 2
        && super::orders::reachable(rules,obs,id,post)
        && !super::tactics::unsafe_post(rules,obs,id,post))
        .then_some(post)
}

#[cfg(all(test, tests_sim_p_s))]
mod tests {
    use super::*;

    #[test]
    fn route_goes_around_a_cliff_instead_of_shuffling_at_the_wall() {
        // The long barrier has a real pass at y=13. The direct route from
        // (3,4) to (18,4) is blocked, but a terrain route must find the gap.
        let blocked=|(x,y):(u16,u16)| x==10 && y<13;
        let path=route((24,20),(3,4),(18,4),|cell|!blocked(cell))
            .expect("pass around the cliff");
        assert!(path.len()>16);
        assert!(path.iter().any(|cell|cell.1>=13));
        assert!(path.iter().all(|cell|!blocked(*cell)));
        assert!(next_post(&path).is_some());
    }

    #[test]
    fn sealed_ridge_cannot_produce_an_invented_waypoint() {
        let path=route((20,20),(2,10),(17,10),|(x,_)|x!=10);
        assert!(path.is_none());
    }

    #[test]
    fn diagonal_crack_between_two_buildings_is_not_a_path() {
        let path=route((2,2),(0,0),(1,1),|cell|
            !matches!(cell,(1,0)|(0,1)));
        assert!(path.is_none());
    }

    #[test]
    fn route_uses_only_the_land_locomotor_projection() {
        assert!(ground_route_zone(MovementZone::Infantry));
        assert!(ground_route_zone(MovementZone::Crusher));
        assert!(!ground_route_zone(MovementZone::Fly));
        assert!(!ground_route_zone(MovementZone::Water));
        assert!(!ground_route_zone(MovementZone::Amphibious));
    }

    #[test]
    fn route_rejects_unbounded_or_malformed_maps() {
        assert!(route((600,600),(1,1),(2,2),|_|true).is_none());
        assert!(route((10,10),(99,99),(2,2),|_|true).is_none());
    }
}
