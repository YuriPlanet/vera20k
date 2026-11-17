//! UPLINK (request 8, spec §3.2) and the hop feedback tail of ACK and
//! STATUS (spec §3.3, §3.4), in the varint encoding of the bandwidth
//! compaction (its §7.1-§7.3, §8). Fixed-width integers are little-endian.

use super::{HopFeedback, RecvWindow};
use crate::{ORDERING_BATCH_CAPACITY, RelayError, varint};

pub(crate) const UPLINK_REQUEST: u8 = 8;
pub(crate) const FLAG_PUSH: u8 = 1;
pub(crate) const FLAG_PACE: u8 = 1 << 1;
pub(crate) const FLAG_PROBE: u8 = 1 << 2;
pub(crate) const FLAG_RESTORING: u8 = 1 << 3;
pub(crate) const LINK_DELAYED: u8 = 1 << 2;
pub(crate) const LINK_JITTERY: u8 = 1 << 3;
/// The longest `[up_high varint][up_bits u64][echo_delay varint]`.
pub(crate) const FEEDBACK_LEN: usize = varint::MAX_LEN + 8 + 3;
pub(crate) const MAX_UPLINK_CHECKSUMS: usize = 2;
pub(crate) const MAX_UPLINK_BATCHES: usize = 4;

/// One pace segment: `[pace u8][frames_per_second varint][tick_micros
/// varint]`, `tick_micros` 0 for none.
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
    pub rttvar_ms: u16,
    pub link: u8,
    pub checksums: Vec<(u32, u32)>,
    pub pace: Option<PaceSegment>,
    pub first_batch_id: u64,
    pub batches: Vec<&'a [u8]>,
}

impl Uplink<'_> {
    /// `[8][flags u8][recv_next varint][down_high varint][down_bits u64]
    /// [srtt_ms varint][base_ms varint][rttvar_ms varint][link u8]
    /// [checksum_count u8]{[frame varint][checksum u32]}[pace segment]
    /// [batch_count u8][first_batch_id varint]{[len varint][P len]}`
    /// (compaction spec §7.1).
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        self.encode_head(out);
        out.push(self.batches.len() as u8);
        if !self.batches.is_empty() {
            varint::write(self.first_batch_id, out);
        }
        for batch in &self.batches {
            varint::write(batch.len() as u64, out);
            out.extend_from_slice(batch);
        }
    }

    fn encode_head(&self, out: &mut Vec<u8>) {
        let flags = (self.flags & !FLAG_PACE) | if self.pace.is_some() { FLAG_PACE } else { 0 };
        out.push(UPLINK_REQUEST);
        out.push(flags);
        varint::write(self.recv_next, out);
        varint::write(self.down.high, out);
        out.extend_from_slice(&self.down.bits.to_le_bytes());
        varint::write(self.srtt_ms.into(), out);
        varint::write(self.base_ms.into(), out);
        varint::write(self.rttvar_ms.into(), out);
        out.push(self.link);
        out.push(self.checksums.len() as u8);
        for (frame, checksum) in &self.checksums {
            varint::write((*frame).into(), out);
            out.extend_from_slice(&checksum.to_le_bytes());
        }
        if let Some(pace) = self.pace {
            out.push(pace.pace);
            varint::write(pace.frames_per_second.into(), out);
            varint::write(pace.tick_micros.into(), out);
        }
    }

    /// Keeps the leading batches that fit `limit` bytes encoded, in order
    /// (spec §6.2: the batches that fit, from `accepted_through + 1`).
    pub(crate) fn fit(&mut self, limit: usize) {
        let mut head = Vec::new();
        self.encode_head(&mut head);
        let mut length = head.len() + 1 + varint::encoded_len(self.first_batch_id);
        let mut fitting = 0;
        for batch in &self.batches {
            length += varint::encoded_len(batch.len() as u64) + batch.len();
            if length > limit {
                break;
            }
            fitting += 1;
        }
        self.batches.truncate(fitting);
    }
}

/// Decodes an UPLINK control payload under every Malformed rule of spec
/// §3.2 and compaction spec §7.1, §8: exact length, at most 2 checksums and
/// 4 batches, reserved bits 0 (flags 4..7, link 4..7), `first_batch_id ≥
/// 1`, `1 ≤ len ≤ 512`, `recv_next ≥ 1`, bit0 of `down_bits` set when
/// `down_high > 0`, shortest varints within their fields' former widths.
/// The pace byte must also name an [`crate::OrderPace`].
pub(crate) fn decode_uplink(payload: &[u8]) -> Result<Uplink<'_>, RelayError> {
    let mut reader = Reader(payload);
    if reader.u8()? != UPLINK_REQUEST {
        return Err(RelayError::Malformed);
    }
    let flags = reader.u8()?;
    let recv_next = reader.varint()?;
    let down = RecvWindow {
        high: reader.varint()?,
        bits: reader.u64()?,
    };
    let srtt_ms = reader.varint_u16()?;
    let base_ms = reader.varint_u16()?;
    let rttvar_ms = reader.varint_u16()?;
    let link = reader.u8()?;
    let checksum_count = usize::from(reader.u8()?);
    if flags & 0xF0 != 0
        || link & 0xF0 != 0
        || checksum_count > MAX_UPLINK_CHECKSUMS
        || recv_next == 0
        || (down.high > 0 && down.bits & 1 == 0)
    {
        return Err(RelayError::Malformed);
    }
    let mut checksums = Vec::with_capacity(checksum_count);
    for _ in 0..checksum_count {
        checksums.push((reader.varint_u32()?, reader.u32()?));
    }
    let pace = if flags & FLAG_PACE != 0 {
        let pace = PaceSegment {
            pace: reader.u8()?,
            frames_per_second: reader.varint_u16()?,
            tick_micros: reader.varint_u32()?,
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
        first_batch_id = reader.varint()?;
        if first_batch_id == 0 || first_batch_id.checked_add(batch_count as u64 - 1).is_none() {
            return Err(RelayError::Malformed);
        }
        for _ in 0..batch_count {
            let length = reader.varint()?;
            if !(OPAQUE_MIN..=OPAQUE_MAX).contains(&length) {
                return Err(RelayError::Malformed);
            }
            batches.push(reader.take(length as usize)?);
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
        rttvar_ms,
        link,
        checksums,
        pace,
        first_batch_id,
        batches,
    })
}

/// Batch length bounds of an UPLINK: plaintext `1 ≤ len ≤ 512`
/// (compaction spec §3.1, §7.1).
const OPAQUE_MIN: u64 = 1;
const OPAQUE_MAX: u64 = ORDERING_BATCH_CAPACITY as u64;

/// Appends the hop feedback tail `[up_high varint][up_bits u64]
/// [echo_delay varint]` (spec §3.3, §3.4; compaction spec §7.2, §7.3).
pub(crate) fn append_feedback(payload: &mut Vec<u8>, feedback: HopFeedback) {
    varint::write(feedback.up.high, payload);
    payload.extend_from_slice(&feedback.up.bits.to_le_bytes());
    varint::write(feedback.echo_delay_ms.into(), payload);
}

/// Reads the feedback tail from `reader`.
pub(crate) fn read_feedback(reader: &mut Reader<'_>) -> Result<HopFeedback, RelayError> {
    Ok(HopFeedback {
        up: RecvWindow {
            high: reader.varint()?,
            bits: reader.u64()?,
        },
        echo_delay_ms: reader.varint_u16()?,
    })
}

/// A cursor over a message: fixed-width little-endian fields and §8
/// varints, each `Malformed` past the end.
pub(crate) struct Reader<'a>(pub(crate) &'a [u8]);

impl<'a> Reader<'a> {
    pub(crate) fn take(&mut self, length: usize) -> Result<&'a [u8], RelayError> {
        if self.0.len() < length {
            return Err(RelayError::Malformed);
        }
        let (head, tail) = self.0.split_at(length);
        self.0 = tail;
        Ok(head)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, RelayError> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn u16(&mut self) -> Result<u16, RelayError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub(crate) fn u32(&mut self) -> Result<u32, RelayError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub(crate) fn u64(&mut self) -> Result<u64, RelayError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub(crate) fn varint(&mut self) -> Result<u64, RelayError> {
        let (value, length) = varint::read(self.0)?;
        self.0 = &self.0[length..];
        Ok(value)
    }
    /// A varint field that kept its former `u32` width (§7.1).
    pub(crate) fn varint_u32(&mut self) -> Result<u32, RelayError> {
        u32::try_from(self.varint()?).map_err(|_| RelayError::Malformed)
    }
    /// A varint field that kept its former `u16` width (§7.1).
    pub(crate) fn varint_u16(&mut self) -> Result<u16, RelayError> {
        u16::try_from(self.varint()?).map_err(|_| RelayError::Malformed)
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet_uplink<'a>(recv_next: u64, high: u64, bits: u64) -> Uplink<'a> {
        Uplink {
            flags: FLAG_PUSH,
            recv_next,
            down: RecvWindow { high, bits },
            srtt_ms: 40,
            base_ms: 40,
            rttvar_ms: 11,
            link: 0,
            checksums: Vec::new(),
            pace: None,
            first_batch_id: 0,
            batches: Vec::new(),
        }
    }

    /// Compaction spec §7.1: a UPLINK without batches, 18 B (35 B before).
    #[test]
    fn uplinks_encode_their_integers_as_varints() {
        let uplink = quiet_uplink(1, 7, 0x7F);
        let mut bytes = Vec::new();
        uplink.encode(&mut bytes);
        #[rustfmt::skip]
        let expected = [
            8, FLAG_PUSH, 1, 7, 0x7F, 0, 0, 0, 0, 0, 0, 0, 40, 40, 11, 0, 0, 0,
        ];
        assert_eq!(bytes, expected);
        assert_eq!(decode_uplink(&bytes).unwrap(), uplink);
        let opaque = [0xAB; 74];
        let mut uplink = quiet_uplink(300, 8, 0xFF);
        uplink.first_batch_id = 1;
        uplink.batches = vec![&opaque];
        uplink.checksums = vec![(15, 9)];
        let mut bytes = Vec::new();
        uplink.encode(&mut bytes);
        assert_eq!(bytes[2..4], [0xAC, 0x02], "recv_next 300");
        assert_eq!(bytes.len(), 18 + 5 + 1 + 1 + 1 + 74);
        assert_eq!(decode_uplink(&bytes).unwrap(), uplink);
    }

    #[test]
    fn fit_keeps_the_leading_batches_within_the_limit() {
        let opaque = [1; 300];
        let mut uplink = quiet_uplink(1, 0, 0);
        uplink.first_batch_id = 1;
        uplink.batches = vec![&opaque, &opaque, &opaque];
        uplink.fit(crate::MAX_WIRE_PAYLOAD);
        assert_eq!(uplink.batches.len(), 2);
        let mut bytes = Vec::new();
        uplink.encode(&mut bytes);
        assert!(bytes.len() <= crate::MAX_WIRE_PAYLOAD);
        uplink.fit(bytes.len() - 1);
        assert_eq!(uplink.batches.len(), 1);
    }

    #[test]
    fn every_malformed_rule_rejects_the_uplink() {
        let opaque = [1; 41];
        let mut uplink = quiet_uplink(1, 7, 0x7F);
        uplink.first_batch_id = 1;
        uplink.batches = vec![&opaque];
        let mut good = Vec::new();
        uplink.encode(&mut good);
        // [8][flags][recv_next 2][high 3][bits 4..12][40][40][11][link 15]
        // [checksums 16][batches 17][first id 18][len 19][opaque 20..]
        assert!(decode_uplink(&good).is_ok());
        let mutate = |change: &dyn Fn(&mut Vec<u8>)| {
            let mut bytes = good.clone();
            change(&mut bytes);
            decode_uplink(&bytes).map(|_| ())
        };
        let malformed = Err(RelayError::Malformed);
        // Exact length.
        assert_eq!(mutate(&|b| b.push(0)), malformed);
        assert_eq!(
            mutate(&|b| {
                b.pop();
            }),
            malformed
        );
        // Counts and reserved bits.
        assert_eq!(mutate(&|b| b[16] = 3), malformed);
        assert_eq!(mutate(&|b| b[17] = 5), malformed);
        assert_eq!(mutate(&|b| b[1] |= 0x10), malformed);
        assert_eq!(mutate(&|b| b[15] |= 0x10), malformed);
        assert!(mutate(&|b| b[15] |= 0x08).is_ok(), "bit3 JITTERY");
        // first_batch_id ≥ 1; batch length bounds.
        assert_eq!(mutate(&|b| b[18] = 0), malformed);
        assert_eq!(mutate(&|b| b[19] = 0), malformed);
        // recv_next ≥ 1; bit0 of down_bits when down_high > 0.
        assert_eq!(mutate(&|b| b[2] = 0), malformed);
        assert_eq!(mutate(&|b| b[4] = 0x7E), malformed);
        // Varints: shortest, within the field's former width (§7.1, §8).
        assert_eq!(
            mutate(&|b| b.splice(12..13, [0xA8, 0x00]).for_each(drop)),
            malformed
        );
        assert_eq!(
            mutate(&|b| b.splice(12..13, [0x80, 0x80, 0x04]).for_each(drop)),
            malformed,
            "srtt past u16"
        );
    }

    #[test]
    fn feedback_round_trips_with_varints() {
        let feedback = HopFeedback {
            up: RecvWindow {
                high: 8,
                bits: 0x7F,
            },
            echo_delay_ms: 300,
        };
        let mut ack = vec![2, 1, 5];
        append_feedback(&mut ack, feedback);
        assert_eq!(ack, [2, 1, 5, 8, 0x7F, 0, 0, 0, 0, 0, 0, 0, 0xAC, 0x02]);
        let mut reader = Reader(&ack[3..]);
        assert_eq!(read_feedback(&mut reader), Ok(feedback));
        assert!(reader.is_empty());
    }
}
