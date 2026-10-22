use super::*;
use std::net::Ipv4Addr;

fn endpoint(last_octet: u8) -> SocketAddrV4 {
    SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, last_octet), 1234)
}

fn timing() -> RetryTiming {
    RetryTiming {
        retry_delta: 30,
        max_retries: u32::MAX,
        timeout: 600,
    }
}

fn connections(config: LanConnectionConfig, peers: &[i32]) -> LanConnections {
    let mut result = LanConnections::new(config, timing());
    for &id in peers {
        result.add_peer(id, endpoint(id as u8)).unwrap();
    }
    result
}

fn frame(kind: u8, seq: u32, data: &[u8]) -> Vec<u8> {
    connection_frame(0xaa04, kind, seq, data)
}

fn native_controls() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../tools/storage_oracle/lan_udp_checksum.json"
    ))
    .unwrap()
}

#[test]
fn lan_start_timing_matches_original_arithmetic_and_conditional_response_reads() {
    let corpus: serde_json::Value =
        serde_json::from_str(include_str!("../../tools/storage_oracle/lan_messages.json")).unwrap();
    for case in corpus["start_timings"].as_array().unwrap() {
        let responses: Vec<_> = case["responses"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap() as u32)
            .collect();
        let mut reads = responses.iter();
        let actual = RetryTiming::lan_start(
            case["host"].as_bool().unwrap(),
            case["completed"].as_bool().unwrap(),
            || *reads.next().expect("unexpected ResponseTime read"),
        );
        let expected = RetryTiming {
            retry_delta: case["retry_delta"].as_u64().unwrap() as u32,
            max_retries: case["max_retries"].as_u64().unwrap() as u32,
            timeout: case["timeout"].as_u64().unwrap() as u32,
        };
        assert_eq!(actual, expected, "{}", case["name"]);
        assert_eq!(
            responses.len() - reads.len(),
            case["reads"].as_u64().unwrap() as usize,
            "{}",
            case["name"]
        );
        assert!(reads.next().is_none(), "{}", case["name"]);
    }
}

#[test]
fn lan_handoff_timings_match_native_outputs_and_ordered_global_reads() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/storage_oracle/lan_manager_timing.json"
    ))
    .unwrap();
    for case in corpus["timing"].as_array().unwrap() {
        let observations = case["input_responses"].as_array().unwrap();
        let mut observations = observations.iter();
        let mut read = || {
            observations
                .next()
                .expect("unexpected native response observation")
                .as_u64()
                .unwrap() as u32
        };
        let (private, global) = if case["role"] == "prepare" {
            RetryTiming::lan_gameplay(&mut read)
        } else {
            (
                RetryTiming::lan_start(false, true, &mut read),
                RetryTiming::LAN_GUEST_COMPLETED_GLOBAL,
            )
        };
        let tuple = |timing: RetryTiming| {
            serde_json::json!([timing.retry_delta, timing.max_retries, timing.timeout])
        };
        assert_eq!(tuple(private), case["private"], "{}", case["name"]);
        assert_eq!(tuple(global), case["global_timing"], "{}", case["name"]);
        assert!(
            observations.next().is_none(),
            "{} did not consume every native read",
            case["name"]
        );
    }
}

#[test]
fn response_history_reset_matches_native_clear_preserve_and_empty_controls() {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/storage_oracle/lan_manager_timing.json"
    ))
    .unwrap();
    for case in corpus["reset"].as_array().unwrap() {
        for (index, &role) in ["global", "private0", "private1"].iter().enumerate() {
            let mut response = ResponseTime::default();
            for elapsed in case["input_samples"][index].as_array().unwrap() {
                response.record(elapsed.as_u64().unwrap() as u32);
            }
            let state = |response: &ResponseTime| [response.total, response.samples, response.mean];
            let expected = |field: &str| {
                std::array::from_fn::<_, 3, _>(|i| case[field][role][i].as_u64().unwrap() as u32)
            };
            assert_eq!(
                state(&response),
                expected("before"),
                "{} {role}",
                case["name"]
            );
            if index == 0 {
                response.reset(true);
            } else {
                response.reset(case["clear"].as_bool().unwrap());
            }
            assert_eq!(
                state(&response),
                expected("after"),
                "{} {role}",
                case["name"]
            );
        }
    }
}

#[test]
fn private_response_reset_preserves_pending_delivery_and_original_ack_time() {
    let mut sender = connections(LanConnectionConfig::default(), &[2]);
    sender.peers[0].send.response_time.record(10);
    assert!(sender.send_to(2, b"first", Delivery::Reliable));
    assert!(sender.send_to(2, b"second", Delivery::Reliable));
    let mut emitted = Vec::new();
    sender.service(100, &mut |_, frame| emitted.push(frame.to_vec()));
    sender
        .receive(endpoint(2), &frame(2, 0, &[]), &mut |_, _| {})
        .unwrap();
    let incoming = frame(0, 0, b"incoming");
    sender
        .receive(endpoint(2), &incoming, &mut |_, _| {})
        .unwrap();
    let before = format!("{:?}", sender.peers[0]);
    sender.reset_response_time(false);
    assert_eq!(format!("{:?}", sender.peers[0]), before);
    sender.reset_response_time(true);
    assert_eq!(sender.mean_response_time(2), Some(0));
    assert_eq!(sender.queued_send_count(2), Some(2));
    assert_eq!(sender.peers[0].send.total_enqueued(), 2);
    assert_eq!(sender.peers[0].next_reliable, 2);
    assert_eq!(sender.peers[0].receive.len(), 1);
    for (entry, original) in sender.peers[0].send.entries.iter().zip(&emitted) {
        assert_eq!(&entry.frame, original);
        assert_eq!(
            (entry.first_sent, entry.last_sent, entry.attempts),
            (100, 100, 1)
        );
    }
    assert!(sender.peers[0].send.entries[0].completed);
    sender.service(120, &mut |_, _| panic!("retry before native interval"));
    assert_eq!(sender.mean_response_time(2), Some(20));
    assert_eq!(sender.queued_send_count(2), Some(1));
    sender
        .receive(endpoint(2), &frame(2, 1, &[]), &mut |_, _| {})
        .unwrap();
    sender.service(130, &mut |_, _| panic!("completed entry retransmitted"));
    assert_eq!(sender.mean_response_time(2), Some(25));
    assert_eq!(sender.pop_received().unwrap().payload, b"incoming");
    assert_eq!(
        sender.receive(endpoint(2), &incoming, &mut |_, _| {}),
        Ok(ReceiveOutcome::DuplicateAcknowledged)
    );
    assert!(sender.pop_received().is_none());
    assert!(sender.send_to(2, b"next", Delivery::Reliable));
    assert_eq!(sequence(&sender.peers[0].send.entries[0].frame), 2);
}

#[test]
fn rolling_checksum_matches_original_udp_send_for_all_tail_lengths() {
    let corpus = native_controls();
    for case in corpus["cases"].as_array().unwrap() {
        let hex = case["payload"].as_str().unwrap();
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16).unwrap())
            .collect();
        assert_eq!(
            u64::from(udp_checksum(&bytes)),
            case["checksum"].as_u64().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn response_mean_matches_original_saturation_unsigned_rounding_and_wrap() {
    let corpus = native_controls();
    for case in corpus["response_time_cases"].as_array().unwrap() {
        let before = &case["before"];
        let mut response = ResponseTime {
            total: before[0].as_u64().unwrap() as u32,
            samples: before[1].as_u64().unwrap() as u32,
            mean: before[2].as_u64().unwrap() as u32,
        };
        response.record(case["elapsed"].as_u64().unwrap() as u32);
        assert_eq!(
            serde_json::json!([response.total, response.samples, response.mean]),
            case["after"],
            "{}",
            case["name"]
        );
    }
}

#[test]
fn empty_peer_broadcast_matches_original_early_rejection() {
    let corpus = native_controls();
    let mut sender = connections(LanConnectionConfig::default(), &[]);
    assert_eq!(
        sender.peers().len() as u64,
        corpus["empty_peer_broadcast"]["peer_count"]
            .as_u64()
            .unwrap()
    );
    assert_eq!(
        u64::from(sender.broadcast(&[], Delivery::Reliable)),
        corpus["empty_peer_broadcast"]["returned"].as_u64().unwrap()
    );
}

#[test]
fn reliable_move_survives_lost_data_lost_ack_reorder_and_duplicates() {
    let mut sender = connections(LanConnectionConfig::default(), &[2]);
    let mut receiver = connections(LanConnectionConfig::default(), &[1]);
    let mut sent = Vec::new();
    let mut acks = Vec::new();
    assert!(sender.broadcast(b"move first", Delivery::Reliable));
    assert!(sender.broadcast(b"move second", Delivery::Reliable));
    assert_eq!(
        sender.service(30, &mut |_, data| sent.push(data.to_vec())),
        None
    );
    assert!(sent.is_empty(), "native retry comparison is strict");
    sender.service(31, &mut |_, data| sent.push(data.to_vec()));
    assert_eq!(
        &sent[0][..14],
        &[4, 170, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    // Lose the first packet. The second is ACKed but waits for its predecessor.
    receiver
        .receive(endpoint(1), &sent[1], &mut |_, data| {
            acks.push(data.to_vec())
        })
        .unwrap();
    assert!(receiver.pop_received().is_none());
    sender
        .receive(endpoint(2), &acks[0], &mut |_, _| panic!("ACK of an ACK"))
        .unwrap();
    sent.clear();
    sender.service(62, &mut |_, data| sent.push(data.to_vec()));
    assert_eq!(sent.len(), 1, "the ACKed second packet is retired");
    acks.clear();
    receiver
        .receive(endpoint(1), &sent[0], &mut |_, data| {
            acks.push(data.to_vec())
        })
        .unwrap();
    assert_eq!(receiver.pop_received().unwrap().payload, b"move first");
    assert_eq!(receiver.pop_received().unwrap().payload, b"move second");
    assert!(receiver.pop_received().is_none());
    // Lose that ACK. A later retry must be ACKed without executing it again.
    sent.clear();
    sender.service(93, &mut |_, data| sent.push(data.to_vec()));
    assert_eq!(
        receiver.receive(endpoint(1), &sent[0], &mut |_, data| acks
            .push(data.to_vec())),
        Ok(ReceiveOutcome::DuplicateAcknowledged)
    );
    assert!(receiver.pop_received().is_none());
    assert_eq!(
        &acks[1][..14],
        &[4, 170, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
    sender
        .receive(endpoint(2), &acks[1], &mut |_, _| {})
        .unwrap();
    sender.service(94, &mut |_, _| panic!("ACKed data retransmitted"));
    assert_eq!(sender.queued_send_count(2), Some(0));
}

#[test]
fn broadcast_backpressure_is_atomic_and_rejected_send_does_not_consume_sequence() {
    let mut sender = connections(
        LanConnectionConfig {
            send_capacity: 1,
            ..Default::default()
        },
        &[1, 2],
    );
    assert!(sender.send_to(2, b"occupied", Delivery::Reliable));
    assert!(!sender.broadcast(b"deferred", Delivery::Reliable));
    assert_eq!(sender.queued_send_count(1), Some(0));
    assert_eq!(sender.queued_send_count(2), Some(1));
    let mut sent = Vec::new();
    sender.service(31, &mut |peer, data| sent.push((peer, data.to_vec())));
    sender
        .receive(endpoint(2), &frame(2, 0, &[]), &mut |_, _| {})
        .unwrap();
    assert!(!sender.broadcast(b"still deferred", Delivery::Reliable));
    sender.service(32, &mut |_, _| {});
    assert!(sender.broadcast(b"accepted", Delivery::Reliable));
    sent.clear();
    sender.service(33, &mut |peer, data| sent.push((peer, data.to_vec())));
    assert_eq!(sent.len(), 2);
    assert_eq!(sequence(&sent[0].1), 0);
    assert_eq!(sequence(&sent[1].1), 1);
    assert_eq!(&sent[0].1[14..], b"accepted");
}

#[test]
fn unreliable_bypasses_reliable_gap_without_ack_or_deduplication() {
    let mut receiver = connections(LanConnectionConfig::default(), &[1]);
    receiver
        .receive(endpoint(1), &frame(0, 1, b"later move"), &mut |_, _| {})
        .unwrap();
    for _ in 0..2 {
        receiver
            .receive(endpoint(1), &frame(1, 0, b"frame info"), &mut |_, _| {
                panic!("unreliable ACK")
            })
            .unwrap();
        assert_eq!(receiver.pop_received().unwrap().payload, b"frame info");
    }
    assert!(receiver.pop_received().is_none());
    receiver
        .receive(endpoint(1), &frame(0, 0, b"first move"), &mut |_, _| {})
        .unwrap();
    assert_eq!(receiver.pop_received().unwrap().payload, b"first move");
    assert_eq!(receiver.pop_received().unwrap().payload, b"later move");
}

#[test]
fn unreliable_is_sent_once_with_its_own_sequence_counter() {
    let mut sender = connections(LanConnectionConfig::default(), &[1]);
    sender.broadcast(b"move", Delivery::Reliable);
    sender.broadcast(b"frame", Delivery::Unreliable);
    sender.broadcast(b"next frame", Delivery::Unreliable);
    let mut sent = Vec::new();
    sender.service(31, &mut |_, data| sent.push(data.to_vec()));
    assert_eq!(
        sent.iter()
            .map(|data| (data[2], sequence(data)))
            .collect::<Vec<_>>(),
        [(0, 0), (1, 0), (1, 1)]
    );
    sent.clear();
    sender.service(62, &mut |_, data| sent.push(data.to_vec()));
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0][2], 0);
}

#[test]
fn reserved_receive_slot_accepts_missing_predecessor_and_retirement_keeps_frontier() {
    let mut receiver = connections(
        LanConnectionConfig {
            receive_capacity: 3,
            ..Default::default()
        },
        &[1],
    );
    let mut ack_count = 0;
    receiver
        .receive(endpoint(1), &frame(0, 2, b"two"), &mut |_, _| {
            ack_count += 1
        })
        .unwrap();
    receiver
        .receive(endpoint(1), &frame(0, 1, b"one"), &mut |_, _| {
            ack_count += 1
        })
        .unwrap();
    assert_eq!(
        receiver.receive(endpoint(1), &frame(0, 3, b"three"), &mut |_, _| {
            ack_count += 1
        }),
        Err(ReceiveError::Full)
    );
    assert_eq!(
        receiver.receive(endpoint(1), &frame(1, 0, b"frame"), &mut |_, _| {
            ack_count += 1
        }),
        Err(ReceiveError::Full)
    );
    receiver
        .receive(endpoint(1), &frame(0, 0, b"zero"), &mut |_, _| {
            ack_count += 1
        })
        .unwrap();
    assert_eq!(ack_count, 3);
    for expected in [b"zero".as_slice(), b"one", b"two"] {
        assert_eq!(receiver.pop_received().unwrap().payload, expected);
    }
    receiver.service(100, &mut |_, _| {});
    assert_eq!(receiver.peers[0].receive.len(), 1);
    assert_eq!(sequence(&receiver.peers[0].receive[0].frame), 2);
    receiver
        .receive(endpoint(1), &frame(0, 3, b"three"), &mut |_, _| {})
        .unwrap();
    assert_eq!(receiver.pop_received().unwrap().payload, b"three");
    receiver.service(101, &mut |_, _| {});
    assert_eq!(receiver.peers[0].receive.len(), 1);
    assert_eq!(sequence(&receiver.peers[0].receive[0].frame), 3);
}

#[test]
fn retry_timeout_uses_first_attempt_strict_limits_and_last_failed_peer() {
    let mut sender = connections(LanConnectionConfig::default(), &[2, 7]);
    sender.set_timing(RetryTiming {
        timeout: 60,
        ..timing()
    });
    sender.broadcast(b"move", Delivery::Reliable);
    let mut destinations = Vec::new();
    assert_eq!(
        sender.service(31, &mut |peer, _| destinations.push(peer)),
        None
    );
    assert_eq!(
        sender.service(61, &mut |_, _| panic!("equal retry delta sent")),
        None
    );
    assert_eq!(sender.service(62, &mut |_, _| {}), None);
    assert_eq!(
        sender.service(93, &mut |peer, _| destinations.push(peer)),
        Some(7)
    );
    assert_eq!(
        destinations,
        [endpoint(2), endpoint(7), endpoint(2), endpoint(7)]
    );
    // Failure is reported on an actual attempted send, not latched forever.
    assert_eq!(
        sender.service(94, &mut |_, _| panic!("sent before retry")),
        None
    );
    sender
        .receive(endpoint(7), &frame(2, 0, &[]), &mut |_, _| {})
        .unwrap();
    sender.service(95, &mut |_, _| {});
    assert_eq!(sender.queued_send_count(7), Some(0));
    assert_eq!(sender.mean_response_time(7), Some(64));
}

#[test]
fn attempt_limit_counts_initial_send_and_clock_difference_wraps() {
    let mut sender = connections(LanConnectionConfig::default(), &[1]);
    sender.set_timing(RetryTiming {
        retry_delta: 0,
        max_retries: 1,
        timeout: u32::MAX,
    });
    sender.broadcast(b"move", Delivery::Reliable);
    assert_eq!(sender.service(u32::MAX, &mut |_, _| {}), None);
    assert_eq!(sender.service(0, &mut |_, _| {}), Some(1));
    sender.set_timing(RetryTiming {
        retry_delta: 30,
        max_retries: u32::MAX,
        timeout: u32::MAX,
    });
    assert_eq!(
        sender.service(30, &mut |_, _| panic!("equal delta sent")),
        None
    );
    assert_eq!(sender.service(31, &mut |_, _| {}), None);
}

#[test]
fn failed_send_pass_defers_receive_retirement_until_a_successful_pass() {
    let mut peer = connections(LanConnectionConfig::default(), &[1]);
    peer.receive(endpoint(1), &frame(0, 0, b"first"), &mut |_, _| {})
        .unwrap();
    peer.receive(endpoint(1), &frame(0, 1, b"second"), &mut |_, _| {})
        .unwrap();
    peer.pop_received().unwrap();
    peer.pop_received().unwrap();
    peer.set_timing(RetryTiming {
        max_retries: 0,
        ..timing()
    });
    peer.send_to(1, b"outgoing", Delivery::Reliable);
    assert_eq!(peer.service(31, &mut |_, _| {}), Some(1));
    assert_eq!(peer.peers[0].receive.len(), 2);
    assert_eq!(peer.service(31, &mut |_, _| {}), None);
    assert_eq!(peer.peers[0].receive.len(), 1);
}

#[test]
fn round_robin_and_remove_readd_do_not_leak_prior_channel_state() {
    let mut receiver = connections(LanConnectionConfig::default(), &[1, 2, 3]);
    for id in [1, 2, 3] {
        receiver
            .receive(endpoint(id), &frame(0, 0, &[id]), &mut |_, _| {})
            .unwrap();
        receiver
            .receive(endpoint(id), &frame(0, 1, &[id + 10]), &mut |_, _| {})
            .unwrap();
    }
    assert_eq!(receiver.pop_received().unwrap().peer_id, 1);
    assert!(receiver.remove_peer(1));
    // Native keeps numeric cursor1 after compaction, so peer3 is next.
    assert_eq!(receiver.pop_received().unwrap().peer_id, 3);
    assert_eq!(receiver.pop_received().unwrap().peer_id, 2);
    receiver.add_peer(1, endpoint(1)).unwrap();
    receiver
        .receive(endpoint(1), &frame(0, 0, b"fresh"), &mut |_, _| {})
        .unwrap();
    let messages: Vec<_> = std::iter::from_fn(|| receiver.pop_received()).collect();
    assert!(
        messages
            .iter()
            .any(|message| message.peer_id == 1 && message.payload == b"fresh")
    );
    assert!(
        !messages
            .iter()
            .any(|message| message.peer_id == 1 && message.payload == [11])
    );
    receiver.reset();
    assert_eq!(receiver.peers().len(), 0);
    assert!(receiver.pop_received().is_none());
    receiver.add_peer(1, endpoint(1)).unwrap();
    receiver.send_to(1, b"fresh send", Delivery::Reliable);
    receiver.service(31, &mut |_, data| assert_eq!(sequence(data), 0));
}

#[test]
fn source_identity_and_bad_headers_cannot_reach_delivery() {
    let mut receiver = connections(LanConnectionConfig::default(), &[1]);
    let mut no_ack = |_, _: &[u8]| panic!("rejected input acknowledged");
    assert_eq!(
        receiver.receive(endpoint(2), &frame(0, 0, &[]), &mut no_ack),
        Err(ReceiveError::UnknownPeer(endpoint(2)))
    );
    assert_eq!(
        receiver.receive(endpoint(1), &[0; 13], &mut no_ack),
        Err(ReceiveError::ShortHeader)
    );
    let mut wrong_product = frame(0, 0, &[]);
    wrong_product[0] ^= 1;
    assert!(matches!(
        receiver.receive(endpoint(1), &wrong_product, &mut no_ack),
        Err(ReceiveError::WrongProduct { .. })
    ));
    assert_eq!(
        receiver.receive(endpoint(1), &frame(3, 0, &[]), &mut no_ack),
        Err(ReceiveError::UnknownKind(3))
    );
    assert_eq!(
        receiver.receive(endpoint(1), &frame(0, 0, &[0; 559]), &mut no_ack),
        Err(ReceiveError::PayloadTooLong)
    );
    assert!(receiver.pop_received().is_none());
}
