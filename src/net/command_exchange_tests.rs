use super::*;
use crate::sim::command::{CommandEnvelope, CommandRecord};
use crate::sim::command_queue::DO_LIST_CAPACITY;
use crate::sim::world::Simulation;
use std::num::NonZeroU32;

fn policy() -> NetworkSendPolicy {
    NetworkSendPolicy::FrameSendRate2 {
        frame_send_rate: NonZeroU32::new(5).unwrap(),
    }
}

#[test]
fn authenticated_source_cannot_claim_another_houses_commands_or_progress() {
    let mut sim = Simulation::new();
    let mut receiver = CommandExchange::new([2, 3]).unwrap();
    let mut sender = CommandExchange::default();
    let mut packet = Vec::new();
    sender
        .send_frame(
            &mut sim.network_command_queues(),
            3,
            5,
            10,
            0,
            policy(),
            |bytes, _| {
                packet = bytes.to_vec();
                true
            },
        )
        .unwrap();
    let before = receiver.peers().to_vec();
    assert!(matches!(
        receiver.receive(2, &packet, 0, &mut sim.network_command_queues(), 0),
        Err(CommandExchangeError::SourceMismatch {
            source_house: 2,
            claimed_house: 3
        })
    ));
    assert_eq!(receiver.peers(), before);
    assert_eq!(sim.network_command_queues().synchronized_len(), 0);
    let mut nested = packet.clone();
    nested.extend_from_slice(&packet);
    assert!(matches!(
        receiver.receive(3, &nested, 0, &mut sim.network_command_queues(), 0),
        Err(CommandExchangeError::Decode(PacketDecodeError {
            kind: super::super::event_packet::DecodeFailureKind::UnexpectedHeader,
            ..
        }))
    ));
}

fn stop(actor: u32) -> CommandRecord {
    let mut payload = actor.to_le_bytes().to_vec();
    payload.push(0x34);
    CommandRecord::encode(6, 2, 3, &payload).unwrap()
}

fn queue_stops(sim: &mut Simulation, count: usize) {
    for actor in 0..count {
        assert!(sim.issue_local_command(CommandEnvelope::synchronized(stop(actor as u32), 7)));
    }
}

#[test]
fn rejected_broadcast_retains_unsent_commands_and_codec_until_retry() {
    let mut sim = Simulation::new();
    for actor in 0..128 {
        let mut record = stop(actor);
        crate::sim::command::MegaMissionRecord {
            house_id: 2,
            frame: 3,
            source_id: actor as i32,
            auxiliary: crate::sim::command::MegaMissionTarget::Null,
            order: crate::sim::command::MegaMissionOrder::Move {
                target_x: 10,
                target_y: 10,
            },
        }
        .write_into(&mut record)
        .unwrap();
        assert!(sim.issue_local_command(CommandEnvelope::synchronized(record, 7)));
    }
    let mut exchange = CommandExchange::new([3]).unwrap();
    assert_eq!(exchange.retained_send(), None);
    let mut attempts = Vec::new();
    let mut queues = sim.network_command_queues();
    let result = exchange
        .send_frame(
            &mut queues,
            2,
            15,
            10,
            0x12345678,
            policy(),
            |bytes, reliable| {
                assert!(reliable);
                attempts.push(bytes.to_vec());
                attempts.len() == 1
            },
        )
        .unwrap();
    assert_eq!(result, SendPoll::Backpressure);
    let accepted = queues.synchronized_len();
    assert!(accepted > 0 && accepted < 128);
    assert_eq!(exchange.sent_commands(), accepted as u64);
    assert_eq!(queues.local_len(), 128 - accepted);
    assert_eq!(
        exchange.retained_send(),
        Some(RetainedSendProgress {
            frame: 15,
            remaining: 128 - accepted,
            total: 128,
            frontier: 128,
        })
    );
    assert_eq!(attempts.len(), 2);
    assert!(
        attempts
            .iter()
            .all(|packet| live_command_frontier(packet) == Some(128))
    );
    let refused = attempts[1].clone();
    assert_eq!(
        exchange
            .send_frame(
                &mut queues,
                2,
                15,
                10,
                0x12345678,
                policy(),
                |bytes, reliable| {
                    assert!(reliable);
                    assert_eq!(bytes, refused);
                    true
                }
            )
            .unwrap(),
        SendPoll::Complete { commands: 128 }
    );
    assert_eq!(exchange.sent_commands(), 128);
    assert_eq!(exchange.retained_send(), None);
    assert_eq!(queues.local_len(), 0);
    assert_eq!(queues.synchronized_len(), 128);
}

#[test]
fn wide_frontier_keeps_lost_first_and_final_packets_blocked_past_65536() {
    let mut sim = Simulation::new();
    queue_stops(&mut sim, 128);
    let mut sender = CommandExchange::new([3]).unwrap();
    sender.sent_commands = 65530;
    let mut packets = Vec::new();
    sender
        .send_frame(
            &mut sim.network_command_queues(),
            2,
            15,
            10,
            0,
            policy(),
            |bytes, _| {
                packets.push(bytes.to_vec());
                true
            },
        )
        .unwrap();
    assert_eq!(sender.sent_commands(), 65658);
    assert_eq!(packets.len(), 2);
    let probe = sender.frame_probe(2, 20, 10, 73, policy()).unwrap();
    for first in [0, 1] {
        let mut receiver = CommandExchange::new([2]).unwrap();
        receiver.peers[0].received_commands = 65530;
        let mut received = Simulation::new();
        assert!(matches!(
            receiver.receive(
                2,
                &probe[12..],
                73,
                &mut received.network_command_queues(),
                0
            ),
            Err(CommandExchangeError::InvalidHeader)
        ));
        receiver
            .receive(2, &probe, 73, &mut received.network_command_queues(), 0)
            .unwrap();
        assert_eq!(receiver.frame_wait(20, 10).missing_commands, Some(2));
        receiver
            .receive(
                2,
                &packets[first],
                73,
                &mut received.network_command_queues(),
                0,
            )
            .unwrap();
        assert_eq!(receiver.frame_wait(20, 10).missing_commands, Some(2));
        assert!(!receiver.frame_wait(20, 10).ready());
        receiver
            .receive(
                2,
                &packets[1 - first],
                73,
                &mut received.network_command_queues(),
                0,
            )
            .unwrap();
        assert_eq!(receiver.peers()[0].received_commands(), 65658);
        assert!(receiver.frame_wait(20, 10).ready());
    }
}

#[test]
fn off_boundary_leaves_commands_local_and_empty_boundary_sends_unreliable_header() {
    let mut sim = Simulation::new();
    queue_stops(&mut sim, 1);
    let mut exchange = CommandExchange::default();
    let mut queues = sim.network_command_queues();
    assert_eq!(
        exchange
            .send_frame(&mut queues, 2, 1, 10, 9, policy(), |_, _| {
                panic!("off-boundary frame must only service the interface")
            })
            .unwrap(),
        SendPoll::OffCadence
    );
    assert_eq!(queues.local_len(), 1);
    exchange
        .send_frame(&mut queues, 2, 5, 10, 9, policy(), |_, reliable| reliable)
        .unwrap();
    assert_eq!(queues.local_len(), 0);
    assert_eq!(
        exchange
            .send_frame(&mut queues, 2, 10, 10, 19, policy(), |bytes, reliable| {
                assert_eq!(bytes.len(), 26);
                assert!(!reliable);
                assert_eq!(decode_frame_info(bytes).unwrap().timing_word, 1);
                true
            })
            .unwrap(),
        SendPoll::Complete { commands: 0 }
    );
}

#[test]
fn decode_failure_updates_advertised_progress_but_not_decoded_frontier_or_prefix() {
    let mut sim = Simulation::new();
    {
        let mut queues = sim.network_command_queues();
        for _ in 0..DO_LIST_CAPACITY - 1 {
            assert!(queues.admit_received(stop(1)));
        }
    }
    let mut encoder = EventPacketCodec::default();
    let source = stop(77);
    let packet = encoder
        .encode_packet(
            &[&source],
            EventPacketHeader::Vera {
                frame_info: FrameInfo {
                    house_id: 2,
                    event_frame: 25,
                    checksum: 10,
                    timing_word: 12,
                    delay: 10,
                },
                command_frontier: 12,
            },
            15,
            NATIVE_EVENT_PACKET_CAPACITY,
        )
        .unwrap();
    let mut exchange = CommandExchange::new([2]).unwrap();
    let error = exchange
        .receive(2, &packet.bytes, 0, &mut sim.network_command_queues(), 0)
        .unwrap_err();
    let CommandExchangeError::Decode(error) = error else {
        panic!("expected decode failure")
    };
    assert_eq!(error.admitted_records, 1);
    assert_eq!(
        error.kind,
        super::super::event_packet::DecodeFailureKind::AdmissionFull
    );
    assert_eq!(
        sim.network_command_queues().synchronized_len(),
        DO_LIST_CAPACITY
    );
    assert_eq!(exchange.peers()[0].frame(), 15);
    assert_eq!(exchange.peers()[0].advertised_commands(), 12);
    assert_eq!(exchange.peers()[0].received_commands(), 0);
    assert!(!exchange.frame_wait(15, 10).ready());
}

#[test]
fn initial_probe_never_enters_dolist_and_normal_wait_obeys_both_gates() {
    let mut exchange = CommandExchange::new([3, 2]).unwrap();
    let mut sim = Simulation::new();
    let make_probe = |house_id, event_frame, timing_word, checksum| {
        encode_live_frame_sync(
            FrameInfo {
                house_id,
                event_frame,
                checksum,
                timing_word,
                delay: 10,
            },
            u64::from(timing_word),
        )
        .unwrap()
    };
    exchange
        .receive(
            3,
            &make_probe(3, 10, 0, 99),
            99,
            &mut sim.network_command_queues(),
            0,
        )
        .unwrap();
    assert!(!exchange.initial_ready());
    exchange
        .receive(
            2,
            &make_probe(2, 10, 0, 99),
            99,
            &mut sim.network_command_queues(),
            0,
        )
        .unwrap();
    assert!(exchange.initial_ready());
    assert_eq!(sim.network_command_queues().synchronized_len(), 0);
    exchange.finish_initial_barrier();
    assert!(exchange.frame_wait(9, 10).ready());
    let wait = exchange.frame_wait(10, 10);
    assert!(!wait.ready());
    assert_eq!(wait.missing_frame, Some(3)); // First equal minimum wins.
    exchange
        .receive(
            3,
            &make_probe(3, 30, 1, 99),
            99,
            &mut sim.network_command_queues(),
            0,
        )
        .unwrap();
    exchange
        .receive(
            2,
            &make_probe(2, 30, 0, 99),
            99,
            &mut sim.network_command_queues(),
            0,
        )
        .unwrap();
    let wait = exchange.frame_wait(10, 10);
    assert!(!wait.ready());
    assert_eq!(wait.missing_frame, None);
    assert_eq!(wait.missing_commands, Some(3));
    // The frontier is an unsigned max, not a modular low16 sequence.
    exchange
        .receive(
            3,
            &make_probe(3, 25, 0, 99),
            99,
            &mut sim.network_command_queues(),
            0,
        )
        .unwrap();
    assert_eq!(exchange.peers()[0].advertised_commands(), 1);
    assert_eq!(exchange.peers()[0].frame(), 20);
}

#[test]
fn wait_bar_reference_and_frame_sync_elapsed_follow_their_separate_writers() {
    let mut exchange = CommandExchange::new([2]).unwrap();
    let mut sim = Simulation::new();
    exchange.begin_wait(16007);
    assert_eq!(exchange.peers()[0].bar_timing(600, 25607), (600, 576));
    let probe = |checksum| {
        encode_live_frame_sync(
            FrameInfo {
                house_id: 2,
                event_frame: 20,
                checksum,
                timing_word: 0,
                delay: 10,
            },
            0,
        )
        .unwrap()
    };
    exchange
        .receive(2, &probe(73), 73, &mut sim.network_command_queues(), 125)
        .unwrap();
    assert_eq!(exchange.peers()[0].bar_timing(600, 25607), (475, 576));
    // A new invocation replaces peer+14; peer+C survives until a valid
    //FRAMESYNC arrives. A scenario mismatch must not write its elapsed time.
    exchange.begin_wait(32009);
    assert!(
        exchange
            .receive(2, &probe(74), 73, &mut sim.network_command_queues(), 500)
            .is_err()
    );
    assert_eq!(exchange.peers()[0].bar_timing(600, 32009), (475, 0));
    exchange.reset();
    assert_eq!(exchange.peers()[0].bar_timing(600, 1000), (600, 60));
}

#[test]
fn wait_bar_timing_matches_executed_native_wrap_and_age_controls() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/storage_oracle/lan_wait_bars.json",
    ))
    .unwrap();
    for case in corpus["cases"].as_array().unwrap() {
        let input = &case["input"];
        let native = &case["rows"]["1"];
        let name = input["name"].as_str().unwrap();
        let mut peer = CommandPeerProgress::new(2);
        peer.response_elapsed_buckets = input["peer_elapsed"].as_u64().unwrap() as u32;
        peer.bar_reference_millis = input["reference_ms"].as_u64().unwrap() as u32;
        let (lag, age) = peer.bar_timing(
            input["wait_elapsed"].as_u64().unwrap() as u32,
            input["now_ms"].as_u64().unwrap() as u32,
        );
        assert_eq!(u64::from(lag), native["lag"].as_u64().unwrap(), "{name}");
        //Native reads timeGetTime only for lag>480. Its omitted reads do
        //not establish an age value for the green/yellow controls.
        if let Some(expected) = native["age_ticks"].as_u64() {
            assert_eq!(u64::from(age), expected, "{name}");
        }
    }
}

#[test]
fn initial_mismatch_preserves_native_progress_and_reset_clears_every_session_field() {
    let mut exchange = CommandExchange::new([2]).unwrap();
    let mut sim = Simulation::new();
    let probe = exchange.frame_probe(2, 0, 10, 77, policy()).unwrap();
    assert!(matches!(
        exchange.receive(2, &probe, 99, &mut sim.network_command_queues(), 0),
        Err(CommandExchangeError::InitialChecksum {
            received: 77,
            expected: 99,
            ..
        })
    ));
    assert!(exchange.initial_ready());
    exchange.reset();
    assert!(!exchange.initial_ready());
    assert_eq!(exchange.peers()[0].frame(), -1);
    assert_eq!(exchange.sent_commands(), 0);
    assert!(exchange.remove_peer(2));
    assert!(exchange.initial_ready());
}

/// Generated strip events use the ordinary packet, private reliable UDP and
/// simulation receivers before disposing the held product on both clients.
#[test]
fn local_prerequisite_loss_publishes_one_raw_cancellation_stream() {
    use crate::net::command_exchange::{CommandExchange, SendPoll};
    use crate::net::lan_messages::SessionScope;
    use crate::net::lockstep::NetworkSendPolicy;
    use crate::net::reliable::{Delivery, LanConnectionConfig, LanConnections, RetryTiming};
    use crate::net::udp::{LanUdpTransport, UdpBindConfig};
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    // The production App injects frame_pacer::wall_clock_bucket. Core cannot
    // depend on App; this stateless integration-test wall clock uses real time.
    fn wall_bucket() -> u32 {
        (SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            / 16) as u32
    }

    crate::sim::production::prerequisite_loss_cancellation_fixture(|sender, receiver, records| {
        // Use the same packet, private reliability and scoped UDP owners as live
        // play. No record/ACK is injected into the receiver or retired by the test.
        let mut wires: [LanUdpTransport; 2] = std::array::from_fn(|_| {
            LanUdpTransport::bind(
                UdpBindConfig {
                    primary: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                    alternate: SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                },
                LanConnections::new(
                    LanConnectionConfig::default(),
                    RetryTiming {
                        retry_delta: 1,
                        max_retries: u32::MAX,
                        timeout: u32::MAX,
                    },
                ),
            )
            .unwrap()
        });
        let binds: [_; 2] = std::array::from_fn(|side| UdpBindConfig {
            primary: wires[side].primary_addr().unwrap(),
            alternate: wires[side].alternate_addr().unwrap(),
        });
        for wire in &mut wires {
            wire.set_lan_scope(SessionScope {
                epoch: [7; 16],
                revision: 1,
            })
            .unwrap();
            wire.start_io_pump(wall_bucket).unwrap();
        }
        let [outbound, inbound] = &mut wires;
        outbound
            .connections_mut()
            .add_peer(1, inbound.primary_addr().unwrap())
            .unwrap();
        inbound
            .connections_mut()
            .add_peer(0, outbound.primary_addr().unwrap())
            .unwrap();
        for bytes in [b"first control".as_slice(), b"second control".as_slice()] {
            assert!(outbound.global_mut().send(
                bytes,
                crate::net::global::GlobalDestination::direct(inbound.primary_addr().unwrap()),
                Delivery::Reliable,
                0
            ));
        }
        let mut send = CommandExchange::new([1]).unwrap();
        let mut receive = CommandExchange::new([0]).unwrap();
        assert_eq!(
            send.send_frame(
                &mut sender.network_command_queues(),
                0,
                0,
                0,
                0,
                NetworkSendPolicy::EveryFrame,
                |bytes, reliable| {
                    assert!(reliable);
                    outbound
                        .connections_mut()
                        .broadcast(bytes, Delivery::Reliable)
                },
            )
            .unwrap(),
            SendPoll::Complete {
                commands: records.len()
            }
        );
        assert_eq!(sender.pending_local_command_count(), 0);
        // Both main consumers now stop servicing, popping and decoding.
        // This getter only takes a read guard; it cannot send, ACK or wake.
        // Reliable retirement must therefore happen entirely in the SAME
        // transport owners' background pumps before ordinary admission resumes.
        let started = Instant::now();
        loop {
            let private_done = outbound.connections().queued_send_count(1) == Some(0);
            let global_done = outbound.global().queued_send_count() == 0;
            if private_done && global_done {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "reliable UDP/ACK did not retire while main consumers were idle"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(
            receive.peers()[0].received_commands(),
            0,
            "physical I/O must not admit application commands"
        );
        // An intentionally stale caller sample must not timestamp active I/O.
        for wire in [&mut *outbound, &mut *inbound] {
            let report = wire.service(u32::MAX);
            assert!(
                report.first_io_error.is_none() && report.failed_peer.is_none(),
                "{report:?}"
            );
            assert_eq!(
                report.rejected_connections + report.rejected_global + report.malformed_datagrams,
                0,
                "{report:?}"
            );
            assert!(
                report.received_datagrams > 0 && report.sent_datagrams > 0,
                "{report:?}"
            );
        }
        // Same App FIFO can inspect both owned controls without waiting for
        // another physical-service pass to retire the first consumed head.
        assert_eq!(
            inbound.pop_global_received().unwrap().payload,
            b"first control"
        );
        assert_eq!(
            inbound.pop_global_received().unwrap().payload,
            b"second control"
        );
        assert!(inbound.pop_global_received().is_none());
        while let Some(message) = inbound.pop_received() {
            assert_eq!((message.peer_id, message.delivery), (0, Delivery::Reliable));
            receive
                .receive(
                    i8::try_from(message.peer_id).unwrap(),
                    &message.payload,
                    0,
                    &mut receiver.network_command_queues(),
                    0,
                )
                .unwrap();
        }
        assert_eq!(receive.peers()[0].received_commands(), records.len() as u64);
        assert!(inbound.pop_received().is_none());
        assert_eq!(send.sent_commands(), records.len() as u64);
        assert_eq!(
            receive.peers()[0].advertised_commands(),
            records.len() as u64
        );
        // Final drop stops/unparks/joins the SAME workers before closing sockets.
        // Rebinding proves no worker/Arc retains either old socket afterward.
        drop(wires);
        for bind in binds {
            drop(
                LanUdpTransport::bind(
                    bind,
                    LanConnections::new(
                        LanConnectionConfig::default(),
                        RetryTiming {
                            retry_delta: 1,
                            max_retries: u32::MAX,
                            timeout: u32::MAX,
                        },
                    ),
                )
                .unwrap(),
            );
        }
    });
}
