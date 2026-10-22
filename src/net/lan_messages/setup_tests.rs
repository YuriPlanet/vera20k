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
fn setup_builders_match_complete_original_packets() {
    let data = corpus();
    let sender = data["name"].as_str().unwrap();
    for case in data["setup_packets"].as_array().unwrap() {
        let message = match case["name"].as_str().unwrap() {
            "roster" => LanMessage::roster(
                sender,
                &[
                    data["game"].as_str().unwrap(),
                    OPEN,
                    CLOSED,
                    WAITING,
                    EASY_AI,
                    MEDIUM_AI,
                    HARD_AI,
                    OPEN_OBSERVER,
                ],
            )
            .unwrap(),
            "options_status" => LanMessage::options(
                sender,
                b"K1",
                6,
                super::super::game_name_hash(data["game"].as_str().unwrap()),
            )
            .unwrap(),
            "player_choices" => LanMessage::player_choices(sender, b"R-2,6,-2,-2").unwrap(),
            "go" => LanMessage::go(27),
            "ready" => LanMessage::ready_to_go(),
            "start_prompt" => LanMessage::start_prompt(sender).unwrap(),
            name => panic!("unexpected native setup control {name}"),
        };
        let expected = unhex(case["payload"].as_str().unwrap());
        assert_eq!(message.as_bytes().as_slice(), expected, "{}", case["name"]);
        assert_eq!(LanMessage::decode(&expected).unwrap(), message);
    }
}

#[test]
fn packed_description_matches_original_encoder_and_decoder() {
    let data = corpus();
    for case in data["packed_descriptions"]["encodings"].as_array().unwrap() {
        let units: Vec<u16> = case["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u.as_u64().unwrap() as u16)
            .collect();
        assert_eq!(
            pack_map_description(&units),
            unhex(case["packed"].as_str().unwrap()),
            "{units:?}"
        );
    }
    for case in data["packed_descriptions"]["decodings"].as_array().unwrap() {
        let expected: Vec<u16> = case["units"]
            .as_array()
            .unwrap()
            .iter()
            .map(|u| u.as_u64().unwrap() as u16)
            .collect();
        assert_eq!(
            unpack_map_description(&unhex(case["packed"].as_str().unwrap())),
            expected,
            "{}",
            case["packed"]
        );
    }
}

fn fields(value: &serde_json::Value) -> LanGameOptions {
    let int = |key: &str| value[key].as_i64().unwrap() as i32;
    let flag = |key: &str| value[key].as_bool().unwrap();
    let string = |key: &str| value[key].as_str().unwrap().to_owned();
    let humans = value["humans"]
        .as_array()
        .unwrap()
        .iter()
        .map(|human| LanOptionsHuman {
            name: human["name"].as_str().unwrap().to_owned(),
            country: human["country"].as_i64().unwrap() as i32,
            color: human["color"].as_i64().unwrap() as i32,
            start: human["start"].as_i64().unwrap() as i32,
            team: human["team"].as_i64().unwrap() as i32,
        })
        .collect();
    let ai = std::array::from_fn(|index| {
        let ai = &value["ai"][index];
        LanOptionsAi {
            difficulty: ai["difficulty"].as_i64().unwrap() as i32,
            country: ai["country"].as_i64().unwrap() as i32,
            color: ai["color"].as_i64().unwrap() as i32,
            start: ai["start"].as_i64().unwrap() as i32,
            team: ai["team"].as_i64().unwrap() as i32,
        }
    });
    LanGameOptions {
        seed: value["seed"].as_u64().unwrap() as u32,
        current_speed: int("current_speed"),
        game_speed: int("game_speed"),
        credits: int("credits"),
        unit_count: int("unit_count"),
        short_game: flag("short_game"),
        superweapons: flag("superweapons"),
        build_off_ally: flag("build_off_ally"),
        mcv_redeploy: flag("mcv_redeploy"),
        crates: flag("crates"),
        mode_index: int("mode_index"),
        cooperative_difficulty: int("cooperative_difficulty"),
        map_description: string("map_description"),
        map_player_capacity: int("map_player_capacity"),
        official_map: flag("official_map"),
        map_file_size: int("map_file_size"),
        scenario_filename: string("scenario_filename"),
        map_digest: string("map_digest"),
        humans,
        ai,
    }
}

#[test]
fn full_options_match_original_formatter_from_native_global_and_node_inputs() {
    for case in corpus()["options_formats"].as_array().unwrap() {
        let options = fields(&case["fields"]);
        assert_eq!(
            options.encode(),
            unhex(case["text"].as_str().unwrap()),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn bounded_views_preserve_reserved_bytes_and_wide_names() {
    let data = corpus();
    let roster_case = data["setup_packets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "roster")
        .unwrap();
    let message = LanMessage::decode(&unhex(roster_case["payload"].as_str().unwrap())).unwrap();
    assert_eq!(
        message.roster_names().unwrap(),
        [
            "Game-ÿ",
            OPEN,
            CLOSED,
            WAITING,
            EASY_AI,
            MEDIUM_AI,
            HARD_AI,
            OPEN_OBSERVER
        ]
    );
    let options =
        LanMessage::options("Host", &[0xc1, 0x80, 0x80, 0x80], 0x87654321, 0xabcdef01).unwrap();
    let fields = options.options_fields().unwrap();
    assert_eq!(fields.text, [0xc1, 0x80, 0x80, 0x80]);
    assert_eq!(
        (fields.color, fields.game_name_hash),
        (0x87654321, 0xabcdef01)
    );
    assert!(LanMessage::options("Host", &[b'x'; 399], 0, 0).is_ok());
    assert!(LanMessage::options("Host", &[b'x'; 400], 0, 0).is_err());
    let text = "Ω".repeat(195);
    assert!(LanMessage::roster("Host", &[&text, "", "", "", "", "", "", ""]).is_ok());
    let text = "Ω".repeat(196);
    assert!(LanMessage::roster("Host", &[&text, "", "", "", "", "", "", ""]).is_err());
    let mut bytes = options.into_bytes();
    bytes[0x20] = 0xa5;
    assert_eq!(LanMessage::from_bytes(bytes).into_bytes(), bytes);
    assert_eq!(LanMessage::go(-1).go_max_ahead(), Some(-1));
    let choices = LanMessage::player_choices("Host", b"R-2,6,-2,-2").unwrap();
    assert_eq!(
        choices.player_choices_text(),
        Some(b"R-2,6,-2,-2".as_slice())
    );
    assert_eq!(&choices.as_bytes()[447..], &[0; 8]);
}
