//! Ordinary Unit DeploysInto continuation. Native identities: EventClass event 9
//! 0x004C77EA..0x004C7812, UnitClass::Deploy 0x007393C0 and Mission_Unload
//! 0x0073D630. See tools/mcv_deploy_oracle.py for executable boundary evidence.
//! MissionCom owns scheduling; the entity flag represents runtime Unit+0x68C.

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::authority::EntityReadyInputProvider;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::movement;
use crate::sim::world::Simulation;

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

pub(crate) fn issue_order(sim: &mut Simulation, id: u64, rules: &RuleSet) -> bool {
    if !sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|e| !e.dying && !e.lifecycle.in_limbo && is_mcv(sim, e, rules))
    {
        return false;
    }
    sim.run_dock_teardown(id, crate::sim::mission::retask::DockTeardown::All);
    let entity = sim.substrate.entities.get_mut(id).unwrap();
    // Event9 clears destination and target before Queue(Unload,false). Queue's
    // existing same-mission guard preserves the handler/timer on repeated D.
    movement::stop_navigation_at_committed_head(entity);
    entity.attack_target = None;
    entity.passively_acquired_target = false;
    entity.order_intent = None;
    sim.mission_queue_exact(
        id,
        MissionId::from_known(MissionType::Unload),
        0,
        sim.session.binary_frame,
        &EntityReadyInputProvider,
    )
    .is_ok()
}

/// Native rounds FacingClass::Current, including wrap at 0xff80.
pub(crate) fn current_direction(entity: &GameEntity, frame: u32) -> u8 {
    let raw = entity
        .body_facing
        .as_ref()
        .map_or(u16::from(entity.facing) << 8, |body| body.current(frame));
    (((u32::from(raw) >> 7) + 1) >> 1) as u8
}

pub(crate) fn start_turn(entity: &mut GameEntity, target: u8, frame: u32) {
    movement::drive_do_turn(entity, u16::from(target) << 8, frame);
    entity.facing_target = Some(target);
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
pub(crate) fn mission_unload(sim: &mut Simulation, id: u64, rules: &RuleSet) -> i32 {
    let state = sim
        .substrate
        .entities
        .get(id)
        .map(|e| e.mission.handler_state());
    if state == Some(0) {
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.navigation.path_replay.cursor = e
            .navigation
            .path_replay
            .directions
            .len()
            .min(u16::MAX as usize) as u16;
        e.mission.set_handler_state(1);
        // Native state 0 falls through to state 1 in this same invocation.
    }
    match state {
        Some(0 | 1) => {
            let moving = sim
                .substrate
                .entities
                .get(id)
                .is_some_and(movement::ready_producer::is_moving_for_unit_shp_draw);
            if !moving {
                sim.deploy_mcv(id, rules, &Default::default());
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
                let accepted = sim.deploy_mcv(id, rules, &Default::default());
                finish_retry(sim, id, accepted);
            }
        }
        _ => {}
    }
    sim.mission_rate_epilogue_for(rules, id, MissionType::Unload)
}

// Kept as the production result seam so original interior-block oracle outputs
// can be compared without claiming to emulate the whole construction transaction.
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
            let hunt = sim
                .houses
                .get(&e.owner())
                .is_some_and(|house| !house.is_controlled_by_human(true))
                && sim.session.game_mode_nonzero;
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

pub(crate) fn per_cell_process(sim: &mut Simulation, id: u64, rules: &RuleSet) {
    // 0x739EEC..0x739EF8 has no current-mission guard. Stop and Move do not
    // erase pending intent; native Deploy decides whether NavCom allows it.
    if sim
        .substrate
        .entities
        .get(id)
        .is_some_and(|e| !e.dying && e.mcv_deploy_pending)
    {
        sim.deploy_mcv(id, rules, &Default::default());
    }
}

#[cfg(test)]
#[path = "mcv_deploy_tests.rs"]
mod tests;
