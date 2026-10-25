//! External LAN fault tool, compiled only for test-support. It forwards the
//! real two-client byte stream and uses the production codecs to damage one
//! ordinary FRAMEINFO checksum after the coordinator validates both prefixes.
//! It never changes a participant, simulation state, reliability or relay auth.

use std::io;
use std::net::{SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::event_packet::{
    EVENT_PACKET_HEADER_LEN, LIVE_EVENT_PREFIX_LEN, decode_frame_info, live_command_frontier,
};
use super::reliable::{
    CONNECTION_HEADER_LEN, Delivery, LanConnectionConfig, sequence, udp_checksum,
};
use super::udp::UDP_DATAGRAM_CAPACITY;
use crate::sim::command::{COMMAND_RECORD_LEN, CommandRecord};
use crate::sim::command_queue::FrameInfo;

const SCOPE_HEADER: usize = 28;
const TRANSPORT_CRC: usize = 4;
const CONNECTION_START: usize = SCOPE_HEADER + TRANSPORT_CRC;
const EVENT_START: usize = CONNECTION_START + CONNECTION_HEADER_LEN;
const HEADER_START: usize = EVENT_START + LIVE_EVENT_PREFIX_LEN;
const HEADER_ONLY_LEN: usize = HEADER_START + EVENT_PACKET_HEADER_LEN;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    HostToGuest,
    GuestToHost,
}
impl Direction {
    fn index(self) -> usize {
        match self {
            Self::HostToGuest => 0,
            Self::GuestToHost => 1,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Corruption {
    pub direction: Direction,
    pub scope: String,
    pub house: i8,
    pub event_frame: i32,
    pub reported_frame: i32,
    pub delay: u8,
    pub command_frontier: u64,
    pub old_checksum: u32,
    pub new_checksum: u32,
    pub private_sequence: u32,
    pub datagram_bytes: usize,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Notice {
    Ready {
        host_proxy: SocketAddrV4,
        guest_proxy: SocketAddrV4,
        host: SocketAddrV4,
        guest: SocketAddrV4,
    },
    Armed {
        scope: String,
        after_frame: i32,
    },
    Corrupted {
        #[serde(flatten)]
        packet: Corruption,
        source: SocketAddrV4,
        received_at: SocketAddrV4,
        forwarded_from: SocketAddrV4,
        destination: SocketAddrV4,
    },
    Stopped {
        corrupted_directions: usize,
        forwarded: u64,
        rejected_sources: u64,
    },
}
#[derive(Debug)]
pub enum Control {
    Arm,
    Stop,
}

struct View {
    scope: [u8; 24],
    info: FrameInfo,
    frontier: u64,
}
fn inspect(packet: &[u8]) -> Option<View> {
    if packet.len() != HEADER_ONLY_LEN || !packet.starts_with(b"VLS\x01") {
        return None;
    }
    let frame = &packet[CONNECTION_START..];
    if frame[..2] != LanConnectionConfig::default().product_id.to_le_bytes()
        || frame[2] != Delivery::Unreliable.wire_kind()
        || udp_checksum(frame)
            != u32::from_le_bytes(packet[SCOPE_HEADER..CONNECTION_START].try_into().ok()?)
    {
        return None;
    }
    let payload = &packet[EVENT_START..];
    let info = decode_frame_info(payload)?;
    if info.house_id < 0 || packet[4..20] == [0; 16] || packet[20..SCOPE_HEADER] == [0; 8] {
        return None;
    }
    Some(View {
        scope: packet[4..SCOPE_HEADER].try_into().ok()?,
        info,
        frontier: live_command_frontier(payload)?,
    })
}
fn scope_text(scope: &[u8; 24]) -> String {
    scope.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

pub struct PacketFault {
    after_frame: i32,
    observed: [Option<([u8; 24], i32)>; 2],
    armed_scope: Option<[u8; 24]>,
    corrupted: [bool; 2],
}
impl PacketFault {
    pub fn new(after_frame: i32) -> io::Result<Self> {
        if after_frame < 300 {
            return Err(invalid("fault requires a real shared 300-frame prefix"));
        }
        Ok(Self {
            after_frame,
            observed: [None, None],
            armed_scope: None,
            corrupted: [false; 2],
        })
    }
    pub fn arm(&mut self) -> io::Result<Notice> {
        if self.armed_scope.is_some() {
            return Err(invalid("fault is already armed"));
        }
        let [Some((left, left_frame)), Some((right, right_frame))] = self.observed else {
            return Err(invalid(
                "both scoped participant streams must be observed before arming",
            ));
        };
        if left != right || left_frame < self.after_frame || right_frame < self.after_frame {
            return Err(invalid("participant scope or prefix progress differs"));
        }
        self.armed_scope = Some(left);
        Ok(Notice::Armed {
            scope: scope_text(&left),
            after_frame: self.after_frame,
        })
    }
    pub fn packet(&mut self, direction: Direction, packet: &mut [u8]) -> Option<Corruption> {
        let view = inspect(packet)?;
        let index = direction.index();
        let reported = view
            .info
            .event_frame
            .wrapping_sub(i32::from(view.info.delay));
        let Some(scope) = self.armed_scope else {
            self.observed[index] = Some((view.scope, reported));
            return None;
        };
        if view.scope != scope || self.corrupted[index] || reported <= self.after_frame {
            return None;
        }
        let mut raw = [0; COMMAND_RECORD_LEN];
        raw[..EVENT_PACKET_HEADER_LEN].copy_from_slice(&packet[HEADER_START..HEADER_ONLY_LEN]);
        let mut record = CommandRecord::decode_exact(&raw).ok()?;
        let mut changed = view.info;
        changed.checksum ^= 1;
        changed.write_to(&mut record);
        packet[HEADER_START..HEADER_ONLY_LEN]
            .copy_from_slice(&record.as_bytes()[..EVENT_PACKET_HEADER_LEN]);
        let checksum = udp_checksum(&packet[CONNECTION_START..]);
        packet[SCOPE_HEADER..CONNECTION_START].copy_from_slice(&checksum.to_le_bytes());
        self.corrupted[index] = true;
        Some(Corruption {
            direction,
            scope: scope_text(&scope),
            house: changed.house_id,
            event_frame: changed.event_frame,
            reported_frame: reported,
            delay: changed.delay,
            command_frontier: view.frontier,
            old_checksum: view.info.checksum,
            new_checksum: changed.checksum,
            private_sequence: sequence(&packet[CONNECTION_START..]),
            datagram_bytes: packet.len(),
        })
    }
}

/// Two distinct admitted peer identities. Guest sends to HostP1, host sends
/// to GuestP2. Forwarding must leave the opposite socket so snapshot/ACK source
/// checks see the same distinct identities throughout setup and gameplay.
pub struct Config {
    pub host_proxy: SocketAddrV4,
    pub guest_proxy: SocketAddrV4,
    pub host: SocketAddrV4,
    pub guest: SocketAddrV4,
    pub timeout: Duration,
}
impl Config {
    fn validate(&self) -> io::Result<()> {
        let endpoints = [self.host_proxy, self.guest_proxy, self.host, self.guest];
        for (index, endpoint) in endpoints.iter().enumerate() {
            if endpoint.port() == 0
                || endpoint.ip().is_unspecified()
                || endpoint.ip().is_multicast()
                || endpoint.ip().is_broadcast()
                || endpoints[..index].contains(endpoint)
            {
                return Err(invalid(
                    "proxy needs four distinct explicit unicast endpoints",
                ));
            }
        }
        if self.timeout.is_zero() || self.timeout > Duration::from_secs(3600) {
            return Err(invalid("proxy lifetime is invalid"));
        }
        Ok(())
    }
}

pub fn run(
    config: Config,
    controls: Receiver<Control>,
    notify: impl FnMut(Notice) -> io::Result<()>,
) -> io::Result<()> {
    config.validate()?;
    let sockets = [
        UdpSocket::bind(config.host_proxy)?,
        UdpSocket::bind(config.guest_proxy)?,
    ];
    run_sockets(config, sockets, controls, notify)
}

fn run_sockets(
    config: Config,
    sockets: [UdpSocket; 2],
    controls: Receiver<Control>,
    mut notify: impl FnMut(Notice) -> io::Result<()>,
) -> io::Result<()> {
    for socket in &sockets {
        socket.set_nonblocking(true)?;
    }
    let deadline = Instant::now() + config.timeout;
    let mut fault = PacketFault::new(300)?;
    let mut buffer = [0; UDP_DATAGRAM_CAPACITY + 1];
    let (mut forwarded, mut rejected_sources) = (0, 0);
    notify(Notice::Ready {
        host_proxy: config.host_proxy,
        guest_proxy: config.guest_proxy,
        host: config.host,
        guest: config.guest,
    })?;
    loop {
        match controls.try_recv() {
            Ok(Control::Arm) => notify(fault.arm()?)?,
            Ok(Control::Stop) | Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "owned fault proxy expired",
            ));
        }
        let mut received_any = false;
        for incoming in 0..2 {
            let (expected, target, outgoing, direction, received_at, forwarded_from) =
                if incoming == 0 {
                    (
                        config.guest,
                        config.host,
                        1,
                        Direction::GuestToHost,
                        config.host_proxy,
                        config.guest_proxy,
                    )
                } else {
                    (
                        config.host,
                        config.guest,
                        0,
                        Direction::HostToGuest,
                        config.guest_proxy,
                        config.host_proxy,
                    )
                };
            // Bounded draining keeps controls/deadlines and the opposite
            // direction live without throttling map windows behind an idle peer.
            for _ in 0..64 {
                let (length, source) = match sockets[incoming].recv_from(&mut buffer) {
                    Ok(value) => value,
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
                    Err(error) => return Err(error),
                };
                received_any = true;
                if source != SocketAddr::V4(expected) {
                    rejected_sources += 1;
                    continue;
                }
                if length > UDP_DATAGRAM_CAPACITY {
                    continue;
                }
                let witness = fault.packet(direction, &mut buffer[..length]);
                if sockets[outgoing].send_to(&buffer[..length], target)? != length {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "proxy datagram was truncated",
                    ));
                }
                forwarded += 1;
                if let Some(packet) = witness {
                    // Emit the public route witness only after actual send.
                    notify(Notice::Corrupted {
                        packet,
                        source: expected,
                        received_at,
                        forwarded_from,
                        destination: target,
                    })?;
                }
            }
        }
        if !received_any {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    notify(Notice::Stopped {
        corrupted_directions: fault.corrupted.iter().filter(|done| **done).count(),
        forwarded,
        rejected_sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::event_packet::{EventPacketCodec, EventPacketHeader};
    use crate::net::lan_messages::SessionScope;
    use crate::net::reliable::{LanConnections, RetryTiming};
    use crate::net::udp::{LanUdpTransport, UdpBindConfig};
    use crate::sim::command_queue::MultiplayerChecksumHistory;
    use std::net::Ipv4Addr;

    fn transport(peer: SocketAddrV4, scope: SessionScope) -> LanUdpTransport {
        let mut connections = LanConnections::new(
            LanConnectionConfig::default(),
            RetryTiming {
                retry_delta: 0,
                max_retries: 10,
                timeout: 100,
            },
        );
        connections.add_peer(2, peer).unwrap();
        let endpoint = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
        let mut transport = LanUdpTransport::bind(
            UdpBindConfig {
                primary: endpoint,
                alternate: endpoint,
            },
            connections,
        )
        .unwrap();
        transport.set_lan_scope(scope).unwrap();
        transport
    }
    fn real_packet(proxy: &UdpSocket, scope: SessionScope) -> (LanUdpTransport, Vec<u8>) {
        let proxy_address = match proxy.local_addr().unwrap() {
            SocketAddr::V4(value) => value,
            _ => unreachable!(),
        };
        let mut sender = transport(proxy_address, scope);
        let header = FrameInfo {
            house_id: 2,
            event_frame: 345,
            checksum: 0x1357_2468,
            timing_word: 0,
            delay: 15,
        };
        let packet = EventPacketCodec::default()
            .encode_packet(
                &[],
                EventPacketHeader::Vera {
                    frame_info: header,
                    command_frontier: 0,
                },
                330,
                558,
            )
            .unwrap()
            .bytes;
        assert!(
            sender
                .connections_mut()
                .send_to(2, &packet, Delivery::Unreliable)
        );
        sender.service(1);
        let mut raw = [0; UDP_DATAGRAM_CAPACITY + 1];
        let (length, _) = proxy.recv_from(&mut raw).unwrap();
        (sender, raw[..length].to_vec())
    }
    #[test]
    fn external_checksum_fault_passes_real_udp_ingress_then_fails_the_shared_comparator() {
        let proxy = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        proxy
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let scope = SessionScope {
            epoch: [7; 16],
            revision: 9,
        };
        let (_sender, mut packet) = real_packet(&proxy, scope);
        let original = packet.clone();
        let mut fault = PacketFault::new(300).unwrap();
        assert!(fault.packet(Direction::HostToGuest, &mut packet).is_none());
        assert!(fault.packet(Direction::GuestToHost, &mut packet).is_none());
        fault.arm().unwrap();
        let witness = fault.packet(Direction::HostToGuest, &mut packet).unwrap();
        assert_eq!(witness.new_checksum, witness.old_checksum ^ 1);
        assert_eq!(&packet[..SCOPE_HEADER], &original[..SCOPE_HEADER]);
        assert_eq!(
            &packet[CONNECTION_START..HEADER_START],
            &original[CONNECTION_START..HEADER_START]
        );
        // Native Event payload starts at7; the CRC's four bytes are the only
        // changed Event bytes, including flags and every other FRAMEINFO field.
        assert_eq!(
            &packet[HEADER_START..HEADER_START + 7],
            &original[HEADER_START..HEADER_START + 7]
        );
        assert_eq!(&packet[HEADER_START + 11..], &original[HEADER_START + 11..]);
        let proxy_address = match proxy.local_addr().unwrap() {
            SocketAddr::V4(value) => value,
            _ => unreachable!(),
        };
        let mut receiver = transport(proxy_address, scope);
        proxy
            .send_to(&packet, receiver.primary_addr().unwrap())
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let received = loop {
            let report = receiver.service(2);
            assert_eq!(report.malformed_datagrams, 0);
            if let Some(packet) = receiver.pop_received() {
                break packet;
            }
            assert!(Instant::now() < deadline);
        };
        let info = decode_frame_info(&received.payload).unwrap();
        let mut history = MultiplayerChecksumHistory::new();
        history.record(witness.reported_frame as u32, witness.old_checksum);
        let mismatch = history.compare(info).unwrap_err();
        assert_eq!(mismatch.remote_checksum, witness.new_checksum);
        assert_eq!(mismatch.local_checksum, witness.old_checksum);
        assert!(fault.packet(Direction::HostToGuest, &mut packet).is_none());
    }
    #[test]
    fn fault_requires_both_scopes_and_ignores_probe_or_changed_scope_bytes() {
        let proxy = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        proxy
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (_sender, mut packet) = real_packet(
            &proxy,
            SessionScope {
                epoch: [3; 16],
                revision: 2,
            },
        );
        let mut fault = PacketFault::new(300).unwrap();
        fault.packet(Direction::HostToGuest, &mut packet);
        assert!(fault.arm().is_err());
        fault.packet(Direction::GuestToHost, &mut packet);
        fault.arm().unwrap();
        let mut wrong = packet.clone();
        wrong[4] ^= 1;
        assert!(fault.packet(Direction::HostToGuest, &mut wrong).is_none());
        let mut probe = packet.clone();
        probe[HEADER_START] = 0x19;
        let checksum = udp_checksum(&probe[CONNECTION_START..]);
        probe[SCOPE_HEADER..CONNECTION_START].copy_from_slice(&checksum.to_le_bytes());
        assert!(fault.packet(Direction::HostToGuest, &mut probe).is_none());
        assert!(fault.packet(Direction::HostToGuest, &mut packet).is_some());
    }
    #[test]
    fn external_router_uses_opposite_identity_sockets_and_rejects_the_wrong_tuple() {
        use std::sync::mpsc;
        let bind = || {
            let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            socket
        };
        let address = |socket: &UdpSocket| match socket.local_addr().unwrap() {
            SocketAddr::V4(value) => value,
            _ => unreachable!(),
        };
        let host = bind();
        let guest = bind();
        let host_identity = bind();
        let guest_identity = bind();
        let host_proxy = address(&host_identity);
        let guest_proxy = address(&guest_identity);
        let config = Config {
            host_proxy,
            guest_proxy,
            host: address(&host),
            guest: address(&guest),
            timeout: Duration::from_secs(5),
        };
        config.validate().unwrap();
        let (send, controls) = mpsc::sync_channel(2);
        let (notices, receive) = mpsc::channel();
        let process = std::thread::spawn(move || {
            run_sockets(
                config,
                [host_identity, guest_identity],
                controls,
                |notice| notices.send(notice).map_err(io::Error::other),
            )
        });
        assert!(
            matches!(receive.recv_timeout(Duration::from_secs(2)).unwrap(), Notice::Ready { host_proxy: p1, guest_proxy: p2, .. } if p1 == host_proxy && p2 == guest_proxy)
        );
        let mut bytes = [0; 64];
        host.send_to(b"global setup", guest_proxy).unwrap();
        let (length, source) = guest.recv_from(&mut bytes).unwrap();
        assert_eq!(
            (&bytes[..length], source),
            (b"global setup".as_slice(), SocketAddr::V4(host_proxy))
        );
        guest.send_to(b"loading/private ACK", host_proxy).unwrap();
        let (length, source) = host.recv_from(&mut bytes).unwrap();
        assert_eq!(
            (&bytes[..length], source),
            (
                b"loading/private ACK".as_slice(),
                SocketAddr::V4(guest_proxy)
            )
        );
        // Each admitted actual tuple is still invalid on the other identity.
        host.send_to(b"wrong host tuple", host_proxy).unwrap();
        guest.send_to(b"guest barrier", host_proxy).unwrap();
        let (length, source) = host.recv_from(&mut bytes).unwrap();
        assert_eq!(
            (&bytes[..length], source),
            (b"guest barrier".as_slice(), SocketAddr::V4(guest_proxy))
        );
        guest.send_to(b"wrong guest tuple", guest_proxy).unwrap();
        host.send_to(b"host barrier", guest_proxy).unwrap();
        let (length, source) = guest.recv_from(&mut bytes).unwrap();
        assert_eq!(
            (&bytes[..length], source),
            (b"host barrier".as_slice(), SocketAddr::V4(host_proxy))
        );
        send.send(Control::Stop).unwrap();
        process.join().unwrap().unwrap();
        assert!(matches!(
            receive.recv_timeout(Duration::from_secs(2)).unwrap(),
            Notice::Stopped {
                forwarded: 4,
                rejected_sources: 2,
                corrupted_directions: 0
            }
        ));
    }
}
