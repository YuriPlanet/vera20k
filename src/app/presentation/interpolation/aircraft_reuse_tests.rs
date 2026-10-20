//! Read-only aircraft consumer-strategy equivalence and isolated CPU timing.
//! The oracle uses the old, still-independent production sampling interfaces.
use super::*;
use std::hint::black_box;
use std::time::Instant;

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
    assert_eq!(bits(a), bits(b));
}

fn seeded(entity: &GameEntity, frame: u32, enabled: bool) -> RenderInterpolation {
    let current = RenderPose::read(entity, frame);
    let mut previous = current;
    previous.location = current.location.map(|v| v.saturating_sub(3));
    previous.screen[0] -= 7.5;
    previous.screen[1] += 3.25;
    previous.body = 0xff00;
    previous.turret = 0xf000;
    previous.barrel = 0x400;
    previous.angles = [3.1, -3.1];
    previous.rocket_pitch = 0.25;
    previous.visual_lift = -8.;
    previous.lift += 4.;
    previous.recoil = ([1.25, -0.75], true);
    let mut cache = RenderInterpolation::new(enabled);
    cache.record(entity.stable_id(), previous, frame.wrapping_sub(1), true);
    cache.record(entity.stable_id(), current, frame, false);
    cache.interval_ms = Some(32.);
    cache.held = false;
    cache
}

#[test]
fn render_interpolation_aircraft_joint_sample_matches_independent_pose_and_coord() {
    let mut checked = 0;
    for frame in [0, 1, 17, u32::MAX] {
        for height in [None, Some(-104), Some(0), Some(727), Some(1500)] {
            for bridge in [false, true] {
                for history in 0..5 {
                    let mut entity = GameEntity::test_default_of_category(
                        7,
                        "ORCA",
                        "Americans",
                        4,
                        4,
                        EntityCategory::Aircraft,
                    );
                    entity.lifecycle.set_in_limbo_for_test(false);
                    entity.position.set_exact_z_leptons_for_test(height);
                    entity.set_on_bridge(bridge);
                    entity.barrel_facing = Some(crate::sim::movement::FacingClass::new(0x1000, 4));
                    for enabled in [false, true] {
                        let mut entity = entity.clone();
                        let mut cache = seeded(&entity, frame, enabled);
                        match history {
                            0 => cache.poses.clear(),
                            2 => cache.poses.insert_for_test(
                                7,
                                Endpoints {
                                    frame: frame.wrapping_sub(1),
                                    ..*cache.poses.get(&7).unwrap()
                                },
                            ),
                            // Eligibility gates poses, but historically NOT
                            // entity_coord. Even that unusual combination stays.
                            3 => entity.lifecycle.set_in_limbo_for_test(true),
                            4 => {
                                entity.body_facing.snap(0x3456, frame);
                            }
                            _ => {}
                        }
                        for held in [false, true] {
                            cache.held = held;
                            for interval in [None, Some(32.)] {
                                cache.interval_ms = interval;
                                for alpha in [-0.5, -0.0, 0., 0.125, 0.5, 0.999, 1., 1.5, f32::NAN]
                                {
                                    cache.alpha = alpha;
                                    for coord in [
                                        [0; 3],
                                        [1234, 5678, -104],
                                        [i32::MIN, i32::MAX, -17],
                                        [2048, 2048, 1500],
                                    ] {
                                        let (pose, xyz) =
                                            cache.sample_entity_and_coord(&entity, frame, coord);
                                        assert_pose_bits(pose, cache.sample_entity(&entity, frame));
                                        assert_eq!(xyz, cache.entity_coord(&entity, frame, coord));
                                        checked += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 57_600);
}

#[test]
fn render_interpolation_aircraft_joint_sample_preserves_native_shadow_corpus() {
    use crate::render::aircraft_shadow::{self, AircraftShadowContext};
    use crate::render::foot_depth::DepthCell;
    use crate::rules::locomotor_type::LocomotorKind;
    let fixture: serde_json::Value = serde_json::from_str(crate::test_fixture::text(
        "tools/voxel_oracle/aircraft_shadow.json",
    ))
    .unwrap();
    for case in fixture["callers"].as_array().unwrap() {
        let input = &case["input"];
        let coord: [i32; 3] = std::array::from_fn(|i| input["xyz"][i].as_i64().unwrap() as i32);
        let at = [(coord[0] / 256) as i16, (coord[1] / 256) as i16];
        let mut entity = GameEntity::test_default_of_category(
            7,
            "ORCA",
            "Americans",
            4,
            4,
            EntityCategory::Aircraft,
        );
        entity.lifecycle.set_in_limbo_for_test(false);
        entity
            .position
            .put_location(crate::sim::components::DriveCoord {
                x: coord[0],
                y: coord[1],
                z: coord[2],
            });
        entity.set_on_bridge(input["on_bridge"].as_bool().unwrap_or(false));
        entity
            .body_facing
            .snap(input["primary"].as_u64().unwrap() as u16, 17);
        entity.barrel_facing = Some(crate::sim::movement::FacingClass::new(
            input["secondary"].as_u64().unwrap() as u16,
            4,
        ));
        for enabled in [false, true] {
            let mut cache = seeded(&entity, 17, enabled);
            for alpha in [0., 0.125, 0.5, 0.999, 1.] {
                cache.alpha = alpha;
                let old_pose = cache.sample_entity(&entity, 17);
                let old_coord = cache.entity_coord(&entity, 17, coord);
                let (new_pose, new_coord) = cache.sample_entity_and_coord(&entity, 17, coord);
                assert_pose_bits(old_pose, new_pose);
                let shadow = |pose: RenderPose, xyz| {
                    aircraft_shadow::pose(
                        AircraftShadowContext {
                            xyz,
                            on_bridge: entity.on_bridge(),
                            locomotor: if input["locomotor"] == "Fly" {
                                LocomotorKind::Fly
                            } else {
                                LocomotorKind::Rocket
                            },
                            is_dropship: input["dropship"].as_bool().unwrap_or(false),
                            primary_facing: pose.body,
                            secondary_facing: pose.turret,
                        },
                        |coord| DepthCell {
                            coord,
                            level: if coord == at {
                                input["level"].as_i64().unwrap_or(0) as i8
                            } else {
                                0
                            },
                            ramp: if coord == at {
                                input["slope"].as_u64().unwrap_or(0) as u8
                            } else {
                                0
                            },
                            flags: input[if coord == at {
                                "flags"
                            } else {
                                "neighbor_flags"
                            }]
                            .as_u64()
                            .unwrap_or(0) as u32,
                            ..Default::default()
                        },
                    )
                    .map(|s| (s.anchor.map(f32::to_bits), s.world_z, s.facing, s.slope))
                };
                let before = shadow(old_pose, old_coord);
                let after = shadow(new_pose, new_coord);
                assert_eq!(before, after, "complete shadow output {input}");
                if !enabled || alpha == 1. {
                    let (anchor, world_z, _, _) = after.unwrap();
                    let anchor = anchor.map(f32::from_bits);
                    assert_eq!(
                        [anchor[0] + 320., anchor[1] - 215.],
                        std::array::from_fn::<_, 2, _>(
                            |i| case["anchor"][i].as_i64().unwrap() as f32
                        )
                    );
                    assert_eq!(world_z, case["temporary_xyz"][2].as_i64().unwrap() as i32);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    type_name: &'static str,
    height: i32,
    shadow: bool,
    body: bool,
}

#[derive(Clone, Copy)]
struct DrawInputs {
    shadow: Option<RenderPose>,
    coord: [i32; 3],
    body: Option<RenderPose>,
}

#[inline(never)]
fn repeated(
    cache: &RenderInterpolation,
    entity: &GameEntity,
    frame: u32,
    coord: [i32; 3],
    case: Case,
) -> DrawInputs {
    // No opaque barrier between old reads: allow normal compiler elimination.
    DrawInputs {
        shadow: case.shadow.then(|| cache.sample_entity(entity, frame)),
        coord: if case.shadow {
            cache.entity_coord(entity, frame, coord)
        } else {
            coord
        },
        body: case.body.then(|| cache.sample_entity(entity, frame)),
    }
}

#[inline(never)]
fn joint(
    cache: &RenderInterpolation,
    entity: &GameEntity,
    frame: u32,
    coord: [i32; 3],
    case: Case,
) -> DrawInputs {
    let shared = case
        .shadow
        .then(|| cache.sample_entity_and_coord(entity, frame, coord));
    DrawInputs {
        shadow: shared.map(|s| s.0),
        coord: shared.map_or(coord, |s| s.1),
        body: case
            .body
            .then(|| shared.map_or_else(|| cache.sample_entity(entity, frame), |s| s.0)),
    }
}

#[test]
#[ignore = "manual Release aircraft consumer CPU probe; excludes builder, GPU and game FPS"]
fn render_interpolation_aircraft_joint_sample_cpu_probe() {
    assert!(!cfg!(debug_assertions), "run --release");
    let rules =
        crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art().expect("retail INIs");
    for case in [
        Case {
            name: "orca_flight",
            type_name: "ORCA",
            height: 1500,
            shadow: true,
            body: true,
        },
        Case {
            name: "orca_ground",
            type_name: "ORCA",
            height: 0,
            shadow: true,
            body: true,
        },
        Case {
            name: "orca_culled_body",
            type_name: "ORCA",
            height: 1500,
            shadow: true,
            body: false,
        },
        Case {
            name: "v3rocket_flight",
            type_name: "V3ROCKET",
            height: 1500,
            shadow: true,
            body: true,
        },
        Case {
            name: "mtnk_no_shadow_control",
            type_name: "MTNK",
            height: 0,
            shadow: false,
            body: true,
        },
    ] {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x20261020);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let id = sim
            .spawn_object(case.type_name, "Americans", 8, 8, 0, &rules)
            .unwrap();
        sim.entities_mut_for_test()
            .get_mut(id)
            .unwrap()
            .position
            .set_exact_z_leptons_for_test(Some(case.height));
        let template = sim.entities().get(id).unwrap().clone();
        assert!(eligible(&template));
        for index in 1..1000 {
            let mut entity = template.clone();
            entity.set_stable_id_for_test(1_000_000 + index);
            sim.entities_mut_for_test().insert(entity);
        }
        let frame = sim.session().binary_frame();
        let hash = sim.state_hash();
        let coord = [2048, 2048, case.height];
        for enabled in [false, true] {
            let mut cache = RenderInterpolation::new(enabled);
            for entity in sim.entities().values() {
                let source = seeded(entity, frame, enabled);
                cache.poses.insert_for_test(
                    entity.stable_id(),
                    *source.poses.get(&entity.stable_id()).unwrap(),
                );
            }
            cache.interval_ms = Some(32.);
            cache.held = false;
            cache.alpha = 0.5;
            for entity in sim.entities().values() {
                let a = repeated(&cache, entity, frame, coord, case);
                let b = joint(&cache, entity, frame, coord, case);
                assert_eq!(a.coord, b.coord);
                for (a, b) in [(a.shadow, b.shadow), (a.body, b.body)] {
                    assert_eq!(a.is_some(), b.is_some());
                    if let (Some(a), Some(b)) = (a, b) {
                        assert_pose_bits(a, b);
                    }
                }
                if enabled && case.shadow {
                    assert_ne!(b.coord, coord, "probe really interpolates");
                }
            }
            for pair in 0..8 {
                for reuse in if pair % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                } {
                    let mut samples = Vec::new();
                    for step in 0..640 {
                        cache.alpha = (step % 32) as f32 / 32.;
                        let start = Instant::now();
                        for entity in sim.entities().values() {
                            let input = (
                                black_box(&cache),
                                black_box(entity),
                                black_box(frame),
                                black_box(coord),
                                black_box(case),
                            );
                            black_box(if reuse {
                                joint(input.0, input.1, input.2, input.3, input.4)
                            } else {
                                repeated(input.0, input.1, input.2, input.3, input.4)
                            });
                        }
                        if step >= 128 {
                            samples.push(start.elapsed().as_nanos() as u64);
                        }
                    }
                    assert_eq!(sim.state_hash(), hash);
                    println!(
                        "INTERPOLATION_AIR_REUSE {}",
                        serde_json::json!({
                            "schema":1,"case":case.name,"type":case.type_name,"entities":1000,
                            "enabled":enabled,"reuse":reuse,"pair":pair,"warmup":128,"measured":512,
                            "mean_ns":samples.iter().map(|&n|n as f64).sum::<f64>()/samples.len() as f64,
                            "raw_ns":samples,"world_hash":format!("{hash:016x}"),
                        })
                    );
                }
            }
        }
    }
}
