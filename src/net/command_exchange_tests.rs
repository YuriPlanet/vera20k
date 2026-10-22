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
fn rejected_broadcast_keeps_native_mirrors_and_advances_frontier_after_whole_call() {
    let mut sim = Simulation::new();
    queue_stops(&mut sim, 128);
    let mut exchange = CommandExchange::new([3]).unwrap();
    let mut packets = Vec::new();
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
                packets.push((bytes.to_vec(), reliable));
                // A full peer refuses the second broadcast.649FA1 ignores it.
                packets.len() == 1
            },
        )
        .unwrap();
    assert_eq!(result, SendPoll::Complete { commands: 128 });
    assert_eq!(exchange.sent_commands(), 128);
    assert_eq!(queues.local_len(), 0);
    assert_eq!(queues.synchronized_len(), 128);
    assert_eq!(packets.len(), 2);
    assert!(packets.iter().all(|(_, reliable)| *reliable));
    for (bytes, _) in &packets {
        assert_eq!(
            decode_frame_info(bytes),
            Some(FrameInfo {
                house_id: 2,
                event_frame: 25,
                checksum: 0x12345678,
                timing_word: 0,
                delay: 10,
            })
        );
    }
    let mut received = Simulation::new();
    let mut receiver = CommandExchange::new([2]).unwrap();
    receiver
        .receive(
            2,
            &packets[0].0,
            0,
            &mut received.network_command_queues(),
            0,
        )
        .unwrap();
    assert!(receiver.peers()[0].received_commands() < 128);
    // The next header advertises all native mirrors, including the refused
    // packet. Receiver progress then exposes its missing commands to Wait.
    exchange
        .send_frame(
            &mut queues,
            2,
            20,
            10,
            0xdeadbeef,
            policy(),
            |bytes, reliable| {
                assert!(!reliable);
                receiver
                    .receive(2, bytes, 0, &mut received.network_command_queues(), 0)
                    .unwrap();
                true
            },
        )
        .unwrap();
    let wait = receiver.frame_wait(20, 10);
    assert_eq!(wait.missing_commands, Some(2));
    assert!(!wait.ready());
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
                assert_eq!(bytes.len(), 14);
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
            FrameInfo {
                house_id: 2,
                event_frame: 25,
                checksum: 10,
                timing_word: 12,
                delay: 10,
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
        encode_frame_sync(FrameInfo {
            house_id,
            event_frame,
            checksum,
            timing_word,
            delay: 10,
        })
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
        encode_frame_sync(FrameInfo {
            house_id: 2,
            event_frame: 20,
            checksum,
            timing_word: 0,
            delay: 10,
        })
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
