//! The one piggyback mechanism: a locomotor temporarily displacing another.
//!
//! `IPiggyback` (IID at `0x00819088`) owns one suspended `ILocomotion`
//! reference. BEGIN takes that complete object, END transfers the same object
//! back, and save/load serializes it as a nested COM object. The Rust seam
//! mirrors ownership, not COM refcounts.
//!
//! Six classes provide the interface, by vtable: Drive `0x007E7E8C`, Walk
//! `0x007F69D4`, Teleport `0x007F4FDC`, Ship `0x007F2D68`, Jumpjet `0x007ECD44`
//! and DropPod `0x007E8254`. Fly and Rocket have none.
//!
//! The verified bodies this module is modelled on are Drive's —
//! `Begin_Piggyback` @ `0x004AF8E0` (`E_POINTER` on null, **`E_FAIL` when the
//! slot is already occupied**, else store and AddRef), `End_Piggyback` @
//! `0x004AF930` (`E_POINTER` on a null out-pointer, **`S_FALSE` when empty**,
//! else transfer and null the slot), `Is_Ok_To_End` @ `0x004AF970`, and
//! `Save` @ `0x004AF800`, whose one-byte presence flag precedes the
//! `OleSaveToStream` of the stashed locomotor. Teleport's twins are
//! `0x00719E90` / `0x00719EE0` / `0x00719F30`; Walk's are `0x0075C850` /
//! `0x0075C8A0` / `0x0075C8E0`, none of them labelled in Ghidra, and
//! `WalkLocomotionClass::Save` is not identified at all — which is why the
//! save-shape claim above cites Drive.
//!
//! The slot itself is one interface pointer inside the *piggybacking*
//! locomotor: Drive at IPiggyback-frame `+0x50` (LocomotorBase+0x68), Teleport
//! at `+0x30`, Walk at `+0x20`.

use std::ops::Deref;

use crate::rules::locomotor_type::{LocomotorKind, MovementZone, SpeedType};
use crate::util::fixed_math::SimFixed;

use super::super::drop_pod_movement::DropPodState;
use super::super::locomotor::{GroundMovePhase, LocomotorState, MovementLayer};
use super::super::rocket_movement::RocketState;
use super::super::slope_transition::SlopeTransitionState;
use super::super::teleport_movement::TeleportState;
use super::super::tunnel_movement::TunnelState;

/// Runtime state shared by every locomotor object, independent of its installed
/// class identity. This is what moves as one object through the piggyback slot.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocomotorCommonRuntime {
    pub powered: bool,
    pub phase: GroundMovePhase,
    pub speed_multiplier: SimFixed,
    pub speed_fraction: SimFixed,
    pub fly_current_speed: SimFixed,
    pub altitude: SimFixed,
    pub balloon_hover: bool,
    pub hover_attack: bool,
    pub speed_type: SpeedType,
    pub movement_zone: MovementZone,
    pub rot: i32,
    pub air_progress: SimFixed,
    pub infantry_wobble_phase: f32,
    pub subcell_dest: Option<(SimFixed, SimFixed)>,
    pub hover_throttle: SimFixed,
    pub hover_speed_request: SimFixed,
    pub hover_bob_offset: SimFixed,
}

/// Walk MoveTo75ACB0 / Stop75ADA0 retain destination independently of the
/// committed head. Both XYZ values belong to this complete locomotor instance.
#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WalkRuntime {
    pub head: Option<crate::sim::components::DriveCoord>,
    pub destination: Option<crate::sim::components::DriveCoord>,
    /// Full object+34 / interface+30, read by IsMoving75AB30. Constructor
    ///75AAD3 clears; MoveTo75AD5A sets; null MoveTo/Stop clear only with no
    ///head. FindSubCellDest's head retirement does not change this byte.
    pub moving: bool,
    /// Full object+36 / interface+32, queried by75CB20 and Infantry520F40.
    /// Process75BD25 sets it before distance/completion; Stop75ADEC clears
    /// only with no paid head. It is distinct from IsMoving(+34).
    pub animation_moving: bool,
}

/// Class-local state that travels with the locomotor object.
///
/// Special process state is carried here rather than reconstructed from a phase
/// byte when a complete locomotor is suspended or loaded.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LocomotorRuntimePayload {
    Drive(SlopeTransitionState),
    Walk(WalkRuntime),
    Teleport(Option<TeleportState>),
    Tunnel(Option<TunnelState>),
    Rocket(Option<RocketState>),
    DropPod(Option<DropPodState>),
    Hover(Option<crate::sim::components::DriveCoord>),
    Mech,
    Ship(SlopeTransitionState),
    Fly(super::super::fly_height::FlyRuntime),
    Jumpjet(super::super::jumpjet_movement::JumpjetRuntime),
    Parachute,
}

impl LocomotorRuntimePayload {
    pub(crate) fn for_kind(kind: LocomotorKind, binary_frame: u32) -> Self {
        match kind {
            LocomotorKind::Drive => {
                Self::Drive(SlopeTransitionState::at_binary_frame(binary_frame))
            }
            LocomotorKind::Walk => Self::Walk(WalkRuntime::default()),
            LocomotorKind::Teleport => Self::Teleport(None),
            LocomotorKind::Tunnel => Self::Tunnel(None),
            LocomotorKind::Rocket => Self::Rocket(None),
            LocomotorKind::DropPod => Self::DropPod(None),
            LocomotorKind::Hover => Self::Hover(None),
            LocomotorKind::Mech => Self::Mech,
            LocomotorKind::Ship => Self::Ship(SlopeTransitionState::at_binary_frame(binary_frame)),
            LocomotorKind::Fly => Self::Fly(Default::default()),
            LocomotorKind::Jumpjet => Self::Jumpjet(Default::default()),
            LocomotorKind::Parachute => Self::Parachute,
        }
    }
}

/// One complete movable locomotor instance. Installed identity remains on the
/// host `LocomotorState`; this object is the active or suspended implementation.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocomotorRuntime {
    pub kind: LocomotorKind,
    pub layer: MovementLayer,
    pub common: LocomotorCommonRuntime,
    pub payload: LocomotorRuntimePayload,
}

impl LocomotorRuntime {
    /// Capture all active runtime-owned state without copying the host's
    /// installed slot or piggyback pointer.
    pub fn capture(state: &LocomotorState) -> Self {
        Self {
            kind: state.kind,
            layer: state.layer,
            common: LocomotorCommonRuntime {
                powered: state.powered,
                phase: state.phase,
                speed_multiplier: state.speed_multiplier,
                speed_fraction: state.speed_fraction,
                fly_current_speed: state.fly_current_speed,
                altitude: state.altitude,
                balloon_hover: state.balloon_hover,
                hover_attack: state.hover_attack,
                speed_type: state.speed_type,
                movement_zone: state.movement_zone,
                rot: state.rot,
                air_progress: state.air_progress,
                infantry_wobble_phase: state.infantry_wobble_phase,
                subcell_dest: state.subcell_dest,
                hover_throttle: state.hover_throttle,
                hover_speed_request: state.hover_speed_request,
                hover_bob_offset: state.hover_bob_offset,
            },
            payload: state.runtime_payload.clone(),
        }
    }

    /// Build an incoming runtime from the current host defaults. New callers
    /// should prefer `begin_with_runtime` when they already own an instance.
    ///
    /// **VERA-internal, gamemd has no equivalent.** Every native BEGIN site
    /// installs a *freshly constructed* COM object — `CoCreateInstance(CLSID)`
    /// then `Link_To_Object(owner)` and nothing else — so the temporary starts
    /// default-initialised and the displaced locomotor keeps all of its state
    /// untouched in the stash. This clones the displaced runtime and resets only
    /// `phase` and `payload`, so the temporary inherits
    /// `altitude`, the hover throttle/speed/bob fields, `subcell_dest`, the two
    /// speed fractions, `fly_current_speed` **and `powered`** — and [`install_into`] copies `powered` back on restore, so a
    /// powered-off flag survives a swap in both directions, which native cannot
    /// do.
    ///
    /// Trigger: every BEGIN. Player effect: none today — on a Chrono Miner,
    /// which is the only stock unit that piggybacks, every carried field is
    /// already zero or 1.0. Frequency: every Chrono Miner move order, with no
    /// divergence. Downstream risk: the carried fields are deterministic state
    /// and are hashed, so this becomes live the first time a hover, jumpjet or
    /// air locomotor is piggybacked.
    pub fn replacement_from(
        state: &LocomotorState,
        kind: LocomotorKind,
        layer: MovementLayer,
        binary_frame: u32,
    ) -> Self {
        let mut runtime = Self::capture(state);
        runtime.kind = kind;
        runtime.layer = layer;
        runtime.common.phase = GroundMovePhase::Idle;
        runtime.payload = LocomotorRuntimePayload::for_kind(kind, binary_frame);
        runtime
    }

    fn install_into(self, state: &mut LocomotorState) {
        state.kind = self.kind;
        state.layer = self.layer;
        state.powered = self.common.powered;
        state.phase = self.common.phase;
        state.speed_multiplier = self.common.speed_multiplier;
        state.speed_fraction = self.common.speed_fraction;
        state.fly_current_speed = self.common.fly_current_speed;
        state.altitude = self.common.altitude;
        state.balloon_hover = self.common.balloon_hover;
        state.hover_attack = self.common.hover_attack;
        state.speed_type = self.common.speed_type;
        state.movement_zone = self.common.movement_zone;
        state.rot = self.common.rot;
        state.air_progress = self.common.air_progress;
        state.infantry_wobble_phase = self.common.infantry_wobble_phase;
        state.subcell_dest = self.common.subcell_dest;
        state.hover_throttle = self.common.hover_throttle;
        state.hover_speed_request = self.common.hover_speed_request;
        state.hover_bob_offset = self.common.hover_bob_offset;
        state.runtime_payload = self.payload;
    }
}

/// A boxed suspended locomotor instance. The box corresponds to the single
/// nested COM object persisted by WalkLocomotionClass::Save.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StashedLocomotor(Box<LocomotorRuntime>);

impl StashedLocomotor {
    /// These legacy height fields mirror the linked owner's height, rather
    /// than a native field belonging to the suspended interface. A real
    /// SetHeight(0) coordinate writer invalidates their previous values.
    pub(crate) fn owner_grounded(&mut self) {
        if matches!(self.0.kind, LocomotorKind::Fly | LocomotorKind::Hover) {
            self.0.common.altitude = crate::util::fixed_math::SIM_ZERO;
        }
        if let LocomotorRuntimePayload::Rocket(Some(rocket)) = &mut self.0.payload {
            rocket.altitude = crate::util::fixed_math::SIM_ZERO;
        }
    }

    pub fn capture(state: &LocomotorState) -> Self {
        Self(Box::new(LocomotorRuntime::capture(state)))
    }

    pub fn from_runtime(runtime: LocomotorRuntime) -> Self {
        Self(Box::new(runtime))
    }

    pub fn into_runtime(self) -> LocomotorRuntime {
        *self.0
    }
}

impl Deref for StashedLocomotor {
    type Target = LocomotorRuntime;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// The outcome of a BEGIN, mirroring the native tri-state return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginOutcome {
    Installed,
    RefusedNull,
    RefusedNested,
}

/// The ownership result of END. `Empty` is native `S_FALSE`; `RefusedNull`
/// represents the native null output pointer, which Rust callers avoid by using
/// `end` or supplying a real destination to `end_into`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndOutcome {
    Restored,
    Empty,
    RefusedNull,
}

/// Stash the active complete runtime and install `incoming` as the new active
/// locomotor. An occupied slot is an atomic E_FAIL-style refusal.
pub fn begin_with_runtime(
    state: &mut LocomotorState,
    incoming: Option<LocomotorRuntime>,
) -> BeginOutcome {
    let Some(incoming) = incoming else {
        return BeginOutcome::RefusedNull;
    };
    if state.piggyback.is_some() {
        return BeginOutcome::RefusedNested;
    }

    state.piggyback = Some(StashedLocomotor::capture(state));
    incoming.install_into(state);
    BeginOutcome::Installed
}

/// Compatibility adapter for callers that name only a replacement class. It
/// still transfers one complete current runtime into the suspended slot.
pub fn begin(
    state: &mut LocomotorState,
    kind: LocomotorKind,
    layer: MovementLayer,
    binary_frame: u32,
) -> BeginOutcome {
    let incoming = LocomotorRuntime::replacement_from(state, kind, layer, binary_frame);
    begin_with_runtime(state, Some(incoming))
}

/// Transfer the suspended runtime into an explicit output location. The active
/// state is unchanged because native END transfers an interface; the caller
/// decides when to install it.
#[cfg(test)]
pub fn end_into(
    state: &mut LocomotorState,
    output: Option<&mut Option<LocomotorRuntime>>,
) -> EndOutcome {
    let Some(output) = output else {
        return EndOutcome::RefusedNull;
    };
    let Some(stashed) = state.piggyback.take() else {
        return EndOutcome::Empty;
    };
    *output = Some(stashed.into_runtime());
    EndOutcome::Restored
}

/// Transfer the suspended runtime back to the host and make it active.
pub fn end(state: &mut LocomotorState) -> Option<StashedLocomotor> {
    let stashed = state.piggyback.take()?;
    let restored = stashed.clone();
    stashed.into_runtime().install_into(state);
    Some(restored)
}

/// The nested-runtime save marker used by the clean-room snapshot seam.
/// Serde persists the following boxed runtime only when this returns one.
#[cfg(test)]
pub fn serialized_presence(state: &LocomotorState) -> u8 {
    u8::from(state.piggyback.is_some())
}

/// Inputs to the class-local `IsOKToEnd` gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndGateContext {
    pub owner_moving: bool,
    pub owner_teleporting: bool,
    pub owner_deploying: bool,
}

/// Whether the active piggyback may be unwound now.
///
/// The movement and populated-slot checks are common. Walk's named-location
/// gate additionally requires its local state and linked owner bytes clear;
/// mapped here to an idle active phase and clear owner transition flags. Special
/// locomotors stay conservative until their Process state machines supply their
/// own completed phase to the caller.
pub fn is_ok_to_end(state: &LocomotorState, context: EndGateContext) -> bool {
    if context.owner_moving || state.piggyback.is_none() {
        return false;
    }

    match state.kind {
        LocomotorKind::Drive
        | LocomotorKind::Walk
        | LocomotorKind::Hover
        | LocomotorKind::Mech
        | LocomotorKind::Ship => {
            state.phase == GroundMovePhase::Idle
                && !context.owner_teleporting
                && !context.owner_deploying
        }
        LocomotorKind::Jumpjet => {
            // The Jumpjet's own state field; `AirMovePhase` is Fly's.
            state
                .jumpjet_runtime()
                .is_none_or(|runtime| runtime.phase == super::super::jumpjet_flight::STATE_GROUND)
                && !context.owner_teleporting
                && !context.owner_deploying
        }
        // Neither native class exposes IPiggyback; it cannot finish a stash.
        LocomotorKind::Fly | LocomotorKind::Parachute => false,
        // `TeleportLocomotionClass::Is_Ok_To_End` is a real six-clause
        // predicate, not a constant false: the locomotor's own warp-active byte
        // must be clear, a runtime must be stashed, the owner's chrono-warp
        // field must be clear, the pending warp phase must be zero and the
        // owner must not be deploying. VERA carries the whole warp in
        // `teleport_state`, so that one flag stands for the warp-active byte
        // and the pending warp phase together. `+0x35` is a **locomotor** byte,
        // not an owner one: `TeleportLocomotionClass::Is_Ok_To_End` @
        // `0x00719F30` reads `*(char*)(this+0x1D)` where `this` is the
        // IPiggyback sub-object at LocomotorBase+0x18. Only `+0x27C` is an
        // owner field, and neither `+0x35` nor `+0x27C` has a Rust model —
        // VERA-internal, gamemd equivalent UNCHECKED. The other two clauses do:
        // `Is_Moving() == 0` is approximated by `context.owner_moving` and
        // `owner+0x6AD` (`FootClass::bIsDeploying`) by `context.owner_deploying`. This is the clause that hands a chrono-warped unit back to
        // its own locomotor when the warp finishes.
        LocomotorKind::Teleport => !context.owner_teleporting && !context.owner_deploying,
        // Tunnel (`0x00728A00`) and Rocket (`0x00661EC0`) have no `IPiggyback`
        // vtable at all, so gamemd can never reach an END gate for them. (The
        // earlier reason given here, that no stock `Locomotor=` key selects
        // them, is wrong for Rocket: `[V3ROCKET]`, `[DMISL]` and `[CMISL]` all
        // do.)
        //
        // **DropPod is different and this arm is VERA-internal for it.**
        // `DropPodLocomotionClass::Constructor` @ `0x004B5AB0` installs the
        // IPiggyback vtable at `0x007E8254`, `Begin_Piggyback` @ `0x004B63B0`
        // is a full body, and `Is_Ok_To_End` @ `0x004B6440` is exactly this
        // module's common prefix — `!Is_Moving() && slot != 0` — so `false` is
        // the wrong answer for it. Trigger: a DropPod locomotor with a live
        // piggyback. Player effect: none — zero stock users of the class.
        // Frequency: zero in ordinary skirmish. Downstream risk: the arm is one
        // line from correct once anything installs one.
        LocomotorKind::Tunnel | LocomotorKind::Rocket | LocomotorKind::DropPod => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::movement::tunnel_movement::{TunnelPhase, TunnelState};

    fn teleporter() -> LocomotorState {
        LocomotorState::for_test_kind(LocomotorKind::Teleport)
    }

    #[test]
    fn begin_rejects_null_and_nested_runtime_without_mutation() {
        let mut state = teleporter();
        assert_eq!(
            begin_with_runtime(&mut state, None),
            BeginOutcome::RefusedNull
        );
        let before = state.clone();

        assert_eq!(
            begin(&mut state, LocomotorKind::Drive, MovementLayer::Ground, 0,),
            BeginOutcome::Installed
        );
        let nested_before = state.clone();
        assert_eq!(
            begin(&mut state, LocomotorKind::Ship, MovementLayer::Ground, 0,),
            BeginOutcome::RefusedNested
        );
        assert_eq!(state.piggyback, nested_before.piggyback);
        assert_eq!(before.kind, LocomotorKind::Teleport);
    }

    #[test]
    fn begin_and_end_transfer_complete_runtime_without_touching_installed_slot() {
        let mut state = teleporter();
        state.altitude = SimFixed::from_num(123);
        state.hover_speed_request = SimFixed::from_num(1);
        let installed = state.slot;

        assert_eq!(
            begin(&mut state, LocomotorKind::Drive, MovementLayer::Ground, 0,),
            BeginOutcome::Installed
        );
        assert_eq!(state.kind, LocomotorKind::Drive);
        assert_eq!(state.slot, installed);
        assert_eq!(
            state.piggyback.as_deref().expect("stash").common.altitude,
            SimFixed::from_num(123)
        );

        let restored = end(&mut state).expect("suspended runtime");
        assert_eq!(restored.kind, LocomotorKind::Teleport);
        assert_eq!(state.kind, LocomotorKind::Teleport);
        assert_eq!(state.altitude, SimFixed::from_num(123));
        assert_eq!(state.slot, installed);
        assert!(state.piggyback.is_none());
    }

    #[test]
    fn piggyback_restores_complete_typed_special_payload() {
        let mut state = LocomotorState::for_test_kind(LocomotorKind::Tunnel);
        state.runtime_payload = LocomotorRuntimePayload::Tunnel(Some(TunnelState {
            phase: TunnelPhase::UndergroundTravel,
        }));

        assert_eq!(
            begin(&mut state, LocomotorKind::Drive, MovementLayer::Ground, 0,),
            BeginOutcome::Installed
        );
        assert_eq!(
            state.runtime_payload,
            LocomotorRuntimePayload::for_kind(LocomotorKind::Drive, 0)
        );
        assert_eq!(
            state.piggyback.as_deref().map(|runtime| &runtime.payload),
            Some(&LocomotorRuntimePayload::Tunnel(Some(TunnelState {
                phase: TunnelPhase::UndergroundTravel,
            })))
        );

        assert!(end(&mut state).is_some());
        assert_eq!(
            state.runtime_payload,
            LocomotorRuntimePayload::Tunnel(Some(TunnelState {
                phase: TunnelPhase::UndergroundTravel,
            }))
        );
    }

    #[test]
    fn serde_round_trip_preserves_active_and_suspended_payloads() {
        let mut state = LocomotorState::for_test_kind(LocomotorKind::Tunnel);
        state.runtime_payload = LocomotorRuntimePayload::Tunnel(Some(TunnelState {
            phase: TunnelPhase::Digging,
        }));
        assert_eq!(
            begin(&mut state, LocomotorKind::DropPod, MovementLayer::Air, 0,),
            BeginOutcome::Installed
        );
        state.runtime_payload = LocomotorRuntimePayload::DropPod(None);

        let bytes = bincode::serialize(&state).expect("serialize locomotor");
        let loaded: LocomotorState = bincode::deserialize(&bytes).expect("load locomotor");

        assert_eq!(
            loaded.runtime_payload,
            LocomotorRuntimePayload::DropPod(None)
        );
        assert_eq!(
            loaded.piggyback.as_deref().map(|runtime| &runtime.payload),
            Some(&LocomotorRuntimePayload::Tunnel(Some(TunnelState {
                phase: TunnelPhase::Digging,
            })))
        );
    }

    #[test]
    fn end_into_reports_null_empty_and_transfers_without_installing() {
        let mut state = teleporter();
        assert_eq!(end_into(&mut state, None), EndOutcome::RefusedNull);
        let mut output = None;
        assert_eq!(end_into(&mut state, Some(&mut output)), EndOutcome::Empty);

        begin(&mut state, LocomotorKind::Drive, MovementLayer::Ground, 0);
        assert_eq!(
            end_into(&mut state, Some(&mut output)),
            EndOutcome::Restored
        );
        assert_eq!(state.kind, LocomotorKind::Drive);
        assert_eq!(
            output.expect("transferred runtime").kind,
            LocomotorKind::Teleport
        );
        assert!(state.piggyback.is_none());
    }

    #[test]
    fn ordinary_and_special_end_gates_are_distinct() {
        let mut state = teleporter();
        begin(&mut state, LocomotorKind::Drive, MovementLayer::Ground, 0);
        let ready = EndGateContext {
            owner_moving: false,
            owner_teleporting: false,
            owner_deploying: false,
        };
        assert!(is_ok_to_end(&state, ready));

        // `TeleportLocomotionClass::Is_Ok_To_End` refuses while the warp is
        // live and permits once it has finished — it is not a constant false.
        state.kind = LocomotorKind::Teleport;
        assert!(!is_ok_to_end(
            &state,
            EndGateContext {
                owner_teleporting: true,
                ..ready
            }
        ));
        assert!(!is_ok_to_end(
            &state,
            EndGateContext {
                owner_deploying: true,
                ..ready
            }
        ));
        assert!(is_ok_to_end(&state, ready));

        // Classes no stock `Locomotor=` key selects stay conservative.
        state.kind = LocomotorKind::Tunnel;
        assert!(!is_ok_to_end(&state, ready));

        state.kind = LocomotorKind::Drive;
        state.phase = GroundMovePhase::Cruising;
        assert!(!is_ok_to_end(&state, ready));
    }

    #[test]
    fn serialized_presence_matches_the_single_suspended_runtime() {
        let mut state = teleporter();
        assert_eq!(serialized_presence(&state), 0);
        begin(&mut state, LocomotorKind::Drive, MovementLayer::Ground, 0);
        assert_eq!(serialized_presence(&state), 1);
    }
}
