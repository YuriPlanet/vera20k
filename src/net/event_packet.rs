//! Active-retail Event packet codec: Compress64B660, cache64B4D0 and
//! Uncompress64BDD0. The session owns these caches; Simulation keeps the only
//! OutList/DoList. Inputs and mirror records are transient transfer values.

use std::collections::BTreeMap;

use crate::sim::command::{COMMAND_RECORD_LEN, CommandRecord, MEGAMISSION_OPCODE};
use crate::sim::command_queue::FrameInfo;

/// Session constructor69768C writes +2808 (A8DA40), passed by64764C.
pub const NATIVE_EVENT_PACKET_CAPACITY: usize = 0x22e;
pub const EVENT_PACKET_HEADER_LEN: usize = 14;
const HEADER_LEN: usize = EVENT_PACKET_HEADER_LEN;
const CACHE_SLOTS: usize = 8;
const MAX_RUN: usize = 128;
const FRAME_SYNC: u8 = 0x19;
const FRAME_INFO: u8 = 0x1c;
const EXTENDED: u8 = 0x1f;
const MEGA_LEN: usize = 23;
type ActorToken = [u8; 5];

/// Original64A000's probe uses the FRAMEINFO header layout with opcode19.
/// `checksum` carries Scenario's random seed (A8E7A8) here; `timing_word`
/// carries the probe sequence. The shared send policy supplies event_frame
/// from current frame + MaxAhead, and delay is MaxAhead's low byte.
pub fn encode_frame_sync(info: FrameInfo) -> Result<[u8; HEADER_LEN], PacketEncodeError> {
    if info.house_id < 0 {
        return Err(PacketEncodeError::InvalidHouse);
    }
    let mut bytes: [u8; HEADER_LEN] = info.encode().as_bytes()[..HEADER_LEN]
        .try_into()
        .expect("fixed packet header");
    bytes[0] = FRAME_SYNC;
    Ok(bytes)
}

/// Inspect the leading header without admitting it to the command queue.
pub fn decode_frame_info(bytes: &[u8]) -> Option<FrameInfo> {
    decode_header(bytes, FRAME_INFO)
}

/// Probe fields have the same layout as FRAMEINFO, but its checksum field is
/// a random seed.64BDD0 skips this header instead of admitting an Event.
pub fn decode_frame_sync(bytes: &[u8]) -> Option<FrameInfo> {
    decode_header(bytes, FRAME_SYNC)
}

fn decode_header(bytes: &[u8], opcode: u8) -> Option<FrameInfo> {
    if bytes.len() < HEADER_LEN || bytes[0] != opcode {
        return None;
    }
    let mut record = [0; COMMAND_RECORD_LEN];
    record[..HEADER_LEN].copy_from_slice(&bytes[..HEADER_LEN]);
    record[0] = FRAME_INFO;
    FrameInfo::decode(&CommandRecord::decode_exact(&record).expect("fixed native Event"))
}

/// Original byte table8208EC, indexed by the 47 Event opcodes (0..2E).
/// RESPONSE_TIME1B starts at Event+0D; ADDPLAYER1F carries external storage.
const PAYLOAD_LENGTHS: [u8; 47] = [
    0, 5, 5, 4, 23, 24, 5, 5, 0, 5, 5, 16, 0, 4, 12, 12, 12, 5, 8, 0, 16, 5, 5, 4, 4, 0, 0, 1, 7,
    0, 10, 4, 5, 2, 0, 4, 4, 104, 64, 0, 4, 5, 10, 0, 5, 4, 12,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeBlock {
    Capacity,
    /// ADDPLAYER's Event+7 pointer is process-local. Its separately owned
    /// trailing bytes must be supplied by that producer before it can travel.
    ExtendedPayload,
    UnsupportedOpcode(u8),
    /// FRAMEINFO/FRAMESYNC are packet headers, not ordinary OutList members.
    PacketHeaderOpcode(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PacketEncodeError {
    #[error("event packet capacity {0} cannot hold its 14-byte header")]
    HeaderCapacity(usize),
    #[error("event packet requires a nonnegative registered source House")]
    InvalidHouse,
    #[error("event packet input exceeds the 128-record native OutList")]
    TooManyRecords,
}

/// Even a blocked suffix retains the successfully encoded prefix and every
/// corresponding mirror. Apply these once before handling `blocked`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedEventPacket {
    pub bytes: Vec<u8>,
    pub mirrors: Vec<CommandRecord>,
    pub blocked: Option<EncodeBlock>,
}

impl EncodedEventPacket {
    pub fn consumed_records(&self) -> usize {
        self.mirrors.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeFailureKind {
    AdmissionFull,
    ExtendedPayload,
    UnsupportedOpcode(u8),
    InvalidHouse,
    InvalidCacheSlot(u8),
    MissingCachedRun(u8),
    InvalidRunLength(u8),
    /// Native lacks a complete check for compressed run tails. Reject an
    /// invalid tail safely; ordinary truncated fixed records keep its prefix.
    TruncatedRun,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("event packet decode stopped at byte {offset}: {kind:?}")]
pub struct PacketDecodeError {
    pub kind: DecodeFailureKind,
    pub offset: usize,
    pub admitted_records: usize,
    /// Includes the stopped record; do not retry previously admitted records.
    /// In particular ADDPLAYER bytes survive without exposing a native pointer.
    pub remainder: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecodedEventPacket {
    /// Native64BDD0 counts expanded records, including FRAMEINFO. The caller
    /// 64A563 subtracts one only when the packet starts with FRAMEINFO.
    pub admitted_records: usize,
    pub consumed_bytes: usize,
}

impl DecodedEventPacket {
    pub fn command_count(self, packet: &[u8]) -> usize {
        self.admitted_records
            .saturating_sub(usize::from(packet.first() == Some(&FRAME_INFO)))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SendCacheSlot {
    actors: Vec<ActorToken>,
    assigned_frame: i32,
}

/// One connection session's compression state. Sender slots are shared across
/// its packets; incoming slots belong to each source House. No command waits
/// here for execution or retransmission.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventPacketCodec {
    send: [SendCacheSlot; CACHE_SLOTS],
    receive: BTreeMap<i8, [Vec<ActorToken>; CACHE_SLOTS]>,
}

struct OpenRun {
    count_offset: usize,
    payload_offset: usize,
    actors: Vec<ActorToken>,
    previous: CommandRecord,
    force_new: bool,
}

impl EventPacketCodec {
    /// ProcessNetwork64787B..6478CB clears both cache directions at reset.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Build one packet from a borrowed OutList prefix. `header.event_frame`
    /// is already stamped by the shared send policy; `current_frame` separately
    /// ages compression slots. The caller bounds this slice by the native
    /// 2000h local-mirror allowance and commits/removes only `mirrors.len()`.
    /// Local timestamps remain with that existing queue owner.
    pub fn encode_packet(
        &mut self,
        records: &[&CommandRecord],
        header: FrameInfo,
        current_frame: i32,
        capacity: usize,
    ) -> Result<EncodedEventPacket, PacketEncodeError> {
        if capacity < HEADER_LEN {
            return Err(PacketEncodeError::HeaderCapacity(capacity));
        }
        if header.house_id < 0 {
            return Err(PacketEncodeError::InvalidHouse);
        }
        if records.len() > MAX_RUN {
            return Err(PacketEncodeError::TooManyRecords);
        }
        let mut packet = EncodedEventPacket {
            bytes: header.encode().as_bytes()[..HEADER_LEN].to_vec(),
            mirrors: Vec::new(),
            blocked: None,
        };
        let mut run: Option<OpenRun> = None;
        for (index, &source) in records.iter().enumerate() {
            let opcode = source.opcode();
            let payload_len = PAYLOAD_LENGTHS.get(usize::from(opcode)).copied();
            let unsupported = match opcode {
                EXTENDED => Some(EncodeBlock::ExtendedPayload),
                FRAME_INFO | FRAME_SYNC => Some(EncodeBlock::PacketHeaderOpcode(opcode)),
                _ if payload_len.is_none() => Some(EncodeBlock::UnsupportedOpcode(opcode)),
                _ => None,
            };
            if let Some(reason) = unsupported {
                self.finish_run(&mut packet.bytes, run.take(), current_frame);
                packet.blocked = Some(reason);
                break;
            }
            let extend = run.as_ref().is_some_and(|run| {
                opcode == MEGAMISSION_OPCODE
                    && !run.force_new
                    && same_mission(&run.previous, source)
            });
            if !extend {
                //64B856..64B924 finalizes the preceding run before checking
                // whether the next record fits. Never discard that prefix.
                self.finish_run(&mut packet.bytes, run.take(), current_frame);
            }
            let required = if extend {
                5
            } else {
                1 + usize::from(payload_len.expect("checked opcode"))
                    + usize::from(opcode == MEGAMISSION_OPCODE)
            };
            if required > capacity.saturating_sub(packet.bytes.len()) {
                self.finish_run(&mut packet.bytes, run.take(), current_frame);
                packet.blocked = Some(EncodeBlock::Capacity);
                break;
            }
            //64B97A..64BA6A: stamp the original before its full 6F-byte
            // mirror, then encode. Only bit0 of flags is cleared.
            let mut mirror = source.clone();
            mirror.set_house_id(header.house_id);
            mirror.set_frame_stamp(header.event_frame);
            mirror.clear_processed();
            packet.mirrors.push(mirror);
            if extend {
                let run = run.as_mut().expect("extended run");
                let actor = actor_token(source);
                packet.bytes.extend_from_slice(&actor);
                run.actors.push(actor);
                packet.bytes[run.count_offset] = run.actors.len() as u8;
                run.previous = source.clone();
                //64BCxx probes each growing actor list. An early exact cache
                // hit closes this run before the next same-mission actor.
                run.force_new =
                    index + 1 < records.len() && self.matching_slot(&run.actors).is_some();
            } else if opcode == MEGAMISSION_OPCODE {
                packet.bytes.push(opcode);
                let count_offset = packet.bytes.len();
                packet.bytes.push(1);
                let payload_offset = packet.bytes.len();
                packet
                    .bytes
                    .extend_from_slice(&source.payload()[..MEGA_LEN]);
                run = Some(OpenRun {
                    count_offset,
                    payload_offset,
                    actors: vec![actor_token(source)],
                    previous: source.clone(),
                    force_new: false,
                });
            } else {
                packet.bytes.push(opcode);
                let start = usize::from(opcode == 0x1b) * 6;
                let len = usize::from(payload_len.expect("checked opcode"));
                packet
                    .bytes
                    .extend_from_slice(&source.payload()[start..start + len]);
            }
        }
        self.finish_run(&mut packet.bytes, run, current_frame);
        Ok(packet)
    }

    fn matching_slot(&self, actors: &[ActorToken]) -> Option<usize> {
        (actors.len() > 1)
            .then(|| self.send.iter().position(|slot| slot.actors == actors))
            .flatten()
    }

    fn finish_run(&mut self, bytes: &mut Vec<u8>, run: Option<OpenRun>, frame: i32) {
        let Some(run) = run.filter(|run| run.actors.len() > 1) else {
            return;
        };
        if let Some(slot) = self.matching_slot(&run.actors) {
            //64B596..64B5CF removes only the repeated trailing actor tokens.
            bytes.truncate(run.payload_offset + MEGA_LEN);
            bytes[run.count_offset] = 0;
            bytes[run.payload_offset + 6] = slot as u8;
            return;
        }
        let slot = self.replacement_slot(frame);
        self.send[slot] = SendCacheSlot {
            actors: run.actors,
            assigned_frame: frame,
        };
        bytes[run.payload_offset + 6] = slot as u8;
    }

    fn replacement_slot(&self, frame: i32) -> usize {
        //64B56E..64B5DF: first zero timestamp, otherwise greatest signed
        //wrapping age. A hit does not refresh this assignment timestamp.
        let mut selected = 0;
        let mut oldest = -1;
        for (index, slot) in self.send.iter().enumerate() {
            if slot.assigned_frame == 0 {
                return index;
            }
            let age = frame.wrapping_sub(slot.assigned_frame);
            if age > oldest {
                selected = index;
                oldest = age;
            }
        }
        selected
    }

    /// Decode directly into the caller's existing admission owner. Failure
    /// retains cache mutations and previously admitted records, matching
    ///64BFE8/64C191/64C27F. No temporary DoList or rollback is created.
    pub fn decode_packet(
        &mut self,
        bytes: &[u8],
        mut admit: impl FnMut(CommandRecord) -> bool,
    ) -> Result<DecodedEventPacket, PacketDecodeError> {
        let mut report = DecodedEventPacket::default();
        if bytes.len() < HEADER_LEN {
            return Ok(report);
        }
        let source_house = bytes[2] as i8;
        if source_house < 0 {
            return Err(decode_error(
                bytes,
                0,
                report,
                DecodeFailureKind::InvalidHouse,
            ));
        }
        let mut house = 0;
        let mut frame = 0;
        while report.consumed_bytes < bytes.len() {
            let start = report.consumed_bytes;
            let opcode = bytes[start];
            let Some(&len) = PAYLOAD_LENGTHS.get(usize::from(opcode)) else {
                return Err(decode_error(
                    bytes,
                    start,
                    report,
                    DecodeFailureKind::UnsupportedOpcode(opcode),
                ));
            };
            if opcode == EXTENDED {
                return Err(decode_error(
                    bytes,
                    start,
                    report,
                    DecodeFailureKind::ExtendedPayload,
                ));
            }
            let wire_len = match opcode {
                FRAME_SYNC | FRAME_INFO => HEADER_LEN,
                MEGAMISSION_OPCODE => 2 + MEGA_LEN,
                _ => 1 + usize::from(len),
            };
            if bytes.len() - start < wire_len {
                //64C325..64C370 returns the accepted prefix on a short next
                //fixed record. It does not report DoList capacity failure.
                break;
            }
            if opcode == FRAME_SYNC {
                report.consumed_bytes += HEADER_LEN;
                continue;
            }
            if opcode == FRAME_INFO {
                house = bytes[start + 2] as i8;
                if house < 0 {
                    return Err(decode_error(
                        bytes,
                        start,
                        report,
                        DecodeFailureKind::InvalidHouse,
                    ));
                }
                frame = i32::from_le_bytes(bytes[start + 3..start + 7].try_into().unwrap());
            }
            let mut record = CommandRecord::encode(opcode, i32::from(house), frame, &[])
                .expect("native table fits an Event payload");
            if opcode == MEGAMISSION_OPCODE {
                let count = bytes[start + 1];
                if usize::from(count) > MAX_RUN {
                    return Err(decode_error(
                        bytes,
                        start,
                        report,
                        DecodeFailureKind::InvalidRunLength(count),
                    ));
                }
                record.payload_mut()[..MEGA_LEN]
                    .copy_from_slice(&bytes[start + 2..start + 2 + MEGA_LEN]);
                let slot = record.payload()[6];
                if count != 1 && usize::from(slot) >= CACHE_SLOTS {
                    return Err(decode_error(
                        bytes,
                        start,
                        report,
                        DecodeFailureKind::InvalidCacheSlot(slot),
                    ));
                }
                let actors = if count == 0 {
                    let cached = self
                        .receive
                        .get(&source_house)
                        .map(|slots| &slots[usize::from(slot)]);
                    let Some(cached) = cached.filter(|actors| !actors.is_empty()) else {
                        return Err(decode_error(
                            bytes,
                            start,
                            report,
                            DecodeFailureKind::MissingCachedRun(slot),
                        ));
                    };
                    cached.clone()
                } else {
                    let trailing = (usize::from(count) - 1) * 5;
                    if bytes.len() - start < wire_len + trailing {
                        return Err(decode_error(
                            bytes,
                            start,
                            report,
                            DecodeFailureKind::TruncatedRun,
                        ));
                    }
                    let mut actors = vec![actor_token(&record)];
                    actors.extend(
                        bytes[start + wire_len..start + wire_len + trailing]
                            .chunks_exact(5)
                            .map(|bytes| <[u8; 5]>::try_from(bytes).unwrap()),
                    );
                    actors
                };
                if count > 1 {
                    self.receive.entry(source_house).or_default()[usize::from(slot)] =
                        vec![actors[0]];
                }
                for (index, actor) in actors.iter().enumerate() {
                    if count > 1 && index > 0 {
                        //64C057..64C091 updates the next cached actor after
                        //the preceding admission, before attempting this one.
                        self.receive.get_mut(&source_house).unwrap()[usize::from(slot)]
                            .push(*actor);
                    }
                    record.payload_mut()[..5].copy_from_slice(actor);
                    if !admit(record.clone()) {
                        return Err(decode_error(
                            bytes,
                            start,
                            report,
                            DecodeFailureKind::AdmissionFull,
                        ));
                    }
                    report.admitted_records += 1;
                }
                report.consumed_bytes += wire_len
                    + if count > 1 {
                        (usize::from(count) - 1) * 5
                    } else {
                        0
                    };
            } else {
                let payload_start = if opcode == FRAME_INFO {
                    start + 7
                } else {
                    start + 1
                };
                let destination = usize::from(opcode == 0x1b) * 6;
                record.payload_mut()[destination..destination + usize::from(len)]
                    .copy_from_slice(&bytes[payload_start..payload_start + usize::from(len)]);
                if !admit(record) {
                    return Err(decode_error(
                        bytes,
                        start,
                        report,
                        DecodeFailureKind::AdmissionFull,
                    ));
                }
                report.admitted_records += 1;
                report.consumed_bytes += wire_len;
            }
        }
        Ok(report)
    }
}

fn actor_token(record: &CommandRecord) -> ActorToken {
    record.payload()[..5]
        .try_into()
        .expect("five-byte actor token")
}

fn same_mission(left: &CommandRecord, right: &CommandRecord) -> bool {
    //64B73F..64B798 compares mission, Target and Destination only. It does
    //not compare the auxiliary token, planning flag or reserved byte.
    left.payload()[5] == right.payload()[5] && left.payload()[7..17] == right.payload()[7..17]
}

fn decode_error(
    bytes: &[u8],
    offset: usize,
    report: DecodedEventPacket,
    kind: DecodeFailureKind,
) -> PacketDecodeError {
    PacketDecodeError {
        kind,
        offset,
        admitted_records: report.admitted_records,
        remainder: bytes[offset..].to_vec(),
    }
}

#[cfg(test)]
#[path = "event_packet_tests.rs"]
mod tests;
