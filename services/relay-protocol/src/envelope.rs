//! Envelope v3 of the bandwidth compaction
//! (`services/relay/bandwidth-compaction/spec.md` §6): the hop framing of
//! every datagram between a client and the relay, both directions.
//!
//! ```text
//! [hdr u8][session u32][seq u32][payload][tag HOP_TAG_LEN]
//! ```
//!
//! `hdr` holds the protocol version (bits 5..7, `VERSION`) and the kind
//! (bits 0..4). Integers are little-endian. UDP carries the length, so the
//! envelope has none: the payload is everything between the header and the
//! tag.
//!
//! This module is the framing and the tag only. Which session and sequence
//! a datagram must carry, the replay window, rate limits and the per-kind
//! payloads (§6.1 table) belong to `ClientSession` and `RelayServer`.
//!
//! Milestone C1: interface only.
#![allow(unused_variables)]

use crate::{RelayError, SecretKey};

/// The protocol version in `hdr` bits 5..7 (§6.1). Datagrams of any other
/// version are dropped (§10).
pub const VERSION: u8 = 3;

/// `hdr`, `session` and `seq`: the bytes before the payload (§6.1).
pub const HEADER_LEN: usize = 9;

/// The truncated HMAC-SHA256 tag after the payload (§6.3, §6.4). The rest
/// of the format does not depend on it, so a larger margin is this constant
/// alone (§6.4).
pub const HOP_TAG_LEN: usize = 4;

/// Fixed bytes of every envelope, either direction: 13 (§6.1).
pub const OVERHEAD: usize = HEADER_LEN + HOP_TAG_LEN;

/// The largest payload an envelope carries.
///
/// Pending: §6.1 names no limit. §5.3 rule 5 bounds a PACK by "the envelope
/// limit (§6.1)" and §5.4 keeps the 681-byte packing limit, so this is 681
/// for now, today's `MAX_WIRE_PAYLOAD`. A federated CONFIRM (§6.1: ticket
/// 98 + cookie 40 + a roster of up to 561 bytes) can exceed it.
pub const MAX_PAYLOAD: usize = 681;

/// The largest envelope v3 datagram: `OVERHEAD + MAX_PAYLOAD`. A receive
/// buffer of this size holds any valid datagram.
pub const MAX_DATAGRAM: usize = OVERHEAD + MAX_PAYLOAD;

/// The hop sequence at which the sender starts a renewal: 2³² − 2¹⁶
/// (§11 case 7). It keeps sending until `u32::MAX`; past that it refuses
/// with `RelayError::CounterExhausted` (§6.1).
pub const RENEWAL_SEQUENCE: u32 = u32::MAX - 0xFFFF;

/// Client to relay kinds (§6.1).
pub const HELLO: u8 = 0;
pub const CONFIRM: u8 = 1;
/// Both directions: upstream payload `[target u8][data]`, downstream
/// `[source u8][data]` (§6.1).
pub const DATA: u8 = 2;
pub const PING: u8 = 3;
/// Both directions: ordering requests and replies (§6.1, §7).
pub const CONTROL: u8 = 4;
/// Relay to client kinds (§6.1).
pub const CHALLENGE: u8 = 0;
pub const READY: u8 = 1;
pub const LEAVE_ACK: u8 = 5;

/// Which way a datagram travels. It selects the MAC domain label and the
/// kinds that are valid (§6.1, §6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Client to relay: kinds HELLO, CONFIRM, DATA, PING, CONTROL; domain
    /// `CLIENT_DOMAIN` at v3.
    Up,
    /// Relay to client: kinds CHALLENGE, READY, DATA, CONTROL, LEAVE_ACK;
    /// domain `SERVER_DOMAIN` at v3.
    Down,
}

/// The fixed fields before the payload (§6.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// `hdr` bits 0..4; one of this module's kind constants valid for the
    /// direction.
    pub kind: u8,
    /// The hop's session from READY (§6.2); 0 on HELLO and CONFIRM, which
    /// have none yet.
    pub session: u32,
    /// The sender's hop sequence, from 1 (§6.1). CHALLENGE and LEAVE_ACK
    /// echo the sequence of the HELLO or Leave they answer.
    pub sequence: u32,
}

/// Writes one datagram into `out`, replacing its contents:
/// `hdr = VERSION << 5 | kind`, then `session`, `sequence`, `payload` and
/// the tag, the first `HOP_TAG_LEN` bytes of
/// `HMAC-SHA256(key, domain ‖ hdr ‖ session ‖ seq ‖ payload)` (§6.3), the
/// domain chosen by `direction`. `out.len()` is then
/// `OVERHEAD + payload.len()`.
///
/// `key` is the hop's packet key (`derive_packet_key`). Any sequence and
/// session are written as given; the sessions choose them.
///
/// `Err(RelayError::Malformed)`, `out` unspecified, when `kind` is not a
/// kind of `direction` or `payload` is longer than `MAX_PAYLOAD`.
pub fn seal(
    direction: Direction,
    key: &SecretKey,
    header: Header,
    payload: &[u8],
    out: &mut Vec<u8>,
) -> Result<(), RelayError> {
    todo!("C2: compaction spec §6.1, §6.3")
}

/// Reads the header without the tag (§6.1): the cheap first step that lets
/// the relay find the hop by `session` (§6.2) and apply the failure limits
/// before any HMAC (§6.4). The result is unauthenticated: no state may
/// change on it alone (§6.4 condition 4).
///
/// `Err(RelayError::Malformed)` when `bytes` is shorter than `OVERHEAD` or
/// longer than `MAX_DATAGRAM`, the version is not `VERSION`, or the kind is
/// not one of `direction`.
pub fn peek(direction: Direction, bytes: &[u8]) -> Result<Header, RelayError> {
    todo!("C2: compaction spec §6.1")
}

/// `peek`, then verifies the tag under `key` with a constant-time
/// comparison (§6.3). Returns the header and the payload: the bytes between
/// the header and the tag, possibly empty.
///
/// Pure: it touches no replay window, receive window, endpoint or failure
/// count; the caller updates those only after `Ok` (§6.4 condition 4).
///
/// `Err(RelayError::Malformed)` as `peek`; `Err(RelayError::Authentication)`
/// when the tag does not match.
pub fn open<'a>(
    direction: Direction,
    key: &SecretKey,
    bytes: &'a [u8],
) -> Result<(Header, &'a [u8]), RelayError> {
    todo!("C2: compaction spec §6.1, §6.3")
}
