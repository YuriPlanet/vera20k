//! Runtime/save coverage for the single retained forced-track authority.
//! Admission/table parity is separately compared with the original binary in
//! movement/track_force_tests.rs; these tests exercise the production object turn.

use super::*;
use crate::rules::{ini_parser::IniFile, locomotor_type::LocomotorKind};
use crate::sim::components::{DriveCoord, DriveLocomotionRuntime};
use crate::sim::game_entity::GameEntity;
use crate::sim::movement::{drive_track, locomotor::LocomotorState};
use crate::sim::snapshot::GameSnapshot;
use crate::util::fixed_math::{SIM_ONE, SimFixed};

fn rules(speed: u32) -> RuleSet {
    RuleSet::from_ini(&IniFile::from_str(&format!(
        "[VehicleTypes]\n0=MTNK\n[MTNK]\nStrength=100\nSpeed={speed}\nAccelerates=no\nSensorsSight=1\nWalkRate=1\nIdleRate=0\n"
    ))).unwrap()
}

fn fixture(selector: i32) -> Simulation {
    // Native Scenario deserialization reseeds this RNG to zero. Using that
    // same initial seed isolates track persistence from the separate RNG rule.
    let mut sim = Simulation::with_seed(0);
    sim.fog.width = 32;
    sim.fog.height = 32;
    let turn = drive_track::turn_track_at(selector as usize).unwrap();
    let first = drive_track::raw_track_points(turn.normal_track)[0];
    let (dx, dy, facing) =
        drive_track::transform_track_point(first.x, first.y, first.facing, turn.flags);
    let head = DriveCoord {
        x: 10 * 256 + 128,
        y: 10 * 256 + 128,
        z: 0,
    };
    let x = head.x + i32::from(dx);
    let y = head.y + i32::from(dy);
    let mut entity =
        GameEntity::test_default(1, "MTNK", "Americans", (x / 256) as u16, (y / 256) as u16);
    entity.owner = sim.intern("Americans");
    entity.type_ref = sim.intern("MTNK");
    entity.position.sub_x = SimFixed::from_num(x % 256);
    entity.position.sub_y = SimFixed::from_num(y % 256);
    entity.position.exact_z_leptons = Some(0);
    entity.facing = facing;
    entity.drive_accelerates = false;
    entity.is_voxel = false;
    entity.locomotor = Some(LocomotorState::for_test_kind(LocomotorKind::Drive));
    entity.drive_locomotion = Some(DriveLocomotionRuntime::default());
    sim.substrate.entities.insert(entity);
    sim.substrate.next_stable_object_id = 2;
    assert!(matches!(
        sim.reveal(1),
        super::super::RevealOutcome::Revealed { .. }
    ));
    assert!(sim.force_drive_track(1, selector, head));
    sim.substrate
        .entities
        .get_mut(1)
        .unwrap()
        .foot_speed
        .applied_fraction = SIM_ONE;
    sim
}

fn tick(sim: &mut Simulation, rules: &RuleSet, frame: u32) -> movement::MovementTickStats {
    sim.session.tick = u64::from(frame);
    sim.session.binary_frame = frame;
    sim.advance_live_object_turn(1, Some(rules), techno_ai::ObjectAiCtx::default())
        .unwrap()
        .movement
}

#[test]
fn every_bunker_selector_restores_before_first_point_midcurve_and_paid_sentinel() {
    let rules = rules(1);
    for selector in 0x43..=0x47 {
        let mut sim = fixture(selector);
        let count = drive_track::raw_track_points(
            drive_track::turn_track_at(selector as usize)
                .unwrap()
                .normal_track,
        )
        .len() as i32;
        let mut observed = [false; 3];
        let mut restored_probes = Vec::new();
        let initial_rng = sim.scenario_rng.logical_state();
        for frame in 0..180 {
            let track = sim
                .substrate
                .entities
                .get(1)
                .unwrap()
                .drive_locomotion
                .as_ref()
                .unwrap()
                .track;
            if track.turn_index < 0 {
                break;
            }
            let stage = if track.cursor == 0 {
                0
            } else if track.cursor == count {
                2
            } else {
                1
            };
            // Exercise the real restore coordinator, including cell lists and
            // raw marks, at each of the three persisted execution boundaries.
            if !observed[stage] {
                observed[stage] = true;
                let bytes = GameSnapshot::save(&sim, 0, 0, "forced-track", 0);
                let mut restored = GameSnapshot::load(&bytes).unwrap().sim;
                restored.restore_after_snapshot_load().unwrap();
                assert_eq!(
                    restored.state_hash(),
                    sim.state_hash(),
                    "selector={selector} stage={stage}"
                );
                restored_probes.push((stage, restored));
            }
            let before_cursor = track.cursor;
            let stats = tick(&mut sim, &rules, frame);
            // Compare each restored branch with the original live world on
            // every subsequent visit through retirement. Reloading both sides
            // would hide state that serialization accidentally dropped.
            for (saved_stage, restored) in &mut restored_probes {
                tick(restored, &rules, frame);
                assert_eq!(
                    restored.state_hash(),
                    sim.state_hash(),
                    "selector={selector} saved_stage={saved_stage} frame={frame}"
                );
            }
            let entity = sim.substrate.entities.get(1).unwrap();
            assert!(entity.movement_target.is_none());
            assert!(stats.moved_steps <= 1, "Speed=1 cannot pay two points");
            if frame == 0 {
                assert_eq!(
                    entity.drive_locomotion.as_ref().unwrap().track.cursor,
                    before_cursor,
                    "first visit retains an unpaid cursor"
                );
            }
        }
        assert_eq!(observed, [true; 3], "selector={selector}");
        let entity = sim.substrate.entities.get(1).unwrap();
        assert_eq!(
            entity.drive_locomotion.as_ref().unwrap().track.turn_index,
            -1
        );
        assert_eq!(
            sim.scenario_rng.logical_state(),
            initial_rng,
            "track/AI fixture has no random receiver"
        );
    }
}

#[test]
fn forced_object_turn_queries_live_speed_and_advances_shp_once() {
    let mut sim = fixture(0x43);
    tick(&mut sim, &rules(1), 0);
    let entity = sim.substrate.entities.get(1).unwrap();
    let slow = entity.foot_speed.cached_current_speed;
    let before = entity.drive_locomotion.as_ref().unwrap().track.cursor;
    let before_body = entity.body_frame_counter;
    let outcome = tick(&mut sim, &rules(6), 1);
    let entity = sim.substrate.entities.get(1).unwrap();
    assert!(entity.foot_speed.cached_current_speed > slow);
    assert!(entity.drive_locomotion.as_ref().unwrap().track.cursor > before + 1);
    assert!(outcome.moved_steps > 1);
    assert_eq!(entity.body_frame_counter, before_body + 1);
    assert!(entity.movement_target.is_none());
}
