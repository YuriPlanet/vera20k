//! Watchers (README "Watchers"): read-only hops that follow a started,
//! journaled match from its first record, some time behind its players.
//! A watcher holds a version 3 ticket and has no source: it counts for no
//! start, pace, checksum or departure, sends no batch and leaves nothing in
//! the stream, so the players' games never learn it is there. It only pulls,
//! and its traffic spends budgets of its own, none of the players'.

// Outside data reaches this module: it refuses bad input, never panics.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::panic))]

use std::collections::VecDeque;

use super::*;
use crate::link::Reader;

/// The version byte of a watch ticket.
pub const WATCH_TICKET_VERSION: u8 = 3;
pub(crate) const WATCH_TICKET_DOMAIN: &[u8] = b"VERA20k relay watch ticket v1\0";
/// The longest delay a watch ticket carries: the longest a match runs
/// (`MAX_MATCH_SECONDS`). A finished match's records outlive its end by
/// only 300 s (`cleanup_due`), so a watcher held back longer than that
/// loses the end of the match.
pub const MAX_WATCH_DELAY_SECONDS: u32 = 24 * 60 * 60;
/// Watchers one room admits; the relay admits `MAX_WATCHERS` in all.
pub const MAX_ROOM_WATCHERS: usize = 256;
const MAX_WATCHERS: usize = 1024;
/// The `source` of a watcher's hop: none of the match's.
pub(crate) const WATCH_SOURCE: u8 = MAX_PEERS;
/// A watcher silent this long loses its place.
const WATCH_SILENCE_MS: u64 = 30_000;
/// A watcher's session after its Leave or renewal answers only Leave for
/// this many seconds: the client's minute of Leave resends, and the second
/// `now` rounds off.
const WATCH_LEAVE_S: u64 = (LEAVE_ATTEMPTS as u64 * LEAVE_INTERVAL_MS).div_ceil(1000) + 1;
/// The same for a watcher the relay dropped, which nothing tells: its game
/// keeps the route until it has heard nothing for `ABSENT_MS` and 15 s more
/// (`RELAY_ABSENT_TICKS`, src/net/udp.rs), and may start its Leave until then.
const WATCH_DROPPED_LEAVE_S: u64 = (ABSENT_MS + 15_000) / 1000 + WATCH_LEAVE_S;
/// A room notes its stream's end this often, and keeps enough of these
/// marks for the longest delay: a delayed watcher's stream grows by this
/// much at a time.
pub const WATCH_MARK_MS: u64 = 1000;
const WATCH_MARKS: usize = MAX_WATCH_DELAY_SECONDS as usize + 2;
/// Checksums every player reported alike, kept one per this many frames
/// for watchers' sync check, and how many are kept.
const WATCH_SAMPLE_FRAMES: u32 = 16 * CHECKSUM_INTERVAL;
const WATCH_SAMPLES: usize = 128;
/// A watcher's Pull answers with at most this many PACKs, and reads no
/// more records than they can hold.
const WATCH_PULL_PACKS: usize = 4;
const WATCH_PULL_RECORDS: u64 = WATCH_PULL_PACKS as u64 * u8::MAX as u64;
/// One watcher's budgets per second.
const WATCHER_PACKETS_PER_SECOND: u64 = 64;
const WATCHER_ARCHIVE_PULLS_PER_SECOND: u64 = 32;
const WATCHER_BYTES_PER_SECOND: u64 = 256 * 1024;
/// All watchers' budgets per second together.
const WATCH_PACKETS_PER_SECOND: u64 = 4096;
const WATCH_ARCHIVE_PULLS_PER_SECOND: u64 = 512;
const WATCH_BYTES_PER_SECOND: u64 = 8 * 1024 * 1024;
/// A watcher's Ready: `[0][agreement 32]`.
const WATCH_READY_LEN: usize = 1 + 32;

/// A watcher's hop ticket, signed by the lobby with the relay ticket key:
/// `[3][room 16][0][expires u64][nonce 16][watcher 16][delay_seconds u64]
/// [HMAC-SHA256 32]`, a v2 ticket's length and places under its own
/// version and signing domain, so neither verifies as the other. Relays
/// before watchers refuse any version but 2.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchTicket {
    pub room: [u8; 16],
    pub expires: u64,
    pub nonce: [u8; 16],
    /// Non-zero; one watch of one watcher, kept across renewals.
    pub watcher: [u8; 16],
    /// The relay gives this watcher no record committed less than this
    /// long ago; at most `MAX_WATCH_DELAY_SECONDS`.
    pub delay_seconds: u32,
}

impl WatchTicket {
    pub fn parse(bytes: &[u8]) -> Result<Self, RelayError> {
        if bytes.len() != TICKET_LEN || bytes[0] != WATCH_TICKET_VERSION || bytes[17] != 0 {
            return Err(RelayError::Malformed);
        }
        let delay = read_u64(bytes, 58)?;
        let ticket = Self {
            room: field(bytes, 1)?,
            expires: read_u64(bytes, 18)?,
            nonce: field(bytes, 26)?,
            watcher: field(bytes, 42)?,
            delay_seconds: u32::try_from(delay).map_err(|_| RelayError::Malformed)?,
        };
        ticket.validate()?;
        Ok(ticket)
    }

    fn validate(&self) -> Result<(), RelayError> {
        if self.watcher == [0; 16] || self.delay_seconds > MAX_WATCH_DELAY_SECONDS {
            return Err(RelayError::Malformed);
        }
        Ok(())
    }

    /// The lobby and native test clients use this same byte layout. The
    /// random nonce must come from OS cryptography.
    pub fn sign(self, server_key: &SecretKey) -> Result<[u8; TICKET_LEN], RelayError> {
        self.validate()?;
        let mut bytes = [0; TICKET_LEN];
        bytes[0] = WATCH_TICKET_VERSION;
        bytes[1..17].copy_from_slice(&self.room);
        bytes[18..26].copy_from_slice(&self.expires.to_le_bytes());
        bytes[26..42].copy_from_slice(&self.nonce);
        bytes[42..58].copy_from_slice(&self.watcher);
        bytes[58..66].copy_from_slice(&u64::from(self.delay_seconds).to_le_bytes());
        let signature = server_key
            .mac(WATCH_TICKET_DOMAIN, &bytes[..TICKET_BODY_LEN])
            .finalize()
            .into_bytes();
        bytes[TICKET_BODY_LEN..].copy_from_slice(&signature);
        Ok(bytes)
    }

    /// The hop's view of this ticket, for what a watcher's hop shares with
    /// a player's (handshake, renewal): source `WATCH_SOURCE`, the watcher
    /// as player.
    pub(crate) fn hop(&self) -> Ticket {
        Ticket {
            room: self.room,
            source: WATCH_SOURCE,
            expires: self.expires,
            nonce: self.nonce,
            player: self.watcher,
            generation: 1,
        }
    }
}

/// A watcher's hop in its room.
pub(crate) struct Watcher {
    pub(crate) ticket: Ticket,
    delay_ms: u64,
    pub(crate) key: SecretKey,
    pub(crate) session: u32,
    pub(crate) endpoint: SocketAddrV4,
    pub(crate) sent: u64,
    // For the feedback tail of its STATUS (spec §3.4).
    link: crate::link::ReaderLink,
    // Its Ready named the match's startup agreement.
    agreed: bool,
    packets: RateBudget,
    archive_pulls: RateBudget,
    outgoing: RateBudget,
}

impl Watcher {
    fn new(
        message: &Incoming<'_>,
        delay_seconds: u32,
        session: u32,
        endpoint: SocketAddrV4,
        now_ms: u64,
    ) -> Self {
        Self {
            ticket: message.ticket,
            delay_ms: u64::from(delay_seconds) * 1000,
            key: message.key.clone(),
            session,
            endpoint,
            sent: 0,
            link: crate::link::ReaderLink::new(now_ms),
            agreed: false,
            packets: RateBudget::default(),
            archive_pulls: RateBudget::default(),
            outgoing: RateBudget::default(),
        }
    }

    /// An authenticated, fresh datagram of this hop at `endpoint`.
    fn heard(
        &mut self,
        sequence: u64,
        endpoint: SocketAddrV4,
        now_ms: u64,
    ) -> Result<(), RelayError> {
        self.link.heard(sequence, now_ms)?;
        // NAT rebinding moves the hop, as a player's (spec §6.2 "换地址").
        self.endpoint = endpoint;
        Ok(())
    }

    /// One datagram on this hop, as `server_packet` sends a player's.
    fn packet(&mut self, kind: u8, payload: &[u8]) -> Result<ForwardDatagram, RelayError> {
        let sequence = crate::next_relay_sequence(self.sent)?;
        let bytes = encode_server(&self.key, kind, self.session, sequence, payload);
        self.sent = sequence;
        Ok(ForwardDatagram {
            endpoint: self.endpoint,
            bytes,
        })
    }
}

/// What all watchers together may spend each second.
#[derive(Default)]
pub(crate) struct WatchBudgets {
    packets: RateBudget,
    archive_pulls: RateBudget,
    outgoing: RateBudget,
}

/// Spends `amount` of a watcher's `own` budget and of all watchers' `all`,
/// or of neither.
fn spend(
    own: &mut RateBudget,
    own_limit: u64,
    all: &mut RateBudget,
    all_limit: u64,
    now: u64,
    amount: u64,
) -> bool {
    if !own.permits(now, amount, own_limit) || !all.permits(now, amount, all_limit) {
        return false;
    }
    own.charge(now, amount);
    all.charge(now, amount);
    true
}

/// Takes out of `room` the watchers whose ticket expired, who fell silent,
/// or whose match the relay no longer holds, each as `prune` parks its
/// session for Leave: `(session, ticket, key, until)`.
pub(crate) fn drop_watchers(
    room: &mut Room,
    now: u64,
    now_ms: u64,
) -> impl Iterator<Item = (u32, Ticket, SecretKey, u64)> + '_ {
    let held = room.ordering.is_some();
    let until = now.saturating_add(WATCH_DROPPED_LEAVE_S);
    room.watchers
        .extract_if(.., move |watcher| {
            !held
                || watcher.ticket.expires <= now
                || now_ms.saturating_sub(watcher.link.last_heard_ms) > WATCH_SILENCE_MS
        })
        .map(move |watcher| (watcher.session, watcher.ticket, watcher.key, until))
}

#[derive(Clone, Copy)]
struct WatchMark {
    at_ms: u64,
    head: u32,
    next_seq: u64,
    active: u32,
}

/// What delayed watchers may read of a room's stream.
#[derive(Default)]
pub(super) struct WatchLog {
    // The stream's end once every `WATCH_MARK_MS`, oldest first: every
    // record before `next_seq` was committed by `at_ms`.
    marks: VecDeque<WatchMark>,
    // `(frame, checksum)` of every `WATCH_SAMPLE_FRAMES`th frame that every
    // active player reported alike, oldest first.
    agreed: VecDeque<(u32, u32)>,
}

impl WatchLog {
    pub(super) fn mark(&mut self, now_ms: u64, head: u32, next_seq: u64, active: u32) {
        if self
            .marks
            .back()
            .is_some_and(|mark| now_ms < mark.at_ms.saturating_add(WATCH_MARK_MS))
        {
            return;
        }
        if self.marks.len() == WATCH_MARKS {
            self.marks.pop_front();
        }
        self.marks.push_back(WatchMark {
            at_ms: now_ms,
            head,
            next_seq,
            active,
        });
    }

    pub(super) fn agree(&mut self, frame: u32, checksum: u32) {
        if frame % WATCH_SAMPLE_FRAMES != 0
            || self.agreed.back().is_some_and(|&(last, _)| last >= frame)
        {
            return;
        }
        if self.agreed.len() == WATCH_SAMPLES {
            self.agreed.pop_front();
        }
        self.agreed.push_back((frame, checksum));
    }

    pub(super) fn memory(&self) -> usize {
        self.marks.capacity() * std::mem::size_of::<WatchMark>()
            + self.agreed.capacity() * std::mem::size_of::<(u32, u32)>()
    }
}

/// The end of the stream a watcher may read now.
#[derive(Clone, Copy)]
struct WatchView {
    head: u32,
    next_seq: u64,
    active: u32,
}

impl OrderingRoom {
    /// Whether a watcher may join: the match started and its whole stream
    /// is on disk.
    fn watchable(&self) -> bool {
        self.starts_at.is_some() && self.journal.is_some() && !self.storage_failed
    }

    /// The stream's end `delay_ms` ago: the newest mark at least that old,
    /// or nothing before the first.
    fn watch_view(&self, delay_ms: u64, now_ms: u64) -> WatchView {
        if delay_ms == 0 {
            return WatchView {
                head: self.head,
                next_seq: self.next_seq,
                active: self.active,
            };
        }
        self.watch
            .marks
            .iter()
            .rev()
            .find(|mark| mark.at_ms.saturating_add(delay_ms) <= now_ms)
            .map_or(
                WatchView {
                    head: 0,
                    next_seq: self.oldest_seq(),
                    active: self.certificate.sources_mask,
                },
                |mark| WatchView {
                    head: mark.head,
                    next_seq: mark.next_seq,
                    active: mark.active,
                },
            )
    }

    /// A watcher's STATUS: the stream at `view`, the hop feedback, then the
    /// newest agreed sample of a frame within `view`:
    /// `[sample_frame varint][checksum u32]`, absent without one.
    fn watch_status(&self, view: WatchView, feedback: HopFeedback, now_ms: u64) -> Vec<u8> {
        let mut status = self.status_of(view.head, view.next_seq, view.active, now_ms);
        crate::link::append_feedback(&mut status, feedback);
        if let Some(&(frame, checksum)) = self
            .watch
            .agreed
            .iter()
            .rev()
            .find(|(frame, _)| *frame <= view.head)
        {
            varint::write(frame.into(), &mut status);
            status.extend_from_slice(&checksum.to_le_bytes());
        }
        status
    }
}

/// A watcher STATUS's sample after its feedback tail, if any.
pub(super) fn read_sample(reader: &mut Reader<'_>) -> Option<(u32, u32)> {
    if reader.is_empty() {
        return None;
    }
    Some((reader.varint_u32().ok()?, reader.u32().ok()?))
}

impl RelayServer {
    /// A watcher's HELLO or CONFIRM, its ticket already verified
    /// (`handshake_incoming`). Handshakes spend the players' handshake
    /// budgets: the watcher's own first, as a player's.
    pub(crate) fn watch_handshake(
        &mut self,
        source: SocketAddrV4,
        message: &Incoming<'_>,
        now: u64,
        now_ms: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        // A watcher carries no federation roster.
        if message.kind == CONFIRM && message.payload.len() != COOKIE_LEN {
            return Err(RelayError::Malformed);
        }
        self.allow_handshake(message, now)?;
        if message.kind == HELLO {
            return self.challenge_budgeted(source, message, now);
        }
        self.verify_cookie(source, message, now)?;
        self.watch_confirm(source, message, now, now_ms)
    }

    /// Admits a watcher to a started match whose whole stream is on disk,
    /// or renews its hop: a newer ticket gets a new session and keeps the
    /// old hop's agreement and budgets. The client moves to the new session
    /// on its READY, so a watcher that quits before then leaves from the
    /// old one, which is parked for that Leave.
    fn watch_confirm(
        &mut self,
        source: SocketAddrV4,
        message: &Incoming<'_>,
        now: u64,
        now_ms: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        let watch = WatchTicket::parse(message.ticket_bytes.expect("a handshake"))?;
        let watchers: usize = self.rooms.values().map(|room| room.watchers.len()).sum();
        let room = self
            .rooms
            .get(&watch.room)
            .ok_or(RelayError::Unregistered)?;
        if !room.ordering.as_ref().is_some_and(OrderingRoom::watchable) {
            return Err(RelayError::Unregistered);
        }
        let existing = room
            .watchers
            .iter()
            .position(|watcher| watcher.ticket.player == watch.watcher);
        let delay_ms = u64::from(watch.delay_seconds) * 1000;
        let same_lease = existing.is_some_and(|index| {
            room.watchers[index].ticket == message.ticket
                && room.watchers[index].delay_ms == delay_ms
        });
        match existing {
            Some(index) if same_lease => {
                let watcher = &room.watchers[index];
                if message.sequence <= watcher.link.highest_received() && watcher.endpoint != source
                {
                    return Err(RelayError::Replay);
                }
            }
            Some(index) if message.ticket.expires <= room.watchers[index].ticket.expires => {
                return Err(RelayError::Authentication);
            }
            Some(_) => {}
            None if room.watchers.len() >= MAX_ROOM_WATCHERS || watchers >= MAX_WATCHERS => {
                return Err(RelayError::Full);
            }
            None => {}
        }
        let cost = envelope::OVERHEAD as u64;
        if !self
            .watching
            .outgoing
            .permits(now, cost, WATCH_BYTES_PER_SECOND)
        {
            return Err(RelayError::RateLimited);
        }
        let session = if same_lease {
            None
        } else {
            Some(self.allocate_session()?)
        };
        // Checked above; allocating a session leaves the rooms alone.
        let room = self
            .rooms
            .get_mut(&watch.room)
            .ok_or(RelayError::Unregistered)?;
        let mut retired = None;
        let reply = match (existing, session) {
            (Some(index), None) => {
                let watcher = &mut room.watchers[index];
                watcher.heard(message.sequence, source, now_ms)?;
                watcher.packet(READY, &[])?
            }
            (_, Some(session)) => {
                let mut watcher =
                    Watcher::new(message, watch.delay_seconds, session, source, now_ms);
                watcher.heard(message.sequence, source, now_ms)?;
                let reply = watcher.packet(READY, &[])?;
                if let Some(index) = existing {
                    let old = std::mem::replace(&mut room.watchers[index], watcher);
                    let new = &mut room.watchers[index];
                    new.agreed = old.agreed;
                    new.packets = old.packets;
                    new.archive_pulls = old.archive_pulls;
                    new.outgoing = old.outgoing;
                    retired = Some(old);
                } else {
                    room.watchers.push(watcher);
                }
                self.sessions.insert(
                    session,
                    SessionSlot {
                        hop: SessionHop::Watcher { room: watch.room },
                        failure_window: 0,
                        failures: 0,
                    },
                );
                reply
            }
            (None, None) => unreachable!("a new watcher gets a session"),
        };
        if let Some(old) = retired {
            let until = now.saturating_add(WATCH_LEAVE_S);
            self.park_session(old.session, old.ticket, old.key, until);
        }
        self.watching.outgoing.charge(now, cost);
        Ok(vec![reply])
    }

    /// A watcher's Leave `[3]`, on its live session or a parked one: the
    /// first takes the watcher out of its room, writing nothing, and parks
    /// its session; each is answered with LEAVE_ACK echoing its sequence.
    /// As a player's (`ordering_leave`), it runs no replay check, since the
    /// client resends the same bytes, and spends the watcher's own
    /// handshake budget before the relay's.
    pub(crate) fn watch_leave(
        &mut self,
        source: SocketAddrV4,
        message: &Incoming<'_>,
        now: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        let cost = envelope::OVERHEAD as u64;
        if !self.player_handshakes(&message.ticket.player).permits(
            now,
            1,
            PLAYER_HANDSHAKES_PER_SECOND,
        ) || !self
            .watching
            .outgoing
            .permits(now, cost, WATCH_BYTES_PER_SECOND)
        {
            return Err(RelayError::RateLimited);
        }
        self.charge_player_handshake(message.ticket.player, now);
        self.watching.outgoing.charge(now, cost);
        if let Some(room) = self.rooms.get_mut(&message.ticket.room)
            && let Some(index) = room
                .watchers
                .iter()
                .position(|watcher| watcher.session == message.session)
        {
            let watcher = room.watchers.swap_remove(index);
            let until = now.saturating_add(WATCH_LEAVE_S);
            self.park_session(watcher.session, watcher.ticket, watcher.key, until);
        }
        Ok(vec![ForwardDatagram {
            endpoint: source,
            bytes: encode_server(
                &message.key,
                LEAVE_ACK,
                message.session,
                message.sequence,
                &[],
            ),
        }])
    }

    /// A datagram `session_incoming` authenticated on a watcher's live
    /// session other than Leave: PING, Ready or Pull. Anything else, DATA
    /// and UPLINK included, is refused.
    pub(crate) fn watch_datagram(
        &mut self,
        source: SocketAddrV4,
        message: &Incoming<'_>,
        now: u64,
        now_ms: u64,
    ) -> Result<Vec<ForwardDatagram>, RelayError> {
        let (room_id, sequence, payload) = (message.ticket.room, message.sequence, message.payload);
        // `session_payload` framed it: PING empty, CONTROL not.
        let request = match message.kind {
            PING => None,
            CONTROL => payload.first().copied(),
            _ => return Err(RelayError::Malformed),
        };
        if request.is_some() {
            self.advance_ordering_room(room_id, now_ms);
        }
        let Room {
            watchers, ordering, ..
        } = self
            .rooms
            .get_mut(&room_id)
            .ok_or(RelayError::Unregistered)?;
        let watcher = watchers
            .iter_mut()
            .find(|watcher| watcher.session == message.session)
            .ok_or(RelayError::Unregistered)?;
        watcher.heard(sequence, source, now_ms)?;
        let budgets = &mut self.watching;
        if !spend(
            &mut watcher.packets,
            WATCHER_PACKETS_PER_SECOND,
            &mut budgets.packets,
            WATCH_PACKETS_PER_SECOND,
            now,
            1,
        ) {
            return Err(RelayError::RateLimited);
        }
        let Some(request) = request else {
            // A keepalive, answered as a player's (spec §6.1).
            if !spend(
                &mut watcher.outgoing,
                WATCHER_BYTES_PER_SECOND,
                &mut budgets.outgoing,
                WATCH_BYTES_PER_SECOND,
                now,
                envelope::OVERHEAD as u64,
            ) {
                return Err(RelayError::RateLimited);
            }
            return Ok(vec![watcher.packet(READY, &[])?]);
        };
        let feedback = watcher.link.feedback(now_ms);
        let failure = ordering
            .as_ref()
            .and_then(|room| room.federation_failure_reply(now_ms));
        let replies = match (request, ordering.as_mut(), failure) {
            (_, _, Some(reply)) => reply,
            (READY_REQUEST | PULL_REQUEST, None, _) => error(OrderError::Inactive, 0),
            (READY_REQUEST, Some(ordering), _) if payload.len() == WATCH_READY_LEN => {
                if payload[1..] != ordering.agreement {
                    error(OrderError::AgreementMismatch, 0)
                } else {
                    watcher.agreed = true;
                    let view = ordering.watch_view(watcher.delay_ms, now_ms);
                    vec![ordering.watch_status(view, feedback, now_ms)]
                }
            }
            (PULL_REQUEST, Some(ordering), _) => {
                // `[2][next varint]` (compaction spec §7.4).
                let mut reader = Reader(&payload[1..]);
                let cursor = reader.varint()?;
                if !reader.is_empty() {
                    return Err(RelayError::Malformed);
                }
                let view = ordering.watch_view(watcher.delay_ms, now_ms);
                if !watcher.agreed {
                    error(OrderError::NotConfigured, 0)
                } else if cursor > view.next_seq {
                    error(OrderError::InvalidCursor, view.next_seq)
                } else {
                    let count = (view.next_seq - cursor).min(WATCH_PULL_RECORDS);
                    // A read from disk is paid for before it happens, even
                    // if its reply then exceeds an output budget.
                    if count > 0
                        && cursor < ordering.hot_oldest_seq()
                        && !spend(
                            &mut watcher.archive_pulls,
                            WATCHER_ARCHIVE_PULLS_PER_SECOND,
                            &mut budgets.archive_pulls,
                            WATCH_ARCHIVE_PULLS_PER_SECOND,
                            now,
                            1,
                        )
                    {
                        return Err(RelayError::RateLimited);
                    }
                    match ordering.history_page(cursor, count as usize) {
                        Ok(records) => {
                            let mut replies = vec![ordering.watch_status(view, feedback, now_ms)];
                            replies.extend(
                                pack::encode_pack(&records, WATCH_PULL_PACKS)
                                    .into_iter()
                                    .map(|(pack, _)| pack),
                            );
                            replies
                        }
                        Err((code, expected)) => error(code, expected),
                    }
                }
            }
            _ => return Err(RelayError::Malformed),
        };
        let cost = replies
            .iter()
            .map(|payload| envelope::OVERHEAD + payload.len())
            .sum::<usize>() as u64;
        if !spend(
            &mut watcher.outgoing,
            WATCHER_BYTES_PER_SECOND,
            &mut budgets.outgoing,
            WATCH_BYTES_PER_SECOND,
            now,
            cost,
        ) {
            return Err(RelayError::RateLimited);
        }
        replies
            .iter()
            .map(|payload| watcher.packet(CONTROL, payload))
            .collect()
    }
}

impl ClientSession {
    /// Whether this hop watches a match (a `WatchTicket`) instead of playing
    /// in it. A watcher only pulls: it sends no UPLINK, batch, checksum,
    /// pace, Pause or DATA.
    pub fn is_watching(&self) -> bool {
        self.ticket.source == WATCH_SOURCE
    }

    /// How far behind the players a watcher's relay holds the stream (its
    /// ticket's delay); `None` for a player's hop.
    pub fn watch_delay_seconds(&self) -> Option<u32> {
        WatchTicket::parse(&self.ticket_bytes)
            .ok()
            .map(|ticket| ticket.delay_seconds)
    }

    /// A watcher's Ready (request 0, README "Watchers") at `now_ms`:
    /// `[0][agreement 32]`, the startup agreement of the match it watches,
    /// which the relay compares with the players'. `certificate` is the
    /// match's ordering certificate as the lobby handed it on; the watcher
    /// reads its rate from it and does not send it. The relay answers with
    /// a STATUS of the stream's delayed end, after which the watcher pulls
    /// with `pull_ordering_at` and leaves with `leave_ordering_at`.
    pub fn configure_watch_at(
        &mut self,
        certificate: &[u8],
        agreement: [u8; 32],
        now_ms: u64,
        out: &mut Vec<u8>,
    ) -> Result<(), RelayError> {
        self.clock_ms = now_ms;
        if !self.ready {
            return Err(RelayError::Unregistered);
        }
        if !self.is_watching() {
            return Err(RelayError::Authentication);
        }
        self.join_ordering(certificate, agreement)?;
        let mut payload = vec![READY_REQUEST];
        payload.extend_from_slice(&agreement);
        self.encode(CONTROL, &payload, out)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{
        TestDirectory, certificate, client, configure, connect, endpoint, events, pull, unix,
    };
    use super::*;

    const ROOM: [u8; 16] = [5; 16];

    /// A started, journaled match of sources 0 and 1 at 45 Hz, its first
    /// frame due at 200 ms, its clock advanced each second to `now_ms`.
    fn running_match(
        directory: &TestDirectory,
        now_ms: u64,
    ) -> (RelayServer, ClientSession, ClientSession) {
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        let cert = certificate(&key, 3, 1300);
        for player in [&mut first, &mut second] {
            connect(&mut server, player, 0);
            configure(&mut server, player, &cert, 0);
        }
        for at in (1000..=now_ms).step_by(1000) {
            server.advance_clock(at, unix(at));
        }
        (server, first, second)
    }

    fn watcher(server: &RelayServer, room: [u8; 16], id: u8, delay_seconds: u32) -> ClientSession {
        let ticket = WatchTicket {
            room,
            expires: 1200,
            nonce: [id; 16],
            watcher: [id; 16],
            delay_seconds,
        }
        .sign(&server.key)
        .unwrap();
        let key = *derive_packet_key(&server.key, &ticket).expose_for_https();
        ClientSession::new(&ticket, key, [0; 32]).unwrap()
    }

    fn watch_ready(
        server: &mut RelayServer,
        watcher: &mut ClientSession,
        agreement: [u8; 32],
        now_ms: u64,
    ) -> Vec<OrderEvent> {
        let cert = certificate(&server.key, 3, 1300);
        let mut bytes = Vec::new();
        watcher
            .configure_watch_at(&cert, agreement, now_ms, &mut bytes)
            .unwrap();
        events(server, watcher, &bytes, now_ms)
    }

    fn leave(server: &mut RelayServer, watcher: &mut ClientSession, now_ms: u64) {
        let mut bytes = Vec::new();
        watcher.leave_ordering_at(now_ms, &mut bytes).unwrap();
        assert!(matches!(
            events(server, watcher, &bytes, now_ms)[..],
            [OrderEvent::Left]
        ));
    }

    #[test]
    fn a_watcher_joins_a_started_match_and_reads_only_its_delayed_end() {
        let directory = TestDirectory::new();
        let (mut server, mut first, mut second) = running_match(&directory, 6000);
        for player in [&mut first, &mut second] {
            let mut bytes = Vec::new();
            player
                .queue_ordering_checksums(&[(240, 0xABCD)], 6000)
                .unwrap();
            player.test_uplink(None, 6000, &mut bytes).unwrap();
            assert!(events(&mut server, player, &bytes, 6000).is_empty());
        }
        for at in [7000, 8000, 9000] {
            server.advance_clock(at, unix(at));
        }
        // No match, no watcher.
        let mut lost = watcher(&server, [6; 16], 43, 2);
        let mut bytes = Vec::new();
        lost.probe(&mut bytes).unwrap();
        let challenge = server
            .process_at(endpoint(&lost), &bytes, unix(9000), 9000)
            .unwrap()
            .remove(0);
        let ServerEvent::Challenge(cookie) = lost.receive(&challenge.bytes).unwrap() else {
            panic!("challenge");
        };
        lost.confirm(cookie, &mut bytes).unwrap();
        assert_eq!(
            server
                .process_at(endpoint(&lost), &bytes, unix(9000), 9000)
                .err(),
            Some(RelayError::Unregistered)
        );

        let mut late = watcher(&server, ROOM, 41, 2);
        connect(&mut server, &mut late, 9000);
        let replies = watch_ready(&mut server, &mut late, [7; 32], 9000);
        let [OrderEvent::Status(status)] = &replies[..] else {
            panic!("status");
        };
        // Two seconds behind: no frame due after 7000 ms.
        let due_by = |now_ms: u64| ((now_ms - 200) * 45 / 1000) as u32;
        assert!(status.head_frame >= 240 && status.head_frame <= due_by(7000));
        assert!(server.rooms[&ROOM].ordering.as_ref().unwrap().head > due_by(8000));
        assert_eq!(status.watch_sample, Some((240, 0xABCD)));
        let (head, edge) = (status.head_frame, status.next_seq);
        let mut cursor = 1;
        while cursor < edge {
            let replies = pull(&mut server, &mut late, cursor, 9000);
            let OrderEvent::Status(again) = &replies[0] else {
                panic!("status");
            };
            assert_eq!(again.next_seq, edge);
            for event in &replies[1..] {
                let OrderEvent::Entry(OrderEntry::FrameEnd { sequence, frame }) = *event else {
                    panic!("frame ends only");
                };
                assert_eq!(sequence, cursor);
                assert!(frame <= head);
                cursor += 1;
            }
        }
        assert_eq!(cursor, edge);
        assert!(matches!(
            pull(&mut server, &mut late, edge + 1, 9000)[..],
            [OrderEvent::Error {
                code: OrderError::InvalidCursor,
                ..
            }]
        ));
        // A live watcher reads up to the players' end.
        let mut live = watcher(&server, ROOM, 42, 0);
        connect(&mut server, &mut live, 9000);
        let replies = watch_ready(&mut server, &mut live, [7; 32], 9000);
        let [OrderEvent::Status(status)] = &replies[..] else {
            panic!("status");
        };
        assert_eq!(
            status.head_frame,
            server.rooms[&ROOM].ordering.as_ref().unwrap().head
        );
    }

    #[test]
    fn watchers_leave_the_match_and_its_stream_as_they_were() {
        let directory = TestDirectory::new();
        let (mut server, _first, _second) = running_match(&directory, 3000);
        let ordering = |server: &RelayServer| {
            let ordering = server.rooms[&ROOM].ordering.as_ref().unwrap();
            (
                ordering.ready,
                ordering.active,
                ordering.head,
                ordering.next_seq,
            )
        };
        let before = (ordering(&server), server.ordering_history(ROOM));

        // Another world is refused, and every Pull before an agreement.
        let mut stranger = watcher(&server, ROOM, 43, 0);
        connect(&mut server, &mut stranger, 3000);
        assert!(matches!(
            watch_ready(&mut server, &mut stranger, [8; 32], 3000)[..],
            [OrderEvent::Error {
                code: OrderError::AgreementMismatch,
                ..
            }]
        ));
        assert!(matches!(
            pull(&mut server, &mut stranger, 1, 3000)[..],
            [OrderEvent::Error {
                code: OrderError::NotConfigured,
                ..
            }]
        ));
        leave(&mut server, &mut stranger, 3000);

        let mut watching = watcher(&server, ROOM, 41, 0);
        connect(&mut server, &mut watching, 3000);
        assert!(matches!(
            watch_ready(&mut server, &mut watching, [7; 32], 3000)[..],
            [OrderEvent::Status(_)]
        ));
        assert!(pull(&mut server, &mut watching, 1, 3000).len() > 1);
        // It sends no batch or UPLINK, and the relay takes none.
        assert!(watching.is_watching());
        assert!(watching.seal_ordering_batch(&[1]).is_err());
        let mut bytes = Vec::new();
        assert!(!watching.poll_uplink(3000, &mut bytes));
        watching
            .encode(CONTROL, &[crate::link::UPLINK_REQUEST, 0], &mut bytes)
            .unwrap();
        assert_eq!(
            server
                .process_at(endpoint(&watching), &bytes, unix(3000), 3000)
                .err(),
            Some(RelayError::Malformed)
        );
        leave(&mut server, &mut watching, 3000);

        assert!(server.rooms[&ROOM].watchers.is_empty());
        assert!(
            server
                .session_table()
                .iter()
                .all(|entry| entry.source != WATCH_SOURCE || entry.leaving)
        );
        assert_eq!(before, (ordering(&server), server.ordering_history(ROOM)));
    }

    /// A watcher whose LEAVE_ACK is lost resends the same Leave until it is
    /// answered, for about a minute; its session takes nothing else
    /// meanwhile, and is released once the client stops.
    #[test]
    fn a_watchers_resent_leave_is_answered_until_the_client_stops() {
        let directory = TestDirectory::new();
        let (mut server, _first, _second) = running_match(&directory, 3000);
        let mut watching = watcher(&server, ROOM, 41, 0);
        connect(&mut server, &mut watching, 3000);
        watch_ready(&mut server, &mut watching, [7; 32], 3000);
        let mut leave = Vec::new();
        watching.leave_ordering_at(3000, &mut leave).unwrap();
        let from = endpoint(&watching);
        let lost = server.process_at(from, &leave, unix(3000), 3000).unwrap();
        assert_eq!(lost.len(), 1);
        assert!(server.rooms[&ROOM].watchers.is_empty());
        let mut ping = Vec::new();
        watching.encode(PING, &[], &mut ping).unwrap();
        assert_eq!(
            server.process_at(from, &ping, unix(3000), 3000).err(),
            Some(RelayError::Unregistered)
        );
        let last_ms = 3000 + u64::from(LEAVE_ATTEMPTS) * LEAVE_INTERVAL_MS;
        assert!(matches!(
            events(&mut server, &mut watching, &leave, last_ms)[..],
            [OrderEvent::Left]
        ));
        let gone_ms = 3000 + WATCH_LEAVE_S * 1000;
        assert_eq!(
            server
                .process_at(from, &leave, unix(gone_ms), gone_ms)
                .err(),
            Some(RelayError::Unregistered)
        );
        assert!(!server.sessions.contains_key(&watching.session().unwrap()));
    }

    /// A watcher that quits before its renewal's READY arrives leaves from
    /// the session the renewal replaced, which still answers it.
    #[test]
    fn a_watcher_quitting_before_its_renewal_ready_still_leaves() {
        let directory = TestDirectory::new();
        let (mut server, _first, _second) = running_match(&directory, 3000);
        let mut watching = watcher(&server, ROOM, 41, 0);
        connect(&mut server, &mut watching, 3000);
        watch_ready(&mut server, &mut watching, [7; 32], 3000);
        let ticket = WatchTicket {
            room: ROOM,
            expires: 1250,
            nonce: [42; 16],
            watcher: [41; 16],
            delay_seconds: 0,
        }
        .sign(&server.key)
        .unwrap();
        let key = *derive_packet_key(&server.key, &ticket).expose_for_https();
        let mut renewal = watching.renewal(&ticket, key, [0; 32]).unwrap();
        // The relay takes the renewal; `watching` never sees its READY.
        connect(&mut server, &mut renewal, 3000);
        leave(&mut server, &mut watching, 3000);
    }

    /// Nothing tells a watcher the relay dropped (here for silence): its
    /// Leave is answered for as long as its game may still send one.
    #[test]
    fn a_dropped_watchers_leave_is_answered_until_its_game_gives_up() {
        let directory = TestDirectory::new();
        let (mut server, _first, _second) = running_match(&directory, 3000);
        let mut watching = watcher(&server, ROOM, 41, 0);
        connect(&mut server, &mut watching, 3000);
        watch_ready(&mut server, &mut watching, [7; 32], 3000);
        let dropped_ms = 3000 + WATCH_SILENCE_MS + 1000;
        server.prune(unix(dropped_ms), dropped_ms);
        assert!(server.rooms[&ROOM].watchers.is_empty());
        let gone_ms = dropped_ms + WATCH_DROPPED_LEAVE_S * 1000;
        leave(&mut server, &mut watching, gone_ms - 1000);
        let mut bytes = Vec::new();
        watching.leave_ordering_at(gone_ms, &mut bytes).unwrap();
        assert_eq!(
            server
                .process_at(endpoint(&watching), &bytes, unix(gone_ms), gone_ms)
                .err(),
            Some(RelayError::Unregistered)
        );
    }

    /// A lobby may grant a watch before the relay has started and journaled
    /// the match: the watcher's CONFIRM goes unanswered (`Unregistered`),
    /// and the same session's next handshake gets in once the match runs.
    #[test]
    fn a_watcher_too_early_for_the_match_gets_in_once_it_runs() {
        let directory = TestDirectory::new();
        let key = SecretKey::new([73; 32]);
        let mut server = RelayServer::with_journal(key.clone(), 2, &directory.0).unwrap();
        let (mut first, mut second) = (client(&key, 0), client(&key, 1));
        for player in [&mut first, &mut second] {
            connect(&mut server, player, 0);
        }
        let mut early = watcher(&server, ROOM, 41, 0);
        let mut bytes = Vec::new();
        early.probe(&mut bytes).unwrap();
        let challenge = server
            .process_at(endpoint(&early), &bytes, unix(0), 0)
            .unwrap()
            .remove(0);
        let ServerEvent::Challenge(cookie) = early.receive(&challenge.bytes).unwrap() else {
            panic!("challenge");
        };
        early.confirm(cookie, &mut bytes).unwrap();
        assert_eq!(
            server
                .process_at(endpoint(&early), &bytes, unix(0), 0)
                .err(),
            Some(RelayError::Unregistered)
        );
        let cert = certificate(&key, 3, 1300);
        for player in [&mut first, &mut second] {
            configure(&mut server, player, &cert, 0);
        }
        for at in (1000..=3000).step_by(1000) {
            server.advance_clock(at, unix(at));
        }
        connect(&mut server, &mut early, 3000);
        assert!(matches!(
            watch_ready(&mut server, &mut early, [7; 32], 3000)[..],
            [OrderEvent::Status(_)]
        ));
    }

    /// A watch may be held back as long as a match runs, and no longer.
    #[test]
    fn a_watch_ticket_may_delay_up_to_a_whole_match() {
        let ticket = |delay_seconds| WatchTicket {
            room: ROOM,
            expires: 1200,
            nonce: [41; 16],
            watcher: [41; 16],
            delay_seconds,
        };
        let key = SecretKey::new([73; 32]);
        let day = 24 * 60 * 60;
        let signed = ticket(day).sign(&key).unwrap();
        assert_eq!(WatchTicket::parse(&signed), Ok(ticket(day)));
        assert_eq!(
            ticket(day + 1).sign(&key).err(),
            Some(RelayError::Malformed)
        );
    }

    /// The lobby's Go `relay.Signer.IssueWatch` signs this same vector
    /// (vera20k-lobby-server `relay/watch_test.go`): both sides must keep
    /// these bytes.
    #[test]
    fn the_lobby_watch_ticket_vector_matches_these_bytes() {
        fn range<const N: usize>(start: u8) -> [u8; N] {
            std::array::from_fn(|index| start + index as u8)
        }
        let key = SecretKey::new(range(0x00));
        let ticket = WatchTicket {
            room: range(0x10),
            expires: 1_900_000_000,
            nonce: range(0x20),
            watcher: range(0x30),
            delay_seconds: 0,
        };
        let signed = ticket.sign(&key).unwrap();
        let hex = |bytes: &[u8]| {
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        assert_eq!(
            hex(&signed),
            "03101112131415161718191a1b1c1d1e1f0000b33f7100000000202122232425262728292a2b2c2d2e2f\
             303132333435363738393a3b3c3d3e3f00000000000000001b89a25d44deaa2751154d7b55366618\
             14c547ca1cea199b68d049de4d9962c4"
        );
        assert_eq!(
            hex(derive_packet_key(&key, &signed).expose_for_https()),
            "2a61c73a0474c855a50fd7767040d22e70b2b1e8ba791b447e319d854c30fdfa"
        );
        assert_eq!(WatchTicket::parse(&signed), Ok(ticket));
    }

    #[test]
    fn a_watchers_wrong_tag_counts_on_its_session_as_a_players_does() {
        // Compaction spec §6.4 item 2: a wrong tag drops only its datagram
        // and counts toward its session's alert.
        let directory = TestDirectory::new();
        let (mut server, _first, _second) = running_match(&directory, 3000);
        let mut watching = watcher(&server, ROOM, 41, 0);
        connect(&mut server, &mut watching, 3000);
        let (from, session) = (endpoint(&watching), watching.session().unwrap());
        let mut bytes = Vec::new();
        watching.encode(PING, &[], &mut bytes).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        assert_eq!(
            server.process_at(from, &bytes, unix(3000), 3000).err(),
            Some(RelayError::Authentication)
        );
        assert_eq!(server.sessions[&session].failures, 1);
    }
}
