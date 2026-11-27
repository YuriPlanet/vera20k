use super::*;
use crate::app::presentation::frame::ObjectRecord;
use crate::sim::command::{Command, CommandEnvelope};

/// Original Fly4CF830, composed by Aircraft414613 and Foot4DAF6D.
#[test]
fn retail_harrier_move_bobs_only_its_body_at_the_committed_draw_frame() {
    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let mut sim = crate::sim::arena_fixture::rival_arena_with_size(&rules, 17, 128);
    let owner = sim.intern("Americans");
    sim.session_mut_for_test()
        .set_current_house_for_test(Some(owner));
    sim.session_mut_for_test()
        .set_game_mode(crate::sim::scenario_session::ScenarioGameMode::Skirmish);
    sim.set_playfield_bounds_for_test(Some(crate::sim::cell_rect::PlayfieldBounds {
        base: 64,
        off_fc: -128,
        off_100: -128,
        off_104: 256,
        off_108: 256,
    }));
    sim.set_playfield_size_height_for_test(Some(64));
    sim.set_input_delay_ticks(0);
    for (kind, house, cell) in [
        ("GAPOWR", "Americans", (40, 48)),
        ("NAPOWR", "Russians", (90, 80)),
    ] {
        sim.spawn_object(kind, house, cell.0, cell.1, 0, &rules)
            .unwrap();
    }
    let plane = sim
        .spawn_object("ORCA", "Americans", 50, 64, 0, &rules)
        .unwrap();
    let mut commands = vec![CommandEnvelope::new(
        owner,
        sim.session().tick(),
        Command::Move {
            entity_id: plane,
            target_rx: 100,
            target_ry: 64,
        },
    )];
    // Executed original Draw_Point values; the body adds each integer twice.
    let native_y = [0, 0, 1, 1, 1, 2, 1, 1, 1, 0, 0, 0, 0, 0, 0, -1, 0, 0, 0, 0];
    let mut saw_low_bob = false;
    let mut phases = [false; 3];
    for _ in 0..160 {
        let grid = sim.path_grid_snapshot();
        sim.advance_tick(&commands, Some(&rules), grid.as_deref(), None, 33);
        commands = sim.take_due_commands();
        let entity = sim
            .entities()
            .get(plane)
            .expect("the moving Harrier stays alive");
        let (taking_off, landing) = entity
            .locomotor
            .as_ref()
            .unwrap()
            .fly_phase_callbacks()
            .unwrap();
        let cells = sim
            .resolved_terrain()
            .map(crate::map::resolved_terrain::NativeCellQuery::isolated);
        let height =
            crate::sim::movement::air_movement::queried_flight_height(entity, cells.as_ref());
        // MainTick draws before Logic, then increments for the next display
        // (55D8F2 -> 55DC9E -> 55DE7E). The retained actor is already committed.
        let phase = sim.session().binary_frame() as usize % 20;
        let expected = if !taking_off && !landing && height > 0 {
            2 * native_y[phase]
        } else {
            0
        };
        let (x, y) = crate::render::locomotor_visual::screen_position(entity);
        let anchor = [x + 0.25, y - 0.5];
        let hash = sim.state_hash();
        // Both stable and posed body paths consume this point before bounds
        // and depth; shadows continue to use the physical screen position.
        assert_eq!(
            {
                let geometry = VehicleGeometry::read(
                    &ObjectRecord::of_test_sim(&sim, &rules, plane),
                    EntityDrawBand::Air,
                    sim.session().binary_frame(),
                    Some(&rules),
                    rules.object("ORCA"),
                    None,
                    || true,
                );
                geometry.body_point(anchor)
            },
            [anchor[0], anchor[1] + expected as f32],
            "phase{phase}, height{height}"
        );
        assert_eq!(
            crate::render::locomotor_visual::screen_position(entity),
            (x, y)
        );
        assert_eq!(
            sim.state_hash(),
            hash,
            "body placement leaves retained simulation state unchanged"
        );
        if expected != 0 {
            saw_low_bob |= height < 208;
            for (seen, wanted) in phases.iter_mut().zip([2, 5, 15]) {
                *seen |= phase == wanted;
            }
        }
        if saw_low_bob && phases.into_iter().all(|seen| seen) {
            return;
        }
    }
    panic!("ordinary Move must reach low-flight bob and the positive/negative native body phases");
}

#[test]
fn climbing_black_hawk_and_harrier_share_height_and_facing_with_their_ground_shadow() {
    use crate::sim::components::DriveCoord;
    use crate::sim::world::Simulation;

    let Some(rules) = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art() else {
        return;
    };
    let terrain = crate::map::resolved_terrain::test_flat_ground_grid(32);
    for (name, band) in [("SHAD", EntityDrawBand::Air), ("ORCA", EntityDrawBand::Top)] {
        let mut sim = Simulation::new();
        let id = sim
            .spawn_object(name, "Americans", 10, 10, 0, &rules)
            .unwrap();
        let mut motion = UnitMotion::default();
        let start = DriveCoord::cell(10, 10, 160);
        let end = DriveCoord {
            x: start.x + 96,
            y: start.y + 48,
            z: start.z + 96,
        };
        let object = rules.object(name);
        let record_at = |sim: &mut Simulation, coord: DriveCoord, turning: bool| {
            use crate::sim::movement::FacingClass;
            let object = sim.entity_mut_for_test(id).unwrap();
            object.position.put_location(coord);
            let facing = if turning { 0xf000 } else { 0x1000 };
            object.body_facing.snap(facing, 0);
            if object.category() == EntityCategory::Aircraft {
                object
                    .locomotor
                    .as_mut()
                    .unwrap()
                    .set_current_fly_speed_for_test(crate::util::fixed_math::SimFixed::from_num(4));
                let mut secondary = FacingClass::new(facing, 4);
                if turning {
                    secondary.set(0x2000, 0);
                }
                object.barrel_facing = Some(secondary);
            }
            ObjectRecord::of_test_in(object, Some(&terrain), &rules)
        };
        let shadow = |record: &ObjectRecord, draw: &VehicleGeometry, frame| {
            if record.category() == EntityCategory::Aircraft {
                let s = aircraft_draw_shadow(record, object, Some(&terrain), draw).unwrap();
                (s.anchor, s.facing)
            } else {
                let s = lifted_unit_shadow_pose(record, &terrain, draw, frame).unwrap();
                (s.anchor, s.facing)
            }
        };
        let previous = record_at(&mut sim, start, true);
        motion.begin_draw(2, Some(1.0), 1.5);
        let from = VehicleGeometry::read(
            &previous,
            band,
            2,
            Some(&rules),
            object,
            Some(&motion),
            || true,
        );
        motion.drawn(id, from.sample.unwrap());
        motion.commit_presented();

        let current = record_at(&mut sim, end, false);
        let to = VehicleGeometry::read(&current, band, 3, Some(&rules), object, None, || true);
        motion.begin_draw(3, Some(0.5), 1.5);
        let mid = VehicleGeometry::read(
            &current,
            band,
            3,
            Some(&rules),
            object,
            Some(&motion),
            || true,
        );
        let midpoint = |a: [f32; 2], b: [f32; 2]| [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
        assert_eq!(
            mid.xyz,
            [start.x + 48, start.y + 24, start.z + 48],
            "{name} XYZ"
        );
        assert_eq!(mid.depth_xyz, mid.xyz, "{name} Foot depth query");
        assert_eq!(
            mid.body_point(mid.pose.screen),
            midpoint(
                from.body_point(from.pose.screen),
                to.body_point(to.pose.screen)
            ),
            "{name} body"
        );
        assert_eq!(
            shadow(&current, &mid, 3).0,
            midpoint(shadow(&previous, &from, 2).0, shadow(&current, &to, 3).0),
            "{name} grounded stencil"
        );
        assert_eq!(
            (mid.pose.body, shadow(&current, &mid, 3).1),
            (0, 0),
            "{name} body and shadow turn through north"
        );
        if current.category() == EntityCategory::Aircraft {
            let Some(PoseTilt::FlyNormal { roll: banked, .. }) = from.tilt else {
                panic!("the banking Harrier must use its native Fly pose");
            };
            let Some(PoseTilt::FlyNormal { roll, .. }) = mid.tilt else {
                panic!("the straightening Harrier must retain its partial bank");
            };
            assert!(banked.abs() > 0.001 && (2.0 * roll - banked).abs() < 1e-6);
            assert!(mid.transient_tilt);
        }
        let row = |record: &ObjectRecord, draw: &VehicleGeometry| {
            ground_sort_row(record, draw.pose.screen[1]) + draw.lift_correction
        };
        assert_eq!(
            row(&current, &mid),
            (row(&previous, &from) + row(&current, &to)) * 0.5,
            "{name} ground row"
        );
    }
}
