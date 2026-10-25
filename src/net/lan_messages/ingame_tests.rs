use super::*;

fn corpus() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/storage_oracle/lan_ingame_messages.json",
    ))
    .unwrap()
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|offset| u8::from_str_radix(&text[offset..offset + 2], 16).unwrap())
        .collect()
}

#[test]
fn loading_chat_and_taunt_fields_match_original_construction() {
    for case in corpus()["packets"].as_array().unwrap() {
        let original = unhex(case["payload"].as_str().unwrap());
        let name = case["name"].as_str().unwrap();
        let game = case["game"].as_str().unwrap();
        let color = case["color"].as_u64().unwrap() as u32;
        if case["kind"] == "loading" {
            let wire_percent = case["wire_percent"].as_i64().unwrap() as i32;
            let read = LoadingProgressMessage::decode(&original).unwrap();
            assert_eq!(read.name(), name);
            assert_eq!(read.percent(), wire_percent);
            assert_eq!(read.as_bytes(), original);
            let made = LoadingProgressMessage::new(name, wire_percent).unwrap();
            assert_eq!(made.as_bytes(), original);
            continue;
        }
        let read = LanMessage::decode(&original).unwrap();
        assert_eq!(read.name(), name);
        assert_eq!(read.as_bytes().as_slice(), original);
        let made = if case["kind"] == "chat" {
            let text = case["text"].as_str().unwrap();
            let fields = read.in_game_chat_fields().unwrap();
            assert_eq!(fields.text(), text);
            assert_eq!(
                fields.text_units().collect::<Vec<_>>(),
                text.encode_utf16().collect::<Vec<_>>()
            );
            assert_eq!(fields.relay_mask, 0);
            assert_eq!(fields.color, color);
            assert_eq!(fields.game_name_hash, game_name_hash(game));
            assert!(read.in_game_taunt_fields().is_none());
            LanMessage::in_game_chat(name, color, game, text).unwrap()
        } else {
            assert_eq!(case["kind"], "taunt");
            let byte = case["byte"].as_u64().unwrap() as u8;
            let fields = read.in_game_taunt_fields().unwrap();
            assert_eq!(fields.byte, byte);
            assert_eq!(fields.relay_mask, 0);
            assert_eq!(fields.color, color);
            assert_eq!(fields.game_name_hash, game_name_hash(game));
            assert!(read.in_game_chat_fields().is_none());
            LanMessage::in_game_taunt(name, color, game, byte).unwrap()
        };
        // Original chat/taunt leave unrelated buffer bytes untouched. The
        // oracle records executed writes, not a second field-layout model.
        for offset in case["written_offsets"].as_array().unwrap() {
            let offset = offset.as_u64().unwrap() as usize;
            assert_eq!(
                made.as_bytes()[offset],
                original[offset],
                "{} offset{offset:#x}",
                case["kind"]
            );
        }
    }
}

#[test]
fn loading_preserves_signed_percent_and_all_short_record_bytes() {
    for percent in [i32::MIN, -1, 0, 100, i32::MAX] {
        let made = LoadingProgressMessage::new("A\u{100}B", percent).unwrap();
        assert_eq!(made.as_bytes().len(), LOADING_PROGRESS_LEN);
        assert_eq!(made.name_bytes(), b"A");
        let mut bytes = made.as_bytes().to_vec();
        bytes[0x2e] = 0xfe;
        let read = LoadingProgressMessage::decode(&bytes).unwrap();
        assert_eq!(read.percent(), percent);
        assert_eq!(read.as_bytes(), bytes);
    }
    let made = LoadingProgressMessage::new("Host", 25).unwrap();
    assert!(matches!(
        LoadingProgressMessage::decode(&made.as_bytes()[..50]),
        Err(InGameMessageError::WrongLoadingLength { actual: 50 })
    ));
    let mut wrong = made.as_bytes().to_vec();
    wrong[0] = CHAT_KIND as u8;
    assert!(matches!(
        LoadingProgressMessage::decode(&wrong),
        Err(InGameMessageError::WrongLoadingKind { actual: CHAT_KIND })
    ));
}

#[test]
fn chat_preserves_utf16_and_uses_field_capacity_before_color() {
    let text = format!("{}😀", "x".repeat(109));
    let made = LanMessage::in_game_chat("Host", 0xf1234567, "Game", &text).unwrap();
    let fields = made.in_game_chat_fields().unwrap();
    assert_eq!(fields.text(), text);
    assert_eq!(fields.text_units().count(), 111);
    assert_eq!(fields.color, 0xf1234567);
    assert!(matches!(
        LanMessage::in_game_chat("Host", 1, "Game", &"😀".repeat(56)),
        Err(InGameMessageError::ChatTooLong {
            actual: 112,
            maximum: 111
        })
    ));
    let stopped = LanMessage::in_game_chat("Host", 2, "Game", "A\0B").unwrap();
    assert_eq!(stopped.in_game_chat_fields().unwrap().text(), "A");

    let mut raw = made.into_bytes();
    raw[RELAY_OFFSET] = 0xa5;
    raw[CHAT_TEXT_OFFSET..CHAT_TEXT_OFFSET + 4].copy_from_slice(&[0, 0xd8, 0, 0]);
    raw[400] = 0xfe;
    let read = LanMessage::decode(&raw).unwrap();
    let fields = read.in_game_chat_fields().unwrap();
    assert_eq!(fields.relay_mask, 0xa5);
    assert_eq!(fields.text_units().collect::<Vec<_>>(), [0xd800]);
    assert_eq!(fields.text(), "�");
    assert_eq!(read.into_bytes(), raw);
}

#[test]
fn taunt_preserves_raw_byte_and_does_not_repack_it() {
    let made = LanMessage::in_game_taunt("Host", u32::MAX, "Game", 0xff).unwrap();
    let mut raw = made.into_bytes();
    raw[RELAY_OFFSET] = 0x80;
    raw[454] = 0x39;
    let read = LanMessage::decode(&raw).unwrap();
    let fields = read.in_game_taunt_fields().unwrap();
    assert_eq!(fields.byte, 0xff);
    assert_eq!(fields.relay_mask, 0x80);
    assert_eq!(fields.color, u32::MAX);
    assert_eq!(read.into_bytes(), raw);
}

#[test]
fn beacon_records_carry_coord_slot_and_house_at_native_offsets() {
    let placed = BeaconRecord::Placed {
        house: 2,
        slot: 1,
        coord: [0x1a80, -0x80, 0x1a0],
    };
    let made = BeaconMessage::new("Host", placed).unwrap();
    let bytes = made.as_bytes();
    assert_eq!(bytes.len(), BEACON_LEN);
    assert_eq!(&bytes[..4], &0x20u32.to_le_bytes());
    assert_eq!(&bytes[4..9], b"Host\0");
    assert_eq!(&bytes[0x2f..0x33], &0x1a80i32.to_le_bytes());
    assert_eq!(&bytes[0x33..0x37], &(-0x80i32).to_le_bytes());
    assert_eq!(&bytes[0x37..0x3b], &0x1a0i32.to_le_bytes());
    assert_eq!((bytes[0x3b], bytes[0x3c]), (1, 2));
    assert_eq!(BeaconMessage::decode(bytes).unwrap().record(), placed);

    let deleted = BeaconMessage::new("Host", BeaconRecord::Deleted { house: 7, slot: 2 }).unwrap();
    let mut raw = deleted.as_bytes().to_vec();
    assert_eq!(&raw[..4], &0x21u32.to_le_bytes());
    // The receiver sign-extends both bytes.
    raw[0x3c] = 0xff;
    assert_eq!(
        BeaconMessage::decode(&raw).unwrap().record(),
        BeaconRecord::Deleted { house: -1, slot: 2 }
    );
    assert!(matches!(
        BeaconMessage::decode(&raw[..60]),
        Err(InGameMessageError::WrongBeaconLength { actual: 60 })
    ));
    raw[0] = 0x22;
    assert!(matches!(
        BeaconMessage::decode(&raw),
        Err(InGameMessageError::WrongBeaconKind { actual: 0x22 })
    ));
}

#[test]
fn scoped_loading_rejects_a_prior_match_and_prior_final_options() {
    let scope = SessionScope {
        epoch: [7; 16],
        revision: 9,
    };
    let progress = LoadingProgressMessage::new("Host", 100).unwrap();
    let bytes = progress.scoped_bytes(scope);
    let received = LoadingProgressMessage::decode_scoped(&bytes, scope).unwrap();
    assert_eq!(received.name(), "Host");
    assert_eq!(received.percent(), 100);
    assert!(
        LoadingProgressMessage::decode_scoped(
            &bytes,
            SessionScope {
                epoch: [8; 16],
                ..scope
            }
        )
        .is_err()
    );
    assert!(
        LoadingProgressMessage::decode_scoped(
            &bytes,
            SessionScope {
                revision: 10,
                ..scope
            }
        )
        .is_err()
    );
    assert!(LoadingProgressMessage::decode_scoped(progress.as_bytes(), scope).is_err());
}
