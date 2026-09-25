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
        None,
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

/// `EventClass::Execute`'s MEGAMISSION arm, `0x004C72E8..0x004C7342`: a unit
/// that is not tethered (`+0x418` clear) sends OVER_OUT to `Contacts[0]`
/// (`PUSH 3; CALL [vt+0x274]`, `0x004C72F8`); a tethered one does so only when
/// that contact is a live `DockUnload=` building (`Type+0x16B3`,
/// `0x004C730F..0x004C7334`), and then also clears its `+0x418`
/// (`0x004C7342`). So a retasked harvester leaves its refinery handshake — or
/// its unload, whose contact gate then drops the latch and commences the new
/// order — while a unit still tethered to its war factory keeps that link.
///
/// Scope: only miners reach this in VERA (the funnel predates the other
/// units' native radio links).
pub(crate) fn break_for_retask(
    sim: &mut Simulation,
    miner_sid: u64,
    rules: Option<&crate::rules::ruleset::RuleSet>,
) {
    let Some(entity) = sim.substrate.entities.get(miner_sid) else {
        return;
    };
    if entity.miner.is_none() {
        return;
    }
    if entity.dock_entered_with.is_none() {
        radio::transmit_to_contact(sim, miner_sid, RadioMessage::Break, rules);
    } else {
        let dock_unload = entity
            .radio_contacts
            .slot(0)
            .and_then(|contact| sim.substrate.entities.get(contact))
            .filter(|contact| {
                contact.is_alive()
                    && contact.category == crate::map::entities::EntityCategory::Structure
            })
            .zip(rules)
            .and_then(|(contact, rules)| sim.object_type(contact.type_ref(), rules))
            .is_some_and(|object| object.dock_unload);
        if dock_unload {
            radio::transmit_to_contact(sim, miner_sid, RadioMessage::Break, rules);
            if let Some(entity) = sim.substrate.entities.get_mut(miner_sid) {
                entity.dock_entered_with = None;
            }
        }
    }
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
        None,
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
            RadioMessage::Tether,
            RadioPayload::default(),
            None,
        );
    }

    /// Whether any miner holds a contact slot of the refinery.
    pub(crate) fn dock_test_is_occupied(sim: &Simulation, refinery_sid: u64) -> bool {
        sim.substrate
            .entities
            .get(refinery_sid)
            .is_some_and(|refinery| !refinery.radio_contacts.is_empty())
    }
}
