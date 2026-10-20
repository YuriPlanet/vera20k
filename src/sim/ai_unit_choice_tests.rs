//! Replay of the chooser and harvester rows of `tools/ai_team_oracle.json`
//! against the computer's unit choices.
//!
//! Each row carries what the native run's stubs answered (CanBuild, Cost_Of
//! and the money, IsRecruitable, the draws); the Rust chooser must ask in
//! the same order and reach the same choice. The teams it tallies are those
//! the original asked for their needs (`Get_Needed_Types`); which teams
//! qualify (`TeamScriptState::wants_members`) rests on instruction reading.

use super::*;
use crate::sim::ai_team_creation::tests::{Draws, dword, events, flag, index, native_draws, rows};
use crate::sim::house_state::HouseState;
use serde_json::Value;

#[test]
fn the_choosers_match_the_original() {
    for (number, row) in rows("choosers").iter().enumerate() {
        let context = format!("{} chooser row {number}", row["kind"]);
        let teams = row["teams"].as_array().unwrap();
        let types = row["types"].as_array().unwrap();
        let tallied: Vec<(i32, Vec<usize>)> = events(row, "needed")
            .into_iter()
            .map(|asked| {
                let team = &teams[index(&asked[1])];
                // A needed type of another class (`["other", n]`) is not
                // this chooser's.
                let needed = team["needed"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(Value::as_u64)
                    .map(|class_index| class_index as usize)
                    .collect();
                (dword(&team["created"]), needed)
            })
            .collect();
        let objects = row["objects"].as_array().unwrap();
        let mut asked_recruitable = Vec::new();
        let mut asked_can_build = Vec::new();
        let candidates = tally(
            types.len(),
            tallied,
            objects
                .iter()
                .enumerate()
                .map(|(slot, object)| (index(&object[0]), slot)),
            |slot| {
                asked_recruitable.push(slot);
                flag(&objects[slot][1])
            },
            dword(&row["money"]),
            |class_index| {
                asked_can_build.push(class_index);
                match dword(&types[class_index][0]) {
                    0 => CanBuild::No,
                    1 => CanBuild::Yes,
                    -1 => CanBuild::AtLimit,
                    other => panic!("CanBuild answers {other}"),
                }
            },
            |class_index| dword(&types[class_index][1]),
        );
        let native_can_build: Vec<usize> = events(row, "can_build")
            .into_iter()
            .map(|asked| index(&asked[1]))
            .collect();
        let native_recruitable: Vec<usize> = events(row, "recruitable")
            .into_iter()
            .map(|asked| index(&asked[1]))
            .collect();
        assert_eq!(
            asked_recruitable, native_recruitable,
            "{context}: IsRecruitable"
        );
        assert_eq!(asked_can_build, native_can_build, "{context}: CanBuild");
        let fill = dword(&row["fill"][index(&row["difficulty"])]);
        let mut draws = Draws::new(row);
        let choice = pick(&candidates, fill, |low, high| draws.draw(low, high));
        assert_eq!(draws.asked, native_draws(row), "{context}: draws");
        assert_eq!(choice.unwrap_or(-1), dword(&row["choice"]), "{context}");
    }
}

#[test]
fn the_harvester_branch_matches_the_original() {
    let mut sim = Simulation::new();
    let name = sim.interner.intern("H0");
    for (number, row) in rows("harvester").iter().enumerate() {
        let mut house = HouseState::new(name, 0, None, flag(&row["human"]), 0, dword(&row["tech"]));
        house.set_player_control_for_test(flag(&row["control"]));
        // The fixture's harvester is vehicle 7; its refinery undeploys into
        // vehicle 9.
        let facts = HarvesterFacts {
            harvester: (flag(&row["harvester"]) && flag(&row["owned"]))
                .then(|| (7, dword(&row["harvester_tech"]))),
            refinery: flag(&row["refinery"]).then(|| flag(&row["undeploys"]).then_some(9)),
            gatherers: dword(&row["gatherers"]),
            destinations: dword(&row["destinations"]),
            harvesters_per_refinery: dword(&row["per_refinery"]),
            slave_miners: dword(&row["slave_miners"]),
            current_iq: dword(&row["iq"]),
            iq_harvester: dword(&row["iq_harvester"]),
            no_ore: flag(&row["no_ore"]),
            human: house.is_controlled_by_human(dword(&row["game_mode"]) != 0),
            tech_level: house.tech_level(),
        };
        let decision = harvester_decision(&facts);
        assert_eq!(
            decision.unwrap_or(-1),
            dword(&row["choice"]),
            "harvester row {number}"
        );
        // Past the branch, the team pass (no candidates here) draws once.
        assert_eq!(
            native_draws(row).len(),
            usize::from(decision.is_none()),
            "harvester row {number}: draws"
        );
    }
}
