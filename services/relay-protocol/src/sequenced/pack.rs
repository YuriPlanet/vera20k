//! PACK v2 of the bandwidth compaction
//! (`services/relay/bandwidth-compaction/spec.md` §5): the relay's wire
//! form of consecutive canonical records. It changes only the wire; every
//! client rebuilds byte for byte the canonical records PACK v1 carries, and
//! each pack decodes alone, whatever arrived before it (§5.1).
//!
//! ```text
//! [12][base_seq varint][base_frame varint][count u8] {entry} × count
//! ```
//!
//! Canonical records, the input of `encode_pack_v2` and the output of
//! `decode_pack_v2`, are the unchanged layouts (little-endian):
//!
//! | kind | canonical record |
//! |---|---|
//! | BATCH 3 | `[3][seq u64][frame u32][source u8][batch_id u64][P]` |
//! | FRAME_END 4 | `[4][seq u64][frame u32]` |
//! | DEPARTURE 5 | `[5][seq u64][frame u32][source u8]`, `[1]` appended when lost |
//! | PAUSE 9 | `[9][seq u64][frame u32][epoch u64][paused u8][source u8]` |
//! | RATE 10 | `[10][seq u64][frame u32][fps u16][source u8]` |
//! | DESYNC 11 | `[11][seq u64][frame u32][sample_frame u32][outlier u8][checksum u32][other u8][checksum u32]` |

use super::{
    BATCH_EVENT, DEPARTURE_EVENT, DESYNC_EVENT, FRAME_END_EVENT, ORDERING_BATCH_CAPACITY,
    PACK_EVENT, PAUSE_EVENT, RATE_EVENT,
};
use crate::envelope::PACK_LIMIT;
use crate::{RelayError, varint};

/// The canonical prefix: kind, sequence, frame.
const CANONICAL_HEAD: usize = 13;
const SAME_FRAME: u8 = 0;
const NEXT_FRAME: u8 = 1 << 4;
const FRAME_DELTA: u8 = 2 << 4;
const FRAME_CODE: u8 = 3 << 4;
const NEXT_BATCH_ID: u8 = 1 << 6;
const RESERVED: u8 = 1 << 7;

/// The state an entry is written against: the previous entry's frame and
/// each source's last BATCH ID in this pack (§5.2).
struct Context {
    frame: u32,
    // (source, last ID), at most one per source; a pack has few sources.
    batch_ids: Vec<(u8, u64)>,
}

impl Context {
    fn new(frame: u32) -> Self {
        Self {
            frame,
            batch_ids: Vec::new(),
        }
    }

    fn last_id(&self, source: u8) -> Option<u64> {
        self.batch_ids
            .iter()
            .find(|(known, _)| *known == source)
            .map(|(_, id)| *id)
    }

    fn set_last_id(&mut self, source: u8, id: Option<u64>) {
        let at = self
            .batch_ids
            .iter()
            .position(|(known, _)| *known == source);
        match (at, id) {
            (Some(at), Some(id)) => self.batch_ids[at].1 = id,
            (Some(at), None) => {
                self.batch_ids.swap_remove(at);
            }
            (None, Some(id)) => self.batch_ids.push((source, id)),
            (None, None) => {}
        }
    }

    /// Whether `id` may and so must be written as bit6 (§5.3 rule 6).
    fn follows(&self, source: u8, id: u64) -> bool {
        self.last_id(source).and_then(|last| last.checked_add(1)) == Some(id)
    }
}

fn record_frame(record: &[u8]) -> u32 {
    u32::from_le_bytes(record[9..13].try_into().expect("canonical record"))
}

/// What `write_entry` changed in a `Context`, to take back.
struct Undo {
    frame: u32,
    batch: Option<(u8, Option<u64>)>,
}

impl Context {
    fn undo(&mut self, undo: Undo) {
        self.frame = undo.frame;
        if let Some((source, id)) = undo.batch {
            self.set_last_id(source, id);
        }
    }
}

/// Appends `record`'s entry against `context` and advances it.
fn write_entry(record: &[u8], context: &mut Context, out: &mut Vec<u8>) -> Undo {
    let kind = record[0];
    let frame = record_frame(record);
    let mut undo = Undo {
        frame: context.frame,
        batch: None,
    };
    let body = &record[CANONICAL_HEAD..];
    debug_assert!(frame >= context.frame, "PACK records go forward in frames");
    let delta = frame - context.frame;
    context.frame = frame;
    let mut tag = kind
        | match delta {
            0 => SAME_FRAME,
            1 => NEXT_FRAME,
            _ => FRAME_DELTA,
        };
    let batch = (kind == BATCH_EVENT).then(|| {
        let id = u64::from_le_bytes(body[1..9].try_into().expect("canonical batch"));
        (body[0], id)
    });
    let follows = batch.is_some_and(|(source, id)| context.follows(source, id));
    if follows {
        tag |= NEXT_BATCH_ID;
    }
    out.push(tag);
    if delta >= 2 {
        varint::write(u64::from(delta), out);
    }
    match kind {
        BATCH_EVENT => {
            let (source, id) = batch.expect("a batch");
            out.push(source);
            if !follows {
                varint::write(id, out);
            }
            let contents = &body[9..];
            varint::write(contents.len() as u64, out);
            out.extend_from_slice(contents);
            undo.batch = Some((source, context.last_id(source)));
            context.set_last_id(source, Some(id));
        }
        FRAME_END_EVENT => {}
        DEPARTURE_EVENT => out.extend_from_slice(&[body[0], u8::from(body.len() == 2)]),
        PAUSE_EVENT => {
            varint::write(
                u64::from_le_bytes(body[..8].try_into().expect("canonical pause")),
                out,
            );
            out.extend_from_slice(&body[8..]);
        }
        // RATE and DESYNC travel as their canonical bodies.
        _ => out.extend_from_slice(body),
    }
    undo
}

/// Packs consecutive canonical records into PACK v2 payloads (§5.2, §5.4),
/// at most `datagrams` of them. Returns each payload with the number of
/// records it carries, in order; their counts sum to the records packed,
/// a prefix of `records`. Empty when `records` is empty or `datagrams` is 0.
///
/// The packing is greedy, each payload filled while
/// the next record's v2 size fits `envelope::PACK_LIMIT` (681 bytes) and
/// `count` stays at most 255. Every record fits a pack alone.
///
/// The encoding is the shortest and only one (§5.4), so tests may compare
/// bytes: `base_frame` is the first record's frame and its entry uses frame
/// code 0; each later entry uses frame code 0 for the same frame, 1 for the
/// next and 2 with a varint `d ≥ 2` otherwise; a BATCH sets bit6 whenever
/// its ID is one more than the previous BATCH of its source in this pack;
/// every varint is shortest (§8).
///
/// `records` must be consecutive committed history: sequences `+1` apart,
/// frames non-decreasing, kinds and layouts as in the module table with
/// `P` of 1..=`ORDERING_BATCH_CAPACITY` bytes. The relay's history only
/// holds such records; anything else is a caller bug (debug assertion).
pub fn encode_pack_v2(records: &[Vec<u8>], datagrams: usize) -> Vec<(Vec<u8>, usize)> {
    let records: Vec<&[u8]> = records.iter().map(Vec::as_slice).collect();
    pack(&records, datagrams, true)
}

/// Whether `encode_pack_v2` packs all of `records` into at most `limit`
/// PACKs, found without building them and stopping past `limit` (the
/// piggyback's packet count, transport spec §7.4).
pub(crate) fn fits_packs_v2(records: &[&[u8]], limit: usize) -> bool {
    let packs = pack(records, limit.saturating_add(1), false);
    packs.len() <= limit
}

/// The greedy packing of `encode_pack_v2`; unless `build`, every pack
/// comes back empty with only its record count.
fn pack(records: &[&[u8]], datagrams: usize, build: bool) -> Vec<(Vec<u8>, usize)> {
    let mut packs = Vec::new();
    let mut next = 0;
    let mut entry = Vec::with_capacity(PACK_LIMIT);
    let mut header = Vec::with_capacity(1 + 2 * varint::MAX_LEN + 1);
    while next < records.len() && packs.len() < datagrams {
        let first = records[next];
        header.clear();
        header.push(PACK_EVENT);
        varint::write(
            u64::from_le_bytes(first[1..9].try_into().expect("canonical record")),
            &mut header,
        );
        varint::write(u64::from(record_frame(first)), &mut header);
        header.push(0);
        let mut pack = if build { header.clone() } else { Vec::new() };
        let mut length = header.len();
        let mut context = Context::new(record_frame(first));
        let mut count = 0;
        while next < records.len() && count < usize::from(u8::MAX) {
            entry.clear();
            // A record that does not fit is written again, against the next
            // pack's fresh context.
            let undo = write_entry(records[next], &mut context, &mut entry);
            if length + entry.len() > PACK_LIMIT {
                debug_assert!(count > 0, "every record fits a pack alone");
                context.undo(undo);
                break;
            }
            length += entry.len();
            if build {
                pack.extend_from_slice(&entry);
            }
            count += 1;
            next += 1;
        }
        if build {
            pack[header.len() - 1] = count as u8;
        }
        packs.push((pack, count));
    }
    packs
}

/// The first sequence and record count of a PACK v2 this relay wrote,
/// from its header alone (spec §5.2); `None` if the header is malformed.
pub(crate) fn pack_v2_range(pack: &[u8]) -> Option<(u64, u8)> {
    if pack.first() != Some(&PACK_EVENT) {
        return None;
    }
    let mut reader = Reader {
        bytes: pack,
        offset: 1,
    };
    let base = reader.varint().ok()?;
    reader.varint().ok()?;
    Some((base, reader.byte().ok()?))
}

/// Reads one PACK v2 payload (from its `12` byte) into its canonical
/// records, in order: record `i` has sequence `base_seq + i` (§5.2). It
/// reads nothing but `payload` (§5.1).
///
/// `Err(RelayError::Malformed)`, discarding the whole pack, on any breach
/// of §5.3:
/// 1. a frame below the previous entry's (the first entry's previous is
///    `base_frame`), or frame code 3;
/// 2. bit6 on a BATCH with no earlier BATCH of the same source in this
///    pack;
/// 3. a varint that breaks §8 (`varint::read`);
/// 4. `count` 0, fewer entries than `count`, or bytes after the last;
/// 5. a `P` outside 1..=512 bytes, or a payload over
///    `envelope::PACK_LIMIT` (681 bytes);
/// 6. an encoding other than the shortest: a first entry with frame code
///    other than 0, frame code 2 with `d` < 2, bit6 on a kind other than
///    BATCH, or bit6 = 0 where bit6 = 1 was possible;
/// 7. a frame past `u32::MAX` or a sequence (`base_seq + i`) past
///    `u64::MAX`.
///
/// Also Malformed: a first byte other than 12, a kind other than
/// 3, 4, 5, 9, 10, 11, tag bit7 set, or DEPARTURE `lost` other than 0 or
/// 1 (§5.2).
///
/// So every accepted pack is exactly what `encode_pack_v2` writes for its
/// records.
pub fn decode_pack_v2(payload: &[u8]) -> Result<Vec<Vec<u8>>, RelayError> {
    if payload.first() != Some(&PACK_EVENT) || payload.len() > PACK_LIMIT {
        return Err(RelayError::Malformed);
    }
    let mut reader = Reader {
        bytes: payload,
        offset: 1,
    };
    let base_seq = reader.varint()?;
    let base_frame = u32::try_from(reader.varint()?).map_err(|_| RelayError::Malformed)?;
    let count = reader.byte()?;
    if count == 0 {
        return Err(RelayError::Malformed);
    }
    let mut context = Context::new(base_frame);
    let mut records = Vec::with_capacity(usize::from(count));
    for index in 0..u64::from(count) {
        let tag = reader.byte()?;
        let kind = tag & 0x0F;
        let next_id = tag & NEXT_BATCH_ID != 0;
        if tag & RESERVED != 0 || (next_id && kind != BATCH_EVENT) {
            return Err(RelayError::Malformed);
        }
        let frame = match tag & FRAME_CODE {
            SAME_FRAME => Some(context.frame),
            _ if index == 0 => None,
            NEXT_FRAME => context.frame.checked_add(1),
            FRAME_DELTA => match reader.varint()? {
                delta @ 2.. => u32::try_from(delta)
                    .ok()
                    .and_then(|delta| context.frame.checked_add(delta)),
                _ => None,
            },
            _ => None,
        }
        .ok_or(RelayError::Malformed)?;
        context.frame = frame;
        let sequence = base_seq.checked_add(index).ok_or(RelayError::Malformed)?;
        // The longest tail but a batch's: DESYNC's 14 bytes.
        let mut record = Vec::with_capacity(CANONICAL_HEAD + 14);
        record.push(kind);
        record.extend_from_slice(&sequence.to_le_bytes());
        record.extend_from_slice(&frame.to_le_bytes());
        match kind {
            BATCH_EVENT => {
                let source = reader.byte()?;
                let id = if next_id {
                    context
                        .last_id(source)
                        .and_then(|last| last.checked_add(1))
                        .ok_or(RelayError::Malformed)?
                } else {
                    let id = reader.varint()?;
                    if context.follows(source, id) {
                        return Err(RelayError::Malformed);
                    }
                    id
                };
                let length =
                    usize::try_from(reader.varint()?).map_err(|_| RelayError::Malformed)?;
                if !(1..=ORDERING_BATCH_CAPACITY).contains(&length) {
                    return Err(RelayError::Malformed);
                }
                let contents = reader.take(length)?;
                record.reserve(9 + length);
                record.push(source);
                record.extend_from_slice(&id.to_le_bytes());
                record.extend_from_slice(contents);
                context.set_last_id(source, Some(id));
            }
            FRAME_END_EVENT => {}
            DEPARTURE_EVENT => {
                let source = reader.byte()?;
                match reader.byte()? {
                    0 => record.push(source),
                    1 => record.extend_from_slice(&[source, 1]),
                    _ => return Err(RelayError::Malformed),
                }
            }
            PAUSE_EVENT => {
                record.extend_from_slice(&reader.varint()?.to_le_bytes());
                record.extend_from_slice(reader.take(2)?);
            }
            RATE_EVENT => record.extend_from_slice(reader.take(3)?),
            DESYNC_EVENT => record.extend_from_slice(reader.take(14)?),
            _ => return Err(RelayError::Malformed),
        }
        records.push(record);
    }
    if reader.offset != payload.len() {
        return Err(RelayError::Malformed);
    }
    Ok(records)
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], RelayError> {
        let bytes = self
            .bytes
            .get(self.offset..self.offset + length)
            .ok_or(RelayError::Malformed)?;
        self.offset += length;
        Ok(bytes)
    }

    fn byte(&mut self) -> Result<u8, RelayError> {
        Ok(self.take(1)?[0])
    }

    fn varint(&mut self) -> Result<u64, RelayError> {
        let (value, length) = varint::read(&self.bytes[self.offset..])?;
        self.offset += length;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(kind: u8, seq: u64, frame: u32, body: &[u8]) -> Vec<u8> {
        let mut record = vec![kind];
        record.extend_from_slice(&seq.to_le_bytes());
        record.extend_from_slice(&frame.to_le_bytes());
        record.extend_from_slice(body);
        record
    }

    fn batch(seq: u64, frame: u32, source: u8, id: u64, contents: &[u8]) -> Vec<u8> {
        let mut body = vec![source];
        body.extend_from_slice(&id.to_le_bytes());
        body.extend_from_slice(contents);
        record(BATCH_EVENT, seq, frame, &body)
    }

    /// One record of each kind, frames by the same, next and a delta.
    fn mixed() -> Vec<Vec<u8>> {
        vec![
            batch(300, 40, 2, 9, &[0xAB]),
            batch(301, 40, 2, 10, &[0xCD, 0xEF]),
            batch(302, 40, 5, 10, &[0x01]),
            record(FRAME_END_EVENT, 303, 40, &[]),
            record(DEPARTURE_EVENT, 304, 41, &[5, 1]),
            record(DEPARTURE_EVENT, 305, 41, &[6]),
            record(PAUSE_EVENT, 306, 41, &[3, 0, 0, 0, 0, 0, 0, 0, 1, 2]),
            record(RATE_EVENT, 307, 45, &[30, 0, 255]),
            record(
                DESYNC_EVENT,
                308,
                45,
                &[15, 0, 0, 0, 1, 1, 2, 3, 4, 2, 5, 6, 7, 8],
            ),
        ]
    }

    #[test]
    fn writes_the_shortest_encoding_and_decodes_it_back_byte_for_byte() {
        let records = mixed();
        let packs = encode_pack_v2(&records, 4);
        assert_eq!(packs.len(), 1);
        let (pack, count) = &packs[0];
        assert_eq!(*count, records.len());
        #[rustfmt::skip]
        let expected = [
            PACK_EVENT, 0xAC, 0x02, 40, 9,
            0x03, 2, 9, 1, 0xAB,
            0x43, 2, 2, 0xCD, 0xEF,
            0x03, 5, 10, 1, 0x01,
            0x04,
            0x15, 5, 1,
            0x05, 6, 0,
            0x09, 3, 1, 2,
            0x2A, 4, 30, 0, 255,
            0x0B, 15, 0, 0, 0, 1, 1, 2, 3, 4, 2, 5, 6, 7, 8,
        ];
        assert_eq!(pack[..], expected);
        assert_eq!(decode_pack_v2(pack), Ok(records));
    }

    #[test]
    fn splits_at_the_packing_limit_and_restarts_context() {
        let records: Vec<_> = (0..3)
            .map(|index| batch(1 + index, 7, 1, 20 + index, &[0x55; 300]))
            .collect();
        let packs = encode_pack_v2(&records, 8);
        assert_eq!(
            packs.iter().map(|(_, count)| *count).collect::<Vec<_>>(),
            [2, 1]
        );
        assert!(packs.iter().all(|(pack, _)| pack.len() <= PACK_LIMIT));
        // The second pack's first batch has no earlier batch: absolute ID.
        assert_eq!(packs[1].0[..7], [PACK_EVENT, 3, 7, 1, 0x03, 1, 22]);
        let decoded: Vec<_> = packs
            .iter()
            .flat_map(|(pack, _)| decode_pack_v2(pack).unwrap())
            .collect();
        assert_eq!(decoded, records);
        assert_eq!(encode_pack_v2(&records, 1).len(), 1);
    }

    #[test]
    fn fitting_counts_the_packs_the_encoder_makes() {
        let records: Vec<_> = (0..5)
            .map(|index| batch(1 + index, 7, 1, 20 + index, &[0x55; 300]))
            .collect();
        let refs: Vec<&[u8]> = records.iter().map(Vec::as_slice).collect();
        let packs = encode_pack_v2(&records, usize::MAX).len();
        assert_eq!(packs, 3);
        assert!(fits_packs_v2(&refs, packs));
        assert!(!fits_packs_v2(&refs, packs - 1));
        assert!(fits_packs_v2(&refs[1..], 2));
    }

    #[test]
    fn refuses_each_breach_of_the_decoding_rules() {
        let pack = |entries: &[u8]| {
            let mut pack = vec![PACK_EVENT, 1, 10];
            pack.extend_from_slice(entries);
            pack
        };
        for (rule, bytes) in [
            ("frame code 3", pack(&[1, 0x34])),
            ("first entry not code 0", pack(&[1, 0x14])),
            ("delta below 2", pack(&[2, 0x04, 0x24, 1])),
            ("bit6 without an earlier batch", pack(&[1, 0x43, 2, 1, 0])),
            ("bit6 off a batch", pack(&[1, 0x44])),
            (
                "bit6 possible but absent",
                pack(&[2, 0x03, 2, 4, 1, 0, 0x03, 2, 5, 1, 0]),
            ),
            ("non-shortest varint", pack(&[1, 0x03, 2, 0x84, 0x00, 1, 0])),
            ("count 0", pack(&[0])),
            ("fewer entries than count", pack(&[2, 0x04])),
            ("trailing bytes", pack(&[1, 0x04, 0])),
            ("empty batch", pack(&[1, 0x03, 2, 4, 0])),
            ("unknown kind", pack(&[1, 0x06])),
            ("reserved bit7", pack(&[1, 0x84])),
            ("lost 2", pack(&[1, 0x05, 3, 2])),
            (
                "frame past u32",
                vec![PACK_EVENT, 1, 0xFF, 0xFF, 0xFF, 0xFF, 0x0F, 2, 0x04, 0x14],
            ),
            (
                "base frame past u32",
                vec![PACK_EVENT, 1, 0x80, 0x80, 0x80, 0x80, 0x10, 1, 0x04],
            ),
            (
                "sequence past u64",
                vec![
                    PACK_EVENT, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01, 0, 2,
                    0x04, 0x04,
                ],
            ),
        ] {
            assert_eq!(decode_pack_v2(&bytes), Err(RelayError::Malformed), "{rule}");
        }
        let mut oversized = pack(&[1, 0x03, 2, 4, 0x80, 0x04]);
        oversized.extend_from_slice(&[0; 512]);
        assert_eq!(
            decode_pack_v2(&oversized).map(|records| records.len()),
            Ok(1)
        );
        oversized[7] = 0x81;
        oversized.push(0);
        assert_eq!(
            decode_pack_v2(&oversized),
            Err(RelayError::Malformed),
            "P 513"
        );
    }
}
