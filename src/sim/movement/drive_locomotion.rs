//! DriveLocomotion runtime helpers.
//!
//! This module owns Drive-specific state updates. Detailed DriveTrack
//! consumption remains in `track_host`; this file owns private per-instance
//! storage and the Drive/Ship speed prefix. Storage operations do not
//! duplicate native movement ports.

use super::locomotion::piggyback::LocomotorRuntimePayload;
use super::locomotor::LocomotorState;
use super::slope_transition::SlopeTransitionState;
use super::track_process::TrackFamily;
use crate::rules::locomotor_type::LocomotorKind;
#[cfg(any(test, feature = "test-support"))]
use crate::sim::components::FootSpeedState;
use crate::sim::components::{DriveCoord, TrackProgress};
use crate::sim::game_entity::GameEntity;
use crate::util::fixed_math::{SIM_ZERO, SimFixed};

/// ShipLocomotion-owned destination, committed head, and target speed state.
///
/// Ships share the ordinary TurnTrack/RawTrack curves, target fraction,
/// Force_Track (`0x006A0310`, Drive's twin) and the tube entry (`0x006A095F`,
/// Drive's `0x004B1297`) with Drive.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ShipLocomotionRuntime {
    #[serde(default)]
    destination: Option<DriveCoord>,
    #[serde(default)]
    head_to: Option<DriveCoord>,
    #[serde(default)]
    track: TrackProgress,
    /// Ship+63, independent of head XYZ; admission6A05FC reads this byte.
    #[serde(default)]
    track_valid: bool,
    /// Native class+62: last eligible Process observation of body rotation.
    /// Do_Turn and track-point facing updates do not write this latch.
    #[serde(default)]
    turn_latched: bool,
    #[serde(default)]
    target_speed_fraction: SimFixed,
}

/// DriveLocomotion-owned destination/head-to state.
///
/// Native can clear destination,
/// head-to, and active track state at different points in the lifecycle.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct DriveLocomotionRuntime {
    #[serde(default)]
    destination: Option<DriveCoord>,
    #[serde(default)]
    head_to: Option<DriveCoord>,
    #[serde(default)]
    track: TrackProgress,
    /// Drive+65, seeded true at constructor4AF5BB. Native4B4BE0/4B4BF0
    /// disable/enable END while Foot Find_Path removes a Team membership.
    #[serde(default = "drive_end_permitted_default")]
    end_permitted: bool,
    #[serde(default)]
    track_valid: bool,
    /// Drive+64: ctor4AF5B8 clears it; fresh selection4B400C/4B4012
    /// stores the crush-straight decision independently of the selector.
    /// END/Force_Track and selector retirement do not clear this byte.
    #[serde(default)]
    straight: bool,
    /// Native class+62: last eligible Process observation of body rotation.
    /// Do_Turn and track-point facing updates do not write this latch.
    #[serde(default)]
    turn_latched: bool,
    #[serde(default)]
    target_speed_fraction: SimFixed,
}

fn drive_end_permitted_default() -> bool {
    true
}

impl Default for DriveLocomotionRuntime {
    fn default() -> Self {
        Self {
            destination: None,
            head_to: None,
            track: TrackProgress::default(),
            end_permitted: true,
            track_valid: false,
            straight: false,
            turn_latched: false,
            target_speed_fraction: SIM_ZERO,
        }
    }
}

impl DriveLocomotionRuntime {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn destination(&self) -> Option<DriveCoord> {
        self.destination
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn head_to(&self) -> Option<DriveCoord> {
        self.head_to
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn track(&self) -> TrackProgress {
        self.track
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn track_valid(&self) -> bool {
        self.track_valid
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn turn_latched(&self) -> bool {
        self.turn_latched
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn target_speed_fraction(&self) -> SimFixed {
        self.target_speed_fraction
    }
    pub(crate) fn end_permitted(&self) -> bool {
        self.end_permitted
    }
    pub(crate) fn straight(&self) -> bool {
        self.straight
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_destination_for_test(mut self, value: Option<DriveCoord>) -> Self {
        self.destination = value;
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_head_to_for_test(mut self, value: Option<DriveCoord>) -> Self {
        self.head_to = value;
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_track_for_test(mut self, value: TrackProgress) -> Self {
        self.track = value;
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_track_valid_for_test(mut self, value: bool) -> Self {
        self.track_valid = value;
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_turn_latched_for_test(mut self, value: bool) -> Self {
        self.turn_latched = value;
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_target_speed_fraction_for_test(mut self, value: SimFixed) -> Self {
        self.target_speed_fraction = value;
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_end_permitted_for_test(mut self, value: bool) -> Self {
        self.end_permitted = value;
        self
    }
}

impl ShipLocomotionRuntime {
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn destination(&self) -> Option<DriveCoord> {
        self.destination
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn head_to(&self) -> Option<DriveCoord> {
        self.head_to
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn track(&self) -> TrackProgress {
        self.track
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn track_valid(&self) -> bool {
        self.track_valid
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn turn_latched(&self) -> bool {
        self.turn_latched
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn target_speed_fraction(&self) -> SimFixed {
        self.target_speed_fraction
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_destination_for_test(mut self, value: Option<DriveCoord>) -> Self {
        self.destination = value;
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_head_to_for_test(mut self, value: Option<DriveCoord>) -> Self {
        self.head_to = value;
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_track_for_test(mut self, value: TrackProgress) -> Self {
        self.track = value;
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_track_valid_for_test(mut self, value: bool) -> Self {
        self.track_valid = value;
        self
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_target_speed_fraction_for_test(mut self, value: SimFixed) -> Self {
        self.target_speed_fraction = value;
        self
    }
}

/// Complete Drive4AF540 class payload (tools/spatial_oracle/cmin_dock).
/// Absence preserves the existing lazy adapter;
/// constructing or suspending a locomotor does not materialize retained state.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DriveRuntime {
    slope: SlopeTransitionState,
    retained: Option<DriveLocomotionRuntime>,
}

impl DriveRuntime {
    pub(super) fn at_binary_frame(frame: u32) -> Self {
        Self {
            slope: SlopeTransitionState::at_binary_frame(frame),
            retained: None,
        }
    }

    pub(crate) fn slope(&self) -> &SlopeTransitionState {
        &self.slope
    }
    pub(super) fn slope_mut(&mut self) -> &mut SlopeTransitionState {
        &mut self.slope
    }
    pub(crate) fn retained(&self) -> Option<&DriveLocomotionRuntime> {
        self.retained.as_ref()
    }
}

/// Complete Ship69EC50 class payload (tools/spatial_oracle/foot_bridge_layer).
/// Absence preserves the existing lazy adapter;
/// constructing or suspending a locomotor does not materialize retained state.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ShipRuntime {
    slope: SlopeTransitionState,
    retained: Option<ShipLocomotionRuntime>,
}

impl ShipRuntime {
    pub(super) fn at_binary_frame(frame: u32) -> Self {
        Self {
            slope: SlopeTransitionState::at_binary_frame(frame),
            retained: None,
        }
    }

    pub(crate) fn slope(&self) -> &SlopeTransitionState {
        &self.slope
    }
    pub(super) fn slope_mut(&mut self) -> &mut SlopeTransitionState {
        &mut self.slope
    }
    pub(crate) fn retained(&self) -> Option<&ShipLocomotionRuntime> {
        self.retained.as_ref()
    }
}

/// Both reads and conditional writes choose the same complete class object.
/// A matching installed class with absent retained state masks older stashes.
#[derive(Clone, Copy)]
enum TrackStorageSelection {
    Installed,
    Stashed,
    Absent,
}

enum TrackRuntimeRef<'a> {
    Drive(&'a DriveRuntime),
    Ship(&'a ShipRuntime),
}
enum TrackRetainedRef<'a> {
    Drive(&'a DriveLocomotionRuntime),
    Ship(&'a ShipLocomotionRuntime),
}
enum TrackRetainedMut<'a> {
    Drive(&'a mut DriveLocomotionRuntime),
    Ship(&'a mut ShipLocomotionRuntime),
}

/// Typed private-storage forwarding through the existing boxed-stash owner.
/// These operations neither allocate nor implement Move/Stop/Process decisions.
pub(super) enum TrackStateMutation {
    Destination(Option<DriveCoord>),
    Head(Option<DriveCoord>),
    Progress(TrackProgress),
    Valid(bool),
    TurnLatched(bool),
    DriveStraight(bool),
    TargetFraction(SimFixed),
}

impl TrackRetainedRef<'_> {
    fn destination(&self) -> Option<DriveCoord> {
        match self {
            Self::Drive(state) => state.destination,
            Self::Ship(state) => state.destination,
        }
    }
    fn head_to(&self) -> Option<DriveCoord> {
        match self {
            Self::Drive(state) => state.head_to,
            Self::Ship(state) => state.head_to,
        }
    }
    fn track(&self) -> TrackProgress {
        match self {
            Self::Drive(state) => state.track,
            Self::Ship(state) => state.track,
        }
    }
    fn track_valid(&self) -> bool {
        match self {
            Self::Drive(state) => state.track_valid,
            Self::Ship(state) => state.track_valid,
        }
    }
    fn turn_latched(&self) -> bool {
        match self {
            Self::Drive(state) => state.turn_latched,
            Self::Ship(state) => state.turn_latched,
        }
    }
    fn target_speed_fraction(&self) -> SimFixed {
        match self {
            Self::Drive(state) => state.target_speed_fraction,
            Self::Ship(state) => state.target_speed_fraction,
        }
    }
}

impl TrackRetainedMut<'_> {
    fn apply(self, mutation: TrackStateMutation) {
        match mutation {
            TrackStateMutation::Destination(value) => match self {
                Self::Drive(state) => state.destination = value,
                Self::Ship(state) => state.destination = value,
            },
            TrackStateMutation::Head(value) => match self {
                Self::Drive(state) => state.head_to = value,
                Self::Ship(state) => state.head_to = value,
            },
            TrackStateMutation::Progress(value) => match self {
                Self::Drive(state) => state.track = value,
                Self::Ship(state) => state.track = value,
            },
            TrackStateMutation::Valid(value) => match self {
                Self::Drive(state) => state.track_valid = value,
                Self::Ship(state) => state.track_valid = value,
            },
            TrackStateMutation::TurnLatched(value) => match self {
                Self::Drive(state) => state.turn_latched = value,
                Self::Ship(state) => state.turn_latched = value,
            },
            TrackStateMutation::DriveStraight(value) => match self {
                Self::Drive(state) => state.straight = value,
                Self::Ship(_) => unreachable!("Drive-only track state selected a Ship"),
            },
            TrackStateMutation::TargetFraction(value) => match self {
                Self::Drive(state) => state.target_speed_fraction = value,
                Self::Ship(state) => state.target_speed_fraction = value,
            },
        }
    }
}

impl LocomotorState {
    fn track_storage_selection(&self, family: TrackFamily) -> TrackStorageSelection {
        let kind = match family {
            TrackFamily::Drive => LocomotorKind::Drive,
            TrackFamily::Ship => LocomotorKind::Ship,
        };
        if self.active_kind() == kind {
            match (family, &self.runtime_payload()) {
                (TrackFamily::Drive, LocomotorRuntimePayload::Drive(_))
                | (TrackFamily::Ship, LocomotorRuntimePayload::Ship(_)) => {
                    TrackStorageSelection::Installed
                }
                _ => TrackStorageSelection::Absent,
            }
        } else if self.piggyback().is_some() {
            TrackStorageSelection::Stashed
        } else {
            TrackStorageSelection::Absent
        }
    }

    fn selected_track_runtime(&self, family: TrackFamily) -> Option<TrackRuntimeRef<'_>> {
        match self.track_storage_selection(family) {
            TrackStorageSelection::Installed => match &self.runtime_payload() {
                LocomotorRuntimePayload::Drive(state) => Some(TrackRuntimeRef::Drive(state)),
                LocomotorRuntimePayload::Ship(state) => Some(TrackRuntimeRef::Ship(state)),
                _ => None,
            },
            TrackStorageSelection::Stashed => {
                self.piggyback().as_ref()?.selected_track_runtime(family)
            }
            TrackStorageSelection::Absent => None,
        }
    }

    fn selected_track_retained(&self, family: TrackFamily) -> Option<TrackRetainedRef<'_>> {
        match self.selected_track_runtime(family)? {
            TrackRuntimeRef::Drive(state) => state.retained.as_ref().map(TrackRetainedRef::Drive),
            TrackRuntimeRef::Ship(state) => state.retained.as_ref().map(TrackRetainedRef::Ship),
        }
    }

    fn installed_track_retained_mut(&mut self) -> Option<TrackRetainedMut<'_>> {
        match self.active_kind() {
            LocomotorKind::Drive => self
                .drive_runtime_mut()?
                .retained
                .as_mut()
                .map(TrackRetainedMut::Drive),
            LocomotorKind::Ship => self
                .ship_runtime_mut()?
                .retained
                .as_mut()
                .map(TrackRetainedMut::Ship),
            _ => None,
        }
    }

    /// Selected immutable class view; this never dispatches suspended Process.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn selected_drive_runtime(&self) -> Option<&DriveRuntime> {
        match self.selected_track_runtime(TrackFamily::Drive)? {
            TrackRuntimeRef::Drive(state) => Some(state),
            _ => None,
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn selected_ship_runtime(&self) -> Option<&ShipRuntime> {
        match self.selected_track_runtime(TrackFamily::Ship)? {
            TrackRuntimeRef::Ship(state) => Some(state),
            _ => None,
        }
    }

    /// Only existing lazy allocation sites may call this. Stashes are never
    /// selected, reset or materialized by an installed-instance initializer.
    pub fn ensure_installed_track_state(&mut self) -> bool {
        if let Some(state) = self.drive_runtime_mut() {
            state.retained.get_or_insert_with(Default::default);
            true
        } else if let Some(state) = self.ship_runtime_mut() {
            state.retained.get_or_insert_with(Default::default);
            true
        } else {
            false
        }
    }

    pub(crate) fn has_track_state(&self, family: TrackFamily) -> bool {
        self.selected_track_retained(family).is_some()
    }
    pub fn track_destination(&self, family: TrackFamily) -> Option<DriveCoord> {
        self.selected_track_retained(family)
            .and_then(|state| state.destination())
    }
    pub fn track_head(&self, family: TrackFamily) -> Option<DriveCoord> {
        self.selected_track_retained(family)
            .and_then(|state| state.head_to())
    }
    pub fn track_progress(&self, family: TrackFamily) -> Option<TrackProgress> {
        self.selected_track_retained(family)
            .map(|state| state.track())
    }
    pub fn track_valid(&self, family: TrackFamily) -> Option<bool> {
        self.selected_track_retained(family)
            .map(|state| state.track_valid())
    }
    pub(crate) fn track_turn_latched(&self, family: TrackFamily) -> Option<bool> {
        self.selected_track_retained(family)
            .map(|state| state.turn_latched())
    }
    /// The selected complete Drive keeps this byte while suspended; callers
    /// still decide whether that class is executing before reading it.
    pub(crate) fn drive_straight(&self) -> Option<bool> {
        match self.selected_track_retained(TrackFamily::Drive)? {
            TrackRetainedRef::Drive(state) => Some(state.straight()),
            TrackRetainedRef::Ship(_) => None,
        }
    }
    pub(crate) fn track_target_fraction(&self, family: TrackFamily) -> Option<SimFixed> {
        self.selected_track_retained(family)
            .map(|state| state.target_speed_fraction())
    }

    /// Constructor permission for an absent installed Drive remains true.
    pub(crate) fn drive_end_permitted(&self) -> bool {
        match (self.active_kind(), &self.runtime_payload()) {
            (LocomotorKind::Drive, LocomotorRuntimePayload::Drive(state)) => state
                .retained
                .as_ref()
                .is_none_or(|state| state.end_permitted()),
            _ => true,
        }
    }

    pub(super) fn apply_track_state_mutation(
        &mut self,
        family: TrackFamily,
        mutation: TrackStateMutation,
    ) -> bool {
        match self.track_storage_selection(family) {
            TrackStorageSelection::Installed => {
                let Some(state) = self.installed_track_retained_mut() else {
                    return false;
                };
                state.apply(mutation);
                true
            }
            TrackStorageSelection::Stashed => self
                .piggyback_mut()
                .unwrap()
                .apply_track_state_mutation(family, mutation),
            TrackStorageSelection::Absent => false,
        }
    }
    pub fn store_track_destination(
        &mut self,
        family: TrackFamily,
        value: Option<DriveCoord>,
    ) -> bool {
        self.apply_track_state_mutation(family, TrackStateMutation::Destination(value))
    }
    pub(crate) fn store_track_head(
        &mut self,
        family: TrackFamily,
        value: Option<DriveCoord>,
    ) -> bool {
        self.apply_track_state_mutation(family, TrackStateMutation::Head(value))
    }
    pub(crate) fn store_track_progress(
        &mut self,
        family: TrackFamily,
        value: TrackProgress,
    ) -> bool {
        self.apply_track_state_mutation(family, TrackStateMutation::Progress(value))
    }
    pub(crate) fn store_track_valid(&mut self, family: TrackFamily, value: bool) -> bool {
        self.apply_track_state_mutation(family, TrackStateMutation::Valid(value))
    }
    pub(crate) fn store_track_turn_latched(&mut self, family: TrackFamily, value: bool) -> bool {
        self.apply_track_state_mutation(family, TrackStateMutation::TurnLatched(value))
    }
    pub(crate) fn store_drive_straight(&mut self, value: bool) -> bool {
        self.apply_track_state_mutation(
            TrackFamily::Drive,
            TrackStateMutation::DriveStraight(value),
        )
    }
    pub(crate) fn store_track_target_fraction(
        &mut self,
        family: TrackFamily,
        value: SimFixed,
    ) -> bool {
        self.apply_track_state_mutation(family, TrackStateMutation::TargetFraction(value))
    }

    pub(crate) fn store_drive_end_permission(&mut self, value: bool) -> bool {
        match self.drive_runtime_mut() {
            Some(state) => {
                let Some(state) = state.retained.as_mut() else {
                    return false;
                };
                state.end_permitted = value;
                true
            }
            None => false,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn install_drive_state_for_test(
        &mut self,
        retained: Option<DriveLocomotionRuntime>,
    ) -> bool {
        match self.track_storage_selection(TrackFamily::Drive) {
            TrackStorageSelection::Installed => match self.runtime_payload_mut() {
                LocomotorRuntimePayload::Drive(state) => {
                    state.retained = retained;
                    true
                }
                _ => false,
            },
            TrackStorageSelection::Stashed => self
                .piggyback_mut()
                .unwrap()
                .install_drive_state_for_test(retained),
            TrackStorageSelection::Absent => false,
        }
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn install_ship_state_for_test(
        &mut self,
        retained: Option<ShipLocomotionRuntime>,
    ) -> bool {
        match self.track_storage_selection(TrackFamily::Ship) {
            TrackStorageSelection::Installed => match self.runtime_payload_mut() {
                LocomotorRuntimePayload::Ship(state) => {
                    state.retained = retained;
                    true
                }
                _ => false,
            },
            TrackStorageSelection::Stashed => self
                .piggyback_mut()
                .unwrap()
                .install_ship_state_for_test(retained),
            TrackStorageSelection::Absent => false,
        }
    }
}

// Drive qwords 0x7E6240/0x7E6248/0x7E6250 (Ship 0x7F1308/0x7F1310/0x7F1318):
// the 0.3 brake floor, the 0.1 sinking floor and the 0.0015 sinking brake,
// promoted binary32; crush cap 0x7E3548 (binary64 0.2).
const DRIVE_DESTINATION_BRAKE_FLOOR: SimFixed = SimFixed::lit("0.3");
const SINKING_BRAKE_FLOOR: SimFixed = SimFixed::lit("0.1");
const CRUSH_SLOWDOWN_CAP: SimFixed = SimFixed::lit("0.2");

/// Drive4B0EF0, Ship6A05C0, Hover516370 and Teleport7192C0 have identical
/// ILocomotion+4C bodies: Set4C9220 on the owner's PrimaryFacing+388.
/// The active-slot adapter calls this same delegate; Drive Process also
/// calls its concrete owner directly. A repeated destination retains its
/// running timer (tools/mcv_deploy_oracle.json turns).
pub(crate) fn drive_do_turn(entity: &mut GameEntity, desired: u16, frame: u32) {
    entity.body_facing.set(desired, frame);
}

/// The live inputs of one Drive/Ship speed prefix (Drive 0x004B0F20..0x004B1295,
/// Ship 0x006A05F0..0x006A095D), read by `track_speed::advance`.
pub(super) struct TrackSpeedPrefix {
    /// Accelerates= (type +0xDBD).
    pub accelerates: bool,
    /// A Unit whose type is Passive= (type +0xE0C).
    pub unit_passive: bool,
    /// The class selector (+0x58).
    pub selector: i32,
    /// The type's stored speed (type +0x678 through vt+0x38C), leptons per frame.
    pub raw_type_speed: i32,
    pub accel: SimFixed,
    pub decel: SimFixed,
    /// SlowdownDistance= (type +0x2F8).
    pub slowdown_distance: i32,
    /// Techno+0x3CD (`SinkingState`).
    pub sinking: bool,
    /// Foot+0x6B5.
    pub crush_slowdown: bool,
}

/// What one prefix wrote: the class target (+0x50) and, when it called the
/// Foot's `SetSpeedFraction` (0x004D3710), the fraction it passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TrackSpeedStep {
    pub target: SimFixed,
    pub set_fraction: Option<SimFixed>,
}

/// One speed prefix over the class target and the Foot's applied fraction
/// (+0x578). `distance` (`track_speed::braking_distance`) is measured only
/// past the gates, as native reaches it (0x004B0FBA). Executed native rows:
/// tools/spatial_oracle/track_speed_native.json.
///
/// - Accelerates=false passes the target to the setter.
/// - A selector of 64 or more, or a Passive Unit, skips the ramp.
/// - Inside SlowdownDistance (strict) the applied fraction brakes by raw type
///   speed x Deceleration down to 0.3; a sinking Techno brakes by 0.0015 x
///   raw speed down to 0.1 instead.
/// - The crush slowdown caps the target at 0.2 and applies it, braking or not.
/// - Otherwise the applied fraction ramps toward the target: up by
///   Acceleration, down by raw speed x Deceleration, never past it. An
///   applied fraction equal to the target calls no setter.
///
/// PRECISION: native works in binary64 x87 (chop53); this port works in
/// `SimFixed` (I16F16). Deceleration= is held to 2^-16, so a brake step can
/// differ from native by about raw speed x 2^-17 (about 1e-4 for a Rhino),
/// below one lepton of speed until raw speed reaches 128.
///
/// RESIDUAL: a Unit's ramp also propagates the applied fraction to its linked
/// members (0x004B1225 / 0x006A08ED); VERA has no linked-member chain.
pub(super) fn track_speed_prefix(
    input: &TrackSpeedPrefix,
    distance: impl FnOnce() -> i32,
    target: SimFixed,
    applied: SimFixed,
) -> TrackSpeedStep {
    let step = |set_fraction| TrackSpeedStep {
        target,
        set_fraction,
    };
    // ProcessMovement retains an unclamped class target. Only the Foot
    // setter clamps the applied fraction to [0,1].
    if !input.accelerates {
        return step(Some(target));
    }
    if input.selector >= 64 || input.unit_passive {
        return step(None);
    }
    let raw = SimFixed::from_num(input.raw_type_speed);
    let type_brake = raw * input.decel;
    // raw x 0.0015 as raw x 3 / 2000, rounded once.
    let sinking_brake = SimFixed::from_num(input.raw_type_speed * 3) / SimFixed::from_num(2000);
    let braking = if distance() < input.slowdown_distance {
        Some((applied - type_brake).max(DRIVE_DESTINATION_BRAKE_FLOOR))
    } else if input.sinking {
        Some((applied - sinking_brake).max(SINKING_BRAKE_FLOOR))
    } else {
        None
    };
    if input.crush_slowdown {
        let capped = target.min(CRUSH_SLOWDOWN_CAP);
        return TrackSpeedStep {
            target: capped,
            set_fraction: Some(capped),
        };
    }
    step(if let Some(braked) = braking {
        Some(braked)
    } else if applied < target {
        Some((applied + input.accel).min(target))
    } else if applied > target {
        Some((applied - type_brake).max(target))
    } else {
        None
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::movement::foot_speed::owner_current_speed_from_fraction;
    use crate::util::fixed_math::{SIM_HALF, SIM_ONE, SIM_ZERO};

    #[test]
    fn complete_track_payload_survives_suspension_serialization_and_end() {
        use super::super::locomotion::piggyback::{self, BeginOutcome};

        for (family, kind) in [
            (TrackFamily::Drive, LocomotorKind::Drive),
            (TrackFamily::Ship, LocomotorKind::Ship),
        ] {
            let mut state = LocomotorState::for_test_kind(kind);
            assert!(!state.has_track_state(family));
            assert!(!state.store_track_valid(family, true));
            assert!(state.ensure_installed_track_state());
            state.active_slope_transition_mut().unwrap().snap(2, 100);
            state
                .active_slope_transition_mut()
                .unwrap()
                .sample_process_entry(5, 200);
            let destination = Some(DriveCoord::cell(9, 8, 416));
            let head = Some(DriveCoord::cell(8, 8, 416));
            let progress = TrackProgress::for_test(3, 7, true, -11);
            assert!(state.store_track_destination(family, destination));
            assert!(state.store_track_head(family, head));
            assert!(state.store_track_progress(family, progress));
            assert!(state.store_track_valid(family, true));
            assert!(state.store_track_turn_latched(family, true));
            assert!(state.store_track_target_fraction(family, SimFixed::lit("1.2")));
            if family == TrackFamily::Drive {
                assert!(state.store_drive_end_permission(false));
                assert_eq!(state.drive_straight(), Some(false));
                let unlatched = state.clone();
                assert!(state.store_drive_straight(true));
                let payload_hash = |state: &LocomotorState| {
                    use std::hash::{Hash, Hasher};
                    let mut hasher = std::hash::DefaultHasher::new();
                    state
                        .selected_drive_runtime()
                        .unwrap()
                        .retained()
                        .hash(&mut hasher);
                    hasher.finish()
                };
                assert_ne!(payload_hash(&state), payload_hash(&unlatched));
            }
            let before = state.clone();
            assert_eq!(
                piggyback::begin(&mut state, LocomotorKind::Teleport, 999),
                BeginOutcome::Installed
            );
            assert_eq!(state.track_destination(family), destination);
            assert_eq!(state.track_head(family), head);
            assert_eq!(state.track_progress(family), Some(progress));
            if family == TrackFamily::Drive {
                assert_eq!(state.drive_straight(), Some(true));
            }
            assert!(!state.ensure_installed_track_state());

            let bytes = serde_json::to_vec(&state).unwrap();
            let mut loaded: LocomotorState = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(loaded, state);
            let released = piggyback::end(&mut loaded).unwrap();
            assert_eq!(released.active_kind(), LocomotorKind::Teleport);
            assert_eq!(loaded, before);
            if family == TrackFamily::Drive {
                assert_eq!(loaded.drive_straight(), Some(true));
                let mut retired = loaded.clone();
                let mut selector = retired.track_progress(family).unwrap();
                selector.clear_selector();
                retired.store_track_progress(family, selector);
                retired.store_track_head(family, None);
                assert_eq!(retired.drive_straight(), Some(true));
            }
            assert_eq!(loaded.track_destination(family), destination);
            assert_eq!(loaded.track_progress(family), Some(progress));
        }
    }

    #[test]
    fn matching_installed_absence_masks_saved_retained_state() {
        use super::super::locomotion::piggyback::{self, BeginOutcome};

        let mut state = LocomotorState::for_test_kind(LocomotorKind::Drive);
        assert!(state.ensure_installed_track_state());
        let destination = Some(DriveCoord::cell(5, 6, 0));
        assert!(state.store_track_destination(TrackFamily::Drive, destination));
        let before = state.clone();
        assert_eq!(
            piggyback::begin(&mut state, LocomotorKind::Drive, 200),
            BeginOutcome::Installed
        );
        assert!(!state.has_track_state(TrackFamily::Drive));
        assert_eq!(state.track_destination(TrackFamily::Drive), None);
        assert!(!state.store_track_destination(TrackFamily::Drive, None));
        assert!(!state.store_drive_end_permission(false));
        assert!(state.drive_end_permitted());
        assert!(state.ensure_installed_track_state());
        assert_eq!(state.track_destination(TrackFamily::Drive), None);
        piggyback::end(&mut state).unwrap();
        assert_eq!(state, before);
        assert_eq!(state.track_destination(TrackFamily::Drive), destination);
    }

    fn prefix(accelerates: bool, raw_type_speed: i32, slowdown_distance: i32) -> TrackSpeedPrefix {
        TrackSpeedPrefix {
            accelerates,
            unit_passive: false,
            selector: 0,
            raw_type_speed,
            accel: SimFixed::lit("0.03"),
            decel: SimFixed::lit("0.002"),
            slowdown_distance,
            sinking: false,
            crush_slowdown: false,
        }
    }

    #[test]
    fn gsi_13_06_stock_shp_current_speed_preserves_native_truncation() {
        use crate::util::fixed_math::ra2_speed_to_leptons_per_second;

        let stock_ship_speed = ra2_speed_to_leptons_per_second(8);
        assert_eq!(
            owner_current_speed_from_fraction(stock_ship_speed, SimFixed::lit("0.03")),
            0,
            "DLPH/SQD first acceleration step truncates to zero"
        );
        assert_eq!(
            owner_current_speed_from_fraction(stock_ship_speed, SimFixed::lit("0.06")),
            1
        );
        let stock_dron_speed = ra2_speed_to_leptons_per_second(10);
        assert_eq!(
            owner_current_speed_from_fraction(stock_dron_speed, SimFixed::lit("0.03")),
            0,
            "DRON's raw stage 25 still truncates 0.75 to zero"
        );
        assert_eq!(
            owner_current_speed_from_fraction(stock_dron_speed, SIM_ONE),
            25,
            "Accelerates=false DRON uses its full converted type speed"
        );
    }

    #[test]
    fn gsi_13_06_nonterminal_ship_post_stop_process_preserves_clamped_target() {
        use crate::util::fixed_math::ra2_speed_to_leptons_per_second;

        let speed = ra2_speed_to_leptons_per_second(8);
        let mut owner_speed = FootSpeedState::default();
        owner_speed.set_speed_fraction(SIM_HALF);
        let ship = ShipLocomotionRuntime::default()
            .with_target_speed_fraction_for_test(SimFixed::lit("0.3"));

        let mut ship = ship;
        for _ in 0..10 {
            let step = track_speed_prefix(
                &prefix(
                    true,
                    crate::util::fixed_math::ra2_speed_to_leptons_per_frame(8),
                    0,
                ),
                || 256,
                ship.target_speed_fraction(),
                owner_speed.applied_fraction(),
            );
            ship = ship.with_target_speed_fraction_for_test(step.target);
            if let Some(fraction) = step.set_fraction {
                owner_speed.set_speed_fraction(fraction);
            }
            assert_eq!(ship.target_speed_fraction(), SimFixed::lit("0.3"));
            assert!(owner_current_speed_from_fraction(speed, owner_speed.applied_fraction()) > 0);
        }

        assert_eq!(owner_speed.applied_fraction(), SimFixed::lit("0.3"));
        assert_eq!(
            owner_current_speed_from_fraction(speed, owner_speed.applied_fraction()),
            6
        );
    }

    fn drive_entity_at(rx: u16, ry: u16) -> GameEntity {
        let mut entity = GameEntity::test_default(1, "HARV", "Americans", rx, ry);
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::for_test_kind(
                crate::rules::locomotor_type::LocomotorKind::Drive,
            ),
        );
        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .ensure_installed_track_state()
        );
        entity
    }

    fn drive_is_moving(entity: &GameEntity) -> bool {
        crate::sim::movement::motion_query::is_moving(entity) == Some(true)
    }

    /// GSI-06.12 GAP 3: a Drive's Is_Moving (ILocomotion+0x10, `0x004AFB80`)
    /// looks at the Drive locomotor's own destination and head-to coords — not
    /// at the owner's order. A non-null destination alone means "moving".
    #[test]
    fn gsi_06_12_drive_is_moving_reads_its_own_destination() {
        let mut entity = drive_entity_at(3, 3);
        assert!(
            !drive_is_moving(&entity),
            "no destination and no head: not moving"
        );

        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .store_track_destination(TrackFamily::Drive, Some(DriveCoord::cell(9, 3, 0)))
        );
        assert!(drive_is_moving(&entity));
    }

    /// With no destination, a null head-to is not moving and a head-to that
    /// already equals the owner's exact lepton X/Y is not moving either. Z is
    /// deliberately not part of the comparison.
    #[test]
    fn gsi_06_12_drive_is_moving_compares_head_to_against_the_owner_position() {
        let mut entity = drive_entity_at(3, 3);
        entity.position.set_sub_x_for_test(SimFixed::from_num(128));
        entity.position.set_sub_y_for_test(SimFixed::from_num(128));

        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .store_track_head(TrackFamily::Drive, Some(DriveCoord::cell(3, 3, 0)))
        );
        assert!(
            !drive_is_moving(&entity),
            "head-to at the owner's own cell centre is not moving"
        );

        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .store_track_head(TrackFamily::Drive, Some(DriveCoord::cell(3, 3, 7)))
        );
        assert!(
            !drive_is_moving(&entity),
            "a Z-only difference does not make it moving"
        );

        assert!(
            entity
                .locomotor
                .as_mut()
                .unwrap()
                .store_track_head(TrackFamily::Drive, Some(DriveCoord::cell(4, 3, 0)))
        );
        assert!(drive_is_moving(&entity));
    }

    #[test]
    fn a_downhill_target_stays_above_one_but_the_owner_fraction_does_not() {
        // `Process_Movement` @ 0x004B2630 writes `drive+0x50` unclamped on its
        // ordinary `drive+0x58 < 0x40` arm (`0x004B3E00`), so a 1.2 downhill
        // product survives on the locomotor-owned slot; the only native clamp is
        // `FootClass::SetSpeedFraction` @ 0x004D3710, and every arm of
        // `Process_Drive_Track` that writes the owner's fraction goes through
        // it, so that fraction never exceeds 1.
        let mut owner_speed = FootSpeedState::default();
        owner_speed.set_speed_fraction(SIM_ZERO);
        let mut drive = DriveLocomotionRuntime::default();

        drive = drive.with_target_speed_fraction_for_test(SimFixed::lit("1.2"));
        let step = track_speed_prefix(
            &prefix(false, 10, 500),
            || 1000,
            drive.target_speed_fraction(),
            owner_speed.applied_fraction(),
        );
        drive = drive.with_target_speed_fraction_for_test(step.target);
        owner_speed.set_speed_fraction(step.set_fraction.unwrap());

        assert_eq!(drive.target_speed_fraction(), SimFixed::lit("1.2"));
        assert_eq!(owner_speed.applied_fraction(), SIM_ONE);
    }
}
