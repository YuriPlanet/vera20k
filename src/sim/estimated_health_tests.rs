use super::EstimatedHealth;
use crate::map::entities::EntityCategory;
use crate::sim::game_entity::GameEntity;
use crate::sim::world::Simulation;

#[test]
fn recovery_matches_original_techno_ai_instructions() {
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../tools/spatial_oracle/estimated_health.json"
    ))
    .unwrap();
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 546);
    for row in rows {
        let mut estimated = EstimatedHealth::from_raw(row["estimated"].as_i64().unwrap() as i32);
        estimated.recover(
            row["actual"].as_i64().unwrap() as i32,
            row["frame"].as_u64().unwrap() as u32,
        );
        assert_eq!(
            estimated.get(),
            row["result"].as_i64().unwrap() as i32,
            "{row}"
        );
    }
}

#[test]
fn actual_object_ai_recovers_each_category_on_binary_frame_mask() {
    for category in [
        EntityCategory::Unit,
        EntityCategory::Infantry,
        EntityCategory::Aircraft,
        EntityCategory::Structure,
    ] {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "ESTIMATE", "Americans", 2, 2);
        entity.category = category;
        entity.lifecycle.in_limbo = false;
        entity.health.current = 100;
        entity.estimated_health = EstimatedHealth::from_raw(-100);
        sim.substrate.entities.insert(entity);
        sim.set_logic_order_for_test(vec![1]);
        sim.session.tick = 4;
        sim.session.binary_frame = 3;
        sim.object_ai_stage(None);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .estimated_health
                .get(),
            -100
        );
        sim.session.tick = 8;
        sim.session.binary_frame = 4;
        sim.object_ai_stage(None);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .estimated_health
                .get(),
            -29
        );
        sim.session.binary_frame = 5;
        sim.object_ai_stage(None);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .estimated_health
                .get(),
            -28
        );
    }
}

#[test]
fn reservations_are_initialized_and_hash_independently_of_actual_health() {
    let mut sim = Simulation::new();
    let entity = GameEntity::test_default(1, "ESTIMATE", "Americans", 2, 2);
    assert_eq!(
        entity.estimated_health.get(),
        i32::from(entity.health.current)
    );
    sim.substrate.entities.insert(entity);
    let baseline = sim.state_hash();
    sim.substrate.entities.get_mut(1).unwrap().estimated_health = EstimatedHealth::from_raw(-1);
    assert_ne!(baseline, sim.state_hash());
    let negative = sim.state_hash();
    sim.substrate.entities.get_mut(1).unwrap().estimated_health = EstimatedHealth::from_raw(-2);
    assert_ne!(negative, sim.state_hash());
}

#[test]
fn active_tube_bypasses_recovery_until_ordinary_ai_resumes() {
    use crate::map::tube_facts::TubeId;
    use crate::sim::components::DriveCoord;
    use crate::sim::movement::tube_movement::LowBridgeTubeMovementState;

    for category in [EntityCategory::Unit, EntityCategory::Infantry] {
        let mut sim = Simulation::new();
        let mut entity = GameEntity::test_default(1, "ESTIMATE", "Americans", 2, 2);
        entity.category = category;
        entity.lifecycle.in_limbo = false;
        entity.estimated_health = EstimatedHealth::from_raw(-100);
        entity.low_bridge_tube_state = Some(LowBridgeTubeMovementState {
            tube_id: TubeId(0),
            cursor: 1,
            target: DriveCoord {
                x: 384,
                y: 128,
                z: 17,
            },
        });
        sim.substrate.entities.insert(entity);
        sim.set_logic_order_for_test(vec![1]);
        sim.session.binary_frame = 4;
        sim.object_ai_stage(None);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .estimated_health
                .get(),
            -100
        );
        sim.substrate
            .entities
            .get_mut(1)
            .unwrap()
            .low_bridge_tube_state = None;
        sim.object_ai_stage(None);
        assert_eq!(
            sim.substrate
                .entities
                .get(1)
                .unwrap()
                .estimated_health
                .get(),
            -29
        );
    }
}

#[test]
fn save_restore_preserves_negative_reservations_and_ai_continuation() {
    use crate::sim::snapshot::GameSnapshot;

    let mut sim = Simulation::new();
    let id = sim.allocate_stable_id();
    let mut entity = GameEntity::test_default(id, "E1", "Americans", 5, 5);
    entity.category = EntityCategory::Infantry;
    entity.lifecycle.in_limbo = false;
    entity.estimated_health = EstimatedHealth::from_raw(-123);
    sim.substrate.entities.insert(entity);
    sim.set_logic_order_for_test(vec![id]);
    sim.add_entity_occupancy(id);
    sim.interner = crate::sim::intern::test_interner();
    sim.scenario_rng = crate::sim::rng::SimRng::new(0);
    sim.session.binary_frame = 3;
    let bytes = GameSnapshot::save(&sim, 0, 0, "estimated health", 0);
    let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
    restored.restore_after_snapshot_load().unwrap();
    assert_eq!(
        restored
            .substrate
            .entities
            .get(id)
            .unwrap()
            .estimated_health
            .get(),
        -123
    );
    assert_eq!(restored.state_hash(), sim.state_hash());
    for frame in [3, 4, 5, 8, 12] {
        sim.session.binary_frame = frame;
        restored.session.binary_frame = frame;
        sim.object_ai_stage(None);
        restored.object_ai_stage(None);
        assert_eq!(restored.state_hash(), sim.state_hash(), "frame {frame}");
    }
    assert_eq!(
        restored
            .substrate
            .entities
            .get(id)
            .unwrap()
            .estimated_health
            .get(),
        -27
    );
}

#[test]
fn common_ai_recovers_before_self_heal_without_mirroring_actual_health() {
    let ini = crate::rules::ini_parser::IniFile::from_str(
        "[General]\nRepairRate=1\n[InfantryTypes]\n0=E1\n[VehicleTypes]\n[AircraftTypes]\n[BuildingTypes]\n[E1]\nStrength=100\nSelfHealing=yes\n",
    );
    let rules = crate::rules::ruleset::RuleSet::from_ini(&ini).unwrap();
    let mut sim = Simulation::new();
    let mut entity = GameEntity::test_default(1, "E1", "Americans", 5, 5);
    entity.category = EntityCategory::Infantry;
    entity.lifecycle.in_limbo = false;
    entity.health.current = 10;
    entity.estimated_health = EstimatedHealth::from_raw(10);
    sim.interner = crate::sim::intern::test_interner();
    sim.substrate.entities.insert(entity);
    sim.set_logic_order_for_test(vec![1]);
    sim.session.binary_frame = 900;

    sim.object_ai_stage(Some(&rules));
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.health.current, 11);
    assert_eq!(entity.estimated_health.get(), 10);

    sim.session.binary_frame = 904;
    sim.object_ai_stage(Some(&rules));
    let entity = sim.substrate.entities.get(1).unwrap();
    assert_eq!(entity.health.current, 11);
    assert_eq!(
        entity.estimated_health.get(),
        10,
        "frame 904 has mask 0x4 clear"
    );

    sim.session.binary_frame = 908;
    sim.object_ai_stage(Some(&rules));
    assert_eq!(
        sim.substrate
            .entities
            .get(1)
            .unwrap()
            .estimated_health
            .get(),
        11
    );
}
