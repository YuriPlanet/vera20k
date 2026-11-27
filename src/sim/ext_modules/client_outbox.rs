//! What the simulation hands the client modules (`ui.wasm`, RFC 0001
//! section 10): notifications simulation modules send (`send_notify`) and
//! the simulation events forwarded as the client event `SimEvent`.
//!
//! The outbox is presentation output, like sounds: it is not saved, not
//! hashed and not in the checksum, and nothing in the simulation reads it.
//! The client (`app::ui_modules`) drains it with
//! [`Simulation::take_client_outbox`] after each frame and filters it for
//! its player: a notification for the player's house (or, with `to` 0, for
//! observers), events whose `house` is the player's, `other` zeroed when the
//! player cannot see it. While catching up the client drains and drops it
//! (RFC 11: notifications are suspended during catch-up).
//!
//! `SimEvent` kinds (RFC 10, "Simulation events"): `Commanded`,
//! `OrderRefused`, `TakeDamage`, `Killed`, `Birth` and `AttackedBy`. An
//! event site builds a payload only when
//! [`TriggerRuntime::module_listens`](crate::sim::trigger_runtime::TriggerRuntime::module_listens)
//! says so; with forwarding on, that is also true for these kinds, so the
//! check stays one test at every site and costs nothing without modules.
//! Forwarding is on once any simulation module is installed or restored
//! (all machines then build the same payloads), and the client turns it on
//! with [`Simulation::set_client_event_forwarding`] when the match has a
//! `ui.wasm` but no `rules.wasm`. Building a payload changes no simulation
//! state: an event no trigger takes reaches only the outbox. So whether a
//! machine forwards never changes what it simulates.
//!
//! Nobody may drain the outbox (headless runs, a server): it keeps at most
//! [`MAX_KEPT`] of each list and drops the oldest past that.

use vera20k_wasm_ext::EventRecord;
use vera20k_wasm_ext::client::Source;
use vera20k_wasm_ext::triggers::{
    EVENT_ATTACKED_BY, EVENT_BIRTH, EVENT_COMMANDED, EVENT_KILLED, EVENT_ORDER_REFUSED,
    EVENT_TAKE_DAMAGE,
};

use super::{ModuleEvent, ModuleSource, house_ref};
use crate::rules::ruleset::RuleSet;
use crate::sim::world::Simulation;

/// The event kinds `SimEvent` forwards (RFC 0001 section 10).
pub(crate) const CLIENT_FORWARDED_EVENTS: [i32; 6] = [
    EVENT_COMMANDED,
    EVENT_ORDER_REFUSED,
    EVENT_TAKE_DAMAGE,
    EVENT_KILLED,
    EVENT_BIRTH,
    EVENT_ATTACKED_BY,
];

/// Entries each list keeps while nobody drains it.
pub const MAX_KEPT: usize = 4096;

/// One `send_notify` (RFC 0001 section 10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientNotify {
    /// House handle whose players receive it; 0 for the observers.
    pub to: i64,
    /// The sending module's source.
    pub source: Source,
    /// The notification ID (`vera.notify`).
    pub id: i32,
    pub data: Vec<u8>,
}

/// One forwarded simulation event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientSimEvent {
    /// The house whose object is the subject, or the house a house event is
    /// for (`OrderRefused` without an actor: the issuer).
    pub house: i64,
    /// The `EventRecord` bytes. `other` (offset 32) is still filled: the
    /// client zeroes it when its player cannot see that object. `trigger`
    /// and `holder` are 0.
    pub record: [u8; EventRecord::SIZE],
}

/// The frame output for client modules, oldest first.
#[derive(Debug, Default)]
pub struct ClientOutbox {
    pub notifies: Vec<ClientNotify>,
    pub sim_events: Vec<ClientSimEvent>,
}

impl ClientOutbox {
    pub fn is_empty(&self) -> bool {
        self.notifies.is_empty() && self.sim_events.is_empty()
    }

    pub(super) fn push_notify(&mut self, notify: ClientNotify) {
        push_bounded(&mut self.notifies, notify);
    }

    pub(super) fn push_sim_event(&mut self, event: ClientSimEvent) {
        push_bounded(&mut self.sim_events, event);
    }
}

/// Appends `item`; past [`MAX_KEPT`] plus a quarter the oldest go, down to
/// [`MAX_KEPT`], so an undrained list costs amortized O(1) per entry.
fn push_bounded<T>(list: &mut Vec<T>, item: T) {
    list.push(item);
    if list.len() > MAX_KEPT + MAX_KEPT / 4 {
        let excess = list.len() - MAX_KEPT;
        list.drain(..excess);
    }
}

impl From<ModuleSource> for Source {
    fn from(source: ModuleSource) -> Self {
        match source {
            ModuleSource::Mod => Self::Mod,
            ModuleSource::Map => Self::Map,
        }
    }
}

impl Simulation {
    /// Takes what the frames since the last call left for client modules
    /// (see the module documentation).
    pub fn take_client_outbox(&mut self) -> super::ClientOutbox {
        std::mem::take(&mut self.ext_modules.client_outbox)
    }

    /// Whether event sites build `SimEvent` payloads for client modules.
    /// On once a simulation module is installed or restored; the client
    /// turns it on for a match with only client modules. Never changes
    /// what the simulation computes.
    pub fn set_client_event_forwarding(&mut self, on: bool) {
        self.trigger_runtime.set_client_forwarding(on);
    }

    /// Puts `event` in the outbox when forwarding is on and its kind is
    /// forwarded: for the house of its subject, or for its own house when
    /// it has no subject. Event sites call it through
    /// [`Simulation::raise_module_event`] and the `OrderRefused` raise.
    pub(crate) fn forward_client_event(&mut self, rules: &RuleSet, event: &ModuleEvent) {
        if !self.trigger_runtime.forwards_to_clients(event.kind) {
            return;
        }
        let owner = match event.subject {
            0 => event.house,
            subject => self
                .entities()
                .get(subject)
                .and_then(|entity| self.houses.index_of(entity.owner()))
                .and_then(|index| u32::try_from(index).ok()),
        };
        let Some(owner) = owner else {
            return;
        };
        // The record as a trigger would queue it, with no trigger.
        let record = self.module_event_record(
            rules,
            ModuleSource::Mod,
            0,
            0,
            event,
            Some(event.house.unwrap_or(owner)),
        );
        self.ext_modules
            .client_outbox
            .push_sim_event(ClientSimEvent {
                house: house_ref(owner as usize),
                record: record.to_bytes(),
            });
    }
}
