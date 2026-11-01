//! Native constructor/Logic/reset comparisons. The corpus executes retail
//! 54FE60, 550150 and scene-clear534949->550000; it does not replay IStream.

use super::*;
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use crate::sim::rng::{MainRng, SimRng};
use serde_json::Value;

fn native() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/building_prism.json")
}

fn integer(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn coordinate(value: &Value) -> ProjectileCoord {
    ProjectileCoord::new(integer(&value[0]), integer(&value[1]), integer(&value[2]))
}

fn birth(value: &Value) -> LaserBirth {
    LaserBirth {
        frame: integer(&value["timer_start"]),
        from: coordinate(&value["source"]),
        to: coordinate(&value["target"]),
        z_adjust: integer(&value["z_adjust"]),
        duration: integer(&value["duration"]),
        width: integer(&value["width"]),
        supported: integer(&value["supported"]) != 0,
        color: if integer(&value["house_color"]) != 0 {
            LaserColor::House(InternedId::default())
        } else {
            LaserColor::Explicit {
                inner: rgb(value),
                outer: std::array::from_fn(|i| integer(&value["outer"][i]) as u8),
                spread: std::array::from_fn(|i| integer(&value["spread"][i]) as u8),
            }
        },
        fade_end: crate::util::native_x87::NativeF32Bits::from_bits(
            value["fade_end_bits"].as_u64().unwrap() as u32,
        ),
        track: None,
    }
}

#[test]
fn original_general_main_continuation_is_sampled_once_per_composite() {
    let native = crate::test_fixture::json("tools/spatial_oracle/disk_laser.json");
    let rows = native["laser_draw"].as_array().unwrap();
    assert_eq!(rows.len(), 46, "selected GeneralDraw control corpus");
    assert!(
        rows.iter().any(|row| {
            row["input"]["name"] == "nonzero_spread_order"
                && row["raw_draws"]
                    .as_array()
                    .is_some_and(|draws| !draws.is_empty())
        }),
        "native nonzero-spread control must expose actual raw draws"
    );
    for row in rows {
        let value = &row["laser"];
        assert_eq!(integer(&value["house_color"]), 0);
        let name = row["input"]["name"].as_str().unwrap();
        let main: MainRng =
            SimRng::from_native_state_hex_for_test(row["rng_before"]["main"].as_str().unwrap())
                .into();
        let mut lasers = Lasers::default();
        lasers.create(birth(value), |_| {
            panic!("explicit color has no House lookup")
        });
        lasers.live[0].age = integer(&value["age"]);
        let before = lasers
            .observations()
            .map(|value| serde_json::to_value(value).unwrap())
            .collect::<Vec<_>>();
        let ((), trace) = crate::sim::rng::trace_draws(|| lasers.composite(&mut main.draws()));
        assert_eq!(
            main.native_state_hex(),
            row["rng_after"]["main"].as_str().unwrap(),
            "{name}: complete Main state after draw"
        );
        // Ranged65C7E0 has an inline raw path: its65C79D/65C84B
        // unmasked-word observations, not calls to65C780, establish the
        // actual words and order consumed by rejection sampling.
        let expected_raw = row["raw_draws"]
            .as_array()
            .unwrap()
            .iter()
            .map(|draw| draw["word"].as_u64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            trace.len(),
            expected_raw.len(),
            "{name}: actual raw draws including rejection"
        );
        assert_eq!(
            trace
                .iter()
                .map(|draw| draw["value"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            expected_raw,
            "{name}: unmasked raw words in consumption order"
        );
        assert_eq!(
            lasers
                .observations()
                .map(|value| serde_json::to_value(value).unwrap())
                .collect::<Vec<_>>(),
            before,
            "{name}: compositing does not age or rewrite copied birth inputs"
        );
        let sampled = main.native_state_hex();
        // Upload and repeated consumers read the resolved cache. They do
        // not receive a Main capability and cannot resample color spread.
        for _ in 0..2 {
            for draw in lasers.draws() {
                draw.lines([0; 2], true, |_| {});
            }
        }
        assert_eq!(main.native_state_hex(), sampled, "{name}: cached replay");
    }
}

fn assert_joined_laser_registry(lasers: &Lasers, state: &Value, births: &[Value], context: &str) {
    let order = state["laser_order"].as_array().unwrap();
    let ages = state["laser_ages"].as_array().unwrap();
    assert_eq!(
        order.len(),
        ages.len(),
        "{context}: native registry columns"
    );
    // Native names select constructor outputs only on the expected side.
    // The production owner retains copied inputs, not Abstract IDs or links
    // to the shot. Compare those full values in actual registry order.
    let expected = order
        .iter()
        .zip(ages)
        .map(|(name, age)| {
            let original = births
                .iter()
                .find(|row| row["name"] == *name)
                .expect("native registered beam has a constructor output");
            (birth(&original["laser"]), integer(age))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        lasers
            .live
            .iter()
            .map(|laser| (laser.birth, laser.age))
            .collect::<Vec<_>>(),
        expected,
        "{context}: copied registry order and ages"
    );
    let expected_draws = expected
        .iter()
        .rev()
        .map(|(birth, age)| {
            (
                birth.from,
                birth.to,
                birth.z_adjust,
                birth.width,
                birth.supported,
                *age,
                birth.duration,
                birth.fade_end,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        lasers
            .draws()
            .map(|draw| {
                (
                    draw.from,
                    draw.to,
                    draw.z_adjust,
                    draw.width,
                    draw.supported,
                    draw.age,
                    draw.duration,
                    draw.fade_end,
                )
            })
            .collect::<Vec<_>>(),
        expected_draws,
        "{context}: reverse birth order reaches the draw consumer"
    );
}

#[test]
fn original_joined_disk_births_keep_the_common_laser_lifetime() {
    let native = crate::test_fixture::json("tools/spatial_oracle/disk_laser.json");
    let histories = native["lifecycle"].as_array().unwrap();
    assert_eq!(histories.len(), 10, "joined native lifecycle controls");
    let mut checked_states = 0;
    let mut copied_births = 0;
    let mut checked_final = false;
    let mut checked_duplicate_expiry = false;
    let mut checked_reverse = false;
    for history in histories {
        let name = history["input"]["name"].as_str().unwrap();
        let births = history["laser_births"].as_array().unwrap();
        let states = history["states"].as_array().unwrap();
        assert!(!states.is_empty(), "{name}: native phase boundaries");
        let mut lasers = Lasers::default();
        let mut next_birth = 0;
        for (ordinal, state) in states.iter().enumerate() {
            let frame = integer(&state["frame"]);
            let phase = state["phase"].as_str().unwrap();
            match phase {
                "disk" => {
                    if let Some(next) = births.get(next_birth) {
                        assert!(
                            integer(&next["frame"]) >= frame,
                            "{name}: no native birth skipped before Disk boundary"
                        );
                    }
                    while let Some(next) = births.get(next_birth)
                        && integer(&next["frame"]) == frame
                    {
                        let value = &next["laser"];
                        assert_eq!(integer(&value["timer_start"]), frame);
                        assert_eq!(integer(&value["age"]), 0);
                        lasers.create(birth(value), |_| {
                            panic!("native General birth has copied RGB")
                        });
                        next_birth += 1;
                    }
                }
                "laser_update" => {
                    assert_eq!(state["logic_frame"], state["frame"]);
                    lasers.update(frame, None);
                }
                // Only actual births and original550150 visits mutate this
                // owner. Shot construction, expiry, duplicate retirement and
                // the later common delete drain cannot detach copied beams.
                "constructor" | "fire" | "action" | "drain" => {}
                other => panic!("{name}: unhandled native phase {other}"),
            }
            assert_joined_laser_registry(
                &lasers,
                state,
                births,
                &format!("{name} state{ordinal} frame{frame} {phase}"),
            );
            checked_states += 1;
        }
        assert_eq!(
            next_birth,
            births.len(),
            "{name}: every constructor consumed"
        );
        copied_births += next_birth;
        assert_eq!(states.last().unwrap()["phase"], "drain");
        assert!(
            lasers.live.is_empty(),
            "{name}: common lifetime naturally retires every beam"
        );
        if !births.is_empty() {
            assert!(
                states.iter().any(|state| {
                    state["phase"] == "drain"
                        && state["disk_order"].as_array().unwrap().is_empty()
                        && state["pending"].as_array().unwrap().is_empty()
                        && !state["laser_order"].as_array().unwrap().is_empty()
                }),
                "{name}: copied beams outlive the shot and common delete drain"
            );
        }
        match name {
            "ordinary" => {
                assert_eq!(births.len(), 17, "sixteen ring pieces and the final beam");
                let final_beam = &births.last().unwrap()["laser"];
                assert_eq!(integer(&final_beam["duration"]), 15);
                assert_eq!(final_beam["fade_end_bits"], 0);
                checked_final = true;
            }
            "duplicate_owner_expiry" => {
                assert_eq!(
                    states
                        .iter()
                        .filter(|state| state["phase"] == "action")
                        .count(),
                    2
                );
                assert_eq!(births.len(), 2);
                checked_duplicate_expiry = true;
            }
            "reverse_birth_order" => {
                assert_eq!(births.len(), 34);
                assert_eq!(births[0]["shot"], "newer");
                checked_reverse = true;
            }
            _ => {}
        }
    }
    assert_eq!(
        copied_births, 78,
        "all joined native LaserDraw constructors"
    );
    assert_eq!(checked_states, 479, "all native phase states consumed");
    assert!(checked_final && checked_duplicate_expiry && checked_reverse);
}

fn rgb(value: &Value) -> [u8; 3] {
    std::array::from_fn(|i| integer(&value["inner"][i]) as u8)
}

fn assert_state(lasers: &Lasers, state: &Value, context: &str) {
    assert_eq!(
        lasers.live.len(),
        state["registered"].as_u64().unwrap() as usize,
        "{context}"
    );
    let expected = &state["laser"];
    if expected.is_null() {
        assert!(lasers.draws().next().is_none(), "{context}");
        return;
    }
    let actual = &lasers.live[0];
    assert_eq!(actual.age, integer(&expected["age"]), "{context} age");
    assert_eq!(
        actual.timer.start_frame(),
        integer(&expected["timer_start"]),
        "{context} timer start"
    );
    assert_eq!(
        actual.timer.duration(),
        integer(&expected["timer_duration"]),
        "{context} timer duration"
    );
    assert_eq!(
        actual.color.inner(),
        rgb(expected),
        "{context} copied color"
    );
    assert_eq!(
        actual.birth.from,
        coordinate(&expected["source"]),
        "{context} copied source"
    );
    assert_eq!(
        actual.birth.to,
        coordinate(&expected["target"]),
        "{context} copied target"
    );
}

#[test]
fn original_logic_lifetimes_include_repeated_skipped_and_wrapping_frames() {
    let native = native();
    let rows = native["laser_lifetime"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    for row in rows {
        let name = row["input"]["name"].as_str().unwrap();
        let states = row["states"].as_array().unwrap();
        let initial = &states[0]["laser"];
        let mut lasers = Lasers::default();
        lasers.create(birth(initial), |_| rgb(initial));
        assert_state(&lasers, &states[0], name);
        for state in &states[1..] {
            lasers.update(integer(&state["frame"]), None);
            assert_state(&lasers, state, &format!("{name} frame {}", state["frame"]));
        }
    }
}

#[test]
fn original_reverse_draw_order_and_scene_clear_use_one_registry() {
    let native = native();
    let row = native["laser_draw"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "reverse_draw_order")
        .unwrap();
    let first = birth(&row["laser"]);
    let first_rgb = rgb(&row["laser"]);
    // These are the second original constructor's explicit inputs in
    // LaserPixels; its allocator gives consecutive 96-byte object slots.
    let second = LaserBirth {
        from: first.to,
        to: first.from,
        z_adjust: -2,
        width: 1,
        supported: false,
        ..first
    };
    let colors = [first_rgb, [200, 150, 100]];
    let mut lasers = Lasers::default();
    lasers.create(first, |_| colors[0]);
    lasers.create(second, |_| colors[1]);
    let expected = row["draw_order"]
        .as_array()
        .unwrap()
        .iter()
        .map(|offset| colors[offset.as_u64().unwrap() as usize / 96])
        .collect::<Vec<_>>();
    assert_eq!(
        lasers
            .draws()
            .map(|draw| draw.color.inner())
            .collect::<Vec<_>>(),
        expected
    );

    let reset = &native["laser_reset"];
    let mut reset_lasers = Lasers::default();
    for value in reset["lasers_before"].as_array().unwrap() {
        reset_lasers.create(birth(value), |_| rgb(value));
    }
    assert_eq!(
        reset_lasers.live.len(),
        reset["freed"].as_array().unwrap().len()
    );
    reset_lasers.clear_on_load();
    assert_eq!(
        reset_lasers.live.len(),
        reset["registered_after"].as_u64().unwrap() as usize
    );
    assert!(reset_lasers.draws().next().is_none());
    reset_lasers.create(first, |_| first_rgb);
    assert_eq!(reset_lasers.live.len(), 1);
    assert_eq!(reset_lasers.live[0].age, 0);
}

#[test]
fn births_after_the_logic_update_keep_constructor_age_until_the_next_visit() {
    let native = native();
    let row = native["laser_lifetime"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["input"]["name"] == "main_15")
        .unwrap();
    let initial = &row["states"][0]["laser"];
    let mut lasers = Lasers::default();
    let first = birth(initial);
    lasers.create(first, |_| rgb(initial));
    // Original Logic55B5C3 precedes object births55B5FF. This replay tests
    // the presentation consumer; the real Sim output ordering has its own test.
    lasers.update(201, None);
    lasers.create(
        LaserBirth {
            frame: 201,
            ..first
        },
        |_| rgb(initial),
    );
    assert_eq!(
        lasers.live[0].age,
        integer(&row["states"][2]["laser"]["age"])
    );
    assert_eq!(lasers.live[1].age, integer(&initial["age"]));
    assert_eq!(lasers.live[1].timer.start_frame(), 201);
    assert_eq!(
        lasers.draws().map(|draw| draw.age).collect::<Vec<_>>(),
        vec![0, 1]
    );
}
