//! A wire checker that reads every datagram's bytes by spec §3 and calls no
//! production decoder, so it is also an independent check of the format
//! (e2e-plan §1.3). Phase 1 reads the current format: requests 0-7 and the
//! 32-byte STATUS, 13-byte ACK and PushState replies.

/// What one datagram is, by its envelope and first payload byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram {
    pub kind: &'static str,
    /// The hop counter: `sent` of a client envelope, `seq` of a relay one.
    pub hop_seq: u64,
    pub detail: Detail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail {
    None,
    Submit { batch_id: u64 },
    Push { next: u64 },
    Pull { next: u64 },
    Ack { batch_id: u64, frame: u32 },
    Pack { base: u64, count: u8 },
}

const CLIENT_HEADER: usize = 114;
const SERVER_HEADER: usize = 48;
const MAC: usize = 32;
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

/// A client datagram: `[VR2C][kind][ticket 98][sent u64][target][len u16]
/// [payload][hmac 32]` (spec §3.1).
pub fn client_datagram(bytes: &[u8]) -> Result<Datagram, String> {
    if bytes.len() < CLIENT_HEADER + MAC || &bytes[..4] != b"VR2C" {
        return Err("client envelope".into());
    }
    let length = usize::from(u16_at(bytes, 112));
    if bytes.len() != CLIENT_HEADER + length + MAC {
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
            (0, 99 | 100) => "READY_REQUEST",
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
            (request, _) => return Err(format!("client request {request} length {length}")),
        },
        kind => return Err(format!("client kind {kind} length {length}")),
    };
    Ok(Datagram {
        kind,
        hop_seq,
        detail,
    })
}

/// A record without its sequence, as a PACK carries it: kind, frame, body
/// (spec §3.5).
fn packed_record(record: &[u8]) -> Result<(), String> {
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
    if ok {
        Ok(())
    } else {
        Err(format!(
            "packed record {} length {}",
            record[0],
            record.len()
        ))
    }
}

/// A relay datagram: `[VR2S][kind][room 16][nonce 16][seq u64][source]
/// [len u16][payload][hmac 32]` (spec §3.1).
pub fn server_datagram(bytes: &[u8]) -> Result<Datagram, String> {
    if bytes.len() < SERVER_HEADER + MAC || &bytes[..4] != b"VR2S" {
        return Err("server envelope".into());
    }
    let length = usize::from(u16_at(bytes, 46));
    if bytes.len() != SERVER_HEADER + length + MAC {
        return Err("server length".into());
    }
    let hop_seq = u64_at(bytes, 37);
    let payload = &bytes[SERVER_HEADER..SERVER_HEADER + length];
    let mut detail = Detail::None;
    let kind = match bytes[4] {
        0 if length == 40 => "CHALLENGE",
        1 if length == 0 => "READY",
        2 if length > 41 => "DATA",
        5 if length == 0 => "LEAVE_ACK",
        4 if length > 0 => match (payload[0], length) {
            (0, 9) => "O_READY",
            (1, 32) => "STATUS",
            (2, 13) => {
                detail = Detail::Ack {
                    batch_id: u64_at(payload, 1),
                    frame: u32_at(payload, 9),
                };
                "ACK"
            }
            (6, 10) => "ERROR",
            (7, 10) if payload[1] <= 1 => "PUSH_STATE",
            (8, 30) if payload[25] <= 1 => "PAUSE_STATUS",
            (12, 10..) if payload[9] > 0 => {
                let count = payload[9];
                let mut offset = 10;
                for _ in 0..count {
                    if offset + 2 > length {
                        return Err("pack record header".into());
                    }
                    let record = usize::from(u16_at(payload, offset));
                    let end = offset + 2 + record;
                    if record == 0 || end > length {
                        return Err("pack record length".into());
                    }
                    packed_record(&payload[offset + 2..end])?;
                    offset = end;
                }
                if offset != length {
                    return Err("pack trailing bytes".into());
                }
                detail = Detail::Pack {
                    base: u64_at(payload, 1),
                    count,
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
        detail,
    })
}
