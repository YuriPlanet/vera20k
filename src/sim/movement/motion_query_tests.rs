use super::*;
use crate::sim::components::{DriveCoord, MovementTarget, NavTargetRef};
use crate::sim::movement::locomotor::LocomotorState;
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

/// Supplied native states, shared by the leaf and complete cell-entry readers.
/// This fixture does not claim that every independent combination is produced
/// by a stock scenario. Production setters are tested in their own corpora.
pub(crate) fn apply_air_state(entity: &mut GameEntity, input: &Value) {
    let kind = match input["family"].as_str().unwrap() {
        "fly" => LocomotorKind::Fly,
        "jumpjet" => LocomotorKind::Jumpjet,
        _ => unreachable!(),
    };
    let mut loco = LocomotorState::for_test_kind(kind);
    let v = &input["destination"];
    let destination = DriveCoord {
        x: v[0].as_i64().unwrap() as i32,
        y: v[1].as_i64().unwrap() as i32,
        z: v[2].as_i64().unwrap() as i32,
    };
    let moving = input["moving"].as_bool().unwrap();
    if kind == LocomotorKind::Fly {
        let state = loco.fly_runtime_mut().unwrap();
        state.retain_destination(destination, None, || unreachable!());
        if !moving {
            state.finish_destination();
        }
        // Distance60/slowdown100 projects exactly the supplied angle.
        entity.flight_attitude.approach(
            60,
            100,
            SimFixed::from_num(input["pitch"].as_f64().unwrap()),
        );
    } else {
        let state = loco.jumpjet_runtime_mut().unwrap();
        state.destination = destination;
        state.moving = moving;
        state.phase = input["phase"].as_i64().unwrap() as i32;
    }
    entity.locomotor = Some(loco);
}

#[test]
fn air_motion_matches_original_queries_independently_of_orders_and_restore() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/air_locomotor_moving.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 44);
    for row in rows {
        let mut entity = GameEntity::test_default(1, "MOVER", "Americans", 11, 10);
        apply_air_state(&mut entity, &row["input"]);
        let expected = row["moving"].as_bool().unwrap();
        for has_order in [false, true] {
            for powered in [false, true] {
                entity.movement_target = has_order.then(MovementTarget::default);
                entity.navigation.nav_com = has_order.then(|| NavTargetRef::cell(15, 15));
                entity.locomotor.as_mut().unwrap().powered = powered;
                assert_eq!(
                    is_moving(&entity),
                    Some(expected),
                    "{row}; order={has_order}"
                );
                let restored: GameEntity =
                    serde_json::from_value(serde_json::to_value(&entity).unwrap()).unwrap();
                assert_eq!(is_moving(&restored), Some(expected), "restored {row}");
            }
        }
    }
}
