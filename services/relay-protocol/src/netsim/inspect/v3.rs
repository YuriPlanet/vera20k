//! The compacted wire (services/relay/bandwidth-compaction/spec.md, "the
//! compaction spec" below), read and written from the spec alone: envelope
//! v3 (§6.1, §6.3), varints (§8), the varint ordering messages (§7), PACK v2
//! (§5.2-§5.4) and plaintext batches (§3.1). Nothing here calls production
//! code, so judgments can hold the production wire against it.

use hmac::{Hmac, Mac};
use sha2::Sha256;

use super::{Datagram, Detail, FLAG_PACE, Feedback, PackRecord, Uplink};

/// `hdr` bits 5..7 (§6.1).
pub const VERSION: u8 = 3;
/// `HOP_TAG_LEN` (§6.4): the truncated HMAC.
pub const HOP_TAG_LEN: usize = 4;
/// `[hdr u8][session u32][seq u32]` (§6.1).
pub const HEADER: usize = 9;
/// §6.1 "信封负载上限：720 B".
pub const MAX_PAYLOAD: usize = 720;
/// §5.3 rule 5, §5.4: the packing cap stays 681 B.
pub const PACK_CAP: usize = 681;
/// Fixed bytes of every v3 datagram, either way (§6.1, §13: 41 B with the
/// 28 B IPv4/UDP header).
pub const OVERHEAD: usize = HEADER + HOP_TAG_LEN;
/// §6.3: only `CLIENT_DOMAIN` / `SERVER_DOMAIN` ("VERA20k relay client
/// v2\0", services/relay-protocol/src/lib.rs) change their version to v3;
/// the hop key stays `derive_packet_key`. The spec spells no bytes; this
/// literal reading is reported.
pub const CLIENT_DOMAIN: &[u8] = b"VERA20k relay client v3\0";
pub const SERVER_DOMAIN: &[u8] = b"VERA20k relay server v3\0";
/// §3.1: `1 <= |P| <= 512` (`ORDERING_BATCH_CAPACITY`).
pub const BATCH_MAX: usize = 512;
const TICKET_LEN: usize = 98;
const COOKIE_LEN: usize = 40;

// Client kinds (§6.1).
pub const HELLO: u8 = 0;
pub const CONFIRM: u8 = 1;
pub const DATA: u8 = 2;
pub const PING: u8 = 3;
pub const CONTROL: u8 = 4;
// Relay kinds (§6.1).
pub const CHALLENGE: u8 = 0;
pub const READY: u8 = 1;
pub const LEAVE_ACK: u8 = 5;

// --- §8 varints ---

/// Unsigned LEB128, shortest form, at most 10 bytes, the 10th only 0 or 1,
/// not past the end (§8).
pub fn read_varint(bytes: &[u8], at: &mut usize) -> Result<u64, String> {
    let mut value = 0u64;
    for index in 0..10 {
        let byte = *bytes.get(*at).ok_or("varint past the end")?;
        *at += 1;
        if index == 9 && byte > 1 {
            return Err("varint tenth byte above 1".into());
        }
        value |= u64::from(byte & 0x7F) << (7 * index);
        if byte & 0x80 == 0 {
            if index > 0 && byte == 0 {
                return Err("varint not shortest".into());
            }
            return Ok(value);
        }
    }
    Err("varint longer than 10 bytes".into())
}

pub fn write_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push(value as u8 | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

pub fn varint_len(value: u64) -> usize {
    let mut out = Vec::new();
    write_varint(&mut out, value);
    out.len()
}

/// A cursor that fails instead of panicking on short input.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], at: usize) -> Self {
        Self { bytes, at }
    }
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| format!("{what} past the end"))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }
    fn u8(&mut self, what: &str) -> Result<u8, String> {
        Ok(self.take(1, what)?[0])
    }
    fn u16(&mut self, what: &str) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2, what)?.try_into().unwrap()))
    }
    fn u32(&mut self, what: &str) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4, what)?.try_into().unwrap()))
    }
    fn u64(&mut self, what: &str) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8, what)?.try_into().unwrap()))
    }
    fn varint(&mut self, what: &str) -> Result<u64, String> {
        read_varint(self.bytes, &mut self.at).map_err(|error| format!("{what}: {error}"))
    }
    /// A varint whose field keeps its transport-spec width (§7.1 "取值范围
    /// 同传输方案 §3.2").
    fn varint_max(&mut self, max: u64, what: &str) -> Result<u64, String> {
        let value = self.varint(what)?;
        if value > max {
            return Err(format!("{what} {value} above {max}"));
        }
        Ok(value)
    }
    fn end(&self, what: &str) -> Result<(), String> {
        if self.at != self.bytes.len() {
            return Err(format!("{what} trailing bytes"));
        }
        Ok(())
    }
}

// --- §6.1 envelope ---

/// `(kind, session, seq, payload)` of a v3 datagram (§6.1). UDP carries the
/// length, so the payload is everything between the header and the tag.
fn envelope(bytes: &[u8]) -> Result<(u8, u32, u32, &[u8]), String> {
    if bytes.len() < OVERHEAD || bytes[0] >> 5 != VERSION {
        return Err("v3 envelope".into());
    }
    let payload = &bytes[HEADER..bytes.len() - HOP_TAG_LEN];
    if payload.len() > MAX_PAYLOAD {
        return Err("v3 payload above 720 B (§6.1)".into());
    }
    Ok((
        bytes[0] & 0x1F,
        u32::from_le_bytes(bytes[1..5].try_into().unwrap()),
        u32::from_le_bytes(bytes[5..9].try_into().unwrap()),
        payload,
    ))
}

fn datagram(kind: &'static str, session: u32, seq: u32, length: usize, detail: Detail) -> Datagram {
    Datagram {
        kind,
        hop_seq: u64::from(seq),
        session,
        payload_len: length,
        detail,
    }
}

/// A client v3 datagram (§6.1): HELLO and CONFIRM carry session 0 and the
/// ticket in their payload; every other kind the hop's nonzero session
/// (§6.2). CONTROL carries the ordering requests with no target byte; the
/// requests the transport spec removed (§1.2: 1, 4, 6, 7) are refused.
pub fn client_datagram(bytes: &[u8]) -> Result<Datagram, String> {
    let (kind, session, seq, payload) = envelope(bytes)?;
    let length = payload.len();
    let mut detail = Detail::None;
    let name = match kind {
        HELLO if length == TICKET_LEN && session == 0 => "HELLO",
        CONFIRM if length >= TICKET_LEN + COOKIE_LEN && session == 0 => "CONFIRM",
        DATA if length >= 2 && session != 0 => "DATA",
        PING if length == 0 && session != 0 => "PING",
        CONTROL if length > 0 && session != 0 => match payload[0] {
            // [0][certificate 66 | 67][agreement 32] (§7.4: fixed).
            0 if matches!(length, 99 | 100) => {
                detail = Detail::ReadyRequest;
                "READY_REQUEST"
            }
            // [2][next varint] (§7.4).
            2 => {
                let mut reader = Reader::new(payload, 1);
                let next = reader.varint("PULL next")?;
                reader.end("PULL")?;
                detail = Detail::Pull { next };
                "PULL"
            }
            3 if length == 1 => "LEAVE",
            // [5][request_id varint][expected_epoch varint][paused u8] (§7.4).
            5 => {
                let mut reader = Reader::new(payload, 1);
                reader.varint("PAUSE request_id")?;
                reader.varint("PAUSE epoch")?;
                if reader.u8("PAUSE paused")? > 1 {
                    return Err("PAUSE paused".into());
                }
                reader.end("PAUSE")?;
                "PAUSE"
            }
            8 => {
                detail = Detail::Uplink(uplink(payload)?);
                "UPLINK"
            }
            request => return Err(format!("v3 client request {request} length {length}")),
        },
        kind => {
            return Err(format!(
                "v3 client kind {kind} session {session} length {length}"
            ));
        }
    };
    Ok(datagram(name, session, seq, length, detail))
}

/// UPLINK (§7.1) with the transport spec's §3.2 decoding rules 1-3, 5 and
/// 6, and §3.1's batch bounds `1 <= len <= 512` in place of rule 4's
/// opaque bounds (the §7.1 wording keeps rule 4; reported).
fn uplink(payload: &[u8]) -> Result<Uplink, String> {
    let fail = |rule: &str| Err(format!("UPLINK v3 {rule}"));
    let mut reader = Reader::new(payload, 1);
    let flags = reader.u8("flags")?;
    let recv_next = reader.varint("recv_next")?;
    let down_high = reader.varint("down_high")?;
    let down_bits = reader.u64("down_bits")?;
    let srtt_ms = reader.varint_max(u16::MAX.into(), "srtt_ms")? as u16;
    let base_ms = reader.varint_max(u16::MAX.into(), "base_ms")? as u16;
    let rttvar_ms = reader.varint_max(u16::MAX.into(), "rttvar_ms")? as u16;
    let link = reader.u8("link")?;
    if flags & 0xF0 != 0 || link & 0xF0 != 0 {
        return fail("reserved bits");
    }
    if recv_next == 0 {
        return fail("recv_next 0");
    }
    if down_high > 0 && down_bits & 1 == 0 {
        return fail("down_bits bit0");
    }
    if down_high == 0 && down_bits != 0 {
        return fail("down_bits without down_high");
    }
    let checksum_count = reader.u8("checksum_count")?;
    if checksum_count > 2 {
        return fail("checksum_count");
    }
    let mut checksums = Vec::new();
    for _ in 0..checksum_count {
        let frame = reader.varint_max(u32::MAX.into(), "checksum frame")? as u32;
        checksums.push((frame, reader.u32("checksum")?));
    }
    let pace = if flags & FLAG_PACE != 0 {
        let pace = reader.u8("pace")?;
        let fps = reader.varint_max(u16::MAX.into(), "fps")? as u16;
        let tick = reader.varint_max(u32::MAX.into(), "tick_micros")? as u32;
        Some((pace, fps, tick))
    } else {
        None
    };
    let batch_count = reader.u8("batch_count")?;
    if batch_count > 4 {
        return fail("batch_count");
    }
    let mut first_batch_id = 0;
    let mut batches = Vec::new();
    if batch_count > 0 {
        first_batch_id = reader.varint("first_batch_id")?;
        if first_batch_id == 0 {
            return fail("first_batch_id 0");
        }
        for _ in 0..batch_count {
            let len = reader.varint("batch len")?;
            if !(1..=BATCH_MAX as u64).contains(&len) {
                return fail("batch len");
            }
            reader.take(len as usize, "batch")?;
            batches.push(len as usize);
        }
    }
    reader.end("UPLINK v3")?;
    Ok(Uplink {
        flags,
        recv_next,
        down_high,
        down_bits,
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

/// `[up_high varint][up_bits u64][echo_delay varint]` (§7.2, §7.3). The
/// transport spec caps `echo_delay` at the u16 maximum (§3.3); §7 restates
/// no cap, so the u16 range is assumed (reported).
fn feedback(reader: &mut Reader) -> Result<Feedback, String> {
    Ok(Feedback {
        high: reader.varint("up_high")?,
        bits: reader.u64("up_bits")?,
        echo_delay_ms: reader.varint_max(u16::MAX.into(), "echo_delay")? as u16,
    })
}

/// A relay v3 datagram (§6.1). CHALLENGE carries session 0 and echoes the
/// HELLO's sequence; LEAVE_ACK echoes the LEAVE's. READY is empty: the one
/// answering a CONFIRM carries the new session in its envelope and the
/// hop's first relay sequence, 1, so `seq == 1` tells it from a PING's
/// reply. Every kind but CHALLENGE carries the hop's nonzero session.
pub fn server_datagram(bytes: &[u8]) -> Result<Datagram, String> {
    let (kind, session, seq, payload) = envelope(bytes)?;
    let length = payload.len();
    let mut detail = Detail::None;
    let name = match kind {
        CHALLENGE if length == COOKIE_LEN && session == 0 => "CHALLENGE",
        READY if length == 0 && session != 0 => {
            if seq == 1 {
                detail = Detail::HopReady { session };
            }
            "READY"
        }
        DATA if length >= 2 && session != 0 => "DATA",
        LEAVE_ACK if length == 0 && session != 0 => "LEAVE_ACK",
        CONTROL if length > 0 && session != 0 => {
            let mut reader = Reader::new(payload, 1);
            match payload[0] {
                // [0][ready_mask u32][sources_mask u32]: no u64 (§7.4).
                0 if length == 9 => {
                    detail = Detail::OrderReady {
                        ready_mask: reader.u32("ready_mask")?,
                        sources_mask: reader.u32("sources_mask")?,
                    };
                    "O_READY"
                }
                // §7.3.
                1 => {
                    let head_frame = reader.varint_max(u32::MAX.into(), "head_frame")? as u32;
                    let oldest_seq = reader.varint("oldest_seq")?;
                    let next_seq = reader.varint("next_seq")?;
                    let active_sources = reader.u32("active_sources")?;
                    let tick_hz = reader.u16("tick_hz")?;
                    let start_delay_ms =
                        reader.varint_max(u32::MAX.into(), "start_delay_ms")? as u32;
                    let started = match reader.u8("started")? {
                        0 => false,
                        1 => true,
                        _ => return Err("STATUS started".into()),
                    };
                    let feedback = feedback(&mut reader)?;
                    reader.end("STATUS v3")?;
                    detail = Detail::Status {
                        head_frame,
                        oldest_seq,
                        next_seq,
                        active_sources,
                        tick_hz,
                        start_delay_ms,
                        started,
                        feedback,
                    };
                    "STATUS"
                }
                // §7.2.
                2 => {
                    let batch_id = reader.varint("accepted_through")?;
                    let frame = reader.varint_max(u32::MAX.into(), "frame")? as u32;
                    let feedback = feedback(&mut reader)?;
                    reader.end("ACK v3")?;
                    detail = Detail::Ack {
                        batch_id,
                        frame,
                        feedback,
                    };
                    "ACK"
                }
                // [6][code u8][expected varint] (§7.4).
                6 => {
                    let code = reader.u8("code")?;
                    let expected = reader.varint("expected")?;
                    reader.end("ERROR v3")?;
                    detail = Detail::Error { code, expected };
                    "ERROR"
                }
                // [8][supported u32][active u32][request_id varint]
                // [epoch varint][paused u8][frame u32] (§7.4: u64 fields).
                8 => {
                    reader.u32("supported")?;
                    reader.u32("active")?;
                    reader.varint("request_id")?;
                    reader.varint("epoch")?;
                    if reader.u8("paused")? > 1 {
                        return Err("PAUSE_STATUS paused".into());
                    }
                    reader.u32("frame")?;
                    reader.end("PAUSE_STATUS v3")?;
                    "PAUSE_STATUS"
                }
                12 => {
                    let (base, count, records) = decode_pack(payload)?;
                    detail = Detail::Pack {
                        base,
                        count,
                        records,
                    };
                    "PACK"
                }
                event => return Err(format!("v3 server event {event} length {length}")),
            }
        }
        kind => {
            return Err(format!(
                "v3 server kind {kind} session {session} length {length}"
            ));
        }
    };
    Ok(datagram(name, session, seq, length, detail))
}

// --- §6.3 tag ---

fn mac(key: &[u8; 32], from_client: bool, signed: &[u8]) -> [u8; HOP_TAG_LEN] {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC key");
    mac.update(if from_client {
        CLIENT_DOMAIN
    } else {
        SERVER_DOMAIN
    });
    mac.update(signed);
    mac.finalize().into_bytes()[..HOP_TAG_LEN]
        .try_into()
        .unwrap()
}

/// `tag = HMAC-SHA256(hop key, domain ‖ hdr ‖ session ‖ seq ‖ payload)`,
/// its first 4 bytes (§6.3).
pub fn tag_ok(key: &[u8; 32], from_client: bool, bytes: &[u8]) -> bool {
    bytes.len() >= OVERHEAD && {
        let (signed, tag) = bytes.split_at(bytes.len() - HOP_TAG_LEN);
        mac(key, from_client, signed) == tag
    }
}

/// A v3 datagram with a correct tag (§6.1, §6.3).
pub fn seal(
    key: &[u8; 32],
    from_client: bool,
    kind: u8,
    session: u32,
    seq: u32,
    payload: &[u8],
) -> Vec<u8> {
    let mut bytes = vec![VERSION << 5 | kind];
    bytes.extend_from_slice(&session.to_le_bytes());
    bytes.extend_from_slice(&seq.to_le_bytes());
    bytes.extend_from_slice(payload);
    let tag = mac(key, from_client, &bytes);
    bytes.extend_from_slice(&tag);
    bytes
}

// --- §5 PACK v2 ---

pub const PACK: u8 = 12;
const BATCH: u8 = 3;
const FRAME_END: u8 = 4;
const DEPARTURE: u8 = 5;
const PAUSE: u8 = 9;
const RATE: u8 = 10;
const DESYNC: u8 = 11;

/// PACK v2 by §5.2 under every §5.3 rule (1-7, rule 6's four
/// non-shortest forms included), each record rebuilt into its canonical
/// bytes (§5.1).
pub fn decode_pack(payload: &[u8]) -> Result<(u64, u8, Vec<PackRecord>), String> {
    if payload.len() > PACK_CAP {
        return Err("PACK above 681 B (§5.3 rule 5)".into());
    }
    let mut reader = Reader::new(payload, 0);
    if reader.u8("PACK type")? != PACK {
        return Err("not a PACK".into());
    }
    let base = reader.varint("base_seq")?;
    let mut frame = reader.varint_max(u32::MAX.into(), "base_frame")?;
    let count = reader.u8("count")?;
    if count == 0 {
        return Err("PACK count 0".into());
    }
    let mut last_batch: [Option<u64>; 256] = [None; 256];
    let mut records = Vec::with_capacity(usize::from(count));
    for index in 0..u64::from(count) {
        let start = reader.at;
        let tag = reader.u8("tag")?;
        if tag & 0x80 != 0 {
            return Err("tag bit7".into());
        }
        let kind = tag & 0x0F;
        let chained = tag & 0x40 != 0;
        let code = (tag >> 4) & 3;
        // §5.3 rule 6: the first entry's frame is `base_frame`.
        if index == 0 && code != 0 {
            return Err("first entry frame code not 0".into());
        }
        frame = match code {
            0 => frame,
            1 => frame + 1,
            2 => {
                let d = reader.varint("frame delta")?;
                if d < 2 {
                    return Err("frame delta below 2".into());
                }
                frame.checked_add(d).ok_or("frame overflow")?
            }
            _ => return Err("frame code 3".into()),
        };
        if frame > u64::from(u32::MAX) {
            return Err("frame above u32".into());
        }
        if chained && kind != BATCH {
            return Err("bit6 on a record other than BATCH".into());
        }
        let seq = base.checked_add(index).ok_or("seq overflow")?;
        let mut canonical = vec![kind];
        canonical.extend_from_slice(&seq.to_le_bytes());
        canonical.extend_from_slice(&(frame as u32).to_le_bytes());
        let (mut source, mut batch_id, mut opaque_len, mut lost) = (None, None, None, false);
        match kind {
            BATCH => {
                let from = reader.u8("source")?;
                let chainable = last_batch[usize::from(from)].and_then(|last| last.checked_add(1));
                let id = if chained {
                    // §5.3 rule 2.
                    last_batch[usize::from(from)]
                        .ok_or("bit6 without an earlier BATCH of its source")?
                        .checked_add(1)
                        .ok_or("batch_id overflow")?
                } else {
                    let id = reader.varint("batch_id")?;
                    // §5.3 rule 6: bit6 must be used when it can be.
                    if chainable == Some(id) {
                        return Err("batch_id written where bit6 applies".into());
                    }
                    id
                };
                let len = reader.varint("batch len")?;
                // §5.3 rule 5.
                if !(1..=BATCH_MAX as u64).contains(&len) {
                    return Err("batch len".into());
                }
                let plaintext = reader.take(len as usize, "batch")?;
                last_batch[usize::from(from)] = Some(id);
                canonical.push(from);
                canonical.extend_from_slice(&id.to_le_bytes());
                canonical.extend_from_slice(plaintext);
                (source, batch_id, opaque_len) = (Some(from), Some(id), Some(len as usize));
            }
            FRAME_END => {}
            DEPARTURE => {
                let from = reader.u8("source")?;
                lost = match reader.u8("lost")? {
                    0 => false,
                    1 => true,
                    _ => return Err("DEPARTURE lost".into()),
                };
                canonical.push(from);
                if lost {
                    canonical.push(1);
                }
                source = Some(from);
            }
            PAUSE => {
                let epoch = reader.varint("epoch")?;
                let paused = reader.u8("paused")?;
                if paused > 1 {
                    return Err("PAUSE paused".into());
                }
                canonical.extend_from_slice(&epoch.to_le_bytes());
                canonical.push(paused);
                canonical.push(reader.u8("source")?);
            }
            RATE => {
                canonical.extend_from_slice(&reader.u16("fps")?.to_le_bytes());
                canonical.push(reader.u8("source")?);
            }
            DESYNC => canonical.extend_from_slice(reader.take(14, "DESYNC")?),
            kind => return Err(format!("PACK v2 kind {kind}")),
        }
        records.push(PackRecord {
            seq,
            kind,
            frame: frame as u32,
            source,
            batch_id,
            opaque_len,
            lost,
            canonical,
            wire_len: reader.at - start,
        });
    }
    // §5.3 rule 4.
    reader.end("PACK v2")?;
    Ok((base, count, records))
}

/// One canonical record's fields (transport spec §3.5).
fn canonical_parts(record: &[u8]) -> Result<(u8, u64, u32, &[u8]), String> {
    if record.len() < 13 {
        return Err("canonical record shorter than 13 bytes".into());
    }
    Ok((
        record[0],
        u64::from_le_bytes(record[1..9].try_into().unwrap()),
        u32::from_le_bytes(record[9..13].try_into().unwrap()),
        &record[13..],
    ))
}

/// The one PACK v2 for these consecutive canonical records, by §5.4: the
/// shortest frame code, bit6 wherever §5.3 rule 2 allows it, shortest
/// varints, `base_frame` the first record's frame (§5.2).
pub fn encode_pack(records: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let (_, base, first_frame, _) = canonical_parts(records.first().ok_or("no records")?)?;
    if records.len() > 255 {
        return Err("more than 255 records".into());
    }
    let mut out = vec![PACK];
    write_varint(&mut out, base);
    write_varint(&mut out, first_frame.into());
    out.push(records.len() as u8);
    let mut previous = first_frame;
    let mut last_batch: [Option<u64>; 256] = [None; 256];
    for (index, record) in records.iter().enumerate() {
        let (kind, seq, frame, rest) = canonical_parts(record)?;
        if seq != base + index as u64 {
            return Err("records not consecutive".into());
        }
        if frame < previous {
            return Err("frame went back".into());
        }
        let delta = frame - previous;
        previous = frame;
        let code: u8 = match delta {
            0 => 0,
            1 => 1,
            _ => 2,
        };
        let mut body = Vec::new();
        let mut chained = false;
        match (kind, rest.len()) {
            (BATCH, 10..) => {
                let source = rest[0];
                let id = u64::from_le_bytes(rest[1..9].try_into().unwrap());
                let plaintext = &rest[9..];
                chained = last_batch[usize::from(source)]
                    .is_some_and(|last| last.checked_add(1) == Some(id));
                last_batch[usize::from(source)] = Some(id);
                body.push(source);
                if !chained {
                    write_varint(&mut body, id);
                }
                write_varint(&mut body, plaintext.len() as u64);
                body.extend_from_slice(plaintext);
            }
            (FRAME_END, 0) => {}
            (DEPARTURE, 1) => body.extend_from_slice(&[rest[0], 0]),
            (DEPARTURE, 2) if rest[1] == 1 => body.extend_from_slice(&[rest[0], 1]),
            (PAUSE, 10) => {
                write_varint(&mut body, u64::from_le_bytes(rest[..8].try_into().unwrap()));
                body.extend_from_slice(&rest[8..]);
            }
            (RATE, 3) | (DESYNC, 14) => body.extend_from_slice(rest),
            (kind, length) => return Err(format!("canonical kind {kind} with {length} bytes")),
        }
        out.push(kind | code << 4 | u8::from(chained) << 6);
        if code == 2 {
            write_varint(&mut out, delta.into());
        }
        out.extend_from_slice(&body);
    }
    Ok(out)
}

/// How many PACK v2s §5.4's packing needs for these consecutive canonical
/// records: greedy, in order, at most 681 B (`PACK_CAP`) and 255 records
/// each.
pub fn packs_needed(records: &[Vec<u8>]) -> Result<usize, String> {
    let mut packs = 0;
    let mut start = 0;
    while start < records.len() {
        let mut end = start + 1;
        if encode_pack(&records[start..end])?.len() > PACK_CAP {
            return Err("one record does not fit a PACK".into());
        }
        while end < records.len()
            && end - start < 255
            && encode_pack(&records[start..=end])?.len() <= PACK_CAP
        {
            end += 1;
        }
        packs += 1;
        start = end;
    }
    Ok(packs)
}
