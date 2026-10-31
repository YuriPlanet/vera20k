//! One released client's side of `tools/relay_compat.py`.
//!
//! The tool builds this file against a release's own `vera20k-relay-protocol`
//! and runs it against a loopback relay built from the checkout. It uses only
//! the client API every supported release has: Pull, never Push or Pause.
//!
//! Three players are signed, but player 2 never connects. After
//! `LOADING_SILENCE_MS` (60 s) the relay removes it with the departure it writes
//! for silence, and the match starts. Player 1 then sends a batch and leaves.
//! Player 0 must then read, by Pull, every record up to player 1's Leave
//! departure: the silence departure, the batch, frame ends and that departure.
//! A released game drops a datagram it cannot decode and pulls the same record
//! again, so an unreadable record stalls it here as it would in a match.
//! Every Pull reply starts with a Status, and a client that cannot read one
//! never sees its match start, so an unreadable Status fails at once.
//! The key below is public fixture data only.
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vera20k_relay_protocol::{
    ClientSession, MAX_DATAGRAM, OrderEntry, OrderError, OrderEvent, OrderingCertificate,
    SecretKey, ServerEvent, Ticket, derive_packet_key,
};

type Result<T> = std::result::Result<T, String>;

const SILENT: u8 = 2;
// The server header is 48 bytes; its kind byte 4 is control (4), and a control
// payload's first byte 1 is a Status. Released clients share this layout.
const CONTROL_KIND: (usize, u8) = (4, 4);
const PAYLOAD_KIND: (usize, u8) = (48, 1);
const BATCH: &[u8] = b"relay-compat-v1";
const DEADLINE: Duration = Duration::from_secs(150);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Record {
    Frame(u32),
    Batch(u32, u8, u64),
    Departure(u32, u8),
}

struct Peer {
    socket: UdpSocket,
    client: ClientSession,
    records: BTreeMap<u64, Record>,
    undecodable: usize,
    left: bool,
}

impl Peer {
    fn send(&self, bytes: &[u8]) -> Result<()> {
        match self.socket.send(bytes) {
            Ok(sent) if sent == bytes.len() => Ok(()),
            _ => Err("send".into()),
        }
    }

    fn packet(&self) -> Result<Option<Vec<u8>>> {
        let mut bytes = vec![0; MAX_DATAGRAM + 1];
        match self.socket.recv(&mut bytes) {
            Ok(length) if length <= MAX_DATAGRAM => {
                bytes.truncate(length);
                Ok(Some(bytes))
            }
            Ok(_) => Err("oversize_packet".into()),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(_) => Err("receive".into()),
        }
    }

    /// Reads every waiting datagram and returns the ready mask if one came.
    fn service(&mut self) -> Result<Option<u32>> {
        let mut ready = None;
        while let Some(packet) = self.packet()? {
            let event = self.client.receive(&packet);
            if is_status(&packet)
                && !matches!(event, Ok(ServerEvent::Ordering(OrderEvent::Status(_))))
            {
                return Err("undecodable_status".into());
            }
            match event {
                Ok(ServerEvent::Ordering(OrderEvent::Entry(entry))) => {
                    if let Some((sequence, record)) = record(&entry) {
                        if *self.records.entry(sequence).or_insert(record) != record {
                            return Err(format!("record_{sequence}_changed"));
                        }
                    }
                }
                Ok(ServerEvent::Ordering(OrderEvent::Ready { ready_mask, .. })) => {
                    ready = Some(ready_mask)
                }
                Ok(ServerEvent::Ordering(OrderEvent::Left)) => self.left = true,
                // Pull answers this until the relay removes player 2 and starts.
                Ok(ServerEvent::Ordering(OrderEvent::Error {
                    code: OrderError::NotStarted,
                    ..
                })) => {}
                Ok(ServerEvent::Ordering(OrderEvent::Error { code, .. })) => {
                    return Err(format!("ordering_error_{code:?}"));
                }
                Ok(_) => {}
                Err(_) => self.undecodable += 1,
            }
        }
        Ok(ready)
    }

    fn next(&self) -> u64 {
        (1..)
            .find(|sequence| !self.records.contains_key(sequence))
            .unwrap()
    }
}

fn is_status(packet: &[u8]) -> bool {
    packet.get(CONTROL_KIND.0) == Some(&CONTROL_KIND.1)
        && packet.get(PAYLOAD_KIND.0) == Some(&PAYLOAD_KIND.1)
}

fn record(entry: &OrderEntry) -> Option<(u64, Record)> {
    match *entry {
        OrderEntry::FrameEnd {
            sequence, frame, ..
        } => Some((sequence, Record::Frame(frame))),
        OrderEntry::Batch {
            sequence,
            frame,
            source,
            batch_id,
            ..
        } => Some((sequence, Record::Batch(frame, source, batch_id))),
        OrderEntry::Departure {
            sequence,
            frame,
            source,
            ..
        } => Some((sequence, Record::Departure(frame, source))),
        // Kinds newer than the oldest supported release (Pause, Rate) never
        // occur here.
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

fn step(deadline: Instant, stage: &str) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(stage.into());
    }
    std::thread::sleep(Duration::from_millis(2));
    Ok(())
}

fn connect(
    address: SocketAddrV4,
    key: &SecretKey,
    room: [u8; 16],
    source: u8,
    now: u64,
    deadline: Instant,
) -> Result<Peer> {
    let mut player = room;
    player[0] ^= source + 1;
    let mut nonce = room;
    nonce[15] ^= source + 1;
    let ticket = Ticket {
        room,
        player,
        source,
        generation: 1,
        nonce,
        expires: now + 240,
    }
    .sign(key)
    .map_err(|_| "ticket")?;
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|_| "bind")?;
    socket.connect(address).map_err(|_| "connect")?;
    socket.set_nonblocking(true).map_err(|_| "nonblocking")?;
    let client = ClientSession::new(
        &ticket,
        *derive_packet_key(key, &ticket).expose_for_https(),
        [89; 32],
    )
    .map_err(|_| "client")?;
    let mut peer = Peer {
        socket,
        client,
        records: BTreeMap::new(),
        undecodable: 0,
        left: false,
    };
    let mut bytes = Vec::new();
    peer.client.probe(&mut bytes).map_err(|_| "probe")?;
    peer.send(&bytes)?;
    loop {
        if let Some(packet) = peer.packet()? {
            match peer
                .client
                .receive(&packet)
                .map_err(|_| "handshake_decode")?
            {
                ServerEvent::Challenge(cookie) => {
                    let cookie = cookie.to_vec();
                    peer.client
                        .confirm(&cookie, &mut bytes)
                        .map_err(|_| "confirm")?;
                    peer.send(&bytes)?;
                }
                ServerEvent::Ready => return Ok(peer),
                _ => return Err("handshake_event".into()),
            }
        }
        step(deadline, "handshake_timeout")?;
    }
}

/// Player 0's records through player 1's Leave departure, once contiguous.
fn complete(records: &BTreeMap<u64, Record>) -> Option<u64> {
    let (&end, _) = records
        .iter()
        .find(|(_, record)| matches!(record, Record::Departure(_, 1)))?;
    (1..=end)
        .all(|sequence| records.contains_key(&sequence))
        .then_some(end)
}

fn run(started: Instant) -> Result<String> {
    let deadline = started + DEADLINE;
    let address: SocketAddrV4 = std::env::args()
        .nth(1)
        .ok_or("address_argument")?
        .parse()
        .map_err(|_| "ipv4_address_argument")?;
    if !address.ip().is_loopback() {
        return Err("loopback_only".into());
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "clock")?;
    let room = now.as_nanos().to_le_bytes();
    let key = SecretKey::new([73; 32]); // PUBLIC loopback fixture key.
    let mut peers = Vec::new();
    for source in 0..2 {
        peers.push(connect(
            address,
            &key,
            room,
            source,
            now.as_secs(),
            deadline,
        )?);
    }
    let certificate = OrderingCertificate {
        room,
        sources_mask: 0b111,
        tick_hz: 45,
        expires: now.as_secs() + 600,
    }
    .sign(&key)
    .map_err(|_| "certificate")?;
    let mut bytes = Vec::new();
    let mut ready = [false; 2];
    let mut retry_at = Instant::now();
    while ready != [true; 2] {
        if Instant::now() >= retry_at {
            for peer in &mut peers {
                peer.client
                    .configure_ordering(&certificate, [7; 32], &mut bytes)
                    .map_err(|_| "configure")?;
                peer.send(&bytes)?;
            }
            retry_at = Instant::now() + Duration::from_millis(50);
        }
        for (index, peer) in peers.iter_mut().enumerate() {
            ready[index] |= peer.service()?.is_some_and(|mask| mask & 0b11 == 0b11);
        }
        step(deadline, "configure_timeout")?;
    }
    // Both players keep pulling, which also keeps them from going silent.
    let mut pull_at = Instant::now();
    let mut submitted = false;
    let mut leave_at = None;
    let end = loop {
        // The relay refuses a batch until the start; its first record follows it.
        if !submitted && !peers[1].records.is_empty() {
            peers[1]
                .client
                .submit_ordering(1, BATCH, &mut bytes)
                .map_err(|_| "submit")?;
            peers[1].send(&bytes)?;
            submitted = true;
        }
        if Instant::now() >= pull_at {
            for peer in peers.iter_mut().filter(|peer| !peer.left) {
                let next = peer.next();
                peer.client
                    .pull_ordering(next, &mut bytes)
                    .map_err(|_| "pull")?;
                peer.send(&bytes)?;
            }
            pull_at = Instant::now() + Duration::from_millis(20);
        }
        for peer in &mut peers {
            peer.service()?;
        }
        let seen_batch = peers[1]
            .records
            .values()
            .any(|record| matches!(record, Record::Batch(_, 1, 1)));
        if seen_batch && !peers[1].left && leave_at.is_none_or(|at| Instant::now() >= at) {
            peers[1]
                .client
                .leave_ordering(&mut bytes)
                .map_err(|_| "leave")?;
            peers[1].send(&bytes)?;
            leave_at = Some(Instant::now() + Duration::from_millis(500));
        }
        if let Some(end) = complete(&peers[0].records) {
            break end;
        }
        if Instant::now() >= deadline {
            let peer = &peers[0];
            return Err(format!(
                "stalled_at_record_{}_after_{}_undecodable_datagrams",
                peer.next(),
                peer.undecodable
            ));
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let records = &peers[0].records;
    let silent = records
        .values()
        .any(|record| matches!(record, Record::Departure(_, SILENT)));
    let batch = records
        .values()
        .any(|record| matches!(record, Record::Batch(_, 1, 1)));
    if !silent || !batch {
        return Err(format!("missing_records_silent_{silent}_batch_{batch}"));
    }
    if (1..=end).any(|sequence| {
        peers[1]
            .records
            .get(&sequence)
            .is_some_and(|record| records.get(&sequence) != Some(record))
    }) {
        return Err("players_read_different_records".into());
    }
    let mut leave_at = Instant::now();
    while !peers[0].left {
        if Instant::now() >= leave_at {
            peers[0]
                .client
                .leave_ordering(&mut bytes)
                .map_err(|_| "leave")?;
            peers[0].send(&bytes)?;
            leave_at = Instant::now() + Duration::from_millis(500);
        }
        peers[0].service()?;
        step(deadline, "leave_timeout")?;
    }
    Ok(format!(
        "\"records\":{end},\"undecodable\":{}",
        peers[0].undecodable
    ))
}

fn main() {
    let started = Instant::now();
    match run(started) {
        Ok(detail) => println!(
            "{{\"ok\":true,{detail},\"elapsed_ms\":{}}}",
            started.elapsed().as_millis()
        ),
        Err(stage) => {
            println!(
                "{{\"ok\":false,\"stage\":\"{stage}\",\"elapsed_ms\":{}}}",
                started.elapsed().as_millis()
            );
            std::process::exit(1);
        }
    }
}
