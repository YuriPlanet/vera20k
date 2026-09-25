//! Compare the production post-PerCell owner against original Walk completion.
//! The native corpus starts after PerCell; the separate bridge/world tests cover
//! its real completed-head caller and callback publication.

use crate::map::entities::EntityCategory;
use crate::rules::{ini_parser::IniFile, locomotor_type::LocomotorKind, ruleset::RuleSet};
use crate::sim::components::{DriveCoord, FootPathQueue, NavTargetRef};
use crate::sim::game_entity::GameEntity;
use crate::sim::house_state::HouseState;
use crate::sim::mission::state::MissionTestFixture;
use crate::sim::mission::{MissionDispatchTimer, MissionId, MissionLeafState};
use crate::sim::movement::{locomotion::LocomotorRuntimePayload, locomotor::LocomotorState};
use crate::sim::timer::CdTimer;
use crate::sim::world::Simulation;
use crate::util::fixed_math::SimFixed;
use serde_json::Value;

#[test]
fn completed_corner_keeps_heading_until_next_head_is_accepted() {
    use crate::map::resolved_terrain::ResolvedTerrainGrid;
    use crate::sim::components::MovementTarget;
    use crate::sim::movement::{FacingClass, ground_pose, locomotor::MovementLayer, walk_head};
    use std::collections::BTreeMap;

    let rules = RuleSet::from_ini(&IniFile::from_str(
        "[InfantryTypes]\n0=E1\n[E1]\nStrength=125\nSpeed=4\n\
         Locomotor={4A582744-9839-11d1-B709-00A024DDAFD1}\n",
    ))
    .unwrap();
    let mut sim = Simulation::new();
    sim.install_resolved_terrain_for_new_map(ResolvedTerrainGrid::from_cells(
        16,
        16,
        (0..16)
            .flat_map(|y| {
                (0..16)
                    .map(move |x| crate::sim::world::common_raw_test_terrain_cell(x, y, 0, false))
            })
            .collect(),
    ));
    let id = sim
        .spawn_object("E1", "Americans", 6, 5, 0, &rules, &BTreeMap::new())
        .unwrap();
    let head = DriveCoord {
        x: 6 * 256 + 192,
        y: 5 * 256 + 64,
        z: 0,
    };
    let body = FacingClass::new(0x3FFF, 0);
    let actor = sim.substrate.entities.get_mut(id).unwrap();
    actor.position.sub_x = SimFixed::from_num(184);
    actor.position.sub_y = SimFixed::from_num(64);
    actor.body_facing = Some(body);
    actor.facing = 0x3F;
    actor.navigation.nav_com = Some(NavTargetRef::cell(6, 6));
    actor.navigation.path_replay = FootPathQueue {
        directions: vec![2, 4],
        cursor: 0,
        reference_cell: Some((5, 5)),
    };
    actor.movement_target = Some(MovementTarget {
        path: vec![(5, 5), (6, 5), (6, 6)],
        path_layers: vec![MovementLayer::Ground; 3],
        next_index: 1,
        final_goal: Some((6, 6)),
        ..Default::default()
    });
    let loco = actor.locomotor.as_mut().unwrap();
    loco.set_walk_destination(Some(DriveCoord::cell(6, 6, 0)));
    loco.set_step_head(Some(head));

    // Exercise real world completion, including Mark/PerCell and navigation.
    // Original75BD70..75BF82 has no movement-turn call;75BC97 owns the next one.
    sim.run_completed_walk_step(id, head, Some(&rules), None, None)
        .unwrap();
    let actor = sim.substrate.entities.get(id).unwrap();
    assert_eq!(ground_pose::position_world_coord(&actor.position), head);
    assert_eq!(actor.movement_target.as_ref().unwrap().next_index, 2);
    assert_eq!(actor.navigation.path_replay.remaining_directions(), &[4]);
    assert_eq!(actor.locomotor.as_ref().unwrap().step_head(), None);
    assert_eq!(actor.body_facing, Some(body));
    assert_eq!(
        actor.facing, 0x3F,
        "completion must not anticipate the corner"
    );

    // A refused head can keep the actor waiting; it must keep both headings.
    for blocked in [true, false] {
        if blocked {
            sim.substrate.raw_cell_occupation.mark_ground(6, 6, 0x20);
        } else {
            sim.substrate.raw_cell_occupation.clear_ground(6, 6, 0x20);
        }
        let accepted = walk_head::prepare_step_head(
            &mut sim.substrate.entities,
            id,
            &sim.substrate.occupancy,
            &mut sim.substrate.raw_cell_occupation,
            sim.resolved_terrain.as_ref(),
            None,
            Some(&rules),
            &sim.interner,
            &mut sim.scenario_rng,
        );
        assert_eq!(accepted, !blocked);
        let actor = sim.substrate.entities.get_mut(id).unwrap();
        assert_eq!(actor.body_facing, Some(body));
        assert_eq!(actor.facing, 0x3F);
        if accepted {
            let next = actor.locomotor.as_ref().unwrap().step_head().unwrap();
            let desired = crate::util::direction_tables::facing16_from_delta(
                next.x - head.x,
                next.y - head.y,
            );
            assert!(walk_head::finish_fresh_head(actor, 101));
            assert_ne!(desired, body.current(101));
            assert_eq!(actor.body_facing.unwrap().current(101), desired);
            assert_eq!(actor.facing, (desired >> 8) as u8);
        }
    }
}

fn i32_value(value: &Value) -> i32 {
    i32::try_from(value.as_i64().unwrap()).unwrap()
}

fn coord(value: &Value) -> DriveCoord {
    DriveCoord {
        x: i32_value(&value[0]),
        y: i32_value(&value[1]),
        z: i32_value(&value[2]),
    }
}

fn retained(value: &Value) -> Option<DriveCoord> {
    let coord = coord(value);
    (coord.x != 0 || coord.y != 0 || coord.z != 0).then_some(coord)
}

#[test]
fn post_percell_completion_matches_original_setter_refusal_and_stop_order() {
    let rows: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tools/spatial_oracle/walk_completion.json"
    ))
    .unwrap();
    assert_eq!(rows.len(), 42);
    for row in rows {
        let input = &row["input"];
        let mut sim = Simulation::new();
        let mut rules = RuleSet::from_ini(&IniFile::from_str(
            "[InfantryTypes]\n0=E1\n[E1]\nStrength=100\n",
        ))
        .unwrap();
        rules.general.blockage_path_delay_ticks = i32_value(&input["blockage"]);
        sim.session.binary_frame = input["frame"].as_u64().unwrap() as u32;
        let mut actor = GameEntity::test_default(1, "E1", "Americans", 16, 15);
        actor.category = EntityCategory::Infantry;
        actor.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Walk));
        actor.lifecycle.object_alive = input["alive"] == 1;
        actor.lifecycle.in_limbo = input["limbo"] == 1;
        actor.object_is_falling_down = input["falling"].as_u64().unwrap() as u8;
        // Keep signed/aliased full XYZ in the existing physical coordinate owner.
        // The query must compare the native truncated low16 cell words.
        let current = coord(&input["current"]);
        actor.position.rx = (current.x / 256).clamp(0, i32::from(u16::MAX)) as u16;
        actor.position.ry = (current.y / 256).clamp(0, i32::from(u16::MAX)) as u16;
        actor.position.sub_x = SimFixed::from_num(current.x - i32::from(actor.position.rx) * 256);
        actor.position.sub_y = SimFixed::from_num(current.y - i32::from(actor.position.ry) * 256);
        actor.position.exact_z_leptons = Some(current.z);
        assert_eq!(
            crate::sim::movement::ground_pose::position_world_coord(&actor.position),
            current
        );
        actor.mission.apply_test_fixture(MissionTestFixture {
            current: MissionId::from_raw(i32_value(&input["mission"])),
            queued: MissionId::from_raw(i32_value(&input["queued_mission"])),
            suspended: MissionId::NONE,
            movement_bypass_latch: 0,
            handler_state: 0,
            mission_start_frame: 0,
            ai_counter: 0,
            dispatch_timer: MissionDispatchTimer::at_frame(0),
        });
        actor.mission_leaf = MissionLeafState::infantry_raw_for_test(0, i32_value(&input["doing"]));
        if input["contact"] == 1 {
            actor.radio_contacts.insert(2);
        }
        let loco = actor.locomotor.as_mut().unwrap();
        loco.set_walk_destination(Some(coord(&input["destination"])));
        // The corpus enters after PerCell with supplied class bytes still set,
        // including the null-destination contrast and replacement-head rows.
        if let LocomotorRuntimePayload::Walk(state) = &mut loco.runtime_payload {
            state.head = retained(&input["head"]);
            state.moving = true;
            state.animation_moving = true;
        }
        actor.navigation.nav_com = Some(NavTargetRef::Entity { id: 2 });
        actor.navigation.nav_com_aux = Some(NavTargetRef::Entity { id: 123 });
        actor.navigation.nav_queue = vec![NavTargetRef::cell(20, 15), NavTargetRef::cell(21, 15)];
        actor.navigation.path_replay = FootPathQueue {
            directions: vec![2, 3, 4, 5],
            cursor: 0,
            reference_cell: Some((9, 8)),
        };
        actor.navigation.path_runtime.start_movement(50, 5);
        actor.navigation.path_runtime.start_blocked(40, 6);
        actor.navigation.path_runtime.retries_left = input["retries"].as_u64().unwrap() as u32;
        actor.foot_speed.applied_fraction = SimFixed::lit("0.75");
        let owner = actor.owner();
        sim.houses.insert(
            owner,
            HouseState::new(owner, 0, None, input["human"] == 1, 0, 0),
        );
        sim.substrate.entities.insert(actor);

        sim.finish_walk_navigation(1, Some(&rules)).unwrap();

        let actor = sim.substrate.entities.get(1).unwrap();
        let loco = actor.locomotor.as_ref().unwrap();
        assert_eq!(
            loco.walk_destination(),
            retained(&row["destination"]),
            "{input}"
        );
        assert_eq!(loco.step_head(), retained(&row["head"]), "{input}");
        assert_eq!(loco.walk_is_moving(), Some(row["moving"] == 1), "{input}");
        assert_eq!(
            loco.walk_animation_moving(),
            Some(row["motion"] == 1),
            "{input}"
        );
        assert_eq!(
            actor.navigation.nav_com.is_some(),
            row["nav"].as_bool().unwrap(),
            "{input}"
        );
        assert_eq!(
            actor.navigation.nav_com_aux.is_some(),
            row["aux"] != 0,
            "{input}"
        );
        let expected_queue = row["queue"]
            .as_array()
            .unwrap()
            .iter()
            .take_while(|value| **value != -1)
            .map(|value| value.as_u64().unwrap() as u8)
            .collect::<Vec<_>>();
        assert_eq!(
            actor.navigation.path_replay.remaining_directions(),
            expected_queue,
            "{input}"
        );
        assert_eq!(
            actor.navigation.nav_queue.len() as u64,
            row["nav_queue_count"].as_u64().unwrap(),
            "{input}"
        );
        for (timer, expected) in [
            (
                actor.navigation.path_runtime.movement_timer,
                &row["movement_timer"],
            ),
            (
                actor.navigation.path_runtime.blocked_timer,
                &row["blocked_timer"],
            ),
        ] {
            // CdTimer retains native start/duration; the unused middle word
            // is observed in the corpus but has no Rust representation.
            assert_eq!(
                timer,
                CdTimer::from_raw(i32_value(&expected[0]), i32_value(&expected[2])),
                "{input}"
            );
        }
        assert_eq!(
            actor.navigation.path_runtime.retries_left as u64,
            row["retries"].as_u64().unwrap(),
            "{input}"
        );
        assert_eq!(
            actor.foot_speed.applied_fraction,
            SimFixed::from_num(row["speed"].as_f64().unwrap()),
            "{input}"
        );
    }
}
