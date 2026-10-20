//! Oracle for the fields consumed by one SHP draw, not a whole builder/GPU
//! benchmark. The repeated strategy is frozen from before local pose reuse.
use super::*;
use std::hint::black_box;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq)]
struct DrawInputs {
    anchor: [f32; 2],
    // The frame resolver still consumes the high byte of this raw direction.
    direction: Option<u16>,
    gun: Option<(u16, u16, ([f32; 2], bool))>,
}

#[derive(Clone, Copy, Debug)]
enum DrawKind {
    Foot,
    Building,
    BuildingGun,
}

/// The old SHP consumer strategy: 3 reads for a visible Foot, 1 for a plain
/// building, 4 for a building with a voxel gun. Off-screen Foot culling used
/// 2 reads; this probe deliberately describes admitted field consumption only.
#[inline(never)]
fn repeated(
    cache: &RenderInterpolation,
    entity: &GameEntity,
    frame: u32,
    kind: DrawKind,
) -> DrawInputs {
    // Opaque inputs belong at the outer loop boundary, not between these
    // calls: the compiler may eliminate repeated work just as in production.
    let read = || cache.sample_entity(entity, frame);
    let [x, y] = read().screen;
    let anchor = match kind {
        DrawKind::Foot => [x, y - read().visual_lift],
        _ => {
            let (x, y) = crate::render::locomotor_visual::building_art_anchor(x, y);
            [x, y]
        }
    };
    DrawInputs {
        anchor,
        direction: matches!(kind, DrawKind::Foot).then(|| read().body),
        gun: matches!(kind, DrawKind::BuildingGun)
            .then(|| (read().body, read().barrel, read().recoil)),
    }
}

/// Exactly the new consumer strategy: one fresh production sample, then
/// field copies inside the same read-only draw. No new cache/API involved.
#[inline(never)]
fn reused(
    cache: &RenderInterpolation,
    entity: &GameEntity,
    frame: u32,
    kind: DrawKind,
) -> DrawInputs {
    let pose = cache.sample_entity(entity, frame);
    let [x, y] = pose.screen;
    let anchor = match kind {
        DrawKind::Foot => [x, y - pose.visual_lift],
        _ => {
            let (x, y) = crate::render::locomotor_visual::building_art_anchor(x, y);
            [x, y]
        }
    };
    DrawInputs {
        anchor,
        direction: matches!(kind, DrawKind::Foot).then_some(pose.body),
        gun: matches!(kind, DrawKind::BuildingGun).then_some((pose.body, pose.barrel, pose.recoil)),
    }
}

fn assert_inputs(a: DrawInputs, b: DrawInputs) {
    assert_eq!(a, b);
    assert_eq!(a.anchor.map(f32::to_bits), b.anchor.map(f32::to_bits));
    assert_eq!(
        a.gun.map(|g| g.2.0.map(f32::to_bits)),
        b.gun.map(|g| g.2.0.map(f32::to_bits)),
    );
}

fn seeded(entity: &GameEntity, frame: u32, enabled: bool) -> RenderInterpolation {
    let current = RenderPose::read(entity, frame);
    let mut previous = current;
    previous.location[0] -= 64;
    previous.screen[0] -= 7.;
    previous.screen[1] += 3.;
    previous.body = 0xff00;
    previous.barrel = 0x400;
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
fn render_interpolation_shp_local_reuse_matches_complete_consumed_fields() {
    for (category, kind) in [
        (EntityCategory::Infantry, DrawKind::Foot),
        (EntityCategory::Unit, DrawKind::Foot),
        (EntityCategory::Aircraft, DrawKind::Foot),
        (EntityCategory::Structure, DrawKind::Building),
        (EntityCategory::Structure, DrawKind::BuildingGun),
    ] {
        for frame in [0, 1, 17, u32::MAX] {
            let mut entity =
                GameEntity::test_default_of_category(7, "MTNK", "Americans", 4, 4, category);
            entity.set_in_playfield(true);
            entity.lifecycle.set_in_limbo_for_test(false);
            assert!(eligible(&entity));
            for enabled in [false, true] {
                let mut entity = entity.clone();
                let mut cache = seeded(&entity, frame, enabled);
                for held in [false, true] {
                    cache.held = held;
                    for alpha in [-0.0, 0.0, 0.125, 0.5, 0.999, 1.0, 1.5, f32::NAN] {
                        cache.alpha = alpha;
                        assert_inputs(
                            repeated(&cache, &entity, frame, kind),
                            reused(&cache, &entity, frame, kind),
                        );
                    }
                }
                // A later draw must reread a same-frame mutation instead of
                // retrieving the earlier draw's sample from a frame-global cache.
                cache.held = false;
                cache.alpha = 0.5;
                entity.body_facing.snap(0x3456, frame);
                entity.position.set_exact_z_leptons_for_test(Some(600));
                let after = reused(&cache, &entity, frame, kind);
                assert_inputs(repeated(&cache, &entity, frame, kind), after);
                if matches!(kind, DrawKind::Foot) {
                    assert_eq!(after.direction, Some(entity.body_facing_current(frame)));
                }
                for observed_frame in [frame.wrapping_add(1), frame.wrapping_add(3)] {
                    assert_inputs(
                        repeated(&cache, &entity, observed_frame, kind),
                        reused(&cache, &entity, observed_frame, kind),
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "manual Release CPU consumer-strategy probe; not builder, GPU or game FPS"]
fn render_interpolation_shp_local_reuse_cpu_probe() {
    assert!(!cfg!(debug_assertions), "run --release");
    let rules = crate::rules::retail_ini_fixture::retail_unlayered_rules_with_art()
        .expect("retail INIs required");
    for (type_name, kind) in [
        ("E1", DrawKind::Foot),
        ("GAPOWR", DrawKind::Building),
        ("GTGCAN", DrawKind::BuildingGun),
    ] {
        let mut sim = crate::sim::arena_fixture::rival_arena(&rules, 0x20261020);
        sim.intern_rule_type_ids(&rules);
        sim.resolve_type_handles(&rules);
        let id = sim
            .spawn_object(type_name, "Americans", 8, 8, 0, &rules)
            .unwrap();
        let template = sim.entities().get(id).unwrap().clone();
        assert!(eligible(&template));
        // Controlled clones with real retail construction, NOT a legal arena.
        for index in 1..1000 {
            let mut entity = template.clone();
            entity.set_stable_id_for_test(1_000_000 + index);
            sim.entities_mut_for_test().insert(entity);
        }
        let frame = sim.session().binary_frame();
        let hash = sim.state_hash();
        for enabled in [false, true] {
            let mut cache = RenderInterpolation::new(enabled);
            for entity in sim.entities().values() {
                let source = seeded(entity, frame, enabled);
                cache.poses.insert_for_test(
                    entity.stable_id(),
                    *source.poses.get(&entity.stable_id()).unwrap(),
                );
            }
            cache.held = false;
            cache.interval_ms = Some(32.);
            for entity in sim.entities().values() {
                cache.alpha = 0.5;
                assert_inputs(
                    repeated(&cache, entity, frame, kind),
                    reused(&cache, entity, frame, kind),
                );
                if enabled && matches!(kind, DrawKind::Foot) {
                    assert_ne!(
                        reused(&cache, entity, frame, kind).anchor[0],
                        RenderPose::read(entity, frame).screen[0]
                    );
                }
            }
            // Same fixed fixture and executable, both function paths, alternating
            // AB/BA. Warmup/timed loop length and full returned payload match.
            for pair in 0..8 {
                let order = if pair % 2 == 0 {
                    [false, true]
                } else {
                    [true, false]
                };
                for reuse in order {
                    let mut samples = Vec::new();
                    for step in 0..640 {
                        cache.alpha = (step % 32) as f32 / 32.;
                        let start = Instant::now();
                        for entity in sim.entities().values() {
                            let output = if reuse {
                                reused(
                                    black_box(&cache),
                                    black_box(entity),
                                    black_box(frame),
                                    black_box(kind),
                                )
                            } else {
                                repeated(
                                    black_box(&cache),
                                    black_box(entity),
                                    black_box(frame),
                                    black_box(kind),
                                )
                            };
                            black_box(output);
                        }
                        if step >= 128 {
                            samples.push(start.elapsed().as_nanos() as u64);
                        }
                    }
                    assert_eq!(sim.state_hash(), hash, "read-only sampling");
                    let mean =
                        samples.iter().map(|&n| n as f64).sum::<f64>() / samples.len() as f64;
                    println!(
                        "INTERPOLATION_SHP_REUSE {}",
                        serde_json::json!({
                            "schema":1, "type":type_name, "kind":format!("{kind:?}"),
                            "entities":1000, "enabled":enabled, "pair":pair, "reuse":reuse,
                            "warmup":128, "measured":512, "mean_ns":mean, "raw_ns":samples,
                            "world_hash":format!("{hash:016x}"),
                        })
                    );
                }
            }
        }
    }
}
