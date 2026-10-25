//! Projection equivalence and opt-in component timing. The ordinary paired
//! probe remains unchanged; these stages are separate, not addable timers.
use super::*;

/// Frozen first-optimization reader, independent of the combined helper.
fn read_before(entity: &GameEntity, frame: u32) -> RenderPose {
    let location = object_location(entity, None);
    let (x, y) = crate::render::locomotor_visual::screen_position(entity);
    let body = entity.body_facing_current(frame);
    RenderPose {
        screen: [x, y],
        body,
        barrel: entity.barrel_elevation().current(frame),
        fly_bank_direction: fly_bank_direction(entity, frame),
        angles: entity
            .rocking
            .as_ref()
            .map_or([0.; 2], |r| r.angles_native()),
        rocket_pitch: entity
            .locomotor
            .as_ref()
            .and_then(|l| l.rocket_runtime())
            .map_or(0., |r| r.draw_pitch()),
        visual_lift: f32::from(entity.ion_blast_lift_pixels()),
        lift: crate::render::locomotor_visual::screen_lift_px(entity) as f32,
        recoil: entity.voxel_recoil(),
        slope: crate::sim::movement::slope_transition::state_for_entity(entity)
            .map(|s| s.render_phase(frame.wrapping_sub(1))),
        turret: entity
            .barrel_facing
            .as_ref()
            .map_or(body, |f| f.current(frame)),
        location: [location.x, location.y, location.z],
        type_id: entity.type_ref().index(),
        on_bridge: entity.on_bridge(),
    }
}

fn assert_pose_bits(a: RenderPose, b: RenderPose) {
    assert_eq!(a, b);
    let bits = |p: RenderPose| {
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
    assert_eq!(bits(a), bits(b), "all float bits including signed zero");
}

#[test]
fn render_interpolation_combined_projection_matches_old_reader_at_signed_edges() {
    use crate::sim::components::Position;
    use crate::util::fixed_math::SimFixed;
    let mut checked = 0;
    for category in [
        EntityCategory::Unit,
        EntityCategory::Infantry,
        EntityCategory::Aircraft,
        EntityCategory::Structure,
    ] {
        let mut entity =
            GameEntity::test_default_of_category(7, "MTNK", "Americans", 4, 4, category);
        for (rx, ry) in [(0, 0), (4, 7), (u16::MAX, u16::MAX)] {
            for level in [0, 1, 127, 128, 255] {
                for z in [
                    None,
                    Some(i32::MIN),
                    Some(-1500),
                    Some(-104),
                    Some(-1),
                    Some(0),
                    Some(1),
                    Some(52),
                    Some(727),
                    Some(728),
                    Some(729),
                    Some(1500),
                    Some(i32::MAX),
                ] {
                    for sub in [-64, 128, 255] {
                        entity.position = Position::new(
                            rx,
                            ry,
                            level,
                            z,
                            SimFixed::from_num(sub),
                            SimFixed::from_num(255 - sub),
                        );
                        let expected = (
                            crate::render::locomotor_visual::screen_position(&entity),
                            crate::render::locomotor_visual::screen_lift_px(&entity),
                        );
                        let actual =
                            crate::render::locomotor_visual::screen_position_and_lift(&entity);
                        assert_eq!(actual.0.0.to_bits(), expected.0.0.to_bits());
                        assert_eq!(actual.0.1.to_bits(), expected.0.1.to_bits());
                        assert_eq!(actual.1, expected.1);
                        assert_pose_bits(RenderPose::read(&entity, 17), read_before(&entity, 17));
                        checked += 1;
                    }
                }
            }
        }
    }
    assert_eq!(checked, 2340);
}

#[test]
#[ignore = "manual release-only pose component timing; not a whole-game FPS result"]
fn render_interpolation_pose_read_stage_cpu_probe() {
    assert!(!cfg!(debug_assertions), "run --release");
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        .expect("retail INIs required");
    let names = [
        "location",
        "projection_before",
        "projection_combined",
        "pose_before",
        "pose_combined",
        "history_prebuilt",
        "observe_combined",
    ];
    for world in [1000, 20_000] {
        let mut round_means: [Vec<f64>; 7] = std::array::from_fn(|_| Vec::new());
        for round in 0..8 {
            let case = Case {
                name: "pose-stage",
                world,
                visible: 250,
                render_ms: 5,
                logic_ms: 32,
                bypass: false,
                smoke_systems: 0,
            };
            let mut f = fixture(&rules, case);
            let mut history = history::PoseHistory::default();
            let mut observed = RenderInterpolation::new(true);
            let mut times: [Vec<u64>; 7] = std::array::from_fn(|_| Vec::new());
            for step in 0..40 {
                commit(&mut f, step);
                let frame = f.sim.session().binary_frame();
                let prepared: Vec<_> = f
                    .sim
                    .entities()
                    .values()
                    .map(|e| (e.stable_id(), RenderPose::read(e, frame)))
                    .collect();
                let audit = step == 0 || step == 39;
                let hash = audit.then(|| f.sim.state_hash());
                for pass in 0..7 {
                    let stage = if (round + step) % 2 == 0 {
                        pass
                    } else {
                        6 - pass
                    };
                    let start = Instant::now();
                    match stage {
                        0 => {
                            for e in f.sim.entities().values() {
                                black_box(object_location(black_box(e), None));
                            }
                        }
                        1 => {
                            for e in f.sim.entities().values() {
                                black_box((
                                    crate::render::locomotor_visual::screen_position(black_box(e)),
                                    crate::render::locomotor_visual::screen_lift_px(black_box(e)),
                                ));
                            }
                        }
                        2 => {
                            for e in f.sim.entities().values() {
                                black_box(
                                    crate::render::locomotor_visual::screen_position_and_lift(
                                        black_box(e),
                                    ),
                                );
                            }
                        }
                        3 => {
                            for e in f.sim.entities().values() {
                                black_box(read_before(black_box(e), black_box(frame)));
                            }
                        }
                        4 => {
                            for e in f.sim.entities().values() {
                                black_box(RenderPose::read(black_box(e), black_box(frame)));
                            }
                        }
                        5 => history.capture(black_box(prepared.iter().copied()), frame, false),
                        6 => observed.observe(black_box(&f.sim), step * 32, Some(32.), false),
                        _ => unreachable!(),
                    }
                    let ns = start.elapsed().as_nanos() as u64;
                    if step >= 8 {
                        times[stage].push(ns);
                    }
                }
                if let Some(hash) = hash {
                    for e in f.sim.entities().values() {
                        assert_pose_bits(RenderPose::read(e, frame), read_before(e, frame));
                    }
                    assert_eq!(hash, f.sim.state_hash());
                }
            }
            for stage in 0..7 {
                round_means[stage].push(
                    times[stage].iter().sum::<u64>() as f64 / times[stage].len() as f64 / 1e6,
                );
            }
        }
        for (name, raw) in names.into_iter().zip(round_means) {
            let mut sorted = raw.clone();
            sorted.sort_by(f64::total_cmp);
            println!(
                "INTERPOLATION_POSE_STAGE {}",
                serde_json::json!({
                    "world": world, "stage": name, "rounds": 8, "warmup_commits": 8,
                    "measured_commits": 32, "median_ms": (sorted[3] + sorted[4]) * 0.5,
                    "round_means_ms": raw, "all_pose_fields_equal": true,
                    "read_only_world_hash_equal": true,
                })
            );
        }
    }
}
