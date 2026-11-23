//! Unit741970's row-six destination handling and Unit7447B0's route planner.
//! The planner writes the existing NavQueue, then returns and removes its first
//! target. Its map queries share the canonical Dummy; it owns no route state,
//! movement timers, marks or RNG draws.

use super::{air_movement, foot_coordinate, foot_path, ground_pose, motion_query};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::ruleset::RuleSet;
use crate::sim::cell_rect::{
    BurrowingContents, cell_allows_burrowing, cell_is_in_playfield_height_aware_in_query,
};
use crate::sim::components::NavTargetRef;
use crate::sim::entity_store::EntityStore;
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::pathfinding::zone_build::{
    find_high_bridge_record_index, native_packed_cell_distance,
};
use crate::sim::pathfinding::zone_map::ZoneQueryCell;
use crate::sim::pathfinding::zone_search::native_path_entry::resolve_bridge_endpoint;
use crate::sim::world::Simulation;

/// Signed packed cell words, copied from a virtual coordinate / 256 toward zero.
type CellCoord = (i16, i16);

fn unsigned_cell((x, y): CellCoord) -> (u16, u16) {
    (x as u16, y as u16)
}

fn source_cell(
    sim: &Simulation,
    cells: &NativeCellQuery<'_>,
    id: u64,
) -> Result<CellCoord, String> {
    let actor = sim
        .substrate
        .entities
        .get(id)
        .ok_or("retired burrowing Unit")?;
    let point = foot_coordinate::navigation_coordinate(actor, Some(cells.terrain()))?;
    Ok(((point.x / 256) as i16, (point.y / 256) as i16))
}

fn target_cell(
    sim: &Simulation,
    cells: &NativeCellQuery<'_>,
    target: NavTargetRef,
) -> Result<CellCoord, String> {
    let point = ground_pose::target_get_coords(target.into(), sim, Some(cells))
        .ok_or("burrowing destination object disappeared")?;
    Ok(((point.x / 256) as i16, (point.y / 256) as i16))
}

fn height(sim: &Simulation, cells: &NativeCellQuery<'_>, id: u64) -> Result<i32, String> {
    let actor = sim
        .substrate
        .entities
        .get(id)
        .ok_or("retired burrowing Unit")?;
    Ok(air_movement::queried_flight_height(actor, Some(cells)))
}

fn zone_id(
    sim: &Simulation,
    cells: &NativeCellQuery<'_>,
    cell: CellCoord,
    movement: MovementZone,
    bridge: bool,
) -> Result<u32, String> {
    sim.zone_grid
        .as_ref()
        .ok_or("burrowing route requires zone topology")?
        .get_zone_id_native_in_query(
            cells.terrain(),
            unsigned_cell(cell),
            movement,
            bridge,
            Some(cells),
        )
        .ok_or_else(|| "burrowing route requires the native movement row".into())
}

/// Every Unit7447B0 Map56D100 call uses Normal and a false fringe flag.
fn normal_reachable(
    sim: &Simulation,
    cells: &NativeCellQuery<'_>,
    from: CellCoord,
    to: CellCoord,
    from_bridge: bool,
    to_bridge: bool,
) -> Result<bool, String> {
    let bounds = sim
        .playfield_bounds()
        .ok_or("burrowing route requires playfield bounds")?;
    let size = sim
        .map_size_diamond()
        .ok_or("burrowing route requires Map Size")?;
    sim.zone_grid
        .as_ref()
        .ok_or("burrowing route requires zone topology")?
        .can_reach_native(
            cells,
            ZoneQueryCell::Copied(from),
            ZoneQueryCell::Copied(to),
            MovementZone::Normal,
            from_bridge,
            to_bridge,
            false,
            bounds,
            size,
        )
        .ok_or_else(|| "burrowing route requires the native movement row".into())
}

/// Unit746000 is a routing decision, not an A* call. Height and normal-zone
/// reachability precede even equal/adjacent cells. Larger local routes use the
/// existing EstimateZoneCost42D170 owner with the explicit Normal override.
fn needs_burrowing_route(
    sim: &Simulation,
    cells: &NativeCellQuery<'_>,
    id: u64,
    from: CellCoord,
    to: CellCoord,
    rules: &RuleSet,
) -> Result<bool, String> {
    if height(sim, cells, id)? < 0 || !normal_reachable(sim, cells, from, to, false, false)? {
        return Ok(true);
    }
    let distance = foot_path::chebyshev(from, to);
    if distance <= 1 {
        return Ok(false);
    }
    if distance >= 12 {
        return Ok(true);
    }
    for cell in [from, to] {
        if !cell_is_in_playfield_height_aware_in_query(
            (i32::from(cell.0), i32::from(cell.1)),
            sim.playfield_bounds(),
            Some(cells.terrain()),
            Some(cells),
        ) {
            return Ok(true);
        }
    }
    let to_bridge = cells.flags(cells.lookup(to)) & 0x100 != 0;
    Ok(sim.estimate_zone_cost_in_query(
        id,
        ZoneQueryCell::Copied(from),
        ZoneQueryCell::Copied(to),
        false,
        to_bridge,
        rules,
        Some(MovementZone::Normal),
    )? > 15)
}

fn bridge_endpoint(
    sim: &Simulation,
    cells: &NativeCellQuery<'_>,
    at: CellCoord,
    reference: CellCoord,
) -> Result<CellCoord, String> {
    let records = sim
        .zone_grid
        .as_ref()
        .ok_or("burrowing route requires bridge records")?;
    let result = resolve_bridge_endpoint(
        cells,
        records.bridge_records(),
        ZoneQueryCell::Copied(at),
        ZoneQueryCell::Copied(reference),
        sim.playfield_bounds(),
    )?;
    Ok((result.0 as i16, result.1 as i16))
}

/// Unit7447B0's FNPC calls all use Track, Normal, 1x1, allowBridge, zero
/// closeTo and no overlay/occupancy check. Only these three flags and the zone
/// vary. The existing owner supplies frame selection and shared Dummy writes.
fn nearby(
    sim: &Simulation,
    seed: CellCoord,
    zone: Option<u32>,
    bridge_aware: bool,
    check_height: bool,
    require_burrowable: bool,
) -> Option<CellCoord> {
    sim.nearby_passable_cell(
        (i32::from(seed.0), i32::from(seed.1)),
        NearbyArgs {
            check_height,
            require_burrowable,
            ..NearbyArgs::plain(
                PassabilityArgs {
                    speed_type: SpeedType::Track,
                    required_zone_id: zone,
                    movement_zone: MovementZone::Normal,
                    bridge_aware_zone: bridge_aware,
                },
                NearbyFootprint::SINGLE,
            )
        },
    )
    .filter(|&cell| cell != (0, 0))
    .map(|(x, y)| (x as i16, y as i16))
}

fn nearby_burrowable_in_zone(
    sim: &Simulation,
    cells: &NativeCellQuery<'_>,
    seed: CellCoord,
    zone_source: CellCoord,
) -> Result<Option<CellCoord>, String> {
    let zone = zone_id(sim, cells, zone_source, MovementZone::Normal, false)?;
    Ok(nearby(sim, seed, Some(zone), false, true, true))
}

#[derive(Clone, Copy)]
enum NormalFallback {
    Bridge,
    GroundWithHeight,
    Ground,
}

/// The N3/N6/N7 fallbacks require a nonempty FNPC answer, then a fresh
/// GetHeight>=0 check (744CA7,74551F,745738 and equivalent return arms).
fn normal_fallback(
    sim: &Simulation,
    cells: &NativeCellQuery<'_>,
    id: u64,
    from: CellCoord,
    to: CellCoord,
    fallback: NormalFallback,
) -> Result<Option<CellCoord>, String> {
    let bridge = matches!(fallback, NormalFallback::Bridge);
    let check_height = !matches!(fallback, NormalFallback::Ground);
    let zone = zone_id(sim, cells, from, MovementZone::Normal, bridge)?;
    let found = nearby(sim, to, Some(zone), bridge, check_height, false);
    if found.is_some() && height(sim, cells, id)? >= 0 {
        Ok(found)
    } else {
        Ok(None)
    }
}

#[derive(Clone, Copy)]
enum RouteTarget {
    Original(NavTargetRef),
    Cell(CellCoord),
}

/// Native queue commit points: each intermediate is looked up and appended
/// before the next lookup, then the final pointer is appended.744C07 removes
/// and returns the first entry. Do not deduplicate or defer these mutations.
fn commit_route(
    entities: &mut EntityStore,
    id: u64,
    cells: &NativeCellQuery<'_>,
    intermediates: &[CellCoord],
    target: RouteTarget,
) -> Result<Option<NavTargetRef>, String> {
    let navigation = &mut entities
        .get_mut(id)
        .ok_or("retired burrowing Unit")?
        .navigation;
    for &coord in intermediates {
        let (x, y) = cells.coord(cells.lookup(coord));
        navigation.append_nav_queue(NavTargetRef::cell(x as u16, y as u16));
    }
    let target = match target {
        RouteTarget::Original(target) => target,
        RouteTarget::Cell(coord) => {
            let (x, y) = cells.coord(cells.lookup(coord));
            NavTargetRef::cell(x as u16, y as u16)
        }
    };
    navigation.append_nav_queue(target);
    Ok(navigation.pop_nav_queue_front())
}

impl Simulation {
    /// CellClass::PassesTileAndOccupancyChecks486FF0 on a live Cell with the
    /// game's building and terrain-object lists: the burrowing setter, the
    /// planner and the Tunnel locomotor ask it.
    pub(crate) fn cell_allows_burrowing(
        &self,
        cells: &NativeCellQuery<'_>,
        cell: NativeCellIdentity,
    ) -> bool {
        cell_allows_burrowing(
            cells,
            cell,
            self.playfield_bounds(),
            Some(BurrowingContents {
                occupancy: &self.substrate.occupancy,
                terrain_objects: self.terrain_objects.cell_index(),
            }),
        )
    }

    /// Unit741E8E..7422D7, before the ordinary queue clear/Foot setter tail.
    /// Both of its exits reach742314's queue-prepend arm, which only a
    /// surfacing Tunnel enters (`Simulation::set_unit_destination`).
    pub(super) fn burrowing_setter_destination(
        &mut self,
        id: u64,
        destination: Option<NavTargetRef>,
        clear_queue: bool,
        rules: &RuleSet,
    ) -> Result<(Option<NavTargetRef>, bool), String> {
        let Some(requested) = destination else {
            return Ok((destination, clear_queue));
        };
        let actor = self
            .substrate
            .entities
            .get(id)
            .ok_or("retired burrowing Unit")?;
        let movement = self
            .entity_object_type(actor, rules)
            .ok_or("burrowing Unit type disappeared")?
            .movement_zone;
        if movement != MovementZone::Subterranean {
            return Ok((destination, clear_queue));
        }
        let moving = motion_query::is_moving(actor).ok_or("burrowing Unit has no locomotor")?;
        let drive = actor
            .locomotor
            .as_ref()
            .is_some_and(|loco| loco.active_kind() == LocomotorKind::Drive);
        if moving && drive {
            //741F73..741FB6 preserves the loop flag and every existing post.
            let navigation = &mut self
                .substrate
                .entities
                .get_mut(id)
                .ok_or("retired burrowing Unit")?
                .navigation;
            navigation.prepend_nav_list(requested);
            navigation.clear_nav_queue();
            let cells = NativeCellQuery::canonical(
                self.resolved_terrain
                    .as_ref()
                    .ok_or("burrowing retask requires map cells")?,
            );
            let mut current = ground_pose::entity_navigation_cell_identity(
                self.substrate
                    .entities
                    .get(id)
                    .ok_or("retired burrowing Unit")?,
                &cells,
                Some((rules, &self.interner)),
            )?;
            if cells.flags(current) & 0x100 != 0 && !self.foot_should_be_on_bridge(id)? {
                //742023 retains Cell+24 across GetZoneID, then rereads it as
                // FNPC's seed. This one call uses the type's row, not Normal.
                let zone = zone_id(self, &cells, cells.coord(current), movement, false)?;
                let seed = cells.coord(current);
                if let Some(found) = self
                    .nearby_passable_cell(
                        (i32::from(seed.0), i32::from(seed.1)),
                        NearbyArgs::plain(
                            PassabilityArgs {
                                speed_type: SpeedType::Track,
                                required_zone_id: Some(zone),
                                movement_zone: movement,
                                bridge_aware_zone: false,
                            },
                            NearbyFootprint::SINGLE,
                        ),
                    )
                    .filter(|&cell| cell != (0, 0))
                {
                    current = cells.lookup((found.0 as i16, found.1 as i16));
                }
            }
            let (x, y) = cells.coord(current);
            return Ok((Some(NavTargetRef::cell(x as u16, y as u16)), false));
        }
        if !clear_queue {
            return Ok((destination, false));
        }
        let needs_plan = {
            let cells = NativeCellQuery::canonical(
                self.resolved_terrain
                    .as_ref()
                    .ok_or("burrowing setter requires map cells")?,
            );
            let on_bridge = self.foot_should_be_on_bridge(id)?;
            let from = source_cell(self, &cells, id)?;
            let from_burrowable = self.cell_allows_burrowing(&cells, cells.lookup(from));
            let to = target_cell(self, &cells, requested)?;
            let target_cell = cells.lookup(to);
            let target_bridge = cells.flags(target_cell) & 0x100 != 0;
            let to_burrowable = self.cell_allows_burrowing(&cells, target_cell);
            let actor_height = height(self, &cells, id)?; //742194 is unconditional.
            if on_bridge && target_bridge {
                //7421C0/7421D5 use tolerance3; the inactive same-record
                // exception measures E(A,A) against E(D,D), not E(A,D).
                let records = self
                    .zone_grid
                    .as_ref()
                    .ok_or("burrowing setter requires bridge records")?
                    .bridge_records();
                let source_record =
                    find_high_bridge_record_index(records, 0, unsigned_cell(from), 3);
                let target_record = find_high_bridge_record_index(records, 0, unsigned_cell(to), 3);
                let same_bridge = if let Some(index) =
                    source_record.filter(|&index| Some(index) == target_record)
                {
                    records[index].active || {
                        let a = bridge_endpoint(self, &cells, from, from)?;
                        let b = bridge_endpoint(self, &cells, to, to)?;
                        native_packed_cell_distance(unsigned_cell(a), unsigned_cell(b)) <= 2
                    }
                } else {
                    false
                };
                !same_bridge
            } else if on_bridge || target_bridge {
                true
            } else {
                !((from_burrowable || actor_height < 0) && to_burrowable)
            }
        };
        if needs_plan {
            //7422D0 bypasses the ordinary clear:7447B0 already owns the queue.
            Ok((
                self.burrowing_plan_destination(id, requested, rules)?,
                false,
            ))
        } else {
            Ok((destination, true))
        }
    }

    /// Original Unit::Find_Path7447B0, all nine source/target cases. Its
    /// initial clear persists on a null return or an unavailable dependency.
    fn burrowing_plan_destination(
        &mut self,
        id: u64,
        requested: NavTargetRef,
        rules: &RuleSet,
    ) -> Result<Option<NavTargetRef>, String> {
        self.substrate
            .entities
            .get_mut(id)
            .ok_or("retired burrowing Unit")?
            .navigation
            .clear_nav_queue();
        let cells = NativeCellQuery::canonical(
            self.resolved_terrain
                .as_ref()
                .ok_or("burrowing planner requires map cells")?,
        );
        let source_bridge = self.foot_should_be_on_bridge(id)?;
        let from = source_cell(self, &cells, id)?;
        let from_burrowable = self.cell_allows_burrowing(&cells, cells.lookup(from));
        //744835 asks GetHeight only if the source predicate failed.
        let underground = from_burrowable || height(self, &cells, id)? < 0;
        let to = target_cell(self, &cells, requested)?;
        let target_cell = cells.lookup(to);
        let target_bridge = cells.flags(target_cell) & 0x100 != 0;
        let to_burrowable = self.cell_allows_burrowing(&cells, target_cell);
        let original = RouteTarget::Original(requested);
        let fallback = match (source_bridge, underground, target_bridge, to_burrowable) {
            //7448CA: bridge to bridge. The second N1 uses EA's zone.
            (true, _, true, _) => {
                let ea = bridge_endpoint(self, &cells, from, to)?;
                let ed = bridge_endpoint(self, &cells, to, to)?;
                let endpoints = [ea, ed];
                if !needs_burrowing_route(self, &cells, id, ea, ed, rules)? {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &endpoints[usize::from(ea == from)..],
                        original,
                    );
                }
                let q1 = nearby_burrowable_in_zone(self, &cells, ea, ea)?;
                let q2 = nearby_burrowable_in_zone(self, &cells, ed, ea)?;
                if let (Some(q1), Some(q2)) = (q1, q2) {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &[q1, q2],
                        original,
                    );
                }
                if normal_reachable(self, &cells, from, to, true, true)? {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &endpoints[usize::from(ea == from)..],
                        original,
                    );
                }
                if let Some(q1) = q1
                    && let Some(q) = nearby(self, to, None, true, false, true).filter(|q| *q != q1)
                {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &[q1],
                        RouteTarget::Cell(q),
                    );
                }
                NormalFallback::Bridge
            }
            //744DDC: bridge to a burrowable ground destination.
            (true, _, false, true) => {
                let ea = bridge_endpoint(self, &cells, from, to)?;
                let endpoints = [ea];
                if !needs_burrowing_route(self, &cells, id, ea, to, rules)? {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &endpoints[usize::from(ea == from)..],
                        original,
                    );
                }
                if let Some(q) = nearby_burrowable_in_zone(self, &cells, ea, ea)? {
                    return commit_route(&mut self.substrate.entities, id, &cells, &[q], original);
                }
                if normal_reachable(self, &cells, from, to, true, false)? {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &endpoints[usize::from(ea == from)..],
                        original,
                    );
                }
                NormalFallback::Bridge
            }
            //744FAA: bridge to nonburrowable ground; N4 retains height checks.
            (true, _, false, false) => {
                let ea = bridge_endpoint(self, &cells, from, to)?;
                let endpoints = [ea];
                if !needs_burrowing_route(self, &cells, id, ea, to, rules)? {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &endpoints[usize::from(ea == from)..],
                        original,
                    );
                }
                let q1 = nearby_burrowable_in_zone(self, &cells, ea, ea)?;
                let q2 = nearby_burrowable_in_zone(self, &cells, to, to)?;
                if let (Some(q1), Some(q2)) = (q1, q2) {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &[q1, q2],
                        original,
                    );
                }
                if normal_reachable(self, &cells, from, to, true, false)? {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &endpoints[usize::from(ea == from)..],
                        original,
                    );
                }
                if let Some(q1) = q1
                    && let Some(q) = nearby(self, to, None, true, true, true).filter(|q| *q != q1)
                {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &[q1],
                        RouteTarget::Cell(q),
                    );
                }
                NormalFallback::Bridge
            }
            //745364: burrowable/underground source to bridge.
            (false, true, true, _) => {
                let ed = bridge_endpoint(self, &cells, to, to)?;
                let endpoints = [ed];
                if !needs_burrowing_route(self, &cells, id, from, ed, rules)? {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &endpoints[usize::from(ed == from)..],
                        original,
                    );
                }
                if let Some(q) = nearby_burrowable_in_zone(self, &cells, ed, ed)? {
                    return commit_route(&mut self.substrate.entities, id, &cells, &[q], original);
                }
                if normal_reachable(self, &cells, from, to, false, true)? {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &endpoints[usize::from(ed == from)..],
                        original,
                    );
                }
                if let Some(q) = nearby(self, to, None, false, false, true) {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &[],
                        RouteTarget::Cell(q),
                    );
                }
                NormalFallback::GroundWithHeight
            }
            //745590: both ground cells already admit burrowing.
            (false, true, false, true) => {
                return commit_route(&mut self.substrate.entities, id, &cells, &[], original);
            }
            //745598: leaving burrowable ground for an unburrowable destination.
            (false, true, false, false) => {
                if !needs_burrowing_route(self, &cells, id, from, to, rules)? {
                    return commit_route(&mut self.substrate.entities, id, &cells, &[], original);
                }
                if let Some(q) = nearby_burrowable_in_zone(self, &cells, to, to)? {
                    return commit_route(&mut self.substrate.entities, id, &cells, &[q], original);
                }
                if normal_reachable(self, &cells, from, to, true, true)? {
                    return commit_route(&mut self.substrate.entities, id, &cells, &[], original);
                }
                if let Some(q) = nearby(self, to, None, false, false, true) {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &[],
                        RouteTarget::Cell(q),
                    );
                }
                NormalFallback::Ground
            }
            //745768: nonburrowable ground source to bridge.
            (false, false, true, _) => {
                let ed = bridge_endpoint(self, &cells, to, to)?;
                let endpoints = [ed];
                if !needs_burrowing_route(self, &cells, id, from, ed, rules)? {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &endpoints[usize::from(ed == from)..],
                        original,
                    );
                }
                let q1 = nearby_burrowable_in_zone(self, &cells, from, from)?;
                let q2 = nearby_burrowable_in_zone(self, &cells, ed, ed)?;
                if let (Some(q1), Some(q2)) = (q1, q2) {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &[q1, q2],
                        original,
                    );
                }
                if normal_reachable(self, &cells, from, to, false, true)? {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &endpoints[usize::from(ed == from)..],
                        original,
                    );
                }
                if let Some(q1) = q1
                    && let Some(q) = nearby(self, to, None, false, false, true).filter(|q| *q != q1)
                {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &[q1],
                        RouteTarget::Cell(q),
                    );
                }
                NormalFallback::Ground
            }
            //745B1B: unburrowable source, burrowable ground destination.
            (false, false, false, true) => {
                if !needs_burrowing_route(self, &cells, id, from, to, rules)? {
                    return commit_route(&mut self.substrate.entities, id, &cells, &[], original);
                }
                if let Some(q) = nearby_burrowable_in_zone(self, &cells, from, from)? {
                    return commit_route(&mut self.substrate.entities, id, &cells, &[q], original);
                }
                if normal_reachable(self, &cells, from, to, false, false)? {
                    return commit_route(&mut self.substrate.entities, id, &cells, &[], original);
                }
                NormalFallback::Ground
            }
            //745C54: both ground cells refuse burrowing.
            (false, false, false, false) => {
                if !needs_burrowing_route(self, &cells, id, from, to, rules)? {
                    return commit_route(&mut self.substrate.entities, id, &cells, &[], original);
                }
                let q1 = nearby_burrowable_in_zone(self, &cells, from, from)?;
                let q2 = nearby_burrowable_in_zone(self, &cells, to, to)?;
                if let (Some(q1), Some(q2)) = (q1, q2) {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &[q1, q2],
                        original,
                    );
                }
                if normal_reachable(self, &cells, from, to, true, true)? {
                    return commit_route(&mut self.substrate.entities, id, &cells, &[], original);
                }
                if let Some(q1) = q1
                    && let Some(q) = nearby(self, to, None, false, false, true).filter(|q| *q != q1)
                {
                    return commit_route(
                        &mut self.substrate.entities,
                        id,
                        &cells,
                        &[q1],
                        RouteTarget::Cell(q),
                    );
                }
                NormalFallback::Ground
            }
        };
        match normal_fallback(self, &cells, id, from, to, fallback)? {
            Some(q) => commit_route(
                &mut self.substrate.entities,
                id,
                &cells,
                &[],
                RouteTarget::Cell(q),
            ),
            None => Ok(None), //745FEB: the initial queue clear remains in force.
        }
    }
}
