//! Replays of real matches whose slow player the clock did not wait for.
//!
//! The relay is the real one; the players are models of the client's pace
//! meter (`src/net/live_commands/sequenced.rs`, `PaceMeter`): each second a
//! player runs the frames waiting for it, up to what its PC can, and reports
//! KeepingUp, CatchingUp or Behind with what it ran and its tick cost.
//!
//! The 2026-11-21 report (match 1e9fac21, 2 players and 12 AI at 60 Hz): one
//! PC (GT 1030) ran about 55 frames a second, though its 90th-percentile tick
//! cost was about 8 ms. The relay read 8 ms as "holds 100 Hz, only draws
//! slowly" and never slowed, so in 828 seconds that player fell 3560 frames
//! behind and its orders took 5 seconds on average to take effect.

use super::*;

/// A modelled player: the frames its PC runs in a second and the tick cost it
/// reports.
#[derive(Clone, Copy)]
struct Player {
    capacity: u32,
    tick_micros: u32,
}

/// What the replay saw of the slow player.
#[derive(Debug, Default)]
struct Outcome {
    max_backlog: u32,
    final_backlog: u32,
    catching_up_seconds: u32,
    slowdowns: u64,
    raises: u64,
    rates: Vec<(u64, u16)>,
}

/// The client's pace meter, sampled once a second (`PaceMeter::report`).
#[derive(Default)]
struct Meter {
    backlog_since: Option<u64>,
    slow_windows: u8,
}

impl Meter {
    fn report(&mut self, now_ms: u64, backlog: u32, ran: u32, rate: u16) -> OrderPace {
        if backlog >= (u32::from(rate) / 8).max(4) {
            self.backlog_since.get_or_insert(now_ms);
        } else {
            self.backlog_since = None;
        }
        self.slow_windows = if ran > 0 && ran < u32::from(rate) && self.backlog_since.is_some() {
            self.slow_windows.saturating_add(1)
        } else {
            0
        };
        let lagging = self
            .backlog_since
            .is_some_and(|since| now_ms - since >= 2_000);
        match (lagging, self.slow_windows >= 2) {
            (false, _) => OrderPace::KeepingUp,
            (true, true) => OrderPace::Behind,
            (true, false) => OrderPace::CatchingUp,
        }
    }
}

fn long_client(key: &SecretKey, source: u8) -> ClientSession {
    client_for(
        key,
        Ticket {
            room: [5; 16],
            source,
            expires: 1200,
            nonce: [source + 1; 16],
            player: [source + 1; 16],
            generation: 1,
        },
    )
}

/// Play `seconds` of a 60 Hz match between a fast player and `slow`.
fn replay(slow: Player, seconds: u64) -> Outcome {
    let key = SecretKey::new([73; 32]);
    let mut server = RelayServer::new(key.clone(), 2);
    let (mut fast_client, mut slow_client) = (long_client(&key, 0), long_client(&key, 1));
    let cert = OrderingCertificate {
        room: [5; 16],
        sources_mask: 3,
        tick_hz: 60,
        expires: 3000,
        pacing: CertificatePacing::On,
        ordered_alone: false,
    }
    .sign(&key)
    .unwrap();
    for peer in [&mut fast_client, &mut slow_client] {
        connect(&mut server, peer, 0);
        configure(&mut server, peer, &cert, 0);
    }
    let fast = Player {
        capacity: 400,
        tick_micros: 4_500,
    };
    let start = ordering(&server).starts_at.unwrap();
    let mut players = [
        (fast, 0u32, Meter::default()),
        (slow, 0u32, Meter::default()),
    ];
    let mut outcome = Outcome::default();
    let mut last_rate = ordering(&server).pace.rate;
    outcome.rates.push((0, last_rate));
    for second in 1..=seconds {
        let now = start + second * 1000;
        // Clients renew their tickets well before the five-minute limit.
        if second % 100 == 0 {
            renew(&mut server, &mut fast_client, now);
            renew(&mut server, &mut slow_client, now);
        }
        // The clock seals the second's frames in turns.
        for step in 1..=10 {
            let at = now - 1000 + step * 100;
            server.advance_clock(at, unix(at));
        }
        let head = ordering(&server).head;
        let rate = ordering(&server).pace.rate;
        for (index, (player, done, meter)) in players.iter_mut().enumerate() {
            let ran = (head - *done).min(player.capacity);
            *done += ran;
            let backlog = head - *done;
            let pace = meter.report(now, backlog, ran, rate);
            let client = if index == 0 {
                &mut fast_client
            } else {
                &mut slow_client
            };
            let mut bytes = Vec::new();
            client.queue_ordering_pace(pace, ran as u16, Some(player.tick_micros));
            client.test_uplink(None, now, &mut bytes).unwrap();
            // Entries the relay pushes back are not this replay's concern.
            server
                .process_at(endpoint(client), &bytes, unix(now), now)
                .unwrap();
            if index == 1 {
                outcome.max_backlog = outcome.max_backlog.max(backlog);
                outcome.final_backlog = backlog;
                if pace != OrderPace::KeepingUp {
                    outcome.catching_up_seconds += 1;
                }
            }
        }
        let rate = ordering(&server).pace.rate;
        if rate != last_rate {
            outcome.rates.push((second, rate));
            last_rate = rate;
        }
    }
    let counts = server.take_ordering_counts();
    outcome.slowdowns = counts.slowdowns;
    outcome.raises = counts.raises;
    println!("{outcome:?}");
    outcome
}

/// Match 1e9fac21: about 55 frames a second at 8 ms ticks, for 828 seconds.
/// The clock must slow for it so it never falls more than a few seconds
/// behind, as the other player (whose report the server kept) never did.
#[test]
fn a_pc_that_runs_below_the_rate_though_its_ticks_are_quick_slows_the_clock() {
    let outcome = replay(
        Player {
            capacity: 55,
            tick_micros: 8_000,
        },
        828,
    );
    assert!(outcome.slowdowns >= 1, "{outcome:?}");
    assert!(
        outcome.max_backlog <= 5 * 60,
        "fell {} frames behind: {outcome:?}",
        outcome.max_backlog
    );
    assert!(outcome.final_backlog <= 60, "{outcome:?}");
}

/// Match 785b3c9b: 28 AI; modelled as a PC that runs about 35 frames a
/// second at 15 ms ticks. Its ticks hold 53 Hz, so the clock slowed one row
/// to 45 and stopped there while the player fell further behind. It must
/// settle at 30, trying a faster rate again at most every five minutes.
#[test]
fn the_clock_settles_at_a_rate_a_slow_pc_holds() {
    let outcome = replay(
        Player {
            capacity: 35,
            tick_micros: 15_000,
        },
        972,
    );
    assert!(
        outcome.max_backlog <= 5 * 60,
        "fell {} frames behind: {outcome:?}",
        outcome.max_backlog
    );
    assert!(outcome.catching_up_seconds <= 972 / 10, "{outcome:?}");
    assert!(outcome.raises <= 972 / 300 + 1, "{outcome:?}");
}
