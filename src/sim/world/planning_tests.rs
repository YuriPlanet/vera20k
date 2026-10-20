//! Production planning-state regression checks against the bounded native corpus.

use super::PlanningPath;
use crate::rules::ruleset::RuleSet;
use crate::sim::command::{Command, CommandRecord};
use crate::sim::intern::InternedId;
use crate::sim::world::Simulation;
use crate::util::save_codec;
use serde_json::Value;

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/planning_mode.json")
}

fn path(pending: i32, loop_start: Option<usize>) -> PlanningPath {
    PlanningPath {
        nodes: vec![0, 1, 2],
        pending,
        loop_start,
        loop_changed: loop_start.is_some(),
        executing: false,
        current: None,
    }
}

#[test]
fn native_commit_rows_reset_pending_and_loop_edit() {
    for row in corpus()["commits"].as_array().unwrap() {
        let mut actual = path(
            row["pending"].as_i64().unwrap() as i32,
            (row["loop_start"].as_i64().unwrap() >= 0)
                .then(|| row["loop_start"].as_u64().unwrap() as usize),
        );
        actual.loop_changed = row["loop_changed"].as_bool().unwrap();
        actual.commit();
        assert_eq!(
            actual.pending,
            row["after_pending"].as_i64().unwrap() as i32
        );
        assert_eq!(
            actual.loop_changed,
            row["after_loop_changed"].as_i64().unwrap() != 0
        );
    }
}

#[test]
fn native_lookup_rows_match_remaining_and_loop_indexing() {
    for row in corpus()["lookups"].as_array().unwrap() {
        let actual = path(
            -1,
            (row["loop_start"].as_i64().unwrap() >= 0)
                .then(|| row["loop_start"].as_u64().unwrap() as usize),
        );
        assert_eq!(
            actual.node_at(row["index"].as_i64().unwrap() as i32),
            row["node"].as_i64().map(|value| value as u64),
            "lookup row {row:?}"
        );
    }
}

#[test]
fn native_loop_consumption_rotates_head_and_updates_pending() {
    for row in corpus()["loop_consumes"].as_array().unwrap() {
        let mut actual = path(row["pending"].as_i64().unwrap() as i32, Some(0));
        assert_eq!(actual.consume(), Some(0));
        let expected: Vec<_> = row["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_u64().unwrap())
            .collect();
        assert_eq!(actual.nodes, expected);
        assert_eq!(
            actual.loop_start,
            Some(row["loop_start"].as_u64().unwrap() as usize)
        );
        assert_eq!(
            actual.pending,
            row["after_pending"].as_i64().unwrap() as i32
        );
    }
}

#[test]
fn edit_append_commit_preserves_missions_and_shares_same_frame_nodes() {
    let (mut sim, rules, owner) = fixture();
    let before = sim.substrate.entities.get(1).unwrap().mission;
    append(&mut sim, &rules, owner, 1, (12, 15));
    append(&mut sim, &rules, owner, 2, (12, 15));
    assert_eq!(sim.planning.nodes.len(), 1);
    assert_eq!(
        sim.planning.nodes[&0]
            .orders
            .iter()
            .map(|(id, _)| *id)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(sim.substrate.entities.get(1).unwrap().mission, before);
    assert!(sim.planning_paths(owner).iter().all(|view| view.editing));
    let commit = sim.encode_plan_commit_record(owner).unwrap();
    apply(&mut sim, &rules, commit);
    assert!(sim.planning_paths(owner).iter().all(|view| !view.editing));
    sim.session
        .set_binary_frame_for_test(sim.session.binary_frame() + 1);
    append(&mut sim, &rules, owner, 1, (16, 15));
    assert_eq!(sim.planning.paths[&1].pending, 1);
    assert_eq!(sim.planning_paths(owner)[0].cells, vec![(12, 15), (16, 15)]);
}

fn fixture() -> (Simulation, RuleSet, InternedId) {
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\nSpeed=4\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\nMovementZone=Normal\n",
    )).unwrap();
    let mut sim = Simulation::new();
    let grid = crate::sim::pathfinding::PathGrid::new(40, 40);
    sim.install_fixture_path_grid(Some(&grid));
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    sim.session.set_current_house_for_test(Some(owner));
    for actor in 1..=3 {
        assert_eq!(sim.allocate_stable_id(), actor);
        crate::sim::world::lifecycle_tests::insert_entity(
            &mut sim,
            actor,
            crate::map::entities::EntityCategory::Unit,
        );
        let entity = sim.substrate.entities.get_mut(actor).unwrap();
        entity.lifecycle.set_in_limbo_for_test(false);
        entity.in_logic_vector.set_for_test(true);
        entity.locomotor = Some(
            crate::sim::movement::locomotor::LocomotorState::from_object_type(
                rules.object("TEST").unwrap(),
                0,
            ),
        );
    }
    sim.substrate.logic.set_order_for_test(vec![1, 2, 3]);
    (sim, rules, owner)
}

fn apply(sim: &mut Simulation, rules: &RuleSet, record: CommandRecord) {
    let envelope = sim
        .decode_native_command_record(&record, sim.session.tick())
        .unwrap();
    assert!(sim.apply_command("Americans", &envelope.payload, Some(rules)));
}

fn append(sim: &mut Simulation, rules: &RuleSet, owner: InternedId, actor: u64, cell: (u16, u16)) {
    let record = sim
        .encode_planning_order_record(
            owner,
            &Command::Move {
                entity_id: actor,
                target_rx: cell.0,
                target_ry: cell.1,
            },
        )
        .unwrap();
    apply(sim, rules, record);
}

#[test]
fn stop_or_uninit_detaches_only_its_actor_from_shared_nodes() {
    for uninit in [false, true] {
        let (mut sim, rules, owner) = fixture();
        for actor in [1, 2] {
            append(&mut sim, &rules, owner, actor, (12, 15));
        }
        if uninit {
            sim.uninit(1);
        } else {
            assert!(sim.apply_command("Americans", &Command::Stop { entity_id: 1 }, Some(&rules)));
        }
        assert!(!sim.planning.paths.contains_key(&1), "uninit={uninit}");
        assert!(sim.planning.paths.contains_key(&2), "uninit={uninit}");
        assert_eq!(
            sim.planning.nodes[&0]
                .orders
                .iter()
                .map(|(id, _)| *id)
                .collect::<Vec<_>>(),
            vec![2],
            "uninit={uninit}"
        );
    }
}

fn special_ordinary_retask_cancels_shared_route(kind: &str) {
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules(
        "[VehicleTypes]\n0=PLANNER\n1=CARRIER\n[PLANNER]\nStrength=100\nSpeed=6\nSpeedType=Track\nMovementZone=Normal\nHarvester=yes\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n[CARRIER]\nStrength=200\nSpeed=6\nSpeedType=Track\nPassengers=5\nSizeLimit=10\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n[BuildingTypes]\n1=DEPOT\n[DEPOT]\nStrength=800\nUnitRepair=yes\n",
    );
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    sim.session.set_current_house_for_test(Some(owner));
    let actor = sim
        .spawn_object("PLANNER", "Americans", 15, 15, 0, &rules)
        .unwrap();
    let survivor = sim
        .spawn_object("PLANNER", "Americans", 16, 15, 0, &rules)
        .unwrap();
    let target = sim
        .spawn_object(
            if kind == "repair" { "DEPOT" } else { "CARRIER" },
            "Americans",
            18,
            15,
            0,
            &rules,
        )
        .unwrap();
    sim.substrate
        .entities
        .get_mut(actor)
        .unwrap()
        .health
        .set_for_test(50);
    for id in [actor, survivor] {
        append(&mut sim, &rules, owner, id, (17, 16));
        append(&mut sim, &rules, owner, id, (17, 17));
    }
    // A committed route waiting for idle must not resume after the new order.
    // Commit only its existing state here, without publishing an in-flight leg.
    for path in sim.planning.paths.values_mut() {
        path.commit();
    }
    let command = match kind {
        "harvest" => Command::HarvestCell {
            entity_id: actor,
            target_rx: 18,
            target_ry: 16,
        },
        "repair" => Command::RepairAtDepot {
            entity_id: actor,
            depot_id: target,
        },
        "board" => Command::EnterTransport {
            passenger_id: actor,
            transport_id: target,
        },
        _ => unreachable!(),
    };
    assert!(
        sim.apply_command_with_overlays(
            "Americans",
            &command,
            Some(&rules),
            Some(&registry),
            crate::sim::world::FrameEffects::default()
        ),
        "accepted {kind} order"
    );
    assert!(
        !sim.planning.paths.contains_key(&actor),
        "{kind} must detach the old route"
    );
    assert_eq!(sim.planning.paths[&survivor].nodes.len(), 2);
    assert_eq!(sim.planning.nodes.len(), 2);
    for node in sim.planning.nodes.values() {
        assert_eq!(
            node.orders.keys().copied().collect::<Vec<_>>(),
            vec![survivor]
        );
    }
    assert!(
        !sim.planning_enter_idle(actor),
        "{kind} must not restart an old leg"
    );
    assert!(sim.pending_command_snapshot().is_empty());
}

#[test]
fn ordinary_harvest_repair_and_boarding_cancel_shared_planning_route() {
    for kind in ["harvest", "repair", "board"] {
        special_ordinary_retask_cancels_shared_route(kind);
    }
}

#[test]
fn native_commit_admission_rows_match_production_mission_and_target_query() {
    let rows = corpus();
    assert_eq!(rows["commit_admissions"].as_array().unwrap().len(), 15);
    for row in rows["commit_admissions"].as_array().unwrap() {
        let (mut sim, rules, owner) = fixture();
        append(&mut sim, &rules, owner, 1, (12, 15));
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity.mission.set_current_for_test(
            crate::sim::mission::MissionId::from_raw(row["mission"].as_i64().unwrap() as i32),
            0,
        );
        entity
            .attack_target
            .set_for_test(match row["target_kind"].as_str().unwrap() {
                "none" => None,
                "cell" => Some(crate::sim::combat::TargetKind::Cell(10, 12)),
                "object" => Some(crate::sim::combat::TargetKind::Entity(2)),
                other => panic!("unknown native target {other}"),
            });
        let commit = sim.encode_plan_commit_record(owner).unwrap();
        apply(&mut sim, &rules, commit);
        let admit = row["admit"].as_bool().unwrap();
        assert_eq!(sim.planning.paths[&1].executing, admit, "{row:?}");
        assert_eq!(
            sim.planning.paths[&1].nodes.len(),
            usize::from(!admit),
            "{row:?}"
        );
        assert_eq!(sim.planning.paths[&1].pending, -1);
    }
}

#[test]
fn native_dispatch_latches_match_production_flags_and_keep_path_shape() {
    for row in corpus()["dispatch_latches"].as_array().unwrap() {
        // Flag1's original sample executes only DispatchLatch6386E0, before
        // append638120. Production flag1 includes append and changes count.
        if row["flag"] != 2 {
            continue;
        }
        let (mut sim, rules, owner) = fixture();
        for cell in [(12, 15), (16, 15), (20, 15)] {
            append(&mut sim, &rules, owner, 1, cell);
        }
        let record = sim.planning.nodes[&0].order(1).unwrap().clone();
        let path = sim.planning.paths.get_mut(&1).unwrap();
        path.pending = 1;
        path.loop_start = Some(0);
        path.executing = true;
        let mut dispatch = record;
        dispatch.payload_mut()[22] = row["flag"].as_u64().unwrap() as u8;
        apply(&mut sim, &rules, dispatch);
        let path = &sim.planning.paths[&1];
        assert_eq!(
            path.executing,
            row["executing"].as_bool().unwrap(),
            "{row:?}"
        );
        assert_eq!(path.nodes.len(), row["count"].as_u64().unwrap() as usize);
        assert_eq!(path.pending, row["pending"].as_i64().unwrap() as i32);
        assert_eq!(
            path.loop_start,
            Some(row["loop_start"].as_u64().unwrap() as usize)
        );
    }
}

#[test]
fn guard_commit_consumes_first_node_stages_deliberate_and_retains_next_node() {
    let (mut sim, rules, owner) = fixture();
    append(&mut sim, &rules, owner, 1, (12, 15));
    append(&mut sim, &rules, owner, 1, (16, 15));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mission
        .set_current_for_test(
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
            0,
        );
    let commit = sim.encode_plan_commit_record(owner).unwrap();
    apply(&mut sim, &rules, commit);
    let path = &sim.planning.paths[&1];
    assert!(path.executing);
    assert_eq!(path.nodes.len(), 1);
    assert_eq!(path.pending, -1);
    assert_eq!(path.current.as_ref().unwrap().payload()[22], 2);
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .mission
            .queued()
            .known(),
        Some(crate::sim::mission::MissionType::Deliberate)
    );
    assert!(sim.take_due_commands().is_empty());
    sim.session.set_tick_for_test(sim.session.tick() + 1);
    let due = sim.take_due_commands();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].execute_tick, sim.session.tick() + 1);
    assert!(matches!(&due[0].payload, Command::PlanningRecord { record }
        if record.payload()[22] == 2 && record.frame_stamp() == sim.session.binary_frame() as i32 + 1));
}

#[test]
fn planning_snapshot_restores_shared_nodes_loops_current_records_and_hash() {
    use crate::sim::snapshot::GameSnapshot;
    let (mut sim, rules, owner) = fixture();
    // Snapshot loading intentionally reseeds the unsaved Scenario RNG to zero.
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    let empty_hash = sim.state_hash();
    for actor in [1, 2] {
        append(&mut sim, &rules, owner, actor, (12, 15));
    }
    append(&mut sim, &rules, owner, 1, (16, 15));
    let connect = sim.encode_plan_connect_record(owner, 1, (12, 15)).unwrap();
    apply(&mut sim, &rules, connect);
    let mut record = sim.planning.nodes[&0].order(1).unwrap().clone();
    record.payload_mut()[22] = 2;
    let path = sim.planning.paths.get_mut(&1).unwrap();
    path.current = Some(record);
    path.executing = true;
    let hash = sim.state_hash();
    assert_ne!(hash, empty_hash);
    let bytes = GameSnapshot::save(&sim, 0, 0, "planning", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    assert_eq!(restored.state_hash(), hash);
    assert_eq!(
        save_codec::serialize(&restored.planning).unwrap(),
        save_codec::serialize(&sim.planning).unwrap()
    );
    assert_eq!(restored.planning.paths[&1].loop_start, Some(0));
    assert!(restored.planning.paths[&1].executing);
    restored.planning.paths.get_mut(&1).unwrap().pending += 1;
    assert_ne!(restored.state_hash(), hash);
}

#[test]
fn ownership_transfer_detaches_path_preserves_generated_event_and_restores_snapshot() {
    use crate::sim::mission::{MissionId, MissionType};
    use crate::sim::snapshot::GameSnapshot;
    let (mut sim, rules, owner) = fixture();
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    for cell in [(12, 15), (16, 15), (20, 15)] {
        append(&mut sim, &rules, owner, 1, cell);
    }
    append(&mut sim, &rules, owner, 2, (20, 15));
    for (actor, mission) in [(1, MissionType::Guard), (2, MissionType::Move)] {
        sim.substrate
            .entities
            .get_mut(actor)
            .unwrap()
            .mission
            .set_current_for_test(MissionId::from_known(mission), 0);
    }
    let commit = sim.encode_plan_commit_record(owner).unwrap();
    apply(&mut sim, &rules, commit);
    let generated = sim.pending_command_snapshot();
    assert_eq!(generated.len(), 1);
    let new_owner = sim.interner.intern("Russians");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            new_owner, 1, None, false, 0, 10,
        ));
    // Techno701585 ->638840 clears the path, not the published DoList event.
    sim.change_owner(1, new_owner);
    assert_eq!(sim.substrate.entities.get(1).unwrap().owner(), new_owner);
    assert!(!sim.planning.paths.contains_key(&1));
    assert_eq!(sim.planning.nodes.len(), 1);
    assert!(
        sim.planning
            .nodes
            .values()
            .all(|node| !node.orders.contains_key(&1))
    );
    assert_eq!(sim.planning_paths(owner)[0].cells, vec![(20, 15)]);
    assert_eq!(sim.pending_command_snapshot(), generated);
    let hash = sim.state_hash();
    let bytes = GameSnapshot::save(&sim, 0, 0, "planning", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(restored.state_hash(), hash);
    assert!(!restored.planning.paths.contains_key(&1));
    assert_eq!(restored.planning_paths(owner)[0].cells, vec![(20, 15)]);
    sim.session.set_tick_for_test(sim.session.tick() + 1);
    let due = sim.take_due_commands();
    assert_eq!(due, generated);
    sim.apply_due_commands(
        &due,
        Some(&rules),
        sim.session.tick() + 1,
        None,
        crate::sim::world::FrameEffects::default(),
    );
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::cell(12, 15)),
        "a published flag2 event still executes after capture"
    );
    assert_eq!(sim.substrate.entities.get(1).unwrap().owner(), new_owner);
    assert!(!sim.planning.paths.contains_key(&1));
}

#[test]
fn same_batch_commit_precedes_staged_edit_and_next_batch_commit_starts_path() {
    use crate::sim::command::CommandEnvelope;
    use crate::sim::world::FrameEffects;
    let (mut sim, rules, owner) = fixture();
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mission
        .set_current_for_test(
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
            0,
        );
    let edit = sim
        .encode_planning_order_record(
            owner,
            &Command::Move {
                entity_id: 1,
                target_rx: 12,
                target_ry: 15,
            },
        )
        .unwrap();
    let commit = sim.encode_plan_commit_record(owner).unwrap();
    let batch = [edit, commit]
        .map(|record| CommandEnvelope::new(owner, 1, Command::PlanningRecord { record }));
    sim.apply_due_commands(&batch, Some(&rules), 1, None, FrameEffects::default());
    assert_eq!(
        sim.planning.paths[&1].pending, 0,
        "commit runs before staged flag1 even when flag1 was inserted first"
    );
    assert!(!sim.planning.paths[&1].executing);
    assert!(sim.take_due_commands().is_empty());
    let commit = sim.encode_plan_commit_record(owner).unwrap();
    let batch = [CommandEnvelope::new(
        owner,
        2,
        Command::PlanningRecord { record: commit },
    )];
    sim.session.set_tick_for_test(1);
    sim.apply_due_commands(&batch, Some(&rules), 2, None, FrameEffects::default());
    assert_eq!(sim.planning.paths[&1].pending, -1);
    assert!(sim.planning.paths[&1].executing);
    assert!(sim.take_due_commands().is_empty());
    sim.session.set_tick_for_test(sim.session.tick() + 1);
    let due = sim.take_due_commands();
    assert_eq!(due.len(), 1);
    assert!(
        matches!(&due[0].payload, Command::PlanningRecord { record } if record.payload()[22] == 2)
    );
}

#[test]
fn planning_restore_rejects_broken_graph_record_actor_and_house_references() {
    use crate::sim::snapshot::GameSnapshot;
    let (mut sim, rules, owner) = fixture();
    append(&mut sim, &rules, owner, 1, (12, 15));
    for corrupt in 0..8 {
        let mut planning = sim.planning.clone();
        match corrupt {
            0 => planning.next_node = 0,
            1 => planning.nodes.get_mut(&0).unwrap().id = 9,
            2 => planning.paths.get_mut(&1).unwrap().nodes[0] = 9,
            3 => planning.paths.get_mut(&1).unwrap().pending = 2,
            4 => planning.paths.get_mut(&1).unwrap().loop_start = Some(1),
            5 => planning.nodes.get_mut(&0).unwrap().member_order.clear(),
            6 => planning.paths.get_mut(&1).unwrap().nodes.clear(),
            7 => {
                let record = planning
                    .nodes
                    .get_mut(&0)
                    .unwrap()
                    .orders
                    .get_mut(&1)
                    .unwrap()
                    .front_mut()
                    .unwrap();
                record.record.payload_mut()[0..4].copy_from_slice(&2_i32.to_le_bytes());
            }
            _ => unreachable!(),
        }
        let bytes = save_codec::serialize(&planning).unwrap();
        assert!(
            save_codec::deserialize::<super::PlanningState>(&bytes).is_err(),
            "corrupt planning case {corrupt}"
        );
    }
    for missing_actor in [true, false] {
        let (mut sim, rules, owner) = fixture();
        append(&mut sim, &rules, owner, 1, (12, 15));
        if missing_actor {
            sim.substrate.entities.remove(1);
        } else {
            sim.houses.remove_for_test(&owner);
        }
        let bytes = GameSnapshot::save(&sim, 0, 0, "planning", 0);
        let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
        assert!(matches!(
            restored.restore_after_snapshot_load(),
            Err(crate::sim::snapshot::SnapshotRestoreError::InvalidPlanningReferences { .. })
        ));
    }
}

#[test]
fn planning_restore_retains_stale_encounter_entries_after_member_detach() {
    let (mut sim, rules, owner) = fixture();
    for actor in [1, 2] {
        append(&mut sim, &rules, owner, actor, (12, 15));
    }
    sim.planning_detach_actor(1);
    assert_eq!(sim.planning.nodes[&0].member_order, vec![1, 2]);
    let bytes = save_codec::serialize(&sim.planning).unwrap();
    sim.planning = save_codec::deserialize(&bytes).unwrap();
    let connect = sim.encode_plan_connect_record(owner, 3, (12, 15)).unwrap();
    apply(&mut sim, &rules, connect);
    assert_eq!(sim.planning.paths[&3].nodes, sim.planning.paths[&2].nodes);
}

#[test]
fn two_planned_move_legs_progress_through_spawn_and_master_frame_lifecycle() {
    use crate::sim::command::CommandEnvelope;
    let (mut sim, rules, registry) = crate::sim::world::entry_test_fixture::fixture_with_rules(
        "[VehicleTypes]\n0=PLANNER\n[PLANNER]\nStrength=100\nSpeed=8\nSpeedType=Track\nMovementZone=Normal\nLocomotor={4A582741-9839-11d1-B709-00A024DDAFD1}\n",
    );
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            owner, 0, None, true, 0, 10,
        ));
    sim.session.set_current_house_for_test(Some(owner));
    let actor = sim
        .spawn_object_with_overlay_registry("PLANNER", "Americans", 15, 15, 0, &rules, &registry)
        .unwrap();
    let initial = sim.substrate.entities.get(actor).unwrap().position;
    for cell in [(16, 15), (17, 15)] {
        append(&mut sim, &rules, owner, actor, cell);
    }
    let record = sim.encode_plan_commit_record(owner).unwrap();
    let commit = CommandEnvelope::new(
        owner,
        sim.session.tick() + 1,
        Command::PlanningRecord { record },
    );
    let grid = sim.path_grid.clone();
    sim.advance_tick(
        &[commit],
        Some(&rules),
        grid.as_deref(),
        Some(&registry),
        67,
    );
    let mut saw_first = false;
    let mut finished = false;
    for _ in 0..600 {
        let due = sim.take_due_commands();
        sim.advance_tick(&due, Some(&rules), grid.as_deref(), Some(&registry), 67);
        let entity = sim.substrate.entities.get(actor).unwrap();
        saw_first |= entity.position.rx() == 16 && entity.position.ry() == 15;
        if entity.position.rx() == 17
            && entity.position.ry() == 15
            && entity.navigation.nav_com().is_none()
        {
            finished = true;
            break;
        }
    }
    assert!(saw_first, "production movement must visit the first leg");
    assert!(
        finished,
        "production Move completion/idle must start and finish the second leg"
    );
    let position = sim.substrate.entities.get(actor).unwrap().position;
    assert_ne!((position.rx(), position.ry()), (initial.rx(), initial.ry()));
    assert!(
        sim.planning_paths(owner).is_empty(),
        "finished nonloop route releases its path"
    );
}

#[test]
fn object_nodes_follow_live_xyz_share_and_retain_target_through_connect_delete() {
    let (mut sim, rules, owner) = fixture();
    sim.substrate
        .entities
        .get_mut(3)
        .unwrap()
        .position
        .set_rx_for_test(20);
    sim.substrate
        .entities
        .get_mut(3)
        .unwrap()
        .position
        .set_ry_for_test(18);
    let record = sim
        .encode_planning_order_record(
            owner,
            &Command::Attack {
                attacker_id: 1,
                target_id: 3,
            },
        )
        .unwrap();
    apply(&mut sim, &rules, record);
    assert_eq!(sim.planning_paths(owner)[0].cells, vec![(20, 18)]);
    sim.substrate
        .entities
        .get_mut(3)
        .unwrap()
        .position
        .set_rx_for_test(21);
    assert!(!sim.planning_node_at_cell(owner, (20, 18)));
    assert!(sim.planning_node_at_cell(owner, (21, 18)));
    let record = sim
        .encode_planning_order_record(
            owner,
            &Command::Attack {
                attacker_id: 2,
                target_id: 3,
            },
        )
        .unwrap();
    apply(&mut sim, &rules, record);
    assert_eq!(
        sim.planning.nodes.len(),
        1,
        "live XYZ and frame share the moved object node"
    );
    let connect = sim.encode_plan_connect_record(owner, 3, (21, 18)).unwrap();
    assert_eq!(
        super::read_target(&connect, 5),
        Some(crate::sim::command::MegaMissionTarget::Object { id: 3 })
    );
    apply(&mut sim, &rules, connect);
    assert_eq!(sim.planning.paths[&3].nodes, sim.planning.paths[&1].nodes);
    let delete = sim.encode_plan_node_delete_record(owner, (21, 18)).unwrap();
    sim.substrate
        .entities
        .get_mut(3)
        .unwrap()
        .position
        .set_rx_for_test(22);
    apply(&mut sim, &rules, delete);
    assert!(sim.planning.nodes.is_empty());
    assert!(sim.planning.paths.is_empty());
}

#[test]
fn object_node_sharing_compares_z_and_screen_hit_box_is_half_open() {
    let (mut sim, rules, owner) = fixture();
    for actor in [2, 3] {
        let position = &mut sim.substrate.entities.get_mut(actor).unwrap().position;
        position.set_rx_for_test(20);
        position.set_ry_for_test(18);
        position.set_exact_z_leptons_for_test(Some(if actor == 2 { 0 } else { 104 }));
    }
    for (actor, target) in [(1, 2), (2, 3)] {
        let record = sim
            .encode_planning_order_record(
                owner,
                &Command::Attack {
                    attacker_id: actor,
                    target_id: target,
                },
            )
            .unwrap();
        apply(&mut sim, &rules, record);
    }
    assert_eq!(
        sim.planning.nodes.len(),
        2,
        "same cell with different Z does not share"
    );
    let coord = sim.planning_node_coords(1).unwrap();
    let (x, y) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
    let point = [x as i32, y as i32];
    assert!(
        sim.planning_node_at_screen(owner, &[], [point[0] - 8, point[1] - 8])
            .is_some()
    );
    assert!(
        sim.planning_node_at_screen(owner, &[], [point[0] + 8, point[1]])
            .is_none()
    );
    let delete = sim
        .encode_plan_node_delete_record_at_screen(owner, point)
        .unwrap();
    assert_eq!(
        super::read_target(&delete, 0),
        Some(crate::sim::command::MegaMissionTarget::Object { id: 3 })
    );
}

#[test]
fn planning_target_priority_is_secondary_auxiliary_destination() {
    use crate::sim::command::{MegaMissionOrder, MegaMissionRecord, MegaMissionTarget};
    let mut record = CommandRecord::encode(4, 0, 0, &[]).unwrap();
    MegaMissionRecord {
        house_id: 0,
        frame: 0,
        source_id: 1,
        order: MegaMissionOrder::AttackMove {
            target: MegaMissionTarget::Object { id: 3 },
            destination: MegaMissionTarget::Cell { x: 20, y: 18 },
        },
    }
    .write_into(&mut record)
    .unwrap();
    assert_eq!(
        super::planning_record_target(&record),
        Some(MegaMissionTarget::Object { id: 3 })
    );
    record.payload_mut()[11] = 0;
    let (value, kind) = MegaMissionTarget::Object { id: 2 }.token().unwrap();
    record.payload_mut()[17..21].copy_from_slice(&value.to_le_bytes());
    record.payload_mut()[21] = kind;
    assert_eq!(
        super::planning_record_target(&record),
        Some(MegaMissionTarget::Object { id: 2 })
    );
    record.payload_mut()[21] = 0;
    assert_eq!(
        super::planning_record_target(&record),
        Some(MegaMissionTarget::Cell { x: 20, y: 18 })
    );
}

#[test]
fn planning_append_rejects_exact_sixteen_and_any_loop() {
    let (mut sim, rules, owner) = fixture();
    for _ in 0..16 {
        append(&mut sim, &rules, owner, 1, (12, 15));
    }
    let record = sim
        .encode_planning_order_record(
            owner,
            &Command::Move {
                entity_id: 1,
                target_rx: 16,
                target_ry: 15,
            },
        )
        .unwrap();
    assert!(!sim.apply_planning_record("Americans", &record, None));
    assert_eq!(sim.planning.paths[&1].nodes.len(), 16);
    append(&mut sim, &rules, owner, 2, (20, 15));
    let record = sim.encode_plan_connect_record(owner, 2, (20, 15)).unwrap();
    apply(&mut sim, &rules, record);
    let record = sim
        .encode_planning_order_record(
            owner,
            &Command::Move {
                entity_id: 2,
                target_rx: 16,
                target_ry: 15,
            },
        )
        .unwrap();
    assert!(!sim.apply_planning_record("Americans", &record, None));
}

#[test]
fn connect_rotates_source_loop_suffix_and_preserves_auxiliary_provenance() {
    for clicked in [0, 1, 2] {
        let (mut sim, rules, owner) = fixture();
        for cell in [(12, 15), (16, 15), (20, 15)] {
            append(&mut sim, &rules, owner, 1, cell);
        }
        let loop_record = sim.encode_plan_connect_record(owner, 1, (16, 15)).unwrap();
        apply(&mut sim, &rules, loop_record);
        let target_cell = [(12, 15), (16, 15), (20, 15)][clicked];
        let connect = sim
            .encode_plan_connect_record(owner, 2, target_cell)
            .unwrap();
        apply(&mut sim, &rules, connect);
        let expected = match clicked {
            0 => vec![0, 1, 2],
            1 => vec![1, 2],
            _ => vec![2, 1],
        };
        assert_eq!(sim.planning.paths[&2].nodes, expected);
        assert_eq!(
            sim.planning.paths[&2].loop_start,
            Some(usize::from(clicked == 0))
        );
        assert!(sim.planning.paths[&2].loop_changed);
        let record = sim.planning.nodes[&(clicked as u64)].order(2).unwrap();
        assert_eq!(
            i32::from_le_bytes(record.payload()[0..4].try_into().unwrap()),
            2
        );
        assert_eq!(
            i32::from_le_bytes(record.payload()[17..21].try_into().unwrap()),
            1,
            "connect rewrites only source, preserving native auxiliary provenance"
        );
        let bytes = save_codec::serialize(&sim.planning).unwrap();
        sim.planning = save_codec::deserialize(&bytes).unwrap();
    }
}

#[test]
fn connect_actor_group_follows_tail_encounter_order_and_deduplicates() {
    let (mut sim, rules, owner) = fixture();
    for actor in [2, 1] {
        append(&mut sim, &rules, owner, actor, (12, 15));
    }
    assert_eq!(
        sim.planning_connect_actors(owner, &[1, 2, 3]),
        vec![2, 1, 3]
    );
    sim.planning_detach_actor(2);
    assert_eq!(sim.planning_connect_actors(owner, &[1, 3]), vec![1, 3]);
}

#[test]
fn deleting_loop_node_cancels_too_short_loop_and_normalizes_last_pending() {
    let (mut sim, rules, owner) = fixture();
    for cell in [(12, 15), (16, 15), (20, 15)] {
        append(&mut sim, &rules, owner, 1, cell);
    }
    let record = sim.encode_plan_connect_record(owner, 1, (12, 15)).unwrap();
    apply(&mut sim, &rules, record);
    let delete = sim.encode_plan_node_delete_record(owner, (16, 15)).unwrap();
    apply(&mut sim, &rules, delete);
    assert_eq!(sim.planning.paths[&1].loop_start, None);
    let delete = sim.encode_plan_node_delete_record(owner, (12, 15)).unwrap();
    apply(&mut sim, &rules, delete);
    sim.planning.paths.get_mut(&1).unwrap().executing = true;
    let delete = sim.encode_plan_node_delete_record(owner, (20, 15)).unwrap();
    apply(&mut sim, &rules, delete);
    assert_eq!(sim.planning.paths[&1].pending, -1);
}

#[test]
fn cell_attack_and_area_guard_tails_are_terminal_for_append() {
    use crate::sim::command::{MegaMissionOrder, MegaMissionRecord, MegaMissionTarget};
    for order in [
        MegaMissionOrder::Attack {
            target: MegaMissionTarget::Cell { x: 12, y: 15 },
        },
        MegaMissionOrder::AreaGuard {
            post: MegaMissionTarget::Cell { x: 12, y: 15 },
        },
    ] {
        let (mut sim, rules, owner) = fixture();
        let mut record = CommandRecord::encode(4, 0, 0, &[]).unwrap();
        MegaMissionRecord {
            house_id: 0,
            frame: 0,
            source_id: 1,
            order,
        }
        .write_into(&mut record)
        .unwrap();
        record.payload_mut()[22] = 1;
        apply(&mut sim, &rules, record);
        let record = sim
            .encode_planning_order_record(
                owner,
                &Command::Move {
                    entity_id: 1,
                    target_rx: 16,
                    target_ry: 15,
                },
            )
            .unwrap();
        assert!(!sim.apply_planning_record("Americans", &record, None));
        assert_eq!(sim.planning.paths[&1].nodes.len(), 1);
    }
}

#[test]
fn planned_force_attack_preserves_admitted_friendly_target_through_native_dispatch() {
    let (mut sim, rules, owner) = fixture();
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().owner(),
        sim.substrate.entities.get(2).unwrap().owner()
    );
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mission
        .set_current_for_test(
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
            0,
        );
    let record = sim
        .encode_planning_order_record(
            owner,
            &Command::Attack {
                attacker_id: 1,
                target_id: 2,
            },
        )
        .unwrap();
    apply(&mut sim, &rules, record);
    append(&mut sim, &rules, owner, 1, (16, 15));
    let commit = sim.encode_plan_commit_record(owner).unwrap();
    apply(&mut sim, &rules, commit);
    sim.session.set_tick_for_test(sim.session.tick() + 1);
    let due = sim.take_due_commands();
    assert_eq!(due.len(), 1);
    assert!(
        matches!(&due[0].payload, Command::PlanningRecord { record } if record.payload()[22] == 2)
    );
    assert!(
        sim.apply_command("Americans", &due[0].payload, Some(&rules)),
        "native Attack event retains the Ctrl-admitted friendly target"
    );
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .attack_target
            .target(),
        Some(crate::sim::combat::TargetKind::Entity(2))
    );
    assert_eq!(
        sim.planning.paths[&1].nodes.len(),
        1,
        "executing the planned attack preserves the following leg"
    );
}

#[test]
fn captured_actor_path_commits_for_current_house_and_preserves_original_issuer() {
    let (mut sim, rules, owner) = fixture();
    let new_owner = sim.interner.intern("Russians");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            new_owner, 1, None, false, 0, 10,
        ));
    append(&mut sim, &rules, owner, 2, (12, 15));
    sim.change_owner(1, new_owner);
    // The old issuer's delayed flag1 is admitted after capture. Its actor's
    // current house determines node grouping and commit; event bytes keep issuer.
    append(&mut sim, &rules, owner, 1, (12, 15));
    assert_eq!(
        sim.planning.nodes.len(),
        1,
        "same-coordinate firstmember house still matches the old issuer"
    );
    append(&mut sim, &rules, owner, 1, (16, 15));
    assert!(sim.planning_node_at_cell(new_owner, (16, 15)));
    assert!(!sim.planning_node_at_cell(owner, (16, 15)));
    assert_eq!(sim.planning_paths(new_owner).len(), 1);
    sim.planning_detach_actor(2);
    assert!(
        sim.planning_node_at_cell(new_owner, (12, 15)),
        "removing the first member exposes the captured actor's current group"
    );
    assert!(!sim.planning_node_at_cell(owner, (12, 15)));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mission
        .set_current_for_test(
            crate::sim::mission::MissionId::from_known(crate::sim::mission::MissionType::Guard),
            0,
        );
    let record = sim.encode_plan_commit_record(new_owner).unwrap();
    let envelope = sim
        .decode_native_command_record(&record, sim.session.tick())
        .unwrap();
    assert!(sim.apply_command("Russians", &envelope.payload, Some(&rules)));
    let generated = sim.pending_command_snapshot();
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].owner, owner);
    assert!(
        matches!(&generated[0].payload, Command::PlanningRecord { record } if record.house_id() == 0)
    );
    let bytes = save_codec::serialize(&sim.planning).unwrap();
    sim.planning = save_codec::deserialize(&bytes).unwrap();
    sim.validate_planning_references().unwrap();
    sim.session.set_tick_for_test(sim.session.tick() + 1);
    let due = sim.take_due_commands();
    assert!(sim.apply_command("Americans", &due[0].payload, Some(&rules)));
    assert_eq!(
        sim.substrate.entities.get(1).unwrap().navigation.nav_com(),
        Some(crate::sim::components::NavTargetRef::cell(12, 15))
    );
    assert!(sim.planning_node_at_cell(new_owner, (16, 15)));
}

#[test]
fn append_and_connect_receivers_defer_actor_owner_and_lifetime_admission() {
    {
        let (mut sim, rules, owner) = fixture();
        let actor = sim.substrate.entities.get_mut(1).unwrap();
        actor.lifecycle.set_object_alive_for_test(false);
        actor.lifecycle.set_in_limbo_for_test(true);
        append(&mut sim, &rules, owner, 1, (12, 15));
        assert_eq!(sim.planning.paths[&1].nodes.len(), 1);
        let bytes = save_codec::serialize(&sim.planning).unwrap();
        sim.planning = save_codec::deserialize(&bytes).unwrap();
        sim.validate_planning_references().unwrap();
    }
    {
        let (mut sim, rules, owner) = fixture();
        append(&mut sim, &rules, owner, 1, (12, 15));
        let connect = sim.encode_plan_connect_record(owner, 2, (12, 15)).unwrap();
        let new_owner = sim.interner.intern("Russians");
        sim.houses
            .register(crate::sim::house_state::HouseState::new(
                new_owner, 1, None, false, 0, 10,
            ));
        sim.change_owner(2, new_owner);
        let actor = sim.substrate.entities.get_mut(2).unwrap();
        actor.lifecycle.set_object_alive_for_test(false);
        actor.lifecycle.set_in_limbo_for_test(true);
        apply(&mut sim, &rules, connect);
        assert_eq!(sim.planning.paths[&2].nodes, sim.planning.paths[&1].nodes);
    }
}

#[test]
fn overlapping_connect_delete_removes_each_occurrence_and_restores_graph() {
    let (mut sim, rules, owner) = fixture();
    append(&mut sim, &rules, owner, 1, (12, 15));
    append(&mut sim, &rules, owner, 1, (20, 15));
    append(&mut sim, &rules, owner, 2, (16, 15));
    append(&mut sim, &rules, owner, 2, (20, 15));
    let connect = sim.encode_plan_connect_record(owner, 1, (16, 15)).unwrap();
    apply(&mut sim, &rules, connect);
    assert_eq!(sim.planning.paths[&1].nodes, vec![0, 1, 2, 1]);
    let delete = sim.encode_plan_node_delete_record(owner, (20, 15)).unwrap();
    apply(&mut sim, &rules, delete);
    assert_eq!(sim.planning.paths[&1].nodes, vec![0, 2]);
    let bytes = save_codec::serialize(&sim.planning).unwrap();
    sim.planning = save_codec::deserialize(&bytes).unwrap();
}

#[test]
fn overlapping_connect_consumption_preserves_occurrences_and_loop_first_record() {
    for looped in [false, true] {
        let (mut sim, rules, owner) = fixture();
        let position = &mut sim.substrate.entities.get_mut(3).unwrap().position;
        position.set_rx_for_test(20);
        position.set_ry_for_test(15);
        position.set_sub_x_for_test(crate::util::lepton::CELL_CENTER_LEPTON);
        position.set_sub_y_for_test(crate::util::lepton::CELL_CENTER_LEPTON);
        append(&mut sim, &rules, owner, 1, (12, 15));
        let attack = sim
            .encode_planning_order_record(
                owner,
                &Command::Attack {
                    attacker_id: 1,
                    target_id: 3,
                },
            )
            .unwrap();
        apply(&mut sim, &rules, attack);
        append(&mut sim, &rules, owner, 2, (16, 15));
        append(&mut sim, &rules, owner, 2, (20, 15));
        let connect = sim.encode_plan_connect_record(owner, 1, (16, 15)).unwrap();
        apply(&mut sim, &rules, connect);
        assert_eq!(sim.planning.paths[&1].nodes, vec![0, 1, 2, 1]);
        assert_eq!(sim.planning.nodes[&1].orders[&1].len(), 2);
        if looped {
            let connect = sim.encode_plan_connect_record(owner, 1, (12, 15)).unwrap();
            apply(&mut sim, &rules, connect);
        }
        // Keep fixture actors on Move, so commit does not start consumption.
        let commit = sim.encode_plan_commit_record(owner).unwrap();
        apply(&mut sim, &rules, commit);
        for (step, mission) in [2, 1, 2, if looped { 1 } else { 2 }]
            .into_iter()
            .enumerate()
        {
            assert!(sim.planning_start_actor(1));
            assert_eq!(
                sim.planning.paths[&1].current.as_ref().unwrap().payload()[5],
                mission,
                "looped={looped}, consume step={step}"
            );
            if step == 1 {
                assert_eq!(
                    sim.planning.nodes[&1].orders[&1].len(),
                    if looped { 2 } else { 1 }
                );
            }
            let bytes = save_codec::serialize(&sim.planning).unwrap();
            sim.planning = save_codec::deserialize(&bytes).unwrap();
            sim.planning_acknowledge(1);
        }
        if looped {
            assert_eq!(sim.planning.paths[&1].nodes, vec![0, 1, 2, 1]);
            assert_eq!(sim.planning.nodes[&1].orders[&1].len(), 2);
        } else {
            assert!(!sim.planning.paths.contains_key(&1));
            assert!(!sim.planning.nodes[&1].orders.contains_key(&1));
        }
    }
}

#[test]
fn connect_suffix_copies_the_selected_source_actor_order_at_shared_node() {
    let (mut sim, rules, owner) = fixture();
    let position = &mut sim.substrate.entities.get_mut(1).unwrap().position;
    position.set_rx_for_test(20);
    position.set_ry_for_test(15);
    position.set_sub_x_for_test(crate::util::lepton::CELL_CENTER_LEPTON);
    position.set_sub_y_for_test(crate::util::lepton::CELL_CENTER_LEPTON);
    append(&mut sim, &rules, owner, 2, (16, 15));
    let attack = sim
        .encode_planning_order_record(
            owner,
            &Command::Attack {
                attacker_id: 3,
                target_id: 1,
            },
        )
        .unwrap();
    apply(&mut sim, &rules, attack);
    append(&mut sim, &rules, owner, 2, (20, 15));
    assert_eq!(
        sim.planning.paths[&2].nodes[1],
        sim.planning.paths[&3].nodes[0]
    );
    let connect = sim.encode_plan_connect_record(owner, 1, (16, 15)).unwrap();
    apply(&mut sim, &rules, connect);
    let node = sim.planning.paths[&1].nodes[1];
    let record = sim.planning.nodes[&node].order(1).unwrap().clone();
    assert_eq!(
        record.payload()[5],
        2,
        "source actor Move survives a different first member Attack"
    );
}

#[test]
fn draw_graph_merges_equal_predecessor_coords_and_tracks_commit_boundary() {
    let (mut sim, rules, owner) = fixture();
    for actor in [2, 1] {
        append(&mut sim, &rules, owner, actor, (12, 15));
        append(&mut sim, &rules, owner, actor, (16, 15));
    }
    let graph = sim.planning_draw_graph(owner, &[], None, None);
    assert_eq!(
        graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert_eq!(graph.nodes[0].members, vec![2, 1]);
    assert!(graph.nodes[0].incoming.is_empty());
    assert_eq!(graph.nodes[1].incoming.len(), 1);
    assert!(graph.nodes[1].incoming[0].bright);
    assert_eq!(graph.actors.len(), 2);
    assert!(
        graph
            .actors
            .iter()
            .all(|actor| actor.connection.as_ref().unwrap().bright)
    );
    assert!(graph.nodes.iter().all(|node| node.badge.is_none()));
    let commit = sim.encode_plan_commit_record(owner).unwrap();
    apply(&mut sim, &rules, commit);
    append(&mut sim, &rules, owner, 1, (20, 15));
    let graph = sim.planning_draw_graph(owner, &[], None, None);
    assert!(!graph.nodes[0].marker_bright);
    assert!(!graph.nodes[1].incoming[0].bright);
    assert!(graph.nodes[2].incoming[0].bright);
    assert!(
        graph
            .actors
            .iter()
            .all(|actor| !actor.connection.as_ref().unwrap().bright)
    );
}

#[test]
fn native_patrol_conversion_updates_shared_events_and_preserves_other_bytes() {
    for row in corpus()["patrol_conversions"].as_array().unwrap() {
        let (mut sim, _, owner) = fixture();
        let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
            "[VehicleTypes]\n0=TEST\n1=UNARMED\n[TEST]\nStrength=100\nSpeed=4\nPrimary=GUN\n[UNARMED]\nStrength=100\nSpeed=4\n[WeaponTypes]\n0=GUN\n[GUN]\nDamage=10\nRange=5\n",
        )).unwrap();
        let unarmed = sim.interner.intern("UNARMED");
        let capabilities = row["capabilities"].as_array().unwrap();
        for (member, capable) in capabilities.iter().enumerate() {
            if !capable.as_bool().unwrap() {
                sim.substrate
                    .entities
                    .get_mut(member as u64 + 1)
                    .unwrap()
                    .set_type_ref_for_test(unarmed);
            }
        }
        let missions = row["missions"].as_array().unwrap();
        for (index, _) in missions.iter().enumerate() {
            for member in 0..capabilities.len() {
                append(
                    &mut sim,
                    &rules,
                    owner,
                    member as u64 + 1,
                    (12 + index as u16 * 4, 15),
                );
            }
        }
        let mut before = Vec::new();
        for (index, missions) in missions.iter().enumerate() {
            for (member, mission) in missions.as_array().unwrap().iter().enumerate() {
                let order = sim
                    .planning
                    .nodes
                    .get_mut(&(index as u64))
                    .unwrap()
                    .orders
                    .get_mut(&(member as u64 + 1))
                    .unwrap()
                    .front_mut()
                    .unwrap();
                order.record.payload_mut()[5] = mission.as_u64().unwrap() as u8;
                before.push((index as u64, member as u64 + 1, order.record.clone()));
            }
        }
        let start = row["loop_start"].as_u64().unwrap() as u16;
        let connect = sim
            .encode_plan_connect_record(owner, 1, (12 + start * 4, 15))
            .unwrap();
        apply(&mut sim, &rules, connect);
        for (node, actor, mut expected) in before {
            expected.payload_mut()[5] = row["after"][node as usize][actor as usize - 1]
                .as_u64()
                .unwrap() as u8;
            assert_eq!(
                sim.planning.nodes[&node].order(actor).unwrap(),
                &expected,
                "{row:?}"
            );
        }
    }
}

#[test]
fn closed_move_patrol_dispatches_attack_move_through_the_production_receiver() {
    use crate::sim::mission::{MissionId, MissionType};
    let (mut sim, _, owner) = fixture();
    let rules = RuleSet::from_ini(&crate::rules::ini_parser::IniFile::from_str(
        "[VehicleTypes]\n0=TEST\n[TEST]\nStrength=100\nSpeed=4\nPrimary=GUN\n[WeaponTypes]\n0=GUN\n[GUN]\nDamage=10\nRange=5\n",
    )).unwrap();
    append(&mut sim, &rules, owner, 1, (12, 15));
    append(&mut sim, &rules, owner, 1, (20, 15));
    append(&mut sim, &rules, owner, 2, (16, 15));
    append(&mut sim, &rules, owner, 2, (20, 15));
    let connect = sim.encode_plan_connect_record(owner, 1, (16, 15)).unwrap();
    apply(&mut sim, &rules, connect);
    assert_eq!(sim.planning.paths[&1].nodes, vec![0, 1, 2, 1]);
    let connect = sim.encode_plan_connect_record(owner, 1, (12, 15)).unwrap();
    apply(&mut sim, &rules, connect);
    assert_eq!(sim.planning.nodes[&1].orders[&1].len(), 2);
    for node in sim.planning.nodes.values() {
        assert!(
            node.orders
                .values()
                .flatten()
                .all(|order| order.record.payload()[5] == 29)
        );
    }
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Guard), 0);
    let commit = sim.encode_plan_commit_record(owner).unwrap();
    apply(&mut sim, &rules, commit);
    let dispatch = sim.planning.paths[&1].current.clone().unwrap();
    assert_eq!(dispatch.payload()[5], 29);
    assert_eq!(dispatch.payload()[22], 2);
    apply(&mut sim, &rules, dispatch);
    let mega = sim.mega_mission(1);
    assert!(mega.is_attack_move());
    assert_eq!(
        mega.destination(),
        Some(crate::sim::combat::TargetKind::Cell(12, 15))
    );
    assert_eq!(sim.planning.paths[&1].loop_start, Some(0));
}

#[test]
fn attack_move_loop_keeps_task_badges_on_entry_and_closing_legs() {
    use crate::sim::combat::TargetKind;
    let (mut sim, rules, owner) = fixture();
    for (x, y) in [(12, 15), (16, 15), (16, 19)] {
        let record = sim
            .encode_planning_order_record(
                owner,
                &Command::AttackMove {
                    entity_id: 1,
                    target: None,
                    destination: Some(TargetKind::Cell(x, y)),
                },
            )
            .unwrap();
        apply(&mut sim, &rules, record);
    }
    let connect = sim.encode_plan_connect_record(owner, 1, (12, 15)).unwrap();
    apply(&mut sim, &rules, connect);
    for committed in [false, true] {
        if committed {
            let commit = sim.encode_plan_commit_record(owner).unwrap();
            apply(&mut sim, &rules, commit);
        }
        let graph = sim.planning_draw_graph(owner, &[], None, None);
        assert_eq!(graph.nodes.len(), 3);
        assert!(
            graph
                .nodes
                .iter()
                .all(|node| { node.badge == Some(super::PlanningBadgeKind::AttackMove) })
        );
        let closing = graph.nodes[0].loop_segment.as_ref().unwrap();
        assert_eq!(closing.source, graph.nodes[2].coord);
        assert_eq!(closing.target, graph.nodes[0].coord);
        assert_eq!(closing.bright, !committed);
        assert_eq!(
            graph
                .nodes
                .iter()
                .map(|node| node.incoming.len())
                .sum::<usize>(),
            2
        );
        assert_eq!(
            graph.actors[0].badge,
            Some(super::PlanningBadgeKind::AttackMove)
        );
        assert_eq!(
            graph.actors[0].connection.as_ref().unwrap().target,
            graph.nodes[0].coord
        );
    }
}

#[test]
fn draw_graph_current_destination_uses_attack_target_and_mega_resume_latch() {
    use crate::sim::{
        combat::TargetKind,
        components::NavTargetRef,
        mission::{MissionId, MissionType, mega_mission::MegaMission},
    };
    let (mut sim, rules, owner) = fixture();
    append(&mut sim, &rules, owner, 1, (20, 15));
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity
            .navigation
            .set_nav_com_for_test(Some(NavTargetRef::cell(16, 15)));
        entity
            .navigation
            .set_nav_com_aux_for_test(Some(NavTargetRef::cell(12, 15)));
    }
    let graph = sim.planning_draw_graph(owner, &[], None, None);
    let actor = &graph.actors[0];
    assert_eq!(
        actor.current_leg.as_ref().unwrap().target,
        crate::sim::components::DriveCoord::cell(12, 15, 0)
    );
    assert_eq!(
        actor.connection.as_ref().unwrap().source,
        actor.current_leg.as_ref().unwrap().target
    );
    {
        let entity = sim.substrate.entities.get_mut(1).unwrap();
        entity
            .mission
            .set_current_for_test(MissionId::from_known(MissionType::Attack), 0);
        entity
            .attack_target
            .set_for_test(Some(TargetKind::Entity(2)));
        entity.mega_mission =
            MegaMission::for_test(true, Some(TargetKind::Cell(16, 15)), None, true);
    }
    let graph = sim.planning_draw_graph(owner, &[], None, None);
    let actor = &graph.actors[0];
    assert_eq!(
        actor.current_leg.as_ref().unwrap().target,
        crate::sim::movement::ground_pose::object_get_coords(
            sim.substrate.entities.get(2).unwrap(),
            None
        )
    );
    assert!(actor.connection.is_none());
    assert_eq!(
        actor.mega_leg.as_ref().unwrap().source,
        actor.current_leg.as_ref().unwrap().target
    );
    assert_eq!(
        actor.mega_leg.as_ref().unwrap().target,
        crate::sim::components::DriveCoord::cell(16, 15, 0)
    );
    assert_eq!(actor.mega_badge, Some(super::PlanningBadgeKind::AttackMove));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .mission
        .set_current_for_test(MissionId::from_known(MissionType::Guard), 0);
    let graph = sim.planning_draw_graph(owner, &[], None, None);
    assert!(graph.actors[0].current_leg.is_none());
    assert!(graph.actors[0].connection.is_some());
}

#[test]
fn delete_handoff_uses_first_member_successor_and_invalidates_retained_node() {
    use crate::sim::world::lifecycle::LifecycleOutput;
    let (mut sim, rules, owner) = fixture();
    append(&mut sim, &rules, owner, 2, (12, 15));
    append(&mut sim, &rules, owner, 2, (16, 15));
    append(&mut sim, &rules, owner, 1, (12, 15));
    append(&mut sim, &rules, owner, 1, (20, 15));
    sim.lifecycle_outputs.clear();
    let delete = sim.encode_plan_node_delete_record(owner, (12, 15)).unwrap();
    apply(&mut sim, &rules, delete);
    assert_eq!(
        sim.lifecycle_outputs,
        vec![LifecycleOutput::PlanningNodeDeleted {
            owner,
            node_id: 0,
            next_node: Some(1),
        }]
    );
    let graph = sim.planning_draw_graph(owner, &[], None, Some(0));
    assert_eq!(graph.highlighted_node, None);
    assert_eq!(graph.hovered_node, None);
    assert_eq!(
        sim.planning_draw_graph(owner, &[], None, Some(1))
            .highlighted_node,
        Some(1)
    );
    sim.lifecycle_outputs.clear();
    let delete = sim.encode_plan_node_delete_record(owner, (16, 15)).unwrap();
    apply(&mut sim, &rules, delete);
    assert_eq!(
        sim.lifecycle_outputs,
        vec![LifecycleOutput::PlanningNodeDeleted {
            owner,
            node_id: 1,
            next_node: None,
        }]
    );
}

#[test]
fn retained_highlight_keeps_hidden_node_coordinates_and_full_membership() {
    let (mut sim, rules, owner) = fixture();
    let enemy = sim.interner.intern("Russians");
    sim.houses
        .register(crate::sim::house_state::HouseState::new(
            enemy, 1, None, false, 0, 10,
        ));
    sim.change_owner(2, enemy);
    let record = sim
        .encode_planning_order_record(
            enemy,
            &Command::Move {
                entity_id: 2,
                target_rx: 12,
                target_ry: 15,
            },
        )
        .unwrap();
    let envelope = sim
        .decode_native_command_record(&record, sim.session.tick())
        .unwrap();
    assert!(sim.apply_command("Russians", &envelope.payload, Some(&rules)));
    // Connect receiver639800 resolves actors without an owner gate. The node
    // remains hidden by its FIRST member while containing a visible actor.
    let connect = sim.encode_plan_connect_record(enemy, 1, (12, 15)).unwrap();
    let envelope = sim
        .decode_native_command_record(&connect, sim.session.tick())
        .unwrap();
    assert!(sim.apply_command("Russians", &envelope.payload, Some(&rules)));
    let coord = sim.planning_node_coords(0).unwrap();
    let graph = sim.planning_draw_graph(owner, &[], None, Some(0));
    assert!(graph.nodes.is_empty());
    assert_eq!(graph.actors.len(), 1);
    assert_eq!(graph.actors[0].actor_id, 1);
    assert_eq!(graph.highlighted_node, Some(0));
    assert_eq!(graph.highlighted_coord, Some(coord));
    assert_eq!(graph.highlighted_members, vec![2, 1]);
    assert_eq!(graph.hovered_coord, None);
}

#[test]
fn press_refusals_and_hover_follow_the_selected_paths() {
    use super::PlanningPressRefusal::{InvalidNode, NoAircraft, NoBuildings, NoUnknowns};
    use crate::map::entities::EntityCategory;
    let (mut sim, rules, owner) = fixture();
    for (id, category) in [
        (4, EntityCategory::Aircraft),
        (5, EntityCategory::Structure),
    ] {
        assert_eq!(sim.allocate_stable_id(), id);
        crate::sim::world::lifecycle_tests::insert_entity(&mut sim, id, category);
    }
    for cell in [(12, 15), (16, 15), (20, 15), (24, 15)] {
        append(&mut sim, &rules, owner, 1, cell);
    }
    append(&mut sim, &rules, owner, 2, (30, 15));
    let point = |sim: &Simulation, node: u64| {
        let coord = sim.planning_node_coords(node).unwrap();
        let (x, y) = crate::util::lepton::absolute_leptons_to_screen(coord.x, coord.y, coord.z);
        [x as i32, y as i32]
    };
    // 63AC70: the first selected token decides; objects without one pass.
    assert_eq!(sim.planning_invalid_node_reason(&[1], 3), Some(2));
    assert_eq!(sim.planning_invalid_node_reason(&[1], 2), Some(3));
    assert_eq!(sim.planning_invalid_node_reason(&[1], 1), None);
    assert_eq!(sim.planning_invalid_node_reason(&[2], 1), Some(5));
    assert_eq!(sim.planning_invalid_node_reason(&[3, 4, 1], 3), Some(2));
    assert_eq!(sim.planning_invalid_node_reason(&[3], 3), None);

    let last = point(&sim, 3);
    assert_eq!(sim.planning_press_refusal(&[1], last), Some(InvalidNode));
    assert_eq!(sim.planning_node_at_screen(owner, &[1], last), None);
    assert_eq!(
        sim.planning_draw_graph(owner, &[1], Some(last), None)
            .hovered_node,
        None
    );
    assert!(sim.planning_release_refused(&[1], last));
    let first = point(&sim, 1);
    assert_eq!(sim.planning_press_refusal(&[1], first), None);
    assert_eq!(
        sim.planning_node_at_screen(owner, &[1], first),
        Some((16, 15))
    );
    assert_eq!(
        sim.planning_draw_graph(owner, &[1], Some(first), None)
            .hovered_node,
        Some(1)
    );
    assert!(!sim.planning_release_refused(&[1], first));
    // A node hit skips 639DA0, so an aircraft without a token passes.
    assert_eq!(sim.planning_press_refusal(&[4, 5], first), None);

    // 639DA0 with no node hit: the first object that cannot use waypoints.
    let open = [first[0] + 200, first[1] + 200];
    assert_eq!(sim.planning_press_refusal(&[1, 3], open), None);
    assert_eq!(
        sim.planning_press_refusal(&[1, 5, 4], open),
        Some(NoBuildings)
    );
    assert_eq!(sim.planning_press_refusal(&[4, 5], open), Some(NoAircraft));
    assert_eq!(sim.planning_press_refusal(&[1, 99], open), Some(NoUnknowns));

    // The press and release loops hit every house's nodes; the hover keeps
    // only own ones.
    let rival = sim.interner.intern("Russians");
    sim.substrate
        .entities
        .get_mut(2)
        .unwrap()
        .set_owner_for_test(rival);
    let foreign = point(&sim, 4);
    assert_eq!(sim.planning_node_at_screen(owner, &[], foreign), None);
    assert_eq!(sim.planning_press_refusal(&[1], foreign), Some(InvalidNode));
    assert_eq!(sim.planning_press_refusal(&[4], foreign), None);

    // A loop (token+0x94) refuses every node until the plan is cleared.
    let connect = sim.encode_plan_connect_record(owner, 1, (12, 15)).unwrap();
    apply(&mut sim, &rules, connect);
    assert_eq!(sim.planning.paths[&1].loop_start, Some(0));
    assert_eq!(sim.planning_invalid_node_reason(&[1], 1), Some(1));
    assert_eq!(sim.planning_press_refusal(&[1], first), Some(InvalidNode));
}
