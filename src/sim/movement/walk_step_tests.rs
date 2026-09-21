use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{DriveCoord, MovementTarget};
use crate::sim::movement::{FacingClass, locomotor::LocomotorState};

#[test]
fn paid_walk_matches_original_numeric_facing_and_boundary_vectors() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/walk_paid_step.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 40);
    for row in rows {
        let input = &row["input"];
        let coord = |name: &str| DriveCoord {
            x: input[name][0].as_i64().unwrap() as i32,
            y: input[name][1].as_i64().unwrap() as i32,
            z: input[name][2].as_i64().unwrap() as i32,
        };
        let current = coord("current");
        let mut entity = GameEntity::test_default(
            1,
            "E1",
            "Americans",
            (current.x / 256) as u16,
            (current.y / 256) as u16,
        );
        entity.category = EntityCategory::Infantry;
        entity.position.sub_x = SimFixed::from_num(current.x % 256);
        entity.position.sub_y = SimFixed::from_num(current.y % 256);
        entity.position.exact_z_leptons = Some(current.z);
        let mut loco = LocomotorState::for_test_kind(LocomotorKind::Walk);
        loco.set_step_head(Some(coord("head")));
        entity.locomotor = Some(loco);
        entity.movement_target = Some(MovementTarget::default());
        let mut body = FacingClass::new(
            input["initial_previous"].as_u64().unwrap() as u16,
            input["rot"].as_u64().unwrap() as u8,
        );
        if input["initial_duration"] != 0 {
            body.set(input["initial_facing"].as_u64().unwrap() as u16, 100);
        }
        entity.body_facing = Some(body);
        entity.facing = (body.current(100) >> 8) as u8;
        entity.foot_speed.applied_fraction = SimFixed::lit("0.5");
        entity.navigation.path_runtime.path_blocked = true;
        let speed = input["speed"].as_i64().unwrap() as i32;
        advance(
            &mut entity,
            SimFixed::from_num(speed * 15),
            None,
            100,
            None,
            None,
        );
        let proposed = crate::sim::movement::ground_pose::position_world_coord(&entity.position);
        let expected = &row["proposed"];
        assert_eq!(
            [proposed.x, proposed.y, proposed.z],
            [
                expected[0].as_i64().unwrap() as i32,
                expected[1].as_i64().unwrap() as i32,
                expected[2].as_i64().unwrap() as i32
            ],
            "{}",
            input["name"]
        );
        let crosses = (proposed.x / 256, proposed.y / 256) != (current.x / 256, current.y / 256);
        assert_eq!(crosses, row["crosses_cell"].as_bool().unwrap());
        let heading = entity.body_facing.unwrap().current(100);
        assert_eq!(u64::from(heading), row["facing"].as_u64().unwrap());
        assert_eq!(entity.facing, (heading >> 8) as u8);
        assert_eq!(entity.foot_speed.applied_fraction, SIM_ONE);
        assert_eq!(entity.foot_speed.cached_current_speed, speed);
        assert!(!entity.navigation.path_runtime.path_blocked);
    }
}
