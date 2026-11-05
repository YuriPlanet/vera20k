//! What the simulation needs of a headless client, whichever one runs: the
//! phase 1 model of the current policy or the adaptive production client.

use super::model_client::ModelLog;
use super::reader::StreamReader;
use crate::ClientLinkStats;

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
    fn log(&self) -> &ModelLog;
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
}
