//! Script42 and ILocomotion+4C field/timer results, executed by
//! tools/team_recruit_oracle.py --force-facing. Its moving answers are bounded
//! fixture inputs; movement-query parity belongs to the existing motion corpora.
use super::*;
use crate::rules::ini_parser::IniFile;
use crate::rules::locomotor_type::LocomotorKind;
use crate::rules::team_ai_ini::TeamAiDefinitionSource;
use crate::sim::components::DriveCoord;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::movement::{DriveLocomotionRuntime, ShipLocomotionRuntime};
use crate::sim::team_script_vm::TeamScriptDefinition;
use serde_json::Value;

fn kind(name: &str) -> LocomotorKind {
    match name {
        "drive" => LocomotorKind::Drive,
        "ship" => LocomotorKind::Ship,
        "walk" => LocomotorKind::Walk,
        "fly" => LocomotorKind::Fly,
        "hover" => LocomotorKind::Hover,
        "jumpjet" => LocomotorKind::Jumpjet,
        "teleport" => LocomotorKind::Teleport,
        "rocket" => LocomotorKind::Rocket,
        _ => panic!("unknown corpus locomotor {name}"),
    }
}

#[test]
fn script42_and_class_turns_match_original_controls() {
    let corpus: Value =
        serde_json::from_str(crate::test_fixture::text("tools/team_force_facing.json")).unwrap();
    for row in corpus.as_array().unwrap() {
        let rules = RuleSet::from_ini(&IniFile::empty()).unwrap();
        let mut sim = Simulation::with_seed(0x6EDCA0);
        let ids: Vec<_> = row["members"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let id = index as u64 + 1;
                let family = kind(input["family"].as_str().unwrap());
                let category = match family {
                    LocomotorKind::Walk | LocomotorKind::Jumpjet | LocomotorKind::Teleport => {
                        EntityCategory::Infantry
                    }
                    LocomotorKind::Fly | LocomotorKind::Rocket => EntityCategory::Aircraft,
                    _ => EntityCategory::Unit,
                };
                let mut entity =
                    GameEntity::test_default_of_category(id, "MEMBER", "OWNER", 10, 10, category);
                entity
                    .lifecycle
                    .set_object_alive_for_test(input["active"].as_bool().unwrap());
                entity
                    .lifecycle
                    .set_in_limbo_for_test(input["limbo"].as_bool().unwrap());
                entity
                    .health
                    .set_for_test(input["health"].as_i64().unwrap() as i32);
                entity.body_facing = serde_json::from_value(input["body"].clone()).unwrap();
                entity.barrel_facing =
                    Some(serde_json::from_value(input["secondary"].clone()).unwrap());
                let mut loco = LocomotorState::for_test_kind(family);
                if input["moving"].as_bool().unwrap() {
                    let destination = Some(DriveCoord::cell(20, 20, 0));
                    match family {
                        LocomotorKind::Drive => assert!(
                            loco.install_drive_state_for_test(Some(
                                DriveLocomotionRuntime::default()
                                    .with_destination_for_test(destination)
                            ))
                        ),
                        LocomotorKind::Ship => assert!(loco.install_ship_state_for_test(Some(
                            ShipLocomotionRuntime::default().with_destination_for_test(destination)
                        ))),
                        LocomotorKind::Walk => loco.set_walk_destination(destination),
                        _ => panic!("moving fixture not mapped for {family:?}"),
                    }
                }
                entity.locomotor = Some(loco);
                assert_eq!(
                    crate::sim::movement::motion_query::is_moving(&entity),
                    input["moving"].as_bool(),
                    "fixture {row}"
                );
                sim.substrate.entities.insert(entity);
                id
            })
            .collect();
        sim.interner = crate::sim::intern::test_interner();
        let owner = sim.interner.intern("OWNER");
        let script = sim.interner.intern("SCRIPT");
        let argument = row["argument"].as_i64().unwrap() as i32;
        sim.team_script_vm.register_script(TeamScriptDefinition {
            id: script,
            source: TeamAiDefinitionSource::Scenario,
            actions: vec![TeamScriptAction {
                action_id: 42,
                argument,
            }],
        });
        let team = sim
            .team_script_vm
            .create_team(owner, script, ids.clone(), 0);
        for (member, input) in sim
            .team_script_vm
            .teams
            .get_mut(&team)
            .unwrap()
            .members
            .iter_mut()
            .zip(row["members"].as_array().unwrap())
        {
            member.initiated = input["initiated"].as_bool().unwrap();
        }
        sim.session.binary_frame = row["frame"].as_u64().unwrap() as u32;
        let rng = [
            sim.scenario_rng.native_state_hex(),
            sim.main_rng.native_state_hex(),
            sim.mapgen_rng.native_state_hex(),
        ];
        sim.team_run_action(
            team,
            TeamScriptAction {
                action_id: 42,
                argument,
            },
            row["first"].as_bool().unwrap(),
            &rules,
            None,
            FrameEffects::default(),
        );
        assert_eq!(
            sim.team_script_vm.team(team).unwrap().advance_pending(),
            row["finished"].as_bool().unwrap(),
            "{row}"
        );
        assert_eq!(
            [
                sim.scenario_rng.native_state_hex(),
                sim.main_rng.native_state_hex(),
                sim.mapgen_rng.native_state_hex()
            ],
            rng
        );
        for (id, output) in ids.iter().zip(row["output"].as_array().unwrap()) {
            let entity = sim.substrate.entities.get(*id).unwrap();
            assert_eq!(
                serde_json::to_value(entity.body_facing).unwrap(),
                output["body"],
                "{row}"
            );
            assert_eq!(
                serde_json::to_value(entity.barrel_facing.unwrap()).unwrap(),
                output["secondary"],
                "{row}"
            );
        }
        for sample in row["samples"].as_array().unwrap() {
            let frame = sample["frame"].as_u64().unwrap() as u32;
            for (id, pair) in ids.iter().zip(sample["values"].as_array().unwrap()) {
                let entity = sim.substrate.entities.get(*id).unwrap();
                assert_eq!(
                    u64::from(entity.body_facing_current(frame)),
                    pair[0].as_u64().unwrap(),
                    "{row}"
                );
                assert_eq!(
                    u64::from(entity.barrel_facing.unwrap().current(frame)),
                    pair[1].as_u64().unwrap(),
                    "{row}"
                );
            }
        }
    }
}

#[test]
fn class_turn_dispatches_the_active_piggyback_instance() {
    let corpus: Value =
        serde_json::from_str(crate::test_fixture::text("tools/team_force_facing.json")).unwrap();
    let native = corpus
        .as_array()
        .unwrap()
        .iter()
        .find(|row| {
            row["argument"] == 2
                && row["frame"] == 100
                && row["members"].as_array().unwrap().len() == 1
                && row["members"][0]["family"] == "teleport"
        })
        .unwrap();
    let frame = native["frame"].as_u64().unwrap() as u32;
    let desired = (native["argument"].as_i64().unwrap() as u16).wrapping_shl(13);
    let mut actor = GameEntity::test_default_of_category(
        1,
        "MEMBER",
        "OWNER",
        10,
        10,
        EntityCategory::Infantry,
    );
    actor.body_facing = serde_json::from_value(native["members"][0]["body"].clone()).unwrap();
    let mut loco = LocomotorState::for_test_kind(LocomotorKind::Walk);
    assert!(loco.begin_piggyback(LocomotorKind::Teleport, frame));
    assert_eq!(loco.active_kind(), LocomotorKind::Teleport);
    actor.locomotor = Some(loco);
    assert!(crate::sim::movement::locomotor_do_turn(
        &mut actor, desired, frame
    ));
    let sample = native["samples"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sample| sample["frame"] == native["frame"])
        .unwrap();
    assert_eq!(
        u64::from(actor.body_facing_current(frame)),
        sample["values"][0][0].as_u64().unwrap(),
        "active Teleport uses Set, while suspended Walk uses SetCurrent"
    );
    assert_eq!(
        serde_json::to_value(actor.body_facing).unwrap(),
        native["output"][0]["body"]
    );
    assert_eq!(actor.body_facing.destination(), desired);
    assert!(actor.body_facing.is_rotating(frame));
}

#[test]
fn script42_keeps_a_missing_locomotor_pending() {
    let mut sim = Simulation::new();
    let mut actor = GameEntity::test_default_of_category(
        1,
        "MEMBER",
        "OWNER",
        10,
        10,
        EntityCategory::Infantry,
    );
    actor.locomotor = None;
    sim.substrate.entities.insert(actor);
    sim.interner = crate::sim::intern::test_interner();
    let owner = sim.interner.get("OWNER").unwrap();
    let script = sim.interner.intern("SCRIPT");
    let team = sim.team_script_vm.create_team(owner, script, vec![1], 0);
    sim.team_action_force_facing(team, 2);
    assert!(!sim.team_script_vm.team(team).unwrap().advance_pending());
}

#[test]
fn retail_all01_facing_scripts_reach_the_shared_turn_owner() {
    let Some((_, assets)) = crate::rules::retail_ini_fixture::retail_assets() else {
        return;
    };
    let map =
        crate::map::map_file::MapFile::from_bytes(assets.get_ref("ALL01UMD.MAP").unwrap()).unwrap();
    let registry =
        crate::rules::team_ai_ini::TeamAiIniRegistry::load_retail(&assets, &map.ini, false)
            .unwrap();
    let scripts: Vec<_> = registry
        .scripts
        .iter()
        .filter(|script| script.actions.iter().any(|action| action.action_id == 42))
        .collect();
    assert!(!scripts.is_empty(), "ALL01 registers facing scripts");
    assert!(action_is_ported(42));
    // The actual authored octants go through the production script decoder,
    // and the caller compares a raw direction word rather than type ROT.
    assert!(
        scripts
            .iter()
            .flat_map(|script| &script.actions)
            .filter(|action| action.action_id == 42)
            .all(|action| (0..=7).contains(&action.argument))
    );
}
