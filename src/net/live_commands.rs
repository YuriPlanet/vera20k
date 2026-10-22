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

use crate::sim::world::{NetworkCommandQueues, NetworkQueueContext, NetworkQueuePass};

use super::command_exchange::{
    CommandExchange, CommandExchangeError, CommandPeerProgress, FrameWait, SendPoll,
};
use super::lockstep::NetworkSendPolicy;
use super::reliable::{Delivery, LanConnections, RetryTiming};
use super::udp::{LanUdpTransport, UdpServiceReport};

// Mode3's original row837370 starts with60. Ordinary648060 passes180.
// Both are6C8C40 wall-clock buckets (timeGetTime>>4), not simulation frames.
const INITIAL_PROBE_INTERVAL: i32 = 60;
const ORDINARY_PROBE_INTERVAL: i32 = 180;
//648930..649529 handles at most64 private messages before checking progress.
const WAIT_RECEIVE_BUDGET: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum LiveCommandError {
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
    Exchange(#[from] CommandExchangeError),
}

#[derive(Debug, Clone, Copy)]
enum FramePhase {
    InitialBarrier {
        entered_at: u32,
        response_time_at_entry: u32,
        last_probe: u32,
    },
    Sending,
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

/// The current648710 invocation, borrowed as facts by the app's existing
/// dialog/recovery owner. Recovery restarts countdowns without changing entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LiveWaitInfo {
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
    local_house: i8,
    initial_seed: u32,
    policy: NetworkSendPolicy,
    current: Option<CurrentFrame>,
    last_service: Option<UdpServiceReport>,
}

impl LiveCommandSession {
    pub fn new(
        transport: LanUdpTransport,
        local_house: i8,
        initial_seed: u32,
        frame_send_rate: NonZeroU32,
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
        Ok(Self {
            transport,
            exchange: CommandExchange::new(houses)?,
            local_house,
            initial_seed,
            policy: NetworkSendPolicy::FrameSendRate2 { frame_send_rate },
            current: None,
            last_service: None,
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
    pub fn into_transport(self) -> LanUdpTransport {
        self.transport
    }

    pub fn connections(&self) -> &LanConnections {
        self.transport.connections()
    }

    pub fn global(&self) -> &super::global::GlobalChannel {
        self.transport.global()
    }

    pub fn global_mut(&mut self) -> &mut super::global::GlobalChannel {
        self.transport.global_mut()
    }

    pub fn pop_global_received(&mut self) -> Option<super::global::GlobalMessage> {
        self.transport.pop_global_received()
    }

    ///48D1E0 services the same manager while demultiplexing global traffic.
    ///Private command messages remain queued for the reached Wait phase.
    pub fn service_transport(&mut self, now_wall_bucket: u32) {
        retain_service_report(
            &mut self.last_service,
            self.transport.service(now_wall_bucket),
        );
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

    pub fn sent_commands(&self) -> u16 {
        self.exchange.sent_commands()
    }

    ///Manager ResponseTime542450 uses private histories, excluding primary
    ///global traffic. Its optional extra channel has no direct-LAN owner.
    pub fn response_time(&self) -> u32 {
        self.transport.response_time()
    }

    /// Keep the private connection and its progress row in the same lifetime.
    pub fn remove_peer(&mut self, house: i8) -> bool {
        if !self.exchange.remove_peer(house) {
            return false;
        }
        assert!(
            self.transport
                .connections_mut()
                .remove_peer(i32::from(house)),
            "the admitted connection and command progress share one roster"
        );
        true
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

    pub fn awaiting_initial_barrier(&self) -> bool {
        self.current
            .is_some_and(|frame| matches!(frame.phase, FramePhase::InitialBarrier { .. }))
    }

    pub fn frame_wait(&self) -> Option<FrameWait> {
        let current = self.current?;
        matches!(current.phase, FramePhase::Waiting { .. }).then(|| {
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
            FramePhase::Waiting {
                entered_at,
                response_time_at_entry,
                ..
            } => (LiveWaitKind::Ordinary, entered_at, response_time_at_entry),
            _ => return None,
        };
        Some(LiveWaitInfo {
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
    ) -> Result<NetworkQueuePass, LiveCommandError> {
        self.last_service = None;
        let result = self.poll_inner(queues, context, max_ahead, now_wall_bucket, now_millis);
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
    ) -> Result<NetworkQueuePass, LiveCommandError> {
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
                    phase: FramePhase::Sending,
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
                    if probe_due(last_probe, now, INITIAL_PROBE_INTERVAL) {
                        self.current.as_mut().unwrap().phase = FramePhase::InitialBarrier {
                            entered_at,
                            response_time_at_entry,
                            last_probe: now,
                        };
                        self.send_probe(context.frame, max_ahead)?;
                    }
                    self.service_and_receive(now, queues)?;
                    if !self.exchange.initial_ready() {
                        return Ok(NetworkQueuePass::Waiting);
                    }
                    self.exchange.finish_initial_barrier();
                    //647B2E/647B32: only a successful initial wait clears
                    //manager response histories, after the peer-frame clear.
                    self.transport.reset_response_time(true);
                    self.current.as_mut().unwrap().phase = FramePhase::Sending;
                }
                FramePhase::Sending => {
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
                            let accepted = transport.connections_mut().broadcast(
                                bytes,
                                if guaranteed {
                                    Delivery::Reliable
                                } else {
                                    Delivery::Unreliable
                                },
                            );
                            //649FA1..649FB5 ignores broadcast admission, then
                            //services the manager before the next packet. Do
                            //not dispatch received events during this phase.
                            retain_service_report(service_report, transport.service(now));
                            accepted
                        },
                    )?;
                    match sent {
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
                            self.exchange.begin_wait(now_millis);
                            self.current.as_mut().unwrap().phase = FramePhase::Waiting {
                                entered_at: now,
                                response_time_at_entry: self.transport.response_time(),
                                last_probe: now,
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
                    self.service_and_receive(now, queues)?;
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
            self.exchange.receive(
                source_house,
                &message.payload,
                self.initial_seed,
                queues,
                elapsed,
            )?;
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
    current.received_datagrams += next.received_datagrams;
    current.sent_datagrams += next.sent_datagrams;
    current.malformed_datagrams += next.malformed_datagrams;
    current.rejected_connections += next.rejected_connections;
    current.rejected_global += next.rejected_global;
    current.discarded_global_sends += next.discarded_global_sends;
    current.send_would_block += next.send_would_block;
    if next.last_rejection.is_some() {
        current.last_rejection = next.last_rejection;
    }
    if next.last_global_rejection.is_some() {
        current.last_global_rejection = next.last_global_rejection;
    }
    if next.failed_peer.is_some() {
        current.failed_peer = next.failed_peer;
    }
    if current.first_io_error.is_none() {
        current.first_io_error = next.first_io_error;
    }
}

fn probe_due(start: u32, now: u32, interval: i32) -> bool {
    //6488DF..6488F1 uses a wrapping subtraction followed by signed JGE.
    (now.wrapping_sub(start) as i32) >= interval
}

#[cfg(test)]
#[path = "live_commands_tests.rs"]
mod tests;
