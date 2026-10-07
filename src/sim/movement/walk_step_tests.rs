use super::*;
use crate::map::entities::EntityCategory;
use crate::rules::locomotor_type::LocomotorKind;
use crate::sim::components::{DriveCoord, MovementTarget};
use crate::sim::movement::{FacingClass, locomotor::LocomotorState};

#[test]
fn paid_walk_matches_original_numeric_facing_and_boundary_vectors() {
    let rows: Vec<serde_json::Value> = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/walk_paid_step.json",
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
        entity.body_facing = body;
        entity.foot_speed.set_speed_fraction(SimFixed::lit("0.5"));
        entity.navigation.path_runtime.path_blocked = true;
        let speed = input["speed"].as_i64().unwrap() as i32;
        // The corpus stops before the same-cell SetCoords/SetHeight
        // (`0x0075C20F`, `0x0075C21C`), so it records the proposal's Z. Stand
        // the walker on flat ground at its own height so SetHeight(0) keeps it.
        let level = (current.z / 104) as u8;
        let terrain = crate::map::resolved_terrain::ResolvedTerrainGrid::from_cells(
            12,
            12,
            (0..12)
                .flat_map(|y| {
                    (0..12).map(move |x| crate::map::resolved_terrain::ResolvedTerrainCell {
                        level,
                        ..crate::map::resolved_terrain::test_flat_cell(x, y)
                    })
                })
                .collect(),
        );
        advance(
            &mut entity,
            SimFixed::from_num(speed * 15),
            None,
            100,
            Some(&terrain),
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
        let heading = entity.body_facing.current(100);
        assert_eq!(u64::from(heading), row["facing"].as_u64().unwrap());
        assert_eq!(entity.foot_speed.applied_fraction(), SIM_ONE);
        assert!(!entity.navigation.path_runtime.path_blocked);
    }
}

#[test]
fn idle_walk_scold_tails_match_original_through_ordinary_process() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_scold_latch.json",
    ))
    .unwrap();
    let mut checked = 0;
    for row in corpus["paid_tails"].as_array().unwrap() {
        let initial_speed = match row["case"].as_str().unwrap() {
            "no_head_speed_zero" => SIM_ZERO,
            "no_head_speed_positive" => SimFixed::lit("0.75"),
            _ => continue,
        };
        let mut sim = crate::sim::world::Simulation::new();
        sim.interner = crate::sim::intern::test_interner();
        let mut actor = GameEntity::test_default(1, "E1", "Americans", 9, 10);
        actor.category = EntityCategory::Infantry;
        actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        if let crate::sim::movement::locomotion::LocomotorRuntimePayload::Walk(state) =
            &mut actor.locomotor.as_mut().unwrap().runtime_payload
        {
            // Supplied retained byte at the native tail boundary.
            state.animation_moving = true;
        }
        actor.foot_speed.set_speed_fraction(initial_speed);
        actor
            .navigation
            .path_runtime
            .set_scold_latch_for_test(row["supplied_byte"].as_u64().unwrap() as u8);
        sim.substrate.entities.insert(actor);

        // No movement adapter is needed for this native Process corridor.
        sim.process_ground_locomotor_for_test(1, None, None, None)
            .unwrap();
        let actor = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            u64::from(actor.navigation.path_runtime.scold_latch_raw()),
            row["final_byte"].as_u64().unwrap(),
            "{row}"
        );
        assert_eq!(
            actor.foot_speed.applied_fraction(),
            SimFixed::from_num(row["speed_fraction"].as_f64().unwrap()),
            "{row}"
        );
        assert_eq!(
            actor.locomotor.as_ref().unwrap().walk_animation_moving(),
            Some(row["motion"] == 1),
            "{row}"
        );
        checked += 1;
    }
    assert_eq!(checked, 6);
}

#[test]
fn same_cell_paid_walk_scold_clear_matches_original_commit_tail() {
    let corpus: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/spatial_oracle/foot_scold_latch.json",
    ))
    .unwrap();
    let mut checked = 0;
    for row in corpus["paid_tails"].as_array().unwrap() {
        if row["case"] != "same_cell_commit" {
            continue;
        }
        let mut actor = GameEntity::test_default(1, "E1", "Americans", 9, 10);
        actor.category = EntityCategory::Infantry;
        actor.position.sub_x = SimFixed::from_num(186);
        actor.position.sub_y = SimFixed::from_num(64);
        actor.position.exact_z_leptons = Some(104);
        actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        actor
            .locomotor
            .as_mut()
            .unwrap()
            .set_step_head(Some(DriveCoord {
                x: 2544,
                y: 2624,
                z: 104,
            }));
        actor
            .navigation
            .path_runtime
            .set_scold_latch_for_test(row["supplied_byte"].as_u64().unwrap() as u8);
        advance(&mut actor, SimFixed::from_num(90), None, 100, None, None);
        let position = crate::sim::movement::ground_pose::position_world_coord(&actor.position);
        assert_eq!((position.x / 256, position.y / 256), (9, 10));
        assert_eq!(
            u64::from(actor.navigation.path_runtime.scold_latch_raw()),
            row["final_byte"].as_u64().unwrap(),
            "{row}"
        );
        checked += 1;
    }
    assert_eq!(checked, 3);
}
