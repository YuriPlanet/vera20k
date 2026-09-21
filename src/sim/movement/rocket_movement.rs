//! Rocket locomotor — the six-phase ballistic controller.
//!
//! `RocketLocomotionClass::Process` (`0x006622c0`) selects a type-specific
//! flight table, then processes ignition, tilt, ascent, cruise, terminal and
//! secondary/relaunch.  Combat owns the eventual impact; this module owns only
//! deterministic flight state and reports completion to its caller.

use crate::sim::components::Position;
use crate::sim::debug_event_log::DebugEventKind;
use crate::sim::entity_store::EntityStore;
use crate::sim::intern::InternedId;
use crate::sim::movement::facing_from_delta;
use crate::sim::movement::locomotion::piggyback::LocomotorRuntimePayload;
use crate::util::fixed_math::{
    SIM_ONE, SIM_ZERO, SimFixed, int_distance_to_sim, native_movement_frame_fraction, sim_to_f32,
};

const IGNITION_FRAMES: u16 = 1;
const TILT_FRAMES: u16 = 2;
const DEFAULT_ASCENT_ALTITUDE: SimFixed = SimFixed::lit("400");
const DEFAULT_ACCELERATION: SimFixed = SimFixed::lit("90");
const DEFAULT_TILT_RATE: SimFixed = SimFixed::lit("0.35");

/// Native `ILocomotion::Process` return translated into a caller-owned outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialMovementOutcome {
    Continue,
    Complete,
    Abort,
}

/// What a spawn-manager missile detonates with when it reaches its target.
///
/// gamemd's `RocketLocomotion::Detonate` picks the warhead and damage from
/// `RulesClass` by missile family and the launcher's elite flag, then calls
/// `Apply_area_damage(Owner, warhead, …)`. Carrying those on the flight state
/// keeps the impact independent of the launcher, which may already be dead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct RocketPayload {
    /// Interned warhead section name.
    pub warhead: InternedId,
    /// Impact damage before verses.
    pub damage: i32,
    /// Launcher stable id — kill credit and house attribution.
    pub firer_id: u64,
}

/// State for an in-flight rocket/missile.
///
/// Set when a weapon fires a rocket projectile. Removed (along with the
/// entity) when the rocket detonates at the target.
///
/// Sim-critical fields use `SimFixed` for deterministic lockstep.
/// `pitch` is render-only and stays as `f32`.
/// The six `RocketLocomotionClass::Process` phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RocketPhase {
    /// State 1: create exhaust and prepare the launch vector.
    Ignition,
    /// State 2: interpolate toward the configured launch tilt.
    Tilt,
    /// State 3: climb vertically while accelerating to the flight speed.
    Ascent,
    /// State 4: cruise and track the target.
    Cruise,
    /// State 5: turn through the bounded terminal descent.
    Terminal,
    /// State 6: secondary/relaunch handoff. A zero counter completes flight.
    Secondary,
}

/// Per-projectile flight values selected from the native rocket table.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RocketFlightParameters {
    pub acceleration: SimFixed,
    pub max_speed: SimFixed,
    pub ascent_altitude: SimFixed,
    pub tilt_rate: SimFixed,
    pub relaunches: u8,
}

impl RocketFlightParameters {
    pub fn legacy(speed: SimFixed) -> Self {
        Self {
            acceleration: DEFAULT_ACCELERATION,
            max_speed: speed.max(SIM_ONE),
            ascent_altitude: DEFAULT_ASCENT_ALTITUDE,
            tilt_rate: DEFAULT_TILT_RATE,
            relaunches: 0,
        }
    }
}

/// Serialized state for one in-flight `RocketLocomotionClass` instance.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RocketState {
    pub phase: RocketPhase,
    pub origin_rx: u16,
    pub origin_ry: u16,
    pub target_rx: u16,
    pub target_ry: u16,
    /// Configured maximum speed, retained for the render/debug boundary.
    pub speed: SimFixed,
    /// Current accelerated speed.
    pub current_speed: SimFixed,
    pub altitude: SimFixed,
    pub progress: SimFixed,
    /// Native frames spent in the current phase.
    pub phase_frames: u16,
    pub parameters: RocketFlightParameters,
    /// Render-only attitude. It never drives the simulation step.
    pub pitch: f32,
    /// Impact warhead/damage for spawn-manager missiles. `None` for rockets
    /// whose damage is owned elsewhere.
    #[serde(default)]
    pub payload: Option<RocketPayload>,
}

/// Attach a rocket using the compatibility parameters used by existing callers.
#[cfg(test)]
pub fn attach_rocket_state(
    entities: &mut EntityStore,
    entity_id: u64,
    origin: (u16, u16),
    target: (u16, u16),
    speed: SimFixed,
) -> bool {
    attach_rocket_state_full(
        entities,
        entity_id,
        origin,
        target,
        RocketFlightParameters::legacy(speed),
        None,
    )
}

/// Same as [`attach_rocket_state`], but carries the impact warhead and damage
/// used by spawn-manager missiles (V3ROCKET / DMISL / CMISL).
pub fn attach_rocket_state_with_payload(
    entities: &mut EntityStore,
    entity_id: u64,
    origin: (u16, u16),
    target: (u16, u16),
    speed: SimFixed,
    payload: Option<RocketPayload>,
) -> bool {
    attach_rocket_state_full(
        entities,
        entity_id,
        origin,
        target,
        RocketFlightParameters::legacy(speed),
        payload,
    )
}

fn attach_rocket_state_full(
    entities: &mut EntityStore,
    entity_id: u64,
    origin: (u16, u16),
    target: (u16, u16),
    parameters: RocketFlightParameters,
    payload: Option<RocketPayload>,
) -> bool {
    let Some(entity) = entities.get_mut(entity_id) else {
        return false;
    };

    entity.facing = facing_from_delta(
        i32::from(target.0) - i32::from(origin.0),
        i32::from(target.1) - i32::from(origin.1),
    );
    let rocket_state = RocketState {
        phase: RocketPhase::Ignition,
        origin_rx: origin.0,
        origin_ry: origin.1,
        target_rx: target.0,
        target_ry: target.1,
        speed: parameters.max_speed,
        current_speed: SIM_ZERO,
        altitude: SIM_ZERO,
        progress: SIM_ZERO,
        phase_frames: 0,
        parameters,
        pitch: std::f32::consts::FRAC_PI_2, // Nose up during launch.
        payload,
    };
    entity.rocket_state = Some(rocket_state.clone());
    if let Some(locomotor) = entity.locomotor.as_mut() {
        locomotor.runtime_payload = LocomotorRuntimePayload::Rocket(Some(rocket_state));
    }
    entity.push_debug_event(
        0,
        DebugEventKind::SpecialMovementStart {
            kind: "Rocket".into(),
        },
    );
    true
}

/// Advance one rocket state. The caller performs detonation/despawn on `Complete`.
///
/// The six branches map directly to `RocketLocomotionClass::Process` at
/// `0x006622c0`; this intentionally does not reuse Fly locomotion.
pub fn process_rocket_state(
    rocket: &mut RocketState,
    position: &mut Position,
) -> SpecialMovementOutcome {
    let dt = native_movement_frame_fraction();
    let before = rocket.phase;
    let altitude_before = rocket.altitude;

    match rocket.phase {
        RocketPhase::Ignition => {
            rocket.phase_frames = rocket.phase_frames.saturating_add(1);
            rocket.pitch = std::f32::consts::FRAC_PI_2;
            if rocket.phase_frames >= IGNITION_FRAMES {
                transition(rocket, RocketPhase::Tilt);
            }
        }
        RocketPhase::Tilt => {
            rocket.phase_frames = rocket.phase_frames.saturating_add(1);
            let target_pitch = std::f32::consts::FRAC_PI_2;
            rocket.pitch = (rocket.pitch
                + sim_to_f32(rocket.parameters.tilt_rate) * (target_pitch - rocket.pitch))
                .min(target_pitch);
            if rocket.phase_frames >= TILT_FRAMES {
                transition(rocket, RocketPhase::Ascent);
            }
        }
        RocketPhase::Ascent => {
            accelerate(rocket, dt);
            let climb = rocket.current_speed.max(SIM_ONE) * dt;
            rocket.altitude = (rocket.altitude + climb).min(rocket.parameters.ascent_altitude);
            rocket.pitch = std::f32::consts::FRAC_PI_2;
            if rocket.altitude >= rocket.parameters.ascent_altitude {
                transition(rocket, RocketPhase::Cruise);
            }
        }
        RocketPhase::Cruise => {
            let remaining = advance_horizontal(rocket, position, dt);
            rocket.pitch = horizontal_pitch(rocket, remaining, false);
            if remaining <= terminal_distance(rocket) {
                transition(rocket, RocketPhase::Terminal);
            }
        }
        RocketPhase::Terminal => {
            let remaining = advance_horizontal(rocket, position, dt);
            let descent = rocket.current_speed.max(SIM_ONE) * dt;
            rocket.altitude = (rocket.altitude - descent).max(SIM_ZERO);
            rocket.pitch = horizontal_pitch(rocket, remaining, true);
            if remaining <= SIM_ZERO || rocket.progress >= SIM_ONE {
                position.rx = rocket.target_rx;
                position.ry = rocket.target_ry;
                rocket.altitude = SIM_ZERO;
                transition(rocket, RocketPhase::Secondary);
            }
        }
        RocketPhase::Secondary => {
            if rocket.parameters.relaunches == 0 {
                return SpecialMovementOutcome::Complete;
            }
            rocket.parameters.relaunches -= 1;
            rocket.progress = SIM_ZERO;
            rocket.altitude = SIM_ZERO;
            rocket.current_speed = SIM_ZERO;
            transition(rocket, RocketPhase::Ignition);
        }
    }

    super::foot_coordinate::publish_altitude_change(position, altitude_before, rocket.altitude);
    if rocket.phase != before {
        rocket.phase_frames = 0;
    }
    SpecialMovementOutcome::Continue
}

/// Advance all rockets in supplied live object order.
///
/// Compatibility return: each ID whose flight completed this frame is returned
/// for the combat/lifecycle owner to resolve. Completion stays idempotent while
/// the entity remains alive, matching the previous detonation queue seam.
pub fn tick_rocket_movement(
    entities: &mut EntityStore,
    live_order: &[u64],
    sim_tick: u64,
) -> Vec<u64> {
    let fallback_order;
    let entity_order: &[u64] = if live_order.is_empty() {
        fallback_order = entities.keys_sorted();
        &fallback_order
    } else {
        live_order
    };
    let mut completed = Vec::new();

    for &id in entity_order {
        let Some(entity) = entities.get_mut(id) else {
            continue;
        };
        // External process state belongs to the active Rocket instance. A
        // suspended Rocket must neither move the owner nor replace Teleport's
        // active payload image while its interface is in the piggyback slot.
        if entity.locomotor.as_ref().is_some_and(|loco| {
            loco.active_kind() != crate::rules::locomotor_type::LocomotorKind::Rocket
        }) {
            continue;
        }
        let (outcome, phase_change) = {
            let Some(rocket) = entity.rocket_state.as_mut() else {
                continue;
            };
            let before = rocket.phase;
            let outcome = process_rocket_state(rocket, &mut entity.position);
            let phase_change = (rocket.phase != before).then(|| format!("{:?}", rocket.phase));
            (outcome, phase_change)
        };
        if let (Some(rocket), Some(locomotor)) =
            (entity.rocket_state.as_ref(), entity.locomotor.as_mut())
        {
            locomotor.runtime_payload = LocomotorRuntimePayload::Rocket(Some(rocket.clone()));
        }
        if let Some(phase) = phase_change {
            entity.push_debug_event(
                sim_tick as u32,
                DebugEventKind::SpecialMovementPhase { phase },
            );
        }
        if outcome == SpecialMovementOutcome::Complete {
            completed.push(id);
            entity.push_debug_event(sim_tick as u32, DebugEventKind::SpecialMovementEnd);
        }
    }

    completed
}

fn transition(rocket: &mut RocketState, next: RocketPhase) {
    rocket.phase = next;
    rocket.phase_frames = 0;
}

fn accelerate(rocket: &mut RocketState, dt: SimFixed) {
    rocket.current_speed = (rocket.current_speed + rocket.parameters.acceleration * dt)
        .min(rocket.parameters.max_speed);
}

fn advance_horizontal(rocket: &mut RocketState, position: &mut Position, dt: SimFixed) -> SimFixed {
    accelerate(rocket, dt);
    let total = flight_distance(rocket);
    if total <= SIM_ZERO {
        rocket.progress = SIM_ONE;
        return SIM_ZERO;
    }
    rocket.progress = (rocket.progress + rocket.current_speed * dt / total).min(SIM_ONE);
    update_rocket_position(rocket, position);
    (SIM_ONE - rocket.progress) * total
}

fn terminal_distance(rocket: &RocketState) -> SimFixed {
    // The original starts the terminal turn when the current one-frame travel
    // can reach the target; no RA2 parabolic split is retained here.
    rocket.current_speed.max(SIM_ONE) * native_movement_frame_fraction()
}

fn horizontal_pitch(rocket: &RocketState, remaining: SimFixed, descending: bool) -> f32 {
    let total = flight_distance(rocket).max(SIM_ONE);
    let slope = sim_to_f32(rocket.altitude / total.max(remaining));
    if descending {
        -slope.atan()
    } else {
        slope.atan()
    }
}

fn flight_distance(rocket: &RocketState) -> SimFixed {
    int_distance_to_sim(
        i32::from(rocket.target_rx) - i32::from(rocket.origin_rx),
        i32::from(rocket.target_ry) - i32::from(rocket.origin_ry),
    )
}

fn update_rocket_position(rocket: &RocketState, position: &mut Position) {
    let origin_x = SimFixed::from_num(rocket.origin_rx);
    let origin_y = SimFixed::from_num(rocket.origin_ry);
    let delta_x = SimFixed::from_num(i32::from(rocket.target_rx) - i32::from(rocket.origin_rx));
    let delta_y = SimFixed::from_num(i32::from(rocket.target_ry) - i32::from(rocket.origin_ry));
    position.rx = (origin_x + delta_x * rocket.progress)
        .to_num::<i32>()
        .clamp(0, i32::from(u16::MAX)) as u16;
    position.ry = (origin_y + delta_y * rocket.progress)
        .to_num::<i32>()
        .clamp(0, i32::from(u16::MAX)) as u16;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::entity_store::EntityStore;
    use crate::sim::game_entity::GameEntity;

    #[test]
    fn rocket_visits_the_native_six_phases_before_completion() {
        let mut entities = EntityStore::new();
        entities.insert(GameEntity::test_default(1, "V3RKT", "Soviet", 5, 5));
        assert!(attach_rocket_state(
            &mut entities,
            1,
            (5, 5),
            (20, 5),
            SimFixed::from_num(300)
        ));

        let mut phases = Vec::new();
        let mut completed = false;
        for _ in 0..400 {
            let rocket = entities.get(1).unwrap().rocket_state.as_ref().unwrap();
            if phases.last() != Some(&rocket.phase) {
                phases.push(rocket.phase);
            }
            if tick_rocket_movement(&mut entities, &[], 0).contains(&1) {
                completed = true;
                break;
            }
        }
        assert_eq!(
            phases,
            vec![
                RocketPhase::Ignition,
                RocketPhase::Tilt,
                RocketPhase::Ascent,
                RocketPhase::Cruise,
                RocketPhase::Terminal,
                RocketPhase::Secondary,
            ]
        );
        assert!(completed);
    }

    #[test]
    fn rocket_secondary_relaunches_only_when_the_flight_table_requests_it() {
        let mut position = Position {
            rx: 0,
            ry: 0,
            z: 0,
            exact_z_leptons: None,
            sub_x: SIM_ZERO,
            sub_y: SIM_ZERO,
        };
        let mut rocket = RocketState {
            phase: RocketPhase::Secondary,
            origin_rx: 0,
            origin_ry: 0,
            target_rx: 1,
            target_ry: 0,
            speed: SimFixed::from_num(1),
            current_speed: SimFixed::from_num(1),
            altitude: SIM_ZERO,
            progress: SIM_ONE,
            phase_frames: 0,
            parameters: RocketFlightParameters {
                relaunches: 1,
                ..RocketFlightParameters::legacy(SimFixed::from_num(1))
            },
            pitch: 0.0,
            payload: None,
        };
        assert_eq!(
            process_rocket_state(&mut rocket, &mut position),
            SpecialMovementOutcome::Continue
        );
        assert_eq!(rocket.phase, RocketPhase::Ignition);
        assert_eq!(rocket.parameters.relaunches, 0);
    }

    #[test]
    fn completion_preserves_live_object_order() {
        let mut entities = EntityStore::new();
        for id in [1, 2] {
            let mut entity = GameEntity::test_default(id, "V3RKT", "Soviet", 5, 5);
            entity.rocket_state = Some(RocketState {
                phase: RocketPhase::Secondary,
                origin_rx: 5,
                origin_ry: 5,
                target_rx: 5,
                target_ry: 5,
                speed: SimFixed::from_num(1),
                current_speed: SIM_ZERO,
                altitude: SIM_ZERO,
                progress: SIM_ONE,
                phase_frames: 0,
                parameters: RocketFlightParameters::legacy(SimFixed::from_num(1)),
                pitch: 0.0,
                payload: None,
            });
            entities.insert(entity);
        }
        assert_eq!(tick_rocket_movement(&mut entities, &[2, 1], 0), vec![2, 1]);
    }
}
