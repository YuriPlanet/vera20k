//! Load generator for a test relay: many fake players in many rooms, each
//! sending what a live match client sends (`src/net/live_commands/sequenced.rs`).
//!
//! Per player: the handshake and ordering setup, a sync batch every 15 frames
//! plus one batch per command, each first sent twice and resent every 100 ms
//! until acknowledged, a push watermark every 100 ms, and a Pull when the
//! stream has a gap for longer than one frame. It reports how long a player's
//! own batch takes to come back in the stream and how evenly frame ends arrive.
//!
//! The ticket signing key arrives as 64 hex digits on stdin. Use a throwaway
//! key and relay, never a live relay's key: the generator signs its own tickets.
//!
//! ```sh
//! cargo run --release -p vera20k-relay-protocol --example relay_load -- \
//!     127.0.0.1:5000 --rooms 4 --players 8 --tick-hz 60 --seconds 60 < key.hex
//! ```
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use vera20k_relay_protocol::{
    ClientSession, MAX_DATAGRAM, MAX_TICKET_LIFETIME, OrderEntry, OrderEvent, OrderingCertificate,
    SecretKey, ServerEvent, Ticket, derive_packet_key,
};

const RESEND_MS: u64 = 100;
const WATERMARK_MS: u64 = 100;
const PULL_RETRY_MS: u64 = 100;
const SAMPLE_FRAMES: u32 = 15;
const SAMPLE_BYTES: usize = 40;
const COMMAND_BYTES: usize = 120;
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

/// xorshift64*: only spreads command times and fills payloads.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Exponential gap with the given mean, for Poisson command arrivals.
    fn exp_ms(&mut self, mean_ms: f64) -> u64 {
        let unit = (self.next() >> 11) as f64 / (1u64 << 53) as f64;
        (-(1.0 - unit).ln() * mean_ms) as u64
    }
}

#[derive(Default)]
struct Stats {
    /// Submit to the player's own batch in the stream, in ms.
    own_batch_ms: Vec<u32>,
    /// Gap between consecutive in-order frame ends at one player, in ms.
    frame_gap_ms: Vec<u32>,
    batches_sent: u64,
    datagrams_sent: u64,
    datagrams_received: u64,
    pulls: u64,
    resends: u64,
    socket_errors: u64,
    decode_errors: u64,
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
        self.pulls += other.pulls;
        self.resends += other.resends;
        self.socket_errors += other.socket_errors;
        self.decode_errors += other.decode_errors;
        self.ordering_errors += other.ordering_errors;
        self.stalled_players += other.stalled_players;
        for (code, count) in other.errors {
            *self.errors.entry(code).or_default() += count;
        }
    }
}

#[derive(PartialEq)]
enum Phase {
    Handshake,
    Configure,
    Running,
}

struct Pending {
    id: u64,
    bytes: Vec<u8>,
    last_sent: Option<Instant>,
}

struct Player {
    socket: UdpSocket,
    client: ClientSession,
    source: u8,
    phase: Phase,
    out: Vec<u8>,
    next_batch: u64,
    pending: VecDeque<Pending>,
    submitted_at: HashMap<u64, Instant>,
    next_sequence: u64,
    ahead: BTreeSet<u64>,
    gap_since: Option<Instant>,
    last_pull: Option<Instant>,
    last_watermark: Option<Instant>,
    watermark: u64,
    last_frame_end: Option<Instant>,
    last_handshake: Option<Instant>,
    next_sample: Instant,
    next_command: Instant,
    queued_commands: u32,
    last_progress: Instant,
}

impl Player {
    fn send(&self, stats: &mut Stats) {
        stats.datagrams_sent += 1;
        if self.socket.send(&self.out).is_err() {
            stats.socket_errors += 1;
        }
    }
}

fn percentile(sorted: &[u32], p: f64) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[((sorted.len() - 1) as f64 * p).round() as usize]
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
    let mut rng = Rng(u64::from_le_bytes(room[..8].try_into().unwrap()) | 1);
    let expires = now_s + options.warmup + options.seconds + 30;
    let sample_every =
        Duration::from_millis(u64::from(SAMPLE_FRAMES) * 1000 / u64::from(options.tick_hz));
    let command_mean_ms = if options.apm == 0 {
        f64::INFINITY
    } else {
        60_000.0 / f64::from(options.apm)
    };
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
        let client = ClientSession::new(
            &ticket,
            *derive_packet_key(key, &ticket).expose_for_https(),
            [89; 32],
        )
        .expect("client");
        let now = Instant::now();
        players.push(Player {
            socket,
            client,
            source,
            phase: Phase::Handshake,
            out: Vec::new(),
            next_batch: 1,
            pending: VecDeque::new(),
            submitted_at: HashMap::new(),
            next_sequence: 1,
            ahead: BTreeSet::new(),
            gap_since: None,
            last_pull: None,
            last_watermark: None,
            watermark: 1,
            last_frame_end: None,
            last_handshake: None,
            next_sample: now,
            next_command: now + Duration::from_millis(rng.exp_ms(command_mean_ms).min(600_000)),
            queued_commands: 0,
            last_progress: now,
        });
    }
    let sources_mask = (1u32 << options.players) - 1;
    let certificate = OrderingCertificate {
        room,
        sources_mask,
        tick_hz: options.tick_hz,
        expires,
    }
    .sign(key)
    .expect("certificate");
    let measure_from = started + Duration::from_secs(options.warmup);
    let end = measure_from + Duration::from_secs(options.seconds);
    let mut stats = Stats::default();
    std::thread::scope(|scope| {
        let handles: Vec<_> = players
            .chunks_mut(PLAYERS_PER_THREAD)
            .map(|chunk| {
                let certificate = &certificate[..];
                let seed = rng.next() | 1;
                let timing = Timing {
                    sample_every,
                    command_mean_ms,
                    measure_from,
                    end,
                };
                scope.spawn(move || drive(chunk, certificate, options, timing, seed))
            })
            .collect();
        for handle in handles {
            stats.merge(handle.join().expect("player thread"));
        }
    });
    stats
}

#[derive(Clone, Copy)]
struct Timing {
    sample_every: Duration,
    command_mean_ms: f64,
    measure_from: Instant,
    end: Instant,
}

fn drive(
    players: &mut [Player],
    certificate: &[u8],
    options: &Options,
    timing: Timing,
    seed: u64,
) -> Stats {
    let Timing {
        sample_every,
        command_mean_ms,
        measure_from,
        end,
    } = timing;
    let mut rng = Rng(seed);
    let mut stats = Stats::default();
    let mut buffer = vec![0u8; MAX_DATAGRAM + 1];
    while Instant::now() < end {
        let now = Instant::now();
        let measuring = now >= measure_from;
        for player in players.iter_mut() {
            // Receive everything waiting.
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
                let event = match player.client.receive(&buffer[..length]) {
                    Ok(event) => event,
                    Err(_) => {
                        stats.decode_errors += 1;
                        continue;
                    }
                };
                let arrived = Instant::now();
                match event {
                    ServerEvent::Challenge(cookie) => {
                        let cookie = cookie.to_vec();
                        if player.client.confirm(&cookie, &mut player.out).is_ok() {
                            player.send(&mut stats);
                        }
                    }
                    ServerEvent::Ready => {
                        if player.phase == Phase::Handshake {
                            player.phase = Phase::Configure;
                        }
                    }
                    ServerEvent::Payload { .. } => {}
                    ServerEvent::Ordering(event) => match event {
                        OrderEvent::Ready {
                            ready_mask,
                            sources_mask,
                        } if ready_mask == sources_mask => {
                            if player.phase == Phase::Configure {
                                player.phase = Phase::Running;
                                player.last_progress = arrived;
                            }
                        }
                        OrderEvent::Ack { batch_id, .. } => {
                            while player.pending.front().is_some_and(|p| p.id <= batch_id) {
                                player.pending.pop_front();
                            }
                        }
                        OrderEvent::Status(status) => {
                            if status.next_seq > player.next_sequence && player.gap_since.is_none()
                            {
                                player.gap_since = Some(arrived);
                            }
                        }
                        OrderEvent::Entry(entry) => {
                            let sequence = match &entry {
                                OrderEntry::Batch { sequence, .. }
                                | OrderEntry::FrameEnd { sequence, .. }
                                | OrderEntry::Departure { sequence, .. }
                                | OrderEntry::Pause { sequence, .. }
                                | OrderEntry::Rate { sequence, .. } => *sequence,
                            };
                            if sequence < player.next_sequence || player.ahead.contains(&sequence) {
                                continue;
                            }
                            if let OrderEntry::Batch {
                                source, batch_id, ..
                            } = &entry
                                && *source == player.source
                                && let Some(sent) = player.submitted_at.remove(batch_id)
                                && measuring
                            {
                                stats
                                    .own_batch_ms
                                    .push(arrived.duration_since(sent).as_millis() as u32);
                            }
                            if sequence == player.next_sequence {
                                player.next_sequence += 1;
                                while player.ahead.remove(&player.next_sequence) {
                                    player.next_sequence += 1;
                                }
                                player.last_progress = arrived;
                                if player.ahead.is_empty() {
                                    player.gap_since = None;
                                }
                            } else {
                                player.ahead.insert(sequence);
                                player.gap_since.get_or_insert(arrived);
                            }
                            // Gaps between frame ends as the player receives
                            // them, in or out of order.
                            if matches!(entry, OrderEntry::FrameEnd { .. }) {
                                if let Some(last) = player.last_frame_end
                                    && measuring
                                {
                                    stats
                                        .frame_gap_ms
                                        .push(arrived.duration_since(last).as_millis() as u32);
                                }
                                player.last_frame_end = Some(arrived);
                            }
                        }
                        // Start-up replies such as NotStarted stay out.
                        OrderEvent::Error { code, .. } if measuring => {
                            stats.ordering_errors += 1;
                            *stats.errors.entry(format!("{code:?}")).or_default() += 1;
                        }
                        _ => {}
                    },
                }
            }
            // Send what a live client would now.
            match player.phase {
                Phase::Handshake => {
                    if player
                        .last_handshake
                        .is_none_or(|last| now.duration_since(last).as_millis() >= 250)
                        && player.client.probe(&mut player.out).is_ok()
                    {
                        player.send(&mut stats);
                        player.last_handshake = Some(now);
                    }
                }
                Phase::Configure => {
                    if player
                        .last_handshake
                        .is_none_or(|last| now.duration_since(last).as_millis() >= 125)
                        && player
                            .client
                            .configure_ordering(certificate, [7; 32], &mut player.out)
                            .is_ok()
                    {
                        player.send(&mut stats);
                        player.last_handshake = Some(now);
                    }
                }
                Phase::Running => {
                    if now >= player.next_command {
                        player.queued_commands += 1;
                        player.next_command =
                            now + Duration::from_millis(rng.exp_ms(command_mean_ms).min(600_000));
                    }
                    let sample_due = now >= player.next_sample;
                    if (sample_due || player.queued_commands > 0)
                        && player.pending.len() < ClientSession::ORDERING_SUBMISSION_WINDOW
                    {
                        let size = if player.queued_commands > 0 {
                            COMMAND_BYTES
                        } else {
                            SAMPLE_BYTES
                        };
                        let mut bytes = vec![1u8; size];
                        for byte in &mut bytes[1..] {
                            *byte = rng.next() as u8;
                        }
                        player.pending.push_back(Pending {
                            id: player.next_batch,
                            bytes,
                            last_sent: None,
                        });
                        player.submitted_at.insert(player.next_batch, now);
                        player.next_batch += 1;
                        player.queued_commands = 0;
                        if sample_due {
                            player.next_sample = now + sample_every;
                        }
                        stats.batches_sent += 1;
                    }
                    for index in 0..player.pending.len() {
                        let pending = &player.pending[index];
                        if pending.last_sent.is_some_and(|last| {
                            now.duration_since(last).as_millis() < RESEND_MS as u128
                        }) {
                            continue;
                        }
                        let copies = if pending.last_sent.is_none() { 2 } else { 1 };
                        if pending.last_sent.is_some() {
                            stats.resends += 1;
                        }
                        let (id, bytes) = (pending.id, pending.bytes.clone());
                        for _ in 0..copies {
                            if player
                                .client
                                .submit_ordering(id, &bytes, &mut player.out)
                                .is_ok()
                            {
                                player.send(&mut stats);
                            }
                        }
                        player.pending[index].last_sent = Some(now);
                    }
                    if (player.last_watermark.is_none_or(|last| {
                        now.duration_since(last).as_millis() >= WATERMARK_MS as u128
                    }) || player.next_sequence.saturating_sub(player.watermark) >= 32)
                        && player
                            .client
                            .set_ordering_push(true, player.next_sequence, &mut player.out)
                            .is_ok()
                    {
                        player.send(&mut stats);
                        player.last_watermark = Some(now);
                        player.watermark = player.next_sequence;
                    }
                    let frame = Duration::from_millis(1000 / u64::from(options.tick_hz))
                        .max(Duration::from_millis(1));
                    let gap_ready = player
                        .gap_since
                        .is_some_and(|since| now.duration_since(since) >= frame);
                    let silent = now.duration_since(player.last_progress) > Duration::from_secs(1);
                    if (gap_ready || silent)
                        && player.last_pull.is_none_or(|last| {
                            now.duration_since(last).as_millis() >= PULL_RETRY_MS as u128
                        })
                        && player
                            .client
                            .pull_ordering(player.next_sequence, &mut player.out)
                            .is_ok()
                    {
                        player.send(&mut stats);
                        player.last_pull = Some(now);
                        stats.pulls += 1;
                    }
                }
            }
        }
        std::thread::sleep(Duration::from_micros(500));
    }
    let now = Instant::now();
    for player in players.iter_mut() {
        if player.phase != Phase::Running
            || now.duration_since(player.last_progress) > Duration::from_secs(2)
        {
            stats.stalled_players += 1;
        }
        if player.client.leave_ordering(&mut player.out).is_ok() {
            player.send(&mut stats);
        }
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
    let seconds = (options.warmup + options.seconds).max(1);
    println!(
        "{{\"rooms\":{},\"players_per_room\":{},\"tick_hz\":{},\"apm\":{},\"seconds\":{},\
\"own_batch_ms\":{{\"p50\":{},\"p95\":{},\"p99\":{},\"max\":{},\"count\":{}}},\
\"frame_gap_ms\":{{\"p50\":{},\"p99\":{},\"max\":{},\"hitches\":{},\"count\":{}}},\
\"sent_per_s\":{},\"received_per_s\":{},\"batches\":{},\"resends\":{},\"pulls\":{},\
\"socket_errors\":{},\"decode_errors\":{},\"ordering_errors\":{},\"stalled_players\":{},\"errors\":{{{}}}}}",
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
        total.batches_sent,
        total.resends,
        total.pulls,
        total.socket_errors,
        total.decode_errors,
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
