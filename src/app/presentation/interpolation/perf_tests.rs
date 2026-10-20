//! Opt-in paired CPU mechanism probe, NOT an end-to-end game/FPS benchmark.
//! Run optimized, alone, with `--ignored --exact --nocapture --test-threads=1`.
//! It exercises the production cache and sampling API against identical,
//! controlled committed transforms. It does not run AI, pathfinding, GPU draws,
//! VSync or OS presentation. World mutation, hashes and fixture setup are untimed.
use super::*;
use std::hint::black_box;
use std::time::Instant;

#[path = "pose_read_tests.rs"]
mod pose_read_tests;

const WARMUP: u32 = 128;
const MEASURED: u32 = 768;
const PAIRS: usize = 8;

#[derive(Clone, Copy, serde::Serialize)]
struct Case {
    name: &'static str,
    world: usize,
    visible: usize,
    render_ms: u32,
    logic_ms: u32,
    bypass: bool,
    smoke_systems: usize,
}

struct Fixture {
    sim: Simulation,
    visible_ids: Vec<u64>,
    points: Vec<(PointKind, u64, PointPose)>,
    base_frame: u32,
}

fn fixture(rules: &crate::rules::ruleset::RuleSet, case: Case) -> Fixture {
    let mut sim = crate::sim::arena_fixture::rival_arena(rules, 0x20261020);
    sim.intern_rule_type_ids(rules);
    sim.resolve_type_handles(rules);
    let id = sim
        .spawn_object("MTNK", "Americans", 8, 8, 0, rules)
        .unwrap();
    if case.smoke_systems > 0 {
        let ty = rules.ps_type_id_by_name("SmallGreySSys").unwrap();
        for _ in 0..case.smoke_systems {
            sim.spawn_particle_system(
                ty,
                glam::IVec3::new(3200, 3200, 1040),
                None,
                None,
                glam::IVec3::ZERO,
                None,
                rules,
            )
            .unwrap();
        }
        for _ in 0..40 {
            sim.advance_tick(&[], Some(rules), None, None, 33);
        }
    }
    // Real retail-constructed tank state, cloned with distinct cache identities.
    // Not a legal 20k-tank arena: these are controlled read-only pose inputs,
    // not a battle fixture or an admission/pathfinding scale claim.
    let template = sim.entities().get(id).unwrap().clone();
    for index in 1..case.world {
        let mut entity = template.clone();
        entity.set_stable_id_for_test(1_000_000 + index as u64);
        sim.entities_mut_for_test().insert(entity);
    }
    assert_eq!(sim.entities().len(), case.world);
    assert!(sim.entities().values().all(eligible));
    let visible_ids = sim
        .entities()
        .values()
        .take(case.visible)
        .map(GameEntity::stable_id)
        .collect();
    let mut points = Vec::new();
    for (&id, system) in sim.particle_systems().iter().filter(|(_, s)| !s.in_limbo()) {
        points.push((
            PointKind::System,
            id,
            PointPose::new(
                system.coords().to_array(),
                system.type_id().0,
                system.owner_entity(),
            ),
        ));
    }
    for (_, particle) in sim.particle_systems().display_particles() {
        points.push((
            PointKind::Particle,
            particle.stable_id(),
            PointPose::new(particle.coords().to_array(), particle.type_id().0, None),
        ));
    }
    if case.smoke_systems > 0 {
        assert!(
            points
                .iter()
                .any(|(kind, _, _)| *kind == PointKind::Particle)
        );
    }
    let base_frame = sim.session().binary_frame();
    Fixture {
        sim,
        visible_ids,
        points,
        base_frame,
    }
}

fn commit(f: &mut Fixture, tick: u32) {
    let frame = f.base_frame + tick + 1;
    f.sim
        .session_mut_for_test()
        .set_binary_frame_for_test(frame);
    // Small continuous moves and real body/turret facing fields. No interpolation
    // producer is mocked or replaced; only the authoritative input is controlled.
    let phase = tick % 64;
    // Sixteen leptons also changes the renderer's integer-pixel projection;
    // a four-lepton step can land on the same pixel in the half-phase audit.
    let shift = (if phase <= 32 { phase } else { 64 - phase }) as i32 * 16;
    for (index, entity) in f.sim.entities_mut_for_test().values_mut().enumerate() {
        entity
            .position
            .put_location(crate::sim::components::DriveCoord {
                x: (index % 32) as i32 * 256 + 128 + shift,
                y: ((index / 32) % 32) as i32 * 256 + 128,
                z: 0,
            });
        entity
            .body_facing
            .snap((tick as u16).wrapping_mul(173), frame);
        if let Some(turret) = entity.barrel_facing.as_mut() {
            turret.snap((tick as u16).wrapping_mul(251), frame);
        }
    }
}

fn sampling(cache: &RenderInterpolation, f: &Fixture) {
    let frame = f.sim.session().binary_frame();
    for (index, &id) in f.visible_ids.iter().enumerate() {
        let entity = f.sim.entities().get(id).unwrap();
        // A fixed declared bundle, not a claim about each real game draw's
        // call count: body + status offset + attached coordinate per object.
        black_box(cache.sample_entity(black_box(entity), black_box(frame)));
        black_box(cache.entity_offset(black_box(entity), black_box(frame)));
        black_box(cache.entity_coord(black_box(entity), black_box(frame), [2048, 2048, 128]));
        // One lazy geometry endpoint per four visible objects (e.g. a link).
        if index % 4 == 0 {
            let c = crate::sim::movement::ground_pose::object_location(entity, None);
            black_box(cache.sample_draw_point(
                PointKind::ControlSource,
                id,
                PointPose::new([c.x, c.y, c.z], entity.type_ref().index(), Some(id)),
                frame,
            ));
        }
    }
    // Frozen, real retail Smoke children test the capture and point-sampling
    // cost. Their physics and input-pose construction are deliberately untimed.
    for &(kind, id, pose) in &f.points {
        black_box(cache.sample_point(black_box(kind), black_box(id), black_box(pose), frame));
    }
}

fn stats(samples: &[u64]) -> serde_json::Value {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let n = sorted.len();
    serde_json::json!({
        "n": n,
        "mean_ns": samples.iter().map(|&n| n as f64).sum::<f64>() / n as f64,
        "median_ns": sorted[n / 2],
        "p95_ns": sorted[(n * 95).div_ceil(100).saturating_sub(1)],
        "max_ns": sorted[n - 1],
    })
}

fn run(rules: &crate::rules::ruleset::RuleSet, case: Case, pair: usize, enabled: bool) -> u64 {
    let mut f = fixture(rules, case);
    let mut cache = RenderInterpolation::new(enabled);
    let interval = (!case.bypass).then_some(case.logic_ms as f32);
    let mut last_tick = None;
    let mut rows = Vec::with_capacity(MEASURED as usize);
    let mut capture = Vec::new();
    let mut repeat = Vec::new();
    let mut sample_times = Vec::new();
    let mut total_times = Vec::new();
    for step in 0..WARMUP + MEASURED {
        let now_ms = step * case.render_ms;
        let tick = now_ms / case.logic_ms;
        let changed = last_tick != Some(tick);
        if changed {
            commit(&mut f, tick);
            last_tick = Some(tick);
        }
        let audit = step == 0 || step == WARMUP + MEASURED - 1;
        let hash = audit.then(|| f.sim.state_hash());
        let start = Instant::now();
        cache.observe(
            black_box(&f.sim),
            black_box(now_ms),
            black_box(interval),
            false,
        );
        let after_observe = Instant::now();
        sampling(black_box(&cache), black_box(&f));
        let end = Instant::now();
        if let Some(hash) = hash {
            assert_eq!(hash, f.sim.state_hash(), "read-only hooks");
        }
        if step >= WARMUP {
            let observe_ns = after_observe.duration_since(start).as_nanos() as u64;
            let sample_ns = end.duration_since(after_observe).as_nanos() as u64;
            let total_ns = end.duration_since(start).as_nanos() as u64;
            if changed {
                capture.push(observe_ns);
            } else {
                repeat.push(observe_ns);
            }
            sample_times.push(sample_ns);
            total_times.push(total_ns);
            rows.push([
                u64::from(step),
                u64::from(changed),
                observe_ns,
                sample_ns,
                total_ns,
            ]);
        }
    }
    assert_eq!(
        cache.poses.len(),
        if enabled && !case.bypass {
            case.world
        } else {
            0
        }
    );
    // Prove the timed ON path is not silently snapping/bypassed. Separate from
    // the timing window, test a genuine new committed endpoint at half phase.
    let tick = last_tick.unwrap() + 1;
    commit(&mut f, tick);
    let now = tick * case.logic_ms;
    cache.observe(&f.sim, now, interval, false);
    cache.observe(&f.sim, now + case.logic_ms / 2, interval, false);
    let entity = f.sim.entities().get(f.visible_ids[0]).unwrap();
    let current = RenderPose::read(entity, f.sim.session().binary_frame());
    let sampled = cache.sample_entity(entity, f.sim.session().binary_frame());
    if enabled && !case.bypass {
        assert_eq!(cache.alpha, 0.5);
        let endpoints = cache.poses.get(&entity.stable_id()).unwrap();
        assert_ne!(endpoints.previous.location, endpoints.current.location);
        assert_ne!(sampled, current);
        assert_ne!(sampled.screen, current.screen);
    } else {
        assert_eq!(sampled, current);
    }
    let hash = f.sim.state_hash();
    println!(
        "INTERPOLATION_PERF {}",
        serde_json::json!({
            "schema": 1, "case": case, "pair": pair, "enabled": enabled,
            "warmup_frames": WARMUP, "measured_frames": MEASURED,
            "final_hash": format!("{hash:016x}"), "point_samples_per_frame": f.points.len(),
            "observe_commit": stats(&capture),
            "observe_repeat": if repeat.is_empty() { serde_json::Value::Null } else { stats(&repeat) },
            "sampling": stats(&sample_times), "total": stats(&total_times),
            "raw_columns": ["render_step", "commit", "observe_ns", "sampling_ns", "total_ns"],
            "raw": rows,
        })
    );
    hash
}

#[test]
#[ignore = "manual paired CPU timing; requires retail INIs and an optimized quiet host"]
fn render_interpolation_paired_cpu_probe() {
    assert!(
        !cfg!(debug_assertions),
        "use cargo test --release, not an opt0 test build"
    );
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        .expect("retail INI input is required; never skip this benchmark");
    let base = Case {
        name: "world1000_visible250_200fps_31tps",
        world: 1000,
        visible: 250,
        render_ms: 5,
        logic_ms: 32,
        bypass: false,
        smoke_systems: 0,
    };
    let cases = [
        Case {
            name: "world100_visible100_200fps_31tps",
            world: 100,
            visible: 100,
            ..base
        },
        base,
        Case {
            name: "world5000_visible500_200fps_31tps",
            world: 5000,
            visible: 500,
            ..base
        },
        Case {
            name: "world20000_visible500_200fps_31tps",
            world: 20000,
            visible: 500,
            ..base
        },
        Case {
            name: "world1000_visible1000_200fps_31tps",
            visible: 1000,
            ..base
        },
        Case {
            name: "world1000_visible250_62fps_62tps",
            render_ms: 16,
            logic_ms: 16,
            ..base
        },
        Case {
            name: "world1000_visible250_200fps_62tps",
            logic_ms: 16,
            ..base
        },
        Case {
            name: "world1000_visible250_200fps_15tps",
            logic_ms: 64,
            ..base
        },
        Case {
            name: "world1000_visible250_200fps_bypass",
            bypass: true,
            ..base
        },
        Case {
            name: "world1000_visible250_smoke64_200fps_31tps",
            smoke_systems: 64,
            ..base
        },
    ];
    for case in cases {
        for pair in 0..PAIRS {
            // Alternating AB / BA controls first-run and thermal/order bias.
            let order = if pair % 2 == 0 {
                [false, true]
            } else {
                [true, false]
            };
            let a = run(&rules, case, pair, order[0]);
            let b = run(&rules, case, pair, order[1]);
            assert_eq!(
                a, b,
                "same committed input and final simulation state for ON/OFF"
            );
        }
    }
}
