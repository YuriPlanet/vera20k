//! Unit740B10 -> Foot Mission_Patrol4D4280, also inherited by Infantry.
//! The post is a retained CellClass pointer, independent of ArchiveTarget.
//! The existing scan, range, Approach, concrete setters and idle receivers own
//! their side effects; MissionClass owns the returned delay's timer write.

use super::{ObjectAiCtx, Simulation, target_scan};
use crate::map::cell_index::NativeCellIdentity;
use crate::map::entities::EntityCategory;
use crate::map::resolved_terrain::NativeCellQuery;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{ScanMission, TargetKind};
use crate::sim::components::{DriveCoord, NavTargetRef};
use crate::sim::find_nearby_cell::{NearbyArgs, NearbyFootprint, PassabilityArgs};
use crate::sim::mission::MissionType;
use crate::sim::movement::ground_pose;
use crate::sim::pathfinding::zone_map::ZoneQueryCell;

#[cfg(test)]
#[path = "foot_patrol_tests.rs"]
mod tests;

/// Foot+5DC. Constructor4D3314 stores NULL; Load4DB5DA swizzles the saved
/// Cell pointer. A mission change alone never clears it. The shared Dummy
/// remains the same allocation when later map queries change its coordinates.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct PatrolPost {
    cell: Option<NativeCellIdentity>,
}

impl PatrolPost {
    pub(crate) const fn cell(self) -> Option<NativeCellIdentity> {
        self.cell
    }
}

pub(super) fn mission_patrol(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) -> i32 {
    let actor = sim.substrate.entities.get(id).expect("Patrol receiver");
    if actor.category() == EntityCategory::Unit {
        let ticks = sim
            .object_type(actor.type_ref(), rules)
            .map_or(0, |t| t.deploy_time_ticks);
        sim.substrate
            .entities
            .get_mut(id)
            .unwrap()
            .close_door(ticks, sim.session.binary_frame());
        // Unit740B13's harvesting clear is owned by UnitAI, as for Move.
    }
    let range = threat_range(sim, id, rules);
    let limit = (range / 256).wrapping_add(6);
    //4D429D also sets the legacy Techno430 planning hint. Its sole literal
    //reader4DC8C9 belongs to the old WaypointPath producer502160, for which
    //no enabled fresh-skirmish call has been established. That separate
    //legacy planning chain remains outside this handler's represented state.
    let state = sim
        .substrate
        .entities
        .get(id)
        .unwrap()
        .mission
        .handler_state();
    match state {
        0 => {
            let point = ground_pose::object_location(
                sim.substrate.entities.get(id).unwrap(),
                sim.resolved_terrain.as_ref(),
            );
            let candidate = scan(sim, id, rules, ctx, point);
            let weapon = target_scan::select_weapon(sim, rules, id, candidate);
            if let Some(candidate) = candidate
                && qualifies(sim, id, candidate, weapon, None, limit, rules, ctx)
            {
                let mut post = sim
                    .object_navigation_cell_identity(id, rules)
                    .expect("Patrol post requires its navigation cell");
                sim.substrate.entities.get_mut(id).unwrap().patrol_post.cell = Some(post);
                if cell_bridge(sim, post) && !should_bridge(sim, id, rules) {
                    let Some(found) = bridge_post(sim, id, post, rules) else {
                        sim.substrate.entities.get_mut(id).unwrap().patrol_post.cell = None;
                        return 60;
                    };
                    post = found;
                    sim.substrate.entities.get_mut(id).unwrap().patrol_post.cell = Some(post);
                }
                let actor = sim.substrate.entities.get(id).unwrap();
                if actor.archive_target().is_none() {
                    let archive = actor
                        .navigation
                        .nav_queue()
                        .last()
                        .copied()
                        .or(actor.navigation.nav_com())
                        .map(TargetKind::from)
                        .unwrap_or_else(|| cell_target(sim, post));
                    sim.substrate
                        .entities
                        .get_mut(id)
                        .unwrap()
                        .set_archive_target(Some(archive));
                }
                destination(sim, id, None, rules, ctx);
                target(sim, id, Some(candidate), rules, ctx);
                set_state(sim, id, 1);
            } else {
                let actor = sim.substrate.entities.get(id).unwrap();
                if actor.navigation.nav_com().is_none()
                    || crate::sim::movement::motion_query::is_moving(actor) != Some(true)
                {
                    return idle(sim, id, rules, ctx);
                }
            }
        }
        1 => {
            let held = sim
                .substrate
                .entities
                .get(id)
                .unwrap()
                .attack_target
                .target();
            let object = held.filter(|target| is_object(sim, *target));
            let weapon = target_scan::select_weapon(sim, rules, id, object);
            if let Some(object) = object {
                if healthy_allied_building_for_engineer(sim, id, object, rules) {
                    target(sim, id, None, rules, ctx);
                } else if qualifies(
                    sim,
                    id,
                    object,
                    weapon,
                    Some(post(sim, id)),
                    limit,
                    rules,
                    ctx,
                ) {
                    sim.approach_target(id, rules, ctx.overlay_registry, ctx.effects);
                    if sim
                        .substrate
                        .entities
                        .get(id)
                        .unwrap()
                        .attack_target
                        .target()
                        .is_none()
                    {
                        let archive = sim.substrate.entities.get(id).unwrap().archive_target();
                        destination(sim, id, archive.map(NavTargetRef::from), rules, ctx);
                        let actor = sim.substrate.entities.get_mut(id).unwrap();
                        actor.set_archive_target(None);
                        actor.patrol_post.cell = None;
                        actor.mission.set_handler_state(0);
                        return 45;
                    }
                    return cadence(sim, id, rules);
                }
            }
            //4D465F rereads the raw pointer: an out-of-leash held target
            //does not scan, but an engineer setter which cleared it does.
            if sim
                .substrate
                .entities
                .get(id)
                .unwrap()
                .attack_target
                .target()
                .is_none()
            {
                let point = post_coords(sim, id);
                let candidate = scan(sim, id, rules, ctx, point);
                let weapon = target_scan::select_weapon(sim, rules, id, candidate);
                if let Some(candidate) = candidate
                    && qualifies(
                        sim,
                        id,
                        candidate,
                        weapon,
                        Some(post(sim, id)),
                        limit,
                        rules,
                        ctx,
                    )
                {
                    target(sim, id, Some(candidate), rules, ctx);
                    destination(sim, id, None, rules, ctx);
                    return 1;
                }
            }
            set_state(sim, id, 2);
            target(sim, id, None, rules, ctx);
            let Some(archive) = sim.substrate.entities.get(id).unwrap().archive_target() else {
                set_state(sim, id, 0);
                return idle(sim, id, rules, ctx);
            };
            let current = sim
                .object_navigation_cell_identity(id, rules)
                .expect("Patrol return navigation cell");
            let current_bridge = should_bridge(sim, id, rules);
            let terrain = sim.resolved_terrain.as_ref().expect("Patrol return map");
            let cells = NativeCellQuery::canonical(terrain);
            let archive_coord = ground_pose::target_get_coords(archive, sim, Some(&cells))
                .expect("Patrol archive exists");
            let archive_cell = cells.lookup_world(archive_coord.x, archive_coord.y);
            let post = post(sim, id);
            let post_cost = zone_cost(
                sim,
                id,
                ZoneQueryCell::Retained(current),
                ZoneQueryCell::Retained(post),
                current_bridge,
                cell_bridge(sim, post),
                rules,
            );
            let archive_cost = zone_cost(
                sim,
                id,
                ZoneQueryCell::Retained(current),
                ZoneQueryCell::Retained(archive_cell),
                current_bridge,
                cell_bridge(sim, archive_cell),
                rules,
            );
            let chosen = if archive_cost <= post_cost {
                archive
            } else {
                cell_target(sim, post)
            };
            destination(sim, id, Some(chosen.into()), rules, ctx);
            return 1;
        }
        2 => {
            let point = post_coords(sim, id);
            let candidate = scan(sim, id, rules, ctx, point);
            let weapon = target_scan::select_weapon(sim, rules, id, candidate);
            if let Some(candidate) = candidate
                && qualifies(
                    sim,
                    id,
                    candidate,
                    weapon,
                    Some(post(sim, id)),
                    limit,
                    rules,
                    ctx,
                )
            {
                target(sim, id, Some(candidate), rules, ctx);
                destination(sim, id, None, rules, ctx);
                set_state(sim, id, 1);
            } else if sim
                .substrate
                .entities
                .get(id)
                .unwrap()
                .navigation
                .nav_com()
                .is_none()
            {
                target(sim, id, None, rules, ctx);
                let Some(archive) = sim.substrate.entities.get(id).unwrap().archive_target() else {
                    set_state(sim, id, 0);
                    return idle(sim, id, rules, ctx);
                };
                destination(sim, id, Some(archive.into()), rules, ctx);
                let actor = sim.substrate.entities.get_mut(id).unwrap();
                actor.set_archive_target(None);
                actor.patrol_post.cell = None;
                actor.mission.set_handler_state(0);
            }
        }
        3 => {
            if sim
                .substrate
                .entities
                .get(id)
                .unwrap()
                .navigation
                .nav_com()
                .is_some()
            {
                set_state(sim, id, 0);
                return 1;
            }
        }
        _ => {}
    }
    cadence(sim, id, rules)
}

fn post(sim: &Simulation, id: u64) -> NativeCellIdentity {
    sim.substrate
        .entities
        .get(id)
        .unwrap()
        .patrol_post
        .cell
        .expect("Patrol states1/2 require the retained post")
}

fn post_coords(sim: &Simulation, id: u64) -> DriveCoord {
    let cells = NativeCellQuery::canonical(sim.resolved_terrain.as_ref().expect("Patrol post map"));
    crate::sim::cell_rect::native_cell_ref(&cells, post(sim, id)).get_coords()
}

fn cell_bridge(sim: &Simulation, cell: NativeCellIdentity) -> bool {
    sim.resolved_terrain
        .as_ref()
        .expect("Patrol map")
        .native_cell_flags(cell)
        & 0x100
        != 0
}

/// Convert a retained pointer without a Map query. The impossible fixed-stride
/// coordinate(-1,-1) names Dummy even when Dummy's current coordinate is real.
fn cell_target(sim: &Simulation, cell: NativeCellIdentity) -> TargetKind {
    let (x, y) = match cell {
        NativeCellIdentity::Real(_) => sim
            .resolved_terrain
            .as_ref()
            .unwrap()
            .native_cell_coord(cell),
        NativeCellIdentity::Dummy => (-1, -1),
    };
    TargetKind::Cell(x as u16, y as u16)
}

fn is_object(sim: &Simulation, target: TargetKind) -> bool {
    matches!(target, TargetKind::Entity(id) if sim.substrate.entities.contains(id) || sim.terrain_objects.objects().contains_key(&id))
}

fn scan(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
    point: DriveCoord,
) -> Option<TargetKind> {
    sim.greatest_threat_represented(
        rules,
        ctx.overlay_registry,
        id,
        ScanMission::AreaGuard,
        Some([point.x, point.y, point.z]),
    )
    .map(TargetKind::Entity)
}

fn threat_range(sim: &Simulation, id: u64, rules: &RuleSet) -> i32 {
    let actor = sim.substrate.entities.get(id).unwrap();
    let object = sim
        .object_type(actor.type_ref(), rules)
        .expect("Patrol type");
    let ranges = if object.guard_range.is_none_or(|range| range.to_bits() == 0) {
        std::array::from_fn(|index| {
            crate::sim::combat::combat_weapon::weapon_range(
                actor,
                object,
                index as i32,
                &sim.substrate.entities,
                rules,
                &sim.interner,
            )
        })
    } else {
        [0, 0]
    };
    crate::sim::combat::threat_range::threat_range_leptons(object, 2, ranges)
}

/// Virtual+BC with null requester: a tube Foot returns before either ground
/// query; otherwise Object5F6A70 samples +4C then raw Object+9C Location.
fn should_bridge(sim: &Simulation, id: u64, rules: &RuleSet) -> bool {
    let actor = sim.substrate.entities.get(id);
    if actor.is_some_and(|a| a.category().is_foot() && a.low_bridge_tube_state.is_some()) {
        return false;
    }
    let navigation = sim
        .navigation_target_coordinate(NavTargetRef::object(id), None, rules)
        .expect("Patrol bridge navigation");
    let cells =
        NativeCellQuery::canonical(sim.resolved_terrain.as_ref().expect("Patrol bridge map"));
    let current = actor.map_or_else(
        || {
            ground_pose::target_get_coords(TargetKind::Entity(id), sim, Some(&cells))
                .expect("Patrol bridge Terrain")
        },
        |actor| ground_pose::position_world_coord(&actor.position),
    );
    ground_pose::navigation_should_be_on_bridge(
        &cells,
        navigation,
        current,
        actor.is_some_and(|a| a.on_bridge()),
        false,
    )
    .expect("Patrol bridge ground")
}

#[allow(clippy::too_many_arguments)]
fn qualifies(
    sim: &Simulation,
    id: u64,
    candidate: TargetKind,
    weapon: i32,
    post: Option<NativeCellIdentity>,
    limit: i32,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) -> bool {
    if target_scan::can_fire_at(sim, rules, id, candidate, weapon, ctx.overlay_registry) {
        return true;
    }
    let TargetKind::Entity(candidate) = candidate else {
        return false;
    };
    let goal = sim
        .navigation_target_coordinate(NavTargetRef::object(candidate), None, rules)
        .expect("Patrol candidate navigation");
    let goal = ZoneQueryCell::Copied(((goal.x / 256) as i16, (goal.y / 256) as i16));
    let start = post.map_or_else(
        || {
            let own = sim
                .navigation_target_coordinate(NavTargetRef::object(id), None, rules)
                .expect("Patrol actor navigation");
            ZoneQueryCell::Copied(((own.x / 256) as i16, (own.y / 256) as i16))
        },
        ZoneQueryCell::Retained,
    );
    let goal_bridge = should_bridge(sim, candidate, rules);
    let start_bridge = post.map_or_else(
        || should_bridge(sim, id, rules),
        |post| cell_bridge(sim, post),
    );
    zone_cost(sim, id, start, goal, start_bridge, goal_bridge, rules) < limit
}

fn zone_cost(
    sim: &Simulation,
    id: u64,
    start: ZoneQueryCell,
    goal: ZoneQueryCell,
    start_bridge: bool,
    goal_bridge: bool,
    rules: &RuleSet,
) -> i32 {
    sim.estimate_zone_cost_in_query(id, start, goal, start_bridge, goal_bridge, rules)
        .expect("Patrol zone-cost topology")
}

fn bridge_post(
    sim: &Simulation,
    id: u64,
    post: NativeCellIdentity,
    rules: &RuleSet,
) -> Option<NativeCellIdentity> {
    let actor = sim.substrate.entities.get(id).unwrap();
    let object = sim
        .object_type(actor.type_ref(), rules)
        .expect("Patrol FNPC type");
    let terrain = sim.resolved_terrain.as_ref().expect("Patrol FNPC map");
    let seed = terrain.native_cell_coord(post);
    let zone = sim
        .zone_grid
        .as_ref()
        .expect("Patrol FNPC zones")
        .get_zone_id_native(
            terrain,
            (seed.0 as u16, seed.1 as u16),
            object.movement_zone,
            false,
        );
    //GetZoneID can stamp Dummy before FNPC reads the retained seed.
    let seed = terrain.native_cell_coord(post);
    let found = sim
        .nearby_passable_cell(
            (i32::from(seed.0), i32::from(seed.1)),
            NearbyArgs {
                allow_bridge_cells: false,
                check_height: true,
                ..NearbyArgs::plain(
                    PassabilityArgs {
                        speed_type: object.speed_type,
                        required_zone_id: zone,
                        movement_zone: object.movement_zone,
                        bridge_aware_zone: false,
                    },
                    NearbyFootprint::SINGLE,
                )
            },
        )
        .filter(|cell| *cell != (0, 0))?;
    Some(NativeCellQuery::canonical(terrain).lookup((found.0 as i16, found.1 as i16)))
}

fn healthy_allied_building_for_engineer(
    sim: &Simulation,
    id: u64,
    target: TargetKind,
    rules: &RuleSet,
) -> bool {
    let actor = sim.substrate.entities.get(id).unwrap();
    if actor.category() != EntityCategory::Infantry
        || !sim
            .object_type(actor.type_ref(), rules)
            .is_some_and(|t| t.engineer)
    {
        return false;
    }
    let TargetKind::Entity(target) = target else {
        return false;
    };
    let Some(building) = sim
        .substrate
        .entities
        .get(target)
        .filter(|t| t.category() == EntityCategory::Structure)
    else {
        return false;
    };
    if !crate::sim::combat::combat_weapon::is_ally_by_object(
        Some(sim.house_alliances.as_map()),
        &sim.interner,
        actor.owner(),
        building.owner(),
    ) {
        return false;
    }
    let strength = sim
        .object_type(building.type_ref(), rules)
        .expect("Patrol building type")
        .strength;
    building
        .health
        .compare_ratio(strength, rules.general.condition_red)
        == crate::util::native_x87::MaskedX87Ordering::Greater
}

fn set_state(sim: &mut Simulation, id: u64, state: u32) {
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mission
        .set_handler_state(state);
}

fn target(
    sim: &mut Simulation,
    id: u64,
    target: Option<TargetKind>,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) {
    let _ = sim.assign_target_represented(id, target, rules, ctx.overlay_registry, ctx.effects);
}

fn destination(
    sim: &mut Simulation,
    id: u64,
    target: Option<NavTargetRef>,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) {
    let _ =
        sim.assign_destination_represented(id, target, rules, ctx.overlay_registry, ctx.effects);
}

fn idle(sim: &mut Simulation, id: u64, rules: &RuleSet, ctx: ObjectAiCtx<'_>) -> i32 {
    crate::sim::world::enter_idle_mode(sim, id, rules, ctx.overlay_registry, ctx.effects);
    set_state(sim, id, 3);
    1
}

fn cadence(sim: &mut Simulation, id: u64, rules: &RuleSet) -> i32 {
    Simulation::mission_rate_epilogue_for(
        &mut sim.scenario_rng,
        &sim.substrate,
        rules,
        id,
        MissionType::Patrol,
    )
}
