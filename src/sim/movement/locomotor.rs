//! Locomotor component and access to each movement mechanism's runtime.
//!
//! Foot's NavCom and path queue live in `NavigationState`. Mechanism payloads
//! own native controller state, including each locomotor's destination; shared
//! fields retain movement and presentation adapters.
//! Current world Z is authoritative in Object coordinates, and GetHeight
//! reads it; Fly's integer target height lives in its own payload.

use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::rules::object_type::ObjectType;
use crate::sim::movement::locomotion::piggyback::{
    self, LocomotorRuntimePayload, StashedLocomotor,
};
use crate::sim::movement::slope_transition::SlopeTransitionState;

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

/// Derived test view of Fly height versus target ([`LocomotorState::air_phase`]).
/// This is neither serialized controller state nor native takeoff/landing flags.
///
/// Fly units cycle through TakingOff → Cruising → Descending → Landed. A
/// Jumpjet does not use it: its phase is the native state field,
/// `JumpjetRuntime::phase`.
#[cfg(any(test, feature = "test-support"))]
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

/// Every field of one locomotor object, for the state hash's fold
/// (`world_hash::hash_locomotor`). [`LocomotorState::hash_fields`] builds it by
/// destructuring, so a new field fails to compile until the fold takes it.
pub(crate) struct LocomotorHashFields<'a> {
    pub(crate) kind: LocomotorKind,
    pub(crate) powered: bool,
    pub(crate) piggyback: Option<&'a LocomotorState>,
    pub(crate) runtime_payload: &'a LocomotorRuntimePayload,
    pub(crate) layer: MovementLayer,
    pub(crate) balloon_hover: bool,
    pub(crate) speed_type: SpeedType,
    pub(crate) movement_zone: MovementZone,
}

/// Runtime locomotor state attached to each movable ECS entity.
///
/// Created from `ObjectType` at spawn time. Each object's Process runs the
/// active mechanism it holds. It is one complete locomotor object: a
/// piggyback suspends the whole object in the new active object's `piggyback`
/// slot and restores it, keeping only the Foot's layer (see
/// [`Self::end_piggyback`]).
///
/// `balloon_hover`, `speed_type` and `movement_zone` cache the
/// Foot's type: set at construction, copied into a BEGIN's temporary and never
/// written afterwards, so every object of one unit holds the same values.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocomotorState {
    /// Which locomotor class this object is.
    kind: LocomotorKind,
    /// Whether this locomotor is powered.
    ///
    /// Natively a plain flag on the locomotor instance, set by `Power_On` /
    /// `Power_Off` and read by `Is_Powered`; both setters also re-dispatch to
    /// another slot, which has no verified effect and is not modelled. Defaults
    /// to on — an unpowered locomotor is a state something must actively put a
    /// unit into.
    powered: bool,
    /// The suspended locomotor object, when a piggyback displaced it, with any
    /// stash of its own: a Chrono Miner's Drive holds the miner's Teleport,
    /// and a Chrono Warp's Teleport holds whatever it displaced.
    #[serde(default)]
    piggyback: Option<StashedLocomotor>,
    /// Class-local state of this locomotor object.
    runtime_payload: LocomotorRuntimePayload,
    /// Which spatial layer the unit currently occupies: the Foot's position,
    /// cached on the active object. `Bridge` is the Ground layer with the
    /// Foot's OnBridge set.
    layer: MovementLayer,
    /// Stay airborne after reaching destination (BalloonHover=yes).
    balloon_hover: bool,
    /// Which terrain type this unit traverses (from rules.ini SpeedType=).
    speed_type: SpeedType,
    /// Pathfinder movement zone — determines crush capability and special routing.
    /// Cached from ObjectType at spawn to avoid per-tick RuleSet lookups.
    movement_zone: MovementZone,
}

impl LocomotorState {
    /// Create a LocomotorState from an ObjectType's parsed rules.ini data.
    ///
    /// Class runtime starts from its native constructor; Fly requests resolve
    /// FlightLevel when takeoff is admitted, not during construction.
    pub fn from_object_type(obj: &ObjectType, binary_frame: u32) -> Self {
        let kind: LocomotorKind = obj.locomotor;
        let mut state = Self::constructed(
            kind,
            Self::spawn_layer(kind),
            binary_frame,
            obj.balloon_hover,
            obj.speed_type,
            obj.movement_zone,
        );
        state.link_to_object(obj);
        state
    }

    /// Ares conversion1004382D..100438AD compares the two Type CLSIDs, not
    /// the active (possibly piggybacked) locomotor. Equal CLSIDs retain the
    /// instance and its timers; VERA's derived type caches still need rebinding.
    #[cfg(feature = "ares")]
    pub(crate) fn change_type_binding(
        &mut self,
        old_type_kind: LocomotorKind,
        object: &ObjectType,
        frame: u32,
    ) {
        if old_type_kind != object.locomotor {
            let mut replacement = Self::from_object_type(object, frame);
            //10043871..100438AA replaces only Foot+674. The Foot's current
            // world position and bridge/air occupancy are not constructor state.
            replacement.layer = self.layer;
            *self = replacement;
        } else {
            self.balloon_hover = object.balloon_hover;
            self.speed_type = object.speed_type;
            self.movement_zone = object.movement_zone;
            if let Some(saved) = self.piggyback.as_mut() {
                saved.change_type_binding(old_type_kind, object, frame);
            }
        }
    }

    /// The class's `Link_To_Object` for a Foot of type `obj`, where it copies
    /// type data: Jumpjet (`0x0054AD30`) copies the type's jumpjet block and
    /// builds the locomotor facing at its `JumpjetTurnRate=`; Hover
    /// (`0x00513CB0`) builds the steering facing at the type's `ROT=`; Fly
    /// (`FlyLocomotionClass::Link_To_Object @ 0x004CCA20`) keeps whether the
    /// type is an `AirportBound=` aircraft (`0x004CCA5B`, `0x004CCA64`).
    fn link_to_object(&mut self, obj: &ObjectType) {
        match &mut self.runtime_payload {
            LocomotorRuntimePayload::Jumpjet(runtime) => runtime.link(&obj.jumpjet_params),
            LocomotorRuntimePayload::Hover(runtime) => runtime.link(obj.turret_rot),
            LocomotorRuntimePayload::Fly(runtime) => runtime.link(
                obj.category == crate::rules::object_type::ObjectCategory::Aircraft
                    && obj.airport_bound,
            ),
            _ => {}
        }
    }

    /// A just-constructed `kind` object on `layer`, before its link: the
    /// `LocomotionClass` constructor (`0x0055A6C0`) raises Powered and each
    /// class constructor clears its own state
    /// ([`LocomotorRuntimePayload::for_kind`]). The three type caches come
    /// from the Foot's type.
    fn constructed(
        kind: LocomotorKind,
        layer: MovementLayer,
        binary_frame: u32,
        balloon_hover: bool,
        speed_type: SpeedType,
        movement_zone: MovementZone,
    ) -> Self {
        Self {
            kind,
            powered: true,
            piggyback: None,
            runtime_payload: LocomotorRuntimePayload::for_kind(kind, binary_frame),
            layer,
            balloon_hover,
            speed_type,
            movement_zone,
        }
    }

    /// The layer a unit's constructed locomotor starts on.
    fn spawn_layer(kind: LocomotorKind) -> MovementLayer {
        match kind {
            LocomotorKind::Drive
            | LocomotorKind::Walk
            | LocomotorKind::Hover
            | LocomotorKind::Ship
            | LocomotorKind::Teleport => MovementLayer::Ground,
            LocomotorKind::Fly | LocomotorKind::Jumpjet | LocomotorKind::Rocket => {
                MovementLayer::Air
            }
        }
    }

    /// A freshly constructed `kind` object linked to the same Foot, as every
    /// native BEGIN site installs one: the Unit setter allocates
    /// (`0x0041C250`), constructs (Drive `0x004AF540` over the
    /// `LocomotionClass` constructor `0x0055A6C0`) and links it
    /// (`0x007426C9`) before BEGIN (`0x0074276F`).
    /// - It keeps this object's type caches and the Foot's current layer. A
    ///   BEGIN does not move the Foot. The setter's Drive and the Chrono
    ///   Warp's Teleport answer Ground from `In_Which_Layer` (`0x004B4820`,
    ///   `0x00719E20`); the Magnetron's Jumpjet takes its type's link block
    ///   ([`Self::begin_piggyback_linked`]) and answers its layer from its
    ///   height (`world::entity_layer`), not from this cache.
    pub(crate) fn fresh_linked(&self, kind: LocomotorKind, binary_frame: u32) -> Self {
        Self::constructed(
            kind,
            self.layer,
            binary_frame,
            self.balloon_hover,
            self.speed_type,
            self.movement_zone,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test_kind(kind: LocomotorKind) -> Self {
        Self::for_test_kind_at_frame(kind, 0)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn for_test_kind_at_frame(kind: LocomotorKind, binary_frame: u32) -> Self {
        Self::constructed(
            kind,
            Self::spawn_layer(kind),
            binary_frame,
            false,
            SpeedType::Track,
            MovementZone::Normal,
        )
    }

    /// Whether this locomotor is in the ground family (Drive/Walk/Hover/Ship).
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_ground_mover(&self) -> bool {
        matches!(
            self.kind,
            LocomotorKind::Drive | LocomotorKind::Walk | LocomotorKind::Hover | LocomotorKind::Ship
        )
    }

    /// Whether this locomotor is an air mover (Fly/Jumpjet/Rocket).
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_air_mover(&self) -> bool {
        matches!(
            self.kind,
            LocomotorKind::Fly | LocomotorKind::Jumpjet | LocomotorKind::Rocket
        )
    }

    pub(crate) fn fly_runtime(&self) -> Option<&super::fly_height::FlyRuntime> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Fly, LocomotorRuntimePayload::Fly(state)) => Some(state),
            _ => None,
        }
    }

    /// Native Fly current speed (`Fly+0x48`), read by the normal-Fly
    /// `Draw_Matrix @ 0x004CF610` PitchSpeed gate through its ILocomotion view.
    pub fn current_fly_speed(&self) -> Option<crate::util::native_x87::NativeF64Bits> {
        self.fly_runtime().map(|state| state.current_speed())
    }

    /// Fly+50/+51 callbacks, read by Draw_Point4CF8B8..4CF8BE.
    pub fn fly_phase_callbacks(&self) -> Option<(bool, bool)> {
        self.fly_runtime()
            .map(|state| (state.taking_off(), state.landing()))
    }

    pub(crate) fn fly_runtime_mut(&mut self) -> Option<&mut super::fly_height::FlyRuntime> {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Fly, LocomotorRuntimePayload::Fly(state)) => Some(state),
            _ => None,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn set_current_fly_speed_for_test(&mut self, speed: crate::util::fixed_math::SimFixed) {
        if let Some(state) = self.fly_runtime_mut() {
            state.set_current_speed_for_test(speed);
        }
    }

    pub fn rocket_runtime(&self) -> Option<&super::rocket_movement::RocketRuntime> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Rocket, LocomotorRuntimePayload::Rocket(state)) => Some(state),
            _ => None,
        }
    }

    pub fn rocket_runtime_mut(&mut self) -> Option<&mut super::rocket_movement::RocketRuntime> {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Rocket, LocomotorRuntimePayload::Rocket(state)) => Some(state),
            _ => None,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn fly_target_height(&self) -> i32 {
        self.fly_runtime().map_or(0, |state| state.target_height())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_fly_target_height(&mut self, height: i32) {
        if let Some(state) = self.fly_runtime_mut() {
            state.set_flight_level(height);
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

    /// The test flight phase at the object's GetHeight `height`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn air_phase(&self, height: i32) -> AirMovePhase {
        self.fly_runtime()
            .map_or(AirMovePhase::Landed, |state| state.mission_phase(height))
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
    pub fn step_head(&self) -> Option<crate::sim::components::DriveCoord> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) => state.head(),
            (LocomotorKind::Hover, LocomotorRuntimePayload::Hover(runtime)) => runtime.head(),
            _ => None,
        }
    }

    pub(crate) fn set_step_head(&mut self, head: Option<crate::sim::components::DriveCoord>) {
        // Hover's head has one writer, its ProcessMovement (`hover_process`).
        if let (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) =
            (self.kind, &mut self.runtime_payload)
        {
            state.store_head(head);
        }
    }

    /// The active Hover object's state.
    pub(crate) fn hover_runtime(&self) -> Option<&super::hover::HoverRuntime> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Hover, LocomotorRuntimePayload::Hover(runtime)) => Some(runtime),
            _ => None,
        }
    }

    pub(crate) fn hover_runtime_mut(&mut self) -> Option<&mut super::hover::HoverRuntime> {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Hover, LocomotorRuntimePayload::Hover(runtime)) => Some(runtime),
            _ => None,
        }
    }

    pub(crate) fn teleport_runtime(&self) -> Option<&super::teleport_movement::TeleportRuntime> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Teleport, LocomotorRuntimePayload::Teleport(runtime)) => Some(runtime),
            _ => None,
        }
    }

    pub(crate) fn teleport_runtime_mut(
        &mut self,
    ) -> Option<&mut super::teleport_movement::TeleportRuntime> {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Teleport, LocomotorRuntimePayload::Teleport(runtime)) => Some(runtime),
            _ => None,
        }
    }

    /// The Teleport that holds a Chronosphere warp, active or suspended.
    pub(crate) fn warp_teleport(&self) -> Option<&super::teleport_movement::TeleportRuntime> {
        self.teleport_runtime()
            .filter(|runtime| runtime.chrono().is_some())
            .or_else(|| self.piggyback.as_deref()?.warp_teleport())
    }

    /// [`Self::warp_teleport`] for its Process, which writes its own timer
    /// and state after the Unit setter's Drive has suspended it: state 5
    /// after `Set_Destination(NULL)` (`0x00719B0D`) at
    /// `0x00719B1B..0x00719B85`, state 7 after `0x00719BCC` at
    /// `0x00719BD2..0x00719BDF`.
    pub(crate) fn warp_teleport_mut(
        &mut self,
    ) -> Option<&mut super::teleport_movement::TeleportRuntime> {
        if self
            .teleport_runtime()
            .is_some_and(|runtime| runtime.chrono().is_some())
        {
            return self.teleport_runtime_mut();
        }
        self.piggyback.as_mut()?.warp_teleport_mut()
    }

    /// Foot warp-effect readers see an effect held by the complete suspended
    /// instance too. This view stores nothing and does not dispatch Process.
    pub(crate) fn teleport_effect_state(&self) -> Option<&super::teleport_movement::TeleportState> {
        self.teleport_runtime()
            .and_then(super::teleport_movement::TeleportRuntime::warp)
            .or_else(|| self.piggyback.as_ref()?.teleport_effect_state())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn has_teleport_instance(&self) -> bool {
        self.teleport_runtime().is_some()
            || self
                .piggyback
                .as_ref()
                .is_some_and(|l| l.has_teleport_instance())
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn teleport_instance_for_test_mut(
        &mut self,
    ) -> Option<&mut super::teleport_movement::TeleportRuntime> {
        if self.kind == LocomotorKind::Teleport {
            return self.teleport_runtime_mut();
        }
        self.piggyback
            .as_mut()?
            .suspended_mut_for_test()
            .teleport_instance_for_test_mut()
    }

    /// The cell of Walk's destination coordinate.
    #[cfg(any(test, feature = "test-support"))]
    pub fn walk_destination_cell(&self) -> Option<(u16, u16)> {
        self.walk_destination()
            .map(|c| ((c.x / 256) as u16, (c.y / 256) as u16))
    }

    pub fn walk_destination(&self) -> Option<crate::sim::components::DriveCoord> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) => state.destination(),
            _ => None,
        }
    }

    pub fn jumpjet_runtime(&self) -> Option<&super::jumpjet_movement::JumpjetRuntime> {
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

    pub fn walk_is_moving(&self) -> Option<bool> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) => Some(state.moving()),
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
            state.assign_destination(coord);
        }
    }

    /// Walk's `Is_Really_Moving_Now` (ILocomotion +0xA8 = `0x0075CB20`):
    /// its class byte +0x36. `None` for every other locomotor.
    pub(crate) fn walk_animation_moving(&self) -> Option<bool> {
        match (self.kind, &self.runtime_payload) {
            (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) => {
                Some(state.animation_moving())
            }
            _ => None,
        }
    }

    /// Original Process75BC2A (a fresh head) and 75BD25 (a paid one), after a
    /// nonnull head is admitted and before testing its remaining distance.
    /// Does not stand for the whole Process.
    pub(crate) fn begin_walk_motion(&mut self) {
        if let (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) =
            (self.kind, &mut self.runtime_payload)
        {
            state.begin_motion();
        }
    }

    /// `Stop_Movement_Animation` (ILocomotion +0xAC): Walk clears +0x36
    /// (`0x0075CBC0`) and keeps its IsMoving byte, destination and head;
    /// every other locomotor's slot is the base `ret 4` (`0x004B4C90`).
    /// Walk's Process makes the same write inline after a failed path search
    /// (`0x0075AFD5`) and on a refused fresh head (`0x0075B6A3`).
    pub(crate) fn stop_movement_animation(&mut self) {
        if let (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) =
            (self.kind, &mut self.runtime_payload)
        {
            state.stop_movement_animation();
        }
    }

    /// Stop75ADA0 differs from null MoveTo: no-head Stop also clears +36
    /// and dispatches the owner's +54C callback. The world receiver performs
    /// that dispatch before returning; this primitive returns its admission.
    pub(crate) fn stop_walk(&mut self) -> bool {
        if let (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) =
            (self.kind, &mut self.runtime_payload)
        {
            return state.stop();
        }
        false
    }

    /// `Lock` (ILocomotion +0xB0): Walk `0x0075CB30` nulls its destination
    /// (+0x18) and head (+0x24) and writes nothing else. FootClass::Limbo
    /// (`0x004DB260`) calls it on the first Limbo, after +9C(0) released the
    /// head's occupation. Other locomotors' +0xB0 bodies are not ported.
    pub(crate) fn walk_lock(&mut self) {
        if let (LocomotorKind::Walk, LocomotorRuntimePayload::Walk(state)) =
            (self.kind, &mut self.runtime_payload)
        {
            state.lock();
        }
    }

    pub(crate) fn active_slope_transition(&self) -> Option<&SlopeTransitionState> {
        match (self.active_kind(), &self.runtime_payload) {
            (LocomotorKind::Drive, LocomotorRuntimePayload::Drive(state)) => Some(state.slope()),
            (LocomotorKind::Ship, LocomotorRuntimePayload::Ship(state)) => Some(state.slope()),
            _ => None,
        }
    }

    pub fn active_slope_transition_mut(&mut self) -> Option<&mut SlopeTransitionState> {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Drive, LocomotorRuntimePayload::Drive(state)) => {
                Some(state.slope_mut())
            }
            (LocomotorKind::Ship, LocomotorRuntimePayload::Ship(state)) => Some(state.slope_mut()),
            _ => None,
        }
    }

    /// The class this unit was built with: the bottom of the piggyback chain,
    /// the object its class constructor created from the type's `Locomotor=`
    /// CLSID. Natively a unit holds one locomotor interface and never
    /// re-selects it; a piggyback only suspends it.
    ///
    /// A Chrono Miner driving out of a war factory on a piggybacked Drive still
    /// *is* a Teleport unit; `kind` answers "what is driving right now" and this
    /// answers "what is this unit".
    pub fn effective_kind(&self) -> LocomotorKind {
        self.piggyback
            .as_deref()
            .map_or(self.kind, LocomotorState::effective_kind)
    }

    /// Whether the constructed locomotor is active, with nothing stashed.
    #[cfg(any(test, feature = "test-support"))]
    pub fn is_primary_active(&self) -> bool {
        self.piggyback.is_none()
    }

    /// Begin a piggyback: a fresh `kind` object suspends the active one, with
    /// any stash of its own, and becomes active. Every native BEGIN site
    /// installs a freshly constructed object ([`Self::fresh_linked`]), and
    /// BEGIN refuses only when that object's own slot is occupied, which it
    /// never is.
    pub fn begin_piggyback(&mut self, kind: LocomotorKind, binary_frame: u32) -> bool {
        let incoming = self.fresh_linked(kind, binary_frame);
        piggyback::begin_with(self, Some(incoming)) == piggyback::BeginOutcome::Installed
    }

    /// [`Self::begin_piggyback`] with the fresh object linked to the Foot's
    /// type `obj`, as `TechnoClass::ImbueLocomotor @ 0x00710000` links its
    /// new locomotor (`0x00710243`) before BEGIN (`0x007102D8`). The link
    /// reads only the type, so linking the installed object is the same.
    pub(crate) fn begin_piggyback_linked(
        &mut self,
        kind: LocomotorKind,
        obj: &ObjectType,
        binary_frame: u32,
    ) -> bool {
        let installed = self.begin_piggyback(kind, binary_frame);
        if installed {
            self.link_to_object(obj);
        }
        installed
    }

    /// `End_Piggyback` (`0x004AF930`) and the install its callers make: make
    /// the suspended object active again. Answers the displaced temporary,
    /// which the caller then releases; `None` is native `S_FALSE` (no
    /// suspended object). Native's `E_POINTER` arm (a null out-pointer) has no
    /// Rust caller.
    ///
    /// The restored object keeps everything from its BEGIN except the layer,
    /// which is the Foot's: END (`0x004AF930`) does not move the Foot, and
    /// Drive and Teleport both answer Ground from `In_Which_Layer`
    /// (`0x004B4820`, `0x00719E20`), so a Foot that drove onto or off a bridge
    /// deck stays there.
    pub fn end_piggyback(&mut self) -> Option<LocomotorState> {
        let stashed = self.piggyback.take()?;
        let released = std::mem::replace(self, stashed.into_active());
        self.layer = released.layer;
        Some(released)
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

    /// Every field, for the state hash's fold ([`LocomotorHashFields`]).
    pub(crate) fn hash_fields(&self) -> LocomotorHashFields<'_> {
        let Self {
            kind,
            powered,
            piggyback,
            runtime_payload,
            layer,
            balloon_hover,
            speed_type,
            movement_zone,
        } = self;
        LocomotorHashFields {
            kind: *kind,
            powered: *powered,
            piggyback: piggyback.as_deref(),
            runtime_payload,
            layer: *layer,
            balloon_hover: *balloon_hover,
            speed_type: *speed_type,
            movement_zone: *movement_zone,
        }
    }

    /// A fixture's power flag.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_powered_for_test(&mut self, powered: bool) {
        self.powered = powered;
    }

    /// The suspended object a piggyback displaced, with any stash of its own.
    pub(crate) fn piggyback(&self) -> &Option<StashedLocomotor> {
        &self.piggyback
    }

    /// The suspended object, for its typed private-storage operations.
    pub(crate) fn piggyback_mut(&mut self) -> Option<&mut StashedLocomotor> {
        self.piggyback.as_mut()
    }

    /// BEGIN's install: `incoming` becomes the active object, with this one
    /// and any stash of its own suspended whole beneath it.
    pub(in crate::sim::movement) fn suspend_beneath(&mut self, incoming: LocomotorState) {
        let displaced = std::mem::replace(self, incoming);
        self.piggyback = Some(StashedLocomotor::new(displaced));
    }

    /// A fixture's suspended object.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_piggyback_for_test(&mut self, piggyback: Option<StashedLocomotor>) {
        self.piggyback = piggyback;
    }

    /// Class-local state of this locomotor object.
    pub(crate) fn runtime_payload(&self) -> &LocomotorRuntimePayload {
        &self.runtime_payload
    }

    /// Class-local state of this locomotor object, for fixtures.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn runtime_payload_mut(&mut self) -> &mut LocomotorRuntimePayload {
        &mut self.runtime_payload
    }

    /// The active Drive object's class state, lent to its class module
    /// (`drive_locomotion`), whose methods alone write it.
    pub(crate) fn drive_runtime_mut(
        &mut self,
    ) -> Option<&mut super::drive_locomotion::DriveRuntime> {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Drive, LocomotorRuntimePayload::Drive(state)) => Some(state),
            _ => None,
        }
    }

    /// The active Ship object's class state, lent to `drive_locomotion`,
    /// whose methods alone write it.
    pub(crate) fn ship_runtime_mut(&mut self) -> Option<&mut super::drive_locomotion::ShipRuntime> {
        match (self.kind, &mut self.runtime_payload) {
            (LocomotorKind::Ship, LocomotorRuntimePayload::Ship(state)) => Some(state),
            _ => None,
        }
    }

    /// A fixture's class-local state.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_runtime_payload_for_test(&mut self, payload: LocomotorRuntimePayload) {
        self.runtime_payload = payload;
    }

    /// The Foot's layer, cached on the active object.
    pub(crate) fn layer(&self) -> MovementLayer {
        self.layer
    }

    /// A fixture's layer.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_layer_for_test(&mut self, layer: MovementLayer) {
        self.layer = layer;
    }

    /// Put the cached layer on the ground plane as OnBridge (`Object+0x8C`)
    /// projects it: [`MovementLayer::Bridge`] on a deck, otherwise
    /// [`MovementLayer::Ground`]. gamemd keeps no such field (the layer is
    /// the locomotor's `In_Which_Layer`); a placement or mover that stores
    /// OnBridge stores this with it.
    pub(crate) fn store_ground_layer(&mut self, on_bridge: bool) {
        self.layer = if on_bridge {
            MovementLayer::Bridge
        } else {
            MovementLayer::Ground
        };
    }

    /// The type's `BalloonHover=`.
    pub(crate) fn balloon_hover(&self) -> bool {
        self.balloon_hover
    }

    /// A fixture's `BalloonHover=`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_balloon_hover_for_test(&mut self, balloon_hover: bool) {
        self.balloon_hover = balloon_hover;
    }

    /// The type's `SpeedType=`.
    pub fn speed_type(&self) -> SpeedType {
        self.speed_type
    }

    /// A fixture's `SpeedType=`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_speed_type_for_test(&mut self, speed_type: SpeedType) {
        self.speed_type = speed_type;
    }

    /// The type's `MovementZone=`.
    pub(crate) fn movement_zone(&self) -> MovementZone {
        self.movement_zone
    }

    /// A fixture's `MovementZone=`.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn set_movement_zone_for_test(&mut self, movement_zone: MovementZone) {
        self.movement_zone = movement_zone;
    }

    /// A fixture's class.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_kind_for_test(&mut self, kind: LocomotorKind) {
        self.kind = kind;
    }
}

#[cfg(test)]
#[path = "locomotor_tests.rs"]
mod locomotor_tests;
