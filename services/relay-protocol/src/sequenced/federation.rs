//! Regional relay federation. The lobby homes each player on a nearby relay.
//! Every relay seals its own players' commands at the shared frame deadlines
//! and sends those contributions to the other relays of the match; each then
//! emits the same merged stream to its own players. An own command waits one
//! way to the farthest relay instead of a round trip to one central relay.
//! Player DATA crosses the same links. Synchronized pause is not federated.
//! Pacing is: the others forward their players' pace reports to the
//! coordinator, whose rate changes reach them as entries in frame order.
//! So are checksums: the others forward their players' reports, and only
//! the coordinator compares them and decides a desync.

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
// Link packets go out at least every 50 ms; a coordinator this quiet is gone.
const COORDINATOR_SILENCE_MS: u64 = 10_000;
const DATA_KIND: u8 = 1;
// A follower's players' latest pace reports, for the coordinator.
const PACE_KIND: u8 = 2;
const PACE_FORWARD_MS: u64 = 500;
// source, pace, LE frames run u16, LE tick cost u32 (0: none), LE age u32.
const PACE_FORWARD_ENTRY: usize = 12;
// A follower's players' checksum reports, for the coordinator: source,
// LE sample frame u32, LE checksum u32.
const CHECKSUM_KIND: u8 = 3;
const CHECKSUM_FORWARD_ENTRY: usize = 9;
// Reports a follower holds for one link pass; 64 fit one link datagram.
pub(super) const FORWARD_CHECKSUMS: usize = 64;
const FRAME_HEADER: usize = 86;
const ENTRY_HEADER: usize = 15;
// A seal sends at once; this only paces startup, acks and clock samples.
const LINK_INTERVAL_MS: u64 = 50;
// A packet carrying entries goes out twice, so one lost datagram costs no
// frame. A bare seal goes once: the next seal, one frame later, covers it.
const LINK_COPIES: usize = 2;
const LINK_PACKETS: usize = 4;
// Entries go again once their acknowledgement is overdue: a round trip, the
// link interval within which the peer sends it, and as much again for jitter.
const RESEND_MARGIN_MS: u64 = 2 * LINK_INTERVAL_MS;
const CLOCK_SAMPLES: usize = 32;
// Before the coordinator has a round trip to every relay.
const UNMEASURED_LINK_MS: u64 = 500;

pub(crate) const fn roster_len(relays: usize) -> usize {
    ROSTER_HEADER + relays * (ROSTER_ENTRY + ROSTER_WRAP)
}

pub(crate) fn is_roster_len(length: usize) -> bool {
    (2..=MAX_FEDERATED_RELAYS).any(|relays| roster_len(relays) == length)
}

/// Layout shared with the lobby (docs/lobby-protocol.md): `VO1F`, room UUID16, relay
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
    // A follower's forwarded pace reports, encoded.
    pub paces: Vec<u8>,
    // A follower's players' checksum reports since its last pass.
    pub checksums: Vec<(u8, u32, u32)>,
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
    // Our first contribution not yet sent, and (end, time) of each send of
    // entries this relay has not acknowledged.
    sent_next: u64,
    flights: VecDeque<(u64, u64)>,
    // Silent past a player's limit after the start; its sources departed.
    lost: bool,
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
    // When a follower last moved its start toward the coordinator's.
    slewed_ms: u64,
    // When a follower last forwarded its players' pace reports.
    paces_sent_ms: Option<u64>,
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
                sent_next: 0,
                flights: VecDeque::new(),
                lost: false,
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
            slewed_ms: 0,
            paces_sent_ms: None,
        })
    }

    /// Every admission to a room must carry the room's exact roster.
    pub(crate) fn admits(&self, roster: &[u8], source: u8) -> bool {
        self.roster == roster && self.local & (1 << source) != 0
    }

    pub(super) fn local(&self) -> u32 {
        self.local
    }

    /// The first roster relay starts the match and decides its rate.
    pub(super) fn is_coordinator(&self) -> bool {
        self.own == 0
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

    /// A frame is complete once every relay still linked has sealed it.
    pub(super) fn merged_through(&self, sealed: u32) -> u32 {
        self.links
            .iter()
            .filter(|link| !link.lost)
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
                if self.links[position].lost {
                    return false;
                }
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
        // A lost relay's players departed here; nothing it sends counts.
        if self.links[position].lost {
            return Err(RelayError::Unregistered);
        }
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
    /// the coordinator cancels, on any relay's request before its start; each
    /// relay requests it once its own startup reaches the loading limit.
    pub(super) fn coordinate(&mut self, ordering: &mut OrderingRoom, now_ms: u64) {
        if ordering.finished_at.is_some() {
            return;
        }
        // Each relay ends only its own players' places, and one that restarts
        // can neither resume its link nor recreate the room. Once a started
        // two-relay room has not heard the other relay for as long as a player
        // may be silent, its players depart as lost after the last frame it
        // sealed here, and the merge goes on without it. With more relays the
        // survivors would have to agree on that frame, which the link format
        // cannot carry, so such a room ends only once its own players have
        // left and every other relay has been silent that long.
        if ordering.starts_at.is_some() && self.links.len() > 1 {
            if ordering.active & self.local == 0
                && self.links.iter().all(|link| {
                    link.echo
                        .is_none_or(|(_, heard)| now_ms.saturating_sub(heard) >= super::ABSENT_MS)
                })
            {
                ordering.abandon(now_ms);
                return;
            }
        } else if ordering.starts_at.is_some() {
            for link in &mut self.links {
                let heard = link.echo.map_or(ordering.created_ms, |(_, heard)| heard);
                if link.lost || now_ms.saturating_sub(heard) < super::ABSENT_MS {
                    continue;
                }
                link.lost = true;
                for source in 0..MAX_PEERS {
                    if ordering.active & link.sources & (1 << source) != 0 {
                        ordering.accept_remote(link.sealed + 1, source, 0, &[1]);
                    }
                }
            }
        }
        let overdue = ordering.starts_at.is_none()
            && now_ms.saturating_sub(ordering.created_ms) >= super::LOADING_LIMIT_MS;
        if overdue {
            // A relay whose players never sent Ready has no room to ask with,
            // so every relay asks once its own startup reaches the limit.
            ordering.cancel_requested = true;
        }
        if ordering.starts_at.is_none()
            && if self.own == 0 {
                ordering.cancel_requested || self.links.iter().any(|link| link.cancelled)
            } else {
                // A coordinator that never got a room, or already dropped it,
                // cannot answer; its silence past the limit cancels here.
                self.links.iter().any(|link| {
                    link.index == 0
                        && (link.cancelled
                            || overdue
                                && link.echo.is_none_or(|(_, heard)| {
                                    now_ms.saturating_sub(heard) >= COORDINATOR_SILENCE_MS
                                }))
                })
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
        // At most a millisecond per elapsed millisecond, however often this
        // relay passes: its frame clock runs at most double speed, or stands,
        // while it converges.
        let step = now_ms.saturating_sub(self.slewed_ms);
        self.slewed_ms = now_ms;
        ordering.starts_at = Some(match ordering.starts_at {
            None => target,
            Some(start) if start < target => target.min(start + step),
            Some(start) => target.max(start.saturating_sub(step)),
        });
    }

    /// Link packets due this pass: at each new seal, and otherwise paced.
    /// Each entry goes once, and again only once its acknowledgement is
    /// overdue; a relay that misses one ignores what follows until then.
    pub(super) fn frames(&mut self, view: &LinkView, now_ms: u64) -> Vec<ForwardDatagram> {
        let mut output = Vec::new();
        let outbox_end = self.outbox_base + self.outbox.len() as u64;
        let held = self
            .links
            .iter()
            .filter(|link| !link.lost)
            .map(|link| link.acked)
            .min()
            .unwrap_or(outbox_end);
        while self.outbox_base < held && self.outbox.pop_front().is_some() {
            self.outbox_base += 1;
        }
        for position in 0..self.links.len() {
            let link = &mut self.links[position];
            if link.lost {
                continue;
            }
            while link
                .flights
                .front()
                .is_some_and(|&(end, _)| end <= link.acked)
            {
                link.flights.pop_front();
            }
            let overdue = link
                .best_sample()
                .map_or(UNMEASURED_LINK_MS, |(rtt, _)| rtt)
                + RESEND_MARGIN_MS;
            let resend = link
                .flights
                .front()
                .is_some_and(|&(_, at)| now_ms.saturating_sub(at) >= overdue);
            if !resend
                && link.last_sent.is_some_and(|(at, sealed)| {
                    sealed == view.sealed && now_ms.saturating_sub(at) < LINK_INTERVAL_MS
                })
            {
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
            let mut first = if resend {
                link.acked
            } else {
                link.acked.max(link.sent_next)
            }
            .max(self.outbox_base);
            let sent_from = first;
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
            let link = &mut self.links[position];
            if resend {
                link.flights.clear();
            }
            if first > sent_from {
                link.flights.push_back((first, now_ms));
            }
            link.sent_next = first;
            link.last_sent = Some((now_ms, view.sealed));
        }
        if self.own != 0
            && !view.paces.is_empty()
            && self
                .paces_sent_ms
                .is_none_or(|at| now_ms.saturating_sub(at) >= PACE_FORWARD_MS)
            && let Some(position) = self
                .links
                .iter()
                .position(|link| link.index == 0 && !link.lost)
        {
            let mut payload = vec![(view.paces.len() / PACE_FORWARD_ENTRY) as u8];
            payload.extend_from_slice(&view.paces);
            if let Ok(datagram) = self.packet(position, PACE_KIND, &payload) {
                output.push(datagram);
                self.paces_sent_ms = Some(now_ms);
            }
        }
        // Each client report repeats the sample before, so a lost link
        // datagram loses none; they go at the pass they arrive in.
        if self.own != 0
            && !view.checksums.is_empty()
            && let Some(position) = self
                .links
                .iter()
                .position(|link| link.index == 0 && !link.lost)
        {
            let mut payload = vec![view.checksums.len() as u8];
            for &(source, frame, checksum) in &view.checksums {
                payload.push(source);
                payload.extend_from_slice(&frame.to_le_bytes());
                payload.extend_from_slice(&checksum.to_le_bytes());
            }
            if let Ok(datagram) = self.packet(position, CHECKSUM_KIND, &payload) {
                output.push(datagram);
            }
        }
        output
    }

    /// The coordinator compares a follower's forwarded checksum reports
    /// with its own.
    fn receive_checksums(
        &self,
        position: usize,
        payload: &[u8],
        ordering: &mut OrderingRoom,
    ) -> Result<(), RelayError> {
        let count = usize::from(*payload.first().ok_or(RelayError::Malformed)?);
        if self.own != 0 || payload.len() != 1 + count * CHECKSUM_FORWARD_ENTRY {
            return Err(RelayError::Malformed);
        }
        let sources = self.links[position].sources;
        if payload[1..]
            .chunks_exact(CHECKSUM_FORWARD_ENTRY)
            .any(|entry| entry[0] >= MAX_PEERS || sources & (1 << entry[0]) == 0)
        {
            return Err(RelayError::Malformed);
        }
        for entry in payload[1..].chunks_exact(CHECKSUM_FORWARD_ENTRY) {
            ordering.record_checksum(entry[0], read_u32(entry, 1), read_u32(entry, 5), true);
        }
        Ok(())
    }

    /// The coordinator stores a follower's forwarded reports as its own.
    fn receive_paces(
        &self,
        position: usize,
        payload: &[u8],
        ordering: &mut OrderingRoom,
        now_ms: u64,
    ) -> Result<(), RelayError> {
        let count = usize::from(*payload.first().ok_or(RelayError::Malformed)?);
        if self.own != 0 || payload.len() != 1 + count * PACE_FORWARD_ENTRY {
            return Err(RelayError::Malformed);
        }
        let sources = self.links[position].sources;
        for entry in payload[1..].chunks_exact(PACE_FORWARD_ENTRY) {
            let source = entry[0];
            let pace = match entry[1] {
                0 => OrderPace::KeepingUp,
                1 => OrderPace::Behind,
                2 => OrderPace::CatchingUp,
                _ => return Err(RelayError::Malformed),
            };
            if source >= MAX_PEERS || sources & (1 << source) == 0 {
                return Err(RelayError::Malformed);
            }
            let tick = read_u32(entry, 4);
            ordering.sources[usize::from(source)].pace = Some(PaceReport {
                pace,
                frames_per_second: u16::from_le_bytes([entry[2], entry[3]]),
                tick_micros: (tick != 0).then_some(tick),
                at_ms: now_ms.saturating_sub(u64::from(read_u32(entry, 8))),
            });
        }
        Ok(())
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
        let from_coordinator = self.links[position].index == 0;
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
            // The coordinator's control entries: a rate change, LE rate u16
            // and the slow source, or a desync record's tail.
            let control = source == CONTROL_SOURCE && from_coordinator;
            if payload.len() < end
                || frame == 0
                || if control {
                    match batch_id {
                        RATE_BATCH => length != 3,
                        DESYNC_BATCH => length != DESYNC_TAIL,
                        _ => true,
                    }
                } else if source >= MAX_PEERS || sources & (1 << source) == 0 {
                    true
                } else if batch_id == 0 {
                    // A departure carries nothing, or 1 when silence ended it.
                    length > 1 || (length == 1 && payload[offset + ENTRY_HEADER] != 1)
                } else {
                    length <= OPAQUE_OVERHEAD || length > OPAQUE_OVERHEAD + ORDERING_BATCH_CAPACITY
                }
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
        // A finished room already has every entry it will archive.
        if ordering.finished_at.is_none() {
            for (frame, source, batch_id, range) in entries.iter().skip(fresh).cloned() {
                ordering.accept_remote(frame, source, batch_id, &payload[range]);
            }
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
        if !self
            .ingress
            .admit_part(&mut room.ingress, now, 1, INGRESS_PER_SECOND)
        {
            return Err(RelayError::RateLimited);
        }
        self.ingress.charge_part(&mut room.ingress, 1);
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
                    || !self.outgoing.admit_part(
                        &mut room.outgoing,
                        now,
                        cost,
                        TOTAL_BYTES_PER_SECOND,
                    )
                {
                    return Err(RelayError::RateLimited);
                }
                self.outgoing.charge_part(&mut room.outgoing, cost);
                room.peers
                    .iter_mut()
                    .filter(|peer| recipients(peer))
                    .map(|peer| server_packet(peer, DATA, source, data))
                    .collect()
            }
            PACE_KIND => {
                if let Some(ordering) = room.ordering.as_mut() {
                    federation.receive_paces(position, payload, ordering, now_ms)?;
                }
                Ok(Vec::new())
            }
            CHECKSUM_KIND => {
                if let Some(ordering) = room.ordering.as_mut() {
                    federation.receive_checksums(position, payload, ordering)?;
                }
                Ok(Vec::new())
            }
            _ => Err(RelayError::Malformed),
        }
    }

    /// Start agreement and contribution exchange for every federated room.
    pub fn drain_federation(&mut self, now_ms: u64, now: u64) -> Vec<ForwardDatagram> {
        self.prune(now);
        let mut output = Vec::new();
        for room in self.rooms.values_mut() {
            let Some(federation) = room.federation.as_mut() else {
                continue;
            };
            if let Some(ordering) = room.ordering.as_mut() {
                federation.coordinate(ordering, now_ms);
            }
            let view = match room.ordering.as_mut() {
                Some(ordering) => LinkView {
                    ready: ordering.ready,
                    agreement: ordering.agreement,
                    cancelled: ordering.cancel_requested
                        || (ordering.starts_at.is_none() && ordering.finished_at.is_some()),
                    starts_at: ordering.starts_at,
                    sealed: ordering.sealed,
                    paces: if federation.own == 0 {
                        Vec::new()
                    } else {
                        ordering.local_paces(federation.local(), now_ms)
                    },
                    checksums: std::mem::take(&mut ordering.forward_checksums),
                },
                None => LinkView {
                    ready: 0,
                    agreement: [0; 32],
                    cancelled: room.ordering_cancelled,
                    starts_at: None,
                    sealed: 0,
                    paces: Vec::new(),
                    checksums: Vec::new(),
                },
            };
            let datagrams = federation.frames(&view, now_ms);
            let cost = datagrams
                .iter()
                .map(|datagram| datagram.bytes.len() as u64)
                .sum();
            if self
                .outgoing
                .admit_part(&mut room.outgoing, now, cost, TOTAL_BYTES_PER_SECOND)
            {
                self.outgoing.charge_part(&mut room.outgoing, cost);
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
    // `Match::stream`'s source for a DESYNC record.
    const DESYNC_MARK: u8 = u8::MAX - 1;
    // Relay B's process clock runs this far ahead of A's.
    const CLOCK_B: u64 = 1000;

    /// San Francisco and Singapore: source 0 homed on A, source 1 on B.
    struct Match {
        relays: [RelayServer; 2],
        journals: [TestDirectory; 2],
        clients: [ClientSession; 2],
        addresses: [SocketAddrV4; 2],
        links: Vec<(u64, usize, Vec<u8>)>,
        delivered: Vec<ForwardDatagram>,
        drop_first_copies: bool,
        // When each relay sent a link datagram carrying entries, and how
        // many of relay A's are still to be lost.
        carried: [Vec<u64>; 2],
        lose: usize,
        now: u64,
    }

    impl Match {
        fn new(drop_first_copies: bool) -> Self {
            Self::build(drop_first_copies, [true; 2])
        }

        /// A match whose players on unready relays connect but never send Ready.
        fn build(drop_first_copies: bool, ready: [bool; 2]) -> Self {
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
                journals,
                clients: [client(&keys[0], 0), client(&keys[1], 1)],
                addresses,
                links: Vec::new(),
                delivered: Vec::new(),
                drop_first_copies,
                carried: [Vec::new(), Vec::new()],
                lose: 0,
                now: 0,
            };
            for relay in 0..2 {
                let clock = game.clock(relay);
                game.clients[relay].set_federation(&roster).unwrap();
                connect(&mut game.relays[relay], &mut game.clients[relay], clock);
                if ready[relay] {
                    configure(
                        &mut game.relays[relay],
                        &mut game.clients[relay],
                        &certificate(&keys[relay], 3, 2000),
                        clock,
                    );
                }
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
                    let carries = datagram.bytes[29] == FRAME_KIND
                        && datagram.bytes[LINK_HEADER + FRAME_HEADER - 1] != 0;
                    if carries {
                        self.carried[from].push(self.now);
                    }
                    let lost = carries && from == 0 && self.lose > 0;
                    self.lose -= usize::from(lost);
                    if !(self.drop_first_copies && copied || lost) {
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

        /// The merged stream a relay's player reads, without sequence gaps. A
        /// rate marker reads `(CONTROL_SOURCE, rate << 8 | slow source)`, a
        /// desync `(DESYNC_MARK, sample frame << 40 | outlier << 32 | its
        /// checksum)`.
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
                        ..
                    }) => Some((sequence, frame, Some((source, 0)))),
                    OrderEvent::Entry(OrderEntry::Rate {
                        sequence,
                        frame,
                        frames_per_second,
                        source,
                    }) => Some((
                        sequence,
                        frame,
                        Some((
                            CONTROL_SOURCE,
                            u64::from(frames_per_second) << 8
                                | u64::from(source.unwrap_or(CONTROL_SOURCE)),
                        )),
                    )),
                    OrderEvent::Entry(OrderEntry::Desync {
                        sequence,
                        frame,
                        sample_frame,
                        outlier,
                        outlier_checksum,
                        ..
                    }) => Some((
                        sequence,
                        frame,
                        Some((
                            DESYNC_MARK,
                            u64::from(sample_frame) << 40
                                | u64::from(outlier) << 32
                                | u64::from(outlier_checksum),
                        )),
                    )),
                    _ => None,
                })
                .collect();
                if page.is_empty() {
                    return entries;
                }
                entries.extend(page);
            }
        }

        fn checksums(&mut self, relay: usize, samples: &[(u32, u32)]) {
            let clock = self.clock(relay);
            let mut bytes = Vec::new();
            self.clients[relay]
                .report_ordering_checksums(samples, &mut bytes)
                .unwrap();
            self.relays[relay]
                .process_at(endpoint(&self.clients[relay]), &bytes, unix(clock), clock)
                .unwrap();
        }

        fn pace(&mut self, relay: usize, pace: OrderPace, frames_per_second: u16) {
            let clock = self.clock(relay);
            let mut bytes = Vec::new();
            self.clients[relay]
                .report_ordering_pace(pace, frames_per_second, None, &mut bytes)
                .unwrap();
            self.relays[relay]
                .process_at(endpoint(&self.clients[relay]), &bytes, unix(clock), clock)
                .unwrap();
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
            // A seal wait of at most one 45 Hz turn of three frames, then one
            // way to the other relay; a single central relay costs a full
            // round trip.
            assert!(
                (LINK_MS..=LINK_MS + 68).contains(&own),
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

    /// A follower's player who cannot keep up reaches the coordinator, whose
    /// rate change both relays apply after the same frame, so both emit one
    /// stream and seal at the new rate.
    #[test]
    fn a_followers_lagging_player_slows_both_relays_after_one_frame() {
        use OrderPace::*;
        let mut game = Match::new(false);
        while game.ordering(0).sealed < 10 || game.ordering(1).sealed < 10 {
            game.step();
        }
        game.pace(0, KeepingUp, 45);
        game.pace(1, Behind, 40);
        for _ in 0..1500 {
            game.step();
            // Both relays seal whole turns from the same anchor.
            for relay in 0..2 {
                let ordering = game.ordering(relay);
                let (anchor, _) = ordering.pace.federated_anchor.unwrap_or((0, 0));
                assert_eq!(
                    (ordering.sealed - anchor) % turn_frames(ordering.pace.rate),
                    0,
                    "relay {relay} sealed {} from {anchor}",
                    ordering.sealed
                );
            }
        }
        assert_eq!(
            game.ordering(0).pace.federated_anchor,
            game.ordering(1).pace.federated_anchor
        );
        assert_eq!(
            [game.ordering(0).pace.rate, game.ordering(1).pace.rate],
            [30, 30]
        );
        let sealed = game.ordering(1).sealed;
        for _ in 0..1000 {
            game.step();
        }
        assert!(
            (29..=31).contains(&(game.ordering(1).sealed - sealed)),
            "the follower seals at the new rate"
        );
        let streams = [game.stream(0), game.stream(1)];
        assert_eq!(streams[0], streams[1], "one canonical stream");
        let markers: Vec<_> = streams[0]
            .iter()
            .filter(|(.., entry)| entry.is_some_and(|(source, _)| source == CONTROL_SOURCE))
            .map(|(_, _, entry)| entry.unwrap().1)
            .collect();
        assert_eq!(markers, [30 << 8 | 1], "slowed to 30 for source 1");
    }

    /// Only the coordinator compares checksums: a follower forwards its
    /// player's, and both relays emit the coordinator's one DESYNC record
    /// after the same frame.
    #[test]
    fn both_relays_emit_the_coordinators_desync_after_the_same_frame() {
        let mut game = Match::new(false);
        // A player reports a sampled frame only once its relays sealed the one
        // before it.
        while game.ordering(0).sealed < 14 || game.ordering(1).sealed < 14 {
            game.step();
        }
        game.checksums(0, &[(15, 1)]);
        game.checksums(1, &[(15, 2)]);
        for _ in 0..500 {
            game.step();
        }
        let streams = [game.stream(0), game.stream(1)];
        assert_eq!(streams[0], streams[1], "one canonical stream");
        let desyncs: Vec<_> = streams[0]
            .iter()
            .filter_map(|&(_, _, entry)| entry.filter(|(source, _)| *source == DESYNC_MARK))
            .map(|(_, desync)| desync)
            .collect();
        assert_eq!(
            desyncs,
            [15 << 40 | 1 << 32 | 2],
            "the later reporter, of two"
        );
    }

    #[test]
    fn a_follower_slews_its_start_by_elapsed_time_however_often_it_passes() {
        let mut game = Match::new(false);
        game.play();
        // Relay B's start is 50 ms off the coordinator's. Ten passes in one
        // millisecond move it one millisecond; one pass ten later, ten more.
        let clock = game.clock(1);
        let start = game.ordering(1).starts_at.unwrap() + 50;
        let room = game.relays[1].rooms.get_mut(&[5; 16]).unwrap();
        room.ordering.as_mut().unwrap().starts_at = Some(start);
        for (now_ms, passes, moved) in [(clock + 1, 10, 1), (clock + 11, 1, 11)] {
            for _ in 0..passes {
                game.relays[1].drain_federation(now_ms, unix(now_ms));
            }
            assert_eq!(game.ordering(1).starts_at, Some(start - moved));
        }
    }

    #[test]
    fn players_leaving_on_both_relays_at_once_free_both_archives() {
        let mut game = Match::new(false);
        game.play();
        game.leave(0);
        game.leave(1);
        for _ in 0..400 {
            game.step();
        }
        // Each relay sealed frames past its last merge before the other's
        // departure arrived, and merges their frame ends after finishing.
        for relay in 0..2 {
            let ordering = game.ordering(relay);
            assert!(ordering.finished_at.is_some() && !ordering.storage_failed);
            assert_eq!(ordering.head, ordering.sealed);
            let clock = game.clock(relay) + crate::MAX_TICKET_LIFETIME * 1000;
            game.relays[relay].advance_clock(clock, unix(clock));
            assert!(!game.journals[relay].0.join("05".repeat(16)).exists());
        }
    }

    #[test]
    fn a_relay_ends_the_match_once_its_players_left_and_the_other_relay_went_silent() {
        let mut game = Match::new(false);
        game.play();
        game.leave(0);
        // Relay B restarts and loses the room, so its player's departure never
        // reaches relay A. A ends the match once B has been silent as long as
        // a player may be, and frees the archive after the replay grace.
        let room = &game.relays[0].rooms[&[5; 16]];
        let (_, heard) = room.federation.as_ref().unwrap().links[0].echo.unwrap();
        for (now_ms, finished) in [(heard + ABSENT_MS - 1, false), (heard + ABSENT_MS, true)] {
            game.relays[0].drain_federation(now_ms, unix(now_ms));
            game.relays[0].advance_clock(now_ms, unix(now_ms));
            assert_eq!(game.ordering(0).finished_at.is_some(), finished);
        }
        let freed = heard + ABSENT_MS + crate::MAX_TICKET_LIFETIME * 1000;
        game.relays[0].advance_clock(freed, unix(freed));
        assert!(!game.journals[0].0.join("05".repeat(16)).exists());
    }

    #[test]
    fn silent_players_depart_as_lost_once_from_their_own_relay() {
        let mut game = Match::new(false);
        game.play();
        // Neither player sends anything for ten minutes while the relays keep
        // their link. Each relay ends only the place it homes, and both merge
        // both departures.
        for _ in 0..ABSENT_MS / 1000 {
            game.now += 999;
            game.step();
        }
        for _ in 0..300 {
            game.step();
        }
        for relay in 0..2 {
            // The departures precede ten minutes of frames, past the RAM cache.
            let ordering = game.relays[relay]
                .rooms
                .get_mut(&[5; 16])
                .unwrap()
                .ordering
                .as_mut()
                .unwrap();
            let mut departures = Vec::new();
            let mut cursor = 1;
            while cursor < ordering.next_seq {
                let page = ordering.history_page(cursor, 32).unwrap();
                cursor += page.len() as u64;
                departures.extend(
                    page.into_iter()
                        .filter(|entry| entry[0] == DEPARTURE_EVENT)
                        .map(|entry| entry[13..].to_vec()),
                );
            }
            assert_eq!(departures, [[0, 1], [1, 1]], "relay {relay}");
        }
    }

    #[test]
    fn a_relay_departs_a_silent_relays_players_as_lost_and_plays_on() {
        let mut game = Match::new(false);
        game.play();
        // Relay B restarts: its link counts from 1 again, which A's replay
        // window refuses, and its files keep it from recreating the room. A
        // never hears from B again; A's player stays.
        let link = &game.relays[0].rooms[&[5; 16]]
            .federation
            .as_ref()
            .unwrap()
            .links[0];
        let ((_, heard), sealed) = (link.echo.unwrap(), link.sealed);
        let keepalive = heard + 150_000;
        pull(&mut game.relays[0], &mut game.clients[0], 1, keepalive);
        for (now_ms, merged) in [(heard + ABSENT_MS - 1, false), (heard + ABSENT_MS, true)] {
            game.relays[0].drain_federation(now_ms, unix(now_ms));
            game.relays[0].advance_clock(now_ms, unix(now_ms));
            let ordering = game.ordering(0);
            assert_eq!(ordering.head == ordering.sealed, merged);
            assert!(ordering.finished_at.is_none(), "relay A's player plays on");
        }
        let ordering = game.relays[0]
            .rooms
            .get_mut(&[5; 16])
            .unwrap()
            .ordering
            .as_mut()
            .unwrap();
        let mut departures = Vec::new();
        let mut cursor = 1;
        while cursor < ordering.next_seq {
            let page = ordering.history_page(cursor, 32).unwrap();
            cursor += page.len() as u64;
            departures.extend(
                page.into_iter()
                    .filter(|entry| entry[0] == DEPARTURE_EVENT)
                    .map(|entry| (read_u32(&entry, 9), entry[13..].to_vec())),
            );
        }
        assert_eq!(
            departures,
            [(sealed + 1, vec![1, 1])],
            "after B's last seal"
        );
    }

    #[test]
    fn link_entries_cross_twice_and_go_again_only_once_overdue() {
        let mut game = Match::new(false);
        let healthy = game.play();
        for _ in 0..500 {
            game.step();
        }
        assert_eq!(
            game.carried.map(|sent| sent.len()),
            [LINK_COPIES; 2],
            "one command each way"
        );
        // Both copies of relay A's command are lost. B ignores A's later
        // packets until A sends it again, a round trip and margin later.
        let mut game = Match::new(false);
        game.lose = LINK_COPIES;
        let latency = game.play();
        for _ in 0..500 {
            game.step();
        }
        let sent = &game.carried[0];
        assert_eq!(sent.len(), 2 * LINK_COPIES);
        let overdue = 2 * LINK_MS + RESEND_MARGIN_MS;
        assert!(
            (overdue..=overdue + 2).contains(&(sent[LINK_COPIES] - sent[0])),
            "sent again {} ms later",
            sent[LINK_COPIES] - sent[0]
        );
        assert_eq!(latency[0], healthy[0]);
        assert_eq!(game.stream(0), game.stream(1));
    }

    #[test]
    fn a_start_whose_coordinator_never_answers_cancels_at_the_loading_limit() {
        // The coordinator's only player never became ready, so it has no
        // ordering room and, once that player's tickets lapse, no hop room.
        let mut game = Match::build(false, [false, true]);
        let limit = game.ordering(1).created_ms + LOADING_LIMIT_MS;
        for now_ms in [limit - 1, limit] {
            game.relays[1].advance_clock(now_ms, unix(now_ms));
            game.relays[1].drain_federation(now_ms, unix(now_ms));
        }
        let ordering = game.ordering(1);
        assert!(ordering.starts_at.is_none() && ordering.finished_at == Some(limit));
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
