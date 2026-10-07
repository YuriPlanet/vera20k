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

use crate::rules::locomotor_type::LocomotorKind;

use super::super::drive_locomotion::{DriveRuntime, ShipRuntime, TrackStateMutation};
use super::super::locomotor::LocomotorState;
use super::super::track_process::TrackFamily;
use crate::sim::components::DriveOccupationFootprint;

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
    /// Full object+36 / interface+32: Is_Really_Moving_Now (75CB20), which
    /// Infantry520F40 asks. Process sets it on a fresh head (75BC2A) and a
    /// paid one (75BD25), and clears it after a failed path search (75AFD5),
    /// on a refused head (75B6A3) and on arrival (75BF60). Stop75ADEC clears
    /// it only with no paid head; Stop_Movement_Animation (75CBC0) always.
    /// It is distinct from IsMoving(+34).
    pub animation_moving: bool,
}

/// Class-local state that travels with the locomotor object.
///
/// Special process state is carried here rather than reconstructed from a phase
/// byte when a complete locomotor is suspended or loaded.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum LocomotorRuntimePayload {
    Drive(DriveRuntime),
    Walk(WalkRuntime),
    Teleport(super::super::teleport_movement::TeleportRuntime),
    Rocket(super::super::rocket_movement::RocketRuntime),
    Hover(super::super::hover::HoverRuntime),
    Ship(ShipRuntime),
    Fly(super::super::fly_height::FlyRuntime),
    Jumpjet(super::super::jumpjet_movement::JumpjetRuntime),
}

impl LocomotorRuntimePayload {
    pub(crate) fn for_kind(kind: LocomotorKind, binary_frame: u32) -> Self {
        match kind {
            LocomotorKind::Drive => Self::Drive(DriveRuntime::at_binary_frame(binary_frame)),
            LocomotorKind::Walk => Self::Walk(WalkRuntime::default()),
            LocomotorKind::Teleport => Self::Teleport(Default::default()),
            LocomotorKind::Rocket => Self::Rocket(
                super::super::rocket_movement::RocketRuntime::constructed(binary_frame),
            ),
            LocomotorKind::Hover => Self::Hover(Default::default()),
            LocomotorKind::Ship => Self::Ship(ShipRuntime::at_binary_frame(binary_frame)),
            LocomotorKind::Fly => Self::Fly(Default::default()),
            LocomotorKind::Jumpjet => Self::Jumpjet(Default::default()),
        }
    }
}

/// The suspended locomotor: the complete object a BEGIN displaced, boxed as
/// the one nested COM object the class `Save` persists.
///
/// It holds no stash of its own because BEGIN refuses when the displaced
/// object has one. Native E_FAIL (`0x004AF8F4`) instead tests the receiving
/// object's own slot, which a fresh object never fills; neither case arises,
/// since the setter reuses an active Drive rather than BEGIN over it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StashedLocomotor(Box<LocomotorState>);

impl Deref for StashedLocomotor {
    type Target = LocomotorState;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl StashedLocomotor {
    /// Forward typed private-storage operations without exposing the boxed
    /// object's mutable state or dispatching its suspended Process.
    pub(in crate::sim::movement) fn apply_track_state_mutation(
        &mut self,
        family: TrackFamily,
        mutation: TrackStateMutation,
    ) -> bool {
        self.0.apply_track_state_mutation(family, mutation)
    }

    pub(in crate::sim::movement) fn take_track_occupation(
        &mut self,
        family: TrackFamily,
    ) -> Option<[Option<DriveOccupationFootprint>; 2]> {
        self.0.take_track_occupation(family)
    }

    pub(in crate::sim::movement) fn forget_track_occupation(
        &mut self,
        mark: DriveOccupationFootprint,
    ) {
        self.0.forget_track_occupation(mark);
    }

    pub(in crate::sim::movement) fn clear_track_occupation_projections(&mut self) {
        self.0.clear_track_occupation_projections();
    }

    #[cfg(test)]
    pub(in crate::sim::movement) fn install_drive_state_for_test(
        &mut self,
        retained: Option<super::super::drive_locomotion::DriveLocomotionRuntime>,
    ) -> bool {
        self.0.install_drive_state_for_test(retained)
    }

    #[cfg(test)]
    pub(in crate::sim::movement) fn install_ship_state_for_test(
        &mut self,
        retained: Option<super::super::drive_locomotion::ShipLocomotionRuntime>,
    ) -> bool {
        self.0.install_ship_state_for_test(retained)
    }

    /// Nothing processes a suspended object; only fixtures write it.
    #[cfg(test)]
    pub(crate) fn suspended_mut_for_test(&mut self) -> &mut LocomotorState {
        &mut self.0
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

/// Stash the active object and install `incoming` in its place. An occupied
/// slot is an atomic E_FAIL-style refusal.
fn begin_with(state: &mut LocomotorState, incoming: Option<LocomotorState>) -> BeginOutcome {
    let Some(incoming) = incoming else {
        return BeginOutcome::RefusedNull;
    };
    if state.piggyback.is_some() {
        return BeginOutcome::RefusedNested;
    }
    let displaced = std::mem::replace(state, incoming);
    state.piggyback = Some(StashedLocomotor(Box::new(displaced)));
    BeginOutcome::Installed
}

/// Supply an effect instance beneath a fixture's existing active class without
/// reconstructing the active class or dropping its retained state.
#[cfg(test)]
pub(crate) fn suspend_effect_for_test(
    active: LocomotorState,
    mut effect: LocomotorState,
) -> LocomotorState {
    assert!(active.piggyback.is_none(), "fixture already has a stash");
    assert_eq!(
        begin_with(&mut effect, Some(active)),
        BeginOutcome::Installed
    );
    effect
}

/// BEGIN with a freshly constructed `kind` object
/// ([`LocomotorState::fresh_linked`]), as every native BEGIN site installs one.
pub fn begin(state: &mut LocomotorState, kind: LocomotorKind, binary_frame: u32) -> BeginOutcome {
    let incoming = state.fresh_linked(kind, binary_frame);
    begin_with(state, Some(incoming))
}

/// Transfer the suspended object into an explicit output location. The active
/// state is unchanged because native END transfers an interface; the caller
/// decides when to install it.
#[cfg(test)]
pub fn end_into(
    state: &mut LocomotorState,
    output: Option<&mut Option<LocomotorState>>,
) -> EndOutcome {
    let Some(output) = output else {
        return EndOutcome::RefusedNull;
    };
    let Some(stashed) = state.piggyback.take() else {
        return EndOutcome::Empty;
    };
    *output = Some(*stashed.0);
    EndOutcome::Restored
}

/// Make the suspended object active again. Answers the displaced temporary,
/// which the owner then releases.
///
/// The restored object keeps everything from its BEGIN except the layer,
/// which is the Foot's: END (`0x004AF930`) does not move the Foot, and Drive
/// and Teleport both answer Ground from `In_Which_Layer` (`0x004B4820`,
/// `0x00719E20`), so a Foot that drove onto or off a bridge deck stays there.
/// Its `altitude` is left as the object kept it; neither class reads one.
pub fn end(state: &mut LocomotorState) -> Option<LocomotorState> {
    let stashed = state.piggyback.take()?;
    let released = std::mem::replace(state, *stashed.0);
    state.layer = released.layer;
    Some(released)
}

/// The nested-object save marker used by the clean-room snapshot seam.
/// Serde persists the following boxed object only when this returns one.
#[cfg(test)]
pub fn serialized_presence(state: &LocomotorState) -> u8 {
    u8::from(state.piggyback.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::locomotor_type::SpeedType;
    use crate::sim::movement::locomotor::MovementLayer;
    use crate::util::fixed_math::SimFixed;

    fn teleporter() -> LocomotorState {
        LocomotorState::for_test_kind(LocomotorKind::Teleport)
    }

    #[test]
    fn begin_rejects_null_and_nested_object_without_mutation() {
        let mut state = teleporter();
        assert_eq!(begin_with(&mut state, None), BeginOutcome::RefusedNull);
        let before = state.clone();

        assert_eq!(
            begin(&mut state, LocomotorKind::Drive, 0),
            BeginOutcome::Installed
        );
        let nested_before = state.clone();
        assert_eq!(
            begin(&mut state, LocomotorKind::Ship, 0),
            BeginOutcome::RefusedNested
        );
        assert_eq!(state.piggyback, nested_before.piggyback);
        assert_eq!(before.kind, LocomotorKind::Teleport);
    }

    /// BEGIN installs a freshly constructed object and suspends the displaced
    /// one whole; END restores that object and releases the temporary,
    /// whatever happened to the temporary meanwhile.
    #[test]
    fn begin_and_end_transfer_the_complete_object() {
        let mut state = teleporter();
        state.altitude = SimFixed::from_num(123);
        state.speed_type = SpeedType::Wheel;
        state.power_off();
        let before = state.clone();

        assert_eq!(
            begin(&mut state, LocomotorKind::Drive, 0),
            BeginOutcome::Installed
        );
        assert_eq!(state.kind, LocomotorKind::Drive);
        assert_eq!(state.effective_kind(), LocomotorKind::Teleport);
        // LocomotionClass constructor 0x0055A6C0 raises Powered; the
        // temporary shares only the type's data with the displaced object.
        assert!(state.powered);
        assert_eq!(state.altitude, SimFixed::ZERO);
        assert_eq!(state.speed_type, SpeedType::Wheel);
        assert_eq!(state.piggyback.as_deref(), Some(&before));

        state.altitude = SimFixed::from_num(7);
        let released = end(&mut state).expect("suspended object");
        assert_eq!(released.kind, LocomotorKind::Drive);
        assert_eq!(released.altitude, SimFixed::from_num(7));
        assert_eq!(state, before);
        assert_eq!(state.effective_kind(), LocomotorKind::Teleport);
    }

    /// Neither swap moves the Foot: a Teleport on a bridge deck hands the
    /// deck to its Drive, and a Drive that left the deck hands the ground
    /// back (Drive and Teleport In_Which_Layer `0x004B4820` / `0x00719E20`
    /// both answer Ground; `Bridge` is the Foot's OnBridge).
    #[test]
    fn begin_and_end_keep_the_foot_layer() {
        let mut state = teleporter();
        state.layer = MovementLayer::Bridge;

        assert_eq!(
            begin(&mut state, LocomotorKind::Drive, 0),
            BeginOutcome::Installed
        );
        assert_eq!(state.layer, MovementLayer::Bridge);

        state.layer = MovementLayer::Ground;
        let released = end(&mut state).expect("suspended object");
        assert_eq!(released.layer, MovementLayer::Ground);
        assert_eq!(state.kind, LocomotorKind::Teleport);
        assert_eq!(state.layer, MovementLayer::Ground);
    }

    #[test]
    fn piggyback_restores_complete_typed_special_payload() {
        let mut hover = super::super::super::hover::HoverRuntime::default();
        hover.set_head(Some(crate::sim::components::DriveCoord::cell(4, 5, 0)));
        let mut state = LocomotorState::for_test_kind(LocomotorKind::Hover);
        state.runtime_payload = LocomotorRuntimePayload::Hover(hover.clone());

        assert_eq!(
            begin(&mut state, LocomotorKind::Drive, 0),
            BeginOutcome::Installed
        );
        assert_eq!(
            state.runtime_payload,
            LocomotorRuntimePayload::for_kind(LocomotorKind::Drive, 0)
        );
        assert_eq!(
            state
                .piggyback
                .as_deref()
                .map(|stashed| &stashed.runtime_payload),
            Some(&LocomotorRuntimePayload::Hover(hover.clone()))
        );

        assert!(end(&mut state).is_some());
        assert_eq!(state.runtime_payload, LocomotorRuntimePayload::Hover(hover));
    }

    #[test]
    fn serde_round_trip_preserves_active_and_suspended_payloads() {
        let mut hover = super::super::super::hover::HoverRuntime::default();
        hover.set_head(Some(crate::sim::components::DriveCoord::cell(6, 7, 0)));
        let mut state = LocomotorState::for_test_kind(LocomotorKind::Hover);
        state.runtime_payload = LocomotorRuntimePayload::Hover(hover.clone());
        assert_eq!(
            begin(&mut state, LocomotorKind::Rocket, 0),
            BeginOutcome::Installed
        );

        let bytes = bincode::serialize(&state).expect("serialize locomotor");
        let loaded: LocomotorState = bincode::deserialize(&bytes).expect("load locomotor");

        assert_eq!(
            loaded.runtime_payload,
            LocomotorRuntimePayload::Rocket(
                super::super::super::rocket_movement::RocketRuntime::constructed(0)
            )
        );
        assert_eq!(
            loaded
                .piggyback
                .as_deref()
                .map(|stashed| &stashed.runtime_payload),
            Some(&LocomotorRuntimePayload::Hover(hover.clone()))
        );
    }

    #[test]
    fn end_into_reports_null_empty_and_transfers_without_installing() {
        let mut state = teleporter();
        assert_eq!(end_into(&mut state, None), EndOutcome::RefusedNull);
        let mut output = None;
        assert_eq!(end_into(&mut state, Some(&mut output)), EndOutcome::Empty);

        begin(&mut state, LocomotorKind::Drive, 0);
        assert_eq!(
            end_into(&mut state, Some(&mut output)),
            EndOutcome::Restored
        );
        assert_eq!(state.kind, LocomotorKind::Drive);
        assert_eq!(
            output.expect("transferred object").kind,
            LocomotorKind::Teleport
        );
        assert!(state.piggyback.is_none());
    }

    #[test]
    fn serialized_presence_matches_the_single_suspended_object() {
        let mut state = teleporter();
        assert_eq!(serialized_presence(&state), 0);
        begin(&mut state, LocomotorKind::Drive, 0);
        assert_eq!(serialized_presence(&state), 1);
    }
}
