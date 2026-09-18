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
pub mod paradrop_mission;
pub mod runtime_contract;

use serde::{Deserialize, Serialize};

use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::AttackTarget;
use crate::sim::mission::MissionTimer;
use crate::sim::movement::air_movement;
use crate::sim::movement::locomotor::AirMovePhase;
use crate::sim::production::foundation_dimensions;
use crate::sim::world::Simulation;
use crate::util::fixed_math::{SIM_ONE, SIM_ZERO, SimFixed};

/// Aircraft mission — determines the high-level behavior each tick.
///
/// Replaces the original engine's MissionClass dispatch for aircraft.
/// Each variant carries its own sub-state for the state machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        /// Set to true when weapon fires during this attack pass.
        /// Ammo is decremented at the START of the next state transition,
        /// not when Fire_At is called. This ensures exactly one ammo per pass.
        has_fired: bool,
        /// Set during strafing attack runs (states 6-9).
        /// Controls whether the aircraft continues forward after firing.
        is_strafe: bool,
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
        pad_index: u8,
    },

    /// Parked on helipad pad — freshly built, waiting for player command.
    /// Dock slot is reserved. Aircraft is Landed, altitude 0.
    /// Exits via Move/Attack command (releases dock, triggers takeoff).
    DockedIdle {
        /// Airfield entity stable_id this aircraft is docked at.
        airfield_id: u64,
        /// Pad index this aircraft is parked on (0-based).
        pad_index: u8,
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

/// Advance aircraft mission state machines for all Fly-locomotor aircraft.
///
/// Called once per tick from `advance_tick()`, after air_movement and before combat.
/// This is the mission orchestration layer — it decides when aircraft fire,
/// where they fly, and what they do after completing an attack pass.
///
/// `path_grid`: threaded from advance_tick. Paradrop's Drop_Payload uses it for
/// drop-cell passability checks. Other missions ignore it for now.
pub fn tick_aircraft_missions(
    sim: &mut Simulation,
    rules: &RuleSet,
    path_grid: Option<&crate::sim::pathfinding::PathGrid>,
) {
    // Phase 1: Snapshot all aircraft with missions.
    struct MissionSnap {
        id: u64,
        mission: AircraftMission,
        release_tail: Option<runtime_contract::AircraftReleaseTail>,
    }

    let snapshots: Vec<MissionSnap> = sim
        .substrate
        .entities
        .values()
        .filter_map(|e| {
            // A Dying aircraft corpse must not run its mission (move, fire,
            // paradrop, reveal fog) for the tick before the end-of-tick drain.
            if e.dying {
                return None;
            }
            let mission = e.aircraft_mission.as_ref()?;
            let loco = e.locomotor.as_ref()?;
            if loco.kind != LocomotorKind::Fly {
                return None;
            }
            Some(MissionSnap {
                id: e.stable_id(),
                mission: mission.clone(),
                release_tail: e.aircraft_release_tail,
            })
        })
        .collect();

    if snapshots.is_empty() {
        return;
    }

    // Phase 2: Process each aircraft through its mission handler.
    struct MissionMutation {
        id: u64,
        new_mission: AircraftMission,
        ammo_delta: i32,
        fire_at: Option<crate::sim::combat::TargetKind>,
        move_to: Option<(u16, u16)>,
        self_destruct: bool,
        set_speed_fraction: Option<SimFixed>,
        set_target_altitude: Option<SimFixed>,
        // Paradrop-specific apply-phase signals.
        paradrop_fire_fog_reveal: bool,
        paradrop_play_chute_sound: bool,
        paradrop_chute_sound_at: Option<(u16, u16)>,
        paradrop_try_drop: bool,
        paradrop_payload_count_pre: u8,
        paradrop_silent_despawn: bool,
        release_tail: Option<runtime_contract::AircraftReleaseTail>,
        clear_attack_target: bool,
    }

    let mut mutations: Vec<MissionMutation> = Vec::new();
    let now = sim.session.binary_frame;

    for snap in &snapshots {
        let mut m = MissionMutation {
            id: snap.id,
            new_mission: snap.mission.clone(),
            ammo_delta: 0,
            fire_at: None,
            move_to: None,
            self_destruct: false,
            set_speed_fraction: None,
            set_target_altitude: None,
            paradrop_fire_fog_reveal: false,
            paradrop_play_chute_sound: false,
            paradrop_chute_sound_at: None,
            paradrop_try_drop: false,
            paradrop_payload_count_pre: 0,
            paradrop_silent_despawn: false,
            release_tail: snap.release_tail,
            clear_attack_target: false,
        };

        match &snap.mission {
            AircraftMission::Idle => {
                let entity = match sim.substrate.entities.get(snap.id) {
                    Some(e) => e,
                    None => continue,
                };
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
                let has_weapon = obj.map_or(false, |o| o.primary.is_some());
                let airport_bound = obj.map_or(false, |o| o.airport_bound);
                let is_airborne = entity
                    .locomotor
                    .as_ref()
                    .map_or(false, |l| l.altitude > SIM_ZERO);
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
                    has_target: entity.attack_target.is_some(),
                    airport_bound,
                    is_airborne,
                    nearest_airfield: nearest,
                };

                match idle_mode::enter_idle_mode(&input) {
                    idle_mode::IdleModeResult::Mission(new_m) => {
                        m.new_mission = new_m;
                    }
                    idle_mode::IdleModeResult::SelfDestruct => {
                        m.self_destruct = true;
                    }
                }
            }

            AircraftMission::Attack {
                sub_state,
                has_fired,
                is_strafe,
            } => {
                if *sub_state == 1 && m.release_tail.is_some() {
                    let mut tail = m.release_tail.expect("checked above");
                    tail.consume_final_release();
                    m.release_tail = Some(tail);
                    // Native frames 369 -> 370: the last release enters
                    // state 10 with the target still retained.
                    m.new_mission = AircraftMission::Attack {
                        sub_state: 10,
                        has_fired: *has_fired,
                        is_strafe: false,
                    };
                } else if *sub_state == 10
                    && m.release_tail.is_some_and(|tail| tail.clear_target_next)
                {
                    let mut tail = m.release_tail.expect("checked above");
                    tail.clear_target();
                    m.release_tail = Some(tail);
                    // Native frames 370 -> 371: state 10 persists while the
                    // target clears; completion remains latched.
                    m.clear_attack_target = true;
                    m.new_mission = AircraftMission::Attack {
                        sub_state: 10,
                        has_fired: *has_fired,
                        is_strafe: false,
                    };
                } else {
                    let result = attack_mission::tick_attack_state(
                        &sim.substrate.entities,
                        rules,
                        &sim.interner,
                        snap.id,
                        *sub_state,
                        *has_fired,
                        *is_strafe,
                    );
                    m.new_mission = result.new_mission;
                    m.ammo_delta = result.ammo_delta;
                    m.fire_at = result.fire_at;
                    m.move_to = result.move_to;
                    if result.fire_at.is_some()
                        && matches!(&m.new_mission, AircraftMission::Attack { sub_state: 1, .. })
                    {
                        m.release_tail =
                            Some(runtime_contract::AircraftReleaseTail::after_final_release());
                    }
                }

                // Dive bombing: when in attack states 3-4, lower altitude to 1/3 cruise.
                if matches!(*sub_state, 3 | 4) {
                    if let Some(entity) = sim.substrate.entities.get(snap.id) {
                        if let Some(loco) = &entity.locomotor {
                            let cruise = loco.target_altitude;
                            let dive_alt = cruise / SimFixed::from_num(3);
                            m.set_target_altitude = Some(dive_alt);
                        }
                    }
                } else if *sub_state == 10 {
                    // Restore cruise altitude on RTB.
                    if let Some(entity) = sim.substrate.entities.get(snap.id) {
                        let type_str = sim.interner.resolve(entity.type_ref());
                        if let Some(obj) = rules.object(type_str) {
                            let cruise =
                                crate::sim::movement::locomotor::LocomotorState::from_object_type(
                                    obj,
                                    rules.general.flight_level,
                                    sim.session.binary_frame,
                                )
                                .target_altitude;
                            m.set_target_altitude = Some(cruise);
                        }
                    }
                    m.set_speed_fraction = Some(SIM_ONE);
                }

                // Speed tiers based on distance to target.
                // Cell targets resolve to cell-center coords via the helper.
                if matches!(*sub_state, 3 | 4) {
                    if let Some(entity) = sim.substrate.entities.get(snap.id) {
                        if let Some(status) =
                            crate::sim::aircraft::attack_mission::aircraft_target_status(
                                entity.attack_target.as_ref(),
                                &sim.substrate.entities,
                            )
                        {
                            let dx = (entity.position.rx as i32 - status.rx as i32).abs();
                            let dy = (entity.position.ry as i32 - status.ry as i32).abs();
                            let dist_cells = dx.max(dy);

                            let speed_frac = if dist_cells < 1 {
                                SIM_ZERO
                            } else if dist_cells < 2 {
                                SimFixed::lit("0.5")
                            } else if dist_cells < 3 {
                                SimFixed::lit("0.75")
                            } else {
                                SIM_ONE
                            };
                            m.set_speed_fraction = Some(speed_frac);
                        }
                    }
                }
            }

            AircraftMission::Guard => {
                m.set_speed_fraction = Some(SIM_ONE);
                let entity = match sim.substrate.entities.get(snap.id) {
                    Some(e) => e,
                    None => continue,
                };
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
                    m.new_mission = AircraftMission::Attack {
                        sub_state: 0,
                        has_fired: false,
                        is_strafe: false,
                    };
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
                        let airport_bound =
                            rules.object(type_str).map_or(false, |o| o.airport_bound);
                        if airport_bound {
                            m.self_destruct = true;
                        }
                    }
                }
            }

            AircraftMission::ReturnToBase { airfield_id } => {
                let entity = match sim.substrate.entities.get(snap.id) {
                    Some(e) => e,
                    None => continue,
                };
                let af_ok = sim
                    .substrate
                    .entities
                    .get(*airfield_id)
                    .is_some_and(|af| af.health.current > 0 && !af.dying);
                if !af_ok {
                    m.new_mission = AircraftMission::Idle;
                    mutations.push(m);
                    continue;
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
                    // by the try_reserve return once a pad is granted.
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
                let entity = match sim.substrate.entities.get(snap.id) {
                    Some(e) => e,
                    None => continue,
                };
                let air_phase = entity.locomotor.as_ref().map(|l| l.air_phase);
                let ammo = entity.aircraft_ammo.as_ref();
                let ammo_current = ammo.map_or(0, |a| a.current);
                let ammo_max = ammo.map_or(0, |a| a.max);
                let reload_rate = rules.general.reload_rate_ticks;

                match sub_state {
                    0 => {
                        // Wait for dock slot.
                        let af_type_ref = sim
                            .substrate
                            .entities
                            .get(*airfield_id)
                            .map_or(entity.type_ref(), |af| af.type_ref());
                        let type_str = sim.interner.resolve(af_type_ref);
                        let max_slots = rules
                            .object(type_str)
                            .map(|o| o.number_of_docks.max(1))
                            .unwrap_or(1);
                        // Native AircraftClass::IsCellOccupied reaches the
                        // unconditional Winged Cell leaf first; dock ownership
                        // and first-free pad reservation remain this wrapper's
                        // meaningful admission gates.
                        if runtime_contract::aircraft_landing_cell_leaf_clear()
                            && let Some(reserved_pad) = sim.production.airfield_docks.try_reserve(
                                *airfield_id,
                                snap.id,
                                max_slots,
                            )
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
                                    let foundation = crate::sim::production::foundation_dimensions(
                                        &obj.foundation,
                                    );
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
                            if let Some(entity) = sim.substrate.entities.get_mut(snap.id)
                                && let Some(ref mut ammo) = entity.aircraft_ammo
                            {
                                ammo.target_pad = Some(reserved_pad);
                            }
                        }
                    }
                    1 => {
                        // Descending — wait for Landed.
                        if air_phase == Some(AirMovePhase::Landed) {
                            m.new_mission = AircraftMission::Docking {
                                airfield_id: *airfield_id,
                                sub_state: 2,
                                reload_timer: MissionTimer::armed(now, reload_rate),
                                pad_index: *pad_index,
                            };
                        }
                    }
                    2 => {
                        // Reloading.
                        if reload_timer.due(now) {
                            m.ammo_delta = 1;
                            if ammo_current + 1 >= ammo_max {
                                // Fully reloaded → launch.
                                sim.production.airfield_docks.release(snap.id);
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
                        } else {
                            // Carry the same frame-anchored timer (no decrement).
                            m.new_mission = AircraftMission::Docking {
                                airfield_id: *airfield_id,
                                sub_state: 2,
                                reload_timer: *reload_timer,
                                pad_index: *pad_index,
                            };
                        }
                    }
                    3 => {
                        // Launching — wait for cruising altitude.
                        if air_phase == Some(AirMovePhase::Cruising) {
                            m.new_mission = AircraftMission::Idle;
                            // Clear target_pad now that the dock is released.
                            if let Some(entity) = sim.substrate.entities.get_mut(snap.id)
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
                let entity = match sim.substrate.entities.get(snap.id) {
                    Some(e) => e,
                    None => continue,
                };
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
                    sim.production.airfield_docks.release(snap.id);
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
                    snap.id,
                    *target_rx,
                    *target_ry,
                    *has_revealed_fog,
                    path_grid,
                );
                m.new_mission = outcome.new_mission;
                m.move_to = outcome.move_to;
                m.paradrop_fire_fog_reveal = outcome.fire_fog_reveal;
                m.paradrop_play_chute_sound = outcome.play_chute_sound;
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
                    snap.id,
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

        if !matches!(&m.new_mission, AircraftMission::Attack { .. }) {
            m.release_tail = None;
        }

        mutations.push(m);
    }

    // Phase 3: Apply mutations.
    for m in &mutations {
        // No "Unit lost" here: `AircraftClass::Enter_Idle_Mode @ 0x004176F0`
        // handles the AirportBound-without-airfield case by calling the
        // `Crash` slot `+0x3DC` directly (`0x004179FD`, `0x00417B88`; body
        // `0x004DEBB0`, which runs `RecordKill +0xE0` and the trigger events
        // but never `Death_Announcement +0x3B8`), and the eventual impact in
        // `AircraftClass::AI` (`0x00414BB0`, height < -400) is `RecordKill` +
        // `UnInit` (`0x00414E1F`). Only a damage kill (`AircraftClass::
        // ReceiveDamage 0x004165C0`, result 4 → `+0x3B8` at `0x00416613`)
        // announces, and that runs through the combat kill loop.
        if m.self_destruct {
            let infantry_terminal = sim.begin_raw_infantry_death(m.id, None);
            sim.substrate.entities.note_dying_transition();
            if let Some(entity) = sim.substrate.entities.get_mut(m.id) {
                if !infantry_terminal {
                    entity.health.current = 0;
                    entity.dying = true;
                }
                entity.aircraft_mission = None;
            }
            continue;
        }

        if let Some(entity) = sim.substrate.entities.get_mut(m.id) {
            entity.aircraft_mission = Some(m.new_mission.clone());
            entity.aircraft_release_tail = m.release_tail;

            if m.clear_attack_target {
                entity.attack_target = None;
            }

            if m.ammo_delta != 0 {
                if let Some(ref mut ammo) = entity.aircraft_ammo {
                    ammo.current = (ammo.current + m.ammo_delta).max(0).min(ammo.max);
                }
            }

            if let Some(speed_frac) = m.set_speed_fraction {
                if let Some(ref mut loco) = entity.locomotor {
                    loco.speed_fraction = speed_frac;
                }
            }

            if let Some(target_alt) = m.set_target_altitude {
                if let Some(ref mut loco) = entity.locomotor {
                    loco.target_altitude = target_alt;
                    if loco.altitude > target_alt {
                        loco.air_phase = AirMovePhase::Descending;
                    } else if loco.altitude < target_alt {
                        loco.air_phase = AirMovePhase::Ascending;
                    }
                }
            }

            // Docking sub_state 1: set air phase to Descending.
            if let AircraftMission::Docking { sub_state: 1, .. } = &m.new_mission {
                if let Some(ref mut loco) = entity.locomotor {
                    loco.air_phase = AirMovePhase::Descending;
                }
                entity.movement_target = None;
            }
            // Docking sub_state 3: set air phase to Ascending (launch).
            if let AircraftMission::Docking { sub_state: 3, .. } = &m.new_mission {
                if let Some(ref mut loco) = entity.locomotor {
                    loco.air_phase = AirMovePhase::Ascending;
                }
            }
        }
    }

    // Phase 4: Issue air move commands and fire commands.
    let air_moves: Vec<(u64, u16, u16)> = mutations
        .iter()
        .filter_map(|m| m.move_to.map(|(rx, ry)| (m.id, rx, ry)))
        .collect();
    for (id, rx, ry) in air_moves {
        // No FASTER stage here: `FlyLocomotionClass` never calls the
        // `FootClass::GetCurrentSpeed` vtable slot (`+0x538`) — see
        // `veterancy::locomotor_consults_current_speed` — so a promoted
        // aircraft flies at its plain `Speed=`.
        let speed = sim
            .substrate
            .entities
            .get(id)
            .and_then(|e| {
                let obj = sim.object_type(e.type_ref(), rules)?;
                Some(crate::util::fixed_math::ra2_speed_to_leptons_per_second(
                    obj.speed.max(1),
                ))
            })
            .unwrap_or(SimFixed::from_num(8));
        air_movement::issue_air_move_command(&mut sim.substrate.entities, id, (rx, ry), speed);
    }

    // Fire commands: set attack_target so combat system fires this tick.
    // Carries TargetKind so Cell-target force-fire fires at coords, not entity.
    let fire_commands: Vec<(u64, crate::sim::combat::TargetKind)> = mutations
        .iter()
        .filter_map(|m| m.fire_at.map(|tk| (m.id, tk)))
        .collect();
    for (attacker_id, target_kind) in fire_commands {
        if let Some(entity) = sim.substrate.entities.get_mut(attacker_id) {
            entity.attack_target = Some(match target_kind {
                crate::sim::combat::TargetKind::Entity(id) => AttackTarget::new(id),
                crate::sim::combat::TargetKind::Cell(rx, ry) => AttackTarget::for_cell(rx, ry),
            });
        }
    }

    // Phase 5: Paradrop apply phase.
    // Standard Mission_Open is silent at the threshold; this compatibility path
    // remains inert for stock SW carriers unless a mission handler requests it.
    let chute_sounds: Vec<(u16, u16)> = mutations
        .iter()
        .filter_map(|m| m.paradrop_chute_sound_at)
        .collect();
    for (rx, ry) in chute_sounds {
        sim.sound_events
            .push(crate::sim::world::SimSoundEvent::ChuteSound { rx, ry });
    }

    // try_drop attempts. Standard SW cadence is Mission_Rescue returning 5
    // game frames after one Drop_Payload call; ParaDropWeapon ROF= is not used.
    let drop_attempts: Vec<(u64, u8)> = mutations
        .iter()
        .filter(|m| m.paradrop_try_drop)
        .map(|m| (m.id, m.paradrop_payload_count_pre))
        .collect();
    for (aircraft_id, payload_pre) in drop_attempts {
        let drop_interval = drop_payload::PARADROP_DROP_INTERVAL_FRAMES;

        let result = drop_payload::try_drop(sim, rules, aircraft_id, payload_pre, path_grid);

        if let Some(entity) = sim.substrate.entities.get_mut(aircraft_id) {
            if let Some(AircraftMission::ParaDropOverfly {
                exit_rx,
                exit_ry,
                payload_count,
                ..
            }) = entity.aircraft_mission.clone()
            {
                let new_mission = match result {
                    drop_payload::DropResult::Success => AircraftMission::ParaDropOverfly {
                        exit_rx,
                        exit_ry,
                        drop_cooldown: drop_interval,
                        landing_state: drop_payload::LANDING_STATE_RESET,
                        payload_count: payload_count.saturating_sub(1),
                    },
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

    // Silent despawns for carriers that exited the playfield with empty cargo.
    // Native is silent too: `AircraftClass::Mission_Rescue @ 0x00415960`
    // never removes the carrier, and the off-playfield removal in
    // `AircraftClass::AI` (`0x00414F93` / `0x00414FD1`) is a bare `UnInit`
    // (`+0xF8`) with no `Death_Announcement` (`+0x3B8`).
    for m in &mutations {
        if m.paradrop_silent_despawn {
            let infantry_terminal = sim.begin_raw_infantry_death(m.id, None);
            sim.substrate.entities.note_dying_transition();
            if let Some(entity) = sim.substrate.entities.get_mut(m.id) {
                if !infantry_terminal {
                    entity.health.current = 0;
                    entity.dying = true;
                }
                entity.aircraft_mission = None;
            }
        }
    }
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
