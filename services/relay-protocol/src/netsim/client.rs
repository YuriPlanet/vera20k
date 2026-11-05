//! What the simulation needs of a headless client.
//!
//! The phase 1 model of the old production policy (`model_client.rs`) was
//! removed when the relay dropped requests 1, 4, 6 and 7 (spec §1.2); its
//! baseline is the phase 1 run of commit 720f12400 on the same scenario
//! files and seeds, which is deterministic.

use std::collections::BTreeMap;

use super::reader::{GapTriggers, StreamReader};
use crate::ClientLinkStats;

/// What a client did, timestamped in virtual µs for the measurement window.
#[derive(Debug, Default)]
pub struct ClientLog {
    /// (issue time, execution time) of each command, both in µs.
    pub commands: Vec<(u64, u64)>,
    /// Times frame ends advanced the executed frame.
    pub advances: Vec<u64>,
    pub errors: BTreeMap<String, u64>,
    /// Datagrams `ClientSession::receive_at` refused (duplicates, replays).
    pub refused: BTreeMap<String, u64>,
    /// Spec §6.3 row 2 triggers this stream makes.
    pub gap_triggers: GapTriggers,
    /// Fallback PULLs `poll_repair_pull` sent (spec §7.7).
    pub repair_pulls: Vec<u64>,
    /// (time µs, batch id) of each batch sealed.
    pub seals: Vec<(u64, u64)>,
    /// (batch id, plaintext) of each batch sealed.
    pub plaintexts: Vec<(u64, Vec<u8>)>,
}

/// A datagram a client sends.
#[derive(Debug)]
pub struct Outgoing {
    pub bytes: Vec<u8>,
    /// Which hop of this client sent it: 0, then one more per renewal.
    pub hop: usize,
    /// The client's service that produced it; none for the transport's
    /// immediate replies (a CONFIRM to a CHALLENGE).
    pub service: Option<u64>,
    /// Sent by `heartbeat_uplink` (spec §6.6), not `poll_uplink`.
    pub heartbeat: bool,
}

pub trait HeadlessClient {
    /// A datagram arrived; `now_ms` is the client's clock at the socket read.
    fn deliver(&mut self, bytes: &[u8], now_ms: u64, at_us: u64);
    /// One service of the game loop.
    fn tick(&mut self, now_ms: u64, at_us: u64);
    fn take_output(&mut self) -> Vec<Outgoing>;
    fn started(&self) -> bool;
    fn reader(&self) -> &StreamReader;
    /// A player command issued at `at_us`, batched at the next service.
    fn queue_command(&mut self, at_us: u64);
    fn log(&self) -> &ClientLog;
    /// Client-clock milliseconds at which event-mode service should run.
    fn wakeups(&self) -> Vec<u64> {
        Vec::new()
    }
    fn leave(&mut self) {}
    /// Moves to a new hop of this ticket (spec §4.7).
    fn renew(&mut self, _ticket: &[u8], _packet_key: [u8; 32], _now_ms: u64) {}
    fn set_restoring(&mut self, _restoring: bool) {}
    fn link_stats(&self) -> Option<ClientLinkStats> {
        None
    }
    /// Relay datagrams whose tag failed (compaction spec §6.4 item 6).
    fn mac_failures(&self) -> u64 {
        0
    }
}
