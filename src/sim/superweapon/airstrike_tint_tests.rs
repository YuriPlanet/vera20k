use super::*;
use crate::sim::superweapon::invulnerability::{InvulnKind, InvulnerabilityState};
use serde_json::Value;

fn oracle() -> &'static Value {
    crate::test_fixture::json("tools/airstrike_tint.json")
}

fn int(v: &Value) -> i32 {
    v.as_i64().unwrap() as i32
}

#[test]
fn tint_histories_and_shared_scenario_draws_match_native() {
    for row in oracle()["histories"].as_array().unwrap() {
        let start = int(&row["start"]);
        let mut air = AirstrikeTintState::default();
        air.start(start, int(&row["air_duration"]));
        let mut iron = row["iron_duration"].as_i64().map(|duration| {
            InvulnerabilityState::new(
                CdTimer::started(start, duration as i32),
                InvulnKind::IronCurtain,
            )
        });
        let mut rng = SimRng::new(row["seed"].as_u64().unwrap());
        let mut active = true;
        let mut expected = row["initial"]["air"].clone();
        let mut steps = row["steps"].as_array().unwrap().iter().peekable();
        let (_, draws) = crate::sim::rng::trace_draws(|| {
            for offset in 0..int(&row["frames"]) {
                let frame = start + offset;
                if let Some(ops) = row["operations"][offset.to_string()].as_array() {
                    for op in ops {
                        match op.as_str().unwrap() {
                            "clear_marker" | "other_target" => active = false,
                            "restore_marker" | "self_target" => active = true,
                            "stop_outer" => air.stop(frame),
                            op => panic!("unexpected operation {op}"),
                        }
                    }
                }
                // Actual Techno AI slice 006F9EAD..006F9EBB calls these in order.
                if let Some(iron) = iron.as_mut() {
                    iron.update_tint(frame, &mut rng);
                }
                air.update_tint(frame, active, &mut rng);
                if steps
                    .peek()
                    .is_some_and(|step| int(&step["offset"]) == offset)
                {
                    expected = steps.next().unwrap()["air"].clone();
                }
                assert_eq!(
                    [
                        i32::from(air.tint_stage),
                        air.tint_timer.start_frame(),
                        air.tint_timer.duration()
                    ],
                    [int(&expected[0]), int(&expected[1]), int(&expected[2])],
                    "{} frame {offset}",
                    row["name"]
                );
            }
        });
        assert!(steps.next().is_none());
        assert_eq!(
            draws.len(),
            row["raw_rng_updates"].as_u64().unwrap() as usize,
            "{}",
            row["name"]
        );
        assert_eq!(
            rng.native_state_hex(),
            row["rng_after"].as_str().unwrap(),
            "{}",
            row["name"]
        );
        assert_eq!(
            [air.timer.start_frame(), air.timer.duration()],
            [int(&row["outer_final"][0]), int(&row["outer_final"][1])]
        );
    }
}

#[test]
fn sequential_iron_and_airstrike_intensity_matches_native_wrapping_and_caps() {
    let data = oracle();
    let inputs: Vec<_> = data["inputs"].as_array().unwrap().iter().map(int).collect();
    for row in data["intensities"].as_array().unwrap() {
        let iron = InvulnerabilityState::with_tint(
            CdTimer::default(),
            InvulnKind::IronCurtain,
            int(&row[0]) as u8,
            CdTimer::from_raw(-1, int(&row[1])),
        );
        let air =
            AirstrikeTintState::with_tint(int(&row[2]) as u8, CdTimer::from_raw(-1, int(&row[3])));
        let actual: Vec<_> = inputs
            .iter()
            .map(|&input| air.effect_tint_intensity(iron.effect_tint_intensity(input, 5000), 5000))
            .collect();
        let expected: Vec<_> = row[4].as_array().unwrap().iter().map(int).collect();
        assert_eq!(
            actual, expected,
            "iron {} left {}; air {} left {}",
            row[0], row[1], row[2], row[3]
        );
    }
}
