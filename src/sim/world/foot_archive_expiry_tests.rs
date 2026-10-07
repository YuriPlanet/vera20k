//! Full original concrete expiry outputs, replayed at the represented receiver
//! seam. Native/producers excluded: spatial_oracle/foot_archive_expiry.md.

use super::*;
use crate::sim::components::Health;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionType};
use crate::sim::vision::FogState;

#[test]
fn native_foot_archive_expiry_matches_all_22_original_class_rows() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_archive_expiry.json",
    ))
    .unwrap();
    assert_eq!(corpus["schema_version"], 1);
    let rows = corpus["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 22);
    for row in rows {
        let input = &row["input"];
        let category = match input["family"].as_str().unwrap() {
            "unit" => EntityCategory::Unit,
            "infantry" => EntityCategory::Infantry,
            "aircraft" => EntityCategory::Aircraft,
            other => panic!("family {other}"),
        };
        let mut sim = Simulation::new();
        sim.session.binary_frame = 1000;
        sim.fog = FogState {
            width: 8,
            height: 8,
            ..FogState::default()
        };
        let owner = sim.interner.intern("Receiver");
        let other_owner = sim.interner.intern("Enemy");
        let type_ref = sim.interner.intern("TEST");
        let mut listener = GameEntity::new_at_frame_zero_for_test(
            1,
            0,
            0,
            0,
            0,
            owner,
            Health { current: 100 },
            type_ref,
            category,
            0,
            0,
            category != EntityCategory::Infantry,
        );
        listener.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_known(MissionType::Rescue),
            suspended: MissionId::NONE,
            queued: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 1000,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(1000),
        });
        listener.set_archive_target(Some(TargetKind::Entity(
            if input["archive_match"].as_bool().unwrap() {
                2
            } else {
                3
            },
        )));
        if input["target_match"].as_bool().unwrap() {
            listener.attack_target = Some(crate::sim::combat::AttackTarget::new(2));
        }
        sim.substrate.entities.insert(listener);
        if input["sensor"].as_i64().unwrap() != 0 {
            sim.fog.increment_sensor_at(owner, 5, 5);
        }
        let rng_before = sim.scenario_rng.logical_state();
        sim.notify_entity_pointer_expired(
            1,
            2,
            (input["expired_flags"].as_i64().unwrap() & 1 != 0).then_some((5, 5)),
            true,
            100,
            false,
            Some(if input["same_owner"].as_bool().unwrap() {
                owner
            } else {
                other_owner
            }),
            if input["control"] == 0 {
                PointerExpiryControl::DetachAll
            } else {
                PointerExpiryControl::Uninit
            },
            None,
            None,
        );
        let listener = sim.substrate.entities.get(1).unwrap();
        let archive = if row["archive"] == 0 {
            None
        } else {
            Some(TargetKind::Entity(3))
        };
        let target = (row["target"] != 0).then_some(TargetKind::Entity(2));
        assert_eq!(listener.archive_target(), archive, "{input}");
        assert_eq!(
            listener.attack_target.as_ref().map(|attack| attack.target),
            target,
            "{input}"
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            rng_before,
            "zero passive duration: {input}"
        );
    }
}
