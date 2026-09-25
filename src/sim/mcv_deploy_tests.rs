use super::*;
use crate::rules::ini_parser::IniFile;
use crate::sim::command::{Command, CommandEnvelope};
use crate::sim::movement::FacingClass;
use crate::sim::world::TickResult;
use std::collections::BTreeMap;

fn fixture(kind: &str, facing: u8, rot: u8, deploy_facing: u8) -> (Simulation, RuleSet, u64) {
    fixture_with_sound(kind, facing, rot, deploy_facing, None)
}

fn fixture_with_sound(
    kind: &str,
    facing: u8,
    rot: u8,
    deploy_facing: u8,
    sound: Option<&str>,
) -> (Simulation, RuleSet, u64) {
    let sound_line = sound.map_or(String::new(), |s| format!("DeploySound={s}\n"));
    let text = format!(
        "[InfantryTypes]\n[AircraftTypes]\n[VehicleTypes]\n0={kind}\n[BuildingTypes]\n0=YARD\n[{kind}]\nStrength=1000\nSpeed=5\nROT={rot}\nLocomotor={{4A582741-9839-11d1-B709-00A024DDAFD1}}\nDeploysInto=YARD\n{sound_line}[YARD]\nStrength=1000\nConstructionYard=yes\nFoundation=4x3\nDeployFacing={deploy_facing}\n[Unload]\nRate=0.016\n"
    );
    let rules = RuleSet::from_ini(&IniFile::from_str(&text)).unwrap();
    let mut sim = Simulation::new();
    // No house AI/opponent defeat system in these command/locomotor fixtures.
    let id = sim
        .spawn_object(kind, "Americans", 20, 22, facing, &rules, &BTreeMap::new())
        .unwrap();
    (sim, rules, id)
}
fn tick(sim: &mut Simulation, rules: &RuleSet, command: Option<Command>) -> TickResult {
    let cmds: Vec<_> = command
        .into_iter()
        .map(|cmd| {
            CommandEnvelope::new(
                sim.interner.get("Americans").unwrap(),
                sim.session.tick + 1,
                cmd,
            )
        })
        .collect();
    let grid = sim.path_grid.clone();
    sim.advance_tick(
        &cmds,
        Some(rules),
        &BTreeMap::new(),
        grid.as_deref(),
        None,
        22,
    )
}
fn yards(sim: &Simulation) -> usize {
    sim.substrate
        .entities
        .values()
        .filter(|e| !e.dying && sim.interner.resolve(e.type_ref()) == "YARD")
        .count()
}
fn finish(sim: &mut Simulation, rules: &RuleSet, id: u64) -> usize {
    let mut receipts = 0;
    for _ in 0..140 {
        let out = tick(sim, rules, None);
        receipts += usize::from(out.spawned_entities);
        if sim.substrate.entities.get(id).is_none_or(|e| e.dying) {
            break;
        }
    }
    assert_eq!(
        yards(sim),
        1,
        "retained MCV: {:?}",
        sim.substrate.entities.get(id).map(|e| (
            &e.position,
            &e.navigation,
            &e.drive_locomotion,
            &e.movement_target,
            &e.body_facing,
            e.facing,
            e.facing_target,
            e.mcv_deploy_pending,
            &e.mission,
            &e.foot_speed
        ))
    );
    receipts
}
#[test]
fn one_command_turns_and_converts_all_stock_mcv_types() {
    for kind in ["AMCV", "SMCV", "PCV"] {
        for facing in [0, 32, 64, 127, 128, 192, 255] {
            let (mut sim, rules, id) = fixture(kind, facing, 5, 4);
            let accepted = tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
            assert!(!accepted.spawned_entities, "acceptance is not conversion");
            assert_eq!(yards(&sim), 0);
            assert_eq!(finish(&mut sim, &rules, id), 1, "one conversion receipt");
            let yard = sim
                .substrate
                .entities
                .values()
                .find(|e| !e.dying && e.category == EntityCategory::Structure)
                .unwrap();
            assert_eq!((yard.position.rx, yard.position.ry), (19, 21));
        }
    }
}
#[test]
fn duplicate_orders_do_not_restart_the_turn_and_zero_rot_still_completes() {
    for rot in [0, 1, 5, 10] {
        let (mut sim, rules, id) = fixture("AMCV", 64, rot, 6);
        tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
        let mut receipts = 0;
        for _ in 0..160 {
            let out = tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
            receipts += usize::from(out.spawned_entities);
            if yards(&sim) == 1 {
                break;
            }
        }
        assert_eq!((yards(&sim), receipts), (1, 1), "ROT={rot}");
    }
}
#[test]
fn turn_completion_converts_before_the_next_mission_retry() {
    let (mut sim, rules, id) = fixture("AMCV", 64, 10, 4);
    tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
    tick(&mut sim, &rules, None);
    let e = sim.substrate.entities.get(id).unwrap();
    assert!(e.mcv_deploy_pending);
    assert_eq!(e.facing, 64);
    let due = e.mission.dispatch_timer();
    for _ in 0..10 {
        let before = sim.session.binary_frame;
        let out = tick(&mut sim, &rules, None);
        if out.spawned_entities {
            assert!(!due.due(before), "conversion is the Drive edge callback");
            assert_eq!(yards(&sim), 1);
            return;
        }
    }
    panic!("turn completion never retried deployment");
}
#[test]
fn native_stop_retains_pending_deployment_and_death_cannot_spawn_a_yard() {
    let (mut sim, rules, id) = fixture("AMCV", 64, 5, 4);
    tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
    tick(&mut sim, &rules, None);
    tick(&mut sim, &rules, Some(Command::Stop { entity_id: id }));
    assert_eq!(finish(&mut sim, &rules, id), 1);
    let (mut sim, rules, id) = fixture("AMCV", 64, 5, 4);
    tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
    tick(&mut sim, &rules, None);
    sim.uninit_with_rules(id, &rules);
    for _ in 0..50 {
        assert!(!tick(&mut sim, &rules, None).spawned_entities);
    }
    assert_eq!(yards(&sim), 0);
}
#[test]
fn pending_turn_roundtrips_through_save_and_hashes_its_latches() {
    let (mut sim, rules, id) = fixture("AMCV", 0, 5, 4);
    tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
    tick(&mut sim, &rules, None);
    let hash = sim.state_hash();
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mcv_deploy_pending = false;
    assert_ne!(hash, sim.state_hash());
    sim.substrate
        .entities
        .get_mut(id)
        .unwrap()
        .mcv_deploy_pending = true;
    let data = crate::sim::snapshot::GameSnapshot::save_validated(&sim, 1, 2, "pending MCV", 0);
    let mut restored = crate::sim::snapshot::GameSnapshot::load(&data).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    // Retail's save reader deliberately reinitializes Scenario RNG. Align the
    // control run to that documented load contract before comparing continuation.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    assert_eq!(sim.state_hash(), restored.state_hash());
    for _ in 0..80 {
        let a = tick(&mut sim, &rules, None);
        let b = tick(&mut restored, &rules, None);
        assert_eq!(a.spawned_entities, b.spawned_entities);
        assert_eq!(sim.state_hash(), restored.state_hash());
    }
    assert_eq!(yards(&restored), 1);
}
#[test]
fn facing_matches_original_drive_oracle() {
    let vectors: serde_json::Value =
        serde_json::from_str(include_str!("../../tools/mcv_deploy_oracle.json")).unwrap();
    for row in vectors["turns"].as_array().unwrap() {
        let rate = row["rate"].as_u64().unwrap() as u16;
        if rate % 256 != 0 {
            continue;
        } // native sub-byte rates are outside rules ROT constructor
        let mut facing =
            FacingClass::new(row["start"].as_u64().unwrap() as u16, (rate / 256) as u8);
        let target = row["target"].as_u64().unwrap() as u16;
        facing.set(target, 100);
        if row["duplicate_at_one"].as_bool().unwrap_or(false) {
            facing.set(target, 101);
        }
        assert_eq!(
            facing.current(100 + row["elapsed"].as_u64().unwrap() as u32),
            row["current"].as_u64().unwrap() as u16,
            "{row}"
        );
    }
}

#[test]
fn moving_mcv_finishes_committed_segment_then_deploys_once() {
    let (mut sim, rules, id) = fixture("AMCV", 64, 5, 4);
    sim.path_grid = Some(std::sync::Arc::new(crate::sim::pathfinding::PathGrid::new(
        64, 64,
    )));
    tick(
        &mut sim,
        &rules,
        Some(Command::Move {
            entity_id: id,
            target_rx: 40,
            target_ry: 22,
            queue: false,
            group_id: None,
        }),
    );
    for _ in 0..8 {
        tick(&mut sim, &rules, None);
    }
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .movement_target
            .is_some()
    );
    let position = sim.substrate.entities.get(id).unwrap().position.clone();
    tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
    assert_eq!(yards(&sim), 0);
    assert_eq!(finish(&mut sim, &rules, id), 1);
    let yard = sim
        .substrate
        .entities
        .values()
        .find(|e| !e.dying && e.category == EntityCategory::Structure)
        .unwrap();
    assert!(yard.position.rx < 39, "must abandon the old destination");
    assert!(
        yard.position.rx.abs_diff(position.rx) <= 3,
        "finish the committed segment only"
    );
}
#[test]
fn placement_is_rechecked_on_turn_completion() {
    let (mut sim, rules, id) = fixture("AMCV", 64, 5, 4);
    tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
    tick(&mut sim, &rules, None);
    // A structure arrives after the initial attempt has already accepted the turn.
    let blocker = sim
        .spawn_object("YARD", "Americans", 19, 21, 0, &rules, &BTreeMap::new())
        .unwrap();
    for _ in 0..35 {
        tick(&mut sim, &rules, None);
    }
    let mcv = sim
        .substrate
        .entities
        .get(id)
        .expect("blocked MCV survives");
    assert!(!mcv.dying && !mcv.mcv_deploy_pending);
    assert_eq!(yards(&sim), 1, "only the blocking building exists");
    sim.uninit_with_rules(blocker, &rules);
    for _ in 0..35 {
        assert!(!tick(&mut sim, &rules, None).spawned_entities);
    }
    assert_eq!(yards(&sim), 0, "failed deploy must not keep retrying");
}
#[test]
fn move_order_during_turn_prevents_conversion_at_the_old_site() {
    let (mut sim, rules, id) = fixture("AMCV", 64, 5, 4);
    sim.path_grid = Some(std::sync::Arc::new(crate::sim::pathfinding::PathGrid::new(
        64, 64,
    )));
    tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
    tick(&mut sim, &rules, None);
    tick(
        &mut sim,
        &rules,
        Some(Command::Move {
            entity_id: id,
            target_rx: 40,
            target_ry: 22,
            queue: false,
            group_id: None,
        }),
    );
    for _ in 0..10 {
        assert!(!tick(&mut sim, &rules, None).spawned_entities);
    }
    assert_eq!(yards(&sim), 0);
    assert!(!sim.substrate.entities.get(id).unwrap().dying);
}

#[test]
fn continuation_result_and_rotation_edge_match_original_blocks() {
    let v: serde_json::Value =
        serde_json::from_str(include_str!("../../tools/mcv_deploy_oracle.json")).unwrap();
    for row in v["mission_branches"].as_array().unwrap() {
        let kind = row["kind"].as_str().unwrap();
        if kind == "state0" {
            continue;
        } // state0 includes the separate path reset/radio handoff
        let (mut sim, _rules, id) = fixture("AMCV", 64, 5, 4);
        let e = sim.substrate.entities.get_mut(id).unwrap();
        e.dying = row["alive"].as_u64().unwrap() == 0;
        e.mcv_deploy_pending = row["flag"].as_u64().unwrap() != 0;
        e.navigation.nav_com = (row["nav"].as_u64().unwrap() != 0)
            .then_some(crate::sim::components::NavTargetRef::cell(30, 30));
        e.mission
            .set_handler_state(if kind == "initial" { 1 } else { 2 });
        if kind == "initial" {
            finish_initial_attempt(&mut sim, id);
        } else {
            finish_retry(&mut sim, id, row["result"].as_u64().unwrap() != 0);
        }
        let e = sim.substrate.entities.get(id).unwrap();
        assert_eq!(
            e.mission.handler_state(),
            row["out_state"].as_u64().unwrap() as u32,
            "{row}"
        );
        assert_eq!(
            e.mcv_deploy_pending,
            row["out_flag"].as_u64().unwrap() != 0,
            "{row}"
        );
    }
}

#[test]
fn conversion_receipt_rebuilds_navigation_in_the_conversion_frame() {
    let (mut sim, rules, id) = fixture("AMCV", 64, 5, 4);
    sim.resolved_terrain = Some(crate::sim::deploy_tests::mcv_deploy_terrain_with(|_| {}));
    assert!(sim.rebuild_dynamic_navigation(&rules));
    assert!(sim.path_grid.as_ref().unwrap().is_walkable(19, 21));
    let accepted = tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
    assert!(!accepted.spawned_entities);
    assert!(sim.path_grid.as_ref().unwrap().is_walkable(19, 21));
    for _ in 0..50 {
        let result = tick(&mut sim, &rules, None);
        if result.spawned_entities {
            assert_eq!(yards(&sim), 1);
            assert!(!sim.path_grid.as_ref().unwrap().is_walkable(19, 21));
            return;
        }
        assert!(sim.path_grid.as_ref().unwrap().is_walkable(19, 21));
    }
    panic!("missing conversion frame");
}
#[test]
fn retail_mcv_and_target_rules_deploy_with_one_command() {
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let mut rules = RuleSet::from_ini(&rules_ini).unwrap();
    let art = crate::rules::art_data::ArtRegistry::from_ini(&art_ini);
    rules.merge_art_data(&art);
    for kind in ["AMCV", "SMCV", "PCV"] {
        for facing in [0, 64, 128, 192] {
            let mut sim = Simulation::new();
            let id = sim
                .spawn_object(kind, "Americans", 20, 22, facing, &rules, &BTreeMap::new())
                .unwrap();
            tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
            let mut converted = false;
            for _ in 0..140 {
                let result = tick(&mut sim, &rules, None);
                if result.spawned_entities {
                    converted = true;
                    break;
                }
            }
            assert!(converted, "{kind}, facing={facing}");
            let target = rules.object(kind).unwrap().deploys_into.as_deref().unwrap();
            assert_eq!(
                sim.substrate
                    .entities
                    .values()
                    .filter(|e| !e.dying && sim.interner.resolve(e.type_ref()) == target)
                    .count(),
                1
            );
        }
    }
}

#[test]
fn active_track_and_same_cell_destination_preserve_the_rotation_latch() {
    let (mut sim, rules, id) = fixture("AMCV", 128, 5, 4);
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.mcv_deploy_pending = true;
    let drive = e.drive_locomotion.get_or_insert_with(Default::default);
    drive.turn_latched = true;
    drive.track.turn_index = 3;
    drive.track_valid = true;
    assert!(
        drive.head_to.is_none(),
        "the native +63/selector gate is independent of Head_To"
    );
    sim.process_ground_locomotor_for_test(id, Some(&rules), None, None)
        .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .drive_locomotion
            .as_ref()
            .unwrap()
            .turn_latched
    );
    assert_eq!(yards(&sim), 0);
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.drive_locomotion.as_mut().unwrap().track_valid = false;
    e.navigation.nav_com = Some(crate::sim::components::NavTargetRef::cell(20, 22));
    sim.process_ground_locomotor_for_test(id, Some(&rules), None, None)
        .unwrap();
    assert!(
        sim.substrate
            .entities
            .get(id)
            .unwrap()
            .drive_locomotion
            .as_ref()
            .unwrap()
            .turn_latched
    );
    assert_eq!(yards(&sim), 0);
    let e = sim.substrate.entities.get_mut(id).unwrap();
    e.navigation.nav_com = None;
    // A retained selector alone cannot override the cleared +63 authority.
    assert_eq!(e.drive_locomotion.as_ref().unwrap().track.turn_index, 3);
    assert!(!e.drive_locomotion.as_ref().unwrap().track_valid);
    sim.process_ground_locomotor_for_test(id, Some(&rules), None, None)
        .unwrap();
    assert_eq!(yards(&sim), 1);
}

#[test]
fn mcv_deploy_sound_occurs_once_with_conversion_at_the_source_cell() {
    use crate::sim::world::SimSoundEvent;
    for facing in [64, 128] {
        let (mut sim, rules, id) = fixture_with_sound("AMCV", facing, 5, 4, Some("PlaceBuilding"));
        tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
        let mut sounds = 0;
        for _ in 0..50 {
            // Repeated D while waiting must not repeat the conversion cue.
            let command = (yards(&sim) == 0).then_some(Command::DeployMcv { entity_id: id });
            sim.sound_events.clear();
            let result = tick(&mut sim, &rules, command);
            let cues: Vec<_> = sim
                .sound_events
                .iter()
                .filter_map(|event| match event {
                    SimSoundEvent::EntityDeployed {
                        deploy_sound_id,
                        rx,
                        ry,
                    } => Some((sim.interner.resolve(*deploy_sound_id), *rx, *ry)),
                    _ => None,
                })
                .collect();
            assert_eq!(cues.len(), usize::from(result.spawned_entities));
            if !cues.is_empty() {
                assert_eq!(cues, vec![("PlaceBuilding", 20, 22)]);
            }
            sounds += cues.len();
        }
        assert_eq!(sounds, 1);
    }
}

#[test]
fn blocked_or_unconfigured_mcv_deploy_does_not_emit_deploy_sound() {
    use crate::sim::world::SimSoundEvent;
    for blocked in [false, true] {
        let (mut sim, rules, id) =
            fixture_with_sound("AMCV", 64, 5, 4, blocked.then_some("PlaceBuilding"));
        tick(&mut sim, &rules, Some(Command::DeployMcv { entity_id: id }));
        tick(&mut sim, &rules, None);
        if blocked {
            sim.spawn_object("YARD", "Americans", 19, 21, 0, &rules, &BTreeMap::new())
                .unwrap();
        }
        for _ in 0..50 {
            tick(&mut sim, &rules, None);
            assert!(
                !sim.sound_events
                    .iter()
                    .any(|e| matches!(e, SimSoundEvent::EntityDeployed { .. }))
            );
        }
        if !blocked {
            assert_eq!(
                yards(&sim),
                1,
                "an unconfigured cue must not prevent conversion"
            );
            assert!(sim.substrate.entities.get(id).is_none_or(|e| e.dying));
        }
    }
}
