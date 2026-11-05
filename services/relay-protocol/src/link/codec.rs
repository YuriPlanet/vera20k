//! UPLINK (request 8, spec §3.2) and the hop feedback tail of ACK and
//! STATUS (spec §3.3, §3.4). Little-endian throughout.

use super::{HopFeedback, RecvWindow};
use crate::RelayError;

pub(crate) const UPLINK_REQUEST: u8 = 8;
pub(crate) const FLAG_PUSH: u8 = 1;
pub(crate) const FLAG_PACE: u8 = 1 << 1;
pub(crate) const FLAG_PROBE: u8 = 1 << 2;
pub(crate) const FLAG_RESTORING: u8 = 1 << 3;
pub(crate) const LINK_DELAYED: u8 = 1 << 2;
/// `[up_high u64][up_bits u64][echo_delay u16]`.
pub(crate) const FEEDBACK_LEN: usize = 18;
pub(crate) const MAX_UPLINK_CHECKSUMS: usize = 2;
pub(crate) const MAX_UPLINK_BATCHES: usize = 4;
/// `40 < len ≤ 552`: nonce, at least one ciphertext byte and tag.
const MIN_OPAQUE_EXCLUSIVE: usize = 40;
const MAX_OPAQUE: usize = 552;

/// One pace segment: `[pace u8][frames_per_second u16][tick_micros u32]`,
/// `tick_micros` 0 for none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PaceSegment {
    pub pace: u8,
    pub frames_per_second: u16,
    pub tick_micros: u32,
}

/// A decoded or to-be-encoded UPLINK. `flags` bit1 PACE always matches
/// `pace`; batch `i` has ID `first_batch_id + i`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Uplink<'a> {
    pub flags: u8,
    pub recv_next: u64,
    pub down: RecvWindow,
    pub srtt_ms: u16,
    pub base_ms: u16,
    pub link: u8,
    pub checksums: Vec<(u32, u32)>,
    pub pace: Option<PaceSegment>,
    pub first_batch_id: u64,
    pub batches: Vec<&'a [u8]>,
}

impl Uplink<'_> {
    /// The encoded length before any batch.
    pub(crate) fn head_len(checksums: usize, pace: bool) -> usize {
        33 + 8 * checksums + if pace { 7 } else { 0 }
    }

    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        let flags = (self.flags & !FLAG_PACE) | if self.pace.is_some() { FLAG_PACE } else { 0 };
        out.push(UPLINK_REQUEST);
        out.push(flags);
        out.extend_from_slice(&self.recv_next.to_le_bytes());
        out.extend_from_slice(&self.down.high.to_le_bytes());
        out.extend_from_slice(&self.down.bits.to_le_bytes());
        out.extend_from_slice(&self.srtt_ms.to_le_bytes());
        out.extend_from_slice(&self.base_ms.to_le_bytes());
        out.push(self.link);
        out.push(self.checksums.len() as u8);
        for (frame, checksum) in &self.checksums {
            out.extend_from_slice(&frame.to_le_bytes());
            out.extend_from_slice(&checksum.to_le_bytes());
        }
        if let Some(pace) = self.pace {
            out.push(pace.pace);
            out.extend_from_slice(&pace.frames_per_second.to_le_bytes());
            out.extend_from_slice(&pace.tick_micros.to_le_bytes());
        }
        out.push(self.batches.len() as u8);
        if !self.batches.is_empty() {
            out.extend_from_slice(&self.first_batch_id.to_le_bytes());
        }
        for batch in &self.batches {
            out.extend_from_slice(&(batch.len() as u16).to_le_bytes());
            out.extend_from_slice(batch);
        }
    }
}

/// Decodes an UPLINK control payload under every Malformed rule of spec
/// §3.2: exact length, at most 2 checksums and 4 batches, reserved bits 0
/// (flags 4..7, link 3..7), `first_batch_id ≥ 1`, `40 < len ≤ 552`,
/// `recv_next ≥ 1`, and bit0 of `down_bits` set when `down_high > 0`. The
/// pace byte must also name an [`crate::OrderPace`].
pub(crate) fn decode_uplink(payload: &[u8]) -> Result<Uplink<'_>, RelayError> {
    let mut reader = Reader(payload);
    if reader.u8()? != UPLINK_REQUEST {
        return Err(RelayError::Malformed);
    }
    let flags = reader.u8()?;
    let recv_next = reader.u64()?;
    let down = RecvWindow {
        high: reader.u64()?,
        bits: reader.u64()?,
    };
    let srtt_ms = reader.u16()?;
    let base_ms = reader.u16()?;
    let link = reader.u8()?;
    let checksum_count = usize::from(reader.u8()?);
    if flags & 0xF0 != 0
        || link & 0xF8 != 0
        || checksum_count > MAX_UPLINK_CHECKSUMS
        || recv_next == 0
        || (down.high > 0 && down.bits & 1 == 0)
    {
        return Err(RelayError::Malformed);
    }
    let mut checksums = Vec::with_capacity(checksum_count);
    for _ in 0..checksum_count {
        checksums.push((reader.u32()?, reader.u32()?));
    }
    let pace = if flags & FLAG_PACE != 0 {
        let pace = PaceSegment {
            pace: reader.u8()?,
            frames_per_second: reader.u16()?,
            tick_micros: reader.u32()?,
        };
        if pace.pace > 2 {
            return Err(RelayError::Malformed);
        }
        Some(pace)
    } else {
        None
    };
    let batch_count = usize::from(reader.u8()?);
    if batch_count > MAX_UPLINK_BATCHES {
        return Err(RelayError::Malformed);
    }
    let mut first_batch_id = 0;
    let mut batches = Vec::with_capacity(batch_count);
    if batch_count > 0 {
        first_batch_id = reader.u64()?;
        if first_batch_id == 0 || first_batch_id.checked_add(batch_count as u64 - 1).is_none() {
            return Err(RelayError::Malformed);
        }
        for _ in 0..batch_count {
            let length = usize::from(reader.u16()?);
            if length <= MIN_OPAQUE_EXCLUSIVE || length > MAX_OPAQUE {
                return Err(RelayError::Malformed);
            }
            batches.push(reader.take(length)?);
        }
    }
    if !reader.0.is_empty() {
        return Err(RelayError::Malformed);
    }
    Ok(Uplink {
        flags,
        recv_next,
        down,
        srtt_ms,
        base_ms,
        link,
        checksums,
        pace,
        first_batch_id,
        batches,
    })
}

/// Appends the hop feedback tail (spec §3.3, §3.4).
pub(crate) fn append_feedback(payload: &mut Vec<u8>, feedback: HopFeedback) {
    payload.extend_from_slice(&feedback.up.high.to_le_bytes());
    payload.extend_from_slice(&feedback.up.bits.to_le_bytes());
    payload.extend_from_slice(&feedback.echo_delay_ms.to_le_bytes());
}

/// The feedback tail at `offset`; the caller checked the length.
pub(crate) fn read_feedback(payload: &[u8], offset: usize) -> HopFeedback {
    let mut reader = Reader(&payload[offset..offset + FEEDBACK_LEN]);
    HopFeedback {
        up: RecvWindow {
            high: reader.u64().expect("checked feedback"),
            bits: reader.u64().expect("checked feedback"),
        },
        echo_delay_ms: reader.u16().expect("checked feedback"),
    }
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], RelayError> {
        if self.0.len() < length {
            return Err(RelayError::Malformed);
        }
        let (head, tail) = self.0.split_at(length);
        self.0 = tail;
        Ok(head)
    }
    fn u8(&mut self) -> Result<u8, RelayError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, RelayError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, RelayError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> Result<u64, RelayError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn le(values: &[(u64, usize)]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|&(value, width)| value.to_le_bytes()[..width].to_vec())
            .collect()
    }

    fn appendix_uplink<'a>(recv_next: u64, high: u64, bits: u64) -> Uplink<'a> {
        Uplink {
            flags: FLAG_PUSH,
            recv_next,
            down: RecvWindow { high, bits },
            srtt_ms: 40,
            base_ms: 40,
            link: 0,
            checksums: Vec::new(),
            pace: None,
            first_batch_id: 0,
            batches: Vec::new(),
        }
    }

    /// Spec appendix A packets 14 and 24: UPLINKs without batches, 33 B.
    #[test]
    fn appendix_uplinks_without_batches_encode_field_for_field() {
        for (uplink, recv_next, high, bits) in [
            (appendix_uplink(1, 7, 0x7F), 1, 7, 0x7F),
            (appendix_uplink(10, 11, 0x7FD), 10, 11, 0x7FD),
        ] {
            let mut bytes = Vec::new();
            uplink.encode(&mut bytes);
            let expected = le(&[
                (8, 1),
                (1, 1),
                (recv_next, 8),
                (high, 8),
                (bits, 8),
                (40, 2),
                (40, 2),
                (0, 1),
                (0, 1),
                (0, 1),
            ]);
            assert_eq!(bytes, expected);
            assert_eq!(bytes.len(), 33);
            assert_eq!(decode_uplink(&bytes).unwrap(), uplink);
        }
    }

    /// Spec appendix A packet 18: one 74-byte batch, 117 B.
    #[test]
    fn appendix_uplink_with_a_batch_encodes_field_for_field() {
        let opaque = [0xAB; 74];
        let mut uplink = appendix_uplink(3, 8, 0xFF);
        uplink.first_batch_id = 1;
        uplink.batches = vec![&opaque];
        let mut bytes = Vec::new();
        uplink.encode(&mut bytes);
        let mut expected = le(&[
            (8, 1),
            (1, 1),
            (3, 8),
            (8, 8),
            (0xFF, 8),
            (40, 2),
            (40, 2),
            (0, 1),
            (0, 1),
            (1, 1),
            (1, 8),
            (74, 2),
        ]);
        expected.extend_from_slice(&opaque);
        assert_eq!(bytes, expected);
        assert_eq!(bytes.len(), 117);
        assert_eq!(decode_uplink(&bytes).unwrap(), uplink);
    }

    #[test]
    fn the_largest_first_batch_fits_beside_both_checksums_and_pace() {
        let opaque = [1; MAX_OPAQUE];
        let mut uplink = appendix_uplink(1, 0, 0);
        uplink.flags = FLAG_PUSH | FLAG_PACE;
        uplink.checksums = vec![(15, 1), (30, 2)];
        uplink.pace = Some(PaceSegment {
            pace: 0,
            frames_per_second: 30,
            tick_micros: 2100,
        });
        uplink.first_batch_id = 1;
        uplink.batches = vec![&opaque];
        let mut bytes = Vec::new();
        uplink.encode(&mut bytes);
        assert_eq!(bytes.len(), 618);
        assert!(bytes.len() <= crate::MAX_WIRE_PAYLOAD);
        assert_eq!(bytes[1], FLAG_PUSH | FLAG_PACE);
        assert_eq!(decode_uplink(&bytes).unwrap(), uplink);
    }

    #[test]
    fn every_malformed_rule_rejects_the_uplink() {
        let opaque = [1; 41];
        let mut uplink = appendix_uplink(1, 7, 0x7F);
        uplink.first_batch_id = 1;
        uplink.batches = vec![&opaque];
        let mut good = Vec::new();
        uplink.encode(&mut good);
        assert!(decode_uplink(&good).is_ok());
        let mutate = |change: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = good.clone();
            change(&mut bytes);
            decode_uplink(&bytes).map(|_| ())
        };
        let malformed = Err(RelayError::Malformed);
        // 1: exact length.
        assert_eq!(mutate(&|b| b.push(0)), malformed);
        assert_eq!(
            mutate(&|b| {
                b.pop();
            }),
            malformed
        );
        // 2: counts and reserved bits.
        assert_eq!(mutate(&|b| b[31] = 3), malformed);
        assert_eq!(mutate(&|b| b[32] = 5), malformed);
        assert_eq!(mutate(&|b| b[1] |= 0x10), malformed);
        assert_eq!(mutate(&|b| b[30] |= 0x08), malformed);
        // 3: first_batch_id ≥ 1.
        assert_eq!(mutate(&|b| b[33..41].fill(0)), malformed);
        // 4: 40 < len ≤ 552.
        assert_eq!(
            mutate(&|b| {
                b[41] = 40;
                b.pop();
            }),
            malformed
        );
        // 5: recv_next ≥ 1.
        assert_eq!(mutate(&|b| b[2..10].fill(0)), malformed);
        // 6: bit0 of down_bits when down_high > 0.
        assert_eq!(mutate(&|b| b[18] = 0x7E), malformed);
        // No feedback yet (down_high 0) needs no bit.
        assert!(
            mutate(&|b| {
                b[10..26].fill(0);
            })
            .is_ok()
        );
    }

    /// Spec appendix A packet 19: the 31-byte ACK.
    #[test]
    fn appendix_ack_feedback_appends_field_for_field() {
        let mut ack = le(&[(2, 1), (1, 8), (5, 4)]);
        append_feedback(
            &mut ack,
            HopFeedback {
                up: RecvWindow {
                    high: 8,
                    bits: 0x7F,
                },
                echo_delay_ms: 0,
            },
        );
        assert_eq!(
            ack,
            le(&[(2, 1), (1, 8), (5, 4), (8, 8), (0x7F, 8), (0, 2)])
        );
        assert_eq!(read_feedback(&ack, 13).up.bits, 0x7F);
    }
}
