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

use super::global::{
    GlobalChannel, GlobalConfig, GlobalDestination, GlobalMessage, GlobalReceiveError,
};
use super::reliable::{LanConnections, ReceiveError, ReceivedMessage, udp_checksum};

/// Original UDP port data at841F30/841F38. The alternate socket is a separate
/// receive path; it does not change the default destination port.
pub const DEFAULT_LAN_PORT: u16 = 1234;
pub const DEFAULT_ALTERNATE_PORT: u16 = 1236;
/// Original recvfrom7B3EDD supplies a640-byte datagram buffer.
pub const UDP_DATAGRAM_CAPACITY: usize = 640;
const CHECKSUM_LEN: usize = 4;

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
        channel: UdpChannel,
        report: &mut UdpServiceReport,
        send: &mut impl FnMut(&[u8], SocketAddrV4) -> io::Result<usize>,
    ) {
        if frame.len() > UDP_DATAGRAM_CAPACITY - CHECKSUM_LEN {
            record_io_error(
                report,
                channel,
                UdpIoOperation::Send,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "connection frame exceeds native UDP buffer",
                ),
            );
            return;
        }
        //7B3C5B..7B3C75 substitutes1234 only for a zero destination port.
        if channel == UdpChannel::Primary && endpoint.port() == 0 {
            endpoint.set_port(DEFAULT_LAN_PORT);
        }
        let mut bytes = [0; UDP_DATAGRAM_CAPACITY];
        bytes[..CHECKSUM_LEN].copy_from_slice(&udp_checksum(frame).to_le_bytes());
        bytes[CHECKSUM_LEN..CHECKSUM_LEN + frame.len()].copy_from_slice(frame);
        self.pending.push_back(PendingDatagram {
            endpoint,
            bytes,
            length: CHECKSUM_LEN + frame.len(),
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

/// Owns both socket lifetimes and exactly one reliable connection owner.
/// Dropping this value closes both sockets and all of those connections.
#[derive(Debug)]
pub struct LanUdpTransport {
    primary: UdpSocket,
    alternate: UdpSocket,
    primary_send: UdpSendQueue,
    alternate_send: UdpSendQueue,
    connections: LanConnections,
    global: GlobalChannel,
    broadcast_targets: Vec<SocketAddrV4>,
}

impl LanUdpTransport {
    /// Explicit bind addresses allow independent clients/tests to select
    /// interfaces or ephemeral ports. A failure drops any socket already
    /// opened by this call; no partially open transport escapes.
    pub fn bind(config: UdpBindConfig, connections: LanConnections) -> io::Result<Self> {
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
        })
    }

    pub fn primary_addr(&self) -> io::Result<SocketAddrV4> {
        ipv4_addr(&self.primary)
    }

    pub fn alternate_addr(&self) -> io::Result<SocketAddrV4> {
        ipv4_addr(&self.alternate)
    }

    pub fn connections(&self) -> &LanConnections {
        &self.connections
    }

    pub fn connections_mut(&mut self) -> &mut LanConnections {
        &mut self.connections
    }

    /// Timing is manager state; constructing private peers after GO inherits
    /// the active settings instead of reinstating lobby defaults.
    pub fn retry_timing(&self) -> super::reliable::RetryTiming {
        self.connections.retry_timing()
    }

    pub fn global(&self) -> &GlobalChannel {
        &self.global
    }

    pub fn global_mut(&mut self) -> &mut GlobalChannel {
        &mut self.global
    }

    /// Interface7B1D10 clears both raw send FIFOs. This does not reset the
    ///independent reliable/global owners or discard their pending messages.
    pub fn clear_pending_sends(&mut self) {
        self.primary_send.pending.clear();
        self.alternate_send.pending.clear();
    }

    /// Manager initialization540A80/540B40 begins a fresh outer lobby visit:
    /// discard private/global traffic and raw attempts, retaining this portable
    /// socket pair and its configured discovery targets.
    pub fn reset_session(&mut self) {
        self.clear_pending_sends();
        self.connections.reset();
        self.global.reset();
    }

    /// UDPInterface's target list7B2FF0/7B2F90. The caller explicitly chooses
    /// IPv4 LAN discovery targets; retail LAN's IPX broadcast is excluded.
    /// Preserve insertion order while avoiding duplicate wire broadcasts.
    pub fn set_broadcast_targets(&mut self, targets: &[SocketAddrV4]) -> io::Result<()> {
        self.primary.set_broadcast(!targets.is_empty())?;
        self.broadcast_targets.clear();
        for target in targets {
            if !self.broadcast_targets.contains(target) {
                self.broadcast_targets.push(*target);
            }
        }
        Ok(())
    }

    /// Manager542450 takes an unsigned maximum over private and global RTTs.
    pub fn response_time(&self) -> u32 {
        self.connections
            .peers()
            .filter_map(|(id, _)| self.connections.mean_response_time(id))
            .fold(self.global.mean_response_time(), u32::max)
    }

    pub fn set_retry_timing(
        &mut self,
        timing: super::reliable::RetryTiming,
        apply_to_global: bool,
    ) {
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
    pub fn service(&mut self, now_ticks: u32) -> UdpServiceReport {
        let mut report = UdpServiceReport::default();
        //Portable polling replaces Windows FD_WRITE notification timing.
        //Give each retained FIFO one write opportunity per pass; each later
        //enqueue still produces its native single-head synchronous attempt.
        service_send_queue(
            &self.primary,
            UdpChannel::Primary,
            &mut self.primary_send,
            &mut report,
        );
        service_send_queue(
            &self.alternate,
            UdpChannel::Alternate,
            &mut self.alternate_send,
            &mut report,
        );
        drain_socket(
            &self.primary,
            &self.primary,
            &mut self.primary_send,
            UdpChannel::Primary,
            &mut self.connections,
            &mut self.global,
            &self.broadcast_targets,
            now_ticks,
            &mut report,
        );
        drain_socket(
            &self.alternate,
            &self.primary,
            &mut self.primary_send,
            UdpChannel::Alternate,
            &mut self.connections,
            &mut self.global,
            &self.broadcast_targets,
            now_ticks,
            &mut report,
        );
        // Manager542197 services the global channel first, discarding failed
        // global sends through5402D0 while continuing every private channel.
        if !self.global.service(now_ticks, &mut |destination, frame| {
            send_global_frame(
                &self.primary,
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
        report.failed_peer = self.connections.service(now_ticks, &mut |endpoint, frame| {
            send_frame(
                &self.primary,
                UdpChannel::Primary,
                &mut self.primary_send,
                endpoint,
                frame,
                &mut report,
            );
        });
        report
    }

    pub fn pop_received(&mut self) -> Option<ReceivedMessage> {
        self.connections.pop_received()
    }

    pub fn pop_global_received(&mut self) -> Option<GlobalMessage> {
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
    ack_queue: &mut UdpSendQueue,
    channel: UdpChannel,
    connections: &mut LanConnections,
    global: &mut GlobalChannel,
    broadcast_targets: &[SocketAddrV4],
    now_ticks: u32,
    report: &mut UdpServiceReport,
) {
    // One extra byte distinguishes a full valid native buffer from an
    // oversized/truncated datagram on platforms whose recvfrom truncates.
    let mut bytes = [0; UDP_DATAGRAM_CAPACITY + 1];
    loop {
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
        if length < CHECKSUM_LEN || length > UDP_DATAGRAM_CAPACITY {
            report.malformed_datagrams += 1;
            continue;
        }
        let SocketAddr::V4(source) = source else {
            report.malformed_datagrams += 1;
            continue;
        };
        let checksum = u32::from_le_bytes(bytes[..CHECKSUM_LEN].try_into().unwrap());
        let frame = &bytes[CHECKSUM_LEN..length];
        //7B3F82..7B3FB9 validates this before publishing to the manager.
        if checksum != udp_checksum(frame) {
            report.malformed_datagrams += 1;
            continue;
        }
        //541820 demultiplexes the first word before private endpoint lookup;
        //otherwise discovery and Join from unknown sources never reach BB.
        if frame
            .get(..2)
            .is_some_and(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]) == global.discriminator())
        {
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
            send_frame(socket, channel, queue, endpoint, frame, report);
        }
        GlobalDestination::Broadcast { port_override } => {
            for &target in targets {
                let mut endpoint = target;
                if port_override != 0 {
                    endpoint.set_port(port_override);
                }
                send_frame(socket, channel, queue, endpoint, frame, report);
            }
        }
    }
}

fn send_frame(
    socket: &UdpSocket,
    channel: UdpChannel,
    queue: &mut UdpSendQueue,
    endpoint: SocketAddrV4,
    frame: &[u8],
    report: &mut UdpServiceReport,
) {
    queue.send(endpoint, frame, channel, report, &mut |bytes, endpoint| {
        send_datagram(socket, bytes, endpoint)
    });
}

fn service_send_queue(
    socket: &UdpSocket,
    channel: UdpChannel,
    queue: &mut UdpSendQueue,
    report: &mut UdpServiceReport,
) {
    queue.service(channel, report, &mut |bytes, endpoint| {
        send_datagram(socket, bytes, endpoint)
    });
}

fn send_datagram(socket: &UdpSocket, bytes: &[u8], endpoint: SocketAddrV4) -> io::Result<usize> {
    let result = socket.send_to(bytes, endpoint);
    if let Err(error) = &result
        && error.kind() != io::ErrorKind::WouldBlock
    {
        //7B3DA4 ->7B25E0 clears SO_ERROR on the selected send socket. Keep
        //the original send error, independently of whether clearing succeeds.
        let _ = socket.take_error();
    }
    result
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
