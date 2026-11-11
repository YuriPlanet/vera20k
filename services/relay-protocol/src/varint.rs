//! Unsigned LEB128 integers of the bandwidth compaction
//! (`services/relay/bandwidth-compaction/spec.md` §8), the one varint codec
//! every relay message shares: envelope v3 payloads (§6), the ordering
//! messages (§7) and PACK v2 (§5).
//!
//! Each byte carries 7 data bits, least significant group first; a set top
//! bit means another byte follows. Encoders write the shortest form and
//! decoders refuse every other one, so a value has exactly one encoding.

use crate::RelayError;

/// The longest encoding: 10 bytes, which `u64::MAX` needs (§8).
pub const MAX_LEN: usize = 10;

/// The length in bytes, 1..=`MAX_LEN`, of `value`'s shortest encoding
/// (§8): 1 for 0..=127, 2 for 128..=16_383, ..., 10 for values of
/// 2⁶³ and above. Message encoders use it to size records and packs before
/// writing them, as PACK v2's packing does (§5.4).
pub fn encoded_len(value: u64) -> usize {
    // 7 data bits per byte; 0 still takes one byte.
    (64 - value.max(1).leading_zeros() as usize).div_ceil(7)
}

/// Appends the shortest encoding of `value` to `out` (§8): exactly
/// `encoded_len(value)` bytes. Never fails.
pub fn write(mut value: u64, out: &mut Vec<u8>) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
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
    let mut value = 0u64;
    for (index, &byte) in bytes.iter().take(MAX_LEN).enumerate() {
        if index == MAX_LEN - 1 && byte > 1 {
            return Err(RelayError::Malformed);
        }
        value |= u64::from(byte & 0x7F) << (7 * index);
        if byte & 0x80 == 0 {
            if byte == 0 && index > 0 {
                return Err(RelayError::Malformed);
            }
            return Ok((value, index + 1));
        }
    }
    Err(RelayError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(value: u64) -> Vec<u8> {
        let mut out = Vec::new();
        write(value, &mut out);
        out
    }

    #[test]
    fn boundaries_round_trip_in_shortest_form() {
        for (value, bytes) in [
            (0, &[0x00][..]),
            (127, &[0x7F]),
            (128, &[0x80, 0x01]),
            (16_383, &[0xFF, 0x7F]),
            (16_384, &[0x80, 0x80, 0x01]),
            (
                u64::MAX,
                &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01],
            ),
        ] {
            assert_eq!(encode(value), bytes, "{value}");
            assert_eq!(encoded_len(value), bytes.len(), "{value}");
            let mut trailing = bytes.to_vec();
            trailing.push(0xAA);
            assert_eq!(read(&trailing), Ok((value, bytes.len())), "{value}");
        }
    }

    #[test]
    fn refuses_truncated_overlong_and_out_of_range() {
        for bytes in [
            &[][..],
            &[0x80],
            &[0xFF, 0xFF],
            // Not shortest.
            &[0x80, 0x00],
            &[0x81, 0x80, 0x00],
            // A 10th byte beyond u64.
            &[0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x02],
            // An 11th byte.
            &[
                0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x81, 0x01,
            ],
        ] {
            assert_eq!(read(bytes), Err(RelayError::Malformed), "{bytes:02x?}");
        }
    }
}
