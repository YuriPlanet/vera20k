//! Turn streams (`turn_streams`): each Logic slot's turn draws from its own
//! stream, and the Scenario table gives each live object pass one draw.

use super::Simulation;
use crate::rules::ruleset::RuleSet;
use crate::sim::rng::{SimRng, trace_draws};
use crate::sim::scenario_session::ScenarioGameMode;
use std::collections::BTreeMap;

/// A skirmish of two human houses, each with a power plant, and `count`
/// retail GIs (Americans) standing on Guard in a row: each GI scans every
/// targeting delay and fidgets, drawing in its own turn, and nothing draws
/// outside the turns.
fn standing_gis(rules: &RuleSet, count: u16, streams: bool) -> Simulation {
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 0x7055_0000_0001, 32);
    let russians = sim.interner.intern("Russians");
    sim.houses_mut_for_test()
        .get_mut(&russians)
        .expect("the arena's computer house")
        .set_is_human_for_test(true);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    sim.session_mut_for_test()
        .set_game_mode(ScenarioGameMode::Skirmish);
    // A house left without buildings loses on the first frame.
    for (house, cell) in [("Americans", 4), ("Russians", 26)] {
        sim.spawn_object("GAPOWR", house, cell, cell, 0, rules)
            .expect("a plant spawns");
    }
    for n in 0..count {
        sim.spawn_object("E1", "Americans", 8 + 3 * n, 16, 0, rules)
            .expect("a GI spawns");
    }
    sim.separate_turn_streams(streams);
    sim
}

fn frame(sim: &mut Simulation, rules: &RuleSet) {
    let commands = sim.take_due_commands();
    sim.advance_app_frame(
        &commands,
        rules,
        33,
        super::TickLane::Ordinary,
        &mut super::FrameEffects::default(),
    )
    .expect("an ordinary frame");
}

/// How many draws take `before`'s table to `after`'s.
fn table_draws(before: &SimRng, after: &SimRng) -> usize {
    let mut table = before.clone();
    for count in 0..10_000 {
        if table.state() == after.state() {
            return count;
        }
        table.next_u32();
    }
    panic!("the table moved by 10,000 draws or more");
}

/// With the switch off every turn draws the Scenario table in Logic order,
/// as gamemd does, and the order decides what each object draws. With it on
/// the table gives each pass one draw, its key; each slot's draws come from
/// its own object's stream, the same values on the same frames with the
/// Logic vector reversed; and idle sleep's check, which compares the stream
/// as a sleeper's turn begins and ends, still finds those turns draw nothing.
#[test]
fn turns_draw_their_own_streams_in_any_logic_order() {
    let Some(fixture) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &fixture.rules;
    let run = |streams: bool, reversed: bool| {
        let mut sim = standing_gis(rules, 4, streams);
        if reversed {
            let mut order = sim.logic_order().to_vec();
            order.reverse();
            sim.set_logic_order_for_test(order);
        }
        sim.sleep_idle_objects(true);
        sim.check_idle_sleep(true);
        let mut by_object = BTreeMap::<u64, Vec<(u32, u64)>>::new();
        for n in 0..200 {
            let at = sim.session.binary_frame();
            let (before, main) = (sim.scenario_rng.clone(), sim.main_rng.state());
            let (_, draws) = trace_draws(|| frame(&mut sim, rules));
            assert_eq!(sim.main_rng.state(), main, "frame {n} drew from Main");
            let count = |stream: &str, in_slot: bool| {
                draws
                    .iter()
                    .filter(|draw| draw["stream"] == stream)
                    .filter(|draw| !draw["logic_object"].is_null() == in_slot)
                    .count()
            };
            // Every traced table draw is the Scenario stream's.
            let advanced = table_draws(&before, &sim.scenario_rng);
            assert_eq!(advanced, count("table", true) + count("table", false));
            assert_eq!(
                count("turn", false),
                0,
                "frame {n}: a turn draw outside a slot"
            );
            if streams {
                assert_eq!(count("table", true), 0, "frame {n}: a turn drew the table");
                assert_eq!(advanced, 1, "frame {n}: the pass's key alone");
            } else {
                assert_eq!(
                    count("table", false),
                    0,
                    "frame {n}: a draw outside the turns"
                );
                assert_eq!(count("turn", true), 0, "frame {n}: a turn stream");
            }
            for draw in draws.iter().filter(|draw| !draw["logic_object"].is_null()) {
                if streams {
                    assert_eq!(draw["turn_object"], draw["logic_object"], "frame {n}");
                }
                let object = draw["logic_object"].as_u64().unwrap();
                let value = draw["value"].as_u64().unwrap();
                by_object.entry(object).or_default().push((at, value));
            }
        }
        let (_, checked) = sim.idle_sleep_counts();
        assert!(checked > 50, "{checked} sleeping turns were checked");
        by_object
    };
    let forward = run(true, false);
    let drawn: usize = forward.values().map(Vec::len).sum();
    assert!(
        forward.len() > 2 && drawn > 20,
        "{} objects drew {drawn} times",
        forward.len()
    );
    assert_eq!(forward, run(true, true));
    assert_ne!(run(false, false), run(false, true));
}
