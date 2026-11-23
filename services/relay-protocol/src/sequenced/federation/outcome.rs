//! A marked federation reports its own terminal ordering verdict to the lobby.
//! Ordinary player departures, including removal of a silent player by their
//! healthy home relay, stay in the command stream. Only an absent relay link
//! in a started marked match, or a peer that already proved that failure, voids
//! it.

use super::*;
use crate::link::array_at;
use sha2::Digest;

const OUTCOME_DOMAIN: &[u8] = b"VERA20k relay federation outcome v1\0";
const OUTCOME_AGE: u64 = 7 * 24 * 60 * 60;
const BODY_LEN: usize = 62;
// Usually the reporter acknowledges the disk transaction on the next pass.
// A full or failed spool must not hold players behind backpressure indefinitely.
const LOCAL_HANDOFF_MS: u64 = 5_000;
/// Inactive's otherwise-zero expected cursor. Released games already stop on
/// Inactive; current games can also tell the player why this match ended.
pub const FEDERATION_LOST: u64 = 1;

/// Immutable signed VFO1 body for the lobby. Never log its bytes.
#[derive(Clone)]
pub struct FederationOutcome([u8; BODY_LEN + MAC_LEN]);

impl FederationOutcome {
    fn sign(federation: &Federation, key: &SecretKey, failed: bool, now: u64) -> Self {
        let mut bytes = [0; BODY_LEN + MAC_LEN];
        bytes[..4].copy_from_slice(b"VFO1");
        bytes[4..20].copy_from_slice(&federation.room);
        bytes[20..52].copy_from_slice(&Sha256::digest(&federation.roster));
        bytes[52] = federation.own;
        bytes[53] = u8::from(failed);
        bytes[54..BODY_LEN].copy_from_slice(&now.to_le_bytes());
        let tag = key.mac(OUTCOME_DOMAIN, &bytes[..BODY_LEN]).finalize();
        bytes[BODY_LEN..].copy_from_slice(&tag.into_bytes());
        Self(bytes)
    }

    /// Reads the local spool's fixed envelope. The lobby verifies the MAC and
    /// the exact stored roster before accepting any outcome.
    pub fn from_bytes(bytes: [u8; BODY_LEN + MAC_LEN]) -> Option<Self> {
        (&bytes[..4] == b"VFO1" && usize::from(bytes[52]) < MAX_FEDERATED_RELAYS && bytes[53] <= 1)
            .then_some(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; BODY_LEN + MAC_LEN] {
        &self.0
    }
    pub fn room(&self) -> [u8; 16] {
        array_at::<4, 16, _>(&self.0)
    }
    pub fn issued_at(&self) -> u64 {
        u64::from_le_bytes(array_at::<54, 8, _>(&self.0))
    }
    pub fn expires_at(&self) -> u64 {
        self.issued_at().saturating_add(OUTCOME_AGE)
    }
    fn failed(&self) -> bool {
        self.0[53] == 1
    }
}

pub(in super::super) struct Terminal {
    receipt: FederationOutcome,
    saved: bool,
}

impl Terminal {
    pub(in super::super) fn into_unsaved(self) -> Option<FederationOutcome> {
        (!self.saved).then_some(self.receipt)
    }
}

impl OrderingRoom {
    pub(in super::super) fn federation_failed(&self) -> bool {
        self.terminal
            .as_ref()
            .is_some_and(|terminal| terminal.receipt.failed())
    }

    /// A relay waiting on a failed disk must not keep its peers' in-match
    /// grace alive. Only a durable proof can publish the affirmative failure bit.
    pub(super) fn adjudicated_link_flags(&self, now_ms: u64) -> u8 {
        if self.starts_at.is_none_or(|start| start > now_ms) {
            return 0;
        }
        match self
            .terminal
            .as_ref()
            .filter(|terminal| terminal.receipt.failed())
        {
            Some(terminal) if terminal.saved => MATCH_STARTED | MATCH_FAILED,
            Some(_) => 0,
            None => MATCH_STARTED,
        }
    }

    pub(in super::super) fn federation_failure_reply(&self, now_ms: u64) -> Option<Vec<Vec<u8>>> {
        let terminal = self.terminal.as_ref()?;
        if !terminal.receipt.failed() {
            return None;
        }
        Some(if terminal.saved {
            error(OrderError::Inactive, FEDERATION_LOST)
        } else if self
            .finished_at
            .is_some_and(|at| now_ms.saturating_sub(at) >= LOCAL_HANDOFF_MS)
        {
            // Players can return to the lobby. Only a saved receipt can
            // decide the ladder; failure to save is ordinary unavailability.
            error(OrderError::Inactive, 0)
        } else {
            error(OrderError::Backpressure, 0)
        })
    }
}

impl Federation {
    pub(super) fn settle(
        &self,
        ordering: &mut OrderingRoom,
        key: &SecretKey,
        now_ms: u64,
        now: u64,
    ) {
        if !self.adjudicated || ordering.terminal.is_some() || ordering.storage_failed {
            return;
        }
        let Some(start) = ordering.starts_at.filter(|start| *start <= now_ms) else {
            return;
        };
        // A room already through every departure (or the game's final frame)
        // keeps its ordinary result even after the other relays go away.
        let closed = ordering.finished_at.is_some();
        let failed = !closed
            && (self.peer_failed
                || self.links.iter().any(|link| {
                    let heard = link.heard_started.unwrap_or(start).max(start);
                    now_ms.saturating_sub(heard) >= ABSENT_MS
                }));
        if !closed && !failed {
            return;
        }
        if failed {
            // No departure enters the journal. End all readers as a failed
            // connection after durable handoff; never invent another frame.
            ordering.abandon(now_ms);
        }
        ordering.terminal = Some(Terminal {
            receipt: FederationOutcome::sign(self, key, failed, now),
            saved: false,
        });
    }
}

impl RelayServer {
    /// Activate only after the binary has opened its durable outcome spool.
    pub fn enable_federation_outcomes(&mut self) {
        self.federation_outcomes_enabled = true;
    }

    /// The binary must durably save each immutable receipt before acknowledging
    /// it. Snapshots may be retried. A match has at most one receipt per relay.
    pub fn pending_federation_outcomes(&self) -> Vec<FederationOutcome> {
        self.rooms
            .values()
            .filter_map(|room| room.ordering.as_ref()?.terminal.as_ref())
            .filter(|terminal| !terminal.saved)
            .map(|terminal| &terminal.receipt)
            .chain(self.retired_outcomes.values())
            .cloned()
            .collect()
    }

    pub fn federation_outcome_saved(&mut self, room: [u8; 16]) {
        if let Some(terminal) = self
            .rooms
            .get_mut(&room)
            .and_then(|room| room.ordering.as_mut())
            .and_then(|ordering| ordering.terminal.as_mut())
        {
            terminal.saved = true;
        } else {
            self.retired_outcomes.remove(&room);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sequenced::tests::{
        TestDirectory, certificate, client, connect, endpoint, events, pull, renew, unix,
    };

    // A real three-way handshake and the production Ready, frame and link
    // handlers. One test disconnects only one direction of a single link.
    struct Match {
        servers: [RelayServer; 3],
        clients: [ClientSession; 3],
        addresses: [SocketAddrV4; 3],
        now: u64,
    }

    impl Match {
        fn new() -> Self {
            Self::build(None)
        }

        fn build(journals: Option<&[TestDirectory; 3]>) -> Self {
            let room = [5; 16];
            let keys = std::array::from_fn::<_, 3, _>(|i| SecretKey::new([11 + i as u8; 32]));
            let addresses = std::array::from_fn(|i| {
                SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, i as u8 + 1), 5000)
            });
            let relays = std::array::from_fn::<_, 3, _>(|i| (addresses[i], 1 << i, &keys[i]));
            let mut roster =
                sign_federation_roster(room, &relays, &SecretKey::new([9; 32])).unwrap();
            let (table, wraps) = roster.split_at_mut(ROSTER_HEADER + 3 * ROSTER_ENTRY);
            for (i, wrap) in wraps.chunks_exact_mut(ROSTER_WRAP).enumerate() {
                let tag = wrap_tag(
                    &keys[i],
                    ADJUDICATED_DOMAIN,
                    table,
                    i as u8,
                    &wrap[..16],
                    &wrap[16..48],
                )
                .finalize()
                .into_bytes();
                wrap[48..].copy_from_slice(&tag);
            }
            let mut game = Self {
                servers: std::array::from_fn(|i| match journals {
                    Some(dirs) => {
                        RelayServer::with_journal(keys[i].clone(), 4, &dirs[i].0).unwrap()
                    }
                    None => RelayServer::new(keys[i].clone(), 4),
                }),
                clients: std::array::from_fn(|i| client(&keys[i], i as u8)),
                addresses,
                now: 0,
            };
            for (i, (server, player)) in game.servers.iter_mut().zip(&mut game.clients).enumerate()
            {
                assert!(Federation::admit(&keys[i], &roster, &player.ticket, false).is_err());
                server.enable_federation_outcomes();
                player.set_federation(&roster).unwrap();
                connect(server, player, 0);
                let mut bytes = Vec::new();
                player
                    .configure_ordering(&certificate(&keys[i], 7, 2000), [7; 32], &mut bytes)
                    .unwrap();
                events(server, player, &bytes, 0);
            }
            // A measured round trip then the coordinator's agreed start.
            for _ in 0..20 {
                game.step(100, None);
            }
            assert!(
                game.servers.iter().all(|server| server.rooms[&room]
                    .ordering
                    .as_ref()
                    .unwrap()
                    .head
                    > 0)
            );
            game
        }

        fn step(&mut self, elapsed: u64, drop_link: Option<(usize, usize)>) {
            self.now += elapsed;
            let now = self.now;
            let mut deliveries = Vec::new();
            for (from, server) in self.servers.iter_mut().enumerate() {
                server.advance_clock(now, unix(now));
                for packet in server.drain_federation(now, unix(now)) {
                    let to = self
                        .addresses
                        .iter()
                        .position(|address| *address == packet.endpoint)
                        .unwrap();
                    if drop_link != Some((from, to)) {
                        deliveries.push((from, to, packet));
                    }
                }
            }
            for (from, to, packet) in deliveries {
                self.servers[to]
                    .process_at(self.addresses[from], &packet.bytes, unix(now), now)
                    .unwrap();
            }
        }

        fn keep_players(&mut self) {
            for (server, client) in self.servers.iter_mut().zip(&mut self.clients) {
                renew(server, client, self.now);
                let cursor = server.rooms[&[5; 16]].ordering.as_ref().unwrap().next_seq;
                assert!(matches!(
                    pull(server, client, cursor, self.now).first(),
                    Some(OrderEvent::Status(_))
                ));
            }
        }
    }

    #[test]
    fn lost_coordinator_link_voids_all_three_even_when_only_one_follower_cannot_hear_it() {
        let mut game = Match::new();
        let cut = Some((0, 1));
        // A short outage heals. Neither a missing source uplink nor the old
        // outage can shorten the new, full link grace.
        game.step(1000, cut);
        game.step(100, None);
        for _ in 0..5 {
            game.step(100_000, cut);
            game.keep_players();
        }
        game.step(99_999, cut);
        assert!(
            game.servers
                .iter()
                .all(|server| server.pending_federation_outcomes().is_empty())
        );
        game.step(1, cut);
        // Only relay 1 has confirmed loss. Until its spool acknowledges the
        // receipt, neither its players nor the rest of the fleet sees it.
        assert_eq!(
            game.servers[1].pending_federation_outcomes()[0].as_bytes()[53],
            1
        );
        assert!(game.servers[0].pending_federation_outcomes().is_empty());
        let answer = pull(&mut game.servers[1], &mut game.clients[1], 1, game.now);
        assert!(matches!(
            answer.as_slice(),
            [OrderEvent::Error {
                code: OrderError::Backpressure,
                ..
            }]
        ));
        game.step(LOCAL_HANDOFF_MS, cut);
        let answer = pull(&mut game.servers[1], &mut game.clients[1], 1, game.now);
        assert!(matches!(
            answer.as_slice(),
            [OrderEvent::Error {
                code: OrderError::Inactive,
                expected: 0
            }]
        ));
        game.servers[1].federation_outcome_saved([5; 16]);
        game.step(100, cut);
        game.step(100, cut);
        for index in [0, 2] {
            let receipt = &game.servers[index].pending_federation_outcomes()[0];
            assert_eq!(receipt.as_bytes()[53], 1);
            assert_eq!(receipt.room(), [5; 16]);
            game.servers[index].federation_outcome_saved(receipt.room());
        }
        for (server, player) in game.servers.iter_mut().zip(&mut game.clients) {
            let mut bytes = Vec::new();
            player.pull_ordering_at(1, game.now, &mut bytes).unwrap();
            let answer = events(server, player, &bytes, game.now);
            assert!(matches!(
                answer.as_slice(),
                [OrderEvent::Error {
                    code: OrderError::Inactive,
                    expected: FEDERATION_LOST
                }]
            ));
            assert!(server.pending_federation_outcomes().is_empty());
        }
    }

    #[test]
    fn a_player_can_go_silent_while_its_federation_keeps_playing() {
        let mut game = Match::new();
        for _ in 0..7 {
            game.step(100_000, None);
            for index in [0, 2] {
                let (server, player) = (&mut game.servers[index], &mut game.clients[index]);
                renew(server, player, game.now);
                let cursor = server.rooms[&[5; 16]].ordering.as_ref().unwrap().next_seq;
                pull(server, player, cursor, game.now);
            }
        }
        game.step(100, None);
        for server in &game.servers {
            let ordering = server.rooms[&[5; 16]].ordering.as_ref().unwrap();
            assert_eq!(ordering.active, 0b101, "only the silent player departs");
            assert!(ordering.finished_at.is_none());
            assert!(server.pending_federation_outcomes().is_empty());
        }
    }

    /// Real lobby-issued marked roster, signed with public fixture keys 0b,
    /// 0c, 0d (each repeated 32 times). The lobby verifies the same VFO1 bytes.
    #[test]
    fn the_lobbys_marked_roster_accepts_the_public_signed_outcome() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/lobby/federation_outcome.json");
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let hex = |name: &str| {
            fixture[name]
                .as_str()
                .unwrap()
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                .collect::<Vec<_>>()
        };
        let roster = hex("roster_hex");
        let index = fixture["signer"].as_u64().unwrap() as u8;
        let key = SecretKey::new([11 + index; 32]);
        let mut ticket = client(&key, index).ticket;
        ticket.room.copy_from_slice(&roster[4..20]);
        let fed = Federation::admit(&key, &roster, &ticket, true).unwrap();
        let proof = FederationOutcome::sign(
            &fed,
            &key,
            fixture["kind"] == 1,
            fixture["issued_at"].as_u64().unwrap(),
        );
        assert_eq!(proof.as_bytes().as_slice(), &hex("receipt"));
    }

    #[test]
    fn orderly_departures_generate_a_single_normal_receipt_per_relay() {
        let dirs = std::array::from_fn(|_| TestDirectory::new());
        let mut game = Match::build(Some(&dirs));
        for (server, player) in game.servers.iter_mut().zip(&mut game.clients) {
            let mut bytes = Vec::new();
            player.leave_ordering_at(game.now, &mut bytes).unwrap();
            server
                .process_at(endpoint(player), &bytes, unix(game.now), game.now)
                .unwrap();
        }
        for _ in 0..12 {
            game.step(100, None);
        }
        for (index, server) in game.servers.iter_mut().enumerate() {
            let receipts = server.pending_federation_outcomes();
            assert_eq!(receipts.len(), 1);
            let bytes = receipts[0].as_bytes();
            assert_eq!(&bytes[52..54], &[index as u8, 0]);
            assert!(
                server
                    .key
                    .mac(OUTCOME_DOMAIN, &bytes[..BODY_LEN])
                    .verify_slice(&bytes[BODY_LEN..])
                    .is_ok()
            );
            assert_eq!(
                receipts[0].as_bytes(),
                server.pending_federation_outcomes()[0].as_bytes()
            );
            // A failed/full disk writer holds only this small receipt after
            // the normal terminal replay grace. The real journal is released.
            let expired = game.now + MAX_TICKET_LIFETIME * 1000;
            server.advance_clock(expired, unix(expired));
            assert_eq!(server.room_count(), 0);
            assert!(
                server
                    .journal_directory
                    .as_ref()
                    .unwrap()
                    .reserved(&receipts[0].room())
                    .is_none()
            );
            assert_eq!(server.pending_federation_outcomes()[0].as_bytes(), bytes);
            server.federation_outcome_saved(receipts[0].room());
            assert!(server.pending_federation_outcomes().is_empty());
        }
    }
}
