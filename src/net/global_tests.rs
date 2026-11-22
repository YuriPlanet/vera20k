use super::*;
use serde_json::Value;

fn corpus() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/storage_oracle/lan_global.json",
    ))
    .unwrap()
}

fn bytes(value: &Value) -> Vec<u8> {
    let text = value.as_str().unwrap();
    (0..text.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).unwrap())
        .collect()
}

fn endpoint(value: &Value) -> SocketAddrV4 {
    let raw = bytes(value);
    SocketAddrV4::new(
        Ipv4Addr::new(raw[4], raw[5], raw[6], raw[7]),
        u16::from_le_bytes([raw[0], raw[1]]),
    )
}

fn timing() -> RetryTiming {
    // Original constructor48BE60, before caller540C60 selects session timing.
    RetryTiming {
        retry_delta: 2,
        max_retries: u32::MAX,
        timeout: 60,
    }
}

fn channel(send_capacity: usize, receive_capacity: usize) -> GlobalChannel {
    GlobalChannel::new(
        GlobalConfig {
            send_capacity,
            receive_capacity,
            ..Default::default()
        },
        timing(),
    )
}

#[test]
fn online_ack_requires_its_sent_destination_and_exposes_one_actual_response_sample() {
    let target = SocketAddrV4::new(Ipv4Addr::new(127, 77, 0, 30), 1234);
    let wrong = SocketAddrV4::new(Ipv4Addr::new(127, 77, 0, 2), 1234);
    let mut sender = channel(8, 8);
    sender.require_authenticated_acknowledgments();
    let sequence = sender.next_send_sequence();
    assert!(sender.send(
        &[],
        GlobalDestination::direct(target),
        Delivery::Reliable,
        0
    ));
    let ack = sender.frame(2, sequence, &[]);
    assert_eq!(
        sender.receive(target, 0, &ack, &mut |_, _| {}),
        Err(GlobalReceiveError::UnmatchedAcknowledgment),
        "unsent messages cannot measure RTT"
    );
    sender.service(100, &mut |_, _| {});
    assert_eq!(
        sender.receive(wrong, 0, &ack, &mut |_, _| {}),
        Err(GlobalReceiveError::UnmatchedAcknowledgment)
    );
    sender.receive(target, 0, &ack, &mut |_, _| {}).unwrap();
    sender.service(110, &mut |_, _| {});
    assert_eq!(
        sender.pop_response_time(),
        Some(GlobalResponseTime {
            destination: GlobalDestination::direct(target),
            sequence,
            elapsed: 10,
        })
    );
    assert!(sender.pop_response_time().is_none());
    assert_eq!(sender.queued_send_count(), 0);
    sender.reset();
    assert!(sender.send(
        &[],
        GlobalDestination::direct(target),
        Delivery::Reliable,
        0
    ));
    sender.service(120, &mut |_, _| {});
    assert_eq!(
        sender.receive(wrong, 0, &ack, &mut |_, _| {}),
        Err(GlobalReceiveError::UnmatchedAcknowledgment),
        "reset retains online source policy"
    );
}

fn assert_emitted(actual: &[(GlobalDestination, Vec<u8>)], expected: &Value) {
    let rows = expected.as_array().unwrap();
    assert_eq!(actual.len(), rows.len());
    for ((destination, frame), row) in actual.iter().zip(rows) {
        assert_eq!(frame, &bytes(&row["frame"]));
        let expected_destination = if row["kind"] == "broadcast" {
            GlobalDestination::BROADCAST
        } else {
            GlobalDestination::Direct {
                endpoint: endpoint(&row["endpoint"]),
                port_override: row["port"].as_u64().unwrap() as u16,
            }
        };
        assert_eq!(*destination, expected_destination);
    }
}

fn assert_state(actual: &GlobalChannel, expected: &Value) {
    assert_eq!(actual.queued_send_count() as u64, expected["send_count"]);
    assert_eq!(
        actual.queued_receive_count() as u64,
        expected["receive_count"]
    );
    assert_eq!(
        u64::from(actual.send.total_enqueued()),
        expected["total_send"]
    );
    assert_eq!(u64::from(actual.total_received), expected["total_receive"]);
    assert_eq!(
        u64::from(actual.mean_response_time()),
        expected["response"][2]
    );
    assert_eq!(actual.next_seen as u64, expected["ring_cursor"]);
    let ring = expected["ring"].as_array().unwrap();
    assert_eq!(actual.seen.len(), ring.len());
    for (actual, expected) in actual.seen.iter().zip(ring) {
        assert_eq!(u64::from(actual.sequence), expected["sequence"]);
        assert_eq!(actual.source, endpoint(&expected["endpoint"]));
    }
    for (actual, expected) in actual
        .receive
        .iter()
        .zip(expected["receives"].as_array().unwrap())
    {
        assert_eq!(actual.frame, bytes(&expected["frame"]));
        assert_eq!(actual.source, endpoint(&expected["source"]));
        assert_eq!(
            actual.consumed,
            expected["flags"].as_u64().unwrap() & 2 != 0
        );
    }
}

fn receive(
    channel: &mut GlobalChannel,
    source: SocketAddrV4,
    port_override: u16,
    frame: &[u8],
    expected: &Value,
) -> GlobalReceiveOutcome {
    let mut emitted = Vec::new();
    let outcome = channel
        .receive(source, port_override, frame, &mut |destination, frame| {
            emitted.push((destination, frame.to_vec()));
        })
        .unwrap();
    assert_emitted(&emitted, &expected["emitted"]);
    assert_state(channel, &expected["state"]);
    outcome
}

fn service(channel: &mut GlobalChannel, now: u32, expected: &Value) {
    let mut emitted = Vec::new();
    let successful = channel.service(now, &mut |destination, frame| {
        emitted.push((destination, frame.to_vec()))
    });
    assert_eq!(successful, expected["returned"] == 1);
    assert_emitted(&emitted, &expected["emitted"]);
    assert_state(channel, &expected["state"]);
}

fn pop(channel: &mut GlobalChannel, expected: &Value) {
    let actual = channel.pop_received();
    assert_eq!(actual.is_some(), expected["returned"] == 1);
    if let Some(actual) = actual {
        assert_eq!(actual.payload, bytes(&expected["payload"]));
        assert_eq!(actual.source, endpoint(&expected["endpoint"]));
        assert_eq!(u64::from(actual.product_id), expected["product"]);
    }
    assert_state(channel, &expected["state"]);
}

#[test]
fn global_frames_and_payload_extents_match_original_constructors_and_receivers() {
    let data = corpus();
    let mut defaults = GlobalChannel::new(GlobalConfig::default(), timing());
    assert_state(&defaults, &data["retail_constructor"]);
    // Native corpus verifies actual queue capacity by constructor inputs; a
    // full queue probe through the production admission avoids exposing state.
    for _ in 0..160 {
        assert!(defaults.send(&[], GlobalDestination::BROADCAST, Delivery::Unreliable, 0));
    }
    assert!(!defaults.send(&[], GlobalDestination::BROADCAST, Delivery::Unreliable, 0));

    for case in data["payload_extents"].as_array().unwrap() {
        let mut sender = channel(8, 8);
        let mut emitted = Vec::new();
        assert!(sender.send(
            &bytes(&case["input"]),
            GlobalDestination::direct(endpoint(&data["endpoints"]["b"])),
            Delivery::Reliable,
            0
        ));
        assert!(sender.service(1000, &mut |destination, frame| {
            emitted.push((destination, frame.to_vec()))
        }));
        assert_emitted(&emitted, &serde_json::json!([case["native_frame"]]));
        let mut receiver = channel(8, 8);
        receive(
            &mut receiver,
            endpoint(&data["endpoints"]["a"]),
            0,
            &emitted[0].1,
            &case["receive"],
        );
        pop(&mut receiver, &case["pop"]);
    }
}

#[test]
fn unknown_sources_duplicates_full_queue_and_blocked_fifo_match_original() {
    let data = corpus();
    let basic = &data["basic"];
    let frame = bytes(&basic["native_frame"]["frame"]);
    let a = endpoint(&data["endpoints"]["a"]);
    let b = endpoint(&data["endpoints"]["b"]);
    let c = endpoint(&data["endpoints"]["c"]);
    let mut receiver = channel(8, 2);
    assert_eq!(
        receive(&mut receiver, a, 0, &frame, &basic["unknown_a"]),
        GlobalReceiveOutcome::Queued
    );
    assert_eq!(
        receive(
            &mut receiver,
            b,
            0,
            &frame,
            &basic["distinct_b_same_sequence"]
        ),
        GlobalReceiveOutcome::Queued
    );
    assert_eq!(
        receive(&mut receiver, a, 0, &frame, &basic["duplicate_a_when_full"]),
        GlobalReceiveOutcome::DuplicateAcknowledged
    );
    assert_eq!(
        receive(&mut receiver, c, 0, &frame, &basic["new_c_when_full"]),
        GlobalReceiveOutcome::DroppedFull
    );
    pop(&mut receiver, &basic["pop_a"]);
    pop(&mut receiver, &basic["pop_again_before_service"]);
    receiver.retire_consumed();
    assert_state(&receiver, &basic["cleanup_a"]["state"]);
    receive(
        &mut receiver,
        a,
        0,
        &frame,
        &basic["duplicate_a_after_cleanup"],
    );
    pop(&mut receiver, &basic["pop_b"]);
    receiver.retire_consumed();
    assert_state(&receiver, &basic["cleanup_b"]["state"]);
    receive(
        &mut receiver,
        a,
        0,
        &frame,
        &basic["duplicate_a_with_empty_queue"],
    );
    assert_eq!(
        receive(
            &mut receiver,
            c,
            0,
            &frame,
            &basic["c_retried_after_capacity_recovered"]
        ),
        GlobalReceiveOutcome::Queued
    );
}

/// Native retires a direct send on an ACK from any host. A LAN host other
/// than the destination can no longer cancel the message; its destination's
/// ACK still retires it with one response sample.
#[test]
fn ack_from_unrelated_source_leaves_the_direct_send_queued() {
    let data = corpus();
    let mut sender = channel(8, 8);
    sender.send(
        b"global-A",
        GlobalDestination::direct(endpoint(&data["endpoints"]["b"])),
        Delivery::Reliable,
        0,
    );
    sender.service(1000, &mut |_, _| {});
    let expected = &data["wrong_source_ack"];
    assert_state(&sender, &expected["sender_before"]);
    let ack = bytes(&expected["ack_frame"]);
    for (source, now, queued) in [("c", 1000, 1), ("b", 1004, 0)] {
        assert_eq!(
            sender.receive(
                endpoint(&data["endpoints"][source]),
                0,
                &ack,
                &mut |_, _| {}
            ),
            Ok(GlobalReceiveOutcome::Acknowledgment)
        );
        sender.service(now, &mut |_, _| {});
        assert_eq!(sender.queued_send_count(), queued);
    }
    assert_eq!(sender.mean_response_time(), 4);
}

#[test]
fn response_reset_preserves_global_delivery_sequence_and_receive_dedup() {
    let data = corpus();
    let source = endpoint(&data["endpoints"]["a"]);
    let destination = endpoint(&data["endpoints"]["b"]);
    let mut sender = channel(8, 8);
    let mut ack = bytes(&data["wrong_source_ack"]["ack_frame"]);
    assert!(sender.send(
        b"history",
        GlobalDestination::direct(destination),
        Delivery::Reliable,
        0
    ));
    sender.service(1000, &mut |_, _| {});
    sender
        .receive(destination, 0, &ack, &mut |_, _| {})
        .unwrap();
    sender.service(1004, &mut |_, _| {});
    assert_eq!(sender.mean_response_time(), 4);
    assert!(sender.send(
        b"pending",
        GlobalDestination::direct(destination),
        Delivery::Reliable,
        0
    ));
    let mut first_send = Vec::new();
    sender.service(1005, &mut |_, frame| first_send.push(frame.to_vec()));
    let incoming = bytes(&data["basic"]["native_frame"]["frame"]);
    assert_eq!(
        sender
            .receive(source, 0, &incoming, &mut |_, _| {})
            .unwrap(),
        GlobalReceiveOutcome::Queued
    );
    let before = format!("{sender:?}");
    sender.reset_response_time(false);
    assert_eq!(format!("{sender:?}"), before);
    sender.reset_response_time(true);
    assert_eq!(sender.mean_response_time(), 0);
    assert_eq!(sender.queued_send_count(), 1);
    assert_eq!(sender.queued_receive_count(), 1);
    assert_eq!(sender.send.total_enqueued(), 2);
    assert_eq!(sender.total_received, 1);
    let mut retried = Vec::new();
    sender.service(1010, &mut |_, frame| retried.push(frame.to_vec()));
    assert_eq!(retried, first_send);
    ack[4..8].copy_from_slice(&1_u32.to_le_bytes());
    sender
        .receive(destination, 0, &ack, &mut |_, _| {})
        .unwrap();
    sender.service(1011, &mut |_, _| {
        panic!("ACKed global message retransmitted")
    });
    assert_eq!(
        sender.mean_response_time(),
        6,
        "ACK uses first send, not reset or retry time"
    );
    assert_eq!(sender.queued_send_count(), 0);
    assert_eq!(
        sender
            .receive(source, 0, &incoming, &mut |_, _| {})
            .unwrap(),
        GlobalReceiveOutcome::DuplicateAcknowledged
    );
    assert_eq!(sender.pop_received().unwrap().payload, b"global-A");
    assert!(sender.pop_received().is_none());
    assert!(sender.send(
        b"next",
        GlobalDestination::direct(destination),
        Delivery::Reliable,
        0
    ));
    let mut next = Vec::new();
    sender.service(1012, &mut |_, frame| next.push(frame.to_vec()));
    assert_eq!(sequence(&next[0]), 2);
}

#[test]
fn aggregate_sequence_overrides_retry_and_failed_cleanup_match_original() {
    let data = corpus();
    let mut sender = channel(3, 8);
    for (index, case) in data["global_sequence"]["admission"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
    {
        let delivery = if case["reliable"] == true {
            Delivery::Reliable
        } else {
            Delivery::Unreliable
        };
        assert_eq!(
            sender.send(
                &[index as u8 + 1],
                GlobalDestination::direct(endpoint(&data["endpoints"]["b"])),
                delivery,
                case["override"].as_u64().unwrap() as u32
            ),
            case["send"]["returned"] == 1
        );
        assert_state(&sender, &case["send"]["state"]);
    }
    service(&mut sender, 1000, &data["global_sequence"]["service"]);
    service(&mut sender, 1002, &data["retry_failure"]["equal_delta"]);
    service(&mut sender, 1003, &data["retry_failure"]["greater_delta"]);
    service(&mut sender, 1064, &data["retry_failure"]["timeout"]);
    assert_eq!(
        sender.discard_failed_sends() as u64,
        data["retry_failure"]["failed_cleanup_returned"]
    );
    assert_state(&sender, &data["retry_failure"]["after_failed_cleanup"]);
}

#[test]
fn global_product_and_source_port_are_preserved_for_the_consumer() {
    let data = corpus();
    let a = endpoint(&data["endpoints"]["a"]);
    let mut foreign = channel(8, 8);
    let expected = &data["foreign_product"];
    receive(
        &mut foreign,
        a,
        0,
        &bytes(&expected["native_frame"]),
        &expected["receive"],
    );
    pop(&mut foreign, &expected["pop"]);
    let mut normalized = channel(8, 8);
    let expected = &data["source_port_normalization"];
    receive(
        &mut normalized,
        a,
        4567,
        &bytes(&data["basic"]["native_frame"]["frame"]),
        expected,
    );
    pop(&mut normalized, &expected["pop"]);
}

#[test]
fn broadcast_downgrades_requested_reliability_like_original() {
    let data = corpus();
    let mut sender = channel(8, 8);
    assert!(sender.send(
        b"global-broadcast",
        GlobalDestination::BROADCAST,
        Delivery::Reliable,
        0
    ));
    service(&mut sender, 1000, &data["null_destination_broadcast"]);
}

#[test]
fn a_reliable_batch_is_delivered_once_every_recipient_acks_its_own_sequence() {
    let first = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1234);
    let second = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 1234);
    let mut sender = channel(8, 8);
    assert!(sender.send(
        b"chat",
        GlobalDestination::direct(first),
        Delivery::Reliable,
        0
    ));
    let batch = sender
        .send_reliable_batch(b"release", &[first, second])
        .unwrap();
    sender.service(1000, &mut |_, _| {});
    // Sequence 0 is the earlier chat; the batch holds 1 and 2.
    for (source, sequence, delivered) in [(first, 0, false), (first, 1, false), (second, 2, true)] {
        let ack = sender.frame(2, sequence, &[]);
        sender.receive(source, 0, &ack, &mut |_, _| {}).unwrap();
        sender.service(1001, &mut |_, _| {});
        assert_eq!(
            sender.batch_delivered(batch),
            delivered,
            "after ACK {sequence}"
        );
    }
}

#[test]
fn explicit_consume_cancellation_and_reset_preserve_owner_boundaries() {
    let data = corpus();
    let b = endpoint(&data["endpoints"]["b"]);
    let mut sender = channel(8, 8);
    let to = |port_override| GlobalDestination::Direct {
        endpoint: b,
        port_override,
    };
    assert!(sender.send(b"first", to(0), Delivery::Reliable, 0));
    assert!(sender.send(b"other port", to(1235), Delivery::Reliable, 0));
    let mut received = Vec::new();
    sender.service(1000, &mut |_, frame| received.push(frame.to_vec()));
    // The override names the endpoint; an ACKed send is not cancelled.
    assert_eq!(sender.cancel_sends_to_endpoint(b), 1);
    let mut ack = received[1].clone();
    ack[2] = 2;
    sender.receive(b, 0, &ack, &mut |_, _| {}).unwrap();
    let mut overridden = b;
    overridden.set_port(1235);
    assert_eq!(sender.cancel_sends_to_endpoint(overridden), 0);
    assert_eq!(sender.queued_send_count(), 1);
    let mut receiver = channel(8, 8);
    for frame in &received {
        receiver.receive(b, 0, frame, &mut |_, _| {}).unwrap();
    }
    assert_eq!(receiver.peek_received(0).unwrap().payload, b"first");
    assert!(receiver.consume_received(0));
    assert!(receiver.peek_received(0).is_none());
    assert!(receiver.pop_received().is_none());
    receiver.service(1000, &mut |_, _| {});
    assert_eq!(receiver.pop_received().unwrap().payload, b"other port");
    receiver.reset();
    assert_state(&receiver, &data["basic"]["constructor"]);
    receiver
        .receive(b, 0, &received[0], &mut |_, _| {})
        .unwrap();
    assert_eq!(receiver.pop_received().unwrap().payload, b"first");
}
