//! Frozen pre-optimization HashMap algorithm as a test oracle. Compare whole
//! endpoint payloads and samples, not merely world hashes or positions.
use super::super::{RenderInterpolation, TELEPORT_LEPTONS};
use super::*;
use std::collections::HashMap;

#[derive(Default)]
struct Before {
    rows: HashMap<u64, Endpoints>,
}

impl Before {
    fn capture(&mut self, input: &[(u64, RenderPose)], frame: u32, reset: bool) {
        for &(id, current) in input {
            // Keep the old expression independent of Endpoints::next.
            let previous = self
                .rows
                .get(&id)
                .filter(|old| {
                    !reset
                        && old.frame.wrapping_add(1) == frame
                        && old.current.type_id == current.type_id
                        && old.current.on_bridge == current.on_bridge
                        && old
                            .current
                            .location
                            .iter()
                            .zip(current.location)
                            .all(|(&a, b)| (i64::from(a) - i64::from(b)).abs() <= TELEPORT_LEPTONS)
                })
                .map_or(current, |old| old.current);
            self.rows.insert(
                id,
                Endpoints {
                    previous,
                    current,
                    frame,
                },
            );
        }
        self.rows.retain(|_, row| row.frame == frame);
    }

    fn sample(&self, id: u64, current: RenderPose, frame: u32, alpha: f32) -> RenderPose {
        self.rows
            .get(&id)
            .filter(|row| row.frame == frame && row.current == current)
            .map_or(current, |row| row.previous.between(current, alpha))
    }
}

fn pose(slot: u32, step: u32) -> RenderPose {
    use crate::sim::movement::slope_transition::SlopeRenderPhase::{Stable, Transition};
    let x = (slot * 31 + step * 8) as i32
        + if slot.is_multiple_of(7) && step.is_multiple_of(13) {
            2000
        } else {
            0
        };
    RenderPose {
        screen: [x as f32 * 0.125, slot as f32 * -0.25],
        location: [x, slot as i32 * 20, (step % 5) as i32 * 8],
        body: (step as u16).wrapping_mul(173),
        turret: (step as u16).wrapping_mul(251),
        barrel: (step as u16).wrapping_mul(47),
        fly_bank_direction: None,
        angles: [step as f32 * -0.125, slot as f32 * 0.001],
        rocket_pitch: step as f32 * 0.005,
        visual_lift: (step % 3) as f32,
        lift: (step % 5) as f32 * 0.5,
        recoil: ([step as f32 * 0.01, slot as f32 * -0.005], step % 2 == 1),
        slope: match step % 4 {
            0 => None,
            1 => Some(Stable(0)),
            _ => Some(Transition {
                from_slope: 0,
                to_slope: 3,
                phase_num: 1,
                phase_den: 3,
            }),
        },
        type_id: if step.is_multiple_of(17) { 2 } else { 1 },
        on_bridge: step.is_multiple_of(31),
    }
}

fn assert_pose_bits(a: RenderPose, b: RenderPose) {
    assert_eq!(a, b, "whole typed payload");
    let floats = |p: RenderPose| {
        [
            p.screen[0],
            p.screen[1],
            p.angles[0],
            p.angles[1],
            p.rocket_pitch,
            p.visual_lift,
            p.lift,
            p.recoil.0[0],
            p.recoil.0[1],
        ]
        .map(f32::to_bits)
    };
    assert_eq!(
        floats(a),
        floats(b),
        "exact float bits, including signed zero"
    );
}

#[test]
fn render_interpolation_dense_history_matches_old_map_through_churn_and_wrap() {
    let mut cache = RenderInterpolation::new(true);
    cache.held = false;
    let mut before = Before::default();
    let mut frame = u32::MAX - 5;
    for step in 0..96u32 {
        frame = frame.wrapping_add(if step.is_multiple_of(11) { 3 } else { 1 });
        if step.is_multiple_of(29) {
            cache.poses.clear();
            before.rows.clear();
        }
        let reset = step.is_multiple_of(23);
        let limit = if step.is_multiple_of(9) { 101 } else { 257 };
        let input: Vec<_> = (0..limit)
            .filter(|slot| (slot + step) % 19 != 0)
            .map(|slot| (1 + u64::from(slot) * 1_000_003, pose(slot, step)))
            .collect();
        cache.poses.capture(input.iter().copied(), frame, reset);
        before.capture(&input, frame, reset);
        assert_eq!(cache.poses.len(), before.rows.len());
        for &(id, current) in &input {
            let actual = cache.poses.get(&id).unwrap();
            let expected = before.rows.get(&id).unwrap();
            assert_eq!(actual.frame, expected.frame);
            assert_pose_bits(actual.previous, expected.previous);
            assert_pose_bits(actual.current, expected.current);
            for alpha in [0., 0.25, 0.5, 0.75, 1.] {
                cache.alpha = alpha;
                assert_pose_bits(
                    cache.sample(id, current, frame),
                    before.sample(id, current, frame, alpha),
                );
                // Out-of-band edits and a mismatched frame must still snap the
                // entire transform rather than borrowing any cached field.
                let changed = RenderPose {
                    body: current.body.wrapping_add(1),
                    ..current
                };
                assert_pose_bits(cache.sample(id, changed, frame), changed);
                assert_pose_bits(cache.sample(id, current, frame.wrapping_add(1)), current);
            }
        }
        for slot in 0..257 {
            let id = 1 + slot * 1_000_003;
            assert_eq!(cache.poses.contains_key(&id), before.rows.contains_key(&id));
        }
    }
    cache
        .poses
        .capture(std::iter::empty(), frame.wrapping_add(1), false);
    assert!(cache.poses.is_empty());
    assert!(cache.poses.spare_ids.is_empty() && cache.poses.spare_rows.is_empty());
}

#[test]
fn render_interpolation_dense_history_keeps_prefix_when_middle_membership_changes() {
    let mut cache = PoseHistory::default();
    let mut before = Before::default();
    // Separate tail truncation, tail append, middle insertion/deletion and a
    // new low ID, including very sparse IDs and the largest possible ID.
    for (step, ids) in [
        vec![10, 20, 30, 40, u64::MAX],
        vec![10, 20, 30],
        vec![10, 20, 30, 50],
        vec![10, 20, 25, 30, 50],
        vec![10, 20, 30, 50],
        vec![1, 10, 20, 30, 50],
        vec![],
        vec![u64::MAX],
    ]
    .into_iter()
    .enumerate()
    {
        let input: Vec<_> = ids
            .into_iter()
            .map(|id| (id, pose((id % 257) as u32, step as u32)))
            .collect();
        cache.capture(input.iter().copied(), step as u32, false);
        before.capture(&input, step as u32, false);
        assert_eq!(
            cache.ids,
            input.iter().map(|&(id, _)| id).collect::<Vec<_>>()
        );
        for &(id, _) in &input {
            assert_eq!(cache.get(&id), before.rows.get(&id));
        }
    }
}

#[test]
fn render_interpolation_offset_and_coordinate_fast_paths_match_previous_helpers() {
    use crate::sim::components::DriveCoord;
    use crate::sim::game_entity::GameEntity;
    use crate::sim::world::{RevealOutcome, Simulation};
    let mut sim = Simulation::new();
    sim.entities_mut_for_test()
        .insert(GameEntity::test_default(7, "LTNK", "Americans", 4, 4));
    *sim.interner_mut_for_test() = crate::sim::intern::test_interner();
    assert!(matches!(sim.reveal(7), RevealOutcome::Revealed { .. }));
    for enabled in [false, true] {
        let mut cache = RenderInterpolation::new(enabled);
        for step in 0..8 {
            sim.session_mut_for_test().set_binary_frame_for_test(step);
            sim.entities_mut_for_test()
                .get_mut(7)
                .unwrap()
                .position
                .put_location(DriveCoord {
                    x: 1200 + step as i32 * 32,
                    y: 1200,
                    z: step as i32 * 8,
                });
            for (elapsed, held, interval) in [
                (0, false, Some(32.)),
                (16, false, Some(32.)),
                (20, true, Some(32.)),
                (24, false, None),
            ] {
                cache.observe(&sim, step * 32 + elapsed, interval, held);
                let e = sim.entities().get(7).unwrap();
                let current = RenderPose::read(e, step);
                let sampled = cache.sample_entity(e, step);
                let expected = [
                    sampled.screen[0] - current.screen[0],
                    sampled.screen[1] - current.screen[1],
                    sampled.lift - current.lift,
                ];
                assert_eq!(
                    cache.entity_offset(e, step).map(f32::to_bits),
                    expected.map(f32::to_bits)
                );
                let coord = [1234, 5678, -104];
                let expected = cache
                    .poses
                    .get(&7)
                    .filter(|e| cache.readout().sampling && e.frame == step && e.current == current)
                    .map_or(coord, |e| {
                        std::array::from_fn(|i| {
                            (coord[i] as f64
                                + (f64::from(e.previous.location[i])
                                    - f64::from(e.current.location[i]))
                                    * f64::from(1. - cache.alpha))
                            .round() as i32
                        })
                    });
                assert_eq!(cache.entity_coord(e, step, coord), expected);
            }
        }
    }
}

#[test]
#[ignore = "manual release-only cache churn timing; not a whole-world/frame benchmark"]
fn render_interpolation_dense_history_churn_cpu_probe() {
    use std::hint::black_box;
    use std::time::Instant;
    assert!(!cfg!(debug_assertions), "run --release");
    // Compare the real history capture against the frozen old map on identical
    // prebuilt poses. Birth/death input construction and equality are untimed.
    // A new high ID replaces retired low IDs, just as monotonic stable IDs do.
    for churn in [0usize, 1, 200, 2000] {
        let mut old_means = Vec::new();
        let mut new_means = Vec::new();
        for pair in 0..8 {
            let mut old = Before::default();
            let mut new = PoseHistory::default();
            let mut old_ns = Vec::new();
            let mut new_ns = Vec::new();
            for step in 0..160u32 {
                let first = u64::from(step) * churn as u64 + 1;
                let input: Vec<_> = (0..20_000)
                    .map(|slot| (first + u64::from(slot), pose(slot % 257, step)))
                    .collect();
                let mut capture_old = || {
                    let start = Instant::now();
                    old.capture(black_box(&input), step, false);
                    start.elapsed().as_nanos() as u64
                };
                let mut capture_new = || {
                    let start = Instant::now();
                    new.capture(black_box(input.iter().copied()), step, false);
                    start.elapsed().as_nanos() as u64
                };
                let (a, b) = if (step as usize + pair) % 2 == 0 {
                    let a = capture_old();
                    let b = capture_new();
                    (a, b)
                } else {
                    let b = capture_new();
                    let a = capture_old();
                    (a, b)
                };
                assert_eq!(old.rows.len(), new.len());
                assert_eq!(new.slots.len(), new.len());
                for &(id, _) in &input {
                    assert_eq!(new.get(&id), old.rows.get(&id));
                }
                if step >= 32 {
                    old_ns.push(a);
                    new_ns.push(b);
                }
            }
            old_means.push(old_ns.iter().sum::<u64>() as f64 / old_ns.len() as f64 / 1e6);
            new_means.push(new_ns.iter().sum::<u64>() as f64 / new_ns.len() as f64 / 1e6);
        }
        let median = |values: &[f64]| {
            let mut values = values.to_vec();
            values.sort_by(f64::total_cmp);
            (values[3] + values[4]) * 0.5
        };
        println!(
            "INTERPOLATION_CHURN {}",
            serde_json::json!({
                "world": 20_000, "retired_and_born_per_commit": churn,
                "pairs": 8, "warmup_commits": 32, "measured_commits": 128,
                "before_capture_ms": median(&old_means), "after_capture_ms": median(&new_means),
                "before_run_means_ms": old_means, "after_run_means_ms": new_means,
                "all_endpoint_payloads_equal": true,
            })
        );
    }
}
