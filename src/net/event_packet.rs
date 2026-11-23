//! Active-retail Event packet codec: Compress64B660, cache64B4D0 and
//! Uncompress64BDD0. The session owns these caches; Simulation keeps the only
//! OutList/DoList. Inputs and mirror records are transient transfer values.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::panic, clippy::unreachable)
)]

use std::collections::BTreeMap;

use crate::sim::command::{
    ADD_PLAYER_OPCODE, COMMAND_RECORD_LEN, CommandRecord, FRAME_SYNC_OPCODE,
    MEGAMISSION_ACTION_OFFSET, MEGAMISSION_DESTINATION_KIND_OFFSET, MEGAMISSION_OPCODE,
    MEGAMISSION_OWNED_PAYLOAD_LEN, MEGAMISSION_SECONDARY_VALUE_OFFSET,
    MEGAMISSION_SOURCE_KIND_OFFSET, RESPONSE_TIME_OPCODE, event_body_len,
};
use crate::sim::command_queue::{FRAMEINFO_EVENT_OPCODE, FrameInfo};
use crate::util::read_helpers::{array_at, read_i32_le};

/// Session constructor69768C writes +2808 (A8DA40), passed by64764C.
pub const NATIVE_EVENT_PACKET_CAPACITY: usize = 0x22e;
pub const EVENT_PACKET_HEADER_LEN: usize = 14;
const HEADER_LEN: usize = EVENT_PACKET_HEADER_LEN;
const CACHE_SLOTS: usize = 8;
const MAX_RUN: usize = 128;
/// A run's per-actor token: the MEGAMISSION source value and kind that
/// start the payload.
const ACTOR_TOKEN_LEN: usize = MEGAMISSION_SOURCE_KIND_OFFSET + 1;
type ActorToken = [u8; ACTOR_TOKEN_LEN];
/// A run's cache slot rides in the MEGAMISSION padding byte at Event+0D.
const RUN_SLOT_PAYLOAD_OFFSET: usize = 6;

/// VERA clients extend the native body with a full command frontier. The
/// prefix is `VCM`, version1, then a little-endian u64. It stays inside the
/// existing private reliable payload limit; native codec fixtures omit it.
pub const LIVE_EVENT_PREFIX_LEN: usize = 12;
const LIVE_EVENT_MAGIC: [u8; 4] = *b"VCM\x01";

#[derive(Debug, Clone, Copy)]
pub enum EventPacketHeader {
    Native(FrameInfo),
    Vera {
        frame_info: FrameInfo,
        command_frontier: u64,
    },
}

impl From<FrameInfo> for EventPacketHeader {
    fn from(value: FrameInfo) -> Self {
        Self::Native(value)
    }
}

impl EventPacketHeader {
    fn frame_info(self) -> FrameInfo {
        match self {
            Self::Native(info)
            | Self::Vera {
                frame_info: info, ..
            } => info,
        }
    }

    fn bytes(self, opcode: u8) -> Vec<u8> {
        let mut bytes = Vec::new();
        if let Self::Vera {
            command_frontier, ..
        } = self
        {
            bytes.extend_from_slice(&LIVE_EVENT_MAGIC);
            bytes.extend_from_slice(&command_frontier.to_le_bytes());
        }
        bytes.extend_from_slice(&self.frame_info().encode().as_bytes()[..HEADER_LEN]);
        let header_start = bytes.len() - HEADER_LEN;
        bytes[header_start] = opcode;
        bytes
    }
}

pub fn live_command_frontier(bytes: &[u8]) -> Option<u64> {
    let (frontier, body) = bytes
        .strip_prefix(&LIVE_EVENT_MAGIC)?
        .split_first_chunk::<{ LIVE_EVENT_PREFIX_LEN - LIVE_EVENT_MAGIC.len() }>()?;
    (body.len() >= HEADER_LEN && matches!(body[0], FRAMEINFO_EVENT_OPCODE | FRAME_SYNC_OPCODE))
        .then(|| u64::from_le_bytes(*frontier))
}

fn body_offset(bytes: &[u8]) -> usize {
    if bytes.starts_with(&LIVE_EVENT_MAGIC) {
        LIVE_EVENT_PREFIX_LEN
    } else {
        0
    }
}

/// Original64A000's probe uses the FRAMEINFO header layout with opcode19.
/// `checksum` carries Scenario's random seed (A8E7A8) here; `timing_word`
/// carries the probe sequence. The shared send policy supplies event_frame
/// from current frame + MaxAhead, and delay is MaxAhead's low byte.
pub fn encode_frame_sync(info: FrameInfo) -> Result<[u8; HEADER_LEN], PacketEncodeError> {
    if info.house_id < 0 {
        return Err(PacketEncodeError::InvalidHouse);
    }
    let mut bytes = array_at::<0, HEADER_LEN, _>(info.encode().as_bytes());
    bytes[0] = FRAME_SYNC_OPCODE;
    Ok(bytes)
}

/// Live probes carry the same frontier as ordinary packets. Otherwise a raw
/// probe could grant frame credit while overtaking every command packet.
pub fn encode_live_frame_sync(
    info: FrameInfo,
    command_frontier: u64,
) -> Result<Vec<u8>, PacketEncodeError> {
    if info.house_id < 0 {
        return Err(PacketEncodeError::InvalidHouse);
    }
    Ok(EventPacketHeader::Vera {
        frame_info: info,
        command_frontier,
    }
    .bytes(FRAME_SYNC_OPCODE))
}

/// Inspect the leading header without admitting it to the command queue.
pub fn decode_frame_info(bytes: &[u8]) -> Option<FrameInfo> {
    decode_header(bytes, FRAMEINFO_EVENT_OPCODE)
}

/// Probe fields have the same layout as FRAMEINFO, but its checksum field is
/// a random seed.64BDD0 skips this header instead of admitting an Event.
pub fn decode_frame_sync(bytes: &[u8]) -> Option<FrameInfo> {
    decode_header(bytes, FRAME_SYNC_OPCODE)
}

fn decode_header(bytes: &[u8], opcode: u8) -> Option<FrameInfo> {
    let bytes = bytes.get(body_offset(bytes)..)?;
    if bytes.len() < HEADER_LEN || bytes[0] != opcode {
        return None;
    }
    let mut record = [0; COMMAND_RECORD_LEN];
    record[..HEADER_LEN].copy_from_slice(&bytes[..HEADER_LEN]);
    record[0] = FRAMEINFO_EVENT_OPCODE;
    FrameInfo::decode(&CommandRecord::decode_exact(&record).expect("fixed native Event"))
}

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
    #[error("event packet capacity {0} cannot hold its header")]
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
    /// VERA packets have one authenticated source/header. An embedded header
    /// could otherwise change the House of later decoded commands.
    UnexpectedHeader,
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
            .saturating_sub(usize::from(decode_frame_info(packet).is_some()))
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
    /// Residual: the queues do not yet retain local issue/admission timestamps
    /// for ExecuteDoList64C6FD's debug latency display. They are not wire bytes.
    pub fn encode_packet(
        &mut self,
        records: &[&CommandRecord],
        header: impl Into<EventPacketHeader>,
        current_frame: i32,
        capacity: usize,
    ) -> Result<EncodedEventPacket, PacketEncodeError> {
        let packet_header = header.into();
        let header = packet_header.frame_info();
        let header_bytes = packet_header.bytes(FRAMEINFO_EVENT_OPCODE);
        if capacity < header_bytes.len() {
            return Err(PacketEncodeError::HeaderCapacity(capacity));
        }
        if header.house_id < 0 {
            return Err(PacketEncodeError::InvalidHouse);
        }
        if records.len() > MAX_RUN {
            return Err(PacketEncodeError::TooManyRecords);
        }
        let mut packet = EncodedEventPacket {
            bytes: header_bytes,
            mirrors: Vec::new(),
            blocked: None,
        };
        let mut run: Option<OpenRun> = None;
        for (index, &source) in records.iter().enumerate() {
            let opcode = source.opcode();
            let payload_len = event_body_len(opcode);
            let unsupported = match opcode {
                ADD_PLAYER_OPCODE => Some(EncodeBlock::ExtendedPayload),
                FRAMEINFO_EVENT_OPCODE | FRAME_SYNC_OPCODE => {
                    Some(EncodeBlock::PacketHeaderOpcode(opcode))
                }
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
                ACTOR_TOKEN_LEN
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
                    .extend_from_slice(&source.payload()[..MEGAMISSION_OWNED_PAYLOAD_LEN]);
                run = Some(OpenRun {
                    count_offset,
                    payload_offset,
                    actors: vec![actor_token(source)],
                    previous: source.clone(),
                    force_new: false,
                });
            } else {
                packet.bytes.push(opcode);
                let start = usize::from(opcode == RESPONSE_TIME_OPCODE) * 6;
                let len = usize::from(payload_len.expect("checked opcode"));
                packet
                    .bytes
                    .extend_from_slice(&source.payload()[start..start + len]);
            }
        }
        self.finish_run(&mut packet.bytes, run, current_frame);
        Ok(packet)
    }

    /// A live send commits cache state only after all reliable destinations
    /// accept the packet. Snapshot only the eight send slots; cloning received
    /// peer caches per packet would multiply a large match's memory traffic.
    pub(crate) fn encode_packet_if_accepted(
        &mut self,
        records: &[&CommandRecord],
        header: impl Into<EventPacketHeader>,
        current_frame: i32,
        capacity: usize,
        accept: impl FnOnce(&EncodedEventPacket) -> bool,
    ) -> Result<Option<EncodedEventPacket>, PacketEncodeError> {
        let saved_send = self.send.clone();
        match self.encode_packet(records, header, current_frame, capacity) {
            Ok(packet) if accept(&packet) => Ok(Some(packet)),
            Ok(_) => {
                self.send = saved_send;
                Ok(None)
            }
            Err(error) => {
                self.send = saved_send;
                Err(error)
            }
        }
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
            bytes.truncate(run.payload_offset + MEGAMISSION_OWNED_PAYLOAD_LEN);
            bytes[run.count_offset] = 0;
            bytes[run.payload_offset + RUN_SLOT_PAYLOAD_OFFSET] = slot as u8;
            return;
        }
        let slot = self.replacement_slot(frame);
        self.send[slot] = SendCacheSlot {
            actors: run.actors,
            assigned_frame: frame,
        };
        bytes[run.payload_offset + RUN_SLOT_PAYLOAD_OFFSET] = slot as u8;
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
        let header_offset = body_offset(bytes);
        if bytes.len() < header_offset + HEADER_LEN {
            return Ok(report);
        }
        report.consumed_bytes = header_offset;
        let source_house = bytes[header_offset + 2] as i8;
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
            if start != header_offset && bytes[start..].starts_with(&LIVE_EVENT_MAGIC) {
                return Err(decode_error(
                    bytes,
                    start,
                    report,
                    DecodeFailureKind::UnexpectedHeader,
                ));
            }
            let Some(len) = event_body_len(opcode) else {
                return Err(decode_error(
                    bytes,
                    start,
                    report,
                    DecodeFailureKind::UnsupportedOpcode(opcode),
                ));
            };
            if opcode == ADD_PLAYER_OPCODE {
                return Err(decode_error(
                    bytes,
                    start,
                    report,
                    DecodeFailureKind::ExtendedPayload,
                ));
            }
            let wire_len = match opcode {
                FRAME_SYNC_OPCODE | FRAMEINFO_EVENT_OPCODE => HEADER_LEN,
                MEGAMISSION_OPCODE => 2 + MEGAMISSION_OWNED_PAYLOAD_LEN,
                _ => 1 + usize::from(len),
            };
            if bytes.len() - start < wire_len {
                //64C325..64C370 returns the accepted prefix on a short next
                //fixed record. It does not report DoList capacity failure.
                break;
            }
            // Our encoder emits exactly one leading header. Reject a second
            // one before it can replace the authenticated source's House.
            // This is transport hardening, not a legacy wire requirement.
            if start != header_offset
                && matches!(opcode, FRAME_SYNC_OPCODE | FRAMEINFO_EVENT_OPCODE)
            {
                return Err(decode_error(
                    bytes,
                    start,
                    report,
                    DecodeFailureKind::UnexpectedHeader,
                ));
            }
            if opcode == FRAME_SYNC_OPCODE {
                report.consumed_bytes += HEADER_LEN;
                continue;
            }
            if opcode == FRAMEINFO_EVENT_OPCODE {
                house = bytes[start + 2] as i8;
                if house < 0 {
                    return Err(decode_error(
                        bytes,
                        start,
                        report,
                        DecodeFailureKind::InvalidHouse,
                    ));
                }
                // The FRAMEINFO header's wire length covered this stamp.
                let Some(stamp) = read_i32_le(bytes, start + 3) else {
                    break;
                };
                frame = stamp;
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
                record.payload_mut()[..MEGAMISSION_OWNED_PAYLOAD_LEN]
                    .copy_from_slice(&bytes[start + 2..start + 2 + MEGAMISSION_OWNED_PAYLOAD_LEN]);
                let slot = record.payload()[RUN_SLOT_PAYLOAD_OFFSET];
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
                    let trailing = (usize::from(count) - 1) * ACTOR_TOKEN_LEN;
                    if bytes.len() - start < wire_len + trailing {
                        return Err(decode_error(
                            bytes,
                            start,
                            report,
                            DecodeFailureKind::TruncatedRun,
                        ));
                    }
                    let mut actors = vec![actor_token(&record)];
                    actors.extend_from_slice(
                        bytes[start + wire_len..start + wire_len + trailing]
                            .as_chunks::<ACTOR_TOKEN_LEN>()
                            .0,
                    );
                    actors
                };
                let mut cache = (count > 1).then(|| {
                    let cache =
                        &mut self.receive.entry(source_house).or_default()[usize::from(slot)];
                    *cache = vec![actors[0]];
                    cache
                });
                for (index, actor) in actors.iter().enumerate() {
                    if index > 0
                        && let Some(cache) = cache.as_mut()
                    {
                        //64C057..64C091 updates the next cached actor after
                        //the preceding admission, before attempting this one.
                        cache.push(*actor);
                    }
                    record.payload_mut()[..ACTOR_TOKEN_LEN].copy_from_slice(actor);
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
                        (usize::from(count) - 1) * ACTOR_TOKEN_LEN
                    } else {
                        0
                    };
            } else {
                let payload_start = if opcode == FRAMEINFO_EVENT_OPCODE {
                    start + 7
                } else {
                    start + 1
                };
                let destination = usize::from(opcode == RESPONSE_TIME_OPCODE) * 6;
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
    record.payload_field::<0, ACTOR_TOKEN_LEN>()
}

fn same_mission(left: &CommandRecord, right: &CommandRecord) -> bool {
    //64B73F..64B798 compares mission, Target and Destination only. It does
    //not compare the auxiliary token, planning flag or reserved byte.
    fn compared(record: &CommandRecord) -> (u8, &[u8]) {
        let payload = record.payload();
        (
            payload[MEGAMISSION_ACTION_OFFSET],
            &payload[MEGAMISSION_SECONDARY_VALUE_OFFSET..=MEGAMISSION_DESTINATION_KIND_OFFSET],
        )
    }
    compared(left) == compared(right)
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

#[cfg(all(test, tests_net))]
#[path = "event_packet_tests.rs"]
mod tests;
