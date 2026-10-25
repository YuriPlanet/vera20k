//! Fixed-clock ordering of opaque encrypted batches. A reader's cursor never
//! participates in clock advancement, admission, or another reader's progress.

use std::collections::{BTreeMap, VecDeque};

use sha2::Digest;

use super::*;

pub const ORDERING_CERTIFICATE_LEN: usize = 66;
pub const ORDERING_BATCH_CAPACITY: usize = 512;
pub const ORDERING_PULL_LIMIT: usize = 32;
const CERTIFICATE_MAGIC: &[u8; 4] = b"VO1C";
const CERTIFICATE_DOMAIN: &[u8] = b"VERA20k relay ordering certificate v1\0";
const ORDER_KEY_DOMAIN: &[u8] = b"VERA20k relay ordering payload key v1\0";
const MAX_MATCH_SECONDS: u64 = 24 * 60 * 60;
const START_DELAY_MS: u64 = 200;
const MAX_HISTORY_ENTRIES: usize = 4096;
const ROOM_HISTORY_BYTES: usize = 2 * 1024 * 1024;
const GLOBAL_ORDERING_BYTES: usize = 64 * 1024 * 1024;
const MAX_SOURCE_PENDING: usize = 8;
const MAX_FRAME_BATCHES: usize = 32;
const OPAQUE_OVERHEAD: usize = 24 + 16;
const ENTRY_OVERHEAD: usize = 128;
const READY_REQUEST: u8 = 0;
const SUBMIT_REQUEST: u8 = 1;
const PULL_REQUEST: u8 = 2;
pub(super) const LEAVE_REQUEST: u8 = 3;
const READY_EVENT: u8 = 0;
const STATUS_EVENT: u8 = 1;
const ACK_EVENT: u8 = 2;
const BATCH_EVENT: u8 = 3;
pub(super) const FRAME_END_EVENT: u8 = 4;
pub(super) const DEPARTURE_EVENT: u8 = 5;
const ERROR_EVENT: u8 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderingCertificate {
    pub room: [u8; 16],
    pub sources_mask: u32,
    pub tick_hz: u16,
    pub expires: u64,
}

impl OrderingCertificate {
    pub fn parse(bytes: &[u8]) -> Result<Self, RelayError> {
        if bytes.len() != ORDERING_CERTIFICATE_LEN || &bytes[..4] != CERTIFICATE_MAGIC {
            return Err(RelayError::Malformed);
        }
        let value = Self {
            room: bytes[4..20].try_into().unwrap(),
            sources_mask: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            tick_hz: u16::from_le_bytes(bytes[24..26].try_into().unwrap()),
            expires: u64::from_le_bytes(bytes[26..34].try_into().unwrap()),
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), RelayError> {
        if self.room == [0; 16]
            || self.sources_mask == 0
            || self.sources_mask >> MAX_PEERS != 0
            || !matches!(self.tick_hz, 60 | 45 | 30 | 20 | 15 | 12 | 10)
        {
            return Err(RelayError::Malformed);
        }
        Ok(())
    }

    /// The HTTPS lobby signs this public manifest separately from hop tickets.
    /// Layout: VO1C, room16, LE mask32, LE Hz16, LE expiry64, domain HMAC32.
    pub fn sign(self, key: &SecretKey) -> Result<[u8; ORDERING_CERTIFICATE_LEN], RelayError> {
        self.validate()?;
        let mut bytes = [0; ORDERING_CERTIFICATE_LEN];
        bytes[..4].copy_from_slice(CERTIFICATE_MAGIC);
        bytes[4..20].copy_from_slice(&self.room);
        bytes[20..24].copy_from_slice(&self.sources_mask.to_le_bytes());
        bytes[24..26].copy_from_slice(&self.tick_hz.to_le_bytes());
        bytes[26..34].copy_from_slice(&self.expires.to_le_bytes());
        let tag = key
            .mac(CERTIFICATE_DOMAIN, &bytes[..34])
            .finalize()
            .into_bytes();
        bytes[34..].copy_from_slice(&tag);
        Ok(bytes)
    }

    fn verify(bytes: &[u8], key: &SecretKey, now: u64) -> Result<Self, RelayError> {
        let certificate = Self::parse(bytes)?;
        key.mac(CERTIFICATE_DOMAIN, &bytes[..34])
            .verify_slice(&bytes[34..])
            .map_err(|_| RelayError::Authentication)?;
        if certificate.expires <= now || certificate.expires > now.saturating_add(MAX_MATCH_SECONDS)
        {
            return Err(RelayError::Expired);
        }
        Ok(certificate)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OrderError {
    NotConfigured = 1,
    NotStarted = 2,
    AgreementMismatch = 3,
    BatchGap = 4,
    ModifiedRetry = 5,
    Backpressure = 6,
    HistoryGap = 7,
    Inactive = 8,
    InvalidCursor = 9,
    StorageUnavailable = 10,
}

impl OrderError {
    fn parse(value: u8) -> Result<Self, RelayError> {
        Ok(match value {
            1 => Self::NotConfigured,
            2 => Self::NotStarted,
            3 => Self::AgreementMismatch,
            4 => Self::BatchGap,
            5 => Self::ModifiedRetry,
            6 => Self::Backpressure,
            7 => Self::HistoryGap,
            8 => Self::Inactive,
            9 => Self::InvalidCursor,
            10 => Self::StorageUnavailable,
            _ => return Err(RelayError::Malformed),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderStatus {
    /// Highest complete sealed frame. Frame zero is startup only.
    pub head_frame: u32,
    pub oldest_seq: u64,
    pub next_seq: u64,
    pub active_sources: u32,
    pub tick_hz: u16,
    pub start_delay_ms: u32,
    pub started: bool,
}

#[derive(Debug)]
pub enum OrderEntry {
    Batch {
        sequence: u64,
        frame: u32,
        source: u8,
        batch_id: u64,
        bytes: DecryptedPayload,
    },
    FrameEnd {
        sequence: u64,
        frame: u32,
    },
    Departure {
        sequence: u64,
        frame: u32,
        source: u8,
    },
}

#[derive(Debug)]
pub enum OrderEvent {
    Ready {
        ready_mask: u32,
        sources_mask: u32,
    },
    Status(OrderStatus),
    Ack {
        batch_id: u64,
        frame: u32,
    },
    Entry(OrderEntry),
    Error {
        code: OrderError,
        expected: u64,
    },
    /// The exact terminal Leave was accepted, or its expired room is gone.
    Left,
}

#[derive(Clone)]
struct SealedBatch {
    id: u64,
    digest: [u8; 32],
    opaque: Vec<u8>,
    acknowledged: bool,
}

#[derive(Clone)]
pub(super) struct ClientOrdering {
    certificate: [u8; ORDERING_CERTIFICATE_LEN],
    agreement: [u8; 32],
    last: Option<SealedBatch>,
    leave: Option<(u64, Vec<u8>)>,
}

fn order_cipher(key: &SecretKey, room: &[u8; 16], source: u8) -> XChaCha20Poly1305 {
    let mut mac = key.mac(ORDER_KEY_DOMAIN, room);
    mac.update(&[source]);
    let mut derived: [u8; 32] = mac.finalize().into_bytes().into();
    let cipher = XChaCha20Poly1305::new(&derived.into());
    derived.zeroize();
    cipher
}

fn order_aad(room: &[u8; 16], source: u8, batch_id: u64) -> [u8; 29] {
    let mut aad = [0; 29];
    aad[..4].copy_from_slice(CERTIFICATE_MAGIC);
    aad[4..20].copy_from_slice(room);
    aad[20] = source;
    aad[21..].copy_from_slice(&batch_id.to_le_bytes());
    aad
}

impl ClientSession {
    /// Move the current stream cache at hop-lease promotion, after the old
    /// route's last receive/submit. Candidate creation alone is only a snapshot.
    pub fn adopt_ordering_from(&mut self, previous: &mut Self) -> Result<(), RelayError> {
        if self.ticket.room != previous.ticket.room
            || self.ticket.source != previous.ticket.source
            || self.ticket.player != previous.ticket.player
            || self.ticket.generation != previous.ticket.generation
            || !self.payload_key.same_key(&previous.payload_key)
        {
            return Err(RelayError::Authentication);
        }
        self.ordering = previous.ordering.take();
        Ok(())
    }

    pub fn configure_ordering(
        &mut self,
        certificate: &[u8],
        agreement: [u8; 32],
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        if !self.ready {
            return Err(RelayError::Unregistered);
        }
        let parsed = OrderingCertificate::parse(certificate)?;
        if parsed.room != self.ticket.room || parsed.sources_mask & (1 << self.ticket.source) == 0 {
            return Err(RelayError::Authentication);
        }
        let certificate: [u8; ORDERING_CERTIFICATE_LEN] = certificate.try_into().unwrap();
        if let Some(current) = &mut self.ordering {
            let old = OrderingCertificate::parse(&current.certificate)?;
            if old.room != parsed.room
                || old.sources_mask != parsed.sources_mask
                || old.tick_hz != parsed.tick_hz
                || current.agreement != agreement
            {
                return Err(RelayError::Authentication);
            }
            if parsed.expires > old.expires {
                current.certificate = certificate;
            }
        } else {
            self.ordering = Some(ClientOrdering {
                certificate,
                agreement,
                last: None,
                leave: None,
            });
        }
        let mut payload = vec![READY_REQUEST];
        payload.extend_from_slice(&certificate);
        payload.extend_from_slice(&agreement);
        self.encode(CONTROL, BROADCAST, &payload, out)
    }

    /// One in-flight batch, stable ciphertext across retries and hop renewals.
    pub fn submit_ordering(
        &mut self,
        batch_id: u64,
        plaintext: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        if !self.ready {
            return Err(RelayError::Unregistered);
        }
        if plaintext.is_empty() || plaintext.len() > ORDERING_BATCH_CAPACITY || batch_id == 0 {
            return Err(RelayError::Malformed);
        }
        let ordering = self.ordering.as_mut().ok_or(RelayError::Unregistered)?;
        let digest: [u8; 32] = Sha256::digest(plaintext).into();
        let same = ordering
            .last
            .as_ref()
            .is_some_and(|last| last.id == batch_id);
        if same {
            if ordering.last.as_ref().unwrap().digest != digest {
                return Err(RelayError::Authentication);
            }
        } else {
            let expected = match &ordering.last {
                Some(last) if last.acknowledged => {
                    last.id.checked_add(1).ok_or(RelayError::CounterExhausted)?
                }
                Some(_) => return Err(RelayError::Replay),
                None => 1,
            };
            if batch_id != expected {
                return Err(RelayError::Replay);
            }
            let mut nonce = [0; 24];
            nonce[..16].copy_from_slice(&self.payload_prefix);
            nonce[16..].copy_from_slice(&batch_id.to_le_bytes());
            let mut opaque = Vec::with_capacity(plaintext.len() + OPAQUE_OVERHEAD);
            opaque.extend_from_slice(&nonce);
            opaque.extend_from_slice(plaintext);
            let tag = order_cipher(&self.payload_key, &self.ticket.room, self.ticket.source)
                .encrypt_inout_detached(
                    &XNonce::from(nonce),
                    &order_aad(&self.ticket.room, self.ticket.source, batch_id),
                    (&mut opaque[24..]).into(),
                )
                .map_err(|_| RelayError::Authentication)?;
            opaque.extend_from_slice(&tag);
            ordering.last = Some(SealedBatch {
                id: batch_id,
                digest,
                opaque,
                acknowledged: false,
            });
        }
        let opaque = &ordering.last.as_ref().unwrap().opaque;
        let mut payload = Vec::with_capacity(9 + opaque.len());
        payload.push(SUBMIT_REQUEST);
        payload.extend_from_slice(&batch_id.to_le_bytes());
        payload.extend_from_slice(opaque);
        self.encode(CONTROL, BROADCAST, &payload, out)
    }

    pub fn pull_ordering(&mut self, next_seq: u64, out: &mut Vec<u8>) -> Result<(), RelayError> {
        if !self.ready || self.ordering.is_none() {
            return Err(RelayError::Unregistered);
        }
        let mut payload = vec![PULL_REQUEST];
        payload.extend_from_slice(&next_seq.to_le_bytes());
        self.encode(CONTROL, BROADCAST, &payload, out)
    }

    pub fn leave_ordering(&mut self, out: &mut Vec<u8>) -> Result<(), RelayError> {
        let Some(ordering) = self.ordering.as_ref() else {
            return Err(RelayError::Unregistered);
        };
        if let Some((_, bytes)) = &ordering.leave {
            out.clone_from(bytes);
            return Ok(());
        }
        // Only this irreversible capability can be sent after readiness or
        // ticket expiry. Cache exact bytes for idempotent terminal retries.
        self.encode(CONTROL, BROADCAST, &[LEAVE_REQUEST], out)?;
        self.ordering.as_mut().unwrap().leave = Some((self.sent, out.clone()));
        Ok(())
    }

    pub fn ordering_is_configured(&self) -> bool {
        self.ordering.is_some()
    }

    pub(super) fn accepts_ordering_leave_ack(&self, sequence: u64) -> bool {
        self.ordering.as_ref().is_some_and(|ordering| {
            ordering
                .leave
                .as_ref()
                .is_some_and(|(sent, _)| *sent == sequence)
        })
    }

    pub(super) fn receive_ordering(&mut self, payload: &[u8]) -> Result<OrderEvent, RelayError> {
        if !self.ready || self.ordering.is_none() || payload.is_empty() {
            return Err(RelayError::Unregistered);
        }
        Ok(match (payload[0], payload.len()) {
            (READY_EVENT, 9) => OrderEvent::Ready {
                ready_mask: read_u32(payload, 1),
                sources_mask: read_u32(payload, 5),
            },
            (STATUS_EVENT, 32) => OrderEvent::Status(OrderStatus {
                head_frame: read_u32(payload, 1),
                oldest_seq: read_u64(payload, 5),
                next_seq: read_u64(payload, 13),
                active_sources: read_u32(payload, 21),
                tick_hz: u16::from_le_bytes(payload[25..27].try_into().unwrap()),
                start_delay_ms: read_u32(payload, 27),
                started: payload[31] != 0,
            }),
            (ACK_EVENT, 13) => {
                let batch_id = read_u64(payload, 1);
                if let Some(last) = self
                    .ordering
                    .as_mut()
                    .and_then(|ordering| ordering.last.as_mut())
                    && last.id == batch_id
                {
                    last.acknowledged = true;
                }
                OrderEvent::Ack {
                    batch_id,
                    frame: read_u32(payload, 9),
                }
            }
            (FRAME_END_EVENT, 13) => OrderEvent::Entry(OrderEntry::FrameEnd {
                sequence: read_u64(payload, 1),
                frame: read_u32(payload, 9),
            }),
            (DEPARTURE_EVENT, 14) if payload[13] < MAX_PEERS => {
                OrderEvent::Entry(OrderEntry::Departure {
                    sequence: read_u64(payload, 1),
                    frame: read_u32(payload, 9),
                    source: payload[13],
                })
            }
            (BATCH_EVENT, length)
                if length > 22 + OPAQUE_OVERHEAD
                    && length <= 22 + OPAQUE_OVERHEAD + ORDERING_BATCH_CAPACITY
                    && payload[13] < MAX_PEERS =>
            {
                let source = payload[13];
                let batch_id = read_u64(payload, 14);
                let opaque = &payload[22..];
                let nonce: [u8; 24] = opaque[..24].try_into().unwrap();
                let length = opaque.len() - OPAQUE_OVERHEAD;
                let mut bytes = DecryptedPayload {
                    bytes: [0; MAX_PAYLOAD],
                    length,
                };
                bytes.bytes[..length].copy_from_slice(&opaque[24..24 + length]);
                let tag: [u8; 16] = opaque[24 + length..].try_into().unwrap();
                order_cipher(&self.payload_key, &self.ticket.room, source)
                    .decrypt_inout_detached(
                        &XNonce::from(nonce),
                        &order_aad(&self.ticket.room, source, batch_id),
                        (&mut bytes.bytes[..length]).into(),
                        &Tag::from(tag),
                    )
                    .map_err(|_| RelayError::Authentication)?;
                // The canonical batch is also its acceptance receipt. A lost
                // ACK must not block the next local submission after replay.
                if source == self.ticket.source
                    && let Some(last) = self
                        .ordering
                        .as_mut()
                        .and_then(|ordering| ordering.last.as_mut())
                    && last.id == batch_id
                {
                    if last.opaque != opaque {
                        return Err(RelayError::Authentication);
                    }
                    last.acknowledged = true;
                }
                OrderEvent::Entry(OrderEntry::Batch {
                    sequence: read_u64(payload, 1),
                    frame: read_u32(payload, 9),
                    source,
                    batch_id,
                    bytes,
                })
            }
            (ERROR_EVENT, 10) => OrderEvent::Error {
                code: OrderError::parse(payload[1])?,
                expected: read_u64(payload, 2),
            },
            _ => return Err(RelayError::Malformed),
        })
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
fn error(code: OrderError, expected: u64) -> Vec<Vec<u8>> {
    let mut payload = vec![ERROR_EVENT, code as u8];
    payload.extend_from_slice(&expected.to_le_bytes());
    vec![payload]
}
fn ack(batch_id: u64, frame: u32) -> Vec<Vec<u8>> {
    let mut payload = vec![ACK_EVENT];
    payload.extend_from_slice(&batch_id.to_le_bytes());
    payload.extend_from_slice(&frame.to_le_bytes());
    vec![payload]
}

#[derive(Default)]
struct Source {
    principal: Option<([u8; 16], u64)>,
    last_batch: Option<(u64, u32, Vec<u8>)>,
    pending: usize,
    leave_packets: RateBudget,
}

struct Pending {
    source: u8,
    batch_id: u64,
    opaque: Vec<u8>,
}
struct Stored {
    frame: u32,
    bytes: Vec<u8>,
}

pub(super) struct OrderingRoom {
    certificate: OrderingCertificate,
    agreement: [u8; 32],
    sources: [Source; MAX_PEERS as usize],
    ready: u32,
    active: u32,
    starts_at: Option<u64>,
    head: u32,
    next_seq: u64,
    history: VecDeque<Stored>,
    history_bytes: usize,
    pending: BTreeMap<u32, Vec<Pending>>,
    journal: Option<crate::journal::Journal>,
    storage_failed: bool,
    finished_at: Option<u64>,
}

impl OrderingRoom {
    fn new(
        certificate: OrderingCertificate,
        agreement: [u8; 32],
        journal: Option<crate::journal::Journal>,
    ) -> Self {
        Self {
            certificate,
            agreement,
            sources: std::array::from_fn(|_| Source::default()),
            ready: 0,
            active: certificate.sources_mask,
            starts_at: None,
            head: 0,
            next_seq: 1,
            history: VecDeque::new(),
            history_bytes: 0,
            pending: BTreeMap::new(),
            journal,
            storage_failed: false,
            finished_at: None,
        }
    }

    pub(super) fn memory(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.history_bytes
            + self
                .pending
                .values()
                .flat_map(|frame| frame.iter())
                .map(|entry| entry.opaque.len() + ENTRY_OVERHEAD)
                .sum::<usize>()
            + self
                .sources
                .iter()
                .filter_map(|source| source.last_batch.as_ref())
                .map(|(_, _, bytes)| bytes.len() + ENTRY_OVERHEAD)
                .sum::<usize>()
    }

    fn oldest_seq(&self) -> u64 {
        if self.journal.is_some() {
            1
        } else {
            self.hot_oldest_seq()
        }
    }
    fn hot_oldest_seq(&self) -> u64 {
        self.next_seq - self.history.len() as u64
    }
    fn status(&self, now_ms: u64) -> Vec<u8> {
        let mut payload = vec![STATUS_EVENT];
        payload.extend_from_slice(&self.head.to_le_bytes());
        payload.extend_from_slice(&self.oldest_seq().to_le_bytes());
        payload.extend_from_slice(&self.next_seq.to_le_bytes());
        payload.extend_from_slice(&self.active.to_le_bytes());
        payload.extend_from_slice(&self.certificate.tick_hz.to_le_bytes());
        payload.extend_from_slice(
            &(self.starts_at.map_or(0, |start| {
                start.saturating_sub(now_ms).min(u64::from(u32::MAX))
            }) as u32)
                .to_le_bytes(),
        );
        payload.push(u8::from(
            self.starts_at.is_some_and(|start| start <= now_ms),
        ));
        payload
    }

    pub(super) fn expired(&self, now: u64) -> bool {
        self.certificate.expires <= now
    }

    fn cleanup_due(&self, now_ms: u64, now: u64) -> bool {
        !self.storage_failed
            && (self.expired(now)
                || self
                    .finished_at
                    .is_some_and(|at| now_ms.saturating_sub(at) >= MAX_TICKET_LIFETIME * 1000))
    }
    fn pop_oldest(&mut self) -> bool {
        if let Some(old) = self.history.pop_front() {
            self.history_bytes -= old.bytes.len() + ENTRY_OVERHEAD;
            true
        } else {
            false
        }
    }

    fn encoded_entry(sequence: u64, frame: u32, kind: u8, tail: &[u8]) -> Vec<u8> {
        let mut bytes = vec![kind];
        bytes.extend_from_slice(&sequence.to_le_bytes());
        bytes.extend_from_slice(&frame.to_le_bytes());
        bytes.extend_from_slice(tail);
        bytes
    }

    fn cache_entry(&mut self, frame: u32, bytes: Vec<u8>) {
        self.next_seq += 1;
        self.history_bytes += bytes.len() + ENTRY_OVERHEAD;
        self.history.push_back(Stored { frame, bytes });
        while self.history.len() > MAX_HISTORY_ENTRIES || self.history_bytes > ROOM_HISTORY_BYTES {
            self.pop_oldest();
        }
    }

    fn depart(&mut self, source: u8, now_ms: u64) {
        let bit = 1 << source;
        if self.active & bit == 0 {
            return;
        }
        if self.starts_at.is_none() {
            // The signed startup requires every source. An explicit Leave
            // makes that immutable quorum impossible, including sources which
            // never reached Ready. Keep an inactive short-ticket tombstone.
            self.active = 0;
            self.ready = 0;
            self.finished_at = Some(now_ms);
            return;
        }
        self.active &= !bit;
        let final_frame = (MAX_MATCH_SECONDS * u64::from(self.certificate.tick_hz)) as u32;
        self.pending
            .entry((self.head + 2).min(final_frame))
            .or_default()
            .push(Pending {
                source,
                batch_id: 0,
                opaque: Vec::new(),
            });
    }

    fn advance(&mut self, now_ms: u64) {
        if self.storage_failed {
            return;
        }
        let Some(start) = self.starts_at else {
            return;
        };
        let target = (now_ms
            .saturating_sub(start)
            .saturating_mul(u64::from(self.certificate.tick_hz))
            / 1000)
            .min(MAX_MATCH_SECONDS * u64::from(self.certificate.tick_hz))
            as u32;
        let final_frame = (MAX_MATCH_SECONDS * u64::from(self.certificate.tick_hz)) as u32;
        if target == final_frame && self.head < final_frame {
            for source in 0..MAX_PEERS {
                if self.active & (1 << source) != 0 {
                    self.pending.entry(final_frame).or_default().push(Pending {
                        source,
                        batch_id: 0,
                        opaque: Vec::new(),
                    });
                }
            }
            self.active = 0;
        }
        let target = if self.active == 0 {
            target.min(
                self.pending
                    .last_key_value()
                    .map_or(self.head, |(&frame, _)| frame),
            )
        } else {
            target
        };
        while self.head < target {
            // In-memory protocol fixtures may trim old empty prefixes. A real
            // relay journals every frame, including empty ones, for late replay.
            let keep_from = target.saturating_sub(MAX_HISTORY_ENTRIES as u32);
            let skip_to = self
                .pending
                .first_key_value()
                .map_or(keep_from, |(&frame, _)| {
                    keep_from.min(frame.saturating_sub(1))
                });
            if self.journal.is_none() && skip_to > self.head {
                self.next_seq += u64::from(skip_to - self.head);
                self.head = skip_to;
                self.history.clear();
                self.history_bytes = 0;
                continue;
            }
            let frame = self.head + 1;
            let mut records = Vec::new();
            if let Some(entries) = self.pending.get_mut(&frame) {
                // Accepted batches remain executable before their source's
                // removal at the same boundary; neither is ever rewritten.
                entries.sort_by_key(|entry| {
                    (
                        entry.source,
                        if entry.batch_id == 0 {
                            u64::MAX
                        } else {
                            entry.batch_id
                        },
                    )
                });
                for entry in entries.iter() {
                    if entry.batch_id == 0 {
                        records.push(Self::encoded_entry(
                            self.next_seq + records.len() as u64,
                            frame,
                            DEPARTURE_EVENT,
                            &[entry.source],
                        ));
                    } else {
                        let mut tail = vec![entry.source];
                        tail.extend_from_slice(&entry.batch_id.to_le_bytes());
                        tail.extend_from_slice(&entry.opaque);
                        records.push(Self::encoded_entry(
                            self.next_seq + records.len() as u64,
                            frame,
                            BATCH_EVENT,
                            &tail,
                        ));
                    }
                }
            }
            records.push(Self::encoded_entry(
                self.next_seq + records.len() as u64,
                frame,
                FRAME_END_EVENT,
                &[],
            ));
            if let Some(journal) = self.journal.as_mut()
                && journal.commit(&records).is_err()
            {
                self.storage_failed = true;
                return;
            }
            if let Some(entries) = self.pending.remove(&frame) {
                for entry in entries {
                    if entry.batch_id != 0 {
                        self.sources[usize::from(entry.source)].pending -= 1;
                    }
                }
            }
            for record in records {
                self.cache_entry(frame, record);
            }
            self.head = frame;
        }
        // A network outage never retires a source. Only explicit Leave or
        // the agreed full-match lifetime ends its place in the stream.
        if ((self.active == 0 && self.pending.is_empty())
            || self.head == (MAX_MATCH_SECONDS * u64::from(self.certificate.tick_hz)) as u32)
            && self.finished_at.is_none()
        {
            self.finished_at = Some(now_ms);
        }
    }

    fn request(
        &mut self,
        ticket: Ticket,
        payload: &[u8],
        now_ms: u64,
    ) -> Result<Vec<Vec<u8>>, RelayError> {
        let source = ticket.source;
        if self.storage_failed {
            return Ok(error(OrderError::StorageUnavailable, self.next_seq));
        }
        let principal = (ticket.player, ticket.generation);
        if self.certificate.sources_mask & (1 << source) == 0
            || self.sources[usize::from(source)].principal != Some(principal)
        {
            return Err(RelayError::Authentication);
        }
        let Some(&kind) = payload.first() else {
            return Err(RelayError::Malformed);
        };
        match (kind, payload.len()) {
            (SUBMIT_REQUEST, length)
                if length > 9 + OPAQUE_OVERHEAD
                    && length <= 9 + OPAQUE_OVERHEAD + ORDERING_BATCH_CAPACITY =>
            {
                if self.starts_at.is_none() {
                    return Ok(error(OrderError::NotStarted, 0));
                }
                let id = read_u64(payload, 1);
                let opaque = &payload[9..];
                let state = &self.sources[usize::from(source)];
                if let Some((last_id, frame, last)) = &state.last_batch
                    && *last_id == id
                {
                    return Ok(if last == opaque {
                        ack(id, *frame)
                    } else {
                        error(OrderError::ModifiedRetry, *last_id)
                    });
                }
                if self.active & (1 << source) == 0 {
                    return Ok(error(OrderError::Inactive, 0));
                }
                let expected = state
                    .last_batch
                    .as_ref()
                    .map_or(Some(1), |(id, _, _)| id.checked_add(1))
                    .ok_or(RelayError::CounterExhausted)?;
                if id != expected {
                    return Ok(error(OrderError::BatchGap, expected));
                }
                let frame = self.head + 2;
                if frame > (MAX_MATCH_SECONDS * u64::from(self.certificate.tick_hz)) as u32 {
                    return Ok(error(OrderError::Inactive, 0));
                }
                if state.pending >= MAX_SOURCE_PENDING
                    || self
                        .pending
                        .get(&frame)
                        .is_some_and(|entries| entries.len() >= MAX_FRAME_BATCHES)
                {
                    return Ok(error(OrderError::Backpressure, expected));
                }
                if let Some(journal) = self.journal.as_mut()
                    && !journal.reserve_batch(22 + opaque.len())
                {
                    return Ok(error(OrderError::Backpressure, expected));
                }
                self.pending.entry(frame).or_default().push(Pending {
                    source,
                    batch_id: id,
                    opaque: opaque.to_vec(),
                });
                let state = &mut self.sources[usize::from(source)];
                state.last_batch = Some((id, frame, opaque.to_vec()));
                state.pending += 1;
                Ok(ack(id, frame))
            }
            (PULL_REQUEST, 9) => {
                let cursor = read_u64(payload, 1);
                if cursor < self.oldest_seq() {
                    return Ok(error(OrderError::HistoryGap, self.oldest_seq()));
                }
                if cursor > self.next_seq {
                    return Ok(error(OrderError::InvalidCursor, self.next_seq));
                }
                let mut output = vec![self.status(now_ms)];
                if cursor < self.hot_oldest_seq() {
                    let Some(journal) = self.journal.as_mut() else {
                        return Ok(error(OrderError::HistoryGap, self.oldest_seq()));
                    };
                    match journal.read(cursor, ORDERING_PULL_LIMIT) {
                        Ok(records) => output.extend(records),
                        Err(_) => {
                            self.storage_failed = true;
                            return Ok(error(OrderError::StorageUnavailable, cursor));
                        }
                    }
                } else {
                    output.extend(
                        self.history
                            .iter()
                            .skip((cursor - self.hot_oldest_seq()) as usize)
                            .take(ORDERING_PULL_LIMIT)
                            .map(|entry| entry.bytes.clone()),
                    );
                }
                Ok(output)
            }
            _ => Err(RelayError::Malformed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(key: &SecretKey, source: u8) -> ClientSession {
        let ticket = Ticket {
            room: [5; 16],
            source,
            expires: 1200,
            nonce: [source + 1; 16],
            player: [source + 1; 16],
            generation: 1,
        }
        .sign(key)
        .unwrap();
        ClientSession::new(
            &ticket,
            *derive_packet_key(key, &ticket).expose_for_https(),
            [89; 32],
        )
        .unwrap()
    }

    struct TestDirectory(std::path::PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let mut random = [0; 16];
            getrandom::fill(&mut random).unwrap();
            let name: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
            Self(std::env::temp_dir().join(format!("vera-relay-journal-{name}")))
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn unix(now_ms: u64) -> u64 {
        1000 + now_ms / 1000
    }

    fn renew(server: &mut RelayServer, client: &mut ClientSession, now_ms: u64) {
        let mut ticket = client.ticket;
        ticket.expires = unix(now_ms) + 200;
        ticket.nonce[15] = ticket.nonce[15].wrapping_add(1);
        let signed = ticket.sign(&server.key).unwrap();
        let mut candidate = client
            .renewal(
                &signed,
                *derive_packet_key(&server.key, &signed).expose_for_https(),
                [89; 32],
            )
            .unwrap();
        connect(server, &mut candidate, now_ms);
        candidate.adopt_ordering_from(client).unwrap();
        *client = candidate;
    }

    fn endpoint(client: &ClientSession) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31000 + u16::from(client.source()))
    }

    fn connect(server: &mut RelayServer, client: &mut ClientSession, now_ms: u64) {
        let mut bytes = Vec::new();
        client.probe(&mut bytes).unwrap();
        let challenge = server
            .process_at(endpoint(client), &bytes, unix(now_ms), now_ms)
            .unwrap()
            .remove(0);
        let ServerEvent::Challenge(cookie) = client.receive(&challenge.bytes).unwrap() else {
            panic!("challenge");
        };
        client.confirm(cookie, &mut bytes).unwrap();
        let ready = server
            .process_at(endpoint(client), &bytes, unix(now_ms), now_ms)
            .unwrap()
            .remove(0);
        assert!(matches!(
            client.receive(&ready.bytes),
            Ok(ServerEvent::Ready)
        ));
    }

    fn events(
        server: &mut RelayServer,
        client: &mut ClientSession,
        bytes: &[u8],
        now_ms: u64,
    ) -> Vec<OrderEvent> {
        server
            .process_at(endpoint(client), bytes, unix(now_ms), now_ms)
            .unwrap()
            .into_iter()
            .map(|packet| {
                let ServerEvent::Ordering(event) = client.receive(&packet.bytes).unwrap() else {
                    panic!("ordering");
                };
                event
            })
            .collect()
    }

    fn configure(
        server: &mut RelayServer,
        client: &mut ClientSession,
        cert: &[u8],
        now_ms: u64,
    ) -> Vec<OrderEvent> {
        let mut bytes = Vec::new();
        client
            .configure_ordering(cert, [7; 32], &mut bytes)
            .unwrap();
        events(server, client, &bytes, now_ms)
    }

    fn pull(
        server: &mut RelayServer,
        client: &mut ClientSession,
        cursor: u64,
        now_ms: u64,
    ) -> Vec<OrderEvent> {
        let mut bytes = Vec::new();
        client.pull_ordering(cursor, &mut bytes).unwrap();
        events(server, client, &bytes, now_ms)
    }

    fn certificate(key: &SecretKey, mask: u32, expires: u64) -> [u8; ORDERING_CERTIFICATE_LEN] {
        OrderingCertificate {
            room: [5; 16],
            sources_mask: mask,
            tick_hz: 45,
            expires,
        }
        .sign(key)
        .unwrap()
    }

    #[test]
    fn terminal_leave_survives_loss_expired_admission_and_deleted_room() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 2000);
        for participant in [&mut first, &mut second] {
            connect(&mut server, participant, 0);
            configure(&mut server, participant, &cert, 0);
        }
        server.advance_clock(1000, unix(1000));
        let mut leave = Vec::new();
        first.leave_ordering(&mut leave).unwrap();
        // Lose the first Leave, then let both hop tickets and admissions
        // expire. Silence alone must not remove either player.
        server.advance_clock(350_000, unix(350_000));
        assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().active, 3);
        let mut forged = leave.clone();
        *forged.last_mut().unwrap() ^= 1;
        assert!(matches!(
            server.process_at(endpoint(&first), &forged, unix(350_000), 350_000),
            Err(RelayError::Authentication)
        ));
        let mut bytes = Vec::new();
        first
            .submit_ordering(1, b"expired input", &mut bytes)
            .unwrap();
        assert!(matches!(
            server.process_at(endpoint(&first), &bytes, unix(350_000), 350_000),
            Err(RelayError::Expired)
        ));
        first.probe(&mut bytes).unwrap();
        assert!(matches!(
            server.process_at(endpoint(&first), &bytes, unix(350_000), 350_000),
            Err(RelayError::Expired)
        ));
        first.forget_readiness();
        first.leave_ordering(&mut bytes).unwrap();
        assert_eq!(bytes, leave, "terminal retry preserves exact authority");
        let lost_ack = server
            .process_at(endpoint(&first), &leave, unix(350_000), 350_000)
            .unwrap();
        assert_eq!(lost_ack.len(), 1);
        assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().active, 2);
        second.leave_ordering(&mut bytes).unwrap();
        server
            .process_at(endpoint(&second), &bytes, unix(351_000), 351_000)
            .unwrap();
        // Lose all ACKs until after the completed archive's existing grace.
        server.advance_clock(351_100, unix(351_100));
        server.advance_clock(652_000, unix(652_000));
        let room = directory.0.join("05".repeat(16));
        assert!(!room.exists());
        assert_eq!(server.room_count(), 0);
        let ack = server
            .process_at(endpoint(&first), &leave, unix(652_000), 652_000)
            .unwrap();
        assert!(matches!(
            first.receive(&ack[0].bytes),
            Ok(ServerEvent::Ordering(OrderEvent::Left))
        ));
        assert_eq!(server.room_count(), 0, "terminal ACK never recreates state");
        assert!(!room.exists());
    }

    #[test]
    fn cancelled_start_reclaims_the_empty_archive_without_reopening_stale_tickets() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 16, &directory.0).unwrap();
        // Nine quick cancellations cross the eight-room archive quota without
        // waiting for any tombstone's short-ticket grace to elapse.
        for attempt in 1..=9 {
            let now_ms = u64::from(attempt) * 1000;
            let mut ticket = client(&key, 0).ticket;
            ticket.room = [attempt; 16];
            ticket.generation = u64::from(attempt);
            let signed = ticket.sign(&key).unwrap();
            let mut participant = ClientSession::new(
                &signed,
                *derive_packet_key(&key, &signed).expose_for_https(),
                [89; 32],
            )
            .unwrap();
            connect(&mut server, &mut participant, now_ms);
            let cert = OrderingCertificate {
                room: ticket.room,
                sources_mask: 3,
                tick_hz: 45,
                expires: 2000,
            }
            .sign(&key)
            .unwrap();
            let ready = if attempt == 1 {
                // Lose this source's first Ready while its peer creates the
                // room. Local retirement must still cancel that full quorum.
                let mut lost_ready = Vec::new();
                participant
                    .configure_ordering(&cert, [7; 32], &mut lost_ready)
                    .unwrap();
                let mut peer_ticket = client(&key, 1).ticket;
                peer_ticket.room = ticket.room;
                let signed = peer_ticket.sign(&key).unwrap();
                let mut peer = ClientSession::new(
                    &signed,
                    *derive_packet_key(&key, &signed).expose_for_https(),
                    [89; 32],
                )
                .unwrap();
                connect(&mut server, &mut peer, now_ms);
                let ready = configure(&mut server, &mut peer, &cert, now_ms);
                assert!(
                    ready
                        .iter()
                        .any(|event| matches!(event, OrderEvent::Ready { ready_mask: 2, .. }))
                );
                ready
            } else {
                configure(&mut server, &mut participant, &cert, now_ms)
            };
            assert!(ready.iter().any(|event| matches!(event,
                OrderEvent::Status(status) if !status.started && status.head_frame == 0)));
            let room = directory.0.join(format!("{attempt:02x}").repeat(16));
            assert_eq!(
                std::fs::metadata(room.join("stream.data")).unwrap().len(),
                0
            );
            let mut bytes = Vec::new();
            participant.leave_ordering(&mut bytes).unwrap();
            assert!(
                events(&mut server, &mut participant, &bytes, now_ms)
                    .iter()
                    .any(|event| matches!(event, OrderEvent::Left))
            );
            server.advance_clock(now_ms, unix(now_ms));
            assert!(!room.exists(), "empty archive reservation must be reusable");
            assert!(matches!(
                configure(&mut server, &mut participant, &cert, now_ms).as_slice(),
                [OrderEvent::Error {
                    code: OrderError::Inactive,
                    ..
                }]
            ));
            assert!(!room.exists(), "stale Ready must not recreate the archive");
        }
        assert_eq!(
            server.room_count(),
            9,
            "inactive tombstones retain their grace"
        );
        // An HTTP cancellation can lose its final UDP Leave. Its retained
        // source binding must still authorize cleanup after the short hop
        // admission and ticket have both expired.
        let mut ticket = client(&key, 0).ticket;
        ticket.room = [10; 16];
        ticket.generation = 10;
        let signed = ticket.sign(&key).unwrap();
        let mut abandoned = ClientSession::new(
            &signed,
            *derive_packet_key(&key, &signed).expose_for_https(),
            [89; 32],
        )
        .unwrap();
        connect(&mut server, &mut abandoned, 10_000);
        let mut cert = OrderingCertificate::parse(&certificate(&key, 3, 2000)).unwrap();
        cert.room = ticket.room;
        let abandoned_cert = cert.sign(&key).unwrap();
        configure(&mut server, &mut abandoned, &abandoned_cert, 10_000);
        let abandoned_path = directory.0.join("0a".repeat(16));
        let after_grace = MAX_TICKET_LIFETIME * 1000 + 10_000;
        server.advance_clock(after_grace, unix(after_grace));
        assert_eq!(server.room_count(), 1, "only the unstarted archive remains");
        assert!(abandoned_path.exists());
        ticket.room = [11; 16];
        ticket.generation = 11;
        ticket.expires = unix(after_grace) + 200;
        let signed = ticket.sign(&key).unwrap();
        let mut next = ClientSession::new(
            &signed,
            *derive_packet_key(&key, &signed).expose_for_https(),
            [89; 32],
        )
        .unwrap();
        connect(&mut server, &mut next, after_grace);
        assert!(
            !abandoned_path.exists(),
            "new admission reclaims the lost Leave"
        );
        let mut bytes = Vec::new();
        abandoned
            .configure_ordering(&abandoned_cert, [7; 32], &mut bytes)
            .unwrap();
        assert!(matches!(
            server.process_at(endpoint(&abandoned), &bytes, unix(after_grace), after_grace),
            Err(RelayError::Expired)
        ));
        assert!(!abandoned_path.exists());
        cert.room = ticket.room;
        assert!(
            configure(
                &mut server,
                &mut next,
                &cert.sign(&key).unwrap(),
                after_grace
            )
            .iter()
            .any(|event| matches!(event, OrderEvent::Status(_)))
        );
        next.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut next, &bytes, after_grace);
        let finished = after_grace + MAX_TICKET_LIFETIME * 1000;
        server.advance_clock(finished, unix(finished));
        assert_eq!(server.room_count(), 0);
    }

    #[test]
    fn disconnected_host_renews_after_ticket_expiry_and_replays_the_full_disk_stream() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut host, mut active, mut third) = (client(&key, 0), client(&key, 1), client(&key, 2));
        for client in [&mut host, &mut active, &mut third] {
            connect(&mut server, client, 0);
        }
        let cert = certificate(&key, 7, 2000);
        configure(&mut server, &mut host, &cert, 0);
        configure(&mut server, &mut active, &cert, 0);
        server.advance_clock(1000, unix(1000));
        assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().head, 0);
        configure(&mut server, &mut third, &cert, 1000);
        // Reverse UDP order is canonicalized by source, including empty frames.
        let mut bytes = Vec::new();
        third
            .submit_ordering(1, b"third input", &mut bytes)
            .unwrap();
        events(&mut server, &mut third, &bytes, 1000);
        active
            .submit_ordering(1, b"active input", &mut bytes)
            .unwrap();
        events(&mut server, &mut active, &bytes, 1000);
        let received = pull(&mut server, &mut active, 1, 1300);
        let batch: Vec<_> = received
            .iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::Batch {
                    sequence,
                    frame,
                    source,
                    bytes,
                    ..
                }) => Some((*sequence, *frame, *source, bytes.as_bytes())),
                _ => None,
            })
            .collect();
        assert_eq!(
            batch,
            [
                (2, 2, 1, b"active input".as_slice()),
                (3, 2, 2, b"third input".as_slice())
            ]
        );
        let ends: Vec<_> = received
            .iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame }) => {
                    Some((*sequence, *frame))
                }
                _ => None,
            })
            .collect();
        assert_eq!(ends, [(1, 1), (4, 2), (5, 3), (6, 4)]);
        active
            .submit_ordering(2, b"healthy input during outage", &mut bytes)
            .unwrap();
        events(&mut server, &mut active, &bytes, 30_000);
        server.advance_clock(250_000, unix(250_000));
        assert!(
            server.rooms[&[5; 16]].peers.is_empty(),
            "every short hop lease has expired"
        );
        let ordering = server.rooms[&[5; 16]].ordering.as_ref().unwrap();
        assert!(ordering.head > 4096);
        assert!(ordering.hot_oldest_seq() > 1);
        assert_eq!(ordering.active, 7);
        renew(&mut server, &mut active, 250_000);
        configure(&mut server, &mut active, &cert, 250_000);
        active
            .submit_ordering(3, b"healthy input after lease renewal", &mut bytes)
            .unwrap();
        let acked = events(&mut server, &mut active, &bytes, 250_000);
        assert!(matches!(acked[0], OrderEvent::Ack { batch_id: 3, frame } if frame > 4096));
        // The host returns after >300 seconds, retains its original principal,
        // and starts from sequence1 even though the hot cache evicted it.
        server.advance_clock(350_000, unix(350_000));
        host.probe(&mut bytes).unwrap();
        assert!(matches!(
            server.process_at(endpoint(&host), &bytes, unix(350_000), 350_000),
            Err(RelayError::Expired)
        ));
        renew(&mut server, &mut host, 350_000);
        configure(&mut server, &mut host, &cert, 350_000);
        let mut cursor = 1;
        let mut last_frame = 0;
        let mut saw_old_input = false;
        let mut saw_new_input = false;
        let mut caught_up = false;
        let mut now_ms = 350_000;
        for attempt in 0..600 {
            now_ms += 25;
            if attempt == 100 {
                active
                    .submit_ordering(4, b"healthy input while host catches up", &mut bytes)
                    .unwrap();
                assert!(matches!(
                    events(&mut server, &mut active, &bytes, now_ms)[0],
                    OrderEvent::Ack { batch_id: 4, .. }
                ));
            }
            let replay = pull(&mut server, &mut host, cursor, now_ms);
            let OrderEvent::Status(status) = replay[0] else {
                panic!("full-history replay must remain available");
            };
            assert_eq!(status.oldest_seq, 1);
            assert_eq!(status.active_sources, 7);
            for event in &replay[1..] {
                match event {
                    OrderEvent::Entry(OrderEntry::Batch {
                        sequence, bytes, ..
                    }) => {
                        assert_eq!(*sequence, cursor);
                        cursor += 1;
                        saw_old_input |= bytes.as_bytes() == b"active input";
                        saw_new_input |= bytes.as_bytes() == b"healthy input after lease renewal";
                    }
                    OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame }) => {
                        assert_eq!(*sequence, cursor);
                        cursor += 1;
                        assert_eq!(*frame, last_frame + 1);
                        last_frame = *frame;
                    }
                    _ => panic!("an outage is never a departure"),
                }
            }
            if cursor == status.next_seq {
                caught_up = true;
                break;
            }
        }
        assert!(caught_up && saw_old_input && saw_new_input);
        assert!(last_frame > 15_000);
        host.submit_ordering(1, b"recovered host input", &mut bytes)
            .unwrap();
        assert!(
            matches!(events(&mut server, &mut host, &bytes, now_ms)[0], OrderEvent::Ack { batch_id: 1, frame } if frame > last_frame)
        );
        assert!(
            server.rooms[&[5; 16]]
                .ordering
                .as_ref()
                .unwrap()
                .history
                .len()
                <= MAX_HISTORY_ENTRIES
        );
    }

    #[test]
    fn accepted_future_batches_deduplicate_and_renewal_recovers_a_lost_ack_from_own_stream() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        for client in [&mut first, &mut second] {
            connect(&mut server, client, 0);
        }
        let cert = certificate(&key, 3, 1300);
        configure(&mut server, &mut first, &cert, 0);
        configure(&mut server, &mut second, &cert, 0);
        let mut bytes = Vec::new();
        first
            .submit_ordering(1, b"private ordered command", &mut bytes)
            .unwrap();
        let opaque = bytes[CLIENT_HEADER..bytes.len() - MAC_LEN].to_vec();
        assert!(
            !bytes
                .windows(b"private ordered command".len())
                .any(|part| part == b"private ordered command")
        );
        let accepted = server
            .process_at(endpoint(&first), &bytes, 1000, 0)
            .unwrap();
        assert_eq!(accepted.len(), 1); // deliberately lose the ACK
        first
            .submit_ordering(1, b"private ordered command", &mut bytes)
            .unwrap();
        assert_eq!(&bytes[CLIENT_HEADER..bytes.len() - MAC_LEN], opaque);
        let duplicate = server
            .process_at(endpoint(&first), &bytes, 1000, 0)
            .unwrap();
        assert_eq!(duplicate.len(), 1);
        let mut modified = opaque.clone();
        *modified.last_mut().unwrap() ^= 1;
        first
            .encode(CONTROL, BROADCAST, &modified, &mut bytes)
            .unwrap();
        assert!(matches!(
            events(&mut server, &mut first, &bytes, 0)[0],
            OrderEvent::Error {
                code: OrderError::ModifiedRetry,
                ..
            }
        ));
        modified[1..9].copy_from_slice(&3u64.to_le_bytes());
        first
            .encode(CONTROL, BROADCAST, &modified, &mut bytes)
            .unwrap();
        assert!(matches!(
            events(&mut server, &mut first, &bytes, 0)[0],
            OrderEvent::Error {
                code: OrderError::BatchGap,
                expected: 2
            }
        ));
        let mut renewed_ticket = first.ticket;
        renewed_ticket.nonce = [42; 16];
        renewed_ticket.expires = 1250;
        let signed = renewed_ticket.sign(&key).unwrap();
        let mut candidate = first
            .renewal(
                &signed,
                *derive_packet_key(&key, &signed).expose_for_https(),
                [89; 32],
            )
            .unwrap();
        connect(&mut server, &mut candidate, 300);
        candidate.adopt_ordering_from(&mut first).unwrap();
        assert!(first.ordering.is_none());
        first = candidate;
        let fresh_cert = certificate(&key, 3, 1400);
        configure(&mut server, &mut first, &fresh_cert, 300);
        first
            .submit_ordering(1, b"private ordered command", &mut bytes)
            .unwrap();
        assert_eq!(&bytes[CLIENT_HEADER..bytes.len() - MAC_LEN], opaque);
        server
            .process_at(endpoint(&first), &bytes, 1000, 300)
            .unwrap(); // lose ACK again
        let entries = pull(&mut server, &mut first, 1, 300);
        assert_eq!(
            entries
                .iter()
                .filter(|event| matches!(
                    event,
                    OrderEvent::Entry(OrderEntry::Batch { batch_id: 1, .. })
                ))
                .count(),
            1
        );
        first.submit_ordering(2, b"next batch", &mut bytes).unwrap();
        assert!(matches!(
            events(&mut server, &mut first, &bytes, 300)[0],
            OrderEvent::Ack {
                batch_id: 2,
                frame: 6
            }
        ));
        first.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut first, &bytes, 300);
        server.advance_clock(400, 1000);
        let tail = pull(&mut server, &mut second, 7, 400);
        let at_six: Vec<_> = tail
            .iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::Batch { frame: 6, .. }) => Some("batch"),
                OrderEvent::Entry(OrderEntry::Departure {
                    frame: 6,
                    source: 0,
                    ..
                }) => Some("departure"),
                _ => None,
            })
            .collect();
        assert_eq!(at_six, ["batch", "departure"]);
        second.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut second, &bytes, 400);
        server.advance_clock(600, unix(600));
        let room = directory.0.join("05".repeat(16));
        assert!(
            room.join("stream.data").exists(),
            "terminal replay retains the short-ticket grace"
        );
        server.advance_clock(301_000, unix(301_000));
        assert!(
            !room.exists(),
            "explicit-all-left retires only the completed journal"
        );
    }

    #[test]
    fn archive_authentication_tamper_and_storage_failure_preserve_the_committed_stream() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        for client in [&mut first, &mut second] {
            connect(&mut server, client, 0);
        }
        let cert = certificate(&key, 3, 1300);
        let mut forged = cert;
        forged[34] ^= 1;
        let mut bytes = Vec::new();
        // A registered source outside the signed manifest cannot join the
        // ready quorum even with a legitimate hop ticket for this room.
        let mut outsider = client(&key, 2);
        connect(&mut server, &mut outsider, 0);
        assert!(matches!(
            outsider.configure_ordering(&cert, [7; 32], &mut bytes),
            Err(RelayError::Authentication)
        ));
        first
            .configure_ordering(&forged, [7; 32], &mut bytes)
            .unwrap();
        assert!(matches!(
            server.process_at(endpoint(&first), &bytes, 1000, 0),
            Err(RelayError::Authentication)
        ));
        assert!(
            OrderingCertificate {
                sources_mask: 1 << MAX_PEERS,
                ..OrderingCertificate::parse(&cert).unwrap()
            }
            .sign(&key)
            .is_err()
        );
        first.ordering = None;
        configure(&mut server, &mut first, &cert, 0);
        configure(&mut server, &mut second, &cert, 0);
        first
            .submit_ordering(1, b"authenticated plaintext", &mut bytes)
            .unwrap();
        events(&mut server, &mut first, &bytes, 0);
        second.pull_ordering(1, &mut bytes).unwrap();
        let packets = server
            .process_at(endpoint(&second), &bytes, 1000, 300)
            .unwrap();
        let mut tampered = packets
            .into_iter()
            .find(|packet| packet.bytes[SERVER_HEADER] == BATCH_EVENT)
            .unwrap()
            .bytes;
        // Even a hop-authenticated ciphertext with altered associated batch id
        // cannot be released as plaintext to the gameplay owner.
        tampered[SERVER_HEADER + 14] ^= 1;
        let split = tampered.len() - MAC_LEN;
        let tag = second
            .key
            .mac(SERVER_DOMAIN, &tampered[..split])
            .finalize()
            .into_bytes();
        tampered[split..].copy_from_slice(&tag);
        assert!(matches!(
            second.receive(&tampered),
            Err(RelayError::Authentication)
        ));
        server.advance_clock(112_000, unix(112_000));
        let replay = pull(&mut server, &mut second, 1, 112_000);
        assert!(matches!(
            replay[0],
            OrderEvent::Status(OrderStatus { oldest_seq: 1, .. })
        ));
        assert!(matches!(
            replay[1],
            OrderEvent::Entry(OrderEntry::FrameEnd {
                sequence: 1,
                frame: 1
            })
        ));
        assert!(
            server.rooms[&[5; 16]]
                .ordering
                .as_ref()
                .unwrap()
                .history
                .len()
                <= MAX_HISTORY_ENTRIES
        );
        assert!(server.ordering_memory() <= GLOBAL_ORDERING_BYTES);
        let cursor = server.rooms[&[5; 16]].ordering.as_ref().unwrap().next_seq;
        assert!(
            matches!(pull(&mut server, &mut second, cursor + 1, 112_000)[0], OrderEvent::Error { code: OrderError::InvalidCursor, expected } if expected == cursor)
        );
        let room = directory.0.join("05".repeat(16));
        let before = std::fs::read(room.join("stream.data")).unwrap();
        assert!(
            !before
                .windows(b"authenticated plaintext".len())
                .any(|part| part == b"authenticated plaintext")
        );
        // Restart migration is unsupported: an existing journal is never
        // overwritten, even by a valid freshly connected principal.
        let mut restarted = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let mut newcomer = client(&key, 0);
        connect(&mut restarted, &mut newcomer, 0);
        assert!(matches!(
            configure(&mut restarted, &mut newcomer, &cert, 0)[0],
            OrderEvent::Error {
                code: OrderError::StorageUnavailable,
                ..
            }
        ));
        assert_eq!(std::fs::read(room.join("stream.data")).unwrap(), before);
        // A broken offset index yields a storage failure, not a fabricated
        // history gap or implicit departure. Preserve the data for inspection.
        std::fs::OpenOptions::new()
            .write(true)
            .open(room.join("stream.index"))
            .unwrap()
            .set_len(0)
            .unwrap();
        assert!(matches!(
            pull(&mut server, &mut second, 1, 112_000)[0],
            OrderEvent::Error {
                code: OrderError::StorageUnavailable,
                ..
            }
        ));
        let head = server.rooms[&[5; 16]].ordering.as_ref().unwrap().head;
        server.advance_clock(113_000, unix(113_000));
        assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().head, head);
        assert_eq!(std::fs::read(room.join("stream.data")).unwrap(), before);
    }
}

impl RelayServer {
    pub(super) fn ordering_leave(
        &mut self,
        endpoint: SocketAddrV4,
        message: &Incoming<'_>,
        now: u64,
        now_ms: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        let cost = (SERVER_HEADER + MAC_LEN) as u64;
        if !self.outgoing.permits(now, cost, TOTAL_BYTES_PER_SECOND)
            || !self.order_packets.permits(now, 1, INGRESS_PER_SECOND)
        {
            return Err(RelayError::RateLimited);
        }
        self.advance_ordering_room(message.ticket.room, now_ms);
        if let Some(room) = self.rooms.get_mut(&message.ticket.room) {
            if !room.outgoing.permits(now, cost, ROOM_BYTES_PER_SECOND)
                || !room.order_packets.permits(now, 1, 8192)
            {
                return Err(RelayError::RateLimited);
            }
            if let Some(ordering) = room.ordering.as_mut() {
                if ordering.certificate.sources_mask & (1 << message.ticket.source) == 0 {
                    return Err(RelayError::Authentication);
                }
                let principal = (message.ticket.player, message.ticket.generation);
                let source = &mut ordering.sources[usize::from(message.ticket.source)];
                if source.principal.is_some_and(|bound| bound != principal) {
                    return Err(RelayError::Authentication);
                }
                if !source.leave_packets.permits(now, 1, 8) {
                    return Err(RelayError::RateLimited);
                }
                source.leave_packets.charge(now, 1);
                if ordering.storage_failed {
                    return Err(RelayError::Unregistered);
                }
                // Like Ready, a signed ticket binds a certificate-admitted
                // source on first contact. Its initial Ready may have been
                // lost while another source already created this room.
                source.principal = Some(principal);
                ordering.depart(message.ticket.source, now_ms);
            } else {
                if room.peers.iter().any(|peer| {
                    peer.ticket.source == message.ticket.source
                        && (peer.ticket.player != message.ticket.player
                            || peer.ticket.generation != message.ticket.generation)
                }) {
                    return Err(RelayError::Authentication);
                }
                room.ordering_cancelled = true;
            }
            room.outgoing.charge(now, cost);
            room.order_packets.charge(now, 1);
        } else if message.ticket.expires > now {
            // An unseen live lease may still have Ready in flight. Only an
            // expired absent room is safely acknowledged without a tombstone.
            return Err(RelayError::Unregistered);
        }
        self.outgoing.charge(now, cost);
        self.order_packets.charge(now, 1);
        Ok(vec![ForwardDatagram {
            endpoint,
            bytes: encode_server(
                &message.ticket,
                &message.key,
                LEAVE_ACK,
                message.sequence,
                message.ticket.source,
                &[],
            ),
        }])
    }

    pub(super) fn retire_superseded_startups(&mut self, ticket: Ticket, now_ms: u64, now: u64) {
        // A higher lobby-signed generation for this principal proves its old
        // membership ended. Source bindings outlive short hop admissions, so
        // this also closes a canceled load whose final Leave was lost.
        let superseded: Vec<_> = self
            .rooms
            .iter()
            .filter_map(|(id, room)| {
                if *id == ticket.room {
                    return None;
                }
                let ordering = room.ordering.as_ref()?;
                if ordering.starts_at.is_some() || ordering.active == 0 || ordering.storage_failed {
                    return None;
                }
                ordering
                    .sources
                    .iter()
                    .position(|source| {
                        source.principal.is_some_and(|(player, generation)| {
                            player == ticket.player && generation < ticket.generation
                        })
                    })
                    .map(|source| (*id, source as u8))
            })
            .collect();
        if superseded.is_empty() {
            return;
        }
        for (id, source) in superseded {
            self.rooms
                .get_mut(&id)
                .unwrap()
                .ordering
                .as_mut()
                .unwrap()
                .depart(source, now_ms);
        }
        self.cleanup_ordering(now_ms, now);
    }

    /// Driven by the server clock even when no datagram arrives.
    pub fn advance_clock(&mut self, now_ms: u64, now: u64) {
        self.prune(now);
        let mut total = self.ordering_memory();
        let mut rooms: Vec<_> = self.rooms.keys().copied().collect();
        rooms.sort_unstable();
        for id in rooms {
            if let Some(ordering) = self
                .rooms
                .get_mut(&id)
                .and_then(|room| room.ordering.as_mut())
            {
                let before = ordering.memory();
                ordering.advance(now_ms);
                total = total
                    .saturating_sub(before)
                    .saturating_add(ordering.memory());
            }
            // Limit peak retained growth to one bounded room at a time.
            if total > GLOBAL_ORDERING_BYTES {
                self.reserve_ordering(0);
                total = self.ordering_memory();
            }
        }
        self.cleanup_ordering(now_ms, now);
    }

    fn cleanup_ordering(&mut self, now_ms: u64, now: u64) {
        let ended: Vec<_> = self
            .rooms
            .iter()
            .filter_map(|(id, room)| {
                room.ordering
                    .as_ref()
                    .filter(|ordering| {
                        ordering.cleanup_due(now_ms, now)
                            || (!ordering.storage_failed
                                && ordering.starts_at.is_none()
                                && ordering.finished_at.is_some()
                                && ordering.journal.is_some())
                    })
                    .map(|_| *id)
            })
            .collect();
        for id in ended {
            let room = self.rooms.get_mut(&id).unwrap();
            let ordering = room.ordering.as_mut().unwrap();
            let retire = ordering.cleanup_due(now_ms, now);
            if let Some(directory) = self.journal_directory.as_mut()
                && let Some(journal) = ordering.journal.take()
                && directory.cleanup(journal).is_err()
            {
                ordering.storage_failed = true;
                continue;
            }
            // No commands or frames can exist before the full Ready quorum.
            // Reclaim its empty archive immediately, retaining only the
            // inactive tombstone until every old hop ticket has expired.
            if !retire {
                continue;
            }
            room.ordering = None;
            if room.peers.is_empty() {
                self.rooms.remove(&id);
            }
        }
    }

    pub(super) fn advance_ordering_room(&mut self, id: [u8; 16], now_ms: u64) {
        if let Some(ordering) = self
            .rooms
            .get_mut(&id)
            .and_then(|room| room.ordering.as_mut())
        {
            let before = ordering.memory();
            ordering.advance(now_ms);
            if ordering.memory() > before {
                self.reserve_ordering(0);
            }
        }
    }

    fn ordering_memory(&self) -> usize {
        self.rooms
            .values()
            .filter_map(|room| room.ordering.as_ref())
            .map(OrderingRoom::memory)
            .sum()
    }

    fn reserve_ordering(&mut self, additional: usize) -> bool {
        let mut total = self.ordering_memory();
        while total.saturating_add(additional) > GLOBAL_ORDERING_BYTES {
            let oldest = self
                .rooms
                .iter()
                .filter_map(|(id, room)| {
                    let ordering = room.ordering.as_ref()?;
                    let entry = ordering.history.front()?;
                    Some((
                        *id,
                        ordering.starts_at.unwrap_or(0).saturating_add(
                            u64::from(entry.frame) * 1000 / u64::from(ordering.certificate.tick_hz),
                        ),
                    ))
                })
                .min_by_key(|(id, time)| (*time, *id));
            let Some((id, _)) = oldest else {
                return false;
            };
            let ordering = self.rooms.get_mut(&id).unwrap().ordering.as_mut().unwrap();
            let before = ordering.memory();
            ordering.pop_oldest();
            total -= before - ordering.memory();
        }
        true
    }

    pub(super) fn ordering_request(
        &mut self,
        message: &Incoming<'_>,
        now: u64,
        now_ms: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        let payload = message.payload;
        let replies = if payload.first() == Some(&READY_REQUEST) {
            if payload.len() != 1 + ORDERING_CERTIFICATE_LEN + 32 {
                return Err(RelayError::Malformed);
            }
            let certificate = OrderingCertificate::verify(
                &payload[1..1 + ORDERING_CERTIFICATE_LEN],
                &self.key,
                now,
            )?;
            if certificate.room != message.ticket.room
                || certificate.sources_mask & (1 << message.ticket.source) == 0
            {
                return Err(RelayError::Authentication);
            }
            if self
                .rooms
                .get(&message.ticket.room)
                .is_some_and(|room| room.ordering_cancelled)
            {
                return self.ordering_reply(message, error(OrderError::Inactive, 0), now);
            }
            let agreement: [u8; 32] = payload[1 + ORDERING_CERTIFICATE_LEN..].try_into().unwrap();
            let allocation = self
                .rooms
                .get(&message.ticket.room)
                .is_some_and(|room| room.ordering.is_none());
            if !self.reserve_ordering(if allocation {
                std::mem::size_of::<OrderingRoom>()
            } else {
                0
            }) {
                return Err(RelayError::Full);
            }
            let journal = if allocation {
                match self.journal_directory.as_mut() {
                    Some(directory) => {
                        match directory.create(certificate.room, certificate.tick_hz) {
                            Ok(journal) => Some(journal),
                            Err(_) => {
                                return self.ordering_reply(
                                    message,
                                    error(OrderError::StorageUnavailable, 0),
                                    now,
                                );
                            }
                        }
                    }
                    None => None,
                }
            } else {
                None
            };
            let room = self
                .rooms
                .get_mut(&message.ticket.room)
                .ok_or(RelayError::Unregistered)?;
            let ordering = room
                .ordering
                .get_or_insert_with(|| OrderingRoom::new(certificate, agreement, journal));
            if ordering.storage_failed {
                error(OrderError::StorageUnavailable, ordering.next_seq)
            } else if ordering.certificate.room != certificate.room
                || ordering.certificate.sources_mask != certificate.sources_mask
                || ordering.certificate.tick_hz != certificate.tick_hz
                || ordering.agreement != agreement
            {
                error(OrderError::AgreementMismatch, 0)
            } else if ordering.active & (1 << message.ticket.source) == 0 {
                error(OrderError::Inactive, 0)
            } else {
                let state = &mut ordering.sources[usize::from(message.ticket.source)];
                let principal = (message.ticket.player, message.ticket.generation);
                if state.principal.is_some_and(|bound| bound != principal) {
                    return Err(RelayError::Authentication);
                }
                state.principal = Some(principal);
                ordering.certificate.expires =
                    ordering.certificate.expires.max(certificate.expires);
                ordering.ready |= 1 << message.ticket.source;
                if ordering.ready == ordering.certificate.sources_mask
                    && ordering.starts_at.is_none()
                {
                    ordering.starts_at = Some(now_ms.saturating_add(START_DELAY_MS));
                }
                let mut ready = vec![READY_EVENT];
                ready.extend_from_slice(&ordering.ready.to_le_bytes());
                ready.extend_from_slice(&ordering.certificate.sources_mask.to_le_bytes());
                vec![ready, ordering.status(now_ms)]
            }
        } else if !self.reserve_ordering(if payload.first() == Some(&SUBMIT_REQUEST) {
            2 * (payload.len() + ENTRY_OVERHEAD)
        } else {
            0
        }) {
            error(OrderError::Backpressure, 0)
        } else {
            let room = self
                .rooms
                .get_mut(&message.ticket.room)
                .ok_or(RelayError::Unregistered)?;
            if let Some(ordering) = room.ordering.as_mut() {
                ordering.request(message.ticket, payload, now_ms)?
            } else {
                error(OrderError::NotConfigured, 0)
            }
        };
        self.ordering_reply(message, replies, now)
    }

    fn ordering_reply(
        &mut self,
        message: &Incoming<'_>,
        mut replies: Vec<Vec<u8>>,
        now: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        let cost = replies
            .iter()
            .map(|payload| SERVER_HEADER + MAC_LEN + payload.len())
            .sum::<usize>() as u64;
        let packets = replies.len() as u64;
        let room = self
            .rooms
            .get_mut(&message.ticket.room)
            .ok_or(RelayError::Unregistered)?;
        let admission = self
            .admissions
            .get_mut(&message.ticket.player)
            .ok_or(RelayError::Unregistered)?;
        if !admission
            .outgoing
            .permits(now, cost, PLAYER_BYTES_PER_SECOND)
            || !room.outgoing.permits(now, cost, ROOM_BYTES_PER_SECOND)
            || !self.outgoing.permits(now, cost, TOTAL_BYTES_PER_SECOND)
            || !admission.order_packets.permits(now, packets, 2048)
            || !room.order_packets.permits(now, packets, 8192)
            || !self.order_packets.permits(now, packets, INGRESS_PER_SECOND)
        {
            return Err(RelayError::RateLimited);
        }
        admission.outgoing.charge(now, cost);
        room.outgoing.charge(now, cost);
        self.outgoing.charge(now, cost);
        admission.order_packets.charge(now, packets);
        room.order_packets.charge(now, packets);
        self.order_packets.charge(now, packets);
        let peer = room
            .peers
            .iter_mut()
            .find(|peer| peer.ticket == message.ticket)
            .ok_or(RelayError::Unregistered)?;
        replies
            .drain(..)
            .map(|payload| server_packet(peer, CONTROL, message.ticket.source, &payload))
            .collect()
    }
}
