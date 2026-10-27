//! The live protocol-2 Queue_AI visit, from ProcessNetwork6475F0 through
//! SendOutList649CA0 and Wait_For_Players648710. Simulation owns commands;
//! this process-session owner retains only transport, packet exchange and the
//! current post-Logic visit while the client continues servicing its UI.
//!
//! Direct UDP LAN is an explicit transport adaptation; retail LAN selected
//! IPX sockets while this shared protocol-2 command behavior is retained.
//! Native wait-dialog/cancel, timeout recovery and diagnostic drawing remain
//! client-session work; this owner never invents a wait timeout or peer exit.

use std::io;
use std::net::SocketAddrV4;
use std::num::NonZeroU32;

mod sequenced;
mod startup;
mod timing;
pub use sequenced::{SequencedError, SequencedTiming};
pub use startup::{StartupAgreement, StartupError};

use crate::sim::world::{NetworkCommandQueues, NetworkQueueContext, NetworkQueuePass};

use super::command_exchange::{
    CommandExchange, CommandExchangeError, CommandPeerProgress, FrameWait, RetainedSendProgress,
    SendPoll,
};
use super::lockstep::NetworkSendPolicy;
use super::reliable::{Delivery, LanConnections, RetryTiming};
use super::udp::{LanUdpTransport, UdpServiceReport};

// Ordinary648060 passes180 for either transport mode.
// Both are6C8C40 wall-clock buckets (timeGetTime>>4), not simulation frames.
const ORDINARY_PROBE_INTERVAL: i32 = 180;
//648930..649529 handles at most64 private messages before checking progress.
const WAIT_RECEIVE_BUDGET: usize = 64;

/// ProcessNetwork647919..647954 indexes the current GameMode's row at
/// 837340. The relay changes physical routing, not this timing selection.
/// Source: original gamemd.exe instructions and the row bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveCommandMode {
    Lan,
    Wol,
}

/// Local presentation only; these facts never affect the committed clock or
/// another participant's progress. A disconnected player keeps their world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkRecoveryStatus {
    Reconnecting,
    CatchingUp { frames: u32 },
}

impl LiveCommandMode {
    fn initial_probe_interval(self) -> i32 {
        match self {
            Self::Lan => 60,
            Self::Wol => 120,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LiveCommandError {
    #[error(transparent)]
    Sequenced(#[from] SequencedError),
    #[error("invalid local House {0}")]
    InvalidLocalHouse(i8),
    #[error("invalid remote House {0}")]
    InvalidRemoteHouse(i32),
    #[error("remote House {house_id} has no direct endpoint: {endpoint}")]
    UnboundPeer {
        house_id: i32,
        endpoint: SocketAddrV4,
    },
    #[error("MaxAhead must be nonnegative, got {0}")]
    InvalidMaxAhead(i32),
    #[error("expected network frame {expected}, got {actual}")]
    FrameSequence { expected: u32, actual: u32 },
    #[error("post-Logic values changed while network frame {0} was retained")]
    ChangedFrame(u32),
    #[error("network frame exchange has already failed")]
    Failed,
    #[error(transparent)]
    Startup(#[from] StartupError),
    #[error(transparent)]
    Exchange(#[from] CommandExchangeError),
}

#[derive(Debug, Clone, Copy)]
enum FramePhase {
    InitialBarrier {
        entered_at: u32,
        response_time_at_entry: u32,
        last_probe: u32,
    },
    Sending {
        entered_at: u32,
        response_time_at_entry: u32,
        last_probe: u32,
    },
    Waiting {
        entered_at: u32,
        response_time_at_entry: u32,
        last_probe: u32,
    },
    Finished(NetworkQueuePass),
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveWaitKind {
    Initial,
    Ordinary,
}

/// Diagnostic projection of the existing retained visit. This does not
/// distinguish backpressure causes; queue and mirror facts remain authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveCommandPhase {
    InitialBarrier,
    Sending,
    Waiting,
    Finished,
    Failed,
}

/// The current648710 invocation, borrowed as facts by the app's existing
/// dialog/recovery owner. Recovery restarts countdowns without changing entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveWaitInfo {
    pub mode: LiveCommandMode,
    pub kind: LiveWaitKind,
    pub entered_at: u32,
    ///64800E..64803A samples this before ordinary Wait_For_Players;
    ///later acknowledgments cannot change that invocation's dialog period.
    pub response_time_at_entry: u32,
}

#[derive(Debug, Clone, Copy)]
struct CurrentFrame {
    context: NetworkQueueContext,
    max_ahead: i32,
    phase: FramePhase,
}

/// One established match's live command channel. CRC history remains with
/// the existing caller's checksum owner. A retained frame never re-runs
/// Logic, recreates an encoded packet, or owns a second gameplay queue.
#[derive(Debug)]
pub struct LiveCommandSession {
    transport: LanUdpTransport,
    exchange: CommandExchange,
    startup: startup::StartupHandshake,
    local_house: i8,
    initial_seed: u32,
    policy: NetworkSendPolicy,
    timing: timing::LiveTiming,
    mode: LiveCommandMode,
    current: Option<CurrentFrame>,
    last_service: Option<UdpServiceReport>,
    sequenced: Option<sequenced::SequencedSession>,
    sequenced_failed: bool,
}

impl LiveCommandSession {
    pub fn new(
        agreement: StartupAgreement,
        transport: LanUdpTransport,
        local_house: i8,
        initial_seed: u32,
        frame_send_rate: NonZeroU32,
        mode: LiveCommandMode,
    ) -> Result<Self, LiveCommandError> {
        if local_house < 0 {
            return Err(LiveCommandError::InvalidLocalHouse(local_house));
        }
        let mut houses = Vec::new();
        for (id, endpoint) in transport.connections().peers() {
            let house = i8::try_from(id)
                .ok()
                .filter(|&house| house >= 0 && house != local_house)
                .ok_or(LiveCommandError::InvalidRemoteHouse(id))?;
            if endpoint.port() == 0
                || endpoint.ip().is_unspecified()
                || endpoint.ip().is_broadcast()
                || endpoint.ip().is_multicast()
            {
                return Err(LiveCommandError::UnboundPeer {
                    house_id: id,
                    endpoint,
                });
            }
            houses.push(house);
        }
        // Intentional modern online policy: solo AI needs no command lead
        // time. The original admission is immutable; losing remote players
        // later must never switch an existing multiplayer game to local input.
        let policy = if mode == LiveCommandMode::Wol && agreement.is_original_singleton() {
            NetworkSendPolicy::Local
        } else {
            NetworkSendPolicy::FrameSendRate2 { frame_send_rate }
        };
        let sequenced = if mode == LiveCommandMode::Wol
            && !agreement.is_original_singleton()
            && transport.ordering_certificate().is_some()
        {
            Some(sequenced::SequencedSession::new(
                &agreement,
                &transport,
                local_house,
            )?)
        } else {
            None
        };
        let startup = startup::StartupHandshake::new(
            agreement,
            local_house,
            houses.iter().copied(),
            initial_seed,
            frame_send_rate,
        )?;
        Ok(Self {
            transport,
            exchange: CommandExchange::new(houses)?,
            startup,
            local_house,
            initial_seed,
            policy,
            timing: timing::LiveTiming::default(),
            mode,
            current: None,
            last_service: None,
            sequenced,
            sequenced_failed: false,
        })
    }

    pub fn primary_addr(&self) -> io::Result<SocketAddrV4> {
        self.transport.primary_addr()
    }

    pub fn alternate_addr(&self) -> io::Result<SocketAddrV4> {
        self.transport.alternate_addr()
    }

    ///Transfer the same socket/global-channel owner back to the session
    ///lobby after gameplay. No second transport is constructed during return.
    pub fn into_transport(mut self) -> LanUdpTransport {
        if self.uses_sequenced_lockstep() {
            self.transport.retire_relay();
        }
        self.transport
    }

    pub fn uses_sequenced_lockstep(&self) -> bool {
        self.sequenced.is_some()
    }

    /// Replay records this immutable admission cadence once at startup. Solo
    /// online Local input and relay commits both dispatch each frame; ordinary
    /// LAN retains its configured native send cadence.
    pub fn dispatches_every_frame(&self) -> bool {
        self.uses_sequenced_lockstep() || self.policy.should_send(1)
    }

    pub fn has_terminal_network_error(&self) -> bool {
        self.sequenced_failed
            || self
                .sequenced
                .as_ref()
                .is_some_and(|session| session.failed())
    }

    /// Highest complete immutable frame buffered by this client. The relay's
    /// advertised head alone never admits simulation or skips missing bytes.
    pub fn committed_frontier(&self) -> u32 {
        self.sequenced
            .as_ref()
            .map_or(0, |session| session.frontier())
    }

    pub fn fixed_frames_per_second(&self) -> Option<u16> {
        self.sequenced.as_ref().map(|session| session.fps)
    }

    /// App-only pacing observation. Availability and canonical dispatch remain
    /// owned by this session, irrespective of presentation's chosen cadence.
    pub fn sequenced_timing(&self) -> Option<SequencedTiming> {
        self.sequenced.as_ref()?.timing()
    }

    pub fn recovery_status(
        &self,
        now_millis: u32,
        current_frame: u32,
    ) -> Option<NetworkRecoveryStatus> {
        self.sequenced
            .as_ref()?
            .recovery_status(now_millis, current_frame)
    }

    pub fn relay_expires(&self) -> Option<u64> {
        self.transport.relay_expires()
    }

    pub fn renew_relay(
        &mut self,
        address: SocketAddrV4,
        ticket: &[u8],
        key: [u8; 32],
        payload_key: [u8; 32],
    ) -> io::Result<()> {
        self.transport
            .renew_relay(address, ticket, key, payload_key)
    }

    pub fn connections(&self) -> super::udp::UdpReadGuard<'_, LanConnections> {
        self.transport.connections()
    }

    pub fn global(&self) -> super::udp::UdpReadGuard<'_, super::global::GlobalChannel> {
        self.transport.global()
    }

    pub fn global_mut(&mut self) -> super::udp::UdpWriteGuard<'_, super::global::GlobalChannel> {
        self.transport.global_mut()
    }

    pub fn pop_global_received(&mut self) -> Option<super::global::GlobalMessage> {
        self.transport.pop_global_received()
    }

    ///48D1E0 services the same manager while demultiplexing global traffic.
    ///Private command messages remain queued for the reached Wait phase.
    pub fn service_transport(&mut self, now_wall_bucket: u32) {
        self.flush_startup();
        retain_service_report(
            &mut self.last_service,
            self.transport.service(now_wall_bucket),
        );
        if let Some(session) = &mut self.sequenced {
            session.service(&mut self.transport, now_wall_bucket.wrapping_shl(4));
        }
    }

    /// Service the same transport and send fresh sequenced input while the
    /// canonical frontier is closed. Queue ownership stays in Simulation;
    /// this does not advance Logic or dispatch received commands. Native LAN
    /// keeps its existing post-Logic transfer boundary.
    pub fn service_pending_commands(
        &mut self,
        queues: &mut NetworkCommandQueues<'_>,
        now_wall_bucket: u32,
    ) {
        self.service_transport(now_wall_bucket);
        if !self.sequenced_failed
            && let Some(session) = &mut self.sequenced
        {
            session.service_pending_commands(
                &mut self.transport,
                queues,
                now_wall_bucket.wrapping_shl(4),
            );
        }
    }

    pub fn send_global(
        &mut self,
        payload: &[u8],
        reliable: bool,
        destination: super::global::GlobalDestination,
    ) -> bool {
        self.transport.global_mut().send(
            payload,
            destination,
            if reliable {
                Delivery::Reliable
            } else {
                Delivery::Unreliable
            },
            0,
        )
    }

    pub fn peers(&self) -> &[CommandPeerProgress] {
        self.exchange.peers()
    }

    pub fn sent_commands(&self) -> u64 {
        if let Some(session) = &self.sequenced {
            return session.sent_commands();
        }
        self.exchange.sent_commands()
    }

    pub fn retained_send(&self) -> Option<RetainedSendProgress> {
        self.exchange.retained_send()
    }

    pub fn phase(&self) -> Option<LiveCommandPhase> {
        self.current.map(|current| match current.phase {
            FramePhase::InitialBarrier { .. } => LiveCommandPhase::InitialBarrier,
            FramePhase::Sending { .. } => LiveCommandPhase::Sending,
            FramePhase::Waiting { .. } => LiveCommandPhase::Waiting,
            FramePhase::Finished(_) => LiveCommandPhase::Finished,
            FramePhase::Failed => LiveCommandPhase::Failed,
        })
    }

    pub fn queued_send_count(&self, house: i8) -> Option<usize> {
        self.transport
            .connections()
            .queued_send_count(i32::from(house))
    }

    /// Existing reliable response history, in16ms wall-clock buckets.
    pub fn peer_response_time(&self, house: i8) -> Option<u32> {
        self.transport
            .connections()
            .mean_response_time(i32::from(house))
    }

    ///Manager ResponseTime542450 uses private histories, excluding primary
    ///global traffic. Its optional extra channel has no direct-LAN owner.
    pub fn response_time(&self) -> u32 {
        self.transport.response_time()
    }

    /// MainTick55D5A4..55D7C2 adds10ms at each quarter-window lag
    /// threshold in WOL. Peer lag comes from Receive64A3F4/652110, not a
    /// new clock or independently maintained progress table.
    pub fn pacing_delay_millis(&self, max_ahead: i32) -> u32 {
        if self.uses_sequenced_lockstep() {
            return 0;
        }
        if self.mode != LiveCommandMode::Wol || max_ahead <= 30 {
            return 0;
        }
        let lag = self
            .exchange
            .peers()
            .iter()
            .map(|peer| peer.lag_frames())
            .fold(0, i32::max);
        if lag == 0 {
            return 0;
        }
        [max_ahead / 4, max_ahead / 2, max_ahead.wrapping_mul(3) / 4]
            .into_iter()
            .filter(|threshold| lag >= *threshold)
            .count() as u32
            * 10
    }

    pub fn configure_precalculated_timing(&mut self, max_ahead: i32, frames_per_second: i32) {
        self.timing
            .configure_precalculated(max_ahead, frames_per_second);
    }

    pub fn record_processing_time(&mut self, milliseconds: u32) {
        self.timing.record_processing_time(milliseconds);
    }

    pub fn begin_processing(&mut self, now_millis: u32) {
        self.timing.begin_processing(now_millis);
    }

    pub fn finish_processing(&mut self, now_millis: u32) {
        self.timing.finish_processing(now_millis);
    }

    /// Keep the private connection and its progress row in the same lifetime.
    pub fn remove_peer(&mut self, house: i8) -> bool {
        if !self.exchange.remove_peer(house) {
            return false;
        }
        if !self.uses_sequenced_lockstep() {
            self.startup.abort();
        }
        assert!(
            self.transport
                .connections_mut()
                .remove_peer(i32::from(house)),
            "the admitted connection and command progress share one roster"
        );
        true
    }

    /// Read-only reference to the immutable install-time certificate. Transport
    /// controls remain private to the existing startup/reliability owners.
    pub fn startup_agreement(&self) -> &StartupAgreement {
        self.startup.agreement()
    }

    /// A kick/cancel before release cannot elect a smaller initial set. The
    /// snapshot remains immutable even while ordinary cleanup removes sockets.
    /// A failed startup remains uncertified throughout that cleanup.
    pub fn startup_pending(&self) -> bool {
        if let Some(session) = &self.sequenced {
            return !session.started();
        }
        !self.startup.released()
    }

    pub fn set_retry_timing(&mut self, timing: RetryTiming) {
        self.transport.connections_mut().set_timing(timing);
    }

    /// Diagnostic facts from every service pass in the last poll. Native
    /// 647F63,649FB5,648B94 and649518 ignore the service result; the app's
    /// Wait_For_Players timers own recovery. A later successful service in
    /// the same poll does not erase an earlier failure diagnostic.
    pub fn take_service_report(&mut self) -> Option<UdpServiceReport> {
        self.last_service.take()
    }

    /// The same aggregate consumed by take_service_report, reset at each poll.
    /// Readers must project numeric counters, never Debug-format its errors.
    pub fn service_report(&self) -> Option<&UdpServiceReport> {
        self.last_service.as_ref()
    }

    pub fn awaiting_initial_barrier(&self) -> bool {
        if let Some(session) = &self.sequenced {
            return !session.started();
        }
        self.current
            .is_some_and(|frame| matches!(frame.phase, FramePhase::InitialBarrier { .. }))
    }

    pub fn frame_wait(&self) -> Option<FrameWait> {
        let current = self.current?;
        matches!(
            current.phase,
            FramePhase::Waiting { .. } | FramePhase::Sending { .. }
        )
        .then(|| {
            self.exchange
                .frame_wait(current.context.frame as i32, current.max_ahead)
        })
    }

    pub fn wait_info(&self) -> Option<LiveWaitInfo> {
        let (kind, entered_at, response_time_at_entry) = match self.current?.phase {
            FramePhase::InitialBarrier {
                entered_at,
                response_time_at_entry,
                ..
            } => (LiveWaitKind::Initial, entered_at, response_time_at_entry),
            FramePhase::Sending {
                entered_at,
                response_time_at_entry,
                ..
            }
            | FramePhase::Waiting {
                entered_at,
                response_time_at_entry,
                ..
            } => (LiveWaitKind::Ordinary, entered_at, response_time_at_entry),
            _ => return None,
        };
        Some(LiveWaitInfo {
            mode: self.mode,
            kind,
            entered_at,
            response_time_at_entry,
        })
    }

    ///64B410 selects the first minimum signed frame from the same progress
    ///rows that gate dispatch. No separate stalled-peer frame table is kept.
    pub fn lowest_frame_peer(&self) -> Option<&CommandPeerProgress> {
        self.exchange.peers().iter().reduce(|first, next| {
            if next.frame() < first.frame() {
                next
            } else {
                first
            }
        })
    }

    ///64926F..6492A1 restarts the probe timer after recovery while the same
    ///Wait_For_Players invocation keeps its original elapsed-time anchor.
    pub fn restart_wait_probe(&mut self, now: u32) -> bool {
        let Some(current) = &mut self.current else {
            return false;
        };
        match &mut current.phase {
            FramePhase::InitialBarrier { last_probe, .. }
            | FramePhase::Sending { last_probe, .. }
            | FramePhase::Waiting { last_probe, .. } => {
                *last_probe = now;
                true
            }
            _ => false,
        }
    }

    /// Poll the same immutable post-Logic visit until Dispatch/SkipDispatch.
    ///The clock arguments must come from one sample: now_wall_bucket is
    ///now_millis>>4. Keep raw milliseconds for the native wait-bar reference.
    ///On failure
    /// the retained visit is terminal, so a partially admitted packet cannot
    /// accidentally be replayed by polling it again.
    pub fn poll(
        &mut self,
        queues: &mut NetworkCommandQueues<'_>,
        context: NetworkQueueContext,
        max_ahead: i32,
        now_wall_bucket: u32,
        now_millis: u32,
        latency_fudge: i32,
        logic_frame_rate: u32,
    ) -> Result<NetworkQueuePass, LiveCommandError> {
        self.last_service = None;
        if self.sequenced_failed {
            return Err(LiveCommandError::Failed);
        }
        if let Some(session) = &mut self.sequenced {
            retain_service_report(
                &mut self.last_service,
                self.transport.service(now_wall_bucket),
            );
            // Transport-only service receives the same wall bucket. Do not
            // alternate raw milliseconds with its rounded value: a later
            // rounded sample can otherwise underflow sequenced retry timers.
            let result = session.poll(
                &mut self.transport,
                queues,
                context,
                now_wall_bucket.wrapping_shl(4),
            );
            self.sequenced_failed = result.is_err();
            return result;
        }
        let result = self.poll_inner(
            queues,
            context,
            max_ahead,
            now_wall_bucket,
            now_millis,
            latency_fudge,
            logic_frame_rate,
        );
        if result.is_err()
            && let Some(current) = &mut self.current
        {
            current.phase = FramePhase::Failed;
        }
        result
    }

    fn poll_inner(
        &mut self,
        queues: &mut NetworkCommandQueues<'_>,
        context: NetworkQueueContext,
        max_ahead: i32,
        now: u32,
        now_millis: u32,
        latency_fudge: i32,
        logic_frame_rate: u32,
    ) -> Result<NetworkQueuePass, LiveCommandError> {
        if self.startup.failed() {
            return Err(StartupError::Aborted.into());
        }
        self.flush_startup();
        if max_ahead < 0 {
            return Err(LiveCommandError::InvalidMaxAhead(max_ahead));
        }
        match self.current {
            None => {
                if context.frame != 0 {
                    return Err(LiveCommandError::FrameSequence {
                        expected: 0,
                        actual: context.frame,
                    });
                }
                //647825 resets progress/cache exactly once on first frame.
                self.exchange.reset();
                self.current = Some(CurrentFrame {
                    context,
                    max_ahead,
                    phase: FramePhase::InitialBarrier {
                        entered_at: now,
                        response_time_at_entry: self.transport.response_time(),
                        last_probe: now,
                    },
                });
                //647914 sends one initial probe before entering648710.
                self.send_probe(context.frame, max_ahead)?;
                self.exchange.begin_wait(now_millis);
            }
            Some(current) if matches!(current.phase, FramePhase::Failed) => {
                return Err(LiveCommandError::Failed);
            }
            Some(current) if current.context.frame == context.frame => {
                if current.context.checksum != context.checksum || current.max_ahead != max_ahead {
                    return Err(LiveCommandError::ChangedFrame(context.frame));
                }
            }
            Some(current) => {
                let expected = current.context.frame.wrapping_add(1);
                if !matches!(current.phase, FramePhase::Finished(_)) || context.frame != expected {
                    return Err(LiveCommandError::FrameSequence {
                        expected,
                        actual: context.frame,
                    });
                }
                self.current = Some(CurrentFrame {
                    context,
                    max_ahead,
                    phase: FramePhase::Sending {
                        entered_at: now,
                        response_time_at_entry: self.transport.response_time(),
                        last_probe: now,
                    },
                });
            }
        }
        loop {
            let current = self.current.expect("started frame visit");
            match current.phase {
                FramePhase::InitialBarrier {
                    entered_at,
                    response_time_at_entry,
                    last_probe,
                } => {
                    if probe_due(last_probe, now, self.mode.initial_probe_interval()) {
                        self.current.as_mut().unwrap().phase = FramePhase::InitialBarrier {
                            entered_at,
                            response_time_at_entry,
                            last_probe: now,
                        };
                        self.send_probe(context.frame, max_ahead)?;
                    }
                    self.service_and_receive(now, queues, logic_frame_rate)?;
                    self.flush_startup();
                    if !self.startup.complete() || !self.exchange.initial_ready() {
                        return Ok(NetworkQueuePass::Waiting);
                    }
                    self.startup.release();
                    self.exchange.finish_initial_barrier();
                    //647B2E/647B32: only a successful initial wait clears
                    //manager response histories, after the peer-frame clear.
                    self.transport.reset_response_time(true);
                    self.current.as_mut().unwrap().phase = FramePhase::Sending {
                        entered_at: now,
                        response_time_at_entry: self.transport.response_time(),
                        last_probe: now,
                    };
                }
                FramePhase::Sending {
                    entered_at,
                    response_time_at_entry,
                    last_probe,
                } => {
                    if !self.exchange.sending() {
                        self.prepare_timing(queues, context.frame, max_ahead, latency_fudge);
                        if self.policy.should_send(context.frame) {
                            self.exchange.begin_wait(now_millis);
                        }
                    }
                    let transport = &mut self.transport;
                    let service_report = &mut self.last_service;
                    let sent = self.exchange.send_frame(
                        queues,
                        self.local_house,
                        context.frame,
                        max_ahead as u32,
                        context.checksum,
                        self.policy,
                        |bytes, guaranteed| {
                            let no_peers = transport.connections().peers().next().is_none();
                            let accepted = no_peers
                                || transport.connections_mut().broadcast(
                                    bytes,
                                    if guaranteed {
                                        Delivery::Reliable
                                    } else {
                                        Delivery::Unreliable
                                    },
                                );
                            // Service existing reliable queues after every
                            // attempt. A full FIFO resumes this Sending frame;
                            // no codec, mirror or counter is committed early.
                            retain_service_report(service_report, transport.service(now));
                            accepted
                        },
                    )?;
                    match sent {
                        SendPoll::Backpressure => {
                            if probe_due(last_probe, now, ORDINARY_PROBE_INTERVAL) {
                                self.current.as_mut().unwrap().phase = FramePhase::Sending {
                                    entered_at,
                                    response_time_at_entry,
                                    last_probe: now,
                                };
                                self.send_probe(context.frame, max_ahead)?;
                            }
                            self.service_and_receive(now, queues, logic_frame_rate)?;
                            return Ok(NetworkQueuePass::Waiting);
                        }
                        SendPoll::OffCadence => {
                            //647F5E..647F92 services transport only. It neither
                            //decodes queued game packets nor checks its result.
                            retain_service_report(
                                &mut self.last_service,
                                self.transport.service(now),
                            );
                            return Ok(self.finish(NetworkQueuePass::SkipDispatch));
                        }
                        SendPoll::Complete { .. } if context.frame == 0 => {
                            return Ok(self.finish(NetworkQueuePass::SkipDispatch));
                        }
                        SendPoll::Complete { .. } => {
                            self.current.as_mut().unwrap().phase = FramePhase::Waiting {
                                entered_at,
                                response_time_at_entry,
                                last_probe,
                            };
                        }
                    }
                }
                FramePhase::Waiting {
                    entered_at,
                    response_time_at_entry,
                    last_probe,
                } => {
                    if probe_due(last_probe, now, ORDINARY_PROBE_INTERVAL) {
                        self.current.as_mut().unwrap().phase = FramePhase::Waiting {
                            entered_at,
                            response_time_at_entry,
                            last_probe: now,
                        };
                        self.send_probe(context.frame, max_ahead)?;
                    }
                    self.service_and_receive(now, queues, logic_frame_rate)?;
                    return if self
                        .exchange
                        .frame_wait(context.frame as i32, max_ahead)
                        .ready()
                    {
                        Ok(self.finish(NetworkQueuePass::Dispatch))
                    } else {
                        Ok(NetworkQueuePass::Waiting)
                    };
                }
                FramePhase::Finished(pass) => return Ok(pass),
                FramePhase::Failed => return Err(LiveCommandError::Failed),
            }
        }
    }

    fn flush_startup(&mut self) {
        if self.uses_sequenced_lockstep() {
            return;
        }
        let transport = &mut self.transport;
        self.startup.flush(|bytes| {
            let no_peers = transport.connections().peers().next().is_none();
            no_peers
                || transport
                    .connections_mut()
                    .broadcast(bytes, Delivery::Reliable)
        });
    }

    fn prepare_timing(
        &mut self,
        queues: &mut NetworkCommandQueues<'_>,
        frame: u32,
        max_ahead: i32,
        latency_fudge: i32,
    ) {
        let Some(shared) = queues.timing() else {
            return;
        };
        let host = queues.timing_host_house() == Some(self.local_house);
        let rate = shared.frame_send_rate();
        // Multiplayer cadence follows ScenarioSession's EVENT_TIMING owner.
        // Local scheduling remains fixed by the original solo admission.
        if self.policy != NetworkSendPolicy::Local {
            self.policy = NetworkSendPolicy::FrameSendRate2 {
                frame_send_rate: NonZeroU32::new(u32::from(rate.get())).unwrap(),
            };
        }
        if frame != 0 && frame & 127 == 0 {
            let retry = timing::periodic_retry(self.transport.response_time(), latency_fudge);
            self.transport.set_retry_timing(retry, false);
            let event = crate::sim::command::ProcessTimeRecord {
                house_id: self.local_house,
                frame: frame as i32,
                milliseconds: self.timing.take_process_average(),
            };
            let _ = queues.issue_timing_record(event.encode().expect("fixed PROCESS_TIME record"));
            if self.mode == LiveCommandMode::Wol && frame & 2047 == 0 {
                self.transport.reset_response_time(false);
            }
        }
        if !host {
            return;
        }
        let event = if let Some(event) = self
            .timing
            .take_precalculated(self.local_house, frame as i32)
        {
            Some(event)
        } else if frame & 255 == 0 && self.exchange.sent_commands() >= 5 {
            let Some(milliseconds) = queues.timing().and_then(|timing| {
                timing.maximum_processing_millis(
                    std::iter::once(self.local_house)
                        .chain(self.exchange.peers().iter().map(|peer| peer.house_id())),
                )
            }) else {
                return;
            };
            let (event, retry) = timing::adaptive_timing(
                self.local_house,
                frame as i32,
                self.mode,
                milliseconds,
                queues.game_speed(),
                max_ahead,
                rate,
                self.transport.response_time(),
                self.transport.response_time(),
                latency_fudge,
                queues.fog_of_war(),
            );
            self.transport.set_retry_timing(retry, false);
            Some(event)
        } else {
            None
        };
        if let Some(mut event) = event {
            if self.policy == NetworkSendPolicy::Local {
                // Preserve the existing FPS/GameSpeed/CPU timing pipeline,
                // without widening its dispatcher retiming window over local
                // commands. Encode the current effective ahead value with
                // the same fog adjustment the receiver will subtract.
                event.max_ahead =
                    max_ahead.wrapping_add(if queues.fog_of_war() { 10 } else { 0 }) as u16;
                event.frame_send_rate = rate;
            }
            let _ = queues.issue_timing_record(event.encode().expect("fixed TIMING record"));
        }
    }

    fn finish(&mut self, pass: NetworkQueuePass) -> NetworkQueuePass {
        self.current.as_mut().expect("started frame").phase = FramePhase::Finished(pass);
        pass
    }

    fn send_probe(&mut self, frame: u32, max_ahead: i32) -> Result<(), LiveCommandError> {
        let bytes = self.exchange.frame_probe(
            self.local_house,
            frame,
            max_ahead as u32,
            self.initial_seed,
            self.policy,
        )?;
        //64A090 ignores admission failure; the next timer expiry tries again.
        self.transport
            .connections_mut()
            .broadcast(&bytes, Delivery::Unreliable);
        Ok(())
    }

    fn service_and_receive(
        &mut self,
        now: u32,
        queues: &mut NetworkCommandQueues<'_>,
        logic_frame_rate: u32,
    ) -> Result<(), LiveCommandError> {
        //648B94 and649518 ignore the manager's failure return and continue
        //receiving/checking progress. Neither retry exhaustion nor a socket
        //diagnostic invents an immediate Wait_For_Players result7 here.
        retain_service_report(&mut self.last_service, self.transport.service(now));
        let elapsed = self
            .wait_info()
            .map_or(0, |wait| now.wrapping_sub(wait.entered_at));
        for _ in 0..WAIT_RECEIVE_BUDGET {
            let Some(message) = self.transport.pop_received() else {
                break;
            };
            let source_house = i8::try_from(message.peer_id)
                .map_err(|_| LiveCommandError::InvalidRemoteHouse(message.peer_id))?;
            let control = self
                .startup
                .receive(source_house, &message.payload, message.delivery)?;
            let advanced = if control {
                false
            } else {
                self.exchange.receive(
                    source_house,
                    &message.payload,
                    self.initial_seed,
                    queues,
                    elapsed,
                )?
            };
            if advanced && self.mode == LiveCommandMode::Wol {
                self.exchange.record_peer_lag(
                    source_house,
                    self.current.expect("active frame").context.frame,
                    logic_frame_rate,
                    self.transport
                        .connections()
                        .mean_response_time(i32::from(source_house))
                        .unwrap_or(0),
                );
            }
            //649514..649518 services again after each private message. The
            //64-message budget still bounds this wait visit.
            retain_service_report(&mut self.last_service, self.transport.service(now));
        }
        Ok(())
    }
}

fn retain_service_report(current: &mut Option<UdpServiceReport>, next: UdpServiceReport) {
    let Some(current) = current else {
        *current = Some(next);
        return;
    };
    current.accumulate(next);
}

fn probe_due(start: u32, now: u32, interval: i32) -> bool {
    //6488DF..6488F1 uses a wrapping subtraction followed by signed JGE.
    (now.wrapping_sub(start) as i32) >= interval
}

#[cfg(test)]
#[path = "live_commands_tests.rs"]
mod tests;
