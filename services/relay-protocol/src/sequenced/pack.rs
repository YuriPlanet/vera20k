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
//!
//! Milestone C1: interface only.
#![allow(unused_variables)]

use crate::RelayError;

/// Packs consecutive canonical records into PACK v2 payloads (§5.2, §5.4),
/// at most `datagrams` of them. Returns each payload with the number of
/// records it carries, in order; their counts sum to the records packed,
/// a prefix of `records`. Empty when `records` is empty or `datagrams` is 0.
///
/// The packing is today's `pack_entries`: greedy, each payload filled while
/// the next record's v2 size fits `envelope::MAX_PAYLOAD` (681 bytes) and
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
    todo!("C4: compaction spec §5.2, §5.4")
}

/// Reads one PACK v2 payload (from its `12` byte) into its canonical
/// records, in order: record `i` has sequence `base_seq + i` (§5.2). It
/// reads nothing but `payload` (§5.1).
///
/// `Err(RelayError::Malformed)`, discarding the whole pack, on any breach
/// of §5.3:
/// 1. a frame below the previous entry's (the first entry's previous is
///    `base_frame`), frame code 3, or a frame or sequence beyond its
///    canonical width (`u32`, `u64`);
/// 2. bit6 on a BATCH with no earlier BATCH of the same source in this
///    pack;
/// 3. a varint that breaks §8 (`varint::read`);
/// 4. `count` 0, fewer entries than `count`, or bytes after the last;
/// 5. a `P` outside 1..=512 bytes, or a payload over
///    `envelope::MAX_PAYLOAD`.
///
/// Also Malformed: a first byte other than 12, a kind other than
/// 3, 4, 5, 9, 10, 11, tag bit7 set, or DEPARTURE `lost` other than 0 or
/// 1 (§5.2).
///
/// Pending (not settled by §5.2-§5.4): whether the decoder also refuses
/// encodings the encoder never writes: bit6 on a kind other than BATCH,
/// frame code 2 with `d` < 2, a first entry with frame code 1 or 2, or
/// bit6 = 0 where bit6 = 1 was possible.
pub fn decode_pack_v2(payload: &[u8]) -> Result<Vec<Vec<u8>>, RelayError> {
    todo!("C4: compaction spec §5.2, §5.3")
}
