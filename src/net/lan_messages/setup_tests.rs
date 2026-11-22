use super::*;
use crate::net::lan_messages::LanPayload;
use crate::test_fixture::unhex;

fn corpus() -> &'static serde_json::Value {
    crate::test_fixture::json("tools/storage_oracle/lan_messages.json")
}

#[test]
fn setup_builders_match_complete_original_packets() {
    let data = corpus();
    let sender = data["name"].as_str().unwrap();
    for case in data["setup_packets"].as_array().unwrap() {
        let message = match case["name"].as_str().unwrap() {
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
fn bounded_views_preserve_reserved_bytes() {
    let options =
        LanMessage::options("Host", &[0xc1, 0x80, 0x80, 0x80], 0x87654321, 0xabcdef01).unwrap();
    let LanPayload::Options(fields) = options.payload() else {
        panic!("options payload");
    };
    assert_eq!(fields.text, [0xc1, 0x80, 0x80, 0x80]);
    assert_eq!(
        (fields.color, fields.game_name_hash),
        (0x87654321, 0xabcdef01)
    );
    assert!(LanMessage::options("Host", &[b'x'; 399], 0, 0).is_ok());
    assert!(LanMessage::options("Host", &[b'x'; 400], 0, 0).is_err());
    let mut bytes = options.into_bytes();
    bytes[0x20] = 0xa5;
    assert_eq!(LanMessage::from_bytes(bytes).into_bytes(), bytes);
    assert_eq!(
        LanMessage::go(-1).payload(),
        LanPayload::Go { max_ahead: -1 }
    );
    let choices = LanMessage::player_choices("Host", b"R-2,6,-2,-2").unwrap();
    assert_eq!(
        choices.payload(),
        LanPayload::PlayerChoices {
            text: b"R-2,6,-2,-2".as_slice()
        }
    );
    assert_eq!(&choices.as_bytes()[447..], &[0; 8]);
}
