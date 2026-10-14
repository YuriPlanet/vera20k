//! `AircraftClass`'s missions. `MissionClass::AI @ 0x005B3060` runs an
//! aircraft's current mission handler (jump table `0x005B34E8`) once its
//! mission timer is due and restarts the timer with the frames the handler
//! returns ([`dispatch_mission`]). Every handler keeps its state in
//! Mission+0xBC (`MissionCom::handler_state`), which Commence and Assign zero.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/components, sim/combat, sim/docking, rules/.
//! - sim/ NEVER depends on render/, ui/, audio/, net/.

mod airfield;
pub mod attack_mission;
pub mod drop_payload;
pub(crate) mod enter_mission;
pub(crate) mod fire_at;
pub(crate) mod guard_mission;
mod hunt_mission;
mod idle_entry;
pub(crate) use idle_entry::enter_idle_mode_for;
#[cfg(test)]
pub(crate) use idle_entry::record_idle_calls_for_test;
pub(crate) mod landing_base;
mod leave_map;
#[cfg(test)]
mod leave_map_tests;
pub(crate) mod move_mission;
pub mod paradrop_mission;
mod retreat_mission;
pub(crate) mod spyplane_mission;

#[cfg(test)]
mod dock_cycle_tests;
#[cfg(test)]
mod release_tests;
#[cfg(test)]
mod team_strike_tests;

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::world::{FrameEffects, ObjectAiCtx, Simulation};

/// What every MissionClass stub returns (`MOV EAX, 0x1C2`).
const MISSION_STUB_FRAMES: i32 = 450;

/// Batch driver for fixtures that dispatch every aircraft without the live
/// object pass. Production dispatches each aircraft in its own LogicVector
/// slot through [`dispatch_mission`].
#[cfg(test)]
pub fn tick_aircraft_missions(
    sim: &mut Simulation,
    rules: &RuleSet,
) -> std::collections::BTreeSet<u64> {
    let order = sim.substrate.logic.as_slice().to_vec();
    order
        .into_iter()
        // This batch fixture has no resident overlay table.
        .filter(|&id| dispatch_mission(sim, id, rules, ObjectAiCtx::default()))
        .collect()
}

/// Mission+0xBC as Mission_Move's and Mission_Attack's switches read it; a
/// value past a byte reads as a state past each of their cases.
pub(crate) fn handler_state(entity: &GameEntity) -> u8 {
    u8::try_from(entity.mission.handler_state()).unwrap_or(u8::MAX)
}

/// Mission_Attack's state while Attack is the current mission (`+0xAC`) of
/// aircraft `entity`.
pub(crate) fn attack_state(entity: &GameEntity) -> Option<u8> {
    (entity.category() == EntityCategory::Aircraft
        && entity.mission.current() == MissionId::from_known(MissionType::Attack))
    .then(|| handler_state(entity))
}

/// `MissionClass::AI @ 0x005B3060` for aircraft `id`: an alive aircraft whose
/// mission timer is due runs its current mission's handler and restarts the
/// timer with the frames it returns. `FootClass::AI @ 0x004DA530` runs it
/// before locomotor Process (`+0x40`), so the handlers' Scenario draws and
/// NavCom reservations interleave with the other objects' AI in Logic order.
///
/// The handlers: Mission_Move (`0x004166E0`, [`move_mission`]),
/// Mission_Attack (`0x00417FE0`, [`attack_mission`]), Mission_Guard for
/// Guard and Sticky and Mission_AreaGuard ([`guard_mission`]), Mission_Enter
/// ([`enter_mission`]), Mission_Hunt (`0x00414A80`, [`hunt_mission`]),
/// Mission_Retreat (`0x00415A50`), Mission_Unload (`0x004151E0`), the
/// paradrop plane's two (`0x004158E0`, `0x00415960`) and the Spy Plane's two
/// (vt+0x26C, vt+0x270).
///
/// Returns whether the combat phase must run a Mission_Attack strike visit
/// (states 4..9) this frame. RESIDUAL: that visit runs in VERA's combat
/// phase after the live pass, like every other attacker's FireAt, so its
/// draws do not interleave.
///
/// Every other slot of the aircraft's table is a MissionClass stub that
/// returns 450 frames (`0x005B2E10..0x005B2FB0`): Sleep, Harmless, Ambush,
/// Harvest, Return, Stop, Construction, Selling, Repair, Missile, Open and
/// Wait, and the Sleep slot QMove, Attack Move (29), NONE and any id past
/// 0x1F reach (`0x005B30BB`, `0x005B34C4`). An attack-move order never queues
/// 29 for an aircraft: its type refuses to attack-move, so
/// `Assign_Target_Command` answers Move.
///
/// RESIDUAL: Patrol (`0x00417300`) and the Foot handlers of Capture
/// and Sabotage (`0x004D4B20`), Eaten (`0x004D4CB0`) and Rescue
/// (`0x004DDF90`) are not run: the timer stays due and nothing happens.
/// Trigger: a map or trigger giving an aircraft one of those missions; no
/// retail AI script does. Effect: the aircraft keeps flying to its
/// destination and then hovers there.
pub(crate) fn dispatch_mission(
    sim: &mut Simulation,
    id: u64,
    rules: &RuleSet,
    ctx: ObjectAiCtx<'_>,
) -> bool {
    let now = sim.session.binary_frame;
    let Some(entity) = sim.substrate.entities.get(id) else {
        return false;
    };
    if entity.dying
        || entity.category() != EntityCategory::Aircraft
        || !entity.mission.dispatch_timer().due(now)
    {
        return false;
    }
    let registry = ctx.overlay_registry;
    let delay = match entity.mission.current().known() {
        Some(MissionType::Move) => {
            let state = handler_state(entity);
            let (next, delay) = sim.aircraft_move(id, state, rules, registry, ctx.effects);
            write_state(sim, id, state, next);
            delay
        }
        Some(MissionType::Attack) => {
            let state = handler_state(entity);
            let Some((next, delay)) = attack_visit(sim, id, state, rules, registry, ctx.effects)
            else {
                return true;
            };
            write_state(sim, id, state, next);
            delay
        }
        Some(MissionType::Guard | MissionType::Sticky) => {
            sim.aircraft_mission_guard(id, rules, ctx)
        }
        Some(MissionType::AreaGuard) => sim.aircraft_mission_area_guard(id, rules, ctx),
        Some(MissionType::Enter) => sim.aircraft_mission_enter(id, rules, registry, ctx.effects),
        Some(MissionType::Hunt) => sim.aircraft_mission_hunt(id, rules, ctx),
        Some(MissionType::Retreat) => retreat_mission::retreat(sim, id, rules, ctx.effects),
        Some(MissionType::Unload) => {
            crate::sim::transport_unload::mission_unload(sim, id, rules, registry, ctx.effects)
        }
        Some(MissionType::ParadropApproach) => {
            paradrop_mission::approach(sim, id, rules, ctx.effects)
        }
        Some(MissionType::ParadropOverfly) => {
            paradrop_mission::overfly(sim, id, rules, registry, ctx.effects)
        }
        Some(MissionType::SpyplaneApproach) => {
            spyplane_mission::approach(sim, id, rules, ctx.effects)
        }
        Some(MissionType::SpyplaneOverfly) => {
            spyplane_mission::overfly(sim, id, rules, ctx.effects)
        }
        Some(
            MissionType::Patrol
            | MissionType::Capture
            | MissionType::Sabotage
            | MissionType::Eaten
            | MissionType::Rescue,
        ) => return false,
        _ => MISSION_STUB_FRAMES,
    };
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        entity.mission.write_dispatch_epilogue(now as i32, delay);
    }
    false
}

/// `ObjectClass::Distance_To @ 0x005F6440` from the aircraft; a NULL target
/// answers 0 (`0x005F644B..0x005F6456`).
fn target_distance(
    sim: &Simulation,
    id: u64,
    target: Option<crate::sim::combat::TargetKind>,
) -> i32 {
    let entities = &sim.substrate.entities;
    target
        .zip(entities.get(id))
        .and_then(|(target, plane)| {
            crate::sim::combat::object_distance_to(plane, &target, entities)
        })
        .unwrap_or(0)
}

/// Whether the aircraft has no NavCom (`+0x5A4`).
fn nav_com_absent(sim: &Simulation, id: u64) -> bool {
    sim.substrate
        .entities
        .get(id)
        .is_some_and(|plane| plane.navigation.nav_com.is_none())
}

/// `Queue_Mission(mission, 0)` (vt+0x1E8 = `0x0041BA90`).
fn queue_mission(sim: &mut Simulation, id: u64, mission: MissionType) {
    if let Some(plane) = sim.substrate.entities.get_mut(id) {
        crate::sim::mission::authority::queue_entity_mission_deferred(
            plane,
            MissionId::from_known(mission),
        );
    }
}

/// Mission+0xBC after a Mission_Move or Mission_Attack visit that read
/// `read` and left `next`. Those handlers write it only where it changes,
/// and never after their `Enter_Idle_Mode(0, 1)` (vt+0x484), whose Commence
/// zeroes it.
fn write_state(sim: &mut Simulation, id: u64, read: u8, next: u8) {
    if next != read
        && let Some(entity) = sim.substrate.entities.get_mut(id)
    {
        entity.mission.set_handler_state(u32::from(next));
    }
}

/// One Mission_Attack visit in the aircraft's dispatch: its state and delay,
/// or `None` for a strike visit (states 4..9), which fires and so runs in
/// the combat phase, where VERA's FireAt lives (`combat::aircraft_release`).
fn attack_visit(
    sim: &mut Simulation,
    id: u64,
    state: u8,
    rules: &RuleSet,
    registry: Option<&crate::rules::overlay_types::OverlayTypeRegistry>,
    frame_effects: FrameEffects<'_>,
) -> Option<(u8, i32)> {
    if let Some(entity) = sim.substrate.entities.get_mut(id) {
        attack_mission::enter_attack_state(entity, state);
    }
    Some(match state {
        0 => sim.aircraft_begin_attack(id),
        1 => sim.aircraft_reengage(id, rules, frame_effects),
        3 => sim.aircraft_approach(id, rules, frame_effects),
        4..=9 if sim.aircraft_strikes(id, state) => return None,
        4..=9 => (10, 1),
        10 => sim.aircraft_exit(id, rules, registry, frame_effects),
        // State 2 (`0x00418D1D`) is the epilogue alone. Mission_Attack runs
        // as the Attack mission's handler, so its epilogue reads Attack's
        // Rate.
        state => (state, sim.mission_rate_epilogue(rules, MissionType::Attack)),
    })
}
