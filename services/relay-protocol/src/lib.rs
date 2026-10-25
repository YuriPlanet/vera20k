//! Authenticated datagram routing and fixed-clock opaque command ordering for
//! VERA clients. This transport owns no simulation state or payload keys.
//!
//! The HTTPS lobby signs a 98-byte v2 ticket and separately returns its derived
//! packet key and a match-private payload key. The relay never receives the
//! latter: gameplay, including chat, is end-to-end encrypted. Domain-separated
//! HMAC-SHA256 uses RustCrypto's constant-time verification:
//! https://docs.rs/hmac/0.12.1/hmac/ . Neither key type prints secret bytes.

use std::collections::HashMap;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddrV4};

use chacha20poly1305::{
    Tag, XChaCha20Poly1305, XNonce,
    aead::{AeadInOut, KeyInit},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use zeroize::Zeroize;

mod journal;
mod sequenced;
pub use sequenced::{
    ORDERING_BATCH_CAPACITY, ORDERING_CERTIFICATE_LEN, ORDERING_PULL_LIMIT, OrderEntry, OrderError,
    OrderEvent, OrderStatus, OrderingCertificate,
};

type HmacSha256 = Hmac<Sha256>;

pub const MAX_PEERS: u8 = 30;
pub const BROADCAST: u8 = 255;
pub const MAX_PAYLOAD: usize = 640;
pub const TICKET_LEN: usize = 98;
const PAYLOAD_HEADER: usize = 25;
const PAYLOAD_TAG: usize = 16;
const MAX_WIRE_PAYLOAD: usize = MAX_PAYLOAD + PAYLOAD_HEADER + PAYLOAD_TAG;
pub const MAX_DATAGRAM: usize = 146 + MAX_WIRE_PAYLOAD;
pub const MAX_TICKET_LIFETIME: u64 = 300;
const TICKET_BODY_LEN: usize = 66;
const CLIENT_HEADER: usize = 114;
const SERVER_HEADER: usize = 48;
const MAC_LEN: usize = 32;
const COOKIE_LEN: usize = 40;
const CLIENT_MAGIC: &[u8; 4] = b"VR2C";
const SERVER_MAGIC: &[u8; 4] = b"VR2S";
const TICKET_DOMAIN: &[u8] = b"VERA20k relay ticket v2\0";
const KEY_DOMAIN: &[u8] = b"VERA20k relay packet key v2\0";
const CLIENT_DOMAIN: &[u8] = b"VERA20k relay client v2\0";
const SERVER_DOMAIN: &[u8] = b"VERA20k relay server v2\0";
const COOKIE_DOMAIN: &[u8] = b"VERA20k relay source v2\0";
const PAYLOAD_KEY_DOMAIN: &[u8] = b"VERA20k relay payload key v2\0";
const KEY_CHECK_DOMAIN: &[u8] = b"VERA20k relay key equality v2\0";
const INGRESS_PER_SECOND: u64 = 16_384;
const HANDSHAKES_PER_SECOND: u64 = 512;
const PLAYER_HANDSHAKES_PER_SECOND: u64 = 8;
const PLAYER_BYTES_PER_SECOND: u64 = 2 * 1024 * 1024;
const ROOM_BYTES_PER_SECOND: u64 = 64 * 1024 * 1024;
const TOTAL_BYTES_PER_SECOND: u64 = 64 * 1024 * 1024;
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

    fn same_key(&self, other: &Self) -> bool {
        self.mac(KEY_CHECK_DOMAIN, &[])
            .verify_slice(&other.mac(KEY_CHECK_DOMAIN, &[]).finalize().into_bytes())
            .is_ok()
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

/// Fixed-capacity plaintext returned only after both authentication layers pass.
pub struct DecryptedPayload {
    bytes: [u8; MAX_PAYLOAD],
    length: usize,
}

impl DecryptedPayload {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}

impl fmt::Debug for DecryptedPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DecryptedPayload")
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}

fn payload_cipher(key: &SecretKey, room: &[u8; 16], source: u8) -> XChaCha20Poly1305 {
    // A distinct source key prevents cross-player nonce collisions. The full
    // entropy match key stays only in HTTPS recipients, never RelayServer.
    let mut mac = key.mac(PAYLOAD_KEY_DOMAIN, room);
    mac.update(&[source]);
    let mut derived: [u8; 32] = mac.finalize().into_bytes().into();
    let cipher = XChaCha20Poly1305::new(&derived.into());
    derived.zeroize();
    cipher
}

fn payload_aad(room: &[u8; 16], source: u8, target: u8) -> [u8; 20] {
    let mut aad = [0; 20];
    aad[..2].copy_from_slice(&[2, DATA]);
    aad[2..18].copy_from_slice(room);
    aad[18] = source;
    aad[19] = target;
    aad
}

pub struct ClientSession {
    ticket_bytes: [u8; TICKET_LEN],
    ticket: Ticket,
    key: SecretKey,
    payload_key: SecretKey,
    payload_prefix: [u8; 16],
    sent: u64,
    received: ReplayWindow,
    probe: Option<u64>,
    ready: bool,
    ordering: Option<sequenced::ClientOrdering>,
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
    Payload { source: u8, bytes: DecryptedPayload },
    Ordering(OrderEvent),
}

impl ClientSession {
    pub fn new(
        ticket: &[u8],
        packet_key: [u8; 32],
        payload_key: [u8; 32],
    ) -> Result<Self, RelayError> {
        let mut payload_prefix = [0; 16];
        getrandom::fill(&mut payload_prefix).map_err(|_| RelayError::EntropyUnavailable)?;
        Ok(Self {
            ticket: Ticket::parse(ticket)?,
            ticket_bytes: ticket.try_into().map_err(|_| RelayError::Malformed)?,
            key: SecretKey::new(packet_key),
            payload_key: SecretKey::new(payload_key),
            payload_prefix,
            sent: 0,
            received: ReplayWindow::default(),
            probe: None,
            ready: false,
            ordering: None,
        })
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
    pub fn renewal(
        &self,
        ticket: &[u8],
        packet_key: [u8; 32],
        payload_key: [u8; 32],
    ) -> Result<Self, RelayError> {
        let mut next = Self::new(ticket, packet_key, payload_key)?;
        if next.ticket.room != self.ticket.room
            || next.ticket.source != self.ticket.source
            || next.ticket.player != self.ticket.player
            || next.ticket.generation != self.ticket.generation
            || !self.payload_key.same_key(&next.payload_key)
            || next.ticket.nonce == self.ticket.nonce
            || next.ticket.expires <= self.ticket.expires
        {
            return Err(RelayError::Authentication);
        }
        next.ordering = self.ordering.clone();
        Ok(next)
    }
    pub fn forget_readiness(&mut self) {
        self.ready = false;
    }

    pub fn probe(&mut self, out: &mut Vec<u8>) -> Result<(), RelayError> {
        self.encode(if self.ready { PING } else { HELLO }, BROADCAST, &[], out)?;
        self.probe = Some(self.sent);
        Ok(())
    }

    pub fn confirm(&mut self, cookie: &[u8], out: &mut Vec<u8>) -> Result<(), RelayError> {
        if cookie.len() != COOKIE_LEN {
            return Err(RelayError::Malformed);
        }
        self.encode(CONFIRM, BROADCAST, cookie, out)
    }

    pub fn data(
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
        let sequence = self
            .sent
            .checked_add(1)
            .ok_or(RelayError::CounterExhausted)?;
        let mut nonce = [0; 24];
        nonce[..16].copy_from_slice(&self.payload_prefix);
        nonce[16..].copy_from_slice(&sequence.to_le_bytes());
        let mut wire = [0; MAX_WIRE_PAYLOAD];
        wire[0] = target;
        wire[1..PAYLOAD_HEADER].copy_from_slice(&nonce);
        let end = PAYLOAD_HEADER + payload.len();
        wire[PAYLOAD_HEADER..end].copy_from_slice(payload);
        // RustCrypto 0.11 AeadInOut; fixed buffers, 24-byte XChaCha nonce and
        // 16-byte tag. https://docs.rs/chacha20poly1305/0.11.0/chacha20poly1305/
        let tag = payload_cipher(&self.payload_key, &self.ticket.room, self.ticket.source)
            .encrypt_inout_detached(
                &XNonce::from(nonce),
                &payload_aad(&self.ticket.room, self.ticket.source, target),
                (&mut wire[PAYLOAD_HEADER..end]).into(),
            )
            .map_err(|_| RelayError::Authentication)?;
        wire[end..end + PAYLOAD_TAG].copy_from_slice(&tag);
        self.encode(DATA, target, &wire[..end + PAYLOAD_TAG], out)
    }

    fn encode(
        &mut self,
        kind: u8,
        target: u8,
        payload: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        if payload.len() > MAX_WIRE_PAYLOAD || (target >= MAX_PEERS && target != BROADCAST) {
            return Err(RelayError::Malformed);
        }
        self.sent = self
            .sent
            .checked_add(1)
            .ok_or(RelayError::CounterExhausted)?;
        out.clear();
        out.extend_from_slice(CLIENT_MAGIC);
        out.push(kind);
        out.extend_from_slice(&self.ticket_bytes);
        out.extend_from_slice(&self.sent.to_le_bytes());
        out.push(target);
        out.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        out.extend_from_slice(payload);
        let tag = self.key.mac(CLIENT_DOMAIN, out).finalize().into_bytes();
        out.extend_from_slice(&tag);
        Ok(())
    }

    pub fn receive<'a>(&mut self, bytes: &'a [u8]) -> Result<ServerEvent<'a>, RelayError> {
        if bytes.len() < SERVER_HEADER + MAC_LEN || &bytes[..4] != SERVER_MAGIC {
            return Err(RelayError::Malformed);
        }
        let length = u16::from_le_bytes([bytes[46], bytes[47]]) as usize;
        if length > MAX_WIRE_PAYLOAD || bytes.len() != SERVER_HEADER + length + MAC_LEN {
            return Err(RelayError::Malformed);
        }
        self.key
            .mac(SERVER_DOMAIN, &bytes[..bytes.len() - MAC_LEN])
            .verify_slice(&bytes[bytes.len() - MAC_LEN..])
            .map_err(|_| RelayError::Authentication)?;
        if bytes[5..21] != self.ticket.room
            || bytes[21..37] != self.ticket.nonce
            || bytes[45] >= MAX_PEERS
        {
            return Err(RelayError::Authentication);
        }
        let sequence = u64::from_le_bytes(bytes[37..45].try_into().expect("checked server header"));
        let payload = &bytes[SERVER_HEADER..SERVER_HEADER + length];
        match bytes[4] {
            CHALLENGE if length == COOKIE_LEN && self.probe == Some(sequence) => {
                self.probe = None;
                self.ready = false;
                Ok(ServerEvent::Challenge(payload))
            }
            READY if payload.is_empty() && bytes[45] == self.ticket.source => {
                self.received.accept(sequence)?;
                self.ready = true;
                Ok(ServerEvent::Ready)
            }
            CONTROL if self.ready && bytes[45] == self.ticket.source => {
                // Hop authentication protects the ordering authority; batch
                // contents additionally authenticate under the match-only key.
                self.received.accept(sequence)?;
                Ok(ServerEvent::Ordering(self.receive_ordering(payload)?))
            }
            LEAVE_ACK
                if payload.is_empty()
                    && bytes[45] == self.ticket.source
                    && self.accepts_ordering_leave_ack(sequence) =>
            {
                // This terminal response echoes the exact cached request,
                // like Challenge. It survives expiry of the peer's independent
                // server sequence and cannot admit or resume any game traffic.
                Ok(ServerEvent::Ordering(OrderEvent::Left))
            }
            DATA if payload.len() > PAYLOAD_HEADER + PAYLOAD_TAG && self.ready => {
                let target = payload[0];
                if target != BROADCAST && target != self.ticket.source {
                    return Err(RelayError::Authentication);
                }
                let nonce: [u8; 24] = payload[1..PAYLOAD_HEADER]
                    .try_into()
                    .expect("checked encrypted payload");
                let length = payload.len() - PAYLOAD_HEADER - PAYLOAD_TAG;
                let mut plaintext = DecryptedPayload {
                    bytes: [0; MAX_PAYLOAD],
                    length,
                };
                plaintext.bytes[..length]
                    .copy_from_slice(&payload[PAYLOAD_HEADER..PAYLOAD_HEADER + length]);
                let tag: [u8; 16] = payload[PAYLOAD_HEADER + length..]
                    .try_into()
                    .expect("checked encrypted tag");
                payload_cipher(&self.payload_key, &self.ticket.room, bytes[45])
                    .decrypt_inout_detached(
                        &XNonce::from(nonce),
                        &payload_aad(&self.ticket.room, bytes[45], target),
                        (&mut plaintext.bytes[..length]).into(),
                        &Tag::from(tag),
                    )
                    .map_err(|_| RelayError::Authentication)?;
                self.received.accept(sequence)?;
                Ok(ServerEvent::Payload {
                    source: bytes[45],
                    bytes: plaintext,
                })
            }
            _ => Err(RelayError::Malformed),
        }
    }
}

struct Incoming<'a> {
    ticket: Ticket,
    ticket_bytes: &'a [u8; TICKET_LEN],
    key: SecretKey,
    kind: u8,
    sequence: u64,
    target: u8,
    payload: &'a [u8],
}

fn incoming<'a>(key: &SecretKey, bytes: &'a [u8], now: u64) -> Result<Incoming<'a>, RelayError> {
    if bytes.len() < CLIENT_HEADER + MAC_LEN || &bytes[..4] != CLIENT_MAGIC {
        return Err(RelayError::Malformed);
    }
    let length = u16::from_le_bytes([bytes[CLIENT_HEADER - 2], bytes[CLIENT_HEADER - 1]]) as usize;
    if length > MAX_WIRE_PAYLOAD || bytes.len() != CLIENT_HEADER + length + MAC_LEN {
        return Err(RelayError::Malformed);
    }
    let ticket_bytes: &[u8; TICKET_LEN] = bytes[5..5 + TICKET_LEN]
        .try_into()
        .expect("checked client header");
    let ticket = Ticket::parse(ticket_bytes)?;
    key.mac(TICKET_DOMAIN, &ticket_bytes[..TICKET_BODY_LEN])
        .verify_slice(&ticket_bytes[TICKET_BODY_LEN..])
        .map_err(|_| RelayError::Authentication)?;
    let target = bytes[CLIENT_HEADER - 3];
    let kind = bytes[4];
    if ticket.expires > now.saturating_add(MAX_TICKET_LIFETIME)
        || (ticket.expires <= now
            && !(kind == CONTROL
                && target == BROADCAST
                && bytes[CLIENT_HEADER..CLIENT_HEADER + length] == [sequenced::LEAVE_REQUEST]))
    {
        return Err(RelayError::Expired);
    }
    let packet_key = derive_packet_key(key, ticket_bytes);
    packet_key
        .mac(CLIENT_DOMAIN, &bytes[..bytes.len() - MAC_LEN])
        .verify_slice(&bytes[bytes.len() - MAC_LEN..])
        .map_err(|_| RelayError::Authentication)?;
    if (target >= MAX_PEERS && target != BROADCAST)
        || !matches!(kind, HELLO | CONFIRM | DATA | PING | CONTROL)
        || (kind == CONFIRM && length != COOKIE_LEN)
        || (kind == DATA
            && (length <= PAYLOAD_HEADER + PAYLOAD_TAG || bytes[CLIENT_HEADER] != target))
        || (matches!(kind, HELLO | PING) && length != 0)
        || (kind == CONTROL && (length == 0 || target != BROADCAST))
    {
        return Err(RelayError::Malformed);
    }
    let sequence = u64::from_le_bytes(
        bytes[5 + TICKET_LEN..CLIENT_HEADER - 3]
            .try_into()
            .expect("checked client header"),
    );
    if sequence == 0 {
        return Err(RelayError::Replay);
    }
    Ok(Incoming {
        ticket,
        ticket_bytes,
        key: packet_key,
        kind,
        sequence,
        target,
        payload: &bytes[CLIENT_HEADER..CLIENT_HEADER + length],
    })
}

#[derive(Clone, Copy, Default)]
struct RateBudget {
    second: u64,
    spent: u64,
}

impl RateBudget {
    fn permits(&self, now: u64, amount: u64, limit: u64) -> bool {
        let spent = if self.second == now { self.spent } else { 0 };
        amount <= limit.saturating_sub(spent)
    }

    fn charge(&mut self, now: u64, amount: u64) {
        if self.second != now {
            self.second = now;
            self.spent = 0;
        }
        self.spent += amount;
    }
}

struct Peer {
    ticket: Ticket,
    key: SecretKey,
    endpoint: SocketAddrV4,
    received: ReplayWindow,
    sent: u64,
    last_seen: u64,
}

#[derive(Default)]
struct Room {
    peers: Vec<Peer>,
    outgoing: RateBudget,
    order_packets: RateBudget,
    ordering: Option<sequenced::OrderingRoom>,
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

/// Bound storage and packet rate; untrusted data never creates a room until
/// both ticket authentication and UDP source reachability are established.
pub struct RelayServer {
    key: SecretKey,
    rooms: HashMap<[u8; 16], Room>,
    admissions: HashMap<[u8; 16], Admission>,
    max_rooms: usize,
    max_principals: usize,
    ingress: RateBudget,
    handshakes: RateBudget,
    outgoing: RateBudget,
    order_packets: RateBudget,
    last_pruned: u64,
    journal_directory: Option<journal::JournalDirectory>,
}

impl RelayServer {
    pub fn new(key: SecretKey, max_rooms: usize) -> Self {
        Self {
            key,
            rooms: HashMap::new(),
            admissions: HashMap::new(),
            max_rooms,
            max_principals: max_rooms.saturating_mul(usize::from(MAX_PEERS)),
            ingress: RateBudget::default(),
            handshakes: RateBudget::default(),
            outgoing: RateBudget::default(),
            order_packets: RateBudget::default(),
            last_pruned: 0,
            journal_directory: None,
        }
    }

    /// Enable full-match encrypted replay on the existing relay's private
    /// StateDirectory. Existing room files are never overwritten or resumed.
    pub fn with_journal(
        key: SecretKey,
        max_rooms: usize,
        state_dir: &std::path::Path,
    ) -> std::io::Result<Self> {
        let mut server = Self::new(key, max_rooms);
        server.journal_directory = Some(journal::JournalDirectory::open(state_dir)?);
        Ok(server)
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
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
        // The cheap global gate bounds even malformed/forged authentication
        // work. No source-address map can grow under spoofed UDP traffic.
        if !self.ingress.permits(now, 1, INGRESS_PER_SECOND) {
            return Err(RelayError::RateLimited);
        }
        self.ingress.charge(now, 1);
        let message = incoming(&self.key, bytes, now)?;
        self.prune(now);
        if message.kind == CONTROL && message.payload == [sequenced::LEAVE_REQUEST] {
            return self.ordering_leave(source, &message, now, now_millis);
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
            .position(|peer| peer.ticket == message.ticket)
        else {
            return Err(RelayError::Unregistered);
        };
        if room.peers[index].endpoint != source {
            return if message.kind == PING {
                self.allow_handshake(&message, now)?;
                self.challenge_budgeted(source, &message, now)
            } else {
                Err(RelayError::Unregistered)
            };
        }
        room.peers[index].received.accept(message.sequence)?;
        let admission = self
            .admissions
            .get_mut(&message.ticket.player)
            .ok_or(RelayError::Unregistered)?;
        if !admission.packets.permits(now, 1, 2048) {
            return Err(RelayError::RateLimited);
        }
        admission.packets.charge(now, 1);
        room.peers[index].last_seen = now;
        if message.kind == CONTROL {
            return self.ordering_request(&message, now, now_millis);
        }
        let recipients = if message.kind == PING {
            1
        } else {
            room.peers
                .iter()
                .filter(|peer| {
                    peer.ticket.source != message.ticket.source
                        && now.saturating_sub(peer.last_seen) <= 30
                        && (message.target == BROADCAST || message.target == peer.ticket.source)
                })
                .count()
        };
        let packet_bytes = SERVER_HEADER
            + MAC_LEN
            + if message.kind == PING {
                0
            } else {
                message.payload.len()
            };
        let cost = (recipients * packet_bytes) as u64;
        // Reserve the full fan-out before any sequence mutation or delivery.
        // Renewals retain these principal budgets and cannot reset the rate.
        if !admission
            .outgoing
            .permits(now, cost, PLAYER_BYTES_PER_SECOND)
            || !room.outgoing.permits(now, cost, ROOM_BYTES_PER_SECOND)
            || !self.outgoing.permits(now, cost, TOTAL_BYTES_PER_SECOND)
        {
            return Err(RelayError::RateLimited);
        }
        admission.outgoing.charge(now, cost);
        room.outgoing.charge(now, cost);
        self.outgoing.charge(now, cost);
        if message.kind == PING {
            return Ok(vec![server_packet(
                &mut room.peers[index],
                READY,
                message.ticket.source,
                &[],
            )?]);
        }
        let mut output = Vec::with_capacity(recipients);
        for peer in &mut room.peers {
            if peer.ticket.source != message.ticket.source
                && now.saturating_sub(peer.last_seen) <= 30
                && (message.target == BROADCAST || message.target == peer.ticket.source)
            {
                output.push(server_packet(
                    peer,
                    DATA,
                    message.ticket.source,
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
        self.rooms.retain(|_, room| {
            room.peers.retain(|peer| peer.ticket.expires > now);
            !room.peers.is_empty() || room.ordering.is_some()
        });
        self.admissions
            .retain(|_, admission| admission.retain_until > now);
        self.last_pruned = now;
    }

    fn allow_handshake(&mut self, message: &Incoming<'_>, now: u64) -> Result<(), RelayError> {
        if !self.handshakes.permits(now, 1, HANDSHAKES_PER_SECOND)
            || self
                .admissions
                .get(&message.ticket.player)
                .is_some_and(|admission| {
                    !admission
                        .handshakes
                        .permits(now, 1, PLAYER_HANDSHAKES_PER_SECOND)
                })
        {
            return Err(RelayError::RateLimited);
        }
        self.handshakes.charge(now, 1);
        if let Some(admission) = self.admissions.get_mut(&message.ticket.player) {
            admission.handshakes.charge(now, 1);
        }
        Ok(())
    }

    fn challenge_budgeted(
        &mut self,
        source: SocketAddrV4,
        message: &Incoming<'_>,
        now: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        let cost = (SERVER_HEADER + MAC_LEN + COOKIE_LEN) as u64;
        if !self.outgoing.permits(now, cost, TOTAL_BYTES_PER_SECOND) {
            return Err(RelayError::RateLimited);
        }
        self.outgoing.charge(now, cost);
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
        let cost = (SERVER_HEADER + MAC_LEN) as u64;
        if !self.outgoing.permits(now, cost, TOTAL_BYTES_PER_SECOND) {
            return Err(RelayError::RateLimited);
        }
        if newer {
            let old = prior.expect("newer generation has an admission");
            if let Some(room) = self.rooms.get_mut(&old.room) {
                room.peers
                    .retain(|peer| peer.ticket.player != message.ticket.player);
                if room.peers.is_empty() && room.ordering.is_none() {
                    self.rooms.remove(&old.room);
                }
            }
        }
        let room = self.rooms.entry(message.ticket.room).or_default();
        let reply = if let Some(peer) = room
            .peers
            .iter_mut()
            .find(|peer| peer.ticket.source == message.ticket.source)
        {
            if peer.ticket != message.ticket {
                if message.ticket.expires <= peer.ticket.expires {
                    return Err(RelayError::Authentication);
                }
                *peer = new_peer(message, source, now);
            }
            if message.sequence <= peer.received.maximum && peer.endpoint != source {
                return Err(RelayError::Replay);
            }
            peer.received.accept(message.sequence)?;
            peer.endpoint = source;
            peer.last_seen = now;
            server_packet(peer, READY, message.ticket.source, &[])?
        } else {
            let mut peer = new_peer(message, source, now);
            peer.received.accept(message.sequence)?;
            let reply = server_packet(&mut peer, READY, message.ticket.source, &[])?;
            room.peers.push(peer);
            reply
        };
        let mut admission = prior.unwrap_or(Admission {
            generation: message.ticket.generation,
            room: message.ticket.room,
            source: message.ticket.source,
            retain_until: 0,
            handshakes: RateBudget::default(),
            packets: RateBudget::default(),
            outgoing: RateBudget::default(),
            order_packets: RateBudget::default(),
        });
        if prior.is_none() {
            admission.handshakes.charge(now, 1);
        }
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
        self.outgoing.charge(now, cost);
        self.retire_superseded_startups(message.ticket, now_ms, now);
        Ok(vec![reply])
    }

    fn challenge(&self, source: SocketAddrV4, message: &Incoming<'_>, now: u64) -> ForwardDatagram {
        let expires = now.saturating_add(10);
        let mut cookie = expires.to_le_bytes().to_vec();
        let tag = self
            .cookie_mac(source, message.ticket_bytes, expires)
            .finalize()
            .into_bytes();
        cookie.extend_from_slice(&tag);
        ForwardDatagram {
            endpoint: source,
            bytes: encode_server(
                &message.ticket,
                &message.key,
                CHALLENGE,
                message.sequence,
                message.ticket.source,
                &cookie,
            ),
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
        self.cookie_mac(source, message.ticket_bytes, expires)
            .verify_slice(&message.payload[8..])
            .map_err(|_| RelayError::Authentication)
    }
}

fn new_peer(message: &Incoming<'_>, endpoint: SocketAddrV4, now: u64) -> Peer {
    Peer {
        ticket: message.ticket,
        key: message.key.clone(),
        endpoint,
        received: ReplayWindow::default(),
        sent: 0,
        last_seen: now,
    }
}

fn server_packet(
    peer: &mut Peer,
    kind: u8,
    source: u8,
    payload: &[u8],
) -> Result<ForwardDatagram, RelayError> {
    peer.sent = peer
        .sent
        .checked_add(1)
        .ok_or(RelayError::CounterExhausted)?;
    Ok(ForwardDatagram {
        endpoint: peer.endpoint,
        bytes: encode_server(&peer.ticket, &peer.key, kind, peer.sent, source, payload),
    })
}

fn encode_server(
    ticket: &Ticket,
    key: &SecretKey,
    kind: u8,
    sequence: u64,
    source: u8,
    payload: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(SERVER_HEADER + payload.len() + MAC_LEN);
    bytes.extend_from_slice(SERVER_MAGIC);
    bytes.push(kind);
    bytes.extend_from_slice(&ticket.room);
    bytes.extend_from_slice(&ticket.nonce);
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.push(source);
    bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
    bytes.extend_from_slice(payload);
    let tag = key.mac(SERVER_DOMAIN, &bytes).finalize().into_bytes();
    bytes.extend_from_slice(&tag);
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(key: &SecretKey, room: u8, source: u8) -> ClientSession {
        let ticket = Ticket {
            room: [room; 16],
            source,
            expires: 1200,
            nonce: [source + 1; 16],
            player: [source + 1; 16],
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
        assert!(
            !bytes
                .windows(b"unchanged game datagram".len())
                .any(|part| part == b"unchanged game datagram")
        );
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
            }) if bytes.as_bytes() == b"unchanged game datagram"
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
        assert!(matches!(
            server.process(a, &bytes, 2000),
            Err(RelayError::Expired)
        ));
    }

    #[test]
    fn source_rebinding_requires_return_challenge_and_reordered_packets_cannot_move_it_back() {
        let key = SecretKey::new([91; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut first = client(&key, 1, 0);
        let old = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31001);
        let new = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31002);
        connect(&mut server, &mut first, old);
        let mut bytes = Vec::new();
        first.probe(&mut bytes).unwrap();
        let challenge = server.process(new, &bytes, 1000).unwrap().remove(0);
        let ServerEvent::Challenge(cookie) = first.receive(&challenge.bytes).unwrap() else {
            panic!("challenge required");
        };
        first.confirm(cookie, &mut bytes).unwrap();
        assert!(matches!(
            server.process(old, &bytes, 1000),
            Err(RelayError::Authentication)
        ));
        let old_confirm = bytes.clone();
        let ready = server.process(new, &bytes, 1000).unwrap().remove(0);
        assert!(matches!(
            first.receive(&ready.bytes),
            Ok(ServerEvent::Ready)
        ));
        assert!(matches!(
            server.process(old, &old_confirm, 1000),
            Err(RelayError::Authentication)
        ));
        first.data(29, b"peer not registered", &mut bytes).unwrap();
        assert!(matches!(
            server.process(old, &bytes, 1000),
            Err(RelayError::Unregistered)
        ));
        assert!(server.process(new, &bytes, 1000).unwrap().is_empty());
        assert!(!format!("{first:?} {key:?}").contains("91, 91"));
    }

    #[test]
    fn handshake_replays_and_confirmations_share_bounded_budgets() {
        let key = SecretKey::new([71; 32]);
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32001);
        let mut first = client(&key, 1, 0);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut bytes = Vec::new();
        first.probe(&mut bytes).unwrap();
        for _ in 0..HANDSHAKES_PER_SECOND {
            assert!(server.process(endpoint, &bytes, 1000).is_ok());
        }
        assert!(matches!(
            server.process(endpoint, &bytes, 1000),
            Err(RelayError::RateLimited)
        ));
        assert_eq!(server.room_count(), 0);

        let mut server = RelayServer::new(key, 1);
        connect(&mut server, &mut first, endpoint);
        first.forget_readiness();
        first.probe(&mut bytes).unwrap();
        let challenge = server.process(endpoint, &bytes, 1000).unwrap().remove(0);
        let ServerEvent::Challenge(cookie) = first.receive(&challenge.bytes).unwrap() else {
            panic!("challenge");
        };
        for _ in 0..6 {
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
            Err(RelayError::Authentication)
        ));
        assert_eq!(server.admissions.len(), 1);
        assert_eq!(server.admissions[&first.ticket.player].generation, 2);
    }

    #[test]
    fn payload_key_and_context_are_required_and_renewals_keep_the_match_key() {
        let key = SecretKey::new([75; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut first = client(&key, 1, 0);
        let mut second = client(&key, 1, 1);
        let a = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32300);
        let b = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32301);
        connect(&mut server, &mut first, a);
        connect(&mut server, &mut second, b);
        let mut bytes = Vec::new();
        first.data(1, b"private match chat", &mut bytes).unwrap();
        let forwarded = server.process(a, &bytes, 1000).unwrap().remove(0);
        second.payload_key = SecretKey::new([88; 32]);
        assert!(matches!(
            second.receive(&forwarded.bytes),
            Err(RelayError::Authentication)
        ));
        second.payload_key = SecretKey::new([89; 32]);
        let payload = &forwarded.bytes[SERVER_HEADER..forwarded.bytes.len() - MAC_LEN];
        let forged = encode_server(&second.ticket, &second.key, DATA, 99, 2, payload);
        assert!(matches!(
            second.receive(&forged),
            Err(RelayError::Authentication)
        ));
        assert!(
            matches!(second.receive(&forwarded.bytes), Ok(ServerEvent::Payload { bytes, .. }) if bytes.as_bytes() == b"private match chat")
        );
        let ticket = Ticket {
            expires: 1250,
            nonce: [23; 16],
            ..first.ticket
        }
        .sign(&key)
        .unwrap();
        let packet_key = *derive_packet_key(&key, &ticket).expose_for_https();
        assert!(first.renewal(&ticket, packet_key, [88; 32]).is_err());
        let renewed = first.renewal(&ticket, packet_key, [89; 32]).unwrap();
        assert_ne!(first.payload_prefix, renewed.payload_prefix);
    }
}
