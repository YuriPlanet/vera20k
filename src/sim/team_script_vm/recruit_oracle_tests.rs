//! `tools/team_recruit_oracle.py`'s native rows replayed: `Recalc`,
//! `Calc_Center`'s arithmetic, `Recruit`'s search, `ObjectClass::Distance`,
//! the action 5 guard timer, action 54's seed cell, `FindOwnBuilding`,
//! action 53's seed cell, `Quarry_To_Threat` and `Coordinate_Attack`'s frame
//! test.
//!
//! Each row carries what the original's stubs answered (Can_Add, the weight
//! and cell tests, the draw); the Rust owner must ask in the same order and
//! reach the same result.

use serde_json::Value;

use super::actions::{
    attack_check_frame, gather_seed_cell, guard_frames, own_building_pick, quarry_mask,
    regroup_seed_cell,
};
use super::membership::{RecruitCandidate, entry_short, recruit_pick};
use super::team_ai::{CenterSample, center_of, center_sample};
use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::ruleset::RuleSet;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;

fn rows(section: &str) -> Vec<Value> {
    let oracle: Value =
        serde_json::from_str(crate::test_fixture::text("tools/team_recruit_oracle.json")).unwrap();
    oracle[section].as_array().unwrap().clone()
}

fn int(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn flag(value: &Value) -> bool {
    value.as_bool().unwrap()
}

fn xy(value: &Value) -> [i32; 2] {
    [int(&value[0]), int(&value[1])]
}

fn xyz(value: &Value) -> [i32; 3] {
    [int(&value[0]), int(&value[1]), int(&value[2])]
}

fn index(value: &Value) -> u64 {
    value.as_u64().unwrap()
}

fn events<'a>(row: &'a Value, kind: &str) -> Vec<&'a Value> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event[0] == kind)
        .collect()
}

/// A team of a TeamType whose TaskForce has `entries`, with `total`
/// members and the row's strength bytes.
fn recalc_fixture(row: &Value) -> (Simulation, RuleSet, u64) {
    let rules = RuleSet::from_ini(&IniFile::from_str("")).unwrap();
    let mut sim = Simulation::new();
    let owner = sim.interner.intern("OWNER");
    let script = sim.interner.intern("SCRIPT");
    let task_force = sim.interner.intern("TASKFORCE");
    let team_type = sim.interner.intern("TEAMTYPE");
    let member_type = TeamMemberTypeIdentity {
        category: ObjectCategory::Infantry,
        id: sim.interner.intern("E1"),
    };
    let vm = &mut sim.team_script_vm;
    vm.register_script(TeamScriptDefinition {
        id: script,
        source: TeamAiDefinitionSource::FixedAimd,
        actions: Vec::new(),
    });
    vm.register_task_force(TeamTaskForceDefinition {
        id: task_force,
        source: TeamAiDefinitionSource::FixedAimd,
        group: -1,
        entries: row["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|count| TeamTaskForceEntry {
                member_type,
                count: int(count),
            })
            .collect(),
    });
    vm.register_team_type(TeamTypeDefinition {
        id: team_type,
        script_id: script,
        task_force_id: task_force,
        priority: 0,
        is_base_defense: false,
        suicide: false,
        aggressive: false,
        combined_movement_zone: MovementZone::Normal,
        base_zone_relation_enforced: true,
        transport_crossing_required: false,
    });
    let metadata = vm.team_type_ini.get_mut(&team_type).unwrap();
    metadata.reinforce = flag(&row["reinforce"]);
    metadata.guard_slower = flag(&row["guard_slower"]);
    let team_id = vm.construct_team(team_type, owner, true, 0).unwrap();
    let team = vm.teams.get_mut(&team_id).unwrap();
    team.members = (1..=index(&row["total"]))
        .map(|id| TeamMember {
            id,
            initiated: false,
        })
        .collect();
    team.has_been_full = flag(&row["has_been"]);
    team.under_strength = flag(&row["under"]);
    team.full_strength = false;
    team.reforming = false;
    team.altered = true;
    team.just_altered = true;
    team.zone = Some(TeamTarget::Cell { x: 1, y: 1 });
    (sim, rules, team_id)
}

#[test]
fn recalc_matches_the_original() {
    for (number, row) in rows("recalc").iter().enumerate() {
        let (mut sim, rules, team_id) = recalc_fixture(row);
        let kept = sim.team_recalc(team_id, &rules);
        assert_eq!(
            kept,
            int(&row["result"]) == 1,
            "recalc row {number}: result"
        );
        let deleted = flag(&row["deleted"]);
        assert_eq!(
            sim.team_script_vm.team(team_id).is_none(),
            deleted,
            "recalc row {number}: destroyed"
        );
        if deleted {
            continue;
        }
        let team = sim.team_script_vm.team(team_id).unwrap();
        let native = |offset: &str| row["bytes"][offset].as_u64().unwrap() != 0;
        assert_eq!(
            [
                team.has_been_full,
                team.full_strength,
                team.under_strength,
                team.reforming,
                team.altered,
                team.just_altered,
                team.zone.is_none(),
            ],
            [
                native("0x78"),
                native("0x79"),
                native("0x7a"),
                native("0x7b"),
                native("0x7d"),
                native("0x7e"),
                flag(&row["zone_cleared"]),
            ],
            "recalc row {number}: +0x78/+0x79/+0x7A/+0x7B/+0x7D/+0x7E/+0x34"
        );
    }
}

/// A member at the row's location with the row's object state.
fn center_member(id: u64, row: &Value) -> GameEntity {
    let [x, y, _] = xyz(&row["xyz"]);
    let mut entity = GameEntity::test_default(id, "E1", "OWNER", (x >> 8) as u16, (y >> 8) as u16);
    entity.position.sub_x = SimFixed::from_num(x & 0xFF);
    entity.position.sub_y = SimFixed::from_num(y & 0xFF);
    entity.lifecycle.object_alive = flag(&row["active"]);
    entity.health.current = int(&row["health"]);
    entity.lifecycle.in_limbo = flag(&row["limbo"]);
    entity.in_playfield = flag(&row["in_playfield"]);
    entity.category = if flag(&row["aircraft"]) {
        EntityCategory::Aircraft
    } else {
        EntityCategory::Infantry
    };
    entity
}

#[test]
fn calc_center_matches_the_original() {
    for (number, row) in rows("center").iter().enumerate() {
        let guard_slower = flag(&row["guard_slower"]);
        let members = row["members"].as_array().unwrap();
        let samples: Vec<CenterSample> = members
            .iter()
            .enumerate()
            .filter_map(|(id, member)| {
                let entity = center_member(id as u64, member);
                center_sample(
                    &entity,
                    flag(&member["initiated"]),
                    guard_slower && flag(&member["weight"]),
                    int(&member["passengers"]) > 0 && flag(&member["naval"]),
                )
            })
            .collect();
        let focus = (!row["focus"].is_null()).then(|| xy(&row["focus"]));
        let cell_at = events(row, "cell_at");
        match center_of(&samples, focus) {
            None => {
                assert!(cell_at.is_empty(), "center row {number}: no mean");
                assert!(row["center"].is_null() && row["closest"].is_null());
            }
            Some((mean, closest)) => {
                assert_eq!(
                    mean,
                    xy(&cell_at[0][1]),
                    "center row {number}: the mean coordinate"
                );
                // The head stands in for a missing closest member.
                let closest = closest.unwrap_or(0);
                assert_eq!(
                    closest,
                    index(&row["closest"]),
                    "center row {number}: closest"
                );
                let refuses = int(&members[closest as usize]["can_enter"]) != 0;
                assert_eq!(
                    row["center"] == "cell",
                    !refuses,
                    "center row {number}: centre on the cell unless the closest refuses it"
                );
            }
        }
    }
}

#[test]
fn recruit_matches_the_original() {
    for (number, row) in rows("recruit").iter().enumerate() {
        let can_adds: Vec<u64> = events(row, "can_add")
            .into_iter()
            .map(|event| index(&event[1]))
            .collect();
        let added = events(row, "add").first().map(|event| index(&event[1]));
        if !entry_short(int(&row["amount"]), int(&row["count"])) {
            assert!(
                can_adds.is_empty() && added.is_none() && int(&row["result"]) == 0,
                "recruit row {number}: a full entry recruits nobody"
            );
            continue;
        }
        let unit = row["kind"] == "unit";
        let candidates: Vec<RecruitCandidate> = row["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(id, candidate)| RecruitCandidate {
                id: id as u64,
                xy: xy(&candidate["xyz"]),
                group: int(&candidate["group"]),
                matches: !unit || (flag(&candidate["own_house"]) && flag(&candidate["entry_type"])),
            })
            .collect();
        let origin = if row["zone"].is_null() {
            [0, 0]
        } else {
            xy(&row["zone"])
        };
        let mut asked = Vec::new();
        let winner = recruit_pick(
            &candidates,
            origin,
            int(&row["group"]),
            flag(&row["recruiter"]),
            |id| {
                asked.push(id);
                flag(&row["candidates"][id as usize]["can_add"])
            },
        );
        assert_eq!(asked, can_adds, "recruit row {number}: Can_Add calls");
        assert_eq!(winner, added, "recruit row {number}: winner");
        assert_eq!(
            int(&row["result"]) == 1,
            winner.is_some(),
            "recruit row {number}: result"
        );
    }
}

#[test]
fn object_distance_matches_the_original() {
    for (number, row) in rows("distance").iter().enumerate() {
        let building = row["building"].as_u64().map(|id| {
            let foundation = crate::rules::foundation::FOUNDATION_TABLE[id as usize];
            (i32::from(foundation.width), i32::from(foundation.height))
        });
        assert_eq!(
            crate::util::native_x87::object_distance(
                xyz(&row["member"]),
                xyz(&row["target"]),
                building
            ),
            int(&row["result"]),
            "distance row {number}"
        );
    }
}

#[test]
fn guard_timer_matches_the_original() {
    for row in rows("guard") {
        assert_eq!(
            guard_frames(int(&row["argument"])),
            int(&row["duration"]),
            "guard argument {}",
            row["argument"]
        );
        assert_eq!(row["start"], row["frame"], "the timer starts now");
    }
}

#[test]
fn regroup_seed_matches_the_original() {
    for (number, row) in rows("regroup").iter().enumerate() {
        let enemy = (!row["enemy"].is_null()).then(|| xy(&row["enemy"]));
        let mut drawn = 0;
        let seed = regroup_seed_cell(xy(&row["own"]), enemy, int(&row["safe_distance"]), || {
            drawn += 1;
            int(&row["draw"])
        });
        assert_eq!(
            [seed.0, seed.1],
            xy(&row["seed"]),
            "regroup row {number}: seed"
        );
        let draws = row["draws"].as_array().unwrap();
        assert_eq!(drawn, draws.len(), "regroup row {number}: draws");
        for draw in draws {
            // Scenario+0x218, RandomRanged(0, 255).
            assert_eq!(
                [int(&draw[0]), int(&draw[1]), int(&draw[2])],
                [0x218, 0, 255]
            );
        }
    }
}

#[test]
fn find_own_building_matches_the_original() {
    for (number, row) in rows("own_building").iter().enumerate() {
        let buildings: Vec<(u64, [i32; 3])> = row["buildings"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .filter(|(_, building)| flag(&building[1]))
            .map(|(id, building)| (id as u64, xyz(&building[0])))
            .collect();
        let mode = row["mode"].as_u64().unwrap() as u32;
        assert_eq!(
            own_building_pick(&buildings, xyz(&row["leader"]), mode, |_| 0),
            row["result"].as_u64(),
            "own building row {number}"
        );
    }
}

#[test]
fn gather_seed_matches_the_original() {
    for (number, row) in rows("gather").iter().enumerate() {
        if !flag(&row["has_enemy"]) {
            continue;
        }
        let leader = xyz(&row["leader"]);
        let seed = gather_seed_cell(
            xy(&row["own"]),
            xy(&row["enemy"]),
            [leader[0], leader[1]],
            int(&row["safe_distance"]),
        );
        let expected = (!row["seed"].is_null()).then(|| {
            let seed = xy(&row["seed"]);
            (seed[0], seed[1])
        });
        assert_eq!(seed, expected, "gather row {number}: seed");
        assert_eq!(
            seed.is_none(),
            flag(&row["finished"]),
            "gather row {number}"
        );
    }
}

#[test]
fn quarry_mask_matches_the_original() {
    for row in rows("quarry") {
        assert_eq!(
            i64::from(quarry_mask(int(&row["quarry"]))),
            row["mask"].as_i64().unwrap(),
            "quarry {}",
            row["quarry"]
        );
    }
}

#[test]
fn attack_check_frame_matches_the_original() {
    for row in rows("attack_cadence") {
        assert_eq!(
            attack_check_frame(int(&row["frame"])),
            flag(&row["asks"]),
            "frame {}",
            row["frame"]
        );
    }
}
