use super::*;

fn corpus() -> serde_json::Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/storage_oracle/lan_messages.json",
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
fn all_discovery_join_builders_match_original_complete_payloads() {
    let data = corpus();
    let name = data["name"].as_str().unwrap();
    let game = data["game"].as_str().unwrap();
    let serial = data["serial"].as_str().unwrap().as_bytes();
    for case in data["packets"].as_array().unwrap() {
        let message = match case["name"].as_str().unwrap() {
            "discovery" => LanMessage::named(LanNamedMessage::Discovery, name).unwrap(),
            "game_query" => LanMessage::named(LanNamedMessage::GameQuery, game).unwrap(),
            "game_query_low_zero" => {
                LanMessage::named(LanNamedMessage::GameQuery, "A\u{100}B").unwrap()
            }
            "presence" => LanMessage::presence(name, 2, 6).unwrap(),
            "advertisement" => LanMessage::advertisement(game, true).unwrap(),
            "participant" => LanMessage::participant(
                name,
                serial,
                LanParticipantDetails {
                    country: -3,
                    color: 6,
                    game_name_hash: game_name_hash(game),
                    field_0x53: 4,
                    field_0x57: 0,
                },
            )
            .unwrap(),
            "join" => LanMessage::join_request(
                name,
                serial,
                LanJoinRequest {
                    country: -3,
                    color: 6,
                    version: LanVersionInterval::RETAIL,
                    compatibility: LanCompatibility {
                        field_0x43: 0x11223344,
                        field_0x47: 0x55667788,
                        field_0x4b: 0x99aabbcc,
                        field_0x4f: 25,
                    },
                    observer_request: 0x123,
                },
            )
            .unwrap(),
            "acceptance" => LanMessage::join_accepted(
                name,
                LanAcceptance {
                    country: -3,
                    color: 6,
                    assigned_row: 3,
                    observer_row: 7,
                },
            )
            .unwrap(),
            "denial" => LanMessage::join_denied(5),
            "kick" => LanMessage::join_denied(4),
            "departure" => LanMessage::named(LanNamedMessage::Departure, name).unwrap(),
            "keepalive" => LanMessage::named(LanNamedMessage::Keepalive, name).unwrap(),
            name => panic!("unhandled native control {name}"),
        };
        let expected = unhex(case["payload"].as_str().unwrap());
        assert_eq!(message.as_bytes().as_slice(), expected, "{}", case["name"]);
        assert_eq!(LanMessage::decode(&expected).unwrap(), message);
    }
}

#[test]
fn game_name_hash_matches_complete_original_wide_hash() {
    for case in corpus()["game_name_hashes"].as_array().unwrap() {
        let units: Vec<u16> = case["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u.as_u64().unwrap() as u16)
            .collect();
        let text = String::from_utf16(&units).unwrap();
        assert_eq!(
            u64::from(game_name_hash(&text)),
            case["value"].as_u64().unwrap(),
            "{units:?}"
        );
    }
}

#[test]
fn readers_keep_signed_fields_and_raw_unknown_bytes() {
    let data = corpus();
    for case in data["packets"].as_array().unwrap() {
        let message = LanMessage::decode(&unhex(case["payload"].as_str().unwrap())).unwrap();
        match message.kind() {
            LanMessageKind::JoinRequest => {
                let fields = message.join_fields().unwrap();
                assert_eq!((fields.country, fields.color), (-3, 6));
                assert_eq!(fields.observer_request, 0x23);
                assert_eq!(message.serial_bytes(), b"1234567890123456789012");
            }
            LanMessageKind::JoinAccepted => assert_eq!(
                message.acceptance_fields().unwrap(),
                LanAcceptance {
                    country: -3,
                    color: 6,
                    assigned_row: 3,
                    observer_row: 7
                }
            ),
            LanMessageKind::Advertisement => assert_eq!(message.advertisement_flags(), Some(1)),
            LanMessageKind::JoinDenied => assert_eq!(
                message.denial_reason(),
                Some(if case["name"] == "kick" { 4 } else { 5 })
            ),
            _ => {}
        }
    }
    let mut raw = [0xa5; LAN_MESSAGE_LEN];
    raw[..4].copy_from_slice(&0x87654321u32.to_le_bytes());
    let unknown = LanMessage::decode(&raw).unwrap();
    assert_eq!(unknown.kind(), LanMessageKind::Unknown(0x87654321));
    assert_eq!(unknown.into_bytes(), raw);
    assert!(matches!(
        LanMessage::decode(&raw[..454]),
        Err(LanMessageError::WrongLength { actual: 454 })
    ));
}

#[test]
fn text_copies_use_fixed_narrow_fields_without_spilling_into_following_fields() {
    assert!(LanMessage::named(LanNamedMessage::Discovery, &"x".repeat(19)).is_ok());
    assert!(matches!(
        LanMessage::named(LanNamedMessage::Discovery, &"x".repeat(20)),
        Err(LanMessageError::TextTooLong { field: "name", .. })
    ));
    let message = LanMessage::named(LanNamedMessage::Discovery, "éÿ").unwrap();
    assert_eq!(message.name_bytes(), &[0xe9, 0xff]);
    assert_eq!(message.name(), "éÿ");
}
