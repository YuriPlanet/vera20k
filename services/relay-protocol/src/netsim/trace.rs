//! Everything a judgment may look at, recorded when a run is traced: every
//! datagram with its fate, and the production observation interfaces
//! (`link_stats`, `reader_link_stats`) whenever they change.

use std::net::SocketAddrV4;

use super::inspect::{Datagram, Detail, PackRecord, Uplink};
use super::reader::Record;
use super::scenario::Direction;
use crate::{ClientLinkStats, ReaderLinkStats};

/// Where a relay datagram came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Client,
    /// `process_at`'s reply to a client datagram.
    Reply,
    /// `drain_ordering_push`.
    Push,
}

#[derive(Debug, Clone)]
pub struct WireRecord {
    pub client: usize,
    pub direction: Direction,
    /// The client's hop: 0, then one more per renewal.
    pub hop: usize,
    /// The client service that sent it (uplink only).
    pub service: Option<u64>,
    /// Sent by `heartbeat_uplink` (spec §6.6).
    pub heartbeat: bool,
    pub sent_us: u64,
    /// Arrival of each copy; empty when the network lost it.
    pub arrivals_us: Vec<u64>,
    pub origin: Origin,
    /// Whole datagram bytes, envelope and MAC, no IP/UDP header.
    pub len: usize,
    /// The datagram itself when the scenario keeps wire bytes.
    pub bytes: Option<Vec<u8>>,
    pub parsed: Result<Datagram, String>,
    /// Uplink: `process_at`'s outcome for the first copy, `Ok` or the error.
    pub relay_result: Option<Result<(), String>>,
}

impl WireRecord {
    pub fn lost(&self) -> bool {
        self.arrivals_us.is_empty()
    }
    pub fn arrived_us(&self) -> Option<u64> {
        self.arrivals_us.iter().min().copied()
    }
    pub fn kind(&self) -> &'static str {
        self.parsed.as_ref().map_or("MALFORMED", |d| d.kind)
    }
    pub fn hop_seq(&self) -> u64 {
        self.parsed.as_ref().map_or(0, |d| d.hop_seq)
    }
    pub fn payload_len(&self) -> usize {
        self.parsed.as_ref().map_or(0, |d| d.payload_len)
    }
    pub fn uplink(&self) -> Option<&Uplink> {
        match &self.parsed.as_ref().ok()?.detail {
            Detail::Uplink(uplink) => Some(uplink),
            _ => None,
        }
    }
    pub fn pack(&self) -> Option<(u64, &[PackRecord])> {
        match &self.parsed.as_ref().ok()?.detail {
            Detail::Pack { base, records, .. } => Some((*base, records)),
            _ => None,
        }
    }
    pub fn sent_ms(&self) -> f64 {
        self.sent_us as f64 / 1000.0
    }
}

#[derive(Debug, Default)]
pub struct Trace {
    pub wire: Vec<WireRecord>,
    /// (time µs, client, snapshot) after each service that changed it.
    pub client_stats: Vec<(u64, usize, ClientLinkStats)>,
    /// (time µs, client, snapshot) after each relay pass that changed it.
    pub reader_stats: Vec<(u64, usize, ReaderLinkStats)>,
    /// (time µs, client) of every service.
    pub services: Vec<(u64, usize)>,
    /// Per client: when each command was issued, in µs.
    pub issued: Vec<Vec<u64>>,
    /// Per client, at the end: (time µs, batch id) of each sealed batch.
    pub seals: Vec<Vec<(u64, u64)>>,
    /// Per client, at the end: (issued µs, executed µs) of each command run.
    pub executed: Vec<Vec<(u64, u64)>>,
    /// Per client, at the end: every record read, in order.
    pub streams: Vec<Vec<Record>>,
    /// Per client, at the end: (batch id, plaintext) of each sealed batch.
    pub plaintexts: Vec<Vec<(u64, Vec<u8>)>>,
    /// Per client, at the end: each hop's packet key.
    pub hop_keys: Vec<Vec<[u8; 32]>>,
    /// Per client: its address, then one more per rebind.
    pub endpoints: Vec<Vec<SocketAddrV4>>,
    /// Relay datagrams to an address no client holds: (time µs, address).
    pub misrouted: Vec<(u64, SocketAddrV4)>,
    /// Every forged datagram and the relay's answer to it.
    pub forged: Vec<Forged>,
    /// Every replayed datagram (`forgery` is the client) and the answer.
    pub replayed: Vec<Forged>,
    /// The relay's canonical records in sequence order, when an interface
    /// reads them (`Simulation::relay_history`).
    pub relay_history: Option<Vec<Vec<u8>>>,
    /// The relay's tag failures over the run (compaction spec §6.4).
    pub mac_counts: Option<crate::MacFailureCounts>,
    /// Per client: tags that failed at the client (§6.4 item 6).
    pub client_mac_failures: Vec<u64>,
}

/// One datagram of a scenario forgery (compaction spec §6.4).
#[derive(Debug, Clone)]
pub struct Forged {
    pub forgery: usize,
    pub at_us: u64,
    pub endpoint: SocketAddrV4,
    pub result: Result<(), String>,
    /// Datagrams the relay sent in reply.
    pub replies: usize,
}

impl Trace {
    pub fn of(&self, client: usize, direction: Direction) -> impl Iterator<Item = &WireRecord> {
        self.wire
            .iter()
            .filter(move |record| record.client == client && record.direction == direction)
    }

    /// The client's link snapshot as it stood at `at_us`.
    pub fn client_stats_at(&self, client: usize, at_us: u64) -> Option<&ClientLinkStats> {
        self.client_stats
            .iter()
            .filter(|(at, c, _)| *c == client && *at <= at_us)
            .last()
            .map(|(_, _, stats)| stats)
    }

    /// The reader's relay-side snapshot as it stood at `at_us`.
    pub fn reader_stats_at(&self, client: usize, at_us: u64) -> Option<&ReaderLinkStats> {
        self.reader_stats
            .iter()
            .filter(|(at, c, _)| *c == client && *at <= at_us)
            .last()
            .map(|(_, _, stats)| stats)
    }
}
