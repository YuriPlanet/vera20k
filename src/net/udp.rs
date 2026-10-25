//! Direct IPv4 LAN socket owner for UDPInterfaceClass (`0x007B30B0`,
//! `0x007B3450`, `0x007B3AA0`). Reliable queues and endpoint identity stay in
//! `LanConnections`; this owner queues raw datagram attempts and adds/checks
//! their four-byte UDP checksum.
//!
//! Global messages admit unknown sources; private messages require an admitted
//! peer roster. Router/localhost rewriting, native self-source filtering, spare sockets and
//! platform socket-buffer tuning remain outside this direct-peer owner.

use std::collections::VecDeque;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle, Thread};
use std::time::Duration;

use vera20k_relay_protocol::{ClientSession, RelayError, ServerEvent};
pub use vera20k_relay_protocol::{
    MAX_PEERS as RELAY_MAX_PEERS, OrderingCertificate, Ticket as RelayTicket,
    peer_endpoint as relay_peer_endpoint,
};

#[cfg(any(test, feature = "test-support"))]
pub mod test_support {
    //Client regressions exercise the same authenticated server/codec owner.
    pub use vera20k_relay_protocol::{MAX_DATAGRAM, RelayServer, SecretKey, derive_packet_key};
}

use super::global::{
    GLOBAL_HEADER_LEN, GlobalChannel, GlobalConfig, GlobalDestination, GlobalMessage,
    GlobalReceiveError,
};
use super::lan_messages::{SessionScope, ingame::LoadingProgressMessage};
use super::reliable::{LanConnections, ReceiveError, ReceivedMessage, udp_checksum};

/// Original UDP port data at841F30/841F38. The alternate socket is a separate
/// receive path; it does not change the default destination port.
pub const DEFAULT_LAN_PORT: u16 = 1234;
pub const DEFAULT_ALTERNATE_PORT: u16 = 1236;
/// Original recvfrom7B3EDD supplies a640-byte datagram buffer.
pub const UDP_DATAGRAM_CAPACITY: usize = 640;
const CHECKSUM_LEN: usize = 4;
// VERA physical scheduling only: leave the guard between bounded receive
// passes, even when peers continuously replenish the socket. No extra inbox.
const IO_PUMP_RECEIVE_BUDGET: usize = 256;
const IO_PUMP_INTERVAL: Duration = Duration::from_millis(6);
// VERA-only match boundary, outside the native checksum/connection codec.
// Scope admission precedes every global/private ACK, sequence and receive cache.
const LAN_SCOPE_MAGIC: &[u8; 4] = b"VLS\x01";
const LAN_SCOPE_HEADER_LEN: usize = 4 + 16 + 8;

/// One admitted route progresses from the existing global loading channel to
/// private gameplay. Loading peers may finish on different service passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LanScopeState {
    Prepared(SessionScope),
    Active(SessionScope),
}

impl LanScopeState {
    fn scope(self) -> SessionScope {
        match self {
            Self::Prepared(scope) | Self::Active(scope) => scope,
        }
    }

    fn outgoing(self) -> Option<SessionScope> {
        match self {
            Self::Prepared(_) => None,
            Self::Active(scope) => Some(scope),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UdpBindConfig {
    pub primary: SocketAddrV4,
    pub alternate: SocketAddrV4,
}

impl Default for UdpBindConfig {
    fn default() -> Self {
        Self {
            primary: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DEFAULT_LAN_PORT),
            alternate: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DEFAULT_ALTERNATE_PORT),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpChannel {
    Primary,
    Alternate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UdpIoOperation {
    Send,
    Receive,
}

#[derive(Debug)]
pub struct UdpIoFailure {
    pub channel: UdpChannel,
    pub operation: UdpIoOperation,
    pub error: io::Error,
}

/// Facts from one nonblocking service pass. Malformed/unknown packets are
/// discarded without growing a diagnostic queue. The session's wait/recovery
/// owner decides what to do with I/O and retry diagnostics. Native interface
/// virtual+0x24 (7B25E0) clears SO_ERROR; it does not close the socket. A dropped
/// WouldBlock send remains owned by the reliable queue and may be retried there.
#[derive(Debug, Default)]
pub struct UdpServiceReport {
    pub received_datagrams: usize,
    pub sent_datagrams: usize,
    pub malformed_datagrams: usize,
    pub rejected_connections: usize,
    pub last_rejection: Option<ReceiveError>,
    pub rejected_global: usize,
    pub last_global_rejection: Option<GlobalReceiveError>,
    pub discarded_global_sends: usize,
    pub send_would_block: usize,
    pub failed_peer: Option<i32>,
    pub first_io_error: Option<UdpIoFailure>,
}

impl UdpServiceReport {
    /// Manager541820 fails its service result on discarded global sends or
    /// a failed private peer. I/O diagnostics have their separate consumers.
    pub fn delivery_failed(&self) -> bool {
        self.discarded_global_sends != 0 || self.failed_peer.is_some()
    }

    /// Same first-I/O-error/last-rejection policy for worker passes and a live poll.
    pub(crate) fn accumulate(&mut self, next: Self) {
        self.received_datagrams += next.received_datagrams;
        self.sent_datagrams += next.sent_datagrams;
        self.malformed_datagrams += next.malformed_datagrams;
        self.rejected_connections += next.rejected_connections;
        self.rejected_global += next.rejected_global;
        self.discarded_global_sends += next.discarded_global_sends;
        self.send_would_block += next.send_would_block;
        if next.last_rejection.is_some() {
            self.last_rejection = next.last_rejection;
        }
        if next.last_global_rejection.is_some() {
            self.last_global_rejection = next.last_global_rejection;
        }
        if next.failed_peer.is_some() {
            self.failed_peer = next.failed_peer;
        }
        if self.first_io_error.is_none() {
            self.first_io_error = next.first_io_error;
        }
    }
}

/// A derived global frontier and its still-unconsumed service failure, read
/// under one I/O-owner lock. Empty after a timeout is not an acknowledged drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalTransportObservation {
    pub queued_sends: usize,
    pub response_time: u32,
    pub pending_delivery_failure: bool,
}

#[derive(Debug)]
struct PendingDatagram {
    endpoint: SocketAddrV4,
    bytes: [u8; UDP_DATAGRAM_CAPACITY],
    length: usize,
}

/// Winsock7B23B0/UDP7B3AA0 owns attempts independently of reliable messages.
/// Native7B20E0 falls back to heap blocks after its128 pooled slots; there is
/// no fixed admission cap here. Only this FIFO owns retained wire bytes.
#[derive(Debug, Default)]
struct UdpSendQueue {
    pending: VecDeque<PendingDatagram>,
}

impl UdpSendQueue {
    fn send(
        &mut self,
        mut endpoint: SocketAddrV4,
        frame: &[u8],
        lan_scope: Option<SessionScope>,
        channel: UdpChannel,
        report: &mut UdpServiceReport,
        send: &mut impl FnMut(&[u8], SocketAddrV4) -> io::Result<usize>,
    ) {
        let scope_len = if lan_scope.is_some() {
            LAN_SCOPE_HEADER_LEN
        } else {
            0
        };
        if frame.len() > UDP_DATAGRAM_CAPACITY - CHECKSUM_LEN - scope_len {
            record_io_error(
                report,
                channel,
                UdpIoOperation::Send,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "connection frame exceeds UDP datagram capacity",
                ),
            );
            return;
        }
        //7B3C5B..7B3C75 substitutes1234 only for a zero destination port.
        if channel == UdpChannel::Primary && endpoint.port() == 0 {
            endpoint.set_port(DEFAULT_LAN_PORT);
        }
        let mut bytes = [0; UDP_DATAGRAM_CAPACITY];
        if let Some(scope) = lan_scope {
            bytes[..4].copy_from_slice(LAN_SCOPE_MAGIC);
            bytes[4..20].copy_from_slice(&scope.epoch);
            bytes[20..LAN_SCOPE_HEADER_LEN].copy_from_slice(&scope.revision.to_le_bytes());
        }
        bytes[scope_len..scope_len + CHECKSUM_LEN]
            .copy_from_slice(&udp_checksum(frame).to_le_bytes());
        bytes[scope_len + CHECKSUM_LEN..scope_len + CHECKSUM_LEN + frame.len()]
            .copy_from_slice(frame);
        self.pending.push_back(PendingDatagram {
            endpoint,
            bytes,
            length: scope_len + CHECKSUM_LEN + frame.len(),
        });
        //7B24A6..7B24B9 synchronously dispatches one write callback. An older
        //retained head is attempted first; the newly admitted block can wait.
        self.service(channel, report, send);
    }

    fn service(
        &mut self,
        channel: UdpChannel,
        report: &mut UdpServiceReport,
        send: &mut impl FnMut(&[u8], SocketAddrV4) -> io::Result<usize>,
    ) {
        let Some(head) = self.pending.front() else {
            return;
        };
        //Each native FD_WRITE callback7B3BA8..7B3EB8 attempts one head only.
        //Every non--1 result retires it, without checking the returned length.
        match send(&head.bytes[..head.length], head.endpoint) {
            Ok(_) => {
                self.pending.pop_front();
                report.sent_datagrams += 1;
            }
            //7B3D7F..7B3DFB also retires WSAEWOULDBLOCK. Reliable messages
            //remain in their separate owner until an ACK or failure.
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                self.pending.pop_front();
                report.send_would_block += 1;
            }
            //7B3D8B..7B3DB0 keeps the exact head for every other error,
            //including Interrupted. No later datagram overtakes it.
            Err(error) => record_io_error(report, channel, UdpIoOperation::Send, error),
        }
    }
}

/// Owns one socket pair and its reliable/global/relay state through lobby,
/// loading and gameplay. The optional physical I/O worker moves with this value;
/// application decoding and simulation admission remain on the calling thread.
pub struct LanUdpTransport {
    io: Arc<Mutex<UdpIo>>,
    pump: Option<IoPump>,
}

#[derive(Debug)]
struct UdpIo {
    primary: UdpSocket,
    alternate: UdpSocket,
    primary_send: UdpSendQueue,
    alternate_send: UdpSendQueue,
    connections: LanConnections,
    global: GlobalChannel,
    broadcast_targets: Vec<SocketAddrV4>,
    relay: Option<RelayRoute>,
    lan_scope: Option<LanScopeState>,
    pending_report: UdpServiceReport,
}

struct IoPump {
    stop: Arc<AtomicBool>,
    worker: JoinHandle<()>,
}

/// A short read of the existing I/O owner. Do not hold this guard across a
/// second transport access, application callback, JSON construction or file I/O.
pub struct UdpReadGuard<'a, T> {
    io: MutexGuard<'a, UdpIo>,
    project: fn(&UdpIo) -> &T,
}

impl<T> Deref for UdpReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        (self.project)(&self.io)
    }
}

/// Mutates the same owner and wakes its worker after releasing the guard.
pub struct UdpWriteGuard<'a, T> {
    io: Option<MutexGuard<'a, UdpIo>>,
    project: fn(&UdpIo) -> &T,
    project_mut: fn(&mut UdpIo) -> &mut T,
    wake: Option<Thread>,
}

impl<T> Deref for UdpWriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        (self.project)(self.io.as_ref().expect("live I/O guard"))
    }
}

impl<T> DerefMut for UdpWriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        (self.project_mut)(self.io.as_mut().expect("live I/O guard"))
    }
}

impl<T> Drop for UdpWriteGuard<'_, T> {
    fn drop(&mut self) {
        drop(self.io.take());
        if let Some(wake) = &self.wake {
            wake.unpark();
        }
    }
}

impl std::fmt::Debug for LanUdpTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let io = self.lock();
        f.debug_struct("LanUdpTransport")
            .field("primary", &ipv4_addr(&io.primary))
            .field("alternate", &ipv4_addr(&io.alternate))
            .field("background_io", &self.pump.is_some())
            .finish_non_exhaustive()
    }
}

impl LanUdpTransport {
    /// Bind remains synchronous for explicit/manual-clock core consumers.
    pub fn bind(config: UdpBindConfig, connections: LanConnections) -> io::Result<Self> {
        Ok(Self {
            io: Arc::new(Mutex::new(UdpIo::bind(config, connections)?)),
            pump: None,
        })
    }

    /// VERA scheduling adaptation, not a native background-thread claim.
    /// UDPInterface7B3AA0 services Windows read/write notifications on its
    /// caller; this worker preserves its queues/ACK/retry bodies during long
    /// Logic/render/capture work. The App injects its existing native wall
    /// bucket function, and only this worker samples it after taking the lock.
    /// Thread creation is fallible; failure leaves a synchronous transport.
    pub fn start_io_pump(&mut self, wall_bucket: fn() -> u32) -> io::Result<()> {
        if self.pump.is_some() {
            self.check_worker();
            return Ok(());
        }
        self.lock().pending_report = UdpServiceReport::default();
        let io = Arc::clone(&self.io);
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let worker = thread::Builder::new()
            .name("vera20k-udp-io".into())
            .spawn(move || {
                while !worker_stop.load(Ordering::Acquire) {
                    {
                        let mut io = io.lock().expect("UDP I/O owner poisoned");
                        if worker_stop.load(Ordering::Acquire) {
                            break;
                        }
                        // Sampling before lock acquisition could time packets
                        // with a stale frame-start sample after contention.
                        let report = io.service(wall_bucket(), IO_PUMP_RECEIVE_BUDGET);
                        io.pending_report.accumulate(report);
                    }
                    // Scheduling request only; spurious/late wakes still
                    // recheck stop and resample the clock on the next pass.
                    thread::park_timeout(IO_PUMP_INTERVAL);
                }
            })?;
        self.pump = Some(IoPump { stop, worker });
        Ok(())
    }

    fn stop_io_pump(&mut self) {
        if let Some(pump) = self.pump.take() {
            pump.stop.store(true, Ordering::Release);
            pump.worker.thread().unpark();
            // Never join under the I/O mutex. The worker holds the only other
            // Arc and exits before the exact socket pair can be closed.
            if let Err(panic) = pump.worker.join() {
                if !thread::panicking() {
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }

    fn check_worker(&self) {
        if let Some(pump) = &self.pump {
            assert!(
                !pump.worker.is_finished(),
                "UDP I/O worker stopped unexpectedly"
            );
        }
    }

    fn lock(&self) -> MutexGuard<'_, UdpIo> {
        self.check_worker();
        self.io.lock().expect("UDP I/O owner poisoned")
    }

    fn wake(&self) {
        if let Some(pump) = &self.pump {
            pump.worker.thread().unpark();
        }
    }

    fn read<T>(&self, project: fn(&UdpIo) -> &T) -> UdpReadGuard<'_, T> {
        UdpReadGuard {
            io: self.lock(),
            project,
        }
    }

    fn write<T>(
        &mut self,
        project: fn(&UdpIo) -> &T,
        project_mut: fn(&mut UdpIo) -> &mut T,
    ) -> UdpWriteGuard<'_, T> {
        UdpWriteGuard {
            io: Some(self.lock()),
            project,
            project_mut,
            wake: self.pump.as_ref().map(|pump| pump.worker.thread().clone()),
        }
    }

    pub fn connections(&self) -> UdpReadGuard<'_, LanConnections> {
        self.read(|io| &io.connections)
    }

    pub fn connections_mut(&mut self) -> UdpWriteGuard<'_, LanConnections> {
        self.write(|io| &io.connections, |io| &mut io.connections)
    }

    pub fn global(&self) -> UdpReadGuard<'_, GlobalChannel> {
        self.read(|io| &io.global)
    }

    pub fn global_mut(&mut self) -> UdpWriteGuard<'_, GlobalChannel> {
        self.write(|io| &io.global, |io| &mut io.global)
    }

    /// Keep a terminal queue change paired with its service result even when
    /// the physical worker runs between the App's poll and admission decision.
    /// Leave the report for its ordinary consumer; this snapshot consumes none.
    pub fn global_observation(&self) -> GlobalTransportObservation {
        let io = self.lock();
        GlobalTransportObservation {
            queued_sends: io.global.queued_send_count(),
            response_time: io.global.mean_response_time(),
            pending_delivery_failure: io.pending_report.delivery_failed(),
        }
    }

    pub fn prepare_lan_scope(&mut self, scope: SessionScope) -> io::Result<()> {
        self.lock().prepare_lan_scope(scope)
    }

    pub fn set_lan_scope(&mut self, scope: SessionScope) -> io::Result<()> {
        self.lock().set_lan_scope(scope)
    }

    pub fn lan_scope(&self) -> Option<SessionScope> {
        self.lock().lan_scope()
    }

    pub fn clear_lan_scope(&mut self) {
        self.lock().clear_lan_scope();
    }

    pub fn enable_relay(
        &mut self,
        address: SocketAddrV4,
        ticket: &[u8],
        packet_key: [u8; 32],
        payload_key: [u8; 32],
    ) -> io::Result<()> {
        let result = self
            .lock()
            .enable_relay(address, ticket, packet_key, payload_key);
        self.wake();
        result
    }

    pub fn relay_ready(&self) -> bool {
        self.lock().relay_ready()
    }
    pub fn relay_source(&self) -> Option<u8> {
        self.lock().relay_source()
    }
    pub fn relay_expires(&self) -> Option<u64> {
        self.lock().relay_expires()
    }

    /// HTTPS admission supplies a lobby-signed immutable match clock/roster.
    /// The UDP owner retains only this transport capability, never simulation.
    pub fn set_ordering_certificate(&mut self, certificate: Vec<u8>) -> io::Result<()> {
        let mut io = self.lock();
        let route = io
            .relay
            .as_mut()
            .filter(|route| route.retiring.is_none())
            .ok_or_else(|| io::Error::other("no relay route"))?;
        let parsed = vera20k_relay_protocol::OrderingCertificate::parse(&certificate)
            .map_err(relay_io_error)?;
        if parsed.sources_mask & (1 << route.session.source()) == 0 {
            return Err(io::Error::other(
                "ordering certificate excludes local source",
            ));
        }
        route.ordering_certificate = Some(certificate);
        Ok(())
    }

    pub fn ordering_certificate(&self) -> Option<Vec<u8>> {
        self.lock().relay.as_ref()?.ordering_certificate.clone()
    }

    pub fn configure_ordering(&mut self, agreement: [u8; 32]) -> io::Result<()> {
        self.send_ordering(|route| {
            let cert = route
                .ordering_certificate
                .as_ref()
                .ok_or(RelayError::Unregistered)?;
            route
                .session
                .configure_ordering(cert, agreement, &mut route.encoded)
        })
    }

    pub fn submit_ordering(&mut self, batch_id: u64, bytes: &[u8]) -> io::Result<()> {
        self.send_ordering(|route| {
            route
                .session
                .submit_ordering(batch_id, bytes, &mut route.encoded)
        })
    }

    pub fn pull_ordering(&mut self, next_sequence: u64) -> io::Result<()> {
        self.send_ordering(|route| {
            route
                .session
                .pull_ordering(next_sequence, &mut route.encoded)
        })
    }

    pub fn leave_ordering(&mut self) -> io::Result<()> {
        self.send_ordering(|route| route.session.leave_ordering(&mut route.encoded))
    }

    fn send_ordering(
        &mut self,
        encode: impl FnOnce(&mut RelayRoute) -> Result<(), RelayError>,
    ) -> io::Result<()> {
        let mut io = self.lock();
        let io = &mut *io;
        let route = io
            .relay
            .as_mut()
            .filter(|route| route.retiring.is_none())
            .ok_or_else(|| io::Error::other("no relay route"))?;
        encode(route).map_err(relay_io_error)?;
        io.primary
            .send_to(&route.encoded, route.address)
            .map(|_| ())
    }

    pub fn pop_ordering_received(&mut self) -> Option<vera20k_relay_protocol::OrderEvent> {
        self.lock().relay.as_mut()?.ordering_inbox.pop_front()
    }

    pub fn renew_relay(
        &mut self,
        address: SocketAddrV4,
        ticket: &[u8],
        key: [u8; 32],
        payload_key: [u8; 32],
    ) -> io::Result<()> {
        let result = self.lock().renew_relay(address, ticket, key, payload_key);
        self.wake();
        result
    }

    pub fn retire_relay(&mut self) {
        self.lock().retire_relay();
        self.wake();
    }

    pub fn relay_retirement_pending(&self) -> bool {
        self.lock()
            .relay
            .as_ref()
            .is_some_and(|route| route.retiring.is_some())
    }

    pub fn begin_relay_peer_probes(&mut self, sources: &[u8]) -> io::Result<()> {
        let result = self.lock().begin_relay_peer_probes(sources);
        self.wake();
        result
    }

    pub fn relay_peer_round_trips(&self) -> Option<Vec<(u8, u32)>> {
        self.lock().relay_peer_round_trips()
    }
    pub fn primary_addr(&self) -> io::Result<SocketAddrV4> {
        self.lock().primary_addr()
    }
    pub fn alternate_addr(&self) -> io::Result<SocketAddrV4> {
        self.lock().alternate_addr()
    }
    pub fn retry_timing(&self) -> super::reliable::RetryTiming {
        self.lock().retry_timing()
    }
    pub fn clear_pending_sends(&mut self) {
        self.lock().clear_pending_sends();
    }
    pub fn reset_session(&mut self) {
        self.lock().reset_session();
    }
    pub fn set_broadcast_targets(&mut self, targets: &[SocketAddrV4]) -> io::Result<()> {
        self.lock().set_broadcast_targets(targets)
    }
    pub fn response_time(&self) -> u32 {
        self.lock().response_time()
    }
    pub fn reset_response_time(&mut self, clear: bool) {
        self.lock().reset_response_time(clear);
    }
    pub fn set_retry_timing(
        &mut self,
        timing: super::reliable::RetryTiming,
        apply_to_global: bool,
    ) {
        self.lock().set_retry_timing(timing, apply_to_global);
    }

    /// Quiesce the worker and give terminal queued control one physical service
    /// pass before the caller drops these same sockets. A wake-only `service`
    /// can lose its final attempt when Drop stops the worker first.
    /// Sample the fresh wall bucket under the I/O lock, after the worker joins.
    /// This is a bounded attempt, not an ACK drain or a change to retry policy;
    /// ordinary Drop still discards unsent traffic without sampling a clock.
    pub fn service_before_close(&mut self, wall_bucket: fn() -> u32) -> UdpServiceReport {
        self.stop_io_pump();
        let mut io = self.lock();
        let report = io.service(wall_bucket(), IO_PUMP_RECEIVE_BUDGET);
        io.pending_report.accumulate(report);
        std::mem::take(&mut io.pending_report)
    }

    /// In worker mode, the caller's now may predate lengthy Logic/render work.
    /// Never apply it to I/O state. Wake the sole physical service owner and
    /// consume its aggregate once; manual-clock consumers retain exact service.
    pub fn service(&mut self, now_ticks: u32) -> UdpServiceReport {
        if self.pump.is_some() {
            self.wake();
            std::mem::take(&mut self.lock().pending_report)
        } else {
            self.lock().service(now_ticks, usize::MAX)
        }
    }

    pub fn pop_received(&mut self) -> Option<ReceivedMessage> {
        let message = self.lock().pop_received();
        self.wake();
        message
    }

    pub fn pop_global_received(&mut self) -> Option<GlobalMessage> {
        let message = {
            let mut io = self.lock();
            if self.pump.is_some() {
                // The old App pop -> service -> pop loop retires a consumed
                // head before inspecting the next control. Physical service
                // is asynchronous now; keep that LOGICAL receive step here,
                // through the same owner, with no clock, I/O or new inbox.
                // Indexed loading consumption and every unconsumed entry keep
                // their original order. Manual service semantics stay native.
                io.global.retire_consumed();
            }
            io.pop_global_received()
        };
        self.wake();
        message
    }
}

impl Drop for LanUdpTransport {
    fn drop(&mut self) {
        self.stop_io_pump();
    }
}

/// Only the physical route changes. Reliable delivery, endpoint lookup and
/// all game bytes retain their existing owners above this authenticated layer.
struct RelayRoute {
    address: SocketAddrV4,
    session: ClientSession,
    renewing: Option<ClientSession>,
    encoded: Vec<u8>,
    last_probe: Option<u32>,
    last_response: Option<u32>,
    peer_probes: Option<Vec<PeerProbe>>,
    ordering_certificate: Option<Vec<u8>>,
    // Dropped excess packets are recovered from the immutable stream cursor.
    // This bounds worker memory even while the UI/simulation thread is paused.
    ordering_inbox: VecDeque<vera20k_relay_protocol::OrderEvent>,
    retiring: Option<RelayRetirement>,
}

struct RelayRetirement {
    last_sent: Option<u32>,
}

struct PeerProbe {
    source: u8,
    endpoint: SocketAddrV4,
    sequence: u32,
    elapsed: Option<u32>,
}

impl std::fmt::Debug for RelayRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RelayRoute")
            .field("address", &self.address)
            .field("session", &self.session)
            .finish_non_exhaustive()
    }
}

impl UdpIo {
    /// Explicit bind addresses allow independent clients/tests to select
    /// interfaces or ephemeral ports. A failure drops any socket already
    /// opened by this call; no partially open transport escapes.
    fn bind(config: UdpBindConfig, connections: LanConnections) -> io::Result<Self> {
        let primary = UdpSocket::bind(config.primary)?;
        primary.set_nonblocking(true)?;
        let alternate = UdpSocket::bind(config.alternate)?;
        alternate.set_nonblocking(true)?;
        let global = GlobalChannel::new(
            GlobalConfig {
                product_id: connections.product_id(),
                ..GlobalConfig::default()
            },
            connections.retry_timing(),
        );
        Ok(Self {
            primary,
            alternate,
            primary_send: UdpSendQueue::default(),
            alternate_send: UdpSendQueue::default(),
            connections,
            global,
            broadcast_targets: Vec::new(),
            relay: None,
            lan_scope: None,
            pending_report: UdpServiceReport::default(),
        })
    }

    /// Bind the accepted GO/Type13 admission before loading. Bootstrap sends
    /// remain plain, while an already-active peer's exact-scoped global loading
    /// data/ACKs can be received. The application owns the scope and validates
    /// it inside loading progress too; no private traffic is admitted yet.
    fn prepare_lan_scope(&mut self, scope: SessionScope) -> io::Result<()> {
        if self.relay.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "relay and LAN match scope are mutually exclusive",
            ));
        }
        if scope.epoch == [0; 16] || scope.revision == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid LAN match scope",
            ));
        }
        if let Some(current) = self.lan_scope {
            if current.scope() == scope {
                return Ok(());
            }
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "LAN match scope must be retired before replacement",
            ));
        }
        self.lan_scope = Some(LanScopeState::Prepared(scope));
        Ok(())
    }

    /// Promote the admitted route at install_gameplay before installing fresh
    /// private channels. Exact-scoped global loading may cross this boundary;
    /// plain private packets and ACKs never do. Existing raw attempts retain
    /// their original encoding, and reliable owners retain retry/sequence state.
    /// The scope is public session identity, not authentication or a secret.
    fn set_lan_scope(&mut self, scope: SessionScope) -> io::Result<()> {
        self.prepare_lan_scope(scope)?;
        self.lan_scope = Some(LanScopeState::Active(scope));
        Ok(())
    }

    fn lan_scope(&self) -> Option<SessionScope> {
        self.lan_scope.map(LanScopeState::scope)
    }

    /// Returning to discovery must not reframe a previous match's queued
    /// messages under an unscoped lobby or a later match. Reuse the same
    /// manager reset540A80/540B40; no second reliability history is introduced.
    fn clear_lan_scope(&mut self) {
        if self.lan_scope.is_some() {
            self.reset_session();
        }
    }

    /// Configure a ticket granted over authenticated HTTPS. The ticket and
    /// its per-client key are deliberately absent from Debug and diagnostics.
    /// `relay_ready` changes only after a real UDP challenge/response round trip.
    fn enable_relay(
        &mut self,
        address: SocketAddrV4,
        ticket: &[u8],
        packet_key: [u8; 32],
        payload_key: [u8; 32],
    ) -> io::Result<()> {
        if self
            .relay
            .as_ref()
            .is_some_and(|route| route.retiring.is_some())
        {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Waiting to confirm departure from the previous online match.",
            ));
        }
        if self.lan_scope.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "relay and LAN match scope are mutually exclusive",
            ));
        }
        if address.port() == 0
            || address.ip().is_unspecified()
            || address.ip().is_broadcast()
            || address.ip().is_multicast()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid relay address",
            ));
        }
        let session =
            ClientSession::new(ticket, packet_key, payload_key).map_err(relay_io_error)?;
        self.relay = Some(RelayRoute {
            address,
            session,
            renewing: None,
            encoded: Vec::with_capacity(vera20k_relay_protocol::MAX_DATAGRAM),
            last_probe: None,
            last_response: None,
            peer_probes: None,
            ordering_certificate: None,
            ordering_inbox: VecDeque::new(),
            retiring: None,
        });
        self.global.require_authenticated_acknowledgments();
        self.broadcast_targets = vec![SocketAddrV4::new(Ipv4Addr::BROADCAST, DEFAULT_LAN_PORT)];
        Ok(())
    }

    fn relay_ready(&self) -> bool {
        self.relay
            .as_ref()
            .is_some_and(|route| route.retiring.is_none() && route.session.is_ready())
    }

    fn relay_source(&self) -> Option<u8> {
        self.relay.as_ref().map(|route| route.session.source())
    }

    fn relay_expires(&self) -> Option<u64> {
        self.relay.as_ref().map(|route| route.session.expires())
    }

    /// Rotate only the outer credential. Existing private/global queues,
    /// packet sequence numbers, sockets and virtual endpoints remain intact.
    fn renew_relay(
        &mut self,
        address: SocketAddrV4,
        ticket: &[u8],
        key: [u8; 32],
        payload_key: [u8; 32],
    ) -> io::Result<()> {
        let route = self
            .relay
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "no relay route"))?;
        if route.retiring.is_some() {
            // The cached terminal capability remains valid after hop expiry.
            // A late HTTPS renewal must not replace its ACK/nonce identity.
            return Ok(());
        }
        if address != route.address {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "relay renewal changed the route",
            ));
        }
        if route.session.same_lease(ticket)
            || route
                .renewing
                .as_ref()
                .is_some_and(|session| session.same_lease(ticket))
        {
            return Ok(());
        }
        let current = route.renewing.as_ref().unwrap_or(&route.session);
        route.renewing = Some(
            current
                .renewal(ticket, key, payload_key)
                .map_err(relay_io_error)?,
        );
        route.last_probe = None;
        Ok(())
    }

    /// Retire game traffic immediately, retaining only the same route's
    /// explicit terminal request until its authenticated acknowledgement.
    fn retire_relay(&mut self) {
        self.reset_session();
        self.broadcast_targets.clear();
        if let Some(route) = self.relay.as_mut()
            && route.session.ordering_is_configured()
        {
            route.renewing = None;
            route.peer_probes = None;
            route.ordering_inbox.clear();
            route
                .retiring
                .get_or_insert(RelayRetirement { last_sent: None });
        } else {
            self.relay = None;
        }
    }

    /// Measure the complete peer path using the existing global reliable
    /// send/ACK queue. Call after every member has completed relay admission.
    /// This probe has no second retry timer and no application command payload.
    fn begin_relay_peer_probes(&mut self, sources: &[u8]) -> io::Result<()> {
        let route = self
            .relay
            .as_mut()
            .filter(|route| route.session.is_ready())
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "relay is not admitted"))?;
        if route.peer_probes.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "peer probes already started",
            ));
        }
        if sources.len() >= usize::from(RELAY_MAX_PEERS)
            || sources.iter().enumerate().any(|(index, source)| {
                *source == route.session.source()
                    || *source >= RELAY_MAX_PEERS
                    || sources[..index].contains(source)
            })
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid peer probe roster",
            ));
        }
        if self.global.available_send_slots() < sources.len() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "global send queue is full",
            ));
        }
        let mut probes = Vec::with_capacity(sources.len());
        for &source in sources {
            let endpoint = relay_peer_endpoint(source).map_err(relay_io_error)?;
            let sequence = self.global.next_send_sequence();
            let accepted = self.global.send(
                &[],
                GlobalDestination::direct(endpoint),
                super::reliable::Delivery::Reliable,
                0,
            );
            assert!(accepted, "reserved global probe slots must fit");
            probes.push(PeerProbe {
                source,
                endpoint,
                sequence,
                elapsed: None,
            });
        }
        route.peer_probes = Some(probes);
        Ok(())
    }

    /// One actual round-trip measurement per remote canonical source, in ms.
    /// A zero is a measured sub-bucket RTT, never an absent observation.
    fn relay_peer_round_trips(&self) -> Option<Vec<(u8, u32)>> {
        self.relay
            .as_ref()?
            .peer_probes
            .as_ref()?
            .iter()
            .map(|probe| Some((probe.source, probe.elapsed?.saturating_mul(16))))
            .collect()
    }

    fn primary_addr(&self) -> io::Result<SocketAddrV4> {
        ipv4_addr(&self.primary)
    }

    fn alternate_addr(&self) -> io::Result<SocketAddrV4> {
        ipv4_addr(&self.alternate)
    }

    /// Timing is manager state; constructing private peers after GO inherits
    /// the active settings instead of reinstating lobby defaults.
    fn retry_timing(&self) -> super::reliable::RetryTiming {
        self.connections.retry_timing()
    }

    /// Interface7B1D10 clears both raw send FIFOs. This does not reset the
    ///independent reliable/global owners or discard their pending messages.
    fn clear_pending_sends(&mut self) {
        self.primary_send.pending.clear();
        self.alternate_send.pending.clear();
    }

    /// Manager initialization540A80/540B40 begins a fresh outer lobby visit:
    /// discard private/global traffic and raw attempts, retaining this portable
    /// socket pair and its configured discovery targets.
    fn reset_session(&mut self) {
        self.clear_pending_sends();
        self.connections.reset();
        self.global.reset();
        self.lan_scope = None;
        self.pending_report = UdpServiceReport::default();
    }

    /// UDPInterface's target list7B2FF0/7B2F90. The caller explicitly chooses
    /// IPv4 LAN discovery targets; retail LAN's IPX broadcast is excluded.
    /// Preserve insertion order while avoiding duplicate wire broadcasts.
    fn set_broadcast_targets(&mut self, targets: &[SocketAddrV4]) -> io::Result<()> {
        self.primary.set_broadcast(!targets.is_empty())?;
        self.broadcast_targets.clear();
        for target in targets {
            if !self.broadcast_targets.contains(target) {
                self.broadcast_targets.push(*target);
            }
        }
        Ok(())
    }

    /// Manager542450 takes an unsigned maximum over private RTTs and its
    /// optional +50 channel; it never reads the primary +48 global channel.
    /// The optional channel is not represented by this direct-LAN transport.
    fn response_time(&self) -> u32 {
        self.connections
            .peers()
            .filter_map(|(id, _)| self.connections.mean_response_time(id))
            .fold(0, u32::max)
    }

    /// IPXManager::ResetResponseTime542EC0 forwards the caller's flag to
    /// private peers and always clears the primary-global history. Socket,
    /// raw datagram and reliable delivery ownership are unaffected.
    fn reset_response_time(&mut self, clear: bool) {
        self.connections.reset_response_time(clear);
        self.global.reset_response_time(true);
    }

    fn set_retry_timing(&mut self, timing: super::reliable::RetryTiming, apply_to_global: bool) {
        self.connections.set_timing(timing);
        if apply_to_global {
            self.global.set_timing(timing);
        }
    }

    ///541820 drains received datagrams before servicing private connections.
    /// Ordinary private/global ACKs use primary UDP7B3C5B even when received on
    /// the alternate socket.48C2CB/53FEC3 select alternate only for manager+4C,
    /// the separate CDEF router connection; that connection is not owned here.
    /// Ordinary queued sends also use primary UDP7B3C5B. The connection
    /// owner alone decides retries, sequence order and peer failure identity.
    fn service(&mut self, now_ticks: u32, receive_budget: usize) -> UdpServiceReport {
        let mut report = UdpServiceReport::default();
        if let Some(route) = self.relay.as_mut() {
            if let Some(retiring) = route.retiring.as_mut() {
                if retiring
                    .last_sent
                    .is_none_or(|last| now_ticks.wrapping_sub(last) >= 16)
                {
                    retiring.last_sent = Some(now_ticks);
                    if let Err(error) = route
                        .session
                        .leave_ordering(&mut route.encoded)
                        .map_err(relay_io_error)
                        .and_then(|()| {
                            self.primary
                                .send_to(&route.encoded, route.address)
                                .map(|_| ())
                        })
                    {
                        record_io_error(
                            &mut report,
                            UdpChannel::Primary,
                            UdpIoOperation::Send,
                            error,
                        );
                    }
                }
            } else {
                if route
                    .last_response
                    .is_some_and(|last| now_ticks.wrapping_sub(last) > 625)
                {
                    route.session.forget_readiness();
                }
                // Keep the NAT mapping alive even without player input. During
                // initial admission retry once a second; established routes ping
                // every two seconds. These wall buckets never enter simulation.
                let interval = if route.session.is_ready() && route.renewing.is_none() {
                    125
                } else {
                    62
                };
                if route
                    .last_probe
                    .is_none_or(|last| now_ticks.wrapping_sub(last) >= interval)
                {
                    route.last_probe = Some(now_ticks);
                    for session in
                        std::iter::once(&mut route.session).chain(route.renewing.iter_mut())
                    {
                        match session.probe(&mut route.encoded) {
                            Ok(()) => {
                                if let Err(error) =
                                    self.primary.send_to(&route.encoded, route.address)
                                {
                                    record_io_error(
                                        &mut report,
                                        UdpChannel::Primary,
                                        UdpIoOperation::Send,
                                        error,
                                    );
                                }
                            }
                            Err(error) => record_io_error(
                                &mut report,
                                UdpChannel::Primary,
                                UdpIoOperation::Send,
                                relay_io_error(error),
                            ),
                        }
                    }
                }
            }
        }
        //Portable polling replaces Windows FD_WRITE notification timing.
        //Give each retained FIFO one write opportunity per pass; each later
        //enqueue still produces its native single-head synchronous attempt.
        service_send_queue(
            &self.primary,
            &mut self.relay,
            UdpChannel::Primary,
            &mut self.primary_send,
            &mut report,
        );
        service_send_queue(
            &self.alternate,
            &mut self.relay,
            UdpChannel::Alternate,
            &mut self.alternate_send,
            &mut report,
        );
        drain_socket(
            &self.primary,
            &self.primary,
            &mut self.relay,
            &mut self.primary_send,
            self.lan_scope,
            UdpChannel::Primary,
            &mut self.connections,
            &mut self.global,
            &self.broadcast_targets,
            now_ticks,
            receive_budget,
            &mut report,
        );
        drain_socket(
            &self.alternate,
            &self.primary,
            &mut self.relay,
            &mut self.primary_send,
            self.lan_scope,
            UdpChannel::Alternate,
            &mut self.connections,
            &mut self.global,
            &self.broadcast_targets,
            now_ticks,
            receive_budget,
            &mut report,
        );
        // Manager542197 services the global channel first, discarding failed
        // global sends through5402D0 while continuing every private channel.
        if !self.global.service(now_ticks, &mut |destination, frame| {
            send_global_frame(
                &self.primary,
                &mut self.relay,
                self.lan_scope.and_then(LanScopeState::outgoing),
                UdpChannel::Primary,
                &mut self.primary_send,
                destination,
                &self.broadcast_targets,
                frame,
                &mut report,
            );
        }) {
            report.discarded_global_sends = self.global.discard_failed_sends();
        }
        while let Some(response) = self.global.pop_response_time() {
            if let Some(probes) = self
                .relay
                .as_mut()
                .and_then(|route| route.peer_probes.as_mut())
            {
                if let Some(probe) = probes.iter_mut().find(|probe| {
                    response.destination == GlobalDestination::direct(probe.endpoint)
                        && response.sequence == probe.sequence
                }) {
                    probe.elapsed = Some(response.elapsed);
                }
            }
        }
        report.failed_peer = self.connections.service(now_ticks, &mut |endpoint, frame| {
            send_frame(
                &self.primary,
                &mut self.relay,
                self.lan_scope.and_then(LanScopeState::outgoing),
                UdpChannel::Primary,
                &mut self.primary_send,
                endpoint,
                frame,
                &mut report,
            );
        });
        report
    }

    fn pop_received(&mut self) -> Option<ReceivedMessage> {
        self.connections.pop_received()
    }

    fn pop_global_received(&mut self) -> Option<GlobalMessage> {
        self.global.pop_received()
    }
}

fn ipv4_addr(socket: &UdpSocket) -> io::Result<SocketAddrV4> {
    match socket.local_addr()? {
        SocketAddr::V4(address) => Ok(address),
        SocketAddr::V6(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "LAN socket must be IPv4",
        )),
    }
}

fn drain_socket(
    socket: &UdpSocket,
    ack_socket: &UdpSocket,
    relay: &mut Option<RelayRoute>,
    ack_queue: &mut UdpSendQueue,
    lan_scope: Option<LanScopeState>,
    channel: UdpChannel,
    connections: &mut LanConnections,
    global: &mut GlobalChannel,
    broadcast_targets: &[SocketAddrV4],
    now_ticks: u32,
    receive_budget: usize,
    report: &mut UdpServiceReport,
) {
    // One extra byte distinguishes a full valid native buffer from an
    // oversized/truncated datagram on platforms whose recvfrom truncates.
    let mut bytes = [0; vera20k_relay_protocol::MAX_DATAGRAM + 1];
    for _ in 0..receive_budget {
        let (length, source) = match socket.recv_from(&mut bytes) {
            Ok(packet) => packet,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if cfg!(windows) && error.raw_os_error() == Some(10040) => {
                // Winsock recvfrom consumes/truncates an oversized datagram
                // with WSAEMSGSIZE, unlike POSIX's successful truncated read.
                // Both use the same safe malformed-packet rejection here.
                // learn.microsoft.com/windows/win32/api/winsock/nf-winsock-recvfrom
                report.received_datagrams += 1;
                report.malformed_datagrams += 1;
                continue;
            }
            Err(error) => {
                record_io_error(report, channel, UdpIoOperation::Receive, error);
                break;
            }
        };
        report.received_datagrams += 1;
        let SocketAddr::V4(source) = source else {
            report.malformed_datagrams += 1;
            continue;
        };
        let decrypted;
        let (datagram, source, received_scope) = if let Some(route) = relay.as_mut() {
            if source != route.address || length > vera20k_relay_protocol::MAX_DATAGRAM {
                report.malformed_datagrams += 1;
                continue;
            }
            if route.retiring.is_some() {
                match route.session.receive(&bytes[..length]) {
                    Ok(ServerEvent::Ordering(vera20k_relay_protocol::OrderEvent::Left)) => {
                        *relay = None;
                    }
                    Ok(_) => {} // No late game traffic enters a returned match.
                    Err(_) => report.malformed_datagrams += 1,
                }
                continue;
            }
            if let Some(event) = route
                .renewing
                .as_mut()
                .and_then(|session| session.receive(&bytes[..length]).ok())
            {
                match event {
                    ServerEvent::Ready => {
                        let mut next = route.renewing.take().expect("admitted candidate");
                        if next.adopt_ordering_from(&mut route.session).is_err() {
                            report.malformed_datagrams += 1;
                            continue;
                        }
                        route.session = next;
                        route.last_response = Some(now_ticks);
                        continue;
                    }
                    ServerEvent::Challenge(cookie) => {
                        if let Err(error) = route
                            .renewing
                            .as_mut()
                            .expect("candidate")
                            .confirm(cookie, &mut route.encoded)
                            .map_err(relay_io_error)
                            .and_then(|()| {
                                ack_socket
                                    .send_to(&route.encoded, route.address)
                                    .map(|_| ())
                            })
                        {
                            record_io_error(report, channel, UdpIoOperation::Send, error);
                        }
                        continue;
                    }
                    ServerEvent::Payload { .. } | ServerEvent::Ordering(_) => {
                        unreachable!("candidate is promoted on READY")
                    }
                }
            }
            match route.session.receive(&bytes[..length]) {
                Ok(ServerEvent::Ordering(event)) => {
                    route.last_response = Some(now_ticks);
                    if route.ordering_inbox.len() < 256 {
                        route.ordering_inbox.push_back(event);
                    }
                    continue;
                }
                Ok(ServerEvent::Payload { source, bytes }) => {
                    route.last_response = Some(now_ticks);
                    decrypted = bytes;
                    (
                        decrypted.as_bytes(),
                        relay_peer_endpoint(source).expect("authenticated source range"),
                        None,
                    )
                }
                Ok(ServerEvent::Ready) => {
                    route.last_response = Some(now_ticks);
                    continue;
                }
                Ok(ServerEvent::Challenge(cookie)) => {
                    match route.session.confirm(cookie, &mut route.encoded) {
                        Ok(()) => {
                            if let Err(error) = ack_socket.send_to(&route.encoded, route.address) {
                                record_io_error(report, channel, UdpIoOperation::Send, error);
                            }
                        }
                        Err(error) => record_io_error(
                            report,
                            channel,
                            UdpIoOperation::Send,
                            relay_io_error(error),
                        ),
                    }
                    continue;
                }
                Err(_) => {
                    report.malformed_datagrams += 1;
                    continue;
                }
            }
        } else {
            let datagram = &bytes[..length];
            if datagram.starts_with(LAN_SCOPE_MAGIC) {
                let Some(scope) = lan_scope.map(LanScopeState::scope) else {
                    // Retired match bytes never enter plain lobby receive state.
                    report.malformed_datagrams += 1;
                    continue;
                };
                if datagram.len() < LAN_SCOPE_HEADER_LEN + CHECKSUM_LEN
                    || datagram.len() > UDP_DATAGRAM_CAPACITY
                    || datagram[4..20] != scope.epoch
                    || datagram[20..LAN_SCOPE_HEADER_LEN] != scope.revision.to_le_bytes()
                {
                    report.malformed_datagrams += 1;
                    continue;
                }
                (&datagram[LAN_SCOPE_HEADER_LEN..], source, Some(scope))
            } else {
                (datagram, source, None)
            }
        };
        if datagram.len() < CHECKSUM_LEN || datagram.len() > UDP_DATAGRAM_CAPACITY {
            report.malformed_datagrams += 1;
            continue;
        }
        let checksum = u32::from_le_bytes(datagram[..CHECKSUM_LEN].try_into().unwrap());
        let frame = &datagram[CHECKSUM_LEN..];
        //7B3F82..7B3FB9 validates this before publishing to the manager.
        if checksum != udp_checksum(frame) {
            report.malformed_datagrams += 1;
            continue;
        }
        //541820 demultiplexes the first word before private endpoint lookup;
        //otherwise discovery and Join from unknown sources never reach BB.
        let is_global = frame.get(..2).is_some_and(|bytes| {
            u16::from_le_bytes([bytes[0], bytes[1]]) == global.discriminator()
        });
        let admitted = match lan_scope {
            // No private connections belong to the new match until loading
            // installs gameplay. In particular a late old plain packet must
            // not initialize a fresh private receive sequence during loading.
            Some(LanScopeState::Prepared(_)) => is_global,
            Some(LanScopeState::Active(scope)) if received_scope.is_none() => {
                // A peer may still be loading after we received its100. Its
                // retry stays plain until it promotes the same route. Only
                // reliable progress carrying the exact application scope may
                // reach global dedup/ACK here; never plain ACKs or commands.
                is_global
                    && frame.get(2) == Some(&0)
                    && frame.get(GLOBAL_HEADER_LEN..).is_some_and(|payload| {
                        LoadingProgressMessage::decode_scoped(payload, scope).is_ok()
                    })
            }
            _ => true,
        };
        if !admitted {
            report.malformed_datagrams += 1;
            continue;
        }
        if is_global {
            //7B4038..408C stores a zero-port endpoint and the recvfrom port
            //separately;5418FB passes that override to53FCB0.
            let endpoint = SocketAddrV4::new(*source.ip(), 0);
            if let Err(error) = global.receive(
                endpoint,
                source.port(),
                frame,
                now_ticks,
                &mut |destination, ack| {
                    send_global_frame(
                        ack_socket,
                        relay,
                        received_scope,
                        UdpChannel::Primary,
                        ack_queue,
                        destination,
                        broadcast_targets,
                        ack,
                        report,
                    );
                },
            ) {
                report.rejected_global += 1;
                report.last_global_rejection = Some(error);
            }
        } else if let Err(error) = connections.receive(source, frame, &mut |endpoint, ack| {
            send_frame(
                ack_socket,
                relay,
                received_scope,
                UdpChannel::Primary,
                ack_queue,
                endpoint,
                ack,
                report,
            );
        }) {
            report.rejected_connections += 1;
            report.last_rejection = Some(error);
        }
    }
}

fn send_global_frame(
    socket: &UdpSocket,
    relay: &mut Option<RelayRoute>,
    lan_scope: Option<SessionScope>,
    channel: UdpChannel,
    queue: &mut UdpSendQueue,
    destination: GlobalDestination,
    targets: &[SocketAddrV4],
    frame: &[u8],
    report: &mut UdpServiceReport,
) {
    match destination {
        GlobalDestination::Direct {
            mut endpoint,
            port_override,
        } => {
            if port_override != 0 {
                endpoint.set_port(port_override);
            }
            send_frame(
                socket, relay, lan_scope, channel, queue, endpoint, frame, report,
            );
        }
        GlobalDestination::Broadcast { port_override } => {
            for &target in targets {
                let mut endpoint = target;
                if port_override != 0 {
                    endpoint.set_port(port_override);
                }
                send_frame(
                    socket, relay, lan_scope, channel, queue, endpoint, frame, report,
                );
            }
        }
    }
}

fn send_frame(
    socket: &UdpSocket,
    relay: &mut Option<RelayRoute>,
    lan_scope: Option<SessionScope>,
    channel: UdpChannel,
    queue: &mut UdpSendQueue,
    endpoint: SocketAddrV4,
    frame: &[u8],
    report: &mut UdpServiceReport,
) {
    queue.send(
        endpoint,
        frame,
        lan_scope,
        channel,
        report,
        &mut |bytes, endpoint| send_datagram(socket, relay, bytes, endpoint),
    );
}

fn service_send_queue(
    socket: &UdpSocket,
    relay: &mut Option<RelayRoute>,
    channel: UdpChannel,
    queue: &mut UdpSendQueue,
    report: &mut UdpServiceReport,
) {
    queue.service(channel, report, &mut |bytes, endpoint| {
        send_datagram(socket, relay, bytes, endpoint)
    });
}

fn send_datagram(
    socket: &UdpSocket,
    relay: &mut Option<RelayRoute>,
    bytes: &[u8],
    endpoint: SocketAddrV4,
) -> io::Result<usize> {
    let result = if let Some(route) = relay.as_mut() {
        if route.retiring.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Waiting to confirm departure from the previous online match.",
            ));
        }
        let target = if endpoint.ip().is_broadcast() {
            vera20k_relay_protocol::BROADCAST
        } else {
            vera20k_relay_protocol::endpoint_peer(endpoint).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "relay destination is not an admitted peer endpoint",
                )
            })?
        };
        route
            .session
            .data(target, bytes, &mut route.encoded)
            .map_err(relay_io_error)?;
        socket.send_to(&route.encoded, route.address)
    } else {
        socket.send_to(bytes, endpoint)
    };
    if let Err(error) = &result
        && error.kind() != io::ErrorKind::WouldBlock
    {
        //7B3DA4 ->7B25E0 clears SO_ERROR on the selected send socket. Keep
        //the original send error, independently of whether clearing succeeds.
        let _ = socket.take_error();
    }
    result
}

fn relay_io_error(error: RelayError) -> io::Error {
    let kind = if error == RelayError::Unregistered {
        io::ErrorKind::WouldBlock
    } else {
        io::ErrorKind::InvalidData
    };
    io::Error::new(kind, error)
}

fn record_io_error(
    report: &mut UdpServiceReport,
    channel: UdpChannel,
    operation: UdpIoOperation,
    error: io::Error,
) {
    if report.first_io_error.is_none() {
        report.first_io_error = Some(UdpIoFailure {
            channel,
            operation,
            error,
        });
    }
}

#[cfg(test)]
#[path = "udp_tests.rs"]
mod tests;

#[cfg(test)]
mod close_tests {
    use super::*;
    use crate::net::lan_messages::{LanMessage, LanMessageKind, LanNamedMessage};
    use crate::net::reliable::{Delivery, LanConnectionConfig, RetryTiming};

    fn paused_bucket() -> u32 {
        0
    }

    fn closing_bucket() -> u32 {
        32
    }

    fn queued_departure() -> (LanUdpTransport, UdpSocket, UdpBindConfig, Vec<u8>) {
        let sink = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        sink.set_nonblocking(true).unwrap();
        let mut wire = LanUdpTransport::bind(
            UdpBindConfig {
                primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            },
            LanConnections::new(
                LanConnectionConfig::default(),
                RetryTiming {
                    retry_delta: 10,
                    max_retries: u32::MAX,
                    timeout: u32::MAX,
                },
            ),
        )
        .unwrap();
        let binds = UdpBindConfig {
            primary: wire.primary_addr().unwrap(),
            alternate: wire.alternate_addr().unwrap(),
        };
        wire.set_broadcast_targets(&[ipv4_addr(&sink).unwrap()])
            .unwrap();
        wire.start_io_pump(paused_bucket).unwrap();
        let payload = LanMessage::named(LanNamedMessage::Departure, "ClosingPeer")
            .unwrap()
            .as_bytes()
            .to_vec();
        assert!(wire.global_mut().send(
            &payload,
            GlobalDestination::BROADCAST,
            Delivery::Unreliable,
            0,
        ));
        // No worker scheduling interleaving can cross the retry gate at0.
        // The ordinary App service argument cannot override the worker clock.
        assert_eq!(wire.service(32).sent_datagrams, 0);
        assert_eq!(wire.global().queued_send_count(), 1);
        assert_eq!(
            sink.recv_from(&mut [0; UDP_DATAGRAM_CAPACITY])
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
        (wire, sink, binds, payload)
    }

    #[test]
    fn final_service_attempts_departure_before_drop_from_the_original_socket() {
        let (mut wire, sink, binds, payload) = queued_departure();
        let timing = wire.retry_timing();
        let report = wire.service_before_close(closing_bucket);
        assert_eq!(report.sent_datagrams, 1);
        assert!(report.first_io_error.is_none());
        assert!(wire.pump.is_none());

        sink.set_nonblocking(false).unwrap();
        sink.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut datagram = [0; UDP_DATAGRAM_CAPACITY];
        let (length, source) = sink.recv_from(&mut datagram).unwrap();
        assert_eq!(source, SocketAddr::V4(binds.primary));
        let frame = &datagram[CHECKSUM_LEN..length];
        assert_eq!(
            u32::from_le_bytes(datagram[..CHECKSUM_LEN].try_into().unwrap()),
            udp_checksum(frame)
        );
        let mut decoded = GlobalChannel::new(GlobalConfig::default(), timing);
        decoded
            .receive(binds.primary, 0, frame, 32, &mut |_, _| {
                panic!("unreliable Departure must not request an ACK")
            })
            .unwrap();
        let received = decoded.pop_received().unwrap();
        assert_eq!(received.payload, payload);
        let departure = LanMessage::decode(&received.payload).unwrap();
        assert_eq!(departure.kind(), LanMessageKind::Departure);
        assert_eq!(departure.name(), "ClosingPeer");

        drop(wire);
        let _primary = UdpSocket::bind(binds.primary).unwrap();
        let _alternate = UdpSocket::bind(binds.alternate).unwrap();
        sink.set_nonblocking(true).unwrap();
        assert_eq!(
            sink.recv_from(&mut datagram).unwrap_err().kind(),
            io::ErrorKind::WouldBlock,
            "terminal Drop must not make another attempt"
        );
    }

    #[test]
    fn ordinary_drop_discards_departure_and_releases_both_sockets() {
        let (wire, sink, binds, _) = queued_departure();
        drop(wire);
        let _primary = UdpSocket::bind(binds.primary).unwrap();
        let _alternate = UdpSocket::bind(binds.alternate).unwrap();
        assert_eq!(
            sink.recv_from(&mut [0; UDP_DATAGRAM_CAPACITY])
                .unwrap_err()
                .kind(),
            io::ErrorKind::WouldBlock
        );
    }
}
