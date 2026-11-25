//! VERA online ordering adaptation. The authenticated relay seals an immutable
//! timeline; no client infers a missing participant's input or rewrites a tick.
//! Native Event/OutList/DoList and the ordinary simulation transaction retain
//! gameplay authority. Fresh packet caches make each batch standalone.

use std::collections::{BTreeMap, VecDeque};

use vera20k_relay_protocol::{
    CHECKSUM_INTERVAL, CertificatePacing, ClientLinkStats, MAX_FEDERATED_RELAYS, MAX_FRAME_BATCHES,
    MAX_PEERS, ORDERING_BATCH_CAPACITY, OrderEntry, OrderError, OrderEvent, OrderPace,
    OrderingCertificate, OrderingGap, PACE_INTERVAL_MS, PaceLag, READY_REQUEST_INTERVAL_MS,
    REORDER_WINDOW, RESTORE_PULL_INTERVAL_MS, WATCH_MARK_MS, admits_rate,
};

use super::{LiveCommandError, StartupAgreement, StartupError};
use crate::net::event_packet::{EVENT_PACKET_HEADER_LEN, EventPacketCodec};
use crate::net::udp::LanUdpTransport;
use crate::sim::command::{
    ADD_PLAYER_OPCODE, CommandRecord, EMPTY_OPCODE, ExitRecord, FRAME_SYNC_OPCODE,
    GAME_SPEED_OPCODE, LATENCY_FUDGE_OPCODE, MESSAGE_OPCODE, PROCESS_TIME_OPCODE,
    REMOVE_PLAYER_OPCODE, RESPONSE_TIME_OPCODE, TIMING_OPCODE,
};
use crate::sim::command_queue::{FRAMEINFO_EVENT_OPCODE, FrameInfo, OUT_LIST_CAPACITY};
use crate::sim::world::{
    NetworkCommandQueues, NetworkInterruption, NetworkQueueContext, NetworkQueuePass,
};
use crate::util::time::Instant;

const BUFFERED_FRAMES: usize = 64;
// The most records a relay's frame carries: each federated relay's batches,
// each batch at most a whole OutList (`decode_batch`), and one leaving record
// per source.
const MAX_FRAME_RECORDS: usize =
    MAX_FRAME_BATCHES * MAX_FEDERATED_RELAYS * OUT_LIST_CAPACITY + MAX_PEERS as usize;
const SAMPLE_HISTORY: usize = 4096;
const PAUSE_PROBE_INTERVAL_MS: i32 = 500;
const NARRATIVE_PAUSE_RETRY_MS: i32 = 1_000;
const COMMAND_TRACE_HISTORY: usize = 256;
// A relay record this build cannot read ends the match once it has held the
// cursor, or before the start every Status, this long while the relay keeps
// repeating it. Silence alone never ends a match.
const UNREADABLE_LIMIT_MS: u32 = 5_000;
const UNREADABLE_REPEAT_MS: u32 = 1_000;
// Ticks kept for one pace report's tick-cost percentile.
const MAX_TICK_COSTS: usize = 256;
// VERA watching (relay README "Watchers"): the relay never pushes to a
// watcher, so it pulls this often. It starts a whole match behind and keeps
// every frame it reads, so it has read a match to its end before the relay
// forgets it (300 s after its end), however long its world takes to play it.
const WATCH_PULL_INTERVAL_MS: u32 = 100;
const WATCH_REORDER_WINDOW: u64 = 2048;
// The relay's agreed samples kept until this world reaches their frames.
const WATCH_SAMPLES: usize = 16;
// A watcher that hears nothing from its relay this long has lost the match:
// the relay dropped it long before.
const WATCH_SILENCE_MS: u32 = 60_000;

/// One match's ordered delivery as the app reads it: a copy of what the
/// session that owns it counted and of its lifecycle, which the session itself
/// decides from. Wall-clock observations; never simulation or delivery input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeliveryCounters {
    /// The relay session's link measurements as last read (spec §10.1),
    /// including the stream gaps it was told of and its fallback Pulls.
    pub link: Option<ClientLinkStats>,
    /// Local command batches whose sealed frame came back, with the
    /// first-transmission to frame-receipt time of each.
    pub round_trips: u64,
    pub round_trip_total_ms: u64,
    pub round_trip_max_ms: u64,
    /// The sampled frame whose checksums differed, from the relay's DESYNC.
    pub desync_frame: Option<u32>,
    /// The relay's clock for this match has started, even if the relay
    /// lost the match later (the session's `ever_started`).
    pub started: bool,
    /// The name (`SequencedError::code`) of the error that ended the match
    /// here (the session's `failure`).
    pub failure: Option<&'static str>,
}

/// What the session counts for `DeliveryCounters`.
#[derive(Debug, Clone, Copy, Default)]
struct DeliveryCounts {
    link: Option<ClientLinkStats>,
    round_trips: u64,
    round_trip_total_ms: u64,
    round_trip_max_ms: u64,
    desync_frame: Option<u32>,
}

/// The error that ended a match here. It stays for the rest of the session:
/// `poll` hands out the error itself once and `LiveCommandError::Failed`
/// after, so every later read still sees a failed match.
#[derive(Debug)]
struct Failure {
    /// `SequencedError::code`, or `local_error` for one of this PC's own.
    code: &'static str,
    unreported: Option<LiveCommandError>,
}

impl Failure {
    fn new(error: LiveCommandError) -> Self {
        let code = match &error {
            LiveCommandError::Sequenced(error) => error.code(),
            _ => "local_error",
        };
        Self {
            code,
            unreported: Some(error),
        }
    }
}

/// App diagnostics for one local command batch: its first transmission, the
/// relay's frame assignment and the contiguous receipt of that sealed frame.
/// Socket receipt times; this never enters simulation or delivery decisions.
#[derive(Debug, Clone, Copy)]
pub struct CommandDeliveryTrace {
    pub batch_id: u64,
    /// When the relay session sealed it, to go out with its next UPLINK.
    pub sent_at: Instant,
    pub assigned: Option<(u32, Instant)>,
    pub frame_received_at: Option<Instant>,
}

/// The socket's latest contiguous-frame receipt, independent of when the app
/// drains it. Presentation can pace these frames; only the sequencer admits
/// canonical commands. This clock never enters deterministic simulation.
#[derive(Debug, Clone, Copy)]
pub struct SequencedTiming {
    pub frame: u32,
    pub received_at: Instant,
    pub frames_per_second: u16,
    /// Changes at a synchronized pause/resume boundary. Presentation must
    /// not turn time spent paused into simulation catch-up or receipt jitter.
    pub clock_epoch: u64,
    /// Frames that reach this game together: the relay's turn for a player
    /// (`turn_frames`), more for a watcher (`watch_turn`).
    pub turn_frames: u32,
}

/// Authenticated relay capability and the latest contiguous pause marker.
/// A client still finishes `frame` before it is stopped; this state never
/// pauses a local simulation ahead of another participant's canonical input.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MultiplayerPauseStatus {
    pub available: bool,
    pub paused: bool,
    pub frame: u32,
    pub epoch: u64,
    pub pending: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrativeHold {
    Player,
    Host,
}

/// A relay rate change, released when its first frame dispatches. The relay
/// slows a match whose player cannot keep up (relay README, Pace).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchSpeedChange {
    /// This House's PC could not keep up, so the match runs slower.
    SlowedBy(i8),
    /// The match runs at its own speed again.
    Normal,
}

/// VERA: a shared pause or resume another player asked for, released when
/// the world reaches it: a pause once the frame it follows has dispatched, a
/// resume with the frame after it. The app names that player in a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchPauseChange {
    /// The House of the player who asked.
    pub house: i8,
    pub paused: bool,
}

/// VERA: another player the relay has not heard from for 10 seconds, or
/// hears again while they are still in the match (relay README "Silence").
/// The app names them in a line. gamemd holds the match for a silent player
/// and names them in its Wait_For_Players dialog (called from ProcessNetwork
/// `0x0064800E`); the relay plays on without them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchConnectionChange {
    pub house: i8,
    pub responding: bool,
}

/// This PC's progress against the relay's clock, reported about once a
/// second as [`PaceLag`] judges it.
#[derive(Debug, Default)]
struct PaceMeter {
    lag: PaceLag,
    last_report: Option<u32>,
    dispatched: u32,
    // Simulation tick costs, in microseconds, since the last report.
    tick_costs: Vec<u32>,
    // A minimised or covered window may be throttled by its system.
    hidden: bool,
}

impl PaceMeter {
    /// One simulation tick's cost, for the next report's 90th percentile.
    fn record_tick_cost(&mut self, micros: u32) {
        // A report window holds about a second of ticks; a stalled report
        // keeps the latest.
        if self.tick_costs.len() == MAX_TICK_COSTS {
            self.tick_costs.remove(0);
        }
        self.tick_costs.push(micros);
    }

    fn tick_cost_p90(&mut self) -> Option<u32> {
        if self.tick_costs.is_empty() {
            return None;
        }
        let rank = (self.tick_costs.len() * 9).div_ceil(10) - 1;
        let (_, p90, _) = self.tick_costs.select_nth_unstable(rank);
        let p90 = *p90;
        self.tick_costs.clear();
        Some(p90.max(1))
    }

    /// Samples the frames waiting here and returns the report due now, with
    /// the frames run per second since the last report.
    fn report(
        &mut self,
        now: u32,
        buffered: usize,
        pace_fps: u16,
        measured: bool,
    ) -> Option<(OrderPace, u16, Option<u32>)> {
        // Frames waiting in an unmeasured or hidden window say nothing of
        // this PC's speed.
        let waiting = if measured && !self.hidden {
            buffered
        } else {
            0
        };
        self.lag.observe(now, waiting, pace_fps);
        if !service_due(self.last_report, now, PACE_INTERVAL_MS as i32) {
            return None;
        }
        let ran = self.last_report.map_or(pace_fps, |last| {
            (u64::from(self.dispatched) * 1000 / u64::from(now.wrapping_sub(last).max(1)))
                .min(u64::from(u16::MAX)) as u16
        });
        self.last_report = Some(now);
        self.dispatched = 0;
        Some((
            self.lag.classify(now, ran, pace_fps),
            ran,
            self.tick_cost_p90(),
        ))
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CanonicalPause {
    paused: bool,
    frame: u32,
    epoch: u64,
}

#[derive(Debug)]
struct PauseRequest {
    id: u64,
    expected_epoch: u64,
    paused: bool,
    narrative: bool,
    last_sent: Option<u32>,
    acknowledged_epoch: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct Unreadable {
    since: u32,
    last: u32,
    next_sequence: u64,
    statuses: u64,
}

#[derive(Debug)]
struct ReceivedEntry {
    entry: OrderEntry,
    received_at: Instant,
}

/// How long a running match's ordered stream may stand still before the
/// client shows Reconnecting (and Quit leaves without the stream).
const STALL_MS: u32 = 5_000;

fn service_due(last: Option<u32>, now: u32, interval: i32) -> bool {
    last.is_none_or(|last| super::probe_due(last, now, interval))
}

#[derive(Debug, thiserror::Error)]
pub enum SequencedError {
    #[error("online command history expired; reconnect to a new match")]
    HistoryExpired,
    #[error("online command timeline is invalid")]
    InvalidTimeline,
    /// Worded like the LAN path's `ChecksumMismatch`, so either reads the
    /// same in a log: the House whose report differs and both checksums.
    #[error(
        "online simulation differs for house {house} at frame {frame}: \
         local {local:#010x}, remote {remote:#010x}"
    )]
    Desync {
        frame: u32,
        house: i8,
        local: u32,
        remote: u32,
    },
    #[error("online ordering service cannot recover this match")]
    Unavailable,
    /// The transport released the relay route: the relay answered nothing for
    /// longer than it keeps a silent player's place (`relay_absent`), so this
    /// game cannot come back to its match.
    #[error(
        "no answer from the online game server for ten minutes: this player's place in the match is gone"
    )]
    Absent,
    /// The relay closed this match's room before it started: a player left.
    #[error("a player left before the online match started")]
    StartCancelled,
    #[error("the match ended after the game servers lost contact with each other")]
    FederationLost,
    #[error("online command queue is full")]
    QueueFull,
    #[error("this online match needs a newer VERA20k")]
    NewerClientRequired,
    /// A watcher's world differs from the checksum every player reported
    /// for `frame` (relay README "Watchers").
    #[error(
        "this watcher's simulation differs from the players' at frame {frame}: \
         local {local:#010x}, players {remote:#010x}"
    )]
    WatchDesync { frame: u32, local: u32, remote: u32 },
    /// A watcher's relay answered nothing for `WATCH_SILENCE_MS`.
    #[error("no answer from the online game server: watching ended")]
    WatchLost,
    /// A refusal the session cannot act on, such as the games' start
    /// agreements differing.
    #[error("the online ordering service refused this match: {0:?}")]
    Refused(OrderError),
}

impl SequencedError {
    /// The name match reports give this error (`docs/lobby-protocol.md`).
    fn code(&self) -> &'static str {
        match self {
            Self::HistoryExpired => "history_expired",
            Self::InvalidTimeline => "invalid_timeline",
            Self::Desync { .. } => "desync",
            Self::Unavailable => "unavailable",
            Self::Absent => "absent",
            Self::StartCancelled => "start_cancelled",
            Self::FederationLost => "federation_lost",
            Self::QueueFull => "queue_full",
            Self::NewerClientRequired => "newer_client_required",
            Self::WatchDesync { .. } => "watch_desync",
            Self::WatchLost => "watch_lost",
            Self::Refused(OrderError::AgreementMismatch) => "agreement_mismatch",
            Self::Refused(OrderError::ModifiedRetry) => "modified_retry",
            Self::Refused(OrderError::InvalidCursor) => "invalid_cursor",
            Self::Refused(_) => "refused",
        }
    }

    /// What a match this error ended tells the player.
    pub fn interruption(&self) -> NetworkInterruption {
        match self {
            Self::Desync { .. } | Self::WatchDesync { .. } => NetworkInterruption::ScenariosDiffer,
            Self::QueueFull => NetworkInterruption::QueueFull,
            Self::NewerClientRequired => NetworkInterruption::NewerClientRequired,
            Self::Refused(OrderError::AgreementMismatch) => NetworkInterruption::StartMismatch,
            Self::StartCancelled => NetworkInterruption::StartCancelled,
            Self::FederationLost => NetworkInterruption::FederationLost,
            Self::HistoryExpired
            | Self::InvalidTimeline
            | Self::Unavailable
            | Self::Absent
            | Self::WatchLost
            | Self::Refused(_) => NetworkInterruption::ConnectionFailed,
        }
    }
}

/// A local batch whose records stay in OutList until the relay accepts it.
#[derive(Debug)]
struct Submission {
    records: Vec<CommandRecord>,
    batch: SubmissionBatch,
}

/// The relay session numbers a sealed batch, keeps its bytes, schedules its
/// UPLINKs and decides when the relay accepted it (spec §6.1-§6.4).
#[derive(Debug)]
enum SubmissionBatch {
    /// Encoded; the route could not take it yet.
    Unsealed(Vec<u8>),
    /// The ID the relay session sealed it as.
    Sealed(u64),
}

/// The relay's DESYNC record: the sampled frame and the outlier's and the
/// other side's source and checksum.
#[derive(Debug, Clone, Copy)]
struct RelayDesync {
    sample_frame: u32,
    outlier: (u8, u32),
    other: (u8, u32),
}

#[derive(Debug, Default)]
struct CommittedFrame {
    frame: u32,
    records: Vec<CommandRecord>,
    // Remote Houses whose connection the relay ended in this frame.
    lost: Vec<i8>,
    // A rate marker after the previous frame.
    speed: Option<MatchSpeedChange>,
    // Other players' pause markers the world reaches with this frame: a
    // resume after the previous frame, then a pause after this one.
    pauses: Vec<MatchPauseChange>,
    // A DESYNC record after the previous frame ends the match here.
    desync: Option<RelayDesync>,
}

/// VERA: a game that watches the match instead of playing in it (relay
/// README "Watchers"). Its `local_source` is the player it follows; it
/// pulls the stream and sends nothing into it.
#[derive(Debug)]
struct Watch {
    /// How far behind the players its relay holds the stream.
    delay_seconds: u32,
    /// The relay's checksums every player reported alike, by frame, kept
    /// until this world has its own for that frame.
    samples: VecDeque<(u32, u32)>,
    /// Sources whose departure this game has read.
    departed: u32,
}

#[derive(Debug)]
pub(super) struct SequencedSession {
    watch: Option<Watch>,
    houses: [Option<i8>; MAX_PEERS as usize],
    local_source: u8,
    agreement: [u8; 32],
    /// The certificate's rate, which the simulation keeps.
    fps: u16,
    /// The relay's current sealing rate, which paces playout.
    pub(super) pace_fps: u16,
    /// The certificate's pacing, which says what rates the relay may seal at
    /// (`admits_rate`).
    pacing: CertificatePacing,
    pace_meter: PaceMeter,
    /// The relay runs this match's clock now. A relay that lost the match's
    /// configuration (NotConfigured) clears it until it is configured again.
    started: bool,
    /// The relay has run this match's clock at some point; this never clears.
    ever_started: bool,
    head_frame: u32,
    failure: Option<Failure>,
    last_response: Option<u32>,
    /// When the ordered stream last moved (its head, next sequence or our
    /// cursor). A relay can keep answering while its stream stands still:
    /// federated relays whose link failed.
    last_progress: Option<(u32, (u32, u64, u64))>,
    first_service: Option<u32>,
    statuses: u64,
    unreadable: Option<Unreadable>,
    last_control: Option<u32>,
    remote_next_sequence: u64,
    delivery: DeliveryCounts,
    pause: CanonicalPause,
    /// VERA: the current pause found this game catching up, and it has not
    /// reached the pause yet; see `take_caught_up_in_pause`.
    pause_found_behind: bool,
    pause_supported_sources: u32,
    pause_active_sources: u32,
    /// The relay will never pause this match: its players are on several
    /// relays. This game stops asking.
    pause_refused: bool,
    last_pause_control: Option<u32>,
    pause_request: Option<PauseRequest>,
    next_pause_request: u64,
    narrative_hold: Option<NarrativeHold>,
    narrative_pause_epoch: Option<u64>,
    late_narrative_pause_epoch: Option<u64>,
    suppress_narrative_pause_notice: bool,
    last_narrative_pause_attempt: Option<u32>,
    keep_running_for_exit: bool,
    next_sequence: u64,
    entries: BTreeMap<u64, ReceivedEntry>,
    contiguous_received_at: Option<Instant>,
    latest_frame_receipt: Option<SequencedTiming>,
    assembling: CommittedFrame,
    frames: VecDeque<CommittedFrame>,
    last_batch: [u64; MAX_PEERS as usize],
    // Sources whose batch could not be read; each departed at that batch's
    // frame and the rest of its stream is ignored.
    refused: u32,
    // This process replaces one that left a match under way. Its batch counter
    // lies in the history it is reading, so it submits nothing until caught up.
    restoring: bool,
    last_restore_log: Option<u32>,
    submissions: VecDeque<Submission>,
    sample_sent: Option<u32>,
    // Derived checksum samples from immutable post-Logic contexts, evicted in
    // frame order: the reports to the relay, and this PC's side of a desync.
    samples: VecDeque<(u32, u32)>,
    current: Option<(NetworkQueueContext, bool)>,
    sent_commands: u64,
    command_traces: VecDeque<CommandDeliveryTrace>,
    // Released when their frame dispatches; the app posts each line.
    lost: Vec<i8>,
    speed_changes: Vec<MatchSpeedChange>,
    pause_changes: Vec<MatchPauseChange>,
    // Other players the relay last said it has not heard from lately, and
    // the changes the app has yet to post.
    silent: u32,
    connection_changes: Vec<MatchConnectionChange>,
}

impl SequencedSession {
    /// `cert` is the transport's certificate, as `super::relay_clock` read it.
    pub(super) fn new(
        agreement: &StartupAgreement,
        transport: &LanUdpTransport,
        local_house: i8,
        cert: &OrderingCertificate,
    ) -> Result<Self, LiveCommandError> {
        let watch_delay = transport.relay_watch_delay();
        let player_source = match watch_delay {
            Some(_) => None,
            None => Some(
                transport
                    .relay_source()
                    .ok_or(StartupError::InvalidAdmission)?,
            ),
        };
        let mut houses = [None; MAX_PEERS as usize];
        if let Some(source) = player_source {
            houses[usize::from(source)] = Some(local_house);
        }
        for (house, endpoint) in transport.connections().peers() {
            let source = vera20k_relay_protocol::endpoint_peer(endpoint)
                .ok_or(StartupError::InvalidAdmission)?;
            houses[usize::from(source)] =
                Some(i8::try_from(house).map_err(|_| StartupError::InvalidAdmission)?);
        }
        let local_source = match player_source {
            Some(source) => source,
            None => {
                let (source, seat) = watched_source(cert.sources_mask, roster_mask(&houses))
                    .filter(|&(source, _)| usize::from(source) < houses.len())
                    .ok_or(StartupError::InvalidAdmission)?;
                if seat {
                    houses[usize::from(source)] = Some(local_house);
                }
                source
            }
        };
        let mask = roster_mask(&houses);
        let (identity, fps, count) = agreement.sequencing_identity()?;
        if mask != cert.sources_mask || fps != cert.tick_hz || count != mask.count_ones() as usize {
            return Err(StartupError::InvalidAdmission.into());
        }
        let mut session = Self::with_roster(houses, local_source, identity, cert.tick_hz);
        session.pacing = cert.pacing;
        session.watch = watch_delay.map(|delay_seconds| Watch {
            delay_seconds,
            samples: VecDeque::new(),
            departed: 0,
        });
        Ok(session)
    }

    fn with_roster(
        houses: [Option<i8>; MAX_PEERS as usize],
        local_source: u8,
        agreement: [u8; 32],
        fps: u16,
    ) -> Self {
        let mask = roster_mask(&houses);
        Self {
            watch: None,
            houses,
            local_source,
            agreement,
            fps,
            pace_fps: fps,
            pacing: CertificatePacing::On,
            pace_meter: PaceMeter::default(),
            started: false,
            ever_started: false,
            head_frame: 0,
            failure: None,
            last_response: None,
            last_progress: None,
            first_service: None,
            statuses: 0,
            unreadable: None,
            last_control: None,
            remote_next_sequence: 1,
            delivery: DeliveryCounts::default(),
            pause: CanonicalPause::default(),
            pause_found_behind: false,
            pause_supported_sources: 0,
            pause_active_sources: mask,
            pause_refused: false,
            last_pause_control: None,
            pause_request: None,
            next_pause_request: 1,
            narrative_hold: None,
            narrative_pause_epoch: None,
            late_narrative_pause_epoch: None,
            suppress_narrative_pause_notice: false,
            last_narrative_pause_attempt: None,
            keep_running_for_exit: false,
            next_sequence: 1,
            entries: BTreeMap::new(),
            contiguous_received_at: None,
            latest_frame_receipt: None,
            assembling: CommittedFrame {
                frame: 1,
                ..Default::default()
            },
            frames: VecDeque::new(),
            last_batch: [0; MAX_PEERS as usize],
            refused: 0,
            restoring: false,
            last_restore_log: None,
            submissions: VecDeque::new(),
            sample_sent: None,
            samples: VecDeque::new(),
            current: None,
            sent_commands: 0,
            command_traces: VecDeque::new(),
            lost: Vec::new(),
            speed_changes: Vec::new(),
            pause_changes: Vec::new(),
            silent: 0,
            connection_changes: Vec::new(),
        }
    }

    /// A watcher joins a match under way too, but sends nothing: it has no
    /// batch counter to restore.
    pub(super) fn begin_restore(&mut self) {
        self.restoring = self.watch.is_none();
    }

    pub(super) fn restoring(&self) -> bool {
        self.restoring
    }

    /// This game watches the match instead of playing in it.
    pub(super) fn watching(&self) -> bool {
        self.watch.is_some()
    }

    /// A watcher's match is over: every player's departure has been read and
    /// every frame before it handed to the world.
    pub(super) fn watch_over(&self) -> bool {
        self.watch.as_ref().is_some_and(|watch| {
            (watch.departed | self.refused) == roster_mask(&self.houses)
                && self.frames.is_empty()
                && self.assembling.records.is_empty()
        })
    }

    fn buffered_frames(&self) -> usize {
        if self.watch.is_some() {
            usize::MAX
        } else {
            BUFFERED_FRAMES
        }
    }

    fn reorder_window(&self) -> u64 {
        if self.watch.is_some() {
            WATCH_REORDER_WINDOW
        } else {
            REORDER_WINDOW
        }
    }

    /// Frames that reach this game together (`SequencedTiming::turn_frames`).
    fn playout_turn(&self) -> u32 {
        match &self.watch {
            Some(watch) => watch_turn(self.pace_fps, watch.delay_seconds),
            None => vera20k_relay_protocol::turn_frames(self.pace_fps),
        }
    }

    /// Ends a restore once the world at `frame` has reached the shared one.
    /// A status can only postdate the last batch the departed process sent,
    /// so history read past that status holds this source's final batch ID,
    /// which the restoring relay session took as its counter.
    fn settle_restore(&mut self, frame: u32) {
        if self.statuses > 0
            && self.next_sequence >= self.remote_next_sequence
            && self.head_frame.saturating_sub(frame) <= u32::from(self.pace_fps) / 2
        {
            self.restoring = false;
            log::info!(
                "Rejoined the match at frame {frame} after batch {}",
                self.last_batch[usize::from(self.local_source)]
            );
        }
    }

    /// Remote Houses whose connection the relay ended in dispatched frames.
    pub(super) fn take_lost(&mut self) -> Vec<i8> {
        std::mem::take(&mut self.lost)
    }

    /// The relay's rate changes in dispatched frames.
    pub(super) fn take_speed_changes(&mut self) -> Vec<MatchSpeedChange> {
        std::mem::take(&mut self.speed_changes)
    }

    /// Other players' shared pauses and resumes the world has reached.
    pub(super) fn take_pause_changes(&mut self) -> Vec<MatchPauseChange> {
        std::mem::take(&mut self.pause_changes)
    }

    /// Other players the relay stopped or started hearing since the last call.
    pub(super) fn take_connection_changes(&mut self) -> Vec<MatchConnectionChange> {
        std::mem::take(&mut self.connection_changes)
    }

    pub(super) fn set_presentation_hidden(&mut self, hidden: bool) {
        self.pace_meter.hidden = hidden;
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn mark_started_for_test(&mut self) {
        self.started = true;
        self.ever_started = true;
    }

    pub(super) fn frontier(&self) -> u32 {
        self.frames.back().map_or_else(
            || {
                self.current
                    .filter(|(_, done)| *done)
                    .map_or(0, |(c, _)| c.frame)
            },
            |f| f.frame,
        )
    }

    pub(super) fn started(&self) -> bool {
        self.started
    }
    pub(super) fn ever_started(&self) -> bool {
        self.ever_started
    }
    pub(super) fn failed(&self) -> bool {
        self.failure.is_some()
    }

    /// Ends this match here. The first error is the one it ended with.
    fn fail(&mut self, error: impl Into<LiveCommandError>) {
        if self.failure.is_none() {
            self.failure = Some(Failure::new(error.into()));
        }
    }
    pub(super) fn record_tick_cost(&mut self, micros: u32) {
        self.pace_meter.record_tick_cost(micros);
    }

    pub(super) fn sent_commands(&self) -> u64 {
        self.sent_commands
    }

    pub(super) fn command_traces(&self) -> &VecDeque<CommandDeliveryTrace> {
        &self.command_traces
    }

    pub(super) fn delivery_counters(&self) -> DeliveryCounters {
        let counts = self.delivery;
        DeliveryCounters {
            link: counts.link,
            round_trips: counts.round_trips,
            round_trip_total_ms: counts.round_trip_total_ms,
            round_trip_max_ms: counts.round_trip_max_ms,
            desync_frame: counts.desync_frame,
            started: self.ever_started,
            failure: self.failure.as_ref().map(|failure| failure.code),
        }
    }

    /// The relay's sealed head as this PC last learned it.
    pub(super) fn head_frame(&self) -> u32 {
        self.head_frame
    }

    pub(super) fn slowed(&self) -> bool {
        self.pace_fps < self.fps
    }

    fn trace_assignment(&mut self, batch_id: u64, frame: u32, received_at: Instant) {
        if let Some(trace) = self
            .command_traces
            .iter_mut()
            .rev()
            .find(|trace| trace.batch_id == batch_id)
        {
            trace.assigned.get_or_insert((frame, received_at));
        }
    }

    pub(super) fn timing(&self) -> Option<SequencedTiming> {
        self.latest_frame_receipt
    }

    pub(super) fn pause_status(&self) -> MultiplayerPauseStatus {
        MultiplayerPauseStatus {
            available: self.started
                && !self.failed()
                && self.pause_active_sources != 0
                && self.pause_supported_sources & self.pause_active_sources
                    == self.pause_active_sources,
            pending: self.pause_request.is_some(),
            paused: self.pause.paused,
            frame: self.pause.frame,
            epoch: self.pause.epoch,
        }
    }

    pub(super) fn request_pause(&mut self, paused: bool) -> bool {
        self.narrative_hold.is_none()
            && self.narrative_pause_epoch.is_none()
            && self.queue_pause_request(paused, false)
    }

    fn queue_pause_request(&mut self, paused: bool, narrative: bool) -> bool {
        if !self.pause_status().available
            || self.pause_request.is_some()
            || self.pause.paused == paused
        {
            return false;
        }
        let Some(next) = self.next_pause_request.checked_add(1) else {
            return false;
        };
        self.pause_request = Some(PauseRequest {
            id: self.next_pause_request,
            expected_epoch: self.pause.epoch,
            paused,
            narrative,
            last_sent: None,
            acknowledged_epoch: None,
        });
        self.next_pause_request = next;
        true
    }

    pub(super) fn set_narrative_hold(&mut self, hold: Option<NarrativeHold>) {
        if self.narrative_hold != hold {
            self.narrative_hold = hold;
            // The barrier no longer needs to acquire this pause. Keep the
            // possible marker, since a packet already on the wire can win.
            // Leaving the retry slot free also permits later pause requests.
            if hold.is_none()
                && let Some(request) = self
                    .pause_request
                    .take_if(|request| request.narrative && request.paused)
                && request.last_sent.is_some()
                && let Some(epoch) = request.expected_epoch.checked_add(1)
                && epoch > self.pause.epoch
            {
                self.late_narrative_pause_epoch = Some(epoch);
            }
            self.last_narrative_pause_attempt = None;
            self.pace_meter.lag.reset();
            self.pause_found_behind = false;
        }
    }

    pub(super) fn keep_running_for_exit(&mut self) {
        self.keep_running_for_exit = true;
    }

    pub(super) fn exit_requested(&self) -> bool {
        self.keep_running_for_exit
    }

    pub(super) fn service(&mut self, transport: &mut LanUdpTransport, now: u32) {
        self.first_service.get_or_insert(now);
        if self.restoring && service_due(self.last_restore_log, now, 5000) {
            self.last_restore_log = Some(now);
            log::info!(
                "Rejoin: frame {:?}, frontier {}, head {}, next sequence {} of {}, buffered {}, started {}, statuses {}",
                self.current.map(|(context, done)| (context.frame, done)),
                self.frontier(),
                self.head_frame,
                self.next_sequence,
                self.remote_next_sequence,
                self.frames.len(),
                self.started,
                self.statuses,
            );
        }
        while let Some(received) = transport.pop_ordering_received_with_time() {
            self.last_response = Some(now);
            if matches!(received.event, OrderEvent::Unrecognized) {
                self.note_unreadable(now);
                continue;
            }
            if let Err(error) = self.receive(received.event, received.received_at) {
                self.fail(error);
            }
        }
        // Network silence is recoverable until the relay departs this source
        // after ten minutes: until then the same stream cursor, unacknowledged
        // bytes and world wait for the route/lease to return, and neither a
        // shorter timeout nor peer progress may manufacture an EXIT or fail a
        // frame. Past it the transport releases the route, and the match ends
        // here instead of showing Reconnecting for as long as the game stays
        // open. Before anything below asks the released route to send.
        if transport.relay_absent() {
            self.fail(SequencedError::Absent);
        }
        if self.watch.is_some()
            && self
                .last_response
                .or(self.first_service)
                .is_some_and(|last| now.wrapping_sub(last) > WATCH_SILENCE_MS)
        {
            self.fail(SequencedError::WatchLost);
        }
        // A failed match pulls, paces and seals nothing more; the transport
        // keeps its route until the app leaves the match.
        if self.failed() {
            return;
        }
        if let Err(error) = self.assemble() {
            self.fail(error);
            return;
        }
        if let Err(error) = self.check_unreadable(now) {
            self.fail(error);
            return;
        }
        let position = (
            self.head_frame,
            self.remote_next_sequence,
            self.next_sequence,
        );
        if self.last_progress.is_none_or(|(_, last)| last != position) {
            self.last_progress = Some((now, position));
        }
        self.service_delivery(transport, now);
        if self.watch.is_some() {
            // A watcher only reads the stream (relay README "Watchers").
            return;
        }
        self.service_pause(transport, now);
        self.service_pace(transport, now);
        self.seal_submissions(transport);
        // The relay session decides every UPLINK (spec §6.3); this layer only
        // hands over its stream position.
        let gap = self.ordering_gap();
        transport.report_ordering_position(
            self.next_sequence,
            gap,
            vera20k_relay_protocol::turn_ms(self.pace_fps),
        );
        if self.started {
            let _ = transport.poll_uplink();
        }
    }

    fn service_pause(&mut self, transport: &mut LanUdpTransport, now: u32) {
        if !self.started {
            return;
        }
        // A confirmed UI exit remains in the native command queue. Keep its
        // timeline running until teardown, including when a pending pause or
        // another participant's pause wins before that EXIT is dispatched.
        if self.keep_running_for_exit {
            self.queue_pause_request(false, false);
        } else if self.narrative_hold == Some(NarrativeHold::Host) {
            if service_due(
                self.last_narrative_pause_attempt,
                now,
                NARRATIVE_PAUSE_RETRY_MS,
            ) && self.queue_pause_request(true, true)
            {
                self.last_narrative_pause_attempt = Some(now);
            }
        } else if self.narrative_pause_epoch == Some(self.pause.epoch) {
            self.queue_pause_request(false, true);
        }
        if let Some(request) = &mut self.pause_request {
            if service_due(request.last_sent, now, 100)
                && transport
                    .control_ordering_pause(request.id, request.expected_epoch, request.paused)
                    .is_ok()
            {
                request.last_sent = Some(now);
            }
        } else if !self.pause_refused
            && !self.pause_status().available
            && service_due(self.last_pause_control, now, PAUSE_PROBE_INTERVAL_MS)
            && transport.control_ordering_pause(0, 0, false).is_ok()
        {
            self.last_pause_control = Some(now);
        }
    }

    fn service_pace(&mut self, transport: &mut LanUdpTransport, now: u32) {
        if !self.started {
            return;
        }
        if let Some((pace, ran, tick_micros)) = self.pace_report(now) {
            // It rides the next UPLINK once; a lost one only goes stale.
            transport.queue_ordering_pace(pace, ran, tick_micros);
        }
    }

    fn pace_report(&mut self, now: u32) -> Option<(OrderPace, u16, Option<u32>)> {
        let measured = !self.restoring
            && !self.pause.paused
            && !self.keep_running_for_exit
            && self.narrative_hold.is_none();
        self.pace_meter
            .report(now, self.frames.len(), self.pace_fps, measured)
    }

    fn finish_pause_request(&mut self) {
        if self.pause_request.as_ref().is_some_and(|request| {
            request
                .acknowledged_epoch
                .is_some_and(|epoch| epoch <= self.pause.epoch)
        }) {
            self.pause_request = None;
        }
    }

    /// Seals each local batch with the relay session in order; it retries
    /// them until acknowledged (spec §6.1-§6.3). An unready route seals on a
    /// later service.
    fn seal_submissions(&mut self, transport: &mut LanUdpTransport) {
        for pending in &mut self.submissions {
            let SubmissionBatch::Unsealed(bytes) = &pending.batch else {
                continue;
            };
            let Ok(id) = transport.seal_ordering_batch(bytes) else {
                break;
            };
            pending.batch = SubmissionBatch::Sealed(id);
            if self.command_traces.len() == COMMAND_TRACE_HISTORY {
                self.command_traces.pop_front();
            }
            self.command_traces.push_back(CommandDeliveryTrace {
                batch_id: id,
                sent_at: Instant::now(),
                assigned: None,
                frame_received_at: None,
            });
        }
    }

    /// The relay holds records past the contiguous cursor (spec §6.3 row 2),
    /// and the frame buffer has room for them.
    fn ordering_gap(&self) -> Option<OrderingGap> {
        if self.frames.len() >= self.buffered_frames() {
            return None;
        }
        OrderingGap::of(
            self.next_sequence,
            self.remote_next_sequence,
            self.entries.first_key_value().map(|(seq, _)| *seq),
        )
    }

    fn service_delivery(&mut self, transport: &mut LanUdpTransport, now: u32) {
        if self.watch.is_some() {
            // Ready until the start, then a Pull from the cursor: the relay
            // pushes nothing to a watcher.
            let sent = if !self.started {
                service_due(self.last_control, now, READY_REQUEST_INTERVAL_MS as i32)
                    && transport.configure_watch(self.agreement).is_ok()
            } else {
                service_due(self.last_control, now, WATCH_PULL_INTERVAL_MS as i32)
                    && transport.pull_ordering(self.next_sequence).is_ok()
            };
            if sent {
                self.last_control = Some(now);
            }
            if let Some(stats) = transport.relay_link_stats() {
                self.delivery.link = Some(stats);
            }
            return;
        }
        if !self.started {
            // Only READY_REQUEST until the start: its STATUS replies carry
            // the start, and there are no records to pull (spec §6.0).
            if service_due(self.last_control, now, READY_REQUEST_INTERVAL_MS as i32)
                && transport.configure_ordering(self.agreement).is_ok()
            {
                // Before the first request that can bring back an entry.
                transport.set_ordering_restore(self.restoring);
                self.last_control = Some(now);
            }
            return;
        }
        if self.restoring {
            // RESTORING: the relay pushes no new records; read them by PULL.
            if self.ordering_gap().is_some()
                && service_due(self.last_control, now, RESTORE_PULL_INTERVAL_MS as i32)
                && transport.pull_ordering(self.next_sequence).is_ok()
            {
                self.last_control = Some(now);
            }
            return;
        }
        // Gaps go to the relay first by UPLINK; the session pulls only
        // after its grace (spec §7.7).
        let _ = transport.poll_repair_pull();
        if let Some(stats) = transport.relay_link_stats() {
            self.delivery.link = Some(stats);
        }
    }

    fn note_unreadable(&mut self, now: u32) {
        match &mut self.unreadable {
            // Only a relay that keeps repeating the record holds the match;
            // after a pause the five seconds start again.
            Some(unreadable) if now.wrapping_sub(unreadable.last) <= UNREADABLE_REPEAT_MS => {
                unreadable.last = now
            }
            _ => {
                self.unreadable = Some(Unreadable {
                    since: now,
                    last: now,
                    next_sequence: self.next_sequence,
                    statuses: self.statuses,
                })
            }
        }
    }

    /// Released clients cannot be changed: a newer relay's record or Status
    /// this build cannot read would otherwise stall it forever. Unreadable
    /// replies that hold nothing back are ignored.
    fn check_unreadable(&mut self, now: u32) -> Result<(), SequencedError> {
        let Some(unreadable) = self.unreadable else {
            return Ok(());
        };
        let cursor_held =
            self.next_sequence == unreadable.next_sequence && self.ordering_gap().is_some();
        let status_held = !self.started && self.statuses == unreadable.statuses;
        if !cursor_held && !status_held {
            self.unreadable = None;
        } else if now.wrapping_sub(unreadable.since) >= UNREADABLE_LIMIT_MS
            && now.wrapping_sub(unreadable.last) <= UNREADABLE_REPEAT_MS
        {
            return Err(SequencedError::NewerClientRequired);
        }
        Ok(())
    }

    fn receive(&mut self, event: OrderEvent, received_at: Instant) -> Result<(), SequencedError> {
        match event {
            OrderEvent::Left => return Err(SequencedError::InvalidTimeline),
            OrderEvent::Ready { .. } => {}
            // service times these while they hold the cursor or start.
            OrderEvent::Unrecognized => {}
            OrderEvent::Status(status) => {
                self.statuses += 1;
                if status.tick_hz != self.fps {
                    return Err(SequencedError::InvalidTimeline);
                }
                if status.oldest_seq > self.next_sequence {
                    return Err(SequencedError::HistoryExpired);
                }
                if status.started && !self.started {
                    // Frame 1 is sealed a frame period after the start, so
                    // the stall clock starts now, not when loading did.
                    self.last_progress = None;
                }
                self.started |= status.started;
                self.ever_started |= status.started;
                self.head_frame = self.head_frame.max(status.head_frame);
                self.remote_next_sequence = self.remote_next_sequence.max(status.next_seq);
                self.pause_active_sources &= status.active_sources;
                self.note_silent(status.silent_sources);
                if let Some(watch) = &mut self.watch
                    && let Some(sample) = status.watch_sample
                    && watch.samples.back().is_none_or(|last| last.0 < sample.0)
                {
                    if watch.samples.len() == WATCH_SAMPLES {
                        watch.samples.pop_front();
                    }
                    watch.samples.push_back(sample);
                }
            }
            OrderEvent::PauseStatus {
                supported_sources,
                active_sources,
                request_id,
                epoch,
                ..
            } => {
                let sources = roster_mask(&self.houses);
                if (supported_sources | active_sources) & !sources != 0
                    || request_id >= self.next_pause_request
                {
                    return Err(SequencedError::InvalidTimeline);
                }
                // The relay counts this source as supporting pause from its
                // first probe on (relay `control_pause`), so a reply without
                // it comes from a match the relay never pauses: one whose
                // players are on several relays.
                self.pause_refused |= supported_sources & (1 << self.local_source) == 0;
                // Capabilities only accumulate and sources only depart. Old
                // replies cannot undo a newer availability observation.
                self.pause_supported_sources |= supported_sources;
                self.pause_active_sources &= active_sources;
                if let Some(request) = &mut self.pause_request
                    && request_id >= request.id
                {
                    request.acknowledged_epoch = Some(
                        request
                            .acknowledged_epoch
                            .map_or(epoch, |old| old.max(epoch)),
                    );
                }
                self.finish_pause_request();
            }
            // The relay session took its acceptance (spec §6.4).
            OrderEvent::Ack {
                batch_id, frame, ..
            } => self.trace_assignment(batch_id, frame, received_at),
            OrderEvent::Entry(entry) => {
                let sequence = match &entry {
                    OrderEntry::Batch {
                        sequence,
                        source,
                        batch_id,
                        frame,
                        ..
                    } => {
                        // A returning process's own source in the stream is
                        // the process it replaces; a watcher's is the player
                        // it follows.
                        if *source == self.local_source && !self.restoring && self.watch.is_none() {
                            self.trace_assignment(*batch_id, *frame, received_at);
                        }
                        *sequence
                    }
                    OrderEntry::FrameEnd { sequence, .. }
                    | OrderEntry::Departure { sequence, .. }
                    | OrderEntry::Pause { sequence, .. }
                    | OrderEntry::Rate { sequence, .. }
                    | OrderEntry::Desync { sequence, .. } => *sequence,
                };
                if sequence >= self.next_sequence
                    && sequence - self.next_sequence < self.reorder_window()
                {
                    self.entries
                        .entry(sequence)
                        .or_insert(ReceivedEntry { entry, received_at });
                }
            }
            OrderEvent::Error { code, expected } => match code {
                OrderError::Inactive if expected == vera20k_relay_protocol::FEDERATION_LOST => {
                    return Err(SequencedError::FederationLost);
                }
                OrderError::NotConfigured => {
                    self.started = false;
                    self.last_control = None;
                }
                // The relay session takes BatchGap's acceptance and resends
                // the rest by its RTO (spec §6.4).
                OrderError::NotStarted | OrderError::Backpressure | OrderError::BatchGap => {}
                OrderError::HistoryGap => return Err(SequencedError::HistoryExpired),
                // Only a match that never started: a restored session has not
                // heard from the relay yet, and its match did start.
                OrderError::Inactive if !self.ever_started && !self.restoring => {
                    return Err(SequencedError::StartCancelled);
                }
                OrderError::Inactive | OrderError::StorageUnavailable => {
                    return Err(SequencedError::Unavailable);
                }
                refused => return Err(SequencedError::Refused(refused)),
            },
        }
        Ok(())
    }

    pub(super) fn recovery_status(
        &self,
        now: u32,
        current_frame: u32,
    ) -> Option<super::NetworkRecoveryStatus> {
        let last = self.last_response.or(self.first_service)?;
        if now.wrapping_sub(last) > 3_000 {
            return Some(super::NetworkRecoveryStatus::Reconnecting);
        }
        // Every running frame ends with a FrameEnd entry, so a started,
        // unpaused match whose stream has stood still while this client
        // waits for it is stalled even though the relay answers.
        if self.started
            && !self.pause.paused
            && current_frame >= self.head_frame
            && self
                .last_progress
                .is_some_and(|(at, _)| now.wrapping_sub(at) > STALL_MS)
        {
            return Some(super::NetworkRecoveryStatus::Reconnecting);
        }
        if self.narrative_hold.is_some() {
            return None;
        }
        self.catching_up_frames(current_frame)
            .map(|frames| super::NetworkRecoveryStatus::CatchingUp { frames })
    }

    /// How far this game is behind the relay's head, when that is more than
    /// half a second of frames: the Catching up message row.
    fn catching_up_frames(&self, current_frame: u32) -> Option<u32> {
        let frames = self.head_frame.saturating_sub(current_frame);
        // A watcher's frames come a pull or a second at a time and wait
        // to be played evenly (`watch_turn`); only more than that is behind.
        let waiting = match self.watch {
            Some(_) => self.playout_turn(),
            None => 0,
        };
        (frames > u32::from(self.pace_fps) / 2 + waiting).then_some(frames)
    }

    /// VERA: true once, when this game reaches a pause that found it
    /// catching up, so it can tell the others it has caught up. `reached` is
    /// whether the simulation has reached the pause; a resume first sends
    /// nothing, nor does a pause the relay has already ended, which a
    /// rejoining game replays while still behind. gamemd's lockstep makes
    /// everyone wait for a slow player, so it has no such line.
    pub(super) fn take_caught_up_in_pause(&mut self, reached: bool, current_frame: u32) -> bool {
        if !self.pause.paused || self.narrative_hold.is_some() {
            self.pause_found_behind = false;
        } else if reached {
            return std::mem::take(&mut self.pause_found_behind)
                && self.catching_up_frames(current_frame).is_none();
        } else if self.catching_up_frames(current_frame).is_some() {
            self.pause_found_behind = true;
        }
        false
    }

    fn assemble(&mut self) -> Result<(), SequencedError> {
        while self.frames.len() < self.buffered_frames() {
            let Some(received) = self.entries.remove(&self.next_sequence) else {
                break;
            };
            self.contiguous_received_at = Some(
                self.contiguous_received_at
                    .map_or(received.received_at, |old| old.max(received.received_at)),
            );
            self.next_sequence = self
                .next_sequence
                .checked_add(1)
                .ok_or(SequencedError::InvalidTimeline)?;
            match received.entry {
                OrderEntry::Batch {
                    frame,
                    source,
                    batch_id,
                    bytes,
                    ..
                } => {
                    if frame != self.assembling.frame
                        || batch_id != self.last_batch[usize::from(source)] + 1
                    {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    self.last_batch[usize::from(source)] = batch_id;
                    let house =
                        self.houses[usize::from(source)].ok_or(SequencedError::InvalidTimeline)?;
                    if self.refused & 1 << source != 0 {
                        continue;
                    }
                    let decoded = decode_batch(&bytes, house, frame).ok();
                    let Some(records) = decoded else {
                        // VERA relay adaptation: every client reads the same
                        // ordered bytes, so each refuses the same batch. Its
                        // sender leaves at this frame, as by Leave, instead
                        // of one bad batch ending the match for everyone.
                        log::warn!(
                            "Relay source {source} sent an unreadable batch; it leaves at frame {frame}."
                        );
                        self.refused |= 1 << source;
                        self.assembling.records.push(
                            ExitRecord {
                                house_id: house,
                                frame,
                            }
                            .encode()
                            .map_err(|_| SequencedError::InvalidTimeline)?,
                        );
                        continue;
                    };
                    if self.assembling.records.len() + records.len() > MAX_FRAME_RECORDS {
                        return Err(SequencedError::QueueFull);
                    }
                    self.assembling.records.extend(records);
                }
                OrderEntry::Departure {
                    frame,
                    source,
                    lost,
                    ..
                } => {
                    if frame != self.assembling.frame {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    let house =
                        self.houses[usize::from(source)].ok_or(SequencedError::InvalidTimeline)?;
                    if let Some(watch) = &mut self.watch {
                        watch.departed |= 1 << source;
                    }
                    // The relay ends a pause that its last player left with a
                    // resume in that player's name (`retire`); the player
                    // leaves in this frame, so nobody asked for it.
                    self.assembling
                        .pauses
                        .retain(|change| change.house != house);
                    if self.refused & 1 << source != 0 {
                        // It already left at its refused batch.
                        continue;
                    }
                    let record = if lost {
                        // Silence ended this place: native connection loss.
                        // DestroyConnection(house, 1) (0x005DA750) sends
                        // EVENT_REMOVEPLAYER (0x005DA898..0x005DA8F0), which
                        // kills the House on every peer; each peer retires its
                        // own connection and posts the line. A watcher posts
                        // the line of the player it follows too.
                        if source != self.local_source || self.watch.is_some() {
                            self.assembling.lost.push(house);
                        }
                        CommandRecord::encode(
                            REMOVE_PLAYER_OPCODE,
                            i32::from(house),
                            frame as i32,
                            &i32::from(house).to_le_bytes(),
                        )
                    } else {
                        ExitRecord {
                            house_id: house,
                            frame,
                        }
                        .encode()
                    };
                    self.assembling
                        .records
                        .push(record.map_err(|_| SequencedError::InvalidTimeline)?);
                }
                OrderEntry::FrameEnd { frame, .. } => {
                    if frame != self.assembling.frame {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    let next = frame
                        .checked_add(1)
                        .ok_or(SequencedError::InvalidTimeline)?;
                    self.frames.push_back(std::mem::replace(
                        &mut self.assembling,
                        CommittedFrame {
                            frame: next,
                            ..Default::default()
                        },
                    ));
                    let received_at = self.contiguous_received_at.expect("consumed frame end");
                    for trace in &mut self.command_traces {
                        if trace
                            .assigned
                            .is_some_and(|(assigned, _)| assigned == frame)
                            && trace.frame_received_at.is_none()
                        {
                            trace.frame_received_at = Some(received_at);
                            let ms = received_at
                                .saturating_duration_since(trace.sent_at)
                                .as_millis() as u64;
                            self.delivery.round_trips += 1;
                            self.delivery.round_trip_total_ms += ms;
                            self.delivery.round_trip_max_ms =
                                self.delivery.round_trip_max_ms.max(ms);
                        }
                    }
                    self.latest_frame_receipt = Some(SequencedTiming {
                        frame,
                        received_at,
                        frames_per_second: self.pace_fps,
                        clock_epoch: self.pause.epoch,
                        turn_frames: self.playout_turn(),
                    });
                }
                OrderEntry::Pause {
                    frame,
                    epoch,
                    paused,
                    source,
                    ..
                } => {
                    if frame.checked_add(1) != Some(self.assembling.frame)
                        || !self.assembling.records.is_empty()
                        || epoch.checked_sub(1) != Some(self.pause.epoch)
                        || paused == self.pause.paused
                        || self.houses[usize::from(source)].is_none()
                        || (!paused && frame != self.pause.frame)
                    {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    self.pause.paused = paused;
                    self.pause.frame = frame;
                    self.pause.epoch = epoch;
                    // A newer manual pause request takes precedence over a
                    // possible late narrative pause at this same epoch.
                    self.narrative_pause_epoch = (paused
                        && source == self.local_source
                        && self
                            .pause_request
                            .as_ref()
                            .filter(|request| {
                                request.paused
                                    && request.expected_epoch.checked_add(1) == Some(epoch)
                            })
                            .map_or(self.late_narrative_pause_epoch == Some(epoch), |request| {
                                request.narrative
                            }))
                    .then_some(epoch);
                    self.late_narrative_pause_epoch = self
                        .late_narrative_pause_epoch
                        .filter(|&possible_epoch| possible_epoch > epoch);
                    // VERA: another player's marker names them once the world
                    // reaches it; gamemd's speed line likewise skips its own
                    // sender (0x004C794B). A watcher names every player.
                    let suppress_notice = if paused {
                        self.narrative_hold.is_some()
                    } else {
                        self.suppress_narrative_pause_notice
                    };
                    self.suppress_narrative_pause_notice = paused && suppress_notice;
                    if let Some(house) = self.houses[usize::from(source)]
                        && (source != self.local_source || self.watch.is_some())
                        && !suppress_notice
                    {
                        let change = MatchPauseChange { house, paused };
                        if !paused {
                            // The world runs on from the next frame.
                            self.assembling.pauses.push(change);
                        } else if let Some(last) = self.frames.back_mut() {
                            // The world stops after `frame`, still queued.
                            last.pauses.push(change);
                        } else if !self.restoring {
                            // `frame` has dispatched: the world is there.
                            self.pause_changes.push(change);
                        }
                    }
                    // The authenticated sequencer only emits these markers
                    // after every active source advertises support. That is
                    // sufficient proof even if all of our status replies were
                    // lost; departed sources can never rejoin this roster.
                    self.pause_supported_sources |= self.pause_active_sources;
                    if !paused {
                        // Anchor resumed playout to this real socket receipt.
                        // The stopped wall time is absent from the timeline.
                        self.latest_frame_receipt = Some(SequencedTiming {
                            frame,
                            received_at: self.contiguous_received_at.expect("consumed resume"),
                            frames_per_second: self.pace_fps,
                            clock_epoch: epoch,
                            turn_frames: self.playout_turn(),
                        });
                    }
                    self.finish_pause_request();
                }
                OrderEntry::Rate {
                    frame,
                    frames_per_second,
                    source,
                    ..
                } => {
                    // Slowdowns name a player; raises name nobody.
                    let slowed_by = source.map(|source| self.houses[usize::from(source)]);
                    if frame.checked_add(1) != Some(self.assembling.frame)
                        || !self.assembling.records.is_empty()
                        || !admits_rate(self.fps, self.pacing, frames_per_second)
                        || frames_per_second == self.pace_fps
                        || slowed_by.is_some() != (frames_per_second < self.pace_fps)
                        || slowed_by.is_some_and(|house| house.is_none())
                    {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    log::info!(
                        "Relay rate: {} -> {frames_per_second} frames a second at frame {frame} ({})",
                        self.pace_fps,
                        match source {
                            Some(source) => format!("slowed by source {source}"),
                            None => "raised".to_owned(),
                        }
                    );
                    self.pace_fps = frames_per_second;
                    self.assembling.speed = match slowed_by.flatten() {
                        Some(house) => Some(MatchSpeedChange::SlowedBy(house)),
                        None => (frames_per_second == self.fps).then_some(MatchSpeedChange::Normal),
                    };
                    // Later frames arrive at the new rate from this receipt.
                    self.latest_frame_receipt = Some(SequencedTiming {
                        frame,
                        received_at: self.contiguous_received_at.expect("consumed rate marker"),
                        frames_per_second,
                        clock_epoch: self.pause.epoch,
                        turn_frames: self.playout_turn(),
                    });
                }
                OrderEntry::Desync {
                    frame,
                    sample_frame,
                    outlier,
                    outlier_checksum,
                    other,
                    other_checksum,
                    ..
                } => {
                    if frame.checked_add(1) != Some(self.assembling.frame)
                        || !self.assembling.records.is_empty()
                        || self.houses[usize::from(outlier)].is_none()
                        || self.houses[usize::from(other)].is_none()
                    {
                        return Err(SequencedError::InvalidTimeline);
                    }
                    self.assembling.desync = Some(RelayDesync {
                        sample_frame,
                        outlier: (outlier, outlier_checksum),
                        other: (other, other_checksum),
                    });
                }
            }
        }
        Ok(())
    }

    /// The relay's desync from this PC's side: the House it differs from
    /// (the outlier, or the other side when this PC is the outlier), this
    /// PC's own sample when it holds one, and that House's checksum.
    fn desync_error(&self, desync: RelayDesync) -> SequencedError {
        let ((source, remote), (_, reported)) = if desync.outlier.0 == self.local_source {
            (desync.other, desync.outlier)
        } else {
            (desync.outlier, desync.other)
        };
        let local = self
            .samples
            .iter()
            .rev()
            .find(|(frame, _)| *frame == desync.sample_frame)
            .map_or(reported, |&(_, checksum)| checksum);
        SequencedError::Desync {
            frame: desync.sample_frame,
            house: self.houses[usize::from(source)].expect("validated desync source"),
            local,
            remote,
        }
    }

    /// VERA: a player's STATUS names the sources its relay has not heard from
    /// lately. One newly named is not responding; one no longer named is
    /// back. A source any report so far showed gone is never named, even by
    /// a late STATUS, and neither is this game's own player.
    pub(super) fn note_silent(&mut self, silent: u32) {
        let active = self.pause_active_sources;
        let silent = silent & active & !(1 << self.local_source);
        for (source, house) in self.houses.iter().enumerate() {
            let bit = 1 << source;
            let responding = if silent & !self.silent & bit != 0 {
                false
            } else if self.silent & !silent & active & bit != 0 {
                true
            } else {
                continue;
            };
            if let Some(house) = *house {
                self.connection_changes
                    .push(MatchConnectionChange { house, responding });
            }
        }
        self.silent = silent;
    }

    /// A watcher's world against the checksums every player reported alike,
    /// once it has its own sample of their frames (relay README "Watchers").
    fn check_watch_samples(&mut self, frame: u32) -> Result<(), SequencedError> {
        let Some(watch) = &mut self.watch else {
            return Ok(());
        };
        while let Some(&(sample_frame, remote)) = watch.samples.front()
            && sample_frame <= frame
        {
            watch.samples.pop_front();
            if let Some(&(_, local)) = self
                .samples
                .iter()
                .rev()
                .find(|(frame, _)| *frame == sample_frame)
                && local != remote
            {
                return Err(SequencedError::WatchDesync {
                    frame: sample_frame,
                    local,
                    remote,
                });
            }
        }
        Ok(())
    }

    /// Service fresh input without another Logic visit. The current context
    /// is an already sampled post-Logic/pre-dispatch CRC, even when its frame
    /// has completed. It is historical metadata; only the relay assigns the
    /// command's execution frame. This path never consumes canonical frames.
    pub(super) fn service_pending_commands(
        &mut self,
        transport: &mut LanUdpTransport,
        queues: &mut NetworkCommandQueues<'_>,
        now: u32,
    ) {
        if !self.started || self.failed() {
            return;
        }
        let Some((context, _)) = self.current.filter(|(context, _)| context.frame != 0) else {
            return;
        };
        if let Err(error) = self.flush_local_commands(transport, queues, context, now) {
            // The regular frame boundary owns terminal error delivery. Its
            // admission gate also observes this retained error while stalled.
            self.fail(error);
        }
    }

    fn flush_local_commands(
        &mut self,
        transport: &mut LanUdpTransport,
        queues: &mut NetworkCommandQueues<'_>,
        context: NetworkQueueContext,
        now: u32,
    ) -> Result<(), LiveCommandError> {
        if self.restoring {
            self.settle_restore(context.frame);
            if !self.restoring {
                transport.set_ordering_restore(false);
            }
        }
        if self.restoring || self.watch.is_some() {
            // Orders given to a world still replaying the past, or by a
            // watcher, are dropped here, the one boundary every local
            // command crosses.
            let records: Vec<_> = queues
                .local_records_after(std::iter::empty())
                .map_err(super::CommandExchangeError::from)?
                .into_iter()
                .cloned()
                .collect();
            queues
                .commit_local_submission(&records)
                .map_err(super::CommandExchangeError::from)?;
            return Ok(());
        }
        // An accepted batch leaves OutList; its records come back in its frame.
        let accepted = transport.ordering_accepted_through().unwrap_or(0);
        while let Some(pending) = self.submissions.pop_front_if(
            |pending| matches!(pending.batch, SubmissionBatch::Sealed(id) if id <= accepted),
        ) {
            queues
                .commit_local_submission(&pending.records)
                .map_err(super::CommandExchangeError::from)?;
            self.sent_commands += pending.records.len() as u64;
        }
        // The relay compares every PC's post-Logic checksum of each sampled
        // frame (relay README, Checksums). The previous sample goes again,
        // so one lost report loses nothing.
        if context.frame % CHECKSUM_INTERVAL == 0 && self.sample_sent != Some(context.frame) {
            let mut reports = vec![(context.frame, context.checksum)];
            reports.extend(
                self.samples
                    .iter()
                    .rev()
                    .find(|(frame, _)| *frame + CHECKSUM_INTERVAL == context.frame)
                    .copied(),
            );
            reports.reverse();
            if transport.queue_ordering_checksums(&reports).is_ok() {
                self.sample_sent = Some(context.frame);
            }
        }
        // Unacknowledged records remain the authoritative OutList prefix.
        // Encode only its not-yet-sealed suffix; do not mirror or requeue it.
        if self.submissions.len()
            < vera20k_relay_protocol::ClientSession::ORDERING_SUBMISSION_WINDOW
        {
            let records = queues
                .local_records_after(self.submissions.iter().flat_map(|batch| &batch.records))
                .map_err(super::CommandExchangeError::from)?;
            if !records.is_empty() {
                if records
                    .iter()
                    .any(|record| !gameplay_opcode(record.opcode()))
                {
                    return Err(SequencedError::InvalidTimeline.into());
                }
                // A batch is the packet's command records alone: no version
                // byte and no FRAME_INFO, which every reader rebuilds
                // (bandwidth-compaction spec §4).
                let encoded = EventPacketCodec::default()
                    .encode_packet(
                        &records,
                        FrameInfo {
                            house_id: self.houses[usize::from(self.local_source)]
                                .expect("`new` seats the local source in the roster"),
                            event_frame: 0,
                            checksum: 0,
                            timing_word: 0,
                            delay: 0,
                        },
                        0,
                        EVENT_PACKET_HEADER_LEN + ORDERING_BATCH_CAPACITY,
                    )
                    .map_err(super::CommandExchangeError::from)?;
                if encoded.mirrors.is_empty() {
                    return Err(SequencedError::InvalidTimeline.into());
                }
                self.submissions.push_back(Submission {
                    records: encoded.mirrors,
                    batch: SubmissionBatch::Unsealed(
                        encoded.bytes[EVENT_PACKET_HEADER_LEN..].to_vec(),
                    ),
                });
                self.service(transport, now);
            }
        }
        Ok(())
    }

    /// Any error ends the match here and stays: the first poll after it
    /// returns that error, every later one `LiveCommandError::Failed`.
    pub(super) fn poll(
        &mut self,
        transport: &mut LanUdpTransport,
        queues: &mut NetworkCommandQueues<'_>,
        context: NetworkQueueContext,
        now: u32,
    ) -> Result<NetworkQueuePass, LiveCommandError> {
        self.service(transport, now);
        if !self.failed() {
            match self.poll_frame(transport, queues, context, now) {
                Ok(pass) => return Ok(pass),
                // The frame's own error (a desync, a full queue) is the one
                // that ends the match, even when its flush's service recorded
                // a relay error earlier in this call.
                Err(error) => self.failure = Some(Failure::new(error)),
            }
        }
        Err(self
            .failure
            .as_mut()
            .and_then(|failure| failure.unreported.take())
            .unwrap_or(LiveCommandError::Failed))
    }

    fn poll_frame(
        &mut self,
        transport: &mut LanUdpTransport,
        queues: &mut NetworkCommandQueues<'_>,
        context: NetworkQueueContext,
        now: u32,
    ) -> Result<NetworkQueuePass, LiveCommandError> {
        if let Some((old, done)) = self.current {
            if context.frame == old.frame {
                if context.checksum != old.checksum {
                    return Err(LiveCommandError::ChangedFrame(context.frame));
                }
                if done {
                    return Ok(if context.frame == 0 {
                        NetworkQueuePass::SkipDispatch
                    } else {
                        NetworkQueuePass::Dispatch
                    });
                }
            } else if !done || context.frame != old.frame + 1 {
                return Err(LiveCommandError::FrameSequence {
                    expected: old.frame + 1,
                    actual: context.frame,
                });
            }
        } else if context.frame != 0 {
            return Err(LiveCommandError::FrameSequence {
                expected: 0,
                actual: context.frame,
            });
        }
        self.current = Some((context, false));
        if !self.started {
            return Ok(NetworkQueuePass::Waiting);
        }
        if context.frame == 0 {
            self.current = Some((context, true));
            return Ok(NetworkQueuePass::SkipDispatch);
        }
        if self
            .samples
            .back()
            .is_none_or(|(frame, _)| *frame != context.frame)
        {
            if self.samples.len() == SAMPLE_HISTORY {
                self.samples.pop_front();
            }
            self.samples.push_back((context.frame, context.checksum));
        }
        self.check_watch_samples(context.frame)?;
        self.flush_local_commands(transport, queues, context, now)?;
        let Some(frame) = self
            .frames
            .pop_front_if(|frame| frame.frame == context.frame)
        else {
            return Ok(NetworkQueuePass::Waiting);
        };
        if let Some(desync) = frame.desync {
            self.delivery
                .desync_frame
                .get_or_insert(desync.sample_frame);
            return Err(self.desync_error(desync).into());
        }
        for record in frame.records {
            if !queues.admit_received(record) {
                return Err(SequencedError::QueueFull.into());
            }
        }
        self.lost.extend(frame.lost);
        self.pace_meter.dispatched = self.pace_meter.dispatched.saturating_add(1);
        // A rejoining world replays the past without its lines.
        if !self.restoring {
            self.speed_changes.extend(frame.speed);
            self.pause_changes.extend(frame.pauses);
        }
        self.current = Some((context, true));
        Ok(NetworkQueuePass::Dispatch)
    }
}

/// Whether a relay batch may carry the record. Batches leave out the packet
/// headers, which every reader rebuilds, and the network control events the
/// relay's own records and pacing replace (speed, departure, timing); the
/// rest have no online producer.
fn gameplay_opcode(opcode: u8) -> bool {
    !matches!(
        opcode,
        FRAME_SYNC_OPCODE
            | FRAMEINFO_EVENT_OPCODE
            | GAME_SPEED_OPCODE
            | REMOVE_PLAYER_OPCODE
            | TIMING_OPCODE
            | PROCESS_TIME_OPCODE
            | LATENCY_FUDGE_OPCODE
            | EMPTY_OPCODE
            | MESSAGE_OPCODE
            | RESPONSE_TIME_OPCODE
            | ADD_PLAYER_OPCODE
    )
}

/// Frames that reach a watcher together at `fps`: what its relay releases
/// at once (a `WATCH_MARK_MS` mark when it holds the stream back, a turn
/// when it does not; relay README "Watchers"), and two pulls' worth for the
/// pull that brings them.
fn watch_turn(fps: u16, delay_seconds: u32) -> u32 {
    let released = if delay_seconds > 0 {
        (u64::from(fps) * WATCH_MARK_MS / 1000) as u32
    } else {
        vera20k_relay_protocol::turn_frames(fps)
    };
    released + 2 * (u32::from(fps) * WATCH_PULL_INTERVAL_MS / 1000).max(1)
}

/// The source a watcher's game follows (relay README "Watchers"), from the
/// match's `sources_mask` and the sources its connections hold as peers, and
/// whether this game takes that source's seat. A roster that lacks one source
/// is a player's own view of the match (a lobby watch, or a launcher's watcher
/// spawn.ini, docs/spawn.md "Watching"): the watcher stands in that seat. A
/// roster that holds every source watches a match of several sources from
/// no seat and follows the first. Any other roster is not this match's.
fn watched_source(sources_mask: u32, roster: u32) -> Option<(u8, bool)> {
    if roster & !sources_mask != 0 {
        return None;
    }
    let missing = sources_mask & !roster;
    match missing.count_ones() {
        1 => Some((missing.trailing_zeros() as u8, true)),
        0 if sources_mask != 0 => Some((sources_mask.trailing_zeros() as u8, false)),
        _ => None,
    }
}

fn roster_mask(houses: &[Option<i8>; MAX_PEERS as usize]) -> u32 {
    houses
        .iter()
        .enumerate()
        .filter(|(_, h)| h.is_some())
        .fold(0, |m, (s, _)| m | (1 << s))
}

/// A batch's command records (bandwidth-compaction spec §4): the packet
/// rebuilt with the FRAME_INFO of its source's `house`, its record's
/// `frame` and zeros, then the unchanged `EventPacketCodec` decode.
fn decode_batch(bytes: &[u8], house: i8, frame: u32) -> Result<Vec<CommandRecord>, SequencedError> {
    if bytes.is_empty() || bytes.len() > ORDERING_BATCH_CAPACITY {
        return Err(SequencedError::InvalidTimeline);
    }
    let header = FrameInfo {
        house_id: house,
        event_frame: i32::try_from(frame).map_err(|_| SequencedError::InvalidTimeline)?,
        checksum: 0,
        timing_word: 0,
        delay: 0,
    }
    .encode();
    let mut packet = Vec::with_capacity(EVENT_PACKET_HEADER_LEN + bytes.len());
    packet.extend_from_slice(&header.as_bytes()[..EVENT_PACKET_HEADER_LEN]);
    packet.extend_from_slice(bytes);
    let packet = packet.as_slice();
    let mut records = Vec::new();
    let mut invalid = false;
    let mut header = true;
    let decoded = EventPacketCodec::default()
        .decode_packet(packet, |mut record| {
            if header {
                header = false;
                return true;
            }
            if record.house_id() != house || !gameplay_opcode(record.opcode()) {
                invalid = true;
                return false;
            }
            record.set_frame_stamp(frame as i32);
            records.push(record);
            true
        })
        .map_err(|_| SequencedError::InvalidTimeline)?;
    if invalid || decoded.consumed_bytes != packet.len() || records.len() > OUT_LIST_CAPACITY {
        return Err(SequencedError::InvalidTimeline);
    }
    Ok(records)
}

/// The sessions and STATUS the tests here and in `live_commands_tests` build on.
#[cfg(all(test, tests_net))]
pub(super) mod fixtures {
    use super::*;
    use vera20k_relay_protocol::OrderStatus;

    /// The session of source 0 in a room whose sources play `houses` in
    /// order, at 30 frames a second.
    pub(in crate::net::live_commands) fn session_for(houses: &[i8]) -> SequencedSession {
        let mut roster = [None; MAX_PEERS as usize];
        for (place, &house) in roster.iter_mut().zip(houses) {
            *place = Some(house);
        }
        SequencedSession::with_roster(roster, 0, [0; 32], 30)
    }

    /// The STATUS of a started room of sources 0 and 1 that sealed nothing yet.
    pub(in crate::net::live_commands) fn started_status() -> OrderStatus {
        OrderStatus {
            head_frame: 0,
            oldest_seq: 1,
            next_seq: 1,
            active_sources: 3,
            tick_hz: 30,
            start_delay_ms: 0,
            started: true,
            feedback: Default::default(),
            watch_sample: None,
            silent_sources: 0,
        }
    }
}

#[cfg(all(test, tests_net))]
mod service_clock_tests {
    use super::service_due;

    #[test]
    fn retry_deadline_survives_bucket_rounding_and_uptime_wrap() {
        assert!(service_due(None, 1008, 100));
        assert!(!service_due(Some(1018), 1008, 100));
        assert!(!service_due(Some(1018), 1117, 100));
        assert!(service_due(Some(1018), 1118, 100));
        assert!(!service_due(Some(u32::MAX - 50), 48, 100));
        assert!(service_due(Some(u32::MAX - 50), 49, 100));
    }
}

#[cfg(all(test, tests_net))]
mod desync_tests {
    use super::fixtures::session_for;
    use super::*;

    /// The relay's DESYNC ends the match in the frame after it. The log
    /// names the outlier's House against this PC's own sample, or the other
    /// side's House when this PC is the outlier.
    #[test]
    fn a_relay_desync_names_the_house_this_pc_differs_from() {
        let mut session = session_for(&[0, 4, 2]);
        session.samples.push_back((15, 0x1111));
        for entry in [
            OrderEntry::FrameEnd {
                sequence: 1,
                frame: 1,
            },
            OrderEntry::Desync {
                sequence: 2,
                frame: 1,
                sample_frame: 15,
                outlier: 1,
                outlier_checksum: 0x2a2a,
                other: 2,
                other_checksum: 0x1110,
            },
            OrderEntry::FrameEnd {
                sequence: 3,
                frame: 2,
            },
        ] {
            session
                .receive(OrderEvent::Entry(entry), Instant::now())
                .unwrap();
        }
        session.assemble().unwrap();
        let desyncs: Vec<_> = session
            .frames
            .iter()
            .map(|frame| frame.desync.is_some())
            .collect();
        assert_eq!(desyncs, [false, true]);
        let desync = session.frames[1].desync.unwrap();
        assert_eq!(
            session.desync_error(desync).to_string(),
            "online simulation differs for house 4 at frame 15: \
             local 0x00001111, remote 0x00002a2a"
        );
        // Without its own sample, the outlier's local value is its report.
        session.samples.clear();
        let outlier = RelayDesync {
            outlier: (0, 0x2a2a),
            ..desync
        };
        assert_eq!(
            session.desync_error(outlier).to_string(),
            "online simulation differs for house 2 at frame 15: \
             local 0x00002a2a, remote 0x00001110"
        );
    }
}

#[cfg(all(test, tests_net))]
mod restore_tests {
    use super::fixtures::{session_for, started_status};
    use super::*;
    use vera20k_relay_protocol::OrderStatus;

    fn status(next_seq: u64, head_frame: u32) -> OrderEvent {
        OrderEvent::Status(OrderStatus {
            head_frame,
            next_seq,
            ..started_status()
        })
    }

    /// A relay that loses the match later does not unsay that it started.
    #[test]
    fn the_report_keeps_that_the_match_started() {
        let mut session = session_for(&[0, 1]);
        assert!(!session.delivery_counters().started);
        session.receive(status(1, 0), Instant::now()).unwrap();
        let lost = OrderEvent::Error {
            code: OrderError::NotConfigured,
            expected: 0,
        };
        session.receive(lost, Instant::now()).unwrap();
        assert!(!session.started());
        assert!(session.delivery_counters().started);
    }

    /// A room closed before the start means a player left. The same refusal
    /// later, or to a restored session, is the service losing the match.
    #[test]
    fn an_inactive_room_is_a_cancelled_start_only_before_the_match_started() {
        let session = || session_for(&[0, 1]);
        let inactive = || OrderEvent::Error {
            code: OrderError::Inactive,
            expected: 0,
        };

        let lost = OrderEvent::Error {
            code: OrderError::Inactive,
            expected: vera20k_relay_protocol::FEDERATION_LOST,
        };
        assert_eq!(
            session()
                .receive(lost, Instant::now())
                .unwrap_err()
                .interruption(),
            NetworkInterruption::FederationLost
        );

        let refused = session().receive(inactive(), Instant::now()).unwrap_err();
        assert_eq!(refused.code(), "start_cancelled");
        assert_eq!(refused.interruption(), NetworkInterruption::StartCancelled);

        let mut started = session();
        started.receive(status(1, 0), Instant::now()).unwrap();
        let refused = started.receive(inactive(), Instant::now()).unwrap_err();
        assert_eq!(refused.code(), "unavailable");
        assert_eq!(
            refused.interruption(),
            NetworkInterruption::ConnectionFailed
        );

        let mut restored = session();
        restored.begin_restore();
        let refused = restored.receive(inactive(), Instant::now()).unwrap_err();
        assert_eq!(refused.code(), "unavailable");
    }

    #[test]
    fn only_differing_start_agreements_read_as_a_start_mismatch() {
        assert_eq!(
            SequencedError::Refused(OrderError::AgreementMismatch).interruption(),
            NetworkInterruption::StartMismatch
        );
        assert_eq!(
            SequencedError::Refused(OrderError::ModifiedRetry).interruption(),
            NetworkInterruption::ConnectionFailed
        );
    }

    /// The counter the departed process reached comes from the history, so
    /// a restore ends only once the world has read and caught up with it.
    #[test]
    fn ends_a_restore_only_once_the_world_caught_up() {
        let mut session = session_for(&[0, 1]);
        session.begin_restore();
        session.next_sequence = 500;

        // Nothing is known of the relay yet.
        session.settle_restore(0);
        assert!(session.restoring());
        session.receive(status(900, 1000), Instant::now()).unwrap();
        // History still to read, then a world still far behind it.
        session.settle_restore(1000);
        assert!(session.restoring());
        session.next_sequence = 900;
        session.settle_restore(900);
        assert!(session.restoring());

        session.settle_restore(990);
        assert!(!session.restoring());
    }
}

#[cfg(all(test, tests_net))]
mod stall_tests {
    use super::fixtures::{session_for, started_status};
    use super::*;

    /// Federated relays whose link failed still answer, but the ordered
    /// stream stops: a running, unpaused match shows Reconnecting, so Quit
    /// leaves without waiting on that stream.
    #[test]
    fn a_stream_that_stands_still_while_its_relay_answers_is_reconnecting() {
        let mut session = session_for(&[0, 1]);
        session.started = true;
        session.head_frame = 40;
        session.last_progress = Some((1_000, (40, 90, 90)));
        session.last_response = Some(1_000 + STALL_MS);
        assert_eq!(session.recovery_status(1_000 + STALL_MS, 40), None);
        assert_eq!(
            session.recovery_status(1_000 + STALL_MS + 1, 40),
            Some(super::super::NetworkRecoveryStatus::Reconnecting)
        );
        session.pause.paused = true;
        assert_eq!(session.recovery_status(1_000 + STALL_MS + 1, 40), None);
    }

    fn paused_at_100() -> SequencedSession {
        let mut session = session_for(&[0, 1]);
        session.started = true;
        session.head_frame = 100;
        session.pause.paused = true;
        session.pause.frame = 100;
        session
    }

    /// A game the pause finds more than half a second behind says once that
    /// it caught up, when it reaches the pause.
    #[test]
    fn a_game_a_pause_finds_catching_up_says_once_that_it_caught_up() {
        let mut session = paused_at_100();
        assert!(
            !session.take_caught_up_in_pause(false, 84),
            "16 frames behind"
        );
        assert!(session.take_caught_up_in_pause(true, 101));
        assert!(!session.take_caught_up_in_pause(true, 101), "once a pause");
    }

    /// A game keeping up says nothing, nor does one a resume reaches first,
    /// nor a rejoining one replaying a pause the relay has already ended.
    #[test]
    fn a_game_keeping_up_or_resumed_before_the_pause_says_nothing() {
        let mut session = paused_at_100();
        assert!(
            !session.take_caught_up_in_pause(false, 85),
            "15 frames behind"
        );
        assert!(!session.take_caught_up_in_pause(true, 101));
        let mut session = paused_at_100();
        assert!(!session.take_caught_up_in_pause(false, 60));
        session.head_frame = 300;
        assert!(
            !session.take_caught_up_in_pause(true, 101),
            "the relay has resumed and run on"
        );
        let mut session = paused_at_100();
        assert!(!session.take_caught_up_in_pause(false, 60));
        session.pause.paused = false;
        assert!(!session.take_caught_up_in_pause(false, 70));
        session.pause.paused = true;
        session.head_frame = 200;
        session.pause.frame = 200;
        assert!(!session.take_caught_up_in_pause(true, 201), "a later pause");
    }

    /// Loading leaves the position still for many seconds; the start itself
    /// must not read as a stall before frame 1 arrives.
    #[test]
    fn the_start_of_a_match_is_not_a_stall() {
        let mut session = session_for(&[0, 1]);
        session.last_progress = Some((1_000, (0, 1, 1)));
        let now = 1_000 + 40_000;
        session.last_response = Some(now);
        session
            .receive(OrderEvent::Status(started_status()), Instant::now())
            .unwrap();
        assert_eq!(session.recovery_status(now, 0), None);
    }
}

#[cfg(all(test, tests_net))]
mod delivery_counter_tests {
    use super::fixtures::session_for;
    use super::*;

    /// A local batch's round trip runs from its first transmission to the
    /// receipt of the frame the relay sealed it into, counted once.
    #[test]
    fn a_sealed_frame_counts_each_local_batch_round_trip_once() {
        let mut session = session_for(&[0, 1]);
        let sent = Instant::now();
        session.command_traces.push_back(CommandDeliveryTrace {
            batch_id: 1,
            sent_at: sent,
            assigned: Some((1, sent)),
            frame_received_at: None,
        });
        let received = sent + std::time::Duration::from_millis(40);
        let frame_end =
            |sequence, frame| OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame });
        session.receive(frame_end(1, 1), received).unwrap();
        session.assemble().unwrap();
        session.receive(frame_end(2, 2), received).unwrap();
        session.assemble().unwrap();
        let counters = session.delivery_counters();
        assert_eq!(
            (
                counters.round_trips,
                counters.round_trip_total_ms,
                counters.round_trip_max_ms
            ),
            (1, 40, 40)
        );
    }
}

#[cfg(all(test, tests_net))]
mod departure_tests {
    use super::fixtures::{session_for, started_status};
    use super::*;
    use crate::sim::command::EXIT_OPCODE;

    /// Silence ends a place as native connection loss: EVENT_REMOVEPLAYER
    /// kills the House on every client, and only a remote House is reported
    /// for its line. A Leave stays EXIT.
    #[test]
    fn lost_departures_remove_the_house_and_leave_stays_exit() {
        let mut session = session_for(&[0, 1, 2]);
        let departure = |sequence, frame, source, lost| {
            OrderEvent::Entry(OrderEntry::Departure {
                sequence,
                frame,
                source,
                lost,
            })
        };
        for event in [
            departure(1, 1, 1, true),
            departure(2, 1, 0, true),
            OrderEvent::Entry(OrderEntry::FrameEnd {
                sequence: 3,
                frame: 1,
            }),
            departure(4, 2, 2, false),
            OrderEvent::Entry(OrderEntry::FrameEnd {
                sequence: 5,
                frame: 2,
            }),
        ] {
            session.receive(event, Instant::now()).unwrap();
        }
        session.assemble().unwrap();
        let frames: Vec<_> = session
            .frames
            .iter()
            .map(|frame| {
                let records: Vec<_> = frame
                    .records
                    .iter()
                    .map(|record| (record.opcode(), record.house_id(), record.payload()[0]))
                    .collect();
                (records, frame.lost.clone())
            })
            .collect();
        assert_eq!(
            frames,
            [
                (
                    vec![(REMOVE_PLAYER_OPCODE, 1, 1), (REMOVE_PLAYER_OPCODE, 0, 0)],
                    vec![1]
                ),
                (vec![(EXIT_OPCODE, 2, 0)], vec![]),
            ]
        );
    }

    /// A record this build cannot read ends the match once it has held the
    /// cursor for five seconds while the relay repeats it; one that holds
    /// nothing back is ignored.
    #[test]
    fn an_unreadable_record_ends_the_match_only_while_it_holds_the_cursor() {
        let mut session = session_for(&[0, 1]);
        session.mark_started_for_test();
        session.note_unreadable(0);
        session.check_unreadable(6_000).unwrap();
        // A newer departure at 1 holds the frame end that follows it.
        session.note_unreadable(6_000);
        let frame_end = OrderEntry::FrameEnd {
            sequence: 2,
            frame: 1,
        };
        session
            .receive(OrderEvent::Entry(frame_end), Instant::now())
            .unwrap();
        session.assemble().unwrap();
        for now in (6_500..=10_500).step_by(500) {
            session.note_unreadable(now);
        }
        session.check_unreadable(10_999).unwrap();
        session.note_unreadable(11_000);
        assert!(matches!(
            session.check_unreadable(11_000),
            Err(SequencedError::NewerClientRequired)
        ));
    }

    /// Before the start an unreadable Status ends the match the same way; a
    /// readable Status in between keeps it waiting.
    #[test]
    fn an_unreadable_status_ends_the_match_before_the_start() {
        use vera20k_relay_protocol::OrderStatus;
        let mut session = session_for(&[0, 1]);
        session.note_unreadable(0);
        let status = OrderStatus {
            started: false,
            ..started_status()
        };
        session
            .receive(OrderEvent::Status(status), Instant::now())
            .unwrap();
        session.check_unreadable(5_000).unwrap();
        for now in (5_000..=10_000).step_by(1_000) {
            session.note_unreadable(now);
        }
        assert!(matches!(
            session.check_unreadable(10_000),
            Err(SequencedError::NewerClientRequired)
        ));
    }

    /// A batch that does not decode departs its sender in its frame
    /// instead of leaving a sequence gap that every pull repeats, or ending
    /// the match: that sender's later batches and departure are ignored.
    #[test]
    fn an_unreadable_batch_departs_its_sender_in_its_frame() {
        let mut session = session_for(&[0, 1]);
        let unreadable = |sequence, batch_id| OrderEntry::Batch {
            sequence,
            frame: 2,
            source: 1,
            batch_id,
            bytes: b"not a batch".to_vec(),
        };
        let frame_end = |sequence, frame| OrderEntry::FrameEnd { sequence, frame };
        for event in [
            frame_end(1, 1),
            unreadable(2, 1),
            unreadable(3, 2),
            frame_end(4, 2),
            OrderEntry::Departure {
                sequence: 5,
                frame: 3,
                source: 1,
                lost: false,
            },
            frame_end(6, 3),
        ] {
            session
                .receive(OrderEvent::Entry(event), Instant::now())
                .unwrap();
        }
        session.assemble().unwrap();
        let frames: Vec<_> = session
            .frames
            .iter()
            .map(|frame| {
                let records: Vec<_> = frame
                    .records
                    .iter()
                    .map(|record| (record.opcode(), record.house_id()))
                    .collect();
                (frame.frame, records)
            })
            .collect();
        assert_eq!(
            frames,
            [(1, vec![]), (2, vec![(EXIT_OPCODE, 1)]), (3, vec![])]
        );
    }
}

#[cfg(all(test, tests_net))]
mod frame_limit_tests {
    use super::fixtures::session_for;
    use super::*;
    use crate::sim::command::DESTRUCT_OPCODE;

    /// The largest frame a federated match seals: each relay's whole share
    /// of batches, every batch a full OutList, and every player leaving
    /// after its last batch.
    #[test]
    fn the_largest_federated_frame_is_assembled() {
        let mut session = session_for(&(0..30).collect::<Vec<_>>());
        let batches = MAX_FRAME_BATCHES * MAX_FEDERATED_RELAYS;
        let mut sequence = 1..;
        let mut receive = |entry| {
            session
                .receive(OrderEvent::Entry(entry), Instant::now())
                .unwrap()
        };
        for index in 0..batches {
            let source = (index % 30) as u8;
            receive(OrderEntry::Batch {
                sequence: sequence.next().unwrap(),
                frame: 1,
                source,
                batch_id: (index / 30 + 1) as u64,
                bytes: vec![DESTRUCT_OPCODE; OUT_LIST_CAPACITY],
            });
            if index + 30 >= batches {
                receive(OrderEntry::Departure {
                    sequence: sequence.next().unwrap(),
                    frame: 1,
                    source,
                    lost: false,
                });
            }
        }
        receive(OrderEntry::FrameEnd {
            sequence: sequence.next().unwrap(),
            frame: 1,
        });
        session.assemble().unwrap();
        assert_eq!(session.frames.len(), 1);
        assert_eq!(
            session.frames[0].records.len(),
            batches * OUT_LIST_CAPACITY + 30
        );
    }
}

#[cfg(all(test, tests_net))]
mod pace_tests {
    use super::fixtures::{session_for, started_status};
    use super::*;

    /// Lagging needs frames waiting two seconds; behind also needs two report
    /// windows in a row that ran some frames but fewer than the clock sealed.
    /// A world that ran none, or one running faster, only catches up; a
    /// hidden window starts over.
    #[test]
    fn a_pc_is_behind_only_while_frames_wait_and_it_runs_some_but_too_few() {
        use OrderPace::*;
        let mut meter = PaceMeter::default();
        let window = |meter: &mut PaceMeter, now: u32, ran: u32| {
            meter.dispatched = ran;
            meter.report(now, 10, 30, true)
        };
        assert_eq!(meter.report(0, 10, 30, true), Some((KeepingUp, 30, None)));
        assert_eq!(meter.report(500, 3, 30, true), None);
        assert_eq!(window(&mut meter, 1000, 20), Some((KeepingUp, 20, None)));
        assert_eq!(window(&mut meter, 2000, 20), Some((KeepingUp, 20, None)));
        assert_eq!(window(&mut meter, 3000, 20), Some((Behind, 20, None)));
        assert_eq!(window(&mut meter, 4000, 0), Some((CatchingUp, 0, None)));
        assert_eq!(window(&mut meter, 5000, 20), Some((CatchingUp, 20, None)));
        assert_eq!(window(&mut meter, 6000, 60), Some((CatchingUp, 60, None)));
        meter.hidden = true;
        assert_eq!(window(&mut meter, 7000, 20), Some((KeepingUp, 20, None)));
        // Each report carries the window's 90th-percentile tick cost.
        for micros in (1..=10).map(|step| step * 1000) {
            meter.record_tick_cost(micros);
        }
        assert_eq!(
            window(&mut meter, 8000, 20),
            Some((KeepingUp, 20, Some(9000)))
        );
        assert_eq!(window(&mut meter, 9000, 20), Some((KeepingUp, 20, None)));
    }

    #[test]
    fn narrative_media_wait_does_not_report_slow_simulation_to_the_relay() {
        let mut session = session_for(&[0, 1]);
        session.frames.resize_with(20, CommittedFrame::default);
        let mut pace = None;
        for now in [0, 1000, 2000] {
            session.pace_meter.dispatched = 20;
            pace = session.pace_report(now).map(|(pace, ..)| pace);
        }
        assert_eq!(
            pace,
            Some(OrderPace::Behind),
            "frames waited while too few ran"
        );
        session.set_narrative_hold(Some(NarrativeHold::Player));
        assert!(!session.pause.paused);
        assert!(session.pause_refused || !session.pause_status().available);
        for now in [3000, 4000, 5000, 6000] {
            assert_eq!(
                session.pace_report(now).unwrap().0,
                OrderPace::KeepingUp,
                "a deliberate wait is not catch-up even if this relay cannot pause"
            );
        }
    }

    /// A rate marker paces the frames after it, keeps the simulation's
    /// rate, and names the slow player in the first frame it applies to.
    #[test]
    fn a_rate_marker_paces_later_frames_and_names_the_slow_player() {
        let mut session = session_for(&[0, 4]);
        let rate = |sequence, frame, frames_per_second, source| OrderEntry::Rate {
            sequence,
            frame,
            frames_per_second,
            source,
        };
        let frame_end = |sequence, frame| OrderEntry::FrameEnd { sequence, frame };
        for entry in [
            frame_end(1, 1),
            rate(2, 1, 15, Some(1)),
            frame_end(3, 2),
            rate(4, 2, 30, None),
            frame_end(5, 3),
        ] {
            session
                .receive(OrderEvent::Entry(entry), Instant::now())
                .unwrap();
        }
        session.assemble().unwrap();
        let speeds: Vec<_> = session.frames.iter().map(|frame| frame.speed).collect();
        assert_eq!(
            speeds,
            [
                None,
                Some(MatchSpeedChange::SlowedBy(4)),
                Some(MatchSpeedChange::Normal)
            ]
        );
        assert_eq!((session.fps, session.pace_fps), (30, 30));
        // Ten frames a second is three rows below this match's thirty.
        session
            .receive(OrderEvent::Entry(rate(6, 3, 10, Some(1))), Instant::now())
            .unwrap();
        assert!(matches!(
            session.assemble(),
            Err(SequencedError::InvalidTimeline)
        ));
    }

    #[test]
    fn narrative_pause_hides_both_notices_even_when_the_resume_arrives_after_release() {
        let admit_pause = |session: &mut SequencedSession, sequence, paused| {
            session
                .receive(
                    OrderEvent::Entry(OrderEntry::Pause {
                        sequence,
                        frame: 0,
                        epoch: sequence,
                        paused,
                        source: 1,
                    }),
                    Instant::now(),
                )
                .unwrap();
            session.assemble().unwrap();
        };
        let mut session = session_for(&[0, 4]);
        session.set_narrative_hold(Some(NarrativeHold::Player));
        admit_pause(&mut session, 1, true);
        assert!(session.pause_status().paused);
        let paused_notices = session.take_pause_changes();

        session.set_narrative_hold(None);
        admit_pause(&mut session, 2, false);
        assert!(!session.pause_status().paused);
        assert!(paused_notices.is_empty());
        assert!(session.assembling.pauses.is_empty());

        let mut manual = session_for(&[0, 4]);
        admit_pause(&mut manual, 1, true);
        assert_eq!(
            manual.take_pause_changes(),
            [MatchPauseChange {
                house: 4,
                paused: true
            }]
        );
        manual.set_narrative_hold(Some(NarrativeHold::Player));
        admit_pause(&mut manual, 2, false);
        assert_eq!(
            manual.assembling.pauses,
            [MatchPauseChange {
                house: 4,
                paused: false
            }]
        );
    }

    /// VERA: another player's pause names them with the frame it follows, or
    /// at once when that frame has dispatched, and a resume with the next
    /// frame. This game's own pause names nobody, nor does a resume whose
    /// player leaves in the next frame (the relay's own, for a pause every
    /// player left).
    #[test]
    fn another_players_pause_names_them_when_the_world_reaches_it() {
        let mut session = session_for(&[0, 4]);
        let pause = |sequence, frame, epoch, paused, source| OrderEntry::Pause {
            sequence,
            frame,
            epoch,
            paused,
            source,
        };
        let frame_end = |sequence, frame| OrderEntry::FrameEnd { sequence, frame };
        let by = |paused| MatchPauseChange { house: 4, paused };
        let receive = |session: &mut SequencedSession, entries: Vec<OrderEntry>| {
            for entry in entries {
                session
                    .receive(OrderEvent::Entry(entry), Instant::now())
                    .unwrap();
            }
            session.assemble().unwrap();
        };
        receive(
            &mut session,
            vec![
                frame_end(1, 1),
                pause(2, 1, 1, true, 1),
                pause(3, 1, 2, false, 1),
                frame_end(4, 2),
            ],
        );
        let named: Vec<_> = session
            .frames
            .iter()
            .map(|frame| frame.pauses.clone())
            .collect();
        assert_eq!(named, [vec![by(true)], vec![by(false)]]);
        // Both frames have dispatched: the world is already where the next
        // pause stops it.
        session.frames.clear();
        receive(&mut session, vec![pause(5, 2, 3, true, 1)]);
        assert_eq!(session.take_pause_changes(), [by(true)]);
        receive(
            &mut session,
            vec![
                pause(6, 2, 4, false, 1),
                frame_end(7, 3),
                pause(8, 3, 5, true, 0),
            ],
        );
        let named: Vec<_> = session
            .frames
            .iter()
            .map(|frame| frame.pauses.clone())
            .collect();
        assert_eq!(named, [vec![by(false)]]);
        assert!(session.take_pause_changes().is_empty());
        // A resume by a player who leaves in the next frame names nobody:
        // the relay's own resume for a pause every player left, which only
        // watchers read, comes this way.
        session.frames.clear();
        receive(
            &mut session,
            vec![
                pause(9, 3, 6, false, 1),
                OrderEntry::Departure {
                    sequence: 10,
                    frame: 4,
                    source: 1,
                    lost: false,
                },
                frame_end(11, 4),
            ],
        );
        assert_eq!(session.frames.len(), 1);
        assert!(session.frames[0].pauses.is_empty());
    }

    /// VERA: a player the relay stops hearing is named once, and once more
    /// when it hears them again; one who left while silent is not back, nor
    /// named again by a late STATUS, and this game's own player is never
    /// named.
    #[test]
    fn another_player_the_relay_stops_hearing_is_named_until_heard_again() {
        use vera20k_relay_protocol::OrderStatus;
        let mut session = session_for(&[0, 4, 7]);
        let status = |session: &mut SequencedSession, silent_sources: u32, active_sources: u32| {
            let status = OrderStatus {
                active_sources,
                silent_sources,
                ..started_status()
            };
            session
                .receive(OrderEvent::Status(status), Instant::now())
                .unwrap();
            session.take_connection_changes()
        };
        let named = |house, responding| MatchConnectionChange { house, responding };
        assert_eq!(status(&mut session, 0b011, 0b111), [named(4, false)]);
        assert!(status(&mut session, 0b011, 0b111).is_empty());
        assert_eq!(
            status(&mut session, 0b100, 0b111),
            [named(4, true), named(7, false)]
        );
        assert!(status(&mut session, 0, 0b011).is_empty());
        assert!(status(&mut session, 0b100, 0b111).is_empty());
    }
}

#[cfg(all(test, tests_net))]
mod watched_source_tests {
    use super::watched_source;

    /// A roster lacking one source takes that seat; one holding every source
    /// follows the first from no seat; any other is refused.
    #[test]
    fn a_watcher_follows_the_missing_seat_or_every_source_from_none() {
        assert_eq!(watched_source(0b111, 0b110), Some((0, true)));
        // One person against AI: the roster holds no other source.
        assert_eq!(watched_source(0b1, 0), Some((0, true)));
        assert_eq!(watched_source(0b111, 0b101), Some((1, true)));
        assert_eq!(watched_source(0b110, 0b110), Some((1, false)));
    }

    #[test]
    fn a_watcher_refuses_a_roster_that_is_not_its_matchs() {
        assert_eq!(watched_source(0b111, 0b100), None);
        assert_eq!(watched_source(0b011, 0b111), None);
        assert_eq!(watched_source(0, 0), None);
    }
}

#[cfg(all(test, tests_net))]
mod watch_tests {
    use super::fixtures::{session_for, started_status};
    use super::*;
    use vera20k_relay_protocol::OrderStatus;

    fn watcher() -> SequencedSession {
        let mut session = session_for(&[0, 1]);
        session.watch = Some(Watch {
            delay_seconds: 120,
            samples: VecDeque::new(),
            departed: 0,
        });
        session.mark_started_for_test();
        session
    }

    /// A watcher stands in the place of the player it follows: that
    /// player's loss gets a line, and the match is over once every player
    /// left and their frames ran.
    #[test]
    fn a_watcher_reads_its_players_stream_to_the_end() {
        let mut session = watcher();
        let departure = |sequence, source, lost| {
            OrderEvent::Entry(OrderEntry::Departure {
                sequence,
                frame: 1,
                source,
                lost,
            })
        };
        for event in [
            departure(1, 1, false),
            departure(2, 0, true),
            OrderEvent::Entry(OrderEntry::FrameEnd {
                sequence: 3,
                frame: 1,
            }),
        ] {
            session.receive(event, Instant::now()).unwrap();
        }
        session.assemble().unwrap();
        assert_eq!(
            session.frames.back().map(|frame| &frame.lost[..]),
            Some(&[0][..])
        );
        assert!(!session.watch_over());
        session.frames.clear();
        assert!(session.watch_over());
    }

    /// A watcher keeps every frame it reads however far its world is behind,
    /// so it has read a match before the relay forgets it.
    #[test]
    fn a_watcher_keeps_every_frame_it_reads() {
        let mut session = watcher();
        for frame in 1..=600 {
            let end = OrderEntry::FrameEnd {
                sequence: u64::from(frame),
                frame,
            };
            session
                .receive(OrderEvent::Entry(end), Instant::now())
                .unwrap();
        }
        session.assemble().unwrap();
        assert_eq!(session.frames.len(), 600);
    }

    /// A watcher's world is checked against each checksum every player
    /// reported alike once it has its own of that frame; a difference ends
    /// the watch, an equal one or a frame not reached yet does not.
    #[test]
    fn a_watcher_stops_where_its_world_differs_from_the_players() {
        let mut session = watcher();
        let status = |sample| {
            OrderEvent::Status(OrderStatus {
                head_frame: 600,
                watch_sample: Some(sample),
                ..started_status()
            })
        };
        session.receive(status((240, 7)), Instant::now()).unwrap();
        session.receive(status((480, 9)), Instant::now()).unwrap();
        session.samples.extend([(240, 7), (480, 8)]);
        session.check_watch_samples(300).unwrap();
        assert!(matches!(
            session.check_watch_samples(480),
            Err(SequencedError::WatchDesync {
                frame: 480,
                local: 8,
                remote: 9
            })
        ));
    }
}
