//! Regional relay federation. The lobby homes each player on a nearby relay.
//! Every relay seals its own players' commands at the shared frame deadlines
//! and sends those contributions to the other relays of the match; each then
//! emits the same merged stream to its own players. An own command waits one
//! way to the farthest relay instead of a round trip to one central relay.
//! Player DATA crosses the same links. Synchronized pause is not federated.

use super::*;

pub const MAX_FEDERATED_RELAYS: usize = 6;
const ROSTER_MAGIC: &[u8; 4] = b"VO1F";
const ROSTER_HEADER: usize = 21;
const ROSTER_ENTRY: usize = 10;
const ROSTER_WRAP: usize = 80;
const WRAP_DOMAIN: &[u8] = b"VERA20k relay federation wrap v1\0";
const ROSTER_DOMAIN: &[u8] = b"VERA20k relay federation roster v1\0";
const LINK_DOMAIN: &[u8] = b"VERA20k relay federation link v1\0";
pub(crate) const LINK_MAGIC: &[u8; 4] = b"VF1L";
const LINK_HEADER: usize = 32;
const FRAME_KIND: u8 = 0;
const DATA_KIND: u8 = 1;
const FRAME_HEADER: usize = 86;
const ENTRY_HEADER: usize = 15;
// A seal sends at once; this only paces startup, acks and clock samples.
const LINK_INTERVAL_MS: u64 = 50;
// A packet carrying entries goes out twice, so one lost datagram costs no
// frame. A bare seal goes once: the next seal, one frame later, covers it.
const LINK_COPIES: usize = 2;
const LINK_PACKETS: usize = 4;
const CLOCK_SAMPLES: usize = 32;
// Before the coordinator has a round trip to every relay.
const UNMEASURED_LINK_MS: u64 = 500;

pub(crate) const fn roster_len(relays: usize) -> usize {
    ROSTER_HEADER + relays * (ROSTER_ENTRY + ROSTER_WRAP)
}

pub(crate) fn is_roster_len(length: usize) -> bool {
    (2..=MAX_FEDERATED_RELAYS).any(|relays| roster_len(relays) == length)
}

/// Layout shared with services/lobby/service.js: `VO1F`, room UUID16, relay
/// count, then per relay IPv4, LE port and LE source mask. Per relay follow a
/// 16-byte nonce, the 32-byte link key XORed with HMAC(relay key, wrap domain,
/// room, nonce) and HMAC(relay key, roster domain, table, index, nonce,
/// wrapped). A relay finds itself as the one entry its own key authenticates.
pub(crate) fn roster_table(
    bytes: &[u8],
) -> Result<([u8; 16], Vec<(SocketAddrV4, u32)>), RelayError> {
    if bytes.len() < ROSTER_HEADER || &bytes[..4] != ROSTER_MAGIC {
        return Err(RelayError::Malformed);
    }
    let count = usize::from(bytes[20]);
    if !(2..=MAX_FEDERATED_RELAYS).contains(&count) || bytes.len() != roster_len(count) {
        return Err(RelayError::Malformed);
    }
    let mut relays: Vec<(SocketAddrV4, u32)> = Vec::with_capacity(count);
    let mut union = 0;
    for entry in
        bytes[ROSTER_HEADER..ROSTER_HEADER + count * ROSTER_ENTRY].chunks_exact(ROSTER_ENTRY)
    {
        let endpoint = SocketAddrV4::new(
            Ipv4Addr::new(entry[0], entry[1], entry[2], entry[3]),
            u16::from_le_bytes([entry[4], entry[5]]),
        );
        let sources = read_u32(entry, 6);
        if sources == 0
            || sources >> MAX_PEERS != 0
            || union & sources != 0
            || endpoint.port() == 0
            || relays.iter().any(|(other, _)| *other == endpoint)
        {
            return Err(RelayError::Malformed);
        }
        union |= sources;
        relays.push((endpoint, sources));
    }
    Ok((bytes[4..20].try_into().unwrap(), relays))
}

fn wrap_stream(key: &SecretKey, room: &[u8; 16], nonce: &[u8]) -> [u8; 32] {
    let mut mac = key.mac(WRAP_DOMAIN, room);
    mac.update(nonce);
    mac.finalize().into_bytes().into()
}

fn wrap_tag(key: &SecretKey, table: &[u8], index: u8, nonce: &[u8], wrapped: &[u8]) -> HmacSha256 {
    let mut mac = key.mac(ROSTER_DOMAIN, table);
    mac.update(&[index]);
    mac.update(nonce);
    mac.update(wrapped);
    mac
}

/// The lobby's signer, for tools and tests. Relay keys never reach clients;
/// the coordinator is the first relay.
pub fn sign_federation_roster(
    room: [u8; 16],
    relays: &[(SocketAddrV4, u32, &SecretKey)],
    link: &SecretKey,
) -> Result<Vec<u8>, RelayError> {
    let count = relays.len();
    let mut bytes = Vec::with_capacity(roster_len(count));
    bytes.extend_from_slice(ROSTER_MAGIC);
    bytes.extend_from_slice(&room);
    bytes.push(u8::try_from(count).map_err(|_| RelayError::Malformed)?);
    for (endpoint, sources, _) in relays {
        bytes.extend_from_slice(&endpoint.ip().octets());
        bytes.extend_from_slice(&endpoint.port().to_le_bytes());
        bytes.extend_from_slice(&sources.to_le_bytes());
    }
    let table = bytes.clone();
    for (index, (_, _, key)) in relays.iter().enumerate() {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).map_err(|_| RelayError::EntropyUnavailable)?;
        let stream = wrap_stream(key, &room, &nonce);
        let wrapped: Vec<u8> = link.0.iter().zip(stream).map(|(a, b)| a ^ b).collect();
        let tag = wrap_tag(key, &table, index as u8, &nonce, &wrapped)
            .finalize()
            .into_bytes();
        bytes.extend_from_slice(&nonce);
        bytes.extend_from_slice(&wrapped);
        bytes.extend_from_slice(&tag);
    }
    roster_table(&bytes)?;
    Ok(bytes)
}

/// What a relay tells its peers about its own part of the match.
pub(super) struct LinkView {
    pub ready: u32,
    pub agreement: [u8; 32],
    pub cancelled: bool,
    pub starts_at: Option<u64>,
    pub sealed: u32,
}

struct Link {
    index: u8,
    endpoint: SocketAddrV4,
    sources: u32,
    sent: u64,
    received: ReplayWindow,
    // Our contributions this relay holds; theirs we hold.
    acked: u64,
    inbox_next: u64,
    sealed: u32,
    ready: u32,
    agreement: [u8; 32],
    cancelled: bool,
    start_at: u64,
    // Their latest send time and when it arrived, echoed for clock samples.
    echo: Option<(u64, u64)>,
    // (round trip, their clock minus ours), best sample wins.
    samples: Vec<(u64, i64)>,
    next_sample: usize,
    last_sent: Option<(u64, u32)>,
}

impl Link {
    fn best_sample(&self) -> Option<(u64, i64)> {
        self.samples.iter().copied().min_by_key(|&(rtt, _)| rtt)
    }
}

pub(crate) struct Federation {
    roster: Vec<u8>,
    room: [u8; 16],
    own: u8,
    local: u32,
    link: SecretKey,
    links: Vec<Link>,
    // This relay's sealed contributions, (frame, encoded entry), from `outbox_base`.
    outbox: VecDeque<(u32, Vec<u8>)>,
    outbox_base: u64,
}

impl Federation {
    /// Admission verifies the roster and that the ticket's player is homed here.
    pub(crate) fn admit(
        key: &SecretKey,
        roster: &[u8],
        ticket: &Ticket,
    ) -> Result<Self, RelayError> {
        let (room, relays) = roster_table(roster)?;
        if room != ticket.room {
            return Err(RelayError::Authentication);
        }
        let table = &roster[..ROSTER_HEADER + relays.len() * ROSTER_ENTRY];
        let wraps = &roster[table.len()..];
        let mut own = None;
        for (index, wrap) in wraps.chunks_exact(ROSTER_WRAP).enumerate() {
            if wrap_tag(key, table, index as u8, &wrap[..16], &wrap[16..48])
                .verify_slice(&wrap[48..])
                .is_ok()
            {
                if own.is_some() {
                    return Err(RelayError::Authentication);
                }
                own = Some(index);
            }
        }
        let own = own.ok_or(RelayError::Authentication)?;
        if relays[own].1 & (1 << ticket.source) == 0 {
            return Err(RelayError::Authentication);
        }
        let wrap = &wraps[own * ROSTER_WRAP..][..ROSTER_WRAP];
        let stream = wrap_stream(key, &room, &wrap[..16]);
        let mut link = [0; 32];
        for (byte, (wrapped, mask)) in link.iter_mut().zip(wrap[16..48].iter().zip(stream)) {
            *byte = wrapped ^ mask;
        }
        let links = relays
            .iter()
            .enumerate()
            .filter(|&(index, _)| index != own)
            .map(|(index, &(endpoint, sources))| Link {
                index: index as u8,
                endpoint,
                sources,
                sent: 0,
                received: ReplayWindow::default(),
                acked: 0,
                inbox_next: 0,
                sealed: 0,
                ready: 0,
                agreement: [0; 32],
                cancelled: false,
                start_at: 0,
                echo: None,
                samples: Vec::with_capacity(CLOCK_SAMPLES),
                next_sample: 0,
                last_sent: None,
            })
            .collect();
        Ok(Self {
            roster: roster.to_vec(),
            room,
            own: own as u8,
            local: relays[own].1,
            link: SecretKey::new(link),
            links,
            outbox: VecDeque::new(),
            outbox_base: 0,
        })
    }

    /// Every admission to a room must carry the room's exact roster.
    pub(crate) fn admits(&self, roster: &[u8], source: u8) -> bool {
        self.roster == roster && self.local & (1 << source) != 0
    }

    pub(super) fn local(&self) -> u32 {
        self.local
    }

    pub(super) fn contribute(&mut self, frame: u32, source: u8, batch_id: u64, opaque: &[u8]) {
        let mut entry = Vec::with_capacity(ENTRY_HEADER + opaque.len());
        entry.extend_from_slice(&frame.to_le_bytes());
        entry.push(source);
        entry.extend_from_slice(&batch_id.to_le_bytes());
        entry.extend_from_slice(&(opaque.len() as u16).to_le_bytes());
        entry.extend_from_slice(opaque);
        self.outbox.push_back((frame, entry));
    }

    /// A frame is complete once every relay has sealed it.
    pub(super) fn merged_through(&self, sealed: u32) -> u32 {
        self.links
            .iter()
            .map(|link| link.sealed)
            .fold(sealed, u32::min)
    }

    /// Remote readiness counts only under the same startup agreement.
    pub(super) fn ready(&self, local_ready: u32, agreement: &[u8; 32]) -> u32 {
        self.links
            .iter()
            .filter(|link| link.agreement == *agreement)
            .fold(local_ready & self.local, |mask, link| mask | link.ready)
    }

    /// Relays forwarding a player DATA packet from `source` to `target`.
    pub(crate) fn data_links(&self, source: u8, target: u8) -> Vec<usize> {
        (0..self.links.len())
            .filter(|&position| {
                let sources = self.links[position].sources & !(1 << source);
                if target == BROADCAST {
                    sources != 0
                } else {
                    target < MAX_PEERS && sources & (1 << target) != 0
                }
            })
            .collect()
    }

    pub(crate) fn link_data(
        &mut self,
        position: usize,
        source: u8,
        target: u8,
        payload: &[u8],
    ) -> Result<ForwardDatagram, RelayError> {
        let mut body = Vec::with_capacity(2 + payload.len());
        body.extend([source, target]);
        body.extend_from_slice(payload);
        self.packet(position, DATA_KIND, &body)
    }

    pub(crate) const fn data_len(payload: usize) -> usize {
        LINK_HEADER + 2 + payload + MAC_LEN
    }

    fn packet(
        &mut self,
        position: usize,
        kind: u8,
        payload: &[u8],
    ) -> Result<ForwardDatagram, RelayError> {
        let link = &mut self.links[position];
        link.sent = link
            .sent
            .checked_add(1)
            .ok_or(RelayError::CounterExhausted)?;
        let mut bytes = Vec::with_capacity(LINK_HEADER + payload.len() + MAC_LEN);
        bytes.extend_from_slice(LINK_MAGIC);
        bytes.extend_from_slice(&self.room);
        bytes.push(self.own);
        bytes.extend_from_slice(&link.sent.to_le_bytes());
        bytes.push(kind);
        bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        bytes.extend_from_slice(payload);
        let tag = self.link.mac(LINK_DOMAIN, &bytes).finalize().into_bytes();
        bytes.extend_from_slice(&tag);
        Ok(ForwardDatagram {
            endpoint: link.endpoint,
            bytes,
        })
    }

    /// Authenticate a link packet. Its MAC and sequence prove the sender, so
    /// a relay whose datagrams leave from another address (NAT, a reserved
    /// IP) still links; replies always go to the roster address.
    fn open<'a>(&mut self, bytes: &'a [u8]) -> Result<(usize, u8, &'a [u8]), RelayError> {
        if bytes.len() < LINK_HEADER + MAC_LEN
            || usize::from(u16::from_le_bytes([bytes[30], bytes[31]]))
                != bytes.len() - LINK_HEADER - MAC_LEN
        {
            return Err(RelayError::Malformed);
        }
        let position = self
            .links
            .iter()
            .position(|link| link.index == bytes[20])
            .ok_or(RelayError::Authentication)?;
        self.link
            .mac(LINK_DOMAIN, &bytes[..bytes.len() - MAC_LEN])
            .verify_slice(&bytes[bytes.len() - MAC_LEN..])
            .map_err(|_| RelayError::Authentication)?;
        self.links[position].received.accept(read_u64(bytes, 21))?;
        Ok((
            position,
            bytes[29],
            &bytes[LINK_HEADER..bytes.len() - MAC_LEN],
        ))
    }

    /// Startup: the coordinator starts once every relay reports the full
    /// roster ready, leaving its slowest link time to hear. The others place
    /// the same start on their own clocks and keep slewing toward it. Only
    /// the coordinator cancels, on any relay's request before its start.
    pub(super) fn coordinate(&self, ordering: &mut OrderingRoom, now_ms: u64) {
        if ordering.finished_at.is_some() {
            return;
        }
        if ordering.starts_at.is_none()
            && if self.own == 0 {
                ordering.cancel_requested || self.links.iter().any(|link| link.cancelled)
            } else {
                self.links
                    .iter()
                    .any(|link| link.index == 0 && link.cancelled)
            }
        {
            ordering.cancel(now_ms);
            return;
        }
        if self.own == 0 {
            if ordering.starts_at.is_none()
                && self.ready(ordering.ready, &ordering.agreement)
                    == ordering.certificate.sources_mask
            {
                let slowest = self
                    .links
                    .iter()
                    .map(|link| {
                        link.best_sample()
                            .map_or(UNMEASURED_LINK_MS, |(rtt, _)| rtt)
                    })
                    .max()
                    .unwrap_or(0);
                ordering.starts_at = Some(now_ms + START_DELAY_MS + slowest);
            }
            return;
        }
        let Some(coordinator) = self.links.iter().find(|link| link.index == 0) else {
            return;
        };
        let (Some((_, offset)), true) = (coordinator.best_sample(), coordinator.start_at != 0)
        else {
            return;
        };
        let target = (i128::from(coordinator.start_at) - i128::from(offset)).max(0) as u64;
        ordering.starts_at = Some(match ordering.starts_at {
            None => target,
            Some(start) if start < target => start + 1,
            Some(start) if start > target => start - 1,
            Some(start) => start,
        });
    }

    /// Link packets due this pass: at each new seal, and otherwise paced.
    pub(super) fn frames(&mut self, view: &LinkView, now_ms: u64) -> Vec<ForwardDatagram> {
        let mut output = Vec::new();
        let outbox_end = self.outbox_base + self.outbox.len() as u64;
        for position in 0..self.links.len() {
            let link = &self.links[position];
            if link.last_sent.is_some_and(|(at, sealed)| {
                sealed == view.sealed && now_ms.saturating_sub(at) < LINK_INTERVAL_MS
            }) {
                continue;
            }
            let start_at = if self.own == 0 {
                view.starts_at.unwrap_or(0)
            } else {
                0
            };
            let (echo_ms, echo_hold) = link.echo.map_or((0, 0), |(sent, received)| {
                (
                    sent,
                    now_ms.saturating_sub(received).min(u64::from(u32::MAX)) as u32,
                )
            });
            let acknowledged = link.inbox_next;
            let mut first = link.acked.max(self.outbox_base);
            for _ in 0..LINK_PACKETS {
                let mut entries = Vec::new();
                let mut count = 0u8;
                let mut next = first;
                while next < outbox_end && count < u8::MAX {
                    let entry = &self.outbox[(next - self.outbox_base) as usize].1;
                    if LINK_HEADER + FRAME_HEADER + entries.len() + entry.len() + MAC_LEN
                        > MAX_DATAGRAM
                    {
                        break;
                    }
                    entries.extend_from_slice(entry);
                    count += 1;
                    next += 1;
                }
                // Our seal covers this packet only up to the first entry it
                // leaves out; outbox entries are in frame order.
                let sealed = if next == outbox_end {
                    view.sealed
                } else {
                    self.outbox[(next - self.outbox_base) as usize].0 - 1
                };
                let mut payload = Vec::with_capacity(FRAME_HEADER + entries.len());
                payload.extend_from_slice(&(view.ready & self.local).to_le_bytes());
                payload.push(u8::from(view.cancelled));
                payload.extend_from_slice(&start_at.to_le_bytes());
                payload.extend_from_slice(&now_ms.to_le_bytes());
                payload.extend_from_slice(&echo_ms.to_le_bytes());
                payload.extend_from_slice(&echo_hold.to_le_bytes());
                payload.extend_from_slice(&acknowledged.to_le_bytes());
                payload.extend_from_slice(&first.to_le_bytes());
                payload.extend_from_slice(&sealed.to_le_bytes());
                payload.extend_from_slice(&view.agreement);
                payload.push(count);
                payload.extend_from_slice(&entries);
                for _ in 0..if count == 0 { 1 } else { LINK_COPIES } {
                    if let Ok(datagram) = self.packet(position, FRAME_KIND, &payload) {
                        output.push(datagram);
                    }
                }
                first = next;
                if next == outbox_end {
                    break;
                }
            }
            self.links[position].last_sent = Some((now_ms, view.sealed));
        }
        output
    }

    /// Apply one authenticated FRAME. Remote entries enter the ordering room
    /// only contiguously, so each is merged exactly once.
    fn receive_frame(
        &mut self,
        position: usize,
        payload: &[u8],
        ordering: Option<&mut OrderingRoom>,
        now_ms: u64,
    ) -> Result<(), RelayError> {
        if payload.len() < FRAME_HEADER || payload[4] > 1 {
            return Err(RelayError::Malformed);
        }
        let ack = read_u64(payload, 33);
        let first = read_u64(payload, 41);
        let sealed = read_u32(payload, 49);
        let sources = self.links[position].sources;
        let mut entries = Vec::with_capacity(usize::from(payload[85]));
        let mut offset = FRAME_HEADER;
        for _ in 0..payload[85] {
            if payload.len() < offset + ENTRY_HEADER {
                return Err(RelayError::Malformed);
            }
            let frame = read_u32(payload, offset);
            let source = payload[offset + 4];
            let batch_id = read_u64(payload, offset + 5);
            let length = usize::from(u16::from_le_bytes([
                payload[offset + 13],
                payload[offset + 14],
            ]));
            let end = offset + ENTRY_HEADER + length;
            if source >= MAX_PEERS
                || sources & (1 << source) == 0
                || payload.len() < end
                || frame == 0
                || (batch_id == 0) != (length == 0)
                || (batch_id != 0
                    && (length <= OPAQUE_OVERHEAD
                        || length > OPAQUE_OVERHEAD + ORDERING_BATCH_CAPACITY))
            {
                return Err(RelayError::Malformed);
            }
            entries.push((frame, source, batch_id, offset + ENTRY_HEADER..end));
            offset = end;
        }
        if offset != payload.len() || ack > self.outbox_base + self.outbox.len() as u64 {
            return Err(RelayError::Malformed);
        }
        let link = &mut self.links[position];
        link.ready = read_u32(payload, 0) & sources;
        link.cancelled |= payload[4] != 0;
        link.agreement = payload[53..85].try_into().unwrap();
        if link.index == 0 {
            link.start_at = read_u64(payload, 5);
        }
        let sent_ms = read_u64(payload, 13);
        let echo_ms = read_u64(payload, 21);
        if echo_ms != 0 && echo_ms <= now_ms {
            let hold = u64::from(read_u32(payload, 29));
            let rtt = (now_ms - echo_ms).saturating_sub(hold);
            // Their clock minus ours, from the midpoints of both legs.
            let offset = (2 * i128::from(sent_ms)
                - i128::from(hold)
                - i128::from(echo_ms)
                - i128::from(now_ms))
                / 2;
            let sample = (
                rtt,
                offset.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64,
            );
            if link.samples.len() < CLOCK_SAMPLES {
                link.samples.push(sample);
            } else {
                link.samples[link.next_sample] = sample;
            }
            link.next_sample = (link.next_sample + 1) % CLOCK_SAMPLES;
        }
        if link.echo.is_none_or(|(sent, _)| sent_ms > sent) {
            link.echo = Some((sent_ms, now_ms));
        }
        link.acked = link.acked.max(ack);
        let held = self.links.iter().map(|link| link.acked).min().unwrap_or(0);
        while self.outbox_base < held && self.outbox.pop_front().is_some() {
            self.outbox_base += 1;
        }
        let link = &mut self.links[position];
        let Some(ordering) = ordering else {
            return Ok(());
        };
        if first > link.inbox_next {
            return Ok(());
        }
        let fresh = (link.inbox_next - first) as usize;
        if entries
            .iter()
            .skip(fresh)
            .any(|(frame, ..)| *frame <= ordering.head)
        {
            return Err(RelayError::Malformed);
        }
        for (frame, source, batch_id, range) in entries.iter().skip(fresh).cloned() {
            ordering.accept_remote(frame, source, batch_id, &payload[range]);
        }
        link.inbox_next = link.inbox_next.max(first + entries.len() as u64);
        link.sealed = link.sealed.max(sealed);
        Ok(())
    }
}

impl RelayServer {
    /// A packet from another relay of a federated match.
    pub(crate) fn federation_receive(
        &mut self,
        bytes: &[u8],
        now: u64,
        now_ms: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        if bytes.len() < LINK_HEADER + MAC_LEN {
            return Err(RelayError::Malformed);
        }
        let id: [u8; 16] = bytes[4..20].try_into().unwrap();
        let room = self.rooms.get_mut(&id).ok_or(RelayError::Unregistered)?;
        let federation = room.federation.as_mut().ok_or(RelayError::Unregistered)?;
        let (position, kind, payload) = federation.open(bytes)?;
        match kind {
            FRAME_KIND => {
                federation.receive_frame(position, payload, room.ordering.as_mut(), now_ms)?;
                // Only the coordinator's cancel binds; it honours any request.
                if payload[4] != 0
                    && room.ordering.is_none()
                    && (federation.own == 0 || federation.links[position].index == 0)
                {
                    room.ordering_cancelled = true;
                }
                self.advance_ordering_room(id, now_ms);
                Ok(Vec::new())
            }
            DATA_KIND => {
                if payload.len() < 3 {
                    return Err(RelayError::Malformed);
                }
                let (source, target, data) = (payload[0], payload[1], &payload[2..]);
                if source >= MAX_PEERS
                    || federation.links[position].sources & (1 << source) == 0
                    || (target >= MAX_PEERS && target != BROADCAST)
                    || data.len() <= PAYLOAD_HEADER + PAYLOAD_TAG
                    || data.len() > MAX_WIRE_PAYLOAD
                    || data[0] != target
                {
                    return Err(RelayError::Malformed);
                }
                let recipients = |peer: &Peer| {
                    now.saturating_sub(peer.last_seen) <= 30
                        && (target == BROADCAST || target == peer.ticket.source)
                };
                let cost = (room.peers.iter().filter(|peer| recipients(peer)).count()
                    * (SERVER_HEADER + MAC_LEN + data.len())) as u64;
                if !room.outgoing.permits(now, cost, ROOM_BYTES_PER_SECOND)
                    || !self.outgoing.permits(now, cost, TOTAL_BYTES_PER_SECOND)
                {
                    return Err(RelayError::RateLimited);
                }
                room.outgoing.charge(now, cost);
                self.outgoing.charge(now, cost);
                room.peers
                    .iter_mut()
                    .filter(|peer| recipients(peer))
                    .map(|peer| server_packet(peer, DATA, source, data))
                    .collect()
            }
            _ => Err(RelayError::Malformed),
        }
    }

    /// Start agreement and contribution exchange for every federated room.
    pub fn drain_federation(&mut self, now_ms: u64, now: u64) -> Vec<ForwardDatagram> {
        let mut output = Vec::new();
        for room in self.rooms.values_mut() {
            let Some(federation) = room.federation.as_mut() else {
                continue;
            };
            if let Some(ordering) = room.ordering.as_mut() {
                federation.coordinate(ordering, now_ms);
            }
            let view = match room.ordering.as_ref() {
                Some(ordering) => LinkView {
                    ready: ordering.ready,
                    agreement: ordering.agreement,
                    cancelled: ordering.cancel_requested
                        || (ordering.starts_at.is_none() && ordering.finished_at.is_some()),
                    starts_at: ordering.starts_at,
                    sealed: ordering.sealed,
                },
                None => LinkView {
                    ready: 0,
                    agreement: [0; 32],
                    cancelled: room.ordering_cancelled,
                    starts_at: None,
                    sealed: 0,
                },
            };
            let datagrams = federation.frames(&view, now_ms);
            let cost = datagrams
                .iter()
                .map(|datagram| datagram.bytes.len() as u64)
                .sum();
            if self.outgoing.permits(now, cost, TOTAL_BYTES_PER_SECOND) {
                self.outgoing.charge(now, cost);
                output.extend(datagrams);
            }
        }
        output
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        TestDirectory, certificate, client, configure, connect, endpoint, events, pull, unix,
    };
    use super::*;

    const LINK_MS: u64 = 88;
    // Relay B's process clock runs this far ahead of A's.
    const CLOCK_B: u64 = 1000;

    /// San Francisco and Singapore: source 0 homed on A, source 1 on B.
    struct Match {
        relays: [RelayServer; 2],
        _journals: [TestDirectory; 2],
        clients: [ClientSession; 2],
        addresses: [SocketAddrV4; 2],
        links: Vec<(u64, usize, Vec<u8>)>,
        delivered: Vec<ForwardDatagram>,
        drop_first_copies: bool,
        now: u64,
    }

    impl Match {
        fn new(drop_first_copies: bool) -> Self {
            let keys = [SecretKey::new([11; 32]), SecretKey::new([12; 32])];
            let addresses = [
                SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 5000),
                SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 2), 5000),
            ];
            let roster = sign_federation_roster(
                [5; 16],
                &[(addresses[0], 1, &keys[0]), (addresses[1], 2, &keys[1])],
                &SecretKey::new([9; 32]),
            )
            .unwrap();
            // Deployed relays journal every merged frame.
            let journals = [TestDirectory::new(), TestDirectory::new()];
            let mut game = Self {
                relays: [
                    RelayServer::with_journal(keys[0].clone(), 2, &journals[0].0).unwrap(),
                    RelayServer::with_journal(keys[1].clone(), 2, &journals[1].0).unwrap(),
                ],
                _journals: journals,
                clients: [client(&keys[0], 0), client(&keys[1], 1)],
                addresses,
                links: Vec::new(),
                delivered: Vec::new(),
                drop_first_copies,
                now: 0,
            };
            for relay in 0..2 {
                let clock = game.clock(relay);
                game.clients[relay].set_federation(&roster).unwrap();
                connect(&mut game.relays[relay], &mut game.clients[relay], clock);
                configure(
                    &mut game.relays[relay],
                    &mut game.clients[relay],
                    &certificate(&keys[relay], 3, 1400),
                    clock,
                );
            }
            game
        }

        fn clock(&self, relay: usize) -> u64 {
            self.now + relay as u64 * CLOCK_B
        }

        fn ordering(&self, relay: usize) -> &OrderingRoom {
            self.relays[relay].rooms[&[5; 16]]
                .ordering
                .as_ref()
                .unwrap()
        }

        fn route(&mut self, from: usize, datagrams: Vec<ForwardDatagram>) {
            // Copies differ only in their sequence number and MAC.
            let body = |bytes: &[u8]| bytes[29..bytes.len() - MAC_LEN].to_vec();
            let mut datagrams = datagrams.into_iter().peekable();
            while let Some(datagram) = datagrams.next() {
                if datagram.endpoint == self.addresses[1 - from] {
                    let copied = datagrams
                        .peek()
                        .is_some_and(|next| body(&next.bytes) == body(&datagram.bytes));
                    if !(self.drop_first_copies && copied) {
                        self.links
                            .push((self.now + LINK_MS, 1 - from, datagram.bytes));
                    }
                } else {
                    self.delivered.push(datagram);
                }
            }
        }

        fn step(&mut self) {
            self.now += 1;
            for relay in 0..2 {
                let clock = self.clock(relay);
                self.relays[relay].advance_clock(clock, unix(clock));
                let datagrams = self.relays[relay].drain_federation(clock, unix(clock));
                self.route(relay, datagrams);
            }
            let (due, later) = std::mem::take(&mut self.links)
                .into_iter()
                .partition(|(at, ..)| *at <= self.now);
            self.links = later;
            for (_, relay, bytes) in due {
                let clock = self.clock(relay);
                let output = self.relays[relay]
                    .process_at(self.addresses[1 - relay], &bytes, unix(clock), clock)
                    .unwrap();
                self.route(relay, output);
            }
        }

        fn submit(&mut self, relay: usize) -> u32 {
            let clock = self.clock(relay);
            let mut bytes = Vec::new();
            self.clients[relay]
                .submit_ordering(1, b"move", &mut bytes)
                .unwrap();
            let OrderEvent::Ack { frame, .. } = events(
                &mut self.relays[relay],
                &mut self.clients[relay],
                &bytes,
                clock,
            )[0] else {
                panic!("ack");
            };
            frame
        }

        fn leave(&mut self, relay: usize) {
            let clock = self.clock(relay);
            let mut bytes = Vec::new();
            self.clients[relay].leave_ordering(&mut bytes).unwrap();
            self.relays[relay]
                .process_at(endpoint(&self.clients[relay]), &bytes, unix(clock), clock)
                .unwrap();
        }

        /// The merged stream a relay's player reads, without sequence gaps.
        fn stream(&mut self, relay: usize) -> Vec<(u64, u32, Option<(u8, u64)>)> {
            let clock = self.clock(relay);
            let mut entries: Vec<(u64, u32, Option<(u8, u64)>)> = Vec::new();
            loop {
                let cursor = entries.last().map_or(1, |(sequence, ..)| sequence + 1);
                let page: Vec<_> = pull(
                    &mut self.relays[relay],
                    &mut self.clients[relay],
                    cursor,
                    clock,
                )
                .into_iter()
                .filter_map(|event| match event {
                    OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame }) => {
                        Some((sequence, frame, None))
                    }
                    OrderEvent::Entry(OrderEntry::Batch {
                        sequence,
                        frame,
                        source,
                        batch_id,
                        ..
                    }) => Some((sequence, frame, Some((source, batch_id)))),
                    OrderEvent::Entry(OrderEntry::Departure {
                        sequence,
                        frame,
                        source,
                    }) => Some((sequence, frame, Some((source, 0)))),
                    _ => None,
                })
                .collect();
                if page.is_empty() {
                    return entries;
                }
                entries.extend(page);
            }
        }

        /// Own-command latency per side, from submission to its merged frame.
        fn play(&mut self) -> [u64; 2] {
            while self.ordering(0).sealed < 10 || self.ordering(1).sealed < 10 {
                self.step();
            }
            let submitted = self.now;
            let frames = [self.submit(0), self.submit(1)];
            let mut latency = [None; 2];
            while latency.iter().any(Option::is_none) {
                assert!(self.now < submitted + 1000, "commands never merged");
                self.step();
                for relay in 0..2 {
                    if latency[relay].is_none() && self.ordering(relay).head >= frames[relay] {
                        latency[relay] = Some(self.now - submitted);
                    }
                }
            }
            latency.map(Option::unwrap)
        }
    }

    #[test]
    fn federated_relays_merge_one_stream_one_way_apart_and_forward_player_data() {
        let mut game = Match::new(false);
        let latency = game.play();
        let starts = [
            game.ordering(0).starts_at.unwrap(),
            game.ordering(1).starts_at.unwrap(),
        ];
        assert!(
            starts[1].abs_diff(starts[0] + CLOCK_B) <= 1,
            "both relays seal on one timeline"
        );
        for own in latency {
            // A seal wait of at most one 45 Hz frame, then one way to the
            // other relay; a single central relay costs a full round trip.
            assert!(
                (LINK_MS..=LINK_MS + 24).contains(&own),
                "own command took {own} ms"
            );
        }
        let clock = game.clock(0);
        let mut bytes = Vec::new();
        game.clients[0].data(1, b"probe", &mut bytes).unwrap();
        let output = game.relays[0]
            .process_at(endpoint(&game.clients[0]), &bytes, unix(clock), clock)
            .unwrap();
        game.route(0, output);
        game.leave(1);
        for _ in 0..200 {
            game.step();
        }
        let probe = game
            .delivered
            .iter()
            .find(|datagram| datagram.endpoint == endpoint(&game.clients[1]))
            .unwrap();
        let ServerEvent::Payload { source: 0, bytes } =
            game.clients[1].receive(&probe.bytes).unwrap()
        else {
            panic!("forwarded player data");
        };
        assert_eq!(bytes.as_bytes(), b"probe");
        let streams = [game.stream(0), game.stream(1)];
        assert_eq!(
            streams[0], streams[1],
            "both relays emit the same canonical stream"
        );
        let commands: Vec<_> = streams[0].iter().filter_map(|(.., entry)| *entry).collect();
        assert_eq!(commands, [(0, 1), (1, 1), (1, 0)]);
    }

    #[test]
    fn a_lost_link_copy_costs_no_frame() {
        assert_eq!(Match::new(true).play(), Match::new(false).play());
    }

    #[test]
    fn only_the_coordinator_cancels_a_federated_start() {
        let mut game = Match::new(false);
        game.leave(1);
        for _ in 0..400 {
            game.step();
        }
        for relay in 0..2 {
            let ordering = game.ordering(relay);
            assert!(ordering.starts_at.is_none() && ordering.finished_at.is_some());
        }
        // The start is already crossing the link when relay B's player leaves.
        let mut game = Match::new(false);
        while game.ordering(0).starts_at.is_none() {
            game.step();
        }
        game.leave(1);
        for _ in 0..1000 {
            game.step();
        }
        assert!(game.ordering(0).head > 1, "relay A's player keeps playing");
        let streams = [game.stream(0), game.stream(1)];
        assert_eq!(streams[0], streams[1]);
        let first = streams[0]
            .iter()
            .find_map(|&(_, frame, entry)| entry.map(|e| (frame, e)));
        assert_eq!(
            first,
            Some((1, (1, 0))),
            "a frame-1 departure, as on one relay"
        );
    }

    #[test]
    fn relays_admit_only_their_own_players_under_an_unchanged_roster() {
        let keys = [SecretKey::new([11; 32]), SecretKey::new([12; 32])];
        let addresses = [
            SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 5000),
            SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 2), 5000),
        ];
        let roster = sign_federation_roster(
            [5; 16],
            &[(addresses[0], 1, &keys[0]), (addresses[1], 2, &keys[1])],
            &SecretKey::new([9; 32]),
        )
        .unwrap();
        let admit = |key: &SecretKey, source: u8, roster: &[u8]| {
            let mut relay = RelayServer::new(key.clone(), 2);
            let mut player = client(key, source);
            player.set_federation(roster).unwrap();
            let mut bytes = Vec::new();
            player.probe(&mut bytes).unwrap();
            let challenge = relay
                .process_at(endpoint(&player), &bytes, unix(0), 0)
                .unwrap()
                .remove(0);
            let ServerEvent::Challenge(cookie) = player.receive(&challenge.bytes).unwrap() else {
                panic!("challenge");
            };
            player.confirm(cookie, &mut bytes).unwrap();
            relay
                .process_at(endpoint(&player), &bytes, unix(0), 0)
                .map(|_| ())
        };
        assert!(admit(&keys[0], 0, &roster).is_ok());
        assert_eq!(
            admit(&keys[1], 0, &roster),
            Err(RelayError::Authentication),
            "source 0 is homed on A"
        );
        let mut swapped = roster.clone();
        swapped[ROSTER_HEADER + 6] = 2;
        swapped[ROSTER_HEADER + ROSTER_ENTRY + 6] = 1;
        assert_eq!(
            admit(&keys[1], 0, &swapped),
            Err(RelayError::Authentication),
            "a player cannot rehome itself in the signed table"
        );
    }
}
