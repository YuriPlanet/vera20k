//! Original E1 class setter controls for the native response/Rescue consumer.
//! Reciprocal+2A8 rows and unconsumed+68E/timer auxiliary words are excluded;
//! this compares the represented ordinary class receiver, not the whole setter.

use crate::map::entities::EntityCategory;
use crate::rules::ini_parser::IniFile;
use crate::rules::native_processing::{RulesLayerKind, RulesLayerStack};
use crate::rules::ruleset::RuleSet;
use crate::sim::combat::{AttackTarget, TargetKind};
use crate::sim::components::Health;
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::{MissionId, MissionType};
use crate::sim::passenger::{PassengerCargo, PassengerRole};
use crate::sim::rng::SimRng;
use crate::sim::world::Simulation;
use serde_json::Value;

fn corpus() -> Value {
    let metadata: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/base_defense_response.meta.json",
    ))
    .unwrap();
    assert_eq!(metadata["schema_version"], 1);
    assert_eq!(
        metadata["native_sha256"],
        "1cdd1180e49024fbda8ad568caac2e86e856063ff67ab38f62b7d2c7bb84298c"
    );
    serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/base_defense_response.json",
    ))
    .unwrap()
}

fn target(label: &str) -> Option<TargetKind> {
    match label {
        "null" => None,
        "new_target" => Some(TargetKind::Entity(2)),
        "old_target" => Some(TargetKind::Entity(3)),
        _ => panic!("undeclared native target {label}"),
    }
}

#[test]
fn native_infantry_target_assignment_matches_58_ordinary_original_rows() {
    compare_original_infantry_target_rows(TargetConsumer::ClassSetter);
}

#[test]
fn ordered_attack_uses_the_class_setter_for_46_original_infantry_rows() {
    // Event4C7467 calls virtual+3C8 before its destination setter. Replay
    // eligible positive target rows through the production command consumer;
    // the class outputs remain the original51B1F0 execution, not Rust goldens.
    compare_original_infantry_target_rows(TargetConsumer::OrderedAttack);
}

#[test]
fn reset_orders_to_guard_uses_class_setter_for_native_null_rows() {
    // Original70F865 dispatches virtual+3C8 after the class destination clear.
    // These are original51B1F0 outputs, not full ResetOrdersToGuard execution.
    compare_original_infantry_target_rows(TargetConsumer::ResetGuard);
}

#[test]
fn open_topped_passenger_target_clear_uses_class_setter_for_native_null_rows() {
    // Original710550 walks the cargo head and calls each virtual+3C8 at71056C.
    // The saved class receiver rows bound this caller comparison; native full
    // boarding/cargo mutation and contained firing are separate mechanisms.
    compare_original_infantry_target_rows(TargetConsumer::OpenToppedPassenger);
}

#[test]
fn owner_change_uses_class_setter_for_native_null_rows() {
    // Original7014DB/7014E9 run target/destination before the house swap,
    // and70183B dispatches target NULL again on the new owner. The expected
    // class outputs are executed native rows55/56. Full original ChangeOwner
    // lifecycle/mission execution remains unproved by this corpus.
    compare_original_infantry_target_rows(TargetConsumer::OwnerChange);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TargetConsumer {
    ClassSetter,
    OrderedAttack,
    ResetGuard,
    OpenToppedPassenger,
    OwnerChange,
}

fn compare_original_infantry_target_rows(consumer: TargetConsumer) {
    let Some(ini) = crate::rules::retail_ini_fixture::retail_ini("rulesmd.ini") else {
        return;
    };
    let Some(art) = crate::rules::retail_ini_fixture::retail_ini("artmd.ini") else {
        return;
    };
    let registry = crate::rules::infantry_sequence::parse_infantry_sequence_registry(&art);
    let rules: std::collections::BTreeMap<_, _> = ["yes", "no"]
        .into_iter()
        .map(|raw| {
            let mut layers = RulesLayerStack::new(ini.clone());
            layers.push(
                RulesLayerKind::Scenario,
                IniFile::from_str(&format!("[E1]\nDeployFire={raw}\n")),
            );
            let mut rules =
                RuleSet::from_processed_rules(&layers.process_with_fixed_art(&art).unwrap())
                    .unwrap();
            rules.install_art_data(crate::rules::art_data::ArtRegistry::from_ini(&art));
            rules.bind_animation_sequences(&registry);
            (raw, rules)
        })
        .collect();
    let native = corpus();
    let evidence = &native["infantry_assignment"];
    assert_eq!(evidence["rows"].as_array().unwrap().len(), 61);
    let mut compared = 0;
    for row in evidence["rows"].as_array().unwrap() {
        let before = &row["before"];
        if before["pair2a8"] != "null" {
            continue;
        }
        let input = &row["input"];
        if consumer == TargetConsumer::OrderedAttack
            && (input["health"].as_i64().unwrap() <= 0
                || !matches!(
                    input["target"].as_str().unwrap(),
                    "assign" | "replace" | "same"
                ))
        {
            // A command refuses an inactive source before the class setter;
            // null-target command ordering has a separate event path.
            continue;
        }
        let name = input["name"].as_str().unwrap();
        if matches!(
            consumer,
            TargetConsumer::ResetGuard
                | TargetConsumer::OpenToppedPassenger
                | TargetConsumer::OwnerChange
        ) && !matches!(name, "target_clear" | "target_same_null")
        {
            continue;
        }
        let rules = &rules[input["deploy_fire_raw"].as_str().unwrap()];
        let mut sim = Simulation::new();
        let house = sim.interner.intern("Receiver");
        let mut actor = GameEntity::new_at_frame_zero_for_test(
            1,
            10,
            10,
            0,
            0,
            house,
            Health {
                current: input["health"].as_i64().unwrap() as i32,
            },
            sim.interner.intern("E1"),
            EntityCategory::Infantry,
            0,
            0,
            false,
        );
        actor.lifecycle.in_limbo = false;
        let doing = before["doing"].as_i64().unwrap() as i32;
        actor
            .mission_leaf
            .set_infantry_doing_verified(doing)
            .unwrap();
        actor
            .mission_leaf
            .set_foot_firing_sequence(before["firing_latch"].as_u64().unwrap() as u8);
        actor.infantry.as_mut().unwrap().is_prone = before["prone"].as_u64().unwrap() != 0;
        actor.set_falling_down_for_test(input["falling"].as_u64().unwrap() != 0);
        actor.passively_acquired_target = before["passive"].as_u64().unwrap() != 0;
        actor.attack_target =
            target(before["target"].as_str().unwrap()).map(|target| match target {
                TargetKind::Entity(id) => AttackTarget::new(id),
                _ => unreachable!(),
            });
        actor.navigation.path_replay.directions =
            vec![before["path_field"].as_i64().unwrap() as u8, 2, 3];
        // Original51B1F0 observes the single signed +F8 and retained stage
        // timer/rate. +104 is copied stack data; FC/increment are unobserved
        // by this corpus and do not participate in the class setter.
        actor.install_native_stage_fixture(crate::sim::stage::StageClass::from_native_fixture(
            before["frame"].as_i64().unwrap() as i32,
            0,
            crate::sim::timer::CdTimer::from_raw(
                before["action_timer"][0].as_i64().unwrap() as i32,
                before["action_timer"][2].as_i64().unwrap() as i32,
            ),
            before["action_repeat"].as_i64().unwrap() as i32,
            1,
        ));
        sim.substrate.entities.insert(actor);
        for id in [2, 3] {
            let mut recipient = GameEntity::new_at_frame_zero_for_test(
                id,
                11,
                10,
                0,
                0,
                house,
                Health { current: 100 },
                sim.interner.intern("MTNK"),
                EntityCategory::Unit,
                0,
                0,
                true,
            );
            if consumer == TargetConsumer::OrderedAttack {
                // The event admits only out-of-limbo object tokens. This
                // additional caller premise does not change the represented
                // class setter's alive-target results.
                recipient.lifecycle.in_limbo = false;
            }
            sim.substrate.entities.insert(recipient);
        }
        let requested = match input["target"].as_str().unwrap() {
            "assign" | "replace" | "same" => Some(TargetKind::Entity(2)),
            "clear" | "same_null" => None,
            other => panic!("undeclared setter input {other}"),
        };
        sim.session.binary_frame = input["frame"].as_u64().unwrap() as u32;
        sim.scenario_rng = SimRng::new(input["seed"].as_u64().unwrap());
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_before"],
            "{name}"
        );
        let main_before = sim.main_rng.logical_state();
        let mapgen_before = sim.mapgen_rng.logical_state();
        if consumer == TargetConsumer::OrderedAttack {
            assert!(sim.order_actor_admits(1), "{name}: actor admission");
            assert!(sim.order_object_token_admits(2), "{name}: target admission");
            assert!(
                sim.apply_command(
                    "Receiver",
                    &crate::sim::command::Command::ForceAttack {
                        attacker_id: 1,
                        target_id: 2,
                    },
                    Some(rules),
                ),
                "{name}"
            );
        } else {
            match consumer {
                TargetConsumer::ClassSetter => {
                    sim.assign_target_represented(1, requested, Some(rules))
                        .unwrap();
                }
                TargetConsumer::ResetGuard => {
                    sim.substrate
                        .entities
                        .get_mut(1)
                        .unwrap()
                        .set_archive_target(Some(TargetKind::Entity(3)));
                    sim.mission_assign_exact(
                        1,
                        MissionId::from_known(MissionType::Attack),
                        sim.session.binary_frame,
                    )
                    .unwrap();
                    sim.reset_orders_to_guard(1, rules);
                    let actor = sim.substrate.entities.get(1).unwrap();
                    assert_eq!(actor.archive_target(), None, "{name}: Guard archive");
                    assert_eq!(
                        actor.mission.current().known(),
                        Some(MissionType::Guard),
                        "{name}: native70F87B Guard assignment",
                    );
                }
                TargetConsumer::OpenToppedPassenger => {
                    let object = rules.object("BFRT").unwrap();
                    assert!(object.open_topped);
                    let mut cargo =
                        PassengerCargo::new(object.passengers.max(0) as u32, object.size_limit);
                    assert!(cargo.board(1, rules.object("E1").unwrap().size));
                    let mut transport = GameEntity::new_at_frame_zero_for_test(
                        4,
                        10,
                        10,
                        0,
                        0,
                        house,
                        Health {
                            current: object.strength,
                        },
                        sim.interner.intern("BFRT"),
                        EntityCategory::Unit,
                        0,
                        0,
                        true,
                    );
                    transport.passenger_role = PassengerRole::Transport { cargo };
                    sim.substrate.entities.insert(transport);
                    sim.open_topped_passengers_take_target(4, requested, rules);
                }
                TargetConsumer::OwnerChange => {
                    let new_owner = sim.interner.intern("NewOwner");
                    for owner in [house, new_owner] {
                        sim.houses
                            .insert(owner, HouseState::new(owner, 0, None, true, 0, 10));
                    }
                    sim.change_owner_with_rules(1, new_owner, rules, None);
                    assert_eq!(
                        sim.substrate.entities.get(1).unwrap().owner(),
                        new_owner,
                        "{name}: Rust caller ownership transfer",
                    );
                }
                TargetConsumer::OrderedAttack => unreachable!(),
            }
        }

        let after = &row["after"];
        let actor = sim.substrate.entities.get(1).unwrap();
        let leaf = actor.mission_leaf.as_infantry().unwrap();
        assert_eq!(
            leaf.doing(),
            after["doing"].as_i64().unwrap() as i32,
            "{name}"
        );
        assert_eq!(
            leaf.firing_sequence_latch(),
            after["firing_latch"].as_u64().unwrap() as u8,
            "{name}",
        );
        assert_eq!(
            actor.attack_target.as_ref().map(|attack| attack.target),
            target(after["target"].as_str().unwrap()),
            "{name}",
        );
        assert_eq!(
            actor.passively_acquired_target,
            after["passive"].as_u64().unwrap() != 0,
            "{name}",
        );
        assert_eq!(
            actor.infantry.as_ref().unwrap().is_prone,
            after["prone"].as_u64().unwrap() != 0,
            "{name}",
        );
        assert_eq!(
            actor.native_stage().value(),
            after["frame"].as_i64().unwrap() as i32,
            "{name}",
        );
        assert_eq!(
            [
                actor.native_stage().timer().start_frame(),
                actor.native_stage().timer().duration(),
                actor.native_stage().rate(),
            ],
            [
                after["action_timer"][0].as_i64().unwrap() as i32,
                after["action_timer"][2].as_i64().unwrap() as i32,
                after["action_repeat"].as_i64().unwrap() as i32,
            ],
            "{name}: native stage timer/rate",
        );
        let head = actor
            .navigation
            .path_replay
            .remaining_directions()
            .first()
            .copied();
        let expected_head = (after["path_field"].as_i64().unwrap() != -1)
            .then(|| after["path_field"].as_u64().unwrap() as u8);
        // This receipt ends at Assign_Target51B1F0. An ordered event then
        // calls the class NULL destination, which can separately clear Path[0]
        // even when Assign_Target refused (AI Doing27..30,51AD11). Its path
        // writes/refusal are compared by walk_percell_stop; the target-only
        // intermediate head is not a golden for the whole event's suffix.
        if consumer != TargetConsumer::OrderedAttack {
            assert_eq!(head, expected_head, "{name}");
        }
        assert_eq!(
            sim.scenario_rng.native_state_hex(),
            row["rng_after"],
            "{name}"
        );
        assert_eq!(sim.main_rng.logical_state(), main_before, "{name}: Main");
        assert_eq!(
            sim.mapgen_rng.logical_state(),
            mapgen_before,
            "{name}: MapGen"
        );
        compared += 1;
    }
    assert_eq!(
        compared,
        match consumer {
            TargetConsumer::ClassSetter => 58,
            TargetConsumer::OrderedAttack => 46,
            TargetConsumer::ResetGuard
            | TargetConsumer::OpenToppedPassenger
            | TargetConsumer::OwnerChange => 2,
        }
    );
}
