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
        assert_eq!(u64::from(actual.arrived), expected["arrival"]);
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
    now: u32,
    expected: &Value,
) -> GlobalReceiveOutcome {
    let mut emitted = Vec::new();
    let outcome = channel
        .receive(
            source,
            port_override,
            frame,
            now,
            &mut |destination, frame| {
                emitted.push((destination, frame.to_vec()));
            },
        )
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
            1000,
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
        receive(&mut receiver, a, 0, &frame, 1000, &basic["unknown_a"]),
        GlobalReceiveOutcome::Queued
    );
    assert_eq!(
        receive(
            &mut receiver,
            b,
            0,
            &frame,
            1000,
            &basic["distinct_b_same_sequence"]
        ),
        GlobalReceiveOutcome::Queued
    );
    assert_eq!(
        receive(
            &mut receiver,
            a,
            0,
            &frame,
            1000,
            &basic["duplicate_a_when_full"]
        ),
        GlobalReceiveOutcome::DuplicateAcknowledged
    );
    assert_eq!(
        receive(&mut receiver, c, 0, &frame, 1000, &basic["new_c_when_full"]),
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
        1000,
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
        1000,
        &basic["duplicate_a_with_empty_queue"],
    );
    assert_eq!(
        receive(
            &mut receiver,
            c,
            0,
            &frame,
            1000,
            &basic["c_retried_after_capacity_recovered"]
        ),
        GlobalReceiveOutcome::Queued
    );
}

#[test]
fn ack_from_unrelated_source_retires_send_and_updates_one_response_history() {
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
    assert_eq!(
        receive(
            &mut sender,
            endpoint(&data["endpoints"]["c"]),
            0,
            &bytes(&expected["ack_frame"]),
            1000,
            &expected["received_from_unrelated_c"]
        ),
        GlobalReceiveOutcome::Acknowledgment
    );
    service(&mut sender, 1004, &expected["after_service"]);
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
        .receive(source, 0, &ack, 1004, &mut |_, _| {})
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
            .receive(source, 0, &incoming, 1005, &mut |_, _| {})
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
    assert_eq!(sender.receive[0].arrived, 1005);
    let mut retried = Vec::new();
    sender.service(1010, &mut |_, frame| retried.push(frame.to_vec()));
    assert_eq!(retried, first_send);
    ack[4..8].copy_from_slice(&1_u32.to_le_bytes());
    sender
        .receive(source, 0, &ack, 1011, &mut |_, _| {})
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
            .receive(source, 0, &incoming, 1011, &mut |_, _| {})
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
fn age_strip_invalidates_first_sequence_match_instead_of_matching_endpoint() {
    let data = corpus();
    let expected = &data["age_strip"];
    let frame = bytes(&data["basic"]["native_frame"]["frame"]);
    let a = endpoint(&data["endpoints"]["a"]);
    let b = endpoint(&data["endpoints"]["b"]);
    let mut receiver = channel(8, 4);
    receive(&mut receiver, a, 0, &frame, 1000, &expected["first_a"]);
    receive(&mut receiver, b, 0, &frame, 1000, &expected["second_b"]);
    pop(&mut receiver, &expected["consume_a"]);
    receiver.retire_consumed();
    assert_state(&receiver, &expected["cleanup_a"]["state"]);
    assert_eq!(receiver.strip_aged(1002, 2), 0);
    assert_state(&receiver, &expected["exact_age_retained"]);
    assert_eq!(receiver.strip_aged(1003, 2), 1);
    assert_state(&receiver, &expected["over_age_b_removed"]);
    assert_eq!(
        receive(
            &mut receiver,
            b,
            0,
            &frame,
            1003,
            &expected["b_duplicate_after_its_queue_entry_aged"]
        ),
        GlobalReceiveOutcome::DuplicateAcknowledged
    );
    assert_eq!(
        receive(
            &mut receiver,
            a,
            0,
            &frame,
            1003,
            &expected["a_reaccepted_after_first_matching_sequence_invalidated"]
        ),
        GlobalReceiveOutcome::Queued
    );
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
        1000,
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
        1000,
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
    assert_eq!(sender.cancel_sends_to(*b.ip(), 0), 1);
    assert_eq!(sender.queued_send_count(), 1);
    let mut receiver = channel(8, 8);
    for frame in &received {
        receiver.receive(b, 0, frame, 1000, &mut |_, _| {}).unwrap();
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
        .receive(b, 0, &received[0], 1000, &mut |_, _| {})
        .unwrap();
    assert_eq!(receiver.pop_received().unwrap().payload, b"first");
}

#[test]
fn cancellation_matches_native_host_and_override_without_dropping_completed_sends() {
    let data = corpus();
    let expected = &data["cancellation"];
    let b = endpoint(&data["endpoints"]["b"]);
    let mut other_port = b;
    other_port.set_port(4444);
    let mut sender = channel(8, 8);
    sender.send(
        b"completed",
        GlobalDestination::direct(b),
        Delivery::Reliable,
        0,
    );
    sender.send(
        b"same-host-other-endpoint-port",
        GlobalDestination::direct(other_port),
        Delivery::Reliable,
        0,
    );
    sender.send(
        b"other-override",
        GlobalDestination::Direct {
            endpoint: b,
            port_override: 5555,
        },
        Delivery::Reliable,
        0,
    );
    service(&mut sender, 1000, &expected["sent"]);
    receive(
        &mut sender,
        endpoint(&data["endpoints"]["c"]),
        0,
        &bytes(&data["wrong_source_ack"]["ack_frame"]),
        1000,
        &expected["acknowledged"],
    );
    assert_eq!(
        sender.cancel_sends_to(*b.ip(), 0) as u64,
        expected["cancelled"]["returned"]
    );
    assert_state(&sender, &expected["cancelled"]["state"]);
    service(&mut sender, 1004, &expected["serviced"]);
}

#[test]
fn receive_age_uses_original_signed_comparison_after_wrapping_addition() {
    let data = corpus();
    let a = endpoint(&data["endpoints"]["a"]);
    let frame = bytes(&data["basic"]["native_frame"]["frame"]);
    for case in data["age_boundaries"].as_array().unwrap() {
        let mut receiver = channel(8, 8);
        receive(
            &mut receiver,
            a,
            0,
            &frame,
            case["arrived"].as_u64().unwrap() as u32,
            &case["admitted"],
        );
        receiver.strip_aged(
            case["now"].as_u64().unwrap() as u32,
            case["maximum_age"].as_u64().unwrap() as u32,
        );
        assert_state(&receiver, &case["after"]);
    }
}
