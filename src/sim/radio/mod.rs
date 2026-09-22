//! Radio contact RPC vocabulary — the message/response opcodes and payload
//! exchanged over the synchronous contact bus.
//!
//! Defines the message/response vocabulary and the `Contacts` slot store; the
//! `transmit()` bus and the per-category `receive_radio()` handlers land in
//! later slices. Opcodes equal the original radio protocol's wire values so
//! dispatch stays a direct discriminant match. Pure enums + integer slots — no
//! float, no RNG. sim/ only — never render/ui/sidebar/audio/net.
use serde::{Deserialize, Serialize};

pub mod contacts;
pub mod receive;
pub use contacts::Contacts;
pub use receive::{
    REFINERY_ACCEPTED_DX, REFINERY_ACCEPTED_DY, receive_radio, refinery_accepted_cell,
};

use crate::map::entities::EntityCategory;
#[cfg(test)]
use crate::sim::world::LifecycleTestEvent;
use crate::sim::world::Simulation;

#[cfg(test)]
use std::cell::RefCell;

/// Ordered radio boundaries exposed only to crate tests.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RadioTestEvent {
    BroadcastSlotRead {
        sender_sid: u64,
        slot: usize,
        target_sid: Option<u64>,
    },
    SenderBreakCleared {
        sender_sid: u64,
        target_sid: u64,
    },
    ReceiverClassEffect {
        receiver_sid: u64,
        sender_sid: u64,
    },
    ReceiverCommonCleared {
        receiver_sid: u64,
        sender_sid: u64,
    },
}

#[cfg(test)]
thread_local! {
    static RADIO_TEST_TRACE: RefCell<Vec<RadioTestEvent>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn clear_test_trace() {
    RADIO_TEST_TRACE.with(|trace| trace.borrow_mut().clear());
}

#[cfg(test)]
pub(crate) fn take_test_trace() -> Vec<RadioTestEvent> {
    RADIO_TEST_TRACE.with(|trace| std::mem::take(&mut *trace.borrow_mut()))
}

#[cfg(test)]
fn record_test_event(event: RadioTestEvent) {
    RADIO_TEST_TRACE.with(|trace| trace.borrow_mut().push(event));
}

/// Send BREAK synchronously to every live contact before Techno Conceal.
///
/// Only the capacity is captured. Each sparse slot is re-read immediately
/// before dispatch so mutations made by an earlier receiver are visible to the
/// remaining ascending-slot walk, matching `Broadcast_Radio_ToAll @ 0x0065ACE0`.
/// No entity borrow is held across [`transmit`].
pub(crate) fn broadcast_break(sim: &mut Simulation, sender_sid: u64) {
    broadcast(sim, sender_sid, RadioMessage::Break);
}

/// Radio65ACE0, shared by landing24/takeoff25 and teardown3. Receivers may
/// mutate later sparse slots synchronously; never collect contacts up front.
pub(crate) fn broadcast(sim: &mut Simulation, sender_sid: u64, message: RadioMessage) {
    let capacity = sim
        .substrate
        .entities
        .get(sender_sid)
        .map_or(0, |sender| sender.radio_contacts.capacity());

    for slot in 0..capacity {
        let target_sid = sim
            .substrate
            .entities
            .get(sender_sid)
            .and_then(|sender| sender.radio_contacts.slot(slot));

        #[cfg(test)]
        record_test_event(RadioTestEvent::BroadcastSlotRead {
            sender_sid,
            slot,
            target_sid,
        });
        #[cfg(test)]
        if message == RadioMessage::Break {
            sim.trace_lifecycle_for_test(LifecycleTestEvent::BreakSlot { slot, target: target_sid });
        }

        if let Some(target_sid) = target_sid {
            transmit(
                sim,
                sender_sid,
                target_sid,
                message,
                RadioPayload::default(),
            );
        }
    }
}

/// Synchronous radio RPC (§5.2.1). Centralizes the HELLO/BREAK sender-side
/// contact bookkeeping (already-linked ⇒ ROGER without re-dispatch; on ROGER
/// the sender records the contact, self-evicting its own slot 0 when full); BREAK
/// nulls every sender slot to the target before forwarding. Every other opcode
/// dispatches straight to the receiver's [`receive_radio`]. The receiver only
/// ever sees an RTTI-filtered (Techno) sender.
pub fn transmit(
    sim: &mut Simulation,
    sender_sid: u64,
    target_sid: u64,
    msg: RadioMessage,
    payload: RadioPayload,
) -> RadioResponse {
    let filtered = filtered_techno_sender(sim, sender_sid);
    match msg {
        RadioMessage::Hello => transmit_hello(sim, sender_sid, target_sid, filtered),
        RadioMessage::Break => {
            transmit_break(sim, sender_sid, target_sid, filtered);
            RadioResponse::None
        }
        _ => receive_radio(sim, target_sid, filtered, msg, payload),
    }
}

/// RTTI sender filter (§5.2.2): the receiver only sees Unit/Aircraft/Building/
/// Infantry senders. Every `GameEntity` is a Techno, so this currently only
/// drops a vanished sender — kept explicit for the non-Techno cases a later
/// slice may introduce.
fn filtered_techno_sender(sim: &Simulation, sender_sid: u64) -> Option<u64> {
    match sim.substrate.entities.get(sender_sid)?.category {
        EntityCategory::Unit
        | EntityCategory::Infantry
        | EntityCategory::Structure
        | EntityCategory::Aircraft => Some(sender_sid),
    }
}

/// HELLO sender side (§5.2.4): already linked ⇒ ROGER without re-dispatch; else
/// dispatch to the receiver and, on ROGER, record the contact (slot-0 self-evict
/// when the sender's own array is full).
fn transmit_hello(
    sim: &mut Simulation,
    sender_sid: u64,
    target_sid: u64,
    filtered: Option<u64>,
) -> RadioResponse {
    if sim
        .substrate
        .entities
        .get(sender_sid)
        .is_some_and(|s| s.radio_contacts.contains(target_sid))
    {
        return RadioResponse::Roger;
    }
    let response = receive_radio(
        sim,
        target_sid,
        filtered,
        RadioMessage::Hello,
        RadioPayload::default(),
    );
    if response == RadioResponse::Roger {
        if let Some(sender) = sim.substrate.entities.get_mut(sender_sid) {
            // A non-building sender holds capacity 1. The miner FSM BREAKs its
            // previous refinery before HELLOing another (`begin_return`, the
            // MEGAMISSION retask, the redirect), so the self-evict below is not
            // reached by that handshake. It evicts the sender's slot only; the
            // evicted partner's own BREAK cascade is not modelled.
            let _ = sender.radio_contacts.insert_evicting(target_sid);
        }
    }
    response
}

/// BREAK sender side (§5.2.5): null EVERY sender slot matching the target, then
/// forward BREAK so the receiver runs its teardown.
fn transmit_break(sim: &mut Simulation, sender_sid: u64, target_sid: u64, filtered: Option<u64>) {
    if let Some(sender) = sim.substrate.entities.get_mut(sender_sid) {
        while sender.radio_contacts.remove(target_sid).is_some() {}
    }

    #[cfg(test)]
    record_test_event(RadioTestEvent::SenderBreakCleared {
        sender_sid,
        target_sid,
    });
    #[cfg(test)]
    sim.trace_lifecycle_for_test(LifecycleTestEvent::BreakSenderCleared { target: target_sid });

    receive_radio(
        sim,
        target_sid,
        filtered,
        RadioMessage::Break,
        RadioPayload::default(),
    );
}

/// A radio message sent from one entity to another. Discriminant = wire opcode.
///
/// Only codes that are sent in stock YR are modelled. Codes marked
/// `name inferred` are behaviour-named (not confirmed wire-string literals).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum RadioMessage {
    Hello = 0x02,
    Break = 0x03,
    DockingComplete = 0x07, // name inferred
    RequestClearance = 0x08,
    DockApproach = 0x0B, // name inferred
    DockArrived = 0x0C,  // name inferred
    AnimStop = 0x0D,
    CanDock = 0x0E,
    CanEnter = 0x0F,
    IsUnitLinked = 0x11, // name inferred
    MoveToCell = 0x12,
    NeedToMove = 0x13,
    DockNow = 0x15,            // name inferred
    TimingSync = 0x16,         // name inferred
    EnterDock = 0x18,          // name inferred
    LeaveDock = 0x19,          // name inferred
    SecondaryLockSet = 0x1A,   // name inferred
    SecondaryLockClear = 0x1B, // name inferred
    RepairTick = 0x1C,
    HelipadReserveAck = 0x1D, // name inferred
    DeploySetNav = 0x1E,      // name inferred
    LinkPassenger = 0x1F,
    IsRepairing = 0x22,
    IsOccupied = 0x23,
}
// Deliberately omitted: 0x10 RESERVE_DOCK (a mission-queue verb argument, not a
// wire message) and 0x24 WANT_RIDE (dormant in stock YR).

impl RadioMessage {
    /// The wire opcode byte.
    #[inline]
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// The reply returned by a `receive_radio` handler. Discriminant = wire opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum RadioResponse {
    None = 0,
    Roger = 1,
    Negatory = 0x0A,
    CellAccepted = 0x14,
    Queued = 0x17,
    InsufficientFunds = 0x20,
    RepairComplete = 0x21,
}

impl RadioResponse {
    /// The wire opcode byte.
    #[inline]
    pub fn code(self) -> u8 {
        self as u8
    }
}

/// Optional data carried alongside a radio message (e.g. the CAN_DOCK accepted
/// cell or the MOVE_TO_CELL goal).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RadioPayload {
    /// Target cell `(x, y)`, when the message carries one.
    pub cell: Option<(u16, u16)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_codes_match_wire_opcodes() {
        assert_eq!(RadioMessage::Hello.code(), 0x02);
        assert_eq!(RadioMessage::Break.code(), 0x03);
        assert_eq!(RadioMessage::CanDock.code(), 0x0E);
        assert_eq!(RadioMessage::DockNow.code(), 0x15);
        assert_eq!(RadioMessage::IsOccupied.code(), 0x23);
    }

    #[test]
    fn response_codes_match_wire_opcodes() {
        assert_eq!(RadioResponse::None.code(), 0);
        assert_eq!(RadioResponse::Roger.code(), 1);
        assert_eq!(RadioResponse::Negatory.code(), 0x0A);
        assert_eq!(RadioResponse::CellAccepted.code(), 0x14);
        assert_eq!(RadioResponse::Queued.code(), 0x17);
        assert_eq!(RadioResponse::InsufficientFunds.code(), 0x20);
        assert_eq!(RadioResponse::RepairComplete.code(), 0x21);
    }

    #[test]
    fn payload_defaults_to_no_cell() {
        assert_eq!(RadioPayload::default().cell, None);
    }
}
