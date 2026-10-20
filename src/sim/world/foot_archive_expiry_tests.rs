//! Full original concrete expiry outputs, replayed at the represented receiver
//! seam. Native/producers excluded: spatial_oracle/foot_archive_expiry.md.

use super::*;
use crate::sim::combat::Health;
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
        sim.session.set_binary_frame_for_test(1000);
        sim.fog = FogState::new(8, 8);
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
            Health::new(100),
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
            listener
                .attack_target
                .set_for_test(Some(crate::sim::combat::TargetKind::Entity(2)));
        }
        sim.substrate.entities.insert(listener);
        if input["expired_flags"].as_i64().unwrap() & 1 != 0 {
            // This native input is a real Techno. Supply its authoritative
            // store identity; an arbitrary Object ID no longer implies Techno.
            let mut expired = GameEntity::test_default(2, "TEST", "Enemy", 5, 5);
            expired.set_type_ref_for_test(type_ref);
            expired.set_owner_for_test(if input["same_owner"].as_bool().unwrap() {
                owner
            } else {
                other_owner
            });
            sim.substrate.entities.insert(expired);
        }
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
            crate::sim::world::FrameEffects::default(),
        );
        let listener = sim.substrate.entities.get(1).unwrap();
        let archive = if row["archive"] == 0 {
            None
        } else {
            Some(TargetKind::Entity(3))
        };
        let target = (row["target"] != 0).then_some(TargetKind::Entity(2));
        assert_eq!(listener.archive_target(), archive, "{input}");
        assert_eq!(listener.attack_target.target(), target, "{input}");
        assert_eq!(
            sim.scenario_rng.logical_state(),
            rng_before,
            "zero passive duration: {input}"
        );
    }
}
