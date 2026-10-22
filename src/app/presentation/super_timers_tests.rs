//! Native comparisons for the timer lines, from `tools/superweapon_oracle.py`'s
//! sections `tactical_timers` and `timer_lines`.

use super::*;
use crate::render::bit_font::BitFont;
use crate::render::bit_font::tests::make_test_font;
use crate::rules::color_scheme::ColorSchemeEntry;
use crate::sim::timer::CdTimer;
use serde_json::{Value, json};

fn oracle() -> &'static Value {
    crate::test_fixture::json("tools/superweapon_oracle.json")
}

fn int(value: &Value) -> i32 {
    value.as_i64().unwrap() as i32
}

fn timer(value: &Value) -> CdTimer {
    CdTimer::from_raw(int(&value[0]), int(&value[1]))
}

/// The row's house `index`.
fn house(index: usize) -> InternedId {
    InternedId::from_index(100 + index as u32)
}

/// `0x006D4941..0x006D4B25` against the native rows: each line's index, the
/// house whose scheme prints it, its seconds, its label and its blink
/// pointers, with the GameMode 0 hold skip.
#[test]
fn the_lines_match_native() {
    let oracle = oracle();
    let rows = oracle["tactical_timers"].as_array().unwrap();
    assert_eq!(rows.len(), 11);
    for row in rows {
        let frame = int(&row["frame"]);
        let houses = row["houses"].as_array().unwrap();
        let supers = row["supers"].as_array().unwrap();
        let views: Vec<SuperTimerView> = supers
            .iter()
            .enumerate()
            .map(|(slot, sw)| {
                // GetRechargeTime `0x006CC260`: the Super's own time, else
                // the type's.
                let custom = int(&sw["custom"]);
                SuperTimerView::for_test(
                    house(sw["owner"].as_u64().unwrap() as usize),
                    InternedId::from_index(slot as u32),
                    CdTimer::from_raw(int(&sw["start"]), int(&sw["left"])).remaining(frame),
                    sw["hold"].as_bool().unwrap(),
                    if custom != -1 {
                        custom
                    } else {
                        int(&sw["recharge"])
                    },
                    slot as u32,
                )
            })
            .collect();
        let scenario = &row["scenario"];
        let scenario_left = (int(&scenario[0]) != -1).then(|| timer(scenario).remaining(frame));
        let blackouts = houses
            .iter()
            .enumerate()
            .map(|(index, entry)| (house(index), timer(&entry[1]).remaining(frame)));
        let lines = timer_lines(
            scenario_left,
            &views,
            blackouts,
            int(&row["game_mode"]) == 0,
        );

        // The fixture's labels: the Scenario's "Mission", each Super's name
        // and the string table's "Blackout".
        let player = house(row["player"].as_u64().unwrap() as usize);
        let scheme = |id: InternedId| houses[(id.index() - 100) as usize][0].clone();
        let ours: Vec<Value> = lines
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let (owner, label, blink) = match line.source {
                    TimerSource::Scenario => (player, json!("Mission"), Value::Null),
                    TimerSource::Super(view) => (
                        view.owner(),
                        supers[view.place() as usize]["name"].clone(),
                        json!(view.place()),
                    ),
                    TimerSource::Blackout(blackout) => (blackout, json!("Blackout"), Value::Null),
                };
                json!(["line", index, scheme(owner), line.seconds, label, blink])
            })
            .collect();
        let events = row["events"].as_array().unwrap();
        let native: Vec<Value> = events.iter().filter(|e| e[0] == "line").cloned().collect();
        assert_eq!(ours, native, "{row}");
        let strings: Vec<&Value> = events.iter().filter(|e| e[0] == "load_string").collect();
        let blackout_lines = lines
            .iter()
            .filter(|line| matches!(line.source, TimerSource::Blackout(_)))
            .count();
        assert_eq!(strings.len(), blackout_lines, "{row}");
        assert!(strings.iter().all(|e| e[1] == BLACKOUT_LABEL), "{row}");
    }
}

/// The oracle's measure, `timer_glyph_width`: 4 pixels a space, 6 a digit or
/// colon, 8 any other character, and a pixel of spacing after each.
fn oracle_font() -> BitFont {
    let mut widths: Vec<(u16, u32)> = ('0'..=':').map(|c| (c as u16, 6)).collect();
    widths.extend(('A'..='Z').chain('a'..='z').map(|c| (c as u16, 8)));
    make_test_font(&widths, 4)
}

/// The RGB565 word the native surface stores.
fn rgb565([r, g, b]: [u8; 3]) -> u16 {
    u16::from(r >> 3) << 11 | u16::from(g >> 2) << 5 | u16::from(b >> 3)
}

/// `0x006D4B50` against the native rows: the texts, the black boxes and
/// where each prints, the colours, and the blink's step at zero seconds.
#[test]
fn each_line_prints_as_native() {
    let font = oracle_font();
    let oracle = oracle();
    let rows = oracle["timer_lines"].as_array().unwrap();
    assert_eq!(rows.len(), 26);
    for row in rows {
        let hsv = |key: &str| -> [u8; 3] { serde_json::from_value(row[key].clone()).unwrap() };
        // The line's scheme is runtime scheme 0, so `[Colors]` entry 0, and
        // ColorScheme::Array[5] is entry 2.
        let entry = |hsv| ColorSchemeEntry {
            name: String::new(),
            hsv,
        };
        let schemes = [
            entry(hsv("scheme_hsv")),
            entry([0; 3]),
            entry(hsv("blink_hsv")),
        ];
        let owner = house_text_rgb(&schemes, HouseColorIndex(0));
        let seconds = int(&row["seconds"]);
        let label = label_text(row["label"].as_str().unwrap());
        let time = time_text(seconds);
        let view = [int(&row["view"][0]), int(&row["view"][1])];
        let (label_at, time_at) = place_line(
            row["index"].as_u64().unwrap() as usize,
            view,
            int(&row["height"]),
            font.text_width(&label) as i32,
            font.text_width(&time) as i32,
        );
        let time_rgb = match row["blink"].as_array() {
            None => owner,
            Some(blink) => {
                let mut state = TimerBlink {
                    on: blink[0] == 1,
                    deadline_ms: blink[1].as_u64().unwrap(),
                    place: 0,
                };
                let blinking = state.step(seconds, row["now"].as_u64().unwrap());
                assert_eq!(
                    json!([u8::from(state.on), state.deadline_ms]),
                    row["blink_after"],
                    "{row}"
                );
                if blinking {
                    house_text_rgb(&schemes, BLINK_SCHEME)
                } else {
                    owner
                }
            }
        };
        // The print clips to the whole view, as the tactical scissor does.
        let clip = json!([0, 0, view[0], view[1]]);
        let ours = vec![
            json!(["fill", label_at.fill, 0]),
            json!(["print", label, label_at.x, label_at.y, rgb565(owner), clip]),
            json!(["fill", time_at.fill, 0]),
            json!(["print", time, time_at.x, time_at.y, rgb565(time_rgb), clip]),
        ];
        let native: Vec<Value> = row["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e[0] != "measure")
            .cloned()
            .collect();
        assert_eq!(ours, native, "{row}");
    }
}

/// A new grant clears the blink (`+0x40`, Grant `0x006CB580`) but leaves its
/// deadline (`+0x48`).
#[test]
fn a_new_grant_clears_the_blink_but_not_its_deadline() {
    let (mut sim, owner, sw_type) = timer_phase_world();
    let mut blinks = TimerBlinks::default();
    let grant = |place| SuperTimerView::for_test(owner, sw_type, 0, false, 900, place);
    let view = grant(4);
    assert!(TimerBlink::of(&mut blinks, &view).step(0, 5000));
    assert!(TimerBlink::of(&mut blinks, &view).step(0, 5999));
    let saved = capture_timer_blinks(&blinks, &sim);
    let mut loaded = prepare_timer_blinks(saved.clone(), &sim).unwrap();
    let blink = TimerBlink::of(&mut loaded, &view);
    assert!(!blink.step(5, 5000));
    assert!(blink.on, "nonzero seconds preserve the retained phase");
    assert!(
        !blink.step(0, 5000),
        "load clears the deadline and toggles saved-on"
    );
    sim.super_weapons_mut_for_test()
        .get_mut(&owner)
        .unwrap()
        .get_mut(&sw_type)
        .unwrap()
        .set_timer_place_for_test(Some(7));
    let reconciled = reconcile_saved_timer_blinks(&saved, &sim);
    assert_eq!((reconciled[0].place, reconciled[0].on), (7, false));
    loaded = prepare_timer_blinks(Vec::new(), &sim).unwrap();
    assert!(
        loaded.is_empty(),
        "an empty incoming map replaces outgoing phases"
    );
    let regranted = grant(7);
    let blink = TimerBlink::of(&mut blinks, &regranted);
    assert_eq!((blink.on, blink.deadline_ms), (false, 6000));
    assert!(!blink.step(0, 5999));
    assert!(blink.step(0, 6000));
}

/// Supplied retained list identity; the tests exercise presentation/envelope
/// ownership rather than replaying the already-owned House grant producer.
fn timer_phase_world() -> (crate::sim::world::Simulation, InternedId, InternedId) {
    let mut sim = crate::sim::world::Simulation::new();
    let owner = sim.intern("Americans");
    let sw_type = sim.intern("NukeSpecial");
    sim.houses_mut_for_test()
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    let mut instance = crate::sim::superweapon::SuperWeaponInstance::new(sw_type, owner, 0);
    instance.set_is_active_for_test(true);
    instance.set_timer_place_for_test(Some(4));
    sim.super_weapons_mut_for_test()
        .entry(owner)
        .or_default()
        .insert(sw_type, instance);
    (sim, owner, sw_type)
}

#[test]
fn saved_timer_phase_round_trips_then_toggles_at_the_first_zero_draw() {
    use crate::sim::snapshot::{GameSnapshot, saved_local_view::SavedLocalView};
    let (sim, owner, sw_type) = timer_phase_world();
    let round_trip = |on| {
        let mut blinks = TimerBlinks::default();
        blinks.insert(
            (owner, sw_type),
            TimerBlink {
                on,
                deadline_ms: u64::MAX,
                place: 4,
            },
        );
        let phases = capture_timer_blinks(&blinks, &sim);
        let mut local = SavedLocalView::new(
            [[0; 2]; 4],
            Vec::new(),
            false,
            std::array::from_fn(|_| Vec::new()),
            None,
            Default::default(),
        )
        .unwrap();
        local.set_super_timer_blinks(phases);
        let bytes = GameSnapshot::save_validated_with_presentation(
            &sim,
            0,
            0,
            "phase",
            0,
            &[],
            None,
            None,
            Some(&local),
            None,
        );
        let mut saved = GameSnapshot::load(&bytes).unwrap();
        let before = saved.sim.state_hash();
        let phases = saved.take_local_view().unwrap().take_super_timer_blinks();
        let mut restored = prepare_timer_blinks(phases, &saved.sim).unwrap();
        assert_eq!(
            saved.sim.state_hash(),
            before,
            "phase restoration cannot alter lockstep state"
        );
        let blink = restored.get_mut(&(owner, sw_type)).unwrap();
        assert_eq!((blink.on, blink.deadline_ms), (on, 0));
        let visible = blink.step(0, 5000);
        assert_eq!(blink.deadline_ms, 6000);
        visible
    };
    // SuperLoad6CDFB6/BD preserves +40 and resets +48/+4C. DrawTimer6D4C75
    // immediately toggles: saved-on shows owner colour, saved-off blink colour.
    assert!(!round_trip(true));
    assert!(round_trip(false));
}
