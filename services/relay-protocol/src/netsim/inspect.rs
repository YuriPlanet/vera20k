//! A wire checker that reads every datagram's bytes by spec §3 and calls no
//! production decoder, so it is also an independent check of the format
//! (e2e-plan §1.3). It reads both formats: the current one (requests 0-7,
//! the 13-byte ACK, 32-byte STATUS and PushState) for the baseline, and the
//! adaptive one (UPLINK request 8, the 31-byte ACK and 50-byte STATUS of
//! spec §3.2-§3.4). Which ones a run may contain is the judgments' business.

/// One datagram, by its envelope and payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram {
    pub kind: &'static str,
    /// The hop counter: `sent` of a client envelope, `seq` of a relay one.
    pub hop_seq: u64,
    /// A relay envelope's ticket nonce (bytes 21..37), which names the hop.
    pub nonce: Option<[u8; 16]>,
    /// The envelope's payload length.
    pub payload_len: usize,
    pub detail: Detail,
}

/// The `[up_high u64][up_bits u64][echo_delay u16]` tail (spec §3.3, §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feedback {
    pub high: u64,
    pub bits: u64,
    pub echo_delay_ms: u16,
}

/// UPLINK (request 8, spec §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uplink {
    pub flags: u8,
    pub recv_next: u64,
    pub down_high: u64,
    pub down_bits: u64,
    pub srtt_ms: u16,
    pub base_ms: u16,
    pub link: u8,
    pub checksums: Vec<(u32, u32)>,
    /// `(pace, frames_per_second, tick_micros)` when flags bit1.
    pub pace: Option<(u8, u16, u32)>,
    pub first_batch_id: u64,
    /// Each batch's opaque length, IDs `first_batch_id + i`.
    pub batches: Vec<usize>,
}

pub const FLAG_PUSH: u8 = 1;
pub const FLAG_PACE: u8 = 2;
pub const FLAG_PROBE: u8 = 4;
pub const FLAG_RESTORING: u8 = 8;

impl Uplink {
    pub fn batch_ids(&self) -> impl Iterator<Item = u64> + '_ {
        (0..self.batches.len() as u64).map(|i| self.first_batch_id + i)
    }
    pub fn carries(&self, batch_id: u64) -> bool {
        self.batch_ids().any(|id| id == batch_id)
    }
}

/// One record of a PACK (spec §3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackRecord {
    pub seq: u64,
    pub kind: u8,
    pub frame: u32,
    /// The record's length field: kind and body, no sequence.
    pub len: usize,
    /// BATCH and DEPARTURE.
    pub source: Option<u8>,
    /// BATCH.
    pub batch_id: Option<u64>,
    /// BATCH opaque length.
    pub opaque_len: Option<usize>,
    /// DEPARTURE of a lost connection.
    pub lost: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail {
    None,
    ReadyRequest,
    Submit {
        batch_id: u64,
    },
    Push {
        next: u64,
    },
    Pull {
        next: u64,
    },
    Uplink(Uplink),
    OrderReady {
        ready_mask: u32,
        sources_mask: u32,
    },
    Status {
        head_frame: u32,
        oldest_seq: u64,
        next_seq: u64,
        active_sources: u32,
        tick_hz: u16,
        start_delay_ms: u32,
        started: bool,
        feedback: Option<Feedback>,
    },
    Ack {
        batch_id: u64,
        frame: u32,
        feedback: Option<Feedback>,
    },
    Error {
        code: u8,
        expected: u64,
    },
    Pack {
        base: u64,
        count: u8,
        records: Vec<PackRecord>,
    },
}

const CLIENT_HEADER: usize = 114;
const SERVER_HEADER: usize = 48;
const MAC: usize = 32;
/// Spec §3.1.
pub const MAX_WIRE_PAYLOAD: usize = 681;
/// `[nonce 24][ciphertext 1..=512][tag 16]` (spec §3.2, unchanged).
const OPAQUE_MIN: usize = 24 + 1 + 16;
const OPAQUE_MAX: usize = 24 + 512 + 16;

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

/// UPLINK by spec §3.2, with each of its six relay decoding rules.
fn uplink(payload: &[u8]) -> Result<Uplink, String> {
    let fail = |rule: &str| Err(format!("UPLINK {rule}"));
    if payload.len() < 33 {
        return fail("shorter than its fixed part");
    }
    let flags = payload[1];
    // Rule 2: reserved flag and link bits are zero.
    if flags & 0xF0 != 0 || payload[30] & 0xF8 != 0 {
        return fail("reserved bits");
    }
    let recv_next = u64_at(payload, 2);
    let down_high = u64_at(payload, 10);
    let down_bits = u64_at(payload, 18);
    // Rules 5 and 6.
    if recv_next == 0 {
        return fail("recv_next 0");
    }
    if down_high > 0 && down_bits & 1 == 0 {
        return fail("down_bits bit0");
    }
    if down_high == 0 && down_bits != 0 {
        return fail("down_bits without down_high");
    }
    let checksum_count = usize::from(payload[31]);
    if checksum_count > 2 {
        return fail("checksum_count");
    }
    let mut offset = 32;
    let mut checksums = Vec::new();
    for _ in 0..checksum_count {
        if offset + 8 > payload.len() {
            return fail("checksum segment");
        }
        checksums.push((u32_at(payload, offset), u32_at(payload, offset + 4)));
        offset += 8;
    }
    let pace = if flags & FLAG_PACE != 0 {
        if offset + 7 > payload.len() {
            return fail("pace segment");
        }
        let pace = (
            payload[offset],
            u16_at(payload, offset + 1),
            u32_at(payload, offset + 3),
        );
        offset += 7;
        Some(pace)
    } else {
        None
    };
    if offset >= payload.len() {
        return fail("batch_count missing");
    }
    let batch_count = usize::from(payload[offset]);
    offset += 1;
    if batch_count > 4 {
        return fail("batch_count");
    }
    let mut first_batch_id = 0;
    let mut batches = Vec::new();
    if batch_count > 0 {
        if offset + 8 > payload.len() {
            return fail("first_batch_id");
        }
        first_batch_id = u64_at(payload, offset);
        offset += 8;
        // Rule 3.
        if first_batch_id == 0 {
            return fail("first_batch_id 0");
        }
        for _ in 0..batch_count {
            if offset + 2 > payload.len() {
                return fail("batch length");
            }
            let len = usize::from(u16_at(payload, offset));
            offset += 2;
            // Rule 4: 40 < len <= 552.
            if !(OPAQUE_MIN..=OPAQUE_MAX).contains(&len) || offset + len > payload.len() {
                return fail("batch opaque");
            }
            offset += len;
            batches.push(len);
        }
    }
    // Rule 1: the counts account for every byte.
    if offset != payload.len() {
        return fail("trailing bytes");
    }
    Ok(Uplink {
        flags,
        recv_next,
        down_high,
        down_bits,
        srtt_ms: u16_at(payload, 26),
        base_ms: u16_at(payload, 28),
        link: payload[30],
        checksums,
        pace,
        first_batch_id,
        batches,
    })
}

/// A client datagram: `[VR2C][kind][ticket 98][sent u64][target][len u16]
/// [payload][hmac 32]` (spec §3.1).
pub fn client_datagram(bytes: &[u8]) -> Result<Datagram, String> {
    if bytes.len() < CLIENT_HEADER + MAC || &bytes[..4] != b"VR2C" {
        return Err("client envelope".into());
    }
    let length = usize::from(u16_at(bytes, 112));
    if bytes.len() != CLIENT_HEADER + length + MAC || length > MAX_WIRE_PAYLOAD {
        return Err("client length".into());
    }
    let hop_seq = u64_at(bytes, 103);
    let target = bytes[111];
    let payload = &bytes[CLIENT_HEADER..CLIENT_HEADER + length];
    let mut detail = Detail::None;
    let kind = match bytes[4] {
        0 if length == 0 => "HELLO",
        1 if length >= 40 => "CONFIRM",
        2 if length > 41 => "DATA",
        3 if length == 0 => "PING",
        4 if length > 0 && target == 255 => match (payload[0], length) {
            (0, 99 | 100) => {
                detail = Detail::ReadyRequest;
                "READY_REQUEST"
            }
            (1, 9..) if (OPAQUE_MIN..=OPAQUE_MAX).contains(&(length - 9)) => {
                detail = Detail::Submit {
                    batch_id: u64_at(payload, 1),
                };
                "SUBMIT"
            }
            (2, 9) => {
                detail = Detail::Pull {
                    next: u64_at(payload, 1),
                };
                "PULL"
            }
            (3, 1) => "LEAVE",
            (4, 10) if payload[1] <= 1 && u64_at(payload, 2) != 0 => {
                detail = Detail::Push {
                    next: u64_at(payload, 2),
                };
                "PUSH"
            }
            (5, 18) if payload[17] <= 1 => "PAUSE",
            (6, 4 | 8) if payload[1] <= 2 => "PACE",
            (7, _)
                if (1..=2).contains(&payload.get(1).copied().unwrap_or(0))
                    && length == 2 + 8 * usize::from(payload[1]) =>
            {
                "CHECKSUM"
            }
            (8, _) => {
                detail = Detail::Uplink(uplink(payload)?);
                "UPLINK"
            }
            (request, _) => return Err(format!("client request {request} length {length}")),
        },
        kind => return Err(format!("client kind {kind} length {length}")),
    };
    Ok(Datagram {
        kind,
        hop_seq,
        nonce: None,
        payload_len: length,
        detail,
    })
}

/// A record without its sequence, as a PACK carries it: kind, frame, body
/// (spec §3.5).
fn packed_record(seq: u64, record: &[u8]) -> Result<PackRecord, String> {
    let ok = match (record[0], record.len()) {
        (3, length) => length > 14 && (OPAQUE_MIN..=OPAQUE_MAX).contains(&(length - 14)),
        (4, 5) => true,
        (5, 6) => true,
        (5, 7) => record[6] == 1,
        (9, 15) => record[13] <= 1,
        (10, 8) => true,
        (11, 19) => true,
        _ => false,
    };
    if !ok {
        return Err(format!(
            "packed record {} length {}",
            record[0],
            record.len()
        ));
    }
    Ok(PackRecord {
        seq,
        kind: record[0],
        frame: u32_at(record, 1),
        len: record.len(),
        source: matches!(record[0], 3 | 5).then(|| record[5]),
        batch_id: (record[0] == 3).then(|| u64_at(record, 6)),
        opaque_len: (record[0] == 3).then(|| record.len() - 14),
        lost: record[0] == 5 && record.len() == 7,
    })
}

fn feedback(payload: &[u8], offset: usize) -> Feedback {
    Feedback {
        high: u64_at(payload, offset),
        bits: u64_at(payload, offset + 8),
        echo_delay_ms: u16_at(payload, offset + 16),
    }
}

/// A relay datagram: `[VR2S][kind][room 16][nonce 16][seq u64][source]
/// [len u16][payload][hmac 32]` (spec §3.1).
pub fn server_datagram(bytes: &[u8]) -> Result<Datagram, String> {
    if bytes.len() < SERVER_HEADER + MAC || &bytes[..4] != b"VR2S" {
        return Err("server envelope".into());
    }
    let length = usize::from(u16_at(bytes, 46));
    if bytes.len() != SERVER_HEADER + length + MAC || length > MAX_WIRE_PAYLOAD {
        return Err("server length".into());
    }
    let hop_seq = u64_at(bytes, 37);
    let nonce: [u8; 16] = bytes[21..37].try_into().unwrap();
    let payload = &bytes[SERVER_HEADER..SERVER_HEADER + length];
    let mut detail = Detail::None;
    let kind = match bytes[4] {
        0 if length == 40 => "CHALLENGE",
        1 if length == 0 => "READY",
        2 if length > 41 => "DATA",
        5 if length == 0 => "LEAVE_ACK",
        4 if length > 0 => match (payload[0], length) {
            (0, 9) => {
                detail = Detail::OrderReady {
                    ready_mask: u32_at(payload, 1),
                    sources_mask: u32_at(payload, 5),
                };
                "O_READY"
            }
            (1, 32 | 50) => {
                detail = Detail::Status {
                    head_frame: u32_at(payload, 1),
                    oldest_seq: u64_at(payload, 5),
                    next_seq: u64_at(payload, 13),
                    active_sources: u32_at(payload, 21),
                    tick_hz: u16_at(payload, 25),
                    start_delay_ms: u32_at(payload, 27),
                    started: match payload[31] {
                        0 => false,
                        1 => true,
                        _ => return Err("STATUS started".into()),
                    },
                    feedback: (length == 50).then(|| feedback(payload, 32)),
                };
                "STATUS"
            }
            (2, 13 | 31) => {
                detail = Detail::Ack {
                    batch_id: u64_at(payload, 1),
                    frame: u32_at(payload, 9),
                    feedback: (length == 31).then(|| feedback(payload, 13)),
                };
                "ACK"
            }
            (6, 10) => {
                detail = Detail::Error {
                    code: payload[1],
                    expected: u64_at(payload, 2),
                };
                "ERROR"
            }
            (7, 10) if payload[1] <= 1 => "PUSH_STATE",
            (8, 30) if payload[25] <= 1 => "PAUSE_STATUS",
            (12, 10..) if payload[9] > 0 => {
                let base = u64_at(payload, 1);
                let count = payload[9];
                let mut offset = 10;
                let mut records = Vec::with_capacity(usize::from(count));
                for index in 0..u64::from(count) {
                    if offset + 2 > length {
                        return Err("pack record header".into());
                    }
                    let record = usize::from(u16_at(payload, offset));
                    let end = offset + 2 + record;
                    if record < 5 || end > length {
                        return Err("pack record length".into());
                    }
                    records.push(packed_record(base + index, &payload[offset + 2..end])?);
                    offset = end;
                }
                if offset != length {
                    return Err("pack trailing bytes".into());
                }
                detail = Detail::Pack {
                    base,
                    count,
                    records,
                };
                "PACK"
            }
            (event, _) => return Err(format!("server event {event} length {length}")),
        },
        kind => return Err(format!("server kind {kind} length {length}")),
    };
    Ok(Datagram {
        kind,
        hop_seq,
        nonce: Some(nonce),
        payload_len: length,
        detail,
    })
}

/// How many PACKs the spec's packing (§3.5: a 10-byte header, then
/// `[len u16]` and the record, at most `MAX_WIRE_PAYLOAD` bytes and 255
/// records, in order) needs for records with these length fields.
pub fn packs_needed(record_lens: &[usize]) -> usize {
    let mut packs = 0;
    let mut used = MAX_WIRE_PAYLOAD;
    let mut count = 0;
    for &len in record_lens {
        if used + 2 + len > MAX_WIRE_PAYLOAD || count == 255 {
            packs += 1;
            used = 10;
            count = 0;
        }
        used += 2 + len;
        count += 1;
    }
    packs
}
