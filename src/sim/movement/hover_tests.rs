//! Hover arithmetic against original execution:
//! `tools/spatial_oracle/hover_speed_altitude.{py,json,meta.json}` runs
//! SpeedUpdate (0x00515ED0) and the altitude controller (0x00513D20).

use super::*;
use crate::rules::ini_parser::IniFile;

fn vectors() -> Vec<serde_json::Value> {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/hover_speed_altitude.json",
    ))
    .expect("hover oracle vectors")
}

fn bits(value: &serde_json::Value) -> NativeF64Bits {
    NativeF64Bits::from_bits(value.as_u64().expect("f64 bits"))
}

fn int(value: &serde_json::Value) -> i32 {
    value.as_i64().expect("integer") as i32
}

/// The oracle's two rule sets (`RULE_SETS`), written at their Rules offsets.
fn oracle_rules(name: &str) -> RuleSet {
    let [bob, boost, accel, brake, dampen] = match name {
        "constructor" => [
            0x403e_0000_0000_0000,
            0x3ff4_cccc_cccc_cccd,
            0x3f9e_b851_eb85_1eb8,
            0x3f9e_b851_eb85_1eb8,
            0x3fe9_9999_9999_999a,
        ],
        "retail" => [
            0x3fa4_7ae1_4000_0000,
            0x3ff8_0000_0000_0000,
            0x3f94_7ae1_4000_0000,
            0x3f9e_b851_e000_0000,
            0x3fd9_9999_9999_9999,
        ],
        other => panic!("rule set {other}"),
    }
    .map(NativeF64Bits::from_bits);
    let mut rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let general = &mut rules.general;
    general.hover_height = 120;
    general.hover_bob = bob;
    general.hover_boost = boost;
    general.hover_acceleration = accel;
    general.hover_brake = brake;
    general.hover_dampen = dampen;
    general.gravity = 6;
    rules
}

/// The reader's defaults are the constructor's (0x00665E2B..0x00665E81), and
/// retail `rulesmd.ini` gives the oracle's retail set.
#[test]
fn hover_keys_read_the_constructor_defaults_and_retail_values() {
    let read = |rules: &RuleSet| {
        let general = &rules.general;
        (
            general.hover_height,
            general.hover_bob,
            general.hover_boost,
            general.hover_acceleration,
            general.hover_brake,
            general.hover_dampen,
        )
    };
    let expected = |name| read(&oracle_rules(name));
    assert_eq!(
        read(&RuleSet::from_ini(&IniFile::from_str("")).unwrap()),
        expected("constructor")
    );
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let retail = RuleSet::from_ini(&ini).unwrap();
    assert_eq!(read(&retail), expected("retail"));
    assert_eq!(retail.general.gravity, 6);
}

fn coord(value: &serde_json::Value) -> DriveCoord {
    DriveCoord {
        x: int(&value[0]),
        y: int(&value[1]),
        z: int(&value[2]),
    }
}

/// SpeedUpdate from its head arm: the request (distance, power and Push
/// arms), SpeedMult and the ramp toward `min(mult * request, 1)`. The
/// oracle's zeroed steering facing turns at once, so its turn gate always
/// passes; the gate and the look-ahead facing are not compared.
#[test]
fn request_and_ramp_match_original_speed_update() {
    let mut rows = 0;
    for row in vectors()
        .iter()
        .filter(|row| row["input"]["kind"] == "speed")
    {
        let input = &row["input"];
        let rules = oracle_rules(input["rules"].as_str().unwrap());
        let path = [int(&input["path"][0]), int(&input["path"][1])];
        let destination = coord(&input["destination"]);
        let mut hover = HoverRuntime {
            destination: (destination != DriveCoord { x: 0, y: 0, z: 0 }).then_some(destination),
            head: Some(coord(&input["head"])),
            speed_request: bits(&input["request"]),
            speed_current: bits(&input["current"]),
            speed_mult: bits(&input["mult"]),
            pushed: input["pushed"] == 1,
            ..HoverRuntime::default()
        };
        // Is_Powered 0x00516C70: the powered byte or a positive height.
        let airborne = input["powered"] == 1 || int(&input["height"]) > 0;
        hover.choose_request(
            coord(&input["location"]),
            coord(&input["head"]),
            airborne,
            int(&input["frame"]) as u32,
        );
        hover.ramp_speed(path[0] != -1 && path[0] == path[1], &rules);
        assert_eq!(
            (hover.speed_request, hover.speed_current, hover.speed_mult),
            (
                bits(&row["request"]),
                bits(&row["current"]),
                bits(&row["mult"])
            ),
            "{input}"
        );
        rows += 1;
    }
    assert_eq!(rows, 336);
}

/// The altitude controller: SetHeight's argument and the spring offset. The
/// climbing test's answer is the original run's (0x00513DC8 compare); the
/// ground-ahead lookup that feeds it (`hover_altitude`) is not compared.
#[test]
fn altitude_step_matches_original_controller() {
    let mut rows = 0;
    for row in vectors()
        .iter()
        .filter(|row| row["input"]["kind"] == "altitude")
    {
        let input = &row["input"];
        let rules = oracle_rules(input["rules"].as_str().unwrap());
        let mut hover = HoverRuntime {
            bob_offset: bits(&input["bob"]),
            ..HoverRuntime::default()
        };
        let visible = hover.altitude_step(
            int(&input["height"]),
            row["climbing"].as_bool().unwrap(),
            int(&input["id"]),
            int(&input["frame"]),
            input["powered"] == 1,
            &rules,
        );
        assert_eq!(
            (visible, hover.bob_offset),
            (int(&row["visible"]), bits(&row["bob"])),
            "{input}"
        );
        rows += 1;
    }
    assert_eq!(rows, 800);
}
