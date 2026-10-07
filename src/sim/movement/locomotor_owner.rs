//! Entity boundary for active Drive instance installation and retirement.
//!
//! Native SetDestination7425F8 reuses an active Drive, otherwise allocates one
//! (41C250), links it (7426C9), begins piggyback (74276F) and swaps (74277E).
//! Constructor4AF540 initializes its own destination/head/track/speed state.
//! END4AF930 transfers the stashed object; owner742587 / FootAI4DAEC3 then
//! release the displaced complete instance, including its private retained
//! state. The generic piggyback gate and other class policies stay with
//! their existing owners. These helpers do not mutate Cell occupation.

use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::game_entity::GameEntity;

pub(crate) fn begin_drive_for_teleporter(entity: &mut GameEntity, binary_frame: u32) -> bool {
    let Some(locomotor) = entity.locomotor.as_mut() else {
        return false;
    };
    locomotor.begin_drive_piggyback_for_teleporter(binary_frame)
}

pub(crate) fn try_restore_primary(entity: &mut GameEntity) -> bool {
    piggyback_end_admitted(entity) && restore_admitted_primary(entity)
}

/// The active locomotor's END gate for an entity-level caller. VERA installs
/// two piggybacks: a Drive over a Teleport primary (the Unit setter's
/// Teleporter arm), behind the Drive's own gate, and the Chrono Warp's
/// Teleport over any Foot's locomotor (`SuperClass::Launch @ 0x006CCB4A`),
/// behind Teleport's ([`teleport_end_admitted`]).
pub(crate) fn piggyback_end_admitted(entity: &GameEntity) -> bool {
    drive_end_admitted(entity) || teleport_end_admitted(entity)
}

/// `TeleportLocomotionClass::Is_Ok_To_End @ 0x00719F30`: not moving (its
/// Is_Moving, `+0x34`), a stash (`+0x48`), the constructor-only `+0x35`
/// clear, no warp latch (Techno `+0x27C`), state (`+0x38`) 0 and no Foot
/// locomotor swap (`+0x6AD`). The warp's payload holds the latch and the
/// state and goes at state 7, so its absence is both tests.
fn teleport_end_admitted(entity: &GameEntity) -> bool {
    entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.piggyback.is_some()
            && locomotor
                .teleport_runtime()
                .is_some_and(|runtime| !runtime.is_moving() && runtime.chrono().is_none())
    }) && !entity.foot_locomotor_swap_active
}

/// Drive IsOKToEnd4AF970 at Foot EnterIdle4D833D, before NavQueue.
/// Native tests IsMoving, stash, Drive+65 and Foot+6AD only. Animation phase
/// and unrelated deploy/teleport adapters cannot add admission gates here.
/// This same class gate applies at other entity-level END callers. A missing
/// lazily allocated Drive payload has its constructor's true permission.
pub(crate) fn try_end_drive_at_foot_idle(entity: &mut GameEntity) -> bool {
    drive_end_admitted(entity) && restore_admitted_primary(entity)
}

/// IsMoving here is the Drive's own Is_Moving (`0x004AFB80`, its destination
/// +34 and its head, not the owner's order): the Drive holds +34 until it
/// arrives, so a Chrono Miner's Drive does not end mid-route. It is not
/// Is_Moving_Now, which also counts hull rotation and live speed.
fn drive_end_admitted(entity: &GameEntity) -> bool {
    entity.locomotor.as_ref().is_some_and(|locomotor| {
        locomotor.active_kind() == LocomotorKind::Drive
            && locomotor.piggyback.is_some()
            && locomotor.drive_end_permitted()
    }) && super::motion_query::is_moving(entity) != Some(true)
        && !entity.foot_locomotor_swap_active
}

/// The caller has already evaluated its END gate, at its own required point in
/// the callback sequence. Keep that timing separate from the instance transfer.
pub(crate) fn restore_admitted_primary(entity: &mut GameEntity) -> bool {
    let physical = super::foot_coordinate::current_coordinate(entity);
    let Some(locomotor) = entity.locomotor.as_mut() else {
        return false;
    };
    let restored = locomotor.end_piggyback();
    if restored {
        // END transfers the controller without moving Object+9C. Restored
        // altitude is controller state, not an addition to this exact XYZ.
        entity.position.exact_z_leptons = Some(physical.z);
    }
    restored
}

impl crate::sim::world::Simulation {
    /// The active locomotor's `Mark_All_Occupation_Bits(0)` (ILocomotion
    /// `+0x9C`): Drive `0x004B48D0` / Ship `0x006A3F00` release their track
    /// occupation, Walk `0x0075CA30`, Teleport `0x0071A090` and Jumpjet
    /// `0x0054D930` clear the owner's raw occupation at their head; Fly and
    /// Rocket take the empty base (`0x004B6620`). Foot Limbo calls it on the
    /// first Limbo (`0x004DB324`), the Chrono Warp on a Unit it arms
    /// (`0x006CCA65`); each body is skipped for an object already in Limbo.
    ///
    /// RESIDUAL: Hover's body (`0x005171C0`, a clear at its Head_To_Coord) is
    /// not ported. Trigger: a Hover Unit limboed or chronoshifted. Effect: its
    /// raw occupation at its head stays until its next Mark.
    pub(crate) fn locomotor_mark_all_occupation_bits_up(&mut self, id: u64) {
        self.release_track_occupation_before_foot_limbo(id);
        self.release_walk_occupation_before_foot_limbo(id);
        self.release_teleport_occupation_before_foot_limbo(id);
        self.release_jumpjet_occupation_before_foot_limbo(id);
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
