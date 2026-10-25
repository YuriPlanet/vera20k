//! Unknown-source global channel used by LAN discovery, lobby and loading.
//!
//! `IPXGlobalConnClass` (`0x0053FA90` onward) shares send admission/retry with
//! private connections, but has its own frame grammar and receive policy.
//! IPv4 endpoints here are VERA's LAN transport adaptation: retail's Network
//! menu uses IPX sockets. No legacy IPX socket or address protocol is included.

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddrV4};

use super::reliable::{Delivery, RetryTiming, SendQueue, connection_prefix, sequence};

mod loading_observation;
pub use loading_observation::{LoadingObservation, LoadingObservationLog};

pub const GLOBAL_HEADER_LEN: usize = 16;
pub const GLOBAL_DISCRIMINATOR: u16 = 0x1236;

/// `0x00540B40 -> 0x0053FA90`: the global queue has160 send and320 receive
/// slots, with455-byte application messages and a separate product word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalConfig {
    pub discriminator: u16,
    pub product_id: u16,
    pub max_payload_bytes: usize,
    pub send_capacity: usize,
    pub receive_capacity: usize,
    pub acknowledgments: GlobalAcknowledgments,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalAcknowledgments {
    /// Original53FCF9 compares sequence only.
    Native,
    /// VERA online authenticates the sender and matches its destination.
    AuthenticatedSource,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            discriminator: GLOBAL_DISCRIMINATOR,
            product_id: 0xaa04,
            max_payload_bytes: 455,
            send_capacity: 160,
            receive_capacity: 320,
            acknowledgments: GlobalAcknowledgments::Native,
        }
    }
}

/// Native stores a12-byte destination and its independent port override on
/// each send entry. In particular, cancellation compares host and override,
/// not the port already present in the endpoint (`0x00540340/0x0053EF30`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalDestination {
    Broadcast {
        port_override: u16,
    },
    Direct {
        endpoint: SocketAddrV4,
        port_override: u16,
    },
}

impl GlobalDestination {
    pub const BROADCAST: Self = Self::Broadcast { port_override: 0 };

    pub const fn direct(endpoint: SocketAddrV4) -> Self {
        Self::Direct {
            endpoint,
            port_override: 0,
        }
    }

    fn host_and_override(self) -> (Ipv4Addr, u16) {
        match self {
            Self::Broadcast { port_override } => (Ipv4Addr::BROADCAST, port_override),
            Self::Direct {
                endpoint,
                port_override,
            } => (*endpoint.ip(), port_override),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum GlobalReceiveError {
    #[error("global frame is shorter than its16-byte header")]
    ShortHeader,
    #[error("global discriminator {actual:#06x} does not match {expected:#06x}")]
    WrongDiscriminator { expected: u16, actual: u16 },
    #[error("global acknowledgment does not match a sent message to its authenticated source")]
    UnmatchedAcknowledgment,
}

/// Observation from the existing send owner at ACK retirement, in wall-clock
/// buckets. This is consumed during startup; it never advances simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalResponseTime {
    pub destination: GlobalDestination,
    pub sequence: u32,
    pub elapsed: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalReceiveOutcome {
    Queued,
    DuplicateAcknowledged,
    Acknowledgment,
    /// Original53FCB0 returns success even when admission fails; no ACK is sent.
    DroppedFull,
    IgnoredKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlobalMessage {
    pub source: SocketAddrV4,
    pub product_id: u16,
    pub sequence: u32,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalMessageRef<'a> {
    pub source: SocketAddrV4,
    pub product_id: u16,
    pub sequence: u32,
    pub payload: &'a [u8],
}

#[derive(Debug)]
struct ReceivedEntry {
    frame: Vec<u8>,
    source: SocketAddrV4,
    arrived: u32,
    consumed: bool,
}

impl ReceivedEntry {
    fn message(&self) -> GlobalMessageRef<'_> {
        GlobalMessageRef {
            source: self.source,
            product_id: u16::from_le_bytes([self.frame[14], self.frame[15]]),
            sequence: sequence(&self.frame),
            payload: &self.frame[GLOBAL_HEADER_LEN..],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SeenMessage {
    source: SocketAddrV4,
    sequence: u32,
}

/// One aggregate send queue/response history, with unknown-source receive
/// admission. This object is carried from lobby through loading and gameplay.
#[derive(Debug)]
pub struct GlobalChannel {
    config: GlobalConfig,
    timing: RetryTiming,
    send: SendQueue<GlobalDestination>,
    receive: VecDeque<ReceivedEntry>,
    seen: Vec<SeenMessage>,
    next_seen: usize,
    total_received: u32,
    responses: VecDeque<GlobalResponseTime>,
    loading_observation: Option<LoadingObservationLog>,
}

impl GlobalChannel {
    pub fn new(config: GlobalConfig, timing: RetryTiming) -> Self {
        assert!(
            config.receive_capacity > 0,
            "global receive capacity must be positive"
        );
        assert!(
            config.max_payload_bytes <= usize::MAX - GLOBAL_HEADER_LEN,
            "global payload capacity must leave room for its header"
        );
        Self {
            config,
            timing,
            send: SendQueue::new(
                config.send_capacity,
                GLOBAL_HEADER_LEN + config.max_payload_bytes,
            ),
            receive: VecDeque::new(),
            // Original53ECB0 initializes the ring's addresses to broadcast,
            // while53FA90 initializes every sequence toFFFFFFFF.
            seen: vec![
                SeenMessage {
                    source: SocketAddrV4::new(Ipv4Addr::BROADCAST, u16::MAX),
                    sequence: u32::MAX,
                };
                config.receive_capacity
            ],
            next_seen: 0,
            total_received: 0,
            responses: VecDeque::new(),
            loading_observation: None,
        }
    }

    /// Manager540B40 destroys/recreates the channel. A base ConnectionClass
    /// reset alone would leave the extra global dedup arrays intact.
    pub fn reset(&mut self) {
        *self = Self::new(self.config, self.timing);
    }

    pub fn set_timing(&mut self, timing: RetryTiming) {
        self.timing = timing;
    }

    pub fn require_authenticated_acknowledgments(&mut self) {
        self.config.acknowledgments = GlobalAcknowledgments::AuthenticatedSource;
    }

    /// Read-only capture fact from the actual ACK matching policy.
    pub fn authenticated_acknowledgments(&self) -> bool {
        self.config.acknowledgments == GlobalAcknowledgments::AuthenticatedSource
    }

    pub fn available_send_slots(&self) -> usize {
        self.send.available_slots()
    }
    pub fn next_send_sequence(&self) -> u32 {
        self.send.total_enqueued()
    }
    pub fn pop_response_time(&mut self) -> Option<GlobalResponseTime> {
        self.responses.pop_front()
    }

    /// Explicit capture opt-in; observations never participate in admission,
    /// retry, ACK matching, row mutation or the simulation hash.
    pub fn observe_loading(&mut self) {
        self.loading_observation
            .get_or_insert_with(Default::default);
    }

    pub fn loading_observation(&self) -> Option<&LoadingObservationLog> {
        self.loading_observation.as_ref()
    }

    pub fn observe_loading_application(
        &mut self,
        source: usize,
        sequence: u32,
        percent: i32,
        displayed: f64,
        applied: bool,
    ) {
        if let Some(log) = self.loading_observation.as_mut() {
            log.application(source, sequence, percent, displayed, applied);
        }
    }

    pub const fn discriminator(&self) -> u16 {
        self.config.discriminator
    }

    pub fn mean_response_time(&self) -> u32 {
        self.send.mean_response_time()
    }

    /// Shared CommBuffer48BAA0 operation; unlike global reset540B40 this
    /// preserves queued messages, aggregate sequence and receive dedup state.
    pub fn reset_response_time(&mut self, clear: bool) {
        self.send.reset_response_time(clear);
    }

    pub fn queued_send_count(&self) -> usize {
        self.send.len()
    }

    pub fn queued_receive_count(&self) -> usize {
        self.receive.len()
    }

    /// Manager5410F0 and global53FBD0: broadcast is unacknowledged, even when
    /// reliability was requested. A nonzero override changes just the wire
    /// sequence; every successful enqueue still advances the aggregate count.
    pub fn send(
        &mut self,
        payload: &[u8],
        destination: GlobalDestination,
        delivery: Delivery,
        sequence_override: u32,
    ) -> bool {
        if payload.len() > self.config.max_payload_bytes
            || !self.send.can_enqueue(GLOBAL_HEADER_LEN + payload.len())
        {
            return false;
        }
        let kind = match destination {
            GlobalDestination::Broadcast { .. } => Delivery::Unreliable.wire_kind(),
            GlobalDestination::Direct { .. } => delivery.wire_kind(),
        };
        let outgoing_sequence = if sequence_override == 0 {
            self.send.total_enqueued()
        } else {
            sequence_override
        };
        let frame = self.frame(kind, outgoing_sequence, payload);
        let admitted = self.send.enqueue(frame, destination);
        if admitted && let Some(log) = self.loading_observation.as_mut() {
            log.admitted(payload, destination, outgoing_sequence, kind == 0);
        }
        admitted
    }

    /// Global53FCB0 accepts new sources. It compares the discriminator, not
    /// the product at+14: BB owns the product filter. Source normalization
    /// affects saved envelopes/dedup, while ACKs retain raw source+override.
    pub fn receive(
        &mut self,
        source: SocketAddrV4,
        source_port_override: u16,
        frame: &[u8],
        now_ticks: u32,
        send: &mut impl FnMut(GlobalDestination, &[u8]),
    ) -> Result<GlobalReceiveOutcome, GlobalReceiveError> {
        if frame.len() < GLOBAL_HEADER_LEN {
            return Err(GlobalReceiveError::ShortHeader);
        }
        let discriminator = u16::from_le_bytes([frame[0], frame[1]]);
        if discriminator != self.config.discriminator {
            return Err(GlobalReceiveError::WrongDiscriminator {
                expected: self.config.discriminator,
                actual: discriminator,
            });
        }
        let incoming_sequence = sequence(frame);
        let mut normalized_source = source;
        if source_port_override != 0 {
            normalized_source.set_port(source_port_override);
        }
        if frame[2] == 2 {
            // Original53FCF9..53FD2F deliberately does not compare source.
            // The online adaptation has authenticated source identities:
            // another admitted member cannot acknowledge this destination.
            let strict = self.config.acknowledgments == GlobalAcknowledgments::AuthenticatedSource;
            let matched = self
                .send
                .acknowledge(incoming_sequence, strict, |destination| {
                    if !strict {
                        return true;
                    }
                    match destination {
                        GlobalDestination::Direct {
                            mut endpoint,
                            port_override,
                        } => {
                            if port_override != 0 {
                                endpoint.set_port(port_override);
                            }
                            endpoint == normalized_source
                        }
                        GlobalDestination::Broadcast { .. } => false,
                    }
                });
            if strict && !matched {
                return Err(GlobalReceiveError::UnmatchedAcknowledgment);
            }
            return Ok(GlobalReceiveOutcome::Acknowledgment);
        }
        if frame[2] > 1 {
            return Ok(GlobalReceiveOutcome::IgnoredKind);
        }
        let duplicate = frame[2] == 0
            && self
                .seen
                .iter()
                .take(self.total_received as usize)
                .any(|seen| seen.source == normalized_source && seen.sequence == incoming_sequence);
        if !duplicate {
            // Unlike the private channel, global admission reserves no slot.
            if self.receive.len() == self.config.receive_capacity
                || frame.len() - GLOBAL_HEADER_LEN > self.config.max_payload_bytes
            {
                return Ok(GlobalReceiveOutcome::DroppedFull);
            }
            self.receive.push_back(ReceivedEntry {
                frame: frame.to_vec(),
                source: normalized_source,
                arrived: now_ticks,
                consumed: false,
            });
            self.total_received = self.total_received.wrapping_add(1);
            if frame[2] == 0 {
                self.seen[self.next_seen] = SeenMessage {
                    source: normalized_source,
                    sequence: incoming_sequence,
                };
                self.next_seen = (self.next_seen + 1) % self.seen.len();
            }
        }
        if frame[2] == 0 {
            let ack = self.frame(2, incoming_sequence, &[]);
            send(
                GlobalDestination::Direct {
                    endpoint: source,
                    port_override: source_port_override,
                },
                &ack,
            );
        }
        Ok(if duplicate {
            GlobalReceiveOutcome::DuplicateAcknowledged
        } else {
            GlobalReceiveOutcome::Queued
        })
    }

    /// Shared48C3B0/48C3E0 orchestration. Manager541820 calls
    /// `discard_failed_sends` after failure; receive retirement is skipped.
    pub fn service(
        &mut self,
        now_ticks: u32,
        send: &mut impl FnMut(GlobalDestination, &[u8]),
    ) -> bool {
        let observe = self.config.acknowledgments == GlobalAcknowledgments::AuthenticatedSource;
        let responses = &mut self.responses;
        let capacity = self.config.send_capacity;
        let loading = &mut self.loading_observation;
        if !self.send.service(
            now_ticks,
            self.timing,
            send,
            &mut |destination, sequence, elapsed| {
                if let Some(log) = loading.as_mut() {
                    log.retired(destination, sequence);
                }
                if observe {
                    // Derived observations are bounded by the send owner's cap.
                    // Startup consumes them every service pass; no history is
                    // retained after the corresponding latency probe completes.
                    if responses.len() == capacity {
                        responses.pop_front();
                    }
                    responses.push_back(GlobalResponseTime {
                        destination,
                        sequence,
                        elapsed,
                    });
                }
            },
        ) {
            return false;
        }
        self.retire_consumed();
        true
    }

    pub fn discard_failed_sends(&mut self) -> usize {
        self.send.discard_failed()
    }

    /// Global53FF10 only visits logical index0. A consumed first entry blocks
    /// another pop until ordinary service performs5400D0 retirement.
    pub fn pop_received(&mut self) -> Option<GlobalMessage> {
        let entry = self.receive.front_mut()?;
        if entry.consumed {
            return None;
        }
        entry.consumed = true;
        let message = entry.message();
        Some(GlobalMessage {
            source: message.source,
            product_id: message.product_id,
            sequence: message.sequence,
            payload: message.payload.to_vec(),
        })
    }

    /// Global53FFA0 inspects an unconsumed logical entry without retiring it.
    pub fn peek_received(&self, index: usize) -> Option<GlobalMessageRef<'_>> {
        self.receive
            .get(index)
            .filter(|entry| !entry.consumed)
            .map(ReceivedEntry::message)
    }

    /// Global540030 marks an existing logical entry consumed.
    pub fn consume_received(&mut self, index: usize) -> bool {
        let Some(entry) = self.receive.get_mut(index) else {
            return false;
        };
        entry.consumed = true;
        true
    }

    /// Global5400D0 retires consumed entries without touching dedup history.
    pub fn retire_consumed(&mut self) -> usize {
        let before = self.receive.len();
        self.receive.retain(|entry| !entry.consumed);
        before - self.receive.len()
    }

    /// Global540110 uses a signed strict comparison after wrapping addition.
    /// It invalidates the first equal sequence in the ring, irrespective of
    /// endpoint, even when a different source's ring slot is the first match.
    pub fn strip_aged(&mut self, now_ticks: u32, maximum_age: u32) -> usize {
        let before = self.receive.len();
        self.receive.retain(|entry| {
            if (entry.arrived.wrapping_add(maximum_age) as i32) < now_ticks as i32 {
                if let Some(seen) = self
                    .seen
                    .iter_mut()
                    .find(|seen| seen.sequence == sequence(&entry.frame))
                {
                    seen.sequence = u32::MAX;
                }
                false
            } else {
                true
            }
        });
        before - self.receive.len()
    }

    /// Global540340 compares host bytes plus the separately stored override.
    pub fn cancel_sends_to(&mut self, host: Ipv4Addr, port_override: u16) -> usize {
        self.send
            .cancel(|destination| destination.host_and_override() == (host, port_override))
    }

    /// VERA direct-LAN lifecycle cancellation includes the UDP port. Several
    /// admitted clients may share one host; native540340's host-only match
    /// remains available for its original consumers above.
    pub fn cancel_sends_to_endpoint(&mut self, endpoint: SocketAddrV4) -> usize {
        self.send.cancel(|destination| match destination {
            GlobalDestination::Direct {
                endpoint: mut sent,
                port_override,
            } => {
                if port_override != 0 {
                    sent.set_port(port_override);
                }
                sent == endpoint
            }
            GlobalDestination::Broadcast { .. } => false,
        })
    }

    fn frame(&self, kind: u8, sequence: u32, payload: &[u8]) -> Vec<u8> {
        let mut frame =
            connection_prefix(self.config.discriminator, kind, sequence, 2 + payload.len());
        frame[14..16].copy_from_slice(&self.config.product_id.to_le_bytes());
        frame[GLOBAL_HEADER_LEN..].copy_from_slice(payload);
        frame
    }
}

#[cfg(test)]
#[path = "global_tests.rs"]
mod tests;
