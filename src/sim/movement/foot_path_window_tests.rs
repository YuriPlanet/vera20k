//! Foot's native 24-word copy boundary and an ordinary stock long Move order.
//!
//! The stock scene uses the production rules, spawn, command and whole-frame
//! movement owners on an authored clear arena. It is a Rust continuation
//! regression, not a native route or frame-timing comparison.

use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::components::{FootPathQueue, NavTargetRef};
use crate::sim::overlay_grid::OverlayGrid;

const OWNER: &str = "Americans";
const START: (u16, u16) = (4, 32);
const DESTINATION: (u16, u16) = (58, 32);
const WINDOW_WORDS: usize = 24;

/// The original Unit setter and Drive/Ship Process reach Find_Path4D3920;
/// only the AStar result at 4CBBA0 is supplied. This comparison covers the
/// shared copy owner (4D3E82..4D3EA1) and reference write (4D4003), not a
/// complete native search or subsequent movement. Native's unused backing
/// words after -1 are intentionally excluded: Rust ends the live Vec instead.
#[test]
fn installed_path_window_matches_original_find_path_copy() {
    let corpus = crate::test_fixture::json("tools/spatial_oracle/foot_path_window.json");
    assert_eq!(corpus["schema_version"], 1);
    let rows = corpus["cases"].as_array().expect("native cases");
    assert_eq!(rows.len(), 10, "both callers retain all five copy controls");
    for row in rows {
        let input = &row["input"];
        let name = input["name"].as_str().expect("case name");
        assert_eq!(input["prefix"], 0, "{name}: ordinary replacement scope");
        assert_eq!(row["wrapper_return"]["returned_al"], 1, "{name}");
        let cell = |value: &serde_json::Value| {
            (
                u16::try_from(value[0].as_u64().expect("cell x")).expect("cell x fits"),
                u16::try_from(value[1].as_u64().expect("cell y")).expect("cell y fits"),
            )
        };
        let reference = cell(&input["reference_cell"]);
        let route: Vec<_> = input["route_cells"]
            .as_array()
            .expect("complete supplied route")
            .iter()
            .map(cell)
            .collect();
        assert_eq!(route.first(), Some(&reference), "{name}");
        let mut queue = FootPathQueue::default();
        queue.install(reference, &route, 1);
        let expected: Vec<_> = row["wrapper_return"]["live_words"]
            .as_array()
            .expect("native live copy")
            .iter()
            .map(|word| u8::try_from(word.as_u64().expect("live word")).expect("word fits"))
            .collect();
        assert_eq!(queue.remaining_directions(), expected, "{name}");
        let native_reference = cell(&row["wrapper_return"]["reference"]);
        assert_eq!(
            queue.reference_cell(),
            Some((native_reference.0 as i16, native_reference.1 as i16)),
            "{name}"
        );
    }
}

fn stock_long_move(rules: &RuleSet, type_id: &str, batched: bool) {
    let context = format!("{type_id}, batched={batched}");
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(rules, 0x4D3E82, 64);
    sim.set_input_delay_ticks(0);
    sim.overlay_grid = Some(OverlayGrid::new(64, 64));
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    if batched {
        sim.batch_path_searches();
    }
    let grid = sim.path_grid_snapshot().expect("arena navigation");
    let mover = sim
        .spawn_object(type_id, OWNER, START.0, START.1, 64, rules)
        .unwrap_or_else(|| panic!("{context}: stock mover spawns"));
    let destination = Some(NavTargetRef::cell(DESTINATION.0, DESTINATION.1));
    let owner = sim.interner.intern(OWNER);
    let order = CommandEnvelope::new(
        owner,
        sim.session.tick(),
        Command::Move {
            entity_id: mover,
            target_rx: DESTINATION.0,
            target_ry: DESTINATION.1,
        },
    );
    let result = sim.advance_tick(&[order], Some(rules), Some(&grid), None, 33);
    assert_eq!(result.executed_commands, 1, "{context}: Move admitted");

    let mut saw_first_window = false;
    let mut saw_replacement = false;
    let mut previous_cursor = 0;
    let mut saw_batch_search = false;
    let mut arrived = false;
    for _ in 0..2400 {
        let actor = sim
            .substrate
            .entities
            .get(mover)
            .unwrap_or_else(|| panic!("{context}: mover stays alive"));
        let queue = actor.navigation.path_replay();
        let position = (actor.position.rx(), actor.position.ry());
        if !saw_first_window && !queue.remaining_directions().is_empty() {
            // Find_Path4D3E82..4D3E9F copies 24 direction words, not the
            // complete distant route. Movement may already have consumed one
            // word, but the queue's backing Vec retains that paid prefix.
            assert_eq!(
                queue.directions().len(),
                WINDOW_WORDS,
                "{context}: the first long route is a bounded window"
            );
            assert_eq!(actor.navigation.nav_com(), destination, "{context}");
            assert_ne!(position, DESTINATION, "{context}");
            saw_first_window = true;
        }
        assert!(
            queue.remaining_directions().len() <= WINDOW_WORDS,
            "{context}: the live Foot queue exceeded its native capacity"
        );
        if saw_first_window && !saw_replacement && queue.cursor() < previous_cursor {
            // Consumption only advances the cursor. The ordinary long-trip
            // continuation must install a fresh window while keeping NavCom.
            // There are no supplied path answers or queue writes in this scene.
            assert!(
                previous_cursor >= 20,
                "{context}: the first window was nearly spent"
            );
            assert_eq!(queue.directions().len(), WINDOW_WORDS, "{context}");
            assert!(queue.remaining_directions().len() >= 22, "{context}");
            assert_eq!(actor.navigation.nav_com(), destination, "{context}");
            assert_ne!(position, START, "{context}");
            assert_ne!(position, DESTINATION, "{context}");
            saw_replacement = true;
        }
        previous_cursor = queue.cursor();
        saw_batch_search |= sim
            .path_batch_stats()
            .is_some_and(|stats| stats.searched > 0);
        if position == DESTINATION && actor.navigation.nav_com().is_none() {
            arrived = true;
            break;
        }
        sim.advance_tick(&[], Some(rules), Some(&grid), None, 33);
    }
    let actor = sim
        .substrate
        .entities
        .get(mover)
        .unwrap_or_else(|| panic!("{context}: mover stays alive"));
    assert!(saw_first_window, "{context}: the Move must find a path");
    assert!(
        saw_replacement,
        "{context}: the distant destination must require another window"
    );
    assert!(
        arrived,
        "{context}: mover stopped at {:?}, NavCom {:?}, queue {:?}",
        (actor.position.rx(), actor.position.ry()),
        actor.navigation.nav_com(),
        actor.navigation.path_replay()
    );
    assert_eq!(
        saw_batch_search, batched,
        "{context}: the requested search mode ran"
    );
}

#[test]
fn retail_tank_and_infantry_replace_path_windows_and_reach_their_move_destination() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    // Batching intentionally changes scheduling. Both modes must preserve the
    // copy boundary and complete the same order; their frame counts may differ.
    for type_id in ["MTNK", "E1"] {
        assert!(
            !rules.object(type_id).expect("stock mover rules").is_train,
            "{type_id}: ordinary zero-prefix Find_Path caller"
        );
        for batched in [false, true] {
            stock_long_move(rules, type_id, batched);
        }
    }
}
