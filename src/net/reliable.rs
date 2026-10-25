//! Shared byte delivery for direct protocol-2 LAN connections.
//!
//! The active UDP interface still uses ConnectionClass (`0x0048BF40` through
//! `0x0048C5A0`) and IPXManagerClass's private peer ordering (`0x005414C0`,
//! `0x005415F0`, `0x00541820`). This owner handles their connection frames and
//! queues; the socket owner supplies endpoints, time and datagram I/O. It never
//! stores or interprets simulation commands.

use std::collections::VecDeque;
use std::net::SocketAddrV4;

use crate::sim::multiplayer_checksum::RetailChecksumAccumulator;

pub const CONNECTION_HEADER_LEN: usize = 14;

/// Constructor inputs from `0x004E80A0 -> 0x005408F0 -> 0x0053F430`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanConnectionConfig {
    pub product_id: u16,
    pub max_payload_bytes: usize,
    pub send_capacity: usize,
    pub receive_capacity: usize,
    pub max_peers: usize,
}

impl Default for LanConnectionConfig {
    fn default() -> Self {
        Self {
            product_id: 0xaa04,
            max_payload_bytes: 558,
            send_capacity: 32,
            receive_capacity: 32,
            max_peers: 7,
        }
    }
}

/// Time is the caller's wrapping count of 16 ms wall-clock buckets, not frames.
/// `u32::MAX` disables the corresponding retry-count or elapsed-time limit.
/// Timing values are selected by the active session caller of `0x00540C60`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryTiming {
    pub retry_delta: u32,
    pub max_retries: u32,
    pub timeout: u32,
}

impl RetryTiming {
    /// Guest GO completion5DD9FD overrides only the primary global channel
    /// through IPXManager540D10; the private/default timing stays independent.
    pub const LAN_GUEST_COMPLETED_GLOBAL: Self = Self {
        retry_delta: 60,
        max_retries: u32::MAX,
        timeout: 600,
    };

    /// MainPrepareSession52E9DF..52EA4F, after CreateConnections697B70 in
    /// LAN mode. Returns private/default timing followed by primary-global
    /// timing. Each unsigned threshold branch re-reads5424C0 on equality;
    /// the replacement observation is neither saturated nor floored again.
    pub fn lan_gameplay(mut global_response: impl FnMut() -> u32) -> (Self, Self) {
        let mut select = |floor| {
            let retry_delta = if global_response().wrapping_add(2) >= floor {
                global_response().wrapping_add(2)
            } else {
                floor
            };
            Self {
                retry_delta,
                max_retries: u32::MAX,
                timeout: 600,
            }
        };
        (select(15), select(60))
    }

    /// LAN GO timing from original5DD541/5DD819 (host) and5DD8A0/5DD9AC
    /// (guest). ResponseTime5424C0 is re-read only on the native branches;
    /// a second observation replaces the first without another clamp.
    pub fn lan_start(host: bool, completed: bool, mut response_time: impl FnMut() -> u32) -> Self {
        let (retry_delta, timeout) = if !host && completed {
            //5DD9B6/5DD9D5 use signed comparisons, unlike the other blocks.
            let retry = (response_time() as i32).max(20) as u32;
            let timeout = (retry.wrapping_shl(3) as i32).max(120) as u32;
            (retry, timeout)
        } else {
            let timeout = if completed {
                if response_time().wrapping_shl(3) >= 120 {
                    response_time().wrapping_shl(3)
                } else {
                    120
                }
            } else if host {
                600
            } else {
                1000
            };
            let first = response_time().wrapping_add(2);
            //Host pre-GO JNC includes equality; post-GO and guest JBE do not.
            let retry = if first > 30 || (host && !completed && first == 30) {
                response_time().wrapping_add(2)
            } else {
                30
            };
            (retry, timeout)
        };
        Self {
            retry_delta,
            max_retries: u32::MAX,
            timeout,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Reliable,
    Unreliable,
}

impl Delivery {
    pub(super) fn wire_kind(self) -> u8 {
        match self {
            Self::Reliable => 0,
            Self::Unreliable => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PeerAdmissionError {
    #[error("the LAN private peer table is full")]
    Full,
    #[error("LAN peer identity {0} is already registered or reserved")]
    DuplicateId(i32),
    #[error("LAN endpoint {0} is already registered")]
    DuplicateEndpoint(SocketAddrV4),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReceiveError {
    #[error("LAN datagram source {0} has no admitted connection")]
    UnknownPeer(SocketAddrV4),
    #[error("connection frame is shorter than its 14-byte header")]
    ShortHeader,
    #[error("connection product {actual:#06x} does not match {expected:#06x}")]
    WrongProduct { expected: u16, actual: u16 },
    #[error("unknown connection packet kind {0}")]
    UnknownKind(u8),
    #[error("connection payload exceeds the configured private packet limit")]
    PayloadTooLong,
    #[error("the peer receive queue has no admissible slot")]
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveOutcome {
    Queued,
    /// Both a duplicate and a newly accepted reliable packet emit an ACK.
    DuplicateAcknowledged,
    Acknowledgment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedMessage {
    pub peer_id: i32,
    pub payload: Vec<u8>,
    pub delivery: Delivery,
}

/// Private queues, sequence counters and delivery cursor have one owner.
#[derive(Debug)]
pub struct LanConnections {
    config: LanConnectionConfig,
    timing: RetryTiming,
    peers: Vec<Peer>,
    next_receive_peer: usize,
}

#[derive(Debug)]
struct Peer {
    id: i32,
    endpoint: SocketAddrV4,
    send: SendQueue<()>,
    receive: VecDeque<ReceiveEntry>,
    next_reliable: u32,
    next_unreliable: u32,
    contiguous_received: u32,
    delivered_reliable: u32,
}

#[derive(Debug)]
struct SendEntry<D> {
    frame: Vec<u8>,
    destination: D,
    completed: bool,
    failed: bool,
    first_sent: u32,
    last_sent: u32,
    attempts: u32,
}

/// One port of CommBuffer send admission and ConnectionClass send service,
/// shared by private peers and the global unknown-source channel. Only this
/// module can mutate entries, timing observations or successful-send count.
#[derive(Debug)]
pub(super) struct SendQueue<D> {
    entries: VecDeque<SendEntry<D>>,
    capacity: usize,
    max_frame_bytes: usize,
    total_enqueued: u32,
    response_time: ResponseTime,
}

#[derive(Debug)]
struct ReceiveEntry {
    frame: Vec<u8>,
    consumed: bool,
}

/// CommBuffer response-time state, `0x0048BA10`; consumed by `0x0048BA80`.
#[derive(Debug, Default)]
struct ResponseTime {
    total: u32,
    samples: u32,
    mean: u32,
}

impl ResponseTime {
    /// CommBuffer::ResetResponseTime48BAA0. This changes response history
    /// only; an in-flight ACK still samples its original first-send time.
    fn reset(&mut self, clear: bool) {
        self.total = if clear { 0 } else { self.mean };
        self.samples = u32::from(!clear);
        if clear {
            self.mean = 0;
        }
    }

    fn record(&mut self, elapsed: u32) {
        if self.samples == 256 {
            self.total = self.total.wrapping_add(elapsed.wrapping_sub(self.mean));
            self.mean = (self.total >> 8).wrapping_add(u32::from(self.total & 255 > 127));
        } else {
            self.samples += 1;
            self.total = self.total.wrapping_add(elapsed);
            self.mean = self.total / self.samples;
        }
    }
}

impl<D: Copy> SendQueue<D> {
    pub(super) fn new(capacity: usize, max_frame_bytes: usize) -> Self {
        assert!(capacity > 0, "send queue capacity must be positive");
        Self {
            entries: VecDeque::new(),
            capacity,
            max_frame_bytes,
            total_enqueued: 0,
            response_time: ResponseTime::default(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(super) fn can_enqueue(&self, frame_bytes: usize) -> bool {
        self.entries.len() < self.capacity && frame_bytes <= self.max_frame_bytes
    }

    pub(super) fn available_slots(&self) -> usize {
        self.capacity.saturating_sub(self.entries.len())
    }

    /// `0x0048B410`: total-send advances on successful admission, not emission.
    pub(super) fn enqueue(&mut self, frame: Vec<u8>, destination: D) -> bool {
        if !self.can_enqueue(frame.len()) {
            return false;
        }
        self.entries.push_back(SendEntry {
            frame,
            destination,
            completed: false,
            failed: false,
            first_sent: 0,
            last_sent: 0,
            attempts: 0,
        });
        self.total_enqueued = self.total_enqueued.wrapping_add(1);
        true
    }

    pub(super) fn total_enqueued(&self) -> u32 {
        self.total_enqueued
    }

    pub(super) fn mean_response_time(&self) -> u32 {
        self.response_time.mean
    }

    pub(super) fn reset_response_time(&mut self, clear: bool) {
        self.response_time.reset(clear);
    }

    /// Both `0x0048C040` and `0x0053FCB0` mark the first reliable sequence
    /// match. Their caller alone decides whether source identity is required.
    pub(super) fn acknowledge(
        &mut self,
        incoming_sequence: u32,
        require_sent: bool,
        mut source_matches: impl FnMut(D) -> bool,
    ) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|entry| {
            entry.frame[2] == 0
                && sequence(&entry.frame) == incoming_sequence
                && (!require_sent || entry.attempts > 0)
                && source_matches(entry.destination)
        }) {
            entry.completed = true;
            return true;
        }
        false
    }

    /// Sole `0x0048C3E0` body. Global and private retirement policies run only
    /// after this method returns, matching `0x0048C3B0`'s virtual dispatch.
    pub(super) fn service(
        &mut self,
        now_ticks: u32,
        timing: RetryTiming,
        send: &mut impl FnMut(D, &[u8]),
        response: &mut impl FnMut(D, u32, u32),
    ) -> bool {
        self.entries.retain(|entry| {
            if entry.completed && entry.frame[2] == 0 {
                let elapsed = now_ticks.wrapping_sub(entry.first_sent);
                self.response_time.record(elapsed);
                response(entry.destination, sequence(&entry.frame), elapsed);
            }
            !entry.completed
        });
        let mut failed = false;
        for entry in &mut self.entries {
            if now_ticks.wrapping_sub(entry.last_sent) <= timing.retry_delta {
                continue;
            }
            send(entry.destination, &entry.frame);
            entry.last_sent = now_ticks;
            if entry.attempts == 0 {
                entry.first_sent = now_ticks;
                entry.completed = entry.frame[2] == 1;
            }
            entry.attempts = entry.attempts.wrapping_add(1);
            let failed_attempt = (timing.max_retries != u32::MAX
                && entry.attempts > timing.max_retries)
                || (timing.timeout != u32::MAX
                    && entry.last_sent.wrapping_sub(entry.first_sent) > timing.timeout);
            entry.failed |= failed_attempt;
            failed |= failed_attempt;
        }
        !failed
    }

    /// Global `0x005402D0`; private failures deliberately keep their entries.
    pub(super) fn discard_failed(&mut self) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|entry| !entry.failed || entry.completed);
        before - self.entries.len()
    }

    /// Global `0x00540340` excludes already completed entries from cancellation.
    pub(super) fn cancel(&mut self, mut matches: impl FnMut(D) -> bool) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|entry| entry.completed || !matches(entry.destination));
        before - self.entries.len()
    }
}

impl LanConnections {
    pub fn new(config: LanConnectionConfig, timing: RetryTiming) -> Self {
        assert!(
            config.send_capacity > 0,
            "LAN send capacity must be positive"
        );
        assert!(
            config.receive_capacity > 0,
            "LAN receive capacity must be positive"
        );
        assert!(config.max_peers > 0, "LAN peer capacity must be positive");
        assert!(
            config.max_payload_bytes <= usize::MAX - CONNECTION_HEADER_LEN,
            "LAN payload capacity must leave room for its header"
        );
        Self {
            config,
            timing,
            peers: Vec::new(),
            next_receive_peer: 0,
        }
    }

    /// `0x00540DC0`: an admitted source's ID and protocol-2 endpoint form one
    /// channel. Duplicate identities are invalid session setup inputs.
    pub fn add_peer(
        &mut self,
        peer_id: i32,
        endpoint: SocketAddrV4,
    ) -> Result<(), PeerAdmissionError> {
        if peer_id == -1 || self.peers.iter().any(|peer| peer.id == peer_id) {
            return Err(PeerAdmissionError::DuplicateId(peer_id));
        }
        if self.peers.iter().any(|peer| peer.endpoint == endpoint) {
            return Err(PeerAdmissionError::DuplicateEndpoint(endpoint));
        }
        if self.peers.len() == self.config.max_peers {
            return Err(PeerAdmissionError::Full);
        }
        self.peers.push(Peer {
            id: peer_id,
            endpoint,
            send: SendQueue::new(
                self.config.send_capacity,
                CONNECTION_HEADER_LEN + self.config.max_payload_bytes,
            ),
            receive: VecDeque::new(),
            next_reliable: 0,
            next_unreliable: 0,
            contiguous_received: u32::MAX,
            delivered_reliable: u32::MAX,
        });
        Ok(())
    }

    /// `0x00540F00`: destroy channel state, compact peers, preserve the numeric
    /// round-robin cursor unless it is now outside the compacted table.
    pub fn remove_peer(&mut self, peer_id: i32) -> bool {
        let Some(index) = self.peers.iter().position(|peer| peer.id == peer_id) else {
            return false;
        };
        self.peers.remove(index);
        if self.next_receive_peer >= self.peers.len() {
            self.next_receive_peer = 0;
        }
        true
    }

    /// The private-channel part of `0x00540A80`; a subsequent peer starts with
    /// fresh queues, sequence counters and response history.
    pub fn reset(&mut self) {
        self.peers.clear();
        self.next_receive_peer = 0;
    }

    pub fn peers(&self) -> impl ExactSizeIterator<Item = (i32, SocketAddrV4)> + '_ {
        self.peers.iter().map(|peer| (peer.id, peer.endpoint))
    }

    /// Active protocol-2 `0x0053EE70` compares IPv4 and port, not MAC fields.
    pub fn peer_id(&self, endpoint: SocketAddrV4) -> Option<i32> {
        self.peers
            .iter()
            .find(|peer| peer.endpoint == endpoint)
            .map(|peer| peer.id)
    }

    pub fn set_timing(&mut self, timing: RetryTiming) {
        self.timing = timing;
    }

    pub(crate) fn retry_timing(&self) -> RetryTiming {
        self.timing
    }

    pub(crate) fn product_id(&self) -> u16 {
        self.config.product_id
    }

    pub fn queued_send_count(&self, peer_id: i32) -> Option<usize> {
        self.peers
            .iter()
            .find(|peer| peer.id == peer_id)
            .map(|peer| peer.send.len())
    }

    pub fn mean_response_time(&self, peer_id: i32) -> Option<u32> {
        self.peers
            .iter()
            .find(|peer| peer.id == peer_id)
            .map(|peer| peer.send.mean_response_time())
    }

    /// Private-channel part of IPXManager::ResetResponseTime542EC0. Keep
    /// queues, delivery cursors, sequence counters and retry state intact.
    pub fn reset_response_time(&mut self, clear: bool) {
        for peer in &mut self.peers {
            peer.send.reset_response_time(clear);
        }
    }

    /// `0x005414C0`: reserve every peer's send slot before admitting any copy.
    /// ACKed entries still occupy their slots until the next service pass.
    pub fn broadcast(&mut self, payload: &[u8], delivery: Delivery) -> bool {
        if self.peers.is_empty()
            || payload.len() > self.config.max_payload_bytes
            || self
                .peers
                .iter()
                .any(|peer| !peer.send.can_enqueue(CONNECTION_HEADER_LEN + payload.len()))
        {
            return false;
        }
        for peer in &mut self.peers {
            let accepted = peer.enqueue(payload, delivery, self.config.product_id);
            debug_assert!(accepted, "broadcast preflight reserved every send slot");
        }
        true
    }

    pub fn send_to(&mut self, peer_id: i32, payload: &[u8], delivery: Delivery) -> bool {
        if payload.len() > self.config.max_payload_bytes {
            return false;
        }
        let Some(peer) = self.peers.iter_mut().find(|peer| peer.id == peer_id) else {
            return false;
        };
        peer.enqueue(payload, delivery, self.config.product_id)
    }

    /// Route and process one checksum-validated connection frame. The caller
    /// drains socket input before `service`, matching `0x00541820`.
    ///
    /// Reliable acceptance (including duplicate acceptance) emits its ACK now;
    /// a full queue emits none. The send callback queues/emits bytes directly
    /// and cannot re-enter this owner. Native `0x0048C2EA` ignores its return.
    pub fn receive(
        &mut self,
        source: SocketAddrV4,
        frame: &[u8],
        send: &mut impl FnMut(SocketAddrV4, &[u8]),
    ) -> Result<ReceiveOutcome, ReceiveError> {
        let peer = self
            .peers
            .iter_mut()
            .find(|peer| peer.endpoint == source)
            .ok_or(ReceiveError::UnknownPeer(source))?;
        if frame.len() < CONNECTION_HEADER_LEN {
            // Native assumes a readable header. Reject malformed datagrams
            // safely instead of reproducing its out-of-bounds memory access.
            return Err(ReceiveError::ShortHeader);
        }
        let product = u16::from_le_bytes([frame[0], frame[1]]);
        if product != self.config.product_id {
            return Err(ReceiveError::WrongProduct {
                expected: self.config.product_id,
                actual: product,
            });
        }
        let incoming_sequence = sequence(frame);
        if frame[2] == 2 {
            peer.send.acknowledge(incoming_sequence, false, |()| true);
            return Ok(ReceiveOutcome::Acknowledgment);
        }
        if frame[2] > 1 {
            return Err(ReceiveError::UnknownKind(frame[2]));
        }
        let duplicate = frame[2] == 0
            && ((peer.contiguous_received != u32::MAX
                && incoming_sequence <= peer.contiguous_received)
                || peer.receive.iter().any(|entry| {
                    entry.frame[2] == 0 && sequence(&entry.frame) == incoming_sequence
                }));
        if !duplicate {
            let free = self.config.receive_capacity - peer.receive.len();
            let is_next_reliable =
                frame[2] == 0 && incoming_sequence == peer.contiguous_received.wrapping_add(1);
            if free == 0 || (free == 1 && !is_next_reliable) {
                return Err(ReceiveError::Full);
            }
            if frame.len() - CONNECTION_HEADER_LEN > self.config.max_payload_bytes {
                return Err(ReceiveError::PayloadTooLong);
            }
            peer.receive.push_back(ReceiveEntry {
                frame: frame.to_vec(),
                consumed: false,
            });
            if is_next_reliable {
                peer.contiguous_received = incoming_sequence;
                while peer.receive.iter().any(|entry| {
                    entry.frame[2] == 0
                        && sequence(&entry.frame) == peer.contiguous_received.wrapping_add(1)
                }) {
                    peer.contiguous_received = peer.contiguous_received.wrapping_add(1);
                }
            }
        }
        if frame[2] == 0 {
            let ack = connection_frame(self.config.product_id, 2, incoming_sequence, &[]);
            send(peer.endpoint, &ack);
        }
        Ok(if duplicate {
            ReceiveOutcome::DuplicateAcknowledged
        } else {
            ReceiveOutcome::Queued
        })
    }

    /// `0x0048C3B0/0x0048C3E0`, then private manager `0x0054222F..0x0054226A`.
    /// Services all peers in registration order and returns the last failure.
    /// A failed channel skips receive retirement on this pass only.
    pub fn service(
        &mut self,
        now_ticks: u32,
        send: &mut impl FnMut(SocketAddrV4, &[u8]),
    ) -> Option<i32> {
        let mut failed_peer = None;
        for peer in &mut self.peers {
            if peer.service(now_ticks, self.timing, send) {
                peer.retire_received();
            } else {
                failed_peer = Some(peer.id);
            }
        }
        failed_peer
    }

    /// `0x005415F0` with each connection's `0x0048C320`. Unreliable packets can
    /// pass a reliable gap; reliable packets are delivered in sequence order.
    pub fn pop_received(&mut self) -> Option<ReceivedMessage> {
        if self.peers.is_empty() {
            return None;
        }
        for _ in 0..self.peers.len() {
            let index = self.next_receive_peer;
            self.next_receive_peer = (index + 1) % self.peers.len();
            let peer = &mut self.peers[index];
            if let Some(message) = peer.pop_received() {
                return Some(message);
            }
        }
        None
    }
}

impl Peer {
    fn enqueue(&mut self, payload: &[u8], delivery: Delivery, product_id: u16) -> bool {
        let next = match delivery {
            Delivery::Reliable => &mut self.next_reliable,
            Delivery::Unreliable => &mut self.next_unreliable,
        };
        if !self.send.enqueue(
            connection_frame(product_id, delivery.wire_kind(), *next, payload),
            (),
        ) {
            return false;
        }
        *next = next.wrapping_add(1);
        true
    }

    fn service(
        &mut self,
        now_ticks: u32,
        timing: RetryTiming,
        send: &mut impl FnMut(SocketAddrV4, &[u8]),
    ) -> bool {
        self.send.service(
            now_ticks,
            timing,
            &mut |(), frame| {
                send(self.endpoint, frame);
            },
            &mut |(), _, _| {},
        )
    }

    fn pop_received(&mut self) -> Option<ReceivedMessage> {
        let entry = self.receive.iter_mut().find(|entry| {
            !entry.consumed
                && (entry.frame[2] == 1
                    || sequence(&entry.frame) == self.delivered_reliable.wrapping_add(1))
        })?;
        if entry.frame[2] == 0 {
            self.delivered_reliable = sequence(&entry.frame);
        }
        entry.consumed = true;
        Some(ReceivedMessage {
            peer_id: self.id,
            payload: entry.frame[CONNECTION_HEADER_LEN..].to_vec(),
            delivery: if entry.frame[2] == 0 {
                Delivery::Reliable
            } else {
                Delivery::Unreliable
            },
        })
    }

    fn retire_received(&mut self) {
        self.receive.retain(|entry| {
            !entry.consumed
                || (entry.frame[2] == 0 && sequence(&entry.frame) >= self.contiguous_received)
        });
    }
}

pub(super) fn sequence(frame: &[u8]) -> u32 {
    u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]])
}

/// `0x0048BF40` plus direct-LAN `0x0053F767..0x0053F774`. Native LAN data
/// leaves byte3 untouched; zero is safe because the direct receiver ignores
/// it. ACK byte3 is explicitly zero. Router-specific behavior is separate.
fn connection_frame(product_id: u16, kind: u8, sequence: u32, payload: &[u8]) -> Vec<u8> {
    let mut frame = connection_prefix(product_id, kind, sequence, payload.len());
    frame[CONNECTION_HEADER_LEN..].copy_from_slice(payload);
    frame
}

/// Common 14-byte prefix; global data adds its product word before payload.
pub(super) fn connection_prefix(
    discriminator: u16,
    kind: u8,
    sequence: u32,
    body_len: usize,
) -> Vec<u8> {
    let mut frame = vec![0; CONNECTION_HEADER_LEN + body_len];
    frame[..2].copy_from_slice(&discriminator.to_le_bytes());
    frame[2] = kind;
    frame[4..8].copy_from_slice(&sequence.to_le_bytes());
    frame
}

/// UDP's `0x007B1FD0/0x007B2050` byte adapter around the shared `0x0064DE80`
/// accumulator. This checksum precedes, and covers, the connection frame.
pub fn udp_checksum(connection_frame: &[u8]) -> u32 {
    let mut checksum = RetailChecksumAccumulator::new();
    let mut words = connection_frame.chunks_exact(4);
    for word in &mut words {
        checksum.fold_value(u32::from_le_bytes([word[0], word[1], word[2], word[3]]));
    }
    let remainder = words.remainder();
    if !remainder.is_empty() {
        let mut word = [0; 4];
        word[..remainder.len()].copy_from_slice(remainder);
        checksum.fold_value(u32::from_le_bytes(word));
    }
    checksum.value()
}

#[cfg(test)]
#[path = "reliable_tests.rs"]
mod tests;
