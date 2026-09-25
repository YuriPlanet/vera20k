//! Radio contact RPC — the message/response opcodes, the `Contacts` slot store
//! and the synchronous transmit bus (`RadioClass::Transmit_Message @
//! 0x0065A970`). Every receiver runs inline inside the sender's transmit, so
//! nested replies finish before the outer transmit returns; the per-class
//! receive chain lives in [`receive`]. Opcodes equal the original radio
//! protocol's wire values so dispatch stays a direct discriminant match. Pure
//! enums + integer slots — no float, no RNG. Native evidence:
//! tools/spatial_oracle/refinery_dock.json (`radio` and `can_dock` rows).
//! sim/ only — never render/ui/sidebar/audio/net.
use serde::{Deserialize, Serialize};

pub mod contacts;
pub mod receive;
pub use contacts::Contacts;
pub use receive::receive_radio;

use crate::map::entities::EntityCategory;
use crate::rules::ruleset::RuleSet;
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

/// One transmit as the native oracle records it: sender, message, receiver
/// and the reply, logged in entry order (a nested transmit follows its outer
/// one) with the reply filled in when the transmit returns.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransmitRecord {
    pub(crate) sender_sid: u64,
    pub(crate) msg: u8,
    pub(crate) target_sid: u64,
    pub(crate) reply: Option<u8>,
}

#[cfg(test)]
thread_local! {
    static TRANSMIT_LOG: RefCell<Vec<TransmitRecord>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn take_transmit_log() -> Vec<TransmitRecord> {
    TRANSMIT_LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
}

/// Send BREAK synchronously to every live contact before Techno Conceal.
///
/// Only the capacity is captured. Each sparse slot is re-read immediately
/// before dispatch so mutations made by an earlier receiver are visible to the
/// remaining ascending-slot walk, matching `Broadcast_Radio_ToAll @ 0x0065ACE0`.
/// No entity borrow is held across [`transmit`].
pub(crate) fn broadcast_break(sim: &mut Simulation, sender_sid: u64, rules: Option<&RuleSet>) {
    broadcast(sim, sender_sid, RadioMessage::Break, rules);
}

/// Radio65ACE0, shared by landing24/takeoff25 and teardown3. Receivers may
/// mutate later sparse slots synchronously; never collect contacts up front.
pub(crate) fn broadcast(
    sim: &mut Simulation,
    sender_sid: u64,
    message: RadioMessage,
    rules: Option<&RuleSet>,
) {
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
                rules,
            );
        }
    }
}

/// `RadioClass::Transmit_Message @ 0x0065A970` (vtable +0x27C): the sender-side
/// HELLO and OVER_OUT bookkeeping, then the receiver's class chain
/// ([`receive_radio`]). Every other opcode returns the receiver's answer
/// unchanged. The receiver only ever sees an RTTI-filtered (Techno) sender
/// (`As_Techno @ 0x0040DD70`). `rules` reaches the type-aware receivers; a
/// caller without it gets their rules-free answers.
pub fn transmit(
    sim: &mut Simulation,
    sender_sid: u64,
    target_sid: u64,
    msg: RadioMessage,
    payload: RadioPayload,
    rules: Option<&RuleSet>,
) -> RadioResponse {
    #[cfg(test)]
    let log_index = TRANSMIT_LOG.with(|log| {
        let mut log = log.borrow_mut();
        log.push(TransmitRecord {
            sender_sid,
            msg: msg.code(),
            target_sid,
            reply: None,
        });
        log.len() - 1
    });
    let filtered = filtered_techno_sender(sim, sender_sid);
    let reply = match msg {
        RadioMessage::Hello => transmit_hello(sim, sender_sid, target_sid, filtered, rules),
        RadioMessage::Break => transmit_over_out(sim, sender_sid, target_sid, filtered, rules),
        _ => receive_radio(sim, target_sid, filtered, msg, payload, rules),
    };
    #[cfg(test)]
    TRANSMIT_LOG.with(|log| {
        if let Some(record) = log.borrow_mut().get_mut(log_index) {
            record.reply = Some(reply.code());
        }
    });
    reply
}

/// `RadioClass @ 0x0065ACB0` (vtable +0x274): transmit to `Contacts[0]`, or
/// answer 0 without dispatching when that slot is null.
pub(crate) fn transmit_to_contact(
    sim: &mut Simulation,
    sender_sid: u64,
    msg: RadioMessage,
    rules: Option<&RuleSet>,
) -> RadioResponse {
    let Some(target_sid) = sim
        .substrate
        .entities
        .get(sender_sid)
        .and_then(|sender| sender.radio_contacts.slot(0))
    else {
        return RadioResponse::None;
    };
    transmit(
        sim,
        sender_sid,
        target_sid,
        msg,
        RadioPayload::default(),
        rules,
    )
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

/// HELLO sender side, `0x0065A9ED..0x0065AA72`: a target already in a slot
/// answers ROGER without a dispatch. Otherwise the first null slot is chosen;
/// with none, the sender first transmits OVER_OUT to `Contacts[0]` and reuses
/// slot 0 (the old link breaks even when this HELLO then fails). A receiver
/// ROGER stores the target in that slot; any other answer returns NEGATORY.
fn transmit_hello(
    sim: &mut Simulation,
    sender_sid: u64,
    target_sid: u64,
    filtered: Option<u64>,
    rules: Option<&RuleSet>,
) -> RadioResponse {
    let Some(sender) = sim.substrate.entities.get(sender_sid) else {
        return RadioResponse::Negatory;
    };
    if sender.radio_contacts.contains(target_sid) {
        return RadioResponse::Roger;
    }
    let slot = match sender.radio_contacts.first_free() {
        Some(slot) => slot,
        None => {
            if let Some(evicted) = sender.radio_contacts.slot(0) {
                transmit(
                    sim,
                    sender_sid,
                    evicted,
                    RadioMessage::Break,
                    RadioPayload::default(),
                    rules,
                );
            }
            0
        }
    };
    let response = receive_radio(
        sim,
        target_sid,
        filtered,
        RadioMessage::Hello,
        RadioPayload::default(),
        rules,
    );
    if response != RadioResponse::Roger {
        return RadioResponse::Negatory;
    }
    if let Some(sender) = sim.substrate.entities.get_mut(sender_sid) {
        sender.radio_contacts.set_slot(slot, target_sid);
    }
    RadioResponse::Roger
}

/// OVER_OUT sender side, `0x0065A99C..0x0065A9DB`: null EVERY sender slot
/// holding the target, then return the receiver's answer to its teardown.
fn transmit_over_out(
    sim: &mut Simulation,
    sender_sid: u64,
    target_sid: u64,
    filtered: Option<u64>,
    rules: Option<&RuleSet>,
) -> RadioResponse {
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
        rules,
    )
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
    /// MOVE_HERE: the payload cell is where the receiver should be; a Foot
    /// already in it answers [`RadioResponse::AlreadyThere`] (`0x004D9139`).
    MoveToCell = 0x12,
    /// "Do you need to move?": a Foot answers ROGER with no NavCom or a
    /// stopped locomotor, else NEGATORY (`0x004D90E8`).
    NeedToMove = 0x13,
    DockNow = 0x15, // name inferred
    /// Sent by a dock after the tether: a Unit turns to face 0x4000, then
    /// answers with [`RadioMessage::DockNow`] (`0x007376AD`).
    PrepareToDock = 0x16, // name inferred
    /// Techno+0x418 tether: the receiver sets its flag and sends the message
    /// back (`0x006F4B1F`), so both ends end up tethered.
    Tether = 0x18, // name inferred
    /// Clears the tether on both ends (`0x006F4B8D`).
    Untether = 0x19, // name inferred
    SecondaryLockSet = 0x1A, // name inferred
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
    /// A Foot's answer to MOVE_HERE when it already stands in the cell.
    AlreadyThere = 0x14, // name inferred
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
        assert_eq!(RadioResponse::AlreadyThere.code(), 0x14);
        assert_eq!(RadioResponse::Queued.code(), 0x17);
        assert_eq!(RadioResponse::InsufficientFunds.code(), 0x20);
        assert_eq!(RadioResponse::RepairComplete.code(), 0x21);
    }

    #[test]
    fn payload_defaults_to_no_cell() {
        assert_eq!(RadioPayload::default().cell, None);
    }
}
