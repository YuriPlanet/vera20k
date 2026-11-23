//! Replays of real matches whose slow player the clock did not wait for.
//!
//! The relay is the real one, and so is the rule a player judges its pace by
//! (`PaceLag`, as the game's pace meter does): each second a modelled player
//! runs the frames waiting for it, up to what its PC can, and reports its pace
//! with what it ran and its tick cost.
//!
//! The 2026-11-21 report (match 1e9fac21, 2 players and 12 AI at 60 Hz): one
//! PC (GT 1030) ran about 55 frames a second, though its 90th-percentile tick
//! cost was about 8 ms. The relay read 8 ms as "holds 100 Hz, only draws
//! slowly" and never slowed, so in 828 seconds that player fell 3560 frames
//! behind and its orders took 5 seconds on average to take effect.

use super::*;

/// A modelled player: the frames its PC runs in a second, by the match's
/// second, and the tick cost it reports.
#[derive(Clone, Copy)]
struct Player {
    capacity: fn(u64) -> u32,
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
    // The clock's mean rate over the replay's last fifth.
    late_rate: f32,
}

/// Play `seconds` of a 60 Hz match between a fast player and `slow`.
fn replay(slow: Player, seconds: u64) -> Outcome {
    replay_paced(slow, seconds, CertificatePacing::On)
}

fn replay_paced(slow: Player, seconds: u64, pacing: CertificatePacing) -> Outcome {
    let key = SecretKey::new([73; 32]);
    let (mut server, mut fast_client, mut slow_client) = paced_room_for(
        &key,
        OrderingCertificate {
            room: [5; 16],
            sources_mask: 3,
            tick_hz: 60,
            expires: 3000,
            pacing,
            ordered_alone: false,
        },
    );
    let fast = Player {
        capacity: |_| 400,
        tick_micros: 4_500,
    };
    let mut late_frames = 0u64;
    let start = ordering(&server).starts_at.unwrap();
    let mut players = [
        (fast, 0u32, PaceLag::default()),
        (slow, 0u32, PaceLag::default()),
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
        for (index, (player, done, lag)) in players.iter_mut().enumerate() {
            let ran = (head - *done).min((player.capacity)(second));
            *done += ran;
            let backlog = head - *done;
            lag.observe(now as u32, backlog as usize, rate);
            let pace = lag.classify(now as u32, ran as u16, rate);
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
        if second > seconds * 4 / 5 {
            late_frames += u64::from(rate);
        }
        if rate != last_rate {
            outcome.rates.push((second, rate));
            last_rate = rate;
        }
    }
    let counts = server.take_ordering_counts();
    outcome.slowdowns = counts.slowdowns;
    outcome.raises = counts.raises;
    outcome.late_rate = late_frames as f32 / (seconds - seconds * 4 / 5) as f32;
    outcome
}

/// Match 1e9fac21: about 55 frames a second at 8 ms ticks, for 828 seconds.
/// The clock must slow for it so it never falls more than a few seconds
/// behind, as the other player (whose report the server kept) never did.
#[test]
fn a_pc_that_runs_below_the_rate_though_its_ticks_are_quick_slows_the_clock() {
    let outcome = replay(
        Player {
            capacity: |_| 55,
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
            capacity: |_| 35,
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

/// Match 1e9fac21 under continuous pacing: Game Speed rows held the 55-frame
/// PC at 45, a quarter slower than it could run; the clock now settles just
/// under what it ran, as lockstep LAN play (`frame_wait`) does.
#[test]
fn continuous_pacing_settles_just_under_what_a_slow_pc_runs() {
    let slow = Player {
        capacity: |_| 55,
        tick_micros: 8_000,
    };
    let outcome = replay_paced(slow, 828, CertificatePacing::Continuous);
    assert!(outcome.max_backlog <= 5 * 60, "{outcome:?}");
    assert!(outcome.final_backlog <= 60, "{outcome:?}");
    assert!((49.0..=55.0).contains(&outcome.late_rate), "{outcome:?}");
}

/// A PC that slows as the match grows: 55, then 40, then 25 frames a second.
/// Rows stop at 30 and fall behind for good once it runs 25; continuous
/// pacing follows it down.
#[test]
fn continuous_pacing_follows_a_pc_below_two_rows() {
    let slow = Player {
        capacity: |second| match second {
            0..300 => 55,
            300..600 => 40,
            _ => 25,
        },
        tick_micros: 8_000,
    };
    let rows = replay_paced(slow, 900, CertificatePacing::On);
    assert!(rows.final_backlog > 1000, "{rows:?}");
    let outcome = replay_paced(slow, 900, CertificatePacing::Continuous);
    assert!(outcome.max_backlog <= 5 * 60, "{outcome:?}");
    assert!(outcome.final_backlog <= 60, "{outcome:?}");
    assert!((20.0..=25.0).contains(&outcome.late_rate), "{outcome:?}");
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

#[test]
fn continuous_pacing_keeps_an_odd_rate_within_the_quarter_step() {
    let slow = Player {
        capacity: |_| 15,
        tick_micros: 8_000,
    };
    let outcome = replay_paced(slow, 30, CertificatePacing::Continuous);
    let first_rates: Vec<_> = outcome
        .rates
        .iter()
        .take(3)
        .map(|&(_, rate)| rate)
        .collect();
    assert_eq!(first_rates, [60, 45, 34]);
}

/// A PC that runs just above the clock after a stall keeps a standing
/// backlog its orders wait behind: 300 frames draining at one a second. The
/// clock slows a twentieth, so the PC runs a twentieth over it and drains the
/// backlog in about a minute and a half instead of five.
#[test]
fn continuous_pacing_drains_a_standing_backlog() {
    let slow = Player {
        capacity: |second| if second < 6 { 0 } else { 61 },
        tick_micros: 8_000,
    };
    let rows = replay_paced(slow, 600, CertificatePacing::On);
    assert!(rows.catching_up_seconds >= 250, "{rows:?}");
    let outcome = replay_paced(slow, 600, CertificatePacing::Continuous);
    assert!(outcome.catching_up_seconds <= 100, "{outcome:?}");
    assert!(outcome.final_backlog <= 60, "{outcome:?}");
    assert!(outcome.late_rate >= 50.0, "{outcome:?}");
}

/// A PC that hitches for eight seconds (loading pictures) at the start, then
/// runs well: continuous pacing slows a quarter at most and is back at the
/// match's own rate a minute after it.
#[test]
fn continuous_pacing_recovers_from_a_hitch() {
    let slow = Player {
        capacity: |second| if (4..12).contains(&second) { 16 } else { 400 },
        tick_micros: 8_000,
    };
    let outcome = replay_paced(slow, 120, CertificatePacing::Continuous);
    assert_eq!(outcome.rates[1].1, 45, "{outcome:?}");
    let &(back, rate) = outcome.rates.last().unwrap();
    assert!(rate == 60 && back <= 80, "{outcome:?}");
}

/// A PC catching up well over the clock's rate drains its backlog itself:
/// a standing backlog slows nothing then.
#[test]
fn continuous_pacing_leaves_a_fast_drain_alone() {
    let slow = Player {
        capacity: |second| if second < 6 { 0 } else { 66 },
        tick_micros: 8_000,
    };
    let outcome = replay_paced(slow, 120, CertificatePacing::Continuous);
    assert_eq!(outcome.slowdowns, 0, "{outcome:?}");
    assert!(outcome.final_backlog <= 60, "{outcome:?}");
}
