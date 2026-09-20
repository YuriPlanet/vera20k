//! Refinery dock contacts.
//!
//! A refinery admits up to `NumberOfDocks` miners into its `Contacts[]` list
//! (capacity-1 for a stock refinery). The slot count `RadioClass+0xE8` starts
//! at 1 in the `RadioClass` ctor (0x0065A764) and is then sized by
//! `BuildingClass::Constructor` 0x0043BCBD..0x0043BCD0: `MOV EAX,[Type+0x1780]`
//! (`NumberOfDocks=`), `CMP EAX,1 ; JGE`, else `MOV EAX,1`, `PUSH EAX`,
//! `CALL Set_Contact_Count` — i.e. capacity = `max(NumberOfDocks, 1)`, which is
//! what the Rust capacity derives (stock refineries are `NumberOfDocks=1`).
//! gamemd stores **no** wait-queue: a denied miner re-probes on demand and
//! whichever re-probing miner wins a freed slot docks next (V3).
//!
//! The refinery handshake has one record, the radio state on the two objects:
//! the refinery's `GameEntity::radio_contacts` and the miner's
//! `dock_entered_with`. The miner FSM reaches it only through the functions
//! here, which go over [`radio::transmit`]. (Other mechanisms write
//! `radio_contacts` for their own links, e.g. a factory exit; none of them
//! touches a refinery.)
//!
//! ## Dependency rules
//! - Part of sim/ -- no dependencies outside sim/.
//! - sim/ NEVER depends on render/, ui/, sidebar/, audio/, net/.

use crate::sim::radio::{self, RadioMessage, RadioPayload, RadioResponse};
use crate::sim::world::Simulation;

/// Result of a refinery HELLO/contact attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContactAdmission {
    /// The harvester is present in the refinery Contacts[] list.
    Accepted,
    /// The refinery refused (saturated, dying, or not the miner's house). The
    /// harvester re-probes on a later tick; there is no stored wait-queue
    /// (V3 — gamemd keeps none).
    Waiting,
}

/// Send HELLO to a refinery. Widens the refinery's contact slots to
/// `capacity` (grow-only, the `Set_Contact_Count` the building ctor runs) and
/// lets the receiver admit into its first free slot. Idempotent: an
/// already-linked miner re-confirms `Accepted` without a second dispatch.
pub(crate) fn hello(
    sim: &mut Simulation,
    miner_sid: u64,
    refinery_sid: u64,
    capacity: usize,
) -> ContactAdmission {
    if let Some(refinery) = sim.substrate.entities.get_mut(refinery_sid) {
        refinery.radio_contacts.set_capacity(capacity);
    }
    match radio::transmit(
        sim,
        miner_sid,
        refinery_sid,
        RadioMessage::Hello,
        RadioPayload::default(),
    ) {
        RadioResponse::Roger => ContactAdmission::Accepted,
        _ => ContactAdmission::Waiting,
    }
}

/// Whether the refinery's Contacts[] list holds the miner.
pub(crate) fn has_contact(sim: &Simulation, refinery_sid: u64, miner_sid: u64) -> bool {
    sim.substrate
        .entities
        .get(refinery_sid)
        .is_some_and(|refinery| refinery.radio_contacts.contains(miner_sid))
}

/// Read-only contact-slot probe: `RadioClass` helper `FUN_0065ADF0`
/// (gamemd.exe), which walks `Contacts[0..+0xE8)` at `+0xE4` and answers true
/// when a slot holds null or already holds the caller. The refinery scanner
/// `FUN_004DEE80` and `BuildingClass::Receive_Radio @ 0x0043C2D0` case 0xF both
/// consult it before any HELLO is sent, so selection must ask without
/// mutating. Capacity floors at 1, the same `max(NumberOfDocks, 1)` the
/// `BuildingClass` ctor passes to `Set_Contact_Count` (see the module doc). It
/// comes from the type because a refinery that was never HELLOed still holds
/// its default single slot in `radio_contacts`.
pub(crate) fn would_admit(
    sim: &Simulation,
    refinery_sid: u64,
    miner_sid: u64,
    capacity: usize,
) -> bool {
    sim.substrate
        .entities
        .get(refinery_sid)
        .is_none_or(|refinery| {
            refinery.radio_contacts.contains(miner_sid)
                || refinery.radio_contacts.len() < capacity.max(1)
        })
}

/// The refinery's HELLO ally gate as a predicate: `refinery_hello` answers
/// NEGATORY to another house for as long as that holds, so a reservation on
/// such a refinery can never complete. The FSM drops it and selects again, the
/// same way it treats a refinery that died. Reached when an engineer captures
/// the reserved refinery or the miner changes house mid-return.
pub(crate) fn same_house(sim: &Simulation, refinery_sid: u64, miner_sid: u64) -> bool {
    let entities = &sim.substrate.entities;
    match (entities.get(refinery_sid), entities.get(miner_sid)) {
        (Some(refinery), Some(miner)) => refinery.owner() == miner.owner(),
        _ => false,
    }
}

/// `EventClass::Execute`'s MEGAMISSION arm, `0x004C72E8..0x004C7342`: a unit
/// that is not tethered (`+0x418` clear) transmits BREAK (`PUSH 3; CALL
/// [vt+0x274]`, `0x004C72F8`); a tethered one does so only when its contact is
/// a `Refinery=` building (`Type+0x16B3`, `0x004C732C`), and then also clears
/// `+0x418` (`0x004C7342`). For a miner both arms end the refinery handshake,
/// so a retasked miner frees the slot for the next one.
///
/// What follows the BREAK depends on the mission the miner is in. Before the
/// unload (Harvest/Enter phases) the handshake simply restarts from HELLO the
/// next time the miner returns. During the unload the phase is left alone:
/// the Unload mission's own `In_Radio_Contact` gate (`0x0073DEE7`,
/// `abort_unload_contact_lost`) finds the contact gone on its next dispatch,
/// drops the unload latch and image and commences the queued order. Resetting
/// the phase here instead would leave the latch set, which blocks the queued
/// mission's readiness while Harvest re-docks the miner. A command that
/// assigns its mission directly never reaches that gate again and must also
/// call `abandon_unload_for_direct_retask` (`Command::HarvestCell` does).
///
/// Scope: only the refinery contact the miner FSM owns. Other contacts keep
/// their existing teardown owners (`DockTeardown`). Commands that write their
/// mission outside the MEGAMISSION funnel (Guard, EjectBunker,
/// UnloadPassengers, ToggleInfantryDeploy; see `mission::retask`) do not reach
/// this yet, so a Guard order on a docking miner still holds the slot until
/// the miner's next return.
pub(crate) fn break_for_retask(sim: &mut Simulation, miner_sid: u64) {
    let Some(refinery_sid) = sim
        .substrate
        .entities
        .get(miner_sid)
        .and_then(|entity| entity.miner.as_ref())
        .and_then(|miner| miner.reserved_refinery)
    else {
        return;
    };
    if !has_contact(sim, refinery_sid, miner_sid) && !has_entered(sim, refinery_sid, miner_sid) {
        return;
    }
    break_contact(sim, miner_sid, refinery_sid);
    if let Some(miner) = sim
        .substrate
        .entities
        .get_mut(miner_sid)
        .and_then(|entity| entity.miner.as_mut())
    {
        use crate::sim::miner::RefineryDockPhase as Phase;
        let unloading = matches!(
            miner.dock_phase,
            Phase::Pivoting | Phase::Unloading | Phase::DepositCooldown | Phase::Departing
        );
        if !unloading {
            // The handshake restarts from HELLO when the miner next returns.
            miner.dock_queued = false;
            miner.dock_phase = Phase::Approach;
            miner.dock_enter_retry.clear();
        }
    }
}

/// Whether the 0x18 ENTER_DOCK handshake linked this miner to the refinery.
pub(crate) fn has_entered(sim: &Simulation, refinery_sid: u64, miner_sid: u64) -> bool {
    sim.substrate
        .entities
        .get(miner_sid)
        .is_some_and(|miner| miner.dock_entered_with == Some(refinery_sid))
}

/// ENTER_DOCK (0x18) over the bus — sets the miner's `dock_entered_with`.
pub(crate) fn enter_dock(sim: &mut Simulation, miner_sid: u64, refinery_sid: u64) {
    let _ = radio::transmit(
        sim,
        miner_sid,
        refinery_sid,
        RadioMessage::EnterDock,
        RadioPayload::default(),
    );
}

/// BREAK over the bus — drops the contact on both ends and clears the miner's
/// `dock_entered_with`.
pub(crate) fn break_contact(sim: &mut Simulation, miner_sid: u64, refinery_sid: u64) {
    let _ = radio::transmit(
        sim,
        miner_sid,
        refinery_sid,
        RadioMessage::Break,
        RadioPayload::default(),
    );
}

/// Test fixtures drive the same bus the FSM does; there is no test-only store.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    /// HELLO with the stock single dock; `true` when the refinery admitted.
    pub(crate) fn dock_test_hello(sim: &mut Simulation, refinery_sid: u64, miner_sid: u64) -> bool {
        hello(sim, miner_sid, refinery_sid, 1) == ContactAdmission::Accepted
    }

    /// Whether any miner holds a contact slot of the refinery.
    pub(crate) fn dock_test_is_occupied(sim: &Simulation, refinery_sid: u64) -> bool {
        sim.substrate
            .entities
            .get(refinery_sid)
            .is_some_and(|refinery| !refinery.radio_contacts.is_empty())
    }
}
