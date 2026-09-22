//! Locomotor component and access to each movement mechanism's runtime.
//!
//! `MovementTarget` owns destination/path data. Mechanism payloads own native
//! controller state; shared fields retain movement and presentation adapters.
//! Current world Z is authoritative in Object coordinates. `altitude` is its
//! bounded cache, while Fly's integer target lives in its own payload.

use crate::rules::jumpjet_params::JumpjetParams;
use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::object_type::ObjectType;
use crate::sim::movement::locomotion::LocomotorSlot;
use crate::sim::movement::locomotion::piggyback::{
    self, EndGateContext, LocomotorRuntimePayload, StashedLocomotor,
};
use crate::sim::movement::slope_transition::SlopeTransitionState;
use crate::util::fixed_math::{SIM_ZERO, SimFixed, sim_from_f32};

/// Which spatial layer the unit currently occupies.
///
/// Affects occupancy checks, rendering, and targeting. Ground units block
/// ground cells; air units occupy the air layer and can fly over obstacles.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum MovementLayer {
    /// Standard ground surface.
    Ground,
    /// Elevated bridge deck above the ground/water layer.
    Bridge,
    /// Airborne (aircraft, jumpjets at altitude).
    Air,
    /// Burrowed underground (tunnel units).
    Underground,
}

/// Phase within a ground mover's movement cycle.
///
/// **VERA-internal, gamemd equivalent UNCHECKED.** The earlier claim here — a
/// "7-state machine matching WalkLocomotionClass (+0x50)" — is refuted:
/// `WalkLocomotionClass::Process` @ `0x0075AC80` toggles a byte at
/// ILocomotion-frame `+0x31` and delegates to `ProcessMovement` @ `0x0075AEC0`,
/// whose 1505 instructions contain no object-relative `+0x50` and no state
/// switch; `DriveLocomotionClass::Process` @ `0x004B0500` branches on `+0x54`
/// and `+0x5F` instead. The `+0x50` that does exist in that family is Drive's
/// **piggyback slot**, in the *IPiggyback* frame at LocomotorBase+0x18
/// (`Begin_Piggyback` @ `0x004AF8E0`; `Save` @ `0x004AF800` reaches the same
/// slot as LocomotorBase `+0x68`) — a different frame from the ILocomotion one
/// these phases would live in. The only numbered 0..=6 locomotor state machine
/// in the binary is Jumpjet's, at *ILocomotion*-frame `+0x4C`, switched in
/// `JumpjetLocomotionClass::Process` @ `0x0054AEC0`.
///
/// The variants below describe Drive behaviours — cruise speed, cell entry,
/// crush, bridge — which Walk does not implement, so this is VERA's own
/// ground-mover phasing. Trigger: every ground mover, every tick. Player
/// effect: none identified; the phasing drives VERA's own step machine.
/// Frequency: continuous. Downstream risk: it is the shape rows GSI-06.13 and
/// GSI-06.14 will have to reconcile with the real `Process` bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GroundMovePhase {
    /// State 0: No movement order. Unit is stationary at a valid cell position.
    /// Entry: set when speed reaches 0 and unit completes all movement at cell center.
    Idle,
    /// State 1: Post-paradrop landing. Speed set to 1.0, velocity zeroed.
    /// Transitions to Accelerating when movement begins.
    Landed,
    /// State 2: Ramping up speed toward cruise. Entered when a new cell-to-cell
    /// step begins — facing is updated and speed starts increasing.
    Accelerating,
    /// State 3: At cruise speed, following path. Entered from Accelerating when
    /// unit reaches cruise speed, or from CellEntry after successful transition.
    Cruising,
    /// State 4: Core path-following tick with distance-based speed zones.
    /// Handles approach deceleration and arrival detection (< 20 leptons).
    PathFollow,
    /// State 5: Cell-to-cell transition step. Handles obstacle detection,
    /// crush logic, passability checks, and bridge-specific behaviors.
    CellEntry,
    /// State 6: Decelerating to halt. Target speed zeroed, deceleration in
    /// UpdatePosition brings speed to 0, then transitions to Idle.
    Stopping,
    /// Blocked by another entity or impassable terrain. Waiting for repath.
    /// Not a state in the original engine's +0x50 field, but tracked here
    /// for diagnostics and UI feedback.
    Blocked,
}

/// Derived view of Fly height versus target for legacy aircraft missions.
/// This is neither serialized controller state nor native takeoff/landing flags.
///
/// Fly units cycle through TakingOff → Cruising → Descending → Landed. A
/// Jumpjet does not use it: its phase is the native state field,
/// `JumpjetRuntime::phase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AirMovePhase {
    /// On the ground, not yet airborne.
    Landed,
    /// Ascending from ground to cruise/hover altitude.
    Ascending,
    /// At cruise altitude, moving toward destination.
    Cruising,
    /// Descending from cruise altitude back to ground.
    Descending,
}

/// Runtime locomotor state attached to each movable ECS entity.
///
/// Created from `ObjectType` at spawn time. The movement system reads this
/// to decide how to process the entity's `MovementTarget` each tick.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LocomotorState {
    /// Which locomotor class is currently active.
    pub kind: LocomotorKind,
    /// The locomotor class this unit was built with — the installed slot.
    ///
    /// Natively a unit holds exactly one locomotor interface, created once in
    /// its class constructor from the type's `Locomotor=` CLSID; there is no
    /// second slot and no re-selection. `kind` is the class *currently driving*
    /// the unit, which differs from this only while a piggyback stash is active.
    pub slot: LocomotorSlot,
    /// Whether this locomotor is powered.
    ///
    /// Natively a plain flag on the locomotor instance, set by `Power_On` /
    /// `Power_Off` and read by `Is_Powered`; both setters also re-dispatch to
    /// another slot, which has no verified effect and is not modelled. Defaults
    /// to on — an unpowered locomotor is a state something must actively put a
    /// unit into.
    pub powered: bool,
    /// One boxed suspended locomotor runtime.
    ///
    /// For CMIN drive phases, `kind` becomes Drive and this stores the complete
    /// primary Teleport runtime until the active Drive locomotor is ok to end.
    #[serde(default)]
    pub piggyback: Option<StashedLocomotor>,
    /// Class-local state of the locomotor currently driving this entity.
    /// Piggyback BEGIN/END transfers this value with the complete runtime.
    pub runtime_payload: LocomotorRuntimePayload,
    /// Which spatial layer the unit currently occupies.
    pub layer: MovementLayer,
    /// Current movement phase (for ground movers).
    pub phase: GroundMovePhase,
    /// Speed multiplier applied on top of ObjectType.speed.
    /// 1.0 for most units, 0.65 for Hover, etc.
    pub speed_multiplier: SimFixed,
    /// Mission-controlled speed fraction (0.0–1.0). Acts as the *target* speed
    /// for Fly aircraft — `fly_current_speed` ramps toward this value.
    /// Set by aircraft missions for dive bombing deceleration and speed tiers.
    /// Default 1.0 (full speed).
    pub speed_fraction: SimFixed,
    /// Actual flight speed fraction (0.0–1.0) for Fly aircraft.
    /// Ramps toward `speed_fraction` (which acts as target) by +/-0.1 per tick,
    /// matching the original engine's TargetSpeed/CurrentSpeed system.
    /// Jumpjets use their own `jumpjet_current_speed` instead.
    pub fly_current_speed: SimFixed,
    /// Bounded altitude cache for movement/presentation adapters. Fly's exact
    /// current height comes from Object Z and terrain; its target is in FlyRuntime.
    pub altitude: SimFixed,
    /// Cached jumpjet flight speed (only for Jumpjet locomotor).
    pub jumpjet_speed: SimFixed,
    /// Jumpjet acceleration rate (JumpjetAccel). Deceleration = accel * 1.5.
    pub jumpjet_accel: SimFixed,
    /// Current speed during jumpjet flight (ramps via accel/decel).
    pub jumpjet_current_speed: SimFixed,
    /// Max lateral deviation in leptons during hover wobble (JumpjetDeviation).
    pub jumpjet_deviation: i32,
    /// Combined crash descent speed: climb + crash (leptons/sec, scaled).
    pub jumpjet_crash_speed: SimFixed,
    /// Facing change rate per tick while airborne (JumpjetTurnRate).
    pub jumpjet_turn_rate: i32,
    /// Stay airborne after reaching destination (BalloonHover=yes).
    pub balloon_hover: bool,
    /// Can attack while hovering in place (HoverAttack=yes).
    pub hover_attack: bool,
    /// Which terrain type this unit traverses (from rules.ini SpeedType=).
    /// Used to select the correct TerrainCostGrid for cost-aware pathfinding.
    pub speed_type: SpeedType,
    /// Pathfinder movement zone — determines crush capability and special routing.
    /// Cached from ObjectType at spawn to avoid per-tick RuleSet lookups.
    pub movement_zone: MovementZone,
    /// Body rotation speed — ROT value from rules.ini (degrees/frame at 15fps).
    /// Used for gradual hull turning before movement. 0 = instant turn.
    /// Infantry always turn instantly regardless of this value (RA2 behavior).
    pub rot: i32,
    /// Air movement progress in cells (0.0 → 1.0 per cell step).
    /// Air movement uses cell-based progress separately from the lepton
    /// advancement used by ground movement. This field is only meaningful
    /// for air-layer entities during horizontal flight.
    pub air_progress: SimFixed,
    /// Infantry lateral wobble phase (radians). Sine wave applied perpendicular
    /// to facing direction during walking, creating natural visual sway/spacing.
    /// **VERA-internal, gamemd equivalent UNCHECKED.** No `+0x88` operand
    /// appears anywhere in `WalkLocomotionClass::ProcessMovement` @
    /// `0x0075AEC0`. A double at object `+0x88` does exist — in
    /// `JumpjetLocomotionClass`, applied by `Update_Coordinates_And_Altitude`
    /// @ `0x0054D0F0` in states 2 and 3.
    /// Render-only (f32) — does not affect simulation determinism.
    #[serde(skip, default)]
    pub infantry_wobble_phase: f32,
    /// Within-cell walk destination for infantry. Set when a sub-cell is allocated
    /// during cell entry. The locomotor walks the infantry toward this point after
    /// the path is exhausted.
    pub subcell_dest: Option<(SimFixed, SimFixed)>,
    /// Hover throttle `[0, 1]` — the persisted speed fraction of the hover
    /// locomotor's SpeedUpdate model (see `sim/movement/hover.rs`). Lives on the
    /// locomotor (not `MovementTarget`) so it survives path recomputes: a hover
    /// unit re-pathed mid-route keeps its momentum instead of re-spinning up.
    /// Zero at spawn (units start from rest) and reset to zero on full stop.
    /// Unused by non-Hover locomotors.
    #[serde(default)]
    pub hover_throttle: SimFixed,

    /// Hover speed *request* — the unramped throttle target, distinct from
    /// `hover_throttle` (the ramped value that lags it).
    ///
    /// Persisted solely so the Mission readiness producer can read it: the
    /// native readiness slot reads the request, not the ramp, and the ramp lags
    /// by up to ~27 ticks on the brake side, which is the direction that would
    /// wrongly report "moving". Takes only three values (0, 0.5, 1).
    /// Unused by non-Hover locomotors.
    #[serde(default)]
    pub hover_speed_request: SimFixed,
    /// Hover vertical-spring state — the velocity-like bob offset of the
    /// damped-spring altitude controller (see `hover::hover_vertical_tick`).
    /// Pairs with `altitude`, which for hover units holds the visible float
    /// height above ground. Unused by non-Hover locomotors.
    #[serde(default)]
    pub hover_bob_offset: SimFixed,
}

impl LocomotorState {
    /// Create a LocomotorState from an ObjectType's parsed rules.ini data.
    ///
    /// Class runtime starts from its native constructor; Fly requests resolve
    /// FlightLevel when takeoff is admitted, not during construction.
    pub fn from_object_type(obj: &ObjectType, binary_frame: u32) -> Self {
        let kind: LocomotorKind = obj.locomotor;
        let sim_one: SimFixed = SimFixed::from_num(1);

        let (layer, speed_multiplier): (MovementLayer, SimFixed) = match kind {
            // Ground family — all use Ground layer
            LocomotorKind::Drive => (MovementLayer::Ground, sim_one),
            LocomotorKind::Walk => (MovementLayer::Ground, sim_one),
            // Hover cruises at full base Speed (throttle 1.0 at cruise), NOT the
            // old made-up 0.65x. The accel/brake throttle ramp + continuous XY
            // integrator land in later M2 phases (see sim/movement/hover.rs).
            LocomotorKind::Hover => (MovementLayer::Ground, sim_one),
            LocomotorKind::Mech => (MovementLayer::Ground, sim_one),
            LocomotorKind::Ship => (MovementLayer::Ground, sim_one),

            // Air family — use Air layer with altitude state
            LocomotorKind::Fly => (MovementLayer::Air, sim_one),
            LocomotorKind::Jumpjet => (MovementLayer::Air, sim_one),
            LocomotorKind::Rocket => (MovementLayer::Air, sim_one),

            // Special — stubbed as ground for now
            LocomotorKind::Teleport => (MovementLayer::Ground, sim_one),
            // Inert TS variants — unconstructible, retained for discriminant
            // stability only. See LocomotorKind::Tunnel.
            LocomotorKind::Tunnel => (MovementLayer::Ground, sim_one),
            LocomotorKind::DropPod => (MovementLayer::Air, sim_one),
            LocomotorKind::Parachute => (MovementLayer::Air, sim_one),
        };

        let jj_speed = if kind == LocomotorKind::Jumpjet {
            obj.jumpjet_params.speed
        } else {
            SIM_ZERO
        };

        // gamemd-derived: every `TechnoType` carries the `+0xD70`..`+0xD90`
        // jumpjet block, but only the Jumpjet locomotor ever copies it out —
        // the parameter copy at `0x0054AD30` pulls `+0xD70` (turn rate),
        // `+0xD74` (speed), `+0xD78` (climb), `+0xD7C` (crash), `+0xD80`
        // (height), `+0xD84` (accel), `+0xD88` (wobbles), `+0xD90`
        // (deviation) and `+0xD8C` (no-wobbles) off the type in one run. A
        // Drive/Fly/Hover locomotor has no such fields in gamemd and never
        // reads the type's, so they stay inert here for every other kind.
        // VERA-internal: this one struct serves every locomotor kind, so the
        // non-jumpjet arms still need *a* value. Zero for the three that mean
        // "off", and 4 for the turn rate — the `TechnoTypeClass::Constructor`
        // seed at `0x007115AE`, which is also what this line produced before
        // the block became unconditional.
        let jj: Option<&JumpjetParams> =
            (kind == LocomotorKind::Jumpjet).then_some(&obj.jumpjet_params);
        let jj_accel: SimFixed = jj.map_or(SIM_ZERO, |p| sim_from_f32(p.accel));
        let jj_deviation: i32 = jj.map_or(0, |p| p.deviation);
        let jj_crash_speed: SimFixed = jj.map_or(SIM_ZERO, |p| {
            (sim_from_f32(p.climb) + sim_from_f32(p.crash)) * SimFixed::from_num(15)
        });
        let jj_turn_rate: i32 = jj.map_or(4, |p| p.turn_rate);

        Self {
            kind,
            slot: LocomotorSlot::from_kind(kind),
            powered: true,
            piggyback: None,
            runtime_payload: {
                // `Link_To_Object @ 0x0054AD30` copies the type's jumpjet block and
                // builds the locomotor facing at its `JumpjetTurnRate=`.
                let mut payload = LocomotorRuntimePayload::for_kind(kind, binary_frame);
                if let LocomotorRuntimePayload::Jumpjet(runtime) = &mut payload {
                    runtime.link(&obj.jumpjet_params);
                }
                if let LocomotorRuntimePayload::Fly(runtime) = &mut payload {
                    runtime.link(
                        obj.category == crate::rules::object_type::ObjectCategory::Aircraft
                            && obj.airport_bound,
                    );
                }
                payload
            },
            layer,
            phase: GroundMovePhase::Idle,

            speed_multiplier,
            speed_fraction: sim_one,
            fly_current_speed: SIM_ZERO,
            altitude: SIM_ZERO,

            jumpjet_speed: jj_speed,
            jumpjet_accel: jj_accel,
            jumpjet_current_speed: SIM_ZERO,
            jumpjet_deviation: jj_deviation,
            jumpjet_crash_speed: jj_crash_speed,
            jumpjet_turn_rate: jj_turn_rate,
            balloon_hover: obj.balloon_hover,
            hover_attack: obj.hover_attack,
            speed_type: obj.speed_type,
            movement_zone: obj.movement_zone,
            rot: obj.turret_rot,
            air_progress: SIM_ZERO,
            infantry_wobble_phase: 0.0,
            subcell_dest: None,
            hover_throttle: SIM_ZERO,
            hover_speed_request: SIM_ZERO,
            hover_bob_offset: SIM_ZERO,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_kind(kind: LocomotorKind) -> Self {
        Self::for_test_kind_at_frame(kind, 0)
    }

    #[cfg(test)]
    pub(crate) fn for_test_kind_at_frame(kind: LocomotorKind, binary_frame: u32) -> Self {
        let (layer, speed_multiplier) = match kind {
            LocomotorKind::Fly
            | LocomotorKind::Jumpjet
            | LocomotorKind::Rocket
            | LocomotorKind::Parachute => (MovementLayer::Air, SimFixed::from_num(1)),
            LocomotorKind::Hover => (MovementLayer::Ground, SimFixed::from_num(1)),
            _ => (MovementLayer::Ground, SimFixed::from_num(1)),
        };

        Self {
            kind,
            slot: LocomotorSlot::from_kind(kind),
            powered: true,
            piggyback: None,
            runtime_payload: LocomotorRuntimePayload::for_kind(kind, binary_frame),
            layer,
            phase: GroundMovePhase::Idle,

            speed_multiplier,
            speed_fraction: SimFixed::from_num(1),
            fly_current_speed: SIM_ZERO,
            altitude: SIM_ZERO,

            jumpjet_speed: SIM_ZERO,
            jumpjet_accel: SIM_ZERO,
            jumpjet_current_speed: SIM_ZERO,
            jumpjet_deviation: 0,
            jumpjet_crash_speed: SIM_ZERO,
            jumpjet_turn_rate: 4,
            balloon_hover: false,
            hover_attack: false,
            speed_type: SpeedType::Track,
            movement_zone: MovementZone::Normal,
            rot: 5,
            air_progress: SIM_ZERO,
            infantry_wobble_phase: 0.0,
            subcell_dest: None,
            hover_throttle: SIM_ZERO,
            hover_speed_request: SIM_ZERO,
            hover_bob_offset: SIM_ZERO,
        }
    }

    /// Whether this locomotor is in the ground family (Drive/Walk/Hover/Mech/Ship).
    #[cfg(test)]
    pub fn is_ground_mover(&self) -> bool {
        matches!(
            self.kind,
            LocomotorKind::Drive
                | LocomotorKind::Walk
                | LocomotorKind::Hover
                | LocomotorKind::Mech
                | LocomotorKind::Ship
        )
    }

    /// Whether this locomotor is an air mover (Fly/Jumpjet/Rocket).
    #[cfg(test)]
    pub fn is_air_mover(&self) -> bool {
        matches!(
            self.kind,
            LocomotorKind::Fly | LocomotorKind::Jumpjet | LocomotorKind::Rocket
        )
    }

    /// Whether this unit is currently airborne (altitude > 0).
    pub fn is_airborne(&self) -> bool {
        self.altitude > SIM_ZERO
    }

    pub(crate) fn fly_runtime(&self) -> Option<&super::fly_height::FlyRuntime> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Fly, LocomotorRuntimePayload::Fly(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn fly_runtime_mut(&mut self) -> Option<&mut super::fly_height::FlyRuntime> {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Fly, LocomotorRuntimePayload::Fly(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn fly_target_height(&self) -> i32 {
        self.fly_runtime().map_or(0, |state| state.target_height())
    }

    pub(crate) fn set_fly_target_height(&mut self, height: i32) {
        if let Some(state) = self.fly_runtime_mut() {
            state.set_target_height(height);
        }
    }

    pub(crate) fn begin_fly_takeoff(&mut self, flight_level: i32) {
        if let Some(state) = self.fly_runtime_mut() {
            state.begin_takeoff(flight_level);
        }
    }

    pub(crate) fn begin_fly_landing(&mut self) {
        if let Some(state) = self.fly_runtime_mut() {
            state.begin_landing();
        }
    }

    #[cfg(test)]
    pub(crate) fn air_phase(&self) -> AirMovePhase {
        self.fly_runtime().map_or(AirMovePhase::Landed, |state| {
            state.mission_phase(self.altitude.to_num::<i32>())
        })
    }

    /// Whether a piggybacked locomotor is currently displacing the installed one.
    pub fn is_overridden(&self) -> bool {
        self.piggyback.is_some()
    }

    /// Current active locomotor class.
    pub fn active_kind(&self) -> LocomotorKind {
        self.kind
    }

    /// Walk interface+24 / Hover+24 retained full XYZ head. This belongs to
    /// the active instance and survives Stop/piggyback/save without sampling
    /// a newly changed ground surface. Native producers75C240/514F70; query
    /// receivers75CA80/517210, tools/spatial_oracle/locomotor_at_coord.
    pub(crate) fn step_head(&self) -> Option<crate::sim::components::DriveCoord> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) => state.head,
            (LocomotorKind::Hover, LocomotorRuntimePayload::Hover(head)) => *head,
            _ => None,
        }
    }

    pub(crate) fn set_step_head(&mut self, head: Option<crate::sim::components::DriveCoord>) {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) => state.head = head,
            (LocomotorKind::Hover, LocomotorRuntimePayload::Hover(stored)) => *stored = head,
            _ => {}
        }
    }

    pub(crate) fn walk_destination(&self) -> Option<crate::sim::components::DriveCoord> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) => state.destination,
            _ => None,
        }
    }

    pub(crate) fn jumpjet_runtime(&self) -> Option<&super::jumpjet_movement::JumpjetRuntime> {
        match (self.active_kind(), &self.runtime_payload) {
            (LocomotorKind::Jumpjet, LocomotorRuntimePayload::Jumpjet(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn jumpjet_runtime_mut(
        &mut self,
    ) -> Option<&mut super::jumpjet_movement::JumpjetRuntime> {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Jumpjet, LocomotorRuntimePayload::Jumpjet(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn walk_is_moving(&self) -> Option<bool> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) => Some(state.moving),
            _ => None,
        }
    }

    /// MoveTo/Stop keep a paid head. Their IsMoving byte survives a null
    ///destination while that head exists (75AD77 /75ADE9).
    pub(crate) fn set_walk_destination(
        &mut self,
        coord: Option<crate::sim::components::DriveCoord>,
    ) {
        if let (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) =
            (self.kind, &mut self.runtime_payload)
        {
            state.destination = coord.filter(|c| c.x != 0 || c.y != 0 || c.z != 0);
            if state.destination.is_some() {
                state.moving = true;
            } else if state.head.is_none() {
                state.moving = false;
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn walk_animation_moving(&self) -> Option<bool> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) => {
                Some(state.animation_moving)
            }
            _ => None,
        }
    }

    /// Original Process75BD25, after a nonnull head is admitted and before
    /// testing its remaining distance. Does not stand for the whole Process.
    pub(crate) fn begin_walk_motion(&mut self) {
        if let (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) =
            (self.kind, &mut self.runtime_payload)
            && state.head.is_some()
        {
            state.animation_moving = true;
        }
    }

    /// Stop75ADA0 differs from null MoveTo: no-head Stop also clears +36.
    pub(crate) fn stop_walk(&mut self) {
        self.set_walk_destination(None);
        if let (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) =
            (self.kind, &mut self.runtime_payload)
            && state.head.is_none()
        {
            state.animation_moving = false;
        }
    }

    pub(crate) fn active_slope_transition(&self) -> Option<&SlopeTransitionState> {
        match (self.active_kind(), &self.runtime_payload) {
            (LocomotorKind::Drive, LocomotorRuntimePayload::Drive(state))
            | (LocomotorKind::Ship, LocomotorRuntimePayload::Ship(state)) => Some(state),
            _ => None,
        }
    }

    pub(crate) fn active_slope_transition_mut(&mut self) -> Option<&mut SlopeTransitionState> {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Drive, LocomotorRuntimePayload::Drive(state))
            | (LocomotorKind::Ship, LocomotorRuntimePayload::Ship(state)) => Some(state),
            _ => None,
        }
    }

    /// The unit's identity for mission-level decisions: the installed class,
    /// seen through any piggyback that is currently driving it.
    ///
    /// A Chrono Miner driving out of a war factory on a piggybacked Drive still
    /// *is* a Teleport unit; `kind` answers "what is driving right now" and this
    /// answers "what is this unit". Both are needed and must stay distinct.
    pub fn effective_kind(&self) -> LocomotorKind {
        self.slot.into()
    }

    /// Whether the primary locomotor is currently active and no piggyback is stored.
    #[cfg(test)]
    pub fn is_primary_active(&self) -> bool {
        self.kind == self.effective_kind() && self.piggyback.is_none()
    }

    /// Activate Drive over a stashed Teleport locomotor — the Chrono Miner
    /// bridge model: the unit stays a Teleport unit, Drive temporarily drives it
    /// for destinations that need ground movement.
    pub fn begin_drive_piggyback_for_teleporter(&mut self, binary_frame: u32) -> bool {
        if self.effective_kind() != LocomotorKind::Teleport {
            return false;
        }
        if self.kind == LocomotorKind::Drive {
            // WalkLocomotionClass::BeginPiggyback rejects nested/incoherent
            // ownership; it never reconstructs a missing Teleport COM object.
            return self.piggyback.is_some();
        }
        self.begin_piggyback(LocomotorKind::Drive, MovementLayer::Ground, binary_frame)
    }

    /// Return from an active piggyback to the stashed locomotor.
    ///
    /// The installed slot is deliberately NOT written here. Natively the
    /// installed interface pointer never changes — a piggyback stashes and
    /// restores around it — and the previous write was retained only until this
    /// mechanism existed to retire it.
    pub fn restore_primary_from_piggyback(&mut self) -> bool {
        self.end_piggyback()
    }

    /// Whether the active piggyback can safely restore to the primary locomotor.
    pub fn can_restore_primary_from_piggyback(
        &self,
        owner_moving: bool,
        owner_teleporting: bool,
        owner_deploying: bool,
    ) -> bool {
        self.is_ok_to_end_piggyback(EndGateContext {
            owner_moving,
            owner_teleporting,
            owner_deploying,
        })
    }

    /// Begin a piggyback: stash the driving locomotor and install this one.
    ///
    /// Refuses, changing nothing, if a stash is already present — the native
    /// BEGIN returns `E_FAIL` in exactly that case.
    pub fn begin_piggyback(
        &mut self,
        kind: LocomotorKind,
        layer: MovementLayer,
        binary_frame: u32,
    ) -> bool {
        piggyback::begin(self, kind, layer, binary_frame) == piggyback::BeginOutcome::Installed
    }

    /// End the active piggyback, restoring the stashed locomotor.
    ///
    /// Returns whether anything was stashed.
    pub fn end_piggyback(&mut self) -> bool {
        piggyback::end(self).is_some()
    }

    /// Power this locomotor back on.
    pub fn power_on(&mut self) {
        self.powered = true;
    }

    /// Power this locomotor off. Only the Hover family has an observable
    /// response today: it stops producing lift and sinks.
    pub fn power_off(&mut self) {
        self.powered = false;
    }

    /// Whether this locomotor is powered.
    pub fn is_powered(&self) -> bool {
        self.powered
    }

    /// Whether the active piggyback may be unwound now. The movement clause
    /// dominates: a moving unit never unwinds.
    ///
    /// **Recorded gap, not closed — VERA has no single locomotor dispatch
    /// point.** `FootClass::AI` @ `0x004DA530` makes exactly *one* `Process`
    /// dispatch per object per frame (it also calls `Is_Moving_Now` `+0x80`
    /// four times and `QueryInterface` around it), `ILocomotion::Process`
    /// (Drive ILocomotion vtable
    /// `0x007E7EB0`, slot `+0x40` = `DriveLocomotionClass::Process` @
    /// `0x004B0500`), and the installed object alone decides which body runs —
    /// there is no kind switch anywhere. VERA instead runs about ten
    /// independent per-kind world passes each tick, every one self-gating on
    /// `locomotor.kind` or on a per-kind state component, with the piggyback
    /// restore last. Trigger: every object, every tick. Player effect: none
    /// named — but inter-family ordering becomes a property of the tick's pass
    /// order rather than of the installed locomotor. Frequency: continuous.
    /// Downstream risk: this is why a piggyback can be installed by one pass and
    /// observed by another in the same tick, and it is the shape rows GSI-06.13
    /// and GSI-06.14 have to build on.
    pub fn is_ok_to_end_piggyback(&self, context: EndGateContext) -> bool {
        piggyback::is_ok_to_end(self, context)
    }
}

#[cfg(test)]
#[path = "locomotor_tests.rs"]
mod locomotor_tests;
