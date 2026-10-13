//! Ordinary Unit DeploysInto continuation. Native identities: EventClass event 9
//! 0x004C77EA..0x004C7812, UnitClass::Deploy 0x007393C0, Mission_Unload
//! 0x0073D630, and the computer house's own route to a base: UnitClass::AI's
//! Hunt queue (0x007363DE..0x0073645B), Mission_Hunt 0x0073EFC0 with
//! TryToDeploy 0x00738D30, and Mission_Guard's Unload arm (0x007408C7..).
//! See tools/mcv_deploy_oracle.py for executable boundary evidence.
//! MissionCom owns scheduling; the entity flag represents runtime Unit+0x68C.

use crate::map::entities::EntityCategory;
use crate::rules::object_type::ObjectType;
use crate::rules::overlay_types::OverlayTypeRegistry;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement;
use crate::sim::movement::ScatterFlags;
use crate::sim::world::FrameEffects;
use crate::sim::world::Simulation;

/// TryToDeploy's fallback sites (`0x00B1D010..0x00B1D09C`), offsets from the
/// unit's cell in visit order: north first, then south, then west and east
/// along the row. A function-local static filled on first use
/// (`0x00738F9C..0x007392C7`); executed in tools/mcv_deploy_oracle.py.
const TRY_TO_DEPLOY_SITES: [(i16, i16); 36] = [
    (0, -1),
    (0, -2),
    (1, -2),
    (-1, -2),
    (0, -3),
    (1, -3),
    (-1, -3),
    (2, -3),
    (-2, -3),
    (0, -4),
    (1, -4),
    (-1, -4),
    (2, -4),
    (-2, -4),
    (0, 1),
    (0, 2),
    (1, 2),
    (-1, 2),
    (0, 3),
    (1, 3),
    (-1, 3),
    (2, 3),
    (-2, 3),
    (0, 4),
    (1, 4),
    (-1, 4),
    (2, 4),
    (-2, 4),
    (-1, 0),
    (-2, 0),
    (-3, 0),
    (-4, 0),
    (1, 0),
    (2, 0),
    (3, 0),
    (4, 0),
];

/// UnitType+0x404 `DeploysInto`, the BuildingType it resolved to.
pub(crate) fn deploys_into<'r>(
    sim: &Simulation,
    entity: &GameEntity,
    rules: &'r RuleSet,
) -> Option<&'r ObjectType> {
    let obj = sim.object_type(entity.type_ref(), rules)?;
    rules.object(obj.deploys_into.as_deref()?)
}

/// `DeploysInto` is one of `[AI] BuildConst=` (Rules `+0x8AC`, compared by
/// pointer): a Construction Yard maker.
fn deploys_into_build_const(sim: &Simulation, entity: &GameEntity, rules: &RuleSet) -> bool {
    deploys_into(sim, entity, rules).is_some_and(|yard| yard.build_const_eligible)
}

/// The owner's AutoBaseBuilding latch (House+0x1F3) and that no human
/// controls it (`IsControlledByHuman 0x0050B730`).
fn owner_builds_its_own_base(sim: &Simulation, entity: &GameEntity) -> bool {
    sim.houses.get(&entity.owner()).is_some_and(|house| {
        house.ai_activation.auto_base_building
            && !house.is_controlled_by_human(sim.session.game_mode_nonzero)
    })
}

/// `UnitClass::AI @ 0x007363DE..0x0073645B`, just before the Ready/Commence
/// check (`0x00736465`): a base-building computer house with no Construction
/// Yard (House+0x60 == 0) in a nonzero game mode sends its Construction Yard
/// maker to Hunt unless it already hunts or unloads.
pub(crate) fn ai_queue_hunt(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return;
    };
    let hunts = deploys_into_build_const(sim, entity, rules)
        && owner_builds_its_own_base(sim, entity)
        && sim.session.game_mode_nonzero
        && sim
            .houses
            .get(&entity.owner())
            .is_some_and(|house| house.build_const_order.is_empty())
        && !matches!(
            entity.mission.current().known(),
            Some(MissionType::Hunt | MissionType::Unload)
        );
    if hunts {
        queue(sim, id, MissionType::Hunt);
    }
}

/// Whether `UnitClass::Mission_Hunt @ 0x0073EFC0` takes its deploy arm: the
/// type deploys into a building, and that building is a Construction Yard
/// maker, or the unit holds a target, or a human controls it
/// (`0x0073EFC4..0x0073F013`). Otherwise the Foot hunt runs.
pub(crate) fn hunt_deploys(sim: &Simulation, id: u64, rules: &RuleSet) -> bool {
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    deploys_into(sim, entity, rules).is_some_and(|yard| {
        yard.build_const_eligible
            || entity.attack_target.is_some()
            || sim.owner_is_human(entity.owner())
    })
}

/// Mission_Hunt's deploy arm (`0x0073F015..0x0073F089`), switched on the
/// handler state (+0xBC): 0 tries to deploy and moves to 1 once Deploy
/// accepts; 1 falls back to 0 when no deploy is pending (+0x68C). Every
/// state returns `ftol(Rate * 900) + RandomRanged(0, 2)` with no scan, even
/// after the unit became a building.
pub(crate) fn mission_hunt_deploy(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> i32 {
    match sim
        .substrate
        .entities
        .get(id)
        .map(|e| e.mission.handler_state())
    {
        Some(0) => {
            if try_to_deploy(sim, id, rules, registry, effects)
                && sim.deploy_mcv(id, rules, registry, effects)
                && let Some(entity) = sim.substrate.entities.get_mut(id)
            {
                entity.mission.set_handler_state(1);
            }
        }
        Some(1) => {
            if let Some(entity) = sim.substrate.entities.get_mut(id)
                && !entity.mcv_deploy_pending
            {
                entity.mission.set_handler_state(0);
            }
        }
        _ => {}
    }
    sim.mission_rate_epilogue_for(rules, id, MissionType::Hunt)
}

/// `UnitClass::TryToDeploy @ 0x00738D30`: true when the unit may deploy where
/// it stands. Otherwise it drives toward the first fallback site whose origin
/// the DeploysInto type can stand on, or, with no site and no destination, a
/// computer-controlled unit scatters.
///
/// The unit is lifted out of its cell (Mark(UP), `0x00738D58`) for the tests
/// and put back (`0x00738E1C` / `0x0073936C`). Every origin is the tested
/// cell plus `(-1, -1)` (`[0x0089F6A4]`), whatever the foundation's size, and
/// CanPlaceAt runs for no house. The BuildConst arm's AI site search
/// (`0x00738E2C..0x00738F8B`) needs House+0x1F1, which only ever stores zero
/// (`0x004F5704`, `0x00738F85`); it falls through to the ordinary test.
pub(crate) fn try_to_deploy(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> bool {
    let Some((yard, cell, owner)) = sim.substrate.entities.get(id).and_then(|entity| {
        Some((
            deploys_into(sim, entity, rules)?,
            (entity.position.rx as i16, entity.position.ry as i16),
            entity.owner(),
        ))
    }) else {
        return true;
    };
    let has_destination = |sim: &Simulation| {
        sim.substrate
            .entities
            .get(id)
            .is_some_and(|e| e.navigation.nav_com.is_some())
    };
    let origin_of = |cell: (i16, i16)| (cell.0.wrapping_sub(1), cell.1.wrapping_sub(1));
    sim.foot_mark_remove(id, Some(rules), registry, effects);
    if !has_destination(sim) {
        if crate::sim::build_site::can_place_building_at(
            sim,
            rules,
            registry,
            yard,
            origin_of(cell),
            None,
        ) {
            sim.foot_mark_put(id, Some(rules), registry, effects);
            return true;
        }
        // 0x007392CA..0x00739360: the first fallback site the type can stand
        // on becomes the destination (vt+0x480 SetDestination(cell, 1)).
        if let Some(site) = TRY_TO_DEPLOY_SITES
            .iter()
            .map(|&(dx, dy)| (cell.0.wrapping_add(dx), cell.1.wrapping_add(dy)))
            .find(|&site| {
                crate::sim::build_site::can_place_building_at(
                    sim,
                    rules,
                    registry,
                    yard,
                    origin_of(site),
                    None,
                )
            })
        {
            // An admitted origin lies on the map, and the site is inside
            // its foundation or one cell past it, so it is too.
            sim.set_unit_destination(
                id,
                crate::sim::components::NavTargetRef::cell(site.0 as u16, site.1 as u16),
                rules,
                true,
                effects,
            );
        }
    }
    sim.foot_mark_put(id, Some(rules), registry, effects);
    // 0x00739372..0x00739394: vt+0x174 Scatter(&ZeroCoord, 0, 0), the Unit
    // receiver's null arm.
    if !has_destination(sim)
        && !sim.owner_is_human(owner)
        && let Err(cause) = sim.scatter(
            id,
            None,
            ScatterFlags::new(false, false),
            rules,
            registry,
            effects,
        )
    {
        log::debug!("MCV {id} did not scatter: {cause}");
    }
    false
}

/// `UnitClass::Mission_Guard`'s Construction Yard maker arm
/// (`0x007408C7..0x00740A19`), after the Slave Miner kick and the harvester
/// arms: a base-building computer house's unit queues Unload. Its caller
/// returns the Guard epilogue (`ftol(Rate * 900) + RandomRanged(0, 2)`).
pub(crate) fn guard_queues_unload(sim: &Simulation, id: u64, rules: &RuleSet) -> bool {
    sim.substrate.entities.get(id).is_some_and(|entity| {
        deploys_into_build_const(sim, entity, rules) && owner_builds_its_own_base(sim, entity)
    })
}

pub(crate) fn is_mcv(sim: &Simulation, entity: &GameEntity, rules: &RuleSet) -> bool {
    entity.category == EntityCategory::Unit
        && sim
            .object_type(entity.type_ref(), rules)
            .is_some_and(|obj| {
                // The passenger/refinery branches precede DeploysInto in
                // 0x73D630. A Slave Miner deploys through the same body; its
                // manager moves with it (`deploy_mcv`, 0x00739956).
                obj.deploys_into
                    .as_deref()
                    .is_some_and(|name| rules.object(name).is_some())
                    && obj.passengers == 0
                    && !obj.harvester
            })
}

pub(crate) fn queue_guard(sim: &mut Simulation, id: u64) {
    queue(sim, id, MissionType::Guard);
}

fn queue(sim: &mut Simulation, id: u64, mission: MissionType) {
    let _ = sim.mission_queue_exact(
        id,
        MissionId::from_known(mission),
        0,
        sim.session.binary_frame,
        &EntityReadyInputProvider,
    );
}

/// Returns a delay to the existing MissionClass epilogue, even when Deploy
/// removed the caller. Every ordinary MCV branch reaches 0x0073DE3A's RNG draw.
pub(crate) fn mission_unload(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) -> i32 {
    let state = sim
        .substrate
        .entities
        .get(id)
        .map(|e| e.mission.handler_state());
    if state == Some(0) {
        // 0x0073DD82..0x0073DD98: OVER_OUT to radio contact 0, then state 1.
        crate::sim::radio::transmit_to_contact(
            sim,
            id,
            crate::sim::radio::RadioMessage::Break,
            Some(rules),
            effects,
        );
        if let Some(e) = sim.substrate.entities.get_mut(id) {
            e.navigation.path_replay.cursor = e
                .navigation
                .path_replay
                .directions
                .len()
                .min(u16::MAX as usize) as u16;
            e.mission.set_handler_state(1);
        }
        // Native state 0 falls through to state 1 in this same invocation.
    }
    let now = sim.session.binary_frame;
    match state {
        Some(0 | 1) => {
            // The locomotor's Is_Moving (vt+0x10 at `0x0073DDBD`).
            let moving = sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| movement::motion_query::is_moving(e) == Some(true));
            if moving {
                // 0x0073DE20..0x0073DE34: a queued mission other than Unload
                // commences while the unit still drives.
                let queued = sim.substrate.entities.get(id).map(|e| e.mission.queued());
                if queued.is_some_and(|queued| {
                    queued != MissionId::NONE
                        && queued != MissionId::from_known(MissionType::Unload)
                }) {
                    let _ = sim.mission_commence_exact(id, now);
                }
            } else {
                sim.deploy_mcv(id, rules, registry, effects);
                finish_initial_attempt(sim, id);
            }
        }
        Some(2) => {
            let pending = sim
                .substrate
                .entities
                .get(id)
                .is_some_and(|e| e.mcv_deploy_pending);
            if pending {
                let accepted = sim.deploy_mcv(id, rules, registry, effects);
                finish_retry(sim, id, accepted);
            } else {
                // 0x0073D6CA..0x0073D6DB: Enter_Idle_Mode(0, 1), then
                // NextMission.
                sim.unit_enter_idle_mode(id, Some(rules), false, effects);
                let _ = sim.mission_commence_exact(id, now);
            }
        }
        _ => {}
    }
    sim.mission_rate_epilogue_for(rules, id, MissionType::Unload)
}

// Kept as the production result seam so original interior-block oracle outputs
// can be compared without claiming to emulate the whole construction transaction.
/// Mission_Unload state 1 after its Deploy (`0x0073DDCB..0x0073DE1E`): a
/// pending deploy moves to state 2; otherwise a computer house in a nonzero
/// game mode hunts and everyone else guards.
pub(crate) fn finish_initial_attempt(sim: &mut Simulation, id: u64) {
    if let Some(e) = sim.substrate.entities.get(id).filter(|e| !e.dying) {
        if e.mcv_deploy_pending {
            sim.substrate
                .entities
                .get_mut(id)
                .unwrap()
                .mission
                .set_handler_state(2);
        } else {
            let hunt =
                sim.houses.get(&e.owner()).is_some_and(|house| {
                    !house.is_controlled_by_human(sim.session.game_mode_nonzero)
                }) && sim.session.game_mode_nonzero;
            queue(
                sim,
                id,
                if hunt {
                    MissionType::Hunt
                } else {
                    MissionType::Guard
                },
            );
        }
    }
}

pub(crate) fn finish_retry(sim: &mut Simulation, id: u64, accepted: bool) {
    if !accepted
        && let Some(e) = sim.substrate.entities.get_mut(id)
        && e.navigation.nav_com.is_some()
    {
        e.mcv_deploy_pending = false;
    }
}

pub(crate) fn per_cell_process(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    registry: Option<&OverlayTypeRegistry>,
    effects: FrameEffects<'_>,
) {
    // 0x739EEC..0x739EF8 has no current-mission guard. Stop and Move do not
    // erase pending intent; native Deploy decides whether NavCom allows it.
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|e| !e.dying && e.mcv_deploy_pending)
    {
        sim.deploy_mcv(id, rules, registry, effects);
    }
}

#[cfg(test)]
#[path = "mcv_deploy_tests.rs"]
mod tests;
