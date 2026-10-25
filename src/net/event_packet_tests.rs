use super::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Corpus {
    input_fill: u8,
    decoded_fill: u8,
    payload_lengths: Vec<u8>,
    flows: Vec<Flow>,
    receives: Vec<Receive>,
    syncs: Vec<Sync>,
}

#[derive(Deserialize)]
struct Sync {
    frame: i32,
    lookahead: u8,
    sequence: u32,
    seed: u32,
    packet: String,
}

#[derive(Deserialize)]
struct Flow {
    name: String,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
struct Step {
    frame: i32,
    capacity: usize,
    do_count: usize,
    allowance: u32,
    inputs: Vec<String>,
    packet: String,
    count: usize,
    remaining: usize,
    mirrors: Vec<String>,
    source_headers: Vec<String>,
    timestamps: Vec<u32>,
    decoded: NativeDecode,
}

#[derive(Deserialize)]
struct Receive {
    name: String,
    packet: String,
    decoded: NativeDecode,
}

#[derive(Deserialize)]
struct NativeDecode {
    initial_count: usize,
    returned: u32,
    records: Vec<String>,
    cache: Vec<Vec<String>>,
}

fn corpus() -> Corpus {
    serde_json::from_str(crate::test_fixture::text(
        "tools/storage_oracle/lan_event_packets.json",
    ))
    .expect("native Event packet corpus")
}

fn unhex(text: &str) -> Vec<u8> {
    assert!(text.len().is_multiple_of(2));
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn record(text: &str, fill: u8) -> CommandRecord {
    let mut bytes = unhex(text);
    bytes.resize(crate::sim::command::COMMAND_RECORD_LEN, fill);
    CommandRecord::decode_exact(&bytes).unwrap()
}

fn header(packet: &[u8]) -> FrameInfo {
    decode_frame_info(packet).unwrap()
}

fn compare_decode(codec: &mut EventPacketCodec, bytes: &[u8], expected: &NativeDecode, name: &str) {
    let capacity = 0x4000 - expected.initial_count;
    let mut admitted = Vec::new();
    let result = codec.decode_packet(bytes, |record| {
        if admitted.len() == capacity {
            false
        } else {
            admitted.push(record);
            true
        }
    });
    let expected_records: Vec<_> = expected.records.iter().map(|r| record(r, 0)).collect();
    assert_eq!(admitted, expected_records, "{name}: admitted native prefix");
    let empty = std::array::from_fn(|_| Vec::<ActorToken>::new());
    let slots = codec.receive.get(&2).unwrap_or(&empty);
    let expected_cache: Vec<Vec<ActorToken>> = expected
        .cache
        .iter()
        .map(|tokens| {
            tokens
                .iter()
                .map(|hex| unhex(hex).try_into().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(
        slots.as_slice(),
        expected_cache,
        "{name}: retained native cache"
    );
    if expected.returned == u32::MAX {
        let error = result.expect_err(name);
        assert_eq!(error.kind, DecodeFailureKind::AdmissionFull, "{name}");
        assert_eq!(error.admitted_records, admitted.len(), "{name}");
        assert_eq!(error.remainder, bytes[error.offset..], "{name}");
    } else {
        let result = result.expect(name);
        assert_eq!(
            result.admitted_records, expected.returned as usize,
            "{name}"
        );
        assert_eq!(
            result.command_count(bytes),
            admitted.len().saturating_sub(1),
            "{name}"
        );
    }
}

#[test]
fn original_packet_bytes_mirrors_caches_and_expansion_match() {
    let controls = corpus();
    assert_eq!(controls.input_fill, 0xa5);
    assert_eq!(controls.decoded_fill, 0);
    assert_eq!(PAYLOAD_LENGTHS.as_slice(), controls.payload_lengths);
    for flow in controls.flows {
        let mut sender = EventPacketCodec::default();
        let mut receiver = EventPacketCodec::default();
        for (index, step) in flow.steps.into_iter().enumerate() {
            let name = format!("{} step{index}", flow.name);
            let inputs: Vec<_> = step.inputs.iter().map(|r| record(r, 0xa5)).collect();
            let bytes = unhex(&step.packet);
            //649CA0 supplies signed max(0,min(count,2000h-DoListCount)).
            let allowance = (0x2000_i32 - step.do_count as i32).min(inputs.len() as i32);
            assert_eq!(step.allowance, allowance as u32, "{name}");
            let refs: Vec<_> = inputs.iter().take(allowance.max(0) as usize).collect();
            let encoded = sender
                .encode_packet(&refs, header(&bytes), step.frame, step.capacity)
                .expect(&name);
            assert_eq!(encoded.bytes, bytes, "{name}: compressed bytes");
            assert_eq!(encoded.consumed_records(), step.count, "{name}");
            assert_eq!(
                inputs.len() - encoded.consumed_records(),
                step.remaining,
                "{name}"
            );
            let mirrors: Vec<_> = step.mirrors.iter().map(|r| record(r, 0xa5)).collect();
            assert_eq!(encoded.mirrors, mirrors, "{name}: exact local mirrors");
            assert_eq!(
                encoded.blocked,
                (step.count < refs.len()).then_some(EncodeBlock::Capacity),
                "{name}"
            );
            for (i, expected_header) in step.source_headers.iter().enumerate() {
                let source = encoded.mirrors.get(i).unwrap_or(&inputs[i]);
                assert_eq!(
                    &source.as_bytes()[..7],
                    unhex(expected_header),
                    "{name}: source{i}"
                );
            }
            assert_eq!(
                step.timestamps,
                (0..step.count).map(|i| 9000 + i as u32).collect::<Vec<_>>()
            );
            compare_decode(&mut receiver, &bytes, &step.decoded, &name);
        }
    }
}

#[test]
fn original_truncated_and_full_admission_keep_the_accepted_prefix() {
    let controls = corpus();
    for receive in controls.receives {
        compare_decode(
            &mut EventPacketCodec::default(),
            &unhex(&receive.packet),
            &receive.decoded,
            &receive.name,
        );
    }
}

#[test]
fn original_frame_sync_probe_uses_common_header_and_skips_admission() {
    for control in corpus().syncs {
        let bytes = unhex(&control.packet);
        let info = decode_frame_sync(&bytes).unwrap();
        assert_eq!(info.house_id, 2);
        assert_eq!(
            info.event_frame,
            ((control.frame + i32::from(control.lookahead) + 4) / 5) * 5
        );
        assert_eq!(info.checksum, control.seed);
        assert_eq!(info.timing_word, control.sequence as u16);
        assert_eq!(info.delay, control.lookahead);
        assert_eq!(encode_frame_sync(info).unwrap().as_slice(), bytes);
        assert!(decode_frame_info(&bytes).is_none());
        let result = EventPacketCodec::default()
            .decode_packet(&bytes, |_| panic!("FRAMESYNC is not admitted"))
            .unwrap();
        assert_eq!(result.admitted_records, 0);
        assert_eq!(result.consumed_bytes, HEADER_LEN);
    }
}

#[test]
fn blocked_external_payload_retains_encoded_prefix_and_source() {
    let controls = corpus();
    let step = &controls.flows[0].steps[0];
    let source = record(&step.inputs[0], 0xa5);
    let bytes = unhex(&step.packet);
    let extended = CommandRecord::encode(EXTENDED, 2, 25, &[0x34; 8]).unwrap();
    let mut codec = EventPacketCodec::default();
    let encoded = codec
        .encode_packet(&[&source, &extended], header(&bytes), 15, 558)
        .unwrap();
    assert_eq!(encoded.bytes, bytes);
    assert_eq!(encoded.blocked, Some(EncodeBlock::ExtendedPayload));
    assert_eq!(encoded.consumed_records(), 1);
    assert_eq!(extended.payload()[..8], [0x34; 8]);
    let mut extended_bytes = bytes;
    extended_bytes.extend([EXTENDED, 4, 0, 0, 0, 1, 2, 3, 4]);
    let mut admitted = Vec::new();
    let error = codec
        .decode_packet(&extended_bytes, |record| {
            admitted.push(record);
            true
        })
        .unwrap_err();
    assert_eq!(error.kind, DecodeFailureKind::ExtendedPayload);
    assert_eq!(error.admitted_records, 2);
    assert_eq!(error.remainder, [EXTENDED, 4, 0, 0, 0, 1, 2, 3, 4]);
}

#[test]
fn malformed_run_fails_safely_after_header_and_reset_clears_cached_lists() {
    let controls = corpus();
    let flow = controls
        .flows
        .iter()
        .find(|f| f.name == "cache_and_early_hit")
        .unwrap();
    let first = unhex(&flow.steps[0].packet);
    let repeated = unhex(&flow.steps[1].packet);
    let mut codec = EventPacketCodec::default();
    codec.decode_packet(&first, |_| true).unwrap();
    codec.decode_packet(&repeated, |_| true).unwrap();
    codec.reset();
    assert_eq!(
        codec.decode_packet(&repeated, |_| true).unwrap_err().kind,
        DecodeFailureKind::MissingCachedRun(0)
    );
    let truncated = &first[..first.len() - 1];
    let error = codec.decode_packet(truncated, |_| true).unwrap_err();
    assert_eq!(error.kind, DecodeFailureKind::TruncatedRun);
    assert_eq!(error.admitted_records, 1);
    let mut invalid = first;
    invalid[HEADER_LEN + 2 + 6] = 8;
    assert_eq!(
        codec.decode_packet(&invalid, |_| true).unwrap_err().kind,
        DecodeFailureKind::InvalidCacheSlot(8)
    );
}
