//! Pace replays: modelled PCs against the real relay.
//!
//! The relay is the real one, and so is the rule a player judges its pace by
//! (`PaceLag`, as the game's pace meter does). Every 100 ms the clock seals
//! what is due, and each modelled PC runs the frames waiting for it, up to
//! what it can in that time. A PC runs no faster while it catches up unless
//! its model says so: the game's catch-up mostly cannot skip drawing (the
//! simulation runs beside the picture), so a slow PC catches up as slowly as
//! it plays. Each PC reports its pace and tick cost once a second and its
//! sampled checksums as it runs them, as the game does.
//!
//! The pacing contract ([`Limits`]) is judged on what the clock did and on
//! the "Catching up with the match" line as the game shows it: while more
//! than half a second of frames waits (`catching_up_frames` in the game's
//! `net/live_commands/sequenced.rs`). The pace meter reports a backlog of an
//! eighth of that after `PACE_BACKLOG_MS`, so the relay hears every backlog
//! the line shows.
//!
//! Matches behind the scenarios:
//! - 2026-11-21, 1e9fac21 (2 players, 12 AI, 60 Hz): a GT 1030 ran about 55
//!   frames a second at 8 ms ticks. The relay read 8 ms as "holds 100 Hz,
//!   only draws slowly", never slowed, and the player fell 3560 frames
//!   behind in 828 seconds.
//! - 2026-11-21, 785b3c9b (28 AI): about 35 frames a second at 15 ms ticks;
//!   the clock stopped one row down.
//! - 2026-11-22: after the measured-shortfall fix, a dip of a few seconds
//!   held the whole room slow for five minutes.
//! - 2026-11-24, continuous pacing: a PC drawing 2588 units ran about 36
//!   frames a second. The clock settled at 95% of that, where the standing
//!   backlog rule judged "it drains itself", and the player's catching-up
//!   line stayed on for minutes.

use super::*;

/// A value of a match second: capacity in frames a second, a tick cost in
/// microseconds, or (above zero) a hidden window.
type Curve = Box<dyn Fn(f64) -> f64>;

/// A modelled PC.
pub(super) struct Pc {
    /// Frames it runs in a second at a match second.
    capacity: Curve,
    /// Frames it runs in a second while frames wait on it, given its
    /// capacity: the same unless catch-up skips drawing.
    catch_up: fn(f64) -> f64,
    /// The 90th-percentile tick cost it reports, in microseconds.
    tick_micros: Curve,
    /// Whether its window is hidden (the game then counts no backlog).
    hidden: Curve,
    /// Each second's capacity is scaled by a factor within this share.
    jitter: f64,
}

impl Pc {
    /// A PC limited by drawing: its ticks are quick, whatever it runs.
    fn drawing(capacity: impl Fn(f64) -> f64 + 'static, tick_micros: u32) -> Self {
        Self {
            capacity: Box::new(capacity),
            catch_up: |capacity| capacity,
            tick_micros: Box::new(move |_| f64::from(tick_micros)),
            hidden: Box::new(|_| 0.0),
            jitter: 0.0,
        }
    }

    fn fast() -> Self {
        Self::drawing(|_| 400.0, 4_500)
    }
}

/// A match to replay: the fast host first, then `pcs`.
pub(super) struct Match {
    tick_hz: u16,
    pacing: CertificatePacing,
    seconds: u64,
    pcs: Vec<Pc>,
    seed: u64,
}

impl Match {
    fn new(tick_hz: u16, seconds: u64, pcs: Vec<Pc>) -> Self {
        Self {
            tick_hz,
            pacing: CertificatePacing::Continuous,
            seconds,
            pcs,
            seed: 7,
        }
    }

    fn paced(mut self, pacing: CertificatePacing) -> Self {
        self.pacing = pacing;
        self
    }
}

/// What a replay saw of one modelled PC (not the fast host).
#[derive(Debug, Default, Clone)]
pub(super) struct Seen {
    /// Seconds the PC takes to run what waits on it: how late its player's
    /// orders and everyone else's reach its screen.
    max_backlog_s: f64,
    /// The second it was furthest behind.
    max_backlog_at: u64,
    final_backlog_s: f64,
    /// The longest unbroken run of the catching-up line, in seconds.
    longest_line_s: f64,
    /// Seconds the catching-up line was on.
    line_s: f64,
}

#[derive(Debug, Default)]
pub(super) struct Outcome {
    seen: Vec<Seen>,
    slowdowns: u64,
    raises: u64,
    /// `(second, rate)` at each change, starting with the certificate's.
    rates: Vec<(u64, u16)>,
    /// The clock's mean rate over the replay's last fifth.
    late_rate: f64,
    /// Frames the clock sealed over what the slowest visible PC could run,
    /// each second the least it ran within a minute either side (no more
    /// than the certificate's rate, no less than the floor): how much of
    /// what the room could play it played. The room is paced at a PC's low
    /// points of the last two minutes (`RAISE_AFTER_MAX_MS`): a PC that slows
    /// for eight seconds of every forty-five is paced for the eight.
    efficiency: f64,
}

impl Outcome {
    fn slow(&self) -> &Seen {
        &self.seen[0]
    }
}

struct Player {
    client: ClientSession,
    lag: PaceLag,
    done: u32,
    budget: f64,
    ran: u32,
    line_since: Option<u64>,
    seen: Seen,
    factor: f64,
}

/// Replays `game`: the host (a fast PC) is source 0, `game.pcs` follow.
pub(super) fn replay(game: &Match) -> Outcome {
    let key = SecretKey::new([73; 32]);
    let sources = game.pcs.len() + 1;
    let certificate = OrderingCertificate {
        room: [5; 16],
        sources_mask: (1 << sources) - 1,
        tick_hz: game.tick_hz,
        expires: 3000,
        pacing: game.pacing,
        ordered_alone: false,
    };
    let cert = certificate.sign(&key).unwrap();
    let mut server = RelayServer::new(key.clone(), 2);
    let host = Pc::fast();
    let pcs: Vec<&Pc> = std::iter::once(&host).chain(&game.pcs).collect();
    let mut players: Vec<Player> = (0..sources)
        .map(|source| {
            let mut client = client(&key, source as u8);
            connect(&mut server, &mut client, 0);
            configure(&mut server, &mut client, &cert, 0);
            Player {
                client,
                lag: PaceLag::default(),
                done: 0,
                budget: 0.0,
                ran: 0,
                line_since: None,
                seen: Seen::default(),
                factor: 1.0,
            }
        })
        .collect();
    let start = ordering(&server).starts_at.unwrap();
    let mut random = game.seed.max(1);
    let mut next = move || {
        // xorshift64: the same jitter for the same seed.
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        (random >> 11) as f64 / (1u64 << 53) as f64
    };
    let mut outcome = Outcome::default();
    let mut last_rate = ordering(&server).pace.rate;
    outcome.rates.push((0, last_rate));
    let (mut sealed_frames, mut late_frames) = (0.0, 0u64);
    let mut slowest_by_second = vec![f64::from(game.tick_hz); game.seconds as usize];
    let floor = CONTINUOUS_FLOOR.min(game.tick_hz);
    for step in 1..=game.seconds * 10 {
        let at = start + step * 100;
        let second = (step - 1) / 10;
        let time = (step - 1) as f64 / 10.0;
        if step % 1000 == 0 {
            for player in &mut players {
                renew(&mut server, &mut player.client, at);
            }
        }
        server.advance_clock(at, unix(at));
        let head = ordering(&server).head;
        let rate = ordering(&server).pace.rate;
        let turn = turn_frames(rate);
        let mut slowest = f64::from(game.tick_hz);
        for (index, (player, pc)) in players.iter_mut().zip(&pcs).enumerate() {
            if step % 10 == 1 {
                player.factor = 1.0 + pc.jitter * (2.0 * next() - 1.0);
            }
            let capacity = (pc.capacity)(time).max(0.0) * player.factor;
            let hidden = (pc.hidden)(time) > 0.0;
            if index > 0 && !hidden {
                slowest = slowest.min(capacity);
            }
            let waiting = head - player.done;
            let speed = if waiting > turn {
                (pc.catch_up)(capacity)
            } else {
                capacity
            };
            // Unused time does not carry over: a PC cannot run frames the
            // clock has not sealed.
            player.budget = (player.budget + speed / 10.0).min(speed / 10.0 + 1.0);
            let ran = (player.budget as u32).min(waiting);
            player.budget -= f64::from(ran);
            let before = player.done;
            player.done += ran;
            player.ran += ran;
            let sampled = player.done / CHECKSUM_INTERVAL * CHECKSUM_INTERVAL;
            if sampled > before && sampled > 0 {
                let checksum = |frame: u32| frame.wrapping_mul(2_654_435_761);
                let mut samples = vec![(sampled, checksum(sampled))];
                if sampled > CHECKSUM_INTERVAL {
                    let previous = sampled - CHECKSUM_INTERVAL;
                    samples.insert(0, (previous, checksum(previous)));
                }
                player
                    .client
                    .queue_ordering_checksums(&samples, at)
                    .unwrap();
            }
            let backlog = head - player.done;
            let counted = if hidden { 0 } else { backlog as usize };
            player.lag.observe(at as u32, counted, rate);
            if step % 10 == 0 {
                let pace = player.lag.classify(at as u32, player.ran as u16, rate);
                let tick = (pc.tick_micros)(time).max(1.0) as u32;
                player
                    .client
                    .queue_ordering_pace(pace, player.ran as u16, Some(tick));
                player.ran = 0;
            }
            let mut bytes = Vec::new();
            player.client.test_uplink(None, at, &mut bytes).unwrap();
            // Entries the relay pushes back are not this replay's concern.
            server
                .process_at(endpoint(&player.client), &bytes, unix(at), at)
                .unwrap();
            // The game's line: more than half a second of frames waiting.
            let line = !hidden && backlog > u32::from(rate) / 2;
            if index == 0 {
                continue;
            }
            let seen = &mut player.seen;
            // Orders wait behind the backlog for as long as the PC takes to
            // run it; a stalled PC's grows as the clock seals.
            let runs = if speed >= 1.0 { speed } else { f64::from(rate) };
            let backlog_s = f64::from(backlog) / runs;
            if backlog_s > seen.max_backlog_s {
                seen.max_backlog_s = backlog_s;
                seen.max_backlog_at = second;
            }
            seen.final_backlog_s = backlog_s;
            if line {
                seen.line_s += 0.1;
                let from = *player.line_since.get_or_insert(step);
                seen.longest_line_s = seen.longest_line_s.max((step - from + 1) as f64 / 10.0);
            } else {
                player.line_since = None;
            }
        }
        let rate = ordering(&server).pace.rate;
        sealed_frames += f64::from(rate) / 10.0;
        let least = &mut slowest_by_second[second as usize];
        *least = least.min(slowest);
        if step % 10 == 0 {
            if second >= game.seconds * 4 / 5 {
                late_frames += u64::from(rate);
            }
            if rate != last_rate {
                outcome.rates.push((second + 1, rate));
                last_rate = rate;
            }
        }
    }
    let counts = server.take_ordering_counts();
    outcome.slowdowns = counts.slowdowns;
    outcome.raises = counts.raises;
    outcome.late_rate = late_frames as f64 / (game.seconds - game.seconds * 4 / 5) as f64;
    let possible: f64 = (0..slowest_by_second.len())
        .map(|second| {
            let window = second.saturating_sub(60)..(second + 61).min(slowest_by_second.len());
            slowest_by_second[window]
                .iter()
                .copied()
                .fold(f64::MAX, f64::min)
                .clamp(f64::from(floor), f64::from(game.tick_hz))
        })
        .sum();
    outcome.efficiency = sealed_frames / possible;
    outcome.seen = players
        .into_iter()
        .skip(1)
        .map(|player| player.seen)
        .collect();
    outcome
}

/// The pacing contract for a match: what no visible player may see.
#[derive(Debug, Clone, Copy)]
pub(super) struct Limits {
    /// The catching-up line's longest unbroken run, in seconds.
    longest_line_s: f64,
    /// Its share of the match.
    line_share: f64,
    /// The most a player falls behind, in seconds of frames.
    max_backlog_s: f64,
    /// The least share of what the slowest PC could run that the room plays.
    efficiency: f64,
    /// The most swings a minute for each slow PC, on average: the game's
    /// speed must not go up and down. A swing is a slowdown after a raise;
    /// raises come in small steps a player does not see, and a slowdown may
    /// take two steps. Each PC's speed moves on its own, and finding where
    /// it went takes trying, so the room may swing for each.
    swings_per_minute: f64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            longest_line_s: 30.0,
            line_share: 0.10,
            max_backlog_s: 3.0,
            efficiency: 0.85,
            swings_per_minute: 1.0,
        }
    }
}

/// Every breach of `limits` in `outcome`, for one assertion message.
pub(super) fn breaches(outcome: &Outcome, seconds: u64, limits: Limits) -> Vec<String> {
    let mut found = Vec::new();
    for (index, seen) in outcome.seen.iter().enumerate() {
        let pc = index + 1;
        if seen.longest_line_s > limits.longest_line_s {
            found.push(format!(
                "pc {pc}: catching up {:.1}s without a break",
                seen.longest_line_s
            ));
        }
        if seen.line_s > limits.line_share * seconds as f64 {
            found.push(format!(
                "pc {pc}: catching up {:.0}s of {seconds}s",
                seen.line_s
            ));
        }
        if seen.max_backlog_s > limits.max_backlog_s {
            found.push(format!(
                "pc {pc}: {:.1}s behind at {}s",
                seen.max_backlog_s, seen.max_backlog_at
            ));
        }
    }
    let swings = outcome
        .rates
        .windows(3)
        .filter(|w| w[1].1 > w[0].1 && w[2].1 < w[1].1)
        .count() as f64
        / (seconds as f64 / 60.0);
    if swings > limits.swings_per_minute * outcome.seen.len() as f64 {
        found.push(format!("swung {swings:.1} times a minute"));
    }
    if outcome.efficiency < limits.efficiency {
        found.push(format!(
            "the room played {:.0}% of what it could",
            outcome.efficiency * 100.0
        ));
    }
    found
}

fn assert_contract(name: &str, game: &Match, limits: Limits) -> Outcome {
    let outcome = replay(game);
    let found = breaches(&outcome, game.seconds, limits);
    assert!(
        found.is_empty(),
        "{name}: {}\nrates {:?}\n{outcome:?}",
        found.join("; "),
        outcome.rates
    );
    outcome
}

/// A match's first minute is light: a PC runs the certificate's rate at the
/// start and reaches `capacity` 45 seconds in.
fn settling(tick_hz: u16, capacity: impl Fn(f64) -> f64) -> impl Fn(f64) -> f64 {
    let start = ramp(0.0, 45.0, f64::from(tick_hz), 0.0);
    move |second| {
        if second < 45.0 {
            let share = second / 45.0;
            start(second) + capacity(second) * share
        } else {
            capacity(second)
        }
    }
}

/// A capacity that goes from `from` to `to` frames a second between
/// `start` and `end` seconds, linearly.
fn ramp(start: f64, end: f64, from: f64, to: f64) -> impl Fn(f64) -> f64 {
    move |second| {
        let share = ((second - start) / (end - start)).clamp(0.0, 1.0);
        from + (to - from) * share
    }
}

/// Match 1e9fac21: about 55 frames a second at 8 ms ticks, for 828 seconds.
#[test]
fn a_pc_that_runs_below_the_rate_though_its_ticks_are_quick_slows_the_clock() {
    let pc = || Pc::drawing(settling(60, |_| 55.0), 8_000);
    let rows = replay(&Match::new(60, 828, vec![pc()]).paced(CertificatePacing::On));
    assert!(rows.slowdowns >= 1, "{rows:?}");
    assert!(rows.slow().max_backlog_s <= 5.0, "{rows:?}");
    assert_contract(
        "1e9fac21",
        &Match::new(60, 828, vec![pc()]),
        Limits::default(),
    );
}

/// Match 785b3c9b: modelled as a PC that runs about 35 frames a second at
/// 15 ms ticks. Rows must settle at 30, trying a faster rate again at most
/// every five minutes; continuous pacing just under 35.
#[test]
fn the_clock_settles_at_a_rate_a_slow_pc_holds() {
    let pc = || Pc::drawing(settling(60, |_| 35.0), 15_000);
    let rows = replay(&Match::new(60, 972, vec![pc()]).paced(CertificatePacing::On));
    assert!(rows.slow().max_backlog_s <= 5.0, "{rows:?}");
    assert!(rows.slow().line_s <= 972.0 / 10.0, "{rows:?}");
    assert!(rows.raises <= 972 / 300 + 1, "{rows:?}");
    let game = Match::new(60, 972, vec![pc()]);
    assert_contract("785b3c9b", &game, Limits::default());
}

/// VO2C flags bit 2 certifies continuous pacing; with bit 0 (pacing off) it
/// is malformed. Under it any whole rate from the certificate's down to the
/// floor is admitted; Game Speed rows stay the only rates otherwise.
#[test]
fn a_continuous_certificate_admits_whole_rates_down_to_the_floor() {
    let key = SecretKey::new([73; 32]);
    let certificate = OrderingCertificate {
        room: [5; 16],
        sources_mask: 3,
        tick_hz: 60,
        expires: 1400,
        pacing: CertificatePacing::Continuous,
        ordered_alone: false,
    };
    let mut signed = certificate.sign(&key).unwrap();
    assert_eq!(signed[34], 4);
    assert_eq!(OrderingCertificate::parse(&signed).unwrap(), certificate);
    signed[34] = 5;
    assert_eq!(
        OrderingCertificate::parse(&signed),
        Err(RelayError::Malformed)
    );
    // The lobby's signer (nakama/vera/internal/relay, ordered alone and
    // continuous) gives the same bytes for key 0x00..0x1f, room 0x10..0x1f.
    let mut lobby_key = [0; 32];
    lobby_key
        .iter_mut()
        .enumerate()
        .for_each(|(i, b)| *b = i as u8);
    let mut room = [0; 16];
    room.iter_mut()
        .enumerate()
        .for_each(|(i, b)| *b = 0x10 + i as u8);
    let lobby = OrderingCertificate {
        room,
        sources_mask: 1,
        tick_hz: 45,
        expires: 87_400,
        pacing: CertificatePacing::Continuous,
        ordered_alone: true,
    };
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        hex(&lobby.sign(&SecretKey::new(lobby_key)).unwrap()),
        "564f3243101112131415161718191a1b1c1d1e1f010000002d006855010000000000063c7300e29e35ee711fd08ee08af60c2a82bb1aa5695132d2ff938130151516ab"
    );
    use CertificatePacing::*;
    assert!(admits_rate(60, Continuous, 53) && admits_rate(60, Continuous, 10));
    assert!(!admits_rate(60, Continuous, 9) && !admits_rate(60, Continuous, 61));
    assert!(!admits_rate(60, On, 53) && admits_rate(60, On, 45));
    assert!(admits_rate(60, Off, 60) && !admits_rate(60, Off, 45));
}
