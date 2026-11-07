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

use hmac::Mac;

use crate::{RelayError, SecretKey};

/// MAC domain labels at v3 (§6.3); `KEY_DOMAIN`, `TICKET_DOMAIN` and
/// `COOKIE_DOMAIN` stay at v2, so the lobby is unchanged.
pub(crate) const CLIENT_DOMAIN: &[u8] = b"VERA20k relay client v3\0";
pub(crate) const SERVER_DOMAIN: &[u8] = b"VERA20k relay server v3\0";

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

/// The largest payload an envelope carries: 720 bytes (§6.1), room for a
/// federated CONFIRM (ticket 98 + cookie 40 + a roster of up to 561 bytes,
/// 699 bytes). PACKs keep their own smaller limit, `PACK_LIMIT`.
pub const MAX_PAYLOAD: usize = 720;

/// The packing limit of a PACK payload: 681 bytes (§5.3 rule 5, §5.4),
/// unchanged so the packing makes the same number of packs.
pub const PACK_LIMIT: usize = 681;

/// The largest envelope v3 datagram: `OVERHEAD + MAX_PAYLOAD`. A receive
/// buffer of this size holds any valid datagram.
pub const MAX_DATAGRAM: usize = OVERHEAD + MAX_PAYLOAD;

/// The hop sequence at which a client starts a renewal: 2³² − 2¹⁶
/// (§11 case 7). It keeps sending until `u32::MAX`; past that it refuses
/// with `RelayError::CounterExhausted` (§6.1). The relay cannot renew: if
/// its sequence on a hop ever runs out, it stops sending on that hop and
/// the client renews by its disconnect path (§6.1).
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
    /// The hop's session (§6.2). 0 on HELLO, CONFIRM and CHALLENGE, which
    /// have none yet; the READY answering a CONFIRM carries the newly
    /// assigned one.
    pub session: u32,
    /// The sender's hop sequence, from 1 (§6.1). CHALLENGE and LEAVE_ACK
    /// echo the sequence of the HELLO or Leave they answer; the READY
    /// answering a CONFIRM is the relay's first on the hop.
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
    if !direction.has_kind(header.kind) || payload.len() > MAX_PAYLOAD {
        return Err(RelayError::Malformed);
    }
    out.clear();
    out.push(VERSION << 5 | header.kind);
    out.extend_from_slice(&header.session.to_le_bytes());
    out.extend_from_slice(&header.sequence.to_le_bytes());
    out.extend_from_slice(payload);
    let tag = key.mac(direction.domain(), out).finalize().into_bytes();
    out.extend_from_slice(&tag[..HOP_TAG_LEN]);
    Ok(())
}

impl Direction {
    fn has_kind(self, kind: u8) -> bool {
        match self {
            Self::Up => matches!(kind, HELLO | CONFIRM | DATA | PING | CONTROL),
            Self::Down => matches!(kind, CHALLENGE | READY | DATA | CONTROL | LEAVE_ACK),
        }
    }

    fn domain(self) -> &'static [u8] {
        match self {
            Self::Up => CLIENT_DOMAIN,
            Self::Down => SERVER_DOMAIN,
        }
    }
}

/// Reads the header without the tag (§6.1): the cheap first step that lets
/// the relay find the hop by `session` (§6.2) and drop an unknown session
/// without an HMAC (§6.4 item 1). The result is unauthenticated: no state
/// may change on it alone (§6.4 item 5).
///
/// `Err(RelayError::Malformed)` when `bytes` is shorter than `OVERHEAD` or
/// longer than `MAX_DATAGRAM`, the version is not `VERSION`, or the kind is
/// not one of `direction`.
pub fn peek(direction: Direction, bytes: &[u8]) -> Result<Header, RelayError> {
    if !(OVERHEAD..=MAX_DATAGRAM).contains(&bytes.len()) || bytes[0] >> 5 != VERSION {
        return Err(RelayError::Malformed);
    }
    let kind = bytes[0] & 0x1F;
    if !direction.has_kind(kind) {
        return Err(RelayError::Malformed);
    }
    Ok(Header {
        kind,
        session: u32::from_le_bytes(bytes[1..5].try_into().expect("checked length")),
        sequence: u32::from_le_bytes(bytes[5..9].try_into().expect("checked length")),
    })
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
    let header = peek(direction, bytes)?;
    let body = bytes.len() - HOP_TAG_LEN;
    // RustCrypto's truncated verification compares in constant time:
    // https://docs.rs/digest/0.10.7/digest/trait.Mac.html#tymethod.verify_truncated_left
    key.mac(direction.domain(), &bytes[..body])
        .verify_truncated_left(&bytes[body..])
        .map_err(|_| RelayError::Authentication)?;
    Ok((header, &bytes[HEADER_LEN..body]))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: Header = Header {
        kind: DATA,
        session: 0x0403_0201,
        sequence: 7,
    };

    fn sealed(direction: Direction, payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0xEE; 3];
        seal(
            direction,
            &SecretKey::new([9; 32]),
            HEADER,
            payload,
            &mut out,
        )
        .unwrap();
        out
    }

    #[test]
    fn round_trips_with_a_four_byte_tag_per_direction() {
        let bytes = sealed(Direction::Up, &[5, 6]);
        assert_eq!(bytes.len(), OVERHEAD + 2);
        assert_eq!(bytes[..11], [0x62, 1, 2, 3, 4, 7, 0, 0, 0, 5, 6]);
        let key = SecretKey::new([9; 32]);
        assert_eq!(
            open(Direction::Up, &key, &bytes),
            Ok((HEADER, &[5u8, 6][..]))
        );
        // The domain separates the directions.
        assert_ne!(bytes, sealed(Direction::Down, &[5, 6]));
        assert_eq!(
            open(Direction::Down, &key, &bytes),
            Err(RelayError::Authentication)
        );
        let mut forged = bytes.clone();
        *forged.last_mut().unwrap() ^= 1;
        assert_eq!(
            open(Direction::Up, &key, &forged),
            Err(RelayError::Authentication)
        );
        assert_eq!(
            open(Direction::Up, &SecretKey::new([8; 32]), &bytes),
            Err(RelayError::Authentication)
        );
    }

    #[test]
    fn peek_refuses_bad_framing() {
        let bytes = sealed(Direction::Up, &[]);
        assert_eq!(peek(Direction::Up, &bytes), Ok(HEADER));
        assert_eq!(
            peek(Direction::Up, &bytes[..OVERHEAD - 1]),
            Err(RelayError::Malformed)
        );
        let mut version = bytes.clone();
        version[0] = 2 << 5 | DATA;
        assert_eq!(peek(Direction::Up, &version), Err(RelayError::Malformed));
        let mut kind = bytes.clone();
        kind[0] = VERSION << 5 | LEAVE_ACK;
        assert_eq!(peek(Direction::Up, &kind), Err(RelayError::Malformed));
        assert!(peek(Direction::Down, &kind).is_ok());
        assert_eq!(
            peek(Direction::Up, &vec![VERSION << 5; MAX_DATAGRAM + 1]),
            Err(RelayError::Malformed)
        );
        let key = SecretKey::new([9; 32]);
        let mut out = Vec::new();
        assert_eq!(
            seal(Direction::Up, &key, HEADER, &[0; MAX_PAYLOAD + 1], &mut out),
            Err(RelayError::Malformed)
        );
        assert_eq!(
            seal(
                Direction::Up,
                &key,
                Header {
                    kind: LEAVE_ACK,
                    ..HEADER
                },
                &[],
                &mut out
            ),
            Err(RelayError::Malformed)
        );
    }
}
