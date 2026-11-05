//! Load generator for a test relay: many fake players in many rooms over
//! real UDP sockets. Each player is the network simulation's headless
//! client (`netsim::adaptive_client`): the production `ClientSession`
//! decides every UPLINK, PULL and heartbeat; the client adds only content
//! (Poisson commands, checksums every 15 frames, pace each second), the
//! stream reader and the hop handshake. It reports how long a player's
//! command takes to run on its own PC, how evenly frame ends arrive, and
//! the datagrams and bytes each way.
//!
//! The ticket signing key arrives as 64 hex digits on stdin. Use a throwaway
//! key and relay, never a live relay's key: the generator signs its own tickets.
//!
//! ```sh
//! cargo run --release -p vera20k-relay-protocol --features test-support \
//!     --example relay_load -- 127.0.0.1:5000 --rooms 4 --players 8 \
//!     --tick-hz 60 --seconds 60 < key.hex
//! ```
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vera20k_relay_protocol::envelope::MAX_DATAGRAM;
use vera20k_relay_protocol::netsim::adaptive_client::AdaptiveClient;
use vera20k_relay_protocol::netsim::client::HeadlessClient;
use vera20k_relay_protocol::netsim::network::Rng;
use vera20k_relay_protocol::{
    ClientSession, MAX_TICKET_LIFETIME, OrderingCertificate, SecretKey, Ticket, derive_packet_key,
};

/// `ClientSession::new` still takes the lobby's payload key; batches are
/// plaintext and it goes unused (bandwidth-compaction spec §3.2).
const PAYLOAD_KEY: [u8; 32] = [89; 32];
/// Plaintext bytes per command, a move's records as the network
/// simulation's default (bandwidth-compaction spec §2, §4).
const COMMAND_BYTES: usize = 19;
/// The game loop's service period (spec §6.3).
const SERVICE_MS: u64 = 16;
/// A big room's players run on several threads, so the generator is not
/// what limits a 30-player match.
const PLAYERS_PER_THREAD: usize = 4;

struct Options {
    relay: SocketAddrV4,
    bind: Ipv4Addr,
    rooms: usize,
    players: u8,
    tick_hz: u16,
    seconds: u64,
    warmup: u64,
    apm: u32,
}

fn options() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let relay = args
        .next()
        .ok_or("usage: relay_load <relay ipv4:port> [--rooms N] [--players N] [--tick-hz N] [--seconds N] [--warmup N] [--apm N] [--bind IPV4]")?
        .parse()
        .map_err(|_| "relay address must be ipv4:port")?;
    let mut options = Options {
        relay,
        bind: Ipv4Addr::UNSPECIFIED,
        rooms: 1,
        players: 8,
        tick_hz: 60,
        seconds: 60,
        warmup: 10,
        apm: 150,
    };
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("{flag} needs a value"))?;
        let bad = || format!("bad value for {flag}");
        match flag.as_str() {
            "--rooms" => options.rooms = value.parse().map_err(|_| bad())?,
            "--players" => options.players = value.parse().map_err(|_| bad())?,
            "--tick-hz" => options.tick_hz = value.parse().map_err(|_| bad())?,
            "--seconds" => options.seconds = value.parse().map_err(|_| bad())?,
            "--warmup" => options.warmup = value.parse().map_err(|_| bad())?,
            "--apm" => options.apm = value.parse().map_err(|_| bad())?,
            "--bind" => options.bind = value.parse().map_err(|_| bad())?,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    // The relay opens no ordering room for a single source.
    if !(2..=30).contains(&options.players) {
        return Err("--players must be 2..=30".into());
    }
    if ![60, 45, 30, 20, 15, 12, 10].contains(&options.tick_hz) {
        return Err("--tick-hz must be 60, 45, 30, 20, 15, 12 or 10".into());
    }
    // Tickets cannot outlive MAX_TICKET_LIFETIME, and this tool never renews.
    if options.warmup + options.seconds + 30 > MAX_TICKET_LIFETIME {
        return Err(format!(
            "--warmup + --seconds must stay under {} s",
            MAX_TICKET_LIFETIME - 30
        ));
    }
    Ok(options)
}

fn key_from_stdin() -> Result<SecretKey, String> {
    let mut text = String::new();
    std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
        .map_err(|_| "cannot read key from stdin")?;
    let text = text.trim().as_bytes();
    if text.len() != 64 || !text.iter().all(u8::is_ascii_hexdigit) {
        return Err("stdin must hold the relay's 64-hex-digit test key".into());
    }
    let mut key = [0u8; 32];
    for (index, pair) in text.chunks_exact(2).enumerate() {
        key[index] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
    }
    Ok(SecretKey::new(key))
}

#[derive(Default)]
struct Stats {
    /// Command issued to its frame run on the issuing PC, in ms.
    own_batch_ms: Vec<u32>,
    /// Gap between consecutive frame-end advances at one player, in ms.
    frame_gap_ms: Vec<u32>,
    batches_sent: u64,
    datagrams_sent: u64,
    datagrams_received: u64,
    bytes_sent: u64,
    bytes_received: u64,
    pulls: u64,
    resends: u64,
    socket_errors: u64,
    decode_errors: u64,
    /// Relay datagrams whose 4-byte tag failed at a player
    /// (`ClientSession::mac_failures`, spec §6.4 item 6).
    mac_failures: u64,
    ordering_errors: u64,
    stalled_players: u64,
    errors: BTreeMap<String, u64>,
}

impl Stats {
    fn merge(&mut self, other: Stats) {
        self.own_batch_ms.extend(other.own_batch_ms);
        self.frame_gap_ms.extend(other.frame_gap_ms);
        self.batches_sent += other.batches_sent;
        self.datagrams_sent += other.datagrams_sent;
        self.datagrams_received += other.datagrams_received;
        self.bytes_sent += other.bytes_sent;
        self.bytes_received += other.bytes_received;
        self.pulls += other.pulls;
        self.resends += other.resends;
        self.socket_errors += other.socket_errors;
        self.decode_errors += other.decode_errors;
        self.mac_failures += other.mac_failures;
        self.ordering_errors += other.ordering_errors;
        self.stalled_players += other.stalled_players;
        for (code, count) in other.errors {
            *self.errors.entry(code).or_default() += count;
        }
    }
}

struct Player {
    socket: UdpSocket,
    client: AdaptiveClient,
    next_command_us: Option<u64>,
    last_service_ms: Option<u64>,
}

impl Player {
    fn flush(&mut self, stats: &mut Stats) {
        for outgoing in self.client.take_output() {
            stats.datagrams_sent += 1;
            stats.bytes_sent += outgoing.bytes.len() as u64;
            if self.socket.send(&outgoing.bytes).is_err() {
                stats.socket_errors += 1;
            }
        }
    }
}

fn percentile(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
}

#[derive(Clone, Copy)]
struct Timing {
    started: Instant,
    command_mean_us: f64,
    measure_from_us: u64,
    end_us: u64,
}

fn run_room(options: &Options, key: &SecretKey, room_index: usize, started: Instant) -> Stats {
    let now_s = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let mut room = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos()
        .to_le_bytes();
    room[8..16].copy_from_slice(&(room_index as u64).to_le_bytes());
    let seed = u64::from_le_bytes(room[..8].try_into().unwrap());
    let expires = now_s + options.warmup + options.seconds + 30;
    let certificate = OrderingCertificate {
        room,
        sources_mask: (1u32 << options.players) - 1,
        tick_hz: options.tick_hz,
        expires,
        pacing: Default::default(),
    }
    .sign(key)
    .expect("certificate");
    let mut players = Vec::new();
    for source in 0..options.players {
        let mut player_id = room;
        player_id[0] ^= source + 1;
        let mut nonce = room;
        nonce[15] ^= source + 1;
        let ticket = Ticket {
            room,
            player: player_id,
            source,
            generation: 1,
            nonce,
            expires,
        }
        .sign(key)
        .expect("ticket");
        let socket = UdpSocket::bind((options.bind, 0)).expect("bind");
        socket.connect(options.relay).expect("connect");
        socket.set_nonblocking(true).expect("nonblocking");
        let session = ClientSession::new(
            &ticket,
            *derive_packet_key(key, &ticket).expose_for_https(),
            PAYLOAD_KEY,
        )
        .expect("client");
        players.push(Player {
            socket,
            client: AdaptiveClient::new(
                session,
                certificate.clone(),
                PAYLOAD_KEY,
                options.tick_hz,
                COMMAND_BYTES,
                None,
            ),
            next_command_us: None,
            last_service_ms: None,
        });
    }
    let timing = Timing {
        started,
        command_mean_us: if options.apm == 0 {
            f64::INFINITY
        } else {
            60_000_000.0 / f64::from(options.apm)
        },
        measure_from_us: options.warmup * 1_000_000,
        end_us: (options.warmup + options.seconds) * 1_000_000,
    };
    let mut stats = Stats::default();
    std::thread::scope(|scope| {
        let handles: Vec<_> = players
            .chunks_mut(PLAYERS_PER_THREAD)
            .enumerate()
            .map(|(chunk, players)| {
                scope.spawn(move || drive(players, timing, Rng::new(seed, chunk as u64)))
            })
            .collect();
        for handle in handles {
            stats.merge(handle.join().expect("player thread"));
        }
    });
    stats
}

fn drive(players: &mut [Player], timing: Timing, mut rng: Rng) -> Stats {
    let mut stats = Stats::default();
    let mut buffer = vec![0u8; MAX_DATAGRAM + 1];
    let elapsed_us = || timing.started.elapsed().as_micros() as u64;
    while elapsed_us() < timing.end_us {
        for player in players.iter_mut() {
            // Receive everything waiting, at its socket read time.
            loop {
                let length = match player.socket.recv(&mut buffer) {
                    Ok(length) => length,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        stats.socket_errors += 1;
                        break;
                    }
                };
                stats.datagrams_received += 1;
                stats.bytes_received += length as u64;
                let at_us = elapsed_us();
                player
                    .client
                    .deliver(&buffer[..length], at_us / 1000, at_us);
                player.flush(&mut stats);
            }
            let at_us = elapsed_us();
            if player.client.started() {
                let next = player.next_command_us.get_or_insert_with(|| {
                    at_us + rng.exponential(timing.command_mean_us).min(6e8) as u64
                });
                while *next <= at_us {
                    player.client.queue_command(*next);
                    *next += rng.exponential(timing.command_mean_us).min(6e8) as u64;
                }
            }
            let now_ms = at_us / 1000;
            if player
                .last_service_ms
                .is_none_or(|last| now_ms >= last + SERVICE_MS)
            {
                player.last_service_ms = Some(now_ms);
                player.client.tick(now_ms, at_us);
                player.flush(&mut stats);
            }
        }
        std::thread::sleep(Duration::from_micros(500));
    }
    let end_us = elapsed_us();
    let measured = |at: u64| at >= timing.measure_from_us;
    for player in players.iter_mut() {
        let log = player.client.log();
        for (issued, ran) in &log.commands {
            if measured(*issued) {
                stats.own_batch_ms.push(((ran - issued) / 1000) as u32);
            }
        }
        for pair in log.advances.windows(2) {
            if measured(pair[0]) {
                stats.frame_gap_ms.push(((pair[1] - pair[0]) / 1000) as u32);
            }
        }
        stats.batches_sent += log.seals.len() as u64;
        stats.pulls += log.repair_pulls.len() as u64;
        for (code, count) in &log.errors {
            stats.ordering_errors += count;
            *stats.errors.entry(code.clone()).or_default() += count;
        }
        stats.decode_errors += log.refused.values().sum::<u64>();
        stats.mac_failures += player.client.mac_failures();
        if let Some(link) = player.client.link_stats() {
            stats.resends += link.rto_timeouts;
        }
        if !player.client.started()
            || log
                .advances
                .last()
                .is_none_or(|last| end_us.saturating_sub(*last) > 2_000_000)
        {
            stats.stalled_players += 1;
        }
        // Leave: the client sends LEAVE at its next service.
        player.client.leave();
        player.client.tick(end_us / 1000, end_us);
        player.flush(&mut stats);
    }
    stats
}

fn main() {
    let options = match options() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let key = match key_from_stdin() {
        Ok(key) => key,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };
    let started = Instant::now();
    let mut total = Stats::default();
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..options.rooms)
            .map(|room| {
                let (options, key) = (&options, &key);
                scope.spawn(move || run_room(options, key, room, started))
            })
            .collect();
        for handle in handles {
            total.merge(handle.join().expect("room thread"));
        }
    });
    total.own_batch_ms.sort_unstable();
    total.frame_gap_ms.sort_unstable();
    let frame_ms = 1000 / u32::from(options.tick_hz);
    let hitches = total
        .frame_gap_ms
        .iter()
        .filter(|gap| **gap >= frame_ms * 3 + 20)
        .count();
    // Rates cover the warmup too: datagram counts are not split by phase.
    // Bytes are UDP payloads: the whole relay datagram, no IP/UDP header.
    let seconds = (options.warmup + options.seconds).max(1);
    println!(
        "{{\"rooms\":{},\"players_per_room\":{},\"tick_hz\":{},\"apm\":{},\"seconds\":{},\
\"own_batch_ms\":{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{},\"count\":{}}},\
\"frame_gap_ms\":{{\"p50\":{},\"p99\":{},\"max\":{},\"hitches\":{},\"count\":{}}},\
\"sent_per_s\":{},\"received_per_s\":{},\"sent_bytes_per_s\":{},\"received_bytes_per_s\":{},\
\"batches\":{},\"resends\":{},\"pulls\":{},\
\"socket_errors\":{},\"decode_errors\":{},\"mac_failures\":{},\"ordering_errors\":{},\"stalled_players\":{},\"errors\":{{{}}}}}",
        options.rooms,
        options.players,
        options.tick_hz,
        options.apm,
        options.seconds,
        percentile(&total.own_batch_ms, 0.50),
        percentile(&total.own_batch_ms, 0.95),
        percentile(&total.own_batch_ms, 0.99),
        total.own_batch_ms.last().copied().unwrap_or(0),
        total.own_batch_ms.len(),
        percentile(&total.frame_gap_ms, 0.50),
        percentile(&total.frame_gap_ms, 0.99),
        total.frame_gap_ms.last().copied().unwrap_or(0),
        hitches,
        total.frame_gap_ms.len(),
        total.datagrams_sent / seconds,
        total.datagrams_received / seconds,
        total.bytes_sent / seconds,
        total.bytes_received / seconds,
        total.batches_sent,
        total.resends,
        total.pulls,
        total.socket_errors,
        total.decode_errors,
        total.mac_failures,
        total.ordering_errors,
        total.stalled_players,
        total
            .errors
            .iter()
            .map(|(code, count)| format!("\"{code}\":{count}"))
            .collect::<Vec<_>>()
            .join(","),
    );
}
