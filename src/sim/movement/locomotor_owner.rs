//! Entity boundary for active Drive instance installation and retirement.
//!
//! Native SetDestination7425F8 reuses an active Drive, otherwise ends an
//! active piggyback that may end (7425FE..742681), allocates a Drive
//! (41C250), links it (7426C9), begins piggyback (74276F) and swaps (74277E).
//! Constructor4AF540 initializes its own destination/head/track/speed state.
//! END4AF930 transfers the stashed object; owner742587 / FootAI4DAEC3 then
//! release the displaced complete instance, including its private retained
//! state. The END owner is [`Simulation::try_end_piggyback`] /
//! [`Simulation::end_admitted_piggyback`]; other class policies stay with
//! their existing owners. These helpers do not mutate Cell occupation.
//!
//! [`Simulation::try_end_piggyback`]: crate::sim::world::Simulation::try_end_piggyback
//! [`Simulation::end_admitted_piggyback`]: crate::sim::world::Simulation::end_admitted_piggyback

use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;

/// The Unit setter's Drive for a Teleporter (`0x007425E6..0x0074277E`): the
/// Chrono Miner stays a Teleport unit while a Drive drives it. An active
/// Drive is reused (`0x007425F8`). Otherwise an active piggyback that may end
/// ends first (`0x007425FE..0x00742681`), and a fresh Drive then suspends
/// whatever is active without the setter asking its class again: a miner
/// whose Chrono Warp Teleport ends onto the Drive it suspended gets a second
/// Drive over that one.
///
/// The Teleporter arm is skipped while a Magnetron holds the object
/// (`0x007423E1..0x00742405`), so the END here is never a lift's.
pub(crate) fn begin_drive_for_teleporter(entity: &mut GameEntity, binary_frame: u32) -> bool {
    let Some(locomotor) = entity.locomotor.as_ref() else {
        return false;
    };
    if locomotor.effective_kind() != LocomotorKind::Teleport {
        return false;
    }
    if locomotor.active_kind() == LocomotorKind::Drive {
        return true;
    }
    if piggyback_end_admitted(entity) {
        let ended = end_piggyback(entity);
        debug_assert!(!ended.lift_ended, "a lifted object takes no Teleporter arm");
    }
    entity
        .locomotor
        .as_mut()
        .is_some_and(|locomotor| locomotor.begin_piggyback(LocomotorKind::Drive, binary_frame))
}

/// The active locomotor's END gate (`IPiggyback::Is_Ok_To_End`). VERA
/// installs three piggybacks, each over whatever is active: a Drive (the
/// Unit setter's Teleporter arm), behind the Drive's own gate, the Chrono
/// Warp's Teleport (`SuperClass::Launch @ 0x006CCB4A`), behind Teleport's
/// ([`teleport_end_admitted`]), and the Magnetron's Jumpjet
/// (`TechnoClass::ImbueLocomotor @ 0x00710000`), behind Jumpjet's
/// ([`jumpjet_end_admitted`]). Each END restores the next object of the
/// chain.
pub(crate) fn piggyback_end_admitted(entity: &GameEntity) -> bool {
    drive_end_admitted(entity) || teleport_end_admitted(entity) || jumpjet_end_admitted(entity)
}

/// `JumpjetLocomotionClass::Is_Ok_To_End @ 0x0054DB00`: not moving (its
/// Is_Moving, the moving byte), a stash (`+0x94`), the constructor-only
/// `+0x91` clear, and no lift or one that may end (Foot `+0x6AD` without
/// `+0x6AE`, `0x0054DB24..0x0054DB36`).
fn jumpjet_end_admitted(entity: &GameEntity) -> bool {
    entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.piggyback().is_some()
            && locomotor
                .jumpjet_runtime()
                .is_some_and(|runtime| !runtime.moving())
    }) && (!entity.lift.swap_active() || entity.lift.may_end())
}

/// `TeleportLocomotionClass::Is_Ok_To_End @ 0x00719F30`: not moving (its
/// Is_Moving, `+0x34`), a stash (`+0x48`), the constructor-only `+0x35`
/// clear, no warp latch (Techno `+0x27C`), state (`+0x38`) 0 and no Foot
/// locomotor swap (`+0x6AD`). The warp's payload holds the latch and the
/// state and goes at state 7, so its absence is both tests.
fn teleport_end_admitted(entity: &GameEntity) -> bool {
    entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.piggyback().is_some()
            && locomotor
                .teleport_runtime()
                .is_some_and(|runtime| !runtime.is_moving() && runtime.chrono().is_none())
    }) && !entity.lift.swap_active()
}

/// `DriveLocomotionClass::Is_Ok_To_End @ 0x004AF970` tests IsMoving, the
/// stash, Drive `+0x65` and Foot `+0x6AD` only; animation phase and unrelated
/// deploy/teleport adapters add no gate. A missing lazily allocated Drive
/// payload has its constructor's true permission.
///
/// IsMoving here is the Drive's own Is_Moving (`0x004AFB80`, its destination
/// +34 and its head, not the owner's order): the Drive holds +34 until it
/// arrives, so a Chrono Miner's Drive does not end mid-route. It is not
/// Is_Moving_Now, which also counts hull rotation and live speed.
fn drive_end_admitted(entity: &GameEntity) -> bool {
    entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.active_kind() == LocomotorKind::Drive
            && locomotor.piggyback().is_some()
            && locomotor.drive_end_permitted()
    }) && super::motion_query::is_moving(entity) != Some(true)
        && !entity.lift.swap_active()
}

/// What one END did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PiggybackEnd {
    /// The stash became active again (`S_OK`); without one, `S_FALSE`.
    restored: bool,
    /// Jumpjet's END ended a Magnetron lift (`+0x6AD` or `+0x6AE` was up).
    lift_ended: bool,
}

/// `End_Piggyback` of the active class, its owner writes then the transfer
/// of the object it suspended: one link of the chain. Drive's
/// (`0x004AF930`) only transfers. Teleport's (`0x00719EE0`) first clears the
/// drop and warp credits (`+0x428`, `+0x42C`). Jumpjet's (`0x0054DA50`)
/// first ends a Magnetron lift (`+0x6AD`, `+0x6AE`) and then clears the
/// credits. The caller has already evaluated its END gate, at its own
/// required point in the callback sequence.
fn end_piggyback(entity: &mut GameEntity) -> PiggybackEnd {
    let physical = super::foot_coordinate::current_coordinate(entity);
    let Some(kind) = entity
        .locomotor
        .as_ref()
        .map(|locomotor| locomotor.active_kind())
    else {
        return PiggybackEnd {
            restored: false,
            lift_ended: false,
        };
    };
    let lift_ended = kind == LocomotorKind::Jumpjet && entity.lift.end();
    if matches!(kind, LocomotorKind::Jumpjet | LocomotorKind::Teleport) {
        entity.drop_credit.forget();
    }
    let restored = entity
        .locomotor
        .as_mut()
        .is_some_and(|locomotor| locomotor.end_piggyback());
    if restored {
        // END transfers the controller without moving Object+9C.
        entity.position.put_z(physical.z);
    }
    PiggybackEnd {
        restored,
        lift_ended,
    }
}

impl crate::sim::world::Simulation {
    /// The callers that QueryInterface the active locomotor for IPiggyback
    /// and END it when its `Is_Ok_To_End` allows, whatever its class: Foot
    /// AI's tail (`0x004DAE5F..0x004DAEC3`), Foot Enter_Idle_Mode
    /// (`0x004D831A..0x004D8376`), the Foot setter's NavQueue arm
    /// (`0x004D9309..0x004D937C`) and the Unit setter's pad arm
    /// (`0x0074250E..0x0074258A`, which asks `Is_Piggybacking` first; the
    /// gates include the stash).
    pub(crate) fn try_end_piggyback(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        self.substrate
            .entities
            .get(id)
            .is_some_and(piggyback_end_admitted)
            && self.end_admitted_piggyback(id, rules)
    }

    /// END of the active piggyback ([`end_piggyback`]) for a caller that has
    /// evaluated its gate. Jumpjet's END that ends a lift also sends a
    /// computer house's `ResourceGatherer=` back to Harvest (`0x0054DA92..
    /// 0x0054DABB`, `Queue_Mission(Harvest, 0)`), before the transfer
    /// natively; the two touch separate state. Returns whether the stash
    /// became active.
    pub(crate) fn end_admitted_piggyback(&mut self, id: u64, rules: Option<&RuleSet>) -> bool {
        let Some(entity) = self.substrate.entities.get_mut(id) else {
            return false;
        };
        let ended = end_piggyback(entity);
        if ended.lift_ended
            && let Some(entity) = self.substrate.entities.get(id)
        {
            let computer = self
                .houses
                .get(&entity.owner())
                .is_some_and(|house| !house.is_human());
            let gatherer = rules
                .and_then(|rules| self.object_type(entity.type_ref(), rules))
                .is_some_and(|object| object.resource_gatherer);
            if computer && gatherer {
                let now = self.session.binary_frame();
                let _ = self.mission_queue_exact(
                    id,
                    crate::sim::mission::MissionId::from_known(
                        crate::sim::mission::MissionType::Harvest,
                    ),
                    0,
                    now,
                    &crate::sim::mission::authority::EntityReadyInputProvider,
                );
            }
        }
        ended.restored
    }

    /// The active locomotor's `Mark_All_Occupation_Bits(mode)` (ILocomotion
    /// `+0x9C`): Drive `0x004B48D0` / Ship `0x006A3F00` release their track
    /// occupation, Hover `0x005171C0`, Walk `0x0075CA30`, Teleport `0x0071A090` and Jumpjet
    /// `0x0054D930` clear the owner's raw occupation at their head; Fly and
    /// Rocket take the empty base (`0x004B6620`). Foot Limbo calls it on the
    /// first Limbo (`0x004DB324`), the Chrono Warp on a Unit it arms
    /// (`0x006CCA65`), and Unit Deploy739692/739A94. The represented
    /// live callers skip an object already in Limbo. Drive/Ship share their
    /// existing Apply_Track_Occupation_Mode owner for both modes.
    ///
    /// Hover's mode1 is an empty return. Mode1 for Walk/Teleport/Jumpjet
    /// remains unported: selected retail MCV and
    /// Slave Miner Deploy use Drive; modded other-class Deploy failure can
    /// miss that class's head claim before its ordinary Foot Mark(DOWN).
    /// Hover5171C0 delegates to the shared Head_To getter514D10, which
    /// substitutes Location for a null head and has no power/limbo gate.
    pub(crate) fn locomotor_mark_all_occupation_bits(&mut self, id: u64, put: bool) {
        let kind = self
            .substrate
            .entities
            .get(id)
            .and_then(|entity| entity.locomotor.as_ref().map(|loco| loco.active_kind()));
        match kind {
            Some(crate::rules::locomotor_type::LocomotorKind::Drive) => {
                self.track_apply_occupation(id, super::track_process::TrackFamily::Drive, put);
            }
            Some(crate::rules::locomotor_type::LocomotorKind::Ship) => {
                self.track_apply_occupation(id, super::track_process::TrackFamily::Ship, put);
            }
            Some(crate::rules::locomotor_type::LocomotorKind::Hover) => {
                self.hover_mark_all_occupation_bits(id, put);
            }
            Some(
                crate::rules::locomotor_type::LocomotorKind::Fly
                | crate::rules::locomotor_type::LocomotorKind::Rocket,
            ) => {}
            _ if !put => {
                self.release_walk_occupation_before_foot_limbo(id);
                self.release_teleport_occupation_before_foot_limbo(id);
                self.release_jumpjet_occupation_before_foot_limbo(id);
            }
            Some(kind) => {
                log::debug!("unported {kind:?} Mark_All_Occupation_Bits(1) for {id}");
            }
            None => {}
        }
    }
}

/// Techno70C5B0/70C5C0 expose the represented warp bytes to Walk/Fly: the
/// teleport's warp-out and warp-in and a Temporal warp.
pub(crate) fn owner_is_warping(entity: &GameEntity) -> bool {
    entity.is_warping_in() || entity.is_warped_out()
}

#[cfg(test)]
#[path = "locomotor_owner_tests.rs"]
mod tests;
