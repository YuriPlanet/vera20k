use super::*;
use crate::sim::house_state::HouseState;
use crate::sim::mission::MissionId;

#[test]
fn deploy_resolves_actor_at_dispatch_and_does_not_recheck_issuer_ownership() {
    let rules = super::tests::amcv_move_rules();
    let mut sim = Simulation::new();
    crate::sim::arena_fixture::supply_native_map(&mut sim);
    let actor_owner = sim.interner.intern("Americans");
    let issuer = sim.interner.intern("British");
    for (index, owner) in [actor_owner, issuer].into_iter().enumerate() {
        sim.houses
            .register(HouseState::new(owner, index as i32, None, true, 0, 10));
    }
    let record = sim.encode_deploy_record(issuer, 42).unwrap();
    assert_eq!(DeployRecord::decode(&record).unwrap().house_id, 1);
    assert!(sim.decode_native_command_record(&record, 99).is_none());
    super::tests::spawn_rule_backed_unit(&mut sim, 42, "AMCV", &rules);
    let envelope = sim.decode_native_command_record(&record, 99).unwrap();
    assert_eq!(envelope.owner, issuer);
    assert!(sim.apply_command("British", &envelope.payload, Some(&rules)));
    assert_eq!(
        sim.entities().get(42).unwrap().mission.queued().known(),
        Some(MissionType::Unload)
    );
}

#[test]
fn retail_battle_encoder_uses_native_type_index_and_manual_place_object_kind() {
    let Some(retail) = crate::rules::retail_ini_fixture::retail_battle_rules() else {
        return;
    };
    let rules = &retail.rules;
    let mut sim = Simulation::new();
    sim.interner
        .intern("interner index is not a native array index");
    let owner = sim.interner.intern("Americans");
    sim.houses
        .register(HouseState::new(owner, 0, None, true, 5000, 10));
    let type_id = sim.interner.intern("GAPOWR");
    let native_index = rules
        .type_array_index(
            crate::rules::object_type::ObjectCategory::Building,
            "GAPOWR",
        )
        .unwrap();
    for event in [
        ProductionEvent::Produce,
        ProductionEvent::Suspend,
        ProductionEvent::Abandon,
        ProductionEvent::AbandonAll,
    ] {
        let record = sim
            .encode_production_record(owner, event, type_id, rules)
            .unwrap();
        let view = ProductionRecord::decode(&record).unwrap();
        assert_eq!(
            (view.rtti, view.type_index, view.naval),
            (7, native_index, 0)
        );
        assert_eq!(view.event, event);
        assert!(
            matches!(sim.decode_native_command_record(&record, 0).unwrap().payload, Command::SynchronizedRecord { record: retained } if retained == record)
        );
    }
    let record = sim
        .encode_place_building_record(owner, type_id, (u16::MAX, 17), rules)
        .unwrap();
    let view = PlaceRecord::decode(&record).unwrap();
    assert_eq!(
        (view.rtti, view.type_index, view.naval, view.x, view.y),
        (6, native_index, 0, -1, 17)
    );
}

#[test]
fn gi_event09_queues_unload_and_reaches_the_existing_infantry_handler() {
    // Reuse the original GI Unload control and its existing action/sequence
    // inputs. This tests the newly connected Event09 prefix, not new goldens.
    let rows: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/infantry_mission_unload.json",
    ))
    .unwrap();
    let (mut sim, rules, id) = crate::sim::deploy_tests::native_deploy_fixture(&rows[0]);
    assert_eq!(
        sim.interner
            .resolve(sim.entities().get(id).unwrap().type_ref()),
        "E1"
    );
    let owner = sim.interner.get("Americans").unwrap();
    let record = sim.encode_deploy_record(owner, id).unwrap();
    let command = sim.decode_native_command_record(&record, 0).unwrap();
    let before = sim.entities().get(id).unwrap().mission.current();
    assert!(sim.apply_command("Americans", &command.payload, Some(&rules)));
    assert_eq!(sim.entities().get(id).unwrap().mission.current(), before);
    assert_eq!(
        sim.entities().get(id).unwrap().mission.queued(),
        MissionId::from_known(MissionType::Unload)
    );
    sim.mission_host_promote(id, sim.session.binary_frame(), &rules);
    assert!(sim.register_live_object(id));
    sim.advance_tick(&[], Some(&rules), None, None, 66);
    assert_eq!(
        sim.entities()
            .get(id)
            .unwrap()
            .mission_leaf
            .as_infantry()
            .unwrap()
            .doing(),
        27
    );
}
