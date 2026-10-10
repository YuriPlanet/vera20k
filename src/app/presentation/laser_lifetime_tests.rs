//! Native constructor/Logic/reset comparisons. The corpus executes retail
//! 54FE60, 550150 and scene-clear534949->550000; it does not replay IStream.

use super::*;
use crate::sim::intern::InternedId;
use crate::sim::projectile::ProjectileCoord;
use serde_json::Value;

fn native() -> Value {
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/building_prism.json",
    ))
    .unwrap()
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
        color: LaserColor::House(InternedId::default()),
    }
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
    assert_eq!(actual.rgb, rgb(expected), "{context} copied color");
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
            lasers.update(integer(&state["frame"]));
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
        lasers.draws().map(|draw| draw.rgb).collect::<Vec<_>>(),
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
    lasers.update(201);
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
