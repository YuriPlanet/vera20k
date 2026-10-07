//! Bounded original GetSpeed -> FireAt -> ordinary motion -> retained draw join.
//!
//! Upstream FLH/target coordinates and admitted collision continuations are the
//! explicit native fixture boundaries in tools/projectile_oracle/bridge_render.md.
//! The expected trajectory/draw values come only from the executed native corpus.

use super::{geometry, ground_probe, projection_admitted};
use crate::map::resolved_terrain::test_flat_ground_grid;
use crate::rules::{art_data::ArtRegistry, ruleset::RuleSet};
use crate::sim::projectile::{
    ProjectileCollisionPhase, ProjectileCollisionPolicy, ProjectileCoord, ProjectilePayload,
    ProjectileSpawn, ProjectileTarget, ProjectileTrajectory, ProjectileVisualState,
    TargetExpiryPolicy, launch, projectile_gravity, projectile_shp_frame,
};
use crate::sim::world::Simulation;
use crate::util::native_x87::NativeF64Bits;
use serde_json::Value;
use std::collections::BTreeSet;

fn coord(value: &Value) -> ProjectileCoord {
    ProjectileCoord::new(
        value[0].as_i64().unwrap() as i32,
        value[1].as_i64().unwrap() as i32,
        value[2].as_i64().unwrap() as i32,
    )
}

fn bits(value: &Value) -> [u64; 3] {
    std::array::from_fn(|axis| u64::from_str_radix(value[axis].as_str().unwrap(), 16).unwrap())
}

fn native_bits(values: [NativeF64Bits; 3]) -> [u64; 3] {
    values.map(|value| value.bits())
}

#[test]
fn original_cannon_launch_motion_and_live_bridge_draw_form_one_production_chain() {
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    if crate::map::retail_trig::verified_math_tables_or_skip("native flight comparison").is_none() {
        return;
    }
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/bridge_render_flight.json",
    ))
    .unwrap();
    let source = &corpus["launch"];
    let source_type = &source["native_type"];
    let unit = rules.object("MTNK").unwrap();
    let weapon = rules.weapon(unit.primary.as_deref().unwrap()).unwrap();
    let kind = rules
        .projectile(weapon.projectile.as_deref().unwrap())
        .unwrap();
    assert_eq!(weapon.id, "105mm");
    assert_eq!(kind.id, "Cannon");
    assert_eq!(kind.image.as_deref(), source_type["image"].as_str());
    // ROT0 GetSpeed uses current gravity/distance, independently of the
    // retained postpass speed. Native speed/postpass evidence is pinned in
    // tools/rules_oracle/weapon_speed{,_order}.

    assert_eq!(
        i64::from(rules.general.gravity),
        source["gravity"].as_i64().unwrap()
    );
    assert_eq!(kind.floater, source["floater"].as_bool().unwrap());
    for (name, actual) in [
        ("arcing", kind.arcing),
        ("shadow", kind.shadow),
        ("inverse_rotates", kind.rotates),
        ("flat", kind.flat),
        ("inviso", kind.inviso),
        ("voxel", kind.voxel),
    ] {
        assert_eq!(
            actual,
            source_type[name].as_bool().unwrap(),
            "native type {name}"
        );
    }
    assert_eq!(kind.rot, 0);
    assert!(!kind.vertical && !kind.inaccurate && !kind.inviso && !kind.voxel);

    let origin = coord(&source["origin"]);
    let target = coord(&source["target"]);
    let launch_speed = launch::weapon_launch_speed(
        weapon.speed,
        Some(launch::LaunchSpeedProjectile {
            rot: kind.rot,
            floater: kind.floater,
        }),
        rules.general.gravity,
        launch::fireat_launch_distance(origin, target),
    );
    assert_eq!(
        i64::from(launch_speed),
        source["launch_speed"].as_i64().unwrap()
    );
    let launched = launch::fireat_launch(launch::FireAtLaunch {
        homing: false,
        delta: ProjectileCoord::new(
            target.x.wrapping_sub(origin.x),
            target.y.wrapping_sub(origin.y),
            target.z.wrapping_sub(origin.z),
        ),
        speed: launch_speed,
        vertical: kind.vertical,
        heading: None,
        arcing: kind.arcing,
        gravity: projectile_gravity(rules.general.gravity, kind.floater),
        high_root: launch::high_arc_root(weapon.lobber, origin, Some(target)),
        voxel_downward: None,
        building_pitch_height: None,
    })
    .expect("original selected launch succeeds");
    assert_eq!(
        native_bits(launched.velocity.native()),
        bits(&source["velocity_bits"])
    );

    let rows = corpus["rows"].as_array().unwrap();
    let frames = corpus["motion"]["frames"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    assert_eq!(frames.len(), rows.len());
    let cells: BTreeSet<_> = rows
        .iter()
        .map(|row| {
            let cell = &row["input"]["mapped_cell"];
            (
                cell[0].as_u64().unwrap() as u16,
                cell[1].as_u64().unwrap() as u16,
            )
        })
        .collect();
    let initial = &rows[0]["input"];
    let mut sim = Simulation::with_seed(31);
    let mut terrain = test_flat_ground_grid(32);
    terrain.test_set_native_allocated_cells(&cells.iter().copied().collect::<Vec<_>>());
    for &(x, y) in &cells {
        let cell = terrain.cell_mut(x, y).unwrap();
        cell.level = initial["level"].as_u64().unwrap() as u8;
        cell.bridge_facts.raw_flags = initial["flags"].as_u64().unwrap() as u32;
    }
    sim.install_resolved_terrain_for_new_map(terrain);
    let weapon_id = sim.interner.intern(&weapon.id);
    let warhead_id = sim.interner.intern(weapon.warhead.as_deref().unwrap());
    let id = sim.allocate_stable_id();
    sim.projectiles.spawn(
        id,
        ProjectileSpawn {
            native_unique_id: 0,
            line_trail: None,
            flat: kind.flat,
            source_id: 7,
            origin,
            target: ProjectileTarget::Entity(42),
            initial_target_position: target,
            payload: ProjectilePayload::new(weapon.damage, warhead_id, weapon_id),
            speed_leptons_per_frame: launched.speed as u16,
            velocity: launched.velocity,
            trajectory: ProjectileTrajectory::Ballistic,
            guidance: None,
            visual: ProjectileVisualState::new(
                kind.anim_low as u8,
                kind.anim_high as u8,
                kind.anim_rate as u8,
            ),
            arm_frames: kind.arm,
            fuse_frames: None,
            ranged_fuse: kind.rot > 0 || kind.ranged,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::DetonateAtLastKnown,
            // World collision decisions are the oracle's admitted-continuation
            // boundary. Only the motion arm's real type inputs are needed here.
            collision: ProjectileCollisionPolicy {
                floater: kind.floater,
                arcing: kind.arcing,
                ..ProjectileCollisionPolicy::NONE
            },
        },
    );
    assert!(sim.register_projectile(id, kind.flat));
    assert_eq!(
        sim.projectiles.get(id).unwrap().position,
        coord(&source["retained_xyz"])
    );

    for (row, frame) in rows.iter().zip(frames) {
        let input = &row["input"];
        let tick = row["tick"].as_u64().unwrap() as u32;
        let flags = input["flags"].as_u64().unwrap() as u32;
        // These are supplied completed bridge transitions, just like the native
        // fixture. Changing the actual retained terrain exercises the live probe.
        let terrain = sim.resolved_terrain.as_mut().unwrap();
        for &(x, y) in &cells {
            terrain.cell_mut(x, y).unwrap().bridge_facts.raw_flags = flags;
        }
        let terrain = sim.resolved_terrain.as_ref().unwrap();
        let mut ordinary_visits = 0;
        let advanced = sim
            .projectiles
            .advance_one(
                id,
                tick,
                |target_id| (target_id == 42).then_some(target),
                Some(terrain),
                &terrain.shared_cell_dummy(),
                rules.general.gravity,
                false,
                false,
                rules.general.safety_altitude,
                |_, candidate, phase| {
                    if let ProjectileCollisionPhase::Ordinary { motion, .. } = phase {
                        ordinary_visits += 1;
                        assert_eq!(
                            candidate,
                            coord(&frame["candidate"]),
                            "tick {tick} candidate"
                        );
                        assert_eq!(
                            native_bits(motion.candidate),
                            bits(&frame["candidate_bits"]),
                            "tick {tick} candidate bits"
                        );
                        assert_eq!(
                            native_bits(motion.velocity),
                            bits(&frame["bits"]),
                            "tick {tick} velocity bits"
                        );
                    }
                    None
                },
            )
            .unwrap();
        assert_eq!(ordinary_visits, 1);
        assert!(advanced.detonations.is_empty() && advanced.expired.is_empty());
        let projectile = sim.projectiles.get(id).unwrap();
        assert_eq!(
            projectile.position,
            coord(&row["xyz"]),
            "tick {tick} retained position"
        );
        assert_eq!(
            native_bits(projectile.velocity.native()),
            bits(&row["velocity_bits"])
        );
        assert_eq!(
            [projectile.position.x / 256, projectile.position.y / 256],
            [
                input["mapped_cell"][0].as_i64().unwrap() as i32,
                input["mapped_cell"][1].as_i64().unwrap() as i32
            ],
            "tick {tick} current mapped cell",
        );
        terrain.shared_cell_dummy().stamp_coord(123, -44);
        let state_before = sim.state_hash();
        let dummy_before = terrain.shared_cell_dummy().snapshot();
        let (ground, structural) = ground_probe(terrain, projectile.position);
        assert_eq!(
            structural,
            flags & 0x100 != 0,
            "tick {tick} live structural flag"
        );
        let actual = geometry(
            projectile.position,
            ground,
            structural,
            projectile.on_bridge,
            kind.shadow,
        );
        let camera = [
            input["camera"][0].as_i64().unwrap() as f32,
            input["camera"][1].as_i64().unwrap() as f32 + 15.0,
        ];
        assert_eq!(
            projection_admitted(actual.body.point, camera, [800.0, 600.0]),
            row["projection_visible"] == 1
        );
        let draws = row["draws"].as_array().unwrap();
        let pieces: Vec<_> = actual
            .shadow
            .into_iter()
            .chain(std::iter::once(actual.body))
            .collect();
        assert_eq!(
            pieces.len(),
            draws.len(),
            "tick {tick} body/shadow admission"
        );
        for (piece, draw) in pieces.iter().zip(draws) {
            assert_eq!(
                [piece.point[0] - camera[0], piece.point[1] - camera[1]],
                [
                    draw["point"][0].as_i64().unwrap() as f32,
                    draw["point"][1].as_i64().unwrap() as f32
                ],
                "tick {tick} projected draw point",
            );
            assert_eq!(
                i64::from(piece.z_adjust),
                draw["z_adjust"].as_i64().unwrap(),
                "tick {tick} draw Z-adjust"
            );
            assert_eq!(
                u64::from(projectile_shp_frame(projectile, kind)),
                draw["frame"].as_u64().unwrap()
            );
        }
        assert_eq!(
            sim.state_hash(),
            state_before,
            "tick {tick} presentation changed simulation"
        );
        assert_eq!(
            terrain.shared_cell_dummy().snapshot(),
            dummy_before,
            "tick {tick} presentation changed dummy state"
        );
    }
}

#[test]
fn original_ifv_dragon_frame_getter_matches_retained_flights_and_all_directions() {
    let Some((rules_ini, art_ini)) = crate::rules::retail_ini_fixture::retail_rules_and_art()
    else {
        return;
    };
    let mut rules = RuleSet::from_ini_with_fixed_art_for_test(&rules_ini, &art_ini).unwrap();
    rules.install_art_data(ArtRegistry::from_ini(&art_ini));
    let weapon = rules.weapon("HoverMissile").unwrap();
    let kind = rules
        .projectile(weapon.projectile.as_deref().unwrap())
        .unwrap();
    assert_eq!(kind.id, "AAHeatSeeker2");
    let corpus: Value = serde_json::from_str(crate::test_fixture::text(
        "tools/projectile_oracle/ifv_render.json",
    ))
    .unwrap();
    assert_eq!(
        kind.image.as_deref(),
        corpus["selected_native_type"]["image"].as_str()
    );
    assert!(!kind.rotates && !kind.shadow);
    let mut sim = Simulation::with_seed(31);
    let weapon_id = sim.interner.intern(&weapon.id);
    let warhead = sim.interner.intern(weapon.warhead.as_deref().unwrap());
    let id = sim.allocate_stable_id();
    let rows = corpus["frames"].as_array().unwrap();
    assert_eq!(rows.len(), 166);
    let origin = coord(&rows[0]["position"]);
    sim.admit_projectile(
        id,
        ProjectileSpawn {
            native_unique_id: 0,
            line_trail: None,
            flat: kind.flat,
            source_id: 0,
            origin,
            target: ProjectileTarget::None,
            initial_target_position: origin,
            payload: ProjectilePayload::new(weapon.damage, warhead, weapon_id),
            speed_leptons_per_frame: 0,
            velocity: crate::sim::projectile::ProjectileVelocity::new(0, 0, 0),
            trajectory: ProjectileTrajectory::Straight,
            guidance: None,
            visual: ProjectileVisualState::new(
                kind.anim_low as u8,
                kind.anim_high as u8,
                kind.anim_rate as u8,
            ),
            arm_frames: 0,
            fuse_frames: None,
            ranged_fuse: false,
            tracks_target: false,
            target_expiry: TargetExpiryPolicy::Expire,
            collision: ProjectileCollisionPolicy::NONE,
        },
    );
    let mut directions = BTreeSet::new();
    for row in rows {
        let projectile = sim.projectiles.get_mut(id).unwrap();
        projectile.position = coord(&row["position"]);
        projectile.velocity = crate::sim::projectile::ProjectileVelocity::from_native(
            bits(&row["velocity"]["bits"]).map(NativeF64Bits::from_bits),
        );
        let before = sim.state_hash();
        let frame = projectile_shp_frame(sim.projectiles.get(id).unwrap(), kind);
        assert_eq!(
            u64::from(frame),
            row["native_frame"].as_u64().unwrap(),
            "original468000 frame for {} with velocity{}",
            row["name"],
            row["velocity"]["bits"]
        );
        if row["kind"] == "direction" {
            directions.insert(frame);
        }
        assert_eq!(
            sim.state_hash(),
            before,
            "frame selection changed simulation state"
        );
    }
    assert_eq!(directions.len(), 32);
}
