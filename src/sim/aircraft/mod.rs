//! Aircraft mission state machines — orchestrates attack runs, guard/RTB,
//! movement, and idle behavior for Fly-locomotor aircraft.
//!
//! This module implements the mission layer that sits between air movement
//! physics (air_movement.rs) and combat firing (combat/). Missions control
//! WHEN aircraft fire, HOW they approach targets, and WHAT they do after
//! completing an attack pass.
//!
//! ## Dependency rules
//! - Part of sim/ — depends on sim/components, sim/combat, sim/docking, rules/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

pub mod attack_mission;
pub mod drop_payload;
pub mod idle_mode;
pub(crate) mod landing_base;
pub mod paradrop_mission;
pub mod runtime_contract;

#[cfg(test)]
mod dock_cycle_tests;
#[cfg(test)]
mod release_tests;

use serde::{Deserialize, Serialize};

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::mission::MissionTimer;
use crate::sim::movement::locomotor::AirMovePhase;
use crate::sim::production::foundation_dimensions;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// Aircraft mission — determines the high-level behavior each tick.
///
/// Replaces the original engine's MissionClass dispatch for aircraft.
/// Each variant carries its own sub-state for the state machine.
#[derive(Debug, Clone, Hash, Serialize, Deserialize)]
pub enum AircraftMission {
    /// Idle on the ground or hovering — waiting for orders.
    /// BalloonHover aircraft idle at cruise altitude.
    Idle,

    /// Flying toward a destination (player Move command).
    Move {
        /// 0=init, 1=set_course, 2=in_flight, 3=arrived, 4=course_correction
        sub_state: u8,
    },

    /// Attacking a target — 11-state machine from gamemd.exe.
    Attack {
        /// State within the attack state machine (0-10).
        sub_state: u8,
    },

    /// Guard — idle in the air, scanning for targets, RTB when low ammo.
    Guard,

    /// Returning to base — flying toward airfield for reload.
    /// Absorbed from the old AircraftDockPhase::ReturnToBase.
    ReturnToBase {
        /// Target airfield entity stable_id.
        airfield_id: u64,
    },

    /// Docking at an airfield — descending, reloading, launching.
    Docking {
        /// Target airfield entity stable_id.
        airfield_id: u64,
        /// 0=wait_for_dock, 1=descending, 2=reloading, 3=launching
        sub_state: u8,
        /// Frame-anchored gate until the next ammo point is restored (during
        /// reloading); was a per-tick `u32` countdown.
        reload_timer: MissionTimer,
        /// Pad index assigned to this aircraft on the airfield (0-based).
        /// Meaningful once `sub_state >= 1` (after pad reservation succeeds).
        pad_index: u32,
    },

    /// Parked on helipad pad — freshly built, waiting for player command.
    /// Dock slot is reserved. Aircraft is Landed, altitude 0.
    /// Exits via Move/Attack command (releases dock, triggers takeoff).
    DockedIdle {
        /// Airfield entity stable_id this aircraft is docked at.
        airfield_id: u64,
        /// Pad index this aircraft is parked on (0-based).
        pad_index: u32,
    },

    /// Standard superweapon paradrop carrier in its Open-equivalent mission.
    /// Kept under the older Rust name for save compatibility; stock SW PDPLANE
    /// starts here (gamemd mission 0x1A), not in binary Mission_ParaDropApproach.
    /// Transitions to the Rescue-equivalent state when distance ≤ ParadropRadius.
    ParaDropApproach {
        target_rx: u16,
        target_ry: u16,
        /// Save-compatible latch retained from the older Rust approach path.
        /// Standard Mission_Open does not emit ChuteSound/fog reveal at threshold.
        has_revealed_fog: bool,
    },

    /// Standard superweapon paradrop carrier in its Rescue-equivalent mission.
    /// Kept under the older Rust name for save compatibility; this is not
    /// binary Mission_ParaDropOverfly for stock SW launches.
    /// Dispenses payload at the Mission_Rescue cadence: one Drop_Payload call
    /// per Rescue execution, then 5 native gameplay frames before the next.
    /// Transitions to silent despawn at the opposite edge once cargo is empty.
    ParaDropOverfly {
        /// Opposite-edge cell to fly to once cargo is empty.
        exit_rx: u16,
        exit_ry: u16,
        /// Ticks until next drop allowed (Mission_Rescue 5-frame cadence).
        drop_cooldown: u16,
        /// LandingState mirror. Drop_Payload writes 5, but in-range Rescue does
        /// not use it as an extra throttle beyond the 5-frame mission cadence.
        landing_state: u8,
        /// Decrements per drop; parity drives V-pattern side (paradrop P25).
        payload_count: u8,
    },
}

impl AircraftMission {
    /// Whether this mission is an active attack (any Attack sub-state).
    pub fn is_attacking(&self) -> bool {
        matches!(self, AircraftMission::Attack { .. })
    }

    /// Whether this aircraft is parked on a helipad waiting for orders.
    pub fn is_docked_idle(&self) -> bool {
        matches!(self, AircraftMission::DockedIdle { .. })
    }
}

/// Batch driver for fixtures that dispatch every aircraft without the live
/// object pass. Production dispatches each aircraft in its own LogicVector
/// slot through [`dispatch_aircraft_mission`].
#[cfg(test)]
pub fn tick_aircraft_missions(
    sim: &mut Simulation,
    rules: &RuleSet,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
) -> std::collections::BTreeSet<u64> {
    let order = sim.substrate.logic.as_slice().to_vec();
    order
        .into_iter()
        .filter(|&id| dispatch_aircraft_mission(sim, rules, id, path_grid))
        .collect()
}

/// One aircraft's mission dispatch inside its own LogicVector slot.
///
/// `FootClass::AI @ 0x004DA530` runs TechnoClass AI, and with it the mission
/// dispatch, before locomotor Process (`+0x40`). Mission_Attack's Scenario RNG
/// draws (state1 Rate jitter, FindFireLocation) and NavCom reservations
/// therefore interleave with the other objects' AI in Logic order, ahead of
/// this aircraft's own Fly Process. Returns whether the combat phase must
/// run a Mission_Attack strike visit (states 4..9) for this aircraft this
/// frame. RESIDUAL: that visit runs in VERA's combat phase after the live
/// pass, like every other attacker's FireAt, so its draws do not interleave.
///
/// `path_grid`: Paradrop's Drop_Payload uses it for drop-cell passability.
pub(crate) fn dispatch_aircraft_mission(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
) -> bool {
    let Some(e) = sim.substrate.entities.get(id) else {
        return false;
    };
    // A Dying aircraft corpse must not run its mission (move, fire,
    // paradrop, reveal fog) for the tick before the end-of-tick drain.
    if e.dying {
        return false;
    }
    let Some(mission) = e.aircraft_mission.clone() else {
        return false;
    };
    if mission.is_attacking() && !e.mission.dispatch_timer().due(sim.session.binary_frame) {
        return false;
    }
    if e.locomotor
        .as_ref()
        .is_none_or(|l| l.kind != LocomotorKind::Fly)
    {
        return false;
    }
    match mission_step(sim, rules, id, &mission, path_grid) {
        Some(m) => apply_mission_mutation(sim, rules, m, path_grid),
        None => false,
    }
}

/// One mission handler's decision, applied by [`apply_mission_mutation`].
struct MissionMutation {
    id: u64,
    new_mission: AircraftMission,
    ammo_delta: i32,
    fire_at: Option<crate::sim::combat::TargetKind>,
    move_to: Option<(u16, u16)>,
    /// `Assign_Destination(target, 1)` through the aircraft's destination
    /// owner (NavCom and the Fly MoveTo), ahead of any `move_to`.
    assign_destination: Option<crate::sim::components::NavTargetRef>,
    self_destruct: bool,
    /// Fly BeginLanding4CFA70 through the world owner, after the mission write.
    begin_landing: bool,
    /// Fly BeginTakeoff4CF950 through the world owner.
    begin_takeoff: bool,
    // Paradrop-specific apply-phase signals.
    paradrop_chute_sound_at: Option<(u16, u16)>,
    paradrop_try_drop: bool,
    paradrop_payload_count_pre: u8,
    paradrop_silent_despawn: bool,
}

fn mission_step(
    sim: &mut Simulation,
    rules: &RuleSet,
    id: u64,
    mission: &AircraftMission,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
) -> Option<MissionMutation> {
    let now = sim.session.binary_frame;
    let mut m = MissionMutation {
        id,
        new_mission: mission.clone(),
        ammo_delta: 0,
        fire_at: None,
        move_to: None,
        assign_destination: None,
        self_destruct: false,
        begin_landing: false,
        begin_takeoff: false,
        paradrop_chute_sound_at: None,
        paradrop_try_drop: false,
        paradrop_payload_count_pre: 0,
        paradrop_silent_despawn: false,
    };

    match mission {
        AircraftMission::Idle => enter_idle_mode(sim, rules, id, &mut m)?,

        AircraftMission::Attack { sub_state } => {
            if let Some(entity) = sim.substrate.entities.get_mut(id) {
                attack_mission::enter_attack_state(entity, *sub_state);
            }
            match *sub_state {
                0 => m.new_mission = sim.aircraft_begin_attack(id),
                1 => m.new_mission = sim.aircraft_reengage(id, rules),
                3 => m.new_mission = sim.aircraft_approach(id, rules),
                // The release, the follow-up shot and the strafe run fire, so
                // they run in the combat phase, where VERA's FireAt lives.
                4..=9 => match sim.aircraft_strike_target(id, *sub_state) {
                    Some(target) => m.fire_at = Some(target),
                    None => m.new_mission = sim.aircraft_attack_visit(id, 10, 1),
                },
                10 => {
                    let (mission, idle) = sim.aircraft_exit(id, rules);
                    m.new_mission = mission;
                    if idle {
                        enter_idle_mode(sim, rules, id, &mut m)?;
                    }
                }
                // State 2 (`0x00418D1D`) is the epilogue alone.
                state => {
                    let delay = attack_mission::mission_epilogue(
                        rules,
                        crate::sim::mission::MissionType::Attack,
                        &mut sim.scenario_rng,
                    );
                    m.new_mission = sim.aircraft_attack_visit(id, state, delay);
                }
            }

            // Fly owns height targets. Native4CF3D4..4CF4CF selects
            // destination-relative height, IsDropship approach height or
            // Type FlightLevel. Repeated attack mission visits must not
            // divide the mutable target by3. The horizontal target-selection
            // transaction remains part of the Fly migration.
            // Fly Process owns the target speed (`air_movement::
            // write_fly_target_speed`); Mission_Attack writes no speed.
        }

        AircraftMission::Guard => {
            let entity = sim.substrate.entities.get(id)?;
            let ammo = entity.aircraft_ammo.as_ref();
            let ammo_current = ammo.map_or(-1, |a| a.current);
            let ammo_max = ammo.map_or(-1, |a| a.max);
            let has_target = entity.attack_target.is_some();

            // gamemd Mission_Guard RTB decision (default ReturnFire mode):
            // an in-flight aircraft returns to rearm whenever it has spent
            // ammo (ammo < maxAmmo) and is not actively engaging a target.
            // Without the second clause a strike craft whose target dies
            // mid-sortie with ammo left would hover here indefinitely.
            // (Mode1 `ammo == 0` / Mode2 `ammo < max/2` are INI-gated
            // globals not yet mapped to keys — default mode is stock.)
            let out_of_ammo = ammo_current <= 0 && ammo_max > 0;
            let spent_and_idle = !has_target && ammo_max > 0 && ammo_current < ammo_max;

            // A spawn-manager child's base is its parent, not an airfield.
            // Stock HORNET/ASW ship with `Dock=` commented out, so their
            // rearm is driven entirely by the parent's SpawnManager
            // (state 3 → 4 → 6). Letting them pick an unrelated helipad
            // here would fight that recall.
            let spawn_child = entity.spawn_owner_id.is_some();

            if has_target && ammo_current > 0 {
                m.new_mission = AircraftMission::Attack { sub_state: 0 };
            } else if spawn_child {
                // Hold station; the parent's manager issues the recall.
            } else if out_of_ammo || spent_and_idle {
                let nearest = find_nearest_airfield_for(
                    sim,
                    rules,
                    entity.owner(),
                    entity.type_ref(),
                    (entity.position.rx, entity.position.ry),
                );
                if let Some((af_id, af_rx, af_ry)) = nearest {
                    m.new_mission = AircraftMission::ReturnToBase { airfield_id: af_id };
                    m.move_to = Some((af_rx, af_ry));
                } else {
                    let type_str = sim.interner.resolve(entity.type_ref());
                    let airport_bound = rules.object(type_str).map_or(false, |o| o.airport_bound);
                    if airport_bound {
                        m.self_destruct = true;
                    }
                }
            }
        }

        AircraftMission::ReturnToBase { airfield_id } => {
            let entity = sim.substrate.entities.get(id)?;
            let af_ok = sim
                .substrate
                .entities
                .get(*airfield_id)
                .is_some_and(|af| af.health.current > 0 && !af.dying);
            if !af_ok {
                m.new_mission = AircraftMission::Idle;
                return Some(m);
            }
            let af = sim.substrate.entities.get(*airfield_id).unwrap();
            let type_str = sim.interner.resolve(af.type_ref());
            let (fw, fh) = rules
                .object(type_str)
                .map(|o| foundation_dimensions(&o.foundation))
                .unwrap_or((1, 1));
            let dock_rx = af.position.rx + fw / 2;
            let dock_ry = af.position.ry + fh / 2;

            let dx = (entity.position.rx as i32 - dock_rx as i32).abs();
            let dy = (entity.position.ry as i32 - dock_ry as i32).abs();
            let dist = dx.max(dy);

            if dist <= 2 {
                // sub_state 0 = WaitForDock; pad_index will be overwritten
                // by the reservation once a pad is granted.
                m.new_mission = AircraftMission::Docking {
                    airfield_id: *airfield_id,
                    sub_state: 0,
                    reload_timer: MissionTimer::default(),
                    pad_index: 0,
                };
            } else if entity.movement_target.is_none() {
                m.move_to = Some((dock_rx, dock_ry));
            }
        }

        AircraftMission::Docking {
            airfield_id,
            sub_state,
            reload_timer,
            pad_index,
        } => {
            let entity = sim.substrate.entities.get(id)?;
            let air_phase = crate::sim::movement::air_movement::fly_mission_phase(
                entity,
                sim.resolved_terrain.as_ref(),
            );
            let landing = entity
                .locomotor
                .as_ref()
                .and_then(|l| l.fly_runtime())
                .is_some_and(|s| s.landing());
            let arrived = crate::sim::movement::air_movement::fly_landing_arrival(entity);
            let af_type_ref = sim
                .substrate
                .entities
                .get(*airfield_id)
                .map_or(entity.type_ref(), |af| af.type_ref());
            let ammo = entity.aircraft_ammo.as_ref();
            let ammo_current = ammo.map_or(0, |a| a.current);
            let ammo_max = ammo.map_or(0, |a| a.max);
            let reload_rate = rules.general.reload_rate_ticks;

            match sub_state {
                0 => {
                    // Wait for dock slot.
                    let max_slots = rules
                        .object(sim.interner.resolve(af_type_ref))
                        .map(|o| o.dock_contact_capacity())
                        .unwrap_or(1);
                    // Native AircraftClass::IsCellOccupied reaches the
                    // unconditional Winged Cell leaf first; dock ownership
                    // and first-free pad reservation remain this wrapper's
                    // meaningful admission gates.
                    if runtime_contract::aircraft_landing_cell_leaf_clear()
                        && let Some(reserved_pad) =
                            sim.reserve_airfield_pad(*airfield_id, id, max_slots)
                    {
                        m.new_mission = AircraftMission::Docking {
                            airfield_id: *airfield_id,
                            sub_state: 1,
                            reload_timer: MissionTimer::default(),
                            pad_index: reserved_pad,
                        };
                        // Re-target descent toward the per-pad cell so
                        // multi-pad airfields visibly spread occupants.
                        if let Some((px, py)) =
                            sim.substrate.entities.get(*airfield_id).and_then(|af| {
                                let obj = sim.object_type(af.type_ref(), rules)?;
                                let foundation =
                                    crate::sim::production::foundation_dimensions(&obj.foundation);
                                obj.pads.get(reserved_pad as usize).map(|pad| {
                                    crate::sim::docking::pad_geometry::pad_cell_for(
                                        (af.position.rx, af.position.ry),
                                        foundation,
                                        pad,
                                    )
                                })
                            })
                        {
                            m.move_to = Some((px, py));
                        }
                        // Mirror into AircraftAmmo for downstream consumers.
                        if let Some(entity) = sim.substrate.entities.get_mut(id)
                            && let Some(ref mut ammo) = entity.aircraft_ammo
                        {
                            ammo.target_pad = Some(reserved_pad);
                        }
                    }
                }
                1 => {
                    // Descend once the pad approach arrives (Fly
                    // Horizontal_Step4CF520's landing arm distance/speed).
                    if air_phase == Some(AirMovePhase::Landed) {
                        m.new_mission = AircraftMission::Docking {
                            airfield_id: *airfield_id,
                            sub_state: 2,
                            reload_timer: MissionTimer::armed(now, reload_rate),
                            pad_index: *pad_index,
                        };
                    } else if !landing && arrived {
                        m.begin_landing = true;
                    }
                }
                2 => {
                    // Reloading.
                    if reload_timer.due(now) {
                        m.ammo_delta = 1;
                        if ammo_current + 1 >= ammo_max {
                            // Fully reloaded → release the pad and launch.
                            sim.release_airfield_pad(id);
                            m.begin_takeoff = true;
                            m.new_mission = AircraftMission::Docking {
                                airfield_id: *airfield_id,
                                sub_state: 3,
                                reload_timer: MissionTimer::default(),
                                pad_index: *pad_index,
                            };
                        } else {
                            m.new_mission = AircraftMission::Docking {
                                airfield_id: *airfield_id,
                                sub_state: 2,
                                reload_timer: MissionTimer::armed(now, reload_rate),
                                pad_index: *pad_index,
                            };
                        }
                    }
                    // Otherwise the same frame-anchored timer carries over.
                }
                3 => {
                    // Launching — wait for cruising altitude.
                    if air_phase == Some(AirMovePhase::Cruising) {
                        m.new_mission = AircraftMission::Idle;
                        // Clear target_pad now that the dock is released.
                        if let Some(entity) = sim.substrate.entities.get_mut(id)
                            && let Some(ref mut ammo) = entity.aircraft_ammo
                        {
                            ammo.target_pad = None;
                        }
                    }
                }
                _ => {
                    m.new_mission = AircraftMission::Idle;
                }
            }
        }

        AircraftMission::Move { .. } => {
            let entity = sim.substrate.entities.get(id)?;
            if entity.movement_target.is_none() {
                m.new_mission = AircraftMission::Idle;
            }
        }

        AircraftMission::DockedIdle {
            airfield_id,
            pad_index: _,
        } => {
            // Check if airfield still alive.
            let af_ok = sim
                .substrate
                .entities
                .get(*airfield_id)
                .is_some_and(|af| af.health.current > 0 && !af.dying);
            if !af_ok {
                // Airfield destroyed — release dock and go to Idle.
                // Idle mode will handle AirportBound self-destruct.
                sim.release_airfield_pad(id);
                m.new_mission = AircraftMission::Idle;
            }
            // Otherwise: stay parked, do nothing.
        }

        AircraftMission::ParaDropApproach {
            target_rx,
            target_ry,
            has_revealed_fog,
        } => {
            let outcome = paradrop_mission::tick_approach(
                sim,
                rules,
                id,
                *target_rx,
                *target_ry,
                *has_revealed_fog,
                path_grid,
            );
            m.new_mission = outcome.new_mission;
            m.move_to = outcome.move_to;
            if outcome.play_chute_sound {
                m.paradrop_chute_sound_at = Some((*target_rx, *target_ry));
            }
        }

        AircraftMission::ParaDropOverfly {
            exit_rx,
            exit_ry,
            drop_cooldown,
            landing_state,
            payload_count,
        } => {
            let outcome = paradrop_mission::tick_overfly(
                sim,
                id,
                *exit_rx,
                *exit_ry,
                *drop_cooldown,
                *landing_state,
                *payload_count,
            );
            m.new_mission = outcome.new_mission;
            m.move_to = outcome.move_to;
            m.paradrop_try_drop = outcome.try_drop;
            m.paradrop_payload_count_pre = outcome.payload_count_pre_dec;
            m.paradrop_silent_despawn = outcome.silent_despawn;
        }
    }
    Some(m)
}

/// Enter_Idle_Mode's decision for an aircraft with nothing to do (the Idle
/// mission, and Mission_Attack state 10's `vt+0x484(0, 1)`).
///
/// A return to an airfield also sends the aircraft there in the same call,
/// replacing whatever destination it held (state 10's edge cell): the
/// airborne arm of `AircraftClass::Enter_Idle_Mode @ 0x004176F0` clears it
/// (`Assign_Destination(NULL, 1)` at `0x004179B4`) and assigns the dock that
/// answers (`0x004179D7`).
///
/// RESIDUAL: the rest of 0x004176F0 is VERA's own tree (`idle_mode`): the
/// Restore and the Retreat/Airstrike returns at its head, FootClass's
/// Enter_Idle_Mode, Guard vs Area Guard for a computer house, the team and
/// `+0x3D4` arms, the landed arm (clears the destination and the Target,
/// `0x00417A89..0x00417A9D`), the airborne arm's clear when no dock answers
/// (`Find_Nearest_Friendly_Airfield 0x0041A160` and Move), and the tail's
/// dock and Target clear (`0x00417B1D`, `0x00417B29`). Trigger: an aircraft
/// entering idle mode on the ground, without a dock, in a team, or computer
/// owned. Effect: a computer-house aircraft keeps Guard where native takes
/// Area Guard, a landed or dockless one keeps its destination and Target.
/// Frequency: every computer-house sortie; the rest are rare. The full port
/// is the Enter_Idle_Mode mechanism.
fn enter_idle_mode(
    sim: &Simulation,
    rules: &RuleSet,
    id: u64,
    m: &mut MissionMutation,
) -> Option<()> {
    let entity = sim.substrate.entities.get(id)?;
    let type_str = sim.interner.resolve(entity.type_ref());
    let obj = rules.object(type_str);
    // Weapon-array slot 0 (`TechnoTypeClass+0x898`) is the armed
    // test here rather than `combat_weapon::is_armed`
    // (`TechnoClass::Is_Armed @ 0x00701120`). The two agree on
    // every stock aircraft: no aircraft section authors
    // `TurretCount=`, so the single slot `GetCurrentWeapon` would
    // read is slot 0. UNCHECKED which predicate the native
    // idle/return-to-airfield path uses; zero stock frequency
    // either way.
    let has_weapon = obj.is_some_and(|o| o.primary.is_some());
    let airport_bound = obj.is_some_and(|o| o.airport_bound);
    let is_airborne = entity
        .locomotor
        .as_ref()
        .is_some_and(|l| l.altitude > SIM_ZERO);
    let ammo = entity.aircraft_ammo.as_ref();

    let nearest = find_nearest_airfield_for(
        sim,
        rules,
        entity.owner(),
        entity.type_ref(),
        (entity.position.rx, entity.position.ry),
    );

    let input = idle_mode::IdleModeInput {
        ammo_current: ammo.map_or(-1, |a| a.current),
        ammo_max: ammo.map_or(-1, |a| a.max),
        has_weapon,
        has_target: attack_mission::aircraft_target_present(
            entity.attack_target.as_ref(),
            &sim.substrate.entities,
        ),
        airport_bound,
        is_airborne,
        nearest_airfield: nearest,
    };

    match idle_mode::enter_idle_mode(&input) {
        idle_mode::IdleModeResult::Mission(new_m) => {
            if let AircraftMission::ReturnToBase { airfield_id } = new_m {
                m.assign_destination =
                    Some(crate::sim::components::NavTargetRef::Building { id: airfield_id });
            }
            m.new_mission = new_m;
        }
        idle_mode::IdleModeResult::SelfDestruct => {
            m.self_destruct = true;
        }
    }
    Some(())
}

/// Apply one handler decision. Returns the Mission_Attack fire request.
fn apply_mission_mutation(
    sim: &mut Simulation,
    rules: &RuleSet,
    m: MissionMutation,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
) -> bool {
    // No "Unit lost" here: `AircraftClass::Enter_Idle_Mode @ 0x004176F0`
    // handles the AirportBound-without-airfield case by calling the
    // `Crash` slot `+0x3DC` directly with no attacker (`0x004179FD`,
    // `0x00417B88`; body `0x004DEBB0`, which runs `RecordKill +0xE0` and the
    // trigger events but never `Death_Announcement +0x3B8`) and returns
    // whatever Crash answers. The crashing aircraft then falls to its impact
    // like a shot-down one (`sim::world::crash`). Only a damage kill
    // (`AircraftClass::ReceiveDamage 0x004165C0`, result 4 → `+0x3B8` at
    // `0x00416613`) announces, and that runs through the combat kill loop.
    //
    // Only AircraftClass has that idle mode (vtable `+0x484` = `0x004176F0`);
    // a custom Fly Infantry or Unit retires through VERA's terminal path.
    if m.self_destruct {
        let aircraft = sim
            .substrate
            .entities
            .get(m.id)
            .is_some_and(|entity| entity.category == EntityCategory::Aircraft);
        if aircraft {
            sim.foot_crash(m.id, None, rules);
        } else {
            let infantry_terminal = sim.begin_raw_infantry_death(m.id, None);
            if !infantry_terminal && let Some(entity) = sim.substrate.entities.get_mut(m.id) {
                entity.health.current = 0;
                entity.dying = true;
            }
        }
        if let Some(entity) = sim.substrate.entities.get_mut(m.id) {
            entity.aircraft_mission = None;
        }
        return false;
    }

    if let Some(entity) = sim.substrate.entities.get_mut(m.id) {
        entity.aircraft_mission = Some(m.new_mission.clone());

        if m.ammo_delta != 0 {
            if let Some(ref mut ammo) = entity.aircraft_ammo {
                ammo.current = (ammo.current + m.ammo_delta).max(0).min(ammo.max);
            }
        }
    }
    // The world owners follow the mission write: a refused AirportBound
    // BeginLanding enters idle mode, which must not be overwritten.
    if m.begin_landing {
        sim.begin_fly_landing(m.id, Some(rules));
    }
    if m.begin_takeoff {
        sim.begin_fly_takeoff(m.id, Some(rules));
    }

    if let Some(destination) = m.assign_destination {
        sim.assign_aircraft_attack_destination(m.id, Some(destination), rules);
    }
    if let Some((rx, ry)) = m.move_to {
        // No FASTER stage here: `FlyLocomotionClass` never calls the
        // `FootClass::GetCurrentSpeed` vtable slot (`+0x538`) — see
        // `veterancy::locomotor_consults_current_speed` — so a promoted
        // aircraft flies at its plain `Speed=`.
        let speed = sim
            .substrate
            .entities
            .get(m.id)
            .and_then(|e| {
                let obj = sim.object_type(e.type_ref(), rules)?;
                Some(crate::util::fixed_math::ra2_speed_to_leptons_per_second(
                    obj.speed.max(1),
                ))
            })
            .unwrap_or(SimFixed::from_num(8));
        sim.issue_air_cell_destination(m.id, (rx, ry), speed, Some(rules));
    }

    // Standard Mission_Open is silent at the threshold; this compatibility path
    // remains inert for stock SW carriers unless a mission handler requests it.
    if let Some((rx, ry)) = m.paradrop_chute_sound_at {
        sim.sound_events
            .push(crate::sim::world::SimSoundEvent::ChuteSound { rx, ry });
    }

    // Standard SW cadence is Mission_Rescue returning 5 game frames after one
    // Drop_Payload call; ParaDropWeapon ROF= is not used.
    if m.paradrop_try_drop {
        let aircraft_id = m.id;
        let drop_interval = drop_payload::PARADROP_DROP_INTERVAL_FRAMES;
        let result = drop_payload::try_drop(
            sim,
            rules,
            aircraft_id,
            m.paradrop_payload_count_pre,
            path_grid,
        );
        let frame = sim.session.binary_frame as i32;
        if let Some(entity) = sim.substrate.entities.get_mut(aircraft_id) {
            if let Some(AircraftMission::ParaDropOverfly {
                exit_rx,
                exit_ry,
                payload_count,
                ..
            }) = entity.aircraft_mission.clone()
            {
                let new_mission = match result {
                    drop_payload::DropResult::Success => {
                        // `Drop_Payload @ 0x00415E88..0x00415EAA`: beside the
                        // LandingState write, a drop restarts the rearm timer
                        // (`+0x2EC`) with no duration.
                        entity.rearm_timer.start(frame, 0);
                        AircraftMission::ParaDropOverfly {
                            exit_rx,
                            exit_ry,
                            drop_cooldown: drop_interval,
                            landing_state: drop_payload::LANDING_STATE_RESET,
                            payload_count: payload_count.saturating_sub(1),
                        }
                    }
                    drop_payload::DropResult::ImpassableRetry
                    | drop_payload::DropResult::AttachFailedRetry => {
                        // Leave mission cadence at 0 — retry on the next
                        // Rescue-equivalent execution. payload_count is already
                        // restored via cargo head re-insert.
                        AircraftMission::ParaDropOverfly {
                            exit_rx,
                            exit_ry,
                            drop_cooldown: 0,
                            landing_state: 0,
                            payload_count,
                        }
                    }
                    drop_payload::DropResult::NoCargo => AircraftMission::Idle,
                };
                entity.aircraft_mission = Some(new_mission);
            }
        }
    }

    // Silent despawn for a carrier that exited the playfield with empty cargo.
    // Native is silent too: `AircraftClass::Mission_Rescue @ 0x00415960`
    // never removes the carrier, and the off-playfield removal in
    // `AircraftClass::AI` (`0x00414F93` / `0x00414FD1`) is a bare `UnInit`
    // (`+0xF8`) with no `Death_Announcement` (`+0x3B8`).
    if m.paradrop_silent_despawn {
        let infantry_terminal = sim.begin_raw_infantry_death(m.id, None);
        if let Some(entity) = sim.substrate.entities.get_mut(m.id) {
            if !infantry_terminal {
                entity.health.current = 0;
                entity.dying = true;
            }
            entity.aircraft_mission = None;
        }
    }
    // Call-local dispatch receipt. Preserve the live Target and its existing
    // timing; combat will admit once, emit the burst and commit the suffix.
    m.fire_at.is_some()
}

/// Find nearest airfield for a given aircraft.
/// Returns (stable_id, dock_rx, dock_ry) if found.
fn find_nearest_airfield_for(
    sim: &Simulation,
    rules: &RuleSet,
    owner: crate::sim::intern::InternedId,
    type_ref: crate::sim::intern::InternedId,
    from: (u16, u16),
) -> Option<(u64, u16, u16)> {
    let aircraft_type_str = sim.interner.resolve(type_ref);
    let aircraft_obj = rules.object(aircraft_type_str)?;
    let dock_list = &aircraft_obj.dock;
    if dock_list.is_empty() {
        return None;
    }

    let mut best: Option<(u64, u16, u16, u32)> = None;
    for entity in sim.substrate.entities.values() {
        if entity.category != EntityCategory::Structure {
            continue;
        }
        if entity.health.current == 0 || entity.dying || entity.lifecycle.in_limbo {
            continue;
        }
        if entity.owner() != owner {
            continue;
        }
        let entity_type_str = sim.interner.resolve(entity.type_ref());
        let Some(obj) = rules.object(entity_type_str) else {
            continue;
        };
        if !obj.unit_reload && !obj.helipad {
            continue;
        }
        if !dock_list
            .iter()
            .any(|d| d.eq_ignore_ascii_case(entity_type_str))
        {
            continue;
        }
        let (w, h) = foundation_dimensions(&obj.foundation);
        let dock_rx = entity.position.rx + w / 2;
        let dock_ry = entity.position.ry + h / 2;
        let dx = (from.0 as i32 - dock_rx as i32).unsigned_abs();
        let dy = (from.1 as i32 - dock_ry as i32).unsigned_abs();
        let dist = dx * dx + dy * dy;

        if best.is_none() || dist < best.unwrap().3 {
            best = Some((entity.stable_id(), dock_rx, dock_ry, dist));
        }
    }

    best.map(|(sid, rx, ry, _)| (sid, rx, ry))
}
