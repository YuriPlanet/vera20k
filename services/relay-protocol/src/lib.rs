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
pub use link::{ClientLinkStats, HopFeedback, LinkState, OrderingGap, ReaderLinkStats, RecvWindow};
pub use sequenced::federation::{MAX_FEDERATED_RELAYS, sign_federation_roster};
pub use sequenced::pack::{decode_pack_v2, encode_pack_v2};
pub use sequenced::{
    CHECKSUM_INTERVAL, CertificatePacing, ORDERING_BATCH_CAPACITY, ORDERING_CERTIFICATE_LEN,
    ORDERING_CERTIFICATE_V2_LEN, ORDERING_PULL_LIMIT, OrderEntry, OrderError, OrderEvent,
    OrderPace, OrderStatus, OrderingCertificate, is_ordering_certificate_len, paced_rates,
    turn_frames, turn_ms,
};

type HmacSha256 = Hmac<Sha256>;

pub const MAX_PEERS: u8 = 30;
pub const BROADCAST: u8 = 255;
/// The largest DATA message a client relays.
pub const MAX_PAYLOAD: usize = 640;
pub const TICKET_LEN: usize = 98;
/// The packing limit of PACK and UPLINK payloads, unchanged by the
/// compaction (bandwidth-compaction spec §5.4).
const MAX_WIRE_PAYLOAD: usize = envelope::PACK_LIMIT;
pub const MAX_DATAGRAM: usize = envelope::MAX_DATAGRAM;
pub const MAX_TICKET_LIFETIME: u64 = 300;
const TICKET_BODY_LEN: usize = 66;
/// Hop envelope bytes before the payload, either direction (§6.1).
const CLIENT_HEADER: usize = envelope::HEADER_LEN;
#[cfg(test)]
const SERVER_HEADER: usize = envelope::HEADER_LEN;
use envelope::HOP_TAG_LEN;
/// A full HMAC-SHA256: ticket signatures and federation links.
const MAC_LEN: usize = 32;
const COOKIE_LEN: usize = 40;
const TICKET_DOMAIN: &[u8] = b"VERA20k relay ticket v2\0";
const KEY_DOMAIN: &[u8] = b"VERA20k relay packet key v2\0";
const COOKIE_DOMAIN: &[u8] = b"VERA20k relay source v2\0";
const INGRESS_PER_SECOND: u64 = 16_384;
const HANDSHAKES_PER_SECOND: u64 = 512;
const PLAYER_HANDSHAKES_PER_SECOND: u64 = 8;
const PLAYER_BYTES_PER_SECOND: u64 = 2 * 1024 * 1024;
const ROOM_BYTES_PER_SECOND: u64 = 64 * 1024 * 1024;
const TOTAL_BYTES_PER_SECOND: u64 = 64 * 1024 * 1024;
const LEAVES_PER_SECOND: u64 = 512;
const LEAVE_GRACE: u64 = sequenced::ROOM_LIFETIME_MS.div_ceil(1000) + MAX_TICKET_LIFETIME;
const HELLO: u8 = 0;
const CONFIRM: u8 = 1;
const DATA: u8 = 2;
const PING: u8 = 3;
const CONTROL: u8 = 4;
const LEAVE_ACK: u8 = 5;
const CHALLENGE: u8 = 0;
const READY: u8 = 1;

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
        if bytes.len() != TICKET_LEN || bytes[0] != 2 || bytes[17] >= MAX_PEERS {
            return Err(RelayError::Malformed);
        }
        let ticket = Self {
            room: bytes[1..17].try_into().expect("checked ticket length"),
            source: bytes[17],
            expires: u64::from_le_bytes(bytes[18..26].try_into().expect("checked ticket length")),
            nonce: bytes[26..42].try_into().expect("checked ticket length"),
            player: bytes[42..58].try_into().expect("checked ticket length"),
            generation: u64::from_le_bytes(
                bytes[58..66].try_into().expect("checked ticket length"),
            ),
        };
        if ticket.generation == 0 || ticket.player == [0; 16] {
            return Err(RelayError::Malformed);
        }
        Ok(ticket)
    }

    /// Cloudflare and native test clients use this same fixed byte layout.
    /// The random nonce and room identifier must come from OS cryptography.
    pub fn sign(self, server_key: &SecretKey) -> Result<[u8; TICKET_LEN], RelayError> {
        if self.source >= MAX_PEERS || self.generation == 0 || self.player == [0; 16] {
            return Err(RelayError::Malformed);
        }
        let mut bytes = [0; TICKET_LEN];
        bytes[0] = 2;
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
    fn accept(&mut self, sequence: u64) -> Result<(), RelayError> {
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
            return Ok(());
        }
        let behind = self.maximum - sequence;
        if behind >= 128 || self.seen & (1u128 << behind) != 0 {
            return Err(RelayError::Replay);
        }
        self.seen |= 1u128 << behind;
        Ok(())
    }
}

pub struct ClientSession {
    ticket_bytes: [u8; TICKET_LEN],
    ticket: Ticket,
    key: SecretKey,
    sent: u64,
    received: ReplayWindow,
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
    /// needs no change.
    pub fn new(
        ticket: &[u8],
        packet_key: [u8; 32],
        _payload_key: [u8; 32],
    ) -> Result<Self, RelayError> {
        Ok(Self {
            ticket: Ticket::parse(ticket)?,
            ticket_bytes: ticket.try_into().map_err(|_| RelayError::Malformed)?,
            key: SecretKey::new(packet_key),
            sent: 0,
            received: ReplayWindow::default(),
            probe: None,
            ready: false,
            session: None,
            mac_failures: 0,
            ordering: None,
            federation: None,
            hop: link::HopLink::default(),
            clock_ms: 0,
            #[cfg(any(test, feature = "test-support"))]
            measured_good: false,
        })
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
        if next.ticket.room != self.ticket.room
            || next.ticket.source != self.ticket.source
            || next.ticket.player != self.ticket.player
            || next.ticket.generation != self.ticket.generation
            || next.ticket.nonce == self.ticket.nonce
            || next.ticket.expires <= self.ticket.expires
        {
            return Err(RelayError::Authentication);
        }
        next.ordering = self.ordering.clone();
        next.federation = self.federation.clone();
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
        if !self.ready {
            return Err(RelayError::Unregistered);
        }
        if payload.is_empty()
            || payload.len() > MAX_PAYLOAD
            || (target >= MAX_PEERS && target != BROADCAST)
        {
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
            self.hop_sent(self.sent);
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
                self.received.accept(sequence)?;
                self.hop_heard(sequence);
                if claim {
                    self.session = Some(header.session);
                }
                self.ready = true;
                Ok(ServerEvent::Ready)
            }
            CONTROL if self.ready => {
                self.received.accept(sequence)?;
                self.hop_heard(sequence);
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
                self.received.accept(sequence)?;
                self.hop_heard(sequence);
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
/// the hop key derived from it before the envelope tag.
fn handshake_incoming<'a>(
    key: &SecretKey,
    header: envelope::Header,
    bytes: &'a [u8],
    now: u64,
) -> Result<Incoming<'a>, RelayError> {
    let body = &bytes[CLIENT_HEADER..bytes.len() - HOP_TAG_LEN];
    if header.session != 0 || body.len() < TICKET_LEN {
        return Err(RelayError::Malformed);
    }
    let ticket_bytes: &[u8; TICKET_LEN] = body[..TICKET_LEN].try_into().expect("checked length");
    let payload = &body[TICKET_LEN..];
    if (header.kind == HELLO && !payload.is_empty())
        || (header.kind == CONFIRM
            && payload.len() != COOKIE_LEN
            && !(payload.len() > COOKIE_LEN
                && sequenced::federation::is_roster_len(payload.len() - COOKIE_LEN)))
    {
        return Err(RelayError::Malformed);
    }
    let ticket = Ticket::parse(ticket_bytes)?;
    key.mac(TICKET_DOMAIN, &ticket_bytes[..TICKET_BODY_LEN])
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
/// CONTROL non-empty, DATA `[target][1..=MAX_PAYLOAD bytes]`.
fn session_payload(kind: u8, payload: &[u8]) -> Result<(u8, &[u8]), RelayError> {
    match kind {
        PING if payload.is_empty() => Ok((BROADCAST, payload)),
        CONTROL if !payload.is_empty() => Ok((BROADCAST, payload)),
        DATA if payload.len() > 1
            && payload.len() <= 1 + MAX_PAYLOAD
            && (payload[0] < MAX_PEERS || payload[0] == BROADCAST) =>
        {
            Ok((payload[0], &payload[1..]))
        }
        _ => Err(RelayError::Malformed),
    }
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
    received: ReplayWindow,
    sent: u64,
    last_seen: u64,
    // After its LEAVE the hop accepts only LEAVE, until this second
    // (spec §6.2 "释放").
    left_until: Option<u64>,
    push: Option<sequenced::PushSubscription>,
    link: link::ReaderLink,
}

/// A hop whose ticket expired: it accepts only LEAVE until `until`, unix
/// seconds (spec §6.2 "释放").
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
    // Only an explicit signed Leave before the first ordering Ready sets this.
    // Prevent a delayed Ready from recreating an already canceled startup.
    ordering_cancelled: bool,
}

#[derive(Clone, Copy)]
struct Admission {
    generation: u64,
    room: [u8; 16],
    source: u8,
    retain_until: u64,
    handshakes: RateBudget,
    packets: RateBudget,
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

/// Envelope v3 datagrams the relay dropped for their session or tag
/// (compaction spec §6.4), since `take_mac_failure_counts` last read them.
/// Statistics only: no count ever drops a datagram. No player, room or
/// endpoint identity.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct MacFailureCounts {
    /// Datagrams dropped for a wrong tag (§6.3), HELLO and CONFIRM
    /// included, plus `unknown_sessions`.
    pub failures: u64,
    /// Of `failures`, datagrams with an unknown session, or session 0 on a
    /// kind other than HELLO and CONFIRM: no key to verify, dropped without
    /// an HMAC (§6.4 item 1).
    pub unknown_sessions: u64,
    /// Alerts: a session failed more than 4096 times within one fixed
    /// 60-second window of `now` (§6.4 item 2). Each window alerts once.
    pub session_alerts: u64,
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
    // Ordering rooms cleaned up before their certificate expired, until that
    // expiry: a late Ready gets Inactive instead of a new timeline.
    ended_orderings: HashMap<[u8; 16], u64>,
    // Since `take_ordering_counts` last read them.
    ordering_counts: OrderingCounts,
    // Every live or leaving hop by its session (spec §6.2).
    sessions: HashMap<u32, SessionSlot>,
    // Released sessions until they may be assigned again (spec §6.2).
    retired_sessions: HashMap<u32, u64>,
    // Since `take_mac_failure_counts` last read them.
    mac_counts: MacFailureCounts,
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
            ended_orderings: HashMap::new(),
            ordering_counts: OrderingCounts::default(),
            sessions: HashMap::new(),
            retired_sessions: HashMap::new(),
            mac_counts: MacFailureCounts::default(),
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
        self.prune(now);
        // Relay-wide budgets are charged only after authentication and the
        // sender's own limits, so forged, junk or replayed datagrams cannot
        // spend capacity other players need. Rejecting one costs at most
        // three HMACs. No source-address map grows under spoofed UDP traffic.
        if bytes.starts_with(sequenced::federation::LINK_MAGIC) {
            return self.federation_receive(bytes, now, now_millis);
        }
        let header = envelope::peek(envelope::Direction::Up, bytes)?;
        let (message, leaving) = if matches!(header.kind, HELLO | CONFIRM) {
            match handshake_incoming(&self.key, header, bytes, now) {
                Err(RelayError::Authentication) => {
                    self.mac_counts.failures += 1;
                    return Err(RelayError::Authentication);
                }
                message => (message?, false),
            }
        } else {
            self.session_incoming(header, bytes, now)?
        };
        if message.kind == CONTROL && message.payload == [sequenced::LEAVE_REQUEST] {
            return self.ordering_leave(source, &message, now, now_millis);
        }
        // A leaving hop accepts only LEAVE (spec §6.2 "释放").
        if leaving {
            return Err(RelayError::Unregistered);
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
        room.peers[index].received.accept(message.sequence)?;
        // NAT rebinding: an authenticated, fresh datagram moves the hop
        // (spec §6.2 "换地址", §6.4 item 5).
        room.peers[index].endpoint = source;
        room.peers[index].link.heard(message.sequence, now_millis);
        // Any datagram of the hop ends the reader's offline state (§5.1);
        // push still waits for its subscription.
        room.peers[index].evaluate_reader(now_millis);
        let admission = self
            .admissions
            .get_mut(&message.ticket.player)
            .ok_or(RelayError::Unregistered)?;
        if !admission.packets.permits(now, 1, 2048)
            || !self
                .ingress
                .admit_part(&mut room.ingress, now, 1, INGRESS_PER_SECOND)
        {
            return Err(RelayError::RateLimited);
        }
        admission.packets.charge(now, 1);
        self.ingress.charge_part(&mut room.ingress, 1);
        room.peers[index].last_seen = now;
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
                .filter(|peer| data_recipient(peer, &message, now))
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
            if data_recipient(peer, &message, now) {
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

    fn prune(&mut self, now: u64) {
        if self.last_pruned == now {
            return;
        }
        // An expired hop leaves the room but keeps its session for Leave
        // (spec §6.2 "释放").
        let mut expired = Vec::new();
        self.rooms.retain(|_, room| {
            room.peers.retain_mut(|peer| {
                if peer.ticket.expires > now {
                    return true;
                }
                let until = peer
                    .left_until
                    .unwrap_or(u64::MAX)
                    .min(peer.ticket.expires.saturating_add(LEAVE_GRACE));
                expired.push((
                    peer.session,
                    LeavingHop {
                        ticket: peer.ticket,
                        key: peer.key.clone(),
                        until,
                    },
                ));
                false
            });
            !room.peers.is_empty() || room.ordering.is_some()
        });
        for (session, hop) in expired {
            if let Some(slot) = self.sessions.get_mut(&session) {
                slot.hop = SessionHop::Leaving(Box::new(hop));
            }
        }
        let released: Vec<u32> = self
            .sessions
            .iter()
            .filter(|(_, slot)| match &slot.hop {
                SessionHop::Leaving(hop) => hop.until <= now,
                SessionHop::Peer { room, source } => self
                    .rooms
                    .get(room)
                    .and_then(|room| room.peers.iter().find(|peer| peer.ticket.source == *source))
                    .is_none_or(|peer| peer.left_until.is_some_and(|until| until <= now)),
            })
            .map(|(session, _)| *session)
            .collect();
        for session in released {
            self.release_session(session, now);
        }
        self.retired_sessions.retain(|_, until| *until > now);
        self.admissions
            .retain(|_, admission| admission.retain_until > now);
        self.ended_orderings.retain(|_, expires| *expires > now);
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
                if room.peers.is_empty() && room.ordering.is_none() {
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
                let previous =
                    std::mem::replace(peer, new_peer(message, session, source, now, now_ms));
                peer.link.carry_path(&previous.link);
                replaced = Some(previous);
            }
            if message.sequence <= peer.received.maximum && peer.endpoint != source {
                return Err(RelayError::Replay);
            }
            peer.received.accept(message.sequence)?;
            peer.link.heard(message.sequence, now_ms);
            peer.evaluate_reader(now_ms);
            if peer.endpoint != source {
                // A confirmed new path must opt in before receiving history.
                peer.push = None;
            }
            peer.endpoint = source;
            peer.last_seen = now;
            server_packet(
                peer,
                READY,
                message.ticket.source,
                &[],
                now_ms,
                link::SEND_OTHER,
            )?
        } else {
            let mut peer = new_peer(message, session, source, now, now_ms);
            peer.received.accept(message.sequence)?;
            peer.link.heard(message.sequence, now_ms);
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
        if let Some(old) = replaced
            && let Some(slot) = self.sessions.get_mut(&old.session)
        {
            slot.hop = SessionHop::Leaving(Box::new(LeavingHop {
                ticket: old.ticket,
                key: old.key.clone(),
                until: now.saturating_add(LEAVE_GRACE),
            }));
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
        let expires = u64::from_le_bytes(
            message.payload[..8]
                .try_into()
                .expect("checked confirm payload"),
        );
        if expires < now || expires > now.saturating_add(10) {
            return Err(RelayError::Expired);
        }
        self.cookie_mac(source, message.ticket_bytes.expect("a handshake"), expires)
            .verify_slice(&message.payload[8..COOKIE_LEN])
            .map_err(|_| RelayError::Authentication)
    }
}

fn new_peer(
    message: &Incoming<'_>,
    session: u32,
    endpoint: SocketAddrV4,
    now: u64,
    now_ms: u64,
) -> Peer {
    Peer {
        ticket: message.ticket,
        key: message.key.clone(),
        session,
        endpoint,
        received: ReplayWindow::default(),
        sent: 0,
        last_seen: now,
        left_until: None,
        push: None,
        link: link::ReaderLink::new(now_ms),
    }
}

/// Whether `peer` gets `message`'s DATA: another recently heard source it
/// addresses.
fn data_recipient(peer: &Peer, message: &Incoming<'_>, now: u64) -> bool {
    peer.ticket.source != message.ticket.source
        && now.saturating_sub(peer.last_seen) <= 30
        && (message.target == BROADCAST || message.target == peer.ticket.source)
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
    let sequence = peer
        .sent
        .checked_add(1)
        .filter(|sequence| *sequence <= u64::from(u32::MAX))
        .ok_or(RelayError::CounterExhausted)?;
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
    peer.link
        .record_send(peer.sent, pack, send_kind, bytes.len(), now_ms);
    Ok(ForwardDatagram {
        endpoint: peer.endpoint,
        bytes,
    })
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
    /// message and whether its hop is leaving (accepts only LEAVE).
    fn session_incoming<'a>(
        &mut self,
        header: envelope::Header,
        bytes: &'a [u8],
        now: u64,
    ) -> Result<(Incoming<'a>, bool), RelayError> {
        let Some(slot) = self.sessions.get(&header.session) else {
            self.mac_counts.failures += 1;
            self.mac_counts.unknown_sessions += 1;
            return Err(RelayError::Unregistered);
        };
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

    fn count_session_failure(&mut self, session: u32, now: u64) {
        self.mac_counts.failures += 1;
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
            self.mac_counts.session_alerts += 1;
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

    /// Whether this hop's next sequence has reached
    /// `envelope::RENEWAL_SEQUENCE` (2³² − 2¹⁶), so the client should renew
    /// it now (§11 case 7), whatever its ticket's remaining lifetime. The
    /// hop keeps sending until `u32::MAX`; past that every send is
    /// `RelayError::CounterExhausted`.
    pub fn sequence_wants_renewal(&self) -> bool {
        self.sent + 1 >= u64::from(envelope::RENEWAL_SEQUENCE)
    }
}

impl RelayServer {
    /// Datagrams dropped for their session or tag since the last call
    /// (compaction spec §6.4); the relay's journal reports them like
    /// `take_ordering_counts`.
    pub fn take_mac_failure_counts(&mut self) -> MacFailureCounts {
        std::mem::take(&mut self.mac_counts)
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
    /// fresh datagram (§6.2, NAT rebinding). `None` once its ticket expired
    /// or a renewal replaced it (the hop then only answers LEAVE, to the
    /// LEAVE's address).
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
impl ClientSession {
    /// Test support only: the next datagram this hop sends carries
    /// sequence `next` (1..=`u32::MAX`), as if the hop had already sent
    /// `next - 1`, so tests reach `envelope::RENEWAL_SEQUENCE` and
    /// `CounterExhausted` (§6.1, §11 case 7) without sending billions. The
    /// skipped sequences count as never sent for the hop's link
    /// measurements. Use it between sends.
    pub fn preset_next_sequence(&mut self, next: u32) {
        assert!(next > 0, "hop sequences start at 1");
        self.sent = u64::from(next) - 1;
    }
}

#[cfg(any(test, feature = "test-support"))]
impl RelayServer {
    /// Test support only: every hop in the session table (§6.2), ordered by
    /// session. A hop is in it from its accepted CONFIRM until
    /// `LEAVE_GRACE` seconds after its LEAVE, its ticket's expiry or the
    /// accepted CONFIRM of its renewal (§6.2).
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

    /// Test support only: the relay's next datagram on `session`'s hop
    /// carries sequence `next` (1..=`u32::MAX`), so tests reach the end of
    /// its counter (§6.1, §11 case 7). Past `u32::MAX` the relay stops
    /// sending on that hop (§6.1). Returns false when no live hop holds
    /// `session`.
    pub fn preset_relay_sequence(&mut self, session: u32, next: u32) -> bool {
        assert!(next > 0, "hop sequences start at 1");
        let Some(SessionSlot {
            hop: SessionHop::Peer { room, source },
            ..
        }) = self.sessions.get(&session)
        else {
            return false;
        };
        let Some(peer) = self.rooms.get_mut(room).and_then(|room| {
            room.peers
                .iter_mut()
                .find(|peer| peer.ticket.source == *source)
        }) else {
            return false;
        };
        peer.sent = u64::from(next) - 1;
        true
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
        bytes[CLIENT_HEADER] ^= 1;
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
        assert_eq!(server.take_mac_failure_counts().failures, 1);
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
        // until its grace ends, then retires (spec §6.2 "续期", "释放").
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
        let released = 1000 + LEAVE_GRACE;
        server.process(endpoint, &bytes, released).ok();
        assert_eq!(
            server.session_retired_until(0x0102_0304),
            Some(released + 600)
        );
        assert_eq!(server.take_mac_failure_counts().unknown_sessions, 1);
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
        assert!(!first.sequence_wants_renewal());
        first.preset_next_sequence(envelope::RENEWAL_SEQUENCE);
        assert!(first.sequence_wants_renewal());
        first.preset_next_sequence(u32::MAX);
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

        // Distinct players still share the relay-wide budget.
        let mut server = RelayServer::new(key.clone(), 1);
        let players = HANDSHAKES_PER_SECOND / PLAYER_HANDSHAKES_PER_SECOND;
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
