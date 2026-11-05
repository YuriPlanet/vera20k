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

pub mod inspect;
pub mod model_client;
pub mod network;
pub mod reader;
pub mod report;
pub mod scenario;

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashMap};
use std::net::{Ipv4Addr, SocketAddrV4};

use crate::{
    ClientSession, ForwardDatagram, OrderingCertificate, RelayServer, SecretKey, Ticket,
    derive_packet_key, turn_frames,
};
use inspect::Detail;
use model_client::ModelClient;
use network::{Link, Loss, Rng};
use reader::Record;
use report::{ClientReport, Distribution, Report, Traffic};
pub use scenario::Scenario;
use scenario::{Direction, DropRule};

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

enum Event {
    RelayPass,
    ClientTick(usize),
    ToRelay(usize, Vec<u8>),
    ToClient(usize, Vec<u8>),
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
    model: ModelClient,
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
    relay: RelayServer,
    _journal: JournalDirectory,
    clients: Vec<Client>,
    endpoints: HashMap<SocketAddrV4, usize>,
    queue: BinaryHeap<Queued>,
    order: u64,
    relay_rejects: BTreeMap<String, u64>,
}

fn unix(now_ms: u64) -> u64 {
    UNIX_BASE + now_ms / 1000
}

impl Simulation {
    pub fn new(scenario: Scenario) -> Self {
        let key = SecretKey::new([0x5A; 32]);
        let room = [0x42; 16];
        let expires = UNIX_BASE + crate::MAX_TICKET_LIFETIME;
        let certificate = OrderingCertificate {
            room,
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
                room,
                source,
                expires,
                nonce: [source + 1; 16],
                player: [source + 101; 16],
                generation: 1,
            }
            .sign(&key)
            .expect("ticket");
            let session = ClientSession::new(
                &ticket,
                *derive_packet_key(&key, &ticket).expose_for_https(),
                [89; 32],
            )
            .expect("client session");
            let stream = |n: u64| Rng::new(scenario.seed, index as u64 * STREAMS + n);
            let endpoint = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, source + 1), 40_000);
            endpoints.insert(endpoint, index);
            let fault = scenario
                .desync
                .filter(|desync| desync.client == source)
                .map(|desync| desync.frame);
            clients.push(Client {
                label,
                endpoint,
                model: ModelClient::new(
                    session,
                    certificate.clone(),
                    scenario.players,
                    scenario.tick_hz,
                    scenario.command_bytes,
                    fault,
                ),
                up: Link::new(up, stream(0)),
                down: Link::new(down, stream(1)),
                commands: stream(2),
                next_command_us: None,
                pack_high: 1,
                seen: HashMap::new(),
                observed: Observed::default(),
            });
        }
        let journal = JournalDirectory::new();
        let mut simulation = Self {
            relay: RelayServer::with_journal(key, 4, &journal.0).expect("relay journal"),
            _journal: journal,
            clients,
            endpoints,
            queue: BinaryHeap::new(),
            order: 0,
            relay_rejects: BTreeMap::new(),
            scenario,
        };
        simulation.schedule(0, Event::RelayPass);
        for index in 0..simulation.clients.len() {
            let start = Rng::new(simulation.scenario.seed, index as u64 * STREAMS + 3).next_u64()
                % START_SPREAD_US;
            simulation.schedule(start, Event::ClientTick(index));
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

    fn send_up(&mut self, index: usize, bytes: Vec<u8>, at_us: u64) {
        let measured = self.measured(at_us);
        let kind = match inspect::client_datagram(&bytes) {
            Ok(datagram) => {
                if let Detail::Submit { batch_id } = datagram.detail {
                    let entry = self.clients[index]
                        .observed
                        .submits
                        .entry(batch_id)
                        .or_insert((at_us, 0));
                    entry.1 += 1;
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
        let lost = if self.dropped_by_rule(index, Direction::Up, &[kind]) {
            Err(Loss::Rule)
        } else {
            self.clients[index]
                .up
                .transmit(at_us, bytes.len() + report::IP_UDP_BYTES)
        };
        match lost {
            Ok(arrivals) => {
                for arrival in arrivals {
                    self.schedule(arrival, Event::ToRelay(index, bytes.clone()));
                }
            }
            Err(_) if measured => self.clients[index].observed.lost_up += 1,
            Err(_) => {}
        }
    }

    fn send_down(&mut self, index: usize, bytes: Vec<u8>, at_us: u64) {
        let measured = self.measured(at_us);
        let client = &mut self.clients[index];
        let kinds: Vec<&'static str> = match inspect::server_datagram(&bytes) {
            Ok(datagram) => match datagram.detail {
                Detail::Pack { base, count } => {
                    let end = base + u64::from(count);
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
        if measured {
            client.observed.down.add(kinds[0], bytes.len());
        }
        let lost = if self.dropped_by_rule(index, Direction::Down, &kinds) {
            Err(Loss::Rule)
        } else {
            self.clients[index]
                .down
                .transmit(at_us, bytes.len() + report::IP_UDP_BYTES)
        };
        match lost {
            Ok(arrivals) => {
                for arrival in arrivals {
                    self.schedule(arrival, Event::ToClient(index, bytes.clone()));
                }
            }
            Err(_) if measured => self.clients[index].observed.lost_down += 1,
            Err(_) => {}
        }
    }

    fn route(&mut self, datagrams: Vec<ForwardDatagram>, at_us: u64) {
        for datagram in datagrams {
            if let Some(&index) = self.endpoints.get(&datagram.endpoint) {
                self.send_down(index, datagram.bytes, at_us);
            }
        }
    }

    fn relay_pass(&mut self, at_us: u64) {
        let now_ms = at_us / 1000;
        self.relay.advance_clock(now_ms, unix(now_ms));
        let pushed = self.relay.drain_ordering_push(now_ms, unix(now_ms));
        self.route(pushed, at_us);
    }

    fn client_tick(&mut self, index: usize, at_us: u64) {
        let now_ms = at_us / 1000 / SERVICE_MS * SERVICE_MS;
        let apm = self.scenario.apm;
        let client = &mut self.clients[index];
        if apm > 0.0 && client.model.started() {
            let mean_us = 60_000_000.0 / apm;
            let next = client
                .next_command_us
                .get_or_insert_with(|| at_us + client.commands.exponential(mean_us) as u64);
            while *next <= at_us {
                client.model.queue_command(*next);
                *next += client.commands.exponential(mean_us) as u64;
            }
        }
        client.model.tick(now_ms, at_us);
        for bytes in client.model.take_output() {
            self.send_up(index, bytes, at_us);
        }
        self.schedule(at_us + SERVICE_MS * 1000, Event::ClientTick(index));
    }

    /// Runs the scenario to its end and reports the measurement window.
    pub fn run(mut self) -> Report {
        let (_, end) = self.window();
        while let Some(Queued { at_us, event, .. }) = self.queue.pop() {
            if at_us >= end {
                break;
            }
            match event {
                Event::RelayPass => {
                    self.relay_pass(at_us);
                    self.schedule(at_us + RELAY_PASS_US, Event::RelayPass);
                }
                Event::ClientTick(index) => self.client_tick(index, at_us),
                Event::ToRelay(index, bytes) => {
                    let now_ms = at_us / 1000;
                    let endpoint = self.clients[index].endpoint;
                    match self
                        .relay
                        .process_at(endpoint, &bytes, unix(now_ms), now_ms)
                    {
                        Ok(output) => {
                            self.route(output, at_us);
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
                    let now_ms = at_us / 1000 / SERVICE_MS * SERVICE_MS;
                    let client = &mut self.clients[index];
                    client.model.deliver(&bytes, now_ms);
                    for bytes in client.model.take_output() {
                        self.send_up(index, bytes, at_us);
                    }
                }
            }
        }
        self.report()
    }

    fn report(&self) -> Report {
        let (from, to) = self.window();
        let seconds = (to - from) as f64 / 1e6;
        let rate = self.scenario.tick_hz;
        let turn_ms = f64::from(turn_frames(rate)) * 1000.0 / f64::from(rate);
        let in_window = |at: u64| (from..to).contains(&at);
        let count = |times: &[u64]| times.iter().filter(|at| in_window(**at)).count() as f64;
        let mut rows = Vec::new();
        for client in &self.clients {
            let observed = &client.observed;
            let log = &client.model.log;
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
            let row = ClientReport {
                label: client.label.clone(),
                clients: 1,
                up: direction(&observed.up, observed.lost_up),
                down: direction(&observed.down, observed.lost_down),
                command_latency_ms: Distribution::of(latencies.clone()),
                turn_interval_ms: Distribution::of(turns.clone()),
                hitches: turns.iter().filter(|gap| **gap > 2.0 * turn_ms).count() as f64,
                gaps: count(&log.gaps),
                pulls: pulls as f64,
                repair_pulls: count(&log.repair_pulls),
                resends: count(&log.resends),
                submits_per_batch: if batches.is_empty() {
                    0.0
                } else {
                    batches.iter().sum::<u64>() as f64 / batches.len() as f64
                },
                batches: batches.len() as f64,
                executed_frames: client
                    .model
                    .reader()
                    .read()
                    .iter()
                    .filter(|record| matches!(record, Record::FrameEnd { .. }))
                    .count() as f64,
                errors: to_f64(&log.errors),
                refused: to_f64(&log.refused),
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
            .map(|client| client.model.reader().read())
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
                    .model
                    .log
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
mod tests;
