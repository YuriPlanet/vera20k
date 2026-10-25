//! Supplied native Foot periodic inputs; natural retail flight lives in the
//! world Jumpjet regression, so these controls do not invent construction.
use super::tests::{event_ingress_fixture, install_event_ingress_map};
use crate::map::entities::EntityCategory;
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::locomotor::LocomotorState;
use crate::sim::world::UninitContext;
use serde_json::Value;

#[test]
fn successful_load_discards_only_the_dummy_cell_tag_attachment() {
    use super::{TagEventInput, tests::spawn_event_ingress_unit};
    use crate::map::{cell_index::NativeCellIdentity, resolved_terrain::SharedCellDummy};
    use crate::sim::{snapshot::GameSnapshot, world::Simulation};
    use crate::util::save_codec;

    let (mut sim, rules, overlays, houses) = event_ingress_fixture();
    let actor = spawn_event_ingress_unit(&mut sim, &rules, houses[0], (15, 15));
    install_event_ingress_map(
        &mut sim,
        &rules,
        "[Triggers]\nD=Country0,<none>,Dummy,0,1,1,1,0\nR=Country0,<none>,Real,0,1,1,1,0\n\
         [Events]\nD=2,4,0,0,13,0,1\nR=2,4,0,0,13,0,1\n\
         [Actions]\nD=1,28,0,7,0,0,0,0,A\nR=1,28,0,8,0,0,0,0,A\n\
         [Tags]\nDT=2,Dummy,D\nRT=2,Real,R\n[CellTags]\n99099=DT\n15015=RT\n",
    );
    let dummy = Simulation::trigger_cell_identity(&sim.resolved_terrain, (99, 99));
    let real = Simulation::trigger_cell_identity(&sim.resolved_terrain, (15, 15));
    assert_eq!(dummy, NativeCellIdentity::Dummy);
    assert_ne!(real, dummy);
    assert_eq!(sim.trigger_runtime.cell_tags.len(), 2);
    // Retain a partially completed live Trigger with its constructor-started
    // Event13 timer, so a detach/refcount repair or timer reset is detectable.
    let triggers: Vec<_> = sim.trigger_runtime.trigger_order.clone();
    for trigger in triggers {
        assert!(!sim.register_live_trigger_event(
            trigger,
            TagEventInput {
                kind: 4,
                ..Default::default()
            },
            true,
            &rules,
        ));
        assert_ne!(sim.trigger_runtime.trigger(trigger).unwrap().completed, 0);
    }
    let terrain = sim.resolved_terrain.as_ref().unwrap().clone();
    let saved = GameSnapshot::save(&sim, 0, 0, "Dummy Cell Tag", 0);
    let mut loaded = GameSnapshot::load(&saved).unwrap().sim;
    let mut expected = loaded.trigger_runtime.clone();
    assert!(expected.cell_tags.remove(&dummy).is_some());
    let rng = loaded.rng_state();
    let pending = save_codec::serialize(&loaded.substrate.pending_delete).unwrap();

    // MouseLoad5BE150 -> Resize565C10 -> CellCtor47BC18 clears Dummy+3C
    // directly; TagLoad6E5730 retains refs and live Trigger state.
    loaded.bind_shared_cell_dummy(SharedCellDummy::fresh());
    loaded.reconstruct_cellclass_dummy_for_map_resize();
    assert!(!loaded.trigger_runtime.cell_tags.contains_key(&dummy));
    assert_eq!(
        save_codec::serialize(&loaded.trigger_runtime).unwrap(),
        save_codec::serialize(&expected).unwrap(),
        "real attachments, Tag refs, definitions, timers and registry state survive",
    );
    assert_eq!(loaded.rng_state(), rng);
    assert_eq!(
        save_codec::serialize(&loaded.substrate.pending_delete).unwrap(),
        pending
    );
    loaded.restore_after_snapshot_load().unwrap();
    loaded.rebuild_caches_after_load(
        terrain,
        crate::sim::pathfinding::terrain_speed::TerrainSpeedConfig::default(),
        &rules,
    );
    loaded
        .restore_map_authority_after_snapshot_load(&rules, &overlays, None)
        .unwrap();
    assert_eq!(
        loaded.trigger_runtime.cell_tags[&real],
        expected.cell_tags[&real]
    );
    loaded.session.set_binary_frame_for_test(15);
    loaded.dispatch_foot_cell_tag_events(actor, UninitContext::new(&rules, Some(&overlays)));
    assert!(
        loaded.global_variables()[8],
        "the real Cell still dispatches after load"
    );
    assert!(!loaded.global_variables()[7]);
}

fn corpus() -> &'static Value {
    crate::test_fixture::json("tools/spatial_oracle/trigger_periodic_cell_events.json")
}

#[test]
fn periodic_flight_cell_admission_matches_the_original_class_getter_controls() {
    let rows = corpus()["caller_controls"].as_array().unwrap();
    assert_eq!(rows.len(), 35);
    for row in rows {
        let input = &row["input"];
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        sim.session
            .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
        let (category, name) = match input["cls"].as_str().unwrap() {
            "unit" => (EntityCategory::Unit, "TAGUNIT"),
            "infantry" => (EntityCategory::Infantry, "JUMPJET"),
            "aircraft" => (EntityCategory::Aircraft, "HORNET"),
            _ => unreachable!(),
        };
        let type_name = match input["rocket_rule"].as_u64() {
            Some(0x4E0) => rules.missile_spawn.v3.type_name.as_str(),
            Some(0x514) => rules.missile_spawn.dmisl.type_name.as_str(),
            None => name,
            _ => unreachable!(),
        };
        let cell = input.get("cell_xy").map_or((15, 15), |xy| {
            (
                xy[0].as_i64().unwrap() as u16,
                xy[1].as_i64().unwrap() as u16,
            )
        });
        let (x, y) = input
            .get("xy")
            .map_or((15 * 256 + 128, 15 * 256 + 128), |xy| {
                (
                    xy[0].as_i64().unwrap() as i32,
                    xy[1].as_i64().unwrap() as i32,
                )
            });
        let id = sim.substrate.allocate_stable_id();
        let mut actor = GameEntity::test_default_of_category(
            id,
            type_name,
            "House0",
            (x / 256) as u16,
            (y / 256) as u16,
            category,
        );
        actor.set_type_ref_for_test(sim.interner.intern(type_name));
        actor.set_owner_for_test(houses[0]);
        actor
            .position
            .set_sub_x_for_test(crate::util::fixed_math::SimFixed::from_num(x % 256));
        actor
            .position
            .set_sub_y_for_test(crate::util::fixed_math::SimFixed::from_num(y % 256));
        actor
            .position
            .set_exact_z_leptons_for_test(Some(input["z"].as_i64().unwrap() as i32));
        actor
            .lifecycle
            .set_cell_marked_for_test(input["marked"] == 1);
        actor.set_on_bridge(input["on_bridge"] == 1);
        actor
            .lifecycle
            .set_object_alive_for_test(input["alive"] != 0);
        actor.lifecycle.set_in_limbo_for_test(input["limbo"] == 1);
        if let Some(health) = input["health"].as_i64() {
            actor.health.set_for_test(health as i32);
        }
        if let Some(phase) = input["rocket_phase"].as_i64() {
            let mut locomotor =
                LocomotorState::for_test_kind(crate::rules::locomotor_type::LocomotorKind::Rocket);
            locomotor
                .rocket_runtime_mut()
                .unwrap()
                .set_mission_state_for_test(phase as i32);
            actor.locomotor = Some(locomotor);
        }
        sim.substrate.entities.insert(actor);
        if let Some(flags) = input["cellflags"].as_u64() {
            sim.resolved_terrain
                .as_mut()
                .unwrap()
                .cell_mut_for_test(cell.0, cell.1)
                .unwrap()
                .bridge_facts
                .raw_flags = flags as u32;
        }
        let attachment = if input["tagged"] == 1 {
            format!(
                "[CellTags]\n{}=AIR_TAG\n",
                u32::from(cell.1) * 1000 + u32::from(cell.0)
            )
        } else {
            String::new()
        };
        install_event_ingress_map(
            &mut sim,
            &rules,
            &format!(
                "[Triggers]\nAIR=Country0,<none>,Airborne,0,1,1,1,0\n\
             [Events]\nAIR=1,59,0,-1\n[Actions]\nAIR=1,28,0,7,0,0,0,0,A\n\
             [Tags]\nAIR_TAG=2,Airborne,AIR\n{attachment}"
            ),
        );
        sim.session
            .set_binary_frame_for_test(input["frame"].as_i64().unwrap() as u32);
        let before = (
            sim.scenario_rng.logical_state(),
            sim.main_rng.logical_state(),
            sim.mapgen_rng.logical_state(),
        );
        sim.dispatch_airborne_cell_tag_event(id, UninitContext::new(&rules, None));
        let raised = row["raised"].as_bool().unwrap();
        assert_eq!(
            !sim.tag_dispatch_trace_for_test().is_empty(),
            raised,
            "{}",
            input["name"]
        );
        assert_eq!(
            sim.trigger_runtime.globals_set.contains(&7),
            raised,
            "{}",
            input["name"]
        );
        assert!(
            sim.tag_dispatch_trace_for_test()
                .iter()
                .all(|(_, kind)| *kind == 59)
        );
        assert_eq!(
            (
                sim.scenario_rng.logical_state(),
                sim.main_rng.logical_state(),
                sim.mapgen_rng.logical_state()
            ),
            before
        );
    }
}

#[test]
fn periodic_cell_histories_share_the_native_lifecycle_and_saved_authority() {
    assert_eq!(
        super::native_tests::replay_native_histories(
            corpus()["tag_histories"]
                .as_array()
                .unwrap()
                .iter()
                .cloned()
        ),
        8,
    );
}

#[test]
fn ground_foot_and_buildings_do_not_enter_the_periodic_flight_path() {
    // Foot4DA7C7's ordinary getter rejects grounded owners. Buildings do not
    // execute FootAI even though they share Rust's live-object turn host.
    for (category, z) in [
        (EntityCategory::Unit, 0),
        (EntityCategory::Infantry, 0),
        (EntityCategory::Aircraft, 0),
        (EntityCategory::Structure, 500),
    ] {
        let (mut sim, rules, _, houses) = event_ingress_fixture();
        let id = sim.substrate.allocate_stable_id();
        let mut actor =
            GameEntity::test_default_of_category(id, "TAGUNIT", "House0", 15, 15, category);
        actor.set_owner_for_test(houses[0]);
        actor.lifecycle.set_cell_marked_for_test(true);
        actor.position.set_exact_z_leptons_for_test(Some(z));
        sim.substrate.entities.insert(actor);
        install_event_ingress_map(
            &mut sim,
            &rules,
            "[Triggers]\nAIR=Country0,<none>,Air,0,1,1,1,0\n[Events]\nAIR=1,59,0,-1\n\
             [Actions]\nAIR=1,28,0,7,0,0,0,0,A\n[Tags]\nAIR_TAG=2,Air,AIR\n[CellTags]\n15015=AIR_TAG\n",
        );
        sim.session.set_binary_frame_for_test(16);
        sim.dispatch_airborne_cell_tag_event(id, UninitContext::new(&rules, None));
        assert!(sim.tag_dispatch_trace_for_test().is_empty());
        assert!(!sim.global_variables()[7]);
    }
}
