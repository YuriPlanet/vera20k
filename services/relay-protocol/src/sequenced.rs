//! Fixed-clock ordering of plaintext batches (bandwidth-compaction spec §3). A reader's cursor never
//! participates in clock advancement, admission, or another reader's progress.

use std::collections::{BTreeMap, VecDeque};

use super::*;
use crate::link::Reader;

mod downlink;
pub(crate) mod federation;
pub(crate) mod pack;

/// A `VO1C` certificate; `VO2C` adds one flags byte.
pub const ORDERING_CERTIFICATE_LEN: usize = 66;
pub const ORDERING_CERTIFICATE_V2_LEN: usize = 67;
pub const ORDERING_BATCH_CAPACITY: usize = 512;
pub const ORDERING_PULL_LIMIT: usize = 32;
const CERTIFICATE_MAGIC: &[u8; 4] = b"VO1C";
const CERTIFICATE_DOMAIN: &[u8] = b"VERA20k relay ordering certificate v1\0";
const CERTIFICATE_V2_MAGIC: &[u8; 4] = b"VO2C";
const CERTIFICATE_V2_DOMAIN: &[u8] = b"VERA20k relay ordering certificate v2\0";
// VO2C flags byte: the host turned relay pacing off.
const CERTIFICATE_PACING_OFF: u8 = 1;
const MAX_MATCH_SECONDS: u64 = 24 * 60 * 60;
const START_DELAY_MS: u64 = 200;
const MAX_HISTORY_ENTRIES: usize = 4096;
const ROOM_HISTORY_BYTES: usize = 2 * 1024 * 1024;
const GLOBAL_ORDERING_BYTES: usize = 64 * 1024 * 1024;
const MAX_SOURCE_PENDING: usize = 8;
const MAX_FRAME_BATCHES: usize = 32;
// A loading source that sends nothing this long departs in the first frame,
// and the others start without it.
const LOADING_SILENCE_MS: u64 = 60_000;
// Even a loading source that still answers departs once the room is this old.
const LOADING_LIMIT_MS: u64 = 180_000;
// A started source silent this long departs through the stream. A shorter
// outage keeps its place for the returning player's replay.
const ABSENT_MS: u64 = 600_000;
// A started room ends at its final frame, which pauses postpone, and silence
// ends an abandoned one. Past this age it is retired anyway: a day of play and
// as long again paused.
pub(super) const ROOM_LIFETIME_MS: u64 =
    LOADING_LIMIT_MS + START_DELAY_MS + 2 * MAX_MATCH_SECONDS * 1000;
// After a resume nobody may pause for this long, so no player can hold a
// match paused by pausing again after every resume.
const PAUSE_COOLDOWN_MS: u64 = 30_000;
const ENTRY_OVERHEAD: usize = 128;
const READY_REQUEST: u8 = 0;
const PULL_REQUEST: u8 = 2;
const PAUSE_REQUEST: u8 = 5;
/// Every this many frames each client reports its post-Logic checksum; the
/// relay takes no other frame's.
pub const CHECKSUM_INTERVAL: u32 = 15;
// Sampled frames whose checksums a room keeps, the newest.
const CHECKSUM_FRAMES: usize = 256;
const PAUSE_STATUS_EVENT: u8 = 8;
const PAUSE_EVENT: u8 = 9;
const PAUSE_ENTRY_BYTES: usize = 23;
const RATE_EVENT: u8 = 10;
const RATE_ENTRY_BYTES: usize = 16;
const DESYNC_EVENT: u8 = 11;
const DESYNC_ENTRY_BYTES: usize = 27;
// A DESYNC record after its sequence and frame: sample frame u32, outlier
// source u8 and checksum u32, other source u8 and checksum u32.
pub(super) const DESYNC_TAIL: usize = DESYNC_ENTRY_BYTES - 13;
// Consecutive history entries in one datagram, PACK v2 (compaction spec
// §5.2, `pack.rs`).
pub(super) const PACK_EVENT: u8 = 12;
// A federated control record travels between relays as an entry of this
// source: a rate change under `RATE_BATCH`, a desync under `DESYNC_BATCH`.
pub(super) const CONTROL_SOURCE: u8 = u8::MAX;
pub(super) const RATE_BATCH: u64 = u64::MAX;
pub(super) const DESYNC_BATCH: u64 = u64::MAX - 1;
// Game Speed's frame rates, fastest first: the rates a certificate may name.
const FRAME_RATES: [u16; 7] = [60, 45, 30, 20, 15, 12, 10];
// A player's report counts this long; a silent client slows nobody.
const PACE_REPORT_MS: u64 = 3_000;
// The share of each second a reporter's simulation may take at a rate it
// can hold; the rest draws and catches up.
const SUSTAIN_PERCENT: u32 = 80;
// Rate changes are at least this far apart, so reports show a step's effect
// before the next one.
const PACE_STEP_MS: u64 = 5_000;
// With nobody behind this long the clock tries the next faster rate. A
// slowdown this soon after a raise doubles the wait, up to the cap.
const PACE_RAISE_MS: u64 = 30_000;
const PACE_RAISE_MAX_MS: u64 = 300_000;
const PUSH_WINDOW: u64 = 64;
const PUSH_LEASE_MS: u64 = 3_000;
const PUSH_STATUS_MS: u64 = 250;
const PUSH_PEER_PACKETS: usize = 8;
const PUSH_ROOM_PACKETS: usize = 32;
const PUSH_DRAIN_PACKETS: usize = 128;
const PUSH_DRAIN_BYTES: usize = 96 * 1024;
// A reader sees a lost push only at a later entry, then repairs it a round
// trip later. Unacknowledged entries are pushed once more when the oldest one
// still owed a copy is this old: past a back-to-back drop and inside the
// reader's one-frame gap grace.
const PUSH_REPEAT_MS: u64 = 8;
// Repeats per second relay-wide. They spend no fresh budget, so they add at
// most half the relay's fresh total to its ordering output.
const PUSH_REPEAT_PACKETS: u64 = 8192;
// One room's fresh ordering packets per second. Entries reach each reader
// packed, a turn's in about one packet, so a full 30-source room at 60 Hz
// needs under a thousand at 300 actions a minute, before repairs. The cap stays
// below the relay-wide total so a flooding room's asks leave the rest unreserved for
// matches that start or repair.
const ROOM_ORDER_PACKETS: u64 = 12_288;
pub(super) const LEAVE_REQUEST: u8 = 3;
const READY_EVENT: u8 = 0;
const STATUS_EVENT: u8 = 1;
const ACK_EVENT: u8 = 2;
const BATCH_EVENT: u8 = 3;
pub(super) const FRAME_END_EVENT: u8 = 4;
pub(super) const DEPARTURE_EVENT: u8 = 5;
const ERROR_EVENT: u8 = 6;

/// What a certificate says about relay pacing (README, Pace).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CertificatePacing {
    /// `VO1C`: members may be released clients that read no rate marker, so
    /// the clock changes only once every active source has reported.
    #[default]
    Legacy,
    /// `VO2C`: every member reads rate markers; a player who cannot keep up
    /// slows the clock.
    On,
    /// `VO2C` with the host's pacing switch off: the clock keeps its rate.
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderingCertificate {
    pub room: [u8; 16],
    pub sources_mask: u32,
    pub tick_hz: u16,
    pub expires: u64,
    pub pacing: CertificatePacing,
}

/// Either certificate length, for a READY payload's split.
pub fn is_ordering_certificate_len(length: usize) -> bool {
    length == ORDERING_CERTIFICATE_LEN || length == ORDERING_CERTIFICATE_V2_LEN
}

impl OrderingCertificate {
    pub fn parse(bytes: &[u8]) -> Result<Self, RelayError> {
        let pacing = match (bytes.len(), bytes.get(..4)) {
            (ORDERING_CERTIFICATE_LEN, Some(magic)) if magic == CERTIFICATE_MAGIC => {
                CertificatePacing::Legacy
            }
            (ORDERING_CERTIFICATE_V2_LEN, Some(magic)) if magic == CERTIFICATE_V2_MAGIC => {
                match bytes[34] {
                    0 => CertificatePacing::On,
                    CERTIFICATE_PACING_OFF => CertificatePacing::Off,
                    _ => return Err(RelayError::Malformed),
                }
            }
            _ => return Err(RelayError::Malformed),
        };
        let value = Self {
            room: bytes[4..20].try_into().unwrap(),
            sources_mask: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            tick_hz: u16::from_le_bytes(bytes[24..26].try_into().unwrap()),
            expires: u64::from_le_bytes(bytes[26..34].try_into().unwrap()),
            pacing,
        };
        value.validate()?;
        Ok(value)
    }

    fn domain(pacing: CertificatePacing) -> &'static [u8] {
        match pacing {
            CertificatePacing::Legacy => CERTIFICATE_DOMAIN,
            _ => CERTIFICATE_V2_DOMAIN,
        }
    }

    fn validate(&self) -> Result<(), RelayError> {
        if self.room == [0; 16]
            || self.sources_mask == 0
            || self.sources_mask >> MAX_PEERS != 0
            || !FRAME_RATES.contains(&self.tick_hz)
        {
            return Err(RelayError::Malformed);
        }
        Ok(())
    }

    /// The HTTPS lobby signs this public manifest separately from hop tickets.
    /// Layout: VO1C, room16, LE mask32, LE Hz16, LE expiry64, domain HMAC32;
    /// VO2C adds a flags byte (bit 0: pacing off) before its own domain's HMAC.
    pub fn sign(self, key: &SecretKey) -> Result<Vec<u8>, RelayError> {
        self.validate()?;
        let (magic, flags) = match self.pacing {
            CertificatePacing::Legacy => (CERTIFICATE_MAGIC, None),
            CertificatePacing::On => (CERTIFICATE_V2_MAGIC, Some(0)),
            CertificatePacing::Off => (CERTIFICATE_V2_MAGIC, Some(CERTIFICATE_PACING_OFF)),
        };
        let mut bytes = Vec::with_capacity(ORDERING_CERTIFICATE_V2_LEN);
        bytes.extend_from_slice(magic);
        bytes.extend_from_slice(&self.room);
        bytes.extend_from_slice(&self.sources_mask.to_le_bytes());
        bytes.extend_from_slice(&self.tick_hz.to_le_bytes());
        bytes.extend_from_slice(&self.expires.to_le_bytes());
        bytes.extend(flags);
        let tag = key
            .mac(Self::domain(self.pacing), &bytes)
            .finalize()
            .into_bytes();
        bytes.extend_from_slice(&tag);
        Ok(bytes)
    }

    fn verify(bytes: &[u8], key: &SecretKey, now: u64) -> Result<Self, RelayError> {
        let certificate = Self::parse(bytes)?;
        let signed = bytes.len() - 32;
        key.mac(Self::domain(certificate.pacing), &bytes[..signed])
            .verify_slice(&bytes[signed..])
            .map_err(|_| RelayError::Authentication)?;
        if certificate.expires <= now || certificate.expires > now.saturating_add(MAX_MATCH_SECONDS)
        {
            return Err(RelayError::Expired);
        }
        Ok(certificate)
    }
}

/// A client's pace against the sealed frames, reported about once a second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum OrderPace {
    /// It runs the frames as they are sealed.
    KeepingUp = 0,
    /// Frames wait on it, and it runs fewer than are sealed.
    Behind = 1,
    /// Frames wait on it, but it runs at least as many as are sealed.
    CatchingUp = 2,
}

/// The rates a match certified at `tick_hz` may be sealed at, fastest first:
/// its own and at most two Game Speed rows slower. A player whose PC cannot
/// keep up slows the match this far and no further.
pub fn paced_rates(tick_hz: u16) -> &'static [u16] {
    let fastest = FRAME_RATES
        .iter()
        .position(|&rate| rate == tick_hz)
        .unwrap_or(FRAME_RATES.len() - 1);
    &FRAME_RATES[fastest..(fastest + 3).min(FRAME_RATES.len())]
}

/// Frames the relay seals at once at `rate`, about fifteen turns a second:
/// 4 at 60 Hz, 3 at 45, 2 at 30 and 20, 1 at 15 and below. A turn's frames
/// reach the players together when its last frame is due.
pub fn turn_frames(rate: u16) -> u32 {
    u32::from(rate).div_ceil(15).max(1)
}

/// A turn's duration `turn_frames(rate) * 1000 / rate` in whole
/// milliseconds, rounded up (spec §2): 67 at 30 Hz and at 45 Hz, 100 at
/// 20 Hz. `ClientSession::set_ordering_turn_ms` takes this value.
pub fn turn_ms(rate: u16) -> u32 {
    (turn_frames(rate) * 1000).div_ceil(u32::from(rate.max(1)))
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
    /// The relay's hop feedback appended to every STATUS (spec §3.4).
    pub feedback: HopFeedback,
}

#[derive(Debug)]
pub enum OrderEntry {
    /// A barrier after `frame` is complete, not another simulation frame.
    Pause {
        sequence: u64,
        frame: u32,
        epoch: u64,
        paused: bool,
        source: u8,
    },
    Batch {
        sequence: u64,
        frame: u32,
        source: u8,
        batch_id: u64,
        /// The plaintext event packet body (compaction spec §3.1, §4). A
        /// body that does not decode is the source's unreadable batch; every
        /// member reads the same bytes, so all of them find it unreadable.
        bytes: Vec<u8>,
    },
    FrameEnd {
        sequence: u64,
        frame: u32,
    },
    Departure {
        sequence: u64,
        frame: u32,
        source: u8,
        /// The relay ended a silent source; otherwise it left by Leave or the
        /// match lifetime. On the wire, a 15th byte 1 is DestroyConnection's
        /// connection-lost error code.
        lost: bool,
    },
    /// A barrier after `frame`: later frames are sealed `frames_per_second`
    /// apart, one of `paced_rates` of the certificate's rate. `source` is the
    /// player who could not keep up; `None` steps back up.
    Rate {
        sequence: u64,
        frame: u32,
        frames_per_second: u16,
        source: Option<u8>,
    },
    /// A barrier after `frame`: players' checksums of `sample_frame`
    /// differed. `outlier` reported a value the majority did not (of two,
    /// the later reporter); `other` is a source it differed from.
    Desync {
        sequence: u64,
        frame: u32,
        sample_frame: u32,
        outlier: u8,
        outlier_checksum: u32,
        other: u8,
        other_checksum: u32,
    },
}

#[derive(Debug)]
pub enum OrderEvent {
    /// Control acknowledgement only. The canonical Pause entry owns admission.
    PauseStatus {
        supported_sources: u32,
        active_sources: u32,
        request_id: u64,
        epoch: u64,
        paused: bool,
        frame: u32,
    },
    Ready {
        ready_mask: u32,
        sources_mask: u32,
    },
    Status(OrderStatus),
    /// ACK (event 2, spec §3.3): the relay accepted this source's batches
    /// through `batch_id` (`accepted_through`, cumulative), the last of them
    /// into `frame`.
    Ack {
        batch_id: u64,
        frame: u32,
        feedback: HopFeedback,
    },
    Entry(OrderEntry),
    Error {
        code: OrderError,
        expected: u64,
    },
    /// The exact terminal Leave was accepted, or its expired room is gone.
    Left,
    /// An authenticated control payload this build cannot read: a newer
    /// relay's record, reply or error code. Replies may grow and are read by
    /// their known prefix; a canonical entry must match its layout exactly,
    /// because members that read different bytes would simulate differently.
    Unrecognized,
}

#[derive(Clone)]
pub(super) struct SealedBatch {
    pub(super) id: u64,
    pub(super) opaque: Vec<u8>,
}

#[derive(Clone)]
pub(super) struct ClientOrdering {
    certificate: Vec<u8>,
    agreement: [u8; 32],
    // The four most recently sealed batches retain their exact bytes.
    // Accepted entries stay cached until a new batch needs the slot, so a
    // delayed app-side acceptance receipt can still retry immutable bytes.
    pub(super) sealed: VecDeque<SealedBatch>,
    pub(super) next_batch: u64,
    pub(super) accepted_through: u64,
    // This process replaces one that left the match: batches it finds of its
    // own source are that one's, and they set the counter instead of being
    // receipts of what this process sent.
    pub(super) restoring: bool,
    pub(super) leave: Option<(u64, Vec<u8>)>,
    // Path measurement carried across hop renewals (spec §4.7).
    pub(super) link: crate::link::ClientLink,
}

impl ClientOrdering {
    fn accept_through(&mut self, id: u64) -> Result<(), RelayError> {
        if self.restoring {
            self.next_batch = self.next_batch.max(id.saturating_add(1));
        }
        if id >= self.next_batch {
            return Err(RelayError::Authentication);
        }
        if id > self.accepted_through {
            // Any acknowledgement progress ends the RTO backoff (spec §4.5).
            self.accepted_through = id;
            self.link.rto_backoff = 0;
        }
        Ok(())
    }
}

impl ClientSession {
    /// Bounded client pipeline, below the existing relay's eight pending
    /// batches per source. This changes neither its wire format nor admission.
    pub const ORDERING_SUBMISSION_WINDOW: usize = 4;

    /// Move the current stream cache at hop-lease promotion, after the old
    /// route's last receive/submit. Candidate creation alone is only a snapshot.
    ///
    /// Also moves the path measurement (spec §4.7): `loss‰`, `srtt`, `base`
    /// and the uplink state carry over; this hop's `down` window, send log
    /// and loss cursor start empty. The next `poll_uplink` on this session
    /// sends an UPLINK at once, with PUSH and every unacknowledged batch.
    pub fn adopt_ordering_from(&mut self, previous: &mut Self) -> Result<(), RelayError> {
        if self.ticket.room != previous.ticket.room
            || self.ticket.source != previous.ticket.source
            || self.ticket.player != previous.ticket.player
            || self.ticket.generation != previous.ticket.generation
        {
            return Err(RelayError::Authentication);
        }
        self.ordering = previous.ordering.take();
        let previous_received = previous.hop_received();
        if let Some(ordering) = self.ordering.as_mut() {
            ordering.link.earlier_hops_received += previous_received;
        }
        Ok(())
    }

    /// While set, this source's batches in the stream belong to the process
    /// this one replaces. The stream must have been read past its last one
    /// before this is cleared and a batch submitted.
    ///
    /// Also the UPLINK RESTORING flag (flags bit3, spec §3.2, §11 row 9):
    /// while set the relay keeps the subscription but pushes no new records,
    /// and the caller reads history with `pull_ordering_at`. Clearing it lets
    /// the relay push again from the acknowledged watermark.
    pub fn set_ordering_restore(&mut self, restoring: bool) {
        if let Some(ordering) = self.ordering.as_mut() {
            ordering.restoring = restoring;
        }
    }

    pub(super) fn configure_ordering(
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
        let certificate = certificate.to_vec();
        if let Some(current) = &mut self.ordering {
            let old = OrderingCertificate::parse(&current.certificate)?;
            if old.room != parsed.room
                || old.sources_mask != parsed.sources_mask
                || old.tick_hz != parsed.tick_hz
                || old.pacing != parsed.pacing
                || current.agreement != agreement
            {
                return Err(RelayError::Authentication);
            }
            if parsed.expires > old.expires {
                current.certificate = certificate.clone();
            }
        } else {
            self.ordering = Some(ClientOrdering {
                certificate: certificate.clone(),
                agreement,
                sealed: VecDeque::new(),
                next_batch: 1,
                accepted_through: 0,
                restoring: false,
                leave: None,
                link: crate::link::ClientLink::new(parsed.tick_hz),
            });
            #[cfg(any(test, feature = "test-support"))]
            if self.measured_good {
                self.preset_uplink_measured_good();
            }
        }
        let mut payload = vec![READY_REQUEST];
        payload.extend_from_slice(&certificate);
        payload.extend_from_slice(&agreement);
        self.encode(CONTROL, &payload, out)
    }

    /// Seals `batch_id` (the next ID) or checks a retained one has the same
    /// plaintext, stable across retries and hop renewals.
    pub(super) fn seal_batch(&mut self, batch_id: u64, plaintext: &[u8]) -> Result<(), RelayError> {
        if !self.ready {
            return Err(RelayError::Unregistered);
        }
        if plaintext.is_empty() || plaintext.len() > ORDERING_BATCH_CAPACITY || batch_id == 0 {
            return Err(RelayError::Malformed);
        }
        let ordering = self.ordering.as_mut().ok_or(RelayError::Unregistered)?;
        if let Some(batch) = ordering.sealed.iter().find(|batch| batch.id == batch_id) {
            if batch.opaque != plaintext {
                return Err(RelayError::Authentication);
            }
            return Ok(());
        }
        if batch_id != ordering.next_batch {
            return Err(RelayError::Replay);
        }
        if batch_id - ordering.accepted_through - 1 >= Self::ORDERING_SUBMISSION_WINDOW as u64 {
            return Err(RelayError::Full);
        }
        let next_batch = batch_id
            .checked_add(1)
            .ok_or(RelayError::CounterExhausted)?;
        // Plaintext `P` (compaction spec §3.1).
        let opaque = plaintext.to_vec();
        if ordering.sealed.len() == Self::ORDERING_SUBMISSION_WINDOW {
            let oldest = ordering.sealed.front().expect("full sealed batch cache");
            if oldest.id > ordering.accepted_through {
                return Err(RelayError::Full);
            }
            ordering.sealed.pop_front();
        }
        ordering.sealed.push_back(SealedBatch {
            id: batch_id,
            opaque,
        });
        ordering.next_batch = next_batch;
        Ok(())
    }

    pub(super) fn pull_ordering(
        &mut self,
        next_seq: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        if !self.ready || self.ordering.is_none() {
            return Err(RelayError::Unregistered);
        }
        // `[2][next varint]` (compaction spec §7.4).
        let mut payload = vec![PULL_REQUEST];
        varint::write(next_seq, &mut payload);
        self.encode(CONTROL, &payload, out)?;
        // Its reply packs are no new-turn arrivals (spec §4.8).
        self.ordering.as_mut().expect("ordering").link.pull_pending = true;
        Ok(())
    }

    /// Request zero advertises pause support and asks for current status.
    /// Positive IDs are monotonic per source for this match, including across
    /// hop renewal. A retry keeps the exact ID, expected epoch and desired state.
    pub(super) fn control_ordering_pause(
        &mut self,
        request_id: u64,
        expected_epoch: u64,
        paused: bool,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        if !self.ready || self.ordering.is_none() {
            return Err(RelayError::Unregistered);
        }
        // `[5][request_id varint][expected_epoch varint][paused u8]`
        // (compaction spec §7.4).
        let mut payload = vec![PAUSE_REQUEST];
        varint::write(request_id, &mut payload);
        varint::write(expected_epoch, &mut payload);
        payload.push(u8::from(paused));
        self.encode(CONTROL, &payload, out)
    }

    pub(super) fn leave_ordering(&mut self, out: &mut Vec<u8>) -> Result<(), RelayError> {
        let Some(ordering) = self.ordering.as_ref() else {
            return Err(RelayError::Unregistered);
        };
        if let Some((_, bytes)) = &ordering.leave {
            out.clone_from(bytes);
            return Ok(());
        }
        // Only this irreversible capability can be sent after readiness or
        // ticket expiry. Cache exact bytes for idempotent terminal retries.
        self.encode(CONTROL, &[LEAVE_REQUEST], out)?;
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
        // Replies are read by their known prefix (compaction spec §7 for
        // their varint fields).
        let mut reader = Reader(&payload[1..]);
        match payload[0] {
            PAUSE_STATUS_EVENT => {
                let read = |reader: &mut Reader| -> Result<OrderEvent, RelayError> {
                    let supported_sources = reader.u32()?;
                    let active_sources = reader.u32()?;
                    let request_id = reader.varint()?;
                    let epoch = reader.varint()?;
                    let paused = reader.u8()?;
                    let frame = reader.u32()?;
                    if paused > 1 {
                        return Err(RelayError::Malformed);
                    }
                    Ok(OrderEvent::PauseStatus {
                        supported_sources,
                        active_sources,
                        request_id,
                        epoch,
                        paused: paused != 0,
                        frame,
                    })
                };
                return Ok(read(&mut reader).unwrap_or(OrderEvent::Unrecognized));
            }
            STATUS_EVENT => {
                let read = |reader: &mut Reader| -> Result<OrderStatus, RelayError> {
                    Ok(OrderStatus {
                        head_frame: reader.varint_u32()?,
                        oldest_seq: reader.varint()?,
                        next_seq: reader.varint()?,
                        active_sources: reader.u32()?,
                        tick_hz: reader.u16()?,
                        start_delay_ms: reader.varint_u32()?,
                        started: reader.u8()? != 0,
                        feedback: crate::link::read_feedback(reader)?,
                    })
                };
                return Ok(read(&mut reader).map_or(OrderEvent::Unrecognized, OrderEvent::Status));
            }
            ACK_EVENT => {
                let Ok((batch_id, frame, feedback)) = (|| -> Result<_, RelayError> {
                    Ok((
                        reader.varint()?,
                        reader.varint_u32()?,
                        crate::link::read_feedback(&mut reader)?,
                    ))
                })() else {
                    return Ok(OrderEvent::Unrecognized);
                };
                if batch_id == 0 {
                    return Err(RelayError::Malformed);
                }
                if let Some(ordering) = self.ordering.as_mut() {
                    // The unchanged relay only accepts consecutive IDs, so
                    // acceptance of this batch also acknowledges its prefix.
                    ordering.accept_through(batch_id)?;
                }
                return Ok(OrderEvent::Ack {
                    batch_id,
                    frame,
                    feedback,
                });
            }
            ERROR_EVENT => {
                let Ok((code, expected)) =
                    (|| -> Result<_, RelayError> { Ok((reader.u8()?, reader.varint()?)) })()
                else {
                    return Ok(OrderEvent::Unrecognized);
                };
                let Ok(code) = OrderError::parse(code) else {
                    return Ok(OrderEvent::Unrecognized);
                };
                if code == OrderError::BatchGap {
                    let accepted = expected.checked_sub(1).ok_or(RelayError::Malformed)?;
                    if let Some(ordering) = self.ordering.as_mut() {
                        ordering.accept_through(accepted)?;
                    }
                }
                return Ok(OrderEvent::Error { code, expected });
            }
            _ => {}
        }
        Ok(match (payload[0], payload.len()) {
            (PAUSE_EVENT, PAUSE_ENTRY_BYTES) if payload[21] <= 1 && payload[22] < MAX_PEERS => {
                OrderEvent::Entry(OrderEntry::Pause {
                    sequence: read_u64(payload, 1),
                    frame: read_u32(payload, 9),
                    epoch: read_u64(payload, 13),
                    paused: payload[21] != 0,
                    source: payload[22],
                })
            }
            (RATE_EVENT, RATE_ENTRY_BYTES)
                if FRAME_RATES.contains(&u16::from_le_bytes([payload[13], payload[14]]))
                    && (payload[15] < MAX_PEERS || payload[15] == u8::MAX) =>
            {
                OrderEvent::Entry(OrderEntry::Rate {
                    sequence: read_u64(payload, 1),
                    frame: read_u32(payload, 9),
                    frames_per_second: u16::from_le_bytes([payload[13], payload[14]]),
                    source: (payload[15] != u8::MAX).then_some(payload[15]),
                })
            }
            (DESYNC_EVENT, DESYNC_ENTRY_BYTES)
                if payload[17] < MAX_PEERS
                    && payload[22] < MAX_PEERS
                    && payload[17] != payload[22] =>
            {
                OrderEvent::Entry(OrderEntry::Desync {
                    sequence: read_u64(payload, 1),
                    frame: read_u32(payload, 9),
                    sample_frame: read_u32(payload, 13),
                    outlier: payload[17],
                    outlier_checksum: read_u32(payload, 18),
                    other: payload[22],
                    other_checksum: read_u32(payload, 23),
                })
            }
            (READY_EVENT, 9..) => OrderEvent::Ready {
                ready_mask: read_u32(payload, 1),
                sources_mask: read_u32(payload, 5),
            },
            (FRAME_END_EVENT, 13) => OrderEvent::Entry(OrderEntry::FrameEnd {
                sequence: read_u64(payload, 1),
                frame: read_u32(payload, 9),
            }),
            (DEPARTURE_EVENT, length @ (14 | 15))
                if payload[13] < MAX_PEERS && (length == 14 || payload[14] == 1) =>
            {
                OrderEvent::Entry(OrderEntry::Departure {
                    sequence: read_u64(payload, 1),
                    frame: read_u32(payload, 9),
                    source: payload[13],
                    lost: length == 15,
                })
            }
            // `[3][seq u64][frame u32][source u8][batch_id u64][P]`, `P`
            // plaintext of 1..=512 bytes (compaction spec §3.1).
            (BATCH_EVENT, length)
                if length > 22
                    && length <= 22 + ORDERING_BATCH_CAPACITY
                    && payload[13] < MAX_PEERS =>
            {
                let source = payload[13];
                let batch_id = read_u64(payload, 14);
                let opaque = &payload[22..];
                // The canonical batch is also its acceptance receipt. A lost
                // ACK must not block the next local submission after replay.
                if source == self.ticket.source
                    && let Some(ordering) = self.ordering.as_mut()
                {
                    if ordering
                        .sealed
                        .iter()
                        .find(|batch| batch.id == batch_id)
                        .is_some_and(|batch| batch.opaque != opaque)
                    {
                        return Err(RelayError::Authentication);
                    }
                    ordering.accept_through(batch_id)?;
                }
                OrderEvent::Entry(OrderEntry::Batch {
                    sequence: read_u64(payload, 1),
                    frame: read_u32(payload, 9),
                    source,
                    batch_id,
                    bytes: opaque.to_vec(),
                })
            }
            _ => OrderEvent::Unrecognized,
        })
    }

    /// A PACK reply: each entry read as if it had come alone.
    pub(super) fn receive_ordering_pack(
        &mut self,
        payload: &[u8],
    ) -> Result<Vec<OrderEvent>, RelayError> {
        pack::decode_pack_v2(payload)?
            .iter()
            .map(|entry| self.receive_ordering(entry))
            .collect()
    }
}

/// The outlier and a source it differed from, once one sampled frame's
/// reports tell: the first reporter off a strict majority of the active and
/// reporting sources, or once every active source reported without such a
/// majority (two players), the first reporter off the first value and the
/// first reporter.
fn desync_outlier(reports: &[(u8, u32)], active: u32) -> Option<((u8, u32), (u8, u32))> {
    let first = reports[0];
    let differing = *reports.iter().find(|report| report.1 != first.1)?;
    let reported = reports
        .iter()
        .fold(0u32, |mask, &(source, _)| mask | 1 << source);
    let voters = (active | reported).count_ones() as usize;
    let holders = |value: u32| reports.iter().filter(|report| report.1 == value).count();
    if let Some(&majority) = reports.iter().find(|report| 2 * holders(report.1) > voters) {
        let outlier = *reports
            .iter()
            .find(|report| report.1 != majority.1)
            .expect("a differing report");
        return Some((outlier, majority));
    }
    (active & !reported == 0).then_some((differing, first))
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}
/// `[6][code u8][expected varint]` (compaction spec §7.4).
fn error(code: OrderError, expected: u64) -> Vec<Vec<u8>> {
    let mut payload = vec![ERROR_EVENT, code as u8];
    varint::write(expected, &mut payload);
    vec![payload]
}
/// ACK before its feedback tail: `[2][accepted_through varint][frame
/// varint]` (compaction spec §7.2).
fn ack(batch_id: u64, frame: u32) -> Vec<Vec<u8>> {
    let mut payload = vec![ACK_EVENT];
    varint::write(batch_id, &mut payload);
    varint::write(frame.into(), &mut payload);
    vec![payload]
}

/// Per authenticated hop delivery credit, derived from the immutable stream.
/// No packet bodies or simulation progress are retained here.
pub(super) struct PushSubscription {
    acknowledged_next: u64,
    sent_next: u64,
    // Entries before this were pushed twice or acknowledged.
    repeat_next: u64,
    // When the oldest entry still owed a second push was first sent.
    unrepeated_since: Option<u64>,
    refreshed_at: u64,
    status_at: u64,
    status_state: (bool, u32),
    // UPLINK RESTORING: keep the subscription, push no records (spec §7.1).
    restoring: bool,
    // Record ranges `[from, to)` owed again (spec §7.3), sorted, disjoint.
    retransmit: VecDeque<(u64, u64)>,
    // Ranges retransmitted recently, with when, for once-per-srtt.
    retransmitted: VecDeque<(u64, u64, u64)>,
    pto_backoff: u32,
    last_ack_progress_ms: u64,
    // The stream end last counted as missed while offline (spec §10.2).
    offline_seen_next: u64,
}

impl PushSubscription {
    /// A subscription from `cursor`, created by an UPLINK with PUSH. Its
    /// first status heartbeat is due 250 ms on.
    fn new(cursor: u64, restoring: bool, status_state: (bool, u32), now_ms: u64) -> Self {
        Self {
            acknowledged_next: cursor,
            sent_next: cursor,
            repeat_next: cursor,
            unrepeated_since: None,
            refreshed_at: now_ms,
            status_at: now_ms,
            status_state,
            restoring,
            retransmit: VecDeque::new(),
            retransmitted: VecDeque::new(),
            pto_backoff: 0,
            last_ack_progress_ms: now_ms,
            offline_seen_next: 0,
        }
    }

    /// This subscription's part of `RelayServer::reader_link_stats`.
    pub(super) fn fill_stats(&self, stats: &mut crate::ReaderLinkStats) {
        stats.acked_next = self.acknowledged_next;
        stats.sent_next = self.sent_next;
        stats.retransmit_ranges = self.retransmit.len();
        stats.pto_backoff = self.pto_backoff;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct PauseRequest {
    id: u64,
    expected_epoch: u64,
    paused: bool,
}

/// A player's latest pace report, with the frames it ran in its last second.
#[derive(Clone, Copy)]
struct PaceReport {
    pace: OrderPace,
    frames_per_second: u16,
    // The reporter's 90th-percentile tick cost; legacy reports carry none.
    tick_micros: Option<u32>,
    at_ms: u64,
}

impl PaceReport {
    /// The fastest rate this reporter can hold while it keeps
    /// `100 - SUSTAIN_PERCENT` percent of each second for everything else
    /// (drawing, catching up); legacy reports: nine tenths of what it ran,
    /// which overstates it during catch-up.
    fn sustainable_rate(&self) -> u32 {
        match self.tick_micros {
            Some(micros) => (1_000_000 * SUSTAIN_PERCENT / 100) / micros.max(1),
            None => u32::from(self.frames_per_second) * 9 / 10,
        }
    }
}

#[derive(Default)]
struct Source {
    principal: Option<([u8; 16], u64)>,
    last_batch: Option<(u64, u32, Vec<u8>)>,
    pending: usize,
    leave_packets: RateBudget,
    pause_supported: bool,
    pause_request: Option<PauseRequest>,
    // Also its capability: a source that reported reads rate markers.
    pace: Option<PaceReport>,
    // Last authenticated packet from this source, keepalives included.
    heard_ms: u64,
}

/// The sealing clock's slowdown for players whose PCs cannot keep up. This
/// is a relay adaptation of native lockstep, where every match runs as slow
/// as its slowest player (ProcessNetwork 0x00647BF4..0x00647E6D, TIMING):
/// here it slows at most two Game Speed rows and tries to speed up again.
struct Pace {
    rate: u16,
    // The head at the last rate change, and when. Pauses displace these
    // times like the start.
    anchor: Option<(u32, u64)>,
    raised_at: Option<u64>,
    raise_wait: u64,
    // When a player last reported being behind or catching up.
    lagging_at: u64,
    // Slowdowns and raises the server has not yet counted for its journal
    // report.
    slowdowns: u32,
    raises: u32,
    // Federated: the frame from which the last rate change applies and its
    // due time as an offset from the start, so the start's slew moves it too.
    federated_anchor: Option<(u32, u64)>,
    // Federated: the coordinator's rate changes for frames after these,
    // `(frame, rate, source)`, in order, each until its due time passes. A
    // healed link can deliver several at once.
    scheduled: VecDeque<(u32, u16, u8)>,
}

impl Pace {
    fn displace(&mut self, paused_for: u64) {
        if let Some((_, at)) = self.anchor.as_mut() {
            *at = at.saturating_add(paused_for);
        }
        if let Some(at) = self.raised_at.as_mut() {
            *at = at.saturating_add(paused_for);
        }
        self.lagging_at = self.lagging_at.saturating_add(paused_for);
    }

    /// Move the rate changes not yet counted into the journal report's counts.
    fn count_changes(&mut self, counts: &mut crate::OrderingCounts) {
        counts.slowdowns += u64::from(std::mem::take(&mut self.slowdowns));
        counts.raises += u64::from(std::mem::take(&mut self.raises));
    }
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
    paused_at: Option<u64>,
    resumed_at: Option<u64>,
    pause_epoch: u64,
    pace: Pace,
    // Merged stream head. A federated relay seals its own sources first, at
    // `sealed`, and merges a frame once every relay has sealed it.
    head: u32,
    sealed: u32,
    // Sources homed on this relay: all of them unless federated.
    local: u32,
    // A federated Leave before the start asks the coordinator to cancel.
    cancel_requested: bool,
    next_seq: u64,
    history: VecDeque<Stored>,
    history_bytes: usize,
    pending: BTreeMap<u32, Vec<Pending>>,
    journal: Option<crate::journal::Journal>,
    storage_failed: bool,
    // When `advance` first found the storage failure.
    failed_at: Option<u64>,
    finished_at: Option<u64>,
    created_ms: u64,
    // Players' checksums by sampled frame, `(source, checksum)` in arrival
    // order, for the newest `CHECKSUM_FRAMES` sampled frames.
    checksums: BTreeMap<u32, Vec<(u8, u32)>>,
    // A DESYNC record was emitted or is on its way; nothing is compared again.
    desync_found: bool,
    // A federated follower's own sources' reports, `(source, frame,
    // checksum)`, until its next link pass forwards them to the coordinator.
    pub(super) forward_checksums: Vec<(u8, u32, u32)>,
    // DESYNC records the server has not yet counted for its journal report.
    desyncs: u32,
}

impl OrderingRoom {
    fn new(
        certificate: OrderingCertificate,
        agreement: [u8; 32],
        journal: Option<crate::journal::Journal>,
        local: u32,
        now_ms: u64,
    ) -> Self {
        Self {
            certificate,
            agreement,
            sources: std::array::from_fn(|_| Source {
                heard_ms: now_ms,
                ..Source::default()
            }),
            ready: 0,
            active: certificate.sources_mask,
            starts_at: None,
            paused_at: None,
            resumed_at: None,
            pause_epoch: 0,
            pace: Pace {
                rate: certificate.tick_hz,
                anchor: None,
                raised_at: None,
                raise_wait: PACE_RAISE_MS,
                lagging_at: 0,
                slowdowns: 0,
                raises: 0,
                federated_anchor: None,
                scheduled: VecDeque::new(),
            },
            head: 0,
            sealed: 0,
            local,
            cancel_requested: false,
            next_seq: 1,
            history: VecDeque::new(),
            history_bytes: 0,
            pending: BTreeMap::new(),
            journal,
            storage_failed: false,
            failed_at: None,
            finished_at: None,
            created_ms: now_ms,
            checksums: BTreeMap::new(),
            desync_found: false,
            forward_checksums: Vec::new(),
            desyncs: 0,
        }
    }

    /// Move the rate changes and desyncs not yet counted into the journal
    /// report's counts.
    pub(super) fn count_changes(&mut self, counts: &mut crate::OrderingCounts) {
        self.pace.count_changes(counts);
        counts.desyncs += u64::from(std::mem::take(&mut self.desyncs));
    }

    /// Any authenticated packet keeps a source's place; a newer admission of
    /// another principal for the same source does not.
    pub(super) fn heard(&mut self, source: u8, principal: ([u8; 16], u64), now_ms: u64) {
        if let Some(state) = self.sources.get_mut(usize::from(source))
            && state.principal.is_none_or(|bound| bound == principal)
        {
            state.heard_ms = now_ms;
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
    /// STATUS before its feedback tail: `[1][head_frame varint][oldest_seq
    /// varint][next_seq varint][active_sources u32][tick_hz u16]
    /// [start_delay_ms varint][started u8]` (compaction spec §7.3).
    fn status(&self, now_ms: u64) -> Vec<u8> {
        let mut payload = vec![STATUS_EVENT];
        varint::write(self.head.into(), &mut payload);
        varint::write(self.oldest_seq(), &mut payload);
        varint::write(self.next_seq, &mut payload);
        payload.extend_from_slice(&self.active.to_le_bytes());
        payload.extend_from_slice(&self.certificate.tick_hz.to_le_bytes());
        varint::write(
            self.starts_at.map_or(0, |start| {
                start.saturating_sub(now_ms).min(u64::from(u32::MAX))
            }),
            &mut payload,
        );
        payload.push(u8::from(
            self.starts_at.is_some_and(|start| start <= now_ms),
        ));
        payload
    }

    pub(super) fn expired(&self, now: u64) -> bool {
        self.certificate.expires <= now
    }

    fn cleanup_due(&self, now_ms: u64, now: u64, peers: bool) -> bool {
        if self.storage_failed {
            // A storage failure freezes the match for good. Its room goes
            // once no live ticket is left to tell, or once a silent player
            // would have lost its place; its files stay.
            return !peers
                || self
                    .failed_at
                    .is_some_and(|at| now_ms.saturating_sub(at) >= ABSENT_MS);
        }
        // The certificate bounds only a startup; once started, the final
        // frame, silence or the room's lifetime ends it.
        (self.starts_at.is_none() && self.expired(now))
            || now_ms.saturating_sub(self.created_ms) >= ROOM_LIFETIME_MS
            || self
                .finished_at
                .is_some_and(|at| now_ms.saturating_sub(at) >= MAX_TICKET_LIFETIME * 1000)
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

    fn append_records(&mut self, frame: u32, records: Vec<Vec<u8>>) -> bool {
        if let Some(journal) = self.journal.as_mut()
            && journal.commit(&records).is_err()
        {
            self.storage_failed = true;
            return false;
        }
        for record in records {
            self.cache_entry(frame, record);
        }
        true
    }

    fn append_pause(&mut self, paused: bool, source: u8, now_ms: u64) -> Result<(), RelayError> {
        let epoch = self
            .pause_epoch
            .checked_add(1)
            .ok_or(RelayError::CounterExhausted)?;
        let mut tail = epoch.to_le_bytes().to_vec();
        tail.extend([u8::from(paused), source]);
        let record = Self::encoded_entry(self.next_seq, self.head, PAUSE_EVENT, &tail);
        if !self.append_records(self.head, vec![record]) {
            return Ok(());
        }
        self.pause_epoch = epoch;
        if paused {
            self.paused_at = Some(now_ms);
        } else if let Some(paused_at) = self.paused_at.take() {
            self.resumed_at = Some(now_ms);
            // Displace wall time instead of generating catch-up frames for the
            // paused interval. The next frame retains its fractional remainder.
            let paused_for = now_ms.saturating_sub(paused_at);
            if let Some(start) = self.starts_at.as_mut() {
                *start = start.saturating_add(paused_for);
            }
            self.pace.displace(paused_for);
        }
        Ok(())
    }

    /// Stores a player's pace report, which `adjust_pace` acts on, and
    /// returns its pace.
    /// A reader's pace segment (spec §3.2), the latest report wins; a zero
    /// tick cost is none. The decoder checked the pace byte.
    fn report_pace(
        &mut self,
        source: u8,
        report: crate::link::PaceSegment,
        now_ms: u64,
    ) -> OrderPace {
        let pace = match report.pace {
            0 => OrderPace::KeepingUp,
            1 => OrderPace::Behind,
            _ => OrderPace::CatchingUp,
        };
        self.sources[usize::from(source)].pace = Some(PaceReport {
            pace,
            frames_per_second: report.frames_per_second,
            tick_micros: (report.tick_micros != 0).then_some(report.tick_micros),
            at_ms: now_ms,
        });
        pace
    }

    /// Slows the clock while a player reports it cannot keep up: to the
    /// fastest paced rate at most nine tenths of what it ran, at least one
    /// row slower, at most two. Catch-up skips drawing, so what it ran
    /// overstates what it keeps up with, and the margin drains the frames
    /// waiting on it. Once nobody has been behind or catching up for the
    /// raise wait it tries one row faster. Every active source must have
    /// reported, so no client meets a marker it cannot read. Each change is a
    /// marker after the head's frame end; later frames are due at the new
    /// rate from its append time.
    fn adjust_pace(&mut self, now_ms: u64, federated: bool) {
        if self.certificate.pacing == CertificatePacing::Off || !self.pace.scheduled.is_empty() {
            return;
        }
        let rates = paced_rates(self.certificate.tick_hz);
        let step = rates
            .iter()
            .position(|&rate| rate == self.pace.rate)
            .expect("the clock runs at a paced rate");
        // The slowest sustainable rate among Behind reporters that cannot
        // hold the current one, and whether a reporter could not hold the
        // next faster rate.
        let mut slowest: Option<(u32, u16, u8)> = None;
        let mut lagging = false;
        let mut raise_blocked = false;
        for (index, source) in self.sources.iter().enumerate() {
            if self.active & (1 << index) == 0 {
                continue;
            }
            let Some(report) = source.pace else {
                // A VO1C member may be a released client that reads no rate
                // marker; under VO2C every member reads them.
                if self.certificate.pacing == CertificatePacing::Legacy {
                    return;
                }
                continue;
            };
            if now_ms.saturating_sub(report.at_ms) > PACE_REPORT_MS {
                continue;
            }
            let sustainable = report.sustainable_rate();
            // A tick-cost reporter that holds the current rate is only
            // drawing slowly: its own catch-up recovers it, and it neither
            // slows the clock nor holds back a raise.
            let simulation_bound =
                report.tick_micros.is_none() || sustainable < u32::from(self.pace.rate);
            lagging |= report.pace != OrderPace::KeepingUp && simulation_bound;
            // Ties keep the slowest that actually ran, as legacy reports did.
            let slowness = (sustainable, report.frames_per_second);
            if report.pace == OrderPace::Behind
                && simulation_bound
                && slowest.is_none_or(|(rate, ran, _)| slowness < (rate, ran))
            {
                slowest = Some((sustainable, report.frames_per_second, index as u8));
            }
            raise_blocked |= report.tick_micros.is_some()
                && step > 0
                && sustainable < u32::from(rates[step - 1]);
        }
        if lagging {
            self.pace.lagging_at = now_ms;
        }
        let changed_at = self.pace.anchor.map_or(0, |(_, at)| at);
        if self.pace.anchor.is_some() && now_ms.saturating_sub(changed_at) < PACE_STEP_MS {
            return;
        }
        let (rate, source) = match slowest {
            Some((sustainable, _, source)) if step + 1 < rates.len() => (
                rates[step + 1..]
                    .iter()
                    .copied()
                    .find(|&rate| u32::from(rate) <= sustainable)
                    .unwrap_or(rates[rates.len() - 1]),
                source,
            ),
            None if step > 0
                && !raise_blocked
                && now_ms.saturating_sub(self.pace.lagging_at.max(changed_at))
                    >= self.pace.raise_wait =>
            {
                (rates[step - 1], u8::MAX)
            }
            _ => return,
        };
        if let Some(journal) = self.journal.as_mut()
            && !journal.reserve_entries(&[RATE_ENTRY_BYTES])
        {
            return;
        }
        let mut tail = rate.to_le_bytes().to_vec();
        tail.push(source);
        if federated {
            // Every relay must change at the same frame: the entry reaches
            // the others in frame order with the next seal, and the merge
            // waits for it there.
            let frame = self.sealed + 1;
            self.pending.entry(frame).or_default().push(Pending {
                source: CONTROL_SOURCE,
                batch_id: RATE_BATCH,
                opaque: tail,
            });
            self.pace.scheduled.push_back((frame, rate, source));
        } else {
            let record = Self::encoded_entry(self.next_seq, self.head, RATE_EVENT, &tail);
            if !self.append_records(self.head, vec![record]) {
                return;
            }
        }
        let pace = &mut self.pace;
        if source == u8::MAX {
            pace.raised_at = Some(now_ms);
            pace.raises += 1;
        } else {
            pace.raise_wait = if pace
                .raised_at
                .is_some_and(|at| now_ms.saturating_sub(at) < PACE_RAISE_MS)
            {
                (pace.raise_wait * 2).min(PACE_RAISE_MAX_MS)
            } else {
                PACE_RAISE_MS
            };
            pace.slowdowns += 1;
        }
        if !federated {
            pace.rate = rate;
            pace.anchor = Some((self.head, now_ms));
        }
    }

    /// A federated rate change takes effect when its frame falls due on the
    /// old schedule; later frames are due at the new rate from there. The
    /// schedule counts from the start, so every relay derives the same one.
    fn apply_scheduled_rate(&mut self, now_ms: u64, start: u64) {
        while let Some(&(frame, rate, _)) = self.pace.scheduled.front() {
            let (anchor, offset) = self.pace.federated_anchor.unwrap_or((0, 0));
            let due =
                offset + u64::from(frame.saturating_sub(anchor)) * 1000 / u64::from(self.pace.rate);
            if now_ms < start + due {
                return;
            }
            self.pace.rate = rate;
            self.pace.federated_anchor = Some((frame, due));
            self.pace.anchor = Some((frame, start + due));
            self.pace.scheduled.pop_front();
        }
    }

    /// About when `frame` was due: exact from the last rate change on, at
    /// the certificate's rate from the start before it.
    fn frame_due_ms(&self, frame: u32) -> u64 {
        match self.pace.anchor {
            Some((anchor, at)) if frame >= anchor => {
                at.saturating_add(u64::from(frame - anchor) * 1000 / u64::from(self.pace.rate))
            }
            _ => self
                .starts_at
                .unwrap_or(0)
                .saturating_add(u64::from(frame) * 1000 / u64::from(self.certificate.tick_hz)),
        }
    }

    fn supported_pause_sources(&self) -> u32 {
        if self.local != self.certificate.sources_mask {
            return 0;
        }
        self.sources
            .iter()
            .enumerate()
            .filter(|(_, source)| source.pause_supported)
            .fold(0, |mask, (index, _)| mask | (1 << index))
    }

    /// `[8][supported u32][active u32][request_id varint][epoch varint]
    /// [paused u8][frame u32]` (compaction spec §7.4).
    fn pause_status(&self, source: u8) -> Vec<Vec<u8>> {
        let mut payload = vec![PAUSE_STATUS_EVENT];
        payload.extend_from_slice(&self.supported_pause_sources().to_le_bytes());
        payload.extend_from_slice(&self.active.to_le_bytes());
        varint::write(
            self.sources[usize::from(source)]
                .pause_request
                .map_or(0, |request| request.id),
            &mut payload,
        );
        varint::write(self.pause_epoch, &mut payload);
        payload.push(u8::from(self.paused_at.is_some()));
        payload.extend_from_slice(&self.head.to_le_bytes());
        vec![payload]
    }

    fn control_pause(
        &mut self,
        ticket: Ticket,
        payload: &[u8],
        now_ms: u64,
    ) -> Result<Vec<Vec<u8>>, RelayError> {
        // `[5][request_id varint][expected_epoch varint][paused u8]`
        // (compaction spec §7.4).
        let mut reader = Reader(&payload[1..]);
        let (id, expected_epoch, paused) = (reader.varint()?, reader.varint()?, reader.u8()?);
        if !reader.is_empty() || paused > 1 {
            return Err(RelayError::Malformed);
        }
        self.authenticate_reader(ticket)?;
        if self.storage_failed {
            return Ok(error(OrderError::StorageUnavailable, self.next_seq));
        }
        let source = ticket.source;
        if self.active & (1 << source) == 0 {
            return Ok(error(OrderError::Inactive, 0));
        }
        let request = PauseRequest {
            id,
            expected_epoch,
            paused: paused != 0,
        };
        if request.id == 0 {
            self.sources[usize::from(source)].pause_supported = true;
            return Ok(self.pause_status(source));
        }
        let state = &mut self.sources[usize::from(source)];
        if let Some(previous) = state.pause_request {
            if request.id == previous.id && request != previous {
                return Ok(error(OrderError::ModifiedRetry, previous.id));
            }
            if request.id <= previous.id {
                return Ok(self.pause_status(source));
            }
        }
        state.pause_request = Some(request);
        // Capability agreement belongs to the admitted match principals, not
        // short hop tickets. Legacy peers never advertise, so receive no marker.
        if self.supported_pause_sources() & self.active != self.active
            || self.starts_at.is_none_or(|start| start > now_ms)
            || request.expected_epoch != self.pause_epoch
            || request.paused == self.paused_at.is_some()
            || (request.paused
                && self
                    .resumed_at
                    .is_some_and(|at| now_ms.saturating_sub(at) < PAUSE_COOLDOWN_MS))
        {
            return Ok(self.pause_status(source));
        }
        if request.paused
            && let Some(journal) = self.journal.as_mut()
            && !journal.reserve_entries(&[PAUSE_ENTRY_BYTES, PAUSE_ENTRY_BYTES])
        {
            return Ok(error(OrderError::Backpressure, request.id));
        }
        self.append_pause(request.paused, source, now_ms)?;
        if self.storage_failed {
            return Ok(error(OrderError::StorageUnavailable, self.next_seq));
        }
        Ok(self.pause_status(source))
    }

    fn depart(&mut self, source: u8, now_ms: u64) {
        let bit = 1 << source;
        if self.active & bit == 0 {
            return;
        }
        if self.starts_at.is_none() {
            if self.local == self.certificate.sources_mask {
                // An explicit Leave before the start cancels it for every
                // source, including sources which never reached Ready. Keep an
                // inactive short-ticket tombstone.
                self.active = 0;
                self.ready = 0;
                self.finished_at = Some(now_ms);
                return;
            }
            // Only the coordinator ends a federated startup. Its start may
            // already be on the way; this Leave then departs at frame 1.
            self.cancel_requested = true;
        }
        self.retire(source, false, now_ms);
    }

    /// Ends an active source's place through the stream at the first frame
    /// this relay has not sealed: frame 1 before the start, where the others
    /// start without it. A departure's opaque byte 1 marks a place silence
    /// ended, DestroyConnection's connection-lost error code.
    fn retire(&mut self, source: u8, lost: bool, now_ms: u64) {
        self.active &= !(1 << source);
        let final_frame = (MAX_MATCH_SECONDS * u64::from(self.certificate.tick_hz)) as u32;
        self.pending
            .entry((self.sealed + 1).min(final_frame))
            .or_default()
            .push(Pending {
                source,
                batch_id: 0,
                opaque: if lost { vec![1] } else { Vec::new() },
            });
        if self.active == 0 && self.paused_at.is_some() {
            // Both markers were reserved before Pause was accepted. Explicit
            // all-left closes the barrier so the existing departure tail ends.
            if self.append_pause(false, source, now_ms).is_err() {
                self.storage_failed = true;
            }
        }
    }

    /// Starts once every remaining source is ready. A federated start belongs
    /// to the coordinator relay.
    fn start_when_ready(&mut self, now_ms: u64) {
        if self.local == self.certificate.sources_mask
            && self.starts_at.is_none()
            && self.active != 0
            && self.ready == self.active
        {
            self.starts_at = Some(now_ms.saturating_add(START_DELAY_MS));
        }
    }

    /// Silence ends one of this relay's own places only past these limits:
    /// before the start for a source that never became ready, after it for
    /// any source. A federated startup cannot drop a source alone, so a
    /// loading source past its limit asks the coordinator to cancel.
    fn retire_absent(&mut self, now_ms: u64) {
        for source in 0..MAX_PEERS {
            let bit = 1 << source;
            if self.active & self.local & bit == 0 {
                continue;
            }
            let silent = now_ms.saturating_sub(self.sources[usize::from(source)].heard_ms);
            if self.starts_at.is_some() {
                if silent >= ABSENT_MS {
                    self.retire(source, true, now_ms);
                }
            } else if self.ready & bit == 0
                && (silent >= LOADING_SILENCE_MS
                    || now_ms.saturating_sub(self.created_ms) >= LOADING_LIMIT_MS)
            {
                if self.local == self.certificate.sources_mask {
                    self.retire(source, true, now_ms);
                } else {
                    self.cancel_requested = true;
                }
            }
        }
        self.start_when_ready(now_ms);
    }

    /// Ends a federated match of three or more relays whose others went
    /// silent. It seals and merges no further entry, so its archive grows by
    /// at most the frame ends of what it already sealed.
    fn abandon(&mut self, now_ms: u64) {
        self.active = 0;
        self.pending.clear();
        self.finished_at = Some(now_ms);
    }

    /// The coordinator's cancel before the start ends this startup here too.
    fn cancel(&mut self, now_ms: u64) {
        self.active = 0;
        self.ready = 0;
        self.finished_at = Some(now_ms);
    }

    /// The latest pace reports of active sources homed here, encoded for
    /// the federation coordinator.
    pub(super) fn local_paces(&self, local: u32, now_ms: u64) -> Vec<u8> {
        let mut encoded = Vec::new();
        for (index, source) in self.sources.iter().enumerate() {
            let Some(report) = source
                .pace
                .filter(|_| self.active & local & (1 << index) != 0)
            else {
                continue;
            };
            encoded.push(index as u8);
            encoded.push(report.pace as u8);
            encoded.extend_from_slice(&report.frames_per_second.to_le_bytes());
            encoded.extend_from_slice(&report.tick_micros.unwrap_or(0).to_le_bytes());
            let age = now_ms.saturating_sub(report.at_ms).min(u64::from(u32::MAX)) as u32;
            encoded.extend_from_slice(&age.to_le_bytes());
        }
        encoded
    }

    /// Another relay's sealed entry, merged with this relay's own.
    fn accept_remote(&mut self, frame: u32, source: u8, batch_id: u64, opaque: &[u8]) {
        if source == CONTROL_SOURCE && batch_id == DESYNC_BATCH {
            // The coordinator's desync decision (only its link carries one).
            if opaque[4] >= MAX_PEERS || opaque[9] >= MAX_PEERS || opaque[4] == opaque[9] {
                self.storage_failed = true;
                return;
            }
            if let Some(journal) = self.journal.as_mut()
                && !journal.reserve_entries(&[DESYNC_ENTRY_BYTES])
            {
                self.storage_failed = true;
            }
            self.desync_found = true;
        } else if source == CONTROL_SOURCE {
            // The coordinator's rate change.
            let rate = u16::from_le_bytes([opaque[0], opaque[1]]);
            if !paced_rates(self.certificate.tick_hz).contains(&rate) {
                self.storage_failed = true;
                return;
            }
            if let Some(journal) = self.journal.as_mut()
                && !journal.reserve_entries(&[RATE_ENTRY_BYTES])
            {
                self.storage_failed = true;
            }
            self.pace.scheduled.push_back((frame, rate, opaque[2]));
        } else if batch_id == 0 {
            self.active &= !(1 << source);
        } else {
            // Its home relay reserved it there; this journal stores the same
            // merged stream. The batch is already sealed, so a full journal
            // fails storage rather than dropping it.
            if let Some(journal) = self.journal.as_mut()
                && !journal.reserve_batch(22 + opaque.len())
            {
                self.storage_failed = true;
            }
            self.sources[usize::from(source)].pending += 1;
        }
        self.pending.entry(frame).or_default().push(Pending {
            source,
            batch_id,
            opaque: opaque.to_vec(),
        });
    }

    /// A player's checksum report. A federated follower forwards it to the
    /// coordinator, which alone compares.
    fn report_checksums(
        &mut self,
        ticket: Ticket,
        samples: &[(u32, u32)],
        federation: Option<&federation::Federation>,
    ) {
        for &(frame, checksum) in samples {
            match federation {
                Some(federation) if !federation.is_coordinator() => {
                    if self.forward_checksums.len() < federation::FORWARD_CHECKSUMS
                        && self.reportable_checksum_frame(frame)
                    {
                        self.forward_checksums
                            .push((ticket.source, frame, checksum));
                    }
                }
                _ => self.record_checksum(ticket.source, frame, checksum, federation.is_some()),
            }
        }
    }

    /// Whether a player may report `frame`: a sampled frame
    /// ([`CHECKSUM_INTERVAL`]) no later than the one after this relay's
    /// sealed frames. A client reports frame F's checksum once it has run
    /// F - 1, which its own relay sealed, and in a federated match every
    /// relay sealed before any merged it.
    fn reportable_checksum_frame(&self, frame: u32) -> bool {
        frame != 0 && frame % CHECKSUM_INTERVAL == 0 && frame <= self.sealed.saturating_add(1)
    }

    /// Stores one source's checksum of a sampled frame, its first report
    /// only, and emits the match's one DESYNC record once that frame's
    /// values name an outlier: after the head's frame end on a single relay,
    /// or as the coordinator's entry for the next frame it seals, which
    /// every relay emits after that frame's end.
    pub(super) fn record_checksum(
        &mut self,
        source: u8,
        frame: u32,
        checksum: u32,
        federated: bool,
    ) {
        if self.desync_found
            || self.storage_failed
            || self.finished_at.is_some()
            || self.starts_at.is_none()
            || !self.reportable_checksum_frame(frame)
            || self.active & (1 << source) == 0
            || (self.checksums.len() == CHECKSUM_FRAMES
                && self
                    .checksums
                    .first_key_value()
                    .is_some_and(|(&oldest, _)| frame < oldest))
        {
            return;
        }
        let active = self.active;
        let reports = self.checksums.entry(frame).or_default();
        if reports.iter().any(|&(reporter, _)| reporter == source) {
            return;
        }
        // A player who has left names no outlier, or its last reports could
        // end the others' match after it is gone.
        reports.retain(|&(reporter, _)| active & (1 << reporter) != 0);
        reports.push((source, checksum));
        let found = desync_outlier(reports, active);
        while self.checksums.len() > CHECKSUM_FRAMES {
            self.checksums.pop_first();
        }
        let Some(((outlier, outlier_checksum), (other, other_checksum))) = found else {
            return;
        };
        if let Some(journal) = self.journal.as_mut()
            && !journal.reserve_entries(&[DESYNC_ENTRY_BYTES])
        {
            return;
        }
        let mut tail = frame.to_le_bytes().to_vec();
        tail.push(outlier);
        tail.extend_from_slice(&outlier_checksum.to_le_bytes());
        tail.push(other);
        tail.extend_from_slice(&other_checksum.to_le_bytes());
        if federated {
            self.pending
                .entry(self.sealed + 1)
                .or_default()
                .push(Pending {
                    source: CONTROL_SOURCE,
                    batch_id: DESYNC_BATCH,
                    opaque: tail,
                });
        } else {
            let record = Self::encoded_entry(self.next_seq, self.head, DESYNC_EVENT, &tail);
            if !self.append_records(self.head, vec![record]) {
                return;
            }
            self.desyncs += 1;
        }
        self.desync_found = true;
    }

    fn advance(&mut self, now_ms: u64, federation: Option<&mut federation::Federation>) {
        if self.storage_failed {
            self.failed_at.get_or_insert(now_ms);
            return;
        }
        self.retire_absent(now_ms);
        if self.paused_at.is_some() {
            return;
        }
        let Some(start) = self.starts_at else {
            return;
        };
        let federated = federation.is_some();
        let coordinator = federation
            .as_ref()
            .is_none_or(|federation| federation.is_coordinator());
        if federated {
            self.apply_scheduled_rate(now_ms, start);
        }
        let (anchor, anchor_ms) = if federated {
            self.pace
                .federated_anchor
                .map_or((0, start), |(frame, offset)| (frame, start + offset))
        } else {
            self.pace.anchor.unwrap_or((0, start))
        };
        // Frames are sealed a turn at a time, in whole turns from the anchor,
        // so every federated relay seals the same turns. The final frame
        // closes the match whatever its turn.
        let elapsed = now_ms
            .saturating_sub(anchor_ms)
            .saturating_mul(u64::from(self.pace.rate))
            / 1000;
        let turn = u64::from(turn_frames(self.pace.rate));
        let target = (u64::from(anchor) + elapsed / turn * turn)
            .min(MAX_MATCH_SECONDS * u64::from(self.certificate.tick_hz))
            as u32;
        // Frames past a scheduled change wait for its rate.
        let target = self
            .pace
            .scheduled
            .front()
            .map_or(target, |&(frame, ..)| target.min(frame));
        let final_frame = (MAX_MATCH_SECONDS * u64::from(self.certificate.tick_hz)) as u32;
        if target == final_frame && self.sealed < final_frame {
            for source in 0..MAX_PEERS {
                if self.active & self.local & (1 << source) != 0 {
                    self.pending.entry(final_frame).or_default().push(Pending {
                        source,
                        batch_id: 0,
                        opaque: Vec::new(),
                    });
                }
            }
            self.active &= !self.local;
        }
        let target = if self.active == 0 {
            target.min(
                self.pending
                    .last_key_value()
                    .map_or(self.sealed, |(&frame, _)| frame),
            )
        } else {
            target
        };
        // A federated relay contributes its own sources' entries as it seals
        // each frame, and merges only what every relay has sealed.
        let target = if let Some(federation) = federation {
            while self.sealed < target {
                self.sealed += 1;
                for entry in self.pending.get(&self.sealed).into_iter().flatten() {
                    // Only the coordinator contributes a rate change; the
                    // others received theirs from it.
                    if if entry.source == CONTROL_SOURCE {
                        federation.is_coordinator()
                    } else {
                        self.local & (1 << entry.source) != 0
                    } {
                        federation.contribute(
                            self.sealed,
                            entry.source,
                            entry.batch_id,
                            &entry.opaque,
                        );
                    }
                }
            }
            federation.merged_through(self.sealed)
        } else {
            self.sealed = self.sealed.max(target);
            self.sealed
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
            let mut controls = Vec::new();
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
                    if entry.source == CONTROL_SOURCE {
                        let kind = if entry.batch_id == DESYNC_BATCH {
                            DESYNC_EVENT
                        } else {
                            RATE_EVENT
                        };
                        controls.push((kind, entry.opaque.clone()));
                    } else if entry.batch_id == 0 {
                        let mut tail = vec![entry.source];
                        tail.extend_from_slice(&entry.opaque);
                        records.push(Self::encoded_entry(
                            self.next_seq + records.len() as u64,
                            frame,
                            DEPARTURE_EVENT,
                            &tail,
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
            // A federated desync or rate change follows its frame's end on
            // every relay, as a single relay's marker follows the head's.
            let desyncs = controls
                .iter()
                .filter(|(kind, _)| *kind == DESYNC_EVENT)
                .count() as u32;
            for (kind, tail) in controls {
                records.push(Self::encoded_entry(
                    self.next_seq + records.len() as u64,
                    frame,
                    kind,
                    &tail,
                ));
            }
            if !self.append_records(frame, records) {
                return;
            }
            self.desyncs += desyncs;
            if let Some(entries) = self.pending.remove(&frame) {
                for entry in entries {
                    if entry.batch_id != 0 && entry.source != CONTROL_SOURCE {
                        self.sources[usize::from(entry.source)].pending -= 1;
                    }
                }
            }
            self.head = frame;
        }
        // A federated match's coordinator decides its rate from every
        // relay's reports; the others follow its rate entries.
        if coordinator && self.active != 0 && start <= now_ms && self.head < final_frame {
            self.adjust_pace(now_ms, federated);
        }
        // Every source has departed, by Leave, silence past its limit or the
        // agreed full-match lifetime; the archive ends after its replay grace.
        if ((self.active == 0 && self.pending.is_empty())
            || self.head == (MAX_MATCH_SECONDS * u64::from(self.certificate.tick_hz)) as u32)
            && self.finished_at.is_none()
        {
            self.finished_at = Some(now_ms);
        }
    }

    fn authenticate_reader(&self, ticket: Ticket) -> Result<(), RelayError> {
        let principal = (ticket.player, ticket.generation);
        if self.certificate.sources_mask & (1 << ticket.source) == 0
            || self.sources[usize::from(ticket.source)].principal != Some(principal)
        {
            return Err(RelayError::Authentication);
        }
        Ok(())
    }

    fn cursor_error(&self, cursor: u64) -> Option<(OrderError, u64)> {
        if self.storage_failed {
            Some((OrderError::StorageUnavailable, self.next_seq))
        } else if cursor < self.oldest_seq() {
            Some((OrderError::HistoryGap, self.oldest_seq()))
        } else if cursor > self.next_seq {
            Some((OrderError::InvalidCursor, self.next_seq))
        } else {
            None
        }
    }

    /// Pull and push read the same committed bytes; neither drives the clock.
    fn history_page(
        &mut self,
        cursor: u64,
        limit: usize,
    ) -> Result<Vec<Vec<u8>>, (OrderError, u64)> {
        if let Some(error) = self.cursor_error(cursor) {
            return Err(error);
        }
        if cursor < self.hot_oldest_seq() {
            let journal = self.journal.as_mut().expect("validated archive cursor");
            journal.read(cursor, limit).map_err(|_| {
                self.storage_failed = true;
                (OrderError::StorageUnavailable, self.next_seq)
            })
        } else {
            Ok(self.hot_page(cursor, limit))
        }
    }

    /// Test support: every committed record from the oldest kept, the
    /// journal's then RAM's (compaction spec §12.2).
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn full_history(&mut self) -> Vec<Vec<u8>> {
        let mut records = Vec::new();
        let mut cursor = self.oldest_seq();
        while cursor < self.next_seq {
            let Ok(page) = self.history_page(cursor, ORDERING_PULL_LIMIT) else {
                break;
            };
            if page.is_empty() {
                break;
            }
            cursor += page.len() as u64;
            records.extend(page);
        }
        records
    }

    /// Committed bytes from `cursor` on, which must still be in RAM.
    fn hot_page(&self, cursor: u64, limit: usize) -> Vec<Vec<u8>> {
        self.history
            .iter()
            .skip((cursor - self.hot_oldest_seq()) as usize)
            .take(limit)
            .map(|entry| entry.bytes.clone())
            .collect()
    }

    fn push_status_state(&self, now_ms: u64) -> (bool, u32) {
        (
            self.starts_at.is_some_and(|start| start <= now_ms),
            self.active,
        )
    }

    /// One UPLINK batch of `source` (spec §8, in ID order): `Ok` when it is
    /// accepted or a duplicate, else the error that stops the UPLINK.
    /// `reserved` tells whether the room's memory reservation for the
    /// UPLINK's batches succeeded.
    fn accept_batch(
        &mut self,
        source: u8,
        id: u64,
        opaque: &[u8],
        reserved: bool,
    ) -> Result<(), (OrderError, u64)> {
        if self.starts_at.is_none() {
            return Err((OrderError::NotStarted, 0));
        }
        if self.active & (1 << source) == 0 {
            return Err((OrderError::Inactive, 0));
        }
        let state = &self.sources[usize::from(source)];
        let last = state.last_batch.as_ref().map_or(0, |(id, _, _)| *id);
        if id < last {
            return Ok(());
        }
        if id == last {
            let (_, _, accepted) = state.last_batch.as_ref().expect("an accepted batch");
            return if accepted.as_slice() == opaque {
                Ok(())
            } else {
                Err((OrderError::ModifiedRetry, last))
            };
        }
        let expected = last + 1;
        if id != expected {
            return Err((OrderError::BatchGap, expected));
        }
        // The open frame: `advance` already sealed every due frame, and no
        // reader executes a frame before its FrameEnd.
        let frame = self.sealed + 1;
        if frame > (MAX_MATCH_SECONDS * u64::from(self.certificate.tick_hz)) as u32 {
            return Err((OrderError::Inactive, 0));
        }
        if !reserved
            || state.pending >= MAX_SOURCE_PENDING
            || self
                .pending
                .get(&frame)
                .is_some_and(|entries| entries.len() >= MAX_FRAME_BATCHES)
        {
            return Err((OrderError::Backpressure, expected));
        }
        if let Some(journal) = self.journal.as_mut()
            && !journal.reserve_batch(22 + opaque.len())
        {
            return Err((OrderError::Backpressure, expected));
        }
        self.pending.entry(frame).or_default().push(Pending {
            source,
            batch_id: id,
            opaque: opaque.to_vec(),
        });
        let state = &mut self.sources[usize::from(source)];
        state.last_batch = Some((id, frame, opaque.to_vec()));
        state.pending += 1;
        Ok(())
    }

    fn request(
        &mut self,
        ticket: Ticket,
        payload: &[u8],
        now_ms: u64,
    ) -> Result<Vec<Vec<u8>>, RelayError> {
        if self.storage_failed {
            return Ok(error(OrderError::StorageUnavailable, self.next_seq));
        }
        self.authenticate_reader(ticket)?;
        let Some(&kind) = payload.first() else {
            return Err(RelayError::Malformed);
        };
        // `[2][next varint]` (compaction spec §7.4).
        let mut reader = Reader(&payload[1..]);
        match (kind, reader.varint()) {
            (PULL_REQUEST, Ok(next)) if reader.is_empty() => {
                match self.history_page(next, ORDERING_PULL_LIMIT) {
                    Ok(records) => {
                        let mut output = vec![self.status(now_ms)];
                        output.extend(
                            pack::encode_pack_v2(&records, records.len())
                                .into_iter()
                                .map(|(pack, _)| pack),
                        );
                        Ok(output)
                    }
                    Err((code, expected)) => Ok(error(code, expected)),
                }
            }
            _ => Err(RelayError::Malformed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(super) fn client(key: &SecretKey, source: u8) -> ClientSession {
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

    pub(super) struct TestDirectory(pub(super) std::path::PathBuf);
    impl TestDirectory {
        pub(super) fn new() -> Self {
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
    pub(super) fn unix(now_ms: u64) -> u64 {
        1000 + now_ms / 1000
    }

    pub(super) fn renew(server: &mut RelayServer, client: &mut ClientSession, now_ms: u64) {
        let mut ticket = client.ticket;
        ticket.expires = (unix(now_ms) + 200).max(client.ticket.expires + 1);
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

    pub(super) fn endpoint(client: &ClientSession) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, 31000 + u16::from(client.source()))
    }

    pub(super) fn connect(server: &mut RelayServer, client: &mut ClientSession, now_ms: u64) {
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

    pub(super) fn events(
        server: &mut RelayServer,
        client: &mut ClientSession,
        bytes: &[u8],
        now_ms: u64,
    ) -> Vec<OrderEvent> {
        server
            .process_at(endpoint(client), bytes, unix(now_ms), now_ms)
            .unwrap()
            .into_iter()
            .flat_map(|packet| received(client, &packet.bytes))
            .collect()
    }

    /// One server datagram's ordering events, each entry of a pack.
    pub(super) fn received(client: &mut ClientSession, bytes: &[u8]) -> Vec<OrderEvent> {
        match client.receive(bytes).unwrap() {
            ServerEvent::Ordering(event) => vec![event],
            ServerEvent::OrderingPack(events) => events,
            _ => panic!("ordering"),
        }
    }

    pub(super) fn configure(
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

    pub(super) fn pull(
        server: &mut RelayServer,
        client: &mut ClientSession,
        cursor: u64,
        now_ms: u64,
    ) -> Vec<OrderEvent> {
        let mut bytes = Vec::new();
        client.pull_ordering(cursor, &mut bytes).unwrap();
        events(server, client, &bytes, now_ms)
    }

    fn ordering(server: &RelayServer) -> &OrderingRoom {
        server.rooms[&[5; 16]].ordering.as_ref().unwrap()
    }

    pub(super) fn certificate(key: &SecretKey, mask: u32, expires: u64) -> Vec<u8> {
        OrderingCertificate {
            room: [5; 16],
            sources_mask: mask,
            tick_hz: 45,
            expires,
            pacing: Default::default(),
        }
        .sign(key)
        .unwrap()
    }

    /// The batches of an encoded client UPLINK datagram.
    fn uplink_batches(datagram: &[u8]) -> Vec<Vec<u8>> {
        crate::link::decode_uplink(&datagram[CLIENT_HEADER..datagram.len() - HOP_TAG_LEN])
            .unwrap()
            .batches
            .iter()
            .map(|batch| batch.to_vec())
            .collect()
    }

    /// An UPLINK payload with `flags` carrying `batches` from `first`.
    fn raw_uplink(flags: u8, first: u64, batches: &[&[u8]]) -> Vec<u8> {
        let mut payload = Vec::new();
        crate::link::Uplink {
            flags,
            recv_next: 1,
            down: RecvWindow::default(),
            srtt_ms: 0,
            base_ms: 0,
            rttvar_ms: 0,
            link: 0,
            checksums: Vec::new(),
            pace: None,
            first_batch_id: first,
            batches: batches.to_vec(),
        }
        .encode(&mut payload);
        payload
    }

    /// An UPLINK with PUSH acknowledging `cursor`, without batches.
    pub(super) fn subscribe(
        server: &mut RelayServer,
        client: &mut ClientSession,
        cursor: u64,
        now_ms: u64,
    ) -> Vec<OrderEvent> {
        let mut bytes = Vec::new();
        client.ordering.as_mut().unwrap().link.schedule.recv_next = cursor;
        client.test_uplink(None, now_ms, &mut bytes).unwrap();
        events(server, client, &bytes, now_ms)
    }

    fn pause_control(
        server: &mut RelayServer,
        client: &mut ClientSession,
        id: u64,
        epoch: u64,
        paused: bool,
        now_ms: u64,
    ) -> OrderEvent {
        let mut bytes = Vec::new();
        client
            .control_ordering_pause(id, epoch, paused, &mut bytes)
            .unwrap();
        events(server, client, &bytes, now_ms).remove(0)
    }

    fn pace_report(
        server: &mut RelayServer,
        client: &mut ClientSession,
        pace: OrderPace,
        frames_per_second: u16,
        now_ms: u64,
    ) {
        tick_report(server, client, pace, frames_per_second, None, now_ms);
    }

    fn tick_report(
        server: &mut RelayServer,
        client: &mut ClientSession,
        pace: OrderPace,
        frames_per_second: u16,
        tick_micros: Option<u32>,
        now_ms: u64,
    ) {
        let mut bytes = Vec::new();
        client.queue_ordering_pace(pace, frames_per_second, tick_micros);
        client.test_uplink(None, now_ms, &mut bytes).unwrap();
        assert!(events(server, client, &bytes, now_ms).is_empty());
    }

    fn paced_room(key: &SecretKey) -> (RelayServer, ClientSession, ClientSession) {
        paced_room_with(key, CertificatePacing::Legacy)
    }

    fn paced_room_with(
        key: &SecretKey,
        pacing: CertificatePacing,
    ) -> (RelayServer, ClientSession, ClientSession) {
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut first, mut second) = (client(key, 0), client(key, 1));
        let cert = OrderingCertificate {
            pacing,
            ..OrderingCertificate::parse(&certificate(key, 3, 1400)).unwrap()
        }
        .sign(key)
        .unwrap();
        for peer in [&mut first, &mut second] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
        }
        (server, first, second)
    }

    /// Under VO2C a silent member does not hold the clock, a player that only
    /// draws slowly catches up on its own, and one whose simulation cannot
    /// hold the rate slows it to a rate it can; the host can switch it off.
    #[test]
    fn a_vo2c_room_slows_only_for_a_simulation_that_cannot_hold_the_rate() {
        use OrderPace::*;
        let key = SecretKey::new([73; 32]);
        let (mut server, mut first, mut second) = paced_room_with(&key, CertificatePacing::On);
        // 10 ms ticks hold 80 Hz at 80%: running 30 is only a slow draw.
        tick_report(&mut server, &mut second, Behind, 30, Some(10_000), 1000);
        server.advance_clock(1000, unix(1000));
        assert_eq!(
            ordering(&server).pace.rate,
            45,
            "a slow draw alone holds the rate, and the silent first player holds nothing"
        );
        // 30 ms ticks hold 26 Hz: slow to the fastest paced rate at most 26.
        tick_report(&mut server, &mut second, Behind, 25, Some(30_000), 2000);
        server.advance_clock(2000, unix(2000));
        assert_eq!(ordering(&server).pace.rate, 20);
        // The slow player keeps up, but 30 ms ticks cannot hold 30 Hz: no
        // raise. A player that only draws slowly holds nothing back.
        for at in (3000..=40_000).step_by(2000) {
            tick_report(&mut server, &mut first, CatchingUp, 20, Some(5_000), at);
            tick_report(&mut server, &mut second, KeepingUp, 20, Some(30_000), at);
            server.advance_clock(at, unix(at));
        }
        assert_eq!(ordering(&server).pace.rate, 20);
        tick_report(&mut server, &mut first, CatchingUp, 20, Some(5_000), 41_000);
        tick_report(
            &mut server,
            &mut second,
            KeepingUp,
            20,
            Some(20_000),
            41_000,
        );
        server.advance_clock(41_000, unix(41_000));
        assert_eq!(ordering(&server).pace.rate, 30);

        let (mut server, _first, mut second) = paced_room_with(&key, CertificatePacing::Off);
        tick_report(&mut server, &mut second, Behind, 5, Some(200_000), 1000);
        server.advance_clock(1000, unix(1000));
        assert_eq!(
            ordering(&server).pace.rate,
            45,
            "the host switched pacing off"
        );
    }

    /// The clock seals whole turns: three frames at a time at 45 Hz, about
    /// fifteen turns a second at every rate.
    #[test]
    fn a_45_hz_room_seals_in_three_frame_turns() {
        assert_eq!(
            [60, 45, 30, 20, 15, 10].map(turn_frames),
            [4, 3, 2, 2, 1, 1]
        );
        let key = SecretKey::new([73; 32]);
        let (mut server, _first, _second) = paced_room(&key);
        let start = ordering(&server).starts_at.unwrap();
        let mut heads = Vec::new();
        // Frame 3 is due 66.7 ms after the start, frame 6 at 133.3 ms.
        for at in [66, 67, 100, 133, 134] {
            server.advance_clock(start + at, unix(start + at));
            heads.push(ordering(&server).head);
        }
        assert_eq!(heads, [0, 3, 3, 3, 6]);
    }

    /// A pull carries its page packed after the status, and the client
    /// rebuilds every entry's exact canonical bytes and sequence.
    /// Spec appendix A packets 5 to 7: the relay's window starts at CONFIRM,
    /// its STATUS reply carries it, and the client takes its first RTT
    /// sample from the socket times.
    #[test]
    fn a_status_reply_carries_the_hop_window_and_gives_the_first_rtt_sample() {
        let key = SecretKey::new([75; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut first = client(&key, 0);
        connect(&mut server, &mut first, 0);
        let mut bytes = Vec::new();
        first
            .configure_ordering_at(&certificate(&key, 3, 1400), [7; 32], 80, &mut bytes)
            .unwrap();
        let replies = server
            .process_at(endpoint(&first), &bytes, unix(100), 100)
            .unwrap();
        first.receive_at(&replies[0].bytes, 120).unwrap();
        let ServerEvent::Ordering(OrderEvent::Status(status)) =
            first.receive_at(&replies[1].bytes, 120).unwrap()
        else {
            panic!("status");
        };
        assert_eq!(
            status.feedback.up,
            RecvWindow {
                high: 3,
                bits: 0b11
            }
        );
        assert_eq!(status.feedback.echo_delay_ms, 0);
        let stats = first.link_stats();
        assert_eq!((stats.srtt_ms, stats.rttvar_ms), (Some(40), Some(20)));
        // READY (1), ordering READY (2) and STATUS (3).
        assert_eq!(
            stats.down,
            RecvWindow {
                high: 3,
                bits: 0b111
            }
        );
        assert_eq!(stats.datagrams_received, 3);
    }

    #[test]
    fn rtt_samples_subtract_the_echo_delay_once_per_logged_sequence() {
        let key = SecretKey::new([76; 32]);
        let mut session = client(&key, 0);
        session.ready = true;
        session.session = Some(1);
        let mut bytes = Vec::new();
        session
            .configure_ordering_at(&certificate(&key, 3, 1400), [7; 32], 1000, &mut bytes)
            .unwrap();
        let feedback = |high, echo_delay_ms| HopFeedback {
            up: RecvWindow { high, bits: 1 },
            echo_delay_ms,
        };
        session.take_feedback(feedback(1, 25), 1100);
        assert_eq!(session.link_stats().srtt_ms, Some(75));
        // The same up_high again, or a saturated echo delay, gives no sample.
        session.take_feedback(feedback(1, 0), 1200);
        for _ in 0..129 {
            session.pull_ordering_at(1, 1300, &mut bytes).unwrap();
        }
        // Sequence 2 was overwritten in the 128-entry log by 130.
        session.take_feedback(feedback(2, 0), 1400);
        session.take_feedback(feedback(129, u16::MAX), 1400);
        assert_eq!(session.link_stats().rtt_samples, 1);
        session.take_feedback(feedback(130, 0), 1400);
        assert_eq!(session.link_stats().rtt_samples, 2);
    }

    #[test]
    fn the_uplink_goes_offline_after_a_silent_second_and_recovers_on_receipt() {
        let key = SecretKey::new([77; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut first = client(&key, 0);
        connect(&mut server, &mut first, 0);
        let mut bytes = Vec::new();
        first
            .configure_ordering_at(&certificate(&key, 3, 1400), [7; 32], 0, &mut bytes)
            .unwrap();
        let replies = server
            .process_at(endpoint(&first), &bytes, unix(0), 0)
            .unwrap();
        first.preset_uplink_measured_good();
        for reply in &replies {
            first.receive_at(&reply.bytes, 0).unwrap();
        }
        first.evaluate_uplink(999);
        assert_eq!(first.link_stats().uplink, LinkState::Good);
        first.evaluate_uplink(1000);
        assert_eq!(first.link_stats().uplink, LinkState::Offline);
        first.ordering.as_mut().unwrap().link.rto_backoff = 3;
        // A Backpressure error ends the silence and holds congestion 2 s.
        let mut error = vec![ERROR_EVENT, OrderError::Backpressure as u8];
        error.extend_from_slice(&[0; 8]);
        let datagram = server_packet(
            server
                .rooms
                .get_mut(&[5; 16])
                .unwrap()
                .peers
                .get_mut(0)
                .unwrap(),
            crate::CONTROL,
            0,
            &error,
            1500,
            crate::link::SEND_OTHER,
        )
        .unwrap();
        first.receive_at(&datagram.bytes, 1500).unwrap();
        let link = &first.ordering.as_ref().unwrap().link;
        assert!(link.recovered);
        assert_eq!(link.rto_backoff, 0);
        assert_eq!(first.link_stats().uplink, LinkState::Congested);
        first.evaluate_uplink(3499);
        assert_eq!(first.link_stats().uplink, LinkState::Offline);
        let stats = first.link_stats();
        assert_eq!(stats.uplink_state_entries, [0, 0, 1, 2]);
        assert_eq!(stats.uplink_state_ms, [1000, 0, 1999, 500]);
    }

    /// Spec appendix A packets 20 and 25 in PACK v2: a pack with two piggybacked frame
    /// ends before a batch, and a status heartbeat's feedback.
    #[test]
    fn appendix_pack_and_status_heartbeat_have_their_layouts() {
        let opaque = [9; 74];
        let mut batch = vec![0];
        batch.extend_from_slice(&1u64.to_le_bytes());
        batch.extend_from_slice(&opaque);
        let records = [
            OrderingRoom::encoded_entry(3, 3, FRAME_END_EVENT, &[]),
            OrderingRoom::encoded_entry(4, 4, FRAME_END_EVENT, &[]),
            OrderingRoom::encoded_entry(5, 5, BATCH_EVENT, &batch),
            OrderingRoom::encoded_entry(6, 5, FRAME_END_EVENT, &[]),
            OrderingRoom::encoded_entry(7, 6, FRAME_END_EVENT, &[]),
        ];
        // PACK v2 (compaction spec §5.2): 86 B, 128 B in v1.
        let packs = pack::encode_pack_v2(&records, 1);
        let (pack, count) = &packs[0];
        assert_eq!((pack.len(), *count), (86, 5));
        assert_eq!(&pack[..6], &[PACK_EVENT, 3, 3, 5, 0x04, 0x14]);
        assert_eq!(&pack[6..10], &[0x13, 0, 1, 74]);
        assert_eq!(&pack[84..], &[0x04, 0x14]);
        assert_eq!(pack::decode_pack_v2(pack).unwrap(), records);

        let key = SecretKey::new([78; 32]);
        let mut reader = client(&key, 1);
        reader.ready = true;
        reader.session = Some(1);
        reader
            .configure_ordering(&certificate(&key, 3, 1400), [7; 32], &mut Vec::new())
            .unwrap();
        // Compaction spec §7.3: 22 B.
        #[rustfmt::skip]
        let status = [
            STATUS_EVENT, 10, 1, 12, 0b11, 0, 0, 0, 30, 0, 0, 1,
            8, 0x7D, 0, 0, 0, 0, 0, 0, 0, 33,
        ];
        let Ok(OrderEvent::Status(status)) = reader.receive_ordering(&status) else {
            panic!("status");
        };
        assert_eq!(
            (status.head_frame, status.next_seq, status.started),
            (10, 12, true)
        );
        assert_eq!(
            status.feedback.up,
            RecvWindow {
                high: 8,
                bits: 0x7D
            }
        );
        assert_eq!(status.feedback.echo_delay_ms, 33);
    }

    #[test]
    fn a_pull_packs_its_page_and_the_client_reads_each_canonical_entry() {
        let key = SecretKey::new([73; 32]);
        let (mut server, mut first, mut second) = paced_room(&key);
        let mut bytes = Vec::new();
        second.test_submit(1, b"packed input", &mut bytes).unwrap();
        events(&mut server, &mut second, &bytes, 0);
        server.advance_clock(400, unix(400));
        first.pull_ordering(1, &mut bytes).unwrap();
        let packets = server
            .process_at(endpoint(&first), &bytes, unix(400), 400)
            .unwrap();
        assert_eq!(packets.len(), 2, "a status and one pack");
        let payload = &packets[1].bytes[SERVER_HEADER..packets[1].bytes.len() - HOP_TAG_LEN];
        let history: Vec<_> = ordering(&server)
            .history
            .iter()
            .map(|entry| entry.bytes.clone())
            .collect();
        assert_eq!(pack::decode_pack_v2(payload).unwrap(), history);
        let entries: Vec<_> = received(&mut first, &packets[1].bytes)
            .into_iter()
            .map(|event| match event {
                OrderEvent::Entry(OrderEntry::Batch {
                    sequence, bytes, ..
                }) => (sequence, Some(bytes)),
                OrderEvent::Entry(OrderEntry::FrameEnd { sequence, .. }) => (sequence, None),
                _ => panic!("a batch or frame end"),
            })
            .collect();
        assert_eq!(entries.len(), 10, "a batch and nine frame ends");
        assert_eq!(entries[0], (1, Some(b"packed input".to_vec())));
        assert!(
            entries
                .iter()
                .enumerate()
                .all(|(index, (sequence, _))| *sequence == index as u64 + 1)
        );
    }

    fn report_checksums(
        server: &mut RelayServer,
        client: &mut ClientSession,
        samples: &[(u32, u32)],
        now_ms: u64,
    ) {
        let mut bytes = Vec::new();
        client.queue_ordering_checksums(samples, now_ms).unwrap();
        client.test_uplink(None, now_ms, &mut bytes).unwrap();
        assert!(events(server, client, &bytes, now_ms).is_empty());
    }

    /// The DESYNC records of a room's history, decoded, each with the entry
    /// before it.
    fn desyncs(server: &RelayServer, reader: &mut ClientSession) -> Vec<(u8, OrderEntry)> {
        let history = &ordering(server).history;
        (1..history.len())
            .filter(|&index| history[index].bytes[0] == DESYNC_EVENT)
            .map(|index| {
                let OrderEvent::Entry(entry) =
                    reader.receive_ordering(&history[index].bytes).unwrap()
                else {
                    panic!("a desync entry");
                };
                (history[index - 1].bytes[0], entry)
            })
            .collect()
    }

    /// The relay compares every player's checksum of a sampled frame and
    /// ends the match once, with a DESYNC record after the head's frame
    /// end: of three players it names the one off the majority, of two the
    /// later reporter.
    #[test]
    fn the_relay_names_the_player_whose_checksum_differs_once() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let mut players: Vec<_> = (0..3).map(|source| client(&key, source)).collect();
        let cert = certificate(&key, 7, 1400);
        for player in &mut players {
            connect(&mut server, player, 0);
            configure(&mut server, player, &cert, 0);
        }
        server.advance_clock(1000, unix(1000));
        for player in &mut players {
            report_checksums(&mut server, player, &[(15, 7)], 1000);
        }
        // Repeats of a report change nothing; one against one of three waits.
        report_checksums(&mut server, &mut players[0], &[(30, 9), (15, 7)], 1100);
        report_checksums(&mut server, &mut players[1], &[(30, 8), (15, 7)], 1100);
        report_checksums(&mut server, &mut players[1], &[(30, 9)], 1100);
        assert!(desyncs(&server, &mut players[0]).is_empty());
        report_checksums(&mut server, &mut players[2], &[(30, 9), (15, 7)], 1200);
        let head = ordering(&server).head;
        // Nothing is compared again, though the relay has sealed frame 45.
        server.advance_clock(1300, unix(1300));
        report_checksums(&mut server, &mut players[0], &[(45, 1)], 1300);
        report_checksums(&mut server, &mut players[2], &[(45, 2)], 1300);
        assert!(matches!(
            &desyncs(&server, &mut players[0])[..],
            [(
                FRAME_END_EVENT,
                OrderEntry::Desync {
                    frame,
                    sample_frame: 30,
                    outlier: 1,
                    outlier_checksum: 8,
                    other: 0,
                    other_checksum: 9,
                    ..
                }
            )] if *frame == head
        ));
        assert_eq!(server.take_ordering_counts().desyncs, 1);

        let (mut server, mut first, mut second) = paced_room(&key);
        server.advance_clock(1000, unix(1000));
        report_checksums(&mut server, &mut first, &[(15, 9)], 1000);
        report_checksums(&mut server, &mut second, &[(15, 8)], 1000);
        assert!(matches!(
            &desyncs(&server, &mut first)[..],
            [(
                _,
                OrderEntry::Desync {
                    outlier: 1,
                    outlier_checksum: 8,
                    other: 0,
                    other_checksum: 9,
                    ..
                }
            )]
        ));
    }

    /// A player reports only sampled frames the relay has reached, so a flood
    /// of later ones cannot push the others' reports out of the window.
    #[test]
    fn far_future_checksum_reports_cannot_switch_desync_detection_off() {
        let key = SecretKey::new([73; 32]);
        let (mut server, mut first, mut second) = paced_room(&key);
        server.advance_clock(1000, unix(1000));
        for pair in 0..CHECKSUM_FRAMES as u32 / 2 {
            let frame = 15_000 + 30 * pair;
            report_checksums(
                &mut server,
                &mut second,
                &[(frame + 15, 1), (frame, 1)],
                1000,
            );
        }
        report_checksums(&mut server, &mut first, &[(15, 9)], 1000);
        report_checksums(&mut server, &mut second, &[(15, 8)], 1000);
        assert_eq!(desyncs(&server, &mut first).len(), 1);
    }

    /// A player's reports stop counting when it leaves: one that reports a
    /// false checksum and then leaves cannot end the other's match.
    #[test]
    fn a_departed_players_checksum_names_no_outlier() {
        let key = SecretKey::new([73; 32]);
        let (mut server, mut first, mut second) = paced_room(&key);
        server.advance_clock(1000, unix(1000));
        report_checksums(&mut server, &mut second, &[(30, 8)], 1000);
        let mut bytes = Vec::new();
        second.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut second, &bytes, 1000);
        report_checksums(&mut server, &mut first, &[(30, 9)], 1000);
        assert!(desyncs(&server, &mut first).is_empty());
    }

    /// A follower whose healed link delivers two rate changes at once applies
    /// both in order, each from its frame's due time on the old schedule.
    #[test]
    fn queued_federated_rate_changes_apply_in_order_on_one_timeline() {
        let key = SecretKey::new([73; 32]);
        let (mut server, _first, _second) = paced_room(&key);
        let room = server.rooms.get_mut(&[5; 16]).unwrap();
        let ordering = room.ordering.as_mut().unwrap();
        let start = ordering.starts_at.unwrap();
        ordering.pace.scheduled.extend([(90, 30, 1), (180, 20, 1)]);
        // 90 frames at 45 Hz are due 2 s in; 90 more at 30 Hz, 3 s later.
        ordering.apply_scheduled_rate(start + 4_999, start);
        assert_eq!(
            (ordering.pace.rate, ordering.pace.federated_anchor),
            (30, Some((90, 2_000)))
        );
        ordering.apply_scheduled_rate(start + 5_000, start);
        assert_eq!(
            (ordering.pace.rate, ordering.pace.federated_anchor),
            (20, Some((180, 5_000)))
        );
        assert!(ordering.pace.scheduled.is_empty());
    }

    /// VO2C adds the host's pacing flag under its own signing domain; the
    /// vectors are an independent HMAC-SHA256 the lobby also checks against.
    #[test]
    fn a_vo2c_certificate_signs_its_pacing_flag_under_its_own_domain() {
        let key = SecretKey::new([73; 32]);
        let cert = |pacing| OrderingCertificate {
            room: [5; 16],
            sources_mask: 3,
            tick_hz: 45,
            expires: 2000,
            pacing,
        };
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let on = cert(CertificatePacing::On).sign(&key).unwrap();
        let off = cert(CertificatePacing::Off).sign(&key).unwrap();
        assert_eq!(
            hex(&on),
            "564f324305050505050505050505050505050505030000002d00d007000000000000003e4c04db8da83d3705b259262fafbcf274224f6ba8d59b743e14db3d5b10dc48"
        );
        assert_eq!(
            hex(&off),
            "564f324305050505050505050505050505050505030000002d00d00700000000000001252f63eae21061da149b08caacb3bbecf61985bf996d0932b3e101330bbc461d"
        );
        assert_eq!(
            OrderingCertificate::verify(&off, &key, 1000)
                .unwrap()
                .pacing,
            CertificatePacing::Off
        );
        let legacy = cert(CertificatePacing::Legacy).sign(&key).unwrap();
        assert_eq!(legacy.len(), ORDERING_CERTIFICATE_LEN);
        assert_eq!(
            OrderingCertificate::verify(&legacy, &key, 1000)
                .unwrap()
                .pacing,
            CertificatePacing::Legacy
        );
        // A VO1C body relabelled VO2C does not verify, and unknown flags fail.
        let mut relabelled = on.clone();
        relabelled[34] = 2;
        assert!(matches!(
            OrderingCertificate::parse(&relabelled),
            Err(RelayError::Malformed)
        ));
        let mut forged = legacy.clone();
        forged[..4].copy_from_slice(CERTIFICATE_V2_MAGIC);
        forged.insert(34, 0);
        assert!(matches!(
            OrderingCertificate::verify(&forged, &key, 1000),
            Err(RelayError::Authentication)
        ));
    }

    #[test]
    fn a_player_who_cannot_keep_up_slows_the_clock_at_most_two_rows() {
        use OrderPace::*;
        let key = SecretKey::new([73; 32]);
        let (mut server, mut first, mut second) = paced_room(&key);
        pace_report(&mut server, &mut second, Behind, 40, 1000);
        server.advance_clock(1000, unix(1000));
        assert_eq!(
            ordering(&server).pace.rate,
            45,
            "a player who has not reported may not read a rate marker"
        );
        pace_report(&mut server, &mut first, KeepingUp, 45, 1000);
        server.advance_clock(1000, unix(1000));
        server.advance_clock(2000, unix(2000));
        assert_eq!(ordering(&server).head, 36 + 30);
        pace_report(&mut server, &mut second, Behind, 25, 3000);
        server.advance_clock(3000, unix(3000));
        assert_eq!(
            ordering(&server).pace.rate,
            30,
            "one change per five seconds"
        );
        pace_report(&mut server, &mut second, Behind, 25, 6000);
        pace_report(&mut server, &mut second, Behind, 5, 11_000);
        server.advance_clock(11_000, unix(11_000));
        let markers: Vec<_> = ordering(&server)
            .history
            .iter()
            .filter(|entry| entry.bytes[0] == RATE_EVENT)
            .map(
                |entry| match first.receive_ordering(&entry.bytes).unwrap() {
                    OrderEvent::Entry(OrderEntry::Rate {
                        frame,
                        frames_per_second,
                        source,
                        ..
                    }) => (frame, frames_per_second, source),
                    _ => panic!("rate marker"),
                },
            )
            .collect();
        // 40 a second leaves 30 a tenth of slack; 25 leaves 20 none to spare.
        assert_eq!(markers, [(36, 30, Some(1)), (186, 20, Some(1))]);
    }

    #[test]
    fn the_clock_speeds_up_once_nobody_lags_and_waits_twice_as_long_after_a_relapse() {
        use OrderPace::*;
        let key = SecretKey::new([73; 32]);
        let (mut server, mut first, mut second) = paced_room(&key);
        pace_report(&mut server, &mut first, KeepingUp, 45, 1000);
        pace_report(&mut server, &mut second, Behind, 40, 1000);
        server.advance_clock(1000, unix(1000));
        // Frames still wait on the slow player: the wait starts once they don't.
        pace_report(&mut server, &mut second, CatchingUp, 33, 2000);
        pace_report(&mut server, &mut second, CatchingUp, 33, 5000);
        pace_report(&mut server, &mut second, KeepingUp, 30, 8000);
        server.advance_clock(37_999, unix(37_999));
        assert_eq!(ordering(&server).pace.rate, 30);
        server.advance_clock(38_000, unix(38_000));
        assert_eq!(ordering(&server).pace.rate, 45);
        pace_report(&mut server, &mut second, Behind, 40, 46_000);
        server.advance_clock(46_000, unix(46_000));
        pace_report(&mut server, &mut second, KeepingUp, 30, 47_000);
        server.advance_clock(106_999, unix(106_999));
        assert_eq!(ordering(&server).pace.rate, 30);
        server.advance_clock(107_000, unix(107_000));
        assert_eq!(ordering(&server).pace.rate, 45);
        pull(&mut server, &mut first, 1, 107_000);
        let counts = server.take_ordering_counts();
        assert_eq!(
            (counts.slowdowns, counts.raises, counts.pull_replies),
            (2, 2, 1)
        );
        assert_eq!(server.take_ordering_counts(), Default::default());
    }

    #[test]
    fn a_pause_holds_the_wait_before_speeding_up() {
        use OrderPace::*;
        let key = SecretKey::new([73; 32]);
        let (mut server, mut first, mut second) = paced_room(&key);
        for peer in [&mut first, &mut second] {
            pause_control(&mut server, peer, 0, 0, false, 500);
        }
        pace_report(&mut server, &mut first, KeepingUp, 45, 1000);
        pace_report(&mut server, &mut second, Behind, 40, 1000);
        server.advance_clock(1000, unix(1000));
        pace_report(&mut server, &mut second, CatchingUp, 35, 20_000);
        server.advance_clock(20_000, unix(20_000));
        pause_control(&mut server, &mut first, 1, 0, true, 40_000);
        pause_control(&mut server, &mut second, 1, 1, false, 140_000);
        server.advance_clock(149_999, unix(149_999));
        assert_eq!(ordering(&server).pace.rate, 30);
        server.advance_clock(150_000, unix(150_000));
        assert_eq!(ordering(&server).pace.rate, 45);
    }

    #[test]
    fn the_journal_count_has_each_slowdown_once_and_no_speedup() {
        use OrderPace::*;
        let key = SecretKey::new([73; 32]);
        let (mut server, mut first, mut second) = paced_room(&key);
        pace_report(&mut server, &mut first, KeepingUp, 45, 1000);
        pace_report(&mut server, &mut second, Behind, 40, 1000);
        server.advance_clock(1000, unix(1000));
        pace_report(&mut server, &mut second, KeepingUp, 30, 2000);
        server.advance_clock(32_000, unix(32_000));
        assert_eq!(ordering(&server).pace.rate, 45, "slowed, then sped up");
        assert_eq!(server.take_ordering_counts().slowdowns, 1);
        assert_eq!(server.take_ordering_counts().slowdowns, 0);
    }

    #[test]
    fn the_journal_count_has_a_second_for_each_catching_up_or_behind_report() {
        use OrderPace::*;
        let key = SecretKey::new([73; 32]);
        let (mut server, mut first, mut second) = paced_room(&key);
        pace_report(&mut server, &mut first, KeepingUp, 45, 1000);
        pace_report(&mut server, &mut second, CatchingUp, 44, 1000);
        pace_report(&mut server, &mut second, Behind, 40, 2000);
        pace_report(&mut server, &mut second, KeepingUp, 45, 3000);
        assert_eq!(server.take_ordering_counts().catching_up_s, 2);
        assert_eq!(server.take_ordering_counts().catching_up_s, 0);
    }

    /// Released clients cannot be changed, so a newer relay may only append
    /// to replies. A reply decodes by its known prefix; a newer error code or
    /// record is reported as unrecognized, and a canonical entry decodes only
    /// in full.
    #[test]
    fn newer_replies_decode_by_prefix_and_newer_records_are_unrecognized() {
        let key = SecretKey::new([74; 32]);
        let mut server = RelayServer::new(key.clone(), 1);
        let mut reader = client(&key, 0);
        connect(&mut server, &mut reader, 0);
        reader
            .configure_ordering(&certificate(&key, 3, 1400), [7; 32], &mut Vec::new())
            .unwrap();
        // STATUS (compaction spec §7.3), all zero but `started`, its feedback
        // tail and one byte more.
        let mut status = vec![STATUS_EVENT, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        crate::link::append_feedback(&mut status, crate::link::HopFeedback::default());
        status.push(0);
        assert!(matches!(
            reader.receive_ordering(&status),
            Ok(OrderEvent::Status(OrderStatus { started: true, .. }))
        ));
        let mut error = vec![ERROR_EVENT, OrderError::NotStarted as u8, 0, 0];
        assert!(matches!(
            reader.receive_ordering(&error),
            Ok(OrderEvent::Error {
                code: OrderError::NotStarted,
                ..
            })
        ));
        error[1] = 200;
        let departure = |reason: &[u8]| {
            let mut payload = vec![DEPARTURE_EVENT];
            payload.extend_from_slice(&[0; 12]);
            payload.push(0);
            payload.extend_from_slice(reason);
            payload
        };
        let mut frame_end = vec![FRAME_END_EVENT];
        frame_end.resize(14, 0);
        for payload in [error, departure(&[2]), departure(&[1, 0]), frame_end] {
            assert!(matches!(
                reader.receive_ordering(&payload),
                Ok(OrderEvent::Unrecognized)
            ));
        }
        assert!(matches!(
            reader.receive_ordering(&departure(&[1])),
            Ok(OrderEvent::Entry(OrderEntry::Departure { lost: true, .. }))
        ));
    }

    #[test]
    fn nobody_pauses_again_until_the_match_has_run_after_a_resume() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1400);
        for peer in [&mut first, &mut second] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
            pause_control(&mut server, peer, 0, 0, false, 300);
        }
        pause_control(&mut server, &mut first, 1, 0, true, 300);
        pause_control(&mut server, &mut second, 1, 1, false, 400);
        let paused = |event| matches!(event, OrderEvent::PauseStatus { paused: true, .. });
        let early = 400 + PAUSE_COOLDOWN_MS - 1;
        assert!(!paused(pause_control(
            &mut server,
            &mut first,
            2,
            2,
            true,
            early
        )));
        assert!(paused(pause_control(
            &mut server,
            &mut first,
            3,
            2,
            true,
            early + 1
        )));
    }

    #[test]
    fn a_one_source_certificate_opens_no_ordering_room() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let mut solo = client(&key, 0);
        connect(&mut server, &mut solo, 0);
        let mut bytes = Vec::new();
        solo.configure_ordering(&certificate(&key, 1, 1400), [7; 32], &mut bytes)
            .unwrap();
        assert!(matches!(
            server.process_at(endpoint(&solo), &bytes, unix(0), 0),
            Err(RelayError::Authentication)
        ));
        assert!(server.rooms[&[5; 16]].ordering.is_none());
    }

    #[test]
    fn pause_requires_all_capabilities_and_rejects_stale_control_epochs() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1400);
        for peer in [&mut first, &mut second] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
        }
        assert!(matches!(
            pause_control(&mut server, &mut first, 0, 0, false, 300),
            OrderEvent::PauseStatus {
                supported_sources: 1,
                request_id: 0,
                paused: false,
                ..
            }
        ));
        assert!(matches!(
            pause_control(&mut server, &mut first, 1, 0, true, 300),
            OrderEvent::PauseStatus {
                request_id: 1,
                epoch: 0,
                paused: false,
                ..
            }
        ));
        pause_control(&mut server, &mut second, 0, 0, false, 300);
        assert!(
            matches!(
                pause_control(&mut server, &mut first, 1, 0, true, 310),
                OrderEvent::PauseStatus {
                    request_id: 1,
                    epoch: 0,
                    paused: false,
                    ..
                }
            ),
            "a previously rejected request cannot become an action after quorum"
        );
        assert!(matches!(
            pause_control(&mut server, &mut first, 2, 0, true, 310),
            OrderEvent::PauseStatus {
                supported_sources: 3,
                request_id: 2,
                epoch: 1,
                paused: true,
                frame: 3,
                ..
            }
        ));
        // Fresh envelopes carrying the same request cannot append twice.
        assert!(matches!(
            pause_control(&mut server, &mut first, 2, 0, true, 1000),
            OrderEvent::PauseStatus {
                epoch: 1,
                frame: 3,
                paused: true,
                ..
            }
        ));
        assert!(matches!(
            pause_control(&mut server, &mut first, 2, 0, false, 1000),
            OrderEvent::Error {
                code: OrderError::ModifiedRetry,
                expected: 2
            }
        ));
        assert!(matches!(
            pause_control(&mut server, &mut second, 1, 0, false, 1000),
            OrderEvent::PauseStatus {
                request_id: 1,
                epoch: 1,
                paused: true,
                ..
            }
        ));
        assert!(matches!(
            pause_control(&mut server, &mut second, 2, 1, false, 2000),
            OrderEvent::PauseStatus {
                epoch: 2,
                paused: false,
                frame: 3,
                ..
            }
        ));
        server.advance_clock(2023, unix(2023));
        assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().head, 3);
        server.advance_clock(2024, unix(2024));
        assert_eq!(
            server.rooms[&[5; 16]].ordering.as_ref().unwrap().head,
            6,
            "resume preserves the next frame deadline instead of catching up paused time"
        );
        let markers: Vec<_> = pull(&mut server, &mut first, 1, 2024)
            .into_iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::Pause {
                    epoch,
                    paused,
                    frame,
                    ..
                }) => Some((epoch, paused, frame)),
                _ => None,
            })
            .collect();
        assert_eq!(markers, [(1, true, 3), (2, false, 3)]);
    }

    #[test]
    fn lost_pause_marker_replays_after_renewal_and_preserves_pending_input() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1400);
        for peer in [&mut first, &mut second] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
            pause_control(&mut server, peer, 0, 0, false, 0);
        }
        subscribe(&mut server, &mut first, 1, 300);
        let mut bytes = Vec::new();
        second
            .test_submit(1, b"accepted before pause", &mut bytes)
            .unwrap();
        events(&mut server, &mut second, &bytes, 300);
        pause_control(&mut server, &mut first, 1, 0, true, 300);
        let mut lost = None;
        for packet in server.drain_ordering_push(300, unix(300)) {
            let payload = &packet.bytes[SERVER_HEADER..packet.bytes.len() - HOP_TAG_LEN];
            if payload[0] == PACK_EVENT {
                lost = pack::decode_pack_v2(payload)
                    .unwrap()
                    .into_iter()
                    .find(|entry| entry[0] == PAUSE_EVENT);
            } else {
                first.receive(&packet.bytes).unwrap();
            }
        }
        let lost = lost.expect("the pause marker was in the intentionally lost push");
        server.advance_clock(1000, unix(1000));
        assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().head, 3);
        renew(&mut server, &mut first, 1000);
        configure(&mut server, &mut first, &cert, 1000);
        assert!(matches!(
            pause_control(&mut server, &mut first, 1, 0, true, 1000),
            OrderEvent::PauseStatus {
                request_id: 1,
                epoch: 1,
                paused: true,
                ..
            }
        ));
        assert!(matches!(
            pause_control(&mut server, &mut second, 1, 1, false, 1000),
            OrderEvent::PauseStatus {
                epoch: 2,
                paused: false,
                frame: 3,
                ..
            }
        ));
        server.advance_clock(250_000, unix(250_000));
        assert!(
            server.rooms[&[5; 16]]
                .ordering
                .as_ref()
                .unwrap()
                .hot_oldest_seq()
                > 1
        );
        renew(&mut server, &mut first, 250_000);
        configure(&mut server, &mut first, &cert, 250_000);
        first.pull_ordering(1, &mut bytes).unwrap();
        let packets = server
            .process_at(endpoint(&first), &bytes, unix(250_000), 250_000)
            .unwrap();
        assert!(
            packets.iter().any(|packet| {
                let payload = &packet.bytes[SERVER_HEADER..packet.bytes.len() - HOP_TAG_LEN];
                payload[0] == PACK_EVENT && pack::decode_pack_v2(payload).unwrap().contains(&lost)
            }),
            "disk replay retains exactly the lost canonical marker bytes"
        );
        let mut markers = Vec::new();
        let mut inputs = Vec::new();
        for packet in packets {
            for event in received(&mut first, &packet.bytes) {
                match event {
                    OrderEvent::Entry(OrderEntry::Pause { epoch, paused, .. }) => {
                        markers.push((epoch, paused))
                    }
                    OrderEvent::Entry(OrderEntry::Batch { frame, bytes, .. }) => {
                        inputs.push((frame, bytes))
                    }
                    _ => {}
                }
            }
        }
        assert_eq!(markers, [(1, true), (2, false)]);
        assert_eq!(inputs, [(4, b"accepted before pause".to_vec())]);
    }

    /// Compaction spec §6.2 "续期": the relay accepted the renewal's CONFIRM
    /// but its READY never arrived, and the player quits from the old hop.
    /// The old session still takes the LEAVE, answers it, and the stream
    /// records a Leave, not a lost connection.
    #[test]
    fn a_player_quitting_before_its_renewal_ready_still_leaves() {
        let key = SecretKey::new([73; 32]);
        let (mut server, mut first, mut second) = paced_room(&key);
        server.advance_clock(300, unix(300));
        let mut ticket = first.ticket;
        ticket.expires += 50;
        ticket.nonce[15] ^= 0x40;
        let signed = ticket.sign(&key).unwrap();
        let mut candidate = first
            .renewal(
                &signed,
                *derive_packet_key(&key, &signed).expose_for_https(),
                [89; 32],
            )
            .unwrap();
        let mut bytes = Vec::new();
        candidate.probe(&mut bytes).unwrap();
        let challenge = server
            .process_at(endpoint(&first), &bytes, unix(300), 300)
            .unwrap()
            .remove(0);
        let ServerEvent::Challenge(cookie) = candidate.receive(&challenge.bytes).unwrap() else {
            panic!("challenge");
        };
        candidate.confirm(cookie, &mut bytes).unwrap();
        // The relay takes the renewal; its READY is lost.
        server
            .process_at(endpoint(&first), &bytes, unix(300), 300)
            .unwrap();
        first.leave_ordering(&mut bytes).unwrap();
        for _ in 0..2 {
            let ack = server
                .process_at(endpoint(&first), &bytes, unix(300), 300)
                .unwrap();
            assert!(matches!(
                first.receive(&ack[0].bytes),
                Ok(ServerEvent::Ordering(OrderEvent::Left))
            ));
        }
        server.advance_clock(700, unix(700));
        let departures: Vec<_> = pull(&mut server, &mut second, 1, 700)
            .into_iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::Departure { source, lost, .. }) => {
                    Some((source, lost))
                }
                _ => None,
            })
            .collect();
        assert_eq!(departures, [(0, false)]);
    }

    #[test]
    fn last_explicit_leave_resumes_the_paused_canonical_departure_tail() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1400);
        for peer in [&mut first, &mut second] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
            pause_control(&mut server, peer, 0, 0, false, 0);
        }
        pause_control(&mut server, &mut first, 1, 0, true, 200);
        assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().head, 0);
        let mut bytes = Vec::new();
        first.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut first, &bytes, 500);
        server.advance_clock(600, unix(600));
        assert!(
            server.rooms[&[5; 16]]
                .ordering
                .as_ref()
                .unwrap()
                .paused_at
                .is_some()
        );
        second.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut second, &bytes, 600);
        server.advance_clock(700, unix(700));
        // The hop that left takes only LEAVE (compaction spec §6.2).
        renew(&mut server, &mut first, 700);
        let events = pull(&mut server, &mut first, 1, 700);
        let mut resumed = false;
        let mut departed = Vec::new();
        for event in events {
            match event {
                OrderEvent::Entry(OrderEntry::Pause {
                    epoch: 2,
                    paused: false,
                    source: 1,
                    frame: 0,
                    ..
                }) => resumed = true,
                OrderEvent::Entry(OrderEntry::Departure {
                    source, frame: 1, ..
                }) => departed.push(source),
                _ => {}
            }
        }
        assert!(
            resumed,
            "the just-departing source remains a valid original roster identity"
        );
        assert_eq!(departed, [0, 1]);
        let ordering = server.rooms[&[5; 16]].ordering.as_ref().unwrap();
        assert_eq!(ordering.head, 1);
        assert!(ordering.finished_at.is_some());
    }

    #[test]
    fn opted_in_push_follows_the_clock_and_keeps_legacy_pull_unchanged() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut pushed, mut legacy) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1400);
        for peer in [&mut pushed, &mut legacy] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
        }
        // The UPLINK subscribes without any reply (spec §3.4).
        assert!(subscribe(&mut server, &mut pushed, 1, 0).is_empty());
        server.advance_clock(350, unix(350));
        let packets = server.drain_ordering_push(350, unix(350));
        assert!(!packets.is_empty());
        assert!(
            packets
                .iter()
                .all(|packet| packet.endpoint == endpoint(&pushed))
        );
        let mut pushed_ends = Vec::new();
        let mut started = false;
        for packet in packets {
            for event in received(&mut pushed, &packet.bytes) {
                match event {
                    OrderEvent::Status(status) => started |= status.started,
                    OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame }) => {
                        pushed_ends.push((sequence, frame))
                    }
                    _ => {}
                }
            }
        }
        assert!(
            started,
            "the startup transition does not wait for a client Pull"
        );
        let legacy_ends: Vec<_> = pull(&mut server, &mut legacy, 1, 350)
            .into_iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame }) => {
                    Some((sequence, frame))
                }
                _ => None,
            })
            .collect();
        assert_eq!(pushed_ends, legacy_ends);
        assert_eq!(pushed_ends.last(), Some(&(6, 6)));
        // Status still describes a missing tail while delivery credit is idle.
        let heartbeat = server.drain_ordering_push(600, unix(600));
        assert!(heartbeat.into_iter().any(|packet| matches!(
            pushed.receive(&packet.bytes),
            Ok(ServerEvent::Ordering(OrderEvent::Status(_)))
        )));
        // A reordered older UPLINK cannot move the watermark back.
        let (mut older, mut newer) = (Vec::new(), Vec::new());
        pushed.ordering.as_mut().unwrap().link.schedule.recv_next = 3;
        pushed.test_uplink(None, 601, &mut older).unwrap();
        pushed.ordering.as_mut().unwrap().link.schedule.recv_next = 7;
        pushed.test_uplink(None, 601, &mut newer).unwrap();
        events(&mut server, &mut pushed, &newer, 601);
        events(&mut server, &mut pushed, &older, 601);
        let stats = server.reader_link_stats([5; 16], 0).unwrap();
        assert_eq!((stats.acked_next, stats.sent_next), (7, 7));
        server.advance_clock(3601, unix(3601));
        assert!(
            server.drain_ordering_push(3601, unix(3601)).is_empty(),
            "a paused reader's subscription expires without retiring its source"
        );
        assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().active, 3);
        server.advance_clock(120_000, unix(120_000));
        let cursor = server.rooms[&[5; 16]].ordering.as_ref().unwrap().next_seq;
        subscribe(&mut server, &mut pushed, cursor, 120_000);
        assert!(
            !server
                .drain_ordering_push(120_250, unix(120_250))
                .is_empty(),
            "the next UPLINK subscribes again"
        );
    }

    /// A new process meets the batches of the one it replaces in the stream.
    /// As receipts of its own they would be refused, and its cursor stay on the
    /// first of them for good. tests/real/rejoin.mjs in the lobby plays this.
    #[test]
    fn a_replacing_process_takes_its_batch_counter_from_the_stream() {
        let key = SecretKey::new([74; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let mut back = client(&key, 0);
        connect(&mut server, &mut back, 0);
        configure(&mut server, &mut back, &certificate(&key, 3, 1400), 0);
        let ordering = back.ordering.as_mut().unwrap();
        assert!(matches!(
            ordering.accept_through(5),
            Err(RelayError::Authentication)
        ));

        back.set_ordering_restore(true);
        back.ordering.as_mut().unwrap().accept_through(5).unwrap();
        back.set_ordering_restore(false);

        let mut bytes = Vec::new();
        assert!(matches!(
            back.test_submit(5, b"late", &mut bytes),
            Err(RelayError::Replay)
        ));
        back.test_submit(6, b"next", &mut bytes).unwrap();
        // Once restored, a batch this process did not send is refused again.
        assert!(matches!(
            back.ordering.as_mut().unwrap().accept_through(7),
            Err(RelayError::Authentication)
        ));
    }

    /// Credit counts entries and caps count datagrams: one pack carries a
    /// reader's whole credit of frame ends.
    #[test]
    fn push_credit_counts_entries_and_existing_pull_repairs_a_lost_pack() {
        use std::collections::BTreeMap;
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let mut clients: Vec<_> = (0..6).map(|source| client(&key, source)).collect();
        let cert = certificate(&key, 63, 1400);
        for peer in &mut clients {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
            subscribe(&mut server, peer, 1, 0);
        }
        // 81 frame ends.
        server.advance_clock(2000, unix(2000));
        // Heard within the 1500 ms offline limit (spec §5.1).
        for peer in &mut clients {
            subscribe(&mut server, peer, 1, 1999);
        }
        let mut lost = false;
        for _ in 0..20 {
            let mut per_peer = BTreeMap::new();
            for packet in server.drain_ordering_push(2000, unix(2000)) {
                let index = clients
                    .iter()
                    .position(|peer| endpoint(peer) == packet.endpoint)
                    .unwrap();
                *per_peer.entry(index).or_insert(0) += 1;
                if packet.bytes[SERVER_HEADER] == PACK_EVENT {
                    if index == 0 && !lost {
                        lost = true;
                        continue; // a real missing authenticated datagram
                    }
                }
                received(&mut clients[index], &packet.bytes);
            }
            assert!(
                per_peer.values().all(|count| *count <= 3),
                "a status, a pack and a tail probe"
            );
        }
        assert!(lost);
        for source in 0..6 {
            assert_eq!(
                server.reader_link_stats([5; 16], source).unwrap().sent_next,
                65,
                "unacknowledged readers cannot exceed their individual credit"
            );
        }
        // A healthy reader continues without granting the stalled readers credit.
        subscribe(&mut server, &mut clients[1], 65, 2001);
        let next = server.drain_ordering_push(2001, unix(2001));
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].endpoint, endpoint(&clients[1]));
        let sequences: Vec<_> = received(&mut clients[1], &next[0].bytes)
            .into_iter()
            .map(|event| match event {
                OrderEvent::Entry(OrderEntry::FrameEnd { sequence, .. }) => sequence,
                _ => panic!("a frame end"),
            })
            .collect();
        assert_eq!(sequences, (65..=81).collect::<Vec<_>>());
        assert!(
            pull(&mut server, &mut clients[0], 1, 2002)
                .iter()
                .any(|event| matches!(
                    event,
                    OrderEvent::Entry(OrderEntry::FrameEnd {
                        sequence: 1,
                        frame: 1
                    })
                )),
            "the unchanged bounded Pull recovers the lost pack"
        );
    }

    /// Spec §7.5: only a lossy reader gets the 8 ms repeat, once.
    #[test]
    fn push_repeats_unacknowledged_entries_once_8_ms_on_only_while_lossy() {
        fn frame_ends(reader: &mut ClientSession, packets: Vec<ForwardDatagram>) -> Vec<u64> {
            packets
                .into_iter()
                .flat_map(|packet| received(reader, &packet.bytes))
                .filter_map(|event| match event {
                    OrderEvent::Entry(OrderEntry::FrameEnd { sequence, .. }) => Some(sequence),
                    _ => None,
                })
                .collect()
        }
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut lossy, mut good) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1400);
        for peer in [&mut lossy, &mut good] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
            subscribe(&mut server, peer, 1, 0);
        }
        let peers = &mut server.rooms.get_mut(&[5; 16]).unwrap().peers;
        peers[0].link.loss.lossy = true;
        peers[1].link.loss = crate::link::LossStats::measured_good();
        server.advance_clock(300, unix(300));
        let first = server.drain_ordering_push(300, unix(300));
        let (lossy_at, good_at) = (endpoint(&lossy), endpoint(&good));
        let to = |at: SocketAddrV4, packets: &[ForwardDatagram]| -> Vec<ForwardDatagram> {
            packets
                .iter()
                .filter(|packet| packet.endpoint == at)
                .map(|packet| ForwardDatagram {
                    endpoint: packet.endpoint,
                    bytes: packet.bytes.clone(),
                })
                .collect()
        };
        assert_eq!(frame_ends(&mut lossy, to(lossy_at, &first)), [1, 2, 3]);
        assert_eq!(frame_ends(&mut good, to(good_at, &first)), [1, 2, 3]);
        assert!(server.drain_ordering_push(307, unix(307)).is_empty());
        let repeat = server.drain_ordering_push(308, unix(308));
        assert!(
            to(good_at, &repeat).is_empty(),
            "a good reader gets no repeat"
        );
        assert_eq!(frame_ends(&mut lossy, to(lossy_at, &repeat)), [1, 2, 3]);
        assert!(
            server.drain_ordering_push(330, unix(330)).is_empty(),
            "each entry is repeated once"
        );
        let stats = server.reader_link_stats([5; 16], 0).unwrap();
        assert_eq!(stats.packets_by_kind[2], 1);
        assert_eq!(stats.piggyback_depth, 2);
    }

    #[test]
    fn repeats_take_only_what_fresh_pushes_leave() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let mut readers: Vec<_> = (0..5).map(|source| client(&key, source)).collect();
        let cert = certificate(&key, 31, 1400);
        for reader in &mut readers {
            connect(&mut server, reader, 0);
            configure(&mut server, reader, &cert, 0);
            subscribe(&mut server, reader, 1, 0);
        }
        // Only lossy readers get repeats (spec §7.5).
        for peer in &mut server.rooms.get_mut(&[5; 16]).unwrap().peers {
            peer.link.loss.lossy = true;
        }
        // Seven full batches in frame 1: each fills a pack of its own.
        let mut bytes = Vec::new();
        for (source, id) in [(0, 1), (1, 1), (2, 1), (3, 1), (4, 1), (0, 2), (1, 2)] {
            readers[source]
                .test_submit(id, &[source as u8; ORDERING_BATCH_CAPACITY], &mut bytes)
                .unwrap();
            events(&mut server, &mut readers[source], &bytes, 0);
        }
        // Every reader holds frames 1-6 in seven packs, due for a repeat when
        // the next turn commits. Its fresh pack plus seven repeats fill one
        // reader's pass share, so four readers could otherwise take the
        // room's whole pass.
        server.advance_clock(360, unix(360));
        while !server.drain_ordering_push(360, unix(360)).is_empty() {}
        server.advance_clock(400, unix(400));
        let spent = server.rooms[&[5; 16]].order_packets.spent;
        let packets = server.drain_ordering_push(400, unix(400));
        assert_eq!(packets.len(), PUSH_ROOM_PACKETS);
        let fresh = packets
            .iter()
            .filter(|packet| {
                let reader = readers
                    .iter_mut()
                    .find(|reader| endpoint(reader) == packet.endpoint)
                    .unwrap();
                received(reader, &packet.bytes).iter().any(|event| {
                    matches!(
                        event,
                        OrderEvent::Entry(OrderEntry::FrameEnd { frame: 7, .. })
                    )
                })
            })
            .count();
        assert_eq!(
            fresh,
            readers.len(),
            "repeats never take a fresh entry's place"
        );
        assert_eq!(
            server.rooms[&[5; 16]].order_packets.spent - spent,
            5,
            "repeats spend no fresh budget"
        );
    }

    #[test]
    fn one_room_may_spend_past_half_the_relay_ordering_output() {
        fn spend(server: &mut RelayServer, packets: u64) {
            let room = server.rooms.get_mut(&[5; 16]).unwrap();
            assert!(server.order_packets.admit_part(
                &mut room.order_packets,
                unix(0),
                packets,
                INGRESS_PER_SECOND
            ));
            server
                .order_packets
                .charge_part(&mut room.order_packets, packets);
        }
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut reader, mut other) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1400);
        for peer in [&mut reader, &mut other] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
        }
        // A busy room may push more than half the total.
        spend(&mut server, INGRESS_PER_SECOND / 2);
        let mut bytes = Vec::new();
        reader.pull_ordering(1, &mut bytes).unwrap();
        assert!(
            server
                .process_at(endpoint(&reader), &bytes, unix(0), 0)
                .is_ok()
        );
        let left = ROOM_ORDER_PACKETS - server.rooms[&[5; 16]].order_packets.spent;
        spend(&mut server, left);
        reader.pull_ordering(1, &mut bytes).unwrap();
        assert!(
            matches!(
                server.process_at(endpoint(&reader), &bytes, unix(0), 0),
                Err(RelayError::RateLimited)
            ),
            "a room leaves the rest of the total to other matches"
        );
    }

    #[test]
    fn renewed_push_replays_journal_batches_and_follows_a_rebound_hop() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut reader, mut sender) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1500);
        for peer in [&mut reader, &mut sender] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
        }
        subscribe(&mut server, &mut reader, 1, 0);
        let mut bytes = Vec::new();
        sender
            .test_submit(1, b"immutable pushed input", &mut bytes)
            .unwrap();
        events(&mut server, &mut sender, &bytes, 0);
        server.advance_clock(350, unix(350));
        let mut canonical = None;
        // The sender's UPLINK subscribed it too.
        let reader_endpoint = endpoint(&reader);
        for packet in server
            .drain_ordering_push(350, unix(350))
            .into_iter()
            .filter(|packet| packet.endpoint == reader_endpoint)
        {
            let payload = &packet.bytes[SERVER_HEADER..packet.bytes.len() - HOP_TAG_LEN];
            if payload[0] == PACK_EVENT {
                canonical = canonical.or(pack::decode_pack_v2(payload)
                    .unwrap()
                    .into_iter()
                    .find(|entry| entry[0] == BATCH_EVENT));
            }
            for event in received(&mut reader, &packet.bytes) {
                if let OrderEvent::Entry(OrderEntry::Batch { bytes, .. }) = event {
                    assert_eq!(bytes, b"immutable pushed input");
                }
            }
        }
        let canonical = canonical.expect("the original batch was pushed");
        server.advance_clock(250_000, unix(250_000));
        assert!(
            server.rooms[&[5; 16]]
                .ordering
                .as_ref()
                .unwrap()
                .hot_oldest_seq()
                > 1
        );
        renew(&mut server, &mut reader, 250_000);
        configure(&mut server, &mut reader, &cert, 250_000);
        assert!(
            server
                .drain_ordering_push(250_000, unix(250_000))
                .is_empty(),
            "new hop keys do not inherit an old subscription"
        );
        subscribe(&mut server, &mut reader, 1, 250_000);
        let replay = server.drain_ordering_push(250_000, unix(250_000));
        assert!(replay.iter().any(|packet| {
            let payload = &packet.bytes[SERVER_HEADER..packet.bytes.len() - HOP_TAG_LEN];
            payload[0] == PACK_EVENT && pack::decode_pack_v2(payload).unwrap().contains(&canonical)
        }));
        for packet in replay {
            reader.receive(&packet.bytes).unwrap();
        }

        // An authenticated datagram from a new address moves the hop
        // (compaction spec §6.2 "换地址").
        let rebound = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 32000);
        reader.probe(&mut bytes).unwrap();
        let ready = server
            .process_at(rebound, &bytes, unix(250_001), 250_001)
            .unwrap()
            .remove(0);
        assert_eq!(ready.endpoint, rebound);
        reader.receive(&ready.bytes).unwrap();
        reader.ordering.as_mut().unwrap().link.schedule.recv_next = 9;
        reader.test_uplink(None, 250_001, &mut bytes).unwrap();
        for packet in server
            .process_at(rebound, &bytes, unix(250_001), 250_001)
            .unwrap()
        {
            reader.receive(&packet.bytes).unwrap();
        }
        let rebound_packets = server.drain_ordering_push(250_001, unix(250_001));
        assert!(!rebound_packets.is_empty());
        assert!(
            rebound_packets
                .iter()
                .all(|packet| packet.endpoint == rebound)
        );
        // The reader acknowledges the credit pushed, so the next pass reads disk.
        reader.ordering.as_mut().unwrap().link.schedule.recv_next = 73;
        reader.test_uplink(None, 250_001, &mut bytes).unwrap();
        server
            .process_at(rebound, &bytes, unix(250_001), 250_001)
            .unwrap();
        // Disk failure keeps the archive and becomes the existing terminal error.
        std::fs::OpenOptions::new()
            .write(true)
            .open(directory.0.join("05".repeat(16)).join("stream.index"))
            .unwrap()
            .set_len(0)
            .unwrap();
        let failure = server.drain_ordering_push(250_002, unix(250_002));
        assert!(failure.into_iter().any(|packet| matches!(
            reader.receive(&packet.bytes),
            Ok(ServerEvent::Ordering(OrderEvent::Error {
                code: OrderError::StorageUnavailable,
                ..
            }))
        )));
        assert!(
            server
                .drain_ordering_push(250_003, unix(250_003))
                .is_empty()
        );
        assert!(
            directory
                .0
                .join("05".repeat(16))
                .join("stream.data")
                .exists()
        );
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
        // expire. Silence shorter than the absence limit removes neither player.
        server.advance_clock(350_000, unix(350_000));
        assert_eq!(server.rooms[&[5; 16]].ordering.as_ref().unwrap().active, 3);
        let mut forged = leave.clone();
        *forged.last_mut().unwrap() ^= 1;
        assert!(matches!(
            server.process_at(endpoint(&first), &forged, unix(350_000), 350_000),
            Err(RelayError::Authentication)
        ));
        // Past its ticket the hop takes only LEAVE (compaction spec §6.2).
        let mut bytes = Vec::new();
        first.test_submit(1, b"expired input", &mut bytes).unwrap();
        assert!(matches!(
            server.process_at(endpoint(&first), &bytes, unix(350_000), 350_000),
            Err(RelayError::Unregistered)
        ));
        first.probe(&mut bytes).unwrap();
        assert!(matches!(
            server.process_at(endpoint(&first), &bytes, unix(350_000), 350_000),
            Err(RelayError::Unregistered)
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
        // An expired ticket's replays stop at the player's own budget instead
        // of spending the relay-wide order budget everyone's input needs.
        for _ in 1..PLAYER_HANDSHAKES_PER_SECOND {
            assert!(
                server
                    .process_at(endpoint(&first), &leave, unix(652_000), 652_000)
                    .is_ok()
            );
        }
        assert_eq!(
            server
                .process_at(endpoint(&first), &leave, unix(652_000), 652_000)
                .err(),
            Some(RelayError::RateLimited)
        );
    }

    #[test]
    fn cancelled_start_reclaims_the_empty_archive_without_reopening_stale_tickets() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 16, &directory.0).unwrap();
        // Nine quick cancellations each free their archive reservation
        // without waiting for any tombstone's short-ticket grace to elapse.
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
                pacing: Default::default(),
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
            // The hop that left takes only LEAVE (compaction spec §6.2).
            renew(&mut server, &mut participant, now_ms);
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
        // An HTTP cancellation can lose its final UDP Leave. A newer admission
        // of the same player cancels the unstarted room until the loading
        // timeout starts it without the peer that never arrived.
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
        let superseded = 10_000 + LOADING_SILENCE_MS - 1;
        server.advance_clock(superseded, unix(superseded));
        assert!(abandoned_path.exists());
        ticket.room = [11; 16];
        ticket.generation = 11;
        ticket.expires = unix(superseded) + 200;
        let signed = ticket.sign(&key).unwrap();
        let mut next = ClientSession::new(
            &signed,
            *derive_packet_key(&key, &signed).expose_for_https(),
            [89; 32],
        )
        .unwrap();
        connect(&mut server, &mut next, superseded);
        assert!(
            !abandoned_path.exists(),
            "new admission reclaims the lost Leave"
        );
        let mut bytes = Vec::new();
        abandoned
            .configure_ordering(&abandoned_cert, [7; 32], &mut bytes)
            .unwrap();
        // The newer generation released the old hop's session (compaction
        // spec §6.2).
        assert!(matches!(
            server.process_at(endpoint(&abandoned), &bytes, unix(superseded), superseded),
            Err(RelayError::Unregistered)
        ));
        assert!(!abandoned_path.exists());
        cert.room = ticket.room;
        assert!(
            configure(
                &mut server,
                &mut next,
                &cert.sign(&key).unwrap(),
                superseded
            )
            .iter()
            .any(|event| matches!(event, OrderEvent::Status(_)))
        );
        next.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut next, &bytes, superseded);
        let finished = superseded + MAX_TICKET_LIFETIME * 1000;
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
        third.test_submit(1, b"third input", &mut bytes).unwrap();
        events(&mut server, &mut third, &bytes, 1000);
        active.test_submit(1, b"active input", &mut bytes).unwrap();
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
                }) => Some((*sequence, *frame, *source, bytes.as_slice())),
                _ => None,
            })
            .collect();
        assert_eq!(
            batch,
            [
                (1, 1, 1, b"active input".as_slice()),
                (2, 1, 2, b"third input".as_slice())
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
        // 100 ms at 45 Hz: one 3-frame turn.
        assert_eq!(ends, [(3, 1), (4, 2), (5, 3)]);
        active
            .test_submit(2, b"healthy input during outage", &mut bytes)
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
            .test_submit(3, b"healthy input after lease renewal", &mut bytes)
            .unwrap();
        let acked = events(&mut server, &mut active, &bytes, 250_000);
        assert!(matches!(acked[0], OrderEvent::Ack { batch_id: 3, frame, .. } if frame > 4096));
        // The host returns after >300 seconds, retains its original principal,
        // and starts from sequence1 even though the hot cache evicted it.
        server.advance_clock(350_000, unix(350_000));
        host.probe(&mut bytes).unwrap();
        assert!(matches!(
            server.process_at(endpoint(&host), &bytes, unix(350_000), 350_000),
            Err(RelayError::Unregistered)
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
                    .test_submit(4, b"healthy input while host catches up", &mut bytes)
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
                        let bytes = bytes.as_slice();
                        saw_old_input |= bytes == b"active input";
                        saw_new_input |= bytes == b"healthy input after lease renewal";
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
        host.test_submit(1, b"recovered host input", &mut bytes)
            .unwrap();
        assert!(
            matches!(events(&mut server, &mut host, &bytes, now_ms)[0], OrderEvent::Ack { batch_id: 1, frame, .. } if frame > last_frame)
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
            .test_submit(1, b"private ordered command", &mut bytes)
            .unwrap();
        // Plaintext (compaction spec §3.1).
        let opaque = uplink_batches(&bytes).remove(0);
        assert_eq!(opaque, b"private ordered command");
        let accepted = server
            .process_at(endpoint(&first), &bytes, 1000, 0)
            .unwrap();
        assert_eq!(accepted.len(), 1); // deliberately lose the ACK
        first
            .test_submit(1, b"private ordered command", &mut bytes)
            .unwrap();
        assert_eq!(uplink_batches(&bytes), [opaque.clone()]);
        let duplicate = server
            .process_at(endpoint(&first), &bytes, 1000, 0)
            .unwrap();
        assert_eq!(duplicate.len(), 1);
        let mut modified = opaque.clone();
        *modified.last_mut().unwrap() ^= 1;
        first
            .encode(
                CONTROL,
                &raw_uplink(crate::link::FLAG_PUSH, 1, &[&modified]),
                &mut bytes,
            )
            .unwrap();
        assert!(matches!(
            events(&mut server, &mut first, &bytes, 0)[0],
            OrderEvent::Error {
                code: OrderError::ModifiedRetry,
                ..
            }
        ));
        first
            .encode(
                CONTROL,
                &raw_uplink(crate::link::FLAG_PUSH, 3, &[&modified]),
                &mut bytes,
            )
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
            .test_submit(1, b"private ordered command", &mut bytes)
            .unwrap();
        assert_eq!(uplink_batches(&bytes), [opaque]);
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
        first.test_submit(2, b"next batch", &mut bytes).unwrap();
        assert!(matches!(
            events(&mut server, &mut first, &bytes, 300)[0],
            OrderEvent::Ack {
                batch_id: 2,
                frame: 4,
                ..
            }
        ));
        first.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut first, &bytes, 300);
        server.advance_clock(400, 1000);
        let tail = pull(&mut server, &mut second, 5, 400);
        let at_four: Vec<_> = tail
            .iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::Batch { frame: 4, .. }) => Some("batch"),
                OrderEvent::Entry(OrderEntry::Departure {
                    frame: 4,
                    source: 0,
                    ..
                }) => Some("departure"),
                _ => None,
            })
            .collect();
        assert_eq!(at_four, ["batch", "departure"]);
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

    /// Spec §8: each row of the batch table, and the reply rules.
    #[test]
    fn uplink_batches_follow_the_relay_table_and_duplicates_still_get_an_ack() {
        let key = SecretKey::new([79; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1300);
        for peer in [&mut first, &mut second] {
            connect(&mut server, peer, 0);
        }
        configure(&mut server, &mut first, &cert, 0);
        for (id, plaintext) in [(1, b"one"), (2, b"two"), (3, b"thr")] {
            first.seal_batch(id, plaintext).unwrap();
        }
        let batch = |client: &ClientSession, id: u64| {
            let ordering = client.ordering.as_ref().unwrap();
            ordering
                .sealed
                .iter()
                .find(|batch| batch.id == id)
                .unwrap()
                .opaque
                .clone()
        };
        let (one, two, three) = (batch(&first, 1), batch(&first, 2), batch(&first, 3));
        fn send(
            server: &mut RelayServer,
            client: &mut ClientSession,
            flags: u8,
            first_id: u64,
            batches: &[&[u8]],
            now_ms: u64,
        ) -> Vec<OrderEvent> {
            let mut bytes = Vec::new();
            client
                .encode(CONTROL, &raw_uplink(flags, first_id, batches), &mut bytes)
                .unwrap();
            events(server, client, &bytes, now_ms)
        }
        let push = crate::link::FLAG_PUSH;
        let error = |events: &[OrderEvent]| match events {
            [OrderEvent::Error { code, expected }] => Some((*code, *expected)),
            _ => None,
        };
        let acked = |events: &[OrderEvent]| match events {
            [OrderEvent::Ack { batch_id, .. }] => Some(*batch_id),
            _ => None,
        };
        // Not all ready: no start is scheduled.
        let early = send(&mut server, &mut first, push, 1, &[&one], 0);
        assert_eq!(error(&early), Some((OrderError::NotStarted, 0)));
        configure(&mut server, &mut second, &cert, 0);
        assert!(
            send(&mut server, &mut first, push, 1, &[], 300).is_empty(),
            "no batch, no reply"
        );
        assert_eq!(
            acked(&send(&mut server, &mut first, push, 1, &[&one], 300)),
            Some(1)
        );
        assert_eq!(
            acked(&send(&mut server, &mut first, push, 1, &[&one], 300)),
            Some(1)
        );
        let changed = [&two[..]];
        assert_eq!(
            error(&send(&mut server, &mut first, push, 1, &changed, 300)),
            Some((OrderError::ModifiedRetry, 1))
        );
        assert_eq!(
            error(&send(&mut server, &mut first, push, 3, &[&three], 300)),
            Some((OrderError::BatchGap, 2))
        );
        // A duplicate prefix is skipped and the rest accepted in order.
        assert_eq!(
            acked(&send(
                &mut server,
                &mut first,
                push,
                1,
                &[&one, &two, &three],
                300
            )),
            Some(3)
        );
        assert_eq!(
            acked(&send(&mut server, &mut first, push, 1, &[&one], 300)),
            Some(3)
        );
        let probe = send(
            &mut server,
            &mut first,
            push | crate::link::FLAG_PROBE,
            1,
            &[],
            300,
        );
        assert!(matches!(probe[..], [OrderEvent::Status(_)]));
        let mut bytes = Vec::new();
        second.seal_batch(1, b"gone").unwrap();
        let gone = batch(&second, 1);
        second.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut second, &bytes, 300);
        // The hop that left takes only LEAVE (compaction spec §6.2).
        second
            .encode(CONTROL, &raw_uplink(push, 1, &[&gone]), &mut bytes)
            .unwrap();
        assert_eq!(
            server
                .process_at(endpoint(&second), &bytes, unix(300), 300)
                .err(),
            Some(RelayError::Unregistered)
        );
        renew(&mut server, &mut second, 300);
        assert_eq!(
            error(&send(&mut server, &mut second, push, 1, &[&gone], 300)),
            Some((OrderError::Inactive, 0))
        );
    }

    #[test]
    fn pipelined_batches_recover_reordering_and_lost_ack_without_duplicate_entries() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut sender, mut peer) = (client(&key, 0), client(&key, 1));
        for client in [&mut sender, &mut peer] {
            connect(&mut server, client, 0);
        }
        let cert = certificate(&key, 3, 1300);
        configure(&mut server, &mut sender, &cert, 0);
        configure(&mut server, &mut peer, &cert, 0);
        let mut bytes = Vec::new();
        sender.test_submit(1, b"checksum", &mut bytes).unwrap();
        sender.test_submit(2, b"move", &mut bytes).unwrap();
        assert!(matches!(
            events(&mut server, &mut sender, &bytes, 0)[0],
            OrderEvent::Error {
                code: OrderError::BatchGap,
                expected: 1
            }
        ));
        sender.test_submit(1, b"checksum", &mut bytes).unwrap();
        let lost_ack = server
            .process_at(endpoint(&sender), &bytes, 1000, 0)
            .unwrap();
        sender.test_submit(2, b"move", &mut bytes).unwrap();
        assert!(matches!(
            events(&mut server, &mut sender, &bytes, 0)[0],
            OrderEvent::Ack { batch_id: 2, .. }
        ));
        // A delayed ACK cannot move the accepted prefix backwards.
        sender.receive(&lost_ack[0].bytes).unwrap();
        assert_eq!(sender.ordering.as_ref().unwrap().accepted_through, 2);
        // An older batch is skipped and its UPLINK still gets the ACK (§8).
        sender.test_submit(1, b"checksum", &mut bytes).unwrap();
        assert!(matches!(
            events(&mut server, &mut sender, &bytes, 0)[0],
            OrderEvent::Ack { batch_id: 2, .. }
        ));
        server.advance_clock(400, 1000);
        let batches: Vec<_> = pull(&mut server, &mut peer, 1, 400)
            .into_iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::Batch {
                    batch_id, bytes, ..
                }) => Some((batch_id, bytes.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(batches, [(1, b"checksum".to_vec()), (2, b"move".to_vec())]);
    }

    #[test]
    fn full_submission_window_survives_renewal_with_immutable_retry_bytes() {
        let key = SecretKey::new([74; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut sender, mut peer) = (client(&key, 0), client(&key, 1));
        for client in [&mut sender, &mut peer] {
            connect(&mut server, client, 0);
        }
        let cert = certificate(&key, 3, 1300);
        configure(&mut server, &mut sender, &cert, 0);
        configure(&mut server, &mut peer, &cert, 0);
        let mut bytes = Vec::new();
        let mut sealed = Vec::new();
        for id in 1..=4 {
            sender.test_submit(id, &[id as u8], &mut bytes).unwrap();
            sealed.push(uplink_batches(&bytes).remove(0));
        }
        assert_eq!(
            sender.test_submit(5, b"fifth", &mut bytes),
            Err(RelayError::Full)
        );
        assert_eq!(
            sender.test_submit(2, b"changed", &mut bytes),
            Err(RelayError::Authentication)
        );
        renew(&mut server, &mut sender, 1000);
        let mut final_ack = None;
        for id in 1..=4 {
            sender.test_submit(id, &[id as u8], &mut bytes).unwrap();
            assert_eq!(uplink_batches(&bytes), [sealed[(id - 1) as usize].clone()]);
            final_ack = Some(
                server
                    .process_at(endpoint(&sender), &bytes, 1001, 1000)
                    .unwrap(),
            );
        }
        // Only the final receipt arrives; strict server admission proves all
        // four predecessors, and one new slot becomes available immediately.
        sender.receive(&final_ack.unwrap()[0].bytes).unwrap();
        sender.test_submit(5, b"fifth", &mut bytes).unwrap();
        assert!(matches!(
            events(&mut server, &mut sender, &bytes, 1000)[0],
            OrderEvent::Ack { batch_id: 5, .. }
        ));
        assert_eq!(
            sender.test_submit(1, b"new old nonce", &mut bytes),
            Err(RelayError::Replay)
        );
        assert_eq!(sender.ordering.as_ref().unwrap().sealed.len(), 4);
    }

    #[test]
    fn archive_quota_admits_rooms_by_reserved_bytes() {
        let directory = TestDirectory::new();
        crate::journal::JournalDirectory::open(&directory.0, std::time::SystemTime::now()).unwrap();
        // A journal left by an earlier process holds only its size on disk.
        let leftover = directory.0.join("0b".repeat(16));
        std::fs::create_dir(&leftover).unwrap();
        std::fs::write(leftover.join("stream.data"), [0; 64]).unwrap();
        let mut journals =
            crate::journal::JournalDirectory::open(&directory.0, std::time::SystemTime::now())
                .unwrap();
        assert_eq!(
            journals.create([11; 16], 45, 2).err().unwrap().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        let mut admitted = Vec::new();
        let full = loop {
            match journals.create([0x80 + admitted.len() as u8; 16], 45, 2) {
                Ok(journal) => admitted.push(journal),
                Err(error) => break error,
            }
        };
        assert_eq!(full.kind(), std::io::ErrorKind::StorageFull);
        // Each two-player 45 FPS room reserves its first hour of clock
        // records and 32 MiB of commands per player, and leaves its next
        // hour free: not a whole day of clock or a fixed eighth of the quota.
        assert_eq!(admitted.len(), 114);
        // A finished room keeps only the bytes it wrote.
        journals.settle(&mut admitted[0], 0);
        assert!(journals.create([0x70; 16], 45, 2).is_ok());
    }

    /// A journaled two-player 10 FPS match, started at 200 ms.
    fn slow_match(directory: &TestDirectory) -> RelayServer {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let cert = OrderingCertificate {
            room: [5; 16],
            sources_mask: 3,
            tick_hz: 10,
            expires: 2000,
            pacing: Default::default(),
        }
        .sign(&key)
        .unwrap();
        for source in 0..2 {
            let mut player = client(&key, source);
            connect(&mut server, &mut player, 0);
            configure(&mut server, &mut player, &cert, 0);
        }
        server
    }

    /// Runs the match five minutes at a time, both players in contact.
    fn play(server: &mut RelayServer, from_ms: u64, until_ms: u64) {
        for now_ms in (from_ms..=until_ms).step_by(300_000) {
            let room = server.rooms.get_mut(&[5; 16]).unwrap();
            for source in 0..2 {
                let ordering = room.ordering.as_mut().unwrap();
                ordering.heard(source, ([source + 1; 16], 1), now_ms);
            }
            server.advance_clock(now_ms, unix(now_ms));
        }
    }

    #[test]
    fn a_journal_reserves_the_next_hour_of_clock_as_the_match_runs() {
        let directory = TestDirectory::new();
        let mut server = slow_match(&directory);
        play(&mut server, 0, 3_900_000);
        assert!(!ordering(&server).storage_failed);
        assert_eq!(ordering(&server).head, 38_998);
    }

    #[test]
    fn a_full_relay_refuses_new_rooms_and_a_running_one_still_takes_its_next_hour() {
        let directory = TestDirectory::new();
        let mut server = slow_match(&directory);
        // Other rooms fill the relay until it refuses the smallest one.
        let journals = server.journal_directory.as_mut().unwrap();
        let mut others = Vec::new();
        for (tick_hz, sources) in [(45, 2), (10, 0)] {
            let refused = loop {
                let mut room = [0x80; 16];
                room[..8].copy_from_slice(&(others.len() as u64).to_le_bytes());
                match journals.create(room, tick_hz, sources) {
                    Ok(journal) => others.push(journal),
                    Err(error) => break error,
                }
            };
            assert_eq!(refused.kind(), std::io::ErrorKind::StorageFull);
        }
        play(&mut server, 0, 3_900_000);
        assert!(!ordering(&server).storage_failed);
        assert_eq!(ordering(&server).head, 38_998);
    }

    #[test]
    fn a_started_match_outlives_its_certificate_until_its_players_go_silent() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1300);
        for peer in [&mut first, &mut second] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
        }
        server.advance_clock(301_000, unix(301_000));
        let archive = directory.0.join("05".repeat(16));
        assert!(archive.exists() && ordering(&server).head > 300 * 45);
        // Silence still ends the abandoned match and frees its archive.
        server.advance_clock(ABSENT_MS, unix(ABSENT_MS));
        let freed = ABSENT_MS + MAX_TICKET_LIFETIME * 1000;
        server.advance_clock(freed, unix(freed));
        assert!(!archive.exists());
    }

    #[test]
    fn flooding_matches_cannot_take_the_ordering_output_another_match_steadily_uses() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 16);
        // Room 1's one steady player waits for a second player, so each Pull
        // returns one status. Rooms 2 and 3 hold four flooders each, and ten
        // idle rooms leave the steady player more than an equal share.
        let mut players = Vec::new();
        let rooms = [(1, 1, 0b11), (2, 4, 0b1111), (3, 4, 0b1111)];
        for (room, sources, sources_mask) in
            rooms.into_iter().chain((4..14).map(|room| (room, 1, 1)))
        {
            let cert = OrderingCertificate {
                room: [room; 16],
                sources_mask,
                tick_hz: 45,
                expires: 1400,
                pacing: Default::default(),
            }
            .sign(&key)
            .unwrap();
            for source in 0..sources {
                let ticket = Ticket {
                    room: [room; 16],
                    source,
                    expires: 1200,
                    nonce: [room; 16],
                    player: [room * 16 + source; 16],
                    generation: 1,
                }
                .sign(&key)
                .unwrap();
                let mut player = ClientSession::new(
                    &ticket,
                    *derive_packet_key(&key, &ticket).expose_for_https(),
                    [89; 32],
                )
                .unwrap();
                connect(&mut server, &mut player, 0);
                if room < 4 {
                    configure(&mut server, &mut player, &cert, 0);
                    players.push(player);
                }
            }
        }
        let pull_at = |server: &mut RelayServer, player: &mut ClientSession, now_ms| {
            let mut bytes = Vec::new();
            player.pull_ordering(1, &mut bytes).unwrap();
            server
                .process_at(endpoint(player), &bytes, unix(now_ms), now_ms)
                .is_ok()
        };
        let (steady, flooders) = players.split_first_mut().unwrap();
        assert!((0..1500).all(|_| pull_at(&mut server, steady, 1000)));
        // The next second, the flooders pull until the relay refuses them.
        for flooder in flooders {
            while pull_at(&mut server, flooder, 2000) {}
        }
        assert!(
            (0..1500).all(|_| pull_at(&mut server, steady, 2000)),
            "the steady match keeps what it used the second before"
        );
    }

    #[test]
    fn leave_refuses_a_ticket_older_than_any_room_it_named() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let mut player = client(&key, 0);
        connect(&mut server, &mut player, 0);
        player
            .configure_ordering(&certificate(&key, 1, 1400), [7; 32], &mut Vec::new())
            .unwrap();
        let mut leave = Vec::new();
        player.leave_ordering(&mut leave).unwrap();
        let last = player.ticket.expires + LEAVE_GRACE - 1;
        assert!(
            server
                .process_at(endpoint(&player), &leave, last, last * 1000)
                .is_ok()
        );
        assert_eq!(
            server
                .process_at(endpoint(&player), &leave, last + 1, (last + 1) * 1000)
                .err(),
            Some(RelayError::Unregistered)
        );
    }

    #[test]
    fn newer_admission_departs_the_player_from_an_older_started_match() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1400);
        for peer in [&mut first, &mut second] {
            connect(&mut server, peer, 0);
            configure(&mut server, peer, &cert, 0);
        }
        server.advance_clock(300, unix(300));
        // The lobby admits the first player to its next match; the old
        // generation can no longer send Leave for itself.
        let mut ticket = first.ticket;
        ticket.room = [6; 16];
        ticket.generation = 2;
        let signed = ticket.sign(&key).unwrap();
        let mut next = ClientSession::new(
            &signed,
            *derive_packet_key(&key, &signed).expose_for_https(),
            [89; 32],
        )
        .unwrap();
        connect(&mut server, &mut next, 300);
        server.advance_clock(400, unix(400));
        let departed: Vec<_> = pull(&mut server, &mut second, 1, 400)
            .into_iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::Departure { source, .. }) => Some(source),
                _ => None,
            })
            .collect();
        assert_eq!(departed, [0]);
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
        let mut forged = cert.clone();
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
            .test_submit(1, b"authenticated plaintext", &mut bytes)
            .unwrap();
        events(&mut server, &mut first, &bytes, 0);
        second.pull_ordering(1, &mut bytes).unwrap();
        let packets = server
            .process_at(endpoint(&second), &bytes, 1000, 300)
            .unwrap();
        // Batches travel plaintext (compaction spec §3): what the relay
        // committed is what every reader gets.
        assert!(packets.iter().any(|packet| {
            matches!(
                received(&mut second, &packet.bytes)[..],
                [OrderEvent::Entry(OrderEntry::Batch { ref bytes, .. }), ..]
                    if bytes == b"authenticated plaintext"
            )
        }));
        server.advance_clock(112_000, unix(112_000));
        let replay = pull(&mut server, &mut second, 1, 112_000);
        assert!(matches!(
            replay[0],
            OrderEvent::Status(OrderStatus { oldest_seq: 1, .. })
        ));
        assert!(matches!(
            replay[1],
            OrderEvent::Entry(OrderEntry::Batch {
                sequence: 1,
                frame: 1,
                ..
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

    #[test]
    fn a_player_silent_for_ten_minutes_departs_through_the_stream() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let (mut present, mut absent) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 2000);
        for participant in [&mut present, &mut absent] {
            connect(&mut server, participant, 0);
            configure(&mut server, participant, &cert, 0);
        }
        // Only one player keeps contact, renewing its short hop ticket.
        let mut bytes = Vec::new();
        for now_ms in (100_000..ABSENT_MS).step_by(100_000) {
            renew(&mut server, &mut present, now_ms);
            present.probe(&mut bytes).unwrap();
            server
                .process_at(endpoint(&present), &bytes, unix(now_ms), now_ms)
                .unwrap();
        }
        server.advance_clock(ABSENT_MS - 1, unix(ABSENT_MS - 1));
        assert_eq!(
            ordering(&server).active,
            3,
            "a shorter outage keeps its place"
        );
        server.advance_clock(ABSENT_MS, unix(ABSENT_MS));
        assert_eq!(ordering(&server).active, 1);
        server.advance_clock(ABSENT_MS + 1000, unix(ABSENT_MS + 1000));
        assert!(
            ordering(&server)
                .history
                .iter()
                .any(|entry| entry.bytes[0] == DEPARTURE_EVENT && entry.bytes[13..] == [1, 1]),
            "source 1 departs with connection lost"
        );
    }

    #[test]
    fn players_who_never_finish_loading_depart_in_the_first_frame() {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::new(key.clone(), 2);
        let mut players: Vec<_> = (0..3).map(|source| client(&key, source)).collect();
        for player in &mut players {
            connect(&mut server, player, 0);
        }
        let cert = certificate(&key, 0b111, 2000);
        // The second player's game crashed while loading, before the first
        // became ready: its silence counts from its last packet. The third
        // still answers but never becomes ready.
        let ready_at = 10_000;
        let mut bytes = Vec::new();
        for now_ms in (ready_at..ready_at + LOADING_LIMIT_MS).step_by(10_000) {
            players[2].probe(&mut bytes).unwrap();
            server
                .process_at(endpoint(&players[2]), &bytes, unix(now_ms), now_ms)
                .unwrap();
            if now_ms == ready_at {
                configure(&mut server, &mut players[0], &cert, now_ms);
            }
            server.advance_clock(now_ms, unix(now_ms));
            let expected = if now_ms < LOADING_SILENCE_MS {
                0b111
            } else {
                0b101
            };
            assert_eq!(ordering(&server).active, expected, "at {now_ms} ms");
            assert!(ordering(&server).starts_at.is_none());
        }
        let limit = ready_at + LOADING_LIMIT_MS;
        server.advance_clock(limit, unix(limit));
        assert_eq!(ordering(&server).active, 1);
        let first = pull(&mut server, &mut players[0], 1, limit + 1000);
        let departed: Vec<_> = first
            .iter()
            .filter_map(|event| match event {
                OrderEvent::Entry(OrderEntry::Departure {
                    frame: 1,
                    source,
                    lost: true,
                    ..
                }) => Some(*source),
                _ => None,
            })
            .collect();
        assert_eq!(departed, [1, 2]);
    }

    #[test]
    fn a_late_returner_cannot_restart_a_cleaned_up_match_under_its_id() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let cert = certificate(&key, 3, 2000);
        for source in 0..2 {
            let mut player = client(&key, source);
            connect(&mut server, &mut player, 0);
            configure(&mut server, &mut player, &cert, 0);
            let mut bytes = Vec::new();
            player.leave_ordering(&mut bytes).unwrap();
            events(&mut server, &mut player, &bytes, 1000);
        }
        server.advance_clock(1000, unix(1000));
        let cleaned = 1000 + MAX_TICKET_LIFETIME * 1000;
        server.advance_clock(cleaned, unix(cleaned));
        assert!(!directory.0.join("05".repeat(16)).exists());
        // The first player returns with a fresh ticket and its certificate.
        let ticket = Ticket {
            room: [5; 16],
            source: 0,
            expires: unix(cleaned) + 200,
            nonce: [9; 16],
            player: [1; 16],
            generation: 1,
        }
        .sign(&key)
        .unwrap();
        let packet_key = *derive_packet_key(&key, &ticket).expose_for_https();
        let mut returner = ClientSession::new(&ticket, packet_key, [89; 32]).unwrap();
        connect(&mut server, &mut returner, cleaned);
        assert!(matches!(
            configure(&mut server, &mut returner, &cert, cleaned).as_slice(),
            [OrderEvent::Error {
                code: OrderError::Inactive,
                ..
            }]
        ));
        assert!(!directory.0.join("05".repeat(16)).exists());
        // The marker outlasts any certificate verified before the cleanup.
        let expiry = unix(cleaned) + MAX_MATCH_SECONDS;
        server.advance_clock(1_000_000, expiry - 1);
        assert!(!server.ended_orderings.is_empty());
        server.advance_clock(1_000_000, expiry);
        assert!(server.ended_orderings.is_empty());
    }

    /// A started journaled match whose archive fails at 112 seconds, when a
    /// pull from the start, past the hot cache, reads a broken index.
    fn failed_match(directory: &TestDirectory) -> (RelayServer, ClientSession) {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 2000);
        for participant in [&mut first, &mut second] {
            connect(&mut server, participant, 0);
            configure(&mut server, participant, &cert, 0);
        }
        server.advance_clock(112_000, unix(112_000));
        std::fs::OpenOptions::new()
            .write(true)
            .open(directory.0.join("05".repeat(16)).join("stream.index"))
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
        server.advance_clock(112_000, unix(112_000));
        (server, first)
    }

    #[test]
    fn leave_after_a_journal_failure_is_acknowledged_and_keeps_the_archive() {
        let directory = TestDirectory::new();
        let (mut server, mut first) = failed_match(&directory);
        let room = directory.0.join("05".repeat(16));
        let before = std::fs::read(room.join("stream.data")).unwrap();
        // The client waits for this acknowledgement before it may join
        // another match.
        let mut bytes = Vec::new();
        first.leave_ordering(&mut bytes).unwrap();
        assert!(matches!(
            events(&mut server, &mut first, &bytes, 112_000).as_slice(),
            [OrderEvent::Left]
        ));
        assert_eq!(std::fs::read(room.join("stream.data")).unwrap(), before);
    }

    #[test]
    fn a_failed_archive_frees_its_room_once_every_ticket_expired_and_keeps_its_files() {
        let directory = TestDirectory::new();
        let (mut server, _) = failed_match(&directory);
        let room = directory.0.join("05".repeat(16));
        let before = std::fs::read(room.join("stream.data")).unwrap();
        // Both hop tickets expire at 1200, 200 seconds in.
        server.advance_clock(199_000, unix(199_000));
        assert!(server.rooms.contains_key(&[5; 16]));
        server.advance_clock(200_000, unix(200_000));
        assert!(!server.rooms.contains_key(&[5; 16]));
        assert_eq!(std::fs::read(room.join("stream.data")).unwrap(), before);
        // Its match ID stays taken, holding only its bytes on disk.
        let on_disk = [
            room.clone(),
            room.join("stream.data"),
            room.join("stream.index"),
        ]
        .iter()
        .map(|path| std::fs::metadata(path).unwrap().len())
        .sum::<u64>();
        let journals = server.journal_directory.as_ref().unwrap();
        assert_eq!(journals.reserved(&[5; 16]), Some(on_disk));
        assert_eq!(journals.total(), on_disk);
    }

    #[test]
    fn a_failed_archive_frees_its_room_ten_minutes_on_while_a_player_renews() {
        let directory = TestDirectory::new();
        let (mut server, mut first) = failed_match(&directory);
        let freed = 112_000 + ABSENT_MS;
        for now_ms in (150_000..freed).step_by(100_000) {
            renew(&mut server, &mut first, now_ms);
        }
        server.advance_clock(freed - 1, unix(freed - 1));
        assert!(server.rooms[&[5; 16]].ordering.is_some());
        server.advance_clock(freed, unix(freed));
        assert!(server.rooms[&[5; 16]].ordering.is_none());
        // A renewed Ready cannot start a new timeline under its match ID: the
        // ended-match marker answers while the certificate lasts, the files
        // after it.
        let cert = certificate(&server.key, 3, 2000);
        assert!(matches!(
            configure(&mut server, &mut first, &cert, freed)[0],
            OrderEvent::Error {
                code: OrderError::Inactive,
                ..
            }
        ));
    }

    /// A burst merge, such as a healed federation link's backlog, draws later
    /// hours instead of failing the room, and nothing stays counted after it.
    #[test]
    fn a_burst_of_frame_ends_draws_later_hours_and_releases_them() {
        let directory = TestDirectory::new();
        let mut journals =
            crate::journal::JournalDirectory::open(&directory.0, std::time::SystemTime::now())
                .unwrap();
        // One frame a second makes an hour 3600 frame ends of 25 bytes.
        let mut journal = journals.create([0x0d; 16], 1, 2).unwrap();
        let (total, growth) = (journals.total(), journals.growth());
        let mut frame_end = vec![0; 13];
        frame_end[0] = FRAME_END_EVENT;
        journal.commit(&vec![frame_end; 3700]).unwrap();
        journals.reserve_frames(&mut journal);
        assert_eq!(journals.total(), total + 3600 * 25);
        assert_eq!(journals.growth(), growth);
        journals.settle(&mut journal, 0);
        assert_eq!(journals.growth(), 0);
        journals.cleanup(journal, false);
        assert_eq!(journals.total(), 0);
    }

    #[test]
    fn startup_sets_aside_only_leftovers_of_ended_matches() {
        let directory = TestDirectory::new();
        let now = std::time::SystemTime::now();
        crate::journal::JournalDirectory::open(&directory.0, now).unwrap();
        for room in ["0b", "0c"] {
            let leftover = directory.0.join(room.repeat(16));
            std::fs::create_dir(&leftover).unwrap();
            std::fs::write(leftover.join("stream.data"), [0; 64]).unwrap();
        }
        // The second was written until an hour after the first.
        let hour = std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(directory.0.join("0c".repeat(16)).join("stream.data"))
            .unwrap()
            .set_modified(now + hour)
            .unwrap();
        let ended = std::time::Duration::from_millis(ROOM_LIFETIME_MS + MAX_TICKET_LIFETIME * 1000);
        let mut journals =
            crate::journal::JournalDirectory::open(&directory.0, now + ended + hour / 2).unwrap();
        assert_eq!(
            std::fs::read(
                directory
                    .0
                    .join("archive")
                    .join("0b".repeat(16))
                    .join("stream.data")
            )
            .unwrap(),
            [0; 64]
        );
        // Only the recent leftover still counts against the quota.
        assert_eq!(journals.reserved(&[0x0b; 16]), None);
        assert_eq!(Some(journals.total()), journals.reserved(&[0x0c; 16]));
        assert!(journals.create([0x0b; 16], 45, 2).is_ok());
        assert_eq!(
            journals.create([0x0c; 16], 45, 2).err().unwrap().kind(),
            std::io::ErrorKind::AlreadyExists
        );
    }

    #[test]
    fn failed_archive_creation_and_removal_release_their_slot() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let mut first = client(&key, 0);
        connect(&mut server, &mut first, 0);
        let cert = certificate(&key, 3, 2000);
        let room = directory.0.join("05".repeat(16));
        crate::journal::FAIL_NEXT_INDEX.set(true);
        assert!(matches!(
            configure(&mut server, &mut first, &cert, 0).as_slice(),
            [OrderEvent::Error {
                code: OrderError::StorageUnavailable,
                ..
            }]
        ));
        assert!(!room.exists());
        assert!(
            configure(&mut server, &mut first, &cert, 0)
                .iter()
                .any(|event| matches!(event, OrderEvent::Status(_)))
        );
        // A cancelled start reclaims its empty archive at once. Another file
        // in its directory makes that removal fail until the file is gone.
        std::fs::write(room.join("stray"), b"").unwrap();
        let mut bytes = Vec::new();
        first.leave_ordering(&mut bytes).unwrap();
        events(&mut server, &mut first, &bytes, 0);
        server.advance_clock(0, unix(0));
        assert!(room.exists());
        std::fs::remove_file(room.join("stray")).unwrap();
        server.advance_clock(1000, unix(1000));
        assert!(!room.exists());
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
        let cost = envelope::OVERHEAD as u64;
        // Leave needs no admission and accepts recently expired tickets. Like
        // a handshake, the player's own budget comes before the relay's. A
        // room's Leave spends that room's part; Leave for a room no longer
        // held, which only an expired ticket reaches, has its own budget and
        // the handshakes' part, so old tickets cannot spend what matches use.
        if !self.player_handshakes(&message.ticket.player).permits(
            now,
            1,
            PLAYER_HANDSHAKES_PER_SECOND,
        ) {
            return Err(RelayError::RateLimited);
        }
        self.advance_ordering_room(message.ticket.room, now_ms);
        if let Some(room) = self.rooms.get_mut(&message.ticket.room) {
            if !room.outgoing.permits(now, cost, ROOM_BYTES_PER_SECOND)
                || !self
                    .outgoing
                    .admit_part(&mut room.outgoing, now, cost, TOTAL_BYTES_PER_SECOND)
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
                // Like Ready, a signed ticket binds a certificate-admitted
                // source on first contact. Its initial Ready may have been
                // lost while another source already created this room.
                source.principal = Some(principal);
                // A failed archive keeps its files for inspection, but still
                // records the departure in memory and acknowledges it: the
                // client may join no other match until it does.
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
            if let Some(peer) = room
                .peers
                .iter_mut()
                .find(|peer| peer.ticket == message.ticket)
            {
                peer.push = None;
            }
            self.outgoing.charge_part(&mut room.outgoing, cost);
        } else if message.ticket.expires > now {
            // An unseen live lease may still have Ready in flight. Only an
            // expired absent room is safely acknowledged without a tombstone.
            return Err(RelayError::Unregistered);
        } else if !self.leaves.permits(now, 1, LEAVES_PER_SECOND)
            || !self.outgoing.admit_part(
                &mut self.handshake_outgoing,
                now,
                cost,
                TOTAL_BYTES_PER_SECOND,
            )
        {
            return Err(RelayError::RateLimited);
        } else {
            self.leaves.charge(now, 1);
            self.outgoing
                .charge_part(&mut self.handshake_outgoing, cost);
        }
        self.charge_player_handshake(message.ticket.player, now);
        // From now on the hop accepts only LEAVE, each answered again
        // (compaction spec §6.2 "释放").
        self.leave_session(message.session, now);
        Ok(vec![ForwardDatagram {
            endpoint,
            bytes: encode_server(
                &message.key,
                LEAVE_ACK,
                message.session,
                message.sequence,
                &[],
            ),
        }])
    }

    pub(super) fn retire_superseded(&mut self, ticket: Ticket, now_ms: u64, now: u64) {
        // A higher lobby-signed generation for this principal proves its old
        // membership ended. Source bindings outlive short hop admissions, so
        // this also closes a canceled load or a match whose final Leave was
        // lost; a started match records the departure like Leave.
        let superseded: Vec<_> = self
            .rooms
            .iter()
            .filter_map(|(id, room)| {
                if *id == ticket.room {
                    return None;
                }
                let ordering = room.ordering.as_ref()?;
                if ordering.active == 0 || ordering.storage_failed {
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
            if let Some(room) = self.rooms.get_mut(&id)
                && let Some(ordering) = room.ordering.as_mut()
            {
                let before = ordering.memory();
                ordering.advance(now_ms, room.federation.as_mut());
                ordering.count_changes(&mut self.ordering_counts);
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
        if let Some(directory) = self.journal_directory.as_mut() {
            directory.retry(now_ms);
            for room in self.rooms.values_mut() {
                if let Some(ordering) = room.ordering.as_mut()
                    && let Some(journal) = ordering.journal.as_mut()
                {
                    if ordering.finished_at.is_some() {
                        directory.settle(journal, ordering.sealed - ordering.head);
                    } else if !ordering.storage_failed {
                        directory.reserve_frames(journal);
                    }
                }
            }
        }
        let ended: Vec<_> = self
            .rooms
            .iter()
            .filter_map(|(id, room)| {
                room.ordering
                    .as_ref()
                    .filter(|ordering| {
                        ordering.cleanup_due(now_ms, now, !room.peers.is_empty())
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
            let retire = ordering.cleanup_due(now_ms, now, !room.peers.is_empty());
            if let Some(directory) = self.journal_directory.as_mut()
                && let Some(journal) = ordering.journal.take()
            {
                directory.cleanup(journal, ordering.storage_failed);
            }
            // No commands or frames can exist before the full Ready quorum.
            // Reclaim its empty archive immediately, retaining only the
            // inactive tombstone until every old hop ticket has expired.
            if !retire {
                continue;
            }
            // Every certificate for this match, renewed ones included, was
            // verified to expire within MAX_MATCH_SECONDS of now or earlier.
            self.ended_orderings
                .insert(id, now.saturating_add(MAX_MATCH_SECONDS));
            room.ordering = None;
            if room.peers.is_empty() {
                self.rooms.remove(&id);
            }
        }
    }

    pub(super) fn advance_ordering_room(&mut self, id: [u8; 16], now_ms: u64) {
        if let Some(room) = self.rooms.get_mut(&id)
            && let Some(ordering) = room.ordering.as_mut()
        {
            let before = ordering.memory();
            ordering.advance(now_ms, room.federation.as_mut());
            ordering.count_changes(&mut self.ordering_counts);
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
                    Some((*id, ordering.frame_due_ms(entry.frame)))
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
        if payload.first() == Some(&crate::link::UPLINK_REQUEST) {
            return self.ordering_uplink(message, now, now_ms);
        }
        if payload.first() == Some(&PAUSE_REQUEST) {
            if !self.reserve_ordering(2 * (PAUSE_ENTRY_BYTES + ENTRY_OVERHEAD)) {
                return self.ordering_reply(
                    message.ticket,
                    error(OrderError::Backpressure, 0),
                    now,
                    now_ms,
                    crate::link::SEND_OTHER,
                );
            }
            let room = self
                .rooms
                .get_mut(&message.ticket.room)
                .ok_or(RelayError::Unregistered)?;
            let replies = match room.ordering.as_mut() {
                Some(ordering) => ordering.control_pause(message.ticket, payload, now_ms)?,
                None => error(OrderError::NotConfigured, 0),
            };
            return self.ordering_reply(
                message.ticket,
                replies,
                now,
                now_ms,
                crate::link::SEND_OTHER,
            );
        }
        let replies = if payload.first() == Some(&READY_REQUEST) {
            let certificate_len = payload.len().saturating_sub(1 + 32);
            if !is_ordering_certificate_len(certificate_len) {
                return Err(RelayError::Malformed);
            }
            let certificate =
                OrderingCertificate::verify(&payload[1..1 + certificate_len], &self.key, now)?;
            // A one-source match runs its commands locally (the client's
            // original-singleton policy) and never orders through the relay.
            // The lobby still signs its certificate for released clients, so
            // refuse it here: a lone client cannot hold a room and journal.
            if certificate.room != message.ticket.room
                || certificate.sources_mask & (1 << message.ticket.source) == 0
                || certificate.sources_mask.count_ones() < 2
            {
                return Err(RelayError::Authentication);
            }
            if self
                .rooms
                .get(&message.ticket.room)
                .is_some_and(|room| room.ordering_cancelled)
                || self.ended_orderings.contains_key(&message.ticket.room)
            {
                return self.ordering_reply(
                    message.ticket,
                    error(OrderError::Inactive, 0),
                    now,
                    now_ms,
                    crate::link::SEND_OTHER,
                );
            }
            let agreement: [u8; 32] = payload[1 + certificate_len..].try_into().unwrap();
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
                        match directory.create(
                            certificate.room,
                            certificate.tick_hz,
                            certificate.sources_mask.count_ones(),
                        ) {
                            Ok(journal) => Some(journal),
                            Err(_) => {
                                return self.ordering_reply(
                                    message.ticket,
                                    error(OrderError::StorageUnavailable, 0),
                                    now,
                                    now_ms,
                                    crate::link::SEND_OTHER,
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
            let local = room
                .federation
                .as_ref()
                .map_or(certificate.sources_mask, federation::Federation::local);
            let ordering = room.ordering.get_or_insert_with(|| {
                let mut ordering =
                    OrderingRoom::new(certificate, agreement, journal, local, now_ms);
                // A peer silent before the first Ready has been silent since
                // its last hop packet, not since this room appeared.
                for peer in &room.peers {
                    if let Some(source) = ordering.sources.get_mut(usize::from(peer.ticket.source))
                    {
                        source.heard_ms = now_ms.saturating_sub(
                            now.saturating_sub(peer.last_seen).saturating_mul(1000),
                        );
                    }
                }
                ordering
            });
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
                // A renewed certificate never extends the room: the first
                // one's expiry bounds its startup, and its creation time
                // its whole lifetime.
                ordering.ready |= 1 << message.ticket.source;
                let ready_mask = match room.federation.as_ref() {
                    Some(federation) => federation.ready(ordering.ready, &ordering.agreement),
                    None => {
                        ordering.start_when_ready(now_ms);
                        ordering.ready
                    }
                };
                let mut ready = vec![READY_EVENT];
                ready.extend_from_slice(&ready_mask.to_le_bytes());
                ready.extend_from_slice(&ordering.certificate.sources_mask.to_le_bytes());
                vec![ready, ordering.status(now_ms)]
            }
        } else {
            let room = self
                .rooms
                .get_mut(&message.ticket.room)
                .ok_or(RelayError::Unregistered)?;
            if let Some(ordering) = room.ordering.as_mut() {
                let replies = ordering.request(message.ticket, payload, now_ms)?;
                if payload.first() == Some(&PULL_REQUEST) {
                    self.ordering_counts.pull_replies += 1;
                }
                replies
            } else {
                error(OrderError::NotConfigured, 0)
            }
        };
        self.ordering_reply(
            message.ticket,
            replies,
            now,
            now_ms,
            crate::link::SEND_OTHER,
        )
    }

    /// An UPLINK (spec §3.2): the reader's subscription and link report
    /// (§7.1 steps 1-3; step 1 is the hop receipt in `process_at`), then its
    /// batches (§8), checksums and pace, and the §8 reply.
    fn ordering_uplink(
        &mut self,
        message: &Incoming<'_>,
        now: u64,
        now_ms: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        let uplink = crate::link::decode_uplink(message.payload)?;
        // Reserve memory only for batches past the source's last accepted
        // one: duplicates take none, and most UPLINKs carry none new.
        let last = self
            .rooms
            .get(&message.ticket.room)
            .and_then(|room| room.ordering.as_ref())
            .and_then(|ordering| {
                ordering.sources[usize::from(message.ticket.source)]
                    .last_batch
                    .as_ref()
            })
            .map_or(0, |(id, _, _)| *id);
        let new_bytes: usize = uplink
            .batches
            .iter()
            .enumerate()
            .filter(|(index, _)| uplink.first_batch_id + *index as u64 > last)
            .map(|(_, opaque)| 2 * (9 + opaque.len() + ENTRY_OVERHEAD))
            .sum();
        let reserved = new_bytes == 0 || self.reserve_ordering(new_bytes);
        let ticket = message.ticket;
        let room = self
            .rooms
            .get_mut(&ticket.room)
            .ok_or(RelayError::Unregistered)?;
        let Some(ordering) = room.ordering.as_mut() else {
            let replies = error(OrderError::NotConfigured, 0);
            return self.ordering_reply(ticket, replies, now, now_ms, crate::link::SEND_OTHER);
        };
        if ordering.storage_failed {
            let replies = error(OrderError::StorageUnavailable, ordering.next_seq);
            return self.ordering_reply(ticket, replies, now, now_ms, crate::link::SEND_OTHER);
        }
        ordering.authenticate_reader(ticket)?;
        let peer = room
            .peers
            .iter_mut()
            .find(|peer| peer.ticket == ticket)
            .ok_or(RelayError::Unregistered)?;
        // A reader back from offline restarts from its watermark (§5.5).
        peer.evaluate_reader(now_ms);
        // §7.1 step 2: the watermark only grows, so a reordered UPLINK
        // cannot move it back. PUSH subscribes or renews the 3000 ms lease.
        if peer
            .push
            .as_ref()
            .is_some_and(|push| now_ms.saturating_sub(push.refreshed_at) >= PUSH_LEASE_MS)
        {
            peer.push = None;
        }
        let cursor = peer.push.as_ref().map_or(uplink.recv_next, |push| {
            push.acknowledged_next.max(uplink.recv_next)
        });
        let cursor_error = ordering.cursor_error(cursor);
        if cursor_error.is_none() {
            let restoring = uplink.flags & crate::link::FLAG_RESTORING != 0;
            if let Some(push) = peer.push.as_mut() {
                push.acknowledge(cursor, now_ms);
                push.restoring = restoring;
                if uplink.flags & crate::link::FLAG_PUSH != 0 {
                    push.refreshed_at = now_ms;
                }
            } else if uplink.flags & crate::link::FLAG_PUSH != 0 {
                peer.push = Some(PushSubscription::new(
                    cursor,
                    restoring,
                    ordering.push_status_state(now_ms),
                    now_ms,
                ));
            }
        }
        // §7.1 step 3.
        peer.link.delayed = uplink.link & crate::link::LINK_DELAYED != 0;
        let jittery = uplink.link & crate::link::LINK_JITTERY != 0;
        if peer.link.jittery
            && !jittery
            && peer.link.clock.state == crate::link::LinkState::Good
            && let Some(push) = peer.push.as_mut()
        {
            // The repeat turns off (spec §7.5).
            push.unrepeated_since = None;
            push.repeat_next = push.sent_next;
        }
        peer.link.jittery = jittery;
        peer.link.srtt_c = uplink.srtt_ms;
        peer.link.rttvar_c = uplink.rttvar_ms;
        // §7.1 step 4: downlink loss, and what the lost packs owe (§7.3).
        let lost = peer.link.judge(uplink.down, now_ms);
        self.ordering_counts.down_lost_packets += peer.owe_lost(&lost, now_ms);
        peer.evaluate_reader(now_ms);
        let mut batch_error = None;
        for (index, opaque) in uplink.batches.iter().enumerate() {
            let id = uplink.first_batch_id + index as u64;
            if let Err(failure) = ordering.accept_batch(ticket.source, id, opaque, reserved) {
                batch_error = Some(failure);
                break;
            }
        }
        if !uplink.checksums.is_empty() {
            ordering.report_checksums(ticket, &uplink.checksums, room.federation.as_ref());
            ordering.count_changes(&mut self.ordering_counts);
        }
        if let Some(pace) = uplink.pace
            && ordering.report_pace(ticket.source, pace, now_ms) != OrderPace::KeepingUp
        {
            self.ordering_counts.catching_up_s += 1;
        }
        // §8 replies: only the error; else an ACK for any batch, even all
        // duplicates; else a STATUS for a probe.
        let replies = if let Some((code, expected)) = batch_error {
            error(code, expected)
        } else if !uplink.batches.is_empty()
            && let Some((id, frame, _)) = &ordering.sources[usize::from(ticket.source)].last_batch
        {
            ack(*id, *frame)
        } else if let Some((code, expected)) = cursor_error {
            error(code, expected)
        } else if uplink.flags & crate::link::FLAG_PROBE != 0 {
            vec![ordering.status(now_ms)]
        } else {
            Vec::new()
        };
        if replies.is_empty() {
            return Ok(Vec::new());
        }
        self.ordering_reply(ticket, replies, now, now_ms, crate::link::SEND_OTHER)
    }

    /// Sends `replies` on `ticket`'s hop under every rate budget. STATUS and
    /// ACK get the hop feedback tail as they are encoded (spec §4.1); PACKs
    /// are logged as `pack_kind`, everything else as non-PACK (spec §4.2).
    fn ordering_reply(
        &mut self,
        ticket: Ticket,
        mut replies: Vec<Vec<u8>>,
        now: u64,
        now_ms: u64,
        pack_kind: u8,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        if let Some(peer) = self
            .rooms
            .get(&ticket.room)
            .and_then(|room| room.peers.iter().find(|peer| peer.ticket == ticket))
        {
            let feedback = peer.link.feedback(now_ms);
            for reply in &mut replies {
                if matches!(reply.first(), Some(&STATUS_EVENT | &ACK_EVENT)) {
                    crate::link::append_feedback(reply, feedback);
                }
            }
        }
        let cost = replies
            .iter()
            .map(|payload| envelope::OVERHEAD + payload.len())
            .sum::<usize>() as u64;
        let packets = replies.len() as u64;
        let room = self
            .rooms
            .get_mut(&ticket.room)
            .ok_or(RelayError::Unregistered)?;
        let admission = self
            .admissions
            .get_mut(&ticket.player)
            .ok_or(RelayError::Unregistered)?;
        if !admission
            .outgoing
            .permits(now, cost, PLAYER_BYTES_PER_SECOND)
            || !room.outgoing.permits(now, cost, ROOM_BYTES_PER_SECOND)
            || !admission.order_packets.permits(now, packets, 2048)
            || !room.order_packets.permits(now, packets, ROOM_ORDER_PACKETS)
        {
            return Err(RelayError::RateLimited);
        }
        // Ask both relay-wide totals, so each counts the room's demand.
        let bytes = self
            .outgoing
            .admit_part(&mut room.outgoing, now, cost, TOTAL_BYTES_PER_SECOND);
        if !(self.order_packets.admit_part(
            &mut room.order_packets,
            now,
            packets,
            INGRESS_PER_SECOND,
        ) && bytes)
        {
            return Err(RelayError::RateLimited);
        }
        admission.outgoing.charge(now, cost);
        self.outgoing.charge_part(&mut room.outgoing, cost);
        admission.order_packets.charge(now, packets);
        self.order_packets
            .charge_part(&mut room.order_packets, packets);
        let peer = room
            .peers
            .iter_mut()
            .find(|peer| peer.ticket == ticket)
            .ok_or(RelayError::Unregistered)?;
        replies
            .drain(..)
            .map(|payload| {
                let kind = if payload.first() == Some(&PACK_EVENT) {
                    pack_kind
                } else {
                    crate::link::SEND_OTHER
                };
                server_packet(peer, CONTROL, ticket.source, &payload, now_ms, kind)
            })
            .collect()
    }
}
