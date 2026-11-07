//! An in-process, single-threaded network simulation of one relay room under
//! a virtual clock (services/relay/adaptive-transport/e2e-plan.md). The
//! production `RelayServer` and `ClientSession` run unchanged; this module
//! supplies only the network, the clock, the content and the observation.
//! Built only for tests and the `test-support` feature.
//!
//! The relay journals to a fresh temporary directory, removed afterwards,
//! because the deployed relay always runs with its journal (main.rs): it
//! serves old history from disk and reserves journal space for batches.
//!
//! Clock: virtual µs. The relay passes (`advance_clock`, then
//! `drain_ordering_push`) every millisecond and after every datagram it
//! accepts, as `services/relay/src/main.rs` does. Each client's game loop
//! runs every 16 ms with its own phase and passes the 16 ms wall bucket.

pub mod adaptive_client;
pub mod client;
pub mod inspect;
pub mod network;
pub mod reader;
pub mod report;
pub mod scenario;
pub mod trace;

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, HashMap};
use std::net::{Ipv4Addr, SocketAddrV4};

use crate::{
    ClientLinkStats, ClientSession, ForwardDatagram, OrderingCertificate, ReaderLinkStats,
    RelayServer, SecretKey, Ticket, derive_packet_key, turn_frames,
};
use adaptive_client::AdaptiveClient;
use client::{HeadlessClient, Outgoing};
use inspect::Detail;
use network::{Link, Loss, Rng};
use reader::Record;
use report::{ClientReport, Distribution, Report, Traffic};
pub use scenario::Scenario;
use scenario::{Direction, DropRule, ServiceMode};
use trace::{Origin, Trace, WireRecord};

/// The relay's wall clock in Unix seconds at virtual time zero.
const UNIX_BASE: u64 = 1_000_000;
/// The game loop's service period and wall bucket (src/net/udp.rs uses
/// 16 ms buckets; spec §6.3 "游戏线程大约每 16 ms 一次").
const SERVICE_MS: u64 = 16;
const RELAY_PASS_US: u64 = 1_000;
/// Clients start their handshakes this far apart at most.
const START_SPREAD_US: u64 = 50_000;
/// Random streams per client: uplink, downlink, commands, start time.
const STREAMS: u64 = 4;
const PAYLOAD_KEY: [u8; 32] = [89; 32];
/// The room every scenario plays in.
pub const ROOM: [u8; 16] = [0x42; 16];

#[derive(Debug, Clone, Copy)]
enum Action {
    Command,
    Leave,
    Renew,
    Restore(bool),
    Rebind,
    Replay,
}

enum Event {
    RelayPass,
    /// The periodic game-loop service, which schedules the next one.
    ClientTick(usize),
    /// An extra event-mode service.
    Wake(usize),
    /// A client datagram copy reaching the relay, with its wire record.
    ToRelay(usize, Vec<u8>, usize),
    ToClient(usize, Vec<u8>),
    Action(usize, Action),
    /// The next datagram of `scenario.forgeries[i]`.
    Forge(usize),
}

struct Queued {
    at_us: u64,
    order: u64,
    event: Event,
}

impl PartialEq for Queued {
    fn eq(&self, other: &Self) -> bool {
        (self.at_us, self.order) == (other.at_us, other.order)
    }
}
impl Eq for Queued {}
impl PartialOrd for Queued {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Queued {
    // Earliest first from a max-heap; ties in scheduling order.
    fn cmp(&self, other: &Self) -> Ordering {
        (other.at_us, other.order).cmp(&(self.at_us, self.order))
    }
}

#[derive(Default)]
struct Observed {
    up: Traffic,
    down: Traffic,
    lost_up: u64,
    lost_down: u64,
    /// SUBMIT datagrams per batch, keyed by batch id, with its first send.
    submits: HashMap<u64, (u64, u64)>,
    malformed: u64,
}

struct Client {
    label: String,
    endpoint: SocketAddrV4,
    driver: Box<dyn HeadlessClient>,
    ticket: Ticket,
    /// Each hop's v3 session once its READY was seen (compaction spec §6.2),
    /// which names v3 datagrams' hop.
    sessions: Vec<Option<u32>>,
    /// Each hop's packet key (`derive_packet_key`), for tag checks.
    hop_keys: Vec<[u8; 32]>,
    /// (sent µs, bytes) of the client's UPLINKs of the last two seconds,
    /// kept only when the scenario replays.
    recent_uplinks: std::collections::VecDeque<(u64, Vec<u8>)>,
    up: Link,
    down: Link,
    commands: Rng,
    next_command_us: Option<u64>,
    /// The next record sequence the relay has not yet put on the wire to
    /// this client, which tells a fresh PACK from a repeat.
    pack_high: u64,
    /// Datagrams seen per (direction, kind), for drop rules.
    seen: HashMap<(Direction, &'static str), u64>,
    observed: Observed,
    wakes: BTreeSet<u64>,
    last_stats: Option<ClientLinkStats>,
    last_reader: Option<ReaderLinkStats>,
}

/// The relay's journal directory, removed with the simulation.
struct JournalDirectory(std::path::PathBuf);

impl JournalDirectory {
    fn new() -> Self {
        let mut random = [0; 16];
        getrandom::fill(&mut random).expect("entropy");
        let name: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        Self(std::env::temp_dir().join(format!("vera-netsim-{name}")))
    }
}

impl Drop for JournalDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Simulation {
    scenario: Scenario,
    key: SecretKey,
    relay: RelayServer,
    _journal: JournalDirectory,
    clients: Vec<Client>,
    endpoints: HashMap<SocketAddrV4, usize>,
    queue: BinaryHeap<Queued>,
    order: u64,
    relay_rejects: BTreeMap<String, u64>,
    /// The relay's own traffic at the network layer, in the window: what
    /// it sent (before loss) and what reached it (copies included).
    relay_sent: Traffic,
    relay_received: Traffic,
    /// Datagrams sent per `scenario.forgeries` entry.
    forged_sent: Vec<usize>,
    trace: Option<Trace>,
}

fn unix(now_ms: u64) -> u64 {
    UNIX_BASE + now_ms / 1000
}

impl Simulation {
    pub fn new(scenario: Scenario) -> Self {
        let key = SecretKey::new([0x5A; 32]);
        let expires = UNIX_BASE + crate::MAX_TICKET_LIFETIME;
        let certificate = OrderingCertificate {
            room: ROOM,
            sources_mask: (1u32 << scenario.players) - 1,
            tick_hz: scenario.tick_hz,
            expires,
            pacing: crate::CertificatePacing::On,
        }
        .sign(&key)
        .expect("certificate");
        let mut clients = Vec::new();
        let mut endpoints = HashMap::new();
        for (index, (label, up, down)) in scenario.client_networks().into_iter().enumerate() {
            let source = index as u8;
            let ticket = Ticket {
                room: ROOM,
                source,
                expires,
                nonce: [source + 1; 16],
                player: [source + 101; 16],
                generation: 1,
            };
            let signed = ticket.sign(&key).expect("ticket");
            let hop_key = *derive_packet_key(&key, &signed).expose_for_https();
            let mut session =
                ClientSession::new(&signed, hop_key, PAYLOAD_KEY).expect("client session");
            if scenario.preset_measured_good {
                session.preset_uplink_measured_good();
            }
            let stream = |n: u64| Rng::new(scenario.seed, index as u64 * STREAMS + n);
            let endpoint = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, source + 1), 40_000);
            endpoints.insert(endpoint, index);
            let fault = scenario
                .desync
                .filter(|desync| desync.client == source)
                .map(|desync| desync.frame);
            let driver: Box<dyn HeadlessClient> = Box::new(AdaptiveClient::new(
                session,
                certificate.clone(),
                PAYLOAD_KEY,
                scenario.tick_hz,
                scenario.command_bytes,
                fault,
            ));
            clients.push(Client {
                label,
                endpoint,
                driver,
                ticket,
                sessions: vec![None],
                hop_keys: vec![hop_key],
                recent_uplinks: Default::default(),
                up: Link::new(up, stream(0)),
                down: Link::new(down, stream(1)),
                commands: stream(2),
                next_command_us: None,
                pack_high: 1,
                seen: HashMap::new(),
                observed: Observed::default(),
                wakes: BTreeSet::new(),
                last_stats: None,
                last_reader: None,
            });
        }
        let journal = JournalDirectory::new();
        let mut simulation = Self {
            relay: RelayServer::with_journal(key.clone(), 4, &journal.0).expect("relay journal"),
            key,
            _journal: journal,
            clients,
            endpoints,
            queue: BinaryHeap::new(),
            order: 0,
            relay_rejects: BTreeMap::new(),
            relay_sent: Traffic::default(),
            relay_received: Traffic::default(),
            forged_sent: vec![0; scenario.forgeries.len()],
            trace: None,
            scenario,
        };
        simulation.schedule(0, Event::RelayPass);
        for index in 0..simulation.clients.len() {
            let start = match &simulation.scenario.start_offsets_ms {
                Some(offsets) => offsets[index] * 1000,
                None => {
                    Rng::new(simulation.scenario.seed, index as u64 * STREAMS + 3).next_u64()
                        % START_SPREAD_US
                }
            };
            simulation.schedule(start, Event::ClientTick(index));
        }
        let actions: Vec<(u8, u64, Action)> = simulation
            .scenario
            .commands
            .iter()
            .map(|at| (at.client, at.at_ms, Action::Command))
            .chain(
                simulation
                    .scenario
                    .leaves
                    .iter()
                    .map(|at| (at.client, at.at_ms, Action::Leave)),
            )
            .chain(
                simulation
                    .scenario
                    .renewals
                    .iter()
                    .map(|at| (at.client, at.at_ms, Action::Renew)),
            )
            .chain(simulation.scenario.restoring.iter().flat_map(|window| {
                [
                    (window.client, window.from_ms, Action::Restore(true)),
                    (window.client, window.to_ms, Action::Restore(false)),
                ]
            }))
            .chain(
                simulation
                    .scenario
                    .rebinds
                    .iter()
                    .map(|at| (at.client, at.at_ms, Action::Rebind)),
            )
            .chain(
                simulation
                    .scenario
                    .replays
                    .iter()
                    .map(|at| (at.client, at.at_ms, Action::Replay)),
            )
            .collect();
        for (client, at_ms, action) in actions {
            simulation.schedule(at_ms * 1000, Event::Action(usize::from(client), action));
        }
        let forgeries: Vec<u64> = simulation
            .scenario
            .forgeries
            .iter()
            .map(|forgery| forgery.from_ms)
            .collect();
        for (index, from_ms) in forgeries.into_iter().enumerate() {
            simulation.schedule(from_ms * 1000, Event::Forge(index));
        }
        simulation
    }

    fn schedule(&mut self, at_us: u64, event: Event) {
        self.order += 1;
        self.queue.push(Queued {
            at_us,
            order: self.order,
            event,
        });
    }

    fn window(&self) -> (u64, u64) {
        let from = self.scenario.warmup_ms * 1000;
        (from, from + self.scenario.duration_ms * 1000)
    }

    fn measured(&self, at_us: u64) -> bool {
        let (from, to) = self.window();
        (from..to).contains(&at_us)
    }

    /// The client's clock: the 16 ms wall bucket in ms for the game loop
    /// (src/net/live_commands.rs line 472), whole ms in event mode.
    fn client_ms(&self, at_us: u64) -> u64 {
        match self.scenario.service {
            ServiceMode::Periodic => at_us / 1000 / SERVICE_MS * SERVICE_MS,
            ServiceMode::Event => at_us / 1000,
        }
    }

    fn dropped_by_rule(
        &mut self,
        index: usize,
        direction: Direction,
        kinds: &[&'static str],
    ) -> bool {
        let client = &mut self.clients[index];
        let mut drop = false;
        for &kind in kinds {
            let seen = client.seen.entry((direction, kind)).or_default();
            *seen += 1;
            let nth = *seen;
            drop |= self.scenario.drop_rules.iter().any(|rule: &DropRule| {
                rule.direction == direction
                    && usize::from(rule.client) == index
                    && rule.kind == kind
                    && rule.nth == nth
            });
        }
        drop
    }

    fn record(&mut self, record: WireRecord) -> usize {
        match self.trace.as_mut() {
            Some(trace) => {
                trace.wire.push(record);
                trace.wire.len() - 1
            }
            None => usize::MAX,
        }
    }

    fn send_up(&mut self, index: usize, outgoing: Outgoing, at_us: u64) {
        let Outgoing {
            bytes,
            hop,
            service,
            heartbeat,
        } = outgoing;
        let measured = self.measured(at_us);
        let parsed = inspect::client_datagram(&bytes);
        let kind = match &parsed {
            Ok(datagram) => {
                if let Detail::Uplink(uplink) = &datagram.detail {
                    for batch_id in uplink.batch_ids() {
                        let entry = self.clients[index]
                            .observed
                            .submits
                            .entry(batch_id)
                            .or_insert((at_us, 0));
                        entry.1 += 1;
                    }
                }
                datagram.kind
            }
            Err(_) => {
                self.clients[index].observed.malformed += 1;
                "MALFORMED"
            }
        };
        if measured {
            self.clients[index].observed.up.add(kind, bytes.len());
        }
        if !self.scenario.replays.is_empty() && kind == "UPLINK" {
            let recent = &mut self.clients[index].recent_uplinks;
            recent.push_back((at_us, bytes.clone()));
            while recent
                .front()
                .is_some_and(|(at, _)| *at + 2_000_000 < at_us)
            {
                recent.pop_front();
            }
        }
        let lost = if self.dropped_by_rule(index, Direction::Up, &[kind]) {
            Err(Loss::Rule)
        } else {
            self.clients[index]
                .up
                .transmit(at_us, bytes.len() + report::IP_UDP_BYTES)
        };
        let arrivals = lost.clone().unwrap_or_default();
        let wire = self.record(WireRecord {
            client: index,
            direction: Direction::Up,
            hop,
            service,
            heartbeat,
            sent_us: at_us,
            arrivals_us: arrivals.clone(),
            origin: Origin::Client,
            len: bytes.len(),
            bytes: self.scenario.keep_wire_bytes.then(|| bytes.clone()),
            parsed,
            relay_result: None,
        });
        if lost.is_err() && measured {
            self.clients[index].observed.lost_up += 1;
        }
        for arrival in arrivals {
            self.schedule(arrival, Event::ToRelay(index, bytes.clone(), wire));
        }
    }

    fn send_down(&mut self, index: usize, bytes: Vec<u8>, at_us: u64, origin: Origin) {
        let measured = self.measured(at_us);
        let client = &mut self.clients[index];
        let parsed = inspect::server_datagram(&bytes);
        let kinds: Vec<&'static str> = match &parsed {
            Ok(datagram) => match &datagram.detail {
                Detail::Pack { base, count, .. } => {
                    let end = base + u64::from(*count);
                    let fresh = end > client.pack_high;
                    client.pack_high = client.pack_high.max(end);
                    vec![
                        if fresh { "PACK_NEW" } else { "PACK_REPEAT" },
                        datagram.kind,
                    ]
                }
                _ => vec![datagram.kind],
            },
            Err(_) => {
                client.observed.malformed += 1;
                vec!["MALFORMED"]
            }
        };
        // Compaction spec §6.2: the READY with the hop's first relay
        // sequence names the newest hop's session; session 0 (CHALLENGE)
        // belongs to the hop being set up, the newest.
        let newest = client.sessions.len() - 1;
        let hop = match parsed.as_ref() {
            Ok(datagram) => {
                if let Detail::HopReady { session } = datagram.detail {
                    client.sessions[newest] = Some(session);
                }
                client
                    .sessions
                    .iter()
                    .position(|known| *known == Some(datagram.session))
                    .unwrap_or(newest)
            }
            Err(_) => newest,
        };
        if measured {
            client.observed.down.add(kinds[0], bytes.len());
            self.relay_sent.add(kinds[0], bytes.len());
        }
        let lost = if self.dropped_by_rule(index, Direction::Down, &kinds) {
            Err(Loss::Rule)
        } else {
            self.clients[index]
                .down
                .transmit(at_us, bytes.len() + report::IP_UDP_BYTES)
        };
        let arrivals = lost.clone().unwrap_or_default();
        self.record(WireRecord {
            client: index,
            direction: Direction::Down,
            hop,
            service: None,
            heartbeat: false,
            sent_us: at_us,
            arrivals_us: arrivals.clone(),
            origin,
            len: bytes.len(),
            bytes: self.scenario.keep_wire_bytes.then(|| bytes.clone()),
            parsed,
            relay_result: None,
        });
        if lost.is_err() && measured {
            self.clients[index].observed.lost_down += 1;
        }
        for arrival in arrivals {
            self.schedule(arrival, Event::ToClient(index, bytes.clone()));
        }
    }

    fn route(&mut self, datagrams: Vec<ForwardDatagram>, at_us: u64, origin: Origin) {
        for datagram in datagrams {
            if let Some(&index) = self.endpoints.get(&datagram.endpoint) {
                self.send_down(index, datagram.bytes, at_us, origin);
            } else if let Some(trace) = self.trace.as_mut() {
                // A former address after a rebind, or a forger's.
                trace.misrouted.push((at_us, datagram.endpoint));
            }
        }
    }

    /// A forger's address: `10.9.<forgery>.<k + 1>` (compaction spec §6.4
    /// counts failures per IP, so each one is its own).
    fn forger_endpoint(forgery: usize, k: u8) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(10, 9, forgery as u8, k + 1), 50_000)
    }

    /// One forged CONTROL datagram on the client's newest known session, a
    /// random sequence and a random tag (compaction spec §6.4), then the
    /// next one. No session yet: nothing to forge, try again later.
    fn forge(&mut self, index: usize, at_us: u64) {
        let forgery = self.scenario.forgeries[index];
        if at_us >= forgery.to_ms * 1000 {
            return;
        }
        let period_us = 1_000_000 / u64::from(forgery.per_second);
        let client = usize::from(forgery.client);
        let sent = self.forged_sent[index];
        let session = self.clients[client]
            .sessions
            .iter()
            .rev()
            .flatten()
            .next()
            .copied();
        if let Some(session) = session {
            let mut random = Rng::new(
                self.scenario.seed ^ 0xF0F0,
                (index as u64) << 32 | sent as u64,
            );
            let mut bytes = vec![inspect::v3::VERSION << 5 | inspect::v3::CONTROL];
            bytes.extend_from_slice(&session.to_le_bytes());
            bytes.extend_from_slice(&(random.next_u64() as u32 | 1).to_le_bytes());
            // An empty UPLINK body would do; random bytes after its type.
            bytes.push(8);
            bytes.extend((0..17).map(|_| random.next_u64() as u8));
            bytes.extend_from_slice(&(random.next_u64() as u32).to_le_bytes());
            let endpoint =
                Self::forger_endpoint(index, (sent % usize::from(forgery.addresses)) as u8);
            let now_ms = at_us / 1000;
            let result = self
                .relay
                .process_at(endpoint, &bytes, unix(now_ms), now_ms);
            let replies = result.as_ref().map_or(0, |output| output.len());
            if let Some(trace) = self.trace.as_mut() {
                trace.forged.push(trace::Forged {
                    forgery: index,
                    at_us,
                    endpoint,
                    result: result
                        .as_ref()
                        .map(|_| ())
                        .map_err(|error| format!("{error:?}")),
                    replies,
                });
            }
            if let Ok(output) = result {
                self.route(output, at_us, Origin::Reply);
            }
        }
        self.forged_sent[index] += usize::from(session.is_some());
        self.schedule(at_us + period_us, Event::Forge(index));
    }

    /// Resends the client's newest UPLINK a second old or older, from an
    /// attacker address, unchanged.
    fn replay(&mut self, index: usize, at_us: u64) {
        let Some((_, bytes)) = self.clients[index]
            .recent_uplinks
            .iter()
            .rev()
            .find(|(sent, _)| *sent + 1_000_000 <= at_us)
            .cloned()
        else {
            return;
        };
        let endpoint = SocketAddrV4::new(Ipv4Addr::new(10, 7, 0, index as u8 + 1), 50_000);
        let now_ms = at_us / 1000;
        let result = self
            .relay
            .process_at(endpoint, &bytes, unix(now_ms), now_ms);
        let replies = result.as_ref().map_or(0, |output| output.len());
        if let Some(trace) = self.trace.as_mut() {
            trace.replayed.push(trace::Forged {
                forgery: index,
                at_us,
                endpoint,
                result: result
                    .as_ref()
                    .map(|_| ())
                    .map_err(|error| format!("{error:?}")),
                replies,
            });
        }
        if let Ok(output) = result {
            self.route(output, at_us, Origin::Reply);
        }
    }

    /// The client moves to a new address (compaction spec §6.2 "换地址"):
    /// datagrams to the old one are lost from now on.
    fn rebind(&mut self, index: usize) {
        let old = self.clients[index].endpoint;
        let new = SocketAddrV4::new(*old.ip(), old.port() + 1);
        self.endpoints.remove(&old);
        self.endpoints.insert(new, index);
        self.clients[index].endpoint = new;
        if let Some(trace) = self.trace.as_mut() {
            trace.endpoints[index].push(new);
        }
    }

    fn relay_pass(&mut self, at_us: u64) {
        let now_ms = at_us / 1000;
        self.relay.advance_clock(now_ms, unix(now_ms));
        let pushed = self.relay.drain_ordering_push(now_ms, unix(now_ms));
        self.route(pushed, at_us, Origin::Push);
        if self.trace.is_some() {
            for index in 0..self.clients.len() {
                let stats = self.relay.reader_link_stats(ROOM, index as u8);
                if stats != self.clients[index].last_reader {
                    self.clients[index].last_reader = stats;
                    if let Some(stats) = stats {
                        self.trace
                            .as_mut()
                            .unwrap()
                            .reader_stats
                            .push((at_us, index, stats));
                    }
                }
            }
        }
    }

    fn client_tick(&mut self, index: usize, at_us: u64, periodic: bool) {
        let now_ms = self.client_ms(at_us);
        let apm = self.scenario.apm;
        let client = &mut self.clients[index];
        if apm > 0.0 && client.driver.started() {
            let mean_us = 60_000_000.0 / apm;
            let next = client
                .next_command_us
                .get_or_insert_with(|| at_us + client.commands.exponential(mean_us) as u64);
            while *next <= at_us {
                client.driver.queue_command(*next);
                if let Some(trace) = self.trace.as_mut() {
                    trace.issued[index].push(*next);
                }
                *next += client.commands.exponential(mean_us) as u64;
            }
        }
        client.driver.tick(now_ms, at_us);
        let output = client.driver.take_output();
        if let Some(trace) = self.trace.as_mut() {
            trace.services.push((at_us, index));
            if let Some(stats) = self.clients[index].driver.link_stats()
                && Some(stats) != self.clients[index].last_stats
            {
                self.clients[index].last_stats = Some(stats);
                trace.client_stats.push((at_us, index, stats));
            }
        }
        for outgoing in output {
            self.send_up(index, outgoing, at_us);
        }
        if periodic {
            self.schedule(at_us + SERVICE_MS * 1000, Event::ClientTick(index));
        }
        if self.scenario.service == ServiceMode::Event {
            for wake_ms in self.clients[index].driver.wakeups() {
                // A deadline already due that this service did not meet
                // is tried again a millisecond later.
                let wake_us = (wake_ms * 1000).max(at_us + 1000);
                if self.clients[index].wakes.insert(wake_us) {
                    self.schedule(wake_us, Event::Wake(index));
                }
            }
        }
    }

    fn renew(&mut self, index: usize, at_us: u64) {
        let now_ms = self.client_ms(at_us);
        let client = &mut self.clients[index];
        let mut ticket = client.ticket;
        ticket.nonce[15] = ticket.nonce[15].wrapping_add(100);
        // A renewal must outlive the ticket it replaces (ClientSession::renewal).
        ticket.expires = (unix(at_us / 1000) + crate::MAX_TICKET_LIFETIME).max(ticket.expires + 1);
        let signed = ticket.sign(&self.key).expect("renewed ticket");
        client.ticket = ticket;
        client.sessions.push(None);
        let hop_key = *derive_packet_key(&self.key, &signed).expose_for_https();
        client.hop_keys.push(hop_key);
        client.driver.renew(&signed, hop_key, now_ms);
        for outgoing in self.clients[index].driver.take_output() {
            self.send_up(index, outgoing, at_us);
        }
    }

    /// Runs the scenario to its end and reports the measurement window.
    pub fn run(self) -> Report {
        self.run_inner(false).0
    }

    /// Like `run`, and also returns every datagram and observation snapshot.
    pub fn run_traced(self) -> (Report, Trace) {
        let (report, trace) = self.run_inner(true);
        (report, trace.expect("traced"))
    }

    /// Handles every event before `end_us`, in order.
    fn step_until(&mut self, end_us: u64) {
        let event_mode = self.scenario.service == ServiceMode::Event;
        while self.queue.peek().is_some_and(|next| next.at_us < end_us) {
            let Queued { at_us, event, .. } = self.queue.pop().unwrap();
            match event {
                Event::RelayPass => {
                    self.relay_pass(at_us);
                    self.schedule(at_us + RELAY_PASS_US, Event::RelayPass);
                }
                Event::ClientTick(index) => self.client_tick(index, at_us, true),
                Event::Wake(index) => {
                    self.clients[index].wakes.remove(&at_us);
                    self.client_tick(index, at_us, false);
                }
                Event::ToRelay(index, bytes, wire) => {
                    if self.measured(at_us) {
                        let kind = inspect::client_datagram(&bytes)
                            .map_or("MALFORMED", |datagram| datagram.kind);
                        self.relay_received.add(kind, bytes.len());
                    }
                    let now_ms = at_us / 1000;
                    let endpoint = self.clients[index].endpoint;
                    let result = self
                        .relay
                        .process_at(endpoint, &bytes, unix(now_ms), now_ms);
                    if let Some(record) = self
                        .trace
                        .as_mut()
                        .and_then(|trace| trace.wire.get_mut(wire))
                        && record.relay_result.is_none()
                    {
                        record.relay_result = Some(
                            result
                                .as_ref()
                                .map(|_| ())
                                .map_err(|error| format!("{error:?}")),
                        );
                    }
                    if result.is_ok()
                        && self.scenario.preset_measured_good
                        && inspect::client_datagram(&bytes).is_ok_and(|d| d.kind == "CONFIRM")
                    {
                        self.relay.preset_reader_measured_good(ROOM, index as u8);
                    }
                    match result {
                        Ok(output) => {
                            self.route(output, at_us, Origin::Reply);
                            self.relay_pass(at_us);
                        }
                        Err(error) => {
                            let kind = inspect::client_datagram(&bytes)
                                .map_or("MALFORMED", |datagram| datagram.kind);
                            *self
                                .relay_rejects
                                .entry(format!("{error:?} {kind}"))
                                .or_default() += 1;
                        }
                    }
                }
                Event::ToClient(index, bytes) => {
                    let now_ms = self.client_ms(at_us);
                    let client = &mut self.clients[index];
                    client.driver.deliver(&bytes, now_ms, at_us);
                    for outgoing in client.driver.take_output() {
                        self.send_up(index, outgoing, at_us);
                    }
                    if event_mode {
                        self.client_tick(index, at_us, false);
                    }
                }
                Event::Forge(index) => self.forge(index, at_us),
                Event::Action(index, action) => {
                    match action {
                        Action::Command => {
                            self.clients[index].driver.queue_command(at_us);
                            if let Some(trace) = self.trace.as_mut() {
                                trace.issued[index].push(at_us);
                            }
                        }
                        Action::Leave => self.clients[index].driver.leave(),
                        Action::Renew => self.renew(index, at_us),
                        Action::Restore(restoring) => {
                            self.clients[index].driver.set_restoring(restoring)
                        }
                        Action::Rebind => self.rebind(index),
                        Action::Replay => self.replay(index, at_us),
                    }
                    if event_mode {
                        self.client_tick(index, at_us, false);
                    }
                }
            }
        }
    }

    /// The relay's canonical history, every committed record in sequence
    /// order (compaction spec §12.2), by the test-support
    /// `RelayServer::ordering_history`.
    fn relay_history(&mut self) -> Option<Vec<Vec<u8>>> {
        Some(self.relay.ordering_history(ROOM))
    }

    fn run_inner(mut self, traced: bool) -> (Report, Option<Trace>) {
        if traced {
            self.trace = Some(Trace {
                issued: vec![Vec::new(); self.clients.len()],
                endpoints: self.clients.iter().map(|c| vec![c.endpoint]).collect(),
                ..Trace::default()
            });
        }
        let (_, end) = self.window();
        self.step_until(end);
        let report = self.report();
        let history = self.relay_history();
        let mac_counts = self.relay.take_mac_failure_counts();
        if let Some(trace) = self.trace.as_mut() {
            trace.relay_history = history;
            trace.mac_counts = Some(mac_counts);
            trace.client_mac_failures = self
                .clients
                .iter()
                .map(|c| c.driver.mac_failures())
                .collect();
            for client in &self.clients {
                let log = client.driver.log();
                trace.seals.push(log.seals.clone());
                trace.executed.push(log.commands.clone());
                trace.streams.push(client.driver.reader().read().to_vec());
                trace.plaintexts.push(log.plaintexts.clone());
                trace.hop_keys.push(client.hop_keys.clone());
            }
        }
        (report, self.trace.take())
    }

    fn report(&self) -> Report {
        let (from, to) = self.window();
        let seconds = (to - from) as f64 / 1e6;
        let rate = self.scenario.tick_hz;
        let turn_ms = f64::from(turn_frames(rate)) * 1000.0 / f64::from(rate);
        let in_window = |at: u64| (from..to).contains(&at);
        let count = |times: &[u64]| times.iter().filter(|at| in_window(**at)).count() as f64;
        let mut rows = Vec::new();
        for (index, client) in self.clients.iter().enumerate() {
            let observed = &client.observed;
            let log = client.driver.log();
            let direction = |traffic: &Traffic, lost: u64| report::Direction {
                packets_per_s: traffic.packets as f64 / seconds,
                bytes_per_s: traffic.bytes as f64 / seconds,
                kinds: traffic
                    .kinds
                    .iter()
                    .map(|(kind, (packets, _))| (kind.to_string(), *packets as f64 / seconds))
                    .collect(),
                net_lost_per_s: lost as f64 / seconds,
            };
            let latencies: Vec<f64> = log
                .commands
                .iter()
                .filter(|(issued, _)| in_window(*issued))
                .map(|(issued, executed)| (executed - issued) as f64 / 1000.0)
                .collect();
            let turns: Vec<f64> = log
                .advances
                .windows(2)
                .filter(|pair| in_window(pair[0]) && in_window(pair[1]))
                .map(|pair| (pair[1] - pair[0]) as f64 / 1000.0)
                .collect();
            let batches: Vec<u64> = observed
                .submits
                .values()
                .filter(|(first, _)| in_window(*first))
                .map(|(_, copies)| *copies)
                .collect();
            let pulls = observed
                .up
                .kinds
                .get("PULL")
                .map_or(0, |(packets, _)| *packets);
            let to_f64 = |map: &BTreeMap<String, u64>| {
                map.iter()
                    .map(|(key, value)| (key.clone(), *value as f64))
                    .collect()
            };
            let link = client.driver.link_stats().map(report::LinkSummary::of);
            let row = ClientReport {
                label: client.label.clone(),
                clients: 1,
                up: direction(&observed.up, observed.lost_up),
                down: direction(&observed.down, observed.lost_down),
                command_latency_ms: Distribution::of(latencies.clone()),
                turn_interval_ms: Distribution::of(turns.clone()),
                hitches: turns.iter().filter(|gap| **gap > 2.0 * turn_ms).count() as f64,
                gaps: link.as_ref().map_or(0.0, |link| link.gaps as f64),
                pulls: pulls as f64,
                repair_pulls: count(&log.repair_pulls),
                resends: link.as_ref().map_or(0.0, |link| link.rto_timeouts as f64),
                submits_per_batch: if batches.is_empty() {
                    0.0
                } else {
                    batches.iter().sum::<u64>() as f64 / batches.len() as f64
                },
                batches: batches.len() as f64,
                executed_frames: client
                    .driver
                    .reader()
                    .read()
                    .iter()
                    .filter(|record| matches!(record, Record::FrameEnd { .. }))
                    .count() as f64,
                errors: to_f64(&log.errors),
                refused: to_f64(&log.refused),
                gap_uplink_triggers: count(&log.gap_triggers.times),
                gap_uplink_resends: count(&log.gap_triggers.second_times),
                link,
                reader: self
                    .relay
                    .reader_link_stats(ROOM, index as u8)
                    .map(report::ReaderSummary::of),
            };
            rows.push((row, latencies, turns));
        }
        let mut labels: Vec<&str> = Vec::new();
        for client in &self.clients {
            if !labels.contains(&client.label.as_str()) {
                labels.push(&client.label);
            }
        }
        let mut groups: Vec<ClientReport> = labels
            .iter()
            .map(|label| {
                let members: Vec<_> = rows
                    .iter()
                    .filter(|(row, ..)| row.label == *label)
                    .map(|(row, l, t)| (row, l.clone(), t.clone()))
                    .collect();
                report::mean(label, &members)
            })
            .collect();
        if labels.len() > 1 {
            let all: Vec<_> = rows
                .iter()
                .map(|(row, l, t)| (row, l.clone(), t.clone()))
                .collect();
            groups.push(report::mean("all", &all));
        }
        let streams: Vec<&[Record]> = self
            .clients
            .iter()
            .map(|client| client.driver.reader().read())
            .collect();
        let shortest = streams.iter().map(|stream| stream.len()).min().unwrap_or(0);
        let stream_consistent = streams
            .iter()
            .all(|stream| stream[..shortest] == streams[0][..shortest]);
        let desync_records = streams[0]
            .iter()
            .filter(|record| matches!(record, Record::Desync { .. }))
            .count();
        let stalled_clients = self
            .clients
            .iter()
            .filter(|client| {
                client
                    .driver
                    .log()
                    .advances
                    .last()
                    .is_none_or(|last| to.saturating_sub(*last) > 2_000_000)
            })
            .count();
        Report {
            scenario: self.scenario.name.clone(),
            seed: self.scenario.seed,
            players: self.scenario.players,
            tick_hz: rate,
            turn_ms,
            measured_ms: self.scenario.duration_ms,
            apm: self.scenario.apm,
            stream_consistent,
            desync_records,
            stalled_clients,
            malformed: self
                .clients
                .iter()
                .map(|client| client.observed.malformed)
                .sum(),
            relay_rejects: self.relay_rejects.clone(),
            relay: report::RelayTraffic::of(&self.relay_sent, &self.relay_received, seconds),
            groups,
            clients: rows.into_iter().map(|(row, ..)| row).collect(),
        }
    }
}

/// Runs one scenario file's text.
pub fn run_json(text: &str) -> Result<Report, String> {
    Ok(Simulation::new(Scenario::from_json(text)?).run())
}

#[cfg(test)]
mod appendix_a;
#[cfg(test)]
mod compaction;
#[cfg(test)]
mod judgments;
#[cfg(test)]
mod tests;
