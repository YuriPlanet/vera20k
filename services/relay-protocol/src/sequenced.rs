//! Fixed-clock ordering of plaintext batches (bandwidth-compaction spec §3). A reader's cursor never
//! participates in clock advancement, admission, or another reader's progress.

use std::collections::{BTreeMap, VecDeque};

use super::*;
use crate::link::{Reader, field, read_u32, read_u64};

mod downlink;
pub(crate) mod federation;
pub(crate) mod pack;
pub(crate) mod watch;

/// A `VO1C` certificate; `VO2C` adds one flags byte.
const ORDERING_CERTIFICATE_LEN: usize = 66;
const ORDERING_CERTIFICATE_V2_LEN: usize = 67;
pub const ORDERING_BATCH_CAPACITY: usize = 512;
const ORDERING_PULL_LIMIT: usize = 32;
const CERTIFICATE_MAGIC: &[u8; 4] = b"VO1C";
const CERTIFICATE_DOMAIN: &[u8] = b"VERA20k relay ordering certificate v1\0";
const CERTIFICATE_V2_MAGIC: &[u8; 4] = b"VO2C";
const CERTIFICATE_V2_DOMAIN: &[u8] = b"VERA20k relay ordering certificate v2\0";
// VO2C flags byte: the host turned relay pacing off.
const CERTIFICATE_PACING_OFF: u8 = 1;
// VO2C flags byte: the relay orders this one-source match.
const CERTIFICATE_ORDERED_ALONE: u8 = 2;
// VO2C flags byte: continuous pacing, which may seal any whole rate from the
// certificate's down to `CONTINUOUS_FLOOR` (README, Pace).
const CERTIFICATE_PACING_CONTINUOUS: u8 = 4;
const MAX_MATCH_SECONDS: u64 = 24 * 60 * 60;
const START_DELAY_MS: u64 = 200;
const MAX_HISTORY_ENTRIES: usize = 4096;
const ROOM_HISTORY_BYTES: usize = 2 * 1024 * 1024;
const GLOBAL_ORDERING_BYTES: usize = 64 * 1024 * 1024;
const MAX_SOURCE_PENDING: usize = 8;
/// Each relay of a match seals at most this many of its own sources' batches
/// into one frame; a federated frame merges up to `MAX_FEDERATED_RELAYS`
/// times as many. Games size their frame buffers from both, so raising either
/// raises `OLDEST_ADMITTED` (tools/relay_compat.py).
pub const MAX_FRAME_BATCHES: usize = 32;
// A loading source that sends nothing this long departs in the first frame,
// and the others start without it.
const LOADING_SILENCE_MS: u64 = 60_000;
// Even a loading source that still answers departs once the room is this old.
const LOADING_LIMIT_MS: u64 = 180_000;
/// A started source silent this long departs through the stream. A shorter
/// outage keeps its place for the returning player's replay. The game ends its
/// own match once it has heard nothing for this long (`SequencedError::Absent`).
pub const ABSENT_MS: u64 = 600_000;
/// VERA: once started, a source silent this long is named in the other
/// players' STATUS until it is heard again (README "Silence").
const SILENT_NOTICE_MS: u64 = 10_000;
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
/// Every canonical record's head: kind u8, sequence u64, frame u32.
pub(super) const CANONICAL_HEAD: usize = 13;
// A RATE record after its head: frames per second u16 and the slow source u8.
pub(super) const RATE_TAIL: usize = RATE_ENTRY_BYTES - CANONICAL_HEAD;
// A DESYNC record after its head: sample frame u32, outlier source u8 and
// checksum u32, other source u8 and checksum u32.
pub(super) const DESYNC_TAIL: usize = DESYNC_ENTRY_BYTES - CANONICAL_HEAD;
// Consecutive history entries in one datagram, PACK (compaction spec
// §5.2, `pack.rs`).
pub(super) const PACK_EVENT: u8 = 12;
// A federated control record travels between relays as an entry of this
// source: a rate change under `RATE_BATCH`, a desync under `DESYNC_BATCH`.
pub(super) const CONTROL_SOURCE: u8 = u8::MAX;
pub(super) const RATE_BATCH: u64 = u64::MAX;
pub(super) const DESYNC_BATCH: u64 = u64::MAX - 1;
/// Game Speed's frame rates, fastest first: the only rates an ordering
/// certificate may name. The game's test in `net::lockstep` pins each
/// selectable speed to one of them.
pub const FRAME_RATES: [u16; 8] = [90, 60, 45, 30, 20, 15, 12, 10];
/// The slowest rate continuous pacing seals at, Game Speed's slowest row.
pub const CONTINUOUS_FLOOR: u16 = FRAME_RATES[FRAME_RATES.len() - 1];
// A player's report counts this long; a silent client slows nobody.
const PACE_REPORT_MS: u64 = 3_000;
// Clients report every `PACE_INTERVAL_MS`, so a report on time never lapses.
const _: () = assert!(PACE_INTERVAL_MS < PACE_REPORT_MS);
/// The share of each second a reporter's simulation may take at a rate it
/// can hold; the rest draws and catches up. The game's online render pacing
/// leaves its simulation the same share between presents.
pub const SUSTAIN_PERCENT: u32 = 80;
// Frames must wait on a client this long before it counts as lagging
// (`PaceLag`).
const PACE_BACKLOG_MS: u32 = 2_000;
// Rate changes are at least this far apart, so reports show a step's effect
// before the next one.
const PACE_STEP_MS: u64 = 5_000;
// With nobody behind this long the clock tries the next faster rate. A
// slowdown this soon after a raise doubles the wait, up to the cap.
const PACE_RAISE_MS: u64 = 30_000;
const PACE_RAISE_MAX_MS: u64 = 300_000;
// Behind this long without a break, a player's tick cost no longer speaks
// for it: what it ran does. A PC whose ticks are quick can still fall behind
// when drawing and copying the world leave it too little of each second
// (2026-11-21, match 1e9fac21: 8 ms ticks, 55 frames a second at 60 Hz).
const SUSTAINED_BEHIND_MS: u64 = 2_000;
// Continuous pacing raises the clock towards this share of what a player ran
// while behind, leaving it a margin to keep up with.
const CONTINUOUS_HOLD_PERCENT: u32 = 95;
// Continuous pacing: a player with frames waiting on it this long, though it
// runs as many as the clock seals, keeps a standing backlog its orders wait
// behind; the clock slows a twentieth to drain it.
const STANDING_BACKLOG_MS: u64 = 10_000;
// Continuous pacing slows at most this share of the rate at a time, so a
// hitch of a few seconds (loading pictures) costs a quarter, not the match.
const CONTINUOUS_STEP_DOWN_PERCENT: u32 = 25;
// Continuous pacing's raises are small, so it waits this fraction of the
// rows' raise wait (30 seconds, doubled after a relapse).
const CONTINUOUS_RAISE_WAIT_DIVISOR: u64 = 3;
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
    /// `VO1C`: the signer could not tell that every member reads rate
    /// markers, so the clock changes only once every active source has
    /// reported.
    #[default]
    Legacy,
    /// `VO2C`: every member reads rate markers; a player who cannot keep up
    /// slows the clock.
    On,
    /// `VO2C` with the host's pacing switch off: the clock keeps its rate.
    Off,
    /// `VO2C` whose members read any whole rate: the clock slows to what the
    /// slowest player holds, down to [`CONTINUOUS_FLOOR`], and speeds up half
    /// the way back at a time, as Spring's speed control does
    /// (`CGameServer::LagProtection`).
    Continuous,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderingCertificate {
    pub room: [u8; 16],
    pub sources_mask: u32,
    pub tick_hz: u16,
    pub expires: u64,
    pub pacing: CertificatePacing,
    /// `VO2C` only: the relay orders this match although it has one source,
    /// a person who watches AI play, so that others can watch it too
    /// (docs/Networking.md, Watching). See [`Self::relay_orders`].
    pub ordered_alone: bool,
}

/// Either certificate length, for a READY payload's split.
fn is_ordering_certificate_len(length: usize) -> bool {
    length == ORDERING_CERTIFICATE_LEN || length == ORDERING_CERTIFICATE_V2_LEN
}

impl OrderingCertificate {
    pub fn parse(bytes: &[u8]) -> Result<Self, RelayError> {
        let (pacing, ordered_alone) = match (bytes.len(), bytes.get(..4)) {
            (ORDERING_CERTIFICATE_LEN, Some(magic)) if magic == CERTIFICATE_MAGIC => {
                (CertificatePacing::Legacy, false)
            }
            (ORDERING_CERTIFICATE_V2_LEN, Some(magic)) if magic == CERTIFICATE_V2_MAGIC => {
                let flags = bytes[34];
                let pacing_flags = CERTIFICATE_PACING_OFF | CERTIFICATE_PACING_CONTINUOUS;
                if flags & !(pacing_flags | CERTIFICATE_ORDERED_ALONE) != 0
                    || flags & pacing_flags == pacing_flags
                {
                    return Err(RelayError::Malformed);
                }
                let pacing = if flags & CERTIFICATE_PACING_OFF != 0 {
                    CertificatePacing::Off
                } else if flags & CERTIFICATE_PACING_CONTINUOUS != 0 {
                    CertificatePacing::Continuous
                } else {
                    CertificatePacing::On
                };
                (pacing, flags & CERTIFICATE_ORDERED_ALONE != 0)
            }
            _ => return Err(RelayError::Malformed),
        };
        let value = Self {
            room: field(bytes, 4)?,
            sources_mask: read_u32(bytes, 20)?,
            tick_hz: u16::from_le_bytes(field(bytes, 24)?),
            expires: read_u64(bytes, 26)?,
            pacing,
            ordered_alone,
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
            || (self.ordered_alone && self.pacing == CertificatePacing::Legacy)
        {
            return Err(RelayError::Malformed);
        }
        Ok(())
    }

    /// Whether the relay orders this match: two or more sources, or one the
    /// lobby marked [`Self::ordered_alone`]. Every other one-source match runs
    /// its commands locally (the client's original-singleton policy), so the
    /// relay refuses its Ready: a lone client holds a room and journal only
    /// where the lobby marked its match.
    pub fn relay_orders(&self) -> bool {
        self.sources_mask.count_ones() > 1 || self.ordered_alone
    }

    /// A renewed certificate can name a later expiry. All other fields identify
    /// the match whose clock and roster the relay admitted.
    fn same_match(&self, other: &Self) -> bool {
        Self {
            expires: other.expires,
            ..*self
        } == *other
    }

    /// The HTTPS lobby signs this public manifest separately from hop tickets.
    /// Layout: VO1C, room16, LE mask32, LE Hz16, LE expiry64, domain HMAC32;
    /// VO2C adds a flags byte (bit 0: pacing off, bit 1: ordered alone, bit 2:
    /// continuous pacing) before its own domain's HMAC.
    pub fn sign(self, key: &SecretKey) -> Result<Vec<u8>, RelayError> {
        self.validate()?;
        let alone = if self.ordered_alone {
            CERTIFICATE_ORDERED_ALONE
        } else {
            0
        };
        let (magic, flags) = match self.pacing {
            CertificatePacing::Legacy => (CERTIFICATE_MAGIC, None),
            CertificatePacing::On => (CERTIFICATE_V2_MAGIC, Some(alone)),
            CertificatePacing::Off => (CERTIFICATE_V2_MAGIC, Some(CERTIFICATE_PACING_OFF | alone)),
            CertificatePacing::Continuous => (
                CERTIFICATE_V2_MAGIC,
                Some(CERTIFICATE_PACING_CONTINUOUS | alone),
            ),
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

impl OrderPace {
    /// Reads the pace byte of an UPLINK pace segment and of a forwarded
    /// federation report.
    pub(crate) fn parse(value: u8) -> Result<Self, RelayError> {
        Ok(match value {
            0 => Self::KeepingUp,
            1 => Self::Behind,
            2 => Self::CatchingUp,
            _ => return Err(RelayError::Malformed),
        })
    }
}

/// How a client judges its [`OrderPace`]: the game's pace meter and the
/// relay's pace replays both use it. Lagging means frames waited on it for
/// `PACE_BACKLOG_MS`; behind also means two report windows in a row with
/// frames waiting ran fewer than the clock sealed, but some: a world that
/// ran none was held by something other than speed. Times are wrapping
/// milliseconds.
#[derive(Debug, Default)]
pub struct PaceLag {
    backlog_since: Option<u32>,
    slow_windows: u8,
}

impl PaceLag {
    /// Samples the frames waiting at `now`. A backlog is at least an eighth
    /// of a second's frames at `rate`, and at least four; fewer end it.
    pub fn observe(&mut self, now: u32, waiting: usize, rate: u16) {
        if waiting >= usize::from(rate / 8).max(4) {
            self.backlog_since.get_or_insert(now);
        } else {
            self.backlog_since = None;
        }
    }

    /// The pace of a report window that ran `ran` frames a second while the
    /// clock sealed `rate`.
    pub fn classify(&mut self, now: u32, ran: u16, rate: u16) -> OrderPace {
        self.slow_windows = if ran > 0 && ran < rate && self.backlog_since.is_some() {
            self.slow_windows.saturating_add(1)
        } else {
            0
        };
        let lagging = self
            .backlog_since
            .is_some_and(|since| now.wrapping_sub(since) >= PACE_BACKLOG_MS);
        match (lagging, self.slow_windows >= 2) {
            (false, _) => OrderPace::KeepingUp,
            (true, true) => OrderPace::Behind,
            (true, false) => OrderPace::CatchingUp,
        }
    }

    /// Starts over, as after a deliberate wait.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
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

/// Rates allowed by the certificate: a fixed rate with pacing off, one of
/// the [`paced_rates`] rows with ordinary pacing, or any whole rate down to
/// [`CONTINUOUS_FLOOR`] with continuous pacing.
pub fn admits_rate(tick_hz: u16, pacing: CertificatePacing, rate: u16) -> bool {
    match pacing {
        CertificatePacing::Continuous => (CONTINUOUS_FLOOR.min(tick_hz)..=tick_hz).contains(&rate),
        CertificatePacing::Off => rate == tick_hz,
        CertificatePacing::On | CertificatePacing::Legacy => paced_rates(tick_hz).contains(&rate),
    }
}

/// Frames the relay seals at once at `rate`, about fifteen turns a second:
/// 6 at 90 Hz, 4 at 60, 3 at 45, 2 at 30 and 20, 1 at 15 and below. A turn's frames
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
    /// A watcher's STATUS only: the newest `(frame, checksum)` sample every
    /// player reported alike, of a frame the watcher may read (README
    /// "Watchers").
    pub watch_sample: Option<(u32, u32)>,
    /// VERA: a player's STATUS only: the active sources the relay has not
    /// heard from for `SILENT_NOTICE_MS` (README "Silence"). 0 while it heard
    /// every player lately, and always from a relay that does not tell.
    pub silent_sources: u32,
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
    /// apart, a rate the certificate admits ([`admits_rate`]). `source` is
    /// the player who could not keep up; `None` speeds back up.
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

pub(super) struct SealedBatch {
    pub(super) id: u64,
    pub(super) opaque: Vec<u8>,
}

pub(super) struct ClientOrdering {
    certificate: OrderingCertificate,
    agreement: [u8; 32],
    // The four most recently sealed batches retain their exact bytes.
    // Accepted entries stay cached until a new batch needs the slot, so this
    // source's batches in the stream are checked against what it sealed.
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
    /// A player's or a watcher's state for a match it has not yet ordered in.
    fn new(certificate: OrderingCertificate, agreement: [u8; 32]) -> Self {
        Self {
            certificate,
            agreement,
            sealed: VecDeque::new(),
            next_batch: 1,
            accepted_through: 0,
            restoring: false,
            leave: None,
            link: crate::link::ClientLink::new(certificate.tick_hz),
        }
    }

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
    /// Bounded client pipeline, below the relay's `MAX_SOURCE_PENDING`
    /// batches per source.
    pub const ORDERING_SUBMISSION_WINDOW: usize = 4;

    /// Move the current stream cache at hop-lease promotion, after the old
    /// route's last receive/submit. A renewal candidate holds no ordering
    /// until this moves it.
    ///
    /// Also moves the path measurement (spec §4.7): `loss‰`, `srtt`, `base`
    /// and the uplink state carry over; this hop's `down` window, send log
    /// and loss cursor start empty. The next `poll_uplink` on this session
    /// sends an UPLINK at once, with PUSH and every unacknowledged batch.
    pub fn adopt_ordering_from(&mut self, previous: &mut Self) -> Result<(), RelayError> {
        if !self.ticket.same_identity(&previous.ticket) {
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

    /// The lobby's ordering certificate, parsed, if it names this hop's room
    /// and, for a player, its source. Only the relay checks its signature.
    pub fn admit_certificate(&self, certificate: &[u8]) -> Result<OrderingCertificate, RelayError> {
        let parsed = OrderingCertificate::parse(certificate)?;
        if parsed.room != self.ticket.room
            || (!self.is_watching() && parsed.sources_mask & (1 << self.ticket.source) == 0)
        {
            return Err(RelayError::Authentication);
        }
        Ok(parsed)
    }

    /// Starts this hop's ordering for the match `certificate` and `agreement`
    /// name, or checks that a later Ready names the same one.
    fn join_ordering(&mut self, certificate: &[u8], agreement: [u8; 32]) -> Result<(), RelayError> {
        let parsed = self.admit_certificate(certificate)?;
        match &self.ordering {
            Some(current)
                if current.certificate.same_match(&parsed) && current.agreement == agreement =>
            {
                Ok(())
            }
            Some(_) => Err(RelayError::Authentication),
            None => {
                self.ordering = Some(ClientOrdering::new(parsed, agreement));
                #[cfg(any(test, feature = "test-support"))]
                if self.measured_good {
                    self.preset_uplink_measured_good();
                }
                Ok(())
            }
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
        if self.is_watching() {
            return Err(RelayError::Authentication);
        }
        self.join_ordering(certificate, agreement)?;
        let mut payload = vec![READY_REQUEST];
        payload.extend_from_slice(certificate);
        payload.extend_from_slice(&agreement);
        self.encode(CONTROL, &payload, out)
    }

    /// Seals `plaintext` as the next batch and returns its ID.
    pub(super) fn seal_batch(&mut self, plaintext: &[u8]) -> Result<u64, RelayError> {
        if !self.ready {
            return Err(RelayError::Unregistered);
        }
        if plaintext.is_empty() || plaintext.len() > ORDERING_BATCH_CAPACITY {
            return Err(RelayError::Malformed);
        }
        let ordering = self.ordering.as_mut().ok_or(RelayError::Unregistered)?;
        let batch_id = ordering.next_batch;
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
        Ok(batch_id)
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
        self.ordering
            .as_mut()
            .ok_or(RelayError::Unregistered)?
            .link
            .pull_pending = true;
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
        self.ordering
            .as_mut()
            .ok_or(RelayError::Unregistered)?
            .leave = Some((self.sent, out.clone()));
        Ok(())
    }

    pub fn ordering_is_configured(&self) -> bool {
        self.ordering.is_some()
    }

    /// The last of this source's batches the relay has accepted, from its
    /// ACK, BatchGap and this source's own BATCH records (spec §6.1, §6.4);
    /// while restoring, the replaced process's. `None` before ordering is
    /// configured.
    pub fn ordering_accepted_through(&self) -> Option<u64> {
        self.ordering
            .as_ref()
            .map(|ordering| ordering.accepted_through)
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
                let watching = self.is_watching();
                let read = |reader: &mut Reader| -> Result<OrderStatus, RelayError> {
                    let mut status = OrderStatus {
                        head_frame: reader.varint_u32()?,
                        oldest_seq: reader.varint()?,
                        next_seq: reader.varint()?,
                        active_sources: reader.u32()?,
                        tick_hz: reader.u16()?,
                        start_delay_ms: reader.varint_u32()?,
                        started: reader.u8()? != 0,
                        feedback: crate::link::read_feedback(reader)?,
                        watch_sample: None,
                        silent_sources: 0,
                    };
                    // After the feedback, a watcher's sample or a player's
                    // silent sources, each absent without one.
                    if watching {
                        status.watch_sample = watch::read_sample(reader);
                    } else {
                        status.silent_sources = reader.u32().unwrap_or(0);
                    }
                    Ok(status)
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
                    // The relay accepts only consecutive IDs (`BatchGap`), so
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
                    sequence: read_u64(payload, 1)?,
                    frame: read_u32(payload, 9)?,
                    epoch: read_u64(payload, 13)?,
                    paused: payload[21] != 0,
                    source: payload[22],
                })
            }
            // The session checks the rate against its certificate
            // (`admits_rate`); any whole rate a certificate may reach parses.
            (RATE_EVENT, RATE_ENTRY_BYTES)
                if (CONTINUOUS_FLOOR..=FRAME_RATES[0])
                    .contains(&u16::from_le_bytes([payload[13], payload[14]]))
                    && (payload[15] < MAX_PEERS || payload[15] == u8::MAX) =>
            {
                OrderEvent::Entry(OrderEntry::Rate {
                    sequence: read_u64(payload, 1)?,
                    frame: read_u32(payload, 9)?,
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
                    sequence: read_u64(payload, 1)?,
                    frame: read_u32(payload, 9)?,
                    sample_frame: read_u32(payload, 13)?,
                    outlier: payload[17],
                    outlier_checksum: read_u32(payload, 18)?,
                    other: payload[22],
                    other_checksum: read_u32(payload, 23)?,
                })
            }
            (READY_EVENT, 9..) => OrderEvent::Ready {
                ready_mask: read_u32(payload, 1)?,
                sources_mask: read_u32(payload, 5)?,
            },
            (FRAME_END_EVENT, 13) => OrderEvent::Entry(OrderEntry::FrameEnd {
                sequence: read_u64(payload, 1)?,
                frame: read_u32(payload, 9)?,
            }),
            (DEPARTURE_EVENT, length @ (14 | 15))
                if payload[13] < MAX_PEERS && (length == 14 || payload[14] == 1) =>
            {
                OrderEvent::Entry(OrderEntry::Departure {
                    sequence: read_u64(payload, 1)?,
                    frame: read_u32(payload, 9)?,
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
                let batch_id = read_u64(payload, 14)?;
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
                    sequence: read_u64(payload, 1)?,
                    frame: read_u32(payload, 9)?,
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
        pack::decode_pack(payload)?
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
    #[cfg(any(test, feature = "test-support"))]
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
    // When its unbroken run of Behind reports began.
    behind_since: Option<u64>,
    // Frames run, time and tick cost at the latest sustained shortfall.
    capacity: Option<(u16, u64, u32)>,
    // When its unbroken run of behind or catching-up reports began.
    lagging_since: Option<u64>,
    // Frames run, time and tick cost at the latest standing backlog
    // (`STANDING_BACKLOG_MS`), for continuous pacing's raise ceiling.
    standing: Option<(u16, u64, u32)>,
    // A sustained shortfall that repeats an earlier one within the longest
    // raise wait: the player relapsed, so what it ran is its capacity, not a
    // hitch. Continuous pacing's raise ceiling.
    relapse: Option<(u16, u64, u32)>,
}

impl PaceReport {
    /// Record measured capacity only after a continuous shortfall at the
    /// current clock rate. A missing or expired sample breaks the run.
    fn new(
        previous: Option<Self>,
        sample: crate::link::PaceSegment,
        at_ms: u64,
        rate: u16,
    ) -> Self {
        let tick_micros = (sample.tick_micros != 0).then_some(sample.tick_micros);
        let running_behind = sample.pace == OrderPace::Behind
            && tick_micros.is_some()
            && (1..rate).contains(&sample.frames_per_second);
        let behind_since = running_behind.then(|| {
            previous
                .filter(|p| at_ms.saturating_sub(p.at_ms) <= PACE_REPORT_MS)
                .and_then(|p| p.behind_since)
                .unwrap_or(at_ms)
        });
        let lagging_since =
            (sample.pace != OrderPace::KeepingUp && tick_micros.is_some()).then(|| {
                previous
                    .filter(|p| at_ms.saturating_sub(p.at_ms) <= PACE_REPORT_MS)
                    .and_then(|p| p.lagging_since)
                    .unwrap_or(at_ms)
            });
        let mut report = Self {
            pace: sample.pace,
            frames_per_second: sample.frames_per_second,
            tick_micros,
            at_ms,
            behind_since,
            capacity: previous.and_then(|p| p.capacity),
            lagging_since,
            standing: previous.and_then(|p| p.standing),
            relapse: previous.and_then(|p| p.relapse),
        };
        if report.measured_rate(rate).is_some() {
            let sample = Some((sample.frames_per_second, at_ms, sample.tick_micros));
            // An earlier run of behind reports measured it too.
            if report.capacity.is_some_and(|(_, at, _)| {
                report.behind_since.is_some_and(|since| at < since)
                    && at_ms.saturating_sub(at) < PACE_RAISE_MAX_MS
            }) {
                report.relapse = sample;
            }
            report.capacity = sample;
        }
        if report.standing_backlog() && sample.frames_per_second > 0 {
            report.standing = Some((sample.frames_per_second, at_ms, sample.tick_micros));
        }
        report
    }

    /// Nine tenths of what the player actually ran once fresh samples show
    /// a sustained shortfall. A rate change can end the shortfall.
    fn measured_rate(&self, rate: u16) -> Option<u32> {
        (self.frames_per_second < rate
            && self
                .behind_since
                .is_some_and(|since| self.at_ms - since >= SUSTAINED_BEHIND_MS))
        .then(|| u32::from(self.frames_per_second) * 9 / 10)
    }

    /// [`Self::sustainable_rate`], or less when what it ran says so.
    fn holdable_rate(&self, rate: u16) -> u32 {
        let sustainable = self.sustainable_rate();
        self.measured_rate(rate)
            .map_or(sustainable, |measured| measured.min(sustainable))
    }

    /// Whether what it ran while behind, within the longest raise wait and
    /// with ticks no quicker by a tenth since, says it cannot hold `rate`.
    /// Quicker ticks mean less to simulate, and its tick cost speaks again.
    fn ran_below(&self, rate: u16, now_ms: u64) -> bool {
        self.measured_capacity(now_ms)
            .is_some_and(|ran| u32::from(ran) * 9 / 10 < u32::from(rate))
    }

    /// What it ran while behind, while that still speaks for it: within the
    /// longest raise wait, with ticks no quicker by a tenth since.
    fn measured_capacity(&self, now_ms: u64) -> Option<u16> {
        self.fresh(self.capacity, now_ms)
    }

    /// A sample's frames run while it still speaks for this reporter.
    fn fresh(&self, sample: Option<(u16, u64, u32)>, now_ms: u64) -> Option<u16> {
        let micros = self.tick_micros?;
        let (ran, at, then) = sample?;
        (now_ms.saturating_sub(at) < PACE_RAISE_MAX_MS
            && u64::from(micros) * 10 > u64::from(then) * 9)
            .then_some(ran)
    }

    /// Frames have waited on this tick-cost reporter for
    /// `STANDING_BACKLOG_MS` without a break.
    fn standing_backlog(&self) -> bool {
        self.lagging_since
            .is_some_and(|since| self.at_ms.saturating_sub(since) >= STANDING_BACKLOG_MS)
    }

    /// The fastest rate continuous pacing raises the clock to for this
    /// reporter: what its tick cost holds, and no more than
    /// `CONTINUOUS_HOLD_PERCENT` of what it ran in its last standing backlog
    /// (`STANDING_BACKLOG_MS`) or relapse, so a single hitch of a few
    /// seconds holds nothing back. A legacy reporter holds back raises only
    /// by lagging.
    fn raise_ceiling(&self, now_ms: u64) -> u32 {
        if self.tick_micros.is_none() {
            return u32::MAX;
        }
        [self.standing, self.relapse]
            .into_iter()
            .filter_map(|sample| self.fresh(sample, now_ms))
            .map(|ran| u32::from(ran) * CONTINUOUS_HOLD_PERCENT / 100)
            .fold(self.sustainable_rate(), u32::min)
    }

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
    pause_supported: bool,
    pause_request: Option<PauseRequest>,
    // Also its capability: a source that reported reads rate markers.
    pace: Option<PaceReport>,
    // Coordinator only: last pace packet sequence and reported age at the follower.
    forwarded_pace: (u64, u32),
    // Last packet from this source that passed authentication and its rate
    // limits, keepalives included, or the latest a federated relay reported.
    heard_ms: u64,
}

impl Source {
    /// The principal bound to this source, or any before one is.
    fn admits(&self, principal: ([u8; 16], u64)) -> bool {
        self.principal.is_none_or(|bound| bound == principal)
    }

    /// A signed ticket binds a certificate-admitted source on first contact,
    /// by Ready or by Leave: its initial Ready may have been lost while
    /// another source already created the room.
    fn bind(&mut self, principal: ([u8; 16], u64)) -> Result<(), RelayError> {
        if !self.admits(principal) {
            return Err(RelayError::Authentication);
        }
        self.principal = Some(principal);
        Ok(())
    }
}

/// The sealing clock's slowdown for players whose PCs cannot keep up. This
/// is a relay adaptation of native lockstep, where every match runs as slow
/// as its slowest player (ProcessNetwork 0x00647BF4..0x00647E6D, TIMING):
/// here it slows at most two Game Speed rows, or by whole rates under
/// continuous pacing, and tries to speed up again.
struct Pace {
    rate: u16,
    // The last rate change's frame and due time, in milliseconds from the
    // match start. A pause or a follower's clock correction moves both together.
    anchor: Option<(u32, u64)>,
    raised_at: Option<u64>,
    raise_wait: u64,
    // When a player last reported being behind or catching up.
    lagging_at: u64,
    // Slowdowns and raises the server has not yet counted for its journal
    // report.
    slowdowns: u32,
    raises: u32,
    // Federated: the coordinator's rate changes for frames after these,
    // `(frame, rate, source)`, in order, each until its due time passes. A
    // healed link can deliver several at once.
    scheduled: VecDeque<(u32, u16, u8)>,
}

impl Pace {
    fn anchor_at(&self, start_ms: u64) -> Option<(u32, u64)> {
        self.anchor
            .map(|(frame, offset)| (frame, start_ms.saturating_add(offset)))
    }

    fn displace(&mut self, paused_for: u64) {
        if let Some(at) = self.raised_at.as_mut() {
            *at = at.saturating_add(paused_for);
        }
        self.lagging_at = self.lagging_at.saturating_add(paused_for);
    }

    /// Notes a player behind or catching up, and returns the last change's
    /// due time once `PACE_STEP_MS` have passed since it.
    fn step_due(&mut self, lagging: bool, now_ms: u64, start_ms: u64) -> Option<u64> {
        if lagging {
            self.lagging_at = now_ms;
        }
        let changed_at = self.anchor_at(start_ms).map_or(0, |(_, at)| at);
        (self.anchor.is_none() || now_ms.saturating_sub(changed_at) >= PACE_STEP_MS)
            .then_some(changed_at)
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

/// Rooms that take no new startup: cancelled before any Ready, here or by
/// another relay of the match, or whose ordering room was retired. A Ready
/// for one gets `Inactive`, and while the room remains, a federated relay
/// tells the others it is cancelled. While an `OrderingRoom` lives it holds
/// that state itself. Unlike the room, which goes once its peers' tickets
/// expire, the record lasts until every certificate signed before its last
/// close has expired.
#[derive(Default)]
pub(super) struct ClosedStartups(HashMap<[u8; 16], u64>);

impl ClosedStartups {
    /// Closes `room` for MAX_MATCH_SECONDS, the 24 hours a certificate
    /// lasts: every one signed before now expires first.
    pub(super) fn close(&mut self, room: [u8; 16], now: u64) {
        self.0.insert(room, now.saturating_add(MAX_MATCH_SECONDS));
    }

    pub(super) fn contains(&self, room: &[u8; 16]) -> bool {
        self.0.contains_key(room)
    }

    pub(super) fn prune(&mut self, now: u64) {
        self.0.retain(|_, until| *until > now);
    }
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
    // A Ready proved a different initial world. Keep the reason in the
    // cancelled room so retries receive the existing precise error.
    agreement_mismatch: bool,
    // That refusal, until the server counts it for its journal report.
    mismatch_uncounted: bool,
    next_seq: u64,
    history: VecDeque<Stored>,
    history_bytes: usize,
    pending: BTreeMap<u32, Vec<Pending>>,
    journal: Option<crate::journal::Journal>,
    storage_failed: bool,
    // When `advance` first found the storage failure.
    failed_at: Option<u64>,
    finished_at: Option<u64>,
    terminal: Option<federation::Terminal>,
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
    // What delayed watchers may read (README "Watchers").
    watch: watch::WatchLog,
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
                scheduled: VecDeque::new(),
            },
            head: 0,
            sealed: 0,
            local,
            cancel_requested: false,
            agreement_mismatch: false,
            mismatch_uncounted: false,
            next_seq: 1,
            history: VecDeque::new(),
            history_bytes: 0,
            pending: BTreeMap::new(),
            journal,
            storage_failed: false,
            failed_at: None,
            finished_at: None,
            terminal: None,
            created_ms: now_ms,
            checksums: BTreeMap::new(),
            desync_found: false,
            forward_checksums: Vec::new(),
            desyncs: 0,
            watch: watch::WatchLog::default(),
        }
    }

    /// Move the rate changes, desyncs and refused start not yet counted into
    /// the journal report's counts.
    pub(super) fn count_changes(&mut self, counts: &mut crate::OrderingCounts) {
        self.pace.count_changes(counts);
        counts.desyncs += u64::from(std::mem::take(&mut self.desyncs));
        counts.start_mismatches += u64::from(std::mem::take(&mut self.mismatch_uncounted));
    }

    /// Any packet that passed authentication and the rate limits keeps a
    /// source's place; a newer admission of another principal for the same
    /// source does not.
    pub(super) fn heard(&mut self, source: u8, principal: ([u8; 16], u64), now_ms: u64) {
        if let Some(state) = self.sources.get_mut(usize::from(source))
            && state.admits(principal)
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
            + self.watch.memory()
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
        self.status_of(self.head, self.next_seq, self.active, now_ms)
    }

    /// `status` with the stream ending at `head` and `next_seq`, `active`
    /// sources then: a watcher's delayed end.
    fn status_of(&self, head: u32, next_seq: u64, active: u32, now_ms: u64) -> Vec<u8> {
        let mut payload = vec![STATUS_EVENT];
        varint::write(head.into(), &mut payload);
        varint::write(self.oldest_seq(), &mut payload);
        varint::write(next_seq, &mut payload);
        payload.extend_from_slice(&active.to_le_bytes());
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

    /// Stores a player's pace report, which `adjust_pace` acts on.
    /// A reader's pace segment (spec §3.2), the latest report wins; a zero
    /// tick cost is none.
    fn report_pace(&mut self, source: u8, report: crate::link::PaceSegment, now_ms: u64) {
        let slot = &mut self.sources[usize::from(source)].pace;
        if slot.is_none_or(|previous| now_ms >= previous.at_ms) {
            *slot = Some(PaceReport::new(*slot, report, now_ms, self.pace.rate));
        }
    }

    /// Changes the clock's rate when players' reports call for it: in Game
    /// Speed rows ([`Self::row_change`]), or by whole rates under continuous
    /// pacing ([`Self::continuous_change`]). Pacing switched off or a
    /// federated change still scheduled keeps the rate. A `VO1C` member may
    /// read no rate marker, so under `VO1C` the rate also stays until every
    /// active source has reported; under `VO2C` every member reads them. Each
    /// change is a marker after the head's frame end; later frames are due at
    /// the new rate from its append time.
    fn adjust_pace(&mut self, now_ms: u64, start_ms: u64, federated: bool) {
        if self.certificate.pacing == CertificatePacing::Off
            || !self.pace.scheduled.is_empty()
            || (self.certificate.pacing == CertificatePacing::Legacy
                && self.active_paces().any(|(_, report)| report.is_none()))
        {
            return;
        }
        let change = if self.certificate.pacing == CertificatePacing::Continuous {
            self.continuous_change(now_ms, start_ms)
        } else {
            self.row_change(now_ms, start_ms)
        };
        if let Some((rate, source)) = change {
            self.change_rate(rate, source, now_ms, start_ms, federated);
        }
    }

    /// Each active source with its latest pace report, if it sent one.
    fn active_paces(&self) -> impl Iterator<Item = (u8, Option<PaceReport>)> {
        self.sources
            .iter()
            .enumerate()
            .filter(|&(index, _)| self.active & (1 << index) != 0)
            .map(|(index, source)| (index as u8, source.pace))
    }

    /// Active sources' reports at most `PACE_REPORT_MS` old.
    fn fresh_paces(&self, now_ms: u64) -> impl Iterator<Item = (u8, PaceReport)> {
        self.active_paces().filter_map(move |(source, report)| {
            report
                .filter(|report| now_ms.saturating_sub(report.at_ms) <= PACE_REPORT_MS)
                .map(|report| (source, report))
        })
    }

    /// The player both pacing modes slow for: the slowest Behind reporter
    /// whose simulation cannot hold the clock's rate, as its holdable rate,
    /// the frames it ran and its source. Also whether any reporter that
    /// cannot hold the rate is behind or catching up.
    fn slowest_behind(&self, now_ms: u64) -> (Option<(u32, u16, u8)>, bool) {
        let rate = self.pace.rate;
        let mut slowest: Option<(u32, u16, u8)> = None;
        let mut lagging = false;
        for (source, report) in self.fresh_paces(now_ms) {
            let sustainable = report.holdable_rate(rate);
            // A tick-cost reporter that holds the current rate is only
            // drawing slowly: its own catch-up recovers it, and it neither
            // slows the clock nor holds back a raise. Unless it stays behind
            // running fewer frames than the clock: then what it ran is what
            // it holds (`PaceReport::measured_rate`).
            let simulation_bound = report.tick_micros.is_none() || sustainable < u32::from(rate);
            lagging |= report.pace != OrderPace::KeepingUp && simulation_bound;
            // Ties keep the slowest that actually ran, as legacy reports did.
            let slowness = (sustainable, report.frames_per_second);
            if report.pace == OrderPace::Behind
                && simulation_bound
                && slowest.is_none_or(|(held, ran, _)| slowness < (held, ran))
            {
                slowest = Some((sustainable, report.frames_per_second, source));
            }
        }
        (slowest, lagging)
    }

    /// [`Self::adjust_pace`] in Game Speed rows: the rate and the source that
    /// slowed it (255 for a raise) of the change due now, if any. A player
    /// behind slows the clock to the fastest paced rate it holds
    /// (`PaceReport::holdable_rate`), at least one row slower, at most two
    /// below the certificate's. Once nobody has been behind or catching up
    /// for the raise wait, and every tick-cost reporter holds the next
    /// faster row, the clock tries that row.
    fn row_change(&mut self, now_ms: u64, start_ms: u64) -> Option<(u16, u8)> {
        let rates = paced_rates(self.certificate.tick_hz);
        let step = rates
            .iter()
            .position(|&rate| rate == self.pace.rate)
            .expect("the clock runs at a paced rate");
        let (slowest, lagging) = self.slowest_behind(now_ms);
        let raise_blocked = step > 0
            && self.fresh_paces(now_ms).any(|(_, report)| {
                report.tick_micros.is_some()
                    && (report.holdable_rate(self.pace.rate) < u32::from(rates[step - 1])
                        || report.ran_below(rates[step - 1], now_ms))
            });
        let changed_at = self.pace.step_due(lagging, now_ms, start_ms)?;
        Some(match slowest {
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
            _ => return None,
        })
    }

    /// [`Self::adjust_pace`] under continuous pacing. A player behind, by its
    /// tick cost or for `SUSTAINED_BEHIND_MS` by what it ran, slows the clock
    /// to the slowest such player's holdable rate, at least one frame slower,
    /// at most `CONTINUOUS_STEP_DOWN_PERCENT` slower at a time and not below
    /// `CONTINUOUS_FLOOR`. Otherwise a player with a standing
    /// backlog (`STANDING_BACKLOG_MS`) that runs no more than a twentieth
    /// over the clock slows it by a twentieth, at least one frame, so the
    /// backlog drains. After the raise wait with nobody
    /// behind or catching up, the clock goes half the way up to the slowest
    /// raise ceiling (`PaceReport::raise_ceiling`), at least one frame and at
    /// most a tenth of its rate, never past the certificate's rate.
    fn continuous_change(&mut self, now_ms: u64, start_ms: u64) -> Option<(u16, u8)> {
        let (ceiling, rate) = (self.certificate.tick_hz, self.pace.rate);
        let floor = CONTINUOUS_FLOOR.min(ceiling);
        let (slowest, mut lagging) = self.slowest_behind(now_ms);
        let mut standing = None;
        let mut hold = u32::from(ceiling);
        for (source, report) in self.fresh_paces(now_ms) {
            let standing_backlog = report.standing_backlog();
            lagging |= report.pace != OrderPace::KeepingUp && standing_backlog;
            // One that runs a twentieth more than the clock drains its
            // backlog itself; slowing for it would not stop.
            if standing_backlog
                && standing.is_none()
                && u32::from(report.frames_per_second) * 20 <= u32::from(rate) * 21
            {
                standing = Some(source);
            }
            hold = hold.min(report.raise_ceiling(now_ms));
        }
        let changed_at = self.pace.step_due(lagging, now_ms, start_ms)?;
        let deepest = u32::from(rate) - u32::from(rate) * CONTINUOUS_STEP_DOWN_PERCENT / 100;
        match slowest {
            Some((sustainable, _, source)) if rate > floor => Some((
                sustainable
                    .max(deepest)
                    .clamp(u32::from(floor), u32::from(rate) - 1) as u16,
                source,
            )),
            // A slowdown's own drain takes time: judge a standing backlog
            // only once the last change has had as long.
            None if let Some(source) = standing
                && rate > floor
                && now_ms.saturating_sub(changed_at) >= STANDING_BACKLOG_MS =>
            {
                Some(((rate * 19 / 20).clamp(floor, rate - 1), source))
            }
            None if rate < ceiling
                && now_ms.saturating_sub(self.pace.lagging_at.max(changed_at))
                    >= self.pace.raise_wait / CONTINUOUS_RAISE_WAIT_DIVISOR =>
            {
                // Half the way, as Spring's speed control smooths, and no
                // more than a tenth: Age of Empires' clock "would slowly
                // settle back" (Bettner and Terrano, 2001).
                let hold = hold as u16;
                (hold > rate).then(|| {
                    let step = ((hold - rate) / 2).clamp(1, (rate / 10).max(1));
                    (rate + step, u8::MAX)
                })
            }
            _ => None,
        }
    }

    /// Appends the rate change [`Self::adjust_pace`] chose: `source` slowed
    /// the clock, or 255 raised it.
    fn change_rate(&mut self, rate: u16, source: u8, now_ms: u64, start_ms: u64, federated: bool) {
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
            pace.anchor = Some((self.head, now_ms - start_ms));
        }
    }

    /// A federated rate change takes effect when its frame falls due on the
    /// old schedule; later frames are due at the new rate from there. The
    /// schedule counts from the start, so every relay derives the same one.
    fn apply_scheduled_rate(&mut self, now_ms: u64, start: u64) {
        while let Some(&(frame, rate, _)) = self.pace.scheduled.front() {
            let (anchor, offset) = self.pace.anchor.unwrap_or((0, 0));
            let due =
                offset + u64::from(frame.saturating_sub(anchor)) * 1000 / u64::from(self.pace.rate);
            if now_ms < start + due {
                return;
            }
            self.pace.rate = rate;
            self.pace.anchor = Some((frame, due));
            self.pace.scheduled.pop_front();
        }
    }

    /// About when `frame` was due: exact from the last rate change on, at
    /// the certificate's rate from the start before it.
    fn frame_due_ms(&self, frame: u32) -> u64 {
        let start = self.starts_at.unwrap_or(0);
        match self.pace.anchor_at(start) {
            Some((anchor, at)) if frame >= anchor => {
                at.saturating_add(u64::from(frame - anchor) * 1000 / u64::from(self.pace.rate))
            }
            _ => {
                start.saturating_add(u64::from(frame) * 1000 / u64::from(self.certificate.tick_hz))
            }
        }
    }

    /// Whether other relays home some of the sources. A federated room exists
    /// only under a roster that homes exactly the certificate's sources on at
    /// least two relays (Ready), so it never homes them all here.
    fn federated(&self) -> bool {
        self.local != self.certificate.sources_mask
    }

    fn supported_pause_sources(&self) -> u32 {
        if self.federated() {
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
        // short hop tickets. Each source advertises once its match started.
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
            if !self.federated() {
                // An explicit Leave before the start cancels it for every
                // source, including sources which never reached Ready. Keep an
                // inactive short-ticket tombstone.
                self.active = 0;
                self.ready = 0;
                self.finished_at = Some(now_ms);
                return;
            }
            // Only the coordinator ends a federated startup for an ordinary
            // reason. Its start may already be on the way; this Leave then
            // departs at frame 1.
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
        if !self.federated()
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
                if self.federated() {
                    self.cancel_requested = true;
                } else {
                    self.retire(source, true, now_ms);
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

    /// A coordinator cancellation or proved mismatch ends this startup.
    fn cancel(&mut self, now_ms: u64) {
        self.active = 0;
        // Retain the authenticated Ready/agreement proof for other relays.
        // Cancellation still wins over starting, even on older relays.
        self.finished_at = Some(now_ms);
    }

    /// A Ready, here or on another relay, proved a different initial world:
    /// cancel the startup and answer every later Ready, and a ready source's
    /// Pull and UPLINK, with `AgreementMismatch`. A federated relay's
    /// cancelled startup asks the coordinator to cancel it everywhere. Only a
    /// startup this ends is counted, not one already cancelled otherwise.
    fn refuse_start(&mut self, now_ms: u64) {
        self.agreement_mismatch = true;
        self.mismatch_uncounted = self.finished_at.is_none();
        self.cancel(now_ms);
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

    /// VERA: active sources not heard from for `SILENT_NOTICE_MS` since the
    /// start, of those whose silence this relay knows: its own, and those of
    /// federated relays that report their players' (README "Silence").
    pub(super) fn silent_sources(
        &self,
        federation: Option<&federation::Federation>,
        now_ms: u64,
    ) -> u32 {
        if self.starts_at.is_none_or(|start| start > now_ms) {
            return 0;
        }
        let known = self.local | federation.map_or(0, federation::Federation::reporting_silence);
        (0..MAX_PEERS)
            .filter(|&source| {
                self.active & known & (1 << source) != 0
                    && now_ms.saturating_sub(self.sources[usize::from(source)].heard_ms)
                        >= SILENT_NOTICE_MS
            })
            .fold(0, |mask, source| mask | 1 << source)
    }

    /// When this relay last heard each active source homed here, as
    /// `[source][LE age u32]` entries for the other relays of the match.
    pub(super) fn local_silence(&self, now_ms: u64) -> Vec<u8> {
        let mut encoded = Vec::new();
        for (index, source) in self.sources.iter().enumerate() {
            if self.active & self.local & (1 << index) == 0 {
                continue;
            }
            encoded.push(index as u8);
            let age = now_ms
                .saturating_sub(source.heard_ms)
                .min(u64::from(u32::MAX)) as u32;
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
            if !admits_rate(self.certificate.tick_hz, self.certificate.pacing, rate) {
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
        if reports.len() == active.count_ones() as usize
            && reports.iter().all(|&(_, value)| value == checksum)
        {
            self.watch.agree(frame, checksum);
        }
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
        // Before this pass seals anything: every record before the mark was
        // committed by `now_ms`.
        self.watch
            .mark(now_ms, self.head, self.next_seq, self.active);
        if self.storage_failed {
            self.failed_at.get_or_insert(now_ms);
            return;
        }
        if self.terminal.is_some() {
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
        let (anchor, anchor_ms) = self.pace.anchor_at(start).unwrap_or((0, start));
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
            self.adjust_pace(now_ms, start, federated);
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
        now: u64,
        archive_pulls: &mut RateBudget,
    ) -> Result<Vec<Vec<u8>>, RelayError> {
        if self.storage_failed {
            return Ok(error(OrderError::StorageUnavailable, self.next_seq));
        }
        self.authenticate_reader(ticket)?;
        if let Some(reply) = self.federation_failure_reply(now_ms) {
            return Ok(reply);
        }
        if self.agreement_mismatch {
            return Ok(error(OrderError::AgreementMismatch, 0));
        }
        let Some(&kind) = payload.first() else {
            return Err(RelayError::Malformed);
        };
        // `[2][next varint]` (compaction spec §7.4).
        let mut reader = Reader(&payload[1..]);
        match (kind, reader.varint()) {
            (PULL_REQUEST, Ok(next)) if reader.is_empty() => {
                // The read happens even if its reply then exceeds an output
                // budget, so the sender pays for it first.
                if next < self.hot_oldest_seq() {
                    if !archive_pulls.permits(now, 1, PLAYER_ARCHIVE_PULLS_PER_SECOND) {
                        return Err(RelayError::RateLimited);
                    }
                    archive_pulls.charge(now, 1);
                }
                match self.history_page(next, ORDERING_PULL_LIMIT) {
                    Ok(records) => {
                        let mut output = vec![self.status(now_ms)];
                        output.extend(
                            pack::encode_pack(&records, records.len())
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
                let source = &mut ordering.sources[usize::from(message.ticket.source)];
                source.bind((message.ticket.player, message.ticket.generation))?;
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
                self.closed_startups.close(message.ticket.room, now);
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
        // Each room's departure touches only that room.
        let mut departed = false;
        for (_, room) in self.rooms.iter_mut().filter(|(id, _)| **id != ticket.room) {
            let Some(ordering) = room
                .ordering
                .as_mut()
                .filter(|ordering| ordering.active != 0 && !ordering.storage_failed)
            else {
                continue;
            };
            let superseded = ordering.sources.iter().position(|source| {
                source.principal.is_some_and(|(player, generation)| {
                    player == ticket.player && generation < ticket.generation
                })
            });
            if let Some(source) = superseded {
                ordering.depart(source as u8, now_ms);
                departed = true;
            }
        }
        if departed {
            self.cleanup_ordering(now_ms, now);
        }
    }

    /// Driven by the server clock even when no datagram arrives.
    pub fn advance_clock(&mut self, now_ms: u64, now: u64) {
        self.prune(now, now_ms);
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
                total = self.evict_ordering(0);
            }
        }
        self.cleanup_ordering(now_ms, now);
        // Cleanup only frees, so the pass's count still bounds what is held.
        self.ordering_estimate = total;
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
        // The loop below removes rooms, so it looks each up again.
        for id in ended {
            let Some(room) = self.rooms.get_mut(&id) else {
                continue;
            };
            let Some(ordering) = room.ordering.as_mut() else {
                continue;
            };
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
            self.closed_startups.close(id, now);
            if let Some(receipt) = ordering
                .terminal
                .take()
                .and_then(federation::Terminal::into_unsaved)
            {
                if self.retired_outcomes.len() < self.max_rooms && now <= receipt.expires_at() {
                    self.retired_outcomes.insert(id, receipt);
                } else {
                    self.ordering_counts.outcome_handoff_drops += 1;
                }
            }
            room.ordering = None;
            if room.vacant() {
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
            let grown = ordering.memory().saturating_sub(before);
            if grown > 0 {
                self.ordering_estimate = self.ordering_estimate.saturating_add(grown);
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

    /// Room for `additional` more ordering bytes under
    /// `GLOBAL_ORDERING_BYTES`. Within `ordering_estimate` it takes no count;
    /// past it, every room is counted and the oldest history evicted. False
    /// when that still leaves no room.
    fn reserve_ordering(&mut self, additional: usize) -> bool {
        let estimate = self.ordering_estimate.saturating_add(additional);
        if estimate <= GLOBAL_ORDERING_BYTES {
            self.ordering_estimate = estimate;
            return true;
        }
        let total = self.evict_ordering(additional);
        let reserved = total.saturating_add(additional) <= GLOBAL_ORDERING_BYTES;
        self.ordering_estimate = if reserved { total + additional } else { total };
        reserved
    }

    /// Counts every room's ordering bytes and evicts the oldest history,
    /// across rooms, until `additional` more fit under
    /// `GLOBAL_ORDERING_BYTES` or no history is left; returns what remains.
    fn evict_ordering(&mut self, additional: usize) -> usize {
        let mut total = self.ordering_memory();
        while total.saturating_add(additional) > GLOBAL_ORDERING_BYTES {
            let oldest = self
                .rooms
                .iter_mut()
                .filter_map(|(id, room)| {
                    let ordering = room.ordering.as_mut()?;
                    let due = ordering.frame_due_ms(ordering.history.front()?.frame);
                    Some(((due, *id), ordering))
                })
                .min_by_key(|(key, _)| *key);
            let Some((_, ordering)) = oldest else {
                break;
            };
            let before = ordering.memory();
            ordering.pop_oldest();
            total -= before - ordering.memory();
        }
        total
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
            // The lobby signs every one-source match's certificate for
            // released clients, which run it locally: order one only when the
            // lobby marked it (a person watching AI play, for watchers).
            if certificate.room != message.ticket.room
                || certificate.sources_mask & (1 << message.ticket.source) == 0
                || !certificate.relay_orders()
            {
                return Err(RelayError::Authentication);
            }
            if self.closed_startups.contains(&message.ticket.room) {
                return self.ordering_reply(
                    message.ticket,
                    error(OrderError::Inactive, 0),
                    now,
                    now_ms,
                    crate::link::SEND_OTHER,
                );
            }
            // The lobby's roster partitions the certificate's sources over
            // the relays (`OrderingRoom::federated`).
            if self
                .rooms
                .get(&message.ticket.room)
                .and_then(|room| room.federation.as_ref())
                .is_some_and(|federation| federation.sources() != certificate.sources_mask)
            {
                return self.ordering_reply(
                    message.ticket,
                    error(OrderError::AgreementMismatch, 0),
                    now,
                    now_ms,
                    crate::link::SEND_OTHER,
                );
            }
            let agreement: [u8; 32] = field(payload, 1 + certificate_len)?;
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
                        source.heard_ms = peer.link.last_heard_ms;
                    }
                }
                ordering
            });
            if let Some(reply) = ordering.federation_failure_reply(now_ms) {
                reply
            } else if ordering.storage_failed {
                error(OrderError::StorageUnavailable, ordering.next_seq)
            } else if ordering.agreement_mismatch
                || !ordering.certificate.same_match(&certificate)
                || ordering.agreement != agreement
            {
                // VERA: a loading player's Ready disagrees on the signed match
                // settings or the initial world. Cancel the start for
                // everyone now, as its Leave could, instead of letting its
                // keepalives hold the others until the loading limit drops it
                // and they start without it. A ready source's agreement
                // stands, so no federated start can be on its way.
                let bit = 1 << message.ticket.source;
                if ordering.starts_at.is_none() && ordering.active & !ordering.ready & bit != 0 {
                    ordering.refuse_start(now_ms);
                }
                error(OrderError::AgreementMismatch, 0)
            } else if ordering.active & (1 << message.ticket.source) == 0 {
                error(OrderError::Inactive, 0)
            } else {
                ordering.sources[usize::from(message.ticket.source)]
                    .bind((message.ticket.player, message.ticket.generation))?;
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
                let admission = self
                    .admissions
                    .get_mut(&message.ticket.player)
                    .ok_or(RelayError::Unregistered)?;
                let replies = ordering.request(
                    message.ticket,
                    payload,
                    now_ms,
                    now,
                    &mut admission.archive_pulls,
                )?;
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
        if let Some(reply) = ordering.federation_failure_reply(now_ms) {
            return self.ordering_reply(ticket, reply, now, now_ms, crate::link::SEND_OTHER);
        }
        if ordering.agreement_mismatch {
            return self.ordering_reply(
                ticket,
                error(OrderError::AgreementMismatch, 0),
                now,
                now_ms,
                crate::link::SEND_OTHER,
            );
        }
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
        if let Some(report) = uplink.pace {
            ordering.report_pace(ticket.source, report, now_ms);
            if report.pace != OrderPace::KeepingUp {
                self.ordering_counts.catching_up_s += 1;
            }
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
    /// ACK get the hop feedback tail as they are encoded (spec §4.1), and a
    /// STATUS then the silent sources if any (README "Silence"); PACKs are
    /// logged as `pack_kind`, everything else as non-PACK (spec §4.2).
    fn ordering_reply(
        &mut self,
        ticket: Ticket,
        mut replies: Vec<Vec<u8>>,
        now: u64,
        now_ms: u64,
        pack_kind: u8,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        if let Some(room) = self.rooms.get(&ticket.room)
            && let Some(peer) = room.peers.iter().find(|peer| peer.ticket == ticket)
        {
            let feedback = peer.link.feedback(now_ms);
            let silent = room.ordering.as_ref().map_or(0, |ordering| {
                ordering.silent_sources(room.federation.as_ref(), now_ms)
            });
            for reply in &mut replies {
                if matches!(reply.first(), Some(&STATUS_EVENT | &ACK_EVENT)) {
                    crate::link::append_feedback(reply, feedback);
                }
                if reply.first() == Some(&STATUS_EVENT) && silent != 0 {
                    reply.extend_from_slice(&silent.to_le_bytes());
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
            || !admission
                .order_packets
                .permits(now, packets, PLAYER_PACKETS_PER_SECOND)
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

#[cfg(test)]
mod tests;
