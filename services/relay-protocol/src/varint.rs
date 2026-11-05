//! Unsigned LEB128 integers of the bandwidth compaction
//! (`services/relay/bandwidth-compaction/spec.md` §8), the one varint codec
//! every relay message shares: envelope v3 payloads (§6), the ordering
//! messages (§7) and PACK v2 (§5).
//!
//! Each byte carries 7 data bits, least significant group first; a set top
//! bit means another byte follows. Encoders write the shortest form and
//! decoders refuse every other one, so a value has exactly one encoding.
//!
//! Milestone C1: interface only.
#![allow(unused_variables)]

use crate::RelayError;

/// The longest encoding: 10 bytes, which `u64::MAX` needs (§8).
pub const MAX_LEN: usize = 10;

/// The length in bytes, 1..=`MAX_LEN`, of `value`'s shortest encoding
/// (§8): 1 for 0..=127, 2 for 128..=16_383, ..., 10 for values of
/// 2⁶³ and above. Message encoders use it to size records and packs before
/// writing them, as PACK v2's packing does (§5.4).
pub fn encoded_len(value: u64) -> usize {
    todo!("C2: compaction spec §8")
}

/// Appends the shortest encoding of `value` to `out` (§8): exactly
/// `encoded_len(value)` bytes. Never fails.
pub fn write(value: u64, out: &mut Vec<u8>) {
    todo!("C2: compaction spec §8")
}

/// Reads one varint from the start of `bytes` (§8) and returns the value
/// with the number of bytes it took, 1..=`MAX_LEN`; bytes after it are the
/// caller's. Reads no further than it needs.
///
/// `Err(RelayError::Malformed)` when:
/// - `bytes` ends before a byte with a clear top bit (truncated, including
///   empty `bytes`);
/// - the encoding is not the shortest one: it is longer than one byte and
///   its last byte is `0x00`;
/// - it would take more than `MAX_LEN` bytes, or its 10th byte is other
///   than `0x00` or `0x01` (beyond `u64::MAX`). A 10th byte `0x00` is
///   already non-shortest by the previous rule.
pub fn read(bytes: &[u8]) -> Result<(u64, usize), RelayError> {
    todo!("C2: compaction spec §8")
}
