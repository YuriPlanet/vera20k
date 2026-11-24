//! Authenticated datagram routing and fixed-clock opaque command ordering for
//! VERA clients. This transport owns no simulation state.
//!
//! The HTTPS lobby signs a 98-byte v2 ticket and separately returns its derived
//! packet key and a match payload key, which clients no longer use. Every datagram travels in
//! envelope v3 (`envelope`, bandwidth-compaction spec §6) under the hop's
//! packet key; after its handshake a hop is named by the session the relay
//! assigns (§6.2). Relayed DATA is plaintext (§3.2). Domain-separated
//! HMAC-SHA256 uses RustCrypto's constant-time verification:
//! https://docs.rs/hmac/0.12.1/hmac/ . Neither key type prints secret bytes.
//! A watcher's hop holds a v3 watch ticket of the same length instead
//! (`WatchTicket`, README "Watchers").

use std::collections::HashMap;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddrV4};

use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroize;

pub mod envelope;
mod journal;
mod link;
#[cfg(any(test, feature = "test-support"))]
pub mod netsim;
mod sequenced;
pub mod varint;
#[cfg(any(test, feature = "test-support"))]
pub use link::ReaderLinkStats;
pub use link::{
    ClientLinkStats, HOP_HANDSHAKE_INTERVAL_MS, HOP_PING_INTERVAL_MS, HOP_READINESS_TIMEOUT_MS,
    HopFeedback, LEAVE_ATTEMPTS, LEAVE_INTERVAL_MS, LinkState, OrderingGap, PACE_INTERVAL_MS,
    READY_REQUEST_INTERVAL_MS, REORDER_WINDOW, RESTORE_PULL_INTERVAL_MS, RecvWindow,
};
pub use sequenced::federation::{FEDERATION_LOST, FederationOutcome, MAX_FEDERATED_RELAYS};
pub use sequenced::watch::{
    MAX_ROOM_WATCHERS, MAX_WATCH_DELAY_SECONDS, WATCH_MARK_MS, WATCH_TICKET_VERSION, WatchTicket,
};
pub use sequenced::{
    ABSENT_MS, CHECKSUM_INTERVAL, CONTINUOUS_FLOOR, CertificatePacing, FRAME_RATES,
    MAX_FRAME_BATCHES, ORDERING_BATCH_CAPACITY, OrderEntry, OrderError, OrderEvent, OrderPace,
    OrderStatus, OrderingCertificate, PaceLag, SUSTAIN_PERCENT, admits_rate, paced_rates,
    turn_frames, turn_ms,
};

type HmacSha256 = Hmac<Sha256>;

pub const MAX_PEERS: u8 = 30;
// Relay, wire and client source masks are u32 with one bit per source.
const _: () = assert!(MAX_PEERS as u32 <= u32::BITS);
pub const BROADCAST: u8 = 255;
/// The largest DATA message a client relays.
pub const MAX_PAYLOAD: usize = 640;
pub const TICKET_LEN: usize = 98;
/// A player ticket's first byte; a watch ticket's is `WATCH_TICKET_VERSION`.
const TICKET_VERSION: u8 = 2;
pub const MAX_DATAGRAM: usize = envelope::MAX_DATAGRAM;
pub const MAX_TICKET_LIFETIME: u64 = 300;
const TICKET_BODY_LEN: usize = 66;
use envelope::{
    CHALLENGE, CONFIRM, CONTROL, DATA, HEADER_LEN, HELLO, HOP_TAG_LEN, LEAVE_ACK, PING, READY,
};
/// A full HMAC-SHA256: ticket signatures and federation links.
const MAC_LEN: usize = 32;
const COOKIE_LEN: usize = 40;
const TICKET_DOMAIN: &[u8] = b"VERA20k relay ticket v2\0";
const KEY_DOMAIN: &[u8] = b"VERA20k relay packet key v2\0";
const COOKIE_DOMAIN: &[u8] = b"VERA20k relay source v2\0";
const INGRESS_PER_SECOND: u64 = 16_384;
// Room for the lobby's 320 sessions and an hour's 640 new guests
// (docs/lobby-protocol.md), whose tickets live 300 seconds, each at their own
// rate. A handshake costs a few HMACs.
const HANDSHAKES_PER_SECOND: u64 = 8192;
const PLAYER_HANDSHAKES_PER_SECOND: u64 = 8;
// Pulls that read the journal. A catching-up client sends about ten a second.
const PLAYER_ARCHIVE_PULLS_PER_SECOND: u64 = 256;
// One player's datagrams in, and its ordering datagrams out, each a second.
const PLAYER_PACKETS_PER_SECOND: u64 = 2048;
const PLAYER_BYTES_PER_SECOND: u64 = 2 * 1024 * 1024;
const ROOM_BYTES_PER_SECOND: u64 = 64 * 1024 * 1024;
const TOTAL_BYTES_PER_SECOND: u64 = 64 * 1024 * 1024;
const LEAVES_PER_SECOND: u64 = 512;
const LEAVE_GRACE: u64 = sequenced::ROOM_LIFETIME_MS.div_ceil(1000) + MAX_TICKET_LIFETIME;

/// Server signing material or a single client's packet key. HTTPS credentials
/// have their own owner; this value is only held in process memory.
#[derive(Clone)]
pub struct SecretKey([u8; 32]);

impl SecretKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    fn mac(&self, domain: &[u8], bytes: &[u8]) -> HmacSha256 {
        let mut mac = <HmacSha256 as Mac>::new_from_slice(&self.0)
            .expect("SHA-256 HMAC accepts any key size");
        mac.update(domain);
        mac.update(bytes);
        mac
    }

    /// For an HTTPS response to the admitted client, never logs or UDP.
    pub fn expose_for_https(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Drop for SecretKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretKey([redacted])")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    pub room: [u8; 16],
    pub source: u8,
    pub expires: u64,
    pub nonce: [u8; 16],
    pub player: [u8; 16],
    pub generation: u64,
}

impl Ticket {
    pub fn parse(bytes: &[u8]) -> Result<Self, RelayError> {
        if bytes.len() != TICKET_LEN || bytes[0] != TICKET_VERSION || bytes[17] >= MAX_PEERS {
            return Err(RelayError::Malformed);
        }
        let ticket = Self {
            room: link::field(bytes, 1)?,
            source: bytes[17],
            expires: link::read_u64(bytes, 18)?,
            nonce: link::field(bytes, 26)?,
            player: link::field(bytes, 42)?,
            generation: link::read_u64(bytes, 58)?,
        };
        if ticket.generation == 0 || ticket.player == [0; 16] {
            return Err(RelayError::Malformed);
        }
        Ok(ticket)
    }

    /// The same player in the same match seat; a hop renewal keeps it.
    fn same_identity(&self, other: &Self) -> bool {
        self.room == other.room
            && self.source == other.source
            && self.player == other.player
            && self.generation == other.generation
    }

    /// Cloudflare and native test clients use this same fixed byte layout.
    /// The random nonce and room identifier must come from OS cryptography.
    pub fn sign(self, server_key: &SecretKey) -> Result<[u8; TICKET_LEN], RelayError> {
        if self.source >= MAX_PEERS || self.generation == 0 || self.player == [0; 16] {
            return Err(RelayError::Malformed);
        }
        let mut bytes = [0; TICKET_LEN];
        bytes[0] = TICKET_VERSION;
        bytes[1..17].copy_from_slice(&self.room);
        bytes[17] = self.source;
        bytes[18..26].copy_from_slice(&self.expires.to_le_bytes());
        bytes[26..42].copy_from_slice(&self.nonce);
        bytes[42..58].copy_from_slice(&self.player);
        bytes[58..66].copy_from_slice(&self.generation.to_le_bytes());
        let signature = server_key
            .mac(TICKET_DOMAIN, &bytes[..TICKET_BODY_LEN])
            .finalize()
            .into_bytes();
        bytes[TICKET_BODY_LEN..].copy_from_slice(&signature);
        Ok(bytes)
    }
}

pub fn derive_packet_key(server_key: &SecretKey, ticket: &[u8; TICKET_LEN]) -> SecretKey {
    SecretKey(
        server_key
            .mac(KEY_DOMAIN, ticket)
            .finalize()
            .into_bytes()
            .into(),
    )
}

/// Immutable endpoint labels let existing reliable/global owners keep their
/// endpoint identity rules while the physical UDP destination is the relay.
pub fn peer_endpoint(source: u8) -> Result<SocketAddrV4, RelayError> {
    if source >= MAX_PEERS {
        return Err(RelayError::Malformed);
    }
    Ok(SocketAddrV4::new(
        Ipv4Addr::new(127, 77, 0, source + 1),
        1234,
    ))
}

pub fn endpoint_peer(endpoint: SocketAddrV4) -> Option<u8> {
    let [a, b, c, d] = endpoint.ip().octets();
    (a == 127 && b == 77 && c == 0 && d > 0 && d <= MAX_PEERS).then_some(d.wrapping_sub(1))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RelayError {
    #[error("malformed relay datagram")]
    Malformed,
    #[error("relay authentication failed")]
    Authentication,
    #[error("relay ticket expired or exceeds its allowed lifetime")]
    Expired,
    #[error("replayed relay datagram")]
    Replay,
    #[error("relay address must complete its challenge")]
    Unregistered,
    #[error("relay session capacity reached")]
    Full,
    #[error("relay packet rate exceeded")]
    RateLimited,
    #[error("relay counter exhausted")]
    CounterExhausted,
    #[error("secure random source unavailable")]
    EntropyUnavailable,
}

/// A small sliding window accepts reordered packets once. State changes only
/// after the authentication tag and fixed framing have been checked.
#[derive(Debug, Default)]
struct ReplayWindow {
    maximum: u64,
    seen: u128,
}

impl ReplayWindow {
    /// Returns whether the accepted sequence advanced the high watermark.
    fn accept(&mut self, sequence: u64) -> Result<bool, RelayError> {
        if sequence == 0 {
            return Err(RelayError::Replay);
        }
        if sequence > self.maximum {
            let advance = sequence - self.maximum;
            self.seen = if advance >= 128 {
                1
            } else {
                (self.seen << advance) | 1
            };
            self.maximum = sequence;
            return Ok(true);
        }
        let behind = self.maximum - sequence;
        if behind >= 128 || self.seen & (1u128 << behind) != 0 {
            return Err(RelayError::Replay);
        }
        self.seen |= 1u128 << behind;
        Ok(false)
    }

    /// Adaptive feedback reports the newest 64 receipts (spec §4.1).
    fn feedback_window(&self) -> RecvWindow {
        RecvWindow {
            high: self.maximum,
            bits: self.seen as u64,
        }
    }
}

pub struct ClientSession {
    ticket_bytes: [u8; TICKET_LEN],
    ticket: Ticket,
    key: SecretKey,
    sent: u64,
    probe: Option<u64>,
    ready: bool,
    // Assigned by the READY answering this hop's CONFIRM (spec §6.2).
    session: Option<u32>,
    // Relay datagrams of this session with a wrong tag (spec §6.4 item 6).
    mac_failures: u64,
    ordering: Option<sequenced::ClientOrdering>,
    // The lobby's relay roster of a federated match, sent with each CONFIRM.
    federation: Option<Vec<u8>>,
    hop: link::HopLink,
    // The `now_ms` of the timed call in progress (spec §4.2, §4.4).
    clock_ms: u64,
    // The hop rides a reliable stream to a UDP gateway (`use_stream_transport`).
    stream_transport: bool,
    // Test support: start the uplink measured good (see
    // `preset_uplink_measured_good`).
    #[cfg(any(test, feature = "test-support"))]
    measured_good: bool,
}

impl fmt::Debug for ClientSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientSession")
            .field("source", &self.ticket.source)
            .field("ready", &self.ready)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub enum ServerEvent<'a> {
    Challenge(&'a [u8]),
    Ready,
    /// A relayed DATA message, plaintext (spec §3.2, §6.1).
    Payload {
        source: u8,
        bytes: &'a [u8],
    },
    Ordering(OrderEvent),
    /// Consecutive history entries packed into one datagram, in order.
    OrderingPack(Vec<OrderEvent>),
}

impl ClientSession {
    /// A hop for `ticket` under its `packet_key`. `_payload_key` is the
    /// match key the lobby still grants; relayed traffic is plaintext
    /// (compaction spec §3.2), so it is accepted and not used, and the lobby
    /// needs no change. A watch ticket makes a watcher's hop
    /// (`is_watching`).
    pub fn new(
        ticket: &[u8],
        packet_key: [u8; 32],
        _payload_key: [u8; 32],
    ) -> Result<Self, RelayError> {
        Ok(Self {
            ticket: match ticket.first() {
                Some(&WATCH_TICKET_VERSION) => WatchTicket::parse(ticket)?.hop(),
                _ => Ticket::parse(ticket)?,
            },
            ticket_bytes: ticket.try_into().map_err(|_| RelayError::Malformed)?,
            key: SecretKey::new(packet_key),
            sent: 0,
            probe: None,
            ready: false,
            session: None,
            mac_failures: 0,
            ordering: None,
            federation: None,
            hop: link::HopLink::default(),
            clock_ms: 0,
            stream_transport: false,
            #[cfg(any(test, feature = "test-support"))]
            measured_good: false,
        })
    }

    /// Marks a hop whose datagrams ride a reliable, ordered stream to a
    /// gateway that forwards each one as a UDP datagram: the browser's
    /// WebSocket to `tools/wasm/gateway`. A copy there can only queue behind
    /// its original, so the lossy-state copies (spec §6.3 row 6) are not
    /// sent; RTO resends (row 4) still recover what the gateway drops or the
    /// UDP leg loses. Renewals keep the mark.
    pub fn use_stream_transport(&mut self) {
        self.stream_transport = true;
    }

    /// Carry a federated match's roster in every admission of this session.
    pub fn set_federation(&mut self, roster: &[u8]) -> Result<(), RelayError> {
        let (room, relays) = sequenced::federation::roster_table(roster)?;
        if room != self.ticket.room
            || !relays
                .iter()
                .any(|(_, sources)| sources & (1 << self.ticket.source) != 0)
        {
            return Err(RelayError::Authentication);
        }
        self.federation = Some(roster.to_vec());
        Ok(())
    }

    pub fn source(&self) -> u8 {
        self.ticket.source
    }
    pub fn is_ready(&self) -> bool {
        self.ready
    }
    pub fn expires(&self) -> u64 {
        self.ticket.expires
    }
    pub fn same_lease(&self, ticket: &[u8]) -> bool {
        self.ticket_bytes.as_slice() == ticket
    }

    /// Prepare a fresh key without discarding the admitted route. Identity
    /// cannot change during a match, and delayed grants cannot roll it back.
    /// `_payload_key` as in `new`.
    pub fn renewal(
        &self,
        ticket: &[u8],
        packet_key: [u8; 32],
        _payload_key: [u8; 32],
    ) -> Result<Self, RelayError> {
        let mut next = Self::new(ticket, packet_key, [0; 32])?;
        if !next.ticket.same_identity(&self.ticket)
            || next.ticket.nonce == self.ticket.nonce
            || next.ticket.expires <= self.ticket.expires
        {
            return Err(RelayError::Authentication);
        }
        next.federation = self.federation.clone();
        next.stream_transport = self.stream_transport;
        Ok(next)
    }
    pub fn forget_readiness(&mut self) {
        self.ready = false;
    }

    pub(crate) fn probe(&mut self, out: &mut Vec<u8>) -> Result<(), RelayError> {
        if self.ready {
            self.encode(PING, &[], out)?;
        } else {
            let ticket = self.ticket_bytes;
            self.encode(HELLO, &ticket, out)?;
        }
        self.probe = Some(self.sent);
        Ok(())
    }

    pub(crate) fn confirm(&mut self, cookie: &[u8], out: &mut Vec<u8>) -> Result<(), RelayError> {
        if cookie.len() != COOKIE_LEN {
            return Err(RelayError::Malformed);
        }
        let mut payload = self.ticket_bytes.to_vec();
        payload.extend_from_slice(cookie);
        payload.extend(self.federation.iter().flatten());
        self.encode(CONFIRM, &payload, out)
    }

    pub(crate) fn data(
        &mut self,
        target: u8,
        payload: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        if !self.ready || self.is_watching() {
            return Err(RelayError::Unregistered);
        }
        if !data_fits(target, payload) {
            return Err(RelayError::Malformed);
        }
        // `[target u8][data]`, plaintext (spec §3.2, §6.1).
        let mut wire = Vec::with_capacity(1 + payload.len());
        wire.push(target);
        wire.extend_from_slice(payload);
        self.encode(DATA, &wire, out)
    }

    /// One envelope v3 datagram of this hop (spec §6.1): HELLO and CONFIRM
    /// carry session 0, every other kind the hop's session.
    fn encode(&mut self, kind: u8, payload: &[u8], out: &mut Vec<u8>) -> Result<(), RelayError> {
        let session = match kind {
            HELLO | CONFIRM => 0,
            _ => self.session.ok_or(RelayError::Unregistered)?,
        };
        let sequence = self
            .sent
            .checked_add(1)
            .filter(|sequence| *sequence <= u64::from(u32::MAX))
            .ok_or(RelayError::CounterExhausted)?;
        envelope::seal(
            envelope::Direction::Up,
            &self.key,
            envelope::Header {
                kind,
                session,
                sequence: sequence as u32,
            },
            payload,
            out,
        )?;
        self.sent = sequence;
        if kind != HELLO {
            self.hop_sent(self.sent, kind != PING);
        }
        Ok(())
    }

    pub(crate) fn receive<'a>(&mut self, bytes: &'a [u8]) -> Result<ServerEvent<'a>, RelayError> {
        let header = envelope::peek(envelope::Direction::Down, bytes)?;
        // The READY answering a CONFIRM brings the session; this hop claims
        // it by its tag (spec §6.2 "客户端认领 READY").
        let claim = header.kind == READY && !self.ready && header.session != 0;
        let expected = match header.kind {
            CHALLENGE => 0,
            _ if claim => header.session,
            _ => self.session.ok_or(RelayError::Unregistered)?,
        };
        if header.session != expected {
            return Err(RelayError::Authentication);
        }
        let payload = match envelope::open(envelope::Direction::Down, &self.key, bytes) {
            Ok((_, payload)) => payload,
            Err(error) => {
                // A renewal's other hop tries the same READY or CHALLENGE
                // (spec §6.2): only this session's own datagrams count.
                if error == RelayError::Authentication && header.kind != CHALLENGE && !claim {
                    self.mac_failures += 1;
                }
                return Err(error);
            }
        };
        let sequence = u64::from(header.sequence);
        match header.kind {
            CHALLENGE if payload.len() == COOKIE_LEN && self.probe == Some(sequence) => {
                self.probe = None;
                self.ready = false;
                // The relay drops the subscription of a new path, so its
                // first UPLINK goes at once again (spec §4.7, §6.0).
                self.hop.uplinks = 0;
                self.hop.first_pack = false;
                Ok(ServerEvent::Challenge(payload))
            }
            READY if payload.is_empty() => {
                self.hop.heard(sequence, self.clock_ms)?;
                if claim {
                    self.session = Some(header.session);
                }
                self.ready = true;
                Ok(ServerEvent::Ready)
            }
            CONTROL if self.ready => {
                self.hop.heard(sequence, self.clock_ms)?;
                if payload.first() == Some(&sequenced::PACK_EVENT) {
                    return Ok(ServerEvent::OrderingPack(
                        self.receive_ordering_pack(payload)?,
                    ));
                }
                Ok(ServerEvent::Ordering(self.receive_ordering(payload)?))
            }
            LEAVE_ACK if payload.is_empty() && self.accepts_ordering_leave_ack(sequence) => {
                // This terminal response echoes the exact cached request,
                // like Challenge. It survives expiry of the peer's independent
                // server sequence and cannot admit or resume any game traffic.
                Ok(ServerEvent::Ordering(OrderEvent::Left))
            }
            // `[source u8][data]`, plaintext (spec §3.2, §6.1).
            DATA if self.ready && payload.len() > 1 && payload[0] < MAX_PEERS => {
                self.hop.heard(sequence, self.clock_ms)?;
                Ok(ServerEvent::Payload {
                    source: payload[0],
                    bytes: &payload[1..],
                })
            }
            _ => Err(RelayError::Malformed),
        }
    }
}

struct Incoming<'a> {
    ticket: Ticket,
    // HELLO and CONFIRM only: the ticket they carry (spec §6.1).
    ticket_bytes: Option<&'a [u8; TICKET_LEN]>,
    key: SecretKey,
    kind: u8,
    // The hop's session; 0 on HELLO and CONFIRM (spec §6.1).
    session: u32,
    sequence: u64,
    // DATA's `target`, else `BROADCAST`.
    target: u8,
    // The payload after the ticket (HELLO, CONFIRM) or target (DATA).
    payload: &'a [u8],
}

/// A HELLO or CONFIRM (spec §6.1): `[ticket 98]`, then for CONFIRM
/// `[cookie 40][roster]`. The ticket's signature and expiry are checked and
/// the hop key derived from it before the envelope tag. A watch ticket
/// comes back as its hop's view, `source` `WATCH_SOURCE`.
fn handshake_incoming<'a>(
    key: &SecretKey,
    header: envelope::Header,
    bytes: &'a [u8],
    now: u64,
) -> Result<Incoming<'a>, RelayError> {
    let body = &bytes[HEADER_LEN..bytes.len() - HOP_TAG_LEN];
    let Some((ticket_bytes, payload)) = body
        .split_first_chunk::<TICKET_LEN>()
        .filter(|_| header.session == 0)
    else {
        return Err(RelayError::Malformed);
    };
    if (header.kind == HELLO && !payload.is_empty())
        || (header.kind == CONFIRM
            && payload.len() != COOKIE_LEN
            && !(payload.len() > COOKIE_LEN
                && sequenced::federation::is_roster_len(payload.len() - COOKIE_LEN)))
    {
        return Err(RelayError::Malformed);
    }
    let (ticket, domain) = match ticket_bytes[0] {
        WATCH_TICKET_VERSION => (
            WatchTicket::parse(ticket_bytes)?.hop(),
            sequenced::watch::WATCH_TICKET_DOMAIN,
        ),
        _ => (Ticket::parse(ticket_bytes)?, TICKET_DOMAIN),
    };
    key.mac(domain, &ticket_bytes[..TICKET_BODY_LEN])
        .verify_slice(&ticket_bytes[TICKET_BODY_LEN..])
        .map_err(|_| RelayError::Authentication)?;
    if ticket.expires > now.saturating_add(MAX_TICKET_LIFETIME) || ticket.expires <= now {
        return Err(RelayError::Expired);
    }
    let packet_key = derive_packet_key(key, ticket_bytes);
    envelope::open(envelope::Direction::Up, &packet_key, bytes)?;
    if header.sequence == 0 {
        return Err(RelayError::Replay);
    }
    Ok(Incoming {
        ticket,
        ticket_bytes: Some(ticket_bytes),
        key: packet_key,
        kind: header.kind,
        session: 0,
        sequence: u64::from(header.sequence),
        target: BROADCAST,
        payload,
    })
}

/// The framing of a session datagram's payload (spec §6.1): PING empty,
/// CONTROL non-empty, DATA `[target][data]`.
fn session_payload(kind: u8, payload: &[u8]) -> Result<(u8, &[u8]), RelayError> {
    match kind {
        PING if payload.is_empty() => Ok((BROADCAST, payload)),
        CONTROL if !payload.is_empty() => Ok((BROADCAST, payload)),
        DATA => match payload.split_first() {
            Some((&target, data)) if data_fits(target, data) => Ok((target, data)),
            _ => Err(RelayError::Malformed),
        },
        _ => Err(RelayError::Malformed),
    }
}

/// A player DATA packet's bounds, as a client sends it and as a relay takes
/// it from a hop or a federation link: 1..=`MAX_PAYLOAD` plaintext bytes
/// (compaction spec §3.2) to one source or to `BROADCAST`.
fn data_fits(target: u8, data: &[u8]) -> bool {
    !data.is_empty() && data.len() <= MAX_PAYLOAD && (target < MAX_PEERS || target == BROADCAST)
}

#[derive(Clone, Copy, Default)]
struct RateBudget {
    second: u64,
    spent: u64,
}

impl RateBudget {
    fn permits(&self, now: u64, amount: u64, limit: u64) -> bool {
        amount <= self.remaining(now, limit)
    }

    fn remaining(&self, now: u64, limit: u64) -> u64 {
        let spent = if self.second == now { self.spent } else { 0 };
        limit.saturating_sub(spent)
    }

    fn charge(&mut self, now: u64, amount: u64) {
        if self.second != now {
            self.second = now;
            self.spent = 0;
        }
        self.spent += amount;
    }
}

/// A per-second budget that rooms share max-min fairly: a relay-wide total,
/// or one room's part of it. Each second `allocate` reserves every part what
/// it asked for the second before, the largest asks cut to one level so the
/// reservations fit the total. A part spends its reservation first, then only
/// what no part reserved, so a room flooding past its level takes nothing
/// another room steadily uses.
#[derive(Default)]
struct SharedBudget {
    second: u64,
    spent: u64,
    // A part's asks this second, admitted or refused.
    asked: u64,
    // Reservations still unspent this second.
    reserved: u64,
}

impl SharedBudget {
    fn roll(&mut self, now: u64) {
        if self.second != now {
            *self = Self {
                second: now,
                ..Self::default()
            };
        }
    }

    /// A room's own rate within its part.
    fn permits(&self, now: u64, amount: u64, limit: u64) -> bool {
        let spent = if self.second == now { self.spent } else { 0 };
        amount <= limit.saturating_sub(spent)
    }

    /// Reserve this second's parts of a `limit` total.
    fn allocate<'a>(&mut self, now: u64, limit: u64, parts: impl Iterator<Item = &'a mut Self>) {
        let mut parts: Vec<_> = parts
            .map(|part| {
                (
                    if part.second + 1 == now {
                        part.asked
                    } else {
                        0
                    },
                    part,
                )
            })
            .collect();
        parts.sort_unstable_by_key(|(asked, _)| *asked);
        let (mut left, mut level) = (limit, u64::MAX);
        for (index, (asked, _)) in parts.iter().enumerate() {
            let share = left / (parts.len() - index) as u64;
            if *asked > share {
                level = share;
                break;
            }
            left -= asked;
        }
        *self = Self {
            second: now,
            ..Self::default()
        };
        for (asked, part) in parts {
            *part = Self {
                second: now,
                reserved: asked.min(level),
                ..Self::default()
            };
            self.reserved += part.reserved;
        }
    }

    /// Whether `part`'s room may spend `amount` of this `limit` total: its
    /// own reservation first, then only what no part reserved. Admitted or
    /// not, the ask counts toward the room's next reservation.
    fn admit_part(&mut self, part: &mut Self, now: u64, amount: u64, limit: u64) -> bool {
        self.roll(now);
        part.roll(now);
        part.asked = part.asked.saturating_add(amount);
        amount.saturating_sub(part.reserved) <= limit.saturating_sub(self.spent + self.reserved)
    }

    /// Spend `amount` that `admit_part` admitted.
    fn charge_part(&mut self, part: &mut Self, amount: u64) {
        let own = amount.min(part.reserved);
        part.reserved -= own;
        part.spent += amount;
        self.reserved = self.reserved.saturating_sub(own);
        self.spent += amount;
    }
}

struct Peer {
    ticket: Ticket,
    key: SecretKey,
    // This hop's key in `RelayServer::sessions` (spec §6.2).
    session: u32,
    endpoint: SocketAddrV4,
    sent: u64,
    // After its LEAVE the hop accepts only LEAVE, until this second
    // (spec §6.2 "释放").
    left_until: Option<u64>,
    push: Option<sequenced::PushSubscription>,
    link: link::ReaderLink,
}

/// A hop that accepts only LEAVE until `until`, unix seconds (spec §6.2
/// "释放"), set by `park_session`: a player's whose ticket expired or a
/// renewal replaced, or a watcher's that left, was renewed or lost its place.
struct LeavingHop {
    ticket: Ticket,
    key: SecretKey,
    until: u64,
}

/// Where a session's hop lives (spec §6.2).
enum SessionHop {
    /// A peer of `room` with this `source`.
    Peer {
        room: [u8; 16],
        source: u8,
    },
    Leaving(Box<LeavingHop>),
    /// A watcher of `room` (README "Watchers").
    Watcher {
        room: [u8; 16],
    },
}

struct SessionSlot {
    hop: SessionHop,
    // Tag failures in the fixed 60 s window `failure_window` (spec §6.4
    // item 2).
    failure_window: u64,
    failures: u64,
}

/// A released session may not be assigned again for this long (spec §6.2).
const SESSION_QUARANTINE: u64 = 600;
/// Failures of one session in one fixed window that log an alert (§6.4).
const SESSION_ALERT_FAILURES: u64 = 4096;
const SESSION_ALERT_WINDOW: u64 = 60;

#[derive(Default)]
struct Room {
    peers: Vec<Peer>,
    outgoing: SharedBudget,
    ingress: SharedBudget,
    order_packets: SharedBudget,
    ordering: Option<sequenced::OrderingRoom>,
    // Set by the first admission carrying a lobby relay roster.
    federation: Option<sequenced::federation::Federation>,
    // Read-only hops apart from the players (README "Watchers").
    watchers: Vec<sequenced::watch::Watcher>,
}

impl Room {
    /// Nothing is left to keep the room for. One that still holds watchers
    /// waits for `prune`, whose `drop_watchers` parks their sessions for Leave.
    fn vacant(&self) -> bool {
        self.peers.is_empty() && self.ordering.is_none() && self.watchers.is_empty()
    }
}

#[derive(Clone, Copy)]
struct Admission {
    generation: u64,
    room: [u8; 16],
    source: u8,
    retain_until: u64,
    handshakes: RateBudget,
    packets: RateBudget,
    archive_pulls: RateBudget,
    outgoing: RateBudget,
    order_packets: RateBudget,
}

pub struct ForwardDatagram {
    pub endpoint: SocketAddrV4,
    pub bytes: Vec<u8>,
}

/// How players kept pace and read the ordering, for the relay's journal
/// report. No player, room or endpoint identity.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OrderingCounts {
    /// Times the clock slowed a match because a player could not keep up.
    pub slowdowns: u64,
    /// Times the clock tried a faster rate again.
    pub raises: u64,
    /// Pace reports from a player catching up or behind. Each player reports
    /// once a second, so this is about the seconds players spent catching up.
    pub catching_up_s: u64,
    /// Pull requests answered: repairs and pull-only readers.
    pub pull_replies: u64,
    /// Matches this relay ended with a DESYNC record.
    pub desyncs: u64,
    /// Ended games whose receipt could not be saved or retained under the
    /// bounded handoff budget. The lobby cannot rate one lacking evidence.
    pub outcome_handoff_drops: u64,
    /// Startups this relay cancelled because a Ready, on it or on another
    /// relay of the match, proved a different initial world.
    pub start_mismatches: u64,
    /// Reader downlink (spec §10.2). Pushed datagrams and their bytes by
    /// send-log kind: 0 status, 1 new pack, 2 repeat, 3 retransmission,
    /// 4 tail probe.
    pub push_packets: [u64; 5],
    pub push_bytes: [u64; 5],
    /// Old records piggybacked in new packs.
    pub piggybacked_records: u64,
    /// Downlink datagrams judged lost.
    pub down_lost_packets: u64,
    /// Records retransmitted for them.
    pub retransmitted_records: u64,
    /// Subscribed readers seen by push passes in each state (by
    /// `LinkState::index`), and the passes: their ratio is the average
    /// number of readers in that state.
    pub reader_states: [u64; 4],
    pub reader_passes: u64,
    /// About one new pack per new stream head an offline reader missed.
    pub offline_unsent_packets: u64,
}

/// Bound storage and packet rate; untrusted data never creates a room until
/// both ticket authentication and UDP source reachability are established.
pub struct RelayServer {
    key: SecretKey,
    rooms: HashMap<[u8; 16], Room>,
    admissions: HashMap<[u8; 16], Admission>,
    max_rooms: usize,
    max_principals: usize,
    // Rooms share the relay-wide totals; handshakes, before any room, take
    // part as one more room, and so does Leave for a room no longer held.
    ingress: SharedBudget,
    handshake_ingress: SharedBudget,
    handshakes: RateBudget,
    // Handshake budgets of players without an admission. Only permitted
    // exchanges add an entry, and `prune` clears them every second.
    unadmitted_handshakes: HashMap<[u8; 16], RateBudget>,
    outgoing: SharedBudget,
    handshake_outgoing: SharedBudget,
    order_packets: SharedBudget,
    leaves: RateBudget,
    repeat_packets: RateBudget,
    last_pruned: u64,
    push_after: Option<([u8; 16], u8)>,
    journal_directory: Option<journal::JournalDirectory>,
    closed_startups: sequenced::ClosedStartups,
    // Since `take_ordering_counts` last read them.
    ordering_counts: OrderingCounts,
    // The ordering bytes the last pass over every room counted, plus what
    // was reserved or sealed since (`reserve_ordering`), so a reservation
    // needs no count of every room. Freed bytes leave it at the next pass:
    // it overcounts, and only an exact count refuses or evicts. Growth no
    // reservation covers, such as a federated frame, joins it at that pass.
    ordering_estimate: usize,
    // An operational reporter owns durable delivery of marked VO1F outcomes.
    federation_outcomes_enabled: bool,
    // A retired room needs only its unacknowledged receipt until the writer
    // can save it. At most one cohort of max_rooms, outside gameplay quotas.
    retired_outcomes: HashMap<[u8; 16], FederationOutcome>,
    // Every live or leaving hop by its session (spec §6.2).
    sessions: HashMap<u32, SessionSlot>,
    // Released sessions until they may be assigned again (spec §6.2).
    retired_sessions: HashMap<u32, u64>,
    // Since `take_session_alerts` last read them.
    session_alerts: u64,
    // What all watchers together may spend; players' budgets never pay
    // for watchers.
    watching: sequenced::watch::WatchBudgets,
    // Test support: the next session assigned (`preset_next_session`).
    #[cfg(any(test, feature = "test-support"))]
    next_session: Option<u32>,
}

impl RelayServer {
    pub fn new(key: SecretKey, max_rooms: usize) -> Self {
        Self {
            key,
            rooms: HashMap::new(),
            admissions: HashMap::new(),
            max_rooms,
            max_principals: max_rooms.saturating_mul(usize::from(MAX_PEERS)),
            ingress: SharedBudget::default(),
            handshake_ingress: SharedBudget::default(),
            handshakes: RateBudget::default(),
            unadmitted_handshakes: HashMap::new(),
            outgoing: SharedBudget::default(),
            handshake_outgoing: SharedBudget::default(),
            order_packets: SharedBudget::default(),
            leaves: RateBudget::default(),
            repeat_packets: RateBudget::default(),
            last_pruned: 0,
            push_after: None,
            journal_directory: None,
            closed_startups: Default::default(),
            ordering_counts: OrderingCounts::default(),
            ordering_estimate: 0,
            federation_outcomes_enabled: false,
            retired_outcomes: HashMap::new(),
            sessions: HashMap::new(),
            retired_sessions: HashMap::new(),
            session_alerts: 0,
            watching: Default::default(),
            #[cfg(any(test, feature = "test-support"))]
            next_session: None,
        }
    }

    /// Enable full-match replay on the existing relay's private
    /// StateDirectory. Existing room files are never overwritten or resumed;
    /// those of long-ended matches move to its `archive` subfolder.
    pub fn with_journal(
        key: SecretKey,
        max_rooms: usize,
        state_dir: &std::path::Path,
    ) -> std::io::Result<Self> {
        let mut server = Self::new(key, max_rooms);
        server.journal_directory = Some(journal::JournalDirectory::open(
            state_dir,
            std::time::SystemTime::now(),
        )?);
        Ok(server)
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    /// How players kept pace and read the ordering since the last call; the
    /// relay's journal reports it.
    pub fn take_ordering_counts(&mut self) -> OrderingCounts {
        std::mem::take(&mut self.ordering_counts)
    }

    pub fn process(
        &mut self,
        source: SocketAddrV4,
        bytes: &[u8],
        now: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        self.process_at(source, bytes, now, now.saturating_mul(1000))
    }

    pub fn process_at(
        &mut self,
        source: SocketAddrV4,
        bytes: &[u8],
        now: u64,
        now_millis: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        // Rooms reserve their fair parts of this second before any budget.
        self.prune(now, now_millis);
        // Relay-wide budgets are charged only after authentication and the
        // sender's own limits, so forged, junk or replayed datagrams cannot
        // spend capacity other players need. Rejecting one before any budget
        // costs at most three HMACs; a CONFIRM within the handshake budget then
        // checks its cookie and any federation roster. No source-address map
        // grows under spoofed UDP traffic.
        if sequenced::federation::is_link_packet(bytes) {
            return self.federation_receive(bytes, now, now_millis);
        }
        let header = envelope::peek(envelope::Direction::Up, bytes)?;
        let (message, leaving) = if matches!(header.kind, HELLO | CONFIRM) {
            (handshake_incoming(&self.key, header, bytes, now)?, false)
        } else {
            self.session_incoming(header, bytes, now)?
        };
        // Watchers never reach a player's path (README "Watchers").
        let watching = message.ticket.source == sequenced::watch::WATCH_SOURCE;
        if watching && matches!(message.kind, HELLO | CONFIRM) {
            return self.watch_handshake(source, &message, now, now_millis);
        }
        if message.kind == CONTROL && message.payload == [sequenced::LEAVE_REQUEST] {
            return if watching {
                self.watch_leave(source, &message, now)
            } else {
                self.ordering_leave(source, &message, now, now_millis)
            };
        }
        // A leaving hop accepts only LEAVE (spec §6.2 "释放").
        if leaving {
            return Err(RelayError::Unregistered);
        }
        if watching {
            return self.watch_datagram(source, &message, now, now_millis);
        }
        // Only the authenticated room needs a fresh boundary before a
        // submission. The socket loop advances every room independently.
        if message.kind == CONTROL {
            self.advance_ordering_room(message.ticket.room, now_millis);
        }
        if let Some(admission) = self.admissions.get(&message.ticket.player) {
            if message.ticket.generation < admission.generation
                || (message.ticket.generation == admission.generation
                    && (message.ticket.room != admission.room
                        || message.ticket.source != admission.source))
            {
                return Err(RelayError::Authentication);
            }
        }
        if message.kind == HELLO {
            self.allow_handshake(&message, now)?;
            return self.challenge_budgeted(source, &message, now);
        }
        if message.kind == CONFIRM {
            self.allow_handshake(&message, now)?;
            self.verify_cookie(source, &message, now)?;
            return self.confirm(source, &message, now, now_millis);
        }
        let Some(room) = self.rooms.get_mut(&message.ticket.room) else {
            return Err(RelayError::Unregistered);
        };
        let Some(index) = room
            .peers
            .iter()
            .position(|peer| peer.session == message.session)
        else {
            return Err(RelayError::Unregistered);
        };
        room.peers[index].link.heard(message.sequence, now_millis)?;
        // NAT rebinding: an authenticated, fresh datagram moves the hop
        // (spec §6.2 "换地址", §6.4 item 5).
        room.peers[index].endpoint = source;
        // Any datagram of the hop ends the reader's offline state (§5.1);
        // push still waits for its subscription.
        room.peers[index].evaluate_reader(now_millis);
        let admission = self
            .admissions
            .get_mut(&message.ticket.player)
            .ok_or(RelayError::Unregistered)?;
        if !admission.packets.permits(now, 1, PLAYER_PACKETS_PER_SECOND)
            || !self
                .ingress
                .admit_part(&mut room.ingress, now, 1, INGRESS_PER_SECOND)
        {
            return Err(RelayError::RateLimited);
        }
        admission.packets.charge(now, 1);
        self.ingress.charge_part(&mut room.ingress, 1);
        if let Some(ordering) = room.ordering.as_mut() {
            ordering.heard(
                message.ticket.source,
                (message.ticket.player, message.ticket.generation),
                now_millis,
            );
        }
        if message.kind == CONTROL {
            return self.ordering_request(&message, now, now_millis);
        }
        let recipients = if message.kind == PING {
            1
        } else {
            room.peers
                .iter()
                .filter(|peer| {
                    data_recipient(peer, message.ticket.source, message.target, now_millis)
                })
                .count()
        };
        let packet_bytes = envelope::OVERHEAD
            + if message.kind == PING {
                0
            } else {
                1 + message.payload.len()
            };
        // Players homed on other relays of a federated match get one copy
        // through each of their relays.
        let links = match (&room.federation, message.kind) {
            (Some(federation), DATA) => {
                federation.data_links(message.ticket.source, message.target)
            }
            _ => Vec::new(),
        };
        let cost = (recipients * packet_bytes
            + links.len() * sequenced::federation::Federation::data_len(message.payload.len()))
            as u64;
        // Reserve the full fan-out before any sequence mutation or delivery.
        // Renewals retain these principal budgets and cannot reset the rate.
        if !admission
            .outgoing
            .permits(now, cost, PLAYER_BYTES_PER_SECOND)
            || !room.outgoing.permits(now, cost, ROOM_BYTES_PER_SECOND)
            || !self
                .outgoing
                .admit_part(&mut room.outgoing, now, cost, TOTAL_BYTES_PER_SECOND)
        {
            return Err(RelayError::RateLimited);
        }
        admission.outgoing.charge(now, cost);
        self.outgoing.charge_part(&mut room.outgoing, cost);
        if message.kind == PING {
            return Ok(vec![server_packet(
                &mut room.peers[index],
                READY,
                message.ticket.source,
                &[],
                now_millis,
                link::SEND_OTHER,
            )?]);
        }
        let mut output = Vec::with_capacity(recipients);
        for peer in &mut room.peers {
            if data_recipient(peer, message.ticket.source, message.target, now_millis) {
                output.push(server_packet(
                    peer,
                    DATA,
                    message.ticket.source,
                    message.payload,
                    now_millis,
                    link::SEND_OTHER,
                )?);
            }
        }
        if let Some(federation) = room.federation.as_mut() {
            for position in links {
                output.push(federation.link_data(
                    position,
                    message.ticket.source,
                    message.target,
                    message.payload,
                )?);
            }
        }
        Ok(output)
    }

    fn prune(&mut self, now: u64, now_ms: u64) {
        if self.last_pruned == now {
            return;
        }
        // An expired hop, or a watcher that lost its place, leaves the room
        // but keeps its session for Leave (spec §6.2 "释放").
        let mut parked = Vec::new();
        self.rooms.retain(|_, room| {
            parked.extend(sequenced::watch::drop_watchers(room, now, now_ms));
            room.peers.retain_mut(|peer| {
                if peer.ticket.expires > now {
                    return true;
                }
                let until = peer
                    .left_until
                    .unwrap_or(u64::MAX)
                    .min(peer.ticket.expires.saturating_add(LEAVE_GRACE));
                parked.push((peer.session, peer.ticket, peer.key.clone(), until));
                false
            });
            !room.vacant()
        });
        for (session, ticket, key, until) in parked {
            self.park_session(session, ticket, key, until);
        }
        let released: Vec<u32> = self
            .sessions
            .iter()
            .filter(|(session, slot)| match &slot.hop {
                SessionHop::Leaving(hop) => hop.until <= now,
                SessionHop::Peer { room, source } => self
                    .rooms
                    .get(room)
                    .and_then(|room| room.peers.iter().find(|peer| peer.ticket.source == *source))
                    .is_none_or(|peer| peer.left_until.is_some_and(|until| until <= now)),
                SessionHop::Watcher { room } => self.rooms.get(room).is_none_or(|room| {
                    !room
                        .watchers
                        .iter()
                        .any(|watcher| watcher.session == **session)
                }),
            })
            .map(|(session, _)| *session)
            .collect();
        for session in released {
            self.release_session(session, now);
        }
        self.retired_sessions.retain(|_, until| *until > now);
        self.admissions
            .retain(|_, admission| admission.retain_until > now);
        self.closed_startups.prune(now);
        self.retired_outcomes
            .retain(|_, receipt| now <= receipt.expires_at());
        self.unadmitted_handshakes.clear();
        self.ingress.allocate(
            now,
            INGRESS_PER_SECOND,
            (self.rooms.values_mut().map(|room| &mut room.ingress))
                .chain([&mut self.handshake_ingress]),
        );
        self.outgoing.allocate(
            now,
            TOTAL_BYTES_PER_SECOND,
            (self.rooms.values_mut().map(|room| &mut room.outgoing))
                .chain([&mut self.handshake_outgoing]),
        );
        self.order_packets.allocate(
            now,
            INGRESS_PER_SECOND,
            self.rooms.values_mut().map(|room| &mut room.order_packets),
        );
        self.last_pruned = now;
    }

    /// A player's own handshake budget, admitted or not. It is checked before
    /// the relay-wide budgets: replaying one valid ticket must not spend the
    /// capacity other players need to join, renew or change address.
    fn player_handshakes(&self, player: &[u8; 16]) -> RateBudget {
        match self.admissions.get(player) {
            Some(admission) => admission.handshakes,
            None => self
                .unadmitted_handshakes
                .get(player)
                .copied()
                .unwrap_or_default(),
        }
    }

    fn charge_player_handshake(&mut self, player: [u8; 16], now: u64) {
        match self.admissions.get_mut(&player) {
            Some(admission) => admission.handshakes.charge(now, 1),
            None => self
                .unadmitted_handshakes
                .entry(player)
                .or_default()
                .charge(now, 1),
        }
    }

    fn allow_handshake(&mut self, message: &Incoming<'_>, now: u64) -> Result<(), RelayError> {
        let player = message.ticket.player;
        if !self
            .player_handshakes(&player)
            .permits(now, 1, PLAYER_HANDSHAKES_PER_SECOND)
            || !self.handshakes.permits(now, 1, HANDSHAKES_PER_SECOND)
            || !self
                .ingress
                .admit_part(&mut self.handshake_ingress, now, 1, INGRESS_PER_SECOND)
        {
            return Err(RelayError::RateLimited);
        }
        self.charge_player_handshake(player, now);
        self.handshakes.charge(now, 1);
        self.ingress.charge_part(&mut self.handshake_ingress, 1);
        Ok(())
    }

    fn challenge_budgeted(
        &mut self,
        source: SocketAddrV4,
        message: &Incoming<'_>,
        now: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        let cost = (envelope::OVERHEAD + COOKIE_LEN) as u64;
        if !self.outgoing.admit_part(
            &mut self.handshake_outgoing,
            now,
            cost,
            TOTAL_BYTES_PER_SECOND,
        ) {
            return Err(RelayError::RateLimited);
        }
        self.outgoing
            .charge_part(&mut self.handshake_outgoing, cost);
        Ok(vec![self.challenge(source, message, now)])
    }

    fn confirm(
        &mut self,
        source: SocketAddrV4,
        message: &Incoming<'_>,
        now: u64,
        now_ms: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        let prior = self.admissions.get(&message.ticket.player).copied();
        if prior.is_none() && self.admissions.len() >= self.max_principals {
            return Err(RelayError::Full);
        }
        let newer = prior.is_some_and(|old| message.ticket.generation > old.generation);
        let frees_room = newer
            && prior.is_some_and(|old| {
                old.room != message.ticket.room
                    && self.rooms.get(&old.room).is_some_and(|room| {
                        room.ordering.is_none()
                            && room.peers.len() == 1
                            && room.peers[0].ticket.player == message.ticket.player
                    })
            });
        if !self.rooms.contains_key(&message.ticket.room)
            && self.rooms.len() >= self.max_rooms
            && !frees_room
        {
            return Err(RelayError::Full);
        }
        if self.rooms.get(&message.ticket.room).is_some_and(|room| {
            room.peers.iter().any(|peer| {
                peer.ticket.source == message.ticket.source
                    && peer.ticket.player != message.ticket.player
            })
        }) {
            return Err(RelayError::Authentication);
        }
        // A federated room admits only its own roster's players homed here.
        let roster = &message.payload[COOKIE_LEN..];
        let existing = self.rooms.get(&message.ticket.room);
        let federation = match existing.and_then(|room| room.federation.as_ref()) {
            Some(federation) if federation.admits(roster, message.ticket.source) => None,
            Some(_) => return Err(RelayError::Authentication),
            None if roster.is_empty() => None,
            None if existing
                .is_some_and(|room| !room.peers.is_empty() || room.ordering.is_some()) =>
            {
                return Err(RelayError::Authentication);
            }
            None => Some(sequenced::federation::Federation::admit(
                &self.key,
                roster,
                &message.ticket,
                self.federation_outcomes_enabled,
            )?),
        };
        let cost = envelope::OVERHEAD as u64;
        if !self.outgoing.admit_part(
            &mut self.handshake_outgoing,
            now,
            cost,
            TOTAL_BYTES_PER_SECOND,
        ) {
            return Err(RelayError::RateLimited);
        }
        let reuses = self
            .rooms
            .get(&message.ticket.room)
            .is_some_and(|room| room.peers.iter().any(|peer| peer.ticket == message.ticket));
        // A new hop gets a new session (spec §6.2 "分配", "续期").
        let session = if reuses { 0 } else { self.allocate_session()? };
        if newer {
            let old = prior.expect("newer generation has an admission");
            if let Some(room) = self.rooms.get_mut(&old.room) {
                let mut released = Vec::new();
                room.peers.retain(|peer| {
                    let keep = peer.ticket.player != message.ticket.player;
                    if !keep {
                        released.push(peer.session);
                    }
                    keep
                });
                if room.vacant() {
                    self.rooms.remove(&old.room);
                }
                for old in released {
                    self.release_session(old, now);
                }
            }
        }
        let mut replaced = None;
        let room = self.rooms.entry(message.ticket.room).or_default();
        if federation.is_some() {
            room.federation = federation;
        }
        let reply = if let Some(peer) = room
            .peers
            .iter_mut()
            .find(|peer| peer.ticket.source == message.ticket.source)
        {
            if peer.ticket != message.ticket {
                if message.ticket.expires <= peer.ticket.expires {
                    return Err(RelayError::Authentication);
                }
                // A renewed hop keeps the measured loss (spec §5.4).
                let previous = std::mem::replace(peer, new_peer(message, session, source, now_ms));
                peer.link.carry_path(&previous.link);
                replaced = Some(previous);
            }
            if message.sequence <= peer.link.highest_received() && peer.endpoint != source {
                return Err(RelayError::Replay);
            }
            peer.link.heard(message.sequence, now_ms)?;
            peer.evaluate_reader(now_ms);
            if peer.endpoint != source {
                // A confirmed new path must opt in before receiving history.
                peer.push = None;
            }
            peer.endpoint = source;
            server_packet(
                peer,
                READY,
                message.ticket.source,
                &[],
                now_ms,
                link::SEND_OTHER,
            )?
        } else {
            let mut peer = new_peer(message, session, source, now_ms);
            peer.link.heard(message.sequence, now_ms)?;
            let reply = server_packet(
                &mut peer,
                READY,
                message.ticket.source,
                &[],
                now_ms,
                link::SEND_OTHER,
            )?;
            room.peers.push(peer);
            reply
        };
        if !reuses {
            self.sessions.insert(
                session,
                SessionSlot {
                    hop: SessionHop::Peer {
                        room: message.ticket.room,
                        source: message.ticket.source,
                    },
                    failure_window: 0,
                    failures: 0,
                },
            );
        }
        // The replaced hop keeps its session for LEAVE: a client that quits
        // before the new READY arrives leaves from it (spec §6.2 "续期").
        // The client moves to the new hop on that READY, so the session is
        // released a ticket lifetime after the old ticket expires, 300 to 600
        // seconds from now (`prune` already dropped an expired peer): time
        // for the client's minute of Leave retries.
        if let Some(old) = replaced {
            let until = old.ticket.expires.saturating_add(MAX_TICKET_LIFETIME);
            self.park_session(old.session, old.ticket, old.key, until);
        }
        let mut admission = prior.unwrap_or(Admission {
            generation: message.ticket.generation,
            room: message.ticket.room,
            source: message.ticket.source,
            retain_until: 0,
            // This Confirm and any earlier Hello this second are already
            // charged here; admission keeps that count.
            handshakes: self
                .unadmitted_handshakes
                .remove(&message.ticket.player)
                .unwrap_or_default(),
            packets: RateBudget::default(),
            archive_pulls: RateBudget::default(),
            outgoing: RateBudget::default(),
            order_packets: RateBudget::default(),
        });
        if prior.is_none() || newer {
            // Even an unseen older grant cannot outlive this tombstone. Its
            // signed maximum lifetime is measured at the time we see the newer
            // generation; no unbounded revoked-ticket set is needed.
            admission.retain_until = now.saturating_add(MAX_TICKET_LIFETIME);
        }
        admission.retain_until = admission.retain_until.max(message.ticket.expires);
        admission.generation = message.ticket.generation;
        admission.room = message.ticket.room;
        admission.source = message.ticket.source;
        self.admissions.insert(message.ticket.player, admission);
        self.outgoing
            .charge_part(&mut self.handshake_outgoing, cost);
        self.retire_superseded(message.ticket, now_ms, now);
        Ok(vec![reply])
    }

    /// CHALLENGE: session 0, the HELLO's sequence (spec §6.1).
    fn challenge(&self, source: SocketAddrV4, message: &Incoming<'_>, now: u64) -> ForwardDatagram {
        let expires = now.saturating_add(10);
        let mut cookie = expires.to_le_bytes().to_vec();
        let tag = self
            .cookie_mac(source, message.ticket_bytes.expect("a handshake"), expires)
            .finalize()
            .into_bytes();
        cookie.extend_from_slice(&tag);
        ForwardDatagram {
            endpoint: source,
            bytes: encode_server(&message.key, CHALLENGE, 0, message.sequence, &cookie),
        }
    }

    fn cookie_mac(
        &self,
        source: SocketAddrV4,
        ticket: &[u8; TICKET_LEN],
        expires: u64,
    ) -> HmacSha256 {
        let mut mac = self.key.mac(COOKIE_DOMAIN, ticket);
        mac.update(&source.ip().octets());
        mac.update(&source.port().to_le_bytes());
        mac.update(&expires.to_le_bytes());
        mac
    }

    fn verify_cookie(
        &self,
        source: SocketAddrV4,
        message: &Incoming<'_>,
        now: u64,
    ) -> Result<(), RelayError> {
        let expires = link::read_u64(message.payload, 0)?;
        if expires < now || expires > now.saturating_add(10) {
            return Err(RelayError::Expired);
        }
        self.cookie_mac(source, message.ticket_bytes.expect("a handshake"), expires)
            .verify_slice(&message.payload[8..COOKIE_LEN])
            .map_err(|_| RelayError::Authentication)
    }
}

fn new_peer(message: &Incoming<'_>, session: u32, endpoint: SocketAddrV4, now_ms: u64) -> Peer {
    Peer {
        ticket: message.ticket,
        key: message.key.clone(),
        session,
        endpoint,
        sent: 0,
        left_until: None,
        push: None,
        link: link::ReaderLink::new(now_ms),
    }
}

/// A player DATA packet goes to other addressed peers heard from in the last
/// 30 seconds, whether it arrived on a player hop or a federation link.
fn data_recipient(peer: &Peer, source: u8, target: u8, now_ms: u64) -> bool {
    peer.ticket.source != source
        && now_ms.saturating_sub(peer.link.last_heard_ms) <= 30_000
        && (target == BROADCAST || target == peer.ticket.source)
}

/// One datagram on `peer`'s hop, logged at `now_ms` as `send_kind`
/// (spec §4.2; `link::SEND_*`). DATA carries `source` first (compaction
/// spec §6.1). The relay never renews a hop: past `u32::MAX` it stops
/// sending on it (compaction spec §6.1).
fn server_packet(
    peer: &mut Peer,
    kind: u8,
    source: u8,
    payload: &[u8],
    now_ms: u64,
    send_kind: u8,
) -> Result<ForwardDatagram, RelayError> {
    let sequence = next_relay_sequence(peer.sent)?;
    let bytes = if kind == DATA {
        let mut data = Vec::with_capacity(1 + payload.len());
        data.push(source);
        data.extend_from_slice(payload);
        encode_server(&peer.key, kind, peer.session, sequence, &data)
    } else {
        encode_server(&peer.key, kind, peer.session, sequence, payload)
    };
    peer.sent = sequence;
    let pack =
        (kind == CONTROL && payload.first() == Some(&sequenced::PACK_EVENT)).then_some(payload);
    peer.link.record_send(peer.sent, pack, send_kind, now_ms);
    #[cfg(any(test, feature = "test-support"))]
    peer.link.count_send(send_kind, bytes.len());
    Ok(ForwardDatagram {
        endpoint: peer.endpoint,
        bytes,
    })
}

/// The relay's next sequence on a hop that has sent `sent`. The relay never
/// renews a hop: past `u32::MAX` it stops sending on it (compaction spec
/// §6.1).
fn next_relay_sequence(sent: u64) -> Result<u64, RelayError> {
    sent.checked_add(1)
        .filter(|sequence| *sequence <= u64::from(u32::MAX))
        .ok_or(RelayError::CounterExhausted)
}

/// An envelope v3 relay datagram (spec §6.1). Callers bound `payload` to
/// `envelope::MAX_PAYLOAD` and `sequence` to `u32`.
fn encode_server(
    key: &SecretKey,
    kind: u8,
    session: u32,
    sequence: u64,
    payload: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(envelope::OVERHEAD + payload.len());
    envelope::seal(
        envelope::Direction::Down,
        key,
        envelope::Header {
            kind,
            session,
            sequence: sequence as u32,
        },
        payload,
        &mut bytes,
    )
    .expect("a relay kind within the envelope limit");
    bytes
}

impl RelayServer {
    /// Authenticates a datagram on a session (spec §6.2, §6.4): an unknown
    /// session is dropped without an HMAC; a known one's tag is verified
    /// under its hop key, a failure dropping only this datagram. Returns the
    /// message and whether its hop is leaving (accepts only LEAVE). A
    /// watcher's message carries its hop's ticket, `source` `WATCH_SOURCE`.
    fn session_incoming<'a>(
        &mut self,
        header: envelope::Header,
        bytes: &'a [u8],
        now: u64,
    ) -> Result<(Incoming<'a>, bool), RelayError> {
        let slot = self
            .sessions
            .get(&header.session)
            .ok_or(RelayError::Unregistered)?;
        let (ticket, key, leaving) = match &slot.hop {
            SessionHop::Peer { room, source } => {
                let peer = self
                    .rooms
                    .get(room)
                    .and_then(|room| room.peers.iter().find(|peer| peer.ticket.source == *source))
                    .filter(|peer| peer.session == header.session)
                    .ok_or(RelayError::Unregistered)?;
                (peer.ticket, peer.key.clone(), peer.left_until.is_some())
            }
            SessionHop::Leaving(hop) => (hop.ticket, hop.key.clone(), true),
            SessionHop::Watcher { room } => {
                let watcher = self
                    .rooms
                    .get(room)
                    .and_then(|room| {
                        room.watchers
                            .iter()
                            .find(|watcher| watcher.session == header.session)
                    })
                    .ok_or(RelayError::Unregistered)?;
                (watcher.ticket, watcher.key.clone(), false)
            }
        };
        let payload = match envelope::open(envelope::Direction::Up, &key, bytes) {
            Ok((_, payload)) => payload,
            Err(RelayError::Authentication) => {
                self.count_session_failure(header.session, now);
                return Err(RelayError::Authentication);
            }
            Err(error) => return Err(error),
        };
        let (target, payload) = session_payload(header.kind, payload)?;
        if header.sequence == 0 {
            return Err(RelayError::Replay);
        }
        Ok((
            Incoming {
                ticket,
                ticket_bytes: None,
                key,
                kind: header.kind,
                session: header.session,
                sequence: u64::from(header.sequence),
                target,
                payload,
            },
            leaving,
        ))
    }

    /// A wrong tag on a known session counts toward its alert (§6.4 item 2).
    fn count_session_failure(&mut self, session: u32, now: u64) {
        let Some(slot) = self.sessions.get_mut(&session) else {
            return;
        };
        let window = now / SESSION_ALERT_WINDOW;
        if slot.failure_window != window {
            slot.failure_window = window;
            slot.failures = 0;
        }
        slot.failures += 1;
        if slot.failures == SESSION_ALERT_FAILURES + 1 {
            self.session_alerts += 1;
        }
    }

    /// A random non-zero session neither held nor retired (spec §6.2).
    fn allocate_session(&mut self) -> Result<u32, RelayError> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(session) = self.next_session.take() {
            return Ok(session);
        }
        loop {
            let mut bytes = [0; 4];
            getrandom::fill(&mut bytes).map_err(|_| RelayError::EntropyUnavailable)?;
            let session = u32::from_le_bytes(bytes);
            if session != 0
                && !self.sessions.contains_key(&session)
                && !self.retired_sessions.contains_key(&session)
            {
                return Ok(session);
            }
        }
    }

    /// Parks `session`: until `until`, unix seconds, it takes only LEAVE,
    /// each answered again, under `ticket` and `key`; then `prune` releases
    /// it (spec §6.2 "释放").
    fn park_session(&mut self, session: u32, ticket: Ticket, key: SecretKey, until: u64) {
        if let Some(slot) = self.sessions.get_mut(&session) {
            slot.hop = SessionHop::Leaving(Box::new(LeavingHop { ticket, key, until }));
        }
    }

    /// Drops `session`; it is not assigned again for 10 minutes (§6.2).
    fn release_session(&mut self, session: u32, now: u64) {
        if self.sessions.remove(&session).is_some() {
            self.retired_sessions
                .insert(session, now.saturating_add(SESSION_QUARANTINE));
        }
    }

    /// A LEAVE on `session` puts a live hop into its leaving state
    /// (spec §6.2 "释放").
    fn leave_session(&mut self, session: u32, now: u64) {
        let Some(SessionSlot {
            hop: SessionHop::Peer { room, source },
            ..
        }) = self.sessions.get(&session)
        else {
            return;
        };
        if let Some(peer) = self
            .rooms
            .get_mut(room)
            .and_then(|room| {
                room.peers
                    .iter_mut()
                    .find(|peer| peer.ticket.source == *source)
            })
            .filter(|peer| peer.session == session)
        {
            peer.left_until
                .get_or_insert(now.saturating_add(LEAVE_GRACE));
        }
    }
}

impl ClientSession {
    /// The session the relay assigned this hop in the envelope of the READY
    /// answering its CONFIRM (compaction spec §6.1, §6.2), which every later
    /// datagram of the hop carries both ways; `None` before that READY. The
    /// client claims that READY by its tag under this hop's key, so of a
    /// renewal's two handshaking hops the one whose key verifies takes it.
    /// A renewal is a new `ClientSession` with its own session (§6.2), so
    /// the old and new hops tell their datagrams apart by it
    /// (`envelope::peek`).
    pub fn session(&self) -> Option<u32> {
        self.session
    }

    /// Relay datagrams on this hop's session dropped for a wrong tag
    /// (§6.4 item 6: the client does not rate-limit, it only counts).
    pub fn mac_failures(&self) -> u64 {
        self.mac_failures
    }
}

impl RelayServer {
    /// Sessions that failed over 4096 tags within one fixed minute since
    /// the last call (compaction spec §6.4 item 2). The dropped datagrams
    /// themselves come back from `process_at` as errors, which the relay's
    /// journal counts: `Authentication` for a wrong tag, `Unregistered` for
    /// an unknown session.
    pub fn take_session_alerts(&mut self) -> u64 {
        std::mem::take(&mut self.session_alerts)
    }
}

/// One hop in the relay's session table (compaction spec §6.2), for tests.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionEntry {
    /// Non-zero, unique on this relay.
    pub session: u32,
    pub room: [u8; 16],
    pub source: u8,
    /// The hop's current endpoint: the address of its last authenticated
    /// fresh datagram (§6.2, NAT rebinding). `None` once its ticket expired,
    /// a renewal replaced it or, for a watcher, it left or lost its place
    /// (the hop then only answers LEAVE, to the LEAVE's address).
    pub endpoint: Option<SocketAddrV4>,
    /// The hop's ticket expiry, unix seconds like `process`'s `now`.
    pub expires: u64,
    /// Whether the hop accepts only LEAVE: after its LEAVE, its ticket's
    /// expiry or its renewal (§6.2 "续期", "释放").
    pub leaving: bool,
    /// The sequence of the relay's next datagram on this hop; `None` once
    /// its ticket expired or a renewal replaced it.
    pub next_sequence: Option<u64>,
}

#[cfg(any(test, feature = "test-support"))]
impl RelayServer {
    /// Test support only: every hop in the session table (§6.2), ordered by
    /// session. A hop is in it from its accepted CONFIRM until
    /// `LEAVE_GRACE` seconds after its LEAVE or its ticket's expiry, or, once
    /// a renewal's CONFIRM replaced it, until `MAX_TICKET_LIFETIME` seconds
    /// after its ticket's expiry (§6.2). A watcher's goes `WATCH_LEAVE_S`
    /// seconds after its LEAVE or renewal, or `WATCH_DROPPED_LEAVE_S` after
    /// `drop_watchers` took it out of its room.
    pub fn session_table(&self) -> Vec<SessionEntry> {
        let mut table: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(session, slot)| match &slot.hop {
                SessionHop::Peer { room, source } => {
                    let peer = self
                        .rooms
                        .get(room)?
                        .peers
                        .iter()
                        .find(|peer| peer.ticket.source == *source)?;
                    Some(SessionEntry {
                        session: *session,
                        room: *room,
                        source: *source,
                        endpoint: Some(peer.endpoint),
                        expires: peer.ticket.expires,
                        leaving: peer.left_until.is_some(),
                        next_sequence: Some(peer.sent + 1),
                    })
                }
                SessionHop::Leaving(hop) => Some(SessionEntry {
                    session: *session,
                    room: hop.ticket.room,
                    source: hop.ticket.source,
                    endpoint: None,
                    expires: hop.ticket.expires,
                    leaving: true,
                    next_sequence: None,
                }),
                SessionHop::Watcher { room } => {
                    let watcher = self
                        .rooms
                        .get(room)?
                        .watchers
                        .iter()
                        .find(|watcher| watcher.session == *session)?;
                    Some(SessionEntry {
                        session: *session,
                        room: *room,
                        source: watcher.ticket.source,
                        endpoint: Some(watcher.endpoint),
                        expires: watcher.ticket.expires,
                        leaving: false,
                        next_sequence: Some(watcher.sent + 1),
                    })
                }
            })
            .collect();
        table.sort_unstable_by_key(|entry| entry.session);
        table
    }

    /// Test support only: `room`'s canonical records in sequence order,
    /// from the oldest the relay keeps (1 with a journal): the journal's,
    /// then those in memory (compaction spec §12.2). Empty when the relay
    /// holds no ordering for `room`.
    pub fn ordering_history(&mut self, room: [u8; 16]) -> Vec<Vec<u8>> {
        self.rooms
            .get_mut(&room)
            .and_then(|room| room.ordering.as_mut())
            .map_or_else(Vec::new, |ordering| ordering.full_history())
    }

    /// Test support only: until when a released `session` may not be
    /// assigned again, unix seconds: its release `now` + 600 (§6.2);
    /// `None` when it is held or that time has passed.
    pub fn session_retired_until(&self, session: u32) -> Option<u64> {
        self.retired_sessions.get(&session).copied()
    }

    /// Test support only: the next session this relay assigns is `session`
    /// instead of a random draw, so tests and fixed byte samples know it in
    /// advance (§6.2, §12.3). Panics when `session` is 0, held or retired.
    pub fn preset_next_session(&mut self, session: u32) {
        assert!(
            session != 0
                && !self.sessions.contains_key(&session)
                && !self.retired_sessions.contains_key(&session),
            "session {session:#x} is not free"
        );
        self.next_session = Some(session);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(key: &SecretKey, room: u8, source: u8) -> ClientSession {
        client_as(key, room, source, [source + 1; 16])
    }

    fn client_as(key: &SecretKey, room: u8, source: u8, player: [u8; 16]) -> ClientSession {
        let ticket = Ticket {
            room: [room; 16],
            source,
            expires: 1200,
            nonce: [source + 1; 16],
            player,
            generation: 1,
        }
        .sign(key)
        .unwrap();
        let packet_key = derive_packet_key(key, &ticket);
        ClientSession::new(&ticket, *packet_key.expose_for_https(), [89; 32]).unwrap()
    }

    fn connect(server: &mut RelayServer, client: &mut ClientSession, endpoint: SocketAddrV4) {
        let mut bytes = Vec::new();
        client.probe(&mut bytes).unwrap();
        let challenge = server.process(endpoint, &bytes, 1000).unwrap().remove(0);
        let ServerEvent::Challenge(cookie) = client.receive(&challenge.bytes).unwrap() else {
            panic!("challenge required");
        };
        client.confirm(cookie, &mut bytes).unwrap();
        let ready = server.process(endpoint, &bytes, 1000).unwrap().remove(0);
        assert!(matches!(
            client.receive(&ready.bytes),
            Ok(ServerEvent::Ready)
        ));
        assert!(client.is_ready());
    }

    #[test]
    fn downlink_feedback_reports_64_sequences_but_rejects_replays_across_128() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut client = client(&key, 1, 0);
        connect(
            &mut server,
            &mut client,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30001),
        );
        let session = client.session().unwrap();
        let packet_key = client.key.clone();
        let packet = |sequence| encode_server(&packet_key, DATA, session, sequence, &[1, 42]);

        client.receive(&packet(200)).unwrap();
        assert_eq!(client.link_stats().down, RecvWindow { high: 200, bits: 1 });
        // Fresh deliveries 1 and 63 behind appear in feedback. At ages 64
        // and 127 they still reach the game exactly once, outside feedback.
        for sequence in [199, 137, 136, 73] {
            let bytes = packet(sequence);
            assert!(matches!(
                client.receive(&bytes),
                Ok(ServerEvent::Payload { .. })
            ));
            assert_eq!(client.receive(&bytes).unwrap_err(), RelayError::Replay);
        }
        assert_eq!(client.receive(&packet(72)).unwrap_err(), RelayError::Replay);
        let stats = client.link_stats();
        assert_eq!(
            stats.down,
            RecvWindow {
                high: 200,
                bits: (1 << 63) | 3
            }
        );
        assert_eq!(stats.datagrams_received, 6); // READY and five DATA
    }

    #[test]
    fn authenticated_round_trip_preserves_payload_and_rejects_replay_forgery_and_other_room() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let mut first = client(&key, 1, 0);
        let mut second = client(&key, 1, 29);
        let mut outsider = client(&key, 2, 1);
        let a = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30001);
        let b = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30002);
        connect(&mut server, &mut first, a);
        connect(&mut server, &mut second, b);
        connect(
            &mut server,
            &mut outsider,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30003),
        );
        let mut bytes = Vec::new();
        first
            .data(BROADCAST, b"unchanged game datagram", &mut bytes)
            .unwrap();
        let forwarded = server.process(a, &bytes, 1000).unwrap();
        assert_eq!(
            forwarded.len(),
            1,
            "broadcast is scoped to the authenticated match"
        );
        assert_eq!(forwarded[0].endpoint, b);
        assert!(matches!(
            second.receive(&forwarded[0].bytes),
            Ok(ServerEvent::Payload {
                source: 0,
                bytes
            }) if bytes == b"unchanged game datagram"
        ));
        assert!(matches!(
            second.receive(&forwarded[0].bytes),
            Err(RelayError::Replay)
        ));
        assert!(matches!(
            server.process(a, &bytes, 1000),
            Err(RelayError::Replay)
        ));
        bytes[HEADER_LEN] ^= 1;
        assert!(matches!(
            server.process(a, &bytes, 1000),
            Err(RelayError::Authentication)
        ));
        // Past its ticket the hop only takes LEAVE (compaction spec §6.2).
        first.data(BROADCAST, b"late", &mut bytes).unwrap();
        assert!(matches!(
            server.process(a, &bytes, 2000),
            Err(RelayError::Unregistered)
        ));
    }

    #[test]
    fn a_data_recipient_is_still_reachable_after_thirty_seconds() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut sender = client(&key, 1, 0);
        let mut recipient = client(&key, 1, 1);
        let from = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30001);
        let to = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30002);
        connect(&mut server, &mut sender, from);
        connect(&mut server, &mut recipient, to);

        let mut bytes = Vec::new();
        sender.data(1, b"ping", &mut bytes).unwrap();
        let at_limit = server.process(from, &bytes, 1030).unwrap();
        assert_eq!(at_limit.len(), 1);
        assert_eq!(at_limit[0].endpoint, to);
        sender.data(1, b"ping", &mut bytes).unwrap();
        assert!(server.process(from, &bytes, 1031).unwrap().is_empty());
    }

    #[test]
    fn a_wall_clock_step_does_not_silence_a_data_recipient() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut sender = client(&key, 1, 0);
        let mut recipient = client(&key, 1, 1);
        let from = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30001);
        let to = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30002);
        connect(&mut server, &mut sender, from);
        connect(&mut server, &mut recipient, to);

        // One second after both were heard, though the wall clock stepped
        // forty seconds ahead.
        let mut bytes = Vec::new();
        sender.data(1, b"ping", &mut bytes).unwrap();
        let output = server.process_at(from, &bytes, 1040, 1_001_000).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].endpoint, to);
    }

    #[test]
    fn a_hop_is_busy_only_while_traffic_other_than_its_keepalive_goes_both_ways() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut hop = client(&key, 1, 0);
        let mut peer = client(&key, 1, 1);
        let a = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30001);
        let b = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 30002);
        connect(&mut server, &mut hop, a);
        connect(&mut server, &mut peer, b);
        // A PING and the READY answering it are not traffic, or one
        // keepalive exchange would stand in for the next.
        let mut bytes = Vec::new();
        hop.data_at(1, b"to the peer", 10, &mut bytes).unwrap();
        hop.probe_at(20, &mut bytes).unwrap();
        let ready = server.process(a, &bytes, 1000).unwrap().remove(0);
        assert!(matches!(
            hop.receive_at(&ready.bytes, 30),
            Ok(ServerEvent::Ready)
        ));
        assert!(!hop.busy_both_ways(40), "only READY heard");
        peer.data(0, b"to the hop", &mut bytes).unwrap();
        let forwarded = server.process(b, &bytes, 1000).unwrap().remove(0);
        hop.receive_at(&forwarded.bytes, 3_000).unwrap();
        hop.probe_at(3_000, &mut bytes).unwrap();
        assert!(!hop.busy_both_ways(3_001), "only a PING sent since 10 ms");
        // Traffic both ways, until either has been quiet for the interval.
        hop.data_at(1, b"to the peer", 3_001, &mut bytes).unwrap();
        assert!(hop.busy_both_ways(4_999));
        assert!(!hop.busy_both_ways(5_000), "nothing heard since 3000 ms");
    }

    #[test]
    fn a_fresh_authenticated_datagram_moves_the_hop_and_a_replay_or_forgery_does_not() {
        // Compaction spec §6.2 "换地址", §6.4 items 2 and 5.
        let key = SecretKey::new([91; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut first = client(&key, 1, 0);
        let mut second = client(&key, 1, 1);
        let old = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31001);
        let new = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31002);
        let attacker = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31003);
        connect(&mut server, &mut first, old);
        connect(
            &mut server,
            &mut second,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31004),
        );
        let session = first.session().unwrap();
        let endpoint = |server: &RelayServer| {
            server
                .session_table()
                .into_iter()
                .find(|entry| entry.session == session)
                .unwrap()
                .endpoint
        };
        let mut sent = Vec::new();
        first.data(1, b"from old", &mut sent).unwrap();
        assert_eq!(server.process(old, &sent, 1000).unwrap().len(), 1);
        // A replay from elsewhere, and a forgery, change nothing.
        assert_eq!(
            server.process(attacker, &sent, 1000).err(),
            Some(RelayError::Replay)
        );
        let mut forged = Vec::new();
        first.data(1, b"forged", &mut forged).unwrap();
        *forged.last_mut().unwrap() ^= 1;
        assert_eq!(
            server.process(attacker, &forged, 1000).err(),
            Some(RelayError::Authentication)
        );
        assert_eq!(endpoint(&server), Some(old));
        // The client's next datagram from its new address moves the hop.
        first.data(1, b"from new", &mut sent).unwrap();
        assert_eq!(server.process(new, &sent, 1000).unwrap().len(), 1);
        assert_eq!(endpoint(&server), Some(new));
        second.data(0, b"reply", &mut sent).unwrap();
        assert_eq!(
            server
                .process(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31004), &sent, 1000)
                .unwrap()[0]
                .endpoint,
            new
        );
        assert!(!format!("{first:?} {key:?}").contains("91, 91"));
    }

    #[test]
    fn sessions_are_assigned_by_confirm_and_released_into_a_quarantine() {
        // Compaction spec §6.1 READY row, §6.2 "分配", "续期", "释放".
        let key = SecretKey::new([92; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut first = client(&key, 1, 0);
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31101);
        server.preset_next_session(0x0102_0304);
        let mut bytes = Vec::new();
        first.probe(&mut bytes).unwrap();
        let challenge = server.process(endpoint, &bytes, 1000).unwrap().remove(0);
        assert_eq!(challenge.bytes[1..5], [0; 4], "CHALLENGE has no session");
        let ServerEvent::Challenge(cookie) = first.receive(&challenge.bytes).unwrap() else {
            panic!("challenge");
        };
        first.confirm(cookie, &mut bytes).unwrap();
        assert_eq!(bytes[1..5], [0; 4], "CONFIRM has no session");
        let ready = server.process(endpoint, &bytes, 1000).unwrap().remove(0);
        assert_eq!(
            envelope::peek(envelope::Direction::Down, &ready.bytes),
            Ok(envelope::Header {
                kind: READY,
                session: 0x0102_0304,
                sequence: 1,
            })
        );
        assert_eq!(first.session(), None);
        first.receive(&ready.bytes).unwrap();
        assert_eq!(first.session(), Some(0x0102_0304));
        // A renewal is a new hop with a new session; the old one retires.
        let ticket = Ticket {
            expires: 1250,
            nonce: [33; 16],
            ..first.ticket
        }
        .sign(&key)
        .unwrap();
        let mut renewed = first
            .renewal(
                &ticket,
                *derive_packet_key(&key, &ticket).expose_for_https(),
                [89; 32],
            )
            .unwrap();
        connect(&mut server, &mut renewed, endpoint);
        // The renewal takes the hop over; the old session takes only LEAVE
        // until a ticket lifetime after its ticket expired (1200), then
        // retires (spec §6.2 "续期", "释放").
        let table = server.session_table();
        assert_eq!(table.len(), 2);
        let old = table
            .iter()
            .find(|entry| entry.session == 0x0102_0304)
            .unwrap();
        assert!(old.leaving && old.endpoint.is_none());
        first.data(BROADCAST, b"old hop", &mut bytes).unwrap();
        assert_eq!(
            server.process(endpoint, &bytes, 1000).err(),
            Some(RelayError::Unregistered)
        );
        let released = 1200 + MAX_TICKET_LIFETIME;
        server.process(endpoint, &bytes, released - 1).ok();
        assert!(
            server
                .session_table()
                .iter()
                .any(|entry| entry.session == 0x0102_0304 && entry.leaving)
        );
        assert_eq!(
            server.process(endpoint, &bytes, released).err(),
            Some(RelayError::Unregistered)
        );
        assert_eq!(
            server.session_retired_until(0x0102_0304),
            Some(released + 600)
        );
    }

    #[test]
    fn the_hop_counter_ends_at_u32_max() {
        // Compaction spec §6.1, §11 case 7.
        let key = SecretKey::new([93; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut first = client(&key, 1, 0);
        connect(
            &mut server,
            &mut first,
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31201),
        );
        // The next send is the hop's last sequence.
        first.sent = u64::from(u32::MAX) - 1;
        let mut bytes = Vec::new();
        first.data(BROADCAST, b"last", &mut bytes).unwrap();
        assert_eq!(
            first.data(BROADCAST, b"past", &mut bytes),
            Err(RelayError::CounterExhausted)
        );
    }

    #[test]
    fn handshake_replays_and_confirmations_share_bounded_budgets() {
        let key = SecretKey::new([71; 32]);
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32001);
        let mut first = client(&key, 1, 0);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut bytes = Vec::new();
        // Replays of one never-confirmed ticket stop at its player's budget
        // and leave the relay's handshakes to everyone else.
        first.probe(&mut bytes).unwrap();
        for _ in 0..PLAYER_HANDSHAKES_PER_SECOND {
            assert!(server.process(endpoint, &bytes, 1000).is_ok());
        }
        assert!(matches!(
            server.process(endpoint, &bytes, 1000),
            Err(RelayError::RateLimited)
        ));
        assert_eq!(server.room_count(), 0);
        connect(
            &mut server,
            &mut client(&key, 1, 1),
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32002),
        );

        // Distinct players still share the relay-wide budget, which outlasts
        // the lobby's 320 sessions and an hour's 640 new guests.
        let mut server = RelayServer::new(key.clone(), 1);
        let players = HANDSHAKES_PER_SECOND / PLAYER_HANDSHAKES_PER_SECOND;
        assert!(players >= 320 + 640);
        for player in 1..=players + 1 {
            let mut id = [0; 16];
            id[..8].copy_from_slice(&player.to_le_bytes());
            client_as(&key, 1, 0, id).probe(&mut bytes).unwrap();
            for _ in 0..PLAYER_HANDSHAKES_PER_SECOND {
                assert_eq!(
                    server.process(endpoint, &bytes, 1000).err(),
                    (player > players).then_some(RelayError::RateLimited)
                );
            }
        }

        let mut server = RelayServer::new(key, 1);
        connect(&mut server, &mut first, endpoint);
        first.forget_readiness();
        first.probe(&mut bytes).unwrap();
        let challenge = server.process(endpoint, &bytes, 1000).unwrap().remove(0);
        let ServerEvent::Challenge(cookie) = first.receive(&challenge.bytes).unwrap() else {
            panic!("challenge");
        };
        // The connecting Hello and Confirm count too: three of eight are spent.
        for _ in 0..5 {
            first.confirm(cookie, &mut bytes).unwrap();
            assert!(server.process(endpoint, &bytes, 1000).is_ok());
        }
        first.confirm(cookie, &mut bytes).unwrap();
        assert!(matches!(
            server.process(endpoint, &bytes, 1000),
            Err(RelayError::RateLimited)
        ));
    }

    #[test]
    fn forged_and_replayed_datagrams_cannot_spend_relay_input_budget() {
        let key = SecretKey::new([74; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut sender = client(&key, 1, 0);
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32200);
        connect(&mut server, &mut sender, endpoint);
        let mut accepted = Vec::new();
        sender.data(BROADCAST, b"input", &mut accepted).unwrap();
        assert!(server.process(endpoint, &accepted, 1000).is_ok());
        let mut forged = accepted.clone();
        *forged.last_mut().unwrap() ^= 1;
        for attempt in 0..=INGRESS_PER_SECOND {
            let (packet, error) = if attempt % 2 == 0 {
                (&forged, RelayError::Authentication)
            } else {
                (&accepted, RelayError::Replay)
            };
            assert_eq!(server.process(endpoint, packet, 1000).err(), Some(error));
        }
        let mut fresh = Vec::new();
        sender.data(BROADCAST, b"input", &mut fresh).unwrap();
        assert!(server.process(endpoint, &fresh, 1000).is_ok());
    }

    #[test]
    fn outbound_budget_charges_complete_fanout_before_delivering() {
        let key = SecretKey::new([72; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut sender = client(&key, 1, 0);
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32100);
        connect(&mut server, &mut sender, endpoint);
        for source in [1, 2] {
            connect(
                &mut server,
                &mut client(&key, 1, source),
                SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32100 + u16::from(source)),
            );
        }
        let mut total = 0;
        let mut bytes = Vec::new();
        let mut limited = false;
        for _ in 0..2048 {
            sender
                .data(BROADCAST, &[7; MAX_PAYLOAD], &mut bytes)
                .unwrap();
            match server.process(endpoint, &bytes, 1000) {
                Ok(output) => {
                    assert_eq!(output.len(), 2);
                    total += output
                        .iter()
                        .map(|packet| packet.bytes.len() as u64)
                        .sum::<u64>();
                }
                Err(RelayError::RateLimited) => {
                    limited = true;
                    break;
                }
                _ => panic!("unexpected relay result"),
            }
        }
        assert!(limited);
        assert!(total <= PLAYER_BYTES_PER_SECOND);
        sender.data(BROADCAST, b"next second", &mut bytes).unwrap();
        assert_eq!(server.process(endpoint, &bytes, 1001).unwrap().len(), 2);
    }

    #[test]
    fn new_generation_retires_old_room_without_reopening_captured_grants() {
        let key = SecretKey::new([74; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32200);
        let mut first = client(&key, 1, 0);
        connect(&mut server, &mut first, endpoint);
        let mut old_packet = Vec::new();
        first
            .data(BROADCAST, b"old match", &mut old_packet)
            .unwrap();
        let ticket = Ticket {
            room: [2; 16],
            generation: 2,
            expires: 1250,
            nonce: [22; 16],
            ..first.ticket
        }
        .sign(&key)
        .unwrap();
        let mut next = ClientSession::new(
            &ticket,
            *derive_packet_key(&key, &ticket).expose_for_https(),
            [90; 32],
        )
        .unwrap();
        connect(&mut server, &mut next, endpoint);
        assert_eq!(server.room_count(), 1);
        assert!(!server.rooms.contains_key(&[1; 16]));
        assert!(matches!(
            server.process(endpoint, &old_packet, 1000),
            Err(RelayError::Unregistered)
        ));
        assert_eq!(server.admissions.len(), 1);
        assert_eq!(server.admissions[&first.ticket.player].generation, 2);
    }

    #[test]
    fn renewals_keep_the_identity_and_ignore_the_payload_key() {
        let key = SecretKey::new([75; 32]);
        let first = client(&key, 1, 0);
        let renewed = |ticket: Ticket| {
            let signed = ticket.sign(&key).unwrap();
            let packet_key = *derive_packet_key(&key, &signed).expose_for_https();
            first.renewal(&signed, packet_key, [88; 32])
        };
        let next = Ticket {
            expires: 1250,
            nonce: [23; 16],
            ..first.ticket
        };
        assert!(renewed(next).is_ok(), "relayed traffic is plaintext");
        assert!(renewed(Ticket { source: 1, ..next }).is_err());
        assert!(
            renewed(Ticket {
                expires: 1200,
                ..next
            })
            .is_err()
        );
    }
}
